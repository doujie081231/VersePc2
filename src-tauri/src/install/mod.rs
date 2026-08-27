// install/mod.rs — 安装编排模块入口
// 职责：协调版本安装流程，调用 download 模块下载文件，管理安装会话

pub mod session;

use std::path::PathBuf;
use std::sync::Arc;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::download;
use crate::modloaders;
use crate::storage;
use crate::utils;

/// 安装阶段权重
const STAGE_WEIGHTS: &[(session::InstallStage, u32)] = &[
    (session::InstallStage::VersionJson, 1),
    (session::InstallStage::ClientJar, 5),
    (session::InstallStage::Libraries, 15),
    (session::InstallStage::Natives, 1),
    (session::InstallStage::Assets, 20),
    (session::InstallStage::Loader, 10),
    (session::InstallStage::Finalizing, 1),
];

/// 计算累计进度（0-99）
fn calc_progress(current_stage: &session::InstallStage, stage_pct: u32) -> u32 {
    let mut total = 0u32;
    let total_weight: u32 = STAGE_WEIGHTS.iter().map(|(_, w)| w).sum();
    let mut in_current = false;
    for (stage, weight) in STAGE_WEIGHTS {
        if stage == current_stage {
            in_current = true;
            total += weight * stage_pct.min(100) / 100;
            break;
        }
        total += weight;
    }
    if !in_current {
        return 99;
    }
    (total * 99 / total_weight.max(1)).min(99)
}

