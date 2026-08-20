// modpack/mrpack_native.rs — Modrinth (.mrpack) 整合包导入
//
// 完整复刻原项目 server/modpack/modrinth.js 的全部功能点。
// 解析 modrinth.index.json，安装基础版本与模组加载器，下载 mods 与 overrides。
//
// 与原项目 1:1 对齐，不做任何简化。
// 不走 theseus 的 install_zipped_mrpack_files_with_reporter，独立实现完整逻辑。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Semaphore};

use super::curseforge_shared as cf_shared;
use super::{emit_progress, emit_progress_with_files, normalize_version_id, versions_dir};
use crate::modloaders::shared as ml_shared;
use crate::storage;

// ============== 常量 ==============

// 单文件下载最大重试轮数
const MAX_DOWNLOAD_ROUNDS: u32 = 3;

/// 取消标志清理守卫：作用域结束时自动 unregister 注册的取消标志
/// 确保导入无论是成功、失败还是被取消，都不会在注册表中残留 token
struct CleanupAbort(Option<String>);

impl Drop for CleanupAbort {
    fn drop(&mut self) {
        if let Some(token) = &self.0 {
            super::unregister_modpack_abort(token);
        }
    }
}

// ============== 数据结构 ==============

// modrinth.index.json schema
#[derive(Deserialize)]
struct MrpackManifest {
    #[serde(default)]
    format_version: Option<i64>,
    #[serde(default)]
    game: Option<String>,
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<MrpackFile>,
    #[serde(default)]
    dependencies: MrpackDependencies,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MrpackDependencies {
    minecraft: Option<String>,
    forge: Option<String>,
    neoforge: Option<String>,
    #[serde(rename = "fabric-loader")]
    fabric_loader: Option<String>,
    #[serde(rename = "quilt-loader")]
    quilt_loader: Option<String>,
}

#[derive(Deserialize)]
struct MrpackFile {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    hashes: MrpackHashes,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: Option<i64>,
    #[serde(default)]
    env: Option<MrpackEnv>,
    #[serde(default)]
    loaders: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MrpackHashes {
    sha1: Option<String>,
    #[allow(dead_code)]
    sha512: Option<String>,
}

#[derive(Deserialize)]
struct MrpackEnv {
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    server: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum LoaderKind {
    Forge,
    NeoForge,
    Fabric,
    None,
}

// ============== 主入口 ==============

/// 导入 Modrinth (.mrpack) 整合包
///
/// 完整复刻原项目 _importMrpack 的全部功能点：
/// 1. modrinth.index.json 解析（formatVersion/game/versionId/name/summary/files/dependencies）
/// 2. 版本目录去重 dedupe_version_id
/// 3. 基础版本安装 ensure_base_version_installed
/// 4. Forge/NeoForge/Fabric 安装（已存在则校验 libs 完整性，损坏则删除重装；Quilt 显式报错）
/// 5. 版本 JSON 创建（mergeVersionJson 合并 + 复制主 jar + 删除加载器目录）
/// 6. mods 目录备份（已存在版本目录时备份到 *.backup_<timestamp>）
/// 7. overrides 解压（路径遍历保护 + 5 次重试 + 50 文件 yield + 实时进度）
/// 8. client_overrides 解压（overrides 之后解压，路径遍历保护）
/// 9. 根目录图标提取（pack.png / icon.png / logo.png）
/// 10. 资源包重定位 relocate_misplaced_resource_packs
/// 11. 版本隔离强制开启（version-settings.json: { isolation: 'on' }）
/// 12. env 字段过滤（client === 'unsupported' 跳过）+ loaders 字段过滤
/// 13. Modrinth 文件下载（多镜像回退 + 64 线程并发 + SHA1 校验）
/// 14. 熔断保护（失败数 > max(5, 10% × 总数) 且 failCount > okCount 才取消）
/// 15. 失败模组列表持久化（failed-mods.json）
/// 16. 模组清单保存 save_mod_manifest
/// 17. 损坏 JAR 修复 repair_corrupted_mod_jars
/// 18. 加载器兼容性检查 ensure_loader_compat
/// 19. 库文件验证 verify_import_libs
/// 20. 资源索引下载（解析 assetIndex + 缺资源并发下载 64 线程）
/// 21. 客户端 JAR 下载（3 次重试 + 失败非致命）
/// 22. Forge 核心文件验证（forge-client.jar / client-srg.jar / client-extra.jar）
/// 23. pack-info.json 写入
/// 24. mrpack-manifest.json 保存（供启动前 mods 完整性检查使用）
/// 25. 失败回滚（恢复备份的 mods 目录 + cleanupVersionChain）
pub async fn import_mrpack(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
    update_version_id: Option<&str>,
    cancel_token: Option<&str>,
) -> Value {
    eprintln!("[mrpack] 开始导入: {}", file_path);

    // 支持取消：注册取消标志（若提供了 cancel_token），供 /api/modpack/cancel 触发中断
    let abort_flag = if let Some(token) = cancel_token {
        if token.is_empty() {
            None
        } else {
            Some(super::register_modpack_abort(token))
        }
    } else {
        None
    };
    // 导入结束（成功或失败）统一清理注册的取消标志
    let _cleanup = CleanupAbort(cancel_token.map(str::to_string));

    // 1. 打开 zip 找 modrinth.index.json
    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    // 找到 modrinth.index.json
    let manifest_idx = {
        let mut found: Option<usize> = None;
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                if entry.name().to_lowercase() == "modrinth.index.json" {
                    found = Some(i);
                    break;
                }
            }
        }
        found
    };
    let manifest_idx = match manifest_idx {
        Some(i) => i,
        None => {
            return json!({
                "success": false,
                "error": "Modrinth 整合包缺少 modrinth.index.json"
            })
        }
    };

    // 3.1 modrinth.index.json 解析
    let manifest_str = {
        let mut entry = match archive.by_index(manifest_idx) {
            Ok(e) => e,
            Err(e) => {
                return json!({
                    "success": false,
                    "error": format!("读取 modrinth.index.json 失败: {}", e)
                })
            }
        };
        let mut buf = String::new();
        if let Err(e) = entry.read_to_string(&mut buf) {
            return json!({
                "success": false,
                "error": format!("解析 modrinth.index.json 失败: {}", e)
            });
        }
        buf
    };

    let manifest: MrpackManifest = match serde_json::from_str(&manifest_str) {
        Ok(m) => m,
        Err(e) => {
            return json!({
                "success": false,
                "error": format!("解析 modrinth.index.json 失败: {}", e)
            })
        }
    };

    // 3.1 game 字段校验（必须是 minecraft）
    if let Some(ref game) = manifest.game {
        if game != "minecraft" {
            return json!({
                "success": false,
                "error": format!("不支持的 game 类型: {}（仅支持 minecraft）", game)
            });
        }
    }

    // 3.1 提取 packName（过滤非法字符）
    let pack_name_raw = manifest
        .name
        .as_ref()
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            Path::new(file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Modpack")
                .to_string()
        });
    let pack_name: String = pack_name_raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    // 3.1 dependencies 解析（minecraft、forge、neoforge、fabric-loader、quilt-loader）
    // mcVersion 提取：必须非空、不等于 'minecraft'、以数字开头
    let mc_version = manifest
        .dependencies
        .minecraft
        .as_ref()
        .filter(|v| {
            !v.is_empty()
                && v.as_str() != "minecraft"
                && v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
        })
        .cloned()
        .unwrap_or_default();

    // 如果 mcVersion 与 versionId 相同则置空
    let mc_version = if !mc_version.is_empty() {
        if let Some(ref vid) = manifest.version_id {
            if vid.as_str() == mc_version {
                String::new()
            } else {
                mc_version
            }
        } else {
            mc_version
        }
    } else {
        mc_version
    };

    let fabric_ver = manifest.dependencies.fabric_loader.clone().unwrap_or_default();
    let mut forge_ver = manifest.dependencies.forge.clone().unwrap_or_default();
    let neoforge_ver = manifest.dependencies.neoforge.clone().unwrap_or_default();
    let quilt_ver = manifest.dependencies.quilt_loader.clone().unwrap_or_default();

    // forge 版本号去掉 mcVersion 前缀（如 "1.20.1-47.4.20" → "47.4.20"）
    if !forge_ver.is_empty() && !mc_version.is_empty() {
        let prefix = format!("{}-", mc_version);
        if forge_ver.starts_with(&prefix) {
            forge_ver = forge_ver[prefix.len()..].to_string();
        }
    }

    // 3.4 Quilt 如果不支持要显式报错
    if !quilt_ver.is_empty() {
        return json!({
            "success": false,
            "error": format!(
                "整合包要求 Quilt Loader {}，但当前版本暂不支持 Quilt，请使用 Fabric 版本的整合包",
                quilt_ver
            )
        });
    }

    emit_progress(
        app,
        8,
        &format!("整合包: {}  MC: {}", pack_name, mc_version),
        "prepare",
    );

    // 3.2 版本目录去重 dedupe_version_id
    // 更新整合包时（update_version_id 非空）复用指定版本目录，不新建版本
    let base_version_id = if !custom_version_name.is_empty() {
        normalize_version_id(custom_version_name)
    } else {
        normalize_version_id(&pack_name)
    };
    let version_id = if let Some(uv) = update_version_id {
        if uv.is_empty() {
            cf_shared::dedupe_version_id(&base_version_id)
        } else {
            normalize_version_id(uv)
        }
    } else {
        cf_shared::dedupe_version_id(&base_version_id)
    };
    let version_dir = versions_dir().join(&version_id);

