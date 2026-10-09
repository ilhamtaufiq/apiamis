//! Backup Google Drive (`google_drive_backup`) lewat router dan MySQL. Google tidak pernah dipanggil:
//! endpoint OAuth dan Drive diarahkan ke server stub lokal lewat env (`GOOGLE_API_BASE_URL`,
//! `GOOGLE_OAUTH_TOKEN_URL`, `GOOGLE_OAUTH_REFRESH_URL`).
//!
//! Tes berjalan berurutan (`LOCK`) karena env dan direktori private dipakai bersama. Baris DB yang
//! dibuat tes diberi prefix `uji-agd-` dan dihapus di akhir. Berkas memakai direktori sementara di
//! `CARGO_TARGET_TMPDIR/uji-agd-gdrive`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test app_settings_gdrive_db -- --include-ignored
//! ```

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};

use api::{app, auth_oauth::cache_prefix, crypt, AppState};
use axum::{
    body::{Body, Bytes},
    extract::{Query, State},
    http::{header, HeaderMap, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tokio::sync::Mutex;
use tower::ServiceExt;

const ADMIN: &str = "uji-agd-admin@example.test";
const PLAIN: &str = "uji-agd-plain@example.test";
/// Tepat 32 byte (kunci AES-256). Hanya untuk tes.
const TEST_APP_KEY: &str = "uji-agd-app-key-0123456789abcdef";
const CLIENT_ID: &str = "uji-agd-client.apps.example.test";
const CLIENT_SECRET: &str = "uji-agd-secret";
const FRONTEND: &str = "http://uji-agd-frontend.example.test";
const REFRESH: &str = "1//uji-agd-refresh";
const BACKUP: &str = "uji-agd-backup-1.zip";
const CHUNK: usize = 8 * 1024 * 1024;
const FILE_ID: &str = "uji-agd-file-1";
const FOLDER_ID: &str = "uji-agd-folder-1";
const STATE_FAILED_MSG: &str = "State+OAuth+Google+Drive+tidak+valid+atau+kedaluwarsa";

static LOCK: Mutex<()> = Mutex::const_new(());

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 16 * 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.expect("koneksi MySQL")
}

/// Env untuk satu tes. Mengembalikan direktori `private` (disk `local` Laravel).
fn setup_env(stub: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uji-agd-gdrive");
    let _ = std::fs::remove_dir_all(&base);
    let private = base.join("private");
    std::fs::create_dir_all(private.join("system-backups")).unwrap();

    std::env::set_var("PRIVATE_STORAGE_PATH", &private);
    std::env::set_var("APP_KEY", TEST_APP_KEY);
    std::env::set_var("GOOGLE_CLIENT_ID", CLIENT_ID);
    std::env::set_var("GOOGLE_CLIENT_SECRET", CLIENT_SECRET);
    std::env::set_var("FRONTEND_URL", FRONTEND);
    std::env::remove_var("GOOGLE_DRIVE_REDIRECT_URI");
    std::env::set_var("GOOGLE_API_BASE_URL", stub);
    std::env::set_var("GOOGLE_OAUTH_TOKEN_URL", format!("{stub}/token"));
    std::env::set_var("GOOGLE_OAUTH_REFRESH_URL", format!("{stub}/token"));
    private
}

// ---------------------------------------------------------------------------
// Server stub Google
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StubLog {
    refresh_calls: AtomicUsize,
    folder_creates: AtomicUsize,
    /// Header `Content-Range` setiap PUT yang diterima.
    puts: StdMutex<Vec<String>>,
    put_bytes: AtomicUsize,
}

#[derive(Clone)]
struct StubCtx {
    base: String,
    log: Arc<StubLog>,
    /// Jeda sebelum jawaban PUT pertama. Dipakai tes batal di tengah upload.
    first_put_delay: Duration,
}

fn json_resp(code: StatusCode, body: Value) -> Response {
    (code, Json(body)).into_response()
}

fn bearer_of(headers: &HeaderMap) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

async fn stub_token(State(ctx): State<StubCtx>, body: String) -> Response {
    if body.contains("grant_type=authorization_code") && body.contains("code=uji-agd-code") {
        return json_resp(
            StatusCode::OK,
            json!({
                "access_token": "ya29.uji-agd-1",
                "refresh_token": REFRESH,
                "expires_in": 3599,
                "token_type": "Bearer",
            }),
        );
    }
    if body.contains("grant_type=refresh_token") && body.contains("uji-agd-refresh") {
        ctx.log.refresh_calls.fetch_add(1, Ordering::SeqCst);
        return json_resp(
            StatusCode::OK,
            json!({ "access_token": "ya29.uji-agd-2", "expires_in": 3599 }),
        );
    }
    json_resp(
        StatusCode::BAD_REQUEST,
        json!({ "error": "invalid_grant", "error_description": "uji stub menolak" }),
    )
}

async fn stub_userinfo(headers: HeaderMap) -> Response {
    if bearer_of(&headers) == "Bearer ya29.uji-agd-1" {
        json_resp(
            StatusCode::OK,
            json!({
                "sub": "uji-agd-sub",
                "name": "Uji AGD",
                "email": "uji-agd-google@example.test",
            }),
        )
    } else {
        json_resp(StatusCode::UNAUTHORIZED, json!({ "error": "bad token" }))
    }
}

async fn stub_drive_list(headers: HeaderMap) -> Response {
    if !bearer_of(&headers).starts_with("Bearer ya29.uji-agd-") {
        return json_resp(
            StatusCode::UNAUTHORIZED,
            json!({ "error": { "message": "bad token" } }),
        );
    }
    json_resp(StatusCode::OK, json!({ "files": [] }))
}

async fn stub_drive_create(State(ctx): State<StubCtx>, headers: HeaderMap) -> Response {
    if !bearer_of(&headers).starts_with("Bearer ya29.uji-agd-") {
        return json_resp(
            StatusCode::UNAUTHORIZED,
            json!({ "error": { "message": "bad token" } }),
        );
    }
    ctx.log.folder_creates.fetch_add(1, Ordering::SeqCst);
    json_resp(
        StatusCode::OK,
        json!({ "id": FOLDER_ID, "name": "Arumanis Backups" }),
    )
}

async fn stub_upload_init(
    State(ctx): State<StubCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    // Hanya token hasil refresh yang diterima, sehingga jalur refresh ikut teruji.
    if bearer_of(&headers) != "Bearer ya29.uji-agd-2"
        || q.get("uploadType").map(String::as_str) != Some("resumable")
        || headers.get("x-upload-content-length").is_none()
    {
        return json_resp(
            StatusCode::UNAUTHORIZED,
            json!({ "error": { "message": "bad init" } }),
        );
    }
    let location = format!("{}/session/uji-agd-sess", ctx.base);
    (
        StatusCode::OK,
        [(header::LOCATION, location)],
        Json(json!({})),
    )
        .into_response()
}

async fn stub_put_chunk(State(ctx): State<StubCtx>, headers: HeaderMap, body: Bytes) -> Response {
    let range = headers
        .get(header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let index = {
        let mut puts = ctx.log.puts.lock().unwrap();
        puts.push(range);
        puts.len() - 1
    };
    ctx.log.put_bytes.fetch_add(body.len(), Ordering::SeqCst);

    if index == 0 {
        if !ctx.first_put_delay.is_zero() {
            tokio::time::sleep(ctx.first_put_delay).await;
        }
        return (
            StatusCode::PERMANENT_REDIRECT,
            [(header::RANGE, format!("bytes=0-{}", CHUNK - 1))],
        )
            .into_response();
    }
    json_resp(
        StatusCode::OK,
        json!({
            "id": FILE_ID,
            "name": BACKUP,
            "size": ctx.log.put_bytes.load(Ordering::SeqCst).to_string(),
        }),
    )
}

/// Menyalakan stub. Mengembalikan base URL dan catatan panggilan.
async fn start_stub(first_put_delay: Duration) -> (String, Arc<StubLog>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(StubLog::default());
    let ctx = StubCtx {
        base: base.clone(),
        log: log.clone(),
        first_put_delay,
    };
    let router = Router::new()
        .route("/token", post(stub_token))
        .route("/oauth2/v3/userinfo", get(stub_userinfo))
        .route(
            "/drive/v3/files",
            get(stub_drive_list).post(stub_drive_create),
        )
        .route("/upload/drive/v3/files", post(stub_upload_init))
        .route("/session/{id}", put(stub_put_chunk))
        // Chunk 8 MiB melebihi batas body default axum (2 MB).
        .layer(axum::extract::DefaultBodyLimit::disable())
        .with_state(ctx);
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, log)
}

// ---------------------------------------------------------------------------
// Pembantu HTTP, pengguna, dan berkas
// ---------------------------------------------------------------------------

async fn call(
    state: &AppState,
    method: Method,
    uri: &str,
    token: Option<&str>,
    extra: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let res = app(&config(), state.clone())
        .oneshot(req.body(Body::empty()).unwrap())
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

fn location(headers: &HeaderMap) -> String {
    headers
        .get(header::LOCATION)
        .expect("redirect punya Location")
        .to_str()
        .unwrap()
        .to_string()
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    // Token dari seeding sebelumnya ikut dihapus agar tidak menggantung.
    sqlx::query("DELETE FROM personal_access_tokens WHERE name = 'uji-agd' AND tokenable_id IN (SELECT id FROM users WHERE email = ?)")
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji AGD', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if admin {
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
    }
    auth::login::create_token(pool, uid, "uji-agd")
        .await
        .unwrap()
}

/// Menghapus hanya baris yang dibuat tes ini: pengguna `uji-agd-*` dan state OAuth yang dicatat.
async fn cleanup(pool: &MySqlPool, states: &[String]) {
    for s in states {
        sqlx::query("DELETE FROM `cache` WHERE `key` = ?")
            .bind(format!("{}google_drive_oauth_state:{s}", cache_prefix()))
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM personal_access_tokens WHERE name = 'uji-agd'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email LIKE 'uji-agd-%')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE 'uji-agd-%'")
        .execute(pool)
        .await
        .unwrap();
}

fn encrypt_json(value: &Value) -> String {
    crypt::encrypt_string(TEST_APP_KEY.as_bytes(), &value.to_string())
}

fn write_creds(private: &Path, value: &Value) {
    let dir = private.join("google-drive");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("credentials.json"), encrypt_json(value)).unwrap();
}

