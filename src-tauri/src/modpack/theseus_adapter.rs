// modpack/theseus_adapter.rs — 整合包导入适配层
//
// 提供 HMCL 与普通 ZIP 格式的通用解压导入。
// Mrpack 走 mrpack_native 模块，CurseForge 走 curseforge 模块。

use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::AppHandle;

use super::{emit_progress, normalize_version_id, versions_dir};

/// 导入 CurseForge 整合包
///
/// 委托给独立的 curseforge 模块处理（theseus 不识别 manifest.json）。
/// 保留此函数用于向后兼容，mod.rs 已直接调用 curseforge::import_curseforge。
#[allow(dead_code)]
pub async fn import_curseforge(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
) -> Value {
    eprintln!("[theseus-adapter] 委托 CurseForge 到独立模块: {}", file_path);
    super::curseforge::import_curseforge(app, file_path, custom_version_name).await
}

/// 导入 HMCL 整合包
///
/// theseus 不直接支持 HMCL 格式，走简化的 ZIP 解压流程。
pub async fn import_hmcl(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
) -> Value {
    eprintln!("[theseus-adapter] 开始导入 HMCL: {}", file_path);
    // HMCL 格式走通用 ZIP 解压
    import_zip_generic(app, file_path, custom_version_name).await
}

/// 导入普通 ZIP 整合包
pub async fn import_raw_zip(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
) -> Value {
    eprintln!("[theseus-adapter] 开始导入普通 ZIP: {}", file_path);
    import_zip_generic(app, file_path, custom_version_name).await
}

/// 通用 ZIP 解压导入（用于 HMCL 和普通 ZIP）
///
/// 这些格式 theseus 不直接支持，走简化的手动解压流程。
async fn import_zip_generic(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
) -> Value {
    use std::fs;
    use std::io::Read;

    emit_progress(app, 5, "正在读取整合包...", "read");

    // 1. 读取 ZIP 中的 modpack.json（HMCL）或直接解压
    let file = match fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    // 2. 尝试读取 HMCL 的 modpack.json 获取名称
    let mut pack_name = String::new();
    for i in 0..archive.len() {
        if let Ok(mut entry) = archive.by_index(i) {
            if entry.name() == "modpack.json" {
                let mut buf = String::new();
                if entry.read_to_string(&mut buf).is_ok() {
                    if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                        if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                            pack_name = name.to_string();
                        }
                    }
                }
                break;
            }
        }
    }

    // 3. 确定版本名
    let version_name = if !custom_version_name.is_empty() {
        custom_version_name.to_string()
    } else if !pack_name.is_empty() {
        pack_name
    } else {
        PathBuf::from(file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Modpack")
            .to_string()
    };
    let version_id = normalize_version_id(&version_name);
    let version_dir = versions_dir().join(&version_id);

    // 4. 创建版本目录
    if let Err(e) = fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    // 5. 解压所有文件到版本目录
    emit_progress(app, 20, "正在解压文件...", "extract");
    let total = archive.len();
    for i in 0..total {
        if let Ok(mut entry) = archive.by_index(i) {
            let name = entry.name().to_string();
            if name.ends_with('/') {
                continue;
            }
            let out_path = version_dir.join(&name);
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                let _ = fs::write(&out_path, &buf);
            }
        }
        let progress = 20 + ((i + 1) as u32 * 70 / total as u32);
        emit_progress(app, progress, &format!("解压中 {}/{}", i + 1, total), "extract");
    }

    emit_progress(app, 100, "导入完成", "completed");

    json!({
        "success": true,
        "versionId": version_id,
        "name": version_name
    })
}
