// versions/export.rs — 启动脚本与整合包导出
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::storage;
use crate::utils;

use super::diagnose::resolve_version_json_recursive;
use super::external::{get_version_game_dir, is_external_version, resolve_external_version_dir};


/// POST /api/version/export-script — 导出启动脚本
///
/// 参数：versionId（必填）、scriptType（bat/sh，默认按平台选）
/// 流程：构建启动参数 → 生成 .bat/.sh 内容 → 写入 DATA_DIR/temp/<versionId>.<ext>
/// 返回：{ success, content, fileName, path }
pub(crate) async fn handle_export_script(body: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;
    use crate::launch;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }

    let is_external = is_external_version(&version_id);
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
    } else {
        version_id.clone()
    };

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    // 1. 解析版本 JSON（递归合并 inheritsFrom）
    let version_json = match resolve_version_json_recursive(&clean_id, &versions_dir) {
        Some(j) => j,
        None => return ApiResult::err(404, "版本 JSON 不存在"),
    };

    // 2. 加载设置和账号
    let settings = storage::load_settings();
    let accounts = storage::load_accounts();
    let accounts_arr = accounts.as_array().cloned().unwrap_or_default();
    let selected_account_id = utils::get_str(&settings, "selectedAccount");
    let account = accounts_arr
        .iter()
        .find(|a| utils::get_str(a, "id") == selected_account_id)
        .or(accounts_arr.first())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "username": "Player",
                "uuid": "",
                "accessToken": "",
                "type": "offline"
            })
        });

    // 3. 构建启动参数
    let launch_args = launch::build_launch_arguments(
        &version_json,
        &settings,
        &account,
        &clean_id,
        None,
        None,
        None,
    );

    // 4. 选择 Java 路径
    let java_path = launch::dep_check::select_java_for_version(&clean_id, &settings, &version_json);
    let java_path = if java_path.is_empty() {
        "java".to_string()
    } else {
        java_path
    };

    // 5. 决定游戏目录
    let game_dir = get_version_game_dir(&clean_id, is_external);

    // 6. 决定脚本扩展名
    let script_type = utils::get_str(&data, "scriptType").to_lowercase();
    let is_windows = cfg!(target_os = "windows");
    let ext = if script_type == "bat" {
        "bat"
    } else if script_type == "sh" {
        "sh"
    } else if is_windows {
        "bat"
    } else {
        "sh"
    };

    // 7. 拼接参数字符串（含空格的参数加双引号）
    let args_str = launch_args
        .args
        .iter()
        .map(|a| {
            if a.contains(' ') || a.contains('\t') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // 8. 生成脚本内容
    let content = if ext == "bat" {
        let mut s = String::new();
        s.push_str("@echo off\r\n");
        s.push_str(&format!("cd /d \"{}\"\r\n", game_dir.display()));
        s.push_str(&format!("\"{}\" {}\r\n", java_path, args_str));
        s.push_str("pause\r\n");
        s
    } else {
        let mut s = String::new();
        s.push_str("#!/bin/bash\n");
        s.push_str(&format!("cd \"{}\"\n", game_dir.display()));
        s.push_str(&format!("\"{}\" {}\n", java_path, args_str));
        s.push_str("read -p \"按回车键退出...\"\n");
        s
    };

    // 9. 写入临时文件
    let temp_dir = data_dir.join("temp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_name = format!("{}.{}", clean_id, ext);
    let script_path = temp_dir.join(&file_name);

    match std::fs::write(&script_path, &content) {
        Ok(_) => {
            #[cfg(unix)]
            if ext == "sh" {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &script_path,
                    std::fs::Permissions::from_mode(0o755),
                );
            }
            ApiResult::ok(json!({
                "success": true,
                "content": content,
                "fileName": file_name,
                "path": script_path.to_string_lossy()
            }))
        }
        Err(e) => ApiResult::err(500, &format!("写入脚本失败: {}", e)),
    }
}

/// POST /api/version/export-modpack — 导出整合包（生成 overrides + modpack.json）
///
/// 参数：versionId（必填）、name、version、author、description、selectedKeys（导出内容 key 数组）
/// 将选中的游戏内容写入 overrides/ 目录，
/// 排除系统/缓存目录、日志与临时文件、版本自身 JAR/JSON，并写入 modpack.json 元信息。
/// 写入 DATA_DIR/temp/<versionId>-export.zip
/// 返回：{ success, path, fileName }
pub(crate) fn handle_export_modpack(body: &Option<Value>) -> crate::api::ApiResult {
    use crate::api::ApiResult;

    let data = body.clone().unwrap_or(Value::Null);
    let version_id = utils::get_str(&data, "versionId");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }
    let pack_name = utils::get_str(&data, "name");
    let pack_version = utils::get_str(&data, "version");
    let author = utils::get_str(&data, "author");
    let description = utils::get_str(&data, "description");
    let selected_keys: Vec<String> = data
        .get("selectedKeys")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let is_external = is_external_version(&version_id);
    let clean_id = if is_external {
        version_id.split(" [外部").next().unwrap_or(&version_id).to_string()
    } else {
        version_id.clone()
    };

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    // 外部文件夹版本的目录不在本启动器 data 目录下，必须按 external-folders 配置定位，
    // 否则会误报「版本目录不存在」而无法导出整合包。
    let version_dir = if is_external {
        match resolve_external_version_dir(&version_id) {
            Some(p) => PathBuf::from(p),
            None => return ApiResult::err(404, "版本目录不存在"),
        }
    } else {
        versions_dir.join(&clean_id)
    };

    if !version_dir.exists() {
        return ApiResult::err(404, "版本目录不存在");
    }

    let temp_dir = data_dir.join("temp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let file_name = format!("{}-export.zip", clean_id);
    let zip_path = temp_dir.join(&file_name);

    match create_modpack_zip(
        &version_dir,
        &clean_id,
        &pack_name,
        &pack_version,
        &author,
        &description,
        &selected_keys,
        &zip_path,
    ) {
        Ok(_) => ApiResult::ok(json!({
            "success": true,
            "path": zip_path.to_string_lossy(),
            "fileName": file_name
        })),
        Err(e) => ApiResult::err(500, &format!("打包失败: {}", e)),
    }
}

/// 生成整合包 ZIP：overrides/ 保存游戏内容，modpack.json 记录元信息
///
/// 若未提供 selectedKeys，则导出完整内容集。
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_modpack_zip(
    src_dir: &Path,
    version_id: &str,
    pack_name: &str,
    pack_version: &str,
    author: &str,
    description: &str,
    selected_keys: &[String],
    zip_path: &Path,
) -> std::io::Result<()> {
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::{Read, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;
    use zip::ZipWriter;

    // 选中 key 映射到游戏内容目录/文件（对应前端导出树 data-key）
    let mut include_dirs: HashSet<String> = HashSet::new();
    let mut include_files: HashSet<String> = HashSet::new();
    if selected_keys.is_empty() {
        // 未指定内容时使用默认全量内容
        for d in ["mods", "config", "resourcepacks", "texturepacks", "shaderpacks",
                  "saves", "screenshots", "defaultconfigs", "kubejs", "scripts",
                  "openloader", "serverconfig", "custom"] {
            include_dirs.insert(d.to_string());
        }
        for f in ["options.txt", "optionsof.txt", "servers.dat"] {
            include_files.insert(f.to_string());
        }
    } else {
        for key in selected_keys {
            match key.as_str() {
                "mods" | "mod_files" => { include_dirs.insert("mods".to_string()); }
                "mod_configs" => { include_dirs.insert("config".to_string()); }
                "resourcepacks" => {
                    include_dirs.insert("resourcepacks".to_string());
                    include_dirs.insert("texturepacks".to_string());
                }
                "shaderpacks" => { include_dirs.insert("shaderpacks".to_string()); }
                "saves" => { include_dirs.insert("saves".to_string()); }
                "screenshots" => { include_dirs.insert("screenshots".to_string()); }
                "defaultconfigs" => { include_dirs.insert("defaultconfigs".to_string()); }
                "kubejs" => { include_dirs.insert("kubejs".to_string()); }
                "game_settings" => { include_files.insert("options.txt".to_string()); }
                "servers" => { include_files.insert("servers.dat".to_string()); }
                _ => {}
            }
        }
    }

    // 系统/缓存/无用目录
    let skip_dirs: HashSet<&str> = [
        "assets", "versions", "libraries", "structureCacheV1", ".fabric", ".git",
        "avatar-cache", "cosmetic-cache", "downloads", "logs", "crash-reports",
    ].iter().cloned().collect();
    // 排除的垃圾文件
    let junk_names: HashSet<&str> = ["hmclversion.cfg", "log4j2.xml", "BakaCoreInfo"]
        .iter().cloned().collect();
    let version_json_name = format!("{}.json", version_id);
    let version_jar_name = format!("{}.jar", version_id);

    let file = File::create(zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated);

    let meta = serde_json::json!({
        "name": if pack_name.is_empty() { version_id } else { pack_name },
        "version": if pack_version.is_empty() { "1.0.0" } else { pack_version },
        "author": author,
        "description": description,
        "versionId": version_id,
        "overrides": "overrides"
    });
    zip.start_file("modpack.json", options)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    zip.write_all(serde_json::to_string_pretty(&meta).unwrap_or_default().as_bytes())?;

    // 递归遍历 src_dir，把符合规则的内容写入 overrides/ 下
    // stack 元素： (目录绝对路径, 相对 src_dir 的目录前缀)
    let mut stack: Vec<(PathBuf, String)> = vec![(src_dir.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries = std::fs::read_dir(&dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match entry.file_name().to_str() {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name == version_json_name || name == version_jar_name {
                continue;
            }
            if junk_names.contains(name.as_str()) || has_junk_ext(&name) {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let zip_name = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            let overrides_name = format!("overrides/{}", zip_name);
            if meta.is_dir() {
                if prefix.is_empty() && !include_dirs.contains(name.as_str()) {
                    continue;
                }
                if !prefix.is_empty() && skip_dirs.contains(name.as_str()) {
                    continue;
                }
                stack.push((path, zip_name));
            } else if meta.is_file() {
                if prefix.is_empty() && !include_files.contains(name.as_str()) {
                    continue;
                }
                zip.start_file(&overrides_name, options)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let mut f = File::open(&path)?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                zip.write_all(&buf)?;
            }
        }
    }

    zip.finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// 文件名是否命中常见的日志/临时垃圾后缀
pub(crate) fn has_junk_ext(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".log")
        || lower.ends_with(".dat_old")
        || lower.ends_with(".bakacoreinfo")
        || lower.ends_with(".tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_extensions_are_detected_case_insensitively() {
        assert!(has_junk_ext("latest.log"));
        assert!(has_junk_ext("level.dat_old"));
        assert!(has_junk_ext("backup.bakacoreinfo"));
        assert!(has_junk_ext("temp.TMP"));
    }

    #[test]
    fn normal_files_are_not_junk() {
        assert!(!has_junk_ext("level.dat"));
        assert!(!has_junk_ext("mod.jar"));
        assert!(!has_junk_ext("options.txt"));
    }
}
