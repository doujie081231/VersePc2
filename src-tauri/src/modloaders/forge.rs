// modloaders/forge.rs — Forge 加载器版本查询
// 职责：从 Forge Maven 元数据 XML 拉取指定 MC 版本的 Forge 版本列表
// 对应原项目 server/api/routes/modloaders.js 中 GET /api/forge/versions 的内联实现
//
// 路由：
//   GET /api/forge/versions?game=1.20.1
//
// 实现细节：
//   - 依次尝试 5 个镜像源（官方 + 4 个镜像），谁先返回包含目标版本的 XML 就用谁
//   - 解析 maven-metadata.xml，提取 <version>gameVersion-forgeVer</version> 条目
//   - 最多返回 30 条，第一条标记为"推荐"，第二条标记为"最新"
//
// 安装逻辑（installForge）暂未迁移，下次迁移。

use serde_json::{json, Value};

use super::shared;

/// Forge Maven 元数据源（原版 + 镜像，依次尝试）
const FORGE_METADATA_URLS: &[&str] = &[
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
    "https://mirror.ghproxy.com/https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
    "https://ghproxy.net/https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
    "https://ghfast.top/https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
    "https://raw.gitmirror.com/Anzhiyuan/MinecraftForgeMaven/main/maven/net/minecraftforge/forge/maven-metadata.xml",
];

/// 获取指定 MC 版本的 Forge 版本列表
/// 对应原项目 GET /api/forge/versions
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
///
/// # 返回
/// Vec<{version: String, gameVersion: String, type: String}>（type 为 "推荐" / "最新" / "release"）
pub async fn get_forge_versions(game_version: &str) -> Vec<Value> {
    // 多源竞速：并发请求各镜像源，谁先返回非空 XML 就用谁（对应 PCL 双源竞速）
    // 注意：Forge 对 MC 1.20.5+ 不再发布版本，元数据中确实没有匹配条目时返回空数组
    let xml = match shared::fetch_text_racing(FORGE_METADATA_URLS, 15).await {
        Some(x) => x,
        None => {
            eprintln!("[Forge] 所有元数据源均不可用（网络问题或被墙）");
            return Vec::new();
        }
    };

    // 解析 <version>1.20.1-47.3.0</version> 条目
    let mut versions: Vec<Value> = Vec::new();
    for cap in find_version_tags(&xml) {
        // cap 形如 "1.20.1-47.3.0"
        if let Some(forge_ver) = cap.strip_prefix(&format!("{}-", game_version)) {
            if !forge_ver.is_empty() {
                versions.push(json!({
                    "version": forge_ver,
                    "gameVersion": game_version,
                    "type": "release",
                }));
            }
        }
    }

    // 倒序（最新版本在前）
    versions.reverse();

    // 第一条标记"推荐"，第二条标记"最新"
    let len = versions.len();
    if len > 0 {
        versions[0] = json!({
            "version": versions[0].get("version").cloned().unwrap_or_default(),
            "gameVersion": game_version,
            "type": "推荐",
        });
        if len > 1 {
            versions[1] = json!({
                "version": versions[1].get("version").cloned().unwrap_or_default(),
                "gameVersion": game_version,
                "type": "最新",
            });
        }
    }

    // 限制最多 30 条
    versions.truncate(30);
    versions
}

/// 从 maven-metadata.xml 中提取所有 <version>...</version> 内容
/// 返回原始版本号字符串列表（如 "1.20.1-47.3.0"）
fn find_version_tags(xml: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_tag = false;
    let mut current = String::new();

    // 简单 XML 解析：提取 <version>xxx</version> 中的 xxx
    let bytes = xml.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // 读取标签名
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                let tag = &xml[start..end];
                let tag_trim = tag.trim();
                if tag_trim == "version" {
                    in_tag = true;
                    current.clear();
                } else if tag_trim == "/version" {
                    if in_tag {
                        result.push(current.clone());
                        in_tag = false;
                    }
                }
                i = end + 1;
            } else {
                break;
            }
        } else if in_tag {
            current.push(bytes[i] as char);
            i += 1;
        } else {
            i += 1;
        }
    }
    result
}

/// 转义正则表达式特殊字符（用于版本号字符串匹配）
#[allow(dead_code)]
fn regex_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

// ============== Forge 安装 ==============
// 对应原项目 server/modloaders/forge.js 的 installForge
//
// 实现策略：使用 Forge 官方 installer 的命令行模式
//   java -jar forge-installer.jar --installClient <data_dir>
//
// 这个方案让 Forge 官方 installer 处理所有复杂逻辑：
//   - 解压 installer 内部文件
//   - 运行 processor（二进制补丁、SRG 重映射等）
//   - 下载所需库文件
//   - 写入版本 JSON
//
// 我们负责：
//   - 下载 installer JAR
//   - 调用 Java 执行 installer
//   - 完成后定位版本目录，按需重命名为 target_version_id

/// Forge Maven 镜像源（用于下载 installer）
const FORGE_MAVEN_BMCLAPI: &str = "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge";
const FORGE_MAVEN_OFFICIAL: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

