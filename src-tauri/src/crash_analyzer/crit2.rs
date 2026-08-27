// crash_analyzer/crit2.rs — 二级崩溃分析
// 职责：Mixin 失败、Forge/Fabric 报错、Suspected Mod 等较精准的匹配
// 运行优先级：高于堆栈分析、低于一级分析；一级分析已命中则不执行。

use std::collections::HashMap;

use super::constants::CrashReason;
use super::prepare::PreparedLogs;
use super::stack::{try_analyze_mod_name, try_analyze_mod_name_one};
use super::utils::{append_reason, between, regex_seek_group, regex_seek_all_group};

/// 二级分析
pub fn analyze(logs: &PreparedLogs) -> HashMap<CrashReason, Vec<String>> {
    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();
    let log_mc = logs.log_mc.as_deref();
    let log_crash = logs.log_crash.as_deref();
    let log_mc_debug = logs.log_mc_debug.as_deref();

    // Mixin 分析（游戏日志）
    if let Some(lm) = log_mc {
        let is_mixin = mixin_analyze(lm, log_crash, log_mc_debug, &mut reasons);

        // Forge 报错
        if lm.contains("An exception was thrown, the game will display an error screen and halt.") {
            if let Some(msg) = regex_seek_group(
                lm,
                r"the game will display an error screen and halt.[\n\r]+[^\n]+?Exception: ([\s\S]+?)(?=\n\tat)",
                1,
            ) {
                append_reason(&mut reasons, CrashReason::ForgeCrash, Some(vec![msg.trim().to_string()]));
            }
        }

        // Fabric 报错并给出解决方案（三种文案）
        for needle in FABRIC_SOLUTION_NEEDLES {
            if lm.contains(needle) {
                // 提取解决方案块（以 "- xxx" 开头的行），再取每行内容
                if let Some(block) = regex_seek_group(
                    lm,
                    &format!(r"{}[\n\r]+((?:\s+ - [^\n]+[\n\r]+)+)", regex::escape(needle)),
                    1,
                ) {
                    let lines: Vec<String> = regex_seek_all_group(r"(?m)\s+ - ([^\n]+)", &block, 1)
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !lines.is_empty() {
                        append_reason(&mut reasons, CrashReason::FabricCrash, Some(lines));
                    }
                }
                break;
            }
        }

        // "due to errors, provided by '...'"（Mixin 失败除外）
        if !is_mixin && lm.contains("due to errors, provided by ") {
            if let Some(name) = regex_seek_group(lm, r"due to errors, provided by '([^']+)", 1) {
                let names = try_analyze_mod_name_one(&name.trim_end_matches('\n'), log_crash, log_mc_debug);
                append_reason(&mut reasons, CrashReason::ModCrashed, Some(names));
            }
        }
    }

    // 崩溃报告分析
    if let Some(lc) = log_crash {
        mixin_analyze(lc, log_crash, log_mc_debug, &mut reasons);

        // Suspected Mod
        if lc.contains("Suspected Mod") {
            let suspects_raw = between(lc, "Suspected Mod", "Stacktrace");
            if !suspects_raw.trim_start().starts_with("s: None") {
                let suspects = regex_seek_group(suspects_raw, r"(?m)\n\t[^(\t]+\(([^)\n]+)", 1);
                if let Some(s) = suspects {
                    let names = try_analyze_mod_name(
                        &s.lines().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>(),
                        log_crash,
                        log_mc_debug,
                    );
                    append_reason(&mut reasons, CrashReason::SuspectedModCrash, Some(names));
                }
            }
        }
    }

    reasons
}

const FABRIC_SOLUTION_NEEDLES: &[&str] = &[
    "A potential solution has been determined:",
    "A potential solution has been determined, this may resolve your problem:",
    "确定了一种可能的解决方法，这样做可能会解决你的问题：",
];

/// Mixin 失败检测；返回是否检测到 Mixin 崩溃
fn mixin_analyze(
    log_text: &str,
    log_crash: Option<&str>,
    log_mc_debug: Option<&str>,
    reasons: &mut HashMap<CrashReason, Vec<String>>,
) -> bool {
    let is_mixin = log_text.contains("Mixin prepare failed ")
        || log_text.contains("Mixin apply failed ")
        || log_text.contains("MixinApplyError")
        || log_text.contains("MixinTransformerError")
        || log_text.contains("mixin.injection.throwables.")
        || log_text.contains(".json] FAILED during )");
    if !is_mixin {
        return false;
    }

    // Mod 名称匹配
    let mod_name = regex_seek_group(log_text, r"from mod ([^.\/ ]+)(?=\] from)", 1)
        .or_else(|| regex_seek_group(log_text, r"for mod ([^.\/ ]+)(?= failed)", 1));
    if let Some(name) = mod_name {
        let names = try_analyze_mod_name_one(&name.trim_end_matches('\n'), log_crash, log_mc_debug);
        append_reason(reasons, CrashReason::ModMixinError, Some(names));
        return true;
    }

    // JSON 名称匹配
    for json_name in super::utils::regex_seek_all_group(
        r"(?m)^[^\t]+[ \[{(]([^ \[{(]+\.[^ ]+)(?=\.json)",
        log_text,
        1,
    ) {
        let name = json_name
            .replace("mixins", "mixin")
            .replace(".mixin", "")
            .replace("mixin.", "");
        let names = try_analyze_mod_name_one(&name, log_crash, log_mc_debug);
        append_reason(reasons, CrashReason::ModMixinError, Some(names));
        return true;
    }

    // 没有明确匹配
    append_reason(reasons, CrashReason::ModMixinError, None);
    true
}