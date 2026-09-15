// updater.rs — 便携版全量自动替换更新
//
// 职责（实现为便携版"全量替换"）：
//   1. 检查更新：读取 GitHub VersPc2 仓库最新 release，比较版本号
//   2. 下载更新：多镜像加速下载新 exe，推送进度与校验
//   3. 安装更新：写替换脚本 → 退出自身 → 脚本替换 exe → 重启
//   4. 跳过版本 / 打开发布页
//
// 前端通过 window.electronAPI.updater.* 触发，状态通过事件 updater:status
// 推送（payload 为 { channel, data }，channel 与 Electron 版一致）。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

// ============== 常量 ==============

const UPDATE_JSON_SOURCES: &[&str] = &[
    "https://ghfast.top/https://raw.githubusercontent.com/doujie081231/VersePc2/main/update.json",
    "https://ghproxy.net/https://raw.githubusercontent.com/doujie081231/VersePc2/main/update.json",
    "https://gh-proxy.com/https://raw.githubusercontent.com/doujie081231/VersePc2/main/update.json",
    "https://raw.githubusercontent.com/doujie081231/VersePc2/main/update.json",
];

const RELEASE_PAGE: &str = "https://github.com/doujie081231/VersePc2/releases/latest";

const GITHUB_RELEASE_BASE: &str = "https://github.com/doujie081231/VersePc2";
const GITEE_RELEASE_BASE: &str = "https://gitee.com/doujie081231/verse-pc2";

fn mirror_urls(url: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if url.starts_with("https://github.com/") {
        for host in ["ghfast.top", "ghproxy.net", "gh-proxy.com", "ghproxy.link"] {
            v.push(format!("https://{}/{}", host, url));
        }
    }
    v.push(url.to_string());
    v
}

/// 把 release 下载地址展开为完整下载源列表：Gitee 优先，GitHub 次之，最后 ghfast 等镜像。
/// 供更新器与场景渲染器组件下载复用。
pub(crate) fn build_download_sources(url: &str) -> Vec<String> {
    let seg: Vec<&str> = url.split('/').collect();
    let (tag, file) = if seg.len() >= 3 {
        (seg[seg.len() - 2], seg[seg.len() - 1])
    } else {
        ("", "")
    };
    let mut v: Vec<String> = Vec::new();
    if url.starts_with("https://gitee.com/") {
        v.push(url.to_string());
    } else if !file.is_empty() {
        v.push(format!("{}/releases/download/{}/{}", GITEE_RELEASE_BASE, tag, file));
    }
    if !file.is_empty() {
        let gh = format!("{}/releases/download/{}/{}", GITHUB_RELEASE_BASE, tag, file);
        for m in mirror_urls(&gh) {
            if !v.contains(&m) {
                v.push(m);
            }
        }
    }
    if !v.contains(&url.to_string()) {
        v.push(url.to_string());
    }
    v
}

// ============== 更新状态 ==============

#[derive(Clone)]
struct UpdateAsset {
    url: String,
    size: u64,
    sha256: Option<String>,
}

#[derive(Clone)]
struct UpdateRelease {
    tag: String,
    tag_ver: String,
    published_at: String,
    body: String,
    asset: Option<UpdateAsset>,
}

struct UpdateState {
    release: UpdateRelease,
    downloaded_path: Option<PathBuf>,
}

static UPDATE_STATE: Mutex<Option<UpdateState>> = Mutex::new(None);

// ============== 工具函数 ==============

fn emit(app: &AppHandle, channel: &str, data: &Value) {
    let _ = app.emit("updater:status", json!({ "channel": channel, "data": data }));
}

/// 更新下载诊断日志：追加写入 <数据目录>/logs/updater-download.log，
/// 记录下载源切换、断点、校验、进度事件，用于排查进度条乱跳等问题。
fn log_download(line: &str) {
    let path = crate::storage::resolve_data_dir()
        .join("logs")
        .join("updater-download.log");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S%.3f"), line);
    }
}

/// 是否正在下载更新包（防止自动下载与手动点击并发写同一 .part 导致进度乱跳/文件损坏）
static UPDATE_DOWNLOADING: AtomicBool = AtomicBool::new(false);

fn current_version() -> (u32, u32, u32) {
    parse_version(env!("CARGO_PKG_VERSION"))
}

