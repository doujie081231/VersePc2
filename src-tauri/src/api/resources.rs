// api/resources.rs — 资源搜索/下载路由
// 职责：Modrinth 资源（模组/整合包/资源包/光影/数据包）搜索、详情、版本列表、下载
// 对应原项目 server/api/routes/resources.js
//
// 路由：
//   GET  /api/resources/search    Modrinth + CurseForge 双源聚合搜索
//   GET  /api/resources/detail    Modrinth 项目详情
//   GET  /api/resources/versions  项目版本列表
//   POST /api/resources/download  下载资源（异步，返回 sessionId）
//
// 下载会话：通过 download::resources_session 管理，前端监听
// 'resource-download-progress' 事件获取进度，或调用 GET /api/resources/download-status 查询

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha1::Digest;
use tauri::{AppHandle, Listener};
use tokio::sync::Mutex;

use crate::api::ApiResult;
use crate::download::{download_with_mirror, resources_session};
use crate::modpack;
use crate::storage;

/// Modrinth API 官方地址
const MODRINTH_API: &str = "https://api.modrinth.com/v2";
/// Modrinth API 镜像地址（与原项目一致，国内访问更快）
const MODRINTH_API_MIRROR: &str = "https://mod.mcimirror.top/modrinth/v2";
/// CurseForge API 官方地址
const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
/// CurseForge API 镜像地址
const CURSEFORGE_API_MIRROR: &str = "https://mod.mcimirror.top/curseforge/v1";
/// CurseForge 默认 API Key（与原项目一致）
const DEFAULT_CF_API_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

/// 全局复用 reqwest::Client，避免每次请求都做 TLS 握手
/// 连接池保持，keep-alive 复用 TCP 连接
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("VersePC/2.0")
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// API 响应缓存项
struct CacheEntry {
    data: Value,
    ts: Instant,
}

/// 全局 API 缓存（URL → CacheEntry）
/// 对齐原项目 cachedFetchJSON 的 60 秒 TTL 机制
fn api_cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 读取缓存；命中且在 TTL 内返回数据
async fn cache_get(url: &str, ttl_ms: u64) -> Option<Value> {
    let cache = api_cache().lock().await;
    if let Some(entry) = cache.get(url) {
        if entry.ts.elapsed().as_millis() < ttl_ms as u128 {
            return Some(entry.data.clone());
        }
    }
    None
}

/// 写入缓存；超过 2000 项时清理过期项
async fn cache_set(url: String, data: Value, ttl_ms: u64) {
    let mut cache = api_cache().lock().await;
    cache.insert(url, CacheEntry { data, ts: Instant::now() });
    if cache.len() > 2000 {
        let now = Instant::now();
        cache.retain(|_, v| now.duration_since(v.ts).as_millis() < (ttl_ms * 2) as u128);
    }
}

/// 带镜像回退的 JSON 请求，对齐原项目 request.js fetchJSON 多步策略：
///   镜像 8s → 官方 10s → 官方完整超时(20s)；不走镜像时官方 10s → 官方完整超时。
/// 关键点：
///   - 镜像连续失败后走熔断（_isMirrorAvailable），熔断期间只用官方，避免拖慢列表
///   - 官方 API（尤其 CurseForge）国内访问较慢，需给足 20s 完整超时
///   - 镜像的 403/404 不阻断，继续尝试官方；官方 403/404 才直接失败
async fn fetch_json_with_mirror(url: &str, headers: Option<&reqwest::header::HeaderMap>) -> Result<Value, String> {
    let client = shared_client();
    use crate::download::mirror;
    let mirror_url = if url.starts_with(MODRINTH_API) {
        Some(url.replace(MODRINTH_API, MODRINTH_API_MIRROR))
    } else if url.starts_with(CURSEFORGE_API) {
        Some(url.replace(CURSEFORGE_API, CURSEFORGE_API_MIRROR))
    } else {
        None
    };

    let use_mirror = mirror_url.is_some() && mirror::is_mirror_available();
    let steps: Vec<(&str, u64, bool)> = if use_mirror {
        let murl = mirror_url.as_deref().unwrap_or("");
        vec![
            (murl, 8000, true),
            (url, 10000, false),
            (url, 20000, false),
        ]
    } else {
        vec![
            (url, 10000, false),
            (url, 20000, false),
        ]
    };

    let mut last_err = String::new();
    for (step_url, timeout_ms, is_mirror) in steps {
        let mut req = client.get(step_url).timeout(Duration::from_millis(timeout_ms));
        if let Some(h) = headers {
            req = req.headers(h.clone());
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    if is_mirror {
                        mirror::mirror_success();
                    }
                    match resp.json::<Value>().await {
                        Ok(v) => return Ok(v),
                        Err(e) => last_err = format!("JSON解析失败: {}", e),
                    }
                } else {
                    last_err = format!("HTTP {}", status);
                    // 镜像的 403/404 不阻断，继续尝试官方；官方 403/404 才直接失败
                    if (status == 403 || status == 404) && !is_mirror {
                        return Err(last_err);
                    }
                    if is_mirror {
                        mirror::mirror_failed();
                    }
                }
            }
            Err(e) => {
                last_err = format!("{}", e);
                if is_mirror {
                    mirror::mirror_failed();
                }
            }
        }
    }
    Err(last_err)
}

