// api/download.rs — 下载/安装相关 API 路由
// 职责：处理 install-start / install-progress / install-cancel / check-version-name

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
            let versions_dir = storage::resolve_data_dir().join("versions");
            let reason = validate_folder_name(name, &versions_dir);
            let available = reason.is_empty();
            Some(ApiResult::ok(json!({
                "available": available,
                "reason": reason
            })))
        }

        _ => None,
    }
}

/// 校验版本整理夹名是否合法且不与现有版本重名。
/// 返回空字符串表示可用，否则返回错误原因。
fn validate_folder_name(name: &str, versions_dir: &std::path::Path) -> String {
    // 1. 空 / 空白
    if name.trim().is_empty() {
        return "文件夹名不能为空！".to_string();
    }
    // 2. 两端空格
    if name.starts_with(' ') {
        return "文件夹名不能以空格开头！".to_string();
    }
    if name.ends_with(' ') {
        return "文件夹名不能以空格结尾！".to_string();
    }
    // 3. 长度 1~100
    let len = name.chars().count();
    if len < 1 {
        return "长度至少需 1 个字符！".to_string();
    }
    if len > 100 {
        return "长度最长为 100 个字符！".to_string();
    }
    // 4. 尾部小数点
    if name.ends_with('.') {
        return "文件夹名不能以小数点结尾！".to_string();
    }
    // 5. 非法字符：Windows 路径非法字符 + Minecraft 额外字符 "!;"
    for c in name.chars() {
        match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '!' | ';' => {
                return format!("文件夹名不可包含 {} 字符！", c);
            }
            c if c.is_control() => {
                return format!("文件夹名不可包含 {} 字符！", c);
            }
            _ => {}
        }
    }
    // 6. Windows 保留名（CON/PRN/AUX/CLOCK$/NUL/COM0-9/LPT0-9），忽略大小写
    let upper = name.to_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "CLOCK$" | "NUL" | "COM0" | "COM1" | "COM2" | "COM3"
            | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT0" | "LPT1"
            | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    );
    if reserved {
        return format!("文件夹名不可为 {}！", name);
    }
    // 7. NTFS 8.3 短文件名形式（"xx~1"）
    let bytes = name.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'~' && i >= 2 && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            return "文件夹名不能包含这一特殊格式！".to_string();
        }
    }
    // 8. 与 versions 目录下现有子文件夹名重名（忽略大小写）
    if let Ok(entries) = std::fs::read_dir(versions_dir) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            if let Some(fname) = entry.file_name().to_str() {
                if fname.eq_ignore_ascii_case(name) {
                    return "不可与现有文件夹重名！".to_string();
                }
            }
        }
    }
    String::new()
}
