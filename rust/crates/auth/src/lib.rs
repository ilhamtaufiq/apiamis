//! Verifikasi token Sanctum (tanpa akses database).
//!
//! Format token yang diberikan ke klien: `{id}|{plain}`. Di tabel
//! `personal_access_tokens`, kolom `token` berisi `sha256(plain)` dalam hex.
//! Lookup ke database ditambahkan saat modul auth dihubungkan ke `db`.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Memecah `{id}|{plain}`. Mengembalikan `None` jika format tidak sesuai.
pub fn parse_token(raw: &str) -> Option<(u64, &str)> {
    let (id, plain) = raw.split_once('|')?;
    let id: u64 = id.parse().ok()?;
    if plain.is_empty() {
        return None;
    }
    Some((id, plain))
}

/// Hash yang disimpan di database: `sha256(plain)` dalam hex huruf kecil.
pub fn hash_token(plain: &str) -> String {
    hex::encode(Sha256::digest(plain.as_bytes()))
}

/// Membandingkan hash tersimpan dengan token dari klien, tanpa bocor waktu.
pub fn verify_token(stored_hash: &str, plain: &str) -> bool {
    let candidate = hash_token(plain);
    stored_hash.as_bytes().ct_eq(candidate.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_id_and_plain() {
        assert_eq!(parse_token("12|abc"), Some((12, "abc")));
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert_eq!(parse_token("abc"), None);
        assert_eq!(parse_token("x|abc"), None);
        assert_eq!(parse_token("12|"), None);
    }

    #[test]
    fn hash_matches_known_sha256_vector() {
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_accepts_matching_token_only() {
        let stored = hash_token("secret-token");
        assert!(verify_token(&stored, "secret-token"));
        assert!(!verify_token(&stored, "other-token"));
    }
}
