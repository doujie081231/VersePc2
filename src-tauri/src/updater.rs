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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

#[path = "updater_download.rs"]
mod updater_download;
#[path = "updater_install.rs"]
mod updater_install;

use updater_download::{clean_stale_updates, download_with_fallback};
use updater_install::{install_and_restart, pending_notice_path, write_pending_notice};

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

/// 取当前平台对应的更新包条目；清单可以暂时只包含部分平台。
fn current_platform_asset(v: &Value) -> Option<&Value> {
    v.get("files")?.get(current_update_key())
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

// 更新配置（跳过版本）路径：便携数据目录/updater-config.json
fn update_config_path() -> PathBuf {
    crate::storage::resolve_data_dir().join("updater-config.json")
}

// 自更新重启后的一次性"已更新提示"文件：升级前写入，重启启动时前端读取并删除
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

/// 更新包地址白名单：只接受本项目在 Gitee / GitHub 的 release 直链。
/// update.json 会经过第三方加速代理，不能信任其中任意 URL（审计 P1-1）。
fn is_trusted_update_asset(url: &str) -> bool {
    url.starts_with("https://github.com/doujie081231/VersePc2/releases/download/")
        || url.starts_with("https://gitee.com/doujie081231/verse-pc2/releases/download/")
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// update.json 发布签名公钥（Ed25519，原始 32 字节的 base64）。
/// 私钥只保存在发布者本机，绝不进入仓库；对应签名脚本见 scripts/sign-update.mjs。
const UPDATE_PUBLIC_KEY_B64: &str = "HY8db8m9lz2RZe5gIaUj9ib971FG/L/4/HR3G7U2IZE=";

/// 规范化字段转义：与 scripts/sign-update.mjs 中的 escapeField 完全一致。
fn canonical_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// 生成签名所覆盖的规范化文本（不依赖 JSON 序列化实现细节）。
/// 字段按固定顺序输出，files 按平台名排序，避免不同实现/键顺序导致验签失败。
fn canonical_update_payload(v: &Value) -> Vec<u8> {
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("");
    let mut out = String::new();
    out.push_str("verse-update-v1\n");
    out.push_str(&format!("version={}\n", canonical_escape(field("version"))));
    out.push_str(&format!(
        "releaseName={}\n",
        canonical_escape(field("releaseName"))
    ));
    out.push_str(&format!(
        "releaseDate={}\n",
        canonical_escape(field("releaseDate"))
    ));
    out.push_str(&format!(
        "releaseNotes={}\n",
        canonical_escape(field("releaseNotes"))
    ));

    let files = v.get("files").and_then(|x| x.as_object());
    let mut platforms: Vec<&String> = files.map(|m| m.keys().collect()).unwrap_or_default();
    platforms.sort();
    for p in platforms {
        let f = files.and_then(|m| m.get(p)).and_then(|x| x.as_object());
        let url = f
            .and_then(|m| m.get("url"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let sha = f
            .and_then(|m| m.get("sha256"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let size = f
            .and_then(|m| m.get("size"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        out.push_str(&format!(
            "file\t{}\t{}\t{}\t{}\n",
            canonical_escape(p),
            canonical_escape(url),
            canonical_escape(&sha),
            size
        ));
    }
    out.into_bytes()
}

/// 校验 update.json 的 Ed25519 签名；缺失或无效一律拒绝（更新链路经过第三方代理）。
fn verify_update_signature(v: &Value, signature_b64: &str) -> bool {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let (Ok(sig), Ok(key)) = (
        STANDARD.decode(signature_b64.trim()),
        STANDARD.decode(UPDATE_PUBLIC_KEY_B64),
    ) else {
        return false;
    };
    if sig.len() != 64 || key.len() != 32 {
        return false;
    }
    let payload = canonical_update_payload(v);
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key.as_slice())
        .verify(&payload, &sig)
        .is_ok()
}

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

    // 更新清单必须携带发布者 Ed25519 签名：清单会经过 ghfast/ghproxy 等第三方代理，
    // 仅靠 HTTPS/白名单无法防止代理篡改版本号、说明或下载地址（审计 P1-1）。
    let signature = v.get("signature").and_then(|x| x.as_str()).unwrap_or("");
    if !verify_update_signature(&v, signature) {
        log_download(&format!("[check] 拒绝未签名或签名无效的更新清单: {}", url));
        return Ok(None);
    }

    let version = v
        .get("version")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if version.is_empty() {
        return Ok(None);
    }

    // 依据当前运行平台选择对应的文件键；缺失则视为该源不适用于本平台。
    // 这样可以先发布 Windows，macOS/Linux 构建完成后再补充各自的条目。
    let Some(f) = current_platform_asset(&v) else {
        log_download(&format!(
            "[check] 清单缺少当前平台 {} 的安装包，跳过: {}",
            current_update_key(),
            url
        ));
        return Ok(None);
    };
    let asset_url = f.get("url").and_then(|x| x.as_str()).unwrap_or("");
    let sha = f.get("sha256").and_then(|x| x.as_str()).unwrap_or("");
    // 不可信的下载地址或缺失哈希：整份清单位于第三方代理链路上，直接换源/失败关闭
    if !is_trusted_update_asset(asset_url) || !is_valid_sha256(sha) {
        log_download(&format!("[check] 拒绝不可信的更新包: url={} sha256={}", asset_url, sha));
        return Ok(None);
    }
    let asset = Some(UpdateAsset {
        url: asset_url.to_string(),
        size: f.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
        sha256: Some(sha.to_string()),
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

    // 哪个源先返回有效结果就用哪个，其余请求立即取消；比等待全部源返回更快、更省资源
    let mut tasks = FuturesUnordered::new();
    for url in UPDATE_JSON_SOURCES {
        tasks.push(try_fetch_source(&client, url));
    }
    while let Some(r) = tasks.next().await {
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
    // 延后 2 秒再启动，避免与启动阶段的 Java 探测、首屏渲染同时抢占 CPU/磁盘。
    let app2 = app.clone();
    let rel2 = release.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let _ = download_update_inner(&app2, &rel2).await;
    });

    Ok(json!({ "available": true, "version": release.tag_ver }))
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

    // 清理超过 7 天的旧安装包/断点文件，避免更新缓存长期占用磁盘（当前目标与断点保留）
    clean_stale_updates(&tmp_dir, &target);

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

/// 是否存在已下载完成、等待替换生效的更新包
pub fn pending_downloaded_path() -> Option<PathBuf> {
    UPDATE_STATE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.downloaded_path.clone())
}

/// 用户关闭程序时调用：若已有下载完成的更新包，执行替换退出。
/// 返回 true 表示已进入更新替换流程，调用方应跳过正常的关闭动画/关闭逻辑（进程由安装脚本负责退出并重启）。
/// 安装脚本启动失败时返回 false，让调用方继续正常关闭——与 PCL 一致：
/// 更新失败只记录错误，绝不把“关闭程序”一起卡住。
pub fn auto_install_on_close(app: &tauri::AppHandle) -> bool {
    let Some(p) = pending_downloaded_path() else {
        return false;
    };
    if !p.exists() {
        return false;
    }
    // 先写一次性"已更新"提示：安装脚本要等本进程退出后才替换并启动新版本，
    // 必须保证文件在退出前落盘；安装失败时再删掉，避免下次启动误报已更新
    if let Some(s) = UPDATE_STATE.lock().unwrap().as_ref() {
        write_pending_notice(&s.release.tag_ver, &s.release.body);
    }
    if let Err(e) = install_and_restart(app, &p) {
        let _ = fs::remove_file(pending_notice_path());
        log_download(&format!("[install] 关闭时安装更新失败，改为正常退出: {}", e));
        return false;
    }
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
    // 先写一次性"已更新"提示（重启启动时前端据此显示"软件已更新至 xxx 版本"），
    // 安装脚本要等本进程退出后才执行，因此必须在退出前落盘
    if let Some(r) = &release {
        write_pending_notice(&r.tag_ver, &r.body);
    }
    if let Err(e) = install_and_restart(&app, &path) {
        // 安装未启动时删掉提示，避免下次启动误报已更新
        let _ = fs::remove_file(pending_notice_path());
        return Err(e);
    }
    Ok(json!({ "success": true }))
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

#[cfg(test)]
mod update_asset_tests {
    use super::*;

    #[test]
    fn only_project_release_urls_are_trusted() {
        assert!(is_trusted_update_asset(
            "https://gitee.com/doujie081231/verse-pc2/releases/download/v1.8.5/VersePC2.exe"
        ));
        assert!(is_trusted_update_asset(
            "https://github.com/doujie081231/VersePc2/releases/download/v1.8.5/VersePC2.exe"
        ));
        assert!(!is_trusted_update_asset("https://evil.example.com/VersePC2.exe"));
        assert!(!is_trusted_update_asset(
            "https://github.com/other/repo/releases/download/v1/VersePC2.exe"
        ));
        assert!(!is_trusted_update_asset("http://gitee.com/doujie081231/verse-pc2/releases/download/v1/x"));
    }

    #[test]
    fn sha256_must_be_64_hex_chars() {
        assert!(is_valid_sha256(
            "17aa01d6b05cfdc4d277ba44fcf5b733f1e014fad0c6e0e17dc7c3bcfcc9a325"
        ));
        assert!(!is_valid_sha256(""));
        assert!(!is_valid_sha256("abc"));
        assert!(!is_valid_sha256(
            "17aa01d6b05cfdc4d277ba44fcf5b733f1e014fad0c6e0e17dc7c3bcfcc9a32Z"
        ));
    }

    const SHIPPED_MANIFEST: &str = include_str!("../../update.json");

    fn shipped_signature(v: &Value) -> String {
        v.get("signature")
            .and_then(|x| x.as_str())
            .expect("update.json 缺少 signature，请先运行 scripts/sign-update.mjs")
            .to_string()
    }

    #[test]
    fn shipped_manifest_signature_verifies() {
        let v: Value = serde_json::from_str(SHIPPED_MANIFEST).expect("update.json 解析失败");
        let sig = shipped_signature(&v);
        assert!(verify_update_signature(&v, &sig));
    }

    #[test]
    fn tampered_version_or_asset_url_fails_verification() {
        let v: Value = serde_json::from_str(SHIPPED_MANIFEST).expect("update.json 解析失败");
        let sig = shipped_signature(&v);

        let mut changed_version = v.clone();
        changed_version["version"] = json!("9.9.9");
        assert!(!verify_update_signature(&changed_version, &sig));

        let mut changed_url = v.clone();
        changed_url["files"]["win-x64"]["url"] = json!("https://evil.example.com/VersePC2.exe");
        assert!(!verify_update_signature(&changed_url, &sig));
    }

    #[test]
    fn manifest_without_current_platform_asset_is_skipped() {
        let v = json!({
            "version": "9.9.9",
            "files": {
                "other-platform": {
                    "url": "https://gitee.com/doujie081231/verse-pc2/releases/download/v9.9.9/VersePC2.exe",
                    "size": 1,
                    "sha256": "17aa01d6b05cfdc4d277ba44fcf5b733f1e014fad0c6e0e17dc7c3bcfcc9a325"
                }
            }
        });
        assert!(current_platform_asset(&v).is_none());
    }

    #[test]
    fn missing_or_malformed_signature_fails_verification() {
        let v: Value = serde_json::from_str(SHIPPED_MANIFEST).expect("update.json 解析失败");
        assert!(!verify_update_signature(&v, ""));
        assert!(!verify_update_signature(&v, "not-base64"));
        assert!(!verify_update_signature(&v, "AAAA"));
    }
}
