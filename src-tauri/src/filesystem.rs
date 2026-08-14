// filesystem.rs — 文件系统路由
// 兼容原项目 server/api/routes/filesystem.js
// 路由清单：
//   GET  /api/fs/browse               浏览文件系统（带白名单校验）
//   POST /api/open-folder             打开预定义文件夹
//   GET  /api/filesystem/quick-access  快速访问路径
//   GET  /api/filesystem/drives       驱动器列表
//   POST /api/filesystem/create-directory  创建目录
//   GET  /api/filesystem/default-mod-path 获取默认模组路径
//   GET  /api/filesystem/default-resource-path 获取默认资源路径
//   POST /api/filesystem/open-in-explorer  在资源管理器中打开
//   POST /api/filesystem/browse            POST 方式浏览文件夹（复用 GET /api/fs/browse）

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::storage;
use crate::utils;

/// 路径白名单校验（与原项目 /api/fs/browse 一致）
/// 只允许访问：DATA_DIR、用户主目录、桌面、文档、下载、.minecraft
fn is_path_allowed(path: &std::path::Path) -> bool {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let data_dir = storage::resolve_data_dir();
    let allowed_prefixes: Vec<PathBuf> = vec![
        data_dir,
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::desktop_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::download_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::home_dir().map(|h| h.join(".minecraft")).unwrap_or_else(|| PathBuf::from(".")),
    ];

    allowed_prefixes.iter().any(|prefix| path.starts_with(prefix))
}

