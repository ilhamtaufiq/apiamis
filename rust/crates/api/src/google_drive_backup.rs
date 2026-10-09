//! Backup ke Google Drive: `GoogleDriveBackupController` dan `GoogleDriveBackupService` di Laravel.
//!
//! Rute (di bawah `/api/app-settings/backups`):
//! - `GET google-drive/status`, `GET google-drive/connect`, `DELETE google-drive`: admin.
//! - `GET google-drive/callback`: publik, dipanggil browser dari Google. Throttle 20 per menit per IP.
//!   Selalu redirect 302 ke `{frontend}/settings?google_drive=connected` atau `...=error&google_drive_message=`.
//! - `GET google-drive/jobs/{jobId}` (status) dan `DELETE google-drive/jobs/{jobId}` (batal): admin.
//! - `POST {filename}/google-drive`: admin. Menjawab 202, unggahan berjalan di task latar.
//!
//! Penyimpanan memakai lokasi dan format yang sama dengan Laravel, sehingga kedua sisi bisa membaca
//! berkas yang ditulis masing-masing:
//! - kredensial `google-drive/credentials.json` di disk private, dienkripsi `APP_KEY` (`crypt`);
//! - status job `google-drive-upload-jobs/{jobId}.json`;
//! - state OAuth di tabel `cache` (`google_drive_oauth_state:{state}`, 15 menit, sekali pakai).
//!
//! Perbedaan dengan Laravel yang diketahui:
//! - Upload berjalan sebagai task tokio di proses API, bukan proses `artisan backup:upload-drive`.
//!   Batal memberi `cancel_requested` yang dicek sebelum setiap chunk. Job yang dimulai Rust tidak
//!   mencatat PID, jadi `kill` hanya dikirim untuk job yang dicatat worker Laravel.
//! - Pesan error dari Google (Guzzle di Laravel) diganti pesan ringkas yang memuat status HTTP.
//! - `ensureBackupFolder` membaca ulang kredensial sebelum menulis `folder_id`. Laravel menulis salinan
//!   lama dan bisa menimpa token yang baru diperbarui.
//! - Endpoint Google bisa diganti lewat env untuk tes (default = Google sungguhan):
//!   `GOOGLE_OAUTH_TOKEN_URL` (tukar kode), `GOOGLE_OAUTH_REFRESH_URL` (refresh token),
//!   `GOOGLE_API_BASE_URL` (userinfo, Drive, dan upload). Redirect URI memakai `GOOGLE_DRIVE_REDIRECT_URI`.

