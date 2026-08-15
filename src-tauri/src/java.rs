// java.rs — Java 检测与文件系统命令
// 模块职责：
//   1. 扫描常见目录查找已安装的 Java
//   2. 执行 java -version 解析版本
//   3. 读写 custom-java-list.json（手动添加的 Java）
//   4. 当前 Java 路径存到 settings.json
//
// 与原项目差异：
//   - 简化扫描目录（只扫 Program Files / JAVA_HOME / .minecraft/runtime 等几个常见位置）
//   - 不依赖 worker_thread（Tauri 用 async 命令，不会阻塞主进程）
//   - 不修改系统环境变量（避免权限问题，只存到 settings.json）

use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::download;
use crate::modloaders::shared;
use crate::storage;
use crate::utils;

/// Java 检测结果缓存（一次启动内只扫一次）
/// 后续调用直接返回缓存
static JAVA_CACHE: Mutex<Option<JavaCache>> = Mutex::new(None);

/// 已取消的 Java 下载会话集合
/// install/download 实现后会在下载循环中查询此集合
static CANCELLED_SESSIONS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

#[derive(Clone)]
struct JavaCache {
    java_list: Vec<Value>,
    cached_at: u64,
}

/// 扫描常见 Java 安装路径（Windows 优先）
fn scan_common_java_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut found: std::collections::HashSet<String> = std::collections::HashSet::new();

    /// 添加并去重（路径规范化为小写 + 正斜杠）
    fn add(paths: &mut Vec<PathBuf>, found: &mut std::collections::HashSet<String>, p: PathBuf) {
        if !p.exists() {
            return;
        }
        let key = p.to_string_lossy().to_lowercase().replace('\\', "/");
        if found.insert(key) {
            paths.push(p);
        }
    }

    // 1. JAVA_HOME / JDK_HOME 环境变量
    for var in &["JAVA_HOME", "JDK_HOME"] {
        if let Ok(home) = std::env::var(var) {
            let home = home.trim_matches(|c| c == '"' || c == '\\').to_string();
            if !home.is_empty() {
                let java_exe = PathBuf::from(&home).join("bin").join("java.exe");
                add(&mut paths, &mut found, java_exe);
            }
        }
    }

    // 2. PATH 环境变量逐个目录详细查找（不仅 where java，还会对含 java/jdk 关键字的 PATH 目录检查父级 bin/java）
    #[cfg(target_os = "windows")]
    if let Ok(path_env) = std::env::var("PATH") {
        let sep = if path_env.contains(';') { ';' } else { ':' };
        for dir in path_env.split(sep) {
            let trimmed = dir.trim().trim_matches('"');
            if trimmed.is_empty() {
                continue;
            }
            let dir_path = PathBuf::from(trimmed);
            // 直接查 PATH 目录下的 java.exe
            let direct = dir_path.join("java.exe");
            add(&mut paths, &mut found, direct);
            // 如果 PATH 目录名包含 java/jdk 关键字，很可能是 .../bin，父目录就是 javaHome
            let dir_lower = trimmed.to_lowercase();
            if dir_lower.contains("java") || dir_lower.contains("jdk") {
                if let Some(parent) = dir_path.parent() {
                    let parent_java = PathBuf::from(parent).join("bin").join("java.exe");
                    add(&mut paths, &mut found, parent_java);
                }
            }
        }
    }

    // 3. Program Files 下的常见 Java 目录
    if let Ok(pf) = std::env::var("ProgramFiles") {
        scan_java_subdirs(&pf, &mut paths, &mut found, add);
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        scan_java_subdirs(&pf86, &mut paths, &mut found, add);
    }

    // 4. AppData / LocalAppData：广义 2 层深度扫描（含任何 java/jdk 关键字的目录都能命中，不局限于特定启动器）
    if let Ok(appdata) = std::env::var("APPDATA") {
        scan_java_subdirs_depth(&PathBuf::from(&appdata), 2, &mut paths, &mut found, add);
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        scan_java_subdirs_depth(&PathBuf::from(&localappdata), 2, &mut paths, &mut found, add);

        // JetBrains JBR（整个 JetBrains 目录深搜，兼容 Toolbox 与其他安装方式）
        let jetbrains = PathBuf::from(&localappdata).join("JetBrains");
        if jetbrains.exists() {
            scan_java_subdirs_depth(&jetbrains, 3, &mut paths, &mut found, add);
        }
    }

    // 4. .minecraft/runtime（Minecraft 自带）— 用 APPDATA 而非 home_dir
    let appdata = std::env::var("APPDATA").ok();
    let localappdata = std::env::var("LOCALAPPDATA").ok();
    let userprofile = std::env::var("USERPROFILE").ok();

    if let Some(ref appdata) = appdata {
        let mc_runtime = PathBuf::from(appdata).join(".minecraft").join("runtime");
        if mc_runtime.exists() {
            scan_java_subdirs_depth(&mc_runtime, 3, &mut paths, &mut found, add);
        }
    }

    // 5. 用户目录：广义 2 层深度扫描（任何 java/jdk 关键字目录都会命中）
    if let Some(ref home) = userprofile {
        scan_java_subdirs_depth(&PathBuf::from(home), 2, &mut paths, &mut found, add);

        let user_java = PathBuf::from(home).join("Java");
        scan_java_subdirs(&user_java.to_string_lossy(), &mut paths, &mut found, add);
        let user_jdks = PathBuf::from(home).join(".jdks");
        scan_java_subdirs(&user_jdks.to_string_lossy(), &mut paths, &mut found, add);
        // scoop / sdkman
        let scoop = PathBuf::from(home).join("scoop").join("apps").join("openjdk");
        scan_java_subdirs_depth(&scoop, 3, &mut paths, &mut found, add);
        let sdkman = PathBuf::from(home).join(".sdkman").join("candidates").join("java");
        scan_java_subdirs_depth(&sdkman, 3, &mut paths, &mut found, add);
    }
    if let Some(ref appdata) = appdata {
        // HMCL 运行时
        let hmcl_rt = PathBuf::from(appdata).join(".hmcl").join("runtime");
        scan_java_subdirs_depth(&hmcl_rt, 3, &mut paths, &mut found, add);
        // HMCL Java 目录
        let hmcl_java = PathBuf::from(appdata).join(".hmcl").join("java");
        scan_java_subdirs_depth(&hmcl_java, 3, &mut paths, &mut found, add);
        // Prism Launcher 运行时
        let prism_java = PathBuf::from(appdata).join("PrismLauncher").join("java");
        scan_java_subdirs_depth(&prism_java, 3, &mut paths, &mut found, add);
        // Modrinth App 运行时
        let modrinth_java = PathBuf::from(appdata).join("ModrinthApp").join("meta").join("java_versions");
        scan_java_subdirs_depth(&modrinth_java, 3, &mut paths, &mut found, add);
        // ATLauncher 运行时
        let at_launcher = PathBuf::from(appdata).join("ATLauncher").join("runtimes").join("minecraft");
        scan_java_subdirs_depth(&at_launcher, 3, &mut paths, &mut found, add);
        // CurseForge 运行时（AppData 目录）
        let curseforge = PathBuf::from(appdata).join("curseforge").join("minecraft").join("Install").join("runtime");
        scan_java_subdirs_depth(&curseforge, 3, &mut paths, &mut found, add);
    }
    if let Some(ref userprofile) = userprofile {
        // CurseForge 运行时（用户目录）
        let curseforge_user = PathBuf::from(userprofile).join("curseforge").join("minecraft").join("Install").join("runtime");
        scan_java_subdirs_depth(&curseforge_user, 3, &mut paths, &mut found, add);
    }
    if let Some(ref localappdata) = localappdata {
        // BakaXL 运行时
        let bakaxl = PathBuf::from(localappdata).join("BakaXL").join("JavaRuntime");
        scan_java_subdirs_depth(&bakaxl, 3, &mut paths, &mut found, add);
        // FTB 运行时
        let ftba = PathBuf::from(localappdata).join(".ftba").join("bin").join("runtime");
        scan_java_subdirs_depth(&ftba, 3, &mut paths, &mut found, add);
        // Microsoft Store 版 Minecraft 自带 runtime
        let ms_base = PathBuf::from(localappdata)
            .join("Packages")
            .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
            .join("LocalCache")
            .join("Local")
            .join("runtime");
        scan_java_subdirs_depth(&ms_base, 4, &mut paths, &mut found, add);
        // Microsoft Store 版本额外的特定子路径（gamma/gold 架构子目录）
        let ms_gamma = ms_base.join("java-runtime-gamma").join("windows-x64");
        scan_java_subdirs_depth(&ms_gamma, 3, &mut paths, &mut found, add);
        let ms_gold = ms_base.join("java-runtime-gold").join("windows-x64");
        scan_java_subdirs_depth(&ms_gold, 3, &mut paths, &mut found, add);
        // Programs 目录
        let programs = PathBuf::from(localappdata).join("Programs");
        scan_java_subdirs(&programs.to_string_lossy(), &mut paths, &mut found, add);
    }
    // Program Files 下的 Minecraft Launcher / Minecraft 运行时
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let pf = PathBuf::from(pf);
        scan_java_subdirs_depth(&pf.join("Minecraft Launcher").join("runtime"), 3, &mut paths, &mut found, add);
        scan_java_subdirs_depth(&pf.join("Minecraft").join("runtime"), 3, &mut paths, &mut found, add);
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        let pf86 = PathBuf::from(pf86);
        scan_java_subdirs_depth(&pf86.join("Minecraft Launcher").join("runtime"), 3, &mut paths, &mut found, add);
        scan_java_subdirs_depth(&pf86.join("Minecraft").join("runtime"), 3, &mut paths, &mut found, add);
    }
    // 文档目录下的 CurseForge 运行时
    if let Some(ref home) = userprofile {
        let curse_docs = PathBuf::from(home).join("Documents").join("Curse").join("Minecraft").join("Install").join("runtime");
        scan_java_subdirs_depth(&curse_docs, 3, &mut paths, &mut found, add);
    }

    // 6. 启动器自带的 Java（.versepc/java）
    if let Some(ref home) = userprofile {
        let vrt = PathBuf::from(home).join(".versepc").join("java");
        scan_java_subdirs_depth(&vrt, 3, &mut paths, &mut found, add);
    }
    // 同时扫 jre 子目录（JDK 8 自带 jre）
    // scan_java_subdirs_depth 找到 bin/java.exe 后 continue 不再深入，
    // 这里对每个已找到的 jdk 目录补扫 jre/bin/java.exe
    let extra: Vec<PathBuf> = paths.iter()
        .filter_map(|p| p.parent().and_then(|b| b.parent()).map(|h| h.join("jre").join("bin").join("java.exe")))
        .collect();
    for e in extra {
        if e.exists() {
            add(&mut paths, &mut found, e);
        }
    }

    // 7. 盘符根目录下的常见 Java 目录（用户直接解压到盘根的情况）
    //    先用 wmic 枚举实际存在的盘符（避免漏了 I:、J: 等移动硬盘 / U 盘）
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut drives: Vec<String> = Vec::new();
        if let Ok(out) = Command::new("wmic")
            .args(["logicaldisk", "get", "caption", "/value"])
            .creation_flags(0x08000000)
            .output()
        {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Ok(re) = regex::Regex::new(r"Caption=(\w:)") {
                    for cap in re.captures_iter(&stdout) {
                        if let Some(m) = cap.get(1) {
                            drives.push(m.as_str().to_string());
                        }
                    }
                }
            }
        }
        if drives.is_empty() {
            // wmic 失败时回退到常见盘符
            for d in &["C:", "D:", "E:", "F:", "G:", "H:", "I:", "J:"] {
                drives.push(d.to_string());
            }
        }
        let dir_names = [
            "Java", "JDK", "JRE", "Runtime",
            "Javas", "JDKs", "JREs", "Runtimes",
            "OpenJDK", "jdk", "jre", "runtime",
            ".jdks", ".jre", ".java",
            "Temurin", "Corretto", "Zulu",
            "Microsoft", "GraalVM", "Liberica", "Dragonwell",
        ];
        for drive in &drives {
            // 先读盘根目录，枚举命中 java/jdk/jre/runtime 关键字的目录（与 Electron 版逻辑一致）
            let root = PathBuf::from(format!("{}\\", drive));
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    let hit = ["java", "jdk", "jre", "runtime"]
                        .iter()
                        .any(|k| name == *k || name == format!("{}s", k));
                    if hit {
                        scan_java_subdirs_depth(&entry.path(), 2, &mut paths, &mut found, add);
                    }
                }
            }
            // 同时保留常见目录名精确匹配
            for dir_name in &dir_names {
                let p = PathBuf::from(format!("{}\\{}", drive, dir_name));
                scan_java_subdirs(&p.to_string_lossy(), &mut paths, &mut found, add);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for drive in &["C:", "D:", "E:", "F:", "G:", "H:"] {
            for dir_name in &[
                "Java", "JDK", "JRE", "OpenJDK", "jdk", "jre",
                ".jdks", ".jre", ".java", "Temurin", "Corretto", "Zulu",
                "Microsoft", "GraalVM", "Liberica", "Dragonwell",
            ] {
                let p = PathBuf::from(format!("{}/{}", drive, dir_name));
                scan_java_subdirs(&p.to_string_lossy(), &mut paths, &mut found, add);
            }
        }
    }
    // ProgramData\Oracle\Java
    if let Ok(pd) = std::env::var("ProgramData") {
        let oracle = PathBuf::from(pd).join("Oracle").join("Java");
        scan_java_subdirs(&oracle.to_string_lossy(), &mut paths, &mut found, add);
    } else {
        let oracle = PathBuf::from("C:\\ProgramData\\Oracle\\Java");
        scan_java_subdirs(&oracle.to_string_lossy(), &mut paths, &mut found, add);
    }

    // 8. where java（PATH 里的 java）
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("where");
        c.arg("java");
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("where");
        c.arg("java");
        c
    };
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let p = PathBuf::from(line.trim());
                if p.exists() {
                    add(&mut paths, &mut found, p);
                }
            }
        }
    }

    // 9. Windows 注册表 JavaSoft（用户用安装器装的 Java 会写入注册表，但可能不在 PATH/常见目录）
    //    对应原项目 detectSystemJava 的注册表查询，避免"装了 Java 却检测不到"
    #[cfg(target_os = "windows")]
    {
        for java_exe in scan_registry_java_paths() {
            add(&mut paths, &mut found, java_exe);
        }
    }

    paths
}

