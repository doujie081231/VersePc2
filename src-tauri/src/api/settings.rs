// api/settings.rs — 设置相关路由
// 兼容原项目 server/api/routes/settings.js
// 路由清单：
//   GET  /api/settings          读取设置
//   POST /api/settings          保存设置（合并）
//   POST /api/settings/set      设置单个字段
//   POST /api/settings/reset     重置为默认值
//   GET  /api/settings/data-dir  查询数据目录

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;
use crate::utils;

pub fn handle(method: &str, path: &str, _params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/settings" => Some(ApiResult::ok(storage::load_settings())),

        "GET /api/settings/data-dir" => {
            let data_dir = storage::resolve_data_dir();
            let exe_config = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("data-config.json")))
                .unwrap_or_else(|| std::path::PathBuf::from("data-config.json"));
            let is_default = !exe_config.exists() && !data_dir.join("data-config.json").exists();
            Some(ApiResult::ok(json!({
                "dataDir": data_dir.to_string_lossy(),
                "isDefault": is_default
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
