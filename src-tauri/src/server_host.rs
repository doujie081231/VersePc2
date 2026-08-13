// server_host.rs — 本地开服模块
// 职责：迁移原 Electron 项目 main/server-host.js 的 11 个 IPC 通道
// 实现：创建开服目录、下载 server.jar、spawn java 子进程、stdin/stdout 控制、模组同步

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

// ============== 路径辅助 ==============

fn servers_root() -> PathBuf {
    storage::resolve_data_dir().join("servers")
}

fn index_file() -> PathBuf {
    servers_root().join("index.json")
}

fn versions_dir() -> PathBuf {
    storage::resolve_data_dir().join("versions")
}

fn server_dir_of(id: &str) -> PathBuf {
    servers_root().join(id)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ============== 全局运行中进程表 ==============

struct RunningServer {
    stdin: Arc<tokio::sync::Mutex<Option<ChildStdin>>>,
    exit_rx: Mutex<Option<oneshot::Receiver<()>>>,
    pid: Option<u32>,
    dir: PathBuf,
    name: String,
    port: u16,
    starting: bool,
}

static RUNNING: Mutex<Option<HashMap<String, RunningServer>>> = Mutex::new(None);

fn running_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, RunningServer>>> {
    let mut guard = RUNNING.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

fn is_running(id: &str) -> bool {
    let guard = running_map();
    guard.as_ref().map(|m| m.contains_key(id)).unwrap_or(false)
}

// ============== 索引文件读写 ==============

fn ensure_root() {
    let _ = std::fs::create_dir_all(servers_root());
    let idx = index_file();
    if !idx.exists() {
        let _ = std::fs::write(&idx, "{\"servers\":[]}");
    }
}

fn load_index() -> Value {
    ensure_root();
    let idx = index_file();
    match std::fs::read_to_string(&idx) {
        Ok(content) => {
            let content = content.trim_start_matches('\u{FEFF}');
            match serde_json::from_str::<Value>(content) {
                Ok(v) if v.get("servers").and_then(|s| s.as_array()).is_some() => v,
                _ => json!({ "servers": [] }),
            }
        }
        Err(_) => json!({ "servers": [] }),
    }
}

fn save_index(idx: &Value) {
    ensure_root();
    let _ = std::fs::write(index_file(), serde_json::to_string_pretty(idx).unwrap_or_default());
}

fn read_json_safe(path: &std::path::Path) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim_start_matches('\u{FEFF}').to_string())
        .and_then(|s| serde_json::from_str(&s).ok())
}

// ============== 事件广播 ==============

fn emit_log(app: &AppHandle, id: &str, line: &str, stream: &str) {
    let _ = app.emit(
        "server-host:log",
        json!({
            "id": id,
            "line": line.replace('\r', ""),
            "stream": stream,
            "ts": now_ms()
        }),
    );
}

fn emit_status(app: &AppHandle, id: &str, status: &str, extra: Value) {
    let mut payload = json!({
        "id": id,
        "status": status,
        "ts": now_ms()
    });
    if let (Some(dst), Some(src)) = (payload.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    let _ = app.emit("server-host:status", payload);
}

// ============== 版本 JSON 解析 ==============

fn load_version_json(version_id: &str) -> Option<Value> {
    if version_id.is_empty() {
        return None;
    }
    let direct = versions_dir().join(version_id).join(format!("{}.json", version_id));
    if let Some(v) = read_json_safe(&direct) {
        return Some(v);
    }
    // 扫描：目录名与 id 不一致的情况
    if let Ok(entries) = std::fs::read_dir(versions_dir()) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };
            let cand = versions_dir().join(&name).join(format!("{}.json", name));
            if let Some(j) = read_json_safe(&cand) {
                let j_id = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if j_id == version_id || name == version_id {
                    return Some(j);
                }
            }
        }
    }
    None
}

/// 沿 inheritsFrom 链查找带 downloads.server 的版本 JSON
fn resolve_server_download(version_id: &str) -> Option<Value> {
    let mut visited = std::collections::HashSet::new();
    let mut cur = version_id.to_string();
    let mut depth = 0u32;
    while !cur.is_empty() && depth < 12 && visited.insert(cur.clone()) {
        depth += 1;
        let data = match load_version_json(&cur) {
            Some(d) => d,
            None => break,
        };
        if let Some(server) = data
            .get("downloads")
            .and_then(|d| d.get("server"))
        {
            if server.get("url").and_then(|u| u.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
                let id = data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&cur)
                    .to_string();
                return Some(json!({
                    "versionId": id,
                    "server": server,
                    "javaVersion": data.get("javaVersion").cloned().unwrap_or(Value::Null),
                    "inheritsFrom": data.get("inheritsFrom").cloned().unwrap_or(Value::Null),
                    "chain": visited.iter().cloned().collect::<Vec<_>>()
                }));
            }
        }
        cur = data
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
    }
    None
}

