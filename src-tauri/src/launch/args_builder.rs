// launch/args_builder.rs — 启动参数构建
// 职责：从版本 JSON + 设置 + 账号 构建完整的 JVM/游戏启动参数
// 对应原项目 server/launch/args-builder.js:buildLaunchArguments
//
// 设计原则：
// 1. 复用 dep_check 模块中已迁移的通用辅助函数（find_external_root、find_main_jar、
//    evaluate_rules、select_java_for_version、inspect_java_version 等）
// 2. 本模块内实现 args-builder.js 专属逻辑（replace_variables、deduplicate_game_args、
//    build_classpath、extract_natives_dir、resolve_version_isolation 等）
// 3. 后续重构方向：把通用辅助函数抽离到 launch/common.rs 或 versions.rs 模块

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::storage;
use crate::utils;

use super::dep_check;
use super::memory::{
    resolve_max_memory, resolve_memory_mode, resolve_min_memory, MemoryMode,
};

/// 启动参数构建结果
pub struct LaunchArguments {
    /// 完整启动参数列表（JVM 参数 + 主类 + 游戏参数）
    pub args: Vec<String>,
    /// 决策出的最大内存（MB）
    pub max_mem_mb: u64,
}

/// 构建 Minecraft 启动参数
/// 对应原项目 args-builder.js:buildLaunchArguments
///
/// `external_version_dir`：外部版本目录路径（非外部版本传 None）
/// `custom_game_dir`：自定义游戏目录（覆盖默认决策）
pub fn build_launch_arguments(
    version_json: &Value,
    settings: &Value,
    account: &Value,
    version_id: &str,
    custom_game_dir: Option<&str>,
    external_version_dir: Option<&Path>,
) -> LaunchArguments {
    let actual_version_id = if !version_id.is_empty() {
        version_id.to_string()
    } else {
        utils::get_str(version_json, "id")
    };
    let is_external = external_version_dir.is_some();
    let external_root = if is_external {
        external_version_dir
            .and_then(dep_check::find_external_root)
            .or_else(|| {
                external_version_dir
                    .and_then(|d| d.parent())
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
            })
    } else {
        None
    };

    let data_dir = storage::resolve_data_dir();
    let versions_dir = data_dir.join("versions");
    let libraries_dir = data_dir.join("libraries");
    let assets_dir = data_dir.join("assets");

    // ============== Classpath 与 Natives ==============
    let classpath = build_classpath(version_json, external_version_dir, &libraries_dir);
    let natives_dir = extract_natives_dir(version_json, &actual_version_id, external_version_dir, &data_dir);

    // ============== 游戏目录决策 ==============
    let game_dir = resolve_game_dir(
        &actual_version_id,
        custom_game_dir,
        external_version_dir,
        settings,
        &versions_dir,
        &data_dir,
    );
    // 预创建子目录
    ensure_game_dirs(&game_dir);

    // 复制 Forge log4j2.xml（如存在）
    copy_forge_log4j(&actual_version_id, &versions_dir, &game_dir);

    // ============== Assets 根 ==============
    let mut assets_root = if is_external && external_root.is_some() {
        external_root
            .as_ref()
            .map(|r| r.join("assets"))
            .unwrap_or_else(|| assets_dir.clone())
    } else {
        assets_dir.clone()
    };
    let asset_index = version_json
        .get("assetIndex")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or(&actual_version_id)
        .to_string();
    // 旧版虚拟资源目录
    if version_json
        .get("assetIndex")
        .and_then(|v| v.get("virtual"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let virtual_dir = assets_root.join("virtual").join("legacy");
        if virtual_dir.exists() {
            assets_root = virtual_dir;
        }
    }

    // ============== 账号信息 ==============
    let player_name = utils::get_str(account, "username");
    let player_name = if player_name.is_empty() {
        "Player".to_string()
    } else {
        player_name
    };
    let raw_access_token = utils::get_str(account, "accessToken");
    let uuid = if let Some(u) = account.get("uuid").and_then(|v| v.as_str()) {
        if u.is_empty() {
            utils::offline_uuid(&player_name)
        } else {
            u.to_string()
        }
    } else {
        utils::offline_uuid(&player_name)
    };
    // 离线账户生成兼容格式的伪令牌，避免 BootstrapLauncher Base64 解析崩溃
    let is_offline_token = raw_access_token.is_empty() || raw_access_token == "0";
    let access_token = if is_offline_token {
        generate_offline_access_token(&uuid, &player_name)
    } else {
        raw_access_token
    };
    let user_type = match utils::get_str(account, "type").as_str() {
        "microsoft" => "msa".to_string(),
        "legacy" => "legacy".to_string(),
        _ => "mojang".to_string(),
    };

    // ============== Main JAR ==============
    let mut main_jar_path = dep_check::find_main_jar(version_json, &actual_version_id, external_version_dir)
        .unwrap_or_else(|| {
            versions_dir
                .join(&actual_version_id)
                .join(format!("{}.jar", actual_version_id))
        });
    // mainJar 是空文件时回退到 Forge patched client.jar
    main_jar_path = fallback_empty_main_jar(
        &main_jar_path,
        version_json,
        &actual_version_id,
        &libraries_dir,
    );

    // ============== 变量替换字典 ==============
    let library_directory = if is_external && external_root.is_some() {
        external_root
            .as_ref()
            .map(|r| r.join("libraries"))
            .unwrap_or_else(|| libraries_dir.clone())
    } else {
        libraries_dir.clone()
    };
    let cp_separator = if cfg!(target_os = "windows") { ";" } else { ":" };
    let mut variables: HashMap<String, String> = HashMap::new();
    variables.insert("auth_player_name".to_string(), player_name.clone());
    variables.insert("version_name".to_string(), actual_version_id.clone());
    variables.insert("game_directory".to_string(), game_dir.to_string_lossy().to_string());
    variables.insert("assets_root".to_string(), assets_root.to_string_lossy().to_string());
    variables.insert("assets_index_name".to_string(), asset_index.clone());
    variables.insert("auth_uuid".to_string(), uuid.clone());
    variables.insert("auth_access_token".to_string(), access_token.clone());
    variables.insert("user_type".to_string(), user_type.clone());
    variables.insert(
        "version_type".to_string(),
        format!("VersePC - {}", actual_version_id),
    );
    let resolution = utils::get_str(settings, "resolution");
    let (res_w, res_h) = parse_resolution(&resolution);
    variables.insert("resolution_width".to_string(), res_w.to_string());
    variables.insert("resolution_height".to_string(), res_h.to_string());
    variables.insert(
        "library_directory".to_string(),
        library_directory.to_string_lossy().to_string(),
    );
    variables.insert("classpath_separator".to_string(), cp_separator.to_string());
    variables.insert(
        "natives_directory".to_string(),
        natives_dir.to_string_lossy().to_string(),
    );
    variables.insert("launcher_name".to_string(), "VersePC".to_string());
    variables.insert("launcher_version".to_string(), "1.0.0".to_string());
    variables.insert("classpath".to_string(), classpath.join(cp_separator));
    variables.insert("clientid".to_string(), uuid.clone());
    variables.insert("auth_xuid".to_string(), uuid.clone());
    variables.insert(
        "quickPlayPath".to_string(),
        game_dir.join("quickPlay").to_string_lossy().to_string(),
    );
    variables.insert("quickPlaySingleplayer".to_string(), String::new());
    variables.insert("quickPlayMultiplayer".to_string(), String::new());
    variables.insert("quickPlayRealms".to_string(), String::new());

    // ============== Mod 数量统计 ==============
    let mod_count = count_mods(&game_dir);

    // ============== 内存决策 ==============
    let (memory_mode, memory_value) = resolve_memory_settings(settings, &actual_version_id, &data_dir);
    let total_mb = total_physical_memory_mb();
    let free_mb = free_physical_memory_mb();
    let max_mem_mb = resolve_max_memory(&memory_mode, total_mb, free_mb, mod_count);
    let min_mem_mb = resolve_min_memory(max_mem_mb);

    // ============== JVM 参数 ==============
    let mut jvm_args: Vec<String> = Vec::new();
    jvm_args.push(format!("-Xmx{}M", max_mem_mb));
    jvm_args.push(format!("-Xms{}M", min_mem_mb));
    jvm_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());
    jvm_args.push("-Djava.net.preferIPv4Stack=true".to_string());

    // GC 选择
    let has_user_gc = jvm_args.iter().any(|a| {
        a.starts_with("-XX:+Use") || a.starts_with("-XX:-Use") || a.starts_with("-XX:Use")
    });
    if !has_user_gc {
        push_gc_args(&mut jvm_args, max_mem_mb, mod_count);
    }
    // 大内存下额外启用字符串去重与元空间限制
    let has_user_mem_opt = jvm_args.iter().any(|a| {
        a.contains("StringDeduplication") || a.contains("CompressedClassSpaceSize") || a.contains("MetaspaceSize")
    });
    if !has_user_mem_opt && max_mem_mb >= 2048 {
        let using_g1 = jvm_args.iter().any(|a| a.contains("UseG1GC"));
        if using_g1 {
            jvm_args.push("-XX:+UseStringDeduplication".to_string());
        }
        let meta_mb = if mod_count >= 200 {
            1024
        } else if mod_count >= 100 {
            768
        } else {
            512
        };
        let ccs_mb = meta_mb.min(512);
        jvm_args.push(format!("-XX:CompressedClassSpaceSize={}m", ccs_mb));
        jvm_args.push(format!("-XX:MaxMetaspaceSize={}m", meta_mb));
    }
    if !jvm_args.iter().any(|a| a.contains("preferIPv4Stack") || a.contains("preferIPv6Stack")) {
        jvm_args.push("-Djava.net.preferIPv4Stack=true".to_string());
        jvm_args.push("-Djava.net.preferIPv4Addresses=true".to_string());
    }

    // 用户自定义 JVM 参数（版本独立 > 全局）
    let ver_settings = storage::load_version_settings(&actual_version_id, is_external);
    let ver_jvm_args = utils::get_str(&ver_settings, "jvmArgs");
    let effective_java_args = if !ver_jvm_args.trim().is_empty() {
        ver_jvm_args
    } else {
        utils::get_str(settings, "javaArgs")
    };
    if !effective_java_args.trim().is_empty() {
        push_user_jvm_args(&mut jvm_args, &effective_java_args);
    }

    // 整合包自带 CustomSkinLoader（万用皮肤补丁）时的兼容修复：
    // CustomSkinLoader 15.x 在 1.21+ 上打 SkinManager 字节码补丁可能失败并直接崩溃，
    // 崩溃报告建议加 -Dcustomskinloader.ignorePatchFailure=true 忽略该补丁失败。
    // 这里检测到 mods 目录存在 CustomSkinLoader 时自动追加，避免用户手动配置。
    if !jvm_args.iter().any(|a| a.contains("customskinloader.ignorePatchFailure"))
        && has_custom_skin_loader(&game_dir)
    {
        jvm_args.push("-Dcustomskinloader.ignorePatchFailure=true".to_string());
    }

    // CDS 归档
    let selected_java_path = dep_check::select_java_for_version(&actual_version_id, settings, version_json);
    let selected_java_path = if selected_java_path.is_empty() {
        "java".to_string()
    } else {
        selected_java_path
    };
    let java_major_ver = dep_check::inspect_java_version(&selected_java_path)
        .map(|(_, mv)| mv)
        .unwrap_or(8);
    let enable_cds = settings.get("enableCds").and_then(|v| v.as_bool()).unwrap_or(true) && java_major_ver >= 8;
    if enable_cds {
        let cds_dir = data_dir.join("cds");
        let cds_archive = cds_dir.join(format!("{}.jsa", actual_version_id));
        if let Ok(meta) = std::fs::metadata(&cds_archive) {
            if meta.len() > 1024 {
                jvm_args.push("-Xshare:on".to_string());
                jvm_args.push(format!("-XX:SharedArchiveFile={}", cds_archive.to_string_lossy()));
            }
        }
    }

    // ============== 加载器类型检测 ==============
    let main_class = {
        let mc = utils::get_str(version_json, "mainClass");
        if mc.is_empty() {
            "net.minecraft.client.main.Main".to_string()
        } else {
            mc
        }
    };
    let game_args_for_detection = version_json
        .get("arguments")
        .and_then(|v| v.get("game"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let has_forge_game_arg = game_args_for_detection.iter().any(|a| {
        a.as_str() == Some("forgeclient") || a.as_str() == Some("forge_server")
    });
    let is_forge = main_class.contains("modlauncher")
        || main_class.contains("fml")
        || main_class.contains("forge")
        || main_class.contains("bootstraplauncher")
        || main_class.contains("BootstrapLauncher")
        || has_forge_game_arg;
    let is_neoforge = main_class.contains("neoforged")
        || main_class.contains("neoforge")
        || game_args_for_detection.iter().any(|a| a.as_str() == Some("--fml.neoForgeVersion"))
        || version_json
            .get("libraries")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().any(|l| {
                    utils::get_str(l, "name").starts_with("net.neoforged.fancymodloader:loader")
                })
            })
            .unwrap_or(false);
    let is_fabric = main_class.contains("fabricmc") || main_class.contains("knot");

    // ============== 离线账户禁用 Realms ==============
    let is_offline_account = is_offline_token
        || utils::get_str(account, "type") == "offline";
    if is_offline_account {
        jvm_args.push("-Dminecraft.api.auth=off".to_string());
        jvm_args.push("-Dminecraft.api.env=local".to_string());
    }

    // ============== Forge / NeoForge 特殊 JVM 参数 ==============
    if is_forge || is_neoforge {
        if !jvm_args.iter().any(|a| a.contains("minecraft.client.jar")) {
            jvm_args.push(format!("-Dminecraft.client.jar={}", main_jar_path.to_string_lossy()));
        }
        if !jvm_args.iter().any(|a| a.contains("earlyLoadingWindow")) {
            jvm_args.push("-Dfml.earlyLoadingWindow=false".to_string());
        }
    }

    // JPMS 模块开放（仅 Java 9+）
    if (is_forge || is_neoforge) && java_major_ver >= 9 {
        push_jpms_flags(&mut jvm_args);
    }

    // ============== 收集版本 JSON 中的 JVM 参数 ==============
    let has_custom_resolution = !resolution.is_empty();
    collect_jvm_args_from_json(&mut jvm_args, version_json, &variables, has_custom_resolution);

    // 始终使用 java.library.path
    fix_library_path(&mut jvm_args, &natives_dir);

    if !jvm_args.iter().any(|a| a.contains("minecraft.launcher.brand")) {
        jvm_args.push("-Dminecraft.launcher.brand=VersePC".to_string());
        jvm_args.push("-Dminecraft.launcher.version=1.0.0".to_string());
    }
    if !jvm_args.iter().any(|a| a.contains("log4j2.formatMsgNoLookups")) {
        jvm_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());
    }

    // macOS 必须在主线程启动（LWJGL 要求）
    if cfg!(target_os = "macos") {
        jvm_args.insert(0, "-XstartOnFirstThread".to_string());
    }

    // module-path 中的引导 JAR 加入 ignoreList
    update_ignore_list_for_module_path(&mut jvm_args);

    // Forge/NeoForge：确保客户端 jar 加入 ignoreList。
    // BootstrapLauncher 按"文件名前缀"匹配 ignoreList（filename.startsWith(filter)）。
    // 本启动器的客户端 jar 命名为 "1.19.2-forge-43.2.21.jar"（版本号在前），
    // 不以 "forge-" 前缀开头，若不加进 ignoreList，该 jar 会在 classpath 中被再次
    // 加载为一个普通模块（_1._19._2.forge），与 -Dminecraft.client.jar 加载的
    // minecraft 模块同时导出 com.mojang.blaze3d.platform，导致模块解析冲突崩溃。
    if is_forge || is_neoforge {
        ensure_main_jar_in_ignore_list(&mut jvm_args, &main_jar_path);
    }

    // 始终使用完整 classpath（包含 client jar）
    let mut final_classpath = classpath.clone();
    let main_jar_str = main_jar_path.to_string_lossy().to_string();
    if !main_jar_str.is_empty() && main_jar_path.exists() {
        // NeoForge 新版 (20.6+/21.x/26.x)：版本目录 jar 即 patched jar 的副本，
        // 不能加入 classpath。patched jar 与 universal jar 均由 -DlibraryDirectory +
        // --fml.neoForgeVersion 让 FML 的 ProductionClientProviderLocator 自动发现并加载。
        // 若把版本目录 jar 加入 classpath，RequiredSystemFiles 会扫描到它，但 patched jar 用
        // --no-mod-manifest 构建、不含 NeoForgeMod.class，FML 会把它误判为 DEV（开发）模式，
        // 报 "Couldn't find NeoForgeMod.class" 与 "The patched Minecraft jar is missing"。
        let is_new_neoforge = main_class.contains("net.neoforged.fml.startup")
            || game_args_for_detection.iter().any(|a| a.as_str() == Some("--fml.neoForgeVersion"));
        if !is_new_neoforge {
            if !final_classpath.contains(&main_jar_str) {
                final_classpath.push(main_jar_str);
            }
        } else {
            println!("[ArgsBuilder] NeoForge 新版: 跳过版本目录 jar (由 -DlibraryDirectory locator 自动发现): {}", main_jar_str);
        }
    }
    let classpath_str = final_classpath.join(cp_separator);
    jvm_args.push("-cp".to_string());
    jvm_args.push(classpath_str);

    // 第三方登录：注入 authlib-injector javaagent
    if utils::get_str(account, "type") == "thirdparty" {
        let server_url = utils::get_str(account, "serverUrl");
        if !server_url.is_empty() {
            inject_authlib_agent(&mut jvm_args, &data_dir, &server_url);
        }
    }

    // log4j2 配置文件下载与注入
    inject_log4j_config(&mut jvm_args, version_json, &actual_version_id, &versions_dir);

    // ============== 主类 ==============
    jvm_args.push(main_class.clone());

    // ============== 游戏参数 ==============
    let mut game_args: Vec<String> = Vec::new();
    collect_game_args_from_json(&mut game_args, version_json, &variables, has_custom_resolution);

    // 旧版 minecraftArguments 模板
    if let Some(template) = version_json.get("minecraftArguments").and_then(|v| v.as_str()) {
        let replaced = replace_variables(template, &variables);
        for part in replaced.split_whitespace() {
            if !part.is_empty() {
                game_args.push(part.to_string());
            }
        }
    }

    // 全屏 / 分辨率
    // 先清理 game_args 中可能残留的 --fullscreen、--width、--height，
    // 确保不会因为版本 JSON 或前序处理引入多余的全屏/窗口参数。
    let mut i = 0;
    while i < game_args.len() {
        if game_args[i] == "--fullscreen" {
            game_args.remove(i);
        } else if game_args[i] == "--width" || game_args[i] == "--height" {
            if i + 1 < game_args.len() && !game_args[i + 1].starts_with("--") {
                game_args.remove(i + 1);
            }
            game_args.remove(i);
        } else {
            i += 1;
        }
    }

    let fullscreen = settings.get("fullscreen").and_then(|v| v.as_bool()).unwrap_or(false);
    if fullscreen {
        game_args.push("--fullscreen".to_string());
    } else {
        let (w, h) = adjust_window_resolution(res_w, res_h, &game_args);
        game_args.push("--width".to_string());
        game_args.push(w.to_string());
        game_args.push("--height".to_string());
        game_args.push(h.to_string());
    }

    // versionType
    let version_type_idx = game_args.iter().position(|a| a == "--versionType");
    if version_type_idx.is_none() {
        let custom_info = utils::get_str(settings, "customInfo").trim().to_string();
        let window_title = utils::get_str(settings, "windowTitle").trim().to_string();
        let vt = if !window_title.is_empty() {
            window_title
        } else if !custom_info.is_empty() {
            custom_info
        } else {
            "VersePC".to_string()
        };
        game_args.push("--versionType".to_string());
        game_args.push(vt);
    }

    // 确保 --gameDir / --assetsDir 已设置且变量已替换。
    // minecraftArguments 模板用 split_whitespace 拆分，含空格的路径（如
    // "E:\Verse Explorer X\..."）会被拆成多段，这里用权威值整体覆盖并清理残留。
    ensure_flag_arg(&mut game_args, "--gameDir", &game_dir.to_string_lossy());
    ensure_flag_arg(&mut game_args, "--assetsDir", &assets_root.to_string_lossy());

    // 去重游戏参数
    let final_game_args = deduplicate_game_args(&game_args);

    let mut all_args = jvm_args;
    all_args.extend(final_game_args);

    // 清理已知会导致 Java 17/21 崩溃的参数
    sanitize_jvm_args(&mut all_args);

    LaunchArguments {
        args: all_args,
        max_mem_mb,
    }
}

