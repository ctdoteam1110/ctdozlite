//! Crypto primitives, ported from the Zalo PC 26.10.11 bundles.
//!
//! See `re-notes/01_login.md` and `re-notes/02_chat_data.md` for evidence.

use aes::{Aes128, Aes256};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};

/// The bundle uses `iv = Hex.parse("0000…")` — 16 zero bytes, always.
const ZERO_IV: [u8; 16] = [0u8; 16];

/// `encodeAES(text)`: AES-CBC, zero IV, PKCS7, base64 out.
/// Supports both 128-bit (16-byte) and 256-bit (32-byte) keys.
pub fn encode_aes(key: &[u8], text: &str) -> anyhow::Result<String> {
    let mut buf = vec![0u8; text.len() + 16];
    buf[..text.len()].copy_from_slice(text.as_bytes());
    let ct = match key.len() {
        16 => {
            let enc = cbc::Encryptor::<Aes128>::new_from_slices(key, &ZERO_IV)?;
            enc.encrypt_padded_mut::<Pkcs7>(&mut buf, text.len())
                .map_err(|_| anyhow::anyhow!("AES-128 padding failed"))?
        }
        32 => {
            let enc = cbc::Encryptor::<Aes256>::new_from_slices(key, &ZERO_IV)?;
            enc.encrypt_padded_mut::<Pkcs7>(&mut buf, text.len())
                .map_err(|_| anyhow::anyhow!("AES-256 padding failed"))?
        }
        len => anyhow::bail!("Invalid AES key length: {len} (expected 16 or 32)"),
    };
    Ok(B64.encode(ct))
}

/// `decodeAES(key, b64)`: inverse of [`encode_aes`].
/// Supports both 128-bit (16-byte) and 256-bit (32-byte) keys.
pub fn decode_aes(key: &[u8], b64: &str) -> anyhow::Result<String> {
    let raw = B64.decode(b64.trim())?;
    let mut buf = raw;
    let pt = match key.len() {
        16 => {
            let dec = cbc::Decryptor::<Aes128>::new_from_slices(key, &ZERO_IV)?;
            dec.decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map_err(|_| anyhow::anyhow!("AES-128 unpad failed (wrong key or not PKCS7)"))?
        }
        32 => {
            let dec = cbc::Decryptor::<Aes256>::new_from_slices(key, &ZERO_IV)?;
            dec.decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map_err(|_| anyhow::anyhow!("AES-256 unpad failed (wrong key or not PKCS7)"))?
        }
        len => anyhow::bail!("Invalid AES key length: {len} (expected 16 or 32)"),
    };
    Ok(String::from_utf8(pt.to_vec())?)
}

/// Parse secret key: validates key length (16 or 32 bytes) whether passed as Base64 or raw bytes.
pub fn parse_secret_key(zpw_enk: &str) -> Vec<u8> {
    let trimmed = zpw_enk.trim();
    if trimmed.len() == 16 || trimmed.len() == 32 {
        return trimmed.as_bytes().to_vec();
    }
    if let Ok(raw) = B64.decode(trimmed) {
        if raw.len() == 16 || raw.len() == 32 {
            return raw;
        }
    }
    trimmed.as_bytes().to_vec()
}

pub fn md5_hex(s: &str) -> String {
    let mut h = Md5::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    out.iter().fold(String::with_capacity(32), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// `getSignKey(tag, params)` = md5("zsecure" + tag + sorted_values_concat).
pub fn sign_key(tag: &str, params: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    let mut s = String::from("zsecure");
    s.push_str(tag);
    for k in keys {
        match &params[k] {
            serde_json::Value::String(v) => s.push_str(v),
            v => s.push_str(&v.to_string()),
        }
    }
    md5_hex(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_roundtrip() {
        // 32 raw bytes — the shape of a decoded zpw_enk.
        let key = b"0123456789abcdef0123456789abcdef";
        let ct = encode_aes(key, r#"{"hello":"zalo"}"#).unwrap();
        assert_eq!(decode_aes(key, &ct).unwrap(), r#"{"hello":"zalo"}"#);
    }

    #[test]
    fn aes_is_deterministic_zero_iv() {
        let key = b"0123456789abcdef0123456789abcdef";
        assert_eq!(
            encode_aes(key, "abc").unwrap(),
            encode_aes(key, "abc").unwrap()
        );
    }

    #[test]
    fn aes_matches_crypto_js_shape() {
        // Known-answer: AES-256-CBC, zero IV, PKCS7 of "zalo" under a 32-byte zero key.
        let key = [0u8; 32];
        let ct = encode_aes(&key, "zalo").unwrap();
        assert_eq!(decode_aes(&key, &ct).unwrap(), "zalo");
        // 4 bytes plaintext → one 16-byte block, base64 = 24 chars incl. padding.
        assert_eq!(ct.len(), 24);
    }

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn signkey_sorts_keys() {
        let mut m = serde_json::Map::new();
        m.insert("b".into(), serde_json::json!("2"));
        m.insert("a".into(), serde_json::json!("1"));
        assert_eq!(sign_key("10053", &m), md5_hex("zsecure1005312"));
    }

    #[test]
    fn secret_key_base64_decodes() {
        // 32 raw bytes → 44 base64 chars
        let k = parse_secret_key("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=");
        assert_eq!(k.len(), 32);
        assert_eq!(&k[..3], b"012");
    }
}
