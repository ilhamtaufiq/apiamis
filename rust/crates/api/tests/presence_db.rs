//! Kehadiran online lewat router terhadap MySQL: heartbeat, validasi, dan daftar online.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test presence_db -- --include-ignored
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

const EMAIL: &str = "uji-presence@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(body).unwrap())
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
async fn heartbeat_validates_and_lists_online_users() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(EMAIL)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Presence', ?, 'x', NOW(), NOW())")
        .bind(EMAIL)
        .execute(&pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(EMAIL)
        .fetch_one(&pool)
        .await
        .unwrap();
    // Mutasi non-admin tanpa rule ditolak `check.route.permission` (sama dengan Laravel), jadi user uji admin.
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, uid, "uji-presence")
        .await
        .unwrap();

    // Tanpa token: 401.
    let (status, _) = send(&pool, Method::GET, "/api/presence/online", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Koordinat salah format dan office_x di luar rentang: 422.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/presence/heartbeat",
        Some(&token),
        Some(json!({ "koordinat": "-6.82 107.14" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"], "The koordinat field format is invalid.",
        "{body}"
    );
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/presence/heartbeat",
        Some(&token),
        Some(json!({ "office_x": 150 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"], "The office x field must not be greater than 100.",
        "{body}"
    );

    // Heartbeat sah: app tidak dikenal menjadi portal, koordinat tersimpan.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/presence/heartbeat",
        Some(&token),
        Some(json!({ "app": "lainnya", "koordinat": "-6.82, 107.14", "office_x": 10, "office_room": "R1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "data": { "ok": true, "online_window_minutes": 5 } })
    );

    // Heartbeat berikutnya tanpa koordinat mempertahankan koordinat sebelumnya.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/presence/heartbeat",
        Some(&token),
        Some(json!({ "app": "pengawasan" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/presence/online",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["online_window_minutes"], json!(5));
    let me = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == json!(uid))
        .unwrap_or_else(|| panic!("pengguna uji harus online: {body}"));
    assert_eq!(me["app"], json!("pengawasan"));
    assert_eq!(me["koordinat"], json!("-6.82, 107.14"));
    assert!(me["koordinat_at"].is_string(), "{me}");
    assert!(
        me["office_x"].is_null() && me.get("office_room").is_none(),
        "{me}"
    );

    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(EMAIL)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(EMAIL)
        .execute(&pool)
        .await
        .unwrap();
}