/// 返回 update.json `files` 中对应当前平台的键名（win-x64 / linux-x86_64 / macos-x86_64 / macos-aarch64）
fn current_update_key() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else {
            "macos-x86_64"
        }
    }
    #[cfg(target_os = "linux")]
    {
        "linux-x86_64"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "win-x64"
    }
}

fn parse_version(s: &str) -> (u32, u32, u32) {
    let mut parts = s
        .trim_start_matches('v')
        .split('.')
        .filter_map(|p| p.parse::<u32>().ok());
    let a = parts.next().unwrap_or(0);
    let b = parts.next().unwrap_or(0);
    let c = parts.next().unwrap_or(0);
    (a, b, c)
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn verify_file(path: &Path, expected_size: u64, expected_sha: Option<&str>) -> bool {
    use std::io::Read;
    if !path.exists() {
        return false;
    }
    if let Ok(md) = fs::metadata(path) {
        if expected_size > 0 && md.len() != expected_size {
            return false;
        }
    }
    if let Some(sha) = expected_sha {
        let Ok(mut f) = fs::File::open(path) else {
            return false;
        };
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 8192];
        loop {
            match f.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => hasher.update(&buf[..n]),
                Err(_) => return false,
            }
        }
        if !hex_lower(&hasher.finalize()).eq_ignore_ascii_case(sha) {
            return false;
        }
    }
    true
}

// 更新配置（跳过版本）路径：便携数据目录/updater-config.json
fn update_config_path() -> PathBuf {
    crate::storage::resolve_data_dir().join("updater-config.json")
}

// 自更新重启后的一次性"已更新提示"文件：升级前写入，重启启动时前端读取并删除
fn pending_notice_path() -> PathBuf {
    crate::storage::resolve_data_dir().join("pending-update-notice.json")
}

fn write_pending_notice(version: &str, notes: &str) {
    let data = json!({
        "version": version,
        "notes": notes,
        "updatedAt": chrono::Local::now().to_rfc3339()
    });
    let path = pending_notice_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string()));
}

#[tauri::command]
pub fn updater_get_pending_notice() -> Result<Value, String> {
    let path = pending_notice_path();
    if !path.exists() {
        return Ok(Value::Null);
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    let v: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));
    Ok(v)
}

fn load_skipped_version() -> Option<String> {
    let path = update_config_path();
    let content = fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&content).ok()?;
    v.get("skippedVersion").and_then(|x| x.as_str()).map(String::from)
}

fn save_skipped_version(version: &str) {
    let path = update_config_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let data = json!({ "skippedVersion": version });
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = fs::write(&path, json);
    }
}

// ============== 检查更新 ==============

/// 并发请求单个 update.json 源，成功且数据有效时返回 Some(UpdateRelease)
async fn try_fetch_source(client: &Client, url: &str) -> Result<Option<UpdateRelease>, ()> {
    let resp = client
        .get(url)
        .header("User-Agent", "VersePC")
        .send()
        .await
        .map_err(|_| ())?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let v: Value = resp.json().await.map_err(|_| ())?;

    let version = v
        .get("version")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if version.is_empty() {
        return Ok(None);
    }

    // 依据当前运行平台选择 update.json 中对应的文件键，macOS/Linux 也能拉到各自的更新包
    let files = v.get("files").and_then(|x| x.get(current_update_key()));
    let asset = files.map(|f| UpdateAsset {
        url: f.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        size: f.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
        sha256: f
            .get("sha256")
            .and_then(|x| x.as_str())
            .map(String::from),
    });

    Ok(Some(UpdateRelease {
        tag: v
            .get("releaseName")
            .and_then(|x| x.as_str())
            .unwrap_or(&version)
            .to_string(),
        tag_ver: version,
        published_at: v
            .get("releaseDate")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        body: v
            .get("releaseNotes")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        asset,
    }))
}

/// 多源并发获取 update.json，第一个成功返回的有效结果胜出
async fn fetch_update_json() -> Result<UpdateRelease, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("初始化网络失败: {}", e))?;

    let tasks: Vec<_> = UPDATE_JSON_SOURCES
        .iter()
        .map(|url| try_fetch_source(&client, url))
        .collect();
    let results = futures_util::future::join_all(tasks).await;

    for r in results {
        if let Ok(Some(release)) = r {
            return Ok(release);
        }
    }
    Err("无法获取更新信息，请检查网络连接后重试".into())
}

