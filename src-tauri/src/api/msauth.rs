// api/msauth.rs — 微软账号认证 API 路由
//
// 路由清单：
//   POST /api/msauth/device-code  启动登录流程，获取设备码
//   POST /api/msauth/poll         轮询登录状态，完成全链路并写入账号
//   POST /api/msauth/refresh      刷新 Token
//
// 注意：没有 cancel 端点，前端只需停止轮询即可

use serde_json::{json, Value};

use crate::api::ApiResult;
use crate::auth;
use crate::storage;
use crate::utils;

/// 处理微软认证相关路由
pub async fn handle(
    _app: &tauri::AppHandle,
    method: &str,
    path: &str,
    _params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 获取设备码 =====
        "POST /api/msauth/device-code" => Some(handle_device_code().await),

        // ===== 轮询登录状态 =====
        "POST /api/msauth/poll" => Some(handle_poll(body).await),

        // ===== 刷新 Token =====
        "POST /api/msauth/refresh" => Some(handle_refresh(body).await),

        _ => None,
    }
}

/// POST /api/msauth/device-code — 获取设备码
/// 返回 device_code、user_code、verification_uri 等
async fn handle_device_code() -> ApiResult {
    let client = auth::create_http_client();

    match auth::microsoft::request_device_code(&client).await {
        Ok(resp) => {
            ApiResult::ok(json!({
                "success": true,
                "deviceCode": resp.device_code,
                "userCode": resp.user_code,
                "verificationUri": resp.verification_uri,
                "verificationUriComplete": resp.verification_uri_complete,
                "expiresIn": resp.expires_in,
                "interval": resp.interval,
                "message": resp.message
            }))
        }
        Err(e) => ApiResult::ok(json!({
            "success": false,
            "errorCode": e.code(),
            "errorMessage": e.message()
        })),
    }
}

/// POST /api/msauth/poll — 轮询登录状态
/// 参数：deviceCode（必填）
/// 成功返回：success=true, account=新账号信息
/// 等待中返回：pending=true
/// 失败返回：success=false, errorCode, errorMessage
async fn handle_poll(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let device_code = utils::get_str(&data, "deviceCode");

    if device_code.is_empty() {
        return ApiResult::err(400, "缺少 deviceCode");
    }

    let client = auth::create_http_client();

    match auth::complete_login(&client, &device_code).await {
        Ok(auth_result) => {
            // 生成账号 ID 并保存
            let account_id = utils::generate_simple_uuid();
            let account = auth::auth_result_to_account(&auth_result, account_id.clone());

            // 加载现有账号，追加新账号
            let mut accounts = storage::load_accounts();
            if let Some(arr) = accounts.as_array_mut() {
                // 检查是否已存在同 UUID 的账号（避免重复）
                let uuid = utils::get_str(&account, "uuid");
                let existing_idx = arr
                    .iter()
                    .position(|a| utils::get_str(a, "uuid") == uuid && utils::get_str(a, "type") == "microsoft");

                if let Some(idx) = existing_idx {
                    // 替换现有账号（保留原 ID）
                    let existing_id = utils::get_str(&arr[idx], "id");
                    let mut new_account = account.clone();
                    if let Some(obj) = new_account.as_object_mut() {
                        obj.insert("id".to_string(), json!(existing_id));
                    }
                    arr[idx] = new_account;
                } else {
                    arr.push(account.clone());
                }
            }
            storage::save_accounts(&accounts);

            // 解密后的账号信息返回给前端（不暴露加密 token）
            ApiResult::ok(json!({
                "success": true,
                "account": {
                    "id": utils::get_str(&account, "id"),
                    "username": utils::get_str(&account, "username"),
                    "uuid": utils::get_str(&account, "uuid"),
                    "type": "microsoft",
                    "skinUrl": utils::get_str(&account, "skinUrl"),
                    "skinModel": utils::get_str(&account, "skinModel")
                }
            }))
        }
        Err(e) => {
            let code = e.code();
            let need_relogin = e.need_relogin();
            let pending = code == "authorization_pending";
            ApiResult::ok(json!({
                "success": false,
                "pending": pending,
                "errorCode": code,
                "errorMessage": e.message(),
                "needRelogin": need_relogin
            }))
        }
    }
}

/// POST /api/msauth/refresh — 刷新 Token
/// 参数：accountId（必填）
/// 成功返回：success=true, account=更新后的账号信息
/// 失败返回：success=false, errorCode, errorMessage
async fn handle_refresh(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");

    if account_id.is_empty() {
        return ApiResult::err(400, "缺少 accountId");
    }

    // 加载账号
    let accounts = storage::load_accounts();
    let account = accounts
        .as_array()
        .and_then(|arr| arr.iter().find(|a| utils::get_str(a, "id") == account_id))
        .cloned();

    let account = match account {
        Some(a) => a,
        None => return ApiResult::err(404, "账号不存在"),
    };

    // 解密 refresh_token
    let encrypted_refresh = utils::get_str(&account, "refreshToken");
    if encrypted_refresh.is_empty() {
        return ApiResult::ok(json!({
            "success": false,
            "errorCode": "invalid_grant",
            "errorMessage": "账号缺少 refreshToken，请重新登录",
            "needRelogin": true
        }));
    }
    let refresh_token = auth::token::decrypt_account_token(&encrypted_refresh);
    if refresh_token == encrypted_refresh {
        // 解密失败
        return ApiResult::ok(json!({
            "success": false,
            "errorCode": "invalid_grant",
            "errorMessage": "Token 解密失败，请重新登录",
            "needRelogin": true
        }));
    }

    let client = auth::create_http_client();

    match auth::refresh_login(&client, &refresh_token).await {
        Ok(auth_result) => {
            // 更新账号信息
            let updated_account = auth::auth_result_to_account(&auth_result, account_id.clone());

            let mut accounts = storage::load_accounts();
            if let Some(arr) = accounts.as_array_mut() {
                if let Some(idx) = arr
                    .iter()
                    .position(|a| utils::get_str(a, "id") == account_id)
                {
                    arr[idx] = updated_account.clone();
                }
            }
            storage::save_accounts(&accounts);

            ApiResult::ok(json!({
                "success": true,
                "account": {
                    "id": utils::get_str(&updated_account, "id"),
                    "username": utils::get_str(&updated_account, "username"),
                    "uuid": utils::get_str(&updated_account, "uuid"),
                    "type": "microsoft"
                }
            }))
        }
        Err(e) => ApiResult::ok(json!({
            "success": false,
            "errorCode": e.code(),
            "errorMessage": e.message(),
            "needRelogin": e.need_relogin()
        })),
    }
}
