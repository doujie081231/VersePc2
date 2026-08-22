// modloaders/shared.rs — 模组加载器共享工具
// 职责：提供 HTTP 请求工具（fetch_json、fetch_text、fetch_with_racing）
//
// 设计原则：
//   - 所有加载器共用同一个 reqwest::Client（连接池复用）
//   - fetch_with_racing 实现双源竞速：同时请求官方源和镜像源，谁先返回用谁
//   - 失败时返回 Result，由调用方决定降级策略

use serde_json::{json, Value};
use std::time::Duration;

/// 共享 HTTP 客户端（懒加载）
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

pub fn shared_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .user_agent("VersePC/1.0")
            .danger_accept_invalid_certs(true) // 跳过证书验证：部分镜像证书链不完整
            .build()
            .expect("failed to build reqwest client")
    })
}

/// 拉取 JSON
pub async fn fetch_json(url: &str) -> Result<Value, String> {
    let client = shared_client();
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text.chars().take(200).collect::<String>()));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON 解析失败: {}", e))
}

/// 拉取文本
pub async fn fetch_text(url: &str, timeout_secs: u64) -> Result<String, String> {
    let client = shared_client();
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text.chars().take(200).collect::<String>()));
    }

    resp.text()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))
}

/// 双源竞速：并发请求所有候选 URL，谁先成功返回就用谁
///
/// 说明：用 JoinSet 按"完成先后"取结果，而不是按 spawn 顺序等待，
/// 从而保证镜像源先成功时不会被官方源（可能被墙/卡顿）拖住。
///
/// # 参数
/// - `urls`: 候选 URL 列表（至少 2 个）
///
/// # 返回
/// - 第一个成功返回的结果
/// - 全部失败时返回最后一个错误
pub async fn fetch_with_racing(urls: Vec<String>) -> Result<Value, String> {
    if urls.is_empty() {
        return Err("无候选 URL".to_string());
    }
    if urls.len() == 1 {
        return fetch_json(&urls[0]).await;
    }

    let client = shared_client();
    let mut join_set = tokio::task::JoinSet::new();

    for url in urls {
        let c = client.clone();
        join_set.spawn(async move {
            let resp = c
                .get(&url)
                .header("Accept", "application/json")
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| format!("{}: {}", url, e))?;

            let status = resp.status();
            if !status.is_success() {
                return Err(format!("{}: HTTP {}", url, status));
            }

            resp.json::<Value>()
                .await
                .map_err(|e| format!("{}: {}", url, e))
        });
    }

    // 真·竞速：按完成顺序取结果，返回第一个成功项
    let mut last_err = String::new();
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(v)) => return Ok(v),
            Ok(Err(e)) => last_err = e,
            Err(e) => last_err = format!("task panicked: {}", e),
        }
    }
    Err(last_err)
}

/// 文本竞速：并发请求多个 URL，返回第一个成功拉取到非空文本的结果。
/// 用于 Forge 等返回 XML/HTML 的元数据源，避免官方源被墙/卡顿拖住镜像源。
pub async fn fetch_text_racing(urls: &[&str], timeout_secs: u64) -> Option<String> {
    if urls.is_empty() {
        return None;
    }
    if urls.len() == 1 {
        return fetch_text(urls[0], timeout_secs).await.ok();
    }

    let client = shared_client();
    let mut join_set = tokio::task::JoinSet::new();
    for url in urls {
        let c = client.clone();
        let url = (*url).to_string();
        join_set.spawn(async move {
            let resp = c
                .get(&url)
                .timeout(Duration::from_secs(timeout_secs))
                .send()
                .await
                .map_err(|_| ())?;
            let status = resp.status();
            if !status.is_success() {
                return Err(());
            }
            let text = resp.text().await.map_err(|_| ())?;
            if text.trim().is_empty() {
                return Err(());
            }
            Ok(text)
        });
    }

    // 真·竞速：按完成顺序取结果，返回第一个成功项
    while let Some(result) = join_set.join_next().await {
        if let Ok(Ok(text)) = result {
            return Some(text);
        }
    }
    None
}

