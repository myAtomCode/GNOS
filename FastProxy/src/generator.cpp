/*
 * generator.cpp — IR → 目标 HTTP 报文 生成器实现。
 */
#include "generator.h"

#include <sstream>

namespace fp {

namespace {

std::string join_headers(const std::vector<std::pair<std::string, std::string>>& hs) {
    std::ostringstream os;
    for (const auto& h : hs) {
        os << h.first << ": " << h.second << "\r\n";
    }
    return os.str();
}

} // namespace

std::string generate_http_request(const IrRequest& req) {
    std::ostringstream os;
    // 请求行：method SP target SP version
    std::string target = req.path;
    if (req.method == "CONNECT") {
        target = req.host + ":" + req.port;
    } else if (!req.path.empty() && req.path[0] != '/') {
        // 已是完整 URI 时原样使用（如正向代理转发 absolute-form）
        target = req.path;
    } else {
        // origin-form 保证以 / 开头
        if (target.empty()) target = "/";
    }
    os << req.method << " " << target << " "
       << (req.version.empty() ? "HTTP/1.1" : req.version) << "\r\n";

    // Host 头：缺失时补上
    if (!find_header(req.headers, "host") && !req.host.empty()) {
        os << "Host: " << req.host;
        if (!req.port.empty() && req.port != "80" && req.port != "443") {
            os << ":" << req.port;
        }
        os << "\r\n";
    }
    os << join_headers(req.headers);
    os << "\r\n";
    if (!req.body.empty()) {
        os.write(req.body.data(), static_cast<std::streamsize>(req.body.size()));
    }
    return os.str();
}

std::string generate_http_response(const IrResponse& res) {
    std::ostringstream os;
    os << (res.version.empty() ? "HTTP/1.1" : res.version) << " "
       << res.status << " "
       << (res.reason.empty() ? "OK" : res.reason) << "\r\n";
    os << join_headers(res.headers);
    os << "\r\n";
    if (!res.body.empty()) {
        os.write(res.body.data(), static_cast<std::streamsize>(res.body.size()));
    }
    return os.str();
}

bool generate_request_from_ir_text(const std::string& ir_text,
                                   std::string& out_bytes,
                                   std::string& err) {
    IrRequest req;
    if (!from_ir_text(ir_text, req, err)) return false;
    out_bytes = generate_http_request(req);
    return true;
}

bool generate_response_from_ir_text(const std::string& ir_text,
                                    std::string& out_bytes,
                                    std::string& err) {
    IrResponse res;
    if (!from_ir_text(ir_text, res, err)) return false;
    out_bytes = generate_http_response(res);
    return true;
}

} // namespace fp
