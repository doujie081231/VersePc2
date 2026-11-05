// versions/mod.rs — Minecraft 版本管理
// 路由清单：
//   GET  /api/versions                       版本列表（远程 + 本地）
//   GET  /api/version/list-folders           外部文件夹列表
//   POST /api/version/add-folder             添加外部文件夹
//   POST /api/version/remove-folder          移除外部文件夹
//   GET  /api/version-local-details          本地版本详情
//   POST /api/version/delete                  删除版本
//   POST /api/version/rename                  重命名版本（写 customName）
//   GET  /api/version/open-folder             打开版本文件夹
//   GET  /api/version/export-info             导出信息
//   POST /api/version/description             设置描述
//   POST /api/version/favorite                收藏/取消
//   GET  /api/version/select-folder           选择文件夹（弹出对话框）
//   GET  /api/version-details                 远程版本详情
//   POST /api/version/repair                  同步修复缺失库文件与客户端 jar
//   GET  /api/version/diagnose                诊断版本完整性（返回缺失文件列表）
//   POST /api/version/repair-start            启动异步修复会话（返回 sessionId）
//   GET  /api/version/repair-progress         查询修复进度
//   GET  /api/version/repair-cancel            取消修复会话
//   GET  /api/version/export-script           导出启动脚本（.bat/.sh）
//   POST /api/version/export-modpack          导出整合包（ZIP）
//   GET  /api/version-icon                    读取版本图标（base64 data URL）

mod diagnose;
mod export;
mod external;
pub mod icons;
mod manifest;
mod repair;
mod scan;

pub(crate) use manifest::fetch_remote_manifest;
#[cfg(test)]
pub(crate) use manifest::set_test_manifest_url;
pub(crate) use scan::{scan_external_versions, scan_local_versions};
pub(crate) use external::resolve_external_version_dir;

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::storage;
use crate::utils;

use diagnose::{handle_diagnose, open_in_explorer, send_to_recycle_bin};
use export::{handle_export_modpack, handle_export_script};
use external::{auto_detect_external_folders, is_external_version};
use icons::handle_version_icon;
use repair::{handle_repair, handle_repair_cancel, handle_repair_progress, handle_repair_start};
use scan::{
    filter_loader_visibility, inherit_loader_from_parent, mark_april_fools, mark_installed,
    scan_versions_in_dir,
};
use manifest::fetch_json_blocking;


