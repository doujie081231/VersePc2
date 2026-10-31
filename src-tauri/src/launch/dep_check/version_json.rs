// version_json.rs — 由 dep_check.rs 拆分
#![allow(unused_imports)]

// 版本 JSON 解析、继承链、规则与 Java 版本匹配
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use regex::Regex;
use crate::java;
use crate::storage;
use crate::utils;
use super::*;

/// 获取当前平台名（windows/osx/linux）
pub(crate) fn current_platform_name() -> String {
    crate::platform::rules_os_name().to_string()
}

/// 获取当前架构名（与版本 JSON rules 中的 os.arch 取值一致）
pub(crate) fn current_arch_name() -> &'static str {
    crate::platform::rules_arch_name()
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
            if let Ok(content) = std::fs::read_to_string(&p)
                && (content.contains("mainClass")
                    || content.contains("libraries")
                    || content.contains("inheritsFrom")
                    || content.contains("minecraftArguments")
                    || content.contains("arguments"))
                {
                    return Some(p);
                }
        }
    }
    None
}

/// 仅查找版本 JSON 路径（不读取内容），用于前置版本存在性判断
pub(crate) fn find_version_json_path(version_id: &str, external_version_dir: Option<&Path>) -> Option<PathBuf> {
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
    if current.get("id").is_none()
        && let Value::Object(ref mut m) = current {
            m.insert("id".to_string(), Value::String(version_id.to_string()));
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
/// 深度合并全部字段（对象递归、数组按 Newtonsoft MergeArrayHandling.Concat 拼接、
/// 其余 scalar 由子版本覆盖），libraries 单独处理为"子版本在前、父版本在后"，
/// 继承标记在合并链中已消费后移除。
pub(crate) fn merge_two_version_jsons(base: Value, child: Value) -> Value {
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
pub(crate) fn deep_merge_json(parent: &Value, child: &Value) -> Value {
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
    let _jar_name = if !jar_field.is_empty() {
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
            if let Some(root) = &external_root
                && dir_name != actual_id {
                    search_paths.push(root.join("versions").join(&dir_name).join(format!("{}.jar", dir_name)));
                }
        }
    }

    search_paths.push(versions_dir.join(&actual_id).join(format!("{}.jar", actual_id)));

    if !inherits_from.is_empty() {
        if let Some(root) = &external_root {
            search_paths.push(root.join("versions").join(&inherits_from).join(format!("{}.jar", inherits_from)));
        }
        if let Some(ext_dir) = external_version_dir
            && let Some(parent) = ext_dir.parent() {
                search_paths.push(parent.join(&inherits_from).join(format!("{}.jar", inherits_from)));
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
            if ver_dir.exists()
                && let Ok(entries) = std::fs::read_dir(&ver_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().and_then(|e| e.to_str()) == Some("jar") {
                            return Some(p);
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
        if let Some(json_path) = resolve_version_json(&chain_id, external_version_dir)
            && let Ok(content) = std::fs::read_to_string(&json_path)
                && let Ok(parent_json) = serde_json::from_str::<Value>(&content) {
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
        break;
    }

    None
}

/// 评估版本 JSON 的 rules 数组
/// `has_custom_resolution` 特性
pub(crate) fn evaluate_rules(
    rules: &Vec<Value>,
    has_custom_resolution: bool,
    has_quick_play: bool,
) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed: Option<bool> = None;
    let mut has_allow_rule = false;
    let current_os = current_platform_name();
    let current_arch = current_arch_name();

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
            if let Some(os_ver) = os.get("version").and_then(|v| v.as_str())
                && !os_ver.is_empty() {
                    rule_matched = rule_matched && os_version_matches(os_ver);
                }
        }

        if let Some(features) = rule.get("features").and_then(|v| v.as_object()) {
            if features.contains_key("is_demo_user") {
                rule_matched = false;
            }
            if features.contains_key("has_custom_resolution") {
                rule_matched = rule_matched && has_custom_resolution;
            }
            // quick play：单机快速进世界是否启用由 has_quick_play 决定
            if features.contains_key("has_quick_plays_support") {
                rule_matched = rule_matched && has_quick_play;
            }
            if features.contains_key("is_quick_play_singleplayer") {
                rule_matched = rule_matched && has_quick_play;
            }
            if features.contains_key("is_quick_play_multiplayer")
                || features.contains_key("is_quick_play_realms")
            {
                rule_matched = false;
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

/// 从版本 JSON 中识别到的加载器信息
#[derive(Default)]
struct LoaderInfo {
    forge: Option<String>,
    neoforge: Option<String>,
    fabric: Option<String>,
    optifine: bool,
    liteloader: bool,
    /// 加载器版本号自带的 MC 版本前缀（Forge/FML 的 "1.20.1-47.3.33"）
    mc_version: String,
}

static FORGE_COORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:net\.minecraftforge:(?:forge|fmlloader):|net/minecraftforge/(?:forge|fmlloader)/)([^/:\s]+)").unwrap()
});
static NEOFORGE_COORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:net\.neoforged:(?:neoforge|fancymodloader):|net/neoforged/(?:neoforge|fancymodloader)/)([^/:\s]+)").unwrap()
});
static FABRIC_COORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:net\.fabricmc:fabric-loader:|net/fabricmc/fabric-loader/)([^/:\s]+)").unwrap()
});