/// 清理 JVM 参数：过滤当前 Java 版本不支持的参数、trim 系统属性值
fn sanitize_jvm_args(args: &mut Vec<String>) {
    // 已知问题参数：
    // --sun-misc-unsafe-memory-access=allow 是 Java 24+ 参数，低版本会报 "Unrecognized option"
    let unsupported_options: Vec<&str> = vec![
        "--sun-misc-unsafe-memory-access=allow",
    ];

    for arg in args.iter_mut() {
        // trim -Dkey=value 的值部分（如 -DFabricMcEmu= xxx ）
        if let Some(pos) = arg.find("-D") {
            if let Some(eq_pos) = arg[pos..].find('=') {
                let abs_eq = pos + eq_pos;
                let key = &arg[..abs_eq];
                let value = arg[abs_eq + 1..].trim();
                *arg = format!("{}={}", key, value);
            }
        }
    }

    args.retain(|a| !unsupported_options.iter().any(|u| a.trim() == *u));
}

// ============================ 辅助函数 ============================

/// 替换字符串中的 ${var} 和 $var 变量
/// 复刻原项目 server/utils.js:replaceVariables
fn replace_variables(input: &str, vars: &HashMap<String, String>) -> String {
    let mut result = input.to_string();
    for (key, value) in vars {
        // ${var}
        let pattern1 = format!("${{{}}}", key);
        result = result.replace(&pattern1, value);
        // $var（后跟非字母数字下划线边界）
        let pattern2 = format!("${}", key);
        replace_dollar_var(&mut result, key, value);
        let _ = pattern2; // 已通过 replace_dollar_var 处理
    }
    result
}