/// 安装入口
pub async fn perform_installation(
    app: AppHandle,
    session_id: String,
    version_id: String,
    version_json_url: String,
    custom_name: Option<String>,
    loader_info: Option<Value>,
    download_source_arg: Option<String>,
    cancel_flag: Arc<std::sync::atomic::AtomicBool>,
) {
    let settings = storage::load_settings();
    // 下载源：优先使用前端传入的值，为空时回退到设置值
    let download_source = download_source_arg
        .unwrap_or_else(|| utils::get_str(&settings, "downloadSource"));
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let libraries_dir = data_dir.join("libraries");
    let assets_dir = data_dir.join("assets");

    // 实际版本 ID（支持自定义名，如 "26.2-Forge-65.1.0"）
    let actual_version_id = custom_name.unwrap_or_else(|| version_id.clone());

    macro_rules! update {
        ($stage:expr, $pct:expr, $msg:expr) => {{
            session::update_session(&app, &session_id, |s| {
                s.stage = $stage;
                s.progress = calc_progress(&$stage, $pct);
                s.message = $msg.to_string();
            });
        }};
        ($stage:expr, $pct:expr, $msg:expr, $file:expr) => {{
            session::update_session(&app, &session_id, |s| {
                s.stage = $stage.clone();
                s.progress = calc_progress(&$stage, $pct);
                s.message = $msg.to_string();
                s.current_file = $file.to_string();
            });
        }};
    }

    // 检查取消
    macro_rules! check_cancel {
        () => {
            if session::is_cancelled(&cancel_flag) {
                update!(session::InstallStage::Cancelled, 0, "已取消");
                return;
            }
        };
    }

    // 阶段 1：拉取版本 JSON
    update!(session::InstallStage::VersionJson, 0, "获取版本信息...");
    let version_details = match fetch_json(&version_json_url, &download_source).await {
        Ok(v) => v,
        Err(e) => {
            session::update_session(&app, &session_id, |s| {
                s.stage = session::InstallStage::Failed;
                s.message = format!("获取版本信息失败: {}", e);
                s.errors.push(e);
            });
            return;
        }
    };

    // 写入版本 JSON（原版基础文件写入基础版本目录，如 versions/26.2，
    // 这样 Forge/NeoForge 检查"原版已安装"时能找到；加载器版本目录由安装器生成）
    // 基础版本号：整合包用 inheritsFrom，普通版本用原始下载版本号。
    // 注意不能回退到 actual_version_id（自定义名，如 "26.2-Forge-65.1.0"），
    // 否则原版会被装到带加载器后缀的目录，导致 Forge 找不到原版。
    let game_version = version_details
        .get("inheritsFrom")
        .and_then(|v| v.as_str())
        .unwrap_or(&version_id)
        .to_string();
    let version_dir = versions_dir.join(&game_version);
    let version_json_path = version_dir.join(format!("{}.json", game_version));
    if let Err(e) = tokio::fs::create_dir_all(&version_dir).await {
        session::update_session(&app, &session_id, |s| {
            s.stage = session::InstallStage::Failed;
            s.message = format!("创建版本目录失败: {}", e);
        });
        return;
    }
    let json_str = serde_json::to_string_pretty(&version_details).unwrap_or_default();
    if let Err(e) = tokio::fs::write(&version_json_path, &json_str).await {
        session::update_session(&app, &session_id, |s| {
            s.stage = session::InstallStage::Failed;
            s.message = format!("写入版本 JSON 失败: {}", e);
        });
        return;
    }
    update!(session::InstallStage::VersionJson, 100, "版本信息已获取");
    check_cancel!();

    // 阶段 2：下载 client.jar
    update!(session::InstallStage::ClientJar, 0, "下载客户端文件...");
    let client_jar_url = version_details
        .pointer("/downloads/client/url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_jar_sha1 = version_details
        .pointer("/downloads/client/sha1")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_jar_size = version_details
        .pointer("/downloads/client/size")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let client_jar_path = version_dir.join(format!("{}.jar", game_version));

    if !client_jar_url.is_empty() {
        let version_id_for_cb = actual_version_id.clone();
        let app_for_cb = app.clone();
        let session_id_for_cb = session_id.clone();
        let stage_for_cb = session::InstallStage::ClientJar;
        let progress_cb: download::ProgressCb = Arc::new(move |p: &download::DownloadProgress| {
            session::update_session(&app_for_cb, &session_id_for_cb, |s| {
                s.stage = stage_for_cb.clone();
                s.progress = calc_progress(&stage_for_cb, ((p.bytes_downloaded as f64 / p.total_bytes.max(1) as f64) * 100.0) as u32);
                s.bytes_downloaded = p.bytes_downloaded;
                s.total_bytes = p.total_bytes;
                s.speed = p.speed;
                s.current_file = format!("{}.jar", version_id_for_cb);
            });
        });

        match download::download_with_mirror(
            &client_jar_url,
            &client_jar_path,
            if client_jar_sha1.is_empty() { None } else { Some(&client_jar_sha1) },
            if client_jar_size > 0 { Some(client_jar_size) } else { None },
            &download_source,
            300,
            Some(progress_cb),
        ).await {
            Ok(()) => update!(session::InstallStage::ClientJar, 100, "客户端文件已下载"),
            Err(e) => {
                session::update_session(&app, &session_id, |s| {
                    s.stage = session::InstallStage::Failed;
                    s.message = format!("下载客户端文件失败: {}", e);
                    s.errors.push(e);
                });
                return;
            }
        }
    }
    check_cancel!();

    // 阶段 3：下载 libraries（并发，最多 16 个）
    update!(session::InstallStage::Libraries, 0, "下载依赖库...");
    let libraries = merge_inherited_libraries(&version_details, &versions_dir);
    let valid_libs: Vec<Value> = libraries.into_iter().filter(|lib| evaluate_rules(lib)).collect();
    let total_libs = valid_libs.len() as u32;

    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use futures_util::stream::{self, StreamExt};
    let completed_libs = Arc::new(AtomicU32::new(0));
    let failed_libs = Arc::new(AtomicU32::new(0));

    let lib_results: Vec<_> = stream::iter(valid_libs.into_iter())
        .map(|lib| {
            let libraries_dir = libraries_dir.clone();
            let download_source = download_source.clone();
            let app = app.clone();
            let session_id = session_id.clone();
            let completed = completed_libs.clone();
            let failed = failed_libs.clone();
            let cancel_flag = cancel_flag.clone();
            async move {
                if session::is_cancelled(&cancel_flag) {
                    return;
                }
                let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                let done = completed.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                session::update_session(&app, &session_id, |s| {
                    s.progress = calc_progress(&session::InstallStage::Libraries, (done * 100) / total_libs.max(1));
                    s.message = format!("下载库 ({}/{})", done, total_libs);
                    s.current_file = name.clone();
                    s.completed_files = done;
                    s.total_files = total_libs;
                });
                if let Err(e) = download_library(&lib, &libraries_dir, &download_source, &app, &session_id).await {
                    failed.fetch_add(1, AtomicOrdering::SeqCst);
                    session::update_session(&app, &session_id, |s| {
                        s.errors.push(format!("库 {} 下载失败: {}", name, e));
                    });
                }
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    drop(lib_results);
    let failed_total = failed_libs.load(AtomicOrdering::SeqCst);
    if failed_total > 0 {
        let fail_msg = format!("依赖库下载失败 {} 个，请检查网络或稍后重试", failed_total);
        session::update_session(&app, &session_id, |s| {
            s.stage = session::InstallStage::Failed;
            s.progress = calc_progress(&session::InstallStage::Libraries, 100);
            s.message = fail_msg;
        });
        return;
    }
    update!(session::InstallStage::Libraries, 100, "依赖库已下载");
    check_cancel!();

    // 阶段 4：下载 assets
    update!(session::InstallStage::Assets, 0, "下载资源文件...");
    if let Some(asset_index) = version_details.get("assetIndex") {
        let asset_index_url = asset_index.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let asset_index_id = asset_index.get("id").and_then(|v| v.as_str()).unwrap_or("legacy");
        let asset_index_sha1 = asset_index.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let asset_index_path = assets_dir.join("indexes").join(format!("{}.json", asset_index_id));

        if !asset_index_url.is_empty() {
            // 下载资源索引
            if let Err(e) = download::download_with_mirror(
                asset_index_url,
                &asset_index_path,
                if asset_index_sha1.is_empty() { None } else { Some(asset_index_sha1) },
                None,
                &download_source,
                60,
                None,
            ).await {
                session::update_session(&app, &session_id, |s| {
                    s.errors.push(format!("资源索引下载失败: {}", e));
                });
            }

            // 解析并批量下载缺失资源
            if let Ok(index_content) = tokio::fs::read_to_string(&asset_index_path).await {
                if let Ok(index_json) = serde_json::from_str::<Value>(&index_content) {
                    if let Some(objects) = index_json.get("objects").and_then(|v| v.as_object()) {
                        let mut asset_objects: Vec<download::AssetObject> = Vec::with_capacity(objects.len());
                        for (name, info) in objects {
                            let hash = info.get("hash").and_then(|v| v.as_str()).unwrap_or("");
                            let size = info.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                            if hash.is_empty() { continue; }
                            let prefix = &hash[..2];
                            let path = assets_dir.join("objects").join(prefix).join(hash);
                            if !path.exists() {
                                asset_objects.push(download::AssetObject {
                                    name: name.clone(),
                                    hash: hash.to_string(),
                                    size,
                                });
                            }
                        }

                        let total_assets = asset_objects.len() as u32;
                        if total_assets > 0 {
                            let sources = download::select_asset_sources(&download_source).await;
                            let max_parallel = settings
                                .get("maxThreads")
                                .and_then(|v| v.as_u64())
                                .map(|n| n as usize)
                                .unwrap_or(32)
                                .min(64)
                                .max(8);

                            let app_progress = app.clone();
                            let session_id_progress = session_id.clone();
                            let total_assets_usize = total_assets as usize;
                            let (done, failed) = download::download_asset_objects(
                                asset_objects,
                                &assets_dir,
                                &sources,
                                max_parallel,
                                move |done, _total, name| {
                                    session::update_session(&app_progress, &session_id_progress, |s| {
                                        s.progress = calc_progress(&session::InstallStage::Assets, (done as u32 * 100) / total_assets.max(1));
                                        s.message = format!("下载资源 ({}/{})", done, total_assets_usize);
                                        s.completed_files = done as u32;
                                        s.total_files = total_assets;
                                        s.current_file = name.to_string();
                                    });
                                },
                            ).await;

                            if failed > 0 {
                                session::update_session(&app, &session_id, |s| {
                                    s.errors.push(format!("{} 个资源文件下载失败", failed));
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    update!(session::InstallStage::Assets, 100, "资源文件已下载");
    check_cancel!();

    // 阶段 5：安装模组加载器
    // 用户在前端下载页面选择了加载器（Forge/NeoForge/Fabric/OptiFine）时触发
    if let Some(loader) = &loader_info {
        let loader_type = loader.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let loader_version = loader.get("version").and_then(|v| v.as_str()).unwrap_or("");
        if !loader_type.is_empty() && !loader_version.is_empty() {
            update!(session::InstallStage::Loader, 5, "准备安装模组加载器...");
            let loader_display = if loader_type == "neoforge" {
                "NeoForge".to_string()
            } else {
                let mut s = loader_type.to_string();
                if !s.is_empty() {
                    s[0..1].make_ascii_uppercase();
                }
                s
            };
            update!(session::InstallStage::Loader, 15, &format!("正在安装{}模组加载器...", loader_display));

            let mut result = json!({ "success": false, "error": "未知加载器类型" });
            match loader_type {
                "forge" => {
                    result = modloaders::forge::install_forge(
                        &game_version,
                        &loader_version,
                        Some(&actual_version_id),
                    ).await;
                }
                "neoforge" => {
                    result = modloaders::neoforge::install_neoforge(
                        &game_version,
                        &loader_version,
                        Some(&actual_version_id),
                    ).await;
                }
                "fabric" => {
                    result = modloaders::fabric::install_fabric_with_target(
                        &game_version,
                        &loader_version,
                        &actual_version_id,
                    ).await;
                }
                "optifine" => {
                    result = modloaders::optifine::install_optifine(
                        &game_version,
                        &loader_version,
                        Some(&actual_version_id),
                    ).await;
                }
                _ => {}
            }

            let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);

            // 记录加载器安装日志到文件，便于排查失败原因
            {
                let log_dir = data_dir.join("logs");
                let _ = std::fs::create_dir_all(&log_dir);
                let log_path = log_dir.join("loader-install.log");
                let line = format!(
                    "[{}] type={} version={} game={} target={} success={} error={}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                    loader_type,
                    loader_version,
                    game_version,
                    actual_version_id,
                    success,
                    result.get("error").and_then(|v| v.as_str()).unwrap_or("")
                );
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                    let _ = writeln!(f, "{}", line);
                }
            }

            if !success {
                let err = result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("模组加载器安装失败")
                    .to_string();
                session::update_session(&app, &session_id, |s| {
                    s.stage = session::InstallStage::Failed;
                    s.message = format!("{}安装失败: {}", loader_display, err);
                    s.errors.push(err);
                });
                return;
            }
            update!(session::InstallStage::Loader, 100, "模组加载器安装完成");

            // Fabric API 自动安装：detail 流程选择 Fabric 加载器时可选一并安装 Fabric API
            if loader_type == "fabric" {
                let api_id = loader.get("fabricApiId").and_then(|v| v.as_str()).unwrap_or("");
                if !api_id.is_empty() {
                    update!(session::InstallStage::Loader, 100, "正在安装 Fabric API...");
                    let api_url = loader.get("fabricApiUrl").and_then(|v| v.as_str()).unwrap_or("");
                    let api_filename = loader.get("fabricApiFilename").and_then(|v| v.as_str()).unwrap_or("");
                    let api_result = crate::modloaders::fabric_api::install_fabric_api(
                        &game_version,
                        api_id,
                        Some(&actual_version_id),
                        if api_url.is_empty() { None } else { Some(api_url) },
                        if api_filename.is_empty() { None } else { Some(api_filename) },
                    ).await;
                    if !api_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        let api_err = api_result.get("error").and_then(|v| v.as_str()).unwrap_or("Fabric API 安装失败").to_string();
                        session::update_session(&app, &session_id, |s| {
                            s.stage = session::InstallStage::Failed;
                            s.message = format!("Fabric API 安装失败: {}", api_err);
                            s.errors.push(api_err);
                        });
                        return;
                    }
                }
            }
        }
    }
    check_cancel!();

    // 完成
    session::update_session(&app, &session_id, |s| {
        s.stage = session::InstallStage::Completed;
        s.progress = 100;
        s.message = "安装完成".to_string();
        s.current_file = String::new();
    });

    // 5 秒后清理会话
    let sid = session_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        session::remove_session(&sid);
    });
}

/// 拉取 JSON（带镜像回退）
async fn fetch_json(url: &str, download_source: &str) -> Result<Value, String> {
    use crate::download::mirror;

    let urls = mirror::get_mirror_urls(url, download_source);
    let mut last_err = String::new();

    for u in &urls {
        match reqwest::get(u).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<Value>().await {
                        Ok(v) => {
                            if u != url {
                                mirror::mirror_success();
                            }
                            return Ok(v);
                        }
                        Err(e) => last_err = format!("解析 JSON 失败: {}", e),
                    }
                } else {
                    last_err = format!("HTTP {}", resp.status());
                }
            }
            Err(e) => {
                last_err = format!("请求失败: {}", e);
                if u != url {
                    mirror::mirror_failed();
                }
            }
        }
    }
    Err(last_err)
}

/// 评估库的 rules（平台兼容性）
/// 递归合并版本及其继承版本（inheritsFrom）的 libraries，供依赖库下载使用。
/// 顺序：子版本在前、父版本在后；按 name 去重，保留先出现的（子版本优先）。
fn merge_inherited_libraries(json: &Value, versions_dir: &std::path::Path) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_libraries(json, versions_dir, &mut out, &mut seen, &mut visited);
    out
}

/// 自底向下递归收集 libraries（子在前、父在后）
fn collect_libraries(
    json: &Value,
    versions_dir: &std::path::Path,
    out: &mut Vec<Value>,
    seen: &mut std::collections::HashSet<String>,
    visited: &mut std::collections::HashSet<String>,
) {
    if let Some(libs) = json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
                if seen.insert(name.to_string()) {
                    out.push(lib.clone());
                }
            } else {
                out.push(lib.clone());
            }
        }
    }
    if let Some(parent_id) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
        if visited.insert(parent_id.to_string()) {
            if let Some(parent) = load_local_version_json(parent_id, versions_dir) {
                collect_libraries(&parent, versions_dir, out, seen, visited);
            }
        }
    }
}

