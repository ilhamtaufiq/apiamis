//! Procurement SPSE: session cookie, sinkron paket, staging, apply, mapping, dan promosi draft.
//!
//! Setara `SpseProcurementController` untuk rute `procurement/spse/*` kecuali `kontrak/push`,
//! dengan `SpseSessionStore`, `SpseCookieParser`, `SpseHttpClient`, `SpseSyncService`, dan
//! `ProcurementMatchingService`. Pemindaian dokumen, impor berkas, dan ZIP ada di `procurement_docs`.
//!
//! Perbedaan dengan Laravel:
//! - `authenticityToken` diambil ulang untuk setiap permintaan DataTables. Laravel men-cache per request.
//! - Promosi draft memakai transaksi per item. Bila satu item gagal, itu di-rollback. Laravel tidak
//!   memakai transaksi, sehingga bisa tersisa pekerjaan tanpa kontrak.
//! - Pesan error database tidak dikirim ke klien (hanya ke log), karena `error_log` tampil di UI.
//! - Respon `staging/map` memuat `pekerjaan` dan `kontrak` dengan subset kolom, bukan seluruh atribut.
//! - Pembersihan teks memakai `kontrak::decode_entities` (entitas umum saja).
//! - Normalisasi nama paket memakai `is_alphanumeric`, yang sedikit berbeda dari `\pL\pN` untuk tanda gabung.

use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
    time::Duration,
};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlArguments, MySql, MySqlPool, Row};

use crate::{
    changes::{self, Target},
    crypt,
    format::iso8601_utc,
    kontrak::{self, decode_entities},
    lookup::carbon_json,
    pagination::{self, PageParams},
    require_auth,
    validation::Errors,
    AppState,
};

const DEFAULT_BASE: &str = "https://spse.inaproc.id";
const DEFAULT_SLUG: &str = "cianjurkab";
const SESSION_HOURS: i64 = 8;
const MAX_PAGES: u64 = 50;
const ERROR_LOG_LIMIT: usize = 50;
pub(crate) const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
pub(crate) const JSON_ACCEPT: &str = "application/json, text/javascript, */*; q=0.01";
pub(crate) const PAGE_ACCEPT: &str = "text/html,application/xhtml+xml,application/pdf,*/*;q=0.8";
const EXPIRED_MSG: &str = "Session SPSE expired. Login ulang di SPSE lalu kirim cookie lagi.";
const NO_SESSION_MSG: &str = "Session SPSE tidak aktif. Login ulang di SPSE.";

/// Sumber sinkron: jenis paket, endpoint DataTables, dan path referer (urutan sama dengan Laravel).
const SOURCES: [(&str, &str, &str); 2] = [
    ("pengadaan_langsung", "/dt/paket-ppk-pl", "/beranda/nontender"),
    ("tender_seleksi", "/dt/paket-ppk", "/home"),
];

pub(crate) const PEKERJAAN_TARGET: Target = Target {
    model_type: "App\\Models\\Pekerjaan",
    label: "Pekerjaan",
    tab: "",
};

/// `Regex` yang dikompilasi sekali. Pola selalu tetap, jadi `expect` tidak mungkin gagal.
macro_rules! regex_once {
    ($pattern:expr) => {{
        static RE: ::std::sync::OnceLock<::regex::Regex> = ::std::sync::OnceLock::new();
        RE.get_or_init(|| ::regex::Regex::new($pattern).expect("pola regex tetap"))
    }};
}
pub(crate) use regex_once;

// ---------------------------------------------------------------------------
// Galat dan util umum
// ---------------------------------------------------------------------------

/// Galat dari permintaan ke SPSE.
#[derive(Debug)]
pub(crate) enum SpseError {
    /// Cookie ditolak atau diarahkan ke halaman login (`SpseSessionExpiredException`).
    Expired,
    Failed(String),
}

impl SpseError {
    pub(crate) fn message(&self) -> String {
        match self {
            SpseError::Expired => EXPIRED_MSG.to_string(),
            SpseError::Failed(m) => m.clone(),
        }
    }
}

pub(crate) fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("procurement: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

fn server_error(detail: &str) -> ApiError {
    tracing::error!("procurement: {detail}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

/// `$url` lengkap untuk `Request::fullUrl()`.
pub(crate) fn full_url(state: &AppState, path: &str) -> String {
    format!("{}{path}", state.app_url.trim_end_matches('/'))
}

/// `SPSE_BASE_URL` tanpa garis miring di akhir (`services.spse.base_url`).
pub(crate) fn spse_root() -> String {
    std::env::var("SPSE_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_BASE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// `SPSE_LPSE_SLUG` (`services.spse.lpse_slug`).
pub(crate) fn default_slug() -> String {
    std::env::var("SPSE_LPSE_SLUG").unwrap_or_else(|_| DEFAULT_SLUG.to_string())
}

fn base_of(slug: &str) -> String {
    format!("{}/{slug}", spse_root())
}

/// `empty()` di PHP: null, "", dan "0" dianggap kosong.
pub(crate) fn php_empty(v: Option<&str>) -> bool {
    matches!(v, None | Some("") | Some("0"))
}

/// Nilai JSON sebagai string seperti `(string)` di PHP.
pub(crate) fn php_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::Bool(true)) => "1".to_string(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => "Array".to_string(),
    }
}

/// `(int)` di PHP untuk angka atau string.
fn php_int(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::String(s)) => {
            let t = s.trim();
            let (sign, digits) = match t.strip_prefix('-') {
                Some(rest) => (-1, rest),
                None => (1, t),
            };
            let n: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
            n.parse::<i64>().map(|v| sign * v).unwrap_or(0)
        }
        _ => 0,
    }
}

/// `mb_substr($s, 0, $n)`.
pub(crate) fn mb_take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Pembersihan teks SPSE: nilai array digabung spasi (nilai kosong dan "0" dibuang), tag dibuang,
/// entitas didekode, spasi dirapikan (`SpseSyncService::cleanText`).
pub(crate) fn clean_text(v: Option<&Value>) -> String {
    let raw = match v {
        Some(Value::Array(items)) => items
            .iter()
            .filter(|x| x.is_string() || x.is_number() || x.is_boolean())
            .map(|x| php_str(Some(x)))
            .filter(|s| !php_empty(Some(s.as_str())))
            .collect::<Vec<_>>()
            .join(" "),
        other => php_str(other),
    };
    let stripped = regex_once!(r"(?s)<[^>]*>").replace_all(&raw, "");
    let decoded = decode_entities(&stripped);
    regex_once!(r"\s+").replace_all(&decoded, " ").trim().to_string()
}

/// Ekstraksi kunci dari baris SPSE (`data_get` untuk array dengan kunci string).
fn data_get_str(raw: Option<&Value>, key: &str) -> Option<String> {
    let v = raw?.as_object()?.get(key)?;
    if v.is_null() {
        None
    } else {
        Some(php_str(Some(v)))
    }
}

/// Normalisasi nama untuk pencocokan: hanya huruf dan angka, huruf kecil.
fn normalize_lookup(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Persen-encoding untuk badan `application/x-www-form-urlencoded`.
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Dekode persen dan `+` menjadi spasi (`urldecode` di PHP).
pub(crate) fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fungsi `strip_tags` sederhana untuk HTML anchor (tag dibuang, teks dipertahankan).
pub(crate) fn strip_tags(s: &str) -> String {
    regex_once!(r"(?s)<[^>]*>").replace_all(s, "").into_owned()
}

/// Ringkasan `looks_like_login` di `SpseHttpClient`.
pub(crate) fn looks_like_login(body: &[u8]) -> bool {
    let trimmed: &[u8] = {
        let start = body
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(body.len());
        &body[start..]
    };
    let head = String::from_utf8_lossy(&trimmed[..trimmed.len().min(20_000)]).to_lowercase();
    if head.is_empty() || !head.starts_with('<') {
        return false;
    }
    head.contains("loginctr") || head.contains("login ctr")
}

// ---------------------------------------------------------------------------
// Cookie (SpseCookieParser)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

/// Cookie terstruktur dari JSON. Nama kosong dibuang. Domain dan path default `.inaproc.id` dan `/`.
fn cookies_from_structured(list: &[Value]) -> Vec<Cookie> {
    list.iter()
        .filter_map(|c| {
            let name = php_str(c.get("name"));
            if php_empty(Some(name.as_str())) {
                return None;
            }
            let domain = match c.get("domain") {
                None | Some(Value::Null) => ".inaproc.id".to_string(),
                v => php_str(v),
            };
            let path = match c.get("path") {
                None | Some(Value::Null) => "/".to_string(),
                v => php_str(v),
            };
            Some(Cookie {
                name,
                value: php_str(c.get("value")),
                domain,
                path,
            })
        })
        .collect()
}

/// Header `Cookie` dari DevTools. Prefix `Cookie:` dan tanda kutip pembungkus dibuang.
pub(crate) fn cookies_from_header(raw: &str) -> Vec<Cookie> {
    let mut t = raw.trim();
    if let Some(rest) = strip_prefix_ci(t, "cookie") {
        if let Some(after) = rest.trim_start().strip_prefix(':') {
            t = after.trim_start();
        }
    }
    let cleaned: String = t.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    let cleaned = cleaned
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == ' ');

    let mut out = Vec::new();
    for part in cleaned.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.chars().any(|c| c.is_whitespace() || c == ',' || c == '"') {
            continue;
        }
        let lower = name.to_lowercase();
        let domain = if lower.starts_with("play_") || name == "sirup_instansi_id" {
            "sirup.inaproc.id"
        } else if name.eq_ignore_ascii_case("SPSE_SESSION") {
            "spse.inaproc.id"
        } else {
            ".inaproc.id"
        };
        out.push(Cookie {
            name: name.to_string(),
            value: value.trim().to_string(),
            domain: domain.to_string(),
            path: "/".to_string(),
        });
    }
    out
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// `SpseCookieParser::parse`: cookie terstruktur diutamakan bila tidak kosong.
pub(crate) fn parse_cookies(header: Option<&str>, structured: Option<&[Value]>) -> Vec<Cookie> {
    if let Some(list) = structured.filter(|l| !l.is_empty()) {
        return cookies_from_structured(list);
    }
    match header.filter(|h| !h.is_empty()) {
        Some(h) => cookies_from_header(h),
        None => Vec::new(),
    }
}

pub(crate) fn cookie_header(cookies: &[Cookie]) -> String {
    cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

fn cookies_to_json(cookies: &[Cookie]) -> String {
    Value::Array(
        cookies
            .iter()
            .map(|c| json!({ "name": c.name, "value": c.value, "domain": c.domain, "path": c.path }))
            .collect(),
    )
    .to_string()
}

fn cookies_from_json(raw: &str) -> Option<Vec<Cookie>> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let list = v.as_array()?;
    Some(cookies_from_structured(list))
}

// ---------------------------------------------------------------------------
// Session tersimpan (SpseSessionStore, tbl_spse_sessions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub id: i64,
    /// Slug LPSE, selalu terisi (default `SPSE_LPSE_SLUG` bila kosong).
    pub lpse_slug: String,
    pub cookies: Vec<Cookie>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Kunci `APP_KEY` untuk enkripsi cookie (`encrypted:array`).
fn app_key() -> Result<Vec<u8>, ApiError> {
    let raw = std::env::var("APP_KEY").map_err(|_| server_error("APP_KEY belum di-set"))?;
    crypt::key_from_app_key(&raw).map_err(|_| server_error("APP_KEY tidak valid"))
}

/// Session aktif milik user (`SpseSession::activeForUser`), atau `None`.
pub(crate) async fn active_session(
    pool: &MySqlPool,
    user_id: u64,
) -> Result<Option<Session>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, lpse_slug, encrypted_cookies, expires_at \
         FROM tbl_spse_sessions \
         WHERE user_id = ? AND is_active = 1 AND (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let payload: String = r.try_get("encrypted_cookies").map_err(internal)?;
    let plain = crypt::decrypt_string(&app_key()?, &payload)
        .map_err(|_| server_error("encrypted_cookies tidak bisa didekripsi"))?;
    let cookies = cookies_from_json(&plain)
        .ok_or_else(|| server_error("encrypted_cookies bukan JSON array"))?;
    let slug: String = r.try_get("lpse_slug").map_err(internal)?;
    Ok(Some(Session {
        id: r.try_get("id").map_err(internal)?,
        lpse_slug: if slug.is_empty() { default_slug() } else { slug },
        cookies,
        expires_at: r.try_get("expires_at").map_err(internal)?,
    }))
}

/// `activeSession` untuk rute yang wajib session (401 bila tidak ada).
pub(crate) async fn require_session(pool: &MySqlPool, user_id: u64) -> Result<Session, ApiError> {
    active_session(pool, user_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, NO_SESSION_MSG))
}

