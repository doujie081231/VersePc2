// api/mods/manage.rs — 模组启停、删除、更新与文件对话框
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::api::ApiResult;
use crate::mods;
use crate::storage;

use super::project::resolve_mods_dir;

/// POST /api/mods/toggle — 启用/禁用模组
pub(crate) fn handle_toggle(body: &Option<Value>) -> ApiResult {
    let mod_id = body
        .as_ref()
        .and_then(|b| b.get("modId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let enabled = body
        .as_ref()
        .and_then(|b| b.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if mod_id.is_empty() {
        return ApiResult::err(400, "Missing modId");
    }

    let settings = storage::load_settings();
    let mods_path = resolve_mods_dir(&settings, version_id, "");
    let mods_path = match mods_path {
        Some(p) => p,
        None => return ApiResult::err(400, "请先安装一个游戏版本"),
    };

    if !mods_path.exists() {
        return ApiResult::err(400, "mods 文件夹不存在");
    }

    let base_name = mod_id.trim_end_matches(".disabled");
    let clean_path = mods_path.join(base_name);
    let disabled_path = mods_path.join(format!("{}.disabled", base_name));

    if enabled {
        if disabled_path.exists()
            && let Err(e) = std::fs::rename(&disabled_path, &clean_path) {
                return ApiResult::err(500, &format!("文件操作失败: {}", e));
            }
    } else {
        if clean_path.exists()
            && let Err(e) = std::fs::rename(&clean_path, &disabled_path) {
                return ApiResult::err(500, &format!("文件操作失败: {}", e));
            }
    }

    ApiResult::ok(json!({ "success": true, "enabled": enabled }))
}

/// POST /api/mods/delete — 删除模组（模糊匹配文件名）
pub(crate) fn handle_delete(body: &Option<Value>) -> ApiResult {
    let mod_id = body
        .as_ref()
        .and_then(|b| b.get("modId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if mod_id.is_empty() {
        return ApiResult::err(400, "Missing modId");
    }

    let settings = storage::load_settings();
    let mods_path = resolve_mods_dir(&settings, "", "");
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(p) = mods_path {
        search_dirs.push(p);
    }
    // 非隔离时搜索共享目录和 .minecraft/mods
    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !version_isolation {
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(storage::resolve_data_dir);
        let shared_mods = game_dir.join("mods");
        if !search_dirs.contains(&shared_mods) {
            search_dirs.push(shared_mods);
        }
        let mc_mods =
            crate::storage::default_minecraft_dir(crate::storage::resolve_data_dir())
                .join("mods");
        if !search_dirs.contains(&mc_mods) {
            search_dirs.push(mc_mods);
        }
    }

    let mod_id_lower = mod_id.to_lowercase();
    let mut deleted_count = 0;
    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let base = name.to_lowercase().replace(".disabled", "");
                if base.contains(&mod_id_lower)
                    && std::fs::remove_file(entry.path()).is_ok() {
                        deleted_count += 1;
                    }
            }
        }
    }

    ApiResult::ok(json!({
        "success": true,
        "message": format!("已删除 {} 个文件", deleted_count),
        "deleted": deleted_count
    }))
}

/// POST /api/mods/check-updates — 检查模组更新
pub(crate) async fn handle_check_updates(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }
    let result = mods::check_mod_updates(version_id).await;
    ApiResult::ok(result)
}

/// POST /api/mods/apply-updates — 批量更新可更新模组
pub(crate) async fn handle_apply_updates(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let updates = body
        .as_ref()
        .and_then(|b| b.get("updates"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }
    if updates.is_empty() {
        return ApiResult::err(400, "没有可更新的模组");
    }
    let result = mods::apply_mod_updates(version_id, &updates).await;
    ApiResult::ok(result)
}

/// POST /api/mods/install-from-file — 从文件安装模组
pub(crate) fn handle_install_from_file(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let file_path = body
        .as_ref()
        .and_then(|b| b.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() || file_path.is_empty() {
        return ApiResult::err(400, "Missing params");
    }

    let settings = storage::load_settings();
    let mods_dir = resolve_mods_dir(&settings, version_id, "");
    let mods_dir = match mods_dir {
        Some(d) => d,
        None => return ApiResult::err(400, "无法确定模组目录"),
    };

    if let Err(e) = std::fs::create_dir_all(&mods_dir) {
        return ApiResult::err(500, &format!("创建目录失败: {}", e));
    }

    let src = PathBuf::from(&file_path);
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod.jar".to_string());
    let dest = mods_dir.join(&file_name);

    match std::fs::copy(&src, &dest) {
        Ok(_) => ApiResult::ok(json!({ "success": true })),
        Err(e) => ApiResult::ok(json!({ "success": false, "error": e.to_string() })),
    }
}

/// POST /api/mods/remove — 删除指定版本的指定模组文件
pub(crate) fn handle_remove(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let file_name = body
        .as_ref()
        .and_then(|b| b.get("fileName"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() || file_name.is_empty() {
        return ApiResult::err(400, "Missing params");
    }
    // 路径安全校验：禁止目录穿越
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return ApiResult::err(400, "Invalid fileName");
    }

    let settings = storage::load_settings();
    let mods_dir = resolve_mods_dir(&settings, version_id, "");
    let mods_dir = match mods_dir {
        Some(d) => d,
        None => return ApiResult::err(400, "无法确定模组目录"),
    };

    let rm_path = mods_dir.join(file_name);
    // 二次校验：确保最终路径在 mods_dir 下
    let canonical_mods = mods_dir.canonicalize().unwrap_or(mods_dir.clone());
    let canonical_rm = rm_path.canonicalize().unwrap_or(rm_path.clone());
    if !canonical_rm.starts_with(&canonical_mods) {
        return ApiResult::err(400, "Invalid path");
    }

    if rm_path.exists()
        && let Err(e) = std::fs::remove_file(&rm_path) {
            return ApiResult::ok(json!({ "success": false, "error": e.to_string() }));
        }

    ApiResult::ok(json!({ "success": true }))
}

// ============== 对话框路由 ==============

/// GET /api/mods/select-modpack-file — 选择整合包文件
pub(crate) async fn handle_select_modpack_file() -> ApiResult {
    // 简化：返回空（前端可用 <input type="file"> 代替）
    ApiResult::ok(Value::Null)
}

/// GET /api/mods/select-file — 选择模组文件
pub(crate) async fn handle_select_file() -> ApiResult {
    ApiResult::ok(Value::Null)
}

/// POST /api/mods/select-save-folder — 选择保存文件夹
pub(crate) async fn handle_select_save_folder(_body: &Option<Value>) -> ApiResult {
    ApiResult::ok(json!({ "cancelled": true, "error": "对话框功能在前端实现" }))
}

// ============== 辅助函数 ==============
