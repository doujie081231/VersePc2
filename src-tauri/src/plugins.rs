// plugins.rs — 插件框架（后端）
// 职责：插件目录解析、清单校验、安装(解压 zip)/卸载/已安装列表。
// 插件安装到 <data>/plugins/<id>/，每个插件目录内必须有一份 plugin.json。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::storage;

/// 默认插件市场索引地址（独立 Gitee 插件仓库）。
const DEFAULT_INDEX_URL: &str =
    "https://gitee.com/doujie081231/verseplugin/raw/master/index.json";

// ============== 路径 ==============

pub fn plugins_root() -> PathBuf {
    storage::resolve_data_dir().join("plugins")
}

/// 返回某插件的安装目录（供 plugin_exec 等其它模块按 id 定位磁盘清单）
pub fn plugin_dir(id: &str) -> PathBuf {
    plugins_root().join(id)
}

fn installed_index() -> PathBuf {
    plugins_root().join("installed.json")
}

/// 宿主侧插件授权表：记录用户确认过的高危权限，插件自己改 plugin.json 无法提权。
fn grants_path() -> PathBuf {
    plugins_root().join(".grants.json")
}

/// 插件 id 只允许普通目录名：拒绝路径分隔符、盘符和 `..`（与开服目录穿越同类问题）
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains(':')
}

fn load_grants() -> Value {
    match std::fs::read_to_string(grants_path()) {
        Ok(c) => serde_json::from_str(c.trim_start_matches('\u{FEFF}'))
            .unwrap_or_else(|_| json!({ "grants": {} })),
        Err(_) => json!({ "grants": {} }),
    }
}

fn save_grants(grants: &Value) {
    let path = grants_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(grants).unwrap_or_default());
}

/// 插件清单是否声明了 native:exec。注意这只是“可申请”条件，不等于已授权。
pub fn plugin_declares_native_exec(id: &str) -> bool {
    if !valid_plugin_id(id) {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(plugin_dir(id).join("plugin.json")) else {
        return false;
    };
    let Ok(m) = serde_json::from_str::<Value>(content.trim_start_matches('\u{FEFF}')) else {
        return false;
    };
    m.get("permissions")
        .and_then(|p| p.get("native"))
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("exec")))
        .unwrap_or(false)
}

/// 宿主授权表中是否记录了该插件的 native:exec 授权
pub fn has_native_exec_grant(id: &str) -> bool {
    if !valid_plugin_id(id) {
        return false;
    }
    load_grants()
        .get("grants")
        .and_then(|g| g.get(id))
        .and_then(|x| x.get("native_exec"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
}

/// 写入/撤销 native:exec 授权（仅安装流程在用户确认后调用）
pub fn set_native_exec_grant(id: &str, granted: bool) {
    if !valid_plugin_id(id) {
        return;
    }
    let mut data = load_grants();
    if !data.is_object() {
        data = json!({ "grants": {} });
    }
    if data.get("grants").and_then(|v| v.as_object()).is_none()
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("grants".to_string(), json!({}));
    }
    if let Some(grants) = data.get_mut("grants").and_then(|v| v.as_object_mut()) {
        if granted {
            grants.insert(
                id.to_string(),
                json!({ "native_exec": true, "grantedAt": crate::utils::now_millis() }),
            );
        } else {
            grants.remove(id);
        }
    }
    save_grants(&data);
}

/// 插件能否执行外部程序：id 合法 + 已安装且清单声明 exec + 宿主授权表已授权。
/// 三者缺一不可：更新后的插件即便旧授权还在，只要新清单不再声明 exec 也会被拒绝。
pub fn can_native_exec(id: &str) -> bool {
    valid_plugin_id(id) && plugin_declares_native_exec(id) && has_native_exec_grant(id)
}

// ============== 清单读取 ==============

/// 读取并校验一份 plugin.json（<id>/plugin.json）。
/// 返回带 `id`/`installedDir`/`installedVersion` 的完整清单；不合法返回 None。
fn read_plugin_manifest(manifest_path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let mut m: Value = serde_json::from_str(&content).ok()?;
    let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version = m.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if id.is_empty() || name.is_empty() || version.is_empty() {
        return None;
    }
    let dir = manifest_path.parent()?.to_path_buf();
    if let Value::Object(ref mut obj) = m {
        obj.insert("installedDir".to_string(), Value::String(dir.to_string_lossy().to_string()));
        obj.insert("installedVersion".to_string(), Value::String(version.to_string()));
    }
    Some(m)
}

// ============== 已安装列表 ==============

/// 扫描 <plugins>/*/plugin.json 得到已安装插件列表。
pub fn list_installed() -> Value {
    let root = plugins_root();
    let _ = std::fs::create_dir_all(&root);
    let mut plugins: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "installed.json" || name.starts_with('.') {
                continue;
            }
            if let Some(m) = read_plugin_manifest(&p.join("plugin.json")) {
                plugins.push(m);
            }
        }
    }
    json!({ "ok": true, "plugins": plugins, "root": root.to_string_lossy() })
}