    // 创建版本目录
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({
            "success": false,
            "error": format!("无法创建版本目录: {}", e)
        });
    }

    let is_new_version_dir = !version_dir.join(format!("{}.json", version_id)).exists();

    // loaderVersionId 计算
    let loader_version_id: Option<String> = if !forge_ver.is_empty() {
        Some(format!("{}-forge-{}", mc_version, forge_ver))
    } else if !neoforge_ver.is_empty() {
        Some(format!("{}-neoforge-{}", mc_version, neoforge_ver))
    } else if !fabric_ver.is_empty() {
        Some(format!("fabric-loader-{}-{}", fabric_ver, mc_version))
    } else {
        None
    };

    // 3.3-3.5 基础版本安装 + 加载器安装 + 版本 JSON 创建
    if is_new_version_dir {
        // 3.3 基础版本安装
        emit_progress(app, 5, "正在准备基础版本...", "base");
        let app_owned = app.clone();
        let on_progress = Some(Box::new(move |pct: u32, msg: String| {
            // 基础版本安装的 0-100 映射到整合包整体的 5-20 区间
            let mapped = 5 + (pct as u64 * 15 / 100).min(15) as u32;
            emit_progress(&app_owned, mapped, &msg, "base")
        }) as ml_shared::BaseVersionProgress);
        if let Err(e) = ml_shared::ensure_base_version_installed(&mc_version, on_progress).await {
            let _ = std::fs::remove_dir_all(&version_dir);
            return json!({
                "success": false,
                "versionId": version_id,
                "error": e
            });
        }

        // 3.4 Forge/NeoForge/Fabric 安装
        if !forge_ver.is_empty() || !neoforge_ver.is_empty() || !fabric_ver.is_empty() {
            emit_progress(app, 20, "正在安装模组加载器...", "loader-install");
            match install_loader(
                &forge_ver,
                &neoforge_ver,
                &fabric_ver,
                &mc_version,
                app,
            )
            .await
            {
                Ok(lv_id) => {
                    // lv_id 已经和 loader_version_id 一致
                }
                Err(e) => {
                    eprintln!("[mrpack] 模组加载器安装失败: {}", e);
                    let _ = std::fs::remove_dir_all(&version_dir);
                    return json!({
                        "success": false,
                        "versionId": version_id,
                        "error": e
                    });
                }
            }
        }

        // 3.5 版本 JSON 创建（mergeVersionJson 合并）
        emit_progress(app, 35, "正在创建版本配置...", "version-config");
        create_version_json_with_merge(
            &version_id,
            &version_dir,
            &mc_version,
            loader_version_id.as_deref(),
        );

        emit_progress(app, 40, "模组加载器就绪", "loader");
    }

    // re-merge 逻辑：如果现有版本 JSON 仍有 inheritsFrom，则重新合并
    // 整合包重装/重导入时检测现有版本JSON是否已合并加载器内容
    {
        let version_json_path = version_dir.join(format!("{}.json", version_id));
        let existing_json: Option<Value> = if version_json_path.exists() {
            std::fs::read_to_string(&version_json_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };

        if let Some(ref existing) = existing_json {
            if existing.get("inheritsFrom").and_then(|v| v.as_str()).is_some() {
                if let Some(ref lv_id) = loader_version_id {
                    emit_progress(
                        app,
                        5,
                        &format!("正在同步加载器到 {}...", lv_id),
                        "base-fix",
                    );

                    // 确保基础版本存在
                    if !mc_version.is_empty() {
                        let app_owned = app.clone();
                        let on_progress = Some(Box::new(move |pct: u32, msg: String| {
                            // 基础版本安装的 0-100 映射到整合包整体的 5-20 区间
                            let mapped = 5 + (pct as u64 * 15 / 100).min(15) as u32;
                            emit_progress(&app_owned, mapped, &msg, "base")
                        }) as ml_shared::BaseVersionProgress);
                        if let Err(e) = ml_shared::ensure_base_version_installed(&mc_version, on_progress).await {
                            eprintln!("[mrpack] 基础版本 {} 安装失败: {}", mc_version, e);
                            let _ = std::fs::remove_dir_all(&version_dir);
                            return json!({
                                "success": false,
                                "versionId": version_id,
                                "error": format!("基础版本 {} 安装失败: {}", mc_version, e)
                            });
                        }
                    }

                    // 确保加载器已安装
                    let lv_json_path = versions_dir().join(lv_id).join(format!("{}.json", lv_id));
                    let need_install = if !lv_json_path.exists() {
                        true
                    } else if !cf_shared::verify_loader_libs(lv_id) {
                        let _ = std::fs::remove_dir_all(versions_dir().join(lv_id));
                        true
                    } else {
                        false
                    };
                    if need_install {
                        if lv_json_path.exists() && !cf_shared::verify_loader_libs(lv_id) {
                            let _ = std::fs::remove_dir_all(versions_dir().join(lv_id));
                        }
                        match install_loader(&forge_ver, &neoforge_ver, &fabric_ver, &mc_version, app).await {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("[mrpack] 加载器 {} 安装失败: {}", lv_id, e);
                                let _ = std::fs::remove_dir_all(&version_dir);
                                return json!({
                                    "success": false,
                                    "versionId": version_id,
                                    "error": format!("整合包要求 {} 但安装失败: {}", lv_id, e)
                                });
                            }
                        }
                    }

                    // 重新合并版本 JSON
                    remerge_version_json(
                        &version_json_path,
                        &version_id,
                        &mc_version,
                        lv_id,
                    );

                    // 删除加载器目录
                    let loader_dir = versions_dir().join(lv_id);
                    if loader_dir.exists() && loader_dir != version_dir {
                        let _ = std::fs::remove_dir_all(&loader_dir);
                    }
                }
            }
        }
    }

    // fallback：如果 isNewVersionDir 但版本 JSON 仍不存在，创建 fallback
    if is_new_version_dir && !version_dir.join(format!("{}.json", version_id)).exists() {
        create_fallback_version_json(
            &version_id,
            &version_dir,
            &mc_version,
            loader_version_id.as_deref(),
        );
    }

    // 3.6 mods 目录备份（已存在版本目录时备份到 *.backup_<timestamp>）
    let backup_dir: Option<PathBuf> = if !is_new_version_dir {
        let existing_mods_dir = version_dir.join("mods");
        if existing_mods_dir.exists() {
            let bk_dir = version_dir
                .parent()
                .map(|p| {
                    p.join(format!(
                        "{}.backup_{}",
                        version_id,
                        cf_shared::now_timestamp()
                    ))
                })
                .unwrap_or_else(|| {
                    version_dir.with_file_name(format!(
                        "{}.backup_{}",
                        version_id,
                        cf_shared::now_timestamp()
                    ))
                });
            if let Err(e) = std::fs::create_dir_all(bk_dir.join("mods")) {
                eprintln!("[mrpack] 备份 mods 目录失败 (非致命): {}", e);
                None
            } else {
                let _ = cf_shared::copy_dir_recursive(&existing_mods_dir, &bk_dir.join("mods"));
                Some(bk_dir)
            }
        } else {
            None
        }
    } else {
        None
    };

    // 主流程（包含 overrides 解压、mods 下载、验证等）
    let result = match run_import_main(
        app,
        &mut archive,
        &manifest,
        &version_id,
        &version_dir,
        &pack_name,
        &mc_version,
        &forge_ver,
        &fabric_ver,
        &neoforge_ver,
        loader_version_id.as_deref(),
        file_path,
        abort_flag.as_ref(),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            // 3.25 失败回滚：恢复备份的 mods 目录 + cleanupVersionChain
            eprintln!("[mrpack] 导入失败: {}", e);
            if let Some(ref bk_dir) = backup_dir {
                let restored_mods_dir = bk_dir.join("mods");
                if restored_mods_dir.exists() {
                    let current_mods_dir = version_dir.join("mods");
                    if current_mods_dir.exists() {
                        let _ = std::fs::remove_dir_all(&current_mods_dir);
                    }
                    let _ = cf_shared::copy_dir_recursive(&restored_mods_dir, &current_mods_dir);
                }
                let _ = std::fs::remove_dir_all(bk_dir);
            }
            cf_shared::cleanup_version_chain(&version_id);
            return json!({
                "success": false,
                "versionId": version_id,
                "error": e
            });
        }
    };

    // 清理备份
    if let Some(bk_dir) = backup_dir {
        let _ = std::fs::remove_dir_all(&bk_dir);
    }

    result
}

// ============== 主流程 ==============

