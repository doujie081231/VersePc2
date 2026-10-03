// redstone_online.rs — 红石联机 Tauri 命令模块
// 职责：拉取节点列表、下载并拉起外部内核 hongshic、
// 解析内核 stdout 的 endpoint 联机地址、按退出码处理隧道生命周期

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;
use std::sync::OnceLock;

// ============== 常量 ==============

const REGISTRY_URL: &str = "https://hongshi.site/api/server/list";
const DEFAULT_MAX_PLAYERS: u64 = 8;
// 等待内核输出 endpoint= 的超时
const STATUS_OPEN_TIMEOUT: Duration = Duration::from_secs(30);
// 内核输出轮询间隔
const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(200);

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

// ============== 平台 / 内核命名（对齐红石2.0接入文档） ==============

/// 下载接口的 platform 参数：windows / linux / macos
fn kernel_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "macos"
    }
}

/// 下载接口的 arch 参数：amd64 / arm64
fn kernel_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        "arm64"
    }
}

/// 内核文件名：{binary}-{platform}-{arch}，仅 windows 追加 .exe
fn kernel_file_name() -> String {
    let exe = if cfg!(target_os = "windows") { ".exe" } else { "" };
    format!("hongshic-{}-{}{}", kernel_platform(), kernel_arch(), exe)
}

// 定位内核 hongshic：优先 exe 同目录，其次用户数据目录 redstone-online/
fn find_kernel() -> Option<PathBuf> {
    let name = kernel_file_name();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&name));
        }
    }
    candidates.push(redstone_dir().join(name));
    candidates.into_iter().find(|p| p.exists())
}

/// 下载内核：GET /api/download/client?platform=&arch= 直接返回二进制文件
async fn download_kernel() -> Result<PathBuf, String> {
    let dest = redstone_dir().join(kernel_file_name());
    std::fs::create_dir_all(redstone_dir()).map_err(|e| format!("创建目录失败: {}", e))?;
    let tmp = dest.with_extension("downloading");
    let _ = std::fs::remove_file(&tmp);

    let url = format!(
        "https://hongshi.site/api/download/client?platform={}&arch={}",
        kernel_platform(),
        kernel_arch()
    );
    let dl_client = http_client(300);
    let resp = dl_client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载内核失败: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let reason = match status.as_u16() {
            400 => "未知平台或架构".to_string(),
            404 => {
                let body = resp.text().await.unwrap_or_default();
                if let Ok(obj) = serde_json::from_str::<Value>(&body) {
                    obj.get("expected_file")
                        .and_then(|v| v.as_str())
                        .map(|s| format!("该构建未发布（期望文件 {}）", s))
                        .unwrap_or_else(|| "该构建未发布".to_string())
                } else {
                    "该构建未发布".to_string()
                }
            }
            422 => "下载请求缺少必要参数".to_string(),
            429 => {
                let retry = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                if retry.is_empty() {
                    "下载过于频繁，请稍后再试".to_string()
                } else {
                    format!("下载过于频繁，请 {} 秒后再试", retry)
                }
            }
            c => format!("下载内核返回 HTTP {}", c),
        };
        return Err(format!("内核下载失败：{}", reason));
    }

    let bytes = resp
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

// ============== HTTP API 函数 ==============

// 拉取服务器节点列表（GET /api/server/list → {地区: 地址}），失败返回空列表
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
    Vec::new()
}

// ============== 隧道启动 / 关闭 ==============

/// 尝试在指定节点上启动一次隧道。
/// 按红石2.0接入文档：拉起 `hongshic -t <relay> [-p <game-port>]`，
/// 从 stdout 读取 `endpoint=` 字段取得联机地址。
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

    log(&format!("中转服务器: {}  本地端口: {}", server_address, game_port));

    // 启动内核：hongshic -t <relay> -p <game-port>
    let mut cmd = tokio::process::Command::new(kernel);
    cmd.arg("-t")
        .arg(server_address)
        .arg("-p")
        .arg(game_port.to_string());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
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

    // 捕获 stdout，逐行匹配 endpoint=；若进程先退出则按退出码判定失败
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法获取内核输出".to_string())?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();

    let deadline = Instant::now() + STATUS_OPEN_TIMEOUT;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(text)) => {
                        if let Some(pos) = text.find("endpoint=") {
                            let rest = text[pos + "endpoint=".len()..].trim();
                            let addr = rest.split_whitespace().next().unwrap_or("").trim().to_string();
                            if !addr.is_empty() {
                                let listen_port = addr
                                    .rsplit(':')
                                    .next()
                                    .and_then(|s| s.parse::<u16>().ok())
                                    .unwrap_or(game_port);
                                log(&format!("隧道已就绪，地址: {}", addr));
                                return Ok((addr, listen_port, child));
                            }
                        }
                    }
                    Ok(None) | Err(_) => {
                        // stdout 已关闭：进程必然退出，交由 child.wait() 分支处理
                    }
                }
            }
            status = child.wait() => {
                let code = status.ok().and_then(|st| st.code());
                let msg = match code {
                    Some(0) => "隧道已结束（房间已关闭或空闲回收）".to_string(),
                    Some(1) => "隧道创建失败（服务器不可达/拒绝请求）".to_string(),
                    c => format!("内核异常退出，退出码 {}", c.map(|x| x.to_string()).unwrap_or_default()),
                };
                return Err(msg);
            }
            _ = tokio::time::sleep(STATUS_POLL_INTERVAL) => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill().await;
            return Err("等待隧道开启超时".to_string());
        }
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
                                    Some(0) => "隧道已结束（房间已关闭或空闲回收）".to_string(),
                                    Some(1) => "隧道创建失败（服务器不可达/拒绝请求）".to_string(),
                                    c => format!("内核退出（退出码 {}）", c.map(|x| x.to_string()).unwrap_or_default()),
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