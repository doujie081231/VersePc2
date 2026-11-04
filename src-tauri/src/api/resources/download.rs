// api/resources/download.rs — 资源下载与下载会话
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::download::{download_with_mirror_cancellable, resources_session};
use crate::modpack;
use crate::storage;

use super::{MODRINTH_API, CURSEFORGE_API, DEFAULT_CF_API_KEY, cached_fetch_json, fetch_json_with_mirror};

/// POST /api/resources/download — 下载资源
///
/// 请求体：
///   - versionId: Modrinth 版本 ID（与 projectId 二选一）
///   - projectId: Modrinth 项目 ID
///   - projectType: 资源类型（mod/modpack/resourcepack/shader/datapack，默认 mod）
///   - savePath: 自定义保存路径（覆盖默认）
///   - customName: 自定义名称（仅 modpack 用作版本 ID）
///   - targetVersionId: 目标 MC 版本 ID（不传则用 selectedVersion）
///   - source: 数据源（modrinth/curseforge，默认 modrinth）
pub(crate) async fn handle_download(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let project_id = body
        .as_ref()
        .and_then(|b| b.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let project_type = body
        .as_ref()
        .and_then(|b| b.get("projectType"))
        .and_then(|v| v.as_str())
        .unwrap_or("mod")
        .to_string();
    let save_path = body
        .as_ref()
        .and_then(|b| b.get("savePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut target_version_id = body
        .as_ref()
        .and_then(|b| b.get("targetVersionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = body
        .as_ref()
        .and_then(|b| b.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth")
        .to_string();
    let custom_name = body
        .as_ref()
        .and_then(|b| b.get("customName"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if version_id.is_empty() && project_id.is_empty() {
        return ApiResult::err(400, "Missing versionId or projectId");
    }

    let settings = storage::load_settings();

    // 解析目标 MC 版本（modpack 不需要）
    if target_version_id.is_empty() && project_type != "modpack" {
        target_version_id = settings
            .get("selectedVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }

    // 决定目标目录
    let dest_dir = if !save_path.is_empty() {
        PathBuf::from(&save_path)
    } else {
        match project_type.as_str() {
            "modpack" => resolve_sub_dir(&settings, &target_version_id, "modpacks"),
            "resourcepack" => resolve_sub_dir(&settings, &target_version_id, "resourcepacks"),
            "shader" => resolve_sub_dir(&settings, &target_version_id, "shaderpacks"),
            "datapack" => resolve_sub_dir(&settings, &target_version_id, "datapacks"),
            _ => {
                // mod → version mods dir
                let Some(mods_dir) = resolve_version_mods_dir(&settings, &target_version_id) else {
                    return ApiResult::err(400, "请先安装一个游戏版本");
                };
                mods_dir
            }
        }
    };

    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        return ApiResult::err(500, &format!("无法创建目录: {}", e));
    }

    // 拉取版本数据 + 项目信息
    let (version_data, project_info) = if source == "curseforge" {
        // CurseForge：需要 modId + fileId，通过文件详情接口获取下载地址
        let cf_api_key = settings
            .get("curseforgeApiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CF_API_KEY);
        let mut cf_headers = reqwest::header::HeaderMap::new();
        if let Ok(v) = reqwest::header::HeaderValue::from_str(cf_api_key) {
            cf_headers.insert("x-api-key", v);
        }
        cf_headers.insert("Accept", reqwest::header::HeaderValue::from_static("application/json"));

        if version_id.is_empty() {
            return ApiResult::err(400, "CurseForge 下载需要指定版本文件 ID");
        }

        let file_detail_url = format!("{}/mods/{}/files/{}", CURSEFORGE_API, project_id, version_id);
        eprintln!("[resources][curseforge] 拉取文件详情: {}", file_detail_url);
        let file_data = match cached_fetch_json(file_detail_url, 0, Some(&cf_headers)).await {
            Ok(v) => v.get("data").cloned().unwrap_or(Value::Null),
            Err(e) => return ApiResult::err(502, &format!("获取文件信息失败: {}", e)),
        };
        if file_data.is_null() {
            return ApiResult::err(502, "未找到该版本文件信息，可能已被下架");
        }

        // CF hashes 是数组 [{algo, value}]，algo=1 表示 sha1
        let sha1 = file_data
            .get("hashes")
            .and_then(|h| h.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|x| x.get("algo").and_then(|a| a.as_u64()).unwrap_or(0) == 1)
            })
            .and_then(|h| h.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let game_versions = file_data
            .get("gameVersions")
            .and_then(|g| g.as_array())
            .cloned()
            .unwrap_or_default();

        // downloadUrl 为空时，根据 fileID(versionId) 和 fileName 构造 CurseForge CDN URL
        // （部分文件 API 返回 downloadUrl 为 null，但 CDN 实际可访问）
        // URL 格式：https://edge.forgecdn.net/files/{fileID前4位}/{fileID剩余位}/{encodeURIComponent(fileName)}
        let raw_dl_url = file_data.get("downloadUrl").and_then(|u| u.as_str()).unwrap_or("").to_string();
        let raw_file_name = file_data.get("fileName").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let mut effective_url = raw_dl_url.clone();
        if effective_url.is_empty() && !raw_file_name.is_empty() && version_id.len() >= 5 {
            let id_str = version_id.clone();
            if id_str.chars().all(|c| c.is_ascii_digit()) {
                let part1 = id_str[..4].parse::<u64>().ok();
                let part2 = id_str[4..].parse::<u64>().ok();
                if let (Some(p1), Some(p2)) = (part1, part2) {
                    let encoded = urlencoding::encode(&raw_file_name);
                    effective_url = format!(
                        "https://edge.forgecdn.net/files/{}/{}/{}",
                        p1, p2, encoded
                    );
                    eprintln!(
                        "[resources][curseforge] downloadUrl 为空，已构造 CDN URL: {}:{} -> {}",
                        project_id, version_id, effective_url
                    );
                }
            }
        }

        let version_data = json!({
            "files": [{
                "url": effective_url,
                "filename": raw_file_name,
                "size": file_data.get("fileLength").and_then(|s| s.as_u64()).unwrap_or(0),
                "primary": true,
                "hashes": { "sha1": sha1 }
            }],
            "game_versions": game_versions
        });

        // modpack 需要项目名作为版本名
        let project_info: Value = if project_type == "modpack" {
            let p_url = format!("{}/mods/{}", CURSEFORGE_API, project_id);
            match cached_fetch_json(p_url, 60000, Some(&cf_headers)).await {
                Ok(v) => v.get("data").cloned().unwrap_or(Value::Null),
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        };

        (version_data, project_info)
    } else {
        // Modrinth（带镜像回退，国内访问更快更稳）
        let version_data: Value = if !version_id.is_empty() {
            let url = format!("{}/version/{}", MODRINTH_API, version_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(v) => v,
                Err(_) => Value::Null,
            }
        } else {
            let url = format!("{}/project/{}/version?limit=1", MODRINTH_API, project_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(arr) => arr.as_array().and_then(|a| a.first()).cloned().unwrap_or(Value::Null),
                Err(_) => Value::Null,
            }
        };

        if version_data.is_null() {
            return ApiResult::err(502, "未找到版本信息，请检查网络连接或稍后重试");
        }

        let project_info: Value = if project_type == "modpack" {
            let url = format!("{}/project/{}", MODRINTH_API, project_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(v) => v,
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        };

        (version_data, project_info)
    };

    // 提取主文件
    let files = version_data.get("files").and_then(|f| f.as_array());
    let primary_file = files
        .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false)))
        .or_else(|| files.and_then(|arr| arr.first()));

    let primary_file = match primary_file {
        Some(f) => f,
        None => return ApiResult::err(502, "未找到下载文件，该版本可能已被下架或不存在"),
    };

    let download_url = primary_file.get("url").and_then(|u| u.as_str()).unwrap_or("");
    let original_filename = primary_file.get("filename").and_then(|f| f.as_str()).unwrap_or("");
    let file_size = primary_file.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
    let expected_sha1 = primary_file
        .get("hashes")
        .and_then(|h| h.get("sha1"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    if download_url.is_empty() {
        return ApiResult::err(502, "下载链接为空，该资源可能暂时不可用");
    }

    // 安全文件名：过滤非法字符
    let default_name = format!("{}.jar", project_id);
    let raw_name = if !original_filename.is_empty() {
        original_filename
    } else {
        &default_name
    };
    let safe_name: String = raw_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let final_name = if safe_name.is_empty() {
        format!("{}.jar", project_id)
    } else {
        safe_name
    };
    let dest_path = dest_dir.join(&final_name);

    // 解析 modpack 元数据
    let mc_version = if project_type == "modpack" {
        version_data
            .get("game_versions")
            .and_then(|g| g.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };
    let pack_name = if project_type == "modpack" {
        project_info
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or(&project_id)
            .to_string()
    } else {
        String::new()
    };

    // 捕获整合包封面图 URL（Modrinth 用 icon_url，CurseForge 用 logo 缩略图），
    // 导入后如果压缩包内没有根图标，则下载该封面作为版本图标
    let modpack_icon_url = if project_type == "modpack" {
        let mr_icon = project_info
            .get("icon_url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !mr_icon.is_empty() {
            mr_icon.to_string()
        } else {
            project_info
                .get("logo")
                .and_then(|l| l.get("thumbnailUrl"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    project_info
                        .get("logo")
                        .and_then(|l| l.get("url"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string()
        }
    } else {
        String::new()
    };

    // 创建下载会话
    let (session_id, cancel_flag) = resources_session::create_session(
        &final_name,
        file_size,
        &project_type,
        &project_id,
    );

    // 写入 modpack 元数据
    if project_type == "modpack" {
        resources_session::update_session_silent(&session_id, |s| {
            s.pack_name = pack_name.clone();
            s.mc_version = mc_version.clone();
        });
    }

    eprintln!(
        "[resources] 开始下载: {} → {} (size={}, sha1={})",
        download_url,
        dest_path.display(),
        file_size,
        if expected_sha1.is_empty() { "(none)" } else { expected_sha1 }
    );

    // spawn 后台下载任务
    let app_handle = app.clone();
    let session_id_for_progress = session_id.clone();
    let session_id_for_task = session_id.clone();
    let project_type_clone = project_type.clone();
    let dest_path_clone = dest_path.clone();
    let custom_name_clone = custom_name.clone();
    let modpack_icon_url_clone = modpack_icon_url.clone();
    let download_url_clone = download_url.to_string();
    let expected_sha1_clone = expected_sha1.to_string();
    // 整合包 mrpack 文件强制使用 china-first（镜像优先，官方兜底），
    // 避免用户设置成 auto 时先连 Modrinth 官方 CDN（国内 TTFB 慢、易限流）导致下载卡住。
    let download_source = if project_type == "modpack" {
        "china-first".to_string()
    } else {
        settings
            .get("downloadSource")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string()
    };

    tokio::spawn(async move {
        let progress_session_id = session_id_for_progress.clone();
        let on_progress: crate::download::ProgressCb = Arc::new(move |p: &crate::download::DownloadProgress| {
            let pct = if p.total_bytes > 0 {
                ((p.bytes_downloaded as f64 / p.total_bytes as f64) * 100.0) as u32
            } else {
                0
            };
            resources_session::update_session_silent(&progress_session_id, |s| {
                if s.status == resources_session::ResourceStage::Cancelled {
                    return;
                }
                s.progress = pct;
                s.download_speed = p.speed;
                s.bytes_downloaded = p.bytes_downloaded;
                s.total_size = p.total_bytes;
                s.downloaded = p.bytes_downloaded;
                let speed_kb = if p.speed > 0 { p.speed / 1024 } else { 0 };
                s.message = format!("下载 {} {}% ({}KB/s)", s.file_name, pct, speed_kb);
                if !s.files.is_empty() {
                    s.files[0].progress = pct;
                    s.files[0].size = p.total_bytes;
                }
            });
        });

        // 调用单流下载（含镜像回退、SHA1 校验、续传、低速检测、取消支持）
        let result = download_with_mirror_cancellable(
            &download_url_clone,
            &dest_path_clone,
            if expected_sha1_clone.is_empty() { None } else { Some(&expected_sha1_clone) },
            if file_size > 0 { Some(file_size) } else { None },
            &download_source,
            300, // 5 分钟超时
            Some(on_progress),
            &cancel_flag,
        )
        .await;

        match result {
            Ok(()) => {
                if project_type_clone == "modpack" {
                    // 下载完成后自动导入整合包
                    // 先更新为导入阶段，再调用 import_modpack 触发安装
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Install;
                        s.progress = 45;
                        s.message = "正在解析整合包...".to_string();
                        s.phase = "install".to_string();
                        if !s.files.is_empty() {
                            s.files[0].status = "downloading".to_string();
                            s.files[0].progress = 100;
                        }
                    });

                    let dest_str = dest_path_clone.to_string_lossy().to_string();
                    let import_result = modpack::import_modpack(
                        &app_handle,
                        &dest_str,
                        &custom_name_clone,
                        None,
                        Some(&session_id_for_task),
                    )
                    .await;

                    if import_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        // 若压缩包内没有根图标，下载整合包封面作为版本图标
                        if !modpack_icon_url_clone.is_empty()
                            && let Some(vid) = import_result.get("versionId").and_then(|v| v.as_str()) {
                                let icon_dest = crate::storage::resolve_data_dir()
                                    .join("versions")
                                    .join(vid)
                                    .join("icon.png");
                                if !icon_dest.exists() {
                                    eprintln!("[resources] 下载整合包图标: {} → {}", modpack_icon_url_clone, icon_dest.display());
                                    let _ = crate::download::download_with_mirror(
                                        &modpack_icon_url_clone,
                                        &icon_dest,
                                        None,
                                        None,
                                        &download_source,
                                        30,
                                        None,
                                    )
                                    .await;
                                }
                            }
                        let pack_name = import_result
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                            s.status = resources_session::ResourceStage::Completed;
                            s.progress = 100;
                            s.message = if pack_name.is_empty() {
                                format!("整合包 {} 安装完成！", s.file_name)
                            } else {
                                format!("整合包 \"{}\" 安装完成！", pack_name)
                            };
                            s.phase = "completed".to_string();
                            if !s.files.is_empty() {
                                s.files[0].status = "completed".to_string();
                                s.files[0].progress = 100;
                            }
                        });
                    } else {
                        let err = import_result
                            .get("error")
                            .and_then(|e| e.as_str())
                            .unwrap_or("未知错误")
                            .to_string();
                        resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                            s.status = resources_session::ResourceStage::Failed;
                            s.progress = 100;
                            s.message = format!("整合包导入失败: {}", err);
                            s.phase = "failed".to_string();
                            if !s.files.is_empty() {
                                s.files[0].status = "failed".to_string();
                            }
                        });
                    }
                } else {
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Completed;
                        s.progress = 100;
                        s.message = format!("{} 下载完成！", s.file_name);
                        s.phase = "completed".to_string();
                        if !s.files.is_empty() {
                            s.files[0].status = "completed".to_string();
                            s.files[0].progress = 100;
                        }
                    });
                }
                // 下载完成后保留会话 60 秒，让前端有机会读取最终状态
                let sid = session_id_for_task.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    resources_session::remove_session(&sid);
                });
            }
            Err(e) => {
                if resources_session::is_cancelled(&cancel_flag) {
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Cancelled;
                        s.progress = 100;
                        s.message = "下载已取消".to_string();
                        s.phase = "cancelled".to_string();
                    });
                } else {
                    eprintln!("[resources] 下载失败: {}", e);
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Failed;
                        s.progress = 100;
                        s.message = format!("下载失败: {}", e);
                        if !s.files.is_empty() {
                            s.files[0].status = "failed".to_string();
                        }
                    });
                }
                // 失败也保留 60 秒便于前端读取错误
                let sid = session_id_for_task.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    resources_session::remove_session(&sid);
                });
            }
        }
    });

    ApiResult::ok(json!({
        "success": true,
        "sessionId": session_id,
        "fileName": final_name,
        "destPath": dest_path.to_string_lossy()
    }))
}

