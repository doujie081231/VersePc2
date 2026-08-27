// api/placeholder.rs — 未迁移路由的占位返回
// 所有尚未迁移的 API 在这里返回占位数据，避免前端报错卡死
// 已迁移的模块（settings/accounts/favorites/system）由各自文件处理
// 这里只处理：版本管理、模组、整合包、Java 安装、模组加载器、皮肤、
//             局域网、红石联机、崩溃分析、文件系统、资源搜索、authlib、
//             自定义下载、游戏启动/运行、杂项等
//
// 占位策略：尽量返回空数组/空对象，让前端"看似成功但啥也没有"
// 后续逐步把路由从这里搬到对应模块

use serde_json::{json, Value};

use super::ApiResult;
use crate::storage;

pub fn handle(method: &str, path: &str, params: &Option<Value>, _body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 版本管理（已迁移到 versions 模块）=====
        // versions / version-local-details / version-details 等路由已由 versions::handle 处理

        "GET /api/debug/versions" => Some(ApiResult::ok(json!({
            "versionsDir": storage::resolve_data_dir().join("versions").to_string_lossy(),
            "folders": []
        }))),

        // ===== 模组（已迁移到 mods 模块）=====
        // /api/mods/* 路由已由 mods::handle 处理

        // ===== 整合包（已迁移到 modpacks 模块）=====
        // "GET /api/modpacks/search" 已由 modpacks::handle 处理

        // ===== 游戏状态（已迁移到 game 模块）=====
        // "GET /api/game/status" 已由 game::handle 处理

        // ===== 收藏夹（已迁移到 favorites 模块）=====
        // "GET /api/favorites" 已由 favorites::handle 处理

        // ===== 杂项（已迁移到 misc 模块）=====
        // current-context / create-shortcut / screenshots / screenshot /
        // server/ping / save-background / clear-background 已由 misc::handle 处理

        // ===== 模组加载器（已迁移到 modloaders 模块）=====
        // fabric/forge/neoforge/optifine/fabric-api 的 versions 路由已由 modloaders::handle 处理

        // ===== 默认占位 =====
        _ => None,
    }
}
