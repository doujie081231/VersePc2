// crash_analyzer/stack.rs — 虚拟机堆栈分析与 Mod 名称匹配
// 职责：
//   1. 从堆栈中提取可能的 Mod 关键词（analyze_stack_keyword）
//   2. 根据关键词匹配实际的 Mod 名称（analyze_mod_name）
//   3. 查找不匹配时回退原关键词的辅助（try_analyze_mod_name）

use std::collections::HashSet;

use super::utils::{after_last, regex_seek, regex_seek_all_group};

/// 堆栈开头需要忽略的包前缀
const IGNORE_STACKS: &[&str] = &[
    "java", "sun", "javax", "jdk", "oolloo", "org.lwjgl", "com.sun", "net.minecraftforge",
    "paulscode.sound", "com.mojang", "net.minecraft", "cpw.mods", "com.google", "org.apache",
    "org.spongepowered", "net.fabricmc", "com.mumfrey",
    "com.electronwill.nightconfig", "it.unimi.dsi",
    "MojangTricksIntelDriversForPerformance_javaw",
];

/// 从堆栈中提取 Mod ID 关键词。若失败则返回空列表。
pub fn analyze_stack_keyword(error_stack: &str) -> Vec<String> {
    let es = format!("\n{}\n", error_stack);

    // 正则匹配
    let mut results: Vec<String> = Vec::new();
    results.extend(regex_seek_all_group(
        r"\n[^{]+([a-zA-Z_]+\w+\.[a-zA-Z_]+[\w\.]+)(?=\.[\w\.$]+\.)",
        &es,
        1,
    ));
    results.extend(
        regex_seek_all_group(
            r"at [^(]+?\.\w+\$\w+\$([\w\$]+?)(?=\$\w+\()",
            &es,
            1,
        )
        .into_iter()
        .map(|s| s.replace('$', ".")),
    );
    // 去重
    let mut seen: HashSet<String> = HashSet::new();
    results.retain(|s| seen.insert(s.clone()));

    // 检查堆栈开头，过滤掉框架包
    let mut possible_stacks: Vec<String> = Vec::new();
    for stack in &results {
        if IGNORE_STACKS.iter().any(|p| stack.starts_with(p)) {
            continue;
        }
        possible_stacks.push(stack.trim().to_string());
    }
    let mut seen2: HashSet<String> = HashSet::new();
    possible_stacks.retain(|s| seen2.insert(s.clone()));

    if possible_stacks.is_empty() {
        return Vec::new();
    }

    // 检查堆栈关键词（取包名前最多 4 节中的有效词作为可能的 Mod ID）
    let mut possible_words: Vec<String> = Vec::new();
    for stack in &possible_stacks {
        let splited: Vec<&str> = stack.split('.').collect();
        let max = std::cmp::min(3, splited.len() - 1);
        for i in 0..=max {
            let word = splited[i];
            if word.len() <= 2 || word.starts_with("func_") {
                continue;
            }
            let wl = word.to_lowercase();
            if WORD_STOPLIST.iter().any(|w| *w == wl) {
                continue;
            }
            possible_words.push(word.trim().to_string());
        }
    }
    let mut seen3: HashSet<String> = HashSet::new();
    possible_words.retain(|s| seen3.insert(s.clone()));

    if possible_words.len() > 10 {
        // 关键词过多，视为匹配出错，不纳入考虑
        Vec::new()
    } else {
        possible_words
    }
}

/// 不作为关键词的常见词
const WORD_STOPLIST: &[&str] = &[
    "com", "org", "net", "asm", "fml", "mod", "jar", "sun", "lib", "map", "gui", "dev", "nio",
    "api", "dsi", "top", "mcp", "core", "init", "mods", "main", "file", "game", "load", "read",
    "done", "util", "tile", "item", "base", "fake", "oshi", "impl", "data", "pool", "task",
    "forge", "setup", "block", "model", "mixin", "event", "unimi", "netty", "world", "lwjgl",
    "fakes", "fabric", "gitlab", "common", "server", "config", "mixins", "compat", "loader",
    "launch", "script", "entity", "assist", "client", "plugin", "modapi", "mojang", "shader",
    "events", "github", "recipe", "render", "packet", "events", "preinit", "preload", "machine",
    "reflect", "channel", "general", "handler", "content", "systems", "modules", "service",
    "scripts", "network", "fastutil", "optifine", "internal", "platform", "override", "fabricmc",
    "neoforge", "external", "injection", "listeners", "scheduler", "minecraft", "universal",
    "multipart", "neoforged", "microsoft", "transformer", "transformers", "minecraftforge",
    "blockentity", "spongepowered", "electronwill", "concurrent",
];