/// GET /api/resources/download-status — 查询下载进度
///
/// 查询参数：
///   - sessionId: 下载会话 ID
pub(crate) fn handle_download_status(params: &Option<Value>) -> ApiResult {
    let session_id = params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if session_id.is_empty() {
        return ApiResult::err(400, "Missing sessionId");
    }

    match resources_session::get_session_status(session_id) {
        Some(status) => ApiResult::ok(status),
        None => ApiResult::err(404, "会话不存在或已过期"),
    }
}

/// POST /api/resources/download-cancel — 取消下载
///
/// 请求体：
///   - sessionId: 下载会话 ID
pub(crate) fn handle_download_cancel(body: &Option<Value>) -> ApiResult {
    let session_id = body
        .as_ref()
        .and_then(|b| b.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if session_id.is_empty() {
        return ApiResult::err(400, "Missing sessionId");
    }

    if resources_session::cancel_session(session_id) {
        ApiResult::ok(json!({ "success": true }))
    } else {
        ApiResult::err(404, "会话不存在或已过期")
    }
}

/// 解析版本子目录（如 modpacks/resourcepacks）
pub(crate) fn resolve_sub_dir(settings: &Value, version_id: &str, subfolder: &str) -> PathBuf {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let clean_id = version_id.split(" [外部").next().unwrap_or(version_id).trim();
    // 空版本 ID 不能去匹配外部文件夹：folder.join("") 会命中文件夹本身
    let external_dir = if clean_id.is_empty() {
        None
    } else {
        crate::versions::resolve_external_version_dir(clean_id).map(PathBuf::from)
    };

    // 与 PCL 的 PathIndie 一致：隔离时用 versions/<id>，否则用共享 gameDir。
    // 复用 resolve_game_dir，确保版本级 isolation 设置与启动路径保持一致。
    crate::launch::args_builder::resolve_game_dir(
        clean_id,
        None,
        external_dir.as_deref(),
        settings,
        &versions_dir,
        &data_dir,
    )
    .join(subfolder)
}

/// 解析版本 mods 目录
pub(crate) fn resolve_version_mods_dir(settings: &Value, version_id: &str) -> Option<PathBuf> {
    if version_id.is_empty() {
        return None;
    }
    Some(resolve_sub_dir(settings, version_id, "mods"))
}
