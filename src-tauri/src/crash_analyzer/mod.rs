// crash_analyzer/mod.rs — 崩溃日志分析模块入口
// 对应原项目 server/crash-analyzer/index.js
//
// 职责：定义 CrashAnalyzer 结构体，编排 收集 → 预处理 → 分析 → 输出 流程
//
// 架构说明：
//   原 JS 项目使用"原型混入"风格，所有方法挂在 CrashAnalyzer.prototype 上
//   Rust 改为"结构体 + 纯函数模块"，各子模块无状态，由 CrashAnalyzer 持有状态并调度
//   这样新增分析规则（crit2/crit3/mod_analyzer）只需扩展 analyze 函数即可

pub mod constants;
pub mod crit1;
pub mod crit2;
pub mod crit3;
pub mod file_collector;
pub mod prepare;
pub mod stack;
pub mod suggest;
pub mod utils;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use self::constants::{default_minecraft_dir, CrashReason};
use self::file_collector::{collect, import_file, CollectParams};
use self::prepare::{prepare, PreparedLogs};

/// 崩溃分析器
/// 持有配置和中间状态，对外提供 analyze / analyze_with_logs 等方法
pub struct CrashAnalyzer {
    pub minecraft_dir: PathBuf,
    pub target_instance: Option<String>,
}

/// 分析输出结构
/// 对应原项目 output.js output() 返回对象
#[derive(Debug, Clone)]
pub struct AnalysisOutput {
    pub detail: String,
    pub files: Vec<String>,
    pub crash_reasons: Vec<(CrashReason, Vec<String>)>,
    pub log_mc: Option<String>,
    pub log_hs: Option<String>,
    pub log_crash: Option<String>,
}

impl CrashAnalyzer {
    /// 创建分析器
    /// target_instance: 目标版本实例名（用于定位版本目录下的日志）
    /// minecraft_dir: .minecraft 目录，None 时用默认值
    pub fn new(target_instance: Option<String>, minecraft_dir: Option<PathBuf>) -> Self {
        Self {
            minecraft_dir: minecraft_dir.unwrap_or_else(default_minecraft_dir),
            target_instance,
        }
    }

    /// 主分析流程：收集 → 预处理 → 一级分析 → 输出
    /// 对应原项目 index.js + analyze.js + output.js 的编排
    pub fn analyze(&self, version_path_index: &str) -> AnalysisOutput {
        // 步骤 1：收集日志文件
        let raw_files = collect(CollectParams {
            minecraft_dir: &self.minecraft_dir,
            version_path_index,
            latest_log_lines: None,
        });

        // 步骤 2：预处理
        let logs = prepare(&raw_files);

        // 步骤 3：完整分析（Crit1 → Crit2 → 堆栈分析 → Crit3）
        let reasons = analyze_logs(&logs);

        // 步骤 4：生成建议文本
        let detail = suggest::get_analyze_result(&reasons, false);

        let crash_reasons: Vec<(CrashReason, Vec<String>)> = {
            let mut v: Vec<(CrashReason, Vec<String>)> = reasons
                .iter()
                .map(|(k, val)| (k.clone(), val.clone()))
                .collect();
            v.sort_by_key(|(r, _)| suggest::reason_order_pub(r));
            v
        };

        AnalysisOutput {
            detail,
            files: logs.output_files.clone(),
            crash_reasons,
            log_mc: logs.log_mc.clone(),
            log_hs: logs.log_hs.clone(),
            log_crash: logs.log_crash.clone(),
        }
    }

    /// 手动导入分析：用户选择一个日志文件进行分析
    /// 对应原项目 /api/crash/analyze（手动导入模式）
    pub fn analyze_file(&self, file_path: &Path) -> AnalysisOutput {
        let raw_files = match import_file(file_path) {
            Some(f) => vec![f],
            None => {
                let mut r: HashMap<CrashReason, Vec<String>> = HashMap::new();
                utils::append_reason(
                    &mut r,
                    CrashReason::Unknown,
                    Some(vec!["无法读取文件".to_string()]),
                );
                let detail = suggest::get_analyze_result(&r, true);
                return AnalysisOutput {
                    detail,
                    files: vec![],
                    crash_reasons: vec![],
                    log_mc: None,
                    log_hs: None,
                    log_crash: None,
                };
            }
        };

        let logs = prepare(&raw_files);
        let reasons = analyze_logs(&logs);

        let detail = suggest::get_analyze_result(&reasons, true);

        let crash_reasons: Vec<(CrashReason, Vec<String>)> = {
            let mut v: Vec<(CrashReason, Vec<String>)> = reasons
                .iter()
                .map(|(k, val)| (k.clone(), val.clone()))
                .collect();
            v.sort_by_key(|(r, _)| suggest::reason_order_pub(r));
            v
        };

        AnalysisOutput {
            detail,
            files: logs.output_files.clone(),
            crash_reasons,
            log_mc: logs.log_mc.clone(),
            log_hs: logs.log_hs.clone(),
            log_crash: logs.log_crash.clone(),
        }
    }
}

