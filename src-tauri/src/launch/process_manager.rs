// launch/process_manager.rs — 游戏进程管理
// 职责：启动子进程、收集日志、状态上报
//
// 当前实现：
// - do_launch：用 tokio::process::Command 启动 java + 启动参数，并行收集 stdout/stderr
// - kill_game：通过 session_id 终止游戏进程
// - list_running_games：列出当前运行中的游戏
// - 日志通过 Tauri 事件 "game-log" 推送给前端，替代 SSE

use serde_json::{json, Value};
use std::process::Stdio;
use tauri::{AppHandle, Emitter};

use super::args_builder::build_launch_arguments;
use super::exit_code::analyze_exit_code;
use super::game_session::{add_instance, remove_instance, GameInstance};

/// 启动游戏
/// 参数：version_id、account、settings、custom_game_dir、external_version_dir
/// `app` 用于通过 Tauri 事件向前端推送日志，前端监听 "game-log" 事件即可拿到实时日志
pub async fn do_launch(
    app: AppHandle,
    version_id: String,
    version_json: Value,
    settings: Value,
    account: Value,
    custom_game_dir: Option<String>,
    external_version_dir: Option<String>,
) -> Result<String, String> {
    let external_path = external_version_dir.as_ref().map(std::path::PathBuf::from);
    let launch_args = build_launch_arguments(
        &version_json,
        &settings,
        &account,
        &version_id,
        custom_game_dir.as_deref(),
        external_path.as_deref(),
    );

    // 选择 Java 路径
    let java_path = super::dep_check::select_java_for_version(&version_id, &settings, &version_json);
    let java_path = if java_path.is_empty() {
        "java".to_string()
    } else {
        java_path
    };

    // 启动子进程
    let mut cmd = tokio::process::Command::new(&java_path);
    cmd.args(&launch_args.args);

    // 设置工作目录为游戏目录
    let game_dir = launch_args
        .args
        .iter()
        .position(|a| a == "--gameDir")
        .and_then(|idx| launch_args.args.get(idx + 1).cloned())
        .unwrap_or_default();
    if !game_dir.is_empty() {
        cmd.current_dir(&game_dir);
    }

    // 启动前设置游戏语言（简体中文）与窗口模式（全屏/窗口化），失败不阻塞启动
    set_game_language_and_window(&game_dir, &version_json, &settings);

    // 标准输出/错误管道
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Windows 上不要让子进程共享控制台
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // 启动失败时写调试日志：成功启动不写，避免用户在 data/logs 目录看到"debug 记事本"
    // 如果启动失败，在 spawn 错误和 3 秒内退出两个分支写日志

    let mut child = cmd
        .spawn()
        .map_err(|e| {
            let data_dir = crate::storage::resolve_data_dir();
            let debug_log = data_dir.join("logs").join("launch-debug.log");
            let _ = std::fs::create_dir_all(debug_log.parent().unwrap_or(std::path::Path::new(".")));
            let cmd_str = format!("[启动失败] Java: {}\nGameDir: {}\nArgs ({}):\n  {}\n\n错误: {}",
                java_path,
                game_dir,
                launch_args.args.len(),
                launch_args.args.join("\n  "),
                e
            );
            let _ = std::fs::write(&debug_log, cmd_str);
            format!("启动游戏失败: {}", e)
        })?;

    let pid = child.id().unwrap_or(0);
    let session_id = format!("game_{}", pid);
    let main_class = launch_args
        .args
        .iter()
        .rev()
        .find(|a| !a.starts_with('-') && !a.contains('=') && !a.contains('/'))
        .cloned()
        .unwrap_or_default();

    // 为"3 秒内退出"分支保留调试用副本（game_dir/java_path 随后会被 move 进实例）
    let debug_game_dir = game_dir.clone();
    let debug_java_path = java_path.clone();

    let instance = GameInstance::new(
        session_id.clone(),
        version_id.clone(),
        pid,
        game_dir,
        java_path,
        main_class,
    );
    add_instance(instance);

    // 通知前端：游戏进程已启动
    let _ = app.emit(
        "game-status",
        json!({
            "event": "launched",
            "sessionId": &session_id,
            "versionId": &version_id,
            "pid": pid
        }),
    );

    // 等待 3 秒，检查进程是否立即退出（崩溃）
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        child.wait(),
    ).await {
        Ok(status_result) => {
            // 进程在 3 秒内退出，说明启动失败
            let code = status_result.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let data_dir = crate::storage::resolve_data_dir();
            let debug_log = data_dir.join("logs").join("launch-debug.log");

            // 尝试读取 stderr 输出
            let mut stderr_text = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                use tokio::io::AsyncReadExt;
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    stderr.read_to_string(&mut stderr_text),
                ).await;
            }
            let mut stdout_text = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                use tokio::io::AsyncReadExt;
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    stdout.read_to_string(&mut stdout_text),
                ).await;
            }

            let exit_msg = format!(
                "[启动失败] Java: {}\nGameDir: {}\nArgs ({}):\n  {}\n\n=== 进程在 3 秒内退出 ===\n退出码: {}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
                debug_java_path,
                debug_game_dir,
                launch_args.args.len(),
                launch_args.args.join("\n  "),
                code, stdout_text, stderr_text
            );
            let _ = std::fs::write(&debug_log, exit_msg);

            // 移除实例
            let _ = remove_instance(&session_id);

            // 推送退出事件
            let _ = app.emit(
                "game-exit",
                json!({
                    "sessionId": &session_id,
                    "versionId": &version_id,
                    "code": code,
                    "reason": "游戏进程启动后立即退出",
                    "suggestion": "请查看 launch-debug.log 获取详细启动参数",
                    "isCrash": true
                }),
            );

            // 写入全局退出分析缓存（启动即退也记录，供崩溃卡片 / 日志导出使用）
            let exit_logs = super::game_session::get_persistent_logs(50);
            super::game_session::set_exit_analysis(json!({
                "code": code,
                "reason": "游戏进程启动后立即退出",
                "suggestion": "请查看 launch-debug.log 获取详细启动参数",
                "isCrash": true,
                "versionId": version_id.clone(),
                "launchInfo": {
                    "versionId": version_id.clone(),
                    "fullVersionId": version_id.clone()
                },
                "logBuffer": exit_logs
            }));

            return Err(format!("游戏进程启动后立即退出（退出码: {}），请检查 Java 版本和游戏文件是否完整", code));
        }
        Err(_) => {
            // 3 秒内未退出，进程仍在运行，正常继续
        }
    }

    // 在独立任务中收集日志（不阻塞调用方）
    let session_id_for_log = session_id.clone();
    let version_id_for_log = version_id.clone();
    let data_dir = crate::storage::resolve_data_dir();
    let app_for_log = app.clone();
    tokio::spawn(async move {
        // 取出 stdout/stderr 管道
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // 启动两个任务并行读取 stdout/stderr
        // stdout 任务：日志按行处理，每行推送 "game-log" 事件给前端
        let session_for_stdout = session_id_for_log.clone();
        let app_for_stdout = app_for_log.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(out) = stdout {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let mut reader = BufReader::new(out);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line
                                .trim_end_matches('\n')
                                .trim_end_matches('\r')
                                .to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            // 写入实例日志缓冲（用于 GET /api/game/log 轮询兜底）
                            super::game_session::update_instance(&session_for_stdout, |inst| {
                                inst.log_buffer.push(trimmed.clone());
                                if inst.log_buffer.len() > 5000 {
                                    let extra = inst.log_buffer.len() - 5000;
                                    inst.log_buffer.drain(0..extra);
                                }
                                // 阶段识别（5 个关键节点）
                                if inst.load_stage < 1 {
                                    inst.load_stage = 1;
                                }
                                if inst.load_stage < 2 && trimmed.contains("Setting user:") {
                                    inst.load_stage = 2;
                                }
                                if inst.load_stage < 3
                                    && trimmed.to_lowercase().contains("lwjgl version")
                                {
                                    inst.load_stage = 3;
                                }
                                if inst.load_stage < 4
                                    && (trimmed.contains("OpenAL initialized")
                                        || trimmed.contains("Starting up SoundSystem"))
                                {
                                    inst.load_stage = 4;
                                }
                                if inst.load_stage < 5
                                    && (trimmed.contains("Created")
                                        && trimmed.contains("textures")
                                        && trimmed.contains("-atlas"))
                                    || trimmed.contains("Found animation info")
                                {
                                    inst.load_stage = 5;
                                    if !inst.game_ready {
                                        inst.game_ready = true;
                                        inst.ready_time = Some(std::time::Instant::now());
                                    }
                                }
                                // 检测局域网联机端口（多种日志格式）
                                if let Some(port) = extract_lan_port(&trimmed) {
                                    inst.lan_port = Some(port);
                                }
                            });
                            // 写入全局持久化日志缓冲（实例移除后仍可导出）
                            super::game_session::push_persistent_log(trimmed.clone());
                            // 通过 Tauri 事件实时推送日志（替代 SSE）
                            let _ = app_for_stdout.emit(
                                "game-log",
                                json!({
                                    "sessionId": &session_for_stdout,
                                    "line": &trimmed,
                                }),
                            );
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        // stderr 任务：同样写入缓冲并通过事件推送
        let session_for_stderr = session_id_for_log.clone();
        let app_for_stderr = app_for_log.clone();
        let stderr_task = tokio::spawn(async move {
            if let Some(err) = stderr {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let mut reader = BufReader::new(err);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line
                                .trim_end_matches('\n')
                                .trim_end_matches('\r')
                                .to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            super::game_session::update_instance(&session_for_stderr, |inst| {
                                inst.log_buffer.push(trimmed.clone());
                                if inst.log_buffer.len() > 5000 {
                                    let extra = inst.log_buffer.len() - 5000;
                                    inst.log_buffer.drain(0..extra);
                                }
                            });
                            // 写入全局持久化日志缓冲（实例移除后仍可导出）
                            super::game_session::push_persistent_log(trimmed.clone());
                            let _ = app_for_stderr.emit(
                                "game-log",
                                json!({
                                    "sessionId": &session_for_stderr,
                                    "line": &trimmed,
                                }),
                            );
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        let _ = tokio::join!(stdout_task, stderr_task);

        // 等待进程退出
        let status = child.wait().await;
        let exit_code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
        // 分析退出码
        let analysis = analyze_exit_code(exit_code, &version_id_for_log, &data_dir);

        // 通过事件推送退出分析（前端可据此展示崩溃提示）
        let _ = app_for_log.emit(
            "game-exit",
            json!({
                "sessionId": &session_id_for_log,
                "versionId": &version_id_for_log,
                "code": exit_code,
                "reason": &analysis.reason,
                "suggestion": &analysis.suggestion,
                "isCrash": analysis.is_crash
            }),
        );

        // 写入全局退出分析缓存（供 GET /api/game/exit-analysis 轮询与日志导出使用）
        let exit_logs = super::game_session::get_persistent_logs(50);
        super::game_session::set_exit_analysis(json!({
            "code": exit_code,
            "reason": analysis.reason.clone(),
            "suggestion": analysis.suggestion.clone(),
            "isCrash": analysis.is_crash,
            "versionId": version_id_for_log.clone(),
            "launchInfo": {
                "versionId": version_id_for_log.clone(),
                "fullVersionId": version_id_for_log.clone()
            },
            "logBuffer": exit_logs
        }));

        eprintln!(
            "[Game] {} exited code={} reason={} crash={}",
            version_id_for_log, exit_code, analysis.reason, analysis.is_crash
        );

        // 移除实例
        let _ = remove_instance(&session_id_for_log);
    });

    Ok(session_id)
}

/// 启动前设置游戏语言（简体中文）与窗口模式（全屏/窗口化）。
/// 失败不阻塞启动流程。
fn set_game_language_and_window(game_dir: &str, version_json: &Value, settings: &Value) {
    let game_dir = std::path::Path::new(game_dir);

    // ===== 语言设置 =====
    let auto_set_chinese = settings
        .get("autoSetChinese")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if auto_set_chinese {
        let mut options_path = game_dir.join("options.txt");
        if !options_path.exists() {
            // Yosbr Mod 会将配置前置到 config/yosbr 目录
            let yosbr = game_dir.join("config").join("yosbr").join("options.txt");
            if yosbr.exists() {
                options_path = yosbr;
            }
        }

        // 通过版本发布时间确定所需的语言代码格式（旧版用 zh_CN，1.11+ 用 zh_cn）
        let release_time = version_json
            .get("releaseTime")
            .or_else(|| version_json.get("time"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // 版本时间无法判断时兜底为简体中文，确保语言非空，避免模组因空语言崩溃
        let required_lang = {
            let l = required_chinese_lang(release_time);
            if l.is_empty() { "zh_cn".to_string() } else { l }
        };

        let mut options_content = String::new();
        if options_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&options_path) {
                options_content = content;
            }
        }
        let current_lang = options_content
            .lines()
            .find(|l| l.starts_with("lang:"))
            .map(|l| l["lang:".len()..].trim().to_string())
            .unwrap_or_else(|| "none".to_string());

        // 语言未设置（为空值或标记为 none）时视为需要修复
        let lang_is_unset = current_lang.is_empty() || current_lang == "none";
        let has_lang_line = options_content.lines().any(|l| l.starts_with("lang:"));

        // 已有存档且语言已显式设置（非空）时保留用户选择，避免覆盖
        let has_existing_saves = game_dir.join("saves").exists();
        let preserve = !lang_is_unset && has_existing_saves;

        if current_lang != required_lang && !preserve {
            if has_lang_line {
                // 替换已有 lang 行（含空值，避免出现重复 lang 行）
                let mut lines: Vec<String> = options_content
                    .lines()
                    .map(|l| l.to_string())
                    .collect();
                for line in lines.iter_mut() {
                    if line.starts_with("lang:") {
                        *line = format!("lang:{}", required_lang);
                    }
                }
                options_content = lines.join("\n");
            } else if !options_content.is_empty() {
                options_content = format!("{}\nlang:{}", options_content.trim_end(), required_lang);
            } else {
                options_content = format!("lang:{}\n", required_lang);
            }
        }

        if let Some(parent) = options_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&options_path, options_content);
    }

    // ===== 窗口模式（全屏/窗口化） =====
    let fullscreen = settings
        .get("fullscreen")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fullscreen_value = if fullscreen { "true" } else { "false" };
    let main_options = game_dir.join("options.txt");
    let mut targets = vec![main_options];
    let yosbr = game_dir.join("config").join("yosbr").join("options.txt");
    if yosbr.exists() {
        targets.push(yosbr);
    }
    for options_path in &targets {
        let mut content = String::new();
        if options_path.exists() {
            if let Ok(c) = std::fs::read_to_string(options_path) {
                content = c;
            }
        }
        if content.lines().any(|l| l.starts_with("fullscreen:")) {
            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            for line in lines.iter_mut() {
                if line.starts_with("fullscreen:") {
                    *line = format!("fullscreen:{}", fullscreen_value);
                }
            }
            content = lines.join("\n");
        } else if !content.is_empty() {
            content = format!("{}\nfullscreen:{}", content.trim_end(), fullscreen_value);
        } else {
            content = format!("fullscreen:{}\n", fullscreen_value);
        }
        if let Some(parent) = options_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(options_path, content);
    }
}

/// 根据版本发布日期确定所需的中文语言代码（旧版 zh_CN，1.11+ zh_cn）。
/// 版本早于 1.1（2012-01-12）时返回空字符串，表示不自动设置中文。
fn required_chinese_lang(release_time: &str) -> String {
    let (r_y, r_m, r_d) = parse_date(release_time);
    if r_y == 0 {
        return "zh_cn".to_string();
    }
    // 1.1: 2012-01-12
    if (r_y, r_m, r_d) <= (2012, 1, 12) {
        return String::new();
    }
    // 1.11: 2016-06-08
    if (r_y, r_m, r_d) <= (2016, 6, 8) {
        return "zh_CN".to_string();
    }
    "zh_cn".to_string()
}

/// 解析 "YYYY-MM-DD"（或含时间）为 (年, 月, 日)，失败返回 (0,0,0)。
fn parse_date(s: &str) -> (u32, u32, u32) {
    let s = s.trim();
    let ymd: Vec<&str> = s.split(['-', 'T', ' ']).collect();
    if ymd.len() >= 3 {
        let y = ymd[0].parse().unwrap_or(0);
        let m = ymd[1].parse().unwrap_or(0);
        let d = ymd[2].parse().unwrap_or(0);
        if y > 0 && m > 0 && d > 0 {
            return (y, m, d);
        }
    }
    (0, 0, 0)
}

/// 从日志行中提取局域网联机端口
/// 匹配多种日志格式：
///   "Local game hosted on 12345"
///   "Started serving on 54321"
///   "Opening LAN server 11111"
///   "本地游戏已托管 12345"
fn extract_lan_port(line: &str) -> Option<u16> {
    let patterns: &[&str] = &[
        "Local game hosted on",
        "Started serving on",
        "Opening LAN server",
        "LAN server started",
        "本地游戏已托管",
    ];
    for pat in patterns {
        if let Some(idx) = line.find(pat) {
            let rest = &line[idx + pat.len()..];
            let digits: String = rest
                .chars()
                .skip_while(|c| c.is_whitespace() || *c == ':')
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(port) = digits.parse::<u16>() {
                if port >= 1000 {
                    return Some(port);
                }
            }
        }
    }
    None
}

/// 通过 PID 终止游戏进程
pub fn kill_game(session_id: &str) -> Result<(), String> {
    if let Some(instance) = remove_instance(session_id) {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &instance.pid.to_string(), "/F", "/T"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill")
                .args(["-9", &instance.pid.to_string()])
                .output();
        }
        Ok(())
    } else {
        Err(format!("游戏实例 {} 不存在", session_id))
    }
}

/// 列出当前运行中的游戏
pub fn list_running_games() -> Vec<Value> {
    super::game_session::get_all_status()
}
