#![allow(unused_imports)]

// launch/dep_check.rs — 依赖完整性检查
// 职责：检查指定版本的 Java / 版本JSON / 主JAR / 库 / natives / 资源 / 前置版本 / Forge核心 / mrpack mods

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::java;
use crate::storage;
use crate::utils;

/// 当前操作系统版本字符串（形如 "10.0.22621"），非 Windows 返回空串。
/// 从注册表读取 Windows NT 版本号，用于匹配版本 JSON 中 libraries 的 os.version 规则。
static OS_VERSION: LazyLock<String> = LazyLock::new(|| {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        {
            let major: Option<u32> = key.get_value("CurrentMajorVersionNumber").ok();
            let minor: Option<u32> = key.get_value("CurrentMinorVersionNumber").ok();
            let build: Option<String> = key.get_value("CurrentBuildNumber").ok();
            if let (Some(ma), Some(mi), Some(b)) = (major, minor, build) {
                return format!("{}.{}.{}", ma, mi, b);
            }
        }
        String::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::new()
    }
});

/// 判断系统版本是否匹配 rules 中 os.version 给定的正则模式。
/// 模式非法时视为匹配（不影响该库的判定）。非 Windows 或无版本信息返回 false。
fn os_version_matches(pattern: &str) -> bool {
    if OS_VERSION.is_empty() {
        return false;
    }
    match Regex::new(pattern) {
        Ok(re) => re.is_match(&OS_VERSION),
        Err(_) => true,
    }
}

// ============== 检查结果结构 ==============