/// 是否为可信的 Minecraft 版本号：
/// - 旧体系必须是 1.x（1.0 ~ 1.21 等）
/// - 新体系从 26.x 开始；2.x ~ 19.x 不可能是 MC 版本，避免把整合包版本（如 2.7.1）误判成 MC 2.7.1
fn is_plausible_mc_version(version: &str) -> bool {
    let (major, minor, _) = parse_version_triple(version);
    if major == 1 {
        return minor <= 50;
    }
    major >= 20
}

/// 从加载器坐标里取出 MC 版本前缀与加载器自身版本
/// 例如 Forge "1.16.5-36.2.42" → ("1.16.5", "36.2.42")
fn split_forge_version(value: &str) -> (String, String) {
    if let Some((mc, forge)) = value.split_once('-')
        && is_plausible_mc_version(mc) {
            return (mc.to_string(), forge.to_string());
        }
    (String::new(), value.to_string())
}

/// 从版本 JSON 的 libraries 中识别 Forge / NeoForge / Fabric / OptiFine / LiteLoader。
/// 不能只依赖版本 id 或 inheritsFrom 里的 "forge" 字样：整合包重命名后（如“香草纪元-2.7.1”）
/// 这些字样可能完全不存在，但 libraries 里仍有确定的加载器坐标。
fn detect_loaders(version_json: &Value) -> LoaderInfo {
    let mut info = LoaderInfo::default();
    if let Some(libs) = version_json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            let mut texts = Vec::new();
            if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
                texts.push(name.to_string());
            }
            if let Some(path) = lib.pointer("/downloads/artifact/path").and_then(|v| v.as_str()) {
                texts.push(path.to_string());
            }
            if let Some(url) = lib.pointer("/downloads/artifact/url").and_then(|v| v.as_str()) {
                texts.push(url.to_string());
            }
            for text in &texts {
                let lower = text.to_lowercase();
                if info.forge.is_none()
                    && let Some(cap) = FORGE_COORD_RE.captures(text) {
                        let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                        let (mc, forge) = split_forge_version(raw);
                        if !mc.is_empty() && info.mc_version.is_empty() {
                            info.mc_version = mc;
                        }
                        if !forge.is_empty() {
                            info.forge = Some(forge);
                        }
                    }
                if info.neoforge.is_none()
                    && let Some(cap) = NEOFORGE_COORD_RE.captures(text)
                    && let Some(ver) = cap.get(1) {
                        info.neoforge = Some(ver.as_str().to_string());
                    }
                if info.fabric.is_none()
                    && let Some(cap) = FABRIC_COORD_RE.captures(text)
                    && let Some(ver) = cap.get(1) {
                        info.fabric = Some(ver.as_str().to_string());
                    }
                if lower.contains("optifine") {
                    info.optifine = true;
                }
                if lower.contains("liteloader") {
                    info.liteloader = true;
                }
            }
        }
    }
    // 兜底：libraries 缺失时仍可从 inheritsFrom / id 的 "forge-版本" 里识别
    let inherited = utils::get_str(version_json, "inheritsFrom").to_lowercase();
    let json_id = utils::get_str(version_json, "id").to_lowercase();
    for text in [inherited, json_id] {
        if info.forge.is_none()
            && let Some(pos) = text.find("forge-") {
                let forge = text[pos + "forge-".len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !forge.is_empty() {
                    info.mc_version = extract_mc_version(&text[..pos]);
                    info.forge = Some(forge);
                }
            }
        if info.neoforge.is_none()
            && let Some(pos) = text.find("neoforge-") {
                let ver = text[pos + "neoforge-".len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !ver.is_empty() {
                    info.neoforge = Some(ver);
                }
            }
    }
    info
}

/// 解析版本号为数字段，用于加载器版本区间比较
fn version_numbers(value: &str) -> Vec<u64> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect()
}

fn version_ge(a: &str, b: &str) -> bool {
    let av = version_numbers(a);
    let bv = version_numbers(b);
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    true
}

fn version_lt(a: &str, b: &str) -> bool {
    !version_ge(a, b)
}

