// modpack/theseus_adapter.rs — 整合包导入适配层
//
// 提供 HMCL 与普通 ZIP 格式的导入。
// Mrpack 走 mrpack_native 模块，CurseForge 走 curseforge 模块，MCBBS 走 mcbbs 模块。

use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};
use tauri::AppHandle;

use super::curseforge::{create_version_json, install_loader, LoaderKind};
use super::curseforge_shared as cf_shared;
use super::{emit_progress, import_log, normalize_version_id, read_zip_json, versions_dir};
use crate::modloaders::shared as ml_shared;
use crate::storage;

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
    super::curseforge::import_curseforge(app, file_path, custom_version_name, "").await
}

/// 导入 HMCL 整合包
///
/// 对齐 PCL：只复制压缩包内 `minecraft/` 到版本目录（不是原样解压整个 ZIP），
/// 否则 mods 会落在 `versions/<id>/minecraft/mods`，启动器读不到。
pub async fn import_hmcl(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
    base: &str,
) -> Value {
    import_log("HMCL", &format!("开始导入: {} (base={:?})", file_path, base));

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    let pack_json = match read_zip_json(&mut archive, &format!("{}modpack.json", base)) {
        Some(v) => v,
        None => {
            return json!({ "success": false, "error": "HMCL 整合包缺少 modpack.json" });
        }
    };

    let game_version = pack_json
        .get("gameVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if game_version.is_empty() {
        return json!({ "success": false, "error": "HMCL 整合包未提供游戏版本信息" });
    }

    let pack_name = resolve_pack_name(
        custom_version_name,
        pack_json.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        file_path,
    );
    let version_id = cf_shared::dedupe_version_id(&normalize_version_id(&pack_name));
    let version_dir = versions_dir().join(&version_id);

    let (mc_version, loader_kind, forge_ver, fabric_ver, neoforge_ver) =
        parse_game_version(&game_version);
    import_log(
        "HMCL",
        &format!(
            "gameVersion={} → mc={} loader={:?} forge={} fabric={} neo={}",
            game_version, mc_version, loader_kind, forge_ver, fabric_ver, neoforge_ver
        ),
    );

    emit_progress(app, 5, &format!("整合包: {}  MC: {}", pack_name, mc_version), "prepare");
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    if let Err(e) = install_base_and_loader(
        app,
        &mc_version,
        &loader_kind,
        &forge_ver,
        &fabric_ver,
        &neoforge_ver,
        &version_id,
        &version_dir,
    )
    .await
    {
        let _ = std::fs::remove_dir_all(&version_dir);
        import_log("HMCL", &format!("安装失败: {}", e));
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // 只复制 minecraft/（PCL 的 HMCL 行为）；兼容部分导出包使用 .minecraft/
    // 若两者都不存在，退回整个压缩包（避免解压出空版本）
    let prefix = find_game_root(&mut archive, base).unwrap_or_else(|| base.to_string());
    emit_progress(app, 55, "正在解压整合包文件...", "extract");
    import_log("HMCL", &format!("解压目录: {}", prefix));
    if let Err(e) = extract_prefix(&mut archive, &prefix, &version_dir, app) {
        let _ = std::fs::remove_dir_all(&version_dir);
        import_log("HMCL", &format!("解压失败: {}", e));
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // HMCL 的 minecraft/ 解压到版本目录内，必须开启版本隔离
    force_isolation(&version_dir);

    finish_import(app, &version_id, &version_dir, &pack_name, &mc_version, "hmcl", "HMCL").await;

    json!({
        "success": true,
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "targetVersion": "",
    })
}

/// 导入普通 ZIP 整合包
///
/// 若压缩包内含 `.minecraft/` 或 `minecraft/`，以其为游戏根目录解压到版本目录；
/// 否则保持原有行为（完整路径解压）。
pub async fn import_raw_zip(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
    base: &str,
) -> Value {
    import_log("ZIP", &format!("开始导入普通 ZIP: {} (base={:?})", file_path, base));

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    let pack_name = resolve_pack_name(custom_version_name, "", file_path);
    let version_id = cf_shared::dedupe_version_id(&normalize_version_id(&pack_name));
    let version_dir = versions_dir().join(&version_id);
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    let game_root = find_game_root(&mut archive, base);
    let prefix = game_root.clone().unwrap_or_default();
    import_log(
        "ZIP",
        &format!(
            "游戏根目录: {}",
            if prefix.is_empty() { "(压缩包根目录)" } else { prefix.as_str() }
        ),
    );

    emit_progress(app, 20, "正在解压文件...", "extract");
    if let Err(e) = extract_prefix(&mut archive, &prefix, &version_dir, app) {
        let _ = std::fs::remove_dir_all(&version_dir);
        import_log("ZIP", &format!("解压失败: {}", e));
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // 版本隔离：解压内容位于版本目录内，必须开启隔离才能作为游戏目录使用
    let vs_path = version_dir.join("version-settings.json");
    let mut vs: Value = std::fs::read_to_string(&vs_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = vs.as_object_mut() {
        obj.insert("isolation".to_string(), json!("on"));
    }
    let _ = std::fs::write(
        &vs_path,
        serde_json::to_string_pretty(&vs).unwrap_or_default(),
    );

    let pack_info = json!({
        "name": pack_name,
        "versionId": version_id,
        "packFormat": "zip",
        "importedAt": cf_shared::now_iso(),
        "sourceFile": file_path,
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info).unwrap_or_default(),
    );

    emit_progress(app, 100, "导入完成", "completed");
    import_log("ZIP", &format!("导入完成: versionId={}", version_id));

    json!({
        "success": true,
        "versionId": version_id,
        "name": pack_name
    })
}

// ============== 共用流程 ==============

/// 安装原版 + 加载器 + 版本 JSON
#[allow(clippy::too_many_arguments)]
pub(crate) async fn install_base_and_loader(
    app: &AppHandle,
    mc_version: &str,
    loader_kind: &LoaderKind,
    forge_ver: &str,
    fabric_ver: &str,
    neoforge_ver: &str,
    version_id: &str,
    version_dir: &Path,
) -> Result<(), String> {
    emit_progress(app, 8, "正在准备基础版本...", "base");
    let app_owned = app.clone();
    let on_progress = Some(Box::new(move |pct: u32, msg: String| {
        let mapped = 8 + (pct as u64 * 12 / 100).min(12) as u32;
        emit_progress(&app_owned, mapped, &msg, "base")
    }) as ml_shared::BaseVersionProgress);
    ml_shared::ensure_base_version_installed(mc_version, on_progress).await?;

    let mut loader_version_id: Option<String> = None;
    if *loader_kind != LoaderKind::None {
        emit_progress(app, 22, "正在安装模组加载器...", "loader-install");
        let lv = install_loader(
            loader_kind,
            mc_version,
            forge_ver,
            fabric_ver,
            neoforge_ver,
            app,
        )
        .await?;
        loader_version_id = Some(lv);
    }

    emit_progress(app, 40, "正在创建版本配置...", "version-config");
    create_version_json(
        version_id,
        version_dir,
        mc_version,
        loader_version_id.as_deref(),
    );
    Ok(())
}

/// 导入收尾：校验库、下载资源、写 pack-info
pub(crate) async fn finish_import(
    app: &AppHandle,
    version_id: &str,
    version_dir: &Path,
    pack_name: &str,
    mc_version: &str,
    pack_format: &str,
    log_tag: &str,
) {
    emit_progress(app, 70, "正在验证整合包完整性...", "verify");
    let settings = storage::load_settings();
    let merged = cf_shared::resolve_version_json(version_id);
    if let Some(asset_index) = merged.as_ref().and_then(|v| v.get("assetIndex")) {
        emit_progress(app, 82, "正在下载游戏资源...", "assets");
        if let Err(e) = super::mrpack_native::download_assets(app, asset_index, &settings).await {
            import_log(log_tag, &format!("资源下载异常(非致命): {}", e));
        }
    }

    let pack_info = json!({
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": mc_version,
        "packFormat": pack_format,
        "importedAt": cf_shared::now_iso(),
        "targetVersion": "",
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info).unwrap_or_default(),
    );

    emit_progress(
        app,
        100,
        &format!("整合包 \"{}\" 导入完成！", pack_name),
        "done",
    );
}

/// 查找游戏根目录前缀：`{base}.minecraft/` 或 `{base}minecraft/`
pub(crate) fn find_game_root(
    archive: &mut zip::ZipArchive<std::fs::File>,
    base: &str,
) -> Option<String> {
    let dot = format!("{}.minecraft/", base).to_lowercase();
    let plain = format!("{}minecraft/", base).to_lowercase();
    let mut has_dot = false;
    let mut has_plain = false;
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        let name = entry.name().replace('\\', "/").to_lowercase();
        if name.starts_with(&dot) {
            has_dot = true;
        } else if name.starts_with(&plain) {
            has_plain = true;
        }
    }
    if has_dot {
        Some(dot)
    } else if has_plain {
        Some(plain)
    } else {
        None
    }
}

/// 解压指定前缀下的文件到目标目录（剥离前缀）
fn extract_prefix(
    archive: &mut zip::ZipArchive<std::fs::File>,
    prefix: &str,
    dest_dir: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let prefix_lower = prefix.to_lowercase();
    let total = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|e| {
                    !e.is_dir()
                        && e.name().replace('\\', "/").to_lowercase().starts_with(&prefix_lower)
                })
                .unwrap_or(false)
        })
        .count()
        .max(1);
    let mut done = 0usize;

    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if !name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let rel = &name[prefix.len()..];
        if rel.is_empty() || !super::curseforge_shared::is_modpack_path_safe(rel) {
            continue;
        }
        let dest = dest_dir.join(rel);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        let _ = std::fs::write(&dest, &buf);
        done += 1;
        if done.is_multiple_of(50) {
            let pct = 55 + (done * 30 / total).min(30) as u32;
            emit_progress(app, pct, &format!("解压中 {}/{}", done, total), "extract");
        }
    }
    Ok(())
}