/// 替换 $var（非 ${var}）形式，避免误匹配 $varXXX
fn replace_dollar_var(s: &mut String, key: &str, value: &str) {
    let token = format!("${}", key);
    let mut idx = 0;
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    while idx < bytes.len() {
        if bytes[idx] == b'$' && s[idx..].starts_with(&token) {
            let end = idx + token.len();
            // 边界检查：后面字符必须是非 [a-zA-Z0-9_]
            let after = s.as_bytes().get(end).copied();
            let is_boundary = match after {
                None => true,
                Some(c) => !(c.is_ascii_alphanumeric() || c == b'_'),
            };
            if is_boundary {
                out.push_str(value);
                idx = end;
                continue;
            }
        }
        // 复制一个 UTF-8 字符
        let ch_len = utf8_char_len(bytes[idx]);
        out.push_str(&s[idx..idx + ch_len]);
        idx += ch_len;
    }
    *s = out;
}

/// 获取 UTF-8 字符长度
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// 构建完整 classpath
/// 简化版：遍历 libraries，根据 rules 过滤，根据 downloads.artifact.path 拼接路径
/// 不实现 natives 处理、模块化库处理等复杂逻辑（natives 由 extract_natives_dir 单独处理）
pub fn build_classpath(
    version_json: &Value,
    external_version_dir: Option<&Path>,
    libraries_dir: &Path,
) -> Vec<String> {
    let mut classpath: Vec<String> = Vec::new();
    let libraries = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 搜索路径：外部根的 libraries 优先，其次本地 libraries
    let mut search_bases: Vec<PathBuf> = Vec::new();
    if let Some(ext_dir) = external_version_dir {
        if let Some(root) = dep_check::find_external_root(ext_dir) {
            search_bases.push(root.join("libraries"));
        }
    }
    search_bases.push(libraries_dir.to_path_buf());

    let current_os = dep_check::current_platform_name();
    let current_arch = if cfg!(target_pointer_width = "64") { "x64" } else { "x86" };

    for lib in &libraries {
        // rules 过滤
        if let Some(rules) = lib.get("rules").and_then(|v| v.as_array()) {
            if !dep_check::evaluate_rules(rules, false) {
                continue;
            }
        }

        let name = utils::get_str(lib, "name");
        // 跳过 natives 库（它们通过 -Djava.library.path 加载）
        if is_native_library(lib, &current_os) {
            continue;
        }

        // NeoForge: neoforge:*:client 是虚拟库记录（官方 Maven 返回 404，不可直接下载），
        // 不把 patched/universal jar 加入 classpath。这些 jar 由 FML 的
        // ProductionClientProviderLocator 通过 -DlibraryDirectory + --fml.neoForgeVersion
        // 参数自动查找并加载。若手动加入 classpath 会让 FML 误判为 DEV 模式，
        // 报 "Couldn't find NeoForgeMod.class" 与 "The patched Minecraft jar is missing"。
        if name.starts_with("net.neoforged:neoforge:") && name.ends_with(":client") {
            continue;
        }

        // 优先用 downloads.artifact.path
        if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
            if let Some(path) = artifact.get("path").and_then(|p| p.as_str()) {
                if !path.is_empty() {
                    if let Some(found) = find_lib_in_bases(&search_bases, path) {
                        if !classpath.contains(&found) {
                            classpath.push(found);
                        }
                        continue;
                    }
                }
            }
        }

        // 回退：从 maven name 解析路径
        if !name.is_empty() {
            if let Some(rel_path) = maven_name_to_path(&name) {
                if let Some(found) = find_lib_in_bases(&search_bases, &rel_path) {
                    if !classpath.contains(&found) {
                        classpath.push(found);
                    }
                    continue;
                }
            }
            // 最后回退：扫描目录
            if let Some(found) = find_lib_by_fallback(&name, &search_bases) {
                if !classpath.contains(&found) {
                    classpath.push(found);
                }
            }
        }
    }

    let _ = (current_os, current_arch); // 标记变量已用
    classpath
}

