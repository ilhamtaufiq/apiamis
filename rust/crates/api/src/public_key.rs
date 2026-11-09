//! Kunci API untuk layanan lain yang membaca data lewat `/api/public/v1/*` (header `X-API-Key`).
//!
//! Terpisah dari token Sanctum milik pengguna: kunci tidak punya peran pengguna, hanya daftar scope
//! (`spam-units:read`, `peripaan:read`), bisa kedaluwarsa dan bisa dicabut. Hanya hash SHA-256 yang disimpan.
//! Kunci asli tampil sekali, saat dibuat lewat `apiamis-api create-api-key`.

use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde_json::Value;
use shared::ApiError;
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};

pub const KEY_PREFIX: &str = "amis_live_";
/// Scope yang dikenal. Tambahkan di sini bila ada endpoint publik baru.
pub const SCOPES: &[&str] = &["spam-units:read", "peripaan:read", "pekerjaan:read"];
/// Jumlah karakter awal yang disimpan terang, untuk mengenali kunci di daftar (bukan untuk autentikasi).
const DISPLAY_PREFIX_LEN: usize = KEY_PREFIX.len() + 6;
/// `last_used_at` hanya diperbarui bila sudah lebih lama dari ini, supaya tiap permintaan tidak menulis.
const LAST_USED_REFRESH_MINUTES: u32 = 5;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Hash SHA-256 heksadesimal dari kunci lengkap.
pub fn hash_key(key: &str) -> String {
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Kunci baru: `amis_live_` diikuti 64 karakter heksadesimal acak (256 bit). Mengembalikan `(kunci, awalan, hash)`.
pub fn generate_key() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("{KEY_PREFIX}{secret}");
    let prefix = key[..DISPLAY_PREFIX_LEN].to_string();
    let hash = hash_key(&key);
    (key, prefix, hash)
}

/// Nilai header `X-API-Key`, bila bentuknya masuk akal (awalan benar, hanya karakter kunci). Yang lain dianggap tidak ada.
pub fn key_from_headers(headers: &HeaderMap) -> Option<&str> {
    let v = headers.get("x-api-key")?.to_str().ok()?.trim();
    let secret = v.strip_prefix(KEY_PREFIX)?;
    (secret.len() >= 32 && secret.len() <= 128 && secret.bytes().all(|c| c.is_ascii_alphanumeric())).then_some(v)
}

/// Apakah `scopes_json` (larik JSON berisi teks) memuat `scope`. JSON rusak berarti tidak ada scope.
pub fn has_scope(scopes_json: &str, scope: &str) -> bool {
    serde_json::from_str::<Value>(scopes_json)
        .ok()
        .and_then(|v| v.as_array().map(|a| a.iter().any(|s| s.as_str() == Some(scope))))
        .unwrap_or(false)
}

/// Memeriksa kunci pada header. 401 bila tidak ada, salah, kedaluwarsa, atau dicabut (tanpa membedakan, supaya
/// penyerang tidak tahu mana yang benar). 403 bila kunci sah tetapi tidak punya scope. Mengembalikan id kunci.
pub async fn authenticate(pool: &MySqlPool, headers: &HeaderMap, scope: &str) -> Result<i64, ApiError> {
    let key = key_from_headers(headers).ok_or_else(ApiError::unauthenticated)?;
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, CAST(scopes AS CHAR) AS scopes FROM api_keys \
         WHERE key_hash = ? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > UTC_TIMESTAMP())",
    )
    .bind(hash_key(key))
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    .ok_or_else(ApiError::unauthenticated)?;
    let id: i64 = row.try_get("id").map_err(internal)?;
    let scopes: String = row.try_get("scopes").map_err(internal)?;
    if !has_scope(&scopes, scope) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "This action is unauthorized."));
    }
    // Pencatatan pemakaian tidak boleh menggagalkan permintaan.
    let _ = sqlx::query(&format!(
        "UPDATE api_keys SET last_used_at = UTC_TIMESTAMP() WHERE id = ? \
         AND (last_used_at IS NULL OR last_used_at < UTC_TIMESTAMP() - INTERVAL {LAST_USED_REFRESH_MINUTES} MINUTE)"
    ))
    .bind(id)
    .execute(pool)
    .await;
    Ok(id)
}

/// Membuat kunci. Mengembalikan `(id, kunci asli)`; kunci asli tidak bisa diambil lagi sesudahnya.
pub async fn create(pool: &MySqlPool, name: &str, scopes: &[String], expires_days: Option<i64>) -> Result<(i64, String), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err("--name wajib, 1 sampai 100 karakter".into());
    }
    if scopes.is_empty() {
        return Err(format!("--scopes wajib, pilihan: {}", SCOPES.join(", ")));
    }
    if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        return Err(format!("scope {bad:?} tidak dikenal, pilihan: {}", SCOPES.join(", ")));
    }
    if expires_days.is_some_and(|d| !(1..=3650).contains(&d)) {
        return Err("--expires-days harus 1 sampai 3650".into());
    }
    let (key, prefix, hash) = generate_key();
    let scopes_json = serde_json::to_string(scopes).map_err(|e| e.to_string())?;
    let result = sqlx::query(
        "INSERT INTO api_keys (name, key_prefix, key_hash, scopes, expires_at, created_at) \
         VALUES (?, ?, ?, CAST(? AS JSON), IF(? IS NULL, NULL, DATE_ADD(UTC_TIMESTAMP(), INTERVAL ? DAY)), UTC_TIMESTAMP())",
    )
    .bind(name)
    .bind(prefix)
    .bind(hash)
    .bind(scopes_json)
    .bind(expires_days)
    .bind(expires_days)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok((result.last_insert_id() as i64, key))
}

