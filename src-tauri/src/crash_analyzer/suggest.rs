// crash_analyzer/suggest.rs — 修复建议生成
// 职责：根据 crashReasons Map 逐条生成中文修复建议文本

use std::collections::HashMap;

use super::constants::CrashReason;

/// 汇总分析结果：按崩溃原因逐条生成修复建议
/// is_hand_analyze: 是否手动导入分析（影响未命中时的提示文案）
pub fn get_analyze_result(
    reasons: &HashMap<CrashReason, Vec<String>>,
    is_hand_analyze: bool,
) -> String {
    if reasons.is_empty() {
        if is_hand_analyze {
            return "分析完成：VersePC 无法确定崩溃原因。".to_string();
        }
        return "很抱歉，我们未能分析出该日志中的崩溃原因。\n如果你认为这应当被分析出，请提交反馈。"
            .trim()
            .to_string();
    }

    // 按迭代顺序处理（CrashReason enum 已定义顺序）
    // HashMap 迭代顺序不确定，这里先收集排序，保证输出稳定
    let mut sorted_reasons: Vec<(CrashReason, Vec<String>)> = reasons
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    sorted_reasons.sort_by_key(|(r, _)| reason_order(r));

    let mut results: Vec<String> = Vec::new();
    for (reason, additional) in &sorted_reasons {
        let suggestion = match reason {
            CrashReason::JavaVersionTooHigh => {
                "当前 Java 版本过高，请降低 Java 版本后再试。\n请下载安装 Java 8 或 Java 11。".to_string()
            }
            CrashReason::ModFileExtracted => {
                "发现 Mod 文件被解压，请删除解压后的文件夹。\n请直接把 Mod 的 .jar 文件放进 Mod 文件夹，不要解压它。".to_string()
            }
            CrashReason::MixinBootstrapError => {
                "MixinBootstrap 错误，请尝试更新或移除相关 Mod。".to_string()
            }
            CrashReason::OutOfMemory => {
                "Minecraft 内存不足，请尝试增加游戏内存。\n如果仍然崩溃，可能是 Mod 过多或资源包过大导致的内存不足。\n\n建议：\n - 如果安装了过多 Mod，请尝试删除一些不必要的 Mod。\n - 如果使用了高分辨率资源包，请尝试使用更低分辨率的资源包。\n - 如果内存仍然不足，请尝试增加游戏内存（通常 4GB-8GB 足够）。".to_string()
            }
            CrashReason::UsingJDK => {
                "你正在使用 JDK 而不是 JRE，这可能导致游戏崩溃。\n请下载安装 Java 运行时环境（JRE）而不是 Java 开发工具包（JDK）。".to_string()
            }
            CrashReason::UsingOpenJ9 => {
                "你正在使用 OpenJ9 Java，这可能导致游戏崩溃。\n请下载安装 Java 8 或 Java 11 的 HotSpot VM 版本。".to_string()
            }
            CrashReason::JavaTooOld => {
                "Java 版本过旧，请更新 Java。\n请下载安装最新版本的 Java 8 或 Java 11。".to_string()
            }
            CrashReason::ModDuplicateModFiles => {
                "发现重复的 Mod 文件，请删除重复的 Mod。\n请检查 Mod 文件夹，确保每个 Mod 只有一个文件。".to_string()
            }
            CrashReason::ModRequiresJava11 => {
                "某些 Mod 需要 Java 11，请下载安装 Java 11。\n请在启动设置中将 Java 版本切换为 Java 11。".to_string()
            }
            CrashReason::ModMissingDependency => {
                if !additional.is_empty() {
                    let list = additional
                        .iter()
                        .map(|a| format!(" - {}", a))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "发现缺少前置或版本不兼容的 Mod，请安装或更新以下前置 Mod：\n{}\n\n请安装缺少的前置 Mod 或更新到兼容的版本。",
                        list
                    )
                } else {
                    "发现缺少前置或版本不兼容的 Mod，请检查日志文件中的详细信息。\n请安装缺少的前置 Mod 或更新到兼容的版本。".to_string()
                }
            }
            CrashReason::ModIncompatible => build_mod_list_suggestion(
                "VersePC 发现以下 Mod 可能导致崩溃：",
                "请尝试删除或更新这些 Mod。",
                additional,
            ),
            CrashReason::ModCrashed => build_mod_list_suggestion(
                "发现以下 Mod 导致崩溃：",
                "请尝试删除或更新这些 Mod。",
                additional,
            ),
            CrashReason::ModNoInfo => build_mod_list_suggestion(
                "发现以下 Mod 导致崩溃，但无法获取详细信息：",
                "请尝试删除或更新这些 Mod。",
                additional,
            ),
            CrashReason::ModMixinError => {
                if additional.is_empty() {
                    "检测到 Mod Mixin 错误，请尝试更新或移除相关 Mod。\n通常这是因为 Mod 版本不兼容或 Mod 本身存在问题。".to_string()
                } else {
                    build_mod_list_suggestion(
                        "发现以下 Mod 的 Mixin 出错：",
                        "请尝试更新或移除这些 Mod。",
                        additional,
                    )
                }
            }
            CrashReason::ModNameContainsSpecialChars => build_mod_list_suggestion(
                "发现以下 Mod 名称包含特殊字符：",
                "请重命名这些 Mod 文件，移除特殊字符。",
                additional,
            ),
            CrashReason::ModNameDuplicate => {
                "发现 Mod 名称重复，请检查并重命名 Mod 文件。\nMod 的文件名不能完全相同，即使它们位于不同的文件夹中。".to_string()
            }
            CrashReason::OptiFineIncompatible => {
                "发现 OptiFine 不兼容，请更新 OptiFine 或删除它。\nOptiFine 可能与当前版本的 Minecraft 或 Forge 不兼容。".to_string()
            }
            CrashReason::ShadersModWithOptiFine => {
                "发现 Shaders Mod 与 OptiFine 冲突，请删除 Shaders Mod。\nOptiFine 已内置光影支持，不需要额外的 Shaders Mod。".to_string()
            }
            CrashReason::ForgeMissing => {
                "发现 Forge 缺失，请重新安装 Forge。\n可能是 Forge 文件损坏或未正确安装。".to_string()
            }
            CrashReason::FabricCrash => {
                if additional.len() == 1 {
                    format!("Fabric Mod {} 导致崩溃，请尝试删除或更新该 Mod。", additional[0])
                } else {
                    "Fabric Mod 崩溃，请检查日志文件中的详细信息。\n请尝试删除或更新最近安装的 Fabric Mod。".to_string()
                }
            }
            CrashReason::ForgeCrash => {
                if additional.len() == 1 {
                    format!("Forge Mod {} 导致崩溃，请尝试删除或更新该 Mod。", additional[0])
                } else {
                    "Forge Mod 崩溃，请检查日志文件中的详细信息。\n请尝试删除或更新最近安装的 Forge Mod。".to_string()
                }
            }
            CrashReason::ModLoaderVersionIncompatible => {
                "Mod 加载器版本与 Mod 不兼容，请更新或降级加载器版本。\n请检查 Mod 的要求，并安装相应版本的 Forge 或 Fabric。".to_string()
            }
            CrashReason::NightConfigBug => {
                "发现 Night Config Bug，这是 Minecraft 的一个已知问题。\n请尝试更新 Forge 或删除相关配置文件。".to_string()
            }
            CrashReason::OpenGL1282Error => {
                "发现 OpenGL 1282 错误，这通常与显卡驱动有关。\n请尝试更新显卡驱动或降低游戏图形设置。".to_string()
            }
            CrashReason::ModIdConflict => {
                if additional.len() == 1 {
                    format!("发现 Mod ID 冲突：{}\n请删除其中一个冲突的 Mod。", additional[0])
                } else if !additional.is_empty() {
                    let list = additional
                        .iter()
                        .map(|a| format!(" - {}", a))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("发现以下 Mod ID 冲突：\n{}\n\n请删除其中一个冲突的 Mod。", list)
                } else {
                    "发现 Mod ID 冲突，请删除其中一个冲突的 Mod。".to_string()
                }
            }
            CrashReason::InvalidPath => {
                "发现无效路径，请检查游戏安装路径。\n游戏路径中不能包含特殊字符或过长的路径。".to_string()
            }
            CrashReason::ModCyclicIssue => {
                "发现 Mod 循环依赖问题，请检查 Mod 的依赖关系。\n某些 Mod 可能相互依赖，导致无法加载。".to_string()
            }
            CrashReason::SecurityException => {
                "发现安全异常，请检查 Java 安全设置。\n可能是 Java 安全策略限制了某些操作。".to_string()
            }
            CrashReason::NativeLinkError => {
                let need_path_check = additional
                    .first()
                    .map(|s| s != "请检查游戏路径是否包含中文字符")
                    .unwrap_or(false);
                if need_path_check {
                    format!(
                        "无法加载本地库 {}。\n请检查游戏路径是否包含中文字符，或尝试重新安装整合包。\n如果是 Forge 整合包，可以在启动器中重新安装 Forge。",
                        additional[0]
                    )
                } else {
                    "无法加载本地库（LWJGL Native），游戏路径可能包含中文字符。\n请将游戏移动到纯英文路径下，或在设置中修复游戏目录。".to_string()
                }
            }
            CrashReason::IntelDriverCrash
            | CrashReason::AMDDriverCrash
            | CrashReason::NVidiaDriverCrash => {
                "发现显卡驱动崩溃，请尝试更新显卡驱动。\n如果问题仍然存在，请尝试降低游戏图形设置或使用 Fast 模式而不是 Fancy 模式。".to_string()
            }
            CrashReason::PixelFormatNotAccelerated => {
                "发现像素格式未加速错误，这通常与显卡驱动有关。\n请尝试更新显卡驱动或降低游戏图形设置。".to_string()
            }
            CrashReason::ManuallyTriggeredCrash => {
                "发现手动触发的崩溃，这通常是为了测试目的。\n如果你不是故意触发此崩溃，请检查你的操作。".to_string()
            }
            CrashReason::OpenJ9Crash => {
                "检测到 OpenJ9 崩溃，请更换 Java 运行时环境。\n请下载安装 Java 8 或 Java 11 的 HotSpot VM 版本。".to_string()
            }
            CrashReason::OptiFineMissingForge => {
                "发现 OptiFine 缺少 Forge，请安装 Forge 后再使用 OptiFine。\nOptiFine 需要 Forge 作为前置才能正常运行。".to_string()
            }
            CrashReason::FabricModCrash => {
                if additional.len() == 1 {
                    format!("Fabric Mod {} 导致崩溃，请尝试删除或更新该 Mod。", additional[0])
                } else {
                    "Fabric Mod 崩溃，请检查日志文件中的详细信息。\n请尝试删除或更新最近安装的 Fabric Mod。".to_string()
                }
            }
            CrashReason::ModMissingOrIncompatible => build_mod_list_suggestion(
                "发现以下 Mod 缺失或不兼容：",
                "请尝试删除或更新这些 Mod。",
                additional,
            ),
            CrashReason::JavaArgsError => {
                "Java 虚拟机参数有误，请检查启动参数设置。\n可能是使用了当前 Java 不支持的启动参数。".to_string()
            }
            CrashReason::GpuNoOpenGL => {
                "检测到显卡不支持 OpenGL，可能是显卡驱动问题。\n请更新显卡驱动，或尝试关闭独立显卡、使用核显启动游戏。".to_string()
            }
            CrashReason::JavaVersionIncompatible => {
                "Java 版本与当前游戏不兼容，请更换 Java 版本。\n请根据游戏使用的加载器要求选择正确的 Java 版本。".to_string()
            }
            CrashReason::Java32Bit => {
                "你正在使用 32 位 Java，这会导致 JVM 无法分配足够的内存。\n请安装并选择 64 位 Java。".to_string()
            }
            CrashReason::ModConfigCrash => build_mod_list_suggestion(
                "Mod 配置文件导致游戏崩溃：",
                "请删除相关 Mod 的配置文件后再试。",
                additional,
            ),
            CrashReason::ModLoaderError => {
                "Mod 加载器报错，请检查 Mod 加载器的安装情况。\n请尝试重新安装或更换 Mod 加载器版本。".to_string()
            }
            CrashReason::ModInitError => build_mod_list_suggestion(
                "以下 Mod 初始化失败：",
                "请尝试删除或更新这些 Mod。",
                additional,
            ),
            CrashReason::StackKeywordFound => build_mod_list_suggestion(
                "崩溃日志堆栈中检测到以下相关关键字：",
                "这可能与对应 Mod 有关，请尝试更新或移除相关 Mod。",
                additional,
            ),
            CrashReason::StackModNameFound => build_mod_list_suggestion(
                "堆栈分析发现以下 Mod 可能导致崩溃：",
                "请尝试更新或移除这些 Mod。",
                additional,
            ),
            CrashReason::SuspectedModCrash => build_mod_list_suggestion(
                "崩溃报告中怀疑以下 Mod 导致了崩溃：",
                "请尝试更新或移除这些 Mod。",
                additional,
            ),
            CrashReason::ShortLogOutput => {
                "程序输出极短即退出，没有记录有效的崩溃信息。\n请尝试重新启动游戏，或检查 Java 环境是否正常。".to_string()
            }
            CrashReason::BlockCrash => {
                if !additional.is_empty() {
                    format!("特定方块导致崩溃：{}\n请尝试移除相关 Mod 或避免在该方块附近活动。", additional[0])
                } else {
                    "特定方块导致崩溃，可能与相关 Mod 有关。\n请尝试移除相关 Mod。".to_string()
                }
            }
            CrashReason::EntityCrash => {
                if !additional.is_empty() {
                    format!("特定实体导致崩溃：{}\n请尝试移除相关 Mod 或避免与该实体交互。", additional[0])
                } else {
                    "特定实体导致崩溃，可能与相关 Mod 有关。\n请尝试移除相关 Mod。".to_string()
                }
            }
            CrashReason::MultipleForge => {
                "版本 JSON 中存在多个 Forge，请重新安装 Forge 修复此问题。".to_string()
            }
            CrashReason::TooManyMods => {
                "Mod 过多导致超出 ID 限制，请删除一些 Mod 后再试。".to_string()
            }
            CrashReason::NoAnalysisFiles => {
                "未找到可用的日志文件，无法分析崩溃原因。\n请先启动游戏触发崩溃，或手动选择日志文件后再分析。".to_string()
            }
            CrashReason::TextureTooLarge => {
                "材质过大或显卡配置不足，请尝试使用更低分辨率的资源包，或降低游戏图形设置。".to_string()
            }
            CrashReason::OptiFineCannotLoadWorld => {
                "发现 OptiFine 导致无法加载世界，请更新 OptiFine 或删除它。\n可能是 OptiFine 与当前版本不兼容。".to_string()
            }
            CrashReason::Unknown => {
                if !additional.is_empty() {
                    format!("发现未知错误：{}", additional[0])
                } else {
                    "发现未知错误，请检查日志文件中的详细信息。".to_string()
                }
            }
        };
        results.push(suggestion);
    }

    results.join("\n\n").trim().to_string()
}

