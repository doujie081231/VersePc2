// modpack/mmc.rs — MultiMC / Prism 风格整合包（mmc-pack.json）导入
//
// 导入流程（对齐 MultiMC JSON-Patches 规范）：
//   1. 读取 mmc-pack.json 的 components，确定 MC 版本与加载器（Forge / NeoForge / Fabric）
//   2. 读取 patches/ 下的 JSON-Patch，按 order 排序，仅保留 components 中存在的 uid
//   3. 把 Patch 中的 jvmArgs / libraries / tweakers / assetIndex / mainClass /
//      minecraftArguments / compatibleJavaMajors 汇总为版本覆盖信息
//   4. 安装原版与加载器，合并产出独立版本 JSON，应用上面的覆盖信息
//   5. 解压 .minecraft/ 与 libraries/ 到版本目录，迁移 instance.cfg 的可映射设置
//   6. 校验/补全库文件、下载资源与客户端 JAR
//
// 说明：Cleanroom（net.minecraftforge 组件版本以 0. 开头）的版本信息由
// patches/net.minecraftforge.json 提供，因此不额外下载其安装器。

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use tauri::AppHandle;

use super::curseforge_shared as cf_shared;
use super::{emit_progress, normalize_version_id, versions_dir};
use crate::modloaders::shared as ml_shared;

/// MMC 整合包解析出的覆盖信息
#[derive(Default, Debug)]
struct MmcPackInfo {
    is_minecraft_overridden: bool,
    is_forge_overridden: bool,
    is_neoforge_overridden: bool,
    is_cleanroom_overridden: bool,
    is_fabric_overridden: bool,
    is_quilt_overridden: bool,
    is_mc_args_edited: bool,
    overrided_json: Value,
}

/// mmc-pack.json 的 components 解析结果
#[derive(Default, Debug)]
struct MmcComponents {
    minecraft: String,
    forge: String,
    neoforge: String,
    fabric: String,
    quilt: String,
}

impl MmcComponents {
    fn has_loader(&self) -> bool {
        !self.forge.is_empty()
            || !self.neoforge.is_empty()
            || !self.fabric.is_empty()
            || !self.quilt.is_empty()
    }
    fn is_cleanroom(&self) -> bool {
        self.forge.starts_with("0.")
    }
}

