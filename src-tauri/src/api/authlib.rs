// api/authlib.rs — authlib-injector 第三方登录支持
// 职责：查询 authlib-injector 最新版本信息，下载 jar 并校验 SHA256
//
// 路由：
//   GET /api/authlib-injector/info      拉取最新版本号、下载地址、文件大小
//   GET /api/authlib-injector/download  下载 jar 到 dataDir/authlib-injector/，校验 SHA256
//
// 公共函数：
//   ensure_authlib_injector() — 检查 jar 是否存在，不存在则下载校验
//                               供 accounts.rs 在第三方登录流程中复用
//
// 安全要求：
//   - 用户传入的 serverUrl 必须是 https:// 开头（防中间人）
//   - 下载后校验官方 checksums.sha256，不匹配则删除文件并报错

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::api::ApiResult;
use crate::storage;

/// authlib-injector 官方最新版本信息 API
const AUTHLIB_ARTIFACT_URL: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";

/// 处理 authlib-injector 路由
pub async fn handle(
    _app: &tauri::AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    _body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/authlib-injector/info" => Some(handle_info(params).await),
        "GET /api/authlib-injector/download" => Some(handle_download(params).await),
        _ => None,
    }
}

/// GET /api/authlib-injector/info — 拉取最新版本信息
///
/// 查询参数：
///   - serverUrl: 可选，第三方登录服务器 URL（必须 https://）
async fn handle_info(params: &Option<Value>) -> ApiResult {
    // 校验 serverUrl 协议
    if let Some(server_url) = params
        .as_ref()
        .and_then(|p| p.get("serverUrl"))
        .and_then(|v| v.as_str())
    {
        if !server_url.is_empty() && !server_url.starts_with("https://") {
            return ApiResult::err(400, "第三方登录服务器必须使用 HTTPS 协议");
        }
    }

    // 拉取官方最新版本信息
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.get(AUTHLIB_ARTIFACT_URL).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return ApiResult::err(502, "获取 authlib-injector 信息失败");
            }
            match resp.json::<Value>().await {
                Ok(data) => ApiResult::ok(json!({
                    "version": data.get("version").and_then(|v| v.as_str()).unwrap_or("unknown"),
                    "downloadUrl": data.get("download_url").and_then(|v| v.as_str()).unwrap_or(""),
                    "size": data.get("size").and_then(|v| v.as_u64()).unwrap_or(0)
                })),
                Err(_) => ApiResult::err(502, "解析 authlib-injector 信息失败"),
            }
        }
        Err(_) => ApiResult::err(502, "无法连接 authlib-injector 服务器"),
    }
}

/// GET /api/authlib-injector/download — 下载 jar 并校验
///
/// 查询参数：
///   - serverUrl: 可选，第三方登录服务器 URL（必须 https://）
///
/// 返回：{ success: true, path, version }
async fn handle_download(params: &Option<Value>) -> ApiResult {
    // 1. 校验 serverUrl 协议
    if let Some(server_url) = params
        .as_ref()
        .and_then(|p| p.get("serverUrl"))
        .and_then(|v| v.as_str())
    {
        if !server_url.is_empty() && !server_url.starts_with("https://") {
            return ApiResult::err(400, "第三方登录服务器必须使用 HTTPS 协议");
        }
    }

    // 2. 复用公共函数完成下载与校验
    match ensure_authlib_injector().await {
        Ok((jar_path, version)) => ApiResult::ok(json!({
            "success": true,
            "path": jar_path.to_string_lossy(),
            "version": version
        })),
        Err(msg) => ApiResult::err(502, &msg),
    }
}

/// 确保 authlib-injector jar 文件存在，不存在则下载并校验
///
/// 逻辑：
///   1. 扫描 dataDir/authlib-injector/ 目录，若已有 .jar 文件则直接返回
///   2. 否则拉取最新版本元数据，下载到目标路径
///   3. 校验 SHA256（若元数据包含 checksums.sha256），不匹配则删除文件并报错
///
/// 返回：(jar_path, version)
pub async fn ensure_authlib_injector() -> Result<(PathBuf, String), String> {
    let data_dir = storage::resolve_data_dir();
    let ai_dir = data_dir.join("authlib-injector");
    if let Err(e) = std::fs::create_dir_all(&ai_dir) {
        return Err(format!("无法创建目录: {}", e));
    }

    // 1. 已有 jar 文件则直接返回
    if let Ok(entries) = std::fs::read_dir(&ai_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jar") {
                let version = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("authlib-injector-"))
                    .unwrap_or("unknown")
                    .to_string();
                return Ok((p, version));
            }
        }
    }

    // 2. 拉取最新版本元数据
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("VersePC/1.0")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let artifact = client
        .get(AUTHLIB_ARTIFACT_URL)
        .send()
        .await
        .map_err(|e| format!("无法连接 authlib-injector 服务器: {}", e))?;

    if !artifact.status().is_success() {
        return Err("无法获取 authlib-injector 版本信息".to_string());
    }

    let artifact: Value = artifact
        .json()
        .await
        .map_err(|_| "解析 authlib-injector 版本信息失败".to_string())?;

    let version = artifact
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let download_url = artifact
        .get("download_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let expected_sha256 = artifact
        .get("checksums")
        .and_then(|c| c.get("sha256"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if download_url.is_empty() {
        return Err("authlib-injector 下载地址为空".to_string());
    }

    let jar_path = ai_dir.join(format!("authlib-injector-{}.jar", version));
    eprintln!("[authlib] 下载 {} → {}", download_url, jar_path.display());

    // 3. 下载文件
    let download_resp = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("下载失败: {}", e))?;

    if !download_resp.status().is_success() {
        return Err("下载请求失败".to_string());
    }

    let bytes = download_resp
        .bytes()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?;

    // 4. 校验 SHA256
    if !expected_sha256.is_empty() {
        let actual_hash = compute_sha256(&bytes);
        if actual_hash != expected_sha256 {
            return Err("authlib-injector 文件校验失败，请重试".to_string());
        }
        eprintln!("[authlib] SHA256 校验通过");
    }

    // 5. 写入文件
    if let Err(e) = std::fs::write(&jar_path, &bytes) {
        return Err(format!("写入文件失败: {}", e));
    }

    Ok((jar_path, version))
}

/// 计算字节的 SHA256 哈希
fn compute_sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
