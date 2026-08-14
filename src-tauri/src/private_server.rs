// private_server.rs — 私人服务器管理 Tauri 命令模块
// 职责：服务器列表 CRUD、远程拉取、MC Server List Ping、剪贴板

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::AppHandle;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// 远程服务器列表 API
const REMOTE_API_URL: &str = "https://www.verselauncher.cn/api/servers.json";

// ============== 路径工具 ==============

fn private_server_dir() -> std::path::PathBuf {
    crate::storage::resolve_data_dir().join("private-server")
}

fn servers_file_path() -> std::path::PathBuf {
    private_server_dir().join("servers.json")
}

// 确保数据目录和文件存在
fn ensure_data_file() {
    let dir = private_server_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = servers_file_path();
    if !file.exists() {
        let _ = std::fs::write(&file, "[]");
    }
}

// ============== 本地读写 ==============

fn load_servers_local() -> Vec<Value> {
    let file = servers_file_path();
    if let Ok(content) = std::fs::read_to_string(&file) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(arr) = v.as_array() {
                return arr.clone();
            }
        }
    }
    Vec::new()
}

fn save_servers_local(list: &[Value]) -> Value {
    ensure_data_file();
    let file = servers_file_path();
    match serde_json::to_string_pretty(list) {
        Ok(json_str) => {
            if std::fs::write(&file, json_str).is_ok() {
                json!({ "ok": true })
            } else {
                json!({ "ok": false, "error": "写入文件失败" })
            }
        }
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

// ============== 远程拉取 ==============

async fn fetch_remote_servers() -> Value {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };

    let resp = match client
        .get(REMOTE_API_URL)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };

    let parsed: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": format!("JSON 解析失败: {}", e) }),
    };

    if let Some(arr) = parsed.as_array() {
        if !arr.is_empty() {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let servers: Vec<Value> = arr
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    json!({
                        "id": s.get("id").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_else(|| format!("srv_remote_{}", i)),
                        "name": s.get("name").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_else(|| "未知服务器".to_string()),
                        "address": s.get("address").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_default(),
                        "description": s.get("description").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_default(),
                        "icon": s.get("icon").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_default(),
                        "modpackUrl": s.get("modpackUrl").and_then(|v| v.as_str()).map(|v| v.to_string())
                            .unwrap_or_default(),
                        "maxPlayers": s.get("maxPlayers").cloned().unwrap_or(Value::Null),
                        "createdAt": s.get("createdAt").cloned().unwrap_or_else(|| json!(now_ms)),
                    })
                })
                .collect();
            return json!({ "ok": true, "servers": servers });
        }
    }
    json!({ "ok": false, "error": "数据格式错误" })
}

// ============== 地址解析 ==============
// 支持 host:port、[ipv6]:port、host（默认 25565）

fn parse_address(address: &str) -> Option<(String, u16)> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return None;
    }
    // IPv6 [addr]:port
    if trimmed.starts_with('[') {
        let end = trimmed.find(']')?;
        let host = trimmed[1..end].to_string();
        let port_part = &trimmed[end + 1..];
        let port = if port_part.starts_with(':') {
            port_part[1..].parse::<u16>().unwrap_or(25565)
        } else {
            25565
        };
        return Some((host, port));
    }
    // 普通 host:port
    let last_colon = trimmed.rfind(':');
    let first_colon = trimmed.find(':');
    if let (Some(last), Some(first)) = (last_colon, first_colon) {
        if last > first {
            // 多个冒号且无方括号，按 IPv6 无括号处理，暂不支持
            return None;
        }
    }
    if let Some(idx) = last_colon {
        let host = &trimmed[..idx];
        let port_str = &trimmed[idx + 1..];
        let port = port_str.parse::<u16>().unwrap_or(25565);
        return Some((host.to_string(), port));
    }
    Some((trimmed.to_string(), 25565))
}

// ============== VarInt 编解码 ==============

fn write_varint(value: i32) -> Vec<u8> {
    let mut v = value as u32;
    let mut buf = Vec::new();
    loop {
        let mut temp = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            temp |= 0x80;
        }
        buf.push(temp);
        if v == 0 {
            break;
        }
    }
    buf
}

