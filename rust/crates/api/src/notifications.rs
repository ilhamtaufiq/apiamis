//! Modul notifikasi, setara `NotificationController` (6 rute):
//!
//! - `GET    /api/notifications`                   index (paginasi 20, atau `unread_only=true` maks 50)
//! - `POST   /api/notifications/{id}/read`         markAsRead
//! - `POST   /api/notifications/mark-all-read`     markAllAsRead
//! - `POST   /api/notifications/broadcast`         sendBroadcast (admin)
//! - `GET    /api/notifications/broadcast-history` getBroadcastHistory (admin)
//! - `DELETE /api/notifications/broadcast/{id}`    deleteBroadcast (admin)
//!
//! Baris `notifications` ditulis dengan bentuk yang sama dengan `AppNotification::toArray()`
//! (lihat `notify.rs`). Rute admin diperiksa ulang di handler karena middleware permission
//! Rust tidak menganggap prefix `/notifications` sebagai admin-only.

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    lookup::carbon_json,
    notify::new_uuid,
    pagination::{self, PageParams},
    require_auth, AppState,
};

/// `notifiable_type` dan `type` seperti yang ditulis Laravel (backslash tunggal).
pub const NOTIFIABLE_TYPE: &str = r"App\Models\User";
pub const NOTIFICATION_TYPE: &str = r"App\Notifications\AppNotification";

/// `$user->notifications()->latest()->paginate(20)`. Laravel tidak membaca `per_page` di sini.
pub const INDEX_PER_PAGE: u64 = 20;
/// `$user->unreadNotifications()->latest()->take(50)` untuk `unread_only=true`.
pub const UNREAD_LIMIT: u64 = 50;
/// `BroadcastHistory::latest()->paginate(10)`. Tidak membaca `per_page`.
pub const HISTORY_PER_PAGE: u64 = 10;

pub const BROADCAST_TYPES: &[&str] = &["all", "single", "multiple"];
pub const NOTIFICATION_TYPES: &[&str] = &["info", "success", "warning", "error"];

/// Jumlah baris per INSERT multi-baris saat broadcast ke banyak user.
const INSERT_CHUNK: usize = 500;

const NOTIF_COLUMNS: &str =
    "id, type, notifiable_type, notifiable_id, data, read_at, created_at, updated_at";
const HISTORY_COLUMNS: &str = "id, title, message, type, notification_type, url, is_banner, \
     recipient_count, created_at, updated_at";

type Errors = BTreeMap<String, Vec<String>>;

/// Error 500 generik. Laravel membalas 500 `"Server Error"` untuk semua exception yang
/// ditangkap controller ini, termasuk `ModelNotFoundException` (lihat `server_error_not_found`).
fn server_error() -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

/// Kesalahan database. Detail hanya masuk log, tidak dikirim ke klien.
fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("notifications: {e}");
    server_error()
}

/// Notifikasi atau broadcast yang tidak ditemukan. Laravel membalas 500, bukan 404,
/// karena `findOrFail` melempar exception yang ditangkap `catch (\Exception)`.
fn not_found_as_server_error() -> ApiError {
    server_error()
}

/// Admin: user yang memiliki role bernama `admin`.
pub async fn require_admin(pool: &MySqlPool, user_id: u64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(pool, user_id)
        .await
        .map_err(internal)?;
    if roles.iter().any(|(_, n)| n == "admin") {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Akses ditolak. Route ini hanya dapat diakses oleh admin.",
        ))
    }
}

/// Bentuk `DatabaseNotification` pada JSON. `data` memakai cast `array`: JSON tidak valid
/// menjadi null, seperti `json_decode` di Laravel.
fn notification_json(r: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    let data: String = r.try_get("data")?;
    let read_at: Option<DateTime<Utc>> = r.try_get("read_at")?;
    let created: Option<DateTime<Utc>> = r.try_get("created_at")?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at")?;
    Ok(json!({
        "id": r.try_get::<String, _>("id")?,
        "type": r.try_get::<String, _>("type")?,
        "notifiable_type": r.try_get::<String, _>("notifiable_type")?,
        "notifiable_id": r.try_get::<u64, _>("notifiable_id")?,
        "data": serde_json::from_str::<Value>(&data).unwrap_or(Value::Null),
        "read_at": carbon_json(read_at),
        "created_at": carbon_json(created),
        "updated_at": carbon_json(updated),
    }))
}

