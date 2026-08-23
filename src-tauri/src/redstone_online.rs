// redstone_online.rs — 红石联机 Tauri 命令模块
// 职责：节点列表、拉起外部内核 hongshi.exe、读取 tunnel.ini 状态、处理退出码
// 依据新版《RedStone 内核接入文档》：外壳只负责选定中转服务器并启动内核，
// 通过 启动参数 + 状态文件 + 退出码 对接，不解析内核协议/日志。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use std::sync::OnceLock;

// ============== 常量 ==============

const REGISTRY_URL: &str = "https://hongshi.site/newserver.json";
const DEFAULT_KERNEL_NAME: &str = "hongshi.exe";
const DEFAULT_MAX_PLAYERS: u64 = 8;
// 等待内核写入 status=open 的超时
const STATUS_OPEN_TIMEOUT: Duration = Duration::from_secs(30);
// 状态文件轮询间隔
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(400);

// ============== 运行时状态 ==============

#[derive(Clone)]
struct TunnelInfo {
    server_address: String,
    listen_port: u16,
    address: String,
    max_players: u32,
}

struct RedstoneState {
    servers: Vec<Value>,
    current_server_idx: usize,
    running: bool,
    stopping: bool,
    pid: Option<u32>,
    status_file: PathBuf,
    tunnel: Option<TunnelInfo>,
    auto_reconnect: bool,
    reconnect_nodes: Vec<Value>,
}

impl RedstoneState {
    fn new() -> Self {
        Self {
            servers: Vec::new(),
            current_server_idx: 0,
            running: false,
            stopping: false,
            pid: None,
            status_file: redstone_dir().join("tunnel.ini"),
            tunnel: None,
            auto_reconnect: false,
            reconnect_nodes: Vec::new(),
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

fn emit_log(app: &AppHandle, msg: &str) {
    let payload = json!({
        "message": msg,
        "ts": chrono::Utc::now().to_rfc3339()
    });
    let _ = app.emit("redstone:log", payload);
}

fn http_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("VersePC-Tauri")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// 定位内核 hongshi.exe：优先 exe 同目录，其次用户数据目录 redstone-online/
fn find_kernel() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(DEFAULT_KERNEL_NAME));
        }
    }
    candidates.push(redstone_dir().join(DEFAULT_KERNEL_NAME));
    candidates.into_iter().find(|p| p.exists())
}

async fn download_kernel() -> Result<PathBuf, String> {
    let api_client = http_client(15);
    let resp = api_client
        .get("https://hongshi.site/api/download/windows")
        .send()
        .await
        .map_err(|e| format!("请求内核下载地址失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("内核下载接口返回 HTTP {}", resp.status()));
    }
    let obj: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析内核下载地址失败: {}", e))?;
    let url = obj
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "内核下载地址为空".to_string())?
        .to_string();

    let dest = redstone_dir().join(DEFAULT_KERNEL_NAME);
    std::fs::create_dir_all(redstone_dir()).map_err(|e| format!("创建目录失败: {}", e))?;
    let tmp = dest.with_extension("exe.downloading");
    let _ = std::fs::remove_file(&tmp);

    let dl_client = http_client(300);
    let body = dl_client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载内核失败: {}", e))?;
    let status = body.status();
    if !status.is_success() {
        return Err(format!("下载内核返回 HTTP {}", status));
    }
    let bytes = body
        .bytes()
        .await
        .map_err(|e| format!("读取内核数据失败: {}", e))?;
    if bytes.len() < 10000 {
        return Err(format!("内核文件异常过小 ({} bytes)", bytes.len()));
    }
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入内核文件失败: {}", e))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("保存内核失败: {}", e))?;
    Ok(dest)
}

