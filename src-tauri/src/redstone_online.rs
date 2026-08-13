// redstone_online.rs — 红石联机内网穿透 Tauri 命令模块
// 职责：节点列表、API Key、隧道启动/关闭、本地中继、保活、自动重连
// 迁移自原 Electron 项目 main/redstone-online.js

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use rand::Rng;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

// ============== 常量 ==============

const REGISTRY_URL: &str = "https://shithub.site/server.json";
const HTTP_PORT: u16 = 3000;
const TCP_PORT: u16 = 7000;
const APIKEY_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const RECONNECT_MAX_ATTEMPTS: u32 = 5;

// ============== 运行时状态 ==============

#[derive(Clone)]
struct TunnelInfo {
    listen_port: u16,
    server_address: String,
    address: String,
    title: String,
    description: String,
    public_access: bool,
    allow_offline: bool,
}

struct RedstoneState {
    apikey: String,
    servers: Vec<Value>,
    current_server_idx: usize,
    tunnel: Option<TunnelInfo>,
    // 控制连接的写端，用于把游戏数据回传到服务器
    control_writer: Option<Arc<Mutex<OwnedWriteHalf>>>,
    running: bool,
    stopping: bool,
    last_params: Option<Value>,
    reconnect_attempts: u32,
    reconnecting: bool,
}

impl RedstoneState {
    fn new() -> Self {
        Self {
            apikey: String::new(),
            servers: Vec::new(),
            current_server_idx: 0,
            tunnel: None,
            control_writer: None,
            running: false,
            stopping: false,
            last_params: None,
            reconnect_attempts: 0,
            reconnecting: false,
        }
    }
}

static STATE: OnceLock<Mutex<RedstoneState>> = OnceLock::new();

fn state() -> &'static Mutex<RedstoneState> {
    STATE.get_or_init(|| Mutex::new(RedstoneState::new()))
}

// ============== 路径 ==============

fn redstone_dir() -> PathBuf {
    crate::storage::resolve_data_dir().join("redstone-online")
}

fn apikey_file() -> PathBuf {
    redstone_dir().join("apikey.txt")
}

fn debug_log_file() -> PathBuf {
    redstone_dir().join("debug.log")
}

// ============== 工具函数 ==============

fn write_debug(msg: &str) {
    let _ = std::fs::create_dir_all(redstone_dir());
    let ts = chrono::Utc::now().to_rfc3339();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(debug_log_file())
    {
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}

fn make_apikey() -> String {
    let mut rng = rand::thread_rng();
    (0..20)
        .map(|_| {
            let idx = rng.gen_range(0..APIKEY_CHARS.len());
            APIKEY_CHARS[idx] as char
        })
        .collect()
}

async fn load_or_create_apikey() -> Result<String, String> {
    let dir = redstone_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let path = apikey_file();
    if let Ok(content) = tokio::fs::read_to_string(&path).await {
        let key = content.trim().to_string();
        if key.len() >= 16 {
            return Ok(key);
        }
    }
    let new_key = make_apikey();
    tokio::fs::write(&path, &new_key)
        .await
        .map_err(|e| e.to_string())?;
    Ok(new_key)
}

// 确保全局状态中有 apikey，没有则加载/生成
// 短暂持锁取出已存在的 apikey，避免在文件 IO 期间长时间持锁阻塞其他命令
async fn ensure_apikey() -> Result<String, String> {
    let existing = {
        let s = state().lock().await;
        s.apikey.clone()
    };
    if !existing.is_empty() {
        return Ok(existing);
    }
    let new_key = load_or_create_apikey().await?;
    let mut s = state().lock().await;
    s.apikey = new_key.clone();
    Ok(new_key)
}

fn http_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("VersePC-Tauri")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn emit_log(app: &AppHandle, msg: &str) {
    let payload = json!({
        "message": msg,
        "ts": chrono::Utc::now().to_rfc3339()
    });
    let _ = app.emit("redstone:log", payload);
}

// ============== HTTP API 函数 ==============

// 拉取服务器节点列表，失败回退默认节点
async fn fetch_server_list() -> Vec<Value> {
    let client = http_client(6);
    match client.get(REGISTRY_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(obj) = resp.json::<Value>().await {
                if let Some(map) = obj.as_object() {
                    let list: Vec<Value> = map
                        .iter()
                        .filter_map(|(name, addr)| {
                            let addr_str = addr.as_str().unwrap_or("").trim().to_string();
                            if addr_str.is_empty() {
                                None
                            } else {
                                Some(json!({ "name": name, "address": addr_str }))
                            }
                        })
                        .collect();
                    if !list.is_empty() {
                        return list;
                    }
                }
            }
        }
        _ => {}
    }
    vec![json!({ "name": "上海", "address": "122.51.108.96" })]
}