/// Satu halaman notifikasi milik user, terbaru dulu.
///
/// Laravel `latest()` hanya mengurutkan `created_at`. `id` ditambahkan sebagai pemutus seri
/// supaya urutan stabil untuk notifikasi yang dibuat dalam detik yang sama.
pub async fn list_page(
    pool: &MySqlPool,
    user_id: u64,
    page: u64,
    per_page: u64,
) -> Result<(Vec<Value>, u64), sqlx::Error> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_type = ? AND notifiable_id = ?",
    )
    .bind(NOTIFIABLE_TYPE)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    let sql = format!(
        "SELECT {NOTIF_COLUMNS} FROM notifications WHERE notifiable_type = ? AND notifiable_id = ? \
         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query(&sql)
        .bind(NOTIFIABLE_TYPE)
        .bind(user_id)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;
    let items = rows
        .iter()
        .map(notification_json)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total as u64))
}

/// Notifikasi belum dibaca, maksimal `UNREAD_LIMIT`, tanpa paginasi.
pub async fn unread_list(pool: &MySqlPool, user_id: u64) -> Result<Vec<Value>, sqlx::Error> {
    let sql = format!(
        "SELECT {NOTIF_COLUMNS} FROM notifications WHERE notifiable_type = ? AND notifiable_id = ? \
         AND read_at IS NULL ORDER BY created_at DESC, id DESC LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(NOTIFIABLE_TYPE)
        .bind(user_id)
        .bind(UNREAD_LIMIT)
        .fetch_all(pool)
        .await?;
    rows.iter().map(notification_json).collect()
}

pub async fn unread_count(pool: &MySqlPool, user_id: u64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_type = ? AND notifiable_id = ? \
         AND read_at IS NULL",
    )
    .bind(NOTIFIABLE_TYPE)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// `markAsRead`: notifikasi harus milik user. Notifikasi yang sudah dibaca tidak diubah
/// (`DatabaseNotification::markAsRead` hanya menyimpan bila `read_at` masih null).
pub async fn mark_read(pool: &MySqlPool, user_id: u64, id: &str) -> Result<(), ApiError> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT id FROM notifications WHERE id = ? AND notifiable_type = ? AND notifiable_id = ?",
    )
    .bind(id)
    .bind(NOTIFIABLE_TYPE)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    if found.is_none() {
        return Err(not_found_as_server_error());
    }
    sqlx::query(
        "UPDATE notifications SET read_at = NOW(), updated_at = NOW() WHERE id = ? \
         AND notifiable_type = ? AND notifiable_id = ? AND read_at IS NULL",
    )
    .bind(id)
    .bind(NOTIFIABLE_TYPE)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(())
}

/// `markAllAsRead`: hanya baris yang belum dibaca milik user.
pub async fn mark_all_read(pool: &MySqlPool, user_id: u64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notifications SET read_at = NOW(), updated_at = NOW() WHERE notifiable_type = ? \
         AND notifiable_id = ? AND read_at IS NULL",
    )
    .bind(NOTIFIABLE_TYPE)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Broadcast
// ---------------------------------------------------------------------------

/// Input `sendBroadcast` yang sudah lolos validasi bentuk. `user_ids` berisi id yang
/// belum dicek keberadaannya di tabel `users`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broadcast {
    pub title: String,
    pub message: String,
    /// `all`, `single`, atau `multiple`. Disimpan ke kolom `type`.
    pub kind: String,
    pub user_ids: Vec<u64>,
    pub notification_type: String,
    pub url: Option<String>,
    pub is_banner: bool,
}

fn push_error(errors: &mut Errors, field: &str, message: String) {
    errors.entry(field.to_string()).or_default().push(message);
}

/// Nama atribut untuk pesan validasi Laravel: `user_ids` menjadi "user ids".
fn label(key: &str) -> String {
    key.replace('_', " ")
}

/// `ConvertEmptyStringsToNull`: string kosong diperlakukan sebagai tidak ada.
fn present<'a>(input: &'a Value, key: &str) -> Option<&'a Value> {
    match input.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(v) => Some(v),
    }
}

/// `required|string|max:n`.
fn required_str(
    input: &Value,
    key: &str,
    max: Option<usize>,
    errors: &mut Errors,
) -> Option<String> {
    match present(input, key) {
        None => {
            push_error(
                errors,
                key,
                format!("The {} field is required.", label(key)),
            );
            None
        }
        Some(Value::String(s)) if s.trim().is_empty() => {
            push_error(
                errors,
                key,
                format!("The {} field is required.", label(key)),
            );
            None
        }
        Some(Value::String(s)) => {
            if max.is_some_and(|m| s.chars().count() > m) {
                push_error(
                    errors,
                    key,
                    format!(
                        "The {} field must not be greater than {} characters.",
                        label(key),
                        max.unwrap_or_default()
                    ),
                );
            }
            Some(s.clone())
        }
        Some(_) => {
            push_error(
                errors,
                key,
                format!("The {} field must be a string.", label(key)),
            );
            None
        }
    }
}

