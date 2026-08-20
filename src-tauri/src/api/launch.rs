// api/launch.rs — 启动相关 API 路由
// 职责：处理 /api/launch/* 路由
// 对应原项目 server/api/routes/launch.js
//
// 路由清单：
//   POST /api/launch              启动游戏（含启动设置读取、依赖检查、调用 do_launch）
//   POST /api/launch/cancel       取消启动并清理游戏实例
//   POST /api/launch/check        检查版本依赖完整性
//   POST /api/launch/args-preview 预览启动参数（不实际启动）
//   POST /api/launch/download-deps 下载缺失依赖文件（同步下载，进度通过事件推送）
//   GET  /api/launch/session-status 查询下载会话状态（占位）
//   GET  /api/launch/diagnose     诊断启动配置（占位）
//
// 简化策略（保持架构合理）：
//   - 微软 Token 刷新、Forge 修复链等复杂子流程未迁移，先返回失败提示
//   - 下载会话机制简化：POST /api/launch/download-deps 同步下载所有缺失文件，
//     进度通过 'launch-download-progress' 事件推送，前端可监听
//   - GET /api/launch/session-status 保留占位，前端改为监听事件

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::launch::{self, dep_check};
use crate::storage;
use crate::utils;

use serde::{Deserialize, Serialize};

// ============== 启动/下载会话：launchSessions ==============
// 对齐原 electron 项目 ctx.sessions.launchSessions
// 支持前端轮询 GET /api/launch/session-status，避免 download-deps HTTP 超时
#[derive(Default, Clone)]
struct LaunchSession {
    status: String,           // downloading / completed / failed / launched / launch_failed
    progress: u32,            // 0~100
    message: String,
    total_files: u32,
    completed_files: u32,
    current_file: String,
    errors: Vec<String>,
    version_id: String,
    last_activity: u64,       // unix ms
    speed: u64,               // 预估 B/s（简化版留 0）
    completed: u32,           // 成功数
    failed: u32,              // 失败数
    queued: u32,
    concurrent_downloads: u32,
    failed_files: Vec<String>,
    active_downloads: Vec<String>,
    launch_result: Option<Value>,
}

/// 并发下载统计
#[derive(Clone, Default)]
struct LaunchDlStats {
    completed: u32,
    failed: u32,
    failed_files: Vec<String>,
    errors: Vec<String>,
}

impl LaunchSession {
    fn to_json(&self) -> Value {
        json!({
            "status": self.status,
            "progress": self.progress,
            "message": self.message,
            "currentFile": self.current_file,
            "totalFiles": self.total_files,
            "completedFiles": self.completed_files,
            "errors": self.errors,
            "launchResult": self.launch_result,
            "activeDownloads": self.active_downloads,
            "completed": self.completed,
            "failed": self.failed,
            "speed": self.speed,
            "queued": self.queued,
            "concurrentDownloads": self.concurrent_downloads,
            "failedFiles": self.failed_files,
        })
    }
}

static LAUNCH_SESSIONS: OnceLock<Mutex<HashMap<String, LaunchSession>>> = OnceLock::new();

fn sessions() -> std::sync::MutexGuard<'static, HashMap<String, LaunchSession>> {
    let mutex = LAUNCH_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    mutex.lock().unwrap()
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 启动锁：防止重复点击导致启动多个游戏实例
/// 30 秒后自动释放（与原项目行为一致）
static LAUNCH_LOCK: Mutex<Option<Instant>> = Mutex::new(None);

fn try_acquire_launch_lock() -> bool {
    let mut guard = LAUNCH_LOCK.lock().unwrap();
    if let Some(t) = *guard {
        if t.elapsed().as_secs() < 30 {
            return false;
        }
    }
    *guard = Some(Instant::now());
    true
}

fn release_launch_lock() {
    *LAUNCH_LOCK.lock().unwrap() = None;
}

/// RAII 守卫：作用域结束时自动释放启动锁
struct LaunchGuard {
    armed: bool,
}

impl LaunchGuard {
    fn acquire() -> Option<Self> {
        if try_acquire_launch_lock() {
            Some(Self { armed: true })
        } else {
            None
        }
    }
}

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        if self.armed {
            release_launch_lock();
        }
    }
}

