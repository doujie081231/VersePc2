// wallpaper_engine.rs — Wallpaper Engine 壁纸支持
// 职责：
//   1. 定位 Steam 库 → 扫描创意工坊目录（workshop/content/431960）→ 解析 project.json 返回壁纸列表
//   2. 提供 wewp:// 自定义协议：为 web 类型壁纸按原相对路径提供本地文件服务，并注入 WE Web SDK 兼容层
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Wallpaper Engine 的 Steam 创意工坊 AppID
const WE_WORKSHOP_ID: &str = "431960";

// ============== Steam 库定位 ==============

/// 从注册表读取 Steam 安装目录（SteamPath 使用正斜杠，统一转反斜杠）
fn steam_install_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey(r"Software\Valve\Steam").ok()?;
        let raw: String = key.get_value("SteamPath").ok()?;
        Some(PathBuf::from(raw.replace('/', "\\")))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// 收集全部 Steam 库的 steamapps 目录（默认库 + libraryfolders.vdf 里的额外库）
fn steam_library_folders() -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    let Some(steam) = steam_install_dir() else {
        return libs;
    };
    let primary = steam.join("steamapps");
    libs.push(primary.clone());

    // libraryfolders.vdf 中每条记录形如： "path" "C:\\Program Files (x86)\\Steam"
    let vdf_path = primary.join("libraryfolders.vdf");
    if let Ok(raw) = std::fs::read_to_string(&vdf_path) {
        let re = Regex::new(r#""path"\s*"([^"]+)""#).unwrap();
        for cap in re.captures_iter(&raw) {
            // VDF 中用 \\ 表示字面反斜杠，需还原
            let p = cap[1].replace("\\\\", "\\");
            let pb = PathBuf::from(&p);
            if !libs.contains(&pb) {
                libs.push(pb);
            }
        }
    }
    libs
}

/// 当前登录的 Steam 账号 ID（32 位账号 ID，不是 SteamID64）。
/// Steam 运行时写入 HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser。
#[cfg(target_os = "windows")]
fn steam_active_user_id() -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey(r"Software\Valve\Steam\ActiveProcess").ok()?;
    key.get_value::<u32, _>("ActiveUser")
        .ok()
        .filter(|id| *id > 0)
}

#[cfg(not(target_os = "windows"))]
fn steam_active_user_id() -> Option<u32> {
    None
}

/// 定位创意工坊清单 appworkshop_431960.acf
fn workshop_manifest_path() -> Option<PathBuf> {
    for lib in steam_library_folders() {
        let p = lib.join("workshop").join("appworkshop_431960.acf");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Valve KeyValues（VDF/ACF）词法切分：引号字符串、`{`、`}`、裸 token。
/// 只用于读取订阅清单，不追求完整 VDF 语义。
fn vdf_tokens(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '/' if it.peek() == Some(&'/') => {
                while let Some(n) = it.next() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => out.push(c.to_string()),
            '"' => {
                let mut s = String::new();
                while let Some(n) = it.next() {
                    if n == '\\' {
                        if let Some(e) = it.next() {
                            s.push(e);
                        }
                    } else if n == '"' {
                        break;
                    } else {
                        s.push(n);
                    }
                }
                out.push(s);
            }
            c if c.is_whitespace() => {}
            c => {
                let mut s = String::new();
                s.push(c);
                while let Some(&n) = it.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    it.next();
                }
                out.push(s);
            }
        }
    }
    out
}