/// 导入 MMC 整合包
pub async fn import_mmc(app: &AppHandle, file_path: &str, custom_version_name: &str) -> Value {
    eprintln!("[mmc] 开始导入: {}", file_path);

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return json!({ "success": false, "error": format!("无法打开文件: {}", e) }),
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return json!({ "success": false, "error": format!("无法读取 ZIP: {}", e) }),
    };

    // 1. 定位 mmc-pack.json（可能在压缩包根目录或单层子目录中）
    let (base, pack_idx) = match find_pack_entry(&mut archive) {
        Some(v) => v,
        None => {
            return json!({
                "success": false,
                "error": "整合包缺少 mmc-pack.json，不是有效的 MultiMC 整合包"
            })
        }
    };
    eprintln!("[mmc] 基础目录: {:?}", base);

    let pack_json: Value = match read_entry_json(&mut archive, pack_idx) {
        Ok(v) => v,
        Err(e) => return json!({ "success": false, "error": e }),
    };
    let components = parse_components(&pack_json);
    if components.minecraft.is_empty() {
        return json!({ "success": false, "error": "该 MMC 整合包未提供游戏版本信息，无法安装！" });
    }

    // 2. JSON-Patches（仅保留 components 中声明的 uid）
    let patch_jsons = collect_patches(&mut archive, &base);
    let pack_info = build_pack_info(&pack_json, &patch_jsons);
    if pack_info.is_quilt_overridden || !components.quilt.is_empty() {
        return json!({
            "success": false,
            "error": format!(
                "整合包要求 Quilt Loader {}，但当前版本暂不支持 Quilt，请使用 Fabric 版本的整合包",
                components.quilt
            )
        });
    }
    if components.is_cleanroom()
        && !(pack_info.is_forge_overridden || pack_info.is_cleanroom_overridden)
    {
        return json!({
            "success": false,
            "error": format!(
                "整合包要求 Cleanroom {}，但压缩包内缺少对应的 JSON-Patch，无法安装",
                components.forge
            )
        });
    }

    // 3. 版本名：优先用户输入，其次 instance.cfg 的 name，最后文件名
    let instance_cfg = read_instance_cfg(&mut archive, &base);
    let pack_name = resolve_pack_name(custom_version_name, &instance_cfg, file_path);
    let version_id = cf_shared::dedupe_version_id(&normalize_version_id(&pack_name));
    let version_dir = versions_dir().join(&version_id);

    emit_progress(
        app,
        5,
        &format!("整合包: {}  MC: {}", pack_name, components.minecraft),
        "prepare",
    );

    if let Err(e) = std::fs::create_dir_all(&version_dir) {
        return json!({ "success": false, "error": format!("无法创建版本目录: {}", e) });
    }

    // 4. 安装原版
    emit_progress(app, 8, "正在准备基础版本...", "base");
    let app_owned = app.clone();
    let on_progress = Some(Box::new(move |pct: u32, msg: String| {
        let mapped = 8 + (pct as u64 * 12 / 100).min(12) as u32;
        emit_progress(&app_owned, mapped, &msg, "base")
    }) as ml_shared::BaseVersionProgress);
    if let Err(e) = ml_shared::ensure_base_version_installed(&components.minecraft, on_progress).await {
        let _ = std::fs::remove_dir_all(&version_dir);
        return json!({ "success": false, "versionId": version_id, "error": e });
    }

    // 提前读取原版 JSON 并复制客户端 JAR：加载器安装完成后可能清理原版目录，
    // 之后就无法再从这里取到基准版本信息。
    let vanilla_json_path = versions_dir()
        .join(&components.minecraft)
        .join(format!("{}.json", components.minecraft));
    let vanilla_json: Value = match std::fs::read_to_string(&vanilla_json_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(v) => v,
        None => {
            let _ = std::fs::remove_dir_all(&version_dir);
            return json!({
                "success": false,
                "versionId": version_id,
                "error": format!("缺少原版版本 JSON: {}", vanilla_json_path.display())
            });
        }
    };
    let vanilla_jar = versions_dir()
        .join(&components.minecraft)
        .join(format!("{}.jar", components.minecraft));
    if vanilla_jar.exists() {
        let _ = std::fs::copy(&vanilla_jar, version_dir.join(format!("{}.jar", version_id)));
    }

    // 5. 安装加载器（Cleanroom 的库由 JSON-Patch 提供，无需安装器）
    let loader_version_id = if components.is_cleanroom() {
        None
    } else {
        let lv_id: Option<String> = if !components.forge.is_empty() {
            Some(format!("{}-forge-{}", components.minecraft, components.forge))
        } else if !components.neoforge.is_empty() {
            Some(format!("{}-neoforge-{}", components.minecraft, components.neoforge))
        } else if !components.fabric.is_empty() {
            Some(format!("fabric-loader-{}-{}", components.fabric, components.minecraft))
        } else {
            None
        };
        if components.has_loader() {
            emit_progress(app, 25, "正在安装模组加载器...", "loader-install");
            if let Err(e) = super::mrpack_native::install_loader(
                &components.forge,
                &components.neoforge,
                &components.fabric,
                &components.minecraft,
                app,
            )
            .await
            {
                let _ = std::fs::remove_dir_all(&version_dir);
                return json!({ "success": false, "versionId": version_id, "error": e });
            }
        }
        lv_id
    };

    // 6. 合并版本 JSON（原版 + 加载器 + MMC 覆盖信息）
    emit_progress(app, 42, "正在创建版本配置...", "version-config");
    match build_version_json(
        &version_id,
        &components,
        loader_version_id.as_deref(),
        &pack_info,
        &vanilla_json,
    ) {
        Ok(mut vj) => {
            // 补充基础版本号，供依赖检查与启动参数使用
            if let Some(obj) = vj.as_object_mut() {
                obj.entry("clientVersion".to_string())
                    .or_insert_with(|| json!(components.minecraft));
            }
            let _ = std::fs::write(
                version_dir.join(format!("{}.json", version_id)),
                serde_json::to_string_pretty(&vj).unwrap_or_default(),
            );
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&version_dir);
            return json!({ "success": false, "versionId": version_id, "error": e });
        }
    }

    // 8. 解压实例文件（.minecraft/ 与 libraries/）与图标
    emit_progress(app, 45, "正在解压整合包文件...", "extract");
    if let Err(e) = extract_instance_files(&mut archive, &base, &version_dir, app) {
        eprintln!("[mmc] 解压失败: {}", e);
        let _ = std::fs::remove_dir_all(&version_dir);
        return json!({ "success": false, "versionId": version_id, "error": e });
    }
    extract_instance_icon(&mut archive, &base, &version_dir, &instance_cfg);

    // 9. 迁移 instance.cfg 中可映射的启动设置（版本隔离 + JVM 参数）
    apply_instance_settings(&version_dir, &instance_cfg);

    // 10. 删除临时加载器目录（内容已合并进版本 JSON）
    if let Some(lv_id) = loader_version_id.as_deref() {
        let loader_dir = versions_dir().join(lv_id);
        if loader_dir.exists() && loader_dir != version_dir {
            let _ = std::fs::remove_dir_all(&loader_dir);
        }
    }

    // 11. 校验/补全库文件
    emit_progress(app, 70, "正在验证整合包完整性...", "verify");
    let (verify_ok, _checked, missing) = cf_shared::verify_import_libs(&version_id).await;
    if !verify_ok {
        eprintln!("[mmc] 库文件补全失败: {} 个缺失", missing);
        return json!({
            "success": false,
            "versionId": version_id,
            "error": format!("整合包库文件补全失败: {} 个文件缺失，请检查网络后重试", missing)
        });
    }

    // 12. 资源与客户端 JAR（失败非致命，启动时会自动补全）
    let settings = crate::storage::load_settings();
    let merged_json = cf_shared::resolve_version_json(&version_id);
    if let Some(ref merged) = merged_json
        && let Some(asset_index) = merged.get("assetIndex")
    {
        emit_progress(app, 82, "正在下载游戏资源...", "assets");
        if let Err(e) = super::mrpack_native::download_assets(app, asset_index, &settings).await {
            eprintln!("[mmc] 资源下载异常(非致命): {}", e);
        }
    }
    if let Some(ref merged) = merged_json {
        download_client_jar_if_missing(app, &version_id, merged).await;
    }

    // 13. pack-info.json（供版本列表识别为整合包）
    let pack_info_json = json!({
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": components.minecraft,
        "packFormat": "mmc",
        "forgeVersion": components.forge,
        "neoforgeVersion": components.neoforge,
        "fabricVersion": components.fabric,
        "cleanroom": components.is_cleanroom(),
        "importedAt": cf_shared::now_iso(),
        "sourceFile": file_path,
        "targetVersion": "",
    });
    let _ = std::fs::write(
        version_dir.join("pack-info.json"),
        serde_json::to_string_pretty(&pack_info_json).unwrap_or_default(),
    );

    emit_progress(
        app,
        100,
        &format!("整合包 \"{}\" 导入完成！", pack_name),
        "done",
    );

    json!({
        "success": true,
        "name": pack_name,
        "versionId": version_id,
        "mcVersion": components.minecraft,
        "targetVersion": "",
        "loaderVersionId": loader_version_id,
    })
}

