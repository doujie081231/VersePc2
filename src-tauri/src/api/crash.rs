// api/crash.rs — 崩溃分析相关 API 路由
// 对应原项目 server/api/routes/crash.js
//
// 路由清单：
//   POST /api/crash/analyze       手动导入日志文件分析
//   GET  /api/crash/logs           列出历史崩溃日志
//   GET  /api/crash/log-content    读取单个崩溃日志内容
//   POST /api/crash/export         导出崩溃报告

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::api::ApiResult;
use crate::crash_analyzer;
use crate::launch;
use crate::storage;

/// 构建崩溃日志搜索目录集合（按版本解析真实游戏目录）
/// 复刻 handle_list_logs / handle_log_content / handle_export 原有目录逻辑，
/// 并补充版本实际游戏目录，覆盖隔离版本 `data/versions/<id>/crash-reports`。
fn version_crash_search_dirs(version_id: &str) -> Vec<PathBuf> {
    let data_dir = storage::resolve_data_dir();
    let settings = storage::load_settings();
    let mut dirs: Vec<PathBuf> = Vec::new();

    if !version_id.is_empty() {
        let versions_dir = data_dir.join("versions");
        let root = launch::args_builder::resolve_game_dir(
            version_id,
            None,
            None,
            &settings,
            &versions_dir,
            &data_dir,
        );
        dirs.push(root.join("crash-reports"));
        dirs.push(versions_dir.join(version_id).join("crash-reports"));
    }

    let game_dir = crate::utils::get_str(&settings, "gameDir");
    if !game_dir.is_empty() {
        dirs.push(PathBuf::from(&game_dir).join("crash-reports"));
    }
    dirs.push(data_dir.join("crash-reports"));
    dirs.push(crash_analyzer::constants::default_minecraft_dir().join("crash-reports"));
    dirs
}

/// 构建崩溃日志路径白名单（覆盖所有版本隔离目录 + 全局目录）
fn crash_whitelist_dirs() -> Vec<PathBuf> {
    let data_dir = storage::resolve_data_dir();
    let settings = storage::load_settings();
    let mut dirs: Vec<PathBuf> = Vec::new();

    let game_dir = crate::utils::get_str(&settings, "gameDir");
    if !game_dir.is_empty() {
        dirs.push(PathBuf::from(&game_dir).join("crash-reports"));
    }
    dirs.push(data_dir.join("crash-reports"));
    dirs.push(crash_analyzer::constants::default_minecraft_dir().join("crash-reports"));

    if let Ok(entries) = std::fs::read_dir(data_dir.join("versions")) {
        for entry in entries.flatten() {
            dirs.push(entry.path().join("crash-reports"));
        }
    }
    dirs
}

/// 处理崩溃分析相关路由
pub async fn handle(
    _app: &tauri::AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 手动分析日志文件 =====
        "POST /api/crash/analyze" => Some(handle_analyze(body)),

        // ===== 列出崩溃日志 =====
        "GET /api/crash/logs" => Some(handle_list_logs(params)),

        // ===== 读取崩溃日志内容 =====
        "GET /api/crash/log-content" => Some(handle_log_content(params)),

        // ===== 导出崩溃报告 =====
        "POST /api/crash/export" => Some(handle_export(body)),

        _ => None,
    }
}

/// POST /api/crash/analyze — 手动导入日志文件分析
/// 参数：filePath（必填，要分析的日志文件路径）
fn handle_analyze(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let file_path = data
        .get("filePath")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        return ApiResult::err(400, "缺少 filePath");
    }

    let path = PathBuf::from(file_path);
    let output = crash_analyzer::analyze_crash_file(&path);

    ApiResult::ok(json!({
        "success": true,
        "result": output.to_json()
    }))
}

/// GET /api/crash/logs — 列出崩溃日志
/// 参数：limit（可选，默认 20）
fn handle_list_logs(params: &Option<Value>) -> ApiResult {
    let limit = params
        .as_ref()
        .and_then(|p| p.get("limit"))
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let search_dirs = version_crash_search_dirs(version_id);

    let mut logs: Vec<Value> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if !name.ends_with(".txt") {
                        continue;
                    }
                    if seen_names.contains(name) {
                        continue;
                    }
                    seen_names.insert(name.to_string());

                    let path = entry.path();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    logs.push(json!({
                        "file": name,
                        "name": name,
                        "path": path.to_string_lossy(),
                        "size": size,
                        "modifiedTime": mtime,
                        "time": mtime
                    }));
                }
            }
        }
    }

    // 按修改时间倒序排序
    logs.sort_by(|a, b| {
        let a_time = a.get("modifiedTime").and_then(|v| v.as_u64()).unwrap_or(0);
        let b_time = b.get("modifiedTime").and_then(|v| v.as_u64()).unwrap_or(0);
        b_time.cmp(&a_time)
    });

    logs.truncate(limit);

    ApiResult::ok(json!({
        "logs": logs,
        "total": logs.len()
    }))
}