pub(crate) async fn deactivate_session(pool: &MySqlPool, session_id: i64) -> Result<(), ApiError> {
    sqlx::query("UPDATE tbl_spse_sessions SET is_active = 0, updated_at = NOW() WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(internal)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP ke SPSE (SpseHttpClient)
// ---------------------------------------------------------------------------

pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::limited(8))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Respon yang sudah dibaca penuh.
pub(crate) struct Resp {
    pub status: u16,
    /// URL akhir setelah redirect.
    pub url: String,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub body: Vec<u8>,
}

pub(crate) async fn send(rb: reqwest::RequestBuilder) -> Result<Resp, reqwest::Error> {
    let r = rb.send().await?;
    let status = r.status().as_u16();
    let url = r.url().to_string();
    let header = |name: header::HeaderName| {
        r.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let location = header(header::LOCATION);
    let content_type = header(header::CONTENT_TYPE);
    let content_disposition = header(header::CONTENT_DISPOSITION);
    let body = r.bytes().await?.to_vec();
    Ok(Resp {
        status,
        url,
        location,
        content_type,
        content_disposition,
        body,
    })
}

pub(crate) fn with_cookies(rb: reqwest::RequestBuilder, cookies: &[Cookie]) -> reqwest::RequestBuilder {
    rb.header(header::COOKIE, cookie_header(cookies))
}

/// `baseUrl($session)` ditambah path, atau URL absolut bila sudah `http(s)://`.
pub(crate) fn absolute_url(s: &Session, url_or_path: &str) -> String {
    if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
        return url_or_path.to_string();
    }
    let path = if url_or_path.starts_with('/') {
        url_or_path.to_string()
    } else {
        format!("/{url_or_path}")
    };
    let slug_prefix = format!("/{}", s.lpse_slug);
    if path == slug_prefix || path.starts_with(&format!("{slug_prefix}/")) {
        return format!("{}{path}", spse_root());
    }
    format!("{}{path}", base_of(&s.lpse_slug))
}

fn path_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|u| u.path().to_string())
        .unwrap_or_default()
}

/// Alasan session tidak valid, atau `None` bila valid (`diagnoseSession`).
pub(crate) async fn diagnose(s: &Session) -> Option<String> {
    let url = format!("{}/home", base_of(&s.lpse_slug));
    let rb = with_cookies(client().get(&url), &s.cookies).header(header::ACCEPT, JSON_ACCEPT);
    let resp = match send(rb).await {
        Ok(r) => r,
        Err(e) => return Some(format!("Tidak dapat terhubung ke SPSE: {e}")),
    };
    if resp.status == 401 || resp.status == 403 {
        return Some(format!("SPSE menolak cookie (HTTP {}).", resp.status));
    }
    if !(200..300).contains(&resp.status) {
        return Some(format!("SPSE membalas HTTP {}.", resp.status));
    }
    if path_of(&resp.url).to_lowercase().contains("/login") || looks_like_login(&resp.body) {
        return Some(
            "Cookie SPSE_SESSION sudah kedaluwarsa atau belum login (diarahkan ke halaman login)."
                .to_string(),
        );
    }
    None
}

pub(crate) fn assert_authenticated(r: &Resp) -> Result<(), SpseError> {
    let location = r.location.as_deref().unwrap_or("").to_lowercase();
    let login_redirect =
        !location.is_empty() && (location.contains("/login") || location.contains("loginctr"));
    if r.status == 401 || r.status == 403 || login_redirect || looks_like_login(&r.body) {
        return Err(SpseError::Expired);
    }
    Ok(())
}

/// Halaman HTML SPSE (`fetchPage`).
pub(crate) async fn fetch_page(
    s: &Session,
    path: &str,
    referer: Option<&str>,
) -> Result<String, SpseError> {
    let url = absolute_url(s, path);
    let mut rb = with_cookies(client().get(&url), &s.cookies).header(header::ACCEPT, PAGE_ACCEPT);
    if let Some(r) = referer {
        rb = rb.header(header::REFERER, absolute_url(s, r));
    }
    let resp = send(rb)
        .await
        .map_err(|e| SpseError::Failed(e.to_string()))?;
    assert_authenticated(&resp)?;
    if !(200..300).contains(&resp.status) {
        return Err(SpseError::Failed(format!(
            "SPSE halaman gagal: HTTP {} ({path})",
            resp.status
        )));
    }
    Ok(String::from_utf8_lossy(&resp.body).into_owned())
}

/// Unduhan biner (`downloadBinary`). `final_url` adalah URL yang diminta, seperti di Laravel.
pub(crate) struct Downloaded {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub final_url: String,
}

pub(crate) async fn download_binary(
    s: &Session,
    url_or_path: &str,
) -> Result<Downloaded, SpseError> {
    let url = absolute_url(s, url_or_path);
    let rb = with_cookies(client().get(&url), &s.cookies).header(header::ACCEPT, PAGE_ACCEPT);
    let resp = send(rb)
        .await
        .map_err(|e| SpseError::Failed(e.to_string()))?;
    if !(200..300).contains(&resp.status) {
        return Err(SpseError::Failed(format!(
            "SPSE unduh gagal: HTTP {}",
            resp.status
        )));
    }
    Ok(Downloaded {
        body: resp.body,
        content_type: resp.content_type,
        content_disposition: resp.content_disposition,
        final_url: url,
    })
}

/// Pemeriksaan ringan sebelum dokumen ditawarkan untuk impor (`isDownloadableBinary`).
pub(crate) async fn is_downloadable_binary(
    s: &Session,
    path: &str,
    referer: Option<&str>,
) -> bool {
    let url = absolute_url(s, path);
    let mut rb = with_cookies(client().get(&url), &s.cookies).header(header::ACCEPT, PAGE_ACCEPT);
    if let Some(r) = referer {
        rb = rb.header(header::REFERER, absolute_url(s, r));
    }
    let Ok(resp) = send(rb).await else {
        return false;
    };
    if !(200..300).contains(&resp.status) {
        return false;
    }
    let content_type = resp.content_type.unwrap_or_default().to_lowercase();
    let disposition = resp.content_disposition.unwrap_or_default().to_lowercase();
    if resp.body.starts_with(b"%PDF") {
        return true;
    }
    if disposition.contains("attachment") || disposition.contains("filename=") {
        return true;
    }
    if ["pdf", "zip", "octet-stream", "msword", "officedocument"]
        .iter()
        .any(|k| content_type.contains(k))
    {
        return true;
    }
    if content_type.contains("text/html") || content_type.contains("text/plain") {
        return false;
    }
    !resp.body.is_empty()
}

