// api/game.rs — 游戏运行相关 API 路由
// 职责：处理 /api/game/* 路由
// 对应原项目 server/api/routes/game.js
//
// 路由清单：
//   GET  /api/game/status         返回正在运行的游戏实例列表
//   GET  /api/game/stop           停止指定/全部游戏实例
//   POST /api/game/stop           同上（POST 版本）
//   GET  /api/game/log            查询游戏日志
//   GET  /api/game/crash-log      获取最近一次崩溃报告
//   GET  /api/game/exit-analysis   返回上次游戏退出分析结果
//   GET  /api/game/play-time      统计存档游戏时间
//   GET  /api/game/diagnose       游戏诊断（系统/Java/账号/版本/依赖）
//
// 日志推送：前端不再需要 SSE，直接监听 Tauri 事件 "game-log" 即可拿到实时日志

use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;

use crate::api::ApiResult;
use crate::launch;
use crate::launch::game_session;
use crate::storage;
use crate::utils;

/// 处理游戏运行相关路由
pub fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 游戏状态 =====
        "GET /api/game/status" => Some(handle_game_status()),

        // ===== 停止游戏（GET/POST 共用） =====
        "GET /api/game/stop" => Some(handle_game_stop(params)),
        "POST /api/game/stop" => Some(handle_game_stop(body)),

        // ===== 游戏日志查询（轮询兜底） =====
        "GET /api/game/log" => Some(handle_game_log(params)),

        // ===== 上次退出分析 =====
        "GET /api/game/exit-analysis" => Some(handle_exit_analysis()),

        // ===== 崩溃报告 =====
        "GET /api/game/crash-log" => Some(handle_crash_log(params)),

        // ===== 崩溃分析（自动触发，从最近日志收集分析） =====
        "GET /api/game/crash-analyze" => Some(handle_crash_analyze(params)),

        // ===== 游戏时间统计 =====
        "GET /api/game/play-time" => Some(handle_play_time(params)),

        // ===== 日志导出 =====
        "GET /api/game/log/export" => Some(handle_log_export(params)),

        // ===== 诊断 =====
        "GET /api/game/diagnose" => Some(handle_diagnose(params)),

        _ => None,
    }
}

/// GET /api/game/status — 返回正在运行的游戏实例列表
fn handle_game_status() -> ApiResult {
    let instances = game_session::get_all_status();
    let running = !instances.is_empty();

    // 检测局域网联机端口：取第一个实例的 lanPort
    let lan_port = instances
        .iter()
        .filter_map(|inst| inst.get("lanPort").and_then(|v| v.as_u64()).and_then(|p| u16::try_from(p).ok()))
        .next();

    ApiResult::ok(json!({
        "running": running,
        "instances": instances,
        "lanPort": lan_port
    }))
}

