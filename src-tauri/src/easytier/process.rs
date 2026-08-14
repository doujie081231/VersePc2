// easytier/process.rs — 陶瓦联机（Terracotta）子进程管理
// 职责：下载并启动 terracotta.exe、主机/客户端模式切换、HTTP API 调用、状态轮询
//
// 启动流程（与 electron 版一致）：
//   1. 检查 terracotta.exe 是否已下载
//   2. spawn terracotta.exe --hmcl <端口文件>，二进制会把 HTTP API 端口写入该文件
//   3. 轮询端口文件获得 HTTP 端口
//   4. 主机模式调用 /state/scanning，客户端模式调用 /state/guesting
//   5. 后台轮询 /state，把原始状态写入全局状态，供前端展示
//
// 主机/客户端模式命令：
//   terracotta.exe --hmcl <port_file>

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::Child;

use super::state::{self, EasyTierMode};

/// 全局子进程句柄
static CHILD: Mutex<Option<Child>> = Mutex::new(None);
/// 后台状态轮询任务句柄
static POLLER: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

/// Terracotta 版本（对应 electron 项目 ctx.network.TERRACOTTA_VERSION）
const TERRACOTTA_VERSION: &str = "0.4.2";

/// 陶瓦联机错误码 -> (key, msg)
fn terracotta_error_map(code: i64) -> (&'static str, &'static str) {
    match code {
        1 => ("配置错误", "请检查网络设置或房间码是否正确"),
        2 => ("网络错误", "无法连接到联机节点，请检查网络或切换节点"),
        3 => ("版本不兼容", "Terracotta 版本不兼容，请尝试更新组件"),
        4 => ("房间已满", "房间人数已达上限"),
        5 => ("房间不存在", "房间码无效或房间已关闭"),
        6 => ("密码错误", "房间密码错误"),
        7 => ("被踢出", "你已被房主移出房间"),
        8 => ("连接超时", "连接超时，请检查网络或重试"),
        9 => ("服务器关闭", "联机服务器已关闭"),
        10 => ("协议错误", "通信协议错误，请重试"),
        _ => ("未知错误", "发生未知错误，请联系开发者"),
    }
}

/// 拉取公共节点列表（与 electron 版一致，从官方节点接口获取，带回退）
pub async fn fetch_public_nodes(_force_refresh: bool) -> Vec<String> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(_) => return default_nodes(),
    };

    let resp = match client.get("https://terracotta.glavo.site/nodes").send().await {
        Ok(r) => r,
        Err(_) => return default_nodes(),
    };
    let data: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return default_nodes(),
    };

    let is_china = std::env::var("TZ").map(|tz| tz.contains("Shanghai") || tz.contains("Chongqing")).unwrap_or(false);

    let mut nodes: Vec<String> = Vec::new();
    if let Some(arr) = data.as_array() {
        for item in arr {
            let url = item.get("url").and_then(|u| u.as_str()).unwrap_or("");
            if url.is_empty() {
                continue;
            }
            if let Some(region) = item.get("region").and_then(|r| r.as_str()) {
                if is_china && region != "CN" {
                    continue;
                }
            }
            nodes.push(url.to_string());
        }
    }

    if nodes.is_empty() {
        default_nodes()
    } else {
        nodes
    }
}

fn default_nodes() -> Vec<String> {
    vec![
        "https://etnode.zkitefly.eu.org/node1".to_string(),
        "https://etnode.zkitefly.eu.org/node2".to_string(),
    ]
}

/// 通过 HTTP API 调用 Terracotta
/// GET http://127.0.0.1:<port><path>
pub async fn http_get(http_port: u16, path: &str) -> Result<Value, String> {
    http_get_raw(http_port, path).await
}

async fn http_get_raw(http_port: u16, path: &str) -> Result<Value, String> {
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

    let text = resp.text().await.unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        Ok(v)
    } else {
        Ok(json!({ "text": text }))
    }
}