pub(crate) fn token_from_cookies(cookies: &[Cookie]) -> Option<String> {
    cookies
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case("SPSE_SESSION"))
        .find_map(|c| {
            regex_once!(r"___AT=([^&]+)")
                .captures(&c.value)
                .map(|m| m[1].to_string())
        })
}

/// Token dari HTML halaman (`extractTokenFromHtml`).
pub(crate) fn token_from_html(html: &str) -> Option<String> {
    const PATTERNS: [&str; 4] = [
        r#"(?i)d\.authenticityToken\s*=\s*['"]([^'"]+)['"]"#,
        r#"(?i)name=["']authenticityToken["']\s+value=["']([^"']+)["']"#,
        r#"(?i)name=["']_csrf["']\s+value=["']([^"']+)["']"#,
        r#"(?i)authenticityToken["']\s*:\s*["']([^"']+)["']"#,
    ];
    let found = |pattern: &str| {
        regex::Regex::new(pattern)
            .ok()
            .and_then(|re| re.captures(html).map(|m| m[1].to_string()))
    };
    PATTERNS.iter().find_map(|p| found(p))
}

/// `resolveAuthenticityToken`: dari cookie `SPSE_SESSION` (`___AT=`), lalu dari halaman.
async fn resolve_token(s: &Session, referer: &str) -> Result<String, SpseError> {
    if let Some(t) = token_from_cookies(&s.cookies) {
        return Ok(t);
    }
    let base = base_of(&s.lpse_slug);
    let mut pages = vec![format!("{base}{referer}"), format!("{base}/beranda/nontender"), format!("{base}/home")];
    let mut seen = HashSet::new();
    pages.retain(|p| seen.insert(p.clone()));
    for url in pages {
        let rb = with_cookies(client().get(&url), &s.cookies).header(header::ACCEPT, PAGE_ACCEPT);
        if let Ok(resp) = send(rb).await {
            if (200..300).contains(&resp.status) {
                if let Some(t) = token_from_html(&String::from_utf8_lossy(&resp.body)) {
                    return Ok(t);
                }
            }
        }
    }
    Err(SpseError::Failed(
        "Tidak dapat mengambil authenticityToken dari SPSE.".to_string(),
    ))
}

/// Body DataTables (`buildDataTablesBody`).
fn datatables_form(draw: u64, start: u64, length: u64, token: &str) -> Vec<(String, String)> {
    let mut f: Vec<(String, String)> = vec![
        ("draw".into(), draw.to_string()),
        ("start".into(), start.to_string()),
        ("length".into(), length.to_string()),
        ("search[value]".into(), String::new()),
        ("search[regex]".into(), "false".into()),
        ("order[0][column]".into(), "0".into()),
        ("order[0][dir]".into(), "desc".into()),
        ("authenticityToken".into(), token.to_string()),
    ];
    for i in 0..5 {
        f.push((format!("columns[{i}][data]"), i.to_string()));
        f.push((format!("columns[{i}][name]"), String::new()));
        f.push((format!("columns[{i}][searchable]"), "true".into()));
        f.push((
            format!("columns[{i}][orderable]"),
            if i <= 1 { "true" } else { "false" }.into(),
        ));
        f.push((format!("columns[{i}][search][value]"), String::new()));
        f.push((format!("columns[{i}][search][regex]"), "false".into()));
    }
    f
}

/// Satu halaman DataTables (`fetchDataTable`). Retry dua kali untuk galat koneksion atau 5xx.
async fn fetch_datatable(
    s: &Session,
    endpoint: &str,
    start: u64,
    length: u64,
    draw: u64,
    referer: &str,
) -> Result<Value, SpseError> {
    let token = resolve_token(s, referer).await?;
    let url = format!("{}{endpoint}?status=1", base_of(&s.lpse_slug));
    let referer_url = format!("{}{referer}", base_of(&s.lpse_slug));
    let body = pct_form(&datatables_form(draw, start, length, &token));

    let mut attempt = 0;
    let resp = loop {
        let rb = with_cookies(client().post(&url), &s.cookies)
            .header(header::ACCEPT, JSON_ACCEPT)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::REFERER, &referer_url)
            .header("X-Requested-With", "XMLHttpRequest")
            .body(body.clone());
        match send(rb).await {
            Ok(r) if r.status >= 500 && attempt < 2 => {}
            Ok(r) => break r,
            Err(e) if attempt < 2 && (e.is_connect() || e.is_timeout()) => {}
            Err(e) => return Err(SpseError::Failed(e.to_string())),
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    if resp.status == 401 || resp.status == 403 || looks_like_login(&resp.body) {
        return Err(SpseError::Expired);
    }
    if !(200..300).contains(&resp.status) {
        return Err(SpseError::Failed(format!(
            "SPSE DataTable gagal: HTTP {}",
            resp.status
        )));
    }
    let json: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    if !json.get("data").is_some_and(Value::is_array) {
        return Err(SpseError::Failed(
            "Response SPSE tidak valid (bukan DataTables JSON).".to_string(),
        ));
    }
    Ok(json)
}

fn pct_form(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// Session: status, simpan, cabut
// ---------------------------------------------------------------------------

fn session_json_iso(ts: Option<DateTime<Utc>>) -> Value {
    iso8601_utc(ts)
}

/// `GET /api/procurement/spse/status`.
pub async fn session_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let Some(session) = active_session(&state.pool, user.user_id).await? else {
        return Ok(Json(json!({
            "connected": false,
            "is_active": false,
            "message": "Belum ada session SPSE. Login manual di SPSE lalu kirim cookie.",
        })));
    };

    if let Some(_reason) = diagnose(&session).await {
        deactivate_session(&state.pool, session.id).await?;
        return Ok(Json(json!({
            "connected": false,
            "is_active": false,
            "message": "Session SPSE expired. Login ulang di SPSE.",
            "expired_at": session_json_iso(session.expires_at),
        })));
    }

    let now = Utc::now();
    sqlx::query("UPDATE tbl_spse_sessions SET last_validated_at = ?, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(now)
        .bind(session.id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "connected": true,
        "is_active": true,
        "lpse_slug": session.lpse_slug,
        "last_validated_at": session_json_iso(Some(now)),
        "expires_at": session_json_iso(session.expires_at),
        "message": "Session SPSE aktif.",
    })))
}

/// `POST /api/procurement/spse/session`.
pub async fn save_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut errors = Errors::default();

    let cookie_header = match body.get("cookie_header") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.chars().count() > 20_000 {
                errors.add(
                    "cookie_header",
                    "The cookie header must not be greater than 20000 characters.",
                );
            }
            Some(s.clone())
        }
        Some(_) => {
            errors.add("cookie_header", "The cookie header must be a string.");
            None
        }
    };

    let cookies_arr = match body.get("cookies") {
        None | Some(Value::Null) => None,
        Some(Value::Array(list)) => {
            for (i, c) in list.iter().enumerate() {
                validate_cookie_entry(&mut errors, i, c);
            }
            Some(list.clone())
        }
        Some(_) => {
            errors.add("cookies", "The cookies must be an array.");
            None
        }
    };

    let lpse_slug = match body.get("lpse_slug") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.chars().count() > 64 {
                errors.add(
                    "lpse_slug",
                    "The lpse slug must not be greater than 64 characters.",
                );
            }
            Some(s.clone())
        }
        Some(_) => {
            errors.add("lpse_slug", "The lpse slug must be a string.");
            None
        }
    };
    errors.finish()?;

    let header_empty = php_empty(cookie_header.as_deref());
    let cookies_empty = cookies_arr.as_ref().is_none_or(|l| l.is_empty());
    if header_empty && cookies_empty {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "cookie_header atau cookies wajib diisi.",
        ));
    }

    let cookies = parse_cookies(cookie_header.as_deref(), cookies_arr.as_deref());
    if cookies.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Cookie SPSE tidak valid atau kosong.",
        ));
    }
    if !cookies
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case("SPSE_SESSION"))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Cookie SPSE_SESSION wajib ada. Pastikan sudah login ke SPSE.",
        ));
    }

    let slug = match lpse_slug.filter(|s| !s.is_empty()) {
        Some(s) => s,
        None => default_slug(),
    };
    let candidate = Session {
        id: 0,
        lpse_slug: slug.clone(),
        cookies: cookies.clone(),
        expires_at: None,
    };
    // Validasi dulu: session lama tetap aktif bila yang baru tidak valid.
    if let Some(reason) = diagnose(&candidate).await {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Session SPSE tidak valid. {reason}"),
        ));
    }

    let key = app_key()?;
    let payload = crypt::encrypt_string(&key, &cookies_to_json(&cookies));
    let now = Utc::now();
    let expires = now + ChronoDuration::hours(SESSION_HOURS);

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE tbl_spse_sessions SET is_active = 0, updated_at = NOW() WHERE user_id = ?")
        .bind(user.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_spse_sessions (user_id, encrypted_cookies, lpse_slug, expires_at, last_validated_at, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(user.user_id)
    .bind(&payload)
    .bind(&slug)
    .bind(expires)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({
        "message": "Session SPSE tersimpan.",
        "session": {
            "id": id,
            "lpse_slug": slug,
            "expires_at": session_json_iso(Some(expires)),
        },
    }))
    .into_response())
}

