// scene_renderer.rs — Wallpaper Engine 场景壁纸渲染进程管理
// 职责：启动/停止 verse-scene-renderer.exe（离屏 OpenGL 渲染 WE scene 壁纸，
//       经本地 HTTP 输出 MJPEG 流），前端 <img> 显示动态壁纸。
//       渲染器组件约 314MB，不随主程序分发；缺失时由前端引导下载到 <数据目录>/scene-renderer/。
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

// creation_flags（隐藏子进程控制台窗口）为 Windows 专用扩展
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// 当前运行的渲染子进程
static RENDERER_CHILD: Mutex<Option<Child>> = Mutex::new(None);
/// 渲染进程使用的本地 HTTP 端口
static RENDERER_PORT: AtomicU16 = AtomicU16::new(0);
/// 渲染进程 PID（0 = 未运行）
static RENDERER_PID: AtomicI32 = AtomicI32::new(0);

/// 渲染器组件压缩包文件名（GitHub/Gitee release 资产，zip 根目录即 exe 与 DLL）
const RENDERER_PACKAGE_FILE: &str = "scene-renderer-windows-x64.zip";

/// 渲染器组件压缩包的 GitHub release 地址（版本号跟随应用版本）
fn renderer_package_github_url() -> String {
    format!(
        "https://github.com/doujie081231/VersePc2/releases/download/v{}/{}",
        env!("CARGO_PKG_VERSION"),
        RENDERER_PACKAGE_FILE
    )
}

/// 定位 verse-scene-renderer.exe（优先 <数据目录>/scene-renderer/，其次主程序同目录）
fn renderer_exe_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let data_dir = crate::storage::resolve_data_dir();
    let candidates = [
        data_dir.join("scene-renderer").join("verse-scene-renderer.exe"),
        exe_dir.join("scene-renderer").join("verse-scene-renderer.exe"),
        exe_dir.join("verse-scene-renderer.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 挑一个空闲 TCP 端口（bind 0 探测）
fn pick_free_port() -> Option<u16> {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    listener.local_addr().ok().map(|a| a.port())
}

/// 渲染进程是否存活
fn is_child_alive(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    }
}

/// 启动场景壁纸渲染进程
/// 参数：workshop_id（WE 创意工坊作品 ID）
#[tauri::command]
pub fn scene_renderer_start(workshop_id: String) -> Value {
    // 已有渲染进程则先停掉
    scene_renderer_stop();

    // 定位壁纸目录
    let Some(project_dir) = crate::wallpaper_engine::resolve_we_project_dir(&workshop_id) else {
        return json!({ "ok": false, "error": "未找到该壁纸目录（workshop/content/431960）" });
    };

    // 定位渲染器可执行文件
    let Some(exe) = renderer_exe_path() else {
        return json!({ "ok": false, "missing": true, "error": "未安装场景渲染器组件" });
    };

    let Some(port) = pick_free_port() else {
        return json!({ "ok": false, "error": "无法分配本地端口" });
    };

    // 渲染分辨率跟随窗口，上限 1080p
    let width = 1280i32;
    let height = 720i32;

    // Mesa 软件渲染（无 GPU/D3D12 环境可靠回退）+ SDL 虚拟音频（渲染器禁用音频）
    let mut cmd = Command::new(&exe);
    cmd.arg("--project")
        .arg(&project_dir)
        .arg("--port")
        .arg(port.to_string())
        .arg("--width")
        .arg(width.to_string())
        .arg("--height")
        .arg(height.to_string())
        .env("GALLIUM_DRIVER", "llvmpipe")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("SDL_AUDIODRIVER", "dummy");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW：隐藏子进程控制台窗口

    // 渲染器 stdout/stderr 重定向到 <数据目录>/logs/scene-renderer.log，便于排查渲染失败原因
    let data_dir = crate::storage::resolve_data_dir();
    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    if let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("scene-renderer.log"))
    {
        if let Ok(o) = log_file.try_clone() {
            cmd.stdout(Stdio::from(o));
        }
        cmd.stderr(Stdio::from(log_file));
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return json!({ "ok": false, "error": format!("启动渲染进程失败: {}", e) });
        }
    };

    let pid = child.id() as i32;
    *RENDERER_CHILD.lock().unwrap() = Some(child);
    RENDERER_PORT.store(port, Ordering::SeqCst);
    RENDERER_PID.store(pid, Ordering::SeqCst);

    json!({ "ok": true, "port": port, "pid": pid, "project": project_dir.to_string_lossy().to_string() })
}

/// 停止场景壁纸渲染进程
#[tauri::command]
pub fn scene_renderer_stop() -> Value {
    let mut guard = RENDERER_CHILD.lock().unwrap();
    if let Some(mut child) = guard.take() {
        // 先优雅结束（HTTP 线程退出），超时则强杀
        let _ = child.kill();
        let _ = child.wait();
        // 兜底：按 PID 再杀一次（防句柄泄漏）
        let pid = RENDERER_PID.swap(0, Ordering::SeqCst);
        if pid > 0 {
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F", "/T"])
                    .creation_flags(0x08000000)
                    .output();
            }
        }
    }
    RENDERER_PORT.store(0, Ordering::SeqCst);
    json!({ "ok": true })
}

