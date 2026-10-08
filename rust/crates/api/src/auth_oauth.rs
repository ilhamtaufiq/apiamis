//! Handoff token dan Google OAuth, setara `AuthController` di Laravel:
//! `createHandoff`, `exchangeHandoff`, `redirectToGoogle`, dan `handleGoogleCallback`.
//!
//! Rute:
//! - `POST /api/auth/handoff` (auth Bearer/cookie, throttle 10 per menit per user)
//! - `POST /api/auth/handoff/exchange` (throttle 20 per menit per IP)
//! - `GET /api/auth/google` (`platform`, `callback_url`)
//! - `GET /api/auth/google/callback` (`code`, `state`)
//!
//! Penyimpanan state dan kode handoff memakai tabel `cache` (`CACHE_STORE=database`
//! di Laravel). Nilainya memakai format PHP `serialize()` dan kuncinya diberi
//! `CACHE_PREFIX` (default `{slug(APP_NAME)}-cache-`), sehingga Laravel dan Rust
//! bisa saling membaca baris yang ditulis masing-masing. Konsumsi kode memakai
//! `SELECT ... FOR UPDATE` lalu `DELETE` dalam satu transaksi (sekali pakai).
//!
//! Perbedaan dengan Laravel yang diketahui:
//! - Token Google access token selalu diverifikasi lewat `userinfo`. Socialite
//!   juga bisa men-decode ID token JWT; jalur itu belum ada di sini.
//! - `GOOGLE_REDIRECT_URI` yang kosong jatuh ke `{APP_URL}/api/auth/google/callback`.
//!   Laravel mengirim `redirect_uri` kosong dalam kasus itu.
//! - `createHandoff` menerima token dari cookie sesi juga (`arumanis_session`).
//!   Laravel mengembalikan 400 "Bearer token required." jika tidak ada Bearer.
//! - Body `exchangeHandoff` hanya dibaca sebagai JSON. Form-urlencoded tidak didukung.
//! - Google `userinfo` tidak memuat `genders`, jadi `gender` selalu `NULL` seperti
//!   di Laravel (kolom itu ikut tertimpa `NULL` setiap login Google).
//! - Validasi `callback_url` memakai pemeriksaan URL sederhana, bukan
//!   `FILTER_VALIDATE_URL` PHP secara penuh.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rand::{distributions::Alphanumeric, Rng};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{desa::internal, maintenance, session, users, AppState};
use auth::login;

const HANDOFF_TTL_SECS: i64 = 60;
const HANDOFF_CODE_LEN: usize = 48;
const OAUTH_STATE_TTL_SECS: i64 = 600;
const OAUTH_STATE_LEN: usize = 40;
const HANDOFF_CREATE_MAX_PER_MINUTE: usize = 10;
const HANDOFF_EXCHANGE_MAX_PER_MINUTE: usize = 20;

/// Scope Socialite (`openid profile email`) ditambah scope gender, sama seperti Laravel.
const GOOGLE_SCOPES: &[&str] = &[
    "openid",
    "profile",
    "email",
    "https://www.googleapis.com/auth/user.gender.read",
];
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
const GOOGLE_TOKEN_URL: &str = "https://www.googleapis.com/oauth2/v4/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v3/userinfo";

const GOOGLE_FAILED_MESSAGE: &str = "Google authentication failed. Please try again.";
const MAINTENANCE_MESSAGE: &str = "Aplikasi sedang maintenance. Login ditutup sementara.";
const HANDOFF_GONE_MESSAGE: &str = "Handoff code invalid or expired.";
const DEFAULT_FRONTEND_URL: &str = "http://arumanis.test";
const DEFAULT_MOBILE_CALLBACK: &str = "pengawas://oauth-callback";

// ---------------------------------------------------------------------------
// Nilai cache dalam format PHP serialize()
// ---------------------------------------------------------------------------

/// Subset nilai PHP yang dipakai Laravel untuk cache ini.
#[derive(Debug, Clone, PartialEq)]
pub enum Php {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<(String, Php)>),
}