/// 识别客户端加载器类型与版本号
fn detect_client_loader(version_id: &str) -> Value {
    let data = load_version_json(version_id);
    let mut chain: Vec<Value> = Vec::new();
    let mut cur = data.clone();
    let mut depth = 0u32;
    while let Some(j) = cur {
        chain.push(j.clone());
        depth += 1;
        if depth >= 12 {
            break;
        }
        let parent_id = j.get("inheritsFrom").and_then(|v| v.as_str()).map(|s| s.to_string());
        cur = parent_id.and_then(|p| load_version_json(&p));
    }

    let top = data.clone().unwrap_or(json!({}));
    let mut all_libs: Vec<Value> = Vec::new();
    for j in &chain {
        if let Some(libs) = j.get("libraries").and_then(|v| v.as_array()) {
            all_libs.extend(libs.clone());
        }
    }
    let mut lib_str = String::new();
    for l in &all_libs {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            lib_str.push_str(name);
            lib_str.push(' ');
        }
    }
    let main_class = top.get("mainClass").and_then(|v| v.as_str()).unwrap_or("");
    lib_str.push_str(&format!("\"{}\"", main_class));

    // 从 arguments.game 抽取 fml.* 版本
    let mut fml_neo: Option<String> = None;
    let mut fml_forge: Option<String> = None;
    let mut fml_mc: Option<String> = None;
    for j in &chain {
        if let Some(args) = j.get("arguments").and_then(|a| a.get("game")).and_then(|g| g.as_array()) {
            for i in 0..args.len() {
                let v = args[i].as_str().unwrap_or("");
                if v == "--fml.neoForgeVersion" {
                    if let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_neo = Some(next.to_string());
                    }
                } else if v == "--fml.forgeVersion" {
                    if let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_forge = Some(next.to_string());
                    }
                } else if v == "--fml.mcVersion" {
                    if let Some(next) = args.get(i + 1).and_then(|v| v.as_str()) {
                        fml_mc = Some(next.to_string());
                    }
                }
            }
            if fml_neo.is_some() || fml_forge.is_some() {
                break;
            }
        }
    }

    let id_lower = version_id.to_lowercase();

    let mut loader = "vanilla".to_string();
    let mut loader_version: Option<String> = None;
    let mut mc_version: Option<String> = if fml_mc.is_some() {
        fml_mc.clone()
    } else {
        None
    };

    // 从链中取带 downloads.server 的版本 id 作为 mc_version 兜底
    for j in chain.iter().rev() {
        if j.get("downloads").and_then(|d| d.get("server")).is_some() {
            let dj = j.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if mc_version.as_deref().map(|s| !s.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(true)
                && !dj.is_empty()
            {
                mc_version = Some(dj.to_string());
            }
            break;
        }
    }

    // 兜底：从 versionId 正则提取
    if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true) {
        let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?(?:-pre\d+|-rc\d+)?)").ok();
        if let Some(re) = re {
            if let Some(captures) = re.captures(version_id) {
                mc_version = Some(captures.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
            }
        }
        if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true) {
            if let Some(fmc) = &fml_mc {
                mc_version = Some(fmc.clone());
            }
        }
        if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true) {
            if let Some(ff) = &fml_forge {
                let re = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)-").ok();
                if let Some(re) = re {
                    if let Some(c) = re.captures(ff) {
                        mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                    }
                }
            }
        }
    }

    // NeoForge 检测
    let neo_re = regex::Regex::new(r#"(?i)net\.neoforged:(?:neoforge|forge):([^:\s""]+)"#).ok();
    let neo_match = neo_re
        .as_ref()
        .and_then(|r| r.captures(&lib_str))
        .or_else(|| {
            regex::Regex::new(r"(?i)neoforge[-_]?(\d+\.\d+\.\d+[^\s/]*)")
                .ok()
                .and_then(|r| r.captures(&id_lower))
        })
        .or_else(|| {
            regex::Regex::new(r"(?i)NeoForge[-_ ]?(\d[\d.\-\w]*)")
                .ok()
                .and_then(|r| r.captures(version_id))
        });

    let is_neo = neo_match.is_some()
        || fml_neo.is_some()
        || lib_str.to_lowercase().contains("neoforge")
        || lib_str.to_lowercase().contains("fancymodloader")
        || id_lower.contains("neoforge");

    if is_neo {
        loader = "neoforge".to_string();
        if let Some(v) = &fml_neo {
            loader_version = Some(v.clone());
        } else if let Some(c) = &neo_match {
            let mut v = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            if v.to_lowercase().contains("neoforge") {
                if let Some(re) = regex::Regex::new(r"(\d+\.\d+\.\d+[\w.-]*)").ok() {
                    if let Some(c2) = re.captures(&v) {
                        v = c2.get(1).map(|m| m.as_str().to_string()).unwrap_or(v);
                    }
                }
            }
            loader_version = Some(v);
        }
    } else {
        // Forge 检测
        let forge_re1 = regex::Regex::new(r#"(?i)net\.minecraftforge:forge:([^:\s""]+)"#).ok();
        let forge_re2 = regex::Regex::new(r#"(?i)net\.minecraftforge:fmlloader:([^:\s""]+)"#).ok();
        let forge_match = forge_re1
            .as_ref()
            .and_then(|r| r.captures(&lib_str))
            .or_else(|| forge_re2.as_ref().and_then(|r| r.captures(&lib_str)));
        let id_forge = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)-[Ff]orge-(\d+\.\d+[\w.-]*)")
            .ok()
            .and_then(|r| r.captures(version_id));

        let is_forge = id_forge.is_some()
            || forge_match.is_some()
            || fml_forge.is_some()
            || lib_str.to_lowercase().contains("minecraftforge")
            || lib_str.to_lowercase().contains("modlauncher")
            || (id_lower.contains("forge") && !id_lower.contains("fabric"));

        if is_forge {
            loader = "forge".to_string();
            if let Some(c) = &id_forge {
                if mc_version.is_none() || !is_valid_mc_ver(mc_version.as_deref().unwrap_or("")) {
                    mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                }
                loader_version = Some(format!(
                    "{}-{}",
                    c.get(1).map(|m| m.as_str()).unwrap_or(""),
                    c.get(2).map(|m| m.as_str()).unwrap_or("")
                ));
            } else if let Some(c) = &forge_match {
                let v = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                loader_version = Some(v.clone());
                if mc_version.is_none() {
                    if let Some(re) = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)-").ok() {
                        if let Some(c2) = re.captures(&v) {
                            mc_version = Some(c2.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                        }
                    }
                }
            } else if let Some(v) = &fml_forge {
                loader_version = Some(v.clone());
                if mc_version.is_none() {
                    if let Some(fmc) = &fml_mc {
                        mc_version = Some(fmc.clone());
                    }
                }
            }
        } else {
            // Fabric 检测
            let fab_re = regex::Regex::new(r#"(?i)net\.fabricmc:fabric-loader:([^:\s""]+)"#).ok();
            let fab_match = fab_re.as_ref().and_then(|r| r.captures(&lib_str));
            let id_fab = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)-[Ff]abric[-_ ]?(\d+\.\d+\.\d+)")
                .ok()
                .and_then(|r| r.captures(version_id))
                .or_else(|| {
                    regex::Regex::new(r"(?i)Fabric[-_ ](\d+\.\d+\.\d+)")
                        .ok()
                        .and_then(|r| r.captures(version_id))
                });

            if fab_match.is_some()
                || id_fab.is_some()
                || lib_str.to_lowercase().contains("fabric-loader")
                || lib_str.to_lowercase().contains("net.fabricmc")
                || id_lower.contains("fabric")
            {
                loader = "fabric".to_string();
                if let Some(c) = &id_fab {
                    if mc_version.is_none() {
                        mc_version = c.get(1).map(|m| m.as_str().to_string());
                    }
                    loader_version = c.get(2).map(|m| m.as_str().to_string());
                } else if let Some(c) = &fab_match {
                    loader_version = c.get(1).map(|m| m.as_str().to_string());
                }
            }
        }
    }

    // Fabric: 从 intermediary 推导 MC 版本
    if loader == "fabric"
        && mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
    {
        for l in &all_libs {
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.to_lowercase().contains("net.fabricmc:intermediary:") {
                if let Some(re) = regex::Regex::new(r"intermediary:(\d+\.\d+(?:\.\d+)?)").ok() {
                    if let Some(c) = re.captures(name) {
                        mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
                        break;
                    }
                }
            }
        }
    }

    // Forge loaderVersion 拼接 mc 版本
    if loader == "forge"
        && loader_version.is_some()
        && mc_version.is_some()
        && !loader_version.as_deref().unwrap_or("").contains(mc_version.as_deref().unwrap_or(""))
        && is_valid_mc_ver(loader_version.as_deref().unwrap_or(""))
    {
        let lv = loader_version.clone().unwrap_or_default();
        let re = regex::Regex::new(r"^\d+\.\d+(\.\d+)?-").ok();
        if re.and_then(|r| r.captures(&lv)).is_none() {
            loader_version = Some(format!("{}-{}", mc_version.as_deref().unwrap_or(""), lv));
        }
    }

    // 从 loaderVersion 反推 mcVersion（仅 forge/neoforge）
    if mc_version.as_deref().map(|s| !is_valid_mc_ver(s)).unwrap_or(true)
        && loader_version.is_some()
        && loader != "fabric"
    {
        let lv = loader_version.clone().unwrap_or_default();
        if let Some(re) = regex::Regex::new(r"^(\d+\.\d+(?:\.\d+)?)(?:-|$)").ok() {
            if let Some(c) = re.captures(&lv) {
                mc_version = Some(c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default());
            }
        }
    }

    let mc_final = mc_version
        .filter(|s| is_valid_mc_ver(s))
        .or_else(|| resolve_server_download(version_id).and_then(|r| r.get("versionId").and_then(|v| v.as_str()).map(|s| s.to_string())))
        .unwrap_or_else(|| version_id.to_string());

    let chain_ids: Vec<Value> = chain
        .iter()
        .filter_map(|j| j.get("id").and_then(|v| v.as_str()).map(|s| json!(s)))
        .collect();

    json!({
        "loader": loader,
        "loaderVersion": loader_version,
        "mcVersion": mc_final,
        "chainIds": chain_ids
    })
}

