#![allow(unused_imports)]

// modpack/curseforge.rs — CurseForge 整合包导入
//
// 解析 manifest.json，安装基础版本与模组加载器，批量下载 mods 与 overrides。
//

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

// ============== 常量 ==============

// CurseForge API 镜像（仅代理 CurseForge API，不走熔断）
const CF_API_OFFICIAL: &str = "https://api.curseforge.com/v1";
const CF_API_MIRROR: &str = "https://mod.mcimirror.top/curseforge/v1";

// CurseForge 默认 API Key（与 api/modpacks.rs 一致）
const DEFAULT_CF_API_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

// 批量获取文件信息的批次大小
const CF_BATCH_SIZE: usize = 50;

// 单文件下载最大重试轮数
const MAX_DOWNLOAD_ROUNDS: u32 = 3;

// ============== 数据结构 ==============

// CurseForge manifest.json schema
#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(default)]
    manifest: Option<CfManifestMeta>,
    #[serde(default)]
    files: Vec<CfFile>,
    #[serde(default)]
    overrides: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct CfModLoader {
    id: String,
    #[serde(default)]
    primary: Option<bool>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct CfFile {
    // CurseForge manifest.json 使用大写 ID（projectID/fileID），需显式映射
    #[serde(rename = "projectID")]
    project_id: i64,
    #[serde(rename = "fileID")]
    file_id: i64,
    #[serde(default, rename = "required")]
    required: Option<bool>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct CfManifestMeta {
    version: Option<String>,
    name: Option<String>,
    author: Option<String>,
}

// CurseForge API 响应
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) struct CfFileResponse {
    id: i64,
    #[serde(default)]
    mod_id: Option<i64>,
    download_url: Option<String>,
    file_name: String,
    #[serde(default)]
    file_length: Option<i64>,
    #[serde(default)]
    hashes: Option<Vec<CfHash>>,
}

#[derive(Deserialize, Clone)]
pub(crate) struct CfHash {
    algo: i32, // 1=SHA1, 2=MD5
    value: String,
}

#[derive(Deserialize)]
pub(crate) struct CfBatchResponse {
    data: Vec<CfFileResponse>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoaderKind {
    Forge,
    NeoForge,
    Fabric,
    None,
}

// ============== 主入口 ==============

/// 写入整合包导入日志（modpack-import.log），便于复现定位导入失败
fn import_log(msg: &str) {
    super::import_log("CurseForge", msg);
}

/// 导入 CurseForge 整合包
///
/// 25 个功能点：
/// 1. manifest.json 解析（mcVersion 校验 + modLoaders 解析 + 元数据保留）
/// 2. 版本目录去重 _dedupeVersionId
/// 3. 基础版本安装 ensureBaseVersionInstalled
/// 4. Forge/NeoForge/Fabric 安装（已存在则校验 libs 完整性，损坏则删除重装）
/// 5. 版本 JSON 创建（inheritsFrom + mainClass + 复制主 jar）
/// 6. mods 目录备份（已存在版本目录时备份到 *.backup_<timestamp>）
/// 7. overrides 解压（路径遍历保护 + 5 次重试 + 50 文件 yield + 实时进度）
/// 8. 根目录图标提取（pack.png / icon.png / logo.png）
/// 9. 资源包重定位 relocateMisplacedResourcePacks
/// 10. 版本隔离强制开启（version-settings.json: { isolation: 'on' }）
/// 11. CurseForge API 批量获取文件信息（POST /v1/mods/files，batch 50，3 次重试，走镜像不走熔断）
/// 12. 已存在文件校验（大小 + SHA1）
/// 13. 下载引擎 downloadFileRace（64 线程并发 + 镜像回退 + SHA1 校验）
/// 14. 下载后 SHA1 校验
/// 15. 熔断保护（失败数 > max(20, 40% × 总数) 且 failRatio > 0.75 才取消）
/// 16. 失败模组列表持久化（failed-mods.json）
/// 17. 模组清单保存 _saveModManifest
/// 18. 损坏 JAR 修复 _repairCorruptedModJars
/// 19. 加载器兼容性检查 ensureLoaderCompat
/// 20. 库文件验证 verifyImportLibs
/// 21. 资源索引下载（解析 assetIndex + 缺资源并发下载 64 线程）
/// 22. 客户端 JAR 下载（3 次重试 + 失败非致命）
/// 23. Forge 核心文件验证（forge-client.jar / client-srg.jar / client-extra.jar）
/// 24. pack-info.json 写入
/// 25. 失败回滚（恢复备份的 mods 目录 + cleanupVersionChain）
pub async fn import_curseforge(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
) -> Value {
    eprintln!("[CurseForge] 开始导入: {}", file_path);
    import_log(&format!("开始导入: {}", file_path));

    // 1. 打开 zip 找 manifest.json
    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    // 找到 manifest.json
    let manifest_idx = {
        let mut found: Option<usize> = None;
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i)
                && entry.name().to_lowercase() == "manifest.json" {
                    found = Some(i);
                    break;
                }
        }
        found
    };
    let manifest_idx = match manifest_idx {
        Some(i) => i,
        None => return json!({ "success": false, "error": "CurseForge 整合包缺少 manifest.json" }),
    };

    // 2.1 manifest.json 解析
    let manifest_str = {
        let mut entry = match archive.by_index(manifest_idx) {
            Ok(e) => e,
            Err(e) => return json!({ "success": false, "error": format!("读取 manifest.json 失败: {}", e) }),
        };
        let mut buf = String::new();
        if let Err(e) = entry.read_to_string(&mut buf) {
            return json!({ "success": false, "error": format!("解析 manifest.json 失败: {}", e) });
        }
        buf
    };

    let manifest: CfManifest = match serde_json::from_str(&manifest_str) {
        Ok(m) => m,
        Err(e) => return json!({ "success": false, "error": format!("解析 manifest.json 失败: {}", e) }),
    };

    // 2.1 mcVersion 缺失时显式报错
    let mc_version = manifest.minecraft.version.clone();
    if mc_version.is_empty() {
        return json!({ "success": false, "error": "CurseForge 整合包未提供 Minecraft 版本信息" });
    }

    // 2.1 modLoaders 数组解析，支持 forge-/neoforge-/fabric-/fabric-loader- 四种前缀正则
    let mod_loader_id = manifest
        .minecraft
        .mod_loaders
        .first()
        .map(|l| l.id.clone())
        .unwrap_or_default();
    let (loader_kind, forge_ver, fabric_ver, neoforge_ver) = parse_loader_id(&mod_loader_id);
    import_log(&format!(
        "manifest 解析: mc={}, loader_id={:?}, kind={:?}, forge={:?}, fabric={:?}, neoforge={:?}",
        mc_version, mod_loader_id, loader_kind, forge_ver, fabric_ver, neoforge_ver
    ));

    // 2.1 manifest.version / name / author 元数据保留
    let pack_name_raw = manifest
        .manifest
        .as_ref()
        .and_then(|m| m.name.as_ref())
        .filter(|s| !s.is_empty()).cloned()
        .unwrap_or_else(|| {
            Path::new(file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Modpack")
                .to_string()
        });
    // 过滤非法字符
    let pack_name: String = pack_name_raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();

    emit_progress(
        app,
        8,
        &format!("整合包: {}  MC: {}", pack_name, mc_version),
        "prepare",
    );

    // 2.2 版本目录去重 _dedupeVersionId
    let base_version_id = if !custom_version_name.is_empty() {
        normalize_version_id(custom_version_name)
    } else {
        normalize_version_id(&pack_name)
    };
    let version_id = cf_shared::dedupe_version_id(&base_version_id);
    let version_dir = versions_dir().join(&version_id);

    // 创建版本目录
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    let is_new_version_dir = !version_dir.join(format!("{}.json", version_id)).exists();

    // CurseForge API Key：优先使用用户配置的 Key，无配置时使用内置公共 Key 兜底
    let settings = storage::load_settings();
    let cf_api_key = settings
        .get("curseforgeApiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CF_API_KEY)
        .to_string();

    let mut loader_version_id: Option<String> = None;

    // 2.3-2.5 基础版本安装 + 加载器安装 + 版本 JSON 创建
    if is_new_version_dir {
        // 2.3 基础版本安装
        emit_progress(app, 5, "正在准备基础版本...", "base");
        let app_owned = app.clone();
        let on_progress = Some(Box::new(move |pct: u32, msg: String| {
            // 基础版本安装的 0-100 映射到整合包整体的 5-20 区间
            let mapped = 5 + (pct as u64 * 15 / 100).min(15) as u32;
            emit_progress(&app_owned, mapped, &msg, "base")
        }) as ml_shared::BaseVersionProgress);
        if let Err(e) = ml_shared::ensure_base_version_installed(&mc_version, on_progress).await {
            import_log(&format!(
                "基础版本安装失败: mc={}, error={}",
                mc_version, e
            ));
            let _ = std::fs::remove_dir_all(&version_dir);
            return json!({ "success": false, "versionId": version_id, "error": e });
        }
        import_log(&format!("基础版本安装成功: mc={}", mc_version));

        // 2.4 Forge/NeoForge/Fabric 安装
        if loader_kind != LoaderKind::None {
            emit_progress(app, 20, "正在安装模组加载器...", "loader-install");
            match install_loader(
                &loader_kind,
                &mc_version,
                &forge_ver,
                &fabric_ver,
                &neoforge_ver,
                app,
            )
            .await
            {
                Ok(lv_id) => {
                    loader_version_id = Some(lv_id.clone());
                    import_log(&format!(
                        "加载器安装成功: kind={:?}, loader_version_id={}",
                        loader_kind, lv_id
                    ));
                }
                Err(e) => {
                    import_log(&format!(
                        "加载器安装失败: kind={:?}, error={}",
                        loader_kind, e
                    ));
                    eprintln!("[CurseForge] 模组加载器安装失败: {}", e);
                    let _ = std::fs::remove_dir_all(&version_dir);
                    return json!({ "success": false, "versionId": version_id, "error": e });
                }
            }
        }

        // 2.5 版本 JSON 创建
        emit_progress(app, 35, "正在创建版本配置...", "version-config");
        import_log(&format!(
            "创建版本JSON: version_id={}, inheritsFrom={:?}",
            version_id,
            loader_version_id.as_deref().unwrap_or(&mc_version)
        ));
        create_version_json(
            &version_id,
            &version_dir,
            &mc_version,
            loader_version_id.as_deref(),
        );

        emit_progress(app, 40, "模组加载器就绪", "loader");
    }

    // 2.6 mods 目录备份（已存在版本目录时备份到 *.backup_<timestamp>）
    let backup_dir: Option<PathBuf> = if !is_new_version_dir {
        let existing_mods_dir = version_dir.join("mods");
        if existing_mods_dir.exists() {
            let bk = version_dir.with_extension(format!(
                "backup_{}",
                cf_shared::now_timestamp()
            ));
            // 注意：原实现是 versionDir + '.backup_<timestamp>'，
            // 但 with_extension 会把 version_id 的最后一段当作扩展名替换
            // 这里需要用完全不同的方式：在 version_dir 同级创建 .backup_<ts> 目录
            let bk_dir = version_dir
                .parent()
                .map(|p| {
                    p.join(format!(
                        "{}.backup_{}",
                        version_id,
                        cf_shared::now_timestamp()
                    ))
                })
                .unwrap_or(bk);
            if let Err(e) = std::fs::create_dir_all(bk_dir.join("mods")) {
                eprintln!("[CurseForge] 备份 mods 目录失败 (非致命): {}", e);
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
        &mod_loader_id,
        &loader_kind,
        &forge_ver,
        &fabric_ver,
        &neoforge_ver,
        loader_version_id.as_deref(),
        &cf_api_key,
        &settings,
        file_path,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            // 2.25 失败回滚：恢复备份的 mods 目录 + cleanupVersionChain
            eprintln!("[CurseForge] 导入失败: {}", e);
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


// ============== 加载器安装 ==============


// ============== 版本 JSON 创建 ==============


// ============== overrides 解压 ==============


// ============== 根目录图标提取 ==============


// ============== modLoader 解析 ==============


// ============== CDN URL 构造 ==============


// ============== 镜像 URL 列表 ==============


// ============== percent-decode 文件名 ==============


// ============== 下载相关 ==============








// ============== 资源索引下载 ==============



// ============== Forge 核心文件验证 ==============


// ============== 文件名工具 ==============



// 子模块声明与公共导出
mod import;
pub(crate) use import::*;
mod loader;
pub(crate) use loader::*;
mod download;
pub(crate) use download::*;
mod extract;
pub(crate) use extract::*;
