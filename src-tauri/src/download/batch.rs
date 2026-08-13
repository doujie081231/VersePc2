// download/batch.rs — 批量小文件下载
// 职责：针对资源文件（assets/objects）数量多、体积小的特点，提供专用批量下载。
// 与单文件下载不同，这里一次性选择最优源，避免对每个小文件重复测速和解析 URL。

use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

use crate::download::single::{compute_sha1, HTTP_CLIENT};

const ASSET_PROBE_TIMEOUT: u64 = 3;
const ASSET_DOWNLOAD_TIMEOUT: u64 = 30;

/// 单个资源对象
pub struct AssetObject {
    pub name: String,
    pub hash: String,
    pub size: u64,
}

/// 为资源文件选择最佳下载源。
/// 资源文件数量多、体积小，逐文件测速会产生大量额外请求拖慢整体速度，
/// 因此在开始下载前只探测一次，后续所有文件共用同一组源。
pub async fn select_asset_sources(download_source: &str) -> Vec<String> {
    let official = "https://resources.download.minecraft.net/".to_string();
    let bmclapi = "https://bmclapi2.bangbang93.com/assets/".to_string();

    let mut candidates: Vec<String> = Vec::with_capacity(2);
    match download_source {
        "mojang" => candidates.push(official),
        "china-first" => {
            candidates.push(bmclapi);
            candidates.push(official);
        }
        _ => {
            candidates.push(official);
            candidates.push(bmclapi);
        }
    }

    if candidates.len() <= 1 {
        return candidates;
    }

    let mut probes = Vec::with_capacity(candidates.len());
    for base in &candidates {
        let client = HTTP_CLIENT.clone();
        let base = base.clone();
        probes.push(async move {
            let start = Instant::now();
            // assets 的 hash 前两位是子目录，用不可能存在的 hash 探测连通性即可
            let probe_url = format!("{}00/0000000000000000000000000000000000000000", base);
            match client
                .head(&probe_url)
                .timeout(Duration::from_secs(ASSET_PROBE_TIMEOUT))
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 200 || status == 404 {
                        (base, start.elapsed().as_millis() as u64)
                    } else {
                        (base, u64::MAX)
                    }
                }
                Err(_) => (base, u64::MAX),
            }
        });
    }

    let mut results = futures_util::future::join_all(probes).await;
    results.sort_by_key(|(_, ms)| *ms);
    results.into_iter().map(|(url, _)| url).collect()
}

/// 批量下载资源对象。
/// 返回 (成功数, 失败数)。每完成一个文件调用一次 on_progress(done, total, current_name)。
pub async fn download_asset_objects<F>(
    objects: Vec<AssetObject>,
    assets_dir: &Path,
    base_urls: &[String],
    max_parallel: usize,
    on_progress: F,
) -> (usize, usize)
where
    F: Fn(usize, usize, &str) + Send + Sync + 'static,
{
    if objects.is_empty() || base_urls.is_empty() {
        return (0, 0);
    }

    let completed = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total = objects.len();
    let semaphore = Arc::new(Semaphore::new(max_parallel.max(1)));
    let base_urls: Arc<Vec<String>> = Arc::new(base_urls.to_vec());
    let on_progress = Arc::new(on_progress);
    let assets_dir = assets_dir.to_path_buf();

    let mut tasks = tokio::task::JoinSet::new();

    for obj in objects {
        let permit = semaphore.clone().acquire_owned().await.ok();
        let completed = completed.clone();
        let failed = failed.clone();
        let base_urls = base_urls.clone();
        let on_progress = on_progress.clone();
        let assets_dir = assets_dir.clone();

        tasks.spawn(async move {
            let _permit = permit;
            let prefix = &obj.hash[..2.min(obj.hash.len())];
            let target_dir = assets_dir.join("objects").join(prefix);
            let target_path = target_dir.join(&obj.hash);

            // 已存在且 SHA1 正确则跳过
            if target_path.exists() {
                let mut hash_ok = false;
                if let Ok(meta) = tokio::fs::metadata(&target_path).await {
                    if obj.size == 0 || meta.len() == obj.size {
                        if let Ok(actual) = compute_sha1(&target_path).await {
                            if actual.to_lowercase() == obj.hash.to_lowercase() {
                                hash_ok = true;
                            }
                        }
                    }
                }
                if hash_ok {
                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                    on_progress(done, total, &obj.name);
                    return;
                }
            }

            let _ = tokio::fs::create_dir_all(&target_dir).await;

            let mut downloaded = false;
            for base in base_urls.iter() {
                let url = format!("{}{}/{}", base, prefix, obj.hash);
                match download_asset_once(&url, &target_path, ASSET_DOWNLOAD_TIMEOUT).await {
                    Ok(()) => {
                        if let Ok(actual) = compute_sha1(&target_path).await {
                            if actual.to_lowercase() == obj.hash.to_lowercase() {
                                downloaded = true;
                                break;
                            }
                        }
                        let _ = tokio::fs::remove_file(&target_path).await;
                    }
                    Err(_) => {
                        let _ = tokio::fs::remove_file(&target_path).await;
                        let _ = tokio::fs::remove_file(&target_path.with_extension("downloading")).await;
                    }
                }
            }

            if downloaded {
                let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                on_progress(done, total, &obj.name);
            } else {
                failed.fetch_add(1, Ordering::SeqCst);
                let done = completed.load(Ordering::SeqCst);
                on_progress(done, total, &obj.name);
            }
        });
    }

    while let Some(_) = tasks.join_next().await {}

    (completed.load(Ordering::SeqCst), failed.load(Ordering::SeqCst))
}

/// 下载单个资源文件到临时文件，完成后重命名。
async fn download_asset_once(url: &str, dest: &Path, timeout_secs: u64) -> Result<(), String> {
    let client = &*HTTP_CLIENT;
    let tmp_path = dest.with_extension("downloading");

    let response = client
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = response.status().as_u16();
    if status == 429 {
        return Err("HTTP 429 限流".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("创建文件失败: {}", e))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取数据失败: {}", e))?;
        file.write_all(&chunk).await.map_err(|e| format!("写入失败: {}", e))?;
    }

    file.flush().await.map_err(|e| format!("刷新失败: {}", e))?;
    drop(file);

    tokio::fs::rename(&tmp_path, dest)
        .await
        .map_err(|e| format!("重命名失败: {}", e))?;

    Ok(())
}