/// Aturan `cookies.*` pada `saveSession`.
fn validate_cookie_entry(errors: &mut Errors, i: usize, c: &Value) {
    for (field, max) in [("name", 255usize), ("value", 5000usize)] {
        let key = format!("cookies.{i}.{field}");
        match c.get(field) {
            None | Some(Value::Null) => errors.add(
                &key,
                format!("The {key} field is required when cookies is present."),
            ),
            Some(Value::String(s)) if s.chars().count() > max => errors.add(
                &key,
                format!(
                    "The {} must not be greater than {max} characters.",
                    key.replace('_', " ")
                ),
            ),
            Some(Value::String(_)) => {}
            Some(_) => errors.add(&key, format!("The {key} must be a string.")),
        }
    }
    for (field, max) in [("domain", 255usize), ("path", 255usize)] {
        let key = format!("cookies.{i}.{field}");
        match c.get(field) {
            None | Some(Value::Null) | Some(Value::String(_)) => {}
            Some(_) => errors.add(&key, format!("The {key} must be a string.")),
        }
        if let Some(Value::String(s)) = c.get(field) {
            if s.chars().count() > max {
                errors.add(
                    &key,
                    format!("The {key} must not be greater than {max} characters."),
                );
            }
        }
    }
}

/// `DELETE /api/procurement/spse/session`.
pub async fn revoke_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    sqlx::query("UPDATE tbl_spse_sessions SET is_active = 0, updated_at = NOW() WHERE user_id = ?")
        .bind(user.user_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "message": "Session SPSE dihapus." })))
}

// ---------------------------------------------------------------------------
// Sinkron paket (SpseSyncService, ProcurementMatchingService)
// ---------------------------------------------------------------------------

/// Daftar pekerjaan untuk pencocokan nama, dimuat sekali per sinkron.
struct PekerjaanIndex {
    loaded: Option<Vec<(i64, String)>>,
}

impl PekerjaanIndex {
    fn new() -> Self {
        Self { loaded: None }
    }

    async fn all(&mut self, pool: &MySqlPool) -> Result<&[(i64, String)], sqlx::Error> {
        if self.loaded.is_none() {
            let rows = sqlx::query(
                "SELECT CAST(id AS SIGNED) AS id, nama_paket FROM tbl_pekerjaan ORDER BY id",
            )
            .fetch_all(pool)
            .await?;
            let mut list = Vec::with_capacity(rows.len());
            for r in rows {
                let nama: Option<String> = r.try_get("nama_paket")?;
                list.push((r.try_get("id")?, nama.unwrap_or_default()));
            }
            self.loaded = Some(list);
        }
        Ok(self.loaded.as_deref().unwrap_or(&[]))
    }
}

/// Hasil pencocokan satu baris staging.
struct MatchOutcome {
    status: &'static str,
    pekerjaan_id: Option<i64>,
    kontrak_id: Option<i64>,
}

/// Pekerjaan pertama dari pivot `kontrak_pekerjaan`, atau relasi lama `tbl_kontrak.id_pekerjaan`.
async fn pekerjaan_for_kontrak(
    pool: &MySqlPool,
    kontrak_id: i64,
    legacy_pekerjaan: Option<i64>,
) -> Result<Option<i64>, sqlx::Error> {
    let pivot: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(p.id AS SIGNED) FROM kontrak_pekerjaan kp \
         JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id \
         WHERE kp.kontrak_id = ? ORDER BY p.id LIMIT 1",
    )
    .bind(kontrak_id)
    .fetch_optional(pool)
    .await?;
    if pivot.is_some() {
        return Ok(pivot);
    }
    match legacy_pekerjaan {
        Some(pid) => {
            sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_pekerjaan WHERE id = ?")
                .bind(pid)
                .fetch_optional(pool)
                .await
        }
        None => Ok(None),
    }
}

/// Kontrak pertama dari pivot `kontrak_pekerjaan` untuk satu pekerjaan (`Pekerjaan::kontrak()->first()`).
async fn kontrak_for_pekerjaan(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT CAST(k.id AS SIGNED) FROM kontrak_pekerjaan kp \
         JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
         WHERE kp.pekerjaan_id = ? ORDER BY k.id LIMIT 1",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
}

/// `ProcurementMatchingService::matchStaging`: kode paket persis, lalu nama paket.
async fn match_staging(
    pool: &MySqlPool,
    index: &mut PekerjaanIndex,
    kode: &str,
    nama: &str,
) -> Result<MatchOutcome, sqlx::Error> {
    if !php_empty_or_blank(kode) {
        let kontrak: Option<(i64, Option<i64>)> = sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, CAST(id_pekerjaan AS SIGNED) AS id_pekerjaan \
             FROM tbl_kontrak WHERE kode_paket = ? ORDER BY id LIMIT 1",
        )
        .bind(kode)
        .fetch_optional(pool)
        .await?
        .map(|r| -> Result<(i64, Option<i64>), sqlx::Error> {
            Ok((r.try_get("id")?, r.try_get("id_pekerjaan")?))
        })
        .transpose()?;
        if let Some((kid, legacy)) = kontrak {
            let pid = pekerjaan_for_kontrak(pool, kid, legacy).await?;
            return Ok(MatchOutcome {
                status: "exact_kode_paket",
                pekerjaan_id: pid,
                kontrak_id: Some(kid),
            });
        }
    }

    let target = normalize_lookup(nama);
    if !target.is_empty() {
        let list = index.all(pool).await?;
        let exact = list.iter().find(|(_, n)| normalize_lookup(n) == target);
        let found = exact.or_else(|| {
            list.iter().find(|(_, n)| {
                let norm = normalize_lookup(n);
                !norm.is_empty() && (norm.contains(&target) || target.contains(&norm))
            })
        });
        if let Some((pid, _)) = found {
            let kid = kontrak_for_pekerjaan(pool, *pid).await?;
            return Ok(MatchOutcome {
                status: "fuzzy_nama_paket",
                pekerjaan_id: Some(*pid),
                kontrak_id: kid,
            });
        }
    }

    Ok(MatchOutcome {
        status: "unmatched",
        pekerjaan_id: None,
        kontrak_id: None,
    })
}

/// `trim($x) === ''` untuk kode paket.
fn php_empty_or_blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// `sync_run` dan semua halamannya. Mengembalikan id run.
async fn run_sync(
    pool: &MySqlPool,
    s: &Session,
    user_id: u64,
    page_length: u64,
) -> Result<i64, ApiError> {
    let run_id = sqlx::query(
        "INSERT INTO tbl_procurement_sync_runs (user_id, status, item_count, matched_count, started_at) \
         VALUES (?, 'running', 0, 0, NOW())",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;

    let mut index = PekerjaanIndex::new();
    let mut total_items: i64 = 0;
    let mut matched_count: i64 = 0;
    let mut errors: Vec<String> = Vec::new();

    'sources: for (jenis, endpoint, referer) in SOURCES {
        let mut seen: HashSet<String> = HashSet::new();
        let mut start: u64 = 0;
        let mut failure: Option<SpseError> = None;

        for draw in 1..=MAX_PAGES {
            let json = match fetch_datatable(s, endpoint, start, page_length, draw, referer).await {
                Ok(j) => j,
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            };
            let rows = json.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            for row in rows.iter().filter_map(Value::as_array) {
                let kode = php_str(row.first()).trim().to_string();
                if kode.is_empty() || !seen.insert(kode.clone()) {
                    continue;
                }
                match store_row(pool, &mut index, run_id, jenis, row).await {
                    Ok(status) => {
                        total_items += 1;
                        if status != "unmatched" {
                            matched_count += 1;
                        }
                    }
                    Err(e) => {
                        tracing::error!("procurement sync baris {kode}: {e}");
                        errors.push(format!("{jenis} [{kode}]: gagal menyimpan baris"));
                    }
                }
            }
            start += rows.len() as u64;
            let total = match json.get("recordsFiltered") {
                Some(Value::Null) | None => php_int(json.get("recordsTotal")),
                v => php_int(v),
            };
            if (rows.len() as u64) < page_length || (total > 0 && start >= total as u64) {
                break;
            }
            if draw == MAX_PAGES {
                failure = Some(SpseError::Failed(format!(
                    "Sync dihentikan: melebihi batas {MAX_PAGES} halaman, data mungkin belum lengkap."
                )));
            }
        }

        if let Some(e) = failure {
            if matches!(e, SpseError::Expired) {
                deactivate_session(pool, s.id).await?;
                errors.push(format!("{jenis}: {}", e.message()));
                break 'sources;
            }
            errors.push(format!("{jenis}: {}", e.message()));
        }
    }

    errors.truncate(ERROR_LOG_LIMIT);
    let status = if errors.is_empty() {
        "completed"
    } else if total_items > 0 {
        "partial"
    } else {
        "failed"
    };
    let error_log = (!errors.is_empty()).then(|| errors.join("\n"));
    sqlx::query(
        "UPDATE tbl_procurement_sync_runs SET status = ?, item_count = ?, matched_count = ?, error_log = ?, finished_at = NOW() WHERE id = ?",
    )
    .bind(status)
    .bind(total_items)
    .bind(matched_count)
    .bind(error_log)
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(internal)?;

    Ok(run_id)
}

