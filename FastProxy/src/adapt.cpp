/*
 * adapt.cpp — 协议适配模式：把私有 chat 协议包装成标准 API。
 *
 * 模式命令：
 *   fastproxy adapt --listen :8080 --upstream cats.renchengzhang.com:443 --tls
 *
 * 客户端路径分流：
 *   /v1/chat/completions  → OpenAI 标准
 *   /v1/messages          → Anthropic 标准
 *
 * 流水线：
 *   客户端标准请求 ──parse──▶ IR ──(to_upstream_body 改写 body/路径)──▶ 上游
 *   上游 {reply} 响应 ──(to_standard_body)──▶ 标准 API 响应 ──generate──▶ 客户端
 *
 * 上游连接支持明文与 TLS（--tls 时走 OpenSSL）。
 */
#include <cstdio>
#include <cstring>
#include <poll.h>
#include <string>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>
#include <vector>

#include "adapter.h"
#include "generator.h"
#include "ir.h"
#include "net.h"
#include "parser.h"
#include "tls.h"

namespace fp {

namespace {

// 上游连接抽象：统一明文 fd 与 TLS 句柄的读写。
struct Upstream {
    int fd = -1;
    TlsClient* tls = nullptr;

    ssize_t read(char* buf, size_t n) {
        return tls ? tls_read(tls, buf, n) : ::read(fd, buf, n);
    }
    int write(const char* buf, size_t n) {
        return tls ? tls_write(tls, buf, n) : write_full(fd, buf, n);
    }
    void close() {
        if (tls) { tls_close(tls); tls = nullptr; }
        else if (fd >= 0) { ::close(fd); fd = -1; }
    }
};

// 从 fd（客户端侧，明文）读取完整请求：头段 + Content-Length 正文。
std::string read_full_client_request(int fd, const std::string& head, const std::string& extra,
                                     std::string& err) {
    IrRequest req;
    size_t body_len = 0;
    std::string all = head + extra;
    if (!parse_http_request_head(all.data(), all.size(), req, body_len, err)) {
        return {};
    }
    const size_t head_end = all.find("\r\n\r\n");
    std::string body = head_end == std::string::npos ? "" : all.substr(head_end + 4);
    std::vector<char> buf(16384);
    while (body.size() < body_len) {
        ssize_t r = ::read(fd, buf.data(), buf.size());
        if (r <= 0) { err = "incomplete"; return {}; }
        body.append(buf.data(), static_cast<size_t>(r));
        if (body.size() > body_len) { err = "body too large"; return {}; }
    }
    return all.substr(0, head_end + 4) + body;
}

// 从上游读取完整响应（头 + Content-Length 正文）。
std::string read_upstream_response(Upstream& up, std::string& err) {
    std::string acc;
    char buf[16384];
    // 读到头段结束
    while (acc.find("\r\n\r\n") == std::string::npos) {
        ssize_t r = up.read(buf, sizeof(buf));
        if (r <= 0) { err = "incomplete"; return {}; }
        acc.append(buf, static_cast<size_t>(r));
        if (acc.size() > 64 * 1024) { err = "head too large"; return {}; }
    }
    const size_t head_end = acc.find("\r\n\r\n");
    IrResponse res;
    size_t body_len = 0;
    if (!parse_http_response_head(acc.data(), acc.size(), res, body_len, err)) {
        return acc;
    }
    while (acc.size() - (head_end + 4) < body_len) {
        ssize_t r = up.read(buf, sizeof(buf));
        if (r <= 0) break;
        acc.append(buf, static_cast<size_t>(r));
        if (acc.size() > 16 * 1024 * 1024) { err = "body too large"; return {}; }
    }
    return acc;
}

// 从上游响应字节中提取 JSON body。
std::string response_body_of(const std::string& resp_bytes, std::string& err) {
    const auto end = resp_bytes.find("\r\n\r\n");
    if (end == std::string::npos) { err = "malformed response"; return {}; }
    return resp_bytes.substr(end + 4);
}

// 构造标准响应报文（200 + application/json + body）。
std::string build_standard_response(const std::string& json_body) {
    IrResponse res;
    res.version = "HTTP/1.1";
    res.status = 200;
    res.reason = "OK";
    res.headers = {
        {"Content-Type", "application/json; charset=utf-8"},
        {"Content-Length", std::to_string(json_body.size())},
        {"Connection", "close"},
    };
    res.body.assign(json_body.begin(), json_body.end());
    return generate_http_response(res);
}

// 判断客户端路径属于哪套标准；未知路径返回 false。
bool api_kind_of_path(const std::string& path, ApiKind& kind) {
    if (path == "/v1/chat/completions") { kind = ApiKind::OpenAi; return true; }
    if (path == "/v1/messages")         { kind = ApiKind::Anthropic; return true; }
    return false;
}

// 404 响应。
std::string not_found(const std::string& msg) {
    const std::string body = "{\"error\":{\"message\":\"" + msg + "\"}}";
    IrResponse res;
    res.version = "HTTP/1.1";
    res.status = 404;
    res.reason = "Not Found";
    res.headers = {
        {"Content-Type", "application/json"},
        {"Content-Length", std::to_string(body.size())},
        {"Connection", "close"},
    };
    res.body.assign(body.begin(), body.end());
    return generate_http_response(res);
}

void handle_adapt_client(int client_fd, const std::string& up_host, const std::string& up_port,
                         bool use_tls) {
    std::string head, extra;
    ssize_t n = read_until_headers_end(client_fd, head, extra);
    if (n <= 0) {
        ::close(client_fd);
        return;
    }

    IrRequest req;
    std::string err;
    size_t body_len = 0;
    if (!parse_http_request_head(head.data(), head.size(), req, body_len, err)) {
        ::close(client_fd);
        return;
    }

    ApiKind kind;
    if (!api_kind_of_path(req.path, kind)) {
        const std::string resp = not_found("仅支持 /v1/chat/completions 与 /v1/messages");
        write_full(client_fd, resp.data(), resp.size());
        ::close(client_fd);
        return;
    }

    // 读完整客户端请求（头 + body）
    const std::string full_req = read_full_client_request(client_fd, head, extra, err);
    if (full_req.empty()) {
        ::close(client_fd);
        return;
    }
    IrRequest full;
    if (!parse_http_request(full_req.data(), full_req.size(), full, err)) {
        ::close(client_fd);
        return;
    }
    req = full;

    // 提取客户端 body 与 model
    const std::string client_body(req.body.begin(), req.body.end());
    const std::string model = extract_model(client_body, kind);

    // 标准请求 body → 上游 miku-chat body
    std::string upstream_body;
    if (!to_upstream_body(client_body, kind, upstream_body, err)) {
        const std::string resp = not_found(err);
        write_full(client_fd, resp.data(), resp.size());
        ::close(client_fd);
        return;
    }

    // 构造上游请求（IR 改写：路径/method/Host/Content-Length/body）
    IrRequest up_req;
    up_req.method = "POST";
    up_req.scheme = use_tls ? "https" : "http";
    up_req.host = up_host;
    up_req.port = up_port;
    up_req.path = "/api/miku-chat";
    up_req.version = "HTTP/1.1";
    set_header(up_req.headers, "Host", up_host + ":" + up_port);
    set_header(up_req.headers, "Content-Type", "application/json");
    set_header(up_req.headers, "Content-Length", std::to_string(upstream_body.size()));
    set_header(up_req.headers, "Connection", "close");
    remove_header(up_req.headers, "Accept-Encoding");
    up_req.body.assign(upstream_body.begin(), upstream_body.end());

    // 上游连接
    Upstream up;
    if (use_tls) {
        up.tls = tls_connect(up_host, up_port, err);
        if (!up.tls) {
            ::close(client_fd);
            return;
        }
    } else {
        up.fd = tcp_connect(up_host, up_port, err);
        if (up.fd < 0) {
            ::close(client_fd);
            return;
        }
    }

    // IR → 上游请求报文并发送
    const std::string up_bytes = generate_http_request(up_req);
    up.write(up_bytes.data(), up_bytes.size());

    // 上游响应 → body → 标准响应 → 回给客户端
    std::string up_resp = read_upstream_response(up, err);
    std::string upstream_resp_body = response_body_of(up_resp, err);
    up.close();

    std::string standard_body;
    if (!to_standard_body(upstream_resp_body, kind, model, standard_body, err)) {
        const std::string resp = not_found(err);
        write_full(client_fd, resp.data(), resp.size());
        ::close(client_fd);
        return;
    }

    const std::string std_resp = build_standard_response(standard_body);
    write_full(client_fd, std_resp.data(), std_resp.size());
    ::close(client_fd);
}

} // namespace

int run_adapt(const std::string& listen_host, const std::string& listen_port,
              const std::string& upstream_host, const std::string& upstream_port,
              bool use_tls) {
    std::string err;
    int listen_fd = tcp_listen(listen_host, listen_port, err);
    if (listen_fd < 0) {
        std::fprintf(stderr, "adapt 监听失败: %s\n", err.c_str());
        return 1;
    }
    std::printf("FastProxy adapt 监听 %s:%s → %s%s:%s\n",
                listen_host.c_str(), listen_port.c_str(),
                use_tls ? "https://" : "http://",
                upstream_host.c_str(), upstream_port.c_str());
    std::printf("  /v1/chat/completions  (OpenAI)\n  /v1/messages          (Anthropic)\n");
    std::fflush(stdout);

    while (true) {
        int client = ::accept(listen_fd, nullptr, nullptr);
        if (client < 0) continue;
        std::thread(handle_adapt_client, client, upstream_host, upstream_port, use_tls).detach();
    }
    return 0;
}

} // namespace fp
