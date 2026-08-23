// mods/list.rs — 已安装模组列表扫描
// 职责：扫描版本 mods 目录 + 共享 mods 目录 + .minecraft/mods，检测重复和冲突

use std::path::PathBuf;

use serde_json::{json, Value};

use super::jar::parse_mod_jar;
use crate::storage;

/// 获取已安装模组列表（含重复检测和冲突检测）
/// 版本取当前选中版本（selectedVersion）
pub fn get_installed_mods() -> Value {
    let settings = storage::load_settings();
    let version_id = settings
        .get("selectedVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    build_installed_mods(&settings, &version_id)
}

/// 获取指定版本的已安装模组列表
pub fn get_installed_mods_for_version(version_id: &str) -> Value {
    let settings = storage::load_settings();
    build_installed_mods(&settings, version_id)
}

fn build_installed_mods(settings: &Value, version_id: &str) -> Value {
    let mods_path = resolve_version_mods_dir(settings, version_id);
    let mut mods: Vec<Value> = Vec::new();
    let mut seen_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 扫描指定目录
    let scan_dir = |dir: &PathBuf, source: &str, mods: &mut Vec<Value>, seen: &mut std::collections::HashSet<String>| {
        if !dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let is_disabled = file_name.ends_with(".disabled");
            let clean_name = if is_disabled {
                file_name.trim_end_matches(".disabled").to_string()
            } else {
                file_name.clone()
            };
            if !clean_name.ends_with(".jar") {
                continue;
            }
            if seen.contains(&clean_name) {
                continue;
            }
            seen.insert(clean_name.clone());

            let name_no_ext = clean_name.trim_end_matches(".jar");
            let id = name_no_ext
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .replace("--", "-");

            let jar_path = entry.path();
            let stat = match std::fs::metadata(&jar_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let parsed = parse_mod_jar(&jar_path);

            mods.push(json!({
                "id": id,
                "slug": parsed.project_id,
                "name": if parsed.name.is_empty() { name_no_ext.to_string() } else { parsed.name.clone() },
                "fileName": file_name,
                "description": if parsed.desc.is_empty() {
                    if is_disabled { "已禁用".to_string() } else { "已安装的模组".to_string() }
                } else {
                    parsed.desc.clone()
                },
                "version": parsed.version,
                "enabled": !is_disabled,
                "disabled": is_disabled,
                "installed": true,
                "size": format_size(stat.len()),
                "source": source,
                "icon": if parsed.icon.is_empty() { "".to_string() } else { format!("/api/mod-icon?hash={}", parsed.icon) },
                "author": parsed.author,
                "projectId": parsed.project_id,
                "_jarPath": jar_path.to_string_lossy().replace('/', "\\")
            }));
        }
    };

    // 1. 版本 mods 目录
    if let Some(p) = &mods_path {
        scan_dir(p, "本地", &mut mods, &mut seen_files);
    }

    // 2. 非隔离时扫描共享目录
    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !version_id.is_empty() && !version_isolation {
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| storage::resolve_data_dir());
        let shared_mods = game_dir.join("mods");
        if Some(&shared_mods) != mods_path.as_ref() {
            scan_dir(&shared_mods, "共享", &mut mods, &mut seen_files);
        }

        // 3. .minecraft/mods
        if let Some(home) = dirs::home_dir() {
            let mc_mods = home
                .join("AppData")
                .join("Roaming")
                .join(".minecraft")
                .join("mods");
            if Some(&mc_mods) != mods_path.as_ref() && mc_mods != shared_mods {
                scan_dir(&mc_mods, ".minecraft", &mut mods, &mut seen_files);
            }
        }
    }

    // 重复检测 + 冲突检测
    let warnings = detect_warnings(&mods);

    json!({
        "mods": mods,
        "warnings": warnings
    })
}