/// 判断库是否为 native 库（按 classifiers 判断）
fn is_native_library(lib: &Value, current_os: &str) -> bool {
    let classifiers = lib
        .get("downloads")
        .and_then(|d| d.get("classifiers"))
        .and_then(|c| c.as_object());
    if let Some(classifiers) = classifiers {
        let native_keys = [
            format!("natives-{}", current_os),
            format!("natives-{}-legacy", current_os),
            format!("natives-{}-arm64", current_os),
        ];
        for k in &native_keys {
            if classifiers.contains_key(k) {
                return true;
            }
        }
        // 检查 lib.name 中是否含 natives-<os> 后缀
        let name = utils::get_str(lib, "name");
        if name.contains(&format!(":natives-{}:", current_os))
            || name.contains(&format!(":natives-{}-legacy:", current_os))
        {
            return true;
        }
    }
    false
}

/// Maven 名称转相对路径
/// 例：`com.google.code.gson:gson:2.10` → `com/google/code/gson/gson/2.10/gson-2.10.jar`
fn maven_name_to_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', "/");
    let artifact_id = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 {
        format!("-{}", parts[3])
    } else {
        String::new()
    };
    let jar_name = format!("{}-{}{}.jar", artifact_id, version, classifier);
    Some(format!("{}/{}/{}/{}", group_path, artifact_id, version, jar_name))
}

/// 在多个搜索基目录中查找文件
fn find_lib_in_bases(bases: &[PathBuf], rel_path: &str) -> Option<String> {
    for base in bases {
        let p = base.join(rel_path);
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// 兜底：扫描目录找到匹配的 JAR
fn find_lib_by_fallback(name: &str, bases: &[PathBuf]) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', std::path::MAIN_SEPARATOR_STR);
    let artifact_id = parts[1];
    let version = parts[2];
    let classifier = if parts.len() >= 4 { Some(parts[3]) } else { None };

    for base in bases {
        let dir = base.join(&group_path).join(artifact_id).join(version);
        if !dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut jars: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".jar")
                && !fname.ends_with("-sources.jar")
                && !fname.ends_with("-javadoc.jar")
            {
                jars.push(fname);
            }
        }
        if jars.is_empty() {
            continue;
        }
        if let Some(cls) = classifier {
            let preferred = format!("{}-{}-{}.jar", artifact_id, version, cls);
            if let Some(m) = jars.iter().find(|f| *f == &preferred) {
                return Some(dir.join(m).to_string_lossy().to_string());
            }
        }
        let preferred = format!("{}-{}.jar", artifact_id, version);
        if let Some(m) = jars.iter().find(|f| *f == &preferred) {
            return Some(dir.join(m).to_string_lossy().to_string());
        }
        return Some(dir.join(&jars[0]).to_string_lossy().to_string());
    }
    None
}

/// 计算 natives 目录路径并从版本 JSON 的 native 库中解压原生二进制文件
/// 复刻原项目 natives.js:extractNatives
///
/// 从 version_json.libraries 中识别 native 库，解压其中的 .dll/.so/.dylib/.jnilib
/// 到 natives 目录。带完整性校验：关键原生库齐全且 jar 未更新时跳过解压。
fn extract_natives_dir(
    version_json: &Value,
    version_id: &str,
    external_version_dir: Option<&Path>,
    data_dir: &Path,
) -> PathBuf {
    let natives_root = data_dir.join("natives");
    let natives_dir = natives_root.join(version_id);

    let libraries = version_json
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let current_platform = dep_check::current_platform_name();
    let is_64 = cfg!(target_pointer_width = "64");

    // 搜索路径：外部根 libraries 优先，其次本地 libraries，再 .minecraft/libraries
    let mut search_bases: Vec<PathBuf> = Vec::new();
    if let Some(ext_dir) = external_version_dir {
        if let Some(root) = dep_check::find_external_root(ext_dir) {
            search_bases.push(root.join("libraries"));
        }
    }
    let libraries_dir = data_dir.join("libraries");
    search_bases.push(libraries_dir.clone());
    if let Some(home) = dirs::home_dir() {
        let mc_lib = home.join(".minecraft").join("libraries");
        if mc_lib != libraries_dir && mc_lib.exists() {
            search_bases.push(mc_lib);
        }
    }

    // 收集所有 native jar 路径
    let mut native_jars: Vec<PathBuf> = Vec::new();
    for lib in &libraries {
        if let Some(rules) = lib.get("rules").and_then(|v| v.as_array()) {
            if !dep_check::evaluate_rules(rules, false) {
                continue;
            }
        }
        let lib_name = utils::get_str(lib, "name");
        let mut native_path: Option<PathBuf> = None;
        let mut is_native = false;

        if let Some(natives) = lib.get("natives").and_then(|v| v.as_object()) {
            let native_key = natives.get(current_platform.as_str()).and_then(|v| v.as_str());
            let Some(native_key) = native_key else {
                continue;
            };
            is_native = true;
            let classifier = native_key.replace("${arch}", if is_64 { "64" } else { "32" });
            if let Some(nd) = lib
                .get("downloads")
                .and_then(|d| d.get("classifiers"))
                .and_then(|c| c.get(classifier.as_str()))
            {
                if let Some(path) = nd.get("path").and_then(|p| p.as_str()) {
                    if !path.is_empty() {
                        native_path = find_native_jar(&search_bases, path);
                    }
                }
            }
        } else if lib_name.contains(":natives-") {
            let name_parts: Vec<&str> = lib_name.split(':').collect();
            let native_suffix = name_parts.last().copied().unwrap_or("");
            if !native_suffix.starts_with("natives-") {
                continue;
            }
            let platform_native = native_suffix.strip_prefix("natives-").unwrap_or("");
            let valid = if is_64 {
                platform_native == current_platform
                    || platform_native == format!("{}-x64", current_platform)
            } else {
                platform_native == format!("{}-x86", current_platform)
                    || platform_native == current_platform
            };
            if !valid {
                continue;
            }
            is_native = true;
            if let Some(path) = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
            {
                if !path.is_empty() {
                    native_path = find_native_jar(&search_bases, path);
                }
            }
            if native_path.is_none() && name_parts.len() >= 4 {
                let group_path = name_parts[0].replace('.', "/");
                let nname = name_parts[1];
                let nver = name_parts[2];
                let nclassifier = name_parts[3];
                let njar = format!("{}-{}-{}.jar", nname, nver, nclassifier);
                native_path = find_native_jar(
                    &search_bases,
                    &format!("{}/{}/{}/{}", group_path, nname, nver, njar),
                );
            }
        }

        if !is_native {
            continue;
        }
        if let Some(np) = native_path {
            if np.exists() && !native_jars.contains(&np) {
                native_jars.push(np);
            }
        }
    }

    if native_jars.is_empty() {
        let _ = std::fs::create_dir_all(&natives_dir);
        return natives_dir;
    }

    // 完整性校验：判断关键原生库是否齐全
    let is_lwjgl2 = libraries
        .iter()
        .any(|l| utils::get_str(l, "name").starts_with("org.lwjgl.lwjgl:"));
    let critical_list: &[&str] = if is_lwjgl2 {
        &["lwjgl.dll", "lwjgl64.dll", "OpenAL32.dll", "OpenAL64.dll"]
    } else {
        &[
            "lwjgl.dll",
            "lwjgl_opengl.dll",
            "glfw.dll",
            "lwjgl_stb.dll",
            "lwjgl_tinyfd.dll",
            "openal.dll",
        ]
    };
    let native_ext = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let is_win = cfg!(target_os = "windows");
    let critical_missing: Vec<&str> = critical_list
        .iter()
        .filter(|n| !native_critical_exists(&natives_dir, n, is_win, native_ext))
        .copied()
        .collect();

    // 判断是否需要重新解压：目录缺失 / 关键库缺失 / jar 已更新
    let needs_extract = if natives_dir.exists() && critical_missing.is_empty() {
        let max_jar_mtime = max_mtime(&native_jars);
        let max_file_mtime = max_dir_mtime(&natives_dir);
        max_jar_mtime > max_file_mtime
    } else {
        true
    };

    if needs_extract {
        if natives_dir.exists() {
            let _ = std::fs::remove_dir_all(&natives_dir);
        }
        let _ = std::fs::create_dir_all(&natives_dir);
        for jar_path in &native_jars {
            extract_native_jar(jar_path, &natives_dir);
        }
    }

    natives_dir
}

