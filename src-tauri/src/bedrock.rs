// bedrock.rs — 基岩版（MineBBS 清单 + 123 云盘分享解析下载 + 本地启动）
// 清单：MineBBS 平台 platform=3 / environment=1（Windows 基岩版）
// 下载：先带 Referer 解析 MineBBS 跳转拿到 123 分享页，再按 123 网页协议本地解析。
// 登录态：用户在客户端内登录一次 123 云盘，authorization 只存在本地，不经过任何服务器。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::api::ApiResult;
use crate::storage;
use crate::utils;

const MINEBBS_API_BASE: &str =
    "https://api.mc.minebbs.com/api/v1/versions/?pageSize=10&platform=3&environment=1";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const CHAR_MAP: [char; 26] = [
    'a', 'd', 'e', 'f', 'g', 'h', 'l', 'm', 'y', 'i', 'j', 'n', 'o', 'p', 'k', 'q', 'r', 's',
    't', 'u', 'b', 'c', 'v', 'w', 's', 'z',
];

// ==================== 小工具 ====================

fn data_dir() -> PathBuf {
    storage::resolve_data_dir()
}

fn token_path() -> PathBuf {
    data_dir().join("123pan-token.txt")
}

fn read_token() -> String {
    std::fs::read_to_string(token_path())
        .map(|s| s.trim().trim_start_matches("Bearer ").to_string())
        .unwrap_or_default()
}

fn save_token(token: &str) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(token_path(), token.trim());
}

fn versions_dir() -> PathBuf {
    data_dir().join("versions")
}

fn marker_path(version_id: &str) -> PathBuf {
    versions_dir()
        .join(storage::sanitize_version_id(version_id))
        .join("bedrock.json")
}

pub fn is_bedrock(version_id: &str) -> bool {
    marker_path(version_id).exists()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn http_client(timeout_secs: u64, redirect: bool) -> reqwest::blocking::Client {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(UA);
    if !redirect {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    builder.build().unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// CRC32（与 123 网页 JS 的 crc32 一致，返回十进制）
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// 123 接口签名：?{authK}={authV}
fn pan_sign(path: &str) -> String {
    use chrono::{FixedOffset, Utc};
    let offset = FixedOffset::east_opt(8 * 3600).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    let now = Utc::now().with_timezone(&offset);
    let ts = now.timestamp();
    let time_str = now.format("%Y%m%d%H%M").to_string();
    let mapped: String = time_str
        .chars()
        .map(|c| {
            let digit = c.to_digit(10).unwrap_or(0) as usize;
            CHAR_MAP[digit]
        })
        .collect();
    let auth_k = crc32(mapped.as_bytes());
    let random_part = (rand::random::<f64>() * 10_000_000.0).round() as u64;
    let raw = format!("{}|{}|{}|web|3|{}", ts, random_part, path, auth_k);
    let hash = crc32(raw.as_bytes());
    format!("?{}={}-{}-{}", auth_k, ts, random_part, hash)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(char::from_u32((h * 16 + l) as u32).unwrap_or('%'));
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn str_field(v: &Value, names: &[&str]) -> String {
    for name in names {
        if let Some(s) = v.get(*name).and_then(|x| x.as_str())
            && !s.is_empty()
        {
            return s.to_string();
        }
        if let Some(n) = v.get(*name).and_then(|x| x.as_i64()) {
            return n.to_string();
        }
    }
    String::new()
}

/// 清洗 Windows 非法文件名字符
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "bedrock".to_string()
    } else {
        trimmed.to_string()
    }
}

// ==================== 123 登录态（本地 WebView 登录） ====================

#[derive(Clone, Default)]
struct LoginState {
    status: String, // idle / pending / success / failed
    message: String,
}

fn login_state() -> &'static Mutex<LoginState> {
    static STATE: OnceLock<Mutex<LoginState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(LoginState::default()))
}

fn set_login(status: &str, message: &str) {
    if let Ok(mut s) = login_state().lock() {
        s.status = status.to_string();
        s.message = message.to_string();
    }
}