/// 递归收集目录下所有文件
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                collect_files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// 下载并解压 Terracotta 内核到 dataDir/terracotta/
pub async fn download() -> Result<(), String> {
    let data_dir = crate::storage::resolve_data_dir();
    let tc_dir = data_dir.join("terracotta");
    std::fs::create_dir_all(&tc_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let exe_path = tc_dir.join("terracotta.exe");
    if exe_path.exists() {
        return Ok(());
    }

    let arch = "windows-x86_64";
    let pkg = format!("terracotta-{}-{}-pkg.tar.gz", TERRACOTTA_VERSION, arch);
    let urls = [
        format!("https://gitee.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRACOTTA_VERSION, pkg),
        format!("https://cnb.cool/HMCL-Terracotta/Terracotta/-/releases/download/v{}/{}", TERRACOTTA_VERSION, pkg),
        format!("https://cdn.jsdelivr.net/gh/burningtnt/Terracotta@v{}/{}", TERRACOTTA_VERSION, pkg),
        format!("https://ghfast.top/https://github.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRACOTTA_VERSION, pkg),
        format!("https://mirror.ghproxy.com/https://github.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRACOTTA_VERSION, pkg),
        format!("https://github.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRACOTTA_VERSION, pkg),
    ];

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let mut bytes: Option<Vec<u8>> = None;
    let mut last_err = String::new();
    for url in &urls {
        match client.get(url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.bytes().await {
                        Ok(b) if b.len() >= 10000 => {
                            bytes = Some(b.to_vec());
                            break;
                        }
                        Ok(b) => {
                            last_err = format!("文件过小 ({} bytes): {}", b.len(), url);
                            continue;
                        }
                        Err(e) => {
                            last_err = format!("读取响应失败: {} ({})", e, url);
                            continue;
                        }
                    }
                } else {
                    last_err = format!("下载 HTTP {} ({})", resp.status(), url);
                    continue;
                }
            }
            Err(e) => {
                last_err = format!("下载失败: {} ({})", e, url);
                continue;
            }
        }
    }

    let bytes = bytes.ok_or_else(|| format!("所有下载源均失败: {}", last_err))?;

    // 解压 tar.gz（与 electron 版一致，使用系统 tar 命令）
    let tmp_dir = tc_dir.join("_tmp_extract");
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let archive_path = tc_dir.join("_pkg.tar.gz");
    std::fs::write(&archive_path, &bytes).map_err(|e| format!("写入压缩包失败: {}", e))?;

    let mut tar_cmd = std::process::Command::new("tar");
    tar_cmd
        .arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&tmp_dir);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        tar_cmd.creation_flags(0x08000000);
    }
    let tar_out = tar_cmd
        .output()
        .map_err(|e| format!("解压失败: {}", e))?;
    let _ = std::fs::remove_file(&archive_path);
    if !tar_out.status.success() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Err(format!(
            "解压失败: {}",
            String::from_utf8_lossy(&tar_out.stderr).trim()
        ));
    }

    // 找到 exe 与 dll
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&tmp_dir, &mut files);

    let mut found_exe: Option<PathBuf> = None;
    let mut dlls: Vec<PathBuf> = Vec::new();
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lower = name.to_lowercase();
        if lower.ends_with(".exe") {
            found_exe = Some(f.clone());
        } else if lower.ends_with(".dll") {
            dlls.push(f);
        }
    }

    if let Some(src) = found_exe {
        std::fs::copy(&src, &exe_path).map_err(|e| format!("复制内核失败: {}", e))?;
        for dll in dlls {
            let _ = std::fs::copy(&dll, tc_dir.join(dll.file_name().unwrap()));
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return Ok(());
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Err("未在压缩包中找到 terracotta.exe".to_string())
}

/// 轮询端口文件，等待 Terracotta 写入 HTTP 端口
async fn wait_for_port_file(file_path: &Path, timeout_ms: u64) -> Result<u16, String> {
    let start = std::time::Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(file_path) {
            let trimmed = text.trim();
            // 优先解析 JSON 中的 port 字段，其次把整段文本当作数字
            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                if let Some(p) = v.get("port").and_then(|x| x.as_u64()) {
                    return Ok(p as u16);
                }
            } else if let Ok(p) = trimmed.parse::<u64>() {
                return Ok(p as u16);
            }
        }
        if start.elapsed().as_millis() as u64 > timeout_ms {
            return Err("Terracotta 启动超时，请重试".to_string());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// 启动主机模式
///
/// 参数：
///   - game_port: Minecraft 游戏端口（默认 25565）
///   - player_name: 主机玩家名
///
/// 返回 { success, httpPort, gamePort }
pub async fn start_host(game_port: u16, player_name: &str) -> Result<Value, String> {
    if !state::is_installed() {
        download().await.map_err(|e| format!("陶瓦联机内核下载失败: {}", e))?;
    }

    stop().await;

    let bin_path = state::get_binary_path();
    let port_file = std::env::temp_dir().join(format!("versepc-terracotta-{}.http", std::process::id()));
    let _ = std::fs::remove_file(&port_file);

    let mut cmd = tokio::process::Command::new(&bin_path);
    cmd.arg("--hmcl").arg(&port_file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动陶瓦联机失败: {}", e))?;

    {
        let mut g = CHILD.lock().unwrap();
        *g = Some(child);
    }

    let http_port = match wait_for_port_file(&port_file, 30000).await {
        Ok(p) => p,
        Err(e) => {
            let _ = stop().await;
            return Err(e);
        }
    };

    state::update_status(|s| {
        s.running = true;
        s.mode = EasyTierMode::Host;
        s.game_port = game_port;
        s.http_port = http_port;
        s.player_name = player_name.to_string();
        s.error_type = None;
        s.error_message = None;
    });

    // 拉取公共节点并开始扫描局域网游戏
    let nodes = fetch_public_nodes(false).await;
    let mut q = format!("player={}", urlencoding::encode(player_name));
    for n in &nodes {
        q.push_str(&format!("&public_nodes={}", urlencoding::encode(n)));
    }

    let mut last_err = String::new();
    for attempt in 0..3 {
        tokio::time::sleep(Duration::from_millis(1000 + attempt * 500)).await;
        match http_get_raw(http_port, &format!("/state/scanning?{}", q)).await {
            Ok(_) => {
                last_err.clear();
                break;
            }
            Err(e) => {
                last_err = e;
                eprintln!("[terracotta] /state/scanning 第{}次失败: {}", attempt + 1, last_err);
                if attempt == 2 {
                    let _ = stop().await;
                    return Err(format!("陶瓦联机初始化失败: {}", last_err));
                }
            }
        }
    }

    start_poller(http_port);

    Ok(json!({
        "success": true,
        "httpPort": http_port,
        "gamePort": game_port
    }))
}

/// 启动客户端模式
///
/// 参数：
///   - room_code: 主机房间码
///   - player_name: 玩家名
///
/// 返回 { success, httpPort }
pub async fn start_guest(room_code: &str, player_name: &str) -> Result<Value, String> {
    if !state::is_installed() {
        download().await.map_err(|e| format!("陶瓦联机内核下载失败: {}", e))?;
    }

    stop().await;

    let bin_path = state::get_binary_path();
    let port_file = std::env::temp_dir().join(format!("versepc-terracotta-{}.http", std::process::id()));
    let _ = std::fs::remove_file(&port_file);

    let mut cmd = tokio::process::Command::new(&bin_path);
    cmd.arg("--hmcl").arg(&port_file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动陶瓦联机失败: {}", e))?;

    {
        let mut g = CHILD.lock().unwrap();
        *g = Some(child);
    }

    let http_port = match wait_for_port_file(&port_file, 30000).await {
        Ok(p) => p,
        Err(e) => {
            let _ = stop().await;
            return Err(e);
        }
    };

    state::update_status(|s| {
        s.running = true;
        s.mode = EasyTierMode::Guest;
        s.room_code = room_code.to_string();
        s.http_port = http_port;
        s.player_name = player_name.to_string();
        s.error_type = None;
        s.error_message = None;
    });

    // 拉取公共节点并加入房间
    let nodes = fetch_public_nodes(false).await;
    let mut q = format!("room={}&player={}", urlencoding::encode(room_code), urlencoding::encode(player_name));
    for n in &nodes {
        q.push_str(&format!("&public_nodes={}", urlencoding::encode(n)));
    }

    let mut last_err = String::new();
    for attempt in 0..3 {
        tokio::time::sleep(Duration::from_millis(1000 + attempt * 500)).await;
        match http_get_raw(http_port, &format!("/state/guesting?{}", q)).await {
            Ok(_) => {
                last_err.clear();
                break;
            }
            Err(e) => {
                last_err = e;
                eprintln!("[terracotta] /state/guesting 第{}次失败: {}", attempt + 1, last_err);
                if attempt == 2 {
                    let _ = stop().await;
                    return Err(format!("陶瓦联机初始化失败: {}", last_err));
                }
            }
        }
    }

    start_poller(http_port);

    Ok(json!({
        "success": true,
        "httpPort": http_port
    }))
}

/// 停止陶瓦联机
pub async fn stop() {
    // 停止轮询
    {
        let mut g = POLLER.lock().unwrap();
        if let Some(h) = g.take() {
            h.abort();
        }
    }

    let child_opt = {
        let mut g = CHILD.lock().unwrap();
        g.take()
    };
    if let Some(mut child) = child_opt {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    state::reset_to_idle();
}

/// 启动后台状态轮询：每 500ms 拉取 /state 更新全局状态
fn start_poller(http_port: u16) {
    {
        let mut g = POLLER.lock().unwrap();
        if let Some(h) = g.take() {
            h.abort();
        }
    }
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if !state::is_running() {
                break;
            }
            if let Ok(v) = http_get_raw(http_port, "/state").await {
                update_state_from_api(v);
            }
        }
    });
    let mut g = POLLER.lock().unwrap();
    *g = Some(handle);
}

/// 解析 /state 响应并写入全局状态
fn update_state_from_api(v: Value) {
    let sv = v.get("state").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let idx = v.get("index").and_then(|x| x.as_i64()).unwrap_or(-1);
    let profiles = v
        .get("profiles")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let difficulty = v.get("difficulty").and_then(|d| d.as_str()).map(String::from);

    let room_code = if sv == "host-ok" {
        v.get("room")
            .map(|r| {
                if let Some(code) = r.get("code").and_then(|c| c.as_str()) {
                    code.to_string()
                } else {
                    r.as_str().unwrap_or("").to_string()
                }
            })
            .unwrap_or_default()
    } else {
        String::new() // 不覆盖已有值
    };

    let virtual_ip = if sv == "guest-ok" {
        v.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string()
    } else {
        String::new()
    };

    let (error_type, error_message) = if sv == "exception" {
        let code = v.get("type").and_then(|t| t.as_i64()).unwrap_or(-1);
        let (k, m) = terracotta_error_map(code);
        (Some(k.to_string()), Some(m.to_string()))
    } else {
        (None, None)
    };

    state::update_status(|s| {
        s.state = Some(v.clone());
        s.state_index = idx;
        s.profiles = profiles;
        s.difficulty = difficulty;
        if sv == "host-ok" && !room_code.is_empty() {
            s.room_code = room_code;
        }
        if sv == "guest-ok" && !virtual_ip.is_empty() {
            s.virtual_ip = virtual_ip;
        }
        if sv == "exception" {
            s.error_type = error_type;
            s.error_message = error_message;
        } else {
            s.error_type = None;
            s.error_message = None;
        }
    });
}