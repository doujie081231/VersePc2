// install.rs — 由 java.rs 拆分
#![allow(unused_imports)]

// Mojang 运行时与 Java 安装
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use crate::modloaders::shared;
use crate::storage;
use crate::utils;
use super::*;

/// 安装 Mojang 官方 Java 运行时组件
pub(crate) async fn install_mojang_runtime(
    session_id: &str,
    major_version: u64,
    component: &str,
) -> Result<(), String> {
    let all_json_urls = [
        "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json",
        "https://bmclapi2.bangbang93.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json",
    ];
    let all_json_text = shared::fetch_text_racing(&all_json_urls, 30)
        .await
        .ok_or_else(|| "获取 Java 运行时列表失败".to_string())?;
    let runtime_list: Value = serde_json::from_str(&all_json_text)
        .map_err(|e| format!("Java 运行时列表解析失败: {}", e))?;
    let arch_aarch64 = std::env::consts::ARCH == "aarch64";
    let platform_key = if cfg!(target_os = "windows") {
        if arch_aarch64 { "windows-arm64" } else { "windows-x64" }
    } else if cfg!(target_os = "macos") {
        if arch_aarch64 { "mac-os-arm64" } else { "mac-os" }
    } else {
        "linux"
    };
    // Apple Silicon 缺少专用运行时（旧组件）时回退到 mac-os（可通过 Rosetta 运行）
    let platform_runtimes = runtime_list
        .get(platform_key)
        .or_else(|| {
            if platform_key == "mac-os-arm64" {
                runtime_list.get("mac-os")
            } else {
                None
            }
        })
        .and_then(|v| v.as_object())
        .ok_or_else(|| format!("平台 {} 无 Java 运行时", platform_key))?;

    let component_entries = platform_runtimes
        .get(component)
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("组件 {} 不存在", component))?;

    let component_info = component_entries
        .first()
        .ok_or_else(|| format!("组件 {} 无可用运行时", component))?;

    let manifest_url = component_info
        .get("manifest")
        .and_then(|m| m.get("url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "manifest URL 缺失".to_string())?;
    let manifest_official = manifest_url.to_string();
    let manifest_mirror = crate::download::mirror::to_mirror_url(&manifest_official)
        .filter(|m| m != &manifest_official)
        .unwrap_or_else(|| manifest_official.clone());
    let manifest_text = shared::fetch_text_racing(&[&manifest_official, &manifest_mirror], 30)
        .await
        .ok_or_else(|| "获取 Java 运行时 manifest 失败".to_string())?;
    let manifest: Value = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("Java 运行时 manifest 解析失败: {}", e))?;
    let files = manifest
        .get("files")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "manifest files 缺失".to_string())?;

    // 3. 下载所有文件到 targetDir
    let data_dir = storage::resolve_data_dir();
    let target_dir = data_dir.join("java").join(component);
    if target_dir.exists() {
        let _ = std::fs::remove_dir_all(&target_dir);
    }
    std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    // 预计算总大小
    let mut total_bytes: u64 = 0;
    for (_, info) in files {
        if let Some(size) = info.pointer("/downloads/raw/size").and_then(|v| v.as_u64()) {
            total_bytes += size;
        }
    }

    let total_files = files.len() as u64;
    let mut done_files: u64 = 0;

    let skip_hashes = [
        "12976a6c2b227cbac58969c1455444596c894656",
        "c80e4bab46e34d02826eab226a4441d0970f2aba",
        "84d2102ad171863db04e7ee22a259d1f6c5de4a5",
    ];

    for (file_path, info) in files {
        let dest = target_dir.join(file_path);
        // 清单中的 link 项：macOS/Linux 创建符号链接（如 legal 目录），不是空文件
        if info.get("type").and_then(|v| v.as_str()) == Some("link") {
            if let Some(target) = info.get("target").and_then(|v| v.as_str()) {
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                #[cfg(unix)]
                {
                    let _ = std::fs::remove_file(&dest);
                    let _ = std::os::unix::fs::symlink(target, &dest);
                }
                #[cfg(not(unix))]
                {
                    let _ = target;
                    let _ = std::fs::write(&dest, "");
                }
            }
            done_files += 1;
            continue;
        }
        let raw = match info.pointer("/downloads/raw") {
            Some(r) => r,
            None => {
                let is_dir = file_path.ends_with('/')
                    || info.get("type").and_then(|v| v.as_str()).unwrap_or("") == "directory";
                if is_dir {
                    let _ = std::fs::create_dir_all(&dest);
                } else {
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&dest, "");
                }
                done_files += 1;
                continue;
            }
        };
        let file_url = raw.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let file_sha1 = raw.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let file_size = raw.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        if skip_hashes.contains(&file_sha1) {
            done_files += 1;
            continue;
        }

        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let expected_size = if file_size > 0 { Some(file_size) } else { None };

        // 走镜像测速 + 3 轮 cache-buster 重试：Java 运行时由大量小文件组成，
        // 单个镜像偶发限速/截断时直接判整次安装失败，容易出现“下载到一半失败”。
        let download_source = crate::storage::load_settings()
            .get("downloadSource")
            .and_then(|v| v.as_str())
            .unwrap_or("china-first")
            .to_string();
        crate::download::download_with_mirror_retry(
            file_url,
            &dest,
            if file_sha1.is_empty() { None } else { Some(file_sha1) },
            expected_size,
            &download_source,
            300,
        ).await.map_err(|e| format!("下载失败 {}: {}", file_path, e))?;
        // Mojang manifest 用 executable 标记 bin/java 等可执行文件；
        // macOS/Linux 必须补上执行权限，否则运行时无法启动。
        if info.get("executable").and_then(|v| v.as_bool()).unwrap_or(false) {
            ensure_executable(&dest);
        }

        done_files += 1;
        let pct = if total_bytes > 0 {
            (10.0 + (done_files as f64 / total_files as f64 * 80.0)).min(90.0) as u32
        } else {
            50
        };
        write_java_status(session_id, &json!({
            "sessionId": session_id,
            "status": "downloading",
            "progress": pct,
            "message": format!("正在下载 Java {} 运行时 ({}/{})...", major_version, done_files, total_files),
            "majorVersion": major_version,
        }));
    }

    Ok(())
}

