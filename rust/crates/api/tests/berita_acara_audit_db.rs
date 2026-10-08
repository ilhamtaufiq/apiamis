//! Nomor urut berita acara dan log audit admin lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test berita_acara_audit_db -- --include-ignored
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

const ADMIN: &str = "uji-audit-admin@example.test";
const BERITA_USER: &str = "uji-berita-user@example.test";
const AUDIT_USER: &str = "uji-audit-user@example.test";
const AUDIT_EVENT: &str = "uji-updated";
const KONTRAK_TYPE: &str = "App\\Models\\Kontrak";
const YEAR: i64 = 2099;

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

/// Buat user uji. Bila `admin`, diberi role `admin` (guard `web`).
async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)",
    )
    .bind(email)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Audit', ?, 'x', NOW(), NOW())")
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
    uid
}

/// Hapus sisa tes penomoran (tahun uji saja).
async fn cleanup_sequence(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(YEAR)
        .execute(pool)
        .await
        .unwrap();
}

/// Hapus log audit uji. Hanya event penanda tes ini, supaya tes paralel tidak saling menghapus.
async fn cleanup_audit(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_audit_logs WHERE event = ?")
        .bind(AUDIT_EVENT)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn berita_acara_sequence_validates_and_upserts() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup_sequence(&pool).await;
    let uid = make_user(&pool, BERITA_USER, false).await;
    let token = auth::login::create_token(&pool, uid, "uji-berita")
        .await
        .unwrap();

    // Tanpa token: 401.
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/berita-acara/sequence?year={YEAR}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Tahun di bawah 2020 ditolak dengan pesan pertama seperti Laravel.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/berita-acara/sequence?year=1999",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"], "The year field must be at least 2020.",
        "{body}"
    );

    // Belum ada baris: last_number 0.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/berita-acara/sequence?year={YEAR}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "year": YEAR, "last_number": 0 }));

    // Simpan lalu baca lagi; simpan kedua kali memperbarui baris yang sama.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/berita-acara/sequence",
        Some(&token),
        Some(json!({ "year": YEAR, "last_number": "7" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "year": YEAR, "last_number": 7 }));
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/berita-acara/sequence",
        Some(&token),
        Some(json!({ "year": YEAR, "last_number": 9 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/berita-acara/sequence?year={YEAR}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(body["last_number"], json!(9), "{body}");
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'",
    )
    .bind(YEAR)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);

    // last_number negatif ditolak.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/berita-acara/sequence",
        Some(&token),
        Some(json!({ "year": YEAR, "last_number": -1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"], "The last number field must be at least 0.",
        "{body}"
    );

    cleanup_sequence(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn audit_logs_are_admin_only_and_resolve_pekerjaan_context() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup_audit(&pool).await;

    let admin = make_user(&pool, ADMIN, true).await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-audit-admin")
        .await
        .unwrap();
    let user = make_user(&pool, AUDIT_USER, false).await;
    let user_token = auth::login::create_token(&pool, user, "uji-audit-user")
        .await
        .unwrap();

    // Pekerjaan pertama dipakai sebagai konteks, dengan nama dari tabel pekerjaan.
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let nama: Option<String> =
        sqlx::query_scalar("SELECT nama_paket FROM tbl_pekerjaan WHERE id = ?")
            .bind(pekerjaan)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Log untuk Kontrak (konteks dari `id_pekerjaan` di new_values) dan untuk Output tanpa pekerjaan.
    sqlx::query("INSERT INTO tbl_audit_logs (user_id, event, auditable_type, auditable_id, new_values, old_values, url, ip_address, created_at, updated_at) VALUES (?, ?, ?, 1, ?, NULL, '/uji', '127.0.0.1', NOW(), NOW())")
        .bind(admin)
        .bind(AUDIT_EVENT)
        .bind(KONTRAK_TYPE)
        .bind(json!({ "id_pekerjaan": pekerjaan }).to_string())
        .execute(&pool)
        .await
        .unwrap();
    let log_id: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_audit_logs WHERE event = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(AUDIT_EVENT)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Non-admin: 403.
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/audit-logs",
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admin: daftar dengan meta, filter `type` mengandung teks.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/audit-logs?type=Kontrak&event=uji-updated",
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], json!(true));
    assert_eq!(body["meta"]["current_page"], json!(1));
    assert_eq!(body["meta"]["total"], json!(1), "{body}");
    let item = &body["data"][0];
    assert_eq!(item["id"], json!(log_id));
    assert_eq!(item["user"]["id"], json!(admin));
    assert_eq!(item["old_values"], Value::Null);
    assert_eq!(item["pekerjaan"]["id"], json!(pekerjaan));
    assert_eq!(item["pekerjaan"]["tab"], json!("kontrak"));
    assert_eq!(item["auditable_type"], json!(KONTRAK_TYPE));
    assert_eq!(item["pekerjaan"]["nama_paket"], json!(nama));

    // Detail satu log.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/audit-logs/{log_id}"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["event"], json!(AUDIT_EVENT));

    // Id tidak ada: 404.
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/audit-logs/999999999",
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup_audit(&pool).await;
}
