//! Autentikasi token Sanctum.
//!
//! Format token yang diberikan ke klien: `{id}|{plain}`. Di tabel
//! `personal_access_tokens`, kolom `token` berisi `sha256(plain)` dalam hex.
//! Alur sama dengan Sanctum: cari baris berdasarkan `id`, cocokkan hash,
//! cek kedaluwarsa, lalu pastikan user pemilik token ada.

pub mod login;
pub mod permission;

use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};
use subtle::ConstantTimeEq;

/// Nilai `tokenable_type` untuk token milik user (`App\Models\User`).
pub const USER_TOKENABLE_TYPE: &str = r"App\Models\User";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Token kosong, format salah, tidak cocok, atau user tidak ada. Respon 401.
    #[error("Unauthenticated.")]
    Invalid,
    /// Token cocok tetapi sudah kedaluwarsa. Respon 401.
    #[error("Unauthenticated.")]
    Expired,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Identitas yang sudah terverifikasi dari token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthUser {
    pub user_id: u64,
    pub token_id: u64,
    pub abilities: Vec<String>,
}

/// Memverifikasi header `Authorization: Bearer <token>` (nilai tanpa prefix).
pub async fn authenticate(pool: &MySqlPool, bearer: &str) -> Result<AuthUser, AuthError> {
    let (token_id, plain) = parse_token(bearer).ok_or(AuthError::Invalid)?;

    let row = sqlx::query(
        "SELECT token, tokenable_type, tokenable_id, abilities, \
         CAST(expires_at IS NOT NULL AND expires_at < NOW() AS SIGNED) AS expired \
         FROM personal_access_tokens WHERE id = ?",
    )
    .bind(token_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AuthError::Invalid)?;

    let stored_hash: String = row.try_get("token")?;
    let tokenable_type: String = row.try_get("tokenable_type")?;
    let user_id: u64 = row.try_get("tokenable_id")?;
    let abilities_raw: Option<String> = row.try_get("abilities")?;
    let expired: i64 = row.try_get("expired")?;

    if tokenable_type != USER_TOKENABLE_TYPE || !verify_token(&stored_hash, plain) {
        return Err(AuthError::Invalid);
    }
    if expired != 0 {
        return Err(AuthError::Expired);
    }

    let user_exists = sqlx::query("SELECT 1 FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    if !user_exists {
        return Err(AuthError::Invalid);
    }

    let abilities = abilities_raw
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default();

    Ok(AuthUser {
        user_id,
        token_id,
        abilities,
    })
}

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
