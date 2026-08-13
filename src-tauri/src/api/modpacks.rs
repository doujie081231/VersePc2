// api/modpacks.rs — 整合包 API 路由
// 职责：整合包搜索（Modrinth + CurseForge 双源聚合）、安装链接获取、导入
// 对应原项目 server/api/routes/modpacks.js
//
// 路由：
//   GET  /api/modpacks/search   双源聚合搜索整合包
//   POST /api/modpacks/install  获取整合包下载链接（不实际下载，前端调 import）
//   POST /api/modpack/import    导入 .mrpack/.zip 整合包（委托 modpack 模块）

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::ApiResult;
use crate::storage;

/// Modrinth API 官方地址
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// Modrinth API 国内镜像（BMCLAPI）
const MODRINTH_API_MIRROR: &str = "https://bmclapi2.bangbang93.com/modrinth/v2";
/// CurseForge API 官方地址
const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
/// CurseForge 默认 API Key（与原项目一致）
const DEFAULT_CF_API_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

/// 处理整合包路由
pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/modpacks/search" => Some(handle_search(params).await),
        "POST /api/modpacks/install" => Some(handle_install(body).await),
        "POST /api/modpack/import" => Some(handle_import(app, body).await),
        "POST /api/modpack/cancel" => Some(handle_cancel(body).await),
        _ => None,
    }
}

