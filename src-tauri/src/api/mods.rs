// api/mods.rs — 模组管理路由
// 职责：模组列表、搜索、详情、版本、下载、管理、依赖解析、对话框
// 对应原项目 server/api/routes/mods/*.js
//
// 路由清单（25 个）：
//   列表（mod-list.js，6 个）：
//     GET  /api/mods                    已安装模组列表（含重复/冲突检测）
//     GET  /api/mod-icon                模组图标（按 hash 从缓存读取）
//     GET  /api/mods/open-save-folder    打开存档文件夹
//     GET  /api/mods/installed           已安装模组（简化版）
//     GET  /api/mods/categories          模组分类列表
//     GET  /api/mods/featured           推荐模组
//   搜索（mod-search.js，1 个）：
//     GET  /api/mods/search              Modrinth + CurseForge 双源搜索
//   详情（mod-detail.js，3 个）：
//     *    /api/mods/project-versions    项目版本列表（兼容旧路径）
//     GET  /api/mods/detail             模组详情
//     GET  /api/mods/versions           模组版本列表
//   下载（mod-download.js，3 个）：
//     POST /api/mods/download           下载模组
//     POST /api/mods/download-version   下载指定版本
//     GET  /api/mods/download-status    下载状态查询
//   依赖（mod-dependencies.js，4 个）：
//     POST /api/mods/get-dependencies    获取依赖
//     POST /api/mods/get-dependencies-recursive  递归获取依赖
//     GET  /api/mods/resolve-deps       解析依赖
//     POST /api/mods/resolve-deps-versions  解析依赖版本
//   管理（mod-manage.js，5 个）：
//     POST /api/mods/toggle             启用/禁用
//     POST /api/mods/delete             删除（模糊匹配）
//     POST /api/mods/check-updates       检查更新
//     POST /api/mods/install-from-file  从文件安装
//     POST /api/mods/remove              删除指定文件
//   对话框（mod-dialog.js，3 个）：
//     GET  /api/mods/select-modpack-file 选择整合包文件
//     GET  /api/mods/select-file         选择模组文件
//     POST /api/mods/select-save-folder  选择保存文件夹

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::download::{download_with_mirror, resources_session};
use crate::mods;
use crate::storage;

/// Modrinth API
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// CurseForge API
const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
/// CurseForge 默认 API Key
const DEFAULT_CF_API_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

/// 处理模组路由
pub async fn handle(
    _app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 列表 =====
        "GET /api/mods" => Some(handle_list()),
        "GET /api/mod-icon" => Some(handle_icon(params)),
        "GET /api/mods/open-save-folder" => Some(handle_open_save_folder(params)),
        "GET /api/mods/installed" => Some(handle_installed()),
        "GET /api/mods/categories" => Some(handle_categories()),
        "GET /api/mods/featured" => Some(handle_featured().await),
        // ===== 搜索 =====
        "GET /api/mods/search" => Some(handle_search(params).await),
        // ===== 详情 =====
        "GET /api/mods/project-versions" => Some(handle_project_versions(params).await),
        "GET /api/mods/detail" => Some(handle_detail(params).await),
        "GET /api/mods/versions" => Some(handle_versions(params).await),
        // ===== 下载 =====
        "POST /api/mods/download" => Some(handle_download(body).await),
        "POST /api/mods/download-version" => Some(handle_download_version(body).await),
        "GET /api/mods/download-status" => Some(handle_download_status(params)),
        // ===== 依赖 =====
        "POST /api/mods/get-dependencies" => Some(handle_get_dependencies(body).await),
        "POST /api/mods/get-dependencies-recursive" => Some(handle_get_dependencies_recursive(body).await),
        "GET /api/mods/resolve-deps" => Some(handle_resolve_deps(params).await),
        "POST /api/mods/resolve-deps-versions" => Some(handle_resolve_deps_versions(body).await),
        // ===== 管理 =====
        "POST /api/mods/toggle" => Some(handle_toggle(body)),
        "POST /api/mods/delete" => Some(handle_delete(body)),
        "POST /api/mods/check-updates" => Some(handle_check_updates(body).await),
        "POST /api/mods/install-from-file" => Some(handle_install_from_file(body)),
        "POST /api/mods/remove" => Some(handle_remove(body)),
        // ===== 对话框 =====
        "GET /api/mods/select-modpack-file" => Some(handle_select_modpack_file().await),
        "GET /api/mods/select-file" => Some(handle_select_file().await),
        "POST /api/mods/select-save-folder" => Some(handle_select_save_folder(body).await),
        _ => None,
    }
}

// ============== 列表路由 ==============

/// GET /api/mods — 已安装模组列表
fn handle_list() -> ApiResult {
    ApiResult::ok(mods::get_installed_mods())
}

