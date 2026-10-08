use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;

/// Konfigurasi aplikasi, dibaca dari environment (dan `.env` lewat `dotenvy`
/// di binary). Nama variabel sama dengan `.env` yang dipakai Laravel.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_env: String,
    pub app_port: u16,
    pub request_timeout_secs: u64,
    pub body_limit_bytes: usize,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            app_env: env::var("APP_ENV").unwrap_or_else(|_| "production".to_string()),
            app_port: parse_env("APP_PORT", 8000),
            request_timeout_secs: parse_env("RUST_REQUEST_TIMEOUT_SECS", 30),
            body_limit_bytes: parse_env("RUST_BODY_LIMIT_BYTES", 10 * 1024 * 1024),
        }
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Error API dengan bentuk JSON yang sama dengan respon Laravel:
/// `{"message": "..."}`, dan `errors` untuk validasi (422).
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub errors: Option<BTreeMap<String, Vec<String>>>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            errors: None,
        }
    }

    /// 401, pesan sama dengan Laravel saat token tidak valid atau tidak ada.
    pub fn unauthenticated() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthenticated.")
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "Not Found.")
    }

    /// 422 dengan daftar error per field, bentuk sama dengan Laravel.
    pub fn validation(message: impl Into<String>, errors: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
            errors: Some(errors),
        }
    }

    pub fn body(&self) -> Value {
        match &self.errors {
            Some(errors) => json!({ "message": self.message, "errors": errors }),
            None => json!({ "message": self.message }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_body_matches_laravel_shape() {
        let err = ApiError::not_found();
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.body(), json!({ "message": "Not Found." }));
    }

    #[test]
    fn unauthenticated_body_matches_laravel_shape() {
        let err = ApiError::unauthenticated();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.body(), json!({ "message": "Unauthenticated." }));
    }

    #[test]
    fn validation_body_includes_errors_per_field() {
        let mut errors = BTreeMap::new();
        errors.insert(
            "nama".to_string(),
            vec!["Field nama wajib diisi.".to_string()],
        );
        let err = ApiError::validation("The nama field is required.", errors);
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            err.body(),
            json!({
                "message": "The nama field is required.",
                "errors": { "nama": ["Field nama wajib diisi."] }
            })
        );
    }
}
