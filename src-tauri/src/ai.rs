// ai.rs — AI 对话代理
// 在 Rust 后端发起 AI API 请求，绕过 WebView 的 CORS 限制
// 支持 openai / anthropic / google 三种接口格式
// 对应原项目 main/ai-proxy.js 的 ai:chat 逻辑

use serde_json::{json, Value};
use std::time::Duration;

/// 根据接口格式构造请求体
fn build_body(format: &str, model: &str, messages: &Value, max_tokens: u64) -> Value {
    let tokens = if max_tokens > 0 { max_tokens } else { 1024 };
    match format {
        "anthropic" => {
            let mut sys_msg = String::new();
            let mut user_msgs: Vec<Value> = Vec::new();
            if let Some(arr) = messages.as_array() {
                for m in arr {
                    let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if role == "system" {
                        sys_msg.push_str(content);
                        sys_msg.push('\n');
                    } else {
                        user_msgs.push(json!({ "role": role, "content": content }));
                    }
                }
            }
            json!({
                "model": model,
                "max_tokens": tokens,
                "system": sys_msg.trim(),
                "messages": user_msgs
            })
        }
        "google" => {
            let mut contents: Vec<Value> = Vec::new();
            if let Some(arr) = messages.as_array() {
                for m in arr {
                    let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    if role == "system" {
                        contents.push(json!({ "role": "user", "parts": [{ "text": content }] }));
                    } else {
                        let g_role = if role == "assistant" { "model" } else { "user" };
                        contents.push(json!({ "role": g_role, "parts": [{ "text": content }] }));
                    }
                }
            }
            json!({ "contents": contents, "generationConfig": { "maxOutputTokens": tokens } })
        }
        _ => {
            // openai 格式
            json!({ "model": model, "messages": messages, "max_tokens": tokens })
        }
    }
}

/// 从响应中提取回复文本
fn extract_reply(format: &str, data: &Value) -> String {
    let empty = "(空回复)";
    match format {
        "anthropic" => {
            if let Some(content) = data.get("content").and_then(|v| v.as_array()) {
                if let Some(first) = content.first() {
                    if let Some(text) = first.get("text").and_then(|v| v.as_str()) {
                        return text.to_string();
                    }
                }
            }
            empty.to_string()
        }
        "google" => {
            if let Some(candidates) = data.get("candidates").and_then(|v| v.as_array()) {
                if let Some(first) = candidates.first() {
                    if let Some(parts) = first
                        .get("content")
                        .and_then(|v| v.get("parts"))
                        .and_then(|v| v.as_array())
                    {
                        if let Some(p) = parts.first() {
                            if let Some(text) = p.get("text").and_then(|v| v.as_str()) {
                                return text.to_string();
                            }
                        }
                    }
                }
            }
            empty.to_string()
        }
        _ => {
            // openai 格式
            if let Some(choices) = data.get("choices").and_then(|v| v.as_array()) {
                if let Some(first) = choices.first() {
                    if let Some(text) = first
                        .get("message")
                        .and_then(|v| v.get("content"))
                        .and_then(|v| v.as_str())
                    {
                        return text.to_string();
                    }
                }
            }
            empty.to_string()
        }
    }
}

/// 将 AI 服务商返回的错误转换为用户能看懂的中文提示
fn friendly_error(status: u16, body: &str) -> String {
    let raw = &body[..body.len().min(300)];
    let lower = raw.to_lowercase();

    // 余额不足 / 配额用尽
    if status == 402
        || lower.contains("insufficient_balance")
        || lower.contains("insufficient balance")
        || lower.contains("insufficient_quota")
        || lower.contains("insufficient quota")
    {
        return format!("AI 账户余额不足，请登录对应服务商官网充值后重试（错误码 {}）", status);
    }
    // API Key 无效或过期
    if status == 401
        || lower.contains("invalid api key")
        || lower.contains("invalid_api_key")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
    {
        return format!(
            "API Key 无效或已过期，请在设置中检查并重新填写正确的 API Key（错误码 {}）",
            status
        );
    }
    // 权限不足 / 内容被拒绝
    if status == 403 || lower.contains("permission_denied") || lower.contains("forbidden") {
        if lower.contains("content") && lower.contains("filter") {
            return format!(
                "请求内容被 AI 服务商拒绝（可能涉及违规内容），请换一种说法重试（错误码 {}）",
                status
            );
        }
        return format!(
            "API Key 权限不足，请检查该 Key 是否有对应模型的调用权限（错误码 {}）",
            status
        );
    }
    // 请求过于频繁 / 限流
    if status == 429 || lower.contains("rate_limit") || lower.contains("rate limit") || lower.contains("too many requests") {
        return format!("请求过于频繁，已触发限流，请稍等几秒后重试（错误码 {}）", status);
    }
    // 接口地址或模型不存在
    if status == 404
        || lower.contains("model not found")
        || lower.contains("model_not_found")
        || lower.contains("does not exist")
    {
        return format!(
            "接口地址或模型名称不正确，请检查供应商配置和所选模型是否匹配（错误码 {}）",
            status
        );
    }
    if status == 408 {
        return format!("AI 请求超时，请检查网络连接后重试（错误码 {}）", status);
    }
    if status == 413
        || lower.contains("too large")
        || lower.contains("maximum context")
        || lower.contains("context length")
    {
        return format!(
            "对话内容过长，超出了 AI 模型的处理上限，请缩短内容后重试（错误码 {}）",
            status
        );
    }
    if status == 400
        || lower.contains("invalid_request")
        || lower.contains("invalid request")
        || lower.contains("bad request")
    {
        return format!(
            "请求参数有误，可能是模型名称或消息格式不正确（错误码 {}）",
            status
        );
    }
    if status >= 500 {
        return format!("AI 服务商暂时不可用，请稍后重试（错误码 {}）", status);
    }
    format!("AI 请求失败（错误码 {}）：{}", status, raw)
}

