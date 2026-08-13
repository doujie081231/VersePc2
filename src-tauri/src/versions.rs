// versions.rs — Minecraft 版本管理
// 兼容原项目 server/api/routes/versions.js 核心功能
// 路由清单：
//   GET  /api/versions                       版本列表（远程 + 本地）
//   GET  /api/version/list-folders           外部文件夹列表
//   POST /api/version/add-folder             添加外部文件夹
//   POST /api/version/remove-folder          移除外部文件夹
//   GET  /api/version-local-details          本地版本详情
//   POST /api/version/delete                  删除版本
//   POST /api/version/rename                  重命名版本（写 customName）
//   GET  /api/version/open-folder             打开版本文件夹
//   GET  /api/version/export-info             导出信息
//   POST /api/version/description             设置描述
//   POST /api/version/favorite                收藏/取消
//   GET  /api/version/select-folder           选择文件夹（弹出对话框）
//   GET  /api/version-details                 远程版本详情
//   POST /api/version/repair                  同步修复缺失库文件与客户端 jar
//   GET  /api/version/diagnose                诊断版本完整性（返回缺失文件列表）
//   POST /api/version/repair-start            启动异步修复会话（返回 sessionId）— TODO
//   GET  /api/version/repair-progress         查询修复进度 — TODO
//   GET  /api/version/repair-cancel            取消修复会话 — TODO
//   POST /api/install-start                   安装新版本（占位）
//   GET  /api/install-progress                安装进度（占位）
//   POST /api/version/export-script           导出启动脚本（.bat/.sh）
//   POST /api/version/export-modpack          导出整合包（ZIP）
//   GET  /api/version-icon                    读取版本图标（base64 data URL）

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::storage;
use crate::utils;

/// 远程版本清单 URL
const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const BMCLAPI_MANIFEST_URL: &str = "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json";

// ============== 内置方块图标（编译时嵌入二进制） ==============
// 原项目从 dist/img/ 读取，这里用 include_bytes! 嵌入，运行时永远可用
const ICON_GRASS: &[u8] = include_bytes!("../../dist/img/Grass.png");
const ICON_COMMAND_BLOCK: &[u8] = include_bytes!("../../dist/img/CommandBlock.png");
const ICON_GOLD_BLOCK: &[u8] = include_bytes!("../../dist/img/GoldBlock.png");
const ICON_COBBLESTONE: &[u8] = include_bytes!("../../dist/img/CobbleStone.png");
const ICON_ANVIL: &[u8] = include_bytes!("../../dist/img/Anvil.png");
const ICON_FABRIC: &[u8] = include_bytes!("../../dist/img/Fabric.png");
const ICON_NEOFORGE: &[u8] = include_bytes!("../../dist/img/NeoForge.png");

/// 根据查询参数选择内置方块图标
fn get_builtin_icon(version_type: &str, is_forge: bool, is_fabric: bool, is_neoforge: bool, is_modpack: bool) -> &'static [u8] {
    // 加载器优先级：NeoForge > Forge > Fabric > 整合包 > 类型默认
    if is_neoforge {
        return ICON_NEOFORGE;
    }
    if is_forge {
        return ICON_ANVIL;
    }
    if is_fabric {
        return ICON_FABRIC;
    }
    if is_modpack {
        return ICON_ANVIL;
    }
    match version_type {
        "snapshot" => ICON_COMMAND_BLOCK,
        "release" => ICON_GRASS,
        "special" => ICON_GOLD_BLOCK,
        "old_beta" | "old_alpha" => ICON_COBBLESTONE,
        _ => ICON_GRASS,
    }
}

