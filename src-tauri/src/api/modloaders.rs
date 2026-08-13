// api/modloaders.rs — 模组加载器路由分发
// 职责：处理 /api/fabric/* /api/forge/* /api/neoforge/* /api/optifine/* /api/fabric-api/* 路由
// 对应原项目 server/api/routes/modloaders.js
//
// 路由清单：
//   GET  /api/fabric/versions           获取 Fabric Loader 版本列表
//   POST /api/fabric/install             安装 Fabric Loader
//   GET  /api/forge/versions             获取 Forge 版本列表
//   POST /api/forge/install              安装 Forge
//   GET  /api/neoforge/versions          获取 NeoForge 版本列表
//   POST /api/neoforge/install           安装 NeoForge
//   GET  /api/optifine/versions          获取 OptiFine 版本列表
//   POST /api/optifine/install           安装 OptiFine
//   GET  /api/fabric-api/versions        获取 Fabric API 模组版本列表
//   POST /api/fabric-api/install         安装 Fabric API
//
// 架构原则：
//   - 路由层只负责参数解析和分发，业务逻辑在 modloaders/ 模块
//   - 所有 GET 路由（版本查询）已完整实现
//   - 所有 POST 路由（安装）调用对应模块的实际安装函数

use serde_json::{json, Value};

use crate::api::ApiResult;
use crate::modloaders;
use crate::utils;

/// 处理模组加载器相关路由（异步，因为涉及 HTTP 请求和文件 IO）
pub async fn handle(
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== Fabric Loader 版本查询 =====
        "GET /api/fabric/versions" => Some(handle_fabric_versions(params).await),

        // ===== Fabric Loader 安装 =====
        "POST /api/fabric/install" => Some(handle_fabric_install(body).await),

        // ===== Forge 版本查询 =====
        "GET /api/forge/versions" => Some(handle_forge_versions(params).await),

        // ===== Forge 安装 =====
        "POST /api/forge/install" => Some(handle_forge_install(body).await),

        // ===== NeoForge 版本查询 =====
        "GET /api/neoforge/versions" => Some(handle_neoforge_versions(params).await),

        // ===== NeoForge 安装 =====
        "POST /api/neoforge/install" => Some(handle_neoforge_install(body).await),

        // ===== OptiFine 版本查询 =====
        "GET /api/optifine/versions" => Some(handle_optifine_versions(params).await),

        // ===== OptiFine 安装 =====
        "POST /api/optifine/install" => Some(handle_optifine_install(body).await),

        // ===== Fabric API 版本查询 =====
        "GET /api/fabric-api/versions" => Some(handle_fabric_api_versions(params).await),

        // ===== Fabric API 安装 =====
        "POST /api/fabric-api/install" => Some(handle_fabric_api_install(body).await),

        _ => None,
    }
}

// ============== GET 路由（版本查询） ==============

/// GET /api/fabric/versions
/// 参数：game（可选，传入则返回该 MC 版本可用列表）
async fn handle_fabric_versions(params: &Option<Value>) -> ApiResult {
    let game_version = params.as_ref().map(|p| utils::get_str(p, "game")).unwrap_or_default();

    let versions = if game_version.is_empty() {
        modloaders::fabric::get_loader_versions().await
    } else {
        modloaders::fabric::get_loader_versions_for_game(&game_version).await
    };

    eprintln!(
        "[modloaders] GET /api/fabric/versions game={:?} → {} 个版本",
        game_version,
        versions.len()
    );

    ApiResult::ok(json!({ "versions": versions }))
}

/// GET /api/forge/versions
/// 参数：game（必传，Minecraft 版本号）
async fn handle_forge_versions(params: &Option<Value>) -> ApiResult {
    let game_version = params.as_ref().map(|p| utils::get_str(p, "game")).unwrap_or_default();

    if game_version.is_empty() {
        return ApiResult::err(400, "Missing game parameter");
    }

    let versions = modloaders::forge::get_forge_versions(&game_version).await;
    eprintln!(
        "[modloaders] GET /api/forge/versions game={:?} → {} 个版本",
        game_version,
        versions.len()
    );
    ApiResult::ok(json!({ "versions": versions }))
}

/// GET /api/neoforge/versions
/// 参数：game（必传）
async fn handle_neoforge_versions(params: &Option<Value>) -> ApiResult {
    let game_version = params.as_ref().map(|p| utils::get_str(p, "game")).unwrap_or_default();

    if game_version.is_empty() {
        return ApiResult::err(400, "Missing game parameter");
    }

    let versions = modloaders::neoforge::get_neoforge_versions_for_game(&game_version).await;
    eprintln!(
        "[modloaders] GET /api/neoforge/versions game={:?} → {} 个版本",
        game_version,
        versions.len()
    );
    ApiResult::ok(json!({ "versions": versions }))
}

/// GET /api/optifine/versions
/// 参数：game（必传）
async fn handle_optifine_versions(params: &Option<Value>) -> ApiResult {
    let game_version = params.as_ref().map(|p| utils::get_str(p, "game")).unwrap_or_default();

    if game_version.is_empty() {
        return ApiResult::err(400, "Missing game parameter");
    }

    let versions = modloaders::optifine::get_optifine_versions(&game_version).await;
    eprintln!(
        "[modloaders] GET /api/optifine/versions game={:?} → {} 个版本",
        game_version,
        versions.len()
    );
    ApiResult::ok(json!({ "versions": versions }))
}