// ============== 安装 ==============

/// 从 zip 解压安装插件。
/// - `zip_path`: 本地 zip 文件路径（下载/镜像已在调用侧完成）。
/// - `expected_id`: 期望的插件 id，用于校验与确定安装目标。
///
/// zip 内允许含一个顶层插件目录（自动剥离），plugin.json 须位于有效根。
pub async fn install_from_zip(zip_path: String, expected_id: String) -> Value {
    let root = plugins_root();
    let _ = std::fs::create_dir_all(&root);

    let tmp = root.join(format!(".installing_{}", crate::utils::now_millis()));
    let _ = std::fs::create_dir_all(&tmp);

    let result = phase_extract(&zip_path, &tmp).and_then(|(manifest, effective)| {
        let id = manifest.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !expected_id.is_empty() && id != expected_id {
            return Err(format!("插件 id 不匹配：zip 内为 {}，期望 {}", id, expected_id));
        }
        apply_install(&tmp, &effective, &id)
    });

    let _ = std::fs::remove_dir_all(&tmp);
    match result {
        Ok(manifest) => json!({ "ok": true, "plugin": manifest }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

/// 解压 zip 到临时目录，定位有效根并读取清单。
fn phase_extract(zip_path: &str, tmp: &Path) -> Result<(Value, PathBuf), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("无法打开插件包: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("插件包不是有效的 zip: {}", e))?;
    extract_safely(&mut archive, tmp)?;

    // 定位 plugin.json：根目录 或 单层顶层子目录
    if tmp.join("plugin.json").is_file() {
        let m = read_plugin_manifest(&tmp.join("plugin.json"))
            .ok_or_else(|| "插件缺少合法的 plugin.json".to_string())?;
        return Ok((m, tmp.to_path_buf()));
    }
    // 找单一顶层目录
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(tmp) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            }
        }
    }
    if dirs.len() == 1 {
        let sub = &dirs[0];
        if sub.join("plugin.json").is_file() {
            let m = read_plugin_manifest(&sub.join("plugin.json"))
                .ok_or_else(|| "插件缺少合法的 plugin.json".to_string())?;
            return Ok((m, sub.clone()));
        }
    }
    Err("插件包内未找到 plugin.json".to_string())
}

/// 把有效根移动到 <plugins>/<id>/（已存在则先删，支持更新覆盖）。
fn apply_install(tmp_root: &Path, effective: &Path, id: &str) -> Result<Value, String> {
    if !valid_plugin_id(id) {
        return Err("插件 id 不合法".to_string());
    }
    let target = plugin_dir(id);
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let _ = std::fs::create_dir_all(parent);
    if target.exists() {
        let _ = std::fs::remove_dir_all(&target);
    }
    if effective != tmp_root {
        // 顶层有子目录：移动其内容到 target
        std::fs::rename(effective, &target).map_err(|e| format!("安装失败: {}", e))?;
    } else {
        std::fs::create_dir_all(&target).map_err(|e| format!("创建插件目录失败: {}", e))?;
        copy_dir_contents(tmp_root, &target)?;
    }
    let m = read_plugin_manifest(&target.join("plugin.json"))
        .ok_or_else(|| "安装后读取清单失败".to_string())?;
    update_index(id, m.get("version").and_then(|v| v.as_str()).unwrap_or(""));
    Ok(m)
}

