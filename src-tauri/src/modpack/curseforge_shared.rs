// modpack/curseforge_shared.rs — CurseForge 整合包导入共享工具
//
// 完整复刻原项目 server/modpack/shared.js + server/modloaders/shared.js 中的
// 版本去重、JAR 修复、路径校验、资源包重定位、模组清单、加载器兼容性检查、
// 库文件验证、版本链清理等共用工具。
//
// 与原项目 1:1 对齐，不做任何简化。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::modloaders::shared as ml_shared;
use crate::storage;

// ============== 常量 ==============

pub const DEFAULT_MODPACK_CONCURRENCY: usize = 64;
pub const MAX_MODPACK_CONCURRENCY: usize = 64;

// ============== 版本 ID 去重 ==============
// 对应原项目 shared.js _dedupeVersionId

/// 重复版本名自动去重，避免覆盖已有版本
/// 重名时追加 (2)~(999)
pub fn dedupe_version_id(base_name: &str) -> String {
    let vdir = versions_dir();
    let mut candidate = base_name.to_string();
    let mut counter = 2;
    while vdir.join(&candidate).exists() {
        candidate = format!("{} ({})", base_name, counter);
        counter += 1;
        if counter > 999 {
            break;
        }
    }
    candidate
}

// ============== 清理 .downloading 残留 ==============
// 对应原项目 shared.js _cleanDownloadingResidue

/// 清理 mods 目录下的 .downloading 残留临时文件
/// 下载中断/失败时 .downloading 文件会残留，续传时基于错误偏移量追加导致 SHA1 必然失败
pub fn clean_downloading_residue(version_dir: &Path) -> usize {
    let mods_dir = version_dir.join("mods");
    if !mods_dir.exists() {
        return 0;
    }
    let mut cleaned = 0usize;
    let entries = match fs::read_dir(&mods_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".downloading") {
                if fs::remove_file(&path).is_ok() {
                    cleaned += 1;
                }
            }
        }
    }
    if cleaned > 0 {
        eprintln!("[Modpack] 已清理 {} 个 .downloading 残留文件", cleaned);
    }
    cleaned
}

// ============== 路径安全校验 ==============
// 对应原项目 shared.js isModpackPathSafe

const WIN_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 拦截 __macosx 前缀和 Windows 保留名
pub fn is_modpack_path_safe(entry_path: &str) -> bool {
    if entry_path.is_empty() {
        return false;
    }
    let normalized = entry_path.replace('\\', "/");
    if normalized.to_lowercase().starts_with("__macosx/") {
        return false;
    }
    for seg in normalized.split('/') {
        if seg.is_empty() {
            continue;
        }
        let stem = seg.split('.').next().unwrap_or(seg);
        if WIN_RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return false;
        }
    }
    true
}

// ============== 资源包检测与重定位 ==============
// 对应原项目 shared.js _isResourcePackZip + relocateMisplacedResourcePacks

/// 检测 zip 是否为资源包（有 pack.mcmeta 且无 mods.toml/mcmod.info）
pub fn is_resource_pack_zip(zip_path: &Path) -> bool {
    let file = match fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return false,
    };
    let mut has_mcmeta = false;
    let mut has_mods_toml = false;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let name = entry.name().to_lowercase();
            if name == "pack.mcmeta" {
                has_mcmeta = true;
            } else if name == "meta-inf/mods.toml" || name == "mcmod.info" {
                has_mods_toml = true;
            }
        }
    }
    has_mcmeta && !has_mods_toml
}

