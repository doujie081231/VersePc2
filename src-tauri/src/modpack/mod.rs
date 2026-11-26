// modpack/mod.rs — 整合包导入模块入口
//
// 模块结构：
//   mod.rs              — 入口 + 格式识别分发
//   theseus/             — theseus 整合包下载引擎（含 mrpack 下载核心）
//   theseus_adapter.rs   — 适配层：把 theseus 的任务制 API 包装为同步返回 JSON
//
// 路由：由 api/modpacks.rs 调用 import_modpack() 入口
//
// 设计原则：
//   - 格式识别后委托给 theseus 适配层处理
//   - theseus 提供 mrpack 下载核心算法（并发下载、SHA1校验、解压overrides）
//   - 进度回调通过 emit_to 通知前端（import-progress 事件）
//   - CurseForge / HMCL / 普通 ZIP 暂时走简化的 ZIP 解压流程

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::storage;

/// 整合包导入取消注册表：token -> abort_flag
///
/// 说明：
/// - 前端导入整合包时生成唯一 token 并随请求传给后端，后端在此注册一个 abort_flag。
/// - 前端点击"取消"时调用 /api/modpack/cancel 接口，本模块把对应 flag 置为 true，
///   下载循环感知后中断整个导入流程。
/// - 导入结束（成功或失败）后由导入流程主动 unregister，避免内存泄漏。
static MODPACK_ABORT_FLAGS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 注册一个取消标志，返回该标志引用
pub fn register_modpack_abort(token: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    MODPACK_ABORT_FLAGS
        .lock()
        .unwrap()
        .insert(token.to_string(), flag.clone());
    flag
}

