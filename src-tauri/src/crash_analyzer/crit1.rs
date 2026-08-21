// crash_analyzer/crit1.rs — 一级崩溃分析：高置信度关键字匹配
// 职责：对 logCrash/logMc/logHs 做 includes + 正则提取，识别 Java、Mod 解压、Mixin、显卡、内存等问题

use std::collections::HashMap;

use super::constants::CrashReason;
use super::prepare::PreparedLogs;
use super::stack::try_analyze_mod_name_one;
use super::utils::{append_reason, before_first, between, regex_check, regex_seek_group};

/// 一级分析：高置信度关键字匹配
pub fn analyze(logs: &PreparedLogs) -> HashMap<CrashReason, Vec<String>> {
    let mut reasons: HashMap<CrashReason, Vec<String>> = HashMap::new();
    let log_mc = logs.log_mc.as_deref();
    let log_hs = logs.log_hs.as_deref();
    let log_crash = logs.log_crash.as_deref();
    let log_mc_debug = logs.log_mc_debug.as_deref();

    // 空白分析
    if log_mc.is_none() && log_hs.is_none() && log_crash.is_none() {
        append_reason(&mut reasons, CrashReason::NoAnalysisFiles, None);
        return reasons;
    }

    // 崩溃报告分析（高优先级）
    if let Some(lc) = log_crash {
        if lc.contains("Unable to make protected final java.lang.Class java.lang.ClassLoader.defineClass") {
            append_reason(&mut reasons, CrashReason::JavaVersionTooHigh, None);
        }
        if lc.contains("Failed loading config file ") {
            let mod_name = regex_seek_group(lc, r"Failed loading config file .+ for modid ([^\n]+)", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            let config_path = regex_seek_group(lc, r"Failed loading config file (.+) of type", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            let mut additional = Vec::new();
            if let Some(m) = mod_name {
                additional.append(&mut try_analyze_mod_name_one(&m, log_crash, log_mc_debug));
            }
            if let Some(p) = config_path {
                additional.push(p);
            }
            append_reason(&mut reasons, CrashReason::ModConfigCrash, Some(additional));
        }
    }

    // 游戏日志分析
    if let Some(lm) = log_mc {
        if lm.contains("Unrecognized option:") {
            append_reason(&mut reasons, CrashReason::JavaArgsError, None);
        }
        if lm.contains("Found multiple arguments for option fml.forgeVersion, but you asked for only one") {
            append_reason(&mut reasons, CrashReason::MultipleForge, None);
        }
        if lm.contains("The driver does not appear to support OpenGL") {
            append_reason(&mut reasons, CrashReason::GpuNoOpenGL, None);
        }
        if lm.contains("java.lang.ClassCastException: java.base/jdk")
            || lm.contains("java.lang.ClassCastException: class jdk.")
        {
            append_reason(&mut reasons, CrashReason::UsingJDK, None);
        }
        if lm.contains("java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.texture.SpriteContents.<init>")
            || lm.contains("java.lang.NoSuchMethodError: 'java.lang.String com.mojang.blaze3d.systems.RenderSystem.getBackendDescription")
            || lm.contains("java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.block.model.BakedQuad.<init>")
            || lm.contains("java.lang.NoSuchMethodError: 'void net.minecraftforge.client.gui.overlay.ForgeGui.renderSelectedItemName")
            || lm.contains("java.lang.NoSuchMethodError: 'void net.minecraft.server.level.DistanceManager")
            || lm.contains("java.lang.NoSuchMethodError: 'net.minecraft.network.chat.FormattedText net.minecraft.client.gui.Font.ellipsize")
        {
            append_reason(&mut reasons, CrashReason::OptiFineIncompatible, None);
        }
        if lm.contains("Open J9 is not supported")
            || lm.contains("OpenJ9 is incompatible")
            || lm.contains(".J9VMInternals.")
        {
            append_reason(&mut reasons, CrashReason::UsingOpenJ9, None);
        }
        if lm.contains("java.lang.NoSuchFieldException: ucp")
            || lm.contains("because module java.base does not export")
            || lm.contains("java.lang.ClassNotFoundException: jdk.nashorn.api.scripting.NashornScriptEngineFactory")
            || lm.contains("java.lang.ClassNotFoundException: java.lang.invoke.LambdaMetafactory")
        {
            append_reason(&mut reasons, CrashReason::JavaVersionTooHigh, None);
        }
        if lm.contains("The directories below appear to be extracted jar files. Fix this before you continue.")
            || lm.contains("Extracted mod jars found, loading will NOT continue")
        {
            append_reason(&mut reasons, CrashReason::ModFileExtracted, None);
        }
        if lm.contains("java.lang.ClassNotFoundException: org.spongepowered.asm.launch.MixinTweaker") {
            append_reason(&mut reasons, CrashReason::MixinBootstrapError, None);
        }
        if lm.contains("Couldn't set pixel format") {
            append_reason(&mut reasons, CrashReason::PixelFormatNotAccelerated, None);
        }
        if lm.contains("java.lang.OutOfMemoryError") || lm.contains("an out of memory error") {
            append_reason(&mut reasons, CrashReason::OutOfMemory, None);
        }
        if lm.contains("java.lang.RuntimeException: Shaders Mod detected. Please remove it, OptiFine has built-in support for shaders.") {
            append_reason(&mut reasons, CrashReason::ShadersModWithOptiFine, None);
        }
        if lm.contains("java.lang.NoSuchMethodError: sun.security.util.ManifestEntryVerifier")
            || lm.contains("java.lang.NoSuchMethodError: 'void sun.security.util.ManifestEntryVerifier")
        {
            append_reason(&mut reasons, CrashReason::ModLoaderVersionIncompatible, None);
        }
        if lm.contains("1282: Invalid operation") {
            append_reason(&mut reasons, CrashReason::OpenGL1282Error, None);
        }
        if lm.contains("signer information does not match signer information of other classes in the same package") {
            let class_name = regex_seek_group(lm, r#"class "([^']+)="'s signer information"#, 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            append_reason(&mut reasons, CrashReason::SecurityException, class_name.map(|s| vec![s]));
        }
        if lm.contains("Maybe try a lower resolution resourcepack?") {
            append_reason(&mut reasons, CrashReason::TextureTooLarge, None);
        }
        if lm.contains("java.lang.NoSuchMethodError: net.minecraft.world.server.ChunkManager$ProxyTicketManager.shouldForceTicks(J)Z")
            && lm.contains("OptiFine")
        {
            append_reason(&mut reasons, CrashReason::OptiFineCannotLoadWorld, None);
        }
        if lm.contains("com.electronwill.nightconfig.core.io.ParsingException: Not enough data available")
            && !reasons.contains_key(&CrashReason::ModConfigCrash)
        {
            append_reason(&mut reasons, CrashReason::NightConfigBug, None);
        }
        if lm.contains("Cannot find launch target fmlclient, unable to launch") {
            append_reason(&mut reasons, CrashReason::ForgeMissing, None);
        }
        if lm.contains("Invalid paths argument, contained no existing paths")
            && lm.contains("libraries\\net\\minecraftforge\\fmlcore")
        {
            append_reason(&mut reasons, CrashReason::ForgeMissing, None);
        }
        if lm.contains("Invalid module name: '' is not a Java identifier") {
            append_reason(&mut reasons, CrashReason::ModNameContainsSpecialChars, None);
        }
        if lm.contains("has been compiled by a more recent version of the Java Runtime (class file version 55.0), this version of the Java Runtime only recognizes class file versions up to")
            || lm.contains("java.lang.RuntimeException: java.lang.NoSuchMethodException: no such method: sun.misc.Unsafe.defineAnonymousClass(Class,byte[],Object[])Class/invokeVirtual")
            || lm.contains("java.lang.IllegalArgumentException: The requested compatibility level JAVA_11 could not be set. Level is not supported by the active JRE or ASM version")
        {
            append_reason(&mut reasons, CrashReason::ModRequiresJava11, None);
        }
        if lm.contains("Unsupported class file major version")
            || lm.contains("Unsupported major.minor version")
            || lm.contains("Level is not supported by the active JRE or ASM version")
        {
            append_reason(&mut reasons, CrashReason::JavaTooOld, None);
        }
        if lm.contains("Invalid maximum heap size") {
            append_reason(&mut reasons, CrashReason::Java32Bit, None);
        }
        if lm.contains("Could not reserve enough space") {
            if lm.contains("for 1048576KB object heap") {
                append_reason(&mut reasons, CrashReason::Java32Bit, None);
            } else {
                append_reason(&mut reasons, CrashReason::OutOfMemory, None);
            }
        }

        // 确定的 Mod 导致崩溃
        if lm.contains("Caught exception from ") {
            let name = regex_seek_group(lm, r"Caught exception from ([^\n]+)", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            if let Some(n) = name {
                let names = try_analyze_mod_name_one(&n, log_crash, log_mc_debug);
                append_reason(&mut reasons, CrashReason::ModCrashed, Some(names));
            }
        }

        // Mod 重复 / 前置问题
        if lm.contains("DuplicateModsFoundException") {
            let jars = super::utils::regex_seek_all_group(r"(?i)\n\t\w+ : [A-Za-z][^/\n]+(/|\\)([^/\\\n]+\.jar)", lm, 1);
            append_reason(&mut reasons, CrashReason::ModDuplicateModFiles, Some(jars));
        }
        if lm.contains("Found a duplicate mod") {
            let jars = super::utils::regex_seek_all_group(r"(?i)[^\\/]+\.jar", lm, 0);
            append_reason(&mut reasons, CrashReason::ModDuplicateModFiles, Some(prev_distinct(jars)));
        }
        if lm.contains("Found duplicate mods") {
            let mod_ids = super::utils::regex_seek_all_group(r"Mod ID: '(\w+)' from mod files:", lm, 1);
            let mut ids: Vec<String> = Vec::new();
            for m in mod_ids.into_iter() {
                for line in m.lines() {
                    if !line.is_empty() && !ids.contains(&line.to_string()) {
                        ids.push(line.to_string());
                    }
                }
            }
            append_reason(&mut reasons, CrashReason::ModDuplicateModFiles, Some(ids));
        }
        if lm.contains("ModResolutionException: Duplicate") {
            let jars = super::utils::regex_seek_all_group(r"(?i)[^\\/]+\.jar", lm, 0);
            append_reason(&mut reasons, CrashReason::ModDuplicateModFiles, Some(prev_distinct(jars)));
        }
        if lm.contains("Incompatible mods found!") {
            let incompatible = regex_seek_group(
                lm,
                r"Incompatible mods found![\s\S]+: ([\s\S]+?)(?=\tat )",
                1,
            )
            .map(|s| {
                before_first(&s, "更多信息：")
                    .replace("Some of your mods are incompatible with the game or each other!", "")
                    .trim()
                    .to_string()
            });
            append_reason(&mut reasons, CrashReason::ModIncompatible, single_nonempty(incompatible));
        }
        if lm.contains("Missing or unsupported mandatory dependencies:") {
            let deps = super::utils::regex_seek_all_group(
                r"(?i)Missing or unsupported mandatory dependencies:([\n\r]+\t(.*))+",
                lm,
                0,
            );
            let deps: Vec<String> = deps
                .into_iter()
                .flat_map(|s| {
                    s.split('\n')
                        .map(|x| x.trim_matches(|c| c == '\n' || c == '\r' || c == '\t' || c == ' ').to_string())
                        .collect::<Vec<_>>()
                })
                .filter(|x| !x.is_empty())
                .collect();
            let mut seen: Vec<String> = Vec::new();
            let deps: Vec<String> = deps.into_iter().fold(Vec::new(), |mut acc, x| {
                if !seen.contains(&x) {
                    seen.push(x.clone());
                    acc.push(x);
                }
                acc
            });
            append_reason(&mut reasons, CrashReason::ModMissingDependency, Some(deps));
        }
    }

    // 虚拟机日志分析
    if let Some(lh) = log_hs {
        if lh.contains("The system is out of physical RAM or swap space")
            || lh.contains("Out of Memory Error")
        {
            append_reason(&mut reasons, CrashReason::OutOfMemory, None);
        }
        if lh.contains("EXCEPTION_ACCESS_VIOLATION") {
            if lh.contains("# C  [ig") {
                append_reason(&mut reasons, CrashReason::IntelDriverCrash, None);
            }
            if lh.contains("# C  [atio") {
                append_reason(&mut reasons, CrashReason::AMDDriverCrash, None);
            }
            if lh.contains("# C  [nvoglv") {
                append_reason(&mut reasons, CrashReason::NVidiaDriverCrash, None);
            }
        }
    }

    // 崩溃报告分析（续）
    if let Some(lc) = log_crash {
        if lc.contains("maximum id range exceeded") {
            append_reason(&mut reasons, CrashReason::TooManyMods, None);
        }
        if lc.contains("java.lang.OutOfMemoryError") {
            append_reason(&mut reasons, CrashReason::OutOfMemory, None);
        }
        if lc.contains("Pixel format not accelerated") {
            append_reason(&mut reasons, CrashReason::PixelFormatNotAccelerated, None);
        }
        if lc.contains("Manually triggered debug crash") {
            append_reason(&mut reasons, CrashReason::ManuallyTriggeredCrash, None);
        }
        if lc.contains("has mods that were not found")
            && regex_check(lc, r"The Mod File [^\n]+optifine\\OptiFine[^\n]+ has mods that were not found")
        {
            append_reason(&mut reasons, CrashReason::OptiFineIncompatible, None);
        }
        // Mod 导致的崩溃
        if lc.contains("-- MOD ") {
            let log_crash_mod = between(lc, "-- MOD ", "Failure message:");
            if log_crash_mod.to_lowercase().contains(".jar") {
                let mod_file = regex_seek_group(log_crash_mod, r"Mod File: (.+)", 1)
                    .map(|s| s.trim_end_matches('\n').to_string());
                append_reason(&mut reasons, CrashReason::ModCrashed, mod_file.map(|s| vec![s]));
            } else {
                let failure_msg = regex_seek_group(lc, r"Failure message: ([\w\W]+?)(?=\tMod)", 1)
                    .map(|s| s.replace('\t', " ").trim_end_matches('\n').trim().to_string());
                append_reason(&mut reasons, CrashReason::ModNoInfo, failure_msg.map(|s| vec![s]));
            }
        }
        if lc.contains("Multiple entries with same key: ") {
            let name = regex_seek_group(lc, r"Multiple entries with same key: ([^=]+)", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            if let Some(n) = name {
                let names = try_analyze_mod_name_one(&n, log_crash, log_mc_debug);
                append_reason(&mut reasons, CrashReason::ModCrashed, Some(names));
            }
        }
        if lc.contains("LoaderExceptionModCrash: Caught exception from ") {
            let name = regex_seek_group(lc, r"LoaderExceptionModCrash: Caught exception from ([^\n]+)", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            if let Some(n) = name {
                let names = try_analyze_mod_name_one(&n, log_crash, log_mc_debug);
                append_reason(&mut reasons, CrashReason::ModCrashed, Some(names));
            }
        }
        if lc.contains("Failed loading config file ") {
            let mod_name = regex_seek_group(lc, r"Failed loading config file .+ for modid ([^\n]+)", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            let config_path = regex_seek_group(lc, r"Failed loading config file (.+) of type", 1)
                .map(|s| s.trim_end_matches('\n').to_string());
            let mut additional = Vec::new();
            if let Some(m) = mod_name {
                additional.append(&mut try_analyze_mod_name_one(&m, log_crash, log_mc_debug));
            }
            if let Some(p) = config_path {
                additional.push(p);
            }
            append_reason(&mut reasons, CrashReason::ModConfigCrash, Some(additional));
        }
    }

    reasons
}

fn prev_distinct(v: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    v.into_iter()
        .filter(|x| {
            if x.is_empty() || seen.contains(x) {
                false
            } else {
                seen.push(x.clone());
                true
            }
        })
        .collect()
}

fn single_nonempty(v: Option<String>) -> Option<Vec<String>> {
    v.filter(|s| !s.is_empty()).map(|s| vec![s])
}