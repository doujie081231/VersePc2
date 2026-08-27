// network/public_ip.rs — 公网 IP 检测
// 职责：通过 HTTP 服务查询本机公网 IP
//
// 多源回退：ipify.cn → ifconfig.me → icanhazip.com
// 缓存 5 分钟避免频繁请求

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

/// 公网 IP 查询源（按优先顺序）
const IP_SOURCES: &[&str] = &[
    "https://ipify.cn/api/ip",
    "https://api.ipify.org?format=json",
    "https://ifconfig.me/ip",
    "https://icanhazip.com/",
];

/// 缓存的公网 IP
struct CachedIp {
    ip: String,
    fetched_at: Instant,
}

static CACHED: Mutex<Option<CachedIp>> = Mutex::new(None);

const CACHE_TTL: Duration = Duration::from_secs(300); // 5 分钟

/// 获取公网 IP（带缓存）
pub async fn get_public_ip() -> Result<String, String> {
    // 先读缓存
    {
        let g = CACHED.lock().unwrap();
        if let Some(cached) = g.as_ref() {
            if cached.fetched_at.elapsed() < CACHE_TTL {
                return Ok(cached.ip.clone());
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("VersePC/1.0")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let mut last_err = String::new();
    for url in IP_SOURCES {
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let text = resp.text().await.unwrap_or_default();
                let ip = parse_ip_from_response(&text);
                if !ip.is_empty() && is_valid_ipv4(&ip) {
                    // 写缓存
                    let mut g = CACHED.lock().unwrap();
                    *g = Some(CachedIp {
                        ip: ip.clone(),
                        fetched_at: Instant::now(),
                    });
                    return Ok(ip);
                }
                last_err = format!("无效 IP: {}", text.trim());
            }
            Ok(resp) => {
                last_err = format!("HTTP {}", resp.status());
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }

    Err(format!("所有 IP 源查询失败: {}", last_err))
}

/// 解析 IP（支持纯文本 / JSON 字段）
fn parse_ip_from_response(text: &str) -> String {
    let trimmed = text.trim();
    // 纯文本
    if is_valid_ipv4(trimmed) {
        return trimmed.to_string();
    }
    // JSON {"ip":"1.2.3.4"} 或 {"ip":"..."}
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(ip) = v.get("ip").and_then(|i| i.as_str()) {
            return ip.to_string();
        }
    }
    String::new()
}

/// 简单校验 IPv4 格式
fn is_valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u8>().is_ok())
}
