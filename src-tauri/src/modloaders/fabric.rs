// modloaders/fabric.rs — Fabric 加载器版本查询与安装
// 职责：从 Fabric Meta API（含 BMCLAPI 镜像）拉取 Fabric Loader 版本列表，
//       以及安装 Fabric Loader（拉取 profile JSON、构造版本配置、并发下载库）
// 对应原项目 server/modloaders/fabric.js
//
// 路由：
//   GET  /api/fabric/versions          → get_loader_versions()
//   GET  /api/fabric/versions?game=1.20.1 → get_loader_versions_for_game(game)
//   POST /api/fabric/install            → install_fabric(game, loader)

use serde_json::{json, Value};

use super::shared;

/// Fabric Meta 官方端点（v2 API）
pub const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2";
/// BMCLAPI 镜像
pub const BMCLAPI_FABRIC_META: &str = "https://bmclapi2.bangbang93.com/fabric-meta";

/// 获取 Fabric Loader 全部版本列表
/// 对应原项目 getFabricLoaderVersions
///
/// # 返回
/// Vec<{version: String, stable: bool}>
pub async fn get_loader_versions() -> Vec<Value> {
    let urls = vec![
        format!("{}/versions/loader", FABRIC_META_URL),
        format!("{}/versions/loader", BMCLAPI_FABRIC_META),
    ];

    match shared::fetch_with_racing(urls).await {
        Ok(data) => {
            if let Some(arr) = data.as_array() {
                arr.iter()
                    .map(|v| json!({
                        "version": shared::jstr(v, "version"),
                        "stable": shared::jbool(v, "stable"),
                    }))
                    .collect()
            } else {
                eprintln!("[Fabric] get_loader_versions: 响应不是数组");
                Vec::new()
            }
        }
        Err(e) => {
            eprintln!("[Fabric] get_loader_versions 所有源失败: {}", e);
            Vec::new()
        }
    }
}

/// 获取指定 MC 版本可用的 Fabric Loader 版本列表
/// 对应原项目 getFabricLoaderVersionsForGame
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
///
/// # 返回
/// Vec<{version: String, stable: bool}>
pub async fn get_loader_versions_for_game(game_version: &str) -> Vec<Value> {
    let urls = vec![
        format!("{}/versions/loader/{}", FABRIC_META_URL, game_version),
        format!("{}/versions/loader/{}", BMCLAPI_FABRIC_META, game_version),
    ];

    match shared::fetch_with_racing(urls).await {
        Ok(data) => {
            if let Some(arr) = data.as_array() {
                arr.iter()
                    .filter_map(|v| {
                        let loader = v.get("loader")?;
                        Some(json!({
                            "version": shared::jstr(loader, "version"),
                            "stable": shared::jbool(loader, "stable"),
                        }))
                    })
                    .collect()
            } else {
                eprintln!("[Fabric] get_loader_versions_for_game({}): 响应不是数组", game_version);
                Vec::new()
            }
        }
        Err(e) => {
            eprintln!("[Fabric] get_loader_versions_for_game({}) 所有源失败: {}", game_version, e);
            Vec::new()
        }
    }
}

// ============== Fabric Loader 安装 ==============
// 对应原项目 server/modloaders/fabric.js 的 installFabric

/// 安装 Fabric 模组加载器（使用默认 versionId）
/// 对应原项目 installFabric
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
/// - `loader_version`: Fabric Loader 版本号，如 "0.16.10"
///
/// # 返回
/// JSON 对象：{ success, versionId?, error? }
pub async fn install_fabric(game_version: &str, loader_version: &str) -> Value {
    install_fabric_impl(game_version, loader_version, None).await
}

/// 安装 Fabric 模组加载器（指定 targetVersionId，避免大小写不一致导致 JSON 覆盖问题）
///
/// # 参数
/// - `game_version`: Minecraft 版本号
/// - `loader_version`: Fabric Loader 版本号
/// - `target_version_id`: 目标版本目录名（如 "1.20.1-Fabric-0.16.10"）
pub async fn install_fabric_with_target(
    game_version: &str,
    loader_version: &str,
    target_version_id: &str,
) -> Value {
    install_fabric_impl(game_version, loader_version, Some(target_version_id)).await
}

