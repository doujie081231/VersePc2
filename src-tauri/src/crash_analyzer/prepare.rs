// crash_analyzer/prepare.rs — 日志预处理与分类
// 职责：按文件名分类（HsErr/CrashReport/MinecraftLog/ExtraLog），截取头尾行用于分析

use std::collections::HashMap;

use super::file_collector::RawLogFile;

/// 文件类型分类
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FileType {
    HsErr,
    CrashReport,
    MinecraftLog,
    ExtraLogFile,
    ExtraReportFile,
}

/// 预处理后的日志内容
#[derive(Default, Debug)]
pub struct PreparedLogs {
    pub log_mc: Option<String>,
    pub log_mc_debug: Option<String>,
    pub log_hs: Option<String>,
    pub log_crash: Option<String>,
    pub output_files: Vec<String>,
}

/// 步骤 2：预处理日志文件
///
/// 流程：
/// 1. 按文件名分类
/// 2. 无 MinecraftLog 时用 ExtraLogFile 顶替
/// 3. 按类型截取头尾行
///    - HsErr: 头200 + 尾100 行（去重）
///    - CrashReport: 头300 + 尾700 行（去重）
///    - MinecraftLog: 优先启动器输出日志，其次 latest.log 头1500+尾500 行
///    - logMcDebug: debug.log 头1000 行
/// 4. 返回是否找到有效日志
pub fn prepare(raw_files: &[RawLogFile]) -> PreparedLogs {
    let mut result = PreparedLogs::default();
    let mut all_files: HashMap<FileType, &RawLogFile> = HashMap::new();

    // 1. 按文件名分类
    for log_file in raw_files {
        let file_name = std::path::Path::new(&log_file.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        let file_type = if file_name.starts_with("hs_err") {
            FileType::HsErr
        } else if file_name.starts_with("crash-") {
            FileType::CrashReport
        } else if is_minecraft_log(&file_name) {
            FileType::MinecraftLog
        } else if file_name.ends_with(".log") {
            FileType::ExtraLogFile
        } else if file_name.ends_with(".txt") {
            FileType::ExtraReportFile
        } else {
            continue;
        };

        if log_file.lines.is_empty() {
            continue;
        }
        all_files.insert(file_type, log_file);
    }

    // 2. 没有 MinecraftLog 时用 ExtraLogFile 顶替
    if !all_files.contains_key(&FileType::MinecraftLog) {
        if let Some(extra) = all_files.get(&FileType::ExtraLogFile).copied() {
            all_files.insert(FileType::MinecraftLog, extra);
            all_files.remove(&FileType::ExtraLogFile);
        }
    }

    // 3. 按类型截取
    for (file_type, file) in &all_files {
        result.output_files.push(file.path.clone());

        match file_type {
            FileType::HsErr => {
                // JVM 崩溃日志：头 200 行 + 尾 100 行
                result.log_hs = Some(get_head_tail_lines(&file.lines, 200, 100));
            }
            FileType::CrashReport => {
                // 崩溃报告：头 300 行 + 尾 700 行
                result.log_crash = Some(get_head_tail_lines(&file.lines, 300, 700));
            }
            FileType::MinecraftLog => {
                let mut log_mc = String::new();
                let mut log_mc_debug = String::new();

                // 建立文件名 → 文件对象的映射
                let mut file_name_dict: HashMap<String, &RawLogFile> = HashMap::new();
                for (_, f) in &all_files {
                    let name = std::path::Path::new(&f.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    file_name_dict.insert(name, *f);
                }

                // 优先使用启动器输出日志（从标记行开始截取）
                for launcher_name in &[
                    "rawoutput.log",
                    "启动器输出日志.txt",
                    "log1.txt",
                    "pcl2启动器输出日志.txt",
                    "pcl启动器输出日志.txt",
                ] {
                    if let Some(current_log) = file_name_dict.get(*launcher_name) {
                        let mut has_launcher_mark = false;
                        for line in &current_log.lines {
                            if has_launcher_mark {
                                log_mc.push_str(line);
                                log_mc.push('\n');
                            } else if line.contains("启动器输出日志") {
                                has_launcher_mark = true;
                            }
                        }
                        if !has_launcher_mark {
                            log_mc.push_str(&get_head_tail_lines(&current_log.lines, 0, 500));
                        }
                        break;
                    }
                }

                // 其次使用 latest.log / debug.log（取头 1500 行 + 尾 500 行）
                for latest_name in &["latest.log", "latest log.txt", "debug.log", "debug log.txt"] {
                    if let Some(current_log) = file_name_dict.get(*latest_name) {
                        log_mc.push_str(&get_head_tail_lines(&current_log.lines, 1500, 500));
                        break;
                    }
                }

                // 单独提取 debug.log 作为 Debug 日志（取头 1000 行）
                for debug_name in &["debug.log", "debug log.txt"] {
                    if let Some(current_log) = file_name_dict.get(*debug_name) {
                        log_mc_debug.push_str(&get_head_tail_lines(&current_log.lines, 1000, 0));
                        break;
                    }
                }

                // 兜底
                if log_mc.is_empty() {
                    if !log_mc_debug.is_empty() {
                        log_mc = log_mc_debug.clone();
                    } else if !file_name_dict.is_empty() {
                        let (_, first_log) = file_name_dict.iter().next().unwrap();
                        log_mc.push_str(&get_head_tail_lines(&first_log.lines, 1500, 500));
                    }
                }

                result.log_mc = if log_mc.is_empty() { None } else { Some(log_mc) };
                result.log_mc_debug = if log_mc_debug.is_empty() {
                    None
                } else {
                    Some(log_mc_debug)
                };
            }
            FileType::ExtraLogFile | FileType::ExtraReportFile => {
                // 额外日志文件不主动处理
            }
        }
    }

    result
}

/// 判断文件名是否属于 Minecraft 主日志
fn is_minecraft_log(file_name: &str) -> bool {
    matches!(
        file_name,
        "latest.log" | "latest log.txt" | "debug.log" | "debug log.txt"
    ) || file_name.contains("启动器输出日志")
        || file_name == "rawoutput.log"
        || file_name == "log1.txt"
        || file_name.contains("pc l2启动器输出日志")
        || file_name.contains("pcl启动器输出日志")
}

/// 提取日志头部 head_lines 行 + 尾部 tail_lines 行（去重）
pub fn get_head_tail_lines(lines: &[String], head_lines: usize, tail_lines: usize) -> String {
    if lines.len() <= head_lines + tail_lines {
        // 全量去重
        let mut seen: Vec<&str> = Vec::new();
        for line in lines {
            if !seen.contains(&line.as_str()) {
                seen.push(line.as_str());
            }
        }
        return seen.join("\n");
    }

    let mut result: Vec<&str> = Vec::new();

    // 头部 head_lines 行（去重）
    let mut real_head_count = 0;
    for line in lines.iter().take(lines.len()) {
        if result.contains(&line.as_str()) {
            continue;
        }
        result.push(line.as_str());
        real_head_count += 1;
        if real_head_count >= head_lines {
            break;
        }
    }

    // 尾部 tail_lines 行（去重），插入到头部行之后
    let mut real_tail_count = 0;
    for line in lines.iter().rev() {
        if result.contains(&line.as_str()) {
            continue;
        }
        result.insert(real_head_count, line.as_str());
        real_tail_count += 1;
        if real_tail_count >= tail_lines {
            break;
        }
    }

    result.join("\n")
}