/// GET /api/crash/log-content — 读取崩溃日志内容
/// 参数：file（必填，文件名）或 path（必填，完整路径）
fn handle_log_content(params: &Option<Value>) -> ApiResult {
    let file_name = params
        .as_ref()
        .and_then(|p| p.get("file"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let direct_path = params
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_name.is_empty() && direct_path.is_empty() {
        return ApiResult::err(400, "缺少 file 或 path 参数");
    }

    // 路径白名单校验：只允许从已知 crash-reports 目录读取
    let target_path = if !direct_path.is_empty() {
        PathBuf::from(direct_path)
    } else {
        // 在已知目录中查找
        let mut search_dirs: Vec<PathBuf> = Vec::new();
        let settings = storage::load_settings();
        let game_dir = crate::utils::get_str(&settings, "gameDir");
        if !game_dir.is_empty() {
            search_dirs.push(PathBuf::from(&game_dir).join("crash-reports"));
        }
        search_dirs.push(storage::resolve_data_dir().join("crash-reports"));
        search_dirs.push(crash_analyzer::constants::default_minecraft_dir().join("crash-reports"));

        let mut found = None;
        for dir in &search_dirs {
            let candidate = dir.join(file_name);
            if candidate.exists() {
                found = Some(candidate);
                break;
            }
        }
        match found {
            Some(p) => p,
            None => return ApiResult::err(404, "文件不存在"),
        }
    };

    // 校验路径在白名单目录下（防止路径穿越）
    let canonical_target = match target_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return ApiResult::err(404, "文件不存在"),
    };

    let search_dirs = crash_whitelist_dirs();

    let mut in_whitelist = false;
    for dir in &search_dirs {
        if let Ok(canonical_dir) = dir.canonicalize() {
            if canonical_target.starts_with(&canonical_dir) {
                in_whitelist = true;
                break;
            }
        }
    }
    if !in_whitelist {
        return ApiResult::err(403, "路径不在允许的范围内");
    }

    let content = match std::fs::read_to_string(&canonical_target) {
        Ok(c) => c,
        Err(e) => return ApiResult::err(500, &format!("读取失败: {}", e)),
    };

    // 限制返回内容长度
    let truncated = if content.len() > 50000 {
        format!("{}...(内容已截断)", &content[..50000])
    } else {
        content
    };

    let file_name = canonical_target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    ApiResult::ok(json!({
        "content": truncated,
        "file": file_name,
        "path": canonical_target.to_string_lossy()
    }))
}

/// POST /api/crash/export — 导出崩溃报告
/// 参数：versionId（可选）、fileName（可选，指定单个文件）
/// 拼接 DATA_DIR/crash-reports/ 下所有 .txt 文件内容，写入临时文件后返回
fn handle_export(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let version_id = data.get("versionId").and_then(|v| v.as_str()).unwrap_or("");
    let file_name_filter = data.get("fileName").and_then(|v| v.as_str()).unwrap_or("");

    let data_dir = storage::resolve_data_dir();
    let settings = storage::load_settings();

    // 收集崩溃报告搜索目录（与 handle_list_logs 一致，按版本解析）
    let search_dirs = version_crash_search_dirs(version_id);

    // 收集文件（按文件名去重）
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name_os = entry.file_name();
                let Some(name) = name_os.to_str() else { continue };
                if !name.ends_with(".txt") {
                    continue;
                }
                if !file_name_filter.is_empty() && name != file_name_filter {
                    continue;
                }
                if seen.contains(name) {
                    continue;
                }
                seen.insert(name.to_string());
                files.push((name.to_string(), entry.path()));
            }
        }
    }

    // 拼接导出内容
    let mut parts: Vec<String> = Vec::new();
    parts.push("=".repeat(60));
    parts.push("VersePC 崩溃报告导出".to_string());
    parts.push(format!("导出时间: {}", crate::utils::now_iso()));
    parts.push(format!(
        "版本: {}",
        if version_id.is_empty() { "未知".to_string() } else { version_id.to_string() }
    ));
    parts.push("=".repeat(60));
    parts.push(String::new());

    parts.push("[环境信息]".to_string());
    parts.push(format!("数据目录: {}", data_dir.display()));
    let java_path = crate::utils::get_str(&settings, "javaPath");
    parts.push(format!(
        "Java路径: {}",
        if java_path.is_empty() { "自动检测".to_string() } else { java_path.clone() }
    ));
    let java_home = std::env::var("JAVA_HOME").unwrap_or_else(|_| "未设置".to_string());
    parts.push(format!("JAVA_HOME: {}", java_home));
    parts.push(String::new());

    if files.is_empty() {
        parts.push("未找到崩溃报告文件".to_string());
    } else {
        parts.push(format!("[崩溃报告] 共 {} 个文件", files.len()));
        parts.push("-".repeat(40));
        parts.push(String::new());
        // 按文件名排序
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in &files {
            parts.push(format!("=== {} ===", name));
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let truncated = if content.len() > 50000 {
                        let mut end = 50000;
                        while end > 0 && !content.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...(内容已截断)", &content[..end])
                    } else {
                        content
                    };
                    parts.push(truncated);
                }
                Err(e) => {
                    parts.push(format!("(读取失败: {})", e));
                }
            }
            parts.push(String::new());
        }
    }

    let export_content = parts.join("\n");
    let timestamp_raw = crate::utils::now_iso().replace([':', '.'], "-");
    let timestamp = &timestamp_raw[..timestamp_raw.len().min(19)];
    let file_name = format!("VersePC_Crash_{}.txt", timestamp);

    // 写入临时文件
    let temp_dir = data_dir.join("temp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let export_path = temp_dir.join(&file_name);
    let _ = std::fs::write(&export_path, &export_content);

    ApiResult::ok(json!({
        "success": true,
        "content": export_content,
        "fileName": file_name,
        "path": export_path.to_string_lossy()
    }))
}
