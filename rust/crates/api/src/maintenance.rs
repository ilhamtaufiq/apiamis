//! Gerbang mode maintenance, setara `EnsureNotInMaintenance` dan `MaintenanceModeService`.
//!
//! Flag disimpan di `app_settings` (`maintenance_mode`). Saat aktif, hanya email
//! di daftar bypass (setting `maintenance_bypass_emails`, lalu env
//! `MAINTENANCE_BYPASS_EMAILS`, lalu default) dan path tertentu yang tetap boleh.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use sqlx::{MySqlPool, Row};

use crate::AppState;

pub const SETTING_KEY: &str = "maintenance_mode";
pub const BYPASS_SETTING_KEY: &str = "maintenance_bypass_emails";
/// Default dari `MaintenanceModeService::DEFAULT_BYPASS_EMAILS`.
pub const DEFAULT_BYPASS_EMAILS: &[&str] = &["ilhamtaufiq@gmail.com"];

/// Path (tanpa prefix `api/`) yang tetap bisa diakses saat maintenance.
pub const EXEMPT_PATHS: &[&str] = &[
    "app-settings/maintenance",
    "auth/login",
    "auth/logout",
    "auth/me",
    "auth/google",
    "auth/google/callback",
    "app-settings/backups/google-drive/callback",
    "auth/handoff",
    "auth/handoff/exchange",
    "up",
];

/// `isEnabled()`: nilai `1`, `true`, atau `on`.
pub fn is_enabled(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "on"))
}

/// Daftar email bypass. Setting database lebih dulu, lalu env, lalu default.
pub fn bypass_emails(setting: Option<&str>, env: Option<&str>) -> Vec<String> {
    let defaults = || {
        DEFAULT_BYPASS_EMAILS
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    let raw = setting
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| env.map(str::trim).filter(|s| !s.is_empty()));
    let Some(raw) = raw else {
        return defaults();
    };
    let emails: Vec<String> = raw
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    if emails.is_empty() {
        defaults()
    } else {
        emails
    }
}

pub fn allows_email(email: Option<&str>, list: &[String]) -> bool {
    match email.map(|e| e.trim().to_lowercase()) {
        Some(e) if !e.is_empty() => list.contains(&e),
        _ => false,
    }
}

/// `$request->path()` tanpa `/` di depan dan tanpa prefix `api/`.
pub fn is_exempt_path(raw_path: &str) -> bool {
    let trimmed = raw_path.trim_start_matches('/');
    let normalized = trimmed.strip_prefix("api/").unwrap_or(trimmed);
    EXEMPT_PATHS.contains(&normalized)
}

async fn setting(pool: &MySqlPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT `value` FROM app_settings WHERE `key` = ? LIMIT 1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => r.try_get("value"),
        None => Ok(None),
    }
}

async fn user_email(pool: &MySqlPool, user_id: u64) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT email FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => r.try_get("email"),
        None => Ok(None),
    }
}

/// Mengecek flag dan email bypass dari database.
pub async fn is_enabled_db(pool: &MySqlPool) -> Result<bool, sqlx::Error> {
    Ok(is_enabled(setting(pool, SETTING_KEY).await?.as_deref()))
}

pub async fn bypass_list_db(pool: &MySqlPool) -> Result<Vec<String>, sqlx::Error> {
    let from_setting = setting(pool, BYPASS_SETTING_KEY).await?;
    let from_env = std::env::var("MAINTENANCE_BYPASS_EMAILS").ok();
    Ok(bypass_emails(from_setting.as_deref(), from_env.as_deref()))
}

pub async fn check(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let raw_path = req.uri().path().to_string();
    if !raw_path.starts_with("/api/") || is_exempt_path(&raw_path) {
        return next.run(req).await;
    }

    match is_enabled_db(&state.pool).await {
        Ok(false) => return next.run(req).await,
        Ok(true) => {}
        Err(e) => return crate::desa::internal(e).into_response(),
    }

    if let Some(user_id) = bearer_user(&state, req.headers()).await {
        let allowed = match (
            user_email(&state.pool, user_id).await,
            bypass_list_db(&state.pool).await,
        ) {
            (Ok(email), Ok(list)) => allows_email(email.as_deref(), &list),
            (Err(e), _) | (_, Err(e)) => return crate::desa::internal(e).into_response(),
        };
        if allowed {
            return next.run(req).await;
        }
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "message": "Aplikasi sedang dalam mode maintenance. Coba lagi nanti.",
            "code": "MAINTENANCE_MODE",
            "maintenance": true,
        })),
    )
        .into_response()
}

/// User id dari token Bearer yang valid; `None` jika tidak ada atau tidak valid.
async fn bearer_user(state: &AppState, headers: &HeaderMap) -> Option<u64> {
    let token = crate::session::token_from_headers(headers, &state.session)?;
    auth::authenticate(&state.pool, &token)
        .await
        .ok()
        .map(|u| u.user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_flag_accepts_only_known_truthy_values() {
        assert!(is_enabled(Some("1")));
        assert!(is_enabled(Some("true")));
        assert!(is_enabled(Some("on")));
        assert!(!is_enabled(Some("0")));
        assert!(!is_enabled(Some("yes")));
        assert!(!is_enabled(None));
    }

    #[test]
    fn bypass_list_prefers_setting_then_env_then_default() {
        assert_eq!(
            bypass_emails(Some("A@x.id, b@y.id;c@z.id"), Some("env@x.id")),
            vec!["a@x.id", "b@y.id", "c@z.id"]
        );
        assert_eq!(
            bypass_emails(Some("  "), Some("env@x.id")),
            vec!["env@x.id"]
        );
        assert_eq!(bypass_emails(None, None), vec!["ilhamtaufiq@gmail.com"]);
        assert_eq!(
            bypass_emails(Some(" ,; "), None),
            vec!["ilhamtaufiq@gmail.com"]
        );
    }

    #[test]
    fn email_match_is_case_and_space_insensitive() {
        let list = vec!["ilhamtaufiq@gmail.com".to_string()];
        assert!(allows_email(Some(" IlhamTaufiq@Gmail.com "), &list));
        assert!(!allows_email(Some("orang@lain.id"), &list));
        assert!(!allows_email(None, &list));
        assert!(!allows_email(Some("  "), &list));
    }

    #[test]
    fn exempt_paths_match_laravel_list() {
        assert!(is_exempt_path("/api/auth/login"));
        assert!(is_exempt_path("/api/app-settings/maintenance"));
        assert!(is_exempt_path("/up"));
        assert!(!is_exempt_path("/api/kecamatan"));
        assert!(!is_exempt_path("/api/auth/login/extra"));
    }
}