impl AnalysisOutput {
    /// 序列化为 JSON（供 API 返回）
    /// 对应原项目 output.js output() 返回对象
    pub fn to_json(&self) -> Value {
        let crash_reasons_json: Vec<Value> = self
            .crash_reasons
            .iter()
            .map(|(reason, additional)| {
                json!({
                    "reason": reason.as_str(),
                    "additional": additional
                })
            })
            .collect();

        json!({
            "detail": self.detail,
            "files": self.files,
            "crashReasons": crash_reasons_json,
            "logMc": self.log_mc,
            "logHs": self.log_hs,
            "logCrash": self.log_crash
        })
    }
}

/// 公开的辅助函数：从外部模块触发分析
/// 用于 API 路由层（api/crash.rs）调用
pub fn analyze_crash(
    target_instance: Option<String>,
    minecraft_dir: Option<PathBuf>,
    version_path_index: &str,
) -> AnalysisOutput {
    let analyzer = CrashAnalyzer::new(target_instance, minecraft_dir);
    analyzer.analyze(version_path_index)
}

/// 公开的辅助函数：手动导入文件分析
pub fn analyze_crash_file(file_path: &Path) -> AnalysisOutput {
    let analyzer = CrashAnalyzer::new(None, None);
    analyzer.analyze_file(file_path)
}

/// 完整崩溃分析编排（按优先级依次执行各分析步骤）：
/// 1. 精准日志匹配（Crit1，高优先级），命中即返回
/// 2. 精准日志匹配（Crit2，中优先级），命中即返回
/// 3. 若安装了加载器，进行虚拟机堆栈分析与 Mod 名称匹配；命中即返回
/// 4. 精准日志匹配（Crit3，低优先级）
fn analyze_logs(logs: &PreparedLogs) -> HashMap<CrashReason, Vec<String>> {
    // 1. 一级分析
    let reasons = crit1::analyze(logs);
    if !reasons.is_empty() {
        return reasons;
    }

    // 2. 二级分析
    let reasons = crit2::analyze(logs);
    if !reasons.is_empty() {
        return reasons;
    }

    // 拼接全部日志用于判断是否安装了加载器
    let log_mc = logs.log_mc.as_deref().unwrap_or("");
    let log_mc_debug = logs.log_mc_debug.as_deref().unwrap_or("");
    let log_hs = logs.log_hs.as_deref().unwrap_or("");
    let log_crash = logs.log_crash.as_deref().unwrap_or("");
    let log_all = if !log_mc.is_empty() {
        format!("{}{}{}", log_mc, log_hs, log_crash)
    } else {
        format!("{}{}{}", log_mc_debug, log_hs, log_crash)
    };

    // 3. 堆栈分析（仅当安装了 Mod：日志中出现 forge/fabric/quilt/liteloader 等字样）
    if log_all.contains("orge")
        || log_all.contains("abric")
        || log_all.contains("uilt")
        || log_all.contains("iteloader")
    {
        let mut keywords: Vec<String> = Vec::new();

        // 崩溃日志堆栈
        if !log_crash.is_empty() {
            let stack = utils::before_first(log_crash, "System Details");
            keywords.extend(stack::analyze_stack_keyword(stack));
        }

        // Minecraft 日志堆栈
        if !log_mc.is_empty() {
            let mut fatals: Vec<String> =
                utils::regex_seek_all(r"/FATAL] .+?(?=\n+\[)", log_mc);
            if log_mc.contains("Unreported exception thrown!") {
                fatals.push(utils::between(log_mc, "Unreported exception thrown!", "at oolloo.jlw.Wrapper").to_string());
            }
            for fatal in &fatals {
                keywords.extend(stack::analyze_stack_keyword(fatal));
            }
        }

        // 虚拟机日志堆栈
        if !log_hs.is_empty() {
            let stack_logs = utils::between(log_hs, "T H R E A D", "Registers:");
            keywords.extend(stack::analyze_stack_keyword(stack_logs));
        }

        if !keywords.is_empty() {
            match stack::analyze_mod_name(
                &keywords,
                logs.log_crash.as_deref(),
                logs.log_mc_debug.as_deref(),
            ) {
                Some(names) => {
                    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();
                    utils::append_reason(&mut reasons, CrashReason::StackModNameFound, Some(names));
                    return reasons;
                }
                None => {
                    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();
                    utils::append_reason(&mut reasons, CrashReason::StackKeywordFound, Some(keywords));
                    return reasons;
                }
            }
        }
    }

    // 4. 三级分析
    crit3::analyze(logs)
}
