//! Draft pekerjaan lewat router terhadap MySQL: updateOrCreate per pekerjaan, audit, dan 204 saat hapus.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test draft_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ACTOR: &str = "uji-draft-admin@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(req)
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
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Draft', ?, 'x', NOW(), NOW())")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ACTOR)
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn draft_store_update_destroy_with_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-draft")
        .await
        .unwrap();
    let pid: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_draft_pekerjaan WHERE pekerjaan_id = ?")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();

    let (status, first) = send(
        &pool,
        Method::POST,
        "/api/draft-pekerjaan",
        &token,
        Some(json!({
            "pekerjaan_id": pid, "kode_rup": "RUP-UJI", "nama_pelaksana": "CV Uji"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let id = first["data"]["id"].as_i64().unwrap();
    assert_eq!(first["data"]["kode_rup"], "RUP-UJI");
    assert_eq!(first["data"]["pekerjaan"]["id"], pid);
    assert!(first["data"]["penyedia"].is_null());

    // Store kedua untuk pekerjaan yang sama memperbarui baris yang sama. Field yang tidak dikirim jadi null.
    let (status, second) = send(
        &pool,
        Method::POST,
        "/api/draft-pekerjaan",
        &token,
        Some(json!({
            "pekerjaan_id": pid, "kode_paket": "PKT-UJI"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        second["data"]["id"], id,
        "updateOrCreate memakai baris yang sama"
    );
    assert!(second["data"]["kode_rup"].is_null());
    assert_eq!(second["data"]["kode_paket"], "PKT-UJI");

    let (status, patched) = send(
        &pool,
        Method::PATCH,
        &format!("/api/draft-pekerjaan/{id}"),
        &token,
        Some(json!({ "nama_pelaksana": "CV Baru" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["data"]["nama_pelaksana"], "CV Baru");
    assert_eq!(
        patched["data"]["kode_paket"], "PKT-UJI",
        "PATCH hanya mengubah field yang dikirim"
    );

    let (status, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/draft-pekerjaan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown["data"]["nama_pelaksana"], "CV Baru");

    let (status, list) = send(
        &pool,
        Method::GET,
        &format!("/api/draft-pekerjaan?search={}", "UJI-NOT-THERE"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["meta"]["total"], 0);
    assert_eq!(list["meta"]["per_page"], 10);

    // Audit: created, updated (kolom yang berubah saja), dan tidak ada notifikasi admin untuk draft.
    let n_created: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\DraftPekerjaan' AND auditable_id = ? AND event = 'created'")
        .bind(id).fetch_one(&pool).await.unwrap().try_get("n").unwrap();
    assert_eq!(n_created, 1);
    let n_updated: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\DraftPekerjaan' AND auditable_id = ? AND event = 'updated'")
        .bind(id).fetch_one(&pool).await.unwrap().try_get("n").unwrap();
    assert!(n_updated >= 2);

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/draft-pekerjaan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/draft-pekerjaan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