/// `SpseSyncService::storeRow` dan pencocokan. Mengembalikan `match_status`.
async fn store_row(
    pool: &MySqlPool,
    index: &mut PekerjaanIndex,
    run_id: i64,
    jenis: &str,
    row: &[Value],
) -> Result<&'static str, sqlx::Error> {
    let kode = mb_take(php_str(row.first()).trim(), 32);
    let nama = mb_take(&clean_text(row.get(1)), 500);
    let isset = |i: usize| row.get(i).is_some_and(|v| !v.is_null());
    let status_paket = isset(2).then(|| mb_take(&clean_text(row.get(2)), 128));
    let metode = isset(5).then(|| mb_take(&clean_text(row.get(5)), 128));
    let raw = Value::Array(row.to_vec()).to_string();

    let m = match_staging(pool, index, &kode, &nama).await?;
    sqlx::query(
        "INSERT INTO tbl_procurement_staging_paket \
         (sync_run_id, sumber, jenis_paket, kode_paket, nama_paket, status_paket, metode_pengadaan, raw_row, fetched_at, \
          matched_pekerjaan_id, matched_kontrak_id, match_status) \
         VALUES (?, 'spse', ?, ?, ?, ?, ?, ?, NOW(), ?, ?, ?)",
    )
    .bind(run_id)
    .bind(jenis)
    .bind(&kode)
    .bind(&nama)
    .bind(status_paket)
    .bind(metode)
    .bind(raw)
    .bind(m.pekerjaan_id)
    .bind(m.kontrak_id)
    .bind(m.status)
    .execute(pool)
    .await?;
    Ok(m.status)
}

// ---------------------------------------------------------------------------
// Sinkron: rute
// ---------------------------------------------------------------------------

fn json_object(bytes: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v) if v.is_object() => v,
        _ => json!({}),
    }
}

/// `POST /api/procurement/spse/sync`.
pub async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let session = require_session(&state.pool, user.user_id).await?;

    let input = json_object(&body);
    let mut errors = Errors::default();
    let page_length = match input.get("page_length") {
        None | Some(Value::Null) => 100,
        Some(v) => int_field(&mut errors, "page_length", "page length", v, 1, Some(500)),
    };
    errors.finish()?;

    let run_id = run_sync(&state.pool, &session, user.user_id, page_length as u64).await?;
    let run = format_run_row(&state.pool, run_id).await?;
    Ok(Json(json!({ "message": "Sync SPSE selesai.", "run": run })).into_response())
}