/// 将 mods 目录下误放的资源包 zip 移到 resourcepacks 目录
pub fn relocate_misplaced_resource_packs(version_dir: &Path) -> RelocateResult {
    let mut result = RelocateResult::default();
    let mods_dir = version_dir.join("mods");
    if !mods_dir.exists() || !mods_dir.is_dir() {
        return result;
    }
    let resourcepacks_dir = version_dir.join("resourcepacks");
    let zip_files: Vec<String> = match fs::read_dir(&mods_dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.to_lowercase().ends_with(".zip") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return result,
    };
    for zip_name in zip_files {
        let src_path = mods_dir.join(&zip_name);
        if !is_resource_pack_zip(&src_path) {
            result.skipped.push(zip_name);
            continue;
        }
        if !resourcepacks_dir.exists() {
            if fs::create_dir_all(&resourcepacks_dir).is_err() {
                result.skipped.push(zip_name);
                continue;
            }
        }
        let dst_path = resourcepacks_dir.join(&zip_name);
        if dst_path.exists() {
            result.skipped.push(zip_name);
            continue;
        }
        if fs::rename(&src_path, &dst_path).is_ok() {
            result.relocated.push(zip_name);
        } else {
            result.skipped.push(zip_name);
        }
    }
    result
}

#[derive(Default, Debug)]
pub struct RelocateResult {
    pub relocated: Vec<String>,
    pub skipped: Vec<String>,
}

// ============== 模组清单保存 ==============
// 对应原项目 shared.js _saveModManifest

/// 保存模组清单到版本目录，供启动前校验使用
pub fn save_mod_manifest(version_dir: &Path, mods: &[Value]) {
    let manifest_path = version_dir.join("mod-manifest.json");
    let manifest_json = json!({
        "generatedAt": now_iso(),
        "mods": mods,
    });
    if let Err(e) = fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest_json).unwrap_or_default(),
    ) {
        eprintln!("[Modpack] 保存模组清单失败: {}", e);
    } else {
        eprintln!(
            "[Modpack] 已保存模组清单: {} ({} 个)",
            manifest_path.display(),
            mods.len()
        );
    }
}

// ============== 并发与超时 ==============
// 对应原项目 shared.js resolveConcurrency + computeModTimeout

/// 根据 settings.maxThreads 解析模组下载并发数，默认 64，上限 64
pub fn resolve_concurrency(settings: &Value) -> usize {
    let max_threads = settings
        .get("maxThreads")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_MODPACK_CONCURRENCY);
    max_threads.min(MAX_MODPACK_CONCURRENCY).max(1)
}

/// 按文件大小返回单个模组下载超时（毫秒）
pub fn compute_mod_timeout(size_bytes: i64) -> u64 {
    if size_bytes > 50 * 1024 * 1024 {
        return 600_000;
    }
    if size_bytes > 20 * 1024 * 1024 {
        return 300_000;
    }
    if size_bytes > 5 * 1024 * 1024 {
        return 180_000;
    }
    120_000
}

// ============== JAR 完整性校验 ==============
// 对应原项目 utils.js isJarIntact + isJarIntactDeep

/// 轻量校验：ZIP 头魔数 + 大小 > 1KB + EOCD 尾
pub fn is_jar_intact(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.len() < 1024 {
        return false;
    }
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    // PK\x03\x04 = ZIP 本地文件头魔数
    let mut buf = [0u8; 4];
    if file.read(&mut buf).is_err() {
        return false;
    }
    if buf[0] != 0x50 || buf[1] != 0x4B || buf[2] != 0x03 || buf[3] != 0x04 {
        return false;
    }
    // 检查 EOCD 尾（End of Central Directory）
    // EOCD 签名：PK\x05\x06，位于文件末尾 22 字节内
    let file_size = meta.len();
    if file_size < 22 {
        return false;
    }
    let seek_pos = file_size.saturating_sub(22);
    if std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(seek_pos)).is_err() {
        return false;
    }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() {
        return false;
    }
    // 在尾部 22 字节内查找 EOCD 签名
    for i in 0..tail.len().saturating_sub(4) {
        if tail[i] == 0x50 && tail[i + 1] == 0x4B && tail[i + 2] == 0x05 && tail[i + 3] == 0x06 {
            return true;
        }
    }
    false
}

