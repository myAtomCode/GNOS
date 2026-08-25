/*
 * net.cpp — POSIX 网络封装实现。
 */
#include "net.h"

#include <arpa/inet.h>
#include <cerrno>
#include <cstring>
#include <netdb.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#include <vector>

namespace fp {

int tcp_listen(const std::string& host, const std::string& port, std::string& err) {
    struct addrinfo hints;
    std::memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_flags = AI_PASSIVE;

    struct addrinfo* res = nullptr;
    const char* h = host.empty() ? nullptr : host.c_str();
    if (getaddrinfo(h, port.c_str(), &hints, &res) != 0 || !res) {
        err = "getaddrinfo failed";
        return -1;
    }

    int fd = -1;
    for (struct addrinfo* p = res; p; p = p->ai_next) {
        fd = socket(p->ai_family, p->ai_socktype, p->ai_protocol);
        if (fd < 0) continue;
        int one = 1;
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
        if (bind(fd, p->ai_addr, p->ai_addrlen) == 0 &&
            listen(fd, 128) == 0) {
            break;
        }
        ::close(fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0) err = "bind/listen failed";
    return fd;
}

int tcp_connect(const std::string& host, const std::string& port, std::string& err) {
    struct addrinfo hints;
    std::memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    struct addrinfo* res = nullptr;
    if (getaddrinfo(host.c_str(), port.c_str(), &hints, &res) != 0 || !res) {
        err = "getaddrinfo failed: " + host + ":" + port;
        return -1;
    }

    int fd = -1;
    for (struct addrinfo* p = res; p; p = p->ai_next) {
        fd = socket(p->ai_family, p->ai_socktype, p->ai_protocol);
        if (fd < 0) continue;
        if (connect(fd, p->ai_addr, p->ai_addrlen) == 0) break;
        ::close(fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0) err = "connect failed: " + host + ":" + port;
    return fd;
}

ssize_t read_full(int fd, char* buf, size_t n) {
    size_t got = 0;
    while (got < n) {
        ssize_t r = ::read(fd, buf + got, n - got);
        if (r == 0) break;
        if (r < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        got += static_cast<size_t>(r);
    }
    return static_cast<ssize_t>(got);
}

int write_full(int fd, const char* buf, size_t n) {
    size_t sent = 0;
    while (sent < n) {
        ssize_t r = ::write(fd, buf + sent, n - sent);
        if (r <= 0) {
            if (r < 0 && errno == EINTR) continue;
            return -1;
        }
        sent += static_cast<size_t>(r);
    }
    return 0;
}

ssize_t read_until_headers_end(int fd, std::string& out, std::string& extra) {
    out.clear();
    extra.clear();
    std::string acc;
    char buf[4096];
    while (true) {
        ssize_t r = ::read(fd, buf, sizeof(buf));
        if (r <= 0) return r; // -1 出错 / 0 EOF
        acc.append(buf, static_cast<size_t>(r));
        auto end = acc.find("\r\n\r\n");
        if (end != std::string::npos) {
            out = acc.substr(0, end + 4);
            extra = acc.substr(end + 4); // 同包到达的正文不能丢
            return static_cast<ssize_t>(out.size());
        }
        if (acc.size() > 64 * 1024) return -1; // 头段过大
    }
}

std::string drain_readable(int fd) {
    std::string data;
    char buf[16384];
    while (true) {
        ssize_t r = ::read(fd, buf, sizeof(buf));
        if (r <= 0) break;
        data.append(buf, static_cast<size_t>(r));
    }
    return data;
}

} // namespace fp
