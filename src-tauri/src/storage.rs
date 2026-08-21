// storage.rs — 数据持久化层
// 负责：数据目录解析、settings.json / accounts.json / favorites.json / store.json 读写
// 搬迁自 lib.rs，保持原接口不变

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde_json::{json, Value};

// ============== 数据目录解析 ==============
// 数据始终跟随程序（exe）所在目录，保证纯便携：
// 把 exe 及其同目录数据整体拷走，数据也随之迁移。
//   优先级 1: exe 同目录 data-config.json 手动指定的 dataDir（存在才用）
//   优先级 2: exe 同目录/data（默认，紧跟 exe）
//
// 兼容性处理（解决"移动文件夹后识别不到原数据"）：
//   a) data-config.json 支持相对路径（如 "./data"）：移动整个文件夹后路径自动跟随 exe；
//   b) 若 data-config.json 记录的绝对路径已失效（文件夹被移动/删除），
//      自动回退到 exe 同目录/data 并把 data-config.json 更新为有效路径；
//   c) 若 exe 所在目录不可写（例如被放到 Program Files / 只读目录），
//      自动改用用户目录 %APPDATA%/VersePC/data，保证软件一定能打开且数据可保存。

/// 探测 exe 所在目录是否可写（尝试创建+删除一个临时探针文件）
fn app_dir_writable(app_dir: &std::path::Path) -> bool {
    let probe = app_dir.join(format!(".versepc_wr_probe_{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 用户目录回退位置：%APPDATA%/VersePC/data（exe 目录不可写时使用）
fn user_fallback_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .map(|d| d.join("VersePC").join("data"))
        .or_else(|| dirs::home_dir().map(|h| h.join("AppData").join("Roaming").join("VersePC").join("data")))
        .unwrap_or_else(|| std::path::PathBuf::from("VersePC").join("data"))
}

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
                // 支持相对路径：基于 exe 目录解析（"./data" 随文件夹移动自动跟随）
                // 去掉开头的 "./" 或 ".\"，否则 PathBuf::join 会拼出 "app_dir\./data"
                // 这种带裸 "." 段落的畸形路径，导致 explorer 无法定位。
                let data_dir = if data_dir.is_absolute() {
                    data_dir
                } else {
                    let rel = data_dir.to_str().unwrap_or_default();
                    let rel = rel
                        .strip_prefix("./")
                        .or_else(|| rel.strip_prefix(".\\"))
                        .unwrap_or(rel);
                    app_dir.join(rel)
                };
                if data_dir.exists() {
                    return data_dir;
                }
                // data-config.json 记录的路径不存在（用户移动了文件夹），
                // 尝试 fallback 路径，如果存在则自动更新 data-config.json
                let fallback = app_dir.join("data");
                if fallback.exists() {
                    // 更新 data-config.json 指向新位置
                    let _ = std::fs::write(
                        &config_path,
                        serde_json::to_string_pretty(&json!({ "dataDir": fallback.to_string_lossy() })).unwrap_or_default(),
                    );
                    return fallback;
                }
            }
        }
    }

    // 优先级 2: exe 同目录/data（默认，紧跟 exe，纯便携）
    let default_dir = app_dir.join("data");
    // 已存在的目录直接使用（无需探测可写性）
    if default_dir.exists() {
        return default_dir;
    }
    // 目录尚不存在、需要创建时：若 exe 所在目录不可写（如 Program Files），
    // 回退到用户目录，避免"打不开"或数据写不进去
    if !app_dir_writable(&app_dir) {
        let user_dir = user_fallback_data_dir();
        // 若 data-config.json 可写，记录回退位置，避免每次启动重复探测
        let _ = std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({ "dataDir": user_dir.to_string_lossy() })).unwrap_or_default(),
        );
        return user_dir;
    }
    // 默认目录当前尚不存在但 exe 目录可写：先创建并固化到 data-config.json，
    // 避免后续启动因临时探测结果变化（如杀毒锁定）导致数据目录漂移、数据"丢失"。
    // 使用相对路径 "./data"，移动整个文件夹后数据目录自动跟随 exe。
    let _ = std::fs::create_dir_all(&default_dir);
    let _ = std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&json!({ "dataDir": "./data" })).unwrap_or_default(),
    );
    default_dir
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
        "maxChunksPerFile": 32,
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

