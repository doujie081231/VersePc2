// modloaders/optifine.rs — OptiFine 加载器版本查询与安装
// 职责：
//   1. 从 optifine.net 解析 HTML 拉取指定 MC 版本的 OptiFine 版本列表
//   2. 下载 OptiFine installer JAR 并创建独立版本目录
//
// 路由：
//   GET  /api/optifine/versions?game=1.20.1
//   POST /api/optifine/install            body: { gameVersion, optifineType, targetVersionId? }
//
// 安装策略：
//   - 主路径：从 installer JAR 中读取 version.json，合并 OptiFine 库后写入版本目录
//   - 降级路径：installer 内无 version.json，手动构建 fallback JSON（launchwrapper + tweakClass）

use serde_json::{json, Value};
use std::path::PathBuf;

use super::shared;

/// OptiFine 下载页 URL
const OPTIFINE_DOWNLOADS_URL: &str = "https://optifine.net/downloads";
/// OptiFine 镜像下载入口（页内含真实 downloadx 连接）
const OPTIFINE_ADLOAD_URL: &str = "https://optifine.net/adloadx?f=";
/// BMCLAPI OptiFine 版本列表
const OPTIFINE_LIST_BMCLAPI: &str = "https://bmclapi2.bangbang93.com/optifine/versionList";

/// 拉取 OptiFine 页面文本（带浏览器 UA，optifine.net 会拦截非浏览器请求）
async fn fetch_optifine_text(url: &str) -> Result<String, String> {
    let client = shared::shared_client();
    let resp = client
        .get(url)
        .header("User-Agent", crate::download::mirror::BROWSER_UA)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status));
    }
    resp.text().await.map_err(|e| format!("读取响应失败: {}", e))
}

