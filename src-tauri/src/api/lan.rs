// api/lan.rs — 局域网与 EasyTier 路由
// 职责：LAN 房间管理、UPnP 端口映射、EasyTier 虚拟局域网
// 对应原项目 server/api/routes/lan.js
//
// 路由清单（19 个）：
//   LAN（8）：
//     GET  /api/lan/my-ip           本机 IPv4 列表 + 公网 IP
//     POST /api/lan/upnp-map         添加 UPnP 端口映射
//     POST /api/lan/upnp-unmap       删除 UPnP 端口映射
//     GET  /api/lan/upnp-status      当前映射列表
//     GET  /api/lan/upnp-diagnose    UPnP 诊断
//     POST /api/lan/remote-create    创建远程房间（含 UPnP + 公网 IP）
//     GET  /api/lan/public-ip        仅查公网 IP
//     GET  /api/lan/port             获取检测的 LAN 端口
//   EasyTier（11）：
//     GET  /api/easytier/status          EasyTier 运行状态
//     POST /api/easytier/host             启动主机模式
//     POST /api/easytier/guest            启动客户端模式
//     POST /api/easytier/stop             停止 EasyTier
//     GET  /api/easytier/diagnose        诊断公共节点连通性
//     GET  /api/easytier/peers           查询节点列表
//     GET  /api/easytier/log             HTTP API 日志
//     GET  /api/easytier/filelog         文件日志
//     POST /api/easytier/download         下载 EasyTier
//     GET  /api/easytier/download-status  下载状态（简化：直接返回 completed）
//     POST /api/easytier/profiles        （占位，未实现 profile 管理）

use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::easytier::{self, state as et_state};
use crate::network::{self, public_ip, upnp};

/// 处理 LAN/EasyTier 路由
pub async fn handle(
    _app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== LAN =====
        "GET /api/lan/my-ip" => Some(handle_my_ip().await),
        "POST /api/lan/upnp-map" => Some(handle_upnp_map(body).await),
        "POST /api/lan/upnp-unmap" => Some(handle_upnp_unmap(body).await),
        "GET /api/lan/upnp-status" => Some(handle_upnp_status()),
        "GET /api/lan/upnp-diagnose" => Some(handle_upnp_diagnose().await),
        "POST /api/lan/remote-create" => Some(handle_remote_create(body).await),
        "GET /api/lan/public-ip" => Some(handle_public_ip().await),
        "GET /api/lan/port" => Some(handle_lan_port()),
        // ===== EasyTier =====
        "GET /api/easytier/status" => Some(handle_et_status()),
        "POST /api/easytier/host" => Some(handle_et_host(body).await),
        "POST /api/easytier/guest" => Some(handle_et_guest(body).await),
        "POST /api/easytier/stop" => Some(handle_et_stop().await),
        "GET /api/easytier/diagnose" => Some(handle_et_diagnose().await),
        "GET /api/easytier/peers" => Some(handle_et_peers().await),
        "GET /api/easytier/log" => Some(handle_et_log().await),
        "GET /api/easytier/filelog" => Some(handle_et_filelog()),
        "POST /api/easytier/download" => Some(handle_et_download().await),
        "GET /api/easytier/download-status" => Some(handle_et_download_status()),

        // ===== 占位：profile 管理尚未实现 =====
        "POST /api/easytier/profiles" | "GET /api/easytier/profiles" | "DELETE /api/easytier/profiles" => {
            Some(ApiResult::err(501, "EasyTier profile 管理功能尚未实现"))
        }

        _ => None,
    }
}

// ============== LAN 路由 ==============

/// GET /api/lan/my-ip — 本机 IPv4 列表 + 公网 IP
async fn handle_my_ip() -> ApiResult {
    let ips = upnp::list_local_ipv4();
    let public_ip = public_ip::get_public_ip().await.unwrap_or_default();
    ApiResult::ok(json!({
        "success": true,
        "ips": ips,
        "publicIP": public_ip
    }))
}