/// Tauri 命令：get_version_icon
/// 前端通过 invoke('get_version_icon', { id, versionType, forge, fabric, neoforge, modpack, extDir }) 调用
/// 返回 { data_url, mime } — 自定义图标返回 base64，方块图标也转 base64
/// 注：rename_all = "camelCase" 让前端可用 camelCase 参数名（versionType/extDir），
///     Rust 端参数仍是 snake_case（version_type/ext_dir）
#[tauri::command(rename_all = "camelCase")]
pub async fn get_version_icon(
    id: String,
    version_type: Option<String>,
    forge: Option<bool>,
    fabric: Option<bool>,
    neoforge: Option<bool>,
    modpack: Option<bool>,
    ext_dir: Option<String>,
) -> Value {
    println!("[version_icon] called: id={}, version_type={:?}, forge={}, fabric={}, neoforge={}, modpack={}, ext_dir={:?}",
        id, version_type, forge.unwrap_or(false), fabric.unwrap_or(false),
        neoforge.unwrap_or(false), modpack.unwrap_or(false), ext_dir);
    if id.is_empty() {
        println!("[version_icon] empty id, returning empty");
        return json!({ "success": false, "data_url": "" });
    }

    // 清洗 ID（去掉 " [外部N]" 后缀）
    let clean_id = id.split(" [外部").next().unwrap_or(&id).to_string();
    let is_forge = forge.unwrap_or(false);
    let is_fabric = fabric.unwrap_or(false);
    let is_neoforge = neoforge.unwrap_or(false);
    let is_modpack = modpack.unwrap_or(false);
    let v_type = version_type.unwrap_or_else(|| "release".to_string());

    // 1. 在版本目录找自定义图标（icon.png / pack.png / logo.png / PCL/Logo.png）
    let data_dir = storage::resolve_data_dir();
    let internal_dir = data_dir.join("versions").join(&clean_id);
    let icon_file_names = ["icon.png", "pack.png", "logo.png"];
    let pcl_logo = std::path::Path::new("PCL").join("Logo.png");

    // 优先级 1：内部版本目录
    let mut custom_icon: Option<(Vec<u8>, String)> = None;

    // PCL/Logo.png
    let pcl_path = internal_dir.join(&pcl_logo);
    if pcl_path.exists() {
        if let Ok(data) = std::fs::read(&pcl_path) {
            custom_icon = Some((data, "image/png".to_string()));
        }
    }
    // icon.png / pack.png / logo.png
    if custom_icon.is_none() {
        for fn_name in &icon_file_names {
            let p = internal_dir.join(fn_name);
            if p.exists() {
                if let Ok(data) = std::fs::read(&p) {
                    let mime = if fn_name.ends_with(".jpg") || fn_name.ends_with(".jpeg") {
                        "image/jpeg"
                    } else {
                        "image/png"
                    };
                    custom_icon = Some((data, mime.to_string()));
                    break;
                }
            }
        }
    }

    // 优先级 2：外部目录
    if custom_icon.is_none() {
        if let Some(ext) = &ext_dir {
            if !ext.is_empty() {
                let ext_path = std::path::Path::new(ext);
                // extDir 自身
                let pcl_path = ext_path.join(&pcl_logo);
                if pcl_path.exists() {
                    if let Ok(data) = std::fs::read(&pcl_path) {
                        custom_icon = Some((data, "image/png".to_string()));
                    }
                }
                if custom_icon.is_none() {
                    for fn_name in &icon_file_names {
                        let p = ext_path.join(fn_name);
                        if p.exists() {
                            if let Ok(data) = std::fs::read(&p) {
                                let mime = if fn_name.ends_with(".jpg") || fn_name.ends_with(".jpeg") {
                                    "image/jpeg"
                                } else {
                                    "image/png"
                                };
                                custom_icon = Some((data, mime.to_string()));
                                break;
                            }
                        }
                    }
                }
                // extDir 上两级
                if custom_icon.is_none() {
                    if let Some(grandparent) = ext_path.parent().and_then(|p| p.parent()) {
                        let pcl_path = grandparent.join(&pcl_logo);
                        if pcl_path.exists() {
                            if let Ok(data) = std::fs::read(&pcl_path) {
                                custom_icon = Some((data, "image/png".to_string()));
                            }
                        }
                        if custom_icon.is_none() {
                            for fn_name in &icon_file_names {
                                let p = grandparent.join(fn_name);
                                if p.exists() {
                                    if let Ok(data) = std::fs::read(&p) {
                                        let mime = if fn_name.ends_with(".jpg") || fn_name.ends_with(".jpeg") {
                                            "image/jpeg"
                                        } else {
                                            "image/png"
                                        };
                                        custom_icon = Some((data, mime.to_string()));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 优先级 3：.minecraft/versions/<id>
    if custom_icon.is_none() {
        if let Some(home) = dirs::home_dir() {
            let mc_versions_dir = home.join(".minecraft").join("versions").join(&clean_id);
            let pcl_path = mc_versions_dir.join(&pcl_logo);
            if pcl_path.exists() {
                if let Ok(data) = std::fs::read(&pcl_path) {
                    custom_icon = Some((data, "image/png".to_string()));
                }
            }
            if custom_icon.is_none() {
                for fn_name in &icon_file_names {
                    let p = mc_versions_dir.join(fn_name);
                    if p.exists() {
                        if let Ok(data) = std::fs::read(&p) {
                            let mime = if fn_name.ends_with(".jpg") || fn_name.ends_with(".jpeg") {
                                "image/jpeg"
                            } else {
                                "image/png"
                            };
                            custom_icon = Some((data, mime.to_string()));
                            break;
                        }
                    }
                }
            }
        }
    }

    // 4. 自定义图标：转 data URL 返回
    if let Some((data, mime)) = custom_icon {
        let data_url = utils::bytes_to_data_url(&data, &mime);
        return json!({ "success": true, "data_url": data_url, "is_custom": true });
    }

    // 5. 无自定义图标：用内置方块图标
    let icon_bytes = get_builtin_icon(&v_type, is_forge, is_fabric, is_neoforge, is_modpack);
    let data_url = utils::bytes_to_data_url(icon_bytes, "image/png");
    json!({ "success": true, "data_url": data_url, "is_custom": false })
}

/// 拉取远程版本清单（同步阻塞，但通过 spawn_blocking 在 Tauri 异步上下文里调用）
/// 按下载源选择优先顺序：china-first → BMCLAPI 优先；其他 → Mojang 优先
pub(crate) fn fetch_remote_manifest(refresh: bool) -> Option<Value> {
    let settings = storage::load_settings();
    let source = utils::get_str(&settings, "downloadSource");
    let cache_path = storage::resolve_data_dir().join("cache").join("version-manifest.json");

    // 非强制刷新时先读缓存（10 分钟内有效）
    if !refresh && cache_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            if let Ok(cached) = serde_json::from_str::<Value>(&content) {
                let timestamp = cached.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_millis() as u64;
                // 10 分钟内有效
                if now - timestamp < 600_000 {
                    return cached.get("data").cloned();
                }
            }
        }
    }

    // 决定 URL 顺序
    let urls: Vec<&str> = if source == "china-first" {
        vec![BMCLAPI_MANIFEST_URL, MOJANG_MANIFEST_URL]
    } else if source == "mojang" {
        vec![MOJANG_MANIFEST_URL]
    } else {
        // auto / official-first
        vec![MOJANG_MANIFEST_URL, BMCLAPI_MANIFEST_URL]
    };

    // 多源竞速：同时请求所有源，先成功者胜出（对应 PCL 的双源竞速策略）
    let manifest = fetch_json_racing(&urls, Duration::from_secs(6));

    // 写缓存
    if let Some(m) = &manifest {
        let _ = std::fs::create_dir_all(cache_path.parent().unwrap_or(Path::new(".")));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let cache = json!({ "data": m, "timestamp": now });
        if let Ok(s) = serde_json::to_string_pretty(&cache) {
            let _ = std::fs::write(&cache_path, s);
        }
    }

    manifest
}

/// 阻塞方式拉取 JSON（用 reqwest blocking，避免弹出 cmd 窗口）
fn fetch_json_blocking(url: &str) -> Option<Value> {
    fetch_json_blocking_with_timeout(url, Duration::from_secs(15))
}

/// 阻塞方式拉取 JSON，带指定超时
fn fetch_json_blocking_with_timeout(url: &str, timeout: Duration) -> Option<Value> {
    eprintln!("[versions] fetch_json_blocking: {}", url);
    // 带超时，避免国内访问被墙的官方源时长时间挂起而无法回退到镜像
    let client = match reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[versions] fetch_json_blocking client error: {}", e);
            return None;
        }
    };
    let resp = match client.get(url).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[versions] fetch_json_blocking reqwest error: {}", e);
            return None;
        }
    };
    let body = match resp.text() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[versions] fetch_json_blocking text error: {}", e);
            return None;
        }
    };
    eprintln!("[versions] fetch_json_blocking OK, body len={}", body.len());
    let json = serde_json::from_str(&body).ok();
    if json.is_none() {
        eprintln!("[versions] fetch_json_blocking JSON parse failed");
    }
    json
}

/// 多源竞速：并发请求多个 URL，返回第一个成功解析为 JSON 的结果。
/// 对应 PCL 的"双源竞速"策略：同时请求多个源，先成功者胜出，失败/慢源不阻塞整体。
/// 每个源有独立的超时，避免慢源或不可达源长时间挂起。
fn fetch_json_racing(urls: &[&str], per_url_timeout: Duration) -> Option<Value> {
    if urls.is_empty() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel::<Option<Value>>();
    for url in urls {
        let url = url.to_string();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let result = fetch_json_blocking_with_timeout(&url, per_url_timeout);
            let _ = tx.send(result);
        });
    }
    drop(tx);
    // rx.iter() 在所有发送端关闭后结束；返回第一个成功结果，其余线程各自超时收尾
    for received in rx.iter() {
        if let Some(v) = received {
            return Some(v);
        }
    }
    None
}

/// 扫描本地已安装版本（versions/ 目录）
fn scan_local_versions() -> Vec<Value> {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    scan_versions_in_dir(&versions_dir, false, None)
}

/// 扫描外部文件夹下的版本
fn scan_external_versions() -> Vec<Value> {
    let external_folders = storage::load_external_folders();
    let mut versions: Vec<Value> = Vec::new();

    for folder in external_folders {
        let path = utils::get_str(&folder, "path");
        if path.is_empty() {
            continue;
        }
        let folder_path = PathBuf::from(&path);
        if !folder_path.exists() {
            continue;
        }
        let ext_versions_dir = folder_path.join("versions");
        if !ext_versions_dir.exists() {
            continue;
        }
        let mut ext_versions = scan_versions_in_dir(&ext_versions_dir, true, Some(&folder_path));
        versions.append(&mut ext_versions);
    }

    versions
}

/// 过滤版本列表可见性
/// 对应原项目 server/versions/version-filter.js:filterVersionsByVisibility
///
/// 规则：
/// - 被整合包继承且自身 mods 为空的加载器版本（Forge/Fabric/NeoForge/OptiFine/LiteLoader）隐藏
/// - 被继承的原版基础版本始终显示（用户可直接启动原版）
/// - 不被继承的版本始终显示
fn filter_loader_visibility(versions: Vec<Value>) -> Vec<Value> {
    if versions.is_empty() {
        return versions;
    }

    // 建立 id -> 版本 映射
    let mut id_map: HashMap<String, Value> = HashMap::new();
    for v in &versions {
        let id = utils::get_str(v, "id");
        id_map.entry(id.clone()).or_insert_with(|| v.clone());
    }

    // 收集所有被 inheritsFrom 引用的加载器版本，并判断其自身 mods 是否为空
    // map: 被继承的加载器版本 id -> 是否保留（mods 非空才保留）
    let mut inherited_loader_keep: HashMap<String, bool> = HashMap::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut visited_chain: HashSet<String> = HashSet::new();
    let loader_re = regex::Regex::new(
        r"^(?:fabric-loader-\d|quilt-loader-\d|\d+\.\d+(?:\.\d+)?-(?:forge|neoforge)-\d)",
    )
    .unwrap();

    for v in &versions {
        let inherits = utils::get_str(v, "inheritsFrom");
        if inherits.is_empty() {
            continue;
        }
        let mut parent_id = inherits;
        visited_chain.clear();
        while !parent_id.is_empty() {
            if visited.contains(&parent_id) || visited_chain.contains(&parent_id) {
                break;
            }
            visited_chain.insert(parent_id.clone());
            let Some(parent) = id_map.get(&parent_id) else {
                break;
            };
            let is_forge = parent.get("isForge").and_then(|x| x.as_bool()).unwrap_or(false);
            let is_fabric = parent.get("isFabric").and_then(|x| x.as_bool()).unwrap_or(false);
            let is_neoforge = parent.get("isNeoForge").and_then(|x| x.as_bool()).unwrap_or(false);
            let is_optifine = parent.get("isOptiFine").and_then(|x| x.as_bool()).unwrap_or(false);
            let is_liteloader = parent.get("isLiteLoader").and_then(|x| x.as_bool()).unwrap_or(false);
            let is_loader = is_forge || is_fabric || is_neoforge || is_optifine || is_liteloader
                || loader_re.is_match(&parent_id);
            if is_loader {
                // 统计该加载器版本自身 mods 目录的 jar 数量
                let data_dir = storage::resolve_data_dir();
                let mods_dir = data_dir.join("versions").join(&parent_id).join("mods");
                let mod_count = count_mods_jar(&mods_dir);
                inherited_loader_keep.insert(parent_id.clone(), mod_count > 0);
            }
            let next = utils::get_str(parent, "inheritsFrom");
            if next.is_empty() {
                visited.insert(parent_id.clone());
                break;
            }
            visited.insert(parent_id.clone());
            parent_id = next;
        }
    }

    versions
        .into_iter()
        .filter(|v| {
            let id = utils::get_str(v, "id");
            match inherited_loader_keep.get(&id) {
                Some(keep) => *keep,
                None => true,
            }
        })
        .collect()
}

