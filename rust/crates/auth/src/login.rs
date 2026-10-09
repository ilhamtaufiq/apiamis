//! Login email dan password, setara `AuthController@login` (bagian kredensial dan token).

use chrono::{DateTime, Utc};
use rand::{distributions::Alphanumeric, Rng};
use sqlx::{MySqlPool, Row};

use crate::hash_token;

/// Panjang token seperti Sanctum (`Str::random(40)`).
const TOKEN_LEN: usize = 40;

/// Baris `users` yang dipakai `UserResource`.
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub avatar: Option<String>,
    pub gender: Option<String>,
    pub nip: Option<String>,
    pub jabatan: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub password: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Email termasuk daftar bypass maintenance (perbandingan tanpa huruf besar/kecil).
pub fn is_bypass(email: &str, list: &[String]) -> bool {
    let e = email.trim().to_lowercase();
    list.contains(&e)
}

/// Verifikasi bcrypt. Mendukung hash `$2y$` dari Laravel.
pub fn verify_password(hash: &str, plain: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

pub fn new_plain_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TOKEN_LEN)
        .map(char::from)
        .collect()
}

/// Membuat token seperti `createToken('auth-token')`. Mengembalikan `id|plain`.
pub async fn create_token(
    pool: &MySqlPool,
    user_id: u64,
    name: &str,
) -> Result<String, sqlx::Error> {
    let plain = new_plain_token();
    let result = sqlx::query(
        "INSERT INTO personal_access_tokens \
         (tokenable_type, tokenable_id, name, token, abilities, created_at, updated_at) \
         VALUES ('App\\\\Models\\\\User', ?, ?, ?, '[\"*\"]', UTC_TIMESTAMP(), UTC_TIMESTAMP())",
    )
    .bind(user_id)
    .bind(name)
    .bind(hash_token(&plain))
    .execute(pool)
    .await?;
    Ok(format!("{}|{}", result.last_insert_id(), plain))
}

pub async fn find_by_email(pool: &MySqlPool, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, email, avatar, gender, nip, jabatan, email_verified_at, password, \
         created_at, updated_at FROM users WHERE email = ? LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(UserRow {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            email: r.try_get("email")?,
            avatar: r.try_get("avatar")?,
            gender: r.try_get("gender")?,
            nip: r.try_get("nip")?,
            jabatan: r.try_get("jabatan")?,
            email_verified_at: r.try_get("email_verified_at")?,
            password: r.try_get("password")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
        })
    })
    .transpose()
}

/// Satu user berdasarkan id, dengan kolom yang sama seperti `find_by_email`.
pub async fn find_by_id(pool: &MySqlPool, id: u64) -> Result<Option<UserRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, email, avatar, gender, nip, jabatan, email_verified_at, password, \
         created_at, updated_at FROM users WHERE id = ? LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(UserRow {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            email: r.try_get("email")?,
            avatar: r.try_get("avatar")?,
            gender: r.try_get("gender")?,
            nip: r.try_get("nip")?,
            jabatan: r.try_get("jabatan")?,
            email_verified_at: r.try_get("email_verified_at")?,
            password: r.try_get("password")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
        })
    })
    .transpose()
}

/// `(id, name)` role user, urut `id` role.
pub async fn roles_of(pool: &MySqlPool, user_id: u64) -> Result<Vec<(u64, String)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT r.id, r.name FROM model_has_roles m JOIN roles r ON r.id = m.role_id \
         WHERE m.model_type = 'App\\\\Models\\\\User' AND m.model_id = ? ORDER BY r.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("id")?, r.try_get("name")?)))
        .collect()
}

/// `(id, name)` permission langsung milik user, urut `id`.
pub async fn permissions_of(
    pool: &MySqlPool,
    user_id: u64,
) -> Result<Vec<(u64, String)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT p.id, p.name FROM model_has_permissions m JOIN permissions p ON p.id = m.permission_id \
         WHERE m.model_type = 'App\\\\Models\\\\User' AND m.model_id = ? ORDER BY p.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| Ok((r.try_get("id")?, r.try_get("name")?)))
        .collect()
}

/// Media avatar pertama: `(media id, disk, file_name)`.
pub async fn avatar_media(
    pool: &MySqlPool,
    user_id: u64,
) -> Result<Option<(u64, String, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, disk, file_name FROM media WHERE model_type = 'App\\\\Models\\\\User' \
         AND model_id = ? AND collection_name = 'avatar' ORDER BY order_column, id LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok((
            r.try_get("id")?,
            r.try_get("disk")?,
            r.try_get("file_name")?,
        ))
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hash dibuat dengan `password_hash('uji-login-42', PASSWORD_BCRYPT)` di PHP.
    const PHP_HASH: &str = "$2y$10$5OHUia7uk7phxn6AR/Iyx.m61eqHC8ZFAfr5IkRJ5WPHAli5tv7vm";

    #[test]
    fn verifies_hash_made_by_laravel_php() {
        assert!(verify_password(PHP_HASH, "uji-login-42"));
        assert!(!verify_password(PHP_HASH, "salah"));
        assert!(!verify_password("bukan-hash", "apa-saja"));
    }

    #[test]
    fn plain_token_has_sanctum_length_and_is_alphanumeric() {
        let t = new_plain_token();
        assert_eq!(t.len(), TOKEN_LEN);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(t, new_plain_token());
    }
}
