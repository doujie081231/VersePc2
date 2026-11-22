//! Rust 版（Horizon）——在启动器内安装、准备并启动。
//!
//! 桌面端不装 Horizon 自己的启动器窗口，而是把官方发行包里的 `horizon-launcher.exe`
//! 当作**运行时**收下，用它的 CLI 子命令完成「安装版本 → 准备资源 → 启动游戏」：
//!
//! ```text
//! horizon-launcher.exe install latest      # 下载并安装一个 Horizon 版本
//! horizon-launcher.exe prepare --download  # 用已登录账号从 Mojang 拉 26.3 资源
//! horizon-launcher.exe play                # 启动客户端
//! ```
//!
//! 和手机端一致：所有步骤的进度与输出都回到本启动器界面，用户不需要再去开另一个应用。
//! 运行时的数据目录由 Horizon 自己决定（Windows 是 `%APPDATA%\horizon-launcher`），
//! 我们只负责放 exe、驱动命令、读状态。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{AppHandle, Emitter, Manager};

const REPO: &str = "horizon-rust/horizon";
/// 便携包（单文件 exe，28MB），比 setup.exe 更适合被当作运行时收编
const ZIP_ASSET: &str = "horizon-launcher-windows-x64.zip";
const EXE_NAME: &str = "horizon-launcher.exe";
/// GitHub 在国内不稳，和手机端一致走加速前缀兜底
const MIRRORS: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
];

#[derive(Serialize, Clone, Default)]
pub struct HorizonStatus {
    /// 运行时（horizon-launcher.exe）是否已就位
    pub runtime: bool,
    pub exe: String,
    /// 已安装的版本 tag（来自 `versions --installed --json`）
    pub installed: Vec<String>,
    /// 当前选中的版本
    pub selected: Option<String>,
    /// 资源是否已准备（存在 resources/horizon-resources.json）
    pub prepared: bool,
    /// 已登录账号（`account list --json` 里被选中的那个）
    pub account: Option<String>,
    pub data_dir: String,
}

#[derive(Serialize, Clone)]
pub struct HorizonRelease {
    pub tag: String,
    pub version: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    pub published: String,
    pub notes: String,
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = base.join("horizon");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn exe_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join(EXE_NAME))
}

/// 运行时的数据目录（Horizon 自己定的，用于判断资源是否就绪）
fn launcher_data_dir() -> Option<PathBuf> {
    let roaming = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(roaming).join("horizon-launcher"))
}