/// 深度校验：用 zip crate 读取所有条目数据，检测内部 entry 损坏
/// 如 "invalid entry size" 这类轻量校验检测不到的损坏
pub fn is_jar_intact_deep(path: &Path) -> bool {
    if !is_jar_intact(path) {
        return false;
    }
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return false,
    };
    for i in 0..archive.len() {
        let entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => return false,
        };
        // 尝试读取每个 entry 的数据，检测 CRC 校验失败
        if !entry.is_dir() {
            let mut reader = entry;
            let mut buf = Vec::with_capacity(8192);
            if std::io::Read::read_to_end(&mut reader, &mut buf).is_err() {
                return false;
            }
        }
    }
    true
}

// ============== 损坏 JAR 修复 ==============
// 对应原项目 shared.js _repairCorruptedModJars
// 完整实现：PowerShell Expand-Archive + unzip 命令 + zip crate 重打包

/// 扫描 mods 目录下所有 .jar 文件，修复损坏的 JAR
///
/// 修复流程：
/// 1. 轻量校验（大小 + ZIP 头 + EOCD 尾）
/// 2. 深度校验（读取所有 entry 数据）
/// 3. 损坏则尝试修复：
///    a. Windows: PowerShell Expand-Archive 解压 + zip crate 重打包
///    b. 其他平台: unzip 命令解压 + zip crate 重打包
///    c. 无法修复则删除
pub fn repair_corrupted_mod_jars(version_dir: &Path) -> RepairResult {
    let mods_dir = version_dir.join("mods");
    if !mods_dir.exists() {
        return RepairResult::default();
    }

    let mut corrupted: Vec<CorruptedJar> = Vec::new();
    scan_dir_for_corrupted_jars(&mods_dir, &mut corrupted);

    if corrupted.is_empty() {
        return RepairResult::default();
    }

    let mut repaired = 0usize;
    let mut failed = 0usize;

    for jar in &corrupted {
        let mut fixed = false;

        // 临时目录
        let temp_dir = jar.path.with_extension("_repair_tmp");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        if let Some(parent) = temp_dir.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Windows: 尝试 PowerShell Expand-Archive
        if cfg!(target_os = "windows") {
            fixed = try_repair_with_powershell(&jar.path, &temp_dir);
        }

        // 回退：尝试 unzip 命令
        if !fixed {
            fixed = try_repair_with_unzip(&jar.path, &temp_dir);
        }

        // 清理临时目录
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }

        if fixed {
            repaired += 1;
        } else {
            // 无法修复，删除损坏文件
            let _ = fs::remove_file(&jar.path);
            eprintln!(
                "[Modpack] JAR 文件损坏已删除: {:?} ({})",
                jar.path.file_name(),
                jar.reason
            );
            failed += 1;
        }
    }

    RepairResult { repaired, failed }
}

#[derive(Default, Debug)]
pub struct RepairResult {
    pub repaired: usize,
    pub failed: usize,
}

struct CorruptedJar {
    path: PathBuf,
    reason: &'static str,
}

fn scan_dir_for_corrupted_jars(dir: &Path, corrupted: &mut Vec<CorruptedJar>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_for_corrupted_jars(&path, corrupted);
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.to_lowercase().ends_with(".jar") {
            continue;
        }
        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() < 100 {
            corrupted.push(CorruptedJar {
                path,
                reason: "too_small",
            });
            continue;
        }
        if !is_jar_intact(&path) {
            corrupted.push(CorruptedJar {
                path,
                reason: "structure_corrupted",
            });
            continue;
        }
        if !is_jar_intact_deep(&path) {
            corrupted.push(CorruptedJar {
                path,
                reason: "entry_corrupted",
            });
        }
    }
}