/// 获取 Java 主版本范围（综合 JSON 声明、MC 版本、加载器约束）
///
/// 加载器规则对齐 PCL：Forge 只按“Forge 自身版本区间 + MC 版本”限制上限，
/// 不能靠版本 id 是否包含 "forge" 决定，否则重命名整合包会误选过高 Java。
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
    if let Some(jv) = version_json.get("javaVersion")
        && let Some(major) = jv.get("majorVersion").and_then(|v| v.as_u64()) {
            let major = major as u32;
            if major > 8 {
                min = min.max(major);
            }
        }

    // 2. 解析基础 MC 版本：加载器坐标最可靠，其次 inheritsFrom，最后版本 id / version_id
    let loaders = detect_loaders(version_json);
    let inherits_from = utils::get_str(version_json, "inheritsFrom");
    let base_mc = if !loaders.mc_version.is_empty() {
        loaders.mc_version.clone()
    } else if !inherits_from.is_empty() {
        extract_mc_version(&inherits_from)
    } else {
        let id = utils::get_str(version_json, "id");
        let from_id = if !id.is_empty() { id } else { clean_version_id.to_string() };
        extract_mc_version(&from_id)
    };
    let (major, minor, patch) = parse_version_triple(&base_mc);
    let is_old_mc = major == 1 && minor <= 12;

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

    // 4. Forge：按 PCL 的 Forge 版本区间限制，而不是猜测版本 id
    if let Some(forge) = &loaders.forge {
        if is_old_mc || base_mc.is_empty() {
            // <=1.12 / 非标准版本：Forge launchwrapper 只兼容 Java 8
            max = max.min(8);
        } else if major == 1 && minor <= 14 {
            max = max.min(10); // 1.13 - 1.14
        } else if major == 1 && minor == 15 {
            max = max.min(15); // 1.15
        } else if version_ge(forge, "34.0.0") && version_ge("36.2.25", forge) {
            max = max.min(8); // 1.16.3 ~ 1.16.5，Forge 34.0.0 ~ 36.2.25
        } else if version_ge(forge, "36.2.26") && version_lt(forge, "37.0.0") {
            max = max.min(23); // 1.16.5，Forge 36.2.26+
        } else if version_ge(forge, "37.0.0") && version_ge("37.0.79", forge) {
            max = max.min(16); // 1.17.1
        } else if version_ge(forge, "45.0.21") && version_ge("45.0.65", forge) {
            max = max.min(19); // 1.19.4
        } else if version_ge(forge, "45.0.66") && version_ge("47.4.8", forge) {
            max = max.min(21); // 1.19.4 ~ 1.20.1
        }
    }

    // 5. NeoForge：1.20.1 与 1.20.2 早期版本最高 Java 21
    if let Some(neo) = &loaders.neoforge
        && ((major == 1 && minor == 20 && patch == 1)
            || !version_ge(neo, "20.2.63-beta")) {
            max = max.min(21);
        }

    // 6. Fabric Loader 0.16.x 及更早版本的 Mixin/ASM 不兼容 Java 25
    if let Some(fabric) = &loaders.fabric
        && version_lt(fabric, "0.17.0") {
            max = max.min(24);
        }

    // 7. OptiFine / LiteLoader
    if loaders.optifine
        && major == 1
        && (minor < 7 || (8..=12).contains(&minor)) {
            max = max.min(8);
        }
    if loaders.liteloader {
        max = max.min(8);
    }

    // 8. launchwrapper 主类：与 Java 9+ 不兼容，强制最高 Java 8（最高优先级安全约束）
    let main_class = utils::get_str(version_json, "mainClass").to_lowercase();
    if main_class == "net.minecraft.launchwrapper.launch" || main_class.contains("launchwrapper") {
        max = max.min(8);
    }

    if min > max {
        max = min;
    }
    (min, max)
}

/// 从版本 ID 提取 Minecraft 版本号（如 "1.20.1-Forge-47.3.0" → "1.20.1"）。
/// 只接受可信 MC 版本，避免把整合包自身版本 2.7.1 当成 MC 2.7.1。
pub(crate) fn extract_mc_version(version_id: &str) -> String {
    for part in version_id.split(['-', ' ', '_', '+', '(', ')']) {
        let candidate = part.trim();
        if is_plausible_mc_version(candidate) {
            return candidate.to_string();
        }
    }
    String::new()
}

/// 解析版本号三元组 (major, minor, patch)，缺省为 0
pub(crate) fn parse_version_triple(version: &str) -> (u32, u32, u32) {
    let parts: Vec<&str> = version.split('.').collect();
    let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
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
    if !ver_java.is_empty()
        && let Some((_, major)) = inspect_java_version(&ver_java)
            && major >= req_min && major <= req_max {
                return ver_java;
            }
    // 全局设置：指定的 Java 必须满足版本要求，否则回退
    let global_java = utils::get_str(settings, "javaPath");
    if !global_java.is_empty()
        && let Some((_, major)) = inspect_java_version(&global_java)
            && major >= req_min && major <= req_max {
                return global_java;
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
        if nearest.as_ref().map(|(_, d)| dist < *d).unwrap_or(true) {
            nearest = Some((j, dist));
        }
    }
    if let Some((j, _)) = exact.or_else(|| nearest.map(|(j, _)| (j, 0))) {
        return utils::get_str(j, "path");
    }
    String::new()
}

/// 解析版本字符串中的主版本号
pub(crate) fn parse_java_major(version: &str) -> u32 {
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
        if line.contains("version")
            && let Some(start) = line.find('"')
                && let Some(end) = line[start + 1..].find('"') {
                    version = line[start + 1..start + 1 + end].to_string();
                    break;
                }
    }
    if version.is_empty() {
        return None;
    }
    let major = parse_java_major(&version);
    Some((version, major))
}
