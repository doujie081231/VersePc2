// api/accounts.rs — 账号相关路由
// 兼容原项目 server/api/routes/accounts.js
// 路由清单：
//   GET  /api/accounts                              获取账号列表
//   POST /api/accounts/add-offline                  添加离线账号
//   POST /api/accounts/delete                       删除账号
//   POST /api/accounts/select                       选择当前账号
//   GET  /api/accounts/thirdparty-verify            验证第三方认证服务器
//   POST /api/accounts/thirdparty-login             第三方账号登录（Yggdrasil）
//   POST /api/accounts/thirdparty-select-profile    第三方多角色选择
//
// 注：微软登录 (/api/msauth/*) 在 msauth.rs 实现，由 mod.rs 优先分发

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use super::authlib;
use crate::api::ApiResult;
use crate::storage;
use crate::utils;

/// 处理账号路由
pub async fn handle(
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/accounts" => Some(ApiResult::ok(storage::load_accounts())),

        "POST /api/accounts/add-offline" => Some(handle_add_offline(body)),

        "POST /api/accounts/delete" => Some(handle_delete(body)),

        "POST /api/accounts/select" => Some(handle_select(body)),

        "GET /api/accounts/thirdparty-verify" => Some(handle_thirdparty_verify(params).await),

        "POST /api/accounts/thirdparty-login" => Some(handle_thirdparty_login(body).await),

        "POST /api/accounts/thirdparty-select-profile" => {
            Some(handle_thirdparty_select_profile(body).await)
        }

        _ => None,
    }
}

// ============================================================================
// 离线账号
// ============================================================================

/// POST /api/accounts/add-offline — 添加离线账号
fn handle_add_offline(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let username = utils::get_str_trim(&data, "username");

    if username.is_empty() {
        return ApiResult::err(400, "请输入用户名");
    }
    if username.len() < 3 || username.len() > 16 {
        return ApiResult::err(400, "用户名长度需为 3 - 16 位");
    }
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return ApiResult::err(400, "用户名只能包含英文字母、数字与下划线");
    }

    let uuid = utils::offline_uuid(&username);
    let new_account = json!({
        "id": utils::generate_simple_uuid(),
        "username": username,
        "uuid": uuid,
        "type": "offline",
        "accessToken": "0",
        "skinFile": "steve_skin.png",
        "skinModel": "default",
        "createdAt": utils::now_iso()
    });

    let mut accounts = storage::load_accounts();
    if let Some(arr) = accounts.as_array_mut() {
        arr.push(new_account.clone());
    }
    storage::save_accounts(&accounts);
    ApiResult::ok(json!({ "success": true, "account": new_account }))
}

/// POST /api/accounts/delete — 删除账号
fn handle_delete(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    if account_id.is_empty() {
        return ApiResult::err(400, "Missing accountId");
    }
    let mut accounts = storage::load_accounts();
    if let Some(arr) = accounts.as_array_mut() {
        arr.retain(|a| utils::get_str(a, "id") != account_id);
    }
    storage::save_accounts(&accounts);
    ApiResult::ok(json!({ "success": true }))
}

/// POST /api/accounts/select — 选择当前账号
fn handle_select(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let mut settings = storage::load_settings();
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("selectedAccount".to_string(), json!(account_id));
    }
    storage::overwrite_settings(&settings);
    ApiResult::ok(json!({ "success": true }))
}

// ============================================================================
// 第三方账号 (Yggdrasil / authlib-injector)
// ============================================================================

