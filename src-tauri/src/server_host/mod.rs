#![allow(unused_imports)]

// server_host.rs — 本地开服模块
// 职责：迁移其中的 11 个 IPC 通道
// 实现：创建开服目录、下载 server.jar、spawn java 子进程、stdin/stdout 控制、模组同步

use std::collections::HashMap;
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

// ============== 路径辅助 ==============






// ============== 全局运行中进程表 ==============





// ============== 索引文件读写 ==============





// ============== 事件广播（传输无关） ==============






// ============== 版本 JSON 解析 ==============






// ============== server.properties / eula.txt ==============



// ============== 启动类型探测 ==============


// ============== 选择 Java ==============


// ============== 模组同步过滤 ==============






// ============== 模组端服务端安装 ==============




// ============== 子进程启动 ==============


// ============== Tauri 命令 ==============

/// 1. 列出所有本地开服配置（传输无关核心）
pub async fn host_list() -> Value {
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
                if starting {
                    "starting"
                } else {
                    "running"
                }
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

    json!({
        "ok": true,
        "servers": servers,
        "root": servers_root().to_string_lossy(),
        "localIps": list_local_ips()
    })
}

/// 1. 列出所有本地开服配置（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_list(_app: AppHandle) -> Value {
    host_list().await
}

/// 创建开服目录 + 下载 server.jar + 写 eula/properties（传输无关核心，供命令与守护进程复用）
pub async fn host_create(
    sink: &Arc<dyn HostSink>,
    version_id: String,
    name: String,
    port: u16,
    options: Option<Value>,
) -> Value {
    let opts = options.unwrap_or(json!({}));
    let clean_name = sanitize_name(&name);
    let max_mem = opts
        .get("maxMem")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(512, 32768) as u32)
        .unwrap_or(2048);
    let online_mode = opts.get("onlineMode").and_then(|v| v.as_bool()).unwrap_or(true);
    let sync_mods = opts.get("syncMods").and_then(|v| v.as_bool()).unwrap_or(true);

    if version_id.is_empty() {
        return json!({ "ok": false, "error": "请选择游戏版本" });
    }
    if load_version_json(&version_id).is_none() {
        return json!({ "ok": false, "error": format!("版本 {} 的 JSON 配置文件不存在，请检查该版本是否安装完整", version_id) });
    }

    let loader_info = detect_client_loader(&version_id);
    let resolved = resolve_server_download(&version_id);
    let loader = loader_info.get("loader").and_then(|v| v.as_str()).unwrap_or("vanilla");
    if resolved.is_none() && loader == "vanilla" {
        return json!({ "ok": false, "error": format!("无法从版本「{}」解析 server.jar 下载地址", version_id) });
    }

    ensure_root();
    let mut idx = load_index();
    let existing_pos = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .iter()
        .position(|s| {
            s.get("versionId").and_then(|v| v.as_str()) == Some(&version_id)
                && s.get("name").and_then(|v| v.as_str()) == Some(&clean_name)
        });

    let id = if let Some(pos) = existing_pos {
        idx["servers"][pos]["id"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(gen_id)
    } else {
        gen_id()
    };
    // id 会直接拼进数据目录：先校验，非法 id 不写入索引
    let Some(dir) = server_dir_of(&id) else {
        return json!({ "ok": false, "error": "服务器 id 非法" });
    };

    let now = now_ms();
    let base_version = resolved
        .as_ref()
        .and_then(|r| r.get("versionId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| loader_info.get("mcVersion").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| version_id.clone());

    let java_major = resolved
        .as_ref()
        .and_then(|r| r.get("javaVersion"))
        .and_then(|jv| jv.get("majorVersion"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or_else(|| resolve_required_java_major(&base_version));

    let entry = json!({
        "id": id,
        "name": clean_name,
        "versionId": version_id,
        "baseVersion": base_version,
        "port": port,
        "maxMem": max_mem,
        "onlineMode": online_mode,
        "loader": loader,
        "loaderVersion": loader_info.get("loaderVersion").cloned().unwrap_or(Value::Null),
        "javaMajor": java_major,
        "createdAt": now,
        "updatedAt": now
    });

    if let Some(arr) = idx.get_mut("servers").and_then(|v| v.as_array_mut()) {
        if let Some(pos) = existing_pos {
            arr[pos] = entry.clone();
        } else {
            arr.push(entry.clone());
        }
    }
    save_index(&idx);

    let _ = std::fs::create_dir_all(&dir);

    // === 模组端分支 ===
    if loader != "vanilla" {
        emit_log(
            sink.as_ref(),
            &id,
            &format!(
                "[VersePC] 检测到加载器: {} {} (MC {})",
                loader,
                loader_info.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or(""),
                loader_info.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("?")
            ),
            "out",
        );

        match install_modded_server(sink, &id, &dir, &loader_info, java_major).await {
            Ok(info) => {
                // 写 eula + properties
                write_eula(&dir);
                write_server_properties(&dir, port, &clean_name, online_mode);

                let launch_kind = info
                    .get("launchKind")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let loader_ver = info
                    .get("loaderVersion")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                // 写 versepc-server.json
                let meta = json!({
                    "id": id,
                    "name": clean_name,
                    "versionId": version_id,
                    "baseVersion": base_version,
                    "loader": loader,
                    "loaderVersion": loader_ver,
                    "launchKind": launch_kind,
                    "port": port,
                    "maxMem": max_mem,
                    "onlineMode": online_mode
                });
                let _ = std::fs::write(dir.join("versepc-server.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());

                // 更新 index
                let mut idx_m = load_index();
                if let Some(arr) = idx_m.get_mut("servers").and_then(|v| v.as_array_mut())
                    && let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id))
                        && let Some(obj) = arr[pos].as_object_mut() {
                            obj.insert("loader".into(), json!(loader));
                            obj.insert("loaderVersion".into(), json!(loader_ver));
                            obj.insert("launchKind".into(), json!(launch_kind));
                            obj.insert("updatedAt".into(), json!(now_ms()));
                        }
                save_index(&idx_m);

                // 同步模组
                let mod_sync = if sync_mods {
                    let src_mods = resolve_client_mods_dir(&version_id);
                    let dest_mods = dir.join("mods");
                    let r = sync_client_mods_to_server(sink.as_ref(), &id, &src_mods, &dest_mods).await;
                    Some(r)
                } else {
                    None
                };

                emit_status(
                    sink.as_ref(),
                    &id,
                    "ready",
                    json!({
                        "progress": 100,
                        "message": format!("{} 服务端已就绪", loader),
                        "path": dir.to_string_lossy(),
                        "stage": "ready"
                    }),
                );

                return json!({
                    "ok": true,
                    "server": {
                        "id": id,
                        "name": clean_name,
                        "versionId": version_id,
                        "baseVersion": base_version,
                        "loader": loader,
                        "loaderVersion": loader_ver,
                        "launchKind": launch_kind,
                        "dir": dir.to_string_lossy(),
                        "port": port,
                        "maxMem": max_mem,
                        "onlineMode": online_mode
                    },
                    "modSync": mod_sync
                });
            }
            Err(e) => {
                emit_log(sink.as_ref(), &id, &format!("[VersePC] 模组端安装失败: {}", e), "err");
                emit_status(sink.as_ref(), &id, "error", json!({ "message": format!("模组端安装失败: {}", e), "stage": "error" }));
                return json!({ "ok": false, "error": format!("{} 服务端安装失败: {}", loader, e) });
            }
        }
    }

    // === 原版分支 ===
    let resolved = match resolved {
        Some(r) => r,
        None => return json!({ "ok": false, "error": "无法解析 server.jar 下载地址" }),
    };

    let server = resolved.get("server").cloned().unwrap_or(Value::Null);
    let server_url = server.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let server_sha1 = server.get("sha1").and_then(|v| v.as_str());
    let server_size = server.get("size").and_then(|v| v.as_u64());
    let version_id_in_json = resolved.get("versionId").and_then(|v| v.as_str()).unwrap_or(&version_id);

    let jar_path = dir.join("server.jar");
    let need_download = match std::fs::metadata(&jar_path) {
        Ok(meta) => {
            // 大小不匹配则需重新下载
            server_size.map(|s| s != meta.len()).unwrap_or(false)
        }
        Err(_) => true,
    };

    if need_download {
        emit_status(sink.as_ref(), &id, "downloading", json!({ "progress": 0, "message": "正在下载 server.jar..." }));
        emit_log(sink.as_ref(), &id, &format!("[VersePC] 下载 server.jar ← {}", server_url), "out");

        if let Err(e) = download_with_mirror(server_url, &jar_path, server_sha1, server_size, "auto", 600, None).await {
            return json!({ "ok": false, "error": format!("下载 server.jar 失败: {}", e) });
        }
        emit_log(sink.as_ref(), &id, "[VersePC] server.jar 就绪", "out");
    } else {
        emit_log(sink.as_ref(), &id, "[VersePC] 复用已有 server.jar", "out");
    }

    write_eula(&dir);
    write_server_properties(&dir, port, &clean_name, online_mode);

    // 元数据
    let meta = json!({
        "id": id,
        "name": clean_name,
        "versionId": version_id,
        "baseVersion": version_id_in_json,
        "port": port,
        "maxMem": max_mem,
        "onlineMode": online_mode,
        "serverSha1": server_sha1
    });
    let _ = std::fs::write(dir.join("versepc-server.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());

    // 更新 index
    let mut idx2 = load_index();
    if let Some(arr) = idx2.get_mut("servers").and_then(|v| v.as_array_mut())
        && let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id))
            && let Some(obj) = arr[pos].as_object_mut() {
                obj.insert("updatedAt".into(), json!(now_ms()));
            }
    save_index(&idx2);

    emit_status(sink.as_ref(), &id, "ready", json!({ "message": "服务端已准备就绪", "path": dir.to_string_lossy() }));

    json!({
        "ok": true,
        "server": {
            "id": id,
            "name": clean_name,
            "versionId": version_id,
            "baseVersion": version_id_in_json,
            "dir": dir.to_string_lossy(),
            "port": port,
            "maxMem": max_mem,
            "onlineMode": online_mode
        }
    })
}

/// 2. 创建开服目录 + 下载 server.jar + 写 eula/properties（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_create(
    app: AppHandle,
    version_id: String,
    name: String,
    port: u16,
    options: Option<Value>,
) -> Value {
    let sink: Arc<dyn HostSink> = Arc::new(TauriHostSink(app));
    host_create(&sink, version_id, name, port, options).await
}

/// 3. 启动服务端（传输无关核心，供命令与守护进程复用）
pub async fn host_start(sink: &Arc<dyn HostSink>, server_id: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }
    if is_running(&server_id) {
        return json!({ "ok": false, "error": "服务器已在运行" });
    }

    let idx = load_index();
    let entry = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id)))
        .cloned();
    let entry = match entry {
        Some(e) => e,
        None => return json!({ "ok": false, "error": "服务器不存在" }),
    };
    // 目录必须位于 servers_root 之下，防止索引被篡改后启动任意目录
    let Some(dir) = server_dir_from_entry(&entry, &server_id) else {
        return json!({ "ok": false, "error": "服务器目录非法" });
    };

    let launch_kind = entry
        .get("launchKind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| detect_launch_kind(&dir));

    let port = entry.get("port").and_then(|v| v.as_u64()).map(|v| v as u16).unwrap_or(25565);
    let max_mem = entry.get("maxMem").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(2048);
    let min_mem = (max_mem / 4).max(256).min(max_mem);
    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let online_mode = entry.get("onlineMode").and_then(|v| v.as_bool()).unwrap_or(true);
    let version_id = entry.get("versionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let base_version = entry.get("baseVersion").and_then(|v| v.as_str()).unwrap_or(&version_id).to_string();

    write_server_properties(&dir, port, &name, online_mode);
    write_eula(&dir);

    let required_major = entry
        .get("javaMajor")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or_else(|| resolve_required_java_major(&base_version));
    emit_log(sink.as_ref(), &server_id, &format!("[VersePC] 需要 Java {}+（{}）", required_major, base_version), "out");
    let java_path = match select_java_for_major(required_major) {
        Ok(p) => p,
        Err(e) => return json!({ "ok": false, "error": e }),
    };

    let mut cmd = if launch_kind == "run-script" {
        let script = if cfg!(target_os = "windows") { "run.bat" } else { "run.sh" };
        emit_log(sink.as_ref(), &server_id, &format!("[VersePC] 使用启动脚本: {}", script), "out");
        emit_status(sink.as_ref(), &server_id, "starting", json!({ "message": "正在启动（run 脚本）...", "javaPath": java_path, "port": port }));
        let mut c = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd.exe");
            c.args(["/c", "run.bat", "nogui"]);
            c
        } else {
            let mut c = Command::new("bash");
            c.args(["run.sh", "nogui"]);
            c
        };
        c.current_dir(&dir);
        c
    } else {
        let args = build_launch_args(&launch_kind, &dir, max_mem, min_mem);
        emit_log(sink.as_ref(), &server_id, &format!("[VersePC] {} {}", java_path, args.join(" ")), "out");
        let kind_msg = if launch_kind == "fabric-launch" {
            "Fabric"
        } else if launch_kind.starts_with("forge-jar:") {
            "Forge"
        } else {
            ""
        };
        emit_status(sink.as_ref(), &server_id, "starting", json!({ "message": format!("正在启动{}...", kind_msg), "javaPath": java_path, "port": port }));
        let mut c = Command::new(&java_path);
        c.args(&args);
        c.current_dir(&dir);
        c
    };

    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("JAVA_TOOL_OPTIONS", "");
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            emit_log(sink.as_ref(), &server_id, &format!("[VersePC] 启动失败: {}", e), "err");
            emit_status(sink.as_ref(), &server_id, "error", json!({ "message": e.to_string() }));
            return json!({ "ok": false, "error": e.to_string() });
        }
    };

    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdin_arc = Arc::new(tokio::sync::Mutex::new(stdin));
    let (exit_tx, exit_rx) = oneshot::channel::<()>();

    let running = RunningServer {
        stdin: stdin_arc.clone(),
        exit_rx: Mutex::new(Some(exit_rx)),
        pid,
        dir: dir.clone(),
        name: name.clone(),
        port,
        starting: true,
    };
    {
        let mut guard = running_map();
        if let Some(m) = guard.as_mut() {
            m.insert(server_id.clone(), running);
        }
    }

    emit_log(sink.as_ref(), &server_id, &format!("[VersePC] cwd={} launchKind={}", dir.display(), launch_kind), "out");

    // stdout 异步读取
    if let Some(stdout) = stdout {
        let app_c = sink.clone();
        let sid = server_id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(app_c.as_ref(), &sid, &line, "out");
                if line.contains("Done (") || line.contains("For help, type \"help\"") {
                    set_starting(&sid, false);
                    emit_status(app_c.as_ref(), &sid, "running", json!({
                        "message": "服务器已就绪",
                        "port": port,
                        "localIps": list_local_ips()
                    }));
                }
                if line.to_lowercase().contains("unsupportedclassversionerror") || line.contains("has been compiled by a more recent version") {
                    emit_log(app_c.as_ref(), &sid, &format!("[VersePC] Java 版本过低：该服务端需要 Java {}+。请到「Java 管理」页下载后重启服务端。", required_major), "err");
                }
            }
        });
    }

    // stderr 异步读取
    if let Some(stderr) = stderr {
        let app_c = sink.clone();
        let sid = server_id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(app_c.as_ref(), &sid, &line, "err");
            }
        });
    }

    // 进程退出监听
    let app_c = sink.clone();
    let sid_c = server_id.clone();
    tokio::spawn(async move {
        let _ = child.wait().await;
        let _ = exit_tx.send(());
        // 从运行表中移除
        {
            let mut guard = running_map();
            if let Some(m) = guard.as_mut() {
                m.remove(&sid_c);
            }
        }
        emit_log(app_c.as_ref(), &sid_c, "[VersePC] 进程退出", "out");
        emit_status(app_c.as_ref(), &sid_c, "stopped", json!({}));
    });

    // 更新 index: lastStartedAt
    let mut idx2 = load_index();
    if let Some(arr) = idx2.get_mut("servers").and_then(|v| v.as_array_mut())
        && let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id))
            && let Some(obj) = arr[pos].as_object_mut() {
                obj.insert("lastStartedAt".into(), json!(now_ms()));
                obj.insert("port".into(), json!(port));
                obj.insert("maxMem".into(), json!(max_mem));
            }
    save_index(&idx2);

    json!({
        "ok": true,
        "pid": pid,
        "port": port,
        "localIps": list_local_ips(),
        "javaPath": java_path
    })
}

