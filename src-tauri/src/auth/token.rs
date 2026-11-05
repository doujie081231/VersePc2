// auth/token.rs — Token 加密与解密
//
// 加密格式（与旧版完全兼容）：
//   外层： "enc:" + iv_hex + ":" + ciphertext_hex
//   内层（去掉 "enc:" 后）：iv_hex + ":" + ciphertext_hex
//   算法： AES-256-CBC
//   密钥： Windows 用 DPAPI 保护的随机密钥（data/token.key）；其他平台用 0600 权限的随机密钥文件；
//         旧版 SHA256(hostname+username+DATA_DIR) 仅用于解密历史令牌回退。
//   IV：   每次用 CSPRNG 生成 16 字节

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::storage;

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// 旧版派生密钥：只用于解密历史令牌，不再用于写入新令牌
fn legacy_token_key() -> [u8; 32] {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    let username = whoami::username();
    let data_dir = storage::resolve_data_dir().to_string_lossy().to_string();

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(username.as_bytes());
    hasher.update(data_dir.as_bytes());
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

fn token_key_path() -> PathBuf {
    storage::resolve_data_dir().join("token.key")
}

fn write_key_file(path: &Path, bytes: &[u8]) -> Option<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(bytes).ok()?;
        }
        // 并发下另一个进程已创建：读取已有密钥
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Some(()),
        Err(_) => return None,
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Some(())
}

#[cfg(target_os = "windows")]
mod dpapi {
    use std::ffi::c_void;
    use std::ptr;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    fn to_blob(data: &[u8]) -> DataBlob {
        DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        }
    }

    unsafe fn take_blob(blob: &mut DataBlob) -> Option<Vec<u8>> {
        if blob.pb_data.is_null() || blob.cb_data == 0 {
            return None;
        }
        let out = unsafe { std::slice::from_raw_parts(blob.pb_data, blob.cb_data as usize) }.to_vec();
        unsafe { LocalFree(blob.pb_data as *mut c_void) };
        blob.pb_data = ptr::null_mut();
        Some(out)
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        let input = to_blob(data);
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return None;
        }
        unsafe { take_blob(&mut output) }
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        let input = to_blob(data);
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return None;
        }
        unsafe { take_blob(&mut output) }
    }
}

#[cfg(target_os = "windows")]
fn key_from_blob(blob: &[u8]) -> Option<[u8; 32]> {
    let raw = dpapi::unprotect(blob)?;
    if raw.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&raw);
    Some(key)
}

fn load_or_create_key() -> Option<[u8; 32]> {
    let path = token_key_path();
    if let Ok(blob) = std::fs::read(&path) {
        #[cfg(target_os = "windows")]
        {
            return key_from_blob(&blob);
        }
        #[cfg(not(target_os = "windows"))]
        {
            if blob.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&blob);
                return Some(key);
            }
            return None;
        }
    }

    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    #[cfg(target_os = "windows")]
    let stored = dpapi::protect(&key)?;
    #[cfg(not(target_os = "windows"))]
    let stored = key.to_vec();
    write_key_file(&path, &stored)?;

    // 并发创建时读回已有文件，保证所有进程使用同一密钥
    if let Ok(existing) = std::fs::read(&path)
        && existing != stored
    {
        #[cfg(target_os = "windows")]
        {
            return key_from_blob(&existing);
        }
        #[cfg(not(target_os = "windows"))]
        {
            if existing.len() == 32 {
                let mut k = [0u8; 32];
                k.copy_from_slice(&existing);
                return Some(k);
            }
            return None;
        }
    }
    Some(key)
}

/// 当前写入使用的密钥；密钥文件/DPAPI 不可用时退回旧派生密钥
fn current_token_key() -> [u8; 32] {
    load_or_create_key().unwrap_or_else(legacy_token_key)
}

fn decrypt_with(key: &[u8; 32], iv: &[u8], ciphertext: &[u8]) -> Option<String> {
    if iv.len() != 16 {
        return None;
    }
    let mut iv_arr = [0u8; 16];
    iv_arr.copy_from_slice(iv);
    let key_arr = *key;
    let cipher = Aes256CbcDec::new(&key_arr.into(), &iv_arr.into());
    let plaintext = cipher.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

/// 加密 Token
/// 返回格式：iv_hex + ":" + ciphertext_hex（不含 "enc:" 前缀）
pub fn encrypt_token(plaintext: &str) -> Option<String> {
    let key = current_token_key();

    // CSPRNG 生成 16 字节 IV，保证同一明文每次密文不同
    let mut iv = [0u8; 16];
    OsRng.fill_bytes(&mut iv);

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

    let current = current_token_key();
    if let Some(plain) = decrypt_with(&current, &iv, &ciphertext) {
        return Some(plain);
    }
    // 兼容旧版派生密钥加密的历史令牌
    let legacy = legacy_token_key();
    if current != legacy {
        return decrypt_with(&legacy, &iv, &ciphertext);
    }
    None
}

/// 解密账号的 Token（自动处理 "enc:" 前缀）
/// 返回解密后的 token；若是离线账号（accessToken="0"）或解密失败则返回原值
pub fn decrypt_account_token(token: &str) -> String {
    if token == "0" || token.is_empty() {
        return token.to_string();
    }
    if let Some(stripped) = token.strip_prefix("enc:")
        && let Some(decrypted) = decrypt_token(stripped) {
            return decrypted;
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
    if !hex.len().is_multiple_of(2) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_encrypt_decrypt_roundtrip() {
        let encrypted = encrypt_token("access-token-123").unwrap();
        assert_ne!(encrypted, "access-token-123");
        assert_eq!(decrypt_token(&encrypted).as_deref(), Some("access-token-123"));
    }

    #[test]
    fn iv_is_random_for_same_plaintext() {
        let a = encrypt_token("same-token").unwrap();
        let b = encrypt_token("same-token").unwrap();
        assert_ne!(a, b, "同一明文两次加密的 IV/密文必须不同");
    }

    #[test]
    fn legacy_derived_key_tokens_still_decrypt() {
        let key = legacy_token_key();
        let iv = [7u8; 16];
        let cipher = Aes256CbcEnc::new(&key.into(), &iv.into());
        let ciphertext = cipher.encrypt_padded_vec_mut::<Pkcs7>(b"legacy-token");
        let payload = format!("{}:{}", hex_encode(&iv), hex_encode(&ciphertext));
        assert_eq!(decrypt_token(&payload).as_deref(), Some("legacy-token"));
    }

    #[test]
    fn account_token_roundtrip_and_plain_passthrough() {
        let encrypted = encrypt_account_token("token-abc");
        assert!(encrypted.starts_with("enc:"));
        assert_eq!(decrypt_account_token(&encrypted), "token-abc");

        assert_eq!(encrypt_account_token("0"), "0");
        assert_eq!(decrypt_account_token("0"), "0");
        assert_eq!(encrypt_account_token(""), "");
        assert_eq!(decrypt_account_token(""), "");
        assert_eq!(decrypt_account_token("plain-token"), "plain-token");
    }

    #[test]
    fn invalid_encrypted_payloads_return_none() {
        assert_eq!(decrypt_token("no-separator"), None);
        assert_eq!(decrypt_token("zz:00"), None);
        assert_eq!(decrypt_token("00:00"), None);
    }

    #[test]
    fn hex_codec_roundtrip_and_errors() {
        let bytes = [0u8, 15, 255, 16];
        let encoded = hex_encode(&bytes);
        assert_eq!(encoded, "000fff10");
        assert_eq!(hex_decode(&encoded), Some(bytes.to_vec()));
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
    }
}
