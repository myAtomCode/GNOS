/*
 * adapter.cpp — 协议适配层实现。
 *
 * 上游（cats.renchengzhang.com/api/miku-chat）实测协议：
 *   请求：{"messages":[{role,content}...],"passiveVocaloidLookups":[],"lyricContexts":[],"memoryBrief":{...}}
 *   响应：{"reply":"...","lookup":{...}}
 *
 * 标准侧：
 *   OpenAI    /v1/chat/completions  请求 {model,messages,stream?}
 *   Anthropic /v1/messages          请求 {model,max_tokens,messages}
 */
#include "adapter.h"

#include <chrono>
#include <ctime>
#include <random>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

using nlohmann::json;

namespace fp {

namespace {

std::string random_suffix(size_t n = 8) {
    static const char* chars = "abcdefghijklmnopqrstuvwxyz0123456789";
    static std::mt19937 rng(static_cast<unsigned>(std::time(nullptr)));
    std::uniform_int_distribution<size_t> dist(0, 35);
    std::string s;
    s.reserve(n);
    for (size_t i = 0; i < n; ++i) s.push_back(chars[dist(rng)]);
    return s;
}

// 把 Anthropic 的 content（字符串或 [{type:"text",text:...}]）归一化为纯文本。
std::string anthropic_content_to_text(const json& content) {
    if (content.is_string()) return content.get<std::string>();
    if (content.is_array()) {
        std::string out;
        for (const auto& block : content) {
            if (block.is_object() && block.value("type", "") == "text") {
                out += block.value("text", "");
            }
        }
        return out;
    }
    return "";
}

} // namespace

std::string extract_model(const std::string& client_body, ApiKind kind) {
    try {
        const auto j = json::parse(client_body);
        if (j.contains("model") && j["model"].is_string()) {
            return j["model"].get<std::string>();
        }
    } catch (...) {}
    return kind == ApiKind::Anthropic ? "claude" : "gpt-3.5-turbo";
}

bool to_upstream_body(const std::string& client_body, ApiKind kind,
                      std::string& out, std::string& err) {
    try {
        const auto j = json::parse(client_body);

        json messages = json::array();
        if (j.contains("messages") && j["messages"].is_array()) {
            for (const auto& m : j["messages"]) {
                if (!m.is_object() || !m.contains("role")) continue;
                json item;
                item["role"] = m["role"];
                if (kind == ApiKind::Anthropic) {
                    item["content"] = anthropic_content_to_text(
                        m.contains("content") ? m["content"] : json(""));
                } else {
                    item["content"] = m.contains("content")
                        ? (m["content"].is_string()
                               ? m["content"].get<std::string>()
                               : m["content"].dump())
                        : "";
                }
                messages.push_back(std::move(item));
            }
        }
        if (messages.empty()) {
            err = "messages 不能为空";
            return false;
        }

        // 组装上游 miku-chat 请求体
        json up;
        up["messages"] = std::move(messages);
        up["passiveVocaloidLookups"] = json::array();
        up["lyricContexts"] = json::array();
        up["memoryBrief"] = json::object();

        out = up.dump();
        return true;
    } catch (const std::exception& e) {
        err = std::string("请求体 JSON 解析失败: ") + e.what();
        return false;
    }
}

bool to_standard_body(const std::string& upstream_body, ApiKind kind,
                      const std::string& model,
                      std::string& out, std::string& err) {
    try {
        const auto j = json::parse(upstream_body);
        if (!j.contains("reply")) {
            err = "上游响应缺少 reply 字段";
            return false;
        }
        const std::string reply = j["reply"].is_string() ? j["reply"].get<std::string>()
                                                         : j["reply"].dump();

        const int64_t created = static_cast<int64_t>(std::time(nullptr));

        if (kind == ApiKind::OpenAi) {
            json res;
            res["id"] = "chatcmpl-" + random_suffix();
            res["object"] = "chat.completion";
            res["created"] = created;
            res["model"] = model;
            json choice;
            choice["index"] = 0;
            json msg;
            msg["role"] = "assistant";
            msg["content"] = reply;
            choice["message"] = std::move(msg);
            choice["finish_reason"] = "stop";
            res["choices"] = json::array({std::move(choice)});
            json usage;
            usage["prompt_tokens"] = 0;
            usage["completion_tokens"] = static_cast<int64_t>(reply.size());
            usage["total_tokens"] = static_cast<int64_t>(reply.size());
            res["usage"] = std::move(usage);
            out = res.dump();
            return true;
        }

        // Anthropic
        json res;
        res["id"] = "msg_" + random_suffix();
        res["type"] = "message";
        res["role"] = "assistant";
        json content;
        content["type"] = "text";
        content["text"] = reply;
        res["content"] = json::array({std::move(content)});
        res["model"] = model;
        res["stop_reason"] = "end_turn";
        res["stop_sequence"] = nullptr;
        json usage;
        usage["input_tokens"] = 0;
        usage["output_tokens"] = static_cast<int64_t>(reply.size());
        res["usage"] = std::move(usage);
        out = res.dump();
        return true;
    } catch (const std::exception& e) {
        err = std::string("上游响应 JSON 解析失败: ") + e.what();
        return false;
    }
}

} // namespace fp