/// GET/POST /api/game/stop — 停止游戏实例
/// 参数：sessionId（可选，不传则停止全部）
fn handle_game_stop(data: &Option<Value>) -> ApiResult {
    let session_id = data
        .as_ref()
        .and_then(|d| d.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !session_id.is_empty() {
        // 停止指定实例
        match launch::kill_game(session_id) {
            Ok(()) => ApiResult::ok(json!({
                "success": true,
                "message": "游戏实例已停止",
                "sessionId": session_id
            })),
            Err(e) => ApiResult::ok(json!({
                "success": false,
                "error": e
            })),
        }
    } else {
        // 停止全部实例
        let pids = launch::stop_all();
        if pids.is_empty() {
            ApiResult::ok(json!({
                "success": false,
                "error": "游戏未在运行"
            }))
        } else {
            ApiResult::ok(json!({
                "success": true,
                "message": format!("已停止 {} 个游戏实例", pids.len())
            }))
        }
    }
}

/// GET /api/game/log — 查询游戏日志
/// 参数：sessionId、count、offset
fn handle_game_log(params: &Option<Value>) -> ApiResult {
    let session_id = params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let count = params
        .as_ref()
        .and_then(|p| p.get("count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;
    let offset = params
        .as_ref()
        .and_then(|p| p.get("offset"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    if session_id.is_empty() {
        // 未指定 session：返回空（前端应使用事件流替代）
        return ApiResult::ok(json!({
            "lines": [],
            "total": 0
        }));
    }

    let logs = game_session::get_logs(session_id, count, offset);
    let total = logs.len();

    ApiResult::ok(json!({
        "lines": logs,
        "total": total,
        "sessionId": session_id
    }))
}

/// GET /api/game/exit-analysis — 返回上次游戏退出分析结果
/// 简化版：从 game_session 中读取最后退出实例的信息
fn handle_exit_analysis() -> ApiResult {
    // 退出分析当前由 process_manager 通过 "game-exit" 事件推送
    // 这里返回空，前端应监听事件
    ApiResult::ok(json!({
        "analysis": null,
        "message": "退出分析已通过 game-exit 事件推送"
    }))
}

/// GET /api/game/crash-log — 获取最近一次崩溃报告
/// 参数：versionId（可选）
fn handle_crash_log(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut search_dirs: Vec<PathBuf> = Vec::new();

    // 版本隔离目录
    if !version_id.is_empty() {
        let versions_dir = storage::resolve_data_dir().join("versions");
        let clean_id = clean_external_marker(version_id);
        search_dirs.push(versions_dir.join(&clean_id).join("crash-reports"));
    }

    // 全局游戏目录
    let settings = storage::load_settings();
    let game_dir = utils::get_str(&settings, "gameDir");
    if !game_dir.is_empty() {
        search_dirs.push(PathBuf::from(&game_dir).join("crash-reports"));
    } else {
        search_dirs.push(storage::resolve_data_dir().join("crash-reports"));
    }

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut crash_files: Vec<_> = entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|n| n.ends_with(".txt"))
                        .unwrap_or(false)
                })
                .collect();
            // 按修改时间倒序排序
            crash_files.sort_by(|a, b| {
                b.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .cmp(&a.metadata().and_then(|m| m.modified()).ok())
            });
            if let Some(file) = crash_files.first() {
                let path = file.path();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let truncated = if content.len() > 10000 {
                        content[..10000].to_string()
                    } else {
                        content
                    };
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    return ApiResult::ok(json!({
                        "crashLog": {
                            "file": file_name,
                            "content": truncated,
                            "path": path.to_string_lossy()
                        }
                    }));
                }
            }
        }
    }

    ApiResult::ok(json!({ "crashLog": null }))
}

/// GET /api/game/crash-analyze — 自动崩溃分析
/// 参数：versionId（可选，用于定位版本目录的日志）
/// 流程：调用 CrashAnalyzer.collect → prepare → analyze → 返回结构化结果
fn handle_crash_analyze(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // 确定 .minecraft 目录
    let settings = storage::load_settings();
    let game_dir = utils::get_str(&settings, "gameDir");
    let minecraft_dir = if !game_dir.is_empty() {
        std::path::PathBuf::from(&game_dir)
    } else {
        crate::crash_analyzer::constants::default_minecraft_dir()
    };

    // 清理外部版本标记
    let clean_id = clean_external_marker(version_id);

    let analyzer = crate::crash_analyzer::CrashAnalyzer::new(
        if clean_id.is_empty() { None } else { Some(clean_id.clone()) },
        Some(minecraft_dir.clone()),
    );

    let output = analyzer.analyze(&clean_id);

    // 如果分析到崩溃原因，取第一条作为主要结果
    let (found, reason_str, solution) = if let Some((reason, _)) = output.crash_reasons.first() {
        let _mod_name = output
            .crash_reasons
            .first()
            .and_then(|(_, additional)| additional.first())
            .cloned();
        (
            true,
            reason.as_str().to_string(),
            output.detail.clone(),
        )
    } else {
        (false, String::new(), String::new())
    };

    // 取主日志文件的摘录（用于前端展示）
    let log_excerpt: Vec<Value> = Vec::new();

    let mod_name = output
        .crash_reasons
        .first()
        .and_then(|(_, additional)| additional.first())
        .cloned();

    // 找到参与分析的日志文件路径
    let log_file = output.files.first().cloned().unwrap_or_default();

    // 严重程度评估
    let severity = output
        .crash_reasons
        .first()
        .map(|(reason, _)| {
            use crate::crash_analyzer::constants::CrashReason::*;
            match reason {
                // 严重：Mod 崩溃、内存不足、驱动崩溃等
                ModCrashed | OutOfMemory | IntelDriverCrash | AMDDriverCrash | NVidiaDriverCrash
                | PixelFormatNotAccelerated | NativeLinkError | InvalidPath => "high",
                // 中等：版本不兼容、缺失前置等
                JavaVersionTooHigh | JavaTooOld | ModRequiresJava11 | ModMissingDependency
                | ModIncompatible | ModDuplicateModFiles | ModIdConflict | ModMixinError
                | ModFileExtracted | ForgeMissing | ModLoaderVersionIncompatible => "medium",
                // 低：其他问题
                _ => "low",
            }
        })
        .unwrap_or("low")
        .to_string();

    let reason = if found {
        reason_str
    } else {
        "未分析出崩溃原因".to_string()
    };

    ApiResult::ok(json!({
        "found": found,
        "reason": reason,
        "solution": solution,
        "modName": mod_name,
        "logFile": log_file,
        "severity": severity,
        "logExcerpt": log_excerpt,
        "crashDescription": output.detail,
        "files": output.files,
        "crashReasons": output.crash_reasons.iter().map(|(r, a)| json!({
            "reason": r.as_str(),
            "additional": a
        })).collect::<Vec<_>>()
    }))
}