// ============== 压缩包读取 ==============

/// 在压缩包中查找 mmc-pack.json，返回 (基础目录前缀, 条目下标)
fn find_pack_entry(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<(String, usize)> {
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        let name = entry.name().replace('\\', "/");
        let lower = name.to_lowercase();
        if lower.ends_with("mmc-pack.json") {
            let base = name[..name.len() - "mmc-pack.json".len()].to_string();
            // 只接受根目录或单层子目录
            if base.matches('/').count() <= 1 {
                return Some((base, i));
            }
        }
    }
    None
}

fn read_entry_bytes(
    archive: &mut zip::ZipArchive<std::fs::File>,
    idx: usize,
) -> Option<Vec<u8>> {
    let mut entry = archive.by_index(idx).ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn read_entry_json(
    archive: &mut zip::ZipArchive<std::fs::File>,
    idx: usize,
) -> Result<Value, String> {
    let bytes = read_entry_bytes(archive, idx).ok_or_else(|| "读取 mmc-pack.json 失败".to_string())?;
    parse_json_bytes(&bytes).ok_or_else(|| "解析 mmc-pack.json 失败".to_string())
}

/// 解析 JSON 字节流：容忍 Windows 工具写入的 UTF-8 BOM
fn parse_json_bytes(bytes: &[u8]) -> Option<Value> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    serde_json::from_slice(bytes).ok()
}

fn read_entry_text(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<String> {
    let target = name.replace('\\', "/").to_lowercase();
    let idx = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|e| e.name().replace('\\', "/").to_lowercase() == target)
            .unwrap_or(false)
    })?;
    let bytes = read_entry_bytes(archive, idx)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// 解析 components（uid → 版本）