impl Php {
    fn arr(items: Vec<(&str, Php)>) -> Php {
        Php::Arr(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn get(&self, key: &str) -> Option<&Php> {
        match self {
            Php::Arr(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Php::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// `serialize()` PHP untuk nilai di atas.
pub fn php_serialize(value: &Php) -> String {
    match value {
        Php::Null => "N;".to_string(),
        Php::Bool(b) => format!("b:{};", u8::from(*b)),
        Php::Int(i) => format!("i:{i};"),
        Php::Str(s) => format!("s:{}:\"{}\";", s.len(), s),
        Php::Arr(items) => {
            let mut out = format!("a:{}:{{", items.len());
            for (k, v) in items {
                out.push_str(&php_serialize(&Php::Str(k.clone())));
                out.push_str(&php_serialize(v));
            }
            out.push('}');
            out
        }
    }
}

/// `unserialize()` PHP untuk nilai yang sama. `None` bila format tidak dikenal.
pub fn php_unserialize(input: &str) -> Option<Php> {
    let mut p = PhpReader {
        s: input.as_bytes(),
        i: 0,
    };
    p.value()
}

struct PhpReader<'a> {
    s: &'a [u8],
    i: usize,
}

impl PhpReader<'_> {
    fn expect(&mut self, lit: &[u8]) -> Option<()> {
        if self.s.get(self.i..self.i + lit.len())? == lit {
            self.i += lit.len();
            Some(())
        } else {
            None
        }
    }

    /// Membaca sampai `delim` (dikonsumsi, tidak ikut dikembalikan).
    fn until(&mut self, delim: u8) -> Option<&str> {
        let start = self.i;
        let rel = self.s[start..].iter().position(|b| *b == delim)?;
        self.i = start + rel + 1;
        std::str::from_utf8(&self.s[start..start + rel]).ok()
    }

    fn value(&mut self) -> Option<Php> {
        let tag = *self.s.get(self.i)?;
        match tag {
            b'N' => {
                self.expect(b"N;")?;
                Some(Php::Null)
            }
            b'b' => {
                self.i += 2;
                let v = self.until(b';')?;
                Some(Php::Bool(v == "1"))
            }
            b'i' => {
                self.i += 2;
                let v = self.until(b';')?;
                Some(Php::Int(v.parse().ok()?))
            }
            b's' => {
                self.i += 2;
                let len: usize = self.until(b':')?.parse().ok()?;
                self.expect(b"\"")?;
                let bytes = self.s.get(self.i..self.i + len)?;
                let text = String::from_utf8(bytes.to_vec()).ok()?;
                self.i += len;
                self.expect(b"\";")?;
                Some(Php::Str(text))
            }
            b'a' => {
                self.i += 2;
                let n: usize = self.until(b':')?.parse().ok()?;
                self.expect(b"{")?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    let key = match self.value()? {
                        Php::Str(s) => s,
                        Php::Int(i) => i.to_string(),
                        _ => return None,
                    };
                    items.push((key, self.value()?));
                }
                self.expect(b"}")?;
                Some(Php::Arr(items))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Akses tabel cache (CACHE_STORE=database)
// ---------------------------------------------------------------------------

/// Prefix kunci cache seperti Laravel: `CACHE_PREFIX`, atau `{slug(APP_NAME)}-cache-`.
pub fn cache_prefix() -> String {
    match std::env::var("CACHE_PREFIX") {
        Ok(prefix) => prefix,
        Err(_) => format!("{}-cache-", slug(&env_or("APP_NAME", "laravel"))),
    }
}

/// Sebagian dari `Str::slug`: huruf kecil ASCII dan angka, pemisah `-`.
fn slug(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// `Cache::put`: upsert baris dengan kedaluwarsa `now + ttl`.
async fn cache_put(
    pool: &MySqlPool,
    key: &str,
    value: &Php,
    ttl_secs: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO `cache` (`key`, `value`, `expiration`) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE `value` = VALUES(`value`), `expiration` = VALUES(`expiration`)",
    )
    .bind(format!("{}{key}", cache_prefix()))
    .bind(php_serialize(value))
    .bind(unix_now() + ttl_secs)
    .execute(pool)
    .await?;
    Ok(())
}

/// `Cache::pull`: ambil lalu hapus dalam satu transaksi. Baris kedaluwarsa dihapus, hasilnya `None`.
async fn cache_pull(pool: &MySqlPool, key: &str) -> Result<Option<Php>, sqlx::Error> {
    let full = format!("{}{key}", cache_prefix());
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT `value`, CAST(`expiration` AS SIGNED) AS expiration FROM `cache` WHERE `key` = ? FOR UPDATE",
    )
    .bind(&full)
    .fetch_optional(&mut *tx)
    .await?;
    let value = match row {
        Some(row) => {
            let expiration: i64 = row.try_get("expiration")?;
            let raw: String = row.try_get("value")?;
            sqlx::query("DELETE FROM `cache` WHERE `key` = ?")
                .bind(&full)
                .execute(&mut *tx)
                .await?;
            (expiration > unix_now())
                .then(|| php_unserialize(&raw))
                .flatten()
        }
        None => None,
    };
    tx.commit().await?;
    Ok(value)
}

// ---------------------------------------------------------------------------
// Handoff
// ---------------------------------------------------------------------------

fn handoff_key(code: &str) -> String {
    format!("auth_handoff:{code}")
}

fn oauth_state_key(state: &str) -> String {
    format!("oauth_state:{state}")
}

fn random_alnum(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// `POST /api/auth/handoff`: menukar token sesi jadi kode sekali pakai (60 detik).
pub async fn create_handoff(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let user = match crate::require_auth(&state, &headers).await {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };
    let key = format!("handoff-create:{}", user.user_id);
    if let Err(retry) =
        state
            .limiter
            .hit(&key, HANDOFF_CREATE_MAX_PER_MINUTE, Duration::from_secs(60))
    {
        return too_many_attempts(retry);
    }
    let Some(token) = session::token_from_headers(&headers, &state.session.name) else {
        return ApiError::unauthenticated().into_response();
    };

    let code = random_alnum(HANDOFF_CODE_LEN);
    let value = Php::arr(vec![
        ("token", Php::Str(token)),
        ("user_id", Php::Int(user.user_id as i64)),
    ]);
    if let Err(e) = cache_put(&state.pool, &handoff_key(&code), &value, HANDOFF_TTL_SECS).await {
        return internal(e).into_response();
    }
    Json(json!({ "code": code, "expires_in": HANDOFF_TTL_SECS })).into_response()
}

/// `POST /api/auth/handoff/exchange`: kode ditukar dengan token dan data user. Kode hanya sekali pakai.
pub async fn exchange_handoff(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let key = format!("handoff-exchange:{}", client_ip(&headers));
    if let Err(retry) = state.limiter.hit(
        &key,
        HANDOFF_EXCHANGE_MAX_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        return too_many_attempts(retry);
    }

    let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let code = match code_from_input(&input) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };

    let payload = match cache_pull(&state.pool, &handoff_key(&code)).await {
        Ok(p) => p,
        Err(e) => return internal(e).into_response(),
    };
    let Some((token, user_id)) = payload.as_ref().and_then(handoff_payload) else {
        return handoff_gone();
    };
    let user = match users::resource(&state.pool, &state.app_url, user_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return handoff_gone(),
        Err(e) => return internal(e).into_response(),
    };
    Json(json!({ "user": user, "token": token })).into_response()
}

/// Aturan `required|string|size:48` dengan pesan Laravel. Input di-trim seperti `TrimStrings`.
fn code_from_input(input: &Value) -> Result<String, ApiError> {
    let invalid = |message: &str| {
        let mut errors = BTreeMap::new();
        errors.insert("code".to_string(), vec![message.to_string()]);
        ApiError::validation("The given data was invalid.", errors)
    };
    match input.get("code") {
        None | Some(Value::Null) => Err(invalid("The code field is required.")),
        Some(Value::String(raw)) => {
            let code = raw.trim();
            if code.is_empty() {
                Err(invalid("The code field is required."))
            } else if code.chars().count() != HANDOFF_CODE_LEN {
                Err(invalid("The code field must be 48 characters."))
            } else {
                Ok(code.to_string())
            }
        }
        Some(_) => Err(invalid("The code field must be a string.")),
    }
}

fn handoff_payload(payload: &Php) -> Option<(String, u64)> {
    let token = payload.get("token")?.as_str()?.to_string();
    let Php::Int(user_id) = payload.get("user_id")? else {
        return None;
    };
    if token.is_empty() || *user_id <= 0 {
        return None;
    }
    Some((token, *user_id as u64))
}

fn handoff_gone() -> Response {
    (
        StatusCode::GONE,
        Json(json!({ "message": HANDOFF_GONE_MESSAGE })),
    )
        .into_response()
}

fn too_many_attempts(retry_secs: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_secs.to_string())],
        Json(json!({ "message": "Too Many Attempts." })),
    )
        .into_response()
}

/// Sama dengan `client_ip` di `auth_routes`: IP pertama dari `X-Forwarded-For`.
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Google OAuth
// ---------------------------------------------------------------------------

/// Endpoint Google. Di produksi selalu `GoogleEndpoints::default()`. Tes memakai server stub.
#[derive(Debug, Clone)]
pub struct GoogleEndpoints {
    pub auth: String,
    pub token: String,
    pub userinfo: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            auth: GOOGLE_AUTH_URL.to_string(),
            token: GOOGLE_TOKEN_URL.to_string(),
            userinfo: GOOGLE_USERINFO_URL.to_string(),
        }
    }
}

/// Klien OAuth Google. `client_secret` tidak ditampilkan lewat `Debug`.
#[derive(Clone)]
pub struct GoogleClient {
    http: reqwest::Client,
    endpoints: GoogleEndpoints,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl std::fmt::Debug for GoogleClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleClient")
            .field("endpoints", &self.endpoints)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

/// Data user dari `userinfo` Google, sudah dipetakan seperti `GoogleProvider::mapUserToObject`.
#[derive(Debug, Clone, PartialEq)]
pub struct GoogleProfile {
    pub google_id: Option<String>,
    pub name: String,
    pub email: String,
    pub avatar: Option<String>,
    pub gender: Option<String>,
}

impl GoogleClient {
    /// Membaca `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, dan `GOOGLE_REDIRECT_URI` dari environment.
    pub fn from_env(app_url: &str) -> Self {
        let redirect_uri = std::env::var("GOOGLE_REDIRECT_URI").unwrap_or_else(|_| {
            format!("{}/api/auth/google/callback", app_url.trim_end_matches('/'))
        });
        Self::with_endpoints(
            GoogleEndpoints::default(),
            &env_or("GOOGLE_CLIENT_ID", ""),
            &env_or("GOOGLE_CLIENT_SECRET", ""),
            &redirect_uri,
        )
    }

