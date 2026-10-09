//! Sesi berbasis cookie, pengganti BFF (`arumanis_session`).
//!
//! Cookie berisi token Sanctum `id|plain`, sama seperti yang disimpan BFF.
//! Request boleh membawa `Authorization: Bearer <token>` atau cookie; Bearer lebih dulu.
//! Nilai cookie di-percent-decode karena BFF (Hono) menyimpan `|` sebagai `%7C`.

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};
use shared::ApiError;

use crate::AppState;

pub const DEFAULT_COOKIE_NAME: &str = "arumanis_session";
/// Cookie httpOnly berisi token Sanctum untuk alur SSO browser (`ARUMANIS_AUTH_COOKIE`).
pub const DEFAULT_AUTH_COOKIE_NAME: &str = "arumanis_token";
/// Cookie impersonasi milik BFF; ikut dihapus saat logout.
pub const IMPERSONATOR_COOKIE: &str = "arumanis_impersonator_session";
/// Sama dengan `maxAge` di BFF: 12 jam.
pub const MAX_AGE_SECS: u64 = 60 * 60 * 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCookie {
    pub name: String,
    /// Flag `Secure`. Produksi memakai `SESSION_COOKIE_SECURE=true`.
    pub secure: bool,
    /// Nama cookie token browser (`ARUMANIS_AUTH_COOKIE`, default `arumanis_token`).
    pub auth_name: String,
    /// Flag `Secure` untuk cookie token: `SESSION_COOKIE_SECURE`, atau default true di production.
    pub auth_secure: bool,
    /// Umur cookie token dalam detik (`SANCTUM_TOKEN_EXPIRATION` menit, default 720).
    pub auth_max_age_secs: u64,
}

impl Default for SessionCookie {
    fn default() -> Self {
        Self {
            name: DEFAULT_COOKIE_NAME.to_string(),
            secure: false,
            auth_name: DEFAULT_AUTH_COOKIE_NAME.to_string(),
            auth_secure: false,
            auth_max_age_secs: MAX_AGE_SECS,
        }
    }
}

impl SessionCookie {
    /// `SESSION_COOKIE_NAME` dan `SESSION_COOKIE_SECURE`, dengan default seperti BFF.
    pub fn from_env() -> Self {
        Self {
            name: std::env::var("SESSION_COOKIE_NAME")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| DEFAULT_COOKIE_NAME.to_string()),
            secure: std::env::var("SESSION_COOKIE_SECURE")
                .map(|v| v.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            auth_name: std::env::var("ARUMANIS_AUTH_COOKIE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| DEFAULT_AUTH_COOKIE_NAME.to_string()),
            // Sama dengan config/sanctum.php: `SESSION_COOKIE_SECURE` lebih dulu, lalu APP_ENV.
            auth_secure: match std::env::var("SESSION_COOKIE_SECURE") {
                Ok(v) => v.trim().eq_ignore_ascii_case("true"),
                Err(_) => {
                    std::env::var("APP_ENV")
                        .unwrap_or_else(|_| "production".to_string())
                        .trim()
                        == "production"
                }
            },
            auth_max_age_secs: std::env::var("SANCTUM_TOKEN_EXPIRATION")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|m| *m > 0)
                .unwrap_or(720)
                * 60,
        }
    }

    fn auth_attrs(&self) -> String {
        let mut s = String::from("; Path=/; HttpOnly; SameSite=Lax");
        if self.auth_secure {
            s.push_str("; Secure");
        }
        s
    }

    /// Header `Set-Cookie` untuk `arumanis_token` setelah login, handoff, atau exchange.
    pub fn auth_set_header(&self, token: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!(
            "{}={}{}; Max-Age={}",
            self.auth_name,
            percent_encode(token),
            self.auth_attrs(),
            self.auth_max_age_secs
        ))
        .ok()
    }

    /// Header `Set-Cookie` untuk menghapus `arumanis_token` (logout).
    pub fn auth_clear_header(&self) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!("{}={}; Max-Age=0", self.auth_name, self.auth_attrs())).ok()
    }

    fn attrs(&self) -> String {
        let mut s = String::from("; Path=/; HttpOnly; SameSite=Strict");
        if self.secure {
            s.push_str("; Secure");
        }
        s
    }

    /// Header `Set-Cookie` untuk menyimpan token setelah login.
    pub fn set_header(&self, token: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!(
            "{}={}{}; Max-Age={}",
            self.name,
            percent_encode(token),
            self.attrs(),
            MAX_AGE_SECS
        ))
        .ok()
    }

    /// Header `Set-Cookie` untuk menghapus cookie (logout).
    pub fn clear_header(&self, name: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!("{name}={}; Max-Age=0", self.attrs())).ok()
    }
}

/// Token dari `Authorization: Bearer`, lalu cookie sesi, lalu cookie `arumanis_token`.
pub fn token_from_headers(headers: &HeaderMap, session: &SessionCookie) -> Option<String> {
    if let Some(bearer) = bearer_from_headers(headers) {
        return Some(bearer.to_string());
    }
    if let Some(token) = cookie_value(headers, &session.name) {
        return Some(token);
    }
    auth_cookie_token(headers, &session.auth_name)
}

