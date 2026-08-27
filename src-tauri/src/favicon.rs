// favicon.rs — 网站图标获取
// 供工具箱页面使用，前端通过 invoke('get_favicon', { domain }) 调用
// 优先使用 Yandex（国内可访问）

use serde_json::json;
use std::time::Duration;

/// 尝试从指定 URL 获取图标，成功返回 base64 data URL，失败返回 None
async fn try_fetch_favicon(client: &reqwest::Client, url: &str) -> Option<String> {
    if let Ok(resp) = client.get(url).send().await {
        if resp.status().is_success() {
            if let Ok(bytes) = resp.bytes().await {
                if !bytes.is_empty() && bytes.len() > 100 {
                    let mime = if bytes.len() > 3 && bytes[..3] == [0x47, 0x49, 0x46] {
                        "image/gif"
                    } else if bytes.len() > 2 && bytes[..2] == [0xFF, 0xD8] {
                        "image/jpeg"
                    } else {
                        "image/png"
                    };
                    let data_url = crate::utils::bytes_to_data_url(&bytes, mime);
                    if data_url.len() > 100 {
                        return Some(data_url);
                    }
                }
            }
        }
    }
    None
}

/// Tauri 命令：get_favicon
/// 前端通过 invoke('get_favicon', { domain }) 调用
/// 返回 { data_url } — base64 data URL 或空字符串
/// 服务优先级：
///   1. Yandex（国内可访问）
///   2. DuckDuckGo
///   3. Google
///   4. Google fallback（gstatic）
///   5. 直接拉取域名的 favicon.ico
#[tauri::command]
pub async fn get_favicon(domain: String) -> serde_json::Value {
    if domain.is_empty() {
        return json!({ "data_url": "" });
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("VersePC-Tauri/1.0")
        .build()
    {
        Ok(c) => c,
        Err(_) => return json!({ "data_url": "" }),
    };

    // 服务列表（按优先级）
    let services = [
        format!("https://favicon.yandex.net/favicon/{}?size=32", domain),
        format!("https://icons.duckduckgo.com/ip3/{}.ico", domain),
        format!("https://www.google.com/s2/favicons?domain={}&sz=64", domain),
        format!("https://t0.gstatic.com/faviconV2?client=SOCIAL&type=FAVICON&fallback=0&url=https://{}&size=64", domain),
    ];

    for url in &services {
        if let Some(data_url) = try_fetch_favicon(&client, url).await {
            return json!({ "data_url": data_url });
        }
    }

    // 最后尝试直接拉取域名的 favicon.ico
    let direct_url = format!("https://{}/favicon.ico", domain);
    if let Ok(resp) = client.get(&direct_url).send().await {
        if resp.status().is_success() {
            if let Ok(bytes) = resp.bytes().await {
                if !bytes.is_empty() {
                    let data_url = crate::utils::bytes_to_data_url(&bytes, "image/x-icon");
                    return json!({ "data_url": data_url });
                }
            }
        }
    }

    // 全部失败，返回空
    json!({ "data_url": "" })
}