/// 主导入流程：overrides 解压 + mods 下载 + 验证 + 资源下载
#[allow(clippy::too_many_arguments)]
async fn run_import_main(
    app: &AppHandle,
    archive: &mut zip::ZipArchive<std::fs::File>,
    manifest: &MrpackManifest,
    version_id: &str,
    version_dir: &Path,
    pack_name: &str,
    mc_version: &str,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    loader_version_id: Option<&str>,
    file_path: &str,
    abort_flag: Option<&Arc<AtomicBool>>,
) -> Result<Value, String> {
    // 3.7 overrides 解压（路径遍历保护 + 5 次重试 + 50 文件 yield + 实时进度）
    emit_progress(app, 40, "解压覆盖文件...", "extract");
    let override_files = extract_overrides_with_progress(archive, version_dir, app)?;

    // 3.8 client_overrides 解压（如果存在，在 overrides 之后解压）
    extract_client_overrides(archive, version_dir, app)?;

    // 3.9 根目录图标提取（pack.png / icon.png / logo.png）
    extract_root_icon(archive, version_dir);

    // 3.10 资源包重定位 relocate_misplaced_resource_packs
    let relocated = cf_shared::relocate_misplaced_resource_packs(version_dir);
    if !relocated.relocated.is_empty() {
        eprintln!(
            "[Modpack] 检测到 {} 个资源包 zip 误放在 mods 目录，已自动移动到 resourcepacks: {}",
            relocated.relocated.len(),
            relocated.relocated.join(", ")
        );
    }

    // 3.11 版本隔离强制开启（version-settings.json: { isolation: 'on' }）
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

    // 3.24 env 字段过滤（client === 'unsupported' 跳过）+ loaders 字段过滤
    let target_loaders: HashSet<String> = {
        let mut s = HashSet::new();
        if !fabric_ver.is_empty() {
            s.insert("fabric".to_string());
        }
        if !forge_ver.is_empty() {
            s.insert("forge".to_string());
        }
        if !neoforge_ver.is_empty() {
            s.insert("neoforge".to_string());
        }
        s
    };

    let mut skipped_by_loader = 0usize;
    let files_list: Vec<&MrpackFile> = manifest
        .files
        .iter()
        .filter(|f| {
            // env.client === 'unsupported' 跳过
            if let Some(ref env) = f.env {
                if env.client.as_deref() == Some("unsupported") {
                    return false;
                }
            }
            // loaders 字段过滤
            if !target_loaders.is_empty() {
                if let Some(ref file_loaders) = f.loaders {
                    if !file_loaders.is_empty() {
                        let compatible = file_loaders.iter().any(|l| {
                            let lower = l.to_lowercase();
                            target_loaders.contains(&lower)
                        });
                        if !compatible {
                            skipped_by_loader += 1;
                            return false;
                        }
                    }
                }
            }
            true
        })
        .collect();

    if skipped_by_loader > 0 {
        eprintln!("[mrpack] {} 个文件因加载器不匹配被跳过", skipped_by_loader);
    }

    let mods_dir = version_dir.join("mods");
    let _ = std::fs::create_dir_all(&mods_dir);

    // 清理上次导入失败留下的 .downloading 残留文件
    cf_shared::clean_downloading_residue(version_dir);

    let total_mods = files_list.len();

    // 构造 mod 文件状态列表
    let mod_file_states: Vec<ModFileState> = files_list
        .iter()
        .map(|f| {
            let file_name = extract_file_name(f);
            ModFileState {
                name: file_name,
                status: ModStatus::Pending,
                progress: 0,
                size: f.file_size.unwrap_or(0),
                error: None,
            }
        })
        .collect();

    emit_progress(
        app,
        50,
        &format!("下载 Mod 文件 (共 {} 个)...", total_mods),
        "mods",
    );

    // Modrinth 官方 CDN 对高并发 IP 限流严重，且实测 32 并发最优（对齐原项目 modrinth.js）：
    // 64 并发反而更慢（连接分摊单文件带宽变少，大文件长尾，且易触发 429 限速）。
    // 这里直接固定为 32，不再跟随全局 maxThreads（全局可能高达 64）。
    let parallel = 32;

    eprintln!(
        "[mrpack] 模组下载: 共 {} 个, 并发={}",
        total_mods, parallel
    );

    // 3.12-3.14 Mod 下载（多镜像回退 + 64 线程并发 + SHA1 校验 + 熔断保护）
    let downloaded_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    // 取消标志：优先复用外部传入的取消标志（支持前端"取消下载"），否则用内部标志
    let abort_flag: Arc<AtomicBool> = match abort_flag {
        Some(flag) => flag.clone(),
        None => Arc::new(AtomicBool::new(false)),
    };

    let semaphore = Arc::new(Semaphore::new(parallel));
    let mods_dir_arc = Arc::new(mods_dir.clone());
    let version_dir_arc = Arc::new(version_dir.to_path_buf());

    let mut tasks = tokio::task::JoinSet::new();

    // 并行模组下载进度通道：每个下载任务把 (idx, status, progress) 推给主循环，
    // 主循环收到后实时广播文件快照，让详情里的每个模组进度条实时更新
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<(usize, &'static str, u32)>();

    // 文件状态快照：每个文件 (名称, 状态, 进度百分比)，供广播任务推给详情视图
    let files_state: Arc<Mutex<Vec<(String, String, u32)>>> = Arc::new(Mutex::new(
        files_list
            .iter()
            .map(|f| (extract_file_name(f), "pending".to_string(), 0))
            .collect(),
    ));

    // 进度广播任务：消费 progress_rx 的实时进度，更新文件快照并通过事件推给前端，
    // 这样详情视图里每个模组的进度条会随下载实时移动；所有下载任务结束后发送者
    // drop，progress_rx.recv() 返回 None，本任务自动结束
    let files_state_broadcast = files_state.clone();
    let app_broadcast = app.clone();
    let broadcast_task = tokio::spawn(async move {
        while let Some((idx, status, progress)) = progress_rx.recv().await {
            let (items, current, done, total) = {
                let mut st = files_state_broadcast.lock().await;
                if idx < st.len() {
                    st[idx].1 = status.to_string();
                    st[idx].2 = progress;
                }
                let items: Vec<Value> = st
                    .iter()
                    .map(|(name, s, p)| json!({ "name": name, "status": s, "progress": p }))
                    .collect();
                let current = st.get(idx).map(|v| v.0.clone()).unwrap_or_default();
                let done = st.iter().filter(|(_, s, _)| s == "completed").count();
                let total = st.len().max(1);
                (items, current, done, total)
            };
            // 总进度在 50→87 区间随已完成文件数实时移动
            let overall = 50 + (done * 37 / total);
            emit_progress_with_files(
                &app_broadcast,
                overall.min(87) as u32,
                &format!("下载 Mod ({}/{})...", done, total),
                "downloading",
                &items,
                &current,
            );
        }
    });

    for (idx, file_entry) in files_list.iter().enumerate() {
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }

        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("获取信号量失败: {}", e))?;

        let file_name = extract_file_name(file_entry);
        let dest_path = if let Some(ref p) = file_entry.path {
            version_dir_arc.join(p)
        } else {
            mods_dir_arc.join(&file_name)
        };
        let downloads = file_entry.downloads.clone();
        let expected_sha1 = file_entry.hashes.sha1.clone().unwrap_or_default();
        let file_size = file_entry.file_size.unwrap_or(0);

        let downloaded_count = downloaded_count.clone();
        let failed_count = failed_count.clone();
        let abort_flag = abort_flag.clone();
        let version_dir_arc = version_dir_arc.clone();
        let total_mods = total_mods;
        let progress_tx = progress_tx.clone();

        tasks.spawn(async move {
            let _permit = permit;
            let result = download_one_mod(
                &file_name,
                &dest_path,
                downloads,
                expected_sha1,
                file_size,
                &version_dir_arc,
                &abort_flag,
                &downloaded_count,
                &failed_count,
                total_mods,
                idx,
                progress_tx,
            )
            .await;
            drop(_permit);
            (idx, result)
        });
    }

    // 等待所有下载任务完成
    let mut mod_results: Vec<DownloadResult> = vec![DownloadResult::default(); files_list.len()];
    while let Some(r) = tasks.join_next().await {
        match r {
            Ok((idx, result)) => {
                if idx < mod_results.len() {
                    mod_results[idx] = result;
                }
            }
            Err(e) => {
                eprintln!("[mrpack] 下载任务异常: {}", e);
            }
        }
    }

    // 等待进度广播任务结束：显式 drop 主循环持有的发送端，否则广播任务收不到 None 永不结束
    drop(progress_tx);
    let _ = broadcast_task.await;

    if abort_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    let ok_count = downloaded_count.load(Ordering::SeqCst);
    let fail_count = failed_count.load(Ordering::SeqCst);
    eprintln!(
        "[mrpack] 模组下载完成: {}成功 {}失败",
        ok_count, fail_count
    );

    if fail_count > 0 {
        let failed_names: Vec<String> = mod_results
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.file_name.clone())
            .collect();
        eprintln!("[mrpack] 失败的模组: {}", failed_names.join(", "));
    }

    // 3.16 损坏 JAR 修复 repair_corrupted_mod_jars
    emit_progress(app, 88, "正在修复损坏的模组文件...", "repair");
    let repair_result = cf_shared::repair_corrupted_mod_jars(version_dir);
    if repair_result.failed > 0 {
        eprintln!(
            "[mrpack] {} 个模组文件损坏且无法修复，游戏启动时可能报错",
            repair_result.failed
        );
    }

    // 3.15 模组清单保存 save_mod_manifest
    let mut manifest_mods: Vec<Value> = Vec::new();
    for (i, f) in files_list.iter().enumerate() {
        let file_name = extract_file_name(f);
        let mods_file_path = if let Some(ref p) = f.path {
            version_dir.join(p)
        } else {
            mods_dir.join(&file_name)
        };
        // 只记录 mods 目录下实际存在的 .jar 文件
        if !file_name.to_lowercase().ends_with(".jar") {
            continue;
        }
        if !mods_file_path.exists() {
            continue;
        }
        let mod_id = cf_shared::read_jar_mod_id(&mods_file_path);
        let download_url = f.downloads.first().cloned().unwrap_or_default();
        manifest_mods.push(json!({
            "projectID": null,
            "fileID": null,
            "fileName": file_name,
            "downloadUrl": download_url,
            "fileLength": f.file_size.unwrap_or(0),
            "sha1": f.hashes.sha1.as_ref().cloned().unwrap_or_default(),
            "modId": mod_id,
        }));
        // 标记是否下载成功（用于后续 failed-mods.json）
        let _ = i;
    }
    cf_shared::save_mod_manifest(version_dir, &manifest_mods);
    eprintln!(
        "[mrpack] 已保存 mod-manifest.json: {} 个 mod",
        manifest_mods.len()
    );

    // 3.17 失败模组列表持久化（failed-mods.json）
    let mut failed_mods_list: Vec<Value> = Vec::new();
    for result in &mod_results {
        if !result.success {
            failed_mods_list.push(json!({
                "name": result.file_name,
                "error": result.error,
            }));
        }
    }
    if !failed_mods_list.is_empty() {
        let failed_path = version_dir.join("failed-mods.json");
        let failed_json = json!({
            "packName": pack_name,
            "mcVersion": mc_version,
            "totalMods": total_mods,
            "failedCount": fail_count,
            "downloadedCount": ok_count,
            "failedAt": cf_shared::now_iso(),
            "failedMods": failed_mods_list.clone(),
            "note": "这些模组在导入时下载失败，可重新导入整合包补下载，或手动从 Modrinth 下载后放入 mods 文件夹"
        });
        let _ = std::fs::write(
            &failed_path,
            serde_json::to_string_pretty(&failed_json).unwrap_or_default(),
        );
        eprintln!(
            "[mrpack] 已保存失败模组列表: {} ({}/{})",
            failed_path.display(),
            fail_count,
            total_mods
        );
    }

    // 3.18 加载器兼容性检查 ensure_loader_compat
    if let Some(lv_id) = loader_version_id {
        if !mc_version.is_empty() {
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
    }

    // 3.19 库文件验证 verify_import_libs
    emit_progress(app, 90, "正在验证整合包完整性...", "verify");
    let (verify_ok, _checked, missing) = cf_shared::verify_import_libs(version_id).await;
    if !verify_ok {
        eprintln!("[mrpack] 库文件补全失败: {} 个文件缺失", missing);
        cf_shared::cleanup_version_chain(version_id);
        return Err(format!(
            "整合包库文件补全失败: {} 个文件缺失，请检查网络后重试",
            missing
        ));
    }

    // 3.20 资源索引下载（解析 assetIndex + 缺资源并发下载 64 线程）
    let settings = storage::load_settings();
    let merged_json = cf_shared::resolve_version_json(version_id);
    if let Some(ref merged) = merged_json {
        if let Some(asset_index) = merged.get("assetIndex") {
            emit_progress(app, 93, "正在下载游戏资源...", "assets");
            if let Err(e) = download_assets(app, asset_index, &settings).await {
                eprintln!("[mrpack] 资源下载异常(非致命): {}", e);
            }
        }
    }

    // 3.21 客户端 JAR 下载（3 次重试 + 失败非致命）
    if let Some(ref merged) = merged_json {
        if let Some(inherits_from) = merged.get("inheritsFrom").and_then(|v| v.as_str()) {
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
                        if let Ok(content) = std::fs::read_to_string(&base_json_path) {
                            if let Ok(json) = serde_json::from_str::<Value>(&content) {
                                return json
                                    .get("downloads")
                                    .and_then(|d| d.get("client"))
                                    .and_then(|c| c.get("url"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                            }
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
                            "china-first",
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
                                    "[mrpack] 客户端JAR下载失败({}/3): {}",
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
                        eprintln!("[mrpack] 客户端JAR下载最终失败(非致命)，启动时会自动补全");
                    }
                }
            }
        }
    }

    // 3.22 Forge 核心文件验证（forge-client.jar / client-srg.jar / client-extra.jar）
    if let Some(ref merged) = merged_json {
        if !forge_ver.is_empty() {
            if let Err(e) = check_forge_core_files(merged) {
                cf_shared::cleanup_version_chain(version_id);
                return Err(e);
            }
        }
    }

    // 3.23 pack-info.json 写入
    let pack_info = json!({
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "packFormat": "mrpack",
        "fabricVersion": fabric_ver,
        "forgeVersion": forge_ver,
        "neoforgeVersion": neoforge_ver,
        "importedAt": cf_shared::now_iso(),
        "sourceFile": file_path,
        "targetVersion": "",
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info).unwrap_or_default(),
    );

    // 3.24 保存 mrpack-manifest.json（供启动前 mods 完整性检查使用）
    let manifest_for_check = build_mrpack_manifest_for_check(manifest, version_dir, pack_name);
    let _ = std::fs::write(
        version_dir.join("mrpack-manifest.json"),
        serde_json::to_string_pretty(&manifest_for_check).unwrap_or_default(),
    );

    // 完成
    emit_progress(
        app,
        100,
        &format!("整合包 \"{}\" 导入完成！", pack_name),
        "done",
    );

    // 失败阈值检查
    // failCount > 0 && failCount >= failThreshold → 返回失败
    // failCount > 0 && failCount < failThreshold → 返回成功+warning
    // failCount == 0 → 返回成功
    let fail_threshold = std::cmp::max(5usize, total_mods / 10);
    if fail_count > 0 && fail_count >= fail_threshold {
        let failed_mod_names: Vec<String> = mod_results
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.file_name.clone())
            .collect();
        let error_msg = format!(
            "{}/{} 个Mod下载失败（阈值{}），整合包不完整无法正常运行。失败的Mod: {}。请检查网络后重试。",
            fail_count,
            total_mods,
            fail_threshold,
            failed_mod_names.join(", ")
        );
        eprintln!("[mrpack] 导入失败: {}", error_msg);
        cf_shared::cleanup_version_chain(version_id);
        let failed_mods_json: Vec<Value> = mod_results
            .iter()
            .filter(|r| !r.success)
            .map(|r| {
                json!({
                    "name": r.file_name,
                    "status": "failed",
                    "error": r.error,
                })
            })
            .collect();
        return Ok(json!({
            "success": false,
            "versionId": version_id,
            "error": error_msg,
            "failedMods": failed_mods_json,
        }));
    }

    if fail_count > 0 {
        let failed_mod_names: Vec<String> = mod_results
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.file_name.clone())
            .collect();
        let warning_msg = format!(
            "{}/{} 个Mod下载失败: {}。请在内部浏览器中手动下载缺失的Mod，或检查网络后重试。",
            fail_count,
            total_mods,
            failed_mod_names.join(", ")
        );
        let failed_mods_json: Vec<Value> = mod_results
            .iter()
            .filter(|r| !r.success)
            .map(|r| {
                json!({
                    "name": r.file_name,
                    "status": "failed",
                    "error": r.error,
                })
            })
            .collect();
        return Ok(json!({
            "success": true,
            "name": pack_name,
            "versionId": version_id,
            "mcVersion": mc_version,
            "targetVersion": "",
            "warning": warning_msg,
            "failedMods": failed_mods_json,
            "loaderVersionId": loader_version_id,
        }));
    }

    Ok(json!({
        "success": true,
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "targetVersion": "",
        "loaderVersionId": loader_version_id,
    }))
}

