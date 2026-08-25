/*
 * ir.cpp — IR 文本格式实现。
 *
 * IR 文本格式（请求）：
 *
 *   # FastProxy IR v1
 *   method GET
 *   scheme http
 *   host example.com
 *   port 80
 *   path /index.html?q=1
 *   version HTTP/1.1
 *   header Host: example.com
 *   header User-Agent: fp/0.1
 *   body-len 0
 *   --body--
 *
 * 设计要点：
 *   - 每行一个"事实"，便于人读、diff、审计；
 *   - header 行保留原始名称大小写，值完整；
 *   - body 在 --body-- 标记后按字节原样存放（文本或二进制均可）；
 *   - 反序列化只做结构校验，不做 HTTP 语义校验。
 */
#include "ir.h"

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <sstream>

namespace fp {

// ---- 序列化请求 ----
std::string to_ir_text(const IrRequest& req) {
    std::ostringstream os;
    os << "# FastProxy IR v1\n";
    os << "method " << req.method << "\n";
    os << "scheme " << (req.scheme.empty() ? "http" : req.scheme) << "\n";
    os << "host " << req.host << "\n";
    os << "port " << req.port << "\n";
    os << "path " << req.path << "\n";
    os << "version " << (req.version.empty() ? "HTTP/1.1" : req.version) << "\n";
    for (const auto& h : req.headers) {
        os << "header " << h.first << ": " << h.second << "\n";
    }
    os << "body-len " << req.body.size() << "\n";
    os << "--body--\n";
    os.write(req.body.data(), static_cast<std::streamsize>(req.body.size()));
    return os.str();
}

// ---- 序列化响应 ----
std::string to_ir_text(const IrResponse& res) {
    std::ostringstream os;
    os << "# FastProxy IR v1 response\n";
    os << "version " << (res.version.empty() ? "HTTP/1.1" : res.version) << "\n";
    os << "status " << res.status << "\n";
    os << "reason " << res.reason << "\n";
    for (const auto& h : res.headers) {
        os << "header " << h.first << ": " << h.second << "\n";
    }
    os << "body-len " << res.body.size() << "\n";
    os << "--body--\n";
    os.write(res.body.data(), static_cast<std::streamsize>(res.body.size()));
    return os.str();
}

// ---- 通用反序列化骨架 ----
namespace {

// 从 body 标记之后原样读取 n 字节。
bool read_body(const std::string& text, size_t offset, size_t n, std::vector<char>& out) {
    if (offset + n > text.size()) return false;
    out.assign(text.begin() + static_cast<std::ptrdiff_t>(offset),
               text.begin() + static_cast<std::ptrdiff_t>(offset + n));
    return true;
}

// 解析 key value 形式的行。
bool split_kv(const std::string& line, std::string& key, std::string& val) {
    auto sp = line.find(' ');
    if (sp == std::string::npos) return false;
    key = line.substr(0, sp);
    val = line.substr(sp + 1);
    return true;
}

} // namespace

bool from_ir_text(const std::string& text, IrRequest& out, std::string& err) {
    out = IrRequest{};
    std::istringstream is(text);
    std::string line;
    bool in_body = false;
    size_t body_left = 0;
    std::string body_marker;
    std::string::size_type body_offset = 0;

    // 先定位 body 偏移：--body-- 行之后紧接 body 字节。
    auto marker = text.find("--body--\n");
    if (marker == std::string::npos) {
        err = "缺少 --body-- 标记";
        return false;
    }
    body_offset = marker + 9; // "--body--\n" 的长度

    while (std::getline(is, line)) {
        // 去掉行尾 \r（CRLF 兼容）
        if (!line.empty() && line.back() == '\r') line.pop_back();

        if (in_body) break; // body 已用偏移量处理，跳出

        if (line.empty()) continue;
        if (line[0] == '#') continue; // 注释行

        if (line == "--body--") {
            in_body = true;
            continue;
        }

        std::string key, val;
        if (!split_kv(line, key, val)) {
            err = "IR 行格式错误: " + line;
            return false;
        }

        if (key == "method") out.method = val;
        else if (key == "scheme") out.scheme = val;
        else if (key == "host") out.host = val;
        else if (key == "port") out.port = val;
        else if (key == "path") out.path = val;
        else if (key == "version") out.version = val;
        else if (key == "body-len") {
            body_left = static_cast<size_t>(std::stoull(val));
        } else if (key == "header") {
            auto colon = val.find(':');
            if (colon == std::string::npos) {
                err = "header 缺少冒号: " + val;
                return false;
            }
            std::string hname = val.substr(0, colon);
            std::string hval = val.substr(colon + 1);
            if (!hval.empty() && hval.front() == ' ') hval.erase(0, 1);
            out.headers.emplace_back(std::move(hname), std::move(hval));
        } else {
            err = "未知 IR 字段: " + key;
            return false;
        }
    }

    if (!read_body(text, body_offset, body_left, out.body)) {
        err = "body 长度不匹配";
        return false;
    }
    return true;
}

bool from_ir_text(const std::string& text, IrResponse& out, std::string& err) {
    out = IrResponse{};
    std::istringstream is(text);
    std::string line;
    std::string body_marker;
    size_t body_left = 0;

    auto marker = text.find("--body--\n");
    if (marker == std::string::npos) {
        err = "缺少 --body-- 标记";
        return false;
    }
    const size_t body_offset = marker + 9;

    while (std::getline(is, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty() || line[0] == '#') continue;
        if (line == "--body--") break;

        std::string key, val;
        if (!split_kv(line, key, val)) {
            err = "IR 行格式错误: " + line;
            return false;
        }
        if (key == "version") out.version = val;
        else if (key == "status") out.status = std::stoi(val);
        else if (key == "reason") out.reason = val;
        else if (key == "body-len") body_left = static_cast<size_t>(std::stoull(val));
        else if (key == "header") {
            auto colon = val.find(':');
            if (colon == std::string::npos) {
                err = "header 缺少冒号: " + val;
                return false;
            }
            std::string hname = val.substr(0, colon);
            std::string hval = val.substr(colon + 1);
            if (!hval.empty() && hval.front() == ' ') hval.erase(0, 1);
            out.headers.emplace_back(std::move(hname), std::move(hval));
        } else {
            err = "未知 IR 字段: " + key;
            return false;
        }
    }

    if (!read_body(text, body_offset, body_left, out.body)) {
        err = "body 长度不匹配";
        return false;
    }
    return true;
}

// ---- 头部辅助 ----
const std::string* find_header(const std::vector<std::pair<std::string, std::string>>& hs,
                               const std::string& name) {
    for (const auto& h : hs) {
        if (h.first.size() == name.size() &&
            std::equal(h.first.begin(), h.first.end(), name.begin(),
                       [](char a, char b) { return std::tolower(a) == std::tolower(b); })) {
            return &h.second;
        }
    }
    return nullptr;
}

void set_header(std::vector<std::pair<std::string, std::string>>& hs,
                const std::string& name, const std::string& value) {
    for (auto& h : hs) {
        if (h.first.size() == name.size() &&
            std::equal(h.first.begin(), h.first.end(), name.begin(),
                       [](char a, char b) { return std::tolower(a) == std::tolower(b); })) {
            h.second = value;
            return;
        }
    }
    hs.emplace_back(name, value);
}

void remove_header(std::vector<std::pair<std::string, std::string>>& hs,
                   const std::string& name) {
    hs.erase(std::remove_if(hs.begin(), hs.end(),
        [&](const auto& h) {
            return h.first.size() == name.size() &&
                   std::equal(h.first.begin(), h.first.end(), name.begin(),
                              [](char a, char b) { return std::tolower(a) == std::tolower(b); });
        }),
        hs.end());
}

} // namespace fp