fn parse_components(pack_json: &Value) -> MmcComponents {
    let mut c = MmcComponents::default();
    if let Some(arr) = pack_json.get("components").and_then(|v| v.as_array()) {
        for comp in arr {
            let uid = comp.get("uid").and_then(|v| v.as_str()).unwrap_or("");
            let ver = comp.get("version").and_then(|v| v.as_str()).unwrap_or("");
            match uid {
                "net.minecraft" => c.minecraft = ver.to_string(),
                "net.minecraftforge" => c.forge = ver.to_string(),
                "net.neoforged" => c.neoforge = ver.to_string(),
                "net.fabricmc.fabric-loader" => c.fabric = ver.to_string(),
                "org.quiltmc.quilt-loader" => c.quilt = ver.to_string(),
                // org.lwjgl 等组件的 Patch 仍会在 build_pack_info 中应用
                _ => {}
            }
        }
    }
    c
}

/// 收集 patches/ 目录下所有 Patch（按 order 升序）
fn collect_patches(archive: &mut zip::ZipArchive<std::fs::File>, base: &str) -> Vec<Value> {
    let prefix = format!("{}patches/", base);
    let mut items: Vec<(i64, usize, Value)> = Vec::new();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        let name = entry.name().replace('\\', "/");
        let lower = name.to_lowercase();
        if !lower.starts_with(&prefix.to_lowercase()) || lower.ends_with('/') {
            continue;
        }
        if !lower.ends_with(".json") {
            continue;
        }
        drop(entry);
        let Some(bytes) = read_entry_bytes(archive, i) else { continue };
        let Some(patch) = parse_json_bytes(&bytes) else { continue };
        let order = patch.get("order").and_then(|v| v.as_i64()).unwrap_or(0);
        items.push((order, i, patch));
    }
    items.sort_by_key(|(order, idx, _)| (*order, *idx));
    items.into_iter().map(|(_, _, p)| p).collect()
}

// ============== JSON-Patches 应用 ==============

/// 把 Patch 汇总为版本覆盖信息（对齐 MultiMC JSON-Patches 规范）
fn build_pack_info(pack_json: &Value, patches: &[Value]) -> MmcPackInfo {
    let mut info = MmcPackInfo {
        overrided_json: json!({}),
        ..Default::default()
    };
    if patches.is_empty() {
        return info;
    }

    let component_uids: HashSet<String> = pack_json
        .get("components")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("uid").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut tweakers: Option<String> = None;
    let mut asset_index: Option<Value> = None;
    let mut java_version: Option<Value> = None;
    let mut main_class: Option<String> = None;
    let mut game_args: Vec<Value> = Vec::new();
    let mut jvm_args: Vec<Value> = Vec::new();
    let mut libraries: Vec<Value> = Vec::new();

    for patch in patches {
        let uid = patch.get("uid").and_then(|v| v.as_str()).unwrap_or("");
        // 只应用 components 中声明的组件 Patch
        if !component_uids.contains(uid) {
            continue;
        }

        match uid {
            "net.minecraft" => info.is_minecraft_overridden = true,
            "net.minecraftforge" => {
                let ver = patch.get("version").and_then(|v| v.as_str()).unwrap_or("");
                if ver.starts_with("0.") {
                    info.is_cleanroom_overridden = true;
                } else {
                    info.is_forge_overridden = true;
                }
            }
            "net.neoforged" => info.is_neoforge_overridden = true,
            "net.fabricmc.fabric-loader" => info.is_fabric_overridden = true,
            "org.quiltmc.quilt-loader" => info.is_quilt_overridden = true,
            _ => {}
        }

        if let Some(arr) = patch.get("+jvmArgs").and_then(|v| v.as_array()) {
            jvm_args.extend(arr.iter().cloned());
        }

        if let Some(arr) = patch.get("libraries").and_then(|v| v.as_array()) {
            libraries.extend(arr.iter().map(normalize_library));
        }
        if let Some(arr) = patch.get("+libraries").and_then(|v| v.as_array()) {
            libraries.extend(arr.iter().map(normalize_library));
        }

        if let Some(arr) = patch.get("+tweakers").and_then(|v| v.as_array())
            && let Some(first) = arr.first().and_then(|v| v.as_str())
        {
            tweakers = Some(first.to_string());
        }

        if let Some(v) = patch.get("assetIndex") {
            asset_index = Some(v.clone());
        }

        if let Some(args) = patch.get("minecraftArguments").and_then(|v| v.as_str()) {
            for token in args.split_whitespace() {
                game_args.push(json!(token));
            }
            info.is_mc_args_edited = true;
        }

        if let Some(mc) = patch.get("mainClass").and_then(|v| v.as_str()) {
            main_class = Some(mc.to_string());
        }

        if let Some(arr) = patch.get("compatibleJavaMajors").and_then(|v| v.as_array()) {
            java_version = Some(pick_java_version(arr));
        }
    }

    if let Some(t) = tweakers {
        game_args.push(json!("--tweakClass"));
        game_args.push(json!(t));
    }

    // MMC Patch 不包含标准 JVM 启动参数，这里补齐（与启动器合并逻辑一致）
    let mut final_jvm = vec![
        json!("-Djava.library.path=${natives_directory}"),
        json!("-Dminecraft.launcher.brand=${launcher_name}"),
        json!("-Dminecraft.launcher.version=${launcher_version}"),
        json!("-cp"),
        json!("${classpath}"),
    ];
    final_jvm.extend(jvm_args);

    let mut overrided = Map::new();
    let mut arguments = Map::new();
    arguments.insert("game".to_string(), Value::Array(game_args));
    arguments.insert("jvm".to_string(), Value::Array(final_jvm));
    overrided.insert("arguments".to_string(), Value::Object(arguments));
    if let Some(mc) = main_class {
        overrided.insert("mainClass".to_string(), json!(mc));
    }
    if let Some(ai) = asset_index {
        overrided.insert("assetIndex".to_string(), ai);
    }
    if let Some(jv) = java_version {
        overrided.insert("javaVersion".to_string(), jv);
    }
    overrided.insert("libraries".to_string(), Value::Array(libraries));
    info.overrided_json = Value::Object(overrided);
    info
}

