/*
 * parser.h — HTTP 报文解析器：原始字节流 → IR 文本。
 */
#ifndef FASTPROXY_PARSER_H
#define FASTPROXY_PARSER_H

#include <string>
#include "ir.h"

namespace fp {

// 把原始 HTTP 请求字节流解析成 IR 内存结构。
// 返回 false 并在 err 中给出原因（报文不完整时返回 false + err=="incomplete"）。
bool parse_http_request(const char* data, size_t len, IrRequest& out, std::string& err);

// 只解析请求行与头部（不要求 body 完整），用于"先读头、再按 Content-Length
// 续读 body"的流式流程。body_len 输出 Content-Length（无 body 时为 0）。
bool parse_http_request_head(const char* data, size_t len, IrRequest& out,
                             size_t& body_len, std::string& err);

// 把原始 HTTP 响应字节流解析成 IR 内存结构。
bool parse_http_response(const char* data, size_t len, IrResponse& out, std::string& err);

// 只解析响应行与头部，不要求 body 完整。
bool parse_http_response_head(const char* data, size_t len, IrResponse& out,
                              size_t& body_len, std::string& err);

// 把原始请求字节流转成 IR *文本*（parse + to_ir_text 的便捷组合）。
// 这是流水线的前半段，供代理核心直接使用。
std::string request_to_ir_text(const char* data, size_t len, bool* ok, std::string& err);

} // namespace fp

#endif // FASTPROXY_PARSER_H
