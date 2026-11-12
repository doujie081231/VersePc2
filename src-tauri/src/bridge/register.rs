// bridge/register.rs — 原生消息宿主注册（免 PowerShell、免管理员）
//
// 浏览器要能启动 VersePC2.exe --native-host，必须存在两样东西：
//   1. 一个 native messaging 清单（JSON，无 BOM）
//   2. 注册表项指向该清单
//
// 清单里的 path 必须是「可执行文件」。本项目不去生成 .cmd，
// 而是直接指向 VersePC2.exe 自身——配合命令行 --native-host 即可工作，
// 这比 PCL 版那套 .cmd → powershell 的链路少两层，也避开了
// 执行策略、GBK 编码、BOM 三个经典坑。
//
// 扩展 ID 由扩展清单里的 "key" 字段固定，因此 allowed_origins 是稳定值。

use serde_json::{json, Value};

/// 原生消息宿主名（扩展里 connectNative 用的名字）
pub const HOST_NAME: &str = "cc.versepc.download_bridge";

/// 默认允许的扩展来源。
///
/// 扩展清单里用 "key" 固定了公钥，扩展 ID 由此唯一确定，不会因重装/卸载而改变：
///   ID = SHA256(DER 公钥) 前 16 字节，每字节高/低 4 位映射到 a-p
/// 当前值由 extension/manifest.json 的 key 推导，改扩展密钥时必须同步改这里。
const DEFAULT_ALLOWED_ORIGINS: &[&str] = &[
    // 当前扩展 ID（由 extension/manifest.json 的 key 推导）
    "chrome-extension://pfmgdfekkaaigjjigfgljcmnebbhlfoa/",
    // 历史 ID：早期 zip 里的 key 不同，装在浏览器里的旧扩展也要能用
    "chrome-extension://bcgdkpfgcbfjjhnahaflogmfkkkdiddi/",
];

/// 扩展 ID 合法形态：32 位、字符限定 a–p（Chromium 的 ID 编码规则）
fn valid_ext_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_lowercase() && b <= b'p')
}

/// 这个 manifest 是不是我们的扩展。
///
/// 不能按 ID 认：扩展装进浏览器的来源不同，ID 就不同——
///   · 启动器释放的未打包版（manifest 里有 key）→ 固定 ID
///   · 商店上架版（key 被移除，由商店签名）→ 商店分配的 ID
///   · 把 zip 直接拖进浏览器 → 由解压目录绝对路径哈希出来的 ID
/// 所以这里按内容认：name 或 homepage_url 里带 VersePC2 就算我们的。
fn is_our_extension(m: &Value) -> bool {
    let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let home = m.get("homepage_url").and_then(|v| v.as_str()).unwrap_or("");
    name.to_ascii_lowercase().contains("versepc2")
        || home.to_ascii_lowercase().contains("versepc2")
}

/// 读一个 manifest.json（容忍 BOM）
fn read_manifest(path: &std::path::Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()
}

/// 各浏览器 User Data 根目录（只取真实存在的）
fn browser_user_data_roots() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::path::PathBuf;
        let Ok(local) = std::env::var("LOCALAPPDATA") else {
            return Vec::new();
        };
        let l = PathBuf::from(local);
        [
            ("Google", "Chrome"),
            ("Google", "Chrome Beta"),
            ("Microsoft", "Edge"),
            ("Microsoft", "Edge Beta"),
            ("Microsoft", "Edge Dev"),
            ("Microsoft", "Edge SxS"),
            ("Chromium", ""),
            ("BraveSoftware", "Brave-Browser"),
        ]
        .iter()
        .map(|(a, b)| {
            let base = if b.is_empty() { l.join(a) } else { l.join(a).join(b) };
            base.join("User Data")
        })
        .filter(|p| p.is_dir())
        .collect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// 扫一个 profile 目录，收集属于本扩展的 ID
