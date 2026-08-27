// system.rs — 运行环境检查
// 职责：检查 WebView2 内核是否安装，缺失则在默认浏览器打开官方安装程序
// 说明：Tauri 打包为 WebView2 运行时可能未内置，首次运行需确保系统已装 WebView2。

use serde_json::{json, Value};

/// WebView2 Runtime 的注册表客户端 ID（EdgeUpdate Clients 键）
const WEBVIEW2_CLIENT_ID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
/// WebView2 官方引导安装程序地址（Evergreen Runtime）
const WEBVIEW2_INSTALLER_URL: &str =
    "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// 检测 WebView2 Runtime 是否已安装
/// 直接用 ws-winreg 读注册表（毫秒级），避免每次启动都 spawn reg.exe 子进程
/// （子进程被安全软件实时扫描时会拖慢启动好几秒）
#[cfg(target_os = "windows")]
pub fn webview2_installed() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let subpaths = [
        format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_CLIENT_ID}"),
        format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_CLIENT_ID}"),
    ];
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for p in &subpaths {
        if hklm.open_subkey(p).is_ok() {
            return true;
        }
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(&subpaths[1]).is_ok()
}

/// 检测 WebView2 Runtime 是否已安装（非 Windows 平台直接视为已安装）
#[cfg(not(target_os = "windows"))]
pub fn webview2_installed() -> bool {
    true
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