// ============== 辅助函数 ==============

/// 清理外部版本标记 "xxx [外部N]" → "xxx"
fn clean_external_marker(version_id: &str) -> String {
    if let Some(idx) = version_id.find(" [外部") {
        version_id[..idx].to_string()
    } else if let Some(idx) = version_id.find("[外部") {
        version_id[..idx].trim_end().to_string()
    } else {
        version_id.to_string()
    }
}

/// 解析版本目录：先扫外部文件夹，找不到则用 DATA_DIR/versions/clean_id
fn resolve_version_dir(clean_id: &str) -> PathBuf {
    let external_folders = storage::load_external_folders();
    for folder in external_folders {
        let p = utils::get_str(&folder, "path");
        if p.is_empty() {
            continue;
        }
        let folder_path = PathBuf::from(&p);
        if !folder_path.exists() {
            continue;
        }
        // 兼容两种结构：<folder>/versions/<clean_id> 与 <folder>/<clean_id>
        let candidate_a = folder_path.join("versions").join(clean_id);
        if candidate_a.exists() {
            return candidate_a;
        }
        let candidate_b = folder_path.join(clean_id);
        if candidate_b.exists() {
            return candidate_b;
        }
    }
    storage::resolve_data_dir().join("versions").join(clean_id)
}

/// 格式化游戏时长（秒 → "X小时Y分钟" / "Y分钟"）
fn format_play_time(total_seconds: f64) -> String {
    let secs = total_seconds.max(0.0) as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours >= 1 {
        format!("{}小时{}分钟", hours, minutes)
    } else {
        format!("{}分钟", minutes)
    }
}

/// 从 level.dat 字节流中解析第一个 NBT TAG_Long "Time" 字段的 ticks 值
/// NBT 结构：TAG_Long 类型=4，名称长度=2字节BigEndian，名称="Time"(4字节)，随后 8 字节 BigInt64BE
fn parse_level_dat_time(bytes: &[u8]) -> Option<i64> {
    let time_name = b"Time";
    let limit = if bytes.len() > 20 { bytes.len() - 20 } else { 0 };
    let mut i = 0;
    while i < limit {
        if bytes[i] == 4 && bytes[i + 1] == 0 && bytes[i + 2] == 4 {
            // 名称长度 = 4，后面 4 字节名称
            if &bytes[i + 3..i + 7] == time_name {
                // 读取后续 8 字节 i64 大端
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&bytes[i + 7..i + 15]);
                return Some(i64::from_be_bytes(buf));
            }
        }
        i += 1;
    }
    None
}

