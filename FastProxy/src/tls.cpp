/*
 * tls.cpp — OpenSSL TLS 客户端实现。
 */
#include "tls.h"

#include <cstring>
#include <openssl/err.h>
#include <openssl/ssl.h>
#include <unistd.h>

#include "net.h"

namespace fp {

struct TlsClient {
    SSL* ssl = nullptr;
    SSL_CTX* ctx = nullptr;
    int fd = -1;
};

TlsClient* tls_connect(const std::string& host, const std::string& port, std::string& err) {
    // 底层 TCP 连接
    std::string terr;
    int fd = tcp_connect(host, port, terr);
    if (fd < 0) {
        err = terr;
        return nullptr;
    }

    SSL_library_init();
    SSL_load_error_strings();

    const SSL_METHOD* method = TLS_client_method();
    SSL_CTX* ctx = SSL_CTX_new(method);
    if (!ctx) {
        err = "SSL_CTX_new failed";
        ::close(fd);
        return nullptr;
    }
    // 不校验证书链的知名宿主 CA 缺失环境；生产可改为加载系统 CA。
    SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE, nullptr);

    SSL* ssl = SSL_new(ctx);
    if (!ssl) {
        err = "SSL_new failed";
        SSL_CTX_free(ctx);
        ::close(fd);
        return nullptr;
    }
    SSL_set_fd(ssl, fd);
    // SNI：域名由 host 提供（IP 时忽略）
    if (host.find_first_not_of("0123456789.:") != std::string::npos) {
        SSL_set_tlsext_host_name(ssl, host.c_str());
    }

    if (SSL_connect(ssl) != 1) {
        char ebuf[256] = {0};
        ERR_error_string_n(ERR_get_error(), ebuf, sizeof(ebuf));
        err = std::string("TLS 握手失败: ") + ebuf;
        SSL_free(ssl);
        SSL_CTX_free(ctx);
        ::close(fd);
        return nullptr;
    }

    auto* c = new TlsClient;
    c->ssl = ssl;
    c->ctx = ctx;
    c->fd = fd;
    return c;
}

ssize_t tls_read(TlsClient* c, char* buf, size_t n) {
    if (!c || !c->ssl) return -1;
    return SSL_read(c->ssl, buf, static_cast<int>(n));
}

int tls_write(TlsClient* c, const char* buf, size_t n) {
    if (!c || !c->ssl) return -1;
    size_t sent = 0;
    while (sent < n) {
        int r = SSL_write(c->ssl, buf + sent, static_cast<int>(n - sent));
        if (r <= 0) return -1;
        sent += static_cast<size_t>(r);
    }
    return 0;
}

void tls_close(TlsClient* c) {
    if (!c) return;
    if (c->ssl) {
        SSL_shutdown(c->ssl);
        SSL_free(c->ssl);
    }
    if (c->ctx) SSL_CTX_free(c->ctx);
    if (c->fd >= 0) ::close(c->fd);
    delete c;
}

int tls_fd(TlsClient* c) {
    return c ? c->fd : -1;
}

} // namespace fp
