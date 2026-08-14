// system.rs — 运行环境检查
// 职责：检查 WebView2 内核是否安装，缺失则在默认浏览器打开官方安装程序
// 说明：Tauri 打包为 WebView2 运行时可能未内置，首次运行需确保系统已装 WebView2。

use serde_json::{json, Value};

/// WebView2 Runtime 的注册表客户端 ID（EdgeUpdate Clients 键）
const WEBVIEW2_CLIENT_ID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
/// WebView2 官方引导安装程序地址（Evergreen Runtime）
const WEBVIEW2_INSTALLER_URL: &str =
    "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// 注册表查询失败/未找到时返回 false，视为未安装
fn reg_query_exists(path: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args(["query", path])
            .output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

/// 检测 WebView2 Runtime 是否已安装
/// 通过注册表 EdgeUpdate Clients 键判断，兼顾 64/32 位视角
pub fn webview2_installed() -> bool {
    #[cfg(target_os = "windows")]
    {
        let keys = [
            format!(
                r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_CLIENT_ID}"
            ),
            format!(r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_CLIENT_ID}"),
            format!(
                r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_CLIENT_ID}"
            ),
        ];
        keys.iter().any(|k| reg_query_exists(k))
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

/// 确保 WebView2 已安装；缺失则在默认浏览器打开官方安装程序
/// 返回 (是否已安装, 是否发起了安装)
pub fn ensure_webview2() -> (bool, bool) {
    if webview2_installed() {
        return (true, false);
    }
    // 未安装：打开官方引导安装程序
    match open::that(WEBVIEW2_INSTALLER_URL) {
        Ok(_) => {
            println!("[webview2] 未检测到 WebView2 内核，已打开官方安装程序");
            (false, true)
        }
        Err(e) => {
            eprintln!("[webview2] 打开安装程序失败: {}", e);
            (false, false)
        }
    }
}

/// 供前端调用的检测命令：返回是否已安装 WebView2
#[tauri::command]
pub fn check_webview2() -> Value {
    json!({ "installed": webview2_installed() })
}