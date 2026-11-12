// bridge/extension.rs — 把浏览器扩展打包进 exe，实现「点一下就装好」
//
// 为什么要把扩展塞进 exe：
//   用户拿到的是单文件便携版，不该再去 GitHub 下载 zip、解压、拖进浏览器。
//   这里用 include_bytes! 把扩展全部文件编进二进制，运行时释放到
//   <data>/extension/，再注册原生宿主，最后拉起浏览器。
//
// 三条安装路径（由可靠到兜底）：
//   ① 注册表外部扩展（Edge 支持 HKCU\Software\Microsoft\Edge\Extensions\<id>）
//   ② 带 --load-extension 启动浏览器（立即生效，Chrome/Edge 都认）
//   ③ 打开 edge://extensions 让用户手动加载（最后兜底，附图文步骤）

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use super::register;

/// 扩展 ID（由 extension/manifest.json 的 key 推导，改密钥必须同步改这里）
pub const EXTENSION_ID: &str = "pfmgdfekkaaigjjigfgljcmnebbhlfoa";

/// 扩展版本，需与 manifest.json 的 version 一致
const EXTENSION_VERSION: &str = "1.2.3";

/// 编进二进制的扩展文件清单（路径相对 extension.rs 所在文件）
const EMBEDDED: &[(&str, &[u8])] = &[
    ("manifest.json", include_bytes!("../../../extension/manifest.json")),
    ("background.js", include_bytes!("../../../extension/background.js")),
    ("popup.html", include_bytes!("../../../extension/popup.html")),
    ("popup.js", include_bytes!("../../../extension/popup.js")),
    ("options.html", include_bytes!("../../../extension/options.html")),
    ("options.js", include_bytes!("../../../extension/options.js")),
    ("icons/16.png", include_bytes!("../../../extension/icons/16.png")),
    ("icons/48.png", include_bytes!("../../../extension/icons/48.png")),
    ("icons/128.png", include_bytes!("../../../extension/icons/128.png")),
];

/// 安装说明单独内嵌，释放到扩展目录的**上一级**（即 <data>/安装说明.txt）。
/// 这样「加载解压缩的扩展」选目录时不会选到一堆无关文件，用户又能在旁边找到说明。
const README_BYTES: &[u8] = include_bytes!("../../../extension/README-INSTALL.txt");

/// 释放目录：<data>/extension
pub fn install_dir() -> PathBuf {
    crate::storage::resolve_data_dir().join("extension")
}

/// 把内嵌文件写到安装目录。已存在且内容一致时跳过，避免每次启动都重写。
pub fn extract() -> Result<PathBuf, String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建扩展目录失败: {}", e))?;

    for (rel, bytes) in EMBEDDED {
        let dest = dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建子目录失败: {}", e))?;
        }
        // 内容一致就跳过（省 IO，也避免无谓地刷新文件时间戳）
        if let Ok(existing) = std::fs::read(&dest)
            && existing == *bytes {
                continue;
            }
        std::fs::write(&dest, bytes)
            .map_err(|e| format!("写入 {} 失败: {}", rel, e))?;
    }

    // 安装说明写到扩展目录上一级，供用户手动加载时参考
    if let Some(parent) = dir.parent() {
        let readme = parent.join("安装说明-浏览器扩展.txt");
        let need_write = std::fs::read(&readme).map(|x| x != README_BYTES).unwrap_or(true);
        if need_write {
            let _ = std::fs::write(&readme, README_BYTES);
        }
    }
    Ok(dir)
}

/// 清理释放的扩展目录
pub fn remove_files() -> bool {
    let dir = install_dir();
    if !dir.exists() {
        return true; // 本来就没有，视为成功
    }
    // 浏览器可能正占用扩展目录里的文件（比如刚加载过），失败时短暂重试一次
    if std::fs::remove_dir_all(&dir).is_ok() {
        return true;
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::remove_dir_all(&dir).is_ok()
}

// ============== 注册表：Edge 外部扩展 ==============

#[cfg(target_os = "windows")]
fn extension_registry_keys() -> Vec<String> {
    [
        r"Software\Microsoft\Edge\Extensions\",
        r"Software\Microsoft\Edge Beta\Extensions\",
        r"Software\Microsoft\Edge Dev\Extensions\",
        r"Software\Google\Chrome\Extensions\",
    ]
    .iter()
    .map(|base| format!("{}{}", base, EXTENSION_ID))
    .collect()
}

/// 写「外部扩展」注册表项。Edge 会在下次启动时提示「已添加扩展」，点启用即可。
#[cfg(target_os = "windows")]
fn write_extension_registry(dir: &Path) -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path_str = dir.to_string_lossy().to_string();
    let mut written = 0usize;

    for key_path in extension_registry_keys() {
        match hkcu.create_subkey(&key_path) {
            Ok((key, _)) => {
                let ok_path = key.set_value("path", &path_str).is_ok();
                let ok_ver = key.set_value("version", &EXTENSION_VERSION).is_ok();
                if ok_path && ok_ver {
                    written += 1;
                }
            }
            Err(e) => eprintln!("[bridge] 写扩展注册表失败 {}: {}", key_path, e),
        }
    }
    written
}

