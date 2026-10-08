//! Pemetaan kegiatan-role lewat router terhadap MySQL: daftar, buat (unik per role), hapus, dan audit.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kegiatan_role_write_db -- --include-ignored
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

const ADMIN: &str = "uji-krole-admin@example.test";
const NAMA_KEG: &str = "UJI-KROLE Kegiatan";

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
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
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
async fn kegiatan_role_index_store_destroy_with_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji KRole', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    let admin: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let admin_role: u64 = sqlx::query_scalar(
        "SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(admin_role)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, admin, "uji-krole")
        .await
        .unwrap();

    // Role uji dibuat bila belum ada, dan kegiatan uji untuk pemetaan.
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('uji-krole', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar(
        "SELECT id FROM roles WHERE name = 'uji-krole' AND guard_name = 'web' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM kegiatan_role WHERE role_id = ?")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_program = ?")
        .bind(NAMA_KEG)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_kegiatan (nama_program, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES (?, '2099', 'APBD', 1000, NOW(), NOW())")
        .bind(NAMA_KEG)
        .execute(&pool)
        .await
        .unwrap();
    let kegiatan: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_kegiatan WHERE nama_program = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(NAMA_KEG)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Store: 201, memuat role dan kegiatan, audit created.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token,
        Some(json!({ "role_id": role, "kegiatan_id": kegiatan })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["role"]["name"], "uji-krole", "{body}");
    assert_eq!(body["kegiatan"]["nama_program"], NAMA_KEG, "{body}");
    assert_eq!(body["kegiatan"]["pagu"], "1000.00", "{body}");
    let id: u64 = body["id"].as_u64().unwrap();
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KegiatanRole' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["created"]);

    // Pasangan role dan kegiatan yang sama ditolak; kegiatan tak dikenal juga.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token,
        Some(json!({ "role_id": role, "kegiatan_id": kegiatan })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["kegiatan_id"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token,
        Some(json!({ "role_id": role, "kegiatan_id": 99999999 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Daftar: paginasi 20 memuat pemetaan ini dengan relasi.
    let (status, body) = send(&pool, Method::GET, "/api/kegiatan-role", &token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["per_page"], 20, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == json!(id) && r["role"]["id"] == json!(role)),
        "{body}"
    );

    // Destroy: pesan, baris hilang, audit deleted.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kegiatan-role/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kegiatan-role mapping deleted");
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KegiatanRole' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["created", "deleted"]);

    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KegiatanRole' AND auditable_id = ?").bind(id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_kegiatan WHERE id = ?")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM roles WHERE id = ?")
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();
}