#[tauri::command]
pub async fn updater_check_for_updates(app: AppHandle) -> Result<Value, String> {
    emit(&app, "checking-for-update", &json!({}));

    let release = match fetch_update_json().await {
        Ok(r) => r,
        Err(e) => {
            emit(&app, "update-error", &json!({
                "message": e,
                "hint": "可尝试使用 VPN 或稍后再试"
            }));
            return Ok(json!({ "available": false, "error": e }));
        }
    };

    let current = current_version();
    let remote = parse_version(&release.tag_ver);
    if remote <= current {
        emit(&app, "update-not-available", &json!({ "version": env!("CARGO_PKG_VERSION") }));
        return Ok(json!({ "available": false, "version": release.tag_ver }));
    }

    // 用户跳过此版本
    if load_skipped_version().as_deref() == Some(release.tag_ver.as_str()) {
        emit(&app, "update-skipped", &json!({ "version": release.tag_ver }));
        return Ok(json!({ "available": true, "version": release.tag_ver, "skipped": true }));
    }

    {
        let mut guard = UPDATE_STATE.lock().unwrap();
        *guard = Some(UpdateState {
            release: release.clone(),
            downloaded_path: None,
        });
    }

    emit(&app, "update-available", &json!({
        "version": release.tag_ver,
        "currentVersion": env!("CARGO_PKG_VERSION"),
        "releaseDate": release.published_at,
        "releaseName": release.tag,
        "releaseNotes": release.body,
    }));

    // 自动更新：检测到新版本即后台自动下载，无需用户点击。下载完成后在用户关闭程序时自动替换。
    let app2 = app.clone();
    let rel2 = release.clone();
    tauri::async_runtime::spawn(async move {
        let _ = download_update_inner(&app2, &rel2).await;
    });

    Ok(json!({ "available": true, "version": release.tag_ver }))
}

// ============== 下载更新 ==============