/// GET /api/game/play-time — 统计存档游戏时间与会话时长
/// 参数：versionId（必填）
fn handle_play_time(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let clean_id = clean_external_marker(version_id);
    let version_dir = resolve_version_dir(&clean_id);

    // 解析各存档的 level.dat 读取游戏时长
    let mut worlds: Vec<Value> = Vec::new();
    let saves_dir = version_dir.join("saves");
    if saves_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&saves_dir) {
            for entry in entries.flatten() {
                let save_path = entry.path();
                if !save_path.is_dir() {
                    continue;
                }
                let level_dat = save_path.join("level.dat");
                if !level_dat.exists() {
                    continue;
                }
                let save_name = entry
                    .file_name()
                    .to_str()
                    .unwrap_or("")
                    .to_string();

                let Ok(compressed) = std::fs::read(&level_dat) else { continue };
                let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
                let mut decompressed = Vec::new();
                if decoder.read_to_end(&mut decompressed).is_err() {
                    continue;
                }

                if let Some(ticks) = parse_level_dat_time(&decompressed) {
                    let seconds = ticks as f64 / 20.0;
                    worlds.push(json!({
                        "worldName": save_name,
                        "ticks": ticks,
                        "seconds": seconds,
                        "formatted": format_play_time(seconds)
                    }));
                }
            }
        }
    }

    // 读取会话累计游戏时间
    let play_time_path = storage::resolve_data_dir().join("play-time.json");
    let session_data: Value = std::fs::read_to_string(&play_time_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(json!({}));

    let version_session = session_data.get(version_id).cloned().unwrap_or(json!({}));
    let total_session_seconds = version_session
        .get("totalSeconds")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let last_played = version_session
        .get("lastPlayed")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let play_count = version_session
        .get("playCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    ApiResult::ok(json!({
        "worlds": worlds,
        "session": {
            "totalSeconds": total_session_seconds,
            "formatted": format_play_time(total_session_seconds),
            "lastPlayed": if last_played.is_empty() { Value::Null } else { Value::String(last_played) },
            "playCount": play_count
        }
    }))
}