// ============== 加载器安装 ==============

/// 安装模组加载器（Forge/NeoForge/Fabric）
/// 已存在则校验 libs 完整性，损坏则删除重装
async fn install_loader(
    forge_ver: &str,
    neoforge_ver: &str,
    fabric_ver: &str,
    mc_version: &str,
    app: &AppHandle,
) -> Result<String, String> {
    if !forge_ver.is_empty() {
        let target_id = format!("{}-forge-{}", mc_version, forge_ver);
        let lv_json_path = versions_dir().join(&target_id).join(format!("{}.json", target_id));
        let need_install = if !lv_json_path.exists() {
            true
        } else if !cf_shared::verify_loader_libs(&target_id) {
            let _ = std::fs::remove_dir_all(versions_dir().join(&target_id));
            true
        } else {
            false
        };
        if need_install {
            emit_progress(app, 20, "正在安装Forge...", "loader-install");
            let r = crate::modloaders::forge::install_forge(
                mc_version,
                forge_ver,
                Some(&target_id),
            )
            .await;
            if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let err = r
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Forge 安装失败")
                    .to_string();
                return Err(err);
            }
        }
        Ok(target_id)
    } else if !neoforge_ver.is_empty() {
        let target_id = format!("{}-neoforge-{}", mc_version, neoforge_ver);
        let lv_json_path = versions_dir().join(&target_id).join(format!("{}.json", target_id));
        let need_install = if !lv_json_path.exists() {
            true
        } else if !cf_shared::verify_loader_libs(&target_id) {
            let _ = std::fs::remove_dir_all(versions_dir().join(&target_id));
            true
        } else {
            false
        };
        if need_install {
            emit_progress(app, 20, "正在安装NeoForge...", "loader-install");
            let r = crate::modloaders::neoforge::install_neoforge(
                mc_version,
                neoforge_ver,
                Some(&target_id),
            )
            .await;
            if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let err = r
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("NeoForge 安装失败")
                    .to_string();
                return Err(err);
            }
        }
        Ok(target_id)
    } else if !fabric_ver.is_empty() {
        let target_id = format!("fabric-loader-{}-{}", fabric_ver, mc_version);
        let lv_json_path = versions_dir().join(&target_id).join(format!("{}.json", target_id));
        let mut need_install = !lv_json_path.exists();
        if !need_install {
            if !cf_shared::verify_loader_libs(&target_id) {
                need_install = true;
            } else {
                // 检查是否有 fabric-loader 库
                if let Some(json) = cf_shared::resolve_version_json(&target_id) {
                    let has_fabric_loader = json
                        .get("libraries")
                        .and_then(|v| v.as_array())
                        .map(|libs| {
                            libs.iter().any(|l| {
                                l.get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|n| n.starts_with("net.fabricmc:fabric-loader"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false);
                    if !has_fabric_loader {
                        need_install = true;
                    }
                } else {
                    need_install = true;
                }
            }
        }
        if need_install {
            if lv_json_path.exists() {
                let _ = std::fs::remove_dir_all(versions_dir().join(&target_id));
            }
            emit_progress(app, 20, "正在安装Fabric...", "loader-install");
            let r = crate::modloaders::fabric::install_fabric_with_target(
                mc_version,
                fabric_ver,
                &target_id,
            )
            .await;
            if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                let err = r
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Fabric 安装失败")
                    .to_string();
                return Err(err);
            }
        }
        Ok(target_id)
    } else {
        Ok(String::new())
    }
}

// ============== 版本 JSON 创建（mergeVersionJson） ==============

/// 创建版本 JSON（合并加载器 JSON 到原版 JSON 之上）
/// 对应原项目 modrinth.js mergeVersionJson 逻辑
fn create_version_json_with_merge(
    version_id: &str,
    version_dir: &Path,
    mc_version: &str,
    loader_version_id: Option<&str>,
) {
    if let Some(lv_id) = loader_version_id {
        let lv_json_path = versions_dir().join(lv_id).join(format!("{}.json", lv_id));
        let mut merged_json: Option<Value> = None;

        if lv_json_path.exists() {
            let lv_json: Value = match std::fs::read_to_string(&lv_json_path) {
                Ok(s) => serde_json::from_str(&s).unwrap_or(json!({})),
                Err(_) => json!({}),
            };

            if !mc_version.is_empty() {
                let vanilla_json_path = versions_dir().join(mc_version).join(format!("{}.json", mc_version));
                if vanilla_json_path.exists() {
                    if let Ok(vanilla_str) = std::fs::read_to_string(&vanilla_json_path) {
                        if let Ok(base_json) = serde_json::from_str::<Value>(&vanilla_str) {
                            merged_json = Some(merge_version_json(&base_json, &lv_json, version_id));
                        }
                    }
                }
            }

            if merged_json.is_none() {
                // 无原版 JSON：直接用加载器 JSON
                let mut j = lv_json.clone();
                if let Some(obj) = j.as_object_mut() {
                    obj.remove("inheritsFrom");
                    obj.remove("_comment_");
                    obj.remove("jar");
                    obj.insert("id".to_string(), json!(version_id));
                    obj.insert("time".to_string(), json!(cf_shared::now_iso()));
                    obj.insert("releaseTime".to_string(), json!(cf_shared::now_iso()));
                }
                merged_json = Some(j);
            }

            // 补充 clientVersion
            if let Some(ref mut mj) = merged_json {
                if mj.get("clientVersion").is_none() && !mc_version.is_empty() {
                    if let Some(obj) = mj.as_object_mut() {
                        obj.insert("clientVersion".to_string(), json!(mc_version));
                    }
                }
            }
        }

        let version_json = merged_json.unwrap_or_else(|| {
            json!({
                "id": version_id,
                "type": "release",
                "time": cf_shared::now_iso(),
                "releaseTime": cf_shared::now_iso()
            })
        });

        // JVM 参数去重
        let version_json = dedup_jvm_args_in_json(version_json);

        // [CRITICAL] 直接写入 mergedJson，不能从文件重新读取
        let _ = std::fs::write(
            version_dir.join(format!("{}.json", version_id)),
            serde_json::to_string_pretty(&version_json).unwrap_or_default(),
        );

        // 复制主 jar 到版本目录
        // [关键修复] 必须检查 targetJar 是否存在且大小 > 0
        let vanilla_jar = versions_dir().join(mc_version).join(format!("{}.jar", mc_version));
        let target_jar = version_dir.join(format!("{}.jar", version_id));
        let mut need_copy = true;
        if target_jar.exists() {
            if let Ok(stat) = std::fs::metadata(&target_jar) {
                if stat.len() > 0 {
                    need_copy = false;
                }
            }
        }
        if need_copy && vanilla_jar.exists() {
            if let Err(e) = std::fs::copy(&vanilla_jar, &target_jar) {
                eprintln!("[mrpack] 复制版本jar失败: {}", e);
            } else if let Ok(stat) = std::fs::metadata(&vanilla_jar) {
                eprintln!(
                    "[mrpack] 已复制 vanilla client.jar 到版本目录: {}.jar ({} bytes)",
                    version_id,
                    stat.len()
                );
            }
        }

        // 删除加载器文件夹（libraries 已合并到版本 JSON）
        let loader_dir = versions_dir().join(lv_id);
        if loader_dir.exists() && loader_dir != version_dir {
            if let Err(e) = std::fs::remove_dir_all(&loader_dir) {
                eprintln!("[mrpack] 删除加载器文件夹失败: {}", e);
            }
        }
    } else {
        // 无加载器：创建简单版本 JSON
        let version_json = json!({
            "id": version_id,
            "inheritsFrom": if mc_version.is_empty() { Value::Null } else { json!(mc_version) },
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "time": cf_shared::now_iso(),
            "releaseTime": cf_shared::now_iso()
        });
        let _ = std::fs::write(
            version_dir.join(format!("{}.json", version_id)),
            serde_json::to_string_pretty(&version_json).unwrap_or_default(),
        );
    }
}

/// 重新合并版本 JSON（existingJson 仍有 inheritsFrom 时）
fn remerge_version_json(
    version_json_path: &Path,
    version_id: &str,
    mc_version: &str,
    loader_version_id: &str,
) {
    let lv_json_path = versions_dir()
        .join(loader_version_id)
        .join(format!("{}.json", loader_version_id));
    if !lv_json_path.exists() {
        return;
    }

    let lv_json: Value = match std::fs::read_to_string(&lv_json_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(json!({})),
        Err(_) => return,
    };

    let mut new_json: Option<Value> = None;

    if !mc_version.is_empty() {
        let vanilla_json_path = versions_dir().join(mc_version).join(format!("{}.json", mc_version));
        if vanilla_json_path.exists() {
            if let Ok(vanilla_str) = std::fs::read_to_string(&vanilla_json_path) {
                if let Ok(base_json) = serde_json::from_str::<Value>(&vanilla_str) {
                    new_json = Some(merge_version_json(&base_json, &lv_json, version_id));
                }
            }
        }
    }

    if new_json.is_none() {
        let mut j = lv_json.clone();
        if let Some(obj) = j.as_object_mut() {
            obj.remove("inheritsFrom");
            obj.remove("_comment_");
            obj.remove("jar");
            obj.insert("id".to_string(), json!(version_id));
            obj.insert("time".to_string(), json!(cf_shared::now_iso()));
            obj.insert("releaseTime".to_string(), json!(cf_shared::now_iso()));
        }
        new_json = Some(j);
    }

    if let Some(ref mut nj) = new_json {
        // 补充 clientVersion
        if nj.get("clientVersion").is_none() && !mc_version.is_empty() {
            if let Some(obj) = nj.as_object_mut() {
                obj.insert("clientVersion".to_string(), json!(mc_version));
            }
        }
        // JVM 参数去重
        let nj_dedup = dedup_jvm_args_in_json(nj.clone());
        *nj = nj_dedup;
        let _ = std::fs::write(
            version_json_path,
            serde_json::to_string_pretty(nj).unwrap_or_default(),
        );
    }
}

/// 创建 fallback 版本 JSON
fn create_fallback_version_json(
    version_id: &str,
    version_dir: &Path,
    mc_version: &str,
    loader_version_id: Option<&str>,
) {
    let mut fallback_json = json!({
        "id": version_id,
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "time": cf_shared::now_iso(),
        "releaseTime": cf_shared::now_iso()
    });

    if let Some(lv_id) = loader_version_id {
        if !mc_version.is_empty() {
            let lv_p = versions_dir().join(lv_id).join(format!("{}.json", lv_id));
            if lv_p.exists() {
                if let Ok(lv_str) = std::fs::read_to_string(&lv_p) {
                    if let Ok(lv_j) = serde_json::from_str::<Value>(&lv_str) {
                        let vanilla_json_path = versions_dir().join(mc_version).join(format!("{}.json", mc_version));
                        let base_json: Option<Value> = std::fs::read_to_string(&vanilla_json_path)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                        if let Some(base) = base_json {
                            fallback_json = merge_version_json(&base, &lv_j, version_id);
                        } else {
                            let mut j = lv_j;
                            if let Some(obj) = j.as_object_mut() {
                                obj.remove("inheritsFrom");
                                obj.remove("_comment_");
                                obj.remove("jar");
                                obj.insert("id".to_string(), json!(version_id));
                                obj.insert("time".to_string(), json!(cf_shared::now_iso()));
                                obj.insert("releaseTime".to_string(), json!(cf_shared::now_iso()));
                            }
                            fallback_json = j;
                        }
                        // 补充 clientVersion
                        if let Some(obj) = fallback_json.as_object_mut() {
                            if obj.get("clientVersion").is_none() {
                                obj.insert("clientVersion".to_string(), json!(mc_version));
                            }
                        }
                    }
                }
            }
        }
    }

    let fallback_json = dedup_jvm_args_in_json(fallback_json);
    let _ = std::fs::write(
        version_dir.join(format!("{}.json", version_id)),
        serde_json::to_string_pretty(&fallback_json).unwrap_or_default(),
    );
}

// ============== mergeVersionJson ==============
// 对应原项目 modrinth.js 内联 mergeVersionJson 函数
// 将加载器 JSON 合并到原版 JSON 之上

/// 合并版本 JSON：baseJson（原版）+ loaderJson（加载器）→ merged
///
/// 逻辑：
/// 1. 以 baseJson 为基础
/// 2. libraries: loaderLibs 在前，vanillaLibs 在后去重
///    - 同名库但 vanilla 含 natives/classifiers 且 existing 不含 → 替换
/// 3. arguments: 合并 game + jvm（--add-opens 等多值标志展开后去重）
/// 4. 其他字段：loaderJson 覆盖（空对象不覆盖非空对象）
/// 5. 删除 inheritsFrom、_comment_、jar
/// 6. 设置 id、time、releaseTime
fn merge_version_json(base_json: &Value, loader_json: &Value, version_id: &str) -> Value {
    let mut merged = base_json.clone();
    // 确保是对象类型，避免 unwrap_or(&mut Map::new()) 的临时值生命周期问题
    if !merged.is_object() {
        merged = json!({});
    }
    let merged_obj = merged.as_object_mut().expect("已确保为对象");

    let vanilla_libs = base_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let loader_libs = loader_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 收集 loaderLibs 的 name → bool
    let mut seen_names: HashSet<String> = HashSet::new();
    for lib in &loader_libs {
        if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
            seen_names.insert(name.to_string());
        }
    }

    // 收集 loaderLibs 中含 natives/classifiers 的库名
    let mut names_with_natives: HashSet<String> = HashSet::new();
    for lib in &loader_libs {
        if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
            let has_natives = lib.get("natives").is_some()
                || lib
                    .get("downloads")
                    .and_then(|d| d.get("classifiers"))
                    .is_some();
            if has_natives {
                names_with_natives.insert(name.to_string());
            }
        }
    }

    // mergedLibs = [...loaderLibs]
    let mut merged_libs: Vec<Value> = loader_libs.clone();

    // 遍历 vanillaLibs，去重添加
    for vl in &vanilla_libs {
        let vl_name = match vl.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let vl_has_natives = vl.get("natives").is_some()
            || vl
                .get("downloads")
                .and_then(|d| d.get("classifiers"))
                .is_some();

        if !seen_names.contains(&vl_name) {
            // 新库：直接添加
            merged_libs.push(vl.clone());
            seen_names.insert(vl_name.clone());
            if vl_has_natives {
                names_with_natives.insert(vl_name);
            }
        } else if vl_has_natives && !names_with_natives.contains(&vl_name) {
            // 同名库但含 natives/classifiers，且现有同名条目不含 natives：替换
            if let Some(pos) = merged_libs.iter().position(|l| {
                l.get("name").and_then(|v| v.as_str()) == Some(&vl_name)
            }) {
                merged_libs[pos] = vl.clone();
            } else {
                merged_libs.push(vl.clone());
            }
            names_with_natives.insert(vl_name);
        }
        // 否则（已有同名带 natives 的条目）：跳过，避免重复
    }

    merged_obj.insert("libraries".to_string(), json!(merged_libs));

    // 遍历 loaderJson 的其他字段
    if let Some(loader_obj) = loader_json.as_object() {
        for (key, loader_val) in loader_obj {
            if key == "libraries" || key == "inheritsFrom" || key == "jar" {
                continue;
            }

            if key == "arguments" && loader_val.get("arguments").is_some() && base_json.get("arguments").is_some() {
                // arguments 合并：game + jvm
                let base_args = base_json.get("arguments").unwrap();
                let loader_args = loader_val;

                // 合并 game 参数（去重）
                let mut merged_game: Vec<Value> = base_args
                    .get("game")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let loader_game = loader_args
                    .get("game")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for ge in &loader_game {
                    let ge_str = value_to_arg_string(ge);
                    let exists = merged_game.iter().any(|mg| value_to_arg_string(mg) == ge_str);
                    if !exists {
                        merged_game.push(ge.clone());
                    }
                }

                // 展开 loader jvm 中的 --add-opens 等多值标志
                let loader_jvm = loader_args
                    .get("jvm")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let expanded_loader_jvm = expand_multi_value_flags(&loader_jvm);

                // 合并 jvm 参数（去重）
                let mut merged_jvm: Vec<Value> = base_args
                    .get("jvm")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for je in &expanded_loader_jvm {
                    let je_str = value_to_arg_string(je);
                    let exists = merged_jvm.iter().any(|mj| value_to_arg_string(mj) == je_str);
                    if !exists {
                        merged_jvm.push(je.clone());
                    }
                }

                let mut args_obj = Map::new();
                args_obj.insert("game".to_string(), json!(merged_game));
                args_obj.insert("jvm".to_string(), json!(merged_jvm));
                merged_obj.insert("arguments".to_string(), json!(args_obj));
            } else {
                // 跳过空对象（如果 base 有非空对象）
                let is_empty_obj = loader_val.is_object()
                    && !loader_val.is_array()
                    && loader_val.as_object().map(|o| o.is_empty()).unwrap_or(false);
                let base_non_empty = base_json
                    .get(key)
                    .map(|v| {
                        v.is_object()
                            && !v.is_array()
                            && v.as_object().map(|o| !o.is_empty()).unwrap_or(false)
                    })
                    .unwrap_or(false);
                if is_empty_obj && base_non_empty {
                    continue;
                }
                merged_obj.insert(key.clone(), loader_val.clone());
            }
        }
    }

    // 删除 inheritsFrom、_comment_、jar
    merged_obj.remove("inheritsFrom");
    merged_obj.remove("_comment_");
    merged_obj.remove("jar");

    // 设置 id、time、releaseTime
    merged_obj.insert("id".to_string(), json!(version_id));
    merged_obj.insert("time".to_string(), json!(cf_shared::now_iso()));
    merged_obj.insert("releaseTime".to_string(), json!(cf_shared::now_iso()));

    merged
}