/// 3. 启动服务端（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_start(app: AppHandle, server_id: String) -> Value {
    let sink: Arc<dyn HostSink> = Arc::new(TauriHostSink(app));
    host_start(&sink, server_id).await
}

/// 4. 停止服务端（写 stop 到 stdin，等 15s 再 SIGKILL）（传输无关核心）
pub async fn host_stop(sink: &Arc<dyn HostSink>, server_id: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }

    // 先从运行表中取出 stdin_arc 和 exit_rx，避免 std::sync::MutexGuard 跨越 await 点
    let (stdin_arc_opt, exit_rx_opt) = {
        let mut guard = running_map();
        match guard.as_mut().and_then(|m| m.get_mut(&server_id)) {
            Some(r) => {
                let stdin_arc = r.stdin.clone();
                let exit_rx = r.exit_rx.lock().unwrap().take();
                (Some(stdin_arc), exit_rx)
            }
            None => (None, None),
        }
    };

    let exit_rx = match exit_rx_opt {
        Some(rx) => rx,
        None => return json!({ "ok": true, "alreadyStopped": true }),
    };
    let stdin_arc = match stdin_arc_opt {
        Some(s) => s,
        None => return json!({ "ok": true, "alreadyStopped": true }),
    };

    // 写 stop\n 到 stdin（tokio Mutex guard 是 Send 的，可跨 await）
    {
        let mut stdin_guard = stdin_arc.lock().await;
        if let Some(stdin) = stdin_guard.as_mut() {
            let _ = stdin.write_all(b"stop\n").await;
            let _ = stdin.flush().await;
        }
    }
    emit_status(sink.as_ref(), &server_id, "stopping", json!({ "message": "正在关闭服务器..." }));

    // 等最多 15s 优雅退出
    let exited = tokio::time::timeout(Duration::from_secs(15), exit_rx).await.is_ok();

    if !exited {
        emit_log(sink.as_ref(), &server_id, "[VersePC] 优雅关闭超时，强制结束进程", "err");
        // 强制 kill
        let pid_opt = {
            let guard = running_map();
            guard
                .as_ref()
                .and_then(|m| m.get(&server_id))
                .and_then(|r| r.pid)
        };
        if let Some(pid) = pid_opt {
            kill_process_tree(pid).await;
        }
        // 从运行表移除
        {
            let mut guard = running_map();
            if let Some(m) = guard.as_mut() {
                m.remove(&server_id);
            }
        }
        emit_status(sink.as_ref(), &server_id, "stopped", json!({ "forced": true }));
    }

    json!({ "ok": true, "forced": !exited })
}

