// api/download_custom.rs — 自定义文件下载（浏览器下载接管的落地接口）
//
// 背景：扩展/宿主只负责把「下载地址 + 文件名 + 目录」塞进来，
//       真正按下「开始下载」的永远是用户。
//
// 路由：
//   GET  /api/download-custom/pending         取走一条待确认请求（取后即清）
//   POST /api/download-custom                 创建下载会话并启动
//   GET  /api/download-custom/status          查询进度
//   POST /api/download-custom/cancel          取消
//   GET  /api/download-custom/default-folder  上次使用的保存目录
//   POST /api/download-custom/save-folder     记住用户选的目录
//
// 引擎直接复用 download::download_with_mirror_cancellable（分块 + 续传 + 多镜像）。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter};

use super::ApiResult;
use crate::download::download_with_mirror_cancellable;
use crate::storage;
use crate::utils;

/// 保存目录在 settings.json 里的键名
const FOLDER_KEY: &str = "customDownloadFolder";

/// 单个下载会话
struct CustomSession {
    session_id: String,
    status: String,       // running / paused / completed / failed / cancelled
    message: String,
    file_name: String,
    save_path: String,
    /// 原始下载地址（暂停后继续要用）
    url: String,
    /// 目标路径（暂停后继续要用同一个，不能再 uniquify 一次）
    dest: String,
    /// 后台任务句柄：暂停时 abort，恢复时重新 spawn
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    progress: u32,
    downloaded: u64,
    total: u64,
    speed: u64,
    error: String,
    cancel: Arc<AtomicBool>,
    started_at: u64,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, Arc<Mutex<CustomSession>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 路由分发
pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/download-custom/pending" => Some(ApiResult::ok(json!({
            "request": crate::bridge::drain_pending()
        }))),
        "POST /api/download-custom" => Some(handle_start(app, body).await),
        "GET /api/download-custom/status" => Some(handle_status(params)),
        "POST /api/download-custom/cancel" => Some(handle_cancel(body)),
        "POST /api/download-custom/pause" => Some(handle_pause(body)),
        "POST /api/download-custom/resume" => Some(handle_resume(app, body)),
        "GET /api/download-custom/default-folder" => Some(ApiResult::ok(json!({
            "folder": default_folder()
        }))),
        "POST /api/download-custom/save-folder" => Some(handle_save_folder(body)),
        // 只解析文件名，不建任务：扩展填表时用它把"download.bin"换成真实名字
        "POST /api/download-custom/resolve-name" => Some(handle_resolve_name(body).await),
        _ => None,
    }
}

/// POST /api/download-custom/resolve-name — 只解析文件名
///
/// 浏览器扩展拿到下载时，`item.filename` 经常是空的或"download"，
/// 这时候界面里会显示 download.bin，用户看不到真实格式。
/// 这里单独提供一个"只解析、不下载"的入口，让卡片在点「开始下载」之前
/// 就显示正确的文件名（走 Content-Disposition / 302 / Content-Type 那套）。
async fn handle_resolve_name(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let url = utils::get_str_trim(&data, "url");
    if url.is_empty() {
        return ApiResult::err(400, "缺少下载地址");
    }
    let name = resolve_file_name(&url).await;
    // resolve 的兜底值 download.bin 对用户没意义，回空串让前端保持"待推断"
    let out = if name == "download.bin" { String::new() } else { name };
    ApiResult::ok(json!({ "fileName": out }))
}