fn read_creds(private: &Path) -> Value {
    let payload = std::fs::read_to_string(private.join("google-drive").join("credentials.json"))
        .expect("berkas kredensial ada");
    let json = crypt::decrypt_string(TEST_APP_KEY.as_bytes(), payload.trim())
        .expect("kredensial bisa didekripsi dengan APP_KEY tes");
    serde_json::from_str(&json).unwrap()
}

fn creds_connected(refresh: &str, access: &str, expires_at: &str, folder: Option<&str>) -> Value {
    json!({
        "refresh_token": refresh,
        "access_token": access,
        "access_token_expires_at": expires_at,
        "email": "uji-agd-google@example.test",
        "folder_id": folder,
        "connected_at": "2026-10-01T08:00:00+00:00",
    })
}

fn write_job_file(private: &Path, job_id: &str, value: &Value) {
    let dir = private.join("google-drive-upload-jobs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{job_id}.json")), value.to_string()).unwrap();
}

/// Berkas backup uji: 8 MiB + 1000 byte, sehingga upload memakai dua chunk.
fn write_backup(private: &Path) -> usize {
    let data: Vec<u8> = (0..CHUNK + 1000).map(|i| (i % 251) as u8).collect();
    std::fs::write(private.join("system-backups").join(BACKUP), &data).unwrap();
    data.len()
}

