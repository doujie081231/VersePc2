// api/skins.rs — 皮肤管理 API 路由
//
// 路由清单：
//   ===== 通用皮肤 =====
//   GET  /api/default-skins      列出内置默认皮肤（Steve/Alex/Zombie/Enderman/Creeper）
//   GET  /api/skin-head           获取皮肤头部 PNG（按 id 或 file 参数）
//   POST /api/set-account-skin    为账号设置默认皮肤
//   POST /api/upload-skin         上传自定义皮肤 PNG（multipart 或 base64）
//   GET  /api/skin-texture        获取整张皮肤图（含 model 头）
//   POST /api/save-avatar         保存头像文件（data URL → PNG/JPG）
//   GET  /api/clear-avatar        清除已保存的头像文件
//
//   ===== 微软账号本地皮肤库 =====
//   存储：DATA_DIR/ms-skins/<accountId>/meta.json + <accountId>/skin_xxx.png
//   GET  /api/ms-skins/local      列出账号本地皮肤库
//   GET  /api/ms-skins/file       获取本地皮肤文件 PNG
//   POST /api/ms-skins/import     导入皮肤到本地库（base64 → PNG）
//   POST /api/ms-skins/apply      应用本地皮肤到 Mojang 官方
//   POST /api/ms-skins/delete     删除本地皮肤
//
// PNG 校验：仅校验 PNG 魔数（89 50 4E 47），不做尺寸校验和缩放
//   理由：Minecraft 1.8+ 会自动适配非标准尺寸皮肤，sharp 引入过重

use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

use crate::api::ApiResult;
use crate::auth;
use crate::storage;
use crate::utils;

// ============== 皮肤操作日志 ==============
// 追加写入 DATA_DIR/skin.log，记录上传/切换皮肤的关键操作，便于排查
fn write_skin_log(msg: &str) {
    let dir = storage::resolve_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let ts = utils::now_iso();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("skin.log"))
    {
        let _ = std::io::Write::write_all(&mut f, format!("[{}] {}\n", ts, msg).as_bytes());
    }
    println!("[skin] {}", msg);
}

// ============== 内置皮肤资源 ==============
// 编译时嵌入到二进制，运行时永远可用（不依赖前端 dist 构建）
// 文件来源：src-tauri/resources/*.png

const STEVE_HEAD_PNG: &[u8] = include_bytes!("../../resources/steve_head.png");
const STEVE_SKIN_PNG: &[u8] = include_bytes!("../../resources/steve_skin.png");
const SKIN_ALEX_PNG: &[u8] = include_bytes!("../../resources/skin_alex.png");
const SKIN_CREEPER_PNG: &[u8] = include_bytes!("../../resources/skin_creeper.png");
const SKIN_ENDERMAN_PNG: &[u8] = include_bytes!("../../resources/skin_enderman.png");
const SKIN_ZOMBIE_PNG: &[u8] = include_bytes!("../../resources/skin_zombie.png");

/// PNG 文件魔数（前 4 字节）
const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47];

/// 内置默认皮肤清单
fn builtin_default_skins() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    // (id, name, file, model)
    vec![
        ("steve", "Steve", "steve_skin.png", "default"),
        ("alex", "Alex", "skin_alex.png", "slim"),
        ("zombie", "Zombie", "skin_zombie.png", "default"),
        ("enderman", "Enderman", "skin_enderman.png", "default"),
        ("creeper", "Creeper", "skin_creeper.png", "default"),
    ]
}

/// 按 file 名获取内置皮肤字节
fn get_builtin_skin_bytes(file: &str) -> Option<&'static [u8]> {
    match file {
        "steve_head.png" => Some(STEVE_HEAD_PNG),
        "steve_skin.png" => Some(STEVE_SKIN_PNG),
        "skin_alex.png" => Some(SKIN_ALEX_PNG),
        "skin_creeper.png" => Some(SKIN_CREEPER_PNG),
        "skin_enderman.png" => Some(SKIN_ENDERMAN_PNG),
        "skin_zombie.png" => Some(SKIN_ZOMBIE_PNG),
        _ => None,
    }
}

