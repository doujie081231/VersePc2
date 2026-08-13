// easytier/process.rs — EasyTier 子进程管理
// 职责：启动/停止 EasyTier 进程、主机/客户端模式切换、HTTP API 调用
// 对应原项目 server/terracotta/terracotta-process.js
//
// 启动流程：
//   1. 检查 easytier.exe 是否存在
//   2. 生成随机房间码 + HTTP API 端口
//   3. tokio::process::Command 启动子进程
//   4. 后台监控进程状态，更新 EasyTierStatus
//   5. 通过 HTTP API 查询节点列表、日志
//
// 主机模式命令：
//   easytier-core --network <room_code> --listener tcp://0.0.0.0:<game_port>
//                  --rpc-portal 127.0.0.1:<http_port>
//
// 客户端模式命令：
//   easytier-core --network <room_code> --peers tcp://<host>:<port>
//                  --rpc-portal 127.0.0.1:<http_port>

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::Child;

use super::state::{self, EasyTierMode};

/// 全局子进程句柄
static CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// HTTP API 端口起始值（动态分配）
static NEXT_HTTP_PORT: AtomicU32 = AtomicU32::new(11010);

/// 公共节点列表（用于客户端连接）
const PUBLIC_NODES: &[&str] = &[
    "tcp://pub1.easytier.cn:11010",
    "tcp://pub2.easytier.cn:11010",
];

/// 启动主机模式
///
/// 参数：
///   - game_port: Minecraft 游戏端口（默认 25565）
///   - player_name: 主机玩家名（仅用于日志）
///
/// 返回 { success, roomCode, virtualIP, gamePort }
pub async fn start_host(game_port: u16, player_name: &str) -> Result<Value, String> {
    // 未安装时自动下载内核，避免首次使用时卡住
    if !state::is_installed() {
        download().await.map_err(|e| format!("陶瓦联机内核下载失败: {}", e))?;
    }

    // 停止已有进程
    stop().await;

    let room_code = generate_room_code();
    let http_port = NEXT_HTTP_PORT.fetch_add(1, Ordering::SeqCst) as u16;
    let bin_path = state::get_binary_path();

    // 启动子进程
    let mut easytier_cmd = tokio::process::Command::new(&bin_path);
    easytier_cmd
        .arg("--network")
        .arg(&room_code)
        .arg("--listener")
        .arg(format!("tcp://0.0.0.0:{}", game_port))
        .arg("--rpc-portal")
        .arg(format!("127.0.0.1:{}", http_port))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        easytier_cmd.creation_flags(0x08000000);
    }
    let child = easytier_cmd
        .spawn()
        .map_err(|e| format!("启动 EasyTier 失败: {}", e))?;

    // 保存子进程句柄
    {
        let mut g = CHILD.lock().unwrap();
        *g = Some(child);
    }

    // 更新状态
    state::update_status(|s| {
        s.running = true;
        s.mode = EasyTierMode::Host;
        s.room_code = room_code.clone();
        s.game_port = game_port;
        s.http_port = http_port;
        s.error_type = None;
        s.error_message = None;
    });

    eprintln!(
        "[easytier] 主机模式启动: room={}, port={}, http={}",
        room_code, game_port, http_port
    );

    // 后台等待 2 秒让 EasyTier 初始化，然后查询虚拟 IP
    let room_clone = room_code.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Ok(virtual_ip) = fetch_virtual_ip(http_port).await {
            state::update_status(|s| {
                if s.running {
                    s.virtual_ip = virtual_ip;
                }
            });
        }
    });

    Ok(json!({
        "success": true,
        "roomCode": room_clone,
        "gamePort": game_port,
        "httpPort": http_port
    }))
}

/// 启动客户端模式
///
/// 参数：
///   - room_code: 主机房间码（或邀请链接）
///   - player_name: 玩家名（仅用于日志）
pub async fn start_guest(room_code: &str, player_name: &str) -> Result<Value, String> {
    // 未安装时自动下载内核，避免首次使用时卡住
    if !state::is_installed() {
        download().await.map_err(|e| format!("陶瓦联机内核下载失败: {}", e))?;
    }

    // 停止已有进程
    stop().await;

    let http_port = NEXT_HTTP_PORT.fetch_add(1, Ordering::SeqCst) as u16;
    let bin_path = state::get_binary_path();

    // 解析邀请码：可能是 room_code 或 tcp://host:port
    let peer_url = if room_code.starts_with("tcp://") || room_code.starts_with("udp://") {
        room_code.to_string()
    } else {
        // 房间码模式：尝试连接公共节点
        let mut peers = String::new();
        for node in PUBLIC_NODES {
            if !peers.is_empty() {
                peers.push(',');
            }
            peers.push_str(node);
        }
        format!("tcp://{}", room_code)
    };

    let mut easytier_cmd = tokio::process::Command::new(&bin_path);
    easytier_cmd
        .arg("--network")
        .arg(room_code)
        .arg("--peers")
        .arg(&peer_url)
        .arg("--rpc-portal")
        .arg(format!("127.0.0.1:{}", http_port))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        easytier_cmd.creation_flags(0x08000000);
    }
    let child = easytier_cmd
        .spawn()
        .map_err(|e| format!("启动 EasyTier 失败: {}", e))?;

    {
        let mut g = CHILD.lock().unwrap();
        *g = Some(child);
    }

    state::update_status(|s| {
        s.running = true;
        s.mode = EasyTierMode::Guest;
        s.room_code = room_code.to_string();
        s.game_port = 0;
        s.http_port = http_port;
        s.error_type = None;
        s.error_message = None;
    });

    eprintln!(
        "[easytier] 客户端模式启动: room={}, http={}",
        room_code, http_port
    );

    // 后台查询虚拟 IP
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Ok(virtual_ip) = fetch_virtual_ip(http_port).await {
            state::update_status(|s| {
                if s.running {
                    s.virtual_ip = virtual_ip;
                }
            });
        }
    });

    Ok(json!({
        "success": true,
        "roomCode": room_code,
        "httpPort": http_port
    }))
}

