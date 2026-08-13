// crash_analyzer/crit1.rs — 一级崩溃分析：高置信度关键字匹配
// 对应原项目 server/crash-analyzer/analyze-crit1.js
// 职责：对 logCrash/logMc/logHs 做 includes + 正则提取，识别 Java 版本、Mod 解压、Mixin、内存、驱动等问题
//
// 注意：原 JS 用 lookbehind `(?<=prefix)content` 提取附加信息
// Rust 的 regex crate 不支持 lookbehind，改写为 `prefix(content)` 捕获组 + regex_seek_group

use std::collections::HashMap;

use super::constants::CrashReason;
use super::prepare::PreparedLogs;
use super::utils::{append_reason, regex_seek, regex_seek_group, single_additional};

/// 一级分析：高置信度关键字匹配
/// 对应原项目 analyze-crit1.js analyzeCrit1
pub fn analyze(logs: &PreparedLogs) -> HashMap<CrashReason, Vec<String>> {
    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();

    let log_mc = logs.log_mc.as_deref().unwrap_or("");
    let log_hs = logs.log_hs.as_deref().unwrap_or("");
    let log_crash = logs.log_crash.as_deref().unwrap_or("");

    if log_mc.is_empty() && log_hs.is_empty() && log_crash.is_empty() {
        append_reason(&mut reasons, CrashReason::Unknown, Some(vec!["未找到任何日志文件".to_string()]));
        return reasons;
    }

    // ===== 崩溃报告中的关键字 =====
    if !log_crash.is_empty() {
        if log_crash.contains("Unable to make protected final java.lang.Class java.lang.ClassLoader.defineClass") {
            append_reason(&mut reasons, CrashReason::JavaVersionTooHigh, None);
        }
        if log_crash.contains("Failed loading config file ") {
            // 原项目用两个 lookbehind 正则提取附加信息
            let mod_name = regex_seek_group(
                log_crash,
                r"Failed loading config file .+ for modid ([^\n]+)",
                1,
            )
            .map(|s| s.trim().to_string());
            let config_path = regex_seek_group(log_crash, r"Failed loading config file (.+) of type", 1)
                .map(|s| s.trim().to_string());
            let mut additional: Vec<String> = Vec::new();
            if let Some(n) = mod_name {
                additional.push(n);
            }
            if let Some(p) = config_path {
                additional.push(p);
            }
            append_reason(&mut reasons, CrashReason::ModFileExtracted, Some(additional));
        }
    }

    // ===== 游戏日志中的关键字 =====
    if !log_mc.is_empty() {
        check_log_mc(log_mc, &mut reasons);
    }

    // ===== JVM 崩溃日志中的关键字 =====
    if !log_hs.is_empty() {
        check_log_hs(log_hs, &mut reasons);
    }

    // ===== 崩溃报告中的关键字（续） =====
    if !log_crash.is_empty() {
        check_log_crash(log_crash, &mut reasons);
    }

    reasons
}

