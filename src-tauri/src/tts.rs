// VersePC TTS 语音合成模块
// 基于微软 Edge Read Aloud WebSocket API（与 msedge-tts 相同协议）
// 主进程合成音频，返回 MP3 字节数据
// 注意：微软自 2024 年起要求 WebSocket 握手携带 Sec-MS-GEC（SHA-256 令牌）等认证信息，
//       且消息需带 X-Timestamp / X-RequestId 头，音频帧为「2字节长度前缀 + 头部 + 数据」格式。

use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// 微软 Edge TTS 可信客户端令牌（公开常量，非密钥）
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// Sec-MS-GEC 版本（对应 Edge 浏览器 Chromium 版本）
const SEC_MS_GEC_VERSION: &str = "1-143.0.3650.75";
/// Windows 文件时间纪元（1601-01-01）与 Unix 纪元（1970-01-01）的秒差
const WIN_EPOCH: i64 = 11644473600;
/// 浏览器 User-Agent
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// 生成 Sec-MS-GEC 认证令牌：
/// 取 Unix 时间戳 + Windows 纪元偏移 → 向下取整到 5 分钟 → 转 100ns 间隔 → 拼接客户端令牌 → SHA-256 → 大写 hex
fn generate_sec_ms_gec() -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut ticks = unix + WIN_EPOCH;
    ticks -= ticks % 300;
    let ticks_ns = ticks * 10_000_000;
    let str_to_hash = format!("{}{}", ticks_ns, TRUSTED_CLIENT_TOKEN);
    let digest = Sha256::digest(str_to_hash.as_bytes());
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

/// 生成随机 MUID（大写 hex）
fn generate_muid() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02X}", x)).collect()
}

/// 生成 SSML 文本
fn build_ssml(text: &str, voice: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        r#"<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xmlns:mstts="https://www.w3.org/2001/mstts" xml:lang="zh-CN"><voice name="{}"><prosody rate="0%" pitch="0%">{}</prosody></voice></speak>"#,
        voice, escaped
    )
}

/// 构建 WebSocket 配置消息（需带 X-Timestamp 头）
fn build_config_msg() -> String {
    let config_json = r#"{"context":{"synthesis":{"audio":{"metadataoptions":{"sentenceBoundaryEnabled":false,"wordBoundaryEnabled":false},"outputFormat":"audio-24khz-48kbitrate-mono-mp3"}}}}"#;
    let ts = chrono::Utc::now().format("%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)");
    format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{}",
        ts, config_json
    )
}

/// 构建 SSML 消息（需带 X-RequestId 头）
fn build_ssml_msg(ssml: &str) -> String {
    let request_id = Uuid::new_v4().simple().to_string();
    format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nPath:ssml\r\n\r\n{}",
        request_id, ssml
    )
}

/// 使用 Edge TTS WebSocket 协议合成语音，返回 MP3 音频字节
pub async fn synthesize(text: &str, voice: &str) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("空文本".to_string());
    }

    let connection_id = Uuid::new_v4().simple().to_string();
    let gec = generate_sec_ms_gec();
    let muid = generate_muid();
    let ws_url = format!(
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken={}&ConnectionId={}&Sec-MS-GEC={}&Sec-MS-GEC-Version={}",
        TRUSTED_CLIENT_TOKEN, connection_id, gec, SEC_MS_GEC_VERSION
    );

    // 构建带认证头的 WebSocket 握手请求
    let request = http::Request::builder()
        .uri(ws_url)
        .header("Origin", "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold")
        .header("Cookie", format!("muid={};", muid))
        .header("User-Agent", USER_AGENT)
        .header("Accept-Encoding", "gzip, deflate, br, zstd")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Sec-MS-GEC", &gec)
        .header("Sec-MS-GEC-Version", SEC_MS_GEC_VERSION)
        .header("Pragma", "no-cache")
        .header("Cache-Control", "no-cache")
        .body(())
        .map_err(|e| format!("构建握手请求失败: {}", e))?;

    // 连接 WebSocket
    let (ws_stream, _) = connect_async(request)
        .await
        .map_err(|e| format!("WebSocket 连接失败: {}", e))?;

    let (mut write, mut read) = ws_stream.split();

    // 发送配置消息
    let config_msg = build_config_msg();
    write
        .send(Message::Text(config_msg))
        .await
        .map_err(|e| format!("发送配置消息失败: {}", e))?;

    // 发送 SSML 消息
    let ssml = build_ssml(text, voice);
    let ssml_msg = build_ssml_msg(&ssml);
    write
        .send(Message::Text(ssml_msg))
        .await
        .map_err(|e| format!("发送 SSML 消息失败: {}", e))?;

    // 接收音频数据
    let mut audio_chunks: Vec<Vec<u8>> = Vec::new();
    let mut turn_ended = false;
    let mut timeout = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        async {
            loop {
                if turn_ended {
                    break;
                }
                match read.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        // 当前协议：2 字节头长度前缀 + 头部 + 音频数据
                        // 旧协议：头部 + \r\n\r\n + 音频数据
                        if let Some(pos) = extract_audio_from_binary(&data) {
                            audio_chunks.push(pos.to_vec());
                        }
                    }
                    Some(Ok(Message::Text(txt))) => {
                        // turn.end 以文本消息到达，检测到即可结束
                        if txt.contains("Path:turn.end") {
                            turn_ended = true;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        break;
                    }
                    Some(Err(e)) => {
                        return Err(format!("WebSocket 接收错误: {}", e));
                    }
                    None => {
                        break;
                    }
                    _ => {}
                }
            }
            Ok::<_, String>(())
        },
    );

    match timeout.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("TTS 合成超时（15秒）".to_string()),
    }

    if audio_chunks.is_empty() {
        return Err("合成结果为空，未收到音频数据".to_string());
    }

    // 合并所有音频块
    let total_len: usize = audio_chunks.iter().map(|c| c.len()).sum();
    let mut result = Vec::with_capacity(total_len);
    for chunk in audio_chunks {
        result.extend_from_slice(&chunk);
    }

    Ok(result)
}

/// 从二进制帧中提取音频数据（兼容新旧两种协议格式）
fn extract_audio_from_binary(data: &[u8]) -> Option<&[u8]> {
    // 新协议：前 2 字节为大端头长度
    if data.len() >= 2 {
        let mut hlen = u16::from_be_bytes([data[0], data[1]]) as usize;
        if hlen > data.len().saturating_sub(2) {
            hlen = u16::from_le_bytes([data[0], data[1]]) as usize;
        }
        if 2 + hlen <= data.len() {
            let header = &data[2..2 + hlen];
            if header.windows(10).any(|w| w == b"Path:audio") {
                let audio = &data[2 + hlen..];
                // 防御性处理：音频段若仍带 Path:audio 前缀则截掉
                if let Some(p) = find_last_subslice(audio, b"Path:audio\r\n") {
                    return Some(&audio[p + b"Path:audio\r\n".len()..]);
                }
                return Some(audio);
            }
        }
    }
    // 旧协议回退：直接按 \r\n\r\n 分隔
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        let audio = &data[pos + 4..];
        if !audio.is_empty() {
            return Some(audio);
        }
    }
    None
}

/// 查找最后一个子串出现的位置
fn find_last_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut found = None;
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    for (i, w) in haystack.windows(needle.len()).enumerate() {
        if w == needle {
            found = Some(i);
        }
    }
    found
}
