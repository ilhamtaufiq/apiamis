//! Tulis kegiatan lewat router terhadap MySQL: create, update, delete, audit, dan daftar per tahun.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kegiatan_write_db -- --include-ignored
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

const ADMIN: &str = "uji-keg-admin@example.test";
const NAMA: &str = "UJI-KEG Program";
const TAHUN: &str = "2099";

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

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kegiatan', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())").execute(pool).await.unwrap();
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

async fn cleanup(pool: &MySqlPool) {
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM tbl_kegiatan WHERE nama_program = ?")
        .bind(NAMA)
        .fetch_all(pool)
        .await
        .unwrap();
    for id in ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kegiatan' AND auditable_id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_program = ?")
        .bind(NAMA)
        .execute(pool)
        .await
        .unwrap();
}

async fn audit_events(pool: &MySqlPool, id: u64) -> Vec<String> {
    sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kegiatan' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kegiatan_create_update_delete_with_audit_and_year_list() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, admin, "uji-keg")
        .await
        .unwrap();
    cleanup(&pool).await;

    // Create: pagu desimal dua digit, kode rekening JSON, audit created.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan",
        &token,
        Some(json!({
            "nama_program": NAMA,
            "tahun_anggaran": TAHUN,
            "sumber_dana": "APBD",
            "pagu": 1500000.5,
            "kode_rekening": ["5.2.02", "5.2.02.01"],
            "sipd_id_sub_bl": 7,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pagu"], "1500000.50", "{body}");
    assert_eq!(
        body["data"]["kode_rekening"],
        json!(["5.2.02", "5.2.02.01"]),
        "{body}"
    );
    let id: u64 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Validasi: sumber dana di luar daftar, pagu negatif, kode rekening bukan array.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan",
        &token,
        Some(json!({ "sumber_dana": "LAINNYA" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["sumber_dana"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan",
        &token,
        Some(json!({ "pagu": -1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["pagu"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan",
        &token,
        Some(json!({ "kode_rekening": "5.2" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["kode_rekening"].is_array(), "{body}");

    // Update tanpa perubahan nilai: tidak ada audit baru (pagu 1500000.5 sama dengan 1500000.50).
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kegiatan/{id}"),
        &token,
        Some(json!({ "pagu": 1500000.5 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Update: pagu dan nama sub kegiatan berubah; kunci lain tidak ikut tercatat.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kegiatan/{id}"),
        &token,
        Some(json!({ "pagu": 2000000, "nama_sub_kegiatan": "Sub uji", "nama_pptk": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pagu"], "2000000.00", "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);
    let new_values: String = sqlx::query_scalar("SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kegiatan' AND auditable_id = ? AND event = 'updated'")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let new_values: Value = serde_json::from_str(&new_values).unwrap();
    assert!(
        new_values.get("pagu").is_some() && new_values.get("nama_sub_kegiatan").is_some(),
        "{new_values}"
    );
    assert!(new_values.get("tahun_anggaran").is_none(), "{new_values}");

    // Daftar per tahun: paginator dengan kegiatan uji.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kegiatan/tahun/{TAHUN}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["per_page"], 15, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["id"] == json!(id)),
        "{body}"
    );

    // Delete: baris hilang, audit deleted.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kegiatan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kegiatan deleted successfully");
    assert_eq!(
        audit_events(&pool, id).await,
        vec!["created", "updated", "deleted"]
    );
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/kegiatan/{id}"),
        &token,
        Some(json!({ "pagu": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