fn close_login_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("bedrock-123-login") {
        let _ = win.close();
    }
}

fn handle_login_request(app: AppHandle, request_line: &str) {
    // 形如：GET /collect?t=xxx HTTP/1.1
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    if let Some(pos) = path.find("t=") {
        let raw = &path[pos + 2..];
        let token = percent_decode(raw.split('&').next().unwrap_or(raw))
            .trim()
            .trim_start_matches("Bearer ")
            .to_string();
        if token.len() > 8 {
            save_token(&token);
            set_login("success", "登录成功");
            close_login_window(&app);
            return;
        }
    }
    set_login("failed", "没有读到登录态，请重新登录");
}

fn start_login_browser(app: &AppHandle) -> Result<(), String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法创建本地接收端口：{}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("无法获取本地端口：{}", e))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("本地端口初始化失败：{}", e))?;

    set_login("pending", "请在弹出的窗口里登录 123 云盘");

    let poll_js = format!(
        r#"(function(){{try{{if(window.__verse123)return;window.__verse123=setInterval(function(){{try{{var t=localStorage.getItem('authorization')||localStorage.getItem('token')||sessionStorage.getItem('authorization');if(!t){{var m=document.cookie.match(/authorization=([^;]+)/);if(m)t=decodeURIComponent(m[1]);}}if(t&&t.length>10){{fetch('http://127.0.0.1:{}/collect?t='+encodeURIComponent(t),{{mode:'no-cors'}});}}}}catch(e){{}}}},1500);}}catch(e){{}}}})()"#,
        port
    );

    let app_accept = app.clone();
    let js_for_thread = poll_js.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut last_eval = Instant::now();
        loop {
            if started.elapsed() > Duration::from_secs(300) {
                set_login("failed", "登录超时，请重试");
                close_login_window(&app_accept);
                break;
            }
            // 登录页会跳转，跳转后 JS 定时器会丢，所以定期重新注入
            if last_eval.elapsed() >= Duration::from_millis(1500) {
                last_eval = Instant::now();
                if let Some(win) = app_accept.get_webview_window("bedrock-123-login") {
                    let _ = win.eval(&js_for_thread);
                }
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    use std::io::{Read, Write};
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
                    handle_login_request(app_accept.clone(), &line);
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(120));
                }
                Err(_) => break,
            }
        }
    });

    let app_build = app.clone();
    let js_for_window = poll_js.clone();
    let _ = app.run_on_main_thread(move || {
        let url = "https://www.123pan.com/".parse().unwrap();
        let builder = tauri::WebviewWindowBuilder::new(
            &app_build,
            "bedrock-123-login",
            tauri::WebviewUrl::External(url),
        )
        .title("登录 123 云盘（登录后会自动关闭）")
        .inner_size(1080.0, 760.0);

        match builder.build() {
            Ok(win) => {
                let _ = win.eval(&js_for_window);
            }
            Err(e) => set_login("failed", &format!("无法打开登录窗口：{}", e)),
        }
    });

    Ok(())
}

// ==================== MineBBS 清单 ====================

fn cache_path() -> PathBuf {
    data_dir().join("cache").join("bedrock-versions.json")
}

fn load_cache() -> Option<Vec<Value>> {
    let meta = std::fs::metadata(cache_path()).ok()?;
    let modified = meta.modified().ok()?;
    if modified.elapsed().ok()?.as_secs() > 6 * 3600 {
        return None;
    }
    let text = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str::<Value>(&text)
        .ok()?
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
}