/// 校验字节是否为 PNG（前 4 字节魔数）
fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes.starts_with(PNG_MAGIC)
}

// ============== 路由分发 ==============

/// 处理皮肤相关路由
pub async fn handle(
    _app: &tauri::AppHandle,
    method: &str,
    path: &str,
    params: &Option<Value>,
    body: &Option<Value>,
) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 通用皮肤 =====
        "GET /api/default-skins" => Some(handle_default_skins()),
        "GET /api/skin-head" => Some(handle_skin_head(params)),
        "POST /api/set-account-skin" => Some(handle_set_account_skin(body)),
        "POST /api/upload-skin" => Some(handle_upload_skin(body).await),
        "GET /api/skin-texture" => Some(handle_skin_texture(params).await),
        "POST /api/save-avatar" => Some(handle_save_avatar(body)),
        "GET /api/clear-avatar" => Some(handle_clear_avatar()),

        // ===== 微软账号本地皮肤库 =====
        "GET /api/ms-skins/local" => Some(handle_ms_skins_local(params)),
        "GET /api/ms-skins/file" => Some(handle_ms_skins_file(params)),
        "POST /api/ms-skins/import" => Some(handle_ms_skins_import(body).await),
        "POST /api/ms-skins/apply" => Some(handle_ms_skins_apply(body).await),
        "POST /api/ms-skins/delete" => Some(handle_ms_skins_delete(body)),

        _ => None,
    }
}

// ====================================================================
// 通用皮肤路由
// ====================================================================

/// GET /api/default-skins — 列出内置默认皮肤
fn handle_default_skins() -> ApiResult {
    let skins: Vec<Value> = builtin_default_skins()
        .iter()
        .map(|(id, name, file, model)| {
            json!({
                "id": id,
                "name": name,
                "file": file,
                "model": model
            })
        })
        .collect();

    ApiResult::ok(json!({
        "success": true,
        "skins": skins
    }))
}

/// GET /api/skin-head — 获取皮肤头部 PNG
/// 参数：id（steve/alex/zombie/enderman/creeper）或 file（自定义文件名）
fn handle_skin_head(params: &Option<Value>) -> ApiResult {
    let id = utils::get_str(&params.clone().unwrap_or(Value::Null), "id");
    let custom_file = utils::get_str(&params.clone().unwrap_or(Value::Null), "file");

    let file = if !custom_file.is_empty() {
        custom_file
    } else if !id.is_empty() {
        match id.as_str() {
            "steve" => "steve_skin.png",
            "alex" => "skin_alex.png",
            "zombie" => "skin_zombie.png",
            "enderman" => "skin_enderman.png",
            "creeper" => "skin_creeper.png",
            _ => "",
        }
        .to_string()
    } else {
        String::new()
    };

    if file.is_empty() {
        return ApiResult::err(400, "Missing id");
    }

    // 先找内置皮肤
    if let Some(bytes) = get_builtin_skin_bytes(&file) {
        return png_response(bytes);
    }

    // 再找用户自定义皮肤（DATA_DIR/img/<file>）
    let custom_path = storage::resolve_data_dir().join("img").join(&file);
    if let Ok(bytes) = std::fs::read(&custom_path) {
        if is_png(&bytes) {
            return png_response(&bytes);
        }
    }

    ApiResult::err(404, "Skin not found")
}