/// 从 JSON 值中取字符串字段（空值返回空串）
pub fn jstr(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// 从 JSON 值中取布尔字段（空值返回 false）
pub fn jbool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

/// 从 JSON 值中取数组字段（空值返回空数组）
#[allow(dead_code)]
pub fn jarr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

// ============== 安装工具函数 ==============
// 提供路径解析、版本 JSON 读写、并发下载库等功能

use std::path::{Path, PathBuf};

/// 追加一行到加载器安装日志文件（便于排查）
pub fn file_log(s: &str) {
    eprintln!("{}", s);
    use std::io::Write;
    if let Ok(dir) = std::fs::create_dir_all(data_dir().join("logs")) {
        let _ = dir;
    }
    let path = data_dir().join("logs").join("loader-install.log");
    let line = format!(
        "[{}] {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        s
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", line);
    }
}

/// 从安装器 JAR 中提取内嵌的 version.json 并写入版本目录
/// 官方 Forge/NeoForge 安装器的 --installClient 模式只把版本信息写进
/// launcher_profiles.json，不会生成独立的版本目录/JSON，需要从这里解出版本
/// JSON 手动落盘，否则启动器找不到该版本。
///
/// # 参数
/// - `installer_path`: 安装器 JAR 路径
/// - `target_version_id`: 目标版本目录名（写入时同步更新 JSON 的 id 字段）
///
/// # 返回
/// 成功写入返回 Some(版本 JSON)，失败返回 None
pub fn extract_installer_version_json(
    installer_path: &Path,
    target_version_id: &str,
) -> Option<Value> {
    use std::io::Read;

    file_log(&format!("[extract] 开始从安装器提取版本 JSON: {}", installer_path.display()));
    let file = match std::fs::File::open(installer_path) {
        Ok(f) => f,
        Err(e) => {
            file_log(&format!("[extract] 无法打开安装器 {}: {}", installer_path.display(), e));
            return None;
        }
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            file_log(&format!("[extract] 无法解析安装器 ZIP: {}", e));
            return None;
        }
    };
    let mut version_json: Option<Value> = None;

    // 列出安装器内的顶层条目，便于判断版本 JSON 存放位置
    {
        let mut names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let n = entry.name().to_string();
                if n.matches('/').count() <= 1 {
                    names.push(n);
                }
            }
        }
        file_log(&format!("[extract] 安装器顶层条目: {}", names.join(", ")));
    }

    // 1) 优先读 version.json 条目
    if let Ok(mut entry) = archive.by_name("version.json") {
        let mut buf = String::new();
        if entry.read_to_string(&mut buf).is_ok() {
            if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                file_log("[extract] 从 version.json 条目读取成功");
                version_json = Some(v);
            } else {
                file_log("[extract] version.json 条目存在但解析失败");
            }
        }
    }

    // 2) 兜底 A：从 install_profile.json 的 json 字段指向的路径读取
    if version_json.is_none() {
        let mut rel = String::new();
        let mut ip_obj: Option<Value> = None;
        if let Ok(mut ip_entry) = archive.by_name("install_profile.json") {
            let mut ip_str = String::new();
            if ip_entry.read_to_string(&mut ip_str).is_ok() {
                if let Ok(ip) = serde_json::from_str::<Value>(&ip_str) {
                    ip_obj = Some(ip.clone());
                    rel = jstr(&ip, "json").trim_start_matches('/').to_string();
                    file_log(&format!("[extract] install_profile.json 的 json 字段指向: {}", rel));
                }
            }
        }
        if !rel.is_empty() {
            if let Ok(mut entry2) = archive.by_name(&rel) {
                let mut buf = String::new();
                if entry2.read_to_string(&mut buf).is_ok() {
                    if let Ok(v) = serde_json::from_str::<Value>(&buf) {
                        file_log(&format!("[extract] 从 install_profile.json 的 json 路径读取成功: {}", rel));
                        version_json = Some(v);
                    }
                }
            }
        }
        // 兜底 B：新版 Forge/NeoForge 把版本 JSON 直接内嵌在 install_profile.json 的 versionInfo 字段
        if version_json.is_none() {
            if let Some(ip) = ip_obj {
                if let Some(vi) = ip.get("versionInfo") {
                    file_log("[extract] 从 install_profile.json 的 versionInfo 字段读取");
                    version_json = Some(vi.clone());
                }
            }
        }
    }

    if let Some(mut vj) = version_json {
        // 同步 id 字段，确保与版本目录名一致
        if let Some(obj) = vj.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(target_version_id));
        }
        let ok = write_version_json(target_version_id, &vj);
        file_log(&format!(
            "[extract] 写入版本 JSON 到 {} 结果: {}",
            versions_dir().join(target_version_id).join(format!("{}.json", target_version_id)).display(),
            ok
        ));
        if ok {
            return Some(vj);
        }
    } else {
        file_log("[extract] 未找到任何可用的版本 JSON 来源");
    }
    None
}

/// 确保数据目录存在 launcher_profiles.json（官方 Forge/NeoForge 安装器需要检测到启动器配置）
/// 不存在时创建一个指向基础版本的最小配置，否则安装器会报
/// "There is no Minecraft launcher profile" 并中止
pub fn ensure_launcher_profile(game_version: &str) {
    use std::io::Write;
    let path = data_dir().join("launcher_profiles.json");
    if path.exists() {
        return;
    }
    let now = chrono::Local::now().to_rfc3339();
    let profile = serde_json::json!({
        "profiles": {
            game_version: {
                "name": game_version,
                "type": "custom",
                "created": now,
                "lastUsed": now,
                "lastVersionId": game_version,
                "icon": "Furnace",
            }
        },
        "selectedProfile": game_version,
        "clientToken": format!("versepc-{}", std::process::id()),
    });
    if let Ok(content) = serde_json::to_string_pretty(&profile) {
        if let Ok(mut f) = std::fs::File::create(&path) {
            let _ = writeln!(f, "{}", content);
        }
    }
}

