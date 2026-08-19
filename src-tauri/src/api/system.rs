// api/system.rs — 系统信息路由
// 兼容原项目 server/api/routes/system.js
// 路由清单：
//   GET /api/status                启动器运行状态
//   GET /api/system/memory         系统内存信息
//   GET /api/system/memory-info    内存使用占比概览
//   POST /api/jvm/preheat          预热 JVM（暂占位）
//   POST /api/jvm/generate-cds     生成 CDS 归档（暂占位）
//   GET /api/jvm/cds-status        CDS 状态（暂占位）
//   GET /api/jvm/optimize-args     优化 JVM 启动参数（暂占位）
//   POST /api/cleanup              执行清理任务，释放磁盘空间
//   GET /api/cleanup/scan          扫描可清理空间（不删除）
// 注：/api/java/* 路由在 java.rs 中实现，由 mod.rs 优先分发

use serde_json::{json, Value};
use std::path::Path;

use super::ApiResult;
use crate::storage;

/// 获取系统总内存（KB），仅 Windows 实现简单版本
pub(crate) fn get_system_memory_kb() -> (u64, u64) {
    // Windows：用 GlobalMemoryStatusEx
    #[cfg(target_os = "windows")]
    {
        use std::mem::{size_of, zeroed};
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_extended_virtual: u64,
        }
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(lpBuffer: *mut MemoryStatusEx) -> i32;
        }
        unsafe {
            let mut status: MemoryStatusEx = zeroed();
            status.dw_length = size_of::<MemoryStatusEx>() as u32;
            if GlobalMemoryStatusEx(&mut status) != 0 {
                return (status.ull_total_phys, status.ull_avail_phys);
            }
        }
    }
    (0, 0)
}

/// 计算自动推荐分配内存（MB）
/// 规则：总内存的 60%，但不超过 8192MB，不少于 1024MB
fn auto_memory_mb(total_mb: u64) -> u64 {
    if total_mb == 0 {
        return 2048;
    }
    let auto_mb = total_mb * 60 / 100;
    auto_mb.max(1024).min(8192)
}

pub fn handle(method: &str, path: &str, params: &Option<Value>, body: &Option<Value>) -> Option<ApiResult> {
    let key = format!("{} {}", method.to_uppercase(), path);

    match key.as_str() {
        "GET /api/system/memory" => {
            let (total_bytes, avail_bytes) = get_system_memory_kb();
            // Windows 返回的是字节，按 1024 转换
            let total_bytes = total_bytes;
            let free_bytes = avail_bytes;
            let total_mb = total_bytes / 1024 / 1024;
            let free_mb = free_bytes / 1024 / 1024;
            let used_mb = total_mb.saturating_sub(free_mb);
            let total_gb = total_mb / 1024;
            let free_gb = free_mb / 1024;
            let used_gb = used_mb / 1024;
            let auto_mb = auto_memory_mb(total_mb);
            let auto_gb = auto_mb / 1024;

            Some(ApiResult::ok(json!({
                "totalBytes": total_bytes,
                "freeBytes": free_bytes,
                "totalMB": total_mb,
                "freeMB": free_mb,
                "usedMB": used_mb,
                "totalGB": total_gb,
                "freeGB": free_gb,
                "usedGB": used_gb,
                "autoMB": auto_mb,
                "autoGB": auto_gb
            })))
        }

        "GET /api/system/memory-info" => {
            let (total_bytes, avail_bytes) = get_system_memory_kb();
            let total_mb = total_bytes / 1024 / 1024;
            let free_mb = avail_bytes / 1024 / 1024;
            let used_mb = total_mb.saturating_sub(free_mb);
            let usage_percent = if total_mb > 0 {
                used_mb * 100 / total_mb
            } else {
                0
            };
            Some(ApiResult::ok(json!({
                "totalMB": total_mb,
                "freeMB": free_mb,
                "usedMB": used_mb,
                "usagePercent": usage_percent,
                "loadPercent": usage_percent,
                "total": total_bytes,
                "used": total_bytes.saturating_sub(avail_bytes)
            })))
        }

        // 占位路由（后续逐步迁移）
        "GET /api/status" => Some(ApiResult::ok(json!({
            "running": true,
            "downloadEngine": "tauri-native",
            "version": "1.3.3-tauri"
        }))),

        "POST /api/jvm/preheat" => Some(handle_jvm_preheat(body)),

        "POST /api/jvm/generate-cds" => Some(handle_jvm_generate_cds(body)),

        "GET /api/jvm/cds-status" => Some(handle_jvm_cds_status(params)),

        "GET /api/jvm/optimize-args" => Some(handle_jvm_optimize_args(params)),

        "POST /api/cleanup" => Some(handle_cleanup()),

        // 前端 cleanupScan 通过 POST 调用，保持与 GET 等价（Electron 端为 GET）
        "GET /api/cleanup/scan" | "POST /api/cleanup/scan" => Some(handle_cleanup_scan()),

        "POST /api/memory-optimize" => Some(handle_memory_optimize()),

        _ => None,
    }
}