/// POST /api/set-account-skin — 为账号设置默认皮肤
/// 参数：accountId, skinId
fn handle_set_account_skin(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let skin_id = utils::get_str(&data, "skinId");

    if account_id.is_empty() || skin_id.is_empty() {
        return ApiResult::err(400, "Missing params");
    }

    let skin_map: &[(&str, &str, &str)] = &[
        ("steve", "steve_skin.png", "default"),
        ("alex", "skin_alex.png", "slim"),
        ("zombie", "skin_zombie.png", "default"),
        ("enderman", "skin_enderman.png", "default"),
        ("creeper", "skin_creeper.png", "default"),
    ];

    let skin_info = skin_map.iter().find(|(id, _, _)| *id == skin_id);
    let (file, model) = match skin_info {
        Some((_, f, m)) => (*f, *m),
        None => return ApiResult::err(400, "Invalid skin"),
    };

    let mut accounts = storage::load_accounts();
    let arr = match accounts.as_array_mut() {
        Some(a) => a,
        None => return ApiResult::err(500, "accounts.json 格式错误"),
    };

    let acc = match arr.iter_mut().find(|a| utils::get_str(a, "id") == account_id) {
        Some(a) => a,
        None => return ApiResult::err(404, "Account not found"),
    };

    if let Some(obj) = acc.as_object_mut() {
        obj.insert("skinFile".to_string(), json!(file));
        obj.insert("skinModel".to_string(), json!(model));
    }

    storage::save_accounts(&accounts);

    write_skin_log(&format!(
        "切换皮肤成功 accountId={} skinId={} file={} model={}",
        account_id, skin_id, file, model
    ));
    ApiResult::ok(json!({ "success": true }))
}

/// POST /api/upload-skin — 上传自定义皮肤
/// 参数（JSON 形式）：accountId, model, fileBase64
/// 或 multipart/form-data（暂不支持，统一走 JSON）
async fn handle_upload_skin(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let model = utils::get_str(&data, "model");
    let file_base64 = utils::get_str(&data, "fileBase64");

    if account_id.is_empty() || file_base64.is_empty() {
        return ApiResult::err(400, "Missing file or accountId");
    }

    // 解码 base64
    let skin_buf = match base64_decode(&file_base64) {
        Some(b) => b,
        None => return ApiResult::err(400, "Invalid base64"),
    };

    if !is_png(&skin_buf) {
        return ApiResult::err(400, "File must be PNG");
    }

    let mut accounts = storage::load_accounts();
    let arr = match accounts.as_array_mut() {
        Some(a) => a,
        None => return ApiResult::err(500, "accounts.json 格式错误"),
    };

    let acc = match arr.iter_mut().find(|a| utils::get_str(a, "id") == account_id) {
        Some(a) => a,
        None => return ApiResult::err(404, "Account not found"),
    };

    // 保存皮肤文件到 DATA_DIR/img/custom_<accountId>_<ts>.png
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("custom_{}_{}.png", account_id, ts);
    let img_dir = storage::resolve_data_dir().join("img");
    let _ = std::fs::create_dir_all(&img_dir);
    let file_path = img_dir.join(&file_name);

    if std::fs::write(&file_path, &skin_buf).is_err() {
        return ApiResult::err(500, "写入皮肤文件失败");
    }

    let final_model = if model == "slim" { "slim" } else { "default" };

    if let Some(obj) = acc.as_object_mut() {
        obj.insert("skinFile".to_string(), json!(file_name));
        obj.insert("skinModel".to_string(), json!(final_model));
    }

    storage::save_accounts(&accounts);

    write_skin_log(&format!(
        "上传皮肤成功 accountId={} model={} file={} size={}",
        account_id,
        final_model,
        file_name,
        skin_buf.len()
    ));
    ApiResult::ok(json!({
        "success": true,
        "fileName": file_name
    }))
}

