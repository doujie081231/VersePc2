// storage.rs — 数据持久化层
// 负责：数据目录解析、settings.json / accounts.json / favorites.json / store.json 读写
// 搬迁自 lib.rs，保持原接口不变

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

// ============== 数据目录解析 ==============
// 数据始终跟随程序（exe）所在目录，保证纯便携：
// 把 exe 及其同目录数据整体拷走，数据也随之迁移。
//   优先级 1: exe 同目录 data-config.json 手动指定的 dataDir（存在才用）
//   优先级 2: exe 同目录/data（默认，紧跟 exe）

pub fn resolve_data_dir() -> std::path::PathBuf {
    let exe = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("verse-tauri.exe"));
    let app_dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));

    // 优先级 1: data-config.json 手动指定
    let config_path = app_dir.join("data-config.json");
    if let Ok(raw) = std::fs::read_to_string(&config_path) {
        let raw = raw.trim_start_matches('\u{FEFF}');
        if let Ok(cfg) = serde_json::from_str::<Value>(&raw) {
            if let Some(data_dir_str) = cfg.get("dataDir").and_then(|v| v.as_str()) {
                let data_dir = std::path::PathBuf::from(data_dir_str);
                if data_dir.exists() {
                    return data_dir;
                }
            }
        }
    }

    // 优先级 2: exe 同目录/data（紧跟 exe，纯便携）
    app_dir.join("data")
}

// ============== settings.json ==============

/// 读取 settings.json（便携数据目录），不存在则返回默认设置
pub fn load_settings() -> Value {
    let settings_file = resolve_data_dir().join("settings.json");
    if let Ok(content) = std::fs::read_to_string(&settings_file) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            return v;
        }
    }
    default_settings()
}

/// 默认设置（与原项目 server/accounts.js loadSettingsCached 一致）
pub fn default_settings() -> Value {
    json!({
        "javaPath": "",
        "maxMemory": 4096,
        "minMemory": 1024,
        "gameDir": "",
        "versionIsolation": true,
        "javaArgs": "",
        "fullscreen": false,
        "resolution": "1920x1080",
        "autoUpdate": true,
        "closeOnLaunch": false,
        "selectedVersion": "",
        "selectedAccount": "",
        "downloadSource": "china-first",
        "versionSource": "mojang",
        "maxThreads": 64,
        "enableChunkDownload": true,
        "maxChunksPerFile": 64,
        "speedLimit": 0,
        "targetDir": "",
        "sslVerify": false,
        "modSource": "modrinth",
        "filenameFormat": "default",
        "modStyle": "title",
        "ignoreQuilt": false,
        "accentColor": "#4a9eff",
        "blurBg": true,
        "backgroundImage": "",
        "avatarImage": "",
        "autoSetChinese": true,
        "jvmPreheat": true
    })
}

/// 保存设置到 settings.json（合并模式：把 new_data 的字段覆盖到现有设置上）
pub fn save_settings(new_data: &Value) -> Value {
    let mut current = load_settings();
    if let (Some(cur_obj), Some(new_obj)) = (current.as_object_mut(), new_data.as_object()) {
        for (k, v) in new_obj {
            cur_obj.insert(k.clone(), v.clone());
        }
    }
    overwrite_settings(&current);
    current
}

/// 直接覆盖保存设置（不做合并）
pub fn overwrite_settings(settings: &Value) -> bool {
    let settings_file = resolve_data_dir().join("settings.json");
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        std::fs::write(&settings_file, json).is_ok()
    } else {
        false
    }
}

// ============== accounts.json ==============
// accounts.json 存放所有账号（离线/微软/第三方）
// 与原项目 server/accounts.js loadAccounts / saveAccounts 一致

pub fn load_accounts() -> Value {
    let accounts_file = resolve_data_dir().join("accounts.json");
    if let Ok(content) = std::fs::read_to_string(&accounts_file) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if v.is_array() {
                return v;
            }
        }
    }
    Value::Array(vec![])
}

pub fn save_accounts(accounts: &Value) -> bool {
    let accounts_file = resolve_data_dir().join("accounts.json");
    if let Ok(json) = serde_json::to_string_pretty(accounts) {
        std::fs::write(&accounts_file, json).is_ok()
    } else {
        false
    }
}

// ============== favorites.json ==============
// favorites.json 存放所有收藏夹
// 与原项目 server/accounts.js loadFavorites / saveFavorites 一致

pub fn load_favorites() -> Value {
    let favorites_file = resolve_data_dir().join("favorites.json");
    if let Ok(content) = std::fs::read_to_string(&favorites_file) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if v.is_array() {
                return v;
            }
        }
    }
    // 默认：单个"默认"收藏夹
    json!([{ "name": "默认", "id": "default", "favs": [], "notes": {} }])
}

pub fn save_favorites(favorites: &Value) -> bool {
    let favorites_file = resolve_data_dir().join("favorites.json");
    if let Ok(json) = serde_json::to_string_pretty(favorites) {
        std::fs::write(&favorites_file, json).is_ok()
    } else {
        false
    }
}

// ============== 外部版本文件夹（external-folders.json） ==============
// 兼容原项目 shared.js 的 loadExternalFolders / saveExternalFolders
// 格式：[{ "path": "...", "name": "...", "addedAt": "ISO" }]