/// 停止 EasyTier
pub async fn stop() {
    let child_opt = {
        let mut g = CHILD.lock().unwrap();
        g.take()
    };

    if let Some(mut child) = child_opt {
        // 尝试优雅关闭
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    state::reset_to_idle();
    eprintln!("[easytier] 已停止");
}

/// 通过 HTTP API 查询虚拟 IP
/// GET http://127.0.0.1:<port>/ip
async fn fetch_virtual_ip(http_port: u16) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{}/ip", http_port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("查询 IP 失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let text = resp.text().await.unwrap_or_default();
    let trimmed = text.trim().trim_matches('"');
    if !trimmed.is_empty() {
        Ok(trimmed.to_string())
    } else {
        Err("虚拟 IP 为空".to_string())
    }
}

/// 通过 HTTP API 调用 EasyTier
/// GET http://127.0.0.1:<port><path>
pub async fn http_get(http_port: u16, path: &str) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{}{}", http_port, path);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("HTTP 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    // 尝试解析 JSON，失败则返回文本
    let text = resp.text().await.unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        Ok(v)
    } else {
        Ok(json!({ "text": text }))
    }
}

/// 拉取公共节点列表
/// 返回 Vec<String>，每个元素是节点 URL
pub async fn fetch_public_nodes(_force_refresh: bool) -> Vec<String> {
    // 简化：直接返回硬编码的公共节点列表
    // 原项目从 https://easytier.cn/free-nodes 拉取，这里用固定列表
    PUBLIC_NODES.iter().map(|s| s.to_string()).collect()
}

/// 生成 6 位房间码（大写字母+数字）
fn generate_room_code() -> String {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let chars = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut result = String::new();
    let mut x = seed;
    for _ in 0..6 {
        let idx = (x % chars.len() as u128) as usize;
        result.push(chars.chars().nth(idx).unwrap_or('X'));
        x /= chars.len() as u128;
        if x == 0 {
            x = seed.wrapping_add(1);
        }
    }
    result
}

/// 内核下载候选地址（多镜像，按顺序尝试，命中即用）
/// 优先国内可访问的加速镜像，最后才是 GitHub 官方源
const EASYTIER_DOWNLOAD_URLS: &[&str] = &[
    "https://ghfast.top/https://github.com/EasyTier/EasyTier/releases/latest/download/easytier-windows-x86_64.zip",
    "https://mirror.ghproxy.com/https://github.com/EasyTier/EasyTier/releases/latest/download/easytier-windows-x86_64.zip",
    "https://github.com/EasyTier/EasyTier/releases/latest/download/easytier-windows-x86_64.zip",
];

/// 下载 EasyTier 到 dataDir/easytier/
/// 解压并重命名为 easytier.exe
pub async fn download() -> Result<(), String> {
    let data_dir = crate::storage::resolve_data_dir();
    let et_dir = data_dir.join("easytier");
    std::fs::create_dir_all(&et_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let et_bin = et_dir.join("easytier.exe");
    if et_bin.exists() {
        return Ok(());
    }

    let et_zip = et_dir.join("easytier.zip");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    // 依次尝试多个镜像，避免单个源（尤其 GitHub）无法访问导致失败
    let mut bytes: Option<bytes::Bytes> = None;
    let mut last_err = String::new();
    for et_url in EASYTIER_DOWNLOAD_URLS {
        match client.get(*et_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.bytes().await {
                        Ok(b) if b.len() >= 10000 => {
                            bytes = Some(b);
                            break;
                        }
                        Ok(b) => {
                            last_err = format!("文件过小 ({} bytes): {}", b.len(), et_url);
                            continue;
                        }
                        Err(e) => {
                            last_err = format!("读取响应失败: {} ({})", e, et_url);
                            continue;
                        }
                    }
                } else {
                    last_err = format!("下载 HTTP {} ({})", resp.status(), et_url);
                    continue;
                }
            }
            Err(e) => {
                last_err = format!("下载失败: {} ({})", e, et_url);
                continue;
            }
        }
    }

    let bytes = bytes.ok_or_else(|| format!("所有下载源均失败: {}", last_err))?;

    std::fs::write(&et_zip, &bytes).map_err(|e| format!("写入文件失败: {}", e))?;

    // 解压
    let zip_file = std::fs::File::open(&et_zip).map_err(|e| format!("打开 ZIP 失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("解析 ZIP 失败: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取 ZIP 条目失败: {}", e))?;
        let name = file.name().to_string();
        if name.ends_with(".exe") {
            let out_path = et_dir.join(&name);
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| format!("创建文件失败: {}", e))?;
            std::io::copy(&mut file, &mut out_file)
                .map_err(|e| format!("解压失败: {}", e))?;
        }
    }

    // 删除 ZIP
    let _ = std::fs::remove_file(&et_zip);

    // 找到解压出的 exe，重命名为 easytier.exe
    let mut found_exe: Option<std::path::PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(&et_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".exe") && name != "easytier.exe" {
                    found_exe = Some(entry.path());
                    break;
                }
            }
        }
    }

    if let Some(src) = found_exe {
        std::fs::rename(&src, &et_bin).map_err(|e| format!("重命名失败: {}", e))?;
    }

    Ok(())
}
