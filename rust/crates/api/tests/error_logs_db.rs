//! Rute admin `/api/error-logs` lewat router terhadap MySQL (daftar, detail, selesai/buka, dan hapus massal).
//!
//! Dijalankan di basis data uji terpisah `apiamis_uji_error_logs` (lihat `rust/fixtures/uji_error_logs_schema.sql`).
//! Tes `/empty` yang menghapus seluruh tabel ada di `error_logs_empty_db`, bukan di sini.
//! Setiap tes memakai pesan `uji-err-<nama>` dan email sendiri, dan hanya menghapus id yang dibuatnya.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis_uji_error_logs?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test error_logs_db -- --include-ignored
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
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn pool_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

async fn connect() -> MySqlPool {
    MySqlPool::connect(&pool_url()).await.unwrap()
}

/// Pengguna uji dengan peran `admin` atau tanpa peran, lalu token Sanctum.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> (u64, String) {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Err', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(pool, uid, "uji-err").await.unwrap();
    (uid, token)
}

/// Baris uji `error_logs`. `created_at` dipakai untuk urutan daftar.
async fn insert_log(
    pool: &MySqlPool,
    message: &str,
    source: &str,
    user_id: Option<u64>,
    created_at: &str,
    metadata: Option<&str>,
) -> u64 {
    let id = sqlx::query(
        "INSERT INTO error_logs (user_id, source, message, url, ip_address, metadata, created_at, updated_at) \
         VALUES (?, ?, ?, 'http://uji-err.test/halaman', '127.0.0.1', ?, ?, ?)",
    )
    .bind(user_id)
    .bind(source)
    .bind(message)
    .bind(metadata)
    .bind(created_at)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    id
}

