/*
 * main.cpp — FastProxy 入口。
 *
 * 用法：
 *   fastproxy proxy   --listen 127.0.0.1:8080
 *   fastproxy reverse --listen 127.0.0.1:8080 --upstream 127.0.0.1:9000
 */
#include <cstdio>
#include <cstring>
#include <string>

namespace fp {
int run_proxy(const std::string& listen_host, const std::string& listen_port);
int run_reverse(const std::string& listen_host, const std::string& listen_port,
                const std::string& upstream_host, const std::string& upstream_port);
int run_adapt(const std::string& listen_host, const std::string& listen_port,
              const std::string& upstream_host, const std::string& upstream_port,
              bool use_tls);
} // namespace fp

static void usage(const char* argv0) {
    std::printf(
        "FastProxy %s\n"
        "用法:\n"
        "  %s proxy   --listen HOST:PORT\n"
        "  %s reverse --listen HOST:PORT --upstream HOST:PORT\n"
        "  %s adapt   --listen HOST:PORT --upstream HOST:PORT [--tls]\n"
        "\n"
        "模式说明:\n"
        "  proxy    正向代理：分析客户端请求，转为 IR 文本，再生成目标请求并转发。\n"
        "  reverse  反向代理：把上游视为目标服务端，同样经过 IR 转换后转发。\n"
        "  adapt    协议适配：把私有 chat 协议包装成 OpenAI(/v1/chat/completions)\n"
        "           与 Anthropic(/v1/messages) 两套标准 API；上游为 HTTPS 时加 --tls。\n",
        "0.2.0", argv0, argv0, argv0);
}

int main(int argc, char** argv) {
    if (argc < 2) {
        usage(argv[0]);
        return 2;
    }

    const std::string mode = argv[1];
    std::string listen_host = "127.0.0.1";
    std::string listen_port = "8080";
    std::string upstream_host;
    std::string upstream_port;
    bool use_tls = false;

    for (int i = 2; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--listen" && i + 1 < argc) {
            const std::string v = argv[++i];
            const auto colon = v.rfind(':');
            if (colon == std::string::npos) {
                listen_port = v;
            } else {
                listen_host = v.substr(0, colon);
                listen_port = v.substr(colon + 1);
            }
        } else if (a == "--upstream" && i + 1 < argc) {
            const std::string v = argv[++i];
            const auto colon = v.rfind(':');
            if (colon == std::string::npos) {
                std::fprintf(stderr, "--upstream 需要 HOST:PORT 格式\n");
                return 2;
            }
            upstream_host = v.substr(0, colon);
            upstream_port = v.substr(colon + 1);
        } else if (a == "--tls") {
            use_tls = true;
        } else {
            std::fprintf(stderr, "未知参数: %s\n", a.c_str());
            usage(argv[0]);
            return 2;
        }
    }

    if (mode == "proxy") {
        return fp::run_proxy(listen_host, listen_port);
    }
    if (mode == "reverse") {
        if (upstream_host.empty()) {
            std::fprintf(stderr, "reverse 模式需要 --upstream HOST:PORT\n");
            return 2;
        }
        return fp::run_reverse(listen_host, listen_port, upstream_host, upstream_port);
    }
    if (mode == "adapt") {
        if (upstream_host.empty()) {
            std::fprintf(stderr, "adapt 模式需要 --upstream HOST:PORT\n");
            return 2;
        }
        return fp::run_adapt(listen_host, listen_port, upstream_host, upstream_port, use_tls);
    }

    usage(argv[0]);
    return 2;
}