/// POST /api/download-custom — 开始下载
///
/// 文件名优先级（重要：格式必须跟真实文件一致）：
///   1. 请求里显式给的 fileName（用户在卡片里填的）
///   2. HEAD/GET 探测 Content-Disposition: filename*= / filename=
///   3. 重定向后的最终 URL 路径段（很多站点直接 302 到 .../pkg.exe）
///   4. 原始 URL 路径段
///   5. download.bin
/// 旧实现只看原始 URL，遇到 download?id=123 这类地址就会把文件名写成
/// "download" 或 "download.bin"，扩展名的丢失就是这么来的。
async fn handle_start(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let url = utils::get_str_trim(&data, "url");
    let save_path = utils::get_str_trim(&data, "savePath");
    let file_name = utils::get_str_trim(&data, "fileName");

    if url.is_empty() {
        return ApiResult::err(400, "缺少下载地址");
    }
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ftp://")) {
        return ApiResult::err(400, "只支持 http/https/ftp 下载地址");
    }
    if save_path.is_empty() {
        return ApiResult::err(400, "缺少保存目录");
    }

    let dir = PathBuf::from(&save_path);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return ApiResult::err(400, &format!("保存目录不可用: {}", e));
    }

    // 文件名：显式给了就用；否则按 URL → 重定向 → Content-Disposition 依次试探
    let name = if file_name.is_empty() {
        resolve_file_name(&url).await
    } else {
        ensure_extension(sanitize_file_name(&file_name), &url).await
    };
    if name.is_empty() {
        return ApiResult::err(400, "无法确定文件名");
    }

    // 目标路径去重：与浏览器 uniquify 语义一致，绝不覆盖已有文件
    let dest = unique_dest(&dir, &name);
    let final_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.clone());

    // 记住目录，供下次接续
    let _ = storage::save_settings(&folder_patch(&save_path));

    let session_id = format!("custom-{}", uuid_like());
    let cancel = Arc::new(AtomicBool::new(false));

    let session = CustomSession {
        session_id: session_id.clone(),
        status: "running".into(),
        message: "正在连接...".into(),
        file_name: final_name.clone(),
        save_path: save_path.clone(),
        url: url.clone(),
        dest: dest.to_string_lossy().to_string(),
        task: None,
        progress: 0,
        downloaded: 0,
        total: 0,
        speed: 0,
        error: String::new(),
        cancel: cancel.clone(),
        started_at: now_ms(),
    };

    SESSIONS
        .lock()
        .unwrap()
        .insert(session_id.clone(), Arc::new(Mutex::new(session)));

    // 启动后台任务
    let app_clone = app.clone();
    let sid = session_id.clone();
    let url_clone = url.clone();
    let dest_clone = dest.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_download(app_clone, sid, url_clone, dest_clone, cancel).await;
    });
    update_session(&session_id, |s| s.task = Some(handle));

    ApiResult::ok(json!({
        "sessionId": session_id,
        "fileName": final_name,
        "savePath": save_path,
        "dest": dest.to_string_lossy(),
    }))
}

