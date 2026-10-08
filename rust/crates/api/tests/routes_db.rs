//! Route `/api` yang melewati gerbang maintenance dan permission, sehingga butuh database.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test routes_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use tower::ServiceExt;

fn state() -> AppState {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    AppState::new(
        sqlx::MySqlPool::connect_lazy(&url).unwrap(),
        "http://localhost".to_string(),
    )
}

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn get(path: &str) -> (StatusCode, Value) {
    let res = app(&config(), state())
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
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
async fn api_health_returns_service_info() {
    let (status, body) = get("/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["service"], "apiamis");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kecamatan_requires_bearer_token() {
    let (status, body) = get("/api/kecamatan").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, json!({ "message": "Unauthenticated." }));
}
