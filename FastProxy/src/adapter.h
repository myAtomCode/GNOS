/*
 * adapter.h — 协议适配层：私有 chat 协议 ↔ OpenAI / Anthropic 标准 API。
 *
 * 在 IR 流水线的 JSON body 层做双向转换：
 *
 *   客户端(OpenAI/Anthropic)  ──to_upstream──▶  上游 miku-chat
 *   上游 miku-chat 的 {reply}  ──to_standard──▶  客户端标准响应
 */
#ifndef FASTPROXY_ADAPTER_H
#define FASTPROXY_ADAPTER_H

#include <string>

namespace fp {

// 客户端请求标准类型
enum class ApiKind {
    OpenAi,     // POST /v1/chat/completions
    Anthropic,  // POST /v1/messages
};

// 把标准 API 请求 body 转成上游 miku-chat 请求 body。
// 成功返回 true 并填充 out；失败返回 false 并给出 err。
bool to_upstream_body(const std::string& client_body, ApiKind kind,
                      std::string& out, std::string& err);

// 把上游 {reply} 响应 body 转成标准 API 响应 body。
// model 用于响应中的 model 字段（可从客户端请求透传）。
bool to_standard_body(const std::string& upstream_body, ApiKind kind,
                      const std::string& model,
                      std::string& out, std::string& err);

// 从标准请求 body 里提取 model（供响应回填）。
std::string extract_model(const std::string& client_body, ApiKind kind);

} // namespace fp

#endif // FASTPROXY_ADAPTER_H