/// AI 对话代理命令
/// 前端通过 window.electronAPI.ai.chat(reqConfig) 调用
/// reqConfig: { provider, apiKey, model, messages, endpoint, apiFormat, maxTokens, timeout }
#[tauri::command]
pub async fn ai_chat(config: Value) -> Value {
    let cfg = match config.as_object() {
        Some(c) => c,
        None => return json!({ "ok": false, "error": "请求参数格式错误" }),
    };

    let provider = cfg.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let api_key = cfg.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let model = cfg.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let endpoint = cfg.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
    let format = cfg.get("apiFormat").and_then(|v| v.as_str()).unwrap_or("openai").to_string();
    let messages = cfg.get("messages").cloned().unwrap_or(json!([]));
    let max_tokens = cfg.get("maxTokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let timeout = cfg.get("timeout").and_then(|v| v.as_u64()).unwrap_or(60000);

    if provider.is_empty() && endpoint.is_empty() {
        return json!({ "ok": false, "error": "未配置供应商" });
    }
    if api_key.is_empty() {
        return json!({ "ok": false, "error": "未配置 API Key" });
    }
    if model.is_empty() {
        return json!({ "ok": false, "error": "未选择模型" });
    }

    // 构造 URL 与请求头
    let mut url = String::new();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );

    if provider == "custom" || !endpoint.is_empty() {
        url = endpoint.to_string();
        if format == "anthropic" {
            headers.insert("x-api-key", reqwest::header::HeaderValue::from_str(api_key).unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")));
            headers.insert("anthropic-version", reqwest::header::HeaderValue::from_static("2023-06-01"));
        } else if format == "google" {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{}{}key={}", url, sep, api_key);
        } else {
            let bearer = format!("Bearer {}", api_key);
            headers.insert(reqwest::header::AUTHORIZATION, reqwest::header::HeaderValue::from_str(&bearer).unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")));
            if !url.ends_with("/chat/completions") && !url.ends_with("/completions") {
                while url.ends_with('/') { url.pop(); }
                url.push_str("/chat/completions");
            }
        }
    } else if provider == "google" || format == "google" {
        url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, api_key
        );
    } else if provider == "anthropic" || format == "anthropic" {
        url = if endpoint.is_empty() {
            "https://api.anthropic.com/v1/messages".to_string()
        } else {
            endpoint.to_string()
        };
        headers.insert("x-api-key", reqwest::header::HeaderValue::from_str(api_key).unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")));
        headers.insert("anthropic-version", reqwest::header::HeaderValue::from_static("2023-06-01"));
    } else {
        url = endpoint.to_string();
        let bearer = format!("Bearer {}", api_key);
        headers.insert(reqwest::header::AUTHORIZATION, reqwest::header::HeaderValue::from_str(&bearer).unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")));
        if url.is_empty() {
            return json!({ "ok": false, "error": "供应商缺少接口地址" });
        }
    }

    if url.is_empty() {
        return json!({ "ok": false, "error": "供应商缺少接口地址" });
    }

    let body = build_body(&format, model, &messages, max_tokens);

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout.min(180_000).max(5_000)))
        .danger_accept_invalid_certs(true)
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({ "ok": false, "error": format!("HTTP 客户端初始化失败: {}", e) }),
    };

    let resp = match client.post(&url).headers(headers).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            return json!({ "ok": false, "error": format!("AI 请求失败: {}", e) });
        }
    };

    let status = resp.status().as_u16();
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => return json!({ "ok": false, "error": format!("读取响应失败: {}", e) }),
    };

    if status >= 400 {
        return json!({ "ok": false, "error": friendly_error(status, &text) });
    }

    let data: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => {
            return json!({ "ok": false, "error": format!("AI 返回非 JSON：{}", &text[..text.len().min(200)]) });
        }
    };

    let reply = extract_reply(&format, &data);
    json!({ "ok": true, "reply": reply })
}