/// GET /api/accounts/thirdparty-verify — 验证第三方认证服务器
///
/// 查询参数：
///   - serverUrl: 认证服务器 API 根地址（必须 https://）
///
/// 返回：{ success, meta: { serverName, implementationName, implementationVersion, serverIcon } }
async fn handle_thirdparty_verify(params: &Option<Value>) -> ApiResult {
    let server_url = params
        .as_ref()
        .and_then(|p| p.get("serverUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();

    if server_url.is_empty() {
        return ApiResult::err(400, "Missing serverUrl");
    }
    if !server_url.starts_with("https://") {
        return ApiResult::ok(json!({
            "success": false,
            "error": "出于安全考虑，仅支持 HTTPS 协议的认证服务器"
        }));
    }

    let client = match build_http_client(10) {
        Ok(c) => c,
        Err(e) => return ApiResult::err(500, &e),
    };

    match client.get(&server_url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return ApiResult::ok(json!({
                    "success": false,
                    "error": format!("认证服务器返回 HTTP {}", resp.status())
                }));
            }
            let info: Value = match resp.json().await {
                Ok(v) => v,
                Err(_) => {
                    return ApiResult::ok(json!({
                        "success": false,
                        "error": "认证服务器返回了无效的响应"
                    }))
                }
            };

            let meta = json!({
                "serverName": info.get("meta")
                    .and_then(|m| m.get("serverName"))
                    .and_then(|v| v.as_str())
                    .or_else(|| info.get("serverName").and_then(|v| v.as_str()))
                    .unwrap_or("未知"),
                "implementationName": info.get("meta")
                    .and_then(|m| m.get("implementationName"))
                    .and_then(|v| v.as_str())
                    .or_else(|| info.get("implementationName").and_then(|v| v.as_str()))
                    .unwrap_or(""),
                "implementationVersion": info.get("meta")
                    .and_then(|m| m.get("implementationVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                "serverIcon": info.get("meta")
                    .and_then(|m| m.get("serverIcon"))
                    .and_then(|v| v.as_str())
                    .or_else(|| info.get("icon").and_then(|v| v.as_str()))
                    .unwrap_or("")
            });

            ApiResult::ok(json!({ "success": true, "meta": meta }))
        }
        Err(e) => ApiResult::ok(json!({
            "success": false,
            "error": format!("无法连接到认证服务器: {}", e)
        })),
    }
}

/// POST /api/accounts/thirdparty-login — 第三方账号登录
///
/// 请求体：{ serverUrl, username, password }
///
/// 流程：
///   1. 调用 /authserver/authenticate 获取 accessToken
///   2. 若 selectedProfile 缺失但有多个 availableProfiles，返回 needSelectProfile
///   3. 否则调用 /authserver/refresh 携带 selectedProfile，刷新令牌
///   4. 提取皮肤 URL 和模型
///   5. 调用 ensure_authlib_injector 确保 jar 就位
///   6. 写入账号列表，设为当前账号
async fn handle_thirdparty_login(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let server_url = utils::get_str(&data, "serverUrl").trim_end_matches('/').to_string();
    let username = utils::get_str(&data, "username");
    let password = utils::get_str(&data, "password");

    if server_url.is_empty() || username.is_empty() || password.is_empty() {
        return ApiResult::err(400, "Missing required fields");
    }
    if !server_url.starts_with("https://") {
        return ApiResult::ok(json!({
            "success": false,
            "error": "出于安全考虑，仅支持 HTTPS 协议的认证服务器"
        }));
    }

    let auth_url = format!("{}/authserver/authenticate", server_url);

    let auth_body = json!({
        "username": username,
        "password": password,
        "requestUser": true,
        "agent": { "name": "Minecraft", "version": 1 }
    });

    let auth_result = match yggdrasil_request(&auth_url, &auth_body, 30).await {
        Ok(v) => v,
        Err(e) => {
            return ApiResult::ok(json!({ "success": false, "error": format!("登录失败: {}", e) }))
        }
    };

    if auth_result.get("error").is_some() {
        let msg = auth_result
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .or_else(|| auth_result.get("error").and_then(|v| v.as_str()))
            .unwrap_or("认证失败")
            .to_string();
        return ApiResult::ok(json!({ "success": false, "error": msg }));
    }

    let access_token = match auth_result.get("accessToken").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return ApiResult::ok(json!({
                "success": false,
                "error": "认证服务器未返回访问令牌"
            }))
        }
    };
    let client_token = auth_result
        .get("clientToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut profile: Option<Value> = auth_result
        .get("selectedProfile")
        .and_then(|v| if v.is_null() { None } else { Some(v.clone()) });

    let available_profiles: Vec<Value> = auth_result
        .get("availableProfiles")
        .and_then(|v| v.as_array())
        .map(|arr| arr.clone())
        .unwrap_or_default();

    // 没有角色
    if profile.is_none() && available_profiles.is_empty() {
        return ApiResult::ok(json!({
            "success": false,
            "error": "未找到游戏角色，请先在皮肤站创建角色"
        }));
    }

    // 多角色，需要用户选择
    if profile.is_none() && available_profiles.len() > 1 {
        let profiles: Vec<Value> = available_profiles
            .iter()
            .map(|p| {
                let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
                json!({
                    "id": id,
                    "name": p.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "skinUrl": format!("https://mc-heads.net/avatar/{}/64", id.replace('-', ""))
                })
            })
            .collect();
        return ApiResult::ok(json!({
            "success": false,
            "needSelectProfile": true,
            "accessToken": access_token,
            "clientToken": client_token,
            "serverUrl": server_url,
            "availableProfiles": profiles
        }));
    }

    // 单角色，自动选中
    if profile.is_none() && available_profiles.len() == 1 {
        profile = Some(available_profiles[0].clone());
    }

    // 调用 /authserver/refresh 携带选中角色
    let mut final_auth_result = auth_result.clone();
    if let Some(prof) = &profile {
        let refresh_url = format!("{}/authserver/refresh", server_url);
        let refresh_body = json!({
            "accessToken": access_token,
            "clientToken": client_token,
            "selectedProfile": prof,
            "requestUser": true
        });
        if let Ok(refresh_result) = yggdrasil_request(&refresh_url, &refresh_body, 300).await {
            // 合并刷新结果
            if let Some(new_token) = refresh_result.get("accessToken").and_then(|v| v.as_str()) {
                final_auth_result["accessToken"] = json!(new_token);
            }
            if let Some(new_prof) = refresh_result.get("selectedProfile") {
                final_auth_result["selectedProfile"] = new_prof.clone();
                profile = Some(new_prof.clone());
            }
        }
    }

    let profile = match profile {
        Some(p) => p,
        None => {
            return ApiResult::ok(json!({
                "success": false,
                "error": "未找到游戏角色，请先在皮肤站创建角色"
            }))
        }
    };

    let skin_url = extract_skin_url_from_auth_result(&final_auth_result);
    let skin_model = extract_skin_model_from_auth_result(&final_auth_result);

    // 确保 authlib-injector jar 就位（与原项目一致）
    let _ = authlib::ensure_authlib_injector().await;

    let new_account = build_thirdparty_account(
        &profile,
        final_auth_result
            .get("accessToken")
            .and_then(|v| v.as_str())
            .unwrap_or(&access_token),
        &client_token,
        &server_url,
        skin_url,
        skin_model,
    );

    upsert_thirdparty_account(&new_account);
    select_account(&new_account);

    ApiResult::ok(json!({ "success": true, "account": new_account }))
}

/// POST /api/accounts/thirdparty-select-profile — 第三方多角色选择
///
/// 请求体：{ accessToken, clientToken, serverUrl, profileId, profileName }
///
/// 流程：
///   1. 调用 /authserver/refresh 携带 selectedProfile={profileId,profileName}
///   2. 提取皮肤 URL 和模型
///   3. 写入账号列表，设为当前账号
async fn handle_thirdparty_select_profile(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let access_token = utils::get_str(&data, "accessToken");
    let client_token = utils::get_str(&data, "clientToken");
    let server_url = utils::get_str(&data, "serverUrl").trim_end_matches('/').to_string();
    let profile_id = utils::get_str(&data, "profileId");
    let profile_name = utils::get_str(&data, "profileName");

    if access_token.is_empty() || profile_id.is_empty() || server_url.is_empty() {
        return ApiResult::err(400, "Missing required fields");
    }
    if !server_url.starts_with("https://") {
        return ApiResult::ok(json!({
            "success": false,
            "error": "出于安全考虑，仅支持 HTTPS 协议的认证服务器"
        }));
    }

    let refresh_url = format!("{}/authserver/refresh", server_url);
    let refresh_body = json!({
        "accessToken": access_token,
        "clientToken": client_token,
        "selectedProfile": { "id": profile_id, "name": profile_name },
        "requestUser": true
    });

    let refresh_result = match yggdrasil_request(&refresh_url, &refresh_body, 30).await {
        Ok(v) => v,
        Err(e) => {
            return ApiResult::ok(json!({
                "success": false,
                "error": format!("角色选择失败: {}", e)
            }))
        }
    };

    if refresh_result.get("error").is_some() {
        let msg = refresh_result
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .unwrap_or("角色选择失败")
            .to_string();
        return ApiResult::ok(json!({ "success": false, "error": msg }));
    }

    let profile = refresh_result
        .get("selectedProfile")
        .cloned()
        .unwrap_or_else(|| json!({ "id": profile_id, "name": profile_name }));

    let new_access_token = refresh_result
        .get("accessToken")
        .and_then(|v| v.as_str())
        .unwrap_or(&access_token)
        .to_string();

    let skin_url = extract_skin_url_from_auth_result(&refresh_result);
    let skin_model = extract_skin_model_from_auth_result(&refresh_result);

    let _ = authlib::ensure_authlib_injector().await;

    let new_account = build_thirdparty_account(
        &profile,
        &new_access_token,
        &client_token,
        &server_url,
        skin_url,
        skin_model,
    );

    upsert_thirdparty_account(&new_account);
    select_account(&new_account);

    ApiResult::ok(json!({ "success": true, "account": new_account }))
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 构造共享 HTTP 客户端（10s 超时，用于普通 Yggdrasil 请求）
fn build_http_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("VersePC/2.0")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))
}