fn read_varint(buf: &[u8], offset: usize) -> Result<(i32, usize), String> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    let mut pos = offset;
    loop {
        if pos >= buf.len() {
            return Err("VarInt 读取越界".to_string());
        }
        let byte = buf[pos];
        pos += 1;
        if shift >= 32 {
            return Err("VarInt 过长".to_string());
        }
        result |= ((byte & 0x7F) as u32) << shift;
        shift += 7;
        if (byte & 0x80) == 0 {
            break;
        }
    }
    Ok((result as i32, pos - offset))
}

fn write_string(s: &str) -> Vec<u8> {
    let encoded = s.as_bytes();
    let mut buf = write_varint(encoded.len() as i32);
    buf.extend_from_slice(encoded);
    buf
}

// ============== MC Server List Ping ==============

async fn check_server_status(address: &str) -> Value {
    let (host, port) = match parse_address(address) {
        Some(hp) => hp,
        None => return json!({ "online": false, "error": "地址格式错误" }),
    };

    let start = Instant::now();
    let addr = format!("{}:{}", host, port);

    // TCP 连接（8 秒超时）
    let mut stream = match tokio::time::timeout(
        Duration::from_secs(8),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return json!({ "online": false, "error": format!("连接失败: {}", e), "host": host, "port": port })
        }
        Err(_) => {
            return json!({ "online": false, "error": "连接超时", "host": host, "port": port })
        }
    };
    let _ = stream.set_nodelay(true);

    // ① Handshake 包（packet ID 0x00，next state = 1）
    let mut handshake_payload = Vec::new();
    handshake_payload.extend_from_slice(&write_varint(-1)); // 协议版本 -1（自动匹配）
    handshake_payload.extend_from_slice(&write_string(&host)); // 服务器地址
    handshake_payload.extend_from_slice(&port.to_be_bytes()); // 端口（大端 u16）
    handshake_payload.extend_from_slice(&write_varint(1)); // next state: 1 = status

    let mut handshake_packet = Vec::new();
    let handshake_len = handshake_payload.len() + 1; // payload + packet ID
    handshake_packet.extend_from_slice(&write_varint(handshake_len as i32));
    handshake_packet.extend_from_slice(&write_varint(0x00)); // packet ID
    handshake_packet.extend_from_slice(&handshake_payload);

    if let Err(e) = stream.write_all(&handshake_packet).await {
        return json!({ "online": false, "error": format!("发送握手失败: {}", e), "host": host, "port": port });
    }

    // ② Request 包（packet ID 0x00，空 payload）
    let mut request_packet = Vec::new();
    request_packet.extend_from_slice(&write_varint(1)); // 包长度
    request_packet.extend_from_slice(&write_varint(0x00)); // packet ID

    if let Err(e) = stream.write_all(&request_packet).await {
        return json!({ "online": false, "error": format!("发送请求失败: {}", e), "host": host, "port": port });
    }

    // ③ 读取 Response（可能分多次到达）
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let json_str: String;

    loop {
        let n = match tokio::time::timeout(Duration::from_secs(8), stream.read(&mut tmp)).await {
            Ok(Ok(0)) => {
                return json!({ "online": false, "error": "连接已关闭", "host": host, "port": port })
            }
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                return json!({ "online": false, "error": format!("读取失败: {}", e), "host": host, "port": port })
            }
            Err(_) => {
                return json!({ "online": false, "error": "读取超时", "host": host, "port": port })
            }
        };
        buf.extend_from_slice(&tmp[..n]);

        match try_parse_response(&buf) {
            Ok(s) => {
                json_str = s;
                break;
            }
            Err(_) => continue, // 数据不完整，继续等待
        }
    }

    let latency = start.elapsed().as_millis() as u64;

    let info: Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            return json!({ "online": false, "error": format!("数据解析失败: {}", e), "host": host, "port": port })
        }
    };

    let motd = parse_motd(&info);
    let version = info
        .get("version")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let protocol = info
        .get("version")
        .and_then(|v| v.get("protocol"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let players_online = info
        .get("players")
        .and_then(|v| v.get("online"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let players_max = info
        .get("players")
        .and_then(|v| v.get("max"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    json!({
        "online": true,
        "latency": latency,
        "host": host,
        "port": port,
        "motd": motd,
        "version": version,
        "playersOnline": players_online,
        "playersMax": players_max,
        "protocol": protocol
    })
}

// 尝试从缓冲区解析一个完整的 Status Response 包
fn try_parse_response(buf: &[u8]) -> Result<String, String> {
    if buf.is_empty() {
        return Err("缓冲区为空".to_string());
    }
    let mut offset = 0;
    let (packet_len, len_bytes) = read_varint(buf, offset)?;
    offset += len_bytes;
    if packet_len <= 0 {
        return Err("包长度无效".to_string());
    }
    if buf.len() < offset + packet_len as usize {
        return Err("数据不完整".to_string());
    }
    let (_packet_id, id_bytes) = read_varint(buf, offset)?;
    offset += id_bytes;
    let (str_len, str_bytes) = read_varint(buf, offset)?;
    offset += str_bytes;
    if str_len < 0 || buf.len() < offset + str_len as usize {
        return Err("字符串不完整".to_string());
    }
    let json_bytes = &buf[offset..offset + str_len as usize];
    String::from_utf8(json_bytes.to_vec()).map_err(|e| e.to_string())
}

// 解析 MOTD（可能是字符串或带 extra 数组的对象）
fn parse_motd(info: &Value) -> String {
    let desc = match info.get("description") {
        Some(d) => d,
        None => return String::new(),
    };
    if let Some(s) = desc.as_str() {
        return s.to_string();
    }
    if let Some(extra) = desc.get("extra").and_then(|v| v.as_array()) {
        let mut result = String::new();
        for e in extra {
            if let Some(text) = e.get("text").and_then(|v| v.as_str()) {
                result.push_str(text);
            }
        }
        return result;
    }
    if let Some(text) = desc.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    desc.to_string()
}

// ============== Tauri 命令 ==============

/// 列出服务器：优先远程 API，失败回退本地
#[tauri::command]
pub async fn private_server_list(_app: AppHandle) -> Value {
    let remote = fetch_remote_servers().await;
    if remote.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let mut result = remote;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("source".to_string(), json!("remote"));
        }
        return result;
    }
    // 回退本地
    let servers = load_servers_local();
    json!({ "ok": true, "servers": servers, "source": "local" })
}

/// 保存整个服务器列表
#[tauri::command]
pub async fn private_server_save(_app: AppHandle, servers: Vec<Value>) -> Value {
    save_servers_local(&servers)
}

/// 添加一个服务器
#[tauri::command]
pub async fn private_server_add(_app: AppHandle, server: Value) -> Value {
    let name = server
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let address = server
        .get("address")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() || address.is_empty() {
        return json!({ "ok": false, "error": "名称和地址不能为空" });
    }

    let mut list = load_servers_local();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let id = server
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // 生成 6 位 base36 随机串
            let rand_str: String = (0..6)
                .map(|_| {
                    let c = rand::random::<u32>() % 36;
                    char::from_digit(c, 36).unwrap_or('0')
                })
                .collect();
            format!("srv_{}_{}", now_ms, rand_str)
        });

    let new_server = json!({
        "id": id,
        "name": name,
        "address": address,
        "description": server.get("description").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "icon": server.get("icon").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "modpackUrl": server.get("modpackUrl").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "maxPlayers": server.get("maxPlayers").cloned().unwrap_or(Value::Null),
        "createdAt": server.get("createdAt").cloned().unwrap_or_else(|| json!(now_ms)),
    });

    list.push(new_server.clone());
    let save_result = save_servers_local(&list);
    if !save_result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return save_result;
    }
    json!({ "ok": true, "server": new_server })
}

