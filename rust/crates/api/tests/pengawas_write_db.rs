//! Pengawas lewat router terhadap MySQL: daftar, detail, buat, ubah, hapus, statistik, dan audit.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pengawas_write_db -- --include-ignored
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

const ADMIN: &str = "uji-pgw-admin@example.test";
const NAMA: &str = "UJI-PGW Pengawas";

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
async fn pengawas_crud_statistics_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Pengawas', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    let admin: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())").execute(&pool).await.unwrap();
    let role: u64 = sqlx::query_scalar(
        "SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, admin, "uji-pgw")
        .await
        .unwrap();
    sqlx::query("DELETE FROM pengawas WHERE nama = ?")
        .bind(NAMA)
        .execute(&pool)
        .await
        .unwrap();

    // Create: respon resource 200 dengan jumlah lokasi 0, audit created.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pengawas",
        &token,
        Some(json!({ "nama": NAMA, "nip": "1234", "telepon": "0812" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama"], NAMA, "{body}");
    assert_eq!(body["data"]["jumlah_lokasi"], 0, "{body}");
    assert!(body["data"]["jabatan"].is_null(), "{body}");
    let id: u64 = body["data"]["id"].as_u64().unwrap();
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pengawas",
        &token,
        Some(json!({ "nip": "1" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["nama"].is_array(), "{body}");

    // Show dan daftar memuat pengawas ini.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/pengawas/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], json!(id), "{body}");
    let (status, body) = send(&pool, Method::GET, "/api/pengawas", &token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == json!(id)),
        "{body}"
    );

    // Update: jabatan terisi, audit updated hanya memuat kolom yang berubah.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/pengawas/{id}"),
        &token,
        Some(json!({ "jabatan": "Staf teknis" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["jabatan"], "Staf teknis", "{body}");
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pengawas' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["created", "updated"]);

    // Statistik: pengawas bertambah; lokasi dan pagu dari pekerjaan yang punya pengawas atau pendamping.
    let (status, body) = send(&pool, Method::GET, "/api/pengawas/statistics", &token, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]["total_pengawas"].as_i64().unwrap() >= 1,
        "{body}"
    );
    assert!(
        body["data"]["total_lokasi"].is_number() && body["data"]["total_pagu"].is_number(),
        "{body}"
    );

    // Delete: pesan, audit deleted, dan 404 setelahnya.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/pengawas/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Pengawas deleted successfully");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/pengawas/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pengawas' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["created", "updated", "deleted"]);

    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pengawas' AND auditable_id = ?").bind(id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM pengawas WHERE nama = ?")
        .bind(NAMA)
        .execute(&pool)
        .await
        .unwrap();
}
