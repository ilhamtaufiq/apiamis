//! Audit log `tbl_audit_logs`, setara trait `Auditable` di Laravel.
//!
//! Kolom yang ditulis: user_id, event, auditable_type, auditable_id, old_values, new_values,
//! url, ip_address, user_agent. Timestamp diisi NOW().

use axum::http::{header, HeaderMap};
use serde_json::{Map, Value};
use sqlx::MySql;

/// Satu baris audit. `old`/`new` = `None` bila tidak ada (created: old null; deleted: new null).
pub struct Entry<'a> {
    pub actor: u64,
    pub event: &'a str,
    /// Nama kelas Laravel, mis. `App\Models\Pekerjaan`.
    pub auditable_type: &'a str,
    pub auditable_id: u64,
    pub old: Option<Map<String, Value>>,
    pub new: Option<Map<String, Value>>,
    pub url: &'a str,
}

/// `X-Forwarded-For` (Laravel `trustProxies('*')`) dan `User-Agent`.
pub fn client_info(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().chars().take(45).collect::<String>())
        .filter(|s| !s.is_empty());
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(255).collect::<String>());
    (ip, ua)
}

pub async fn write(
    tx: &mut sqlx::Transaction<'_, MySql>,
    entry: Entry<'_>,
    headers: &HeaderMap,
) -> Result<(), sqlx::Error> {
    let (ip, ua) = client_info(headers);
    let old = entry.old.map(|m| Value::Object(m).to_string());
    let new = entry.new.map(|m| Value::Object(m).to_string());
    sqlx::query(
        "INSERT INTO tbl_audit_logs (user_id, event, auditable_type, auditable_id, old_values, new_values, url, ip_address, user_agent, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(entry.actor)
    .bind(entry.event)
    .bind(entry.auditable_type)
    .bind(entry.auditable_id)
    .bind(old)
    .bind(new)
    .bind(entry.url)
    .bind(ip)
    .bind(ua)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
