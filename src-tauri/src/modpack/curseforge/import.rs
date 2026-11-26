// import.rs — 由 curseforge.rs 拆分
#![allow(unused_imports)]

// CurseForge 整合包导入主流程
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::Semaphore;
use crate::modpack::curseforge_shared as cf_shared;
use super::{emit_progress, normalize_version_id, versions_dir};
use crate::modloaders::shared as ml_shared;
use crate::storage;
use super::*;

/// 主导入流程：overrides 解压 + mods 下载 + 验证 + 资源下载
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_import_main(
    app: &AppHandle,
    archive: &mut zip::ZipArchive<std::fs::File>,
    manifest: &CfManifest,
    version_id: &str,
    version_dir: &Path,
    pack_name: &str,
    mc_version: &str,
    mod_loader_id: &str,
    _loader_kind: &LoaderKind,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    loader_version_id: Option<&str>,
    cf_api_key: &str,
    settings: &Value,
    file_path: &str,
    overrides_prefix: &str,
) -> Result<Value, String> {
    // 记录导入前原版版本目录是否已存在：若为本次流程新建，导入收尾时清理，避免遗留独立原版目录
    let base_vanilla_pre_existed = {
        if mc_version.is_empty() {
            false
        } else {
            versions_dir()
                .join(mc_version)
                .join(format!("{}.json", mc_version))
                .exists()
        }
    };

    // 2.7 overrides 解压
    emit_progress(app, 40, "解压覆盖文件...", "extract");
    let _override_files = extract_overrides_with_progress(archive, version_dir, app, overrides_prefix)?;

    // 2.8 根目录图标提取
    extract_root_icon(archive, version_dir);

    // 2.9 资源包重定位
    let relocated = cf_shared::relocate_misplaced_resource_packs(version_dir);
    if !relocated.relocated.is_empty() {
        eprintln!(
            "[Modpack] 检测到 {} 个资源包 zip 误放在 mods 目录，已自动移动到 resourcepacks: {}",
            relocated.relocated.len(),
            relocated.relocated.join(", ")
        );
    }

    // 2.10 版本隔离强制开启
    let vs_path = version_dir.join("version-settings.json");
    let mut vs: Value = if vs_path.exists() {
        std::fs::read_to_string(&vs_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}))
    } else {
        json!({})
    };
    if let Some(obj) = vs.as_object_mut() {
        obj.insert("isolation".to_string(), json!("on"));
    }
    let _ = std::fs::write(
        &vs_path,
        serde_json::to_string_pretty(&vs).unwrap_or_default(),
    );

    let cf_files = &manifest.files;
    let mods_dir = version_dir.join("mods");
    let _ = std::fs::create_dir_all(&mods_dir);

    // 2.6 清理上次导入失败留下的 .downloading 残留文件
    cf_shared::clean_downloading_residue(version_dir);

    let total_mods = cf_files.len();
    let _cf_mod_files: Vec<ModFileState> = cf_files
        .iter()
        .map(|f| ModFileState {
            name: format!("Mod #{}", f.project_id),
            status: ModStatus::Pending,
            progress: 0,
            project_id: f.project_id,
            file_id: f.file_id,
            dest_path: None,
            mod_id: None,
            file_info: None,
            error: None,
        })
        .collect();

    emit_progress(
        app,
        50,
        &format!("下载 Mod 文件 (共 {} 个)...", total_mods),
        "mods",
    );

    // 2.11 CurseForge API 批量获取文件信息
    let mut file_info_map: HashMap<i64, CfFileResponse> = HashMap::new();
    if !cf_api_key.is_empty() && total_mods > 0 {
        emit_progress(
            app,
            50,
            &format!("正在获取 {} 个 Mod 的下载信息...", total_mods),
            "mods",
        );
        // 直接使用镜像 URL，避免熔断后回退到被墙的官方源
        let _cf_api_base = CF_API_MIRROR;
        for batch_start in (0..total_mods).step_by(CF_BATCH_SIZE) {
            let batch_end = (batch_start + CF_BATCH_SIZE).min(total_mods);
            let file_ids: Vec<i64> = cf_files[batch_start..batch_end]
                .iter()
                .map(|f| f.file_id)
                .collect();

            let mut batch_ok = false;
            // 重试3次，应对镜像偶发 ECONNRESET
            for try_idx in 0..3u32 {
                let url = format!("{}/mods/files", _cf_api_base);
                let body = json!({ "fileIds": file_ids });
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
                let resp = client
                    .post(&url)
                    .header("x-api-key", cf_api_key)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await;

                match resp {
                    Ok(r) if r.status().is_success() => {
                        match r.json::<CfBatchResponse>().await {
                            Ok(parsed) => {
                                for fi in parsed.data {
                                    file_info_map.insert(fi.id, fi);
                                }
                                batch_ok = true;
                                break;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[CurseForge] 批量响应解析失败(batch {}-{}, 尝试 {}/3): {}",
                                    batch_start, batch_end, try_idx + 1, e
                                );
                            }
                        }
                    }
                    Ok(r) => {
                        eprintln!(
                            "[CurseForge] 批量请求失败 HTTP {}(batch {}-{}, 尝试 {}/3)",
                            r.status(),
                            batch_start,
                            batch_end,
                            try_idx + 1
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[CurseForge] 批量请求异常(batch {}-{}, 尝试 {}/3): {}",
                            batch_start, batch_end, try_idx + 1, e
                        );
                    }
                }
                // ECONNRESET 是网络瞬态错误，等待后重试
                if try_idx < 2 {
                    tokio::time::sleep(Duration::from_secs(1 + try_idx as u64)).await;
                }
            }
            if !batch_ok {
                eprintln!(
                    "[CurseForge] 批量获取文件信息最终失败(batch {}-{})，将逐个获取",
                    batch_start, batch_end
                );
            }
        }
    }

    // 2.13 下载引擎：64 线程并发 + 镜像回退 + SHA1 校验
    let downloaded_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let abort_flag = Arc::new(AtomicBool::new(false));

    let parallel = cf_shared::resolve_concurrency(settings);
    let semaphore = Arc::new(Semaphore::new(parallel));

    let file_info_map_arc = Arc::new(file_info_map);
    let mods_dir_arc = Arc::new(mods_dir.clone());
    let cf_api_key_arc = Arc::new(cf_api_key.to_string());

    let mut tasks = tokio::task::JoinSet::new();

    for (idx, file) in cf_files.iter().enumerate() {
        // 检查熔断
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }

        let project_id = file.project_id;
        let file_id = file.file_id;

        // 从 map 中取文件信息
        let file_info = file_info_map_arc.get(&file_id).cloned();

        // 构造下载 URL
        let (download_url, file_name, file_size, expected_sha1) = match &file_info {
            Some(fi) => {
                let mut url = fi.download_url.clone();
                // 2.11 downloadUrl 为空时按 fileID 构造 CDN URL
                if url.is_none() {
                    let id_str = file_id.to_string();
                    if id_str.len() >= 5 {
                        let constructed = construct_cdn_url(file_id, &fi.file_name);
                        eprintln!(
                            "[CurseForge] downloadUrl 为空，已构造 CDN URL: {}:{} -> {}",
                            project_id,
                            file_id,
                            constructed
                        );
                        url = Some(constructed);
                    }
                }
                let sha1 = fi
                    .hashes
                    .as_ref()
                    .and_then(|h| h.iter().find(|x| x.algo == 1))
                    .map(|h| h.value.clone())
                    .unwrap_or_default();
                (url, fi.file_name.clone(), fi.file_length.unwrap_or(0), sha1)
            }
            None => (None, format!("Mod-{}.jar", project_id), 0i64, String::new()),
        };

        let permit = semaphore.clone().acquire_owned().await.map_err(|e| format!("获取信号量失败: {}", e))?;

        let task = download_one_mod(
            project_id,
            file_id,
            download_url,
            file_name,
            file_size,
            expected_sha1,
            mods_dir_arc.clone(),
            abort_flag.clone(),
            downloaded_count.clone(),
            failed_count.clone(),
            total_mods,
            cf_api_key_arc.clone(),
            file_info_map_arc.clone(),
        );

        tasks.spawn(async move {
            let _permit = permit;
            let result = task.await;
            drop(_permit);
            (idx, result)
        });
    }

    // 等待所有下载任务完成（每完成一个就汇报一次进度，让总进度条在 50→88 区间实时移动）
    let mut mod_results: Vec<Option<DownloadResult>> = vec![None; cf_files.len()];
    let mut mods_collected = 0usize;
    let mods_total = total_mods.max(1);
    while let Some(r) = tasks.join_next().await {
        match r {
            Ok((idx, result)) => {
                if idx < mod_results.len() {
                    mod_results[idx] = Some(result);
                }
                mods_collected += 1;
                let pct = 50 + (mods_collected * 38 / mods_total);
                emit_progress(
                    app,
                    pct.min(87) as u32,
                    &format!("下载 Mod ({}/{})...", mods_collected, total_mods),
                    "mods",
                );
            }
            Err(e) => {
                eprintln!("[CurseForge] 下载任务异常: {}", e);
            }
        }
    }

    if abort_flag.load(Ordering::SeqCst) {
        return Err(format!(
            "模组下载失败过多（失败 {} 个 / 成功 {} 个），导入已停止，请检查网络后重试",
            failed_count.load(Ordering::SeqCst),
            downloaded_count.load(Ordering::SeqCst)
        ));
    }

    // 2.16 失败模组列表持久化
    let mut failed_mods_list: Vec<Value> = Vec::new();
    let mut success_mods_list: Vec<Value> = Vec::new();
    for (idx, result_opt) in mod_results.iter().enumerate() {
        if let Some(result) = result_opt {
            if result.success {
                // 2.17 模组清单保存
                if let Some(ref info) = result.file_info {
                    success_mods_list.push(json!({
                        "projectID": cf_files[idx].project_id,
                        "fileID": cf_files[idx].file_id,
                        "fileName": info.file_name,
                        "downloadUrl": info.download_url,
                        "fileLength": info.file_length,
                        "sha1": info.sha1,
                        "modId": result.mod_id,
                    }));
                }
            } else {
                failed_mods_list.push(json!({
                    "projectID": cf_files[idx].project_id,
                    "fileID": cf_files[idx].file_id,
                    "name": result.file_name,
                    "error": result.error,
                }));
            }
        }
    }

    let downloaded_n = downloaded_count.load(Ordering::SeqCst);
    let failed_n = failed_count.load(Ordering::SeqCst);
    eprintln!(
        "[CurseForge] Mod 下载汇总: {} 成功, {} 失败",
        downloaded_n, failed_n
    );

    // 2.16 写入 failed-mods.json
    if !failed_mods_list.is_empty() {
        let failed_path = version_dir.join("failed-mods.json");
        let failed_json = json!({
            "packName": pack_name,
            "mcVersion": mc_version,
            "totalMods": total_mods,
            "failedCount": failed_n,
            "downloadedCount": downloaded_n,
            "failedAt": cf_shared::now_iso(),
            "failedMods": failed_mods_list,
            "note": "这些模组在导入时下载失败，可重新导入整合包补下载，或手动从 CurseForge 下载后放入 mods 文件夹"
        });
        let _ = std::fs::write(
            &failed_path,
            serde_json::to_string_pretty(&failed_json).unwrap_or_default(),
        );
        eprintln!(
            "[CurseForge] 已保存失败模组列表: {} ({}/{})",
            failed_path.display(),
            failed_n,
            total_mods
        );
    }

    // 2.17 模组清单保存 _saveModManifest
    cf_shared::save_mod_manifest(version_dir, &success_mods_list);

    // 2.18 损坏 JAR 修复 _repairCorruptedModJars
    emit_progress(app, 88, "正在修复损坏的模组文件...", "repair");
    let repair_result = cf_shared::repair_corrupted_mod_jars(version_dir);
    if repair_result.failed > 0 {
        eprintln!(
            "[CurseForge] {} 个模组文件损坏且无法修复，游戏启动时可能报错",
            repair_result.failed
        );
    }

    // 2.19 加载器兼容性检查 ensureLoaderCompat
    if let Some(_lv_id) = loader_version_id
        && !mc_version.is_empty() {
            let loader_type = if !fabric_ver.is_empty() {
                "fabric"
            } else if !forge_ver.is_empty() || !neoforge_ver.is_empty() {
                "forge"
            } else {
                ""
            };
            let current_ver = if !fabric_ver.is_empty() {
                fabric_ver
            } else if !forge_ver.is_empty() {
                forge_ver
            } else {
                neoforge_ver
            };
            if !loader_type.is_empty() && !current_ver.is_empty() {
                let _ = cf_shared::ensure_loader_compat(
                    version_id,
                    version_dir,
                    mc_version,
                    current_ver,
                    loader_type,
                )
                .await;
            }
        }

    // 2.20 库文件验证 verifyImportLibs
    emit_progress(app, 90, "正在验证整合包完整性...", "verify");
    let (verify_ok, _checked, missing) = cf_shared::verify_import_libs(version_id).await;
    if !verify_ok {
        eprintln!("[CurseForge] 库文件补全失败: {} 个文件缺失", missing);
        cf_shared::cleanup_version_chain(version_id);
        return Err(format!(
            "整合包库文件补全失败: {} 个文件缺失，请检查网络后重试",
            missing
        ));
    }

    // 2.21 资源索引下载
    let cf_merged_json = cf_shared::resolve_version_json(version_id);
    if let Some(ref merged) = cf_merged_json
        && let Some(asset_index) = merged.get("assetIndex") {
            emit_progress(app, 93, "正在下载游戏资源...", "assets");
            if let Err(e) = download_assets(app, asset_index, settings).await {
                eprintln!("[CurseForge] 资源下载异常(非致命): {}", e);
            }
        }

    // 2.22 客户端 JAR 下载
    if let Some(ref merged) = cf_merged_json
        && let Some(inherits_from) = merged.get("inheritsFrom").and_then(|v| v.as_str()) {
            let main_jar_id = merged
                .get("jar")
                .and_then(|v| v.as_str())
                .unwrap_or(inherits_from);
            let main_jar_path = versions_dir().join(main_jar_id).join(format!("{}.jar", main_jar_id));
            if !main_jar_path.exists() {
                let jar_url = merged
                    .get("downloads")
                    .and_then(|d| d.get("client"))
                    .and_then(|c| c.get("url"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        // 从基础版本 JSON 读取 client url
                        let base_json_path = versions_dir().join(main_jar_id).join(format!("{}.json", main_jar_id));
                        if let Ok(content) = std::fs::read_to_string(&base_json_path)
                            && let Ok(json) = serde_json::from_str::<Value>(&content) {
                                return json
                                    .get("downloads")
                                    .and_then(|d| d.get("client"))
                                    .and_then(|c| c.get("url"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                            }
                        None
                    });
                if let Some(url) = jar_url {
                    emit_progress(app, 97, "正在下载客户端JAR...", "assets");
                    let mut jar_ok = false;
                    for attempt in 0..3u32 {
                        match crate::download::single::download_with_mirror(
                            &url,
                            &main_jar_path,
                            None,
                            None,
                            "mojang",
                            180,
                            None,
                        )
                        .await
                        {
                            Ok(_) => {
                                jar_ok = true;
                                break;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[CurseForge] 客户端JAR下载失败({}/3): {}",
                                    attempt + 1,
                                    e
                                );
                                let _ = std::fs::remove_file(&main_jar_path);
                                if attempt < 2 {
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                }
                            }
                        }
                    }
                    if !jar_ok {
                        eprintln!("[CurseForge] 客户端JAR下载最终失败(非致命)，启动时会自动补全");
                    }
                }
            }
        }

    // 2.23 Forge 核心文件验证
    if let Some(ref merged) = cf_merged_json
        && (!forge_ver.is_empty() || !neoforge_ver.is_empty())
            && let Err(e) = check_forge_core_files(merged) {
                cf_shared::cleanup_version_chain(version_id);
                return Err(e);
            }

    // 2.24 pack-info.json 写入
    let pending_mods: Vec<Value> = if cf_api_key.is_empty() {
        cf_files
            .iter()
            .map(|f| {
                json!({
                    "projectID": f.project_id,
                    "fileID": f.file_id
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let pack_info = json!({
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "packFormat": "curseforge",
        "modLoader": mod_loader_id,
        "forgeVersion": forge_ver,
        "fabricVersion": fabric_ver,
        "neoforgeVersion": neoforge_ver,
        "importedAt": cf_shared::now_iso(),
        "sourceFile": file_path,
        "targetVersion": "",
        "pendingMods": pending_mods,
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info).unwrap_or_default(),
    );

    // 完成
    emit_progress(
        app,
        100,
        &format!("整合包 \"{}\" 导入完成！", pack_name),
        "done",
    );

    // 原版依赖目录为流程本次新建、且整合包版本已自含（合并后独立）时，清理该临时原版目录，
    // 避免遗留独立的原版版本目录（与合并式安装行为一致）
    if !base_vanilla_pre_existed
        && !mc_version.is_empty()
        && mc_version != version_id
    {
        let vanilla_dir = versions_dir().join(mc_version);
        if vanilla_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&vanilla_dir) {
                import_log(&format!("清理临时原版目录失败: {}", e));
            } else {
                import_log(&format!(
                    "已清理本次新建的临时原版目录: {}",
                    vanilla_dir.display()
                ));
            }
        }
    }

    let cf_warning = if cf_api_key.is_empty() {
        Some("CurseForge Mod 文件需要 API Key，overrides 已解压。请在设置中配置 CurseForge API Key 后重新导入。".to_string())
    } else {
        None
    };
    let fail_warning = if failed_n > 0 {
        let failed_names: Vec<String> = failed_mods_list
            .iter()
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect();
        Some(format!(
            "{}/{} 个Mod下载失败: {}。请检查网络后重试。",
            failed_n,
            total_mods,
            failed_names.join(", ")
        ))
    } else {
        None
    };

    Ok(json!({
        "success": true,
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "targetVersion": "",
        "warning": cf_warning.or(fail_warning),
        "failedMods": if failed_n > 0 { Some(failed_mods_list) } else { None },
        "loaderVersionId": loader_version_id,
    }))
}
