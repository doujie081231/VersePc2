use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let original = "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar";
    let dest_path = std::path::Path::new("target/probe_forge_installer.jar");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .unwrap();

    // 1) resolve_final_url
    let resp = client.get(original).send().await.unwrap();
    let final_url = resp.url().to_string();
    println!("[1] final_url = {}", &final_url[..final_url.len().min(80)]);

    // 2) 分块下载（复刻 chunked::download_chunked_with_mirror 对单个 URL 的逻辑）
    let dest2 = dest_path.with_extension("jar.chunk");
    let _ = std::fs::remove_file(&dest2);
    let t = Instant::now();
    let url = final_url.clone();
    let res = full_chunked(&client, &url, dest2.as_path()).await;
    println!("[2] chunked result: {:?} 耗时 {:?}", res, t.elapsed());
    if let Ok(true) = &res {
        println!("    chunked file size = {}", std::fs::metadata(&dest2).map(|m| m.len()).unwrap_or(0));
    }

    // 3) 单流下载
    let dest3 = dest_path.with_extension("jar.single");
    let _ = std::fs::remove_file(&dest3);
    let t = Instant::now();
    let res3 = full_single(&client, &final_url, dest3.as_path()).await;
    println!("[3] single result: {:?} 耗时 {:?}", res3, t.elapsed());
    if res3.is_ok() {
        println!("    single file size = {}", std::fs::metadata(&dest3).map(|m| m.len()).unwrap_or(0));
    }
}

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

async fn full_chunked(client: &reqwest::Client, url: &str, dest: &std::path::Path) -> Result<bool, String> {
    // probe
    let resp = client.get(url).header("Range", "bytes=0-0").send().await
        .map_err(|e| format!("probe fail: {}", e))?;
    let status = resp.status().as_u16();
    let (size, supports_range) = if status == 206 {
        let size = resp.headers().get("content-range").and_then(|v| v.to_str().ok())
            .and_then(|s| s.split('/').nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
        (size, true)
    } else if status == 200 {
        (resp.content_length().unwrap_or(0), false)
    } else {
        return Err(format!("probe HTTP {}", status));
    };
    println!("    probe size={} supports_range={}", size, supports_range);
    if !supports_range || size < 1024 * 1024 {
        return Ok(false);
    }
    let chunk_count = ((size / (512 * 1024)).max(1) as usize).min(64).max(1);
    let base = size / chunk_count as u64;
    let mut handles = Vec::new();
    let bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    for i in 0..chunk_count as u64 {
        let c = client.clone();
        let u = url.to_string();
        let d = dest.with_extension(format!("c{}", i));
        let bytes = bytes.clone();
        let (s, e) = (
            i * base,
            if i == chunk_count as u64 - 1 { size - 1 } else { (i + 1) * base - 1 },
        );
        handles.push(tokio::spawn(async move {
            let r = c.get(&u).header("Range", format!("bytes={}-{}", s, e)).send().await
                .map_err(|e| format!("chunk {} req fail: {}", i, e))?;
            if r.status().as_u16() != 206 {
                return Err(format!("chunk {} HTTP {}", i, r.status()));
            }
            let mut file = tokio::fs::File::create(&d).await.map_err(|e| e.to_string())?;
            let mut stream = r.bytes_stream();
            while let Some(ch) = stream.next().await {
                let ch = ch.map_err(|e| e.to_string())?;
                file.write_all(&ch).await.map_err(|e| e.to_string())?;
                bytes.fetch_add(ch.len() as u64, std::sync::atomic::Ordering::SeqCst);
            }
            file.flush().await.map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        }));
    }
    let mut first_err = None;
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => { if first_err.is_none() { first_err = Some(e); } }
            Err(e) => { if first_err.is_none() { first_err = Some(format!("panic {}", e)); } }
        }
    }
    if let Some(e) = first_err { return Err(e); }
    // merge
    let mut merged = tokio::fs::File::create(dest).await.map_err(|e| e.to_string())?;
    for i in 0..chunk_count as u64 {
        let d = dest.with_extension(format!("c{}", i));
        let data = tokio::fs::read(&d).await.map_err(|e| e.to_string())?;
        merged.write_all(&data).await.map_err(|e| e.to_string())?;
        let _ = tokio::fs::remove_file(&d).await;
    }
    merged.flush().await.map_err(|e| e.to_string())?;
    println!("    chunked total written = {}", bytes.load(std::sync::atomic::Ordering::SeqCst));
    Ok(true)
}

async fn full_single(client: &reqwest::Client, url: &str, dest: &std::path::Path) -> Result<u64, String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut total = 0u64;
    while let Some(ch) = stream.next().await {
        let ch = ch.map_err(|e| e.to_string())?;
        file.write_all(&ch).await.map_err(|e| e.to_string())?;
        total += ch.len() as u64;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(total)
}