// 读取 tunnel.ini，若 status=open 返回 (server, port)
fn read_open_tunnel(status_file: &PathBuf) -> Option<(String, u16)> {
    let content = std::fs::read_to_string(status_file).ok()?;
    let mut section = String::new();
    let mut status = String::new();
    let mut server = String::new();
    let mut port = String::new();
    for raw in content.lines() {
        let line = raw.split(';').next().unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).trim().to_string();
            continue;
        }
        if section != "tunnel" {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_lowercase();
            let val = line[eq + 1..].trim().to_string();
            match key.as_str() {
                "status" => status = val,
                "server" => server = val,
                "port" => port = val,
                _ => {}
            }
        }
    }
    if status.eq_ignore_ascii_case("open") {
        let p = port.trim().parse::<u16>().ok()?;
        let s = if server.is_empty() { return None } else { server };
        return Some((s, p));
    }
    None
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
    vec![json!({ "name": "南京", "address": "nanjing.hongshi.site" })]
}

// ============== 隧道启动 / 关闭 ==============

/// 尝试在指定节点上启动一次隧道。
/// 成功返回 (address, listen_port, child)，失败返回 Err。
async fn try_start_node(
    app: &AppHandle,
    kernel: &Path,
    server_address: &str,
    game_port: u16,
    max_players: u32,
) -> Result<(String, u16, tokio::process::Child), String> {
    let log = |msg: &str| {
        emit_log(app, msg);
    };

    // 准备状态文件（清掉旧数据，避免读到上一次的隧道）
    let status_file = redstone_dir().join("tunnel.ini");
    let _ = std::fs::create_dir_all(redstone_dir());
    let _ = std::fs::remove_file(&status_file);

    log(&format!("中转服务器: {}  本地端口: {}", server_address, game_port));

    // 启动内核
    let mut cmd = tokio::process::Command::new(kernel);
    cmd.arg("-server")
        .arg(server_address)
        .arg("-port")
        .arg(game_port.to_string())
        .arg("-status-file")
        .arg(&status_file);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("启动内核失败: {}", e));
        }
    };
    let pid = child.id();
    let status_file_lookup = status_file.clone();

    // 轮询状态文件，等待 status=open
    let deadline = Instant::now() + STATUS_OPEN_TIMEOUT;
    loop {
        // 内核提前退出 → 启动失败，换下一个节点
        if let Ok(Some(status)) = child.try_wait() {
            let code = status.code().unwrap_or(-1);
            let msg = match code {
                0 => "内核已退出：隧道被回收或服务器关闭".to_string(),
                1 => "隧道创建失败（服务器不可达/拒绝/无空闲端口）".to_string(),
                2 => "参数错误".to_string(),
                c => format!("内核异常退出，退出码 {}", c),
            };
            return Err(msg);
        }
        if let Some(info) = read_open_tunnel(&status_file_lookup) {
            let (tunnel_server, listen_port) = info;
            let address = format!("{}:{}", tunnel_server, listen_port);
            log(&format!("隧道已就绪，地址: {}", address));
            let _ = pid;
            return Ok((address, listen_port, child));
        }
        if Instant::now() >= deadline {
            let _ = child.kill().await;
            return Err("等待隧道开启超时".to_string());
        }
        tokio::time::sleep(STATUS_POLL_INTERVAL).await;
    }
}