/// 把加载器安装器的完整 stdout/stderr 写入 logs 目录下的独立文件
/// 便于在不截断的情况下查看安装器真实的失败原因
pub fn dump_installer_output(name: &str, stdout: &str, stderr: &str) {
    use std::io::Write;
    if let Ok(dir) = std::fs::create_dir_all(data_dir().join("logs")) {
        let _ = dir;
    }
    let path = data_dir().join("logs").join(name);
    let content = format!(
        "===== STDOUT =====\n{}\n===== STDERR =====\n{}\n",
        stdout, stderr
    );
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = f.write_all(content.as_bytes());
    }
}

/// 获取数据目录
pub fn data_dir() -> PathBuf {
    crate::storage::resolve_data_dir()
}

/// 获取 versions 目录
pub fn versions_dir() -> PathBuf {
    data_dir().join("versions")
}

/// 获取 libraries 目录
pub fn libraries_dir() -> PathBuf {
    data_dir().join("libraries")
}

/// 获取 assets 目录
#[allow(dead_code)]
pub fn assets_dir() -> PathBuf {
    data_dir().join("assets")
}

/// 读取版本 JSON
/// 返回 None 表示文件不存在或解析失败
#[allow(dead_code)]
pub fn read_version_json(version_id: &str) -> Option<Value> {
    let path = versions_dir().join(version_id).join(format!("{}.json", version_id));
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<Value>(&content).ok()
}

/// 写入版本 JSON
pub fn write_version_json(version_id: &str, json: &Value) -> bool {
    let dir = versions_dir().join(version_id);
    if !dir.exists() {
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
    }
    let path = dir.join(format!("{}.json", version_id));
    let content = serde_json::to_string_pretty(json).unwrap_or_default();
    std::fs::write(&path, content).is_ok()
}

/// 把安装器生成的版本目录规范化为目标版本目录名（含版本 JSON 改名与 id 更新）。
/// 关键点：Windows 文件系统不区分字母大小写，当目标名与来源名仅大小写不同时
/// （如 "26.2-forge-65.1.0" -> "26.2-Forge-65.1.0"），二者在系统眼里是同一个目录。
/// 此时不能"先删目标再改"，否则会把刚生成的安装结果删掉；必须经临时名中转来改大小写。
pub fn normalize_version_dir(
    source_dir: &Path,
    source_id: &str,
    target_dir: &Path,
    target_id: &str,
) {
    if source_id == target_id {
        return;
    }
    let rename_ok = if source_id.eq_ignore_ascii_case(target_id) {
        // 仅大小写不同：Windows 视为同一目录，用临时名中转改大小写
        rename_dir_case_insensitive(source_dir, target_dir)
    } else {
        // 真正不同的目录：先清理旧目标，再整体改名
        if target_dir.exists() {
            let _ = std::fs::remove_dir_all(target_dir);
        }
        std::fs::rename(source_dir, target_dir).is_ok()
    };
    if rename_ok {
        file_log(&format!(
            "[version] 版本目录规范化: {} -> {}",
            source_dir.display(),
            target_dir.display()
        ));
        let old_json = target_dir.join(format!("{}.json", source_id));
        let new_json = target_dir.join(format!("{}.json", target_id));
        let _ = std::fs::rename(&old_json, &new_json);
        // 更新 JSON 内的 id 字段
        if let Ok(content) = std::fs::read_to_string(&new_json) {
            if let Ok(mut json) = serde_json::from_str::<Value>(&content) {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("id".to_string(), serde_json::json!(target_id));
                }
                let _ = std::fs::write(&new_json, serde_json::to_string_pretty(&json).unwrap_or_default());
            }
        }
    } else {
        file_log(&format!(
            "[version] 版本目录规范化失败: {} -> {}",
            source_dir.display(),
            target_dir.display()
        ));
    }
}

/// 在文件系统不区分大小写（Windows）时安全地改目录大小写。
/// 直接 rename 源->目标会被视为同一个目录而无效果，需先改名到临时名，再改到目标名。
fn rename_dir_case_insensitive(source: &Path, target: &Path) -> bool {
    let temp = source.with_extension(format!("__tmp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    if std::fs::rename(source, &temp).is_err() {
        return false;
    }
    if std::fs::rename(&temp, target).is_ok() {
        return true;
    }
    // 回滚
    let _ = std::fs::rename(&temp, source);
    false
}

/// 确保文件所在目录存在
#[allow(dead_code)]
pub fn ensure_parent_dir(path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).is_ok()
    } else {
        true
    }
}

