// api/cloud.rs — Verse 云盘
// 云盘文件服务部署在作者本机（经 SSH 反向隧道 + 服务器 nginx 以 /cloudapi/ 反代）
// 本模块提供：登录中继 / cloudapi 通用中继 / 打包版本存档 / 流式上传 / 下载并解压到版本存档目录
use super::ApiResult;
use serde_json::{json, Value};
use tauri::Emitter;
use std::io::Read;

const CLOUD_ORIGIN: &str = "https://www.verselauncher.cn";
const CLOUD_API: &str = "https://www.verselauncher.cn/cloudapi";

/// api_proxy 路由入口
pub async fn handle(
    app: &tauri::AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    if !path.starts_with("/api/cloud") {
        return None;
    }
    let _ = params;
    match (method.to_uppercase().as_str(), path) {
        ("POST", "/api/cloud/login") => Some(cloud_login(body).await),
        ("POST", "/api/cloud/relay") => Some(cloud_relay(body).await),
        ("POST", "/api/cloud/pack-saves") => Some(pack_saves(body)),
        ("POST", "/api/cloud/upload") => Some(cloud_upload(app.clone(), body).await),
        ("POST", "/api/cloud/download") => Some(cloud_download(app.clone(), body).await),
        _ => Some(ApiResult::err(404, &format!("未实现: {}", path))),
    }
}

fn bv(body: &Option<Value>, key: &str) -> String {
    body.as_ref()
        .and_then(|b| b.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// 清洗外部版本标记 "xxx [外部N]" → "xxx"（与 versions.rs 同规则）
fn clean_external_marker(version_id: &str) -> String {
    if let Some(idx) = version_id.find(" [外部") {
        version_id[..idx].to_string()
    } else if let Some(idx) = version_id.find("[外部") {
        version_id[..idx].trim_end().to_string()
    } else {
        version_id.to_string()
    }
}

/// 解析版本存档目录（考虑版本隔离设置）
fn resolve_saves_dir(version_id: &str) -> std::path::PathBuf {
    let clean_id = clean_external_marker(version_id);
    let data_dir = crate::storage::resolve_data_dir();
    let settings = crate::storage::load_settings();
    let isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if isolation {
        data_dir.join("versions").join(clean_id).join("saves")
    } else {
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if game_dir.is_empty() {
            dirs::home_dir()
                .map(|h| h.join(".minecraft").join("saves"))
                .unwrap_or_else(|| data_dir.join("versions").join(clean_id).join("saves"))
        } else {
            std::path::PathBuf::from(game_dir).join("saves")
        }
    }
}

/// POST /api/cloud/login {uid,password} → 官网签发云盘token
async fn cloud_login(body: &Option<Value>) -> ApiResult {
    let uid = bv(body, "uid");
    let password = bv(body, "password");
    if uid.is_empty() || password.is_empty() {
        return ApiResult::err(400, "请输入UID和密码");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let resp = client
        .post(format!("{}/api/cloud/pc-login", CLOUD_ORIGIN))
        .json(&json!({ "uid": uid, "password": password }))
        .send()
        .await;
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let j: Value = r.json().await.unwrap_or_else(|_| json!({"ok": false, "msg": "响应解析失败"}));
            ApiResult::ok(json!({ "status": status, "body": j }))
        }
        Err(e) => ApiResult::err(502, &format!("无法连接官网：{}", e)),
    }
}

/// POST /api/cloud/relay {method,path,token,body} → cloudapi 小请求中继（overview/redeem/delete）
async fn cloud_relay(body: &Option<Value>) -> ApiResult {
    let token = bv(body, "token");
    let sub = bv(body, "path");
    let rmethod = {
        let m = bv(body, "method");
        if m.is_empty() { "GET".to_string() } else { m.to_uppercase() }
    };
    let rbody = body
        .as_ref()
        .and_then(|b| b.get("body"))
        .cloned()
        .filter(|v| !v.is_null());
    if token.is_empty() || sub.is_empty() || !sub.starts_with('/') {
        return ApiResult::err(400, "参数不完整");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut req = match rmethod.as_str() {
        "POST" => client.post(format!("{}{}", CLOUD_API, sub)),
        _ => client.get(format!("{}{}", CLOUD_API, sub)),
    };
    req = req.header("authorization", format!("Bearer {}", token));
    if rbody.is_some() {
        req = req.json(&rbody.unwrap_or(json!({})));
    }
    match req.send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let j: Value = r.json().await.unwrap_or_else(|_| json!({"ok": false, "msg": "响应解析失败"}));
            ApiResult::ok(json!({ "status": status, "body": j }))
        }
        Err(e) => ApiResult::err(502, &format!("云盘服务无法连接：{}", e)),
    }
}

