// auth/token.rs — Token 加密与解密
//
// 加密格式（与旧版完全兼容）：
//   外层： "enc:" + iv_hex + ":" + ciphertext_hex
//   内层（去掉 "enc:" 后）：iv_hex + ":" + ciphertext_hex
//   算法： AES-256-CBC
//   密钥： SHA256(hostname + username + DATA_DIR)，取前32字节
//   IV：    每次随机生成 16 字节

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use sha2::{Digest, Sha256};

use crate::storage;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// 派生加密密钥
/// SHA256(hostname + username + DATA_DIR)
fn get_token_enc_key() -> [u8; 32] {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    let username = whoami::username();
    let data_dir = storage::resolve_data_dir()
        .to_string_lossy()
        .to_string();

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(username.as_bytes());
    hasher.update(data_dir.as_bytes());
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// 加密 Token
/// 返回格式：iv_hex + ":" + ciphertext_hex（不含 "enc:" 前缀）
pub fn encrypt_token(plaintext: &str) -> Option<String> {
    let key = get_token_enc_key();

    // 生成 16 字节随机 IV
    // 用 SystemTime + hostname + username 哈希作为熵源（避免引入 rand crate）
    let mut iv = [0u8; 16];
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(whoami::username().as_bytes());
    hasher.update(plaintext.as_bytes()); // 加入明文增加熵
    let entropy = hasher.finalize();
    iv.copy_from_slice(&entropy[..16]);

    let cipher = Aes256CbcEnc::new(&key.into(), &iv.into());
    let ciphertext = cipher.encrypt_padded_vec_mut::<Pkcs7>(plaintext.as_bytes());

    let iv_hex = hex_encode(&iv);
    let cipher_hex = hex_encode(&ciphertext);
    Some(format!("{}:{}", iv_hex, cipher_hex))
}

/// 解密 Token
/// 输入：iv_hex + ":" + ciphertext_hex（不含 "enc:" 前缀）
pub fn decrypt_token(data: &str) -> Option<String> {
    let parts: Vec<&str> = data.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let iv = hex_decode(parts[0])?;
    let ciphertext = hex_decode(parts[1])?;
    if iv.len() != 16 {
        return None;
    }

    let key = get_token_enc_key();
    let mut iv_arr = [0u8; 16];
    iv_arr.copy_from_slice(&iv);

    let cipher = Aes256CbcDec::new(&key.into(), &iv_arr.into());
    let plaintext = cipher.decrypt_padded_vec_mut::<Pkcs7>(&ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

/// 解密账号的 Token（自动处理 "enc:" 前缀）
/// 返回解密后的 token；若是离线账号（accessToken="0"）或解密失败则返回原值
pub fn decrypt_account_token(token: &str) -> String {
    if token == "0" || token.is_empty() {
        return token.to_string();
    }
    if let Some(stripped) = token.strip_prefix("enc:") {
        if let Some(decrypted) = decrypt_token(stripped) {
            return decrypted;
        }
    }
    token.to_string()
}

/// 加密账号的 Token（添加 "enc:" 前缀）
pub fn encrypt_account_token(token: &str) -> String {
    if token == "0" || token.is_empty() {
        return token.to_string();
    }
    if let Some(encrypted) = encrypt_token(token) {
        format!("enc:{}", encrypted)
    } else {
        token.to_string()
    }
}

// ============== Hex 编解码工具 ==============

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut result = Vec::with_capacity(hex.len() / 2);
    let chars: Vec<char> = hex.chars().collect();
    for i in (0..chars.len()).step_by(2) {
        let high = chars[i].to_digit(16)?;
        let low = chars[i + 1].to_digit(16)?;
        result.push((high * 16 + low) as u8);
    }
    Some(result)
}

// 简单的 whoami 替代（不引入额外 crate）
mod whoami {
    pub fn username() -> String {
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}
