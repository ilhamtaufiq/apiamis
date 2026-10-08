//! Tulis tag lewat router terhadap MySQL: create, update, delete, dan audit-nya.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test tags_write_db -- --ignored
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

const ACTOR: &str = "uji-tags-actor@example.test";

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
    let payload = match body {
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(payload).unwrap())
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Tags', ?, 'x', NOW(), NOW())")
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

async fn audit_events(pool: &MySqlPool, tag_id: u64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Tag' AND auditable_id = ? ORDER BY id",
    )
    .bind(tag_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tag_create_update_delete_write_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-tags")
        .await
        .unwrap();

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("Uji Tag {stamp}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/tags",
        &token,
        Some(json!({ "name": name, "color": "#1a2B3c" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_u64().unwrap();
    assert_eq!(body["data"]["slug"], format!("uji-tag-{stamp}").as_str());
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Nama duplikat ditolak
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/tags",
        &token,
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["name"].is_array());

    let renamed = format!("Uji Ubah {stamp}");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/tags/{id}"),
        &token,
        Some(json!({ "name": renamed })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], renamed.as_str());
    assert_eq!(body["data"]["slug"], format!("uji-ubah-{stamp}").as_str());
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/tags/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Tag deleted successfully");
    assert_eq!(
        audit_events(&pool, id).await,
        vec!["created", "updated", "deleted"]
    );

    let remaining: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_tags WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(remaining, 0);
}