/// 安全解压：逐项写出，防 zip-slip（拒绝越过目标目录的路径）。
/// 供插件安装与场景渲染器组件解压复用。
pub(crate) fn extract_safely(archive: &mut zip::ZipArchive<std::fs::File>, dest: &Path) -> Result<(), String> {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("读取 zip 项失败: {}", e))?;
        let name = entry.name().to_string();
        let normalized = name.replace('\\', "/");
        // 防穿越：Path::starts_with 只比组件前缀、不解析 ..，必须显式检查
        if normalized.starts_with('/')
            || normalized.as_bytes().get(1) == Some(&b':')
            || normalized.split('/').any(|seg| seg == "..")
        {
            return Err(format!("插件包含非法路径: {}", name));
        }
        let out_path = dest.join(&normalized);
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&out_path);
            continue;
        }
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut f = std::fs::File::create(&out_path).map_err(|e| format!("写出失败 {}: {}", name, e))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| format!("写出失败 {}: {}", name, e))?;
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dst);
    if let Ok(entries) = std::fs::read_dir(src) {
        for e in entries.flatten() {
            let from = e.path();
            let to = dst.join(e.file_name());
            if from.is_dir() {
                copy_dir_contents(&from, &to)?;
            } else {
                std::fs::copy(&from, &to).map_err(|e| format!("复制失败 {}: {}", from.display(), e))?;
            }
        }
    }
    Ok(())
}

// ============== 卸载 ==============

pub fn uninstall(id: &str) -> Value {
    if !valid_plugin_id(id) {
        return json!({ "ok": false, "error": "插件 id 不合法" });
    }
    let target = plugin_dir(id);
    if !target.exists() {
        return json!({ "ok": false, "error": format!("插件 {} 未安装", id) });
    }
    match std::fs::remove_dir_all(&target) {
        Ok(()) => {
            remove_index(id);
            // 卸载时同步撤销宿主授权，避免同名插件重新安装后继承旧授权
            set_native_exec_grant(id, false);
            json!({ "ok": true })
        }
        Err(e) => json!({ "ok": false, "error": format!("卸载失败: {}", e) }),
    }
}

// ============== 已安装索引（简单记录，用于去重/版本） ==============

fn load_index() -> Value {
    let p = installed_index();
    match std::fs::read_to_string(&p) {
        Ok(c) => serde_json::from_str(c.trim_start_matches('\u{FEFF}')).unwrap_or(json!({ "plugins": [] })),
        Err(_) => json!({ "plugins": [] }),
    }
}

fn save_index(idx: &Value) {
    let _ = std::fs::write(installed_index(), serde_json::to_string_pretty(idx).unwrap_or_default());
}

fn update_index(id: &str, version: &str) {
    let mut idx = load_index();
    let arr = idx.get_mut("plugins").and_then(|v| v.as_array_mut());
    if let Some(arr) = arr {
        let now = crate::utils::now_millis();
        if let Some(existing) = arr.iter_mut().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id)) {
            if let Some(obj) = existing.as_object_mut() {
                obj.insert("version".to_string(), Value::String(version.to_string()));
                obj.insert("updatedAt".to_string(), json!(now));
            }
        } else {
            arr.push(json!({ "id": id, "version": version, "installedAt": now }));
        }
    }
    save_index(&idx);
}

fn remove_index(id: &str) {
    let mut idx = load_index();
    if let Some(arr) = idx.get_mut("plugins").and_then(|v| v.as_array_mut()) {
        arr.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some(id));
    }
    save_index(&idx);
}

// ============== Tauri 命令 ==============

/// 列出已安装插件
#[tauri::command]
pub fn plugin_list() -> Value {
    list_installed()
}

/// 卸载插件。请求体：{ id }
#[tauri::command]
pub fn plugin_uninstall(id: String) -> Value {
    uninstall(&id)
}

// ============== 插件市场 ==============

/// 拉取插件市场索引并叠加已安装信息。
pub async fn market_index(index_url: &str) -> Value {
    let url = if index_url.trim().is_empty() {
        DEFAULT_INDEX_URL.to_string()
    } else {
        index_url.to_string()
    };

    let installed = list_installed();
    let mut installed_map: HashMap<String, String> = HashMap::new();
    if let Some(arr) = installed.get("plugins").and_then(|v| v.as_array()) {
        for p in arr {
            let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let ver = p.get("installedVersion").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !id.is_empty() {
                installed_map.insert(id, ver);
            }
        }
    }

    match crate::modloaders::shared::fetch_json(&url).await {
        Ok(v) => {
            let items = v
                .get("plugins")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            let mut annotated: Vec<Value> = Vec::new();
            for mut item in items {
                let id = item.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let latest = item.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let installed_ver = installed_map.get(&id).cloned().unwrap_or_default();
                let is_installed = !installed_ver.is_empty();
                if let Value::Object(ref mut obj) = item {
                    obj.insert("isInstalled".into(), json!(is_installed));
                    obj.insert("installedVersion".into(), json!(installed_ver));
                    obj.insert("hasUpdate".into(), json!(is_installed && !latest.is_empty() && installed_ver != latest));
                }
                annotated.push(item);
            }
            json!({ "ok": true, "plugins": annotated, "source": url })
        }
        Err(e) => json!({ "ok": false, "error": format!("无法拉取插件市场索引: {}", e), "source": url }),
    }
}

