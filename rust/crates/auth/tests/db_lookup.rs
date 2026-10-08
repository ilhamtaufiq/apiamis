//! Uji integrasi autentikasi token terhadap MySQL sungguhan.
//!
//! Dijalankan manual, karena butuh database yang sudah berisi tabel
//! `personal_access_tokens` dan `users`:
//!
//! ```bash
//! DATABASE_URL=mysql://root@127.0.0.1:3306/apiamis \
//!     cargo test -p auth --test db_lookup -- --ignored
//! ```
//!
//! Setiap test membuat baris token sendiri dan menghapusnya di akhir.

use auth::{authenticate, hash_token, AuthError, USER_TOKENABLE_TYPE};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url)
        .await
        .expect("gagal konek ke MySQL")
}

async fn any_user_id(pool: &MySqlPool) -> u64 {
    sqlx::query("SELECT id FROM users ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("tabel users kosong")
        .try_get("id")
        .unwrap()
}

/// Menyisipkan token dengan `plain` yang diketahui. Mengembalikan `id` baris.
async fn insert_token(
    pool: &MySqlPool,
    tokenable_type: &str,
    tokenable_id: u64,
    plain: &str,
) -> u64 {
    sqlx::query(
        "INSERT INTO personal_access_tokens \
         (tokenable_type, tokenable_id, name, token, abilities, created_at, updated_at) \
         VALUES (?, ?, 'rust-integration-test', ?, '[\"*\"]', NOW(), NOW())",
    )
    .bind(tokenable_type)
    .bind(tokenable_id)
    .bind(hash_token(plain))
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn delete_token(pool: &MySqlPool, id: u64) {
    sqlx::query("DELETE FROM personal_access_tokens WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn valid_token_authenticates_and_returns_abilities() {
    let pool = pool().await;
    let user_id = any_user_id(&pool).await;
    let plain = "uji-valid-7f3a9c";
    let id = insert_token(&pool, USER_TOKENABLE_TYPE, user_id, plain).await;

    let me = authenticate(&pool, &format!("{id}|{plain}")).await.unwrap();
    assert_eq!(me.user_id, user_id);
    assert_eq!(me.token_id, id);
    assert_eq!(me.abilities, vec!["*".to_string()]);

    delete_token(&pool, id).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn wrong_secret_is_rejected() {
    let pool = pool().await;
    let user_id = any_user_id(&pool).await;
    let id = insert_token(&pool, USER_TOKENABLE_TYPE, user_id, "rahasia-benar").await;

    let err = authenticate(&pool, &format!("{id}|rahasia-salah"))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::Invalid));

    delete_token(&pool, id).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn expired_token_is_rejected() {
    let pool = pool().await;
    let user_id = any_user_id(&pool).await;
    let plain = "uji-kedaluwarsa-2b1d";
    let id = insert_token(&pool, USER_TOKENABLE_TYPE, user_id, plain).await;
    sqlx::query(
        "UPDATE personal_access_tokens SET expires_at = NOW() - INTERVAL 1 DAY WHERE id = ?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let err = authenticate(&pool, &format!("{id}|{plain}"))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::Expired));

    delete_token(&pool, id).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn token_for_missing_user_is_rejected() {
    let pool = pool().await;
    let plain = "uji-user-hilang-9e8f";
    let id = insert_token(&pool, USER_TOKENABLE_TYPE, 999_999_999, plain).await;

    let err = authenticate(&pool, &format!("{id}|{plain}"))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::Invalid));

    delete_token(&pool, id).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn non_user_tokenable_type_is_rejected() {
    let pool = pool().await;
    let user_id = any_user_id(&pool).await;
    let plain = "uji-tipe-lain-4c6e";
    let id = insert_token(&pool, "App\\Models\\Pengawas", user_id, plain).await;

    let err = authenticate(&pool, &format!("{id}|{plain}"))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::Invalid));

    delete_token(&pool, id).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn unknown_token_id_and_malformed_input_are_rejected() {
    let pool = pool().await;
    let err = authenticate(&pool, "999999999|apa-saja").await.unwrap_err();
    assert!(matches!(err, AuthError::Invalid));
    let err = authenticate(&pool, "tanpa-pemisah").await.unwrap_err();
    assert!(matches!(err, AuthError::Invalid));
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn real_dump_tokens_reject_random_secret() {
    // Token asli dari dump tidak diketahui plain-nya, jadi hanya bisa diuji
    // bahwa secret acak ditolak dan query berjalan di data nyata.
    let pool = pool().await;
    let rows = sqlx::query("SELECT id FROM personal_access_tokens ORDER BY id LIMIT 50")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        let id: u64 = row.try_get("id").unwrap();
        let err = authenticate(&pool, &format!("{id}|bukan-token-asli"))
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Invalid));
    }
}