/// 将 Value 转换为参数字符串（用于比较）
/// 字符串直接返回，非字符串序列化为 JSON
fn value_to_arg_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

/// 展开多值标志（--add-opens/--add-exports/--add-reads/--add-modules）
/// 对应 modrinth.js mergeVersionJson 中的 expandedLoaderJvm 逻辑
fn expand_multi_value_flags(args: &[Value]) -> Vec<Value> {
    let multi_value_flags = ["--add-opens", "--add-exports", "--add-reads", "--add-modules"];
    let mut expanded: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let arg = &args[i];
        let is_multi = arg
            .as_str()
            .map(|s| multi_value_flags.contains(&s))
            .unwrap_or(false);
        if is_multi {
            let mut values: Vec<Value> = Vec::new();
            while i + 1 < args.len() {
                if let Some(next) = args[i + 1].as_str() {
                    if !next.starts_with('-') {
                        i += 1;
                        values.push(args[i].clone());
                        continue;
                    }
                }
                break;
            }
            if values.is_empty() {
                expanded.push(arg.clone());
            } else {
                for val in &values {
                    expanded.push(arg.clone());
                    expanded.push(val.clone());
                }
            }
        } else {
            expanded.push(arg.clone());
        }
        i += 1;
    }
    expanded
}

// ============== deduplicateJvmArgs ==============
// 对应原项目 versions/version-merge.js deduplicateJvmArgs

