// plugin_exec.rs - 可信插件「安全执行外部进程」能力
// 说明：仅对 plugin.json 显式声明 permissions.native 含 "exec" 的插件开放。
// 前端沙箱对声明并确认过的插件放行；后端这里以磁盘上 <plugins>/<id>/plugin.json 再次硬校验，
// 防止绕过前端直接调用。
// 子进程 stdout/stderr 逐行回传 plugin:<id>:exec-log，进程退出回传 plugin:<id>:exec-exit。

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::Emitter;

use crate::plugins;

/// 运行中的插件进程表（懒加载，仅存 pid/启动时间，进程主体由后台任务持有）
struct PluginProc {
    pid: u32,
    start_ms: u64,
}

static PLUGIN_PROCS: Mutex<Option<HashMap<String, PluginProc>>> = Mutex::new(None);

fn proc_init() {
    let mut g = PLUGIN_PROCS.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

/// 校验插件是否声明了 native:exec 权限（以磁盘 plugin.json 为准）
fn has_native_exec(id: &str) -> bool {
    let dir = plugins::plugin_dir(id);
    let Ok(content) = std::fs::read_to_string(dir.join("plugin.json")) else {
        return false;
    };
    let Ok(m) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    m.get("permissions")
        .and_then(|p| p.get("native"))
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("exec")))
        .unwrap_or(false)
}

fn err(code: i32, msg: &str) -> Value {
    json!({ "ok": false, "code": code, "error": msg })
}

/// 平台化结束进程（Windows 用 taskkill /F /T 连带子进程树，Unix 用 kill -9）
fn kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}

/// 启动外部进程。请求：{ pluginId, exe, args?, cwd?, ini?, configName? }
///  - `ini`：可选，frpc 配置文本。提供时后端将写到 <插件目录>/<configName|frpc.ini>，
///    并在 args 中已含 `-c` 时不重复注入，否则自动追加 `-c <写出的绝对路径>`。
///   这样像 Sakura/MSL 这类需要 ini 配置的服务商，插件只需把取回的配置文本交给后端托管，
///   无需越权写任意文件。
#[tauri::command]
pub async fn plugin_process_exec(
    app: tauri::AppHandle,
    plugin_id: String,
    exe: String,
    args: Option<Vec<String>>,
    cwd: Option<String>,
    ini: Option<String>,
    config_name: Option<String>,
) -> Value {
    if !has_native_exec(&plugin_id) {
        return err(403, &format!("插件 {} 未声明 native:exec 权限", plugin_id));
    }
    if exe.trim().is_empty() {
        return err(400, "缺少可执行文件路径");
    }

    proc_init();
    {
        let g = PLUGIN_PROCS.lock().unwrap();
        if let Some(map) = g.as_ref() {
            if map.contains_key(&plugin_id) {
                return err(409, "该插件已有进程在运行");
            }
        }
    }

    let mut launch_args: Vec<String> = args.unwrap_or_default();

    // 可选：把 frpc 配置写入插件自身目录，并自动注入 -c 参数
    let mut config_path: Option<String> = None;
    if let Some(text) = ini {
        if !text.trim().is_empty() {
            let name = config_name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "frpc.ini".to_string());
            // 仅允许常规文件名，防止目录穿越
            if name.contains('/') || name.contains('\\') || name.contains("..") {
                return err(400, "非法的配置文件名");
            }
            let dir = plugins::plugin_dir(&plugin_id);
            let _ = std::fs::create_dir_all(&dir);
            let full_path = dir.join(&name);
            if let Err(e) = std::fs::write(&full_path, text.as_bytes()) {
                return err(500, &format!("写入配置失败: {}", e));
            }
            let path_str = full_path.to_string_lossy().to_string();
            if !launch_args.iter().any(|a| a == "-c") {
                launch_args.push("-c".to_string());
                launch_args.push(path_str.clone());
            }
            config_path = Some(path_str);
        }
    }

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&launch_args);
    if let Some(dir) = cwd {
        if !dir.trim().is_empty() {
            cmd.current_dir(dir);
        }
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return err(500, &format!("启动进程失败: {}", e)),
    };

    let pid = child.id().unwrap_or(0);
    {
        let mut g = PLUGIN_PROCS.lock().unwrap();
        if let Some(map) = g.as_mut() {
            map.insert(
                plugin_id.clone(),
                PluginProc {
                    pid,
                    start_ms: crate::utils::now_millis(),
                },
            );
        }
    }

    // 后台任务：读两流转发日志，等待退出后回传退出事件并从进程表移除
    let app2 = app.clone();
    let plug_id2 = plugin_id.clone();
    tauri::async_runtime::spawn(async move {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        async fn read_stream<S>(
            stream: Option<S>,
            app: tauri::AppHandle,
            evt: String,
            pid: u32,
            tag: &str,
        ) where
            S: tokio::io::AsyncRead + Unpin,
        {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let Some(s) = stream else { return };
            let mut reader = BufReader::new(s);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line
                            .trim_end_matches('\n')
                            .trim_end_matches('\r')
                            .to_string();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let _ = app.emit(
                            &evt,
                            json!({ "pid": pid, "stream": tag, "line": trimmed }),
                        );
                    }
                    Err(_) => break,
                }
            }
        }

        let evt_log = format!("plugin:{}:exec-log", plug_id2.clone());
        let evt_exit = format!("plugin:{}:exec-exit", plug_id2.clone());
        tokio::join!(
            read_stream(stdout, app2.clone(), evt_log.clone(), pid, "stdout"),
            read_stream(stderr, app2.clone(), evt_log, pid, "stderr"),
        );

        let status = child.wait().await;
        let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = app2.emit(&evt_exit, json!({ "pid": pid, "code": code }));

        // 进程结束，从运行表移除
        let mut g = PLUGIN_PROCS.lock().unwrap();
        if let Some(map) = g.as_mut() {
            if let Some(p) = map.get(&plug_id2) {
                if p.pid == pid {
                    map.remove(&plug_id2);
                }
            }
        }
    });

    json!({ "ok": true, "pid": pid, "configPath": config_path })
}

/// 停止外部进程。请求：{ pluginId }
#[tauri::command]
pub fn plugin_process_stop(plugin_id: String) -> Value {
    proc_init();
    let mut g = PLUGIN_PROCS.lock().unwrap();
    let proc = if let Some(map) = g.as_mut() {
        map.remove(&plugin_id)
    } else {
        None
    };
    match proc {
        Some(p) => {
            kill_pid(p.pid);
            json!({ "ok": true, "pid": p.pid })
        }
        None => err(404, "该插件没有正在运行的进程"),
    }
}

/// 查询外部进程状态。请求：{ pluginId }
#[tauri::command]
pub fn plugin_process_status(plugin_id: String) -> Value {
    proc_init();
    let g = PLUGIN_PROCS.lock().unwrap();
    if let Some(map) = g.as_ref() {
        if let Some(p) = map.get(&plugin_id) {
            return json!({ "ok": true, "running": true, "pid": p.pid });
        }
    }
    json!({ "ok": true, "running": false })
}