/// Field `in:...` yang wajib.
fn required_in(input: &Value, key: &str, allowed: &[&str], errors: &mut Errors) -> Option<String> {
    match present(input, key) {
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => Some(s.clone()),
        _ => {
            if present(input, key).is_none() {
                push_error(
                    errors,
                    key,
                    format!("The {} field is required.", label(key)),
                );
            } else {
                push_error(
                    errors,
                    key,
                    format!("The selected {} is invalid.", label(key)),
                );
            }
            None
        }
    }
}

/// Id user dari angka JSON atau string berisi digit, seperti `exists:users,id` pada input.
fn parse_user_id(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    }
}

/// Aturan `boolean` Laravel: `true`, `false`, `1`, `0`, `"1"`, `"0"`.
fn parse_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) if n.as_u64() == Some(1) => Some(true),
        Value::Number(n) if n.as_u64() == Some(0) => Some(false),
        Value::String(s) if s == "1" => Some(true),
        Value::String(s) if s == "0" => Some(false),
        _ => None,
    }
}

/// Validasi `sendBroadcast` tanpa akses database. Pesan mengikuti default bahasa Inggris
/// Laravel; `vendor/` tidak tersedia, jadi teks ini belum dibandingkan dengan Laravel.
pub fn parse_broadcast(input: &Value) -> Result<Broadcast, Errors> {
    let mut errors = Errors::new();

    let title = required_str(input, "title", Some(255), &mut errors);
    let message = required_str(input, "message", None, &mut errors);
    let kind = required_in(input, "type", BROADCAST_TYPES, &mut errors);
    let needs_ids = matches!(kind.as_deref(), Some("single" | "multiple"));

    let mut user_ids = Vec::new();
    match present(input, "user_ids") {
        None => {
            if needs_ids {
                push_error(
                    &mut errors,
                    "user_ids",
                    "The user ids field is required when type is single, multiple.".to_string(),
                );
            }
        }
        Some(Value::Array(items)) => {
            if items.is_empty() && needs_ids {
                push_error(
                    &mut errors,
                    "user_ids",
                    "The user ids field is required when type is single, multiple.".to_string(),
                );
            }
            for (i, item) in items.iter().enumerate() {
                match parse_user_id(item) {
                    Some(id) => user_ids.push(id),
                    None => push_error(
                        &mut errors,
                        &format!("user_ids.{i}"),
                        format!("The selected user_ids.{i} is invalid."),
                    ),
                }
            }
        }
        Some(_) => push_error(
            &mut errors,
            "user_ids",
            "The user ids field must be an array.".to_string(),
        ),
    }

    let notification_type = match present(input, "notification_type") {
        None => Some("info".to_string()),
        Some(Value::String(s)) if NOTIFICATION_TYPES.contains(&s.as_str()) => Some(s.clone()),
        Some(_) => {
            push_error(
                &mut errors,
                "notification_type",
                "The selected notification type is invalid.".to_string(),
            );
            None
        }
    };

    // Batas 255 mengikuti kolom `url varchar(255)`. Laravel tidak memvalidasinya, dan insert
    // yang melebihi batas gagal di database (500). Di sini dijadikan 422.
    let url = match present(input, "url") {
        None => None,
        Some(Value::String(s)) => {
            if s.chars().count() > 255 {
                push_error(
                    &mut errors,
                    "url",
                    "The url field must not be greater than 255 characters.".to_string(),
                );
            }
            Some(s.clone())
        }
        Some(_) => {
            push_error(
                &mut errors,
                "url",
                "The url field must be a string.".to_string(),
            );
            None
        }
    };

    let is_banner = match input.get("is_banner") {
        None | Some(Value::Null) => Some(false),
        Some(Value::String(s)) if s.is_empty() => Some(false),
        Some(v) => match parse_bool(v) {
            Some(b) => Some(b),
            None => {
                push_error(
                    &mut errors,
                    "is_banner",
                    "The is banner field must be true or false.".to_string(),
                );
                None
            }
        },
    };

    if !errors.is_empty() {
        return Err(errors);
    }
    let (Some(title), Some(message), Some(kind), Some(notification_type), Some(is_banner)) =
        (title, message, kind, notification_type, is_banner)
    else {
        // Setiap `None` di atas sudah mencatat error, jadi jalur ini tidak tercapai.
        return Err(errors);
    };
    Ok(Broadcast {
        title,
        message,
        kind,
        user_ids,
        notification_type,
        url,
        is_banner,
    })
}

