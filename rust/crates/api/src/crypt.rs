//! Enkripsi kompatibel dengan `Illuminate\Encryption\Encrypter` (AES-256-CBC + HMAC-SHA256).
//!
//! Format payload sama dengan `Crypt::encryptString`: base64 dari JSON
//! `{"iv": b64, "value": b64, "mac": hex, "tag": ""}`, dengan mac = HMAC(key, iv_b64 + value_b64).
//! Kunci dibaca dari `APP_KEY` (format `base64:...` atau 32 byte mentah).

use aes::Aes256;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::Sha256;

type Enc = cbc::Encryptor<Aes256>;
type Dec = cbc::Decryptor<Aes256>;

#[derive(Debug, PartialEq, Eq)]
pub enum CryptError {
    Key,
    Format,
    Mac,
    Decrypt,
}

/// Kunci 32 byte dari `APP_KEY`. Tanpa `base64:` dipakai apa adanya (harus 32 byte).
pub fn key_from_app_key(app_key: &str) -> Result<Vec<u8>, CryptError> {
    let key = match app_key.strip_prefix("base64:") {
        Some(b) => B64.decode(b).map_err(|_| CryptError::Key)?,
        None => app_key.as_bytes().to_vec(),
    };
    if key.len() == 32 {
        Ok(key)
    } else {
        Err(CryptError::Key)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn mac_of(key: &[u8], iv_b64: &str, value_b64: &str) -> Hmac<Sha256> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC menerima kunci ukuran apa pun");
    mac.update(iv_b64.as_bytes());
    mac.update(value_b64.as_bytes());
    mac
}

/// Mendekripsi payload `encryptString`. Mac diverifikasi sebelum dekripsi.
pub fn decrypt_string(key: &[u8], payload_b64: &str) -> Result<String, CryptError> {
    let json = B64.decode(payload_b64).map_err(|_| CryptError::Format)?;
    let p: Value = serde_json::from_slice(&json).map_err(|_| CryptError::Format)?;
    let field = |name: &str| {
        p[name]
            .as_str()
            .map(str::to_string)
            .ok_or(CryptError::Format)
    };
    let (iv_s, value_s, mac_s) = (field("iv")?, field("value")?, field("mac")?);

    let expected = unhex(&mac_s).ok_or(CryptError::Format)?;
    mac_of(key, &iv_s, &value_s)
        .verify_slice(&expected)
        .map_err(|_| CryptError::Mac)?;

    let iv = B64.decode(&iv_s).map_err(|_| CryptError::Format)?;
    let ct = B64.decode(&value_s).map_err(|_| CryptError::Format)?;
    let pt = Dec::new_from_slices(key, &iv)
        .map_err(|_| CryptError::Format)?
        .decrypt_padded_vec_mut::<Pkcs7>(&ct)
        .map_err(|_| CryptError::Decrypt)?;
    String::from_utf8(pt).map_err(|_| CryptError::Decrypt)
}

/// Mengenkripsi teks dengan format yang sama seperti `Crypt::encryptString`.
pub fn encrypt_string(key: &[u8], plain: &str) -> String {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);
    let ct = Enc::new_from_slices(key, &iv)
        .expect("kunci 32 byte dan iv 16 byte selalu valid")
        .encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());

    let iv_b64 = B64.encode(iv);
    let value_b64 = B64.encode(ct);
    let mac = hex(&mac_of(key, &iv_b64, &value_b64).finalize().into_bytes());
    let payload = json!({ "iv": iv_b64, "value": value_b64, "mac": mac, "tag": "" });
    B64.encode(payload.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vektor dari PHP `openssl_encrypt` + format Laravel, dengan kunci contoh 32 x 'k'.
    const VECTOR: &str = "eyJpdiI6ImFXbHBhV2xwYVdscGFXbHBhV2xwYVE9PSIsInZhbHVlIjoiT3lPNmJwWFAyVjd4dFJTeFl3M2RPQT09IiwibWFjIjoiMDVhYzYxOWFiNzBhYTdmZjMxMWIzZGQ2MTFiODlkMWViODk3NmQ2ZWVjZDk0MzY2M2Y3NjFjNDQ0NDI0YmI4ZSIsInRhZyI6IiJ9";

    fn key() -> Vec<u8> {
        vec![b'k'; 32]
    }

    #[test]
    fn decrypts_laravel_vector() {
        assert_eq!(decrypt_string(&key(), VECTOR).unwrap(), "Rahasia 123");
    }

    #[test]
    fn rejects_wrong_key_before_decrypting() {
        assert_eq!(
            decrypt_string(&vec![b'x'; 32], VECTOR),
            Err(CryptError::Mac)
        );
    }

    #[test]
    fn roundtrip_matches_laravel_layout() {
        let enc = encrypt_string(&key(), "Jl. Contoh No. 5");
        assert_eq!(decrypt_string(&key(), &enc).unwrap(), "Jl. Contoh No. 5");
        let json = String::from_utf8(B64.decode(&enc).unwrap()).unwrap();
        assert!(json.starts_with(r#"{"iv":"#) && json.contains(r#""tag":"""#));
    }

    #[test]
    fn app_key_base64_and_raw_forms() {
        let raw = "k".repeat(32);
        assert_eq!(key_from_app_key(&raw).unwrap(), key());
        let b64 = format!("base64:{}", B64.encode(key()));
        assert_eq!(key_from_app_key(&b64).unwrap(), key());
        assert_eq!(key_from_app_key("pendek"), Err(CryptError::Key));
    }
}