/// 处理启动相关路由
/// 返回 Some(ApiResult) 表示已处理，None 表示不匹配
pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== POST /api/launch — 启动游戏主入口 =====
        "POST /api/launch" => Some(handle_launch(app, body).await),

        // ===== POST /api/launch/cancel — 取消启动 =====
        "POST /api/launch/cancel" => Some(handle_launch_cancel()),

        // ===== POST /api/launch/check — 检查版本依赖完整性 =====
        "POST /api/launch/check" => Some(handle_launch_check(params, body)),

        // ===== POST /api/launch/args-preview — 预览启动参数 =====
        "POST /api/launch/args-preview" => Some(handle_args_preview(body)),

        // ===== POST /api/launch/download-deps — 下载缺失依赖文件 =====
        "POST /api/launch/download-deps" => Some(handle_download_deps(app, body).await),

        // ===== GET /api/launch/session-status — 查询下载/启动会话进度 ==============
        "GET /api/launch/session-status" => {
            let sid = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if sid.is_empty() {
                return Some(ApiResult::ok(json!({
                    "status": "unknown",
                    "progress": 0,
                    "message": ""
                })));
            }
            let sess = {
                let s = sessions();
                s.get(sid).cloned()
            };
            match sess {
                Some(s) => {
                    let status_end = matches!(s.status.as_str(), "launched" | "launch_failed" | "failed" | "completed");
                    // 已结束的会话 60 秒后清理（对齐 electron）
                    if status_end {
                        let sid_owned = sid.to_string();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(60));
                            let mut s = sessions();
                            let _ = s.remove(&sid_owned);
                        });
                    }
                    Some(ApiResult::ok(s.to_json()))
                }
                None => Some(ApiResult::ok(json!({
                    "status": "unknown",
                    "progress": 0,
                    "message": ""
                }))),
            }
        }

        // ===== GET /api/launch/diagnose — 诊断启动配置（简化版） =====
        "GET /api/launch/diagnose" => Some(handle_diagnose(params)),

        _ => None,
    }
}

