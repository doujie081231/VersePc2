// install/stages/libraries.rs — 依赖库阶段
// 职责：合并继承版本的 libraries 清单，并发下载缺失库，统计失败数量。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use serde_json::Value;

use crate::download;
use crate::install::context::{
    append_install_log, calc_progress, log_now, InstallCtx, StageError,
};
use crate::install::session;

/// 执行依赖库阶段
pub(crate) async fn run(ctx: &InstallCtx, version_details: &Value) -> Result<(), StageError> {
    ctx.update(session::InstallStage::Libraries, 0, "下载依赖库...");

    let libraries = merge_inherited_libraries(version_details, &ctx.versions_dir);
    let valid_libs: Vec<Value> = libraries.into_iter().filter(evaluate_rules).collect();
    let total_libs = valid_libs.len() as u32;

    let completed_libs = Arc::new(AtomicU32::new(0));
    let failed_libs = Arc::new(AtomicU32::new(0));

    let lib_results: Vec<_> = stream::iter(valid_libs)
        .map(|lib| {
            let libraries_dir = ctx.libraries_dir.clone();
            let download_source = ctx.download_source.clone();
            let app = ctx.app.clone();
            let session_id = ctx.session_id.clone();
            let completed = completed_libs.clone();
            let failed = failed_libs.clone();
            let cancel_flag = ctx.cancel.clone();
            let data_dir_for_log = ctx.data_dir.clone();
            async move {
                if session::is_cancelled(&cancel_flag) {
                    return;
                }
                let name = lib
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let done = completed.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                session::update_session(&app, &session_id, |s| {
                    s.progress = calc_progress(
                        &session::InstallStage::Libraries,
                        (done * 100) / total_libs.max(1),
                    );
                    s.message = format!("下载库 ({}/{})", done, total_libs);
                    s.current_file = name.clone();
                    s.completed_files = done;
                    s.total_files = total_libs;
                });
                if let Err(e) =
                    download_library(&lib, &libraries_dir, &download_source, &app, &session_id).await
                {
                    failed.fetch_add(1, AtomicOrdering::SeqCst);
                    append_install_log(
                        &data_dir_for_log,
                        &format!("[{}] 库下载失败: {} — {}", log_now(), name, e),
                    );
                    session::update_session(&app, &session_id, |s| {
                        s.errors.push(format!("库 {} 下载失败: {}", name, e));
                    });
                }
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    drop(lib_results);

    // 取消同样清理未完成的版本目录（仅限本次新建的目录）
    if ctx.is_cancelled() {
        ctx.cleanup_new_version_dir();
        append_install_log(
            &ctx.data_dir,
            &format!("[{}] 安装取消: {}", log_now(), ctx.actual_version_id),
        );
        return Err(ctx.cancelled());
    }

    let failed_total = failed_libs.load(AtomicOrdering::SeqCst);
    if failed_total > 0 {
        let fail_msg = format!("依赖库下载失败 {} 个，请检查网络或稍后重试", failed_total);
        ctx.cleanup_new_version_dir();
        append_install_log(
            &ctx.data_dir,
            &format!(
                "[{}] 安装失败: {} — {}",
                log_now(),
                ctx.actual_version_id,
                fail_msg
            ),
        );
        ctx.update(session::InstallStage::Libraries, 100, &fail_msg);
        return Err(ctx.fail(fail_msg, None));
    }

    ctx.update(session::InstallStage::Libraries, 100, "依赖库已下载");
    if ctx.is_cancelled() {
        return Err(ctx.cancelled());
    }
    Ok(())
}

/// 递归合并版本及其继承版本（inheritsFrom）的 libraries，供依赖库下载使用。
/// 顺序：子版本在前、父版本在后；按 name 去重，保留先出现的（子版本优先）。
fn merge_inherited_libraries(json: &Value, versions_dir: &std::path::Path) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_libraries(json, versions_dir, &mut out, &mut seen, &mut visited);
    out
}

/// 自底向下递归收集 libraries（子在前、父在后）
fn collect_libraries(
    json: &Value,
    versions_dir: &std::path::Path,
    out: &mut Vec<Value>,
    seen: &mut std::collections::HashSet<String>,
    visited: &mut std::collections::HashSet<String>,
) {
    if let Some(libs) = json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            if let Some(name) = lib.get("name").and_then(|v| v.as_str()) {
                if seen.insert(name.to_string()) {
                    out.push(lib.clone());
                }
            } else {
                out.push(lib.clone());
            }
        }
    }
    if let Some(parent_id) = json.get("inheritsFrom").and_then(|v| v.as_str())
        && visited.insert(parent_id.to_string())
        && let Some(parent) = load_local_version_json(parent_id, versions_dir)
    {
        collect_libraries(&parent, versions_dir, out, seen, visited);
    }
}

