/*
 * generator.h — IR → 目标服务端请求 生成器。
 *
 * 消费 IR 文本（或内存对象），产出目标服务端能直接接受的原始 HTTP 报文。
 * 流水线的后半段。
 */
#ifndef FASTPROXY_GENERATOR_H
#define FASTPROXY_GENERATOR_H

#include <string>
#include "ir.h"

namespace fp {

// 内存 IR → 原始 HTTP 请求报文。
std::string generate_http_request(const IrRequest& req);

// IR 文本 → 原始 HTTP 请求报文。
// 失败时返回 false 并在 err 中给出原因。
bool generate_request_from_ir_text(const std::string& ir_text,
                                   std::string& out_bytes,
                                   std::string& err);

// 内存 IR → 原始 HTTP 响应报文。
std::string generate_http_response(const IrResponse& res);

// IR 文本 → 原始 HTTP 响应报文。
bool generate_response_from_ir_text(const std::string& ir_text,
                                    std::string& out_bytes,
                                    std::string& err);

} // namespace fp

#endif // FASTPROXY_GENERATOR_H
