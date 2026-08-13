// modloaders/neoforge.rs — NeoForge 加载器版本查询
// 职责：从 BMCLAPI 或 Maven 拉 NeoForge 版本列表
// 对应原项目 server/modloaders/neoforge.js 中的 getNeoForgeVersionsForGame
//
// 路由：
//   GET /api/neoforge/versions?game=1.21.1
//
// 版本号规则：
//   - MC 1.20.1 及以前：使用旧版 Forge，版本号 47.x（含 "1.20.1-" 前缀）
//   - MC 1.20.5+（NeoForge）：版本号格式 <MC次版本>.<MC patch>.<补丁>（如 21.1.x）
//
// 安装逻辑（installNeoForge）暂未迁移，下次迁移。

use serde_json::{json, Value};

use super::shared;

/// BMCLAPI NeoForge 元数据
const BMCLAPI_NEOFORGE_META: &str = "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml";
const BMCLAPI_FORGE_META: &str = "https://bmclapi2.bangbang93.com/maven/net/neoforged/forge/maven-metadata.xml";

/// NeoForge 官方 Maven 版本列表 API
const NEOFORGE_MAVEN_API: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

/// 获取指定 MC 版本的 NeoForge 版本列表
/// 对应原项目 getNeoForgeVersionsForGame
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.21.1"
///
/// # 返回
/// Vec<{version: String, gameVersion: String, type: String}>（第一条 type 为 "推荐"）
pub async fn get_neoforge_versions_for_game(game_version: &str) -> Vec<Value> {
    // 解析 MC 版本号
    let parts: Vec<u32> = game_version
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    let mc_major = parts.first().copied().unwrap_or(0);
    let mc_minor = parts.get(1).copied().unwrap_or(0);
    let mc_patch = parts.get(2).copied().unwrap_or(0);

    // 判断使用新方案还是旧方案
    // - 新方案（NeoForge）：MC 1.20.5+ 或 1.21+
    // - 旧方案（Forge 兼容）：MC 1.20.1 及以前
    let is_new_scheme = (mc_major == 1 && mc_minor == 20 && mc_patch >= 5)
        || (mc_major == 1 && mc_minor >= 21);

    let neo_prefix = if is_new_scheme {
        format!("{}.{}", mc_minor, mc_patch)
    } else {
        format!("{}.{}", mc_major, mc_minor)
    };

    // 拉取所有 NeoForge 版本（XML）
    let mut all_neoforge_versions: Vec<String> = Vec::new();
    let mut all_forge_versions: Vec<String> = Vec::new();
    let mut last_error = String::new();

    // 并发拉取 NeoForge 和 Forge 元数据
    let (neo_result, forge_result) = tokio::join!(
        fetch_xml_versions(BMCLAPI_NEOFORGE_META),
        fetch_xml_versions(BMCLAPI_FORGE_META),
    );

    if let Ok(v) = neo_result {
        all_neoforge_versions = v;
    } else if let Err(e) = neo_result {
        last_error = e;
    }
    if let Ok(v) = forge_result {
        all_forge_versions = v;
    }

    // 双源都失败时，回退到官方 Maven API
    if all_neoforge_versions.is_empty() && all_forge_versions.is_empty() {
        match shared::fetch_json(NEOFORGE_MAVEN_API).await {
            Ok(data) => {
                if let Some(arr) = data.get("versions").and_then(|v| v.as_array()) {
                    all_neoforge_versions = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                }
            }
            Err(e) => {
                last_error = e;
            }
        }
    }

    if all_neoforge_versions.is_empty() && all_forge_versions.is_empty() {
        eprintln!("[NeoForge] 所有源均不可达，最后错误: {}", last_error);
        return Vec::new();
    }

    // 匹配 NeoForge 版本（严格按前缀匹配，如 21.1.x）
    let mut matched: Vec<String> = Vec::new();
    for ver in &all_neoforge_versions {
        if ver.starts_with(&format!("{}.", neo_prefix)) {
            matched.push(ver.clone());
        }
    }

    // 旧方案时，匹配 Forge 版本（如 "1.20.1-47.x"）
    if !is_new_scheme {
        for ver in &all_forge_versions {
            if ver.starts_with(&format!("{}-", game_version))
                || ver.starts_with(&format!("{}.", game_version))
            {
                if !matched.contains(ver) {
                    matched.push(ver.clone());
                }
            }
        }
    }

    // 去重 + 倒序
    matched.sort();
    matched.dedup();
    matched.reverse();

    // 找一个稳定版本（不含 -beta/-alpha）放到第一位作为"推荐"
    let stable_idx = matched.iter().position(|v| !v.contains("-beta") && !v.contains("-alpha"));
    if let Some(idx) = stable_idx {
        if idx > 0 {
            let stable = matched.remove(idx);
            matched.insert(0, stable);
        }
    }

    // 最多 10 条
    matched.truncate(10);

    // 转换为 JSON 输出格式
    matched
        .iter()
        .enumerate()
        .map(|(i, ver)| {
            json!({
                "version": ver,
                "gameVersion": game_version,
                "type": if i == 0 { "推荐" } else { "" },
            })
        })
        .collect()
}