use std::{
    collections::HashMap,
    io::SeekFrom,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use axum::{
    extract::{Path as UrlPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySqlPool;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    app_settings::{backup_dir, guard_filename, guard_job_id, private_root},
    auth_oauth::{cache_pull, cache_put, client_ip, form_encode, http_client, random_alnum, Php},
    crypt, mailer,
    notifications::require_admin,
    notify::new_uuid,
    require_auth, AppState,
};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
const DEFAULT_TOKEN_URL: &str = "https://www.googleapis.com/oauth2/v4/token";
const DEFAULT_REFRESH_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_API_BASE: &str = "https://www.googleapis.com";
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const FOLDER_NAME: &str = "Arumanis Backups";
const CREDENTIALS_DIR: &str = "google-drive";
const CREDENTIALS_FILE: &str = "credentials.json";
const JOB_DIR: &str = "google-drive-upload-jobs";
/// Harus kelipatan 256 KiB untuk resumable upload Drive.
const CHUNK_BYTES: usize = 8 * 1024 * 1024;
const STATE_PREFIX: &str = "google_drive_oauth_state:";
const STATE_TTL_SECS: i64 = 15 * 60;
const STATE_LEN: usize = 40;
const CALLBACK_MAX_PER_MINUTE: usize = 20;
const CONNECT_MISSING: &str = "Google OAuth belum dikonfigurasi (GOOGLE_CLIENT_ID / SECRET)";
const NOT_CONNECTED: &str = "Google Drive belum terhubung";
const CONNECT_FIRST: &str = "Google Drive belum terhubung. Hubungkan dulu di Pengaturan.";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn zip_route(name: &str) -> Result<(), ApiError> {
    if name.ends_with(".zip") {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

async fn require_admin_user(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let user = require_auth(state, headers).await?;
    require_admin(&state.pool, user.user_id).await
}

// ---------------------------------------------------------------------------
// Konfigurasi dan waktu
// ---------------------------------------------------------------------------

/// `env()` yang dipangkas; kosong dianggap tidak ada.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn non_blank(s: &str) -> bool {
    !s.trim().is_empty()
}

/// `filled()` Laravel untuk nilai JSON berupa string.
fn value_filled(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str).is_some_and(non_blank)
}

fn str_of(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| non_blank(s))
        .map(str::to_string)
}

fn loose_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn iso_format(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

fn now_iso() -> String {
    iso_format(Utc::now())
}

fn iso_in(secs: i64) -> String {
    iso_format(Utc::now() + chrono::Duration::seconds(secs))
}

fn configured() -> bool {
    env_nonempty("GOOGLE_CLIENT_ID").is_some() && env_nonempty("GOOGLE_CLIENT_SECRET").is_some()
}

fn redirect_uri(app_url: &str) -> String {
    env_nonempty("GOOGLE_DRIVE_REDIRECT_URI").unwrap_or_else(|| {
        format!(
            "{}/api/app-settings/backups/google-drive/callback",
            app_url.trim_end_matches('/')
        )
    })
}

struct Endpoints {
    token: String,
    refresh: String,
    userinfo: String,
    drive_files: String,
    upload_files: String,
}

fn endpoints() -> Endpoints {
    let base = env_nonempty("GOOGLE_API_BASE_URL").unwrap_or_else(|| DEFAULT_API_BASE.to_string());
    let base = base.trim_end_matches('/');
    Endpoints {
        token: env_nonempty("GOOGLE_OAUTH_TOKEN_URL").unwrap_or_else(|| DEFAULT_TOKEN_URL.into()),
        refresh: env_nonempty("GOOGLE_OAUTH_REFRESH_URL")
            .unwrap_or_else(|| DEFAULT_REFRESH_URL.into()),
        userinfo: format!("{base}/oauth2/v3/userinfo"),
        drive_files: format!("{base}/drive/v3/files"),
        upload_files: format!("{base}/upload/drive/v3/files"),
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// Body `application/x-www-form-urlencoded` seperti `http_build_query` PHP.
fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `error.message` dari JSON Google, atau teks mentah bila bukan JSON seperti itu.
fn google_error_text(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| str_of(v.get("error").and_then(|e| e.get("message"))))
        .unwrap_or_else(|| text.to_string())
}

// ---------------------------------------------------------------------------
// Kredensial terenkripsi dan status job (disk private)
// ---------------------------------------------------------------------------

fn credentials_path() -> PathBuf {
    private_root().join(CREDENTIALS_DIR).join(CREDENTIALS_FILE)
}

fn job_path(job_id: &str) -> PathBuf {
    private_root().join(JOB_DIR).join(format!("{job_id}.json"))
}

fn app_key() -> Result<Vec<u8>, String> {
    let raw = std::env::var("APP_KEY").map_err(|_| "APP_KEY belum di-set".to_string())?;
    crypt::key_from_app_key(&raw).map_err(|e| format!("APP_KEY tidak valid: {e:?}"))
}

/// `readCredentials`: berkas hilang atau tidak bisa didekripsi berarti belum terhubung.
async fn read_credentials() -> Map<String, Value> {
    let Ok(payload) = tokio::fs::read_to_string(credentials_path()).await else {
        return Map::new();
    };
    let decoded = app_key()
        .and_then(|key| crypt::decrypt_string(&key, payload.trim()).map_err(|e| format!("{e:?}")));
    match decoded.map(|json| serde_json::from_str::<Value>(&json)) {
        Ok(Ok(Value::Object(map))) => map,
        Ok(_) => Map::new(),
        Err(reason) => {
            tracing::warn!(reason = %reason, "Google Drive credentials tidak bisa dibaca");
            Map::new()
        }
    }
}

async fn write_credentials(data: &Map<String, Value>) -> Result<(), String> {
    let key = app_key()?;
    let json = serde_json::to_string(data).map_err(|e| e.to_string())?;
    let payload = crypt::encrypt_string(&key, &json);
    tokio::fs::create_dir_all(private_root().join(CREDENTIALS_DIR))
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(credentials_path(), payload)
        .await
        .map_err(|e| e.to_string())
}

async fn read_job(job_id: &str) -> Option<Value> {
    let bytes = tokio::fs::read(job_path(job_id)).await.ok()?;
    match serde_json::from_slice::<Value>(&bytes).ok()? {
        v @ Value::Object(_) => Some(v),
        _ => None,
    }
}

async fn write_job(job_id: &str, status: &Value) -> Result<(), String> {
    tokio::fs::create_dir_all(private_root().join(JOB_DIR))
        .await
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(status).map_err(|e| e.to_string())?;
    tokio::fs::write(job_path(job_id), bytes)
        .await
        .map_err(|e| e.to_string())
}

async fn is_cancel_requested(job_id: &str) -> bool {
    read_job(job_id)
        .await
        .and_then(|j| j.get("cancel_requested").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn status_payload(creds: &Map<String, Value>) -> Value {
    json!({
        "configured": configured(),
        "connected": value_filled(creds.get("refresh_token")),
        "email": creds.get("email").cloned().unwrap_or(Value::Null),
        "folder_id": creds.get("folder_id").cloned().unwrap_or(Value::Null),
        "folder_name": FOLDER_NAME,
        "connected_at": creds.get("connected_at").cloned().unwrap_or(Value::Null),
    })
}

// ---------------------------------------------------------------------------
// Rute admin: status, connect, disconnect
// ---------------------------------------------------------------------------

/// `GET /api/app-settings/backups/google-drive/status`: admin.
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin_user(&state, &headers).await?;
    let creds = read_credentials().await;
    Ok(Json(json!({ "data": status_payload(&creds) })))
}

/// `GET /api/app-settings/backups/google-drive/connect`: admin. Menyimpan state 15 menit, lalu
/// mengembalikan URL OAuth Google dengan scope Drive `drive.file`.
pub async fn connect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin_user(&state, &headers).await?;
    if !configured() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            CONNECT_MISSING,
        ));
    }

    let token = random_alnum(STATE_LEN);
    let value = Php::arr(vec![("created_at", Php::Str(now_iso()))]);
    cache_put(
        &state.pool,
        &format!("{STATE_PREFIX}{token}"),
        &value,
        STATE_TTL_SECS,
    )
    .await
    .map_err(internal)?;

    let client_id = env_nonempty("GOOGLE_CLIENT_ID").unwrap_or_default();
    // Urutan parameter mengikuti Socialite: scope default (openid profile email) lalu scope Drive,
    // kemudian access_type, prompt, dan state dari `with()`.
    let scope = ["openid", "profile", "email", DRIVE_SCOPE].join(" ");
    let redirect = redirect_uri(&state.app_url);
    let url = format!(
        "{GOOGLE_AUTH_URL}?{}",
        form_body(&[
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("scope", scope.as_str()),
            ("response_type", "code"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", token.as_str()),
        ])
    );
    Ok(Json(json!({ "data": { "url": url } })))
}

/// `DELETE /api/app-settings/backups/google-drive`: admin. Menghapus kredensial di disk.
pub async fn disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin_user(&state, &headers).await?;
    match tokio::fs::remove_file(credentials_path()).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(internal(e)),
    }
    let creds = read_credentials().await;
    Ok(Json(json!({
        "message": "Koneksi Google Drive diputus",
        "data": status_payload(&creds),
    })))
}