/// GET /api/mod-icon — 模组图标
fn handle_icon(params: &Option<Value>) -> ApiResult {
    let hash = params
        .as_ref()
        .and_then(|p| p.get("hash"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if hash.is_empty() {
        return ApiResult::err(400, "Missing hash");
    }

    let icon_path = storage::resolve_data_dir()
        .join("icon-cache")
        .join(format!("{}.png", hash));

    if !icon_path.exists() {
        return ApiResult::err(404, "图标不存在");
    }

    match std::fs::read(&icon_path) {
        Ok(data) => {
            // 返回 base64 data URL
            let data_url = crate::utils::bytes_to_data_url(&data, "image/png");
            ApiResult::ok(json!({ "dataUrl": data_url }))
        }
        Err(e) => ApiResult::err(500, &format!("读取图标失败: {}", e)),
    }
}

/// GET /api/mods/open-save-folder — 打开存档文件夹
fn handle_open_save_folder(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let saves_dir = mods::resolve_saves_dir(version_id);
    if !saves_dir.exists() {
        let _ = std::fs::create_dir_all(&saves_dir);
    }
    let _ = open::that(&saves_dir);
    ApiResult::ok(json!({ "success": true, "path": saves_dir.to_string_lossy() }))
}

/// GET /api/mods/installed — 已安装模组（简化版，返回 mods 数组）
fn handle_installed() -> ApiResult {
    let result = mods::get_installed_mods();
    let mods_arr = result.get("mods").cloned().unwrap_or(json!([]));
    ApiResult::ok(json!({ "mods": mods_arr }))
}

/// GET /api/mods/categories — 模组分类列表
fn handle_categories() -> ApiResult {
    // 简化：返回常用分类
    let categories = vec![
        "performance", "optimization", "utility", "storage",
        "technology", "magic", "adventure", "decoration",
        "worldgen", "mobs", "food", "transportation",
        "armor", "weapons", "tools", "redstone",
    ];
    let arr: Vec<Value> = categories
        .iter()
        .map(|c| json!({ "name": c, "id": c }))
        .collect();
    ApiResult::ok(json!({ "categories": arr }))
}

/// GET /api/mods/featured — 推荐模组
async fn handle_featured() -> ApiResult {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // Modrinth 推荐模组（按下载量排序，取前 15）
    let url = format!(
        "{}/search?query=&index=downloads&limit=15&facets={}",
        MODRINTH_API,
        urlencoding::encode(r#"[["project_type:mod"]]"#)
    );

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<Value>().await {
                Ok(result) => {
                    let hits = result
                        .get("hits")
                        .and_then(|h| h.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|hit| {
                                    json!({
                                        "id": hit.get("project_id").and_then(|v| v.as_str()).unwrap_or(""),
                                        "slug": hit.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                                        "title": hit.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                                        "description": hit.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                                        "icon": hit.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                                        "downloads": hit.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                                        "source": "modrinth"
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    ApiResult::ok(json!({ "hits": hits, "total": hits.len() }))
                }
                Err(_) => ApiResult::ok(json!({ "hits": [], "total": 0 })),
            }
        }
        _ => ApiResult::ok(json!({ "hits": [], "total": 0 })),
    }
}

// ============== 搜索路由 ==============

/// GET /api/mods/search — Modrinth + CurseForge 双源搜索
async fn handle_search(params: &Option<Value>) -> ApiResult {
    let query = params
        .as_ref()
        .and_then(|p| p.get("query"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = params
        .as_ref()
        .and_then(|p| p.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("any")
        .to_string();
    let loader = params
        .as_ref()
        .and_then(|p| p.get("loader"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mc_version = params
        .as_ref()
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let category = params
        .as_ref()
        .and_then(|p| p.get("category"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sort = params
        .as_ref()
        .and_then(|p| p.get("sort"))
        .and_then(|v| v.as_str())
        .unwrap_or("relevance")
        .to_string();
    let limit = params
        .as_ref()
        .and_then(|p| p.get("limit"))
        .and_then(|v| v.as_u64())
        .unwrap_or(15) as usize;
    let offset = params
        .as_ref()
        .and_then(|p| p.get("offset"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut all_hits: Vec<Value> = Vec::new();

    // Modrinth 源
    if source == "any" || source == "modrinth" {
        if let Ok(hits) = search_modrinth_mods(&client, &query, &loader, &mc_version, &category, &sort, limit, offset).await {
            all_hits.extend(hits);
        }
    }

    // CurseForge 源
    if source == "any" || source == "curseforge" {
        if let Ok(hits) = search_curseforge_mods(&client, &query, &loader, &mc_version, &sort, limit, offset).await {
            all_hits.extend(hits);
        }
    }

    // 双源混合时按下载量排序
    if source == "any" && all_hits.len() > 1 {
        all_hits.sort_by(|a, b| {
            let da = a.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            let db = b.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            db.cmp(&da)
        });
        all_hits.truncate(limit);
    }

    ApiResult::ok(json!({
        "hits": all_hits,
        "total": all_hits.len(),
        "offset": offset
    }))
}

/// Modrinth 搜索模组
async fn search_modrinth_mods(
    client: &reqwest::Client,
    query: &str,
    loader: &str,
    mc_version: &str,
    category: &str,
    sort: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, String> {
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:mod".to_string()]];
    if !loader.is_empty() {
        facets.push(vec![format!("categories:{}", loader)]);
    }
    if !mc_version.is_empty() {
        facets.push(vec![format!("versions:{}", mc_version)]);
    }
    if !category.is_empty() {
        facets.push(vec![format!("categories:{}", category)]);
    }
    let facets_json = serde_json::to_string(&facets).unwrap_or_default();

    let sort_field = match sort {
        "downloads" => "downloads",
        "newest" => "newest",
        "updated" => "updated",
        "follows" => "follows",
        _ => "relevance",
    };

    let url = format!(
        "{}/search?query={}&index={}&limit={}&offset={}&facets={}",
        MODRINTH_API,
        urlencoding::encode(query),
        sort_field,
        limit,
        offset,
        urlencoding::encode(&facets_json)
    );

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let result: Value = resp.json().await.map_err(|e| e.to_string())?;
    let hits = result
        .get("hits")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .map(|hit| {
                    json!({
                        "id": hit.get("project_id").and_then(|v| v.as_str()).unwrap_or(""),
                        "slug": hit.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                        "title": hit.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": hit.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "author": hit.get("author").and_then(|v| v.as_str()).unwrap_or("").replace('_', ""),
                        "icon": hit.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                        "downloads": hit.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                        "followers": hit.get("followers").and_then(|v| v.as_u64()).unwrap_or(0),
                        "categories": hit.get("categories").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                        "versions": hit.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                        "dateCreated": hit.get("date_created").and_then(|v| v.as_str()).unwrap_or(""),
                        "dateModified": hit.get("date_modified").and_then(|v| v.as_str()).unwrap_or(""),
                        "source": "modrinth",
                        "installed": false
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

/// CurseForge 搜索模组
async fn search_curseforge_mods(
    client: &reqwest::Client,
    query: &str,
    loader: &str,
    mc_version: &str,
    sort: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, String> {
    let settings = storage::load_settings();
    let cf_api_key = settings
        .get("curseforgeApiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CF_API_KEY);

    let sort_field = match sort {
        "downloads" => "6",
        "newest" => "11",
        "updated" => "3",
        _ => "2",
    };

    let mut url = format!(
        "{}/mods/search?gameId=432&searchFilter={}&sortOrder=Desc&classId=6&pageSize={}&index={}&sortField={}",
        CURSEFORGE_API,
        urlencoding::encode(query),
        limit,
        offset,
        sort_field
    );

    if !loader.is_empty() {
        let loader_id = match loader.to_lowercase().as_str() {
            "forge" => "1",
            "fabric" => "4",
            "quilt" | "neoforge" => "5",
            _ => "",
        };
        if !loader_id.is_empty() {
            url.push_str(&format!("&modLoaderType={}", loader_id));
        }
    }

    if !mc_version.is_empty() {
        url.push_str(&format!("&gameVersion={}", urlencoding::encode(mc_version)));
    }

    let resp = client
        .get(&url)
        .header("x-api-key", cf_api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let result: Value = resp.json().await.map_err(|e| e.to_string())?;
    let hits = result
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .map(|m| {
                    let author = m
                        .get("authors")
                        .and_then(|a| a.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|a| a.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown");
                    let categories = m
                        .get("categories")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|c| {
                                    c.get("name")
                                        .and_then(|n| n.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| c.get("id").map(|i| i.to_string()).unwrap_or_default())
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    json!({
                        "id": m.get("id").map(|i| i.to_string()).unwrap_or_default(),
                        "slug": m.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                        "title": m.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "description": m.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                        "author": author,
                        "icon": m.get("logo").and_then(|l| l.get("url")).and_then(|u| u.as_str()).unwrap_or(""),
                        "downloads": m.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
                        "followers": m.get("followers").and_then(|v| v.as_u64()).unwrap_or(0),
                        "categories": categories,
                        "versions": [],
                        "dateCreated": m.get("dateCreated").and_then(|v| v.as_str()).unwrap_or(""),
                        "dateModified": m.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
                        "source": "curseforge",
                        "installed": false
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

// ============== 详情路由 ==============

/// GET /api/mods/project-versions — 项目版本列表（兼容旧路径）
async fn handle_project_versions(params: &Option<Value>) -> ApiResult {
    handle_versions(params).await
}

/// GET /api/mods/detail — 模组详情
async fn handle_detail(params: &Option<Value>) -> ApiResult {
    let project_id = params
        .as_ref()
        .and_then(|p| p.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let source = params
        .as_ref()
        .and_then(|p| p.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth");

    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if source == "modrinth" {
        let url = format!("{}/project/{}", MODRINTH_API, project_id);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(project) => {
                        let gallery = project
                            .get("gallery")
                            .and_then(|g| g.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .map(|g| {
                                        if let Some(s) = g.as_str() {
                                            s.to_string()
                                        } else {
                                            g.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let detail = json!({
                            "id": project.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "slug": project.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                            "title": project.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                            "description": project.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                            "body": project.get("body").and_then(|v| v.as_str()).unwrap_or(""),
                            "icon": project.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                            "downloads": project.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                            "followers": project.get("followers").and_then(|v| v.as_u64()).unwrap_or(0),
                            "categories": project.get("categories").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                            "loaders": project.get("loaders").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                            "gameVersions": project.get("game_versions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                            "clientSide": project.get("client_side").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            "serverSide": project.get("server_side").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            "license": project.get("license").and_then(|l| l.get("name")).and_then(|n| n.as_str()).unwrap_or(""),
                            "sourceUrl": project.get("source_url").and_then(|v| v.as_str()).unwrap_or(""),
                            "issuesUrl": project.get("issues_url").and_then(|v| v.as_str()).unwrap_or(""),
                            "wikiUrl": project.get("wiki_url").and_then(|v| v.as_str()).unwrap_or(""),
                            "discordUrl": project.get("discord_url").and_then(|v| v.as_str()).unwrap_or(""),
                            "dateCreated": project.get("published").and_then(|v| v.as_str()).unwrap_or(""),
                            "dateModified": project.get("updated").and_then(|v| v.as_str()).unwrap_or(""),
                            "gallery": gallery,
                            "source": "modrinth"
                        });
                        ApiResult::ok(detail)
                    }
                    Err(e) => ApiResult::err(502, &format!("解析失败: {}", e)),
                }
            }
            Ok(resp) => ApiResult::err(502, &format!("Modrinth HTTP {}", resp.status())),
            Err(e) => ApiResult::err(502, &format!("请求失败: {}", e)),
        }
    } else if source == "curseforge" {
        let settings = storage::load_settings();
        let cf_api_key = settings
            .get("curseforgeApiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CF_API_KEY);
        let url = format!("{}/mods/{}", CURSEFORGE_API, project_id);
        match client.get(&url).header("x-api-key", cf_api_key).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(result) => {
                        let m = result.get("data").unwrap_or(&result);
                        let categories = m
                            .get("categories")
                            .and_then(|c| c.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .map(|c| {
                                        if let Some(s) = c.as_str() {
                                            s.to_string()
                                        } else {
                                            c.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let loaders = m
                            .get("latestFilesIndexes")
                            .and_then(|l| l.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|f| {
                                        f.get("modLoader").and_then(|ml| ml.as_u64()).and_then(|id| match id {
                                            1 => Some("forge"),
                                            4 => Some("fabric"),
                                            5 => Some("neoforge"),
                                            _ => None,
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let game_versions = m
                            .get("latestFilesIndexes")
                            .and_then(|l| l.as_array())
                            .map(|arr| {
                                let mut set = std::collections::HashSet::new();
                                for f in arr {
                                    if let Some(v) = f.get("gameVersion").and_then(|g| g.as_str()) {
                                        set.insert(v.to_string());
                                    }
                                }
                                set.into_iter().collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let detail = json!({
                            "id": m.get("id").map(|i| i.to_string()).unwrap_or_default(),
                            "slug": m.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                            "title": m.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                            "description": m.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                            "body": m.get("description").and_then(|v| v.as_str()).unwrap_or(m.get("summary").and_then(|v| v.as_str()).unwrap_or("")),
                            "icon": m.get("logo").and_then(|l| l.get("url")).and_then(|u| u.as_str()).unwrap_or(""),
                            "downloads": m.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
                            "followers": m.get("followers").and_then(|v| v.as_u64()).unwrap_or(0),
                            "categories": categories,
                            "loaders": loaders,
                            "gameVersions": game_versions,
                            "clientSide": "unknown",
                            "serverSide": "unknown",
                            "license": "",
                            "sourceUrl": m.get("links").and_then(|l| l.get("sourceUrl")).and_then(|u| u.as_str()).unwrap_or(""),
                            "issuesUrl": m.get("links").and_then(|l| l.get("issuesUrl")).and_then(|u| u.as_str()).unwrap_or(""),
                            "wikiUrl": m.get("links").and_then(|l| l.get("wikiUrl")).and_then(|u| u.as_str()).unwrap_or(""),
                            "discordUrl": "",
                            "dateCreated": m.get("dateCreated").and_then(|v| v.as_str()).unwrap_or(""),
                            "dateModified": m.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
                            "gallery": m.get("screenshots").and_then(|s| s.as_array()).map(|arr| arr.iter().map(|s| s.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string()).collect::<Vec<_>>()).unwrap_or_default(),
                            "source": "curseforge"
                        });
                        ApiResult::ok(detail)
                    }
                    Err(e) => ApiResult::err(502, &format!("解析失败: {}", e)),
                }
            }
            Ok(resp) => ApiResult::err(502, &format!("CurseForge HTTP {}", resp.status())),
            Err(e) => ApiResult::err(502, &format!("请求失败: {}", e)),
        }
    } else {
        ApiResult::err(400, "Unsupported source")
    }
}

/// GET /api/mods/versions — 模组版本列表
async fn handle_versions(params: &Option<Value>) -> ApiResult {
    let project_id = params
        .as_ref()
        .and_then(|p| p.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let source = params
        .as_ref()
        .and_then(|p| p.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth");
    let loader = params
        .as_ref()
        .and_then(|p| p.get("loader"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let game_ver = params
        .as_ref()
        .and_then(|p| p.get("gameVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    if source == "modrinth" {
        let mut url = format!("{}/project/{}/version", MODRINTH_API, project_id);
        let mut params_list: Vec<String> = Vec::new();
        if !loader.is_empty() {
            params_list.push(format!("loaders=[\"{}\"]", loader));
        }
        if !game_ver.is_empty() {
            params_list.push(format!("game_versions=[\"{}\"]", game_ver));
        }
        if !params_list.is_empty() {
            url.push('?');
            url.push_str(&params_list.join("&"));
        }

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Vec<Value>>().await {
                    Ok(versions_raw) => {
                        let versions: Vec<Value> = versions_raw
                            .iter()
                            .map(|v| {
                                let files = v.get("files").and_then(|f| f.as_array()).map(|arr| {
                                    arr.iter()
                                        .map(|f| {
                                            json!({
                                                "url": f.get("url").and_then(|u| u.as_str()).unwrap_or(""),
                                                "filename": f.get("filename").and_then(|n| n.as_str()).unwrap_or(""),
                                                "primary": f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false),
                                                "size": f.get("size").and_then(|s| s.as_u64()).unwrap_or(0)
                                            })
                                        })
                                        .collect::<Vec<_>>()
                                }).unwrap_or_default();
                                json!({
                                    "id": v.get("id").and_then(|i| i.as_str()).unwrap_or(""),
                                    "name": v.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                                    "versionNumber": v.get("version_number").and_then(|n| n.as_str()).unwrap_or(""),
                                    "gameVersions": v.get("game_versions").and_then(|g| g.as_array()).cloned().unwrap_or_default(),
                                    "loaders": v.get("loaders").and_then(|l| l.as_array()).cloned().unwrap_or_default(),
                                    "releaseType": v.get("version_type").and_then(|t| t.as_str()).unwrap_or("release"),
                                    "datePublished": v.get("date_published").and_then(|d| d.as_str()).unwrap_or(""),
                                    "downloads": v.get("downloads").and_then(|d| d.as_u64()).unwrap_or(0),
                                    "changelog": v.get("changelog").and_then(|c| c.as_str()).unwrap_or(""),
                                    "files": files
                                })
                            })
                            .collect();
                        ApiResult::ok(json!({ "versions": versions }))
                    }
                    Err(e) => ApiResult::err(502, &format!("解析失败: {}", e)),
                }
            }
            Ok(resp) => ApiResult::err(502, &format!("Modrinth HTTP {}", resp.status())),
            Err(e) => ApiResult::err(502, &format!("请求失败: {}", e)),
        }
    } else if source == "curseforge" {
        let settings = storage::load_settings();
        let cf_api_key = settings
            .get("curseforgeApiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CF_API_KEY);

        // CurseForge 版本列表接口
        let mut url = format!("{}/mods/{}/files?pageSize=50", CURSEFORGE_API, project_id);
        if !game_ver.is_empty() {
            url.push_str(&format!("&gameVersion={}", urlencoding::encode(&game_ver)));
        }
        if !loader.is_empty() {
            let loader_id = match loader.to_lowercase().as_str() {
                "forge" => "1",
                "fabric" => "4",
                "neoforge" => "6",
                "quilt" => "5",
                _ => "",
            };
            if !loader_id.is_empty() {
                url.push_str(&format!("&modLoaderType={}", loader_id));
            }
        }

        match client.get(&url).header("x-api-key", cf_api_key).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<Value>().await {
                    Ok(result) => {
                        let files = result
                            .get("data")
                            .and_then(|d| d.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let versions: Vec<Value> = files
                            .iter()
                            .map(|f| {
                                let loaders = f
                                    .get("modLoaders")
                                    .and_then(|l| l.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|ml| {
                                                ml.get("modLoader").and_then(|id| id.as_u64()).and_then(|id| match id {
                                                    1 => Some("forge"),
                                                    4 => Some("fabric"),
                                                    5 => Some("neoforge"),
                                                    _ => None,
                                                })
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                let files_arr: Vec<Value> = f
                                    .get("fileName")
                                    .and_then(|n| n.as_str())
                                    .map(|name| {
                                        vec![json!({
                                            "url": f.get("downloadUrl").and_then(|u| u.as_str()).unwrap_or(""),
                                            "filename": name,
                                            "primary": true,
                                            "size": f.get("fileLength").and_then(|s| s.as_u64()).unwrap_or(0)
                                        })]
                                    })
                                    .unwrap_or_default();
                                let release_type = match f.get("releaseType").and_then(|t| t.as_u64()).unwrap_or(1) {
                                    2 => "beta",
                                    3 => "alpha",
                                    _ => "release",
                                };
                                json!({
                                    "id": f.get("id").map(|i| i.to_string()).unwrap_or_default(),
                                    "name": f.get("displayName").and_then(|n| n.as_str()).or_else(|| f.get("fileName").and_then(|n| n.as_str())).unwrap_or(""),
                                    "versionNumber": f.get("fileName").and_then(|n| n.as_str()).unwrap_or(""),
                                    "gameVersions": f.get("gameVersions").and_then(|g| g.as_array()).cloned().unwrap_or_default(),
                                    "loaders": loaders,
                                    "releaseType": release_type,
                                    "datePublished": f.get("fileDate").and_then(|d| d.as_str()).unwrap_or(""),
                                    "downloads": f.get("downloadCount").and_then(|d| d.as_u64()).unwrap_or(0),
                                    "changelog": f.get("changelog").and_then(|c| c.as_str()).unwrap_or(""),
                                    "files": files_arr
                                })
                            })
                            .collect();
                        ApiResult::ok(json!({ "versions": versions }))
                    }
                    Err(e) => ApiResult::err(502, &format!("解析失败: {}", e)),
                }
            }
            Ok(resp) => ApiResult::err(502, &format!("CurseForge HTTP {}", resp.status())),
            Err(e) => ApiResult::err(502, &format!("请求失败: {}", e)),
        }
    } else {
        ApiResult::err(400, "Unsupported source")
    }
}

// ============== 下载路由 ==============

/// POST /api/mods/download — 下载模组
async fn handle_download(body: &Option<Value>) -> ApiResult {
    let project_id = body
        .as_ref()
        .and_then(|b| b.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = body
        .as_ref()
        .and_then(|b| b.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth")
        .to_string();
    let loader = body
        .as_ref()
        .and_then(|b| b.get("loader"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mc_version = body
        .as_ref()
        .and_then(|b| b.get("mcVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }

    // 确定目标 mods 目录
    let settings = storage::load_settings();
    let mods_dir = resolve_mods_dir(&settings, &version_id, &mc_version);
    let mods_dir = match mods_dir {
        Some(d) => d,
        None => return ApiResult::err(400, "请先安装一个游戏版本"),
    };

    if let Err(e) = std::fs::create_dir_all(&mods_dir) {
        return ApiResult::err(500, &format!("创建 mods 目录失败: {}", e));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 拉取版本信息
    let (download_url, file_name, file_size, expected_sha1) = if source == "modrinth" {
        let version_url = format!("{}/project/{}/version", MODRINTH_API, project_id);

        // 三级查询策略
        let versions: Vec<Value> = if !version_id.is_empty() {
            // 指定版本 ID
            let url = format!("{}/version/{}", MODRINTH_API, version_id);
            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
                _ => Vec::new(),
            }
        } else if !loader.is_empty() && !mc_version.is_empty() {
            let url = format!("{}?loaders=[\"{}\"]&game_versions=[\"{}\"]", version_url, loader, mc_version);
            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
                _ => Vec::new(),
            }
        } else if !mc_version.is_empty() {
            let url = format!("{}?game_versions=[\"{}\"]", version_url, mc_version);
            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
                _ => Vec::new(),
            }
        } else {
            let url = format!("{}?limit=10", version_url);
            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
                _ => Vec::new(),
            }
        };

        if versions.is_empty() {
            return ApiResult::err(502, "未找到可用版本");
        }

        let first = &versions[0];
        let files = first.get("files").and_then(|f| f.as_array());
        let primary_file = files
            .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false)))
            .or_else(|| files.and_then(|arr| arr.first()));

        if let Some(f) = primary_file {
            (
                f.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                f.get("filename").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                f.get("hashes").and_then(|h| h.get("sha1")).and_then(|s| s.as_str()).unwrap_or("").to_string(),
            )
        } else {
            return ApiResult::err(502, "未找到下载文件");
        }
    } else {
        // CurseForge：通过 fileId 获取文件下载信息
        let settings = storage::load_settings();
        let cf_api_key = settings
            .get("curseforgeApiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CF_API_KEY);

        // version_id 即 CurseForge 的 file id
        let file_id = if !version_id.is_empty() {
            version_id.clone()
        } else {
            // 未指定 file id 时，查最新文件
            let files_url = format!("{}/mods/{}/files?pageSize=1", CURSEFORGE_API, project_id);
            let list_resp = client.get(&files_url).header("x-api-key", cf_api_key.clone()).send().await;
            match list_resp {
                Ok(r) if r.status().is_success() => {
                    let v: Value = r.json().await.unwrap_or_default();
                    v.pointer("/data/0/id").and_then(|i| i.as_u64()).map(|i| i.to_string()).unwrap_or_default()
                }
                _ => String::new(),
            }
        };

        if file_id.is_empty() {
            return ApiResult::err(502, "未找到可用版本");
        }

        let file_url = format!("{}/mods/{}/files/{}", CURSEFORGE_API, project_id, file_id);
        let file_resp = client.get(&file_url).header("x-api-key", cf_api_key).send().await;
        match file_resp {
            Ok(r) if r.status().is_success() => {
                let v: Value = r.json().await.unwrap_or_default();
                let f = v.get("data").unwrap_or(&v);
                let download_url = f.get("downloadUrl").and_then(|u| u.as_str()).unwrap_or("").to_string();
                let file_name = f.get("fileName").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let file_size = f.get("fileLength").and_then(|s| s.as_u64()).unwrap_or(0);
                let sha1 = f.pointer("/hashes/0/value").and_then(|s| s.as_str()).unwrap_or("").to_string();
                if download_url.is_empty() {
                    return ApiResult::err(502, "未找到下载文件");
                }
                (download_url, file_name, file_size, sha1)
            }
            Ok(resp) => return ApiResult::err(502, &format!("CurseForge HTTP {}", resp.status())),
            Err(e) => return ApiResult::err(502, &format!("请求失败: {}", e)),
        }
    };

    if download_url.is_empty() {
        return ApiResult::err(502, "下载链接为空");
    }

    // 安全文件名
    let safe_name: String = file_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let final_name = if safe_name.is_empty() {
        format!("{}.jar", project_id)
    } else {
        safe_name
    };
    let dest_path = mods_dir.join(&final_name);

    // 创建下载会话
    let (session_id, _cancel_flag) = resources_session::create_session(
        &final_name,
        file_size,
        "mod",
        &project_id,
    );

    eprintln!(
        "[mods] 开始下载: {} → {} (sha1={})",
        download_url,
        dest_path.display(),
        if expected_sha1.is_empty() { "(none)" } else { &expected_sha1 }
    );

    // spawn 后台下载任务
    let session_id_for_progress = session_id.clone();
    let session_id_for_task = session_id.clone();
    let download_source = settings
        .get("downloadSource")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();
    let expected_sha1_task = expected_sha1.clone();

    tokio::spawn(async move {
        let progress_session_id = session_id_for_progress.clone();
        let on_progress: crate::download::ProgressCb = std::sync::Arc::new(move |p: &crate::download::DownloadProgress| {
            let pct = if p.total_bytes > 0 {
                ((p.bytes_downloaded as f64 / p.total_bytes as f64) * 100.0) as u32
            } else {
                0
            };
            resources_session::update_session_silent(&progress_session_id, |s| {
                if s.status == resources_session::ResourceStage::Cancelled {
                    return;
                }
                s.progress = pct;
                s.download_speed = p.speed;
                s.bytes_downloaded = p.bytes_downloaded;
                s.total_size = p.total_bytes;
                s.downloaded = p.bytes_downloaded;
                let speed_kb = if p.speed > 0 { p.speed / 1024 } else { 0 };
                s.message = format!("下载 {} {}% ({}KB/s)", s.file_name, pct, speed_kb);
                if !s.files.is_empty() {
                    s.files[0].progress = pct;
                    s.files[0].size = p.total_bytes;
                }
            });
        });

        let result = download_with_mirror(
            &download_url,
            &dest_path,
            if expected_sha1_task.is_empty() { None } else { Some(&expected_sha1_task) },
            if file_size > 0 { Some(file_size) } else { None },
            &download_source,
            300,
            Some(on_progress),
        )
        .await;

        match result {
            Ok(()) => {
                resources_session::update_session_silent(&session_id_for_task, |s| {
                    s.status = resources_session::ResourceStage::Completed;
                    s.progress = 100;
                    s.message = format!("{} 下载完成！", s.file_name);
                    if !s.files.is_empty() {
                        s.files[0].status = "completed".to_string();
                        s.files[0].progress = 100;
                    }
                });
            }
            Err(e) => {
                eprintln!("[mods] 下载失败: {}", e);
                resources_session::update_session_silent(&session_id_for_task, |s| {
                    s.status = resources_session::ResourceStage::Failed;
                    s.message = format!("下载失败: {}", e);
                    if !s.files.is_empty() {
                        s.files[0].status = "failed".to_string();
                    }
                });
            }
        }

        // 60 秒后清理会话
        let sid = session_id_for_task.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            resources_session::remove_session(&sid);
        });
    });

    ApiResult::ok(json!({
        "success": true,
        "sessionId": session_id,
        "fileName": final_name
    }))
}

/// POST /api/mods/download-version — 下载指定版本
async fn handle_download_version(body: &Option<Value>) -> ApiResult {
    // 复用 handle_download 的逻辑，但 versionId 必填
    handle_download(body).await
}

/// GET /api/mods/download-status — 下载状态查询
fn handle_download_status(params: &Option<Value>) -> ApiResult {
    let session_id = params
        .as_ref()
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if session_id.is_empty() {
        return ApiResult::err(400, "Missing sessionId");
    }

    match resources_session::get_session_status(session_id) {
        Some(status) => ApiResult::ok(status),
        None => ApiResult::err(404, "会话不存在或已过期"),
    }
}

// ============== 依赖路由 ==============

/// POST /api/mods/get-dependencies — 获取依赖
async fn handle_get_dependencies(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let _source = body
        .as_ref()
        .and_then(|b| b.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth");
    let game_version = body
        .as_ref()
        .and_then(|b| b.get("gameVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let loader = body
        .as_ref()
        .and_then(|b| b.get("loader"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let _project_id = body
        .as_ref()
        .and_then(|b| b.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if version_id.is_empty() {
        return ApiResult::ok(json!({ "dependencies": [] }));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 拉取版本信息
    let url = format!("{}/version/{}", MODRINTH_API, version_id);
    let version_data: Value = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or(Value::Null),
        _ => return ApiResult::ok(json!({ "dependencies": [] })),
    };

    let deps = version_data
        .get("dependencies")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    // 过滤必要依赖（排除 Fabric API / QSL）
    let required: Vec<Value> = deps
        .iter()
        .filter(|d| {
            let dep_type = d.get("dependency_type").and_then(|t| t.as_str()).unwrap_or("");
            let pid = d.get("project_id").and_then(|p| p.as_str()).unwrap_or("");
            dep_type == "required" && !pid.is_empty() && pid != "P7dR8mSH" && pid != "qvIfYCYJ"
        })
        .cloned()
        .collect();

    // 简化：直接返回依赖 ID 列表（完整实现需查询每个依赖的兼容版本）
    let result: Vec<Value> = required
        .iter()
        .map(|d| {
            json!({
                "projectId": d.get("project_id").and_then(|p| p.as_str()).unwrap_or(""),
                "dependencyType": d.get("dependency_type").and_then(|t| t.as_str()).unwrap_or(""),
                "versionId": d.get("version_id").and_then(|v| v.as_str()).unwrap_or(""),
                "gameVersion": game_version,
                "loader": loader
            })
        })
        .collect();

    ApiResult::ok(json!({ "dependencies": result }))
}

/// POST /api/mods/get-dependencies-recursive — 递归获取依赖
async fn handle_get_dependencies_recursive(body: &Option<Value>) -> ApiResult {
    // 简化：递归依赖复杂，先返回第一层
    handle_get_dependencies(body).await
}

/// GET /api/mods/resolve-deps — 解析依赖
async fn handle_resolve_deps(params: &Option<Value>) -> ApiResult {
    // 简化：复用 get-dependencies 的逻辑
    handle_get_dependencies(params).await
}

/// POST /api/mods/resolve-deps-versions — 解析依赖版本
async fn handle_resolve_deps_versions(body: &Option<Value>) -> ApiResult {
    // 简化：复用 get-dependencies 的逻辑
    handle_get_dependencies(body).await
}

// ============== 管理路由 ==============

/// POST /api/mods/toggle — 启用/禁用模组
fn handle_toggle(body: &Option<Value>) -> ApiResult {
    let mod_id = body
        .as_ref()
        .and_then(|b| b.get("modId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let enabled = body
        .as_ref()
        .and_then(|b| b.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if mod_id.is_empty() {
        return ApiResult::err(400, "Missing modId");
    }

    let settings = storage::load_settings();
    let mods_path = resolve_mods_dir(&settings, version_id, "");
    let mods_path = match mods_path {
        Some(p) => p,
        None => return ApiResult::err(400, "请先安装一个游戏版本"),
    };

    if !mods_path.exists() {
        return ApiResult::err(400, "mods 文件夹不存在");
    }

    let base_name = mod_id.trim_end_matches(".disabled");
    let clean_path = mods_path.join(base_name);
    let disabled_path = mods_path.join(format!("{}.disabled", base_name));

    if enabled {
        if disabled_path.exists() {
            if let Err(e) = std::fs::rename(&disabled_path, &clean_path) {
                return ApiResult::err(500, &format!("文件操作失败: {}", e));
            }
        }
    } else {
        if clean_path.exists() {
            if let Err(e) = std::fs::rename(&clean_path, &disabled_path) {
                return ApiResult::err(500, &format!("文件操作失败: {}", e));
            }
        }
    }

    ApiResult::ok(json!({ "success": true, "enabled": enabled }))
}

/// POST /api/mods/delete — 删除模组（模糊匹配文件名）
fn handle_delete(body: &Option<Value>) -> ApiResult {
    let mod_id = body
        .as_ref()
        .and_then(|b| b.get("modId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if mod_id.is_empty() {
        return ApiResult::err(400, "Missing modId");
    }

    let settings = storage::load_settings();
    let mods_path = resolve_mods_dir(&settings, "", "");
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(p) = mods_path {
        search_dirs.push(p);
    }
    // 非隔离时搜索共享目录和 .minecraft/mods
    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !version_isolation {
        let game_dir = settings
            .get("gameDir")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| storage::resolve_data_dir());
        let shared_mods = game_dir.join("mods");
        if !search_dirs.contains(&shared_mods) {
            search_dirs.push(shared_mods);
        }
        if let Some(home) = dirs::home_dir() {
            let mc_mods = home
                .join("AppData")
                .join("Roaming")
                .join(".minecraft")
                .join("mods");
            if !search_dirs.contains(&mc_mods) {
                search_dirs.push(mc_mods);
            }
        }
    }

    let mod_id_lower = mod_id.to_lowercase();
    let mut deleted_count = 0;
    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let base = name.to_lowercase().replace(".disabled", "");
                if base.contains(&mod_id_lower) {
                    if std::fs::remove_file(entry.path()).is_ok() {
                        deleted_count += 1;
                    }
                }
            }
        }
    }

    ApiResult::ok(json!({
        "success": true,
        "message": format!("已删除 {} 个文件", deleted_count),
        "deleted": deleted_count
    }))
}

/// POST /api/mods/check-updates — 检查模组更新
async fn handle_check_updates(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() {
        return ApiResult::err(400, "Missing versionId");
    }
    let result = mods::check_mod_updates(version_id).await;
    ApiResult::ok(result)
}

/// POST /api/mods/install-from-file — 从文件安装模组
fn handle_install_from_file(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let file_path = body
        .as_ref()
        .and_then(|b| b.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() || file_path.is_empty() {
        return ApiResult::err(400, "Missing params");
    }

    let settings = storage::load_settings();
    let mods_dir = resolve_mods_dir(&settings, version_id, "");
    let mods_dir = match mods_dir {
        Some(d) => d,
        None => return ApiResult::err(400, "无法确定模组目录"),
    };

    if let Err(e) = std::fs::create_dir_all(&mods_dir) {
        return ApiResult::err(500, &format!("创建目录失败: {}", e));
    }

    let src = PathBuf::from(&file_path);
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "mod.jar".to_string());
    let dest = mods_dir.join(&file_name);

    match std::fs::copy(&src, &dest) {
        Ok(_) => ApiResult::ok(json!({ "success": true })),
        Err(e) => ApiResult::ok(json!({ "success": false, "error": e.to_string() })),
    }
}

/// POST /api/mods/remove — 删除指定版本的指定模组文件
fn handle_remove(body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let file_name = body
        .as_ref()
        .and_then(|b| b.get("fileName"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if version_id.is_empty() || file_name.is_empty() {
        return ApiResult::err(400, "Missing params");
    }
    // 路径安全校验：禁止目录穿越
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        return ApiResult::err(400, "Invalid fileName");
    }

    let settings = storage::load_settings();
    let mods_dir = resolve_mods_dir(&settings, version_id, "");
    let mods_dir = match mods_dir {
        Some(d) => d,
        None => return ApiResult::err(400, "无法确定模组目录"),
    };

    let rm_path = mods_dir.join(file_name);
    // 二次校验：确保最终路径在 mods_dir 下
    let canonical_mods = mods_dir.canonicalize().unwrap_or(mods_dir.clone());
    let canonical_rm = rm_path.canonicalize().unwrap_or(rm_path.clone());
    if !canonical_rm.starts_with(&canonical_mods) {
        return ApiResult::err(400, "Invalid path");
    }

    if rm_path.exists() {
        if let Err(e) = std::fs::remove_file(&rm_path) {
            return ApiResult::ok(json!({ "success": false, "error": e.to_string() }));
        }
    }

    ApiResult::ok(json!({ "success": true }))
}

// ============== 对话框路由 ==============

/// GET /api/mods/select-modpack-file — 选择整合包文件
async fn handle_select_modpack_file() -> ApiResult {
    // 简化：返回空（前端可用 <input type="file"> 代替）
    ApiResult::ok(Value::Null)
}

/// GET /api/mods/select-file — 选择模组文件
async fn handle_select_file() -> ApiResult {
    ApiResult::ok(Value::Null)
}

/// POST /api/mods/select-save-folder — 选择保存文件夹
async fn handle_select_save_folder(_body: &Option<Value>) -> ApiResult {
    ApiResult::ok(json!({ "cancelled": true, "error": "对话框功能在前端实现" }))
}

// ============== 辅助函数 ==============

/// 解析 mods 目录
/// 复刻原项目 versions.getVersionModsDir 逻辑
fn resolve_mods_dir(settings: &Value, version_id: &str, _mc_version: &str) -> Option<PathBuf> {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    // 优先用指定的 versionId
    let vid = if !version_id.is_empty() {
        version_id.to_string()
    } else {
        // 否则用 selectedVersion
        settings
            .get("selectedVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };

    if vid.is_empty() {
        // 最后兜底：扫描 versions 目录找第一个已安装的
        if !versions_dir.exists() {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // 跳过特殊目录
                    if name.starts_with('.') || name == "version-settings.json" {
                        continue;
                    }
                    return Some(versions_dir.join(name).join("mods"));
                }
            }
        }
        return None;
    }

    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if version_isolation {
        Some(versions_dir.join(&vid).join("mods"))
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