#[cfg(not(target_os = "windows"))]
fn write_extension_registry(_dir: &Path) -> usize {
    0
}

#[cfg(target_os = "windows")]
fn remove_extension_registry() -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut removed = 0usize;
    for key_path in extension_registry_keys() {
        if hkcu.delete_subkey(&key_path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(not(target_os = "windows"))]
fn remove_extension_registry() -> usize {
    0
}

#[cfg(target_os = "windows")]
fn count_extension_registry() -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut count = 0usize;
    for key_path in extension_registry_keys() {
        if hkcu.open_subkey(&key_path).is_ok() {
            count += 1;
        }
    }
    count
}

#[cfg(not(target_os = "windows"))]
fn count_extension_registry() -> usize {
    0
}

// ============== 浏览器定位与启动 ==============

/// 让子进程完全不继承我们的标准句柄。
///
/// 这是必须的：从原生消息宿主（`--native-host`）里拉起浏览器时，如果子进程继承了
/// stdout，浏览器会一直持有那个管道，Chrome 就永远等不到 EOF，扩展那边表现为
/// 「宿主不响应 / 超时」。Windows 上还要加 CREATE_NO_WINDOW，避免弹出黑框。
#[cfg(target_os = "windows")]
fn detach(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(target_os = "windows"))]
fn detach(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
}

/// 一个可启动的浏览器（非 Windows 分支不会构造，故允许字段未读取）
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct Browser {
    name: &'static str,
    exe: PathBuf,
    /// 扩展管理页 URL（Chrome 与 Edge 不同）
    extensions_url: &'static str,
}