/// 重复模组 + 冲突模组检测
fn detect_warnings(mods: &[Value]) -> Vec<Value> {
    let mut warnings: Vec<Value> = Vec::new();

    // 重复检测（按 projectId）
    let mut id_map: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
    for m in mods {
        let pid = m.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        if pid.is_empty() {
            continue;
        }
        let enabled = m.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        if let Some(existing) = id_map.get(pid) {
            let existing_enabled = existing
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if enabled && existing_enabled {
                warnings.push(json!({
                    "type": "duplicate",
                    "modId": pid,
                    "message": format!("重复模组: {} 与 {} 使用相同的ID ({})",
                        m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        existing.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        pid),
                    "mods": [
                        m.get("fileName").and_then(|v| v.as_str()).unwrap_or(""),
                        existing.get("fileName").and_then(|v| v.as_str()).unwrap_or("")
                    ]
                }));
            }
        } else {
            id_map.insert(pid.to_string(), m);
        }
    }

    // 冲突组检测
    let conflict_groups: &[(&str, &[&str], &str)] = &[
        ("渲染优化", &["sodium", "rubidium", "embeddium"], "多个渲染优化模组可能冲突"),
        ("服务端优化", &["lithium", "canary", "hamlib"], "多个服务端优化模组可能冲突"),
        ("光影", &["iris", "oculus"], "Iris和Oculus不能同时使用"),
        ("渲染", &["sodium", "optifine", "optifabric"], "Sodium和OptiFine不能同时使用"),
        ("API", &["fabric-api", "fabric", "quilted_fabric_api"], "多个Fabric/Quilt API可能冲突"),
        ("加载器", &["forge", "neoforge", "fmlloader"], "不能同时使用多个模组加载器"),
    ];

    for (group_name, ids, msg) in conflict_groups {
        let found: Vec<&Value> = mods
            .iter()
            .filter(|m| {
                let enabled = m.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
                if !enabled {
                    return false;
                }
                let pid = m
                    .get("projectId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let name = m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let file_name = m
                    .get("fileName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                ids.iter().any(|id| {
                    pid.contains(id) || name.contains(id) || file_name.contains(id)
                })
            })
            .collect();
        if found.len() > 1 {
            warnings.push(json!({
                "type": "conflict",
                "group": group_name,
                "message": format!("{}: {}", msg, found.iter().map(|m| m.get("name").and_then(|v| v.as_str()).unwrap_or("")).collect::<Vec<_>>().join(", ")),
                "mods": found.iter().map(|m| m.get("fileName").and_then(|v| v.as_str()).unwrap_or("")).collect::<Vec<_>>()
            }));
        }
    }

    warnings
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// 解析版本 mods 目录
fn resolve_version_mods_dir(settings: &Value, version_id: &str) -> Option<PathBuf> {
    if version_id.is_empty() {
        return None;
    }
    // 外部版本：mods 目录位于外部版本目录下（外部文件夹/versions/<id> 或 外部文件夹/<id>）
    if let Some(ext_mods) = resolve_external_version_mods_dir(version_id) {
        return Some(ext_mods);
    }
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let game_dir = crate::launch::args_builder::resolve_game_dir(
        version_id,
        None,
        None,
        settings,
        &versions_dir,
        &data_dir,
    );
    Some(game_dir.join("mods"))
}

/// 供外部模块解析指定版本的 mods 目录
pub fn resolve_mods_dir_for(settings: &Value, version_id: &str) -> Option<PathBuf> {
    resolve_version_mods_dir(settings, version_id)
}

/// 去掉版本 ID 上的 " [外部N]" / "[外部N]" 标记
fn clean_external_marker(version_id: &str) -> &str {
    if let Some(idx) = version_id.find(" [外部") {
        &version_id[..idx]
    } else if let Some(idx) = version_id.find("[外部") {
        version_id[..idx].trim_end()
    } else {
        version_id
    }
}

/// 在外部文件夹配置中解析该版本对应的 mods 目录
fn resolve_external_version_mods_dir(version_id: &str) -> Option<PathBuf> {
    let clean_id = clean_external_marker(version_id);
    if clean_id.is_empty() {
        return None;
    }
    let folders = storage::load_external_folders();
    for folder in folders {
        let path_str = folder.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path_str.is_empty() {
            continue;
        }
        let folder_path = PathBuf::from(path_str);
        if !folder_path.exists() {
            continue;
        }
        let ver_dir = folder_path.join("versions").join(clean_id);
        if ver_dir.join("mods").is_dir() || ver_dir.is_dir() {
            return Some(ver_dir.join("mods"));
        }
        let direct_dir = folder_path.join(clean_id);
        if direct_dir.join("mods").is_dir() || direct_dir.is_dir() {
            return Some(direct_dir.join("mods"));
        }
    }
    None
}

/// 解析存档目录（screenshots/saves 等用）
pub fn resolve_saves_dir(version_id: &str) -> PathBuf {
    let settings = storage::load_settings();
    let version_id = if version_id.is_empty() {
        settings
            .get("selectedVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        version_id.to_string()
    };
    if version_id.is_empty() {
        return storage::resolve_data_dir().join("saves");
    }
    let game_dir = resolve_version_game_dir(&settings, &version_id);
    game_dir.join("saves")
}

/// 解析版本游戏目录
fn resolve_version_game_dir(settings: &Value, version_id: &str) -> PathBuf {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    crate::launch::args_builder::resolve_game_dir(
        version_id,
        None,
        None,
        settings,
        &versions_dir,
        &data_dir,
    )
}