/// 构建 "Mod 列表" 风格的建议文本
/// 单条用 "X Mod 导致崩溃"，多条用列表
fn build_mod_list_suggestion(
    header: &str,
    footer: &str,
    additional: &[String],
) -> String {
    if additional.len() == 1 {
        format!("{}{}", header.trim_end_matches('：'), additional[0])
    } else if additional.is_empty() {
        format!("{}\n{}", header, footer)
    } else {
        let list = additional
            .iter()
            .map(|a| format!(" - {}", a))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{}\n{}\n\n{}", header, list, footer)
    }
}

/// 为排序提供稳定顺序（按原 CrashReason 枚举定义顺序）
fn reason_order(reason: &CrashReason) -> u32 {
    reason_order_pub(reason)
}

/// 公开版本，供 mod.rs 调用
pub fn reason_order_pub(reason: &CrashReason) -> u32 {
    match reason {
        CrashReason::JavaVersionTooHigh => 0,
        CrashReason::ModFileExtracted => 1,
        CrashReason::MixinBootstrapError => 2,
        CrashReason::OutOfMemory => 3,
        CrashReason::UsingJDK => 4,
        CrashReason::UsingOpenJ9 => 5,
        CrashReason::JavaTooOld => 6,
        CrashReason::ModDuplicateModFiles => 7,
        CrashReason::ModRequiresJava11 => 8,
        CrashReason::ModMissingDependency => 9,
        CrashReason::ModIncompatible => 10,
        CrashReason::ModMissingOrIncompatible => 11,
        CrashReason::ModCrashed => 12,
        CrashReason::ModNoInfo => 13,
        CrashReason::ModMixinError => 14,
        CrashReason::ModNameContainsSpecialChars => 15,
        CrashReason::ModNameDuplicate => 16,
        CrashReason::OptiFineIncompatible => 17,
        CrashReason::AMDDriverCrash => 18,
        CrashReason::NVidiaDriverCrash => 19,
        CrashReason::IntelDriverCrash => 20,
        CrashReason::PixelFormatNotAccelerated => 21,
        CrashReason::ManuallyTriggeredCrash => 22,
        CrashReason::OptiFineMissingForge => 23,
        CrashReason::ShadersModWithOptiFine => 24,
        CrashReason::ForgeMissing => 25,
        CrashReason::FabricCrash => 26,
        CrashReason::FabricModCrash => 27,
        CrashReason::ForgeCrash => 28,
        CrashReason::ModLoaderVersionIncompatible => 29,
        CrashReason::NightConfigBug => 30,
        CrashReason::OpenJ9Crash => 31,
        CrashReason::OpenGL1282Error => 32,
        CrashReason::ModIdConflict => 33,
        CrashReason::InvalidPath => 34,
        CrashReason::ModCyclicIssue => 35,
        CrashReason::SecurityException => 36,
        CrashReason::NativeLinkError => 37,
        CrashReason::Unknown => 38,
        CrashReason::Java32Bit => 39,
        CrashReason::JavaVersionIncompatible => 40,
        CrashReason::GpuNoOpenGL => 41,
        CrashReason::JavaArgsError => 42,
        CrashReason::TextureTooLarge => 43,
        CrashReason::OptiFineCannotLoadWorld => 44,
        CrashReason::MultipleForge => 45,
        CrashReason::TooManyMods => 46,
        CrashReason::ModConfigCrash => 47,
        CrashReason::ModLoaderError => 48,
        CrashReason::ModInitError => 49,
        CrashReason::SuspectedModCrash => 50,
        CrashReason::ShortLogOutput => 51,
        CrashReason::BlockCrash => 52,
        CrashReason::EntityCrash => 53,
        CrashReason::NoAnalysisFiles => 54,
        CrashReason::StackKeywordFound => 55,
        CrashReason::StackModNameFound => 56,
    }
}
