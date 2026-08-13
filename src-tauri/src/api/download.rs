// api/download.rs — 下载/安装相关 API 路由
// 职责：处理 install-start / install-progress / install-cancel / check-version-name
// 对应原项目 server/api/routes/download.js + versions.js 的安装部分

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::install;
use crate::storage;
use crate::utils;

/// 处理下载/安装相关路由
/// 返回 Some(ApiResult) 表示已处理，None 表示不匹配
pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 安装入口 =====
        "POST /api/install-start" => {
            let body = body.as_ref().or(params.as_ref())?;
            let version_id = body.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let version_url = body.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let custom_name = body.get("customName").and_then(|v| v.as_str()).map(|s| s.to_string());
            let loader_info = body.get("loaderInfo").cloned();
            // 下载源（前端已选择，如 china-first/auto/mojang），为空时后端回退到设置值
            let download_source = body.get("downloadSource").and_then(|v| v.as_str()).map(|s| s.to_string());

            if version_id.is_empty() || version_url.is_empty() {
                return Some(ApiResult::err(400, "缺少 versionId 或 url"));
            }

            // 创建安装会话
            let (session_id, cancel_flag) = install::session::create_session(&version_id);

            // 复制变量给异步任务
            let app_clone = app.clone();
            let sid = session_id.clone();
            let vid = version_id.clone();
            let vurl = version_url.clone();
            let cname = custom_name.clone();
            let linfo = loader_info.clone();
            let dsource = download_source.clone();

            // 启动异步安装任务
            tauri::async_runtime::spawn(async move {
                install::perform_installation(app_clone, sid, vid, vurl, cname, linfo, dsource, cancel_flag).await;
            });

            Some(ApiResult::ok(json!({
                "success": true,
                "sessionId": session_id,
                "versionId": version_id,
                "loaderInfo": loader_info,
                "message": "安装已开始"
            })))
        }

        // ===== 安装进度查询（兼容旧的轮询模式） =====
        "GET /api/install-progress" => {
            let session_id = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if session_id.is_empty() {
                return Some(ApiResult::ok(json!({
                    "sessionId": "",
                    "versionId": "",
                    "status": "idle",
                    "progress": 0,
                    "stage": "",
                    "message": "无安装任务",
                    "currentFile": "",
                    "totalFiles": 0,
                    "completedFiles": 0,
                    "speed": 0,
                    "bytesDownloaded": 0,
                    "totalBytes": 0,
                    "errors": []
                })));
            }

            match install::session::get_session_status(session_id) {
                Some(status) => Some(ApiResult::ok(status)),
                None => Some(ApiResult::ok(json!({
                    "sessionId": session_id,
                    "status": "completed",
                    "progress": 100,
                    "stage": "completed",
                    "message": "会话已结束"
                }))),
            }
        }

        // ===== 取消安装 =====
        "GET /api/install-cancel" => {
            let session_id = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if session_id.is_empty() {
                return Some(ApiResult::err(400, "缺少 sessionId"));
            }

            let success = install::session::cancel_session(session_id);
            Some(ApiResult::ok(json!({
                "success": success,
                "message": if success { "已发送取消请求" } else { "会话不存在" }
            })))
        }

        // ===== POST 取消安装（与 GET 逻辑一致，从 body 读 sessionId） =====
        "POST /api/install-cancel" => {
            let data = body.as_ref().or(params.as_ref()).cloned().unwrap_or(Value::Null);
            let session_id = utils::get_str(&data, "sessionId");

            if session_id.is_empty() {
                return Some(ApiResult::err(400, "缺少 sessionId"));
            }

            let success = install::session::cancel_session(&session_id);
            Some(ApiResult::ok(json!({
                "success": success,
                "message": if success { "已取消" } else { "会话不存在" }
            })))
        }

        // ===== 检查版本名是否可用 =====
        "POST /api/check-version-name" => {
            let body = body.as_ref().or(params.as_ref())?;
            let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                return Some(ApiResult::ok(json!({ "available": false, "reason": "名称为空" })));
            }

            // 检查本地是否已存在同名版本
            let versions_dir = storage::resolve_data_dir().join("versions");
            let target = versions_dir.join(name);
            let available = !target.exists();
            Some(ApiResult::ok(json!({
                "available": available,
                "reason": if available { "" } else { "版本名已存在" }
            })))
        }

        _ => None,
    }
}