/// Isi `Authorization: Bearer ...` bila ada.
pub fn bearer_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Token dari cookie `arumanis_token`. Hanya dibaca bila header `Authorization` tidak ada,
/// sehingga header yang sudah ada (walau bukan Bearer) tetap dipakai apa adanya.
pub fn auth_cookie_token(headers: &HeaderMap, name: &str) -> Option<String> {
    if headers.contains_key(header::AUTHORIZATION) {
        return None;
    }
    cookie_value(headers, name)
}

/// Header yang hanya bisa dikirim lewat fetch/XHR, bukan form lintas situs.
/// Sama dengan aturan CSRF di Laravel (`X-Arumanis-App`, `X-Requested-With`, JSON).
pub fn has_csrf_header(headers: &HeaderMap) -> bool {
    if headers.contains_key("x-arumanis-app") || headers.contains_key("x-requested-with") {
        return true;
    }
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("application/json"))
}

/// Middleware: request yang diautentikasi lewat cookie `arumanis_token` dan mengubah state
/// (POST, PUT, PATCH, DELETE) wajib membawa header CSRF. Jika tidak, 401 `Unauthenticated.`.
/// Bearer dan cookie sesi (`arumanis_session`) tidak terpengaruh.
pub async fn reject_cookie_csrf(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let cookie_only = !headers.contains_key(header::AUTHORIZATION)
        && cookie_value(headers, &state.session.name).is_none()
        && cookie_value(headers, &state.session.auth_name).is_some();
    let state_changing = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if cookie_only && state_changing && !has_csrf_header(headers) {
        return ApiError::unauthenticated().into_response();
    }
    next.run(req).await
}

/// Nilai satu cookie dari header `Cookie`, sudah di-decode.
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| percent_decode(v))
        .filter(|v| !v.is_empty())
}

fn percent_encode(value: &str) -> String {
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

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn headers_with_cookie(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn decodes_bff_cookie_with_encoded_pipe() {
        let h = headers_with_cookie("theme=dark; arumanis_session=2502%7Cabc123; x=1");
        assert_eq!(
            cookie_value(&h, "arumanis_session").as_deref(),
            Some("2502|abc123")
        );
    }

    #[test]
    fn bearer_wins_over_cookie() {
        let mut h = headers_with_cookie("arumanis_session=1%7Ccookie");
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer 2|bearer"),
        );
        assert_eq!(
            token_from_headers(&h, &SessionCookie::default()).as_deref(),
            Some("2|bearer")
        );
    }

    #[test]
    fn missing_or_empty_cookie_is_none() {
        let cfg = SessionCookie::default();
        assert_eq!(token_from_headers(&HeaderMap::new(), &cfg), None);
        let h = headers_with_cookie("arumanis_session=");
        assert_eq!(token_from_headers(&h, &cfg), None);
    }

    #[test]
    fn auth_cookie_used_only_without_authorization_header() {
        let cfg = SessionCookie::default();
        let h = headers_with_cookie("arumanis_token=3%7Cabc");
        assert_eq!(token_from_headers(&h, &cfg).as_deref(), Some("3|abc"));

        // Header Authorization yang bukan Bearer tidak membuka cookie token.
        let mut h = headers_with_cookie("arumanis_token=3%7Cabc");
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic xyz"));
        assert_eq!(token_from_headers(&h, &cfg), None);
    }

    #[test]
    fn auth_cookie_set_and_clear_attributes() {
        let cfg = SessionCookie::default();
        let s = cfg.auth_set_header("4|tok").unwrap();
        let s = s.to_str().unwrap();
        assert!(s.starts_with("arumanis_token=4%7Ctok;"));
        assert!(s.contains("HttpOnly"));
        assert!(s.contains("SameSite=Lax"));
        assert!(s.contains("Path=/"));
        assert!(s.contains("Max-Age=43200"));
        let c = cfg.auth_clear_header().unwrap();
        assert!(c.to_str().unwrap().starts_with("arumanis_token=;"));
        assert!(c.to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn set_cookie_encodes_pipe_and_is_httponly_strict() {
        let cfg = SessionCookie {
            name: "arumanis_session".into(),
            secure: true,
            ..SessionCookie::default()
        };
        let v = cfg.set_header("2|plain").unwrap();
        let s = v.to_str().unwrap();
        assert!(s.starts_with("arumanis_session=2%7Cplain;"));
        assert!(s.contains("HttpOnly"));
        assert!(s.contains("SameSite=Strict"));
        assert!(s.contains("Secure"));
        assert!(s.contains("Max-Age=43200"));
    }

    #[test]
    fn roundtrip_set_then_read() {
        let cfg = SessionCookie::default();
        let set = cfg.set_header("9|tok+en/x").unwrap();
        let pair = set.to_str().unwrap().split(';').next().unwrap().to_string();
        let h = headers_with_cookie(&pair);
        assert_eq!(cookie_value(&h, &cfg.name).as_deref(), Some("9|tok+en/x"));
    }

    #[test]
    fn clear_cookie_expires_immediately() {
        let cfg = SessionCookie::default();
        let v = cfg.clear_header("arumanis_session").unwrap();
        assert!(v.to_str().unwrap().contains("Max-Age=0"));
    }
}
