//! Uji umur token: token lebih tua dari `SANCTUM_TOKEN_EXPIRATION` (default 720 menit) ditolak,
//! seperti Sanctum di Laravel. Dijalankan dengan `--ignored` dan `DATABASE_URL`.

use auth::{authenticate, AuthError};
use sqlx::MySqlPool;

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.expect("gagal konek ke MySQL")
}

async fn any_user_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>("SELECT id FROM users ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("tidak ada user di DB uji")
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn token_baru_diterima_dan_token_lewat_umur_ditolak() {
    let pool = pool().await;
    let uid = any_user_id(&pool).await;

    let bearer = auth::login::create_token(&pool, uid, "uji-expiry")
        .await
        .expect("gagal membuat token");
    let token_id: u64 = bearer.split('|').next().unwrap().parse().unwrap();

    // Token baru masih berlaku.
    authenticate(&pool, &bearer).await.expect("token baru harus diterima");

    // Mundurkan umur token 721 menit: harus ditolak sebagai Expired.
    sqlx::query("UPDATE personal_access_tokens SET created_at = UTC_TIMESTAMP() - INTERVAL 721 MINUTE WHERE id = ?")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
    let err = authenticate(&pool, &bearer).await.expect_err("token lewat umur harus ditolak");
    assert!(matches!(err, AuthError::Expired), "{err:?}");

    sqlx::query("DELETE FROM personal_access_tokens WHERE id = ?")
        .bind(token_id)
        .execute(&pool)
        .await
        .unwrap();
}
