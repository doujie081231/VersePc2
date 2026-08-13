// utils.rs — 通用工具函数
// 包含：UUID 生成、ISO 时间戳、日期转换等

use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// 生成简单 UUID v4（基于时间戳+计数器，足够离线使用）
/// 复刻原项目 utils.generateUUID()
pub fn generate_simple_uuid() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let ts = now as u64;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let seed = ts ^ (c.wrapping_mul(0x9E3779B97F4A7C15));

    let bytes = seed.to_le_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        // version 4
        (c as u8 & 0x0F) | 0x40,
        (c.rotate_right(8) as u8 & 0x3F) | 0x80,
        bytes[0] ^ 0x55, bytes[1] ^ 0xAA,
        bytes[2] ^ 0x55, bytes[3] ^ 0xAA,
        bytes[4] ^ 0x55, bytes[5] ^ 0xAA,
    )
}

/// 生成 Minecraft 离线账号 UUID（Java 版 OfflinePlayer 算法）
/// 复刻原项目 server/api/routes/accounts.js:178 - MD5('OfflinePlayer:' + username) 并设置 version/variant 位
pub fn offline_uuid(username: &str) -> String {
    let input = format!("OfflinePlayer:{}", username);
    let digest = md5::compute(input.as_bytes());
    let mut bytes = digest.0;

    // 设置 version 位（第 6 字节高 4 位为 0011 = version 3）
    bytes[6] = (bytes[6] & 0x0F) | 0x30;
    // 设置 variant 位（第 8 字节高 2 位为 10）
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// 生成当前时间的 ISO 8601 字符串（UTC，用于账号 createdAt 字段）
/// 复刻原项目 new Date().toISOString()
pub fn now_iso() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;

    let (y, mo, d) = days_to_ymd(days as i64);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, mo, d, h, m, s, millis
    )
}

/// UNIX 天数 → 年月日（Howard Hinnant 算法）
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 把字节数据编码成 base64 data URL（用于头像返回）
/// 例如 data:image/png;base64,xxxx
pub fn bytes_to_data_url(bytes: &[u8], mime: &str) -> String {
    use base64::{engine::general_purpose, Engine as _};
    let b64 = general_purpose::STANDARD.encode(bytes);
    format!("data:{};base64,{}", mime, b64)
}

/// 从 Value 中安全取字符串字段
pub fn get_str(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// 从 Value 中安全取字符串字段（trim 后）
pub fn get_str_trim(v: &Value, key: &str) -> String {
    get_str(v, key).trim().to_string()
}

/// 从 Value 中安全取 u64 字段
pub fn get_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

/// 从 Value 中安全取 bool 字段
pub fn get_bool(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

// ============== 文件校验工具 ==============
// 复刻原项目 server/utils.js 的 calculateSHA1Sync / isJarIntact

/// 计算文件 SHA1（同步流式读取，对小/中文件适用）
/// 大文件建议改用异步流式实现，但依赖检查中的库文件通常 < 50MB
pub fn calculate_sha1(path: &std::path::Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    let mut hasher = Sha1::new();
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// 校验 JAR 文件完整性（PK 头 + EOCD 尾）
/// 复刻原项目 server/utils.js:isJarIntact
pub fn is_jar_intact(path: &std::path::Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let size = metadata.len();
    if size < 200 {
        return false;
    }

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    // 读文件头 4 字节，必须是 PK\x03\x04
    let mut header = [0u8; 4];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    if header != [0x50, 0x4B, 0x03, 0x04] {
        return false;
    }

    if size < 22 {
        // 已经通过 size < 200 检查，这里不会到达
        return true;
    }

    // 读末尾最多 65557 字节，搜索 EOCD 签名 0x06054B50（小端 50 4B 05 06）
    let buf_size = std::cmp::min(65557u64, size) as usize;
    let search_start = size - buf_size as u64;
    let mut buf = vec![0u8; buf_size];
    if file.seek(SeekFrom::Start(search_start)).is_err() {
        return false;
    }
    if file.read_exact(&mut buf).is_err() {
        return false;
    }

    let mut eocd_offset: isize = -1;
    let mut i = buf.len() as isize - 22;
    while i >= 0 {
        let idx = i as usize;
        if buf[idx] == 0x50 && buf[idx + 1] == 0x4B && buf[idx + 2] == 0x05 && buf[idx + 3] == 0x06 {
            eocd_offset = i;
            break;
        }
        i -= 1;
    }
    if eocd_offset < 0 {
        return false;
    }

    let eocd = eocd_offset as usize;
    let comment_len = u16::from_le_bytes([buf[eocd + 20], buf[eocd + 21]]) as u64;
    let eocd_offset_in_file = search_start + eocd as u64;
    if eocd_offset_in_file + 22 + comment_len > size {
        return false;
    }

    // 校验中央目录头
    let cd_offset = u32::from_le_bytes([
        buf[eocd + 16],
        buf[eocd + 17],
        buf[eocd + 18],
        buf[eocd + 19],
    ]) as u64;
    if cd_offset + 4 > size {
        return false;
    }
    let mut cd_hdr = [0u8; 4];
    if file.seek(SeekFrom::Start(cd_offset)).is_err() {
        return false;
    }
    if file.read_exact(&mut cd_hdr).is_err() {
        return false;
    }
    // 中央目录签名 0x02014B50（小端 50 4B 01 02）
    cd_hdr == [0x50, 0x4B, 0x01, 0x02]
}
