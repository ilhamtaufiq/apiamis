//! Tulis kecamatan lewat router terhadap MySQL: create, update, delete, audit, dan notifikasi admin.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kecamatan_write_db -- --include-ignored
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

const ADMIN_A: &str = "uji-kec-admin-a@example.test";
const ADMIN_B: &str = "uji-kec-admin-b@example.test";
const NAMA_A: &str = "UJI-KEC Alpha";
const NAMA_B: &str = "UJI-KEC Beta";

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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kecamatan', ?, 'x', NOW(), NOW())")
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
    let ids: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec LIKE 'UJI-KEC%'")
            .fetch_all(pool)
            .await
            .unwrap();
    for id in ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kecamatan' AND auditable_id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE 'UJI-KEC%'")
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
    sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kecamatan' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn notif_titles(pool: &MySqlPool, admin_b: u64) -> Vec<String> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT CAST(data AS CHAR) FROM notifications WHERE notifiable_id = ? ORDER BY created_at, id")
        .bind(admin_b)
        .fetch_all(pool)
        .await
        .unwrap();
    // Urutan `created_at` hanya sampai detik; daftar dibandingkan setelah diurutkan.
    let mut titles: Vec<String> = rows
        .iter()
        .map(|d| {
            serde_json::from_str::<Value>(d).unwrap()["title"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    titles.sort();
    titles
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kecamatan_create_update_delete_with_audit_and_notifications() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin_a = make_admin(&pool, ADMIN_A).await;
    let admin_b = make_admin(&pool, ADMIN_B).await;
    let token_a = auth::login::create_token(&pool, admin_a, "uji-kec")
        .await
        .unwrap();
    cleanup(&pool, admin_b).await;

    // Create: respon 200 dengan jumlah_desa 0, audit created, dan notifikasi ke admin lain.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kecamatan",
        &token_a,
        Some(json!({ "n_kec": NAMA_A })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_kecamatan"], NAMA_A, "{body}");
    assert_eq!(body["data"]["jumlah_desa"], 0, "{body}");
    let id: u64 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);
    let data: String = sqlx::query_scalar("SELECT CAST(data AS CHAR) FROM notifications WHERE notifiable_id = ? ORDER BY created_at DESC, id DESC LIMIT 1")
        .bind(admin_b)
        .fetch_one(&pool)
        .await
        .unwrap();
    let link = serde_json::from_str::<Value>(&data).unwrap()["url"].clone();
    assert_eq!(link, json!(format!("/kecamatan/{id}/edit")));
    assert_eq!(
        notif_titles(&pool, admin_b).await,
        vec!["Data Kecamatan dibuat"]
    );

    // Nama duplikat dan nama kosong ditolak dengan 422.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kecamatan",
        &token_a,
        Some(json!({ "n_kec": NAMA_A })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["n_kec"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kecamatan",
        &token_a,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["n_kec"].is_array(), "{body}");

    // Update dengan nama yang sama: tidak ada audit baru.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kecamatan/{id}"),
        &token_a,
        Some(json!({ "n_kec": NAMA_A })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Update dengan nama baru: audit updated hanya memuat kolom yang berubah.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kecamatan/{id}"),
        &token_a,
        Some(json!({ "n_kec": NAMA_B })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_kecamatan"], NAMA_B, "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);
    let new_values: String = sqlx::query_scalar("SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kecamatan' AND auditable_id = ? AND event = 'updated'")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let new_values: Value = serde_json::from_str(&new_values).unwrap();
    assert_eq!(new_values["n_kec"], NAMA_B);
    assert!(
        new_values.get("created_at").is_none(),
        "hanya kolom yang berubah: {new_values}"
    );

    // Nama yang dipakai kecamatan lain ditolak; update ke id tak dikenal 404.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kecamatan",
        &token_a,
        Some(json!({ "n_kec": "UJI-KEC Gamma" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kecamatan/{id}"),
        &token_a,
        Some(json!({ "n_kec": "UJI-KEC Gamma" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = send(
        &pool,
        Method::PUT,
        "/api/kecamatan/99999999",
        &token_a,
        Some(json!({ "n_kec": "UJI-KEC X" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Delete: baris hilang, audit deleted, dan pesan hapus ke admin lain.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kecamatan/{id}"),
        &token_a,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kecamatan deleted successfully");
    assert_eq!(
        audit_events(&pool, id).await,
        vec!["created", "updated", "deleted"]
    );
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/kecamatan/{id}"),
        &token_a,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        notif_titles(&pool, admin_b).await,
        vec![
            "Data Kecamatan dibuat",
            "Data Kecamatan dibuat",
            "Data Kecamatan dihapus",
            "Data Kecamatan diperbarui",
        ]
    );

    cleanup(&pool, admin_b).await;
}
