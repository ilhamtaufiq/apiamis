//! Tautan SIPD dan pustaka tanda tangan lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test sipd_signature_db -- --include-ignored
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

const SIPD_USER: &str = "uji-sipd-user@example.test";
const SIG_USER: &str = "uji-sig-user@example.test";
const SIG_OTHER: &str = "uji-sig-other@example.test";
const SIG_PLAIN: &str = "uji-sig-plain@example.test";
const SUB_BL: i64 = 987_654_001;
const SIG_NAME: &str = "uji-sig-ttd";
const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

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
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// User uji dan tokennya. Bila `admin`, diberi peran `admin` (Spatie). Dibuat ulang setiap tes.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> (u64, String) {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Sig', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(pool, uid, "uji-sig").await.unwrap();
    (uid, token)
}

/// Hapus tautan uji saja (tes SIPD).
async fn cleanup_links(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_sipd_pekerjaan_links WHERE id_sub_bl = ?")
        .bind(SUB_BL)
        .execute(pool)
        .await
        .unwrap();
}

/// Hapus tanda tangan uji saja (tes signature).
async fn cleanup_signatures(pool: &MySqlPool) {
    sqlx::query("DELETE FROM signature_libraries WHERE name = ?")
        .bind(SIG_NAME)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sipd_links_upsert_index_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup_links(&pool).await;
    let (_, token) = user_token(&pool, SIPD_USER, true).await;

    // Tanpa id_sub_bl: 422 dengan pesan abort.
    let (status, body) = send(&pool, Method::GET, "/api/sipd-pekerjaan-links", Some(&token), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "id_sub_bl wajib diisi");

    // Validasi upsert.
    let (status, body) = send(
        &pool,
        Method::PUT,
        "/api/sipd-pekerjaan-links",
        Some(&token),
        Some(json!({ "id_sub_bl": SUB_BL, "id_rinci_sub_bl": 0, "pekerjaan_id": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The id rinci sub bl field must be at least 1.", "{body}");

    // Simpan lalu ubah: satu baris per (sub, rinci).
    let (status, body) = send(
        &pool,
        Method::PUT,
        "/api/sipd-pekerjaan-links",
        Some(&token),
        Some(json!({ "id_sub_bl": SUB_BL, "id_rinci_sub_bl": 5, "pekerjaan_id": 11 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "data": { "id_rinci_sub_bl": 5, "pekerjaan_id": 11 } }));
    let (status, body) = send(
        &pool,
        Method::PUT,
        "/api/sipd-pekerjaan-links",
        Some(&token),
        Some(json!({ "id_sub_bl": SUB_BL, "id_rinci_sub_bl": 5, "pekerjaan_id": 12 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan_id"], 12);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/sipd-pekerjaan-links?id_sub_bl={SUB_BL}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "data": [{ "id_rinci_sub_bl": 5, "pekerjaan_id": 12 }] }));

    // Lepas tautan: data null.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/sipd-pekerjaan-links",
        Some(&token),
        Some(json!({ "id_sub_bl": SUB_BL, "id_rinci_sub_bl": 5 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "data": null }));

    cleanup_links(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn signature_store_index_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup_signatures(&pool).await;
    let (_, token) = user_token(&pool, SIG_USER, true).await;
    let (_, other_token) = user_token(&pool, SIG_OTHER, true).await;
    let (_, plain_token) = user_token(&pool, SIG_PLAIN, false).await;

    // Pengguna tanpa peran admin dan tanpa rule: mutasi ditolak middleware (sama dengan Laravel).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/signature-libraries",
        Some(&plain_token),
        Some(json!({ "name": SIG_NAME, "mime_type": "image/png", "data_url": PNG, "width": 1, "height": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Validasi: data URL harus gambar base64 yang didukung.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/signature-libraries",
        Some(&token),
        Some(json!({
            "name": SIG_NAME, "mime_type": "image/gif", "data_url": "data:image/gif;base64,AAAA",
            "width": 10, "height": 10
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The data url field format is invalid.", "{body}");

    // Simpan: 201, lalu nama sama diperbarui: 200.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/signature-libraries",
        Some(&token),
        Some(json!({ "name": SIG_NAME, "mime_type": "image/png", "data_url": PNG, "width": 120, "height": 40 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["message"], "Signature berhasil disimpan");
    let id = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/signature-libraries",
        Some(&token),
        Some(json!({ "name": SIG_NAME, "mime_type": "image/png", "data_url": PNG, "width": 130, "height": 40 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Signature berhasil diperbarui");
    assert_eq!(body["data"]["id"], json!(id));
    assert_eq!(body["data"]["width"], 130);

    // Daftar hanya milik sendiri.
    let (status, body) = send(&pool, Method::GET, "/api/signature-libraries", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 1, "{body}");
    let (status, body) = send(&pool, Method::GET, "/api/signature-libraries", Some(&other_token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 0, "{body}");

    // Hapus oleh admin lain: diizinkan (canManage). Setelah itu hilang dari daftar pemilik.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/signature-libraries/{id}"),
        Some(&other_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Simpan lagi untuk tes hapus oleh pemilik.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/signature-libraries",
        Some(&token),
        Some(json!({ "name": SIG_NAME, "mime_type": "image/png", "data_url": PNG, "width": 5, "height": 5 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/signature-libraries/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Signature berhasil dihapus");
    let (status, body) = send(&pool, Method::GET, "/api/signature-libraries", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 0, "{body}");

    cleanup_signatures(&pool).await;
}