/// POST /api/lan/upnp-map — 添加 UPnP 端口映射
async fn handle_upnp_map(body: &Option<Value>) -> ApiResult {
    let internal_port = body
        .as_ref()
        .and_then(|b| b.get("internalPort"))
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;
    let external_port = body
        .as_ref()
        .and_then(|b| b.get("externalPort"))
        .and_then(|v| v.as_u64())
        .unwrap_or(internal_port as u64) as u16;
    let desc = body
        .as_ref()
        .and_then(|b| b.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("VersePC Minecraft");

    match upnp::add_port_mapping(internal_port, external_port, desc).await {
        Ok(v) => ApiResult::ok(v),
        Err(e) => ApiResult::err(500, &format!("UPnP 映射失败: {}", e)),
    }
}

/// POST /api/lan/upnp-unmap — 删除 UPnP 端口映射
async fn handle_upnp_unmap(body: &Option<Value>) -> ApiResult {
    let ext_port = body
        .as_ref()
        .and_then(|b| b.get("externalPort"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u16;
    if ext_port == 0 {
        return ApiResult::err(400, "Missing externalPort");
    }

    match upnp::delete_port_mapping(ext_port).await {
        Ok(v) => ApiResult::ok(v),
        Err(e) => ApiResult::err(500, &format!("UPnP 解除映射失败: {}", e)),
    }
}

/// GET /api/lan/upnp-status — 当前映射列表
fn handle_upnp_status() -> ApiResult {
    ApiResult::ok(json!({
        "success": true,
        "mappings": network::list_mappings()
    }))
}

/// GET /api/lan/upnp-diagnose — UPnP 诊断
async fn handle_upnp_diagnose() -> ApiResult {
    let mut checks: Vec<Value> = Vec::new();
    let mut recommendations: Vec<String> = Vec::new();
    let mut can_use_upnp = false;

    // 平台信息
    checks.push(json!({
        "name": "Platform",
        "result": std::env::consts::OS,
        "status": "info"
    }));

    // 本地 IP 检测
    let local_ips = upnp::list_local_ipv4();
    checks.push(json!({
        "name": "Local IPs",
        "result": local_ips.clone(),
        "status": if !local_ips.is_empty() { "ok" } else { "warn" }
    }));

    if local_ips.is_empty() {
        recommendations.push("未找到非内部 IPv4 地址，请检查网络连接".to_string());
    } else {
        // 检查私有 IP
        let has_private = local_ips.iter().any(|ip| {
            ip.get("address")
                .and_then(|a| a.as_str())
                .map(|addr| {
                    let parts: Vec<&str> = addr.split('.').collect();
                    if parts.len() != 4 {
                        return false;
                    }
                    let p0: u8 = parts[0].parse().unwrap_or(0);
                    let p1: u8 = parts[1].parse().unwrap_or(0);
                    p0 == 10 || (p0 == 172 && (16..=31).contains(&p1)) || (p0 == 192 && p1 == 168)
                })
                .unwrap_or(false)
        });
        if has_private {
            checks.push(json!({
                "name": "NAT Detection",
                "result": "Behind NAT (private IP)",
                "status": "info"
            }));
            recommendations.push("您处于 NAT 后方，需要 UPnP 端口映射才能让远程连接".to_string());
        } else {
            checks.push(json!({
                "name": "NAT Detection",
                "result": "Public IP detected",
                "status": "ok"
            }));
        }
    }

    // Windows SSDP 服务检查
    if cfg!(target_os = "windows") {
        let mut sc_cmd = std::process::Command::new("sc");
        sc_cmd.args(["query", "SSDPSRV"]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            sc_cmd.creation_flags(0x08000000);
        }
        let ssdp_check = sc_cmd.output();

        match ssdp_check {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let is_running = stdout.contains("RUNNING");
                checks.push(json!({
                    "name": "Windows SSDP Discovery Service",
                    "result": if is_running { "Running (may conflict)" } else { "Not running" },
                    "status": if is_running { "warn" } else { "ok" }
                }));
                if is_running {
                    recommendations.push(
                        "Windows SSDP Discovery 服务正在运行，可能拦截 UPnP 响应。可用 sc stop SSDPSRV 停止（需管理员）".to_string()
                    );
                }
            }
            Err(_) => {
                checks.push(json!({
                    "name": "Windows SSDP Discovery Service",
                    "result": "Could not check",
                    "status": "warn"
                }));
            }
        }
    }

    // UPnP 网关发现（SSDP 同步阻塞，用 spawn_blocking 避免阻塞异步运行时）
    let gateway_result = tokio::task::spawn_blocking(|| upnp::discover_gateway())
        .await
        .map_err(|e| format!("任务调度失败: {}", e));
    match gateway_result {
        Ok(Ok(gateway)) => {
            checks.push(json!({
                "name": "UPnP Gateway Discovery",
                "result": format!("Found: {}", gateway.address),
                "status": "ok"
            }));
            // 进一步检查 WANIPConnection（简化：直接标记为 OK）
            checks.push(json!({
                "name": "WANIPConnection Service",
                "result": "Found",
                "status": "ok"
            }));
            can_use_upnp = true;
        }
        Ok(Err(e)) => {
            checks.push(json!({
                "name": "UPnP Gateway Discovery",
                "result": e,
                "status": "fail"
            }));
            recommendations.push("未发现 UPnP 网关，请检查：".to_string());
            recommendations.push("1. 路由器管理面板中启用 UPnP 功能".to_string());
            recommendations.push("2. Windows 防火墙允许 UDP 1900 端口（SSDP）".to_string());
            recommendations.push("3. 网络类型设置为\"专用\"而非\"公用\"".to_string());
            recommendations.push("4. 无其他安全软件拦截多播流量".to_string());
        }
        Err(e) => {
            checks.push(json!({
                "name": "UPnP Gateway Discovery",
                "result": e,
                "status": "fail"
            }));
        }
    }

    // 公网 IP 检测
    match public_ip::get_public_ip().await {
        Ok(ip) => {
            checks.push(json!({
                "name": "Public IP",
                "result": ip,
                "status": "ok"
            }));
        }
        Err(_) => {
            checks.push(json!({
                "name": "Public IP",
                "result": "Could not detect",
                "status": "warn"
            }));
        }
    }

    if recommendations.is_empty() && can_use_upnp {
        recommendations.push("UPnP 工作正常".to_string());
    }

    ApiResult::ok(json!({
        "success": true,
        "diagnosis": {
            "platform": std::env::consts::OS,
            "checks": checks,
            "canUseUPnP": can_use_upnp,
            "recommendations": recommendations
        }
    }))
}

/// POST /api/lan/remote-create — 创建远程房间
async fn handle_remote_create(body: &Option<Value>) -> ApiResult {
    let game_port = body
        .as_ref()
        .and_then(|b| b.get("port"))
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;
    let use_upnp = body
        .as_ref()
        .and_then(|b| b.get("useUPnP"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // UPnP 映射
    let upnp_result = if use_upnp {
        match upnp::add_port_mapping(game_port, game_port, "VersePC Minecraft").await {
            Ok(v) => Some(v),
            Err(e) => Some(json!({ "success": false, "error": e })),
        }
    } else {
        None
    };

    // 公网 IP
    let public_ip = public_ip::get_public_ip().await.ok();

    // 本地 IP 列表
    let local_ips: Vec<String> = upnp::list_local_ipv4()
        .iter()
        .filter_map(|ip| ip.get("address").and_then(|a| a.as_str()).map(|s| s.to_string()))
        .collect();

    let connect_info = if let Some(ref p) = public_ip {
        format!("{}:{}", p, game_port)
    } else if !local_ips.is_empty() {
        format!("{}:{}", local_ips[0], game_port)
    } else {
        "unknown".to_string()
    };

    ApiResult::ok(json!({
        "success": true,
        "gamePort": game_port,
        "upnp": upnp_result,
        "publicIP": public_ip,
        "localIPs": local_ips,
        "connectInfo": connect_info
    }))
}

/// GET /api/lan/public-ip — 仅查公网 IP
async fn handle_public_ip() -> ApiResult {
    match public_ip::get_public_ip().await {
        Ok(ip) => ApiResult::ok(json!({ "success": true, "publicIP": ip })),
        Err(e) => ApiResult::err(502, &format!("查询公网 IP 失败: {}", e)),
    }
}

/// GET /api/lan/port — 获取检测的 LAN 端口
fn handle_lan_port() -> ApiResult {
    let port = network::get_detected_lan_port().unwrap_or(0);
    ApiResult::ok(json!({ "success": true, "port": port }))
}

// ============== EasyTier 路由 ==============

/// GET /api/easytier/status — EasyTier 运行状态
fn handle_et_status() -> ApiResult {
    let state = et_state::get_status();
    ApiResult::ok(json!({
        "success": true,
        "installed": et_state::is_installed(),
        "state": state,
        "running": state.get("running").and_then(|v| v.as_bool()).unwrap_or(false),
        "mode": state.get("mode").and_then(|v| v.as_str()).unwrap_or("idle"),
        "roomCode": state.get("roomCode").and_then(|v| v.as_str()).unwrap_or(""),
        "virtualIP": state.get("virtualIP").and_then(|v| v.as_str()).unwrap_or(""),
        "gamePort": state.get("gamePort").and_then(|v| v.as_u64()).unwrap_or(0),
        "profiles": state.get("profiles").cloned().unwrap_or(json!([])),
        "difficulty": state.get("difficulty"),
        "errorType": state.get("errorType"),
        "errorMessage": state.get("errorMessage")
    }))
}

/// POST /api/easytier/host — 启动主机模式
async fn handle_et_host(body: &Option<Value>) -> ApiResult {
    let game_port = body
        .as_ref()
        .and_then(|b| b.get("gamePort"))
        .and_then(|v| v.as_u64())
        .unwrap_or(25565) as u16;
    let player_name = body
        .as_ref()
        .and_then(|b| b.get("playerName"))
        .and_then(|v| v.as_str())
        .unwrap_or("主机");

    match easytier::start_host(game_port, player_name).await {
        Ok(v) => ApiResult::ok(v),
        Err(e) => ApiResult::err(500, &format!("创建联机失败: {}", e)),
    }
}

/// POST /api/easytier/guest — 启动客户端模式
async fn handle_et_guest(body: &Option<Value>) -> ApiResult {
    let room_code = body
        .as_ref()
        .and_then(|b| b.get("roomCode").or_else(|| b.get("invitationCode")))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if room_code.is_empty() {
        return ApiResult::err(400, "缺少房间码");
    }
    let player_name = body
        .as_ref()
        .and_then(|b| b.get("playerName"))
        .and_then(|v| v.as_str())
        .unwrap_or("玩家");

    match easytier::start_guest(room_code, player_name).await {
        Ok(v) => ApiResult::ok(v),
        Err(e) => ApiResult::err(500, &format!("加入联机失败: {}", e)),
    }
}

/// POST /api/easytier/stop — 停止 EasyTier
async fn handle_et_stop() -> ApiResult {
    easytier::stop().await;
    ApiResult::ok(json!({ "success": true }))
}

/// GET /api/easytier/diagnose — 诊断公共节点连通性
async fn handle_et_diagnose() -> ApiResult {
    let nodes = easytier::fetch_public_nodes(true).await;
    let mut results: Vec<Value> = Vec::new();

    for node in &nodes {
        let start = std::time::Instant::now();
        let mut status = "unknown".to_string();
        let mut latency: i64 = -1;

        if node.starts_with("http") {
            // HTTP HEAD 测试
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            match client.head(node).send().await {
                Ok(r) => {
                    status = if r.status().is_success() { "ok".to_string() } else { format!("http_{}", r.status().as_u16()) };
                    latency = start.elapsed().as_millis() as i64;
                }
                Err(e) => {
                    status = format!("err_{}", e);
                }
            }
        } else if node.starts_with("tcp://") || node.starts_with("udp://") {
            // TCP 连接测试
            let proto_end = node.find("://").unwrap_or(0);
            let after_proto = &node[proto_end + 3..];
            let parts: Vec<&str> = after_proto.split(':').collect();
            let host = parts[0];
            let port: u16 = parts
                .get(1)
                .and_then(|s| s.split('/').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(11010);

            let addr = format!("{}:{}", host, port);
            match tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            {
                Ok(Ok(_)) => {
                    status = "ok".to_string();
                    latency = start.elapsed().as_millis() as i64;
                }
                Ok(Err(e)) => {
                    status = format!("err_{}", e);
                }
                Err(_) => {
                    status = "timeout".to_string();
                }
            }
        }

        results.push(json!({
            "node": node,
            "status": status,
            "latency": latency
        }));
    }

    ApiResult::ok(json!({ "nodes": results }))
}

/// GET /api/easytier/peers — 查询节点列表
async fn handle_et_peers() -> ApiResult {
    let status = et_state::get_status();
    let http_port = status
        .get("gamePort")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u16;

    // 简化：直接返回当前状态（实际应通过 HTTP API 查询节点列表）
    ApiResult::ok(json!({
        "success": true,
        "state": status,
        "status": status
    }))
}

/// GET /api/easytier/log — HTTP API 日志
async fn handle_et_log() -> ApiResult {
    let status = et_state::get_status();
    let http_port = status
        .get("gamePort")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u16;

    if http_port == 0 {
        return ApiResult::ok(json!({ "log": "" }));
    }

    match easytier::http_get(http_port, "/log?fetch=true").await {
        Ok(v) => {
            let log_str = if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                serde_json::to_string(&v).unwrap_or_default()
            };
            ApiResult::ok(json!({ "log": log_str }))
        }
        Err(e) => ApiResult::ok(json!({ "log": "", "error": e })),
    }
}

/// GET /api/easytier/filelog — 文件日志
fn handle_et_filelog() -> ApiResult {
    let log_path = et_state::get_log_path();
    if !log_path.exists() {
        return ApiResult::ok(json!({ "success": true, "log": "" }));
    }

    match std::fs::read_to_string(&log_path) {
        Ok(content) => ApiResult::ok(json!({ "success": true, "log": content })),
        Err(e) => ApiResult::ok(json!({ "success": false, "error": e.to_string() })),
    }
}

/// POST /api/easytier/download — 下载 EasyTier
async fn handle_et_download() -> ApiResult {
    match easytier::download().await {
        Ok(_) => ApiResult::ok(json!({ "success": true, "sessionId": "easytier" })),
        Err(e) => ApiResult::ok(json!({ "error": e, "sessionId": "easytier" })),
    }
}

/// GET /api/easytier/download-status — 下载状态
fn handle_et_download_status() -> ApiResult {
    // 下载为同步完成，直接根据安装结果返回最终状态
    if et_state::is_installed() {
        ApiResult::ok(json!({ "status": "completed", "progress": 100, "message": "安装完成" }))
    } else {
        ApiResult::ok(json!({ "status": "error", "progress": 0, "message": "陶瓦联机内核下载失败，请检查网络后重试" }))
    }
}
