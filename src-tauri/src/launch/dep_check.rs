// launch/dep_check.rs — 依赖完整性检查
// 职责：检查指定版本的 Java / 版本JSON / 主JAR / 库 / natives / 资源 / 前置版本 / Forge核心 / mrpack mods
// 对应原项目 server/dependencies/check.js + server/dependencies/forge.js

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::java;
use crate::storage;
use crate::utils;

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
/// 对应原项目 server/dependencies/check.js:checkDependencies
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
        if let Some(rules) = lib.get("rules").and_then(|v| v.as_array()) {
            if !evaluate_rules(rules, false) {
                continue;
            }
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
        let name_suffix = name.split(':').last().unwrap_or("");
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
        if has_natives_field {
            if let Some(missing) = check_legacy_native(lib, &libraries_dir, external_version_dir, &current_platform) {
                lib_total += 1;
                result.natives.missing.push(missing);
            }
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

        if !index_path.exists() {
            if let Some(ext_assets) = &external_assets_dir {
                let ext_index = ext_assets.join("indexes").join(format!("{}.json", asset_index_id));
                if ext_index.exists() {
                    index_path = ext_index;
                }
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
                                let sub_dir = &hash[..2.min(hash.len())];
                                let asset_path = assets_dir.join("objects").join(sub_dir).join(&hash);

                                let mut found = asset_path.exists();
                                if !found {
                                    if let Some(ext_assets) = &external_assets_dir {
                                        let ext_asset = ext_assets.join("objects").join(sub_dir).join(&hash);
                                        if ext_asset.exists() {
                                            found = true;
                                        }
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

/// 获取当前平台名（windows/osx/linux）
pub(crate) fn current_platform_name() -> String {
    if cfg!(target_os = "windows") {
        "windows".to_string()
    } else if cfg!(target_os = "macos") {
        "osx".to_string()
    } else {
        "linux".to_string()
    }
}

/// 沿目录树向上查找外部根（同时有 versions/ 和 libraries/ 或 assets/ 的目录）
pub(crate) fn find_external_root(version_dir: &Path) -> Option<PathBuf> {
    let mut dir = version_dir.to_path_buf();
    for _ in 0..8 {
        if dir.join("versions").exists() && dir.join("libraries").exists() {
            return Some(dir);
        }
        if dir.join("versions").exists() && dir.join("assets").exists() {
            return Some(dir);
        }
        if dir.join("versions").exists() {
            return Some(dir);
        }
        if let Some(parent) = dir.parent() {
            if parent == dir {
                break;
            }
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

/// 沿外部根目录或本地 versions/ 查找版本 JSON
pub(crate) fn resolve_version_json(version_id: &str, external_version_dir: Option<&Path>) -> Option<PathBuf> {
    // 1. 优先：外部版本目录
    if let Some(ext_dir) = external_version_dir {
        let p = ext_dir.join(format!("{}.json", version_id));
        if p.exists() {
            return Some(p);
        }
        // 外部根的 versions/<id>/<id>.json
        if let Some(root) = find_external_root(ext_dir) {
            let p = root.join("versions").join(version_id).join(format!("{}.json", version_id));
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 2. 本地 versions/<id>/<id>.json
    let data_dir = storage::resolve_data_dir();
    let local_path = data_dir.join("versions").join(version_id).join(format!("{}.json", version_id));
    if local_path.exists() {
        return Some(local_path);
    }
    // 3. 兜底扫描：本地或外部目录中找任意 .json 含 mainClass/libraries/inheritsFrom
    let scan_dir = external_version_dir
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| data_dir.join("versions").join(version_id));
    if let Ok(entries) = std::fs::read_dir(&scan_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&p) {
                if content.contains("mainClass")
                    || content.contains("libraries")
                    || content.contains("inheritsFrom")
                    || content.contains("minecraftArguments")
                    || content.contains("arguments")
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 仅查找版本 JSON 路径（不读取内容），用于前置版本存在性判断
fn find_version_json_path(version_id: &str, external_version_dir: Option<&Path>) -> Option<PathBuf> {
    resolve_version_json(version_id, external_version_dir)
}

/// 递归读取并合并版本 JSON 的继承链
/// 父版本提供 libraries / arguments / natives 等，当前版本覆盖同名字段
pub(crate) fn merge_version_json_chain(
    version_id: &str,
    external_version_dir: Option<&Path>,
) -> Option<Value> {
    let path = resolve_version_json(version_id, external_version_dir)?;
    let content = std::fs::read_to_string(&path).ok()?;
    let mut current: Value = serde_json::from_str(&content).ok()?;

    // 先把 id 设对（有些 inheritsFrom 子 JSON 没 id）
    if current.get("id").is_none() {
        if let Value::Object(ref mut m) = current {
            m.insert("id".to_string(), Value::String(version_id.to_string()));
        }
    }

    let inherits_from = utils::get_str(&current, "inheritsFrom");
    if inherits_from.is_empty() {
        return Some(current);
    }

    // 递归合并父版本
    let parent = merge_version_json_chain(&inherits_from, external_version_dir)?;
    Some(merge_two_version_jsons(parent, current))
}

/// 合并两个版本 JSON：base 是父版本，child 是当前版本
/// 对齐 PCL2 (ModMinecraft.vb JsonObject 合并段)：
/// 深度合并全部字段（对象递归、数组按 Newtonsoft MergeArrayHandling.Concat 拼接、
/// 其余 scalar 由子版本覆盖），libraries 单独处理为"子版本在前、父版本在后"，
/// 继承标记在合并链中已消费后移除。
fn merge_two_version_jsons(base: Value, child: Value) -> Value {
    let child_libs = child.get("libraries").cloned();
    let parent_libs = base.get("libraries").cloned();

    let mut result = deep_merge_json(&base, &child);

    if child_libs.is_some() || parent_libs.is_some() {
        let mut libs = child_libs.and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if let Some(pl) = parent_libs.and_then(|v| v.as_array().cloned()) {
            libs.extend(pl);
        }
        if let Value::Object(ref mut m) = result {
            m.insert("libraries".to_string(), Value::Array(libs));
        }
    }

    if let Value::Object(ref mut m) = result {
        m.remove("inheritsFrom");
    }

    result
}

/// 递归深度合并两个 JSON：两值都是对象则按 key 递归合并；
/// 两值都是数组则拼接（父在前面）；否则子版本值覆盖父版本。
fn deep_merge_json(parent: &Value, child: &Value) -> Value {
    match (parent, child) {
        (Value::Object(p), Value::Object(c)) => {
            let mut out = p.clone();
            for (k, cv) in c {
                match out.get_mut(k) {
                    Some(pv) if pv.is_object() && cv.is_object() => {
                        *pv = deep_merge_json(pv, cv);
                    }
                    Some(pv) if pv.is_array() && cv.is_array() => {
                        let mut arr = pv.as_array().cloned().unwrap_or_default();
                        arr.extend(cv.as_array().cloned().unwrap_or_default());
                        *pv = Value::Array(arr);
                    }
                    _ => {
                        out.insert(k.clone(), cv.clone());
                    }
                }
            }
            Value::Object(out)
        }
        _ => child.clone(),
    }
}

/// 沿多路径搜索主 JAR
/// 对应原项目 server/versions/version-parse.js:findMainJar
pub(crate) fn find_main_jar(version_json: &Value, version_id: &str, external_version_dir: Option<&Path>) -> Option<PathBuf> {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    let actual_id = if !version_id.is_empty() {
        version_id.to_string()
    } else {
        utils::get_str(version_json, "id")
    };
    let jar_field = utils::get_str(version_json, "jar");
    let inherits_from = utils::get_str(version_json, "inheritsFrom");
    let jar_name = if !jar_field.is_empty() {
        jar_field.clone()
    } else if !inherits_from.is_empty() {
        inherits_from.clone()
    } else {
        actual_id.clone()
    };

    let external_root = external_version_dir.and_then(find_external_root);

    let mut search_paths: Vec<PathBuf> = Vec::new();

    // jar 字段优先
    if !jar_field.is_empty() {
        if let Some(root) = &external_root {
            search_paths.push(root.join("versions").join(&jar_field).join(format!("{}.jar", jar_field)));
        }
        search_paths.push(versions_dir.join(&jar_field).join(format!("{}.jar", jar_field)));
    }

    if external_version_dir.is_some() {
        if let Some(root) = &external_root {
            search_paths.push(root.join("versions").join(&actual_id).join(format!("{}.jar", actual_id)));
        }
        if let Some(ext_dir) = external_version_dir {
            search_paths.push(ext_dir.join(format!("{}.jar", actual_id)));
            let dir_name = ext_dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            search_paths.push(ext_dir.join(format!("{}.jar", dir_name)));
            if let Some(root) = &external_root {
                if dir_name != actual_id {
                    search_paths.push(root.join("versions").join(&dir_name).join(format!("{}.jar", dir_name)));
                }
            }
        }
    }

    search_paths.push(versions_dir.join(&actual_id).join(format!("{}.jar", actual_id)));

    if !inherits_from.is_empty() {
        if let Some(root) = &external_root {
            search_paths.push(root.join("versions").join(&inherits_from).join(format!("{}.jar", inherits_from)));
        }
        if let Some(ext_dir) = external_version_dir {
            if let Some(parent) = ext_dir.parent() {
                search_paths.push(parent.join(&inherits_from).join(format!("{}.jar", inherits_from)));
            }
        }
        search_paths.push(versions_dir.join(&inherits_from).join(format!("{}.jar", inherits_from)));
    }

    for p in &search_paths {
        if p.exists() {
            return Some(p.clone());
        }
    }

    // 外部根目录下扫描第一个含 jar 的版本目录
    if let Some(root) = &external_root {
        for jar_id in &[&jar_field, &inherits_from, &actual_id] {
            if jar_id.is_empty() {
                continue;
            }
            let ver_dir = root.join("versions").join(jar_id);
            if ver_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&ver_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().and_then(|e| e.to_str()) == Some("jar") {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }

    // 沿继承链递归查找主 JAR。
    // 传入的 version_json 可能是合并后的（inheritsFrom/jar 已被移除），
    // 因此这里重新读取当前版本的原始 JSON，恢复继承链并逐级向下查找。
    // 例：RLCraft → 1.12.2-forge-14.23.5.2860 → 1.12.2，最终命中 1.12.2.jar
    let mut chain_id = if inherits_from.is_empty() {
        actual_id.clone()
    } else {
        inherits_from.clone()
    };
    let mut depth = 0;
    while depth < 32 && !chain_id.is_empty() {
        depth += 1;

        // 尝试当前层级的 jar 是否存在
        let mut hit = versions_dir.join(&chain_id).join(format!("{}.jar", chain_id));
        if hit.exists() {
            return Some(hit);
        }
        if let Some(root) = &external_root {
            hit = root.join("versions").join(&chain_id).join(format!("{}.jar", chain_id));
            if hit.exists() {
                return Some(hit);
            }
        }

        // 读取父版本 JSON，跟随其 jar / inheritsFrom
        if let Some(json_path) = resolve_version_json(&chain_id, external_version_dir) {
            if let Ok(content) = std::fs::read_to_string(&json_path) {
                if let Ok(parent_json) = serde_json::from_str::<Value>(&content) {
                    let parent_jar = utils::get_str(&parent_json, "jar");
                    let parent_inherits = utils::get_str(&parent_json, "inheritsFrom");
                    // 若父版本显式声明 jar，则下一层级直接检查该 jar
                    if !parent_jar.is_empty() {
                        let jp = versions_dir.join(&parent_jar).join(format!("{}.jar", parent_jar));
                        if jp.exists() {
                            return Some(jp);
                        }
                        if let Some(root) = &external_root {
                            let jp2 = root.join("versions").join(&parent_jar).join(format!("{}.jar", parent_jar));
                            if jp2.exists() {
                                return Some(jp2);
                            }
                        }
                    }
                    chain_id = parent_inherits;
                    continue;
                }
            }
        }
        break;
    }

    None
}

/// 评估版本 JSON 的 rules 数组
/// 复刻原项目 server/versions/version-merge.js:evaluateRules
/// `has_custom_resolution` 对应原项目 features.has_custom_resolution 特性
pub(crate) fn evaluate_rules(rules: &Vec<Value>, has_custom_resolution: bool) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed: Option<bool> = None;
    let mut has_allow_rule = false;
    let current_os = current_platform_name();
    let current_arch = if cfg!(target_pointer_width = "64") { "x64" } else { "x86" };

    for rule in rules {
        let action = utils::get_str(rule, "action");
        if action == "allow" {
            has_allow_rule = true;
        }
        let mut rule_matched = true;

        if let Some(os) = rule.get("os").and_then(|v| v.as_object()) {
            if let Some(os_name) = os.get("name").and_then(|v| v.as_str()) {
                rule_matched = os_name == current_os;
            }
            if let Some(os_arch) = os.get("arch").and_then(|v| v.as_str()) {
                rule_matched = rule_matched && os_arch == current_arch;
            }
            // os.version 正则匹配（暂略，影响有限）
        }

        if let Some(features) = rule.get("features").and_then(|v| v.as_object()) {
            if features.contains_key("is_demo_user") {
                rule_matched = false;
            }
            if features.contains_key("has_custom_resolution") {
                rule_matched = rule_matched && has_custom_resolution;
            }
            // quick play 系列一律不匹配
            for key in ["has_quick_plays_support", "is_quick_play_singleplayer", "is_quick_play_multiplayer", "is_quick_play_realms"] {
                if features.contains_key(key) {
                    rule_matched = false;
                }
            }
        }

        if rule.get("os").is_none() && rule.get("features").is_none() {
            rule_matched = true;
        }

        if rule_matched {
            allowed = Some(action == "allow");
        }
    }

    allowed.unwrap_or(!has_allow_rule)
}

/// 获取 Java 主版本范围（综合 JSON 声明、MC 版本、加载器约束）
/// 对应原项目 server/java/java-version.js
pub(crate) fn get_java_version_range_from_json(version_id: &str, version_json: &Value) -> (u32, u32) {
    let mut min: u32 = 8;
    let mut max: u32 = 999;

    // 0. 清理外部版本标记：外部版本在列表中显示为 "26.2 [外部1]"，解析基础 MC 版本前需去掉后缀
    let clean_version_id = if version_id.contains(" [外部") {
        version_id.split(" [外部").next().unwrap_or(version_id).trim()
    } else if version_id.contains("[外部") {
        version_id.split("[外部").next().unwrap_or(version_id).trim()
    } else {
        version_id
    };

    // 1. JSON 明确要求的 javaVersion.majorVersion（优先级最高，作为下限）
    if let Some(jv) = version_json.get("javaVersion") {
        if let Some(major) = jv.get("majorVersion").and_then(|v| v.as_u64()) {
            let major = major as u32;
            if major > 8 {
                min = min.max(major);
            }
        }
    }

    // 2. 解析基础 MC 版本：优先 inheritsFrom，其次版本 id / version_id
    let inherits_from = utils::get_str(version_json, "inheritsFrom");
    let base_mc = if !inherits_from.is_empty() {
        extract_mc_version(&inherits_from)
    } else {
        let id = utils::get_str(version_json, "id");
        let from_id = if !id.is_empty() { id } else { clean_version_id.to_string() };
        extract_mc_version(&from_id)
    };
    let (major, minor, patch) = parse_version_triple(&base_mc);

    // 3. 按 MC 版本设定 Java 下限
    if major >= 2 {
        // 新版本号体系（如 26.2）：需要 Java 25+（Java 25 为长期支持版本）
        min = min.max(25);
    } else if major == 1 && minor >= 20 && patch >= 5 {
        min = min.max(21); // 1.20.5+ → Java 21
    } else if major == 1 && minor >= 18 {
        min = min.max(17); // 1.18+ → Java 17
    } else if major == 1 && minor == 17 {
        min = min.max(16); // 1.17 → Java 16
    }

    // 4. Forge / NeoForge 且 MC ≤1.12.2：必须 Java 8（launchwrapper 与 Java 9+ 不兼容）
    let version_id_lower = version_id.to_lowercase();
    let inherits_lower = inherits_from.to_lowercase();
    let is_neo = inherits_lower.contains("neoforge") || inherits_lower.contains("neoforged")
        || version_json.to_string().to_lowercase().contains("neoforged");
    let is_forge = (version_id_lower.contains("forge")
        || inherits_lower.contains("forge")
        || version_json.to_string().to_lowercase().contains("minecraftforge"))
        && !is_neo;
    if is_forge && (major == 1 && minor <= 12) {
        min = min.max(8);
        max = max.min(8);
    }

    // 5. Forge 1.18-1.20.4：官方推荐 Java 17，最高兼容到 Java 21。
    //    Java 22+ 会触发 modlauncher 模块导出冲突（如 "Modules ... and minecraft export
    //    package ... to module fancymenu"）导致启动崩溃，因此把上限限制在 21，
    //    避免误选过新的 Java。
    if is_forge && (major == 1 && minor >= 18 && (minor < 20 || (minor == 20 && patch <= 4))) {
        max = max.min(21);
    }

    // 6. launchwrapper 主类：与 Java 9+ 不兼容，强制最高 Java 8（最高优先级安全约束）
    let main_class = utils::get_str(version_json, "mainClass").to_lowercase();
    if main_class == "net.minecraft.launchwrapper.launch" || main_class.contains("launchwrapper") {
        max = max.min(8);
    }

    if min > max {
        max = min;
    }
    (min, max)
}

/// 从版本 ID 提取 Minecraft 版本号（如 "1.20.1-Forge-47.3.0" → "1.20.1"）
pub(crate) fn extract_mc_version(version_id: &str) -> String {
    for part in version_id.split('-') {
        let segs: Vec<&str> = part.split('.').collect();
        if segs.len() >= 2 {
            if segs[0].parse::<u32>().is_ok() && segs[1].parse::<u32>().is_ok() {
                return part.to_string();
            }
        }
    }
    String::new()
}

/// 解析版本号三元组 (major, minor, patch)，缺省为 0
pub(crate) fn parse_version_triple(version: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = version.split('.').collect();
    let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

/// 选择适合版本要求的 Java 路径
/// 简化版：优先版本独立 javaPath，其次全局 javaPath，最后从检测列表中匹配
/// 设置中的路径如果无法检测版本，会自动回退到检测列表
///
/// 匹配策略（与主流启动器一致）：
/// 以版本 JSON 声明的 javaVersion.majorVersion 为"精确目标版本"，优先选完全一致
/// 的 Java；找不到时再就近选不低于需求、不高于上限的版本，避免误选过高版本
/// 触发模块系统冲突（如 Forge 用 Java 25 启动 1.20.1 崩溃）。
pub(crate) fn select_java_for_version(version_id: &str, settings: &Value, version_json: &Value) -> String {
    let (req_min, req_max) = get_java_version_range_from_json(version_id, version_json);

    // 版本独立设置：指定的 Java 必须满足版本要求，否则回退
    let is_external = version_id.contains(" [外部");
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(version_id).to_string()
    } else {
        version_id.to_string()
    };
    let ver_settings = storage::load_version_settings(&clean_id, is_external);
    let ver_java = utils::get_str(&ver_settings, "javaPath");
    if !ver_java.is_empty() {
        if let Some((_, major)) = inspect_java_version(&ver_java) {
            if major >= req_min && major <= req_max {
                return ver_java;
            }
        }
    }
    // 全局设置：指定的 Java 必须满足版本要求，否则回退
    let global_java = utils::get_str(settings, "javaPath");
    if !global_java.is_empty() {
        if let Some((_, major)) = inspect_java_version(&global_java) {
            if major >= req_min && major <= req_max {
                return global_java;
            }
        }
    }
    // 检测列表中找满足范围的最高版本 Java
    let java_list = java::detect_all();
    eprintln!(
        "[select_java_for_version] 需求范围 {}~{}，检测到 {} 个 Java",
        req_min,
        req_max,
        java_list.len()
    );
    for j in &java_list {
        let mv = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0);
        let p = utils::get_str(j, "path");
        eprintln!("[select_java_for_version]   候选: major={} path={}", mv, p);
    }
    // 以版本 JSON 声明的 javaVersion.majorVersion 为精确目标版本：
    // 优先选与目标完全一致的 Java；找不到时再就近选不低于需求、不高于上限的版本，
    // 始终受 [req_min, req_max] 范围约束，避免误选过高版本。
    let target_major = version_json
        .get("javaVersion")
        .and_then(|jv| jv.get("majorVersion"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .filter(|v| *v > 8)
        .unwrap_or(req_min);

    let mut exact: Option<(&Value, u32)> = None;
    let mut nearest: Option<(&Value, i64)> = None; // (candidate, |major - target|)
    for j in &java_list {
        let mv = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if mv < req_min || mv > req_max {
            continue;
        }
        if mv == target_major {
            exact = Some((j, mv));
            break;
        }
        let dist = (mv as i64 - target_major as i64).abs();
        if nearest.is_none() || dist < nearest.unwrap().1 {
            nearest = Some((j, dist));
        }
    }
    if let Some((j, _)) = exact.or_else(|| nearest.map(|(j, _)| (j, 0))) {
        return utils::get_str(j, "path");
    }
    String::new()
}

/// 解析版本字符串中的主版本号
fn parse_java_major(version: &str) -> u32 {
    if version.starts_with("1.") {
        version.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(8)
    } else {
        version.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0)
    }
}

/// 执行 java -version 获取版本字符串和主版本号
/// 优先读取 <javaHome>/release 文件（更快、更稳），失败再执行 java -version
pub(crate) fn inspect_java_version(java_path: &str) -> Option<(String, u32)> {
    use std::process::Command;

    let path = PathBuf::from(java_path);
    // 1. 优先读 release 文件，避免某些 runtime 执行 java -version 失败导致误判
    if let Some(java_home) = path.parent().and_then(|p| p.parent()) {
        let release = java_home.join("release");
        if let Ok(content) = std::fs::read_to_string(&release) {
            for line in content.lines() {
                if line.starts_with("JAVA_VERSION=") {
                    let version = line.trim_start_matches("JAVA_VERSION=").trim_matches('"');
                    let major = parse_java_major(version);
                    return Some((version.to_string(), major));
                }
            }
        }
    }

    // 2. 回退到执行 java -version
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(java_path);
        c.arg("-version");
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new(java_path);
        c.arg("-version");
        c
    };
    let out = cmd.output().ok()?;
    // javaw.exe 通常没有 stderr/stdout 输出，若命令成功但无输出，尝试用 java.exe 同目录再测一次
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    // 解析 version "x.y.z"
    let mut version = String::new();
    for line in combined.lines() {
        if line.contains("version") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    version = line[start + 1..start + 1 + end].to_string();
                    break;
                }
            }
        }
    }
    if version.is_empty() {
        return None;
    }
    let major = parse_java_major(&version);
    Some((version, major))
}

// ============== 库检查子函数 ==============

/// 检查新格式 native（classifier 以 natives- 开头）
/// 返回 Some(MissingFile) 表示缺失，None 表示存在或跳过
fn check_native_lib(
    lib: &Value,
    libraries_dir: &Path,
    external_version_dir: Option<&Path>,
    current_platform: &str,
) -> Option<MissingFile> {
    let name = utils::get_str(lib, "name");
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 4 {
        return None;
    }
    let native_suffix = parts[3];
    let platform_native = native_suffix.strip_prefix("natives-").unwrap_or("");
    // 架构匹配
    let is_valid_platform = if cfg!(target_pointer_width = "64") {
        platform_native == current_platform || platform_native == format!("{}-x64", current_platform)
    } else if cfg!(target_arch = "x86") {
        platform_native == format!("{}-x86", current_platform) || platform_native == current_platform
    } else if cfg!(target_arch = "aarch64") {
        platform_native == format!("{}-arm64", current_platform) || platform_native == current_platform
    } else {
        false
    };
    if !is_valid_platform {
        return None;
    }

    // 优先从 downloads.artifact.path 取路径
    let mut native_path: Option<PathBuf> = None;
    if let Some(artifact_path) = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("path")).and_then(|v| v.as_str()) {
        let p = safe_lib_path(artifact_path, libraries_dir);
        if let Some(p) = p {
            if !p.exists() {
                if let Some(ext_dir) = external_version_dir {
                    if let Some(root) = find_external_root(ext_dir) {
                        let ext_p = safe_lib_path(artifact_path, &root.join("libraries"));
                        if let Some(ext_p) = ext_p {
                            if ext_p.exists() {
                                native_path = Some(ext_p);
                            }
                        }
                    }
                }
            } else {
                native_path = Some(p);
            }
        }
    }

    if native_path.is_none() && parts.len() >= 4 {
        // 按 maven 坐标构造本地路径
        let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR_STR);
        let jar_name = format!("{}-{}-{}.jar", parts[1], parts[2], parts[3]);
        let local_path = libraries_dir.join(&group_path).join(parts[1]).join(parts[2]).join(&jar_name);
        if local_path.exists() {
            native_path = Some(local_path);
        } else if let Some(ext_dir) = external_version_dir {
            if let Some(root) = find_external_root(ext_dir) {
                let ext_path = root.join("libraries").join(&group_path).join(parts[1]).join(parts[2]).join(&jar_name);
                if ext_path.exists() {
                    native_path = Some(ext_path);
                }
            }
        }
    }

    match native_path {
        Some(p) if p.exists() => {
            // 校验 SHA1（如果提供）
            let sha1 = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("");
            if !sha1.is_empty() {
                if let Some(actual) = utils::calculate_sha1(&p) {
                    if actual != sha1 {
                        let url = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("url")).and_then(|v| v.as_str()).unwrap_or("");
                        let size = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("size")).and_then(|v| v.as_u64()).unwrap_or(0);
                        return Some(MissingFile {
                            kind: "native".to_string(),
                            url: url.to_string(),
                            path: p.to_string_lossy().to_string(),
                            sha1: sha1.to_string(),
                            size,
                            name,
                            desc: String::new(),
                            message: String::new(),
                        });
                    }
                }
            }
            None
        }
        _ => {
            // 缺失：构造下载 URL
            let group_maven = parts[0].replace('.', "/");
            let jar_name = format!("{}-{}-{}.jar", parts[1], parts[2], parts[3]);
            let url = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("url")).and_then(|v| v.as_str()).unwrap_or("");
            let base_url = if !url.is_empty() {
                url.to_string()
            } else {
                let lib_url = utils::get_str(lib, "url");
                if !lib_url.is_empty() {
                    lib_url
                } else {
                    "https://libraries.minecraft.net/".to_string()
                }
            };
            let native_url = if !url.is_empty() {
                url.to_string()
            } else {
                format!("{}{}/{}/{}/{}", base_url, group_maven, parts[1], parts[2], jar_name)
            };
            let local_path = libraries_dir.join(parts[0].replace('.', std::path::MAIN_SEPARATOR_STR)).join(parts[1]).join(parts[2]).join(&jar_name);
            let sha1 = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("");
            let size = lib.get("downloads").and_then(|v| v.get("artifact")).and_then(|v| v.get("size")).and_then(|v| v.as_u64()).unwrap_or(0);
            Some(MissingFile {
                kind: "native".to_string(),
                url: native_url,
                path: local_path.to_string_lossy().to_string(),
                sha1: sha1.to_string(),
                size,
                name,
                desc: String::new(),
                message: String::new(),
            })
        }
    }
}

/// 检查旧格式 native（lib.natives 字典 + lib.downloads.classifiers）
fn check_legacy_native(
    lib: &Value,
    libraries_dir: &Path,
    external_version_dir: Option<&Path>,
    current_platform: &str,
) -> Option<MissingFile> {
    let natives = lib.get("natives").and_then(|v| v.as_object())?;
    let native_key = natives.get(current_platform).and_then(|v| v.as_str())?;
    let classifier = if cfg!(target_pointer_width = "64") {
        native_key.replace("${arch}", "64")
    } else {
        native_key.replace("${arch}", "32")
    };
    let native_download = lib.get("downloads").and_then(|v| v.get("classifiers")).and_then(|v| v.get(&classifier));
    let native_download = match native_download {
        Some(d) => d,
        None => return None,
    };

    let path_str = utils::get_str(native_download, "path");
    if path_str.is_empty() {
        return None;
    }
    let mut native_path = libraries_dir.join(&path_str);
    if !native_path.exists() {
        if let Some(ext_dir) = external_version_dir {
            if let Some(root) = find_external_root(ext_dir) {
                let ext_path = root.join("libraries").join(&path_str);
                if ext_path.exists() {
                    native_path = ext_path;
                }
            }
        }
    }

    if native_path.exists() {
        // 校验 SHA1
        let sha1 = utils::get_str(native_download, "sha1");
        if !sha1.is_empty() {
            if let Some(actual) = utils::calculate_sha1(&native_path) {
                if actual == sha1 {
                    return None;
                }
            }
        } else {
            return None;
        }
    }

    let url = utils::get_str(native_download, "url");
    let size = native_download.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    Some(MissingFile {
        kind: "native".to_string(),
        url,
        path: native_path.to_string_lossy().to_string(),
        sha1: utils::get_str(native_download, "sha1"),
        size,
        name: format!("{} ({})", utils::get_str(lib, "name"), classifier),
        desc: String::new(),
        message: String::new(),
    })
}

/// 检查标准 library（有 downloads.artifact）
fn check_standard_library(
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
        if let Some(ext_dir) = external_version_dir {
            if let Some(root) = find_external_root(ext_dir) {
                if let Some(ext_p) = safe_lib_path(&path_str, &root.join("libraries")) {
                    if ext_p.exists() {
                        lib_path = ext_p;
                    }
                }
            }
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
            if let Some(actual) = utils::calculate_sha1(&lib_path) {
                if actual == sha1 {
                    return;
                }
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
fn check_maven_library(
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
    if !lib_path.exists() {
        if let Some(ext_dir) = external_version_dir {
            if let Some(root) = find_external_root(ext_dir) {
                let ext_p = root.join("libraries").join(&group_path).join(name_part).join(version).join(&jar_name);
                if ext_p.exists() {
                    lib_path = ext_p;
                }
            }
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
            base_url = "https://maven.neoforged.net/".to_string();
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
/// 对应原项目 server/utils.js:safeLibPath
fn safe_lib_path(artifact_path: &str, base_dir: &Path) -> Option<PathBuf> {
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

// ============== 主 JAR 缺失处理 ==============

fn push_missing_main_jar(result: &mut DepCheckResult, version_json: &Value, version_id: &str, jar_path: &Path) {
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

fn push_missing_main_jar_with_fallback(result: &mut DepCheckResult, version_json: &Value, version_id: &str, versions_dir: &Path) {
    // 沿 inheritsFrom 链查找前置版本的 client 下载信息
    let mut fallback_url = String::new();
    let mut fallback_sha1 = String::new();
    let mut fallback_size: u64 = 0;
    let mut fallback_jar_id = String::new();

    let mut visited = std::collections::HashSet::new();
    let mut cur_json = version_json.clone();
    let mut cur_inherits = utils::get_str(&cur_json, "inheritsFrom");
    while !cur_inherits.is_empty() && !visited.contains(&cur_inherits) {
        visited.insert(cur_inherits.clone());
        let pj_path = versions_dir.join(&cur_inherits).join(format!("{}.json", cur_inherits));
        if !pj_path.exists() {
            break;
        }
        if let Ok(content) = std::fs::read_to_string(&pj_path) {
            if let Ok(pj) = serde_json::from_str::<Value>(&content) {
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

// ============== Forge 核心库检查 ==============

/// 沿 inheritsFrom 链判断版本是否依赖 Forge（非 NeoForge）
fn scan_inherits_forge(version_id: &str, visited: &mut std::collections::HashSet<String>) -> bool {
    if version_id.is_empty() || visited.contains(version_id) {
        return false;
    }
    let vl = version_id.to_lowercase();
    if vl.contains("forge") && !vl.contains("neoforge") && !vl.contains("neoforged") {
        return true;
    }
    visited.insert(version_id.to_string());
    let versions_dir = storage::resolve_data_dir().join("versions");
    let vj_path = versions_dir.join(version_id).join(format!("{}.json", version_id));
    if !vj_path.exists() {
        return false;
    }
    if let Ok(content) = std::fs::read_to_string(&vj_path) {
        if let Ok(vj) = serde_json::from_str::<Value>(&content) {
            let inherits = utils::get_str(&vj, "inheritsFrom");
            if !inherits.is_empty() && !visited.contains(&inherits) {
                return scan_inherits_forge(&inherits, visited);
            }
        }
    }
    false
}

/// 检查 Forge / NeoForge 核心库完整性
/// 对应原项目 server/dependencies/forge.js:checkForgeCore
fn check_forge_core(
    version_json: &Value,
    version_id: &str,
    external_version_dir: Option<&Path>,
    libraries_dir: &Path,
    result: &mut DepCheckResult,
) {
    let forge_libs: Vec<Value> = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter(|l| {
            let name = utils::get_str(l, "name");
            name.starts_with("net.minecraftforge:forge:")
                || name.starts_with("net.minecraftforge:fmlloader:")
                || name.starts_with("net.neoforged:neoforge:")
                || name.starts_with("net.neoforged.fancymodloader:")
                || (name.starts_with("net.minecraft:client:") && (name.ends_with(":srg") || name.ends_with(":extra")))
        }).cloned().collect())
        .unwrap_or_default();

    let v_lower = version_id.to_lowercase();
    let is_neo = v_lower.contains("neoforge") || v_lower.contains("neoforged");
    let has_forge_id = v_lower.contains("forge") && !is_neo;
    let has_forge_lib_only = version_json.get("libraries").and_then(|v| v.as_array()).map(|libs| {
        libs.iter().any(|l| {
            let name = utils::get_str(l, "name");
            name.starts_with("net.minecraftforge:forge:") || name.starts_with("net.minecraftforge:fmlloader:")
        })
    }).unwrap_or(false);

    let mut visited = std::collections::HashSet::new();
    let is_forge_version = has_forge_id || scan_inherits_forge(version_id, &mut visited) || has_forge_lib_only;

    if !is_forge_version {
        return;
    }

    // 新版 Forge 格式检测（MC 26+ 嵌入式）
    let game_args = version_json.get("arguments").and_then(|v| v.get("game")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let has_fml_args = game_args.iter().any(|a| {
        let s = a.as_str().unwrap_or("");
        s == "--fml.forgeVersion" || s == "--fml.mcVersion"
    });
    let main_class = utils::get_str(version_json, "mainClass");
    let has_bootstrap_main = main_class.contains("bootstraplauncher") || main_class.contains("BootstrapLauncher") || main_class.contains("cpw.mods");
    let has_forge_libs_in_json = forge_libs.iter().any(|l| {
        let name = utils::get_str(l, "name");
        name.starts_with("net.minecraftforge:forge:") || name.starts_with("net.minecraftforge:fmlloader:") || name.starts_with("net.minecraftforge:fmlcore:")
    });
    let is_new_forge_format = !is_neo && !has_fml_args && !has_bootstrap_main && !has_forge_libs_in_json;

    if is_new_forge_format {
        result.forge_core.ok = true;
        result.forge_core.message = "新版Forge格式，核心已嵌入版本JAR".to_string();
        return;
    }

    // 收集需要检查的核心库
    let mut forge_core_libs: Vec<(String, PathBuf, String)> = Vec::new(); // (name, path, desc)

    let find_forge_lib = |pred: &dyn Fn(&str) -> bool| -> Option<&Value> {
        forge_libs.iter().find(|l| pred(&utils::get_str(l, "name")))
    };

    // 从 libraries 中查找各类核心库
    let forge_client_lib = find_forge_lib(&|n| n.starts_with("net.minecraftforge:forge:") && (n.ends_with(":client") || n.split(':').count() == 3));
    let forge_main_lib = find_forge_lib(&|n| n.starts_with("net.minecraftforge:forge:") && n.split(':').count() == 3);
    let neo_forge_lib = find_forge_lib(&|n| n.starts_with("net.neoforged:neoforge:"));
    let neo_fml_lib = find_forge_lib(&|n| n.starts_with("net.neoforged.fancymodloader:loader:"));
    let srg_lib = find_forge_lib(&|n| n.starts_with("net.minecraft:client:") && n.ends_with(":srg"));
    let extra_lib = find_forge_lib(&|n| n.starts_with("net.minecraft:client:") && n.ends_with(":extra"));

    let external_root_for_forge = external_version_dir.and_then(find_external_root);

    let find_forge_core_file = |fp: &[&str], jar_name: &str| -> PathBuf {
        let group_path = fp[0].replace('.', std::path::MAIN_SEPARATOR_STR);
        let local_path = libraries_dir.join(&group_path).join(fp[1]).join(fp[2]).join(jar_name);
        if local_path.exists() {
            return local_path;
        }
        if let Some(root) = &external_root_for_forge {
            let ext_path = root.join("libraries").join(&group_path).join(fp[1]).join(fp[2]).join(jar_name);
            if ext_path.exists() {
                return ext_path;
            }
        }
        local_path
    };

    if let Some(lib) = forge_client_lib {
        let name = utils::get_str(lib, "name");
        let fp: Vec<&str> = name.split(':').collect();
        let cl = if fp.len() >= 4 { format!("-{}", fp[3]) } else { String::new() };
        let jar_name = format!("{}-{}{}.jar", fp[1], fp[2], cl);
        forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "Forge客户端核心".to_string()));
    }
    if let Some(lib) = forge_main_lib {
        if Some(lib) != forge_client_lib {
            let name = utils::get_str(lib, "name");
            let fp: Vec<&str> = name.split(':').collect();
            let jar_name = format!("{}-{}.jar", fp[1], fp[2]);
            forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "Forge主核心".to_string()));
        }
    }
    if let Some(lib) = srg_lib {
        let name = utils::get_str(lib, "name");
        let sp: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}-srg.jar", sp[1], sp[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&sp, &jar_name), "Minecraft SRG映射客户端".to_string()));
    }
    if let Some(lib) = extra_lib {
        let name = utils::get_str(lib, "name");
        let ep: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}-extra.jar", ep[1], ep[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&ep, &jar_name), "Minecraft额外客户端".to_string()));
    }
    if let Some(lib) = neo_forge_lib {
        let name = utils::get_str(lib, "name");
        let is_neo_client_virtual = name.ends_with(":client");
        let mut neo_patched_ok = false;
        if is_neo_client_virtual {
            let neo_ver = name.split(':').nth(2).unwrap_or("");
            let patched_jar = libraries_dir
                .join("net")
                .join("neoforged")
                .join("minecraft-client-patched")
                .join(neo_ver)
                .join(format!("minecraft-client-patched-{}.jar", neo_ver));
            neo_patched_ok = patched_jar.exists() && utils::is_jar_intact(&patched_jar);
        }
        if !is_neo_client_virtual || !neo_patched_ok {
            let fp: Vec<&str> = name.split(':').collect();
            let cl = if fp.len() >= 4 { format!("-{}", fp[3]) } else { String::new() };
            let jar_name = format!("{}-{}{}.jar", fp[1], fp[2], cl);
            forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "NeoForge核心".to_string()));
        }
    }
    if let Some(lib) = neo_fml_lib {
        let name = utils::get_str(lib, "name");
        let fp: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}.jar", fp[1], fp[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "NeoForge FML加载器".to_string()));
    }

    // libraries 中未直接找到时，按版本号目录结构兜底搜索
    if forge_core_libs.is_empty() {
        let mut forge_ver_match = None;
        // 优先从 game args / fmlloader 库中提取真实版本号
        let mut fallback_mc_ver: Option<String> = None;
        let mut fallback_f_ver: Option<String> = None;

        if let Some(fv_idx) = game_args.iter().position(|a| a.as_str() == Some("--fml.forgeVersion")) {
            if let Some(v) = game_args.get(fv_idx + 1).and_then(|v| v.as_str()) {
                fallback_f_ver = Some(v.to_string());
            }
        }
        if let Some(mv_idx) = game_args.iter().position(|a| a.as_str() == Some("--fml.mcVersion")) {
            if let Some(v) = game_args.get(mv_idx + 1).and_then(|v| v.as_str()) {
                fallback_mc_ver = Some(v.to_string());
            }
        }
        if fallback_mc_ver.is_none() {
            let cv = utils::get_str(version_json, "clientVersion");
            if !cv.is_empty() {
                fallback_mc_ver = Some(cv);
            }
        }
        if fallback_f_ver.is_none() || fallback_mc_ver.is_none() {
            if let Some(fml_lib) = forge_libs.iter().find(|l| {
                let name = utils::get_str(l, "name");
                name.starts_with("net.minecraftforge:fmlloader:") || name.starts_with("net.minecraftforge:forge:")
            }) {
                let fml_name = utils::get_str(fml_lib, "name");
                let parts: Vec<&str> = fml_name.split(':').collect();
                if parts.len() >= 3 {
                    let ver_part = parts[2];
                    if let Some(dash_idx) = ver_part.rfind('-') {
                        if dash_idx > 0 {
                            if fallback_mc_ver.is_none() {
                                fallback_mc_ver = Some(ver_part[..dash_idx].to_string());
                            }
                            if fallback_f_ver.is_none() {
                                fallback_f_ver = Some(ver_part[dash_idx + 1..].to_string());
                            }
                        }
                    }
                }
            }
        }

        if fallback_mc_ver.is_none() || fallback_f_ver.is_none() {
            // 正则匹配版本 ID
            if let Some(m) = regex_match_forge_version(version_id) {
                forge_ver_match = Some(m);
            } else if !inherits_from_field(version_json).is_empty() {
                if let Some(m) = regex_match_forge_version(&inherits_from_field(version_json)) {
                    forge_ver_match = Some(m);
                }
            }
        }

        let mc_ver = fallback_mc_ver.clone().or_else(|| forge_ver_match.as_ref().map(|m| m.0.clone()));
        let f_ver = fallback_f_ver.or_else(|| forge_ver_match.as_ref().map(|m| m.1.clone()));

        if let (Some(mc_ver), Some(f_ver)) = (mc_ver, f_ver) {
            let forge_search_bases = {
                let mut v: Vec<PathBuf> = vec![libraries_dir.to_path_buf()];
                if let Some(root) = &external_root_for_forge {
                    v.insert(0, root.join("libraries"));
                }
                v
            };

            // 1) Forge client jar
            for base in &forge_search_bases {
                let forge_dir = base.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver));
                if forge_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&forge_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.file_name().and_then(|n| n.to_str()).map(|s| s.ends_with("-client.jar")).unwrap_or(false) {
                                forge_core_libs.push((format!("forge-client:{}-{}", mc_ver, f_ver), p, "Forge客户端核心".to_string()));
                                break;
                            }
                        }
                    }
                    break;
                }
            }
            // 2) NeoForge jar
            for base in &forge_search_bases {
                let neo_dir = base.join("net").join("neoforged").join("neoforge").join(&f_ver);
                if neo_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&neo_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            if name.ends_with(".jar") && !name.ends_with("-sources.jar") && !name.ends_with("-javadoc.jar") {
                                forge_core_libs.push((format!("net.neoforged:neoforge:{}", f_ver), p, "NeoForge核心".to_string()));
                                break;
                            }
                        }
                    }
                    break;
                }
            }
            // 3) Minecraft client srg/extra
            for base in &forge_search_bases {
                let client_dir = base.join("net").join("minecraft").join("client");
                if client_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&client_dir) {
                        for entry in entries.flatten() {
                            let sd = entry.file_name().to_string_lossy().to_string();
                            if !sd.starts_with(&format!("{}-", mc_ver)) && sd != mc_ver {
                                continue;
                            }
                            let full_dir = client_dir.join(&sd);
                            if !full_dir.is_dir() {
                                continue;
                            }
                            if let Ok(files) = std::fs::read_dir(&full_dir) {
                                for f in files.flatten() {
                                    let name = f.file_name().to_string_lossy().to_string();
                                    let p = f.path();
                                    if name.ends_with("-srg.jar") {
                                        forge_core_libs.push((format!("client-srg:{}", sd), p.clone(), "Minecraft SRG映射客户端".to_string()));
                                    }
                                    if name.ends_with("-extra.jar") {
                                        forge_core_libs.push((format!("client-extra:{}", sd), p, "Minecraft额外客户端".to_string()));
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            }
            // 4) 新式 Forge 模块化核心（fmlcore/javafmllanguage/mclanguage/lowcodelanguage）
            if main_class.contains("bootstraplauncher") || main_class.contains("ForgeBootstrap") {
                let fml_version = format!("{}-{}", mc_ver, f_ver);
                for mod_name in &["fmlcore", "javafmllanguage", "mclanguage", "lowcodelanguage"] {
                    let is_declared = forge_libs.iter().any(|l| utils::get_str(l, "name").starts_with(&format!("net.minecraftforge:{}:{}", mod_name, fml_version)));
                    if is_declared {
                        let mod_path = libraries_dir.join("net").join("minecraftforge").join(mod_name).join(&fml_version).join(format!("{}-{}.jar", mod_name, fml_version));
                        if !forge_core_libs.iter().any(|(_, p, _)| p == &mod_path) {
                            forge_core_libs.push((format!("net.minecraftforge:{}:{}", mod_name, fml_version), mod_path, format!("Forge模块:{}", mod_name)));
                        }
                    }
                }
            }
            // 5) Forge 通用核心 universal jar（BootstrapLauncher 模式必须）
            if !is_neo {
                let is_bootstrap_main = main_class.contains("bootstraplauncher") || main_class.contains("ForgeBootstrap");
                if is_bootstrap_main {
                    let universal_path = libraries_dir.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver)).join(format!("forge-{}-{}-universal.jar", mc_ver, f_ver));
                    forge_core_libs.push((format!("net.minecraftforge:forge:{}-{}:universal", mc_ver, f_ver), universal_path, "Forge通用核心(FML MinecraftLocator)".to_string()));
                } else {
                    // 旧版 Forge binary patcher 模式：需要 client/srg/extra 三件套
                    let client_path = libraries_dir.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver)).join(format!("forge-{}-{}-client.jar", mc_ver, f_ver));
                    forge_core_libs.push((format!("net.minecraftforge:forge:{}-{}:client", mc_ver, f_ver), client_path, "Forge客户端核心".to_string()));

                    let client_base_dir = libraries_dir.join("net").join("minecraft").join("client");
                    let mut mcp_dir_name: Option<String> = None;
                    if client_base_dir.exists() {
                        if let Ok(entries) = std::fs::read_dir(&client_base_dir) {
                            for entry in entries.flatten() {
                                let d = entry.file_name().to_string_lossy().to_string();
                                if d.starts_with(&format!("{}-", mc_ver)) && entry.path().is_dir() {
                                    mcp_dir_name = Some(d);
                                    break;
                                }
                            }
                        }
                    }
                    if let Some(mcp) = mcp_dir_name {
                        let srg_path = client_base_dir.join(&mcp).join(format!("client-{}-srg.jar", mcp));
                        let extra_path = client_base_dir.join(&mcp).join(format!("client-{}-extra.jar", mcp));
                        forge_core_libs.push((format!("net.minecraft:client:{}:srg", mcp), srg_path, "Minecraft SRG映射客户端".to_string()));
                        forge_core_libs.push((format!("net.minecraft:client:{}:extra", mcp), extra_path, "Minecraft额外客户端".to_string()));
                    } else {
                        let srg_path = client_base_dir.join(format!("{}-mcp", mc_ver)).join(format!("client-{}-mcp-srg.jar", mc_ver));
                        let extra_path = client_base_dir.join(format!("{}-mcp", mc_ver)).join(format!("client-{}-mcp-extra.jar", mc_ver));
                        forge_core_libs.push((format!("net.minecraft:client:{}:srg", mc_ver), srg_path, "Minecraft SRG映射客户端".to_string()));
                        forge_core_libs.push((format!("net.minecraft:client:{}:extra", mc_ver), extra_path, "Minecraft额外客户端".to_string()));
                    }
                }
            }
        }
    }

    // 校验所有核心库是否存在且 JAR 完整
    for (name, path, desc) in &forge_core_libs {
        let exists = path.exists();
        let intact = if exists && path.extension().and_then(|e| e.to_str()) == Some("jar") {
            utils::is_jar_intact(path)
        } else {
            exists
        };
        if !intact {
            result.forge_core.missing.push(MissingFile {
                kind: "forge_core".to_string(),
                url: String::new(),
                path: path.to_string_lossy().to_string(),
                sha1: String::new(),
                size: 0,
                name: name.clone(),
                desc: desc.clone(),
                message: format!("Forge核心库缺失: {}", desc),
            });
        }
    }

    if !result.forge_core.missing.is_empty() {
        result.forge_core.ok = false;
        let missing_names = result.forge_core.missing.iter().map(|m| {
            if m.desc.is_empty() { m.name.clone() } else { m.desc.clone() }
        }).collect::<Vec<_>>().join("、");
        result.forge_core.message = format!(
            "{} 个Forge核心库文件缺失({})，无法启动游戏。\n修复建议:\n1) 前往\"版本设置 → 文件修复\"自动修复缺失文件\n2) 重新安装该Forge版本(版本设置 → 删除后重新安装)\n3) 检查杀毒软件是否将Forge核心库文件误删并加入白名单\n4) 如果使用自定义游戏目录,确认libraries文件夹完整",
            result.forge_core.missing.len(),
            missing_names
        );

        // 为缺失的核心库构造 maven 下载 URL
        for m in result.forge_core.missing.clone() {
            if result.missing_files.iter().any(|f| f.path == m.path) {
                continue;
            }
            let mut forge_url = String::new();
            if m.name.contains(':') {
                let parts: Vec<&str> = m.name.split(':').collect();
                if parts.len() >= 3 {
                    let group_id = parts[0];
                    let artifact_id = parts[1];
                    let version = parts[2];
                    let group_path = group_id.replace('.', "/");
                    let classifier_suffix = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
                    let maven_file = format!("{}-{}{}.jar", artifact_id, version, classifier_suffix);
                    forge_url = if group_id == "net.minecraft" {
                        format!("https://libraries.minecraft.net/{}/{}/{}/{}", group_path, artifact_id, version, maven_file)
                    } else {
                        format!("https://maven.minecraftforge.net/{}/{}/{}/{}", group_path, artifact_id, version, maven_file)
                    };
                }
            }
            let message = format!("Forge核心库缺失: {}", m.desc);
            result.missing_files.push(MissingFile {
                kind: "forge_core".to_string(),
                url: forge_url,
                path: m.path,
                sha1: String::new(),
                size: 0,
                name: m.name,
                desc: m.desc,
                message,
            });
        }
    }
}

