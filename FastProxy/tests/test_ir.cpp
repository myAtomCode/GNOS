/*
 * test_ir.cpp — IR 文本格式往返测试。
 */
#include <cassert>
#include <cstdio>
#include <string>

#include "ir.h"

static void test_request_roundtrip() {
    fp::IrRequest req;
    req.method = "GET";
    req.scheme = "http";
    req.host = "example.com";
    req.port = "80";
    req.path = "/index.html?q=1";
    req.version = "HTTP/1.1";
    req.headers = {{"Host", "example.com"}, {"User-Agent", "fp-test"}};
    req.body = {'h', 'i'};

    const std::string text = fp::to_ir_text(req);
    fp::IrRequest out;
    std::string err;
    assert(fp::from_ir_text(text, out, err));
    assert(out.method == "GET");
    assert(out.host == "example.com");
    assert(out.port == "80");
    assert(out.path == "/index.html?q=1");
    assert(out.headers.size() == 2);
    assert(out.body.size() == 2 && out.body[0] == 'h');
}

static void test_response_roundtrip() {
    fp::IrResponse res;
    res.version = "HTTP/1.1";
    res.status = 404;
    res.reason = "Not Found";
    res.headers = {{"Content-Type", "text/plain"}};
    res.body = {'N', 'F'};

    const std::string text = fp::to_ir_text(res);
    fp::IrResponse out;
    std::string err;
    assert(fp::from_ir_text(text, out, err));
    assert(out.status == 404);
    assert(out.reason == "Not Found");
    assert(out.body.size() == 2);
}

static void test_missing_body_marker() {
    fp::IrRequest out;
    std::string err;
    assert(!fp::from_ir_text("method GET\n", out, err));
}

static void test_header_helpers() {
    std::vector<std::pair<std::string, std::string>> hs = {{"Content-Type", "a/b"}};
    assert(fp::find_header(hs, "content-type") != nullptr);
    fp::set_header(hs, "CONTENT-TYPE", "c/d");
    assert(fp::find_header(hs, "content-type")->compare("c/d") == 0);
    fp::remove_header(hs, "Content-Type");
    assert(fp::find_header(hs, "content-type") == nullptr);
}

int main() {
    test_request_roundtrip();
    test_response_roundtrip();
    test_missing_body_marker();
    test_header_helpers();
    std::printf("test_ir: OK\n");
    return 0;
}
