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

use std::path::PathBuf;

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

    // 1. 检查原版是否已安装
    let base_json_path = shared::versions_dir()
        .join(game_version)
        .join(format!("{}.json", game_version));
    let base_jar_path = shared::versions_dir()
        .join(game_version)
        .join(format!("{}.jar", game_version));

    if !base_json_path.exists() || !base_jar_path.exists() {
        return json!({
            "success": false,
            "error": format!("请先安装原版 {}", game_version)
        });
    }

    // 2. 优先尝试 profile/json 端点（包含完整版本配置）
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

    // 4. 设置 id 和 inheritsFrom
    profile["id"] = json!(version_id);
    profile["inheritsFrom"] = json!(game_version);
    if profile.get("time").is_none() {
        profile["time"] = json!(chrono_now_iso());
    }

    // 5. 收集需要下载的库（先不可变遍历，收集下载任务）
    let libs = profile.get("libraries").and_then(|l| l.as_array());
    let mut libs_to_download: Vec<(String, PathBuf)> = Vec::new();
    // 待补全 downloads.artifact.path 的库索引及其 path 值
    let mut pending_artifact_paths: Vec<(usize, String)> = Vec::new();

    if let Some(libs) = libs {
        for (idx, lib) in libs.iter().enumerate() {
            // 处理有 downloads.artifact.url 的库
            if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
                let url = shared::jstr(artifact, "url");
                let path = shared::jstr(artifact, "path");
                if !url.is_empty() && !path.is_empty() {
                    let dest = shared::libraries_dir().join(&path);
                    if !shared::is_jar_intact(&dest) {
                        libs_to_download.push((url, dest));
                    }
                    continue;
                }
            }

            // 处理仅有 name（maven 坐标）的库
            let name = shared::jstr(lib, "name");
            if !name.is_empty() {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() >= 3 {
                    let group_path = parts[0].replace('.', "/");
                    let lname = parts[1];
                    let lver = parts[2];
                    let classifier = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
                    let jar_name = format!("{}-{}{}.jar", lname, lver, classifier);
                    let local_group = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
                    let dest = shared::libraries_dir().join(&local_group).join(lname).join(lver).join(&jar_name);

                    if !shared::is_jar_intact(&dest) {
                        let base_url = shared::jstr(lib, "url");
                        let base_url = if base_url.is_empty() {
                            "https://maven.fabricmc.net/".to_string()
                        } else {
                            base_url
                        };
                        let url = format!("{}{}/{}/{}/{}", base_url, group_path, lname, lver, jar_name);
                        libs_to_download.push((url, dest));
                    }

                    // 记录待补全的库（避免不可变借用冲突）
                    if lib.get("downloads").is_none() {
                        let artifact_path = format!("{}/{}/{}/{}", group_path, lname, lver, jar_name);
                        pending_artifact_paths.push((idx, artifact_path));
                    }
                }
            }
        }
    }

    // 补全 downloads.artifact.path（可变借用，分开避免冲突）
    if !pending_artifact_paths.is_empty() {
        if let Some(libs) = profile.get_mut("libraries").and_then(|l| l.as_array_mut()) {
            for (idx, artifact_path) in pending_artifact_paths {
                if let Some(lib) = libs.get_mut(idx) {
                    lib["downloads"] = json!({
                        "artifact": {
                            "path": artifact_path,
                            "url": "",
                            "sha1": "",
                            "size": 0
                        }
                    });
                }
            }
        }
    }

    // 6. 并发下载库
    if !libs_to_download.is_empty() {
        eprintln!("[Fabric] 需要下载 {} 个库文件", libs_to_download.len());
        let (success, fail) = shared::download_libraries_concurrent(libs_to_download, 16).await;
        eprintln!("[Fabric] 库下载完成: 成功 {}, 失败 {}", success, fail);
        if fail > 0 {
            eprintln!("[Fabric] 警告: {} 个库下载失败", fail);
            return json!({
                "success": false,
                "error": format!("Fabric 加载器依赖库下载失败 {} 个，请检查网络后重试", fail)
            });
        }
    } else {
        eprintln!("[Fabric] 所有库文件已存在，无需下载");
    }

    // 7. 创建版本目录并写入 JSON
    let version_dir = shared::versions_dir().join(&version_id);
    if !version_dir.exists() {
        if std::fs::create_dir_all(&version_dir).is_err() {
            return json!({
                "success": false,
                "error": "无法创建版本目录"
            });
        }
    }

    if shared::write_version_json(&version_id, &profile) {
        eprintln!("[Fabric] 版本 JSON 已写入: {}/{}.json", version_dir.display(), version_id);
        json!({
            "success": true,
            "versionId": version_id
        })
    } else {
        json!({
            "success": false,
            "error": "写入版本 JSON 失败"
        })
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