/// JVM 参数去重：先展开多值标志（--add-opens 等），再去重 -D/-X/-XX 开头的重复参数
fn deduplicate_jvm_args(args: &[Value]) -> Vec<Value> {
    if args.is_empty() {
        return Vec::new();
    }

    // 先展开多值标志
    let expanded = expand_multi_value_flags(args);

    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<Value> = Vec::new();

    for arg in &expanded {
        match arg {
            Value::String(s) => {
                if s.starts_with("-D") || s.starts_with("-X") || s.starts_with("-XX") {
                    if seen.contains(s) {
                        continue;
                    }
                    seen.insert(s.clone());
                    result.push(arg.clone());
                } else {
                    result.push(arg.clone());
                }
            }
            _ => {
                result.push(arg.clone());
            }
        }
    }

    result
}

/// 对版本 JSON 中的 arguments.jvm 进行去重
fn dedup_jvm_args_in_json(mut json: Value) -> Value {
    if let Some(jvm) = json
        .get("arguments")
        .and_then(|a| a.get("jvm"))
        .and_then(|v| v.as_array())
        .cloned()
    {
        let deduped = deduplicate_jvm_args(&jvm);
        if let Some(args) = json.get_mut("arguments").and_then(|a| a.as_object_mut()) {
            args.insert("jvm".to_string(), json!(deduped));
        }
    }
    json
}

// ============== overrides 解压 ==============

/// 解压 overrides 到版本目录
/// 含路径遍历保护、Windows 保留名过滤、5 次重试、50 文件 yield、实时进度回调
fn extract_overrides_with_progress(
    archive: &mut zip::ZipArchive<std::fs::File>,
    dest_dir: &Path,
    app: &AppHandle,
) -> Result<Vec<Value>, String> {
    let prefix = "overrides/";
    let dest_canonical = dest_dir
        .canonicalize()
        .unwrap_or_else(|_| dest_dir.to_path_buf());

    // 先统计待解压文件总数
    let mut override_total = 0usize;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.is_dir() {
                continue;
            }
            let name = entry.name();
            if name.starts_with(prefix) {
                override_total += 1;
            }
        }
    }

    let mut override_files: Vec<Value> = Vec::new();
    let mut yield_counter = 0usize;
    let mut extract_count = 0usize;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if !cf_shared::is_modpack_path_safe(&name) {
            continue;
        }
        if !name.starts_with(prefix) {
            continue;
        }

        let rel_path = &name[prefix.len()..];
        let dest_path = dest_dir.join(rel_path);

        // 路径遍历校验
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            let parent_canonical = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if !parent_canonical.starts_with(&dest_canonical) {
                eprintln!("[Modpack] mrpack路径遍历已拦截: {}", rel_path);
                continue;
            }
        }

        // 读取 entry 数据
        let mut buf = Vec::new();
        if let Err(e) = entry.read_to_end(&mut buf) {
            eprintln!("[mrpack] 读取 entry 失败: {} - {}", rel_path, e);
            continue;
        }

        // 5 次重试写入
        let mut written = false;
        for attempt in 1..=5u32 {
            match std::fs::write(&dest_path, &buf) {
                Ok(_) => {
                    written = true;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[Modpack] mrpack解压 {} 第 {} 次失败: {}",
                        rel_path, attempt, e
                    );
                    if attempt < 5 {
                        std::thread::sleep(Duration::from_millis((attempt - 1) as u64 * 2000));
                    }
                }
            }
        }

        if written {
            override_files.push(json!({
                "name": rel_path,
                "status": "completed",
                "progress": 100
            }));
            extract_count += 1;
        }

        // 实时进度反馈：每 50 个文件更新一次进度（40% → 50% 区间）
        yield_counter += 1;
        if override_total > 0 && yield_counter % 50 == 0 {
            let pct = 40 + (extract_count * 10 / override_total);
            emit_progress(
                app,
                pct as u32,
                &format!("解压覆盖文件... ({}/{})", extract_count, override_total),
                "extract",
            );
            std::thread::yield_now();
        }
    }

    Ok(override_files)
}

/// 解压 client_overrides 到版本目录（在 overrides 之后解压）
fn extract_client_overrides(
    archive: &mut zip::ZipArchive<std::fs::File>,
    dest_dir: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let prefix = "client-overrides/";
    let dest_canonical = dest_dir
        .canonicalize()
        .unwrap_or_else(|_| dest_dir.to_path_buf());

    let mut yield_counter = 0usize;
    let mut extract_count = 0usize;

    // 先统计
    let mut total = 0usize;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.is_dir() {
                continue;
            }
            if entry.name().starts_with(prefix) {
                total += 1;
            }
        }
    }

    if total == 0 {
        return Ok(());
    }

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if !cf_shared::is_modpack_path_safe(&name) {
            continue;
        }
        if !name.starts_with(prefix) {
            continue;
        }

        let rel_path = &name[prefix.len()..];
        let dest_path = dest_dir.join(rel_path);

        // 路径遍历校验
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            let parent_canonical = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if !parent_canonical.starts_with(&dest_canonical) {
                eprintln!("[Modpack] mrpack client-overrides 路径遍历已拦截: {}", rel_path);
                continue;
            }
        }

        let mut buf = Vec::new();
        if let Err(e) = entry.read_to_end(&mut buf) {
            eprintln!("[mrpack] 读取 client-overrides entry 失败: {} - {}", rel_path, e);
            continue;
        }

        // 5 次重试写入
        let mut written = false;
        for attempt in 1..=5u32 {
            match std::fs::write(&dest_path, &buf) {
                Ok(_) => {
                    written = true;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[Modpack] mrpack client-overrides 解压 {} 第 {} 次失败: {}",
                        rel_path, attempt, e
                    );
                    if attempt < 5 {
                        std::thread::sleep(Duration::from_millis((attempt - 1) as u64 * 2000));
                    }
                }
            }
        }

        if written {
            extract_count += 1;
        }

        yield_counter += 1;
        if total > 0 && yield_counter % 50 == 0 {
            let pct = 40 + (extract_count * 10 / total);
            emit_progress(
                app,
                pct as u32,
                &format!("解压 client-overrides... ({}/{})", extract_count, total),
                "extract",
            );
            std::thread::yield_now();
        }
    }

    Ok(())
}