/// 在多个搜索基准目录中查找 native jar 的相对路径
fn find_native_jar(bases: &[PathBuf], href: &str) -> Option<PathBuf> {
    for base in bases {
        let p = base.join(href);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 判断关键原生库文件是否存在（Windows 下直接匹配 .dll，其他平台按扩展名）
fn native_critical_exists(natives_dir: &Path, dll_name: &str, is_win: bool, ext: &str) -> bool {
    let target = if is_win {
        natives_dir.join(dll_name)
    } else {
        let base = dll_name.trim_end_matches(".dll");
        natives_dir.join(format!("{}.{}", base, ext))
    };
    target.exists()
}

/// 计算一组路径中的最大修改时间（毫秒）
fn max_mtime(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .filter_map(|p| file_mtime_ms(p))
        .max()
        .unwrap_or(0)
}

/// 计算目录中所有文件的最大修改时间（毫秒）
fn max_dir_mtime(dir: &Path) -> u64 {
    let mut max = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if let Some(m) = e.metadata().ok().and_then(|m| m.modified().ok()) {
                if let Ok(s) = m.duration_since(std::time::UNIX_EPOCH) {
                    max = max.max(s.as_millis() as u64);
                }
            }
        }
    }
    max
}

/// 计算单个文件的修改时间（毫秒）
fn file_mtime_ms(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let secs = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(secs.as_millis() as u64)
}

/// 解压单个 native jar 中的原生二进制文件（.dll/.so/.dylib/.jnilib）
fn extract_native_jar(jar_path: &Path, natives_dir: &Path) {
    use std::io::Read;
    let file = match std::fs::File::open(jar_path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return,
    };
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        if name.starts_with("META-INF")
            || name.starts_with('.')
            || name.contains("/.git")
            || name.ends_with(".sha1")
            || name.ends_with(".gitkeep")
            || name.ends_with(".gitignore")
        {
            continue;
        }
        let lower = name.to_lowercase();
        let is_native = lower.ends_with(".dll")
            || lower.ends_with(".so")
            || lower.ends_with(".dylib")
            || lower.ends_with(".jnilib");
        if !is_native {
            continue;
        }
        let file_name = match std::path::Path::new(&name).file_name() {
            Some(f) => f.to_string_lossy().to_string(),
            None => continue,
        };
        let dest_path = natives_dir.join(&file_name);
        // 已存在且大小相同则跳过
        if let Ok(existing) = std::fs::metadata(&dest_path) {
            if existing.len() == entry.size() {
                continue;
            }
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
            let _ = std::fs::write(&dest_path, buf);
        }
    }
}

/// 解析版本游戏目录
/// 复刻原项目 args-builder.js 中的 gameDir 决策
fn resolve_game_dir(
    version_id: &str,
    custom_game_dir: Option<&str>,
    external_version_dir: Option<&Path>,
    settings: &Value,
    versions_dir: &Path,
    data_dir: &Path,
) -> PathBuf {
    if let Some(custom) = custom_game_dir {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    if let Some(ext_dir) = external_version_dir {
        return ext_dir.to_path_buf();
    }
    if resolve_version_isolation(version_id) {
        return versions_dir.join(version_id);
    }
    let settings_game_dir = utils::get_str(settings, "gameDir");
    if !settings_game_dir.is_empty() {
        return PathBuf::from(settings_game_dir);
    }
    data_dir.to_path_buf()
}

/// 简化版版本隔离判定
/// 复刻原项目 server/versions/version-dir.js:resolveVersionIsolation
fn resolve_version_isolation(version_id: &str) -> bool {
    if version_id.is_empty() || version_id.contains(" [外部") {
        return !version_id.is_empty();
    }
    let settings = storage::load_settings();
    let ver_settings = storage::load_version_settings(version_id, false);
    let ver_isolation = utils::get_str(&ver_settings, "isolation");
    let mut effective: bool;
    if ver_isolation == "on" {
        effective = true;
    } else if ver_isolation == "off" {
        effective = false;
    } else {
        effective = settings
            .get("versionIsolation")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
    }
    // 关闭隔离时若已有 mods/saves 子目录则保持隔离
    if !effective {
        let data_dir = storage::resolve_data_dir();
        let version_dir = data_dir.join("versions").join(version_id);
        let mods_dir = version_dir.join("mods");
        let saves_dir = version_dir.join("saves");
        if mods_dir.exists() && dir_has_visible_files(&mods_dir) {
            effective = true;
        }
        if !effective && saves_dir.exists() && dir_has_subdirs(&saves_dir) {
            effective = true;
        }
    }
    effective
}

fn dir_has_visible_files(dir: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') {
                return true;
            }
        }
    }
    false
}

fn dir_has_subdirs(dir: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    return true;
                }
            }
        }
    }
    false
}

/// 预创建游戏目录常用子目录
fn ensure_game_dirs(game_dir: &Path) {
    let _ = std::fs::create_dir_all(game_dir);
    for sub in ["mods", "resourcepacks", "shaderpacks", "saves", "config", "logs", "crash-reports"] {
        let _ = std::fs::create_dir_all(game_dir.join(sub));
    }
}

/// 复制 Forge log4j2.xml 到游戏目录
fn copy_forge_log4j(version_id: &str, versions_dir: &Path, game_dir: &Path) {
    let src = versions_dir.join(version_id).join("log4j2.xml");
    if !src.exists() {
        return;
    }
    let dst = game_dir.join("log4j2.xml");
    if dst.exists() {
        return;
    }
    let _ = std::fs::copy(&src, &dst);
}

/// 解析分辨率字符串（如 "1280x720"）
fn parse_resolution(resolution: &str) -> (u32, u32) {
    let parts: Vec<&str> = resolution.split('x').collect();
    let w = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(854);
    let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(480);
    (w, h)
}

/// 生成离线 access token（JWT 格式，避免 BootstrapLauncher Base64 解析崩溃）
fn generate_offline_access_token(uuid: &str, player_name: &str) -> String {
    use base64::{engine::general_purpose, Engine as _};
    let header = general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::json!({"alg":"none","typ":"JWT"}).to_string());
    let payload = general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::json!({
            "sub": uuid,
            "iss": "VersePC",
            "name": player_name,
            "offline": true
        })
        .to_string(),
    );
    format!("{}.{}.offline", header, payload)
}

/// 统计 mods 目录中的 .jar 文件数量
fn count_mods(game_dir: &Path) -> u64 {
    let mods_dir = game_dir.join("mods");
    if !mods_dir.exists() {
        return 0;
    }
    let mut count = 0u64;
    if let Ok(entries) = std::fs::read_dir(&mods_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".jar") && !name.ends_with(".jar.disabled") {
                count += 1;
            }
        }
    }
    count
}

/// 检测整合包 mods 目录是否包含 CustomSkinLoader（万用皮肤补丁）
/// 文件名通常形如 "[万用皮肤补丁] CustomSkinLoader_Universal-*.jar" 或 "CustomSkinLoader-*.jar"
fn has_custom_skin_loader(game_dir: &Path) -> bool {
    let mods_dir = game_dir.join("mods");
    if !mods_dir.exists() {
        return false;
    }
    if let Ok(entries) = std::fs::read_dir(&mods_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".jar")
                && !name.ends_with(".jar.disabled")
                && name.contains("customskinloader")
            {
                return true;
            }
        }
    }
    false
}