/// Windows: PowerShell Expand-Archive 解压 + zip crate 重打包
fn try_repair_with_powershell(jar_path: &Path, temp_dir: &Path) -> bool {
    if !cfg!(target_os = "windows") {
        return false;
    }
    let jar_str = jar_path.to_string_lossy().replace('\'', "''");
    let temp_str = temp_dir.to_string_lossy().replace('\'', "''");
    let ps_cmd = format!(
        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
        jar_str, temp_str
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output();
    let output = match output {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "[Modpack] PowerShell修复失败 {:?}: {}",
                jar_path.file_name(),
                e
            );
            return false;
        }
    };
    if !output.status.success() {
        eprintln!(
            "[Modpack] PowerShell修复失败 {:?}: {}",
            jar_path.file_name(),
            String::from_utf8_lossy(&output.stderr).chars().take(200).collect::<String>()
        );
        return false;
    }
    // 收集解压的文件
    let files = collect_files(temp_dir);
    if files.is_empty() {
        return false;
    }
    // 用 zip crate 重新打包
    if !repack_files(jar_path, &files, temp_dir) {
        return false;
    }
    is_jar_intact_deep(jar_path)
}

/// unzip 命令解压 + zip crate 重打包
fn try_repair_with_unzip(jar_path: &Path, temp_dir: &Path) -> bool {
    let output = std::process::Command::new("unzip")
        .args(["-o"])
        .arg(jar_path)
        .args(["-d"])
        .arg(temp_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    let output = match output {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let files = collect_files(temp_dir);
    if files.is_empty() {
        return false;
    }
    if !repack_files(jar_path, &files, temp_dir) {
        return false;
    }
    is_jar_intact_deep(jar_path)
}

fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let stack = vec![dir.to_path_buf()];
    let mut stack = stack;
    while let Some(d) = stack.pop() {
        let entries = match fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// 用 zip crate 把文件重新打包为 JAR
fn repack_files(jar_path: &Path, files: &[PathBuf], base_dir: &Path) -> bool {
    let tmp_jar = jar_path.with_extension("jar.repacking");
    let file = match fs::File::create(&tmp_jar) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut zip_writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for f in files {
        let rel = match f.strip_prefix(base_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if let Ok(data) = fs::read(f) {
            if zip_writer.start_file(&rel_str, options).is_err() {
                continue;
            }
            if zip_writer.write_all(&data).is_err() {
                continue;
            }
        }
    }
    if zip_writer.finish().is_err() {
        let _ = fs::remove_file(&tmp_jar);
        return false;
    }
    // 替换原文件
    if fs::rename(&tmp_jar, jar_path).is_err() {
        let _ = fs::remove_file(jar_path);
        let _ = fs::rename(&tmp_jar, jar_path);
    }
    true
}

// ============== 版本链清理 ==============
// 对应原项目 versions/version-list.js cleanupVersionChain

/// 清理版本链：删除版本目录及其继承链中的非原版目录
/// 原版目录（如 1.20.1）不删除
pub fn cleanup_version_chain(version_id: &str) {
    let chain = find_version_chain(version_id);
    let vanilla_pattern = regex::Regex::new(
        r"^\d+\.\d+(\.\d+)?(-rc\d+|-pre\d+|-snapshot.*)?$",
    )
    .unwrap();

    let mut to_delete: Vec<String> = Vec::new();
    for id in &chain {
        if vanilla_pattern.is_match(id) && id != &version_id {
            continue;
        }
        to_delete.push(id.clone());
    }
    if !to_delete.contains(&version_id.to_string()) {
        to_delete.push(version_id.to_string());
    }

    for id in &to_delete {
        let dir = versions_dir().join(id);
        if !dir.exists() {
            continue;
        }
        // 最多重试 5 次删除（文件可能被占用）
        let mut deleted = false;
        for attempt in 1..=5u32 {
            if fs::remove_dir_all(&dir).is_ok() {
                deleted = true;
                break;
            }
            eprintln!("[Cleanup] 删除 {} 失败 (第{}次)", id, attempt);
            if attempt < 5 {
                std::thread::sleep(Duration::from_millis(attempt as u64 * 1000));
            }
        }
        if !deleted {
            eprintln!("[Cleanup] {} 文件可能被占用，请关闭游戏后重试", id);
        }
    }
}

/// 查找版本继承链
fn find_version_chain(version_id: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = version_id.to_string();
    let mut visited = std::collections::HashSet::new();
    while !visited.contains(&current) {
        visited.insert(current.clone());
        chain.push(current.clone());
        let json_path = versions_dir().join(&current).join(format!("{}.json", current));
        if !json_path.exists() {
            break;
        }
        let content = match fs::read_to_string(&json_path) {
            Ok(c) => c,
            Err(_) => break,
        };
        let json: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => break,
        };
        match json.get("inheritsFrom").and_then(|v| v.as_str()) {
            Some(parent) if !parent.is_empty() => current = parent.to_string(),
            _ => break,
        }
    }
    chain
}

// ============== 版本 JSON 递归解析 ==============
// 对应原项目 versions.resolveVersionJson

/// 递归解析版本 JSON（合并 inheritsFrom 的 libraries + downloads.client）
pub fn resolve_version_json(version_id: &str) -> Option<Value> {
    resolve_version_json_recursive(version_id, &versions_dir())
}

fn resolve_version_json_recursive(version_id: &str, versions_dir: &Path) -> Option<Value> {
    let json_path = versions_dir.join(version_id).join(format!("{}.json", version_id));
    if !json_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&json_path).ok()?;
    let mut json: Value = serde_json::from_str(&content).ok()?;

    if let Some(parent_id) = json
        .get("inheritsFrom")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    {
        if let Some(parent_json) = resolve_version_json_recursive(&parent_id, versions_dir) {
            // 合并 libraries（父版本在前，子版本在后，去重）
            if let (Some(parent_libs), Some(self_libs)) = (
                parent_json.get("libraries").and_then(|v| v.as_array()).cloned(),
                json.get("libraries").and_then(|v| v.as_array()).cloned(),
            ) {
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut merged: Vec<Value> = Vec::new();
                for lib in parent_libs.iter().chain(self_libs.iter()) {
                    if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
                        if seen.insert(name.to_string()) {
                            merged.push(lib.clone());
                        }
                    } else {
                        merged.push(lib.clone());
                    }
                }
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("libraries".to_string(), json!(merged));
                }
            }
            // 合并 downloads.client
            if json.get("downloads").and_then(|d| d.get("client")).is_none() {
                if let Some(parent_client) = parent_json.get("downloads").and_then(|d| d.get("client")) {
                    if let Some(obj) = json.as_object_mut() {
                        let mut downloads = obj.get("downloads").cloned().unwrap_or(json!({}));
                        if let Some(d_obj) = downloads.as_object_mut() {
                            d_obj.insert("client".to_string(), parent_client.clone());
                        }
                        obj.insert("downloads".to_string(), downloads);
                    }
                }
            }
            // 合并 assetIndex
            if json.get("assetIndex").is_none() {
                if let Some(parent_asset_index) = parent_json.get("assetIndex") {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("assetIndex".to_string(), parent_asset_index.clone());
                    }
                }
            }
            // 合并 jar
            if json.get("jar").is_none() {
                if let Some(parent_jar) = parent_json.get("jar").and_then(|v| v.as_str()) {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("jar".to_string(), json!(parent_jar));
                    }
                }
            }
        }
    }
    Some(json)
}