fn is_valid_mc_ver(s: &str) -> bool {
    if let Ok(re) = regex::Regex::new(r"^\d+\.\d+") {
        re.is_match(s)
    } else {
        false
    }
}

/// 解析版本所需 Java 主版本（沿 inheritsFrom 链找 javaVersion.majorVersion）
fn resolve_required_java_major(version_id: &str) -> u32 {
    let mut visited = std::collections::HashSet::new();
    let mut cur = version_id.to_string();
    let mut depth = 0u32;
    while !cur.is_empty() && depth < 12 && visited.insert(cur.clone()) {
        depth += 1;
        if let Some(j) = load_version_json(&cur) {
            if let Some(mv) = j
                .get("javaVersion")
                .and_then(|jv| jv.get("majorVersion"))
                .and_then(|v| v.as_u64())
            {
                return mv as u32;
            }
            cur = j
                .get("inheritsFrom")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
        } else {
            break;
        }
    }
    // 兜底：按 MC 版本映射
    let re = regex::Regex::new(r"(\d+)(?:\.(\d+))?").ok();
    if let Some(re) = re {
        if let Some(c) = re.captures(version_id) {
            let major: u32 = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let minor: u32 = c
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            if major >= 26 {
                return 25;
            }
            if major == 1 && minor >= 21 {
                return 21;
            }
            if major == 1 && minor == 20 {
                let patch: u32 = regex::Regex::new(r"\.(\d+)(?:\D|$)")
                    .ok()
                    .and_then(|re| re.captures(version_id))
                    .and_then(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
                    .unwrap_or(0);
                return if patch >= 5 { 21 } else { 17 };
            }
            if major == 1 && minor >= 18 {
                return 17;
            }
        }
    }
    17
}

// ============== server.properties / eula.txt ==============

fn write_eula(dir: &std::path::Path) {
    let p = dir.join("eula.txt");
    let _ = std::fs::write(p, "# Generated by VersePC Server Host\neula=true\n");
}

fn write_server_properties(dir: &std::path::Path, port: u16, motd: &str, online_mode: bool) {
    let p = dir.join("server.properties");
    let mut lines: Vec<String> = if p.exists() {
        std::fs::read_to_string(&p)
            .unwrap_or_default()
            .lines()
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![
            "enable-jmx-monitoring=false".into(),
            "rcon.port=25575".into(),
            "level-seed=".into(),
            "gamemode=survival".into(),
            "enable-command-block=false".into(),
            "enable-query=false".into(),
            "generator-settings={}".into(),
            "level-name=world".into(),
            "motd=A Minecraft Server".into(),
            "query.port=25565".into(),
            "pvp=true".into(),
            "generate-structures=true".into(),
            "difficulty=easy".into(),
            "network-compression-threshold=256".into(),
            "max-tick-time=60000".into(),
            "require-resource-pack=false".into(),
            "max-players=20".into(),
            "use-native-transport=true".into(),
            "online-mode=true".into(),
            "enable-status=true".into(),
            "allow-flight=false".into(),
            "broadcast-rcon-to-ops=true".into(),
            "view-distance=10".into(),
            "server-ip=".into(),
            "allow-nether=true".into(),
            "server-port=25565".into(),
            "enable-rcon=false".into(),
            "sync-chunk-writes=true".into(),
            "op-permission-level=4".into(),
            "prevent-proxy-connections=false".into(),
            "hide-online-players=false".into(),
            "resource-pack=".into(),
            "entity-broadcast-range-percentage=100".into(),
            "simulation-distance=10".into(),
            "player-idle-timeout=0".into(),
            "force-gamemode=false".into(),
            "rate-limit=0".into(),
            "hardcore=false".into(),
            "white-list=false".into(),
            "broadcast-console-to-ops=true".into(),
            "spawn-npcs=true".into(),
            "spawn-animals=true".into(),
            "function-permission-level=2".into(),
            "initial-enabled-packs=vanilla".into(),
            "level-type=minecraft\\:normal".into(),
            "text-filtering-config=".into(),
            "spawn-monsters=true".into(),
            "enforce-whitelist=false".into(),
            "spawn-protection=16".into(),
            "resource-pack-sha1=".into(),
            "max-world-size=29999984".into(),
        ]
    };

    let set_kv = |lines: &mut Vec<String>, key: &str, val: &str| {
        let escaped_key = regex::escape(key);
        let re = regex::Regex::new(&format!("^{}", escaped_key)).ok();
        let mut found = false;
        for line in lines.iter_mut() {
            if re.as_ref().map(|r| r.is_match(line)).unwrap_or(false) {
                *line = format!("{}={}", key, val);
                found = true;
                break;
            }
        }
        if !found {
            lines.push(format!("{}={}", key, val));
        }
    };

    set_kv(&mut lines, "server-port", &port.to_string());
    set_kv(&mut lines, "query.port", &port.to_string());
    set_kv(&mut lines, "motd", &motd.replace('\n', " "));
    set_kv(&mut lines, "online-mode", if online_mode { "true" } else { "false" });
    set_kv(&mut lines, "server-ip", "");

    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    let _ = std::fs::write(p, content);
}

// ============== 启动类型探测 ==============

fn detect_launch_kind(dir: &std::path::Path) -> String {
    if dir.join("run.bat").exists() || dir.join("run.sh").exists() {
        return "run-script".into();
    }
    if dir.join("fabric-server-launch.jar").exists() {
        return "fabric-launch".into();
    }
    if let Ok(files) = std::fs::read_dir(dir) {
        let mut shim: Option<String> = None;
        for entry in files.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_shim = regex::Regex::new(r"(?i)^forge-.*(-shim)?\.jar$")
                .map(|r| r.is_match(&name) && !name.to_lowercase().contains("installer") && name != "server.jar")
                .unwrap_or(false);
            if is_shim {
                shim = Some(name);
                break;
            }
        }
        if let Some(name) = shim {
            return format!("forge-jar:{}", name);
        }
        if dir.join("unix_args.txt").exists() || dir.join("win_args.txt").exists() {
            return "argfile".into();
        }
    }
    if dir.join("server.jar").exists() {
        return "server-jar".into();
    }
    "unknown".into()
}

