// api/misc.rs — 杂项 API 路由
// 职责：当前上下文、快捷方式、截图、服务器 ping、背景图
// 对应原项目 server/api/routes/misc.js
//
// 路由：
//   GET  /api/current-context      聚合当前选中版本/模组/Java 等信息
//   POST /api/create-shortcut      创建桌面/开始菜单快捷方式（仅 Windows）
//   GET  /api/screenshots          列出版本/全局截图
//   GET  /api/screenshot           读取截图文件（带路径白名单）
//   DELETE /api/screenshot         删除截图
//   GET  /api/server/ping          Minecraft 服务器 ping
//   POST /api/save-background     保存 base64 背景图
//   GET  /api/clear-background     清除背景图

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::ApiResult;
use crate::storage;

/// 处理杂项路由
pub async fn handle(
    _app: &tauri::AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/current-context" => Some(handle_current_context().await),
        "POST /api/create-shortcut" => Some(handle_create_shortcut(body).await),
        "GET /api/screenshots" => Some(handle_screenshots(params)),
        "GET /api/screenshot" => Some(handle_screenshot_get(params)),
        "DELETE /api/screenshot" => Some(handle_screenshot_delete(params)),
        "GET /api/server/ping" => Some(handle_server_ping(params).await),
        "POST /api/save-background" => Some(handle_save_background(body)),
        "GET /api/clear-background" => Some(handle_clear_background()),
        _ => None,
    }
}

/// GET /api/current-context — 当前上下文聚合
///
/// 返回选中版本、模组目录、加载器类型、Java 路径、内存等配置
async fn handle_current_context() -> ApiResult {
    let settings = storage::load_settings();
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    let selected_version = settings
        .get("selectedVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut version_dir = String::new();
    let mut mods_dir = String::new();
    let mut loader = String::new();
    let mut loader_version = String::new();
    let mut mods_count: u64 = 0;
    let mut mods_enabled: u64 = 0;
    let mut mods_disabled: u64 = 0;

    if !selected_version.is_empty() {
        let ver_dir = versions_dir.join(&selected_version);
        version_dir = ver_dir.to_string_lossy().to_string();

        // 检测加载器类型
        let forge_json = ver_dir.join("version.json");
        let fabric_json = ver_dir.join("fabric-loader.json");
        let neo_json = ver_dir.join("neoforge-loader.json");

        if forge_json.exists() {
            loader = "forge".to_string();
            if let Ok(content) = std::fs::read_to_string(&forge_json) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    loader_version = json
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
        } else if fabric_json.exists() {
            loader = "fabric".to_string();
            if let Ok(content) = std::fs::read_to_string(&fabric_json) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    loader_version = json
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
        } else if neo_json.exists() {
            loader = "neoforge".to_string();
            if let Ok(content) = std::fs::read_to_string(&neo_json) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    loader_version = json
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
        }

        // 模组目录：版本隔离时在 ver_dir/mods，否则在 gameDir/mods
        let version_isolation = settings
            .get("versionIsolation")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.clone());

        let mods_path = if version_isolation {
            ver_dir.join("mods")
        } else {
            game_dir.join("mods")
        };
        mods_dir = mods_path.to_string_lossy().to_string();

        // 统计模组数量
        if let Ok(entries) = std::fs::read_dir(&mods_path) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".jar") {
                        mods_count += 1;
                        mods_enabled += 1;
                    } else if name.ends_with(".jar.disabled") {
                        mods_count += 1;
                        mods_disabled += 1;
                    }
                }
            }
        }
    }

    ApiResult::ok(json!({
        "selectedVersion": selected_version,
        "versionDir": version_dir,
        "modsDir": mods_dir,
        "loader": loader,
        "loaderVersion": loader_version,
        "javaPath": settings.get("javaPath").and_then(|v| v.as_str()).unwrap_or(""),
        "maxMemory": settings.get("maxMemory").and_then(|v| v.as_str()).unwrap_or(""),
        "minMemory": settings.get("minMemory").and_then(|v| v.as_str()).unwrap_or(""),
        "javaArgs": settings.get("javaArgs").and_then(|v| v.as_str()).unwrap_or(""),
        "versionIsolation": settings.get("versionIsolation").and_then(|v| v.as_bool()).unwrap_or(true),
        "modsCount": mods_count,
        "modsEnabled": mods_enabled,
        "modsDisabled": mods_disabled
    }))
}

