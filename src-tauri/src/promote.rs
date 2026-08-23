// promote.rs — 提权子进程机制
// 主进程以普通权限运行，需要管理员权限的操作
// （如内存优化）通过 runas 启动一个提权子进程，经命名管道通信执行。
//
// 说明：
// - 主进程调用 run_memory_optimize()：若当前已是管理员则直接执行，否则启动提权子进程执行。
// - 提权子进程模式由命令行参数 "promote <主进程PID>" 识别，连接命名管道服务端并阻塞执行命令。
// - 管道名格式：\\.\pipe\versepc_pm@<主进程PID>

use std::ffi::c_void;

const PIPE_PREFIX: &str = r"\\.\pipe\versepc_pm@";

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
const PIPE_WAIT: u32 = 0x0000_0000;
const INVALID_PIPE_INSTANCES: u32 = 1;
const NMPWAIT_USE_DEFAULT_WAIT: u32 = 0x0000_0000;
const TOKEN_QUERY: u32 = 0x0008;
const TOKEN_ELEVATION: i32 = 20;
const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
const SW_HIDE: i32 = 0;
const MAX_PATH: usize = 1024;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn CreateNamedPipeW(
        lp_name: *const u16,
        dw_open_mode: u32,
        dw_pipe_mode: u32,
        n_max_instances: u32,
        n_out_buffer_size: u32,
        n_in_buffer_size: u32,
        n_default_time_out: u32,
        lp_security_attributes: *mut c_void,
    ) -> *mut c_void;
    fn ConnectNamedPipe(h_named_pipe: *mut c_void, lp_overlapped: *mut c_void) -> i32;
    fn ReadFile(
        h_file: *mut c_void,
        lp_buffer: *mut u8,
        n_number_of_bytes_to_read: u32,
        lp_number_of_bytes_read: *mut u32,
        lp_overlapped: *mut c_void,
    ) -> i32;
    fn WriteFile(
        h_file: *mut c_void,
        lp_buffer: *const u8,
        n_number_of_bytes_to_write: u32,
        lp_number_of_bytes_written: *mut u32,
        lp_overlapped: *mut c_void,
    ) -> i32;
    fn CloseHandle(h_object: *mut c_void) -> i32;
    fn GetLastError() -> u32;
    fn WaitNamedPipeW(lp_named_pipe_name: *const u16, n_time_out: u32) -> i32;
    fn CreateFileW(
        lp_file_name: *const u16,
        dw_desired_access: u32,
        dw_share_mode: u32,
        lp_security_attributes: *mut c_void,
        dw_creation_disposition: u32,
        dw_flags_and_attributes: u32,
        h_template_file: *mut c_void,
    ) -> *mut c_void;
    fn GetModuleFileNameW(
        lp_module_name: *const u16,
        lp_filename: *mut u16,
        n_size: u32,
    ) -> u32;
    fn OpenProcessToken(
        process_handle: *mut c_void,
        desired_access: u32,
        token_handle: *mut *mut c_void,
    ) -> i32;
    fn GetTokenInformation(
        token_handle: *mut c_void,
        token_information_class: i32,
        token_information: *mut c_void,
        token_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteExW(lp_exec_info: *mut SHELLEXECUTEINFOW) -> i32;
}

#[repr(C)]
struct TOKEN_ELEVATION_STRUCT {
    token_is_elevated: u32,
}

// ShellExecuteExW 透传结构（x64 布局）
#[repr(C)]
struct SHELLEXECUTEINFOW {
    cb_size: u32,
    f_mask: u32,
    hwnd: *mut c_void,
    lp_verb: *const u16,
    lp_file: *const u16,
    lp_parameters: *const u16,
    lp_directory: *const u16,
    n_show: i32,
    h_inst_app: *mut c_void,
    lp_id_list: *mut c_void,
    lp_class: *const u16,
    hkey_class: *mut c_void,
    dw_hot_key: u32,
    _union: u64,
    h_process: *mut c_void,
}

fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

fn current_process_id() -> u32 {
    unsafe { GetCurrentProcessId() }
}

// 判断当前进程是否以管理员权限运行
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION_STRUCT { token_is_elevated: 0 };
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TOKEN_ELEVATION,
            &mut elevation as *mut TOKEN_ELEVATION_STRUCT as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION_STRUCT>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.token_is_elevated != 0
    }
}

fn exe_path() -> String {
    unsafe {
        let mut buf = vec![0u16; MAX_PATH];
        let len = GetModuleFileNameW(std::ptr::null(), buf.as_mut_ptr(), MAX_PATH as u32);
        buf.truncate(len as usize);
        wide_to_string(&buf)
    }
}

// 主进程：以 runas 启动提权子进程（触发 UAC）
fn start_promote_process(main_pid: u32) -> bool {
    unsafe {
        let exe = to_utf16(&exe_path());
        let params = to_utf16(&format!("promote {}", main_pid));
        let verb = to_utf16("runas");
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cb_size = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.f_mask = SEE_MASK_NOCLOSEPROCESS;
        sei.lp_verb = verb.as_ptr();
        sei.lp_file = exe.as_ptr();
        sei.lp_parameters = params.as_ptr();
        sei.n_show = SW_HIDE;
        let result = ShellExecuteExW(&mut sei);
        if result == 0 {
            return false;
        }
        if !sei.h_process.is_null() {
            CloseHandle(sei.h_process);
        }
        true
    }
}

