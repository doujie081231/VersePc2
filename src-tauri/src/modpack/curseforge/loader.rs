// loader.rs — 由 curseforge.rs 拆分
#![allow(unused_imports)]

// 加载器安装与版本 JSON 构造
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

/// 安装模组加载器（Forge/NeoForge/Fabric）
/// 已存在则校验 libs 完整性，损坏则删除重装
#[allow(clippy::too_many_arguments)]
pub(crate) async fn install_loader(
    loader_kind: &LoaderKind,
    mc_version: &str,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    _app: &AppHandle,
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
                    None,
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
                    None,
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

/// 创建版本 JSON（inheritsFrom + mainClass + 复制主 jar）
pub(crate) fn create_version_json(
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

/// 解析 modLoader id，返回 (加载器类型, forge版本, fabric版本, neoforge版本)
/// 支持 forge-/neoforge-/fabric-/fabric-loader- 四种前缀正则
pub(crate) fn parse_loader_id(id: &str) -> (LoaderKind, String, String, String) {
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

/// 检查 Forge 核心文件是否存在且完整
/// forge-client.jar / client-srg.jar / client-extra.jar
pub(crate) fn check_forge_core_files(merged_json: &Value) -> Result<(), String> {
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