/// 规范化库条目：MultiMC 的 MMC-hint 字段在标准版本 JSON 中不存在，改为 hint
fn normalize_library(lib: &Value) -> Value {
    let mut out = lib.clone();
    if let Some(obj) = out.as_object_mut()
        && let Some(hint) = obj.remove("MMC-hint")
    {
        obj.insert("hint".to_string(), hint);
    }
    out
}

/// 选择 Java 主版本：优先 21 / 17 / 11 / 8，否则取数组第一项
fn pick_java_version(majors: &[Value]) -> Value {
    let mut version = 0i64;
    let mut component: Option<&str> = None;
    for m in majors {
        let Some(v) = m.as_i64() else { continue };
        if version > v {
            continue;
        }
        match v {
            21 => {
                version = 21;
                component = Some("java-runtime-delta");
            }
            17 => {
                version = 17;
                component = Some("java-runtime-gamma");
            }
            11 => {
                version = 11;
                component = None;
            }
            8 => {
                version = 8;
                component = Some("jre-legacy");
            }
            _ => {}
        }
    }
    if version == 0 {
        version = majors.first().and_then(|v| v.as_i64()).unwrap_or(8);
        component = None;
    }
    let mut jv = Map::new();
    jv.insert("majorVersion".to_string(), json!(version));
    if let Some(c) = component {
        jv.insert("component".to_string(), json!(c));
    }
    Value::Object(jv)
}

// ============== 版本 JSON 构建 ==============