fn scan_profile_ids(dir: &std::path::Path, out: &mut Vec<String>) {
    // ① 常规安装：Extensions/<id>/<version>/manifest.json
    if let Ok(entries) = std::fs::read_dir(dir.join("Extensions")) {
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if !valid_ext_id(&id) {
                continue;
            }
            let Ok(vers) = std::fs::read_dir(e.path()) else {
                continue;
            };
            for v in vers.flatten() {
                if let Some(m) = read_manifest(&v.path().join("manifest.json"))
                    && is_our_extension(&m)
                {
                    out.push(id.clone());
                    break;
                }
            }
        }
    }

    // ② 开发者模式「加载解压缩的扩展」：没有 Extensions 目录，只存在于 Preferences
    for name in ["Preferences", "Secure Preferences"] {
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
            continue;
        };
        let Some(settings) = v
            .get("extensions")
            .and_then(|x| x.get("settings"))
            .and_then(|x| x.as_object())
        else {
            continue;
        };
        for (id, s) in settings {
            if !valid_ext_id(id) {
                continue;
            }
            let mut hit = s.get("manifest").map(is_our_extension).unwrap_or(false);
            if !hit
                && let Some(path) = s.get("path").and_then(|x| x.as_str())
                && let Some(m) = read_manifest(&std::path::Path::new(path).join("manifest.json"))
            {
                hit = is_our_extension(&m);
            }
            if hit {
                out.push(id.clone());
            }
        }
    }
}