async fn stream_download(
    app: &AppHandle,
    url: &str,
    target: &Path,
    expected_size: u64,
) -> Result<(), String> {
    let tmp = target.with_extension("part");
    let mut resume = 0u64;
    if let Ok(md) = fs::metadata(&tmp) {
        resume = md.len();
    }
    if expected_size > 0 && resume >= expected_size {
        return Ok(());
    }
    log_download(&format!(
        "[stream] 开始下载 {} resume={}B expected={}B",
        url, resume, expected_size
    ));

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let mut req = client.get(url);
    if resume > 0 {
        req = req.header("Range", format!("bytes={}-", resume));
    }
    let resp = tokio::time::timeout(Duration::from_secs(20), req.send())
        .await
        .map_err(|_| "TTFB 超时：服务器响应头迟迟未返回，切换镜像".to_string())?
        .map_err(|e| format!("连接失败: {}", e))?;
    let status_206 = resp.status().as_u16() == 206;
    if !resp.status().is_success() && !status_206 {
        return Err(format!("HTTP {}", resp.status()));
    }
    if resume > 0 && !status_206 {
        // 该源不支持 Range（返回 200 全量）：保留 .part 断点给下一个支持 Range 的镜像续传，
        // 并返回特殊错误让 fallback 直接切换镜像（同源重试无意义，只会再次全量重下导致进度回跳）
        log_download(&format!(
            "[stream] 服务器不支持 Range(HTTP {})，保留断点，切换镜像续传",
            resp.status().as_u16()
        ));
        return Err("NO_RANGE_SOURCE: 服务器不支持断点续传，切换镜像".to_string());
    }

    let total = if status_206 {
        resp.headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').nth(1))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        resp.content_length().unwrap_or(0)
    };
    let total = if total > 0 { total } else { expected_size };

    // 断点续传提示：拿到 total 后再发，与下载中的 percent 用同一基准，避免基准不同导致百分比跳动
    if resume > 0 {
        let pct = if total > 0 {
            ((resume as f64 / total as f64) * 100.0 * 10.0).round() / 10.0
        } else {
            0.0
        };
        log_download(&format!("[stream] 断点续传 resume={}B total={}B ({:.1}%)", resume, total, pct));
        emit(app, "download-progress", &json!({
            "percent": pct,
            "transferred": resume,
            "total": total.max(resume),
            "bytesPerSecond": 0,
            "resuming": true,
        }));
    } else {
        log_download(&format!("[stream] 全新下载 total={}B (content-length)", total));
    }

    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&tmp)
        .await
        .map_err(|e| format!("创建文件失败: {}", e))?;

    let mut stream = resp.bytes_stream();
    let mut transferred = resume;
    let mut last_report = Instant::now();
    let mut last_bytes = resume;
    let mut last_log_pct = -1.0;
    // 距上次收到数据的时刻；仅当超过 IDLE_TIMEOUT 仍无任何字节时才判源失效，
    // 避免把 Gitee 高峰期"慢但仍在传"的连接误杀。
    let idle_timeout = Duration::from_secs(60);

    loop {
        let waited = tokio::time::timeout(idle_timeout, stream.next()).await;
        let chunk = match waited {
            Ok(Some(r)) => r.map_err(|e| format!("下载中断: {}", e))?,
            Ok(None) => break,
            Err(_) => {
                // 60 秒整未收到任何数据块：判定该源暂时失效（切换/续传）
                log_download(&format!("[stream] 60s 无数据，切换镜像 (transferred={}B)", transferred));
                return Err("下载长时间无数据，切换镜像".to_string());
            }
        };
        transferred += chunk.len() as u64;
        f.write_all(&chunk).await.map_err(|e| format!("写入失败: {}", e))?;

        if last_report.elapsed().as_millis() >= 300 {
            let elapsed = last_report.elapsed().as_secs_f64().max(0.001);
            let pct = if total > 0 {
                (transferred as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            emit(&app, "download-progress", &json!({
                "percent": (pct * 10.0).round() / 10.0,
                "transferred": transferred,
                "total": total.max(transferred),
                "bytesPerSecond": ((transferred - last_bytes) as f64 / elapsed) as u64,
            }));
            // 诊断日志：按约 2% 采样记录，避免刷屏
            if pct - last_log_pct >= 2.0 || transferred >= total {
                log_download(&format!(
                    "[progress] {:.1}% transferred={}B total={}B",
                    pct, transferred, total
                ));
                last_log_pct = pct;
            }
            last_report = Instant::now();
            last_bytes = transferred;
        }
    }

    f.flush().await.map_err(|e| e.to_string())?;
    f.sync_all().await.map_err(|e| e.to_string())?;
    drop(f);

    let final_size = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    log_download(&format!(
        "[stream] 流结束 final={}B expected={}B 完整={}",
        final_size,
        expected_size,
        final_size == expected_size
    ));
    if total > 0 {
        emit(&app, "download-progress", &json!({
            "percent": 100.0,
            "transferred": total,
            "total": total,
            "bytesPerSecond": 0
        }));
    }
    Ok(())
}

async fn download_with_fallback(
    app: &AppHandle,
    url: &str,
    target: &Path,
    expected_size: u64,
    expected_sha: Option<&str>,
) -> Result<(), String> {
    if verify_file(target, expected_size, expected_sha) {
        if expected_size > 0 {
            emit(&app, "download-progress", &json!({
                "percent": 100.0,
                "transferred": expected_size,
                "total": expected_size,
                "bytesPerSecond": 0
            }));
        }
        return Ok(());
    }
    let _ = fs::remove_file(target);
    let part = target.with_extension("part");

    // 基准源波动多是瞬时性的（Gitee 高峰期偶发丢包），直接换源会丢掉续传进度且可能更慢。
    // 因此：同一源最多重试 MAX_RETRY_PER_SOURCE 次，每次都沿用 .part 断点续传；
    // 多次仍失败，才切换到下一个镜像（部分文件保留，跨源请求 Range 继续下载）。
    const MAX_RETRY_PER_SOURCE: u32 = 3;

    let mut last_err = String::new();
    for murl in build_download_sources(url) {
        log_download(&format!("[fallback] 尝试下载源: {}", murl));
        for attempt in 0..=MAX_RETRY_PER_SOURCE {
            if attempt > 0 {
                // 短暂让出，给网络/服务器一点恢复时间，再以断点续传重试同一源
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
            log_download(&format!("[fallback]  源内第 {} 次尝试", attempt + 1));
            match stream_download(app, &murl, target, expected_size).await {
                Ok(()) => {
                    if verify_file(&part, expected_size, expected_sha) {
                        log_download(&format!("[fallback] 校验通过 size={}B，下载完成", expected_size));
                        let _ = fs::remove_file(target);
                        fs::rename(&part, target)
                            .map_err(|e| format!("下载完成但保存失败: {}", e))?;
                        return Ok(());
                    }
                    // 校验失败：区分"被截断"（保留断点续传）与"数据损坏"（清空重下）
                    let part_size = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                    if expected_size > 0 && part_size == expected_size {
                        log_download(&format!(
                            "[fallback] 文件完整但校验不符 size={}B (可能是损坏)，清空断点从下一镜像重下",
                            part_size
                        ));
                        let _ = fs::remove_file(&part);
                    } else {
                        log_download(&format!(
                            "[fallback] 文件不完整 size={}B != expected={}B，保留断点，下一镜像续传",
                            part_size, expected_size
                        ));
                    }
                    last_err = "文件校验失败，已从下一镜像重试".to_string();
                    let _ = fs::remove_file(target);
                    break;
                }
                Err(e) => {
                    // 该源临时失败：保留 .part，稍后沿用续传继续尝试同一源
                    last_err = e;
                    log_download(&format!("[fallback] 源失败: {}", last_err));
                    // 源不支持 Range 时同源重试无意义（每次都全量重下导致进度回跳），
                    // 直接切换下一镜像，断点留给支持 Range 的镜像续传
                    if last_err.starts_with("NO_RANGE_SOURCE:") {
                        last_err = "下载源均不支持断点续传，下载中断，请稍后重试".to_string();
                        break;
                    }
                }
            }
        }
    }
    if last_err.is_empty() {
        Err("所有下载源均失败，请稍后重试或手动下载".into())
    } else {
        Err(last_err)
    }
}

#[tauri::command]
pub async fn updater_download_update(app: AppHandle) -> Result<Value, String> {
    let release = {
        let guard = UPDATE_STATE.lock().unwrap();
        guard.as_ref().map(|s| s.release.clone())
    };
    let release = release.ok_or("没有可用的更新信息，请先检查更新")?;
    match download_update_inner(&app, &release).await {
        Ok(()) => Ok(json!({ "success": true })),
        Err(e) => Ok(json!({ "success": false, "error": e })),
    }
}

/// 下载更新包本体（幂等：目标文件已存在且校验通过则跳过，前端会收到进度事件）。
/// 手动「下载」按钮与"检查到更新即自动下载"共用此实现。
async fn download_update_inner(app: &AppHandle, release: &UpdateRelease) -> Result<(), String> {
    let asset = release
        .asset
        .clone()
        .ok_or("未找到适用于当前平台的安装包")?;

    // 并发锁：自动下载与手动点击并发时，只允许一个下载写同一 .part（否则进度乱跳/文件损坏）
    if UPDATE_DOWNLOADING.swap(true, Ordering::SeqCst) {
        log_download("[inner] 已有下载任务在进行，本次跳过");
        return Err("已有下载任务正在进行中".into());
    }

    emit(app, "start-download", &json!({}));
    log_download(&format!(
        "[inner] 开始下载 v{} size={}B url={}",
        release.tag_ver, asset.size, asset.url
    ));

    let data_dir = crate::storage::resolve_data_dir();
    let tmp_dir = data_dir.join("updates");
    let _ = fs::create_dir_all(&tmp_dir);
    // 下载文件按平台命名：Windows .exe / macOS .zip / Linux 无后缀
    let file_name = match current_update_key() {
        "win-x64" => format!("VersePC2-{}.exe", release.tag_ver),
        "linux-x86_64" => format!("VersePC2-{}-linux-x86_64", release.tag_ver),
        "macos-x86_64" => format!("VersePC2-{}-macos-x86_64.zip", release.tag_ver),
        "macos-aarch64" => format!("VersePC2-{}-macos-aarch64.zip", release.tag_ver),
        _ => format!("VersePC2-{}", release.tag_ver),
    };
    let target = tmp_dir.join(file_name);

    let result = download_with_fallback(app, &asset.url, &target, asset.size, asset.sha256.as_deref()).await;

    UPDATE_DOWNLOADING.store(false, Ordering::SeqCst);

    match result {
        Ok(()) => {
            {
                let mut guard = UPDATE_STATE.lock().unwrap();
                if let Some(s) = guard.as_mut() {
                    s.downloaded_path = Some(target);
                }
            }
            log_download(&format!("[inner] 下载完成 v{}", release.tag_ver));
            emit(app, "update-downloaded", &json!({
                "version": release.tag_ver,
                "releaseName": release.tag,
            }));
            Ok(())
        }
        Err(e) => {
            log_download(&format!("[inner] 下载失败: {}", e));
            emit(app, "update-error", &json!({ "message": e }));
            Err(e)
        }
    }
}

// ============== 安装更新（全量自动替换） ==============

fn install_and_restart(app: &AppHandle, new_pkg: &Path) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = current_exe
        .parent()
        .ok_or("无法定位安装目录")?
        .to_path_buf();
    let exe_name = current_exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    {
        let script = dir.join("_versepc_update.bat");
        let n = new_pkg.to_string_lossy().replace('/', "\\");
        let c = current_exe.to_string_lossy().replace('/', "\\");
        // 等待当前实例退出（约8秒）→ 若后台残留则强制结束 → 替换 exe → 启动 exe → 删除脚本
        // 说明：关窗后进程可能仍在后台占用 exe（导致 move 改名失败），必须兜底 taskkill 强制结束再替换。
        // 加固：替换后先校验 exe 是否真的存在（替换可能被安全软件拦截删除），
        // 存在才启动；若主 exe 丢失则回退启动更新包原件，避免「Windows 找不到文件」原生报错。
        let content = format!(
            "@echo off\r\n\
             chcp 65001 >nul\r\n\
             set \"NEW={new}\"\r\n\
             set \"CUR={cur}\"\r\n\
             set /a tries=0\r\n\
             :wait\r\n\
             tasklist /fi \"imagename eq {exe}\" 2>nul | find /i \"{exe}\" >nul\r\n\
             if errorlevel 1 goto replace\r\n\
             set /a tries+=1\r\n\
             if %tries% geq 8 goto kill\r\n\
             timeout /t 1 /nobreak >nul\r\n\
             goto wait\r\n\
             :kill\r\n\
             taskkill /f /im \"{exe}\" >nul 2>nul\r\n\
             timeout /t 2 /nobreak >nul\r\n\
             :replace\r\n\
             move /y \"%NEW%\" \"%CUR%\" >nul 2>nul\r\n\
             if not exist \"%CUR%\" (\r\n\
               taskkill /f /im \"{exe}\" >nul 2>nul\r\n\
               timeout /t 2 /nobreak >nul\r\n\
               move /y \"%NEW%\" \"%CUR%\" >nul 2>nul\r\n\
             )\r\n\
             if not exist \"%CUR%\" copy /y \"%NEW%\" \"%CUR%\" >nul 2>nul\r\n\
             if exist \"%CUR%\" (\r\n\
               start \"\" \"%CUR%\"\r\n\
             ) else (\r\n\
               if exist \"%NEW%\" start \"\" \"%NEW%\"\r\n\
               if not exist \"%NEW%\" echo [VersePC] update failed: exe missing > \"%~dp0_versepc_update_error.txt\"\r\n\
             )\r\n\
             :done\r\n\
             del /q \"%~f0\"\r\n",
            exe = exe_name, new = n, cur = c
        );
        fs::write(&script, content).map_err(|e| format!("无法写入更新脚本: {}", e))?;
        let script_str = script.to_string_lossy().replace('/', "\\");
        Command::new("cmd")
            .args(["/c", "start", "", "/min", &script_str])
            .spawn()
            .map_err(|e| format!("无法启动更新脚本: {}", e))?;
        app.exit(0);
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        // new_pkg 是单个可执行文件：替换当前二进制后重启
        let script = dir.join("_versepc_update.sh");
        let n = new_pkg.to_string_lossy();
        let c = current_exe.to_string_lossy();
        let content = format!(
            "#!/bin/sh\n\
             while pgrep -f \"{c}\" >/dev/null 2>&1; do sleep 1; done\n\
             cp \"{n}\" \"{c}\" && chmod +x \"{c}\"\n\
             nohup \"{c}\" >/dev/null 2>&1 &\n\
             rm -f \"$0\"\n"
        );
        fs::write(&script, content).map_err(|e| format!("无法写入更新脚本: {}", e))?;
        Command::new("/bin/sh").arg(&script).spawn().map_err(|e| format!("无法启动更新脚本: {}", e))?;
        app.exit(0);
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        // new_pkg 是含 VersePC2.app 的 zip：解压后整包替换旧 .app，再 open 新 app
        let app_bundle = current_exe
            .ancestors()
            .find(|p| p.file_name().map(|f| f == "VersePC2.app").unwrap_or(false))
            .map(|p| p.to_path_buf())
            .unwrap_or(dir.clone());
        let parent = app_bundle
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| dir.clone());
        let script = parent.join("_versepc_update.sh");
        let zip = new_pkg.to_string_lossy();
        let ab = app_bundle.to_string_lossy();
        let content = format!(
            "#!/bin/sh\n\
             while pgrep -f \"{ab}/Contents/MacOS/VersePC2\" >/dev/null 2>&1; do sleep 1; done\n\
             TMP=\"$(mktemp -d)\"\n\
             cd \"$TMP\" && unzip -o -q \"{zip}\" || exit 1\n\
             rm -rf \"{ab}\"\n\
             mv \"$TMP/VersePC2.app\" \"{ab}\"\n\
             chmod +x \"{ab}/Contents/MacOS/VersePC2\"\n\
             open \"{ab}\"\n\
             rm -rf \"$TMP\"\n\
             rm -f \"$0\"\n"
        );
        fs::write(&script, content).map_err(|e| format!("无法写入更新脚本: {}", e))?;
        Command::new("/bin/sh").arg(&script).spawn().map_err(|e| format!("无法启动更新脚本: {}", e))?;
        app.exit(0);
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (exe_name, dir);
        Err("当前平台暂不支持自动更新".to_string())
    }
}