// ---------------------------------------------------------------------------
// Callback OAuth (publik)
// ---------------------------------------------------------------------------

/// `GET /api/app-settings/backups/google-drive/callback`: publik, throttle 20 per menit per IP.
/// Selalu redirect 302 ke frontend, termasuk saat gagal (pesan dikirim lewat query).
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let key = format!("gdrive-callback:{}", client_ip(&headers));
    if let Err(retry_secs) =
        state
            .limiter
            .hit(&key, CALLBACK_MAX_PER_MINUTE, Duration::from_secs(60))
    {
        return too_many_attempts(retry_secs);
    }

    let location = match handle_callback(&state, &query).await {
        Ok(()) => frontend_return_url(&state.pool, &state.app_url, "connected", None).await,
        Err(message) => {
            tracing::warn!(message = %message, "Google Drive OAuth callback failed");
            frontend_return_url(&state.pool, &state.app_url, "error", Some(&message)).await
        }
    };
    redirect_to(&location)
}

/// Inti `callback` + `GoogleDriveBackupService::handleCallback`. Pesan Err ditampilkan ke frontend.
async fn handle_callback(state: &AppState, query: &HashMap<String, String>) -> Result<(), String> {
    if let Some(error) = query.get("error").filter(|e| non_blank(e)) {
        return Err(error.clone());
    }
    let code = query.get("code").cloned().unwrap_or_default();
    if code.is_empty() {
        return Err("Kode OAuth tidak ada".into());
    }
    if !configured() {
        return Err("Google OAuth belum dikonfigurasi".into());
    }

    // State sekali pakai: `Cache::pull`, dan state kosong atau kedaluwarsa ditolak.
    let state_ok = match query.get("state").filter(|s| non_blank(s)) {
        Some(s) => cache_pull(&state.pool, &format!("{STATE_PREFIX}{s}"))
            .await
            .map_err(|e| e.to_string())?
            .is_some(),
        None => false,
    };
    if !state_ok {
        return Err("State OAuth Google Drive tidak valid atau kedaluwarsa".into());
    }

    let client_id = env_nonempty("GOOGLE_CLIENT_ID").unwrap_or_default();
    let client_secret = env_nonempty("GOOGLE_CLIENT_SECRET").unwrap_or_default();
    let redirect = redirect_uri(&state.app_url);
    let ep = endpoints();
    let client = http_client();

    // Tukar kode (Socialite `getAccessTokenResponse`).
    let res = client
        .post(&ep.token)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form_body(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
        ]))
        .send()
        .await
        .map_err(|_| "Permintaan token Google gagal".to_string())?;
    if !res.status().is_success() {
        return Err(format!(
            "Google menolak penukaran kode (HTTP {})",
            res.status().as_u16()
        ));
    }
    let token_body: Value = res
        .json()
        .await
        .map_err(|_| "Respons token Google tidak valid".to_string())?;
    let access = str_of(token_body.get("access_token"))
        .ok_or_else(|| "Google tidak mengembalikan access token".to_string())?;

    // Profil dari userinfo (Socialite `getUserByToken`). Yang dipakai hanya email.
    let res = client
        .get(format!("{}?prettyPrint=false", ep.userinfo))
        .header(header::ACCEPT, "application/json")
        .header(header::AUTHORIZATION, bearer(&access))
        .send()
        .await
        .map_err(|_| "Permintaan userinfo Google gagal".to_string())?;
    if !res.status().is_success() {
        return Err(format!(
            "userinfo Google gagal (HTTP {})",
            res.status().as_u16()
        ));
    }
    let userinfo: Value = res
        .json()
        .await
        .map_err(|_| "Respons userinfo Google tidak valid".to_string())?;
    let email = str_of(userinfo.get("email"));

    let expires_in = token_body
        .get("expires_in")
        .and_then(loose_i64)
        .unwrap_or(3600);

    // Refresh token hanya dikirim Google saat consent pertama. Bila kosong, pakai yang tersimpan.
    let refresh = match str_of(token_body.get("refresh_token")) {
        Some(r) => r,
        None => match str_of(read_credentials().await.get("refresh_token")) {
            Some(r) => r,
            None => {
                return Err("Google tidak mengembalikan refresh token. Cabut akses aplikasi di https://myaccount.google.com/permissions lalu hubungkan ulang.".into())
            }
        },
    };

    let mut creds = Map::new();
    creds.insert("refresh_token".into(), json!(refresh));
    creds.insert("access_token".into(), json!(access));
    creds.insert(
        "access_token_expires_at".into(),
        json!(iso_in(std::cmp::max(60, expires_in - 60))),
    );
    creds.insert("email".into(), json!(email));
    creds.insert("folder_id".into(), Value::Null);
    creds.insert("connected_at".into(), json!(now_iso()));
    write_credentials(&creds).await?;

    // Folder dibuat agar status langsung siap. Kegagalan di sini tidak membatalkan koneksi.
    if let Err(e) = ensure_backup_folder().await {
        tracing::warn!(message = %e, "Google Drive folder ensure failed after connect");
    }
    Ok(())
}

