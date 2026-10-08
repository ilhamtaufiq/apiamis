use std::env;

/// Konfigurasi aplikasi, dibaca dari environment (dan `.env` lewat `dotenvy`
/// di binary). Nama variabel sama dengan `.env` yang dipakai Laravel.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_env: String,
    pub app_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            app_env: env::var("APP_ENV").unwrap_or_else(|_| "production".to_string()),
            app_port: env::var("APP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8000),
        }
    }
}

/// Error API dengan bentuk JSON yang sama dengan respon Laravel: `{"message": "..."}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    pub fn not_found() -> Self {
        Self {
            status: 404,
            message: "Not Found.".to_string(),
        }
    }

    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({ "message": self.message })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_body_matches_laravel_shape() {
        let err = ApiError::not_found();
        assert_eq!(err.status, 404);
        assert_eq!(err.body(), serde_json::json!({ "message": "Not Found." }));
    }
}