/// 触发取消：把对应 token 的 flag 置为 true，返回是否找到该 token
pub fn cancel_modpack_abort(token: &str) -> bool {
    let guard = MODPACK_ABORT_FLAGS.lock().unwrap();
    if let Some(flag) = guard.get(token) {
        flag.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// 移除注册的取消标志
pub fn unregister_modpack_abort(token: &str) {
    MODPACK_ABORT_FLAGS.lock().unwrap().remove(token);
}

pub mod curseforge;
pub mod curseforge_shared;
pub mod mcbbs;
pub mod mmc;
pub mod mrpack_native;
pub mod theseus;
pub mod theseus_adapter;

/// 导入整合包统一入口
///
/// # 参数
/// - `app`: Tauri 应用句柄（用于发送进度事件）
/// - `file_path`: 整合包文件路径
/// - `custom_version_name`: 用户自定义版本名（为空时用整合包名）
/// - `update_version_id`: 可选，更新整合包时指定要覆盖的已存在版本目录（不传则新建版本）
/// - `cancel_token`: 可选，前端生成的会话标识，用于支持导入过程中的"取消下载"
///
/// # 返回
/// `{ success: bool, versionId?: string, name?: string, error?: string }`
pub async fn import_modpack(
    app: &AppHandle,
    file_path: &str,
    custom_version_name: &str,
    update_version_id: Option<&str>,
    cancel_token: Option<&str>,
) -> Value {
    eprintln!("[modpack] 开始导入: {}", file_path);

    // 验证文件存在
    if !Path::new(file_path).exists() {
        return json!({
            "success": false,
            "error": format!("文件不存在: {}", file_path)
        });
    }

    // 识别整合包格式（同时返回内层基础目录，兼容被套了一层文件夹的整合包）
    let (format, base) = match detect_format(file_path) {
        Ok(f) => f,
        Err(e) => {
            return json!({ "success": false, "error": e });
        }
    };

    eprintln!("[modpack] 识别为 {} 格式（base={:?}）", format, base);

    let result = match format {
        ModpackFormat::Mrpack => {
            // 走原生实现的完整逻辑
            // 不走 theseus 的 install_zipped_mrpack_files_with_reporter
            mrpack_native::import_mrpack(
                app,
                file_path,
                custom_version_name,
                update_version_id,
                cancel_token,
            )
            .await
        }
        ModpackFormat::Mmc => {
            // MultiMC / Prism 格式（mmc-pack.json + patches/ + .minecraft/）
            mmc::import_mmc(app, file_path, custom_version_name).await
        }
        ModpackFormat::Curseforge => {
            // CurseForge 走独立的 curseforge 模块（theseus 不识别 manifest.json）
            curseforge::import_curseforge(app, file_path, custom_version_name, &base).await
        }
        ModpackFormat::Mcbbs => {
            // MCBBS / BBSMC：mcbbs.packmeta（或带 addons 的 manifest.json）+ overrides/
            mcbbs::import_mcbbs(app, file_path, custom_version_name, &base).await
        }
        ModpackFormat::Hmcl => {
            // HMCL：modpack.json + minecraft/（PCL 只复制 minecraft/ 到版本目录）
            theseus_adapter::import_hmcl(app, file_path, custom_version_name, &base).await
        }
        ModpackFormat::RawZip => {
            // 普通 ZIP：若含 .minecraft/（或 minecraft/）则以其为游戏根目录解压
            theseus_adapter::import_raw_zip(app, file_path, custom_version_name, &base).await
        }
    };

    // 通知前端完成
    let is_success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let import_error = result.get("error").and_then(|v| v.as_str()).unwrap_or("");
    let _ = app.emit(
        "import-progress",
        json!({
            "progress": 100,
            "message": if is_success {
                "导入完成".to_string()
            } else if !import_error.is_empty() {
                format!("导入失败: {}", import_error)
            } else {
                "导入失败".to_string()
            },
            "error": import_error,
            "stage": "completed"
        }),
    );

    // 失败时写入诊断日志，便于打包环境下定位问题
    if !is_success {
        let log_dir = data_dir().join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("modpack-import.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "[{}] 导入失败 (format={}, file={}) error={}",
                crate::modpack::curseforge_shared::now_iso(),
                format,
                file_path,
                import_error
            );
        }
    }

    result
}

/// 整合包格式枚举
#[derive(Debug, PartialEq)]
pub enum ModpackFormat {
    Mrpack,     // Modrinth .mrpack
    Mmc,        // MultiMC / Prism（含 mmc-pack.json）
    Curseforge, // CurseForge（含 manifest.json 或 minecraftinstance.xml）
    Mcbbs,      // MCBBS / BBSMC（含 mcbbs.packmeta，或带 addons 的 manifest.json）
    Hmcl,       // HMCL（含 modpack.json）
    RawZip,     // 普通 ZIP
}

impl std::fmt::Display for ModpackFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModpackFormat::Mrpack => write!(f, "Modrinth .mrpack"),
            ModpackFormat::Mmc => write!(f, "MultiMC"),
            ModpackFormat::Curseforge => write!(f, "CurseForge"),
            ModpackFormat::Mcbbs => write!(f, "MCBBS"),
            ModpackFormat::Hmcl => write!(f, "HMCL"),
            ModpackFormat::RawZip => write!(f, "普通 ZIP"),
        }
    }
}