/// 并发下载多个库文件
///
/// # 参数
/// - `libs`: (url, dest_path) 列表
/// - `parallel`: 并发数
///
/// # 返回
/// (成功数, 失败数)
pub async fn download_libraries_concurrent(
    libs: Vec<(String, PathBuf)>,
    parallel: usize,
) -> (usize, usize) {
    if libs.is_empty() {
        return (0, 0);
    }
    let parallel = parallel.max(1).min(libs.len());
    let mut success = 0usize;
    let mut fail = 0usize;
    let mut tasks = tokio::task::JoinSet::new();

    for (url, dest) in libs {
        // 控制并发数
        while tasks.len() >= parallel {
            match tasks.join_next().await {
                Some(Ok(Ok(()))) => success += 1,
                _ => fail += 1,
            }
        }
        tasks.spawn(async move {
            // 程序内部方式下载；失败时改用系统 curl 兜底（能连上 reqwest 连不上的源）
            if crate::download::single::download_with_mirror(
                &url,
                &dest,
                None,
                None,
                "libraries",
                120,
                None,
            )
            .await
            .is_ok()
            {
                return Ok::<(), String>(());
            }
            download_with_curl(&url, &dest, 300).await
        });
    }

    // 等待剩余任务
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => success += 1,
            _ => fail += 1,
        }
    }

    (success, fail)
}

/// 判断 JAR 文件是否完整（大小 > 1KB）
pub fn is_jar_intact(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    match std::fs::metadata(path) {
        Ok(m) => m.len() > 1024,
        Err(_) => false,
    }
}

// ============== Java 查找 ==============
// 优先使用 settings.javaPath，其次扫描系统已安装的 Java

use crate::storage;

/// 查找可用的 Java 可执行文件路径（用于运行 installer）
///
/// 优先级：
/// 1. settings.json 中的 javaPath
/// 2. 系统扫描的 Java 列表中第一个 major >= 8 的
/// 3. 系统 PATH 中的 java
pub fn find_java_for_install(min_major: u32) -> Option<std::path::PathBuf> {
    // 1. settings.javaPath
    let settings = storage::load_settings();
    let java_path = jstr(&settings, "javaPath");
    if !java_path.is_empty() {
        let p = std::path::PathBuf::from(&java_path);
        if p.exists() {
            if let Some(major) = inspect_java_major(&p) {
                if major >= min_major {
                    return Some(p);
                }
            }
            // 即使版本检测失败也允许使用（兜底）
            return Some(p);
        }
    }

    // 2. 扫描系统 Java
    let java_list = crate::java::detect_all();
    for entry in &java_list {
        let major = entry.get("majorVersion").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let path = jstr(entry, "path");
        if !path.is_empty() && major >= min_major {
            return Some(std::path::PathBuf::from(path));
        }
    }
    // 没有满足版本的，取第一个
    if let Some(first) = java_list.first() {
        let path = jstr(first, "path");
        if !path.is_empty() {
            return Some(std::path::PathBuf::from(path));
        }
    }

    // 3. PATH 中的 java
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let java_exe = std::path::PathBuf::from(dir).join("java.exe");
            if java_exe.exists() {
                return Some(java_exe);
            }
            let java_exe = std::path::PathBuf::from(dir).join("java");
            if java_exe.exists() {
                return Some(java_exe);
            }
        }
    }

    None
}

/// 检测 Java 主版本号（执行 java -version）
fn inspect_java_major(java_path: &Path) -> Option<u32> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(java_path);
        c.arg("-version");
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = std::process::Command::new(java_path);
        c.arg("-version");
        c
    };
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr);
    let text2 = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", text, text2);

    // 匹配 "version \"1.8.0_432\"" 或 "version \"17.0.1\""
    if let Some(idx) = combined.find("version \"") {
        let rest = &combined[idx + 9..];
        if let Some(end) = rest.find('"') {
            let ver_str = &rest[..end];
            let parts: Vec<&str> = ver_str.split('.').collect();
            if parts.len() >= 2 {
                // 旧版本格式 1.8.0_xxx → 主版本 8
                // 新版本格式 17.0.1 → 主版本 17
                let major = if parts[0] == "1" {
                    parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0)
                } else {
                    parts[0].parse().ok().unwrap_or(0)
                };
                return Some(major);
            }
            return parts[0].parse().ok();
        }
    }
    None
}

// ============== 基础版本安装 ==============
// 整合包下载/导入时，若原版基础版本缺失，自动下载安装（版本 JSON + client.jar + 核心库），
// 无需用户先手动安装原版。

/// 基础版本安装进度回调：参数 (百分比, 提示信息)
pub type BaseVersionProgress = Box<dyn Fn(u32, String) + Send + Sync>;