/// POST /api/create-shortcut — 创建快捷方式（仅 Windows）
///
/// 请求体：
///   - type: shortcut 类型（desktop/start-menu）
///   - versionId: 可选，版本 ID（用于命名快捷方式）
async fn handle_create_shortcut(body: &Option<Value>) -> ApiResult {
    #[cfg(not(target_os = "windows"))]
    {
        return ApiResult::ok(json!({ "success": false, "error": "创建快捷方式仅支持 Windows 系统" }));
    }

    #[cfg(target_os = "windows")]
    {
        let shortcut_type = body
            .as_ref()
            .and_then(|b| b.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("desktop");
        let version_id = body
            .as_ref()
            .and_then(|b| b.get("versionId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 获取当前 exe 路径
        let exe_path = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => return ApiResult::err(500, &format!("无法获取 exe 路径: {}", e)),
        };

        let shortcut_name = if !version_id.is_empty() {
            format!("VersePC2 - {}", version_id)
        } else {
            "VersePC2".to_string()
        };

        // 清理非法字符
        let safe_name: String = shortcut_name
            .chars()
            .filter(|c| !matches!(c, '"' | '$' | '`' | '<' | '>' | ':' | '*' | '?' | '\\' | '/' | '|'))
            .collect();

        let shortcut_dir = if shortcut_type == "desktop" {
            dirs::desktop_dir()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| dirs::home_dir().map(|p| p.join("Desktop")).unwrap_or_default())
        } else {
            dirs::config_dir()
                .map(|p| {
                    p.join("Microsoft")
                        .join("Windows")
                        .join("Start Menu")
                        .join("Programs")
                })
                .unwrap_or_default()
        };

        if !shortcut_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&shortcut_dir) {
                return ApiResult::err(500, &format!("无法创建目录: {}", e));
            }
        }

        let shortcut_path = shortcut_dir.join(format!("{}.lnk", safe_name));
        let working_dir = exe_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // 使用 PowerShell WScript.Shell 创建快捷方式
        // 采用 here-string（@' '@）包裹路径和描述，避免路径中出现引号等特殊字符时提前终止字符串
        let ps_script = format!(
r#"
$shortcutPath = @'
{}
'@
$targetPath = @'
{}
'@
$workingDir = @'
{}
'@
$description = @'
VersePC2 Minecraft Launcher
'@
$ws = New-Object -ComObject WScript.Shell
$sc = $ws.CreateShortcut($shortcutPath)
$sc.TargetPath = $targetPath
$sc.WorkingDirectory = $workingDir
$sc.Description = $description
$sc.Save()
"#,
            shortcut_path.display(),
            exe_path.display(),
            working_dir.display()
        );

        // 使用 -EncodedCommand：把脚本转成 UTF-16LE Base64 再传参，彻底避免
        // 中文编码、引号嵌套、特殊字符转义失败导致的 PowerShell 语法错误。
        use base64::{Engine as _, engine::general_purpose::STANDARD as b64};
        let utf16le: Vec<u8> = ps_script.encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let encoded_cmd = b64.encode(&utf16le);

        let mut ps_cmd = std::process::Command::new("powershell");
        ps_cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded_cmd,
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            ps_cmd.creation_flags(0x08000000);
        }
        let output = ps_cmd.output();

        match output {
            Ok(out) => {
                if out.status.success() && shortcut_path.exists() {
                    ApiResult::ok(json!({
                        "success": true,
                        "path": shortcut_path.to_string_lossy()
                    }))
                } else {
                    let err = String::from_utf8_lossy(&out.stderr).to_string();
                    let msg = if err.trim().is_empty() {
                        if shortcut_path.exists() {
                            "快捷方式已生成，但 PowerShell 返回了非零状态码".to_string()
                        } else {
                            "未找到生成的快捷方式文件".to_string()
                        }
                    } else {
                        format!("PowerShell 执行失败: {}", err)
                    };
                    ApiResult::err(500, &msg)
                }
            }
            Err(e) => ApiResult::err(500, &format!("执行 PowerShell 失败: {}", e)),
        }
    }
}

