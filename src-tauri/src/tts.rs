// VersePC TTS 语音合成模块
// 基于微软 Edge Read Aloud WebSocket API（与 msedge-tts 相同协议）
// 主进程合成音频，返回 MP3 字节数据

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

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

/// 构建 WebSocket 配置消息
fn build_config_msg() -> String {
    let config_json = r#"{"context":{"synthesis":{"audio":{"metadataoptions":{"sentenceBoundaryEnabled":false,"wordBoundaryEnabled":false},"outputFormat":"audio-24khz-48kbitrate-mono-mp3"}}}}"#;
    format!(
        "Content-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{}",
        config_json
    )
}

/// 构建 SSML 消息
fn build_ssml_msg(ssml: &str) -> String {
    format!(
        "Content-Type:application/ssml+xml\r\nPath:ssml\r\n\r\n{}",
        ssml
    )
}

/// 使用 Edge TTS WebSocket 协议合成语音，返回 MP3 音频字节
pub async fn synthesize(text: &str, voice: &str) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("空文本".to_string());
    }

    let connection_id = Uuid::new_v4().to_string();
    let ws_url = format!(
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken=6A5AA5D939EA40E4B77A40B2A6B2C2AB&ConnectionId={}",
        connection_id
    );

    // 连接 WebSocket
    let (ws_stream, _) = connect_async(&ws_url)
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
                        // 二进制消息格式：文本头 + \r\n\r\n + 音频数据
                        // 找到分隔符位置
                        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header =
                                std::str::from_utf8(&data[..pos]).unwrap_or("");
                            // 提取音频数据
                            if pos + 4 < data.len() {
                                let audio_data = &data[pos + 4..];
                                if !audio_data.is_empty() {
                                    audio_chunks.push(audio_data.to_vec());
                                }
                            }
                            // 检查是否是 turn.end
                            if header.contains("Path:turn.end") {
                                turn_ended = true;
                            }
                        }
                    }
                    Some(Ok(Message::Text(_))) => {
                        // 文本消息通常包含 turn.end 或 metadata，忽略
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