/// 评估库的平台 rules（当前平台是否允许该库）
fn evaluate_rules(lib: &Value) -> bool {
    let rules = match lib.get("rules").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return true, // 无 rules 默认允许
    };
    let os_name = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(os) = rule.get("os").and_then(|v| v.as_object()) {
            let rule_os = os.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if rule_os == os_name {
                allowed = action == "allow";
            }
        } else {
            allowed = action == "allow";
        }
    }
    allowed
}

/// 拉取版本详情 JSON（带镜像回退）
async fn fetch_version_details(url: &str, download_source: &str) -> Result<Value, String> {
    let urls = crate::download::mirror::get_mirror_urls(url, download_source);
    let mut last_err = String::new();
    for u in &urls {
        match fetch_json(u).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// 并发下载基础版本的核心库（复用 download_libraries_concurrent 的镜像+curl 兜底）
/// 返回 (成功数, 失败数)；已存在的库跳过
async fn download_basic_libraries(
    version_details: &Value,
    libraries_dir: &PathBuf,
) -> (usize, usize) {
    let libraries = version_details
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks: Vec<(String, PathBuf)> = Vec::new();
    for lib in libraries {
        if !evaluate_rules(&lib) {
            continue;
        }
        // 优先用 downloads.artifact
        if let Some(artifact) = lib.pointer("/downloads/artifact") {
            let aurl = artifact.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let rel = artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if aurl.is_empty() || rel.is_empty() {
                continue;
            }
            let dest = libraries_dir.join(rel);
            if dest.starts_with(libraries_dir) && !dest.exists() {
                tasks.push((aurl.to_string(), dest));
            }
            continue;
        }
        // 无 artifact，按 maven 坐标构造 URL
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        let group = parts[0];
        let artifact_id = parts[1];
        let version = parts[2];
        let classifier = if parts.len() >= 4 { format!("-{}", parts[3]) } else { String::new() };
        let group_path = group.replace('.', "/");
        let jar_name = format!("{}-{}{}.jar", artifact_id, version, classifier);
        let rel_path = format!("{}/{}/{}/{}", group_path, artifact_id, version, jar_name);
        let dest = libraries_dir.join(&rel_path);
        if dest.exists() {
            continue;
        }
        let base_url = if group.contains("fabric") || group.contains("fabricmc") {
            "https://maven.fabricmc.net/"
        } else if group.contains("neoforged") {
            "https://maven.neoforged.net/"
        } else if group.contains("forge") || group.contains("minecraftforge") || group.starts_with("net.minecraft") {
            "https://maven.minecraftforge.net/"
        } else {
            "https://libraries.minecraft.net/"
        };
        tasks.push((format!("{}{}", base_url, rel_path), dest));
    }

    if tasks.is_empty() {
        return (0, 0);
    }
    download_libraries_concurrent(tasks, 16).await
}

/// 确保原版 Minecraft 已安装；缺失时自动下载安装。
///
/// # 返回
/// Ok(()) 表示已安装（或本次自动安装成功），Err(String) 表示安装失败
pub async fn ensure_base_version_installed(
    game_version: &str,
    on_progress: Option<BaseVersionProgress>,
) -> Result<(), String> {
    let report = on_progress.unwrap_or_else(|| Box::new(|_, _| {}) as BaseVersionProgress);
    file_log(&format!("[base] 检查基础版本 {}", game_version));

    let base_dir = versions_dir().join(game_version);
    let base_json_path = base_dir.join(format!("{}.json", game_version));
    let base_jar_path = base_dir.join(format!("{}.jar", game_version));

    // 1. 已存在版本 JSON 时，校验 client.jar 是否完整（带 SHA1 就校验，无 SHA1 视为通过）
    if base_json_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&base_json_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&content) {
                let sha1 = json.pointer("/downloads/client/sha1").and_then(|v| v.as_str()).unwrap_or("");
                let jar_ok = if base_jar_path.exists() {
                    if sha1.is_empty() {
                        true
                    } else {
                        match crate::download::single::compute_sha1(&base_jar_path).await {
                            Ok(actual) => actual.eq_ignore_ascii_case(sha1),
                            Err(_) => false,
                        }
                    }
                } else {
                    false
                };
                if jar_ok {
                    file_log(&format!("[base] {} 已安装（校验通过）", game_version));
                    return Ok(());
                }
                file_log(&format!("[base] {} JAR 缺失或损坏，重新下载", game_version));
            }
        }
    }

    // 2. 开始自动安装
    report(5, "获取版本清单...".to_string());
    let manifest = crate::versions::fetch_remote_manifest(false)
        .ok_or_else(|| format!("无法获取版本清单，请检查网络后重试"))?;
    let version_info = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|it| jstr(it, "id") == game_version))
        .cloned()
        .ok_or_else(|| format!("没找到 Minecraft {}", game_version))?;
    let details_url = jstr(&version_info, "url");
    if details_url.is_empty() {
        return Err(format!("版本 {} 缺少详情地址", game_version));
    }

    report(10, "正在下载版本信息...".to_string());
    let settings = storage::load_settings();
    let download_source = jstr(&settings, "downloadSource");
    let version_details = fetch_version_details(&details_url, &download_source).await
        .map_err(|e| format!("获取版本信息失败: {}", e))?;

    // 3. 写入版本 JSON
    if std::fs::create_dir_all(&base_dir).is_err() {
        return Err(format!("无法创建版本目录 {}", game_version));
    }
    if !write_version_json(game_version, &version_details) {
        return Err(format!("无法写入版本 JSON {}", game_version));
    }

    // 4. 下载 client.jar
    report(20, "正在下载客户端文件...".to_string());
    let client_url = version_details.pointer("/downloads/client/url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_sha1 = version_details.pointer("/downloads/client/sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_size = version_details.pointer("/downloads/client/size").and_then(|v| v.as_u64()).unwrap_or(0);
    if !client_url.is_empty() {
        crate::download::download_with_mirror(
            &client_url,
            &base_jar_path,
            if client_sha1.is_empty() { None } else { Some(&client_sha1) },
            if client_size > 0 { Some(client_size) } else { None },
            &download_source,
            300,
            None,
        ).await.map_err(|e| format!("下载客户端文件失败: {}", e))?;
    }

    // 5. 并发下载核心库
    report(30, "正在下载依赖库...".to_string());
    let libs_dir = libraries_dir();
    let (_ok, fail) = download_basic_libraries(&version_details, &libs_dir).await;
    if fail > 0 {
        // 依赖库下载失败不中断安装（部分库失败时版本会被拒绝，导致整合包安装中止）。
        // 缺失库由“文件修复 / 启动前补全”自动下载补齐。
        file_log(&format!("[base] 依赖库下载失败（{} 个），将由启动前修复与补全流程补齐", fail));
    }

    report(90, "基础版本安装完成".to_string());
    file_log(&format!("[base] {} 基础版本自动安装完成", game_version));
    Ok(())
}

