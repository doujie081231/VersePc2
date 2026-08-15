// download/mirror.rs — 镜像源管理
// 职责：根据原始 URL 生成镜像候选列表，按下载源模式排序
// 对应原项目 server/http-client/mirror.js 的 getMirrorUrls

use std::time::{Duration, Instant};
use std::sync::{Mutex, LazyLock};
use std::collections::HashSet;

/// 浏览器请求头模拟，绕过部分 CDN（如 CurseForge）对非浏览器 UA 的拦截
pub const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 镜像源健康状态（熔断机制）
/// 3 次失败后熔断 60 秒，期间跳过镜像源
struct MirrorHealth {
    fails: u32,
    down_until: Option<Instant>,
}

/// 坏源黑名单：本次会话内下载失败过的 host，后续下载直接跳过
/// 对应原项目 AdaptiveController._badHosts
static BAD_HOSTS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// 记录一个下载失败的源（按 host 记忆）
pub fn mark_bad_host(url: &str) {
    if let Some(host) = host_of(url) {
        let mut set = BAD_HOSTS.lock().unwrap();
        set.insert(host);
    }
}

/// 记录一个下载成功的源（从黑名单移除）
pub fn clear_bad_host(url: &str) {
    if let Some(host) = host_of(url) {
        let mut set = BAD_HOSTS.lock().unwrap();
        set.remove(&host);
    }
}

/// 该 URL 的 host 是否在坏源黑名单中
pub fn is_bad_host(url: &str) -> bool {
    if let Some(host) = host_of(url) {
        BAD_HOSTS.lock().unwrap().contains(&host)
    } else {
        false
    }
}

/// 从 URL 提取 host（小写）
fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    Some(rest.split(['/', '?', ':']).next()?.to_lowercase())
}

static MIRROR_HEALTH: Mutex<MirrorHealth> = Mutex::new(MirrorHealth {
    fails: 0,
    down_until: None,
});

/// 镜像失败计数 +1，达到阈值则熔断
pub fn mirror_failed() {
    let mut h = MIRROR_HEALTH.lock().unwrap();
    h.fails += 1;
    if h.fails >= 3 {
        h.down_until = Some(Instant::now() + Duration::from_secs(60));
    }
}

/// 镜像成功，重置计数
pub fn mirror_success() {
    let mut h = MIRROR_HEALTH.lock().unwrap();
    h.fails = 0;
    h.down_until = None;
}

/// 镜像是否可用（未熔断或熔断已过期）
pub fn is_mirror_available() -> bool {
    let mut h = MIRROR_HEALTH.lock().unwrap();
    match h.down_until {
        Some(until) => {
            if Instant::now() >= until {
                // 熔断过期，重置
                h.fails = 0;
                h.down_until = None;
                true
            } else {
                false
            }
        }
        None => true,
    }
}

/// 把 Adoptium Temurin 的 GitHub 直链转换为中科大 USTC 镜像 URL
/// 对应原项目 server/java/java-download.js 的 getTemurinMirrorUrl
/// 形如：
///   https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.5%2B11/OpenJDK21U-jdk_x64_windows_hotspot_21.0.5_11.zip
///   → https://mirrors.ustc.edu.cn/adoptium/releases/temurin21-binaries/jdk-21.0.5%2B11/OpenJDK21U-jdk_x64_windows_hotspot_21.0.5_11.zip
fn temurin_ustc_mirror(github_url: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"https://github\.com/adoptium/temurin(\d+)-binaries/releases/download/([^/]+)/([^/?]+)",
    )
    .ok()?;
    let cap = re.captures(github_url)?;
    let major = cap.get(1)?.as_str();
    let tag = cap.get(2)?.as_str();
    let file = cap.get(3)?.as_str();
    Some(format!(
        "https://mirrors.ustc.edu.cn/adoptium/releases/temurin{}-binaries/{}/{}",
        major, tag, file
    ))
}

