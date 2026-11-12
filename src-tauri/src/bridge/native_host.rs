// bridge/native_host.rs — 原生消息宿主（Native Messaging Host）
//
// 由浏览器以 stdio 方式拉起：VersePC2.exe --native-host
// 协议：stdin/stdout 上 [4 字节小端长度][UTF-8 JSON]，无 BOM。
//
// 职责很薄：把浏览器发来的 {action:"download", url, filename} 转给首实例的命名管道，
// 并把结果写回 stdout。宿主本身不做任何下载、不碰 UI、不写文件。
//
// 之所以放进同一个 exe（而不是像 PCL 那样用 PowerShell 脚本）：
//   - 免去执行策略、编码、BOM 这些坑
//   - 进程启动从 ~400ms（PowerShell）降到 ~20ms
//   - 不破坏便携版"单 exe"承诺（只需旁边放一个 200 字节的 .cmd 转发器）

use serde_json::{json, Value};
use std::io::{Read, Write};

use super::ipc;

/// 单条消息上限 64MB（浏览器规范是 1MB 请求 / 32MB 响应，这里放宽给响应留余量）
const MAX_MESSAGE: usize = 64 * 1024 * 1024;

/// 宿主主循环。返回进程退出码。
pub fn run_stdio() -> i32 {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    // stdin 关闭（read_message 返回 None）= 浏览器断开，正常退出
    while let Some(raw) = read_message(&mut stdin) {
        let req: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                let _ = write_message(&mut stdout, &json!({
                    "ok": false,
                    "error": format!("请求不是合法 JSON: {}", e)
                }));
                continue;
            }
        };

        let resp = handle_request(&req);
        if write_message(&mut stdout, &resp).is_err() {
            break;
        }
    }
    0
}

/// 处理一条浏览器请求
fn handle_request(req: &Value) -> Value {
    let action = req.get("action").and_then(|v| v.as_str()).unwrap_or("");

    match action {
        "ping" => {
            // 探测启动器是否在运行
            match ipc::call(&json!({ "action": "ping" }), std::time::Duration::from_millis(1500)) {
                Some(r) => {
                    let mut out = r.clone();
                    if let Some(obj) = out.as_object_mut() {
                        obj.insert("ok".into(), json!(true));
                        obj.insert("host".into(), json!("verse-native-host"));
                    }
                    out
                }
                None => json!({
                    "ok": false,
                    "error": "启动器未运行，或命名管道不可达",
                    "host": "verse-native-host"
                }),
            }
        }

        "download" => {
            let url = req.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() {
                return json!({ "ok": false, "error": "缺少 url" });
            }

            let payload = json!({
                "action": "download",
                "url": url,
                "filename": req.get("filename").and_then(|v| v.as_str()).unwrap_or(""),
                "folder": req.get("folder").and_then(|v| v.as_str()).unwrap_or(""),
            });

            match ipc::call(&payload, std::time::Duration::from_millis(2500)) {
                Some(r) if r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) => r,
                Some(r) => r,
                None => {
                    // 启动器没开：冷启动它，把参数随命令行带过去，窗口出现即为填好的状态
                    match cold_start(&payload) {
                        Ok(()) => json!({
                            "ok": true,
                            "via": "cold-start",
                            "message": "已带参数启动 VersePC2，窗口出现即已填好"
                        }),
                        Err(e) => json!({ "ok": false, "error": e }),
                    }
                }
            }
        }

        // 浏览器扩展要"一键扫描本机启动器位置"：扩展没有文件系统权限，
        // 只能由宿主代劳。返回候选列表（含推荐的自身路径）。
        "scanLauncher" => super::scan::scan_launcher(),

        // 用户手动填/选了一个路径，校验它是不是真的 VersePC2.exe
        "probeLauncher" => {
            let path = req.get("path").and_then(|v| v.as_str()).unwrap_or("");
            super::scan::probe_launcher(path)
        }

        // 扩展侧选好路径后调用：把该 exe 重新注册成原生宿主，
        // 下次浏览器就会直接启动这个位置的程序。
        "useLauncher" => {
            let path = req.get("path").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            if path.is_empty() {
                json!({ "ok": false, "error": "路径为空" })
            } else {
                let checked = super::scan::probe_launcher(&path);
                let ok = checked.get("ok").and_then(|v| v.as_bool()) == Some(true);
                let has_bridge = checked
                    .get("supportsBridge")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !ok {
                    checked
                } else if !has_bridge {
                    // 坚决不把宿主指向"没有下载接管模块"的 exe（官方原版/旧版/随便一个 exe），
                    // 否则注册表改完之后一点下载就失败，用户还找不到原因。
                    json!({
                        "ok": false,
                        "error": checked.get("warning")
                            .and_then(|v| v.as_str())
                            .unwrap_or("这个 exe 里没有 VersePC2 下载接管模块，不能作为宿主"),
                        "path": checked.get("path").cloned().unwrap_or(json!(path)),
                    })
                } else {
                    let exe = checked
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&path)
                        .to_string();
                    let res = super::register::install_for(&exe, None);
                    let mut out = res;
                    if let Some(obj) = out.as_object_mut() {
                        obj.insert("ok".into(), json!(true));
                        obj.insert("launcher".into(), json!(exe));
                        obj.insert("note".into(), json!("已把宿主清单指向该路径"));
                    }
                    out
                }
            }
        }

        "getDefaultFolder" => json!({
            "ok": true,
            "folder": req.get("folder").and_then(|v| v.as_str()).unwrap_or("")
        }),

        other => json!({ "ok": false, "error": format!("未知 action: {}", other) }),
    }
}

