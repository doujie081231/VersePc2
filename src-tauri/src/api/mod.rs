// api/mod.rs — API 路由模块入口
// 包含 ApiResult 定义、api_proxy 命令分发器
// 每个 API 模块导出一个 handle 函数，按路径前缀分发

pub mod accounts;
pub mod authlib;
pub mod crash;
pub mod download;
pub mod favorites;
pub mod game;
pub mod lan;
pub mod launch;
pub mod misc;
pub mod modloaders;
pub mod modpacks;
pub mod mods;
pub mod msauth;
pub mod placeholder;
pub mod resources;
pub mod settings;
pub mod skins;
pub mod sponsor;
pub mod system;

use serde_json::Value;

// Java、文件系统、版本管理模块在 lib.rs 直接引用（非 api 子模块）
// 这里用 re-export 让分发更集中
use crate::java as java_module;
use crate::filesystem as fs_module;
use crate::versions as versions_module;

/// API 响应结构，前端通过 result.status 判断成功失败，result.body 取数据
#[derive(serde::Serialize, Clone)]
pub struct ApiResult {
    pub status: u16,
    pub body: Value,
}

impl ApiResult {
    pub fn ok(body: Value) -> Self {
        Self { status: 200, body }
    }

    pub fn err(status: u16, msg: &str) -> Self {
        Self {
            status,
            body: serde_json::json!({ "error": msg }),
        }
    }
}

/// 路由分发主入口
/// 前端通过 invoke('api_proxy', { method, path, params, body }) 调用
#[tauri::command]
pub async fn api_proxy(
    app: tauri::AppHandle,
    method: String,
    path: String,
    params: Option<Value>,
    body: Option<Value>,
) -> ApiResult {
    let key = format!("{} {}", method.to_uppercase(), path);
    println!("[api_proxy] {}", key);

    // 特殊路由：需要调用系统对话框的，单独处理
    if key == "GET /api/version/select-folder" {
        return crate::dialog::select_folder_api(
            &app,
            Some("选择 Minecraft 文件夹".to_string()),
            None,
        )
        .await;
    }

    // 下载/安装相关路由（async，优先处理）
    if path.starts_with("/api/install")
        || path == "/api/check-version-name"
        || path == "/api/install-cancel"
    {
        if let Some(r) = download::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 启动相关路由（async，调用 do_launch 等异步函数）
    if path.starts_with("/api/launch") {
        if let Some(r) = launch::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 游戏运行相关路由（同步，仅查询 game_session）
    if path.starts_with("/api/game") {
        if let Some(r) = game::handle(&method, &path, &params, &body) {
            return r;
        }
    }

    // 崩溃分析相关路由（async，可选手动导入分析）
    if path.starts_with("/api/crash") {
        if let Some(r) = crash::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 微软账号认证路由（async，发起 OAuth 请求）
    if path.starts_with("/api/msauth") {
        if let Some(r) = msauth::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // authlib-injector 第三方登录路由（async，HTTP 拉取+下载校验）
    if path.starts_with("/api/authlib-injector") {
        if let Some(r) = authlib::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 账号路由（async，第三方登录涉及 Yggdrasil HTTP 请求 + authlib 下载）
    if path.starts_with("/api/accounts") {
        if let Some(r) = accounts::handle(&method, &path, &params, &body).await {
            return r;
        }
    }

    // 整合包路由（async，Modrinth + CurseForge 双源 HTTP 搜索）
    if path.starts_with("/api/modpacks") || path == "/api/modpack/import" {
        if let Some(r) = modpacks::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 杂项路由（async，含 TCP ping + PowerShell 子进程）
    if path == "/api/current-context"
        || path == "/api/create-shortcut"
        || path == "/api/screenshots"
        || path == "/api/screenshot"
        || path == "/api/server/ping"
        || path == "/api/save-background"
        || path == "/api/clear-background"
    {
        if let Some(r) = misc::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 资源搜索/下载路由（async，Modrinth/CurseForge HTTP 拉取 + 后台下载会话）
    if path.starts_with("/api/resources") {
        if let Some(r) = resources::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // LAN / EasyTier 路由（async，UPnP SSDP + EasyTier 子进程管理）
    if path.starts_with("/api/lan") || path.starts_with("/api/easytier") {
        if let Some(r) = lan::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 模组管理路由（async，Modrinth/CurseForge HTTP 搜索 + JAR 解析 + 下载会话）
    if path == "/api/mods"
        || path == "/api/mod-icon"
        || path.starts_with("/api/mods/")
    {
        if let Some(r) = mods::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 皮肤管理路由（async，含远程拉取和上传到 Mojang）
    if path.starts_with("/api/ms-skins")
        || path == "/api/default-skins"
        || path == "/api/skin-head"
        || path == "/api/set-account-skin"
        || path == "/api/upload-skin"
        || path == "/api/skin-texture"
        || path == "/api/save-avatar"
        || path == "/api/clear-avatar"
    {
        if let Some(r) = skins::handle(&app, &method, &path, &params, &body).await {
            return r;
        }
    }

    // 模组加载器路由（async，涉及 HTTP 请求拉取版本列表）
    if path.starts_with("/api/fabric")
        || path.starts_with("/api/forge")
        || path.starts_with("/api/neoforge")
        || path.starts_with("/api/optifine")
    {
        if let Some(r) = modloaders::handle(&method, &path, &params, &body).await {
            return r;
        }
    }

    // 按路径前缀分发到对应模块
    let result = if path.starts_with("/api/settings") {
        settings::handle(&method, &path, &params, &body)
    } else if path.starts_with("/api/favorites") {
        favorites::handle(&method, &path, &params, &body)
    } else if path.starts_with("/api/java") {
        // Java 路由单独处理（不归 system 模块）
        java_module::handle(&method, &path, &params, &body)
    } else if path.starts_with("/api/fs")
        || path.starts_with("/api/filesystem")
        || path == "/api/open-folder"
    {
        fs_module::handle(&method, &path, &params, &body)
    } else if path.starts_with("/api/version")
        || path == "/api/versions"
    {
        // 版本管理路由（install 已在上面处理）
        versions_module::handle(&method, &path, &params, &body).await
    } else if path.starts_with("/api/system")
        || path == "/api/status"
        || path.starts_with("/api/jvm")
        || path.starts_with("/api/cleanup")
    {
        system::handle(&method, &path, &params, &body)
    } else if path.starts_with("/api/sponsor") {
        sponsor::handle(&method, &path, &params, &body)
    } else {
        placeholder::handle(&method, &path, &params, &body)
    };

    match result {
        Some(r) => r,
        None => {
            eprintln!("[api_proxy] 未实现: {}", key);
            ApiResult::err(404, &format!("未实现: {}", key))
        }
    }
}
