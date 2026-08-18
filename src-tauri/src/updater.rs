// updater.rs — 便携版全量自动替换更新
//
// 职责（与 Electron 版 updater 接口对齐，但实现为便携版"全量替换"）：
//   1. 检查更新：读取 GitHub VersPc2 仓库最新 release，比较版本号
//   2. 下载更新：多镜像加速下载新 exe，推送进度与校验
//   3. 安装更新：写替换脚本 → 退出自身 → 脚本替换 exe → 重启
//   4. 跳过版本 / 打开发布页
//
// 前端通过 window.electronAPI.updater.* 触发，状态通过事件 updater:status
// 推送（payload 为 { channel, data }，channel 与 Electron 版一致）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn build_download_sources(url: &str) -> Vec<String> {
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

fn current_version() -> (u32, u32, u32) {
    parse_version(env!("CARGO_PKG_VERSION"))
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

    let files = v.get("files").and_then(|x| x.get("win-x64"));
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

    Ok(json!({ "available": true, "version": release.tag_ver }))
}

// ============== 下载更新 ==============

async fn stream_download(
    app: &AppHandle,
    url: &str,
    target: &Path,
    expected_size: u64,
) -> Result<(), String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("连接失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(expected_size);
    let tmp = target.with_extension("part");
    let mut f = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("创建文件失败: {}", e))?;

    let mut stream = resp.bytes_stream();
    let mut transferred: u64 = 0;
    let mut last_report = Instant::now();
    let mut last_bytes: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {}", e))?;
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
            last_report = Instant::now();
            last_bytes = transferred;
        }
    }

    f.flush().await.map_err(|e| e.to_string())?;
    f.sync_all().await.map_err(|e| e.to_string())?;
    drop(f);

    let _ = fs::remove_file(target);
    fs::rename(&tmp, target).map_err(|e| format!("下载完成但保存失败: {}", e))?;

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
    // 文件已完整且校验通过，直接复用
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
    let _ = fs::remove_file(target.with_extension("part"));

    for (i, murl) in build_download_sources(url).iter().enumerate() {
        match stream_download(app, murl, target, expected_size).await {
            Ok(()) => {
                if verify_file(target, expected_size, expected_sha) {
                    return Ok(());
                }
                let _ = fs::remove_file(target);
                let _ = fs::remove_file(target.with_extension("part"));
            }
            Err(_) => {
                let _ = fs::remove_file(target);
                let _ = fs::remove_file(target.with_extension("part"));
            }
        }
        let _ = i;
    }
    Err("所有下载源均失败，请稍后重试或手动下载".into())
}

#[tauri::command]
pub async fn updater_download_update(app: AppHandle) -> Result<Value, String> {
    let release = {
        let guard = UPDATE_STATE.lock().unwrap();
        guard.as_ref().map(|s| s.release.clone())
    };
    let release = release.ok_or("没有可用的更新信息，请先检查更新")?;
    let asset = release.asset.clone().ok_or("未找到适用于当前平台的安装包")?;

    emit(&app, "start-download", &json!({}));

    let data_dir = crate::storage::resolve_data_dir();
    let tmp_dir = data_dir.join("updates");
    let _ = fs::create_dir_all(&tmp_dir);
    let target = tmp_dir.join(format!("VersePC2-{}.exe", release.tag_ver));

    match download_with_fallback(&app, &asset.url, &target, asset.size, asset.sha256.as_deref()).await {
        Ok(()) => {
            {
                let mut guard = UPDATE_STATE.lock().unwrap();
                if let Some(s) = guard.as_mut() {
                    s.downloaded_path = Some(target);
                }
            }
            emit(&app, "update-downloaded", &json!({
                "version": release.tag_ver,
                "releaseName": release.tag,
            }));
            Ok(json!({ "success": true }))
        }
        Err(e) => {
            emit(&app, "update-error", &json!({ "message": e }));
            Ok(json!({ "success": false, "error": e }))
        }
    }
}

// ============== 安装更新（全量自动替换） ==============

fn install_and_restart(app: &AppHandle, new_exe: &Path) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = current_exe
        .parent()
        .ok_or("无法定位安装目录")?
        .to_path_buf();
    let script = dir.join("_versepc_update.bat");

    let exe_name = current_exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let n = new_exe.to_string_lossy().replace('/', "\\");
    let c = current_exe.to_string_lossy().replace('/', "\\");

    // 脚本流程：等待当前 exe 退出 → 替换 exe → 启动新 exe → 删除脚本
    let content = format!(
        "@echo off\r\nchcp 65001 >nul\r\n:wait\r\ntasklist /fi \"imagename eq {exe}\" | find /i \"{exe}\" >nul\r\nif not errorlevel 1 (\r\n  timeout /t 1 /nobreak >nul\r\n  goto wait\r\n)\r\nmove /y \"{new}\" \"{cur}\" >nul\r\nif errorlevel 1 goto done\r\nstart \"\" \"{cur}\"\r\n:done\r\ndel /q \"%~f0\"\r\n",
        exe = exe_name,
        new = n,
        cur = c
    );
    fs::write(&script, content).map_err(|e| format!("无法写入更新脚本: {}", e))?;

    // 隐藏窗口启动替换脚本（独立进程，不受本应用退出影响）
    let script_str = script.to_string_lossy().replace('/', "\\");
    Command::new("cmd")
        .args(["/c", "start", "", "/min", &script_str])
        .spawn()
        .map_err(|e| format!("无法启动更新脚本: {}", e))?;

    // 退出本应用，交由脚本完成替换
    app.exit(0);
    Ok(())
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