#[derive(Clone, Debug)]
pub struct MissingFile {
    pub kind: String, // "java" | "main_jar" | "library" | "native" | "asset_index" | "asset" | "asset_batch" | "parent_version" | "forge_core" | "mod"
    pub url: String,
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub name: String,
    pub desc: String,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct DepCheckResult {
    pub java: CheckItem,
    pub version_json: CheckItem,
    pub main_jar: CheckItem,
    pub libraries: CheckList,
    pub natives: CheckList,
    pub assets: CheckList,
    pub parent_version: CheckItem,
    pub forge_core: CheckList,
    pub mrpack_mods: CheckList,
    pub ready: bool,
    pub missing_files: Vec<MissingFile>,
    // Java 附加字段（在 java 字段外暴露方便使用）
    pub java_path: String,
    pub java_version: String,
    pub java_required: u32,
    pub java_max_version: u32,
}

#[derive(Clone, Debug, Default)]
pub struct CheckItem {
    pub ok: bool,
    pub message: String,
    pub warning: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CheckList {
    pub ok: bool,
    pub total: u64,
    pub missing: Vec<MissingFile>,
    pub message: String,
}

impl DepCheckResult {
    fn new() -> Self {
        Self {
            java: CheckItem::default(),
            version_json: CheckItem::default(),
            main_jar: CheckItem::default(),
            libraries: CheckList {
                ok: true,
                ..Default::default()
            },
            natives: CheckList {
                ok: true,
                ..Default::default()
            },
            assets: CheckList {
                ok: true,
                ..Default::default()
            },
            parent_version: CheckItem {
                ok: true,
                ..Default::default()
            },
            forge_core: CheckList {
                ok: true,
                ..Default::default()
            },
            mrpack_mods: CheckList {
                ok: true,
                ..Default::default()
            },
            ready: false,
            missing_files: Vec::new(),
            java_path: String::new(),
            java_version: String::new(),
            java_required: 8,
            java_max_version: 999,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "java": {
                "ok": self.java.ok,
                "path": self.java_path,
                "version": self.java_version,
                "required": self.java_required,
                "maxVersion": self.java_max_version,
                "rangeSource": "default",
                "message": self.java.message,
                "warning": self.java.warning
            },
            "versionJson": {
                "ok": self.version_json.ok,
                "message": self.version_json.message
            },
            "mainJar": {
                "ok": self.main_jar.ok,
                "message": self.main_jar.message
            },
            "libraries": {
                "ok": self.libraries.ok,
                "total": self.libraries.total,
                "missing": self.libraries.missing.iter().map(missing_to_json).collect::<Vec<_>>(),
                "message": self.libraries.message
            },
            "natives": {
                "ok": self.natives.ok,
                "total": self.natives.total,
                "missing": self.natives.missing.iter().map(missing_to_json).collect::<Vec<_>>(),
                "message": self.natives.message
            },
            "assets": {
                "ok": self.assets.ok,
                "total": self.assets.total,
                "missing": self.assets.missing.iter().map(missing_to_json).collect::<Vec<_>>(),
                "message": self.assets.message
            },
            "parentVersion": {
                "ok": self.parent_version.ok,
                "message": self.parent_version.message
            },
            "forgeCore": {
                "ok": self.forge_core.ok,
                "missing": self.forge_core.missing.iter().map(missing_to_json).collect::<Vec<_>>(),
                "message": self.forge_core.message
            },
            "mrpackMods": {
                "ok": self.mrpack_mods.ok,
                "total": self.mrpack_mods.total,
                "missing": self.mrpack_mods.missing.iter().map(missing_to_json).collect::<Vec<_>>(),
                "message": self.mrpack_mods.message
            },
            "ready": self.ready,
            "missingFiles": self.missing_files.iter().map(missing_to_json).collect::<Vec<_>>()
        })
    }
}

fn missing_to_json(m: &MissingFile) -> Value {
    json!({
        "type": m.kind,
        "url": m.url,
        "path": m.path,
        "sha1": m.sha1,
        "size": m.size,
        "name": m.name,
        "desc": m.desc,
        "message": m.message
    })
}

// ============== 主入口 ==============

/// 检查指定版本的依赖完整性
pub fn check_dependencies(version_id: &str, settings: &Value, external_version_dir: Option<&Path>) -> DepCheckResult {
    let mut result = DepCheckResult::new();

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let libraries_dir = data_dir.join("libraries");
    let assets_dir = data_dir.join("assets");

    // 外部版本目录的资源根
    let external_assets_dir = external_version_dir.and_then(|d| {
        let root = find_external_root(d);
        root.map(|r| r.join("assets")).filter(|p| p.exists())
    });

    // ----- 1. 版本 JSON 解析（合并继承链） -----
    let version_json_path = match resolve_version_json(version_id, external_version_dir) {
        Some(p) => p,
        None => {
            result.version_json.ok = false;
            result.version_json.message = format!("版本 {} 的JSON文件缺失或损坏", version_id);
            return result;
        }
    };
    result.version_json.ok = true;

    // 读取当前版本原始 JSON，用于错误提示和路径计算
    let raw_version_json: Value = match std::fs::read_to_string(&version_json_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => {
                result.version_json.ok = false;
                result.version_json.message = format!("版本 {} 的JSON文件损坏", version_id);
                return result;
            }
        },
        Err(_) => {
            result.version_json.ok = false;
            result.version_json.message = format!("版本 {} 的JSON文件读取失败", version_id);
            return result;
        }
    };

    // 合并继承链，让 libraries / arguments / javaVersion 等字段完整
    let version_json = merge_version_json_chain(version_id, external_version_dir)
        .unwrap_or_else(|| raw_version_json.clone());

    // ----- 2. Java 版本检查 -----
    // 外部版本在列表中显示为 "<id> [外部]"，解析 Java 版本范围时需去掉后缀
    let clean_version_id = if version_id.contains(" [外部") {
        version_id.split(" [外部").next().unwrap_or(version_id)
    } else {
        version_id
    };
    let (req_min, req_max) = get_java_version_range_from_json(clean_version_id, &version_json);
    result.java_required = req_min;
    result.java_max_version = req_max;