/// POST /api/launch — 启动游戏
/// 流程：
///   1. 解析请求参数（versionId、checkOnly）
///   2. 加载设置（全局 + 版本级）
///   3. 加载账号列表，选择当前账号
///   4. 解析版本 JSON
///   5. 调用 do_launch 启动进程
async fn handle_launch(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId").to_string();
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let check_only = data.get("checkOnly").and_then(|v| v.as_bool()).unwrap_or(false);

    // 仅校验模式不需要加锁；非校验模式必须持有锁才能继续
    let _lock_guard = if !check_only {
        match LaunchGuard::acquire() {
            Some(g) => g,
            None => {
                return ApiResult::ok(json!({
                    "success": false,
                    "error": "正在启动中，请稍候"
                }));
            }
        }
    } else {
        LaunchGuard { armed: false }
    };

    // 清理外部版本标记
    let clean_id = clean_external_marker(&version_id);

    // 加载设置并应用前端 store 中的启动设置覆盖
    let mut settings = storage::load_settings();
    apply_launch_settings_overrides(&mut settings);

    // 应用版本级设置覆盖（customInfo、windowTitle、fullscreen、resolution、memory）
    apply_version_settings(&mut settings, &clean_id);

    // 前端启动时直接传入的分辨率（如 "1920x1080" 或 "854x480"）优先，
    // 确保用户当前在"启动设置"里选择的窗口大小一定生效，不依赖 store 往返。
    let req_res = utils::get_str(&data, "resolution");
    if !req_res.is_empty() {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("resolution".to_string(), json!(req_res));
        }
    }

    // 加载账号列表
    let accounts = storage::load_accounts();
    let accounts_arr = accounts.as_array().cloned().unwrap_or_default();
    if accounts_arr.is_empty() {
        return ApiResult::ok(json!({
            "success": false,
            "error": "未登录，请先添加账户后再启动游戏。"
        }));
    }
    let selected_account = utils::get_str(&settings, "selectedAccount");
    let account = accounts_arr
        .iter()
        .find(|a| utils::get_str(a, "id") == selected_account)
        .or(accounts_arr.first())
        .cloned()
        .unwrap_or(Value::Null);

    // 解析版本 JSON
    let external_version_dir = resolve_external_version_dir(&version_id);
    let external_path = external_version_dir
        .as_ref()
        .map(PathBuf::from);

    let version_json = match dep_check::merge_version_json_chain(&clean_id, external_path.as_deref()) {
        Some(v) => v,
        None => {
            return ApiResult::ok(json!({
                "success": false,
                "error": format!("找不到或无法解析版本 {} 的 JSON 文件", version_id)
            }));
        }
    };

    // 依赖完整性检查
    // 这是纯同步重活（对每个 library/native/asset 逐一下文件系统 exists），
    // 挪到阻塞线程池执行，避免占住 async 运行时导致启动流程卡顿
    let dep_check_result = {
        let clean_id = clean_id.clone();
        let settings = settings.clone();
        let external_path = external_path.clone();
        match tokio::task::spawn_blocking(move || {
            dep_check::check_dependencies(&clean_id, &settings, external_path.as_deref())
        })
        .await
        {
            Ok(r) => r,
            Err(_) => {
                return ApiResult::ok(json!({
                    "success": false,
                    "error": "依赖检查任务异常，请重试"
                }));
            }
        }
    };

    if !dep_check_result.java.ok {
        return ApiResult::ok(json!({
            "success": false,
            "error": dep_check_result.java.message,
            "needDownload": false,
            "depCheck": dep_check_result.to_json()
        }));
    }

    if !dep_check_result.version_json.ok {
        return ApiResult::ok(json!({
            "success": false,
            "error": dep_check_result.version_json.message,
            "needDownload": false,
            "depCheck": dep_check_result.to_json()
        }));
    }

    if !dep_check_result.parent_version.ok {
        return ApiResult::ok(json!({
            "success": false,
            "error": dep_check_result.parent_version.message,
            "needDownload": true,
            "depCheck": dep_check_result.to_json()
        }));
    }

    // 仅校验模式：返回就绪状态
    if check_only {
        return ApiResult::ok(json!({
            "success": true,
            "ready": dep_check_result.ready,
            "message": "所有文件就绪，可以启动",
            "depCheck": dep_check_result.to_json()
        }));
    }

    // 缺失文件提示（简化版：直接返回失败让前端走修复流程）
    if !dep_check_result.missing_files.is_empty() {
        return ApiResult::ok(json!({
            "success": false,
            "error": format!("有 {} 个文件缺失，请使用文件修复或重新下载", dep_check_result.missing_files.len()),
            "needDownload": true,
            "depCheck": dep_check_result.to_json()
        }));
    }

    // 调用 do_launch 启动游戏
    match launch::do_launch(
        app.clone(),
        clean_id.clone(),
        version_json,
        settings,
        account,
        None, // custom_game_dir，由 args_builder 内部决策
        external_version_dir,
    )
    .await
    {
        Ok(session_id) => {
            // 记录游戏启动时间到 play-time.json
            record_launch_time(&clean_id);
            ApiResult::ok(json!({
                "success": true,
                "sessionId": session_id,
                "message": "游戏已启动"
            }))
        }
        Err(e) => {
            // 写入启动失败日志，便于事后排查
            write_launch_fail_log(&clean_id, &e);
            ApiResult::ok(json!({
                "success": false,
                "error": e
            }))
        }
    }
}

