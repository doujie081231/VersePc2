// download/single.rs — 单流下载
// 职责：单连接下载文件，支持续传、SHA1 校验、低速检测、超时重试
// 对应原项目 server/http-client/download-single.js 的 _dlSingle

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sha1::{Sha1, Digest};
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;

use super::mirror;
use super::chunked;

/// 分块阈值：小于该字节数的文件视为小文件，直接单流下载（与 chunked.rs 一致）
const CHUNK_THRESHOLD: u64 = 1024 * 1024;

/// 全局复用 HTTP 客户端：让大量小文件（如 assets）共享连接池，避免每个文件都重新
/// 建立 TLS 连接导致大量握手开销。对应全局 HttpClient 复用连接池的思路。
/// 注意：不在此设置总超时，改为在单个请求上用 .timeout() 覆盖，以支持不同超时。
pub static HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15)) // TTFB 超时：15 秒内连不上就判失败换源
        .user_agent(mirror::BROWSER_UA) // 模拟浏览器 UA，绕过部分 CDN 拦截
        .danger_accept_invalid_certs(true) // 跳过证书验证：部分镜像证书链不完整
        .pool_max_idle_per_host(64)
        .build()
        .expect("创建全局 HTTP 客户端失败")
});

/// 下载进度回调类型
pub type ProgressCb = Arc<dyn Fn(&DownloadProgress) + Send + Sync>;

/// 下载进度
#[derive(Clone, Default)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed: u64,
}

/// 跟随跳转解析出真实下载地址。
/// 部分 CDN（如 edge.forgecdn.net）对带 Range 的请求直接返回 404，
/// 但对不带 Range 的请求返回 302 跳转到真实文件服务器（如 mediafilez.forgecdn.net）。
/// 分块下载依赖 Range，因此必须先解析出最终地址，再用它做探测与分块。
///
/// 手动处理重定向：reqwest 默认自动跟随重定向，但 CurseForge CDN 偶尔会返回
/// 非法 Location 头或过长的跳转链，导致 "error following redirect"。手动解析可
/// 兼容这类情况，并在失败时返回原始 URL 兜底。
async fn resolve_final_url(url: &str, timeout_secs: u64) -> String {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.min(15).max(5)))
        .danger_accept_invalid_certs(true) // 跳过证书验证：部分镜像证书链不完整
        .user_agent(mirror::BROWSER_UA) // 模拟浏览器 UA，绕过部分 CDN 拦截
        .redirect(reqwest::redirect::Policy::none()) // 手动跟随，避免自动重定向报错
        .build()
    {
        Ok(c) => c,
        Err(_) => return url.to_string(),
    };

    let mut current = url.to_string();
    for _ in 0..5 {
        let resp = match client.get(&current).send().await {
            Ok(r) => r,
            Err(_) => return url.to_string(),
        };
        let status = resp.status().as_u16();
        if (301..=308).contains(&status) {
            if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                let next = if loc.starts_with("http://") || loc.starts_with("https://") {
                    loc.to_string()
                } else if loc.starts_with('/') {
                    // 相对路径：基于当前 URL 拼 host
                    let base = current.split('/').take(3).collect::<Vec<_>>().join("/");
                    if base.starts_with("http") {
                        format!("{}{}", base, loc)
                    } else {
                        return url.to_string();
                    }
                } else {
                    // 相对文件路径，替换最后一段
                    if let Some(pos) = current.rfind('/') {
                        format!("{}{}", &current[..pos + 1], loc)
                    } else {
                        return url.to_string();
                    }
                };
                current = next;
                continue;
            }
            // 3xx 但没有 Location，返回原始 URL 兜底
            return url.to_string();
        }
        // 非 3xx，当前 URL 就是最终地址
        return current;
    }
    // 超过最大跳转次数，返回当前地址
    current
}

/// 对多个候选源并行测速（Range:0-0 探针），按延迟升序返回
/// 对应原项目 probeMirrorsParallel：延迟越低越优先，失败的超时排最后
async fn probe_speed_sort(urls: &[String], timeout_secs: u64) -> Vec<String> {
    if urls.len() <= 1 {
        return urls.to_vec();
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.min(5).max(2)))
        .danger_accept_invalid_certs(true)
        .user_agent(mirror::BROWSER_UA)
        .build()
    {
        Ok(c) => c,
        Err(_) => return urls.to_vec(),
    };

    let mut futures = Vec::with_capacity(urls.len());
    for (i, url) in urls.iter().enumerate() {
        let client = client.clone();
        let url = url.clone();
        futures.push(async move {
            let start = Instant::now();
            let resp = client
                .get(&url)
                .header("Range", "bytes=0-0")
                .send()
                .await;
            let elapsed = start.elapsed().as_millis();
            match resp {
                Ok(r) if r.status().as_u16() == 206 || r.status().as_u16() == 200 => (i, elapsed),
                _ => (i, u128::MAX), // 失败/超时排最后
            }
        });
    }
    let results = futures_util::future::join_all(futures).await;
    let mut results = results;
    results.sort_by_key(|(_, ms)| *ms);
    results.into_iter().map(|(i, _)| urls[i].clone()).collect()
}

