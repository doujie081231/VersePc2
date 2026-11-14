// api/verse_skins.rs — Verse 皮肤站「受控接口」中继
//
// 上游只调用 /api/verse-skins（必须带 X-Api-Key），不再直接打站点自带的
// /skinlib/list、/skinlib/info/{tid}：那两个是 Blessing Skin 网页皮肤库的
// 公开接口，不带任何凭据就能拉走全站皮肤数据，会绕过开发者 Key 的调用统计。
//
//   GET {SKIN_SITE}/api/verse-skins?page=N&limit=24&sort=new
//     X-Api-Key: <应用级 Key>
//     → { page, limit, total, items: [{ tid, name, model, url, likes, uploaded_at }] }
//
// 对前端保持原来的本地契约不变：
//   GET  /api/verse-skins/list?page=N → { success, skins, page, lastPage, total }
//   POST /api/verse-skins/texture     → { success, fileBase64, model, name, hash }
//   skins[] 里带上服务端下发的 url，换肤时原样回传，无需再查一次详情。
//
// 卡片预览图 /preview/{tid} 与纹理 PNG 是站点公开静态资源（<img> 无法带
// 自定义请求头），皮肤站网页端同样依赖它们，因此不纳入 Key 保护；
// 受保护的「皮肤目录数据」由本模块的列表请求统一取回。
use super::ApiResult;
use base64::Engine;
use serde_json::{json, Value};

const SKIN_SITE: &str = "https://skin.verselauncher.cn";
const SKIN_HOST: &str = "skin.verselauncher.cn";
/// 前端每页卡片数；同时作为受控接口的 limit（服务端上限 48）
const PAGE_LIMIT: u64 = 24;

/// 应用级 API Key（uid=0，只能读列表，不能改任何人的角色）。
/// 与 Android 端 SkinLibraryAppKey 是同一把；每字节异或 0x5A 存放，
/// 避免明文出现在二进制字符串表里。客户端内置凭据无法真正保密
/// （CurseForge 的 key 同样处境），服务端可随时轮换。
const APP_KEY_MASK: u8 = 0x5A;
const APP_KEY_XOR: [u8; 51] = [
    44, 49, 5, 111, 63, 111, 110, 107, 111, 59,
    63, 60, 110, 106, 111, 59, 99, 56, 60, 105,
    56, 99, 108, 108, 99, 63, 98, 111, 62, 109,
    110, 98, 111, 63, 111, 98, 60, 110, 56, 59,
    98, 60, 59, 105, 60, 111, 57, 60, 56, 99, 99,
];

fn app_key() -> String {
    APP_KEY_XOR
        .iter()
        .map(|b| (*b ^ APP_KEY_MASK) as char)
        .collect()
}

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

/// 校验纹理直链并取出 hash，拒绝任何非本站地址。
/// url 来自前端回传，属于不可信输入，这里必须收窄，避免本地服务被当成任意 URL 代理。
fn texture_hash(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some(SKIN_HOST)
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let hash = parsed.path().strip_prefix("/textures/")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(hash.to_string())
}

/// GET /api/verse-skins/list?page=N → 受控接口列表，适配成前端原有字段
async fn list_skins(params: &Option<Value>) -> ApiResult {
    let page = page_of(params);
    let url = format!("{SKIN_SITE}/api/verse-skins?page={page}&limit={PAGE_LIMIT}&sort=new");
    let response = match client()
        .get(&url)
        .header("X-Api-Key", app_key())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return ApiResult::err(502, &format!("皮肤站连接失败：{e}")),
    };

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return ApiResult::err(502, "皮肤站 API Key 无效或已撤销，请更新启动器");
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return ApiResult::err(502, "皮肤站请求过于频繁，请稍后再试");
    }
    if !status.is_success() {
        return ApiResult::err(502, &format!("皮肤站返回 HTTP {status}"));
    }
    let data: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => return ApiResult::err(502, &format!("皮肤站响应解析失败：{e}")),
    };

    let total = data.get("total").and_then(Value::as_u64).unwrap_or(0);
    let limit = data
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(PAGE_LIMIT)
        .max(1);
    let last_page = total.div_ceil(limit).max(1);
    let items = data
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let skins: Vec<Value> = items
        .iter()
        .map(|item| {
            let slim = item.get("model").and_then(Value::as_str) == Some("slim");
            json!({
                "tid": item.get("tid").and_then(Value::as_u64).unwrap_or(0),
                "name": item.get("name").and_then(Value::as_str).unwrap_or("未命名"),
                // 前端旧字段 type 用 steve/alex，受控接口用 classic/slim
                "type": if slim { "alex" } else { "steve" },
                "model": if slim { "slim" } else { "default" },
                "likes": item.get("likes").and_then(Value::as_u64).unwrap_or(0),
                // 受控接口刻意不返回上传者信息，前端会显示「匿名」
                "nickname": "",
                "url": item.get("url").and_then(Value::as_str).unwrap_or(""),
                "uploaded_at": item.get("uploaded_at").and_then(Value::as_str).unwrap_or("")
            })
        })
        .collect();

    ApiResult::ok(json!({
        "success": true,
        "skins": skins,
        "page": page,
        "lastPage": last_page,
        "total": total
    }))
}

/// POST /api/verse-skins/texture {tid, url, name, model} → 下载纹理 PNG 并以 base64 返回
/// 返回的 model 为 slim / default，与账户域上传接口一致。
async fn fetch_texture(body: &Option<Value>) -> ApiResult {
    let Some(body) = body.as_ref() else {
        return ApiResult::err(400, "缺少皮肤参数");
    };
    let tex_url = body.get("url").and_then(Value::as_str).unwrap_or("");
    let Some(hash) = texture_hash(tex_url) else {
        return ApiResult::err(400, "皮肤纹理地址无效，请从皮肤库列表重新选择");
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Verse 皮肤站皮肤");
    let model = match body.get("model").and_then(Value::as_str) {
        Some("slim") => "slim",
        _ => "default",
    };

    let tex_resp = match client()
        .get(tex_url)
        .header("X-Api-Key", app_key())
        .send()
        .await
    {
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