    let java_path = select_java_for_version(version_id, settings, &version_json);
    if java_path.is_empty() {
        result.java.ok = false;
        let range_desc = if req_max < 999 {
            format!("{}~{}", req_min, req_max)
        } else {
            format!("{}+", req_min)
        };
        let java_list = java::detect_all();
        if !java_list.is_empty() {
            let detected_list = java_list
                .iter()
                .filter_map(|j| {
                    let mv = j.get("majorVersion").and_then(|v| v.as_u64())?;
                    let p = utils::get_str(j, "path");
                    Some(format!("Java {} ({})", mv, p))
                })
                .collect::<Vec<_>>()
                .join(", ");
            result.java.message = format!(
                "未找到合适版本的Java（需要 {}，检测到 {} 个但版本不匹配: {}），请前往 Java 管理页面安装或配置",
                range_desc,
                java_list.len(),
                detected_list
            );
        } else {
            result.java.message = format!(
                "未找到Java运行环境（需要 {}），请前往 Java 管理页面安装或配置",
                range_desc
            );
        }
    } else {
        result.java_path = java_path.clone();
        // 执行 -version 验证
        match inspect_java_version(&java_path) {
            Some((version_str, major)) => {
                result.java_version = version_str.clone();
                if major >= req_min && major <= req_max {
                    result.java.ok = true;
                    result.java.message = if req_max < 999 {
                        format!("Java {} (满足要求 {}~{})", version_str, req_min, req_max)
                    } else {
                        format!("Java {} (满足要求 {}+)", version_str, req_min)
                    };
                } else {
                    result.java.ok = false;
                    result.java.warning = true;
                    let range_desc = if req_max < 999 {
                        format!("{}~{}", req_min, req_max)
                    } else {
                        format!("{}+", req_min)
                    };
                    result.java.message = format!(
                        "Java {} 不满足要求(需要 {})，请在版本设置中更换Java或使用文件修复功能自动安装",
                        version_str, range_desc
                    );
                }
            }
            None => {
                result.java.ok = false;
                result.java.message = "无法检测Java版本".to_string();
            }
        }
    }

    // ----- 3. 前置版本检查（inheritsFrom） -----
    // 注意：必须使用 raw_version_json 获取 inheritsFrom，因为 merge_version_json_chain 会移除该字段
    let inherits_from = utils::get_str(&raw_version_json, "inheritsFrom");
    if !inherits_from.is_empty() {
        let parent_json_found = find_version_json_path(&inherits_from, external_version_dir).is_some();
        let main_jar_path = find_main_jar(&version_json, version_id, external_version_dir);
        let main_jar_found = main_jar_path
            .as_ref()
            .map(|p| p.exists())
            .unwrap_or(false);

        if !parent_json_found && !main_jar_found {
            // 外部版本豁免：自包含的 JSON（有 mainClass 或 Forge/Fabric 库）即使没有前置版本也允许启动
            let has_main_class = !utils::get_str(&version_json, "mainClass").is_empty();
            let has_forge_libs = version_json
                .get("libraries")
                .and_then(|v| v.as_array())
                .map(|libs| {
                    libs.iter().any(|l| {
                        let name = utils::get_str(l, "name");
                        name.contains("net.minecraftforge")
                            || name.contains("fancymodloader")
                            || name.contains("net.neoforged")
                            || name.contains("fabric-loader")
                    })
                })
                .unwrap_or(false);
            let is_self_sufficient = external_version_dir.is_some() && (has_main_class || has_forge_libs);
            if !is_self_sufficient {
                result.parent_version.ok = false;
                result.parent_version.message = format!("缺少基础版本 {}，请先安装", inherits_from);
                result.missing_files.push(MissingFile {
                    kind: "parent_version".to_string(),
                    url: String::new(),
                    path: String::new(),
                    sha1: String::new(),
                    size: 0,
                    name: inherits_from.clone(),
                    desc: String::new(),
                    message: format!("缺少基础版本 {}", inherits_from),
                });
            } else {
                result.parent_version.ok = true;
            }
        } else {
            // 前置版本 JSON 或主 JAR 至少有一个存在，视为通过
            result.parent_version.ok = true;
        }
    }