/// 单流下载一个文件（带镜像回退 + SHA1 校验）
/// 对应原项目 downloadFileWithMirror
pub async fn download_with_mirror(
    original_url: &str,
    dest: &Path,
    sha1: Option<&str>,
    expected_size: Option<u64>,
    download_source: &str,
    timeout_secs: u64,
    on_progress: Option<ProgressCb>,
) -> Result<(), String> {
    // 文件已存在且校验通过，跳过
    if dest.exists() && should_skip(dest, sha1, expected_size).await {
        return Ok(());
    }

    // 小文件（<1MB，如资源文件 assets）直接单流下载：
    // 跳过分块 Range 探测和跳转解析，避免每个小文件都多两次网络握手导致整体极慢
    let is_small_file = expected_size
        .map(|s| s > 0 && s < CHUNK_THRESHOLD)
        .unwrap_or(false);

    // edge.forgecdn.net 会对 Range 请求返回 404，先解析出真实下载地址（mediafilez）
    // 避免分块探测失败，始终用最终地址进行下载
    // Modrinth CDN（cdn.modrinth.com/cdn-alt）直接提供文件，无需跳转解析，
    // 且其官方源在国内 TTFB 极慢（实测 >10s），跳过解析避免白白等官方源。
    let needs_resolve = (!is_small_file && !original_url.contains("cdn.modrinth.com"))
        || original_url.contains("edge.forgecdn.net")
        || original_url.contains("mediafilez.forgecdn.net");
    let base_url = if needs_resolve {
        resolve_final_url(original_url, timeout_secs).await
    } else {
        original_url.to_string()
    };
    let urls = mirror::get_mirror_urls(&base_url, download_source);

    // 坏源黑名单过滤：本次会话已失败的 host 直接跳过
    let urls: Vec<String> = urls
        .into_iter()
        .filter(|u| !mirror::is_bad_host(u))
        .collect();
    if urls.is_empty() {
        return Err(format!("所有下载源均已被标记为不可用: {}", base_url));
    }
    // 多个源时并行测速，优先最快的源
    let urls = probe_speed_sort(&urls, timeout_secs).await;

    // 大文件优先走多线程分块下载（读取设置中的并发数）
    let max_chunks = crate::storage::load_settings()
        .get("maxChunksPerFile")
        .and_then(|v| v.as_u64())
        .unwrap_or(64)
        .min(64) as usize;
    let enable_chunk = crate::storage::load_settings()
        .get("enableChunkDownload")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if enable_chunk && !is_small_file {
        match chunked::download_chunked_with_mirror(
            &urls,
            dest,
            sha1,
            expected_size,
            timeout_secs,
            max_chunks,
            None,
            on_progress.clone(),
        )
        .await
        {
            Ok(true) => {
                if urls.len() > 1 {
                    mirror::mirror_success();
                }
                return Ok(());
            }
            Ok(false) => {
                // 不支持分块，回退单流
            }
            Err(e) => {
                // 分块失败，清理后回退单流
                let _ = tokio::fs::remove_file(dest).await;
                eprintln!("[download] 分块下载失败，回退单流: {} ({})", original_url, e);
            }
        }
    }

    let mut last_err = String::new();

    for url in &urls {
        match download_with_retry(url, dest, sha1, expected_size, timeout_secs, on_progress.clone()).await {
            Ok(()) => {
                if url != original_url {
                    mirror::mirror_success();
                }
                mirror::clear_bad_host(url);
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                if url != original_url {
                    mirror::mirror_failed();
                }
                // 记录坏源，后续下载跳过该 host
                mirror::mark_bad_host(url);
                // 清理半成品
                let _ = tokio::fs::remove_file(dest).await;
                let _ = tokio::fs::remove_file(dest.with_extension("downloading")).await;
                continue;
            }
        }
    }
    Err(last_err)
}