/// api_proxy 路由处理：版本管理相关
pub async fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<crate::api::ApiResult> {
    use crate::api::ApiResult;

    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 版本列表 =====
        "GET /api/versions" => {
            eprintln!("[versions] GET /api/versions called");
            let refresh = params.as_ref()
                .and_then(|p| p.get("refresh"))
                .and_then(|v| v.as_str())
                .map(|s| s == "true")
                .unwrap_or(false);
            let installed_only = params.as_ref()
                .and_then(|p| p.get("installed"))
                .and_then(|v| v.as_str())
                .map(|s| s == "true")
                .unwrap_or(false);
            eprintln!("[versions] refresh={}, installed_only={}", refresh, installed_only);

            // 本地版本
            let local_versions = scan_local_versions();
            let external_versions = scan_external_versions();
            let mut all_local: Vec<Value> = Vec::new();
            all_local.extend(local_versions.clone());
            all_local.extend(external_versions.clone());
            eprintln!("[versions] local_versions={}, external_versions={}", local_versions.len(), external_versions.len());

            // 整合包自身无加载器标志时，从父版本继承加载器类型（RLCraft 等），
            // 避免设置页把整合包误判为"原版"
            inherit_loader_from_parent(&mut all_local);

            // 过滤被整合包继承且自身 mods 为空的加载器版本（如 Forge 底座版本），
            all_local = filter_loader_visibility(all_local);

            // 收集本地 ID（用于标记远程版本）
            let local_ids: Vec<String> = all_local.iter()
                .map(|v| utils::get_str(v, "id"))
                .collect();

            if installed_only {
                eprintln!("[versions] installed_only, returning local only");
                return Some(ApiResult::ok(json!({
                    "latest": { "release": "", "snapshot": "" },
                    "versions": all_local,
                    "installed": all_local
                })));
            }

            // 拉远程清单（spawn_blocking 避免阻塞 Tauri 异步运行时）
            // 8 秒总超时，超时则仅返回本地版本
            eprintln!("[versions] fetching remote manifest...");
            let manifest = tokio::time::timeout(
                std::time::Duration::from_secs(8),
                tokio::task::spawn_blocking(move || fetch_remote_manifest(refresh)),
            ).await;
            eprintln!("[versions] manifest timeout result: {:?}", manifest.is_ok());
            let manifest = manifest.ok().and_then(|r| r.ok()).flatten();
            eprintln!("[versions] manifest loaded: {}", manifest.is_some());

            let (latest, mut remote_versions) = match manifest {
                Some(m) => {
                    let latest_release = m.get("latest").and_then(|l| l.get("release")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let latest_snapshot = m.get("latest").and_then(|l| l.get("snapshot")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let versions = m.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    eprintln!("[versions] remote versions count: {}", versions.len());
                    (json!({ "release": latest_release, "snapshot": latest_snapshot }), versions)
                }
                None => (json!({ "release": "", "snapshot": "" }), Vec::new()),
            };

            // 标记已安装
            mark_installed(&mut remote_versions, &local_ids);

            // 愚人节版本在官方清单中 type 多为 snapshot/release，
            // 这里统一标记为 special，让前端"愚人节版"tab 能正确归类展示
            mark_april_fools(&mut remote_versions);

            Some(ApiResult::ok(json!({
                "latest": latest,
                "versions": remote_versions,
                "installed": all_local
            })))
        }

        // ===== 外部文件夹列表 =====
        "GET /api/version/list-folders" => {
            let folders = storage::load_external_folders();
            let mut result: Vec<Value> = Vec::new();
            for folder in folders {
                let path_str = utils::get_str(&folder, "path");
                let name = utils::get_str(&folder, "name");
                let added_at = utils::get_str(&folder, "addedAt");
                let path = PathBuf::from(&path_str);
                let exists = path.exists();
                let version_count = if exists {
                    scan_versions_in_dir(&path.join("versions"), true, Some(&path)).len() as u64
                } else {
                    0
                };
                result.push(json!({
                    "path": path_str,
                    "name": name,
                    "addedAt": added_at,
                    "exists": exists,
                    "versionCount": version_count
                }));
            }
            Some(ApiResult::ok(json!({ "success": true, "folders": result })))
        }

        // ===== 添加外部文件夹 =====
        "POST /api/version/add-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let mut path_str = utils::get_str(&data, "path");
            let name = utils::get_str(&data, "name");
            if path_str.is_empty() {
                return Some(ApiResult::err(400, "Missing path"));
            }
            let mut folder_path = PathBuf::from(&path_str);
            if !folder_path.exists() {
                return Some(ApiResult::ok(json!({ "success": false, "error": format!("文件夹不存在: {}", path_str) })));
            }
            if !folder_path.is_dir() {
                return Some(ApiResult::ok(json!({ "success": false, "error": format!("路径不是文件夹: {}", path_str) })));
            }

            // 兼容用户直接选择 PCL/官方启动器的 versions 目录，或单个版本目录：
            // 统一换算回真正的 Minecraft 根目录后再校验 versions/。
            if !folder_path.join("versions").is_dir() {
                let selected_name = folder_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if selected_name.eq_ignore_ascii_case("versions")
                    && let Some(parent) = folder_path.parent() {
                        folder_path = parent.to_path_buf();
                } else if folder_path.join(format!("{}.json", selected_name)).is_file()
                    && let Some(versions) = folder_path.parent()
                    && versions.file_name().and_then(|n| n.to_str()) == Some("versions")
                    && let Some(root) = versions.parent() {
                        folder_path = root.to_path_buf();
                    }
                path_str = folder_path.to_string_lossy().to_string();
            }

            // 校验：必须包含 versions 子目录
            let versions_dir = folder_path.join("versions");
            if !versions_dir.exists() {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹下未找到 versions 子目录，请选择有效的 Minecraft 文件夹" })));
            }
            let mut folders = storage::load_external_folders();
            // 去重
            if folders.iter().any(|f| utils::get_str(f, "path") == path_str) {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹已添加" })));
            }
            // 扫描并校验是否存在有效版本
            let scanned_versions = scan_versions_in_dir(&versions_dir, true, Some(&folder_path));
            if scanned_versions.is_empty() {
                return Some(ApiResult::ok(json!({ "success": false, "error": "该文件夹下未找到有效的 Minecraft 版本" })));
            }
            let name = if name.is_empty() {
                folder_path.file_name().and_then(|n| n.to_str()).unwrap_or("外部").to_string()
            } else {
                name
            };
            folders.push(json!({
                "path": path_str,
                "name": name,
                "addedAt": utils::now_iso()
            }));
            storage::save_external_folders(&folders);
            Some(ApiResult::ok(json!({ "success": true, "versions": scanned_versions })))
        }

        // ===== 移除外部文件夹 =====
        "POST /api/version/remove-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let path_str = utils::get_str(&data, "path");
            let mut folders = storage::load_external_folders();
            folders.retain(|f| utils::get_str(f, "path") != path_str);
            storage::save_external_folders(&folders);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        // ===== 重命名外部文件夹 =====
        "POST /api/version/rename-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let path_str = utils::get_str(&data, "path");
            let name = utils::get_str(&data, "name");
            if path_str.is_empty() || name.trim().is_empty() {
                return Some(ApiResult::err(400, "Missing path or name"));
            }
            let trimmed = name.trim().to_string();
            let mut folders = storage::load_external_folders();
            let mut found = false;
            for folder in folders.iter_mut() {
                if utils::get_str(folder, "path") == path_str {
                    if let Some(obj) = folder.as_object_mut() {
                        obj.insert("name".to_string(), json!(trimmed));
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                return Some(ApiResult::ok(json!({ "success": false, "error": "未找到该外部文件夹" })));
            }
            storage::save_external_folders(&folders);
            Some(ApiResult::ok(json!({ "success": true })))
        }

        // ===== 本地版本详情 =====
        "GET /api/version-local-details" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            // 判断是否外部版本
            let is_external = is_external_version(version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };
            let settings = storage::load_version_settings(&clean_id, is_external);
            Some(ApiResult::ok(json!({
                "hasMods": false,
                "hasSaves": false,
                "hasResourcepacks": false,
                "error": false,
                "errorReason": "",
                "customName": utils::get_str(&settings, "customName"),
                "description": utils::get_str(&settings, "description"),
                "favorite": settings.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false)
            })))
        }

        // ===== 版本独立设置（读取） =====
        "GET /api/version/settings" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };
            let settings = storage::load_version_settings(&clean_id, is_external);
            Some(ApiResult::ok(settings))
        }

        // ===== 版本独立设置（保存，合并已有字段） =====
        "POST /api/version/settings/save" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id.clone()
            };
            // 读取现有设置并合并
            let mut settings = storage::load_version_settings(&clean_id, is_external);
            if let (Some(dst), Some(src)) = (settings.as_object_mut(), data.as_object()) {
                for (k, v) in src {
                    // 跳过 versionId（保持原值）
                    if k == "versionId" {
                        continue;
                    }
                    dst.insert(k.clone(), v.clone());
                }
            }
            let ok = storage::save_version_settings(&clean_id, is_external, &settings);
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 版本图标设置（保存自定义图标类型） =====
        "POST /api/version/icon" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let icon = utils::get_str(&data, "icon");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "icon", json!(icon));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 删除版本 =====
        "POST /api/version/delete" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let permanent = data.get("permanent").and_then(|v| v.as_bool()).unwrap_or(false);
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id.clone()
            };

            let data_dir = storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let version_dir = versions_dir.join(storage::sanitize_version_id(&clean_id));

            if !version_dir.exists() {
                return Some(ApiResult::err(404, "版本不存在"));
            }

            // 删除目录
            let result = if permanent {
                std::fs::remove_dir_all(&version_dir)
            } else {
                // 优先送回收站，失败回退强制删除
                send_to_recycle_bin(&version_dir).or_else(|_| std::fs::remove_dir_all(&version_dir))
            };

            match result {
                Ok(_) => Some(ApiResult::ok(json!({
                    "success": true,
                    "deleted": [clean_id],
                    "permanent": permanent
                }))),
                Err(e) => Some(ApiResult::err(500, &format!("删除失败: {}", e))),
            }
        }

        // ===== 重命名版本（写 customName 到 version-settings.json） =====
        "POST /api/version/rename" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let new_name = utils::get_str(&data, "newName");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "customName", json!(new_name));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 打开版本文件夹 =====
        "GET /api/version/open-folder" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let folder_type = params.as_ref()
                .and_then(|p| p.get("folderType"))
                .and_then(|v| v.as_str())
                .unwrap_or("version");
            eprintln!("[open-folder] version_id={:?} folder_type={:?}", version_id, folder_type);
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let is_external = is_external_version(version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };
            eprintln!("[open-folder] is_external={} clean_id={:?}", is_external, clean_id);

            let data_dir = storage::resolve_data_dir();
            eprintln!("[open-folder] data_dir = {}", data_dir.display());
            let versions_dir = data_dir.join("versions");
            eprintln!("[open-folder] versions_dir = {}", versions_dir.display());
            let version_dir = versions_dir.join(storage::sanitize_version_id(&clean_id));
            eprintln!("[open-folder] version_dir = {} (exists={})", version_dir.display(), version_dir.exists());

            let game_root = if is_external {
                version_dir.clone()
            } else {
                let settings = storage::load_settings();
                let gr = crate::launch::args_builder::resolve_game_dir(
                    &clean_id,
                    None,
                    None,
                    &settings,
                    &versions_dir,
                    &data_dir,
                );
                eprintln!("[open-folder] resolve_game_dir version isolation -> game_root = {}", gr.display());
                gr
            };

            let target = match folder_type {
                "version" => version_dir,
                "saves" => game_root.join("saves"),
                "mods" => game_root.join("mods"),
                "resourcepacks" => game_root.join("resourcepacks"),
                "shaderpacks" => game_root.join("shaderpacks"),
                "logs" => game_root.join("logs"),
                "crash-reports" => game_root.join("crash-reports"),
                _ => version_dir,
            };
            eprintln!("[open-folder] target = {} (exists={}, is_dir={})",
                target.display(), target.exists(), target.is_dir());

            if !target.exists() {
                let _ = std::fs::create_dir_all(&target);
                eprintln!("[open-folder] created target dir (after create exists={})", target.exists());
            }

            if open_in_explorer(&target) {
                Some(ApiResult::ok(json!({ "success": true, "path": target.to_string_lossy() })))
            } else {
                Some(ApiResult::err(500, "无法打开文件夹"))
            }
        }

        // ===== 设置描述 =====
        "POST /api/version/description" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let description = utils::get_str(&data, "description");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "description", json!(description));
            Some(ApiResult::ok(json!({ "success": ok })))
        }

        // ===== 收藏/取消收藏 =====
        "POST /api/version/favorite" => {
            let data = body.clone().unwrap_or(Value::Null);
            let version_id = utils::get_str(&data, "versionId");
            let favorite = data.get("favorite").and_then(|v| v.as_bool()).unwrap_or(false);
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let is_external = is_external_version(&version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
            } else {
                version_id
            };
            let ok = storage::update_version_setting(&clean_id, is_external, "favorite", json!(favorite));
            Some(ApiResult::ok(json!({ "success": ok, "favorite": favorite })))
        }

        // ===== 导出信息 =====
        "GET /api/version/export-info" => {
            let version_id = params.as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let is_external = is_external_version(version_id);
            let clean_id = if is_external {
                version_id.split(" [外部").next().unwrap_or(version_id).to_string()
            } else {
                version_id.to_string()
            };

            let data_dir = storage::resolve_data_dir();
            let version_dir = data_dir.join("versions").join(&clean_id);
            let mods_dir = version_dir.join("mods");
            let saves_dir = version_dir.join("saves");
            let rp_dir = version_dir.join("resourcepacks");

            // 统计 mods 数量
            let mod_count = if mods_dir.exists() {
                std::fs::read_dir(&mods_dir)
                    .map(|d| d.filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path().extension().and_then(|x| x.to_str()).map(|s| s == "jar" || s == "zip").unwrap_or(false)
                        })
                        .count())
                    .unwrap_or(0)
            } else { 0 };

            // 统计 saves 数量
            let mut saves: Vec<String> = Vec::new();
            if saves_dir.exists()
                && let Ok(entries) = std::fs::read_dir(&saves_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() && entry.path().join("level.dat").exists()
                            && let Some(name) = entry.file_name().to_str() {
                                saves.push(name.to_string());
                                if saves.len() >= 20 {
                                    break;
                                }
                            }
                    }
                }

            // resourcepacks zip 文件
            let mut resource_packs: Vec<String> = Vec::new();
            if rp_dir.exists()
                && let Ok(entries) = std::fs::read_dir(&rp_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().and_then(|x| x.to_str()) == Some("zip")
                            && let Some(name) = entry.file_name().to_str() {
                                resource_packs.push(name.to_string());
                            }
                    }
                }

            // gameDesc：简化版（完整需要解析加载器版本）
            let game_desc = clean_id.clone();

            Some(ApiResult::ok(json!({
                "gameDesc": game_desc,
                "resourcePacks": resource_packs,
                "modCount": mod_count,
                "savesCount": saves.len(),
                "saves": saves
            })))
        }

        // ===== 选择文件夹（弹出对话框） =====
        "GET /api/version/select-folder" => {
            // 这个路由由前端 dialog_open 命令处理，这里返回提示
            // 前端应该调用 invoke('select_folder', ...) 而不是走 api_proxy
            Some(ApiResult::ok(json!({
                "success": false,
                "cancelled": true,
                "message": "请使用对话框命令选择文件夹"
            })))
        }

        // ===== 一键识别版本文件夹（跨盘深度扫描） =====
        "GET /api/version/auto-detect-folders" => {
            // 阻塞扫描放到独立线程执行并加超时，避免阻塞 Tauri 异步运行时导致请求超时
            let folders = match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                tokio::task::spawn_blocking(auto_detect_external_folders),
            )
            .await
            {
                Ok(Ok(folders)) => folders,
                Ok(Err(e)) => {
                    eprintln!("[versions] auto-detect scan thread error: {}", e);
                    return Some(ApiResult::err(500, "版本文件夹扫描失败"));
                }
                Err(_) => {
                    eprintln!("[versions] auto-detect scan timed out");
                    return Some(ApiResult::err(504, "版本文件夹扫描超时，请稍后再试"));
                }
            };
            Some(ApiResult::ok(json!({
                "success": true,
                "folders": folders
            })))
        }

        // ===== 远程版本详情 =====
        "GET /api/version-details" => {
            let url = params.as_ref()
                .and_then(|p| p.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                return Some(ApiResult::err(400, "Missing url"));
            }
            // 同步阻塞拉取
            let details = fetch_json_blocking(url);

            match details {
                Some(d) => Some(ApiResult::ok(d)),
                None => Some(ApiResult::err(502, "无法获取版本详情")),
            }
        }

        // ===== 安装相关已迁移到 api/download.rs =====
        // install-start / install-progress / install-cancel / check-version-name

        // ===== 清理版本目录（删 natives/logs/crash-reports） =====
        "GET /api/version/cleanup" => {
            let version_id = params
                .as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            let data_dir = crate::storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let version_dir = versions_dir.join(version_id);
            if !version_dir.exists() {
                return Some(ApiResult::err(404, "版本目录不存在"));
            }

            let mut deleted: Vec<String> = Vec::new();
            let cleanup_dirs = ["natives", "logs", "crash-reports", ".fabric", ".quilt"];
            for sub in &cleanup_dirs {
                let p = version_dir.join(sub);
                if p.exists()
                    && std::fs::remove_dir_all(&p).is_ok() {
                        deleted.push(sub.to_string());
                    }
            }
            Some(ApiResult::ok(json!({
                "success": true,
                "deleted": deleted
            })))
        }

        // ===== 设置版本分类（写入 version-settings.json） =====
        "POST /api/version/category" => {
            let version_id = body
                .as_ref()
                .and_then(|b| b.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let category = body
                .as_ref()
                .and_then(|b| b.get("category"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }
            // 读取现有 version-settings.json
            let data_dir = crate::storage::resolve_data_dir();
            let versions_dir = data_dir.join("versions");
            let settings_path = versions_dir.join("version-settings.json");
            let mut settings_json: Value = if settings_path.exists() {
                std::fs::read_to_string(&settings_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(json!({ "versions": {} }))
            } else {
                json!({ "versions": {} })
            };

            // 更新对应版本的 category
            if let Some(versions) = settings_json
                .get_mut("versions")
                .and_then(|v| v.as_object_mut())
            {
                let entry = versions
                    .entry(version_id.to_string())
                    .or_insert_with(|| json!({}));
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("category".to_string(), json!(category));
                }
            }

            // 写回
            let content = serde_json::to_string_pretty(&settings_json).unwrap_or_default();
            match std::fs::write(&settings_path, content) {
                Ok(_) => Some(ApiResult::ok(json!({ "success": true }))),
                Err(e) => Some(ApiResult::err(
                    500,
                    &format!("写入失败: {}", e),
                )),
            }
        }

        // ===== 删除版本链（递归收集子版本一起删除） =====
        "POST /api/version/delete-chain" => {
            let version_id = body
                .as_ref()
                .and_then(|b| b.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if version_id.is_empty() {
                return Some(ApiResult::err(400, "Missing versionId"));
            }

            let data_dir = crate::storage::resolve_data_dir();
            let versions_root = data_dir.join("versions");

            // 收集所有引用此版本作为 inheritsFrom 的子版本
            let mut to_delete: Vec<String> = vec![version_id.to_string()];
            let mut visited = std::collections::HashSet::new();
            visited.insert(version_id.to_string());

            // 递归查找子版本
            let mut changed = true;
            while changed {
                changed = false;
                if let Ok(entries) = std::fs::read_dir(&versions_root) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if visited.contains(name) {
                                continue;
                            }
                            // 检查这个版本的 JSON 是否 inheritsFrom 当前要删除的版本之一
                            let json_path = versions_root.join(name).join(format!("{}.json", name));
                            if !json_path.exists() {
                                continue;
                            }
                            if let Ok(content) = std::fs::read_to_string(&json_path)
                                && let Ok(json) = serde_json::from_str::<Value>(&content)
                                    && let Some(inherits) = json.get("inheritsFrom").and_then(|v| v.as_str())
                                        && visited.contains(inherits) {
                                            to_delete.push(name.to_string());
                                            visited.insert(name.to_string());
                                            changed = true;
                                        }
                        }
                    }
                }
            }

            // 执行删除
            let mut deleted: Vec<String> = Vec::new();
            let mut failed: Vec<Value> = Vec::new();
            for vid in &to_delete {
                let dir = versions_root.join(vid);
                if !dir.exists() {
                    continue;
                }
                // 优先送回收站，失败则直接删除
                match send_to_recycle_bin(&dir) {
                    Ok(_) => {
                        deleted.push(vid.clone());
                    }
                    Err(_) => {
                        if let Err(e) = std::fs::remove_dir_all(&dir) {
                            failed.push(json!({
                                "versionId": vid,
                                "error": e.to_string()
                            }));
                        } else {
                            deleted.push(vid.clone());
                        }
                    }
                }
            }

            Some(ApiResult::ok(json!({
                "success": failed.is_empty(),
                "deleted": deleted,
                "failed": failed,
                "total": to_delete.len()
            })))
        }

        // ===== 版本修复（同步） =====
        "POST /api/version/repair" => Some(handle_repair(body).await),

        // ===== 版本诊断 =====
        "GET /api/version/diagnose" => Some(handle_diagnose(params)),

        // ===== 异步修复会话 =====
        "POST /api/version/repair-start" => Some(handle_repair_start(body).await),
        "GET /api/version/repair-progress" => Some(handle_repair_progress(params)),
        "GET /api/version/repair-cancel" => Some(handle_repair_cancel(params).await),

        // ===== 导出启动脚本（.bat / .sh） =====
        "POST /api/version/export-script" => Some(handle_export_script(body).await),

        // ===== 导出整合包（ZIP） =====
        "POST /api/version/export-modpack" => Some(handle_export_modpack(body)),

        // ===== 读取版本图标文件（base64） =====
        "GET /api/version-icon" => Some(handle_version_icon(params)),

        _ => None,
    }
}
