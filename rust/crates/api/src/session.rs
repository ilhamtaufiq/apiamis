//! Sesi berbasis cookie, pengganti BFF (`arumanis_session`).
//!
//! Cookie berisi token Sanctum `id|plain`, sama seperti yang disimpan BFF.
//! Request boleh membawa `Authorization: Bearer <token>` atau cookie; Bearer lebih dulu.
//! Nilai cookie di-percent-decode karena BFF (Hono) menyimpan `|` sebagai `%7C`.

use axum::http::{header, HeaderMap, HeaderValue};

pub const DEFAULT_COOKIE_NAME: &str = "arumanis_session";
/// Cookie impersonasi milik BFF; ikut dihapus saat logout.
pub const IMPERSONATOR_COOKIE: &str = "arumanis_impersonator_session";
/// Sama dengan `maxAge` di BFF: 12 jam.
pub const MAX_AGE_SECS: u64 = 60 * 60 * 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCookie {
    pub name: String,
    /// Flag `Secure`. Produksi memakai `SESSION_COOKIE_SECURE=true`.
    pub secure: bool,
}

impl Default for SessionCookie {
    fn default() -> Self {
        Self {
            name: DEFAULT_COOKIE_NAME.to_string(),
            secure: false,
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
        }
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

/// Token dari `Authorization: Bearer` atau, bila tidak ada, dari cookie sesi.
pub fn token_from_headers(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    if let Some(bearer) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(bearer.to_string());
    }
    cookie_value(headers, cookie_name)
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
            token_from_headers(&h, "arumanis_session").as_deref(),
            Some("2|bearer")
        );
    }

    #[test]
    fn missing_or_empty_cookie_is_none() {
        assert_eq!(
            token_from_headers(&HeaderMap::new(), "arumanis_session"),
            None
        );
        let h = headers_with_cookie("arumanis_session=");
        assert_eq!(token_from_headers(&h, "arumanis_session"), None);
    }

    #[test]
    fn set_cookie_encodes_pipe_and_is_httponly_strict() {
        let cfg = SessionCookie {
            name: "arumanis_session".into(),
            secure: true,
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