/// 扫描 Windows 注册表 JavaSoft 键下的 JavaHome，返回 java.exe 路径
/// 覆盖 32/64 位注册表视图与 JRE/JDK/Java Development Kit 各键
#[cfg(target_os = "windows")]
fn scan_registry_java_paths() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    let mut result: Vec<PathBuf> = Vec::new();
    let keys = [
        r"HKLM\SOFTWARE\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\JavaSoft\JDK",
        r"HKLM\SOFTWARE\JavaSoft\Java Development Kit",
        r"HKLM\SOFTWARE\Wow6432Node\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\Wow6432Node\JavaSoft\JDK",
        r"HKLM\SOFTWARE\Wow6432Node\JavaSoft\Java Development Kit",
        r"HKCU\SOFTWARE\JavaSoft\Java Runtime Environment",
        r"HKCU\SOFTWARE\JavaSoft\JDK",
        r"HKCU\SOFTWARE\JavaSoft\Java Development Kit",
    ];
    for key in keys {
        let mut cmd = Command::new("reg");
        cmd.args(["query", key, "/s"]);
        cmd.creation_flags(0x08000000);
        if let Ok(out) = cmd.output() {
            if !out.status.success() {
                continue;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                let line = line.trim();
                // 形如 "   JavaHome    REG_SZ    C:\Program Files\Java\jdk-17"
                if let Some(home_idx) = line.find("JavaHome") {
                    let after = &line[home_idx..];
                    if let Some(sz_idx) = after.find("REG_SZ") {
                        let home = after[sz_idx + "REG_SZ".len()..]
                            .trim()
                            .trim_matches('"')
                            .trim_end_matches('\\');
                        if !home.is_empty() {
                            let java_exe = PathBuf::from(home).join("bin").join("java.exe");
                            if java_exe.exists() {
                                let norm = java_exe
                                    .to_string_lossy()
                                    .to_lowercase()
                                    .replace('\\', "/");
                                if !result.iter().any(|p| {
                                    p.to_string_lossy().to_lowercase().replace('\\', "/") == norm
                                }) {
                                    result.push(java_exe);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

/// 在 dir 下扫描一层包含 java/jdk/jre 关键字的子目录，找到 java.exe
fn scan_java_subdirs(
    dir: &str,
    paths: &mut Vec<PathBuf>,
    found: &mut std::collections::HashSet<String>,
    add: fn(&mut Vec<PathBuf>, &mut std::collections::HashSet<String>, PathBuf),
) {
    let dir_path = PathBuf::from(dir);
    if !dir_path.exists() {
        return;
    }
    // 关键点：如果 dir 本身就是 Java Home（C:\Java\bin\java.exe 存在），直接匹配，
    // 这样用户把 JDK 解压成单层 C:\Java 的情况也能识别（对齐初代启动器）
    let self_java = dir_path.join("bin").join("java.exe");
    if self_java.exists() {
        add(paths, found, self_java);
    }
    let keywords = [
        "java", "jdk", "jre", "adopt", "temurin", "corretto", "zulu", "amazon",
        "microsoft", "sapmachine", "bellsoft", "graalvm", "dragonwell", "openjdk",
        "liberica", "jbr", "jetbrains",
    ];

    let entries = match std::fs::read_dir(&dir_path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_lowercase(),
            None => continue,
        };
        if !keywords.iter().any(|k| name.contains(k)) {
            continue;
        }
        // 找到 java/jdk 目录，深入一层找 bin/java.exe
        let java_exe = path.join("bin").join("java.exe");
        if java_exe.exists() {
            add(paths, found, java_exe);
            continue;
        }
        // 再深入一层
        if let Ok(sub_entries) = std::fs::read_dir(&path) {
            for sub in sub_entries.flatten() {
                let sub_java = sub.path().join("bin").join("java.exe");
                if sub_java.exists() {
                    add(paths, found, sub_java);
                }
            }
        }
    }
}

/// 深度扫描
fn scan_java_subdirs_depth(
    dir: &Path,
    depth: u32,
    paths: &mut Vec<PathBuf>,
    found: &mut std::collections::HashSet<String>,
    add: fn(&mut Vec<PathBuf>, &mut std::collections::HashSet<String>, PathBuf),
) {
    if depth == 0 || !dir.exists() {
        return;
    }
    // 先检查 dir 本身是不是 Java Home（用户直接解压到盘根目录的单层 Java Home）
    let self_java = dir.join("bin").join("java.exe");
    if self_java.exists() {
        add(paths, found, self_java);
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let java_exe = path.join("bin").join("java.exe");
        if java_exe.exists() {
            add(paths, found, java_exe);
            continue;
        }
        scan_java_subdirs_depth(&path, depth - 1, paths, found, add);
    }
}

/// 读取 <javaHome>/release 文件中的 JAVA_VERSION 与 OS_ARCH
/// 比执行 java -version 快得多；OS_ARCH 用于判断 64 位
fn read_release_version(java_home: &Path) -> Option<(String, u32, bool)> {
    let release_file = java_home.join("release");
    let content = std::fs::read_to_string(&release_file).ok()?;
    let mut version_str: Option<String> = None;
    let mut is64 = true; // 默认 64 位，仅当 OS_ARCH 明确为 32 位时才为 false
    for line in content.lines() {
        if line.starts_with("JAVA_VERSION=") {
            let v = line.trim_start_matches("JAVA_VERSION=").trim_matches('"');
            version_str = Some(v.to_string());
        } else if line.starts_with("OS_ARCH=") {
            let arch = line.trim_start_matches("OS_ARCH=").trim_matches('"').to_lowercase();
            // x86_64 / amd64 / aarch64 / arm64 均为 64 位
            is64 = arch.contains("64") || arch.contains("arm") || arch.contains("aarch");
        }
    }
    let version = version_str?;
    let major = parse_version_major(&version);
    Some((version, major, is64))
}

/// 解析版本字符串的主版本号："1.8.0_301" → 8，"17.0.1" → 17
fn parse_version_major(version: &str) -> u32 {
    if let Some(first) = version.split('.').next() {
        if first == "1" {
            // 旧式版本号 1.8.0_301 → 8
            version.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(8)
        } else {
            first.parse().unwrap_or(0)
        }
    } else {
        0
    }
}

/// 从 java 输出中提取版本号并规范化为 4 段："17.0.1" → (17, 0, 1, 0)
/// 规则：
///   - 取 version "..." 引号内内容（或 openjdk N 前缀）
///   - 把 _ 与 + 替换为 .
///   - 截断到 - 之前（如 "1.8.0_301-b09"）
///   - 去除开头的 1.（旧式版本号）
///   - 补齐 4 段
fn normalize_version_string(raw: &str) -> Option<String> {
    let mut v = raw.replace('_', ".").replace('+', ".");
    if let Some(idx) = v.find('-') {
        v.truncate(idx);
    }
    if v.starts_with("1.") {
        v = v[2..].to_string();
    }
    let mut segs: Vec<&str> = v.split('.').collect();
    if segs.is_empty() || segs[0].is_empty() {
        return None;
    }
    while segs.len() < 4 {
        segs.push("0");
    }
    let major = segs[0].parse::<u32>().ok()?;
    // 主版本 4 及以下或 100 及以上视为解析异常
    if major <= 4 || major >= 100 {
        return None;
    }
    Some(segs[..4].join("."))
}

/// 执行 java -version 解析版本和架构
/// 参照 PCL 的 CheckAsync 逻辑：优先运行 -XshowSettings:properties -version 获取完整信息，
/// 包括版本号、64 位检测、file.encoding 等；失败时回退到 -version 仅解析版本号
pub fn inspect_java(java_exe: &Path) -> Option<(String, u32, bool)> {
    if !java_exe.exists() {
        return None;
    }
    // 先试读 release 文件（不执行 java，速度快）
    if let Some(java_home) = java_exe.parent().and_then(|p| p.parent()) {
        if let Some((v, m, is64)) = read_release_version(java_home) {
            return Some((v, m, is64));
        }
    }

    // 尝试运行 java -XshowSettings:properties -version 获取完整信息
    let mut cmd = Command::new(java_exe);
    cmd.args(["-XshowSettings:properties", "-version"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if let Ok(out) = cmd.output() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let combined = format!("{}\n{}", stderr, stdout);

        // 检查是否包含致命错误（参照 PCL 对此类错误的检测）
        if combined.contains("/lib/ext exists") {
            return None;
        }
        if combined.contains("a fatal error") || combined.contains("error: ") {
            return None;
        }

        // 提取版本号
        if let Some(version) = extract_version_from_output(&combined) {
            if let Some(normalized) = normalize_version_string(&version) {
                let major = parse_version_major(&normalized);
                // 检查 64 位
                let is64 = combined.contains("os.arch = x86_64")
                    || combined.contains("os.arch = amd64")
                    || combined.contains("64-Bit")
                    || combined.contains("64-bit");
                return Some((normalized, major, is64));
            }
        }
    }

    // 回退：仅运行 java -version 解析版本号
    let mut cmd = Command::new(java_exe);
    cmd.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().ok()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    if let Some(version) = extract_version_from_output(&combined) {
        if let Some(normalized) = normalize_version_string(&version) {
            let major = parse_version_major(&normalized);
            let is64 = combined.contains("64-Bit") || combined.contains("64-bit");
            return Some((normalized, major, is64));
        }
    }
    None
}

/// 从 java -version 输出中提取版本字符串
fn extract_version_from_output(output: &str) -> Option<String> {
    // 匹配 version "1.8.0_301" 或 version "17.0.1"（引号可选）
    let re = regex::Regex::new(r#"version\s+"?([^"\s]+)"?"#).ok()?;
    if let Some(cap) = re.captures(output) {
        return Some(cap[1].to_string());
    }
    // 回退匹配 openjdk <number>
    let re2 = regex::Regex::new(r"(?i)(?<=openjdk )\d+").ok()?;
    if let Some(m) = re2.find(output) {
        return Some(m.as_str().to_string());
    }
    None
}

/// 候选文件夹列表：包含完整 Java 的常见目录，扫描到其中的 Java 会获得更高排序优先级
fn candidate_folders() -> Vec<String> {
    let mut folders: Vec<String> = Vec::new();
    let mut push = |p: Option<String>| {
        if let Some(s) = p {
            if !s.is_empty() {
                folders.push(s);
            }
        }
    };

    let appdata = std::env::var("APPDATA").ok();
    let localappdata = std::env::var("LOCALAPPDATA").ok();
    let userprofile = std::env::var("USERPROFILE").ok();
    let pf = std::env::var("ProgramFiles").ok();
    let pf86 = std::env::var("ProgramFiles(x86)").ok();
    let documents = std::env::var("USERPROFILE").ok().map(|h| format!("{}\\Documents", h));

    push(appdata.as_ref().map(|a| format!("{}\\.minecraft\\runtime\\", a)));
    push(appdata.as_ref().map(|a| format!("{}\\.hmcl\\java\\", a)));
    push(appdata.as_ref().map(|a| format!("{}\\ATLauncher\\runtimes\\minecraft\\", a)));
    push(appdata.as_ref().map(|a| format!("{}\\ModrinthApp\\meta\\java_versions\\", a)));
    push(appdata.as_ref().map(|a| format!("{}\\PrismLauncher\\java\\", a)));
    push(userprofile.as_ref().map(|u| format!("{}\\curseforge\\minecraft\\Install\\runtime\\", u)));
    push(userprofile.as_ref().map(|u| format!("{}\\.jdks\\", u)));
    push(userprofile.as_ref().map(|u| format!("{}\\.sdkman\\candidates\\java\\", u)));
    push(localappdata.as_ref().map(|l| format!("{}\\.ftba\\bin\\runtime\\", l)));
    push(localappdata.as_ref().map(|l| {
        format!(
            "{}\\Packages\\Microsoft.4297127D64EC6_8wekyb3d8bbwe\\LocalCache\\Local\\runtime\\",
            l
        )
    }));
    push(pf86.as_ref().map(|p| format!("{}\\Minecraft Launcher\\runtime\\", p)));
    push(pf86.as_ref().map(|p| format!("{}\\Minecraft\\runtime\\", p)));
    push(documents.as_ref().map(|d| format!("{}\\Curse\\Minecraft\\Install\\runtime\\", d)));
    push(pf.as_ref().map(|p| format!("{}\\Java\\", p)));
    push(pf.as_ref().map(|p| format!("{}\\Eclipse Adoptium\\", p)));
    push(pf.as_ref().map(|p| format!("{}\\Amazon Corretto\\", p)));
    push(pf.as_ref().map(|p| format!("{}\\Zulu\\", p)));

    folders
}

/// 判断 java 可执行文件所在目录是否属于候选文件夹（父目录前缀匹配）
fn is_in_candidate_folder(java_exe: &Path, candidates: &[String]) -> bool {
    let java_folder = java_exe
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/").to_lowercase())
        .unwrap_or_default();
    candidates.iter().any(|f| {
        let f = f.replace('\\', "/").to_lowercase();
        java_folder.starts_with(&f)
    })
}

/// 排序 Java 列表：
/// 1. 候选文件夹中的 Java 优先
/// 2. 其次主版本号接近 21 的 Java 优先
fn sort_java_list(list: &mut Vec<Value>) {
    let candidates = candidate_folders();
    list.sort_by(|a, b| {
        let pa = a.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let pb = b.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let ia = is_in_candidate_folder(Path::new(pa), &candidates);
        let ib = is_in_candidate_folder(Path::new(pb), &candidates);
        if ia != ib {
            return ib.cmp(&ia);
        }
        let ma = a.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let mb = b.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let da = (ma - 21).abs();
        let db = (mb - 21).abs();
        da.cmp(&db)
    });
}

/// 检查路径是否包含重解析点（符号链接 / junction）
/// 对应 PCL 对含重解析点 Java 的筛除；.minecraft\runtime 目录不受此限制
fn has_reparse_point(java_exe: &Path) -> bool {
    let lower = java_exe.to_string_lossy().to_lowercase();
    if lower.contains(".minecraft\\runtime") || lower.contains(".minecraft/runtime") {
        return false;
    }
    // 从 java.exe 所在目录向上逐级检查是否为符号链接
    let mut cur = java_exe.parent().map(|p| p.to_path_buf());
    while let Some(dir) = cur {
        match std::fs::symlink_metadata(&dir) {
            Ok(md) => {
                if md.file_type().is_symlink() {
                    return true;
                }
            }
            Err(_) => return false,
        }
        cur = dir.parent().map(|p| p.to_path_buf());
    }
    false
}

/// 检查路径是否为特殊路径（Java 8 自动更新链接 / system32 等）
/// 对应 PCL 对此类路径的筛除
fn is_special_path(java_exe: &Path) -> bool {
    let lower = java_exe.to_string_lossy().to_lowercase();
    lower.contains("java8path_target_")
        || lower.contains("javapath_target_")
        || lower.contains("javatmp")
        || lower.contains("\\system32\\")
}

/// 执行完整 Java 检测（带缓存）
pub fn detect_all() -> Vec<Value> {
    // 检查缓存（10 分钟内有效）
    {
        let cache = JAVA_CACHE.lock().unwrap();
        if let Some(c) = cache.as_ref() {
            return c.java_list.clone();
        }
    }

    let candidates = scan_common_java_paths();
    println!("[java] 扫描到 {} 个候选路径", candidates.len());
    for c in &candidates {
        println!("[java]   候选: {}", c.display());
    }
    let mut java_list: Vec<Value> = Vec::new();

    for java_exe in candidates {
        // 排除 finalshell / paranoia 等
        let path_str = java_exe.to_string_lossy().to_lowercase();
        if path_str.contains("finalshell") || path_str.contains("paranoia") {
            continue;
        }
        // 排除重解析点与特殊路径
        if has_reparse_point(&java_exe) || is_special_path(&java_exe) {
            continue;
        }

        if let Some((version, major, is64)) = inspect_java(&java_exe) {
            // 只接受 64 位 Java
            if !is64 {
                println!("[java]   跳过 32 位 Java: {}", java_exe.display());
                continue;
            }
            let java_home = java_exe
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            java_list.push(json!({
                "path": java_exe.to_string_lossy(),
                "version": version,
                "majorVersion": major,
                "minorVersion": 0,
                "is64Bit": is64,
                "isJdk": true,
                "source": "system",
                "javaHome": java_home
            }));
        }
    }

    // 加载手动添加的 Java（custom-java-list.json）
    let custom = load_custom_java();
    if let Some(custom_arr) = custom.as_array() {
        for entry in custom_arr {
            // 合并到 java_list
            java_list.push(entry.clone());
        }
    }

    // 排序（候选文件夹优先，其次接近 21 优先）
    sort_java_list(&mut java_list);

    // 写缓存
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut cache = JAVA_CACHE.lock().unwrap();
    *cache = Some(JavaCache {
        java_list: java_list.clone(),
        cached_at: now,
    });

    println!("[java] detect_all 完成，找到 {} 个 Java", java_list.len());
    for j in &java_list {
        println!("[java]   {} v{}", j.get("path").and_then(|v| v.as_str()).unwrap_or("?"), j.get("version").and_then(|v| v.as_str()).unwrap_or("?"));
    }

    java_list
}

/// 根据游戏版本选择最合适的 Java（用于 Forge/NeoForge 安装器等需按版本匹配 Java 的场景）
/// 返回 java.exe 的绝对路径；找不到合适版本返回 None
///
/// 流程：
/// 1. 根据游戏版本计算所需的 Java 主版本范围 [min, max]
/// 2. 收集候选：用户指定 Java + 系统检测到的所有 Java
/// 3. 过滤出主版本在范围内的候选
/// 4. 按"距离需求版本最近、64 位优先"排序，取最优
pub fn select_java_for_version(game_version: &str) -> Option<String> {
    let (min_version, max_version) = crate::launch::get_java_version_range(game_version);

    let mut candidates: Vec<Value> = Vec::new();

    // 1. settings.json 中用户指定的 Java
    let settings = storage::load_settings();
    let current_java_path = utils::get_str(&settings, "javaPath");
    if !current_java_path.is_empty() {
        let p = PathBuf::from(&current_java_path);
        if p.exists() {
            if let Some((version, major, is64)) = inspect_java(&p) {
                candidates.push(json!({
                    "path": p.to_string_lossy(),
                    "version": version,
                    "majorVersion": major,
                    "is64Bit": is64,
                    "source": "user",
                }));
            }
        }
    }

    // 2. 系统检测到的所有 Java
    for j in detect_all() {
        candidates.push(j);
    }

    // 过滤：主版本在 [min, max] 范围内
    let mut suitable: Vec<&Value> = candidates
        .iter()
        .filter(|j| {
            let major = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            major >= min_version && major <= max_version
        })
        .collect();

    if suitable.is_empty() {
        return None;
    }

    // 排序：距离需求版本最近优先，其次 64 位优先
    suitable.sort_by(|a, b| {
        let ma = a.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let mb = b.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let da = (ma - min_version as i64).abs();
        let db = (mb - min_version as i64).abs();
        da.cmp(&db).then_with(|| {
            let ia = a.get("is64Bit").and_then(|v| v.as_bool()).unwrap_or(false);
            let ib = b.get("is64Bit").and_then(|v| v.as_bool()).unwrap_or(false);
            ib.cmp(&ia)
        })
    });

    Some(suitable[0].get("path").and_then(|v| v.as_str()).unwrap_or("").to_string())
}

/// 根据游戏版本收集"可用 Java 候选列表"（最优在前），用于安装器失败时自动换 Java 重试
///
/// 顺序：用户指定 Java 优先，其余按"距离需求版本最近、64 位优先"排序。
/// 返回 java.exe 绝对路径列表；无任何可用候选时返回空列表。
pub fn select_java_candidates_for_version(game_version: &str) -> Vec<String> {
    let (min_version, max_version) = crate::launch::get_java_version_range(game_version);

    let mut candidates: Vec<Value> = Vec::new();

    // 1. settings.json 中用户指定的 Java（优先）
    let settings = storage::load_settings();
    let current_java_path = utils::get_str(&settings, "javaPath");
    if !current_java_path.is_empty() {
        let p = PathBuf::from(&current_java_path);
        if p.exists() {
            if let Some((version, major, is64)) = inspect_java(&p) {
                candidates.push(json!({
                    "path": p.to_string_lossy(),
                    "version": version,
                    "majorVersion": major,
                    "is64Bit": is64,
                    "source": "user",
                }));
            }
        }
    }

    // 2. 系统检测到的所有 Java
    for j in detect_all() {
        candidates.push(j);
    }

    // 过滤：主版本在 [min, max] 范围内（确保安装器能运行）
    let mut suitable: Vec<&Value> = candidates
        .iter()
        .filter(|j| {
            let major = j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            major >= min_version && major <= max_version
        })
        .collect();

    // 排序：距离需求版本最近优先，其次 64 位优先
    suitable.sort_by(|a, b| {
        let ma = a.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let mb = b.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let da = (ma - min_version as i64).abs();
        let db = (mb - min_version as i64).abs();
        da.cmp(&db).then_with(|| {
            let ia = a.get("is64Bit").and_then(|v| v.as_bool()).unwrap_or(false);
            let ib = b.get("is64Bit").and_then(|v| v.as_bool()).unwrap_or(false);
            ib.cmp(&ia)
        })
    });

    // 去重并转成路径列表
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<String> = Vec::new();
    for j in suitable {
        let p = j.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if p.is_empty() {
            continue;
        }
        if seen.insert(p.clone()) {
            result.push(p);
        }
    }
    result
}

/// 读取自定义 Java 列表（custom-java-list.json）
fn load_custom_java() -> Value {
    let path = storage::resolve_data_dir().join("custom-java-list.json");
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(arr) = v.get("entries").and_then(|v| v.as_array()) {
                return Value::Array(arr.clone());
            }
        }
    }
    Value::Array(vec![])
}

/// 保存自定义 Java 列表
fn save_custom_java(entries: &Vec<Value>) -> bool {
    let path = storage::resolve_data_dir().join("custom-java-list.json");
    let data = json!({ "entries": entries });
    if let Ok(s) = serde_json::to_string_pretty(&data) {
        std::fs::write(&path, s).is_ok()
    } else {
        false
    }
}

/// 强制刷新缓存（手动添加 / 设置当前 Java 后调用）
pub fn invalidate_cache() {
    let mut cache = JAVA_CACHE.lock().unwrap();
    *cache = None;
}

/// 标记某个下载会话为已取消
/// install/download 路由实现后会在下载循环中查询此状态并主动中止
pub fn cancel_java_download(session_id: &str) {
    let mut guard = CANCELLED_SESSIONS.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    set.insert(session_id.to_string());
    eprintln!("[java] 已标记会话 {} 为取消状态", session_id);
}

/// 查询某个下载会话是否已被取消（供 install/download 路由使用）
pub fn is_java_download_cancelled(session_id: &str) -> bool {
    let guard = CANCELLED_SESSIONS.lock().unwrap();
    guard
        .as_ref()
        .map(|s| s.contains(session_id))
        .unwrap_or(false)
}

/// 清理已结束会话的取消标记（install/download 完成后调用）
pub fn clear_cancelled_session(session_id: &str) {
    let mut guard = CANCELLED_SESSIONS.lock().unwrap();
    if let Some(set) = guard.as_mut() {
        set.remove(session_id);
    }
}

// ============== Java 下载/安装（简化版） ==============

/// 写入状态文件
fn write_java_status(session_id: &str, status: &Value) {
    let data_dir = storage::resolve_data_dir();
    let status_file = data_dir.join(format!("java-download-{}.json", session_id));
    if let Ok(s) = serde_json::to_string_pretty(status) {
        let _ = std::fs::write(&status_file, s);
    }
}

/// 读取状态文件
fn read_java_status(session_id: &str) -> Option<Value> {
    let data_dir = storage::resolve_data_dir();
    let status_file = data_dir.join(format!("java-download-{}.json", session_id));
    std::fs::read_to_string(&status_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
}

// ============== Java 导入（压缩包/文件夹） ==============
// 对应原项目 server/java/java-custom.js 的 importJavaArchive / importJavaDirectory
// 以及 server/api/routes/java.js 的 /api/java/import 路由。
// 导入成功后将解压/复制后的 Java 写入 custom-java-list.json（source='imported'）。

/// 写入导入状态文件（java-import-{sessionId}.json）
fn write_java_import_status(session_id: &str, status: &Value) {
    let data_dir = storage::resolve_data_dir();
    let status_file = data_dir.join(format!("java-import-{}.json", session_id));
    if let Ok(s) = serde_json::to_string_pretty(status) {
        let _ = std::fs::write(&status_file, s);
    }
}

/// 读取导入状态文件
fn read_java_import_status(session_id: &str) -> Option<Value> {
    let data_dir = storage::resolve_data_dir();
    let status_file = data_dir.join(format!("java-import-{}.json", session_id));
    std::fs::read_to_string(&status_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
}

/// 递归查找目录下第一个 bin/java.exe（导入解压目录中定位 javaHome 用）
fn find_java_exe_recursive(dir: &Path, depth: u32) -> Option<PathBuf> {
    if depth == 0 || !dir.is_dir() {
        return None;
    }
    let direct = dir.join("bin").join("java.exe");
    if direct.exists() {
        return Some(direct);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Some(found) = find_java_exe_recursive(&p, depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// 后台执行 Java 导入任务（压缩包或文件夹），边执行边写 java-import-{sessionId}.json
async fn import_java_async(session_id: &str, import_type: &str, source_path: &str) {
    let report = |status: &str, progress: u32, message: &str| {
        write_java_import_status(
            session_id,
            &json!({
                "status": status,
                "progress": progress,
                "message": message,
                "sessionId": session_id,
            }),
        );
    };

    report("importing", 5, "正在准备导入...");

    // 校验源路径存在
    if source_path.is_empty() {
        report("error", 0, "缺少导入路径");
        return;
    }
    let source = PathBuf::from(source_path);
    if !source.exists() {
        report("error", 0, &format!("文件不存在: {}", source_path));
        return;
    }

    let data_dir = storage::resolve_data_dir();
    let java_dir = data_dir.join("java");
    if let Err(e) = std::fs::create_dir_all(&java_dir) {
        report("error", 0, &format!("创建 Java 目录失败: {}", e));
        return;
    }

    // 导入的 Java 最终落位：java/imported-jdk-{major}-{timestamp}
    let import_root = java_dir.join(format!("_import_{}", chrono::Local::now().timestamp_millis()));
    let mut java_exe: Option<PathBuf> = None;
    let mut found_home: Option<PathBuf> = None;

    if import_type == "archive" {
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext != "zip" {
            report("error", 0, "目前仅支持 .zip 格式的 Java 压缩包");
            return;
        }

        report("importing", 15, "正在解压压缩包...");
        // 解压到临时目录（spawn_blocking 避免阻塞 tokio worker）
        let src = source.clone();
        let tmp = import_root.clone();
        let extract_result = tokio::task::spawn_blocking(move || extract_zip(&src, &tmp)).await;
        match extract_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = std::fs::remove_dir_all(&import_root);
                report("error", 0, &format!("解压失败: {}", e));
                return;
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&import_root);
                report("error", 0, &format!("解压任务异常: {}", e));
                return;
            }
        }

        report("importing", 50, "正在查找 Java 可执行文件...");
        java_exe = find_java_exe_recursive(&import_root, 5);
        if let Some(ref exe) = java_exe {
            found_home = exe.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        }
    } else if import_type == "directory" {
        // 文件夹导入：源目录应是 javaHome（含 bin/java.exe），若用户选了 bin 目录则上移一级
        let java_exe_name = "java.exe";
        let mut src_dir = source.clone();
        if !src_dir.join("bin").join(java_exe_name).exists() {
            let parent_exe = src_dir
                .parent()
                .map(|p| p.join("bin").join(java_exe_name));
            if let Some(pe) = parent_exe {
                if pe.exists() {
                    src_dir = src_dir.parent().unwrap_or(src_dir.as_path()).to_path_buf();
                } else {
                    report("error", 0, &format!("该目录不是有效的 Java 安装目录（缺少 bin/{}）", java_exe_name));
                    return;
                }
            } else {
                report("error", 0, &format!("该目录不是有效的 Java 安装目录（缺少 bin/{}）", java_exe_name));
                return;
            }
        }
        found_home = Some(src_dir.clone());
        java_exe = Some(src_dir.join("bin").join(java_exe_name));
    } else {
        report("error", 0, "type 必须是 archive 或 directory");
        return;
    }

    let java_exe = match java_exe {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_dir_all(&import_root);
            report("error", 0, "未找到有效的 Java（缺少 bin/java.exe）");
            return;
        }
    };

    report("importing", 70, "正在验证 Java...");
    let info = inspect_java(&java_exe);
    let (version, major, is64) = match info {
        Some(v) => v,
        None => {
            let _ = std::fs::remove_dir_all(&import_root);
            report("error", 0, "Java 无法运行或识别失败");
            return;
        }
    };

    let found_home = found_home.unwrap_or_else(|| {
        java_exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| java_exe.clone())
    });

    // 确定最终目录：压缩包导入时需要把找到的 javaHome 移到最终位置；
    // 文件夹导入时把源目录复制到最终位置
    let final_dir_name = format!("imported-jdk-{}-{}", major, chrono::Local::now().timestamp_millis());
    let final_dir = java_dir.join(&final_dir_name);

    if import_type == "archive" {
        report("importing", 80, "正在整理 Java 目录...");
        // 把找到的 javaHome 移到最终目录；跨盘失败时用 copy
        if found_home.as_path() != import_root.as_path() {
            let _ = std::fs::create_dir_all(final_dir.parent().unwrap_or(&java_dir));
            match std::fs::rename(&found_home, &final_dir) {
                Ok(_) => {}
                Err(_) => {
                    let _ = copy_dir_recursive(&found_home, &final_dir);
                }
            }
        } else {
            // javaHome 就是解压根目录
            let _ = std::fs::create_dir_all(final_dir.parent().unwrap_or(&java_dir));
            match std::fs::rename(&import_root, &final_dir) {
                Ok(_) => {}
                Err(_) => {
                    let _ = copy_dir_recursive(&import_root, &final_dir);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&import_root);
    } else {
        report("importing", 80, "正在复制 Java 到启动器目录...");
        let _ = std::fs::create_dir_all(&java_dir);
        if let Err(e) = copy_dir_recursive(&found_home, &final_dir) {
            let _ = std::fs::remove_dir_all(&final_dir);
            report("error", 0, &format!("复制 Java 目录失败: {}", e));
            return;
        }
    }

    // 最终 java.exe 路径
    let final_java_exe = if import_type == "archive" {
        // 压缩包导入后 javaHome 已移到 final_dir，java.exe 在 final_dir/bin/java.exe
        final_dir.join("bin").join("java.exe")
    } else {
        final_dir.join("bin").join("java.exe")
    };

    // 校验最终 java.exe 存在
    if !final_java_exe.exists() {
        let _ = std::fs::remove_dir_all(&final_dir);
        report("error", 0, "导入后未找到 java.exe");
        return;
    }

    report("importing", 90, "正在保存到列表...");
    let entry = json!({
        "path": final_java_exe.to_string_lossy().to_string(),
        "javaHome": final_dir.to_string_lossy().to_string(),
        "source": "imported",
        "addedAt": chrono::Local::now().to_rfc3339(),
        "majorVersion": major,
        "minorVersion": 0,
        "version": version,
        "isJdk": final_dir.join("bin").join("javac.exe").exists(),
        "is64Bit": is64
    });

    // 去重后保存到 custom-java-list.json
    let mut custom = load_custom_java();
    if let Some(arr) = custom.as_array_mut() {
        let norm_new = entry
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase()
            .replace('\\', "/");
        let dup = arr
            .iter()
            .any(|v| utils::get_str(v, "path").to_lowercase().replace('\\', "/") == norm_new);
        if dup {
            let _ = std::fs::remove_dir_all(&final_dir);
            report("error", 0, "该 Java 已在列表中");
            return;
        }
        arr.push(entry.clone());
    } else {
        custom = Value::Array(vec![entry.clone()]);
    }
    save_custom_java(&custom.as_array().cloned().unwrap_or_default());
    invalidate_cache();

    report("completed", 100, &format!("导入成功: Java {}", major));
    eprintln!("[java] 导入 Java {}: {}", major, final_java_exe.display());
}

/// 递归复制目录（文件夹导入 Java 用）
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源目录不存在: {}", src.display()));
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目标目录失败: {}", e))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取源目录失败: {}", e))? {
        let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| format!("复制文件失败: {}", e))?;
        }
    }
    Ok(())
}

/// 解压 ZIP 文件到目标目录（含 ZipSlip 保护）
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开 ZIP 失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 ZIP 失败: {}", e))?;

    std::fs::create_dir_all(dest_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("读取条目失败: {}", e))?;
        let name = entry.name().to_string();

        // ZipSlip 保护：跳过包含 .. 的路径
        if name.split('/').any(|c| c == "..") || name.split('\\').any(|c| c == "..") {
            eprintln!("[java] 跳过可疑路径: {}", name);
            continue;
        }

        let out_path = dest_dir.join(&name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
            }
            let mut out_file =
                std::fs::File::create(&out_path).map_err(|e| format!("创建文件失败: {}", e))?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|e| format!("写入文件失败: {}", e))?;
        }
    }

    Ok(())
}

