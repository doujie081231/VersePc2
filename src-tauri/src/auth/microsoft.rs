// auth/microsoft.rs — 微软 OAuth 设备代码流
// 对应原项目 server/api/routes/accounts.js 中的 OAuth 部分（步骤 1-3）
//
// 流程：
//   [1] POST /devicecode → 返回 device_code, user_code, verification_uri
//   [2] 用户在浏览器输入 user_code
//   [3] POST /token (grant_type=device_code) → ms_access_token, ms_refresh_token
//        若 error="authorization_pending" → 等待重试
//        若 error="expired_token" → 设备码过期
//        若 error="slow_down" → 速率过快，等待更久

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::error::AuthError;

/// OAuth 2.0 端点（设备代码流）
pub const MS_CLIENT_ID: &str = "b5b442d7-5978-4637-a81e-88faf9bfc8a6";
pub const OAUTH_AUTHORIZE_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
pub const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";

/// 设备码响应（步骤 1）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// 微软会返回预填好 user_code 的完整链接（如 https://www.microsoft.com/link?otc=xxx）
    /// 前端优先使用这个链接直接打开浏览器，省去用户手动输入的步骤
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Token 响应（步骤 3，poll 返回）
#[derive(Debug, Clone, Deserialize)]
pub struct MsTokenResponse {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    /// 错误时填：authorization_pending / expired_token / slow_down
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// 步骤 1：获取设备码
/// POST {OAUTH_AUTHORIZE_BASE}/devicecode
pub async fn request_device_code(client: &reqwest::Client) -> Result<DeviceCodeResponse, AuthError> {
    let url = format!("{}/devicecode", OAUTH_AUTHORIZE_BASE);
    let body = format!(
        "client_id={}&scope={}",
        MS_CLIENT_ID,
        urlencoding::encode(OAUTH_SCOPE)
    );

    let resp = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("devicecode 请求失败: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AuthError::Network(format!(
            "devicecode HTTP {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    resp.json::<DeviceCodeResponse>()
        .await
        .map_err(|e| AuthError::Network(format!("devicecode 响应解析失败: {}", e)))
}

/// 步骤 3：用 device_code 轮询获取 Token
/// POST {OAUTH_AUTHORIZE_BASE}/token
/// grant_type=urn:ietf:params:oauth:grant-type:device_code
pub async fn poll_device_code(
    client: &reqwest::Client,
    device_code: &str,
) -> Result<MsTokenResponse, AuthError> {
    let url = format!("{}/token", OAUTH_AUTHORIZE_BASE);
    let body = format!(
        "grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id={}&device_code={}",
        MS_CLIENT_ID,
        urlencoding::encode(device_code)
    );

    let resp = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("token 请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    // 解析响应（OAuth 错误响应也是 JSON 体）
    let parsed: MsTokenResponse = serde_json::from_str(&text).map_err(|e| {
        AuthError::Network(format!(
            "token 响应解析失败 (HTTP {}): {}",
            status,
            e
        ))
    })?;

    // 错误处理
    if let Some(err) = parsed.error.as_ref() {
        return match err.as_str() {
            "authorization_pending" => Err(AuthError::AuthorizationPending),
            "expired_token" => Err(AuthError::DeviceCodeExpired),
            "slow_down" => Err(AuthError::RateLimit(10)),
            "authorization_declined" => Err(AuthError::AuthorizationDeclined),
            "invalid_grant" | "expired_token" => Err(AuthError::TokenExpired),
            _ => Err(AuthError::Other(format!(
                "OAuth 错误: {} - {}",
                err,
                parsed.error_description.unwrap_or_default()
            ))),
        };
    }

    if parsed.access_token.is_none() || parsed.refresh_token.is_none() {
        return Err(AuthError::Other("响应缺少 access_token 或 refresh_token".to_string()));
    }

    Ok(parsed)
}

/// 刷新 Token
/// POST {OAUTH_AUTHORIZE_BASE}/token
/// grant_type=refresh_token
pub async fn refresh_ms_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<MsTokenResponse, AuthError> {
    let url = format!("{}/token", OAUTH_AUTHORIZE_BASE);
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}&scope={}",
        MS_CLIENT_ID,
        urlencoding::encode(refresh_token),
        urlencoding::encode(OAUTH_SCOPE)
    );

    let resp = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("refresh 请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let parsed: MsTokenResponse = serde_json::from_str(&text).map_err(|e| {
        AuthError::Network(format!(
            "refresh 响应解析失败 (HTTP {}): {}",
            status,
            e
        ))
    })?;

    if let Some(err) = parsed.error.as_ref() {
        return match err.as_str() {
            "invalid_grant" | "expired_token" => Err(AuthError::TokenExpired),
            "slow_down" => Err(AuthError::RateLimit(10)),
            _ => Err(AuthError::Other(format!(
                "refresh 错误: {} - {}",
                err,
                parsed.error_description.unwrap_or_default()
            ))),
        };
    }

    if parsed.access_token.is_none() || parsed.refresh_token.is_none() {
        return Err(AuthError::Other("刷新响应缺少 token".to_string()));
    }

    Ok(parsed)
}

/// 简单的 URL 编码（避免引入 urlencoding crate）
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len() * 3);
        for &b in s.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(b as char);
                }
                b' ' => result.push_str("+"),
                _ => result.push_str(&format!("%{:02X}", b)),
            }
        }
        result
    }
}

/// 构造 MsTokenResponse 的 JSON 序列化（供调试）
pub fn token_response_to_json(resp: &MsTokenResponse) -> serde_json::Value {
    json!({
        "hasAccessToken": resp.access_token.is_some(),
        "hasRefreshToken": resp.refresh_token.is_some(),
        "expiresIn": resp.expires_in,
        "error": resp.error,
        "errorDescription": resp.error_description
    })
}
