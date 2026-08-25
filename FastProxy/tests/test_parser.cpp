/*
 * test_parser.cpp — HTTP 解析器与流水线测试。
 */
#include <cassert>
#include <cstdio>
#include <string>

#include "generator.h"
#include "ir.h"
#include "parser.h"

static void test_parse_get() {
    const std::string raw =
        "GET /index.html?q=1 HTTP/1.1\r\n"
        "Host: example.com\r\n"
        "User-Agent: fp-test\r\n"
        "\r\n";
    fp::IrRequest req;
    std::string err;
    assert(fp::parse_http_request(raw.data(), raw.size(), req, err));
    assert(req.method == "GET");
    assert(req.path == "/index.html?q=1");
    assert(req.host == "example.com");
    assert(req.port == "80");
    assert(req.body.empty());
}

static void test_parse_post_body() {
    const std::string body = "name=fastproxy";
    const std::string raw =
        "POST /api HTTP/1.1\r\n"
        "Host: example.com\r\n"
        "Content-Type: application/x-www-form-urlencoded\r\n"
        "Content-Length: " + std::to_string(body.size()) + "\r\n"
        "\r\n" + body;
    fp::IrRequest req;
    std::string err;
    assert(fp::parse_http_request(raw.data(), raw.size(), req, err));
    assert(req.method == "POST");
    assert(req.body.size() == body.size());
    assert(std::string(req.body.data(), req.body.size()) == body);
}

static void test_pipeline() {
    // 完整流水线：原始请求 → IR 文本 → 目标请求
    const std::string raw =
        "GET /hello HTTP/1.1\r\n"
        "Host: example.com\r\n"
        "\r\n";
    bool ok = false;
    std::string err;
    const std::string ir_text = fp::request_to_ir_text(raw.data(), raw.size(), &ok, err);
    assert(ok);

    std::string target;
    assert(fp::generate_request_from_ir_text(ir_text, target, err));
    assert(target.rfind("GET /hello HTTP/1.1\r\n", 0) == 0);
    assert(target.find("Host: example.com") != std::string::npos);
}

static void test_absolute_uri() {
    // 正向代理常见：absolute-form 请求行
    const std::string raw =
        "GET http://example.com/path HTTP/1.1\r\n"
        "Host: example.com\r\n"
        "\r\n";
    fp::IrRequest req;
    std::string err;
    assert(fp::parse_http_request(raw.data(), raw.size(), req, err));
    assert(req.host == "example.com");
    assert(req.path == "/path");
}

static void test_generate_response() {
    fp::IrResponse res;
    res.version = "HTTP/1.1";
    res.status = 200;
    res.reason = "OK";
    res.headers = {{"Content-Type", "text/plain"}, {"Content-Length", "2"}};
    res.body = {'o', 'k'};

    const std::string out = fp::generate_http_response(res);
    assert(out.rfind("HTTP/1.1 200 OK\r\n", 0) == 0);
    assert(out.find("Content-Type: text/plain\r\n") != std::string::npos);
    assert(out.find("\r\n\r\nok") != std::string::npos);
}

static void test_generate_host_default() {
    // 无 Host 头时按 IR host/port 补 Host，80 端口不带端口号
    fp::IrRequest req;
    req.method = "GET";
    req.host = "example.com";
    req.port = "80";
    req.path = "/";
    const std::string out = fp::generate_http_request(req);
    assert(out.find("Host: example.com\r\n") != std::string::npos);
    assert(out.find("Host: example.com:80") == std::string::npos);
}

int main() {
    test_parse_get();
    test_parse_post_body();
    test_pipeline();
    test_absolute_uri();
    test_generate_response();
    test_generate_host_default();
    std::printf("test_parser: OK\n");
    return 0;
}