/// 根据 Mod 关键词尝试获取实际的 Mod 名称。若失败则返回 None。
pub fn analyze_mod_name(
    keywords: &[String],
    log_crash: Option<&str>,
    log_mc_debug: Option<&str>,
) -> Option<Vec<String>> {
    let mut mod_file_names: Vec<String> = Vec::new();

    // 预处理关键词（分割括号）
    let mut real_keywords: Vec<String> = Vec::new();
    for kw in keywords {
        for sub in kw.split('(') {
            let t = sub.trim_matches(|c| c == ' ' || c == ')').to_string();
            if !t.is_empty() {
                real_keywords.push(t);
            }
        }
    }

    // 从崩溃报告获取 Mod 信息
    if let Some(lc) = log_crash {
        if lc.contains("A detailed walkthrough of the error") {
            let details = lc.replace("A detailed walkthrough of the error", "¨");
            let is_fabric_detail = details.contains("Fabric Mods");
            let details = if is_fabric_detail {
                details.replace("Fabric Mods", "¨")
            } else {
                details
            };
            let details = after_last(&details, "¨");

            // [Forge] 获取所有包含 .jar 的行；[Fabric] 获取所有含 Mod 信息的行
            let mut mod_name_lines: Vec<String> = Vec::new();
            for line in details.split('\n') {
                let has_single_jar =
                    line.to_lowercase().matches(".jar").count() == 1;
                let is_fabric_mod_line = is_fabric_detail
                    && line.starts_with("\t\t")
                    && !regex_seek(line, r"\t\tfabric[\w-]*: Fabric").is_some();
                if has_single_jar || is_fabric_mod_line {
                    mod_name_lines.push(line.to_string());
                }
            }

            // 获取 Mod ID 与关键词的匹配行
            let mut hint_lines: Vec<String> = Vec::new();
            for kw in &real_keywords {
                let kw_lower = kw.to_lowercase().replace('_', "");
                for mod_string in &mod_name_lines {
                    let real_mod = mod_string.to_lowercase().replace('_', "");
                    if !real_mod.contains(&kw_lower) {
                        continue;
                    }
                    let lower = mod_string.to_lowercase();
                    if lower.contains("minecraft.jar")
                        || lower.contains(" forge-")
                        || lower.contains(" mixin-")
                    {
                        continue;
                    }
                    hint_lines.push(mod_string.trim_matches('\n').to_string());
                    break;
                }
            }
            let mut seen: HashSet<String> = HashSet::new();
            hint_lines.retain(|s| seen.insert(s.clone()));

            // 从 Mod 匹配行中提取 .jar 文件的名称
            for line in &hint_lines {
                let name = if is_fabric_detail {
                    // Fabric: 取冒号后到下个空白前的部分
                    super::utils::regex_seek_group(line, r": ([^\n]+)(?= [^\n]+)", 1)
                } else {
                    // Forge: 括号内或 (tab/| 之后) 的 .jar
                    super::utils::regex_seek_group(
                        line,
                        r"(?:\(|(?:\t\t|\| ))([^\t\|]+\.jar)",
                        1,
                    )
                };
                if let Some(n) = name {
                    if !mod_file_names.contains(&n) {
                        mod_file_names.push(n);
                    }
                }
            }
        }
    }

    // 从 debug.log 获取 Mod 信息
    if let Some(debug) = log_mc_debug {
        // Forge: Found valid mod file YungsBetterStrongholds-...jar with {betterstrongholds} mods
        let mod_name_lines: Vec<String> =
            regex_seek_all_group(r"(?m)valid mod file (.*)", debug, 1);

        let mut hint_lines: Vec<String> = Vec::new();
        for kw in &real_keywords {
            for mod_string in &mod_name_lines {
                if mod_string.contains(&format!("{{{}}}", kw)) {
                    hint_lines.push(mod_string.clone());
                }
            }
        }
        let mut seen: HashSet<String> = HashSet::new();
        hint_lines.retain(|s| seen.insert(s.clone()));

        for line in &hint_lines {
            if let Some(name) = regex_seek(line, r".*(?= with)") {
                if !mod_file_names.contains(&name) {
                    mod_file_names.push(name);
                }
            }
        }
    }

    if mod_file_names.is_empty() {
        None
    } else {
        Some(mod_file_names)
    }
}

/// 尝试从关键字获取 Mod 名称，若失败则返回原关键字列表。
pub fn try_analyze_mod_name(
    orig: &[String],
    log_crash: Option<&str>,
    log_mc_debug: Option<&str>,
) -> Vec<String> {
    if orig.is_empty() {
        return orig.to_vec();
    }
    match analyze_mod_name(orig, log_crash, log_mc_debug) {
        Some(names) => names,
        None => orig.to_vec(),
    }
}

/// 为单关键词适配的便捷辅助：返回 String 列表
pub fn try_analyze_mod_name_one(
    kw: &str,
    log_crash: Option<&str>,
    log_mc_debug: Option<&str>,
) -> Vec<String> {
    let orig = if kw.is_empty() {
        Vec::new()
    } else {
        vec![kw.to_string()]
    };
    if orig.is_empty() {
        return orig;
    }
    match analyze_mod_name(&orig, log_crash, log_mc_debug) {
        Some(names) => names,
        None => orig,
    }
}