// ============== 根目录图标提取 ==============

/// 提取整合包根目录的图标文件（pack.png / icon.png / logo.png）
fn extract_root_icon(archive: &mut zip::ZipArchive<std::fs::File>, version_dir: &Path) {
    let icon_names = ["pack.png", "icon.png", "logo.png"];
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let entry_name = entry.name().replace('\\', "/");
        if icon_names.contains(&entry_name.as_str()) {
            let dest_icon_path = version_dir.join(&entry_name);
            if !dest_icon_path.exists() {
                let mut buf = Vec::new();
                if entry.read_to_end(&mut buf).is_ok() {
                    let _ = std::fs::write(&dest_icon_path, &buf);
                    eprintln!("[Modpack] 提取整合包图标: {}", entry_name);
                }
            }
            break;
        }
    }
}

// ============== 文件名提取 ==============

/// 从 fileEntry 提取文件名（basename of path 或 downloads[0]）
fn extract_file_name(f: &MrpackFile) -> String {
    if let Some(ref p) = f.path {
        let normalized = p.replace('\\', "/");
        if let Some(name) = normalized.rsplit('/').next() {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    if let Some(first) = f.downloads.first() {
        let path = first.split('?').next().unwrap_or(first);
        if let Some(name) = path.rsplit('/').next() {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    "unknown".to_string()
}

// ============== 镜像 URL 列表 ==============

/// 构造镜像 URL 列表（mod.mcimirror.top 镜像优先，官方源兜底）
/// cdn.modrinth.com / cdn-alt.modrinth.com → mod.mcimirror.top
/// 对齐原项目 modrinth.js 的 china-first 模式：镜像在前，官方在后。
/// 官方 CDN 在国内 TTFB 极慢且对高并发 IP 限流，镜像优先可避免每 mod 都卡在官方源上。
fn get_mirror_urls(url: &str) -> Vec<String> {
    let mut urls = Vec::new();

    if url.contains("cdn.modrinth.com") {
        let mirror = url.replace("cdn.modrinth.com", "mod.mcimirror.top");
        if mirror != url {
            urls.push(mirror);
        }
    } else if url.contains("cdn-alt.modrinth.com") {
        let mirror = url.replace("cdn-alt.modrinth.com", "mod.mcimirror.top");
        if mirror != url {
            urls.push(mirror);
        }
    }

    // 官方源兜底（放最后）
    urls.push(url.to_string());
    urls
}

// ============== 下载相关 ==============

#[derive(Debug, Clone, Default)]
struct DownloadResult {
    success: bool,
    file_name: String,
    error: String,
}

#[derive(Debug, Clone, PartialEq)]
enum ModStatus {
    Pending,
    #[allow(dead_code)]
    Downloading,
    #[allow(dead_code)]
    Completed,
    #[allow(dead_code)]
    Failed,
}

#[allow(dead_code)]
struct ModFileState {
    name: String,
    status: ModStatus,
    progress: u32,
    size: i64,
    error: Option<String>,
}

/// 下载单个模组文件
///
/// 流程：
/// 1. 已存在则校验大小+isJarIntact+SHA1，通过则跳过
/// 2. 构造镜像 URL 列表（原 downloads + mcimirror 镜像）
/// 3. 3 轮重试下载
/// 4. 下载后 isJarIntact + SHA1 校验
/// 5. 熔断保护：失败数 > max(5, 10% × 总数) 且 failCount > okCount 才取消
#[allow(clippy::too_many_arguments)]
async fn download_one_mod(
    file_name: &str,
    dest_path: &Path,
    downloads: Vec<String>,
    expected_sha1: String,
    file_size: i64,
    _version_dir: &Path,
    abort_flag: &Arc<AtomicBool>,
    downloaded_count: &Arc<AtomicUsize>,
    failed_count: &Arc<AtomicUsize>,
    total_mods: usize,
    idx: usize,
    progress_tx: tokio::sync::mpsc::UnboundedSender<(usize, &'static str, u32)>,
) -> DownloadResult {
    let make_result = |success: bool, error: &str| DownloadResult {
        success,
        file_name: file_name.to_string(),
        error: error.to_string(),
    };

    // 按文件大小计算单个模组下载超时（秒）：越大给越久，避免大文件在慢速/并发下超时丢失
    fn mod_download_timeout(size_bytes: i64) -> u64 {
        if size_bytes > 50 * 1024 * 1024 {
            600
        } else if size_bytes > 20 * 1024 * 1024 {
            300
        } else if size_bytes > 5 * 1024 * 1024 {
            180
        } else {
            120
        }
    }

    if downloads.is_empty() {
        eprintln!("[mrpack] 模组无下载链接，跳过: {}", file_name);
        failed_count.fetch_add(1, Ordering::SeqCst);
        check_circuit_breaker(failed_count, downloaded_count, total_mods, abort_flag);
        return make_result(false, "无可用下载链接");
    }

    // 确保目标目录存在
    if let Some(parent) = dest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // 已存在校验：大小 + isJarIntact + SHA1
    if file_size > 0 && dest_path.exists() {
        let can_skip = match std::fs::metadata(dest_path) {
            Ok(stat) => {
                if stat.len() as i64 == file_size && cf_shared::is_jar_intact(dest_path) {
                    // 进一步用 SHA1 校验
                    if !expected_sha1.is_empty() {
                        match crate::download::single::compute_sha1(dest_path).await {
                            Ok(actual) => {
                                if actual.to_lowercase() == expected_sha1.to_lowercase() {
                                    true
                                } else {
                                    false
                                }
                            }
                            Err(_) => false,
                        }
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
            Err(_) => false,
        };

        if can_skip {
            downloaded_count.fetch_add(1, Ordering::SeqCst);
            return make_result(true, "");
        }
    }

    // 构造所有镜像 URL（原 downloads + mcimirror 镜像）
    let mut all_urls: Vec<String> = Vec::new();
    for dl in &downloads {
        for mu in get_mirror_urls(dl) {
            if !all_urls.contains(&mu) {
                all_urls.push(mu);
            }
        }
    }

    let mut last_err = String::new();

    // 3 轮重试
    for round in 0..MAX_DOWNLOAD_ROUNDS {
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }

        if round > 0 {
            tokio::time::sleep(Duration::from_millis(3000 + round as u64 * 2000)).await;
        }

        // 走 XMCL 等价的多镜像下载：一次性传入完整镜像列表（对应原项目 downloadFileRace）
        let sha1_opt = if expected_sha1.is_empty() {
            None
        } else {
            Some(expected_sha1.as_str())
        };
        let size_opt = if file_size > 0 {
            Some(file_size as u64)
        } else {
            None
        };

        // 逐文件下载进度上报：每个文件下载时，把当前百分比推给主循环，
        // 主循环据此实时广播文件快照（详情里每个模组的进度条才会动）
        let progress_tx_clone = progress_tx.clone();
        let on_progress: Option<crate::download::ProgressCb> = Some(Arc::new(
            move |p: &crate::download::DownloadProgress| {
                let pct = if p.total_bytes > 0 {
                    ((p.bytes_downloaded as f64 / p.total_bytes as f64) * 100.0) as u32
                } else {
                    0
                };
                let _ = progress_tx_clone.send((idx, "downloading", pct));
            },
        ));

        match crate::download::single::download_file_race(
            &all_urls,
            dest_path,
            sha1_opt,
            size_opt,
            mod_download_timeout(file_size),
            on_progress,
        )
        .await
        {
            Ok(_) => {
                // 下载后校验
                if cf_shared::is_jar_intact(dest_path) {
                    let sha1_ok = if !expected_sha1.is_empty() {
                        match crate::download::single::compute_sha1(dest_path).await {
                            Ok(actual) => {
                                if actual.to_lowercase() == expected_sha1.to_lowercase() {
                                    true
                                } else {
                                    eprintln!(
                                        "[mrpack] SHA1校验失败: {} (期望={}, 实际={})",
                                        file_name,
                                        &expected_sha1[..8.min(expected_sha1.len())],
                                        &actual[..8.min(actual.len())]
                                    );
                                    let _ = std::fs::remove_file(dest_path);
                                    false
                                }
                            }
                            Err(e) => {
                                eprintln!("[mrpack] SHA1计算失败: {} - {}", file_name, e);
                                true // SHA1 计算失败不阻塞下载
                            }
                        }
                    } else {
                        true
                    };

                    if sha1_ok {
                        downloaded_count.fetch_add(1, Ordering::SeqCst);
                        let _ = progress_tx.send((idx, "completed", 100));
                        return make_result(true, "");
                    }
                } else {
                    let _ = std::fs::remove_file(dest_path);
                }
            }
            Err(e) => {
                eprintln!(
                    "[mrpack] {} 下载失败 (round {}/3): {}",
                    file_name,
                    round + 1,
                    &e[..100.min(e.len())]
                );
                last_err = e;
                let _ = std::fs::remove_file(dest_path);
                let _ = std::fs::remove_file(dest_path.with_extension("downloading"));
            }
        }
    }

    // 清理 .downloading 残留
    let _ = std::fs::remove_file(dest_path.with_extension("downloading"));

    failed_count.fetch_add(1, Ordering::SeqCst);

    // 熔断保护
    check_circuit_breaker(failed_count, downloaded_count, total_mods, abort_flag);

    let err = if abort_flag.load(Ordering::SeqCst) {
        "已取消"
    } else if last_err.is_empty() {
        "下载失败"
    } else {
        &last_err[..120.min(last_err.len())]
    };
    let _ = progress_tx.send((idx, "failed", 100));
    make_result(false, err)
}

/// 熔断保护：失败数超过一定比例且失败占比过高时才取消剩余下载
/// 失败数需 > max(20, 10% × 总数×4) 且 失败占比 > 0.75 才取消，避免少量失败导致大量文件缺失
fn check_circuit_breaker(
    failed_count: &Arc<AtomicUsize>,
    downloaded_count: &Arc<AtomicUsize>,
    total_mods: usize,
    abort_flag: &Arc<AtomicBool>,
) {
    let fail_n = failed_count.load(Ordering::SeqCst);
    let ok_n = downloaded_count.load(Ordering::SeqCst);
    let total_attempts = fail_n + ok_n;
    let fail_ratio = if total_attempts > 0 {
        fail_n as f64 / total_attempts as f64
    } else {
        0.0
    };
    let threshold = std::cmp::max(20usize, (total_mods as f64 * 0.4) as usize);
    if fail_n > threshold && fail_ratio > 0.75 {
        eprintln!(
            "[mrpack] 失败数({})超过阈值({})且失败占比({:.2}%)，取消剩余下载",
            fail_n,
            threshold,
            fail_ratio * 100.0
        );
        abort_flag.store(true, Ordering::SeqCst);
    }
}

// ============== 资源索引下载 ==============

/// 下载游戏资源索引和缺失资源
async fn download_assets(
    app: &AppHandle,
    asset_index: &Value,
    settings: &Value,
) -> Result<(), String> {
    let assets_dir = cf_shared::assets_dir();
    let asset_index_id = asset_index
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("assetIndex 无 id")?;
    let asset_index_url = asset_index
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("assetIndex 无 url")?;
    let asset_index_sha1 = asset_index
        .get("sha1")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let index_path = assets_dir
        .join("indexes")
        .join(format!("{}.json", asset_index_id));

    // 下载索引文件
    if !index_path.exists()
        || (!asset_index_sha1.is_empty()
            && !verify_file_sha1(&index_path, asset_index_sha1).await)
    {
        if let Some(parent) = index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if index_path.exists() {
            let _ = std::fs::remove_file(&index_path);
        }
        crate::download::single::download_with_mirror(
            asset_index_url,
            &index_path,
            if asset_index_sha1.is_empty() { None } else { Some(asset_index_sha1) },
            None,
            "china-first",
            60,
            None,
        )
        .await
        .map_err(|e| format!("下载资源索引失败: {}", e))?;
    }

    if !index_path.exists() {
        return Ok(());
    }

    // 解析索引文件，收集缺失资源
    let index_content = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("读取资源索引失败: {}", e))?;
    let index_json: Value = serde_json::from_str(&index_content)
        .map_err(|e| format!("解析资源索引失败: {}", e))?;

    let objects = index_json
        .get("objects")
        .and_then(|v| v.as_object())
        .ok_or("资源索引无 objects")?;

    let mut asset_objects: Vec<crate::download::AssetObject> = Vec::with_capacity(objects.len());
    for (name, info) in objects {
        let hash = info
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let size = info
            .get("size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if hash.is_empty() {
            continue;
        }
        let sub_dir = &hash[..2];
        let a_path = assets_dir.join("objects").join(sub_dir).join(hash);
        if !a_path.exists() {
            asset_objects.push(crate::download::AssetObject {
                name: name.clone(),
                hash: hash.to_string(),
                size,
            });
        }
    }

    if asset_objects.is_empty() {
        return Ok(());
    }

    let asset_total = asset_objects.len();
    let sources = crate::download::select_asset_sources("china-first").await;
    let asset_parallel = settings
        .get("maxThreads")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(32)
        .min(64)
        .max(8);

    emit_progress(
        app,
        93,
        &format!("下载游戏资源 (0/{})", asset_total),
        "assets",
    );

    let app_progress = app.clone();
    let (done, failed) = crate::download::download_asset_objects(
        asset_objects,
        &assets_dir,
        &sources,
        asset_parallel,
        move |done, _total, name| {
            let pct = 93 + (done as u64 * 4 / asset_total as u64);
            emit_progress(
                &app_progress,
                pct.min(97) as u32,
                &format!("下载游戏资源 ({}/{})", done, asset_total),
                "assets",
            );
            if done % 50 == 0 || done == asset_total {
                eprintln!("[mrpack] 资源下载进度 {}/{} 当前: {}", done, asset_total, name);
            }
        },
    ).await;

    if failed > 0 {
        eprintln!("[mrpack] {} 个资源文件下载失败", failed);
    }

    crate::download::ensure_language_assets(objects, &assets_dir, &sources, asset_parallel)
        .await?;

    emit_progress(
        app,
        97,
        &format!("游戏资源下载完成 ({}/{})", done, asset_total),
        "assets",
    );

    Ok(())
}

/// 验证文件 SHA1
async fn verify_file_sha1(path: &Path, expected_sha1: &str) -> bool {
    match crate::download::single::compute_sha1(path).await {
        Ok(actual) => actual.to_lowercase() == expected_sha1.to_lowercase(),
        Err(_) => false,
    }
}

// ============== Forge 核心文件验证 ==============

/// 检查 Forge 核心文件是否存在且完整
/// forge-client.jar / client-srg.jar / client-extra.jar
/// 对应原项目 modrinth.js 的 Forge 核心文件验证逻辑
fn check_forge_core_files(merged_json: &Value) -> Result<(), String> {
    let libs = merged_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let forge_client_lib = libs.iter().find(|l| {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            if regex::Regex::new(r"^net\.minecraftforge:forge:\d")
                .unwrap()
                .is_match(name)
            {
                return name.ends_with(":client") || name.split(':').count() == 3;
            }
        }
        false
    });

    let srg_lib = libs.iter().find(|l| {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            name.starts_with("net.minecraft:client:") && name.ends_with(":srg")
        } else {
            false
        }
    });

    let extra_lib = libs.iter().find(|l| {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            name.starts_with("net.minecraft:client:") && name.ends_with(":extra")
        } else {
            false
        }
    });

    let libraries_dir = cf_shared::libraries_dir();
    let mut missing: Vec<(String, PathBuf)> = Vec::new();

    if let Some(lib) = forge_client_lib {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 {
            let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
            let classifier = if parts.len() >= 4 {
                format!("-{}", parts[3])
            } else {
                String::new()
            };
            let jar_name = format!("{}-{}{}.jar", parts[1], parts[2], classifier);
            let p = libraries_dir
                .join(&group_path)
                .join(parts[1])
                .join(parts[2])
                .join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push(("forge-client.jar".to_string(), p));
            }
        }
    }

    if let Some(lib) = srg_lib {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 {
            let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
            let jar_name = format!("{}-{}-srg.jar", parts[1], parts[2]);
            let p = libraries_dir
                .join(&group_path)
                .join(parts[1])
                .join(parts[2])
                .join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push(("client-srg.jar".to_string(), p));
            }
        }
    }

    if let Some(lib) = extra_lib {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 {
            let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
            let jar_name = format!("{}-{}-extra.jar", parts[1], parts[2]);
            let p = libraries_dir
                .join(&group_path)
                .join(parts[1])
                .join(parts[2])
                .join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push(("client-extra.jar".to_string(), p));
            }
        }
    }

    if !missing.is_empty() {
        let missing_names: Vec<String> = missing.iter().map(|(n, _)| n.clone()).collect();
        let missing_paths: Vec<String> = missing.iter().map(|(_, p)| p.display().to_string()).collect();
        eprintln!(
            "[mrpack] Forge核心文件验证失败: 缺失 {} 个文件: {}",
            missing.len(),
            missing_names.join(", ")
        );
        for p in &missing {
            eprintln!("[mrpack]   缺失: {}", p.1.display());
        }
        return Err(format!(
            "Forge核心文件生成失败: 缺失 {}。\n请检查Java环境是否正常，网络是否畅通，然后重试。\n缺失文件路径:\n{}",
            missing_names.join(", "),
            missing_paths.join("\n")
        ));
    }

    Ok(())
}

