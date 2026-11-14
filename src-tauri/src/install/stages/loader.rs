// install/stages/loader.rs — 模组加载器阶段
// 职责：按类型安装 Forge/NeoForge/Fabric/OptiFine，并处理可选的 Fabric API。

use serde_json::json;

use crate::install::context::{InstallCtx, StageError};
use crate::install::session;
use crate::modloaders;

/// 执行模组加载器阶段
pub(crate) async fn run(ctx: &InstallCtx) -> Result<(), StageError> {
    let Some(loader) = &ctx.loader_info else {
        return Ok(());
    };
    let loader_type = loader.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let loader_version = loader
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if loader_type.is_empty() || loader_version.is_empty() {
        return Ok(());
    }

    ctx.update(
        session::InstallStage::Loader,
        5,
        "准备安装模组加载器...",
    );
    let loader_display = if loader_type == "neoforge" {
        "NeoForge".to_string()
    } else {
        let mut s = loader_type.to_string();
        if !s.is_empty() {
            s[0..1].make_ascii_uppercase();
        }
        s
    };
    ctx.update(
        session::InstallStage::Loader,
        15,
        &format!("正在安装{}模组加载器...", loader_display),
    );

    let game_version = ctx.game_version().to_string();
    // 把安装器内部的耗时步骤（下载支持库 / 运行安装器）反馈到安装进度，
    // 避免长时间停在同一百分比让用户以为卡死（PCL 同样显示当前子步骤）。
    let progress_cb: modloaders::shared::LoaderProgress = {
        let app = ctx.app.clone();
        let sid = ctx.session_id.clone();
        std::sync::Arc::new(move |pct: u32, msg: &str| {
            session::update_session(&app, &sid, |s| {
                s.progress = crate::install::context::calc_progress(&session::InstallStage::Loader, pct);
                s.stage = session::InstallStage::Loader;
                s.message = msg.to_string();
            });
        })
    };
    let progress = Some(&progress_cb);
    let mut result = json!({ "success": false, "error": "未知加载器类型" });
    match loader_type {
        "forge" => {
            result = modloaders::forge::install_forge(
                &game_version,
                loader_version,
                Some(&ctx.actual_version_id),
                progress,
            )
            .await;
        }
        "neoforge" => {
            result = modloaders::neoforge::install_neoforge(
                &game_version,
                loader_version,
                Some(&ctx.actual_version_id),
                progress,
            )
            .await;
        }
        "fabric" => {
            result = modloaders::fabric::install_fabric_with_target(
                &game_version,
                loader_version,
                &ctx.actual_version_id,
            )
            .await;
        }
        "optifine" => {
            result = modloaders::optifine::install_optifine(
                &game_version,
                loader_version,
                Some(&ctx.actual_version_id),
            )
            .await;
        }
        _ => {}
    }

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 记录加载器安装日志到文件，便于排查失败原因
    {
        let log_dir = ctx.data_dir.join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("loader-install.log");
        let line = format!(
            "[{}] type={} version={} game={} target={} success={} error={}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            loader_type,
            loader_version,
            game_version,
            ctx.actual_version_id,
            success,
            result.get("error").and_then(|v| v.as_str()).unwrap_or("")
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(f, "{}", line);
        }
    }

    if !success {
        let err = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("模组加载器安装失败")
            .to_string();
        ctx.cleanup_new_version_dir();
        return Err(ctx.fail(format!("{}安装失败: {}", loader_display, err), Some(err)));
    }
    ctx.update(
        session::InstallStage::Loader,
        100,
        "模组加载器安装完成",
    );

    // 加载器已把原版内容合并进目标实例：清理本次安装新建的原版目录。
    // 用户此前已安装的原版目录（version_dir_was_new=false）保持不动。
    if ctx.version_dir_was_new() && !game_version.is_empty() && game_version != ctx.actual_version_id
    {
        modloaders::shared::cleanup_orphan_vanilla(&game_version);
    }

    // Fabric API 自动安装：detail 流程选择 Fabric 加载器时可选一并安装 Fabric API
    if loader_type == "fabric" {
        let api_id = loader
            .get("fabricApiId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !api_id.is_empty() {
            ctx.update(session::InstallStage::Loader, 100, "正在安装 Fabric API...");
            let api_url = loader
                .get("fabricApiUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let api_filename = loader
                .get("fabricApiFilename")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let api_result = crate::modloaders::fabric_api::install_fabric_api(
                &game_version,
                api_id,
                Some(&ctx.actual_version_id),
                if api_url.is_empty() {
                    None
                } else {
                    Some(api_url)
                },
                if api_filename.is_empty() {
                    None
                } else {
                    Some(api_filename)
                },
            )
            .await;
            if !api_result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                let api_err = api_result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Fabric API 安装失败")
                    .to_string();
                return Err(ctx.fail(format!("Fabric API 安装失败: {}", api_err), Some(api_err)));
            }
        }
    }

    if ctx.is_cancelled() {
        return Err(ctx.cancelled());
    }
    Ok(())
}
