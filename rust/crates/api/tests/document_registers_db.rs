//! Register dokumen lewat router terhadap MySQL: nomor urut, validasi, dan hapus.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test document_registers_db -- --include-ignored
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

const ADMIN: &str = "uji-dreg-admin@example.test";
const CODE: &str = "UJI-DREG";
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
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn admin_token(pool: &MySqlPool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(ADMIN).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji DReg', ?, 'x', NOW(), NOW())")
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
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    auth::login::create_token(pool, uid, "uji-dreg").await.unwrap()
}

/// Bersihkan register, sequence, dan tipe uji. Tahun uji dipakai agar tidak menyentuh data lain.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_document_registers WHERE year = ?").bind(YEAR).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(YEAR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_document_types WHERE code = ?").bind(CODE).execute(pool).await.unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_registers_numbering_and_crud() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let token = admin_token(&pool).await;

    sqlx::query("INSERT INTO tbl_document_types (name, code, format_template, created_at, updated_at) VALUES ('Uji', ?, '{sequence}/{code}-UJI/{month}/{year}', NOW(), NOW())")
        .bind(CODE)
        .execute(&pool)
        .await
        .unwrap();
    let type_id: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_document_types WHERE code = ?")
        .bind(CODE)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_kontrak () VALUES ()").execute(&pool).await.unwrap();
    let kontrak: i64 = sqlx::query_scalar("SELECT CAST(MAX(id) AS SIGNED) FROM tbl_kontrak")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Validasi: kontrak wajib.
    let (status, body) = send(&pool, Method::POST, "/api/document-registers", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The kontrak id field is required.", "{body}");

    // Nomor otomatis dari urutan tahun; bulan romawi dan template tipe.
    let reg = |extra: Value| {
        let mut base = json!({ "kontrak_id": kontrak, "type_id": type_id, "tanggal": "2099-03-05", "nilai": 1500.5 });
        base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        base
    };
    let (status, body) = send(&pool, Method::POST, "/api/document-registers", Some(&token), Some(reg(json!({})))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["nomor"], format!("001/{CODE}-UJI/III/{YEAR}"), "{body}");
    assert_eq!(body["sequence_number"], 1);
    assert_eq!(body["year"], YEAR);
    assert_eq!(body["nilai"], 1500.5);
    assert_eq!(body["type"]["code"], CODE);
    let first = body["id"].as_i64().unwrap();

    let (status, body) = send(&pool, Method::POST, "/api/document-registers", Some(&token), Some(reg(json!({})))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["sequence_number"], 2);
    let second = body["id"].as_i64().unwrap();

    // Sequence yang sudah dipakai: 422 dengan pesan Laravel.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/document-registers",
        Some(&token),
        Some(reg(json!({ "sequence_number": 1 }))),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], format!("Sequence nomor 1 untuk tahun {YEAR} sudah digunakan."), "{body}");

    // Nomor manual yang sudah ada: 422. Transaksi dibatalkan, urutan tidak naik.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/document-registers",
        Some(&token),
        Some(reg(json!({ "nomor": format!("001/{CODE}-UJI/III/{YEAR}") }))),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], format!("Nomor dokumen 001/{CODE}-UJI/III/{YEAR} sudah terdaftar."), "{body}");
    let last: i64 = sqlx::query_scalar("SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(YEAR)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(last, 2);

    // Daftar: filter tahun dan tipe, bentuk paginator, relasi kontrak dan tipe.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/document-registers?tahun={YEAR}&type_id={type_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 2, "{body}");
    assert_eq!(body["per_page"], 20);
    assert_eq!(body["data"][0]["kontrak"]["id"], kontrak);
    assert_eq!(body["data"][0]["type"]["id"], type_id);

    // Ubah: nomor yang dipakai register lain ditolak; nomor baru diterima.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/document-registers/{first}"),
        Some(&token),
        Some(json!({ "tanggal": "2099-03-05", "nomor": format!("002/{CODE}-UJI/III/{YEAR}") })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Nomor dokumen sudah digunakan oleh registrasi lain.", "{body}");
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/document-registers/{first}"),
        Some(&token),
        Some(json!({ "tanggal": "2099-03-05", "nomor": "UJI-BARU" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["nomor"], "UJI-BARU");

    // Hapus register kedua: urutan tahun disetel ke maksimum sisa (1).
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/document-registers/{second}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Register deleted" }));
    let last: i64 = sqlx::query_scalar("SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(YEAR)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(last, 1);

    let (status, _) = send(&pool, Method::DELETE, &format!("/api/document-registers/{second}"), Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
    sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?").bind(kontrak).execute(&pool).await.unwrap();
}