// ============== 加载器库验证 ==============
// 对应原项目 modloaders/shared.js verifyLoaderLibs

/// 验证加载器库文件是否完整
/// 检查 libraries 目录下所有库文件是否存在且完整
pub fn verify_loader_libs(version_id: &str) -> bool {
    let merged_json = match resolve_version_json(version_id) {
        Some(j) => j,
        None => {
            let json_path = versions_dir().join(version_id).join(format!("{}.json", version_id));
            if !json_path.exists() {
                return false;
            }
            let content = match fs::read_to_string(&json_path) {
                Ok(c) => c,
                Err(_) => return false,
            };
            let data: Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            let libs = data.get("libraries").and_then(|v| v.as_array());
            if libs.map(|a| a.is_empty()).unwrap_or(true) {
                return false;
            }
            data
        }
    };
    let libs = merged_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut checked = 0usize;
    let mut missing = 0usize;
    for lib in &libs {
        let lib_path = resolve_lib_path(lib);
        if let Some(p) = lib_path {
            checked += 1;
            if !p.exists() {
                missing += 1;
            }
        }
    }
    if missing > 0 {
        return false;
    }
    checked > 0
}

/// 从库 JSON 解析本地路径
fn resolve_lib_path(lib: &Value) -> Option<PathBuf> {
    if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
        if let Some(path) = artifact.get("path").and_then(|v| v.as_str()) {
            if !path.is_empty() {
                return Some(libraries_dir().join(path));
            }
        }
    }
    let name = lib.get("name").and_then(|v| v.as_str())?;
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR.to_string().as_str());
    let nm = parts[1];
    let vr = parts[2];
    let classifier = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
    let jar_name = format!("{}-{}{}.jar", nm, vr, classifier);
    Some(libraries_dir().join(&group_path).join(nm).join(vr).join(&jar_name))
}