/// GET /api/skin-texture — 获取整张皮肤图
/// 参数：uuid（必填）、serverUrl、username
///
/// 优先级：
///   1. 账号本地 skinFile（内置或自定义）
///   2. 账号 skinUrl（微软账号登录后从 Mojang 拿到的纹理 URL）
///   3. 外置认证服务器 /skin/{username}.png
///   4. 公共皮肤服务（mc-heads.net/skin、crafatar.com/skins、minotar.net/skin）
///   5. 回退到 Steve 整皮
async fn handle_skin_texture(params: &Option<Value>) -> ApiResult {
    let p = params.clone().unwrap_or(Value::Null);
    let uuid = utils::get_str(&p, "uuid");
    let server_url = utils::get_str(&p, "serverUrl");
    let username = utils::get_str(&p, "username");

    if uuid.is_empty() {
        return ApiResult::err(400, "Missing uuid");
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
            if !skin_file.is_empty() {
                if let Some(bytes) = get_builtin_skin_bytes(&skin_file) {
                    return png_response_with_model(bytes, utils::get_str(acc, "skinModel"));
                }
                // 用户自定义皮肤
                let custom_path = storage::resolve_data_dir().join("img").join(&skin_file);
                if let Ok(bytes) = std::fs::read(&custom_path) {
                    if is_png(&bytes) {
                        return png_response_with_model(&bytes, utils::get_str(acc, "skinModel"));
                    }
                }
            }

            // 1b. 微软/外置账号的 skinUrl 字段：从 Mojang texture URL 直接拉取整皮
            //   这是登录时从 https://api.minecraftservices.com/minecraft/profile 拿到的
            //   textures.SKIN.url 字段
            let skin_url = utils::get_str(acc, "skinUrl");
            if !skin_url.is_empty() {
                if let Some(bytes) = fetch_remote_bytes(&skin_url).await {
                    if is_png(&bytes) {
                        let model = utils::get_str(acc, "skinModel");
                        return png_response_with_model(&bytes, model);
                    }
                }
                // skinUrl 拉取失败继续尝试后续数据源
            }
        }
    }

    // 2. 外置服务器：从 /skin/{username}.png 拉取
    if !server_url.is_empty() && !username.is_empty() {
        let url = format!(
            "{}/skin/{}.png",
            server_url.trim_end_matches('/'),
            username
        );
        if let Some(bytes) = fetch_remote_bytes(&url).await {
            if is_png(&bytes) {
                return png_response_with_model(&bytes, "default".to_string());
            }
        }
    }

    // 3. 公共皮肤服务（按 UUID 拉取整张皮肤）
    //   注意：这里用 SKIN_SERVICES（整皮），不是 AVATAR_SERVICES（头像）
    //   避免拿到 64x64 头像图被当成 64x64 皮肤拉伸
    let skin_services: &[&str] = &[
        "https://mc-heads.net/skin/{uuid}",
        "https://crafatar.com/skins/{uuid}",
        "https://minotar.net/skin/{uuid}",
    ];
    for tmpl in skin_services {
        let url = tmpl.replace("{uuid}", &clean_uuid);
        if let Some(bytes) = fetch_remote_bytes(&url).await {
            if is_png(&bytes) {
                return png_response_with_model(&bytes, "default".to_string());
            }
        }
    }

    // 4. 全部失败，回退到 Steve 整皮
    png_response_with_model(STEVE_SKIN_PNG, "default".to_string())
}

/// POST /api/save-avatar — 保存头像文件
/// 参数：dataUrl（如 data:image/png;base64,xxxx）
fn handle_save_avatar(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let data_url = utils::get_str(&data, "dataUrl");

    if data_url.is_empty() {
        return ApiResult::err(400, "dataUrl required");
    }

    // 解析 data URL：data:image/<fmt>;base64,<payload>
    let captures: Vec<&str> = data_url.splitn(2, "base64,").collect();
    if captures.len() != 2 {
        return ApiResult::err(400, "invalid dataUrl");
    }
    let header = captures[0];
    let payload = captures[1];

    let fmt = if header.contains("image/jpeg") || header.contains("image/jpg") {
        "jpg"
    } else if header.contains("image/png") {
        "png"
    } else if header.contains("image/webp") {
        "webp"
    } else {
        "png"
    };

    let bytes = match base64_decode(payload) {
        Some(b) => b,
        None => return ApiResult::err(400, "invalid base64 payload"),
    };

    let file_name = format!("avatar.{}", fmt);
    let file_path = storage::resolve_data_dir().join(&file_name);

    if std::fs::write(&file_path, &bytes).is_err() {
        return ApiResult::err(500, "写入头像文件失败");
    }

    ApiResult::ok(json!({
        "success": true,
        "path": file_path.to_string_lossy()
    }))
}

