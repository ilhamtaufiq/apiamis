//! Login end-to-end terhadap MySQL: membuat user uji dengan hash bcrypt dari PHP.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test login_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const EMAIL: &str = "uji-login@example.test";
/// `password_hash('uji-login-42', PASSWORD_BCRYPT)` dari PHP.
const PHP_HASH: &str = "$2y$10$5OHUia7uk7phxn6AR/Iyx.m61eqHC8ZFAfr5IkRJ5WPHAli5tv7vm";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn login(pool: &MySqlPool, email: &str, password: &str) -> (StatusCode, Value) {
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let res = app(&config(), state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "email": email, "password": password }).to_string(),
                ))
                .unwrap(),
        )
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
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(EMAIL)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(EMAIL)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn login_with_laravel_hash_returns_user_and_working_token() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Login', ?, ?, NOW(), NOW())")
        .bind(EMAIL)
        .bind(PHP_HASH)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = login(&pool, EMAIL, "salah-banget").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The given data was invalid.");
    assert_eq!(
        body["errors"]["email"],
        json!(["The provided credentials are incorrect."])
    );

    let (status, body) = login(&pool, EMAIL, "uji-login-42").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["email"], EMAIL);
    assert!(
        body["user"].get("password").is_none(),
        "password tidak boleh bocor"
    );
    assert_eq!(body["user"]["is_protected_from_deletion"], false);

    let token = body["token"].as_str().unwrap().to_string();
    let (id, _) = token.split_once('|').unwrap();
    let who = auth::authenticate(&pool, &token).await.unwrap();
    assert_eq!(who.token_id.to_string(), id);
    assert_eq!(who.abilities, vec!["*".to_string()]);

    cleanup(&pool).await;
}