// ============== 内存优化 ==============

#[cfg(target_os = "windows")]
fn do_memory_optimize_purge() -> Result<(), String> {
    use std::ffi::c_void;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlAdjustPrivilege(
            privilege: u32,
            enable: i32,
            current_thread: i32,
            enabled: *mut i32,
        ) -> i32;
        fn NtSetSystemInformation(
            system_information_class: i32,
            system_information: *mut c_void,
            system_information_length: i32,
        ) -> i32;
    }

    const SE_INCREASE_QUOTA_PRIVILEGE: u32 = 5;
    const SE_PROFILE_SINGLE_PROCESS_PRIVILEGE: u32 = 13;
    const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
    const SYSTEM_FILE_CACHE_INFORMATION_EX: i32 = 81;
    const SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION: i32 = 130;
    const SYSTEM_REGISTRY_RECONCILIATION_INFORMATION: i32 = 155;

    unsafe {
        // 启用内存优化所需特权
        let mut enabled: i32 = 0;
        for privilege in [SE_INCREASE_QUOTA_PRIVILEGE, SE_PROFILE_SINGLE_PROCESS_PRIVILEGE] {
            let status = RtlAdjustPrivilege(privilege, 1, 0, &mut enabled);
            if status != 0 {
                return Err(format!("启用内存优化权限失败（错误代码：{}）", status));
            }
        }

        // 内存列表操作：清空工作集(2)、刷新修改列表(3)、清理待机列表(4)、清理低优先级待机列表(5)
        for op in [2i32, 3, 4, 5] {
            let op_bytes: [u8; 4] = op.to_ne_bytes();
            let status = NtSetSystemInformation(
                SYSTEM_MEMORY_LIST_INFORMATION,
                op_bytes.as_ptr() as *mut c_void,
                4,
            );
            if status != 0 {
                return Err(format!("内存优化操作 {} 失败（错误代码：{}）", op, status));
            }
        }

        // 刷新系统文件缓存（最小/最大工作集设为 SIZE_MAX 触发缓存收缩）
        #[repr(C)]
        #[derive(Default)]
        struct SystemFileCacheInformation {
            current_size: usize,
            peak_size: usize,
            page_fault_count: usize,
            minimum_working_set: usize,
            maximum_working_set: usize,
            current_size_including_transition_in_pages: usize,
            peak_size_including_transition_in_pages: usize,
            transition_re_purpose_count: usize,
            flags: usize,
        }
        let mut file_cache = SystemFileCacheInformation::default();
        file_cache.minimum_working_set = usize::MAX;
        file_cache.maximum_working_set = usize::MAX;
        let status = NtSetSystemInformation(
            SYSTEM_FILE_CACHE_INFORMATION_EX,
            &mut file_cache as *mut SystemFileCacheInformation as *mut c_void,
            std::mem::size_of::<SystemFileCacheInformation>() as i32,
        );
        if status != 0 {
            return Err(format!("刷新文件缓存失败（错误代码：{}）", status));
        }

        // 注册表内存对账
        let status = NtSetSystemInformation(
            SYSTEM_REGISTRY_RECONCILIATION_INFORMATION,
            std::ptr::null_mut(),
            0,
        );
        if status != 0 {
            return Err(format!("注册表内存对账失败（错误代码：{}）", status));
        }

        // 物理内存合并
        #[repr(C)]
        #[derive(Default)]
        struct MemoryCombineInformationEx {
            handle: *mut c_void,
            pages_combined: usize,
            flags: u32,
        }
        let mut combine = MemoryCombineInformationEx::default();
        let status = NtSetSystemInformation(
            SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION,
            &mut combine as *mut MemoryCombineInformationEx as *mut c_void,
            std::mem::size_of::<MemoryCombineInformationEx>() as i32,
        );
        if status != 0 {
            return Err(format!("合并物理内存失败（错误代码：{}）", status));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn do_memory_optimize_purge() -> Result<(), String> {
    Err("内存优化仅支持 Windows".to_string())
}

/// POST /api/memory-optimize — 执行内存优化
fn handle_memory_optimize() -> ApiResult {
    let (_, before_avail) = get_system_memory_kb();
    let before_mb = before_avail / 1024 / 1024;

    match do_memory_optimize_purge() {
        Ok(()) => {
            let (_, after_avail) = get_system_memory_kb();
            let after_mb = after_avail / 1024 / 1024;
            let freed_mb = after_mb.saturating_sub(before_mb);
            ApiResult::ok(json!({
                "success": true,
                "freedMB": freed_mb,
                "beforeMB": before_mb,
                "afterMB": after_mb
            }))
        }
        Err(e) => ApiResult::err(500, e.as_str()),
    }
}

// ============== 清理功能 ==============

/// 递归删除目录并统计释放字节数
fn safe_rm_dir(dir: &Path) -> u64 {
    if !dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = entry.file_type() {
                if meta.is_dir() {
                    bytes += safe_rm_dir(&path);
                } else {
                    if let Ok(m) = std::fs::metadata(&path) {
                        bytes += m.len();
                    }
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
    bytes
}

/// 递归扫描目录占用字节数（不删除）
fn scan_dir_size(dir: &Path) -> u64 {
    if !dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = entry.file_type() {
                if meta.is_dir() {
                    bytes += scan_dir_size(&path);
                } else if let Ok(m) = std::fs::metadata(&path) {
                    bytes += m.len();
                }
            }
        }
    }
    bytes
}

/// 清理各版本目录下的 logs / crash-reports / latest.log
fn clean_version_logs() -> u64 {
    let versions_dir = storage::resolve_data_dir().join("versions");
    if !versions_dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let version_dir = entry.path();
            for sub in &["logs", "crash-reports"] {
                let d = version_dir.join(sub);
                if d.exists() {
                    bytes += safe_rm_dir(&d);
                }
            }
            let mc_log = version_dir.join("latest.log");
            if mc_log.exists() {
                if let Ok(m) = std::fs::metadata(&mc_log) {
                    bytes += m.len();
                }
                let _ = std::fs::remove_file(&mc_log);
            }
        }
    }
    bytes
}

