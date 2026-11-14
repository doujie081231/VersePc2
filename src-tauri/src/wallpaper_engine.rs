// wallpaper_engine.rs — Wallpaper Engine 壁纸支持
// 职责：
//   1. 定位 Steam 库 → 扫描创意工坊目录（workshop/content/431960）→ 解析 project.json 返回壁纸列表
//   2. 提供 wewp:// 自定义协议：为 web 类型壁纸按原相对路径提供本地文件服务，并注入 WE Web SDK 兼容层
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
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

/// 当前账号：优先 Steam 运行时写入的 ActiveUser；Steam 未运行时该值会失效，
/// 回退到 loginusers.vdf 中最近登录的账号，避免因读不到账号而退回“列出全部”。
fn steam_current_user_id() -> Option<u32> {
    steam_active_user_id().or_else(steam_last_login_user_id)
}

/// 从 Steam 的 config/loginusers.vdf 读取最近登录的账号（timestamp 最大者）。
fn steam_last_login_user_id() -> Option<u32> {
    let steam = steam_install_dir()?;
    let raw = std::fs::read_to_string(steam.join("config").join("loginusers.vdf")).ok()?;
    parse_last_login_user(&raw)
}

/// 解析 loginusers.vdf：键为 SteamID64，减去基址即 32 位账号 ID（即 ActiveUser 使用的 ID）。
/// 取 timestamp 最大的条目作为最近登录账号；无 timestamp 时退化为首个可解析账号。
fn parse_last_login_user(raw: &str) -> Option<u32> {
    // SteamID64 = 76561197960265728 + 32 位账号 ID
    const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;
    let tokens = vdf_tokens(raw);
    let mut best: Option<(u64, u32)> = None;
    let mut i = 0usize;
    while i + 1 < tokens.len() {
        if tokens[i + 1] == "{"
            && let Ok(steam_id64) = tokens[i].parse::<u64>()
        {
            let mut timestamp = 0u64;
            let mut j = i + 2;
            while j < tokens.len() && tokens[j] != "}" {
                if tokens[j] == "timestamp" {
                    timestamp = tokens
                        .get(j + 1)
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);
                    j += 1;
                }
                j += 1;
            }
            if let Some(id) = steam_id64
                .checked_sub(STEAM_ID64_BASE)
                .filter(|id| *id <= u32::MAX as u64)
                .map(|id| id as u32)
                .filter(|id| *id > 0)
                && best.map(|(t, _)| timestamp > t).unwrap_or(true)
            {
                best = Some((timestamp, id));
            }
            i = j;
        }
        i += 1;
    }
    best.map(|(_, id)| id)
}

/// 收集全部 Steam 库的创意工坊清单 appworkshop_431960.acf。
/// 壁纸内容可能分散在多个库，只读第一个库的清单会把其它库的订阅漏掉。
fn workshop_manifest_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for lib in steam_library_folders() {
        let p = lib.join("workshop").join("appworkshop_431960.acf");
        if p.is_file() {
            paths.push(p);
        }
    }
    paths
}

/// Valve KeyValues（VDF/ACF）词法切分：引号字符串、`{`、`}`、裸 token。
/// 只用于读取订阅清单，不追求完整 VDF 语义。
fn vdf_tokens(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '/' if it.peek() == Some(&'/') => {
                for n in it.by_ref() {
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

/// 解析 appworkshop_431960.acf 的 WorkshopItemDetails。
/// 返回（清单里是否带 subscribedby 归属信息，作品 ID → 订阅该作品的账号列表）。
/// 账号列表兼容单个 ID，以及家庭共享时逗号/分号分隔的多个 ID。
fn parse_item_owners(raw: &str) -> (bool, HashMap<String, Vec<u32>>) {
    let tokens = vdf_tokens(raw);
    let mut owners: HashMap<String, Vec<u32>> = HashMap::new();
    let mut has_owner = false;
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
                        let mut account_ids: Vec<u32> = Vec::new();
                        while i < tokens.len() && inner > 0 {
                            match tokens[i].as_str() {
                                "{" => inner += 1,
                                "}" => inner -= 1,
                                "subscribedby" => {
                                    has_owner = true;
                                    account_ids = tokens
                                        .get(i + 1)
                                        .map(|v| parse_owner_ids(v.as_str()))
                                        .unwrap_or_default();
                                    i += 1;
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                        if !account_ids.is_empty() {
                            owners.insert(id, account_ids);
                        }
                    }
                    _ => i += 1,
                }
            }
        } else {
            i += 1;
        }
    }
    (has_owner, owners)
}

/// subscribedby 可能是单个账号 ID，也可能是家庭共享下逗号/分号分隔的多个 ID。
fn parse_owner_ids(raw: &str) -> Vec<u32> {
    raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|s| s.parse::<u32>().ok())
        .filter(|id| *id > 0)
        .collect()
}

/// 壁纸列表的账号过滤结果
enum WeSubscriptions {
    /// 已确认当前账号：只保留该账号订阅的作品（空集表示确实没有订阅）
    Known(HashSet<String>),
    /// 清单里没有任何账号归属信息（旧版 Steam），无法区分账号，只能保持列出全部
    Unknown,
}

