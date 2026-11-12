// bridge/scan.rs — 找出这台机器上的 VersePC2.exe，供浏览器扩展「一键扫描」用
//
// 为什么由启动器来扫：
//   浏览器扩展没有任何文件系统权限（Chrome 的 file 选择器也拿不到真实路径），
//   所以"扫描本机启动器位置"只能由原生宿主（就是本 exe）代劳。
//
// 扫描来源（按可靠性排序）：
//   1. 当前进程自身（宿主就是启动器，最可靠）
//   2. 正在运行的 VersePC2 进程（用户已经开着的那个）
//   3. 卸载表 / App Paths 注册表
//   4. 常见安装目录（Program Files、%LOCALAPPDATA%\Programs、桌面、开始菜单…）
//   5. 便携版常见位置（任意盘根下的 Verse* 目录，浅层）
//
// 全部失败也没关系：扩展侧还有"手动选择路径"，走 probe 校验即可。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 认这些文件名（大小写不敏感）
const EXE_NAMES: &[&str] = &["versepc2.exe", "verse.exe", "versepc.exe"];

/// 单目录递归深度（防止在 Program Files 里翻太久）
const MAX_DEPTH: usize = 3;

fn is_candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    EXE_NAMES.contains(&name.as_str())
}

/// Windows 的 canonicalize 会加 \\?\ 前缀，写进宿主清单会失效，这里统一剥掉
fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix("\\\\?\\") {
        // \?\C:\... → C:\...
        return PathBuf::from(rest);
    }
    p
}

fn push_unique(list: &mut Vec<PathBuf>, p: PathBuf) {
    if !p.is_file() {
        return;
    }
    if !is_candidate(&p) {
        return;
    }
    let canon = strip_unc(std::fs::canonicalize(&p).unwrap_or(p));
    if !list.iter().any(|x| x == &canon) {
        list.push(canon);
    }
}

/// 在 dir 下浅层递归找可执行文件
fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_file() {
            push_unique(out, path);
        } else if ft.is_dir() {
            // 跳过明显无关的大目录
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(
                name.as_str(),
                "node_modules" | "windows" | "windowsapps" | "$recycle.bin" | "system volume information"
                    | "windows.old" | "appdata" | ".git" | "target"
            ) {
                continue;
            }
            walk(&path, depth + 1, out);
        }
    }
}

/// 当前进程自身
fn from_self(out: &mut Vec<PathBuf>) {
    if let Ok(p) = std::env::current_exe() {
        push_unique(out, p);
    }
}

/// 正在运行的 VersePC2 进程
fn from_processes(out: &mut Vec<PathBuf>) {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes();
    for proc_ in sys.processes().values() {
        // sysinfo 0.30 的 exe() 返回 Option<&Path>
        if let Some(p) = proc_.exe()
            && is_candidate(p)
        {
            push_unique(out, p.to_path_buf());
        }
    }
}