/// 更新服务器（按 id）
#[tauri::command]
pub async fn private_server_update(_app: AppHandle, server: Value) -> Value {
    let id = match server.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({ "ok": false, "error": "缺少服务器 id" }),
    };

    let mut list = load_servers_local();
    let idx = match list
        .iter()
        .position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        Some(i) => i,
        None => return json!({ "ok": false, "error": "服务器不存在" }),
    };

    let existing = &list[idx];
    let updated = json!({
        "id": id,
        "name": server.get("name").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "address": server.get("address").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "description": server.get("description").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "icon": server.get("icon").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "modpackUrl": server.get("modpackUrl").and_then(|v| v.as_str()).unwrap_or("").trim(),
        "maxPlayers": server.get("maxPlayers").cloned().unwrap_or(Value::Null),
        "createdAt": existing.get("createdAt").cloned().unwrap_or_else(|| json!(chrono::Utc::now().timestamp_millis())),
    });

    list[idx] = updated.clone();
    let save_result = save_servers_local(&list);
    if !save_result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return save_result;
    }
    json!({ "ok": true, "server": updated })
}

/// 删除服务器
#[tauri::command]
pub async fn private_server_delete(_app: AppHandle, id: String) -> Value {
    let mut list = load_servers_local();
    let idx = match list
        .iter()
        .position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id))
    {
        Some(i) => i,
        None => return json!({ "ok": false, "error": "服务器不存在" }),
    };
    list.remove(idx);
    let save_result = save_servers_local(&list);
    if !save_result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return save_result;
    }
    json!({ "ok": true })
}

