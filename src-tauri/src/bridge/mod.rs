// bridge/mod.rs — 浏览器扩展桥接层
// 职责：
//   1. 启动参数处理：--native-host（原生消息宿主）/ --download（外部传入下载请求）
//   2. 单实例命名管道：把二次实例的请求转发给已运行实例
//   3. 待处理下载请求队列：供前端 /api/download-custom/pending 取用
//
// 设计约束：
//   - 不新增依赖：命名管道走 tokio::net::windows::named_pipe，转发用 std::fs 打开管道
//   - 非 Windows 平台全部降级为空实现（项目需同时构建 Linux/macOS）

use serde_json::{json, Value};
use std::sync::{LazyLock, Mutex};

pub mod extension;
pub mod native_host;
pub mod register;
pub mod scan;
mod ipc;

/// 待前端取走的下载请求队列（FIFO）
static PENDING: LazyLock<Mutex<Vec<Value>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// 写入一条待处理请求（来自命令行参数或命名管道转发）
pub fn push_pending(req: Value) {
    let mut g = PENDING.lock().unwrap();
    // 队列上限：避免异常情况下无限堆积
    if g.len() >= 16 {
        g.remove(0);
    }
    g.push(req);
}

/// 取走最早的一条待处理请求
pub fn drain_pending() -> Option<Value> {
    let mut g = PENDING.lock().unwrap();
    if g.is_empty() {
        None
    } else {
        Some(g.remove(0))
    }
}

/// 当前待处理条数
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn pending_count() -> usize {
    PENDING.lock().unwrap().len()
}

/// 判断当前进程承担的是否为「首实例」角色（供安装管道服务端）
pub fn install(app: tauri::AppHandle) {
    ipc::serve(app);
}

/// 处理启动参数。返回 true 表示启动流程已在此结束（不应再创建 Tauri 窗口）。
pub fn handle_startup() -> bool {
    let args: Vec<String> = std::env::args().collect();

    if is_native_host_mode(&args) {
        let code = native_host::run_stdio();
        std::process::exit(code);
    }

    // 外部下载请求：--download <url> [--filename <name>] [--folder <dir>]
    if let Some(req) = parse_download_args(&args) {
        // 已运行实例优先：转发成功后本进程直接退出（单实例语义）
        if ipc::try_forward(&json!({
            "action": "download",
            "url": req.0,
            "filename": req.1,
            "folder": req.2,
        })) {
            return true;
        }
        // 没有已运行实例：写入本实例队列，继续正常启动，窗口出现时前端取走
        push_pending(json!({
            "url": req.0,
            "filename": req.1,
            "folder": req.2,
            "source": "cli",
            "at": now_ms(),
        }));
    }

    false
}

/// 判断本次进程是否应作为「原生消息宿主」运行（只跑 stdio，不开窗口）。
///
/// 要认两条路径：
///   ① 显式 `--native-host`：手动调试、或注册表 path 指向 .cmd 转发器时
///   ② Chromium 的真实调用形态：从宿主清单启动时，浏览器把「扩展来源」作为
///      唯一命令行参数传进来（形如 `chrome-extension://<id>/`）。
///      漏掉这条会导致「浏览器一点下载，启动器主界面就被拉起来」。
pub fn is_native_host_mode(args: &[String]) -> bool {
    if args.iter().any(|a| a == "--native-host") {
        return true;
    }
    // 只看第一个业务参数：exe 自身路径在 args[0]
    args.get(1)
        .map(|a| a.starts_with("chrome-extension://") || a.starts_with("moz-extension://"))
        .unwrap_or(false)
}

/// 解析 --download 系列参数，返回 (url, filename, folder)
fn parse_download_args(args: &[String]) -> Option<(String, String, String)> {
    let mut url = String::new();
    let mut name = String::new();
    let mut folder = String::new();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].as_str();
        if key == "--download" || key == "--filename" || key == "--folder" {
            let value = args.get(i + 1).cloned().unwrap_or_default();
            match key {
                "--download" => url = value,
                "--filename" => name = value,
                "--folder" => folder = value,
                _ => {}
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    if url.is_empty() {
        return None;
    }
    Some((url, name, folder))
}

/// 毫秒时间戳
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn native_host_mode_accepts_explicit_flag() {
        assert!(is_native_host_mode(&args(&["VersePC2.exe", "--native-host"])));
    }

    #[test]
    fn native_host_mode_accepts_chromium_origin_argument() {
        // Chromium 从宿主清单启动进程时，传的是扩展来源而非 --native-host
        assert!(is_native_host_mode(&args(&[
            "VersePC2.exe",
            "chrome-extension://pfmgdfekkaaigjjigfgljcmnebbhlfoa/"
        ])));
        assert!(is_native_host_mode(&args(&[
            "VersePC2.exe",
            "moz-extension://11111111-2222-3333-4444-555555555555/"
        ])));
    }

    #[test]
    fn native_host_mode_ignores_normal_launch() {
        assert!(!is_native_host_mode(&args(&["VersePC2.exe"])));
        assert!(!is_native_host_mode(&args(&["VersePC2.exe", "--download", "https://a/b"])));
        // 只有第一个业务参数算数，后面的巧合字符串不该误判
        assert!(!is_native_host_mode(&args(&[
            "VersePC2.exe",
            "--download",
            "chrome-extension://x/"
        ])));
    }

    #[test]
    fn parse_download_args_reads_all_three_flags() {
        let a = args(&[
            "VersePC2.exe",
            "--download",
            "https://example.com/a.zip",
            "--filename",
            "a.zip",
            "--folder",
            r"D:\Downloads",
        ]);
        let (url, name, folder) = parse_download_args(&a).expect("应解析出参数");
        assert_eq!(url, "https://example.com/a.zip");
        assert_eq!(name, "a.zip");
        assert_eq!(folder, r"D:\Downloads");
    }

    #[test]
    fn parse_download_args_allows_missing_optional_flags() {
        let a = args(&["VersePC2.exe", "--download", "https://example.com/b.jar"]);
        let (url, name, folder) = parse_download_args(&a).expect("url 存在即可");
        assert_eq!(url, "https://example.com/b.jar");
        assert!(name.is_empty());
        assert!(folder.is_empty());
    }

    #[test]
    fn parse_download_args_returns_none_without_url() {
        let a = args(&["VersePC2.exe", "--filename", "x.zip"]);
        assert!(parse_download_args(&a).is_none());
        assert!(parse_download_args(&[]).is_none());
    }

    #[test]
    fn parse_download_args_ignores_trailing_flag_without_value() {
        let a = args(&["VersePC2.exe", "--download", "https://example.com/c"]);
        let (url, name, _) = parse_download_args(&a).unwrap();
        assert_eq!(url, "https://example.com/c");
        assert!(name.is_empty());
    }

    #[test]
    fn pending_queue_is_fifo_and_bounded() {
        // 队列是全局静态，测试里只验证行为契约，结束前清空
        while drain_pending().is_some() {}

        push_pending(json!({ "n": 1 }));
        push_pending(json!({ "n": 2 }));
        assert_eq!(pending_count(), 2);
        assert_eq!(drain_pending().unwrap()["n"], 1);
        assert_eq!(drain_pending().unwrap()["n"], 2);
        assert!(drain_pending().is_none());

        // 超过上限时丢弃最旧的
        for i in 0..20 {
            push_pending(json!({ "n": i }));
        }
        assert!(pending_count() <= 16);
        while drain_pending().is_some() {}
    }
}