/// GET /api/modpacks/search — Modrinth + CurseForge 双源聚合搜索
///
/// 查询参数：
///   - query: 搜索关键词
///   - loader: 加载器过滤（fabric/forge/neoforge/quilt）
///   - version: MC 版本过滤
///   - source: 数据源（any/modrinth/curseforge）
///   - limit: 返回数量（默认 10）
///   - offset: 偏移量（默认 0）
async fn handle_search(params: &Option<Value>) -> ApiResult {
    let query = params
        .as_ref()
        .and_then(|p| p.get("query"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let loader = params
        .as_ref()
        .and_then(|p| p.get("loader"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = params
        .as_ref()
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = params
        .as_ref()
        .and_then(|p| p.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("any")
        .to_string();
    let limit = params
        .as_ref()
        .and_then(|p| p.get("limit"))
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let offset = params
        .as_ref()
        .and_then(|p| p.get("offset"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    // 中文关键词翻译（简化版：直接透传，复杂翻译后续迁移）
    let mp_query = translate_chinese_query(&query);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut all_hits: Vec<Value> = Vec::new();

    // --- Modrinth 源 ---
    if source == "any" || source == "modrinth" {
        if let Ok(mr_hits) = search_modrinth(&client, &mp_query, &loader, &version, limit, offset).await {
            all_hits.extend(mr_hits);
        }
    }

    // --- CurseForge 源 ---
    if source == "any" || source == "curseforge" {
        if let Ok(cf_hits) = search_curseforge(&client, &mp_query, &loader, &version, limit, offset).await {
            all_hits.extend(cf_hits);
        }
    }

    // 双源混合时按下载量排序
    if source == "any" {
        all_hits.sort_by(|a, b| {
            let da = a.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            let db = b.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            db.cmp(&da)
        });
        all_hits.truncate(limit);
    }

    let total = all_hits.len();
    ApiResult::ok(json!({
        "hits": all_hits,
        "total": total,
        "offset": offset
    }))
}

/// 从 Modrinth 搜索整合包
async fn search_modrinth(
    client: &reqwest::Client,
    query: &str,
    loader: &str,
    version: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, String> {
    // 构造 facets
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:modpack".to_string()]];
    if !loader.is_empty() {
        facets.push(vec![format!("categories:{}", loader)]);
    }
    if !version.is_empty() {
        facets.push(vec![format!("versions:{}", version)]);
    }
    let facets_json = serde_json::to_string(&facets).unwrap_or_default();

    let url = format!(
        "{}/search?query={}&index=relevance&limit={}&offset={}&facets={}",
        MODRINTH_API,
        urlencoding::encode(query),
        limit,
        offset,
        urlencoding::encode(&facets_json)
    );

    eprintln!("[modpacks] Modrinth 搜索: {}", url);

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Modrinth 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Modrinth HTTP {}", resp.status()));
    }

    let result: Value = resp
        .json()
        .await
        .map_err(|e| format!("Modrinth 解析失败: {}", e))?;

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
                        "categories": hit.get("categories").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                        "versions": hit.get("versions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                        "source": "modrinth"
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

/// 从 CurseForge 搜索整合包
async fn search_curseforge(
    client: &reqwest::Client,
    query: &str,
    loader: &str,
    version: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, String> {
    let settings = storage::load_settings();
    let cf_api_key = settings
        .get("curseforgeApiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CF_API_KEY);

    // classId 4471 = 整合包
    let mut url = format!(
        "{}/mods/search?gameId=432&searchFilter={}&sortOrder=Desc&classId=4471&pageSize={}&index={}&sortField=2",
        CURSEFORGE_API,
        urlencoding::encode(query),
        limit,
        offset
    );

    if !loader.is_empty() {
        // CurseForge modLoaderType: forge=1, fabric=4, quilt=5, neoforge=5（NeoForge 与 Quilt 共用 5）
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

    if !version.is_empty() {
        url.push_str(&format!("&gameVersion={}", urlencoding::encode(version)));
    }

    eprintln!("[modpacks] CurseForge 搜索: {}", url);

    let resp = client
        .get(&url)
        .header("x-api-key", cf_api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("CurseForge 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("CurseForge HTTP {}", resp.status()));
    }

    let result: Value = resp
        .json()
        .await
        .map_err(|e| format!("CurseForge 解析失败: {}", e))?;

    let hits = result
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .map(|mod_obj| {
                    let author = mod_obj
                        .get("authors")
                        .and_then(|a| a.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|a| a.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown");
                    let categories = mod_obj
                        .get("categories")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|c| {
                                    c.get("name")
                                        .and_then(|n| n.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| {
                                            c.get("id")
                                                .map(|i| i.to_string())
                                                .unwrap_or_default()
                                        })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    json!({
                        "id": mod_obj.get("id").map(|i| i.to_string()).unwrap_or_default(),
                        "slug": mod_obj.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                        "title": mod_obj.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "description": mod_obj.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                        "author": author,
                        "icon": mod_obj.get("logo").and_then(|l| l.get("url")).and_then(|u| u.as_str()).unwrap_or(""),
                        "downloads": mod_obj.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
                        "categories": categories,
                        "versions": [],
                        "source": "curseforge"
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

/// POST /api/modpacks/install — 获取整合包下载链接
///
/// 请求体：
///   - projectId: Modrinth 项目 ID
///   - mcVersion: 可选，MC 版本过滤
///
/// 返回：{ success, name, versionId, fileName, downloadUrl, destPath, size, mcVersion, loaders }
async fn handle_install(body: &Option<Value>) -> ApiResult {
    let project_id = body
        .as_ref()
        .and_then(|b| b.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mc_version = body
        .as_ref()
        .and_then(|b| b.get("mcVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // 拉取版本列表
    let mut version_url = format!("{}/project/{}/version", MODRINTH_API, project_id);
    if !mc_version.is_empty() {
        version_url.push_str(&format!("?game_versions=[\"{}\"]", mc_version));
    }

    eprintln!("[modpacks] 拉取版本: {}", version_url);

    let resp = match client.get(&version_url).send().await {
        Ok(r) => r,
        Err(e) => {
            return ApiResult::err(502, &format!("请求失败: {}", e));
        }
    };

    if !resp.status().is_success() {
        return ApiResult::err(502, &format!("Modrinth HTTP {}", resp.status()));
    }

    let version_data: Vec<Value> = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            return ApiResult::err(502, &format!("解析失败: {}", e));
        }
    };

    if version_data.is_empty() {
        return ApiResult::ok(json!({ "success": false, "error": "未找到可用版本" }));
    }

    let target = &version_data[0];
    let files = target.get("files").and_then(|f| f.as_array());
    let primary_file = files
        .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false)))
        .or_else(|| files.and_then(|arr| arr.first()));

    let primary_file = match primary_file {
        Some(f) => f,
        None => return ApiResult::ok(json!({ "success": false, "error": "未找到下载链接" })),
    };

    let download_url = primary_file
        .get("url")
        .and_then(|u| u.as_str())
        .unwrap_or("");
    if download_url.is_empty() {
        return ApiResult::ok(json!({ "success": false, "error": "下载链接为空" }));
    }

    let default_file_name = format!("{}.mrpack", project_id);
    let file_name = primary_file
        .get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or(&default_file_name);

    // 解析下载目录：dataDir/modpacks（简化版，未实现版本隔离路径解析）
    let settings = storage::load_settings();
    let data_dir = storage::resolve_data_dir();
    let game_dir = settings
        .get("gameDir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| data_dir.clone());
    let download_dir = game_dir.join("modpacks");

    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        return ApiResult::err(500, &format!("无法创建目录: {}", e));
    }

    let dest_path = download_dir.join(file_name);
    let modpack_name = target
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or(project_id);
    let version_id = target
        .get("id")
        .and_then(|i| i.as_str())
        .unwrap_or("");
    let size = primary_file
        .get("size")
        .and_then(|s| s.as_u64())
        .unwrap_or(0);
    let actual_mc_version = target
        .get("game_versions")
        .and_then(|gv| gv.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or(mc_version);
    let loaders = target
        .get("loaders")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();

    ApiResult::ok(json!({
        "success": true,
        "name": modpack_name,
        "versionId": version_id,
        "fileName": file_name,
        "downloadUrl": download_url,
        "destPath": dest_path.to_string_lossy(),
        "size": size,
        "mcVersion": actual_mc_version,
        "loaders": loaders,
        "message": "整合包下载链接已获取，请使用 modpack/import 接口导入"
    }))
}

/// POST /api/modpack/import — 导入整合包文件
///
/// 请求体：
///   - filePath: 整合包文件路径
///   - customName: 可选，用户自定义版本名
///   - updateVersionId: 可选，更新整合包时指定要覆盖的已存在版本目录（不传则新建版本）
///   - cancelToken: 可选，前端生成的会话标识，用于支持导入过程中的"取消下载"
///
/// 委托 modpack::import_modpack 处理，支持 mrpack/curseforge/hmcl/raw_zip 四种格式
async fn handle_import(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let file_path = body
        .as_ref()
        .and_then(|b| b.get("filePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file_path.is_empty() {
        return ApiResult::err(400, "Missing filePath");
    }

    let custom_name = body
        .as_ref()
        .and_then(|b| b.get("customName"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let update_version_id = body
        .as_ref()
        .and_then(|b| b.get("updateVersionId"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let cancel_token = body
        .as_ref()
        .and_then(|b| b.get("cancelToken"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // 调用 modpack 模块统一入口
    let result = crate::modpack::import_modpack(
        app,
        file_path,
        &custom_name,
        update_version_id.as_deref(),
        cancel_token.as_deref(),
    )
    .await;
    ApiResult::ok(result)
}

/// POST /api/modpack/cancel — 取消正在进行的整合包导入
///
/// 请求体：
///   - cancelToken: 前端导入时生成的会话标识
async fn handle_cancel(body: &Option<Value>) -> ApiResult {
    let token = body
        .as_ref()
        .and_then(|b| b.get("cancelToken"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if token.is_empty() {
        return ApiResult::err(400, "Missing cancelToken");
    }

    let canceled = crate::modpack::cancel_modpack_abort(token);
    if canceled {
        eprintln!("[modpacks] 已请求取消整合包导入: {}", token);
        ApiResult::ok(json!({ "success": true }))
    } else {
        ApiResult::ok(json!({
            "success": false,
            "error": "未找到正在进行的导入任务（可能已结束）"
        }))
    }
}

/// 中文搜索关键词翻译（简化版）
/// 原项目使用 data/mod-chinese-names.js 进行精确翻译
/// 此处先返回原查询，后续迁移完整翻译表
fn translate_chinese_query(query: &str) -> String {
    // 检测是否包含中文
    let has_chinese = query.chars().any(|c| {
        ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
    });

    if !has_chinese {
        return query.to_string();
    }

    // 简化的中文关键词映射（原项目有完整表，后续迁移）
    // 这里只做最常见的几个翻译
    let translations: &[(&str, &str)] = &[
        ("整合包", "modpack"),
        ("空岛", "skyblock"),
        ("生存", "survival"),
        ("冒险", "adventure"),
        ("RPG", "rpg"),
        ("科幻", "sci-fi"),
        ("魔法", "magic"),
        ("工业", "tech"),
    ];

    for (cn, en) in translations {
        if query.contains(cn) {
            return en.to_string();
        }
    }

    // 没有匹配的翻译，原样返回
    query.to_string()
}