/// 同步跑一条 CLI 命令并拿 stdout（短命令：versions / settings / account list）
fn run_capture(exe: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("执行失败: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    Ok(text)
}

/// 查 Horizon 最新桌面版（挑 Windows x64 便携 zip）
#[tauri::command]
pub async fn horizon_latest() -> Result<HorizonRelease, String> {
    let client = reqwest::Client::builder()
        .user_agent("VersePC2")
        .build()
        .map_err(|e| e.to_string())?;

    let releases: serde_json::Value = client
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=10"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析失败: {e}"))?;

    let list = releases.as_array().ok_or("返回不是数组")?;
    for rel in list {
        if rel["prerelease"].as_bool().unwrap_or(false) {
            continue;
        }
        let assets = rel["assets"].as_array().cloned().unwrap_or_default();
        let picked = assets.iter().find(|a| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case(ZIP_ASSET)
        });
        if let Some(asset) = picked {
            let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
            return Ok(HorizonRelease {
                version: tag.trim_start_matches("launcher@").to_string(),
                tag,
                asset: asset["name"].as_str().unwrap_or("").to_string(),
                url: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
                size: asset["size"].as_u64().unwrap_or(0),
                published: rel["published_at"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
                notes: rel["html_url"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Err("没有找到可用的 Windows 运行包".into())
}

/// 当前状态：运行时 / 已装版本 / 资源 / 账号
#[tauri::command]
pub async fn horizon_status(app: AppHandle) -> Result<HorizonStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = data_dir(&app)?;
        let exe = dir.join(EXE_NAME);
        let mut st = HorizonStatus {
            runtime: exe.is_file(),
            exe: exe.to_string_lossy().to_string(),
            data_dir: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        if !st.runtime {
            return Ok(st);
        }

        // 已安装版本（离线查询，不触网）：{"installed": [...], "selected": "..."}
        if let Ok(text) = run_capture(&exe, &["versions", "--installed", "--json"]) {
            let (list, selected) = parse_versions(&text);
            st.installed = list;
            st.selected = selected;
        }
        // settings 里的 selected_version 作为兜底（versions 已给就用 versions 的）
        if st.selected.is_none() {
            if let Ok(text) = run_capture(&exe, &["settings", "--json"]) {
                st.selected = parse_selected(&text);
            }
        }
        if let Ok(text) = run_capture(&exe, &["account", "list", "--json"]) {
            st.account = parse_account(&text);
        }
        // 资源就绪：任一已装版本下存在 horizon-resources.json
        if let Some(base) = launcher_data_dir() {
            let version_dirs: Vec<PathBuf> = if st.installed.is_empty() {
                st.selected
                    .clone()
                    .map(|v| vec![base.join("versions").join(v)])
                    .unwrap_or_default()
            } else {
                st.installed
                    .iter()
                    .map(|v| base.join("versions").join(v))
                    .collect()
            };
            st.prepared = version_dirs.iter().any(|d| {
                d.join("resources").join("horizon-resources.json").is_file()
                    || d.join("horizon-resources.json").is_file()
            });
        }
        Ok(st)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 下载并解出运行时（28MB 便携 zip），然后把它自己的语言设成中文
#[tauri::command]
pub async fn horizon_install_runtime(app: AppHandle) -> Result<HorizonStatus, String> {
    use futures_util::StreamExt;

    let rel = horizon_latest().await?;
    let dir = data_dir(&app)?;

    let client = reqwest::Client::builder()
        .user_agent("VersePC2")
        .build()
        .map_err(|e| e.to_string())?;

    let mut last_err = String::new();
    let mut downloaded: Option<PathBuf> = None;
    let mut urls = vec![rel.url.clone()];
    for m in MIRRORS {
        urls.push(format!("{m}{}", rel.url));
    }
    let tmp = dir.join(format!("{ZIP_ASSET}.part"));
    for (idx, url) in urls.iter().enumerate() {
        let _ = app.emit(
            "horizon-log",
            serde_json::json!({ "line": format!("下载运行时 {}. {}", idx + 1, url), "kind": "info" }),
        );
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let total = resp.content_length().unwrap_or(rel.size);
                let mut stream = resp.bytes_stream();
                let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                let mut done: u64 = 0;
                let mut failed = false;
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            use std::io::Write;
                            if let Err(e) = file.write_all(&bytes) {
                                last_err = e.to_string();
                                failed = true;
                                break;
                            }
                            done += bytes.len() as u64;
                            let _ = app.emit(
                                "horizon-progress",
                                serde_json::json!({ "phase": "runtime", "done": done, "total": total }),
                            );
                        }
                        Err(e) => {
                            last_err = format!("读取失败: {e}");
                            failed = true;
                            break;
                        }
                    }
                }
                if !failed && (total == 0 || done == total) {
                    downloaded = Some(tmp.clone());
                    break;
                }
                let _ = std::fs::remove_file(&tmp);
            }
            Ok(resp) => last_err = format!("HTTP {}", resp.status()),
            Err(e) => last_err = format!("请求失败: {e}"),
        }
    }
    let Some(zip_file) = downloaded else {
        return Err(format!("下载运行时失败：{last_err}"));
    };

    // 解出 exe
    let file = std::fs::File::open(&zip_file).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 zip: {e}"))?;
    let mut extracted = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if !name.to_ascii_lowercase().ends_with(EXE_NAME) {
            continue;
        }
        let target = dir.join(EXE_NAME);
        let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        extracted = true;
        break;
    }
    let _ = std::fs::remove_file(&zip_file);
    if !extracted {
        return Err("发行包里没有 horizon-launcher.exe".into());
    }

    // 界面语言跟启动器统一成中文（失败不影响主流程）
    let exe = exe_path(&app)?;
    let _ = run_capture(&exe, &["settings", "language", "zh"]);
    let _ = app.emit(
        "horizon-log",
        serde_json::json!({ "line": format!("运行时已就位：{}", exe.display()), "kind": "ok" }),
    );

    horizon_status(app.clone()).await
}