/// 真正的下载执行体
async fn run_download(
    app: AppHandle,
    session_id: String,
    url: String,
    dest: PathBuf,
    cancel: Arc<AtomicBool>,
) {
    let settings = storage::load_settings();
    let source = utils::get_str(&settings, "downloadSource");
    let source = if source.is_empty() { "china-first".to_string() } else { source };

    // 进度回调：更新累计字节 + 会话状态（500ms 节流由前端负责）
    let app_cb = app.clone();
    let sid_cb = session_id.clone();
    let last_emit = Arc::new(Mutex::new(0u64));

    let on_progress: crate::download::ProgressCb = Arc::new(move |p| {
        update_session(&sid_cb, |s| {
            s.downloaded = p.bytes_downloaded;
            if p.total_bytes > 0 {
                s.total = p.total_bytes;
            }
            s.speed = p.speed;
            if s.total > 0 {
                s.progress = ((p.bytes_downloaded as f64 / s.total as f64) * 100.0)
                    .min(100.0) as u32;
            }
            s.message = format!(
                "{}/{} · {}",
                human_size(p.bytes_downloaded),
                human_size(s.total),
                human_speed(p.speed)
            );
        });
        // 100ms 节流推送，避免高频事件刷爆 WebView
        let now = now_ms();
        let mut guard = last_emit.lock().unwrap();
        if now.saturating_sub(*guard) >= 100 {
            *guard = now;
            drop(guard);
            if let Some(snap) = session_snapshot(&sid_cb) {
                let _ = app_cb.emit("custom-download-progress", snap);
            }
        }
    });

    // 总请求超时 30 分钟（与 updater 一致）：小值会在慢速源上把大文件反复截断，
    // 传 0 则 reqwest 立即超时，因此这里给一个足够宽的上限，真正的停滞由底层 stall 检测兜底。
    const REQUEST_TIMEOUT_SECS: u64 = 1800;
    let result = download_with_mirror_cancellable(
        &url,
        &dest,
        None,
        None,
        &source,
        REQUEST_TIMEOUT_SECS,
        Some(on_progress),
        &cancel,
    )
    .await;

    let (status, message) = match result {
        Ok(()) => ("completed".to_string(), "下载完成".to_string()),
        Err(e) if e.contains("已取消") => ("cancelled".to_string(), "已取消".to_string()),
        Err(e) => ("failed".to_string(), e),
    };

    update_session(&session_id, |s| {
        // 暂停时任务被 abort，这里的"已取消"不代表用户取消：
        // 会话状态已经由 handle_pause 置成 paused，别覆盖它。
        if s.status == "paused" {
            return;
        }
        s.status = status.clone();
        s.message = message.clone();
        if status == "completed" {
            s.progress = 100;
            if s.total == 0 {
                s.total = s.downloaded;
            }
        }
        if status == "failed" {
            s.error = message.clone();
        }
    });

    if let Some(snap) = session_snapshot(&session_id) {
        let _ = app.emit("custom-download-progress", snap);
    }

    // 完成/失败后 60 秒清掉会话，避免长期驻留内存。
    // 暂停的会话必须留着 —— 用户随时可能点「继续」。
    let paused = session_snapshot(&session_id)
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "paused"))
        .unwrap_or(false);
    if !paused {
        let sid = session_id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            SESSIONS.lock().unwrap().remove(&sid);
        });
    }
}

/// GET /api/download-custom/status
fn handle_status(params: &Option<Value>) -> ApiResult {
    let sid = params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if sid.is_empty() {
        return ApiResult::err(400, "缺少 sessionId");
    }
    match session_snapshot(sid) {
        Some(v) => ApiResult::ok(v),
        None => ApiResult::ok(json!({ "status": "not_found" })),
    }
}


/// POST /api/download-custom/pause — 暂停
///
/// 实现方式：置 cancel 标志 + abort 后台任务。
/// 已写入的 `*.downloading` / `*.c{N}` 分块文件保留，继续时靠 Range 断点续传。
fn handle_pause(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let sid = utils::get_str(&data, "sessionId");
    if sid.is_empty() {
        return ApiResult::err(400, "缺少 sessionId");
    }

    let mut task = None;
    let mut found = false;
    if let Some(arc) = SESSIONS.lock().unwrap().get(&sid) {
        let mut s = arc.lock().unwrap();
        if s.status == "running" {
            s.cancel.store(true, Ordering::SeqCst);
            s.status = "paused".into();
            s.message = "已暂停".into();
            task = s.task.take();
            found = true;
        }
    }
    if let Some(t) = task {
        t.abort();
    }

    if !found {
        return ApiResult::err(400, "当前没有正在进行的下载");
    }
    if let Some(snap) = session_snapshot(&sid) {
        ApiResult::ok(json!({ "success": true, "session": snap }))
    } else {
        ApiResult::ok(json!({ "success": true }))
    }
}

