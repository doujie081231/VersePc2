// library.rs — 由 dep_check.rs 拆分
#![allow(unused_imports)]

// 标准库、Maven 库与主 JAR 检查
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use regex::Regex;
use crate::java;
use crate::storage;
use crate::utils;
use super::*;

/// 检查标准 library（有 downloads.artifact）
pub(crate) fn check_standard_library(
    lib: &Value,
    libraries_dir: &Path,
    external_version_dir: Option<&Path>,
    result: &mut DepCheckResult,
) {
    let artifact = match lib.get("downloads").and_then(|v| v.get("artifact")) {
        Some(a) => a,
        None => return,
    };
    let path_str = utils::get_str(artifact, "path");
    if path_str.is_empty() {
        return;
    }
    let mut lib_path = match safe_lib_path(&path_str, libraries_dir) {
        Some(p) => p,
        None => return,
    };

    if !lib_path.exists() {
        // 外部目录回退
        if let Some(ext_dir) = external_version_dir
            && let Some(root) = find_external_root(ext_dir)
                && let Some(ext_p) = safe_lib_path(&path_str, &root.join("libraries"))
                    && ext_p.exists() {
                        lib_path = ext_p;
                    }
    }

    let sha1 = utils::get_str(artifact, "sha1");
    let size = artifact.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    let url = utils::get_str(artifact, "url");
    let name = if !utils::get_str(lib, "name").is_empty() {
        utils::get_str(lib, "name")
    } else {
        Path::new(&path_str).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
    };

    if lib_path.exists() {
        if !sha1.is_empty() {
            if let Some(actual) = utils::calculate_sha1(&lib_path)
                && actual == sha1 {
                    return;
                }
        } else {
            return;
        }
    }

    // 缺失或 SHA1 校验失败
    let mut fix_url = url.clone();
    if fix_url.is_empty() {
        let lib_name = utils::get_str(lib, "name");
        if !lib_name.is_empty() {
            let p: Vec<&str> = lib_name.split(':').collect();
            if p.len() >= 3 {
                let gp = p[0].replace('.', "/");
                let nm = p[1];
                let vr = p[2];
                let cl = if p.len() >= 4 { p[3] } else { "" };
                let jn = if !cl.is_empty() {
                    format!("{}-{}-{}.jar", nm, vr, cl)
                } else {
                    format!("{}-{}.jar", nm, vr)
                };
                let base = if utils::get_str(lib, "url").is_empty() {
                    if p[0].contains("minecraftforge") || p[0].contains("forge") || p[0].contains("minecraft") {
                        "https://maven.minecraftforge.net/".to_string()
                    } else {
                        "https://libraries.minecraft.net/".to_string()
                    }
                } else {
                    utils::get_str(lib, "url")
                };
                fix_url = format!("{}{}/{}/{}/{}", base, gp, nm, vr, jn);
            }
        }
    }

    result.libraries.missing.push(MissingFile {
        kind: "library".to_string(),
        url: fix_url,
        path: lib_path.to_string_lossy().to_string(),
        sha1,
        size,
        name,
        desc: String::new(),
        message: String::new(),
    });
}

/// 检查无 downloads.artifact 的 maven 库
pub(crate) fn check_maven_library(
    lib: &Value,
    libraries_dir: &Path,
    external_version_dir: Option<&Path>,
) -> Option<MissingFile> {
    let name = utils::get_str(lib, "name");
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR_STR);
    let name_part = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 { parts[3] } else { "" };
    let jar_name = if !classifier.is_empty() {
        format!("{}-{}-{}.jar", name_part, version, classifier)
    } else {
        format!("{}-{}.jar", name_part, version)
    };

    let mut lib_path = libraries_dir.join(&group_path).join(name_part).join(version).join(&jar_name);
    if !lib_path.exists()
        && let Some(ext_dir) = external_version_dir
            && let Some(root) = find_external_root(ext_dir) {
                let ext_p = root.join("libraries").join(&group_path).join(name_part).join(version).join(&jar_name);
                if ext_p.exists() {
                    lib_path = ext_p;
                }
            }

    if lib_path.exists() {
        return None;
    }

    // 缺失：按 group 选择 maven 仓库
    let mut base_url = utils::get_str(lib, "url");
    if base_url.is_empty() {
        if name.contains("fabric") || name.contains("fabricmc") {
            base_url = "https://maven.fabricmc.net/".to_string();
        } else if name.contains("neoforged") {
            // NeoForge 官方仓库路径带 releases 段
            base_url = "https://maven.neoforged.net/releases/".to_string();
        } else if name.contains("forge") || name.contains("minecraftforge") || parts[0] == "net.minecraft" {
            base_url = "https://maven.minecraftforge.net/".to_string();
        } else {
            base_url = "https://libraries.minecraft.net/".to_string();
        }
    }
    let group_maven = parts[0].replace('.', "/");
    let download_url = format!("{}{}/{}/{}/{}", base_url, group_maven, name_part, version, jar_name);

    Some(MissingFile {
        kind: "library".to_string(),
        url: download_url,
        path: lib_path.to_string_lossy().to_string(),
        sha1: String::new(),
        size: 0,
        name,
        desc: String::new(),
        message: String::new(),
    })
}