/// 写入启动失败日志到 DATA_DIR/logs/launch-fail-{timestamp}.json
fn write_launch_fail_log(version_id: &str, error: &str) {
    let data_dir = storage::resolve_data_dir();
    let logs_dir = data_dir.join("logs");
    if std::fs::create_dir_all(&logs_dir).is_err() {
        return;
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let log_file = logs_dir.join(format!("launch-fail-{}.json", timestamp));
    let log_content = json!({
        "versionId": version_id,
        "error": error,
        "timestamp": format!("{}", timestamp)
    });
    let _ = std::fs::write(&log_file, log_content.to_string());
}

/// POST /api/launch/cancel — 取消启动并清理游戏实例
fn handle_launch_cancel() -> ApiResult {
    // 终止所有运行中的游戏实例
    let _pids = launch::stop_all();
    ApiResult::ok(json!({
        "success": true,
        "message": "启动已取消"
    }))
}

/// POST /api/launch/check — 检查版本依赖完整性
fn handle_launch_check(_params: &Option<Value>, body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId").to_string();
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let clean_id = clean_external_marker(&version_id);
    let settings = storage::load_settings();
    let external_dir = resolve_external_version_dir(&version_id);
    let external_path = external_dir.as_ref().map(PathBuf::from);

    let dep_result = dep_check::check_dependencies(&clean_id, &settings, external_path.as_deref());

    // 与原 Electron 后端保持一致：把 dep_result 的字段展开到顶层
    // 前端直接用 depCheck.java / depCheck.maxVersion 等访问
    let mut body = serde_json::Map::new();
    body.insert("success".to_string(), Value::Bool(true));
    if let Value::Object(map) = dep_result.to_json() {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    ApiResult::ok(Value::Object(body))
}

/// POST /api/launch/args-preview — 预览启动参数（不实际启动）
fn handle_args_preview(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId").to_string();
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let clean_id = clean_external_marker(&version_id);
    let settings = storage::load_settings();

    // 解析版本 JSON
    let external_dir = resolve_external_version_dir(&version_id);
    let external_path = external_dir.as_ref().map(PathBuf::from);
    let version_json = match dep_check::merge_version_json_chain(&clean_id, external_path.as_deref()) {
        Some(v) => v,
        None => return ApiResult::err(400, "版本 JSON 缺失或损坏"),
    };

    // 使用默认离线账号预览（不依赖账号存在）
    let account = json!({
        "username": "Player",
        "uuid": "",
        "accessToken": "",
        "type": "offline"
    });

    let launch_args = launch::build_launch_arguments(
        &version_json,
        &settings,
        &account,
        &clean_id,
        None,
        external_path.as_deref(),
    );

    let java_path = dep_check::select_java_for_version(&clean_id, &settings, &version_json);

    ApiResult::ok(json!({
        "args": launch_args.args,
        "maxMemMB": launch_args.max_mem_mb,
        "javaPath": java_path
    }))
}

/// GET /api/launch/diagnose — 诊断启动配置
/// 构建 classpath、检查缺失库、查找主 jar、预览启动参数
fn handle_diagnose(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let clean_id = clean_external_marker(version_id);
    let external_dir = resolve_external_version_dir(version_id);
    let external_path = external_dir.as_ref().map(PathBuf::from);

    let version_json = match dep_check::merge_version_json_chain(&clean_id, external_path.as_deref()) {
        Some(v) => v,
        None => return ApiResult::err(400, "版本 JSON 缺失或损坏"),
    };

    let settings = storage::load_settings();
    let java_path = dep_check::select_java_for_version(&clean_id, &settings, &version_json);

    // 主类 / inheritsFrom / libraries 数量
    let main_class = utils::get_str(&version_json, "mainClass");
    let inherits_from = utils::get_str(&version_json, "inheritsFrom");
    let libraries_count = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // 构建 classpath 并检查每个条目是否存在
    let data_dir = storage::resolve_data_dir();
    let libraries_dir = data_dir.join("libraries");
    let classpath_entries =
        launch::args_builder::build_classpath(&version_json, external_path.as_deref(), &libraries_dir);

    let mut missing_libraries: Vec<String> = Vec::new();
    let mut critical_missing: Vec<String> = Vec::new();
    let critical_keywords = [
        "securejarhandler",
        "forge",
        "neoforge",
        "fmlloader",
        "modlauncher",
        "fabric-loader",
        "launchwrapper",
        "log4j",
        "lwjgl",
    ];

    for entry in &classpath_entries {
        let path = std::path::Path::new(entry);
        if !path.exists() {
            missing_libraries.push(entry.clone());
            let basename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if critical_keywords.iter().any(|kw| basename.contains(kw)) {
                critical_missing.push(entry.clone());
            }
        }
    }

    // 查找主 jar 文件（使用 dep_check::find_main_jar 确保继承链搜索）
    let main_jar_found = dep_check::find_main_jar(&version_json, &clean_id, external_path.as_deref())
        .map(|p| p.exists())
        .unwrap_or(false);
    let main_jar_path = dep_check::find_main_jar(&version_json, &clean_id, external_path.as_deref())
        .map(|p| p.to_string_lossy().to_string());

    // 选择当前账号（找不到则用默认离线账号）
    let accounts = storage::load_accounts();
    let selected_account_id = utils::get_str(&settings, "selectedAccount").to_string();
    let account = accounts
        .get("accounts")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|a| utils::get_str(a, "id") == selected_account_id)
                .cloned()
        })
        .or_else(|| {
            accounts
                .get("accounts")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first().cloned())
        })
        .unwrap_or_else(|| {
            json!({
                "username": "Player",
                "uuid": "",
                "accessToken": "",
                "type": "offline"
            })
        });

    // 预览启动参数并估算命令行长度
    let launch_args = launch::build_launch_arguments(
        &version_json,
        &settings,
        &account,
        &clean_id,
        None,
        external_path.as_deref(),
    );
    let args_count = launch_args.args.len();
    let cmd_len = java_path.len() + launch_args.args.iter().map(|a| a.len() + 3).sum::<usize>();

    let result = json!({
        "versionId": clean_id,
        "externalDir": external_dir,
        "mainClass": main_class,
        "inheritsFrom": inherits_from,
        "librariesCount": libraries_count,
        "javaPath": java_path,
        "classpathEntries": classpath_entries,
        "missingLibraries": missing_libraries,
        "criticalMissing": critical_missing,
        "mainJarFound": main_jar_found,
        "mainJarPath": main_jar_path,
        "argsPreview": launch_args.args,
        "argsCount": args_count,
        "estimatedCmdLength": cmd_len
    });

    ApiResult::ok(result)
}