/// 判断文件是否可跳过（大小+SHA1 校验通过）
async fn should_skip(dest: &Path, sha1: Option<&str>, expected_size: Option<u64>) -> bool {
    if let Some(expected) = expected_size {
        if expected > 0 {
            if let Ok(meta) = tokio::fs::metadata(dest).await {
                if meta.len() != expected {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    if let Some(expected_sha1) = sha1 {
        if !expected_sha1.is_empty() {
            if let Ok(actual) = compute_sha1(dest).await {
                return actual.to_lowercase() == expected_sha1.to_lowercase();
            }
            return false;
        }
    }
    true
}

/// 带重试的单流下载（3 次尝试）
async fn download_with_retry(
    url: &str,
    dest: &Path,
    sha1: Option<&str>,
    expected_size: Option<u64>,
    timeout_secs: u64,
    on_progress: Option<ProgressCb>,
) -> Result<(), String> {
    let mut last_err = String::new();

    for attempt in 0..3u32 {
        match download_once(url, dest, timeout_secs, on_progress.clone()).await {
            Ok(actual_size) => {
                // 校验大小
                if let Some(expected) = expected_size {
                    if expected > 0 && actual_size != expected {
                        last_err = format!("大小不匹配: 期望 {} 实际 {}", expected, actual_size);
                        let _ = tokio::fs::remove_file(dest).await;
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(500 + attempt as u64 * 500)).await;
                            continue;
                        }
                        continue;
                    }
                }
                // 校验 SHA1
                if let Some(expected_sha1) = sha1 {
                    if !expected_sha1.is_empty() {
                        match compute_sha1(dest).await {
                            Ok(actual_sha1) => {
                                if actual_sha1.to_lowercase() != expected_sha1.to_lowercase() {
                                    last_err = "SHA1 校验失败".to_string();
                                    let _ = tokio::fs::remove_file(dest).await;
                                    if attempt < 2 {
                                        tokio::time::sleep(Duration::from_millis(500 + attempt as u64 * 500)).await;
                                        continue;
                                    }
                                    continue;
                                }
                            }
                            Err(e) => {
                                last_err = format!("计算 SHA1 失败: {}", e);
                                if attempt < 2 {
                                    continue;
                                }
                                continue;
                            }
                        }
                    }
                }
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                if attempt < 2 {
                    // 429 限流：等待 10 秒后再试，避免立即重试再次被限流
                    if last_err.contains("429") {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(500 + attempt as u64 * 500)).await;
                    }
                }
            }
        }
    }
    Err(last_err)
}

/// 单次下载尝试
async fn download_once(
    url: &str,
    dest: &Path,
    timeout_secs: u64,
    on_progress: Option<ProgressCb>,
) -> Result<u64, String> {
    // 复用全局 HTTP 客户端（共享连接池），避免大量小文件重复 TLS 握手
    let client = &*HTTP_CLIENT;

    // 续传：检查已下载部分
    let tmp_path = dest.with_extension("downloading");
    let mut resume_offset = 0u64;
    if tmp_path.exists() {
        if let Ok(meta) = tokio::fs::metadata(&tmp_path).await {
            resume_offset = meta.len();
        }
    }

    let mut req = client.get(url).timeout(Duration::from_secs(timeout_secs));
    if resume_offset > 0 {
        req = req.header("Range", format!("bytes={}-", resume_offset));
    }

    let response = req.send().await.map_err(|e| format!("请求失败: {}", e))?;

    let status = response.status().as_u16();
    if status == 429 {
        // 限流：等待后重试（对应网络请求中 429 Too Many Requests 的处理）
        return Err("HTTP 429 限流".to_string());
    }
    if !response.status().is_success() && status != 206 {
        return Err(format!("HTTP {}", response.status()));
    }

    let total_size = if resume_offset > 0 && response.status().as_u16() == 206 {
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    } else {
        response.content_length().unwrap_or(0)
    };

    // 确保父目录存在
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败: {}", e))?;
    }

    // 续传：追加模式；否则：创建新文件
    let mut file = if resume_offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&tmp_path)
            .await
            .map_err(|e| format!("打开文件失败: {}", e))?
    } else {
        tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| format!("创建文件失败: {}", e))?
    };

    let mut stream = response.bytes_stream();
    let mut downloaded = resume_offset;
    let start_time = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_bytes = resume_offset;
    // 单流读取同样加 stall 刹车：服务器卡住/滴漏时及时中断，避免永远卡在"下载中"
    let mut finished = false;
    while !finished {
        let waited = tokio::time::timeout(Duration::from_secs(30), stream.next()).await;
        let chunk_result = match waited {
            Ok(Some(r)) => r,
            Ok(None) => {
                finished = true;
                break;
            }
            Err(_) => {
                return Err("低速检测：切换镜像".to_string());
            }
        };
        let chunk = chunk_result.map_err(|e| format!("读取数据失败: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入文件失败: {}", e))?;
        downloaded += chunk.len() as u64;

        // 进度回调（50ms 节流）
        let now = Instant::now();
        if now.duration_since(last_progress).as_millis() > 50 {
            let elapsed = now.duration_since(start_time).as_millis().max(1) as u64;
            let speed = ((downloaded - resume_offset) * 1000) / elapsed;
            if let Some(cb) = &on_progress {
                cb(&DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total_size,
                    speed,
                });
            }
            last_progress = now;
            last_bytes = downloaded;
        }
    }

    file.flush().await.map_err(|e| format!("刷新文件失败: {}", e))?;
    drop(file);

    // 重命名 .downloading → 目标文件
    tokio::fs::rename(&tmp_path, dest)
        .await
        .map_err(|e| format!("重命名文件失败: {}", e))?;

    Ok(downloaded)
}

/// 计算文件 SHA1
pub async fn compute_sha1(path: &Path) -> Result<String, String> {
    let data = tokio::fs::read(path)
        .await
        .map_err(|e| format!("读取文件失败: {}", e))?;
    let mut hasher = Sha1::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}