// ============== 选择 Java ==============

fn select_java_for_major(required_major: u32) -> Result<String, String> {
    let java_list = crate::java::detect_all();
    let mut best: Option<String> = None;
    let mut best_major: i32 = -1;
    let mut max_found: i32 = 0;
    for j in &java_list {
        let mv = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
        if mv > max_found {
            max_found = mv;
        }
        if mv >= required_major as i32 && (best_major < 0 || mv < best_major) {
            best = j.get("path").and_then(|v| v.as_str()).map(|s| s.to_string());
            best_major = mv;
        }
    }
    if let Some(p) = best {
        return Ok(p);
    }
    // 尝试 settings 全局 javaPath
    let settings = storage::load_settings();
    let global = settings.get("javaPath").and_then(|v| v.as_str()).unwrap_or("");
    if !global.is_empty() && std::path::Path::new(global).exists() {
        return Ok(global.to_string());
    }
    Err(format!(
        "此服务端需要 Java {}+，当前系统最高仅 Java {}。请到「Java 管理」页下载 Java {} 后重试。",
        required_major,
        if max_found > 0 { max_found.to_string() } else { "?".into() },
        required_major
    ))
}

// ============== 模组同步过滤 ==============

fn resolve_client_mods_dir(version_id: &str) -> PathBuf {
    let cand = versions_dir().join(version_id).join("mods");
    if cand.exists() {
        return cand;
    }
    let global = storage::resolve_data_dir().join("mods");
    if global.exists() {
        return global;
    }
    cand
}

fn is_client_only_by_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let hints = [
        "optifine", "sodium", "iris", "rubidium", "oculus",
        "notenoughanimations", "entityculling", "fabrishot", "modmenu",
        "reeses-sodium", "immediatelyfast", "dynamic-fps", "zoomify",
        "controllable", "mousewheelie", "xaero", "journeymap",
        "minimap", "litematica", "tweakeroo", "malilib",
        "replaymod", "freecam", "bobby",
    ];
    hints.iter().any(|h| lower.contains(h))
}

/// 读取 jar 中的指定条目（zip 解析）
fn read_jar_entry(jar_path: &std::path::Path, entry_name: &str) -> Option<String> {
    let file = std::fs::File::open(jar_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).ok()?;
        if entry.name() == entry_name {
            use std::io::Read;
            let mut buf = String::new();
            entry.take(8 * 1024 * 1024).read_to_string(&mut buf).ok()?;
            return Some(buf.trim_start_matches('\u{FEFF}').to_string());
        }
    }
    None
}