// ============== 首次运行：检测 Electron 旧版数据并迁移 ==============
// 旧版 Electron 版 VersePC 的数据目录默认在 ~/.versepc（或由 data-config.json 指定）。
// 新版 Tauri 版首次运行时，若检测到旧版数据目录，则把其中的个性化设置
// （settings / accounts / favorites / external-folders / app-store 的 versepc_* 键）
// 迁移到新版数据目录，保证用户自定义图片、视频、账号等设置无缝衔接。

/// 迁移标记用的 store key（避免每次启动都重复迁移/扫描）
pub const MIGRATE_MARKER: &str = "versepc_legacy_migrated";

/// 旧版数据目录候选（与旧版 paths.js 解析优先级一致）
fn legacy_data_dir_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    // 旧版默认用户目录 ~/.versepc
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".versepc"));
    }
    // 当前 exe 同目录 /data（用户若把新版放到旧版旁边，数据已就位）
    candidates.push(resolve_data_dir());
    // data-config.json 指定的 dataDir
    let exe = std::env::current_exe().unwrap_or_default();
    if let Some(app_dir) = exe.parent() {
        let cfg = app_dir.join("data-config.json");
        if let Ok(raw) = std::fs::read_to_string(&cfg) {
            if let Ok(c) = serde_json::from_str::<Value>(raw.trim_start_matches('\u{FEFF}')) {
                if let Some(d) = c.get("dataDir").and_then(|v| v.as_str()) {
                    candidates.push(std::path::PathBuf::from(d));
                }
            }
        }
    }
    candidates
}

/// 判断一个目录是否为"旧版 Electron 数据目录"
fn looks_like_legacy_dir(dir: &std::path::Path) -> bool {
    dir.join("app-store.json").exists() || dir.join("accounts.json").exists()
}

/// 迁移入口：每次启动调用。分两阶段：
/// 1. 首次运行：把旧版的数据文件（settings/accounts/favorites/external-folders）
///    复制到新版数据目录（若新版还没有）。
/// 2. 每次启动：从旧版 app-store 补充缺失的 versepc_* 个性化键。
///    这让即使之前迁移不全（如自定义图片/视频键缺失）也能在重启后自动补全，
///    解决"个性化设置重启又没了"的问题。
/// 迁移入口：仅在首次启动（或 marker 不存在时）执行。
/// 1. 把旧版的数据文件（settings/accounts/favorites/external-folders）复制到新版数据目录（若新版还没有）。
/// 2. 从旧版 app-store 合并缺失的 versepc_* 个性化键。
/// 之后每次启动直接返回，避免无谓的磁盘 IO 扫描导致"启动黑屏/黑框"。
pub fn migrate_legacy_if_first_run(store: &Store) -> bool {
    // 已经做过迁移：完全跳过，不碰旧目录、不读 data-config，避免启动时磁盘 IO 抖动
    if store.get(MIGRATE_MARKER).is_some() {
        return false;
    }

    let data_dir = resolve_data_dir();
    let mut migrated_any = false;

    // 定位旧版数据目录
    let mut source: Option<std::path::PathBuf> = None;
    for cand in legacy_data_dir_candidates() {
        if cand == data_dir {
            continue;
        }
        if cand.exists() && looks_like_legacy_dir(&cand) {
            source = Some(cand);
            break;
        }
    }
    let src_path = match source {
        Some(s) => s,
        None => {
            // 没有旧版来源：也打上 marker，下次启动不再扫描
            let _ = store.set(MIGRATE_MARKER.into(), json!(true));
            return false;
        }
    };
    let src = &src_path;

    // 阶段 1：首次运行复制数据文件（不覆盖新版已有的文件）
    {
        let has_data = data_dir.join("settings.json").exists()
            || data_dir.join("accounts.json").exists()
            || data_dir.join("favorites.json").exists();
        if !has_data {
            migrated_any |= copy_legacy_files(src, &data_dir);
        }
    }

    // 阶段 2：首次运行补充缺失的 versepc_* 个性化键（之后不再执行）
    migrated_any |= merge_legacy_store_keys(src, store);

    // 标记完成（以后每次启动都快速跳过）
    let _ = store.set(MIGRATE_MARKER.into(), json!(true));
    migrated_any
}

/// 复制旧版的 JSON 数据文件到新版目录（仅当新版还没有对应文件）
fn copy_legacy_files(src: &Path, dst: &Path) -> bool {
    let _ = std::fs::create_dir_all(dst);
    let mut any = false;
    for fname in ["settings.json", "accounts.json", "favorites.json", "external-folders.json"] {
        let s = src.join(fname);
        let d = dst.join(fname);
        if s.exists() && !d.exists() {
            if std::fs::copy(&s, &d).is_ok() {
                println!("[migrate] 已迁移 {fname}");
                any = true;
            }
        }
    }
    any
}