/// Fabric 安装核心实现
async fn install_fabric_impl(
    game_version: &str,
    loader_version: &str,
    target_version_id: Option<&str>,
) -> Value {
    // 默认 versionId：fabric-loader-<loader>-<game>
    // 若传入 target_version_id，则用其作为版本目录名（但版本 JSON 内部的 id 仍用标准格式）
    let version_id = target_version_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("fabric-loader-{}-{}", loader_version, game_version));
    eprintln!(
        "[Fabric] 安装 versionId={} (target={:?})",
        version_id, target_version_id
    );

    // 1. 获取加载器 profile JSON（不单独安装原版，下方统一合并）
    let profile_url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        FABRIC_META_URL, game_version, loader_version
    );
    let mirror_profile_url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        BMCLAPI_FABRIC_META, game_version, loader_version
    );

    let mut full_profile: Option<Value> = None;
    for url in [&profile_url, &mirror_profile_url] {
        match shared::fetch_json(url).await {
            Ok(data) => {
                eprintln!("[Fabric] profile/json 获取成功");
                full_profile = Some(data);
                break;
            }
            Err(e) => {
                eprintln!("[Fabric] profile/json 失败 {}: {}", url, e);
            }
        }
    }

    // 3. profile/json 失败时，从基础端点手动构造版本配置
    if full_profile.is_none()
        || full_profile
            .as_ref()
            .and_then(|p| p.get("libraries"))
            .and_then(|l| l.as_array())
            .map_or(true, |a| a.is_empty())
    {
        eprintln!("[Fabric] profile/json 不可用，回退到基础端点构造");
        let base_meta_url = format!(
            "{}/versions/loader/{}/{}",
            FABRIC_META_URL, game_version, loader_version
        );
        let mirror_meta_url = format!(
            "{}/versions/loader/{}/{}",
            BMCLAPI_FABRIC_META, game_version, loader_version
        );

        let profile_data = match shared::fetch_json(&base_meta_url).await {
            Ok(d) => Some(d),
            Err(_) => shared::fetch_json(&mirror_meta_url).await.ok(),
        };

        if let Some(profile_data) = profile_data {
            let mut profile = json!({
                "id": version_id,
                "inheritsFrom": game_version,
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "type": "release",
                "time": chrono_now_iso(),
                "libraries": [],
                "arguments": { "game": [], "jvm": [] }
            });

            // 从 launcherMeta 提取库列表（common + client）
            if let Some(launcher_meta) = profile_data.get("launcherMeta") {
                if let Some(libs) = launcher_meta.get("libraries") {
                    let mut all_libs = Vec::new();
                    if let Some(common) = libs.get("common").and_then(|c| c.as_array()) {
                        all_libs.extend(common.iter().cloned());
                    }
                    if let Some(client) = libs.get("client").and_then(|c| c.as_array()) {
                        all_libs.extend(client.iter().cloned());
                    }
                    profile["libraries"] = json!(all_libs);
                }
                // 主类
                if let Some(mc) = launcher_meta.get("mainClass") {
                    let mc_str = if let Some(s) = mc.as_str() {
                        Some(s.to_string())
                    } else {
                        mc.get("client").and_then(|c| c.as_str()).map(|s| s.to_string())
                    };
                    if let Some(s) = mc_str {
                        if s.contains("fabricmc") {
                            profile["mainClass"] = json!(s);
                        }
                    }
                }
            }

            // loader 主类
            if let Some(loader) = profile_data.get("loader") {
                if let Some(mc) = loader.get("mainClass").and_then(|m| m.as_str()) {
                    profile["mainClass"] = json!(mc);
                }
                // 添加 fabric-loader 主库
                if let Some(maven) = loader.get("maven").and_then(|m| m.as_str()) {
                    let parts: Vec<&str> = maven.split(':').collect();
                    if parts.len() >= 3 {
                        profile["libraries"].as_array_mut().unwrap().push(json!({
                            "name": maven,
                            "url": "https://maven.fabricmc.net/"
                        }));
                    }
                }
            }

            // intermediary 中间映射库
            if let Some(intermediary) = profile_data.get("intermediary") {
                if let Some(maven) = intermediary.get("maven").and_then(|m| m.as_str()) {
                    let parts: Vec<&str> = maven.split(':').collect();
                    if parts.len() >= 3 {
                        profile["libraries"].as_array_mut().unwrap().push(json!({
                            "name": maven,
                            "url": "https://maven.fabricmc.net/"
                        }));
                    }
                }
            }

            // 合并启动参数
            if let Some(args) = profile_data.get("arguments") {
                if let Some(obj) = args.as_object() {
                    if let Some(profile_args) =
                        profile.get_mut("arguments").and_then(|v| v.as_object_mut())
                    {
                        for (key, val) in obj {
                            if val.is_array() {
                                profile_args.insert(key.clone(), val.clone());
                            }
                        }
                    }
                }
            }

            full_profile = Some(profile);
        } else {
            return json!({
                "success": false,
                "error": "无法获取 Fabric Loader 元数据（官方源和镜像源均失败）"
            });
        }
    }

    let mut profile = match full_profile {
        Some(p) => p,
        None => {
            return json!({
                "success": false,
                "error": "Fabric Loader 元数据获取失败"
            });
        }
    };

    // 4. 与对应原版合并，产出单一独立版本（自含原版内容，删除 inheritsFrom）
    match shared::install_merged_loader(game_version, &version_id, &profile, None).await {
        Ok(_) => {
            eprintln!("[Fabric] 合并式安装完成: {}", version_id);
            // 清理不再被引用的原版目录（合并后目标版本自含，不遗留独立原版目录）
            if !game_version.is_empty() && game_version != version_id {
                shared::cleanup_orphan_vanilla(game_version);
            }
            json!({
                "success": true,
                "versionId": version_id
            })
        }
        Err(e) => {
            eprintln!("[Fabric] 安装失败: {}", e);
            json!({ "success": false, "error": e })
        }
    }
}

/// 获取当前时间的 ISO 8601 字符串（简易实现，不依赖 chrono crate）
fn chrono_now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 简易 UTC 时间转换（精度到秒）
    let days = secs / 86400;
    let remainder = secs % 86400;
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;

    // 从 1970-01-01 开始计算日期
    let mut year = 1970;
    let mut day_of_year = days as i64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if day_of_year < days_in_year {
            break;
        }
        day_of_year -= days_in_year;
        year += 1;
    }

    let month_lengths = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    let mut day = day_of_year as u32 + 1;
    for &ml in &month_lengths {
        if day <= ml {
            break;
        }
        day -= ml;
        month += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