/// 跑一个步骤：install / prepare / signin / play，输出按行回到界面
#[tauri::command]
pub async fn horizon_step(app: AppHandle, step: String) -> Result<i32, String> {
    let exe = exe_path(&app)?;
    if !exe.is_file() {
        return Err("运行时尚未下载".into());
    }
    let args: Vec<&str> = match step.as_str() {
        "install" => vec!["install", "latest"],
        "prepare" => vec!["prepare", "--download"],
        "signin" => vec!["account", "sign-in"],
        "play" => vec!["play"],
        other => return Err(format!("未知步骤: {other}")),
    };

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        // 不要弹出控制台窗口
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动失败: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let app_out = app.clone();
    tauri::async_runtime::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let Some(stream) = stdout else { return };
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app_out.emit("horizon-log", serde_json::json!({ "line": line, "kind": "out" }));
        }
    });
    let app_err = app.clone();
    tauri::async_runtime::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let Some(stream) = stderr else { return };
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app_err.emit("horizon-log", serde_json::json!({ "line": line, "kind": "err" }));
        }
    });

    let status = child.wait().await.map_err(|e| e.to_string())?;
    let code = status.code().unwrap_or(-1);
    let _ = app.emit(
        "horizon-step-done",
        serde_json::json!({ "step": step, "code": code }),
    );
    Ok(code)
}

/// 停止正在运行的游戏
#[tauri::command]
pub async fn horizon_stop(app: AppHandle) -> Result<(), String> {
    let exe = exe_path(&app)?;
    #[cfg(target_os = "windows")]
    {
        let name = exe
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(EXE_NAME)
            .to_string();
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/IM", &name, "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.status();
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = exe;
        Err("当前平台暂不支持".into())
    }
}

// ===================== JSON 解析（对版本差异做宽松处理） =====================

/// `versions --installed --json` → `{"installed": [...], "selected": "..."}`
/// 元素可能是字符串，也可能是带 tag/version 的对象，两种都认
fn parse_versions(text: &str) -> (Vec<String>, Option<String>) {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        installed: Vec<serde_json::Value>,
        #[serde(default)]
        selected: Option<String>,
    }
    if let Ok(w) = serde_json::from_str::<Wrap>(text.trim()) {
        let list = w
            .installed
            .into_iter()
            .filter_map(|v| {
                v.as_str().map(|s| s.to_string()).or_else(|| {
                    v.get("tag")
                        .or_else(|| v.get("version"))
                        .or_else(|| v.get("name"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect();
        return (list, w.selected);
    }
    // 文本形式：每行第一列是 tag
    let list = text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .map(|s| s.to_string())
        .collect();
    (list, None)
}

fn parse_selected(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    v.get("selected_version")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("selectedVersion")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
}

/// `account list --json` → `{"accounts": [...], "owner": bool}`
fn parse_account(text: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        accounts: Vec<serde_json::Value>,
        #[serde(default)]
        owner: bool,
    }
    let name_of = |row: &serde_json::Value| -> Option<String> {
        row.get("name")
            .or_else(|| row.get("username"))
            .or_else(|| row.get("profile_name"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
    };

    if let Ok(w) = serde_json::from_str::<Wrap>(text.trim()) {
        for row in &w.accounts {
            let selected = row
                .get("selected")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
                || row.get("current").and_then(|x| x.as_bool()).unwrap_or(false);
            if selected {
                if let Some(n) = name_of(row) {
                    return Some(n);
                }
            }
        }
        // owner=true 表示这台机器上就一个账号、归它用
        if w.owner {
            if let Some(n) = w.accounts.first().and_then(name_of) {
                return Some(n);
            }
        }
        return w.accounts.first().and_then(name_of);
    }

    // 文本形式：带 * 的那行
    text.lines()
        .find(|l| l.trim_start().starts_with('*'))
        .map(|l| {
            l.trim_start_matches(['*', ' '])
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .filter(|s| !s.is_empty())
}