/// 当前账号实际订阅的 WE 作品 ID 集合。
/// 同一台机器可能有多个 Steam 账号，workshop 目录是整机共享的，必须按 subscribedby 过滤，
/// 否则会混入其他账号订阅/取消订阅后残留的壁纸。
fn current_we_subscriptions() -> WeSubscriptions {
    let mut has_owner = false;
    let mut owners: HashMap<String, Vec<u32>> = HashMap::new();
    for path in workshop_manifest_paths() {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let (manifest_has_owner, manifest_owners) = parse_item_owners(&raw);
            has_owner = has_owner || manifest_has_owner;
            owners.extend(manifest_owners);
        }
    }
    if !has_owner {
        return WeSubscriptions::Unknown;
    }
    // 清单能区分账号却读不到当前账号时宁可不显示，也不能把其他账号的壁纸列进来
    let Some(user) = steam_current_user_id() else {
        return WeSubscriptions::Known(HashSet::new());
    };
    let ids = owners
        .into_iter()
        .filter(|(_, accounts)| accounts.contains(&user))
        .map(|(id, _)| id)
        .collect();
    WeSubscriptions::Known(ids)
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
        if let WeSubscriptions::Known(ids) = &subscribed {
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
    // 非 Windows 使用 wewp://localhost，Windows 的自定义协议会以 http://wewp.localhost 形式到达
    let after_scheme = uri
        .strip_prefix("wewp://")
        .or_else(|| uri.strip_prefix("http://wewp.localhost"))
        .unwrap_or(&uri);
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
    // 二次校验通过后交给通用 Range 服务
    let mime = mime_for(&rel_decoded);
    serve_file(&file_path, mime, request, mime.starts_with("text/html"))
}

/// wpfile:// 本地媒体协议：让 `<video>` / `<img>` 直接按 Range 流式读取已授权的本地媒体文件。
/// 之前自定义视频壁纸走 IPC 读整文件，大文件会失败；回退地址又指向不存在的 wpfile 协议，
/// 结果必然「视频壁纸加载失败」。这里只放行媒体扩展名 + read_file_buffer 同一套白名单。
pub fn handle_wpfile_request(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri().to_string();
    // Windows 下自定义协议以 http://wpfile.localhost 形式到达
    let after_scheme = uri
        .strip_prefix("wpfile://")
        .or_else(|| uri.strip_prefix("http://wpfile.localhost"))
        .unwrap_or(&uri);
    let path_part = after_scheme.split('?').next().unwrap_or(after_scheme);
    // Windows 本机路径为 /D:/dir/a.mp4，去掉前导斜杠
    let decoded = percent_decode(path_part.trim_start_matches('/'));
    if decoded.is_empty() {
        return not_found_response();
    }
    let path = std::path::PathBuf::from(&decoded);
    if !crate::filesystem::is_stream_media_allowed(&path) {
        return forbidden_response();
    }
    let mime = mime_for(&decoded);
    serve_file(&path, mime, request, false)
}

/// 按 HTTP Range 返回本地文件（不支持的请求返回 416）
fn serve_file(
    file_path: &std::path::Path,
    mime: &'static str,
    request: &tauri::http::Request<Vec<u8>>,
    inject_html_shim: bool,
) -> tauri::http::Response<Vec<u8>> {
    let Ok(meta) = std::fs::metadata(file_path) else {
        return not_found_response();
    };
    let len = meta.len();

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
        let mut file = match std::fs::File::open(file_path) {
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

    let Ok(bytes) = std::fs::read(file_path) else {
        return not_found_response();
    };
    let body = if inject_html_shim {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_last_login_user_picks_latest_timestamp() {
        let raw = r#"
"users"
{
    "76561197960265729"
    {
        "AccountName" "old"
        "timestamp" "100"
    }
    "76561199492578030"
    {
        "AccountName" "recent"
        "timestamp" "1792907014"
    }
}
"#;
        assert_eq!(parse_last_login_user(raw), Some(1532312302));
    }

    #[test]
    fn parse_owner_ids_accepts_single_and_multiple() {
        assert_eq!(parse_owner_ids("1532312302"), vec![1532312302]);
        assert_eq!(
            parse_owner_ids("1532312302,1909968719"),
            vec![1532312302, 1909968719]
        );
        assert!(parse_owner_ids("0").is_empty());
    }

    #[test]
    fn parse_item_owners_keeps_only_current_account() {
        let raw = r#"
"AppWorkshop"
{
    "WorkshopItemDetails"
    {
        "111"
        {
            "subscribedby" "1532312302"
        }
        "222"
        {
            "subscribedby" "1909968719"
        }
        "333"
        {
            "subscribedby" "1532312302,1909968719"
        }
    }
}
"#;
        let (has_owner, owners) = parse_item_owners(raw);
        assert!(has_owner);
        assert_eq!(owners.len(), 3);
        assert_eq!(owners["111"], vec![1532312302]);
        assert_eq!(owners["333"], vec![1532312302, 1909968719]);

        let mine: HashSet<String> = owners
            .into_iter()
            .filter(|(_, accounts)| accounts.contains(&1532312302))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            mine,
            HashSet::from(["111".to_string(), "333".to_string()])
        );
    }

    #[test]
    fn parse_item_owners_reports_missing_owner_info() {
        let raw = r#"
"AppWorkshop"
{
    "WorkshopItemDetails"
    {
        "111"
        {
            "manifest" "1"
        }
    }
}
"#;
        let (has_owner, owners) = parse_item_owners(raw);
        assert!(!has_owner);
        assert!(owners.is_empty());
    }
}