async fn mark_resolved(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE error_logs SET resolved_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

/// `true` bila `resolved_at` terisi.
async fn is_resolved(pool: &MySqlPool, id: u64) -> bool {
    let v: i64 = sqlx::query_scalar(
        "SELECT CAST(resolved_at IS NOT NULL AS SIGNED) FROM error_logs WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    v == 1
}

async fn updated_at(pool: &MySqlPool, id: u64) -> String {
    sqlx::query_scalar("SELECT CAST(updated_at AS CHAR) FROM error_logs WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn log_exists(pool: &MySqlPool, id: u64) -> bool {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM error_logs WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    n == 1
}

/// Hapus baris uji yang dibuat tes ini, lalu akun uji.
async fn cleanup(pool: &MySqlPool, log_ids: &[u64], emails: &[&str]) {
    for id in log_ids {
        sqlx::query("DELETE FROM error_logs WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    for email in emails {
        sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE email = ?")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
    }
}

const ADMIN_AUTH: &str = "uji-err-admin-auth@example.test";
const ADMIN_DAFTAR: &str = "uji-err-admin-daftar@example.test";
const ADMIN_DETAIL: &str = "uji-err-admin-detail@example.test";
const ADMIN_STATUS: &str = "uji-err-admin-status@example.test";
const ADMIN_BULK: &str = "uji-err-admin-bulk@example.test";
const ADMIN_HAPUS: &str = "uji-err-admin-hapus@example.test";
const VIEWER_DAFTAR: &str = "uji-err-viewer-daftar@example.test";
const VIEWER_STATUS: &str = "uji-err-viewer-status@example.test";
const VIEWER_BULK: &str = "uji-err-viewer-bulk@example.test";

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn daftar_butuh_admin() {
    let pool = connect().await;
    let (_, admin) = user_token(&pool, ADMIN_AUTH, true).await;
    let (_, viewer) = user_token(&pool, VIEWER_DAFTAR, false).await;

    let (status, _) = send(&pool, Method::GET, "/api/error-logs", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&pool, Method::GET, "/api/error-logs", Some(&viewer), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = send(&pool, Method::GET, "/api/error-logs", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], json!(true));
    assert!(body["meta"]["total"].is_u64());

    cleanup(&pool, &[], &[ADMIN_AUTH, VIEWER_DAFTAR]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn daftar_filter_urutan_dan_meta() {
    let pool = connect().await;
    let (uid, admin) = user_token(&pool, ADMIN_DAFTAR, true).await;
    let marker = "uji-err-daftar";

    let a = insert_log(&pool, &format!("{marker} a"), "react", Some(uid), "2026-01-01 10:00:00", Some(r#"{"app":"web"}"#)).await;
    let b = insert_log(&pool, &format!("{marker} b"), "react", Some(uid), "2026-01-02 10:00:00", None).await;
    mark_resolved(&pool, b).await;
    let c = insert_log(&pool, &format!("{marker} c"), "manual", None, "2026-01-03 10:00:00", Some("[]")).await;
    let ids = [a, b, c];

    // Filter source dan status, dibatasi `search` supaya hanya baris uji yang terhitung.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}&source=react&status=open"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], json!(a));
    assert_eq!(data[0]["resolved_at"], Value::Null);
    assert_eq!(data[0]["metadata"], json!({"app": "web"}));
    assert_eq!(data[0]["user"]["email"], json!(ADMIN_DAFTAR));
    assert!(data[0]["user"].get("password").is_none());
    assert!(data[0]["user"].get("remember_token").is_none());

    // Semua baris uji, terbaru dulu. Metadata kosong dan user null.
    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}"),
        Some(&admin),
        None,
    )
    .await;
    let data = body["data"].as_array().unwrap();
    let order: Vec<u64> = data.iter().map(|d| d["id"].as_u64().unwrap()).collect();
    assert_eq!(order, vec![c, b, a]);
    assert_eq!(data[0]["user"], Value::Null);
    assert_eq!(data[0]["metadata"], json!([]));
    assert_eq!(data[1]["metadata"], Value::Null);
    assert_ne!(data[1]["resolved_at"], Value::Null);
    assert_eq!(data[0]["created_at"], json!("2026-01-03T10:00:00.000000Z"));
    let meta = &body["meta"];
    assert_eq!(meta["current_page"], json!(1));
    assert_eq!(meta["last_page"], json!(1));
    assert_eq!(meta["per_page"], json!(15));
    assert_eq!(meta["total"], json!(3));
    assert_eq!(meta["from"], json!(1));
    assert_eq!(meta["to"], json!(3));

    // Paginasi: 2 per halaman, halaman 2 berisi satu baris.
    let (_, page2) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}&per_page=2&page=2"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(page2["data"].as_array().unwrap().len(), 1);
    assert_eq!(page2["data"][0]["id"], json!(a));
    assert_eq!(page2["meta"]["last_page"], json!(2));
    assert_eq!(page2["meta"]["from"], json!(3));
    assert_eq!(page2["meta"]["to"], json!(3));

    // Halaman di luar jangkauan: data kosong, from dan to null.
    let (_, past) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}&per_page=2&page=9"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(past["data"], json!([]));
    assert_eq!(past["meta"]["from"], Value::Null);
    assert_eq!(past["meta"]["to"], Value::Null);
    assert_eq!(past["meta"]["total"], json!(3));

    // per_page di atas 100 dijepit ke 100.
    let (_, big) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}&per_page=500"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(big["meta"]["per_page"], json!(100));

    // Pencarian tanpa cocok menghasilkan data kosong dan total nol.
    let (_, none) = send(
        &pool,
        Method::GET,
        &format!("/api/error-logs?search={marker}-tidak-ada"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(none["data"], json!([]));
    assert_eq!(none["meta"]["total"], json!(0));
    assert_eq!(none["meta"]["last_page"], json!(1));

    cleanup(&pool, &ids, &[ADMIN_DAFTAR]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn detail_dan_404() {
    let pool = connect().await;
    let (uid, admin) = user_token(&pool, ADMIN_DETAIL, true).await;
    let id = insert_log(&pool, "uji-err-detail", "fatal", Some(uid), "2026-02-01 08:00:00", None).await;

    let (status, body) = send(&pool, Method::GET, &format!("/api/error-logs/{id}"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["id"], json!(id));
    assert_eq!(body["data"]["source"], json!("fatal"));
    assert_eq!(body["data"]["user"]["name"], json!("Uji Err"));
    assert_eq!(body["data"]["ip_address"], json!("127.0.0.1"));

    let (status, _) = send(&pool, Method::GET, "/api/error-logs/999999999", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::GET, "/api/error-logs/abc", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, &[id], &[ADMIN_DETAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn selesai_dan_buka_satu_baris() {
    let pool = connect().await;
    let (_, admin) = user_token(&pool, ADMIN_STATUS, true).await;
    let (_, viewer) = user_token(&pool, VIEWER_STATUS, false).await;
    let id = insert_log(&pool, "uji-err-status", "react", None, "2026-03-01 08:00:00", None).await;

    let (status, _) = send(&pool, Method::POST, &format!("/api/error-logs/{id}/resolve"), Some(&viewer), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = send(&pool, Method::POST, &format!("/api/error-logs/{id}/resolve"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(body["data"]["resolved_at"], Value::Null);
    assert_eq!(body["data"]["user"], Value::Null);
    assert!(is_resolved(&pool, id).await);

    // Buka lagi: `resolved_at` kembali null.
    let (status, body) = send(&pool, Method::POST, &format!("/api/error-logs/{id}/reopen"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["resolved_at"], Value::Null);
    assert!(!is_resolved(&pool, id).await);

    // Membuka baris yang sudah terbuka tidak mengubah `updated_at` (Eloquent tidak menyimpan).
    let before = updated_at(&pool, id).await;
    let (status, _) = send(&pool, Method::POST, &format!("/api/error-logs/{id}/reopen"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_at(&pool, id).await, before);

    let (status, _) = send(&pool, Method::POST, "/api/error-logs/999999999/resolve", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::POST, "/api/error-logs/999999999/reopen", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::POST, "/api/error-logs/abc/resolve", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, &[id], &[ADMIN_STATUS, VIEWER_STATUS]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn bulk_selesai_dan_buka_hanya_baris_yang_berubah() {
    let pool = connect().await;
    let (_, admin) = user_token(&pool, ADMIN_BULK, true).await;
    let x = insert_log(&pool, "uji-err-bulk x", "react", None, "2026-04-01 08:00:00", None).await;
    let y = insert_log(&pool, "uji-err-bulk y", "react", None, "2026-04-02 08:00:00", None).await;
    let z = insert_log(&pool, "uji-err-bulk z", "react", None, "2026-04-03 08:00:00", None).await;
    mark_resolved(&pool, z).await;
    let ids = [x, y, z];

    // `bulk/resolve` harus sampai ke handler bulk, bukan ditangkap `/{errorLog}`.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/error-logs/bulk/resolve",
        Some(&admin),
        Some(json!({ "ids": [x, y, z] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "success": true, "affected": 2 }));
    assert!(is_resolved(&pool, x).await && is_resolved(&pool, y).await && is_resolved(&pool, z).await);

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/error-logs/bulk/reopen",
        Some(&admin),
        Some(json!({ "ids": [x, z] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["affected"], json!(2));
    assert!(!is_resolved(&pool, x).await && !is_resolved(&pool, z).await);
    assert!(is_resolved(&pool, y).await);

    // Validasi: sama dengan aturan Laravel.
    let cases: [(Value, &str); 5] = [
        (json!({}), "The ids field is required."),
        (json!({ "ids": [] }), "The ids field is required."),
        (json!({ "ids": "1" }), "The ids field must be an array."),
        (json!({ "ids": ["abc"] }), "The ids.0 field must be an integer."),
        (json!({ "ids": [999999999] }), "The selected ids.0 is invalid."),
    ];
    for (body, pesan) in cases {
        let (status, res) = send(&pool, Method::POST, "/api/error-logs/bulk/resolve", Some(&admin), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{res}");
        assert_eq!(res["message"], json!(pesan));
    }

    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/error-logs/bulk/reopen",
        Some(&admin),
        Some(json!({ "ids": [999999999] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (_, viewer) = user_token(&pool, VIEWER_BULK, false).await;
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/error-logs/bulk/resolve",
        Some(&viewer),
        Some(json!({ "ids": [x] })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup(&pool, &ids, &[ADMIN_BULK, VIEWER_BULK]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn bulk_hapus_hanya_baris_yang_dikirim() {
    let pool = connect().await;
    let (_, admin) = user_token(&pool, ADMIN_HAPUS, true).await;
    let p = insert_log(&pool, "uji-err-hapus p", "manual", None, "2026-05-01 08:00:00", None).await;
    let q = insert_log(&pool, "uji-err-hapus q", "manual", None, "2026-05-02 08:00:00", None).await;
    let r = insert_log(&pool, "uji-err-hapus r", "manual", None, "2026-05-03 08:00:00", None).await;

    // DELETE /bulk dan POST /bulk/delete memakai handler yang sama.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/error-logs/bulk",
        Some(&admin),
        Some(json!({ "ids": [p, q] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "success": true, "affected": 2 }));
    assert!(!log_exists(&pool, p).await && !log_exists(&pool, q).await);
    assert!(log_exists(&pool, r).await);

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/error-logs/bulk/delete",
        Some(&admin),
        Some(json!({ "ids": [r] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["affected"], json!(1));
    assert!(!log_exists(&pool, r).await);

    let (status, res) = send(
        &pool,
        Method::DELETE,
        "/api/error-logs/bulk",
        Some(&admin),
        Some(json!({ "ids": [p] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{res}");

    cleanup(&pool, &[p, q, r], &[ADMIN_HAPUS]).await;
}
