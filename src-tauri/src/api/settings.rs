// api/settings.rs — 设置相关路由
// 兼容原项目 server/api/routes/settings.js
// 路由清单：
//   GET  /api/settings          读取设置
//   POST /api/settings          保存设置（合并）
//   POST /api/settings/set      设置单个字段
//   POST /api/settings/reset     重置为默认值
//   GET  /api/settings/data-dir  查询数据目录
//   POST /api/settings/data-dir  修改/重置数据目录（对齐 electron 原项目）

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;
use crate::utils;

fn exe_adjacent_data_config() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("data-config.json")))
        .unwrap_or_else(|| std::path::PathBuf::from("data-config.json"))
}

fn path_eq(p1: &std::path::Path, p2: &std::path::Path) -> bool {
    // 规范化后对比：解析绝对路径，Windows 上同时忽略大小写
    let a = p1.canonicalize().unwrap_or_else(|_| p1.to_path_buf());
    let b = p2.canonicalize().unwrap_or_else(|_| p2.to_path_buf());
    let s1 = a.to_string_lossy();
    let s2 = b.to_string_lossy();
    s1.eq_ignore_ascii_case(&s2)
}

pub fn handle(method: &str, path: &str, _params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/settings" => Some(ApiResult::ok(storage::load_settings())),

        "GET /api/settings/data-dir" => {
            let data_dir = storage::resolve_data_dir();
            let exe_config = exe_adjacent_data_config();
            let is_default = !exe_config.exists();
            Some(ApiResult::ok(json!({
                "dataDir": data_dir.to_string_lossy(),
                "isDefault": is_default
            })))
        }

        "POST /api/settings/data-dir" => {
            let data = body.clone().unwrap_or(Value::Null);
            let reset = utils::get_bool(&data, "reset");
            let exe_config = exe_adjacent_data_config();

            // 重置：直接删 data-config.json，重启生效
            if reset {
                if exe_config.exists() {
                    let _ = std::fs::remove_file(&exe_config);
                }
                return Some(ApiResult::ok(json!({
                    "ok": true,
                    "message": "已重置为默认目录，重启后生效"
                })));
            }

            let new_dir_input = utils::get_str(&data, "dataDir");
            if new_dir_input.is_empty() {
                return Some(ApiResult::err(400, "请提供有效的目录路径"));
            }
            // 规范化为绝对路径（对齐 electron 里 path.resolve）
            let resolved = std::path::PathBuf::from(&new_dir_input);
            let resolved_path = if resolved.is_absolute() {
                resolved
            } else {
                // 相对 exe 目录解析
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                exe_dir.join(&resolved)
            };
            let old_data_dir = storage::resolve_data_dir();

            // 相同目录无需修改
            if path_eq(&resolved_path, &old_data_dir) {
                return Some(ApiResult::ok(json!({
                    "ok": true,
                    "message": "新目录与当前目录相同，无需修改"
                })));
            }

            // mkdir -p
            if let Err(e) = std::fs::create_dir_all(&resolved_path) {
                return Some(ApiResult::err(500, &format!("创建新目录失败: {}", e)));
            }

            // 复制关键配置文件（对齐 electron 的 criticalFiles 列表）
            let critical_files = [
                "app-store.json",
                "window-config.json",
                "accounts.json",
                "settings.json",
                "external-folders.json",
                "favorites.json",
                "update-config.json",
                "store.json",
            ];
            for fname in critical_files.iter() {
                let src = old_data_dir.join(fname);
                let dst = resolved_path.join(fname);
                if src.exists() {
                    let _ = std::fs::copy(&src, &dst);
                }
            }

            // 写入 data-config.json 到 exe 同目录
            match serde_json::to_string_pretty(&json!({ "dataDir": resolved_path.to_string_lossy() })) {
                Ok(json_str) => {
                    if let Err(e) = std::fs::write(&exe_config, json_str) {
                        return Some(ApiResult::err(500, &format!("写入 data-config.json 失败: {}", e)));
                    }
                }
                Err(e) => {
                    return Some(ApiResult::err(500, &format!("序列化 data-config.json 失败: {}", e)));
                }
            }

            // 把旧 data 目录注册成外部文件夹（旧版本保留可见）
            {
                let old_versions_dir = old_data_dir.join("versions");
                if old_versions_dir.exists() && old_versions_dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&old_versions_dir) {
                        let has_versions = entries.filter_map(|e| e.ok())
                            .any(|e| e.path().is_dir());
                        if has_versions {
                            let mut folders = storage::load_external_folders();
                            let already_registered = folders.iter().any(|f| {
                                let p = f.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                path_eq(&std::path::PathBuf::from(p), &old_data_dir)
                            });
                            if !already_registered {
                                let name = old_data_dir.file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "旧数据目录".into());
                                let now = chrono::Utc::now().to_rfc3339();
                                folders.push(json!({
                                    "name": name,
                                    "path": old_data_dir.to_string_lossy(),
                                    "addedAt": now
                                }));
                                storage::save_external_folders(&folders);
                            }
                        }
                    }
                }
            }

            Some(ApiResult::ok(json!({
                "ok": true,
                "dataDir": resolved_path.to_string_lossy(),
                "message": "数据目录已修改，关键配置已迁移。请重启软件使设置完全生效。"
            })))
        }

        "POST /api/settings" => {
            let data = body.clone().unwrap_or(Value::Null);
            let updated = storage::save_settings(&data);
            Some(ApiResult::ok(json!({ "success": true, "settings": updated })))
        }

        "POST /api/settings/set" => {
            let data = body.clone().unwrap_or(Value::Null);
            let key_name = utils::get_str(&data, "key");
            let value = data.get("value").cloned().unwrap_or(Value::Null);
            if key_name.is_empty() {
                return Some(ApiResult::err(400, "Missing key"));
            }
            let mut settings = storage::load_settings();
            if let Some(obj) = settings.as_object_mut() {
                obj.insert(key_name, value);
            }
            storage::overwrite_settings(&settings);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "POST /api/settings/reset" => {
            let defaults = storage::default_settings();
            storage::overwrite_settings(&defaults);
            Some(ApiResult::ok(json!({ "success": true, "settings": defaults })))
        }

        _ => None,
    }
}