/// 判断模组是否仅客户端（fabric.mod.json 的 environment 或 mods.toml 的 clientSideOnly）
fn is_mod_client_only(jar_path: &std::path::Path) -> bool {
    // fabric.mod.json
    if let Some(content) = read_jar_entry(jar_path, "fabric.mod.json") {
        if let Ok(j) = serde_json::from_str::<Value>(&content) {
            let env = j.get("environment");
            // 字符串形式："client" | "server" | "*"
            if let Some(env_str) = env.and_then(|v| v.as_str()) {
                if env_str == "client" {
                    return true;
                }
            }
            // 数组形式（少见）
            if let Some(arr) = env.and_then(|v| v.as_array()) {
                if arr.len() == 1 && arr[0].as_str() == Some("client") {
                    return true;
                }
            }
            // environment 字段可能也写成对象
            if let Some(obj) = env.and_then(|v| v.as_object()) {
                if let Some(client) = obj.get("client").and_then(|v| v.as_str()) {
                    if client == "client" {
                        return true;
                    }
                }
            }
        }
    }
    // META-INF/mods.toml
    if let Some(content) = read_jar_entry(jar_path, "META-INF/mods.toml") {
        if let Ok(re) = regex::Regex::new(r"(?i)clientSideOnly\s*=\s*true") {
            if re.is_match(&content) {
                return true;
            }
        }
    }
    false
}

/// 同步客户端 mods 到服务端
async fn sync_client_mods_to_server(
    app: &AppHandle,
    server_id: &str,
    src_mods: &std::path::Path,
    dest_mods: &std::path::Path,
) -> Value {
    let _ = tokio::fs::create_dir_all(dest_mods).await;

    if !src_mods.exists() {
        return json!({
            "ok": true,
            "copied": 0,
            "skipped": 0,
            "clientOnly": 0,
            "message": "客户端无 mods 目录"
        });
    }

    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(src_mods) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if lower.ends_with(".jar") && !lower.starts_with('.') {
                files.push(name);
            }
        }
    }

    let total = files.len();
    let mut copied = 0u32;
    let mut skipped = 0u32;
    let mut client_only_count = 0u32;

    for (i, name) in files.iter().enumerate() {
        let pct = if total > 0 { ((i + 1) * 100 / total) as u8 } else { 100 };
        emit_status(
            app,
            server_id,
            "syncing-mods",
            json!({
                "progress": pct,
                "indeterminate": false,
                "message": format!("同步模组 {}/{}: {}", i + 1, total, name),
                "stage": "mods"
            }),
        );
        emit_log(app, server_id, &format!("[VersePC] 同步模组 {}/{}: {}", i + 1, total, name), "out");

        if is_client_only_by_name(name) {
            client_only_count += 1;
            skipped += 1;
            continue;
        }

        let src = src_mods.join(name);
        let dest = dest_mods.join(name);

        let env_client_only = is_mod_client_only(&src);
        if env_client_only {
            client_only_count += 1;
            skipped += 1;
            continue;
        }

        // 已存在且大小相同则跳过
        let should_copy = match (std::fs::metadata(&src), std::fs::metadata(&dest)) {
            (Ok(s), Ok(d)) => s.len() != d.len(),
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if !should_copy {
            skipped += 1;
            continue;
        }

        if std::fs::copy(&src, &dest).is_ok() {
            copied += 1;
        } else {
            skipped += 1;
        }
    }

    let other_skipped = skipped.saturating_sub(client_only_count);
    json!({
        "ok": true,
        "copied": copied,
        "skipped": skipped,
        "clientOnly": client_only_count,
        "total": total,
        "source": src_mods.to_string_lossy(),
        "dest": dest_mods.to_string_lossy(),
        "message": format!("已同步 {} 个模组（跳过客户端 {}，其他跳过 {}）", copied, client_only_count, other_skipped)
    })
}

// ============== 模组端服务端安装 ==============

/// 运行 java -jar 安装器
async fn run_installer(
    app: &AppHandle,
    server_id: &str,
    java_path: &str,
    args: Vec<String>,
    cwd: &std::path::Path,
    timeout_secs: u64,
) -> Result<(), String> {
    emit_log(app, server_id, &format!("[VersePC] 运行安装器: {} {}", java_path, args.join(" ")), "out");
    let mut cmd = Command::new(java_path);
    cmd.args(&args);
    cmd.current_dir(cwd);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动安装器失败: {}", e))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let sid = server_id.to_string();
    let app_clone = app.clone();
    if let Some(stdout) = stdout {
        let app_clone2 = app_clone.clone();
        let sid2 = sid.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_clone2, &sid2, &line, "out");
            }
        });
    }
    if let Some(stderr) = stderr {
        let app_clone2 = app_clone.clone();
        let sid2 = sid.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_clone2, &sid2, &line, "err");
            }
        });
    }

    let wait_fut = child.wait();
    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), wait_fut)
        .await
        .map_err(|_| {
            let _ = child.start_kill();
            format!("安装器超时 ({}s)", timeout_secs)
        })?;
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("安装器退出码 {:?}", status.code())),
        Err(e) => Err(format!("安装器执行失败: {}", e)),
    }
}

/// 下载文件（多镜像回退，简化版：尝试多个 URL，逐个调 download_with_mirror）
async fn download_with_fallback(
    app: &AppHandle,
    server_id: &str,
    urls: Vec<String>,
    dest: &std::path::Path,
    sha1: Option<&str>,
    size: Option<u64>,
) -> Result<(), String> {
    let mut last_err = String::new();
    for url in &urls {
        emit_log(app, server_id, &format!("[VersePC] 下载: {}", url), "out");
        let result = download_with_mirror(url, dest, sha1, size, "auto", 600, None).await;
        match result {
            Ok(()) => {
                if let Ok(meta) = std::fs::metadata(dest) {
                    if meta.len() > 1024 {
                        return Ok(());
                    }
                }
                last_err = "下载文件过小".into();
            }
            Err(e) => {
                last_err = e;
            }
        }
        let _ = std::fs::remove_file(dest);
    }
    Err(last_err)
}