/// 从 appworkshop_431960.acf 的 WorkshopItemDetails 中筛出当前账号订阅的作品 ID。
/// 同一台机器可能有多个 Steam 账号，旧逻辑会把其他账号订阅/残留的壁纸也列出来。
fn parse_subscribed_ids(raw: &str, active_user: u32) -> HashSet<String> {
    let tokens = vdf_tokens(raw);
    let mut ids: HashSet<String> = HashSet::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if tokens[i] == "WorkshopItemDetails" && tokens.get(i + 1).map(|s| s == "{").unwrap_or(false) {
            i += 2;
            let mut depth = 1usize;
            while i < tokens.len() && depth > 0 {
                match tokens[i].as_str() {
                    "{" => {
                        depth += 1;
                        i += 1;
                    }
                    "}" => {
                        depth -= 1;
                        i += 1;
                    }
                    _ if depth == 1 && tokens.get(i + 1).map(|s| s == "{").unwrap_or(false) => {
                        let id = tokens[i].clone();
                        i += 2;
                        let mut inner = 1usize;
                        let mut subscribed: Option<u32> = None;
                        while i < tokens.len() && inner > 0 {
                            match tokens[i].as_str() {
                                "{" => inner += 1,
                                "}" => inner -= 1,
                                "subscribedby" => {
                                    subscribed = tokens
                                        .get(i + 1)
                                        .and_then(|v| v.parse::<u32>().ok());
                                    i += 1;
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                        if subscribed == Some(active_user) {
                            ids.insert(id);
                        }
                    }
                    _ => i += 1,
                }
            }
        } else {
            i += 1;
        }
    }
    ids
}

/// 当前账号实际订阅的 WE 作品 ID 集合；取不到清单时返回 None（保持旧行为，列出全部）
fn current_we_subscriptions() -> Option<HashSet<String>> {
    let user = steam_active_user_id()?;
    let raw = std::fs::read_to_string(workshop_manifest_path()?).ok()?;
    let ids = parse_subscribed_ids(&raw, user);
    if ids.is_empty() {
        None
    } else {
        Some(ids)
    }
}

/// 遍历所有库，收集 WE 创意工坊下的壁纸项目目录
fn we_project_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for lib in steam_library_folders() {
        let wc = lib
            .join("workshop")
            .join("content")
            .join(WE_WORKSHOP_ID);
        let Ok(entries) = std::fs::read_dir(&wc) else {
            continue;
        };
        for e in entries.flatten() {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                dirs.push(e.path());
            }
        }
    }
    dirs
}