/// GET /api/clear-avatar — 清除已保存的头像文件
fn handle_clear_avatar() -> ApiResult {
    let data_dir = storage::resolve_data_dir();
    for ext in &["png", "jpg", "jpeg", "webp"] {
        let p = data_dir.join(format!("avatar.{}", ext));
        let _ = std::fs::remove_file(&p);
    }
    ApiResult::ok(json!({ "success": true }))
}

// ====================================================================
// 微软账号本地皮肤库
// ====================================================================

/// 获取微软皮肤库目录：DATA_DIR/ms-skins/<accountId>/
fn ms_skins_dir(account_id: &str) -> PathBuf {
    let dir = storage::resolve_data_dir().join("ms-skins").join(account_id);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 读取皮肤库 meta.json
fn load_ms_skin_meta(account_id: &str) -> Value {
    let meta_file = ms_skins_dir(account_id).join("meta.json");
    if let Ok(content) = std::fs::read_to_string(&meta_file) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            return v;
        }
    }
    json!({ "skins": [] })
}

/// 保存皮肤库 meta.json
fn save_ms_skin_meta(account_id: &str, meta: &Value) -> bool {
    let meta_file = ms_skins_dir(account_id).join("meta.json");
    if let Ok(json) = serde_json::to_string_pretty(meta) {
        std::fs::write(&meta_file, json).is_ok()
    } else {
        false
    }
}

/// GET /api/ms-skins/local — 获取本地皮肤库
/// 参数：accountId（必填）
fn handle_ms_skins_local(params: &Option<Value>) -> ApiResult {
    let p = params.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&p, "accountId");

    if account_id.is_empty() {
        return ApiResult::err(400, "Missing accountId");
    }

    // 校验账号存在且为微软账号
    if let Some(err) = validate_ms_account(&account_id) {
        return err;
    }

    let meta = load_ms_skin_meta(&account_id);
    let skins = meta.get("skins").cloned().unwrap_or(json!([]));

    ApiResult::ok(json!({
        "success": true,
        "skins": skins
    }))
}

/// GET /api/ms-skins/file — 获取本地皮肤文件 PNG
/// 参数：accountId, skinId
fn handle_ms_skins_file(params: &Option<Value>) -> ApiResult {
    let p = params.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&p, "accountId");
    let skin_id = utils::get_str(&p, "skinId");

    if account_id.is_empty() || skin_id.is_empty() {
        return ApiResult::err(400, "Missing params");
    }

    let meta = load_ms_skin_meta(&account_id);
    let skin = meta
        .get("skins")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.iter().find(|s| utils::get_str(s, "id") == skin_id))
        .cloned();

    let skin = match skin {
        Some(s) => s,
        None => return ApiResult::err(404, "Skin not found"),
    };

    let file = utils::get_str(&skin, "file");
    let file_path = ms_skins_dir(&account_id).join(&file);

    if let Ok(bytes) = std::fs::read(&file_path) {
        return png_response(&bytes);
    }

    ApiResult::err(404, "File not found")
}

