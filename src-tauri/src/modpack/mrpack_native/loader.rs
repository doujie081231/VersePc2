// modpack/mrpack_native/loader.rs — 整合包加载器安装

use tauri::AppHandle;

use crate::modpack::curseforge_shared as cf_shared;
use super::{emit_progress, versions_dir};


/// 安装模组加载器（Forge/NeoForge/Fabric）
/// 已存在则校验 libs 完整性，损坏则删除重装
pub(crate) async fn install_loader(
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