/// 按作品 ID 定位壁纸项目目录（wewp 协议用）
pub fn resolve_we_project_dir(workshop_id: &str) -> Option<PathBuf> {
    for lib in steam_library_folders() {
        let d = lib
            .join("workshop")
            .join("content")
            .join(WE_WORKSHOP_ID)
            .join(workshop_id);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

// ============== project.json 解析 ==============

/// 读取并解析项目 project.json（general 字段优先，兼容根级字段）
fn read_project(dir: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(dir.join("project.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 依次在 general 与根级取值
fn pick_field(proj: &Value, general: &Value, key: &str) -> Option<String> {
    for src in [general, proj] {
        if let Some(s) = src.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
    }
    None
}

/// 推断壁纸类型：WE 的 project.json 中 type 可能缺失/不规范，
/// 以内容文件后缀为准（scene.pkg/scene.json → scene；.mp4/.webm → video；.html → web；.exe → application）。
fn infer_we_type(declared: &str, file: &str, preview: &str) -> String {
    let fl = file.to_lowercase();
    if fl.ends_with(".pkg") || fl.ends_with("scene.json") {
        return "scene".to_string();
    }
    if fl.ends_with(".mp4") || fl.ends_with(".webm") || fl.ends_with(".mov") {
        return "video".to_string();
    }
    if fl.ends_with(".html") || fl.ends_with(".htm") {
        return "web".to_string();
    }
    if fl.ends_with(".exe") {
        return "application".to_string();
    }
    let d = declared.to_lowercase();
    if matches!(d.as_str(), "scene" | "video" | "web" | "application" | "background") {
        return d;
    }
    // 无内容文件、类型也识别不出：有预览图则按静态背景，否则按场景兜底
    if !preview.is_empty() {
        "background".to_string()
    } else {
        "scene".to_string()
    }
}

// ============== Tauri 命令 ==============

/// 扫描 Wallpaper Engine 创意工坊，返回全部壁纸列表
/// 返回 { success, wallpapers: [{ id, title, author, type, file, preview, filePath, previewPath, dir, tags }] }
#[tauri::command]
pub fn wallpaper_engine_list() -> Value {
    let mut wallpapers: Vec<Value> = Vec::new();
    // 只列出当前 Steam 账号订阅的作品：workshop 目录是整机共享的，
    // 直接遍历会把其他账号订阅/取消订阅后残留的壁纸一起列出来。
    let subscribed = current_we_subscriptions();
    for dir in we_project_dirs() {
        if let Some(ids) = &subscribed {
            let id = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !ids.contains(id) {
                continue;
            }
        }
        let Some(proj) = read_project(&dir) else {
            continue;
        };
        let general = proj.get("general").cloned().unwrap_or_default();

        let file = pick_field(&proj, &general, "file");
        let preview = pick_field(&proj, &general, "preview");
        let title = pick_field(&proj, &general, "title").unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        let author = pick_field(&proj, &general, "author").unwrap_or_default();
        let workshopid = pick_field(&proj, &general, "workshopid").unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        // 类型：以内容文件推断为准（WE 的 type 字段可能缺失/不规范，误判会把场景壁纸当静态图显示）
        let wtype = infer_we_type(
            &pick_field(&proj, &general, "type").unwrap_or_default(),
            &file.clone().unwrap_or_default(),
            &preview.clone().unwrap_or_default(),
        );
        let tags: Vec<String> = proj
            .get("tags")
            .or_else(|| general.get("tags"))
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let abs = |rel: &Option<String>| -> String {
            rel.as_deref()
                .map(|f| dir.join(f))
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        wallpapers.push(json!({
            "id": workshopid,
            "title": title,
            "author": author,
            "type": wtype,
            "file": file.clone().unwrap_or_default(),
            "preview": preview.clone().unwrap_or_default(),
            "filePath": abs(&file),
            "previewPath": abs(&preview),
            "dir": dir.to_string_lossy().to_string(),
            "tags": tags,
        }));
    }
    json!({ "success": true, "wallpapers": wallpapers })
}

// ============== wewp:// 本地协议（web 壁纸） ==============

/// 简易百分号解码
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hv = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(h), Some(l)) = (hv(b[i + 1]), hv(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// 注入 WE Web SDK 兼容层：为在启动器内运行的 web 壁纸提供 WE 全局 API 表面
fn inject_we_shim(html: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(html);
    let shim = r#"<script>
(function(){
  if (window.__weShimInjected) return;
  window.__weShimInjected = true;
  var _audioListeners = [];
  window.wallpaperRegisterAudioListener = function(fn){ _audioListeners.push(fn); };
  window.__wePushAudio = function(arr){ for (var i=0;i<_audioListeners.length;i++){ try{ _audioListeners[i](arr); }catch(e){} } };
  if (!window.wallpaperPropertyListener) window.wallpaperPropertyListener = { applyUserProperties: function(){}, applyGeneralProperties: function(){} };
  window.wallpaperRegisterPropertyListener = function(){};
  window.wallpaperRegisterMediaStatusListener = function(){};
  window.wallpaperRegisterMediaPropertiesListener = function(){};
  window.wallpaperRegisterMediaThumbnailListener = function(){};
  window.wallpaperRegisterAudioVisualizerListener = function(){};
  window.wallpaperMediaIntegration = { playback: { PLAYING:1, PAUSED:2, STOPPED:3 } };
  window.wallpaperApplyGeneralProperties = function(){};
  window.wallpaperRequestRandomFileForProperty = function(){ return ''; };
})();
</script>"#;
    let out = if let Some(pos) = s.find("</head>") {
        format!("{}{}{}", &s[..pos], shim, &s[pos..])
    } else {
        format!("{}{}", shim, s)
    };
    out.into_bytes()
}

/// wewp 协议请求处理：wewp://localhost/{workshop_id}/{相对路径}
pub fn handle_wewp_request(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri().to_string();
    let after_scheme = uri.strip_prefix("wewp://").unwrap_or(&uri);
    // 去掉 host 段（localhost / 空）得到 /{id}/{path}
    let path_part = match after_scheme.find('/') {
        Some(idx) => &after_scheme[idx..],
        None => "/",
    };
    // 去掉查询参数
    let path_only = path_part.split('?').next().unwrap_or(path_part).trim_start_matches('/');
    let mut segs = path_only.splitn(2, '/');
    let workshop_id = segs.next().unwrap_or("");
    let rel = segs.next().unwrap_or("index.html");

    if workshop_id.is_empty() {
        return not_found_response();
    }
    // 路径穿越防护
    if rel.split(['/', '\\']).any(|s| s == "..") {
        return forbidden_response();
    }

    let Some(dir) = resolve_we_project_dir(&percent_decode(workshop_id)) else {
        return not_found_response();
    };
    let rel_decoded = percent_decode(rel);
    let file_path = dir.join(&rel_decoded);
    if !file_path.is_file() {
        return not_found_response();
    }
    // 二次校验：防止 project.json 里的绝对路径/编码后的 .. 跳出壁纸目录
    let (Ok(canon_dir), Ok(canon_file)) = (dir.canonicalize(), file_path.canonicalize()) else {
        return not_found_response();
    };
    if !canon_file.starts_with(&canon_dir) {
        return forbidden_response();
    }
    let Ok(meta) = std::fs::metadata(&canon_file) else {
        return not_found_response();
    };
    let len = meta.len();
    let mime = mime_for(&rel_decoded);

    // WE 视频壁纸通过 wewp:// 直接播放原文件：必须支持 HTTP Range，
    // 否则 WebView 媒体栈会退化为整文件读取（大文件内存暴涨甚至崩溃）。
    let range = request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_byte_range(v, len));

    if request.headers().contains_key("range") && range.is_none() {
        return tauri::http::Response::builder()
            .status(416)
            .header("Content-Range", format!("bytes */{}", len))
            .header("Accept-Ranges", "bytes")
            .header("Access-Control-Allow-Origin", "*")
            .body(Vec::new())
            .unwrap_or_else(|_| not_found_response());
    }

    if let Some((start, end)) = range {
        let mut file = match std::fs::File::open(&file_path) {
            Ok(f) => f,
            Err(_) => return not_found_response(),
        };
        let read_len = (end - start + 1).min(8 * 1024 * 1024) as usize;
        let end = start + read_len as u64 - 1;
        let mut buf = vec![0u8; read_len];
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut buf).is_err() {
            return not_found_response();
        }
        return tauri::http::Response::builder()
            .status(206)
            .header("Content-Type", mime)
            .header("Content-Length", read_len.to_string())
            .header("Content-Range", format!("bytes {}-{}/{}", start, end, len))
            .header("Accept-Ranges", "bytes")
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges")
            .body(buf)
            .unwrap_or_else(|_| not_found_response());
    }

    let Ok(bytes) = std::fs::read(&file_path) else {
        return not_found_response();
    };
    let body = if mime.starts_with("text/html") {
        inject_we_shim(&bytes)
    } else {
        bytes
    };
    let body_len = body.len();

    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", mime)
        .header("Content-Length", body_len.to_string())
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_else(|_| not_found_response())
}

/// 解析单段 Range（bytes=start-end / start- / -suffix），返回闭区间 [start, end]
fn parse_byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let spec = header.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    if a.is_empty() {
        let suffix = b.trim().parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = len.saturating_sub(suffix);
        return Some((start, len - 1));
    }
    let start = a.trim().parse::<u64>().ok()?;
    if start >= len {
        return None;
    }
    let end = if b.trim().is_empty() {
        len - 1
    } else {
        b.trim().parse::<u64>().ok()?.min(len - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

fn not_found_response() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(404)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap_or_default()
}

fn forbidden_response() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(403)
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new())
        .unwrap_or_default()
}