/// 渲染进程状态
#[tauri::command]
pub fn scene_renderer_status() -> Value {
    let mut guard = RENDERER_CHILD.lock().unwrap();
    let running = match guard.as_mut() {
        Some(child) => is_child_alive(child),
        None => false,
    };
    json!({
        "ok": true,
        "running": running,
        "pid": if running { RENDERER_PID.load(Ordering::SeqCst) } else { 0 },
        "port": if running { RENDERER_PORT.load(Ordering::SeqCst) } else { 0 },
    })
}

/// 应用退出时清理渲染进程
pub fn shutdown_renderer() {
    scene_renderer_stop();
}

/// 等待渲染进程就绪（HTTP 监听），最多等 N 秒
pub fn wait_renderer_ready(timeout: Duration) -> bool {
    let port = RENDERER_PORT.load(Ordering::SeqCst);
    if port == 0 {
        return false;
    }
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 渲染器组件是否已安装（供前端展示下载入口）
#[tauri::command]
pub fn scene_renderer_installed() -> Value {
    match renderer_exe_path() {
        Some(p) => json!({
            "ok": true,
            "installed": true,
            "path": p.to_string_lossy().to_string(),
        }),
        None => json!({
            "ok": true,
            "installed": false,
            "sizeHintMb": 314,
        }),
    }
}

/// 下载并安装场景渲染器组件到 <数据目录>/scene-renderer/。
/// 下载源：Gitee → GitHub → ghfast 镜像（与更新器一致）；进度通过 scene-renderer:progress 事件推送。
#[tauri::command]
pub async fn scene_renderer_download(app: tauri::AppHandle) -> Value {
    // 已安装则直接返回
    if let Some(p) = renderer_exe_path() {
        return json!({ "ok": true, "already": true, "path": p.to_string_lossy().to_string() });
    }

    let data_dir = crate::storage::resolve_data_dir();
    let dest_dir = data_dir.join("scene-renderer");
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        return json!({ "ok": false, "error": format!("无法创建数据目录: {}", e) });
    }
    let tmp_zip = data_dir.join(format!(".renderer_{}.zip", crate::utils::now_millis()));
    let tmp_dir = data_dir.join(format!(".renderer_tmp_{}", crate::utils::now_millis()));

    // 下载进度 → 前端事件（stage=download）
    let app2 = app.clone();
    let on_progress: Option<crate::download::ProgressCb> = Some(std::sync::Arc::new(move |p: &crate::download::DownloadProgress| {
        let percent = if p.total_bytes > 0 {
            ((p.bytes_downloaded as f64 / p.total_bytes as f64) * 100.0).min(99.0)
        } else {
            0.0
        };
        let _ = app2.emit("scene-renderer:progress", &json!({
            "stage": "download",
            "percent": percent,
            "transferred": p.bytes_downloaded,
            "total": p.total_bytes,
            "speed": p.speed,
        }));
    }));

    // Gitee → GitHub → ghfast 镜像，复用更新器的下载源构建逻辑
    let sources = crate::updater::build_download_sources(&renderer_package_github_url());

    if let Err(e) = crate::download::single::download_file_race(&sources, &tmp_zip, None, None, 300, on_progress).await {
        let _ = std::fs::remove_file(&tmp_zip);
        return json!({ "ok": false, "error": format!("组件下载失败: {}", e) });
    }

    let _ = app.emit("scene-renderer:progress", &json!({ "stage": "extract", "percent": 99.0, "message": "正在解压组件..." }));
    let file = match std::fs::File::open(&tmp_zip) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_zip);
            return json!({ "ok": false, "error": format!("无法打开下载的组件包: {}", e) });
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_zip);
            return json!({ "ok": false, "error": format!("组件包不是有效的 zip: {}", e) });
        }
    };
    // 安全解压到临时目录（防 zip-slip），再整体替换目标目录
    if let Err(e) = crate::plugins::extract_safely(&mut archive, &tmp_dir) {
        let _ = std::fs::remove_file(&tmp_zip);
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return json!({ "ok": false, "error": format!("解压组件失败: {}", e) });
    }
    if !tmp_dir.join("verse-scene-renderer.exe").is_file() {
        let _ = std::fs::remove_file(&tmp_zip);
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return json!({ "ok": false, "error": "组件包缺少 verse-scene-renderer.exe" });
    }
    let _ = std::fs::remove_dir_all(&dest_dir);
    match std::fs::rename(&tmp_dir, &dest_dir) {
        Ok(()) => {
            let _ = app.emit("scene-renderer:progress", &json!({ "stage": "done", "percent": 100.0 }));
            json!({ "ok": true, "path": dest_dir.join("verse-scene-renderer.exe").to_string_lossy().to_string() })
        }
        Err(e) => json!({ "ok": false, "error": format!("安装组件失败: {}", e) }),
    }
}