/// 构建最终版本 JSON：原版 → MMC 覆盖 → 各加载器（未被 Patch 覆盖时）
fn build_version_json(
    version_id: &str,
    components: &MmcComponents,
    loader_version_id: Option<&str>,
    info: &MmcPackInfo,
    vanilla: &Value,
) -> Result<Value, String> {
    let mut output = if info.is_minecraft_overridden {
        info.overrided_json.clone()
    } else {
        let mut merged = vanilla.clone();
        if info.overrided_json.is_object() {
            json_merge(&mut merged, &info.overrided_json);
        }
        merged
    };

    // 加载器 JSON 合并（被 Patch 覆盖的加载器不再合并）
    let mut legacy_args: Vec<String> = Vec::new();
    if let Some(t) = vanilla.get("minecraftArguments").and_then(|v| v.as_str()) {
        legacy_args.push(t.to_string());
    }
    let mut merge_loader = |output: &mut Value, lv_id: &str, overridden: bool| {
        if overridden || lv_id.is_empty() {
            return;
        }
        let path = versions_dir().join(lv_id).join(format!("{}.json", lv_id));
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let Ok(mut lv) = serde_json::from_str::<Value>(&text) else { return };
        if let Some(obj) = lv.as_object_mut() {
            obj.remove("releaseTime");
            obj.remove("time");
            if let Some(args) = obj.get("minecraftArguments").and_then(|v| v.as_str()) {
                legacy_args.push(args.to_string());
            }
        }
        json_merge(output, &lv);
    };

    if !components.forge.is_empty() && !components.is_cleanroom() {
        merge_loader(
            &mut output,
            loader_version_id.unwrap_or(""),
            info.is_forge_overridden,
        );
    }
    if !components.neoforge.is_empty() {
        merge_loader(
            &mut output,
            loader_version_id.unwrap_or(""),
            info.is_neoforge_overridden,
        );
    }
    if !components.fabric.is_empty() {
        merge_loader(
            &mut output,
            loader_version_id.unwrap_or(""),
            info.is_fabric_overridden,
        );
    }

    // 旧版 minecraftArguments 合并（加载器 JSON 常以字符串形式提供额外参数）
    if !legacy_args.is_empty()
        && let Some(obj) = output.as_object_mut()
    {
        let joined = legacy_args.join(" ");
        let deduped = dedup_space_args(&joined);
        if !deduped.is_empty() {
            obj.insert("minecraftArguments".to_string(), json!(deduped));
        }
    }
    if info.is_mc_args_edited
        && let Some(obj) = output.as_object_mut()
    {
        obj.remove("minecraftArguments");
    }

    let obj = output
        .as_object_mut()
        .ok_or_else(|| "版本 JSON 格式错误".to_string())?;
    obj.remove("inheritsFrom");
    obj.remove("_comment_");
    obj.remove("jar");
    obj.insert("id".to_string(), json!(version_id));
    obj.insert("time".to_string(), json!(cf_shared::now_iso()));
    obj.insert("releaseTime".to_string(), json!(cf_shared::now_iso()));
    Ok(output)
}

/// JSON 合并：对象递归合并，数组追加，其余直接覆盖（与 MultiMC/Merge 语义一致）
fn json_merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(k) {
                    Some(slot) => json_merge(slot, v),
                    None => {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (Value::Array(t), Value::Array(p)) => t.extend(p.iter().cloned()),
        (t, p) => *t = p.clone(),
    }
}

/// 参数字符串去重：按"flag + 紧随的值"分组去重后重新拼接
fn dedup_space_args(raw: &str) -> String {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut groups: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let cur = tokens[i];
        if cur.starts_with('-') {
            let mut group = cur.to_string();
            if i + 1 < tokens.len() && !tokens[i + 1].starts_with('-') {
                group.push(' ');
                group.push_str(tokens[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
            if !groups.contains(&group) {
                groups.push(group);
            }
        } else {
            if !groups.contains(&cur.to_string()) {
                groups.push(cur.to_string());
            }
            i += 1;
        }
    }
    groups.join(" ")
}

// ============== 实例文件与设置 ==============

/// 解压 `.minecraft/` 与 `libraries/` 到版本目录
fn extract_instance_files(
    archive: &mut zip::ZipArchive<std::fs::File>,
    base: &str,
    version_dir: &Path,
    app: &AppHandle,
) -> Result<(), String> {
    let mc_prefix = format!("{}.minecraft/", base);
    let lib_prefix = format!("{}libraries/", base);
    let mc_prefix_l = mc_prefix.to_lowercase();
    let lib_prefix_l = lib_prefix.to_lowercase();

    let mut targets: Vec<(usize, PathBuf)> = Vec::new();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        let name = entry.name().replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        let lower = name.to_lowercase();
        if lower.starts_with(&mc_prefix_l) {
            let rel = &name[mc_prefix.len()..];
            if cf_shared::is_modpack_path_safe(rel) {
                targets.push((i, version_dir.join(rel)));
            }
        } else if lower.starts_with(&lib_prefix_l) {
            let rel = &name[lib_prefix.len()..];
            if cf_shared::is_modpack_path_safe(rel) {
                targets.push((i, version_dir.join("libraries").join(rel)));
            }
        }
    }

    let total = targets.len().max(1);
    for (done, (idx, dest)) in targets.into_iter().enumerate() {
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(bytes) = read_entry_bytes(archive, idx) {
            let _ = std::fs::write(&dest, bytes);
        }
        if done % 50 == 0 {
            let pct = 45 + ((done as u64 * 20 / total as u64).min(20) as u32);
            emit_progress(app, pct, &format!("解压文件 {}/{}", done + 1, total), "extract");
        }
    }
    Ok(())
}

/// instance.cfg 解析结果（仅保留能映射到本启动器的字段）
#[derive(Default, Debug)]
struct InstanceCfg {
    name: String,
    icon_key: String,
    jvm_args: String,
    override_java_args: bool,
}

fn read_instance_cfg(archive: &mut zip::ZipArchive<std::fs::File>, base: &str) -> InstanceCfg {
    let mut cfg = InstanceCfg::default();
    let Some(text) = read_entry_text(archive, &format!("{}instance.cfg", base)) else {
        return cfg;
    };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let value = value.trim();
        match key {
            "name" => cfg.name = value.to_string(),
            "iconKey" => cfg.icon_key = value.to_string(),
            "JvmArgs" => cfg.jvm_args = value.to_string(),
            "OverrideJavaArgs" => {
                cfg.override_java_args = value.eq_ignore_ascii_case("true") || value == "1";
            }
            _ => {}
        }
    }
    cfg
}

