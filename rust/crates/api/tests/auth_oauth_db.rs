//! Handoff dan Google OAuth (`auth_oauth`) lewat router dan MySQL. Google tidak pernah dipanggil:
//! endpoint diganti server stub lokal.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test auth_oauth_db -- --include-ignored
//! ```

use std::collections::HashMap;

use api::{app, auth_oauth, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const CALLER_EMAIL: &str = "uji-oauth-caller@example.test";
const ROW_CALLER_EMAIL: &str = "uji-oauth-caller-row@example.test";
const NEW_EMAIL: &str = "uji-oauth-baru@example.test";
const EXISTING_EMAIL: &str = "uji-oauth-lama@example.test";
const FAILING_EMAIL: &str = "uji-oauth-gagal@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

fn state(pool: &MySqlPool) -> AppState {
    AppState::new(pool.clone(), "http://localhost".to_string())
}

/// Kirim satu request ke router. Mengembalikan status, header, dan body JSON (atau `Null`).
async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
    };
    let res = app(&config(), state(pool))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn cleanup_users(pool: &MySqlPool, emails: &[&str]) {
    for email in emails {
        sqlx::query(
            "DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM users WHERE email = ?")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
    }
}

/// User uji dengan token Sanctum sungguhan (`auth-token`), untuk handoff.
async fn caller_token(pool: &MySqlPool, email: &str) -> (u64, String) {
    cleanup_users(pool, &[email]).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Caller', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "auth-token")
        .await
        .unwrap();
    (uid, token)
}

fn cache_key(suffix: &str) -> String {
    format!("{}{suffix}", auth_oauth::cache_prefix())
}

async fn cache_row_exists(pool: &MySqlPool, key: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM `cache` WHERE `key` = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
        > 0
}

async fn delete_cache(pool: &MySqlPool, key: &str) {
    sqlx::query("DELETE FROM `cache` WHERE `key` = ?")
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
}

/// Baris cache seperti yang ditulis Laravel (`serialize()`), dengan kedaluwarsa `offset` detik dari sekarang.
async fn insert_laravel_row(pool: &MySqlPool, key: &str, value: &str, offset: i64) {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT INTO `cache` (`key`, `value`, `expiration`) VALUES (?, ?, ?)")
        .bind(key)
        .bind(value)
        .bind(now + offset)
        .execute(pool)
        .await
        .unwrap();
}

/// `serialize()` PHP untuk state OAuth `['platform' => ..., 'callback_url' => ...]`.
fn state_row(platform: &str, callback: &str) -> String {
    format!(
        "a:2:{{s:8:\"platform\";s:{}:\"{platform}\";s:12:\"callback_url\";s:{}:\"{callback}\";}}",
        platform.len(),
        callback.len()
    )
}

fn location(headers: &HeaderMap) -> String {
    headers
        .get(header::LOCATION)
        .expect("Location header")
        .to_str()
        .unwrap()
        .to_string()
}

/// Ambil nilai `key` dari fragment `#k=v` (sudah di-percent-decode sederhana).
fn fragment_value(url: &str, key: &str) -> Option<String> {
    let fragment = url.split_once('#')?.1;
    fragment.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.replace("%7C", "|").replace("%20", " "))
    })
}

