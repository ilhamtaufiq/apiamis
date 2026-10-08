//! `POST /api/koordinat/validate` lewat router terhadap MySQL: validasi input dan hasil validasi.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test koordinat_validate_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-koo-admin@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn post(pool: &MySqlPool, token: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/koordinat/validate")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req)
    .await
    .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn koordinat_validate_checks_input_and_reports_result() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Koordinat', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    let admin: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, admin, "uji-koo")
        .await
        .unwrap();
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Input: pekerjaan wajib dan harus ada; koordinat wajib.
    let (status, body) = post(&pool, &token, json!({ "koordinat": "-6.8, 107.1" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["pekerjaan_id"].is_array(), "{body}");
    let (status, body) = post(
        &pool,
        &token,
        json!({ "pekerjaan_id": 99999999, "koordinat": "-6.8, 107.1" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["pekerjaan_id"].is_array(), "{body}");
    let (status, body) = post(&pool, &token, json!({ "pekerjaan_id": pekerjaan })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["koordinat"].is_array(), "{body}");

    // Koordinat yang tidak bisa dibaca: hasil tidak valid dengan pesan dari Laravel.
    let (status, body) = post(
        &pool,
        &token,
        json!({ "pekerjaan_id": pekerjaan, "koordinat": "bukan koordinat" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["validasi_koordinat"], false, "{body}");
    assert_eq!(
        body["validasi_koordinat_message"],
        "Koordinat tidak dapat dibaca. Gunakan format lat, lng.",
        "{body}"
    );
}