/// Id user yang ada di tabel `users`, urut id. Dipakai untuk `exists:users,id` dan penerima.
async fn users_in(pool: &MySqlPool, ids: &[u64]) -> Result<Vec<u64>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb = QueryBuilder::<MySql>::new("SELECT id FROM users WHERE id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(*id);
    }
    sep.push_unseparated(") ORDER BY id");
    qb.build_query_scalar::<u64>().fetch_all(pool).await
}

/// Penerima: semua user bila `type` = `all`, selain itu user pada `user_ids` (tanpa duplikat).
async fn resolve_recipients(pool: &MySqlPool, b: &Broadcast) -> Result<Vec<u64>, sqlx::Error> {
    if b.kind == "all" {
        sqlx::query_scalar("SELECT id FROM users ORDER BY id")
            .fetch_all(pool)
            .await
    } else {
        users_in(pool, &b.user_ids).await
    }
}

/// `POST /api/notifications/broadcast` tanpa lapisan HTTP. Mengembalikan body respon sukses.
pub async fn send_broadcast_for(pool: &MySqlPool, input: &Value) -> Result<Value, ApiError> {
    let b = match parse_broadcast(input) {
        Ok(b) => b,
        Err(errors) => return Err(ApiError::validation("Validation error", errors)),
    };

    if !b.user_ids.is_empty() {
        let existing = users_in(pool, &b.user_ids).await.map_err(internal)?;
        let mut errors = Errors::new();
        for (i, id) in b.user_ids.iter().enumerate() {
            if !existing.contains(id) {
                push_error(
                    &mut errors,
                    &format!("user_ids.{i}"),
                    format!("The selected user_ids.{i} is invalid."),
                );
            }
        }
        if !errors.is_empty() {
            return Err(ApiError::validation("Validation error", errors));
        }
    }

    let recipients = resolve_recipients(pool, &b).await.map_err(internal)?;
    if recipients.is_empty() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "No recipients found"));
    }

    // Laravel menulis history lalu mengirim notifikasi tanpa transaksi. Di sini keduanya
    // dalam satu transaksi supaya tidak ada history yatim bila salah satu insert gagal.
    let mut tx = pool.begin().await.map_err(internal)?;
    let history = sqlx::query(
        "INSERT INTO broadcast_histories (title, message, type, notification_type, url, is_banner, \
         recipient_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(&b.title)
    .bind(&b.message)
    .bind(&b.kind)
    .bind(&b.notification_type)
    .bind(&b.url)
    .bind(b.is_banner)
    .bind(recipients.len() as u64)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let history_id = history.last_insert_id();

    // `AppNotification::toArray()`. Kunci `type` berisi `notification_type`, bukan `type` broadcast.
    let data = json!({
        "title": b.title,
        "message": b.message,
        "url": b.url,
        "type": b.notification_type,
        "is_banner": b.is_banner,
        "broadcast_history_id": history_id,
    })
    .to_string();

    for chunk in recipients.chunks(INSERT_CHUNK) {
        let mut qb = QueryBuilder::<MySql>::new(
            "INSERT INTO notifications (id, type, notifiable_type, notifiable_id, data, created_at, updated_at) ",
        );
        qb.push_values(chunk.iter(), |mut row, uid| {
            row.push_bind(new_uuid())
                .push_bind(NOTIFICATION_TYPE)
                .push_bind(NOTIFIABLE_TYPE)
                .push_bind(*uid)
                .push_bind(data.clone())
                .push_unseparated(", NOW(), NOW()");
        });
        qb.build().execute(&mut *tx).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;

    Ok(json!({
        "message": "Notification broadcasted successfully",
        "recipient_count": recipients.len(),
    }))
}

fn history_json(r: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    let created: Option<DateTime<Utc>> = r.try_get("created_at")?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at")?;
    Ok(json!({
        "id": r.try_get::<u64, _>("id")?,
        "title": r.try_get::<String, _>("title")?,
        "message": r.try_get::<String, _>("message")?,
        "type": r.try_get::<String, _>("type")?,
        "notification_type": r.try_get::<String, _>("notification_type")?,
        "url": r.try_get::<Option<String>, _>("url")?,
        "is_banner": r.try_get::<bool, _>("is_banner")?,
        "recipient_count": r.try_get::<i32, _>("recipient_count")?,
        "created_at": carbon_json(created),
        "updated_at": carbon_json(updated),
    }))
}

/// Satu halaman riwayat broadcast, terbaru dulu.
pub async fn history_page(
    pool: &MySqlPool,
    page: u64,
    per_page: u64,
) -> Result<(Vec<Value>, u64), sqlx::Error> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM broadcast_histories")
        .fetch_one(pool)
        .await?;
    let sql = format!(
        "SELECT {HISTORY_COLUMNS} FROM broadcast_histories \
         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query(&sql)
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(pool)
        .await?;
    let items = rows
        .iter()
        .map(history_json)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total as u64))
}