async fn start_tunnel_inner(app: &AppHandle, params: Value) -> Value {
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

    let selected_address = params
        .get("serverAddress")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let game_port = params
        .get("gamePort")
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;
    let max_players = params
        .get("maxPlayers")
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| params.get("maxPlayers").and_then(|v| v.as_u64()))
        .unwrap_or(DEFAULT_MAX_PLAYERS) as u32;
    // 最大自动切换重试次数（避免无限循环，每个节点至多再检查一轮）
    let max_attempts = params
        .get("maxAttempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .min(8) as u32;

    if game_port == 0 {
        return json!({ "ok": false, "error": "游戏端口无效" });
    }

    // 组装候选节点（按序）：选中的节点在前，其余节点去重后追加
    let mut candidates: Vec<String> = Vec::new();
    if !selected_address.is_empty() {
        candidates.push(selected_address.clone());
    }
    {
        let s = state().lock().await;
        for node in &s.servers {
            let addr = node.get("address").and_then(|v| v.as_str()).unwrap_or("");
            if !addr.is_empty() && (candidates.is_empty() || candidates[0] != addr) && !candidates.iter().any(|c| c == addr) {
                candidates.push(addr.to_string());
            }
        }
    }
    if candidates.is_empty() {
        return json!({ "ok": false, "error": "未指定服务器节点" });
    }
    log(&format!("候选节点: {}", candidates.join(", ")));

    // 定位内核
    let kernel = match find_kernel() {
        Some(k) => k,
        None => {
            log("未找到内核，正在下载内核 ...");
            match download_kernel().await {
                Ok(path) => path,
                Err(e) => {
                    log(&e);
                    return json!({ "ok": false, "error": e });
                }
            }
        }
    };
    log(&format!("正在启动内核 {} ...", kernel.display()));

    // 按序尝试候选节点，成功即停
    let mut last_err = String::new();
    let mut tried: Vec<String> = Vec::new();
    for addr in &candidates {
        if tried.contains(addr) {
            continue;
        }
        tried.push(addr.clone());
        log(&format!("尝试节点: {}", addr));
        match try_start_node(app, &kernel, addr, game_port, max_players).await {
            Ok((address, listen_port, child)) => {
                log(&format!("节点连接成功: {}", addr));
                let tunnel_info = TunnelInfo {
                    server_address: addr.clone(),
                    listen_port,
                    address: address.clone(),
                    max_players,
                };
                {
                    let mut s = state().lock().await;
                    s.tunnel = Some(tunnel_info.clone());
                    s.running = true;
                    s.stopping = false;
                    s.pid = child.id();
                    // 记录剩余可切换的候选节点（当前节点已用，排除）
                    s.reconnect_nodes = candidates
                        .iter()
                        .filter(|a| *a != addr)
                        .cloned()
                        .map(|a| json!({ "address": a, "name": a }))
                        .collect();
                    s.auto_reconnect = true;
                }

                // 监听内核退出：非用户停止时自动切换下一个节点重连
                let app2 = app.clone();
                let kernel2 = kernel.clone();
                let current_addr = addr.clone();
                let game_port2 = game_port;
                let max_players2 = max_players;
                let max_attempts2 = max_attempts;
                tokio::spawn(async move {
                    let mut child = child;
                    write_debug(&format!("[hongshi] 内核已启动 节点={}", current_addr));
                    let mut attempts_done: u32 = 0;
                    loop {
                        let exit = child.wait().await;
                        let code = exit.ok().and_then(|st| st.code());
                        let should_reconnect = {
                            let mut s = state().lock().await;
                            let auto = s.auto_reconnect;
                            let stopping = s.stopping;
                            if !stopping {
                                s.tunnel = None;
                                s.running = false;
                                s.pid = None;
                            }
                            auto && !stopping
                        };
                        if !should_reconnect || attempts_done >= max_attempts2 {
                            let reason = if should_reconnect {
                                "reconnect max attempts reached".to_string()
                            } else {
                                match code {
                                    Some(0) => "tunnel closed (exit 0)".to_string(),
                                    Some(1) => "tunnel create failed (exit 1)".to_string(),
                                    Some(2) => "parameter error (exit 2)".to_string(),
                                    c => format!("kernel exited (code {})", c.map(|x| x.to_string()).unwrap_or_default()),
                                }
                            };
                            {
                                let mut s = state().lock().await;
                                s.auto_reconnect = false;
                            }
                            write_debug(&format!("[hongshi] 内核退出: {}", reason));
                            let _ = app2.emit("redstone:disconnected", json!({ "reason": reason }));
                            return;
                        }

                        // 自动切换到下一个候选节点
                        attempts_done += 1;
                        let next_addr = {
                            let mut s = state().lock().await;
                            if let Some(node) = s.reconnect_nodes.first().cloned() {
                                let addr = node.get("address").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                s.reconnect_nodes.remove(0);
                                Some(addr)
                            } else {
                                None
                            }
                        };
                        let reason = match code {
                            Some(0) => "tunnel closed (exit 0)".to_string(),
                            Some(1) => "tunnel create failed (exit 1)".to_string(),
                            Some(2) => "parameter error (exit 2)".to_string(),
                            c => format!("kernel exited (code {})", c.map(|x| x.to_string()).unwrap_or_default()),
                        };
                        let _ = app2.emit("redstone:reconnecting", json!({ "reason": reason, "attempt": attempts_done, "maxAttempts": max_attempts2 }));
                        let Some(next_addr) = next_addr else {
                            write_debug(&format!("[hongshi] 无更多候选节点，重连停止"));
                            let _ = app2.emit("redstone:disconnected", json!({ "reason": "no more nodes" }));
                            return;
                        };
                        write_debug(&format!("[hongshi] 自动切换节点: {}", next_addr));
                        match try_start_node(&app2, &kernel2, &next_addr, game_port2, max_players2).await {
                            Ok((new_address, new_listen_port, new_child)) => {
                                write_debug(&format!("[hongshi] 切换到节点 {} 成功: {}", next_addr, new_address));
                                {
                                    let mut s = state().lock().await;
                                    s.tunnel = Some(TunnelInfo {
                                        server_address: next_addr.clone(),
                                        listen_port: new_listen_port,
                                        address: new_address.clone(),
                                        max_players: max_players2,
                                    });
                                    s.pid = new_child.id();
                                }
                                let _ = app2.emit("redstone:reconnected", json!({
                                    "address": new_address,
                                    "listenPort": new_listen_port,
                                    "serverAddress": next_addr
                                }));
                                child = new_child;
                            }
                            Err(e) => {
                                write_debug(&format!("[hongshi] 节点 {} 连接失败: {}", next_addr, e));
                            }
                        }
                    }
                });
                return json!({ "ok": true, "address": address.clone(), "listenPort": listen_port });
            }
            Err(e) => {
                log(&format!("节点失败: {} ({})", addr, e));
                last_err = e;
                // 短暂间隔再试下一个，避免过于频繁
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
        }
    }

    json!({ "ok": false, "error": format!("所有节点连接失败：{}", last_err) })
}

async fn stop_tunnel_inner(app: &AppHandle) -> Value {
    let log = |msg: &str| {
        emit_log(app, msg);
    };

    let pid = {
        let mut s = state().lock().await;
        if s.stopping {
            return json!({ "ok": true });
        }
        s.stopping = true;
        s.pid.take()
    };

    log("正在关闭隧道...");

    if let Some(pid) = pid {
        // 结束内核进程（含子进程）
        let _ = kill_pid(pid);
    }

    // 清理状态
    {
        let mut s = state().lock().await;
        s.tunnel = None;
        s.pid = None;
        s.running = false;
        s.stopping = false;
        s.auto_reconnect = false;
        s.reconnect_nodes = Vec::new();
    }

    log("隧道已关闭");
    json!({ "ok": true })
}

// 按 PID 结束进程（Windows taskkill /T 含子进程）
fn kill_pid(pid: u32) -> std::io::Result<()> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F", "/T"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    status.map(|_| ())
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
            tokio::net::TcpStream::connect(("127.0.0.1", *port)),
        )
        .await;
        if let Ok(Ok(_)) = result {
            return json!({ "ok": true, "port": port });
        }
    }

    json!({ "ok": true, "port": null })
}

/// 启动隧道（拉起 hongshi.exe 内核）
#[tauri::command]
pub async fn redstone_start(app: AppHandle, params: Value) -> Value {
    start_tunnel_inner(&app, params).await
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
        "maxPlayers": s.tunnel.as_ref().map(|t| t.max_players),
        "servers": s.servers,
        "reconnecting": false,
        "reconnectAttempt": 0,
        "reconnectMaxAttempts": 0,
    })
}