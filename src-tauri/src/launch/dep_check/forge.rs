// forge.rs — 由 dep_check.rs 拆分
#![allow(unused_imports)]

// Forge 核心与 mrpack 依赖检查
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use regex::Regex;
use crate::java;
use crate::storage;
use crate::utils;
use super::*;

/// 沿 inheritsFrom 链判断版本是否依赖 Forge（非 NeoForge）
pub(crate) fn scan_inherits_forge(version_id: &str, visited: &mut std::collections::HashSet<String>) -> bool {
    if version_id.is_empty() || visited.contains(version_id) {
        return false;
    }
    let vl = version_id.to_lowercase();
    if vl.contains("forge") && !vl.contains("neoforge") && !vl.contains("neoforged") {
        return true;
    }
    visited.insert(version_id.to_string());
    let versions_dir = storage::resolve_data_dir().join("versions");
    let vj_path = versions_dir.join(version_id).join(format!("{}.json", version_id));
    if !vj_path.exists() {
        return false;
    }
    if let Ok(content) = std::fs::read_to_string(&vj_path)
        && let Ok(vj) = serde_json::from_str::<Value>(&content) {
            let inherits = utils::get_str(&vj, "inheritsFrom");
            if !inherits.is_empty() && !visited.contains(&inherits) {
                return scan_inherits_forge(&inherits, visited);
            }
        }
    false
}