/// 推送 GC 参数（按内存大小与 mod 数量选择）
fn push_gc_args(jvm_args: &mut Vec<String>, max_mem_mb: u64, mod_count: u64) {
    if max_mem_mb <= 1024 {
        jvm_args.push("-XX:+UseSerialGC".to_string());
    } else {
        let cpu_count = num_cpus();
        let parallel_gc_threads = if cpu_count <= 4 {
            2
        } else {
            std::cmp::max(2, (cpu_count * 5) / 8)
        };
        let conc_gc_threads = std::cmp::max(1, parallel_gc_threads / 2);
        jvm_args.push("-XX:+UnlockExperimentalVMOptions".to_string());
        jvm_args.push("-XX:+UseG1GC".to_string());
        jvm_args.push("-XX:MaxGCPauseMillis=100".to_string());
        jvm_args.push("-XX:+AlwaysPreTouch".to_string());
        jvm_args.push("-XX:G1NewSizePercent=40".to_string());
        jvm_args.push("-XX:G1ReservePercent=20".to_string());
        jvm_args.push("-XX:SurvivorRatio=32".to_string());
        jvm_args.push(format!("-XX:ParallelGCThreads={}", parallel_gc_threads));
        jvm_args.push(format!("-XX:ConcGCThreads={}", conc_gc_threads));
        jvm_args.push("-XX:+PerfDisableSharedMem".to_string());
        if max_mem_mb >= 4096 {
            let region_size = if max_mem_mb >= 8192 { "32m" } else { "16m" };
            jvm_args.push(format!("-XX:G1HeapRegionSize={}", region_size));
        }
        if mod_count > 50 {
            jvm_args.push("-XX:G1MixedGCCountTarget=16".to_string());
            jvm_args.push("-XX:G1HeapWastePercent=5".to_string());
        }
    }
    jvm_args.push("-XX:+DisableExplicitGC".to_string());
}

/// 获取 CPU 核数
fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

/// 获取物理内存总量（MB）
fn total_physical_memory_mb() -> u64 {
    // 通过 sysinfo 风格的读取（这里用 std 兜底，平台差异较大）
    // Windows: 用 GetProcessMemoryInfo / GlobalMemoryStatusEx（暂用简化版）
    #[cfg(target_os = "windows")]
    {
        // 使用 windows-sys 风格的 API（暂用 sysinfo 兜底）
        // 这里简单返回 8GB（兜底值），实际应调用系统 API
        // TODO: 接入 sysinfo crate 获取准确值
        8192
    }
    #[cfg(not(target_os = "windows"))]
    {
        8192
    }
}

/// 获取当前可用物理内存（MB）
fn free_physical_memory_mb() -> u64 {
    // TODO: 接入 sysinfo crate 获取准确值
    4096
}

/// 推送用户自定义 JVM 参数（去重）
fn push_user_jvm_args(jvm_args: &mut Vec<String>, java_args: &str) {
    let user_args: Vec<String> = java_args
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    for arg in user_args {
        let base_arg = arg.split('=').next().unwrap_or(&arg).to_string();
        let has_conflict = jvm_args.iter().any(|existing| existing.starts_with(&base_arg));
        // GC 参数冲突检测
        let gc_patterns = ["-XX:+Use", "-XX:-Use"];
        let is_gc_arg = gc_patterns.iter().any(|p| arg.starts_with(p));
        if is_gc_arg {
            let has_gc_conflict = jvm_args
                .iter()
                .any(|existing| gc_patterns.iter().any(|p| existing.starts_with(p)));
            if has_gc_conflict {
                continue;
            }
        }
        if !has_conflict {
            jvm_args.push(arg);
        }
    }
}

/// JPMS 模块开放标志（Java 9+，Forge/NeoForge）
fn push_jpms_flags(jvm_args: &mut Vec<String>) {
    let flags: &[(&str, &str)] = &[
        ("--add-exports", "java.base/sun.security.util=ALL-UNNAMED"),
        ("--add-exports", "java.base/sun.security.x509=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.lang=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.lang.invoke=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.lang.reflect=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.io=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.nio=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.util=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.util.concurrent=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.util.concurrent.atomic=ALL-UNNAMED"),
        ("--add-opens", "java.base/java.util.concurrent.locks=ALL-UNNAMED"),
        ("--add-opens", "java.base/sun.nio.ch=ALL-UNNAMED"),
        ("--add-opens", "java.base/sun.nio.fs=ALL-UNNAMED"),
        ("--add-opens", "java.base/sun.security.action=ALL-UNNAMED"),
        ("--add-opens", "java.base/sun.security.provider=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.loader=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.ref=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.reflect=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.math=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.misc=ALL-UNNAMED"),
        ("--add-opens", "java.base/jdk.internal.util=ALL-UNNAMED"),
        ("--add-opens", "java.management/sun.management=ALL-UNNAMED"),
        ("--add-opens", "java.management/com.sun.jmx.mbeanserver=ALL-UNNAMED"),
        ("--add-opens", "jdk.management/com.sun.management.internal=ALL-UNNAMED"),
        ("--add-opens", "java.rmi/sun.rmi.registry=ALL-UNNAMED"),
        ("--add-opens", "java.rmi/sun.rmi.server=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/java.awt=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/java.awt.font=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/java.awt.peer=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/javax.swing=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/sun.awt=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/sun.java2d=ALL-UNNAMED"),
        ("--add-opens", "java.desktop/sun.font=ALL-UNNAMED"),
        ("--add-opens", "jdk.unsupported/sun.misc=ALL-UNNAMED"),
    ];
    for (flag, value) in flags {
        let exists = jvm_args.iter().enumerate().any(|(idx, a)| {
            a == flag
                && idx + 1 < jvm_args.len()
                && jvm_args[idx + 1] == *value
        });
        if !exists {
            jvm_args.push(flag.to_string());
            jvm_args.push(value.to_string());
        }
    }
}