/// 列目录，返回 items 数组
fn list_directory(path: &std::path::Path, dirs_only: bool) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return items,
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        // 排除以 . 开头的隐藏文件（除 .minecraft）
        if name.starts_with('.') && name != ".minecraft" {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if dirs_only && !meta.is_dir() {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        items.push(json!({
            "name": name,
            "path": entry_path.to_string_lossy(),
            "isDirectory": meta.is_dir(),
            "size": meta.len(),
            "modified": modified
        }));

        // 最多 300 条
        if items.len() >= 300 {
            break;
        }
    }

    // 排序：目录在前，名称 localeCompare
    items.sort_by(|a, b| {
        let a_dir = a.get("isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);
        let b_dir = b.get("isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);
        if a_dir != b_dir {
            return b_dir.cmp(&a_dir);
        }
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    items
}

/// 解析预定义文件夹类型为实际路径
fn resolve_folder_type(folder: &str, custom_path: &str) -> Option<PathBuf> {
    let data_dir = storage::resolve_data_dir();
    let settings = storage::load_settings();
    let minecraft_dir = if utils::get_str(&settings, "gameDir").is_empty() {
        dirs::home_dir().map(|h| h.join(".minecraft")).unwrap_or_else(|| data_dir.clone())
    } else {
        PathBuf::from(utils::get_str(&settings, "gameDir"))
    };

    let version_id = utils::get_str(&settings, "selectedVersion");
    let versions_dir = data_dir.join("versions");
    let version_dir = if !version_id.is_empty() {
        versions_dir.join(&version_id)
    } else {
        minecraft_dir.clone()
    };

    let path = match folder {
        "minecraft" | "game" => minecraft_dir,
        "versions" => versions_dir,
        "mods" => version_dir.join("mods"),
        "assets" => version_dir.join("assets"),
        "logs" => version_dir.join("logs"),
        "crash-reports" => version_dir.join("crash-reports"),
        "shaderpacks" => version_dir.join("shaderpacks"),
        "resourcepacks" => version_dir.join("resourcepacks"),
        "datapacks" => version_dir.join("datapacks"),
        "saves" => version_dir.join("saves"),
        "data" => data_dir,
        "custom" => {
            if custom_path.is_empty() {
                return None;
            }
            PathBuf::from(custom_path)
        }
        _ => return None,
    };

    Some(path)
}

/// 在资源管理器中打开路径（Windows: explorer.exe）
fn open_in_explorer(path: &std::path::Path) -> bool {
    // 不存在则创建
    if !path.exists() {
        let _ = std::fs::create_dir_all(path);
    }
    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("explorer.exe").arg(path).spawn() {
            Ok(_) => true,
            Err(e) => {
                eprintln!("[open-folder] 启动 explorer 失败: {}", e);
                false
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .is_ok()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// 获取驱动器列表（Windows：探测 C: D: E: F: G: H:）
fn list_drives() -> Vec<Value> {
    let mut drives: Vec<Value> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for letter in b'C'..=b'H' {
            let drive = format!("{}:", letter as char);
            let path = PathBuf::from(&drive);
            if path.exists() {
                let total_size = std::fs::metadata(&path)
                    .ok()
                    .map(|m| m.len())
                    .unwrap_or(0);
                drives.push(json!({
                    "name": (letter as char).to_string(),
                    "path": drive,
                    "type": "fixed",
                    "totalSize": total_size
                }));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for p in ["/", "/home", "/mnt", "/Volumes"] {
            let path = PathBuf::from(p);
            if path.exists() {
                drives.push(json!({
                    "name": p,
                    "path": p,
                    "type": "fixed",
                    "totalSize": ""
                }));
            }
        }
    }
    drives
}

/// api_proxy 路由处理：文件系统相关
pub fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<crate::api::ApiResult> {
    use crate::api::ApiResult;

    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        // ===== 浏览文件系统 =====
        "GET /api/fs/browse" => {
            let target_path = params
                .as_ref()
                .and_then(|p| p.get("path"))
                .and_then(|v| v.as_str());

            // 无 path：返回 quickAccess
            if target_path.is_none() || target_path.unwrap().is_empty() {
                let data_dir = storage::resolve_data_dir();
                let settings = storage::load_settings();
                let version_id = utils::get_str(&settings, "selectedVersion");
                let minecraft_dir = if utils::get_str(&settings, "gameDir").is_empty() {
                    dirs::home_dir().map(|h| h.join(".minecraft")).unwrap_or_else(|| data_dir.clone())
                } else {
                    PathBuf::from(utils::get_str(&settings, "gameDir"))
                };
                let versions_dir = data_dir.join("versions");
                let version_dir = if !version_id.is_empty() {
                    versions_dir.join(&version_id)
                } else {
                    minecraft_dir.clone()
                };

                let home_str = dirs::home_dir()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_default();
                let data_dir_str = data_dir.to_string_lossy().to_string();
                let version_dir_str = version_dir.to_string_lossy().to_string();
                let mods_dir_str = version_dir.join("mods").to_string_lossy().to_string();
                let saves_dir_str = version_dir.join("saves").to_string_lossy().to_string();
                let rp_dir_str = version_dir.join("resourcepacks").to_string_lossy().to_string();

                let quick_access = json!([
                    { "name": "VersePC 数据目录", "path": data_dir_str, "type": "app" },
                    { "name": "用户主目录", "path": home_str, "type": "home" },
                    { "name": format!("版本: {}", version_id), "path": version_dir_str, "type": "version" },
                    { "name": "模组目录", "path": mods_dir_str, "type": "mods" },
                    { "name": "存档目录", "path": saves_dir_str, "type": "saves" },
                    { "name": "资源包目录", "path": rp_dir_str, "type": "resourcepacks" }
                ]);
                return Some(ApiResult::ok(json!({ "success": true, "quickAccess": quick_access })));
            }

            let target_path = target_path.unwrap();
            let path_buf = PathBuf::from(target_path);

            // 白名单校验
            if !is_path_allowed(&path_buf) {
                return Some(ApiResult::err(403, "无权访问该路径"));
            }

            let dirs_only = params
                .as_ref()
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .map(|s| s == "dir")
                .unwrap_or(false);

            let items = list_directory(&path_buf, dirs_only);
            let parent = path_buf
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            Some(ApiResult::ok(json!({
                "success": true,
                "path": target_path,
                "parent": parent,
                "items": items,
                "total": items.len()
            })))
        }

        // ===== 打开预定义文件夹 =====
        "POST /api/open-folder" => {
            let data = body.clone().unwrap_or(Value::Null);
            let folder = utils::get_str(&data, "folder");
            let custom_path = utils::get_str(&data, "customPath");
            let folder = if folder.is_empty() { "data".to_string() } else { folder };

            let target_path = match resolve_folder_type(&folder, &custom_path) {
                Some(p) => p,
                None => return Some(ApiResult::err(400, "无效的文件夹类型")),
            };

            let path_str = target_path.to_string_lossy().to_string();
            if !open_in_explorer(&target_path) {
                return Some(ApiResult::err(500, "无法打开文件夹"));
            }
            Some(ApiResult::ok(json!({ "success": true, "path": path_str })))
        }

        // ===== 快速访问路径 =====
        "GET /api/filesystem/quick-access" => {
            let data_dir = storage::resolve_data_dir();
            let home_dir_str = dirs::home_dir()
                .map(|h| h.join(".minecraft").to_string_lossy().to_string())
                .unwrap_or_default();
            let desktop_str = dirs::desktop_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let docs_str = dirs::document_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let downloads_str = dirs::download_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let data_dir_str = data_dir.to_string_lossy().to_string();

            let quick_access = json!([
                { "name": "桌面", "path": desktop_str, "icon": "desktop" },
                { "name": "文档", "path": docs_str, "icon": "docs" },
                { "name": "下载", "path": downloads_str, "icon": "downloads" },
                { "name": ".minecraft", "path": home_dir_str, "icon": "minecraft" },
                { "name": "VersePC 数据", "path": data_dir_str, "icon": "versepc" }
            ]);
            // 注意：原项目直接返回数组，不是对象包裹
            Some(ApiResult::ok(quick_access))
        }

        // ===== 驱动器列表 =====
        "GET /api/filesystem/drives" => {
            let drives = list_drives();
            Some(ApiResult::ok(Value::Array(drives)))
        }

        // ===== 创建目录 =====
        "POST /api/filesystem/create-directory" => {
            let data = body.clone().unwrap_or(Value::Null);
            let parent_path = utils::get_str(&data, "parentPath");
            let name = utils::get_str(&data, "name");
            if parent_path.is_empty() || name.is_empty() {
                return Some(ApiResult::err(400, "Missing parentPath or name"));
            }

            let full_path = PathBuf::from(&parent_path).join(&name);
            if full_path.exists() {
                return Some(ApiResult::err(400, "Directory already exists"));
            }

            match std::fs::create_dir_all(&full_path) {
                Ok(_) => Some(ApiResult::ok(json!({
                    "success": true,
                    "path": full_path.to_string_lossy()
                }))),
                Err(e) => Some(ApiResult::err(500, &format!("创建失败: {}", e))),
            }
        }

        // ===== 默认模组路径 =====
        "GET /api/filesystem/default-mod-path" => {
            let data_dir = storage::resolve_data_dir();
            let minecraft_dir = dirs::home_dir()
                .map(|h| h.join(".minecraft"))
                .unwrap_or_else(|| data_dir.clone());
            let mods_dir = minecraft_dir.join("mods");
            let _ = std::fs::create_dir_all(&mods_dir);
            Some(ApiResult::ok(json!({
                "success": true,
                "path": mods_dir.to_string_lossy()
            })))
        }

        // ===== 默认资源路径 =====
        "GET /api/filesystem/default-resource-path" => {
            let data_dir = storage::resolve_data_dir();
            let minecraft_dir = dirs::home_dir()
                .map(|h| h.join(".minecraft"))
                .unwrap_or_else(|| data_dir.clone());
            Some(ApiResult::ok(json!({
                "resourcepacks": minecraft_dir.join("resourcepacks").to_string_lossy(),
                "shaderpacks": minecraft_dir.join("shaderpacks").to_string_lossy(),
                "datapacks": data_dir.join("datapacks").to_string_lossy()
            })))
        }

        // ===== 在资源管理器中打开指定路径 =====
        "POST /api/filesystem/open-in-explorer" => {
            let data = body.clone().unwrap_or(Value::Null);
            let target = utils::get_str(&data, "path");
            if target.is_empty() {
                return Some(ApiResult::err(400, "Missing path"));
            }
            let path_buf = PathBuf::from(&target);
            if !path_buf.exists() {
                return Some(ApiResult::err(404, "路径不存在"));
            }
            if !open_in_explorer(&path_buf) {
                return Some(ApiResult::err(500, "无法打开"));
            }
            Some(ApiResult::ok(json!({ "success": true, "path": target })))
        }

        // ===== POST 方式浏览文件夹（复用 GET /api/fs/browse 逻辑） =====
        // body: { startPath, filters }
        // 简化版：把 startPath 转为 path 参数，复用 GET /api/fs/browse 的列目录逻辑
        "POST /api/filesystem/browse" => {
            let data = body.clone().unwrap_or(Value::Null);
            let start_path = utils::get_str(&data, "startPath");
            let filters = utils::get_str(&data, "filters");

            // 构造与 GET /api/fs/browse 一致的 params
            let mut fake_map = serde_json::Map::new();
            if !start_path.is_empty() {
                fake_map.insert("path".to_string(), json!(start_path));
            }
            if filters == "dir" {
                fake_map.insert("type".to_string(), json!("dir"));
            }
            let fake_params = if fake_map.is_empty() {
                None
            } else {
                Some(Value::Object(fake_map))
            };

            handle("GET", "/api/fs/browse", &fake_params, &None)
        }

        _ => None,
    }
}

/// 读取指定文件的原始字节（返回 ArrayBuffer）
/// 用于壁纸等本地图片加载：Tauri 的 WebView 无法直接加载本地文件路径，
/// 前端读取字节后转 blob URL 即可显示。
#[tauri::command]
pub async fn read_file_buffer(_app: tauri::AppHandle, path: String) -> Result<Vec<u8>, String> {
    let path_buf = std::path::Path::new(&path);
    if !path_buf.is_file() {
        return Err("文件不存在".to_string());
    }
    std::fs::read(&path).map_err(|e| e.to_string())
}
