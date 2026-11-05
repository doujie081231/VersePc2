// 基础版本安装：自动补齐版本 JSON、客户端 JAR 与核心库。
use serde_json::Value;
use std::path::PathBuf;
use crate::storage;
use super::shared::{download_libraries_concurrent, fetch_json, file_log, jstr, libraries_dir, versions_dir, write_version_json};

// ============== 基础版本安装 ==============
// 整合包下载/导入时，若原版基础版本缺失，自动下载安装（版本 JSON + client.jar + 核心库），
// 无需用户先手动安装原版。

/// 基础版本安装进度回调：参数 (百分比, 提示信息)
pub type BaseVersionProgress = Box<dyn Fn(u32, String) + Send + Sync>;

/// 评估库的平台 rules（当前平台是否允许该库）
fn evaluate_rules(lib: &Value) -> bool {
    let rules = match lib.get("rules").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return true, // 无 rules 默认允许
    };
    let os_name = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(os) = rule.get("os").and_then(|v| v.as_object()) {
            let rule_os = os.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if rule_os == os_name {
                allowed = action == "allow";
            }
        } else {
            allowed = action == "allow";
        }
    }
    allowed
}

/// 拉取版本详情 JSON（带镜像回退）
pub(super) async fn fetch_version_details(url: &str, download_source: &str) -> Result<Value, String> {
    let urls = crate::download::mirror::get_mirror_urls(url, download_source);
    let mut last_err = String::new();
    for u in &urls {
        match fetch_json(u).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 并发下载基础版本的核心库（复用 download_libraries_concurrent 的镜像+curl 兜底）
/// 返回 (成功数, 失败数)；已存在的库跳过
pub(super) async fn download_basic_libraries(
    version_details: &Value,
    libraries_dir: &PathBuf,
) -> (usize, usize) {
    let libraries = version_details
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks: Vec<(String, PathBuf)> = Vec::new();
    for lib in libraries {
        if !evaluate_rules(&lib) {
            continue;
        }
        // 优先用 downloads.artifact
        if let Some(artifact) = lib.pointer("/downloads/artifact") {
            let aurl = artifact.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let rel = artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if aurl.is_empty() || rel.is_empty() {
                continue;
            }
            let dest = libraries_dir.join(rel);
            if dest.starts_with(libraries_dir) && !dest.exists() {
                tasks.push((aurl.to_string(), dest));
            }
            continue;
        }
        // 无 artifact，按 maven 坐标构造 URL
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        let group = parts[0];
        let artifact_id = parts[1];
        let version = parts[2];
        let classifier = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
        let group_path = group.replace('.', "/");
        let jar_name = format!("{}-{}{}.jar", artifact_id, version, classifier);
        let rel_path = format!("{}/{}/{}/{}", group_path, artifact_id, version, jar_name);
        let dest = libraries_dir.join(&rel_path);
        if dest.exists() {
            continue;
        }
        let base_url = if group.contains("fabric") || group.contains("fabricmc") {
            "https://maven.fabricmc.net/"
        } else if group.contains("neoforged") {
            // NeoForge 官方仓库路径带 releases 段
            "https://maven.neoforged.net/releases/"
        } else if group.contains("forge") || group.contains("minecraftforge") || group.starts_with("net.minecraft") {
            "https://maven.minecraftforge.net/"
        } else {
            "https://libraries.minecraft.net/"
        };
        tasks.push((format!("{}{}", base_url, rel_path), dest));
    }

    if tasks.is_empty() {
        return (0, 0);
    }
    download_libraries_concurrent(tasks, 16).await
}

/// 确保原版 Minecraft 已安装；缺失时自动下载安装。
///
/// # 返回
/// Ok(()) 表示已安装（或本次自动安装成功），Err(String) 表示安装失败
pub async fn ensure_base_version_installed(
    game_version: &str,
    on_progress: Option<BaseVersionProgress>,
) -> Result<(), String> {
    let report = on_progress.unwrap_or_else(|| Box::new(|_, _| {}) as BaseVersionProgress);
    file_log(&format!("[base] 检查基础版本 {}", game_version));

    let base_dir = versions_dir().join(game_version);
    let base_json_path = base_dir.join(format!("{}.json", game_version));
    let base_jar_path = base_dir.join(format!("{}.jar", game_version));

    // 1. 已存在版本 JSON 时，校验 client.jar 是否完整（带 SHA1 就校验，无 SHA1 视为通过）
    if base_json_path.exists()
        && let Ok(content) = std::fs::read_to_string(&base_json_path)
            && let Ok(json) = serde_json::from_str::<Value>(&content) {
                let sha1 = json.pointer("/downloads/client/sha1").and_then(|v| v.as_str()).unwrap_or("");
                let jar_ok = if base_jar_path.exists() {
                    if sha1.is_empty() {
                        true
                    } else {
                        match crate::download::single::compute_sha1(&base_jar_path).await {
                            Ok(actual) => actual.eq_ignore_ascii_case(sha1),
                            Err(_) => false,
                        }
                    }
                } else {
                    false
                };
                if jar_ok {
                    file_log(&format!("[base] {} 已安装（校验通过）", game_version));
                    return Ok(());
                }
                file_log(&format!("[base] {} JAR 缺失或损坏，重新下载", game_version));
            }

    // 2. 开始自动安装
    report(5, "获取版本清单...".to_string());
    let manifest = crate::versions::fetch_remote_manifest(false)
        .ok_or_else(|| "无法获取版本清单，请检查网络后重试".to_string())?;
    let version_info = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|it| jstr(it, "id") == game_version))
        .cloned()
        .ok_or_else(|| format!("没找到 Minecraft {}", game_version))?;
    let details_url = jstr(&version_info, "url");
    if details_url.is_empty() {
        return Err(format!("版本 {} 缺少详情地址", game_version));
    }

    report(10, "正在下载版本信息...".to_string());
    let settings = storage::load_settings();
    let download_source = jstr(&settings, "downloadSource");
    let version_details = fetch_version_details(&details_url, &download_source).await
        .map_err(|e| format!("获取版本信息失败: {}", e))?;

    // 3. 写入版本 JSON
    if std::fs::create_dir_all(&base_dir).is_err() {
        return Err(format!("无法创建版本目录 {}", game_version));
    }
    if !write_version_json(game_version, &version_details) {
        return Err(format!("无法写入版本 JSON {}", game_version));
    }

    // 4. 下载 client.jar
    report(20, "正在下载客户端文件...".to_string());
    let client_url = version_details.pointer("/downloads/client/url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_sha1 = version_details.pointer("/downloads/client/sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_size = version_details.pointer("/downloads/client/size").and_then(|v| v.as_u64()).unwrap_or(0);
    if !client_url.is_empty() {
        crate::download::download_with_mirror(
            &client_url,
            &base_jar_path,
            if client_sha1.is_empty() { None } else { Some(&client_sha1) },
            if client_size > 0 { Some(client_size) } else { None },
            &download_source,
            300,
            None,
        ).await.map_err(|e| format!("下载客户端文件失败: {}", e))?;
    }

    // 5. 并发下载核心库
    report(30, "正在下载依赖库...".to_string());
    let libs_dir = libraries_dir();
    let (_ok, fail) = download_basic_libraries(&version_details, &libs_dir).await;
    if fail > 0 {
        // 依赖库下载失败不中断安装（部分库失败时版本会被拒绝，导致整合包安装中止）。
        // 缺失库由“文件修复 / 启动前补全”自动下载补齐。
        file_log(&format!("[base] 依赖库下载失败（{} 个），将由启动前修复与补全流程补齐", fail));
    }

    report(90, "基础版本安装完成".to_string());
    file_log(&format!("[base] {} 基础版本自动安装完成", game_version));
    Ok(())
}
