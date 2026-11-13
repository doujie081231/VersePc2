// filesystem.rs — 文件系统路由
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

/// 归一化 Windows verbatim 前缀：canonicalize() 在 Windows 上返回
/// 带 `\\?\` 前缀的路径（如 `\\?\D:\...`），与白名单前缀（普通路径）直接
/// starts_with 永远不匹配，导致所有路径被误判为无权访问。
/// 这里统一去掉 `\\?\` 前缀后再比较（UNC 的 `\\?\UNC\` 还原为 `\\`）。
fn normalize_path_for_allowlist(path: &std::path::Path) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{}", rest));
        }
        if let Some(rest) = s.strip_prefix("\\\\?\\") {
            return std::path::PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// 路径白名单校验
/// 只允许访问：DATA_DIR、用户主目录、桌面、文档、下载、.minecraft
fn is_path_allowed(path: &std::path::Path) -> bool {
    let path = match path.canonicalize() {
        Ok(p) => normalize_path_for_allowlist(&p),
        Err(_) => return false,
    };

    let data_dir = storage::resolve_data_dir();
    let allowed_prefixes: Vec<PathBuf> = vec![
        data_dir,
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::desktop_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")),
        dirs::download_dir().unwrap_or_else(|| PathBuf::from(".")),
        storage::default_minecraft_dir(PathBuf::from(".")),
    ]
    .into_iter()
    .map(|p| normalize_path_for_allowlist(&p))
    .collect::<Vec<PathBuf>>();

    let allowed = allowed_prefixes.iter().any(|prefix| path.starts_with(prefix));
    eprintln!("[fs-browse] path={:?} allowed={} prefixes={:?}", path, allowed, allowed_prefixes);
    allowed
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
    eprintln!("[open-folder] resolve_folder_type folder={:?} data_dir={}", folder, data_dir.display());
    let settings = storage::load_settings();
    let minecraft_dir = if utils::get_str(&settings, "gameDir").is_empty() {
        storage::default_minecraft_dir(data_dir.clone())
    } else {
        PathBuf::from(utils::get_str(&settings, "gameDir"))
    };
    eprintln!("[open-folder] minecraft_dir={}", minecraft_dir.display());

    let version_id = utils::get_str(&settings, "selectedVersion");
    let versions_dir = data_dir.join("versions");
    // 外部版本必须定位到 external-folders.json 里的真实目录；
    // 否则“打开模组/存档/日志目录”会错误打开启动器内部 data/versions/<id>。
    let external_dir = if version_id.is_empty() {
        None
    } else {
        crate::versions::resolve_external_version_dir(&version_id)
            .map(PathBuf::from)
    };
    let version_dir = external_dir.clone().unwrap_or_else(|| {
        if version_id.is_empty() {
            minecraft_dir.clone()
        } else {
            versions_dir.join(&version_id)
        }
    });
    // 外部版本目录结构为 <root>/versions/<id>，其父父目录即外部 Minecraft 根
    let external_root = external_dir.as_ref().and_then(|p| {
        p.parent().and_then(|versions| versions.parent()).map(|root| root.to_path_buf())
    });
    // 实际游戏目录：与启动参数/PCL 的 PathIndie 保持一致。
    // 版本隔离关闭时，资源包、模组、存档等应落在共享 gameDir，而不是 versions/<id>。
    let game_dir = if version_id.is_empty() {
        minecraft_dir.clone()
    } else {
        crate::launch::args_builder::resolve_game_dir(
            &version_id,
            None,
            external_dir.as_deref(),
            &settings,
            &versions_dir,
            &data_dir,
        )
    };
    eprintln!("[open-folder] selectedVersion={:?} versions_dir={} version_dir={} game_dir={}",
        version_id, versions_dir.display(), version_dir.display(), game_dir.display());

    let path = match folder {
        "minecraft" | "game" => external_root.clone().unwrap_or(minecraft_dir),
        "versions" => external_root
            .as_ref()
            .map(|root| root.join("versions"))
            .unwrap_or(versions_dir),
        "mods" => game_dir.join("mods"),
        "assets" => version_dir.join("assets"),
        "logs" => game_dir.join("logs"),
        "crash-reports" => game_dir.join("crash-reports"),
        "shaderpacks" => game_dir.join("shaderpacks"),
        "resourcepacks" => game_dir.join("resourcepacks"),
        "datapacks" => game_dir.join("datapacks"),
        "saves" => game_dir.join("saves"),
        "data" => data_dir,
        "custom" => {
            if custom_path.is_empty() {
                return None;
            }
            PathBuf::from(custom_path)
        }
        _ => return None,
    };
    eprintln!("[open-folder] resolved path={} (exists={})", path.display(), path.exists());

    Some(path)
}

/// 在资源管理器中打开路径（Windows: explorer.exe）
fn open_in_explorer(path: &std::path::Path) -> bool {
    // 不存在则创建
    if !path.exists() {
        let _ = std::fs::create_dir_all(path);
    }
    eprintln!("[open-folder-explorer] path={} is_dir={}", path.display(), path.is_dir());
    #[cfg(target_os = "windows")]
    {
        let result = if path.is_dir() {
            // 直接传原路径，由 Command::arg 自动对含空格路径加引号。
            // 不能补尾部反斜杠：含空格路径的结尾会拼成 \"，被命令行当作转义引号，
            // 导致路径断裂、explorer 回退打开默认位置（如“文档”）。
            let p = path.to_string_lossy().to_string();
            eprintln!("[open-folder-explorer] cmd = explorer.exe {:?}", p);
            std::process::Command::new("explorer.exe").arg(p).spawn()
        } else {
            // 文件：用 /select 定位并选中该文件
            let p = path.to_string_lossy().to_string();
            eprintln!("[open-folder-explorer] cmd = explorer.exe /select, {:?}", p);
            std::process::Command::new("explorer.exe")
                .args(["/select,", &p])
                .spawn()
        };
        match result {
            Ok(_) => true,
            Err(e) => {
                eprintln!("[open-folder] 启动 explorer 失败: {}", e);
                false
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // 文件使用 -R 在 Finder 中定位选中，目录直接打开
        let mut cmd = std::process::Command::new("open");
        if !path.is_dir() {
            cmd.arg("-R");
        }
        cmd.arg(path).spawn().is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(if path.is_dir() {
                path.to_path_buf()
            } else {
                path.parent().unwrap_or(path).to_path_buf()
            })
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
            if target_path.is_none_or(|s| s.is_empty()) {
                let data_dir = storage::resolve_data_dir();
                let settings = storage::load_settings();
                let version_id = utils::get_str(&settings, "selectedVersion");
                let minecraft_dir = if utils::get_str(&settings, "gameDir").is_empty() {
                    storage::default_minecraft_dir(data_dir.clone())
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

            let target_path = target_path.unwrap_or_default();
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
            let home_dir_str = storage::default_minecraft_dir(data_dir.clone())
                .to_string_lossy()
                .to_string();
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
            // 注意：直接返回数组，不是对象包裹
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
            let settings = storage::load_settings();
            let version_id = params
                .as_ref()
                .and_then(|p| p.get("versionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let resolved = crate::api::mods::resolve_mods_dir(&settings, &version_id, "");
            let mods_dir = resolved.unwrap_or_else(|| {
                storage::default_minecraft_dir(storage::resolve_data_dir()).join("mods")
            });
            let _ = std::fs::create_dir_all(&mods_dir);
            Some(ApiResult::ok(json!({
                "success": true,
                "path": mods_dir.to_string_lossy()
            })))
        }

        // ===== 默认资源路径 =====
        "GET /api/filesystem/default-resource-path" => {
            let settings = storage::load_settings();
            let res_type = params
                .as_ref()
                .and_then(|p| p.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let subfolder = match res_type.as_str() {
                "resourcepack" => "resourcepacks",
                "shader" => "shaderpacks",
                "datapack" => "datapacks",
                _ => "",
            };
            let data_dir = storage::resolve_data_dir();
            let minecraft_dir = storage::default_minecraft_dir(data_dir.clone());
            let rp = minecraft_dir.join("resourcepacks");
            let sp = minecraft_dir.join("shaderpacks");
            let dp = data_dir.join("datapacks");
            let mut target = if res_type == "datapack" {
                data_dir.join("datapacks")
            } else if !subfolder.is_empty() {
                minecraft_dir.join(subfolder)
            } else {
                data_dir.clone()
            };
            if !subfolder.is_empty()
                && let Some(mods_dir) =
                    crate::api::mods::resolve_mods_dir(&settings, "", "")
                    && let Some(game_dir) = mods_dir.parent() {
                        target = game_dir.join(subfolder);
                    }
            let _ = std::fs::create_dir_all(&target);
            Some(ApiResult::ok(json!({
                "success": true,
                "path": target.to_string_lossy(),
                "resourcepacks": rp.to_string_lossy(),
                "shaderpacks": sp.to_string_lossy(),
                "datapacks": dp.to_string_lossy()
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
        // body: { startPath, filters }  或  { path, showHidden }
        // 兼容两种字段约定：前端 browseDirectory(path, showHidden) 传 path，旧调用传 startPath
        "POST /api/filesystem/browse" => {
            let data = body.clone().unwrap_or(Value::Null);
            let mut start_path = utils::get_str(&data, "startPath");
            if start_path.is_empty() {
                start_path = utils::get_str(&data, "path");
            }
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

/// read_file_buffer 允许读取的媒体扩展名：壁纸/预览/存档图标等都是这些类型。
/// 不含 json/db/无扩展名文件，避免前端任意 JS 借此读取账号令牌、浏览器密码库、
/// SSH 私钥等（审计 P0-3）。
const READABLE_MEDIA_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "gif", "bmp", "svg", "ico",
    "mp4", "webm", "mov", "m4v", "mkv", "avi", "wmv", "flv",
];

/// 仅允许在数据目录下读取的日志类扩展名（问题反馈自动收集日志用）
const READABLE_LOG_EXTS: &[&str] = &["log", "txt", "install"];

/// 插件目录内允许读取的前端资源扩展名。
/// 插件 UI 由 plugin-runtime 通过 read_file_buffer 读取 JS/CSS/JSON 后执行，
/// 不做放行会导致所有插件页面空白（审计 P0-3 修复后的回归）。
/// 仅限 <数据目录>/plugins/ 内部，账号令牌等仍在数据目录其他位置，不受影响。
const READABLE_PLUGIN_EXTS: &[&str] = &["js", "mjs", "css", "json", "html"];

/// Windows 路径前缀比较（大小写不敏感）
fn path_starts_with_ci(path: &std::path::Path, prefix: &std::path::Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let p = path.to_string_lossy().to_lowercase();
        let q = prefix.to_string_lossy().to_lowercase();
        if q.is_empty() || !p.starts_with(&q) {
            return false;
        }
        // 防止 `data` 前缀误匹配 `database`
        p.len() == q.len()
            || p.as_bytes()[q.len()] == b'\\'
            || p.as_bytes()[q.len()] == b'/'
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.starts_with(prefix)
    }
}

/// 凭据/隐私类目录：不允许通过 read_file_buffer 读取
fn sensitive_read_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for key in ["APPDATA", "LOCALAPPDATA", "ProgramData", "SystemRoot"] {
        if let Some(v) = std::env::var_os(key) {
            roots.push(PathBuf::from(v));
        }
    }
    if let Some(home) = dirs::home_dir() {
        for name in [".ssh", ".aws", ".gnupg", ".config", ".kube", ".docker"] {
            roots.push(home.join(name));
        }
    }
    roots
        .into_iter()
        .map(|p| normalize_path_for_allowlist(&p))
        .collect()
}

/// 读取能力判定：
/// - 媒体文件：任意目录（数据目录、外部盘、Steam 创意工坊等），但敏感目录除外
/// - 日志文件：仅数据目录（反馈页自动收集 logs 下的 .log/.txt/.install）
fn is_read_allowed(
    path: &std::path::Path,
    data_dir: &std::path::Path,
    sensitive: &[PathBuf],
) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let under_data = path_starts_with_ci(path, data_dir);
    let under_plugins = under_data && path_starts_with_ci(path, &data_dir.join("plugins"));
    let is_media = READABLE_MEDIA_EXTS.contains(&ext.as_str());
    let is_log = under_data && READABLE_LOG_EXTS.contains(&ext.as_str());
    let is_plugin_asset = under_plugins && READABLE_PLUGIN_EXTS.contains(&ext.as_str());
    if !is_media && !is_log && !is_plugin_asset {
        return false;
    }
    if under_data {
        return true;
    }
    // 日志类离开数据目录后不再放行
    if !is_media {
        return false;
    }
    !sensitive.iter().any(|root| path_starts_with_ci(path, root))
}

/// 读取指定文件的原始字节（返回 ArrayBuffer）
/// 用于壁纸等本地图片加载：Tauri 的 WebView 无法直接加载本地文件路径，
/// 前端读取字节后转 blob URL 即可显示。
///
/// 安全约束（审计 P0-3）：必须先 canonicalize 再判断，防止 `..` 绕过；
/// 只允许媒体扩展名，敏感目录（AppData/ProgramData/SystemRoot/.ssh 等）一律拒绝。
#[tauri::command]
pub async fn read_file_buffer(_app: tauri::AppHandle, path: String) -> Result<Vec<u8>, String> {
    let canonical = std::fs::canonicalize(&path).map_err(|_| "文件不存在或路径无效".to_string())?;
    let normalized = normalize_path_for_allowlist(&canonical);
    if !normalized.is_file() {
        return Err("文件不存在".to_string());
    }
    let data_dir = std::fs::canonicalize(storage::resolve_data_dir())
        .map(|p| normalize_path_for_allowlist(&p))
        .unwrap_or_else(|_| normalize_path_for_allowlist(&storage::resolve_data_dir()));
    if !is_read_allowed(&normalized, &data_dir, &sensitive_read_roots()) {
        return Err("不允许读取该文件".to_string());
    }
    std::fs::read(&normalized).map_err(|e| e.to_string())
}

#[cfg(test)]
mod read_guard_tests {
    use super::*;
    use std::fs;

    fn setup() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "verse-fs-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(root.join("data").join("logs")).unwrap();
        fs::create_dir_all(root.join("data").join("plugins").join("demo")).unwrap();
        fs::create_dir_all(root.join("sensitive")).unwrap();
        fs::write(root.join("pic.png"), b"x").unwrap();
        fs::write(root.join("secret.json"), b"{}").unwrap();
        fs::write(root.join("id_rsa"), b"key").unwrap();
        fs::write(root.join("note.txt"), b"x").unwrap();
        fs::write(root.join("data").join("logs").join("launch-debug.log"), b"log").unwrap();
        fs::write(root.join("data").join("accounts.json"), b"{}").unwrap();
        fs::write(root.join("data").join("secret.js"), b"x").unwrap();
        fs::write(root.join("data").join("plugins").join("demo").join("host.js"), b"x").unwrap();
        fs::write(root.join("data").join("plugins").join("demo").join("style.css"), b"x").unwrap();
        fs::write(root.join("data").join("plugins").join("demo").join("data.bin"), b"x").unwrap();
        fs::write(root.join("sensitive").join("photo.jpg"), b"x").unwrap();
        root
    }

    fn canon(p: PathBuf) -> PathBuf {
        fs::canonicalize(p).unwrap()
    }

    #[test]
    fn media_is_allowed_but_credentials_and_logs_outside_data_are_not() {
        let root = setup();
        let data = canon(root.join("data"));
        let sensitive = vec![canon(root.join("sensitive"))];

        assert!(is_read_allowed(&canon(root.join("pic.png")), &data, &sensitive));
        assert!(!is_read_allowed(&canon(root.join("secret.json")), &data, &sensitive));
        assert!(!is_read_allowed(&canon(root.join("id_rsa")), &data, &sensitive));
        // .txt 属于日志类扩展名：只允许在数据目录下读取
        assert!(!is_read_allowed(&canon(root.join("note.txt")), &data, &sensitive));
        assert!(is_read_allowed(
            &canon(root.join("data").join("logs").join("launch-debug.log")),
            &data,
            &sensitive
        ));
    }

    #[test]
    fn plugin_assets_are_allowed_only_inside_plugins_dir() {
        let root = setup();
        let data = canon(root.join("data"));
        let sensitive: Vec<PathBuf> = Vec::new();
        let plugin = root.join("data").join("plugins").join("demo");

        // 插件 UI 依赖这些文件，必须放行
        assert!(is_read_allowed(&canon(plugin.join("host.js")), &data, &sensitive));
        assert!(is_read_allowed(&canon(plugin.join("style.css")), &data, &sensitive));
        // 插件目录内未知二进制仍拒绝
        assert!(!is_read_allowed(&canon(plugin.join("data.bin")), &data, &sensitive));
        // 数据目录里的账号文件 / 普通 JS 不允许借插件规则读取
        assert!(!is_read_allowed(
            &canon(root.join("data").join("accounts.json")),
            &data,
            &sensitive
        ));
        assert!(!is_read_allowed(
            &canon(root.join("data").join("secret.js")),
            &data,
            &sensitive
        ));
    }

    #[test]
    fn sensitive_directories_are_denied_even_for_media() {
        let root = setup();
        let data = canon(root.join("data"));
        let sensitive = vec![canon(root.join("sensitive"))];
        assert!(!is_read_allowed(&canon(root.join("sensitive").join("photo.jpg")), &data, &sensitive));
    }

    #[test]
    fn data_prefix_does_not_match_sibling_directory() {
        let root = setup();
        let data = canon(root.join("data"));
        fs::create_dir_all(root.join("database")).unwrap();
        fs::write(root.join("database").join("x.log"), b"x").unwrap();
        assert!(!is_read_allowed(&canon(root.join("database").join("x.log")), &data, &[]));
    }
}