/// 4. 停止服务端（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_stop(app: AppHandle, server_id: String) -> Value {
    let sink: Arc<dyn HostSink> = Arc::new(TauriHostSink(app));
    host_stop(&sink, server_id).await
}

/// 5. 向服务端 stdin 发送命令（传输无关核心）
pub async fn host_command(sink: &Arc<dyn HostSink>, server_id: String, cmd: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }
    let cmd_clean = cmd.replace(['\r', '\n'], "");
    if cmd_clean.is_empty() {
        return json!({ "ok": true });
    }

    let stdin_arc = {
        let guard = running_map();
        match guard.as_ref().and_then(|m| m.get(&server_id)) {
            Some(r) => r.stdin.clone(),
            None => return json!({ "ok": false, "error": "服务器未运行" }),
        }
    };

    emit_log(sink.as_ref(), &server_id, &format!("> {}", cmd_clean), "cmd");
    let mut guard = stdin_arc.lock().await;
    if let Some(stdin) = guard.as_mut() {
        let _ = stdin.write_all(format!("{}\n", cmd_clean).as_bytes()).await;
        let _ = stdin.flush().await;
        json!({ "ok": true })
    } else {
        json!({ "ok": false, "error": "服务器 stdin 已关闭" })
    }
}

/// 5. 向服务端 stdin 发送命令（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_command(app: AppHandle, server_id: String, cmd: String) -> Value {
    let sink: Arc<dyn HostSink> = Arc::new(TauriHostSink(app));
    host_command(&sink, server_id, cmd).await
}

