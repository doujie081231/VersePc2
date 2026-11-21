// resolve.rs — 由 server_host.rs 拆分
#![allow(unused_imports)]

// 版本解析、加载器检测与 Java 选择
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::oneshot;
use crate::download::download_with_mirror;
use crate::storage;
use super::*;

pub(crate) fn load_version_json(version_id: &str) -> Option<Value> {
    if version_id.is_empty() {
        return None;
    }
    let direct = versions_dir().join(version_id).join(format!("{}.json", version_id));
    if let Some(v) = read_json_safe(&direct) {
        return Some(v);
    }
    // 扫描：目录名与 id 不一致的情况
    if let Ok(entries) = std::fs::read_dir(versions_dir()) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };
            let cand = versions_dir().join(&name).join(format!("{}.json", name));
            if let Some(j) = read_json_safe(&cand) {
                let j_id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if j_id == version_id || name == version_id {
                    return Some(j);
                }
            }
        }
    }
    None
}

/// 沿 inheritsFrom 链查找带 downloads.server 的版本 JSON
pub(crate) fn resolve_server_download(version_id: &str) -> Option<Value> {
    let mut visited = std::collections::HashSet::new();
    let mut cur = version_id.to_string();
    let mut depth = 0u32;
    while !cur.is_empty() && depth < 12 && visited.insert(cur.clone()) {
        depth += 1;
        let data = match load_version_json(&cur) {
            Some(d) => d,
            None => break,
        };
        if let Some(server) = data
            .get("downloads")
            .and_then(|d| d.get("server"))
            && server.get("url").and_then(|u| u.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
                let id = data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&cur)
                    .to_string();
                return Some(json!({
                    "versionId": id,
                    "server": server,
                    "javaVersion": data.get("javaVersion").cloned().unwrap_or(Value::Null),
                    "inheritsFrom": data.get("inheritsFrom").cloned().unwrap_or(Value::Null),
                    "chain": visited.iter().cloned().collect::<Vec<_>>()
                }));
            }
        cur = data
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
    }
    None
}

