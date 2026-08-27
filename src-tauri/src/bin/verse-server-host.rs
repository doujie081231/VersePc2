// verse-server-host — 独立开服守护进程入口
// 用法：verse-server-host --data-dir <主程序数据目录> [--port <端口>]

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut data_dir: Option<PathBuf> = None;
    let mut port: u16 = 27310;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--data-dir" => {
                i += 1;
                if i < args.len() {
                    data_dir = Some(PathBuf::from(&args[i]));
                }
            }
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().unwrap_or(port);
                }
            }
            "--help" | "-h" => {
                eprintln!("用法: verse-server-host --data-dir <路径> [--port <端口>]");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }

    let Some(dd) = data_dir else {
        eprintln!("缺少 --data-dir 参数，无法确定共享数据目录");
        std::process::exit(1);
    };

    if let Err(e) = verse_tauri_lib::server_host_daemon::run(dd, port).await {
        eprintln!("[verse-server-host] 启动失败: {}", e);
        std::process::exit(1);
    }
}