/// 从 XML 中提取所有 <version>xxx</version> 内容
async fn fetch_xml_versions(url: &str) -> Result<Vec<String>, String> {
    let xml = shared::fetch_text(url, 15).await?;
    Ok(extract_version_tags(&xml))
}

/// 从 maven-metadata.xml 中提取所有 <version>...</version> 内容
fn extract_version_tags(xml: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_version = false;
    let mut current = String::new();

    let bytes = xml.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                let tag = xml[start..end].trim();
                if tag == "version" {
                    in_version = true;
                    current.clear();
                } else if tag == "/version" && in_version {
                    result.push(current.clone());
                    in_version = false;
                }
                i = end + 1;
            } else {
                break;
            }
        } else if in_version {
            current.push(bytes[i] as char);
            i += 1;
        } else {
            i += 1;
        }
    }
    result
}

// ============== NeoForge 安装 ==============
// 对应原项目 server/modloaders/neoforge.js 的 installNeoForge
//
// 实现策略：使用 NeoForge 官方 installer 的命令行模式
//   java -jar neoforge-installer.jar --installClient <data_dir>
//
// 与 Forge 安装逻辑几乎完全相同，仅 installer URL 不同
// NeoForge 1.20.1 兼容版本使用 "forge" 包名，其他版本用 "neoforge"

/// NeoForge Maven 镜像源
const NEOFORGE_MAVEN_BMCLAPI: &str = "https://bmclapi2.bangbang93.com/maven/net/neoforged";
const NEOFORGE_MAVEN_OFFICIAL: &str = "https://maven.neoforged.net/releases/net/neoforged";