/// 统计 mods 目录中的 .jar 文件数量
fn count_mods_jar(mods_dir: &Path) -> usize {
    let entries = match std::fs::read_dir(mods_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.ends_with(".jar") && !name.ends_with(".jar.disabled")
        })
        .count()
}

/// 整合包自身无加载器标志时，从父版本继承加载器类型与基础 MC 版本。
/// 例如 RLCraft 的 version.json 只声明 inheritsFrom 指向 Forge 加载器版本，
/// 自身没有 libraries/mainClass，若不继承父加载器信息，会被设置页误判为"原版"。
fn inherit_loader_from_parent(versions: &mut Vec<Value>) {
    let snapshot = versions.clone();
    let bare_mc_re = regex::Regex::new(r"^\d+\.\d+(\.\d+)?(-\d+)?$").unwrap();
    for v in versions.iter_mut() {
        let is_modpack = v.get("isModpack").and_then(|x| x.as_bool()).unwrap_or(false);
        if !is_modpack {
            continue;
        }
        let has_own_loader = v.get("isFabric").and_then(|x| x.as_bool()).unwrap_or(false)
            || v.get("isForge").and_then(|x| x.as_bool()).unwrap_or(false)
            || v.get("isNeoForge").and_then(|x| x.as_bool()).unwrap_or(false)
            || v.get("isOptiFine").and_then(|x| x.as_bool()).unwrap_or(false)
            || v.get("isLiteLoader").and_then(|x| x.as_bool()).unwrap_or(false);
        if has_own_loader {
            continue;
        }
        let inherits = utils::get_str(v, "inheritsFrom");
        if inherits.is_empty() {
            continue;
        }
        let parent = snapshot.iter().find(|c| utils::get_str(c, "id") == inherits);
        let Some(parent) = parent else { continue };
        let p_forge = parent.get("isForge").and_then(|x| x.as_bool()).unwrap_or(false);
        let p_fabric = parent.get("isFabric").and_then(|x| x.as_bool()).unwrap_or(false);
        let p_neoforge = parent.get("isNeoForge").and_then(|x| x.as_bool()).unwrap_or(false);
        let p_optifine = parent.get("isOptiFine").and_then(|x| x.as_bool()).unwrap_or(false);
        let p_liteloader = parent.get("isLiteLoader").and_then(|x| x.as_bool()).unwrap_or(false);
        if !(p_forge || p_fabric || p_neoforge || p_optifine || p_liteloader) {
            continue;
        }
        if let Value::Object(m) = v {
            if p_forge {
                m.insert("isForge".to_string(), Value::Bool(true));
            }
            if p_fabric {
                m.insert("isFabric".to_string(), Value::Bool(true));
            }
            if p_neoforge {
                m.insert("isNeoForge".to_string(), Value::Bool(true));
            }
            if p_optifine {
                m.insert("isOptiFine".to_string(), Value::Bool(true));
            }
            if p_liteloader {
                m.insert("isLiteLoader".to_string(), Value::Bool(true));
            }
            // 基础 MC 版本：父版本若直接继承原版基础版本，则用它覆盖，便于前端模组筛选
            let p_base = utils::get_str(parent, "baseVersion");
            if !p_base.is_empty() && bare_mc_re.is_match(&p_base) {
                m.insert("baseVersion".to_string(), Value::String(p_base));
            }
        }
    }
}

/// 扫描指定 versions 目录下的所有版本
fn scan_versions_in_dir(versions_dir: &Path, is_external: bool, external_folder_root: Option<&Path>) -> Vec<Value> {
    let mut versions: Vec<Value> = Vec::new();
    let entries = match std::fs::read_dir(versions_dir) {
        Ok(e) => e,
        Err(_) => return versions,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        // 跳过特殊目录
        let name_lower = name.to_lowercase();
        if ["cache", "blclient", "pcl", "temp"].contains(&name_lower.as_str()) {
            continue;
        }

        // 找版本 JSON
        let version_json = find_version_json(&path, &name);
        if version_json.is_none() {
            continue;
        }
        let (json_content, json_path) = version_json.unwrap();
        let parsed: Value = match serde_json::from_str(&json_content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 提取版本信息
        let id = utils::get_str(&parsed, "id");
        let id = if id.is_empty() { name.clone() } else { id };
        let main_class = utils::get_str(&parsed, "mainClass");
        let release_time = utils::get_str(&parsed, "releaseTime");
        let inherits_from = utils::get_str(&parsed, "inheritsFrom");
        let version_type = utils::get_str(&parsed, "type");

        // 识别加载器
        let (is_fabric, is_forge, is_neoforge, is_optifine, is_liteloader, is_modpack, base_version) =
            detect_loader(&parsed, &id, &inherits_from, Some(&path));

        // 读取版本独立设置
        let version_settings = storage::load_version_settings(&id, is_external);
        let custom_name = utils::get_str(&version_settings, "customName");
        let description = utils::get_str(&version_settings, "description");
        let favorite = version_settings.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false);
        let isolation = utils::get_str(&version_settings, "isolation");

        // 检查 mods/saves/resourcepacks
        let mods_dir = path.join("mods");
        let saves_dir = path.join("saves");
        let rp_dir = path.join("resourcepacks");
        let has_mods = mods_dir.exists() && std::fs::read_dir(&mods_dir).map(|mut d| d.next().is_some()).unwrap_or(false);
        let has_saves = saves_dir.exists() && std::fs::read_dir(&saves_dir).map(|mut d| d.next().is_some()).unwrap_or(false);
        let has_rp = rp_dir.exists() && std::fs::read_dir(&rp_dir).map(|mut d| d.next().is_some()).unwrap_or(false);

        versions.push(json!({
            "id": id,
            "type": version_type,
            "releaseTime": release_time,
            "mainClass": main_class,
            "installed": true,
            "inheritsFrom": if inherits_from.is_empty() { Value::Null } else { json!(inherits_from) },
            "isFabric": is_fabric,
            "isForge": is_forge,
            "isNeoForge": is_neoforge,
            "isOptiFine": is_optifine,
            "isLiteLoader": is_liteloader,
            "isModpack": is_modpack,
            "modpackLoader": "",
            "baseVersion": base_version,
            "isAprilFools": false,
            "isExternal": is_external,
            "externalVersionDir": if is_external { Some(path.to_string_lossy().to_string()) } else { None },
            "externalPath": external_folder_root.map(|p| p.to_string_lossy().to_string()),
            "isolation": isolation == "on" || isolation == "global",
            "hasMods": has_mods,
            "hasSaves": has_saves,
            "hasResourcepacks": has_rp,
            "error": false,
            "errorReason": "",
            "customName": custom_name,
            "description": description,
            "favorite": favorite,
            "jsonPath": json_path.to_string_lossy()
        }));
    }

    versions
}

/// 找版本 JSON 文件
/// 1. 优先 <name>.json
/// 2. 扫描任意 .json 找第一个含 mainClass/libraries/inheritsFrom/minecraftArguments/arguments 的
fn find_version_json(version_dir: &Path, name: &str) -> Option<(String, PathBuf)> {
    // 1. <name>.json
    let prefer_path = version_dir.join(format!("{}.json", name));
    if prefer_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&prefer_path) {
            return Some((content, prefer_path));
        }
    }

    // 2. 任意 .json
    let entries = std::fs::read_dir(version_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            // 简单判断：含 mainClass 或 libraries 或 inheritsFrom 等关键字
            if content.contains("mainClass")
                || content.contains("libraries")
                || content.contains("inheritsFrom")
                || content.contains("minecraftArguments")
                || content.contains("arguments")
            {
                return Some((content, path));
            }
        }
    }
    None
}