/// 通过读取压缩包内的关键文件识别整合包格式
///
/// 返回 `(格式, 内层基础目录)`。基础目录用于兼容「整包被套了一层文件夹」的
/// 下载产物（如 `小寂寂寂整合包/manifest.json`），为空表示关键文件在压缩包根目录。
/// 判定顺序与 PCL 的 ModpackInstall 保持一致：
/// mcbbs.packmeta → mmc-pack.json → modrinth.index.json →
/// manifest.json（有 addons 为 MCBBS，否则 CurseForge）→ modpack.json。
fn detect_format(file_path: &str) -> Result<(ModpackFormat, String), String> {
    let file = std::fs::File::open(file_path)
        .map_err(|e| format!("无法打开文件: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("无法读取 ZIP: {}", e))?;

    // 收集全部文件条目：保留原始大小写（解压时要用），另存小写用于匹配
    let mut names: Vec<(String, String)> = Vec::new();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let original = entry.name().replace('\\', "/");
        let lower = original.to_lowercase();
        names.push((original, lower));
    }

    // 候选基础目录：根目录 + 单层子目录（PCL 同样只检查一层）
    let mut bases: Vec<String> = vec![String::new()];
    for (original, _) in &names {
        if let Some(idx) = original.find('/') {
            let base = &original[..=idx];
            let rest = &original[idx + 1..];
            if !rest.contains('/') && !bases.iter().any(|b| b == base) {
                bases.push(base.to_string());
            }
        }
    }

    for base in &bases {
        let base_lower = base.to_lowercase();
        let has = |file: &str| {
            let target = format!("{}{}", base_lower, file);
            names.iter().any(|(_, lower)| lower == &target)
        };

        if has("mcbbs.packmeta") {
            return Ok((ModpackFormat::Mcbbs, base.clone()));
        }
        if has("mmc-pack.json") {
            return Ok((ModpackFormat::Mmc, base.clone()));
        }
        if has("modrinth.index.json") {
            return Ok((ModpackFormat::Mrpack, base.clone()));
        }
        if has("manifest.json") {
            // MCBBS 的 manifest.json 带 addons，CurseForge 的没有
            let is_mcbbs = read_zip_json(&mut archive, &format!("{}manifest.json", base))
                .map(|v| v.get("addons").is_some())
                .unwrap_or(false);
            return Ok((
                if is_mcbbs { ModpackFormat::Mcbbs } else { ModpackFormat::Curseforge },
                base.clone(),
            ));
        }
        if has("minecraftinstance.xml") {
            return Ok((ModpackFormat::Curseforge, base.clone()));
        }
        if has("modpack.json") {
            return Ok((ModpackFormat::Hmcl, base.clone()));
        }
    }

    Ok((ModpackFormat::RawZip, String::new()))
}

/// 按名称读取 ZIP 内 JSON（忽略大小写与反斜杠）
pub(crate) fn read_zip_json(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Value> {
    let target = name.replace('\\', "/").to_lowercase();
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else { continue };
        if entry.name().replace('\\', "/").to_lowercase() != target {
            continue;
        }
        let mut buf = String::new();
        if std::io::Read::read_to_string(&mut entry, &mut buf).is_err() {
            return None;
        }
        let buf = buf.trim_start_matches('\u{feff}');
        return serde_json::from_str(buf).ok();
    }
    None
}

/// 发送导入进度事件
pub fn emit_progress(app: &AppHandle, progress: u32, message: &str, stage: &str) {
    let _ = app.emit(
        "import-progress",
        json!({
            "progress": progress,
            "message": message,
            "stage": stage
        }),
    );
}

/// 写入整合包导入日志（modpack-import.log），便于复现定位导入卡住/失败。
///
/// `tag` 区分导入器（MMC / ZIP / CurseForge），日志行形如
/// `[2026-01-01 12:00:00] [MMC] 消息`。
pub fn import_log(tag: &str, msg: &str) {
    eprintln!("[{}] {}", tag, msg);
    let dir = data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("modpack-import.log");
    let line = format!(
        "[{}] [{}] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        tag,
        msg
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

/// 发送带文件列表的导入进度事件
#[allow(dead_code)]
pub fn emit_progress_with_files(
    app: &AppHandle,
    progress: u32,
    message: &str,
    stage: &str,
    files: &[Value],
    current_file: &str,
) {
    let _ = app.emit(
        "import-progress",
        json!({
            "progress": progress,
            "message": message,
            "stage": stage,
            "files": files,
            "currentFile": current_file
        }),
    );
}

/// 标准化版本 ID（避免非法字符）
/// 规范化版本 ID（用于版本目录名 / version.json 的 id）
/// 这里只过滤 Windows 不允许的非法字符，
/// 而不是把所有非 ASCII 字符都替换成下划线（否则中文整合包名会变成一串 "_"）。
pub fn normalize_version_id(name: &str) -> String {
    let trimmed = name.trim();
    let mut out = String::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        // 过滤 Windows 路径非法字符、控制字符
        match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('_'),
            c if c.is_control() => out.push('_'),
            _ => out.push(c),
        }
    }
    // Windows 保留名（CON/PRN/AUX/NUL/COMx/LPTx）后接下划线避免冲突
    let upper = out.to_uppercase();
    let reserved = matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
            | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4"
            | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    );
    if reserved {
        out.push('_');
    }
    if out.is_empty() {
        out.push_str("Version");
    }
    out
}