/// 异步安装 Java（后台任务）
/// 完整流程：获取链接 → 下载 → 解压 → 刷新缓存
pub(crate) async fn install_java_async(session_id: String, major_version: u64) {
    let data_dir = storage::resolve_data_dir();
    let _java_dir = data_dir.join("java");
    let started_at = utils::now_iso();

    macro_rules! fail {
        ($status:expr, $msg:expr) => {{
            let mut err = $status.clone();
            if let Some(obj) = err.as_object_mut() {
                obj.insert("status".to_string(), json!("error"));
                obj.insert("message".to_string(), json!($msg));
                obj.insert("completedAt".to_string(), json!(utils::now_iso()));
            }
            eprintln!("[java-install] 失败 (session={}): {}", session_id, $msg);
            write_java_status(&session_id, &err);
            clear_cancelled_session(&session_id);
            return;
        }};
    }

    // 步骤 1: fetching
    let mut status = json!({
        "sessionId": session_id,
        "status": "fetching",
        "progress": 0,
        "message": format!("正在获取 Java {} 下载链接...", major_version),
        "majorVersion": major_version,
        "startedAt": started_at,
    });
    write_java_status(&session_id, &status);

    // 检查取消
    if is_java_download_cancelled(&session_id) {
        clear_cancelled_session(&session_id);
        return;
    }

    // 使用 Mojang 官方 Java 运行时源
    let mojang_component_map = [
        ("8", "jre-legacy"),
        ("17", "java-runtime-beta"),
        ("21", "java-runtime-delta"),
        ("25", "java-runtime-epsilon"),
    ];
    if let Some((_, component)) = mojang_component_map
        .iter()
        .find(|(v, _)| v.parse::<u64>().ok() == Some(major_version))
    {
        status = json!({
            "sessionId": session_id,
            "status": "fetching",
            "progress": 5,
            "message": "正在获取Mojang官方Java运行时信息...",
            "majorVersion": major_version,
            "startedAt": started_at,
        });
        write_java_status(&session_id, &status);

        match install_mojang_runtime(&session_id, major_version, component).await {
            Ok(()) => {
                let install_dir = storage::resolve_data_dir().join("java").join(component);
                // macOS 的 Mojang 运行时位于 jre.bundle/Contents/Home/bin/java，
                // 不能假设安装目录本身就是 javaHome。
                let java_path = find_java_exe_recursive(&install_dir, 6)
                    .unwrap_or_else(|| install_dir.join("bin").join(java_bin_name()));
                ensure_executable(&java_path);
                let java_home = java_path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| install_dir.clone());
                let completed = json!({
                    "sessionId": session_id,
                    "status": "completed",
                    "progress": 100,
                    "message": format!("Java {} 安装成功！", major_version),
                    "majorVersion": major_version,
                    "javaHome": java_home.to_string_lossy(),
                    "javaPath": java_path.to_string_lossy(),
                    "startedAt": started_at,
                    "completedAt": utils::now_iso(),
                });
                write_java_status(&session_id, &completed);
                invalidate_cache();
                let mut settings = storage::load_settings();
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("javaPath".to_string(), json!(java_path.to_string_lossy().to_string()));
                }
                storage::overwrite_settings(&settings);
                clear_cancelled_session(&session_id);
                return;
            }
            Err(e) => {
                fail!(status, format!("Java {} 下载失败: {}", major_version, e));
            }
        }
    }

    fail!(status, format!("Java {} 不支持自动下载", major_version));
}
