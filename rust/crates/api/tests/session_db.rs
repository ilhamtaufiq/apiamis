//! Sesi cookie (pengganti BFF) terhadap MySQL: login, me, dan logout lewat router.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test session_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const EMAIL: &str = "uji-session@example.test";
/// Sama dengan `login_db`: hash bcrypt dari PHP untuk `uji-login-42`.
const PHP_HASH: &str = "$2y$10$5OHUia7uk7phxn6AR/Iyx.m61eqHC8ZFAfr5IkRJ5WPHAli5tv7vm";
const EMAIL_SYNC: &str = "uji-sync@example.test";
const PASSWORD: &str = "uji-login-42";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024,
        app_url: "http://localhost".to_string(),
    }
}

fn state(pool: &MySqlPool) -> AppState {
    AppState::new(pool.clone(), "http://localhost".to_string())
}

async fn send(
    pool: &MySqlPool,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<String>, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let body = match body {
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app(&config(), state(pool))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let set_cookies: Vec<String> = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        set_cookies,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn setup_user(pool: &MySqlPool, email: &str) {
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Session', ?, ?, NOW(), NOW())")
        .bind(email)
        .bind(PHP_HASH)
        .execute(pool)
        .await
        .unwrap();
}

/// Nilai `arumanis_session=...` (tanpa atribut) dari daftar `Set-Cookie`.
fn session_pair(set_cookies: &[String]) -> Option<String> {
    set_cookies
        .iter()
        .find(|c| c.starts_with("arumanis_session="))
        .and_then(|c| c.split(';').next())
        .map(str::to_string)
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn login_sets_cookie_me_reads_it_and_logout_revokes_it() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    setup_user(&pool, EMAIL).await;

    let (status, set_cookies, body) = send(
        &pool,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": EMAIL, "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("token").is_none(), "token tidak boleh ada di body");

    let setc = set_cookies
        .iter()
        .find(|c| c.starts_with("arumanis_session="))
        .expect("Set-Cookie sesi ada");
    assert!(setc.contains("HttpOnly"));
    assert!(setc.contains("SameSite=Strict"));
    assert!(setc.contains("Path=/"));
    assert!(setc.contains("Max-Age=43200"));
    let cookie = session_pair(&set_cookies).unwrap();

    // Cookie saja (tanpa Authorization) cukup untuk me.
    let (status, _, me) = send(&pool, "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["email"], EMAIL);

    // Logout mencabut token dan menghapus cookie.
    let (status, cleared, body) =
        send(&pool, "POST", "/api/auth/logout", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Logged out successfully");
    let expired = cleared
        .iter()
        .find(|c| c.starts_with("arumanis_session="))
        .expect("Set-Cookie penghapusan ada");
    assert!(expired.contains("Max-Age=0"));
    assert!(cleared
        .iter()
        .any(|c| c.starts_with("arumanis_impersonator_session=") && c.contains("Max-Age=0")));

    // Token yang sudah dicabut tidak bisa dipakai lagi.
    let (status, _, _) = send(&pool, "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    setup_user(&pool, EMAIL).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn me_without_session_is_unauthorized() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let (status, _, _) = send(&pool, "GET", "/api/auth/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sync_token_turns_bearer_into_session_cookie() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    setup_user(&pool, EMAIL_SYNC).await;
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(EMAIL_SYNC)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, uid, "uji-sync")
        .await
        .unwrap();

    let (status, set_cookies, body) = send(
        &pool,
        "POST",
        "/api/auth/sync-token",
        None,
        Some(json!({ "token": token })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["email"], EMAIL_SYNC);
    let cookie = session_pair(&set_cookies).expect("Set-Cookie sesi ada");

    let (status, _, me) = send(&pool, "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["data"]["email"], EMAIL_SYNC);

    let (status, _, _) = send(
        &pool,
        "POST",
        "/api/auth/sync-token",
        None,
        Some(json!({ "token": "999999|salah" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    setup_user(&pool, EMAIL_SYNC).await;
}