pub fn load_external_folders() -> Vec<Value> {
    let path = resolve_data_dir().join("external-folders.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(arr) = v.as_array() {
                return arr.clone();
            }
        }
    }
    Vec::new()
}

pub fn save_external_folders(folders: &[Value]) -> bool {
    let path = resolve_data_dir().join("external-folders.json");
    let arr = Value::Array(folders.to_vec());
    if let Ok(json) = serde_json::to_string_pretty(&arr) {
        std::fs::write(&path, json).is_ok()
    } else {
        false
    }
}

// ============== 版本独立设置（version-settings.json） ==============
// 内部版本：versions/<id>/version-settings.json
// 外部版本：external-settings/<sanitized-id>-settings.json

/// 读取版本独立设置
pub fn load_version_settings(version_id: &str, is_external: bool) -> Value {
    let path = version_settings_path(version_id, is_external);
    if !path.exists() {
        return default_version_settings(version_id);
    }
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            // 合并默认值（缺失字段用默认值填充）
            return merge_version_settings(&v, version_id);
        }
    }
    default_version_settings(version_id)
}

/// 保存版本独立设置
pub fn save_version_settings(version_id: &str, is_external: bool, settings: &Value) -> bool {
    let path = version_settings_path(version_id, is_external);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        std::fs::write(&path, json).is_ok()
    } else {
        false
    }
}

/// 更新版本独立设置中的单个字段
pub fn update_version_setting(version_id: &str, is_external: bool, key: &str, value: Value) -> bool {
    let mut settings = load_version_settings(version_id, is_external);
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    save_version_settings(version_id, is_external, &settings)
}

fn version_settings_path(version_id: &str, is_external: bool) -> std::path::PathBuf {
    let data_dir = resolve_data_dir();
    if is_external {
        let sanitized = sanitize_version_id(version_id);
        let dir = data_dir.join("external-settings");
        dir.join(format!("{}-settings.json", sanitized))
    } else {
        let versions_dir = data_dir.join("versions");
        versions_dir.join(version_id).join("version-settings.json")
    }
}

/// 把版本 ID 中的非法字符替换为 _
/// 同时阻止目录穿越（过滤 `..` 和单独的 `.` 在路径分隔位置）
pub fn sanitize_version_id(id: &str) -> String {
    let mut result: String = id
        .chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>' => '_',
            _ => c,
        })
        .collect();
    // 防止目录穿越：把连续的 `..` 替换为 `__`
    while result.contains("..") {
        result = result.replace("..", "__");
    }
    // 防止以 `.` 开头（在某些系统下表示隐藏文件）
    if result.starts_with('.') {
        result.insert(0, '_');
    }
    result
}

/// 默认版本独立设置
fn default_version_settings(version_id: &str) -> Value {
    json!({
        "versionId": version_id,
        "customName": "",
        "description": "",
        "icon": "auto",
        "category": "auto",
        "favorite": false,
        "isolation": "global",
        "windowTitle": "",
        "customInfo": "",
        "javaPath": "global",
        "memoryMode": "global",
        "memoryValue": 4096,
        "memOptimize": "global",
        "jvmArgs": "",
        "gameArgs": "",
        "customMainClass": "",
        "beforeLaunchCommand": "",
        "afterLaunchCommand": "",
        "fullscreen": "global",
        "resolution": ""
    })
}

/// 合并版本设置（缺失字段用默认值填充）
fn merge_version_settings(saved: &Value, version_id: &str) -> Value {
    let defaults = default_version_settings(version_id);
    if !saved.is_object() {
        return defaults;
    }
    let mut result = defaults;
    if let Some(dst) = result.as_object_mut() {
        if let Some(src) = saved.as_object() {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    result
}

// ============== store.json（通用 KV 存储，兼容 electron-store） ==============
// 原 Electron 项目用 electron-store 保存一些杂项状态（如 window-config）
// Tauri 版用 store.json 替代

pub struct Store {
    pub data: Mutex<HashMap<String, Value>>,
    pub path: std::path::PathBuf,
}

impl Store {
    pub fn new() -> Self {
        let data_dir = resolve_data_dir();
        let _ = std::fs::create_dir_all(&data_dir);
        let store_path = data_dir.join("store.json");

        let initial_data: HashMap<String, Value> = if store_path.exists() {
            match std::fs::read_to_string(&store_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        Self {
            data: Mutex::new(initial_data),
            path: store_path,
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let data = self.data.lock().unwrap();
        data.get(key).cloned()
    }

    pub fn set(&self, key: String, value: Value) -> bool {
        let mut data = self.data.lock().unwrap();
        data.insert(key, value);
        match serde_json::to_string_pretty(&*data) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    eprintln!("[store] 写入失败: {}", e);
                    return false;
                }
                true
            }
            Err(e) => {
                eprintln!("[store] 序列化失败: {}", e);
                false
            }
        }
    }

    pub fn delete(&self, key: &str) -> bool {
        let mut data = self.data.lock().unwrap();
        data.remove(key);
        if let Ok(json) = serde_json::to_string_pretty(&*data) {
            let _ = std::fs::write(&self.path, json);
        }
        true
    }
}
