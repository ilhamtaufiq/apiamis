//! Master fase pekerjaan lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test master_fase_db -- --include-ignored
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

const ADMIN: &str = "uji-fase-admin@example.test";
const JENIS: &str = "UJI_FASE";

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

/// Buat user admin uji dan kembalikan token Sanctum-nya.
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Fase', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-fase")
        .await
        .unwrap()
}

/// Hapus baris uji saja (jenis proyek penanda), supaya tes paralel tidak saling menghapus.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM master_fase_pekerjaans WHERE jenis_proyek = ?")
        .bind(JENIS)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn master_fase_crud_validates_and_responds_like_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let token = admin_token(&pool).await;

    // Tanpa token: 401.
    let (status, _) = send(&pool, Method::GET, "/api/master-fase-pekerjaan", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Validasi: field wajib, lalu tipe prioritas.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/master-fase-pekerjaan",
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The jenis proyek field is required.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/master-fase-pekerjaan",
        Some(&token),
        Some(json!({
            "jenis_proyek": JENIS, "kode_fase": "A1", "nama_fase": "Fase A",
            "prioritas": "x"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The prioritas field must be an integer.", "{body}");

    // Simpan: 201, durasi_faktor tetap 1.1, keywords array.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/master-fase-pekerjaan",
        Some(&token),
        Some(json!({
            "jenis_proyek": JENIS, "kode_fase": "A1", "nama_fase": "Fase A",
            "prioritas": 1, "overlap_persen": 10, "durasi_faktor": 1.1,
            "keywords": ["survei", "desain"], "deskripsi": "Uji", "is_active": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let data = &body["data"];
    assert_eq!(body["success"], true);
    assert_eq!(data["durasi_faktor"], json!(1.1));
    assert_eq!(data["keywords"], json!(["survei", "desain"]));
    assert_eq!(data["is_active"], true);
    let id = data["id"].as_i64().unwrap();

    // Kode sama pada jenis sama: ditolak (unique).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/master-fase-pekerjaan",
        Some(&token),
        Some(json!({
            "jenis_proyek": JENIS, "kode_fase": "A1", "nama_fase": "Duplikat", "prioritas": 2
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The kode fase has already been taken.", "{body}");

    // Baca satu dan daftar dengan filter jenis.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/master-fase-pekerjaan/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["kode_fase"], "A1");

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/master-fase-pekerjaan?jenis_proyek={JENIS}&is_active=0"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 0, "{body}");

    // PATCH parsial: hanya nama berubah, field lain tetap.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/master-fase-pekerjaan/{id}"),
        Some(&token),
        Some(json!({ "nama_fase": "Fase A revisi" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_fase"], "Fase A revisi");
    assert_eq!(body["data"]["durasi_faktor"], json!(1.1));
    assert_eq!(body["data"]["keywords"], json!(["survei", "desain"]));

    // PUT dengan keywords kosong dan is_active false.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/master-fase-pekerjaan/{id}"),
        Some(&token),
        Some(json!({ "keywords": [], "is_active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["keywords"], json!([]));
    assert_eq!(body["data"]["is_active"], false);

    // Hapus, lalu baca lagi: 404.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/master-fase-pekerjaan/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Data deleted successfully");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/master-fase-pekerjaan/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
