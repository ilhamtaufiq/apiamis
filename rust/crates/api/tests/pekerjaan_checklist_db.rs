//! Centang checklist pekerjaan dan ekspor Excel lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_checklist_db -- --include-ignored
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

const USER: &str = "uji-pc-user@example.test";
const ADMIN: &str = "uji-pc-admin@example.test";
const ITEM_NAME: &str = "uji-pc-item";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Kirim permintaan; kembalikan status, header content-type, dan body mentah.
async fn send_raw(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Vec<u8>) {
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
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let ct = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, ct, bytes.to_vec())
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (s, _, b) = send_raw(pool, method, uri, token, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// User uji dan token. Bila `admin`, diberi peran admin.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji PC', ?, 'x', NOW(), NOW())")
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
        let role: u64 =
            sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
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
    auth::login::create_token(pool, uid, "uji-pc").await.unwrap()
}

async fn cleanup(pool: &MySqlPool, pekerjaan: i64, item: i64) {
    sqlx::query("DELETE FROM pekerjaan_checklist_histories WHERE pekerjaan_id = ? AND checklist_item_id = ?")
        .bind(pekerjaan)
        .bind(item)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM pekerjaan_checklist WHERE pekerjaan_id = ? AND checklist_item_id = ?")
        .bind(pekerjaan)
        .bind(item)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE name = ?")
        .bind(ITEM_NAME)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn pekerjaan_checklist_toggle_and_export() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let pekerjaan: i64 = sqlx::query_scalar("SELECT CAST(MIN(id) AS SIGNED) FROM tbl_pekerjaan")
        .fetch_one(&pool)
        .await
        .unwrap();
    // Bersihkan sisa tes sebelumnya sebelum membuat item uji.
    sqlx::query("DELETE FROM pekerjaan_checklist WHERE checklist_item_id IN (SELECT id FROM tbl_checklist_items WHERE name = ?)")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE name = ?")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_checklist_items (name, sort_order, context, created_at, updated_at) VALUES (?, 999, 'pekerjaan', NOW(), NOW())")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    let item: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_checklist_items WHERE name = ?")
        .bind(ITEM_NAME)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = user_token(&pool, USER, false).await;
    let admin = user_token(&pool, ADMIN, true).await;

    // Validasi: boolean dan exists.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan-checklist/toggle",
        Some(&token),
        Some(json!({ "pekerjaan_id": pekerjaan, "checklist_item_id": item, "is_checked": "mungkin" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The is checked field must be true or false.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan-checklist/toggle",
        Some(&token),
        Some(json!({ "pekerjaan_id": 999999999, "checklist_item_id": item, "is_checked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The selected pekerjaan id is invalid.", "{body}");

    // Centang: 200, lalu baris dan riwayat tercatat.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan-checklist/toggle",
        Some(&token),
        Some(json!({ "pekerjaan_id": pekerjaan, "checklist_item_id": item, "is_checked": true, "notes": "uji" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Checklist updated");
    assert_eq!(body["is_checked"], true);
    assert_eq!(body["checked_by_name"], "Uji PC");

    // Lepas centang: checked_at tetap dari centang sebelumnya.
    let before: (i8, Option<String>) = sqlx::query_as(
        "SELECT is_checked, CAST(checked_at AS CHAR) FROM pekerjaan_checklist WHERE pekerjaan_id = ? AND checklist_item_id = ?",
    )
    .bind(pekerjaan)
    .bind(item)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before.0, 1);
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan-checklist/toggle",
        Some(&token),
        Some(json!({ "pekerjaan_id": pekerjaan, "checklist_item_id": item, "is_checked": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after: (i8, Option<String>) = sqlx::query_as(
        "SELECT is_checked, CAST(checked_at AS CHAR) FROM pekerjaan_checklist WHERE pekerjaan_id = ? AND checklist_item_id = ?",
    )
    .bind(pekerjaan)
    .bind(item)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after.0, 0);
    assert_eq!(after.1, before.1, "checked_at tetap dari centang terakhir");
    let riwayat: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM pekerjaan_checklist_histories WHERE pekerjaan_id = ? AND checklist_item_id = ?",
    )
    .bind(pekerjaan)
    .bind(item)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(riwayat, 2);

    // Ekspor Excel: admin, berkas xlsx (zip "PK").
    let (status, ct, bytes) = send_raw(
        &pool,
        Method::GET,
        "/api/pekerjaan-checklist/export/excel",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ct.unwrap().contains("spreadsheetml"));
    assert_eq!(&bytes[..2], b"PK");

    // Pengguna biasa: ditolak (sama dengan akses penuh di daftar).
    let (status, _) = send(&pool, Method::GET, "/api/pekerjaan-checklist/export/excel", Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup(&pool, pekerjaan, item).await;
}
