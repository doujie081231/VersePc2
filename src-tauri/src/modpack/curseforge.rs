// modpack/curseforge.rs — CurseForge 整合包导入
//
// 完整复刻原项目 server/modpack/curseforge.js 的全部 25 个功能点。
// 解析 manifest.json，安装基础版本与模组加载器，批量下载 mods 与 overrides。
//
// 与原项目 1:1 对齐，不做任何简化。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Semaphore;

use super::curseforge_shared as cf_shared;
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
struct CfManifest {
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
struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Deserialize)]
struct CfModLoader {
    id: String,
    #[serde(default)]
    primary: Option<bool>,
}

#[derive(Deserialize)]
struct CfFile {
    // CurseForge manifest.json 使用大写 ID（projectID/fileID），需显式映射
    #[serde(rename = "projectID")]
    project_id: i64,
    #[serde(rename = "fileID")]
    file_id: i64,
    #[serde(default, rename = "required")]
    required: Option<bool>,
}

#[derive(Deserialize)]
struct CfManifestMeta {
    version: Option<String>,
    name: Option<String>,
    author: Option<String>,
}

// CurseForge API 响应
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CfFileResponse {
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
struct CfHash {
    algo: i32, // 1=SHA1, 2=MD5
    value: String,
}

#[derive(Deserialize)]
struct CfBatchResponse {
    data: Vec<CfFileResponse>,
}

#[derive(Debug, Clone, PartialEq)]
enum LoaderKind {
    Forge,
    NeoForge,
    Fabric,
    None,
}

// ============== 主入口 ==============

/// 写入整合包导入日志（modpack-import.log），便于复现定位导入失败
fn import_log(msg: &str) {
    eprintln!("[CurseForge] {}", msg);
    let dir = crate::storage::resolve_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("modpack-import.log");
    let line = format!(
        "[{}] [CurseForge] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        msg
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

/// 导入 CurseForge 整合包
///
/// 完整复刻原项目 _importCurseForge 的 25 个功能点：
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
            if let Ok(entry) = archive.by_index(i) {
                if entry.name().to_lowercase() == "manifest.json" {
                    found = Some(i);
                    break;
                }
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
        .filter(|s| !s.is_empty())
        .map(|s| s.clone())
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
            // 注意：原项目是 versionDir + '.backup_<timestamp>'，
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

/// 主导入流程：overrides 解压 + mods 下载 + 验证 + 资源下载
#[allow(clippy::too_many_arguments)]
async fn run_import_main(
    app: &AppHandle,
    archive: &mut zip::ZipArchive<std::fs::File>,
    manifest: &CfManifest,
    version_id: &str,
    version_dir: &Path,
    pack_name: &str,
    mc_version: &str,
    mod_loader_id: &str,
    loader_kind: &LoaderKind,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    loader_version_id: Option<&str>,
    cf_api_key: &str,
    settings: &Value,
    file_path: &str,
) -> Result<Value, String> {
    // 2.7 overrides 解压
    emit_progress(app, 40, "解压覆盖文件...", "extract");
    let override_files = extract_overrides_with_progress(archive, version_dir, app)?;

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
    let cf_mod_files: Vec<ModFileState> = cf_files
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
                        url = Some(construct_cdn_url(file_id, &fi.file_name));
                        eprintln!(
                            "[CurseForge] downloadUrl 为空，已构造 CDN URL: {}:{} -> {}",
                            project_id,
                            file_id,
                            url.as_ref().unwrap()
                        );
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
        return Err("下载已取消".to_string());
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
    if let Some(ref merged) = cf_merged_json {
        if let Some(asset_index) = merged.get("assetIndex") {
            emit_progress(app, 93, "正在下载游戏资源...", "assets");
            if let Err(e) = download_assets(app, asset_index, settings).await {
                eprintln!("[CurseForge] 资源下载异常(非致命): {}", e);
            }
        }
    }

    // 2.22 客户端 JAR 下载
    if let Some(ref merged) = cf_merged_json {
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
    }

    // 2.23 Forge 核心文件验证
    if let Some(ref merged) = cf_merged_json {
        if !forge_ver.is_empty() || !neoforge_ver.is_empty() {
            if let Err(e) = check_forge_core_files(merged) {
                cf_shared::cleanup_version_chain(version_id);
                return Err(e);
            }
        }
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

// ============== 加载器安装 ==============

/// 安装模组加载器（Forge/NeoForge/Fabric）
/// 已存在则校验 libs 完整性，损坏则删除重装
#[allow(clippy::too_many_arguments)]
async fn install_loader(
    loader_kind: &LoaderKind,
    mc_version: &str,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    app: &AppHandle,
) -> Result<String, String> {
    match loader_kind {
        LoaderKind::Forge => {
            let target_id = format!("{}-forge-{}", mc_version, forge_ver);
            let lv_json_path = versions_dir().join(&target_id).join(format!("{}.json", target_id));
            let need_install = if !lv_json_path.exists() {
                true
            } else if !cf_shared::verify_loader_libs(&target_id) {
                // 损坏则删除重装
                let _ = std::fs::remove_dir_all(versions_dir().join(&target_id));
                true
            } else {
                false
            };
            if need_install {
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
        }
        LoaderKind::NeoForge => {
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
        }
        LoaderKind::Fabric => {
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
        }
        LoaderKind::None => Ok(String::new()),
    }
}

// ============== 版本 JSON 创建 ==============

/// 创建版本 JSON（inheritsFrom + mainClass + 复制主 jar）
fn create_version_json(
    version_id: &str,
    version_dir: &Path,
    mc_version: &str,
    loader_version_id: Option<&str>,
) {
    let version_json = if let Some(lv_id) = loader_version_id {
        // 从加载器版本 JSON 读取 mainClass
        let loader_json_path = versions_dir().join(lv_id).join(format!("{}.json", lv_id));
        let main_class = std::fs::read_to_string(&loader_json_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("mainClass").and_then(|m| m.as_str()).map(|s| s.to_string()))
            .unwrap_or_default();
        let mut j = json!({
            "id": version_id,
            "inheritsFrom": lv_id,
            "type": "release",
            "time": cf_shared::now_iso(),
            "releaseTime": cf_shared::now_iso(),
        });
        if !main_class.is_empty() {
            j["mainClass"] = json!(main_class);
        }
        j
    } else {
        json!({
            "id": version_id,
            "inheritsFrom": mc_version,
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "time": cf_shared::now_iso(),
            "releaseTime": cf_shared::now_iso(),
        })
    };
    let _ = std::fs::write(
        version_dir.join(format!("{}.json", version_id)),
        serde_json::to_string_pretty(&version_json).unwrap_or_default(),
    );

    // 复制主 jar 到版本目录（解决 Forge ignoreList ${version_name}.jar 占位符导致的 JPMS split package 冲突）
    if let Some(lv_id) = loader_version_id {
        let src = versions_dir().join(lv_id).join(format!("{}.jar", lv_id));
        let dst = version_dir.join(format!("{}.jar", version_id));
        if src.exists() && !dst.exists() {
            if let Err(e) = std::fs::copy(&src, &dst) {
                eprintln!("[CurseForge] 复制主 jar 失败 (非致命): {}", e);
            } else {
                eprintln!("[CurseForge] 已复制主 jar 到版本目录: {}.jar", version_id);
            }
        }
    } else {
        // 无加载器场景：复制原版 jar 到新版本目录，原因同上
        let src = versions_dir().join(mc_version).join(format!("{}.jar", mc_version));
        let dst = version_dir.join(format!("{}.jar", version_id));
        if src.exists() && !dst.exists() {
            if let Err(e) = std::fs::copy(&src, &dst) {
                eprintln!("[CurseForge] 复制原版 jar 失败 (非致命): {}", e);
            } else {
                eprintln!("[CurseForge] 已复制原版 jar 到版本目录: {}.jar", version_id);
            }
        }
    }
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
                eprintln!("[Modpack] CurseForge路径遍历已拦截: {}", rel_path);
                continue;
            }
        }

        // 读取 entry 数据
        let mut buf = Vec::new();
        if let Err(e) = entry.read_to_end(&mut buf) {
            eprintln!("[CurseForge] 读取 entry 失败: {} - {}", rel_path, e);
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
                        "[Modpack] CF解压 {} 第 {} 次失败: {}",
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
            // yield to event loop
            std::thread::yield_now();
        }
    }

    Ok(override_files)
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

// ============== modLoader 解析 ==============

/// 解析 modLoader id，返回 (加载器类型, forge版本, fabric版本, neoforge版本)
/// 支持 forge-/neoforge-/fabric-/fabric-loader- 四种前缀正则
fn parse_loader_id(id: &str) -> (LoaderKind, String, String, String) {
    if id.is_empty() {
        return (LoaderKind::None, String::new(), String::new(), String::new());
    }
    let lower = id.to_lowercase();
    let mut forge_ver = String::new();
    let mut fabric_ver = String::new();
    let mut neoforge_ver = String::new();

    // forge-XXX 或 forgeXXX（正则 /^forge[-]?(\d)/）
    if regex::Regex::new(r"^forge[-]?(\d)").unwrap().is_match(&lower) {
        let parts: Vec<&str> = id.split('-').collect();
        if parts[0].to_lowercase() == "forge" {
            forge_ver = parts[1..].join("-");
        } else {
            forge_ver = id.to_lowercase().replacen("forge", "", 1);
            if let Some(rest) = forge_ver.strip_prefix('-') {
                forge_ver = rest.to_string();
            }
        }
    }
    // neoforge-XXX 或 neoforgeXXX（正则 /^neoforge[-]?(\d)/）
    else if regex::Regex::new(r"^neoforge[-]?(\d)").unwrap().is_match(&lower) {
        let parts: Vec<&str> = id.split('-').collect();
        if parts[0].to_lowercase() == "neoforge" {
            neoforge_ver = parts[1..].join("-");
        } else {
            neoforge_ver = id.to_lowercase().replacen("neoforge", "", 1);
            if let Some(rest) = neoforge_ver.strip_prefix('-') {
                neoforge_ver = rest.to_string();
            }
        }
    }
    // fabric-loader-XXX（正则 /^fabric[-]?loader[-]?(\d)/）
    else if regex::Regex::new(r"^fabric[-]?loader[-]?(\d)").unwrap().is_match(&lower) {
        let parts: Vec<&str> = id.split('-').collect();
        if parts.len() >= 3 && parts[0].to_lowercase() == "fabric" && parts[1].to_lowercase() == "loader" {
            fabric_ver = parts[2..].join("-");
        } else if parts[0].to_lowercase() == "fabric" {
            let rest = parts[1..].join("-");
            // 去掉 loader- 前缀
            let re = regex::Regex::new(r"(?i)^loader[-]?").unwrap();
            fabric_ver = re.replace(&rest, "").to_string();
        } else {
            let rest = id.to_string();
            let re = regex::Regex::new(r"(?i)^fabric[-]?loader[-]?").unwrap();
            fabric_ver = re.replace(&rest, "").to_string();
        }
    }
    // fabric-XXX 或 fabricXXX（正则 /^fabric[-]?(\d)/）
    else if regex::Regex::new(r"^fabric[-]?(\d)").unwrap().is_match(&lower) {
        let parts: Vec<&str> = id.split('-').collect();
        if parts[0].to_lowercase() == "fabric" {
            fabric_ver = parts[1..].join("-");
        } else {
            fabric_ver = id.to_lowercase().replacen("fabric", "", 1);
            if let Some(rest) = fabric_ver.strip_prefix('-') {
                fabric_ver = rest.to_string();
            }
        }
    }

    let kind = if !forge_ver.is_empty() {
        LoaderKind::Forge
    } else if !neoforge_ver.is_empty() {
        LoaderKind::NeoForge
    } else if !fabric_ver.is_empty() {
        LoaderKind::Fabric
    } else {
        LoaderKind::None
    };

    (kind, forge_ver, fabric_ver, neoforge_ver)
}

// ============== CDN URL 构造 ==============

/// 构造 edge.forgecdn.net CDN URL
/// CurseForge 部分文件 API 返回 downloadUrl 为 null，但 CDN 实际可访问
/// URL 格式：https://edge.forgecdn.net/files/{fileID前4位}/{fileID剩余位}/{encodeURIComponent(fileName)}
fn construct_cdn_url(file_id: i64, file_name: &str) -> String {
    let id_str = file_id.to_string();
    if id_str.len() < 5 {
        return String::new();
    }
    let (part1, part2) = id_str.split_at(4);
    let encoded = urlencoding::encode(file_name);
    format!(
        "https://edge.forgecdn.net/files/{}/{}/{}",
        part1, part2, encoded
    )
}

// ============== 镜像 URL 列表 ==============

/// 替换为镜像 URL 列表（原 URL + mod.mcimirror.top 镜像）
/// media.forgecdn.net / edge.forgecdn.net → mod.mcimirror.top
fn get_mirror_urls(url: &str) -> Vec<String> {
    let mut urls = vec![url.to_string()];

    if url.contains("mediafilez.forgecdn.net") {
        let mirror = url.replace("mediafilez.forgecdn.net", "mod.mcimirror.top");
        if mirror != url {
            urls.push(mirror);
        }
    } else if url.contains("media.forgecdn.net") {
        let mirror = url.replace("media.forgecdn.net", "mod.mcimirror.top");
        if mirror != url {
            urls.push(mirror);
        }
    } else if url.contains("edge.forgecdn.net") {
        let mirror = url.replace("edge.forgecdn.net", "mod.mcimirror.top");
        if mirror != url {
            urls.push(mirror);
        }
    }

    urls
}

// ============== percent-decode 文件名 ==============

/// 从 URL 提取 basename 并 percent-decode
/// 对应原项目 decodeURIComponent(path.basename(downloadUrl))
fn percent_decode_basename(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let basename = path.rsplit('/').next()?;
    Some(urlencoding::decode(basename).ok()?.to_string())
}

// ============== 下载相关 ==============

#[derive(Debug, Clone, Default)]
struct FileInfo {
    file_name: String,
    download_url: String,
    file_length: i64,
    sha1: String,
}

#[derive(Debug, Clone)]
struct DownloadResult {
    success: bool,
    project_id: i64,
    file_id: i64,
    file_name: String,
    download_url: Option<String>,
    file_length: i64,
    sha1: String,
    mod_id: Option<String>,
    file_info: Option<FileInfo>,
    error: String,
}

#[derive(Debug, Clone)]
enum ModStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

#[allow(dead_code)]
struct ModFileState {
    name: String,
    status: ModStatus,
    progress: u32,
    project_id: i64,
    file_id: i64,
    dest_path: Option<PathBuf>,
    mod_id: Option<String>,
    file_info: Option<FileInfo>,
    error: Option<String>,
}

/// 逐个查询单个文件信息（批量接口失败时的兜底）
///
/// 调用 CurseForge `GET /mods/{projectID}/files/{fileID}`，
/// 返回 `(download_url, file_name, file_length, sha1)`。
async fn fetch_single_file_info(
    project_id: i64,
    file_id: i64,
    cf_api_key: &str,
) -> Option<(String, String, i64, String)> {
    let url = format!("{}/mods/{}/files/{}", CF_API_MIRROR, project_id, file_id);
    eprintln!("[CurseForge] 单个文件信息查询: {}", url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .ok()?;
    let res = client
        .get(&url)
        .header("x-api-key", cf_api_key)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        eprintln!("[CurseForge] 单个文件查询失败 status={}", res.status());
        return None;
    }
    let data: CfFileResponse = res.json().await.ok()?;
    let dl_url = data.download_url.clone().unwrap_or_default();
    let sha1 = data
        .hashes
        .as_ref()
        .and_then(|h| h.iter().find(|x| x.algo == 1))
        .map(|h| h.value.clone())
        .unwrap_or_default();
    Some((dl_url, data.file_name, data.file_length.unwrap_or(0), sha1))
}

/// 下载单个模组文件
///
/// 流程：
/// 1. 确定目标文件名（优先从 URL 解码，其次用 API fileName）
/// 2. 已存在则校验大小+SHA1，通过则跳过
/// 3. 构造镜像 URL 列表（原 URL + mcimirror 镜像）
/// 4. 3 轮重试下载，每轮失败后延后重试
/// 5. 下载后 SHA1 校验
/// 6. 熔断保护：失败数 > max(20, 40% × 总数) 且 failRatio > 0.75 才取消
#[allow(clippy::too_many_arguments)]
async fn download_one_mod(
    project_id: i64,
    file_id: i64,
    download_url: Option<String>,
    file_name: String,
    file_size: i64,
    expected_sha1: String,
    mods_dir: Arc<PathBuf>,
    abort_flag: Arc<AtomicBool>,
    downloaded_count: Arc<AtomicUsize>,
    failed_count: Arc<AtomicUsize>,
    total_mods: usize,
    _cf_api_key: Arc<String>,
    file_info_map: Arc<HashMap<i64, CfFileResponse>>,
) -> DownloadResult {
    // 1. 确定文件名：优先从 URL basename 解码
    let final_name = download_url
        .as_ref()
        .and_then(|u| percent_decode_basename(u))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sanitize_filename(&file_name));
    let dest = mods_dir.join(&final_name);

    let make_result = |success: bool, error: &str| DownloadResult {
        success,
        project_id,
        file_id,
        file_name: final_name.clone(),
        download_url: download_url.clone(),
        file_length: file_size,
        sha1: expected_sha1.clone(),
        mod_id: None,
        file_info: None,
        error: error.to_string(),
    };

    // 2. 已存在校验：大小 + SHA1
    if dest.exists() && cf_shared::is_jar_intact_deep(&dest) {
        let can_skip = if file_size > 0 {
            match std::fs::metadata(&dest) {
                Ok(meta) => {
                    if meta.len() as i64 != file_size {
                        eprintln!(
                            "[CurseForge] 已存在文件大小不匹配，重新下载: {} (期望={}, 实际={})",
                            final_name, file_size, meta.len()
                        );
                        false
                    } else if !expected_sha1.is_empty() {
                        match crate::download::single::compute_sha1(&dest).await {
                            Ok(actual) => {
                                if actual.to_lowercase() == expected_sha1.to_lowercase() {
                                    true
                                } else {
                                    eprintln!(
                                        "[CurseForge] 已存在文件 SHA1 不匹配，重新下载: {}",
                                        final_name
                                    );
                                    false
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[CurseForge] 校验已存在文件失败: {} - {}",
                                    final_name, e
                                );
                                false
                            }
                        }
                    } else {
                        true
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[CurseForge] 校验已存在文件失败: {} - {}",
                        final_name, e
                    );
                    false
                }
            }
        } else {
            true
        };

        if !can_skip {
            let _ = std::fs::remove_file(&dest);
        } else {
            // 跳过下载，直接返回成功
            let mod_id = cf_shared::read_jar_mod_id(&dest);
            let file_info = file_info_map.get(&file_id).map(|fi| FileInfo {
                file_name: final_name.clone(),
                download_url: fi.download_url.clone().unwrap_or_default(),
                file_length: fi.file_length.unwrap_or(0),
                sha1: fi
                    .hashes
                    .as_ref()
                    .and_then(|h| h.iter().find(|x| x.algo == 1))
                    .map(|h| h.value.clone())
                    .unwrap_or_default(),
            });
            downloaded_count.fetch_add(1, Ordering::SeqCst);
            return DownloadResult {
                success: true,
                project_id,
                file_id,
                file_name: final_name,
                download_url: download_url.clone(),
                file_length: file_size,
                sha1: expected_sha1,
                mod_id,
                file_info,
                error: String::new(),
            };
        }
    }

    // 3. URL 校验 + 兜底获取
    // 对齐原项目 curseforge.js 第 407-428 行：
    //   只要拿到了 fileName，即便 downloadUrl 为空，也要按 fileID 分段构造 edge.forgecdn.net CDN URL。
    //   CurseForge 部分文件 API 返回 downloadUrl 为 null，但 CDN 实际可访问。
    let mut resolved_url: Option<String> = download_url.clone();
    let mut resolved_name: String = file_name.clone();
    let mut resolved_size: i64 = file_size;
    let mut resolved_sha1: String = expected_sha1.clone();

    if resolved_url.as_deref().unwrap_or("").is_empty() {
        // 批量查询失败或结果中无 downloadUrl，走单个文件接口兜底
        if let Some((dl_url, name, size, sha1)) =
            fetch_single_file_info(project_id, file_id, _cf_api_key.as_str()).await
        {
            // 用单个接口返回的信息覆盖（更准确）
            if !name.is_empty() {
                resolved_name = sanitize_filename(&name);
            }
            if size > 0 {
                resolved_size = size;
            }
            if !sha1.is_empty() {
                resolved_sha1 = sha1;
            }
            if !dl_url.is_empty() {
                resolved_url = Some(dl_url);
            }
        }
    }

    // 关键兜底：resolved_url 仍为空，但有文件名 → 构造 CDN URL
    if resolved_url.as_deref().unwrap_or("").is_empty() && !resolved_name.is_empty() {
        let cdn = construct_cdn_url(file_id, &resolved_name);
        if !cdn.is_empty() {
            eprintln!(
                "[CurseForge] downloadUrl 为空，已构造 CDN URL: {}:{} -> {}",
                project_id, file_id, cdn
            );
            resolved_url = Some(cdn);
        }
    }

    let url = match resolved_url {
        Some(u) if !u.is_empty() => u,
        _ => {
            let err_msg = "CurseForge 未提供下载链接";
            failed_count.fetch_add(1, Ordering::SeqCst);
            check_circuit_breaker(&failed_count, &downloaded_count, total_mods, &abort_flag);
            // 用最新的 resolved_* 重新构造返回（避免文件名仍是占位符）
            return DownloadResult {
                success: false,
                project_id,
                file_id,
                file_name: resolved_name.clone(),
                download_url: None,
                file_length: resolved_size,
                sha1: resolved_sha1.clone(),
                mod_id: None,
                file_info: None,
                error: err_msg.to_string(),
            };
        }
    };

    // 兜底查询拿到了准确信息 → 重新计算 final_name / dest，再做一次存在性校验
    // （原先是基于占位符名算的，可能会误跳过或放到错误路径）
    let final_name = percent_decode_basename(&url)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sanitize_filename(&resolved_name));
    let dest = mods_dir.join(&final_name);

    let make_result_late = |success: bool, error: &str| DownloadResult {
        success,
        project_id,
        file_id,
        file_name: final_name.clone(),
        download_url: Some(url.clone()),
        file_length: resolved_size,
        sha1: resolved_sha1.clone(),
        mod_id: None,
        file_info: None,
        error: error.to_string(),
    };

    // 基于准确文件名再做一次已存在校验（大小 + SHA1）
    if dest.exists() && cf_shared::is_jar_intact_deep(&dest) {
        let can_skip = if resolved_size > 0 {
            match std::fs::metadata(&dest) {
                Ok(meta) => {
                    if meta.len() as i64 != resolved_size {
                        false
                    } else if !resolved_sha1.is_empty() {
                        match crate::download::single::compute_sha1(&dest).await {
                            Ok(actual) => actual.to_lowercase() == resolved_sha1.to_lowercase(),
                            Err(_) => true,
                        }
                    } else {
                        true
                    }
                }
                Err(_) => false,
            }
        } else {
            true
        };
        if can_skip {
            let mod_id = cf_shared::read_jar_mod_id(&dest);
            let file_info = file_info_map.get(&file_id).map(|fi| FileInfo {
                file_name: final_name.clone(),
                download_url: fi.download_url.clone().unwrap_or_else(|| url.clone()),
                file_length: fi.file_length.unwrap_or(resolved_size),
                sha1: fi
                    .hashes
                    .as_ref()
                    .and_then(|h| h.iter().find(|x| x.algo == 1))
                    .map(|h| h.value.clone())
                    .unwrap_or_else(|| resolved_sha1.clone()),
            });
            downloaded_count.fetch_add(1, Ordering::SeqCst);
            return DownloadResult {
                success: true,
                project_id,
                file_id,
                file_name: final_name,
                download_url: Some(url.clone()),
                file_length: resolved_size,
                sha1: resolved_sha1,
                mod_id,
                file_info,
                error: String::new(),
            };
        } else {
            let _ = std::fs::remove_file(&dest);
        }
    }

    // 将后续闭包/下载逻辑中用到的变量统一替换为 resolved 版本
    // （为了最小化改动，此处用 shadowing 的方式覆盖）
    // resolved_sha1 被 make_result_late 借用，因此 clone 一份给后续校验用
    let expected_sha1 = resolved_sha1.clone();
    let file_size = resolved_size;
    let _make_result_old = make_result; // 屏蔽旧闭包，避免误用
    let make_result = make_result_late;

    // 4. 镜像 URL 列表
    let urls = get_mirror_urls(&url);
    let mut last_err = String::new();

    // 3 轮重试
    for round in 0..MAX_DOWNLOAD_ROUNDS {
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }

        if round > 0 {
            // 延后重试
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

        match crate::download::single::download_file_race(
            &urls,
            &dest,
            sha1_opt,
            size_opt,
            180,
            None,
        )
        .await
        {
            Ok(_) => {
                // 2.14 下载后 SHA1 校验
                if cf_shared::is_jar_intact(&dest) {
                    let sha1_ok = if !expected_sha1.is_empty() {
                        match crate::download::single::compute_sha1(&dest).await {
                            Ok(actual) => {
                                if actual.to_lowercase() == expected_sha1.to_lowercase() {
                                    true
                                } else {
                                    eprintln!(
                                        "[CurseForge] 下载后 SHA1 不匹配，重试: {} (期望={}, 实际={})",
                                        final_name,
                                        &expected_sha1[..8.min(expected_sha1.len())],
                                        &actual[..8.min(actual.len())]
                                    );
                                    let _ = std::fs::remove_file(&dest);
                                    false
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[CurseForge] SHA1 计算失败: {} - {}",
                                    final_name, e
                                );
                                true // SHA1 计算失败不阻塞下载
                            }
                        }
                    } else {
                        true
                    };

                    if sha1_ok {
                        let mod_id = cf_shared::read_jar_mod_id(&dest);
                        let file_info = file_info_map.get(&file_id).map(|fi| FileInfo {
                            file_name: final_name.clone(),
                            download_url: fi.download_url.clone().unwrap_or_default(),
                            file_length: fi.file_length.unwrap_or(0),
                            sha1: fi
                                .hashes
                                .as_ref()
                                .and_then(|h| h.iter().find(|x| x.algo == 1))
                                .map(|h| h.value.clone())
                                .unwrap_or_default(),
                        });
                        downloaded_count.fetch_add(1, Ordering::SeqCst);
                        return DownloadResult {
                            success: true,
                            project_id,
                            file_id,
                            file_name: final_name,
                            download_url: Some(url.clone()),
                            file_length: file_size,
                            sha1: expected_sha1,
                            mod_id,
                            file_info,
                            error: String::new(),
                        };
                    }
                } else {
                    let _ = std::fs::remove_file(&dest);
                }
            }
            Err(e) => {
                eprintln!(
                    "[CurseForge] {} 下载失败 (round {}/3): {}",
                    final_name,
                    round + 1,
                    &e[..100.min(e.len())]
                );
                last_err = e;
                let _ = std::fs::remove_file(&dest);
                let _ = std::fs::remove_file(dest.with_extension("downloading"));
            }
        }
    }

    // 下载失败
    let _ = std::fs::remove_file(dest.with_extension("downloading"));
    failed_count.fetch_add(1, Ordering::SeqCst);

    // 2.15 熔断保护
    check_circuit_breaker(&failed_count, &downloaded_count, total_mods, &abort_flag);

    let err = if abort_flag.load(Ordering::SeqCst) {
        "已取消"
    } else if last_err.is_empty() {
        "下载失败"
    } else {
        &last_err[..120.min(last_err.len())]
    };
    make_result(false, err)
}

/// 熔断保护：仅当失败率超过 40% 且失败数明显大于成功数时才取消
/// 失败数 > max(20, 40% × 总数) 且 failRatio > 0.75 才取消
fn check_circuit_breaker(
    failed_count: &Arc<AtomicUsize>,
    downloaded_count: &Arc<AtomicUsize>,
    total_mods: usize,
    abort_flag: &Arc<AtomicBool>,
) {
    let fail_n = failed_count.load(Ordering::SeqCst);
    let success_n = downloaded_count.load(Ordering::SeqCst);
    let total = fail_n + success_n;
    if total == 0 {
        return;
    }
    let fail_ratio = fail_n as f64 / total as f64;
    let threshold = (total_mods as f64 * 0.4).max(20.0) as usize;
    if fail_n > threshold && fail_ratio > 0.75 {
        eprintln!(
            "[CurseForge] 失败率过高({}/{}/{} = {:.1}%)，取消剩余下载",
            fail_n,
            success_n,
            total,
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

    let index_path = assets_dir.join("indexes").join(format!("{}.json", asset_index_id));

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
            "mojang",
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
    let sources = crate::download::select_asset_sources("mojang").await;
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
        &format!("下载游戏资源 (0/{ })", asset_total),
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
                &format!("下载游戏资源 ({}/{ })", done, asset_total),
                "assets",
            );
            if done % 50 == 0 || done == asset_total {
                eprintln!("[CurseForge] 资源下载进度 {}/{} 当前: {}", done, asset_total, name);
            }
        },
    ).await;

    if failed > 0 {
        eprintln!("[CurseForge] {} 个资源文件下载失败", failed);
    }

    crate::download::ensure_language_assets(objects, &assets_dir, &sources, asset_parallel)
        .await?;

    emit_progress(
        app,
        97,
        &format!("游戏资源下载完成 ({}/{})", asset_total, asset_total),
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
fn check_forge_core_files(merged_json: &Value) -> Result<(), String> {
    let libs = merged_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let forge_client_lib = libs.iter().find(|l| {
        if let Some(name) = l.get("name").and_then(|v| v.as_str()) {
            // 匹配 net.minecraftforge:forge:数字开头
            if regex::Regex::new(r"^net\.minecraftforge:forge:\d")
                .unwrap()
                .is_match(name)
            {
                // 以 :client 结尾 或 只有三段
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
    let mut missing: Vec<String> = Vec::new();

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
            let p = libraries_dir.join(&group_path).join(parts[1]).join(parts[2]).join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push("forge-client.jar".to_string());
            }
        }
    }

    if let Some(lib) = srg_lib {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 {
            let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
            let jar_name = format!("{}-{}-srg.jar", parts[1], parts[2]);
            let p = libraries_dir.join(&group_path).join(parts[1]).join(parts[2]).join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push("client-srg.jar".to_string());
            }
        }
    }

    if let Some(lib) = extra_lib {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 3 {
            let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
            let jar_name = format!("{}-{}-extra.jar", parts[1], parts[2]);
            let p = libraries_dir.join(&group_path).join(parts[1]).join(parts[2]).join(&jar_name);
            if !p.exists() || !cf_shared::is_jar_intact(&p) {
                missing.push("client-extra.jar".to_string());
            }
        }
    }

    if !missing.is_empty() {
        let missing_names = missing.join(", ");
        eprintln!(
            "[CurseForge] Forge核心文件验证失败: {}个缺失: {}",
            missing.len(),
            missing_names
        );
        return Err(format!(
            "Forge核心文件生成失败: 缺失 {}。请检查Java环境和网络后重试。",
            missing_names
        ));
    }

    Ok(())
}

// ============== 文件名工具 ==============

/// 过滤文件名中的非法字符
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect()
}