/// 版本名：用户输入 > instance.cfg 的 name > 文件名
fn resolve_pack_name(custom_name: &str, cfg: &InstanceCfg, file_path: &str) -> String {
    let trimmed = custom_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    let cfg_name = cfg.name.trim();
    if !cfg_name.is_empty() && is_valid_folder_name(cfg_name) {
        return cfg_name.to_string();
    }
    Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Modpack")
        .to_string()
}

/// 与 MultiMC 一致的文件夹名校验：非空、不含非法字符、不是 Windows 保留名
fn is_valid_folder_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control())
    {
        return false;
    }
    let upper = name.to_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    !RESERVED.contains(&upper.as_str())
}

/// 迁移实例图标：instance.cfg 的 iconKey 指向 `{iconKey}.png`
fn extract_instance_icon(
    archive: &mut zip::ZipArchive<std::fs::File>,
    base: &str,
    version_dir: &Path,
    cfg: &InstanceCfg,
) {
    if cfg.icon_key.is_empty() {
        return;
    }
    let icon_name = if cfg.icon_key.to_lowercase().ends_with(".png") {
        cfg.icon_key.clone()
    } else {
        format!("{}.png", cfg.icon_key)
    };
    let entry_name = format!("{}{}", base, icon_name);
    let Some(bytes) = read_entry_bytes_by_name(archive, &entry_name) else {
        return;
    };
    let _ = std::fs::write(version_dir.join("icon.png"), bytes);
}

fn read_entry_bytes_by_name(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<Vec<u8>> {
    let target = name.replace('\\', "/").to_lowercase();
    let idx = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|e| e.name().replace('\\', "/").to_lowercase() == target)
            .unwrap_or(false)
    })?;
    read_entry_bytes(archive, idx)
}

/// 写入可映射的实例设置：版本隔离开启 + JVM 参数
fn apply_instance_settings(version_dir: &Path, cfg: &InstanceCfg) {
    let path = version_dir.join("version-settings.json");
    let mut settings: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let Some(obj) = settings.as_object_mut() else { return };
    // 实例文件解压在版本目录内，必须开启版本隔离才能生效
    obj.insert("isolation".to_string(), json!("on"));
    if !cfg.jvm_args.trim().is_empty() {
        let jvm_args = if cfg.override_java_args {
            cfg.jvm_args.trim().to_string()
        } else {
            let global = crate::utils::get_str(
                &crate::storage::load_settings(),
                "javaArgs",
            );
            let global = global.trim();
            if global.is_empty() {
                cfg.jvm_args.trim().to_string()
            } else {
                format!("{} {}", cfg.jvm_args.trim(), global)
            }
        };
        obj.insert("jvmArgs".to_string(), json!(jvm_args));
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&settings).unwrap_or_default(),
    );
}