/// 6. 查询运行状态（传输无关核心）
pub async fn host_status(server_id: Option<String>) -> Value {
    if let Some(id) = server_id {
        if id.is_empty() {
            return status_all();
        }
        let idx = load_index();
        let entry = idx
            .get("servers")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id)))
            .cloned();
        let entry = match entry {
            Some(e) => e,
            None => return json!({ "ok": false, "error": "not found" }),
        };
        let port = entry.get("port").and_then(|v| v.as_u64()).map(|v| v as u16).unwrap_or(25565);
        let running = is_running(&id);
        let (status, pid) = {
            let guard = running_map();
            guard
                .as_ref()
                .and_then(|m| m.get(&id))
                .map(|r| {
                    let status = if r.starting { "starting" } else { "running" };
                    (status, r.pid)
                })
                .unwrap_or(("stopped", None))
        };
        return json!({
            "ok": true,
            "server": entry,
            "running": running,
            "status": status,
            "port": if running { port } else { entry.get("port").and_then(|v| v.as_u64()).map(|v| v as u16).unwrap_or(25565) },
            "localIps": list_local_ips(),
            "pid": pid
        });
    }
    status_all()
}

/// 6. 查询运行状态（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_status(_app: AppHandle, server_id: Option<String>) -> Value {
    host_status(server_id).await
}