/// 识别客户端加载器类型与版本号
pub(crate) fn detect_client_loader(version_id: &str) -> Value {
    let data = load_version_json(version_id);
    let mut chain: Vec<Value> = Vec::new();
    let mut cur = data.clone();
    let mut depth = 0u32;
    while let Some(j) = cur {
        chain.push(j.clone());
        depth += 1;
        if depth >= 12 {
            break;
        }
        let parent_id = j.get("inheritsFrom").and_then(|v| v.as_str()).map(|s| s.to_string());
        cur = parent_id.and_then(|p| load_version_json(&p));
    }

    let top = data.clone().unwrap_or(json!({}));
    let mut all_libs: Vec<Value> = Vec::new();
    for j in &chain {
        if let Some(libs) = j.get("libraries").and_then(|v| v.as_array()) {
            all_libs.extend(libs.clone());
        }
    }
    let mut lib_str = String::new();
    for l in &all_libs {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            lib_str.push_str(name);
            lib_str.push(' ');
        }
    }
    let main_class = top.get("mainClass").and_then(|v| v.as_str()).unwrap_or("");
    lib_str.push_str(&format!("\"{}\"", main_class));

    // 从 arguments.game 抽取 fml.* 版本
    let mut fml_neo: Option<String> = None;
    let mut fml_forge: Option<String> = None;
    let mut fml_mc: Option<String> = None;
    for j in &chain {
        if let Some(args) = j.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
            for i in 0..args.len() {
                let v = args[i].as_str().unwrap_or("");
                if v == "--fml.neoForgeVersion" {
                    if let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_neo = Some(next.to_string());
                    }
                } else if v == "--fml.forgeVersion" {
                    if let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_forge = Some(next.to_string());
                    }
                } else if v == "--fml.mcVersion"
                    && let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_mc = Some(next.to_string());
                    }
            }
            if fml_neo.is_some() || fml_forge.is_some() {
                break;
            }
        }
    }

    let id_lower = version_id.to_lowercase();

    let mut loader = "vanilla".to_string();
    let mut loader_version: Option<String> = None;
    let mut mc_version: Option<String> = if fml_mc.is_some() {
        fml_mc.clone()
    } else {
        None
    };

    // 从链中取带 downloads.server 的版本 id 作为 mc_version 兜底
    for j in chain.iter().rev() {
        if j.get("downloads").and_then(|d| d.get("server")).is_some() {
            let dj = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if mc_version.as_deref().map(|s| !s.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(true)
                && !dj.is_empty()
            {
                mc_version = Some(dj.to_string());
            }
            break;
        }
    }

    // 兜底：从 versionId 正则提取
    if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true) {
        let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?(?:-pre\d+|-rc\d+)?)").ok();
        if let Some(re) = re
            && let Some(captures) = re.captures(version_id) {
                mc_version = Some(captures.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
            }
        if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
            && let Some(fmc) = &fml_mc {
                mc_version = Some(fmc.clone());
            }
        if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
            && let Some(ff) = &fml_forge {
                let re = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)-").ok();
                if let Some(re) = re
                    && let Some(c) = re.captures(ff) {
                        mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                    }
            }
    }

    // NeoForge 检测
    let neo_re = regex::Regex::new(r#"(?i)net\.neoforged:(?:neoforge|forge):([^:\s""]+)"#).ok();
    let neo_match = neo_re
        .as_ref()
        .and_then(|r| r.captures(&lib_str))
        .or_else(|| {
            regex::Regex::new(r"(?i)neoforge[-_]?(\d+\.\d+\.\d+[^\s/]*)")
                .ok()
                .and_then(|r| r.captures(&id_lower))
        })
        .or_else(|| {
            regex::Regex::new(r"(?i)NeoForge[-_ ]?(\d[\d.\-\w]*)")
                .ok()
                .and_then(|r| r.captures(version_id))
        });

    let is_neo = neo_match.is_some()
        || fml_neo.is_some()
        || lib_str.to_lowercase().contains("neoforge")
        || lib_str.to_lowercase().contains("fancymodloader")
        || id_lower.contains("neoforge");

    if is_neo {
        loader = "neoforge".to_string();
        if let Some(v) = &fml_neo {
            loader_version = Some(v.clone());
        } else if let Some(c) = &neo_match {
            let mut v = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            if v.to_lowercase().contains("neoforge")
                && let Ok(re) = regex::Regex::new(r"(\d+\.\d+\.\d+[\w.-]*)")
                    && let Some(c2) = re.captures(&v) {
                        v = c2.get(1).map(|m| m.as_str().to_string()).unwrap_or(v);
                    }
            loader_version = Some(v);
        }
    } else {
        // Forge 检测
        let forge_re1 = regex::Regex::new(r#"(?i)net\.minecraftforge:forge:([^:\s""]+)"#).ok();
        let forge_re2 = regex::Regex::new(r#"(?i)net\.minecraftforge:fmlloader:([^:\s""]+)"#).ok();
        let forge_match = forge_re1
            .as_ref()
            .and_then(|r| r.captures(&lib_str))
            .or_else(|| forge_re2.as_ref().and_then(|r| r.captures(&lib_str)));
        let id_forge = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)-[Ff]orge-(\d+\.\d+[\w.-]*)")
            .ok()
            .and_then(|r| r.captures(version_id));

        let is_forge = id_forge.is_some()
            || forge_match.is_some()
            || fml_forge.is_some()
            || lib_str.to_lowercase().contains("minecraftforge")
            || lib_str.to_lowercase().contains("modlauncher")
            || (id_lower.contains("forge") && !id_lower.contains("fabric"));

        if is_forge {
            loader = "forge".to_string();
            if let Some(c) = &id_forge {
                if mc_version.is_none() || !is_valid_mc_ver(mc_version.as_deref().unwrap_or("")) {
                    mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                }
                loader_version = Some(format!(
                    "{}-{}",
                    c.get(1).map(|m| m.as_str()).unwrap_or(""),
                    c.get(2).map(|m| m.as_str()).unwrap_or("")
                ));
            } else if let Some(c) = &forge_match {
                let v = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                loader_version = Some(v.clone());
                if mc_version.is_none()
                    && let Ok(re) = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)-")
                        && let Some(c2) = re.captures(&v) {
                            mc_version = Some(c2.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                        }
            } else if let Some(v) = &fml_forge {
                loader_version = Some(v.clone());
                if mc_version.is_none()
                    && let Some(fmc) = &fml_mc {
                        mc_version = Some(fmc.clone());
                    }
            }
        } else {
            // Fabric 检测
            let fab_re = regex::Regex::new(r#"(?i)net\.fabricmc:fabric-loader:([^:\s""]+)"#).ok();
            let fab_match = fab_re.as_ref().and_then(|r| r.captures(&lib_str));
            let id_fab = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)-[Ff]abric[-_ ]?(\d+\.\d+\.\d+)")
                .ok()
                .and_then(|r| r.captures(version_id))
                .or_else(|| {
                    regex::Regex::new(r"(?i)Fabric[-_ ](\d+\.\d+\.\d+)")
                        .ok()
                        .and_then(|r| r.captures(version_id))
                });

            if fab_match.is_some()
                || id_fab.is_some()
                || lib_str.to_lowercase().contains("fabric-loader")
                || lib_str.to_lowercase().contains("net.fabricmc")
                || id_lower.contains("fabric")
            {
                loader = "fabric".to_string();
                if let Some(c) = &id_fab {
                    if mc_version.is_none() {
                        mc_version = c.get(1).map(|m| m.as_str().to_string());
                    }
                    loader_version = c.get(2).map(|m| m.as_str().to_string());
                } else if let Some(c) = &fab_match {
                    loader_version = c.get(1).map(|m| m.as_str().to_string());
                }
            }
        }
    }

    // Fabric: 从 intermediary 推导 MC 版本
    if loader == "fabric"
        && mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
    {
        let intermediary_re = regex::Regex::new(r"intermediary:(\d+\.\d+(?:\.\d+)?)").ok();
        for l in &all_libs {
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.to_lowercase().contains("net.fabricmc:intermediary:")
                && let Some(re) = intermediary_re.as_ref()
                    && let Some(c) = re.captures(name) {
                        mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                        break;
                    }
        }
    }

    // Forge loaderVersion 拼接 mc 版本
    if loader == "forge"
        && loader_version.is_some()
        && mc_version.is_some()
        && !loader_version.as_deref().unwrap_or("").contains(mc_version.as_deref().unwrap_or(""))
        && is_valid_mc_ver(loader_version.as_deref().unwrap_or(""))
    {
        let lv = loader_version.clone().unwrap_or_default();
        let re = regex::Regex::new(r"^\d+\.\d+(\.\d+)?-").ok();
        if re.and_then(|r| r.captures(&lv)).is_none() {
            loader_version = Some(format!("{}-{}", mc_version.as_deref().unwrap_or(""), lv));
        }
    }

    // 从 loaderVersion 反推 mcVersion（仅 forge/neoforge）
    if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
        && loader_version.is_some()
        && loader != "fabric"
    {
        let lv = loader_version.clone().unwrap_or_default();
        if let Ok(re) = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)(?:-|$)")
            && let Some(c) = re.captures(&lv) {
                mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
            }
    }

    let mc_final = mc_version
        .filter(|s| is_valid_mc_ver(s))
        .or_else(|| {
            resolve_server_download(version_id)
                .and_then(|r| r.get("versionId").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .filter(|s| is_valid_mc_ver(s))
        })
        .or_else(|| extract_leading_mc_version(version_id))
        .unwrap_or_else(|| version_id.to_string());

    let chain_ids: Vec<Value> = chain
        .iter()
        .filter_map(|j| j.get("id").and_then(|v| v.as_str()).map(|s| json!(s)))
        .collect();

    json!({
        "loader": loader,
        "loaderVersion": loader_version,
        "mcVersion": mc_final,
        "chainIds": chain_ids
    })
}

pub(crate) fn is_valid_mc_ver(s: &str) -> bool {
    // 必须整体是 MC 版本号；不能用 "以数字开头" 判断，
    // 否则 "26.2-Fabric-0.20.1" 这类完整版本 ID 会被误当成 MC 版本传给加载器安装器。
    if let Ok(re) = regex::Regex::new(r"^\d+\.\d+(?:\.\d+)?(?:-(?:pre|rc)\d+)?$") {
        re.is_match(s)
    } else {
        false
    }
}

/// 从完整版本 ID/名称中提取开头的 MC 版本号（如 "26.2-Fabric-0.20.1" → "26.2"）
fn extract_leading_mc_version(s: &str) -> Option<String> {
    let re = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?(?:-(?:pre|rc)\d+)?)").ok()?;
    re.captures(s)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

/// 解析版本所需 Java 主版本（沿 inheritsFrom 链找 javaVersion.majorVersion）
pub(crate) fn resolve_required_java_major(version_id: &str) -> u32 {
    let mut visited = std::collections::HashSet::new();
    let mut cur = version_id.to_string();
    let mut depth = 0u32;
    while !cur.is_empty() && depth < 12 && visited.insert(cur.clone()) {
        depth += 1;
        if let Some(j) = load_version_json(&cur) {
            if let Some(mv) = j
                .get("javaVersion")
                .and_then(|jv| jv.get("majorVersion"))
                .and_then(|v| v.as_u64())
            {
                return mv as u32;
            }
            cur = j
                .get("inheritsFrom")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
        } else {
            break;
        }
    }
    // 兜底：按 MC 版本映射
    let re = regex::Regex::new(r"(\d+)(?:\.(\d+))?").ok();
    if let Some(re) = re
        && let Some(c) = re.captures(version_id) {
            let major: u32 = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let minor: u32 = c
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            if major >= 26 {
                return 25;
            }
            if major == 1 && minor >= 21 {
                return 21;
            }
            if major == 1 && minor == 20 {
                let patch: u32 = regex::Regex::new(r"\.(\d+)(?:\D|$)")
                    .ok()
                    .and_then(|re| re.captures(version_id))
                    .and_then(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
                    .unwrap_or(0);
                return if patch >= 5 { 21 } else { 17 };
            }
            if major == 1 && minor >= 18 {
                return 17;
            }
        }
    17
}

pub(crate) fn select_java_for_major(required_major: u32) -> Result<String, String> {
    let java_list = crate::java::detect_all();
    let mut best: Option<String> = None;
    let mut best_major: i32 = -1;
    let mut max_found: i32 = 0;
    for j in &java_list {
        let mv = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
        if mv > max_found {
            max_found = mv;
        }
        if mv >= required_major as i32 && (best_major < 0 || mv < best_major) {
            best = j.get("path").and_then(|v| v.as_str()).map(|s| s.to_string());
            best_major = mv;
        }
    }
    if let Some(p) = best {
        return Ok(p);
    }
    // 尝试 settings 全局 javaPath
    let settings = storage::load_settings();
    let global = settings.get("javaPath").and_then(|v| v.as_str()).unwrap_or("");
    if !global.is_empty() && std::path::Path::new(global).exists() {
        return Ok(global.to_string());
    }
    Err(format!(
        "此服务端需要 Java {}+，当前系统最高仅 Java {}。请到「Java 管理」页下载 Java {} 后重试。",
        required_major,
        if max_found > 0 { max_found.to_string() } else { "?".into() },
        required_major
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mc_version_validation_rejects_full_loader_ids() {
        assert!(is_valid_mc_ver("26.2"));
        assert!(is_valid_mc_ver("1.21.1"));
        assert!(is_valid_mc_ver("1.20.1-pre1"));
        assert!(!is_valid_mc_ver("26.2-Fabric-0.20.1"));
        assert!(!is_valid_mc_ver("26.3-NeoForge-0.20.1"));
    }

    #[test]
    fn leading_mc_version_extracts_from_loader_id() {
        assert_eq!(
            extract_leading_mc_version("26.2-Fabric-0.20.1").as_deref(),
            Some("26.2")
        );
        assert_eq!(
            extract_leading_mc_version("1.21.1-neoforge-21.1.0").as_deref(),
            Some("1.21.1")
        );
        assert_eq!(extract_leading_mc_version("MyPack"), None);
    }
}