/// Integer dengan aturan `integer|min|max` dan pesan Laravel. Mengembalikan 0 bila salah (error dicatat).
fn int_field(
    errors: &mut Errors,
    key: &str,
    attribute: &str,
    v: &Value,
    min: i64,
    max: Option<i64>,
) -> i64 {
    let parsed = match v {
        Value::Number(n) if n.is_i64() => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match parsed {
        None => {
            errors.add(key, format!("The {attribute} field must be an integer."));
            0
        }
        Some(n) if n < min => {
            errors.add(key, format!("The {attribute} field must be at least {min}."));
            n
        }
        Some(n) if max.is_some_and(|m| n > m) => {
            errors.add(
                key,
                format!(
                    "The {attribute} field must not be greater than {}.",
                    max.unwrap_or_default()
                ),
            );
            n
        }
        Some(n) => n,
    }
}

/// `formatRun`: id, status, hitungan, `error_log`, dan waktu ISO.
fn run_json(
    id: i64,
    status: &str,
    item_count: i64,
    matched_count: i64,
    error_log: Option<String>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
) -> Value {
    json!({
        "id": id,
        "status": status,
        "item_count": item_count,
        "matched_count": matched_count,
        "error_log": error_log,
        "started_at": iso8601_utc(started_at),
        "finished_at": iso8601_utc(finished_at),
    })
}

async fn format_run_row(pool: &MySqlPool, run_id: i64) -> Result<Value, ApiError> {
    let r = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, status, CAST(item_count AS SIGNED) AS item_count, \
         CAST(matched_count AS SIGNED) AS matched_count, error_log, started_at, finished_at \
         FROM tbl_procurement_sync_runs WHERE id = ?",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(run_row_json(&r).map_err(internal)?)
}

fn run_row_json(r: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    Ok(run_json(
        r.try_get("id")?,
        &r.try_get::<String, _>("status")?,
        r.try_get("item_count")?,
        r.try_get("matched_count")?,
        r.try_get("error_log")?,
        r.try_get("started_at")?,
        r.try_get("finished_at")?,
    ))
}

/// `GET /api/procurement/spse/sync/runs`: 20 run terakhir milik user.
pub async fn sync_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, status, CAST(item_count AS SIGNED) AS item_count, \
         CAST(matched_count AS SIGNED) AS matched_count, error_log, started_at, finished_at \
         FROM tbl_procurement_sync_runs WHERE user_id = ? ORDER BY id DESC LIMIT 20",
    )
    .bind(user.user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let data = rows
        .iter()
        .map(run_row_json)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    Ok(Json(json!({ "data": data })))
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

const STAGING_SELECT: &str = "SELECT CAST(s.id AS SIGNED) AS id, CAST(s.sync_run_id AS SIGNED) AS sync_run_id, s.sumber, \
     s.kode_paket, s.nama_paket, s.status_paket, s.metode_pengadaan, s.jenis_paket, \
     CAST(s.matched_pekerjaan_id AS SIGNED) AS matched_pekerjaan_id, CAST(s.matched_kontrak_id AS SIGNED) AS matched_kontrak_id, \
     s.match_status, CAST(s.raw_row AS CHAR) AS raw_row, s.fetched_at, \
     CAST(p.id AS SIGNED) AS p_id, p.nama_paket AS p_nama, \
     CAST(k.id AS SIGNED) AS k_id, k.kode_paket AS k_kode, k.spk AS k_spk \
     FROM tbl_procurement_staging_paket s \
     LEFT JOIN tbl_pekerjaan p ON p.id = s.matched_pekerjaan_id \
     LEFT JOIN tbl_kontrak k ON k.id = s.matched_kontrak_id";

fn raw_row_value(raw: Option<String>) -> Value {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

/// Satu baris staging dengan relasi `pekerjaan:id,nama_paket` dan `kontrak:id,kode_paket,spk`.
fn staging_item_json(r: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    let p_id: Option<i64> = r.try_get("p_id")?;
    let k_id: Option<i64> = r.try_get("k_id")?;
    Ok(json!({
        "id": r.try_get::<i64, _>("id")?,
        "sync_run_id": r.try_get::<i64, _>("sync_run_id")?,
        "sumber": r.try_get::<String, _>("sumber")?,
        "kode_paket": r.try_get::<String, _>("kode_paket")?,
        "nama_paket": r.try_get::<String, _>("nama_paket")?,
        "status_paket": r.try_get::<Option<String>, _>("status_paket")?,
        "metode_pengadaan": r.try_get::<Option<String>, _>("metode_pengadaan")?,
        "jenis_paket": r.try_get::<Option<String>, _>("jenis_paket")?,
        "matched_pekerjaan_id": r.try_get::<Option<i64>, _>("matched_pekerjaan_id")?,
        "matched_kontrak_id": r.try_get::<Option<i64>, _>("matched_kontrak_id")?,
        "match_status": r.try_get::<String, _>("match_status")?,
        "raw_row": raw_row_value(r.try_get("raw_row")?),
        "fetched_at": carbon_json(r.try_get("fetched_at")?),
        "pekerjaan": match p_id {
            Some(id) => json!({ "id": id, "nama_paket": r.try_get::<Option<String>, _>("p_nama")? }),
            None => Value::Null,
        },
        "kontrak": match k_id {
            Some(id) => json!({
                "id": id,
                "kode_paket": r.try_get::<Option<String>, _>("k_kode")?,
                "spk": r.try_get::<Option<String>, _>("k_spk")?,
            }),
            None => Value::Null,
        },
    }))
}

/// Pemilik run: `whereHas('syncRun', user_id)`.
async fn owned_staging_row(
    pool: &MySqlPool,
    user_id: u64,
    id: i64,
) -> Result<Option<sqlx::mysql::MySqlRow>, ApiError> {
    sqlx::query(&format!(
        "{STAGING_SELECT} WHERE s.id = ? AND s.sync_run_id IN (SELECT id FROM tbl_procurement_sync_runs WHERE user_id = ?)"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)
}

#[derive(Debug, Clone)]
enum Bind {
    Int(i64),
    Str(String),
}

fn apply_binds<'q>(
    mut q: sqlx::query::Query<'q, MySql, MySqlArguments>,
    binds: &[Bind],
) -> sqlx::query::Query<'q, MySql, MySqlArguments> {
    for b in binds {
        q = match b {
            Bind::Int(i) => q.bind(*i),
            Bind::Str(s) => q.bind(s.clone()),
        };
    }
    q
}

/// `GET /api/procurement/spse/staging`.
pub async fn staging(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let mut errors = Errors::default();

    let sync_run_id = match q.get("sync_run_id").filter(|v| !v.is_empty()) {
        None => None,
        Some(v) => match v.parse::<i64>() {
            Err(_) => {
                errors.add("sync_run_id", "The sync run id field must be an integer.");
                None
            }
            Ok(id) => {
                if !row_exists(pool, "tbl_procurement_sync_runs", id).await? {
                    errors.add("sync_run_id", "The selected sync run id is invalid.");
                }
                Some(id)
            }
        },
    };
    const MATCH_STATUSES: [&str; 5] = [
        "unmatched",
        "exact_kode_paket",
        "fuzzy_nama_paket",
        "manual_map",
        "promoted_draft",
    ];
    let match_status = q.get("match_status").filter(|v| !v.is_empty()).cloned();
    if let Some(m) = &match_status {
        if !MATCH_STATUSES.contains(&m.as_str()) {
            errors.add("match_status", "The selected match status is invalid.");
        }
    }
    let search = q.get("search").filter(|v| !v.is_empty()).cloned();
    if search.as_ref().is_some_and(|s| s.chars().count() > 200) {
        errors.add("search", "The search field must not be greater than 200 characters.");
    }
    let tahun = q.get("tahun").filter(|v| !v.is_empty()).cloned();
    if tahun.as_ref().is_some_and(|s| s.chars().count() > 4) {
        errors.add("tahun", "The tahun field must not be greater than 4 characters.");
    }
    let page = match q.get("page").filter(|v| !v.is_empty()) {
        None => 1,
        Some(v) => int_field(&mut errors, "page", "page", &Value::String(v.clone()), 1, None),
    };
    let per_page = match q.get("per_page").filter(|v| !v.is_empty()) {
        None => 20,
        Some(v) => int_field(
            &mut errors,
            "per_page",
            "per page",
            &Value::String(v.clone()),
            1,
            Some(200),
        ),
    };
    errors.finish()?;

    let mut where_sql = String::from(
        "s.sync_run_id IN (SELECT id FROM tbl_procurement_sync_runs WHERE user_id = ?)",
    );
    let mut binds = vec![Bind::Int(user.user_id as i64)];

    // Tanpa `sync_run_id`, pakai run terbaru milik user.
    let run_filter = match sync_run_id {
        Some(id) => Some(id),
        None => sqlx::query_scalar::<_, i64>(
            "SELECT CAST(id AS SIGNED) FROM tbl_procurement_sync_runs WHERE user_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(user.user_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?,
    };
    if let Some(id) = run_filter {
        where_sql.push_str(" AND s.sync_run_id = ?");
        binds.push(Bind::Int(id));
    }
    if let Some(m) = match_status {
        where_sql.push_str(" AND s.match_status = ?");
        binds.push(Bind::Str(m));
    }
    if let Some(s) = search {
        where_sql.push_str(" AND (s.nama_paket LIKE ? OR s.kode_paket LIKE ?)");
        binds.push(Bind::Str(format!("%{s}%")));
        binds.push(Bind::Str(format!("%{s}%")));
    }
    if let Some(t) = tahun {
        where_sql.push_str(
            " AND (s.match_status = 'unmatched' OR EXISTS (SELECT 1 FROM tbl_pekerjaan p2 \
             JOIN tbl_kegiatan k2 ON k2.id = p2.kegiatan_id \
             WHERE p2.id = s.matched_pekerjaan_id AND k2.tahun_anggaran = ?))",
        );
        binds.push(Bind::Str(t));
    }

    let total: i64 = apply_binds(
        sqlx::query(&format!(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_procurement_staging_paket s WHERE {where_sql}"
        )),
        &binds,
    )
    .fetch_one(pool)
    .await
    .map_err(internal)?
    .try_get(0)
    .map_err(internal)?;

    let params = PageParams {
        page: page as u64,
        per_page: per_page as u64,
    };
    let mut page_binds = binds.clone();
    page_binds.push(Bind::Int(per_page));
    page_binds.push(Bind::Int((page - 1) * per_page));
    let rows = apply_binds(
        sqlx::query(&format!(
            "{STAGING_SELECT} WHERE {where_sql} ORDER BY s.id DESC LIMIT ? OFFSET ?"
        )),
        &page_binds,
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let data = rows
        .iter()
        .map(staging_item_json)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    let base = format!("{}/api/procurement/spse/staging", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total as u64, params, &base)))
}

/// `GET /api/procurement/spse/staging/{id}`.
pub async fn staging_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let pool = &state.pool;
    let row = owned_staging_row(pool, user.user_id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;

    let kode: String = row.try_get("kode_paket").map_err(internal)?;
    let jenis: Option<String> = row.try_get("jenis_paket").map_err(internal)?;
    let path = if jenis.as_deref() == Some("tender_seleksi") {
        format!("/tender/{kode}")
    } else {
        format!("/nontender/{kode}")
    };
    let spse_url = format!("{}/{}{path}", spse_root(), default_slug());

    let matched_pekerjaan: Option<i64> = row.try_get("matched_pekerjaan_id").map_err(internal)?;
    let pekerjaan = match matched_pekerjaan {
        Some(pid) => pekerjaan_detail_json(pool, pid).await?,
        None => Value::Null,
    };

    let matched_kontrak: Option<i64> = row.try_get("matched_kontrak_id").map_err(internal)?;
    let kontrak_json = match matched_kontrak {
        Some(kid) => {
            let r = sqlx::query(
                "SELECT CAST(id AS SIGNED) AS id, kode_paket, spk, CAST(nilai_kontrak AS DOUBLE) AS nilai_kontrak, tgl_spk \
                 FROM tbl_kontrak WHERE id = ?",
            )
            .bind(kid)
            .fetch_optional(pool)
            .await
            .map_err(internal)?;
            match r {
                Some(r) => json!({
                    "id": r.try_get::<i64, _>("id").map_err(internal)?,
                    "kode_paket": r.try_get::<Option<String>, _>("kode_paket").map_err(internal)?,
                    "spk": r.try_get::<Option<String>, _>("spk").map_err(internal)?,
                    "nilai_kontrak": r.try_get::<Option<f64>, _>("nilai_kontrak").map_err(internal)?,
                    "tgl_spk": r.try_get::<Option<chrono::NaiveDate>, _>("tgl_spk").map_err(internal)?
                        .map(|d| d.format("%Y-%m-%d").to_string()),
                }),
                None => Value::Null,
            }
        }
        None => Value::Null,
    };

    let sync_run = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, status, CAST(item_count AS SIGNED) AS item_count, \
         CAST(matched_count AS SIGNED) AS matched_count, started_at, finished_at \
         FROM tbl_procurement_sync_runs WHERE id = ?",
    )
    .bind(row.try_get::<i64, _>("sync_run_id").map_err(internal)?)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    .map(|r| -> Result<Value, sqlx::Error> {
        // `error_log` tidak ikut dimuat di relasi, jadi selalu null (seperti Laravel).
        Ok(run_json(
            r.try_get("id")?,
            &r.try_get::<String, _>("status")?,
            r.try_get("item_count")?,
            r.try_get("matched_count")?,
            None,
            r.try_get("started_at")?,
            r.try_get("finished_at")?,
        ))
    })
    .transpose()
    .map_err(internal)?
    .unwrap_or(Value::Null);

    Ok(Json(json!({
        "data": {
            "id": id,
            "sync_run_id": row.try_get::<i64, _>("sync_run_id").map_err(internal)?,
            "sumber": row.try_get::<String, _>("sumber").map_err(internal)?,
            "kode_paket": kode,
            "nama_paket": row.try_get::<String, _>("nama_paket").map_err(internal)?,
            "status_paket": row.try_get::<Option<String>, _>("status_paket").map_err(internal)?,
            "metode_pengadaan": row.try_get::<Option<String>, _>("metode_pengadaan").map_err(internal)?,
            "jenis_paket": jenis,
            "matched_pekerjaan_id": matched_pekerjaan,
            "matched_kontrak_id": matched_kontrak,
            "match_status": row.try_get::<String, _>("match_status").map_err(internal)?,
            "raw_row": raw_row_value(row.try_get("raw_row").map_err(internal)?),
            "fetched_at": iso8601_utc(row.try_get("fetched_at").map_err(internal)?),
            "spse_url": spse_url,
            "pekerjaan": pekerjaan,
            "kontrak": kontrak_json,
            "sync_run": sync_run,
        },
    })))
}

