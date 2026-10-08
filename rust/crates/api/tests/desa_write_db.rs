//! Tulis desa lewat router terhadap MySQL: create, update, delete, audit, dan notifikasi admin.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test desa_write_db -- --include-ignored
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

const ADMIN_A: &str = "uji-desa-admin-a@example.test";
const ADMIN_B: &str = "uji-desa-admin-b@example.test";
const KEC: &str = "UJI-DESA Kecamatan";

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

async fn make_admin(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Desa', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
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
    uid
}

async fn cleanup(pool: &MySqlPool, admin_b: u64) {
    let desa: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM tbl_desa WHERE n_desa LIKE 'UJI-DESA%'")
            .fetch_all(pool)
            .await
            .unwrap();
    for id in desa {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Desa' AND auditable_id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa LIKE 'UJI-DESA%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(KEC)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM notifications WHERE notifiable_id = ?")
        .bind(admin_b)
        .execute(pool)
        .await
        .unwrap();
}

async fn audit_events(pool: &MySqlPool, id: u64) -> Vec<String> {
    sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Desa' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn desa_create_update_delete_with_audit_and_notifications() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin_a = make_admin(&pool, ADMIN_A).await;
    let admin_b = make_admin(&pool, ADMIN_B).await;
    let token = auth::login::create_token(&pool, admin_a, "uji-desa")
        .await
        .unwrap();
    cleanup(&pool, admin_b).await;

    sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(KEC)
    .execute(&pool)
    .await
    .unwrap();
    let kec: u64 = sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(KEC)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Create: respon memuat kecamatan, audit created, dan notifikasi tanpa tautan.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/desa",
        &token,
        Some(json!({ "nama_desa": "UJI-DESA Satu", "luas": "12.5", "jumlah_penduduk": 1500, "jumlah_kk": 400, "kecamatan_id": kec })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_desa"], "UJI-DESA Satu", "{body}");
    assert_eq!(body["data"]["kecamatan"]["nama_kecamatan"], KEC, "{body}");
    assert_eq!(body["data"]["jumlah_kk"], 400, "{body}");
    let id: u64 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);
    let data: String = sqlx::query_scalar("SELECT CAST(data AS CHAR) FROM notifications WHERE notifiable_id = ? ORDER BY created_at DESC, id DESC LIMIT 1")
        .bind(admin_b)
        .fetch_one(&pool)
        .await
        .unwrap();
    let n: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(n["title"], "Data Desa dibuat", "{n}");
    assert!(n["url"].is_null(), "desa tidak punya tautan: {n}");

    // Validasi: nama wajib, kecamatan harus ada, jumlah penduduk harus integer.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/desa",
        &token,
        Some(json!({ "luas": 1, "jumlah_penduduk": 1, "kecamatan_id": kec })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["nama_desa"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/desa",
        &token,
        Some(json!({ "nama_desa": "UJI-DESA X", "luas": 1, "jumlah_penduduk": 1, "kecamatan_id": 99999999 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["kecamatan_id"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/desa",
        &token,
        Some(json!({ "nama_desa": "UJI-DESA Y", "luas": 1, "jumlah_penduduk": 1.5, "kecamatan_id": kec })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["jumlah_penduduk"].is_array(), "{body}");

    // Update tanpa perubahan: tidak ada audit. nama_desa null diabaikan.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/desa/{id}"),
        &token,
        Some(json!({ "nama_desa": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_desa"], "UJI-DESA Satu", "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Update: luas dan jumlah penduduk berubah, audit hanya memuat kolom yang berubah.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/desa/{id}"),
        &token,
        Some(json!({ "luas": 20, "jumlah_penduduk": 1700 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["jumlah_penduduk"], 1700, "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);
    let new_values: String = sqlx::query_scalar("SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Desa' AND auditable_id = ? AND event = 'updated'")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let new_values: Value = serde_json::from_str(&new_values).unwrap();
    assert!(new_values.get("jumlah_penduduk").is_some(), "{new_values}");
    assert!(
        new_values.get("nama_desa").is_none() && new_values.get("n_desa").is_none(),
        "nama tidak berubah: {new_values}"
    );

    // Daftar per kecamatan: tanpa relasi kecamatan.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/desa/kecamatan/{kec}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = body["data"].as_array().unwrap();
    assert!(list.iter().any(|d| d["id"] == json!(id)), "{body}");
    assert!(
        list.iter().all(|d| d.get("kecamatan").is_none()),
        "tanpa relasi kecamatan: {body}"
    );

    // Delete: baris hilang, audit deleted.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/desa/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Desa deleted successfully");
    assert_eq!(
        audit_events(&pool, id).await,
        vec!["created", "updated", "deleted"]
    );
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/desa/{id}"),
        &token,
        Some(json!({ "luas": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, admin_b).await;
}