/// POST /api/ms-skins/import — 导入皮肤到本地库
/// 参数：accountId, fileBase64, model, name
async fn handle_ms_skins_import(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let file_base64 = utils::get_str(&data, "fileBase64");
    let model = utils::get_str(&data, "model");
    let name = utils::get_str(&data, "name");

    if account_id.is_empty() || file_base64.is_empty() {
        return ApiResult::err(400, "Missing accountId or fileBase64");
    }

    if let Some(err) = validate_ms_account(&account_id) {
        return err;
    }

    let skin_buf = match base64_decode(&file_base64) {
        Some(b) => b,
        None => return ApiResult::err(400, "Invalid base64"),
    };

    if !is_png(&skin_buf) {
        return ApiResult::err(400, "File must be PNG");
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let skin_id = format!("skin_{}", ts);
    let file_name = format!("{}.png", skin_id);
    let file_path = ms_skins_dir(&account_id).join(&file_name);

    if std::fs::write(&file_path, &skin_buf).is_err() {
        return ApiResult::err(500, "写入皮肤文件失败");
    }

    let mut meta = load_ms_skin_meta(&account_id);
    let skins = meta
        .get_mut("skins")
        .and_then(|s| s.as_array_mut())
        .ok_or(())
        .map(|arr| {
            let new_skin = json!({
                "id": skin_id,
                "name": if name.is_empty() {
                    format!("自定义皮肤 {}", arr.len() + 1)
                } else {
                    name.clone()
                },
                "file": file_name,
                "model": if model == "slim" { "slim" } else { "default" },
                "importedAt": utils::now_iso()
            });
            arr.push(new_skin.clone());
            new_skin
        });

    match skins {
        Ok(new_skin) => {
            save_ms_skin_meta(&account_id, &meta);
            ApiResult::ok(json!({
                "success": true,
                "skin": new_skin
            }))
        }
        Err(_) => ApiResult::err(500, "meta.json 格式错误"),
    }
}

/// POST /api/ms-skins/apply — 应用本地皮肤到 Mojang 官方
/// 参数：accountId, skinId
/// 流程：
///   1. 校验账号存在且为微软账号
///   2. 解密 accessToken
///   3. 读取本地皮肤文件
///   4. 构造 multipart/form-data，POST 到 https://api.minecraftservices.com/minecraft/profile/skins
///   5. 处理 200/401/429/其他状态码
async fn handle_ms_skins_apply(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let skin_id = utils::get_str(&data, "skinId");

    if account_id.is_empty() || skin_id.is_empty() {
        return ApiResult::err(400, "Missing accountId or skinId");
    }

    if let Some(err) = validate_ms_account(&account_id) {
        return err;
    }

    // 加载账号，解密 accessToken
    let accounts = storage::load_accounts();
    let account = accounts
        .as_array()
        .and_then(|arr| arr.iter().find(|a| utils::get_str(a, "id") == account_id))
        .cloned();

    let account = match account {
        Some(a) => a,
        None => return ApiResult::err(404, "Account not found"),
    };

    let encrypted_token = utils::get_str(&account, "accessToken");
    if encrypted_token.is_empty() || encrypted_token == "0" {
        return ApiResult::err(401, "账户未登录，请重新登录微软账户");
    }

    let access_token = auth::token::decrypt_account_token(&encrypted_token);
    // 若解密失败（返回原值），说明 token 异常
    if access_token == encrypted_token && encrypted_token.starts_with("enc:") {
        return ApiResult::err(401, "Token 解密失败，请重新登录微软账户");
    }

    // 读取本地皮肤文件
    let meta = load_ms_skin_meta(&account_id);
    let skin = meta
        .get("skins")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.iter().find(|s| utils::get_str(s, "id") == skin_id))
        .cloned();

    let skin = match skin {
        Some(s) => s,
        None => return ApiResult::err(404, "Skin not found"),
    };

    let file = utils::get_str(&skin, "file");
    let file_path = ms_skins_dir(&account_id).join(&file);
    let skin_buf = match std::fs::read(&file_path) {
        Ok(b) => b,
        Err(_) => return ApiResult::err(404, "Skin file missing"),
    };

    let variant = if utils::get_str(&skin, "model") == "slim" {
        "slim"
    } else {
        "classic"
    };

    // 上传到 Mojang
    let upload = upload_skin_to_mojang(&access_token, &skin_buf, variant).await;

    match upload {
        UploadResult::Success(new_skin_url) => {
            // 更新账号 skinUrl / skinModel
            let mut accounts = storage::load_accounts();
            if let Some(arr) = accounts.as_array_mut() {
                if let Some(acc) = arr
                    .iter_mut()
                    .find(|a| utils::get_str(a, "id") == account_id)
                {
                    if let Some(obj) = acc.as_object_mut() {
                        if !new_skin_url.is_empty() {
                            obj.insert("skinUrl".to_string(), json!(new_skin_url));
                        }
                        obj.insert(
                            "skinModel".to_string(),
                            json!(if variant == "slim" { "slim" } else { "default" }),
                        );
                    }
                }
            }
            storage::save_accounts(&accounts);

            write_skin_log(&format!(
                "应用微软皮肤成功 accountId={} skinId={}",
                account_id, skin_id
            ));
            ApiResult::ok(json!({
                "success": true,
                "skinUrl": new_skin_url
            }))
        }
        UploadResult::Unauthorized => {
            write_skin_log(&format!(
                "应用微软皮肤失败 accountId={} skinId={} 原因=登录已过期",
                account_id, skin_id
            ));
            ApiResult::err(401, "登录已过期，请重新登录微软账户")
        }
        UploadResult::RateLimited(retry_after) => {
            write_skin_log(&format!(
                "应用微软皮肤限流 accountId={} skinId={} retryAfter={:?}",
                account_id, skin_id, retry_after
            ));
            let wait_seconds = retry_after.unwrap_or(60);
            let wait_minutes = (wait_seconds + 59) / 60;
            let msg = if wait_minutes > 1 {
                format!(
                    "操作过于频繁，Mojang 限制每分钟只能更换一次皮肤，请 {} 分钟后再试",
                    wait_minutes
                )
            } else {
                format!(
                    "操作过于频繁，Mojang 限制每分钟只能更换一次皮肤，请 {} 秒后再试",
                    wait_seconds
                )
            };
            ApiResult {
                status: 429,
                body: json!({
                    "success": false,
                    "error": msg,
                    "rateLimited": true,
                    "retryAfter": wait_seconds
                }),
            }
        }
        UploadResult::Failed(status, msg) => {
            write_skin_log(&format!(
                "应用微软皮肤失败 accountId={} skinId={} status={} msg={}",
                account_id, skin_id, status, msg
            ));
            ApiResult::err(status, &format!("上传失败: {}", msg))
        }
    }
}