// 提权子进程模式：连接主进程管道服务端并执行命令
// 返回 true 表示应停止普通启动流程（提权进程只做后台工作）
pub fn try_run_promote_process() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "promote" {
        return false;
    }
    let main_pid: u32 = match args[2].parse() {
        Ok(v) => v,
        Err(_) => return true,
    };
    run_promote_client(main_pid);
    true
}

// 提权子进程：作为管道客户端，读取主进程命令并执行内存优化
fn run_promote_client(main_pid: u32) {
    let pipe_name = to_utf16(&format!("{}{}", PIPE_PREFIX, main_pid));
    let handle = connect_pipe_client(&pipe_name);
    let handle = match handle {
        Some(h) => h,
        None => return,
    };
    // 读取命令直到管道关闭
    loop {
        let mut buf = [0u8; 256];
        let mut read: u32 = 0;
        let ok = unsafe { ReadFile(handle, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut()) };
        if ok == 0 || read == 0 {
            break;
        }
        let cmd = String::from_utf8_lossy(&buf[..read as usize]).to_string();
        if cmd.starts_with("memswap") {
            let result = match crate::api::system::do_memory_optimize_purge() {
                Ok(()) => "OK".to_string(),
                Err(e) => format!("ERR:{}", e),
            };
            let out = result.as_bytes();
            let mut written: u32 = 0;
            unsafe {
                WriteFile(handle, out.as_ptr(), out.len() as u32, &mut written, std::ptr::null_mut());
            }
        }
    }
    unsafe { CloseHandle(handle) };
}

// 客户端连接命名管道，失败返回 None
fn connect_pipe_client(pipe_name: &[u16]) -> Option<*mut c_void> {
    unsafe {
        let handle = CreateFileW(
            pipe_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if (handle as isize) != -1 {
            return Some(handle);
        }
        // 管道可能刚创建还在等待，等待后重试
        WaitNamedPipeW(pipe_name.as_ptr(), NMPWAIT_USE_DEFAULT_WAIT);
        let handle2 = CreateFileW(
            pipe_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if (handle2 as isize) == -1 {
            None
        } else {
            Some(handle2)
        }
    }
}

// 主进程：创建管道服务端，启动提权子进程，等待结果
// 返回 Ok(()) 表示内存优化执行成功
fn run_promote_server() -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let main_pid = current_process_id();
    let pipe_name = to_utf16(&format!("{}{}", PIPE_PREFIX, main_pid));
    let handle: *mut c_void;
    unsafe {
        handle = CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            INVALID_PIPE_INSTANCES,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        );
    }
    if (handle as isize) == -1 {
        return Err(format!("创建管道失败（错误代码：{}）", unsafe { GetLastError() }));
    }

    // 后台线程等待子进程连接；用 AtomicBool 标记是否已连接，主线程轮询带超时
    let server_handle_usize = handle as usize;
    let connected = Arc::new(AtomicBool::new(false));
    let connected_clone = connected.clone();
    let _connect_thread = std::thread::spawn(move || unsafe {
        ConnectNamedPipe(server_handle_usize as *mut c_void, std::ptr::null_mut());
        connected_clone.store(true, Ordering::SeqCst);
    });

    // 启动提权子进程
    if !start_promote_process(main_pid) {
        unsafe { CloseHandle(handle) };
        return Err("提权进程启动失败，请允许管理员权限以使用内存优化".to_string());
    }

    // 等待连接，最长时间 30s（用户确认 UAC 需要时间）
    let mut waited_ms = 0u64;
    while !connected.load(Ordering::SeqCst) && waited_ms < 30_000 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        waited_ms += 100;
    }
    if !connected.load(Ordering::SeqCst) {
        unsafe { CloseHandle(handle) };
        return Err("提权子进程未连接（可能取消了管理员授权），请允许管理员权限以使用内存优化".to_string());
    }

    // 发送命令
    let mut written: u32 = 0;
    let cmd = b"memswap\n";
    let write_ok = unsafe {
        WriteFile(handle, cmd.as_ptr(), cmd.len() as u32, &mut written, std::ptr::null_mut())
    };
    if write_ok == 0 {
        unsafe { CloseHandle(handle) };
        return Err(format!("发送命令失败（错误代码：{}）", unsafe { GetLastError() }));
    }

    // 读取结果
    let mut buf = [0u8; 512];
    let mut read: u32 = 0;
    let read_ok = unsafe {
        ReadFile(handle, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut())
    };
    let result_str = if read_ok != 0 {
        String::from_utf8_lossy(&buf[..read as usize]).to_string()
    } else {
        String::new()
    };

    unsafe { CloseHandle(handle) };

    if result_str == "OK" {
        Ok(())
    } else if let Some(err) = result_str.strip_prefix("ERR:") {
        Err(err.to_string())
    } else {
        Err("提权子进程未返回有效结果".to_string())
    }
}

// 入口：执行内存优化。已管理员则直接执行，否则通过提权子进程执行。
pub fn run_memory_optimize() -> Result<(), String> {
    if is_elevated() {
        return crate::api::system::do_memory_optimize_purge();
    }
    run_promote_server()
}