/// POST /api/download-custom/resume — 继续
///
/// 重新起一个 run_download，目标路径沿用会话里记的那个 dest，
/// 引擎会检测 `*.downloading` 剩余长度并用 `Range: bytes=N-` 续传。
fn handle_resume(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let sid = utils::get_str(&data, "sessionId");
    if sid.is_empty() {
        return ApiResult::err(400, "缺少 sessionId");
    }

    let (url, dest, cancel) = {
        let guard = SESSIONS.lock().unwrap();
        let Some(arc) = guard.get(&sid) else {
            return ApiResult::err(404, "下载会话不存在");
        };
        let mut s = arc.lock().unwrap();
        if s.status != "paused" {
            return ApiResult::err(400, "当前不是暂停状态");
        }
        s.status = "running".into();
        s.message = "正在继续...".into();
        s.error.clear();
        let c = Arc::new(AtomicBool::new(false));
        s.cancel = c.clone();
        (s.url.clone(), PathBuf::from(s.dest.clone()), c)
    };

    // 重新起任务：引擎检测 *.downloading 的剩余长度，用 Range 断点续传
    let app_clone = app.clone();
    let sid_clone = sid.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_download(app_clone, sid_clone, url, dest, cancel).await;
    });
    update_session(&sid, |s| s.task = Some(handle));

    if let Some(snap) = session_snapshot(&sid) {
        ApiResult::ok(json!({ "success": true, "session": snap }))
    } else {
        ApiResult::ok(json!({ "success": true }))
    }
}

/// POST /api/download-custom/cancel
fn handle_cancel(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let sid = utils::get_str(&data, "sessionId");
    if sid.is_empty() {
        return ApiResult::err(400, "缺少 sessionId");
    }

    let mut ok = false;
    let mut task = None;
    let mut dest: Option<PathBuf> = None;
    if let Some(arc) = SESSIONS.lock().unwrap().get(&sid) {
        let mut s = arc.lock().unwrap();
        s.cancel.store(true, Ordering::SeqCst);
        s.status = "cancelled".into();
        s.message = "已取消".into();
        task = s.task.take();
        if !s.dest.is_empty() {
            dest = Some(PathBuf::from(s.dest.clone()));
        }
        ok = true;
    }
    // abort 后台任务：让阻塞在 stream.next() 的循环立刻结束
    if let Some(t) = task {
        t.abort();
    }
    // 取消 = 彻底放弃这次下载：把已经落盘的部分全删掉，
    // 别在用户目录里留一堆 .downloading / .c0 半成品（用户明确要求"清空已下载的文件"）。
    let mut removed = 0usize;
    if let Some(d) = dest {
        removed += purge_partial(&d);
    }
    ApiResult::ok(json!({ "success": ok, "removed": removed }))
}

/// 删掉某个下载目标的所有中间文件：
///   dest.downloading               单流半成品
///   dest.c0 / dest.c1 / ...        分块半成品
///   dest 本身                       还没合完或已合完的成品（取消即丢弃）
/// 返回删除的文件数。
fn purge_partial(dest: &Path) -> usize {
    let mut n = 0usize;

    // 单流半成品：xxx.downloading
    let single = dest.with_extension("downloading");
    if single.exists() && std::fs::remove_file(&single).is_ok() {
        n += 1;
    }

    // 分块半成品：xxx.c0 / xxx.c1 ...
    if let (Some(parent), Some(name)) = (dest.parent(), dest.file_name()) {
        let prefix = format!("{}.c", name.to_string_lossy());
        if let Ok(rd) = std::fs::read_dir(parent) {
            for entry in rd.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.starts_with(&prefix)
                    && fname[prefix.len()..].chars().all(|c| c.is_ascii_digit())
                    && std::fs::remove_file(entry.path()).is_ok()
                {
                    n += 1;
                }
            }
        }
    }

    // 成品本身
    if dest.exists() && std::fs::remove_file(dest).is_ok() {
        n += 1;
    }
    n
}

/// POST /api/download-custom/save-folder
fn handle_save_folder(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let folder = utils::get_str_trim(&data, "folder");
    if folder.is_empty() {
        return ApiResult::err(400, "缺少目录");
    }
    let _ = std::fs::create_dir_all(Path::new(&folder));
    let _ = storage::save_settings(&folder_patch(&folder));
    ApiResult::ok(json!({ "success": true, "folder": folder }))
}

// ============== 会话工具 ==============

