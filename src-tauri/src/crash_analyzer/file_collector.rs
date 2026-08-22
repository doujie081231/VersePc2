// crash_analyzer/file_collector.rs — 日志文件收集与导入
// 职责：从 .minecraft 目录按时间窗口收集 crash-reports/版本目录/latest.log/hs_err_pid

use std::path::Path;
use std::time::SystemTime;

use serde_json::json;

/// 收集到的原始日志文件
#[derive(Clone, Debug)]
pub struct RawLogFile {
    pub path: String,
    pub lines: Vec<String>,
}

/// 收集参数
pub struct CollectParams<'a> {
    pub minecraft_dir: &'a Path,
    pub version_path_index: &'a str,
    pub latest_log_lines: Option<Vec<String>>,
}

/// 步骤 1：收集可能存在的日志文件
///
/// 流程：
/// 1. crash-reports 目录下的 crash-*.txt
/// 2. 版本目录下的 *.log
/// 3. logs/latest.log、logs/debug.log
/// 4. .minecraft/hs_err_pid*.log
/// 5. 去重
/// 6. 筛选最近 30 分钟内修改的非空文件
/// 7. 若全无则放宽时间限制，取所有非空文件
/// 8. 若仍无则用 latestLog 写入 RawOutput.log
pub fn collect(params: CollectParams) -> Vec<RawLogFile> {
    let mc_dir = params.minecraft_dir;
    let mut possible_logs: Vec<std::path::PathBuf> = Vec::new();

    // 1. 搜索 crash-reports 目录
    let crash_reports_dir = mc_dir.join("crash-reports");
    if let Ok(entries) = std::fs::read_dir(&crash_reports_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("crash-") && name.ends_with(".txt") {
                    possible_logs.push(entry.path());
                }
            }
        }
    }

    // 2. 搜索版本目录下的日志
    if !params.version_path_index.is_empty() {
        let version_dir = mc_dir.join("versions").join(params.version_path_index);
        if version_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&version_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".log") {
                            possible_logs.push(entry.path());
                        }
                    }
                }
            }
        }
    }

    // 3. 添加 latest.log 和 debug.log
    possible_logs.push(mc_dir.join("logs").join("latest.log"));
    possible_logs.push(mc_dir.join("logs").join("debug.log"));

    // 4. 搜索 hs_err_pid*.log
    if let Ok(entries) = std::fs::read_dir(mc_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("hs_err_pid") && name.ends_with(".log") {
                    possible_logs.push(entry.path());
                }
            }
        }
    }

    // 5. 去重
    possible_logs.sort();
    possible_logs.dedup();

    // 6. 筛选最近 30 分钟内修改的非空文件
    let now = SystemTime::now();
    let mut right_logs: Vec<std::path::PathBuf> = Vec::new();
    for log_path in &possible_logs {
        if let Ok(meta) = std::fs::metadata(log_path) {
            if meta.len() == 0 {
                continue;
            }
            if let Ok(mtime) = meta.modified() {
                if let Ok(duration) = now.duration_since(mtime) {
                    if duration.as_secs() < 30 * 60 {
                        right_logs.push(log_path.clone());
                    }
                }
            }
        }
    }

    // 7. 若无最近修改的日志，放宽时间限制，使用所有非空文件
    if right_logs.is_empty() {
        for log_path in &possible_logs {
            if let Ok(meta) = std::fs::metadata(log_path) {
                if meta.len() > 0 {
                    right_logs.push(log_path.clone());
                }
            }
        }
    }

    // 8. 若仍无，用 latestLog 写入 RawOutput.log
    if right_logs.is_empty() {
        if let Some(lines) = params.latest_log_lines {
            return vec![RawLogFile {
                path: "RawOutput.log".to_string(),
                lines,
            }];
        }
        return Vec::new();
    }

    // 读取所有文件内容
    let mut result: Vec<RawLogFile> = Vec::new();
    for log_path in &right_logs {
        let path_str = log_path.to_string_lossy().to_string();
        if let Ok(content) = std::fs::read_to_string(log_path) {
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            if !lines.is_empty() {
                result.push(RawLogFile {
                    path: path_str,
                    lines,
                });
            }
        }
    }

    result
}

/// 导入单个日志文件（手动分析场景）
/// 注意：暂不支持 .jar/.zip 解压（用户手动分析场景较少）
pub fn import_file(file_path: &Path) -> Option<RawLogFile> {
    if !file_path.exists() {
        return None;
    }

    let path_str = file_path.to_string_lossy().to_string();
    if let Ok(content) = std::fs::read_to_string(file_path) {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if !lines.is_empty() {
            return Some(RawLogFile {
                path: path_str,
                lines,
            });
        }
    }
    None
}

/// 将 RawLogFile 序列化为 JSON（供调试或前端展示）
pub fn raw_log_file_to_json(file: &RawLogFile) -> serde_json::Value {
    json!({
        "path": file.path,
        "lineCount": file.lines.len()
    })
}