/// 识别加载器类型
/// 返回 (isFabric, isForge, isNeoForge, isOptiFine, isLiteLoader, isModpack, baseVersion)
fn detect_loader(parsed: &Value, id: &str, inherits_from: &str, version_dir: Option<&std::path::Path>) -> (bool, bool, bool, bool, bool, bool, String) {
    let mut is_fabric = false;
    let mut is_forge = false;
    let mut is_neoforge = false;
    let mut is_optifine = false;
    let mut is_liteloader = false;

    // 检查 libraries 中是否有加载器特征
    if let Some(libs) = parsed.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            let name = utils::get_str(lib, "name").to_lowercase();
            if name.contains("net.fabricmc") || name.contains("fabric-loader") {
                is_fabric = true;
            }
            if name.contains("net.minecraftforge") || name.contains("fmlcore") {
                is_forge = true;
            }
            if name.contains("net.neoforged") {
                is_neoforge = true;
            }
            if name.contains("optifine") || name.contains("boush") {
                is_optifine = true;
            }
            if name.contains("liteloader") || name.contains("com.mumfrey") {
                is_liteloader = true;
            }
        }
    }

    // 从 ID 识别
    let id_lower = id.to_lowercase();
    if id_lower.contains("fabric") {
        is_fabric = true;
    }
    if id_lower.contains("forge") && !id_lower.contains("neoforge") {
        is_forge = true;
    }
    if id_lower.contains("neoforge") {
        is_neoforge = true;
    }
    if id_lower.contains("optifine") {
        is_optifine = true;
    }

    // 识别整合包：优先检查版本目录中是否有 pack-info.json 或 pack.png
    let mut is_modpack = false;
    if let Some(dir) = version_dir {
        if dir.join("pack-info.json").exists() || dir.join("pack.png").exists() {
            is_modpack = true;
        }
    }
    // 目录标记文件缺失时，按版本 ID + 继承关系判断（对齐原项目 version-parse.js）：
    // 版本 ID 不是"纯 MC 版本号"、也不是"加载器 ID"，则视为整合包。
    // 这样即使整合包顶层没有 pack-info.json/pack.png（如仅含 icon.png），也能正确识别。
    if !is_modpack {
        let bare_mc_re = regex::Regex::new(r"^\d+\.\d+(\.\d+)?(-\d+)?$").unwrap();
        let loader_id_re = regex::Regex::new(
            r"^(?:fabric-loader-\d|quilt-loader-\d|\d+\.\d+(?:\.\d+)?-(?:forge|neoforge)-\d)",
        )
        .unwrap();
        let is_bare_mc = bare_mc_re.is_match(id);
        let is_loader_id = loader_id_re.is_match(id);
        let inherits_non_mc = !inherits_from.is_empty()
            && !bare_mc_re.is_match(inherits_from);
        // 纯原版内容：无加载器标志、无引导启动、主类为原版客户端入口、无加载器库
        let has_no_loader = !is_fabric && !is_forge && !is_neoforge && !is_optifine && !is_liteloader;
        let main_class = utils::get_str(parsed, "mainClass").to_lowercase();
        let is_content_vanilla = has_no_loader
            && !main_class.contains("bootstraplauncher")
            && !main_class.contains("fml")
            && !main_class.contains("modlauncher")
            && (main_class.contains("net.minecraft.client.main") || main_class.is_empty());
        if !is_bare_mc && !is_loader_id && (!is_content_vanilla || inherits_non_mc) {
            is_modpack = true;
        }
    }

    // 基础版本：优先用 inheritsFrom
    let base_version = if !inherits_from.is_empty() {
        inherits_from.to_string()
    } else {
        // 从 ID 提取基础版本号
        extract_base_version(id)
    };

    (is_fabric, is_forge, is_neoforge, is_optifine, is_liteloader, is_modpack, base_version)
}

/// 从版本 ID 提取基础 Minecraft 版本号
/// 如 "1.20.1-neoforge-47.1.0" → "1.20.1"
fn extract_base_version(id: &str) -> String {
    // 匹配 1.x.x 形式
    let parts: Vec<&str> = id.split('-').collect();
    for part in parts {
        let segs: Vec<&str> = part.split('.').collect();
        if segs.len() >= 2 {
            // 检查是否数字
            if segs[0].parse::<u32>().is_ok() && segs[1].parse::<u32>().is_ok() {
                return part.to_string();
            }
        }
    }
    String::new()
}

/// 标记远程版本列表中的已安装版本
fn mark_installed(remote_versions: &mut Vec<Value>, local_ids: &[String]) {
    for v in remote_versions.iter_mut() {
        let id = utils::get_str(v, "id");
        let installed = local_ids.iter().any(|s| s == &id);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("installed".to_string(), json!(installed));
            obj.insert("size".to_string(), json!(""));
        }
    }
}

/// 愚人节版本识别：这些版本在官方清单中 type 为 snapshot/release，
/// 但用户习惯上归入"愚人节版"分类。这里按已知 ID 集合标记为 special。
fn is_april_fools_id(id: &str) -> bool {
    matches!(
        id,
        "26.1.1"
            | "26w14a"
            | "25w14craftmine"
            | "24w14potato"
            | "23w13a_or_b"
            | "22w13oneblockatatime"
            | "20w14infinite"
            | "3D Shareware v1.34"
            | "15w14a"
            | "1.RV-Pre1"
    )
}

/// 把愚人节版本的 type 统一标记为 special
fn mark_april_fools(remote_versions: &mut Vec<Value>) {
    for v in remote_versions.iter_mut() {
        let id = utils::get_str(v, "id");
        if is_april_fools_id(&id) {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("type".to_string(), json!("special"));
            }
        }
    }
}

/// 获取版本的游戏目录（考虑隔离）
fn get_version_game_dir(version_id: &str, is_external: bool) -> PathBuf {
    let settings = storage::load_settings();
    let version_isolation = settings.get("versionIsolation").and_then(|v| v.as_bool()).unwrap_or(true);

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    if is_external {
        // 外部版本：在外部文件夹的 versions/<id> 下
        // 简化：直接用 versions_dir/<id>
        versions_dir.join(version_id)
    } else if version_isolation {
        versions_dir.join(version_id)
    } else {
        // 不隔离：用全局 gameDir
        let game_dir = utils::get_str(&settings, "gameDir");
        if game_dir.is_empty() {
            dirs::home_dir().map(|h| h.join(".minecraft")).unwrap_or_else(|| data_dir.clone())
        } else {
            PathBuf::from(game_dir)
        }
    }
}