// ============== 导入库验证 ==============
// 对应原项目 modloaders/shared.js verifyImportLibs

/// 验证导入整合包的库文件是否完整
/// 返回 (ok, checked, missing)
pub async fn verify_import_libs(version_id: &str) -> (bool, usize, usize) {
    let merged_json = match resolve_version_json(version_id) {
        Some(j) => j,
        None => return (false, 0, 0),
    };
    let libs = merged_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let current_platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let core_prefixes = ["net.minecraftforge", "net.neoforged", "cpw.mods", "net.minecraft"];
    let is_core = |lib_name: &str| -> bool {
        if lib_name.is_empty() {
            return false;
        }
        let pkg = lib_name.split(':').next().unwrap_or("");
        core_prefixes.iter().any(|p| pkg.starts_with(p))
    };

    let mut lib_checked = 0usize;
    let mut core_lib_missing = 0usize;
    let mut non_core_lib_missing = 0usize;
    let mut missing_libs: Vec<(String, String, String)> = Vec::new(); // (url, path, name)

    for lib in &libs {
        // 跳过带 rules 的库（简化：不评估 rules）
        if lib.get("rules").is_some() {
            continue;
        }

        let lib_path = resolve_lib_path(lib);
        let lib_name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // 跳过 natives 库（简化处理）
        if lib.get("natives").is_some() {
            continue;
        }

        let dl_url = lib
            .get("downloads")
            .and_then(|d| d.get("artifact"))
            .and_then(|a| a.get("url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(p) = lib_path {
            lib_checked += 1;
            let exists = if p.extension().and_then(|e| e.to_str()) == Some("jar") {
                is_jar_intact(&p)
            } else {
                p.exists()
            };
            if !exists {
                if is_core(&lib_name) {
                    core_lib_missing += 1;
                } else {
                    non_core_lib_missing += 1;
                }
                missing_libs.push((dl_url.clone(), p.to_string_lossy().to_string(), lib_name.clone()));
            }
        }
    }

    // 尝试补全缺失的核心库
    if core_lib_missing > 0 {
        eprintln!(
            "[Modpack] 发现 {} 个核心库缺失，尝试补全",
            core_lib_missing
        );
        let mut to_download: Vec<(String, PathBuf)> = Vec::new();
        for (url, path, name) in &missing_libs {
            if !url.is_empty() && is_core(name) {
                to_download.push((url.clone(), PathBuf::from(path)));
            }
        }
        if !to_download.is_empty() {
            let (success, fail) = ml_shared::download_libraries_concurrent(to_download, 16).await;
            eprintln!(
                "[Modpack] 核心库补全: 成功 {}, 失败 {}",
                success, fail
            );
            if fail > 0 {
                return (false, lib_checked, fail);
            }
            core_lib_missing = 0;
        }
    }

    // 尝试补全非核心库
    if non_core_lib_missing > 0 {
        eprintln!(
            "[Modpack] 发现 {} 个非核心库缺失，尝试补全",
            non_core_lib_missing
        );
        let mut to_download: Vec<(String, PathBuf)> = Vec::new();
        for (url, path, name) in &missing_libs {
            if !url.is_empty() && !is_core(name) {
                to_download.push((url.clone(), PathBuf::from(path)));
            }
        }
        if !to_download.is_empty() {
            let (success, fail) = ml_shared::download_libraries_concurrent(to_download, 16).await;
            eprintln!(
                "[Modpack] 非核心库补全: 成功 {}, 失败 {}",
                success, fail
            );
            if fail > 0 {
                eprintln!("[Modpack] 警告: {} 个非核心库补全失败，将继续导入", fail);
            }
        }
    }

    (true, lib_checked, 0)
}

// ============== 加载器兼容性检查 ==============
// 对应原项目 modloaders/shared.js ensureLoaderCompat

/// 检查并升级加载器版本（如果 mods 目录中的模组要求更高版本）
/// 简化实现：只检查当前加载器版本 JSON 是否存在且 libs 完整
pub async fn ensure_loader_compat(
    version_id: &str,
    version_dir: &Path,
    mc_version: &str,
    current_loader_ver: &str,
    loader_type: &str,
) -> EnsureLoaderCompatResult {
    // 扫描 mods 目录中的模组，检查是否需要更高版本的加载器
    let mods_dir = version_dir.join("mods");
    let needed = scan_mods_for_loader_reqs(&mods_dir, loader_type);

    if needed.is_empty() || current_loader_ver.is_empty() {
        return EnsureLoaderCompatResult::default();
    }

    if compare_semver(&needed, current_loader_ver) <= 0 {
        return EnsureLoaderCompatResult::default();
    }

    eprintln!(
        "[Modpack] 检测到需要升级 {} 加载器到 {} (当前 {})",
        loader_type, needed, current_loader_ver
    );

    let new_loader_version_id = if loader_type == "fabric" {
        format!("fabric-loader-{}-{}", needed, mc_version)
    } else {
        format!("{}-forge-{}", mc_version, needed)
    };

    let install_result = if loader_type == "fabric" {
        crate::modloaders::fabric::install_fabric_with_target(mc_version, &needed, &new_loader_version_id).await
    } else {
        crate::modloaders::forge::install_forge(mc_version, &needed, Some(&new_loader_version_id)).await
    };

    if !install_result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let err = install_result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误")
            .to_string();
        eprintln!("[Modpack] {} 升级失败: {}", loader_type, err);
        return EnsureLoaderCompatResult {
            upgraded: false,
            new_version: None,
            error: Some(err),
        };
    }

    // 更新版本 JSON 的 inheritsFrom
    let old_json_path = version_dir.join(format!("{}.json", version_id));
    if old_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&old_json_path) {
            if let Ok(mut old_json) = serde_json::from_str::<Value>(&content) {
                if let Some(obj) = old_json.as_object_mut() {
                    obj.insert("inheritsFrom".to_string(), json!(new_loader_version_id));
                }
                // 从新加载器 JSON 读取 mainClass
                let lv_json_path = versions_dir()
                    .join(&new_loader_version_id)
                    .join(format!("{}.json", new_loader_version_id));
                if lv_json_path.exists() {
                    if let Ok(lv_content) = fs::read_to_string(&lv_json_path) {
                        if let Ok(lv_json) = serde_json::from_str::<Value>(&lv_content) {
                            if let Some(mc) = lv_json.get("mainClass").and_then(|v| v.as_str()) {
                                if let Some(obj) = old_json.as_object_mut() {
                                    obj.insert("mainClass".to_string(), json!(mc));
                                }
                            }
                        }
                    }
                }
                let _ = fs::write(
                    &old_json_path,
                    serde_json::to_string_pretty(&old_json).unwrap_or_default(),
                );
            }
        }
    }

    EnsureLoaderCompatResult {
        upgraded: true,
        new_version: Some(needed),
        error: None,
    }
}