/// 客户端 JAR 缺失时下载（失败非致命）
async fn download_client_jar_if_missing(app: &AppHandle, version_id: &str, merged: &Value) {
    let jar_path = versions_dir().join(version_id).join(format!("{}.jar", version_id));
    if jar_path.exists() {
        return;
    }
    let url = merged
        .get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|c| c.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(url) = url {
        emit_progress(app, 92, "正在下载客户端JAR...", "assets");
        if let Err(e) = crate::download::single::download_with_mirror(
            &url,
            &jar_path,
            None,
            None,
            "china-first",
            180,
            None,
        )
        .await
        {
            eprintln!("[mmc] 客户端JAR下载失败(非致命): {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_bytes_accepts_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"{\"a\":1}");
        assert_eq!(parse_json_bytes(&bytes).unwrap()["a"], 1);
    }

    #[test]
    fn json_merge_objects_recursively_and_arrays_append() {
        let mut base = json!({
            "libraries": [{"name": "a:b:1"}],
            "arguments": {"game": ["--username"]},
            "mainClass": "net.minecraft.client.main.Main"
        });
        let patch = json!({
            "libraries": [{"name": "c:d:2"}],
            "arguments": {"game": ["--tweakClass"]},
            "mainClass": "top.outlands.foundation.boot.Foundation"
        });
        json_merge(&mut base, &patch);
        assert_eq!(base["libraries"].as_array().unwrap().len(), 2);
        assert_eq!(base["arguments"]["game"].as_array().unwrap().len(), 2);
        assert_eq!(base["mainClass"], "top.outlands.foundation.boot.Foundation");
    }

    #[test]
    fn pick_java_version_prefers_known_majors() {
        let v = pick_java_version(&[json!(25), json!(17), json!(8)]);
        assert_eq!(v["majorVersion"], 17);
        assert_eq!(v["component"], "java-runtime-gamma");

        let v = pick_java_version(&[json!(25)]);
        assert_eq!(v["majorVersion"], 25);
        assert!(v.get("component").is_none());
    }

    #[test]
    fn build_pack_info_applies_only_declared_components() {
        let pack = json!({
            "components": [
                {"uid": "net.minecraft", "version": "1.12.2"},
                {"uid": "net.minecraftforge", "version": "0.7.14-alpha"}
            ]
        });
        let patches = vec![
            json!({
                "uid": "net.minecraftforge",
                "version": "0.7.14-alpha",
                "mainClass": "top.outlands.foundation.boot.Foundation",
                "+tweakers": ["net.minecraftforge.fml.common.launcher.FMLTweaker"],
                "libraries": [{"name": "com.cleanroommc:cleanroom:0.7.14-alpha:universal", "MMC-hint": "local"}]
            }),
            json!({
                "uid": "org.lwjgl",
                "version": "3.4.3",
                "+jvmArgs": ["-Dorg.lwjgl.util.Debug=false"],
                "libraries": [{"name": "org.lwjgl:lwjgl:3.4.3"}]
            }),
        ];

        let info = build_pack_info(&pack, &patches);
        assert!(info.is_cleanroom_overridden);
        assert!(!info.is_forge_overridden);
        assert_eq!(
            info.overrided_json["mainClass"],
            "top.outlands.foundation.boot.Foundation"
        );
        // 未声明的 org.lwjgl Patch 不应用
        assert_eq!(info.overrided_json["libraries"].as_array().unwrap().len(), 1);
        let lib = &info.overrided_json["libraries"][0];
        assert_eq!(lib["hint"], "local");
        assert!(lib.get("MMC-hint").is_none());
        // tweakers 转为 --tweakClass 参数
        let game = info.overrided_json["arguments"]["game"].as_array().unwrap();
        assert_eq!(game.len(), 2);
        assert_eq!(game[0], "--tweakClass");
        // JVM 参数补齐标准前缀
        let jvm = info.overrided_json["arguments"]["jvm"].as_array().unwrap();
        assert_eq!(jvm.len(), 5);
        assert_eq!(jvm[3], "-cp");
    }

    #[test]
    fn build_pack_info_without_patches_keeps_empty_object() {
        let pack = json!({"components": [{"uid": "net.minecraft", "version": "1.7.10"}]});
        let info = build_pack_info(&pack, &[]);
        assert!(info.overrided_json.is_object());
        assert!(info.overrided_json.as_object().unwrap().is_empty());
        assert!(!info.is_minecraft_overridden);
    }

    #[test]
    fn dedup_space_args_keeps_flag_value_pairs() {
        let out = dedup_space_args("--username Steve --username Steve --gameDir .");
        assert_eq!(out, "--username Steve --gameDir .");
    }

    #[test]
    fn folder_name_validation() {
        assert!(is_valid_folder_name("GTNH 2.7.0"));
        assert!(!is_valid_folder_name("a/b"));
        assert!(!is_valid_folder_name("CON"));
        assert!(!is_valid_folder_name(""));
    }

    #[test]
    fn parse_components_maps_loaders() {
        let pack = json!({
            "components": [
                {"uid": "net.minecraft", "version": "1.20.1"},
                {"uid": "net.minecraftforge", "version": "47.3.0"},
                {"uid": "org.lwjgl", "version": "3.3.1"}
            ]
        });
        let c = parse_components(&pack);
        assert_eq!(c.minecraft, "1.20.1");
        assert_eq!(c.forge, "47.3.0");
        assert!(!c.is_cleanroom());
        assert!(c.has_loader());
    }
}