/// 清理临时目录
fn clean_temp_dir() -> u64 {
    let tmp = storage::resolve_data_dir().join("temp");
    if !tmp.exists() {
        return 0;
    }
    safe_rm_dir(&tmp)
}

/// 清理 natives 解压目录
fn clean_natives() -> u64 {
    let natives_dir = storage::resolve_data_dir().join("natives");
    if !natives_dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(&natives_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.file_type() {
                if meta.is_dir() {
                    bytes += safe_rm_dir(&entry.path());
                }
            }
        }
    }
    bytes
}

/// 清理图标缓存目录
fn clean_icon_cache() -> u64 {
    let icon_cache = storage::resolve_data_dir().join("icon-cache");
    if !icon_cache.exists() {
        return 0;
    }
    safe_rm_dir(&icon_cache)
}

/// 清理整合包缓存（.mrpack / .zip）
fn clean_modpack_cache() -> u64 {
    let settings = storage::load_settings();
    let game_dir = crate::utils::get_str(&settings, "gameDir");
    let base = if game_dir.is_empty() {
        storage::resolve_data_dir()
    } else {
        std::path::PathBuf::from(&game_dir)
    };
    let mp_dir = base.join("modpacks");
    if !mp_dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(&mp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext = ext.to_lowercase();
                if ext == "mrpack" || ext == "zip" {
                    if let Ok(m) = std::fs::metadata(&path) {
                        bytes += m.len();
                    }
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    bytes
}

/// 清理下载缓存目录（保留 .gitkeep）
fn clean_download_cache() -> u64 {
    let cache_dir = storage::resolve_data_dir().join("cache");
    if !cache_dir.exists() {
        return 0;
    }
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.file_type() {
                if meta.is_file() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name == ".gitkeep" {
                        continue;
                    }
                    let path = entry.path();
                    if let Ok(m) = std::fs::metadata(&path) {
                        bytes += m.len();
                    }
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    bytes
}

/// POST /api/cleanup — 执行清理任务，释放磁盘空间
fn handle_cleanup() -> ApiResult {
    let mut details = serde_json::Map::new();
    let game_logs = clean_version_logs();
    let temp_files = clean_temp_dir();
    let natives = clean_natives();
    let icon_cache = clean_icon_cache();
    let modpack_cache = clean_modpack_cache();
    let download_cache = clean_download_cache();

    details.insert("gameLogs".to_string(), json!(game_logs));
    details.insert("tempFiles".to_string(), json!(temp_files));
    details.insert("natives".to_string(), json!(natives));
    details.insert("iconCache".to_string(), json!(icon_cache));
    details.insert("modpackCache".to_string(), json!(modpack_cache));
    details.insert("downloadCache".to_string(), json!(download_cache));

    let total_bytes = game_logs + temp_files + natives + icon_cache + modpack_cache + download_cache;
    let total_mb = (total_bytes as f64 / (1024.0 * 1024.0) * 100.0).round() / 100.0;

    ApiResult::ok(json!({
        "success": true,
        "freedBytes": total_bytes,
        "freedMB": total_mb,
        "details": details,
        "message": format!("清理完成，释放 {} MB 空间", total_mb)
    }))
}

/// GET /api/cleanup/scan — 扫描可清理空间（不删除）
fn handle_cleanup_scan() -> ApiResult {
    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");

    let mut details = serde_json::Map::new();

    // 各版本目录下的 logs / crash-reports
    if versions_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let version_id = entry.file_name().to_string_lossy().to_string();
                for sub in &["logs", "crash-reports"] {
                    let d = entry.path().join(sub);
                    if d.exists() {
                        let size = scan_dir_size(&d);
                        if size > 0 {
                            details.insert(
                                format!("{}/{}", version_id, sub),
                                json!(size),
                            );
                        }
                    }
                }
            }
        }
    }

    let temp_size = scan_dir_size(&data_dir.join("temp"));
    if temp_size > 0 {
        details.insert("temp".to_string(), json!(temp_size));
    }

    let natives_size = scan_dir_size(&data_dir.join("natives"));
    if natives_size > 0 {
        details.insert("natives".to_string(), json!(natives_size));
    }

    let icon_size = scan_dir_size(&data_dir.join("icon-cache"));
    if icon_size > 0 {
        details.insert("iconCache".to_string(), json!(icon_size));
    }

    let cache_size = scan_dir_size(&data_dir.join("cache"));
    if cache_size > 0 {
        details.insert("cache".to_string(), json!(cache_size));
    }

    // 整合包缓存
    let settings = storage::load_settings();
    let game_dir = crate::utils::get_str(&settings, "gameDir");
    let base = if game_dir.is_empty() {
        data_dir
    } else {
        std::path::PathBuf::from(&game_dir)
    };
    let mp_dir = base.join("modpacks");
    if mp_dir.exists() {
        let mut mp_total = 0u64;
        if let Ok(entries) = std::fs::read_dir(&mp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext = ext.to_lowercase();
                    if ext == "mrpack" || ext == "zip" {
                        if let Ok(m) = std::fs::metadata(&path) {
                            mp_total += m.len();
                        }
                    }
                }
            }
        }
        if mp_total > 0 {
            details.insert("modpacks".to_string(), json!(mp_total));
        }
    }

    let total_bytes: u64 = details.values().filter_map(|v| v.as_u64()).sum();
    let total_mb = (total_bytes as f64 / (1024.0 * 1024.0) * 100.0).round() / 100.0;

    ApiResult::ok(json!({
        "success": true,
        "details": details,
        "totalBytes": total_bytes,
        "totalMB": total_mb
    }))
}

