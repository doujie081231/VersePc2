//! Rust 版（Horizon 桌面端）支持：查询最新版本、下载启动器、运行安装包。
//!
//! 桌面端不需要像安卓那样内置引擎——Horizon 官方提供 Windows 启动器，
//! 安装后由它自己维护游戏与资源，这里只做"下载 → 运行 → 打开"的引导。

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const REPO: &str = "horizon-rust/horizon";

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

/// 查 Horizon 最新桌面版（挑 Windows x64 安装包）
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
        // 优先 setup.exe（带安装程序），退而求其次用 zip 免安装版
        let picked = assets
            .iter()
            .find(|a| a["name"].as_str().unwrap_or("").eq_ignore_ascii_case("horizon-launcher-setup.exe"))
            .or_else(|| {
                assets.iter().find(|a| {
                    a["name"].as_str().unwrap_or("").eq_ignore_ascii_case("horizon-launcher-windows-x64.zip")
                })
            });
        if let Some(asset) = picked {
            let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
            return Ok(HorizonRelease {
                version: tag.trim_start_matches("launcher@").to_string(),
                tag,
                asset: asset["name"].as_str().unwrap_or("").to_string(),
                url: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
                size: asset["size"].as_u64().unwrap_or(0),
                published: rel["published_at"].as_str().unwrap_or("").chars().take(10).collect(),
                notes: rel["html_url"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Err("没有找到可用的 Windows 构建".into())
}

fn download_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = base.join("horizon");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// 下载安装包到应用数据目录，边下边发进度事件 horizon-progress
#[tauri::command]
pub async fn horizon_download(
    app: AppHandle,
    url: String,
    name: String,
) -> Result<String, String> {
    use futures_util::StreamExt;
    use std::io::Write;

    let dir = download_dir(&app)?;
    let dest = dir.join(&name);
    let tmp = dir.join(format!("{name}.part"));

    let client = reqwest::Client::builder()
        .user_agent("VersePC2")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut done: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("读取失败: {e}"))?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        done += bytes.len() as u64;
        let _ = app.emit(
            "horizon-progress",
            serde_json::json!({ "done": done, "total": total }),
        );
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if total > 0 && done != total {
        return Err(format!("下载不完整: {done}/{total}"));
    }
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

/// 运行安装包（或已解压的启动器）
#[tauri::command]
pub fn horizon_run(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("文件不存在: {path}"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path])
            .spawn()
            .map_err(|e| format!("启动失败: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("当前平台暂不支持一键运行，请到 Horizon 官网下载".into())
    }
}

/// 已下载过的安装包（重新打开应用时不用重复下载）
#[tauri::command]
pub fn horizon_cached(app: AppHandle) -> Result<Option<String>, String> {
    let dir = download_dir(&app)?;
    let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && (name.ends_with("setup.exe") || name.ends_with("windows-x64.zip"))
        {
            return Ok(Some(path.to_string_lossy().to_string()));
        }
    }
    Ok(None)
}