/// POST /api/ms-skins/delete — 删除本地皮肤
/// 参数：accountId, skinId
fn handle_ms_skins_delete(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let account_id = utils::get_str(&data, "accountId");
    let skin_id = utils::get_str(&data, "skinId");

    if account_id.is_empty() || skin_id.is_empty() {
        return ApiResult::err(400, "Missing accountId or skinId");
    }

    let mut meta = load_ms_skin_meta(&account_id);
    let skins = match meta.get_mut("skins").and_then(|s| s.as_array_mut()) {
        Some(arr) => arr,
        None => return ApiResult::err(500, "meta.json 格式错误"),
    };

    let idx = skins
        .iter()
        .position(|s| utils::get_str(s, "id") == skin_id);
    let idx = match idx {
        Some(i) => i,
        None => return ApiResult::err(404, "Skin not found"),
    };

    let file = utils::get_str(&skins[idx], "file");
    let file_path = ms_skins_dir(&account_id).join(&file);
    let _ = std::fs::remove_file(&file_path);

    skins.remove(idx);
    save_ms_skin_meta(&account_id, &meta);

    ApiResult::ok(json!({ "success": true }))
}

// ====================================================================
// 辅助函数
// ====================================================================

/// 校验账号存在且为微软账号，返回 Err(ApiResult) 表示失败
fn validate_ms_account(account_id: &str) -> Option<ApiResult> {
    let accounts = storage::load_accounts();
    let acc = accounts
        .as_array()
        .and_then(|arr| arr.iter().find(|a| utils::get_str(a, "id") == account_id))
        .cloned();

    match acc {
        None => Some(ApiResult::err(404, "Account not found")),
        Some(a) => {
            if utils::get_str(&a, "type") != "microsoft" {
                Some(ApiResult::err(400, "Only microsoft account supported"))
            } else {
                None
            }
        }
    }
}

/// 远程拉取字节（带超时）
async fn fetch_remote_bytes(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC-Tauri/1.0")
        .build()
        .ok()?;

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok().map(|b| b.to_vec())
}