// ============== JVM 相关功能 ==============

/// 读取 Java 主版本号
fn get_java_major_version(java_path: &str) -> u32 {
    if java_path.is_empty() || !Path::new(java_path).exists() {
        return 0;
    }
    // 先读 release 文件（快）
    let java_home = Path::new(java_path)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    if let Some(home) = java_home {
        let release = home.join("release");
        if let Ok(content) = std::fs::read_to_string(&release) {
            for line in content.lines() {
                if let Some(v) = line.strip_prefix("JAVA_VERSION=") {
                    let v = v.trim_matches('"');
                    if let Some(first) = v.split('.').next() {
                        if first == "1" {
                            return v.split('.').nth(1)
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0);
                        }
                        return first.parse().unwrap_or(0);
                    }
                }
            }
        }
    }
    // 回退：执行 java -version
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
    if let Ok(out) = cmd.output()
    {
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        for line in text.lines() {
            if line.contains("version") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        let v = &line[start + 1..start + 1 + end];
                        if let Some(first) = v.split('.').next() {
                            if first == "1" {
                                return v.split('.').nth(1)
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0);
                            }
                            return first.parse().unwrap_or(0);
                        }
                    }
                }
            }
        }
    }
    0
}

/// POST /api/jvm/preheat — 预热 JVM 以加速首次启动
/// 启动一个短生命周期的 Java 进程加载核心类
fn handle_jvm_preheat(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let java_path = crate::utils::get_str(&data, "javaPath").to_string();
    let max_mem_mb = data.get("maxMemMB")
        .and_then(|v| v.as_u64())
        .unwrap_or(1024) as u32;

    if java_path.is_empty() || !Path::new(&java_path).exists() {
        return ApiResult::ok(json!({
            "success": false,
            "error": "Java 路径不存在"
        }));
    }

    // 异步启动预热进程（不阻塞响应）
    let java_path_owned = java_path.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        let mut cmd = {
            use std::os::windows::process::CommandExt;
            let mut c = std::process::Command::new(&java_path_owned);
            c.args([
                "-Xmx".to_string() + &max_mem_mb.to_string() + "m",
                "-version".to_string(),
            ]);
            c.creation_flags(0x08000000);
            c
        };
        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut c = std::process::Command::new(&java_path_owned);
            c.args([
                "-Xmx".to_string() + &max_mem_mb.to_string() + "m",
                "-version".to_string(),
            ]);
            c
        };
        let result = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = result {
            // 等 2 秒让 JVM 加载核心类
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = child.kill();
        }
    });

    ApiResult::ok(json!({ "success": true }))
}