/// Pekerjaan beserta kegiatan, kecamatan, dan desa untuk detail staging (`stagingDetail`).
async fn pekerjaan_detail_json(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Value, ApiError> {
    let r = sqlx::query(
        "SELECT CAST(p.id AS SIGNED) AS id, p.nama_paket, p.kode_rekening, CAST(p.pagu AS DOUBLE) AS pagu, \
         CAST(p.kegiatan_id AS SIGNED) AS kegiatan_id, CAST(p.kecamatan_id AS SIGNED) AS kecamatan_id, \
         CAST(p.desa_id AS SIGNED) AS desa_id FROM tbl_pekerjaan p WHERE p.id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = r else {
        return Ok(Value::Null);
    };

    let kegiatan_id: Option<i64> = r.try_get("kegiatan_id").map_err(internal)?;
    let kegiatan = match kegiatan_id {
        Some(kid) => sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, nama_kegiatan, tahun_anggaran FROM tbl_kegiatan WHERE id = ?",
        )
        .bind(kid)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|k| -> Result<Value, sqlx::Error> {
            Ok(json!({
                "id": k.try_get::<i64, _>("id")?,
                "nama_kegiatan": k.try_get::<Option<String>, _>("nama_kegiatan")?,
                "tahun_anggaran": k.try_get::<Option<String>, _>("tahun_anggaran")?,
            }))
        })
        .transpose()
        .map_err(internal)?
        .unwrap_or(Value::Null),
        None => Value::Null,
    };

    let kecamatan_id: Option<i64> = r.try_get("kecamatan_id").map_err(internal)?;
    let kecamatan = match kecamatan_id.filter(|id| *id > 0) {
        Some(kid) => sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, n_kec FROM tbl_kecamatan WHERE id = ?",
        )
        .bind(kid)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|k| -> Result<Value, sqlx::Error> {
            let n: Option<String> = k.try_get("n_kec")?;
            Ok(json!({
                "id": k.try_get::<i64, _>("id")?,
                "nama_kecamatan": n,
                "n_kec": n,
            }))
        })
        .transpose()
        .map_err(internal)?
        .unwrap_or(Value::Null),
        None => Value::Null,
    };

    let desa_id: Option<i64> = r.try_get("desa_id").map_err(internal)?;
    let desa = match desa_id.filter(|id| *id > 0) {
        Some(did) => sqlx::query("SELECT CAST(id AS SIGNED) AS id, n_desa FROM tbl_desa WHERE id = ?")
            .bind(did)
            .fetch_optional(pool)
            .await
            .map_err(internal)?
            .map(|d| -> Result<Value, sqlx::Error> {
                let n: Option<String> = d.try_get("n_desa")?;
                Ok(json!({
                    "id": d.try_get::<i64, _>("id")?,
                    "nama_desa": n,
                    "n_desa": n,
                }))
            })
            .transpose()
            .map_err(internal)?
            .unwrap_or(Value::Null),
        None => Value::Null,
    };

    Ok(json!({
        "id": r.try_get::<i64, _>("id").map_err(internal)?,
        "nama_paket": r.try_get::<Option<String>, _>("nama_paket").map_err(internal)?,
        "kode_rekening": r.try_get::<Option<String>, _>("kode_rekening").map_err(internal)?,
        "pagu": r.try_get::<f64, _>("pagu").map_err(internal)?,
        "kegiatan": kegiatan,
        "kecamatan": kecamatan,
        "desa": desa,
    }))
}

/// Nilai `boolean` Laravel: true/false, 1/0, "1"/"0", "true"/"false".
fn bool_field(errors: &mut Errors, key: &str, v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) if n.as_i64() == Some(1) => Some(true),
        Value::Number(n) if n.as_i64() == Some(0) => Some(false),
        Value::String(s) if s == "1" || s == "true" => Some(true),
        Value::String(s) if s == "0" || s == "false" => Some(false),
        _ => {
            errors.add(key, format!("The {} field must be true or false.", key.replace('_', " ")));
            None
        }
    }
}

/// Apakah baris dengan `id` ada di `table` (`exists:<table>,id`).
async fn row_exists(pool: &MySqlPool, table: &str, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(&format!(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(n > 0)
}

/// Daftar `ids` (wajib, minimal satu, `integer`, `exists:tbl_procurement_staging_paket,id`).
async fn staging_ids(
    pool: &MySqlPool,
    body: &Value,
    max: Option<usize>,
    errors: &mut Errors,
) -> Result<Vec<i64>, ApiError> {
    let list = match body.get("ids") {
        None | Some(Value::Null) => {
            errors.add("ids", "The ids field is required.");
            return Ok(Vec::new());
        }
        Some(Value::Array(a)) => a.clone(),
        Some(_) => {
            errors.add("ids", "The ids field must be an array.");
            return Ok(Vec::new());
        }
    };
    if list.is_empty() {
        errors.add("ids", "The ids field must have at least 1 items.");
    }
    if let Some(m) = max.filter(|m| list.len() > *m) {
        errors.add("ids", format!("The ids field must not have more than {m} items."));
    }
    let mut ids = Vec::new();
    for (i, v) in list.iter().enumerate() {
        let key = format!("ids.{i}");
        let parsed = match v {
            Value::Number(n) if n.is_i64() => n.as_i64(),
            Value::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        };
        let Some(id) = parsed else {
            errors.add(&key, format!("The {key} field must be an integer."));
            continue;
        };
        if !row_exists(pool, "tbl_procurement_staging_paket", id).await? {
            errors.add(&key, format!("The selected {key} is invalid."));
        }
        ids.push(id);
    }
    Ok(ids)
}

/// Baris staging milik user untuk daftar id, diurutkan id (`whereIn` + `whereHas`).
async fn owned_staging_for_ids(
    pool: &MySqlPool,
    user_id: u64,
    ids: &[i64],
) -> Result<Vec<sqlx::mysql::MySqlRow>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "{STAGING_SELECT} WHERE s.id IN ({placeholders}) AND s.sync_run_id IN (SELECT id FROM tbl_procurement_sync_runs WHERE user_id = ?) ORDER BY s.id"
    );
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(*id);
    }
    q.bind(user_id).fetch_all(pool).await.map_err(internal)
}

/// Kontrak untuk satu baris staging (`ProcurementMatchingService::applyToKontrak`).
async fn kontrak_for_staging(
    pool: &MySqlPool,
    matched_kontrak: Option<i64>,
    matched_pekerjaan: Option<i64>,
) -> Result<Option<kontrak::KontrakRow>, ApiError> {
    if let Some(kid) = matched_kontrak {
        if let Some(k) = kontrak::find_row(pool, kid).await.map_err(internal)? {
            return Ok(Some(k));
        }
    }
    if let Some(pid) = matched_pekerjaan {
        if let Some(kid) = kontrak_for_pekerjaan(pool, pid).await.map_err(internal)? {
            return kontrak::find_row(pool, kid).await.map_err(internal);
        }
    }
    Ok(None)
}

/// `POST /api/procurement/spse/staging/apply`.
pub async fn apply_staging(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let mut errors = Errors::default();
    let ids = staging_ids(pool, &body, None, &mut errors).await?;
    let overwrite = match body.get("overwrite") {
        None | Some(Value::Null) => false,
        Some(v) => bool_field(&mut errors, "overwrite", v).unwrap_or(false),
    };
    errors.finish()?;

    let items = owned_staging_for_ids(pool, user.user_id, &ids).await?;
    let mut applied = 0;
    let mut skipped = 0;
    let mut results: Vec<Value> = Vec::new();
    let url = full_url(&state, "/api/procurement/spse/staging/apply");

    for item in &items {
        let id: i64 = item.try_get("id").map_err(internal)?;
        let match_status: String = item.try_get("match_status").map_err(internal)?;
        let matched_pekerjaan: Option<i64> = item.try_get("matched_pekerjaan_id").map_err(internal)?;
        let matched_kontrak: Option<i64> = item.try_get("matched_kontrak_id").map_err(internal)?;

        if match_status == "unmatched" && matched_pekerjaan.is_none() {
            skipped += 1;
            results.push(json!({ "id": id, "status": "skipped", "reason": "unmatched" }));
            continue;
        }

        let kode: String = item.try_get("kode_paket").map_err(internal)?;
        let Some(kontrak) = kontrak_for_staging(pool, matched_kontrak, matched_pekerjaan).await? else {
            skipped += 1;
            results.push(json!({ "id": id, "status": "skipped", "reason": "no_kontrak" }));
            continue;
        };

        let kode_now = kontrak.kode_paket.clone();
        if (overwrite || php_empty(kode_now.as_deref())) && kode_now.as_deref() != Some(kode.as_str()) {
            let mut tx = pool.begin().await.map_err(internal)?;
            let before = kontrak::find_row(&mut *tx, kontrak.id).await.map_err(internal)?;
            sqlx::query("UPDATE tbl_kontrak SET kode_paket = ?, updated_at = NOW() WHERE id = ?")
                .bind(&kode)
                .bind(kontrak.id)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            let after = kontrak::find_row(&mut *tx, kontrak.id)
                .await
                .map_err(internal)?
                .ok_or_else(|| server_error("kontrak hilang setelah update"))?;
            let old_row = before.ok_or_else(|| server_error("kontrak hilang sebelum update"))?;
            let mut old = Map::new();
            old.insert("kode_paket".into(), json!(old_row.kode_paket));
            old.insert("updated_at".into(), carbon_json(old_row.updated_at));
            let mut new = Map::new();
            new.insert("kode_paket".into(), json!(after.kode_paket));
            new.insert("updated_at".into(), carbon_json(after.updated_at));
            changes::log(
                &mut tx,
                &headers,
                user.user_id,
                &changes::KONTRAK,
                "updated",
                kontrak.id,
                Some(old),
                Some(new),
                after.id_pekerjaan,
                &url,
            )
            .await?;
            tx.commit().await.map_err(internal)?;
        }

        let final_kode = if overwrite || php_empty(kode_now.as_deref()) {
            Some(kode.clone())
        } else {
            kode_now
        };
        applied += 1;
        results.push(json!({
            "id": id,
            "status": "applied",
            "kontrak_id": kontrak.id,
            "kode_paket": final_kode,
        }));
    }

    Ok(Json(json!({
        "message": format!("Apply selesai: {applied} berhasil, {skipped} dilewati."),
        "applied": applied,
        "skipped": skipped,
        "results": results,
    }))
    .into_response())
}

