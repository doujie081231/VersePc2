// download/chunked.rs — 多线程分块下载
// 职责：并发 Range 请求分块下载大文件，突破单连接限速，对齐原项目多线程下载
// 对应原项目 server/http-client/download-chunked.js

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use futures_util::StreamExt;

use super::single::{compute_sha1, DownloadProgress, ProgressCb};

/// 分块临时文件路径：在目标文件名后追加 `.cN`（如 mod.jar.c0），不覆盖原扩展名
fn chunk_path(dest: &Path, idx: usize) -> PathBuf {
    let mut name = dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    name.push_str(&format!(".c{}", idx));
    dest.with_file_name(name)
}

/// 分块阈值：大于该字节数才启用分块（原项目 1MB）
const CHUNK_THRESHOLD: u64 = 1024 * 1024;
/// 最小分块大小（原项目 512KB）
const MIN_CHUNK_SIZE: u64 = 512 * 1024;
/// 最大并发分块数（原项目上限 64，实际受 maxChunksPerFile 设置约束）
const MAX_CHUNKS: usize = 64;
/// 单块 stall 超时（秒）：对齐 XMCL stallTimeout 默认 30s
const CHUNK_STALL_SECS: u64 = 30;
/// 初始并发分块数（对齐原项目 _MAX_INITIAL_THREADS=4：前 4 个分块不受速度限制）
const INITIAL_CHUNKS: usize = 4;
/// 速度下限（字节/秒）：低于此值才新增分块并发（对齐原项目 _speedFloor 初始 256KB/s）
const SPEED_FLOOR_BASE: u64 = 256 * 1024;

/// 全局连接预算：限制所有文件的分块连接总数，避免多个大文件各开满并发导致
/// 连接爆炸（如 64 mod × 64 分块 = 4096 连接），从而触发 CDN 限流反而拖慢速度。
/// 对齐 XMCL 引擎思路：连接池充足到能并行下载多个大文件，但不至于无限并发。
/// 默认 256（对齐原项目 DownloadManager 的 connectionLimit 上限）。
fn global_conn_budget() -> usize {
    crate::storage::load_settings()
        .get("downloadConcurrency")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(256)
        .clamp(4, 256)
}
static GLOBAL_CONN: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(global_conn_budget()));

/// 动态并发调度器（对齐原项目 download-chunked.js 的 P2-10/P2-11 线程决策逻辑）：
/// - 初始 4 个分块并发，不受速度限制（保证基础并发）
/// - 滑动窗口采样实际下载速度（每 200ms）
/// - 速度下限 floor = max(256KB/s, 实际平均速度 * 0.85)，随速度动态调整
/// - 仅当当前速度低于下限时才允许新增分块并发（上限 max_chunks）
/// - 若所有分块都已结束，必须允许启动新的（避免卡死）
///
/// 效果：网络快就保持少量并发不堆连接（避免触发 CDN 限流）；网络慢/单连接被限速
/// 时才逐步增加并发去"碰运气"提速，直到速度达标或到上限。
struct ChunkScheduler {
    launched: AtomicUsize,  // 已启动的分块任务总数
    active: AtomicUsize,    // 正在下载数据的分块数
    max_chunks: usize,
    // 滑动窗口：上次采样累计字节、上次采样时刻、计算出的瞬时速度
    window: Mutex<(u64, Instant, u64)>,
    bytes_done: Arc<AtomicU64>, // 引用全局累计下载字节，用于计算速度
}

impl ChunkScheduler {
    fn new(max_chunks: usize, bytes_done: Arc<AtomicU64>) -> Self {
        Self {
            launched: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            max_chunks: max_chunks.max(INITIAL_CHUNKS),
            window: Mutex::new((0, Instant::now(), 0)),
            bytes_done,
        }
    }

    /// 更新滑动窗口速度（每 200ms 采样一次），返回当前瞬时速度（字节/秒）
    fn sample_speed(&self) -> u64 {
        let now = Instant::now();
        let bytes = self.bytes_done.load(Ordering::SeqCst);
        let mut w = self.window.lock().unwrap();
        let (prev_bytes, prev_time, _) = *w;
        if now.duration_since(prev_time).as_millis() >= 200 {
            let dt = now.duration_since(prev_time).as_millis().max(1) as u64;
            let speed = ((bytes - prev_bytes) * 1000) / dt;
            *w = (bytes, now, speed);
            speed
        } else {
            w.2
        }
    }

