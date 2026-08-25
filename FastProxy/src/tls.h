/*
 * tls.h — 基于 OpenSSL 的 TLS 客户端封装。
 *
 * 上游是 HTTPS（如 cats.renchengzhang.com:443），plain TCP 连接后需要
 * TLS 握手。这里封装一个极简的 TLS 客户端：连接 + 读写 + 关闭，
 * 接口形态与 net.h 保持一致，便于流水线复用。
 */
#ifndef FASTPROXY_TLS_H
#define FASTPROXY_TLS_H

#include <cstddef>
#include <string>

namespace fp {

// TLS 连接句柄（内部持有 SSL* 与底层 fd）。
struct TlsClient;

// 建立到 host:port 的 TLS 连接（含 TCP 连接 + TLS 握手）。
// 成功返回非空句柄；失败返回 nullptr 并填充 err。
TlsClient* tls_connect(const std::string& host, const std::string& port, std::string& err);

// 阻塞读/写。返回实际处理的字节数；<=0 表示出错或关闭。
ssize_t tls_read(TlsClient* c, char* buf, size_t n);
int tls_write(TlsClient* c, const char* buf, size_t n);

// 关闭并释放。
void tls_close(TlsClient* c);

// 底层 fd（用于 poll 等待可读事件时取用）。
int tls_fd(TlsClient* c);

} // namespace fp

#endif // FASTPROXY_TLS_H
