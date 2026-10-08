//! `POST` dan `DELETE /api/error-logs/empty` lewat router terhadap MySQL.
//!
//! Rute ini menghapus SEMUA baris `error_logs`, jadi hanya dijalankan di basis data uji sendiri
//! `apiamis_uji_error_logs_empty` (lihat `rust/fixtures/uji_error_logs_schema.sql`). Satu tes saja
//! di file ini, supaya tidak ada tes lain yang sedang menulis baris saat tabel dikosongkan.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis_uji_error_logs_empty?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test error_logs_empty_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn send(pool: &MySqlPool, method: Method, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(Body::empty()).unwrap())
    .await
    .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kosong', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if admin {
        sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
            .execute(pool)
            .await
            .unwrap();
        let role: u64 = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
            .bind(role)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    auth::login::create_token(pool, uid, "uji-kosong").await.unwrap()
}

async fn seed(pool: &MySqlPool, n: usize) {
    for i in 0..n {
        sqlx::query("INSERT INTO error_logs (source, message, created_at, updated_at) VALUES ('manual', ?, NOW(), NOW())")
            .bind(format!("uji-kosong {i}"))
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn count(pool: &MySqlPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM error_logs")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kosongkan_semua_baris_dan_butuh_admin() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = user_token(&pool, "uji-kosong-admin@example.test", true).await;
    let viewer = user_token(&pool, "uji-kosong-viewer@example.test", false).await;

    // Tabel uji dimulai dari kosong. Ini aman karena basis data ini hanya dipakai tes ini.
    sqlx::query("DELETE FROM error_logs").execute(&pool).await.unwrap();

    for method in [Method::POST, Method::DELETE] {
        seed(&pool, 2).await;
        let (status, _) = send(&pool, method.clone(), "/api/error-logs/empty", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send(&pool, method.clone(), "/api/error-logs/empty", Some(&viewer)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(count(&pool).await, 2, "non-admin tidak boleh menghapus");

        let (status, body) = send(&pool, method.clone(), "/api/error-logs/empty", Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "success": true, "affected": 2 }));
        assert_eq!(count(&pool).await, 0);
    }

    // Kosong dari awal: affected 0.
    let (status, body) = send(&pool, Method::POST, "/api/error-logs/empty", Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["affected"], serde_json::json!(0));

    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email IN (?, ?))")
        .bind("uji-kosong-admin@example.test")
        .bind("uji-kosong-viewer@example.test")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email IN (?, ?)")
        .bind("uji-kosong-admin@example.test")
        .bind("uji-kosong-viewer@example.test")
        .execute(&pool)
        .await
        .unwrap();
}
