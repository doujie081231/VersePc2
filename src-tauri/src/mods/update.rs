// mods/update.rs — 模组更新检查与批量更新
// 职责：通过 SHA1 哈希批量查询 Modrinth，检查已安装模组是否有更新
//
// 流程（对齐 PCL 的「检查模组更新」）：
//   1. 读取 mods 目录下所有 .jar 文件
//   2. 计算 SHA1 哈希
//   3. POST /version_files 查询当前版本；POST /version_files/update
//      携带当前加载器与游戏版本，查询真正可安装的新版本
//   4. 批量拉取项目信息补充名称
//   5. 返回 { updates, total, checked }，只包含有可安装新版本的模组

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::jar::compute_sha1_file;
use crate::storage;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

/// 检查模组更新
pub async fn check_mod_updates(version_id: &str) -> Value {
    let settings = storage::load_settings();
    let version_id = if version_id.is_empty() {
        settings
            .get("selectedVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        version_id.to_string()
    };

    let mods_dir = resolve_version_mods_dir(&settings, &version_id);
    let mods_dir = match mods_dir {
        Some(d) if d.exists() => d,
        _ => return json!({ "updates": [], "total": 0, "checked": 0 }),
    };

    // 收集 .jar 文件
    let mod_files: Vec<String> = match std::fs::read_dir(&mods_dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".jar") && !name.ends_with(".jar.disabled") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return json!({ "updates": [], "total": 0, "checked": 0 }),
    };

    if mod_files.is_empty() {
        return json!({ "updates": [], "total": 0, "checked": 0 });
    }

    // 计算哈希
    let mut hashes: HashMap<String, (String, String)> = HashMap::new(); // hash → (fileName, filePath)
    for file in &mod_files {
        let path = mods_dir.join(file);
        if let Some(hash) = compute_sha1_file(&path) {
            hashes.insert(hash, (file.clone(), path.to_string_lossy().to_string()));
        }
    }

    if hashes.is_empty() {
        return json!({ "updates": [], "total": mod_files.len(), "checked": 0 });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 当前版本信息（用于展示"当前版本"；失败不影响更新判断）
    let hash_list: Vec<String> = hashes.keys().cloned().collect();
    let current_res = post_version_files(
        &client,
        &format!("{}/version_files", MODRINTH_API),
        &json!({ "hashes": hash_list, "algorithm": "sha1" }),
    )
    .await
    .unwrap_or_else(|_| json!({}));

    // 最新可安装版本：带上当前加载器与游戏版本，避免给出装不上的更新（与 PCL 一致）
    let ctx = crate::server_host::detect_client_loader(&version_id);
    let loader = ctx.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    let mc_version = ctx.get("mcVersion").and_then(|v| v.as_str()).unwrap_or("");
    let mut update_body = json!({ "hashes": hash_list, "algorithm": "sha1" });
    if !loader.is_empty() && loader != "vanilla" {
        update_body["loaders"] = json!([loader]);
    }
    if !mc_version.is_empty() {
        update_body["game_versions"] = json!([mc_version]);
    }

    let latest_res = match post_version_files(
        &client,
        &format!("{}/version_files/update", MODRINTH_API),
        &update_body,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "updates": [],
                "total": mod_files.len(),
                "checked": hash_list.len(),
                "error": e
            });
        }
    };

    // 收集所有 projectId，批量拉取项目信息
    let latest_obj = latest_res.as_object().cloned().unwrap_or_default();
    let current_obj = current_res.as_object().cloned().unwrap_or_default();
    let project_ids: Vec<String> = latest_obj
        .values()
        .filter_map(|v| v.get("project_id").and_then(|p| p.as_str()).map(|s| s.to_string()))
        .collect::<Vec<_>>();
    let project_ids_unique: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        project_ids.into_iter().filter(|id| seen.insert(id.clone())).collect()
    };

    let mut project_map: HashMap<String, Value> = HashMap::new();
    if !project_ids_unique.is_empty() {
        let projects_url = format!(
            "{}/projects?ids={}",
            MODRINTH_API,
            urlencoding::encode(&serde_json::to_string(&project_ids_unique).unwrap_or_default())
        );
        if let Ok(resp) = client.get(&projects_url).send().await
            && resp.status().is_success()
                && let Ok(projects) = resp.json::<Vec<Value>>().await {
                    for p in projects {
                        if let Some(id) = p.get("id").and_then(|i| i.as_str()) {
                            project_map.insert(id.to_string(), p);
                        }
                    }
                }
    }

    // 组装更新列表：只保留确实存在可安装新版本的模组
    let mut updates: Vec<Value> = Vec::new();
    for (hash, latest_ver) in latest_obj.iter() {
        let local = match hashes.get(hash) {
            Some(l) => l,
            None => continue,
        };
        let latest_id = latest_ver.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let current_ver = current_obj.get(hash);
        let current_id = current_ver
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 有当前版本信息时按版本 ID 判断；否则退回文件哈希判断
        let has_update = if !current_id.is_empty() {
            current_id != latest_id
        } else {
            !version_contains_hash(latest_ver, hash)
        };
        if !has_update {
            continue;
        }

        let Some(file) = primary_file(latest_ver) else { continue };
        let download_url = file.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let new_file_name = file.get("filename").and_then(|v| v.as_str()).unwrap_or("");
        if download_url.is_empty() || new_file_name.is_empty() {
            continue;
        }

        let project_id = latest_ver.get("project_id").and_then(|p| p.as_str()).unwrap_or("");
        let project = project_map.get(project_id);
        let mod_name = project
            .and_then(|p| p.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or(project_id);
        updates.push(json!({
            "fileName": local.0,
            "modName": mod_name,
            "currentVersion": current_ver
                .and_then(|v| v.get("version_number"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "currentVersionId": current_id,
            "targetVersion": latest_ver.get("version_number").and_then(|v| v.as_str()).unwrap_or(""),
            "targetVersionId": latest_id,
            "downloadUrl": download_url,
            "newFileName": new_file_name,
            "sha1": file.get("hashes").and_then(|h| h.get("sha1")).and_then(|v| v.as_str()).unwrap_or(""),
            "size": file.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            "projectUrl": format!("https://modrinth.com/mod/{}", project_id),
            "projectId": project_id
        }));
    }
    updates.sort_by(|a, b| {
        let an = a.get("modName").and_then(|v| v.as_str()).unwrap_or("");
        let bn = b.get("modName").and_then(|v| v.as_str()).unwrap_or("");
        an.to_lowercase().cmp(&bn.to_lowercase())
    });

    json!({
        "updates": updates,
        "total": mod_files.len(),
        "checked": hash_list.len()
    })
}

/// POST 批量查询 Modrinth 版本接口
async fn post_version_files(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Result<Value, String> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

/// 版本的所有文件中是否包含指定 SHA1（用于识别"本地已是最新"）
fn version_contains_hash(version: &Value, sha1: &str) -> bool {
    version
        .get("files")
        .and_then(|v| v.as_array())
        .map(|files| {
            files.iter().any(|f| {
                f.get("hashes")
                    .and_then(|h| h.get("sha1"))
                    .and_then(|v| v.as_str())
                    == Some(sha1)
            })
        })
        .unwrap_or(false)
}

/// 取版本的主文件（primary 优先，否则第一个）
fn primary_file(version: &Value) -> Option<&Value> {
    let files = version.get("files")?.as_array()?;
    files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
}

/// 批量更新模组：下载新文件、备份旧文件后替换（对齐 PCL 的批量更新）
///
/// `items` 为 `check_mod_updates` 返回的更新项。旧文件移动到
/// `mods/.backup/<时间戳>/`，便于回滚。
pub async fn apply_mod_updates(version_id: &str, items: &[Value]) -> Value {
    let settings = storage::load_settings();
    let mods_dir = match resolve_version_mods_dir(&settings, version_id) {
        Some(d) => d,
        None => {
            return json!({
                "success": false,
                "updated": 0,
                "failed": items.len(),
                "failedFiles": [],
                "error": "无法确定模组目录"
            });
        }
    };
    if let Err(e) = std::fs::create_dir_all(&mods_dir) {
        return json!({
            "success": false,
            "updated": 0,
            "failed": items.len(),
            "failedFiles": [],
            "error": format!("创建目录失败: {}", e)
        });
    }

    let backup_dir = mods_dir
        .join(".backup")
        .join(chrono::Local::now().format("%Y%m%d-%H%M%S").to_string());
    let mut updated = 0usize;
    let mut failed_files: Vec<Value> = Vec::new();

    for item in items {
        let file_name = item.get("fileName").and_then(|v| v.as_str()).unwrap_or("");
        let download_url = item.get("downloadUrl").and_then(|v| v.as_str()).unwrap_or("");
        let new_file_name = item
            .get("newFileName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(file_name);
        let sha1 = item
            .get("sha1")
            .and_then(|v| v.as_str())
            .filter(|s| s.len() == 40);
        let size = item
            .get("size")
            .and_then(|v| v.as_u64())
            .filter(|s| *s > 0);

        if !is_safe_file_name(file_name)
            || !is_safe_file_name(new_file_name)
            || download_url.is_empty()
        {
            failed_files.push(json!({ "fileName": file_name, "error": "参数无效" }));
            continue;
        }

        let part_path = mods_dir.join(format!(".{}.part", new_file_name));
        let download = crate::download::download_with_mirror_retry(
            download_url,
            &part_path,
            sha1,
            size,
            "china-first",
            300,
        )
        .await;
        if let Err(e) = download {
            let _ = std::fs::remove_file(&part_path);
            failed_files.push(json!({ "fileName": file_name, "error": e }));
            continue;
        }

        // 旧文件移入备份目录（PCL 放入回收站，这里保留在 mods/.backup 下）
        let old_path = mods_dir.join(file_name);
        if old_path.exists() {
            let _ = std::fs::create_dir_all(&backup_dir);
            let _ = std::fs::rename(&old_path, backup_dir.join(file_name));
        }

        let dest = mods_dir.join(new_file_name);
        if dest.exists() {
            let _ = std::fs::remove_file(&dest);
        }
        match std::fs::rename(&part_path, &dest) {
            Ok(()) => updated += 1,
            Err(e) => {
                let _ = std::fs::remove_file(&part_path);
                failed_files.push(json!({
                    "fileName": file_name,
                    "error": format!("替换文件失败: {}", e)
                }));
            }
        }
    }

    json!({
        "success": failed_files.is_empty(),
        "updated": updated,
        "failed": failed_files.len(),
        "failedFiles": failed_files,
        "backupDir": backup_dir.to_string_lossy(),
    })
}

/// 校验文件名是否安全（禁止目录穿越与非法分隔符）
fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
}

/// 解析版本 mods 目录
fn resolve_version_mods_dir(settings: &Value, version_id: &str) -> Option<PathBuf> {
    if version_id.is_empty() {
        return None;
    }
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if version_isolation {
        Some(versions_dir.join(version_id).join("mods"))
    } else {
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.clone());
        Some(game_dir.join("mods"))
    }
}

/// 通过 Modrinth 按文件 SHA1 批量解析本地产物的网络项目图标地址。
///
/// 每个已安装的 .jar 文件计算 SHA1 后，POST /version_files 查询其对应的
/// 线上版本，再拉取项目详情中的 icon_url，得到该产物在模组平台上的
/// 官方图标（源站 CDN 地址，已替换为国内镜像）。图标地址供前端直接渲染，
/// 加载失败时由前端回退到本地默认占位图。
///
/// 返回：jar 文件路径（Windows 反斜杠形式）→ icon_url 映射。
pub async fn resolve_project_icons(jar_paths: &[PathBuf]) -> HashMap<String, String> {
    let mut result: HashMap<String, String> = HashMap::new();
    if jar_paths.is_empty() {
        return result;
    }

    // 计算每个 jar 的 SHA1，建立 sha1 → 绝对路径 的映射
    let mut hash_to_path: HashMap<String, String> = HashMap::new();
    for p in jar_paths {
        let key = p.to_string_lossy().replace('/', "\\");
        if let Some(hash) = compute_sha1_file(p) {
            hash_to_path.insert(hash, key);
        }
    }
    if hash_to_path.is_empty() {
        return result;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 按 SHA1 批量查询对应线上版本
    let hash_list: Vec<String> = hash_to_path.keys().cloned().collect();
    let post_body = json!({ "hashes": hash_list, "algorithm": "sha1" });
    let version_res: Value = match client
        .post(format!("{}/version_files", MODRINTH_API))
        .header("Content-Type", "application/json")
        .json(&post_body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(v) => v,
            Err(_) => return result,
        },
        _ => return result,
    };

    // 收集涉及的项目 id 并批量拉取项目信息
    let version_obj = version_res.as_object().cloned().unwrap_or_default();
    let mut project_ids: Vec<String> = version_obj
        .values()
        .filter_map(|v| v.get("project_id").and_then(|p| p.as_str()).map(|s| s.to_string()))
        .collect();
    project_ids.sort();
    project_ids.dedup();
    if project_ids.is_empty() {
        return result;
    }

    let projects_url = format!(
        "{}/projects?ids={}",
        MODRINTH_API,
        urlencoding::encode(&serde_json::to_string(&project_ids).unwrap_or_default())
    );
    let mut icon_by_project: HashMap<String, String> = HashMap::new();
    if let Ok(resp) = client.get(&projects_url).send().await
        && resp.status().is_success()
            && let Ok(projects) = resp.json::<Vec<Value>>().await {
                for p in projects {
                    let id = p.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let icon = p.get("icon_url").and_then(|i| i.as_str()).unwrap_or("");
                    if !id.is_empty() && !icon.is_empty() {
                        icon_by_project.insert(id.to_string(), icon.to_string());
                    }
                }
            }

    // 组装：sha1 → 项目图标
    for (hash, path) in hash_to_path.iter() {
        if let Some(ver) = version_obj.get(hash) {
            let pid = ver.get("project_id").and_then(|p| p.as_str()).unwrap_or("");
            if let Some(icon) = icon_by_project.get(pid) {
                result.insert(path.clone(), to_modrinth_cdn_mirror(icon));
            }
        }
    }
    result
}

/// 将 Modrinth 官方 CDN 图片地址替换为国内镜像，避免部分网络下连接失败。
fn to_modrinth_cdn_mirror(url: &str) -> String {
    if url.contains("cdn.modrinth.com") {
        url.replace("cdn.modrinth.com", "mod.mcimirror.top")
    } else if url.contains("cdn-alt.modrinth.com") {
        url.replace("cdn-alt.modrinth.com", "mod.mcimirror.top")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_contains_hash_matches_any_file() {
        let version = json!({
            "files": [
                { "hashes": { "sha1": "aaa" } },
                { "hashes": { "sha1": "bbb" } }
            ]
        });
        assert!(version_contains_hash(&version, "bbb"));
        assert!(!version_contains_hash(&version, "ccc"));
        assert!(!version_contains_hash(&json!({}), "aaa"));
    }

    #[test]
    fn primary_file_prefers_primary_flag() {
        let version = json!({
            "files": [
                { "filename": "a.jar", "primary": false },
                { "filename": "b.jar", "primary": true }
            ]
        });
        assert_eq!(primary_file(&version).unwrap()["filename"], "b.jar");
        let no_flag = json!({ "files": [{ "filename": "only.jar" }] });
        assert_eq!(primary_file(&no_flag).unwrap()["filename"], "only.jar");
        assert!(primary_file(&json!({})).is_none());
    }

    #[test]
    fn safe_file_name_rejects_traversal() {
        assert!(is_safe_file_name("mod-1.0.jar"));
        assert!(!is_safe_file_name(""));
        assert!(!is_safe_file_name("../evil.jar"));
        assert!(!is_safe_file_name("dir/evil.jar"));
        assert!(!is_safe_file_name("dir\\evil.jar"));
    }
}