/// Ambil `state` dari URL connect dan pastikan parameternya sama dengan Socialite.
async fn connect_state(state: &AppState, admin_token: &str) -> String {
    let (status, _, body) = call(
        state,
        Method::GET,
        "/api/app-settings/backups/google-drive/connect",
        Some(admin_token),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = body["data"]["url"].as_str().unwrap().to_string();
    assert!(url.starts_with("https://accounts.google.com/o/oauth2/auth?"));
    let state_token = url.rsplit("state=").next().unwrap().to_string();
    assert_eq!(state_token.len(), 40, "state Str::random(40)");
    assert!(state_token.chars().all(|c| c.is_ascii_alphanumeric()));
    state_token
}

async fn wait_job(state: &AppState, token: &str, job_id: &str, want: &[&str]) -> Value {
    let uri = format!("/api/app-settings/backups/google-drive/jobs/{job_id}");
    for _ in 0..300 {
        let (status, _, body) = call(state, Method::GET, &uri, Some(token), &[]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let s = body["data"]["status"].as_str().unwrap_or("").to_string();
        if want.contains(&s.as_str()) {
            return body["data"].clone();
        }
        if s == "failed" {
            panic!("job gagal: {}", body["data"]);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("job {job_id} tidak mencapai {want:?}");
}

// ---------------------------------------------------------------------------
// Tes
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_routes_are_admin_only() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    let (status, _, _) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/status",
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for (method, uri) in [
        (Method::GET, "/api/app-settings/backups/google-drive/status"),
        (
            Method::GET,
            "/api/app-settings/backups/google-drive/connect",
        ),
        (Method::DELETE, "/api/app-settings/backups/google-drive"),
        (
            Method::GET,
            "/api/app-settings/backups/google-drive/jobs/uji-agd-job-x",
        ),
        (
            Method::DELETE,
            "/api/app-settings/backups/google-drive/jobs/uji-agd-job-x",
        ),
        (
            Method::POST,
            "/api/app-settings/backups/uji-agd-backup-1.zip/google-drive",
        ),
    ] {
        let (status, _, body) = call(&state, method.clone(), uri, Some(&plain), &[]).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
    }

    let (status, _, body) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/status",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["folder_name"], "Arumanis Backups");
    assert_eq!(body["data"]["connected"], false);
    assert_eq!(body["data"]["configured"], true);

    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_connect_url_matches_socialite_and_stores_state() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;

    let (status, _, body) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/connect",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = body["data"]["url"].as_str().unwrap();
    let prefix =
        "https://accounts.google.com/o/oauth2/auth?client_id=uji-agd-client.apps.example.test\
&redirect_uri=http%3A%2F%2Flocalhost%2Fapi%2Fapp-settings%2Fbackups%2Fgoogle-drive%2Fcallback\
&scope=openid+profile+email+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file\
&response_type=code&access_type=offline&prompt=consent&state=";
    assert!(url.starts_with(prefix), "{url}");

    let state_token = url.trim_start_matches(prefix).to_string();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM `cache` WHERE `key` = ?")
        .bind(format!(
            "{}google_drive_oauth_state:{state_token}",
            cache_prefix()
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "state tersimpan di tabel cache");

    cleanup(&pool, &[state_token]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_callback_connects_encrypts_and_consumes_state() {
    let _guard = LOCK.lock().await;
    let (stub, log) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;

    let token_state = connect_state(&state, &admin).await;
    let uri = format!(
        "/api/app-settings/backups/google-drive/callback?code=uji-agd-code&state={token_state}"
    );
    let (status, headers, _) = call(&state, Method::GET, &uri, None, &[]).await;
    assert_eq!(status, StatusCode::FOUND);
    let loc = location(&headers);
    assert!(loc.ends_with("/settings?google_drive=connected"), "{loc}");

    // Kredensial terenkripsi, dengan refresh token dan email dari userinfo.
    let creds = read_creds(&private);
    assert_eq!(creds["refresh_token"], REFRESH);
    assert_eq!(creds["email"], "uji-agd-google@example.test");
    assert_eq!(creds["access_token"], "ya29.uji-agd-1");
    // Folder dibuat setelah connect (best-effort) dan disimpan.
    assert_eq!(log.folder_creates.load(Ordering::SeqCst), 1);
    assert_eq!(creds["folder_id"], FOLDER_ID);

    let (status, _, body) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/status",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["connected"], true);
    assert_eq!(body["data"]["email"], "uji-agd-google@example.test");
    assert_eq!(body["data"]["folder_id"], FOLDER_ID);
    let text = body.to_string();
    assert!(
        !text.contains(REFRESH) && !text.contains("ya29."),
        "token tidak boleh bocor: {text}"
    );

    // State sekali pakai: pemutaran ulang ditolak dengan redirect error.
    let (status, headers, _) = call(&state, Method::GET, &uri, None, &[]).await;
    assert_eq!(status, StatusCode::FOUND);
    let loc = location(&headers);
    assert!(
        loc.contains(&format!(
            "google_drive=error&google_drive_message={STATE_FAILED_MSG}"
        )),
        "{loc}"
    );

    cleanup(&pool, &[token_state]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_callback_failures_redirect_with_message() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;

    // Error dari Google.
    let (status, headers, _) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/callback?error=access_denied",
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert!(location(&headers).contains("google_drive=error&google_drive_message=access_denied"));

    // Tanpa code.
    let (_, headers, _) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/callback?state=uji-agd-x",
        None,
        &[],
    )
    .await;
    assert!(location(&headers).contains("google_drive_message=Kode+OAuth+tidak+ada"));

    // Tanpa state.
    let (_, headers, _) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/callback?code=uji-agd-code",
        None,
        &[],
    )
    .await;
    assert!(location(&headers).contains(&format!("google_drive_message={STATE_FAILED_MSG}")));

    // Kode ditolak Google: state terpakai, kredensial tidak ditulis.
    let token_state = connect_state(&state, &admin).await;
    let uri = format!(
        "/api/app-settings/backups/google-drive/callback?code=uji-agd-buruk&state={token_state}"
    );
    let (status, headers, _) = call(&state, Method::GET, &uri, None, &[]).await;
    assert_eq!(status, StatusCode::FOUND);
    let loc = location(&headers);
    assert!(
        loc.contains("google_drive=error&google_drive_message=Google+menolak+penukaran+kode"),
        "{loc}"
    );
    assert!(!private
        .join("google-drive")
        .join("credentials.json")
        .exists());

    cleanup(&pool, &[token_state]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_callback_is_throttled_twenty_per_minute() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    setup_env(&stub);
    let pool = pool().await;
    // Satu AppState dipakai bersama agar limiter-nya sama antar request.
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let uri = "/api/app-settings/backups/google-drive/callback?state=uji-agd-x";
    let ip = [("x-forwarded-for", "203.0.113.77")];

    for i in 0..20 {
        let (status, _, _) = call(&state, Method::GET, uri, None, &ip).await;
        assert_eq!(status, StatusCode::FOUND, "request ke-{}", i + 1);
    }
    let (status, headers, body) = call(&state, Method::GET, uri, None, &ip).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.get(header::RETRY_AFTER).is_some());
    assert_eq!(body["message"], "Too Many Attempts.");

    // IP lain tidak terkena batas.
    let (status, _, _) = call(
        &state,
        Method::GET,
        uri,
        None,
        &[("x-forwarded-for", "203.0.113.78")],
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);

    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_disconnect_removes_credentials() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;
    write_creds(
        &private,
        &creds_connected(
            REFRESH,
            "ya29.uji-agd-2",
            "2999-01-01T00:00:00+00:00",
            Some(FOLDER_ID),
        ),
    );

    let (status, _, body) = call(
        &state,
        Method::DELETE,
        "/api/app-settings/backups/google-drive",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Koneksi Google Drive diputus");
    assert_eq!(body["data"]["connected"], false);
    assert!(!private
        .join("google-drive")
        .join("credentials.json")
        .exists());

    // Idempoten: menghapus lagi tetap 200.
    let (status, _, body) = call(
        &state,
        Method::DELETE,
        "/api/app-settings/backups/google-drive",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_upload_refreshes_token_creates_folder_and_completes() {
    let _guard = LOCK.lock().await;
    let (stub, log) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;
    let total = write_backup(&private);
    // Access token sudah kedaluwarsa: harus di-refresh dulu.
    write_creds(
        &private,
        &creds_connected(
            REFRESH,
            "ya29.uji-agd-lama",
            "2000-01-01T00:00:00+00:00",
            None,
        ),
    );

    let uri = format!("/api/app-settings/backups/{BACKUP}/google-drive");
    let (status, _, body) = call(&state, Method::POST, &uri, Some(&admin), &[]).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["message"], "Upload ke Google Drive sedang diproses");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();

    let done = wait_job(&state, &admin, &job_id, &["completed"]).await;
    assert_eq!(done["result"]["id"], FILE_ID);
    assert_eq!(done["result"]["folder_id"], FOLDER_ID);
    assert_eq!(done["result"]["name"], BACKUP);
    assert_eq!(
        done["result"]["webViewLink"],
        format!("https://drive.google.com/file/d/{FILE_ID}/view")
    );
    assert_eq!(done["result"]["size"], total as i64);
    assert_eq!(done["progress"], 100);

    assert!(
        log.refresh_calls.load(Ordering::SeqCst) >= 1,
        "token lama harus di-refresh"
    );
    let puts = log.puts.lock().unwrap().clone();
    assert_eq!(puts.len(), 2, "dua chunk: 308 lalu 200");
    assert_eq!(puts[0], format!("bytes 0-{}/{total}", CHUNK - 1));
    assert_eq!(puts[1], format!("bytes {CHUNK}-{}/{total}", total - 1));
    assert_eq!(log.put_bytes.load(Ordering::SeqCst), total);

    let creds = read_creds(&private);
    assert_eq!(creds["access_token"], "ya29.uji-agd-2");
    assert_eq!(creds["folder_id"], FOLDER_ID);
    assert!(private
        .join("google-drive-upload-jobs")
        .join(format!("{job_id}.json"))
        .exists());

    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_upload_rejects_unconnected_and_missing_backup() {
    let _guard = LOCK.lock().await;
    let (stub, log) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;
    write_backup(&private);

    // Belum terhubung.
    let uri = format!("/api/app-settings/backups/{BACKUP}/google-drive");
    let (status, _, body) = call(&state, Method::POST, &uri, Some(&admin), &[]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Google Drive belum terhubung. Hubungkan dulu di Pengaturan."
    );

    // Terhubung, tetapi berkas backup tidak ada.
    write_creds(
        &private,
        &creds_connected(
            REFRESH,
            "ya29.uji-agd-2",
            "2999-01-01T00:00:00+00:00",
            Some(FOLDER_ID),
        ),
    );
    let (status, _, body) = call(
        &state,
        Method::POST,
        "/api/app-settings/backups/uji-agd-tidak-ada.zip/google-drive",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "Backup tidak ditemukan");

    // Nama tidak lolos guard (spasi) dan nama bukan .zip (tidak cocok dengan rute).
    let (status, _, body) = call(
        &state,
        Method::POST,
        "/api/app-settings/backups/uji%20agd.zip/google-drive",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Nama backup tidak valid");
    let (status, _, _) = call(
        &state,
        Method::POST,
        "/api/app-settings/backups/uji-agd-backup-1.txt/google-drive",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert!(
        log.puts.lock().unwrap().is_empty(),
        "tidak ada upload yang dikirim"
    );
    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_job_status_and_cancel_without_worker() {
    let _guard = LOCK.lock().await;
    let (stub, _) = start_stub(Duration::ZERO).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;

    let (status, _, body) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/jobs/uji-agd-job-tidak-ada",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "Status upload tidak ditemukan");
    let (status, _, body) = call(
        &state,
        Method::GET,
        "/api/app-settings/backups/google-drive/jobs/bad.id",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "ID backup tidak valid");

    // Job yang masih berjalan: batal hanya menandai cancel_requested.
    write_job_file(
        &private,
        "uji-agd-job-jalan",
        &json!({
            "job_id": "uji-agd-job-jalan", "status": "running", "filename": BACKUP, "size": 10,
            "created_at": "2026-10-09T10:00:00+00:00", "message": "Mengunggah ke Google Drive", "progress": 40,
        }),
    );
    let (status, _, body) = call(
        &state,
        Method::DELETE,
        "/api/app-settings/backups/google-drive/jobs/uji-agd-job-jalan",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Permintaan pembatalan upload dikirim");
    assert_eq!(body["data"]["status"], "running");
    assert_eq!(body["data"]["cancel_requested"], true);

    // Job yang sudah selesai dibiarkan apa adanya.
    write_job_file(
        &private,
        "uji-agd-job-selesai",
        &json!({
            "job_id": "uji-agd-job-selesai", "status": "completed", "filename": BACKUP,
        }),
    );
    let (status, _, body) = call(
        &state,
        Method::DELETE,
        "/api/app-settings/backups/google-drive/jobs/uji-agd-job-selesai",
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "completed");
    assert!(body["data"].get("cancel_requested").is_none());

    cleanup(&pool, &[]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "butuh DATABASE_URL"]
async fn gdrive_inflight_cancel_stops_before_next_chunk() {
    let _guard = LOCK.lock().await;
    // PUT pertama ditahan 1,5 detik agar pembatalan masuk saat chunk pertama sedang dikirim.
    let (stub, log) = start_stub(Duration::from_millis(1500)).await;
    let private = setup_env(&stub);
    let pool = pool().await;
    let state = AppState::new(pool.clone(), "http://localhost".to_string());
    let admin = user_token(&pool, ADMIN, true).await;
    write_backup(&private);
    write_creds(
        &private,
        &creds_connected(
            REFRESH,
            "ya29.uji-agd-2",
            "2999-01-01T00:00:00+00:00",
            Some(FOLDER_ID),
        ),
    );

    let uri = format!("/api/app-settings/backups/{BACKUP}/google-drive");
    let (status, _, body) = call(&state, Method::POST, &uri, Some(&admin), &[]).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();

    wait_job(&state, &admin, &job_id, &["running"]).await;
    let (status, _, body) = call(
        &state,
        Method::DELETE,
        &format!("/api/app-settings/backups/google-drive/jobs/{job_id}"),
        Some(&admin),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["cancel_requested"], true);

    let cancelled = wait_job(&state, &admin, &job_id, &["cancelled"]).await;
    assert_eq!(cancelled["message"], "Upload ke Google Drive dibatalkan");
    assert_eq!(
        log.puts.lock().unwrap().len(),
        1,
        "chunk kedua tidak boleh dikirim"
    );

    cleanup(&pool, &[]).await;
}