/// Baris untuk daftar kunci (tanpa hash).
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

pub async fn list(pool: &MySqlPool) -> Result<Vec<KeyRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, name, key_prefix, CAST(scopes AS CHAR) AS scopes, expires_at, revoked_at, last_used_at \
         FROM api_keys ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(KeyRow {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                prefix: r.try_get("key_prefix")?,
                scopes: r.try_get("scopes")?,
                expires_at: r.try_get("expires_at")?,
                revoked_at: r.try_get("revoked_at")?,
                last_used_at: r.try_get("last_used_at")?,
            })
        })
        .collect()
}

/// Memeriksa daftar scope: tidak kosong, dan setiap nilai ada di `SCOPES`.
pub fn validasi_scopes(scopes: &[String]) -> Result<(), String> {
    if scopes.is_empty() {
        return Err(format!("scope wajib dipilih, pilihan: {}", SCOPES.join(", ")));
    }
    if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
        return Err(format!("scope {bad:?} tidak dikenal, pilihan: {}", SCOPES.join(", ")));
    }
    Ok(())
}

/// Hasil mengubah scope kunci.
pub enum UbahScopeHasil {
    Berhasil,
    TidakAda,
    SudahDicabut,
}

/// Mengganti scope kunci yang belum dicabut. Kunci dan nilainya tidak berubah; hanya daftar scope.
pub async fn ubah_scope(pool: &MySqlPool, id: i64, scopes: &[String]) -> Result<UbahScopeHasil, sqlx::Error> {
    let baris = sqlx::query("SELECT revoked_at FROM api_keys WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(baris) = baris else {
        return Ok(UbahScopeHasil::TidakAda);
    };
    let revoked: Option<DateTime<Utc>> = baris.try_get("revoked_at")?;
    if revoked.is_some() {
        return Ok(UbahScopeHasil::SudahDicabut);
    }
    // Scope disimpan sebagai JSON, sama seperti saat membuat kunci.
    let scopes_json = serde_json::to_string(scopes).unwrap_or_else(|_| "[]".into());
    sqlx::query("UPDATE api_keys SET scopes = CAST(? AS JSON) WHERE id = ?")
        .bind(scopes_json)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(UbahScopeHasil::Berhasil)
}

/// Mencabut kunci. `false` bila id tidak ada atau sudah dicabut.
pub async fn revoke(pool: &MySqlPool, id: i64) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE api_keys SET revoked_at = UTC_TIMESTAMP() WHERE id = ? AND revoked_at IS NULL")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validasi_scopes_menolak_kosong_dan_tidak_dikenal() {
        assert!(validasi_scopes(&[]).is_err());
        assert!(validasi_scopes(&["tidak-ada:read".into()]).is_err());
        assert!(validasi_scopes(&["pekerjaan:read".into(), "spam-units:read".into()]).is_ok());
    }

    use axum::http::HeaderValue;

    fn headers(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-api-key", HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn generated_key_has_prefix_length_and_matching_hash() {
        let (key, prefix, hash) = generate_key();
        assert!(key.starts_with(KEY_PREFIX));
        assert_eq!(key.len(), KEY_PREFIX.len() + 64);
        assert!(key.is_ascii());
        assert!(key.starts_with(&prefix));
        assert_eq!(prefix.len(), DISPLAY_PREFIX_LEN);
        assert_eq!(hash, hash_key(&key));
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn keys_are_unique_and_hash_does_not_contain_the_key() {
        let (a, _, ha) = generate_key();
        let (b, _, hb) = generate_key();
        assert_ne!(a, b);
        assert_ne!(ha, hb);
        assert!(!ha.contains(&a[KEY_PREFIX.len()..]));
    }

    #[test]
    fn hash_matches_known_sha256() {
        // SHA-256("abc")
        assert_eq!(hash_key("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn header_must_have_prefix_and_plausible_secret() {
        let (key, _, _) = generate_key();
        assert_eq!(key_from_headers(&headers(&key)), Some(key.as_str()));
        assert_eq!(key_from_headers(&headers(&format!("  {key}  "))), Some(key.as_str()));
        for bad in [
            "",
            "abc",
            "amis_live_",
            "amis_live_short",
            "amis_test_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "amis_live_zz zz zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert_eq!(key_from_headers(&headers(bad)), None, "{bad:?}");
        }
        assert_eq!(key_from_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn scope_check_is_exact() {
        let s = r#"["spam-units:read","peripaan:read"]"#;
        assert!(has_scope(s, "spam-units:read"));
        assert!(has_scope(s, "peripaan:read"));
        assert!(!has_scope(s, "spam-units"));
        assert!(!has_scope(s, "pekerjaan:write"));
        assert!(!has_scope("[]", "spam-units:read"));
        assert!(!has_scope("bukan json", "spam-units:read"));
        assert!(!has_scope(r#"{"spam-units:read":true}"#, "spam-units:read"));
        assert!(!has_scope(r#"[1,2]"#, "spam-units:read"));
    }

    #[test]
    fn known_scopes_are_listed() {
        assert!(SCOPES.contains(&"spam-units:read") && SCOPES.contains(&"peripaan:read"));
        assert!(SCOPES.contains(&"pekerjaan:read"));
    }
}