fn fetch_manifest(force: bool) -> Result<Vec<Value>, String> {
    if !force && let Some(list) = load_cache() {
        return Ok(list);
    }
    let client = http_client(10, false);
    let mut all: Vec<Value> = Vec::new();
    let mut page = 1u32;
    loop {
        let url = format!("{}&page={}", MINEBBS_API_BASE, page);
        let resp = client
            .get(&url)
            .header("Referer", "https://mc.minebbs.com/")
            .send()
            .map_err(|e| format!("无法连接 MineBBS：{}", e))?;
        if !resp.status().is_success() {
            return Err(format!("MineBBS 返回 HTTP {}", resp.status()));
        }
        let text = resp.text().map_err(|e| format!("读取清单失败：{}", e))?;
        let root: Value = serde_json::from_str(&text).map_err(|e| format!("清单格式异常：{}", e))?;
        let list = root
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        if list.is_empty() {
            break;
        }
        all.extend(list);
        let total_pages = root
            .get("totalPages")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as u32;
        if page >= total_pages || page >= 12 {
            break;
        }
        page += 1;
    }
    if all.is_empty() {
        return Err("MineBBS 没有返回版本数据".to_string());
    }
    let _ = std::fs::create_dir_all(cache_path().parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(cache_path(), json!({ "data": all.clone() }).to_string());
    Ok(all)
}

fn installed_versions() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(versions_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("bedrock.json").exists()
                && let Some(name) = entry.file_name().to_str()
            {
                out.push(name.to_string());
            }
        }
    }
    out
}

// ==================== 123 分享解析 ====================

fn resolve_share_url(link: &str) -> Result<String, String> {
    let url = if link.starts_with("//") { format!("https:{}", link) } else { link.to_string() };
    let client = http_client(20, false);
    let resp = client
        .get(&url)
        .header("Referer", "https://mc.minebbs.com/")
        .send()
        .map_err(|e| format!("请求 MineBBS 下载跳转失败：{}", e))?;
    if resp.status().is_redirection() {
        if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
            return Ok(loc.to_string());
        }
    }
    // 有些版本会直接给 123 地址
    let text = resp.text().unwrap_or_default();
    if text.contains("123pan.com") || text.contains("123912.com") {
        return Ok(text.trim().to_string());
    }
    Err("MineBBS 没有跳转到 123 云盘（分享链接可能已失效）".to_string())
}

fn parse_share(url: &str) -> Result<(String, String, String, String), String> {
    // 返回 (origin, shareKey, pwd, sharePageUrl)
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("分享地址异常：{}", e))?;
    let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("www.123pan.com"));
    let path = parsed.path().trim_end_matches('/');
    let key = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".html")
        .to_string();
    if key.is_empty() {
        return Err("无法从链接里识别分享编号".to_string());
    }
    let pwd = parsed
        .query_pairs()
        .find(|(k, _)| k.as_ref() == "pwd" || k.as_ref() == "password")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    Ok((origin, key, pwd, url.to_string()))
}

struct ShareItem {
    file_id: Value,
    name: String,
    size: u64,
    s3_key_flag: String,
    etag: String,
    is_folder: bool,
}