/// GET /api/game/log/export — 导出环境信息+日志+崩溃报告为文本
/// 参数：versionId（可选）
/// 由于 ApiResult 当前只支持 JSON，返回 { success, content, fileName }，前端拿到内容后自行下载
fn handle_log_export(params: &Option<Value>) -> ApiResult {
    let export_version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut parts: Vec<String> = Vec::new();

    parts.push("=".repeat(60));
    parts.push("VersePC 游戏日志导出".to_string());
    parts.push(format!("导出时间: {}", utils::now_iso()));
    parts.push(format!("版本: {}", if export_version_id.is_empty() { "未知".to_string() } else { export_version_id.to_string() }));
    parts.push("=".repeat(60));
    parts.push(String::new());

    let settings = storage::load_settings();
    let data_dir = storage::resolve_data_dir();
    let java_dir = data_dir.join("java");

    parts.push("[环境信息]".to_string());
    parts.push(format!("数据目录: {}", data_dir.display()));
    parts.push(format!("JAVA_DIR: {}", java_dir.display()));
    let java_path = utils::get_str(&settings, "javaPath");
    parts.push(format!("Java路径: {}", if java_path.is_empty() { "自动检测".to_string() } else { java_path.clone() }));
    if !java_path.is_empty() && std::path::Path::new(&java_path).exists() {
        // 输出 Java 路径版本信息（如果能从 detect_all 中匹配）
        let java_list = crate::java::detect_all();
        if let Some(j) = java_list.iter().find(|v| utils::get_str(v, "path") == java_path) {
            let ver = utils::get_str(j, "version");
            let major = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0);
            parts.push(format!("Java路径版本: {} (major={})", ver, major));
        }
    }
    let java_home = std::env::var("JAVA_HOME").unwrap_or_else(|_| "未设置".to_string());
    parts.push(format!("JAVA_HOME: {}", java_home));
    let max_memory = settings.get("maxMemory").and_then(|v| v.as_u64()).unwrap_or(2048);
    parts.push(format!("最大内存: {}MB", max_memory));
    let version_isolation = settings.get("versionIsolation").and_then(|v| v.as_bool()).unwrap_or(false);
    parts.push(format!("版本隔离: {}", if version_isolation { "是" } else { "否" }));
    parts.push(String::new());

    parts.push("[Java检测]".to_string());
    let java_list = crate::java::detect_all();
    let sys_count = java_list.iter().filter(|v| utils::get_str(v, "source") == "system").count();
    let bundled_count = java_list.iter().filter(|v| utils::get_str(v, "source") == "bundled").count();
    parts.push(format!("系统Java: {}个", sys_count));
    for j in &java_list {
        if utils::get_str(j, "source") != "system" {
            continue;
        }
        parts.push(format!(
            "  - {} (版本={}, major={}, 来源={})",
            utils::get_str(j, "path"),
            utils::get_str(j, "version"),
            j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0),
            utils::get_str(j, "source")
        ));
    }
    parts.push(format!("内置Java: {}个", bundled_count));
    for j in &java_list {
        if utils::get_str(j, "source") != "bundled" {
            continue;
        }
        parts.push(format!(
            "  - {} (版本={}, major={}, 来源={})",
            utils::get_str(j, "path"),
            utils::get_str(j, "version"),
            j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0),
            utils::get_str(j, "source")
        ));
    }
    if java_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&java_dir) {
            let names: Vec<String> = entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect();
            parts.push(format!("JAVA_DIR内容: {}", names.join(", ")));
        }
    }
    let minecraft_dir = if !utils::get_str(&settings, "gameDir").is_empty() {
        PathBuf::from(utils::get_str(&settings, "gameDir"))
    } else {
        data_dir.clone()
    };
    let mc_runtime = minecraft_dir.join("runtime");
    if mc_runtime.exists() {
        if let Ok(entries) = std::fs::read_dir(&mc_runtime) {
            let names: Vec<String> = entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect();
            parts.push(format!(".minecraft/runtime内容: {}", names.join(", ")));
        }
    }
    parts.push(String::new());

    // 上次退出分析：Rust 项目通过 game-exit 事件推送，无全局缓存，跳过

    // 游戏日志缓冲区：取最近运行实例的日志（最多 2000 行）
    let instances = game_session::get_all_status();
    if !instances.is_empty() {
        if let Some(inst) = instances.first() {
            let session_id = utils::get_str(inst, "sessionId");
            if !session_id.is_empty() {
                let logs = game_session::get_logs(&session_id, 2000, 0);
                if !logs.is_empty() {
                    parts.push(format!("[游戏日志] (最近 {} 行)", logs.len()));
                    parts.push("-".repeat(40));
                    for line in &logs {
                        parts.push(line.clone());
                    }
                    parts.push(String::new());
                }
            }
        }
    }

    // 崩溃报告与 latest.log（如有 versionId）
    if !export_version_id.is_empty() {
        let clean_id = clean_external_marker(export_version_id);
        let version_dir = resolve_version_dir(&clean_id);

        let crash_dir = version_dir.join("crash-reports");
        if crash_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&crash_dir) {
                let mut crash_files: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                    .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
                    .collect();
                crash_files.sort();
                crash_files.reverse();
                if let Some(latest) = crash_files.first() {
                    let crash_path = crash_dir.join(latest);
                    if let Ok(content) = std::fs::read_to_string(&crash_path) {
                        parts.push(format!("[最新崩溃报告] {}", latest));
                        parts.push("-".repeat(40));
                        let truncated = if content.len() > 5000 {
                            content[..5000].to_string()
                        } else {
                            content.clone()
                        };
                        parts.push(truncated);
                        if content.len() > 5000 {
                            parts.push(format!("... (已截断，共{}字符)", content.len()));
                        }
                        parts.push(String::new());
                    }
                }
            }
        }

        let latest_log = version_dir.join("logs").join("latest.log");
        if latest_log.exists() {
            if let Ok(content) = std::fs::read_to_string(&latest_log) {
                parts.push("[latest.log] (最后 2000 行)".to_string());
                parts.push("-".repeat(40));
                let lines: Vec<&str> = content.split('\n').collect();
                let start = if lines.len() > 2000 { lines.len() - 2000 } else { 0 };
                for line in &lines[start..] {
                    parts.push(line.to_string());
                }
                parts.push(String::new());
            }
        }
    }

    let export_content = parts.join("\n");
    let timestamp = utils::now_iso().replace([':', '.'], "-");
    let file_name = format!("VersePC_Log_{}.txt", &timestamp[..timestamp.len().min(19)]);

    // 写入临时文件（与原项目一致），随后返回内容
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