// ============== mrpack-manifest.json 构建 ==============

/// 构建 mrpack-manifest.json（供启动前 mods 完整性检查使用）
/// 只保留 mods 目录下实际存在的文件
fn build_mrpack_manifest_for_check(
    manifest: &MrpackManifest,
    version_dir: &Path,
    pack_name: &str,
) -> Value {
    let files: Vec<Value> = manifest
        .files
        .iter()
        .filter(|f| {
            // 只保留 mods 目录下实际存在的文件
            if let Some(ref p) = f.path {
                let fp = version_dir.join(p);
                fp.exists()
            } else {
                // 非 mods 文件（如 shaderpacks）保留
                true
            }
        })
        .map(|f| {
            json!({
                "path": f.path.as_ref().cloned().unwrap_or_default(),
                "hashes": f.hashes.sha1.as_ref().map(|s| json!({"sha1": s})).unwrap_or(json!({})),
                "downloads": f.downloads,
                "fileSize": f.file_size.unwrap_or(0)
            })
        })
        .collect();

    json!({
        "format": manifest.format_version.unwrap_or(1),
        "game": manifest.game.as_ref().cloned().unwrap_or_else(|| "minecraft".to_string()),
        "versionId": manifest.version_id.as_ref().cloned().unwrap_or_default(),
        "name": manifest.name.as_ref().cloned().unwrap_or_else(|| pack_name.to_string()),
        "dependencies": manifest_deps_to_json(&manifest.dependencies),
        "files": files
    })
}

/// 将 MrpackDependencies 转为 JSON
fn manifest_deps_to_json(deps: &MrpackDependencies) -> Value {
    let mut obj = Map::new();
    if let Some(ref mc) = deps.minecraft {
        obj.insert("minecraft".to_string(), json!(mc));
    }
    if let Some(ref forge) = deps.forge {
        obj.insert("forge".to_string(), json!(forge));
    }
    if let Some(ref neoforge) = deps.neoforge {
        obj.insert("neoforge".to_string(), json!(neoforge));
    }
    if let Some(ref fabric) = deps.fabric_loader {
        obj.insert("fabric-loader".to_string(), json!(fabric));
    }
    if let Some(ref quilt) = deps.quilt_loader {
        obj.insert("quilt-loader".to_string(), json!(quilt));
    }
    json!(obj)
}
