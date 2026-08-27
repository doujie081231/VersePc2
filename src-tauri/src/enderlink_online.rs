// enderlink_online.rs - EnderLink 联机模块
// 对接 lytapi.asia 联机大厅与 frp 内网穿透客户端(frpc)
// 流程: 拉大厅房间列表 -> 拉 frp 节点列表(解析+解密token) -> 选择节点与远程端口 ->
//       生成 frpc 配置并拉起 frpc.exe -> 公开房间时 POST api.php 创建并每30秒心跳

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

const HALL_URL: &str = "https://lytapi.asia/rooma.php";
const CREATE_URL: &str = "https://lytapi.asia/api.php";
const NODES_URL: &str = "https://lytapi.asia/frplist58.txt";
const FRP_DOWNLOAD_URL: &str =
    "https://gitee.com/lyt590/enderlinkupdate/releases/download/frp/frp.zip";
const FRP_EXE_NAME: &str = "frpc.exe";
const MIN_REMOTE_PORT: u16 = 10000;
const MAX_REMOTE_PORT: u16 = 60000;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

struct EnderlinkState {
    nodes: Vec<Value>,
    running: bool,
    stopping: bool,
    pid: Option<u32>,
    node: Option<Value>,
    remote_port: Option<u16>,
    local_port: u16,
    config_file: PathBuf,
    heartbeat: Option<tokio::task::JoinHandle<()>>,
}

impl EnderlinkState {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            running: false,
            stopping: false,
            pid: None,
            node: None,
            remote_port: None,
            local_port: 25565,
            config_file: PathBuf::new(),
            heartbeat: None,
        }
    }
}

static STATE: OnceLock<Mutex<EnderlinkState>> = OnceLock::new();

fn state() -> &'static Mutex<EnderlinkState> {
    STATE.get_or_init(|| Mutex::new(EnderlinkState::new()))
}

fn enderlink_dir() -> PathBuf {
    crate::storage::resolve_data_dir().join("enderlink")
}

fn client_dir() -> PathBuf {
    enderlink_dir().join("client")
}

fn client_exe() -> PathBuf {
    client_dir().join(FRP_EXE_NAME)
}

fn emit_log(app: &AppHandle, msg: &str) {
    let payload = json!({
        "message": msg,
        "ts": chrono::Utc::now().to_rfc3339()
    });
    let _ = app.emit("enderlink:log", payload);
}

fn http_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("VersePC-Tauri")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn decrypt_token(enc: &str) -> String {
    let mut out = String::new();
    for ch in enc.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_lowercase() {
            let idx = c as u8 - b'a';
            out.push_str(&(21 + idx as u32).to_string());
        }
    }
    out
}

fn parse_nodes(text: &str) -> Vec<Value> {
    let mut list = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let id_part = raw.split('#').next().unwrap_or("").trim();
        if id_part.is_empty() {
            continue;
        }
        let rest = raw.splitn(3, '#').nth(1).unwrap_or("").trim();
        let (b_start, b_end) = (rest.find('['), rest.rfind(']'));
        let info = match (b_start, b_end) {
            (Some(s), Some(e)) if e > s => &rest[s + 1..e],
            _ => "",
        };
        let parts: Vec<&str> = info.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let token = parts[parts.len() - 1];
        let hostport = parts[parts.len() - 2];
        let name = parts[..parts.len() - 2].join(" ");
        let (addr, port) = match hostport.rfind(':') {
            Some(idx) => (&hostport[..idx], &hostport[idx + 1..]),
            None => continue,
        };
        let frp_port = match port.parse::<u16>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        list.push(json!({
            "id": id_part,
            "name": name,
            "frpIp": addr,
            "frpPort": frp_port,
            "token": token,
            "decryptedToken": decrypt_token(token),
        }));
    }
    list
}

async fn download_client() -> Result<(), String> {
    let exe = client_exe();
    if exe.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(enderlink_dir()).map_err(|e| format!("创建目录失败: {}", e))?;
    std::fs::create_dir_all(client_dir()).map_err(|e| format!("创建客户端目录失败: {}", e))?;
    let zip_path = enderlink_dir().join("frp.zip");
    let _ = std::fs::remove_file(&zip_path);

    let client = http_client(180);
    let mut resp = client
        .get(FRP_DOWNLOAD_URL)
        .send()
        .await
        .map_err(|e| format!("下载 frp 客户端失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("下载 frp 客户端返回 HTTP {}", resp.status()));
    }

    let mut f = std::fs::File::create(&zip_path).map_err(|e| format!("创建文件失败: {}", e))?;
    while let Ok(Some(chunk)) = resp.chunk().await {
        f.write_all(&chunk).map_err(|e| format!("写入文件失败: {}", e))?;
    }
    drop(f);

    let mut cmd = std::process::Command::new("tar");
    cmd.arg("-xf").arg(&zip_path).arg("-C").arg(client_dir());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    let _ = std::fs::remove_file(&zip_path);
    if !ok {
        return Err("解压 frp 客户端失败".to_string());
    }
    if !exe.exists() {
        return Err("未在压缩包中找到 frpc.exe".to_string());
    }
    Ok(())
}

fn pick_remote_port() -> u16 {
    use std::net::{TcpListener, TcpStream};
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(3) as u64;
    let range = (MAX_REMOTE_PORT - MIN_REMOTE_PORT) as u64;
    for _ in 0..64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let port = MIN_REMOTE_PORT + (seed % range) as u16;
        if TcpListener::bind(("0.0.0.0", port)).is_err() {
            continue;
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            continue;
        }
        return port;
    }
    MIN_REMOTE_PORT + (std::process::id() as u64 % range) as u16
}