/// 下载插件包（校验 sha256）并安装。
/// 注意：market 提供的是 sha256，而 download_with_mirror 的哈希参数是 sha1，
/// 因此下载阶段不传哈希（只做 size 校验），下载完成后用 sha256 独立校验。
pub async fn download_and_install(
    app: tauri::AppHandle,
    url: String,
    sha256: String,
    size: u64,
    expected_id: String,
) -> Value {
    let root = plugins_root();
    let _ = std::fs::create_dir_all(&root);
    let tmp = root.join(format!(".dl_{}.zip", crate::utils::now_millis()));

    let sha = if sha256.is_empty() { None } else { Some(sha256.as_str()) };
    let size_opt = if size > 0 { Some(size) } else { None };

    let result = match crate::download::download_with_mirror(&url, &tmp, None, size_opt, "auto", 300, None).await {
        Ok(()) => {
            if let Some(expected_sha) = sha {
                match crate::utils::calculate_sha256(&tmp) {
                    Some(actual) if actual.eq_ignore_ascii_case(expected_sha) => {
                        install_from_zip(tmp.to_string_lossy().to_string(), expected_id).await
                    }
                    _ => json!({ "ok": false, "error": "插件包校验哈希值失败".to_string() }),
                }
            } else {
                install_from_zip(tmp.to_string_lossy().to_string(), expected_id).await
            }
        }
        Err(e) => json!({ "ok": false, "error": format!("插件包下载失败: {}", e) }),
    };
    // 安装成功后处理 native:exec 授权：
    // 新清单声明 exec 时，必须由后端弹出原生确认框，用户点过才写入宿主授权表；
    // 这样即使前端被 XSS 调用安装命令，也无法绕过用户确认静默提权。
    if let Some(installed) = result.get("plugin").cloned()
        && let Some(id) = installed.get("id").and_then(|v| v.as_str())
    {
        if plugin_declares_native_exec(id) {
            let granted = confirm_native_exec(&app, id, &installed).await;
            set_native_exec_grant(id, granted);
        } else {
            // 更新后不再声明 exec：撤销旧授权，避免静默保留执行能力
            set_native_exec_grant(id, false);
        }
    }
    let _ = std::fs::remove_file(&tmp);
    result
}

/// 原生确认框：插件声明 native:exec 时，必须由用户在系统对话框中确认。
/// 使用异步 `show` + oneshot，避免阻塞异步运行时。
async fn confirm_native_exec(app: &tauri::AppHandle, id: &str, manifest: &Value) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(id);
    let message = format!(
        "插件「{}」（{}）声明了执行外部程序的权限。\n\n\
         信任后，该插件可以启动任意本地程序。\n\
         插件脚本运行在启动器进程内，JS 无法提供真正的沙箱隔离，请只安装你信任的插件。",
        name, id
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title("插件权限确认")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "信任并安装".to_string(),
            "取消".to_string(),
        ))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

/// 拉取插件市场。请求参数：{ indexUrl? }
#[tauri::command]
pub async fn plugin_market_index(index_url: String) -> Value {
    market_index(&index_url).await
}

/// 从市场下载并安装插件。请求参数：{ url, sha256?, size?, expectedId? }
#[tauri::command]
pub async fn plugin_download_install(
    app: tauri::AppHandle,
    url: String,
    sha256: String,
    size: u64,
    expected_id: String,
) -> Value {
    download_and_install(app, url, sha256, size, expected_id).await
}

#[cfg(test)]
mod grant_tests {
    use super::*;

    #[test]
    fn plugin_id_rejects_path_traversal() {
        for bad in ["", "..", "../x", "..\\x", "a/b", "a\\b", "C:\\Windows", "C:/Windows"] {
            assert!(!valid_plugin_id(bad), "应拒绝非法插件 id: {bad}");
        }
        assert!(valid_plugin_id("my-plugin_1"));
    }
}