/// GET /api/screenshots — 列出截图
///
/// 查询参数：
///   - versionId: 可选，版本 ID（用于版本隔离路径）
fn handle_screenshots(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    // 版本截图目录
    let mut dirs_to_scan: Vec<PathBuf> = Vec::new();
    if !version_id.is_empty() {
        let ver_ss_dir = versions_dir.join(version_id).join("screenshots");
        dirs_to_scan.push(ver_ss_dir);
    }
    // 全局截图目录
    dirs_to_scan.push(data_dir.join("screenshots"));

    let mut screenshots: Vec<Value> = Vec::new();

    for dir in &dirs_to_scan {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if !matches!(ext_lower.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let time = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    screenshots.push(json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                        "size": size,
                        "time": time,
                        "url": format!("/api/screenshot?path={}", urlencoding::encode(&path.to_string_lossy()))
                    }));
                }
            }
        }
    }

    // 按时间倒序
    screenshots.sort_by(|a, b| {
        let ta = a.get("time").and_then(|v| v.as_u64()).unwrap_or(0);
        let tb = b.get("time").and_then(|v| v.as_u64()).unwrap_or(0);
        tb.cmp(&ta)
    });

    ApiResult::ok(json!({ "screenshots": screenshots }))
}

/// GET /api/screenshot — 读取截图文件
///
/// 查询参数：
///   - path: 截图文件路径（必须位于白名单目录内）
fn handle_screenshot_get(params: &Option<Value>) -> ApiResult {
    let path_str = params
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if path_str.is_empty() {
        return ApiResult::err(400, "Missing path");
    }

    let decoded_path = urlencoding::decode(path_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| path_str.to_string());

    let path = PathBuf::from(&decoded_path);

    if !path.exists() {
        return ApiResult::err(404, "Not found");
    }

    // 路径白名单校验
    if !is_screenshot_path_allowed(&path) {
        return ApiResult::err(403, "Forbidden");
    }

    // 读取文件并以 base64 返回（前端用 data URL 显示）
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => return ApiResult::err(500, &format!("读取失败: {}", e)),
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        _ => "image/png",
    };

    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
    ApiResult::ok(json!({
        "dataUrl": format!("data:{};base64,{}", mime, b64),
        "size": data.len()
    }))
}

/// DELETE /api/screenshot — 删除截图
fn handle_screenshot_delete(params: &Option<Value>) -> ApiResult {
    let path_str = params
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if path_str.is_empty() {
        return ApiResult::err(400, "Missing path");
    }

    let decoded_path = urlencoding::decode(path_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| path_str.to_string());

    let path = PathBuf::from(&decoded_path);

    if !is_screenshot_path_allowed(&path) {
        return ApiResult::err(403, "Forbidden");
    }

    match std::fs::remove_file(&path) {
        Ok(_) => ApiResult::ok(json!({ "success": true })),
        Err(e) => ApiResult::err(500, &format!("删除失败: {}", e)),
    }
}