/// 7. 删除服务端（传输无关核心）
pub async fn host_delete(server_id: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }
    if is_running(&server_id) {
        return json!({ "ok": false, "error": "请先停止服务器再删除" });
    }
    let mut idx = load_index();
    // 先查索引并校验目录：查不到就拒绝，不能再用原始 id 拼兜底路径（审计 P1-2）
    let entry = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id)))
        .cloned();
    let Some(entry) = entry else {
        return json!({ "ok": false, "error": "服务器不存在" });
    };
    let Some(dir) = server_dir_from_entry(&entry, &server_id) else {
        return json!({ "ok": false, "error": "服务器目录非法" });
    };

    if let Some(arr) = idx.get_mut("servers").and_then(|v| v.as_array_mut()) {
        arr.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(&server_id));
    }
    save_index(&idx);

    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    json!({ "ok": true })
}

/// 7. 删除服务端（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_delete(_app: AppHandle, server_id: String) -> Value {
    host_delete(server_id).await
}

/// 8. 打开服务端目录（传输无关核心）
pub async fn host_open_dir(server_id: Option<String>) -> Value {
    ensure_root();
    let dir = match server_id {
        Some(id) if !id.is_empty() => {
            let idx = load_index();
            let entry = idx
                .get("servers")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id)))
                .cloned();
            let Some(entry) = entry else {
                return json!({ "ok": false, "error": "服务器不存在" });
            };
            match server_dir_from_entry(&entry, &id) {
                Some(d) => d,
                None => return json!({ "ok": false, "error": "服务器目录非法" }),
            }
        }
        _ => servers_root(),
    };
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    let _ = open::that(&dir);
    json!({ "ok": true, "path": dir.to_string_lossy() })
}

