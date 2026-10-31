// versions/scan.rs — 本地版本扫描与加载器识别
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::storage;
use crate::utils;


/// 扫描本地已安装版本（versions/ 目录）
pub(crate) fn scan_local_versions() -> Vec<Value> {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    scan_versions_in_dir(&versions_dir, false, None)
}

/// 扫描外部文件夹下的版本
pub(crate) fn scan_external_versions() -> Vec<Value> {
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
///
/// 规则：
/// - 被整合包继承且自身 mods 为空的加载器版本（Forge/Fabric/NeoForge/OptiFine/LiteLoader）隐藏
/// - 被继承的原版基础版本始终显示（用户可直接启动原版）
/// - 不被继承的版本始终显示
pub(crate) fn filter_loader_visibility(versions: Vec<Value>) -> Vec<Value> {
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
pub(crate) fn count_mods_jar(mods_dir: &Path) -> usize {
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
pub(crate) fn inherit_loader_from_parent(versions: &mut [Value]) {
    let snapshot = versions.to_vec();
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
        let Some(parent) = parent else {
            // 父版本目录缺失（如独立加载器版本被清理后）时，无法从父版本对象继承加载器类型，
            // 改为按 inheritsFrom 的 ID 文本来推断加载器类型，避免兼容子（整合包）被误判为原版/铁砧图标。
            let inherits_lower = inherits.to_lowercase();
            if let Value::Object(m) = v {
                if inherits_lower.contains("fabric") {
                    m.insert("isFabric".to_string(), Value::Bool(true));
                }
                if inherits_lower.contains("neoforge") {
                    m.insert("isNeoForge".to_string(), Value::Bool(true));
                } else if inherits_lower.contains("forge") {
                    m.insert("isForge".to_string(), Value::Bool(true));
                }
                if inherits_lower.contains("optifine") {
                    m.insert("isOptiFine".to_string(), Value::Bool(true));
                }
                if inherits_lower.contains("liteloader") {
                    m.insert("isLiteLoader".to_string(), Value::Bool(true));
                }
            }
            continue;
        };
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
pub(crate) fn scan_versions_in_dir(versions_dir: &Path, is_external: bool, external_folder_root: Option<&Path>) -> Vec<Value> {
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

        // 基岩版：目录里有 bedrock.json 安装标记，不走 Java 版本 JSON 解析
        let bedrock_marker = path.join("bedrock.json");
        if bedrock_marker.exists() {
            let marker_text = std::fs::read_to_string(&bedrock_marker).unwrap_or_default();
            let marker: Value = serde_json::from_str(&marker_text).unwrap_or(Value::Null);
            let version = utils::get_str(&marker, "version");
            let id = if version.is_empty() { name.clone() } else { version };
            let installed_at = marker.get("installedAt").and_then(|v| v.as_u64()).unwrap_or(0);
            versions.push(json!({
                "id": id.clone(),
                "type": "bedrock",
                "releaseTime": utils::ts_to_iso(installed_at / 1000),
                "mainClass": "",
                "installed": true,
                "inheritsFrom": Value::Null,
                "isFabric": false,
                "isForge": false,
                "isNeoForge": false,
                "isOptiFine": false,
                "isLiteLoader": false,
                "isModpack": false,
                "modpackLoader": "",
                "baseVersion": id,
                "isAprilFools": false,
                "isBedrock": true,
                "isExternal": false,
                "externalVersionDir": Value::Null,
                "externalPath": Value::Null,
                "isolation": true,
                "hasMods": false,
                "hasSaves": false,
                "hasResourcepacks": false,
                "error": false,
                "errorReason": "",
                "customName": "",
                "description": "",
                "favorite": false,
                "jsonPath": bedrock_marker.to_string_lossy()
            }));
            continue;
        }

        // 找版本 JSON
        let version_json = find_version_json(&path, &name);
        let Some((json_content, json_path)) = version_json else {
            continue;
        };
        let parsed: Value = match serde_json::from_str(&json_content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 提取版本信息
        let id = utils::get_str(&parsed, "id");
        let id = if id.is_empty() { name.clone() } else { id };
        let main_class = utils::get_str(&parsed, "mainClass");
        let mut release_time = utils::get_str(&parsed, "releaseTime");
        // 兜底：releaseTime 缺失或为 1970 占位（旧版 now_iso 的 bug 产物）时，
        // 用版本 JSON 的修改时间作为发布时间，避免界面显示 1970
        if utils::is_invalid_release_time(&release_time)
            && let Ok(meta) = std::fs::metadata(&json_path)
                && let Ok(modified) = meta.modified()
                    && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                        release_time = utils::ts_to_iso(dur.as_secs());
                    }
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
pub(crate) fn find_version_json(version_dir: &Path, name: &str) -> Option<(String, PathBuf)> {
    // 1. <name>.json
    let prefer_path = version_dir.join(format!("{}.json", name));
    if prefer_path.exists()
        && let Ok(content) = std::fs::read_to_string(&prefer_path) {
            return Some((content, prefer_path));
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
pub(crate) fn detect_loader(parsed: &Value, id: &str, inherits_from: &str, version_dir: Option<&std::path::Path>) -> (bool, bool, bool, bool, bool, bool, String) {
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
    if let Some(dir) = version_dir
        && (dir.join("pack-info.json").exists() || dir.join("pack.png").exists()) {
            is_modpack = true;
        }
    // 目录标记文件缺失时，按版本 ID + 继承关系判断：
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

    if is_neoforge {
        is_fabric = false;
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
pub(crate) fn extract_base_version(id: &str) -> String {
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
pub(crate) fn mark_installed(remote_versions: &mut [Value], local_ids: &[String]) {
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
pub(crate) fn is_april_fools_id(id: &str) -> bool {
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
pub(crate) fn mark_april_fools(remote_versions: &mut [Value]) {
    for v in remote_versions.iter_mut() {
        let id = utils::get_str(v, "id");
        if is_april_fools_id(&id)
            && let Some(obj) = v.as_object_mut() {
                obj.insert("type".to_string(), json!("special"));
            }
    }
}