/// 安全解析库文件路径（防路径穿越）
pub(crate) fn safe_lib_path(artifact_path: &str, base_dir: &Path) -> Option<PathBuf> {
    if artifact_path.is_empty() {
        return None;
    }
    let normalized = artifact_path.replace('/', std::path::MAIN_SEPARATOR_STR);
    let resolved = base_dir.join(&normalized);
    let canonical_base = base_dir.canonicalize().unwrap_or_else(|_| base_dir.to_path_buf());
    let canonical_resolved = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if canonical_resolved.starts_with(&canonical_base) || canonical_resolved == canonical_base {
        Some(resolved)
    } else {
        None
    }
}

/// 判断资源文件是否完好：存在且（未声明大小 或 实际大小一致）。
/// 大小校验与资源缺失检测中的判定一致。
pub(crate) fn asset_exists_ok(path: &Path, expected_size: u64) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (expected_size == 0 || m.len() == expected_size),
        Err(_) => false,
    }
}

pub(crate) fn push_missing_main_jar(result: &mut DepCheckResult, version_json: &Value, version_id: &str, jar_path: &Path) {
    if let Some(client) = version_json.get("downloads").and_then(|v| v.get("client")) {
        result.main_jar.ok = false;
        result.main_jar.message = "主JAR文件缺失".to_string();
        result.missing_files.push(MissingFile {
            kind: "main_jar".to_string(),
            url: utils::get_str(client, "url"),
            path: jar_path.to_string_lossy().to_string(),
            sha1: utils::get_str(client, "sha1"),
            size: client.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            name: format!("{}.jar", version_id),
            desc: String::new(),
            message: String::new(),
        });
    } else {
        // 无 downloads.client 信息，标记为缺失但无法自动下载
        result.main_jar.ok = false;
        result.main_jar.message = "主JAR文件缺失且无下载信息".to_string();
    }
}

pub(crate) fn push_missing_main_jar_with_fallback(result: &mut DepCheckResult, version_json: &Value, version_id: &str, versions_dir: &Path) {
    let mut fallback_url = String::new();
    let mut fallback_sha1 = String::new();
    let mut fallback_size: u64 = 0;
    let mut fallback_jar_id = String::new();

    // 优先使用当前版本自身声明的 client 下载信息（自包含合并版本移除了 inheritsFrom，
    // 但合并时保留了 downloads.client，缺主 jar 时可直接据此下载到本版本目录）
    if let Some(client) = version_json.get("downloads").and_then(|v| v.get("client")) {
        let url = utils::get_str(client, "url");
        if !url.is_empty() {
            fallback_url = url;
            fallback_sha1 = utils::get_str(client, "sha1");
            fallback_size = client.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            fallback_jar_id = version_id.to_string();
        }
    }

    // 自身无 client 下载信息时，沿 inheritsFrom 链查找前置版本的 client 下载信息
    let mut visited = std::collections::HashSet::new();
    let mut cur_json = version_json.clone();
    let mut cur_inherits = utils::get_str(&cur_json, "inheritsFrom");
    while fallback_jar_id.is_empty() && !cur_inherits.is_empty() && !visited.contains(&cur_inherits) {
        visited.insert(cur_inherits.clone());
        let pj_path = versions_dir.join(&cur_inherits).join(format!("{}.json", cur_inherits));
        if !pj_path.exists() {
            break;
        }
        if let Ok(content) = std::fs::read_to_string(&pj_path)
            && let Ok(pj) = serde_json::from_str::<Value>(&content) {
                if let Some(client) = pj.get("downloads").and_then(|v| v.get("client")) {
                    let url = utils::get_str(client, "url");
                    if !url.is_empty() {
                        fallback_url = url;
                        fallback_sha1 = utils::get_str(client, "sha1");
                        fallback_size = client.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                        fallback_jar_id = cur_inherits.clone();
                        break;
                    }
                }
                cur_json = pj;
                cur_inherits = utils::get_str(&cur_json, "inheritsFrom");
                continue;
            }
        break;
    }

    result.main_jar.ok = false;
    result.main_jar.message = "主JAR文件缺失".to_string();
    if !fallback_url.is_empty() && !fallback_jar_id.is_empty() {
        let fallback_path = versions_dir.join(&fallback_jar_id).join(format!("{}.jar", fallback_jar_id));
        result.missing_files.push(MissingFile {
            kind: "main_jar".to_string(),
            url: fallback_url,
            path: fallback_path.to_string_lossy().to_string(),
            sha1: fallback_sha1,
            size: fallback_size,
            name: format!("{}.jar", fallback_jar_id),
            desc: String::new(),
            message: String::new(),
        });
    }
}