/// POST /api/cloud/pack-saves {versionId} → 把版本的 saves/ 打包为 zip，返回临时文件路径
fn pack_saves(body: &Option<Value>) -> ApiResult {
    let version_id = bv(body, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "缺少版本ID");
    }
    let saves_dir = resolve_saves_dir(&version_id);
    if !saves_dir.is_dir() {
        return ApiResult::err(400, "该版本没有存档目录");
    }
    let entries = match std::fs::read_dir(&saves_dir) {
        Ok(e) => e,
        Err(e) => return ApiResult::err(500, &format!("读取存档目录失败：{}", e)),
    };
    let mut worlds: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                worlds.push(name.to_string());
            }
        }
    }
    if worlds.is_empty() {
        return ApiResult::err(400, "该版本没有可上传的存档");
    }

    // 输出到系统临时目录
    let zip_path = std::env::temp_dir().join(format!(
        "verse-cloud-{}.zip",
        uuid::Uuid::new_v4().simple()
    ));

    match create_saves_zip(&saves_dir, &zip_path) {
        Ok(size) => ApiResult::ok(json!({
            "path": zip_path.to_string_lossy(),
            "size": size,
            "worlds": worlds,
            "versionId": clean_external_marker(&version_id),
        })),
        Err(e) => ApiResult::err(500, &format!("打包存档失败：{}", e)),
    }
}

/// 递归打包 saves 目录（zip 内为世界文件夹结构，下载解压后直接还原）
fn create_saves_zip(saves_dir: &std::path::Path, zip_path: &std::path::Path) -> std::io::Result<u64> {
    use std::io::{Read, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;
    use zip::ZipWriter;

    let file = std::fs::File::create(zip_path)?;
    let mut total: u64 = 0;
    {
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut stack: Vec<(std::path::PathBuf, String)> = vec![(saves_dir.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            let entries = std::fs::read_dir(&dir)?;
            for entry in entries.flatten() {
                let path = entry.path();
                let name = match entry.file_name().to_str() {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
                if path.is_dir() {
                    zip.add_directory(rel.clone(), options)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    stack.push((path.clone(), rel));
                } else {
                    let mut f = std::fs::File::open(&path)?;
                    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                    zip.start_file(rel.clone(), options)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
                    let mut buf = vec![0u8; 256 * 1024];
                    loop {
                        let n = f.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        zip.write_all(&buf[..n])?;
                        total += n as u64;
                    }
                    let _ = size;
                }
            }
        }
        zip.finish()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    }
    Ok(std::fs::metadata(zip_path)?.len())
}

/// POST /api/cloud/upload {path,name,token} → 流式上传 zip 到 cloudapi
async fn cloud_upload(app: tauri::AppHandle, body: &Option<Value>) -> ApiResult {
    let file_path = bv(body, "path");
    let name = bv(body, "name");
    let token = bv(body, "token");
    if file_path.is_empty() || token.is_empty() {
        return ApiResult::err(400, "参数不完整");
    }
    let is_temp_zip = file_path.contains("verse-cloud-") && file_path.ends_with(".zip");
    let cleanup_path = if is_temp_zip { Some(file_path.clone()) } else { None };
    let _ = app.emit("cloud:progress", json!({ "stage": "upload", "percent": 0, "message": "开始上传..." }));

    let result = tauri::async_runtime::spawn_blocking(move || {
        let file = match std::fs::File::open(&file_path) {
            Ok(f) => f,
            Err(e) => return ApiResult::err(500, &format!("打开临时文件失败：{}", e)),
        };
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3600))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        let resp = client
            .post(format!("{}/upload", CLOUD_API))
            .header("authorization", format!("Bearer {}", token))
            .header("x-file-name", urlencoding::encode(&name).to_string())
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::blocking::Body::from(file))
            .send();
        match resp {
            Ok(r) => {
                let status = r.status().as_u16();
                let j: Value = r.json().unwrap_or_else(|_| json!({"ok": false, "msg": "响应解析失败"}));
                ApiResult::ok(json!({ "status": status, "body": j }))
            }
            Err(e) => ApiResult::err(502, &format!("上传失败：{}", e)),
        }
    })
    .await
    .unwrap_or_else(|e| ApiResult::err(500, &format!("上传任务异常：{}", e)));

    let _ = app.emit("cloud:progress", json!({ "stage": "upload", "percent": 100, "message": "上传完成" }));
    // 清理打包产生的临时 zip（仅限本模块生成的命名）
    if let Some(p) = cleanup_path {
        let _ = std::fs::remove_file(&p);
    }
    result
}

