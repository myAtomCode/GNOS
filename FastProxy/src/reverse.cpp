/*
 * reverse.cpp — 反向代理模式实现。
 *
 * 流水线（与正向代理同构，只是目标不再是请求里的 Host，而是固定上游）：
 *   客户端请求 ──parse──▶ IR 文本（改写 host/port 指向上游）
 *                    ──generate──▶ 上游服务端请求
 *   上游响应 ──parse──▶ IR 文本 ──generate──▶ 回给客户端
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

// 从 fd 读取完整请求：头段（含预读字节）+ Content-Length 正文。
std::string read_full_request(int fd, const std::string& head, const std::string& extra,
                              std::string& err) {
    IrRequest req;
    size_t body_len = 0;
    std::string all = head + extra;
    if (!parse_http_request_head(all.data(), all.size(), req, body_len, err)) {
        return {};
    }
    if (req.is_tunnel) return all;

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

// 读取上游完整响应：头段 + Content-Length 正文。
std::string read_full_response(int fd, const std::string& extra, std::string& err) {
    std::string head, rest;
    if (!extra.empty()) {
        head = extra;
    } else {
        ssize_t n = read_until_headers_end(fd, head, rest);
        if (n <= 0) { err = "incomplete"; return {}; }
    }

    IrResponse res;
    size_t body_len = 0;
    std::string all = head + rest;
    if (!parse_http_response_head(all.data(), all.size(), res, body_len, err)) {
        return all;
    }
    const size_t head_end = all.find("\r\n\r\n");
    std::string body = head_end == std::string::npos ? "" : all.substr(head_end + 4);

    std::vector<char> buf(16384);
    while (body.size() < body_len) {
        ssize_t r = ::read(fd, buf.data(), buf.size());
        if (r <= 0) break;
        body.append(buf.data(), static_cast<size_t>(r));
        if (body.size() > body_len) { err = "body too large"; return {}; }
    }
    return all.substr(0, head_end + 4) + body;
}

// 双向隧道转发（反向代理对 CONNECT 同样直通）。
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
            if (r <= 0) return;
            if (write_full(dst, buf.data(), static_cast<size_t>(r)) != 0) return;
        }
    }
}

void handle_reverse_client(int client_fd, const std::string& up_host, const std::string& up_port) {
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

    // CONNECT：反向代理同样直通上游（https 场景）
    if (req.is_tunnel) {
        const char* ok = "HTTP/1.1 200 Connection Established\r\n\r\n";
        write_full(client_fd, ok, std::strlen(ok));
        std::string terr;
        int up = tcp_connect(up_host, up_port, terr);
        if (up < 0) {
            ::close(client_fd);
            return;
        }
        if (!extra.empty()) write_full(up, extra.data(), extra.size());
        tunnel(client_fd, up);
        ::close(up);
        ::close(client_fd);
        return;
    }

    // 读完整请求（头 + 正文）
    const std::string full_bytes = read_full_request(client_fd, head, extra, err);
    if (full_bytes.empty()) {
        ::close(client_fd);
        return;
    }
    IrRequest full_req;
    if (!parse_http_request(full_bytes.data(), full_bytes.size(), full_req, err)) {
        ::close(client_fd);
        return;
    }
    req = full_req;

    // 反向代理核心：把 IR 中的目标改写为上游（Host 头同步改写）
    req.host = up_host;
    req.port = up_port;
    set_header(req.headers, "Host", up_host + ":" + up_port);

    // 请求 → IR 文本（审计；这里展示改写后的 IR）
    const std::string ir_text = to_ir_text(req);
    std::fputs(ir_text.c_str(), stdout);
    std::fflush(stdout);

    std::string up_err;
    int up = tcp_connect(up_host, up_port, up_err);
    if (up < 0) {
        ::close(client_fd);
        return;
    }

    // IR 文本 → 上游请求
    std::string target_bytes;
    if (!generate_request_from_ir_text(ir_text, target_bytes, err)) {
        ::close(up);
        ::close(client_fd);
        return;
    }
    write_full(up, target_bytes.data(), target_bytes.size());

    // 上游响应 → 完整字节 → IR → 重新生成 → 回给客户端
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

int run_reverse(const std::string& listen_host, const std::string& listen_port,
                const std::string& upstream_host, const std::string& upstream_port) {
    std::string err;
    int listen_fd = tcp_listen(listen_host, listen_port, err);
    if (listen_fd < 0) {
        std::fprintf(stderr, "reverse 监听失败: %s\n", err.c_str());
        return 1;
    }
    std::printf("FastProxy reverse 监听 %s:%s → 上游 %s:%s\n",
                listen_host.c_str(), listen_port.c_str(),
                upstream_host.c_str(), upstream_port.c_str());
    std::fflush(stdout);

    while (true) {
        int client = ::accept(listen_fd, nullptr, nullptr);
        if (client < 0) continue;
        std::thread(handle_reverse_client, client, upstream_host, upstream_port).detach();
    }
    return 0;
}

} // namespace fp