/// 检测服务器在线状态（MC Server List Ping）
#[tauri::command]
pub async fn private_server_check(_app: AppHandle, address: String) -> Value {
    check_server_status(&address).await
}

/// 复制地址到剪贴板
#[tauri::command]
pub fn private_server_copy_address(address: String) -> Value {
    match arboard::Clipboard::new() {
        Ok(mut cb) => match cb.set_text(&address) {
            Ok(_) => json!({ "ok": true }),
            Err(e) => json!({ "ok": false, "error": e.to_string() }),
        },
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

// ============== 图标转 data URL ==============
// 私人服务器图标可能是本地路径或网络 URL，Tauri 的 WebView 无法直接加载本地文件路径，
// 这里统一读成 base64 data URL 返回给前端，保证图标能正常显示。

fn mime_from_path(icon: &str) -> &'static str {
    let lower = icon.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}

fn mime_from_content(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        "image/png"
    } else if bytes.len() >= 3 && &bytes[..3] == b"\xff\xd8\xff" {
        "image/jpeg"
    } else if bytes.len() >= 6
        && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a")
    {
        "image/gif"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() >= 4 && &bytes[..4] == b"<svg" {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

/// 把图标（本地路径或网络 URL）转成 base64 data URL
#[tauri::command]
pub async fn private_server_icon(_app: AppHandle, icon: String) -> Value {
    let icon = icon.trim().to_string();
    if icon.is_empty() {
        return json!({ "ok": false, "error": "图标地址为空", "dataUrl": "" });
    }

    // 1. 本地文件路径
    let path = std::path::Path::new(&icon);
    if path.exists() && path.is_file() {
        if let Ok(bytes) = std::fs::read(&icon) {
            if !bytes.is_empty() {
                let mime = mime_from_path(&icon);
                let b64 = BASE64.encode(&bytes);
                return json!({ "ok": true, "dataUrl": format!("data:{};base64,{}", mime, b64) });
            }
        }
    }

    // 2. 网络 URL（http/https）
    if icon.starts_with("http://") || icon.starts_with("https://") {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("VersePC-Tauri/1.0")
            .build()
        {
            Ok(c) => c,
            Err(_) => return json!({ "ok": false, "error": "创建请求失败", "dataUrl": "" }),
        };
        match client.get(&icon).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(bytes) = resp.bytes().await {
                    if !bytes.is_empty() {
                        let mime = mime_from_content(&bytes);
                        let b64 = BASE64.encode(&bytes);
                        return json!({ "ok": true, "dataUrl": format!("data:{};base64,{}", mime, b64) });
                    }
                }
            }
            _ => {}
        }
    }

    json!({ "ok": false, "error": "图标加载失败", "dataUrl": "" })
}