/// POST /api/cloud/download {fileId,name,token,versionId} → 下载 zip 并解压到版本存档目录
async fn cloud_download(app: tauri::AppHandle, body: &Option<Value>) -> ApiResult {
    let file_id = bv(body, "fileId");
    let file_name = bv(body, "name");
    let token = bv(body, "token");
    let version_id = bv(body, "versionId");
    if file_id.is_empty() || token.is_empty() || version_id.is_empty() {
        return ApiResult::err(400, "参数不完整");
    }
    if !file_id.chars().all(|c| c.is_ascii_hexdigit()) || file_id.len() != 24 {
        return ApiResult::err(400, "文件ID不合法");
    }

    let saves_dir = resolve_saves_dir(&version_id);
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _ = app2.emit("cloud:progress", json!({ "stage": "download", "percent": 0, "message": "开始下载..." }));
        // 1. 下载到临时文件（带进度）
        let tmp_dir = match tempfile::Builder::new().prefix("verse-cloud-dl-").tempdir() {
            Ok(t) => t,
            Err(e) => return ApiResult::err(500, &format!("创建临时目录失败：{}", e)),
        };
        let zip_path = tmp_dir.path().join("download.zip");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3600))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        let mut resp = match client
            .get(format!("{}/files/{}", CLOUD_API, file_id))
            .header("authorization", format!("Bearer {}", token))
            .send()
        {
            Ok(r) => r,
            Err(e) => return ApiResult::err(502, &format!("下载失败：{}", e)),
        };
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let msg = resp.json::<Value>().ok()
                .and_then(|j| j.get("msg").and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| format!("HTTP {}", status));
            return ApiResult::err(status, &msg);
        }
        let total = resp.content_length().unwrap_or(0);
        let mut out = match std::fs::File::create(&zip_path) {
            Ok(f) => f,
            Err(e) => return ApiResult::err(500, &format!("写临时文件失败：{}", e)),
        };
        use std::io::Write;
        let mut received: u64 = 0;
        let mut last_emit: u64 = 0;
        let mut buf = [0u8; 256 * 1024];
        loop {
            let n = match resp.read(&mut buf) {
                Ok(n) => n,
                Err(e) => return ApiResult::err(502, &format!("下载中断：{}", e)),
            };
            if n == 0 {
                break;
            }
            if out.write_all(&buf[..n]).is_err() {
                return ApiResult::err(500, "写临时文件失败");
            }
            received += n as u64;
            if total > 0 && received - last_emit > 1024 * 1024 {
                last_emit = received;
                let _ = app2.emit("cloud:progress", json!({
                    "stage": "download",
                    "percent": (received as f64 / total as f64 * 100.0).min(99.0),
                    "message": format!("下载中 {:.1}MB / {:.1}MB", received as f64 / 1048576.0, total as f64 / 1048576.0),
                }));
            }
        }
        let _ = app2.emit("cloud:progress", json!({ "stage": "extract", "percent": 100, "message": "正在解压到版本存档..." }));

        // 2. 解压到版本存档目录（防路径穿越）
        if let Some(parent) = saves_dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::create_dir_all(&saves_dir).map_err(|e| format!("创建存档目录失败：{}", e)).ok();
        let zip_file = match std::fs::File::open(&zip_path) {
            Ok(f) => f,
            Err(e) => return ApiResult::err(500, &format!("打开下载文件失败：{}", e)),
        };
        let mut archive = match zip::ZipArchive::new(zip_file) {
            Ok(a) => a,
            Err(_) => return ApiResult::err(400, "下载内容不是有效的 zip 压缩包"),
        };
        let saves_canon = match saves_dir.canonicalize() {
            Ok(c) => c,
            Err(_) => return ApiResult::err(500, "存档目录不可用"),
        };
        let mut extracted_worlds: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            let mut entry = match archive.by_index(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let raw_name = entry.name().to_string();
            if raw_name.starts_with('/') || raw_name.contains("..") || raw_name.contains(':') {
                continue; // 拒绝可疑路径
            }
            let has_dir = raw_name.contains('/');
            let final_target = if has_dir {
                saves_dir.join(&raw_name)
            } else {
                saves_dir.join("导入的存档").join(&raw_name)
            };
            if let Some(p) = final_target.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            // 目录穿越兜底：目标必须位于存档目录内
            match final_target.parent().and_then(|p| p.canonicalize().ok()) {
                Some(pc) if pc.starts_with(&saves_canon) => {}
                _ => continue,
            }
            if entry.is_dir() {
                let _ = std::fs::create_dir_all(&final_target);
                continue;
            }
            let mut out = match std::fs::File::create(&final_target) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let _ = std::io::copy(&mut entry, &mut out);
            let world = if has_dir {
                raw_name.split('/').next().unwrap_or("").to_string()
            } else {
                "导入的存档".to_string()
            };
            if !world.is_empty() && !extracted_worlds.contains(&world) {
                extracted_worlds.push(world);
            }
        }
        // 记录已解压世界名
        let _ = app2.emit("cloud:progress", json!({ "stage": "done", "percent": 100, "message": "还原完成" }));
        ApiResult::ok(json!({
            "savesDir": saves_dir.to_string_lossy(),
            "worlds": extracted_worlds,
            "fileName": file_name,
        }))
    })
    .await;

    match result {
        Ok(r) => r,
        Err(e) => ApiResult::err(500, &format!("下载任务异常：{}", e)),
    }
}