/// `frontendReturnUrl`: `{FRONTEND_URL}/settings?google_drive=...`.
async fn frontend_return_url(
    pool: &MySqlPool,
    app_url: &str,
    status: &str,
    message: Option<&str>,
) -> String {
    let from_setting = mailer::setting(pool, "frontend_url")
        .await
        .ok()
        .flatten()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let base = from_setting
        .or_else(|| env_nonempty("FRONTEND_URL"))
        .or_else(|| env_nonempty("APP_URL"))
        .unwrap_or_else(|| app_url.to_string());
    let base = base.trim_end_matches('/');

    let mut query = vec![format!("google_drive={}", form_encode(status))];
    if let Some(m) = message.filter(|m| !m.is_empty()) {
        query.push(format!("google_drive_message={}", form_encode(m)));
    }
    format!("{base}/settings?{}", query.join("&"))
}

fn redirect_to(url: &str) -> Response {
    match HeaderValue::from_str(url) {
        Ok(value) => (StatusCode::FOUND, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn too_many_attempts(retry_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_secs.to_string())],
        Json(json!({ "message": "Too Many Attempts." })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Folder dan access token
// ---------------------------------------------------------------------------

/// `ensureBackupFolder`: pakai `folder_id` yang tersimpan, cari folder Drive dengan nama yang sama,
/// atau buat baru.
async fn ensure_backup_folder() -> Result<String, String> {
    let creds = read_credentials().await;
    if let Some(id) = str_of(creds.get("folder_id")) {
        return Ok(id);
    }

    let ep = endpoints();
    let access = get_access_token(false).await?;
    let query = format!(
        "name = '{FOLDER_NAME}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false"
    );
    let list = http_client()
        .get(&ep.drive_files)
        .header(header::AUTHORIZATION, bearer(&access))
        .query(&[
            ("q", query.as_str()),
            ("spaces", "drive"),
            ("fields", "files(id,name)"),
            ("pageSize", "1"),
        ])
        .send()
        .await
        .map_err(|_| "Gagal mencari folder Drive".to_string())?;
    if list.status().is_success() {
        let body: Value = list.json().await.unwrap_or(Value::Null);
        let found = body
            .get("files")
            .and_then(Value::as_array)
            .and_then(|files| files.first())
            .and_then(|f| str_of(f.get("id")));
        if let Some(id) = found {
            save_folder_id(&id).await?;
            return Ok(id);
        }
    }

    let create = http_client()
        .post(&ep.drive_files)
        .header(header::AUTHORIZATION, bearer(&access))
        .header(header::CONTENT_TYPE, "application/json")
        .body(
            json!({ "name": FOLDER_NAME, "mimeType": "application/vnd.google-apps.folder" })
                .to_string(),
        )
        .send()
        .await
        .map_err(|_| "Gagal membuat folder Drive".to_string())?;
    let ok = create.status().is_success();
    let text = create.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    match (ok, str_of(body.get("id"))) {
        (true, Some(id)) => {
            save_folder_id(&id).await?;
            Ok(id)
        }
        _ => Err(format!(
            "Gagal membuat folder Drive: {}",
            google_error_text(&text)
        )),
    }
}

async fn save_folder_id(id: &str) -> Result<(), String> {
    let mut creds = read_credentials().await;
    creds.insert("folder_id".into(), json!(id));
    write_credentials(&creds).await
}

/// `getAccessToken`: pakai access token tersimpan bila belum kedaluwarsa, selain itu refresh.
async fn get_access_token(force: bool) -> Result<String, String> {
    let mut creds = read_credentials().await;
    let refresh = str_of(creds.get("refresh_token")).ok_or_else(|| NOT_CONNECTED.to_string())?;

    if !force {
        let valid_until = creds
            .get("access_token_expires_at")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        if let (Some(access), Some(until)) = (str_of(creds.get("access_token")), valid_until) {
            if Utc::now() < until {
                return Ok(access);
            }
        }
    }

    let ep = endpoints();
    let client_id = env_nonempty("GOOGLE_CLIENT_ID").unwrap_or_default();
    let client_secret = env_nonempty("GOOGLE_CLIENT_SECRET").unwrap_or_default();
    let res = http_client()
        .post(&ep.refresh)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form_body(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ]))
        .send()
        .await
        .map_err(|_| "Gagal refresh token Google Drive. Hubungkan ulang akun. ".to_string())?;
    let ok = res.status().is_success();
    let text = res.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let access = match (ok, str_of(body.get("access_token"))) {
        (true, Some(a)) => a,
        _ => {
            let reason = str_of(body.get("error_description"))
                .or_else(|| str_of(body.get("error")))
                .unwrap_or_default();
            return Err(format!(
                "Gagal refresh token Google Drive. Hubungkan ulang akun. {reason}"
            ));
        }
    };

    let expires_in = body.get("expires_in").and_then(loose_i64).unwrap_or(3600);
    creds.insert("access_token".into(), json!(access));
    creds.insert(
        "access_token_expires_at".into(),
        json!(iso_in(std::cmp::max(60, expires_in - 60))),
    );
    // Google kadang memutar refresh token.
    if let Some(rotated) = str_of(body.get("refresh_token")) {
        creds.insert("refresh_token".into(), json!(rotated));
    }
    write_credentials(&creds).await?;
    Ok(access)
}

// ---------------------------------------------------------------------------
// Upload job
// ---------------------------------------------------------------------------

/// `POST /api/app-settings/backups/{filename}/google-drive`: admin. 202 dan job berjalan di latar.
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(filename): UrlPath<String>,
) -> Result<Response, ApiError> {
    zip_route(&filename)?;
    require_admin_user(&state, &headers).await?;
    guard_filename(&filename)?;

    let absolute = backup_dir().join(&filename);
    let size = tokio::fs::metadata(&absolute)
        .await
        .map(|m| m.len())
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "Backup tidak ditemukan"))?;

    let creds = read_credentials().await;
    if !value_filled(creds.get("refresh_token")) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            CONNECT_FIRST,
        ));
    }

    let job_id = new_uuid();
    let queued = json!({
        "job_id": job_id,
        "status": "queued",
        "filename": filename,
        "size": size,
        "created_at": now_iso(),
        "message": "Upload ke Google Drive masuk antrean",
        "progress": 0,
    });
    write_job(&job_id, &queued).await.map_err(internal)?;
    tokio::spawn(run_upload_job(job_id.clone(), filename.clone()));

    let data = read_job(&job_id).await.unwrap_or(queued);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "data": data,
            "message": "Upload ke Google Drive sedang diproses",
        })),
    )
        .into_response())
}