/// 安装 Forge 模组加载器
/// 对应原项目 installForge
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
/// - `forge_version`: Forge 版本号，如 "47.3.0"
/// - `target_version_id`: 可选的目标版本目录名（默认为 "<game>-forge-<forge>"）
///
/// # 返回
/// JSON 对象：{ success, versionId?, error? }
pub async fn install_forge(
    game_version: &str,
    forge_version: &str,
    target_version_id: Option<&str>,
) -> Value {
    eprintln!(
        "[Forge] 安装 game={} forge={} target={:?}",
        game_version, forge_version, target_version_id
    );

    // 1. 检查原版已安装（缺失时自动下载）
    if let Err(e) = shared::ensure_base_version_installed(game_version, None).await {
        return json!({ "success": false, "error": e });
    }

    // 2. 标准化 Forge 版本号（去除 "1.20.1-" 前缀）
    let forge_version = if forge_version.starts_with(&format!("{}-", game_version)) {
        &forge_version[game_version.len() + 1..]
    } else {
        forge_version
    };

    let version_str = format!("{}-{}", game_version, forge_version);
    let default_version_id = format!("{}-forge-{}", game_version, forge_version);
    let target_id = target_version_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_version_id.clone());

    eprintln!("[Forge] versionStr={}, targetId={}", version_str, target_id);

    // 3. 下载 installer JAR
    let installer_dir = shared::data_dir().join("temp");
    let installer_path = installer_dir.join(format!("forge-installer-{}.jar", version_str));
    if let Err(e) = std::fs::create_dir_all(&installer_dir) {
        return json!({ "success": false, "error": format!("无法创建临时目录: {}", e) });
    }

    let installer_urls = vec![
        format!("{}/{}/forge-{}-installer.jar", FORGE_MAVEN_BMCLAPI, version_str, version_str),
        format!("{}/{}/forge-{}-installer.jar", FORGE_MAVEN_OFFICIAL, version_str, version_str),
    ];

    let mut installer_ok = false;
    for url in &installer_urls {
        eprintln!("[Forge] 下载 installer: {}", url);
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
                eprintln!("[Forge] reqwest 下载失败 {}: {}", url, e);
                let _ = std::fs::remove_file(&installer_path);
            }
        }

        // 2) 程序内部方式失败时，改用系统 curl 兜底（能连上 reqwest 连不上的源）
        if !downloaded_ok {
            eprintln!("[Forge] 改用 curl 下载 installer: {}", url);
            match shared::download_with_curl(url, &installer_path, 300).await {
                Ok(()) => {
                    downloaded_ok = true;
                }
                Err(e) => {
                    eprintln!("[Forge] curl 下载失败 {}: {}", url, e);
                    let _ = std::fs::remove_file(&installer_path);
                }
            }
        }

        if downloaded_ok {
            // 验证文件大小（>64KB）和 ZIP 魔数
            if let Ok(meta) = std::fs::metadata(&installer_path) {
                if meta.len() < 64 * 1024 {
                    eprintln!("[Forge] installer 文件过小 ({} bytes)，尝试下一个源", meta.len());
                    let _ = std::fs::remove_file(&installer_path);
                    continue;
                }
                if shared::verify_zip_magic(&installer_path) {
                    eprintln!("[Forge] installer 下载成功 ({} bytes)", meta.len());
                    installer_ok = true;
                    break;
                } else {
                    eprintln!("[Forge] installer ZIP 魔数无效，尝试下一个源");
                    let _ = std::fs::remove_file(&installer_path);
                    continue;
                }
            }
        }
    }

    if !installer_ok {
        shared::file_log(&format!(
            "[Forge] installer 下载失败（所有镜像源均失败），version={}",
            version_str
        ));
        return json!({ "success": false, "error": format!("Forge installer 下载失败（所有镜像源均失败），版本 {}", version_str) });
    }

    // 4. 收集可用 Java 候选（按游戏版本匹配；安装器失败时自动换下一个重试）
    let java_candidates = crate::java::select_java_candidates_for_version(game_version);
    if java_candidates.is_empty() {
        let (min_v, _) = crate::launch::get_java_version_range(game_version);
        let _ = std::fs::remove_file(&installer_path);
        shared::file_log(&format!(
            "[Forge] 未找到合适的 Java（需要 Java {}）来运行 installer，game={}",
            min_v, game_version
        ));
        return json!({
            "success": false,
            "error": format!("未找到合适的 Java（需要 Java {}）来运行 Forge installer。请在设置中安装或配置 Java 路径。", min_v)
        });
    }

    eprintln!(
        "[Forge] 可用 Java 候选: {}",
        java_candidates
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    );

    // 5. 调用 Java installer，失败时自动换下一个候选 Java 重试
    // 命令：java -jar forge-installer.jar --installClient <data_dir>
    let data_dir = shared::data_dir();

    // 官方安装器需要检测到启动器配置，否则报 "There is no Minecraft launcher profile" 中止
    shared::ensure_launcher_profile(game_version);

    eprintln!("[Forge] 执行: java -jar ... --installClient {}", data_dir.display());

    let mut last_error: Option<String> = None;
    let mut installed = false;

    for (idx, java_path) in java_candidates.iter().enumerate() {
        eprintln!("[Forge] 尝试第 {} 个 Java: {}", idx + 1, java_path);

        // [Java 9+ 必需] 现代 Forge 安装器依赖 cpw.mods.bootstraplauncher 模块，
        // 不加 --add-exports 会报模块访问错误导致安装器崩溃（与原项目 runForgeInstallerJar 一致）
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
                eprintln!("[Forge] Java {} 执行失败: {}", java_path, e);
                continue;
            }
        };

        eprintln!("[Forge] installer 退出码: {}", exit_code);
        if !stdout.is_empty() {
            eprintln!("[Forge] installer stdout (最后 500 字): {}", &stdout[stdout.len().saturating_sub(500)..]);
        }
        if !stderr.is_empty() {
            eprintln!("[Forge] installer stderr (最后 500 字): {}", &stderr[stderr.len().saturating_sub(500)..]);
        }

        // 无论成败，都把安装器完整输出写入日志文件，便于定位真实失败原因
        shared::dump_installer_output(&format!("forge-{}.install", version_str), &stdout, &stderr);

        if exit_code == 0 {
            eprintln!("[Forge] 安装成功，使用的 Java: {}", java_path);
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
        eprintln!("[Forge] Java {} 安装失败，尝试下一个候选...", java_path);
    }

    if !installed {
        let _ = std::fs::remove_file(&installer_path);
        let err_tail = last_error.unwrap_or_else(|| "未知错误".to_string());
        shared::file_log(&format!(
            "[Forge] installer 执行失败（所有 Java 候选均失败），version={}，err={}",
            version_str, err_tail
        ));
        return json!({
            "success": false,
            "error": format!("Forge installer 失败（所有 Java 候选均失败）: {}", err_tail)
        });
    }

    // 6. 定位 installer 生成的版本目录
    // Forge installer 会创建 versions/<default_version_id>/<default_version_id>.json
    let generated_dir = shared::versions_dir().join(&default_version_id);
    let generated_json = generated_dir.join(format!("{}.json", default_version_id));

    if !generated_json.exists() {
        shared::file_log(&format!(
            "[Forge] 未找到默认版本 JSON {}，开始扫描 versions 目录",
            generated_json.display()
        ));
        // 尝试在 versions 目录下查找最新的 forge-* 目录
        let mut found_version_id: Option<String> = None;
        if let Ok(entries) = std::fs::read_dir(shared::versions_dir()) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.contains(&format!("forge-{}", forge_version))
                        || name == &default_version_id
                    {
                        let json_path = shared::versions_dir().join(name).join(format!("{}.json", name));
                        if json_path.exists() {
                            found_version_id = Some(name.to_string());
                            break;
                        }
                    }
                }
            }
        }

        if let Some(found_id) = found_version_id {
            eprintln!("[Forge] 找到生成的版本: {}", found_id);
            // 如果找到的不是默认 version_id，说明 installer 用了不同的命名
            // 把它安全地规范化为 target_id（处理 Windows 大小写视为同一目录的情况）
            let found_dir = shared::versions_dir().join(&found_id);
            let target_dir = shared::versions_dir().join(&target_id);
            shared::normalize_version_dir(&found_dir, &found_id, &target_dir, &target_id);
        } else {
            // 安装器 --installClient 模式只写 launcher profile，不生成独立版本目录。
            // 此时从安装器 JAR 内嵌的 version.json 里解出版本 JSON 手动落盘。
            shared::file_log("[Forge] 未找到生成版本目录，尝试从 installer 提取 version.json");
            if shared::extract_installer_version_json(&installer_path, &target_id).is_some() {
                shared::file_log(&format!("[Forge] 已从 installer 提取版本 JSON: {}", target_id));
            } else {
                let _ = std::fs::remove_file(&installer_path);
                shared::file_log(&format!(
                    "[Forge] 从 installer 提取版本 JSON 失败: {}",
                    generated_json.display()
                ));
                return json!({
                    "success": false,
                    "error": format!("Forge installer 完成但未找到版本 JSON（{}）", generated_json.display())
                });
            }
        }
    } else {
        // installer 生成的版本目录就是默认路径，按需安全规范化到 target_id
        let target_dir = shared::versions_dir().join(&target_id);
        shared::normalize_version_dir(&generated_dir, &default_version_id, &target_dir, &target_id);
    }

    // 7. 清理 installer
    let _ = std::fs::remove_file(&installer_path);

    // 8. 验证最终版本 JSON 存在
    let final_json = shared::versions_dir().join(&target_id).join(format!("{}.json", target_id));
    if !final_json.exists() {
        return json!({
            "success": false,
            "error": "Forge 安装完成但版本 JSON 不存在"
        });
    }

    eprintln!("[Forge] 安装完成: {}", target_id);
    json!({
        "success": true,
        "versionId": target_id
    })
}
