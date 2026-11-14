// modpack/curseforge_compat.rs — CurseForge 整合包加载器库验证与兼容性检查
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::modloaders::shared as ml_shared;

use super::curseforge_chain::resolve_version_json;
use super::curseforge_fs::{libraries_dir, versions_dir};
use super::curseforge_repair::is_jar_intact;

// ============== 加载器库验证 ==============

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
    if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact"))
        && let Some(path) = artifact.get("path").and_then(|v| v.as_str())
            && !path.is_empty() {
                return Some(libraries_dir().join(path));
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

    let _current_platform = if cfg!(target_os = "windows") {
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
        crate::modloaders::forge::install_forge(mc_version, &needed, Some(&new_loader_version_id), None).await
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
    if old_json_path.exists()
        && let Ok(content) = fs::read_to_string(&old_json_path)
            && let Ok(mut old_json) = serde_json::from_str::<Value>(&content) {
                if let Some(obj) = old_json.as_object_mut() {
                    obj.insert("inheritsFrom".to_string(), json!(new_loader_version_id));
                }
                // 从新加载器 JSON 读取 mainClass
                let lv_json_path = versions_dir()
                    .join(&new_loader_version_id)
                    .join(format!("{}.json", new_loader_version_id));
                if lv_json_path.exists()
                    && let Ok(lv_content) = fs::read_to_string(&lv_json_path)
                        && let Ok(lv_json) = serde_json::from_str::<Value>(&lv_content)
                            && let Some(mc) = lv_json.get("mainClass").and_then(|v| v.as_str())
                                && let Some(obj) = old_json.as_object_mut() {
                                    obj.insert("mainClass".to_string(), json!(mc));
                                }
                let _ = fs::write(
                    &old_json_path,
                    serde_json::to_string_pretty(&old_json).unwrap_or_default(),
                );
            }

    EnsureLoaderCompatResult {
        upgraded: true,
        new_version: Some(needed),
        error: None,
    }
}

#[derive(Default, Debug)]
#[allow(dead_code)]
pub struct EnsureLoaderCompatResult {
    pub upgraded: bool,
    pub new_version: Option<String>,
    pub error: Option<String>,
}

/// 解析版本需求字符串，返回 (操作符, 版本号)
/// 无操作符时默认为 >=
fn parse_version_requirement(req: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"^([><=]+)\s*(.+)").ok()?;
    if let Some(cap) = re.captures(req) {
        Some((cap[1].to_string(), cap[2].trim().to_string()))
    } else {
        Some((">=".to_string(), req.trim().to_string()))
    }
}

/// 扫描 mods 目录中的模组，提取指定加载器所需的最低版本
/// 逐个解析 JAR 中的 fabric.mod.json / quilt.mod.json 的依赖声明，
/// 取所有模组要求的最高版本。仅统计 >=、=、== 这类最小值约束。
fn scan_mods_for_loader_reqs(mods_dir: &Path, loader_type: &str) -> String {
    if !mods_dir.exists() {
        return String::new();
    }
    let mut needed = String::new();
    let entries = match std::fs::read_dir(mods_dir) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jar") {
            continue;
        }
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(_) => continue,
        };
        let mut meta_text: Option<String> = None;
        let mut is_quilt = false;
        if let Ok(mut entry) = archive.by_name("fabric.mod.json") {
            let mut text = String::new();
            if std::io::Read::read_to_string(&mut entry, &mut text).is_ok() {
                meta_text = Some(text);
            }
        } else if let Ok(mut entry) = archive.by_name("quilt.mod.json") {
            let mut text = String::new();
            if std::io::Read::read_to_string(&mut entry, &mut text).is_ok() {
                meta_text = Some(text);
                is_quilt = true;
            }
        }
        let meta_text = match meta_text {
            Some(t) => t,
            None => continue,
        };
        let meta: Value = match serde_json::from_str(&meta_text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let deps = if is_quilt {
            meta.get("quilt_loader").and_then(|q| q.get("dependencies"))
        } else {
            meta.get("depends")
        };
        let deps = match deps {
            Some(d) => d,
            None => continue,
        };
        let req_str = if loader_type == "fabric" {
            deps.get("fabricloader").or_else(|| deps.get("fabric-loader"))
        } else {
            deps.get("forge")
        };
        let req_str = match req_str {
            Some(Value::String(s)) => s.as_str(),
            _ => continue,
        };
        let parsed = match parse_version_requirement(req_str) {
            Some(p) => p,
            None => continue,
        };
        let (op, version) = parsed;
        if op != ">=" && op != "=" && op != "==" {
            continue;
        }
        if needed.is_empty() || compare_semver(&version, &needed) > 0 {
            needed = version;
        }
    }
    needed
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
