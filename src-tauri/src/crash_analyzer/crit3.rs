// crash_analyzer/crit3.rs — 三级崩溃分析
// 职责：极短输出、Mod 解析失败、Mod 实例创建失败、特定方块/实体导致崩溃
// 运行优先级：最低，仅在更高级别未命中时执行。

use std::collections::HashMap;

use super::constants::CrashReason;
use super::prepare::PreparedLogs;
use super::stack::try_analyze_mod_name_one;
use super::utils::{append_reason, regex_seek_group};

/// 三级分析
pub fn analyze(logs: &PreparedLogs) -> HashMap<CrashReason, Vec<String>> {
    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();
    let log_mc = logs.log_mc.as_deref();
    let log_hs = logs.log_hs.as_deref();
    let log_crash = logs.log_crash.as_deref();
    let log_mc_debug = logs.log_mc_debug.as_deref();

    // 游戏日志分析
    if let Some(lm) = log_mc {
        // 极短的程序输出
        let is_normal = lm.contains("at net.") || lm.contains("INFO]");
        if !is_normal && log_hs.is_none() && log_crash.is_none() && lm.len() < 100 {
            append_reason(&mut reasons, CrashReason::ShortLogOutput, Some(vec![lm.to_string()]));
        }

        // Mod 解析错误（常见于 Fabric 前置校验失败）
        if lm.contains("Mod resolution failed") {
            append_reason(&mut reasons, CrashReason::ModLoaderError, None);
        }

        // Mixin 失败可能造成大量 Mod 实例创建失败，因此放到低优先级
        if lm.contains("Failed to create mod instance.") {
            let name = regex_seek_group(lm, r"Failed to create mod instance\. ModID: ([^,]+)", 1)
                .or_else(|| regex_seek_group(lm, r"Failed to create mod instance\. ModId ([^\n]+)(?= for )", 1));
            if let Some(n) = name {
                let names = try_analyze_mod_name_one(&n.trim_end_matches('\n'), log_crash, log_mc_debug);
                append_reason(&mut reasons, CrashReason::ModInitError, Some(names));
            } else {
                append_reason(&mut reasons, CrashReason::ModInitError, None);
            }
        }
    }

    // 崩溃报告分析
    if let Some(lc) = log_crash {
        // 特定方块导致崩溃
        if lc.contains("\tBlock location: World: ") {
            let block = regex_seek_group(lc, r"\tBlock: Block\{([^\}]+)", 1).unwrap_or_default();
            let location = regex_seek_group(lc, r"\tBlock location: World: (\([^\)]+\))", 1).unwrap_or_default();
            let additional = vec![format!("{} {}", block, location).trim().to_string()];
            append_reason(&mut reasons, CrashReason::BlockCrash, Some(additional));
        }

        // 特定实体导致崩溃
        if lc.contains("\tEntity's Exact location: ") {
            let entity = regex_seek_group(lc, r"\tEntity Type: ([^\n]+)(?= \()", 1).unwrap_or_default();
            let location = regex_seek_group(lc, r"\tEntity's Exact location: ([^\n]+)", 1).unwrap_or_default();
            let additional = vec![format!("{} ({})", entity, location.trim_end_matches('\n')).trim().to_string()];
            append_reason(&mut reasons, CrashReason::EntityCrash, Some(additional));
        }
    }

    reasons
}