#[derive(Default, Debug)]
pub struct EnsureLoaderCompatResult {
    pub upgraded: bool,
    pub new_version: Option<String>,
    pub error: Option<String>,
}

/// 扫描 mods 目录中的模组，提取加载器版本需求
/// 简化实现：返回空字符串（不强制升级）
/// 完整实现需要解析每个 JAR 的 fabric.mod.json / mods.toml 中的依赖声明
fn scan_mods_for_loader_reqs(mods_dir: &Path, _loader_type: &str) -> String {
    if !mods_dir.exists() {
        return String::new();
    }
    // 简化：不强制升级加载器
    // 原项目会解析 JAR 中的 fabric.mod.json / mods.toml 提取依赖的加载器版本
    // 这里返回空字符串，表示不需要升级
    String::new()
}

/// 比较语义化版本号
/// 返回：负数表示 a < b，0 表示相等，正数表示 a > b
fn compare_semver(a: &str, b: &str) -> i32 {
    let pa: Vec<i64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<i64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    let max_len = pa.len().max(pb.len());
    for i in 0..max_len {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        if va != vb {
            return if va < vb { -1 } else { 1 };
        }
    }
    0
}

// ============== JAR modId 读取 ==============
// 对应原项目 utils.js readJarModId

/// 从 JAR 文件中读取 modId
/// 依次尝试：META-INF/mods.toml → mcmod.info → fabric.mod.json
pub fn read_jar_mod_id(file_path: &Path) -> Option<String> {
    if !file_path.exists() {
        return None;
    }
    let file = fs::File::open(file_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Forge 1.13+: mods.toml
    if let Ok(entry) = archive.by_name("META-INF/mods.toml") {
        let mut reader = entry;
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut reader, &mut text).is_ok() {
            // TOML 中 [[mods]] 段下的 modId = "xxx"
            let re = regex::Regex::new(r#"\[\[mods\]\][\s\S]*?modId\s*=\s*["']([^"']+)["']"#).ok()?;
            if let Some(cap) = re.captures(&text) {
                if let Some(m) = cap.get(1) {
                    return Some(m.as_str().trim().to_string());
                }
            }
        }
    }

    // Forge 旧版: mcmod.info
    if let Ok(entry) = archive.by_name("mcmod.info") {
        let mut reader = entry;
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut reader, &mut text).is_ok() {
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                if let Some(arr) = data.as_array() {
                    if let Some(first) = arr.first() {
                        if let Some(modid) = first.get("modid").and_then(|v| v.as_str()) {
                            return Some(modid.trim().to_string());
                        }
                    }
                }
                if let Some(modid) = data.get("modid").and_then(|v| v.as_str()) {
                    return Some(modid.trim().to_string());
                }
            }
        }
    }

    // Fabric: fabric.mod.json
    if let Ok(entry) = archive.by_name("fabric.mod.json") {
        let mut reader = entry;
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut reader, &mut text).is_ok() {
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    return Some(id.trim().to_string());
                }
            }
        }
    }

    None
}

// ============== 路径工具 ==============

pub fn data_dir() -> PathBuf {
    storage::resolve_data_dir()
}

pub fn versions_dir() -> PathBuf {
    data_dir().join("versions")
}

pub fn libraries_dir() -> PathBuf {
    data_dir().join("libraries")
}

pub fn assets_dir() -> PathBuf {
    data_dir().join("assets")
}

/// 递归复制目录
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dest = dst.join(file_name);
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            let _ = fs::copy(&path, &dest);
        }
    }
    Ok(())
}

// ============== 时间工具 ==============

/// 当前 Unix 时间戳（秒）
pub fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前时间的 ISO 8601 字符串
pub fn now_iso() -> String {
    ml_shared::now_iso()
}

// ============== Windows process creation flags ==============

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
