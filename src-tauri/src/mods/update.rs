// mods/update.rs — 模组更新检查
// 职责：通过 SHA1 哈希批量查询 Modrinth，检查已安装模组是否有更新
// 对应原项目 server/mods.js 的 checkModUpdates
//
// 流程：
//   1. 读取 mods 目录下所有 .jar 文件
//   2. 计算 SHA1 哈希
//   3. POST /version_files 查询每个哈希对应的版本信息
//   4. 批量拉取项目信息补充名称
//   5. 返回 { updates, total, checked }

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

    // POST /version_files 批量查询
    let hash_list: Vec<String> = hashes.keys().cloned().collect();
    let post_body = json!({ "hashes": hash_list, "algorithm": "sha1" });
    let url = format!("{}/version_files", MODRINTH_API);

    let resp = match client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&post_body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return json!({
                "updates": [],
                "total": mod_files.len(),
                "checked": hash_list.len(),
                "error": e.to_string()
            });
        }
    };

    if !resp.status().is_success() {
        return json!({
            "updates": [],
            "total": mod_files.len(),
            "checked": hash_list.len(),
            "error": format!("HTTP {}", resp.status())
        });
    }

    let version_res: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "updates": [],
                "total": mod_files.len(),
                "checked": hash_list.len(),
                "error": e.to_string()
            });
        }
    };

    // 收集所有 projectId，批量拉取项目信息
    let version_obj = version_res.as_object().cloned().unwrap_or_default();
    let project_ids: Vec<String> = version_obj
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
        if let Ok(resp) = client.get(&projects_url).send().await {
            if resp.status().is_success() {
                if let Ok(projects) = resp.json::<Vec<Value>>().await {
                    for p in projects {
                        if let Some(id) = p.get("id").and_then(|i| i.as_str()) {
                            project_map.insert(id.to_string(), p);
                        }
                    }
                }
            }
        }
    }

    // 组装更新列表
    let mut updates: Vec<Value> = Vec::new();
    for (hash, info) in version_obj.iter() {
        let local = match hashes.get(hash) {
            Some(l) => l,
            None => continue,
        };
        let project_id = info.get("project_id").and_then(|p| p.as_str()).unwrap_or("");
        let project = project_map.get(project_id);
        let mod_name = project
            .and_then(|p| p.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or(project_id);
        updates.push(json!({
            "fileName": local.0,
            "modName": mod_name,
            "currentVersion": info.get("version_number").and_then(|v| v.as_str()).unwrap_or(""),
            "currentVersionId": info.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "projectUrl": format!("https://modrinth.com/mod/{}", project_id),
            "projectId": project_id
        }));
    }

    json!({
        "updates": updates,
        "total": mod_files.len(),
        "checked": hash_list.len()
    })
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
