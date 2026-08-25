/*
 * ir.h — FastProxy 中间表示（Intermediate Representation）。
 *
 * 设计目标：把"客户端请求"与"目标服务端请求"之间的转换，统一经过一个
 * 文本形式的 IR。整条流水线是：
 *
 *   客户端原始请求 ──parse──▶ IR 文本 ──generate──▶ 目标服务端请求
 *
 * IR 内存结构在这里定义；文本序列化/反序列化在 ir.cpp。
 */
#ifndef FASTPROXY_IR_H
#define FASTPROXY_IR_H

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace fp {

// 请求在 IR 中表示为一张"事实表"：方法、目标、头部、正文。
struct IrRequest {
    std::string method;      // GET / POST / CONNECT ...
    std::string scheme;      // http / https（CONNECT 隧道用）
    std::string host;        // 目标主机（不带头部、不带端口）
    std::string port;        // 端口；空 = 按 scheme 取默认
    std::string path;        // 含查询串，如 /api/user?id=1
    std::string version;     // HTTP/1.0 / HTTP/1.1
    std::vector<std::pair<std::string, std::string>> headers;
    std::vector<char> body;  // 请求体；CONNECT 隧道时为空
    bool        is_tunnel = false;  // CONNECT 隧道直通标记
};

// 响应同样转成 IR，便于日志与后续分析。
struct IrResponse {
    std::string version;   // HTTP/1.1
    int         status = 0;
    std::string reason;    // OK / Not Found ...
    std::vector<std::pair<std::string, std::string>> headers;
    std::vector<char> body;
};

// ---- 文本序列化：对象 <-> IR 文本 ----
std::string to_ir_text(const IrRequest& req);
bool from_ir_text(const std::string& text, IrRequest& out, std::string& err);

std::string to_ir_text(const IrResponse& res);
bool from_ir_text(const std::string& text, IrResponse& out, std::string& err);

// 辅助：大小写不敏感地查找/设置头部。
const std::string* find_header(const std::vector<std::pair<std::string, std::string>>& hs,
                               const std::string& name);
void set_header(std::vector<std::pair<std::string, std::string>>& hs,
                const std::string& name, const std::string& value);
void remove_header(std::vector<std::pair<std::string, std::string>>& hs,
                   const std::string& name);

} // namespace fp

#endif // FASTPROXY_IR_H