fn list_share_files(origin: &str, key: &str, pwd: &str, referer: &str, token: &str) -> Result<Vec<ShareItem>, String> {
    let api = format!(
        "{}/a/api/share/get?limit=100&next=1&orderBy=file_name&orderDirection=asc&shareKey={}&SharePwd={}&ParentFileId=0&Page=1&event=homeListFile&operateType=1",
        origin, urlencoding::encode(key), urlencoding::encode(pwd)
    );
    let client = http_client(30, true);
    let mut req = client
        .get(&api)
        .header("Platform", "web")
        .header("App-Version", "3")
        .header("Referer", referer)
        .header("Origin", origin);
    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let resp = req.send().map_err(|e| format!("读取分享文件列表失败：{}", e))?;
    let text = resp.text().map_err(|e| format!("读取分享文件列表失败：{}", e))?;
    let root: Value = serde_json::from_str(&text).map_err(|e| format!("分享返回格式异常：{}", e))?;
    if api_code(&root) != 0 {
        let msg = root.get("message").and_then(|v| v.as_str()).unwrap_or("分享解析失败");
        return Err(msg.to_string());
    }
    let data = root.get("data").cloned().unwrap_or(Value::Null);
    let arr = data
        .get("Info")
        .or_else(|| data.get("info"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for item in arr {
        let file_id = item
            .get("FileId")
            .or_else(|| item.get("fileId"))
            .cloned()
            .unwrap_or(Value::Null);
        let file_type = item
            .get("Type")
            .or_else(|| item.get("type"))
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        if file_id.is_null() {
            continue;
        }
        items.push(ShareItem {
            file_id,
            name: str_field(&item, &["FileName", "fileName", "name"]),
            size: item.get("Size").or_else(|| item.get("size")).and_then(|v| v.as_u64()).unwrap_or(0),
            s3_key_flag: str_field(&item, &["S3KeyFlag", "s3KeyFlag"]),
            etag: str_field(&item, &["Etag", "etag"]),
            is_folder: file_type == 1,
        });
    }
    if items.is_empty() {
        return Err("分享里没有找到可下载的文件".to_string());
    }
    Ok(items)
}

fn decode_download_url(raw: &str) -> Result<String, String> {
    use base64::Engine;
    let parsed = reqwest::Url::parse(raw).map_err(|e| format!("下载地址异常：{}", e))?;
    let params = match parsed.query_pairs().find(|(k, _)| k.as_ref() == "params") {
        Some((_, v)) => v.to_string(),
        None => return Ok(raw.to_string()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(params.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(params.as_bytes()))
        .map_err(|e| format!("下载参数解码失败：{}", e))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        for key in ["url", "downloadUrl", "redirect_url"] {
            if let Some(url) = v.get(key).and_then(|u| u.as_str()) {
                return Ok(url.to_string());
            }
        }
        if let Some(obj) = v.as_object() {
            for value in obj.values() {
                if let Some(url) = value.as_str()
                    && url.starts_with("http")
                {
                    return Ok(url.to_string());
                }
            }
        }
    }
    if text.starts_with("http") {
        return Ok(text);
    }
    Err("下载参数里没有找到地址".to_string())
}

fn fetch_final_url(app: &AppHandle, session_id: &str, url: &str, referer: &str) -> Result<String, String> {
    let client = http_client(60, true);
    let resp = client
        .get(url)
        .header("Referer", referer)
        .header("User-Agent", UA)
        .send()
        .map_err(|e| format!("获取下载地址失败：{}", e))?;
    if !resp.status().is_success() {
        return Err(format!("获取下载地址失败，HTTP {}", resp.status()));
    }
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let text = resp.text().map_err(|e| format!("读取下载信息失败：{}", e))?;
    if !content_type.contains("json") {
        return Ok(final_url);
    }
    let root: Value = serde_json::from_str(&text).map_err(|_| "123 返回的下载信息不是 JSON（可能需要登录或付费）".to_string())?;
    if api_code(&root) != 0 {
        let msg = root
            .get("message")
            .or_else(|| root.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("下载被拒绝");
        update_session(app, session_id, |s| {
            s.message = format!("123 云盘：{}", msg);
        });
        return Err(msg.to_string());
    }
    root.get("data")
        .and_then(|d| d.get("redirect_url").or_else(|| d.get("redirectUrl")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "下载信息里没有 redirect_url（可能需要登录或付费）".to_string())
}

fn request_download_url(
    origin: &str, key: &str, item: &ShareItem, referer: &str, token: &str,
) -> Result<String, String> {
    let client = http_client(30, true);
    let (api, body) = if item.is_folder {
        (
            format!("{}/b/api/file/batch_download_share_info{}", origin, pan_sign("/b/api/file/batch_download_share_info")),
            json!({ "ShareKey": key, "fileIdList": [{ "fileId": item.file_id }] }),
        )
    } else {
        (
            format!("{}/a/api/share/download/info{}", origin, pan_sign("/a/api/share/download/info")),
            json!({
                "ShareKey": key,
                "FileID": item.file_id,
                "S3keyFlag": item.s3_key_flag,
                "Size": item.size,
                "Etag": item.etag,
            }),
        )
    };
    let mut req = client
        .post(&api)
        .header("Platform", "web")
        .header("App-Version", "3")
        .header("Referer", referer)
        .header("Origin", origin)
        .json(&body);
    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let resp = req.send().map_err(|e| format!("请求下载地址失败：{}", e))?;
    let text = resp.text().map_err(|e| format!("读取下载地址失败：{}", e))?;
    let root: Value = serde_json::from_str(&text).map_err(|e| format!("下载接口返回格式异常：{}", e))?;
    if api_code(&root) != 0 {
        let msg = root
            .get("message")
            .or_else(|| root.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("123 云盘拒绝了下载请求");
        return Err(msg.to_string());
    }
    let data = root.get("data").cloned().unwrap_or(Value::Null);
    let raw = data
        .get("DownloadUrl")
        .or_else(|| data.get("DownloadURL"))
        .or_else(|| data.get("downloadUrl"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "下载接口没有返回地址".to_string())?;
    decode_download_url(raw)
}

// ==================== 安装会话 ====================

#[derive(Default)]
struct BedrockSession {
    status: String,
    progress: u32,
    message: String,
    downloaded: u64,
    total: u64,
    speed: u64,
    version: String,
    error: String,
    cancel: Option<Arc<AtomicBool>>,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, BedrockSession>>> = OnceLock::new();

fn sessions() -> std::sync::MutexGuard<'static, HashMap<String, BedrockSession>> {
    SESSIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
}

fn update_session<F: FnOnce(&mut BedrockSession)>(_app: &AppHandle, id: &str, f: F) {
    let mut map = sessions();
    if let Some(s) = map.get_mut(id) {
        f(s);
    }
}

fn api_code(v: &Value) -> i64 {
    v.get("code")
        .and_then(|c| {
            c.as_i64()
                .or_else(|| c.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
        .unwrap_or(-1)
}

fn session_json(id: &str) -> Value {
    let map = sessions();
    match map.get(id) {
        Some(s) => json!({
            "sessionId": id,
            "status": s.status,
            "progress": s.progress,
            "message": s.message,
            "downloadedBytes": s.downloaded,
            "totalBytes": s.total,
            "speed": s.speed,
            "version": s.version,
            "error": s.error,
        }),
        None => json!({
            "sessionId": id,
            "status": "completed",
            "progress": 100,
            "message": "会话已结束",
        }),
    }
}

fn find_download_item(items: &[ShareItem], version: &str) -> Option<usize> {
    let matches_version = |item: &ShareItem| {
        let parts: Vec<&str> = version.split('.').collect();
        let name_parts: Vec<&str> = item
            .name
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .collect();
        if name_parts.len() >= 2
            && parts.first().is_some_and(|p| *p == name_parts[0])
            && parts.get(1).is_some_and(|p| *p == name_parts[1])
        {
            return true;
        }
        item.name.contains(version) || version.contains(item.name.as_str())
    };
    items
        .iter()
        .position(|i| i.is_folder && matches_version(i))
        .or_else(|| items.iter().position(|i| i.is_folder))
        .or_else(|| items.iter().position(|i| matches_version(i)))
        .or_else(|| items.iter().position(|_| true))
}

fn common_top_dir(names: &[String]) -> Option<String> {
    let first = names.iter().find(|n| !n.is_empty())?;
    let prefix = first.split('/').next().unwrap_or("");
    if prefix.is_empty() || !first.contains('/') {
        return None;
    }
    let dir_prefix = format!("{}/", prefix);
    for name in names {
        if name.is_empty() {
            continue;
        }
        let value = name.as_str();
        if value == prefix
            || value == dir_prefix.as_str()
            || value.starts_with(dir_prefix.as_str())
        {
            continue;
        }
        return None;
    }
    Some(prefix.to_string())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<PathBuf, String> {
    use std::io::{Read, Write};
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开压缩包失败：{}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("压缩包无效：{}", e))?;

    let mut names: Vec<String> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| format!("读取压缩包失败：{}", e))?;
        names.push(entry.name().replace('\\', "/"));
    }
    let prefix = common_top_dir(&names);

    std::fs::create_dir_all(dest).map_err(|e| format!("创建版本目录失败：{}", e))?;
    for (i, raw) in names.iter().enumerate() {
        if raw.starts_with('/') || raw.contains("..") || raw.contains(':') {
            continue;
        }
        let mut entry = archive.by_index(i).map_err(|e| format!("读取压缩包失败：{}", e))?;
        let relative = match &prefix {
            Some(p) => {
                let strip_prefix = format!("{}/", p);
                raw.strip_prefix(strip_prefix.as_str()).unwrap_or(raw.as_str())
            }
            None => raw.as_str(),
        };
        if relative.is_empty() {
            continue;
        }
        let target = dest.join(relative);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&target);
            continue;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| format!("写入文件失败：{}", e))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = entry.read(&mut buf).map_err(|e| format!("解压失败：{}", e))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("写入文件失败：{}", e))?;
        }
    }

    // 找启动 exe：优先客户端，其次服务端，最后任意 exe
    let candidates = walk_exe(dest);
    let pick = candidates
        .iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("").eq_ignore_ascii_case("Minecraft.Windows.exe"))
        .or_else(|| candidates.iter().find(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("").eq_ignore_ascii_case("bedrock_server.exe")))
        .or_else(|| candidates.first())
        .cloned();
    pick.ok_or_else(|| "压缩包里没有找到可执行文件".to_string())
}

fn walk_exe(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_exe(&path));
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            out.push(path);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn install_task(
    app: AppHandle, session_id: String, version: String, link: String, cancel: Arc<AtomicBool>,
) {
    let stage = |p: u32, msg: &str| {
        update_session(&app, &session_id, |s| {
            s.progress = p;
            s.message = msg.to_string();
        });
    };

    if cancel.load(Ordering::Relaxed) {
        update_session(&app, &session_id, |s| { s.status = "cancelled".into(); s.message = "已取消".into(); });
        return;
    }

    stage(3, "正在解析下载链接...");
    let share_url = match resolve_share_url(&link) {
        Ok(v) => v,
        Err(e) => return fail(&app, &session_id, &e),
    };
    let (origin, key, pwd, referer) = match parse_share(&share_url) {
        Ok(v) => v,
        Err(e) => return fail(&app, &session_id, &e),
    };

    stage(8, "正在读取分享文件列表...");
    let token = read_token();
    let items = match list_share_files(&origin, &key, &pwd, &referer, &token) {
        Ok(v) => v,
        Err(e) => return fail(&app, &session_id, &e),
    };
    let index = match find_download_item(&items, &version) {
        Some(i) => i,
        None => return fail(&app, &session_id, "分享里没有匹配的版本文件"),
    };
    let item = &items[index];

    if token.is_empty() {
        return fail(&app, &session_id, "请先登录 123 云盘再下载（客户端不会上传你的账号信息）");
    }

    stage(12, "正在申请下载地址...");
    let raw_url = match request_download_url(&origin, &key, item, &referer, &token) {
        Ok(v) => v,
        Err(e) => return fail(&app, &session_id, &e),
    };
    let final_url = match fetch_final_url(&app, &session_id, &raw_url, &referer) {
        Ok(v) => v,
        Err(e) => return fail(&app, &session_id, &e),
    };

    let tmp_dir = data_dir().join("cache").join("bedrock-download");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let zip_path = tmp_dir.join(format!("{}.zip", safe_name(&version)));
    stage(15, "开始下载...");
    let client = http_client(3600, true);
    let mut resp = match client
        .get(&final_url)
        .header("Referer", &referer)
        .header("User-Agent", UA)
        .send()
    {
        Ok(r) => r,
        Err(e) => return fail(&app, &session_id, &format!("下载失败：{}", e)),
    };
    if !resp.status().is_success() {
        return fail(&app, &session_id, &format!("下载失败，HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    update_session(&app, &session_id, |s| s.total = total);
    let mut out = match std::fs::File::create(&zip_path) {
        Ok(f) => f,
        Err(e) => return fail(&app, &session_id, &format!("创建临时文件失败：{}", e)),
    };
    use std::io::{Read, Write};
    let mut received: u64 = 0;
    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut buf = [0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = std::fs::remove_file(&zip_path);
            update_session(&app, &session_id, |s| { s.status = "cancelled".into(); s.message = "已取消".into(); });
            return;
        }
        let n = match resp.read(&mut buf) {
            Ok(n) => n,
            Err(e) => return fail(&app, &session_id, &format!("下载中断：{}", e)),
        };
        if n == 0 {
            break;
        }
        if out.write_all(&buf[..n]).is_err() {
            return fail(&app, &session_id, "写入下载文件失败");
        }
        received += n as u64;
        if last_report.elapsed() >= Duration::from_millis(600) {
            last_report = Instant::now();
            let speed = if started.elapsed().as_secs_f64() > 0.1 {
                (received as f64 / started.elapsed().as_secs_f64()) as u64
            } else {
                0
            };
            let pct = if total > 0 {
                (15 + received.min(total) * 80 / total) as u32
            } else {
                50
            };
            update_session(&app, &session_id, |s| {
                s.downloaded = received;
                s.speed = speed;
                s.progress = pct;
                s.message = if total > 0 {
                    format!("下载中 {:.1}MB / {:.1}MB", received as f64 / 1048576.0, total as f64 / 1048576.0)
                } else {
                    format!("下载中 {:.1}MB", received as f64 / 1048576.0)
                };
            });
        }
    }
    let _ = out.flush();
    drop(out);

    stage(96, "正在解压安装...");
    let version_dir = versions_dir().join(storage::sanitize_version_id(&version));
    let exe = match extract_zip(&zip_path, &version_dir) {
        Ok(exe) => exe,
        Err(e) => return fail(&app, &session_id, &e),
    };
    let relative_exe = exe
        .strip_prefix(&version_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| exe.to_string_lossy().to_string());

    let manifest = json!({
        "version": version,
        "exe": relative_exe,
        "installedAt": now_ms(),
        "source": share_url,
    });
    if let Err(e) = std::fs::write(version_dir.join("bedrock.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default()) {
        return fail(&app, &session_id, &format!("写入安装标记失败：{}", e));
    }
    let _ = std::fs::remove_file(&zip_path);

    update_session(&app, &session_id, |s| {
        s.status = "completed".into();
        s.progress = 100;
        s.message = "安装完成".into();
        s.downloaded = received;
    });
}

fn fail(app: &AppHandle, session_id: &str, msg: &str) {
    update_session(app, session_id, |s| {
        s.status = "failed".into();
        s.error = msg.to_string();
        s.message = msg.to_string();
    });
}

// ==================== 启动 ====================

pub fn launch(version_id: &str) -> Result<String, String> {
    let marker = marker_path(version_id);
    let text = std::fs::read_to_string(&marker).map_err(|e| format!("读取基岩版安装信息失败：{}", e))?;
    let info: Value = serde_json::from_str(&text).map_err(|e| format!("基岩版安装信息损坏：{}", e))?;
    let exe_rel = utils::get_str(&info, "exe");
    if exe_rel.is_empty() {
        return Err("安装信息里没有启动程序".to_string());
    }
    let version_dir = marker.parent().unwrap_or(Path::new(".")).to_path_buf();
    let exe = version_dir.join(&exe_rel);
    if !exe.exists() {
        return Err(format!("启动程序不存在：{}", exe.display()));
    }
    std::process::Command::new(&exe)
        .current_dir(&version_dir)
        .spawn()
        .map_err(|e| format!("启动基岩版失败：{}", e))?;
    Ok("基岩版已启动".to_string())
}

// ==================== 路由 ====================

pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);
    match key.as_str() {
        "GET /api/bedrock/versions" => {
            let force = params
                .as_ref()
                .and_then(|p| p.get("refresh"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "true");
            let installed = installed_versions();
            let result = tokio::task::spawn_blocking(move || fetch_manifest(force)).await;
            match result {
                Ok(Ok(list)) => {
                    let versions: Vec<Value> = list
                        .into_iter()
                        .map(|v| {
                            let version = utils::get_str(&v, "version");
                            let links = v.get("downloadlinks").and_then(|x| x.as_array()).cloned().unwrap_or_default();
                            let time = v.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                            let installed_flag = installed.iter().any(|i| i == &version);
                            json!({
                                "id": v.get("id").cloned().unwrap_or(Value::Null),
                                "version": version,
                                "size": v.get("size").and_then(|x| x.as_u64()).unwrap_or(0),
                                "time": utils::ts_to_iso(time.max(0) as u64),
                                "abstract": utils::get_str(&v, "abstract"),
                                "wiki": utils::get_str(&v, "wikiLinks"),
                                "links": links,
                                "installed": installed_flag,
                                "isBedrock": true,
                            })
                        })
                        .collect();
                    Some(ApiResult::ok(json!({ "versions": versions })))
                }
                Ok(Err(e)) => Some(ApiResult::err(502, &e)),
                Err(_) => Some(ApiResult::err(500, "清单任务异常")),
            }
        }

        "POST /api/bedrock/install" => {
            let data = body.as_ref().or(params.as_ref()).cloned().unwrap_or(Value::Null);
            let version = utils::get_str(&data, "version");
            let link = utils::get_str(&data, "link");
            if version.is_empty() || link.is_empty() {
                return Some(ApiResult::err(400, "缺少 version 或 link"));
            }
            if marker_path(&version).exists() {
                return Some(ApiResult::ok(json!({ "success": false, "alreadyInstalled": true, "message": "该基岩版已安装" })));
            }
            let session_id = uuid::Uuid::new_v4().to_string();
            let cancel = Arc::new(AtomicBool::new(false));
            {
                let mut map = sessions();
                map.insert(session_id.clone(), BedrockSession {
                    status: "running".into(),
                    progress: 0,
                    message: "准备下载".into(),
                    version: version.clone(),
                    cancel: Some(cancel.clone()),
                    ..Default::default()
                });
            }
            let app_clone = app.clone();
            let sid = session_id.clone();
            tokio::task::spawn_blocking(move || {
                install_task(app_clone, sid, version, link, cancel);
            });
            Some(ApiResult::ok(json!({ "success": true, "sessionId": session_id })))
        }

        "GET /api/bedrock/install-progress" => {
            let sid = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if sid.is_empty() {
                return Some(ApiResult::err(400, "缺少 sessionId"));
            }
            Some(ApiResult::ok(session_json(sid)))
        }

        "POST /api/bedrock/install-cancel" => {
            let data = body.as_ref().or(params.as_ref()).cloned().unwrap_or(Value::Null);
            let sid = utils::get_str(&data, "sessionId");
            let mut map = sessions();
            if let Some(s) = map.get_mut(&sid)
                && let Some(flag) = &s.cancel
            {
                flag.store(true, Ordering::Relaxed);
            }
            Some(ApiResult::ok(json!({ "success": true })))
        }

        "GET /api/bedrock/login-status" => {
            let state = login_state()
                .lock()
                .map(|s| (*s).clone())
                .unwrap_or_default();
            let has_token = !read_token().is_empty();
            Some(ApiResult::ok(json!({
                "status": if has_token && state.status.is_empty() { "success" } else { state.status.as_str() },
                "message": if has_token && state.message.is_empty() { "已登录" } else { state.message.as_str() },
                "loggedIn": has_token,
            })))
        }

        "POST /api/bedrock/login-start" => {
            if !read_token().is_empty() {
                return Some(ApiResult::ok(json!({ "success": true, "loggedIn": true })));
            }
            match start_login_browser(app) {
                Ok(_) => Some(ApiResult::ok(json!({ "success": true, "loggedIn": false }))),
                Err(e) => Some(ApiResult::err(500, &e)),
            }
        }

        "POST /api/bedrock/logout" => {
            let _ = std::fs::remove_file(token_path());
            set_login("idle", "");
            Some(ApiResult::ok(json!({ "success": true })))
        }

        _ => None,
    }
}