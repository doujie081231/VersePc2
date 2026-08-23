// launch/java_scan.rs — Java 系统扫描决策纯函数
// 1:1 翻译自 server/launch/java-scan-resolver.js

/// Java 候选项
#[derive(Clone, Debug)]
pub struct JavaCandidate {
    pub major_version: u32,
    pub path: String,
}

/// 决策是否跳过系统 Java 扫描（对应 java-scan-resolver.js）
/// 只有 candidates 里有"精确匹配"（majorVersion == requiredVersion）时才跳过
/// 旧逻辑只要"满足要求"就跳过，会错过更精确的版本
pub fn should_skip_system_scan(
    candidates: &[JavaCandidate],
    required_version: u32,
    max_version: u32,
) -> bool {
    if candidates.is_empty() {
        return false;
    }
    candidates
        .iter()
        .any(|j| j.major_version == required_version && j.major_version <= max_version)
}

/// 获取版本要求的 Java 主版本号范围
/// 1.20.5+ / 1.21+ → Java 21
/// 1.18 - 1.20.4 → Java 17
/// 1.17 → Java 16
/// 1.12 - 1.16 → Java 8
/// 旧版 → Java 8
pub fn get_java_version_range(version_id: &str) -> (u32, u32) {
    // 从版本 ID 提取主版本号（如 "1.20.1-Forge-47.3.0" → 1.20.1）
    let mc_version = extract_mc_version(version_id);
    let (major, minor, patch) = parse_version(&mc_version);

    if major > 1 || (major == 1 && minor > 20) || (major == 1 && minor == 20 && patch >= 5) {
        (21, 999)
    } else if major == 1 && minor >= 18 {
        (17, 999)
    } else if major == 1 && minor == 17 {
        (16, 999)
    } else {
        (8, 999)
    }
}

/// 从版本 ID 提取 Minecraft 版本号
/// "1.20.1-Forge-47.3.0" → "1.20.1"
/// "1.20.1" → "1.20.1"
/// "1.20" → "1.20"
fn extract_mc_version(version_id: &str) -> String {
    let parts: Vec<&str> = version_id.split('-').collect();
    if parts.is_empty() {
        return String::new();
    }
    // 找到第一个看起来像版本号的 part
    for p in &parts {
        if p.starts_with(|c: char| c.is_ascii_digit()) {
            return p.to_string();
        }
    }
    parts[0].to_string()
}

/// 解析版本号字符串为 (major, minor, patch)
/// "1.20.1" → (1, 20, 1)
/// "1.20" → (1, 20, 0)
fn parse_version(v: &str) -> (u32, u32, u32) {
    let parts: Vec<u32> = v
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}


