// auth/minecraft.rs — Minecraft Token 获取与档案查询
// 对应原项目 server/api/routes/accounts.js 中的 MC 部分（步骤 6-7）
//
// 流程：
//   [6] POST api.minecraftservices.com/authentication/login_with_xbox
//       用 XSTS Token 换取 MC access_token
//   [7] 验证游戏所有权 + 拉取档案
//       GET api.minecraftservices.com/entitlements/mcstore → 检查是否拥有 MC
//       GET api.minecraftservices.com/minecraft/profile → 获取 UUID、用户名、皮肤

use serde::Deserialize;
use serde_json::{json, Value};

use super::error::AuthError;
use super::xbox::XstsResult;

/// MC Token 响应（步骤 6）
#[derive(Debug, Clone, Deserialize)]
pub struct McTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// MC 档案响应（步骤 7）
#[derive(Debug, Clone, Deserialize)]
pub struct McProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<McSkin>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McSkin {
    pub id: String,
    pub state: String,
    pub url: String,
    pub variant: Option<String>,
}

/// 步骤 6：用 XSTS Token 换取 MC Token
/// POST https://api.minecraftservices.com/authentication/login_with_xbox
pub async fn login_minecraft(
    client: &reqwest::Client,
    xsts: &XstsResult,
) -> Result<String, AuthError> {
    let url = "https://api.minecraftservices.com/authentication/login_with_xbox";
    let body = json!({
        "identityToken": format!("XBL3.0 x={};{}", xsts.uhs, xsts.token)
    });

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("MC 登录请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.as_u16() == 429 {
        return Err(AuthError::RateLimit(10));
    }

    if !status.is_success() {
        return Err(AuthError::Network(format!(
            "MC 登录 HTTP {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    let parsed: McTokenResponse = serde_json::from_str(&text)
        .map_err(|e| AuthError::Network(format!("MC 登录响应解析失败: {}", e)))?;

    Ok(parsed.access_token)
}

/// 步骤 7a：验证游戏所有权
/// GET https://api.minecraftservices.com/entitlements/mcstore
pub async fn check_entitlements(
    client: &reqwest::Client,
    mc_token: &str,
) -> Result<bool, AuthError> {
    let url = "https://api.minecraftservices.com/entitlements/mcstore";
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", mc_token))
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("所有权验证请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.as_u16() == 429 {
        return Err(AuthError::RateLimit(10));
    }

    if !status.is_success() {
        return Err(AuthError::Network(format!(
            "所有权验证 HTTP {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    // 解析响应：只要 items 非空即视为持有（兼容 Xbox Game Pass）
    #[derive(Deserialize)]
    struct EntitlementsResponse {
        #[serde(default)]
        items: Vec<Value>,
    }

    let parsed: EntitlementsResponse = serde_json::from_str(&text)
        .map_err(|e| AuthError::Network(format!("所有权响应解析失败: {}", e)))?;

    let has_mc = !parsed.items.is_empty();

    Ok(has_mc)
}

/// 步骤 7b：拉取 Minecraft 档案
/// GET https://api.minecraftservices.com/minecraft/profile
pub async fn fetch_profile(
    client: &reqwest::Client,
    mc_token: &str,
) -> Result<McProfile, AuthError> {
    let url = "https://api.minecraftservices.com/minecraft/profile";
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", mc_token))
        .send()
        .await
        .map_err(|e| AuthError::Network(format!("档案请求失败: {}", e)))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.as_u16() == 429 {
        return Err(AuthError::RateLimit(10));
    }

    if status.as_u16() == 404 {
        // 没有创建 MC 档案
        return Err(AuthError::NeedCreateProfile);
    }

    if !status.is_success() {
        return Err(AuthError::Network(format!(
            "档案 HTTP {}: {}",
            status,
            text.chars().take(200).collect::<String>()
        )));
    }

    let parsed: McProfile = serde_json::from_str(&text)
        .map_err(|e| AuthError::Network(format!("档案响应解析失败: {}", e)))?;

    Ok(parsed)
}
