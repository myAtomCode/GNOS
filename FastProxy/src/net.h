/*
 * net.h — 极简 POSIX 网络封装：监听、连接、收发。
 */
#ifndef FASTPROXY_NET_H
#define FASTPROXY_NET_H

#include <string>

namespace fp {

struct SocketError {
    std::string message;
};

// 在 host:port 上创建监听 socket，返回 fd；失败返回 -1 并填充 err。
int tcp_listen(const std::string& host, const std::string& port, std::string& err);

// 连接 host:port，返回已连接 fd；失败返回 -1 并填充 err。
int tcp_connect(const std::string& host, const std::string& port, std::string& err);

// 阻塞读满 n 字节（或 EOF）。返回实际读到的字节数。
ssize_t read_full(int fd, char* buf, size_t n);

// 阻塞写满 n 字节。返回 0 表示成功，-1 失败。
int write_full(int fd, const char* buf, size_t n);

// 从 fd 读到分隔符 \r\n\r\n 为止（用于读取 HTTP 头段）。
// out 为头段（含分隔符）；extra 为同一次读入中分隔符之后的字节
// （TCP 可能把头部与正文一起送达，正文不能丢）。返回头段总字节数；
// -1 出错，0 表示 EOF。
ssize_t read_until_headers_end(int fd, std::string& out, std::string& extra);

// 便捷：一次性读取当前可读的全部数据（非严格，用于隧道）。
std::string drain_readable(int fd);

} // namespace fp

#endif // FASTPROXY_NET_H