/// 安装 NeoForge 模组加载器
/// 对应原项目 installNeoForge
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.21.1"
/// - `neo_version`: NeoForge 版本号，如 "21.1.1" 或 "1.20.1-47.1.0"
/// - `target_version_id`: 可选的目标版本目录名
///
/// # 返回
/// JSON 对象：{ success, versionId?, error? }
pub async fn install_neoforge(
    game_version: &str,
    neo_version: &str,
    target_version_id: Option<&str>,
) -> Value {
    eprintln!(
        "[NeoForge] 安装 game={} neo={} target={:?}",
        game_version, neo_version, target_version_id
    );

    // 1. 检查原版已安装（缺失时自动下载）
    if let Err(e) = shared::ensure_base_version_installed(game_version, None).await {
        return json!({ "success": false, "error": e });
    }

    // 2. 确定 package 名（旧版 1.20.1 用 forge，新版用 neoforge）
    let is_legacy = neo_version.starts_with("1.20.1-");
    let package_name = if is_legacy { "forge" } else { "neoforge" };

    let default_version_id = format!("{}-NeoForge-{}", game_version, neo_version);
    let target_id = target_version_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_version_id.clone());

    eprintln!(
        "[NeoForge] package={}, versionId={}, targetId={}",
        package_name, default_version_id, target_id
    );

    // 3. 下载 installer JAR
    let installer_dir = shared::data_dir().join("temp");
    let installer_path = installer_dir.join(format!("neoforge-installer-{}.jar", neo_version));
    if let Err(e) = std::fs::create_dir_all(&installer_dir) {
        return json!({ "success": false, "error": format!("无法创建临时目录: {}", e) });
    }

    let installer_urls = vec![
        format!(
            "{}/{}/{}/{}-{}-installer.jar",
            NEOFORGE_MAVEN_BMCLAPI, package_name, neo_version, package_name, neo_version
        ),
        format!(
            "{}/{}/{}/{}-{}-installer.jar",
            NEOFORGE_MAVEN_OFFICIAL, package_name, neo_version, package_name, neo_version
        ),
    ];

    let mut installer_ok = false;
    for url in &installer_urls {
        eprintln!("[NeoForge] 下载 installer: {}", url);
        let mut downloaded_ok = false;

        // 1) 程序内部方式下载（reqwest）
        match crate::download::single::download_with_mirror(
            url,
            &installer_path,
            None,
            None,
            "libraries",
            180,
            None,
        )
        .await
        {
            Ok(()) => {
                downloaded_ok = true;
            }
            Err(e) => {
                eprintln!("[NeoForge] reqwest 下载失败 {}: {}", url, e);
                let _ = std::fs::remove_file(&installer_path);
            }
        }

        // 2) 程序内部方式失败时，改用系统 curl 兜底（能连上 reqwest 连不上的源）
        if !downloaded_ok {
            eprintln!("[NeoForge] 改用 curl 下载 installer: {}", url);
            match shared::download_with_curl(url, &installer_path, 300).await {
                Ok(()) => {
                    downloaded_ok = true;
                }
                Err(e) => {
                    eprintln!("[NeoForge] curl 下载失败 {}: {}", url, e);
                    let _ = std::fs::remove_file(&installer_path);
                }
            }
        }

        if downloaded_ok {
            if let Ok(meta) = std::fs::metadata(&installer_path) {
                if meta.len() < 64 * 1024 {
                    eprintln!("[NeoForge] installer 文件过小 ({} bytes)", meta.len());
                    let _ = std::fs::remove_file(&installer_path);
                    continue;
                }
                if shared::verify_zip_magic(&installer_path) {
                    eprintln!("[NeoForge] installer 下载成功 ({} bytes)", meta.len());
                    installer_ok = true;
                    break;
                } else {
                    eprintln!("[NeoForge] installer ZIP 魔数无效");
                    let _ = std::fs::remove_file(&installer_path);
                    continue;
                }
            }
        }
    }

    if !installer_ok {
        return json!({
            "success": false,
            "error": "NeoForge installer 下载失败（所有镜像源均失败）"
        });
    }

    // 4. 收集可用 Java 候选（按游戏版本匹配；安装器失败时自动换下一个重试）
    let java_candidates = crate::java::select_java_candidates_for_version(game_version);
    if java_candidates.is_empty() {
        let (min_v, _) = crate::launch::get_java_version_range(game_version);
        let _ = std::fs::remove_file(&installer_path);
        return json!({
            "success": false,
            "error": format!("未找到合适的 Java（需要 Java {}）来运行 NeoForge installer", min_v)
        });
    }

    eprintln!(
        "[NeoForge] 可用 Java 候选: {}",
        java_candidates
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    );

    // 5. 调用 Java installer，失败时自动换下一个候选 Java 重试
    let data_dir = shared::data_dir();

    // 官方安装器需要检测到启动器配置，否则报 "There is no Minecraft launcher profile" 中止
    shared::ensure_launcher_profile(game_version);

    eprintln!(
        "[NeoForge] 执行: java -jar ... --installClient {}",
        data_dir.display()
    );

    let mut last_error: Option<String> = None;
    let mut installed = false;

    for (idx, java_path) in java_candidates.iter().enumerate() {
        eprintln!("[NeoForge] 尝试第 {} 个 Java: {}", idx + 1, java_path);

        // [Java 9+ 必需] 现代 NeoForge 安装器依赖 cpw.mods.bootstraplauncher 模块，
        // 不加 --add-exports 会报模块访问错误导致安装器崩溃
        let mut args: Vec<String> = Vec::new();
        if let Some((_, major, _)) = crate::java::inspect_java(std::path::Path::new(java_path)) {
            if major >= 9 {
                args.push("--add-exports".to_string());
                args.push("cpw.mods.bootstraplauncher/cpw.mods.bootstraplauncher=ALL-UNNAMED".to_string());
            }
        }
        args.push("-jar".to_string());
        args.push(installer_path.to_string_lossy().to_string());
        args.push("--installClient".to_string());
        args.push(data_dir.to_string_lossy().to_string());

        let install_result = shared::run_subprocess_with_timeout(
            std::path::Path::new(java_path),
            &args,
            None,
            600, // 10 分钟超时
        )
        .await;

        let (exit_code, stdout, stderr) = match install_result {
            Ok(r) => r,
            Err(e) => {
                last_error = Some(e.clone());
                eprintln!("[NeoForge] Java {} 执行失败: {}", java_path, e);
                continue;
            }
        };

        eprintln!("[NeoForge] installer 退出码: {}", exit_code);
        if !stdout.is_empty() {
            eprintln!(
                "[NeoForge] installer stdout (最后 500 字): {}",
                &stdout[stdout.len().saturating_sub(500)..]
            );
        }
        if !stderr.is_empty() {
            eprintln!(
                "[NeoForge] installer stderr (最后 500 字): {}",
                &stderr[stderr.len().saturating_sub(500)..]
            );
        }

        if exit_code == 0 {
            eprintln!("[NeoForge] 安装成功，使用的 Java: {}", java_path);
            installed = true;
            break;
        }

        last_error = Some(if !stderr.is_empty() {
            stderr[stderr.len().saturating_sub(300)..].to_string()
        } else if !stdout.is_empty() {
            stdout[stdout.len().saturating_sub(300)..].to_string()
        } else {
            "(无输出)".to_string()
        });
        eprintln!(
            "[NeoForge] Java {} 安装失败，尝试下一个候选...",
            java_path
        );
    }

    if !installed {
        let _ = std::fs::remove_file(&installer_path);
        let err_tail = last_error.unwrap_or_else(|| "未知错误".to_string());
        return json!({
            "success": false,
            "error": format!("NeoForge installer 失败（所有 Java 候选均失败）: {}", err_tail)
        });
    }

    // 6. 定位 installer 生成的版本目录
    // NeoForge installer 创建的版本目录名可能因 installer 版本不同而变化
    let generated_dir = shared::versions_dir().join(&default_version_id);
    let generated_json = generated_dir.join(format!("{}.json", default_version_id));

    let final_version_id = if generated_json.exists() {
        // 默认路径存在
        if target_id != default_version_id {
            rename_version_dir(&default_version_id, &target_id);
        }
        target_id.clone()
    } else {
        // 在 versions 目录下查找 neoforge-* 或包含 neo_version 的目录
        let mut found_id: Option<String> = None;
        if let Ok(entries) = std::fs::read_dir(shared::versions_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    let json_path = shared::versions_dir().join(name).join(format!("{}.json", name));
                    if !json_path.exists() {
                        continue;
                    }
                    // 匹配规则：
                    // - 名字包含 neo_version（如 "1.21.1-NeoForge-21.1.1" 或 "neoforge-21.1.1"）
                    // - 名字包含 "neoforge"
                    if name.contains(&neo_version) || name.to_lowercase().contains("neoforge") {
                        found_id = Some(name.to_string());
                        break;
                    }
                }
            }
        }

        match found_id {
            Some(found_id) => {
                eprintln!("[NeoForge] 找到生成的版本: {}", found_id);
                if found_id != target_id {
                    rename_version_dir(&found_id, &target_id);
                }
                target_id.clone()
            }
            None => {
                // 安装器 --installClient 模式只写 launcher profile，不生成独立版本目录。
                // 此时从安装器 JAR 内嵌的 version.json 里解出版本 JSON 手动落盘。
                eprintln!("[NeoForge] 未找到生成版本目录，尝试从 installer 提取 version.json");
                if shared::extract_installer_version_json(&installer_path, &target_id).is_none() {
                    let _ = std::fs::remove_file(&installer_path);
                    return json!({
                        "success": false,
                        "error": format!(
                            "NeoForge installer 完成但未找到版本 JSON（{}）",
                            generated_json.display()
                        )
                    });
                }
                eprintln!("[NeoForge] 已从 installer 提取版本 JSON: {}", target_id);
                target_id.clone()
            }
        }
    };

    // 7. 清理 installer
    let _ = std::fs::remove_file(&installer_path);

    // 8. 验证最终版本 JSON
    let final_json = shared::versions_dir()
        .join(&final_version_id)
        .join(format!("{}.json", final_version_id));
    if !final_json.exists() {
        return json!({
            "success": false,
            "error": "NeoForge 安装完成但版本 JSON 不存在"
        });
    }

    eprintln!("[NeoForge] 安装完成: {}", final_version_id);
    json!({
        "success": true,
        "versionId": final_version_id
    })
}

/// 重命名版本目录（包括目录名和 JSON 文件名，并更新 JSON 中的 id 字段）
/// 委托给 shared::normalize_version_dir，处理 Windows 大小写视为同一目录的问题
fn rename_version_dir(old_id: &str, new_id: &str) {
    let old_dir = shared::versions_dir().join(old_id);
    let new_dir = shared::versions_dir().join(new_id);
    shared::normalize_version_dir(&old_dir, old_id, &new_dir, new_id);
}