/// GET /api/fabric-api/versions
/// 参数：game（必传）
async fn handle_fabric_api_versions(params: &Option<Value>) -> ApiResult {
    let game_version = params.as_ref().map(|p| utils::get_str(p, "game")).unwrap_or_default();

    if game_version.is_empty() {
        return ApiResult::err(400, "Missing game parameter");
    }

    let (versions, recommended) = modloaders::fabric_api::get_fabric_api_versions(&game_version).await;
    eprintln!(
        "[modloaders] GET /api/fabric-api/versions game={:?} → {} 个版本，推荐={}",
        game_version,
        versions.len(),
        recommended
    );
    ApiResult::ok(json!({
        "versions": versions,
        "recommended": recommended,
    }))
}

// ============== POST 路由（安装） ==============

/// POST /api/fabric/install
/// body: { gameVersion, loaderVersion, targetVersionId? }
async fn handle_fabric_install(body: &Option<Value>) -> ApiResult {
    let body = match body {
        Some(b) => b,
        None => return ApiResult::err(400, "Missing body"),
    };

    let game_version = utils::get_str(body, "gameVersion");
    let loader_version = utils::get_str(body, "loaderVersion");
    let target_version_id = utils::get_str(body, "targetVersionId");

    if game_version.is_empty() || loader_version.is_empty() {
        return ApiResult::err(400, "Missing gameVersion or loaderVersion");
    }

    eprintln!(
        "[modloaders] POST /api/fabric/install game={} loader={} target={:?}",
        game_version, loader_version, target_version_id
    );

    let result = if target_version_id.is_empty() {
        modloaders::fabric::install_fabric(&game_version, &loader_version).await
    } else {
        modloaders::fabric::install_fabric_with_target(
            &game_version,
            &loader_version,
            &target_version_id,
        )
        .await
    };

    ApiResult::ok(result)
}

/// POST /api/forge/install
/// body: { gameVersion, forgeVersion, targetVersionId? }
async fn handle_forge_install(body: &Option<Value>) -> ApiResult {
    let body = match body {
        Some(b) => b,
        None => return ApiResult::err(400, "Missing body"),
    };

    let game_version = utils::get_str(body, "gameVersion");
    let forge_version = utils::get_str(body, "forgeVersion");
    let target_version_id = utils::get_str(body, "targetVersionId");

    if game_version.is_empty() || forge_version.is_empty() {
        return ApiResult::err(400, "Missing gameVersion or forgeVersion");
    }

    eprintln!(
        "[modloaders] POST /api/forge/install game={} forge={} target={:?}",
        game_version, forge_version, target_version_id
    );

    let result = modloaders::forge::install_forge(
        &game_version,
        &forge_version,
        if target_version_id.is_empty() { None } else { Some(&target_version_id) },
    )
    .await;

    ApiResult::ok(result)
}

/// POST /api/neoforge/install
/// body: { gameVersion, neoVersion, targetVersionId? }
async fn handle_neoforge_install(body: &Option<Value>) -> ApiResult {
    let body = match body {
        Some(b) => b,
        None => return ApiResult::err(400, "Missing body"),
    };

    let game_version = utils::get_str(body, "gameVersion");
    let neo_version = utils::get_str(body, "neoVersion");
    let target_version_id = utils::get_str(body, "targetVersionId");

    if game_version.is_empty() || neo_version.is_empty() {
        return ApiResult::err(400, "Missing gameVersion or neoVersion");
    }

    eprintln!(
        "[modloaders] POST /api/neoforge/install game={} neo={} target={:?}",
        game_version, neo_version, target_version_id
    );

    let result = modloaders::neoforge::install_neoforge(
        &game_version,
        &neo_version,
        if target_version_id.is_empty() { None } else { Some(&target_version_id) },
    )
    .await;

    ApiResult::ok(result)
}

/// POST /api/optifine/install
/// body: { gameVersion, optifineVersion, targetVersionId? }
async fn handle_optifine_install(body: &Option<Value>) -> ApiResult {
    let body = match body {
        Some(b) => b,
        None => return ApiResult::err(400, "Missing body"),
    };

    let game_version = utils::get_str(body, "gameVersion");
    let optifine_version = utils::get_str(body, "optifineVersion");
    let target_version_id = utils::get_str(body, "targetVersionId");

    if game_version.is_empty() || optifine_version.is_empty() {
        return ApiResult::err(400, "Missing gameVersion or optifineVersion");
    }

    eprintln!(
        "[modloaders] POST /api/optifine/install game={} optifine={} target={:?}",
        game_version, optifine_version, target_version_id
    );

    let result = modloaders::optifine::install_optifine(
        &game_version,
        &optifine_version,
        if target_version_id.is_empty() { None } else { Some(&target_version_id) },
    )
    .await;

    ApiResult::ok(result)
}

/// POST /api/fabric-api/install
/// body: { gameVersion, versionId, versionName? }
async fn handle_fabric_api_install(body: &Option<Value>) -> ApiResult {
    let body = match body {
        Some(b) => b,
        None => return ApiResult::err(400, "Missing body"),
    };

    let game_version = utils::get_str(body, "gameVersion");
    let version_id = utils::get_str(body, "versionId");
    let version_name = utils::get_str(body, "versionName");

    if game_version.is_empty() || version_id.is_empty() {
        return ApiResult::err(400, "Missing gameVersion or versionId");
    }

    eprintln!(
        "[modloaders] POST /api/fabric-api/install game={} versionId={} versionName={:?}",
        game_version, version_id, version_name
    );

    let result = modloaders::fabric_api::install_fabric_api(
        &game_version,
        &version_id,
        if version_name.is_empty() { None } else { Some(&version_name) },
    )
    .await;

    ApiResult::ok(result)
}