/// `GET /api/app-settings/backups/google-drive/jobs/{jobId}`: admin.
pub async fn show_upload_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(job_id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin_user(&state, &headers).await?;
    guard_job_id(&job_id)?;
    let status = read_job(&job_id)
        .await
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Status upload tidak ditemukan"))?;
    Ok(Json(json!({ "data": status })))
}

/// `DELETE /api/app-settings/backups/google-drive/jobs/{jobId}`: admin. Menandai `cancel_requested`
/// bila job belum selesai. Job Laravel yang mencatat PID dihentikan dengan sinyal.
pub async fn cancel_upload_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(job_id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    require_admin_user(&state, &headers).await?;
    guard_job_id(&job_id)?;
    let status = read_job(&job_id)
        .await
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Status upload tidak ditemukan"))?;

    let current = if matches!(
        status.get("status").and_then(Value::as_str).unwrap_or(""),
        "completed" | "failed" | "cancelled"
    ) {
        status
    } else {
        let mut next = status.as_object().cloned().unwrap_or_default();
        next.insert("cancel_requested".into(), json!(true));
        next.insert("cancel_requested_at".into(), json!(now_iso()));
        next.insert("message".into(), json!("Membatalkan upload…"));
        let next = Value::Object(next);
        write_job(&job_id, &next).await.map_err(internal)?;
        terminate_pid(status.get("pid").and_then(loose_i64)).await;
        read_job(&job_id).await.unwrap_or(next)
    };

    let message = if current.get("status").and_then(Value::as_str) == Some("cancelled") {
        "Upload ke Google Drive dibatalkan"
    } else {
        "Permintaan pembatalan upload dikirim"
    };
    Ok(Json(json!({ "data": current, "message": message })))
}