/// 8. 打开服务端目录（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_open_dir(_app: AppHandle, server_id: Option<String>) -> Value {
    host_open_dir(server_id).await
}

/// 9. 沿 inheritsFrom 链查 downloads.server（传输无关核心）
pub async fn host_resolve_version(version_id: String) -> Value {
    match resolve_server_download(&version_id) {
        Some(r) => json!({ "ok": true, "versionId": r.get("versionId"), "server": r.get("server"), "javaVersion": r.get("javaVersion") }),
        None => json!({ "ok": false, "error": "无法解析 server 下载" }),
    }
}

/// 9. 沿 inheritsFrom 链查 downloads.server（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_resolve_version(_app: AppHandle, version_id: String) -> Value {
    host_resolve_version(version_id).await
}

/// 10. 识别客户端加载器（传输无关核心）
pub async fn host_detect_loader(version_id: String) -> Value {
    let info = detect_client_loader(&version_id);
    json!({
        "ok": true,
        "loader": info.get("loader"),
        "loaderVersion": info.get("loaderVersion"),
        "mcVersion": info.get("mcVersion"),
        "chainIds": info.get("chainIds")
    })
}

/// 10. 识别客户端加载器（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_detect_loader(_app: AppHandle, version_id: String) -> Value {
    host_detect_loader(version_id).await
}

