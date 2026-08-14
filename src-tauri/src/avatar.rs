// avatar.rs — 账号头像处理 + 皮肤纹理
// 兼容原项目 /api/avatar 路由
//
// 实现策略（与原项目 server/skins.js fetchAvatarData 一致）：
//   1. 离线账号：直接读取项目内置 img/steve_head.png，返回 base64 data URL
//   2. 账号 skinUrl 字段（微软/外置账号登录时从 Mojang 拿到的 texture URL）：
//      HTTP 拉取整张皮肤 PNG，标记 is_full_skin=true（前端会用 canvas 裁剪头像）
//   3. 外置认证服务器 /skin/{username}.png：拉取整皮，is_full_skin=true
//   4. 公共 Avatar 服务（crafatar / minotar / mc-heads）：拿 64x64 头像
//   5. 全部失败回退到内置 Steve 头
//
// 前端通过 invoke('get_avatar', { uuid, serverUrl, username, offline }) 调用
// preload 层负责拦截 /api/avatar 的 fetch 请求转成 invoke

use serde_json::{json, Value};
use std::time::Duration;

use crate::storage;
use crate::utils;

/// 公共 Avatar 服务列表（与原项目 server/context.js 第 264-269 行一致）
/// 这些服务返回 64x64 头像（不是整张皮肤）
const AVATAR_SERVICES: &[&str] = &[
    "https://minotar.net/helm/{uuid}.png",
    "https://mc-heads.net/avatar/{uuid}/64",
    "https://crafatar.com/avatars/{uuid}?size=64&overlay",
    "https://visage.surgeplay.com/face/64/{uuid}",
];

/// PNG 文件魔数（前 4 字节：89 50 4E 47）
const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47];

/// JPEG 文件魔数（前 3 字节：FF D8 FF）
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];

/// 内置 steve_head.png，编译时嵌入到二进制里
/// 用 include_bytes! 在编译期把 src-tauri/resources/steve_head.png 直接嵌入 exe
/// 这样无论 exe 在哪运行都能找到，不依赖外部文件
const STEVE_HEAD_PNG: &[u8] = include_bytes!("../resources/steve_head.png");

/// 返回内置 steve_head.png 的 data URL
/// 编译时已嵌入二进制，运行时永远可用
fn steve_head_data_url(_app: &tauri::AppHandle) -> Option<String> {
    let bytes = STEVE_HEAD_PNG;
    if bytes.len() > 8 && bytes.starts_with(PNG_MAGIC) {
        return Some(crate::utils::bytes_to_data_url(bytes, "image/png"));
    }
    None
}

/// 通用 HTTP 拉取字节（带超时）
/// 用于拉取 skinUrl 指向的整张皮肤 PNG
async fn fetch_url_bytes(url: &str) -> Option<Vec<u8>> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() > 8 && bytes.starts_with(PNG_MAGIC) {
        Some(bytes.to_vec())
    } else {
        None
    }
}

/// 调用公共 Avatar 服务拉取头像
/// uuid 需要去掉横杠的 32 位无符号形式
async fn fetch_avatar_from_services(uuid: &str) -> Option<Vec<u8>> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    for tmpl in AVATAR_SERVICES {
        let url = tmpl.replace("{uuid}", uuid);
        match client.get(&url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        // 简单校验是不是 PNG/JPEG
                        if bytes.len() > 8
                            && (bytes.starts_with(PNG_MAGIC) || bytes.starts_with(JPEG_MAGIC))
                        {
                            return Some(bytes.to_vec());
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }
    None
}

/// 从外置认证服务器的传统 API 拉皮肤：{serverUrl}/skin/{username}.png
async fn fetch_skin_from_server(server_url: &str, username: &str) -> Option<Vec<u8>> {
    if server_url.is_empty() || username.is_empty() {
        return None;
    }
    let url = format!("{}/skin/{}.png", clean_server_url(server_url), username);
    fetch_url_bytes(&url).await
}

/// 清理服务器 URL：去掉 @@@/@@ 分隔后缀、尾部斜杠、以及 /api/yggdrasil 前缀
fn clean_server_url(url: &str) -> String {
    let mut s = url.split("@@@").next().unwrap_or(url).to_string();
    s = s.split("@@").next().unwrap_or(&s).to_string();
    let mut s = s.trim_end_matches('/').to_string();
    if s.ends_with("/api/yggdrasil") {
        s.truncate(s.len() - "/api/yggdrasil".len());
        s = s.trim_end_matches('/').to_string();
    }
    s
}

/// 把 32 位无横杠 UUID 转成带横杠格式
fn dashed_uuid(uuid: &str) -> String {
    let c = uuid.replace('-', "");
    if c.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &c[0..8],
            &c[8..12],
            &c[12..16],
            &c[16..20],
            &c[20..32]
        )
    } else {
        uuid.to_string()
    }
}

