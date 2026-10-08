//! User lewat router terhadap MySQL: create (bcrypt $2y$, role), validasi, daftar, detail, ubah, hapus, dan audit.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test users_write_db -- --include-ignored
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

const ADMIN: &str = "uji-usr-admin@example.test";
const BARU: &str = "uji-usr-baru@example.test";

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

async fn cleanup(pool: &MySqlPool) {
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM users WHERE email LIKE 'uji-usr-%'")
        .fetch_all(pool)
        .await
        .unwrap();
    for id in &ids {
        sqlx::query("DELETE FROM model_has_roles WHERE model_type = 'App\\\\Models\\\\User' AND model_id = ?").bind(id).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\User' AND auditable_id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM users WHERE email LIKE 'uji-usr-%'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn users_create_update_delete_with_roles_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Admin User', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(&pool, admin, "uji-usr")
        .await
        .unwrap();

    // Create: 201, role tersinkron, password di-hash $2y$ dan cocok, dan tanpa field password di respon.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/users",
        &token,
        Some(json!({ "name": "Uji Baru", "email": BARU, "password": "rahasia-uji", "gender": "female", "roles": ["admin"] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["roles"][0]["name"], "admin", "{body}");
    assert!(
        body.get("password").is_none(),
        "password tidak boleh bocor: {body}"
    );
    let id: u64 = body["id"].as_u64().unwrap();
    let hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(hash.starts_with("$2y$"), "{hash}");
    assert!(bcrypt::verify("rahasia-uji", &hash).unwrap());
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\User' AND auditable_id = ? ORDER BY id").bind(id).fetch_all(&pool).await.unwrap();
    assert_eq!(events, vec!["created"]);
    let audit_new: String = sqlx::query_scalar("SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\User' AND auditable_id = ? AND event = 'created'").bind(id).fetch_one(&pool).await.unwrap();
    assert!(
        !audit_new.contains("password"),
        "audit tidak memuat password: {audit_new}"
    );

    // Validasi: email terpakai, password pendek, gender tidak dikenal, dan role tak ada (500).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/users",
        &token,
        Some(json!({ "name": "X", "email": BARU, "password": "rahasia-uji" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["email"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/users",
        &token,
        Some(json!({ "name": "X", "email": "uji-usr-x@example.test", "password": "123" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["password"].is_array(), "{body}");
    let (status, body) = send(&pool, Method::POST, "/api/users", &token, Some(json!({ "name": "X", "email": "uji-usr-y@example.test", "password": "rahasia", "gender": "tidak" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["gender"].is_array(), "{body}");
    let (status, _) = send(&pool, Method::POST, "/api/users", &token, Some(json!({ "name": "X", "email": "uji-usr-z@example.test", "password": "rahasia", "roles": ["tidak-ada"] }))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // Daftar dengan pencarian dan detail.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/users?search=Uji%20Baru",
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["id"] == json!(id)),
        "{body}"
    );
    assert_eq!(body["meta"]["per_page"], 15, "{body}");
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/users/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["email"], BARU, "{body}");

    // Update: role dikosongkan, gender berubah, password diganti; audit hanya kolom yang berubah.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/users/{id}"),
        &token,
        Some(json!({ "gender": "other", "roles": [], "password": "baru-uji-123" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["roles"], json!([]), "{body}");
    let hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(bcrypt::verify("baru-uji-123", &hash).unwrap());
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\User' AND auditable_id = ? ORDER BY id").bind(id).fetch_all(&pool).await.unwrap();
    assert_eq!(events, vec!["created", "updated"]);

    // Hapus: pesan, audit deleted, dan 404 setelahnya.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/users/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "User deleted");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/users/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\User' AND auditable_id = ? ORDER BY id").bind(id).fetch_all(&pool).await.unwrap();
    assert_eq!(events, vec!["created", "updated", "deleted"]);

    cleanup(&pool).await;
}
