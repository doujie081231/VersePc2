// modpack/mcbbs.rs — MCBBS / BBSMC 整合包导入
//
// 对齐 PCL 的 InstallPackMCBBS：
//   1. 读取 mcbbs.packmeta（或带 addons 的 manifest.json）
//   2. addons 提供 game / forge / neoforge / fabric / optifine
//   3. 安装原版与加载器
//   4. 只解压 `overrides/` 到版本目录（mods 就在这里，不能把 overrides 前缀保留下来）
//
// 目录结构示例：
//   整合包.zip
//   ├── mcbbs.packmeta
//   └── overrides/
//       ├── mods/*.jar
//       └── config/...
//
// 若整包被套了一层文件夹（如 `小寂寂寂整合包/mcbbs.packmeta`），由 mod.rs 传入 base。

use std::collections::HashMap;

use serde_json::{json, Value};
use tauri::AppHandle;

use super::curseforge::extract_overrides_with_progress;
use super::curseforge::LoaderKind;
use super::curseforge_shared as cf_shared;
use super::{emit_progress, import_log, normalize_version_id, read_zip_json, versions_dir};
use super::theseus_adapter::{finish_import, install_base_and_loader};

/// 导入 MCBBS / BBSMC 整合包
pub async fn import_mcbbs(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
    base: &str,
) -> Value {
    import_log("MCBBS", &format!("开始导入: {} (base={:?})", file_path, base));

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    // packmeta 优先，其次 manifest.json（PCL 同序）
    let pack_json = read_zip_json(&mut archive, &format!("{}mcbbs.packmeta", base))
        .or_else(|| read_zip_json(&mut archive, &format!("{}manifest.json", base)));
    let pack_json = match pack_json {
        Some(v) => v,
        None => {
            return json!({ "success": false, "error": "MCBBS 整合包缺少 mcbbs.packmeta" });
        }
    };

    // addons: [{ id, version }]
    let mut addons: HashMap<String, String> = HashMap::new();
    if let Some(arr) = pack_json.get("addons").and_then(|v| v.as_array()) {
        for entry in arr {
            let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let version = entry.get("version").and_then(|v| v.as_str()).unwrap_or("");
            if !id.is_empty() && !version.is_empty() {
                addons.insert(id.to_lowercase(), version.to_string());
            }
        }
    }

    let game = addons.get("game").cloned().unwrap_or_default();
    if game.is_empty() {
        return json!({ "success": false, "error": "该 MCBBS 整合包未提供游戏版本信息，无法安装！" });
    }

    let forge_ver = addons.get("forge").cloned().unwrap_or_default();
    let neoforge_ver = addons.get("neoforge").cloned().unwrap_or_default();
    let fabric_ver = addons.get("fabric").cloned().unwrap_or_default();
    let optifine_ver = addons.get("optifine").cloned().unwrap_or_default();

    let loader_kind = if !forge_ver.is_empty() {
        LoaderKind::Forge
    } else if !neoforge_ver.is_empty() {
        LoaderKind::NeoForge
    } else if !fabric_ver.is_empty() {
        LoaderKind::Fabric
    } else {
        LoaderKind::None
    };

    let pack_name = resolve_name(
        custom_version_name,
        pack_json.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        file_path,
    );
    let version_id = cf_shared::dedupe_version_id(&normalize_version_id(&pack_name));
    let version_dir = versions_dir().join(&version_id);

    import_log(
        "MCBBS",
        &format!(
            "addons: game={} forge={} neoforge={} fabric={} optifine={}",
            game, forge_ver, neoforge_ver, fabric_ver, optifine_ver
        ),
    );

    emit_progress(app, 5, &format!("整合包: {}  MC: {}", pack_name, game), "prepare");
    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    if let Err(e) = install_base_and_loader(
        app,
        &game,
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
        import_log("MCBBS", &format!("安装失败: {}", e));
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // 只解压 overrides/ 并剥掉前缀（mods 在这里）
    emit_progress(app, 45, "正在解压整合包文件...", "extract");
    let prefix = format!("{}overrides/", base);
    import_log("MCBBS", &format!("解压目录: {}", prefix));
    if let Err(e) = extract_overrides_with_progress(&mut archive, &version_dir, app, &prefix) {
        let _ = std::fs::remove_dir_all(&version_dir);
        import_log("MCBBS", &format!("解压失败: {}", e));
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // 版本隔离 + launchInfo 参数
    apply_launch_info(&version_dir, &pack_json);

    finish_import(app, &version_id, &version_dir, &pack_name, &game, "mcbbs", "MCBBS").await;

    // 覆盖成更完整的 pack-info
    let pack_info = json!({
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": game,
        "packFormat": "mcbbs",
        "forgeVersion": forge_ver,
        "neoforgeVersion": neoforge_ver,
        "fabricVersion": fabric_ver,
        "optifineVersion": optifine_ver,
        "importedAt": cf_shared::now_iso(),
        "sourceFile": file_path,
        "targetVersion": "",
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info).unwrap_or_default(),
    );

    let warning = if !optifine_ver.is_empty() {
        Some(format!(
            "该整合包需要 OptiFine {}，当前导入流程未自动安装，请到版本设置中手动安装 OptiFine 后再启动",
            optifine_ver
        ))
    } else {
        None
    };

    json!({
        "success": true,
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": game,
        "targetVersion": "",
        "warning": warning,
    })
}

/// 开启版本隔离并写入 launchInfo 的 JVM / 游戏参数（对齐 PCL 的 Setup.ini 迁移）
fn apply_launch_info(version_dir: &std::path::Path, pack_json: &Value) {
    let path = version_dir.join("version-settings.json");
    let mut settings: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let Some(obj) = settings.as_object_mut() else { return };

    obj.insert("isolation".to_string(), json!("on"));

    if let Some(info) = pack_json.get("launchInfo") {
        if let Some(args) = join_args(info.get("javaArgument")) {
            obj.insert("jvmArgs".to_string(), json!(args));
        }
        if let Some(args) = join_args(info.get("launchArgument")) {
            obj.insert("gameArgs".to_string(), json!(args));
        }
    }

    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&settings).unwrap_or_default(),
    );
}

/// launchInfo 参数既可能是字符串，也可能是字符串数组
fn join_args(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Array(arr)) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.trim().is_empty())
                .collect();
            if parts.is_empty() { None } else { Some(parts.join(" ")) }
        }
        _ => None,
    }
}

/// 版本名：自定义 > 包内 name > 文件名
fn resolve_name(custom: &str, pack_name: &str, file_path: &str) -> String {
    let custom = custom.trim();
    if !custom.is_empty() {
        return custom.to_string();
    }
    let inner = pack_name.trim();
    if !inner.is_empty() {
        return inner.to_string();
    }
    std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Modpack")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_args_accepts_string_and_array() {
        assert_eq!(join_args(Some(&json!("-Xmx4G"))).as_deref(), Some("-Xmx4G"));
        assert_eq!(
            join_args(Some(&json!(["-Xmx4G", "-Dfoo=1"]))).as_deref(),
            Some("-Xmx4G -Dfoo=1")
        );
        assert_eq!(join_args(Some(&json!("  "))), None);
        assert_eq!(join_args(None), None);
    }

    #[test]
    fn resolve_name_prefers_custom_then_inner() {
        assert_eq!(resolve_name("自定义", "包内", "D:/a.zip"), "自定义");
        assert_eq!(resolve_name("", "包内", "D:/a.zip"), "包内");
        assert_eq!(resolve_name("", "", "D:/小寂寂寂.zip"), "小寂寂寂");
    }
}
