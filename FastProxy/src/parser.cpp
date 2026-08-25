/*
 * parser.cpp — HTTP 报文解析器实现。
 *
 * 只做"报文 → IR 内存结构"这一步；语义改写（Host 重写等）由调用方完成。
 */
#include "parser.h"

#include <cstdlib>
#include <cstring>
#include <sstream>

namespace fp {

namespace {

bool parse_request_line(const std::string& line, IrRequest& req) {
    // "GET /path HTTP/1.1"
    auto sp1 = line.find(' ');
    if (sp1 == std::string::npos) return false;
    auto sp2 = line.find(' ', sp1 + 1);
    if (sp2 == std::string::npos) return false;

    req.method = line.substr(0, sp1);
    std::string target = line.substr(sp1 + 1, sp2 - sp1 - 1);
    req.version = line.substr(sp2 + 1);

    // 完整 URI 形式（http://host/path）或 origin-form（/path）
    if (target.rfind("http://", 0) == 0 || target.rfind("https://", 0) == 0) {
        auto scheme_end = target.find("://");
        req.scheme = target.substr(0, scheme_end);
        std::string rest = target.substr(scheme_end + 3);
        auto slash = rest.find('/');
        std::string authority = slash == std::string::npos ? rest : rest.substr(0, slash);
        req.path = slash == std::string::npos ? "/" : rest.substr(slash);

        auto colon = authority.rfind(':');
        if (colon != std::string::npos) {
            req.host = authority.substr(0, colon);
            req.port = authority.substr(colon + 1);
        } else {
            req.host = authority;
            req.port = req.scheme == "https" ? "443" : "80";
        }
    } else {
        // origin-form：host 由 Host 头补齐
        req.path = target;
        req.scheme = "http";
        req.port = "80";
    }
    return true;
}

bool parse_response_line(const std::string& line, IrResponse& res) {
    // "HTTP/1.1 200 OK"
    auto sp1 = line.find(' ');
    if (sp1 == std::string::npos) return false;
    auto sp2 = line.find(' ', sp1 + 1);
    res.version = line.substr(0, sp1);
    std::string code = line.substr(sp1 + 1, sp2 - sp1 - 1);
    res.status = std::atoi(code.c_str());
    res.reason = sp2 == std::string::npos ? "" : line.substr(sp2 + 1);
    return true;
}

bool parse_headers(const std::string& block, size_t start,
                   std::vector<std::pair<std::string, std::string>>& headers) {
    size_t pos = start;
    while (pos < block.size()) {
        auto eol = block.find("\r\n", pos);
        if (eol == std::string::npos) eol = block.size();
        if (eol == pos) break; // 空行 = 头段结束
        std::string line = block.substr(pos, eol - pos);
        auto colon = line.find(':');
        if (colon == std::string::npos) return false;
        std::string name = line.substr(0, colon);
        std::string value = line.substr(colon + 1);
        // 去掉值前导空白（OWS）
        size_t vs = value.find_first_not_of(" \t");
        if (vs != std::string::npos) value = value.substr(vs);
        headers.emplace_back(std::move(name), std::move(value));
        pos = eol + 2;
    }
    return true;
}

// 从 Headers 段解析出 body 长度（Content-Length），并解析 Host 等。
size_t content_length_of(const std::vector<std::pair<std::string, std::string>>& headers,
                         bool has_transfer_encoding) {
    if (has_transfer_encoding) return 0; // chunked 视为 0（本版不聚合 chunk）
    const std::string* cl = find_header(headers, "content-length");
    if (!cl) return 0;
    return static_cast<size_t>(std::strtoull(cl->c_str(), nullptr, 10));
}

bool has_header(const std::vector<std::pair<std::string, std::string>>& headers,
                const std::string& name) {
    return find_header(headers, name) != nullptr;
}

} // namespace

bool parse_http_request_head(const char* data, size_t len, IrRequest& out,
                             size_t& body_len, std::string& err) {
    out = IrRequest{};
    body_len = 0;
    const std::string buf(data, len);

    auto head_end = buf.find("\r\n\r\n");
    if (head_end == std::string::npos) {
        err = "incomplete";
        return false;
    }

    auto line_end = buf.find("\r\n");
    if (line_end == std::string::npos) {
        err = "malformed request line";
        return false;
    }

    if (!parse_request_line(buf.substr(0, line_end), out)) {
        err = "malformed request line";
        return false;
    }

    if (!parse_headers(buf, line_end + 2, out.headers)) {
        err = "malformed headers";
        return false;
    }

    // origin-form 时用 Host 头补 host/port
    if (out.host.empty()) {
        const std::string* host = find_header(out.headers, "host");
        if (host) {
            auto colon = host->rfind(':');
            if (colon != std::string::npos &&
                host->find(']') == std::string::npos) { // 非 IPv6 括号形式
                out.host = host->substr(0, colon);
                out.port = host->substr(colon + 1);
            } else {
                out.host = *host;
                out.port = out.scheme == "https" ? "443" : "80";
            }
        }
    }

    // CONNECT 隧道：目标在请求行 authority，无 body
    if (out.method == "CONNECT") {
        out.is_tunnel = true;
        if (out.path.find(':') != std::string::npos) {
            auto colon = out.path.rfind(':');
            out.host = out.path.substr(0, colon);
            out.port = out.path.substr(colon + 1);
            out.path.clear();
        }
        out.scheme = "https";
        return true;
    }

    // 只算 Content-Length，不要求 body 完整
    const bool te = has_header(out.headers, "transfer-encoding");
    body_len = content_length_of(out.headers, te);
    return true;
}

bool parse_http_request(const char* data, size_t len, IrRequest& out, std::string& err) {
    size_t body_len = 0;
    if (!parse_http_request_head(data, len, out, body_len, err)) return false;

    if (out.is_tunnel) return true;

    const std::string buf(data, len);
    auto head_end = buf.find("\r\n\r\n");
    const size_t body_start = head_end + 4;
    if (body_start + body_len > buf.size()) {
        err = "incomplete";
        return false;
    }
    out.body.assign(buf.begin() + static_cast<std::ptrdiff_t>(body_start),
                    buf.begin() + static_cast<std::ptrdiff_t>(body_start + body_len));
    return true;
}

bool parse_http_response_head(const char* data, size_t len, IrResponse& out,
                              size_t& body_len, std::string& err) {
    out = IrResponse{};
    body_len = 0;
    const std::string buf(data, len);

    auto head_end = buf.find("\r\n\r\n");
    if (head_end == std::string::npos) {
        err = "incomplete";
        return false;
    }
    auto line_end = buf.find("\r\n");
    if (line_end == std::string::npos || !parse_response_line(buf.substr(0, line_end), out)) {
        err = "malformed response line";
        return false;
    }
    if (!parse_headers(buf, line_end + 2, out.headers)) {
        err = "malformed headers";
        return false;
    }

    const bool te = has_header(out.headers, "transfer-encoding");
    body_len = content_length_of(out.headers, te);
    return true;
}

bool parse_http_response(const char* data, size_t len, IrResponse& out, std::string& err) {
    size_t body_len = 0;
    if (!parse_http_response_head(data, len, out, body_len, err)) return false;

    const std::string buf(data, len);
    auto head_end = buf.find("\r\n\r\n");
    const size_t body_start = head_end + 4;
    if (body_start + body_len > buf.size()) {
        err = "incomplete";
        return false;
    }
    out.body.assign(buf.begin() + static_cast<std::ptrdiff_t>(body_start),
                    buf.begin() + static_cast<std::ptrdiff_t>(body_start + body_len));
    return true;
}

std::string request_to_ir_text(const char* data, size_t len, bool* ok, std::string& err) {
    IrRequest req;
    if (!parse_http_request(data, len, req, err)) {
        if (ok) *ok = false;
        return {};
    }
    if (ok) *ok = true;
    return to_ir_text(req);
}

} // namespace fp