    /// 当前是否应启动新的分块任务（对齐原项目 _shouldAddThread）
    fn should_add(&self) -> bool {
        let launched = self.launched.load(Ordering::SeqCst);
        // 前 INITIAL_CHUNKS 个分块不受速度限制，保证基础并发
        if launched < INITIAL_CHUNKS {
            return true;
        }
        // 没有正在下载的分块时必须启动新的，否则会卡死（前一批已全部完成）
        if self.active.load(Ordering::SeqCst) == 0 {
            return true;
        }
        // 已达上限不再新增
        if launched >= self.max_chunks {
            return false;
        }
        // 速度下限 = max(256KB/s, 实际平均速度 * 0.85)
        let speed = self.sample_speed();
        let floor = SPEED_FLOOR_BASE.max(speed * 85 / 100);
        // 速度达标：不再新增并发，避免无谓堆连接触发限流
        speed >= floor
    }

    fn mark_launched(&self) {
        self.launched.fetch_add(1, Ordering::SeqCst);
    }

    fn mark_active_start(&self) {
        self.active.fetch_add(1, Ordering::SeqCst);
    }

    fn mark_active_end(&self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 探测文件大小与 Range 支持
/// 返回 (file_size, supports_range)
async fn probe(client: &reqwest::Client, url: &str, timeout_secs: u64) -> Result<(u64, bool), String> {
    // 探测用独立短超时（最多 15 秒），避免服务器不响应头时长时间卡住
    let probe_timeout = Duration::from_secs(timeout_secs.min(15).max(5));
    let resp = client
        .get(url)
        .header("Range", "bytes=0-0")
        .timeout(probe_timeout)
        .send()
        .await
        .map_err(|e| format!("探测请求失败: {}", e))?;

    let status = resp.status().as_u16();
    if status == 206 {
        let size = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok((size, true))
    } else if status == 200 {
        let size = resp.content_length().unwrap_or(0);
        Ok((size, false))
    } else {
        Err(format!("HTTP {}", status))
    }
}

/// 计算分块数：均匀切分，上限 MAX_CHUNKS
fn calc_chunk_count(file_size: u64, max_chunks: usize) -> usize {
    let by_size = (file_size / MIN_CHUNK_SIZE).max(1) as usize;
    by_size.min(max_chunks).max(1)
}

/// 多线程分块下载单个 URL
/// 仅当文件支持 Range 且大于阈值时使用；否则返回 None 让上层走单流
async fn download_chunked_once(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha1: Option<&str>,
    expected_size: Option<u64>,
    timeout_secs: u64,
    max_chunks: usize,
    cancel: Option<&Arc<AtomicBool>>,
    on_progress: Option<ProgressCb>,
) -> Result<bool, String> {
    // 探测
    let (file_size, supports_range) = match probe(client, url, timeout_secs).await {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let file_size = if file_size > 0 { file_size } else {
        expected_size.unwrap_or(0)
    };
    if file_size <= 0 {
        return Ok(false); // 拿不到大小，回退单流
    }
    if !supports_range || file_size < CHUNK_THRESHOLD {
        return Ok(false); // 不支持 Range 或文件太小，回退单流
    }

    // 分块
    let chunk_count = calc_chunk_count(file_size, max_chunks);
    let base_size = file_size / chunk_count as u64;
    let mut chunks: Vec<(u64, u64)> = Vec::with_capacity(chunk_count);
    for i in 0..chunk_count as u64 {
        let s = i * base_size;
        let e = if i == (chunk_count as u64 - 1) {
            file_size - 1
        } else {
            (i + 1) * base_size - 1
        };
        chunks.push((s, e));
    }

    // 确保父目录存在
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败: {}", e))?;
    }

    // 进度聚合状态
    let bytes_done = Arc::new(AtomicU64::new(0));
    let start_time = Instant::now();
    let last_report = Arc::new(std::sync::Mutex::new(Instant::now()));
    let failed = Arc::new(AtomicBool::new(false));
    // 动态并发调度器：初始 4 起步，速度不够才加，上限 max_chunks
    let scheduler = Arc::new(ChunkScheduler::new(max_chunks, bytes_done.clone()));

    // 下载分块，写入 dest.c{i}
    // 每个分块任务先等待调度器允许启动（对齐原项目 _safeDlChunk 的 _shouldAddThread），
    // 允许后再占连接下载，实现"4 起步 + 速度不够才加并发"的动态线程池。
    let mut handles = Vec::with_capacity(chunk_count);
    for (idx, (s, e)) in chunks.into_iter().enumerate() {
        let client = client.clone();
        let url = url.to_string();
        let tmp_path = chunk_path(dest, idx);
        let bytes_done = bytes_done.clone();
        let failed = failed.clone();
        let cancel = cancel.cloned();
        let on_progress = on_progress.clone();
        let last_report = last_report.clone();
        let file_size = file_size;
        let scheduler = scheduler.clone();

        handles.push(tokio::spawn(async move {
            // 等待调度器允许启动（每次检查间隔 50ms，与原项目 while 轮询一致）
            loop {
                if scheduler.should_add() {
                    break;
                }
                if failed.load(Ordering::SeqCst) {
                    return Ok::<(), String>(());
                }
                if let Some(c) = &cancel {
                    if c.load(Ordering::SeqCst) {
                        return Err("已取消".to_string());
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            scheduler.mark_launched();
            scheduler.mark_active_start();
            // 用守卫确保下载结束后标记 active 结束（无论成功失败）
            struct ActiveGuard(Arc<ChunkScheduler>);
            impl Drop for ActiveGuard {
                fn drop(&mut self) {
                    self.0.mark_active_end();
                }
            }
            let _guard = ActiveGuard(scheduler);
            // 全局连接预算：所有文件的分块连接共享，避免连接爆炸触发 CDN 限流
            // 加超时：32 并发大文件下载时连接预算可能被占满，若长时间等不到许可就放弃
            // 该分块（让文件级重试换源），避免所有分块无限排队导致整体"卡住"。
            let _global_perm = match tokio::time::timeout(Duration::from_secs(120), GLOBAL_CONN.acquire()).await {
                Ok(Ok(p)) => p,
                _ => return Err("等待全局连接预算超时".to_string()),
            };
            if failed.load(Ordering::SeqCst) {
                return Ok::<(), String>(());
            }
            if let Some(c) = &cancel {
                if c.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }

            // 续传：检查已下载部分
            let mut start = s;
            if let Ok(meta) = tokio::fs::metadata(&tmp_path).await {
                let existing = meta.len();
                if existing >= (e - s + 1) {
                    bytes_done.fetch_add(e - s + 1, Ordering::SeqCst);
                    return Ok(());
                }
                start = s + existing;
            }

            let mut req = client.get(&url).header("Range", format!("bytes={}-{}", start, e));
            req = req.timeout(Duration::from_secs(timeout_secs));

            // TTFB 超时保护：分块请求同样可能卡在等待响应头上（Modrinth CDN 跳转后
            // 慢速响应），必须及时中断，避免分块长期挂起导致"下载卡住"。
            let resp = match tokio::time::timeout(
                Duration::from_secs(20),
                req.send(),
            )
            .await
            {
                Ok(Ok(r)) => r,
                Ok(Err(err)) => {
                    failed.store(true, Ordering::SeqCst);
                    return Err(format!("分块 {} 请求失败: {}", idx, err));
                }
                Err(_) => {
                    failed.store(true, Ordering::SeqCst);
                    return Err(format!("分块 {} TTFB 超时，切换镜像", idx));
                }
            };
            if resp.status().as_u16() != 206 {
                failed.store(true, Ordering::SeqCst);
                return Err(format!("分块 {} HTTP {}", idx, resp.status()));
            }

            // 追加模式打开
            let mut file = match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&tmp_path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    failed.store(true, Ordering::SeqCst);
                    return Err(format!("打开分块文件失败: {}", e));
                }
            };

            let mut stream = resp.bytes_stream();
            let mut finished = false;
            // 记录本分块实际写入的字节数，用于校验是否下载完整
            let mut written: u64 = 0;
            // 用超时包裹每次读取：服务器滴漏（慢速挤数据）时也能及时中断，
            // 避免循环永远出不来导致"一直下载中"
            while !finished {
                let waited = tokio::time::timeout(
                    Duration::from_secs(CHUNK_STALL_SECS),
                    stream.next(),
                )
                .await;
                let chunk_result = match waited {
                    Ok(Some(r)) => r,
                    Ok(None) => {
                        // 服务器提前结束：校验本分块是否已写满，防止下载不完整却被当成成功
                        let expected_len = e - start + 1;
                        if written < expected_len {
                            failed.store(true, Ordering::SeqCst);
                            return Err(format!(
                                "分块 {} 数据不完整: 期望 {} 字节, 实际 {} 字节",
                                idx, expected_len, written
                            ));
                        }
                        finished = true;
                        break;
                    }
                    Err(_) => {
                        failed.store(true, Ordering::SeqCst);
                        return Err(format!("分块 {} stall 超时", idx));
                    }
                };
                if let Some(c) = &cancel {
                    if c.load(Ordering::SeqCst) {
                        failed.store(true, Ordering::SeqCst);
                        return Err("已取消".to_string());
                    }
                }
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        failed.store(true, Ordering::SeqCst);
                        return Err(format!("分块 {} 读取失败: {}", idx, e));
                    }
                };
                if let Err(e) = file.write_all(&chunk).await {
                    failed.store(true, Ordering::SeqCst);
                    return Err(format!("分块 {} 写入失败: {}", idx, e));
                }
                written += chunk.len() as u64;
                bytes_done.fetch_add(chunk.len() as u64, Ordering::SeqCst);

                // 进度上报（200ms 节流）
                let now = Instant::now();
                let mut lr = last_report.lock().unwrap();
                if now.duration_since(*lr).as_millis() > 200 {
                    let elapsed = now.duration_since(start_time).as_millis().max(1) as u64;
                    let done = bytes_done.load(Ordering::SeqCst);
                    let speed = (done * 1000) / elapsed;
                    if let Some(cb) = &on_progress {
                        cb(&DownloadProgress {
                            bytes_downloaded: done,
                            total_bytes: file_size,
                            speed,
                        });
                    }
                    *lr = now;
                }
                drop(lr);
            }

            file.flush().await.map_err(|e| format!("刷新分块失败: {}", e))?;
            drop(file);
            Ok(())
        }));
    }

    // 等待所有分块完成
    let mut first_err: Option<String> = None;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(format!("分块任务异常: {}", e));
                }
            }
        }
    }

    if let Some(e) = first_err {
        return Err(e);
    }

    // 合并分块
    let mut merged = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("创建目标文件失败: {}", e))?;
    for idx in 0..chunk_count {
        let tmp_path = chunk_path(dest, idx);
        let data = tokio::fs::read(&tmp_path).await.map_err(|e| format!("读取分块 {} 失败: {}", idx, e))?;
        merged.write_all(&data).await.map_err(|e| format!("合并分块 {} 失败: {}", idx, e))?;
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
    merged.flush().await.map_err(|e| format!("刷新文件失败: {}", e))?;
    drop(merged);

    // 校验大小：用合并后文件的真实大小，而非探针值，避免下载不完整却被当成成功
    let actual_size = tokio::fs::metadata(dest).await.map_err(|e| format!("获取文件大小失败: {}", e))?.len();
    if file_size > 0 && actual_size != file_size {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(format!("分块合并后大小不匹配: 期望 {} 实际 {}", file_size, actual_size));
    }
    if let Some(expected) = expected_size {
        if expected > 0 && actual_size != expected {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(format!("大小不匹配: 期望 {} 实际 {}", expected, actual_size));
        }
    }

    // 校验 SHA1
    if let Some(expected_sha1) = sha1 {
        if !expected_sha1.is_empty() {
            let actual = compute_sha1(dest).await.map_err(|e| e)?;
            if actual.to_lowercase() != expected_sha1.to_lowercase() {
                let _ = tokio::fs::remove_file(dest).await;
                return Err("SHA1 校验失败".to_string());
            }
        }
    }

    // 最终进度
    if let Some(cb) = &on_progress {
        cb(&DownloadProgress {
            bytes_downloaded: file_size,
            total_bytes: file_size,
            speed: 0,
        });
    }

    Ok(true)
}