/// 安装 Forge / NeoForge / Fabric 服务端
async fn install_modded_server(
    app: &AppHandle,
    server_id: &str,
    dir: &std::path::Path,
    loader_info: &Value,
    java_major: u32,
) -> Result<Value, String> {
    let loader = loader_info
        .get("loader")
        .and_then(|v| v.as_str())
        .unwrap_or("vanilla");
    let mc_version = loader_info
        .get("mcVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let loader_version = loader_info
        .get("loaderVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    emit_log(app, server_id, &format!("[VersePC] 安装器需要 Java {}+", java_major), "out");
    let java_path = select_java_for_major(java_major)?;
    emit_log(app, server_id, &format!("[VersePC] 选择 Java: {}", java_path), "out");

    match loader {
        "forge" | "neoforge" => {
            let is_neo = loader == "neoforge";
            let mut ver = loader_version.to_string();
            if ver.is_empty() {
                return Err("无法解析 Forge/NeoForge 版本号，请确认客户端版本完整".into());
            }
            if !is_neo && !mc_version.is_empty() && !ver.contains(mc_version) {
                if let Some(re) = regex::Regex::new(r"^.*?(\d+\.\d+[\w.-]*)$").ok() {
                    if let Some(c) = re.captures(&ver) {
                        let suffix = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
                        ver = format!("{}-{}", mc_version, suffix);
                    }
                }
            }

            let installer_name = if is_neo {
                format!("neoforge-{}-installer.jar", ver)
            } else {
                format!("forge-{}-installer.jar", ver)
            };
            let installer_path = dir.join(&installer_name);

            emit_status(
                app,
                server_id,
                "downloading",
                json!({
                    "progress": 0,
                    "message": format!("下载 {} 安装器...", if is_neo { "NeoForge" } else { "Forge" }),
                    "stage": "installer"
                }),
            );

            let urls: Vec<String> = if is_neo {
                vec![
                    format!("https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/{}/neoforge-{}-installer.jar", ver, ver),
                    format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}-installer.jar", ver, ver),
                    format!("https://bmclapi2.bangbang93.com/maven/net/neoforged/forge/{}/forge-{}-installer.jar", ver, ver),
                    format!("https://maven.neoforged.net/releases/net/neoforged/forge/{}/forge-{}-installer.jar", ver, ver),
                ]
            } else {
                vec![
                    format!("https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/{}/forge-{}-installer.jar", ver, ver),
                    format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{}/forge-{}-installer.jar", ver, ver),
                ]
            };

            download_with_fallback(app, server_id, urls, &installer_path, None, None).await?;

            emit_status(
                app,
                server_id,
                "installing",
                json!({
                    "indeterminate": true,
                    "message": format!("正在安装 {} 服务端...", if is_neo { "NeoForge" } else { "Forge" }),
                    "stage": "install"
                }),
            );

            // 第一次尝试：--installServer <dir>
            let args1 = vec![
                "-jar".to_string(),
                installer_name.clone(),
                "--installServer".to_string(),
                dir.to_string_lossy().to_string(),
            ];
            let result1 = run_installer(app, server_id, &java_path, args1, dir, 900).await;
            if let Err(e) = result1 {
                emit_log(app, server_id, &format!("[VersePC] 安装器参数回退: --installServer ({})", e), "err");
                let args2 = vec![
                    "-jar".to_string(),
                    installer_name.clone(),
                    "--installServer".to_string(),
                ];
                run_installer(app, server_id, &java_path, args2, dir, 900).await?;
            }

            let _ = std::fs::remove_file(&installer_path);
            let launch_kind = detect_launch_kind(dir);
            emit_log(
                app,
                server_id,
                &format!("[VersePC] {} 服务端安装完成, launch={}", if is_neo { "NeoForge" } else { "Forge" }, launch_kind),
                "out",
            );

            Ok(json!({
                "loader": loader,
                "loaderVersion": ver,
                "launchKind": launch_kind
            }))
        }
        "fabric" => {
            let mc = if !mc_version.is_empty() {
                mc_version.to_string()
            } else {
                return Err("无法解析 Fabric 的 MC 版本".into());
            };
            let loader_ver = if !loader_version.is_empty() {
                loader_version.to_string()
            } else {
                emit_log(app, server_id, "[VersePC] 拉取 Fabric loader 版本...", "out");
                let urls = vec![
                    format!("https://meta.fabricmc.net/v2/versions/loader/{}", urlencoding::encode(&mc)),
                    format!("https://bmclapi2.bangbang93.com/fabric-meta/v2/versions/loader/{}", urlencoding::encode(&mc)),
                ];
                let mut fetched: Option<String> = None;
                for url in &urls {
                    if let Ok(body) = crate::modloaders::shared::fetch_json(url).await {
                        if let Some(arr) = body.as_array() {
                            if let Some(first) = arr.first() {
                                if let Some(v) = first
                                    .get("loader")
                                    .and_then(|l| l.get("version"))
                                    .and_then(|v| v.as_str())
                                {
                                    fetched = Some(v.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
                fetched.ok_or_else(|| "无法解析 Fabric Loader 版本".to_string())?
            };

            let installer_path = dir.join("fabric-installer.jar");
            emit_status(
                app,
                server_id,
                "downloading",
                json!({
                    "progress": 0,
                    "message": "下载 Fabric 安装器...",
                    "stage": "installer"
                }),
            );

            let urls = vec![
                "https://bmclapi2.bangbang93.com/maven/net/fabricmc/fabric-installer/1.0.1/fabric-installer-1.0.1.jar".to_string(),
                "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.0.1/fabric-installer-1.0.1.jar".to_string(),
                "https://maven.fabricmc.net/net/fabricmc/fabric-installer/0.11.2/fabric-installer-0.11.2.jar".to_string(),
            ];
            download_with_fallback(app, server_id, urls, &installer_path, None, None).await?;

            emit_status(
                app,
                server_id,
                "installing",
                json!({
                    "indeterminate": true,
                    "message": "正在安装 Fabric 服务端...",
                    "stage": "install"
                }),
            );
            emit_log(
                app,
                server_id,
                &format!("[VersePC] Fabric server -mcversion {} -loader {}", mc, loader_ver),
                "out",
            );

            let args = vec![
                "-jar".to_string(),
                "fabric-installer.jar".to_string(),
                "server".to_string(),
                "-mcversion".to_string(),
                mc.clone(),
                "-loader".to_string(),
                loader_ver.clone(),
                "-dir".to_string(),
                dir.to_string_lossy().to_string(),
                "-downloadMinecraft".to_string(),
            ];
            run_installer(app, server_id, &java_path, args, dir, 900).await?;

            let _ = std::fs::remove_file(&installer_path);
            let launch_kind = detect_launch_kind(dir);
            emit_log(app, server_id, "[VersePC] Fabric 服务端安装完成", "out");

            Ok(json!({
                "loader": "fabric",
                "loaderVersion": loader_ver,
                "launchKind": launch_kind
            }))
        }
        _ => Err(format!("未知加载器类型: {}", loader)),
    }
}

// ============== 子进程启动 ==============

/// 构建启动参数
fn build_launch_args(launch_kind: &str, dir: &std::path::Path, max_mem: u32, min_mem: u32) -> Vec<String> {
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

// ============== Tauri 命令 ==============

/// 1. 列出所有本地开服配置
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_list(app: AppHandle) -> Value {
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

/// 2. 创建开服目录 + 下载 server.jar + 写 eula/properties
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_create(
    app: AppHandle,
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
            .unwrap_or_else(|| gen_id())
    } else {
        gen_id()
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

    let dir = server_dir_of(&id);
    let _ = std::fs::create_dir_all(&dir);

    // === 模组端分支 ===
    if loader != "vanilla" {
        emit_log(
            &app,
            &id,
            &format!(
                "[VersePC] 检测到加载器: {} {} (MC {})",
                loader,
                loader_info.get("loaderVersion").and_then(|v| v.as_str()).unwrap_or(""),
                loader_info.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("?")
            ),
            "out",
        );

        match install_modded_server(&app, &id, &dir, &loader_info, java_major).await {
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
                if let Some(arr) = idx_m.get_mut("servers").and_then(|v| v.as_array_mut()) {
                    if let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id)) {
                        if let Some(obj) = arr[pos].as_object_mut() {
                            obj.insert("loader".into(), json!(loader));
                            obj.insert("loaderVersion".into(), json!(loader_ver));
                            obj.insert("launchKind".into(), json!(launch_kind));
                            obj.insert("updatedAt".into(), json!(now_ms()));
                        }
                    }
                }
                save_index(&idx_m);

                // 同步模组
                let mod_sync = if sync_mods {
                    let src_mods = resolve_client_mods_dir(&version_id);
                    let dest_mods = dir.join("mods");
                    let r = sync_client_mods_to_server(&app, &id, &src_mods, &dest_mods).await;
                    Some(r)
                } else {
                    None
                };

                emit_status(
                    &app,
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
                emit_log(&app, &id, &format!("[VersePC] 模组端安装失败: {}", e), "err");
                emit_status(&app, &id, "error", json!({ "message": format!("模组端安装失败: {}", e), "stage": "error" }));
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
        emit_status(&app, &id, "downloading", json!({ "progress": 0, "message": "正在下载 server.jar..." }));
        emit_log(&app, &id, &format!("[VersePC] 下载 server.jar ← {}", server_url), "out");

        if let Err(e) = download_with_mirror(server_url, &jar_path, server_sha1, server_size, "auto", 600, None).await {
            return json!({ "ok": false, "error": format!("下载 server.jar 失败: {}", e) });
        }
        emit_log(&app, &id, "[VersePC] server.jar 就绪", "out");
    } else {
        emit_log(&app, &id, "[VersePC] 复用已有 server.jar", "out");
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
    if let Some(arr) = idx2.get_mut("servers").and_then(|v| v.as_array_mut()) {
        if let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id)) {
            if let Some(obj) = arr[pos].as_object_mut() {
                obj.insert("updatedAt".into(), json!(now_ms()));
            }
        }
    }
    save_index(&idx2);

    emit_status(&app, &id, "ready", json!({ "message": "服务端已准备就绪", "path": dir.to_string_lossy() }));

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

/// 3. 启动服务端
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_start(app: AppHandle, server_id: String) -> Value {
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

    let dir = server_dir_of(&server_id);
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
    emit_log(&app, &server_id, &format!("[VersePC] 需要 Java {}+（{}）", required_major, base_version), "out");
    let java_path = match select_java_for_major(required_major) {
        Ok(p) => p,
        Err(e) => return json!({ "ok": false, "error": e }),
    };

    let mut cmd = if launch_kind == "run-script" {
        let script = if cfg!(target_os = "windows") { "run.bat" } else { "run.sh" };
        emit_log(&app, &server_id, &format!("[VersePC] 使用启动脚本: {}", script), "out");
        emit_status(&app, &server_id, "starting", json!({ "message": "正在启动（run 脚本）...", "javaPath": java_path, "port": port }));
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
        emit_log(&app, &server_id, &format!("[VersePC] {} {}", java_path, args.join(" ")), "out");
        let kind_msg = if launch_kind == "fabric-launch" {
            "Fabric"
        } else if launch_kind.starts_with("forge-jar:") {
            "Forge"
        } else {
            ""
        };
        emit_status(&app, &server_id, "starting", json!({ "message": format!("正在启动{}...", kind_msg), "javaPath": java_path, "port": port }));
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
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            emit_log(&app, &server_id, &format!("[VersePC] 启动失败: {}", e), "err");
            emit_status(&app, &server_id, "error", json!({ "message": e.to_string() }));
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

    emit_log(&app, &server_id, &format!("[VersePC] cwd={} launchKind={}", dir.display(), launch_kind), "out");

    // stdout 异步读取
    if let Some(stdout) = stdout {
        let app_c = app.clone();
        let sid = server_id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_c, &sid, &line, "out");
                if line.contains("Done (") || line.contains("For help, type \"help\"") {
                    set_starting(&sid, false);
                    emit_status(&app_c, &sid, "running", json!({
                        "message": "服务器已就绪",
                        "port": port,
                        "localIps": list_local_ips()
                    }));
                }
                if line.to_lowercase().contains("unsupportedclassversionerror") || line.contains("has been compiled by a more recent version") {
                    emit_log(&app_c, &sid, &format!("[VersePC] Java 版本过低：该服务端需要 Java {}+。请到「Java 管理」页下载后重启服务端。", required_major), "err");
                }
            }
        });
    }

    // stderr 异步读取
    if let Some(stderr) = stderr {
        let app_c = app.clone();
        let sid = server_id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_log(&app_c, &sid, &line, "err");
            }
        });
    }

    // 进程退出监听
    let app_c = app.clone();
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
        emit_log(&app_c, &sid_c, "[VersePC] 进程退出", "out");
        emit_status(&app_c, &sid_c, "stopped", json!({}));
    });

    // 更新 index: lastStartedAt
    let mut idx2 = load_index();
    if let Some(arr) = idx2.get_mut("servers").and_then(|v| v.as_array_mut()) {
        if let Some(pos) = arr.iter().position(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id)) {
            if let Some(obj) = arr[pos].as_object_mut() {
                obj.insert("lastStartedAt".into(), json!(now_ms()));
                obj.insert("port".into(), json!(port));
                obj.insert("maxMem".into(), json!(max_mem));
            }
        }
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

/// 4. 停止服务端（写 stop 到 stdin，等 15s 再 SIGKILL）
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_stop(app: AppHandle, server_id: String) -> Value {
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
    emit_status(&app, &server_id, "stopping", json!({ "message": "正在关闭服务器..." }));

    // 等最多 15s 优雅退出
    let exited = tokio::time::timeout(Duration::from_secs(15), exit_rx).await.is_ok();

    if !exited {
        emit_log(&app, &server_id, "[VersePC] 优雅关闭超时，强制结束进程", "err");
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
        emit_status(&app, &server_id, "stopped", json!({ "forced": true }));
    }

    json!({ "ok": true, "forced": !exited })
}

/// 5. 向服务端 stdin 发送命令
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_command(app: AppHandle, server_id: String, cmd: String) -> Value {
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

    emit_log(&app, &server_id, &format!("> {}", cmd_clean), "cmd");
    let mut guard = stdin_arc.lock().await;
    if let Some(stdin) = guard.as_mut() {
        let _ = stdin.write_all(format!("{}\n", cmd_clean).as_bytes()).await;
        let _ = stdin.flush().await;
        json!({ "ok": true })
    } else {
        json!({ "ok": false, "error": "服务器 stdin 已关闭" })
    }
}

/// 6. 查询运行状态
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_status(app: AppHandle, server_id: Option<String>) -> Value {
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
    let _ = app; // 保留参数以便将来广播
    status_all()
}

fn status_all() -> Value {
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

/// 7. 删除服务端
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_delete(app: AppHandle, server_id: String) -> Value {
    if server_id.is_empty() {
        return json!({ "ok": false, "error": "缺少服务器 id" });
    }
    if is_running(&server_id) {
        return json!({ "ok": false, "error": "请先停止服务器再删除" });
    }
    let mut idx = load_index();
    let dir = idx
        .get("servers")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&server_id)))
        .and_then(|s| s.get("dir").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .map(PathBuf::from)
        .unwrap_or_else(|| server_dir_of(&server_id));

    if let Some(arr) = idx.get_mut("servers").and_then(|v| v.as_array_mut()) {
        arr.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(&server_id));
    }
    save_index(&idx);

    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let _ = app; // 保留参数以便将来广播
    json!({ "ok": true })
}