/// 在解压目录中查找真正的 javaHome
/// 有些 ZIP 解压后 extract_dir 本身就是 javaHome，有些则包含一个子目录
fn find_java_home(extract_dir: &Path) -> PathBuf {
    // extract_dir/bin/java.exe 存在，直接用
    let direct = extract_dir.join("bin").join("java.exe");
    if direct.exists() {
        return extract_dir.to_path_buf();
    }

    // 否则查找子目录
    if let Ok(entries) = std::fs::read_dir(extract_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let java_exe = p.join("bin").join("java.exe");
                if java_exe.exists() {
                    return p;
                }
            }
        }
    }

    extract_dir.to_path_buf()
}

/// 安装 Mojang 官方 Java 运行时组件
/// 成功返回 Ok(())，失败返回 Err（用于回退到 Adoptium 下载）
async fn install_mojang_runtime(
    session_id: &str,
    major_version: u64,
    component: &str,
) -> Result<(), String> {
    // 1. 拉取 Mojang Java 运行时清单（动态平台 key，对齐 electron 的 getPlatformKey）
    let runtime_url = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
    let runtime_list = shared::fetch_json(runtime_url).await?;
    let arch_aarch64 = std::env::consts::ARCH == "aarch64";
    let platform_key = if cfg!(target_os = "windows") {
        if arch_aarch64 { "windows-arm64" } else { "windows-x64" }
    } else if cfg!(target_os = "macos") {
        if arch_aarch64 { "mac-os-arm64" } else { "mac-os" }
    } else {
        "linux"
    };
    let platform_runtimes = runtime_list
        .get(platform_key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("平台 {} 无 Java 运行时", platform_key))?;

    let component_info = platform_runtimes
        .iter()
        .find(|r| shared::jstr(r, "name") == component)
        .ok_or_else(|| format!("组件 {} 不存在", component))?;

    let manifest_url = component_info
        .get("manifest")
        .and_then(|m| m.get("url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "manifest URL 缺失".to_string())?;
    // 镜像替换 manifest URL
    let manifest_url = crate::download::mirror::to_mirror_url(manifest_url)
        .unwrap_or_else(|| manifest_url.to_string());

    // 2. 拉取 manifest
    let manifest = shared::fetch_json(&manifest_url).await?;
    let files = manifest
        .get("files")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "manifest files 缺失".to_string())?;

    // 3. 下载所有文件到 targetDir
    let data_dir = storage::resolve_data_dir();
    let target_dir = data_dir.join("java").join(component);
    std::fs::create_dir_all(&target_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    // 预计算总大小
    let mut total_bytes: u64 = 0;
    for (_, info) in files {
        if let Some(size) = info.pointer("/downloads/raw/size").and_then(|v| v.as_u64()) {
            total_bytes += size;
        }
    }

    let mut downloaded_bytes: u64 = 0;
    let total_files = files.len() as u64;
    let mut done_files: u64 = 0;

    for (file_path, info) in files {
        let dest = target_dir.join(file_path);
        let url = info
            .pointer("/downloads/raw/url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match url {
            Some(u) => {
                let size = info
                    .pointer("/downloads/raw/size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let expected_size = if size > 0 { Some(size) } else { None };
                let url = crate::download::mirror::to_mirror_url(&u).unwrap_or(u);
                crate::download::download_with_mirror(
                    &url,
                    &dest,
                    None,
                    expected_size,
                    "china-first",
                    120,
                    None,
                )
                .await?;
                downloaded_bytes += size;
            }
            None => {
                // 无 raw 下载项（目录占位），写空文件
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&dest, "");
            }
        }

        done_files += 1;
        let pct = if total_bytes > 0 {
            (10.0 + (downloaded_bytes as f64 / total_bytes as f64 * 80.0)).min(90.0) as u32
        } else {
            50
        };
        write_java_status(session_id, &json!({
            "sessionId": session_id,
            "status": "downloading",
            "progress": pct,
            "message": format!("正在下载 Java {} 运行时 ({}/{})...", major_version, done_files, total_files),
            "majorVersion": major_version,
        }));
    }

    Ok(())
}

/// 异步安装 Java（后台任务）
/// 完整流程：获取链接 → 下载 → 解压 → 刷新缓存
async fn install_java_async(session_id: String, major_version: u64) {
    let data_dir = storage::resolve_data_dir();
    let java_dir = data_dir.join("java");
    let started_at = utils::now_iso();

    macro_rules! fail {
        ($status:expr, $msg:expr) => {{
            let mut err = $status.clone();
            if let Some(obj) = err.as_object_mut() {
                obj.insert("status".to_string(), json!("error"));
                obj.insert("message".to_string(), json!($msg));
                obj.insert("completedAt".to_string(), json!(utils::now_iso()));
            }
            write_java_status(&session_id, &err);
            clear_cancelled_session(&session_id);
            return;
        }};
    }

    // 步骤 1: fetching
    let mut status = json!({
        "sessionId": session_id,
        "status": "fetching",
        "progress": 0,
        "message": format!("正在获取 Java {} 下载链接...", major_version),
        "majorVersion": major_version,
        "startedAt": started_at,
    });
    write_java_status(&session_id, &status);

    // 检查取消
    if is_java_download_cancelled(&session_id) {
        clear_cancelled_session(&session_id);
        return;
    }

    // 优先使用 Mojang 官方 Java 运行时源（失败时回退到下方 Adoptium 逻辑）
    let mojang_component_map = [
        ("8", "jre-legacy"),
        ("17", "java-runtime-beta"),
        ("21", "java-runtime-delta"),
        ("25", "java-runtime-epsilon"),
    ];
    if let Some((_, component)) = mojang_component_map
        .iter()
        .find(|(v, _)| v.parse::<u64>().ok() == Some(major_version))
    {
        status = json!({
            "sessionId": session_id,
            "status": "fetching",
            "progress": 5,
            "message": "正在获取Mojang官方Java运行时信息...",
            "majorVersion": major_version,
            "startedAt": started_at,
        });
        write_java_status(&session_id, &status);

        match install_mojang_runtime(&session_id, major_version, component).await {
            Ok(()) => {
                let java_home = storage::resolve_data_dir().join("java").join(component);
                let java_path = java_home.join("bin").join("java.exe");
                let completed = json!({
                    "sessionId": session_id,
                    "status": "completed",
                    "progress": 100,
                    "message": format!("Java {} 安装成功！", major_version),
                    "majorVersion": major_version,
                    "javaHome": java_home.to_string_lossy(),
                    "javaPath": java_path.to_string_lossy(),
                    "startedAt": started_at,
                    "completedAt": utils::now_iso(),
                });
                write_java_status(&session_id, &completed);
                invalidate_cache();
                clear_cancelled_session(&session_id);
                return;
            }
            Err(e) => {
                eprintln!("[java] Mojang官方源下载失败，回退到Adoptium: {}", e);
            }
        }
    }

    // 步骤 2: 调用 Adoptium API 获取下载链接（动态平台/架构，对齐 electron 逻辑）
    // 架构映射：x86_64→x64、aarch64→aarch64、其余→x64
    let arch = if std::env::consts::ARCH == "x86_64" {
        "x64"
    } else if std::env::consts::ARCH == "aarch64" {
        "aarch64"
    } else {
        "x64"
    };
    // 平台映射：Windows→windows、macOS→mac、Linux→linux
    let os_name = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let api_url = format!(
        "https://api.adoptium.net/v3/assets/latest/{}/hotspot?architecture={}&image_type=jdk&os={}&vendor=eclipse",
        major_version, arch, os_name
    );

    let api_result = match shared::fetch_json(&api_url).await {
        Ok(v) => v,
        Err(e) => fail!(status, format!("获取下载链接失败: {}", e)),
    };

    let arr = match api_result.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => fail!(status, "Adoptium API 返回空数组".to_string()),
    };

    let first = &arr[0];
    let binary = first.get("binary").cloned().unwrap_or(Value::Null);
    let package = binary.get("package").cloned().unwrap_or(Value::Null);

    let download_url = package
        .get("link")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let file_name_raw = package
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let total_size = package.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    if download_url.is_empty() || file_name_raw.is_empty() {
        fail!(status, "下载链接或文件名为空".to_string());
    }

    // 确保 JAVA_DIR 存在
    if let Err(e) = std::fs::create_dir_all(&java_dir) {
        fail!(status, format!("创建 Java 目录失败: {}", e));
    }

    // 准备路径
    let zip_file_name = if file_name_raw.ends_with(".zip") {
        file_name_raw.clone()
    } else {
        format!("{}.zip", file_name_raw)
    };
    let zip_path = java_dir.join(&zip_file_name);
    let dir_name = zip_file_name.trim_end_matches(".zip").to_string();
    let extract_dir = java_dir.join(&dir_name);

    // 步骤 3: downloading
    status = json!({
        "sessionId": session_id,
        "status": "downloading",
        "progress": 10,
        "message": format!("正在下载 {}...", zip_file_name),
        "majorVersion": major_version,
        "downloadUrl": download_url,
        "fileName": zip_file_name,
        "totalSize": total_size,
        "startedAt": started_at,
    });
    write_java_status(&session_id, &status);

    // 执行下载（china-first：中科大 USTC 镜像优先，Adoptium 官方 GitHub 兜底）
    let expected_size = if total_size > 0 { Some(total_size) } else { None };
    if let Err(e) = download::download_with_mirror(
        &download_url,
        &zip_path,
        None,
        expected_size,
        "china-first",
        600,
        None,
    )
    .await
    {
        fail!(status, format!("下载失败: {}", e));
    }

    // 检查取消
    if is_java_download_cancelled(&session_id) {
        let _ = std::fs::remove_file(&zip_path);
        clear_cancelled_session(&session_id);
        return;
    }

    // 步骤 4: extracting
    status = json!({
        "sessionId": session_id,
        "status": "extracting",
        "progress": 80,
        "message": format!("正在解压 {}...", zip_file_name),
        "majorVersion": major_version,
        "downloadUrl": download_url,
        "fileName": zip_file_name,
        "totalSize": total_size,
        "startedAt": started_at,
    });
    write_java_status(&session_id, &status);

    // 同步解压放到 spawn_blocking，避免阻塞 tokio worker
    let zip_path_clone = zip_path.clone();
    let extract_dir_clone = extract_dir.clone();
    let extract_result =
        tokio::task::spawn_blocking(move || extract_zip(&zip_path_clone, &extract_dir_clone))
            .await;
    match extract_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&zip_path);
            fail!(status, format!("解压失败: {}", e));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&zip_path);
            fail!(status, format!("解压任务异常: {}", e));
        }
    }

    // 步骤 5: 删除 ZIP 文件
    let _ = std::fs::remove_file(&zip_path);

    // 查找 javaHome
    let java_home = find_java_home(&extract_dir);
    let java_path = java_home.join("bin").join("java.exe");

    // 步骤 6: completed
    let completed = json!({
        "sessionId": session_id,
        "status": "completed",
        "progress": 100,
        "message": format!("Java {} 安装完成", major_version),
        "majorVersion": major_version,
        "downloadUrl": download_url,
        "fileName": zip_file_name,
        "totalSize": total_size,
        "javaHome": java_home.to_string_lossy(),
        "javaPath": java_path.to_string_lossy(),
        "startedAt": started_at,
        "completedAt": utils::now_iso(),
    });
    write_java_status(&session_id, &completed);

    // 刷新 Java 检测缓存
    invalidate_cache();
    clear_cancelled_session(&session_id);
}