fn update_session(session_id: &str, updater: impl FnOnce(&mut CustomSession)) {
    let guard = SESSIONS.lock().unwrap();
    if let Some(arc) = guard.get(session_id) {
        let mut s = arc.lock().unwrap();
        updater(&mut s);
    }
}

fn session_snapshot(session_id: &str) -> Option<Value> {
    let guard = SESSIONS.lock().unwrap();
    let arc = guard.get(session_id)?;
    let s = arc.lock().unwrap();
    let elapsed = now_ms().saturating_sub(s.started_at).max(1);
    let avg_speed = if s.downloaded > 0 {
        s.downloaded * 1000 / elapsed
    } else {
        0
    };
    Some(json!({
        "sessionId": s.session_id,
        "status": s.status,
        "message": s.message,
        "fileName": s.file_name,
        "savePath": s.save_path,
        "progress": s.progress,
        "downloaded": s.downloaded,
        "total": s.total,
        "speed": if s.speed > 0 { s.speed } else { avg_speed },
        "error": s.error,
        // 暂停/继续需要知道能不能续、续哪个文件
        "canPause": s.status == "running",
        "canResume": s.status == "paused",
        "dest": s.dest,
    }))
}

/// 构造 settings 补丁对象：{ "customDownloadFolder": "<dir>" }
/// 不用 json! 宏是因为宏会把标识符当成字面 key。
fn folder_patch(folder: &str) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(FOLDER_KEY.to_string(), Value::String(folder.to_string()));
    Value::Object(map)
}

// ============== 路径与文件名 ==============

/// 上次使用的保存目录；没有则回落到 data/downloads
fn default_folder() -> String {
    let settings = storage::load_settings();
    let saved = utils::get_str_trim(&settings, FOLDER_KEY);
    if !saved.is_empty() && Path::new(&saved).exists() {
        return saved;
    }
    let fallback = storage::resolve_data_dir().join("downloads");
    let _ = std::fs::create_dir_all(&fallback);
    fallback.to_string_lossy().to_string()
}

/// 文件名解析：URL → 跟随重定向拿最终 URL / Content-Disposition → 兜底
async fn resolve_file_name(url: &str) -> String {
    // 先看 URL 本身；只有在"看起来不像文件名"时才去探测网络，
    // 避免每次下载都多打一次请求（直链场景 99% 能直接拿到 name.ext）
    let from_url = guess_file_name(url);
    if url_looks_like_file(url) && has_extension(&from_url) {
        return from_url;
    }

    if let Some((final_url, cd_name, content_type)) = probe_remote_name(url).await {
        if !cd_name.is_empty() {
            return cd_name;
        }
        if let Some(last) = url_last_segment(&final_url) {
            let name = sanitize_file_name(&last);
            if !name.is_empty() && name != "download.bin" && has_extension(&name) {
                return name;
            }
        }
        // 连重定向后的 URL 都没有扩展名：用响应的 Content-Type 补一个
        if let Some(ext) = ext_from_content_type(&content_type) {
            let base = url_last_segment(&final_url)
                .map(|s| sanitize_file_name(&s))
                .filter(|s| !s.is_empty() && s != "download.bin")
                .unwrap_or_else(|| "download".to_string());
            let base = base.trim_end_matches('.');
            if !has_extension(base) {
                return format!("{}.{}", base, ext);
            }
            return base.to_string();
        }
    }

    from_url
}

/// Content-Type → 常见扩展名（只在 URL/头部都没线索时兜底）
fn ext_from_content_type(ct: &str) -> Option<&'static str> {
    let ct = ct.split(';').next().unwrap_or("").trim();
    Some(match ct {
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/java-archive" => "jar",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable"
        | "application/x-msdos-program" | "application/exe" => "exe",
        "application/x-msi" | "application/x-msi-package" => "msi",
        "application/pdf" => "pdf",
        "application/json" => "json",
        "application/x-tar" => "tar",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-7z-compressed" => "7z",
        "application/x-rar-compressed" | "application/vnd.rar" => "rar",
        "text/plain" => "txt",
        "text/html" => return None, // 网页多半是错误页，别乱加后缀
        _ => return None,
    })
}