/// 多线程分块下载（带镜像回退）
/// 返回 true 表示成功；false/Err 表示无法分块或失败，上层决定回退单流
pub async fn download_chunked_with_mirror(
    urls: &[String],
    dest: &Path,
    sha1: Option<&str>,
    expected_size: Option<u64>,
    timeout_secs: u64,
    max_chunks: usize,
    cancel: Option<&Arc<AtomicBool>>,
    on_progress: Option<ProgressCb>,
) -> Result<bool, String> {
    // 复用全局 HTTP 客户端（共享连接池），避免多个文件/分块重复 TLS 握手
    let client = &*super::single::HTTP_CLIENT;

    let mut last_err = String::new();
    for url in urls {
        // 会话内坏源直接跳过
        if super::mirror::is_bad_host(url) {
            continue;
        }
        // 同一源续传重试：单个分块临时失败（请求中断/stall/不完整）时先在同一快源
        // 重试（分块文件保留可续传），避免轻易掉到下一个慢源、导致大文件尾部卡住。
        // 只有"源本身速度过低"才立即换源。
        for attempt in 0..3usize {
            if let Some(c) = cancel {
                if c.load(Ordering::SeqCst) {
                    return Err("已取消".to_string());
                }
            }
            let handled = download_chunked_once(
                &client,
                url,
                dest,
                sha1,
                expected_size,
                timeout_secs,
                max_chunks,
                cancel,
                on_progress.clone(),
            )
            .await;
            match handled {
                Ok(true) => {
                    super::mirror::clear_bad_host(url);
                    return Ok(true);
                }
                Ok(false) => return Ok(false), // 不支持分块，交给单流
                Err(e) => {
                    last_err = e.clone();
                    // 源本身慢（低速检测）或 TTFB 超时（响应头迟迟不回）→ 立即换源；
                    // 否则为临时失败，续传重试
                    if e.contains("速度过低") || e.contains("TTFB 超时") {
                        break;
                    }
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(500 + attempt as u64 * 500)).await;
                        continue;
                    }
                }
            }
        }
        // 多次仍失败，清理并换源
        super::mirror::mark_bad_host(url);
        let _ = tokio::fs::remove_file(dest).await;
        for i in 0..MAX_CHUNKS {
            let _ = tokio::fs::remove_file(chunk_path(dest, i)).await;
        }
    }
    Err(last_err)
}