/// 是否存在已下载完成、等待替换生效的更新包
pub fn pending_downloaded_path() -> Option<PathBuf> {
    UPDATE_STATE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.downloaded_path.clone())
}

/// 用户关闭程序时调用：若已有下载完成的更新包，写入一次性"已更新"提示后执行替换退出。
/// 返回 true 表示已进入更新替换流程，调用方应跳过正常的关闭动画/关闭逻辑（进程由安装脚本负责退出并重启）。
pub fn auto_install_on_close(app: &tauri::AppHandle) -> bool {
    let Some(p) = pending_downloaded_path() else {
        return false;
    };
    if !p.exists() {
        return false;
    }
    if let Some(s) = UPDATE_STATE.lock().unwrap().as_ref() {
        write_pending_notice(&s.release.tag_ver, &s.release.body);
    }
    let _ = install_and_restart(app, &p);
    true
}

#[tauri::command]
pub async fn updater_install_update(app: AppHandle) -> Result<Value, String> {
    let (downloaded, release) = {
        let guard = UPDATE_STATE.lock().unwrap();
        (
            guard.as_ref().and_then(|s| s.downloaded_path.clone()),
            guard.as_ref().map(|s| s.release.clone()),
        )
    };
    let path = downloaded.ok_or("更新尚未下载完成")?;
    if !path.exists() {
        return Err("更新文件不存在，请重新下载".into());
    }
    // 升级前写入一次性提示，重启启动时前端据此显示"软件已更新至 xxx 版本"
    if let Some(r) = &release {
        write_pending_notice(&r.tag_ver, &r.body);
    }
    install_and_restart(&app, &path)?;
    Ok(json!({ "success": true }))
}

// ============== 跳过版本 / 打开发布页 ==============

#[tauri::command]
pub fn updater_skip_version(version: String) -> Result<Value, String> {
    save_skipped_version(&version);
    {
        let mut guard = UPDATE_STATE.lock().unwrap();
        *guard = None;
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub fn updater_open_release_page(app: AppHandle) -> Result<Value, String> {
    let _ = app;
    let _ = open::that(RELEASE_PAGE);
    Ok(json!({ "success": true }))
}