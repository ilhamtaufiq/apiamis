//! Tambah, ubah, dan hapus tipe dokumen lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test document_types_db -- --include-ignored
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

const ADMIN: &str = "uji-dtype-admin@example.test";
const CODE_A: &str = "UJI-DTYPE-A";
const CODE_B: &str = "UJI-DTYPE-B";
const REG_NOMOR: &str = "UJI-DTYPE-REG-1";

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

/// User admin uji dengan token Sanctum.
async fn admin_token(pool: &MySqlPool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Dtype', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(pool)
        .await
        .unwrap();
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
    auth::login::create_token(pool, uid, "uji-dtype")
        .await
        .unwrap()
}

/// Bersihkan data uji: register penanda, lalu tipe uji.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_document_registers WHERE nomor = ?")
        .bind(REG_NOMOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_document_types WHERE code IN (?, ?)")
        .bind(CODE_A)
        .bind(CODE_B)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_types_store_update_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let token = admin_token(&pool).await;

    // Validasi: name dan code wajib.
    let (status, body) = send(&pool, Method::POST, "/api/document-types", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");

    // Simpan tanpa format_template: 201, kunci format_template tidak ikut (seperti Eloquent).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/document-types",
        Some(&token),
        Some(json!({ "name": "Uji A", "code": CODE_A })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["code"], CODE_A);
    assert!(body.get("format_template").is_none(), "{body}");
    let id_a = body["id"].as_i64().unwrap();

    // Kode sama: ditolak.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/document-types",
        Some(&token),
        Some(json!({ "name": "Duplikat", "code": CODE_A })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The code has already been taken.", "{body}");

    // Ubah: kode sendiri tidak dianggap duplikat, format_template diisi.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/document-types/{id_a}"),
        Some(&token),
        Some(json!({ "name": "Uji A revisi", "code": CODE_A, "format_template": "{sequence}/{code}" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "Uji A revisi");
    assert_eq!(body["format_template"], "{sequence}/{code}");

    // Kode dipakai tipe lain: ditolak.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/document-types",
        Some(&token),
        Some(json!({ "name": "Uji B", "code": CODE_B })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id_b = body["id"].as_i64().unwrap();
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/document-types/{id_b}"),
        Some(&token),
        Some(json!({ "name": "Uji B", "code": CODE_A })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The code has already been taken.", "{body}");

    // Tipe yang masih dipakai register: hapus ditolak 422.
    sqlx::query(
        "INSERT INTO tbl_document_registers (kontrak_id, type_id, nomor, tanggal, sequence_number, year, created_at, updated_at) \
         VALUES (1, ?, ?, '2026-01-01', 9001, 2026, NOW(), NOW())",
    )
    .bind(id_a)
    .bind(REG_NOMOR)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/document-types/{id_a}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Tidak dapat menghapus tipe yang sudah memiliki data register",
        "{body}"
    );

    // Tipe tanpa register: hapus 200, lalu tidak ada di daftar.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/document-types/{id_b}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Tipe berhasil dihapus" }));
    let (status, body) = send(&pool, Method::GET, "/api/document-types", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array().unwrap().iter().all(|t| t["id"] != json!(id_b)),
        "{body}"
    );

    // Id tidak ada: 404.
    let (status, _) = send(
        &pool,
        Method::PUT,
        "/api/document-types/999999999",
        Some(&token),
        Some(json!({ "name": "x", "code": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