/// 开启版本隔离（解压内容位于版本目录内）
fn force_isolation(version_dir: &Path) {
    let path = version_dir.join("version-settings.json");
    let mut settings: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = settings.as_object_mut() {
        obj.insert("isolation".to_string(), json!("on"));
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&settings).unwrap_or_default(),
    );
}

/// 版本名：自定义名 > 包内名 > 文件名
fn resolve_pack_name(custom_name: &str, pack_name: &str, file_path: &str) -> String {
    let custom = custom_name.trim();
    if !custom.is_empty() {
        return custom.to_string();
    }
    let inner = pack_name.trim();
    if !inner.is_empty() {
        return inner.to_string();
    }
    Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Modpack")
        .to_string()
}

/// 解析 HMCL 的 gameVersion（如 `1.20.1` / `1.20.1-forge-47.2.0` / `1.20.1-fabric-0.15.11`）
fn parse_game_version(raw: &str) -> (String, LoaderKind, String, String, String) {
    let lower = raw.to_lowercase();
    if let Ok(re) = regex::Regex::new(
        r"^(\d+\.\d+(?:\.\d+)?)(?:-(forge|neoforge|fabric|fabric-loader)-(.+))?$",
    ) && let Some(caps) = re.captures(&lower)
    {
        let mc = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let loader = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let ver = caps.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
        return match loader {
            "forge" => (mc, LoaderKind::Forge, ver, String::new(), String::new()),
            "neoforge" => (mc, LoaderKind::NeoForge, String::new(), String::new(), ver),
            "fabric" | "fabric-loader" => {
                (mc, LoaderKind::Fabric, String::new(), ver, String::new())
            }
            _ => (mc, LoaderKind::None, String::new(), String::new(), String::new()),
        };
    }
    (raw.to_string(), LoaderKind::None, String::new(), String::new(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_game_version_maps_loaders() {
        let (mc, kind, forge, fabric, neo) = parse_game_version("1.20.1-forge-47.2.0");
        assert_eq!(mc, "1.20.1");
        assert_eq!(kind, LoaderKind::Forge);
        assert_eq!(forge, "47.2.0");
        assert!(fabric.is_empty() && neo.is_empty());

        let (mc, kind, _, fabric, _) = parse_game_version("1.20.1-fabric-0.15.11");
        assert_eq!(mc, "1.20.1");
        assert_eq!(kind, LoaderKind::Fabric);
        assert_eq!(fabric, "0.15.11");

        let (mc, kind, _, _, _) = parse_game_version("1.20.1");
        assert_eq!(mc, "1.20.1");
        assert_eq!(kind, LoaderKind::None);
    }

    #[test]
    fn resolve_pack_name_prefers_custom_then_inner() {
        assert_eq!(resolve_pack_name("自定义", "包内", "D:/a.zip"), "自定义");
        assert_eq!(resolve_pack_name("", "包内", "D:/a.zip"), "包内");
        assert_eq!(resolve_pack_name("", "", "D:/小寂寂寂.zip"), "小寂寂寂");
    }
}