fn write_config(node: &Value, remote_port: u16, local_port: u16, token: &str) -> Result<PathBuf, String> {
    use std::fmt::Write as _;
    let frp_ip = node.get("frpIp").and_then(|v| v.as_str()).unwrap_or("");
    let frp_port = node.get("frpPort").and_then(|v| v.as_u64()).unwrap_or(7000);
    let cfg = enderlink_dir().join(format!("frpc-{}.toml", std::process::id()));
    std::fs::create_dir_all(enderlink_dir()).map_err(|e| format!("创建目录失败: {}", e))?;
    let mut s = String::new();
    let _ = writeln!(s, "serverAddr = \"{}\"", frp_ip);
    let _ = writeln!(s, "serverPort = {}", frp_port);
    if !token.is_empty() {
        let _ = writeln!(s, "auth.method = \"token\"");
        let _ = writeln!(s, "auth.token = \"{}\"", token);
    }
    let _ = writeln!(s, "");
    let _ = writeln!(s, "[[proxies]]");
    let _ = writeln!(s, "name = \"MC_C_{}\"", remote_port);
    let _ = writeln!(s, "type = \"tcp\"");
    let _ = writeln!(s, "localIP = \"127.0.0.1\"");
    let _ = writeln!(s, "localPort = {}", local_port);
    let _ = writeln!(s, "remotePort = {}", remote_port);
    std::fs::write(&cfg, s).map_err(|e| format!("写入配置失败: {}", e))?;
    Ok(cfg)
}

async fn create_public_room(app: &AppHandle, params: &Value, remote_port: u16, node: &Value) {
    let body = json!({
        "remote_port": remote_port,
        "node_id": node.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "room_name": params.get("roomName").and_then(|v| v.as_str()).unwrap_or(""),
        "player_count": 1,
        "game_version": params.get("gameVersion").and_then(|v| v.as_str()).unwrap_or("1.12.2"),
        "host_player": params.get("hostPlayer").and_then(|v| v.as_str()).unwrap_or("Steve"),
        "is_public": true,
        "server_addr": node.get("frpIp").and_then(|v| v.as_str()).unwrap_or(""),
    });
    let client = http_client(15);
    let resp = client.post(CREATE_URL).json(&body).send().await;
    match resp {
        Ok(r) if r.status().is_success() => {
            emit_log(app, "公开房间已创建，开始心跳维持");
        }
        Ok(r) => emit_log(app, &format!("创建公开房间返回 HTTP {}", r.status())),
        Err(e) => emit_log(app, &format!("创建公开房间失败(继续联机): {}", e)),
    }
    let body2 = body.clone();
    let handle = tokio::spawn(async move {
        let hb = http_client(15);
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            let _ = hb.post(CREATE_URL).json(&body2).send().await;
        }
    });
    let mut s = state().lock().await;
    s.heartbeat = Some(handle);
}

#[tauri::command]
pub async fn enderlink_rooms(_app: AppHandle) -> Value {
    let client = http_client(10);
    let resp = match client.get(HALL_URL).send().await {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "error": format!("拉取房间列表失败: {}", e), "rooms": [] }),
    };
    if !resp.status().is_success() {
        return json!({ "ok": false, "error": format!("房间列表返回 HTTP {}", resp.status()), "rooms": [] });
    }
    let rooms = match resp.json::<Value>().await {
        Ok(v) => v.as_array().cloned().unwrap_or_default(),
        Err(e) => return json!({ "ok": false, "error": format!("解析房间列表失败: {}", e), "rooms": [] }),
    };
    json!({ "ok": true, "rooms": rooms })
}