/// 从旧版 app-store.json 合并缺失的 versepc_* 个性化键到新版 store
fn merge_legacy_store_keys(src: &Path, store: &Store) -> bool {
    let app_store_file = src.join("app-store.json");
    let mut any = false;
    if let Ok(content) = std::fs::read_to_string(&app_store_file) {
        if let Ok(app_store) = serde_json::from_str::<Value>(&content) {
            if let Some(obj) = app_store.as_object() {
                for (k, v) in obj {
                    if k.starts_with("versepc_") && store.get(k).is_none() {
                        store.set(k.clone(), v.clone());
                        any = true;
                    }
                }
            }
        }
    }
    if any {
        println!("[migrate] 已补充缺失的个性化设置键");
    }
    any
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

        let mut store = Self {
            data: Mutex::new(initial_data),
            path: store_path,
        };

        // 迁移后一次性"补键"兜底：
        // 若 migration marker 存在但个性化核心键仍缺（比如之前迁移跑在错误目录、
        // 用户换了 data-dir、或用户手动丢了 store.json），从旧版 app-store.json
        // 补一次 versepc_* 键，并打 LAZY_MERGE_DONE 标记，以后永远跳过。
        store.try_lazy_merge_personalize_keys();

        store
    }

    /// 一次性懒补个性化键：仅 migration 已跑过 + LAZY_MERGE_DONE 没打 + 核心键至少缺一个时才会读旧目录
    fn try_lazy_merge_personalize_keys(&mut self) {
        pub const LAZY_MERGE_DONE: &str = "versepc_lazy_merge_done_v1";

        // 已经做过懒补：直接跳过
        {
            let data = self.data.lock().unwrap();
            if data.get(LAZY_MERGE_DONE).is_some() {
                return;
            }
            // 连迁移标记都不存在：说明还没到"迁移后"的状态，首次迁移流程会处理
            if data.get(MIGRATE_MARKER).is_none() {
                return;
            }
            // 关键个性化键已经全有：无需懒补
            const CORE_KEYS: &[&str] = &[
                "versepc_personalize_settings",
                "versepc_launch_settings",
                "versepc_other_settings",
                "versepc_theme",
            ];
            let any_missing = CORE_KEYS.iter().any(|k| data.get(*k).is_none());
            if !any_missing {
                // 打标记，下次直接跳过
                drop(data);
                let _ = self.set(LAZY_MERGE_DONE.into(), json!(true));
                return;
            }
        }

        // 尝试定位旧版 app-store.json（按 legacy 候选找一遍）
        let mut legacy_store_src: Option<std::path::PathBuf> = None;
        let current_data_dir = resolve_data_dir();
        for cand in legacy_data_dir_candidates() {
            if cand == current_data_dir {
                continue;
            }
            let f = cand.join("app-store.json");
            if f.exists() {
                legacy_store_src = Some(f);
                break;
            }
        }
        let legacy_src = match legacy_store_src {
            Some(p) => p,
            None => {
                // 没找到旧源也打个标记，免得下次继续找
                let _ = self.set(LAZY_MERGE_DONE.into(), json!(true));
                return;
            }
        };

        // 读取旧 app-store.json，补充缺失的 verspc_* 键
        if let Ok(content) = std::fs::read_to_string(&legacy_src) {
            if let Ok(app_store) = serde_json::from_str::<Value>(&content) {
                if let Some(obj) = app_store.as_object() {
                    let mut wrote_any = false;
                    let mut data = self.data.lock().unwrap();
                    for (k, v) in obj {
                        if k.starts_with("versepc_") && data.get(k).is_none() {
                            data.insert(k.clone(), v.clone());
                            wrote_any = true;
                        }
                    }
                    // 不管补没补到，都打 done
                    data.insert(LAZY_MERGE_DONE.into(), json!(true));
                    drop(data);
                    if wrote_any {
                        // 立即写盘
                        if let Ok(json) = serde_json::to_string_pretty(
                            &*self.data.lock().unwrap()
                        ) {
                            let _ = std::fs::write(&self.path, json);
                        }
                        println!("[store] 懒补个性化键完成");
                    }
                }
            }
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
