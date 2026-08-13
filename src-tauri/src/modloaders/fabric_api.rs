// modloaders/fabric_api.rs — Fabric API 模组版本查询
// 职责：从 Modrinth API 拉取指定 MC 版本可用的 Fabric API 模组版本列表
// 对应原项目 server/api/routes/modloaders.js 中 GET /api/fabric-api/versions 的内联实现
//
// 路由：
//   GET /api/fabric-api/versions?game=1.20.1
//
// 实现细节：
//   - 调用 Modrinth API 查询 fabric-api 项目（ID: P7dR8mSH）的版本列表
//   - 过滤出支持目标 MC 版本和 fabric loader 的版本
//   - 标记推荐版本（最新稳定 release）
//   - 失败时降级查询不带过滤条件的全部版本
//
// 安装逻辑（POST /api/fabric-api/install）暂未迁移，下次迁移。

use serde_json::{json, Value};

use super::shared;

/// Modrinth API
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// Fabric API 在 Modrinth 的项目 ID
const FABRIC_API_PROJECT_ID: &str = "P7dR8mSH";

/// Modrinth API 镜像源
const MODRINTH_API_MIRROR: &str = "https://mod.mcimirror.top/modrinth/v2";

/// 获取指定 MC 版本的 Fabric API 版本列表
/// 对应原项目 GET /api/fabric-api/versions
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
///
/// # 返回
/// { versions: Vec<Value>, recommended: String }
pub async fn get_fabric_api_versions(game_version: &str) -> (Vec<Value>, String) {
    eprintln!("[fabric_api] 开始查询 game_version={}", game_version);

    // 优先：带过滤条件查询，同时尝试官方源和镜像源
    let filtered_url = format!(
        "{}/project/{}/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
        MODRINTH_API, FABRIC_API_PROJECT_ID, game_version
    );
    let mirror_filtered_url = format!(
        "{}/project/{}/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
        MODRINTH_API_MIRROR, FABRIC_API_PROJECT_ID, game_version
    );

    let mut raw_versions: Option<Vec<Value>> = None;

    // 竞速请求官方源和镜像源
    for (i, url) in [filtered_url, mirror_filtered_url].iter().enumerate() {
        match shared::fetch_json(url).await {
            Ok(data) => {
                eprintln!("[fabric_api] 源{} 返回成功", i);
                if let Some(arr) = data.as_array() {
                    if !arr.is_empty() {
                        raw_versions = Some(arr.clone());
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("[fabric_api] 源{} 失败: {}", i, e);
            }
        }
    }

    // 降级：不带过滤条件查询
    if raw_versions.as_ref().map_or(true, |v| v.is_empty()) {
        eprintln!("[fabric_api] 过滤查询无结果，降级到不带过滤条件的全量查询");
        let fallback_url = format!("{}/project/{}/version", MODRINTH_API, FABRIC_API_PROJECT_ID);
        let mirror_fallback_url = format!("{}/project/{}/version", MODRINTH_API_MIRROR, FABRIC_API_PROJECT_ID);

        for (i, url) in [fallback_url, mirror_fallback_url].iter().enumerate() {
            match shared::fetch_json(url).await {
                Ok(data) => {
                    eprintln!("[fabric_api] 降级源{} 返回成功", i);
                    if let Some(arr) = data.as_array() {
                        if !arr.is_empty() {
                            raw_versions = Some(arr.clone());
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[fabric_api] 降级源{} 失败: {}", i, e);
                }
            }
        }
    }

    let raw = match &raw_versions {
        Some(v) if !v.is_empty() => v,
        _ => {
            eprintln!("[fabric_api] 所有源均失败，返回空列表");
            return (Vec::new(), String::new());
        }
    };

    eprintln!("[fabric_api] 原始返回 {} 个版本", raw.len());

    // 转换为前端期望的格式
    let all: Vec<Value> = raw
        .iter()
        .filter_map(|v| {
            let files = v.get("files").and_then(|f| f.as_array())?;
            let primary = files.first()?;
            Some(json!({
                "versionId": shared::jstr(v, "id"),
                "versionNumber": if v.get("version_number").is_some() {
                    shared::jstr(v, "version_number")
                } else {
                    shared::jstr(v, "id")
                },
                "name": shared::jstr(v, "name"),
                "gameVersions": v.get("game_versions").cloned().unwrap_or(json!([])),
                "loaders": v.get("loaders").cloned().unwrap_or(json!([])),
                "releaseType": shared::jstr(v, "version_type"),
                "datePublished": shared::jstr(v, "date_published"),
                "downloads": v.get("downloads").cloned().unwrap_or(json!(0)),
                "filename": shared::jstr(primary, "filename"),
                "url": shared::jstr(primary, "url"),
                "size": primary.get("size").cloned().unwrap_or(json!(0)),
            }))
        })
        .collect();

    // 过滤：兼容当前游戏版本和 fabric loader
    let compatible: Vec<Value> = all
        .iter()
        .filter(|v| {
            let game_versions = v.get("gameVersions").and_then(|x| x.as_array());
            let loaders = v.get("loaders").and_then(|x| x.as_array());
            let has_game = game_versions
                .map(|gv| gv.iter().any(|x| x.as_str() == Some(game_version)))
                .unwrap_or(false);
            let has_fabric = loaders
                .map(|ld| ld.iter().any(|x| x.as_str() == Some("fabric")))
                .unwrap_or(false);
            has_game && has_fabric
        })
        .cloned()
        .collect();

    let mut list = if !compatible.is_empty() {
        compatible
    } else {
        all
    };

    // 过滤：release 或 beta 类型，且有 url
    list.retain(|v| {
        let release_type = v.get("releaseType").and_then(|x| x.as_str()).unwrap_or("");
        let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("");
        (release_type == "release" || release_type == "beta") && !url.is_empty()
    });

    // 推荐版本：第一个 release
    let recommended = list
        .iter()
        .find(|v| {
            v.get("releaseType").and_then(|x| x.as_str()) == Some("release")
        })
        .or_else(|| list.first())
        .and_then(|v| v.get("versionId"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    (list, recommended)
}

/// 安装指定 Fabric API 版本
/// 对应原项目 POST /api/fabric-api/install
///
/// # 参数
/// - `game_version`: Minecraft 版本号
/// - `version_id`: Modrinth 上的 Fabric API 版本 ID
/// - `version_name`: 目标版本名（用于定位 mods 目录，可选）
///
/// # 返回
/// JSON 对象：{ success, filename?, path?, modsDir?, versionId?, versionNumber?, error? }
pub async fn install_fabric_api(
    game_version: &str,
    version_id: &str,
    version_name: Option<&str>,
) -> Value {
    eprintln!(
        "[fabric_api] 安装 game={}, versionId={}, versionName={:?}",
        game_version, version_id, version_name
    );

    // 1. 查询 Modrinth API 获取版本列表（带镜像回退）
    let filtered_url = format!(
        "{}/project/{}/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
        MODRINTH_API, FABRIC_API_PROJECT_ID, game_version
    );
    let mirror_url = format!(
        "{}/project/{}/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
        MODRINTH_API_MIRROR, FABRIC_API_PROJECT_ID, game_version
    );

    let mut raw_versions: Option<Value> = None;
    for url in [&filtered_url, &mirror_url] {
        match shared::fetch_json(url).await {
            Ok(data) => {
                raw_versions = Some(data);
                break;
            }
            Err(e) => {
                eprintln!("[fabric_api] 查询失败 {}: {}", url, e);
            }
        }
    }

    // 降级：不带过滤条件
    if raw_versions.is_none() {
        let fallback_url = format!("{}/project/{}/version", MODRINTH_API, FABRIC_API_PROJECT_ID);
        let mirror_fallback = format!("{}/project/{}/version", MODRINTH_API_MIRROR, FABRIC_API_PROJECT_ID);
        for url in [fallback_url, mirror_fallback] {
            if let Ok(data) = shared::fetch_json(&url).await {
                raw_versions = Some(data);
                break;
            }
        }
    }

    let versions = match &raw_versions {
        Some(v) => v.as_array().cloned().unwrap_or_default(),
        None => {
            return json!({ "success": false, "error": "无法获取 Fabric API 版本列表" });
        }
    };

    // 2. 找到匹配 versionId 的版本
    let target = versions.iter().find(|v| {
        shared::jstr(v, "id") == version_id
    });

    let target = match target {
        Some(t) => t,
        None => {
            return json!({ "success": false, "error": "找不到指定的 Fabric API 版本" });
        }
    };

    // 3. 获取 primary file
    let files = target.get("files").and_then(|f| f.as_array());
    let primary = match files.and_then(|f| f.first()) {
        Some(f) => f,
        None => {
            return json!({ "success": false, "error": "Fabric API 版本没有可下载文件" });
        }
    };

    let file_url = shared::jstr(primary, "url");
    let file_filename = shared::jstr(primary, "filename");
    if file_url.is_empty() || file_filename.is_empty() {
        return json!({ "success": false, "error": "文件信息不完整" });
    }

    // 4. 确定 mods 目录（版本隔离目录优先，其次全局 mods）
    let version_name = version_name.unwrap_or("");
    let version_dir = if !version_name.is_empty() {
        shared::versions_dir().join(version_name)
    } else {
        shared::versions_dir().join(format!("fabric-loader-{}", game_version))
    };

    let mods_dir = {
        let version_mods = version_dir.join("mods");
        if version_mods.exists() {
            version_mods
        } else {
            shared::data_dir().join("mods")
        }
    };

    // 确保 mods 目录存在
    if !mods_dir.exists() {
        if std::fs::create_dir_all(&mods_dir).is_err() {
            return json!({ "success": false, "error": "无法创建 mods 目录" });
        }
    }

    // 5. 清理旧版本 Fabric API（fabric-api-*.jar）
    if let Ok(existing) = std::fs::read_dir(&mods_dir) {
        for entry in existing.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                // 匹配 fabric-api-*.jar 或 fabric-api.jar
                if (name.starts_with("fabric-api-") && name.ends_with(".jar"))
                    || name.eq_ignore_ascii_case("fabric-api.jar")
                {
                    let _ = std::fs::remove_file(entry.path());
                    eprintln!("[fabric_api] 清理旧版本: {}", name);
                }
            }
        }
    }

    // 6. 下载 JAR 到 mods 目录
    let dest_path = mods_dir.join(&file_filename);
    eprintln!(
        "[fabric_api] 下载 {} -> {}",
        file_url,
        dest_path.display()
    );

    match crate::download::single::download_with_mirror(
        &file_url,
        &dest_path,
        None,
        None,
        "modrinth",
        180,
        None,
    )
    .await
    {
        Ok(()) => {
            let version_number = shared::jstr(target, "version_number");
            eprintln!("[fabric_api] 安装完成: {}", file_filename);
            json!({
                "success": true,
                "filename": file_filename,
                "path": dest_path.to_string_lossy(),
                "modsDir": mods_dir.to_string_lossy(),
                "versionId": version_id,
                "versionNumber": if version_number.is_empty() { version_id.to_string() } else { version_number }
            })
        }
        Err(e) => {
            eprintln!("[fabric_api] 下载失败: {}", e);
            json!({ "success": false, "error": format!("Fabric API 下载失败: {}", e) })
        }
    }
}