#[tauri::command]
pub async fn enderlink_nodes(_app: AppHandle) -> Value {
    let client = http_client(15);
    let resp = match client.get(NODES_URL).send().await {
        Ok(r) => r,
        Err(e) => return json!({ "ok": false, "error": format!("拉取节点列表失败: {}", e), "nodes": [] }),
    };
    if !resp.status().is_success() {
        return json!({ "ok": false, "error": format!("节点列表返回 HTTP {}", resp.status()), "nodes": [] });
    }
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => return json!({ "ok": false, "error": format!("读取节点列表失败: {}", e), "nodes": [] }),
    };
    let nodes = parse_nodes(&text);
    let mut s = state().lock().await;
    s.nodes = nodes.clone();
    json!({ "ok": true, "nodes": nodes })
}

#[tauri::command]
pub async fn enderlink_download(_app: AppHandle) -> Value {
    match download_client().await {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

#[tauri::command]
pub async fn enderlink_start(app: AppHandle, params: Value) -> Value {
    let log = |msg: &str| emit_log(&app, msg);
    {
        let s = state().lock().await;
        if s.running {
            return json!({ "ok": false, "error": "联机已在运行中，请先关闭" });
        }
    }

    if let Err(e) = download_client().await {
        log(&e);
        return json!({ "ok": false, "error": e });
    }

    let node = match params.get("node") {
        Some(n) if !n.is_null() => n.clone(),
        _ => {
            log("未指定节点");
            return json!({ "ok": false, "error": "未指定节点" });
        }
    };
    let frp_ip = node.get("frpIp").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if frp_ip.is_empty() {
        log("节点地址为空");
        return json!({ "ok": false, "error": "节点地址为空" });
    }
    let token = node
        .get("decryptedToken")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let local_port = params.get("localPort").and_then(|v| v.as_u64()).unwrap_or(25565) as u16;
    let remote_port = pick_remote_port();

    let cfg = match write_config(&node, remote_port, local_port, &token) {
        Ok(c) => c,
        Err(e) => {
            log(&e);
            return json!({ "ok": false, "error": e });
        }
    };

    let mut cmd = tokio::process::Command::new(client_exe());
    cmd.arg("-c")
        .arg(&cfg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let err = format!("启动 frpc 失败: {}", e);
            log(&err);
            return json!({ "ok": false, "error": err });
        }
    };
    let pid = child.id();

    let is_public = params.get("isPublic").and_then(|v| v.as_bool()).unwrap_or(false);
    if is_public {
        create_public_room(&app, &params, remote_port, &node).await;
    }

    {
        let mut s = state().lock().await;
        s.running = true;
        s.stopping = false;
        s.pid = pid;
        s.node = Some(node.clone());
        s.remote_port = Some(remote_port);
        s.local_port = local_port;
        s.config_file = cfg.clone();
    }
    log(&format!("frpc 已启动 (PID {})", pid.map(|p| p.to_string()).unwrap_or_else(|| "?".to_string())));
    log(&format!("隧道: {}:{} -> 本机 {}", frp_ip, remote_port, local_port));

    let app2 = app.clone();
    tokio::spawn(async move {
        let exit = child.wait().await;
        let code = exit.ok().and_then(|st| st.code());
        let stopping = {
            let mut s = state().lock().await;
            s.running = false;
            s.node = None;
            s.remote_port = None;
            s.pid = None;
            s.stopping
        };
        let reason = if stopping {
            "stopped".to_string()
        } else {
            format!("frpc 退出 (code {})", code.map(|c| c.to_string()).unwrap_or_default())
        };
        let _ = app2.emit("enderlink:disconnected", json!({ "reason": reason }));
    });

    json!({ "ok": true, "address": format!("{}:{}", frp_ip, remote_port), "remotePort": remote_port })
}

#[tauri::command]
pub async fn enderlink_stop(app: AppHandle) -> Value {
    let log = |msg: &str| emit_log(&app, msg);
    let (pid, heartbeat) = {
        let mut s = state().lock().await;
        if s.stopping {
            return json!({ "ok": true });
        }
        s.stopping = true;
        (s.pid.take(), s.heartbeat.take())
    };
    if let Some(h) = heartbeat {
        h.abort();
    }
    log("正在关闭联机...");
    if let Some(pid) = pid {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    {
        let mut s = state().lock().await;
        s.running = false;
        s.node = None;
        s.remote_port = None;
        s.pid = None;
        s.stopping = false;
    }
    log("联机已关闭");
    json!({ "ok": true })
}

#[tauri::command]
pub async fn enderlink_status(_app: AppHandle) -> Value {
    let s = state().lock().await;
    json!({
        "ok": true,
        "running": s.running,
        "address": s.node.as_ref().and_then(|n| n.get("frpIp").and_then(|v| v.as_str())).map(|ip| {
            let port = s.remote_port.map(|p| p.to_string()).unwrap_or_default();
            format!("{}:{}", ip, port)
        }),
        "remotePort": s.remote_port,
        "localPort": s.local_port,
        "nodes": s.nodes,
    })
}