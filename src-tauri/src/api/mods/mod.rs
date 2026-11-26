// api/mods/mod.rs — 模组管理路由
// 职责：模组列表、搜索、详情、版本、下载、管理、依赖解析、对话框
use serde_json::Value;
use tauri::AppHandle;

use crate::api::ApiResult;

mod deps;
mod manage;
mod project;
mod search;

pub(crate) use deps::*;
pub(crate) use manage::*;
pub(crate) use project::*;
pub(crate) use search::*;

/// Modrinth API
pub(crate) const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// CurseForge API
pub(crate) const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";

/// CurseForge 默认 API Key
pub(crate) const DEFAULT_CF_API_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

/// 依赖递归解析的最大深度
pub(crate) const MAX_DEPENDENCY_DEPTH: usize = 32;

/// Modrinth 上由加载器自动安装、不应当作前置重复下载的项目（Fabric API / Quilt API）
pub(crate) const MODRINTH_BUNDLED_DEPS: [&str; 2] = ["P7dR8mSH", "qvIfYCYJ"];

/// CurseForge 上由加载器自动安装、不应当作前置重复下载的项目（Fabric API / Quilt API）
pub(crate) const CURSEFORGE_BUNDLED_DEPS: [u64; 2] = [306612, 634179];

/// 处理模组路由
pub async fn handle(
    _app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 列表 =====
        "GET /api/mods" => Some(handle_list().await),
        "GET /api/mod-icon" => Some(handle_icon(params)),
        "GET /api/mods/open-save-folder" => Some(handle_open_save_folder(params)),
        "GET /api/mods/installed" => Some(handle_installed(params).await),
        "GET /api/mods/categories" => Some(handle_categories()),
        "GET /api/mods/featured" => Some(handle_featured().await),
        // ===== 搜索 =====
        "GET /api/mods/search" => Some(handle_search(params).await),
        // ===== 详情 =====
        "GET /api/mods/project-versions" => Some(handle_project_versions(params).await),
        "GET /api/mods/detail" => Some(handle_detail(params).await),
        "GET /api/mods/versions" => Some(handle_versions(params).await),
        // ===== 下载 =====
        "POST /api/mods/download" => Some(handle_download(body).await),
        "POST /api/mods/download-version" => Some(handle_download_version(body).await),
        "GET /api/mods/download-status" => Some(handle_download_status(params)),
        // ===== 依赖 =====
        "POST /api/mods/get-dependencies" => Some(handle_get_dependencies(body).await),
        "POST /api/mods/get-dependencies-recursive" => Some(handle_get_dependencies_recursive(body).await),
        "GET /api/mods/resolve-deps" => Some(handle_resolve_deps(params).await),
        "POST /api/mods/resolve-deps-versions" => Some(handle_resolve_deps_versions(body).await),
        // ===== 管理 =====
        "POST /api/mods/toggle" => Some(handle_toggle(body)),
        "POST /api/mods/delete" => Some(handle_delete(body)),
        "POST /api/mods/check-updates" => Some(handle_check_updates(body).await),
        "POST /api/mods/apply-updates" => Some(handle_apply_updates(body).await),
        "POST /api/mods/install-from-file" => Some(handle_install_from_file(body)),
        "POST /api/mods/remove" => Some(handle_remove(body)),
        // ===== 对话框 =====
        "GET /api/mods/select-modpack-file" => Some(handle_select_modpack_file().await),
        "GET /api/mods/select-file" => Some(handle_select_file().await),
        "POST /api/mods/select-save-folder" => Some(handle_select_save_folder(body).await),
        _ => None,
    }
}

// ============== 列表路由 ==============