/// POST /api/jvm/generate-cds — 为指定版本生成 CDS 类共享归档
fn handle_jvm_generate_cds(body: &Option<Value>) -> ApiResult {
    let data = body.clone().unwrap_or(Value::Null);
    let version_id = crate::utils::get_str(&data, "versionId").to_string();
    if version_id.is_empty() {
        return ApiResult::ok(json!({
            "success": false,
            "error": "缺少 versionId 参数"
        }));
    }

    let data_dir = storage::resolve_data_dir();
    let cds_dir = data_dir.join("cds");
    if let Err(e) = std::fs::create_dir_all(&cds_dir) {
        return ApiResult::ok(json!({
            "success": false,
            "error": format!("无法创建 CDS 目录: {}", e)
        }));
    }

    let cds_archive = cds_dir.join(format!("{}.jsa", version_id));

    // 读取版本 JSON 找 Java 路径
    let versions_dir = data_dir.join("versions");
    let version_json_path = versions_dir.join(&version_id).join(format!("{}.json", version_id));
    let version_json: Value = match std::fs::read_to_string(&version_json_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return ApiResult::ok(json!({
                "success": false,
                "error": "版本 JSON 损坏"
            })),
        },
        Err(_) => return ApiResult::ok(json!({
            "success": false,
            "error": "版本 JSON 缺失"
        })),
    };

    let settings = storage::load_settings();
    let java_path = crate::launch::dep_check::select_java_for_version(
        &version_id,
        &settings,
        &version_json,
    );

    if java_path.is_empty() || !Path::new(&java_path).exists() {
        return ApiResult::ok(json!({
            "success": false,
            "error": "Java 未找到"
        }));
    }

    let major_ver = get_java_major_version(&java_path);
    if major_ver < 8 {
        return ApiResult::ok(json!({
            "success": false,
            "error": "CDS 需要 Java 8 或以上版本"
        }));
    }

    // 优先复用 JDK 自带的默认归档
    let java_home = Path::new(&java_path)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    if let Some(home) = java_home {
        let default_jsa = home.join("lib").join("server").join("classes.jsa");
        if default_jsa.exists() {
            if std::fs::copy(&default_jsa, &cds_archive).is_ok() {
                return ApiResult::ok(json!({
                    "success": true,
                    "archive": cds_archive.to_string_lossy(),
                    "source": "default"
                }));
            }
        }
    }

    // 调用 java -Xshare:dump 生成归档
    let archive_arg = format!("-XX:SharedArchiveFile={}", cds_archive.display());
    #[cfg(target_os = "windows")]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(&java_path);
        c.args(["-Xshare:dump", &archive_arg]);
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = std::process::Command::new(&java_path);
        c.args(["-Xshare:dump", &archive_arg]);
        c
    };
    let output = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(out) => {
            if cds_archive.exists() {
                if let Ok(meta) = std::fs::metadata(&cds_archive) {
                    if meta.len() > 1024 {
                        return ApiResult::ok(json!({
                            "success": true,
                            "archive": cds_archive.to_string_lossy(),
                            "source": "generated",
                            "sizeKB": meta.len() / 1024
                        }));
                    }
                }
            }
            let err_msg = String::from_utf8_lossy(&out.stderr);
            ApiResult::ok(json!({
                "success": false,
                "error": err_msg.chars().take(300).collect::<String>()
            }))
        }
        Err(e) => ApiResult::ok(json!({
            "success": false,
            "error": format!("执行失败: {}", e)
        })),
    }
}