/// 获取 data 目录路径
#[allow(dead_code)]
pub fn data_dir() -> std::path::PathBuf {
    storage::resolve_data_dir()
}

/// 获取 versions 目录路径
pub fn versions_dir() -> std::path::PathBuf {
    storage::resolve_data_dir().join("versions")
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use std::io::Write;

    fn make_zip(entries: &[(&str, &str)]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut writer = zip::ZipWriter::new(file.reopen().unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (name, content) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        file
    }

    fn detect(entries: &[(&str, &str)]) -> (ModpackFormat, String) {
        let file = make_zip(entries);
        detect_format(file.path().to_str().unwrap()).unwrap()
    }

    #[test]
    fn detect_mmc_pack_at_root_and_subfolder() {
        assert_eq!(detect(&[("mmc-pack.json", "{}")]).0, ModpackFormat::Mmc);
        assert_eq!(
            detect(&[("GTNH/mmc-pack.json", "{}"), ("GTNH/instance.cfg", "name=x")]),
            (ModpackFormat::Mmc, "GTNH/".to_string())        );
    }

    #[test]
    fn detect_mcbbs_packmeta_and_addons_manifest() {
        // mcbbs.packmeta 在根目录
        assert_eq!(
            detect(&[("mcbbs.packmeta", "{}"), ("overrides/mods/a.jar", "")]),
            (ModpackFormat::Mcbbs, String::new())
        );
        // 整包被套了一层文件夹
        assert_eq!(
            detect(&[
                ("小寂寂寂整合包/mcbbs.packmeta", "{}"),
                ("小寂寂寂整合包/overrides/mods/a.jar", "")
            ]),
            (ModpackFormat::Mcbbs, "小寂寂寂整合包/".to_string())
        );
        // 带 addons 的 manifest.json 是 MCBBS，不是 CurseForge
        assert_eq!(
            detect(&[
                ("manifest.json", r#"{"addons":[{"id":"game","version":"1.20.1"}]}"#),
                ("overrides/mods/a.jar", "")
            ]),
            (ModpackFormat::Mcbbs, String::new())
        );
        // 没有 addons 的 manifest.json 仍是 CurseForge
        assert_eq!(
            detect(&[
                ("manifest.json", r#"{"minecraft":{"version":"1.20.1"}}"#),
                ("overrides/mods/a.jar", "")
            ]),
            (ModpackFormat::Curseforge, String::new())
        );
        // 套了一层文件夹的 CurseForge / HMCL 也要识别出 base
        assert_eq!(
            detect(&[
                ("Pack/manifest.json", r#"{"minecraft":{"version":"1.20.1"}}"#),
                ("Pack/overrides/mods/a.jar", "")
            ]),
            (ModpackFormat::Curseforge, "Pack/".to_string())
        );
        assert_eq!(
            detect(&[
                ("Pack/modpack.json", r#"{"gameVersion":"1.20.1"}"#),
                ("Pack/minecraft/mods/a.jar", "")
            ]),
            (ModpackFormat::Hmcl, "Pack/".to_string())
        );
    }

    #[test]
    fn detect_keeps_existing_formats() {
        assert_eq!(
            detect(&[("modrinth.index.json", "{}")]),
            (ModpackFormat::Mrpack, String::new())
        );
        assert_eq!(
            detect(&[("modpack.json", r#"{"gameVersion":"1.20.1"}"#)]),
            (ModpackFormat::Hmcl, String::new())
        );
        assert_eq!(
            detect(&[("mods/a.jar", "")]),
            (ModpackFormat::RawZip, String::new())
        );
    }
}