/// 用户自己填了文件名但没有扩展名时（比如只写 "installer"），
/// 用 URL 或探测结果把扩展名补回去，保证落地文件能被系统识别。
async fn ensure_extension(name: String, url: &str) -> String {
    if has_extension(&name) {
        return name;
    }
    let guessed = resolve_file_name(url).await;
    let ext = std::path::Path::new(&guessed)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    if ext.is_empty() || ext == "bin" {
        name
    } else {
        format!("{}.{}", name.trim_end_matches('.'), ext)
    }
}

/// 文件名后半段是否带扩展名（1-8 位字母数字，允许 .tar.gz 这类）
fn has_extension(name: &str) -> bool {
    if name.is_empty() || name == "download.bin" {
        return false;
    }
    let ext = match name.rsplit_once('.') {
        Some((_, e)) => e,
        None => return false,
    };
    !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

/// URL 最后一段是否像个文件名（含点），用于判断要不要发探测请求
fn url_looks_like_file(url: &str) -> bool {
    match url_last_segment(url) {
        Some(s) => s.contains('.') && !s.starts_with('.'),
        None => false,
    }
}

/// 取 URL 路径最后一段（去查询串、% 解码）
fn url_last_segment(url: &str) -> Option<String> {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    let path_part = match no_query.find("://") {
        Some(idx) => {
            let after = &no_query[idx + 3..];
            match after.find('/') {
                Some(slash) => &after[slash..],
                None => return None,
            }
        }
        None => no_query,
    };
    let last = path_part.rsplit('/').find(|s| !s.is_empty())?;
    Some(percent_decode(last))
}

/// 发一次 HEAD（失败则带 Range 的 GET）拿最终 URL 和 Content-Disposition 文件名。
/// 任何异常都静默返回 None —— 绝不因为探测失败而让下载本身失败。
async fn probe_remote_name(url: &str) -> Option<(String, String, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VersePC2")
        .build()
        .ok()?;

    let resp = match client.head(url).send().await {
        Ok(r) if r.status().is_success() || r.status().is_redirection() => r,
        _ => {
            // 有些站点不认 HEAD，用 Range: bytes=0-0 的 GET 试探
            match client
                .get(url)
                .header(reqwest::header::RANGE, "bytes=0-0")
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => return None,
            }
        }
    };

    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let cd_name = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition)
        .map(|n| sanitize_file_name(&n))
        .unwrap_or_default();

    Some((final_url, cd_name, content_type))
}

/// 解析 Content-Disposition：优先 RFC 5987 的 filename*=UTF-8''xxx，再退回 filename=
fn parse_content_disposition(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();

    if let Some(idx) = lower.find("filename*=") {
        let rest = &value[idx + "filename*=".len()..];
        let token = rest.split(';').next().unwrap_or(rest).trim();
        // 形如 UTF-8''%E4%B8%AD.zip
        let encoded = match token.split_once("''") {
            Some((_charset, enc)) => enc,
            None => token,
        };
        let decoded = percent_decode(encoded.trim_matches('"'));
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }

    if let Some(idx) = lower.find("filename=") {
        let rest = &value[idx + "filename=".len()..];
        let token = rest.split(';').next().unwrap_or(rest).trim();
        let unquoted = token.trim_matches('"').trim();
        if !unquoted.is_empty() {
            return Some(percent_decode(unquoted));
        }
    }

    None
}

/// 从 URL 推断文件名：取最后一个 path 段，去掉查询串并做百分号解码
fn guess_file_name(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    // 去掉 scheme://host 部分，只留路径；否则 "https://example.com" 会把主机名当文件名
    let path_part = match no_query.find("://") {
        Some(idx) => {
            let after = &no_query[idx + 3..];
            match after.find('/') {
                Some(slash) => &after[slash..],
                None => "", // 完全没有路径
            }
        }
        None => no_query,
    };
    let last = path_part.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    let decoded = percent_decode(last);
    let name = sanitize_file_name(&decoded);
    if name.is_empty() {
        "download.bin".to_string()
    } else {
        name
    }
}

