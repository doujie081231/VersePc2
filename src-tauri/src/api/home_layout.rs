// api/home_layout.rs — 主页布局持久化
//
// 为什么不只用 localStorage：
//   WebView2 的 localStorage 在进程被强杀（taskkill /F、断电、崩溃）时会丢数据，
//   而"编辑布局 → 确认 → 重启又变默认"正是这个症状。
//   布局改存到 data/settings.json（由 Rust 原子写入），彻底规避。
//
// 路由：
//   GET  /api/home-layout        读取（没有则返回 default:true）
//   POST /api/home-layout        写入（body 即布局对象）

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;

/// settings.json 里的键名
const KEY: &str = "homeLayout";

/// GET /api/home-layout
pub fn get() -> ApiResult {
    let settings = storage::load_settings();
    let layout = settings.get(KEY).cloned().unwrap_or(Value::Null);
    if layout.is_null() {
        ApiResult::ok(json!({ "layout": null, "default": true }))
    } else {
        ApiResult::ok(json!({ "layout": layout, "default": false }))
    }
}

/// POST /api/home-layout — body 是完整布局对象
pub fn save(body: &Option<Value>) -> ApiResult {
    let layout = match body {
        Some(v) if v.is_object() => v.clone(),
        _ => return ApiResult::err(400, "布局必须是对象"),
    };

    // 只保留白名单字段，避免把任意垃圾写进 settings
    let mut patch = serde_json::Map::new();
    patch.insert(KEY.to_string(), layout.clone());
    // save_settings 返回合并后的完整 settings（Value），不是 Result
    storage::save_settings(&Value::Object(patch));
    ApiResult::ok(json!({ "success": true, "layout": layout }))
}

/// DELETE /api/home-layout — 恢复默认（真正删掉键，不是写 null）
pub fn reset() -> ApiResult {
    let mut current = storage::load_settings();
    if let Some(obj) = current.as_object_mut() {
        obj.remove(KEY);
    }
    storage::overwrite_settings(&current);
    ApiResult::ok(json!({ "success": true }))
}

/// 路由分发
pub fn handle(method: &str, path: &str, body: &Option<Value>) -> Option<ApiResult> {
    match (method, path) {
        ("GET", "/api/home-layout") => Some(get()),
        ("POST", "/api/home-layout") => Some(save(body)),
        ("DELETE", "/api/home-layout") => Some(reset()),
        _ => None,
    }
}
