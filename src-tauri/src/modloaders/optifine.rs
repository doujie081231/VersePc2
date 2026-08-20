// modloaders/optifine.rs — OptiFine 加载器版本查询与安装
// 职责：
//   1. 从 optifine.net 解析 HTML 拉取指定 MC 版本的 OptiFine 版本列表
//   2. 下载 OptiFine installer JAR 并创建独立版本目录
// 对应原项目：
//   - server/api/routes/modloaders.js 中 GET /api/optifine/versions 的内联实现
//   - server/api/routes/modloaders.js 中 POST /api/optifine/install 的内联实现
//   - server/modloaders/optifine.js 的 mergeOptiFineToVersion（已合并到本文件）
//
// 路由：
//   GET  /api/optifine/versions?game=1.20.1
//   POST /api/optifine/install            body: { gameVersion, optifineType, targetVersionId? }
//
// 安装策略（与原项目一致）：
//   - 主路径：从 installer JAR 中读取 version.json，合并 OptiFine 库后写入版本目录
//   - 降级路径：installer 内无 version.json，手动构建 fallback JSON（launchwrapper + tweakClass）

use serde_json::{json, Value};
use std::path::PathBuf;

use super::shared;

/// OptiFine 下载页 URL
const OPTIFINE_DOWNLOADS_URL: &str = "https://optifine.net/downloads";

/// 获取指定 MC 版本的 OptiFine 版本列表
/// 对应原项目 GET /api/optifine/versions
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
///
/// # 返回
/// Vec<{version: String, gameVersion: String}>（version 形如 "HD_U_Z"）
pub async fn get_optifine_versions(game_version: &str) -> Vec<Value> {
    let url = format!("{}?f={}", OPTIFINE_DOWNLOADS_URL, game_version);

    // optifine.net 实际返回 HTML 页面，必须用 fetch_text 而不是 fetch_json
    let page_html = match shared::fetch_text(&url, 15).await {
        Ok(html) => html,
        Err(e) => {
            eprintln!("[OptiFine] 获取版本列表失败: {}", e);
            // 完全失败时返回默认 HD_U_Z
            let fallback = json!({
                "version": "HD_U_Z",
                "gameVersion": game_version,
            });
            return vec![fallback];
        }
    };

    // 字符串扫描匹配：OptiFine_<gamever>_HD_U_X.jar
    let prefix = format!("OptiFine_{}_HD_U_", game_version);
    let mut versions: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let bytes = page_html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 找下一个 "OptiFine_<gamever>_HD_U_" 出现位置
        if let Some(idx) = page_html[i..].find(&prefix) {
            let abs_idx = i + idx + prefix.len();
            if abs_idx >= bytes.len() {
                break;
            }
            // 扫描字母数字（版本号字母，如 Z、Z6、Z8_1 等）
            let mut end = abs_idx;
            while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
                end += 1;
            }
            // 后面应该是 .jar 或非字母数字字符
            if end > abs_idx {
                let ver = &page_html[abs_idx..end];
                // 排除超长异常匹配（正常版本号最多 5 字符）
                if ver.len() <= 8 && !seen.contains(ver) {
                    seen.insert(ver.to_string());
                    versions.push(json!({
                        "version": format!("HD_U_{}", ver),
                        "gameVersion": game_version,
                    }));
                }
            }
            i = end;
        } else {
            break;
        }
    }

    // 回退：未匹配到则生成 Z~A 字母版本
    if versions.is_empty() {
        for c in "ZYXWVUTSRQPONMLKJIHGFEDCBA".chars() {
            versions.push(json!({
                "version": format!("HD_U_{}", c),
                "gameVersion": game_version,
            }));
        }
    }

    versions.truncate(10);
    versions
}

// ============== OptiFine 安装 ==============
// 对应原项目 server/api/routes/modloaders.js 中 POST /api/optifine/install
//
// 实现策略：
//   1. 下载 OptiFine installer JAR（同时它也是 OptiFine 主库本体）
//   2. 主路径：从 installer JAR 中读取 version.json，作为版本配置基础
//   3. 降级路径：installer 内无 version.json 时，手动构建 fallback JSON
//      （依赖 launchwrapper + --tweakClass optifine.OptiFineTweaker）
//   4. 复制 installer JAR 到 libraries/optifine/OptiFine/<game>_<type>/OptiFine-<game>_<type>.jar
//   5. 在版本 libraries 中追加 optifine:OptiFine:<game>_<type> 条目
//   6. 写入版本 JSON

/// OptiFine installer 下载基础 URL（不带 k 参数，原项目使用空 k）
const OPTIFINE_INSTALLER_URL: &str = "https://optifine.net/downloadx";