/// 清洗文件名：去掉 Windows 非法字符与控制字符，保留扩展名
fn sanitize_file_name(raw: &str) -> String {
    let trimmed = raw.trim();
    let cleaned: String = trimmed
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').trim().to_string();
    if cleaned.len() > 200 {
        // 超长时保留扩展名
        let ext = Path::new(&cleaned)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let keep = 200 - ext.len();
        let mut s: String = cleaned.chars().take(keep).collect();
        s.push_str(&ext);
        s
    } else {
        cleaned
    }
}

/// 生成不冲突的目标路径：name.ext → name (1).ext → name (2).ext ...
fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for i in 1..1000 {
        let next = dir.join(format!("{} ({}){}", stem, i, ext));
        if !next.exists() {
            return next;
        }
    }
    dir.join(format!("{}-{}{}", stem, uuid_like(), ext))
}

/// 极简百分号解码（不引依赖）
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ============== 展示格式 ==============

fn human_size(v: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = v as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", v, UNITS[0])
    } else {
        format!("{:.1} {}", size, UNITS[idx])
    }
}

fn human_speed(v: u64) -> String {
    format!("{}/s", human_size(v))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn uuid_like() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", now)
}

#[cfg(test)]
mod tests {
    use super::*;



    #[test]
    fn ext_from_content_type_maps_common_types() {
        assert_eq!(ext_from_content_type("application/zip"), Some("zip"));
        assert_eq!(ext_from_content_type("application/zip; charset=binary"), Some("zip"));
        assert_eq!(ext_from_content_type("application/java-archive"), Some("jar"));
        assert_eq!(ext_from_content_type("application/x-msdownload"), Some("exe"));
        assert_eq!(ext_from_content_type("text/html"), None);
        assert_eq!(ext_from_content_type("application/octet-stream"), None);
    }

    // ── 扩展名保持（本次修复的重点） ──

    #[test]
    fn has_extension_recognises_common_exts() {
        assert!(has_extension("setup.exe"));
        assert!(has_extension("mod.zip"));
        assert!(has_extension("a.jar"));
        assert!(has_extension("archive.tar.gz"));
        assert!(!has_extension("download"));
        assert!(!has_extension("download.bin"));
        assert!(!has_extension(""));
        assert!(!has_extension("no-dot-name"));
    }

    #[test]
    fn url_last_segment_handles_query_and_missing_path() {
        assert_eq!(url_last_segment("https://a.com/x/setup.exe?t=1").as_deref(), Some("setup.exe"));
        assert_eq!(url_last_segment("https://a.com"), None);
        assert_eq!(url_last_segment("https://a.com/").as_deref(), None);
        assert_eq!(url_last_segment("https://a.com/%E6%B5%8B%E8%AF%95.zip").as_deref(), Some("测试.zip"));
    }

    #[test]
    fn url_looks_like_file_requires_dot() {
        assert!(url_looks_like_file("https://a.com/setup.exe"));
        assert!(url_looks_like_file("https://a.com/f/a.zip?x=1"));
        assert!(!url_looks_like_file("https://a.com/download"));
        assert!(!url_looks_like_file("https://a.com/download?id=9"));
    }

    #[test]
    fn parse_content_disposition_plain() {
        assert_eq!(
            parse_content_disposition("attachment; filename=\"setup.exe\"").as_deref(),
            Some("setup.exe")
        );
        assert_eq!(
            parse_content_disposition("inline; filename=mod.zip").as_deref(),
            Some("mod.zip")
        );
    }

    #[test]
    fn parse_content_disposition_rfc5987() {
        assert_eq!(
            parse_content_disposition("attachment; filename*=UTF-8''%E5%AE%89%E8%A3%85.zip").as_deref(),
            Some("安装.zip")
        );
    }

