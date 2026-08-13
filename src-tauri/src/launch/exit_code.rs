// launch/exit_code.rs — 退出码分析
// 1:1 翻译自 server/launch/shared.js 的 analyzeExitCode

use std::path::{Path, PathBuf};
use serde_json::{json, Value};

/// 退出分析结果
#[derive(Clone, Debug)]
pub struct ExitAnalysis {
    pub code: i32,
    pub reason: String,
    pub suggestion: String,
    pub is_crash: bool,
    pub crash_log_file: Option<String>,
}

impl ExitAnalysis {
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "code": self.code,
            "reason": self.reason,
            "suggestion": self.suggestion,
            "isCrash": self.is_crash,
        });
        if let Some(f) = &self.crash_log_file {
            v["crashLogFile"] = json!(f);
        }
        v
    }
}

/// 分析游戏进程退出码（对应 shared.js:analyzeExitCode）
pub fn analyze_exit_code(code: i32, version_id: &str, data_dir: &Path) -> ExitAnalysis {
    let mut analysis = ExitAnalysis {
        code,
        reason: String::new(),
        suggestion: String::new(),
        is_crash: false,
        crash_log_file: None,
    };

    if code == 0 {
        analysis.reason = "正常退出".to_string();
        return analysis;
    }

    analysis.is_crash = true;
    match code {
        1 => {
            analysis.reason = "游戏异常退出（通用错误）".to_string();
            analysis.suggestion = "可能是模组冲突或Java参数问题，请查看崩溃日志".to_string();
        }
        -1 => {
            analysis.reason = "游戏进程被强制终止".to_string();
            analysis.suggestion = "可能是内存不足或用户手动结束进程".to_string();
        }
        137 => {
            analysis.reason = "内存不足（OOM Killer）".to_string();
            analysis.suggestion = "请增加分配内存或减少模组数量".to_string();
        }
        134 => {
            analysis.reason = "程序异常终止（SIGABRT）".to_string();
            analysis.suggestion = "可能是JVM内部错误，尝试更新Java版本".to_string();
        }
        139 => {
            analysis.reason = "段错误（SIGSEGV）".to_string();
            analysis.suggestion = "可能是JVM崩溃或原生库问题，尝试更新显卡驱动和Java".to_string();
        }
        -7 | -1073741819 => {
            analysis.reason = "JVM 崩溃（访问违规）".to_string();
            analysis.suggestion = "可能是显卡驱动不兼容或内存损坏，请更新显卡驱动和Java版本，尝试减少分配内存".to_string();
        }
        _ => {
            analysis.reason = format!("异常退出（退出码: {}）", code);
            analysis.suggestion = "请查看崩溃日志获取更多信息".to_string();
        }
    }

    // 扫描崩溃日志
    let search_dirs = vec![
        data_dir.join("versions").join(version_id).join("crash-reports"),
        data_dir.join("crash-reports"),
    ];

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(mut entries) = std::fs::read_dir(dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .map(|s| s.ends_with(".txt"))
                        .unwrap_or(false)
                })
                .collect();
            // 按文件名倒序，取最新
            files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

            if let Some(latest) = files.first() {
                let path = latest.path();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    refine_analysis(&content, &mut analysis);
                    analysis.crash_log_file = path.to_str().map(|s| s.to_string());
                    break;
                }
            }
        }
    }

    analysis
}

/// 根据崩溃日志内容细化分析
fn refine_analysis(content: &str, analysis: &mut ExitAnalysis) {
    if content.contains("java.lang.OutOfMemoryError") {
        analysis.reason = "内存不足（OutOfMemoryError）".to_string();
        analysis.suggestion = "请在设置中增加最大内存分配".to_string();
    } else if content.contains("UnsupportedClassVersionError") || content.contains("Unsupported major.minor version") {
        analysis.reason = "Java版本不兼容".to_string();
        analysis.suggestion = "游戏需要更高版本的Java，请在设置中更换Java版本".to_string();
    } else if content.contains("java.lang.NoSuchMethodError") || content.contains("NoClassDefFoundError") {
        analysis.reason = "模组版本不兼容".to_string();
        analysis.suggestion = "请检查模组是否与当前游戏版本和加载器版本匹配".to_string();
    } else if content.contains("Unable to make protected final") || content.contains("does not export") {
        analysis.reason = "Java版本过高导致模块访问限制".to_string();
        analysis.suggestion = "请降级Java版本或使用Java 8/17启动".to_string();
    } else if content.contains("ClassCastException") && content.contains("AppClassLoader") && content.contains("URLClassLoader") {
        analysis.reason = "Java版本过高（旧版 launchwrapper 不兼容 Java 9+）".to_string();
        analysis.suggestion = "该整合包需要 Java 8 才能运行。\n修复: 1)启动设置→Java→选择 JRE 8  2)启动器设置中关闭\"自动选择高版本 Java\"".to_string();
    } else if content.contains("FMLCommonSetupEvent") || content.contains("fml") {
        analysis.reason = "Forge/Fabric初始化失败".to_string();
        analysis.suggestion = "请检查模组兼容性，尝试移除最近添加的模组".to_string();
    } else if content.contains("ShaderCompilationException") || content.contains("shader") {
        analysis.reason = "着色器编译失败".to_string();
        analysis.suggestion = "可能是光影模组问题，尝试移除光影模组".to_string();
    } else if content.contains("Mixin") || content.contains("mixin") {
        analysis.reason = "Mixin注入失败".to_string();
        analysis.suggestion = "可能是模组与当前版本不兼容，检查Mixin相关模组".to_string();
    } else if content.contains("OpenGL") || content.contains("GLFW") {
        analysis.reason = "图形驱动问题".to_string();
        analysis.suggestion = "请更新显卡驱动或检查OpenGL支持".to_string();
    } else if content.contains("Invalid paths argument") || content.contains("contained no existing paths") {
        analysis.reason = "Forge核心库文件缺失（Invalid paths argument）".to_string();
        analysis.suggestion = "Forge安装不完整(fmlcore/javafmllanguage/mclanguage/lowcodelanguage缺失)。\n修复: 1)版本设置→文件修复 2)重新安装Forge 3)检查杀毒白名单".to_string();
    }
}
