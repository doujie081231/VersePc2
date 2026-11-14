// process.rs — 由 server_host.rs 拆分
#![allow(unused_imports)]

// 运行状态、启动参数与进程管理
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::oneshot;
use crate::download::download_with_mirror;
use crate::storage;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use super::*;

#[allow(dead_code)]
pub(crate) struct RunningServer {
    pub(crate) stdin: Arc<tokio::sync::Mutex<Option<ChildStdin>>>,
    pub(crate) exit_rx: Mutex<Option<oneshot::Receiver<()>>>,
    pub(crate) pid: Option<u32>,
    pub(crate) dir: PathBuf,
    pub(crate) name: String,
    pub(crate) port: u16,
    pub(crate) starting: bool,
}

pub(crate) static RUNNING: Mutex<Option<HashMap<String, RunningServer>>> = Mutex::new(None);

pub(crate) fn running_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, RunningServer>>> {
    let mut guard = RUNNING.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

pub(crate) fn is_running(id: &str) -> bool {
    let guard = running_map();
    guard.as_ref().map(|m| m.contains_key(id)).unwrap_or(false)
}

/// 构建启动参数
pub(crate) fn build_launch_args(launch_kind: &str, dir: &std::path::Path, max_mem: u32, min_mem: u32) -> Vec<String> {
    let jvm_mem = vec![
        format!("-Xms{}M", min_mem),
        format!("-Xmx{}M", max_mem),
        "-XX:+UseG1GC".to_string(),
        "-XX:+ParallelRefProcEnabled".to_string(),
        "-XX:MaxGCPauseMillis=200".to_string(),
        "-Dfile.encoding=UTF-8".to_string(),
    ];

    if launch_kind == "run-script" {
        // 不会到这里：run-script 单独处理
        return jvm_mem;
    }
    if launch_kind == "fabric-launch" {
        let mut args = jvm_mem;
        args.push("-jar".into());
        args.push("fabric-server-launch.jar".into());
        args.push("--nogui".into());
        return args;
    }
    if launch_kind.starts_with("forge-jar:") {
        let jar_name = launch_kind.strip_prefix("forge-jar:").unwrap_or("");
        let mut args = jvm_mem;
        args.push("-jar".into());
        args.push(jar_name.to_string());
        args.push("--nogui".into());
        return args;
    }
    // 默认 server-jar
    let mut args = jvm_mem;
    args.push("-jar".into());
    args.push("server.jar".into());
    args.push("--nogui".into());
    let _ = dir; // 抑制未使用警告
    args
}

pub(crate) fn status_all() -> Value {
    let idx = load_index();
    let servers = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let running = is_running(&id);
            let status = if running {
                let guard = running_map();
                let starting = guard
                    .as_ref()
                    .and_then(|m| m.get(&id))
                    .map(|r| r.starting)
                    .unwrap_or(false);
                if starting { "starting" } else { "running" }
            } else {
                "stopped"
            };
            let mut s = s;
            if let Some(obj) = s.as_object_mut() {
                obj.insert("running".into(), json!(running));
                obj.insert("status".into(), json!(status));
            }
            s
        })
        .collect::<Vec<_>>();
    json!({ "ok": true, "servers": servers })
}

pub(crate) fn sanitize_name(name: &str) -> String {
    let re = regex::Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap();
    let mut s = re.replace_all(name, "_").to_string();
    let re2 = regex::Regex::new(r"\s+").unwrap();
    s = re2.replace_all(&s, " ").trim().to_string();
    if s.len() > 64 {
        s = s.chars().take(64).collect();
    }
    if s.is_empty() {
        "MyServer".into()
    } else {
        s
    }
}

pub(crate) fn gen_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // 直接取 36 进制字符：原实现把 10..35 映射到 26..51，超过基数 36 时
    // from_digit 返回 None 并 panic，创建服务器会随机卡死。
    let rand_part: String = (0..5)
        .map(|_| char::from_digit(rand::random::<u32>() % 36, 36).unwrap_or('0'))
        .collect();
    format!("srv_{:x}_{}", ts, rand_part)
}

pub(crate) fn set_starting(id: &str, starting: bool) {
    let mut guard = running_map();
    if let Some(m) = guard.as_mut()
        && let Some(r) = m.get_mut(id) {
            r.starting = starting;
        }
}

pub(crate) fn list_local_ips() -> Vec<String> {
    let mut ips = Vec::new();
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                ips.push(v4.ip.to_string());
            }
        }
    }
    ips
}

pub(crate) async fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .spawn();
    }
}