/// 带缓存的 JSON 请求：命中缓存直接返回，否则 fetch 后写缓存
async fn cached_fetch_json(url: String, ttl_ms: u64, headers: Option<&reqwest::header::HeaderMap>) -> Result<Value, String> {
    if let Some(cached) = cache_get(&url, ttl_ms).await {
        return Ok(cached);
    }
    let data = fetch_json_with_mirror(&url, headers).await?;
    cache_set(url, data.clone(), ttl_ms).await;
    Ok(data)
}

/// 处理资源路由
pub async fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/resources/search" => Some(handle_search(params).await),
        "GET /api/resources/detail" => Some(handle_detail(params).await),
        "GET /api/resources/versions" => Some(handle_versions(params).await),
        "POST /api/resources/download" => Some(handle_download(app, body).await),
        "GET /api/resources/download-status" => Some(handle_download_status(params)),
        "POST /api/resources/download-cancel" => Some(handle_download_cancel(body)),
        "GET /api/resource-image" => Some(handle_resource_image(params).await),
        _ => None,
    }
}

/// 从图片 URL 中推断扩展名与 MIME（默认 png）
fn image_ext_and_mime(url: &str) -> (&'static str, &'static str) {
    let lower = url.to_lowercase();
    let without_query = lower.split(['?', '#']).next().unwrap_or("").to_string();
    let ext = without_query
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    match ext {
        "webp" => ("webp", "image/webp"),
        "jpg" | "jpeg" => ("jpg", "image/jpeg"),
        "gif" => ("gif", "image/gif"),
        "avif" => ("avif", "image/avif"),
        "svg" => ("svg", "image/svg+xml"),
        "ico" => ("ico", "image/x-icon"),
        "bmp" => ("bmp", "image/bmp"),
        _ => ("png", "image/png"),
    }
}

/// GET /api/resource-image — 下载并缓存图片，返回本地 base64 data URL
async fn handle_resource_image(params: &Option<Value>) -> ApiResult {
    let url = params
        .as_ref()
        .and_then(|p| p.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if url.is_empty() {
        return ApiResult::err(400, "Missing url");
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return ApiResult::err(400, "Invalid url");
    }

    let mut hasher = sha1::Sha1::new();
    hasher.update(url.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let (ext, mime) = image_ext_and_mime(url);

    let cache_dir = storage::resolve_data_dir().join("image-cache");
    let cache_path = cache_dir.join(format!("{}.{}", hash, ext));

    if cache_path.exists() {
        if let Ok(data) = std::fs::read(&cache_path) {
            return ApiResult::ok(json!({ "dataUrl": crate::utils::bytes_to_data_url(&data, mime) }));
        }
    }

    let client = shared_client();
    let resp = match client.get(url).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return ApiResult::err(502, &format!("HTTP {}", r.status())),
        Err(e) => return ApiResult::err(502, &format!("请求失败: {}", e)),
    };
    let bytes = match resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => return ApiResult::err(502, &format!("读取失败: {}", e)),
    };
    if bytes.is_empty() {
        return ApiResult::err(502, "空响应");
    }

    if std::fs::create_dir_all(&cache_dir).is_ok() {
        let _ = std::fs::write(&cache_path, &bytes);
    }

    ApiResult::ok(json!({ "dataUrl": crate::utils::bytes_to_data_url(&bytes, mime) }))
}

