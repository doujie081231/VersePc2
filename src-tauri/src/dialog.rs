// dialog.rs — 文件对话框命令
// 使用 tauri-plugin-dialog 官方插件实现原生文件选择
// 兼容原项目 dialog-open / dialog:select-folder / dialog:select-file

use serde_json::{json, Value};
use tauri_plugin_dialog::{DialogExt, FilePath};

/// 解析 defaultPath：不存在则尝试创建，创建失败则回退到最近的已存在父目录
fn resolve_default_path(default_path: Option<String>) -> Option<std::path::PathBuf> {
    let p = default_path?;
    if p.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(&p);
    if path.exists() {
        return Some(path);
    }
    if std::fs::create_dir_all(&path).is_ok() {
        return Some(path);
    }
    let mut fallback = path.parent();
    while let Some(f) = fallback {
        if f.exists() {
            return Some(f.to_path_buf());
        }
        fallback = f.parent();
    }
    None
}

/// Tauri 命令：dialog_open
/// 兼容 Electron dialog.showOpenDialog(options)
#[tauri::command]
pub async fn dialog_open(app: tauri::AppHandle, options: Option<Value>) -> Value {
    let opts = options.unwrap_or(Value::Null);
    let title = opts
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("选择")
        .to_string();
    let default_path = opts
        .get("defaultPath")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let properties = opts.get("properties").and_then(|v| v.as_array());

    let is_folder = properties
        .map(|arr| arr.iter().any(|p| p.as_str() == Some("openDirectory")))
        .unwrap_or(false);
    let is_multi = properties
        .map(|arr| arr.iter().any(|p| p.as_str() == Some("multiSelections")))
        .unwrap_or(false);

    let result = tokio::task::spawn_blocking(move || -> Value {
        let mut builder = app.dialog().file();
        builder = builder.set_title(&title);
        if let Some(ref dp) = resolve_default_path(default_path) {
            if dp.exists() {
                builder = builder.set_directory(dp);
            }
        }

        if is_folder {
            if is_multi {
                match builder.blocking_pick_folders() {
                    Some(paths) => {
                        let file_paths: Vec<String> = paths
                            .iter()
                            .filter_map(|fp| match fp {
                                FilePath::Path(p) => p.to_str().map(|s| s.to_string()),
                                _ => None,
                            })
                            .collect();
                        json!({ "canceled": false, "filePaths": file_paths })
                    }
                    None => json!({ "canceled": true, "filePaths": [] }),
                }
            } else {
                match builder.blocking_pick_folder() {
                    Some(fp) => match fp {
                        FilePath::Path(p) => json!({ "canceled": false, "filePaths": [p.to_string_lossy()] }),
                        _ => json!({ "canceled": true, "filePaths": [] }),
                    },
                    None => json!({ "canceled": true, "filePaths": [] }),
                }
            }
        } else {
            if is_multi {
                match builder.blocking_pick_files() {
                    Some(paths) => {
                        let file_paths: Vec<String> = paths
                            .iter()
                            .filter_map(|fp| match fp {
                                FilePath::Path(p) => p.to_str().map(|s| s.to_string()),
                                _ => None,
                            })
                            .collect();
                        json!({ "canceled": false, "filePaths": file_paths })
                    }
                    None => json!({ "canceled": true, "filePaths": [] }),
                }
            } else {
                match builder.blocking_pick_file() {
                    Some(fp) => match fp {
                        FilePath::Path(p) => json!({ "canceled": false, "filePaths": [p.to_string_lossy()] }),
                        _ => json!({ "canceled": true, "filePaths": [] }),
                    },
                    None => json!({ "canceled": true, "filePaths": [] }),
                }
            }
        }
    })
    .await
    .unwrap_or(json!({ "canceled": true, "filePaths": [] }));

    result
}

/// Tauri 命令：select_folder
#[tauri::command]
pub async fn select_folder(app: tauri::AppHandle, title: Option<String>, default_path: Option<String>) -> Value {
    let title = title.unwrap_or_else(|| "选择文件夹".to_string());
    let result = tokio::task::spawn_blocking(move || -> Value {
        let mut builder = app.dialog().file();
        builder = builder.set_title(&title);
        if let Some(ref dp) = resolve_default_path(default_path) {
            if dp.exists() {
                builder = builder.set_directory(dp);
            }
        }
        match builder.blocking_pick_folder() {
            Some(fp) => match fp {
                FilePath::Path(p) => json!({ "cancelled": false, "path": p.to_string_lossy() }),
                _ => json!({ "cancelled": true }),
            },
            None => json!({ "cancelled": true }),
        }
    })
    .await
    .unwrap_or(json!({ "cancelled": true }));
    result
}

/// api_proxy 内部用的 select-folder 路由处理函数
/// 返回结构兼容原项目：{ success: true, path } 或 { success: false, cancelled: true }
pub async fn select_folder_api(
    app: &tauri::AppHandle,
    title: Option<String>,
    default_path: Option<String>,
) -> crate::api::ApiResult {
    let title = title.unwrap_or_else(|| "选择文件夹".to_string());
    let app_clone = app.clone();

    let body = tokio::task::spawn_blocking(move || -> Value {
        let mut builder = app_clone.dialog().file();
        builder = builder.set_title(&title);
        if let Some(ref dp) = resolve_default_path(default_path) {
            if dp.exists() {
                builder = builder.set_directory(dp);
            }
        }
        match builder.blocking_pick_folder() {
            Some(fp) => match fp {
                FilePath::Path(p) => json!({ "success": true, "path": p.to_string_lossy() }),
                _ => json!({ "success": false, "cancelled": true }),
            },
            None => json!({ "success": false, "cancelled": true }),
        }
    })
    .await
    .unwrap_or(json!({ "success": false, "cancelled": true }));

    crate::api::ApiResult { status: 200, body }
}

/// Tauri 命令：select_file
#[tauri::command]
pub async fn select_file(app: tauri::AppHandle, title: Option<String>, default_path: Option<String>) -> Value {
    let title = title.unwrap_or_else(|| "选择文件".to_string());
    let result = tokio::task::spawn_blocking(move || -> Value {
        let mut builder = app.dialog().file();
        builder = builder.set_title(&title);
        if let Some(ref dp) = resolve_default_path(default_path) {
            if dp.exists() {
                builder = builder.set_directory(dp);
            }
        }
        match builder.blocking_pick_file() {
            Some(fp) => match fp {
                FilePath::Path(p) => json!({ "cancelled": false, "path": p.to_string_lossy() }),
                _ => json!({ "cancelled": true }),
            },
            None => json!({ "cancelled": true }),
        }
    })
    .await
    .unwrap_or(json!({ "cancelled": true }));
    result
}
