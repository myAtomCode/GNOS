/*
 * proxy.cpp — 正向代理模式实现。
 *
 * 流水线（请求侧）：
 *   客户端原始请求 ──parse──▶ IR 文本 ──generate──▶ 目标服务端请求
 * 响应侧同样经过 IR（解析上游响应 → IR 文本 → 重新生成 → 回给客户端）。
 *
 * CONNECT 隧道：解析出目标后直通（隧道本身不再逐字节转 IR，但建立过程
 * 仍经过 IR 解析）。
 */
#include <cstdio>
#include <cstring>
#include <poll.h>
#include <string>
#include <sys/socket.h>
#include <thread>
#include <unistd.h>
#include <vector>

#include "generator.h"
#include "ir.h"
#include "net.h"
#include "parser.h"

namespace fp {

namespace {

// 双向隧道转发：把 a<->b 的所有剩余字节互转，直到任一端 EOF/出错。
void tunnel(int a, int b) {
    std::vector<char> buf(16384);
    while (true) {
        struct pollfd fds[2];
        fds[0] = {a, POLLIN, 0};
        fds[1] = {b, POLLIN, 0};
        int pr = ::poll(fds, 2, -1);
        if (pr < 0) break;

        for (int i = 0; i < 2; ++i) {
            if (!(fds[i].revents & (POLLIN | POLLHUP | POLLERR))) continue;
            int src = (i == 0) ? a : b;
            int dst = (i == 0) ? b : a;
            ssize_t r = ::read(src, buf.data(), buf.size());
            if (r <= 0) return; // EOF 或错误：隧道结束
            if (write_full(dst, buf.data(), static_cast<size_t>(r)) != 0) return;
        }
    }
}

// 从 fd 读取完整请求：头段（含预读字节）+ Content-Length 正文。
// 成功返回完整字节串；失败返回空串并置 err。
std::string read_full_request(int fd, const std::string& head, const std::string& extra,
                              std::string& err) {
    IrRequest req;
    size_t body_len = 0;
    // head 可能同时含头部 + 预读正文（同包到达），统一按 head-only 解析
    std::string all = head + extra;
    if (!parse_http_request_head(all.data(), all.size(), req, body_len, err)) {
        return {};
    }
    if (req.is_tunnel) return all; // CONNECT 无 body

    const size_t head_end = all.find("\r\n\r\n");
    size_t have = head_end == std::string::npos ? 0 : all.size() - head_end - 4;
    std::string body = head_end == std::string::npos ? "" : all.substr(head_end + 4);
    (void)have;
    std::vector<char> buf(16384);
    while (body.size() < body_len) {
        ssize_t r = ::read(fd, buf.data(), buf.size());
        if (r <= 0) { err = "incomplete"; return {}; }
        body.append(buf.data(), static_cast<size_t>(r));
        if (body.size() > body_len) { err = "body too large"; return {}; }
    }
    std::string result = all.substr(0, head_end + 4) + body;
    return result;
}

// 读取上游完整响应：头段 + Content-Length 正文。
std::string read_full_response(int fd, const std::string& extra, std::string& err) {
    std::string head, rest;
    if (!extra.empty()) {
        head = extra; // 响应头可能随请求的预读字节一起被读入
    } else {
        ssize_t n = read_until_headers_end(fd, head, rest);
        if (n <= 0) { err = "incomplete"; return {}; }
    }

    IrResponse res;
    size_t body_len = 0;
    std::string all = head + rest;
    if (!parse_http_response_head(all.data(), all.size(), res, body_len, err)) {
        return all; // 解析不了也原样转发
    }
    const size_t head_end = all.find("\r\n\r\n");
    size_t have = head_end == std::string::npos ? 0 : all.size() - head_end - 4;
    std::string body = head_end == std::string::npos ? "" : all.substr(head_end + 4);
    (void)have;

    std::vector<char> buf(16384);
    while (body.size() < body_len) {
        ssize_t r = ::read(fd, buf.data(), buf.size());
        if (r <= 0) break;
        body.append(buf.data(), static_cast<size_t>(r));
        if (body.size() > body_len) { err = "body too large"; return {}; }
    }
    return all.substr(0, head_end + 4) + body;
}

// 处理单个客户端连接（每连接一线程）。
void handle_proxy_client(int client_fd) {
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

    // CONNECT 隧道：确认后直通（目标在请求行 authority）
    if (req.is_tunnel) {
        const char* ok = "HTTP/1.1 200 Connection Established\r\n\r\n";
        write_full(client_fd, ok, std::strlen(ok));
        std::string terr;
        int up = tcp_connect(req.host, req.port, terr);
        if (up < 0) {
            ::close(client_fd);
            return;
        }
        // 隧道：把预读字节先交给目标，再双向转发
        if (!extra.empty()) write_full(up, extra.data(), extra.size());
        tunnel(client_fd, up);
        ::close(up);
        ::close(client_fd);
        return;
    }

    // 普通请求：读完整（头 + Content-Length 正文）后再转 IR
    const std::string full_req_bytes = read_full_request(client_fd, head, extra, err);
    if (full_req_bytes.empty()) {
        ::close(client_fd);
        return;
    }
    IrRequest full_req;
    if (!parse_http_request(full_req_bytes.data(), full_req_bytes.size(), full_req, err)) {
        ::close(client_fd);
        return;
    }
    req = full_req;

    // 请求 → IR 文本（流水线前半段；同时打印审计）
    const std::string ir_text = to_ir_text(req);
    std::fputs(ir_text.c_str(), stdout);
    std::fflush(stdout);

    std::string up_err;
    int up = tcp_connect(req.host, req.port, up_err);
    if (up < 0) {
        ::close(client_fd);
        return;
    }

    // IR 文本 → 目标请求（流水线后半段）
    std::string target_bytes;
    if (!generate_request_from_ir_text(ir_text, target_bytes, err)) {
        ::close(up);
        ::close(client_fd);
        return;
    }
    write_full(up, target_bytes.data(), target_bytes.size());

    // 上游响应 → 完整字节 → 解析成 IR → 重新生成 → 回给客户端
    std::string resp_bytes = read_full_response(up, "", err);
    if (!resp_bytes.empty()) {
        IrResponse res;
        if (parse_http_response(resp_bytes.data(), resp_bytes.size(), res, err)) {
            const std::string res_ir = to_ir_text(res);
            std::fputs(res_ir.c_str(), stdout);
            std::fflush(stdout);
            const std::string regenerated = generate_http_response(res);
            write_full(client_fd, regenerated.data(), regenerated.size());
        } else {
            write_full(client_fd, resp_bytes.data(), resp_bytes.size());
        }
    }

    ::close(up);
    ::close(client_fd);
}

} // namespace

int run_proxy(const std::string& listen_host, const std::string& listen_port) {
    std::string err;
    int listen_fd = tcp_listen(listen_host, listen_port, err);
    if (listen_fd < 0) {
        std::fprintf(stderr, "proxy 监听失败: %s\n", err.c_str());
        return 1;
    }
    std::printf("FastProxy proxy 监听 %s:%s\n", listen_host.c_str(), listen_port.c_str());
    std::fflush(stdout);

    while (true) {
        int client = ::accept(listen_fd, nullptr, nullptr);
        if (client < 0) continue;
        // 每连接一线程（detach，短生命周期）
        std::thread(handle_proxy_client, client).detach();
    }
    return 0;
}

} // namespace fp