/// Server stub untuk `token` dan `userinfo` Google.
async fn stub_google(token_ok: bool, userinfo: Value) -> auth_oauth::GoogleEndpoints {
    let app = Router::new()
        .route(
            "/token",
            post(move |body: String| async move {
                if token_ok && body.contains("code=uji-code") {
                    (StatusCode::OK, Json(json!({ "access_token": "ya29.stub" })))
                } else {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid_grant" })),
                    )
                }
            }),
        )
        .route(
            "/userinfo",
            axum::routing::get(move |headers: HeaderMap| {
                let userinfo = userinfo.clone();
                async move {
                    let authorized = headers
                        .get(header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        == Some("Bearer ya29.stub");
                    if authorized {
                        (StatusCode::OK, Json(userinfo))
                    } else {
                        (StatusCode::UNAUTHORIZED, Json(json!({})))
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    auth_oauth::GoogleEndpoints {
        auth: format!("http://{addr}/auth"),
        token: format!("http://{addr}/token"),
        userinfo: format!("http://{addr}/userinfo"),
        // Tidak ada route /people di stub: People API gagal dan gender tetap NULL.
        people: format!("http://{addr}/people"),
    }
}

fn google_client(endpoints: auth_oauth::GoogleEndpoints) -> auth_oauth::GoogleClient {
    auth_oauth::GoogleClient::with_endpoints(
        endpoints,
        "uji-client.example.test",
        "uji-secret",
        "http://localhost/api/auth/google/callback",
    )
}

fn query(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Handoff
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn handoff_create_then_exchange_returns_same_token_once() {
    let pool = pool().await;
    let (uid, token) = caller_token(&pool, CALLER_EMAIL).await;

    let (status, _, body) =
        send(&pool, Method::POST, "/api/auth/handoff", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["expires_in"], 60);
    let code = body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 48);
    assert!(cache_row_exists(&pool, &cache_key(&format!("auth_handoff:{code}"))).await);

    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["token"], token);
    assert_eq!(body["user"]["id"], uid);
    assert_eq!(body["user"]["email"], CALLER_EMAIL);
    assert!(body["user"]["roles"].is_array());

    // Sekali pakai: kode kedua harus 410.
    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(body["message"], "Handoff code invalid or expired.");
    assert!(!cache_row_exists(&pool, &cache_key(&format!("auth_handoff:{code}"))).await);

    cleanup_users(&pool, &[CALLER_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn handoff_create_requires_authentication() {
    let pool = pool().await;
    let (status, _, body) = send(&pool, Method::POST, "/api/auth/handoff", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Unauthenticated.");

    let (status, _, _) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff",
        Some("999999|salah"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn handoff_exchange_validates_code_like_laravel() {
    let pool = pool().await;

    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The given data was invalid.");
    assert_eq!(body["errors"]["code"][0], "The code field is required.");

    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": "pendek" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["code"][0],
        "The code field must be 48 characters."
    );

    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": "a".repeat(48) })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::GONE,
        "kode valid format tapi tidak ada: {body}"
    );
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn handoff_exchange_accepts_row_written_by_laravel_and_rejects_expired() {
    let pool = pool().await;
    let (uid, token) = caller_token(&pool, ROW_CALLER_EMAIL).await;

    // Format yang ditulis `Cache::put('auth_handoff:…', ['token' => …, 'user_id' => …], 60)`.
    let code = "u".repeat(48);
    let key = cache_key(&format!("auth_handoff:{code}"));
    let serialized = format!(
        "a:2:{{s:5:\"token\";s:{}:\"{}\";s:7:\"user_id\";i:{uid};}}",
        token.len(),
        token
    );
    insert_laravel_row(&pool, &key, &serialized, 60).await;
    let (status, _, body) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token"], token);

    // Baris kedaluwarsa: 410 dan baris ikut dihapus.
    let expired_code = "e".repeat(48);
    let expired_key = cache_key(&format!("auth_handoff:{expired_code}"));
    insert_laravel_row(&pool, &expired_key, &serialized, -5).await;
    let (status, _, _) = send(
        &pool,
        Method::POST,
        "/api/auth/handoff/exchange",
        None,
        Some(json!({ "code": expired_code })),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    assert!(!cache_row_exists(&pool, &expired_key).await);

    cleanup_users(&pool, &[ROW_CALLER_EMAIL]).await;
}

// ---------------------------------------------------------------------------
// Google: redirect dan callback
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn google_redirect_returns_url_and_stores_state_for_ten_minutes() {
    let pool = pool().await;
    let (status, _, body) = send(
        &pool,
        Method::GET,
        "/api/auth/google?platform=mobile&callback_url=pengawas%3A%2F%2Foauth-callback%2F",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let url = body["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://accounts.google.com/o/oauth2/auth?client_id="),
        "{url}"
    );
    assert!(url.contains("&response_type=code&state="), "{url}");
    assert!(url.contains(
        "scope=openid+profile+email+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fuser.gender.read"
    ));

    let state = url.rsplit("state=").next().unwrap().to_string();
    assert_eq!(state.len(), 40);
    let key = cache_key(&format!("oauth_state:{state}"));
    let row: String = sqlx::query_scalar("SELECT `value` FROM `cache` WHERE `key` = ?")
        .bind(&key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        row,
        state_row("mobile", "pengawas://oauth-callback").as_str()
    );
    delete_cache(&pool, &key).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn google_callback_without_code_redirects_with_error_and_consumes_state() {
    let pool = pool().await;
    let state = "uji-state-tanpa-code-0000000000000000000";
    let key = cache_key(&format!("oauth_state:{state}"));
    insert_laravel_row(
        &pool,
        &key,
        &state_row("mobile", "http://127.0.0.1:9/cb-uji"),
        600,
    )
    .await;

    let uri = format!("/api/auth/google/callback?state={state}&error=access_denied");
    let (status, headers, _) = send(&pool, Method::GET, &uri, None, None).await;
    assert_eq!(status, StatusCode::FOUND);
    let loc = location(&headers);
    assert_eq!(
        loc,
        "http://127.0.0.1:9/cb-uji#error=Google%20authentication%20failed.%20Please%20try%20again."
    );
    assert!(
        !cache_row_exists(&pool, &key).await,
        "state harus sekali pakai"
    );
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn google_callback_creates_user_with_default_role_and_returns_token() {
    let pool = pool().await;
    cleanup_users(&pool, &[NEW_EMAIL]).await;
    let endpoints = stub_google(
        true,
        json!({ "sub": "uji-sub-1", "name": "Uji Baru", "email": NEW_EMAIL, "picture": "https://img.example.test/p.png" }),
    )
    .await;
    let google = google_client(endpoints);

    let state = "uji-state-baru-00000000000000000000000000";
    let key = cache_key(&format!("oauth_state:{state}"));
    insert_laravel_row(
        &pool,
        &key,
        &state_row("mobile", "http://127.0.0.1:9/cb-uji"),
        600,
    )
    .await;

    let q = query(&[("code", "uji-code"), ("state", state)]);
    let res = auth_oauth::callback_response(&pool, &q, &google).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    let loc = location(res.headers());
    assert!(loc.starts_with("http://127.0.0.1:9/cb-uji#token="), "{loc}");
    let token = fragment_value(&loc, "token").unwrap();

    let who = auth::authenticate(&pool, &token)
        .await
        .expect("token harus valid");
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(who.user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(email, NEW_EMAIL);

    let (google_id, avatar, gender, name): (
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = sqlx::query_as("SELECT google_id, avatar, gender, name FROM users WHERE id = ?")
        .bind(who.user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(google_id.as_deref(), Some("uji-sub-1"));
    assert_eq!(avatar.as_deref(), Some("https://img.example.test/p.png"));
    assert_eq!(gender, None);
    assert_eq!(name, "Uji Baru");

    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.name FROM model_has_roles m JOIN roles r ON r.id = m.role_id WHERE m.model_id = ? AND m.model_type = ?",
    )
    .bind(who.user_id)
    .bind(auth::USER_TOKENABLE_TYPE)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(roles, vec!["user".to_string()]);

    // Login kedua untuk email yang sama: tidak ada user baru, role tidak ditambah lagi.
    insert_laravel_row(
        &pool,
        &key,
        &state_row("mobile", "http://127.0.0.1:9/cb-uji"),
        600,
    )
    .await;
    let res = auth_oauth::callback_response(&pool, &q, &google).await;
    assert!(location(res.headers()).contains("#token="));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
        .bind(NEW_EMAIL)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let role_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM model_has_roles WHERE model_id = ?")
            .bind(who.user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(role_rows, 1);

    cleanup_users(&pool, &[NEW_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn google_callback_updates_existing_user_by_email() {
    let pool = pool().await;
    cleanup_users(&pool, &[EXISTING_EMAIL]).await;
    sqlx::query("INSERT INTO users (name, email, gender, password, created_at, updated_at) VALUES ('Lama', ?, 'male', 'x', NOW(), NOW())")
        .bind(EXISTING_EMAIL)
        .execute(&pool)
        .await
        .unwrap();
    let before: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(EXISTING_EMAIL)
        .fetch_one(&pool)
        .await
        .unwrap();

    let google = google_client(
        stub_google(
            true,
            json!({ "sub": "uji-sub-lama", "name": "Lama Diperbarui", "email": EXISTING_EMAIL }),
        )
        .await,
    );
    let q = query(&[("code", "uji-code")]);
    let res = auth_oauth::callback_response(&pool, &q, &google).await;
    let loc = location(res.headers());
    // Tanpa state tersimpan: redirect ke FRONTEND_URL (web), jadi cukup cek fragment token.
    let token = fragment_value(&loc, "token").expect("token di fragment");
    let who = auth::authenticate(&pool, &token).await.unwrap();
    assert_eq!(
        who.user_id, before,
        "user lama dipakai ulang, bukan dibuat baru"
    );

    let (name, google_id, gender): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT name, google_id, gender FROM users WHERE id = ?")
            .bind(before)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Lama Diperbarui");
    assert_eq!(google_id.as_deref(), Some("uji-sub-lama"));
    // Perilaku Laravel: gender ikut ditimpa NULL karena userinfo tidak memuat gender.
    assert_eq!(gender, None);

    cleanup_users(&pool, &[EXISTING_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn google_callback_token_failure_redirects_with_error_and_no_user() {
    let pool = pool().await;
    cleanup_users(&pool, &[FAILING_EMAIL]).await;
    let google = google_client(
        stub_google(
            false,
            json!({ "sub": "uji-sub-gagal", "name": "Gagal", "email": FAILING_EMAIL }),
        )
        .await,
    );
    let q = query(&[("code", "kode-salah")]);
    let res = auth_oauth::callback_response(&pool, &q, &google).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    let loc = location(res.headers());
    assert!(
        loc.contains("#error=Google%20authentication%20failed.%20Please%20try%20again."),
        "{loc}"
    );
    assert!(!loc.contains("#token="));

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
        .bind(FAILING_EMAIL)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "kode gagal tidak boleh membuat user");
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    MySqlPool::connect(&url).await.expect("koneksi MySQL")
}