// 注册 API Key（幂等，409 视为已存在）
async fn register_apikey(server_address: &str, apikey: &str) -> Result<bool, String> {
    let url = format!("http://{}:{}/apikey", server_address, HTTP_PORT);
    let client = http_client(6);
    let resp = client
        .post(&url)
        .json(&json!({ "apikey": apikey }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status == 200 || status == 409 {
        Ok(status == 409)
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("register apikey failed: {} {}", status, body))
    }
}

// 发送一次创建隧道请求
async fn send_create_tunnel(
    client: &reqwest::Client,
    url: &str,
    apikey: &str,
    body: &Option<Value>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut req = client.post(url).header("Authorization", apikey);
    if let Some(b) = body {
        req = req.json(b);
    }
    req.send().await
}

// 创建隧道，返回 (listen_port, tunnel_id)
async fn create_tunnel(
    server_address: &str,
    apikey: &str,
    title: &str,
    description: &str,
    public_access: bool,
    allow_offline: bool,
) -> Result<(u16, u64), String> {
    let url = format!(
        "http://{}:{}/tunnels?publicAccess={}",
        server_address,
        HTTP_PORT,
        if public_access { 1 } else { 0 }
    );
    let client = http_client(10);

    let body_opt = if public_access {
        Some(json!({
            "title": title.chars().take(8).collect::<String>(),
            "description": description.chars().take(100).collect::<String>(),
            "online": !allow_offline
        }))
    } else {
        None
    };

    let resp = send_create_tunnel(&client, &url, apikey, &body_opt)
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status >= 200 && status < 300 {
        let obj: Value = resp.json().await.map_err(|e| e.to_string())?;
        let listen_port = obj
            .get("listenPort")
            .and_then(|v| v.as_u64())
            .ok_or("missing listenPort")? as u16;
        let tunnel_id = obj.get("tunnelId").and_then(|v| v.as_u64()).unwrap_or(0);
        return Ok((listen_port, tunnel_id));
    }
    // 429：已有隧道 → 先 DELETE 再重试一次
    if status == 429 {
        let _ = close_tunnel_api(server_address, apikey).await;
        let resp2 = send_create_tunnel(&client, &url, apikey, &body_opt)
            .await
            .map_err(|e| e.to_string())?;
        let status2 = resp2.status().as_u16();
        if status2 >= 200 && status2 < 300 {
            let obj: Value = resp2.json().await.map_err(|e| e.to_string())?;
            let listen_port = obj
                .get("listenPort")
                .and_then(|v| v.as_u64())
                .ok_or("missing listenPort")? as u16;
            let tunnel_id = obj.get("tunnelId").and_then(|v| v.as_u64()).unwrap_or(0);
            return Ok((listen_port, tunnel_id));
        }
        let body = resp2.text().await.unwrap_or_default();
        return Err(format!("create tunnel retry failed: {} {}", status2, body));
    }
    let body = resp.text().await.unwrap_or_default();
    Err(format!("create tunnel failed: {} {}", status, body))
}

// 关闭隧道 HTTP API
async fn close_tunnel_api(server_address: &str, apikey: &str) -> Result<(u16, String), String> {
    let url = format!("http://{}:{}/tunnels", server_address, HTTP_PORT);
    let client = http_client(8);
    let resp = client
        .delete(&url)
        .header("Authorization", apikey)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    Ok((status, body))
}

// 拉取公开房间列表
async fn list_public_tunnels(server_address: &str, offset: u32, apikey: &str) -> Result<Value, String> {
    let client = http_client(8);
    let mut last_err = String::new();

    // 尝试 1: 不带 apikey，参数名 offset / from 都试一次
    for param_name in &["offset", "from"] {
        let url = format!(
            "http://{}:{}/tunnels/list?{}={}",
            server_address, HTTP_PORT, param_name, offset
        );
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(obj) = resp.json::<Value>().await {
                    write_debug(&format!("[listPublicTunnels] OK with ?{}={}", param_name, offset));
                    return Ok(obj);
                }
            }
            Ok(resp) => {
                last_err = format!("list tunnels failed: {}", resp.status());
                write_debug(&format!(
                    "[listPublicTunnels] ?{}={} -> {}",
                    param_name,
                    offset,
                    resp.status()
                ));
            }
            Err(e) => {
                last_err = format!("list tunnels error: {}", e);
                write_debug(&format!("[listPublicTunnels] ?{}={} -> {}", param_name, offset, e));
            }
        }
    }

    // 尝试 2: 加 apikey 再试
    let url = format!(
        "http://{}:{}/tunnels/list?offset={}",
        server_address, HTTP_PORT, offset
    );
    match client
        .get(&url)
        .header("Authorization", apikey)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(obj) = resp.json::<Value>().await {
                write_debug("[listPublicTunnels] OK with apikey");
                return Ok(obj);
            }
        }
        Ok(resp) => {
            write_debug(&format!("[listPublicTunnels] with apikey -> {}", resp.status()));
        }
        Err(e) => {
            write_debug(&format!("[listPublicTunnels] with apikey -> {}", e));
        }
    }

    Err(last_err)
}

// 获取单个公开房间的模组列表
async fn get_tunnel_mods(server_address: &str, tunnel_id: u64, apikey: &str) -> Result<Value, String> {
    let client = http_client(8);
    let url = format!(
        "http://{}:{}/tunnels/mods?id={}",
        server_address, HTTP_PORT, tunnel_id
    );

    // 尝试无认证
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(obj) = resp.json::<Value>().await {
                return Ok(obj);
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            return Ok(json!({ "mods": [] }));
        }
        _ => {}
    }

    // 尝试加 apikey
    match client
        .get(&url)
        .header("Authorization", apikey)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(obj) = resp.json::<Value>().await {
                return Ok(obj);
            }
        }
        Ok(resp) if resp.status().as_u16() == 404 => {
            return Ok(json!({ "mods": [] }));
        }
        _ => {}
    }

    Err("get tunnel mods failed".to_string())
}