// ============== 辅助函数 ==============

/// 清理外部版本标记 "xxx [外部N]" → "xxx"
fn clean_external_marker(version_id: &str) -> String {
    // 简化实现：去除 " [外部N]" 后缀
    if let Some(idx) = version_id.find(" [外部") {
        version_id[..idx].to_string()
    } else if let Some(idx) = version_id.find("[外部") {
        version_id[..idx].trim_end().to_string()
    } else {
        version_id.to_string()
    }
}

/// 解析外部版本目录
/// 对应原项目 server/versions/version-dir.js:resolveExternalVersionDir
/// 简化版：从 store 中读取 externalVersionFolders，匹配版本 ID
fn resolve_external_version_dir(version_id: &str) -> Option<String> {
    // 从 external-folders.json 读取外部版本目录列表
    // 注意：外部版本扫描（versions.rs scan_external_versions）用的也是这份配置，
    // 两者必须一致，否则外部版本会被误当成本地版本处理
    let folders = storage::load_external_folders();

    let clean_id = clean_external_marker(version_id);

    for folder in &folders {
        let path_str = utils::get_str(folder, "path");
        if path_str.is_empty() {
            continue;
        }
        let path = std::path::PathBuf::from(&path_str);
        if !path.exists() {
            continue;
        }
        // 检查 versions/<clean_id> 子目录
        let ver_dir = path.join("versions").join(&clean_id);
        if ver_dir.is_dir() {
            return Some(ver_dir.to_string_lossy().to_string());
        }
        // 检查直接以版本 ID 命名的子目录
        let direct_dir = path.join(&clean_id);
        if direct_dir.is_dir() {
            return Some(direct_dir.to_string_lossy().to_string());
        }
    }
    None
}

/// 应用前端 store 中的启动设置覆盖（窗口大小、全屏、自定义信息、窗口标题）
/// 对应原项目 server/api/routes/launch.js 中读取 app-store.json 的逻辑
fn apply_launch_settings_overrides(settings: &mut Value) {
    let data_dir = storage::resolve_data_dir();
    // 前端设置通过 store_get/set 写入 store.json（见 storage.rs Store::new）
    let store_file = data_dir.join("store.json");
    let store: Value = match std::fs::read_to_string(&store_file) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return,
        },
        Err(_) => return,
    };

    if let Some(launch_str) = store.get("versepc_launch_settings").and_then(|v| v.as_str()) {
        if let Ok(ls) = serde_json::from_str::<Value>(launch_str) {
            if let Some(obj) = settings.as_object_mut() {
                if let Some(window_size) = ls.get("windowSize").and_then(|v| v.as_str()) {
                    let res = if window_size == "default" {
                        "854x480".to_string()
                    } else {
                        window_size.to_string()
                    };
                    obj.insert("resolution".to_string(), json!(res));
                }
                if let Some(fullscreen) = ls.get("fullscreen").and_then(|v| v.as_bool()) {
                    obj.insert("fullscreen".to_string(), json!(fullscreen));
                }
                if let Some(custom_info) = ls.get("customInfo").and_then(|v| v.as_str()) {
                    if !custom_info.is_empty() {
                        obj.insert("customInfo".to_string(), json!(custom_info));
                    }
                }
                if let Some(window_title) = ls.get("windowTitle").and_then(|v| v.as_str()) {
                    if !window_title.is_empty() {
                        obj.insert("windowTitle".to_string(), json!(window_title));
                    }
                }
                // 全局自定义 JVM 参数：前端存 key 为 jvmArgs，启动参数构建读取 javaArgs，需映射
                if let Some(jvm_args) = ls.get("jvmArgs").and_then(|v| v.as_str()) {
                    if !jvm_args.trim().is_empty() {
                        obj.insert("javaArgs".to_string(), json!(jvm_args));
                    }
                }
            }
        }
    }
}