/// 从 Session Server 的 profile 接口解析出皮肤纹理 URL（现代 Yggdrasil 皮肤站标准方式）
/// 请求 {server}/sessionserver/session/minecraft/profile/{uuid}，解析 textures 的 base64
async fn fetch_skin_url_from_session(uuid: &str, server_url: &str) -> Option<String> {
    if server_url.is_empty() {
        return None;
    }
    let clean = clean_server_url(server_url);
    let url = format!(
        "{}/sessionserver/session/minecraft/profile/{}",
        clean,
        dashed_uuid(uuid)
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let props = v.get("properties")?.as_array()?;
    let textures_val = props
        .iter()
        .find(|p| p.get("name").and_then(|x| x.as_str()) == Some("textures"))?
        .get("value")?
        .as_str()?;
    use base64::Engine;
    let dec = base64::engine::general_purpose::STANDARD
        .decode(textures_val)
        .ok()?;
    let tv: Value = serde_json::from_slice(&dec).ok()?;
    tv.get("textures")?
        .get("SKIN")?
        .get("url")?
        .as_str()
        .map(String::from)
}

/// 从 CSL（统一通行证）API 拉皮肤：{server}/csl/{username}.json → /textures/{hash}
async fn fetch_skin_from_csl(server_url: &str, username: &str) -> Option<Vec<u8>> {
    if server_url.is_empty() || username.is_empty() {
        return None;
    }
    let clean = clean_server_url(server_url);
    let csl_url = format!("{}/csl/{}.json", clean, username);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
        .ok()?;
    let resp = client.get(&csl_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let skins = v.get("skins")?;
    // 对象格式：{ slim: hash, default: hash, steve: hash }
    let skin_hash = if let Some(o) = skins.as_object() {
        o.get("slim")
            .or_else(|| o.get("default"))
            .or_else(|| o.get("steve"))
            .and_then(|x| x.as_str())
            .map(String::from)
    } else if let Some(arr) = skins.as_array() {
        // 数组格式：找 type 为 skin/default 的项，取 hash 或 url
        arr.iter()
            .find(|s| {
                s.get("type")
                    .and_then(|x| x.as_str())
                    .map(|t| t == "skin" || t == "default")
                    .unwrap_or(false)
            })
            .and_then(|s| {
                s.get("hash")
                    .or_else(|| s.get("id"))
                    .and_then(|x| x.as_str())
                    .map(String::from)
            })
    } else {
        None
    }?;
    let tex_url = format!("{}/textures/{}", clean, skin_hash);
    fetch_url_bytes(&tex_url).await
}

/// 头像返回结果
#[derive(serde::Serialize)]
pub struct AvatarResult {
    /// data URL（如 data:image/png;base64,xxxx）或空字符串（失败）
    pub data_url: String,
    /// 是否为整张皮肤图（true 时前端会自动裁剪头像）
    pub is_full_skin: bool,
    /// 是否成功
    pub success: bool,
}

impl Default for AvatarResult {
    fn default() -> Self {
        Self {
            data_url: String::new(),
            is_full_skin: false,
            success: false,
        }
    }
}

/// Tauri 命令：get_avatar
/// 前端通过 invoke('get_avatar', { uuid, serverUrl, username, offline }) 调用
/// 注：rename_all = "camelCase" 让前端可用 camelCase 参数名（serverUrl），
///     Rust 端参数仍是 snake_case（server_url）
#[tauri::command(rename_all = "camelCase")]
pub async fn get_avatar(
    app: tauri::AppHandle,
    uuid: String,
    server_url: Option<String>,
    username: Option<String>,
    offline: Option<bool>,
) -> Value {
    println!("[avatar] get_avatar called, uuid={}, server_url={:?}, username={:?}, offline={:?}", uuid, server_url, username, offline);
    let clean_uuid = uuid.replace('-', "");
    if clean_uuid.is_empty() {
        println!("[avatar] empty uuid, returning empty");
        return json!({
            "success": false,
            "data_url": "",
            "is_full_skin": false
        });
    }

    let is_offline = offline.unwrap_or(false);
    let server_url = server_url.unwrap_or_default();
    let username = username.unwrap_or_default();
    println!("[avatar] is_offline={}, server_url={}, username={}", is_offline, server_url, username);

    // 1. 离线账号：直接返回内置 Steve 头像
    if is_offline && server_url.is_empty() {
        if let Some(data_url) = steve_head_data_url(&app) {
            return json!({
                "success": true,
                "data_url": data_url,
                "is_full_skin": false
            });
        }
        // steve_head.png 不存在，回退空
        return json!({
            "success": false,
            "data_url": "",
            "is_full_skin": false
        });
    }

    // 2. 读取账号的 skinUrl 字段（微软/外置账号登录后从 Mojang 拿到的纹理 URL）
    //   这是登录时从 https://api.minecraftservices.com/minecraft/profile 拿到的
    //   textures.SKIN.url 字段，原项目保存在 acc.skinUrl
    //   skinUrl 指向的是整张皮肤 PNG，所以 is_full_skin=true（前端会裁剪头像）
    let mut stored_skin_url = String::new();
    let accounts = crate::storage::load_accounts();
    if let Some(arr) = accounts.as_array() {
        if let Some(acc) = arr
            .iter()
            .find(|a| crate::utils::get_str(a, "uuid").replace('-', "") == clean_uuid)
        {
            stored_skin_url = crate::utils::get_str(acc, "skinUrl");
        }
    }
    println!("[avatar] stored_skin_url={}", if stored_skin_url.is_empty() { "(empty)" } else { "has value" });

    if !stored_skin_url.is_empty() {
        println!("[avatar] fetching skin from skinUrl...");
        if let Some(skin_bytes) = fetch_url_bytes(&stored_skin_url).await {
            println!("[avatar] skinUrl fetch success, bytes={}", skin_bytes.len());
            let data_url = crate::utils::bytes_to_data_url(&skin_bytes, "image/png");
            return json!({
                "success": true,
                "data_url": data_url,
                "is_full_skin": true
            });
        }
        println!("[avatar] skinUrl fetch failed, trying other sources");
        // skinUrl 拉取失败继续尝试其他数据源
    }

    // 3. 外置服务器 + 用户名：尝试从 /skin/{username}.png 拉整皮
    if !server_url.is_empty() && !username.is_empty() {
        println!("[avatar] trying external server skin");
        if let Some(skin_bytes) = fetch_skin_from_server(&server_url, &username).await {
            let data_url = crate::utils::bytes_to_data_url(&skin_bytes, "image/png");
            return json!({
                "success": true,
                "data_url": data_url,
                "is_full_skin": true
            });
        }
        // 失败继续尝试其他数据源
    }

    // 3.5 外置服务器 CSL API：{server}/csl/{username}.json → /textures/{hash}
    if !server_url.is_empty() && !username.is_empty() {
        println!("[avatar] trying CSL API");
        if let Some(skin_bytes) = fetch_skin_from_csl(&server_url, &username).await {
            let data_url = crate::utils::bytes_to_data_url(&skin_bytes, "image/png");
            return json!({
                "success": true,
                "data_url": data_url,
                "is_full_skin": true
            });
        }
    }

    // 3.6 外置服务器 Session Server（Yggdrasil 标准）：解析 profile 里的 textures 皮肤 URL
    if !server_url.is_empty() {
        println!("[avatar] trying session server");
        if let Some(skin_texture_url) = fetch_skin_url_from_session(&uuid, &server_url).await {
            if let Some(skin_bytes) = fetch_url_bytes(&skin_texture_url).await {
                let data_url = crate::utils::bytes_to_data_url(&skin_bytes, "image/png");
                return json!({
                    "success": true,
                    "data_url": data_url,
                    "is_full_skin": true
                });
            }
        }
    }

    // 4. 无外置服务器时，尝试公共 Avatar 服务（拿 64x64 头像）
    //    第三方皮肤站的 uuid 在 Mojang 公共服务里查不到，故仅在无 serverUrl 时才尝试
    if server_url.is_empty() {
        println!("[avatar] trying public avatar services");
        if let Some(avatar_bytes) = fetch_avatar_from_services(&clean_uuid).await {
            println!("[avatar] public avatar fetch success, bytes={}", avatar_bytes.len());
            let mime = if avatar_bytes.starts_with(PNG_MAGIC) {
                "image/png"
            } else {
                "image/jpeg"
            };
            let data_url = crate::utils::bytes_to_data_url(&avatar_bytes, mime);
            return json!({
                "success": true,
                "data_url": data_url,
                "is_full_skin": false
            });
        }
    }

    // 5. 全部失败，回退 Steve
    println!("[avatar] all sources failed, returning Steve head");
    if let Some(data_url) = steve_head_data_url(&app) {
        return json!({
            "success": true,
            "data_url": data_url,
            "is_full_skin": false
        });
    }

    println!("[avatar] Steve head also failed, returning empty");
    json!({
        "success": false,
        "data_url": "",
        "is_full_skin": false
    })
}

// ============== 皮肤纹理（用于 3D 模型） ==============

/// 皮肤纹理返回结果
#[derive(serde::Serialize)]
pub struct SkinTextureResult {
    /// data URL（如 data:image/png;base64,xxxx）或空字符串（失败）
    pub data_url: String,
    /// 皮肤模型（"default" 或 "slim"）
    pub model: String,
    /// 是否成功
    pub success: bool,
}

/// Tauri 命令：get_skin_texture
/// 前端通过 invoke('get_skin_texture', { uuid, serverUrl, username }) 调用
/// 返回整张皮肤纹理的 data URL，供 skinview3d 使用
#[tauri::command(rename_all = "camelCase")]
pub async fn get_skin_texture(
    uuid: String,
    server_url: Option<String>,
    username: Option<String>,
) -> SkinTextureResult {
    let server_url = server_url.unwrap_or_default();
    let username = username.unwrap_or_default();

    if uuid.is_empty() {
        return SkinTextureResult { data_url: String::new(), model: "default".to_string(), success: false };
    }

    let clean_uuid = uuid.replace('-', "");

    // 1. 优先用账号本地存储的皮肤文件（skinFile）或 skinUrl（微软账号）
    let accounts = storage::load_accounts();
    if let Some(arr) = accounts.as_array() {
        if let Some(acc) = arr
            .iter()
            .find(|a| utils::get_str(a, "uuid").replace('-', "") == clean_uuid)
        {
            let skin_file = utils::get_str(acc, "skinFile");
            let skin_model = utils::get_str(acc, "skinModel");

            if !skin_file.is_empty() {
                // 内置皮肤
                if let Some(_) = get_builtin_skin_bytes(&skin_file) {
                    // 内置皮肤直接读取资源文件
                    let builtin_path = storage::resolve_data_dir().join("img").join(&skin_file);
                    if let Ok(bytes) = std::fs::read(&builtin_path) {
                        let data_url = utils::bytes_to_data_url(&bytes, "image/png");
                        return SkinTextureResult { data_url, model: skin_model, success: true };
                    }
                }
                // 用户自定义皮肤
                let custom_path = storage::resolve_data_dir().join("img").join(&skin_file);
                if let Ok(bytes) = std::fs::read(&custom_path) {
                    let data_url = utils::bytes_to_data_url(&bytes, "image/png");
                    return SkinTextureResult { data_url, model: skin_model, success: true };
                }
            }

            // 微软/外置账号的 skinUrl 字段
            let skin_url = utils::get_str(acc, "skinUrl");
            if !skin_url.is_empty() {
                if let Some(bytes) = fetch_url_bytes(&skin_url).await {
                    let data_url = utils::bytes_to_data_url(&bytes, "image/png");
                    return SkinTextureResult { data_url, model: skin_model, success: true };
                }
            }
        }
    }

    // 2. 外置服务器：从 /skin/{username}.png 拉取
    if !server_url.is_empty() && !username.is_empty() {
        let url = format!("{}/skin/{}.png", clean_server_url(&server_url), username);
        if let Some(bytes) = fetch_url_bytes(&url).await {
            let data_url = utils::bytes_to_data_url(&bytes, "image/png");
            return SkinTextureResult { data_url, model: "default".to_string(), success: true };
        }
    }

    // 2.5 外置服务器 CSL API
    if !server_url.is_empty() && !username.is_empty() {
        if let Some(bytes) = fetch_skin_from_csl(&server_url, &username).await {
            let data_url = utils::bytes_to_data_url(&bytes, "image/png");
            return SkinTextureResult { data_url, model: "default".to_string(), success: true };
        }
    }

    // 2.6 外置服务器 Session Server（Yggdrasil 标准）
    if !server_url.is_empty() {
        if let Some(skin_texture_url) = fetch_skin_url_from_session(&uuid, &server_url).await {
            if let Some(bytes) = fetch_url_bytes(&skin_texture_url).await {
                let data_url = utils::bytes_to_data_url(&bytes, "image/png");
                return SkinTextureResult { data_url, model: "default".to_string(), success: true };
            }
        }
    }

    // 3. 无外置服务器时，公共皮肤服务（整皮）
    if server_url.is_empty() {
        let skin_services: &[&str] = &[
            "https://mc-heads.net/skin/{uuid}",
            "https://crafatar.com/skins/{uuid}",
            "https://minotar.net/skin/{uuid}",
        ];
        for tmpl in skin_services {
            let url = tmpl.replace("{uuid}", &clean_uuid);
            if let Some(bytes) = fetch_url_bytes(&url).await {
                let data_url = utils::bytes_to_data_url(&bytes, "image/png");
                return SkinTextureResult { data_url, model: "default".to_string(), success: true };
            }
        }
    }

    // 4. 全部失败
    SkinTextureResult { data_url: String::new(), model: "default".to_string(), success: false }
}

/// 获取内置皮肤字节（从 storage 读取）
fn get_builtin_skin_bytes(skin_file: &str) -> Option<Vec<u8>> {
    let builtin_skins = ["steve.png", "alex.png", "zombie.png", "enderman.png", "creeper.png"];
    if builtin_skins.contains(&skin_file) {
        let path = storage::resolve_data_dir().join("img").join(skin_file);
        std::fs::read(&path).ok()
    } else {
        None
    }
}