/// api_proxy 路由处理：版本管理相关
pub async fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<crate::api::ApiResult> {
    use crate::api::ApiResult;

    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 版本列表 =====
        "GET /api/versions" => {
            eprintln!("[versions] GET /api/versions called");
            let refresh = params.as_ref()
                .and_then(|p| p.get("refresh"))
                .and_then(|v| v.as_str())
                .map(|s| s == "true")
                .unwrap_or(false);
            let installed_only = params.as_ref()
                .and_then(|p| p.get("installed"))
                .and_then(|v| v.as_str())
                .map(|s| s == "true")
                .unwrap_or(false);
            eprintln!("[versions] refresh={}, installed_only={}", refresh, installed_only);

            // 本地版本
            let local_versions = scan_local_versions();
            let external_versions = scan_external_versions();
            let mut all_local: Vec<Value> = Vec::new();
            all_local.extend(local_versions.clone());
            all_local.extend(external_versions.clone());
            eprintln!("[versions] local_versions={}, external_versions={}", local_versions.len(), external_versions.len());

            // 整合包自身无加载器标志时，从父版本继承加载器类型（RLCraft 等），
            // 避免设置页把整合包误判为"原版"
            inherit_loader_from_parent(&mut all_local);

            // 过滤被整合包继承且自身 mods 为空的加载器版本（如 Forge 底座版本），
            // 与原项目 version-list.js 的可见性过滤保持一致
            all_local = filter_loader_visibility(all_local);

            // 收集本地 ID（用于标记远程版本）
            let local_ids: Vec<String> = all_local.iter()
                .map(|v| utils::get_str(v, "id"))
                .collect();

            if installed_only {
                eprintln!("[versions] installed_only, returning local only");
                return Some(ApiResult::ok(json!({
                    "latest": { "release": "", "snapshot": "" },
                    "versions": all_local,
                    "installed": all_local
                })));
            }

            // 拉远程清单（spawn_blocking 避免阻塞 Tauri 异步运行时）
            // 8 秒总超时，超时则仅返回本地版本
            eprintln!("[versions] fetching remote manifest...");
            let manifest = tokio::time::timeout(
                std::time::Duration::from_secs(8),
                tokio::task::spawn_blocking(move || fetch_remote_manifest(refresh)),
            ).await;
            eprintln!("[versions] manifest timeout result: {:?}", manifest.is_ok());
            let manifest = manifest.ok().and_then(|r| r.ok()).flatten();
            eprintln!("[versions] manifest loaded: {}", manifest.is_some());

            let (latest, mut remote_versions) = match manifest {
                Some(m) => {
                    let latest_release = m.get("latest").and_then(|l| l.get("release")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let latest_snapshot = m.get("latest").and_then(|l| l.get("snapshot")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let versions = m.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    eprintln!("[versions] remote versions count: {}", versions.len());
                    (json!({ "release": latest_release, "snapshot": latest_snapshot }), versions)
                }
                None => (json!({ "release": "", "snapshot": "" }), Vec::new()),
            };

            // 标记已安装
            mark_installed(&mut remote_versions, &local_ids);

            // 愚人节版本在官方清单中 type 多为 snapshot/release，
            // 这里统一标记为 special，让前端"愚人节版"tab 能正确归类展示
            mark_april_fools(&mut remote_versions);

            Some(ApiResult::ok(json!({
                "latest": latest,
                "versions": remote_versions,
                "installed": all_local
            })))
        }

        // ===== 外部文件夹列表 =====
        "GET /api/version/list-folders" => {
            let folders = storage::load_external_folders();
            let mut result: Vec<Value> = Vec::new();
            for folder in folders {
                let path_str = utils::get_str(&folder, "path");
                let name = utils::get_str(&folder, "name");
                let added_at = utils::get_str(&folder, "addedAt");
                let path = PathBuf::from(&path_str);
                let exists = path.exists();
                let version_count = if exists {
                    scan_versions_in_dir(&path.join("versions"), true, Some(&path)).len() as u64
                } else {
                    0
                };
                result.push(json!({
                    "path": path_str,
                    "name": name,
                    "addedAt": added_at,
                    "exists": exists,
                    "versionCount": version_count
                }));
            }
            Some(ApiResult::ok(json!({ "success": true, "folders": result })))
        }

        // ===== 添加外部文件夹 =====
        "POST /api/version/add-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let path_str = utils::get_str(&data, "path");
            let name = utils::get_str(&data, "name");
            if path_str.is_empty() {
                return Some(ApiResult::err(400, "Missing path"));
            }
            let folder_path = PathBuf::from(&path_str);
            if !folder_path.exists() {
                return Some(ApiResult::ok(json!({ "success": false, "error": format!("文件夹不存在: {}", path_str) })));
            }
            if !folder_path.is_dir() {
                return Some(ApiResult::ok(json!({ "success": false, "error": format!("路径不是文件夹: {}", path_str) })));
            }
            // 校验：必须包含 versions 子目录
            let versions_dir = folder_path.join("versions");
            if !versions_dir.exists() {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹下未找到 versions 子目录，请选择有效的 Minecraft 文件夹" })));
            }
            let mut folders = storage::load_external_folders();
            // 去重
            if folders.iter().any(|f| utils::get_str(f, "path") == path_str) {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹已添加" })));
            }
            // 扫描并校验是否存在有效版本
            let scanned_versions = scan_versions_in_dir(&versions_dir, true, Some(&folder_path));
            if scanned_versions.is_empty() {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹下未找到有效的 Minecraft 版本" })));
            }
            let name = if name.is_empty() {
                folder_path.file_name().and_then(|n| n.to_str()).unwrap_or("外部").to_string()
            } else {
                name
            };
            folders.push(json!({
                "path": path_str,
                "name": name,
                "addedAt": utils::now_iso()
            }));
            storage::save_external_folders(&folders);
            Some(ApiResult::ok(json!({ "success": true, "versions": scanned_versions })))
        }

        // ===== 移除外部文件夹 =====
        "POST /api/version/remove-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let path_str = utils::get_str(&data, "path");
            let mut folders = storage::load_external_folders();
            folders.retain(|f| utils::get_str(f, "path") != path_str);
            storage::save_external_folders(&folders);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        // ===== 本地版本详情 =====
        "GET /api/version-local-details" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            // 判断是否外部版本
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };
            let settings = storage::load_version_settings(&clean_id, is_external);
            Some(ApiResult::ok(json!({
                "hasMods": false,
                "hasSaves": false,
                "hasResourcepacks": false,
                "error": false,
                "errorReason": "",
                "customName": utils::get_str(&settings, "customName"),
                "description": utils::get_str(&settings, "description"),
                "favorite": settings.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false)
            })))
        }

        // ===== 版本独立设置（读取） =====
        "GET /api/version/settings" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };
            let settings = storage::load_version_settings(&clean_id, is_external);
            Some(ApiResult::ok(settings))
        }

        // ===== 版本独立设置（保存，合并已有字段） =====
        "POST /api/version/settings/save" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id.clone()
            };
            // 读取现有设置并合并
            let mut settings = storage::load_version_settings(&clean_id, is_external);
            if let (Some(dst), Some(src)) = (settings.as_object_mut(), data.as_object()) {
                for (k, v) in src {
                    // 跳过 versionId（保持原值）
                    if k == "versionId" {
                        continue;
                    }
                    dst.insert(k.clone(), v.clone());
                }
            }
            let ok = storage::save_version_settings(&clean_id, is_external, &settings);
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 版本图标设置（保存自定义图标类型） =====
        "POST /api/version/icon" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let icon = utils::get_str(&data, "icon");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "icon", json!(icon));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 删除版本 =====
        "POST /api/version/delete" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let permanent = data.get("permanent").and_then(|v| v.as_bool()).unwrap_or(false);
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id.clone()
            };

            let data_dir = storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let version_dir = versions_dir.join(storage::sanitize_version_id(&clean_id));

            if !version_dir.exists() {
                return Some(ApiResult::err(404, "版本不存在"));
            }

            // 删除目录
            let result = if permanent {
                std::fs::remove_dir_all(&version_dir)
            } else {
                // 优先送回收站，失败回退强制删除
                send_to_recycle_bin(&version_dir).or_else(|_| std::fs::remove_dir_all(&version_dir))
            };

            match result {
                Ok(_) => Some(ApiResult::ok(json!({
                    "success": true,
                    "deleted": [clean_id],
                    "permanent": permanent
                }))),
                Err(e) => Some(ApiResult::err(500, &format!("删除失败: {}", e))),
            }
        }

        // ===== 重命名版本（写 customName 到 version-settings.json） =====
        "POST /api/version/rename" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let new_name = utils::get_str(&data, "newName");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "customName", json!(new_name));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 打开版本文件夹 =====
        "GET /api/version/open-folder" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let folder_type = params.as_ref()
                .and_then(|p| p.get("folderType"))
                .and_then(|v| v.as_str())
                .unwrap_or("version");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };

            let data_dir = storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let version_dir = versions_dir.join(storage::sanitize_version_id(&clean_id));

            let target = match folder_type {
                "version" => version_dir,
                "saves" => version_dir.join("saves"),
                "mods" => version_dir.join("mods"),
                "resourcepacks" => version_dir.join("resourcepacks"),
                "shaderpacks" => version_dir.join("shaderpacks"),
                "logs" => version_dir.join("logs"),
                "crash-reports" => version_dir.join("crash-reports"),
                _ => version_dir,
            };

            if !target.exists() {
                let _ = std::fs::create_dir_all(&target);
            }

            if open_in_explorer(&target) {
                Some(ApiResult::ok(json!({ "success": true, "path": target.to_string_lossy() })))
            } else {
                Some(ApiResult::err(500, "无法打开文件夹"))
            }
        }

        // ===== 设置描述 =====
        "POST /api/version/description" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let description = utils::get_str(&data, "description");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "description", json!(description));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 收藏/取消收藏 =====
        "POST /api/version/favorite" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let favorite = data.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false);
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "favorite", json!(favorite));
            Some(ApiResult::ok(json!({ "success": ok, "favorite": favorite })))
        }

        // ===== 导出信息 =====
        "GET /api/version/export-info" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let is_external = version_id.contains(" [外部");
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };

            let data_dir = storage::resolve_data_dir();
            let version_dir = data_dir.join("versions").join(&clean_id);
            let mods_dir = version_dir.join("mods");
            let saves_dir = version_dir.join("saves");
            let rp_dir = version_dir.join("resourcepacks");

            // 统计 mods 数量
            let mod_count = if mods_dir.exists() {
                std::fs::read_dir(&mods_dir)
                    .map(|d| d.filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path().extension().and_then(|x| x.to_str()).map(|s| s == "jar" || s == "zip").unwrap_or(false)
                        })
                        .count())
                    .unwrap_or(0)
            } else { 0 };

            // 统计 saves 数量
            let mut saves: Vec<String> = Vec::new();
            if saves_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&saves_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() && entry.path().join("level.dat").exists() {
                            if let Some(name) = entry.file_name().to_str() {
                                saves.push(name.to_string());
                                if saves.len() >= 20 {
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // resourcepacks zip 文件
            let mut resource_packs: Vec<String> = Vec::new();
            if rp_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&rp_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().and_then(|x| x.to_str()) == Some("zip") {
                            if let Some(name) = entry.file_name().to_str() {
                                resource_packs.push(name.to_string());
                            }
                        }
                    }
                }
            }

            // gameDesc：简化版（完整需要解析加载器版本）
            let game_desc = clean_id.clone();

            Some(ApiResult::ok(json!({
                "gameDesc": game_desc,
                "resourcePacks": resource_packs,
                "modCount": mod_count,
                "savesCount": saves.len(),
                "saves": saves
            })))
        }

        // ===== 选择文件夹（弹出对话框） =====
        "GET /api/version/select-folder" => {
            // 这个路由由前端 dialog_open 命令处理，这里返回提示
            // 前端应该调用 invoke('select_folder', ...) 而不是走 api_proxy
            Some(ApiResult::ok(json!({
                "success": false,
                "cancelled": true,
                "message": "请使用对话框命令选择文件夹"
            })))
        }

        // ===== 远程版本详情 =====
        "GET /api/version-details" => {
            let url = params.as_ref()
                .and_then(|p| p.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                return Some(ApiResult::err(400, "Missing url"));
            }
            // 同步阻塞拉取
            let details = fetch_json_blocking(url);

            match details {
                Some(d) => Some(ApiResult::ok(d)),
                None => Some(ApiResult::err(502, "无法获取版本详情")),
            }
        }

        // ===== 安装相关已迁移到 api/download.rs =====
        // install-start / install-progress / install-cancel / check-version-name

        // ===== 清理版本目录（删 natives/logs/crash-reports） =====
        "GET /api/version/cleanup" => {
            let version_id = params
                .as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let data_dir = crate::storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let version_dir = versions_dir.join(version_id);
            if !version_dir.exists() {
                return Some(ApiResult::err(404, "版本目录不存在"));
            }

            let mut deleted: Vec<String> = Vec::new();
            let cleanup_dirs = ["natives", "logs", "crash-reports", ".fabric", ".quilt"];
            for sub in &cleanup_dirs {
                let p = version_dir.join(sub);
                if p.exists() {
                    if std::fs::remove_dir_all(&p).is_ok() {
                        deleted.push(sub.to_string());
                    }
                }
            }
            Some(ApiResult::ok(json!({
                "success": true,
                "deleted": deleted
            })))
        }

        // ===== 设置版本分类（写入 version-settings.json） =====
        "POST /api/version/category" => {
            let version_id = body
                .as_ref()
                .and_then(|b| b.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let category = body
                .as_ref()
                .and_then(|b| b.get("category"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            // 读取现有 version-settings.json
            let data_dir = crate::storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let settings_path = versions_dir.join("version-settings.json");
            let mut settings_json: Value = if settings_path.exists() {
                std::fs::read_to_string(&settings_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(json!({ "versions": {} }))
            } else {
                json!({ "versions": {} })
            };

            // 更新对应版本的 category
            if let Some(versions) = settings_json
                .get_mut("versions")
                .and_then(|v| v.as_object_mut())
            {
                let entry = versions
                    .entry(version_id.to_string())
                    .or_insert_with(|| json!({}));
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("category".to_string(), json!(category));
                }
            }

            // 写回
            let content = serde_json::to_string_pretty(&settings_json).unwrap_or_default();
            match std::fs::write(&settings_path, content) {
                Ok(_) => Some(ApiResult::ok(json!({ "success": true }))),
                Err(e) => Some(ApiResult::err(
                    500,
                    &format!("写入失败: {}", e),
                )),
            }
        }

        // ===== 删除版本链（递归收集子版本一起删除） =====
        "POST /api/version/delete-chain" => {
            let version_id = body
                .as_ref()
                .and_then(|b| b.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let data_dir = crate::storage::resolve_data_dir();
            let versions_root = data_dir.join("versions");

            // 收集所有引用此版本作为 inheritsFrom 的子版本
            let mut to_delete: Vec<String> = vec![version_id.to_string()];
            let mut visited = std::collections::HashSet::new();
            visited.insert(version_id.to_string());

            // 递归查找子版本
            let mut changed = true;
            while changed {
                changed = false;
                if let Ok(entries) = std::fs::read_dir(&versions_root) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if visited.contains(name) {
                                continue;
                            }
                            // 检查这个版本的 JSON 是否 inheritsFrom 当前要删除的版本之一
                            let json_path = versions_root.join(name).join(format!("{}.json", name));
                            if !json_path.exists() {
                                continue;
                            }
                            if let Ok(content) = std::fs::read_to_string(&json_path) {
                                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                                    if let Some(inherits) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
                                        if visited.contains(inherits) {
                                            to_delete.push(name.to_string());
                                            visited.insert(name.to_string());
                                            changed = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 执行删除
            let mut deleted: Vec<String> = Vec::new();
            let mut failed: Vec<Value> = Vec::new();
            for vid in &to_delete {
                let dir = versions_root.join(vid);
                if !dir.exists() {
                    continue;
                }
                // 优先送回收站，失败则直接删除
                match send_to_recycle_bin(&dir) {
                    Ok(_) => {
                        deleted.push(vid.clone());
                    }
                    Err(_) => {
                        if let Err(e) = std::fs::remove_dir_all(&dir) {
                            failed.push(json!({
                                "versionId": vid,
                                "error": e.to_string()
                            }));
                        } else {
                            deleted.push(vid.clone());
                        }
                    }
                }
            }

            Some(ApiResult::ok(json!({
                "success": failed.is_empty(),
                "deleted": deleted,
                "failed": failed,
                "total": to_delete.len()
            })))
        }

        // ===== 版本修复（同步） =====
        "POST /api/version/repair" => Some(handle_repair(body).await),

        // ===== 版本诊断 =====
        "GET /api/version/diagnose" => Some(handle_diagnose(params)),

        // ===== 异步修复会话（TODO: 后续迁移完整流程） =====
        "POST /api/version/repair-start" => Some(ApiResult::err(501, "异步修复会话正在迁移中，请使用同步修复 POST /api/version/repair")),
        "GET /api/version/repair-progress" => Some(ApiResult::err(501, "异步修复进度正在迁移中")),
        "GET /api/version/repair-cancel" => Some(ApiResult::err(501, "取消修复正在迁移中")),

        // ===== 导出启动脚本（.bat / .sh） =====
        "POST /api/version/export-script" => Some(handle_export_script(body).await),

        // ===== 导出整合包（ZIP） =====
        "POST /api/version/export-modpack" => Some(handle_export_modpack(body)),

        // ===== 读取版本图标文件（base64） =====
        "GET /api/version-icon" => Some(handle_version_icon(params)),

        _ => None,
    }
}

/// 在资源管理器中打开路径
fn open_in_explorer(path: &Path) -> bool {
    if !path.exists() {
        let _ = std::fs::create_dir_all(path);
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe").arg(path).spawn().is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().is_ok()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// 送回回收站（Windows 用 PowerShell VisualBasic）
/// 对齐原项目 server/api/routes/versions.js 的回收站实现：
/// `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory(..., 'OnlyErrorDialogs', 'SendToRecycleBin')`
/// 相比 Shell.Application 的 InvokeVerb('delete')（异步、不可靠、被占用时静默失败），
/// 该方式同步执行、明确指定 SendToRecycleBin，能正确返回失败状态供上层回退。
#[cfg(target_os = "windows")]
fn send_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // 单引号需转义为两个单引号，避免 PowerShell 字符串提前闭合
    let escaped = path.to_string_lossy().replace('\'', "''");
    let ps_cmd = format!(
        "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', 'OnlyErrorDialogs', 'SendToRecycleBin')",
        escaped
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("回收站操作失败 (exit {:?})", status.code()),
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_to_recycle_bin(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "暂不支持"))
}

// ============== 版本修复 ==============

/// POST /api/version/repair — 同步修复缺失的库文件与客户端 jar
///
/// 流程：
/// 1. 复用 dep_check 依赖检查，获取所有缺失文件（含 natives 原生库与客户端 jar）
/// 2. 遍历缺失文件，通过 download_with_mirror 使用用户配置的下载源下载
///
/// 返回：{ success, repaired: N, missing: [...] }
async fn handle_repair(body: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let version_dir = versions_dir.join(&version_id);

    if !version_dir.exists() {
        return ApiResult::err(404, "版本目录不存在");
    }

    // 使用用户配置的下载源（默认 china-first），避免国内直连官方源超时
    let settings = storage::load_settings();
    let configured_source = utils::get_str(&settings, "downloadSource");
    let download_source = if configured_source.is_empty() {
        "china-first"
    } else {
        configured_source.as_str()
    };

    // 复用依赖检查，获取所有缺失文件（含 natives 原生库与客户端 jar）
    let dep_result = crate::launch::dep_check::check_dependencies(
        &version_id,
        &settings,
        None,
    );

    let mut repaired: u32 = 0;
    let mut missing: Vec<String> = Vec::new();

    for file in &dep_result.missing_files {
        if file.url.is_empty() {
            continue;
        }

        let dest = std::path::PathBuf::from(&file.path);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[repair] 创建目录失败 {}: {}", parent.display(), e);
                missing.push(file.name.clone());
                continue;
            }
        }

        let sha1 = if file.sha1.is_empty() { None } else { Some(file.sha1.as_str()) };
        let size = if file.size > 0 { Some(file.size) } else { None };

        eprintln!("[repair] 下载缺失文件: {}", file.name);
        match crate::download::download_with_mirror(
            &file.url,
            &dest,
            sha1,
            size,
            download_source,
            120,
            None,
        ).await {
            Ok(()) => repaired += 1,
            Err(e) => {
                eprintln!("[repair] 下载失败 {}: {}", file.name, e);
                missing.push(file.name.clone());
            }
        }
    }

    ApiResult::ok(json!({
        "success": missing.is_empty(),
        "repaired": repaired,
        "missing": missing
    }))
}

/// GET /api/version/diagnose — 诊断版本完整性
///
/// 扫描所有库文件，返回缺失和损坏的文件列表
fn handle_diagnose(params: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;

    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let libraries_dir = data_dir.join("libraries");
    let version_dir = versions_dir.join(version_id);

    if !version_dir.exists() {
        return ApiResult::err(404, "版本目录不存在");
    }

    // 解析版本 JSON
    let version_json = match resolve_version_json_recursive(&version_id, &versions_dir) {
        Some(j) => j,
        None => return ApiResult::err(400, "版本JSON文件缺失"),
    };

    let mut missing_libs: Vec<String> = Vec::new();
    let mut corrupt_libs: Vec<String> = Vec::new();
    let mut total_libs: u32 = 0;

    if let Some(libs) = version_json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            let lib_name = match lib.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            let suffix = lib_name.split(':').last().unwrap_or("");
            if suffix.starts_with("natives-") {
                continue;
            }
            if lib_name.starts_with("net.neoforged:neoforge:") && lib_name.ends_with(":client") {
                continue;
            }

            total_libs += 1;

            let download_artifact = lib.get("downloads").and_then(|d| d.get("artifact"));
            let lib_path = download_artifact
                .and_then(|a| a.get("path"))
                .and_then(|v| v.as_str())
                .map(|p| libraries_dir.join(p))
                .unwrap_or_else(|| resolve_library_path_from_name(&lib_name, &libraries_dir));

            if !lib_path.exists() {
                missing_libs.push(lib_name);
            } else if lib_path.extension().and_then(|e| e.to_str()) == Some("jar") {
                // 简单完整性检查：文件大小 > 1KB
                if let Ok(meta) = std::fs::metadata(&lib_path) {
                    if meta.len() < 1024 {
                        corrupt_libs.push(lib_name);
                    }
                }
            }
        }
    }

    // 检查客户端 jar
    let jar_path = version_dir.join(format!("{}.jar", version_id));
    let client_jar_ok = if jar_path.exists() {
        std::fs::metadata(&jar_path).map(|m| m.len() > 0).unwrap_or(false)
    } else {
        false
    };

    ApiResult::ok(json!({
        "versionId": version_id,
        "totalLibs": total_libs,
        "missingLibs": missing_libs,
        "corruptLibs": corrupt_libs,
        "clientJarOk": client_jar_ok,
        "healthy": missing_libs.is_empty() && corrupt_libs.is_empty() && client_jar_ok
    }))
}

/// 递归解析版本 JSON（合并 inheritsFrom 的 libraries）
///
/// 简化版：只合并 libraries 数组，不深度合并其他字段
fn resolve_version_json_recursive(version_id: &str, versions_dir: &Path) -> Option<Value> {
    let json_path = versions_dir.join(version_id).join(format!("{}.json", version_id));
    if !json_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&json_path).ok()?;
    let mut json: Value = serde_json::from_str(&content).ok()?;

    // 合并 inheritsFrom 的 libraries
    if let Some(parent_id) = json.get("inheritsFrom").and_then(|v| v.as_str()).map(|s| s.to_string()) {
        if let Some(parent_json) = resolve_version_json_recursive(&parent_id, versions_dir) {
            if let (Some(parent_libs), Some(self_libs)) = (
                parent_json.get("libraries").and_then(|v| v.as_array()).cloned(),
                json.get("libraries").and_then(|v| v.as_array()).cloned(),
            ) {
                // 合并：父版本在前，子版本在后（去重）
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut merged: Vec<Value> = Vec::new();
                for lib in parent_libs.iter().chain(self_libs.iter()) {
                    if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
                        if seen.insert(name.to_string()) {
                            merged.push(lib.clone());
                        }
                    } else {
                        merged.push(lib.clone());
                    }
                }
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("libraries".to_string(), json!(merged));
                }
            }
            // 合并 downloads.client
            if json.get("downloads").and_then(|d| d.get("client")).is_none() {
                if let Some(parent_client) = parent_json.get("downloads").and_then(|d| d.get("client")) {
                    if let Some(obj) = json.as_object_mut() {
                        let mut downloads = obj.get("downloads").cloned().unwrap_or(json!({}));
                        if let Some(d_obj) = downloads.as_object_mut() {
                            d_obj.insert("client".to_string(), parent_client.clone());
                        }
                        obj.insert("downloads".to_string(), downloads);
                    }
                }
            }
        }
    }

    Some(json)
}