/// GET /api/game/diagnose — 游戏诊断
/// 参数：versionId（可选）、sessionId（可选）
/// 返回系统信息、Java 列表、当前账号、版本信息、依赖检查、运行中实例
fn handle_diagnose(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session_id = params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // 系统信息
    let (total_bytes, avail_bytes) = crate::api::system::get_system_memory_kb();
    let total_mb = total_bytes / 1024 / 1024;
    let avail_mb = avail_bytes / 1024 / 1024;
    let system = json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "totalMemoryMB": total_mb,
        "availableMemoryMB": avail_mb
    });

    // Java 检测
    let java_list = crate::java::detect_all();
    let java = json!({
        "count": java_list.len(),
        "list": java_list
    });

    // 当前账号
    let settings = storage::load_settings();
    let accounts = storage::load_accounts();
    let selected_account_id = utils::get_str(&settings, "selectedAccount");
    let selected_account = accounts
        .as_array()
        .and_then(|arr| arr.iter().find(|a| utils::get_str(a, "id") == selected_account_id))
        .cloned()
        .unwrap_or(Value::Null);
    let account = json!({
        "selected": selected_account,
        "totalCount": accounts.as_array().map(|a| a.len()).unwrap_or(0)
    });

    // 版本信息与依赖检查
    let (version_info, deps) = if !version_id.is_empty() {
        let clean_id = clean_external_marker(version_id);
        let is_external = version_id.contains(" [外部");
        let version_dir = resolve_version_dir(&clean_id);
        let external_path = if is_external {
            Some(version_dir.as_path())
        } else {
            None
        };

        // 读取版本 JSON
        let version_json_path =
            crate::launch::dep_check::resolve_version_json(&clean_id, external_path);
        let version_json: Value = match &version_json_path {
            Some(p) => std::fs::read_to_string(p)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null),
            None => Value::Null,
        };

        let main_class = utils::get_str(&version_json, "mainClass");
        let inherits_from = utils::get_str(&version_json, "inheritsFrom");
        let libraries_count = version_json
            .get("libraries")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let version_info = json!({
            "versionId": clean_id,
            "isExternal": is_external,
            "mainClass": main_class,
            "inheritsFrom": inherits_from,
            "librariesCount": libraries_count,
            "jsonPath": version_json_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            "jsonExists": version_json_path.is_some()
        });

        // 依赖检查
        let dep_result =
            crate::launch::dep_check::check_dependencies(&clean_id, &settings, external_path);
        let deps = dep_result.to_json();

        (version_info, deps)
    } else {
        (Value::Null, Value::Null)
    };

    // 运行中游戏实例
    let instances = game_session::get_all_status();
    let running_count = instances.len();
    let (current_session, session_logs) = if !session_id.is_empty() {
        let inst = instances
            .iter()
            .find(|i| utils::get_str(i, "sessionId") == session_id)
            .cloned()
            .unwrap_or(Value::Null);
        let logs = game_session::get_logs(session_id, 100, 0);
        (
            inst,
            json!({
                "total": logs.len(),
                "lines": logs
            }),
        )
    } else {
        (Value::Null, Value::Null)
    };
    let running = json!({
        "count": running_count,
        "instances": instances,
        "currentSession": current_session,
        "sessionLogs": session_logs
    });

    ApiResult::ok(json!({
        "success": true,
        "diagnosis": {
            "system": system,
            "java": java,
            "account": account,
            "version": version_info,
            "deps": deps,
            "running": running
        }
    }))
}
