// download/diag.rs — 下载诊断日志
//
// 用户反馈"下载还是慢"时，光看界面速度没法定位：到底是
//   · 走了直连（没命中镜像/代理）
//   · 分块没启用（服务器不支持 Range / 文件太小）
//   · 并发数被设置限制住
// 把这三个关键结论写进 <data>/logs/download-diag.log，一眼能看出来。

use std::io::Write;
use std::path::PathBuf;

fn log_path() -> PathBuf {
    let dir = crate::storage::resolve_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("download-diag.log")
}

/// 追加一条诊断日志（静默失败，绝不影响下载）
pub fn log(line: &str) {
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    else {
        return;
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(f, "[{}] {}", ts, line);

    // 超过 1MB 截断一次，避免长期使用无限增长
    if let Ok(meta) = f.metadata()
        && meta.len() > 1024 * 1024 {
            let _ = std::fs::remove_file(log_path());
        }
}
