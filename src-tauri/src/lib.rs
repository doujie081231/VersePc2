// VersePC Tauri 后端入口
// 职责：模块声明、窗口控制命令、路径获取命令、命令注册

mod api;
mod ai;
mod auth;
mod avatar;
mod crash_analyzer;
mod dialog;
mod download;
mod favicon;
mod easytier;
mod filesystem;
mod install;
mod java;
mod launch;
mod modloaders;
mod modpack;
mod mods;
mod network;
mod private_server;
mod redstone_online;
mod server_host;
mod storage;
mod system;
mod tts;
mod updater;
mod utils;
mod versions;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, PhysicalPosition, Position};

// ============== Windows 内存压缩：EmptyWorkingSet ==============
// 调用 Windows psapi.dll 的 EmptyWorkingSet，将进程不活跃的内存页交换出物理内存
// 这就是 electron 项目"最小化后内存数字大幅下降"的核心手段
#[cfg(target_os = "windows")]
fn trim_working_set() {
    use std::ffi::c_void;
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn EmptyWorkingSet(hProcess: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }
    unsafe {
        let handle = GetCurrentProcess();
        if !handle.is_null() {
            EmptyWorkingSet(handle);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn trim_working_set() {
    // 非 Windows 平台不做处理
}

/// 执行一次完整的内存优化（对齐 electron 项目的 _doFullMemoryOptimize）
/// 1. 提示前端做垃圾回收 + 壁纸挂起
/// 2. 调用 EmptyWorkingSet 压缩工作集（任务管理器里可见的内存下降）
fn do_full_memory_optimize(app_handle: &tauri::AppHandle) {
    // 1. 通知前端执行内存清理（壁纸挂起、释放可选缓存、提示GC）
    if let Some(win) = app_handle.get_webview_window("main") {
        let _ = win.emit("memory-optimize-request", ());
    }
    // 2. Windows API 工作集压缩（这步让任务管理器里的数字明显变小）
    trim_working_set();
}

// ============== 窗口控制 ==============

#[tauri::command]
fn window_minimize(window: tauri::WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
fn window_maximize(window: tauri::WebviewWindow) {
    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    } else if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

/// 显示主窗口（前端 splash 首屏渲染完成后调用，避免启动时"黑屏闪一下"）
#[tauri::command]
fn window_show(window: tauri::WebviewWindow) {
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn window_close(window: tauri::WebviewWindow) {
    // 关闭前触发前端 CSS 关闭动画
    let _ = window.emit("request-close-animate", ());

    let win = window.clone();
    tauri::async_runtime::spawn(async move {
        let (start_x, start_y, win_h) = match win.outer_position() {
            Ok(pos) => {
                let h = win
                    .inner_size()
                    .ok()
                    .map(|s| s.height as i32)
                    .unwrap_or(800);
                (pos.x, pos.y, h)
            }
            Err(_) => return,
        };

        // 移动距离 = 窗口高度 + 100px，确保完全移出屏幕
        let distance = win_h + 100;
        let target_y = start_y - distance;
        // 总时长 300ms，20 步×15ms
        let steps = 20u32;
        let step_delay_ms = 15u64;

        for i in 1..=steps {
            tokio::time::sleep(std::time::Duration::from_millis(step_delay_ms)).await;
            let progress = i as f64 / steps as f64;
            // easeOutCubic 缓动：先快后慢
            let eased = 1.0 - (1.0 - progress).powi(3);
            let y = start_y + ((target_y - start_y) as f64 * eased) as i32;
            let _ = win.set_position(Position::Physical(PhysicalPosition {
                x: start_x,
                y,
            }));
        }
        let _ = win.hide();
        let _ = win.close();
    });
}

#[tauri::command]
fn window_destroy(window: tauri::WebviewWindow) {
    let _ = window.close();
}

#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    window.open_devtools();
}

/// 记录启动阶段耗时（诊断用）：把前端 init() 各阶段耗时追加写入 logs/startup-timing.log
#[tauri::command]
fn write_startup_timing(content: String) -> bool {
    let dir = storage::resolve_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write;
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("startup-timing.log"))
        .and_then(|mut f| {
            writeln!(f, "[{}] {}", ts, content)
        })
        .is_ok()
}

#[tauri::command]
fn window_is_maximized(window: tauri::WebviewWindow) -> bool {
    window.is_maximized().unwrap_or(false)
}

#[tauri::command]
fn window_is_fullscreen(window: tauri::WebviewWindow) -> bool {
    window.is_fullscreen().unwrap_or(false)
}

#[tauri::command]
fn window_restore(window: tauri::WebviewWindow) {
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
    }
    let _ = window.set_focus();
}

#[tauri::command]
fn window_is_minimized(window: tauri::WebviewWindow) -> bool {
    window.is_minimized().unwrap_or(false)
}

#[tauri::command]
fn window_set_size(window: tauri::WebviewWindow, width: f64, height: f64) -> bool {
    use tauri::LogicalSize;
    window.set_size(LogicalSize { width, height }).is_ok()
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ============== 设置存储命令（store.json KV 存储） ==============
// 兼容原项目 window.electronAPI.store.get(key) / set(key, value)

#[tauri::command]
fn store_get(state: tauri::State<storage::Store>, key: String) -> Option<Value> {
    state.get(&key)
}

#[tauri::command]
fn store_set(state: tauri::State<storage::Store>, key: String, value: Value) -> bool {
    state.set(key, value)
}

#[tauri::command]
fn store_delete(state: tauri::State<storage::Store>, key: String) -> bool {
    state.delete(&key)
}

// ============== 系统工具命令 ==============

#[tauri::command]
fn open_external(url: String) -> bool {
    // 在默认浏览器中打开 URL（支持 http/https/mailto 等协议）
    // 使用 `open` crate，跨平台实现
    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("mailto:") {
        return false;
    }
    match open::that(&url) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[open_external] 打开 URL 失败: {} - {}", url, e);
            false
        }
    }
}

// ============== 路径获取命令 ==============

#[tauri::command]
fn get_versions_dir() -> Value {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let _ = std::fs::create_dir_all(&versions_dir);
    json!({ "success": true, "path": versions_dir.to_string_lossy() })
}

#[tauri::command]
fn get_external_version_folders(state: tauri::State<storage::Store>) -> Value {
    let folders = state
        .get("externalVersionFolders")
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({ "success": true, "folders": folders })
}

#[tauri::command]
fn get_default_mod_path(state: tauri::State<storage::Store>) -> Value {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let settings_file = data_dir.join("settings.json");
    let minecraft_dir = dirs::home_dir()
        .map(|h| h.join(".minecraft"))
        .unwrap_or_else(|| data_dir.clone());

    // 读取 settings.json
    let settings: Value = std::fs::read_to_string(&settings_file)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or(Value::Null);

    let mut version_id = settings
        .get("selectedVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 没有选中版本则取 versions 目录下第一个子目录
    if version_id.is_empty() {
        if let Ok(entries) = std::fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        version_id = name.to_string();
                        break;
                    }
                }
            }
        }
    }

    // 没有版本：回退到 .minecraft/mods
    if version_id.is_empty() {
        let default_path = minecraft_dir.join("mods");
        let _ = std::fs::create_dir_all(&default_path);
        return json!({ "success": true, "path": default_path.to_string_lossy() });
    }

    // 判断游戏目录
    let game_dir;
    if version_id.contains("[外部]") {
        let folders = state
            .get("externalVersionFolders")
            .and_then(|v| v.as_array().cloned());
        let clean_id = version_id.replace(" [外部]", "").replace("[外部]", "");
        let mut found = None;
        if let Some(arr) = folders {
            for folder in arr {
                if let Some(fs) = folder.as_str() {
                    let candidate = std::path::PathBuf::from(fs).join(&clean_id);
                    if candidate.exists() {
                        found = Some(candidate);
                        break;
                    }
                    let candidate2 = std::path::PathBuf::from(fs).join(&version_id);
                    if candidate2.exists() {
                        found = Some(candidate2);
                        break;
                    }
                }
            }
        }
        game_dir = found.unwrap_or_else(|| versions_dir.join(clean_id));
    } else {
        // 读取版本设置判断隔离
        let ver_settings_file = versions_dir.join(&version_id).join("version-settings.json");
        let mut effective_isolation: Option<bool> = None;
        if let Ok(content) = std::fs::read_to_string(&ver_settings_file) {
            if let Ok(vs) = serde_json::from_str::<Value>(&content) {
                match vs.get("isolation").and_then(|v| v.as_str()) {
                    Some("on") => effective_isolation = Some(true),
                    Some("off") => effective_isolation = Some(false),
                    _ => {}
                }
            }
        }
        if effective_isolation.is_none() {
            effective_isolation = Some(
                settings
                    .get("versionIsolation")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            );
        }
        let eff = effective_isolation.unwrap_or(true);
        if !eff {
            let version_dir = versions_dir.join(&version_id);
            let has_mods = version_dir.join("mods").exists();
            let has_saves = version_dir.join("saves").exists();
            let has_config = version_dir.join("config").exists();
            if has_mods || has_saves || has_config {
                game_dir = version_dir;
            } else {
                game_dir = settings
                    .get("gameDir")
                    .and_then(|v| v.as_str())
                    .map(std::path::PathBuf::from)
                    .unwrap_or(data_dir);
            }
        } else {
            game_dir = versions_dir.join(&version_id);
        }
    }

    let default_path = game_dir.join("mods");
    let _ = std::fs::create_dir_all(&default_path);
    json!({ "success": true, "path": default_path.to_string_lossy() })
}