/// 上传皮肤到 Mojang 的结果
enum UploadResult {
    /// 成功，包含新的 skinUrl（可能为空）
    Success(String),
    /// 401 未授权
    Unauthorized,
    /// 429 速率限制，包含 Retry-After 秒数
    RateLimited(Option<u64>),
    /// 其他失败，包含 HTTP 状态码和错误消息
    Failed(u16, String),
}

/// 上传皮肤到 Mojang 官方
/// POST https://api.minecraftservices.com/minecraft/profile/skins
/// 格式：multipart/form-data，包含 variant 和 file 字段
async fn upload_skin_to_mojang(
    access_token: &str,
    skin_buf: &[u8],
    variant: &str,
) -> UploadResult {
    let boundary = format!("----VersePCSkinUpload{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0));

    let header_part = format!(
        "--{}\r\n\
         Content-Disposition: form-data; name=\"variant\"\r\n\r\n{}\r\n\
         --{}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n\
         Content-Type: image/png\r\n\r\n",
        boundary, variant, boundary
    );
    let end_part = format!("\r\n--{}--\r\n", boundary);

    let header_bytes = header_part.into_bytes();
    let end_bytes = end_part.into_bytes();

    // 拼接 multipart body
    let mut body_bytes = Vec::with_capacity(header_bytes.len() + skin_buf.len() + end_bytes.len());
    body_bytes.extend_from_slice(&header_bytes);
    body_bytes.extend_from_slice(skin_buf);
    body_bytes.extend_from_slice(&end_bytes);

    let url = "https://api.minecraftservices.com/minecraft/profile/skins";
    let content_type = format!("multipart/form-data; boundary={}", boundary);

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return UploadResult::Failed(0, format!("构建 HTTP 客户端失败: {}", e)),
    };

    let resp = match client
        .post(url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", &content_type)
        .body(body_bytes)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return UploadResult::Failed(0, format!("上传请求失败: {}", e)),
    };

    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let text = resp.text().await.unwrap_or_default();

    if status == 200 {
        // 解析响应取 ACTIVE 皮肤的 url
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        let new_url = parsed
            .get("skins")
            .and_then(|s| s.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|s| s.get("state").and_then(|v| v.as_str()) == Some("ACTIVE"))
            })
            .and_then(|s| s.get("url").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        UploadResult::Success(new_url)
    } else if status == 401 {
        UploadResult::Unauthorized
    } else if status == 429 {
        UploadResult::RateLimited(retry_after)
    } else {
        // 尝试从响应体提取 errorMessage
        let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({}));
        let msg = parsed
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .unwrap_or(&text)
            .to_string();
        UploadResult::Failed(status, msg)
    }
}

/// base64 解码
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose, Engine as _};
    // 容忍 data URL 前缀
    let payload = if let Some(idx) = s.find("base64,") {
        &s[idx + 7..]
    } else {
        s
    };
    general_purpose::STANDARD.decode(payload).ok()
}

/// 返回 PNG 图片响应（status=200，body 为 base64 data URL）
/// 注意：Tauri 端不能用 HTTP 直接返回二进制，统一编码为 base64 data URL
/// 前端 preload 层会把 data URL 转回 Blob
fn png_response(bytes: &[u8]) -> ApiResult {
    let data_url = utils::bytes_to_data_url(bytes, "image/png");
    ApiResult::ok(json!({
        "success": true,
        "dataUrl": data_url,
        "contentType": "image/png"
    }))
}

/// 返回 PNG 图片响应（带 skin model 头）
/// model 统一转小写返回，避免前端 accounts.js 检查 'slim' 时因大小写不匹配失败
///   （Mojang API 返回的 variant 是 "SLIM"/"CLASSIC"）
fn png_response_with_model(bytes: &[u8], model: String) -> ApiResult {
    let data_url = utils::bytes_to_data_url(bytes, "image/png");
    let model_lower = model.to_lowercase();
    ApiResult::ok(json!({
        "success": true,
        "dataUrl": data_url,
        "contentType": "image/png",
        "skinModel": if model_lower.is_empty() { "default".to_string() } else { model_lower }
    }))
}