/// 发现本机浏览器里实际装着的扩展 ID。
///
/// 扩展 ID 取决于安装来源（见 is_our_extension 的注释），编译期无法写死，
/// 只能在运行时扫一遍浏览器数据目录再写进 allowed_origins。
pub fn discover_extension_ids() -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for root in browser_user_data_roots() {
        scan_profile_ids(&root, &mut ids);
        if let Ok(profiles) = std::fs::read_dir(&root) {
            for p in profiles.flatten() {
                if !p.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let name = p.file_name().to_string_lossy().to_string();
                // 跳过不是 profile 的大目录，少读几个没用的文件
                if matches!(name.as_str(), "Extensions" | "ShaderCache" | "GrShaderCache" | "GraphiteDawnCache") {
                    continue;
                }
                scan_profile_ids(&p.path(), &mut ids);
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// 清单写入位置：<data>/bridge/cc.versepc.download_bridge.json
pub fn manifest_path() -> std::path::PathBuf {
    crate::storage::resolve_data_dir()
        .join("bridge")
        .join(format!("{}.json", HOST_NAME))
}

/// 注册表路径（当前用户，无需管理员）
#[cfg(target_os = "windows")]
fn registry_keys() -> Vec<String> {
    [
        r"Software\Google\Chrome\NativeMessagingHosts\",
        r"Software\Google\Chrome Beta\NativeMessagingHosts\",
        r"Software\Google\Chrome Dev\NativeMessagingHosts\",
        r"Software\Chromium\NativeMessagingHosts\",
        r"Software\Microsoft\Edge\NativeMessagingHosts\",
        r"Software\Microsoft\Edge Beta\NativeMessagingHosts\",
        r"Software\Microsoft\Edge Dev\NativeMessagingHosts\",
        r"Software\Microsoft\Edge Canary\NativeMessagingHosts\",
    ]
    .iter()
    .map(|base| format!("{}{}", base, HOST_NAME))
    .collect()
}

/// 生成清单内容
fn build_manifest(allowed_origins: &[String], exe: &str) -> Value {
    json!({
        "name": HOST_NAME,
        "description": "VersePC2 Download Bridge - 把浏览器下载交给 VersePC2",
        "path": exe,
        "type": "stdio",
        "allowed_origins": allowed_origins,
    })
}

/// 当前可执行文件路径
fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 安装/更新宿主注册（当前进程自身作为宿主）。
pub fn install(extra_origin: Option<String>) -> Value {
    let exe = exe_path();
    install_for(&exe, extra_origin)
}

/// 把宿主清单指向指定 exe（用于扩展侧"选择启动器位置"）。
pub fn install_for(exe: &str, extra_origin: Option<String>) -> Value {
    let mut origins: Vec<String> = DEFAULT_ALLOWED_ORIGINS.iter().map(|s| s.to_string()).collect();
    if let Some(o) = extra_origin {
        let o = o.trim().to_string();
        if !o.is_empty() {
            let full = if o.starts_with("chrome-extension://") {
                if o.ends_with('/') { o } else { format!("{}/", o) }
            } else {
                format!("chrome-extension://{}/", o.trim_end_matches('/'))
            };
            if !origins.contains(&full) {
                origins.push(full);
            }
        }
    }

    // 把浏览器里实际装着的扩展 ID 也加进来（商店版 / 拖 zip 版 / 未打包版各不相同）
    for id in discover_extension_ids() {
        let full = format!("chrome-extension://{}/", id);
        if !origins.contains(&full) {
            origins.push(full);
        }
    }

    let manifest = build_manifest(&origins, exe);
    let mpath = manifest_path();

    // 写清单：必须无 BOM，否则浏览器 JSON 解析失败
    if let Some(dir) = mpath.parent()
        && let Err(e) = std::fs::create_dir_all(dir) {
            return json!({ "ok": false, "error": format!("创建目录失败: {}", e) });
        }
    let text = match serde_json::to_string_pretty(&manifest) {
        Ok(t) => t,
        Err(e) => return json!({ "ok": false, "error": format!("序列化清单失败: {}", e) }),
    };
    if let Err(e) = std::fs::write(&mpath, text.as_bytes()) {
        return json!({ "ok": false, "error": format!("写入清单失败: {}", e) });
    }

    let written = write_registry(&mpath);

    json!({
        "ok": true,
        "manifest": mpath.to_string_lossy(),
        "exe": exe,
        "origins": origins,
        "registryCount": written,
    })
}

/// 卸载注册
pub fn uninstall() -> Value {
    let mpath = manifest_path();
    let _ = std::fs::remove_file(&mpath);
    let removed = remove_registry();
    json!({ "ok": true, "removedCount": removed })
}

/// 查询当前注册状态
pub fn status() -> Value {
    let mpath = manifest_path();
    let manifest_ok = mpath.exists();
    let current_exe = exe_path();

    // 清单里的 path 与当前 exe 是否一致（用户挪动文件夹后需要重新注册）
    let mut path_matches = false;
    let mut origins: Vec<String> = Vec::new();
    if manifest_ok
        && let Ok(text) = std::fs::read_to_string(&mpath)
            && let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
                path_matches = v.get("path").and_then(|x| x.as_str()) == Some(current_exe.as_str());
                origins = v
                    .get("allowed_origins")
                    .and_then(|x| x.as_array())
                    .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
            }

    let registered = count_registered(&mpath);

    json!({
        "hostName": HOST_NAME,
        "manifestPath": mpath.to_string_lossy(),
        "manifestExists": manifest_ok,
        "pathMatches": path_matches,
        "origins": origins,
        "registryCount": registered,
        "installed": manifest_ok && path_matches && registered > 0,
        "exe": current_exe,
    })
}

// ============== 注册表操作（按平台拆分，保证非 Windows 构建无未使用告警） ==============

/// 写入注册表，返回成功项数
#[cfg(target_os = "windows")]
fn write_registry(manifest: &std::path::Path) -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let value = manifest.to_string_lossy().to_string();
    let mut written = 0usize;
    for key_path in registry_keys() {
        match hkcu.create_subkey(&key_path) {
            Ok((key, _)) => match key.set_value("", &value) {
                Ok(()) => written += 1,
                Err(e) => eprintln!("[bridge] 写注册表失败 {}: {}", key_path, e),
            },
            Err(e) => eprintln!("[bridge] 创建注册表项失败 {}: {}", key_path, e),
        }
    }
    written
}

#[cfg(not(target_os = "windows"))]
fn write_registry(_manifest: &std::path::Path) -> usize {
    0
}

/// 删除注册表项，返回成功项数
#[cfg(target_os = "windows")]
fn remove_registry() -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut removed = 0usize;
    for key_path in registry_keys() {
        if hkcu.delete_subkey(&key_path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(not(target_os = "windows"))]
fn remove_registry() -> usize {
    0
}

/// 统计指向该清单的注册表项数量
#[cfg(target_os = "windows")]
fn count_registered(manifest: &std::path::Path) -> usize {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let want = manifest.to_string_lossy().to_string();
    let mut count = 0usize;
    for key_path in registry_keys() {
        if let Ok(key) = hkcu.open_subkey(&key_path)
            && let Ok(v) = key.get_value::<String, _>("")
                && v == want {
                    count += 1;
                }
    }
    count
}

#[cfg(not(target_os = "windows"))]
fn count_registered(_manifest: &std::path::Path) -> usize {
    0
}

/// 启动时自检：路径变了（用户挪了文件夹）或浏览器里出现了新的扩展 ID → 自动重写注册
///
/// 第二条是关键：用户把商店版装进浏览器后，它的 ID 和内置版不一样，
/// allowed_origins 里没有就会「拦截失败」。这里每次启动自动补齐。
pub fn self_heal() {
    let mpath = manifest_path();
    if !mpath.exists() {
        return;
    }
    let st = status();
    let path_ok = st
        .get("pathMatches")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut want: Vec<String> = DEFAULT_ALLOWED_ORIGINS.iter().map(|s| s.to_string()).collect();
    for id in discover_extension_ids() {
        let full = format!("chrome-extension://{}/", id);
        if !want.contains(&full) {
            want.push(full);
        }
    }
    want.sort();

    let mut cur: Vec<String> = st
        .get("origins")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    cur.sort();

    if path_ok && cur == want {
        return;
    }
    eprintln!("[bridge] 原生宿主注册需要更新（路径或扩展 ID 变化），自动重建");
    let _ = install(None);
}

// ============== Tauri 命令 ==============

#[tauri::command]
pub fn bridge_install_native_host(extension_id: Option<String>) -> Value {
    install(extension_id)
}

#[tauri::command]
pub fn bridge_uninstall_native_host() -> Value {
    uninstall()
}

#[tauri::command]
pub fn bridge_native_host_status() -> Value {
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name_is_stable() {
        // 扩展里 connectNative 的字符串依赖这个名字，改动会导致既有安装全部失效
        assert_eq!(HOST_NAME, "cc.versepc.download_bridge");
    }

    #[test]
    fn manifest_matches_native_messaging_schema() {
        let origins = vec!["chrome-extension://abc/".to_string()];
        let self_exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let m = build_manifest(&origins, &self_exe);
        assert_eq!(m.get("type").and_then(|v| v.as_str()), Some("stdio"));
        assert_eq!(m.get("name").and_then(|v| v.as_str()), Some(HOST_NAME));
        // path 必须指向存在的可执行文件（测试进程自身）
        let path = m.get("path").and_then(|v| v.as_str()).unwrap_or("");
        assert!(!path.is_empty(), "path 不能为空");
        assert!(std::path::Path::new(path).exists(), "path 必须指向真实文件: {path}");
        let arr = m.get("allowed_origins").and_then(|v| v.as_array()).unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn manifest_path_sits_under_data_dir() {
        let p = manifest_path();
        assert!(p.to_string_lossy().ends_with("download_bridge.json"));
        assert!(p.to_string_lossy().contains("bridge"));
    }

    #[test]
    fn exe_path_is_absolute() {
        let p = exe_path();
        assert!(std::path::Path::new(&p).is_absolute(), "exe 路径必须是绝对路径: {p}");
    }

    #[test]
    fn ext_id_format_is_enforced() {
        assert!(valid_ext_id("pfmgdfekkaaigjjigfgljcmnebbhlfoa"));
        assert!(valid_ext_id("cjfhdnfkoenifpgjkbapgddbolecgelo"));
        assert!(!valid_ext_id("PFMGDFEKKAAIGJJIGFGLJCMNEBBHLFOA"));
        assert!(!valid_ext_id("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
        assert!(!valid_ext_id("abc"));
    }

    #[test]
    fn our_extension_is_recognized_by_content() {
        let store: Value = serde_json::from_str(
            r#"{"name":"VersePC2 下载接管","homepage_url":"https://github.com/doujie081231/VersePc2"}"#,
        )
        .unwrap();
        assert!(is_our_extension(&store));
        let other: Value = serde_json::from_str(
            r#"{"name":"Some Other Extension","homepage_url":"https://example.com"}"#,
        )
        .unwrap();
        assert!(!is_our_extension(&other));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn registry_keys_cover_chrome_and_edge() {
        let keys = registry_keys();
        assert!(keys.iter().any(|k| k.contains("Google\\Chrome\\NativeMessagingHosts")));
        assert!(keys.iter().any(|k| k.contains("Microsoft\\Edge\\NativeMessagingHosts")));
        assert!(keys.iter().all(|k| k.ends_with(HOST_NAME)));
    }
}
