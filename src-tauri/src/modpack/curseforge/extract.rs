// extract.rs — 由 curseforge.rs 拆分
#![allow(unused_imports)]

// overrides 解压与资源下载
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::Semaphore;
use crate::modpack::curseforge_shared as cf_shared;
use super::{emit_progress, normalize_version_id, versions_dir};
use crate::modloaders::shared as ml_shared;
use crate::storage;
use super::*;

/// 解压 overrides 到版本目录
/// 含路径遍历保护、Windows 保留名过滤、5 次重试、50 文件 yield、实时进度回调
pub(crate) fn extract_overrides_with_progress(
    archive: &mut zip::ZipArchive<std::fs::File>,
    dest_dir: &Path,
    app: &AppHandle,
    prefix: &str,
) -> Result<Vec<Value>, String> {
    let dest_canonical = dest_dir
        .canonicalize()
        .unwrap_or_else(|_| dest_dir.to_path_buf());

    // 先统计待解压文件总数
    let mut override_total = 0usize;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            if name.starts_with(prefix) {
                override_total += 1;
            }
        }
    }

    let mut override_files: Vec<Value> = Vec::new();
    let mut yield_counter = 0usize;
    let mut extract_count = 0usize;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        // 统一分隔符：Windows 打包的 zip 可能使用反斜杠
        let name = entry.name().replace('\\', "/");
        if !cf_shared::is_modpack_path_safe(&name) {
            continue;
        }
        if !name.starts_with(prefix) {
            continue;
        }

        let rel_path = &name[prefix.len()..];
        let dest_path = dest_dir.join(rel_path);

        // 路径遍历校验
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            let parent_canonical = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if !parent_canonical.starts_with(&dest_canonical) {
                eprintln!("[Modpack] CurseForge路径遍历已拦截: {}", rel_path);
                continue;
            }
        }

        // 读取 entry 数据
        let mut buf = Vec::new();
        if let Err(e) = entry.read_to_end(&mut buf) {
            eprintln!("[CurseForge] 读取 entry 失败: {} - {}", rel_path, e);
            continue;
        }

        // 5 次重试写入
        let mut written = false;
        for attempt in 1..=5u32 {
            match std::fs::write(&dest_path, &buf) {
                Ok(_) => {
                    written = true;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[Modpack] CF解压 {} 第 {} 次失败: {}",
                        rel_path, attempt, e
                    );
                    if attempt < 5 {
                        std::thread::sleep(Duration::from_millis((attempt - 1) as u64 * 2000));
                    }
                }
            }
        }

        if written {
            override_files.push(json!({
                "name": rel_path,
                "status": "completed",
                "progress": 100
            }));
            extract_count += 1;
        }

        // 实时进度反馈：每 50 个文件更新一次进度（40% → 50% 区间）
        yield_counter += 1;
        if override_total > 0 && yield_counter.is_multiple_of(50) {
            let pct = 40 + (extract_count * 10 / override_total);
            emit_progress(
                app,
                pct as u32,
                &format!("解压覆盖文件... ({}/{})", extract_count, override_total),
                "extract",
            );
            // yield to event loop
            std::thread::yield_now();
        }
    }

    Ok(override_files)
}

/// 提取整合包根目录的图标文件（pack.png / icon.png / logo.png）
pub(crate) fn extract_root_icon(archive: &mut zip::ZipArchive<std::fs::File>, version_dir: &Path) {
    let icon_names = ["pack.png", "icon.png", "logo.png"];
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let entry_name = entry.name().replace('\\', "/");
        if icon_names.contains(&entry_name.as_str()) {
            let dest_icon_path = version_dir.join(&entry_name);
            if !dest_icon_path.exists() {
                let mut buf = Vec::new();
                if entry.read_to_end(&mut buf).is_ok() {
                    let _ = std::fs::write(&dest_icon_path, &buf);
                    eprintln!("[Modpack] 提取整合包图标: {}", entry_name);
                }
            }
            break;
        }
    }
}

/// 下载游戏资源索引和缺失资源
pub(crate) async fn download_assets(
    app: &AppHandle,
    asset_index: &Value,
    settings: &Value,
) -> Result<(), String> {
    let assets_dir = cf_shared::assets_dir();
    let asset_index_id = asset_index
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("assetIndex 无 id")?;
    let asset_index_url = asset_index
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("assetIndex 无 url")?;
    let asset_index_sha1 = asset_index
        .get("sha1")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let index_path = assets_dir.join("indexes").join(format!("{}.json", asset_index_id));

    // 下载索引文件
    if !index_path.exists()
        || (!asset_index_sha1.is_empty()
            && !verify_file_sha1(&index_path, asset_index_sha1).await)
    {
        if let Some(parent) = index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if index_path.exists() {
            let _ = std::fs::remove_file(&index_path);
        }
        crate::download::single::download_with_mirror(
            asset_index_url,
            &index_path,
            if asset_index_sha1.is_empty() { None } else { Some(asset_index_sha1) },
            None,
            "mojang",
            60,
            None,
        )
        .await
        .map_err(|e| format!("下载资源索引失败: {}", e))?;
    }

    if !index_path.exists() {
        return Ok(());
    }

    // 解析索引文件，收集缺失资源
    let index_content = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("读取资源索引失败: {}", e))?;
    let index_json: Value = serde_json::from_str(&index_content)
        .map_err(|e| format!("解析资源索引失败: {}", e))?;

    let objects = index_json
        .get("objects")
        .and_then(|v| v.as_object())
        .ok_or("资源索引无 objects")?;

    let mut asset_objects: Vec<crate::download::AssetObject> = Vec::with_capacity(objects.len());
    for (name, info) in objects {
        let hash = info
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let size = info
            .get("size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if hash.is_empty() {
            continue;
        }
        let sub_dir = &hash[..2];
        let a_path = assets_dir.join("objects").join(sub_dir).join(hash);
        if !a_path.exists() {
            asset_objects.push(crate::download::AssetObject {
                name: name.clone(),
                hash: hash.to_string(),
                size,
            });
        }
    }

    if asset_objects.is_empty() {
        return Ok(());
    }

    let asset_total = asset_objects.len();
    let sources = crate::download::select_asset_sources("mojang").await;
    let asset_parallel = settings
        .get("maxThreads")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(32)
        .clamp(8, 64);

    emit_progress(
        app,
        93,
        &format!("下载游戏资源 (0/{ })", asset_total),
        "assets",
    );

    let app_progress = app.clone();
    let (_done, failed) = crate::download::download_asset_objects(
        asset_objects,
        &assets_dir,
        &sources,
        asset_parallel,
        move |done, _total, name| {
            let pct = 93 + (done as u64 * 4 / asset_total as u64);
            emit_progress(
                &app_progress,
                pct.min(97) as u32,
                &format!("下载游戏资源 ({}/{ })", done, asset_total),
                "assets",
            );
            if done % 50 == 0 || done == asset_total {
                eprintln!("[CurseForge] 资源下载进度 {}/{} 当前: {}", done, asset_total, name);
            }
        },
    ).await;

    if failed > 0 {
        eprintln!("[CurseForge] {} 个资源文件下载失败", failed);
    }

    crate::download::ensure_language_assets(objects, &assets_dir, &sources, asset_parallel)
        .await?;

    emit_progress(
        app,
        97,
        &format!("游戏资源下载完成 ({}/{})", asset_total, asset_total),
        "assets",
    );

    Ok(())
}
