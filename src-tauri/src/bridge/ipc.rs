// bridge/ipc.rs — 单实例命名管道
//
// 目标：浏览器扩展 / 命令行二次启动时，把请求转发给"已在运行的那个实例"，
//       而不是再开一个新进程。
//
// 管道名：\\.\pipe\VersePC2_ipc_<用户名>
//   - 用用户名做后缀，避免同机多用户互相抢管道
//   - 默认安全描述符继承当前用户令牌，其他用户连不上，天然隔离
//
// 协议：一行 JSON 请求 → 一行 JSON 响应（换行分隔），完成后双方关闭连接。
//
// 平台：仅 Windows 有实现；其它平台降级为 no-op（调用方按"转发失败"处理）。

use serde_json::{json, Value};
use std::time::Duration;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const ACTION_DOWNLOAD: &str = "download";

/// 管道全名
#[cfg(target_os = "windows")]
pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    let safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    format!(r"\\.\pipe\VersePC2_ipc_{}", safe)
}

// ============== 客户端：把请求发给已运行实例 ==============

/// 同步调用：连接管道 → 写一行 JSON → 读一行 JSON 响应。
/// 返回 None 表示没有已运行实例（管道不存在）或超时/出错。
#[cfg(target_os = "windows")]
pub fn call(req: &Value, timeout: Duration) -> Option<Value> {
    use std::io::{Read, Write};

    let name = pipe_name();
    let payload = format!("{}\n", req);

    // 独立线程执行，避免管道存在但对端卡死时阻塞调用方（浏览器宿主/启动流程）
    let (tx, rx) = std::sync::mpsc::channel::<Option<Value>>();
    std::thread::spawn(move || {
        let result = (|| -> Option<Value> {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
                .ok()?;
            file.write_all(payload.as_bytes()).ok()?;
            file.flush().ok()?;
            let mut buf = vec![0u8; 8192];
            let n = file.read(&mut buf).ok()?;
            if n == 0 {
                return None;
            }
            serde_json::from_str::<Value>(String::from_utf8_lossy(&buf[..n]).trim()).ok()
        })();
        let _ = tx.send(result);
    });

    rx.recv_timeout(timeout).ok().flatten()
}

#[cfg(not(target_os = "windows"))]
pub fn call(_req: &Value, _timeout: Duration) -> Option<Value> {
    None
}

/// 尝试转发；true 表示对方已接收（调用方应直接退出，不再启动自己的窗口）
pub fn try_forward(req: &Value) -> bool {
    match call(req, Duration::from_millis(2500)) {
        Some(resp) => resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        None => false,
    }
}

// ============== 服务端：首实例常驻监听 ==============

/// 在首实例里启动管道监听（后台线程 + 独立 runtime，不占用 Tauri 的 runtime）
#[cfg(target_os = "windows")]
pub fn serve(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[bridge] 管道线程 runtime 创建失败: {}", e);
                return;
            }
        };
        rt.block_on(async move {
            if let Err(e) = accept_loop(app).await {
                eprintln!("[bridge] 管道监听退出: {}", e);
            }
        });
    });
}

#[cfg(not(target_os = "windows"))]
pub fn serve(_app: tauri::AppHandle) {}

#[cfg(target_os = "windows")]
async fn accept_loop(app: tauri::AppHandle) -> Result<(), String> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = pipe_name();
    // first_pipe_instance 抢占名字：抢不到说明已有实例在跑，本次不再监听
    let mut server = match ServerOptions::new().first_pipe_instance(true).create(&name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[bridge] 管道创建失败（可能已有实例）: {}", e);
            return Ok(());
        }
    };

    loop {
        if let Err(e) = server.connect().await {
            eprintln!("[bridge] 管道连接失败: {}", e);
            match ServerOptions::new().create(&name) {
                Ok(s) => {
                    server = s;
                    continue;
                }
                Err(e2) => return Err(format!("重建管道失败: {}", e2)),
            }
        }

        // 已连接实例交给独立任务，主循环立刻建下一个实例接新客户端
        let connected = server;
        let app_clone = app.clone();
        tokio::spawn(async move {
            handle_connection(connected, app_clone).await;
        });

        server = match ServerOptions::new().create(&name) {
            Ok(s) => s,
            Err(e) => return Err(format!("创建管道实例失败: {}", e)),
        };
    }
}

#[cfg(target_os = "windows")]
async fn handle_connection(
    mut conn: tokio::net::windows::named_pipe::NamedPipeServer,
    app: tauri::AppHandle,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 8192];
    let n = match tokio::time::timeout(Duration::from_secs(3), conn.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return,
    };
    if n == 0 {
        return;
    }
    let text = String::from_utf8_lossy(&buf[..n]).trim().to_string();
    let req: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => {
            let _ = conn.write_all(b"{\"ok\":false,\"error\":\"bad json\"}\n").await;
            return;
        }
    };

    let reply = dispatch(&app, &req).await;
    let _ = conn.write_all(format!("{}\n", reply).as_bytes()).await;
    let _ = conn.flush().await;
}

/// 处理一条来自管道/原生宿主的请求
/// （非 Windows 平台没有管道，此函数只作为协议实现的单一出口保留）
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub async fn dispatch(app: &tauri::AppHandle, req: &Value) -> Value {
    let action = req.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        ACTION_DOWNLOAD => {
            let url = req.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if url.is_empty() {
                return json!({ "ok": false, "error": "缺少 url" });
            }
            let name = req.get("filename").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let folder = req.get("folder").and_then(|v| v.as_str()).unwrap_or("").to_string();
            submit_download(app, url, name, folder).await
        }
        "ping" => json!({
            "ok": true,
            "pid": std::process::id(),
            "pending": super::pending_count(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
        _ => json!({ "ok": false, "error": format!("未知 action: {}", action) }),
    }
}

/// 写入待处理队列 + 窗口置前 + 通知前端
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub async fn submit_download(
    app: &tauri::AppHandle,
    url: String,
    filename: String,
    folder: String,
) -> Value {
    use tauri::{Emitter, Manager};

    let req = json!({
        "url": url,
        "filename": filename,
        "folder": folder,
        "source": "bridge",
        "at": super::now_ms(),
    });
    if let Some(win) = app.get_webview_window("main") {
        // 后台唤起时窗口可能还停在 splash 阶段（visible:false）
        let _ = win.show();
        let _ = win.unminimize();
        // Topmost 抖动是 Tauri 里最可靠的前台提升手段
        let _ = win.set_always_on_top(true);
        let _ = win.set_focus();
        tokio::time::sleep(Duration::from_millis(120)).await;
        let _ = win.set_always_on_top(false);
        // 窗口在 = 前端已订阅事件，直接推送即可；
        // 不再入队，否则前端下次启动会重复把旧请求填一遍。
        let _ = win.emit("download-request", &req);
    } else {
        // 没有窗口（冷启动早期/首次拉起）：先入队，前端起来后自己取走
        super::push_pending(req.clone());
    }

    json!({ "ok": true, "accepted": true, "pending": super::pending_count() })
}