/// 应用版本级设置覆盖（customInfo、windowTitle、fullscreen、resolution、memory）
/// 对应原项目 server/api/routes/launch.js 中读取版本设置的逻辑
fn apply_version_settings(settings: &mut Value, version_id: &str) {
    let ver_settings = storage::load_version_settings(version_id, false);

    if let Some(obj) = settings.as_object_mut() {
        if let Some(custom_info) = ver_settings.get("customInfo").and_then(|v| v.as_str()) {
            if !custom_info.is_empty() {
                obj.insert("customInfo".to_string(), json!(custom_info));
            }
        }
        if let Some(window_title) = ver_settings.get("windowTitle").and_then(|v| v.as_str()) {
            if !window_title.is_empty() {
                obj.insert("windowTitle".to_string(), json!(window_title));
            }
        }
        if let Some(fullscreen) = ver_settings.get("fullscreen").and_then(|v| v.as_str()) {
            if fullscreen != "global" {
                let fs = fullscreen == "true" || fullscreen == "on";
                obj.insert("fullscreen".to_string(), json!(fs));
            }
        }
        if let Some(resolution) = ver_settings.get("resolution").and_then(|v| v.as_str()) {
            if !resolution.is_empty() {
                obj.insert("resolution".to_string(), json!(resolution));
            }
        }
        // 版本级内存设置优先级最高
        if let Some(memory_mode) = ver_settings.get("memoryMode").and_then(|v| v.as_str()) {
            if memory_mode == "custom" {
                if let Some(memory_value) = ver_settings.get("memoryValue").and_then(|v| v.as_u64()) {
                    obj.insert("memoryMode".to_string(), json!("custom"));
                    obj.insert("memoryValue".to_string(), json!(memory_value));
                }
            } else if memory_mode == "auto" {
                obj.insert("memoryMode".to_string(), json!("auto"));
                obj.remove("memoryValue");
            }
        }
    }
}