    pub fn with_endpoints(
        endpoints: GoogleEndpoints,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
    ) -> Self {
        Self {
            http: http_client().clone(),
            endpoints,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
        }
    }

    /// URL redirect ke Google, dengan urutan parameter seperti `buildAuthUrlFromBase`.
    pub fn auth_url(&self, state: &str) -> String {
        let scope = GOOGLE_SCOPES.join(" ");
        let fields = [
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("scope", scope.as_str()),
            ("response_type", "code"),
            ("state", state),
        ];
        let query = fields
            .iter()
            .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{}?{query}", self.endpoints.auth)
    }

    /// Tukar `code` dengan access token, lalu ambil profil dari `userinfo`.
    pub async fn profile(&self, code: &str) -> Result<GoogleProfile, String> {
        let form = [
            ("grant_type", "authorization_code"),
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", self.redirect_uri.as_str()),
        ]
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

        let res = self
            .http
            .post(&self.endpoints.token)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(|e| format!("token request failed: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("token endpoint returned {}", res.status()));
        }
        let token_body: Value = res
            .json()
            .await
            .map_err(|e| format!("token response not json: {e}"))?;
        let access_token = parse_access_token(&token_body)?;

        let res = self
            .http
            .get(format!("{}?prettyPrint=false", self.endpoints.userinfo))
            .header(header::ACCEPT, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .send()
            .await
            .map_err(|e| format!("userinfo request failed: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("userinfo endpoint returned {}", res.status()));
        }
        let raw: Value = res
            .json()
            .await
            .map_err(|e| format!("userinfo response not json: {e}"))?;
        parse_profile(&raw)
    }
}

/// `access_token` dari respons token. Tanpa itu, Socialite juga gagal.
pub fn parse_access_token(body: &Value) -> Result<String, String> {
    body.get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "access_token missing from token response".to_string())
}

/// Memetakan `userinfo` Google. `email` dan `name` wajib, karena kolom `users` tidak boleh NULL.
pub fn parse_profile(raw: &Value) -> Result<GoogleProfile, String> {
    let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_string);
    let email = text("email")
        .filter(|e| !e.is_empty())
        .ok_or_else(|| "email missing from userinfo".to_string())?;
    let name = text("name").ok_or_else(|| "name missing from userinfo".to_string())?;
    // Sama seperti Laravel: `genders` berasal dari People API, bukan userinfo.
    let gender = raw
        .get("genders")
        .and_then(|g| g.get(0))
        .and_then(|g| g.get("value"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(GoogleProfile {
        google_id: text("sub"),
        name,
        email,
        avatar: text("picture"),
        gender,
    })
}

/// Encoding `http_build_query` PHP (RFC1738): spasi jadi `+`, selain alfanumerik dan `-_.` jadi `%XX`.
pub fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `rawurlencode` PHP (RFC3986): dipakai untuk nilai di fragment redirect.
pub fn raw_url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `normalizeOAuthCallbackUrl`: `pengawas://...` atau URL http(s) dengan host, tanpa `/` akhir.
pub fn normalize_callback_url(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("pengawas://") || is_http_url(trimmed) {
        return Some(trimmed.trim_end_matches('/').to_string());
    }
    None
}

fn is_http_url(value: &str) -> bool {
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    if host_port.starts_with('[') {
        return false;
    }
    let host = host_port.split(':').next().unwrap_or("");
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
}

/// `GET /api/auth/google`: menyimpan state 10 menit, lalu mengembalikan `{url}` ke Google.
pub async fn redirect_to_google(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let platform = if query.get("platform").map(String::as_str) == Some("mobile") {
        "mobile"
    } else {
        "web"
    };
    let callback_url = normalize_callback_url(query.get("callback_url").map(String::as_str));
    let token = random_alnum(OAUTH_STATE_LEN);
    let value = Php::arr(vec![
        ("platform", Php::Str(platform.to_string())),
        ("callback_url", callback_url.map_or(Php::Null, Php::Str)),
    ]);
    if let Err(e) = cache_put(
        &state.pool,
        &oauth_state_key(&token),
        &value,
        OAUTH_STATE_TTL_SECS,
    )
    .await
    {
        return internal(e).into_response();
    }
    let google = GoogleClient::from_env(&state.app_url);
    Json(json!({ "url": google.auth_url(&token) })).into_response()
}

/// `GET /api/auth/google/callback`: selalu redirect 302 ke frontend atau callback mobile.
pub async fn handle_google_callback(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let google = GoogleClient::from_env(&state.app_url);
    callback_response(&state.pool, &query, &google).await
}

/// Inti callback. Dipisah supaya tes bisa memakai `GoogleClient` dengan endpoint stub.
pub async fn callback_response(
    pool: &MySqlPool,
    query: &HashMap<String, String>,
    google: &GoogleClient,
) -> Response {
    let base =
        match resolve_callback_base(pool, query.get("state").map_or("", String::as_str)).await {
            Ok(b) => b,
            Err(e) => return internal(e).into_response(),
        };
    let fragment = match finish_login(pool, google, query.get("code").map(String::as_str)).await {
        Ok(token) => format!("#token={}", raw_url_encode(&token)),
        Err(LoginFailure::Maintenance) => {
            format!("#error={}", raw_url_encode(MAINTENANCE_MESSAGE))
        }
        Err(LoginFailure::Failed(reason)) => {
            tracing::warn!(reason = %reason, "Google OAuth failed");
            format!("#error={}", raw_url_encode(GOOGLE_FAILED_MESSAGE))
        }
    };
    redirect(&format!("{base}{fragment}"))
}

#[derive(Debug)]
enum LoginFailure {
    Maintenance,
    Failed(String),
}

/// Pertukaran kode, upsert user, role default, cek maintenance, lalu token. Urutannya sama seperti Laravel.
async fn finish_login(
    pool: &MySqlPool,
    google: &GoogleClient,
    code: Option<&str>,
) -> Result<String, LoginFailure> {
    let code = code
        .filter(|c| !c.is_empty())
        .ok_or_else(|| LoginFailure::Failed("code missing".to_string()))?;
    let profile = google.profile(code).await.map_err(LoginFailure::Failed)?;

    let (user_id, created) = upsert_google_user(pool, &profile)
        .await
        .map_err(|e| LoginFailure::Failed(format!("user upsert: {e}")))?;
    if created {
        assign_default_role(pool, user_id).await;
    }

    let maintenance_on = maintenance::is_enabled_db(pool)
        .await
        .map_err(|e| LoginFailure::Failed(format!("maintenance: {e}")))?;
    if maintenance_on {
        let bypass = maintenance::bypass_list_db(pool)
            .await
            .map_err(|e| LoginFailure::Failed(format!("maintenance bypass: {e}")))?;
        if !login::is_bypass(&profile.email, &bypass) {
            return Err(LoginFailure::Maintenance);
        }
    }

    login::create_token(pool, user_id, "auth-token")
        .await
        .map_err(|e| LoginFailure::Failed(format!("create token: {e}")))
}

/// `updateOrCreate(['email' => ...])`. Mengembalikan `(id, baru_dibuat)`.
async fn upsert_google_user(
    pool: &MySqlPool,
    p: &GoogleProfile,
) -> Result<(u64, bool), sqlx::Error> {
    let existing: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ? LIMIT 1")
        .bind(&p.email)
        .fetch_optional(pool)
        .await?;
    match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE users SET name = ?, google_id = ?, avatar = ?, gender = ?, \
                 email_verified_at = NOW(), updated_at = NOW() WHERE id = ?",
            )
            .bind(&p.name)
            .bind(&p.google_id)
            .bind(&p.avatar)
            .bind(&p.gender)
            .bind(id)
            .execute(pool)
            .await?;
            Ok((id, false))
        }
        None => {
            let result = sqlx::query(
                "INSERT INTO users (name, email, google_id, avatar, gender, email_verified_at, \
                 created_at, updated_at) VALUES (?, ?, ?, ?, ?, NOW(), NOW(), NOW())",
            )
            .bind(&p.name)
            .bind(&p.email)
            .bind(&p.google_id)
            .bind(&p.avatar)
            .bind(&p.gender)
            .execute(pool)
            .await?;
            Ok((result.last_insert_id(), true))
        }
    }
}

/// Role `user` (guard `web`) untuk user baru. Gagal di sini tidak menggagalkan login, seperti Laravel.
async fn assign_default_role(pool: &MySqlPool, user_id: u64) {
    let result: Result<(), sqlx::Error> = async {
        let role_id: Option<u64> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = 'user' AND guard_name = 'web' LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        if let Some(role_id) = role_id {
            sqlx::query(
                "INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)",
            )
            .bind(role_id)
            .bind(auth::USER_TOKENABLE_TYPE)
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        tracing::warn!("failed to assign default role during Google OAuth: {e}");
    }
}

/// `resolveOAuthCallbackBase`: tujuan redirect dari state yang tersimpan. State diambil sekali pakai.
async fn resolve_callback_base(pool: &MySqlPool, state_param: &str) -> Result<String, sqlx::Error> {
    let frontend = env_or("FRONTEND_URL", DEFAULT_FRONTEND_URL);
    let frontend_callback = format!("{}/oauth-callback", frontend.trim_end_matches('/'));
    let mobile_base = env_or("MOBILE_OAUTH_CALLBACK_URL", DEFAULT_MOBILE_CALLBACK)
        .trim_end_matches('/')
        .to_string();

    let stored = if state_param.is_empty() {
        None
    } else {
        cache_pull(pool, &oauth_state_key(state_param)).await?
    };
    if let Some(stored @ Php::Arr(_)) = stored {
        if stored.get("platform").and_then(Php::as_str) == Some("mobile") {
            let override_url =
                normalize_callback_url(stored.get("callback_url").and_then(Php::as_str));
            return Ok(override_url.unwrap_or(mobile_base));
        }
        return Ok(frontend_callback);
    }
    // Fallback lama: state berisi teks "mobile" dari klien terdahulu.
    if state_param == "mobile" {
        return Ok(mobile_base);
    }
    Ok(frontend_callback)
}

fn redirect(url: &str) -> Response {
    match HeaderValue::from_str(url) {
        Ok(value) => (StatusCode::FOUND, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `env()` Laravel: nilai environment, atau default bila variabel tidak ada.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};

    fn sample_client(endpoints: GoogleEndpoints) -> GoogleClient {
        GoogleClient::with_endpoints(
            endpoints,
            "uji-client-id.apps.example.test",
            "uji-secret",
            "http://localhost/api/auth/google/callback",
        )
    }

    #[test]
    fn auth_url_matches_socialite_parameters_and_order() {
        let client = sample_client(GoogleEndpoints::default());
        let url = client.auth_url("abc123");
        assert_eq!(
            url,
            "https://accounts.google.com/o/oauth2/auth?\
             client_id=uji-client-id.apps.example.test\
             &redirect_uri=http%3A%2F%2Flocalhost%2Fapi%2Fauth%2Fgoogle%2Fcallback\
             &scope=openid+profile+email+https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fuser.gender.read\
             &response_type=code&state=abc123"
        );
    }

    #[test]
    fn client_secret_never_appears_in_debug_output() {
        let client = sample_client(GoogleEndpoints::default());
        let dbg = format!("{client:?}");
        assert!(!dbg.contains("uji-secret"));
        assert!(dbg.contains("<redacted>"));
    }

    #[test]
    fn form_encode_matches_php_urlencode() {
        assert_eq!(form_encode("a b~c"), "a+b%7Ec");
        assert_eq!(form_encode("https://x/y"), "https%3A%2F%2Fx%2Fy");
    }

    #[test]
    fn raw_url_encode_matches_php_rawurlencode() {
        assert_eq!(raw_url_encode("1|ab~c d"), "1%7Cab~c%20d");
    }

    #[test]
    fn php_serialize_roundtrips_laravel_shapes() {
        let v = Php::arr(vec![
            ("token", Php::Str("3|ab+é".to_string())),
            ("user_id", Php::Int(42)),
            ("callback_url", Php::Null),
        ]);
        let text = php_serialize(&v);
        assert_eq!(
            text,
            "a:3:{s:5:\"token\";s:7:\"3|ab+é\";s:7:\"user_id\";i:42;s:12:\"callback_url\";N;}"
        );
        assert_eq!(php_unserialize(&text), Some(v));
    }

    #[test]
    fn php_unserialize_reads_value_written_by_laravel() {
        let raw = r#"a:2:{s:5:"token";s:4:"1|xy";s:7:"user_id";i:7;}"#;
        let v = php_unserialize(raw).unwrap();
        assert_eq!(handoff_payload(&v), Some(("1|xy".to_string(), 7)));
        assert_eq!(php_unserialize("garbage"), None);
        assert_eq!(php_unserialize(r#"s:9:"short";"#), None);
    }

    #[test]
    fn slug_matches_laravel_default_cache_prefix() {
        assert_eq!(slug("Laravel"), "laravel");
        assert_eq!(slug("Apiamis Prod!"), "apiamis-prod");
    }

    #[test]
    fn callback_url_normalisation() {
        assert_eq!(
            normalize_callback_url(Some(" pengawas://oauth-callback/ ")),
            Some("pengawas://oauth-callback".to_string())
        );
        assert_eq!(
            normalize_callback_url(Some("https://ami.example.test/cb/")),
            Some("https://ami.example.test/cb".to_string())
        );
        assert_eq!(normalize_callback_url(Some("javascript:alert(1)")), None);
        assert_eq!(normalize_callback_url(Some("ftp://x.example.test")), None);
        assert_eq!(normalize_callback_url(Some("https://")), None);
        assert_eq!(normalize_callback_url(Some("https://a b.test")), None);
        assert_eq!(normalize_callback_url(Some("   ")), None);
        assert_eq!(normalize_callback_url(None), None);
    }

    #[test]
    fn code_validation_matches_laravel_messages() {
        let err = code_from_input(&json!({})).unwrap_err();
        assert_eq!(
            err.body()["errors"]["code"][0],
            "The code field is required."
        );
        let err = code_from_input(&json!({ "code": "short" })).unwrap_err();
        assert_eq!(
            err.body()["errors"]["code"][0],
            "The code field must be 48 characters."
        );
        let err = code_from_input(&json!({ "code": 123 })).unwrap_err();
        assert_eq!(
            err.body()["errors"]["code"][0],
            "The code field must be a string."
        );
        let ok = code_from_input(&json!({ "code": format!("  {}  ", "a".repeat(48)) })).unwrap();
        assert_eq!(ok.len(), 48);
    }

    #[test]
    fn google_userinfo_maps_like_socialite() {
        let raw = json!({
            "sub": "1098",
            "name": "Uji Google",
            "email": "uji-google@example.test",
            "picture": "https://img.example.test/a.png",
            "email_verified": true
        });
        let p = parse_profile(&raw).unwrap();
        assert_eq!(p.google_id.as_deref(), Some("1098"));
        assert_eq!(p.email, "uji-google@example.test");
        assert_eq!(p.avatar.as_deref(), Some("https://img.example.test/a.png"));
        assert_eq!(p.gender, None);

        let with_gender =
            json!({ "name": "A", "email": "a@example.test", "genders": [{ "value": "female" }] });
        assert_eq!(
            parse_profile(&with_gender).unwrap().gender.as_deref(),
            Some("female")
        );

        assert!(parse_profile(&json!({ "name": "A" })).is_err());
        assert!(parse_profile(&json!({ "email": "a@example.test" })).is_err());
        assert!(parse_access_token(&json!({ "access_token": "" })).is_err());
        assert_eq!(
            parse_access_token(&json!({ "access_token": "ya29.x" })).unwrap(),
            "ya29.x"
        );
    }

    /// Server stub lokal untuk endpoint Google. Tidak memanggil Google sungguhan.
    async fn stub_google(token_status: u16, userinfo: Value) -> GoogleEndpoints {
        let app = Router::new()
            .route(
                "/token",
                post(move |body: String| async move {
                    let status = StatusCode::from_u16(token_status).unwrap();
                    let ok_body = json!({ "access_token": "ya29.stub", "expires_in": 3599 });
                    let reply = if body.contains("grant_type=authorization_code")
                        && body.contains("code=uji-code")
                    {
                        ok_body
                    } else {
                        json!({ "error": "invalid_grant" })
                    };
                    (status, Json(reply))
                }),
            )
            .route(
                "/userinfo",
                axum::routing::get(move |headers: HeaderMap| {
                    let userinfo = userinfo.clone();
                    async move {
                        if headers
                            .get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            == Some("Bearer ya29.stub")
                        {
                            (StatusCode::OK, Json(userinfo))
                        } else {
                            (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({ "error": "bad token" })),
                            )
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        GoogleEndpoints {
            auth: format!("http://{addr}/auth"),
            token: format!("http://{addr}/token"),
            userinfo: format!("http://{addr}/userinfo"),
        }
    }

    #[tokio::test]
    async fn profile_exchanges_code_and_reads_userinfo_from_stub() {
        let endpoints = stub_google(
            200,
            json!({ "sub": "9", "name": "Stub", "email": "stub@example.test", "picture": null }),
        )
        .await;
        let client = sample_client(endpoints);
        let p = client.profile("uji-code").await.unwrap();
        assert_eq!(p.email, "stub@example.test");
        assert_eq!(p.google_id.as_deref(), Some("9"));
        assert_eq!(p.avatar, None);
    }

    #[tokio::test]
    async fn profile_fails_when_token_endpoint_rejects_code() {
        let endpoints = stub_google(400, json!({})).await;
        let client = sample_client(endpoints);
        let err = client.profile("uji-code").await.unwrap_err();
        assert!(err.contains("400"), "{err}");
        let err = client.profile("wrong-code").await.unwrap_err();
        assert!(err.contains("400"), "{err}");
    }
}
