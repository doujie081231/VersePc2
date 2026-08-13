// auth/mod.rs — 微软账号认证模块入口
// 对应原项目 server/api/routes/accounts.js 中微软账号登录的核心逻辑
//
// 完整流程：
//   [1] microsoft::request_device_code       → device_code, user_code, verification_uri
//   [2] 用户在浏览器输入 user_code（前端处理）
//   [3] microsoft::poll_device_code          → ms_access_token, ms_refresh_token
//   [4] xbox::login_xbl                      → xbl_token, uhs
//   [5] xbox::get_xsts                       → xsts_token, xsts_uhs
//   [6] minecraft::login_minecraft           → mc_access_token
//   [7] minecraft::check_entitlements + fetch_profile → 验证所有权 + 档案

pub mod error;
pub mod minecraft;
pub mod microsoft;
pub mod token;
pub mod xbox;

use serde_json::{json, Value};

use self::error::AuthError;
use self::minecraft::McProfile;
use self::microsoft::{MsTokenResponse, DeviceCodeResponse};
use self::xbox::XstsResult;

/// 完整登录结果（poll 端点成功后返回）
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub mc_access_token: String,
    pub ms_refresh_token: String,
    pub mc_expires_at: u64,
    pub profile: McProfile,
}

/// 创建共享的 reqwest 客户端
/// 启用 rustls 避免系统 OpenSSL 链接问题
pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .user_agent("VersePC/1.0")
        .build()
        .expect("failed to build reqwest client")
}

/// 完整登录流程（步骤 3-7）
/// 输入：device_code（步骤 1 已获取，步骤 2 用户已授权）
/// 对应原项目 /api/msauth/poll 的核心逻辑
pub async fn complete_login(
    client: &reqwest::Client,
    device_code: &str,
) -> Result<AuthResult, AuthError> {
    // 步骤 3：用 device_code 换取 ms_access_token 和 ms_refresh_token
    let ms_token: MsTokenResponse = microsoft::poll_device_code(client, device_code).await?;
    let ms_access_token = ms_token.access_token.ok_or_else(|| {
        AuthError::Other("OAuth 响应缺少 access_token".to_string())
    })?;
    let ms_refresh_token = ms_token.refresh_token.ok_or_else(|| {
        AuthError::Other("OAuth 响应缺少 refresh_token".to_string())
    })?;

    // 步骤 4-5：XBL + XSTS
    let (xbl_token, _xbl_uhs) = xbox::login_xbl(client, &ms_access_token).await?;
    let xsts: XstsResult = xbox::get_xsts(client, &xbl_token).await?;

    // 步骤 6：MC Token
    let mc_access_token = minecraft::login_minecraft(client, &xsts).await?;

    // 步骤 7a：验证游戏所有权
    let has_mc = minecraft::check_entitlements(client, &mc_access_token).await?;
    if !has_mc {
        return Err(AuthError::NeedPurchase);
    }

    // 步骤 7b：拉取档案
    let profile = minecraft::fetch_profile(client, &mc_access_token).await?;

    // 计算 MC Token 过期时间（默认 24 小时）
    let mc_expires_at = now_millis() + 24 * 60 * 60 * 1000;

    Ok(AuthResult {
        mc_access_token,
        ms_refresh_token,
        mc_expires_at,
        profile,
    })
}

/// 刷新登录（步骤 3' - 7）
/// 输入：refresh_token（之前登录保存的）
/// 对应原项目 /api/msauth/refresh 的核心逻辑
pub async fn refresh_login(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<AuthResult, AuthError> {
    // 步骤 3'：用 refresh_token 换取新的 access_token 和 refresh_token
    let ms_token = microsoft::refresh_ms_token(client, refresh_token).await?;
    let ms_access_token = ms_token
        .access_token
        .ok_or_else(|| AuthError::Other("refresh 响应缺少 access_token".to_string()))?;
    let ms_refresh_token = ms_token
        .refresh_token
        .ok_or_else(|| AuthError::Other("refresh 响应缺少 refresh_token".to_string()))?;

    // 步骤 4-5：XBL + XSTS
    let (xbl_token, _) = xbox::login_xbl(client, &ms_access_token).await?;
    let xsts = xbox::get_xsts(client, &xbl_token).await?;

    // 步骤 6：MC Token
    let mc_access_token = minecraft::login_minecraft(client, &xsts).await?;

    // 步骤 7：验证 + 档案
    let has_mc = minecraft::check_entitlements(client, &mc_access_token).await?;
    if !has_mc {
        return Err(AuthError::NeedPurchase);
    }
    let profile = minecraft::fetch_profile(client, &mc_access_token).await?;

    let mc_expires_at = now_millis() + 24 * 60 * 60 * 1000;

    Ok(AuthResult {
        mc_access_token,
        ms_refresh_token,
        mc_expires_at,
        profile,
    })
}

/// 把 AuthResult 转换为 accounts.json 中的一条记录
/// accessToken 和 refreshToken 用加密格式存储
pub fn auth_result_to_account(
    auth: &AuthResult,
    account_id: String,
) -> Value {
    let encrypted_access = token::encrypt_account_token(&auth.mc_access_token);
    let encrypted_refresh = token::encrypt_account_token(&auth.ms_refresh_token);

    let skin_url = auth
        .profile
        .skins
        .iter()
        .find(|s| s.state == "ACTIVE")
        .map(|s| s.url.clone())
        .unwrap_or_default();
    let skin_model = auth
        .profile
        .skins
        .iter()
        .find(|s| s.state == "ACTIVE")
        .and_then(|s| s.variant.clone())
        .unwrap_or_else(|| "default".to_string());

    json!({
        "id": account_id,
        "username": auth.profile.name,
        "uuid": auth.profile.id,
        "type": "microsoft",
        "accessToken": encrypted_access,
        "refreshToken": encrypted_refresh,
        "tokenExpiresAt": auth.mc_expires_at,
        "lastRefreshed": now_iso(),
        "createdAt": now_iso(),
        "skinUrl": skin_url,
        "skinModel": skin_model
    })
}

/// 当前毫秒时间戳
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// ISO 8601 时间字符串
fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简化的 ISO 格式（不带时区）
    format!("{}", now)
}