/// 收集版本 JSON 中的 JVM 参数
/// 复刻原项目 args-builder.js 中"收集版本 JSON 中的 JVM 参数"逻辑
fn collect_jvm_args_from_json(
    jvm_args: &mut Vec<String>,
    version_json: &Value,
    variables: &HashMap<String, String>,
    has_custom_resolution: bool,
) {
    // 收集 jvm 参数来源：标准 jvm 组 + default-user-jvm 组
    let mut sources: Vec<Value> = Vec::new();
    if let Some(arr) = version_json
        .get("arguments")
        .and_then(|v| v.get("jvm"))
        .and_then(|v| v.as_array())
    {
        sources.extend(arr.iter().cloned());
    }
    if let Some(arr) = version_json
        .get("arguments")
        .and_then(|v| v.get("default-user-jvm"))
        .and_then(|v| v.as_array())
    {
        sources.extend(arr.iter().cloned());
    }

    if sources.is_empty() {
        return;
    }

    let mut prev_was_cp = false;
    for arg in &sources {
        if let Some(s) = arg.as_str() {
            let replaced = replace_variables(s, variables);
            // 跳过 -cp 和 classpath 字符串
            if replaced == "-cp" {
                prev_was_cp = true;
                continue;
            }
            if prev_was_cp {
                prev_was_cp = false;
                continue;
            }
            prev_was_cp = false;

            let is_multi_value_flag = matches!(
                replaced.as_str(),
                "--add-opens" | "--add-exports" | "--add-reads" | "--add-modules" | "--patch-module" | "-javaagent"
            );
            if is_multi_value_flag {
                jvm_args.push(replaced);
                // 下一个字符串参数也加入
                // 注意：sources 是 owned Vec，需要在外部处理，这里简化为不预读下个
            } else {
                if is_gc_arg(&replaced) && has_garbage_collector_arg(jvm_args) {
                    continue;
                }
                if replaced.starts_with("-Xmx") || replaced.starts_with("-Xms") {
                    let prefix = &replaced[..4];
                    if !jvm_args.iter().any(|e| e.starts_with(prefix)) {
                        jvm_args.push(replaced);
                    }
                } else if !jvm_args.iter().any(|existing| existing == &replaced) {
                    jvm_args.push(replaced);
                }
            }
        } else if let Some(value) = arg.get("value") {
            let rules_match = arg
                .get("rules")
                .and_then(|v| v.as_array())
                .map(|rules| dep_check::evaluate_rules(rules, has_custom_resolution))
                .unwrap_or(true);
            if !rules_match {
                continue;
            }
            // value 可能是 string 或 array
            if let Some(s) = value.as_str() {
                let replaced = replace_variables(s, variables);
                let is_multi = matches!(
                    replaced.as_str(),
                    "--add-opens" | "--add-exports" | "--add-reads" | "--add-modules" | "--patch-module" | "-javaagent"
                );
                if is_multi {
                    jvm_args.push(replaced);
                } else {
                    if is_gc_arg(&replaced) && has_garbage_collector_arg(jvm_args) {
                        continue;
                    }
                    if !jvm_args.iter().any(|existing| existing == &replaced) {
                        jvm_args.push(replaced);
                    }
                }
            } else if let Some(arr) = value.as_array() {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        let replaced = replace_variables(s, variables);
                        let is_multi = matches!(
                            replaced.as_str(),
                            "--add-opens" | "--add-exports" | "--add-reads" | "--add-modules" | "--patch-module" | "-javaagent"
                        );
                        if is_multi {
                            jvm_args.push(replaced);
                        } else {
                            if is_gc_arg(&replaced) && has_garbage_collector_arg(jvm_args) {
                                continue;
                            }
                            if !jvm_args.iter().any(|existing| existing == &replaced) {
                                jvm_args.push(replaced);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 判断是否为 GC 参数
fn is_gc_arg(s: &str) -> bool {
    s.starts_with("-XX:+Use") || s.starts_with("-XX:-Use")
}

/// 检测是否已存在 GC 参数
/// 复刻原项目 natives.js:hasGarbageCollectorArg
fn has_garbage_collector_arg(args: &[String]) -> bool {
    args.iter().any(|a| {
        // -XX:+Use<XXX>GC / -XX:-Use<XXX>GC / -XX:+Use<XXX>Collector / -XX:-Use<XXX>Collector
        if let Some(rest) = a.strip_prefix("-XX:+Use") {
            rest.ends_with("GC") || rest.ends_with("Collector")
        } else if let Some(rest) = a.strip_prefix("-XX:-Use") {
            rest.ends_with("GC") || rest.ends_with("Collector")
        } else {
            false
        }
    })
}

/// 修复 java.library.path 参数（确保变量已替换）
fn fix_library_path(jvm_args: &mut Vec<String>, natives_dir: &Path) {
    let natives_str = natives_dir.to_string_lossy().to_string();
    let idx = jvm_args.iter().position(|a| a.contains("java.library.path"));
    if let Some(idx) = idx {
        let val = &jvm_args[idx];
        if val.contains("${natives_directory}") || val.contains("$natives_directory") {
            let replaced = val
                .replace("${natives_directory}", &natives_str)
                .replace("$natives_directory", &natives_str);
            jvm_args[idx] = replaced;
        }
    } else {
        jvm_args.push(format!("-Djava.library.path={}", natives_str));
    }
}

/// 确保指定的主 Jar 文件名被加入 ignoreList。
/// BootstrapLauncher 的 ignoreList 按"文件名前缀"匹配，漏掉会导致客户端 jar 被重复加载为模块。
fn ensure_main_jar_in_ignore_list(jvm_args: &mut Vec<String>, main_jar_path: &Path) {
    let file_name = match main_jar_path.file_name().and_then(|n| n.to_str()) {
        Some(n) if !n.is_empty() => n,
        _ => return,
    };
    let il_idx = jvm_args.iter().position(|a| a.starts_with("-DignoreList="));
    if let Some(il_idx) = il_idx {
        let existing: Vec<String> = jvm_args[il_idx]
            .strip_prefix("-DignoreList=")
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if existing.iter().any(|e| e == file_name) {
            return;
        }
        let mut all = existing;
        all.push(file_name.to_string());
        jvm_args[il_idx] = format!("-DignoreList={}", all.join(","));
    } else {
        jvm_args.push(format!("-DignoreList={}", file_name));
    }
}

/// 更新 module-path 中的引导 JAR 加入 ignoreList
fn update_ignore_list_for_module_path(jvm_args: &mut Vec<String>) {
    let p_idx = jvm_args
        .iter()
        .position(|a| a == "-p" || a == "--module-path");
    if let Some(idx) = p_idx {
        if idx + 1 >= jvm_args.len() {
            return;
        }
        let module_path_str = jvm_args[idx + 1].clone();
        let sep = if cfg!(target_os = "windows") { ';' } else { ':' };
        let module_jars: Vec<&str> = module_path_str
            .split(sep)
            .filter(|p| p.ends_with(".jar"))
            .collect();
        let mut prefixes: Vec<String> = Vec::new();
        for jar_path in &module_jars {
            let basename = std::path::Path::new(jar_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            // 提取前缀：到第一个 -<数字> 之前
            let prefix = if let Some(idx) = basename.rfind("-") {
                let after = &basename[idx + 1..];
                if after.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    &basename[..idx]
                } else {
                    basename
                }
            } else {
                basename
            };
            if !prefix.is_empty() && !prefixes.contains(&prefix.to_string()) {
                prefixes.push(prefix.to_string());
            }
        }
        if prefixes.is_empty() {
            return;
        }
        let il_idx = jvm_args
            .iter()
            .position(|a| a.starts_with("-DignoreList="));
        if let Some(il_idx) = il_idx {
            let existing: Vec<&str> = jvm_args[il_idx]
                .strip_prefix("-DignoreList=")
                .unwrap_or("")
                .split(',')
                .filter(|s| !s.is_empty())
                .collect();
            let mut all: Vec<String> = existing.iter().map(|s| s.to_string()).collect();
            for pf in &prefixes {
                if !all.iter().any(|it| it == pf) {
                    all.push(pf.clone());
                }
            }
            jvm_args[il_idx] = format!("-DignoreList={}", all.join(","));
        } else {
            jvm_args.push(format!("-DignoreList={}", prefixes.join(",")));
        }
    }
}

/// 注入 authlib-injector javaagent
fn inject_authlib_agent(jvm_args: &mut Vec<String>, data_dir: &Path, server_url: &str) {
    let ai_dir = data_dir.join("authlib-injector");
    if !ai_dir.exists() {
        return;
    }
    let mut jars: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&ai_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".jar") {
                jars.push(name);
            }
        }
    }
    jars.sort();
    if let Some(latest) = jars.last() {
        let ai_path = ai_dir.join(latest);
        // 去除 serverUrl 中的 @@@ / @@ 分隔符
        let clean_url = server_url.split("@@@").next().unwrap_or(server_url);
        let clean_url = clean_url.split("@@").next().unwrap_or(clean_url);
        let has_javaagent = jvm_args.iter().any(|a| a.starts_with("-javaagent:"));
        if !has_javaagent {
            jvm_args.insert(0, format!("-javaagent:{}={}", ai_path.to_string_lossy(), clean_url));
        }
    }
}

/// 下载并注入 log4j2 配置文件
fn inject_log4j_config(
    jvm_args: &mut Vec<String>,
    version_json: &Value,
    version_id: &str,
    versions_dir: &Path,
) {
    let argument = version_json
        .get("logging")
        .and_then(|v| v.get("client"))
        .and_then(|v| v.get("argument"))
        .and_then(|v| v.as_str());
    let file_id = version_json
        .get("logging")
        .and_then(|v| v.get("client"))
        .and_then(|v| v.get("file"))
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str());
    let (argument, file_id) = match (argument, file_id) {
        (Some(a), Some(f)) => (a.to_string(), f.to_string()),
        _ => return,
    };
    let log_config_path = versions_dir.join(version_id).join(&file_id);
    if !log_config_path.exists() {
        // 异步下载由调用方处理，这里跳过
        return;
    }
    let arg = argument.replace("${path}", &log_config_path.to_string_lossy());
    let has_log_arg = jvm_args.iter().any(|a| {
        a.contains("log4j2") || a.contains("Log4j") || a.contains("log4j.configurationFile")
    });
    if !has_log_arg {
        jvm_args.push(arg);
    }
}

/// 收集版本 JSON 中的游戏参数
fn collect_game_args_from_json(
    game_args: &mut Vec<String>,
    version_json: &Value,
    variables: &HashMap<String, String>,
    has_custom_resolution: bool,
) {
    let mut sources: Vec<Value> = Vec::new();
    if let Some(arr) = version_json
        .get("arguments")
        .and_then(|v| v.get("game"))
        .and_then(|v| v.as_array())
    {
        sources.extend(arr.iter().cloned());
    }
    if let Some(arr) = version_json
        .get("arguments")
        .and_then(|v| v.get("default-user-game"))
        .and_then(|v| v.as_array())
    {
        sources.extend(arr.iter().cloned());
    }

    for arg in &sources {
        if let Some(s) = arg.as_str() {
            game_args.push(replace_variables(s, variables));
        } else if let Some(value) = arg.get("value") {
            let rules_match = arg
                .get("rules")
                .and_then(|v| v.as_array())
                .map(|rules| dep_check::evaluate_rules(rules, has_custom_resolution))
                .unwrap_or(true);
            if !rules_match {
                continue;
            }
            if let Some(s) = value.as_str() {
                game_args.push(replace_variables(s, variables));
            } else if let Some(arr) = value.as_array() {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        game_args.push(replace_variables(s, variables));
                    }
                }
            }
        }
    }
}