/// 在注册表 App Paths 与常见安装目录里找浏览器
#[cfg(target_os = "windows")]
fn find_browsers() -> Vec<Browser> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut out: Vec<Browser> = Vec::new();

    let app_paths: [(&'static str, &'static str, &'static str); 3] = [
        ("msedge.exe", "Edge", "edge://extensions/"),
        ("chrome.exe", "Chrome", "chrome://extensions/"),
        ("brave.exe", "Brave", "brave://extensions/"),
    ];

    for (exe_name, label, url) in app_paths {
        let rel = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}", exe_name);
        let mut found: Option<PathBuf> = None;

        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let hk = RegKey::predef(root);
            if let Ok(key) = hk.open_subkey(&rel)
                && let Ok(p) = key.get_value::<String, _>("") {
                    let pb = PathBuf::from(p);
                    if pb.exists() {
                        found = Some(pb);
                        break;
                    }
                }
        }

        if found.is_none() {
            // 常见安装位置兜底
            let candidates = [
                format!(r"C:\Program Files (x86)\Microsoft\Edge\Application\{}", exe_name),
                format!(r"C:\Program Files\Microsoft\Edge\Application\{}", exe_name),
                format!(r"C:\Program Files\Google\Chrome\Application\{}", exe_name),
                format!(r"C:\Program Files (x86)\Google\Chrome\Application\{}", exe_name),
                format!(r"C:\Program Files\BraveSoftware\Brave-Browser\Application\{}", exe_name),
            ];
            for c in candidates {
                let pb = PathBuf::from(&c);
                if pb.exists() {
                    found = Some(pb);
                    break;
                }
            }
        }

        if let Some(exe) = found {
            out.push(Browser { name: label, exe, extensions_url: url });
        }
    }

    if out.is_empty() {
        // 最后兜底：交给系统默认浏览器（只能打开扩展页，不能 --load-extension）
        out.push(Browser {
            name: "默认浏览器",
            exe: PathBuf::new(),
            extensions_url: "edge://extensions/",
        });
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn find_browsers() -> Vec<Browser> {
    Vec::new()
}

/// 判断浏览器是否正在运行。
/// 关键：浏览器已经开着时，新起的进程只是把参数转给现有实例然后立刻退出，
/// --load-extension 会被静默忽略——必须先把这件事识别出来，否则用户会以为装好了。
#[cfg(target_os = "windows")]
fn is_browser_running(browser: &Browser) -> bool {
    if browser.exe.as_os_str().is_empty() {
        return false;
    }
    let stem = browser
        .exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        return false;
    }
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("IMAGENAME eq {}.exe", stem), "/NH"])
        .stdin(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .to_lowercase()
                .contains(&format!("{}.exe", stem.to_lowercase()))
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn is_browser_running(_browser: &Browser) -> bool {
    false
}

/// 尝试带扩展启动浏览器。
///
/// 注意 `--load-extension` 有两个现实限制，因此它只是**附加尝试**，不能当主路径：
///   1. Chrome 137 起正式版已移除该开关（仅 Chromium/Chrome for Testing 可用）
///   2. 浏览器已在运行时参数会被现有实例吞掉，扩展不会加载
///
/// 真正的可靠路径是「外部扩展注册表 + 重启浏览器」或「用户手动加载解压目录」。
#[cfg(target_os = "windows")]
fn launch_with_extension(browser: &Browser, dir: &Path) -> bool {
    if browser.exe.as_os_str().is_empty() || is_browser_running(browser) {
        return false;
    }
    let mut cmd = std::process::Command::new(&browser.exe);
    cmd.arg(format!("--load-extension={}", dir.to_string_lossy()))
        .arg(browser.extensions_url);
    detach(&mut cmd).spawn().is_ok()
}

#[cfg(not(target_os = "windows"))]
fn launch_with_extension(_browser: &Browser, _dir: &Path) -> bool {
    false
}

/// 打开浏览器扩展管理页（无论浏览器是否在运行都能用）
#[cfg(target_os = "windows")]
fn open_extensions_page(browser: &Browser) -> bool {
    if browser.exe.as_os_str().is_empty() {
        return open::that(browser.extensions_url).is_ok();
    }
    let mut cmd = std::process::Command::new(&browser.exe);
    cmd.arg(browser.extensions_url);
    detach(&mut cmd).spawn().is_ok()
}

#[cfg(not(target_os = "windows"))]
fn open_extensions_page(_browser: &Browser) -> bool {
    false
}

/// 把扩展目录复制到剪贴板（方便用户粘贴到「加载解压缩的扩展」对话框）
#[cfg(target_os = "windows")]
fn copy_to_clipboard(text: &str) -> bool {
    // 用 clip.exe，避免引入额外依赖
    use std::io::Write;
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "clip"]).stdin(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn copy_to_clipboard(_text: &str) -> bool {
    false
}

// ============== 对外：安装 / 卸载 / 状态 ==============

/// 一键安装：释放文件 → 注册原生宿主 → 写外部扩展注册表 → 拉起浏览器
pub fn install() -> Value {
    // ① 释放扩展文件
    let dir = match extract() {
        Ok(d) => d,
        Err(e) => return json!({ "ok": false, "error": e }),
    };

    // ② 注册原生宿主（扩展要靠它跟启动器通信，缺了等于白装）
    let host = register::install(None);
    if !host.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return json!({
            "ok": false,
            "error": host.get("error").and_then(|v| v.as_str()).unwrap_or("原生宿主注册失败"),
            "dir": dir.to_string_lossy(),
        });
    }

    // ③ 写外部扩展注册表（Edge 下次启动会提示「已添加扩展」）
    let reg_count = write_extension_registry(&dir);

    // ④ 处理浏览器。这里按「能否自动加载」分三种情况给出不同提示，
    //    绝不谎报成功——装没装上，用户必须能一眼看出来。
    let browsers = find_browsers();
    let dir_str = dir.to_string_lossy().to_string();
    let mut launched: Vec<String> = Vec::new();
    let mut auto_loaded = false;
    let mut running: Vec<String> = Vec::new();

    for b in &browsers {
        if is_browser_running(b) {
            running.push(b.name.to_string());
        }
        if launch_with_extension(b, &dir) {
            launched.push(b.name.to_string());
            auto_loaded = true;
        }
    }

    // 没自动加载成功：至少把扩展页打开，并复制目录方便粘贴
    let mut copied = false;
    if !auto_loaded
        && let Some(b) = browsers.first() {
            if open_extensions_page(b) {
                launched.push(format!("{}（扩展页）", b.name));
            }
            copied = copy_to_clipboard(&dir_str);
        }

    // 提示文案要跟实际结果严格对应
    let (hint, need_manual) = if auto_loaded {
        (
            "浏览器已带扩展启动。若扩展列表里没出现，重启一次浏览器即可。".to_string(),
            false,
        )
    } else if !running.is_empty() {
        (
            format!(
                "{} 正在运行，无法自动加载扩展。已为你打开扩展页{}，请点『加载解压缩的扩展』并选择扩展目录（重启浏览器后可自动生效）。",
                running.join("、"),
                if copied { "，扩展目录已复制到剪贴板" } else { "" }
            ),
            true,
        )
    } else {
        (
            format!(
                "已打开扩展页{}。请开启「开发人员模式」→『加载解压缩的扩展』→ 选择扩展目录。",
                if copied { "，扩展目录已复制到剪贴板" } else { "" }
            ),
            true,
        )
    };

    json!({
        "ok": true,
        "dir": dir_str,
        "extensionId": EXTENSION_ID,
        "hostManifest": host.get("manifest").cloned().unwrap_or(Value::Null),
        "extensionRegistryCount": reg_count,
        "launched": launched,
        "autoLoaded": auto_loaded,
        "needManual": need_manual,
        "runningBrowsers": running,
        "hint": hint,
    })
}