/// 从 Maven 坐标推导库文件路径
///
/// 例：`com.mojang:authlib:1.5.25` → `libraries/com/mojang/authlib/1.5.25/authlib-1.5.25.jar`
fn resolve_library_path_from_name(name: &str, libraries_dir: &Path) -> PathBuf {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return libraries_dir.join("unknown.jar");
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let filename = format!("{}-{}.jar", artifact, version);
    libraries_dir.join(&group).join(artifact).join(version).join(filename)
}

// ============== 导出启动脚本 / 导出整合包 / 读取版本图标 ==============

/// POST /api/version/export-script — 导出启动脚本
///
/// 参数：versionId（必填）、scriptType（bat/sh，默认按平台选）
/// 流程：构建启动参数 → 生成 .bat/.sh 内容 → 写入 DATA_DIR/temp/<versionId>.<ext>
/// 返回：{ success, content, fileName, path }
async fn handle_export_script(body: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;
    use crate::launch;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let is_external = version_id.contains(" [外部");
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
    } else {
        version_id.clone()
    };

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    // 1. 解析版本 JSON（递归合并 inheritsFrom）
    let version_json = match resolve_version_json_recursive(&clean_id, &versions_dir) {
        Some(j) => j,
        None => return ApiResult::err(404, "版本 JSON 不存在"),
    };

    // 2. 加载设置和账号
    let settings = storage::load_settings();
    let accounts = storage::load_accounts();
    let accounts_arr = accounts.as_array().cloned().unwrap_or_default();
    let selected_account_id = utils::get_str(&settings, "selectedAccount");
    let account = accounts_arr
        .iter()
        .find(|a| utils::get_str(a, "id") == selected_account_id)
        .or(accounts_arr.first())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "username": "Player",
                "uuid": "",
                "accessToken": "",
                "type": "offline"
            })
        });

    // 3. 构建启动参数
    let launch_args = launch::build_launch_arguments(
        &version_json,
        &settings,
        &account,
        &clean_id,
        None,
        None,
    );

    // 4. 选择 Java 路径
    let java_path = launch::dep_check::select_java_for_version(&clean_id, &settings, &version_json);
    let java_path = if java_path.is_empty() {
        "java".to_string()
    } else {
        java_path
    };

    // 5. 决定游戏目录
    let game_dir = get_version_game_dir(&clean_id, is_external);

    // 6. 决定脚本扩展名
    let script_type = utils::get_str(&data, "scriptType").to_lowercase();
    let is_windows = cfg!(target_os = "windows");
    let ext = if script_type == "bat" {
        "bat"
    } else if script_type == "sh" {
        "sh"
    } else if is_windows {
        "bat"
    } else {
        "sh"
    };

    // 7. 拼接参数字符串（含空格的参数加双引号）
    let args_str = launch_args
        .args
        .iter()
        .map(|a| {
            if a.contains(' ') || a.contains('\t') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // 8. 生成脚本内容
    let content = if ext == "bat" {
        let mut s = String::new();
        s.push_str("@echo off\r\n");
        s.push_str(&format!("cd /d \"{}\"\r\n", game_dir.display()));
        s.push_str(&format!("\"{}\" {}\r\n", java_path, args_str));
        s.push_str("pause\r\n");
        s
    } else {
        let mut s = String::new();
        s.push_str("#!/bin/bash\n");
        s.push_str(&format!("cd \"{}\"\n", game_dir.display()));
        s.push_str(&format!("\"{}\" {}\n", java_path, args_str));
        s.push_str("read -p \"按回车键退出...\"\n");
        s
    };

    // 9. 写入临时文件
    let temp_dir = data_dir.join("temp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_name = format!("{}.{}", clean_id, ext);
    let script_path = temp_dir.join(&file_name);

    match std::fs::write(&script_path, &content) {
        Ok(_) => ApiResult::ok(json!({
            "success": true,
            "content": content,
            "fileName": file_name,
            "path": script_path.to_string_lossy()
        })),
        Err(e) => ApiResult::err(500, &format!("写入脚本失败: {}", e)),
    }
}

/// POST /api/version/export-modpack — 导出整合包（简化版 ZIP）
///
/// 参数：versionId（必填）、format（仅 zip 实现，其他忽略）
/// 简化版：直接打包版本目录所有文件为 ZIP（排除版本 JSON 自身）
/// 写入 DATA_DIR/temp/<versionId>-export.zip
/// 返回：{ success, path, fileName }
fn handle_export_modpack(body: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let is_external = version_id.contains(" [外部");
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
    } else {
        version_id.clone()
    };

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let version_dir = versions_dir.join(&clean_id);

    if !version_dir.exists() {
        return ApiResult::err(404, "版本目录不存在");
    }

    let temp_dir = data_dir.join("temp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_name = format!("{}-export.zip", clean_id);
    let zip_path = temp_dir.join(&file_name);

    // 排除版本 JSON 自身
    let exclude_names: Vec<String> = vec![format!("{}.json", clean_id)];

    match create_zip_from_dir(&version_dir, &zip_path, &exclude_names) {
        Ok(_) => ApiResult::ok(json!({
            "success": true,
            "path": zip_path.to_string_lossy(),
            "fileName": file_name
        })),
        Err(e) => ApiResult::err(500, &format!("打包失败: {}", e)),
    }
}

/// 把一个目录递归打包成 ZIP（排除指定文件名的根级文件）
fn create_zip_from_dir(
    src_dir: &Path,
    zip_path: &Path,
    exclude_root_files: &[String],
) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::{Read, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;
    use zip::ZipWriter;

    let file = File::create(zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated);

    let mut stack: Vec<(PathBuf, String)> = vec![(src_dir.to_path_buf(), String::new())];

    while let Some((dir, prefix)) = stack.pop() {
        let entries = std::fs::read_dir(&dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };

            // 仅排除根级文件（prefix 为空时）
            if prefix.is_empty() && exclude_root_files.iter().any(|f| f == &name) {
                continue;
            }

            let zip_entry = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            if meta.is_dir() {
                zip.add_directory(&zip_entry, options)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                stack.push((path, zip_entry));
            } else if meta.is_file() {
                zip.start_file(&zip_entry, options)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                let mut f = File::open(&path)?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            }
        }
    }

    zip.finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(())
}

/// GET /api/version-icon — 读取版本图标文件（base64 data URL）
///
/// 参数：versionId（必填）、type（版本类型）、forge、fabric、neoforge、modpack（可选）
/// 查找版本目录下的 icon.png / icon.jpg / icon.jpeg / pack.png / logo.png
/// 找到则读为 base64 返回 { success, dataUrl }
/// 找不到自定义图标时，用内置方块图标兜底
fn handle_version_icon(params: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;

    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    // 读取前端传的可选参数（用于远程版本无法解析 JSON 时兜底）
    let version_type = params.as_ref()
        .and_then(|p| p.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("release");
    let is_forge = params.as_ref()
        .and_then(|p| p.get("forge"))
        .and_then(|v| v.as_str())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let is_fabric = params.as_ref()
        .and_then(|p| p.get("fabric"))
        .and_then(|v| v.as_str())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let is_neoforge = params.as_ref()
        .and_then(|p| p.get("neoforge"))
        .and_then(|v| v.as_str())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    let is_modpack = params.as_ref()
        .and_then(|p| p.get("modpack"))
        .and_then(|v| v.as_str())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);

    let is_external = version_id.contains(" [外部");
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(version_id).to_string()
    } else {
        version_id.to_string()
    };

    let data_dir = storage::resolve_data_dir();
    let version_dir = data_dir.join("versions").join(&clean_id);

    // 1. 找自定义图标
    let icon_candidates = ["icon.png", "icon.jpg", "icon.jpeg", "pack.png", "logo.png"];
    for fname in &icon_candidates {
        let icon_path = version_dir.join(fname);
        if icon_path.exists() {
            if let Ok(data) = std::fs::read(&icon_path) {
                let mime = if fname.ends_with(".jpg") || fname.ends_with(".jpeg") {
                    "image/jpeg"
                } else {
                    "image/png"
                };
                let data_url = utils::bytes_to_data_url(&data, mime);
                return ApiResult::ok(json!({
                    "success": true,
                    "dataUrl": data_url
                }));
            }
        }
    }

    // 2. 找不到自定义图标：根据版本 JSON 推断加载器类型
    let versions_dir = data_dir.join("versions");
    let (detected_type, detected_forge, detected_fabric, detected_neoforge) =
        if let Some(version_json) = resolve_version_json_recursive(&clean_id, &versions_dir) {
            let vt = utils::get_str(&version_json, "type");
            let inherits_from = utils::get_str(&version_json, "inheritsFrom");
            let (df, dg, dn, _, _, _, _) =
                detect_loader(&version_json, &clean_id, &inherits_from, None);
            (vt, dg, df, dn)
        } else {
            // 远程版本（未安装）：用前端传的参数
            (version_type.to_string(), is_forge, is_fabric, is_neoforge)
        };

    let icon_bytes = get_builtin_icon(&detected_type, detected_forge, detected_fabric, detected_neoforge, is_modpack);
    let data_url = utils::bytes_to_data_url(icon_bytes, "image/png");
    ApiResult::ok(json!({
        "success": true,
        "dataUrl": data_url
    }))
}