/// `SystemBackupService::terminateProcessPid`: TERM, lalu KILL bila masih hidup setelah 300 ms.
/// PID sendiri tidak pernah disentuh.
async fn terminate_pid(pid: Option<i64>) {
    let Some(pid) = pid.filter(|p| *p > 0) else {
        return;
    };
    if pid == i64::from(std::process::id()) {
        return;
    }
    let signal = |sig: &str| {
        std::process::Command::new("kill")
            .arg(sig)
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    let _ = signal("-TERM");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let alive = std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if alive {
        let _ = signal("-KILL");
    }
}

/// Galat job: `Cancelled` memakai status batal, `Msg` menjadi status `failed`.
enum Fail {
    Cancelled,
    Msg(String),
}

async fn check_cancel(job_id: &str) -> Result<(), Fail> {
    if is_cancel_requested(job_id).await {
        Err(Fail::Cancelled)
    } else {
        Ok(())
    }
}

async fn finalize_cancelled(job_id: &str, filename: &str, previous: &Value) {
    let next = json!({
        "job_id": job_id,
        "status": "cancelled",
        "filename": filename,
        "size": previous.get("size").cloned().unwrap_or(Value::Null),
        "created_at": previous.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
        "started_at": previous.get("started_at").cloned().unwrap_or(Value::Null),
        "finished_at": now_iso(),
        "message": "Upload ke Google Drive dibatalkan",
        "progress": 0,
        "cancel_requested": true,
    });
    if let Err(e) = write_job(job_id, &next).await {
        tracing::error!(job_id, message = %e, "gagal menulis status batal upload Drive");
    }
}

/// `runUploadJob`: status running, lalu upload, lalu completed, cancelled, atau failed.
async fn run_upload_job(job_id: String, filename: String) {
    let absolute = backup_dir().join(&filename);
    let initial = read_job(&job_id).await.unwrap_or_else(|| json!({}));
    if is_cancel_requested(&job_id).await {
        finalize_cancelled(&job_id, &filename, &initial).await;
        return;
    }

    let size = tokio::fs::metadata(&absolute)
        .await
        .map(|m| m.len())
        .ok()
        .or_else(|| initial.get("size").and_then(loose_i64).map(|s| s as u64))
        .unwrap_or(0);
    let running = json!({
        "job_id": job_id,
        "status": "running",
        "filename": filename,
        "size": size,
        "created_at": initial.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
        "started_at": now_iso(),
        "message": "Mengunggah ke Google Drive",
        "progress": 1,
    });
    if let Err(e) = write_job(&job_id, &running).await {
        tracing::error!(job_id = %job_id, message = %e, "gagal menulis status upload Drive");
        return;
    }

    match upload_file(&absolute, &filename, &job_id).await {
        Ok(result) => {
            let before = read_job(&job_id).await.unwrap_or_else(|| json!({}));
            let completed = json!({
                "job_id": job_id,
                "status": "completed",
                "filename": filename,
                "size": tokio::fs::metadata(&absolute).await.map(|m| m.len()).unwrap_or(size),
                "created_at": before.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
                "started_at": before.get("started_at").cloned().unwrap_or(Value::Null),
                "finished_at": now_iso(),
                "message": "Upload ke Google Drive berhasil",
                "progress": 100,
                "result": result,
            });
            if let Err(e) = write_job(&job_id, &completed).await {
                tracing::error!(job_id = %job_id, message = %e, "gagal menulis status selesai");
            }
        }
        Err(Fail::Cancelled) => {
            let before = read_job(&job_id).await.unwrap_or_else(|| json!({}));
            finalize_cancelled(&job_id, &filename, &before).await;
        }
        Err(Fail::Msg(message)) => {
            if is_cancel_requested(&job_id).await {
                let before = read_job(&job_id).await.unwrap_or_else(|| json!({}));
                finalize_cancelled(&job_id, &filename, &before).await;
                return;
            }
            tracing::error!(job_id = %job_id, filename = %filename, error = %message, "Google Drive upload job failed");
            let before = read_job(&job_id).await.unwrap_or_else(|| json!({}));
            let failed = json!({
                "job_id": job_id,
                "status": "failed",
                "filename": filename,
                "created_at": before.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
                "started_at": before.get("started_at").cloned().unwrap_or(Value::Null),
                "finished_at": now_iso(),
                "message": "Upload ke Google Drive gagal",
                "progress": 0,
                "error": message,
            });
            if let Err(e) = write_job(&job_id, &failed).await {
                tracing::error!(job_id = %job_id, message = %e, "gagal menulis status gagal");
            }
        }
    }
}

async fn patch_progress(job_id: &str, progress: i64, message: String) -> Result<(), Fail> {
    check_cancel(job_id).await?;
    let mut current = read_job(job_id)
        .await
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    current.insert("progress".into(), json!(progress));
    current.insert("message".into(), json!(message));
    let status = current
        .get("status")
        .cloned()
        .unwrap_or_else(|| json!("running"));
    current.insert("status".into(), status);
    write_job(job_id, &Value::Object(current))
        .await
        .map_err(Fail::Msg)
}

fn file_web_link(file_id: &str) -> String {
    format!("https://drive.google.com/file/d/{file_id}/view")
}

/// `uploadFileResumable`: sesi resumable, lalu chunk 8 MiB dengan `Content-Range`.
async fn upload_file(absolute: &Path, filename: &str, job_id: &str) -> Result<Value, Fail> {
    let file_err = || Fail::Msg("File backup tidak ditemukan di server".to_string());
    let size = tokio::fs::metadata(absolute)
        .await
        .map_err(|_| file_err())?
        .len();

    let ep = endpoints();
    let folder_id = ensure_backup_folder().await.map_err(Fail::Msg)?;
    let access = get_access_token(false).await.map_err(Fail::Msg)?;

    let init = http_client()
        .post(format!(
            "{}?uploadType=resumable&fields=id,name,webViewLink,size",
            ep.upload_files
        ))
        .header(header::AUTHORIZATION, bearer(&access))
        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
        .header("X-Upload-Content-Type", "application/zip")
        .header("X-Upload-Content-Length", size.to_string())
        .body(json!({ "name": filename, "parents": [folder_id] }).to_string())
        .send()
        .await
        .map_err(|_| Fail::Msg("Gagal memulai upload Drive".to_string()))?;
    if !init.status().is_success() {
        let text = init.text().await.unwrap_or_default();
        return Err(Fail::Msg(format!(
            "Gagal memulai upload Drive: {}",
            google_error_text(&text)
        )));
    }
    let session_uri = init
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .filter(|v| non_blank(v))
        .map(str::to_string)
        .ok_or_else(|| Fail::Msg("Google Drive tidak mengembalikan session upload".to_string()))?;

    let mut file = tokio::fs::File::open(absolute)
        .await
        .map_err(|_| Fail::Msg("Gagal membuka file backup untuk diunggah".to_string()))?;
    let mut offset: u64 = 0;
    let mut auth_retries = 0u8;
    let mut final_body: Option<Value> = None;

    while offset < size {
        check_cancel(job_id).await?;

        let len = std::cmp::min(CHUNK_BYTES as u64, size - offset) as usize;
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|_| Fail::Msg("Gagal membaca chunk file backup".to_string()))?;
        let mut chunk = vec![0u8; len];
        file.read_exact(&mut chunk)
            .await
            .map_err(|_| Fail::Msg("Gagal membaca chunk file backup".to_string()))?;
        let end = offset + len as u64 - 1;

        let res = http_client()
            .put(&session_uri)
            .timeout(Duration::from_secs(600))
            .header(header::CONTENT_TYPE, "application/zip")
            .header(
                header::CONTENT_RANGE,
                format!("bytes {offset}-{end}/{size}"),
            )
            .body(chunk)
            .send()
            .await
            .map_err(|_| Fail::Msg("Upload Drive gagal: koneksi terputus".to_string()))?;
        let code = res.status();

        // 308 Resume Incomplete: lanjut ke chunk berikutnya.
        if code.as_u16() == 308 {
            offset = end + 1;
            auth_retries = 0;
            let progress = ((offset as f64 / size.max(1) as f64) * 100.0)
                .floor()
                .clamp(1.0, 99.0) as i64;
            patch_progress(job_id, progress, format!("Mengunggah… {progress}%")).await?;
            continue;
        }
        if code.is_success() {
            final_body = res.json().await.ok();
            break;
        }
        // Token bisa kedaluwarsa di tengah upload: refresh lalu ulangi chunk yang sama.
        if matches!(code.as_u16(), 401 | 403) && auth_retries < 2 {
            auth_retries += 1;
            get_access_token(true).await.map_err(Fail::Msg)?;
            continue;
        }
        let text = res.text().await.unwrap_or_default();
        return Err(Fail::Msg(format!(
            "Upload Drive gagal (HTTP {}): {}",
            code.as_u16(),
            google_error_text(&text)
        )));
    }

    let body = final_body
        .filter(|v| v.is_object() && str_of(v.get("id")).is_some())
        .ok_or_else(|| Fail::Msg("Upload Drive selesai tanpa metadata file".to_string()))?;
    let id = str_of(body.get("id")).unwrap_or_default();
    Ok(json!({
        "id": id,
        "name": str_of(body.get("name")).unwrap_or_else(|| filename.to_string()),
        "webViewLink": str_of(body.get("webViewLink")).unwrap_or_else(|| file_web_link(&id)),
        "size": body.get("size").and_then(loose_i64).unwrap_or(size as i64),
        "folder_id": folder_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_order_matches_socialite_merge() {
        // scopes() Socialite: default (openid, profile, email) digabung scope Drive, lalu unik.
        let scope = ["openid", "profile", "email", DRIVE_SCOPE].join(" ");
        assert_eq!(
            scope,
            "openid profile email https://www.googleapis.com/auth/drive.file"
        );
    }

    #[test]
    fn google_error_prefers_error_message() {
        assert_eq!(
            google_error_text(r#"{"error":{"message":"Kuota habis"}}"#),
            "Kuota habis"
        );
        assert_eq!(google_error_text("teks biasa"), "teks biasa");
    }

    #[test]
    fn loose_int_accepts_string_and_number() {
        assert_eq!(loose_i64(&json!(3599)), Some(3599));
        assert_eq!(loose_i64(&json!("12345")), Some(12345));
        assert_eq!(loose_i64(&json!(null)), None);
    }

    #[test]
    fn iso_strings_use_utc_offset_like_carbon() {
        let t = DateTime::parse_from_rfc3339("2026-10-09T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso_format(t), "2026-10-09T10:00:00+00:00");
    }

    #[test]
    fn form_body_encodes_like_http_build_query() {
        assert_eq!(
            form_body(&[("scope", "a b"), ("x", "https://y")]),
            "scope=a+b&x=https%3A%2F%2Fy"
        );
    }
}