/// `POST /api/procurement/spse/staging/map`.
pub async fn map_staging(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let mut errors = Errors::default();
    let staging_id = int_body(&mut errors, "id", &body, pool, "tbl_procurement_staging_paket").await?;
    let pekerjaan_id = int_body(&mut errors, "pekerjaan_id", &body, pool, "tbl_pekerjaan").await?;
    errors.finish()?;

    let owned: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_procurement_staging_paket WHERE id = ? AND sync_run_id IN (SELECT id FROM tbl_procurement_sync_runs WHERE user_id = ?)",
    )
    .bind(staging_id)
    .bind(user.user_id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    if owned == 0 {
        return Err(ApiError::not_found());
    }

    let kontrak_id = kontrak_for_pekerjaan(pool, pekerjaan_id).await.map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_procurement_staging_paket SET matched_pekerjaan_id = ?, matched_kontrak_id = ?, match_status = 'manual_map' WHERE id = ?",
    )
    .bind(pekerjaan_id)
    .bind(kontrak_id)
    .bind(staging_id)
    .execute(pool)
    .await
    .map_err(internal)?;

    let row = owned_staging_row(pool, user.user_id, staging_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({
        "message": "Mapping manual tersimpan.",
        "staging": staging_item_json(&row).map_err(internal)?,
    }))
    .into_response())
}

/// Bilangan bulat dari body dengan aturan `required|integer|exists:<tabel>,id`.
async fn int_body(
    errors: &mut Errors,
    key: &str,
    body: &Value,
    pool: &MySqlPool,
    table: &str,
) -> Result<i64, ApiError> {
    let v = match body.get(key) {
        None | Some(Value::Null) => {
            errors.add(key, format!("The {} field is required.", key.replace('_', " ")));
            return Ok(0);
        }
        Some(v) => v,
    };
    let parsed = match v {
        Value::Number(n) if n.is_i64() => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    let Some(id) = parsed else {
        errors.add(key, format!("The {} field must be an integer.", key.replace('_', " ")));
        return Ok(0);
    };
    if !row_exists(pool, table, id).await? {
        errors.add(key, format!("The selected {} is invalid.", key.replace('_', " ")));
    }
    Ok(id)
}

/// `POST /api/procurement/spse/staging/promote-draft`.
pub async fn promote_staging(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let mut errors = Errors::default();
    let ids = staging_ids(pool, &body, Some(50), &mut errors).await?;
    let kegiatan_id = match body.get("kegiatan_id") {
        None | Some(Value::Null) => None,
        Some(_) => Some(int_body(&mut errors, "kegiatan_id", &body, pool, "tbl_kegiatan").await?),
    };
    let is_konsultan = match body.get("is_konsultan") {
        None | Some(Value::Null) => true,
        Some(v) => bool_field(&mut errors, "is_konsultan", v).unwrap_or(true),
    };
    errors.finish()?;

    let items = owned_staging_for_ids(pool, user.user_id, &ids).await?;
    let url = full_url(&state, "/api/procurement/spse/staging/promote-draft");
    let mut created = 0;
    let mut skipped = 0;
    let mut results: Vec<Value> = Vec::new();

    for item in &items {
        let id: i64 = item.try_get("id").map_err(internal)?;
        let outcome = promote_one(pool, &headers, user.user_id, item, kegiatan_id, is_konsultan, &url).await;
        match outcome {
            Ok(Promoted::Created {
                pekerjaan_id,
                kontrak_id,
            }) => {
                created += 1;
                results.push(json!({
                    "id": id,
                    "status": "created",
                    "pekerjaan_id": pekerjaan_id,
                    "kontrak_id": kontrak_id,
                }));
            }
            Ok(Promoted::AlreadyMatched { pekerjaan_id }) => {
                skipped += 1;
                results.push(json!({
                    "id": id,
                    "status": "skipped",
                    "reason": "already_matched",
                    "pekerjaan_id": pekerjaan_id,
                }));
            }
            Err(e) => {
                skipped += 1;
                tracing::error!("promote staging {id}: {}", e.message);
                results.push(json!({
                    "id": id,
                    "status": "error",
                    "reason": "Gagal menyimpan draft pekerjaan.",
                }));
            }
        }
    }

    Ok(Json(json!({
        "message": format!("Promote draft: {created} dibuat, {skipped} dilewati."),
        "created": created,
        "skipped": skipped,
        "results": results,
    }))
    .into_response())
}

enum Promoted {
    Created {
        pekerjaan_id: i64,
        kontrak_id: i64,
    },
    AlreadyMatched {
        pekerjaan_id: Option<i64>,
    },
}

/// `ProcurementMatchingService::promoteToDraft` untuk satu baris, dalam satu transaksi.
#[allow(clippy::too_many_arguments)]
async fn promote_one(
    pool: &MySqlPool,
    headers: &HeaderMap,
    actor: u64,
    item: &sqlx::mysql::MySqlRow,
    kegiatan_id: Option<i64>,
    is_konsultan: bool,
    url: &str,
) -> Result<Promoted, ApiError> {
    let staging_id: i64 = item.try_get("id").map_err(internal)?;
    let matched_pekerjaan: Option<i64> = item.try_get("matched_pekerjaan_id").map_err(internal)?;
    if let Some(pid) = matched_pekerjaan {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_pekerjaan WHERE id = ?")
                .bind(pid)
                .fetch_optional(pool)
                .await
                .map_err(internal)?;
        return Ok(Promoted::AlreadyMatched { pekerjaan_id: exists });
    }

    let kode: String = item.try_get("kode_paket").map_err(internal)?;
    let nama: String = item.try_get("nama_paket").map_err(internal)?;
    let raw_row = raw_row_value(item.try_get("raw_row").map_err(internal)?);
    let raw_row_opt = (!raw_row.is_null()).then_some(raw_row);
    let kode_rup = data_get_str(raw_row_opt.as_ref(), "kode_rup")
        .or_else(|| data_get_str(raw_row_opt.as_ref(), "kodeRup"));
    let nama_pekerjaan = if php_empty(Some(nama.as_str())) {
        format!("Paket SPSE {kode}")
    } else {
        nama
    };
    let nama_pekerjaan = mb_take(&nama_pekerjaan, 225);
    let pagu: f64 = 0.0;

    let mut tx = pool.begin().await.map_err(internal)?;

    let pekerjaan_id = sqlx::query(
        "INSERT INTO tbl_pekerjaan (nama_paket, kode_rekening, kegiatan_id, pagu, is_konsultan, kecamatan_id, desa_id, created_at, updated_at) \
         VALUES (?, NULL, ?, ?, ?, NULL, NULL, NOW(), NOW())",
    )
    .bind(&nama_pekerjaan)
    .bind(kegiatan_id)
    .bind(pagu)
    .bind(is_konsultan)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let now = Utc::now();
    let attrs = json!({
        "id": pekerjaan_id,
        "nama_paket": nama_pekerjaan,
        "kode_rekening": null,
        "kegiatan_id": kegiatan_id,
        "pagu": pagu,
        "is_konsultan": is_konsultan,
        "kecamatan_id": null,
        "desa_id": null,
        "created_at": carbon_json(Some(now)),
        "updated_at": carbon_json(Some(now)),
    });
    changes::log_linked(
        &mut tx,
        headers,
        actor,
        &PEKERJAAN_TARGET,
        "created",
        pekerjaan_id,
        None,
        attrs.as_object().cloned(),
        Some(format!("/pekerjaan/{pekerjaan_id}")),
        url,
    )
    .await?;

    let kontrak_id = sqlx::query(
        "INSERT INTO tbl_kontrak (id_pekerjaan, id_kegiatan, kode_paket, kode_rup, nilai_kontrak, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(kegiatan_id)
    .bind(&kode)
    .bind(&kode_rup)
    .bind((pagu > 0.0).then_some(pagu))
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    changes::log(
        &mut tx,
        headers,
        actor,
        &changes::KONTRAK,
        "created",
        kontrak_id,
        None,
        json!({
            "id": kontrak_id,
            "id_pekerjaan": pekerjaan_id,
            "id_kegiatan": kegiatan_id,
            "kode_paket": kode,
            "kode_rup": kode_rup,
            "nilai_kontrak": (pagu > 0.0).then_some(pagu),
        })
        .as_object()
        .cloned(),
        Some(pekerjaan_id),
        url,
    )
    .await?;

    let draft_id = sqlx::query(
        "INSERT INTO tbl_draft_pekerjaan (pekerjaan_id, penyedia_id, nama_pelaksana, kode_rup, kode_paket, created_at, updated_at) \
         VALUES (?, NULL, NULL, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(&kode_rup)
    .bind(&kode)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    changes::audit_only(
        &mut tx,
        headers,
        actor,
        "App\\Models\\DraftPekerjaan",
        "created",
        draft_id,
        None,
        json!({
            "id": draft_id,
            "pekerjaan_id": pekerjaan_id,
            "penyedia_id": null,
            "nama_pelaksana": null,
            "kode_rup": kode_rup,
            "kode_paket": kode,
        })
        .as_object()
        .cloned(),
        url,
    )
    .await?;

    sqlx::query(
        "UPDATE tbl_procurement_staging_paket SET match_status = 'promoted_draft', matched_pekerjaan_id = ?, matched_kontrak_id = ? WHERE id = ?",
    )
    .bind(pekerjaan_id)
    .bind(kontrak_id)
    .bind(staging_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    tx.commit().await.map_err(internal)?;
    Ok(Promoted::Created {
        pekerjaan_id,
        kontrak_id,
    })
}