// 查询当前用户已存在的隧道
async fn query_existing_tunnel(server_address: &str, apikey: &str) -> Result<Option<u16>, String> {
    let url = format!("http://{}:{}/tunnels", server_address, HTTP_PORT);
    let client = http_client(6);
    let resp = client
        .get(&url)
        .header("Authorization", apikey)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().as_u16() != 200 {
        return Ok(None);
    }
    let obj: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(tunnels) = obj.get("tunnels").and_then(|v| v.as_array()) {
        if let Some(first) = tunnels.first() {
            if let Some(port) = first.get("listenPort").and_then(|v| v.as_u64()) {
                return Ok(Some(port as u16));
            }
        }
    }
    Ok(None)
}

// ============== MC Server List Ping 协议 ==============

fn write_varint(mut value: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    loop {
        let mut temp = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            temp |= 0x80;
        }
        buf.push(temp);
        if value == 0 {
            break;
        }
    }
    buf
}

// 构造完整 MC SLP ping 包：Handshake(nextState=1) + StatusRequest + PingRequest
fn build_full_ping(port: u16) -> Vec<u8> {
    // Handshake payload: protocolVersion + serverAddr + port + nextState
    let proto_ver = write_varint(0);
    let addr = b"127.0.0.1";
    let addr_len = write_varint(addr.len() as u32);
    let mut port_bytes = [0u8; 2];
    port_bytes[0] = (port >> 8) as u8;
    port_bytes[1] = (port & 0xff) as u8;
    let next_state = write_varint(1);

    let mut hs_payload = Vec::new();
    hs_payload.extend_from_slice(&proto_ver);
    hs_payload.extend_from_slice(&addr_len);
    hs_payload.extend_from_slice(addr);
    hs_payload.extend_from_slice(&port_bytes);
    hs_payload.extend_from_slice(&next_state);

    // Handshake 包帧：VarInt(包长) + VarInt(packetId=0) + payload
    let hs_pid = write_varint(0);
    let hs_len = write_varint((hs_pid.len() + hs_payload.len()) as u32);
    let mut handshake = Vec::new();
    handshake.extend_from_slice(&hs_len);
    handshake.extend_from_slice(&hs_pid);
    handshake.extend_from_slice(&hs_payload);

    // Status Request 包：VarInt(包长=1) + VarInt(packetId=0)
    let req_pid = write_varint(0);
    let req_len = write_varint(req_pid.len() as u32);
    let mut request = Vec::new();
    request.extend_from_slice(&req_len);
    request.extend_from_slice(&req_pid);

    // Ping Request 包：VarInt(包长=9) + VarInt(packetId=1) + 8 字节时间戳
    let payload = chrono::Utc::now().timestamp_millis() as u64;
    let ping_pid = write_varint(1);
    let ping_len = write_varint((ping_pid.len() + 8) as u32);
    let mut ping_request = Vec::new();
    ping_request.extend_from_slice(&ping_len);
    ping_request.extend_from_slice(&ping_pid);
    ping_request.extend_from_slice(&payload.to_be_bytes());

    let mut result = Vec::new();
    result.extend_from_slice(&handshake);
    result.extend_from_slice(&request);
    result.extend_from_slice(&ping_request);
    result
}

// ============== 数据包过滤 ==============

// 检测 HTTP 探测包（互联网扫描机器人会发 HTTP 请求，不能转发给 MC 服务器）
fn is_http_probe(buf: &[u8]) -> bool {
    if buf.len() < 4 {
        return false;
    }
    let first4 = &buf[..4];
    let upper: Vec<u8> = first4.iter().map(|b| b.to_ascii_uppercase()).collect();
    matches!(
        upper.as_slice(),
        b"GET " | b"POST" | b"PUT " | b"HEAD" | b"DELE" | b"PATC" | b"OPTI" | b"CONN" | b"TRAC"
    )
}

// 检测 MC 新玩家连接的 Handshake 包
// 启发式：第1字节是合理长度，第2字节=0x00（packetId），后续字节像协议版本+地址
fn is_mc_handshake(buf: &[u8]) -> bool {
    if buf.len() < 4 {
        return false;
    }
    let pkt_len = buf[0];
    if pkt_len < 0x10 || pkt_len > 0x20 {
        return false;
    }
    if buf[1] != 0x00 {
        return false;
    }
    if buf[2] < 0x80 {
        return false;
    }
    // 解析 VarInt 协议版本
    let mut proto: u32 = 0;
    let mut shift: u32 = 0;
    let mut idx = 2usize;
    while idx < buf.len() && idx < 6 {
        let b = buf[idx];
        proto |= ((b & 0x7F) as u32) << shift;
        shift += 7;
        idx += 1;
        if (b & 0x80) == 0 {
            break;
        }
    }
    if proto < 393 || proto > 800 {
        return false;
    }
    if idx >= buf.len() {
        return false;
    }
    let addr_len = buf[idx] as usize;
    if addr_len < 5 || addr_len > 30 {
        return false;
    }
    let addr_start = idx + 1;
    if addr_start + addr_len > buf.len() {
        return false;
    }
    let mut ascii_count = 0;
    for i in addr_start..addr_start + addr_len {
        if buf[i] >= 0x20 && buf[i] < 0x7F {
            ascii_count += 1;
        }
    }
    ascii_count >= addr_len.saturating_sub(2)
}