/// 安装 OptiFine 模组加载器
/// 对应原项目 POST /api/optifine/install
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
/// - `optifine_type`: OptiFine 类型版本，如 "HD_U_Z"（默认 "HD_U_Z"）
/// - `target_version_id`: 可选的目标版本目录名
///
/// # 返回
/// JSON 对象：{ success, versionId?, error? }
pub async fn install_optifine(
    game_version: &str,
    optifine_type: &str,
    target_version_id: Option<&str>,
) -> Value {
    // 1. 参数规范化
    let optifine_type = if optifine_type.is_empty() {
        "HD_U_Z".to_string()
    } else {
        optifine_type.to_string()
    };
    let default_version_id = format!("OptiFine_{}_{}", game_version, optifine_type);
    let version_id = target_version_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_version_id.clone());

    eprintln!(
        "[OptiFine] 安装 game={} type={} versionId={}",
        game_version, optifine_type, version_id
    );

    // 2. 检查原版已安装（缺失时自动下载）
    if let Err(e) = shared::ensure_base_version_installed(game_version, None).await {
        return json!({ "success": false, "error": e });
    }

    // 3. 下载 installer JAR
    let installer_dir = shared::data_dir().join("temp");
    let installer_path = installer_dir.join(format!(
        "optifine-installer-{}-{}.jar",
        game_version, optifine_type
    ));
    if let Err(e) = std::fs::create_dir_all(&installer_dir) {
        return json!({ "success": false, "error": format!("无法创建临时目录: {}", e) });
    }

    // OptiFine 下载 URL：?f=OptiFine_<game>_<type>.jar&k=
    let jar_filename = format!("OptiFine_{}_{}.jar", game_version, optifine_type);
    let download_url = format!(
        "{}?f={}&k=",
        OPTIFINE_INSTALLER_URL, jar_filename
    );

    eprintln!("[OptiFine] 下载 installer: {}", download_url);
    if let Err(e) = crate::download::single::download_with_mirror(
        &download_url,
        &installer_path,
        None,
        None,
        "libraries",
        180,
        None,
    )
    .await
    {
        return json!({ "success": false, "error": format!("OptiFine installer 下载失败: {}", e) });
    }

    // 验证文件
    let installer_meta = match std::fs::metadata(&installer_path) {
        Ok(m) => m,
        Err(e) => {
            return json!({ "success": false, "error": format!("installer 文件不存在: {}", e) });
        }
    };
    if installer_meta.len() < 64 * 1024 {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": "OptiFine installer 文件过小（可能下载失败）" });
    }
    if !shared::verify_zip_magic(&installer_path) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": "OptiFine installer 不是有效 ZIP 文件" });
    }
    eprintln!(
        "[OptiFine] installer 下载成功 ({} bytes)",
        installer_meta.len()
    );

    // 4. 创建版本目录
    let version_dir = shared::versions_dir().join(&version_id);
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    // 5. 尝试从 installer 中读取 version.json 或 <versionId>.json
    let mut version_json: Option<Value> = None;
    let mut launchwrapper_entries: Vec<(String, Vec<u8>)> = Vec::new();

    match std::fs::File::open(&installer_path) {
        Ok(file) => {
            match zip::ZipArchive::new(file) {
                Ok(mut archive) => {
                    for i in 0..archive.len() {
                        if let Ok(mut entry) = archive.by_index(i) {
                            let name = entry.name().to_string();
                            // 主路径：寻找 version.json 或 <versionId>.json
                            if name == "version.json"
                                || name == format!("{}.json", default_version_id)
                                || name == format!("{}.json", version_id)
                            {
                                let mut buf = Vec::with_capacity(entry.size() as usize);
                                if std::io::copy(&mut entry, &mut buf).is_ok() {
                                    if let Ok(content) = String::from_utf8(buf) {
                                        if let Ok(json) = serde_json::from_str::<Value>(&content)
                                        {
                                            version_json = Some(json);
                                            eprintln!("[OptiFine] 从 installer 中读取 {}", name);
                                        }
                                    }
                                }
                            }
                            // 降级路径所需：提取 launchwrapper-*.jar
                            if name.starts_with("launchwrapper") && name.ends_with(".jar") {
                                let mut buf = Vec::with_capacity(entry.size() as usize);
                                if std::io::copy(&mut entry, &mut buf).is_ok() {
                                    launchwrapper_entries.push((name.clone(), buf));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[OptiFine] ZIP 解压失败: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("[OptiFine] 打开 installer 失败: {}", e);
        }
    }

    // 6. 准备 OptiFine 库文件路径
    let of_lib_rel = format!(
        "optifine/OptiFine/{}_{}/OptiFine-{}_{}.jar",
        game_version, optifine_type, game_version, optifine_type
    );
    let of_lib_path = shared::libraries_dir().join(&of_lib_rel);
    let of_lib_name = format!("optifine:OptiFine:{}_{}", game_version, optifine_type);

    // 复制 installer JAR 到 libraries 目录
    if let Some(parent) = of_lib_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[OptiFine] 创建库目录失败: {}", e);
            }
        }
    }
    if let Err(e) = std::fs::copy(&installer_path, &of_lib_path) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": format!("复制 OptiFine 库失败: {}", e) });
    }
    eprintln!("[OptiFine] 已复制到 libraries: {}", of_lib_path.display());

    // 7. 根据是否存在 version.json 选择主路径或降级路径
    let final_json: Value = if let Some(mut profile) = version_json.take() {
        // ===== 主路径 =====
        // 设置 id / inheritsFrom / type
        if let Some(obj) = profile.as_object_mut() {
            obj.insert("id".to_string(), json!(version_id));
            obj.insert("inheritsFrom".to_string(), json!(game_version));
            obj.insert("type".to_string(), json!("release"));
        }

        // 兼容旧格式：minecraftArguments → arguments
        if let Some(obj) = profile.as_object_mut() {
            if obj.get("arguments").is_none() {
                if let Some(mc_args) = obj.get("minecraftArguments").and_then(|v| v.as_str()) {
                    let game_args: Vec<String> = mc_args.split(' ').map(|s| s.to_string()).collect();
                    obj.insert(
                        "arguments".to_string(),
                        json!({ "game": game_args, "jvm": [] }),
                    );
                }
            }
        }

        // 下载 libraries 中带 url 的库
        let libs_to_download = collect_libraries_with_urls(&profile);
        if !libs_to_download.is_empty() {
            eprintln!("[OptiFine] 主路径需下载 {} 个库文件", libs_to_download.len());
            let (success, fail) = shared::download_libraries_concurrent(libs_to_download, 16).await;
            eprintln!(
                "[OptiFine] 库下载完成: 成功 {}, 失败 {}",
                success, fail
            );
            if fail > 0 {
                return json!({
                    "success": false,
                    "error": format!("OptiFine 依赖库下载失败 {} 个，请检查网络后重试", fail)
                });
            }
        }

        // 追加 OptiFine 库（如不存在）
        if let Some(libs) = profile.get_mut("libraries").and_then(|l| l.as_array_mut()) {
            let already = libs.iter().any(|l| {
                l.get("name").and_then(|n| n.as_str()).map(|n| n == of_lib_name).unwrap_or(false)
            });
            if !already {
                libs.push(json!({
                    "name": of_lib_name,
                    "downloads": {
                        "artifact": {
                            "path": of_lib_rel
                        }
                    }
                }));
            }
        } else {
            // 没有 libraries 字段时创建
            if let Some(obj) = profile.as_object_mut() {
                obj.insert(
                    "libraries".to_string(),
                    json!([{
                        "name": of_lib_name,
                        "downloads": { "artifact": { "path": of_lib_rel } }
                    }]),
                );
            }
        }

        profile
    } else {
        // ===== 降级路径 =====
        // 从 installer 中提取 launchwrapper-* 到 libraries/net/minecraft/launchwrapper/1.12/
        for (lw_name, lw_data) in &launchwrapper_entries {
            let lw_dest = shared::libraries_dir()
                .join("net")
                .join("minecraft")
                .join("launchwrapper")
                .join("1.12")
                .join(lw_name);
            if !lw_dest.exists() {
                if let Some(parent) = lw_dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&lw_dest, lw_data).is_err() {
                    eprintln!("[OptiFine] 提取 launchwrapper 失败: {}", lw_name);
                } else {
                    eprintln!("[OptiFine] 已提取 launchwrapper: {}", lw_dest.display());
                }
            }
        }

        json!({
            "id": version_id,
            "inheritsFrom": game_version,
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "type": "release",
            "libraries": [
                {
                    "name": "net.minecraft:launchwrapper:1.12",
                    "downloads": {
                        "artifact": {
                            "path": "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar",
                            "url": "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
                        }
                    }
                },
                {
                    "name": of_lib_name,
                    "downloads": { "artifact": { "path": of_lib_rel } }
                }
            ],
            "minecraftArguments": "--tweakClass optifine.OptiFineTweaker"
        })
    };

    // 8. 写入版本 JSON
    let json_path = version_dir.join(format!("{}.json", version_id));
    let json_str = serde_json::to_string_pretty(&final_json).unwrap_or_default();
    if let Err(e) = std::fs::write(&json_path, json_str) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": format!("写入版本 JSON 失败: {}", e) });
    }
    eprintln!("[OptiFine] 版本 JSON 已写入: {}", json_path.display());

    // 9. 清理 installer
    let _ = std::fs::remove_file(&installer_path);

    eprintln!("[OptiFine] 安装完成: {}", version_id);
    json!({
        "success": true,
        "versionId": version_id
    })
}

/// 从版本 JSON 中收集带 url 的库（用于下载）
/// 返回 (url, dest_path) 列表
fn collect_libraries_with_urls(profile: &Value) -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    let libs = match profile.get("libraries").and_then(|l| l.as_array()) {
        Some(arr) => arr,
        None => return result,
    };

    for lib in libs {
        if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
            let url = shared::jstr(artifact, "url");
            let path = shared::jstr(artifact, "path");
            if !url.is_empty() && !path.is_empty() {
                let dest = shared::libraries_dir().join(&path);
                if !shared::is_jar_intact(&dest) {
                    result.push((url, dest));
                }
            }
        }
    }
    result
}