/// 游戏窗口分辨率修正：减去标题栏+边框，避免窗口铺满屏幕时标题栏被顶出屏幕外。
/// 对所有版本（原版 / Forge / NeoForge / Fabric）统一生效，与原项目一致。
fn adjust_window_resolution(res_w: u32, res_h: u32, _game_args: &[String]) -> (u32, u32) {
    let (screen_w, screen_h, work_h) = get_screen_size();
    if screen_w == 0 || screen_h == 0 {
        return (res_w, res_h);
    }
    let mut final_w = res_w;
    let mut final_h = res_h;
    // 当用户设置的分辨率接近屏幕分辨率（差距 < 40px）时，窗口客户区等于屏幕大小，
    // 加上标题栏和边框后实际窗口会超出屏幕，导致标题栏被顶到屏幕外不可见。
    // 此时减去标题栏+边框高度（约 39px）和左右边框宽度（约 16px），
    // 让窗口刚好占满屏幕工作区，标题栏正常显示。
    if final_w.abs_diff(screen_w) < 40 {
        final_w = final_w.saturating_sub(16).max(800);
    }
    if final_h.abs_diff(screen_h) < 40 || (work_h > 0 && final_h.abs_diff(work_h) < 40) {
        final_h = final_h.saturating_sub(39).max(600);
    }
    (final_w, final_h)
}

/// 获取主显示器信息：(屏幕宽, 屏幕高, 工作区高)。失败/未知时返回 (0, 0, 0)。
fn get_screen_size() -> (u32, u32, u32) {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetSystemMetrics(nIndex: i32) -> i32;
        }
        const SM_CXSCREEN: i32 = 0;
        const SM_CYSCREEN: i32 = 1;
        const SM_CYWORKAREA: i32 = 49;
        unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            let wh = GetSystemMetrics(SM_CYWORKAREA);
            if w > 0 && h > 0 {
                return (w as u32, h as u32, wh.max(0) as u32);
            }
        }
    }
    (0, 0, 0)
}

/// 确保 --flag 参数为权威值，并清理被空格拆分残留的路径片段。
/// minecraftArguments 模板用 split_whitespace 拆分，含空格的路径会被拆成多段，
/// 这里删除 flag 之后到下一个 -- 选项之间所有片段，再整体插入权威路径。
fn ensure_flag_arg(game_args: &mut Vec<String>, flag: &str, value: &str) {
    let idx = game_args.iter().position(|a| a == flag);
    if let Some(idx) = idx {
        // 删除 flag 之后直到下一个 -- 选项之间所有片段（可能是含空格路径被拆开）
        let mut end = idx + 1;
        while end < game_args.len() && !game_args[end].starts_with("--") {
            end += 1;
        }
        game_args.drain(idx + 1..end);
        game_args.insert(idx + 1, value.to_string());
    } else {
        game_args.push(flag.to_string());
        game_args.push(value.to_string());
    }
}

/// 游戏参数去重：对单值选项去重，保留首次出现的值
/// 复刻原项目 server/versions/version-merge.js:deduplicateGameArgs
fn deduplicate_game_args(args: &[String]) -> Vec<String> {
    if args.is_empty() {
        return Vec::new();
    }
    const SINGLE_VALUE_OPTIONS: &[&str] = &[
        "--version",
        "--username",
        "--uuid",
        "--accessToken",
        "--userType",
        "--versionType",
        "--gameDir",
        "--assetsDir",
        "--assetIndex",
        "--width",
        "--height",
        "--server",
        "--port",
        "--xuid",
        "--clientId",
        "--launchTarget",
        "--fml.forgeVersion",
        "--fml.mcVersion",
        "--fml.forgeGroup",
        "--fml.mcpVersion",
        "--fml.neoForgeVersion",
        "--fml.neoFormVersion",
        "--fml.fmlVersion",
    ];

    let mut result: Vec<String> = Vec::new();
    let mut seen_options: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if SINGLE_VALUE_OPTIONS.contains(&arg.as_str()) {
            if seen_options.contains(arg) {
                // 跳过当前参数和它的值
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                }
                i += 1;
                continue;
            }
            seen_options.insert(arg.clone());
            result.push(arg.clone());
            if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                result.push(args[i + 1].clone());
                i += 1;
            }
        } else {
            result.push(arg.clone());
        }
        i += 1;
    }

    result
}

/// mainJar 是空文件时回退到 Forge patched client.jar
fn fallback_empty_main_jar(
    main_jar_path: &Path,
    version_json: &Value,
    _version_id: &str,
    libraries_dir: &Path,
) -> PathBuf {
    let metadata = match std::fs::metadata(main_jar_path) {
        Ok(m) => m,
        Err(_) => return main_jar_path.to_path_buf(),
    };
    if metadata.len() != 0 {
        return main_jar_path.to_path_buf();
    }
    let game_args = version_json
        .get("arguments")
        .and_then(|v| v.get("game"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let is_forge = game_args.iter().any(|a| a.as_str() == Some("forgeclient"))
        || utils::get_str(version_json, "mainClass")
            .to_lowercase()
            .contains("bootstraplauncher");
    if !is_forge {
        return main_jar_path.to_path_buf();
    }
    let mut fv = String::new();
    let mut mv = String::new();
    for (idx, a) in game_args.iter().enumerate() {
        if a.as_str() == Some("--fml.forgeVersion") && idx + 1 < game_args.len() {
            if let Some(v) = game_args[idx + 1].as_str() {
                fv = v.to_string();
            }
        }
        if a.as_str() == Some("--fml.mcVersion") && idx + 1 < game_args.len() {
            if let Some(v) = game_args[idx + 1].as_str() {
                mv = v.to_string();
            }
        }
    }
    if mv.is_empty() {
        mv = utils::get_str(version_json, "clientVersion");
    }
    if fv.is_empty() || mv.is_empty() {
        return main_jar_path.to_path_buf();
    }
    let patched_jar = libraries_dir
        .join("net")
        .join("minecraftforge")
        .join("forge")
        .join(format!("{}-{}", mv, fv))
        .join(format!("forge-{}-{}-client.jar", mv, fv));
    if patched_jar.exists() {
        if let Ok(meta) = std::fs::metadata(&patched_jar) {
            if meta.len() > 0 {
                return patched_jar;
            }
        }
    }
    main_jar_path.to_path_buf()
}

/// 读取启动器存储中的内存配置
fn resolve_memory_settings(
    settings: &Value,
    version_id: &str,
    data_dir: &Path,
) -> (MemoryMode, Option<u64>) {
    // 优先级 1：版本独立设置（version-settings.json 的 memoryMode/memoryValue）
    let ver_settings = storage::load_version_settings(version_id, false);
    let ver_mode = utils::get_str(&ver_settings, "memoryMode");
    let ver_value = ver_settings.get("memoryValue").and_then(|v| v.as_u64());
    if ver_mode == "custom" {
        return (MemoryMode::Custom(ver_value.unwrap_or(4096)), None);
    }
    if ver_mode == "auto" {
        return (MemoryMode::Auto, None);
    }

    // 优先级 2：前端 launch_settings（store.json 中的 versepc_launch_settings）
    let store_path = data_dir.join("store.json");
    if store_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&store_path) {
            if let Ok(store) = serde_json::from_str::<Value>(&content) {
                if let Some(launch_str) = store.get("versepc_launch_settings").and_then(|v| v.as_str()) {
                    if let Ok(launch_settings) = serde_json::from_str::<Value>(launch_str) {
                        let launch_mode = utils::get_str(&launch_settings, "memoryMode");
                        let launch_value = launch_settings.get("memoryValue").and_then(|v| v.as_u64());
                        if launch_mode == "custom" {
                            return (MemoryMode::Custom(launch_value.unwrap_or(4096)), None);
                        }
                        if launch_mode == "auto" {
                            return (MemoryMode::Auto, None);
                        }
                        // 启动设置存在但无明确模式 → 走 resolve_memory_mode 决策
                        let settings_max = settings.get("maxMemory").and_then(|v| v.as_u64()).unwrap_or(4096);
                        let mode = resolve_memory_mode(settings_max, true, Some("auto"), launch_value);
                        return (mode, None);
                    }
                }
            }
        }
    }

    // 优先级 3：通过 resolve_memory_mode 决策
    let settings_max = settings.get("maxMemory").and_then(|v| v.as_u64()).unwrap_or(4096);
    let mode = resolve_memory_mode(settings_max, false, None, None);
    (mode, None)
}