/// 发起 Yggdrasil POST 请求，返回 JSON 响应
///
/// reqwest 默认支持重定向（最多 10 次），无需手动处理
async fn yggdrasil_request(
    url: &str,
    body: &Value,
    timeout_secs: u64,
) -> Result<Value, String> {
    let client = build_http_client(timeout_secs)?;

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Connection refused") {
                "无法连接到认证服务器，请检查服务器地址是否正确".to_string()
            } else if e.is_connect() || e.is_request() {
                format!("连接认证服务器失败: {}", e)
            } else {
                msg
            }
        })?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?;

    if !status.is_success() {
        // 尝试解析错误消息
        let err_msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("errorMessage")
                    .and_then(|v| v.as_str())
                    .or_else(|| v.get("error").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| format!("认证服务器返回 HTTP {}", status));
        return Err(err_msg);
    }

    serde_json::from_str::<Value>(&text).map_err(|_| "认证服务器返回了无效的响应".to_string())
}

/// 从认证结果中提取皮肤 URL
///
/// 皮肤 URL 在 textures 属性中，base64 编码的 JSON
/// 查找路径：selectedProfile.properties 或 user.properties 中的 textures 字段
fn extract_skin_url_from_auth_result(auth_result: &Value) -> Option<String> {
    let sources = [
        auth_result
            .get("selectedProfile")
            .and_then(|p| p.get("properties")),
        auth_result.get("user").and_then(|u| u.get("properties")),
    ];

    for properties in sources.iter().flatten() {
        if let Some(arr) = properties.as_array() {
            for prop in arr {
                if prop.get("name").and_then(|v| v.as_str()) == Some("textures") {
                    if let Some(value) = prop.get("value").and_then(|v| v.as_str()) {
                        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value) {
                            if let Ok(decoded_json) = serde_json::from_slice::<Value>(&decoded) {
                                if let Some(url) = decoded_json
                                    .get("textures")
                                    .and_then(|t| t.get("SKIN"))
                                    .and_then(|s| s.get("url"))
                                    .and_then(|v| v.as_str())
                                {
                                    return Some(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// 从认证结果中提取皮肤模型
///
/// 模型在 textures.SKIN.metadata.model 中，值为 "slim" 或 "default"
fn extract_skin_model_from_auth_result(auth_result: &Value) -> Option<String> {
    let sources = [
        auth_result
            .get("selectedProfile")
            .and_then(|p| p.get("properties")),
        auth_result.get("user").and_then(|u| u.get("properties")),
    ];

    for properties in sources.iter().flatten() {
        if let Some(arr) = properties.as_array() {
            for prop in arr {
                if prop.get("name").and_then(|v| v.as_str()) == Some("textures") {
                    if let Some(value) = prop.get("value").and_then(|v| v.as_str()) {
                        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value) {
                            if let Ok(decoded_json) = serde_json::from_slice::<Value>(&decoded) {
                                if let Some(model) = decoded_json
                                    .get("textures")
                                    .and_then(|t| t.get("SKIN"))
                                    .and_then(|s| s.get("metadata"))
                                    .and_then(|m| m.get("model"))
                                    .and_then(|v| v.as_str())
                                {
                                    return Some(
                                        if model == "slim" {
                                            "slim".to_string()
                                        } else {
                                            "default".to_string()
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// 构造第三方账号对象
fn build_thirdparty_account(
    profile: &Value,
    access_token: &str,
    client_token: &str,
    server_url: &str,
    skin_url: Option<String>,
    skin_model: Option<String>,
) -> Value {
    json!({
        "id": utils::generate_simple_uuid(),
        "username": profile.get("name").and_then(|v| v.as_str()).unwrap_or("Player"),
        "uuid": profile.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "type": "thirdparty",
        "accessToken": access_token,
        "clientToken": client_token,
        "serverUrl": server_url,
        "skinUrl": skin_url.unwrap_or_default(),
        "skinModel": skin_model.unwrap_or_else(|| "default".to_string()),
        "createdAt": utils::now_iso()
    })
}

/// 插入或更新第三方账号
///
/// 按 uuid + type='thirdparty' 匹配，存在则更新，不存在则追加
fn upsert_thirdparty_account(new_account: &Value) {
    let mut accounts = storage::load_accounts();
    let new_uuid = utils::get_str(new_account, "uuid");

    if let Some(arr) = accounts.as_array_mut() {
        let mut found = false;
        for acc in arr.iter_mut() {
            if utils::get_str(acc, "uuid") == new_uuid
                && utils::get_str(acc, "type") == "thirdparty"
            {
                // 保留原 createdAt
                let created_at = utils::get_str(acc, "createdAt");
                *acc = new_account.clone();
                if !created_at.is_empty() {
                    if let Some(obj) = acc.as_object_mut() {
                        obj.insert("createdAt".to_string(), json!(created_at));
                    }
                }
                found = true;
                break;
            }
        }
        if !found {
            arr.push(new_account.clone());
        }
    }
    storage::save_accounts(&accounts);
}

/// 设置当前账号
fn select_account(account: &Value) {
    let account_id = utils::get_str(account, "id");
    let mut settings = storage::load_settings();
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("selectedAccount".to_string(), json!(account_id));
    }
    storage::overwrite_settings(&settings);
}