/// 11. 同步客户端 mods 到服务端（传输无关核心）
pub async fn host_sync_mods(sink: &Arc<dyn HostSink>, server_id: String, client_version_id: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }
    let idx = load_index();
    let entry = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id)))
        .cloned();
    let entry = match entry {
        Some(e) => e,
        None => return json!({ "ok": false, "error": "服务器不存在" }),
    };

    let Some(dir) = server_dir_from_entry(&entry, &server_id) else {
        return json!({ "ok": false, "error": "服务器目录非法" });
    };

    let src_mods = resolve_client_mods_dir(&client_version_id);
    let dest_mods = dir.join("mods");
    let result = sync_client_mods_to_server(sink.as_ref(), &server_id, &src_mods, &dest_mods).await;
    let message = result.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    emit_status(sink.as_ref(), &server_id, "ready", json!({ "progress": 100, "message": message, "stage": "ready" }));
    let mut response = json!({ "ok": true });
    if let (Some(dst), Some(src)) = (response.as_object_mut(), result.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    response
}

/// 11. 同步客户端 mods 到服务端（Tauri 命令入口）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_sync_mods(app: AppHandle, server_id: String, client_version_id: String) -> Value {
    let sink: Arc<dyn HostSink> = Arc::new(TauriHostSink(app));
    host_sync_mods(&sink, server_id, client_version_id).await
}

// ============== 辅助 ==============







// 子模块声明与公共导出
mod index;
pub(crate) use index::*;
mod process;
pub(crate) use process::*;
mod sink;
pub(crate) use sink::*;
mod resolve;
pub(crate) use resolve::*;
mod install;
pub(crate) use install::*;
mod mods;
pub(crate) use mods::*;