/// `deleteBroadcast`: hapus notifikasi yang menunjuk ke history ini, lalu history-nya.
pub async fn delete_broadcast_for(pool: &MySqlPool, id: &str) -> Result<Value, ApiError> {
    let Ok(history_id) = id.parse::<u64>() else {
        return Err(not_found_as_server_error());
    };
    let exists: Option<u64> = sqlx::query_scalar("SELECT id FROM broadcast_histories WHERE id = ?")
        .bind(history_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    if exists.is_none() {
        return Err(not_found_as_server_error());
    }

    let mut tx = pool.begin().await.map_err(internal)?;
    sqlx::query(
        "DELETE FROM notifications WHERE JSON_UNQUOTE(JSON_EXTRACT(data, '$.\"broadcast_history_id\"')) = ?",
    )
    .bind(history_id.to_string())
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("DELETE FROM broadcast_histories WHERE id = ?")
        .bind(history_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(json!({ "message": "Broadcast deleted successfully" }))
}

// ---------------------------------------------------------------------------
// Handler HTTP
// ---------------------------------------------------------------------------

/// `GET /api/notifications`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let unread_only = query.get("unread_only").is_some_and(|v| v == "true");

    let notifications = if unread_only {
        Value::Array(
            unread_list(&state.pool, user.user_id)
                .await
                .map_err(internal)?,
        )
    } else {
        let page = pagination::page_params(&query).page;
        let (rows, total) = list_page(&state.pool, user.user_id, page, INDEX_PER_PAGE)
            .await
            .map_err(internal)?;
        let base = format!("{}/api/notifications", state.app_url.trim_end_matches('/'));
        pagination::paginate(
            rows,
            total,
            PageParams {
                page,
                per_page: INDEX_PER_PAGE,
            },
            &base,
        )
    };
    let unread = unread_count(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "notifications": notifications,
        "unread_count": unread,
    })))
}

/// `POST /api/notifications/{id}/read`.
pub async fn mark_as_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    mark_read(&state.pool, user.user_id, &id).await?;
    Ok(Json(json!({ "message": "Notification marked as read" })))
}

/// `POST /api/notifications/mark-all-read`.
pub async fn mark_all_as_read(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    mark_all_read(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({ "message": "All notifications marked as read" }),
    ))
}

/// Body yang bukan objek JSON diperlakukan kosong, sehingga validasi tetap menghasilkan 422.
fn parse_body(body: &[u8]) -> Value {
    serde_json::from_slice::<Value>(body)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// `POST /api/notifications/broadcast` (admin). Pemeriksaan admin dilakukan sebelum validasi,
/// seperti middleware `role:admin` di Laravel.
pub async fn send_broadcast(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let input = parse_body(&body);
    Ok(Json(send_broadcast_for(&state.pool, &input).await?))
}

/// `GET /api/notifications/broadcast-history` (admin).
pub async fn broadcast_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let page = pagination::page_params(&query).page;
    let (rows, total) = history_page(&state.pool, page, HISTORY_PER_PAGE)
        .await
        .map_err(internal)?;
    let base = format!(
        "{}/api/notifications/broadcast-history",
        state.app_url.trim_end_matches('/')
    );
    Ok(Json(json!({
        "history": pagination::paginate(
            rows,
            total,
            PageParams {
                page,
                per_page: HISTORY_PER_PAGE,
            },
            &base,
        ),
    })))
}

/// `DELETE /api/notifications/broadcast/{id}` (admin).
pub async fn delete_broadcast(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    Ok(Json(delete_broadcast_for(&state.pool, &id).await?))
}