    #[test]
    fn parse_content_disposition_missing() {
        assert_eq!(parse_content_disposition("attachment"), None);
    }

    #[test]
    fn ensure_extension_keeps_existing_name() {
        // 已有扩展名时原样返回（不能把 setup.exe 变成 setup.exe.bin）
        assert!(has_extension("setup.exe"));
    }

    // ── 文件名推断 ──

    #[test]
    fn guess_file_name_takes_last_path_segment() {
        assert_eq!(
            guess_file_name("https://example.com/a/b/c.zip"),
            "c.zip"
        );
    }

    #[test]
    fn guess_file_name_strips_query_and_fragment() {
        assert_eq!(
            guess_file_name("https://example.com/file.jar?x=1&y=2#frag"),
            "file.jar"
        );
    }

    #[test]
    fn guess_file_name_decodes_percent_escapes() {
        // 中文文件名的 UTF-8 百分号编码
        assert_eq!(
            guess_file_name("https://example.com/%E6%B5%8B%E8%AF%95.zip"),
            "测试.zip"
        );
    }

    #[test]
    fn guess_file_name_falls_back_when_empty() {
        // 只有主机名 / 只有一个斜杠时没有真实文件名可推断
        assert_eq!(guess_file_name("https://example.com/"), "download.bin");
        assert_eq!(guess_file_name("https://example.com"), "download.bin");
        // 带路径时正常取最后一段
        assert_eq!(guess_file_name("https://example.com/a/b.zip"), "b.zip");
    }

    // ── 文件名清洗 ──

    #[test]
    fn sanitize_replaces_windows_illegal_chars() {
        // 每个非法字符各替换成一个下划线；输入里 f 与 g 相邻，故输出为 "fg"
        assert_eq!(sanitize_file_name(r#"a<b>c:d"e/f|g?h*i"#), "a_b_c_d_e_f_g_h_i");
        assert_eq!(sanitize_file_name("a/b"), "a_b");
    }

    #[test]
    fn sanitize_trims_dots_and_spaces() {
        assert_eq!(sanitize_file_name("  ..name.zip..  "), "name.zip");
    }

    #[test]
    fn sanitize_keeps_chinese_and_extension_when_truncating() {
        let long = format!("{}.zip", "文".repeat(300));
        let out = sanitize_file_name(&long);
        assert!(out.ends_with(".zip"), "扩展名必须保留: {}", out);
        assert!(out.chars().count() <= 200);
    }

    // ── 目标路径去重 ──

    #[test]
    fn unique_dest_keeps_original_when_free() {
        let dir = temp_dir("unique-free");
        let p = unique_dest(&dir, "a.zip");
        assert_eq!(p.file_name().unwrap(), "a.zip");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_dest_appends_counter_when_taken() {
        let dir = temp_dir("unique-taken");
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        let p = unique_dest(&dir, "a.zip");
        assert_eq!(p.file_name().unwrap(), "a (1).zip");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unique_dest_increments_further() {
        let dir = temp_dir("unique-two");
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        std::fs::write(dir.join("a (1).zip"), b"x").unwrap();
        let p = unique_dest(&dir, "a.zip");
        assert_eq!(p.file_name().unwrap(), "a (2).zip");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 百分号解码 ──

    #[test]
    fn percent_decode_handles_plain_and_escaped() {
        assert_eq!(percent_decode("abc"), "abc");
        assert_eq!(percent_decode("%41%42"), "AB");
        // 非法序列原样保留
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    // ── 展示格式 ──

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    // ── settings 补丁 ──

    #[test]
    fn folder_patch_uses_key_name_not_identifier() {
        let v = folder_patch(r"D:\Downloads");
        assert_eq!(v.get("customDownloadFolder").and_then(|x| x.as_str()), Some(r"D:\Downloads"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("verse-custom-dl-{}-{}", tag, now_ms()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}