/// 将原版 JSON 与加载器 JSON 合并为单一独立版本 JSON。
/// 结果删除 inheritsFrom 与 jar，自含原版全部内容与加载器内容，id 替换为目标版本名。
fn merge_version_json(vanilla: &Value, loader: &Value, output_id: &str) -> Value {
    let mut out = vanilla.clone();
    let vanilla = vanilla; // 复用原始引用

    let vanilla_libs: Vec<Value> = vanilla
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let loader_libs: Vec<Value> = loader
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 合并 libraries：以原版为底，追加加载器库，按 name 去重（name 在前者更优先）
    if let Some(arr) = out.get_mut("libraries").and_then(|v| v.as_array_mut()) {
        let mut existing: Vec<String> = arr
            .iter()
            .filter_map(|l| {
                let name = jstr(l, "name");
                if name.is_empty() { None } else { Some(name) }
            })
            .collect();
        for lib in &loader_libs {
            let name = jstr(lib, "name");
            if name.is_empty() {
                arr.push(lib.clone());
            } else if !existing.iter().any(|e| e == &name) {
                arr.push(lib.clone());
                existing.push(name);
            }
        }
    } else if let Some(obj) = out.as_object_mut() {
        obj.insert("libraries".to_string(), json!(loader_libs));
    }

    // 合并 minecraftArguments（旧格式参数）
    let vanilla_args = jstr(vanilla, "minecraftArguments");
    let loader_args = jstr(loader, "minecraftArguments");
    if !loader_args.is_empty() {
        let combined = if vanilla_args.is_empty() {
            loader_args
        } else {
            format!("{} {}", vanilla_args, loader_args)
        };
        out["minecraftArguments"] = json!(combined);
    }

    // 合并 arguments 对象中的数组字段（game / jvm），按值去重
    if let (Some(out_args), Some(ld_args)) = (
        out.get_mut("arguments").and_then(|v| v.as_object_mut()),
        loader.get("arguments").and_then(|v| v.as_object()),
    ) {
        for (k, v) in ld_args {
            if let Some(ld_arr) = v.as_array() {
                if k == "game" || k == "jvm" {
                    let existing = out_args
                        .get(k)
                        .and_then(|e| e.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let mut seen: Vec<String> = existing
                        .iter()
                        .filter_map(|i| i.as_str())
                        .map(|s| s.to_string())
                        .collect();
                    let mut merged = existing;
                    for item in ld_arr {
                        let s = item.as_str().map(|s| s.to_string()).unwrap_or_default();
                        if s.is_empty() {
                            merged.push(item.clone());
                        } else if !seen.iter().any(|x| x == &s) {
                            merged.push(item.clone());
                            seen.push(s);
                        }
                    }
                    out_args.insert(k.clone(), json!(merged));
                }
            }
        }
    }

    // mainClass：加载器优先，否则保留原版
    let loader_main = jstr(loader, "mainClass");
    if !loader_main.is_empty() {
        out["mainClass"] = json!(loader_main);
    }

    // 清理与修正：删除继承字段，id 设为输出版本名
    if let Some(obj) = out.as_object_mut() {
        obj.remove("inheritsFrom");
        obj.remove("jar");
        obj.remove("_comment_");
        obj.insert("id".to_string(), json!(output_id));
        obj.insert("type".to_string(), json!("release"));
    }

    out
}

/// 将加载器 JSON 与对应原版合并，产出单一独立版本并落盘（不遗留独立原版目录）。
///
/// # 参数
/// - `game_version`: Minecraft 版本号
/// - `version_id`: 输出版本目录名
/// - `loader_json`: 加载器 JSON（Fabric/OptiFine 等，可含 inheritsFrom，会被删除）
/// - `on_progress`: 进度回调（可选）
///
/// # 返回
/// Ok(最终合并后的版本 JSON)
pub async fn install_merged_loader(
    game_version: &str,
    version_id: &str,
    loader_json: &Value,
    on_progress: Option<BaseVersionProgress>,
) -> Result<Value, String> {
    use crate::download::download_with_mirror;

    let report = on_progress.unwrap_or_else(|| Box::new(|_, _| {}) as BaseVersionProgress);

    // 1. 拉取原版版本清单，找到详情地址
    report(5, "获取版本清单...".to_string());
    let manifest = crate::versions::fetch_remote_manifest(false)
        .ok_or_else(|| format!("无法获取版本清单，请检查网络后重试"))?;
    let version_info = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|it| jstr(it, "id") == game_version))
        .cloned()
        .ok_or_else(|| format!("没找到 Minecraft {}", game_version))?;
    let details_url = jstr(&version_info, "url");
    if details_url.is_empty() {
        return Err(format!("版本 {} 缺少详情地址", game_version));
    }

    // 2. 拉取原版版本详情 JSON
    report(10, "正在下载版本信息...".to_string());
    let settings = storage::load_settings();
    let download_source = jstr(&settings, "downloadSource");
    let vanilla = fetch_version_details(&details_url, &download_source).await
        .map_err(|e| format!("获取版本信息失败: {}", e))?;

    // 3. 合并为独立版本
    let merged = merge_version_json(&vanilla, loader_json, version_id);
    file_log(&format!("[loader] 合并版本 {} 完成（自含原版内容）", version_id));

    // 4. 创建版本目录并写入 JSON（须先落盘，供客户端 JAR 下载后位于同目录）
    let version_dir = versions_dir().join(version_id);
    if std::fs::create_dir_all(&version_dir).is_err() {
        return Err(format!("无法创建版本目录 {}", version_dir.display()));
    }
    if !write_version_json(version_id, &merged) {
        return Err(format!("无法写入版本 JSON {}", version_id));
    }

    // 5. 下载客户端 JAR 到版本目录（合并后没有继承，JAR 必须自含）
    report(20, "正在下载客户端文件...".to_string());
    let client_url = vanilla.pointer("/downloads/client/url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_sha1 = vanilla.pointer("/downloads/client/sha1").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let client_size = vanilla.pointer("/downloads/client/size").and_then(|v| v.as_u64()).unwrap_or(0);
    if !client_url.is_empty() {
        let jar_path = version_dir.join(format!("{}.jar", version_id));
        download_with_mirror(
            &client_url,
            &jar_path,
            if client_sha1.is_empty() { None } else { Some(&client_sha1) },
            if client_size > 0 { Some(client_size) } else { None },
            &download_source,
            300,
            None,
        ).await.map_err(|e| format!("下载客户端文件失败: {}", e))?;
    }

    // 6. 下载合并后版本的全部库
    report(30, "正在下载依赖库...".to_string());
    let libs_dir = libraries_dir();
    let (_ok, fail) = download_basic_libraries(&merged, &libs_dir).await;
    if fail > 0 {
        // 所有需下载的库均失败：不中断安装（版本被拒绝会导致整个整合包装不上）。
        // 缺失库由“文件修复 / 启动前补全”自动下载补齐。
        file_log(&format!("[loader] 依赖库下载失败（{} 个），将由启动前修复与补全流程补齐", fail));
    }

    report(90, "版本安装完成".to_string());
    Ok(merged)
}

