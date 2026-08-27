// mods/jar.rs — JAR 文件解析与图标提取
// 职责：从 JAR 文件中提取模组元数据（名称、版本、作者、图标）
//
// 支持格式：
//   - fabric.mod.json（Fabric）
//   - META-INF/mods.toml（Forge 1.13+）
//   - META-INF/neoforge.mods.toml（NeoForge）

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use md5::Context as Md5Context;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use zip::ZipArchive;

/// 模组元数据
#[derive(Clone, Default)]
pub struct ModMeta {
    pub icon: String,
    pub name: String,
    pub desc: String,
    pub version: String,
    pub author: String,
    pub project_id: String,
}

/// LRU 缓存（简化版：达到上限时清空一半）
static MOD_META_CACHE: Mutex<Option<HashMap<String, ModMeta>>> = Mutex::new(None);
const CACHE_MAX: usize = 1000;

/// 解析模组 JAR 文件
/// 返回 ModMeta（含 icon hash，用于 /api/mod-icon?hash=xxx 拉取图标）
pub fn parse_mod_jar(jar_path: &Path) -> ModMeta {
    // 先查缓存
    let cache_key = jar_path.to_string_lossy().to_string();
    {
        let g = MOD_META_CACHE.lock().unwrap();
        if let Some(map) = g.as_ref() {
            if let Some(meta) = map.get(&cache_key) {
                return meta.clone();
            }
        }
    }

    // 缓存上限检查
    {
        let mut g = MOD_META_CACHE.lock().unwrap();
        if g.is_none() {
            *g = Some(HashMap::new());
        }
        if let Some(map) = g.as_mut() {
            if map.len() > CACHE_MAX {
                // 清空一半（简化 LRU）
                let keys: Vec<String> = map.keys().take(CACHE_MAX / 2).cloned().collect();
                for k in keys {
                    map.remove(&k);
                }
            }
        }
    }

    let mut result = ModMeta {
        version: "1.0".to_string(),
        ..Default::default()
    };

    // 文件过大跳过（>100MB）
    if let Ok(stat) = fs::metadata(jar_path) {
        if stat.len() > 100 * 1024 * 1024 {
            cache_meta(&cache_key, result.clone());
            return result;
        }
    }

    let file = match fs::File::open(jar_path) {
        Ok(f) => f,
        Err(_) => {
            cache_meta(&cache_key, result.clone());
            return result;
        }
    };

    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            cache_meta(&cache_key, result.clone());
            return result;
        }
    };

    // 第一遍：找元数据文件
    let mut fabric_json_entry: Option<usize> = None;
    let mut forge_toml_entry: Option<usize> = None;
    let mut neoforge_toml_entry: Option<usize> = None;

    for i in 0..archive.len() {
        let file = match archive.by_index_raw(i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let name = file.name().to_string();
        if name == "fabric.mod.json" {
            fabric_json_entry = Some(i);
        } else if name == "META-INF/neoforge.mods.toml" {
            neoforge_toml_entry = Some(i);
        } else if name == "META-INF/mods.toml" {
            forge_toml_entry = Some(i);
        }
    }

    // 解析 fabric.mod.json
    let mut icon_path: Option<String> = None;
    let mut fabric_project_id = String::new();
    if let Some(idx) = fabric_json_entry {
        if let Ok(mut file) = archive.by_index(idx) {
            let mut text = String::new();
            if file.read_to_string(&mut text).is_ok() {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    result.name = json
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    result.desc = json
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    result.version = json
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("1.0")
                        .to_string();
                    if let Some(authors) = json.get("authors").and_then(|a| a.as_array()) {
                        result.author = authors
                            .iter()
                            .filter_map(|a| {
                                a.as_str()
                                    .map(|s| s.to_string())
                                    .or_else(|| a.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                    }
                    result.project_id = json
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    fabric_project_id = result.project_id.clone();

                    // 解析图标路径
                    if let Some(icon) = json.get("icon") {
                        if let Some(s) = icon.as_str() {
                            icon_path = Some(s.to_string());
                        } else if let Some(obj) = icon.as_object() {
                            // 选 <=128 中最大的
                            let mut best_size = 0;
                            let mut best_key: Option<String> = None;
                            for (k, _v) in obj {
                                if let Some(size) = k.parse::<u32>().ok() {
                                    if size <= 128 && size > best_size {
                                        best_size = size;
                                        best_key = Some(k.clone());
                                    }
                                }
                            }
                            if let Some(k) = best_key {
                                icon_path = obj.get(&k).and_then(|v| v.as_str()).map(|s| s.to_string());
                            } else if let Some(first) = obj.values().next() {
                                icon_path = first.as_str().map(|s| s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // fabric.mod.json 无 icon 字段时找 assets/<id>/icon.png（在 archive 借用外搜索）
    if icon_path.is_none() && !fabric_project_id.is_empty() {
        let expected = format!("assets/{}/icon.png", fabric_project_id);
        for i in 0..archive.len() {
            if let Ok(f) = archive.by_index_raw(i) {
                if f.name() == expected {
                    icon_path = Some(expected);
                    break;
                }
            }
        }
    }

    // 解析 mods.toml / neoforge.mods.toml（正则提取关键字段）
    let toml_entries: Vec<usize> = [forge_toml_entry, neoforge_toml_entry]
        .iter()
        .filter_map(|e| *e)
        .collect();

    for idx in toml_entries {
        if let Ok(mut file) = archive.by_index(idx) {
            let mut text = String::new();
            if file.read_to_string(&mut text).is_ok() {
                parse_toml_mod(&text, &mut result, &mut icon_path);
            }
        }
    }

    // 无 icon_path 时找 pack.png / logo.png / icon.png（含 webp 等常见图标格式）
    if icon_path.is_none() {
        for i in 0..archive.len() {
            if let Ok(f) = archive.by_index_raw(i) {
                let name = f.name().to_string();
                let lower = name.to_lowercase();
                if lower == "pack.png"
                    || lower == "logo.png"
                    || lower == "icon.png"
                    || lower == "pack.webp"
                    || lower == "logo.webp"
                    || lower == "icon.webp"
                    || lower == "pack.jpg"
                    || lower == "logo.jpg"
                    || lower == "icon.jpg"
                    || lower.ends_with("/icon.png")
                    || lower.ends_with("/icon.webp")
                {
                    icon_path = Some(name);
                    break;
                }
            }
        }
    }

    // 提取图标到缓存目录
    if let Some(ip) = icon_path {
        let normalized = ip.replace('\\', "/");
        for i in 0..archive.len() {
            if let Ok(mut f) = archive.by_index(i) {
                if f.name().replace('\\', "/") == normalized {
                    // 读取图标数据
                    let mut buf = Vec::new();
                    if f.read_to_end(&mut buf).is_ok() {
                        // 计算 MD5(jarPath + iconPath)
                        let mut hasher = Md5Context::new();
                        hasher.consume(cache_key.as_bytes());
                        hasher.consume(b"|");
                        hasher.consume(normalized.as_bytes());
                        let hash = hasher.compute();
                        let hash_hex = format!("{:x}", hash);

                        let icon_cache_dir = crate::storage::resolve_data_dir().join("icon-cache");
                        let _ = fs::create_dir_all(&icon_cache_dir);
                        let cache_file = icon_cache_dir.join(format!("{}.png", hash_hex));
                        if !cache_file.exists() {
                            let _ = fs::write(&cache_file, &buf);
                        }
                        result.icon = hash_hex;
                    }
                    break;
                }
            }
        }
    }

    cache_meta(&cache_key, result.clone());
    result
}

/// 解析 mods.toml（正则提取字段）
fn parse_toml_mod(text: &str, result: &mut ModMeta, icon_path: &mut Option<String>) {
    // displayName
    if result.name.is_empty() {
        if let Some(caps) = regex::Regex::new(r#"displayName\s*=\s*"([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                result.name = m.as_str().to_string();
            }
        }
    }

    // description（优先 """...""" 多行，其次 "..." 单行）
    if result.desc.is_empty() {
        let re_multi = regex::Regex::new(r#"description\s*=\s*"""([\s\S]*?)""""#).ok();
        if let Some(re) = re_multi {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    result.desc = m.as_str().trim().to_string();
                }
            }
        }
        if result.desc.is_empty() {
            let re_single = regex::Regex::new(r#"description\s*=\s*"([^"]+)""#).ok();
            if let Some(re) = re_single {
                if let Some(caps) = re.captures(text) {
                    if let Some(m) = caps.get(1) {
                        result.desc = m.as_str().trim().to_string();
                    }
                }
            }
        }
    }

    // version
    if result.version == "1.0" {
        if let Some(caps) = regex::Regex::new(r#"version\s*=\s*"([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                result.version = m.as_str().to_string();
            }
        }
    }

    // authors（数组或单值）
    if result.author.is_empty() {
        if let Some(caps) = regex::Regex::new(r#"authors\s*=\s*\[([^\]]+)\]"#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                result.author = m
                    .as_str()
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
            }
        } else if let Some(caps) = regex::Regex::new(r#"author\s*=\s*"([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                result.author = m.as_str().to_string();
            }
        }
    }

    // modId
    if result.project_id.is_empty() {
        if let Some(caps) = regex::Regex::new(r#"modId\s*=\s*"([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                result.project_id = m.as_str().to_string();
            }
        }
    }

    // logoFile
    if icon_path.is_none() {
        if let Some(caps) = regex::Regex::new(r#"logoFile\s*=\s*"([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(text))
        {
            if let Some(m) = caps.get(1) {
                *icon_path = Some(m.as_str().to_string());
            }
        }
    }
}

/// 写入缓存
fn cache_meta(key: &str, meta: ModMeta) {
    let mut g = MOD_META_CACHE.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    if let Some(map) = g.as_mut() {
        map.insert(key.to_string(), meta);
    }
}

/// 计算 SHA1（用于更新检查）
pub fn compute_sha1_file(path: &Path) -> Option<String> {
    let mut hasher = Sha1::new();
    let mut file = fs::File::open(path).ok()?;
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    Some(hash.iter().map(|b| format!("{:02x}", b)).collect())
}