/// GET /api/resources/search — Modrinth + CurseForge 双源聚合搜索
///
/// 查询参数：
///   - query: 搜索关键词
///   - type: 资源类型（mod/modpack/resourcepack/shader/datapack，默认 modpack）
///   - loader: 加载器过滤
///   - version: MC 版本过滤
///   - category: 分类过滤（仅 Modrinth）
///   - sort: 排序（relevance/downloads/newest/updated，默认 downloads）
///   - limit: 返回数量（默认 15）
///   - offset: 偏移量（默认 0）
///   - source: 数据源（空/modrinth/curseforge）
async fn handle_search(params: &Option<Value>) -> ApiResult {
    let query = params
        .as_ref()
        .and_then(|p| p.get("query"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let res_type = params
        .as_ref()
        .and_then(|p| p.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("modpack")
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
        .unwrap_or("downloads")
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
    let source = params
        .as_ref()
        .and_then(|p| p.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 双源并行查询（原项目是串行，Tauri 版并行优化以提升速度）
    let use_mr = source.is_empty() || source == "modrinth";
    let use_cf = (source.is_empty() || source == "curseforge") && (res_type == "modpack" || res_type == "mod");

    let mr_fut = if use_mr {
        Some(search_modrinth(&query, &res_type, &loader, &version, &category, &sort, limit, offset))
    } else { None };
    let cf_fut = if use_cf {
        Some(search_curseforge(&query, &res_type, &loader, &version, limit, offset))
    } else { None };

    // match 两个 Option，走对应分支避免 pending
    let (mr_res, cf_res): (Result<Vec<Value>, String>, Result<Vec<Value>, String>) = match (mr_fut, cf_fut) {
        (Some(mr), Some(cf)) => {
            let (m, c) = tokio::join!(mr, cf);
            (m, c)
        }
        (Some(mr), None) => (mr.await, Ok(vec![])),
        (None, Some(cf)) => (Ok(vec![]), cf.await),
        (None, None) => (Ok(vec![]), Ok(vec![])),
    };

    let mut all_hits: Vec<Value> = Vec::new();
    if let Ok(hits) = mr_res {
        all_hits.extend(hits);
    }
    if let Ok(hits) = cf_res {
        all_hits.extend(hits);
    }

    // 双源混合时按下载量排序
    if all_hits.len() > 1 {
        all_hits.sort_by(|a, b| {
            let da = a.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            let db = b.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
            db.cmp(&da)
        });
        if source.is_empty() {
            all_hits.truncate(limit);
        }
    }

    let total = all_hits.len();
    ApiResult::ok(json!({
        "hits": all_hits,
        "total": total,
        "offset": offset
    }))
}

/// 从 Modrinth 搜索资源
async fn search_modrinth(
    query: &str,
    res_type: &str,
    loader: &str,
    version: &str,
    category: &str,
    sort: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<Value>, String> {
    // 构造 facets
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", res_type)]];
    if !loader.is_empty() {
        facets.push(vec![format!("categories:{}", loader)]);
    }
    if !version.is_empty() {
        facets.push(vec![format!("versions:{}", version)]);
    }
    if !category.is_empty() {
        facets.push(vec![format!("categories:{}", category)]);
    }
    let facets_json = serde_json::to_string(&facets).unwrap_or_default();

    // 排序字段映射
    let sort_field = match sort {
        "relevance" => "relevance",
        "newest" => "newest",
        "updated" => "updated",
        _ => "downloads",
    };
    // 空查询时默认按下载量
    let final_sort = if query.is_empty() && sort == "relevance" {
        "downloads"
    } else {
        sort_field
    };

    let url = format!(
        "{}/search?query={}&index={}&limit={}&offset={}&facets={}",
        MODRINTH_API,
        urlencoding::encode(query),
        final_sort,
        limit,
        offset,
        urlencoding::encode(&facets_json)
    );

    // 60 秒 TTL 缓存（与原项目一致）
    let result = cached_fetch_json(url, 60000, None).await?;

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
                        "projectType": res_type
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

/// 从 CurseForge 搜索资源
async fn search_curseforge(
    query: &str,
    res_type: &str,
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

    // classId: 4471=整合包, 6=mod
    let cf_class_id = if res_type == "modpack" { 4471 } else { 6 };

    let mut url = format!(
        "{}/mods/search?gameId=432&searchFilter={}&sortOrder=Desc&classId={}&pageSize={}&index={}&sortField=2",
        CURSEFORGE_API,
        urlencoding::encode(query),
        cf_class_id,
        limit,
        offset
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

    if !version.is_empty() {
        url.push_str(&format!("&gameVersion={}", urlencoding::encode(version)));
    }

    // 构造 CurseForge 请求头（需要 x-api-key）
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&cf_api_key) {
        headers.insert("x-api-key", v);
    }
    headers.insert("Accept", reqwest::header::HeaderValue::from_static("application/json"));

    // 60 秒 TTL 缓存
    let result = cached_fetch_json(url, 60000, Some(&headers)).await?;

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
                                            c.get("id").map(|i| i.to_string()).unwrap_or_default()
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
                        "followers": mod_obj.get("followers").and_then(|v| v.as_u64()).unwrap_or(0),
                        "categories": categories,
                        "versions": [],
                        "dateCreated": mod_obj.get("dateCreated").and_then(|v| v.as_str()).unwrap_or(""),
                        "dateModified": mod_obj.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
                        "source": "curseforge",
                        "projectType": res_type
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(hits)
}

/// GET /api/resources/detail — Modrinth 项目详情
///
/// 查询参数：
///   - projectId: Modrinth 项目 ID
async fn handle_detail(params: &Option<Value>) -> ApiResult {
    let project_id = params
        .as_ref()
        .and_then(|p| p.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("VersePC/1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let url = format!("{}/project/{}", MODRINTH_API, project_id);
    eprintln!("[resources] 拉取详情: {}", url);

    let resp = match client.get(&url).header("Accept", "application/json").send().await {
        Ok(r) => r,
        Err(e) => return ApiResult::err(502, &format!("请求失败: {}", e)),
    };

    if !resp.status().is_success() {
        return ApiResult::err(502, &format!("Modrinth HTTP {}", resp.status()));
    }

    let project: Value = match resp.json().await {
        Ok(d) => d,
        Err(e) => return ApiResult::err(502, &format!("解析失败: {}", e)),
    };

    let gallery = project
        .get("gallery")
        .and_then(|g| g.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| {
                    let obj = match g.as_object() {
                        Some(o) => o,
                        None => return None,
                    };
                    let url = obj
                        .get("url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    if url.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "url": url,
                        "rawUrl": obj.get("raw_url").and_then(|u| u.as_str()).unwrap_or(""),
                        "title": obj.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": obj.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "created": obj.get("created").and_then(|v| v.as_str()).unwrap_or(""),
                    }))
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
        "license": project.get("license").and_then(|l| l.get("name")).and_then(|n| n.as_str()).unwrap_or(""),
        "sourceUrl": project.get("source_url").and_then(|v| v.as_str()).unwrap_or(""),
        "dateCreated": project.get("published").and_then(|v| v.as_str()).unwrap_or(""),
        "dateModified": project.get("updated").and_then(|v| v.as_str()).unwrap_or(""),
        "gallery": gallery,
        "source": "modrinth",
        "projectType": project.get("project_type").and_then(|v| v.as_str()).unwrap_or("")
    });

    ApiResult::ok(detail)
}

/// GET /api/resources/versions — Modrinth 项目版本列表
///
/// 查询参数：
///   - projectId: Modrinth 项目 ID
///   - loader: 加载器过滤
///   - gameVersion: MC 版本过滤
async fn handle_versions(params: &Option<Value>) -> ApiResult {
    let project_id = params
        .as_ref()
        .and_then(|p| p.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if project_id.is_empty() {
        return ApiResult::err(400, "Missing projectId");
    }
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

    if source == "curseforge" {
        return handle_versions_curseforge(&project_id).await;
    }
    if source != "modrinth" {
        // 其他来源暂不支持版本列表，返回空
        return ApiResult::ok(json!({ "versions": [] }));
    }

    let mut url = format!("{}/project/{}/version", MODRINTH_API, project_id);
    let mut query_parts: Vec<String> = Vec::new();
    if !loader.is_empty() {
        query_parts.push(format!("loaders=[\"{}\"]", loader));
    }
    if !game_ver.is_empty() {
        query_parts.push(format!("game_versions=[\"{}\"]", game_ver));
    }
    if !query_parts.is_empty() {
        url.push('?');
        url.push_str(&query_parts.join("&"));
    }

    eprintln!("[resources] 拉取版本: {}", url);

    let result = match fetch_json_with_mirror(&url, None).await {
        Ok(v) => v,
        Err(e) => return ApiResult::err(502, &format!("请求失败: {}", e)),
    };

    let versions: Vec<Value> = match result.as_array() {
        Some(arr) => arr.clone(),
        None => return ApiResult::err(502, "API返回格式异常"),
    };

    let result: Vec<Value> = versions
        .iter()
        .map(|v| {
            let files = v
                .get("files")
                .and_then(|f| f.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|f| {
                            json!({
                                "url": f.get("url").and_then(|u| u.as_str()).unwrap_or(""),
                                "filename": f.get("filename").and_then(|n| n.as_str()).unwrap_or(""),
                                "size": f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                                "primary": f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false),
                                "sha1": f.get("hashes").and_then(|h| h.get("sha1")).and_then(|s| s.as_str()).unwrap_or("")
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let deps = v
                .get("dependencies")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|d| {
                            json!({
                                "projectId": d.get("project_id").and_then(|p| p.as_str()).unwrap_or(""),
                                "versionId": d.get("version_id").and_then(|ver| ver.as_str()).unwrap_or(""),
                                "dependencyType": d.get("dependency_type").and_then(|t| t.as_str()).unwrap_or("")
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            json!({
                "id": v.get("id").and_then(|i| i.as_str()).unwrap_or(""),
                "versionNumber": v.get("version_number").and_then(|n| n.as_str()).unwrap_or(""),
                "versionName": v.get("name").and_then(|n| n.as_str()).unwrap_or(v.get("version_number").and_then(|n| n.as_str()).unwrap_or("")),
                "gameVersions": v.get("game_versions").and_then(|g| g.as_array()).cloned().unwrap_or_default(),
                "loaders": v.get("loaders").and_then(|l| l.as_array()).cloned().unwrap_or_default(),
                "releaseType": v.get("version_type").and_then(|t| t.as_str()).unwrap_or("release"),
                "datePublished": v.get("date_published").and_then(|d| d.as_str()).unwrap_or(""),
                "downloads": v.get("downloads").and_then(|d| d.as_u64()).unwrap_or(0),
                "changelog": v.get("changelog").and_then(|c| c.as_str()).unwrap_or(""),
                "files": files,
                "dependencies": deps
            })
        })
        .collect();

    ApiResult::ok(json!({ "versions": result }))
}

/// 从 CurseForge 获取项目版本列表（对齐 Modrinth versions 结构）
/// 官方接口：GET /v1/mods/{modId}/files?pageSize=50
async fn handle_versions_curseforge(project_id: &str) -> ApiResult {
    let settings = storage::load_settings();
    let cf_api_key = settings
        .get("curseforgeApiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_CF_API_KEY);

    let url = format!("{}/mods/{}/files?pageSize=50", CURSEFORGE_API, project_id);
    eprintln!("[resources][curseforge] 拉取版本: {}", url);

    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&cf_api_key) {
        headers.insert("x-api-key", v);
    }
    headers.insert("Accept", reqwest::header::HeaderValue::from_static("application/json"));

    let result = match cached_fetch_json(url, 60000, Some(&headers)).await {
        Ok(v) => v,
        Err(e) => return ApiResult::err(502, &format!("请求失败: {}", e)),
    };

    let files = result
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let versions: Vec<Value> = files
        .iter()
        .map(|f| {
            // CF 的 gameVersions 混有 "Client"/"Server" 和加载器，只保留 Minecraft 版本号
            let game_versions: Vec<Value> = f
                .get("gameVersions")
                .and_then(|g| g.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|gv| {
                            if let Some(s) = gv.as_str() {
                                let low = s.to_lowercase();
                                if low.contains("snapshot") || low.contains("client") || low.contains("server") {
                                    return false;
                                }
                                if low == "forge" || low == "fabric" || low == "neoforge" || low == "quilt" {
                                    return false;
                                }
                                // 形如 1.19.2 / 1.21 / 24w14a
                                s.starts_with(|c: char| c.is_ascii_digit())
                            } else {
                                false
                            }
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            // 从原始 gameVersions 中提取加载器
            let loaders: Vec<Value> = f
                .get("gameVersions")
                .and_then(|g| g.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|gv| {
                            let s = gv.as_str()?.to_lowercase();
                            if s == "forge" || s == "fabric" || s == "neoforge" || s == "quilt" {
                                Some(Value::String(s))
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            // CF releaseType: 1=Release, 2=Beta, 3=Alpha
            let release_type = match f.get("releaseType").and_then(|r| r.as_u64()).unwrap_or(1) {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            };

            let file_id = f.get("id").map(|i| i.to_string()).unwrap_or_default();
            let files_arr = json!([{
                "id": file_id,
                "url": f.get("downloadUrl").and_then(|u| u.as_str()).unwrap_or(""),
                "filename": f.get("fileName").and_then(|n| n.as_str()).unwrap_or(""),
                "size": f.get("fileLength").and_then(|s| s.as_u64()).unwrap_or(0),
                "primary": true,
                "sha1": f.get("hashes").and_then(|h| h.get("sha1")).and_then(|s| s.as_str()).unwrap_or(""),
                "datePublished": f.get("fileDate").and_then(|d| d.as_str()).unwrap_or(""),
                "releaseType": release_type
            }]);

            let deps: Vec<Value> = f
                .get("dependencies")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|d| {
                            json!({
                                "projectId": d.get("modId").map(|m| m.to_string()).unwrap_or_default(),
                                "versionId": d.get("fileId").map(|m| m.to_string()).unwrap_or_default(),
                                "dependencyType": d.get("relationType").map(|m| m.to_string()).unwrap_or_default()
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            json!({
                "id": file_id,
                "versionNumber": f.get("fileName").and_then(|n| n.as_str()).unwrap_or(""),
                "versionName": f.get("displayName").and_then(|n| n.as_str()).unwrap_or(""),
                "gameVersions": game_versions,
                "loaders": loaders,
                "releaseType": release_type,
                "datePublished": f.get("fileDate").and_then(|d| d.as_str()).unwrap_or(""),
                "downloads": 0,
                "changelog": "",
                "files": files_arr,
                "dependencies": deps
            })
        })
        .collect();

    ApiResult::ok(json!({ "versions": versions }))
}

/// POST /api/resources/download — 下载资源
///
/// 请求体：
///   - versionId: Modrinth 版本 ID（与 projectId 二选一）
///   - projectId: Modrinth 项目 ID
///   - projectType: 资源类型（mod/modpack/resourcepack/shader/datapack，默认 mod）
///   - savePath: 自定义保存路径（覆盖默认）
///   - customName: 自定义名称（仅 modpack 用作版本 ID）
///   - targetVersionId: 目标 MC 版本 ID（不传则用 selectedVersion）
///   - source: 数据源（modrinth/curseforge，默认 modrinth）
async fn handle_download(app: &AppHandle, body: &Option<Value>) -> ApiResult {
    let version_id = body
        .as_ref()
        .and_then(|b| b.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let project_id = body
        .as_ref()
        .and_then(|b| b.get("projectId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let project_type = body
        .as_ref()
        .and_then(|b| b.get("projectType"))
        .and_then(|v| v.as_str())
        .unwrap_or("mod")
        .to_string();
    let save_path = body
        .as_ref()
        .and_then(|b| b.get("savePath"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut target_version_id = body
        .as_ref()
        .and_then(|b| b.get("targetVersionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let source = body
        .as_ref()
        .and_then(|b| b.get("source"))
        .and_then(|v| v.as_str())
        .unwrap_or("modrinth")
        .to_string();
    let custom_name = body
        .as_ref()
        .and_then(|b| b.get("customName"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if version_id.is_empty() && project_id.is_empty() {
        return ApiResult::err(400, "Missing versionId or projectId");
    }

    let settings = storage::load_settings();

    // 解析目标 MC 版本（modpack 不需要）
    if target_version_id.is_empty() && project_type != "modpack" {
        target_version_id = settings
            .get("selectedVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }

    // 决定目标目录
    let dest_dir = if !save_path.is_empty() {
        PathBuf::from(&save_path)
    } else {
        match project_type.as_str() {
            "modpack" => resolve_sub_dir(&settings, &target_version_id, "modpacks"),
            "resourcepack" => resolve_sub_dir(&settings, &target_version_id, "resourcepacks"),
            "shader" => resolve_sub_dir(&settings, &target_version_id, "shaderpacks"),
            "datapack" => resolve_sub_dir(&settings, &target_version_id, "datapacks"),
            _ => {
                // mod → version mods dir
                let mods_dir = resolve_version_mods_dir(&settings, &target_version_id);
                if mods_dir.is_none() {
                    return ApiResult::err(400, "请先安装一个游戏版本");
                }
                mods_dir.unwrap()
            }
        }
    };

    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        return ApiResult::err(500, &format!("无法创建目录: {}", e));
    }

    // 拉取版本数据 + 项目信息
    let (version_data, project_info) = if source == "curseforge" {
        // CurseForge：需要 modId + fileId，通过文件详情接口获取下载地址
        let cf_api_key = settings
            .get("curseforgeApiKey")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_CF_API_KEY);
        let mut cf_headers = reqwest::header::HeaderMap::new();
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&cf_api_key) {
            cf_headers.insert("x-api-key", v);
        }
        cf_headers.insert("Accept", reqwest::header::HeaderValue::from_static("application/json"));

        if version_id.is_empty() {
            return ApiResult::err(400, "CurseForge 下载需要指定版本文件 ID");
        }

        let file_detail_url = format!("{}/mods/{}/files/{}", CURSEFORGE_API, project_id, version_id);
        eprintln!("[resources][curseforge] 拉取文件详情: {}", file_detail_url);
        let file_data = match cached_fetch_json(file_detail_url, 0, Some(&cf_headers)).await {
            Ok(v) => v.get("data").cloned().unwrap_or(Value::Null),
            Err(e) => return ApiResult::err(502, &format!("获取文件信息失败: {}", e)),
        };
        if file_data.is_null() {
            return ApiResult::err(502, "未找到该版本文件信息，可能已被下架");
        }

        // CF hashes 是数组 [{algo, value}]，algo=1 表示 sha1
        let sha1 = file_data
            .get("hashes")
            .and_then(|h| h.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|x| x.get("algo").and_then(|a| a.as_u64()).unwrap_or(0) == 1)
            })
            .and_then(|h| h.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let game_versions = file_data
            .get("gameVersions")
            .and_then(|g| g.as_array())
            .cloned()
            .unwrap_or_default();

        // downloadUrl 为空时，根据 fileID(versionId) 和 fileName 构造 CurseForge CDN URL
        // （对齐原项目：部分文件 API 返回 downloadUrl 为 null，但 CDN 实际可访问）
        // URL 格式：https://edge.forgecdn.net/files/{fileID前4位}/{fileID剩余位}/{encodeURIComponent(fileName)}
        let raw_dl_url = file_data.get("downloadUrl").and_then(|u| u.as_str()).unwrap_or("").to_string();
        let raw_file_name = file_data.get("fileName").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let mut effective_url = raw_dl_url.clone();
        if effective_url.is_empty() && !raw_file_name.is_empty() && version_id.len() >= 5 {
            let id_str = version_id.clone();
            if id_str.chars().all(|c| c.is_ascii_digit()) {
                let part1 = id_str[..4].parse::<u64>().ok();
                let part2 = id_str[4..].parse::<u64>().ok();
                if let (Some(p1), Some(p2)) = (part1, part2) {
                    let encoded = urlencoding::encode(&raw_file_name);
                    effective_url = format!(
                        "https://edge.forgecdn.net/files/{}/{}/{}",
                        p1, p2, encoded
                    );
                    eprintln!(
                        "[resources][curseforge] downloadUrl 为空，已构造 CDN URL: {}:{} -> {}",
                        project_id, version_id, effective_url
                    );
                }
            }
        }

        let version_data = json!({
            "files": [{
                "url": effective_url,
                "filename": raw_file_name,
                "size": file_data.get("fileLength").and_then(|s| s.as_u64()).unwrap_or(0),
                "primary": true,
                "hashes": { "sha1": sha1 }
            }],
            "game_versions": game_versions
        });

        // modpack 需要项目名作为版本名
        let project_info: Value = if project_type == "modpack" {
            let p_url = format!("{}/mods/{}", CURSEFORGE_API, project_id);
            match cached_fetch_json(p_url, 60000, Some(&cf_headers)).await {
                Ok(v) => v.get("data").cloned().unwrap_or(Value::Null),
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        };

        (version_data, project_info)
    } else {
        // Modrinth（带镜像回退，对齐原项目 http.fetchJSON 行为，国内访问更快更稳）
        let version_data: Value = if !version_id.is_empty() {
            let url = format!("{}/version/{}", MODRINTH_API, version_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(v) => v,
                Err(_) => Value::Null,
            }
        } else {
            let url = format!("{}/project/{}/version?limit=1", MODRINTH_API, project_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(arr) => arr.as_array().and_then(|a| a.first()).cloned().unwrap_or(Value::Null),
                Err(_) => Value::Null,
            }
        };

        if version_data.is_null() {
            return ApiResult::err(502, "未找到版本信息，请检查网络连接或稍后重试");
        }

        let project_info: Value = if project_type == "modpack" {
            let url = format!("{}/project/{}", MODRINTH_API, project_id);
            match fetch_json_with_mirror(&url, None).await {
                Ok(v) => v,
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        };

        (version_data, project_info)
    };

    // 提取主文件
    let files = version_data.get("files").and_then(|f| f.as_array());
    let primary_file = files
        .and_then(|arr| arr.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false)))
        .or_else(|| files.and_then(|arr| arr.first()));

    let primary_file = match primary_file {
        Some(f) => f,
        None => return ApiResult::err(502, "未找到下载文件，该版本可能已被下架或不存在"),
    };

    let download_url = primary_file.get("url").and_then(|u| u.as_str()).unwrap_or("");
    let original_filename = primary_file.get("filename").and_then(|f| f.as_str()).unwrap_or("");
    let file_size = primary_file.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
    let expected_sha1 = primary_file
        .get("hashes")
        .and_then(|h| h.get("sha1"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    if download_url.is_empty() {
        return ApiResult::err(502, "下载链接为空，该资源可能暂时不可用");
    }

    // 安全文件名：过滤非法字符
    let default_name = format!("{}.jar", project_id);
    let raw_name = if !original_filename.is_empty() {
        original_filename
    } else {
        &default_name
    };
    let safe_name: String = raw_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    let final_name = if safe_name.is_empty() {
        format!("{}.jar", project_id)
    } else {
        safe_name
    };
    let dest_path = dest_dir.join(&final_name);

    // 解析 modpack 元数据
    let mc_version = if project_type == "modpack" {
        version_data
            .get("game_versions")
            .and_then(|g| g.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };
    let pack_name = if project_type == "modpack" {
        project_info
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or(&project_id)
            .to_string()
    } else {
        String::new()
    };

    // 捕获整合包封面图 URL（Modrinth 用 icon_url，CurseForge 用 logo 缩略图），
    // 导入后如果压缩包内没有根图标，则下载该封面作为版本图标
    let modpack_icon_url = if project_type == "modpack" {
        let mr_icon = project_info
            .get("icon_url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !mr_icon.is_empty() {
            mr_icon.to_string()
        } else {
            project_info
                .get("logo")
                .and_then(|l| l.get("thumbnailUrl"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    project_info
                        .get("logo")
                        .and_then(|l| l.get("url"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string()
        }
    } else {
        String::new()
    };

    // 创建下载会话
    let (session_id, cancel_flag) = resources_session::create_session(
        &final_name,
        file_size,
        &project_type,
        &project_id,
    );

    // 写入 modpack 元数据
    if project_type == "modpack" {
        resources_session::update_session_silent(&session_id, |s| {
            s.pack_name = pack_name.clone();
            s.mc_version = mc_version.clone();
        });
    }

    eprintln!(
        "[resources] 开始下载: {} → {} (size={}, sha1={})",
        download_url,
        dest_path.display(),
        file_size,
        if expected_sha1.is_empty() { "(none)" } else { expected_sha1 }
    );

    // spawn 后台下载任务
    let app_handle = app.clone();
    let session_id_for_progress = session_id.clone();
    let session_id_for_task = session_id.clone();
    let project_type_clone = project_type.clone();
    let dest_path_clone = dest_path.clone();
    let custom_name_clone = custom_name.clone();
    let modpack_icon_url_clone = modpack_icon_url.clone();
    let download_url_clone = download_url.to_string();
    let expected_sha1_clone = expected_sha1.to_string();
    // 整合包 mrpack 文件强制使用 china-first（镜像优先，官方兜底），
    // 避免用户设置成 auto 时先连 Modrinth 官方 CDN（国内 TTFB 慢、易限流）导致下载卡住。
    let download_source = if project_type == "modpack" {
        "china-first".to_string()
    } else {
        settings
            .get("downloadSource")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string()
    };

    tokio::spawn(async move {
        let _cancel = cancel_flag; // 持有 cancel_flag，便于扩展取消逻辑
        let progress_session_id = session_id_for_progress.clone();
        let on_progress: crate::download::ProgressCb = Arc::new(move |p: &crate::download::DownloadProgress| {
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

        // 调用单流下载（含镜像回退、SHA1 校验、续传、低速检测）
        let result = download_with_mirror(
            &download_url_clone,
            &dest_path_clone,
            if expected_sha1_clone.is_empty() { None } else { Some(&expected_sha1_clone) },
            if file_size > 0 { Some(file_size) } else { None },
            &download_source,
            300, // 5 分钟超时
            Some(on_progress),
        )
        .await;

        match result {
            Ok(()) => {
                if project_type_clone == "modpack" {
                    // 对齐原项目：下载完成后自动导入整合包（importModpackFromPath）
                    // 先更新为导入阶段，再调用 import_modpack 触发安装
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Install;
                        s.progress = 45;
                        s.message = "正在解析整合包...".to_string();
                        s.phase = "install".to_string();
                        if !s.files.is_empty() {
                            s.files[0].status = "downloading".to_string();
                            s.files[0].progress = 100;
                        }
                    });

                    let dest_str = dest_path_clone.to_string_lossy().to_string();
                    let import_result = modpack::import_modpack(
                        &app_handle,
                        &dest_str,
                        &custom_name_clone,
                        None,
                        Some(&session_id_for_task),
                    )
                    .await;

                    if import_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                        // 若压缩包内没有根图标，下载整合包封面作为版本图标
                        if !modpack_icon_url_clone.is_empty() {
                            if let Some(vid) = import_result.get("versionId").and_then(|v| v.as_str()) {
                                let icon_dest = crate::storage::resolve_data_dir()
                                    .join("versions")
                                    .join(vid)
                                    .join("icon.png");
                                if !icon_dest.exists() {
                                    eprintln!("[resources] 下载整合包图标: {} → {}", modpack_icon_url_clone, icon_dest.display());
                                    let _ = crate::download::download_with_mirror(
                                        &modpack_icon_url_clone,
                                        &icon_dest,
                                        None,
                                        None,
                                        &download_source,
                                        30,
                                        None,
                                    )
                                    .await;
                                }
                            }
                        }
                        let pack_name = import_result
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                            s.status = resources_session::ResourceStage::Completed;
                            s.progress = 100;
                            s.message = if pack_name.is_empty() {
                                format!("整合包 {} 安装完成！", s.file_name)
                            } else {
                                format!("整合包 \"{}\" 安装完成！", pack_name)
                            };
                            s.phase = "completed".to_string();
                            if !s.files.is_empty() {
                                s.files[0].status = "completed".to_string();
                                s.files[0].progress = 100;
                            }
                        });
                    } else {
                        let err = import_result
                            .get("error")
                            .and_then(|e| e.as_str())
                            .unwrap_or("未知错误")
                            .to_string();
                        resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                            s.status = resources_session::ResourceStage::Failed;
                            s.progress = 100;
                            s.message = format!("整合包导入失败: {}", err);
                            s.phase = "failed".to_string();
                            if !s.files.is_empty() {
                                s.files[0].status = "failed".to_string();
                            }
                        });
                    }
                } else {
                    resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                        s.status = resources_session::ResourceStage::Completed;
                        s.progress = 100;
                        s.message = format!("{} 下载完成！", s.file_name);
                        s.phase = "completed".to_string();
                        if !s.files.is_empty() {
                            s.files[0].status = "completed".to_string();
                            s.files[0].progress = 100;
                        }
                    });
                }
                // 下载完成后保留会话 60 秒，让前端有机会读取最终状态
                let sid = session_id_for_task.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    resources_session::remove_session(&sid);
                });
            }
            Err(e) => {
                eprintln!("[resources] 下载失败: {}", e);
                resources_session::update_session(&app_handle, &session_id_for_task, |s| {
                    s.status = resources_session::ResourceStage::Failed;
                    s.progress = 100;
                    s.message = format!("下载失败: {}", e);
                    if !s.files.is_empty() {
                        s.files[0].status = "failed".to_string();
                    }
                });
                // 失败也保留 60 秒便于前端读取错误
                let sid = session_id_for_task.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    resources_session::remove_session(&sid);
                });
            }
        }
    });

    ApiResult::ok(json!({
        "success": true,
        "sessionId": session_id,
        "fileName": final_name,
        "destPath": dest_path.to_string_lossy()
    }))
}

/// GET /api/resources/download-status — 查询下载进度
///
/// 查询参数：
///   - sessionId: 下载会话 ID
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

/// POST /api/resources/download-cancel — 取消下载
///
/// 请求体：
///   - sessionId: 下载会话 ID
fn handle_download_cancel(body: &Option<Value>) -> ApiResult {
    let session_id = body
        .as_ref()
        .and_then(|b| b.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if session_id.is_empty() {
        return ApiResult::err(400, "Missing sessionId");
    }

    if resources_session::cancel_session(session_id) {
        ApiResult::ok(json!({ "success": true }))
    } else {
        ApiResult::err(404, "会话不存在或已过期")
    }
}

/// 解析版本子目录（如 modpacks/resourcepacks）
/// 复刻原项目 versions.getVersionSubDir
fn resolve_sub_dir(settings: &Value, version_id: &str, subfolder: &str) -> PathBuf {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    // 版本隔离时：versions/<id>/<subfolder>
    let version_isolation = settings
        .get("versionIsolation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if !version_id.is_empty() && version_isolation {
        return versions_dir.join(version_id).join(subfolder);
    }

    // 否则：gameDir/<subfolder>
    let game_dir = settings
        .get("gameDir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.clone());
    game_dir.join(subfolder)
}

/// 解析版本 mods 目录
/// 复刻原项目 versions.getVersionModsDir
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