/// 启动时调用：只做「已安装用户」的维护，绝不主动给新用户装东西。
///
/// 判据：扩展目录里已有 manifest.json 才算「用户装过」——
/// 新用户第一次启动不会凭空多出一个 extension 目录。
pub fn prepare_for_launch() -> Result<(), String> {
    let dir = install_dir();
    if !dir.join("manifest.json").exists() {
        // 首次启动：不释放文件，只做宿主路径自愈（如果之前注册过）
        super::register::self_heal();
        return Ok(());
    }
    // 已安装过：刷新文件（升级后扩展内容变了要同步）+ 宿主自愈
    extract()?;
    super::register::self_heal();
    Ok(())
}

/// 只释放文件 + 注册宿主 + 写外部扩展注册表（不起浏览器）
pub fn prepare() -> Value {
    let dir = match extract() {
        Ok(d) => d,
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    let host = register::install(None);
    let reg_count = write_extension_registry(&dir);
    json!({
        "ok": host.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        "dir": dir.to_string_lossy(),
        "extensionId": EXTENSION_ID,
        "extensionRegistryCount": reg_count,
        "host": host,
    })
}

/// 卸载：删注册表 + 删文件
pub fn uninstall() -> Value {
    let reg = remove_extension_registry();
    let files = remove_files();
    // 一并清掉释放到 <data>/ 的安装说明
    let dir = install_dir();
    if let Some(parent) = dir.parent() {
        let _ = std::fs::remove_file(parent.join("安装说明-浏览器扩展.txt"));
    }
    let host = register::uninstall();
    json!({
        "ok": true,
        "removedRegistry": reg,
        "removedFiles": files,
        "host": host,
    })
}

/// 状态
pub fn status() -> Value {
    let dir = install_dir();
    let extracted = dir.join("manifest.json").exists();
    let host = register::status();
    json!({
        "extensionId": EXTENSION_ID,
        "version": EXTENSION_VERSION,
        "dir": dir.to_string_lossy(),
        "extracted": extracted,
        "registryCount": count_extension_registry(),
        "fileCount": EMBEDDED.len(),
        "hostInstalled": host.get("installed").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

/// 在资源管理器里打开扩展目录（手动加载时要用）
pub fn open_dir() -> bool {
    let dir = install_dir();
    if !dir.exists() {
        return false;
    }
    open::that(&dir).is_ok()
}

// ============== Tauri 命令 ==============

#[tauri::command]
pub fn bridge_install_extension() -> Value {
    install()
}

#[tauri::command]
pub fn bridge_prepare_extension() -> Value {
    prepare()
}

#[tauri::command]
pub fn bridge_uninstall_extension() -> Value {
    uninstall()
}

#[tauri::command]
pub fn bridge_extension_status() -> Value {
    status()
}

#[tauri::command]
pub fn bridge_open_extension_dir() -> bool {
    open_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 极简 base64 解码（仅测试用，避免为测试引入依赖分支）
    fn base64_decode(input: &str) -> Option<Vec<u8>> {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0u32;
        for c in input.bytes() {
            if c == b'=' || c == b'\n' || c == b'\r' || c == b' ' {
                continue;
            }
            let val = TABLE.iter().position(|&x| x == c)? as u32;
            buf = (buf << 6) | val;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        Some(out)
    }

    #[test]
    fn extension_id_is_32_chars_of_a_to_p() {
        assert_eq!(EXTENSION_ID.len(), 32);
        assert!(EXTENSION_ID.chars().all(|c| ('a'..='p').contains(&c)));
    }

    #[test]
    fn embedded_manifest_is_parseable_and_matches_constants() {
        let idx = EMBEDDED.iter().position(|(n, _)| *n == "manifest.json").unwrap();
        let text = std::str::from_utf8(EMBEDDED[idx].1).expect("manifest 必须是 UTF-8");
        let v: Value = serde_json::from_str(text).expect("manifest 必须是合法 JSON");
        assert_eq!(v.get("manifest_version").and_then(|x| x.as_u64()), Some(3));
        assert_eq!(v.get("version").and_then(|x| x.as_str()), Some(EXTENSION_VERSION));
        // key 必须存在，否则扩展 ID 会随安装环境变化
        assert!(v.get("key").and_then(|x| x.as_str()).map(|s| !s.is_empty()).unwrap_or(false));
        // 原生宿主名必须与后端常量一致
        assert_eq!(
            v.get("permissions").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0),
            5
        );
    }

    #[test]
    fn embedded_files_all_non_empty() {
        for (name, bytes) in EMBEDDED {
            assert!(!bytes.is_empty(), "内嵌文件为空: {name}");
        }
    }

    #[test]
    fn embedded_has_required_entry_files() {
        for required in ["manifest.json", "background.js", "popup.html", "popup.js"] {
            assert!(
                EMBEDDED.iter().any(|(n, _)| *n == required),
                "缺少必需文件: {required}"
            );
        }
    }

    /// 真实安装/卸载的集成验证。默认跳过（会写注册表、释放文件），
    /// 用 `cargo test --lib bridge::extension::tests::install_roundtrip -- --ignored --nocapture` 手动跑。
    /// 真实写盘的集成验证（只测文件释放/清理与宿主注册，不启动浏览器）。
    /// 默认跳过：`cargo test --lib bridge::extension::tests::prepare_roundtrip -- --ignored --nocapture`
    #[test]
    #[ignore = "会写磁盘与注册表，需手动执行"]
    fn prepare_roundtrip() {
        // prepare() 只做「释放文件 + 注册宿主」，不含启动浏览器
        let r = prepare();
        println!("prepare => {}", serde_json::to_string_pretty(&r).unwrap());
        assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true), "prepare 应成功");

        let dir = install_dir();
        assert!(dir.join("manifest.json").exists(), "扩展 manifest 应被释放");
        assert!(dir.join("background.js").exists());
        assert!(dir.join("icons").join("128.png").exists());

        let st = status();
        println!("status => {}", serde_json::to_string_pretty(&st).unwrap());
        assert_eq!(st.get("extracted").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(st.get("hostInstalled").and_then(|v| v.as_bool()), Some(true));

        let u = uninstall();
        println!("uninstall => {}", serde_json::to_string_pretty(&u).unwrap());
        assert!(!dir.join("manifest.json").exists(), "卸载后扩展文件应被清掉");
    }

    #[test]
    fn install_dir_sits_under_data_dir() {
        let d = install_dir();
        assert!(d.to_string_lossy().ends_with("extension"));
    }

    #[test]
    fn readme_is_embedded_and_non_empty() {
        assert!(!README_BYTES.is_empty());
        let text = std::str::from_utf8(README_BYTES).expect("说明文件必须是 UTF-8");
        assert!(text.contains("cc.versepc.download_bridge"));
    }

    #[test]
    fn embedded_paths_are_relative_and_do_not_escape() {
        for (rel, _) in EMBEDDED {
            assert!(!rel.starts_with('/'), "路径不能是绝对路径: {rel}");
            assert!(!rel.contains(".."), "路径不能越出扩展目录: {rel}");
            assert!(Path::new(rel).file_name().is_some(), "路径必须有文件名: {rel}");
        }
    }

    #[test]
    fn manifest_key_derives_the_expected_extension_id() {
        use sha2::Digest as _;
        use sha2::Sha256;
        // 扩展 ID = SHA256(DER 公钥) 前 16 字节，每字节高/低 4 位映射到 a-p
        let idx = EMBEDDED.iter().position(|(n, _)| *n == "manifest.json").unwrap();
        let text = std::str::from_utf8(EMBEDDED[idx].1).unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        let key_b64 = v.get("key").and_then(|x| x.as_str()).expect("manifest 缺少 key");

        let der = base64_decode(key_b64).expect("key 必须是合法 base64");
        let digest = Sha256::digest(&der);
        let mut id = String::new();
        for b in digest.iter().take(16) {
            id.push((b'a' + (b >> 4)) as char);
            id.push((b'a' + (b & 0x0f)) as char);
        }
        assert_eq!(id, EXTENSION_ID, "manifest 的 key 与 EXTENSION_ID 不一致，改了密钥必须同步改常量");
    }
}