// ============== TTS 语音合成命令 ==============

#[tauri::command]
async fn tts_speak(text: String, voice: Option<String>) -> Result<Vec<u8>, String> {
    let voice = voice.unwrap_or_else(|| "zh-CN-XiaoxiaoNeural".to_string());
    tts::synthesize(&text, &voice).await
}

// ============== 崩溃记录器 ==============
// 程序任何地方发生 panic（异常）时，先把原因写入 logs/crash.log 再退出，
// 避免出现"软件直接消失却查不到原因"。release 下 panic=abort，回调在退出前执行。

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "未知错误".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let line = format!("[{}] PANIC: {} @ {}\n", ts, msg, loc);
        eprintln!("{}", line);

        // 追加写入日志（resolve_data_dir 无锁无 unwrap，崩溃回调中可安全调用）
        let dir = storage::resolve_data_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("crash.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }));
}

// ============== 应用入口 ==============

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 初始化存储：使用便携版数据目录（与原项目一致，数据跟 exe 走）
            let data_dir = storage::resolve_data_dir();
            std::fs::create_dir_all(&data_dir)?;

            let store = storage::Store::new();
            println!("[store] 数据目录: {:?}", data_dir);
            println!("[store] 存储文件: {:?}", store.path);
            println!("[store] 加载条目数: {}", store.data.lock().unwrap().len());

            // 首次运行：检测旧版 Electron VersePC 数据目录并迁移个性化设置
            let migrated = storage::migrate_legacy_if_first_run(&store);
            if migrated {
                println!("[store] 已完成旧版 Electron 数据迁移");
            }

            // 首次运行/环境检查：WebView2 内核缺失则提示安装
            crate::system::ensure_webview2();

            app.manage(store);

            // 窗口默认隐藏（visible:false），等前端 splash 首屏渲染后调用 window_show 显示，避免启动黑屏闪一下。
            // 兜底：若前端因脚本异常始终未调用 show，延迟强制显示，确保窗口不会一直不出现。
            if let Some(main_win) = app.get_webview_window("main") {
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    if !main_win.is_visible().unwrap_or(false) {
                        let _ = main_win.show();
                    }
                });
            }

            // 最小化/恢复内存优化（对齐 electron 项目行为）
            // - 最小化：延迟 1.5s 后第一次优化，之后每 30s 循环执行
            // - 恢复：立即停止循环，通知前端恢复壁纸
            // - 空闲 3 分钟无操作：自动执行一次优化
            if let Some(main_win) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let main_win_for_min = main_win.clone();
                tauri::async_runtime::spawn(async move {
                    let mut last_min = false;
                    let mut _minimize_timer: Option<tokio::task::JoinHandle<()>> = None;

                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        let Ok(min) = main_win_for_min.is_minimized() else { continue };

                        if min != last_min {
                            // 状态变化：先通知前端（壁纸挂起/恢复）
                            let ev = if min { "window-minimized" } else { "window-restored" };
                            let _ = main_win_for_min.emit(ev, ());

                            if min {
                                // ===== 最小化了：启动定时优化循环 =====
                                // 取消上一次可能存在的定时器
                                if let Some(t) = _minimize_timer.take() {
                                    t.abort();
                                }
                                let ah = app_handle.clone();
                                _minimize_timer = Some(tokio::task::spawn(async move {
                                    // 第一次：延迟 1.5 秒
                                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                    do_full_memory_optimize(&ah);
                                    // 之后每 30 秒循环
                                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                                    loop {
                                        interval.tick().await;
                                        do_full_memory_optimize(&ah);
                                    }
                                }));
                            } else {
                                // ===== 恢复了：停止循环 =====
                                if let Some(t) = _minimize_timer.take() {
                                    t.abort();
                                }
                                // 恢复时也做一次轻量清理（移除不再需要的临时缓存）
                                tokio::task::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                                    trim_working_set();
                                });
                            }

                            last_min = min;
                        }
                    }
                });

                // ===== 空闲 5 分钟自动优化 =====
                let main_win_for_idle = main_win.clone();
                tauri::async_runtime::spawn(async move {
                    // 每 5 分钟做一次轻量优化（不通知前端，只压缩工作集）
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        // 如果此时正最小化（有单独的优化循环在跑），跳过
                        let minimized = main_win_for_idle.is_minimized().unwrap_or(false);
                        if !minimized {
                            trim_working_set();
                        }
                    }
                });
            }

            // 初始化 theseus 事件系统：把 Tauri AppHandle 注入 theseus，
            // 这样 theseus 内部的 emit_install_job / emit_loading 才能向前端推送事件
            let app_handle = app.handle().clone();
            modpack::theseus::event::set_app_handle(app_handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 窗口控制
            window_minimize,
            window_maximize,
            window_show,
            window_close,
            window_destroy,
            window_is_maximized,
            window_is_fullscreen,
            window_restore,
            window_is_minimized,
            window_set_size,
            // 应用信息
            get_app_version,
            // 设置存储（KV）
            store_get,
            store_set,
            store_delete,
            // 文件对话框
            dialog::dialog_open,
            dialog::select_folder,
            dialog::select_file,
            filesystem::read_file_buffer,
            // 路径获取
            get_versions_dir,
            get_external_version_folders,
            get_default_mod_path,
            // API 代理分发
            api::api_proxy,
            // 头像
            avatar::get_avatar,
            avatar::get_skin_texture,
            // 版本图标
            versions::get_version_icon,
            // 网站图标
            favicon::get_favicon,
            // 开发工具
            open_devtools,
            // 启动计时诊断
            write_startup_timing,
            // 系统工具
            open_external,
            // 运行环境检查（WebView2）
            system::check_webview2,
            // TTS 语音合成
            tts_speak,
            // AI 对话代理
            ai::ai_chat,
            // 自动更新（便携版全量替换）
            updater::updater_check_for_updates,
            updater::updater_download_update,
            updater::updater_install_update,
            updater::updater_skip_version,
            updater::updater_open_release_page,
            updater::updater_get_pending_notice,
            // 私人服务器管理
            private_server::private_server_list,
            private_server::private_server_save,
            private_server::private_server_add,
            private_server::private_server_update,
            private_server::private_server_delete,
            private_server::private_server_check,
            private_server::private_server_copy_address,
            private_server::private_server_icon,
            // 本地开服
            server_host::server_host_list,
            server_host::server_host_create,
            server_host::server_host_start,
            server_host::server_host_stop,
            server_host::server_host_command,
            server_host::server_host_status,
            server_host::server_host_delete,
            server_host::server_host_open_dir,
            server_host::server_host_resolve_version,
            server_host::server_host_detect_loader,
            server_host::server_host_sync_mods,
            // 红石联机内网穿透
            redstone_online::redstone_servers,
            redstone_online::redstone_apikey,
            redstone_online::redstone_apikey_reset,
            redstone_online::redstone_scan_port,
            redstone_online::redstone_start,
            redstone_online::redstone_stop,
            redstone_online::redstone_status,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用时出错");
}