/// GET /api/server/ping — Minecraft 服务器 ping
///
/// 查询参数：
///   - host: 服务器地址
///   - port: 端口（默认 25565）
async fn handle_server_ping(params: &Option<Value>) -> ApiResult {
    let host = params
        .as_ref()
        .and_then(|p| p.get("host"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let port = params
        .as_ref()
        .and_then(|p| p.get("port"))
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;

    if host.is_empty() {
        return ApiResult::err(400, "host required");
    }

    // 实现 Minecraft Server List Ping 协议
    match mc_ping(host, port).await {
        Ok(result) => ApiResult::ok(result),
        Err(e) => ApiResult::ok(json!({
            "online": false,
            "error": e
        })),
    }
}

/// POST /api/save-background — 保存 base64 背景图
///
/// 请求体：
///   - dataUrl: base64 data URL（如 data:image/png;base64,xxx）
fn handle_save_background(body: &Option<Value>) -> ApiResult {
    let data_url = body
        .as_ref()
        .and_then(|b| b.get("dataUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if data_url.is_empty() {
        return ApiResult::err(400, "dataUrl required");
    }

    // 解析 data URL: data:image/<ext>;base64,<data>
    let re = match regex::Regex::new(r"^data:image/(\w+);base64,(.+)$") {
        Ok(r) => r,
        Err(_) => return ApiResult::err(500, "regex 构造失败"),
    };
    let captures = match re.captures(data_url) {
        Some(c) => c,
        None => return ApiResult::err(400, "invalid dataUrl"),
    };

    let ext = captures.get(1).map(|m| m.as_str()).unwrap_or("png");
    let ext_normalized = if ext == "jpeg" { "jpg" } else { ext };
    let b64_data = captures.get(2).map(|m| m.as_str()).unwrap_or("");

    let data = match base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        b64_data,
    ) {
        Ok(d) => d,
        Err(e) => return ApiResult::err(400, &format!("base64 解码失败: {}", e)),
    };

    let data_dir = storage::resolve_data_dir();
    let bg_path = data_dir.join(format!("background.{}", ext_normalized));

    if let Err(e) = std::fs::write(&bg_path, &data) {
        return ApiResult::err(500, &format!("写入失败: {}", e));
    }

    ApiResult::ok(json!({
        "success": true,
        "path": bg_path.to_string_lossy()
    }))
}

/// GET /api/clear-background — 清除背景图
fn handle_clear_background() -> ApiResult {
    let data_dir = storage::resolve_data_dir();
    let bg_files = ["background.png", "background.jpg", "background.jpeg"];

    for f in &bg_files {
        let p = data_dir.join(f);
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
    }

    ApiResult::ok(json!({ "success": true }))
}

/// 路径白名单校验：截图路径必须位于 DATA_DIR 或 VERSIONS_DIR 下
fn is_screenshot_path_allowed(path: &Path) -> bool {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    // 检查路径是否以 data_dir 或 versions_dir 开头
    let canonical_str = canonical.to_string_lossy().to_lowercase();
    let data_dir_str = data_dir.to_string_lossy().to_lowercase();
    let versions_dir_str = versions_dir.to_string_lossy().to_lowercase();

    canonical_str.starts_with(&data_dir_str) || canonical_str.starts_with(&versions_dir_str)
}

/// Minecraft Server List Ping 协议实现
/// 对应原项目 network.mcPing
///
/// 协议步骤：
/// 1. TCP 连接
/// 2. 发送 Handshake 包（协议版本 -1 = ping）
/// 3. 发送 StatusRequest 包（空 JSON）
/// 4. 接收 StatusResponse（JSON 含 description/players/version）
async fn mc_ping(host: &str, port: u16) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::Duration;

    // read_varint 已在下面定义为泛型函数，需要 AsyncReadExt trait in scope

    let address = format!("{}:{}", host, port);
    eprintln!("[misc] ping {}", address);

    let mut stream = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(&address),
    )
    .await
    .map_err(|_| format!("连接超时: {}", address))?
    .map_err(|e| format!("连接失败: {}", e))?;

    // === Handshake 包 ===
    // Packet ID: 0x00
    // Fields: protocolVersion (VarInt, -1), serverAddress (String), serverPort (UShort), nextState (VarInt, 1=status)
    let mut handshake = Vec::new();
    write_varint(&mut handshake, -1); // protocol version = -1 (ping)
    write_string(&mut handshake, host); // server address
    handshake.extend_from_slice(&port.to_be_bytes()); // port (big endian u16)
    write_varint(&mut handshake, 1); // next state = 1 (status)

    // 发送 Handshake 数据包
    let mut packet = Vec::new();
    write_varint(&mut packet, 0); // packet ID = 0
    packet.extend_from_slice(&handshake);

    let mut frame = Vec::new();
    write_varint(&mut frame, packet.len() as i32);
    frame.extend_from_slice(&packet);

    stream
        .write_all(&frame)
        .await
        .map_err(|e| format!("发送 Handshake 失败: {}", e))?;

    // === StatusRequest 包 ===
    // Packet ID: 0x00, 无字段
    let mut req_frame = Vec::new();
    write_varint(&mut req_frame, 1); // length = 1
    write_varint(&mut req_frame, 0); // packet ID = 0

    stream
        .write_all(&req_frame)
        .await
        .map_err(|e| format!("发送 StatusRequest 失败: {}", e))?;

    // === 读取响应 ===
    // 1. 读取包长度
    let packet_len = read_varint(&mut stream)
        .await
        .map_err(|e| format!("读取长度失败: {}", e))?;

    // 2. 读取 packet ID
    let _packet_id = read_varint(&mut stream)
        .await
        .map_err(|e| format!("读取 packet ID 失败: {}", e))?;

    // 3. 读取 JSON 字符串
    let json_len = read_varint(&mut stream)
        .await
        .map_err(|e| format!("读取 JSON 长度失败: {}", e))?;

    let mut json_bytes = vec![0u8; json_len as usize];
    if json_len > 0 {
        stream
            .read_exact(&mut json_bytes)
            .await
            .map_err(|e| format!("读取 JSON 内容失败: {}", e))?;
    }

    let json_str = String::from_utf8_lossy(&json_bytes);
    let status: Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

    // 提取关键字段
    let description = status.get("description");
    let players = status.get("players");
    let version = status.get("version");

    let motd = if let Some(desc) = description {
        if let Some(text) = desc.get("text").and_then(|t| t.as_str()) {
            text.to_string()
        } else if let Some(extra) = desc.get("extra").and_then(|e| e.as_array()) {
            extra
                .iter()
                .filter_map(|e| e.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        } else {
            desc.to_string()
        }
    } else {
        String::new()
    };

    Ok(json!({
        "online": true,
        "host": host,
        "port": port,
        "description": motd,
        "players": {
            "max": players.and_then(|p| p.get("max")).and_then(|m| m.as_u64()).unwrap_or(0),
            "online": players.and_then(|p| p.get("online")).and_then(|o| o.as_u64()).unwrap_or(0)
        },
        "version": {
            "name": version.and_then(|v| v.get("name")).and_then(|n| n.as_str()).unwrap_or(""),
            "protocol": version.and_then(|v| v.get("protocol")).and_then(|p| p.as_u64()).unwrap_or(0)
        },
        "favicon": status.get("favicon").and_then(|f| f.as_str()).unwrap_or("")
    }))
}

/// 写入 VarInt（Variable-length integer）
fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    while value & !0x7F != 0 {
        buf.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    buf.push((value & 0x7F) as u8);
}

/// 写入 String（VarInt 长度前缀 + UTF-8 字节）
fn write_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_varint(buf, bytes.len() as i32);
    buf.extend_from_slice(bytes);
}

/// 读取 VarInt
async fn read_varint<R>(reader: &mut R) -> Result<i32, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut result: i32 = 0;
    let mut shift = 0;

    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .await
            .map_err(|e| format!("读取字节失败: {}", e))?;

        let b = byte[0];
        result |= ((b & 0x7F) as i32) << shift;

        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 32 {
            return Err("VarInt 过长".to_string());
        }
    }

    Ok(result)
}