/// 从本地版本目录读取某版本的 json
fn load_local_version_json(version_id: &str, versions_dir: &std::path::Path) -> Option<Value> {
    let path = versions_dir
        .join(version_id)
        .join(format!("{}.json", version_id));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 评估库的 rules（平台兼容性）
fn evaluate_rules(lib: &Value) -> bool {
    let rules = match lib.get("rules").and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return true, // 无 rules 默认允许
    };

    let os_name = current_os_name();

    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(os) = rule.get("os").and_then(|v| v.as_object()) {
            let rule_os = os.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if rule_os == os_name {
                allowed = action == "allow";
            }
        } else {
            // 无 os 限制的规则
            allowed = action == "allow";
        }
    }
    allowed
}

/// 当前平台 OS 名（与版本 JSON natives 规则中的键一致）
fn current_os_name() -> &'static str {
    crate::platform::rules_os_name()
}

/// 解析库的当前平台 native classifier 条目：
/// 取 lib.natives[os]（展开 ${arch} 为 64/32）后查 downloads.classifiers。
/// 1.7.10 的 twitch/lwjgl-platform/jinput-platform 等库只有 classifiers、没有 artifact。
fn native_classifier_target<'a>(lib: &'a Value, os_name: &str, arch: &str) -> Option<&'a Value> {
    let key = lib
        .get("natives")?
        .as_object()?
        .get(os_name)?
        .as_str()?
        .replace("${arch}", arch);
    lib.pointer("/downloads/classifiers")?.get(&key)
}

/// 按 downloads 节点（artifact 或 classifier 条目）下载到 libraries 目录
async fn download_downloads_node(
    node: &Value,
    libraries_dir: &PathBuf,
    download_source: &str,
) -> Result<(), String> {
    let url = node.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let path = node.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let sha1 = node.get("sha1").and_then(|v| v.as_str()).unwrap_or("");
    let size = node.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    if url.is_empty() || path.is_empty() {
        return Ok(());
    }
    let dest = libraries_dir.join(path);
    // 安全检查：路径不能逃出 libraries_dir
    if !dest.starts_with(libraries_dir) {
        return Err("路径越界".to_string());
    }
    download::download_with_mirror(
        url,
        &dest,
        if sha1.is_empty() { None } else { Some(sha1) },
        if size > 0 { Some(size) } else { None },
        download_source,
        120,
        None,
    )
    .await
}