/// 若某原版版本不再被任何其他版本以 inheritsFrom / jar 引用，则删除其版本目录。
/// 用于加载器安装完成后清理仅作 patch 输入的遗留原版目录，避免遗留独立原版目录。
pub fn cleanup_orphan_vanilla(game_version: &str) {
    if game_version.is_empty() {
        return;
    }
    let versions_root = versions_dir();
    let mut referenced = false;
    if let Ok(entries) = std::fs::read_dir(&versions_root) {
        for entry in entries.flatten() {
            let dir_name = match entry.file_name().to_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            if dir_name == game_version {
                continue;
            }
            let json_path = versions_root.join(&dir_name).join(format!("{}.json", dir_name));
            let content = match std::fs::read_to_string(&json_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if content.contains("\"inheritsFrom\"") || content.contains("\"jar\"") {
                if let Ok(v) = serde_json::from_str::<Value>(&content) {
                    if jstr(&v, "inheritsFrom") == game_version || jstr(&v, "jar") == game_version {
                        referenced = true;
                        break;
                    }
                }
            }
        }
    }
    if referenced {
        return;
    }
    let target = versions_root.join(game_version);
    if target.exists() {
        if let Err(e) = std::fs::remove_dir_all(&target) {
            file_log(&format!("[cleanup] 清理原版目录失败 {}: {}", target.display(), e));
        } else {
            file_log(&format!("[cleanup] 已清理无引用的原版目录: {}", target.display()));
        }
    }
}

/// 获取当前时间的 ISO 8601 字符串（UTC）
pub fn now_iso() -> String {
    crate::utils::now_iso()
}

/// 验证文件是否是合法 ZIP（检查 PK 头魔数）
/// 用于检查下载的 installer JAR 完整性
pub fn verify_zip_magic(path: &Path) -> bool {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(mut f) => {
            let mut buf = [0u8; 4];
            if f.read(&mut buf).is_ok() {
                // PK\x03\x04 = ZIP 本地文件头魔数
                return buf[0] == 0x50 && buf[1] == 0x4B && buf[2] == 0x03 && buf[3] == 0x04;
            }
            false
        }
        Err(_) => false,
    }
}