// ============== Tauri 命令 ==============

/// Tauri 命令：java_detect
/// 兼容原项目 GET /api/java/detect
#[tauri::command]
pub async fn java_detect() -> Value {
    let java_list = tokio::task::spawn_blocking(|| detect_all()).await.unwrap_or_default();

    let has_java = !java_list.is_empty();
    let has_java17 = java_list
        .iter()
        .any(|j| j.get("majorVersion").and_then(|v| v.as_u64()) == Some(17));
    let has_java21 = java_list
        .iter()
        .any(|j| j.get("majorVersion").and_then(|v| v.as_u64()) == Some(21));

    json!({
        "success": true,
        "platform": std::env::consts::OS,
        "javaList": java_list,
        "hasJava": has_java,
        "hasJava17": has_java17,
        "hasJava21": has_java21
    })
}

/// api_proxy 路由处理：Java 相关
pub fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<crate::api::ApiResult> {
    use crate::api::ApiResult;

    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/java/detect" => {
            let java_list = detect_all();
            let has_java = !java_list.is_empty();
            let has_java17 = java_list.iter().any(|j| {
                j.get("majorVersion").and_then(|v| v.as_u64()) == Some(17)
            });
            let has_java21 = java_list.iter().any(|j| {
                j.get("majorVersion").and_then(|v| v.as_u64()) == Some(21)
            });
            Some(ApiResult::ok(json!({
                "success": true,
                "platform": std::env::consts::OS,
                "javaList": java_list,
                "hasJava": has_java,
                "hasJava17": has_java17,
                "hasJava21": has_java21
            })))
        }

        "GET /api/java/installed" => {
            let java_list = detect_all();
            let settings = storage::load_settings();
            let current_java_path = utils::get_str(&settings, "javaPath");
            let total = java_list.len();
            Some(ApiResult::ok(json!({
                "java": java_list,
                "total": total,
                "currentJavaPath": current_java_path,
                "detecting": false
            })))
        }

        "GET /api/java/list" => Some(ApiResult::ok(json!({
            "versions": [
                { "majorVersion": 8, "version": "Java 8", "source": "Mojang 官方" },
                { "majorVersion": 17, "version": "Java 17", "source": "Mojang 官方" },
                { "majorVersion": 21, "version": "Java 21", "source": "Mojang 官方" },
                { "majorVersion": 25, "version": "Java 25", "source": "Mojang 官方" }
            ]
        }))),

        "POST /api/java/set-current" => {
            let data = body.clone().unwrap_or(Value::Null);
            let java_path = utils::get_str(&data, "javaPath");
            if java_path.is_empty() {
                return Some(ApiResult::err(400, "Missing javaPath"));
            }
            let mut settings = storage::load_settings();
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("javaPath".to_string(), json!(java_path));
            }
            storage::overwrite_settings(&settings);
            Some(ApiResult::ok(json!({ "success": true, "message": "已设为当前 Java" })))
        }

        "POST /api/java/add-manual" => {
            let data = body.clone().unwrap_or(Value::Null);
            // 前端 api.js 传入字段为 javaPath（与原项目 /api/java/add-manual 契约一致）
            let path_str = utils::get_str(&data, "javaPath");
            if path_str.is_empty() {
                return Some(ApiResult::err(400, "Missing javaPath"));
            }
            let path = PathBuf::from(&path_str);
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            if !["java.exe", "java", "javaw.exe"].contains(&file_name.as_str()) {
                return Some(ApiResult::err(400, "请选择 java.exe 或 java 可执行文件"));
            }
            if !path.exists() {
                return Some(ApiResult::err(400, "文件不存在"));
            }

            // 探测版本
            let (version, major, is64) = inspect_java(&path).unwrap_or(("未知".to_string(), 0, true));
            let java_home = path
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let entry = json!({
                "path": path_str,
                "javaHome": java_home,
                "source": "manual",
                "addedAt": utils::now_iso(),
                "majorVersion": major,
                "minorVersion": 0,
                "version": version,
                "isJdk": true,
                "is64Bit": is64
            });

            // 加到 custom-java-list.json
            let mut custom = load_custom_java();
            if let Some(arr) = custom.as_array_mut() {
                // 去重
                arr.retain(|v| utils::get_str(v, "path").to_lowercase().replace('\\', "/") != path_str.to_lowercase().replace('\\', "/"));
                arr.push(entry.clone());
            } else {
                custom = Value::Array(vec![entry.clone()]);
            }
            save_custom_java(&custom.as_array().cloned().unwrap_or_default());
            invalidate_cache();

            Some(ApiResult::ok(json!({
                "success": true,
                "message": format!("已添加 Java {}", major),
                "entry": entry
            })))
        }

        "POST /api/java/remove-custom" => {
            let data = body.clone().unwrap_or(Value::Null);
            // 前端 api.js 传入字段为 javaHome（与原项目 /api/java/remove-custom 契约一致）
            let java_home_str = utils::get_str(&data, "javaHome");
            let delete_files = data.get("deleteFiles").and_then(|v| v.as_bool()).unwrap_or(false);
            if java_home_str.is_empty() {
                return Some(ApiResult::err(400, "Missing javaHome"));
            }
            let mut custom = load_custom_java();
            let removed_entry: Option<Value> = if let Some(arr) = custom.as_array_mut() {
                let idx = arr.iter().position(|v| {
                    utils::get_str(v, "javaHome").to_lowercase().replace('\\', "/")
                        == java_home_str.to_lowercase().replace('\\', "/")
                });
                idx.map(|i| arr.remove(i))
            } else {
                None
            };
            save_custom_java(&custom.as_array().cloned().unwrap_or_default());
            invalidate_cache();

            // 仅 source=imported 且请求删除文件时才删除目录；manual 是原位引用不删
            if delete_files {
                if let Some(entry) = &removed_entry {
                    let source = utils::get_str(entry, "source");
                    if source == "imported" {
                        let java_home = utils::get_str(entry, "javaHome");
                        if !java_home.is_empty() {
                            let _ = std::fs::remove_dir_all(&java_home);
                        }
                    }
                }
            }
            Some(ApiResult::ok(json!({ "success": true })))
        }

        // 下载/安装相关路由暂未迁移
        "GET /api/java/download-sources" => Some(ApiResult::ok(json!({
            "sources": [
                { "id": "adoptium", "name": "Adoptium (Temurin)", "url": "https://adoptium.net" },
                { "id": "amazon", "name": "Amazon Corretto", "url": "https://aws.amazon.com/corretto" },
                { "id": "microsoft", "name": "Microsoft OpenJDK", "url": "https://microsoft.com/openjdk" },
                { "id": "zulu", "name": "Azul Zulu", "url": "https://www.azul.com" }
            ]
        }))),

        "GET /api/java/install-status" | "GET /api/java/download-status" => {
            let session_id = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if session_id.is_empty() {
                return Some(ApiResult::ok(json!({
                    "status": "idle",
                    "progress": 0,
                    "message": "缺少 sessionId 参数"
                })));
            }

            match read_java_status(session_id) {
                Some(st) => Some(ApiResult::ok(st)),
                None => Some(ApiResult::ok(json!({
                    "status": "not_found",
                    "progress": 0,
                    "message": "未找到会话状态"
                }))),
            }
        }

        "GET /api/java/import-status" => {
            let session_id = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if session_id.is_empty() {
                return Some(ApiResult::ok(json!({
                    "status": "idle",
                    "progress": 0,
                    "message": "缺少 sessionId 参数"
                })));
            }

            match read_java_import_status(session_id) {
                Some(st) => Some(ApiResult::ok(st)),
                None => Some(ApiResult::ok(json!({
                    "status": "not_found",
                    "progress": 0,
                    "message": "未找到导入会话状态"
                }))),
            }
        }

        "POST /api/java/import" => {
            // 导入 Java（压缩包或文件夹），与原项目 /api/java/import 契约一致：
            // body: { type: 'archive'|'directory', path: '...' }
            let data = body.clone().unwrap_or(Value::Null);
            let import_type = utils::get_str(&data, "type");
            let source_path = utils::get_str(&data, "path");
            if import_type.is_empty() || source_path.is_empty() {
                return Some(ApiResult::err(400, "缺少 type 或 path 参数"));
            }
            if import_type != "archive" && import_type != "directory" {
                return Some(ApiResult::err(400, "type 必须是 archive 或 directory"));
            }

            let session_id = format!("java-import-{}", chrono::Local::now().timestamp_millis());
            // 初始化导入状态文件
            write_java_import_status(
                &session_id,
                &json!({
                    "status": "starting",
                    "progress": 0,
                    "message": "准备导入...",
                    "sessionId": session_id,
                }),
            );

            // 后台执行导入
            let sid = session_id.clone();
            let itype = import_type.clone();
            let spath = source_path.clone();
            tokio::spawn(async move {
                import_java_async(&sid, &itype, &spath).await;
                // 完成后 60 秒清理状态文件
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let _ = std::fs::remove_file(
                    storage::resolve_data_dir().join(format!("java-import-{}.json", sid)),
                );
            });

            Some(ApiResult::ok(json!({
                "success": true,
                "sessionId": session_id,
                "status": "starting",
                "message": "已开始导入 Java"
            })))
        }

        // 自动安装：先检测本地已有 Java，满足要求直接返回；否则启动下载（对应原项目 autoInstallJava）
        "POST /api/java/auto-install" => {
            let data = body.clone().unwrap_or(Value::Null);
            let required_version = data
                .get("requiredVersion")
                .and_then(|v| v.as_u64())
                .or_else(|| data.get("majorVersion").and_then(|v| v.as_u64()))
                .unwrap_or(17);

            // 1. 先扫描本地已有 Java，满足要求直接返回，不重复下载
            let java_list = detect_all();
            if let Some(j) = java_list
                .iter()
                .find(|j| j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) >= required_version)
            {
                return Some(ApiResult::ok(json!({
                    "success": true,
                    "installed": false,
                    "javaPath": utils::get_str(j, "path"),
                    "version": utils::get_str(j, "version"),
                    "majorVersion": j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(required_version),
                    "message": format!("已检测到合适的 Java {}，无需安装", j.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(required_version))
                })));
            }

            // 2. 没有合适的 Java：启动异步下载安装
            let session_id = uuid::Uuid::new_v4().to_string();
            let sid = session_id.clone();
            tokio::spawn(async move {
                install_java_async(sid, required_version).await;
            });

            Some(ApiResult::ok(json!({
                "success": true,
                "installed": false,
                "sessionId": session_id,
                "status": "fetching",
                "message": format!("正在自动下载 Java {}...", required_version)
            })))
        }

        "POST /api/java/install" | "POST /api/java/download" => {
            let data = body.clone().unwrap_or(Value::Null);
            let major_version = data
                .get("majorVersion")
                .and_then(|v| v.as_u64())
                .or_else(|| data.get("major_version").and_then(|v| v.as_u64()))
                .unwrap_or(17);

            let session_id = data
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            // 启动后台异步任务
            let sid = session_id.clone();
            tokio::spawn(async move {
                install_java_async(sid, major_version).await;
            });

            Some(ApiResult::ok(json!({
                "success": true,
                "sessionId": session_id,
                "status": "fetching",
                "message": format!("已开始安装 Java {}", major_version)
            })))
        }

        "POST /api/java/configure-env" => {
            // 简化实现：不修改系统环境变量，仅记录到 settings.json
            // 避免系统权限问题，与原项目行为一致
            let data = body.clone().unwrap_or(Value::Null);
            let java_path = crate::utils::get_str(&data, "javaPath").to_string();
            if !java_path.is_empty() {
                let mut settings = storage::load_settings();
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("javaPath".to_string(), Value::String(java_path.clone()));
                    let _ = storage::save_settings(&settings);
                }
                invalidate_cache();
            }
            Some(ApiResult::ok(json!({
                "success": true,
                "message": "已保存到 settings.json（未修改系统环境变量）"
            })))
        }

        "POST /api/java/delete" => {
            let data = body.clone().unwrap_or(Value::Null);
            let java_path = crate::utils::get_str(&data, "javaPath").to_string();
            if java_path.is_empty() {
                return Some(ApiResult::err(400, "缺少 javaPath 参数"));
            }
            // 不允许删除正在使用的 Java
            let settings = storage::load_settings();
            let current = crate::utils::get_str(&settings, "javaPath");
            if current == java_path {
                return Some(ApiResult::ok(json!({
                    "success": false,
                    "error": "该 Java 正在使用，请先切换到其他 Java"
                })));
            }
            // 删除 javaHome 整个目录
            let java_home = Path::new(&java_path)
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf());
            if let Some(home) = java_home {
                if home.exists() {
                    match std::fs::remove_dir_all(&home) {
                        Ok(()) => {
                            invalidate_cache();
                            Some(ApiResult::ok(json!({
                                "success": true,
                                "message": format!("已删除: {}", home.display())
                            })))
                        }
                        Err(e) => Some(ApiResult::ok(json!({
                            "success": false,
                            "error": format!("删除失败: {}", e)
                        }))),
                    }
                } else {
                    Some(ApiResult::err(404, "Java 目录不存在"))
                }
            } else {
                Some(ApiResult::err(400, "无法解析 Java Home 路径"))
            }
        }

        // ===== 取消 Java 下载任务（GET/POST 共用） =====
        // 原项目路由：* /api/java/cancel
        // 实现：设置全局取消标志，让正在下载的 install 任务感知并中止
        // 当前 install/download 路由还是占位，cancel 先返回成功并清理会话
        "GET /api/java/cancel" | "POST /api/java/cancel" => {
            let session_id = params
                .as_ref()
                .and_then(|p| p.get("sessionId"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    body.as_ref()
                        .and_then(|b| b.get("sessionId"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string();

            if session_id.is_empty() {
                return Some(crate::api::ApiResult::err(400, "缺少sessionId参数"));
            }

            // 设置取消标志（如果会话存在）
            cancel_java_download(&session_id);

            // 同步更新状态文件为已取消
            let data_dir = crate::storage::resolve_data_dir();
            let status_file = data_dir.join(format!("java-download-{}.json", session_id));
            if status_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&status_file) {
                    if let Ok(mut st) = serde_json::from_str::<Value>(&content) {
                        let current_status = st
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if current_status != "completed"
                            && current_status != "error"
                            && current_status != "cancelled"
                        {
                            if let Some(obj) = st.as_object_mut() {
                                obj.insert("status".to_string(), json!("cancelled"));
                                obj.insert("message".to_string(), json!("下载已取消"));
                            }
                            let _ = std::fs::write(&status_file, st.to_string());
                        }
                    }
                }
            }

            Some(crate::api::ApiResult::ok(json!({
                "success": true,
                "message": "已取消Java下载"
            })))
        }

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_version_various_formats() {
        // 新式版本号
        assert_eq!(normalize_version_string("17.0.1"), Some("17.0.1.0".to_string()));
        assert_eq!(normalize_version_string("21.0.7"), Some("21.0.7.0".to_string()));
        // 旧式版本号 1.8.0_301 → 8.0.301.0
        assert_eq!(normalize_version_string("1.8.0_301"), Some("8.0.301.0".to_string()));
        // 带构建号 1.8.0_301-b09
        assert_eq!(normalize_version_string("1.8.0_301-b09"), Some("8.0.301.0".to_string()));
        // 带 + 号（+ 替换为 . 后成为第 4 段）
        assert_eq!(normalize_version_string("17.0.1+12"), Some("17.0.1.12".to_string()));
        // 四段版本
        assert_eq!(normalize_version_string("17.0.1.12"), Some("17.0.1.12".to_string()));
        // 无效版本（主版本过低：1.4.0 去 1. 前缀后主版本 4）
        assert_eq!(normalize_version_string("1.4.0"), None);
        // 无效版本（主版本过高）
        assert_eq!(normalize_version_string("100.0.0"), None);
        // 无法解析
        assert_eq!(normalize_version_string(""), None);
    }

    #[test]
    fn parse_version_major_formats() {
        assert_eq!(parse_version_major("1.8.0_301"), 8);
        assert_eq!(parse_version_major("17.0.1"), 17);
        assert_eq!(parse_version_major("21.0.7"), 21);
        assert_eq!(parse_version_major("26.1.2"), 26);
    }

    #[test]
    fn extract_version_from_output_variants() {
        // 标准 java -version 输出
        let out = "java version \"1.8.0_301\"\nJava(TM) SE Runtime Environment (build 1.8.0_301-b09)";
        assert_eq!(extract_version_from_output(out).as_deref(), Some("1.8.0_301"));
        // OpenJDK 输出
        let out2 = "openjdk version \"17.0.1\" 2021-10-19\nOpenJDK Runtime Environment";
        assert_eq!(extract_version_from_output(out2).as_deref(), Some("17.0.1"));
        // 无版本信息
        assert_eq!(extract_version_from_output("nothing here"), None);
    }

    #[test]
    fn special_path_detection() {
        // Java 8 自动更新重定向目录
        assert!(is_special_path(Path::new(r"C:\ProgramData\Oracle\Java\javapath_target_1.8.0\java.exe")));
        assert!(is_special_path(Path::new(r"C:\Windows\System32\java.exe")));
        assert!(is_special_path(Path::new(r"C:\ProgramData\javatmp\bin\java.exe")));
        assert!(!is_special_path(Path::new(r"C:\Program Files\Java\jdk-17\bin\java.exe")));
    }

    #[test]
    fn sort_java_list_prefers_near_21() {
        let mut list = vec![
            json!({"path": r"C:\x\jdk-8\bin\java.exe", "majorVersion": 8}),
            json!({"path": r"C:\x\jdk-25\bin\java.exe", "majorVersion": 25}),
            json!({"path": r"C:\x\jdk-21\bin\java.exe", "majorVersion": 21}),
            json!({"path": r"C:\x\jdk-17\bin\java.exe", "majorVersion": 17}),
        ];
        sort_java_list(&mut list);
        // 排序后第一个应是接近 21 的 Java 21
        let first = list[0].get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0);
        assert_eq!(first, 21);
    }

    /// 验证 add-manual 路由能正确识别并写入自定义 Java 列表
    #[test]
    fn handle_add_manual_route() {
        // 系统真实存在的 Java（路径不存在则跳过）
        let real_java = r"C:\Program Files\Java\jdk-17\bin\java.exe";
        if !Path::new(real_java).exists() {
            return;
        }
        let body = Some(json!({ "javaPath": real_java }));
        let r = handle("POST", "/api/java/add-manual", &None, &body);
        assert!(r.is_some(), "add-manual 应返回结果");
        let res = r.unwrap();
        assert_eq!(res.status, 200, "add-manual 应成功: {}", res.body);
        assert_eq!(res.body["success"], true, "add-manual 返回: {}", res.body);
        // 清理测试写入的条目，避免污染
        let custom = load_custom_java();
        let cleaned: Vec<Value> = custom
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| {
                utils::get_str(e, "path").to_lowercase().replace('\\', "/")
                    != real_java.to_lowercase().replace('\\', "/")
            })
            .collect();
        save_custom_java(&cleaned);
        invalidate_cache();
    }

    /// 验证 directory 导入：把真实 Java Home 复制到启动器目录并写入列表
    #[tokio::test]
    async fn handle_import_directory_route() {
        let real_java = r"C:\Program Files\Java\jdk-17\bin\java.exe";
        if !Path::new(real_java).exists() {
            return;
        }
        let java_home = Path::new(real_java)
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let body = Some(json!({ "type": "directory", "path": java_home }));
        let r = handle("POST", "/api/java/import", &None, &body);
        assert!(r.is_some(), "import 应返回结果");
        let res = r.unwrap();
        assert_eq!(res.status, 200, "import 应成功: {}", res.body);
        let sid = res.body["sessionId"].as_str().unwrap_or("").to_string();
        assert!(!sid.is_empty(), "应返回 sessionId: {}", res.body);

        // 轮询等待后台导入完成（最多 10 秒）
        let mut final_status = String::new();
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let q = handle(
                "GET",
                "/api/java/import-status",
                &Some(json!({ "sessionId": sid })),
                &None,
            );
            if let Some(qr) = q {
                let st = qr.body["status"].as_str().unwrap_or("").to_string();
                if st == "completed" || st == "error" {
                    final_status = qr.body.to_string();
                    break;
                }
            }
        }
        assert!(final_status.contains("\"completed\""), "导入应完成: {}", final_status);
        // 清理测试写入的条目与目录，避免污染
        if let Some(arr) = load_custom_java().as_array().cloned() {
            let mut kept = Vec::new();
            for e in arr {
                if utils::get_str(&e, "source") == "imported"
                    && utils::get_str(&e, "javaHome").contains("imported-jdk-")
                {
                    let home = utils::get_str(&e, "javaHome");
                    if !home.is_empty() {
                        let _ = std::fs::remove_dir_all(&home);
                    }
                    continue;
                }
                kept.push(e);
            }
            save_custom_java(&kept);
            invalidate_cache();
        }
    }

    /// 验证注册表扫描能发现系统安装的 Java（用户环境检测不到 Java 的关键补充）
    #[cfg(target_os = "windows")]
    #[test]
    fn registry_scan_finds_installed_java() {
        let found = scan_registry_java_paths();
        // 本机注册表 HKLM\SOFTWARE\JavaSoft\JDK 指向 jdk-17，应能扫到
        assert!(
            !found.is_empty(),
            "注册表扫描应至少找到一个 Java，实际: {:?}",
            found
        );
        eprintln!("[test] 注册表扫描到 {} 个 Java:", found.len());
        for p in &found {
            eprintln!("[test]   {}", p.display());
        }
    }
}
