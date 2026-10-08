//! Pengguna yang sedang online (`UserPresenceController` dan `UserPresenceService`).
//!
//! Laravel menyimpan daftar ini di cache dengan kunci `presence:online_users`. Di sini daftar disimpan
//! di memori proses, dengan jendela 5 menit yang sama. Daftar hilang bila proses restart, dan tidak
//! dibagi dengan proses Laravel selama masa transisi.
//!
//! Field `office_*` divalidasi seperti Laravel, tetapi tidak disimpan: `UserPresenceService::heartbeat`
//! hanya menerima tiga argumen, jadi nilai office tidak pernah tersimpan di sana.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, OnceLock},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{format::iso8601_utc, require_auth, validation::Errors, AppState};

const ONLINE_WINDOW_MINUTES: i64 = 5;
const APPS: [&str; 2] = ["portal", "pengawasan"];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[derive(Clone)]
struct Entry {
    id: i64,
    name: String,
    email: String,
    avatar: Option<String>,
    gender: Option<String>,
    app: String,
    last_seen_at: DateTime<Utc>,
    koordinat: Option<String>,
    koordinat_at: Option<DateTime<Utc>>,
}

type Online = HashMap<i64, Entry>;

fn store() -> MutexGuard<'static, Online> {
    static STORE: OnceLock<Mutex<Online>> = OnceLock::new();
    STORE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// `pruneStale`: buang entri yang terakhir terlihat sebelum jendela online.
fn prune(users: &mut Online, now: DateTime<Utc>) {
    let cutoff = now - Duration::minutes(ONLINE_WINDOW_MINUTES);
    users.retain(|_, e| e.last_seen_at >= cutoff);
}

fn entry_json(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "name": e.name,
        "email": e.email,
        "avatar": e.avatar,
        "gender": e.gender,
        "app": e.app,
        "last_seen_at": iso8601_utc(Some(e.last_seen_at)),
        "koordinat": e.koordinat,
        "koordinat_at": iso8601_utc(e.koordinat_at),
    })
}

/// Aturan `koordinat`: `lat, lon` dengan desimal opsional dan spasi bebas di sekitar koma.
fn koordinat_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 2 {
        return false;
    }
    parts.iter().all(|part| {
        let t = part.trim();
        let t = t.strip_prefix('-').unwrap_or(t);
        let mut halves = t.splitn(2, '.');
        let whole = halves.next().unwrap_or("");
        let frac = halves.next();
        !whole.is_empty()
            && whole.chars().all(|c| c.is_ascii_digit())
            && frac.is_none_or(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
    })
}

/// `numeric|min:0|max:100` untuk `office_x` dan `office_y`.
fn office_coordinate(field: &str, value: &Value, errors: &mut Errors) {
    let parsed = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite());
    let attribute = field.replace('_', " ");
    match parsed {
        None => errors.add(field, format!("The {attribute} field must be a number.")),
        Some(v) if v < 0.0 => {
            errors.add(field, format!("The {attribute} field must be at least 0."))
        }
        Some(v) if v > 100.0 => errors.add(
            field,
            format!("The {attribute} field must not be greater than 100."),
        ),
        Some(_) => {}
    }
}

/// `POST /api/presence/heartbeat`: menandai pengguna online dan mencatat koordinat terakhir.
pub async fn heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let auth = require_auth(&state, &headers).await?;
    let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    let mut errors = Errors::default();
    let app_input = input.get("app").filter(|v| !v.is_null());
    if let Some(app) = app_input {
        match app.as_str() {
            Some(s) if s.chars().count() > 32 => errors.add(
                "app",
                "The app field must not be greater than 32 characters.",
            ),
            Some(_) => {}
            None => errors.add("app", "The app field must be a string."),
        }
    }
    let koordinat_input = input.get("koordinat").filter(|v| !v.is_null());
    if let Some(k) = koordinat_input {
        match k.as_str() {
            Some(s) if s.chars().count() > 64 => errors.add(
                "koordinat",
                "The koordinat field must not be greater than 64 characters.",
            ),
            // Regex Laravel berjalan pada nilai mentah: spasi di ujung membuatnya gagal.
            Some(s) if s != s.trim() || !koordinat_ok(s) => {
                errors.add("koordinat", "The koordinat field format is invalid.")
            }
            Some(_) => {}
            None => errors.add("koordinat", "The koordinat field must be a string."),
        }
    }
    if let Some(room) = input.get("office_room").filter(|v| !v.is_null()) {
        match room.as_str() {
            Some(s) if s.chars().count() > 32 => errors.add(
                "office_room",
                "The office room field must not be greater than 32 characters.",
            ),
            Some(_) => {}
            None => errors.add("office_room", "The office room field must be a string."),
        }
    }
    for field in ["office_x", "office_y"] {
        if let Some(v) = input.get(field).filter(|v| !v.is_null()) {
            office_coordinate(field, v, &mut errors);
        }
    }
    errors.finish()?;

    // `(string) ($validated['app'] ?? 'portal')`, lalu app yang tidak dikenal menjadi portal.
    let app = app_input
        .and_then(Value::as_str)
        .filter(|a| APPS.contains(a))
        .unwrap_or("portal")
        .to_string();
    let koordinat = koordinat_input
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let user = sqlx::query("SELECT name, email, avatar, gender FROM users WHERE id = ?")
        .bind(auth.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;

    let now = Utc::now();
    let mut online = store();
    let previous = online.get(&(auth.user_id as i64)).cloned();
    // `$koordinat ?? $previousKoordinat` dan waktunya hanya diperbarui bila koordinat dikirim.
    let koordinat_at = if koordinat.is_some() {
        Some(now)
    } else {
        previous.as_ref().and_then(|p| p.koordinat_at)
    };
    let koordinat = koordinat.or_else(|| previous.as_ref().and_then(|p| p.koordinat.clone()));
    online.insert(
        auth.user_id as i64,
        Entry {
            id: auth.user_id as i64,
            name: user.try_get("name").map_err(internal)?,
            email: user.try_get("email").map_err(internal)?,
            avatar: user.try_get("avatar").map_err(internal)?,
            gender: user.try_get("gender").map_err(internal)?,
            app,
            last_seen_at: now,
            koordinat,
            koordinat_at,
        },
    );
    prune(&mut online, now);
    drop(online);

    Ok(Json(json!({
        "data": { "ok": true, "online_window_minutes": ONLINE_WINDOW_MINUTES }
    }))
    .into_response())
}

/// `GET /api/presence/online`: pengguna yang masih dalam jendela online, terbaru di atas.
pub async fn online(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let now = Utc::now();
    let mut users: Vec<Entry> = {
        let mut online = store();
        prune(&mut online, now);
        online.values().cloned().collect()
    };
    users.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
    let data: Vec<Value> = users.iter().map(entry_json).collect();
    Ok(Json(json!({
        "data": data,
        "meta": { "online_window_minutes": ONLINE_WINDOW_MINUTES },
    }))
    .into_response())
}