/// 8. 打开服务端目录
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_open_dir(app: AppHandle, server_id: Option<String>) -> Value {
    ensure_root();
    let dir = match server_id {
        Some(id) if !id.is_empty() => {
            let idx = load_index();
            let entry_dir = idx
                .get("servers")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(&id)))
                .and_then(|s| s.get("dir").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .map(PathBuf::from)
                .unwrap_or_else(|| server_dir_of(&id));
            entry_dir
        }
        _ => servers_root(),
    };
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    let _ = open::that(&dir);
    let _ = app;
    json!({ "ok": true, "path": dir.to_string_lossy() })
}

/// 9. 沿 inheritsFrom 链查 downloads.server
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_resolve_version(app: AppHandle, version_id: String) -> Value {
    let _ = app;
    match resolve_server_download(&version_id) {
        Some(r) => json!({ "ok": true, "versionId": r.get("versionId"), "server": r.get("server"), "javaVersion": r.get("javaVersion") }),
        None => json!({ "ok": false, "error": "无法解析 server 下载" }),
    }
}

/// 10. 识别客户端加载器
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_detect_loader(app: AppHandle, version_id: String) -> Value {
    let _ = app;
    let info = detect_client_loader(&version_id);
    json!({
        "ok": true,
        "loader": info.get("loader"),
        "loaderVersion": info.get("loaderVersion"),
        "mcVersion": info.get("mcVersion"),
        "chainIds": info.get("chainIds")
    })
}