/// 拼接两个路径部分（处理分隔符）
#[allow(dead_code)]
pub fn join_paths(base: &str, relative: &str) -> String {
    let base = base.trim_end_matches(|c| c == '/' || c == '\\');
    let relative = relative.trim_start_matches(|c| c == '/' || c == '\\');
    format!("{}/{}", base, relative)
}

/// 异步运行子进程并捕获输出（用于调用 Java installer）
///
/// # 参数
/// - `java_path`: Java 可执行文件路径
/// - `args`: 命令行参数列表
/// - `cwd`: 工作目录（可选）
/// - `timeout_secs`: 超时时间（秒）
///
/// # 返回
/// Ok((exit_code, stdout, stderr)) 或 Err(错误信息)
pub async fn run_subprocess_with_timeout(
    java_path: &Path,
    args: &[String],
    cwd: Option<&Path>,
    timeout_secs: u64,
) -> Result<(i32, String, String), String> {
    use tokio::process::Command;
    use tokio::time::Duration;

    let mut cmd = Command::new(java_path);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());

    // Windows 下隐藏窗口
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn().map_err(|e| format!("启动进程失败: {}", e))?;
    let id = child.id();

    // 等待完成或超时
    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(-1);
            Ok((code, stdout, stderr))
        }
        Ok(Err(e)) => Err(format!("进程执行失败: {}", e)),
        Err(_) => {
            // 超时，尝试杀死进程
            if let Some(pid) = id {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill")
                        .args(&["/F", "/T", "/PID"])
                        .arg(pid.to_string())
                        .creation_flags(0x08000000)
                        .spawn();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = std::process::Command::new("kill")
                        .arg("-9")
                        .arg(pid.to_string())
                        .spawn();
                }
            }
            Err(format!("进程执行超时（{} 秒）", timeout_secs))
        }
    }
}

/// 用系统自带的 curl 下载文件（绕过程序内部网络无法连接某些源的情况）
/// curl 使用系统网络通道和系统证书，能连上程序内部连不上的地址。
///
/// # 参数
/// - `url`: 下载地址
/// - `dest`: 目标文件路径
/// - `timeout_secs`: 最大超时时间（秒）
///
/// # 返回
/// Ok(()) 表示下载成功；Err(String) 表示失败
pub async fn download_with_curl(url: &str, dest: &Path, timeout_secs: u64) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return Err("无法创建目录".to_string());
        }
    }
    let _ = std::fs::remove_file(dest);

    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = tokio::process::Command::new("curl.exe");
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = tokio::process::Command::new("curl");

    cmd.arg("--silent")
        .arg("--insecure") // 跳过证书验证：部分源证书链不完整，严格验证会全部失败
        .arg("--fail")
        .arg("--location")
        .arg("--retry")
        .arg("3")
        .arg("--connect-timeout")
        .arg("15")
        .arg("--max-time")
        .arg(timeout_secs.to_string())
        .arg("--output")
        .arg(dest)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("curl 启动失败: {}", e))?;

    if !output.status.success() {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "curl 下载失败 (exit {})",
            output.status.code().unwrap_or(-1)
        ));
    }
    if !dest.exists() || std::fs::metadata(dest).map(|m| m.len() == 0).unwrap_or(true) {
        let _ = std::fs::remove_file(dest);
        return Err("curl 下载结果为空".to_string());
    }
    Ok(())
}