    // ----- 4. 主 JAR 检查 -----
    // 使用 raw_version_json 保留 inheritsFrom 字段，确保 find_main_jar 能沿继承链找到父版本 JAR
    let main_jar_path = find_main_jar(&raw_version_json, version_id, external_version_dir);
    let is_modded = utils::get_str(&version_json, "forge").contains("forge")
        || utils::get_str(&version_json, "neoforge").contains("neoforge")
        || utils::get_str(&version_json, "fabricVersion").contains("fabric")
        || !inherits_from.is_empty();

    if let Some(jar_path) = &main_jar_path {
        if jar_path.exists() {
            let client_sha1 = version_json
                .get("downloads")
                .and_then(|v| v.get("client"))
                .and_then(|v| v.get("sha1"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !client_sha1.is_empty() && !is_modded {
                match utils::calculate_sha1(jar_path) {
                    Some(actual) if actual == client_sha1 => {
                        result.main_jar.ok = true;
                    }
                    Some(_) => {
                        result.main_jar.ok = false;
                        result.main_jar.message = "主JAR文件SHA1校验失败".to_string();
                        let client = version_json.get("downloads").and_then(|v| v.get("client")).cloned().unwrap_or(Value::Null);
                        result.missing_files.push(MissingFile {
                            kind: "main_jar".to_string(),
                            url: utils::get_str(&client, "url"),
                            path: jar_path.to_string_lossy().to_string(),
                            sha1: client_sha1.to_string(),
                            size: client.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                            name: format!("{}.jar", version_id),
                            desc: String::new(),
                            message: String::new(),
                        });
                    }
                    None => {
                        // SHA1 计算失败，但文件存在视为有效
                        result.main_jar.ok = true;
                    }
                }
            } else {
                result.main_jar.ok = true;
            }
        } else {
            // 路径存在但文件不存在
            push_missing_main_jar(&mut result, &version_json, version_id, jar_path);
        }
    } else {
        // 沿 inheritsFrom 链查找 fallback URL（使用 raw_version_json 保留 inheritsFrom）
        push_missing_main_jar_with_fallback(&mut result, &raw_version_json, version_id, &versions_dir);
    }

    // ----- 5. 库与 natives 检查 -----
    let libraries = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let current_platform = current_platform_name();
    let mut lib_total: u64 = 0;

    for lib in &libraries {
        // rules 筛选
        if let Some(rules) = lib.get("rules").and_then(|v| v.as_array())
            && !evaluate_rules(rules, false, false) {
                continue;
            }

        let name = utils::get_str(lib, "name");

        // NeoForge 虚拟 client 库跳过（patched jar 已存在则视为有效）
        if name.starts_with("net.neoforged:neoforge:") && name.ends_with(":client") {
            let neo_ver = name.split(':').nth(2).unwrap_or("");
            let patched_jar = libraries_dir
                .join("net")
                .join("neoforged")
                .join("minecraft-client-patched")
                .join(neo_ver)
                .join(format!("minecraft-client-patched-{}.jar", neo_ver));
            if patched_jar.exists() {
                lib_total += 1;
                continue;
            }
        }

        // 新格式 native（classifier 以 natives- 开头）
        let name_suffix = name.split(':').next_back().unwrap_or("");
        let has_natives_field = lib
            .get("natives")
            .and_then(|v| v.as_object())
            .map(|m| m.contains_key(&current_platform))
            .unwrap_or(false);
        let is_new_format_native = !has_natives_field && name_suffix.starts_with("natives-");

        if is_new_format_native {
            let native_check = check_native_lib(lib, &libraries_dir, external_version_dir, &current_platform);
            if let Some(missing) = native_check {
                lib_total += 1;
                result.natives.missing.push(missing);
            } else if name_suffix.starts_with("natives-") {
                lib_total += 1;
            }
        } else if lib.get("downloads").and_then(|v| v.get("artifact")).is_some() {
            // 标准 library（有 downloads.artifact）
            lib_total += 1;
            check_standard_library(lib, &libraries_dir, external_version_dir, &mut result);
        } else if !name.is_empty() && !has_natives_field {
            // 无 downloads.artifact：按 maven 坐标定位
            if let Some(missing) = check_maven_library(lib, &libraries_dir, external_version_dir) {
                lib_total += 1;
                result.libraries.missing.push(missing);
            } else {
                lib_total += 1;
            }
        }

        // 旧格式 natives（lib.natives 字典）
        if has_natives_field
            && let Some(missing) = check_legacy_native(lib, &libraries_dir, external_version_dir, &current_platform) {
                lib_total += 1;
                result.natives.missing.push(missing);
            }
    }

    result.libraries.total = lib_total;
    result.libraries.ok = result.libraries.missing.is_empty();
    if !result.libraries.missing.is_empty() {
        result.libraries.message = format!("{} 个库文件缺失或损坏", result.libraries.missing.len());
        result.missing_files.extend(result.libraries.missing.clone());
    }

    result.natives.total = result.natives.missing.len() as u64;
    result.natives.ok = result.natives.missing.is_empty();
    if !result.natives.missing.is_empty() {
        result.natives.message = format!("{} 个原生库缺失或损坏", result.natives.missing.len());
        result.missing_files.extend(result.natives.missing.clone());
    }

    // ----- 6. Forge / NeoForge 核心库检查 -----
    check_forge_core(&version_json, version_id, external_version_dir, &libraries_dir, &mut result);

    // ----- 7. mrpack 整合包 mods 完整性 -----
    check_mrpack_mods(version_id, external_version_dir, &mut result);

    // ----- 8. 资源文件检查 -----
    if let Some(asset_index) = version_json.get("assetIndex") {
        let asset_index_id = utils::get_str(asset_index, "id");
        let asset_index_url = utils::get_str(asset_index, "url");
        let asset_index_sha1 = utils::get_str(asset_index, "sha1");
        let asset_index_size = asset_index.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        let local_index_path = assets_dir.join("indexes").join(format!("{}.json", asset_index_id));
        let mut index_path = local_index_path.clone();

        if !index_path.exists()
            && let Some(ext_assets) = &external_assets_dir {
                let ext_index = ext_assets.join("indexes").join(format!("{}.json", asset_index_id));
                if ext_index.exists() {
                    index_path = ext_index;
                }
            }

        if !index_path.exists() {
            result.assets.ok = false;
            result.assets.message = "资源索引文件缺失".to_string();
            result.missing_files.push(MissingFile {
                kind: "asset_index".to_string(),
                url: asset_index_url,
                path: index_path.to_string_lossy().to_string(),
                sha1: asset_index_sha1,
                size: asset_index_size,
                name: format!("{}.json", asset_index_id),
                desc: String::new(),
                message: String::new(),
            });
        } else {
            match std::fs::read_to_string(&index_path) {
                Ok(content) => {
                    if let Ok(index_data) = serde_json::from_str::<Value>(&content) {
                        if let Some(objects) = index_data.get("objects").and_then(|v| v.as_object()) {
                            result.assets.total = objects.len() as u64;
                            let mut missing_count = 0u64;

                            for (name, info) in objects {
                                let hash = utils::get_str(info, "hash");
                                if hash.is_empty() {
                                    continue;
                                }
                                let expected_size = info.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                                let sub_dir = &hash[..2.min(hash.len())];
                                let asset_path = assets_dir.join("objects").join(sub_dir).join(&hash);

                                let mut found = asset_exists_ok(&asset_path, expected_size);
                                if !found
                                    && let Some(ext_assets) = &external_assets_dir {
                                        let ext_asset = ext_assets.join("objects").join(sub_dir).join(&hash);
                                        if asset_exists_ok(&ext_asset, expected_size) {
                                            found = true;
                                        }
                                    }

                                if !found {
                                    missing_count += 1;
                                    let size = info.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                                    result.assets.missing.push(MissingFile {
                                        kind: "asset".to_string(),
                                        url: format!("https://resources.download.minecraft.net/{}/{}", sub_dir, hash),
                                        path: asset_path.to_string_lossy().to_string(),
                                        sha1: hash.clone(),
                                        size,
                                        name: name.clone(),
                                        desc: String::new(),
                                        message: String::new(),
                                    });
                                }
                            }

                            result.assets.ok = missing_count == 0;
                            if missing_count > 0 {
                                result.assets.message = format!("{} 个资源文件缺失", missing_count);
                                result.missing_files.extend(result.assets.missing.clone());
                            }
                        }
                    } else {
                        result.assets.ok = false;
                        result.assets.message = "无法解析资源索引文件".to_string();
                    }
                }
                Err(_) => {
                    result.assets.ok = false;
                    result.assets.message = "无法读取资源索引文件".to_string();
                }
            }
        }
    }

    // ----- 9. 汇总 ready 标志 -----
    result.ready = result.java.ok
        && result.version_json.ok
        && result.main_jar.ok
        && result.libraries.ok
        && result.natives.ok
        && result.parent_version.ok
        && result.assets.ok
        && result.forge_core.ok
        && result.mrpack_mods.ok;

    result
}

// ============== 工具函数 ==============

















// ============== 库检查子函数 ==============







// ============== 主 JAR 缺失处理 ==============



// ============== Forge 核心库检查 ==============





// ============== mrpack mods 完整性检查 ==============


#[cfg(test)]
mod tests {
    use super::*;

    /// 1.20.4-Fabric 应要求 Java 17（不是 25），验证版本范围计算不误判
    #[test]
    fn java_range_for_1_20_4_fabric_is_17() {
        // 模拟外部版本的完整 JSON（含 javaVersion=17）
        let version_json = json!({
            "id": "1.20.4-Fabric 0.19.3",
            "javaVersion": { "majorVersion": 17 },
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [
                { "name": "net.fabricmc:fabric-loader:0.19.3" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("1.20.4-Fabric 0.19.3", &version_json);
        assert_eq!(min, 17, "1.20.4 应要求 Java 17，实际 min={}", min);
        assert_eq!(max, 999, "1.20.4 Fabric max 应为 999，实际 max={}", max);
    }

    /// 1.20.1 应要求 Java 17
    #[test]
    fn java_range_for_1_20_1_is_17() {
        let version_json = json!({
            "id": "1.20.1",
            "javaVersion": { "majorVersion": 17 }
        });
        let (min, max) = get_java_version_range_from_json("1.20.1", &version_json);
        assert_eq!(min, 17, "1.20.1 应要求 Java 17，实际 min={}", min);
        assert_eq!(max, 999);
    }

    /// 1.21.1 应要求 Java 21
    #[test]
    fn java_range_for_1_21_1_is_21() {
        let version_json = json!({
            "id": "1.21.1",
            "javaVersion": { "majorVersion": 21 }
        });
        let (min, _) = get_java_version_range_from_json("1.21.1", &version_json);
        assert_eq!(min, 21, "1.21.1 应要求 Java 21，实际 min={}", min);
    }

    /// 26.2 应要求 Java 25
    #[test]
    fn java_range_for_26_2_is_25() {
        let version_json = json!({
            "id": "26.2",
            "javaVersion": { "majorVersion": 25 }
        });
        let (min, _) = get_java_version_range_from_json("26.2", &version_json);
        assert_eq!(min, 25, "26.2 应要求 Java 25，实际 min={}", min);
    }

    /// 重命名整合包（香草纪元-2.7.1）携带 Forge 1.20.1-47.3.33：
    /// 不能把整合包版本 2.7.1 当成 MC 2.7.1，也不能因为 id 里没有 forge 就允许 Java 25
    #[test]
    fn java_range_for_renamed_forge_modpack_is_17_to_21() {
        let version_json = json!({
            "id": "香草纪元-2.7.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "libraries": [
                { "name": "net.minecraftforge:forge:1.20.1-47.3.33" },
                { "name": "net.minecraftforge:fmlloader:1.20.1-47.3.33" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("香草纪元-2.7.1", &version_json);
        assert_eq!(min, 17, "Forge 1.20.1 下限应为 Java 17，实际 min={}", min);
        assert_eq!(max, 21, "Forge 1.20.1 上限应为 Java 21，实际 max={}", max);
    }

    /// 1.16.5 整合包（1.16.5-jojo）Forge 36.2.42：上限 Java 23，避免选中 Java 25
    #[test]
    fn java_range_for_forge_1_16_5_36_2_42_is_8_to_23() {
        let version_json = json!({
            "id": "1.16.5-jojo",
            "libraries": [
                { "name": "net.minecraftforge:forge:1.16.5-36.2.42" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("1.16.5-jojo", &version_json);
        assert_eq!(min, 8);
        assert_eq!(max, 23, "Forge 36.2.42 上限应为 Java 23，实际 max={}", max);
    }

    /// Forge 36.2.25 及更早的 1.16.5 版本最高 Java 8
    #[test]
    fn java_range_for_forge_36_2_25_is_java_8() {
        let version_json = json!({
            "id": "1.16.5-forge-36.2.25",
            "libraries": [
                { "name": "net.minecraftforge:forge:1.16.5-36.2.25" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("1.16.5-forge-36.2.25", &version_json);
        assert_eq!(min, 8);
        assert_eq!(max, 8, "Forge 36.2.25 最高应为 Java 8，实际 max={}", max);
    }

    /// NeoForge 1.20.1 最高 Java 21
    #[test]
    fn java_range_for_neoforge_1_20_1_is_max_21() {
        let version_json = json!({
            "id": "1.20.1-neoforge-47.1.106",
            "libraries": [
                { "name": "net.neoforged:neoforge:47.1.106" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("1.20.1-neoforge-47.1.106", &version_json);
        assert_eq!(min, 17);
        assert_eq!(max, 21, "NeoForge 1.20.1 上限应为 Java 21，实际 max={}", max);
    }

    /// Fabric Loader 0.16.x 最高 Java 24
    #[test]
    fn java_range_for_old_fabric_loader_is_max_24() {
        let version_json = json!({
            "id": "1.20.1-fabric-0.16.14",
            "libraries": [
                { "name": "net.fabricmc:fabric-loader:0.16.14" }
            ]
        });
        let (min, max) = get_java_version_range_from_json("1.20.1-fabric-0.16.14", &version_json);
        assert_eq!(min, 17);
        assert_eq!(max, 24, "Fabric 0.16.x 上限应为 Java 24，实际 max={}", max);
    }

    /// 没有加载器坐标时，不应把整合包版本 2.7.1 当成 MC 2.7.1
    #[test]
    fn extract_mc_version_rejects_modpack_version() {
        assert_eq!(extract_mc_version("香草纪元-2.7.1"), "");
        assert_eq!(extract_mc_version("1.20.1-Forge-47.3.33"), "1.20.1");
        assert_eq!(extract_mc_version("26.2"), "26.2");
    }
}


// 子模块声明与公共导出
mod version_json;
pub(crate) use version_json::*;
mod native;
pub(crate) use native::*;
mod library;
pub(crate) use library::*;
mod forge;
pub(crate) use forge::*;