/// 从 BMCLAPI 解析指定 MC 版本的 OptiFine 版本列表
async fn optifine_versions_from_bmclapi(game_version: &str) -> Option<Vec<Value>> {
    let text = shared::fetch_text(OPTIFINE_LIST_BMCLAPI, 20).await.ok()?;
    let arr: Value = serde_json::from_str(&text).ok()?;
    let arr = arr.as_array()?;
    let mut out: Vec<Value> = Vec::new();
    for tok in arr {
        if shared::jstr(tok, "mcversion") != game_version {
            continue;
        }
        let ftype = shared::jstr(tok, "type");
        let patch = shared::jstr(tok, "patch");
        let fname = shared::jstr(tok, "filename");
        if fname.is_empty() || (ftype.is_empty() && patch.is_empty()) {
            continue;
        }
        let token = if patch.is_empty() {
            ftype.clone()
        } else {
            format!("{}_{}", ftype, patch)
        };
        out.push(json!({
            "version": token,
            "gameVersion": game_version,
            "fileName": fname,
        }));
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 从官方下载页解析指定 MC 版本的 OptiFine 版本列表
async fn optifine_versions_from_official(game_version: &str) -> Option<Vec<Value>> {
    let url = format!("{}?f={}", OPTIFINE_DOWNLOADS_URL, game_version);
    let html = fetch_optifine_text(&url).await.ok()?;
    let re = regex::Regex::new(r"OptiFine_([0-9A-Za-z_.]+)\.jar").ok()?;
    let mut out: Vec<Value> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let prefix = format!("{}_", game_version);
    for cap in re.captures_iter(&html) {
        let name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let token = match name.strip_prefix(&prefix) {
            Some(t) => t,
            None => continue,
        };
        if token.is_empty() || !seen.insert(token.to_string()) {
            continue;
        }
        // preview 文件名带 preview_ 前缀
        let fname = format!(
            "{}OptiFine_{}.jar",
            if token.contains("pre") { "preview_" } else { "" },
            name
        );
        out.push(json!({
            "version": token,
            "gameVersion": game_version,
            "fileName": fname,
        }));
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 获取指定 MC 版本的 OptiFine 版本列表
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
///
/// # 返回
/// Vec<{version, gameVersion, fileName}>（version 形如 "HD_U_I5"、"HD_U_I5_pre4"）
pub async fn get_optifine_versions(game_version: &str) -> Vec<Value> {
    // BMCLAPI 源优先，官方源兜底
    if let Some(v) = optifine_versions_from_bmclapi(game_version).await {
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(mut v) = optifine_versions_from_official(game_version).await {
        v.truncate(12);
        return v;
    }
    eprintln!("[OptiFine] 获取版本列表失败（双源均不可用），返回空列表");
    Vec::new()
}

// ============== OptiFine 安装 ==============
//
// 实现策略：
//   1. 下载 OptiFine installer JAR（同时它也是 OptiFine 主库本体）
//   2. 主路径：从 installer JAR 中读取 version.json，作为版本配置基础
//   3. 降级路径：installer 内无 version.json 时，手动构建 fallback JSON
//      （依赖 launchwrapper + --tweakClass optifine.OptiFineTweaker）
//   4. 复制 installer JAR 到 libraries/optifine/OptiFine/<game>_<type>/OptiFine-<game>_<type>.jar
//   5. 在版本 libraries 中追加 optifine:OptiFine:<game>_<type> 条目
//   6. 写入版本 JSON

/// 解析 OptiFine 的真实下载地址。
/// OptiFine 官方不允许直接下载固定地址：需先请求 adloadx 镜像页，
/// 从页内提取携带一次性 token 的 `downloadx?f=<file>&x=<token>` 地址，再用它下载。
async fn resolve_optifine_download_url(filename: &str) -> Result<String, String> {
    let mirror_url = format!("{}{}", OPTIFINE_ADLOAD_URL, filename);
    let html = fetch_optifine_text(&mirror_url)
        .await
        .map_err(|e| format!("获取 OptiFine 镜像页失败: {}", e))?;
    if html.len() < 200 {
        return Err("镜像页内容过短".to_string());
    }
    let pat = format!(
        r#"downloadx\?f={}(?:&|&amp;)[^'"<]*?x=[0-9a-fA-F]+"#,
        regex::escape(filename)
    );
    let re = regex::Regex::new(&pat).map_err(|e| format!("正则编译失败: {}", e))?;
    let m = re
        .find(&html)
        .ok_or_else(|| "镜像页中未找到 downloadx 下载地址".to_string())?;
    Ok(format!("https://optifine.net/{}", &html[m.start()..m.end()]))
}

/// 安装 OptiFine 模组加载器
///
/// # 参数
/// - `game_version`: Minecraft 版本号，如 "1.20.1"
/// - `optifine_type`: OptiFine 类型版本，如 "HD_U_Z"（默认 "HD_U_Z"）
/// - `target_version_id`: 可选的目标版本目录名
///
/// # 返回
/// JSON 对象：{ success, versionId?, error? }
pub async fn install_optifine(
    game_version: &str,
    optifine_type: &str,
    target_version_id: Option<&str>,
) -> Value {
    // 1. 参数规范化（必须从版本列表选择真实版本，不默认伪造版本号）
    if optifine_type.is_empty() {
        return json!({ "success": false, "error": "缺少 OptiFine 版本号，请从版本列表中选择" });
    }
    let optifine_type = optifine_type.to_string();
    let default_version_id = format!("OptiFine_{}_{}", game_version, optifine_type);
    let version_id = target_version_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_version_id.clone());

    eprintln!(
        "[OptiFine] 安装 game={} type={} versionId={}",
        game_version, optifine_type, version_id
    );

    // 2. 下载 installer JAR（不单独安装原版，下方统一合并）
    let installer_dir = shared::data_dir().join("temp");
    let installer_path = installer_dir.join(format!(
        "optifine-installer-{}-{}.jar",
        game_version, optifine_type
    ));
    if let Err(e) = std::fs::create_dir_all(&installer_dir) {
        return json!({ "success": false, "error": format!("无法创建临时目录: {}", e) });
    }

    let is_preview = optifine_type.contains("pre");
    // OptiFine 下载文件名：预览版带 preview_ 前缀
    let jar_filename = format!(
        "{}{}_{}_{}.jar",
        if is_preview { "preview_" } else { "" },
        "OptiFine",
        game_version,
        optifine_type
    );
    let download_url = match resolve_optifine_download_url(&jar_filename).await {
        Ok(u) => u,
        Err(e) => {
            let _ = std::fs::remove_file(&installer_path);
            return json!({ "success": false, "error": format!("获取 OptiFine 下载地址失败: {}", e) });
        }
    };

    eprintln!("[OptiFine] 下载 installer: {}", download_url);
    if let Err(e) = crate::download::single::download_single(
        &download_url,
        &installer_path,
        None,
        None,
        300,
    )
    .await
    {
        return json!({ "success": false, "error": format!("OptiFine installer 下载失败: {}", e) });
    }

    // 验证文件
    let installer_meta = match std::fs::metadata(&installer_path) {
        Ok(m) => m,
        Err(e) => {
            return json!({ "success": false, "error": format!("installer 文件不存在: {}", e) });
        }
    };
    if installer_meta.len() < 64 * 1024 {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": "OptiFine installer 文件过小（可能下载失败）" });
    }
    if !shared::verify_zip_magic(&installer_path) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": "OptiFine installer 不是有效 ZIP 文件" });
    }
    eprintln!(
        "[OptiFine] installer 下载成功 ({} bytes)",
        installer_meta.len()
    );

    // 4. 尝试从 installer 中读取 version.json 或 <versionId>.json
    let mut version_json: Option<Value> = None;
    let mut launchwrapper_entries: Vec<(String, Vec<u8>)> = Vec::new();

    match std::fs::File::open(&installer_path) {
        Ok(file) => {
            match zip::ZipArchive::new(file) {
                Ok(mut archive) => {
                    for i in 0..archive.len() {
                        if let Ok(mut entry) = archive.by_index(i) {
                            let name = entry.name().to_string();
                            // 主路径：寻找 version.json 或 <versionId>.json
                            if name == "version.json"
                                || name == format!("{}.json", default_version_id)
                                || name == format!("{}.json", version_id)
                            {
                                let mut buf = Vec::with_capacity(entry.size() as usize);
                                if std::io::copy(&mut entry, &mut buf).is_ok() {
                                    if let Ok(content) = String::from_utf8(buf) {
                                        if let Ok(json) = serde_json::from_str::<Value>(&content)
                                        {
                                            version_json = Some(json);
                                            eprintln!("[OptiFine] 从 installer 中读取 {}", name);
                                        }
                                    }
                                }
                            }
                            // 降级路径所需：提取 launchwrapper-*.jar
                            if name.starts_with("launchwrapper") && name.ends_with(".jar") {
                                let mut buf = Vec::with_capacity(entry.size() as usize);
                                if std::io::copy(&mut entry, &mut buf).is_ok() {
                                    launchwrapper_entries.push((name.clone(), buf));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[OptiFine] ZIP 解压失败: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("[OptiFine] 打开 installer 失败: {}", e);
        }
    }

    // 6. 准备 OptiFine 库文件路径
    let of_lib_rel = format!(
        "optifine/OptiFine/{}_{}/OptiFine-{}_{}.jar",
        game_version, optifine_type, game_version, optifine_type
    );
    let of_lib_path = shared::libraries_dir().join(&of_lib_rel);
    let of_lib_name = format!("optifine:OptiFine:{}_{}", game_version, optifine_type);

    // 复制 installer JAR 到 libraries 目录
    if let Some(parent) = of_lib_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[OptiFine] 创建库目录失败: {}", e);
            }
        }
    }
    if let Err(e) = std::fs::copy(&installer_path, &of_lib_path) {
        let _ = std::fs::remove_file(&installer_path);
        return json!({ "success": false, "error": format!("复制 OptiFine 库失败: {}", e) });
    }
    eprintln!("[OptiFine] 已复制到 libraries: {}", of_lib_path.display());

    // 7. 根据是否存在 version.json 选择主路径或降级路径，构造加载器 JSON
    let loader_json: Value = if let Some(mut profile) = version_json.take() {
        // 兼容旧格式：minecraftArguments → arguments
        if let Some(obj) = profile.as_object_mut() {
            if obj.get("arguments").is_none() {
                if let Some(mc_args) = obj.get("minecraftArguments").and_then(|v| v.as_str()) {
                    let game_args: Vec<String> = mc_args.split(' ').map(|s| s.to_string()).collect();
                    obj.insert(
                        "arguments".to_string(),
                        json!({ "game": game_args, "jvm": [] }),
                    );
                }
            }
            obj.remove("inheritsFrom");
            obj.remove("jar");
        }

        // 追加 OptiFine 库（如不存在）
        if let Some(libs) = profile.get_mut("libraries").and_then(|l| l.as_array_mut()) {
            let already = libs.iter().any(|l| {
                l.get("name").and_then(|n| n.as_str()).map(|n| n == of_lib_name).unwrap_or(false)
            });
            if !already {
                libs.push(json!({
                    "name": of_lib_name,
                    "downloads": {
                        "artifact": {
                            "path": of_lib_rel
                        }
                    }
                }));
            }
        } else {
            // 没有 libraries 字段时创建
            if let Some(obj) = profile.as_object_mut() {
                obj.insert(
                    "libraries".to_string(),
                    json!([{
                        "name": of_lib_name,
                        "downloads": { "artifact": { "path": of_lib_rel } }
                    }]),
                );
            }
        }

        profile
    } else {
        // ===== 降级路径 =====
        // 从 installer 中提取 launchwrapper-* 到 libraries/net/minecraft/launchwrapper/1.12/
        for (lw_name, lw_data) in &launchwrapper_entries {
            let lw_dest = shared::libraries_dir()
                .join("net")
                .join("minecraft")
                .join("launchwrapper")
                .join("1.12")
                .join(lw_name);
            if !lw_dest.exists() {
                if let Some(parent) = lw_dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&lw_dest, lw_data).is_err() {
                    eprintln!("[OptiFine] 提取 launchwrapper 失败: {}", lw_name);
                } else {
                    eprintln!("[OptiFine] 已提取 launchwrapper: {}", lw_dest.display());
                }
            }
        }

        json!({
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "libraries": [
                {
                    "name": "net.minecraft:launchwrapper:1.12",
                    "downloads": {
                        "artifact": {
                            "path": "net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar",
                            "url": "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
                        }
                    }
                },
                {
                    "name": of_lib_name,
                    "downloads": { "artifact": { "path": of_lib_rel } }
                }
            ],
            "minecraftArguments": "--tweakClass optifine.OptiFineTweaker"
        })
    };

    // 8. 清理 installer
    let _ = std::fs::remove_file(&installer_path);

    // 9. 与对应原版合并，产出单一独立版本（自含原版内容，删除 inheritsFrom）
    match shared::install_merged_loader(game_version, &version_id, &loader_json, None).await {
        Ok(_) => {
            eprintln!("[OptiFine] 合并式安装完成: {}", version_id);
            // 清理不再被引用的原版目录（合并后目标版本自含，不遗留独立原版目录）
            if !game_version.is_empty() && game_version != version_id {
                shared::cleanup_orphan_vanilla(game_version);
            }
            json!({
                "success": true,
                "versionId": version_id
            })
        }
        Err(e) => {
            eprintln!("[OptiFine] 安装失败: {}", e);
            json!({ "success": false, "error": e })
        }
    }
}