// ============== 本地中继 ==============
// 把控制连接的数据双向转发到本地 gamePort
// 控制连接在 OK TUNNEL 后变成数据通道

// 启动本地中继任务
// 控制连接的读端、写端传入，gamePort 为本地游戏端口
fn spawn_local_relay(
    app: AppHandle,
    control_read: OwnedReadHalf,
    control_write: Arc<Mutex<OwnedWriteHalf>>,
    game_port: u16,
) {
    tokio::spawn(async move {
        let mut control_read = control_read;
        let mut buf = vec![0u8; 8192];

        // 首次建立 game socket
        let mut game: Option<TcpStream> = match TcpStream::connect(("127.0.0.1", game_port)).await {
            Ok(s) => Some(s),
            Err(e) => {
                emit_log(&app, &format!("[诊断] gameSocket 连接失败: {}", e));
                write_debug(&format!("[startLocalRelay] gameSocket 连接失败: {}", e));
                None
            }
        };

        if game.is_some() {
            emit_log(&app, &format!("[诊断] gameSocket 已连接 127.0.0.1:{}", game_port));
        }

        let mut game_buf = vec![0u8; 8192];

        loop {
            // 用 select! 同时读 control 和 game
            tokio::select! {
                // game → control
                n = async {
                    match game.as_mut() {
                        Some(g) => g.read(&mut game_buf).await,
                        None => std::future::pending::<Result<usize, std::io::Error>>().await,
                    }
                } => {
                    match n {
                        Ok(0) | Err(_) => {
                            // game socket 断开，尝试重建
                            emit_log(&app, "[诊断] gameSocket 关闭，尝试重建");
                            write_debug("[startLocalRelay] gameSocket 关闭，尝试重建");
                            game = TcpStream::connect(("127.0.0.1", game_port)).await.ok();
                            if game.is_none() {
                                // 重建失败，继续等待控制连接数据触发再次重建
                            }
                        }
                        Ok(n) => {
                            let mut w = control_write.lock().await;
                            if w.write_all(&game_buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                // control → game
                n = control_read.read(&mut buf) => {
                    match n {
                        Ok(0) | Err(_) => {
                            // 控制连接断开
                            emit_log(&app, "控制连接已关闭");
                            write_debug("[startLocalRelay] 控制连接已关闭");
                            break;
                        }
                        Ok(n) => {
                            let data = &buf[..n];
                            write_debug(&format!(
                                "[startLocalRelay] 控制连接收到数据 {} 字节，前16字节: {}",
                                data.len(),
                                data.iter().take(16).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")
                            ));
                            // 过滤 HTTP 探测包
                            if is_http_probe(data) {
                                emit_log(&app, &format!("忽略 HTTP 探测包 ({} 字节)", data.len()));
                                write_debug("[startLocalRelay] 忽略 HTTP 探测包");
                                continue;
                            }
                            // 检测新玩家 Handshake 包：强制重建 gameSocket（旧的可能还没 close）
                            if is_mc_handshake(data) {
                                emit_log(&app, "[诊断] 检测到新玩家 Handshake 包，重建 gameSocket");
                                write_debug("[startLocalRelay] 检测到新玩家 Handshake 包，重建 gameSocket");
                                game = TcpStream::connect(("127.0.0.1", game_port)).await.ok();
                                if game.is_none() {
                                    emit_log(&app, "[诊断] gameSocket 重建失败，等待重试");
                                    continue;
                                }
                            }
                            // game socket 不存在则建一个
                            if game.is_none() {
                                game = TcpStream::connect(("127.0.0.1", game_port)).await.ok();
                            }
                            if let Some(g) = game.as_mut() {
                                if g.write_all(data).await.is_err() {
                                    // 写入失败，关闭旧 socket，下次循环重建
                                    emit_log(&app, "[诊断] gameSocket 写入失败，将重建");
                                    game = None;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 中继结束：清理状态并触发重连
        emit_log(&app, "本地中继任务退出");
        write_debug("[startLocalRelay] 本地中继任务退出");

        // 关闭控制连接写端，让对端感知断开
        {
            let mut w = control_write.lock().await;
            let _ = w.shutdown().await;
        }

        // 更新状态，触发重连
        let (prev_tunnel, apikey, stopping, has_params) = {
            let mut s = state().lock().await;
            s.running = false;
            s.control_writer = None;
            (
                s.tunnel.take(),
                s.apikey.clone(),
                s.stopping,
                s.last_params.is_some(),
            )
        };
        // 主动调用 HTTP 关闭隧道（不持有 state 锁，避免阻塞其他命令）
        if let Some(t) = prev_tunnel {
            if !apikey.is_empty() {
                let _ = close_tunnel_api(&t.server_address, &apikey).await;
            }
        }
        let should_reconnect = !stopping && has_params;

        if should_reconnect {
            let _ = app.emit("redstone:disconnected", json!({ "reason": "control connection closed" }));
            schedule_reconnect(app).await;
        } else {
            let _ = app.emit("redstone:disconnected", json!({ "reason": "stopped" }));
        }
    });
}

// ============== 保活任务 ==============
// 每 10 秒发送完整 MC SLP ping 包到游戏端口，检测游戏是否在线
// 连续 3 次失败（约 30 秒）触发关隧道
fn spawn_keepalive(app: AppHandle, game_port: u16, start_delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(start_delay).await;

        let mut fail_count: u32 = 0;
        let ping_packet = build_full_ping(game_port);

        loop {
            // 检查运行状态
            {
                let s = state().lock().await;
                if !s.running || s.stopping {
                    return;
                }
            }

            // 发送 ping 包到游戏端口
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                send_ping_to_game(game_port, &ping_packet),
            )
            .await;

            match result {
                Ok(Ok(_)) => {
                    fail_count = 0;
                }
                _ => {
                    fail_count += 1;
                    emit_log(
                        &app,
                        &format!("[诊断] 游戏端口保活失败 {}/3", fail_count),
                    );
                    if fail_count >= 3 {
                        emit_log(&app, "检测到游戏已关闭（端口连续 3 次不可达），自动关闭隧道");
                        // 触发停止
                        let app2 = app.clone();
                        tokio::spawn(async move {
                            let _ = stop_tunnel_inner(&app2).await;
                        });
                        return;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

// 向游戏端口发送一次 ping 包
async fn send_ping_to_game(game_port: u16, ping_packet: &[u8]) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(("127.0.0.1", game_port)).await?;
    stream.write_all(ping_packet).await?;
    // 读回响应（最多读 1024 字节，丢弃即可）
    let mut buf = vec![0u8; 1024];
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
    Ok(())
}

// ============== 自动重连 ==============

async fn schedule_reconnect(app: AppHandle) {
    let (attempt, delay_ms, can_reconnect) = {
        let mut s = state().lock().await;
        if s.stopping || s.last_params.is_none() {
            s.reconnecting = false;
            return;
        }
        if s.reconnect_attempts >= RECONNECT_MAX_ATTEMPTS {
            s.reconnecting = false;
            emit_log(
                &app,
                &format!("自动重连失败：已达最大重试次数 {} 次", RECONNECT_MAX_ATTEMPTS),
            );
            let _ = app.emit("redstone:disconnected", json!({ "reason": "max reconnect attempts reached" }));
            return;
        }
        s.reconnect_attempts += 1;
        s.reconnecting = true;
        let delay = 3000u64 * 2u64.pow(s.reconnect_attempts - 1);
        (s.reconnect_attempts, delay, true)
    };

    if !can_reconnect {
        return;
    }

    emit_log(
        &app,
        &format!(
            "将在 {} 秒后自动重连（第 {}/{} 次）",
            delay_ms / 1000,
            attempt,
            RECONNECT_MAX_ATTEMPTS
        ),
    );
    let _ = app.emit(
        "redstone:reconnecting",
        json!({ "attempt": attempt, "maxAttempts": RECONNECT_MAX_ATTEMPTS, "delay": delay_ms }),
    );

    tokio::time::sleep(Duration::from_millis(delay_ms)).await;

    // 检查停止标志
    {
        let s = state().lock().await;
        if s.stopping || s.last_params.is_none() {
            return;
        }
    }

    emit_log(
        &app,
        &format!("正在自动重连（第 {}/{} 次）...", attempt, RECONNECT_MAX_ATTEMPTS),
    );

    // 获取参数并尝试重连
    let params = {
        let s = state().lock().await;
        s.last_params.clone()
    };

    if let Some(params) = params {
        // 用 Box::pin 打破 async fn 相互递归（schedule_reconnect ↔ start_tunnel_inner）
        let result = Box::pin(start_tunnel_inner(&app, params, true)).await;
        if result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            // 重连成功
            let mut s = state().lock().await;
            s.reconnect_attempts = 0;
            s.reconnecting = false;
        } else {
            // 失败：start_tunnel_inner 内部会再次触发 schedule_reconnect
            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            emit_log(&app, &format!("自动重连失败: {}", err));
        }
    }
}

// ============== 隧道启动 / 关闭 ==============

async fn start_tunnel_inner(app: &AppHandle, params: Value, is_reconnect: bool) -> Value {
    let log = |msg: &str| {
        emit_log(app, msg);
    };

    // 检查是否已在运行
    {
        let s = state().lock().await;
        if s.running {
            return json!({ "ok": false, "error": "隧道已在运行中，请先关闭" });
        }
    }

    let server_address = params
        .get("serverAddress")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let game_port = params
        .get("gamePort")
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let public_access = params.get("publicAccess").map(|v| v.as_bool().unwrap_or(true)).unwrap_or(
        params.get("isOpen").map(|v| v.as_bool().unwrap_or(true)).unwrap_or(true)
    );
    let allow_offline = params
        .get("allowOffline")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if server_address.is_empty() {
        return json!({ "ok": false, "error": "未指定服务器节点" });
    }
    if game_port == 0 {
        return json!({ "ok": false, "error": "游戏端口无效" });
    }

    // 设置运行状态
    {
        let mut s = state().lock().await;
        s.stopping = false;
        s.running = true;
        if !is_reconnect {
            s.last_params = Some(params.clone());
            s.reconnect_attempts = 0;
            s.reconnecting = false;
        }
    }

    // 1. 确保有 API Key
    let apikey = match ensure_apikey().await {
        Ok(k) => k,
        Err(e) => {
            let mut s = state().lock().await;
            s.running = false;
            return json!({ "ok": false, "error": format!("加载 API Key 失败: {}", e) });
        }
    };
    log(&format!("API Key: {}", apikey));

    // 2. 注册 API Key
    log(&format!("正在注册 API Key 到 {} ...", server_address));
    if let Err(e) = register_apikey(&server_address, &apikey).await {
        log(&format!("注册 API Key 失败: {}", e));
        let mut s = state().lock().await;
        s.running = false;
        if is_reconnect && !s.stopping {
            schedule_reconnect(app.clone()).await;
        }
        return json!({ "ok": false, "error": e });
    }
    log("API Key 已注册");

    // 3. 建立 TCP 控制连接到 7000 端口
    log(&format!("正在连接控制服务器 {}:7000 ...", server_address));
    let control_socket = match tokio::time::timeout(
        Duration::from_secs(8),
        TcpStream::connect((server_address.as_str(), TCP_PORT)),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            log(&format!("连接控制服务器失败: {}", e));
            let mut s = state().lock().await;
            s.running = false;
            if is_reconnect && !s.stopping {
                schedule_reconnect(app.clone()).await;
            }
            return json!({ "ok": false, "error": format!("连接控制服务器失败: {}", e) });
        }
        Err(_) => {
            log("连接控制服务器超时");
            let mut s = state().lock().await;
            s.running = false;
            if is_reconnect && !s.stopping {
                schedule_reconnect(app.clone()).await;
            }
            return json!({ "ok": false, "error": "连接控制服务器超时" });
        }
    };
    let _ = control_socket.set_nodelay(true);
    log("已建立 TCP 控制连接");

    // 4. 发送 apikey 进入连接池
    let (mut control_read, control_write) = control_socket.into_split();
    let control_write = Arc::new(Mutex::new(control_write));

    let apikey_line = format!("{}\n", apikey);
    {
        let mut w = control_write.lock().await;
        if let Err(e) = w.write_all(apikey_line.as_bytes()).await {
            log(&format!("发送 apikey 失败: {}", e));
            let mut s = state().lock().await;
            s.running = false;
            if is_reconnect && !s.stopping {
                schedule_reconnect(app.clone()).await;
            }
            return json!({ "ok": false, "error": format!("发送 apikey 失败: {}", e) });
        }
    }

    // 5. 等待首行响应
    let first_line = match read_line_from_stream(&mut control_read, Duration::from_secs(12)).await {
        Ok(line) => line,
        Err(e) => {
            log(&format!("等待服务器响应失败: {}", e));
            let mut s = state().lock().await;
            s.running = false;
            if is_reconnect && !s.stopping {
                schedule_reconnect(app.clone()).await;
            }
            return json!({ "ok": false, "error": format!("等待服务器响应失败: {}", e) });
        }
    };
    log(&format!("服务器响应: {}", first_line));

    let listen_port: u16;
    if first_line.starts_with("OK TUNNEL ") {
        // 已有隧道，调 API 查询端口
        match query_existing_tunnel(&server_address, &apikey).await {
            Ok(Some(p)) => {
                listen_port = p;
            }
            _ => {
                log("已有隧道但无法获取 listenPort");
                let mut s = state().lock().await;
                s.running = false;
                if is_reconnect && !s.stopping {
                    schedule_reconnect(app.clone()).await;
                }
                return json!({ "ok": false, "error": "已有隧道但无法获取 listenPort" });
            }
        }
    } else if first_line.starts_with("OK WAITING") {
        // 需要创建隧道
        log(if public_access { "正在创建公开房间..." } else { "正在创建私人隧道..." });
        match create_tunnel(
            &server_address,
            &apikey,
            &title,
            &description,
            public_access,
            allow_offline,
        )
        .await
        {
            Ok((port, _tunnel_id)) => {
                listen_port = port;
                log(&format!("隧道已创建，端口: {}", listen_port));
            }
            Err(e) => {
                log(&format!("创建隧道失败: {}", e));
                let mut s = state().lock().await;
                s.running = false;
                if is_reconnect && !s.stopping {
                    schedule_reconnect(app.clone()).await;
                }
                return json!({ "ok": false, "error": e });
            }
        }
        // 等待 OK TUNNEL 通知（超时也继续）
        let _ = read_line_from_stream(&mut control_read, Duration::from_secs(8)).await;
    } else if first_line.starts_with("ERR") {
        let msg = format!("服务器拒绝: {}", first_line);
        log(&msg);
        let mut s = state().lock().await;
        s.running = false;
        if is_reconnect && !s.stopping {
            schedule_reconnect(app.clone()).await;
        }
        return json!({ "ok": false, "error": msg });
    } else {
        let msg = format!("未知响应: {}", first_line);
        log(&msg);
        let mut s = state().lock().await;
        s.running = false;
        if is_reconnect && !s.stopping {
            schedule_reconnect(app.clone()).await;
        }
        return json!({ "ok": false, "error": msg });
    }

    let tunnel_info = TunnelInfo {
        listen_port,
        server_address: server_address.clone(),
        address: format!("{}:{}", server_address, listen_port),
        title: title.clone(),
        description: description.clone(),
        public_access,
        allow_offline,
    };

    // 保存状态
    {
        let mut s = state().lock().await;
        s.tunnel = Some(tunnel_info.clone());
        s.control_writer = Some(control_write.clone());
    }

    // 7. 启动本地中继
    log(&format!("正在启动本地中转 (游戏端口 {})...", game_port));
    spawn_local_relay(app.clone(), control_read, control_write.clone(), game_port);

    log(&format!("隧道已就绪，地址: {}", tunnel_info.address));

    // 8. 启动保活任务（60s 后开始，避免游戏还在加载）
    spawn_keepalive(app.clone(), game_port, Duration::from_secs(60));

    // 重连成功通知
    if is_reconnect {
        log("自动重连成功");
        let _ = app.emit("redstone:reconnected", json!({}));
    }

    json!({ "ok": true, "address": tunnel_info.address, "listenPort": listen_port })
}

async fn stop_tunnel_inner(app: &AppHandle) -> Value {
    let log = |msg: &str| {
        emit_log(app, msg);
    };

    // 设置停止标志，防止重连
    let (server_address, apikey) = {
        let mut s = state().lock().await;
        if s.stopping {
            return json!({ "ok": true });
        }
        s.stopping = true;
        let addr = s.tunnel.as_ref().map(|t| t.server_address.clone()).unwrap_or_default();
        let key = s.apikey.clone();
        (addr, key)
    };

    log("正在关闭隧道...");

    // 关闭控制连接写端（会触发中继任务退出）
    // 先克隆出 Arc，避免在 shutdown 期间持有 state 锁阻塞其他命令
    let cw_opt = {
        let s = state().lock().await;
        s.control_writer.clone()
    };
    if let Some(cw) = cw_opt {
        let mut w = cw.lock().await;
        let _ = w.shutdown().await;
    }

    // 调用 HTTP API 删除隧道
    if !server_address.is_empty() && !apikey.is_empty() {
        match close_tunnel_api(&server_address, &apikey).await {
            Ok((status, body)) => {
                log(&format!("DELETE /tunnels -> {} {}", status, body));
            }
            Err(e) => {
                log(&format!("关闭隧道 API 失败: {}", e));
            }
        }
    }

    // 清理状态
    {
        let mut s = state().lock().await;
        s.tunnel = None;
        s.control_writer = None;
        s.running = false;
        s.stopping = false;
        s.last_params = None;
        s.reconnect_attempts = 0;
        s.reconnecting = false;
    }

    log("隧道已关闭");
    json!({ "ok": true })
}

// 读取一行（以 \n 结尾）
// 注意：MC 协议含二进制数据，超时返回 Err
async fn read_line_from_stream(
    reader: &mut OwnedReadHalf,
    timeout_duration: Duration,
) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];

    let result = tokio::time::timeout(timeout_duration, async {
        loop {
            let n = reader.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed".to_string());
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(idx) = buf.iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&buf[..idx]).trim().to_string();
                return Ok(line);
            }
        }
    })
    .await;

    match result {
        Ok(Ok(line)) => Ok(line),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("读取超时".to_string()),
    }
}

// ============== Tauri 命令 ==============

/// 拉取服务器节点列表
#[tauri::command]
pub async fn redstone_servers(_app: AppHandle) -> Value {
    let list = fetch_server_list().await;
    let mut s = state().lock().await;
    s.servers = list.clone();
    json!({ "ok": true, "servers": list })
}

/// 获取当前 API Key（不存在则生成）
#[tauri::command]
pub async fn redstone_apikey(_app: AppHandle) -> Value {
    let mut s = state().lock().await;
    if s.apikey.is_empty() {
        match load_or_create_apikey().await {
            Ok(k) => {
                s.apikey = k.clone();
                json!({ "ok": true, "apikey": k })
            }
            Err(e) => json!({ "ok": false, "error": e }),
        }
    } else {
        json!({ "ok": true, "apikey": s.apikey })
    }
}

/// 重置 API Key（生成新的并保存）
#[tauri::command]
pub async fn redstone_apikey_reset(_app: AppHandle) -> Value {
    let new_key = make_apikey();
    let dir = redstone_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return json!({ "ok": false, "error": e.to_string() });
    }
    if let Err(e) = std::fs::write(apikey_file(), &new_key) {
        return json!({ "ok": false, "error": e.to_string() });
    }
    let mut s = state().lock().await;
    s.apikey = new_key.clone();
    json!({ "ok": true, "apikey": new_key })
}

/// 扫描本机 java 进程监听的端口
#[tauri::command]
pub async fn redstone_scan_port() -> Value {
    use tokio::process::Command;

    // 1. 获取 java.exe PID 列表
    let output = match Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq java.exe", "/FO", "CSV", "/NH"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "error": e.to_string(), "port": null }),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut pids: Vec<String> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // CSV 格式: "java.exe","1234","Console","1","100,000 K"
        let parts: Vec<&str> = line.split("\",\"").collect();
        if parts.len() >= 2 {
            let pid_str = parts[1].trim_matches('"');
            if !pid_str.is_empty() && pid_str.chars().all(|c| c.is_ascii_digit()) {
                pids.push(pid_str.to_string());
            }
        }
    }

    if pids.is_empty() {
        return json!({ "ok": true, "port": null });
    }

    // 2. 获取 LISTENING 端口
    let mut netstat_cmd = Command::new("netstat");
    netstat_cmd.args(["-ano"]);
    #[cfg(target_os = "windows")]
    {
        netstat_cmd.creation_flags(0x08000000);
    }
    let output2 = match netstat_cmd.output().await {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "error": e.to_string(), "port": null }),
    };

    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    let mut candidates: Vec<u16> = Vec::new();
    for line in stdout2.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("LISTENING") {
            continue;
        }
        // 行末是 PID
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let pid = parts[parts.len() - 1];
        if !pids.contains(&pid.to_string()) {
            continue;
        }
        // 解析端口：0.0.0.0:25565 或 [::]:25565
        let local_addr = parts[1];
        if let Some(colon) = local_addr.rfind(':') {
            if let Ok(port) = local_addr[colon + 1..].parse::<u16>() {
                if !candidates.contains(&port) {
                    candidates.push(port);
                }
            }
        }
    }

    if candidates.is_empty() {
        return json!({ "ok": true, "port": null });
    }

    // 3. TCP 测试，找第一个能连的
    for port in &candidates {
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            TcpStream::connect(("127.0.0.1", *port)),
        )
        .await;
        if let Ok(Ok(_)) = result {
            return json!({ "ok": true, "port": port });
        }
    }

    json!({ "ok": true, "port": null })
}

/// 启动隧道
#[tauri::command]
pub async fn redstone_start(app: AppHandle, params: Value) -> Value {
    start_tunnel_inner(&app, params, false).await
}

/// 关闭隧道
#[tauri::command]
pub async fn redstone_stop(app: AppHandle) -> Value {
    stop_tunnel_inner(&app).await
}

/// 查询当前运行状态
#[tauri::command]
pub async fn redstone_status(_app: AppHandle) -> Value {
    let s = state().lock().await;
    json!({
        "ok": true,
        "running": s.running,
        "address": s.tunnel.as_ref().map(|t| t.address.clone()),
        "listenPort": s.tunnel.as_ref().map(|t| t.listen_port),
        "title": s.tunnel.as_ref().map(|t| t.title.clone()).unwrap_or_default(),
        "description": s.tunnel.as_ref().map(|t| t.description.clone()).unwrap_or_default(),
        "publicAccess": s.tunnel.as_ref().map(|t| t.public_access).unwrap_or(true),
        "allowOffline": s.tunnel.as_ref().map(|t| t.allow_offline).unwrap_or(false),
        "apikey": s.apikey,
        "servers": s.servers,
        "reconnecting": s.reconnecting,
        "reconnectAttempt": s.reconnect_attempts,
        "reconnectMaxAttempts": RECONNECT_MAX_ATTEMPTS,
    })
}

/// 拉取公开房间列表
#[tauri::command(rename_all = "camelCase")]
pub async fn redstone_public_tunnels(_app: AppHandle, server_address: String) -> Value {
    if server_address.is_empty() {
        let s = state().lock().await;
        let fallback = s.servers.first().and_then(|v| v.get("address")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        if fallback.is_empty() {
            return json!({ "ok": false, "error": "未指定服务器节点", "tunnels": [] });
        }
        drop(s);
        // 递归调用拿不到，重新组织
        let mut s = state().lock().await;
        let apikey = if s.apikey.is_empty() {
            match load_or_create_apikey().await {
                Ok(k) => {
                    s.apikey = k.clone();
                    k
                }
                Err(e) => return json!({ "ok": false, "error": e, "tunnels": [] }),
            }
        } else {
            s.apikey.clone()
        };
        drop(s);
        match list_public_tunnels(&fallback, 0, &apikey).await {
            Ok(data) => {
                let tunnels = data.get("tunnels").cloned().unwrap_or(Value::Array(vec![]));
                return json!({ "ok": true, "tunnels": tunnels, "serverAddress": fallback });
            }
            Err(e) => return json!({ "ok": false, "error": e, "tunnels": [] }),
        }
    }

    let mut s = state().lock().await;
    let apikey = if s.apikey.is_empty() {
        match load_or_create_apikey().await {
            Ok(k) => {
                s.apikey = k.clone();
                k
            }
            Err(e) => return json!({ "ok": false, "error": e, "tunnels": [] }),
        }
    } else {
        s.apikey.clone()
    };
    drop(s);

    match list_public_tunnels(&server_address, 0, &apikey).await {
        Ok(data) => {
            let tunnels = data.get("tunnels").cloned().unwrap_or(Value::Array(vec![]));
            json!({ "ok": true, "tunnels": tunnels, "serverAddress": server_address })
        }
        Err(e) => json!({ "ok": false, "error": e, "tunnels": [] }),
    }
}

/// 获取单个公开房间的模组列表
#[tauri::command(rename_all = "camelCase")]
pub async fn redstone_tunnel_mods(_app: AppHandle, server_address: String, tunnel_id: String) -> Value {
    if server_address.is_empty() {
        return json!({ "ok": false, "error": "未指定服务器节点", "mods": [] });
    }
    let tunnel_id_u64 = tunnel_id.parse::<u64>().unwrap_or(0);

    let mut s = state().lock().await;
    let apikey = if s.apikey.is_empty() {
        match load_or_create_apikey().await {
            Ok(k) => {
                s.apikey = k.clone();
                k
            }
            Err(e) => return json!({ "ok": false, "error": e, "mods": [] }),
        }
    } else {
        s.apikey.clone()
    };
    drop(s);

    match get_tunnel_mods(&server_address, tunnel_id_u64, &apikey).await {
        Ok(data) => {
            let mods = data.get("mods").cloned().unwrap_or(Value::Array(vec![]));
            json!({ "ok": true, "mods": mods, "serverAddress": server_address })
        }
        Err(e) => json!({ "ok": false, "error": e, "mods": [] }),
    }
}