/// 11. 同步客户端 mods 到服务端
#[tauri::command(rename_all = "camelCase")]
pub async fn server_host_sync_mods(app: AppHandle, server_id: String, client_version_id: String) -> Value {
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

    let dir = entry
        .get("dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| server_dir_of(&server_id));

    let src_mods = resolve_client_mods_dir(&client_version_id);
    let dest_mods = dir.join("mods");
    let result = sync_client_mods_to_server(&app, &server_id, &src_mods, &dest_mods).await;
    let message = result.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    emit_status(&app, &server_id, "ready", json!({ "progress": 100, "message": message, "stage": "ready" }));
    let mut response = json!({ "ok": true });
    if let (Some(dst), Some(src)) = (response.as_object_mut(), result.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    response
}

// ============== 辅助 ==============

fn sanitize_name(name: &str) -> String {
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

fn gen_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let rand_part: String = (0..5)
        .map(|_| {
            let n = rand::random::<u32>() % 36;
            if n < 10 {
                char::from_digit(n, 10).unwrap()
            } else {
                char::from_digit(n - 10 + 26, 36).unwrap()
            }
        })
        .collect();
    format!("srv_{:x}_{}", ts, rand_part)
}

fn set_starting(id: &str, starting: bool) {
    let mut guard = running_map();
    if let Some(m) = guard.as_mut() {
        if let Some(r) = m.get_mut(id) {
            r.starting = starting;
        }
    }
}

fn list_local_ips() -> Vec<String> {
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

async fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
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
