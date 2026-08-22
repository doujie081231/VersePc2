// auth/xbox.rs — Xbox Live (XBL) 和 XSTS Token 认证
//
// 流程：
//   [4] XBL 登录：POST user.auth.xboxlive.com/user/authenticate
//       用 ms_access_token 换取 xbl_token (Token) 和 uhs (DisplayClaims.xui[0].uhs)
//   [5] XSTS 获取：POST xsts.auth.xboxlive.com/xsts/authorize
//       用 xbl_token 换取 xsts_token 和 xsts_uhs
//       若返回 XErr 字段：账号有问题（无Xbox账号/地区不可用/封禁等）

use serde::Deserialize;
use serde_json::json;

use super::error::{xerr_message, AuthError};

/// XBL 登录响应（步骤 4）
#[derive(Debug, Clone, Deserialize)]
pub struct XblResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XblDisplayClaims,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XblDisplayClaims {
    pub xui: Vec<XblUhs>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XblUhs {
    pub uhs: String,
}

/// XSTS 响应（步骤 5）
#[derive(Debug, Clone, Deserialize)]
pub struct XstsResponse {
    #[serde(rename = "Token")]
    pub token: Option<String>,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: Option<XstsDisplayClaims>,
    /// XErr：错误时返回（账号问题）
    #[serde(rename = "XErr")]
    pub xerr: Option<u64>,
    #[serde(rename = "Message")]
    pub message: Option<String>,
    #[serde(rename = "Redirect")]
    pub redirect: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XstsDisplayClaims {
    pub xui: Vec<XblUhs>,
}

/// XSTS 完整结果（包含 token 和 uhs）
#[derive(Debug, Clone)]
pub struct XstsResult {
    pub token: String,
    pub uhs: String,
}

/// 步骤 4：XBL 登录
/// POST https://user.auth.xboxlive.com/user/authenticate
pub async fn login_xbl(
    client: &reqwest::Client,
    ms_access_token: &str,
) -> Result<(String, String), AuthError> {
    let url = "https://user.auth.xboxlive.com/user/authenticate";
    let rps_ticket = if ms_access_token.starts_with("d=") {
        ms_access_token.to_string()
    } else {
        format!("d={}", ms_access_token)
    };
    let body = json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": rps_ticket
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("x-xbl-contract-version", "2")
        .json(&body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("XBL 请求失败: {}", e)))?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp.text().await.unwrap_or_default();

    // 429 速率限制
    if status.as_u16() == 429 {
        let retry_after = headers
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);
        return Err(AuthError::RateLimit(retry_after));
    }

    if !status.is_success() {
        return Err(AuthError::Network(format!(
            "XBL HTTP {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    let parsed: XblResponse = serde_json::from_str(&text)
        .map_err(|e| AuthError::Network(format!("XBL 响应解析失败: {}", e)))?;

    let uhs = parsed
        .display_claims
        .xui
        .first()
        .map(|x| x.uhs.clone())
        .ok_or_else(|| AuthError::Other("XBL 响应缺少 uhs".to_string()))?;

    Ok((parsed.token, uhs))
}

/// 步骤 5：XSTS Token 获取
/// POST https://xsts.auth.xboxlive.com/xsts/authorize
pub async fn get_xsts(
    client: &reqwest::Client,
    xbl_token: &str,
) -> Result<XstsResult, AuthError> {
    let url = "https://xsts.auth.xboxlive.com/xsts/authorize";
    let body = json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("x-xbl-contract-version", "2")
        .json(&body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("XSTS 请求失败: {}", e)))?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp.text().await.unwrap_or_default();

    // 429 速率限制
    if status.as_u16() == 429 {
        let retry_after = headers
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);
        return Err(AuthError::RateLimit(retry_after));
    }

    // 解析响应（注意 XErr 字段可能在不同状态码下返回）
    let parsed: XstsResponse = serde_json::from_str(&text).map_err(|e| {
        AuthError::Network(format!(
            "XSTS 响应解析失败 (HTTP {}): {}",
            status,
            e
        ))
    })?;

    // 检查 XErr
    if let Some(xerr) = parsed.xerr {
        let message = parsed
            .message
            .clone()
            .unwrap_or_else(|| xerr_message(xerr).to_string());
        return Err(AuthError::XErr(xerr, message));
    }

    let token = parsed
        .token
        .ok_or_else(|| AuthError::Other("XSTS 响应缺少 Token".to_string()))?;
    let uhs = parsed
        .display_claims
        .and_then(|dc| dc.xui.into_iter().next().map(|x| x.uhs))
        .ok_or_else(|| AuthError::Other("XSTS 响应缺少 uhs".to_string()))?;

    Ok(XstsResult { token, uhs })
}