/// 检查 Forge / NeoForge 核心库完整性
pub(crate) fn check_forge_core(
    version_json: &Value,
    version_id: &str,
    external_version_dir: Option<&Path>,
    libraries_dir: &Path,
    result: &mut DepCheckResult,
) {
    let forge_libs: Vec<Value> = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter(|l| {
            let name = utils::get_str(l, "name");
            name.starts_with("net.minecraftforge:forge:")
                || name.starts_with("net.minecraftforge:fmlloader:")
                || name.starts_with("net.neoforged:neoforge:")
                || name.starts_with("net.neoforged.fancymodloader:")
                || (name.starts_with("net.minecraft:client:") && (name.ends_with(":srg") || name.ends_with(":extra")))
        }).cloned().collect())
        .unwrap_or_default();

    let v_lower = version_id.to_lowercase();
    let is_neo = v_lower.contains("neoforge") || v_lower.contains("neoforged");
    let has_forge_id = v_lower.contains("forge") && !is_neo;
    let has_forge_lib_only = version_json.get("libraries").and_then(|v| v.as_array()).map(|libs| {
        libs.iter().any(|l| {
            let name = utils::get_str(l, "name");
            name.starts_with("net.minecraftforge:forge:") || name.starts_with("net.minecraftforge:fmlloader:")
        })
    }).unwrap_or(false);

    let mut visited = std::collections::HashSet::new();
    let is_forge_version = has_forge_id || scan_inherits_forge(version_id, &mut visited) || has_forge_lib_only;

    if !is_forge_version {
        return;
    }

    // 新版 Forge 格式检测（MC 26+ 嵌入式）
    let game_args = version_json.get("arguments").and_then(|v| v.get("game")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let has_fml_args = game_args.iter().any(|a| {
        let s = a.as_str().unwrap_or("");
        s == "--fml.forgeVersion" || s == "--fml.mcVersion"
    });
    let main_class = utils::get_str(version_json, "mainClass");
    let has_bootstrap_main = main_class.contains("bootstraplauncher") || main_class.contains("BootstrapLauncher") || main_class.contains("cpw.mods");
    let has_forge_libs_in_json = forge_libs.iter().any(|l| {
        let name = utils::get_str(l, "name");
        name.starts_with("net.minecraftforge:forge:") || name.starts_with("net.minecraftforge:fmlloader:") || name.starts_with("net.minecraftforge:fmlcore:")
    });
    let is_new_forge_format = !is_neo && !has_fml_args && !has_bootstrap_main && !has_forge_libs_in_json;

    if is_new_forge_format {
        result.forge_core.ok = true;
        result.forge_core.message = "新版Forge格式，核心已嵌入版本JAR".to_string();
        return;
    }

    // 收集需要检查的核心库
    let mut forge_core_libs: Vec<(String, PathBuf, String)> = Vec::new(); // (name, path, desc)

    let find_forge_lib = |pred: &dyn Fn(&str) -> bool| -> Option<&Value> {
        forge_libs.iter().find(|l| pred(&utils::get_str(l, "name")))
    };

    // 从 libraries 中查找各类核心库
    let forge_client_lib = find_forge_lib(&|n| n.starts_with("net.minecraftforge:forge:") && (n.ends_with(":client") || n.split(':').count() == 3));
    let forge_main_lib = find_forge_lib(&|n| n.starts_with("net.minecraftforge:forge:") && n.split(':').count() == 3);
    let neo_forge_lib = find_forge_lib(&|n| n.starts_with("net.neoforged:neoforge:"));
    let neo_fml_lib = find_forge_lib(&|n| n.starts_with("net.neoforged.fancymodloader:loader:"));
    let srg_lib = find_forge_lib(&|n| n.starts_with("net.minecraft:client:") && n.ends_with(":srg"));
    let extra_lib = find_forge_lib(&|n| n.starts_with("net.minecraft:client:") && n.ends_with(":extra"));

    let external_root_for_forge = external_version_dir.and_then(find_external_root);

    let find_forge_core_file = |fp: &[&str], jar_name: &str| -> PathBuf {
        let group_path = fp[0].replace('.', std::path::MAIN_SEPARATOR_STR);
        let local_path = libraries_dir.join(&group_path).join(fp[1]).join(fp[2]).join(jar_name);
        if local_path.exists() {
            return local_path;
        }
        if let Some(root) = &external_root_for_forge {
            let ext_path = root.join("libraries").join(&group_path).join(fp[1]).join(fp[2]).join(jar_name);
            if ext_path.exists() {
                return ext_path;
            }
        }
        local_path
    };

    if let Some(lib) = forge_client_lib {
        let name = utils::get_str(lib, "name");
        let fp: Vec<&str> = name.split(':').collect();
        let cl = if fp.len() >= 4 { format!("-{}", fp[3]) } else { String::new() };
        let jar_name = format!("{}-{}{}.jar", fp[1], fp[2], cl);
        forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "Forge客户端核心".to_string()));
    }
    if let Some(lib) = forge_main_lib
        && Some(lib) != forge_client_lib {
            let name = utils::get_str(lib, "name");
            let fp: Vec<&str> = name.split(':').collect();
            let jar_name = format!("{}-{}.jar", fp[1], fp[2]);
            forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "Forge主核心".to_string()));
        }
    if let Some(lib) = srg_lib {
        let name = utils::get_str(lib, "name");
        let sp: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}-srg.jar", sp[1], sp[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&sp, &jar_name), "Minecraft SRG映射客户端".to_string()));
    }
    if let Some(lib) = extra_lib {
        let name = utils::get_str(lib, "name");
        let ep: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}-extra.jar", ep[1], ep[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&ep, &jar_name), "Minecraft额外客户端".to_string()));
    }
    if let Some(lib) = neo_forge_lib {
        let name = utils::get_str(lib, "name");
        let is_neo_client_virtual = name.ends_with(":client");
        let mut neo_patched_ok = false;
        if is_neo_client_virtual {
            let neo_ver = name.split(':').nth(2).unwrap_or("");
            let patched_jar = libraries_dir
                .join("net")
                .join("neoforged")
                .join("minecraft-client-patched")
                .join(neo_ver)
                .join(format!("minecraft-client-patched-{}.jar", neo_ver));
            neo_patched_ok = patched_jar.exists() && utils::is_jar_intact(&patched_jar);
        }
        if !is_neo_client_virtual || !neo_patched_ok {
            let fp: Vec<&str> = name.split(':').collect();
            let cl = if fp.len() >= 4 { format!("-{}", fp[3]) } else { String::new() };
            let jar_name = format!("{}-{}{}.jar", fp[1], fp[2], cl);
            forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "NeoForge核心".to_string()));
        }
    }
    if let Some(lib) = neo_fml_lib {
        let name = utils::get_str(lib, "name");
        let fp: Vec<&str> = name.split(':').collect();
        let jar_name = format!("{}-{}.jar", fp[1], fp[2]);
        forge_core_libs.push((name.clone(), find_forge_core_file(&fp, &jar_name), "NeoForge FML加载器".to_string()));
    }

    // libraries 中未直接找到时，按版本号目录结构兜底搜索
    if forge_core_libs.is_empty() {
        let mut forge_ver_match = None;
        // 优先从 game args / fmlloader 库中提取真实版本号
        let mut fallback_mc_ver: Option<String> = None;
        let mut fallback_f_ver: Option<String> = None;

        if let Some(fv_idx) = game_args.iter().position(|a| a.as_str() == Some("--fml.forgeVersion"))
            && let Some(v) = game_args.get(fv_idx + 1).and_then(|v| v.as_str()) {
                fallback_f_ver = Some(v.to_string());
            }
        if let Some(mv_idx) = game_args.iter().position(|a| a.as_str() == Some("--fml.mcVersion"))
            && let Some(v) = game_args.get(mv_idx + 1).and_then(|v| v.as_str()) {
                fallback_mc_ver = Some(v.to_string());
            }
        if fallback_mc_ver.is_none() {
            let cv = utils::get_str(version_json, "clientVersion");
            if !cv.is_empty() {
                fallback_mc_ver = Some(cv);
            }
        }
        if (fallback_f_ver.is_none() || fallback_mc_ver.is_none())
            && let Some(fml_lib) = forge_libs.iter().find(|l| {
                let name = utils::get_str(l, "name");
                name.starts_with("net.minecraftforge:fmlloader:") || name.starts_with("net.minecraftforge:forge:")
            }) {
                let fml_name = utils::get_str(fml_lib, "name");
                let parts: Vec<&str> = fml_name.split(':').collect();
                if parts.len() >= 3 {
                    let ver_part = parts[2];
                    if let Some(dash_idx) = ver_part.rfind('-')
                        && dash_idx > 0 {
                            if fallback_mc_ver.is_none() {
                                fallback_mc_ver = Some(ver_part[..dash_idx].to_string());
                            }
                            if fallback_f_ver.is_none() {
                                fallback_f_ver = Some(ver_part[dash_idx + 1..].to_string());
                            }
                        }
                }
            }

        if fallback_mc_ver.is_none() || fallback_f_ver.is_none() {
            // 正则匹配版本 ID
            if let Some(m) = regex_match_forge_version(version_id) {
                forge_ver_match = Some(m);
            } else if !inherits_from_field(version_json).is_empty()
                && let Some(m) = regex_match_forge_version(&inherits_from_field(version_json)) {
                    forge_ver_match = Some(m);
                }
        }

        let mc_ver = fallback_mc_ver.clone().or_else(|| forge_ver_match.as_ref().map(|m| m.0.clone()));
        let f_ver = fallback_f_ver.or_else(|| forge_ver_match.as_ref().map(|m| m.1.clone()));

        if let (Some(mc_ver), Some(f_ver)) = (mc_ver, f_ver) {
            let forge_search_bases = {
                let mut v: Vec<PathBuf> = vec![libraries_dir.to_path_buf()];
                if let Some(root) = &external_root_for_forge {
                    v.insert(0, root.join("libraries"));
                }
                v
            };

            // 1) Forge client jar
            for base in &forge_search_bases {
                let forge_dir = base.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver));
                if forge_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&forge_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.file_name().and_then(|n| n.to_str()).map(|s| s.ends_with("-client.jar")).unwrap_or(false) {
                                forge_core_libs.push((format!("forge-client:{}-{}", mc_ver, f_ver), p, "Forge客户端核心".to_string()));
                                break;
                            }
                        }
                    }
                    break;
                }
            }
            // 2) NeoForge jar
            for base in &forge_search_bases {
                let neo_dir = base.join("net").join("neoforged").join("neoforge").join(&f_ver);
                if neo_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&neo_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            if name.ends_with(".jar") && !name.ends_with("-sources.jar") && !name.ends_with("-javadoc.jar") {
                                forge_core_libs.push((format!("net.neoforged:neoforge:{}", f_ver), p, "NeoForge核心".to_string()));
                                break;
                            }
                        }
                    }
                    break;
                }
            }
            // 3) Minecraft client srg/extra
            for base in &forge_search_bases {
                let client_dir = base.join("net").join("minecraft").join("client");
                if client_dir.exists() {
                    if let Ok(entries) = std::fs::read_dir(&client_dir) {
                        for entry in entries.flatten() {
                            let sd = entry.file_name().to_string_lossy().to_string();
                            if !sd.starts_with(&format!("{}-", mc_ver)) && sd != mc_ver {
                                continue;
                            }
                            let full_dir = client_dir.join(&sd);
                            if !full_dir.is_dir() {
                                continue;
                            }
                            if let Ok(files) = std::fs::read_dir(&full_dir) {
                                for f in files.flatten() {
                                    let name = f.file_name().to_string_lossy().to_string();
                                    let p = f.path();
                                    if name.ends_with("-srg.jar") {
                                        forge_core_libs.push((format!("client-srg:{}", sd), p.clone(), "Minecraft SRG映射客户端".to_string()));
                                    }
                                    if name.ends_with("-extra.jar") {
                                        forge_core_libs.push((format!("client-extra:{}", sd), p, "Minecraft额外客户端".to_string()));
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            }
            // 4) 新式 Forge 模块化核心（fmlcore/javafmllanguage/mclanguage/lowcodelanguage）
            if main_class.contains("bootstraplauncher") || main_class.contains("ForgeBootstrap") {
                let fml_version = format!("{}-{}", mc_ver, f_ver);
                for mod_name in &["fmlcore", "javafmllanguage", "mclanguage", "lowcodelanguage"] {
                    let is_declared = forge_libs.iter().any(|l| utils::get_str(l, "name").starts_with(&format!("net.minecraftforge:{}:{}", mod_name, fml_version)));
                    if is_declared {
                        let mod_path = libraries_dir.join("net").join("minecraftforge").join(mod_name).join(&fml_version).join(format!("{}-{}.jar", mod_name, fml_version));
                        if !forge_core_libs.iter().any(|(_, p, _)| p == &mod_path) {
                            forge_core_libs.push((format!("net.minecraftforge:{}:{}", mod_name, fml_version), mod_path, format!("Forge模块:{}", mod_name)));
                        }
                    }
                }
            }
            // 5) Forge 通用核心 universal jar（BootstrapLauncher 模式必须）
            if !is_neo {
                let is_bootstrap_main = main_class.contains("bootstraplauncher") || main_class.contains("ForgeBootstrap");
                if is_bootstrap_main {
                    let universal_path = libraries_dir.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver)).join(format!("forge-{}-{}-universal.jar", mc_ver, f_ver));
                    forge_core_libs.push((format!("net.minecraftforge:forge:{}-{}:universal", mc_ver, f_ver), universal_path, "Forge通用核心(FML MinecraftLocator)".to_string()));
                } else {
                    // 旧版 Forge binary patcher 模式：需要 client/srg/extra 三件套
                    let client_path = libraries_dir.join("net").join("minecraftforge").join("forge").join(format!("{}-{}", mc_ver, f_ver)).join(format!("forge-{}-{}-client.jar", mc_ver, f_ver));
                    forge_core_libs.push((format!("net.minecraftforge:forge:{}-{}:client", mc_ver, f_ver), client_path, "Forge客户端核心".to_string()));

                    let client_base_dir = libraries_dir.join("net").join("minecraft").join("client");
                    let mut mcp_dir_name: Option<String> = None;
                    if client_base_dir.exists()
                        && let Ok(entries) = std::fs::read_dir(&client_base_dir) {
                            for entry in entries.flatten() {
                                let d = entry.file_name().to_string_lossy().to_string();
                                if d.starts_with(&format!("{}-", mc_ver)) && entry.path().is_dir() {
                                    mcp_dir_name = Some(d);
                                    break;
                                }
                            }
                        }
                    if let Some(mcp) = mcp_dir_name {
                        let srg_path = client_base_dir.join(&mcp).join(format!("client-{}-srg.jar", mcp));
                        let extra_path = client_base_dir.join(&mcp).join(format!("client-{}-extra.jar", mcp));
                        forge_core_libs.push((format!("net.minecraft:client:{}:srg", mcp), srg_path, "Minecraft SRG映射客户端".to_string()));
                        forge_core_libs.push((format!("net.minecraft:client:{}:extra", mcp), extra_path, "Minecraft额外客户端".to_string()));
                    } else {
                        let srg_path = client_base_dir.join(format!("{}-mcp", mc_ver)).join(format!("client-{}-mcp-srg.jar", mc_ver));
                        let extra_path = client_base_dir.join(format!("{}-mcp", mc_ver)).join(format!("client-{}-mcp-extra.jar", mc_ver));
                        forge_core_libs.push((format!("net.minecraft:client:{}:srg", mc_ver), srg_path, "Minecraft SRG映射客户端".to_string()));
                        forge_core_libs.push((format!("net.minecraft:client:{}:extra", mc_ver), extra_path, "Minecraft额外客户端".to_string()));
                    }
                }
            }
        }
    }

    // 校验所有核心库是否存在且 JAR 完整
    for (name, path, desc) in &forge_core_libs {
        let exists = path.exists();
        let intact = if exists && path.extension().and_then(|e| e.to_str()) == Some("jar") {
            utils::is_jar_intact(path)
        } else {
            exists
        };
        if !intact {
            result.forge_core.missing.push(MissingFile {
                kind: "forge_core".to_string(),
                url: String::new(),
                path: path.to_string_lossy().to_string(),
                sha1: String::new(),
                size: 0,
                name: name.clone(),
                desc: desc.clone(),
                message: format!("Forge核心库缺失: {}", desc),
            });
        }
    }

    if !result.forge_core.missing.is_empty() {
        result.forge_core.ok = false;
        let missing_names = result.forge_core.missing.iter().map(|m| {
            if m.desc.is_empty() { m.name.clone() } else { m.desc.clone() }
        }).collect::<Vec<_>>().join("、");
        result.forge_core.message = format!(
            "{} 个Forge核心库文件缺失({})，无法启动游戏。\n修复建议:\n1) 前往\"版本设置 → 文件修复\"自动修复缺失文件\n2) 重新安装该Forge版本(版本设置 → 删除后重新安装)\n3) 检查杀毒软件是否将Forge核心库文件误删并加入白名单\n4) 如果使用自定义游戏目录,确认libraries文件夹完整",
            result.forge_core.missing.len(),
            missing_names
        );

        // 为缺失的核心库构造 maven 下载 URL
        for m in result.forge_core.missing.clone() {
            if result.missing_files.iter().any(|f| f.path == m.path) {
                continue;
            }
            let mut forge_url = String::new();
            if m.name.contains(':') {
                let parts: Vec<&str> = m.name.split(':').collect();
                if parts.len() >= 3 {
                    let group_id = parts[0];
                    let artifact_id = parts[1];
                    let version = parts[2];
                    let group_path = group_id.replace('.', "/");
                    let classifier_suffix = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
                    let maven_file = format!("{}-{}{}.jar", artifact_id, version, classifier_suffix);
                    forge_url = if group_id == "net.minecraft" {
                        // net.minecraft:client:<ver>:srg / :extra 是 Forge 安装器在本地生成的产物，
                        // Maven 上没有对应文件；留空交由补全流程提示“缺少下载地址”，
                        // 而不是请求一个必然 404 的地址。
                        if parts.len() >= 4 {
                            String::new()
                        } else {
                            format!("https://libraries.minecraft.net/{}/{}/{}/{}", group_path, artifact_id, version, maven_file)
                        }
                    } else {
                        format!("https://maven.minecraftforge.net/{}/{}/{}/{}", group_path, artifact_id, version, maven_file)
                    };
                }
            }
            let message = format!("Forge核心库缺失: {}", m.desc);
            result.missing_files.push(MissingFile {
                kind: "forge_core".to_string(),
                url: forge_url,
                path: m.path,
                sha1: String::new(),
                size: 0,
                name: m.name,
                desc: m.desc,
                message,
            });
        }
    }
}

/// 从版本 ID 中正则提取 (mc_version, forge_version)
pub(crate) fn regex_match_forge_version(version_id: &str) -> Option<(String, String)> {
    // 简化版正则：匹配 <mc>-Forge-<ver> 或 <mc>-NeoForge-<ver>
    let lower = version_id.to_lowercase();
    let marker = if lower.contains("neoforge") { "neoforge" } else if lower.contains("forge") { "forge" } else { return None; };
    let idx = lower.find(&format!("-{}", marker))?;
    let mc_ver = version_id[..idx].to_string();
    let rest = &version_id[idx + marker.len() + 2..]; // skip "-forge-"
    // 取到下一个 '-' 之前
    let end = rest.find('-').unwrap_or(rest.len());
    let f_ver = rest[..end].to_string();
    if mc_ver.is_empty() || f_ver.is_empty() {
        return None;
    }
    Some((mc_ver, f_ver))
}

pub(crate) fn inherits_from_field(version_json: &Value) -> String {
    utils::get_str(version_json, "inheritsFrom")
}

pub(crate) fn check_mrpack_mods(version_id: &str, external_version_dir: Option<&Path>, result: &mut DepCheckResult) {
    let version_dir = external_version_dir
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| storage::resolve_data_dir().join("versions").join(version_id));
    let manifest_path = version_dir.join("mrpack-manifest.json");
    if !manifest_path.exists() {
        return;
    }
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let manifest: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    let files = manifest.get("files").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mod_entries: Vec<&Value> = files.iter().filter(|f| {
        utils::get_str(f, "path").starts_with("mods/")
    }).collect();
    result.mrpack_mods.total = mod_entries.len() as u64;

    for entry in &mod_entries {
        let path = utils::get_str(entry, "path");
        let file_name = Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let dest_path = version_dir.join(&path);
        let mut need_recheck = false;

        if !dest_path.exists() {
            need_recheck = true;
        } else {
            let expected_size = entry.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
            let expected_sha1 = entry.get("hashes").and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("");
            if expected_size > 0 {
                if let Ok(meta) = std::fs::metadata(&dest_path) {
                    if meta.len() != expected_size {
                        need_recheck = true;
                    }
                } else {
                    need_recheck = true;
                }
            }
            if !need_recheck && dest_path.extension().and_then(|e| e.to_str()) == Some("jar")
                && !utils::is_jar_intact(&dest_path) {
                    need_recheck = true;
                }
            if !need_recheck && !expected_sha1.is_empty()
                && let Some(actual) = utils::calculate_sha1(&dest_path)
                    && actual != expected_sha1 {
                        need_recheck = true;
                    }
        }

        if need_recheck {
            let dl_url = entry.get("downloads").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let sha1 = entry.get("hashes").and_then(|v| v.get("sha1")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let size = entry.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
            let urls: Vec<String> = entry.get("downloads").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let _ = urls; // 保留扩展字段以备下载模块使用
            result.mrpack_mods.missing.push(MissingFile {
                kind: "mod".to_string(),
                url: dl_url,
                path: dest_path.to_string_lossy().to_string(),
                sha1,
                size,
                name: file_name,
                desc: String::new(),
                message: String::new(),
            });
        }
    }

    result.mrpack_mods.ok = result.mrpack_mods.missing.is_empty();
    if !result.mrpack_mods.missing.is_empty() {
        result.mrpack_mods.message = format!("{} 个 Mod 文件缺失或损坏", result.mrpack_mods.missing.len());
        result.missing_files.extend(result.mrpack_mods.missing.clone());
    }
}
