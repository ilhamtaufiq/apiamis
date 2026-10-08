//! Tulis item checklist lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test checklist_items_db -- --include-ignored
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

const USER: &str = "uji-cl-user@example.test";
const PREFIX: &str = "uji-cl-%";

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

async fn user_token(pool: &MySqlPool) -> String {
    sqlx::query("DELETE FROM users WHERE email = ?").bind(USER).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji CL', ?, 'x', NOW(), NOW())")
        .bind(USER)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(USER)
        .fetch_one(pool)
        .await
        .unwrap();
    auth::login::create_token(pool, uid, "uji-cl").await.unwrap()
}

async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_checklist_items WHERE name LIKE ?")
        .bind(PREFIX)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn checklist_items_store_update_reorder_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let token = user_token(&pool).await;

    let (status, body) = send(&pool, Method::POST, "/api/checklist-items", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items",
        Some(&token),
        Some(json!({ "name": "uji-cl-a", "context": "tidak-ada" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The selected context is invalid.", "{body}");

    // Simpan: 200 (bukan 201, seperti JsonResource Laravel), urutan berikutnya setelah maksimum.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items",
        Some(&token),
        Some(json!({ "name": "uji-cl-a", "description": "Uji" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["context"], "pekerjaan");
    let a = body["data"]["id"].as_i64().unwrap();
    let order_a = body["data"]["sort_order"].as_i64().unwrap();

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items",
        Some(&token),
        Some(json!({ "name": "uji-cl-b" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let b = body["data"]["id"].as_i64().unwrap();
    assert_eq!(body["data"]["sort_order"].as_i64().unwrap(), order_a + 1, "{body}");

    // Ubah: nilai negatif ditolak; PATCH nama saja.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/checklist-items/{a}"),
        Some(&token),
        Some(json!({ "sort_order": -1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The sort order field must be at least 0.", "{body}");

    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/checklist-items/{a}"),
        Some(&token),
        Some(json!({ "name": "uji-cl-a2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], "uji-cl-a2");
    assert_eq!(body["data"]["description"], "Uji");

    // Reorder: pesan validasi memakai nama atribut bertitik seperti Laravel.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items/reorder",
        Some(&token),
        Some(json!({ "items": [{ "id": a, "sort_order": 9 }, { "id": b, "sort_order": "x" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The items.1.sort order field must be an integer.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items/reorder",
        Some(&token),
        Some(json!({ "items": [{ "id": 999999999, "sort_order": 1 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The selected items.0.id is invalid.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/checklist-items/reorder",
        Some(&token),
        Some(json!({ "items": [{ "id": a, "sort_order": 9 }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Reorder successful" }));
    let (status, body) = send(&pool, Method::GET, &format!("/api/checklist-items/{a}"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["sort_order"], 9);

    // Hapus: 200, lalu 404.
    let (status, body) = send(&pool, Method::DELETE, &format!("/api/checklist-items/{b}"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Checklist item deleted successfully" }));
    let (status, _) = send(&pool, Method::GET, &format!("/api/checklist-items/{b}"), Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::DELETE, "/api/checklist-items/999999999", Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