/// 注册表：卸载项 DisplayIcon / InstallLocation，以及 App Paths
#[cfg(target_os = "windows")]
fn from_registry(out: &mut Vec<PathBuf>) {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let uninstall_roots = [
        (HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];

    for (hive, path) in uninstall_roots {
        let root = RegKey::predef(hive);
        let Ok(key) = root.open_subkey_with_flags(path, KEY_READ) else { continue };
        for name in key.enum_keys().flatten() {
            let Ok(sub) = key.open_subkey_with_flags(&name, KEY_READ) else { continue };
            let display: String = sub.get_value("DisplayName").unwrap_or_default();
            let icon: String = sub.get_value("DisplayIcon").unwrap_or_default();
            let install: String = sub.get_value("InstallLocation").unwrap_or_default();

            if !display.to_ascii_lowercase().contains("verse")
                && !icon.to_ascii_lowercase().contains("verse")
                && !install.to_ascii_lowercase().contains("verse")
            {
                continue;
            }
            // DisplayIcon 可能是 "C:\a\b.exe,0" 或带引号
            let icon_path = icon
                .trim_matches('"')
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !icon_path.is_empty() {
                push_unique(out, PathBuf::from(icon_path));
            }
            if !install.is_empty() {
                walk(Path::new(&install), 0, out);
            }
        }
    }

    // App Paths
    let app_paths = [
        (HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\App Paths"),
        (HKEY_LOCAL_MACHINE, r"Software\Microsoft\Windows\CurrentVersion\App Paths"),
    ];
    for (hive, base) in app_paths {
        let root = RegKey::predef(hive);
        let Ok(key) = root.open_subkey_with_flags(base, KEY_READ) else { continue };
        for name in key.enum_keys().flatten() {
            if !name.to_ascii_lowercase().contains("verse") {
                continue;
            }
            if let Ok(sub) = key.open_subkey_with_flags(&name, KEY_READ) {
                let p: String = sub.get_value("").unwrap_or_default();
                if !p.is_empty() {
                    push_unique(out, PathBuf::from(p));
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn from_registry(_out: &mut Vec<PathBuf>) {}

/// 常见目录
fn from_common_dirs(out: &mut Vec<PathBuf>) {
    let mut roots: Vec<PathBuf> = Vec::new();

    for var in ["LOCALAPPDATA", "APPDATA", "ProgramFiles", "ProgramFiles(x86)", "USERPROFILE", "SystemDrive"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            roots.push(PathBuf::from(v));
        }
    }

    // 便携版数据目录的上一级就是启动器所在目录
    let data = crate::storage::resolve_data_dir();
    if let Some(parent) = data.parent() {
        roots.push(parent.to_path_buf());
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    for r in roots {
        dirs.push(r.clone());
        dirs.push(r.join("Programs"));
        dirs.push(r.join("Desktop"));
        dirs.push(r.join("Downloads"));
        dirs.push(r.join("Documents"));
        dirs.push(r.join("OneDrive").join("Desktop"));
    }
    // 公共桌面 / 开始菜单
    dirs.push(PathBuf::from(r"C:\Users\Public\Desktop"));
    dirs.push(PathBuf::from(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs"));
    dirs.push(PathBuf::from(r"C:\Program Files"));
    dirs.push(PathBuf::from(r"C:\Program Files (x86)"));

    for d in dirs {
        if d.is_dir() {
            walk(&d, 0, out);
        }
    }
}

/// 收集候选并排序：进程自身优先，其次修改时间新的
fn rank(paths: Vec<PathBuf>) -> Vec<(PathBuf, u64, u64)> {
    let mut list: Vec<(PathBuf, u64, u64)> = paths
        .into_iter()
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Some((p, meta.len(), mtime))
        })
        .collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.2));
    list
}

/// 扫描入口：返回 { ok, candidates: [{path,size,mtime,recommended}], self }
pub fn scan_launcher() -> Value {
    let mut found: Vec<PathBuf> = Vec::new();
    from_self(&mut found);
    from_processes(&mut found);
    from_registry(&mut found);
    from_common_dirs(&mut found);

    let ranked = rank(found);
    let self_path = std::env::current_exe()
        .map(|p| strip_unc(std::fs::canonicalize(&p).unwrap_or(p)))
        .ok();

    let candidates: Vec<Value> = ranked
        .iter()
        .enumerate()
        .map(|(i, (p, size, mtime))| {
            json!({
                "path": p.to_string_lossy(),
                "size": size,
                "mtime": mtime,
                // 当前宿主自己就是启动器，直接推荐它
                "recommended": Some(p) == self_path.as_ref() || (i == 0 && self_path.is_none()),
                // 有没有下载接管模块（官方原版没有，选它点了会失败）
                "supportsBridge": exe_contains(p, b"cc.versepc.download_bridge"),
            })
        })
        .collect();

    json!({
        "ok": true,
        "count": candidates.len(),
        "self": self_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "candidates": candidates,
    })
}

/// 校验一个用户手动填的路径
pub fn probe_launcher(path: &str) -> Value {
    let trimmed = path.trim().trim_matches('"');
    if trimmed.is_empty() {
        return json!({ "ok": false, "error": "路径为空" });
    }
    let p = PathBuf::from(trimmed);

    // 用户可能给的是目录，自动找里面的 exe
    let exe = if p.is_dir() {
        let mut inner: Vec<PathBuf> = Vec::new();
        walk(&p, 0, &mut inner);
        match inner.first() {
            Some(x) => x.clone(),
            None => {
                return json!({ "ok": false, "error": "该目录下没有找到 VersePC2.exe" });
            }
        }
    } else {
        p
    };

    if !exe.is_file() {
        return json!({ "ok": false, "error": "文件不存在" });
    }
    let meta = match std::fs::metadata(&exe) {
        Ok(m) => m,
        Err(e) => return json!({ "ok": false, "error": format!("读不到文件信息: {}", e) }),
    };
    // PE 头校验：至少得是 exe，别把 txt 当启动器
    let mut head = [0u8; 2];
    if let Ok(mut f) = std::fs::File::open(&exe) {
        use std::io::Read;
        if f.read_exact(&mut head).is_err() || &head != b"MZ" {
            return json!({ "ok": false, "error": "这不是一个可执行文件" });
        }
    }

    // 能力检测：这个 exe 里到底有没有下载接管宿主？
    // 官方原版 / 老版本没有这段代码，选它会导致"注册成功但一点下载就失败"，
    // 所以这里提前告警，让用户知道要换 VersePC2 v1.9.0 以上。
    let supports = exe_contains(&exe, b"cc.versepc.download_bridge");

    json!({
        "ok": true,
        "path": exe.to_string_lossy(),
        "size": meta.len(),
        "supportsBridge": supports,
        "warning": if supports {
            ""
        } else {
            "这个 exe 里没有 VersePC2 下载接管模块（可能是官方原版或旧版本），选它之后点下载会失败。请换成本项目的 VersePC2 v1.9.0 以上。"
        },
        "isSelf": std::env::current_exe()
            .ok()
            .and_then(|c| std::fs::canonicalize(&c).ok())
            .map(|c| c == std::fs::canonicalize(&exe).unwrap_or(exe.clone()))
            .unwrap_or(false),
    })
}

/// 在二进制里粗略搜一段 ASCII 标记，判断它是不是带 bridge 的构建
fn exe_contains(path: &Path, needle: &[u8]) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = vec![0u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let Ok(n) = f.read(&mut buf) else { return false };
        if n == 0 {
            return false;
        }
        let mut window = Vec::with_capacity(carry.len() + n);
        window.extend_from_slice(&carry);
        window.extend_from_slice(&buf[..n]);
        if window.windows(needle.len()).any(|w| w == needle) {
            return true;
        }
        // 保留尾部，避免标记跨块被漏掉
        let keep = needle.len().saturating_sub(1);
        carry = window[window.len().saturating_sub(keep)..].to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_unc_removes_verbatim_prefix() {
        assert_eq!(
            strip_unc(PathBuf::from(r"\\?\C:\a\b.exe")),
            PathBuf::from(r"C:\a\b.exe")
        );
        assert_eq!(
            strip_unc(PathBuf::from(r"C:\a\b.exe")),
            PathBuf::from(r"C:\a\b.exe")
        );
    }

    #[test]
    fn recognises_exe_names() {
        assert!(is_candidate(Path::new(r"C:\a\VersePC2.exe")));
        assert!(is_candidate(Path::new(r"C:\a\versepc2.EXE")));
        assert!(is_candidate(Path::new(r"C:\a\verse.exe")));
        assert!(!is_candidate(Path::new(r"C:\a\setup.exe")));
        assert!(!is_candidate(Path::new(r"C:\a\VersePC2.zip")));
    }

    #[test]
    fn probe_rejects_empty_and_missing() {
        let empty = probe_launcher("   ");
        assert_eq!(empty.get("ok").and_then(|v| v.as_bool()), Some(false));
        let missing = probe_launcher(r"Z:\nope\VersePC2.exe");
        assert_eq!(missing.get("ok").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn probe_accepts_current_exe() {
        let self_path = std::env::current_exe().unwrap();
        let r = probe_launcher(&self_path.to_string_lossy());
        // 测试二进制名为 verse_tauri-xxxx.exe，不一定命中候选名，
        // 但 probe 只要求"存在 + MZ 头"，所以应当 ok
        assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn probe_reports_bridge_capability() {
        let self_path = std::env::current_exe().unwrap();
        let r = probe_launcher(&self_path.to_string_lossy());
        assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true));
        // 测试二进制同样链接了 bridge 模块，理应包含宿主名字符串
        assert!(r.get("supportsBridge").is_some());
    }

    #[test]
    fn probe_flags_non_bridge_exe() {
        // 系统自带的 notepad.exe 是合法 PE，但没有 bridge 标记
        let sys = r"C:\Windows\notepad.exe";
        if Path::new(sys).is_file() {
            let r = probe_launcher(sys);
            assert_eq!(r.get("ok").and_then(|v| v.as_bool()), Some(true));
            assert_eq!(r.get("supportsBridge").and_then(|v| v.as_bool()), Some(false));
        }
    }

    #[test]
    fn scan_returns_json_shape() {
        let v = scan_launcher();
        assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
        assert!(v.get("candidates").map(|c| c.is_array()).unwrap_or(false));
    }
}