/// 把官方 URL 替换为 BMCLAPI 镜像 URL
/// 对应原项目 context.js 的 ctx.mirrors.BMCLAPI_MIRROR 映射表
pub fn to_mirror_url(original: &str) -> Option<String> {
    // Adoptium Temurin JDK GitHub 直链 → 中科大 USTC 镜像（Java 下载用）
    if original.starts_with("https://github.com/adoptium/temurin") {
        return temurin_ustc_mirror(original);
    }
    // Mojang 官方源 → BMCLAPI
    let mirror = if original.starts_with("https://piston-data.mojang.com/") {
        original.replace("https://piston-data.mojang.com/", "https://bmclapi2.bangbang93.com/")
    } else if original.starts_with("https://piston-meta.mojang.com/") {
        original.replace("https://piston-meta.mojang.com/", "https://bmclapi2.bangbang93.com/")
    } else if original.starts_with("https://libraries.minecraft.net/") {
        original.replace("https://libraries.minecraft.net/", "https://bmclapi2.bangbang93.com/libraries/")
    } else if original.starts_with("https://resources.download.minecraft.net/") {
        original.replace("https://resources.download.minecraft.net/", "https://bmclapi2.bangbang93.com/assets/")
    } else if original.starts_with("https://launchermeta.mojang.com/") {
        original.replace("https://launchermeta.mojang.com/", "https://bmclapi2.bangbang93.com/")
    } else if original.starts_with("https://launcher.mojang.com/") {
        original.replace("https://launcher.mojang.com/", "https://bmclapi2.bangbang93.com/")
    } else if original.starts_with("https://meta.fabricmc.net/") {
        original.replace("https://meta.fabricmc.net/", "https://bmclapi2.bangbang93.com/fabric-meta/")
    } else if original.starts_with("https://maven.minecraftforge.net/") {
        original.replace("https://maven.minecraftforge.net/", "https://bmclapi2.bangbang93.com/maven/")
    } else if original.starts_with("https://maven.neoforged.net/") {
        original.replace("https://maven.neoforged.net/", "https://bmclapi2.bangbang93.com/maven/")
    } else if original.starts_with("https://maven.fabricmc.net/") {
        original.replace("https://maven.fabricmc.net/", "https://bmclapi2.bangbang93.com/maven/")
    } else if original.starts_with("https://cdn.modrinth.com/") {
        // Modrinth CDN → 国内镜像（对齐整合包下载的 mcimirror 镜像）
        original.replace("https://cdn.modrinth.com/", "https://mod.mcimirror.top/")
    } else if original.starts_with("https://cdn-alt.modrinth.com/") {
        original.replace("https://cdn-alt.modrinth.com/", "https://mod.mcimirror.top/")
    } else {
        return None;
    };
    Some(mirror)
}

/// 根据下载源模式生成镜像候选 URL 列表
/// - `china-first`: 镜像在前，官方在后
/// - `mojang`: 只用官方
/// - `auto` / `official-first`: 官方在前，镜像在后
pub fn get_mirror_urls(original: &str, download_source: &str) -> Vec<String> {
    let mut urls = vec![original.to_string()];

    if download_source == "mojang" {
        return urls;
    }

    if let Some(mirror) = to_mirror_url(original) {
        if mirror != original {
            if download_source == "china-first" {
                // 镜像优先，但熔断时跳过
                if is_mirror_available() {
                    urls.insert(0, mirror);
                }
            } else {
                // auto / official-first：官方优先，镜像兜底
                urls.push(mirror);
            }
        }
    }

    // libraries.minecraft.net 额外补 Forge maven 镜像，作为兜底候选（与原项目一致）
    if original.starts_with("https://libraries.minecraft.net/") {
        let forge_mirror = original.replacen(
            "https://libraries.minecraft.net/",
            "https://maven.minecraftforge.net/",
            1,
        );
        if forge_mirror != original && !urls.contains(&forge_mirror) {
            urls.push(forge_mirror);
        }
    }

    urls
}