/// 从本地版本目录读取某版本的 json
fn load_local_version_json(version_id: &str, versions_dir: &std::path::Path) -> Option<Value> {
    let path = versions_dir
        .join(version_id)
        .join(format!("{}.json", version_id));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn evaluate_rules(lib: &Value) -> bool {
    let rules = match lib.get("rules").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return true, // 无 rules 默认允许
    };

    let os_name = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(os) = rule.get("os").and_then(|v| v.as_object()) {
            let rule_os = os.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if rule_os == os_name {
                allowed = action == "allow";
            }
        } else {
            // 无 os 限制的规则
            allowed = action == "allow";
        }
    }
    allowed
}

/// 下载单个库文件
async fn download_library(
    lib: &Value,
    libraries_dir: &PathBuf,
    download_source: &str,
    _app: &AppHandle,
    _session_id: &str,
) -> Result<(), String> {
    // 优先用 downloads.artifact
    if let Some(artifact) = lib.pointer("/downloads/artifact") {
        let url = artifact.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let path = artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let sha1 = artifact.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
        let size = artifact.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        if url.is_empty() || path.is_empty() {
            return Ok(()); // 跳过无 URL 的库
        }

        let dest = libraries_dir.join(path);
        // 安全检查：路径不能逃出 libraries_dir
        if !dest.starts_with(libraries_dir) {
            return Err("路径越界".to_string());
        }

        return download::download_with_mirror(
            url,
            &dest,
            if sha1.is_empty() { None } else { Some(sha1) },
            if size > 0 { Some(size) } else { None },
            download_source,
            120,
            None,
        ).await;
    }

    // 无 artifact，按 maven 坐标构造 URL
    let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if name.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return Ok(());
    }
    let group = parts[0];
    let artifact_id = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };

    let group_path = group.replace('.', "/");
    let jar_name = format!("{}-{}{}.jar", artifact_id, version, classifier);
    let rel_path = format!("{}/{}/{}/{}", group_path, artifact_id, version, jar_name);
    let dest = libraries_dir.join(&rel_path);

    // 选择 base URL
    let base_url = if group.contains("fabric") || group.contains("fabricmc") {
        "https://maven.fabricmc.net/"
    } else if group.contains("neoforged") {
        "https://maven.neoforged.net/"
    } else if group.contains("forge") || group.contains("minecraftforge") || group.starts_with("net.minecraft") {
        "https://maven.minecraftforge.net/"
    } else {
        "https://libraries.minecraft.net/"
    };
    let url = format!("{}{}", base_url, rel_path);

    download::download_with_mirror(
        &url,
        &dest,
        None,
        None,
        download_source,
        120,
        None,
    ).await
}