/// 游戏日志关键字检测（提取自原 analyzeCrit1 的 logMc 分支）
fn check_log_mc(log_mc: &str, reasons: &mut HashMap<CrashReason, Vec<String>>) {
    // Java 版本相关
    if log_mc.contains("Unrecognized option:") {
        append_reason(reasons, CrashReason::JavaVersionTooHigh, None);
    }
    if log_mc.contains("Found multiple arguments for option fml.forgeVersion, but you asked for only one") {
        append_reason(reasons, CrashReason::ModLoaderVersionIncompatible, None);
    }
    if log_mc.contains("The driver does not appear to support OpenGL") {
        append_reason(reasons, CrashReason::UsingOpenJ9, None);
    }
    if log_mc.contains("java.lang.ClassCastException: java.base/jdk")
        || log_mc.contains("java.lang.ClassCastException: class jdk.")
    {
        append_reason(reasons, CrashReason::UsingJDK, None);
    }

    // OptiFine 缺少 Forge 的多种 NoSuchMethodError 表现
    if log_mc.contains("TRANSFORMER/net.optifine/net.optifine.reflect.Reflector.<clinit>(Reflector.java)")
        || log_mc.contains("java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.texture.SpriteContents.<init>()'")
        || log_mc.contains("java.lang.NoSuchMethodError: 'java.lang.String com.mojang.blaze3d.systems.RenderSystem.getBackendDescription'")
        || log_mc.contains("java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.block.model.BakedQuad.<init>()'")
        || log_mc.contains("java.lang.NoSuchMethodError: 'void net.minecraftforge.client.gui.overlay.ForgeGui.renderSelectedItemName'")
        || log_mc.contains("java.lang.NoSuchMethodError: 'void net.minecraft.world.level.DistanceManager'")
        || log_mc.contains("java.lang.NoSuchMethodError: net.minecraft.network.chat.FormattedText net.minecraft.client.gui.Font.ellipsize")
    {
        append_reason(reasons, CrashReason::OptiFineMissingForge, None);
    }

    // OpenJ9
    if log_mc.contains("Open J9 is not supported")
        || log_mc.contains("OpenJ9 is incompatible")
        || log_mc.contains(".J9VMInternals.")
    {
        append_reason(reasons, CrashReason::UsingOpenJ9, None);
    }

    // Java 版本过高
    if log_mc.contains("java.lang.NoSuchFieldException: ucp")
        || log_mc.contains("because module java.base does not export")
        || log_mc.contains("java.lang.ClassNotFoundException: jdk.nashorn.api.scripting.NashornScriptEngineFactory")
        || log_mc.contains("java.lang.ClassNotFoundException: class jdk.")
    {
        append_reason(reasons, CrashReason::JavaVersionTooHigh, None);
    }

    // Mod 文件被解压
    if log_mc.contains("The directories below appear to be extracted jar files. Fix this before you continue.")
        || log_mc.contains("Extracted mod jars found, loading will NOT continue")
    {
        append_reason(reasons, CrashReason::ModFileExtracted, None);
    }

    // Mixin 引导失败
    if log_mc.contains("java.lang.ClassNotFoundException: org.spongepowered.asm.launch.MixinTweaker") {
        append_reason(reasons, CrashReason::MixinBootstrapError, None);
    }

    // 像素格式未加速
    if log_mc.contains("Couldn't set pixel format") {
        append_reason(reasons, CrashReason::PixelFormatNotAccelerated, None);
    }

    // 内存不足
    if log_mc.contains("java.lang.OutOfMemoryError")
        || log_mc.contains("an out of memory error")
        || log_mc.contains("Invalid maximum heap size")
        || log_mc.contains("Could not reserve enough space")
    {
        append_reason(reasons, CrashReason::OutOfMemory, None);
    }

    // Shaders Mod 与 OptiFine 冲突
    if log_mc.contains("java.lang.RuntimeException: Shaders Mod detected. Please remove it, OptiFine has built-in support for shaders.") {
        append_reason(reasons, CrashReason::ShadersModWithOptiFine, None);
    }

    // Mod 加载器版本不兼容
    if log_mc.contains("java.lang.NoSuchMethodError: sun.security.util.ManifestEntryVerifier")
        || log_mc.contains("java.lang.NoSuchMethodError: 'void sun.security.util.ManifestEntryVerifier'")
    {
        append_reason(reasons, CrashReason::ModLoaderVersionIncompatible, None);
    }

    // OpenGL 1282 错误
    if log_mc.contains("1282: Invalid operation") {
        append_reason(reasons, CrashReason::OpenGL1282Error, None);
    }

    // Mod 名称包含特殊字符
    if log_mc.contains("signer information does not match signer information of other classes in the same package") {
        // 原: regexSeek('(?<=class ")[^\'"]+(?="\'s signer information)')
        let class_name = regex_seek_group(log_mc, r#"class "([^'"]+)="'s signer information"#, 1)
            .map(|s| s.trim().to_string());
        append_reason(reasons, CrashReason::ModNameContainsSpecialChars, single_additional(class_name));
    }

    // Mod 循环问题
    if log_mc.contains("Maybe try a lower resolution resourcepack?") {
        append_reason(reasons, CrashReason::ModCyclicIssue, None);
    }

    // OptiFine 不兼容
    if log_mc.contains("java.lang.NoSuchMethodError: net.minecraft.world.server.ChunkManager$ProxyTicketManager.shouldForceTickets(J)Z")
        && log_mc.contains("OptiFine")
    {
        append_reason(reasons, CrashReason::OptiFineIncompatible, None);
    }

    // Java 版本过旧
    if log_mc.contains("Unsupported class file major version")
        || log_mc.contains("Unsupported major.minor version")
    {
        append_reason(reasons, CrashReason::JavaTooOld, None);
    }

    // NightConfig Bug
    if log_mc.contains("com.electronwill.nightconfig.core.io.ParsingException: Not enough data available")
        && !reasons.contains_key(&CrashReason::NightConfigBug)
    {
        append_reason(reasons, CrashReason::NightConfigBug, None);
    }

    // Forge 缺失
    if log_mc.contains("Cannot find launch target fmlclient, unable to launch")
        || (log_mc.contains("Invalid paths argument, contained no existing paths")
            && log_mc.contains("libraries\\net\\minecraftforge\\fmlcore"))
    {
        append_reason(reasons, CrashReason::ForgeMissing, None);
    }

    // Mod 名称重复
    if log_mc.contains("Invalid module name: '' is not a Java identifier") {
        append_reason(reasons, CrashReason::ModNameDuplicate, None);
    }

    // Mod 需要 Java 11
    if log_mc.contains("has been compiled by a more recent version of the Java Runtime (class file version 55.0), this version of the Java Runtime only recognizes class file versions up to")
        || log_mc.contains("java.lang.RuntimeException: java.lang.NoSuchMethodException: no such method: sun.misc.Unsafe.defineAnonymousClass(Class,byte[],Object[])Class/invokeVirtual")
        || log_mc.contains("java.lang.IllegalArgumentException: The requested compatibility level JAVA_11 could not be set. Level is not supported by the active JRE or ASM version")
    {
        append_reason(reasons, CrashReason::ModRequiresJava11, None);
    }

    // Mod 崩溃：从 "Caught exception from" 后提取 Mod 名
    if log_mc.contains("Caught exception from ") {
        // 原: regexSeek('(?<=Caught exception from )[^\n]+')
        let mod_name = regex_seek_group(log_mc, r"Caught exception from ([^\n]+)", 1)
            .map(|s| s.trim().to_string());
        append_reason(reasons, CrashReason::ModCrashed, single_additional(mod_name));
    }

    // 重复 Mod
    if log_mc.contains("DuplicateModsFoundException") {
        // 原: regexSeek('(?<=\n\t[\w]+ : [A-Za-z][^/\n]+(/|\\)[^/\\\n]+\.jar', 'gi')
        // Rust 不支持 lookbehind，改用捕获组
        let jar_names = regex_seek(r"\n\t\w+ : [A-Za-z][^/\n]+(/|\\)[^/\\\n]+\.jar", log_mc);
        append_reason(reasons, CrashReason::ModDuplicateModFiles, single_additional(jar_names));
    }
    if log_mc.contains("Found a duplicate mod") {
        let jar_names = if log_mc.contains("Found a duplicate mod[^\n]+") {
            regex_seek(r"[^\\/]+\.jar", log_mc)
        } else {
            None
        };
        append_reason(reasons, CrashReason::ModDuplicateModFiles, single_additional(jar_names));
    }
    if log_mc.contains("Found duplicate mods") {
        // 原: regexSeek('(?<=Mod ID: \')\w+(?=\' from mod files:)')
        let mod_ids = regex_seek_group(log_mc, r"Mod ID: '(\w+)' from mod files:", 1);
        let additional = mod_ids
            .map(|s| s.split('\n').map(|x| x.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        append_reason(reasons, CrashReason::ModDuplicateModFiles, Some(additional));
    }
    if log_mc.contains("ModResolutionException: Duplicate") {
        let jar_names = if log_mc.contains("ModResolutionException: Duplicate[^\n]+") {
            regex_seek(r"[^\\/]+\.jar", log_mc)
        } else {
            None
        };
        append_reason(reasons, CrashReason::ModDuplicateModFiles, single_additional(jar_names));
    }

    // 不兼容 Mod
    if log_mc.contains("Incompatible mods found!") {
        // 原: regexSeek('(?<=Incompatible mods found![\s\S]+: )[\s\S]+?(?=\tat )')
        //      .replace('Some of your mods are incompatible with the game or each other!', '')
        //      .trim()
        let incompatible = regex_seek_group(
            log_mc,
            r"Incompatible mods found![\s\S]+: ([\s\S]+?)(?=\tat )",
            1,
        )
        .map(|s| {
            s.replace(
                "Some of your mods are incompatible with the game or each other!",
                "",
            )
            .trim()
            .to_string()
        });
        append_reason(reasons, CrashReason::ModIncompatible, single_additional(incompatible));
    }

    // 缺少前置 Mod
    if log_mc.contains("Missing or unsupported mandatory dependencies:") {
        // 原: regexSeek('(?<=Missing or unsupported mandatory dependencies:)([\n\r]+\t.*)+', 'gi')
        let dep_match = regex_seek(
            log_mc,
            r"Missing or unsupported mandatory dependencies:([\n\r]+\t.*)+",
        );
        let deps = dep_match
            .map(|s| {
                s.split('\n')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
            })
            .map(|mut v| {
                v.sort();
                v.dedup();
                v
            })
            .unwrap_or_default();
        append_reason(reasons, CrashReason::ModMissingDependency, Some(deps));
    }
}

/// JVM 崩溃日志关键字检测（提取自原 analyzeCrit1 的 logHs 分支）
fn check_log_hs(log_hs: &str, reasons: &mut HashMap<CrashReason, Vec<String>>) {
    if log_hs.contains("The system is out of physical RAM or swap space")
        || log_hs.contains("Out Of Memory Error")
    {
        append_reason(reasons, CrashReason::OutOfMemory, None);
    }

    // EXCEPTION_ACCESS_VIOLATION：根据驱动库名区分显卡厂商
    if log_hs.contains("EXCEPTION_ACCESS_VIOLATION") {
        if log_hs.contains("# C  [ig") {
            append_reason(reasons, CrashReason::IntelDriverCrash, None);
        }
        if log_hs.contains("# C  [atio") {
            append_reason(reasons, CrashReason::AMDDriverCrash, None);
        }
        if log_hs.contains("# C  [nvoglv") {
            append_reason(reasons, CrashReason::NVidiaDriverCrash, None);
        }
    }
}

/// 崩溃报告关键字检测（提取自原 analyzeCrit1 的 logCrash 分支续）
fn check_log_crash(log_crash: &str, reasons: &mut HashMap<CrashReason, Vec<String>>) {
    if log_crash.contains("maximum id range exceeded") {
        append_reason(reasons, CrashReason::ModIdConflict, None);
    }
    if log_crash.contains("java.lang.OutOfMemoryError") {
        append_reason(reasons, CrashReason::OutOfMemory, None);
    }
    if log_crash.contains("Pixel format not accelerated") {
        append_reason(reasons, CrashReason::PixelFormatNotAccelerated, None);
    }
    if log_crash.contains("Manually triggered debug crash") {
        append_reason(reasons, CrashReason::ManuallyTriggeredCrash, None);
    }

    // OptiFine 缺少 Forge
    if log_crash.contains("has mods that were not found")
        && super::utils::regex_check(
            log_crash,
            r"The Mod File [^\n]+optifine\\OptiFine[^\n]+ has mods that were not found",
        )
    {
        append_reason(reasons, CrashReason::OptiFineMissingForge, None);
    }

    // "-- MOD " 段落：提取 Mod 文件名或失败信息
    if log_crash.contains("-- MOD ") {
        let mod_start = log_crash.find("-- MOD ").unwrap();
        let fail_start = log_crash.find("Failure message:");
        let log_crash_mod = if let Some(fail) = fail_start {
            if fail > mod_start {
                &log_crash[mod_start..fail]
            } else {
                &log_crash[mod_start..]
            }
        } else {
            &log_crash[mod_start..]
        };

        if log_crash_mod.to_lowercase().contains(".jar") {
            // 原: regexSeek('(?<=Mod File: ).+')
            let mod_file = regex_seek_group(log_crash_mod, r"Mod File: (.+)", 1)
                .map(|s| s.trim().to_string());
            append_reason(reasons, CrashReason::ModCrashed, single_additional(mod_file));
        } else {
            // 原: regexSeek('(?<=Failure message: )[\w\W]+?(?=\tMod)').replace(/\t/g, ' ').trim()
            let failure_msg = regex_seek_group(log_crash, r"Failure message: ([\w\W]+?)(?=\tMod)", 1)
                .map(|s| s.replace('\t', " ").trim().to_string());
            append_reason(reasons, CrashReason::ModNoInfo, single_additional(failure_msg));
        }
    }

    // Mod ID 冲突
    if log_crash.contains("Multiple entries with same key: ") {
        // 原: regexSeek('(?<=Multiple entries with same key: )[^=]+')
        let mod_name = regex_seek_group(log_crash, r"Multiple entries with same key: ([^=]+)", 1)
            .map(|s| s.trim().to_string());
        append_reason(reasons, CrashReason::ModIdConflict, single_additional(mod_name));
    }

    // Mod 崩溃
    if log_crash.contains("LoaderExceptionModCrash: Caught exception from ") {
        // 原: regexSeek('(?<=LoaderExceptionModCrash: Caught exception from )[^\n]+')
        let mod_name =
            regex_seek_group(log_crash, r"LoaderExceptionModCrash: Caught exception from ([^\n]+)", 1)
                .map(|s| s.trim().to_string());
        append_reason(reasons, CrashReason::ModCrashed, single_additional(mod_name));
    }

    // Mod 文件被解压（崩溃报告中的）
    if log_crash.contains("Failed loading config file ") {
        let mod_name = regex_seek_group(
            log_crash,
            r"Failed loading config file .+ for modid ([^\n]+)",
            1,
        )
        .map(|s| s.trim().to_string());
        let config_path = regex_seek_group(log_crash, r"Failed loading config file (.+) of type", 1)
            .map(|s| s.trim().to_string());
        let mut additional: Vec<String> = Vec::new();
        if let Some(n) = mod_name {
            additional.push(n);
        }
        if let Some(p) = config_path {
            additional.push(p);
        }
        append_reason(reasons, CrashReason::ModFileExtracted, Some(additional));
    }
}