/// GET /api/jvm/cds-status — 查询指定版本的 CDS 归档状态
fn handle_jvm_cds_status(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if version_id.is_empty() {
        return ApiResult::ok(json!({ "available": false }));
    }

    let archive = storage::resolve_data_dir()
        .join("cds")
        .join(format!("{}.jsa", version_id));

    if archive.exists() {
        if let Ok(meta) = std::fs::metadata(&archive) {
            return ApiResult::ok(json!({
                "available": true,
                "archive": archive.to_string_lossy(),
                "sizeKB": meta.len() / 1024,
                "modified": meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0)
            }));
        }
    }

    ApiResult::ok(json!({ "available": false }))
}

/// GET /api/jvm/optimize-args — 根据内存与模组数量推算优化后的 JVM 启动参数
fn handle_jvm_optimize_args(params: &Option<Value>) -> ApiResult {
    let version_id = params
        .as_ref()
        .and_then(|p| p.get("versionId"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if version_id.is_empty() {
        return ApiResult::err(400, "缺少 versionId 参数");
    }

    // 系统内存（KB → 字节）
    let (total_kb, avail_kb) = get_system_memory_kb();
    let total_mem_bytes = total_kb * 1024;
    let free_mem_bytes = avail_kb * 1024;
    let available_gb = free_mem_bytes as f64 / 1_073_741_824.0;

    // 统计 mods 目录下 jar 文件数量
    let version_dir = storage::resolve_data_dir().join("versions").join(version_id);
    let mods_dir = version_dir.join("mods");
    let mod_count: u32 = if mods_dir.exists() {
        std::fs::read_dir(&mods_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_type().map(|t| t.is_file()).unwrap_or(false)
                            && e.path()
                                .extension()
                                .and_then(|x| x.to_str())
                                .map(|x| x.eq_ignore_ascii_case("jar"))
                                .unwrap_or(false)
                    })
                    .count() as u32
            })
            .unwrap_or(0)
    } else {
        0
    };

    // 基于模组数量估算各档内存阈值（与原项目一致）
    let (t0, t1, t2, t3) = if mod_count > 0 {
        (
            0.5 + mod_count as f64 / 150.0,
            1.5 + mod_count as f64 / 90.0,
            2.7 + mod_count as f64 / 50.0,
            4.5 + mod_count as f64 / 25.0,
        )
    } else {
        (0.5, 1.5, 2.5, 4.0)
    };

    // 按梯度累加可分配内存
    let mut ram_give: f64 = 0.0;
    let mut ram_available = available_gb;

    let mut delta = t1;
    ram_give += ram_available.min(delta);
    ram_available -= delta;

    delta = t2 - t1;
    ram_give += (ram_available * 0.7).min(delta);
    ram_available -= delta / 0.7;

    delta = t3 - t2;
    ram_give += (ram_available * 0.4).min(delta);
    ram_available -= delta / 0.4;

    delta = t3;
    ram_give += (ram_available * 0.15).min(delta);

    ram_give = ram_give.max(t0);
    ram_give = (ram_give * 10.0).round() / 10.0;

    // 不超过系统总内存的 70%
    let max_gb = total_mem_bytes as f64 / 1_073_741_824.0 * 0.7;
    ram_give = ram_give.min(max_gb);

    let total_ram_mb = (ram_give * 1024.0).floor() as u32;
    let new_gen_mb = (total_ram_mb as f64 * 0.15).floor() as u32;

    let args = vec![
        format!("-Xmx{}m", total_ram_mb),
        format!("-Xmn{}m", new_gen_mb),
        "-XX:+UseG1GC".to_string(),
        "-XX:-UseAdaptiveSizePolicy".to_string(),
        "-XX:-OmitStackTraceInFastThrow".to_string(),
        "-Djdk.lang.Process.allowAmbiguousCommands=true".to_string(),
        "-Dfml.ignoreInvalidMinecraftCertificates=True".to_string(),
        "-Dfml.ignorePatchDiscrepancies=True".to_string(),
        "-Dlog4j2.formatMsgNoLookups=true".to_string(),
    ];

    ApiResult::ok(json!({
        "args": args.join(" "),
        "xmxMB": total_ram_mb,
        "xmnMB": new_gen_mb,
        "ramGB": ram_give,
        "modCount": mod_count,
        "totalMemGB": total_mem_bytes as f64 / 1_073_741_824.0,
        "freeMemGB": free_mem_bytes as f64 / 1_073_741_824.0
    }))
}
