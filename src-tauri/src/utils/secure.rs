use getrandom::getrandom;
use keyring::Entry;

use crate::constants::KEYRING_SERVICE;

/// keyring 中存储的 clipboard 数据库主密钥信息。
pub struct ClipboardDbKey {
    pub raw_key_hex: String,
    pub was_created: bool,
}

const CLIPBOARD_DB_KEY_ACCOUNT: &str = "clipboard-db-key";
const RAW_KEY_BYTES: usize = 32;
const RAW_KEY_HEX_LEN: usize = RAW_KEY_BYTES * 2;

pub fn get_or_create_clipboard_db_key() -> Result<ClipboardDbKey, String> {
    let entry = Entry::new(KEYRING_SERVICE, CLIPBOARD_DB_KEY_ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(existing) => {
            validate_raw_key_hex(&existing)?;
            Ok(ClipboardDbKey {
                raw_key_hex: existing,
                was_created: false,
            })
        }
        Err(keyring::Error::NoEntry) => {
            let raw_key_hex = generate_raw_key_hex()?;
            entry
                .set_password(&raw_key_hex)
                .map_err(|e| format!("failed to persist clipboard DB key: {e}"))?;
            Ok(ClipboardDbKey {
                raw_key_hex,
                was_created: true,
            })
        }
        Err(e) => Err(format!("failed to read clipboard DB key: {e}")),
    }
}

fn generate_raw_key_hex() -> Result<String, String> {
    let mut bytes = [0u8; RAW_KEY_BYTES];
    getrandom(&mut bytes).map_err(|e| format!("failed to generate encryption key: {e}"))?;
    Ok(bytes_to_hex(&bytes))
}

fn validate_raw_key_hex(value: &str) -> Result<(), String> {
    if value.len() != RAW_KEY_HEX_LEN || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("clipboard DB key in keyring is invalid".to_string());
    }
    Ok(())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}
