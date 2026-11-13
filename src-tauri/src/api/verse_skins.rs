// api/verse_skins.rs — Verse 皮肤站（Blessing Skin Server）中继
//
// 皮肤站公开接口：
//   GET /skinlib/list?page=N      公开皮肤库分页列表（tid / name / type / uploader / likes / nickname）
//   GET /skinlib/info/{tid}      单个皮肤详情（含纹理 hash、type）
//   GET /textures/{hash}         皮肤纹理 PNG
//   GET /preview/{tid}           皮肤 3D 效果模型预览图（前端 <img> 直接加载，无需中继）
//
// 皮肤站不返回 CORS 头，WebView 里无法直接 fetch 列表与纹理，因此由 Rust 侧代理。
// 本模块只负责「取数据」，真正的皮肤写入仍复用 skins 模块已有的
// /api/upload-skin、/api/ms-skins/import、/api/ms-skins/apply 接口。
use super::ApiResult;
use base64::Engine;
use serde_json::{json, Value};

const SKIN_SITE: &str = "https://skin.verselauncher.cn";

/// api_proxy 路由入口
pub async fn handle(
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    if !path.starts_with("/api/verse-skins") {
        return None;
    }
    match (method.to_uppercase().as_str(), path) {
        ("GET", "/api/verse-skins/list") => Some(list_skins(params).await),
        ("POST", "/api/verse-skins/texture") => Some(fetch_texture(body).await),
        _ => Some(ApiResult::err(404, &format!("未实现: {path}"))),
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn page_of(params: &Option<Value>) -> u64 {
    let raw = params.as_ref().and_then(|p| p.get("page"));
    let value = match raw {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(1),
        Some(Value::String(s)) => s.parse::<u64>().unwrap_or(1),
        _ => 1,
    };
    value.clamp(1, 100)
}

/// GET /api/verse-skins/list?page=N → 公开皮肤库分页
async fn list_skins(params: &Option<Value>) -> ApiResult {
    let page = page_of(params);
    let url = format!("{SKIN_SITE}/skinlib/list?page={page}");
    let response = match client().get(&url).send().await {
        Ok(r) => r,
        Err(e) => return ApiResult::err(502, &format!("皮肤站连接失败：{e}")),
    };
    if !response.status().is_success() {
        return ApiResult::err(502, &format!("皮肤站返回 HTTP {}", response.status()));
    }
    let data: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => return ApiResult::err(502, &format!("皮肤站响应解析失败：{e}")),
    };

    let skins = data
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let last_page = data.get("last_page").and_then(Value::as_u64).unwrap_or(1);
    let total = data.get("total").and_then(Value::as_u64).unwrap_or(0);

    ApiResult::ok(json!({
        "success": true,
        "skins": skins,
        "page": page,
        "lastPage": last_page,
        "total": total
    }))
}

/// POST /api/verse-skins/texture {tid} → 下载纹理 PNG 并以 base64 返回
/// 返回的 model 为皮肤站 type 对应值：alex → slim，steve → default。
async fn fetch_texture(body: &Option<Value>) -> ApiResult {
    let Some(tid) = body
        .as_ref()
        .and_then(|b| b.get("tid"))
        .and_then(Value::as_u64)
    else {
        return ApiResult::err(400, "缺少有效的皮肤 tid");
    };

    let info_url = format!("{SKIN_SITE}/skinlib/info/{tid}");
    let info_resp = match client().get(&info_url).send().await {
        Ok(r) => r,
        Err(e) => return ApiResult::err(502, &format!("皮肤详情获取失败：{e}")),
    };
    if !info_resp.status().is_success() {
        return ApiResult::err(502, &format!("皮肤详情返回 HTTP {}", info_resp.status()));
    }
    let info: Value = match info_resp.json().await {
        Ok(v) => v,
        Err(e) => return ApiResult::err(502, &format!("皮肤详情解析失败：{e}")),
    };

    let hash = info.get("hash").and_then(Value::as_str).unwrap_or("");
    // hash 会拼进 URL，严格校验，避免把不可信数据带进请求路径
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return ApiResult::err(502, "皮肤站返回的纹理 hash 无效");
    }
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Verse 皮肤站皮肤");
    let model = if info.get("type").and_then(Value::as_str) == Some("alex") {
        "slim"
    } else {
        "default"
    };

    let tex_url = format!("{SKIN_SITE}/textures/{hash}");
    let tex_resp = match client().get(&tex_url).send().await {
        Ok(r) => r,
        Err(e) => return ApiResult::err(502, &format!("皮肤纹理下载失败：{e}")),
    };
    if !tex_resp.status().is_success() {
        return ApiResult::err(502, &format!("皮肤纹理返回 HTTP {}", tex_resp.status()));
    }
    let bytes = match tex_resp.bytes().await {
        Ok(b) => b,
        Err(e) => return ApiResult::err(502, &format!("皮肤纹理读取失败：{e}")),
    };
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return ApiResult::err(502, "皮肤站返回的纹理不是 PNG");
    }
    // 皮肤纹理通常只有几 KB，过大的响应视为异常，避免占用过多内存
    if bytes.len() > 2 * 1024 * 1024 {
        return ApiResult::err(502, "皮肤纹理体积异常");
    }

    let file_base64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_ref());
    ApiResult::ok(json!({
        "success": true,
        "fileBase64": file_base64,
        "model": model,
        "name": name,
        "hash": hash
    }))
}