/// 冷启动启动器（仅 Windows）
///
/// 必须脱离当前进程：宿主是浏览器用 stdio 拉起的，Chromium 会把它放进
/// Job Object 并在宿主退出时清理 job 内所有进程。用 Command::spawn 出来的
/// 子进程默认继承该 job，表现就是「启动器闪一下就没了 / 没有窗口句柄」。
/// 所以这里交给 shell（等价于用户双击），由 explorer 拉起，天然在 job 之外。
fn cold_start(payload: &Value) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|e| format!("取自身路径失败: {}", e))?;
        let url = payload.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let name = payload.get("filename").and_then(|v| v.as_str()).unwrap_or("");
        let folder = payload.get("folder").and_then(|v| v.as_str()).unwrap_or("");

        let mut params = format!("--download \"{}\"", url);
        if !name.is_empty() {
            params.push_str(&format!(" --filename \"{}\"", name));
        }
        if !folder.is_empty() {
            params.push_str(&format!(" --folder \"{}\"", folder));
        }

        if shell_execute(&exe, &params, exe.parent()) {
            return Ok(());
        }

        // 兜底：shell 不可用（极少见）时退回普通 spawn，至少把参数带过去
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--download").arg(url);
        if !name.is_empty() {
            cmd.arg("--filename").arg(name);
        }
        if !folder.is_empty() {
            cmd.arg("--folder").arg(folder);
        }
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            cmd.creation_flags(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB);
        }
        cmd.spawn().map_err(|e| format!("启动 VersePC2 失败: {}", e))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = payload;
        Err("当前平台不支持冷启动唤起".to_string())
    }
}

/// 用 ShellExecuteW 打开可执行文件（等价双击），返回是否成功。
#[cfg(target_os = "windows")]
fn shell_execute(exe: &std::path::Path, params: &str, workdir: Option<&std::path::Path>) -> bool {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> *mut core::ffi::c_void;
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let op = wide(std::ffi::OsStr::new("open"));
    let file = wide(exe.as_os_str());
    let parameters = wide(std::ffi::OsStr::new(params));
    let dir = workdir.map(|d| wide(d.as_os_str()));
    let dir_ptr = dir.as_ref().map(|d| d.as_ptr()).unwrap_or(std::ptr::null());

    // SW_SHOWNORMAL = 1
    let ret = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            dir_ptr,
            1,
        )
    };
    // 返回值 > 32 表示成功
    ret as usize > 32
}

// ============== 长度帧读写 ==============

/// 读一条消息：4 字节小端长度 + UTF-8 JSON
fn read_message(stdin: &mut std::io::Stdin) -> Option<String> {
    let mut len_buf = [0u8; 4];
    if stdin.read_exact(&mut len_buf).is_err() {
        return None;
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_MESSAGE {
        return None;
    }
    let mut buf = vec![0u8; len];
    if stdin.read_exact(&mut buf).is_err() {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// 写一条消息：4 字节小端长度 + UTF-8 JSON
fn write_message(stdout: &mut std::io::Stdout, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{\"ok\":false}".to_string());
    let bytes = text.as_bytes();
    stdout.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stdout.write_all(bytes)?;
    stdout.flush()
}