/// 记录游戏启动时间到 play-time.json
fn record_launch_time(version_id: &str) {
    let data_dir = storage::resolve_data_dir();
    let play_time_path = data_dir.join("play-time.json");

    let mut pt_data: Value = std::fs::read_to_string(&play_time_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or(json!({}));

    if let Some(obj) = pt_data.as_object_mut() {
        let entry = obj
            .entry(version_id.to_string())
            .or_insert_with(|| json!({ "totalSeconds": 0, "playCount": 0, "lastPlayed": null }));
        if let Some(entry_obj) = entry.as_object_mut() {
            *entry_obj.get_mut("lastPlayed").unwrap_or(&mut Value::Null) =
                json!(utils::now_iso());
            let play_count = entry_obj
                .get("playCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            *entry_obj.get_mut("playCount").unwrap_or(&mut Value::Null) =
                json!(play_count + 1);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            *entry_obj.get_mut("_launchTime").unwrap_or(&mut Value::Null) =
                json!(now_ms);
        }
    }

    let _ = std::fs::write(&play_time_path, pt_data.to_string());
}

/// POST /api/launch/download-deps — 下载缺失依赖文件
///
/// 请求体：{ versionId, sessionId? }
///
/// 流程：
/// 1. 调用 dep_check::check_dependencies 获取缺失文件列表
/// 2. 若无缺失，立即返回
/// 3. 逐个下载缺失文件，通过 'launch-download-progress' 事件推送进度
/// 4. 返回 { success, completed, failed, failedFiles }
///
/// 简化策略：同步下载，不并发（原项目支持并发下载，此处先做可用版本）
/// POST /api/launch/download-deps — 下载缺失依赖文件（对齐 electron 异步会话版）
///
/// 请求体：{ versionId, sessionId? }
///
/// 返回：{ success: true, sessionId, missingCount }（立即返回，不等待下载）
/// 之后前端通过 GET /api/launch/session-status?sessionId=X 轮询进度。
/// 同时也会通过 Tauri 事件 launch-download-progress 推送进度（保留向后兼容）。
async fn handle_download_deps(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    use tauri::Emitter;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId").to_string();
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let clean_id = clean_external_marker(&version_id);
    let settings = storage::load_settings();
    let external_dir = resolve_external_version_dir(&version_id);
    let external_path = external_dir.as_ref().map(PathBuf::from);

    // 1. 检查缺失依赖
    let dep_result = dep_check::check_dependencies(&clean_id, &settings, external_path.as_deref());
    let missing_files = dep_result.missing_files;

    if missing_files.is_empty() {
        return ApiResult::ok(json!({
            "success": true,
            "message": "无需下载",
            "completed": 0,
            "failed": 0
        }));
    }

    // 2. 创建下载会话（若已存在相同 sessionId 则复用）
    let input_sid = utils::get_str(&data, "sessionId");
    let dl_session_id = if !input_sid.is_empty() {
        input_sid
    } else {
        format!("launch-{}", now_ms())
    };

    {
        let mut s = sessions();
        if !s.contains_key(&dl_session_id) {
            let total = missing_files.len() as u32;
            s.insert(dl_session_id.clone(), LaunchSession {
                status: "downloading".into(),
                progress: 0,
                message: format!("正在下载 {} 个缺失文件..", total),
                total_files: total,
                completed_files: 0,
                current_file: String::new(),
                errors: Vec::new(),
                version_id: version_id.clone(),
                last_activity: now_ms(),
                speed: 0,
                completed: 0,
                failed: 0,
                queued: total,
                concurrent_downloads: 1,
                failed_files: Vec::new(),
                active_downloads: Vec::new(),
                launch_result: None,
            });
        }
    }

    // 3. 立即返回，不等待下载完成（解决 HTTP 超时问题，对齐 electron）
    let missing_count = missing_files.len() as u32;
    let response = ApiResult::ok(json!({
        "success": true,
        "sessionId": dl_session_id,
        "missingCount": missing_count
    }));

    // 4. spawn 后台任务跑下载（进度写入 session map，并推送事件）
    let app_cloned = app.clone();
    let sid = dl_session_id.clone();
    let configured_source = utils::get_str(&settings, "downloadSource");
    let download_source: String = if configured_source.is_empty() {
        "china-first".into()
    } else {
        configured_source
    };

    tauri::async_runtime::spawn(async move {
        let total = missing_files.len() as u32;
        // 并发下载池（对齐初代 downloadOne 并发数 8）
        const PARALLEL: usize = 8;
        // (completed, failed, failed_files, errors)
        let stats = std::sync::Arc::new(Mutex::new(LaunchDlStats::default()));

        eprintln!("[launch/download-deps] 后台开始下载 {} 个缺失文件 (session={})", total, sid);

        let files: Vec<_> = missing_files
            .into_iter()
            .map(|f| {
                let d = std::path::PathBuf::from(&f.path);
                (f, d)
            })
            .collect();

        // 为并发流克隆引用，保留原变量供完成阶段使用
        let app_stream = app_cloned.clone();
        let sid_stream = sid.clone();
        let src_stream = download_source.clone();
        let stats_stream = stats.clone();

        use futures_util::StreamExt;
        futures_util::stream::iter(files)
            .for_each_concurrent(PARALLEL, move |(file, dest)| {
                let app_cur = app_stream.clone();
                let sid_cur = sid_stream.clone();
                let source_cur = src_stream.clone();
                let stats_cur = stats_stream.clone();
                async move {
                    // 跳过 java 类型（用户自行安装）
                    if file.kind == "java" {
                        eprintln!("[launch/download-deps] 跳过 Java 依赖: {}", file.name);
                        return;
                    }
                    // 创建父目录
                    if let Some(parent) = dest.parent() {
                        if !parent.exists() {
                            if let Err(e) = std::fs::create_dir_all(parent) {
                                eprintln!("[launch/download-deps] 创建目录失败 {}: {}", parent.display(), e);
                                let mut st = stats_cur.lock().unwrap();
                                st.failed += 1;
                                st.failed_files.push(file.name.clone());
                                st.errors.push(format!("创建目录失败: {}", e));
                                return;
                            }
                        }
                    }
                    let sha1 = if file.sha1.is_empty() { None } else { Some(file.sha1.clone()) };
                    let size = if file.size > 0 { Some(file.size) } else { None };
                    match crate::download::download_with_mirror_retry(
                        &file.url,
                        &dest,
                        sha1.as_deref(),
                        size,
                        &source_cur,
                        120,
                    )
                    .await
                    {
                        Ok(()) => {
                            stats_cur.lock().unwrap().completed += 1;
                            eprintln!("[launch/download-deps] 下载成功: {}", file.name);
                        }
                        Err(e) => {
                            let mut st = stats_cur.lock().unwrap();
                            st.failed += 1;
                            st.failed_files.push(file.name.clone());
                            st.errors.push(format!("{}: {}", file.name, e));
                            eprintln!("[launch/download-deps] 下载失败 {}: {}", file.name, e);
                        }
                    }
                    // 推送进度 + 更新 session（每个文件完成后）
                    let (completed, failed) = {
                        let st = stats_cur.lock().unwrap();
                        (st.completed, st.failed)
                    };
                    {
                        let mut s = sessions();
                        if let Some(sess) = s.get_mut(&sid_cur) {
                            let progress = (completed + failed) * 100 / total.max(1);
                            sess.status = "downloading".into();
                            sess.progress = progress;
                            sess.message = format!("下载完成 {}/{} 个文件", completed + failed, total);
                            sess.current_file = file.name.clone();
                            sess.completed_files = completed;
                            sess.completed = completed;
                            sess.failed = failed;
                            sess.queued = total.saturating_sub(completed + failed);
                            sess.concurrent_downloads = PARALLEL as u32;
                            sess.last_activity = now_ms();
                        }
                    }
                    let _ = app_cur.emit(
                        "launch-download-progress",
                        json!({
                            "sessionId": sid_cur,
                            "status": "downloading",
                            "progress": (completed + failed) * 100 / total.max(1),
                            "message": format!("下载完成 {}/{} 个文件", completed + failed, total),
                            "currentFile": file.name,
                            "completed": completed,
                            "failed": failed,
                            "total": total
                        }),
                    );
                }
            })
            .await;

        // 完成：读取最终统计并写入 session
        let final_stats = stats.lock().unwrap().clone();
        let completed = final_stats.completed;
        let failed = final_stats.failed;
        let failed_files = final_stats.failed_files;
        let errors = final_stats.errors;
        let final_status: &str = if failed > 0 && completed == 0 { "failed" } else { "completed" };
        {
            let mut s = sessions();
            if let Some(sess) = s.get_mut(&sid) {
                sess.status = final_status.into();
                sess.progress = 100;
                sess.message = format!("下载完成: {} 个成功, {} 个失败", completed, failed);
                sess.completed_files = completed;
                sess.completed = completed;
                sess.failed = failed;
                sess.queued = 0;
                sess.concurrent_downloads = 0;
                sess.failed_files = failed_files.clone();
                sess.errors = errors;
                sess.active_downloads = Vec::new();
                sess.current_file = String::new();
                sess.last_activity = now_ms();
            }
        }

        let _ = app_cloned.emit(
            "launch-download-progress",
            json!({
                "sessionId": sid,
                "status": final_status,
                "progress": 100,
                "message": format!("下载完成: {} 个成功, {} 个失败", completed, failed),
                "completed": completed,
                "failed": failed,
                "total": total,
                "failedFiles": failed_files
            }),
        );

        eprintln!("[launch/download-deps] 完成: 成功 {} / 失败 {} / 总 {}", completed, failed, total);
    });

    response
}