/// 下载单个库文件
async fn download_library(
    lib: &Value,
    libraries_dir: &PathBuf,
    download_source: &str,
    _app: &tauri::AppHandle,
    _session_id: &str,
) -> Result<(), String> {
    let os_name = current_os_name();
    let arch = crate::platform::pointer_width_suffix();

    // 优先用 downloads.artifact
    if let Some(artifact) = lib.pointer("/downloads/artifact") {
        let url = artifact.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let path = artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");

        if url.is_empty() || path.is_empty() {
            // artifact 节点无 URL：若存在当前平台的 native classifier 则下载它，否则跳过
            match native_classifier_target(lib, os_name, arch) {
                Some(nd) => {
                    return download_downloads_node(nd, libraries_dir, download_source).await;
                }
                None => return Ok(()),
            }
        }

        download_downloads_node(artifact, libraries_dir, download_source).await?;

        // 主 jar 之外，有 natives 规则的库还需下载对应 native classifier（启动期解压用）
        if let Some(nd) = native_classifier_target(lib, os_name, arch) {
            return download_downloads_node(nd, libraries_dir, download_source).await;
        }
        return Ok(());
    }

    // 无 artifact：有 natives 规则时下载对应 native classifier
    if let Some(nd) = native_classifier_target(lib, os_name, arch) {
        return download_downloads_node(nd, libraries_dir, download_source).await;
    }
    // classifiers 存在但当前平台无可用条目，跳过
    if lib.pointer("/downloads/classifiers").is_some() {
        return Ok(());
    }

    // 无 artifact/classifiers，按 maven 坐标构造 URL
    let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if name.is_empty() {
        return Ok(());
    }
    // Maven 名称格式：group:artifact:version[:classifier][@extension]
    // 必须保留 @ 后的扩展名（Forge 安装器库里有 @zip），否则会拼成 ...@zip.jar 导致 404。
    let Some((group, rel_path)) = maven_rel_path(name) else {
        return Ok(());
    };
    let dest = libraries_dir.join(&rel_path);

    // 选择 base URL
    let base_url = if group.contains("fabric") || group.contains("fabricmc") {
        "https://maven.fabricmc.net/"
    } else if group.contains("neoforged") {
        // NeoForge 官方仓库路径带 releases 段
        "https://maven.neoforged.net/releases/"
    } else if group.contains("forge")
        || group.contains("minecraftforge")
        || group.starts_with("net.minecraft")
        || group.starts_with("de.oceanlabs.mcp")
    {
        // de.oceanlabs.mcp（mcp_config 等）同样由 Forge Maven 托管
        "https://maven.minecraftforge.net/"
    } else {
        "https://libraries.minecraft.net/"
    };
    let url = format!("{}{}", base_url, rel_path);

    download::download_with_mirror(&url, &dest, None, None, download_source, 120, None).await
}

/// 解析 Maven 名称并生成 group 与相对路径。
/// 支持 `group:artifact:version[:classifier][@extension]`，扩展名缺省为 jar。
fn maven_rel_path(name: &str) -> Option<(String, String)> {
    let (coords, extension) = match name.split_once('@') {
        Some((c, ext)) if !ext.is_empty() => (c, ext),
        _ => (name, "jar"),
    };
    let parts: Vec<&str> = coords.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0];
    let artifact_id = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 {
        format!("-{}", parts[3])
    } else {
        String::new()
    };

    let group_path = group.replace('.', "/");
    let file_name = format!("{}-{}{}.{}", artifact_id, version, classifier, extension);
    Some((
        group.to_string(),
        format!("{}/{}/{}/{}", group_path, artifact_id, version, file_name),
    ))
}

#[cfg(test)]
mod maven_path_tests {
    use super::maven_rel_path;

    #[test]
    fn plain_jar_path() {
        let (group, path) = maven_rel_path("net.minecraftforge:forge:1.20.1-47.4.18").unwrap();
        assert_eq!(group, "net.minecraftforge");
        assert_eq!(path, "net/minecraftforge/forge/1.20.1-47.4.18/forge-1.20.1-47.4.18.jar");
    }

    #[test]
    fn classifier_jar_path() {
        let (_, path) = maven_rel_path("net.minecraftforge:ForgeAutoRenamingTool:0.1.22:all").unwrap();
        assert_eq!(
            path,
            "net/minecraftforge/ForgeAutoRenamingTool/0.1.22/ForgeAutoRenamingTool-0.1.22-all.jar"
        );
    }

    #[test]
    fn zip_extension_is_preserved() {
        let (_, path) =
            maven_rel_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip").unwrap();
        assert_eq!(
            path,
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip"
        );
    }
}