/// 从版本 ID 中正则提取 (mc_version, forge_version)
fn regex_match_forge_version(version_id: &str) -> Option<(String, String)> {
    // 简化版正则：匹配 <mc>-Forge-<ver> 或 <mc>-NeoForge-<ver>
    let lower = version_id.to_lowercase();
    let marker = if lower.contains("neoforge") { "neoforge" } else if lower.contains("forge") { "forge" } else { return None; };
    let idx = lower.find(&format!("-{}", marker))?;
    let mc_ver = version_id[..idx].to_string();
    let rest = &version_id[idx + marker.len() + 2..]; // skip "-forge-"
    // 取到下一个 '-' 之前
    let end = rest.find('-').unwrap_or(rest.len());
    let f_ver = rest[..end].to_string();
    if mc_ver.is_empty() || f_ver.is_empty() {
        return None;
    }
    Some((mc_ver, f_ver))
}

fn inherits_from_field(version_json: &Value) -> String {
    utils::get_str(version_json, "inheritsFrom")
}

// ============== mrpack mods 完整性检查 ==============

fn check_mrpack_mods(version_id: &str, external_version_dir: Option<&Path>, result: &mut DepCheckResult) {
    let version_dir = external_version_dir
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| storage::resolve_data_dir().join("versions").join(version_id));
    let manifest_path = version_dir.join("mrpack-manifest.json");
    if !manifest_path.exists() {
        return;
    }
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let manifest: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    let files = manifest.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mod_entries: Vec<&Value> = files.iter().filter(|f| {
        utils::get_str(f, "path").starts_with("mods/")
    }).collect();
    result.mrpack_mods.total = mod_entries.len() as u64;

    for entry in &mod_entries {
        let path = utils::get_str(entry, "path");
        let file_name = Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let dest_path = version_dir.join(&path);
        let mut need_recheck = false;

        if !dest_path.exists() {
            need_recheck = true;
        } else {
            let expected_size = entry.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
            let expected_sha1 = entry.get("hashes").and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("");
            if expected_size > 0 {
                if let Ok(meta) = std::fs::metadata(&dest_path) {
                    if meta.len() != expected_size {
                        need_recheck = true;
                    }
                } else {
                    need_recheck = true;
                }
            }
            if !need_recheck && dest_path.extension().and_then(|e| e.to_str()) == Some("jar") {
                if !utils::is_jar_intact(&dest_path) {
                    need_recheck = true;
                }
            }
            if !need_recheck && !expected_sha1.is_empty() {
                if let Some(actual) = utils::calculate_sha1(&dest_path) {
                    if actual != expected_sha1 {
                        need_recheck = true;
                    }
                }
            }
        }

        if need_recheck {
            let dl_url = entry.get("downloads").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let sha1 = entry.get("hashes").and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let size = entry.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
            let urls: Vec<String> = entry.get("downloads").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let _ = urls; // 保留扩展字段以备下载模块使用
            result.mrpack_mods.missing.push(MissingFile {
                kind: "mod".to_string(),
                url: dl_url,
                path: dest_path.to_string_lossy().to_string(),
                sha1,
                size,
                name: file_name,
                desc: String::new(),
                message: String::new(),
            });
        }
    }

    result.mrpack_mods.ok = result.mrpack_mods.missing.is_empty();
    if !result.mrpack_mods.missing.is_empty() {
        result.mrpack_mods.message = format!("{} 个 Mod 文件缺失或损坏", result.mrpack_mods.missing.len());
        result.missing_files.extend(result.mrpack_mods.missing.clone());
    }
}

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
}
