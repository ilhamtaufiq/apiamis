//! Laporan error klien untuk admin (`ClientErrorReportController` selain `store`): daftar, detail,
//! tandai selesai dan buka lagi, hapus massal, dan kosongkan semua. Tabel `error_logs`.
//! `POST /api/client-error-reports` (store) tetap di `quality_insight.rs`.
//!
//! Rute (semua admin, seperti `role:admin`):
//! - `GET /api/error-logs`: `index`. Filter `source`, `status` (`open`/`resolved`), `search`
//!   (`LIKE` di `message`, `url`, `source`), dan `per_page` (default 15, maks 100).
//! - `GET /api/error-logs/{id}`: `show`.
//! - `POST /api/error-logs/{id}/resolve` dan `/reopen`.
//! - `POST /api/error-logs/bulk/resolve`, `/bulk/reopen`, `/bulk/delete`, dan `DELETE /api/error-logs/bulk`.
//! - `POST /api/error-logs/empty` dan `DELETE /api/error-logs/empty`: hapus semua baris.
//!
//! Perbedaan yang diketahui dengan Laravel:
//! - Urutan daftar `created_at DESC, id DESC`. Laravel hanya `created_at DESC`, jadi urutan antar
//!   baris dengan waktu sama tidak ditentukan.
//! - `per_page` dibaca seperti `(int)` PHP, lalu dijepit ke 1..=100. Laravel memakai nilai mentah:
//!   0 atau negatif menghasilkan error (500). Di sini hasilnya halaman dengan 1 baris.
//! - Body hanya JSON. Body form-encoded tidak dibaca (sama dengan `store`).
//! - `resolved_at` dan `updated_at` memakai `NOW()` MySQL (zona sesi), sama dengan `store`.
//!   Laravel memakai `now()` UTC.
//! - Resolve dan reopen satu baris memperbarui `updated_at`. Update massal dan hapus tidak,
//!   sama dengan builder query Laravel. Reopen pada baris yang sudah terbuka tidak mengubah baris
//!   (Eloquent tidak menyimpan perubahan tanpa dirty).
//! - `user` pada JSON berisi kolom `users` kecuali `password` dan `remember_token` (`$hidden`).
//!   Kolom lain, termasuk `google_id`, ikut seperti di Laravel.
//! - Parameter `{errorLog}` hanya menerima angka (`whereNumber`). Selain itu 404.
//!   Untuk pengguna non-admin, 403 didahulukan sebelum pengecekan id.
//! - `DELETE /bulk` dan `DELETE /empty` memakai handler yang sama dengan POST, seperti di Laravel.

use std::collections::{BTreeSet, HashMap};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    lookup::carbon_json, notifications::require_admin, pagination, require_auth,
    validation::Errors, AppState,
};

const DEFAULT_PER_PAGE: i64 = 15;
const MAX_PER_PAGE: i64 = 100;

const SELECT_LOGS: &str = "SELECT CAST(e.id AS SIGNED) AS id, CAST(e.user_id AS SIGNED) AS user_id, \
     e.source, e.message, e.stack, e.component_stack, e.url, e.user_agent, e.ip_address, \
     CAST(e.metadata AS CHAR) AS metadata, e.resolved_at, e.created_at, e.updated_at \
     FROM error_logs e";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

struct LogRow {
    id: i64,
    user_id: Option<i64>,
    source: String,
    message: String,
    stack: Option<String>,
    component_stack: Option<String>,
    url: Option<String>,
    user_agent: Option<String>,
    ip_address: Option<String>,
    metadata: Option<String>,
    resolved_at: Option<DateTime<Utc>>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<LogRow, sqlx::Error> {
    Ok(LogRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        source: r.try_get("source")?,
        message: r.try_get("message")?,
        stack: r.try_get("stack")?,
        component_stack: r.try_get("component_stack")?,
        url: r.try_get("url")?,
        user_agent: r.try_get("user_agent")?,
        ip_address: r.try_get("ip_address")?,
        metadata: r.try_get("metadata")?,
        resolved_at: r.try_get("resolved_at")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Kolom `array` di Laravel: JSON didekode. Objek kosong menjadi `[]`, seperti PHP.
fn metadata_json(raw: &Option<String>) -> Value {
    match raw.as_deref().map(serde_json::from_str::<Value>) {
        Some(Ok(Value::Object(map))) if map.is_empty() => json!([]),
        Some(Ok(v)) => v,
        _ => Value::Null,
    }
}

/// Parameter `{errorLog}` dengan `whereNumber`: hanya digit.
fn route_id(raw: &str) -> Result<i64, ApiError> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::not_found());
    }
    raw.parse::<i64>().map_err(|_| ApiError::not_found())
}

/// Teks query setelah `TrimStrings`: kosong dihitung tidak ada (`ConvertEmptyStringsToNull`).
fn text(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query
        .get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `(int)` PHP untuk `per_page`: awalan angka setelah spasi, selain itu 0. Lalu dijepit ke 1..=100.
fn per_page_param(query: &HashMap<String, String>) -> u64 {
    let Some(raw) = query.get("per_page") else {
        return DEFAULT_PER_PAGE as u64;
    };
    let t = raw.trim_start();
    let bytes = t.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    let value = if end == digits_start {
        0
    } else {
        t[..end]
            .parse::<i64>()
            .unwrap_or(if t.starts_with('-') { i64::MIN } else { i64::MAX })
    };
    value.clamp(1, MAX_PER_PAGE) as u64
}

/// Nilai `ids.*` untuk aturan `integer`: angka JSON bulat, atau string berisi bilangan bulat.
fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Pengguna pemilik baris, dalam bentuk kolom `users` tanpa `password` dan `remember_token`.
async fn load_users(pool: &MySqlPool, rows: &[LogRow]) -> Result<HashMap<i64, Value>, ApiError> {
    let ids: Vec<i64> = rows
        .iter()
        .filter_map(|r| r.user_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut users = HashMap::new();
    if ids.is_empty() {
        return Ok(users);
    }
    let sql = format!(
        "SELECT CAST(id AS SIGNED) AS id, google_id, name, email, avatar, gender, nip, jabatan, \
         email_verified_at, created_at, updated_at FROM users WHERE id IN ({})",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    for r in q.fetch_all(pool).await.map_err(internal)? {
        let id: i64 = r.try_get("id").map_err(internal)?;
        users.insert(
            id,
            json!({
                "id": id,
                "google_id": r.try_get::<Option<String>, _>("google_id").map_err(internal)?,
                "name": r.try_get::<String, _>("name").map_err(internal)?,
                "email": r.try_get::<String, _>("email").map_err(internal)?,
                "avatar": r.try_get::<Option<String>, _>("avatar").map_err(internal)?,
                "gender": r.try_get::<Option<String>, _>("gender").map_err(internal)?,
                "nip": r.try_get::<Option<String>, _>("nip").map_err(internal)?,
                "jabatan": r.try_get::<Option<String>, _>("jabatan").map_err(internal)?,
                "email_verified_at": carbon_json(
                    r.try_get::<Option<DateTime<Utc>>, _>("email_verified_at").map_err(internal)?
                ),
                "created_at": carbon_json(
                    r.try_get::<Option<DateTime<Utc>>, _>("created_at").map_err(internal)?
                ),
                "updated_at": carbon_json(
                    r.try_get::<Option<DateTime<Utc>>, _>("updated_at").map_err(internal)?
                ),
            }),
        );
    }
    Ok(users)
}

/// Bentuk `ErrorLog` pada JSON: kolom tabel plus relasi `user` (null bila tidak ada).
fn resource(row: &LogRow, users: &HashMap<i64, Value>) -> Value {
    json!({
        "id": row.id,
        "user_id": row.user_id,
        "source": row.source,
        "message": row.message,
        "stack": row.stack,
        "component_stack": row.component_stack,
        "url": row.url,
        "user_agent": row.user_agent,
        "ip_address": row.ip_address,
        "metadata": metadata_json(&row.metadata),
        "resolved_at": carbon_json(row.resolved_at),
        "created_at": carbon_json(row.created_at),
        "updated_at": carbon_json(row.updated_at),
        "user": row.user_id.and_then(|u| users.get(&u).cloned()).unwrap_or(Value::Null),
    })
}

async fn find(pool: &MySqlPool, id: i64) -> Result<Option<LogRow>, ApiError> {
    let sql = format!("{SELECT_LOGS} WHERE e.id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_row(&r))
        .transpose()
        .map_err(internal)
}

/// Validasi `ids` seperti `required|array|min:1` dan `ids.*` (`integer`, `exists:error_logs,id`).
/// Mengembalikan id dalam urutan input (boleh duplikat).
async fn validated_ids(pool: &MySqlPool, body: &Value) -> Result<Vec<i64>, ApiError> {
    let mut errors = Errors::default();
    let items: Vec<Value> = match body.get("ids") {
        None | Some(Value::Null) => {
            errors.add("ids", "The ids field is required.");
            Vec::new()
        }
        Some(Value::Array(items)) => {
            if items.is_empty() {
                // `required` gagal untuk array kosong, dan `min:1` juga gagal.
                errors.add("ids", "The ids field is required.");
                errors.add("ids", "The ids field must have at least 1 items.");
            }
            items.clone()
        }
        Some(_) => {
            errors.add("ids", "The ids field must be an array.");
            Vec::new()
        }
    };

    let mut checked: Vec<(String, i64)> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        let key = format!("ids.{i}");
        match int_value(v) {
            Some(n) => checked.push((key, n)),
            None => errors.add(&key, format!("The {key} field must be an integer.")),
        }
    }

    let wanted: Vec<i64> = checked
        .iter()
        .map(|(_, n)| *n)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !wanted.is_empty() {
        let sql = format!(
            "SELECT CAST(id AS SIGNED) FROM error_logs WHERE id IN ({})",
            placeholders(wanted.len())
        );
        let mut q = sqlx::query_scalar::<_, i64>(&sql);
        for id in &wanted {
            q = q.bind(id);
        }
        let found: BTreeSet<i64> = q
            .fetch_all(pool)
            .await
            .map_err(internal)?
            .into_iter()
            .collect();
        for (key, n) in &checked {
            if !found.contains(n) {
                errors.add(key, format!("The selected {key} is invalid."));
            }
        }
    }

    errors.finish()?;
    Ok(checked.into_iter().map(|(_, n)| n).collect())
}

/// Body JSON sebagai nilai. Body kosong atau bukan JSON diperlakukan sebagai input kosong.
fn json_body(body: &Bytes) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

/// `GET /api/error-logs`: daftar terbaru dengan paginasi Laravel (`data` dan `meta`, tanpa `links`).
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let mut clauses: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(source) = text(&query, "source") {
        clauses.push("e.source = ?");
        binds.push(source);
    }
    match text(&query, "status").as_deref() {
        Some("resolved") => clauses.push("e.resolved_at IS NOT NULL"),
        Some("open") => clauses.push("e.resolved_at IS NULL"),
        _ => {}
    }
    if let Some(search) = text(&query, "search") {
        clauses.push("(e.message LIKE ? OR e.url LIKE ? OR e.source LIKE ?)");
        let like = format!("%{search}%");
        binds.extend([like.clone(), like.clone(), like]);
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    let per_page = per_page_param(&query);
    let page = pagination::page_params(&query).page;

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM error_logs e{where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(&state.pool).await.map_err(internal)?.max(0) as u64;

    let offset = (page - 1).saturating_mul(per_page);
    let list_sql = format!(
        "{SELECT_LOGS}{where_sql} ORDER BY e.created_at DESC, e.id DESC LIMIT {per_page} OFFSET {offset}"
    );
    let mut list_q = sqlx::query(&list_sql);
    for b in &binds {
        list_q = list_q.bind(b);
    }
    let rows: Vec<LogRow> = list_q
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<_, _>>()
        .map_err(internal)?;

    let users = load_users(&state.pool, &rows).await?;
    let data: Vec<Value> = rows.iter().map(|r| resource(r, &users)).collect();

    // `firstItem()` dan `lastItem()` bernilai null bila halaman ini kosong.
    let (from, to) = if rows.is_empty() {
        (Value::Null, Value::Null)
    } else {
        let from = offset + 1;
        (json!(from), json!(from + rows.len() as u64 - 1))
    };
    let last_page = total.div_ceil(per_page).max(1);
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": {
            "current_page": page,
            "last_page": last_page,
            "per_page": per_page,
            "total": total,
            "from": from,
            "to": to,
        },
    }))
    .into_response())
}

/// `GET /api/error-logs/{errorLog}`: detail satu laporan dengan `user`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id = route_id(&raw)?;
    let row = find(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
    let users = load_users(&state.pool, std::slice::from_ref(&row)).await?;
    Ok(Json(json!({ "success": true, "data": resource(&row, &users) })).into_response())
}

/// Resolve atau reopen satu baris. Resolve selalu mengisi `resolved_at` (walau sudah terisi),
/// seperti `update(['resolved_at' => now()])` di Laravel.
async fn set_resolved(pool: &MySqlPool, id: i64, resolve: bool) -> Result<Response, ApiError> {
    if find(pool, id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    let sql = if resolve {
        "UPDATE error_logs SET resolved_at = NOW(), updated_at = NOW() WHERE id = ?"
    } else {
        "UPDATE error_logs SET resolved_at = NULL, updated_at = NOW() \
         WHERE id = ? AND resolved_at IS NOT NULL"
    };
    sqlx::query(sql)
        .bind(id)
        .execute(pool)
        .await
        .map_err(internal)?;
    let row = find(pool, id).await?.ok_or_else(ApiError::not_found)?;
    let users = load_users(pool, std::slice::from_ref(&row)).await?;
    Ok(Json(json!({ "success": true, "data": resource(&row, &users) })).into_response())
}

/// `POST /api/error-logs/{errorLog}/resolve`
pub async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id = route_id(&raw)?;
    set_resolved(&state.pool, id, true).await
}

/// `POST /api/error-logs/{errorLog}/reopen`
pub async fn reopen(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id = route_id(&raw)?;
    set_resolved(&state.pool, id, false).await
}

/// `POST /api/error-logs/bulk/resolve`: hanya baris yang belum selesai. `affected` = jumlah baris berubah.
pub async fn bulk_resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let ids = validated_ids(&state.pool, &json_body(&body)).await?;
    let sql = format!(
        "UPDATE error_logs SET resolved_at = NOW() WHERE resolved_at IS NULL AND id IN ({})",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let affected = q
        .execute(&state.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    Ok(Json(json!({ "success": true, "affected": affected })).into_response())
}

/// `POST /api/error-logs/bulk/reopen`: hanya baris yang sudah selesai.
pub async fn bulk_reopen(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let ids = validated_ids(&state.pool, &json_body(&body)).await?;
    let sql = format!(
        "UPDATE error_logs SET resolved_at = NULL WHERE resolved_at IS NOT NULL AND id IN ({})",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let affected = q
        .execute(&state.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    Ok(Json(json!({ "success": true, "affected": affected })).into_response())
}

/// `POST /api/error-logs/bulk/delete` dan `DELETE /api/error-logs/bulk`: hapus permanen (tanpa soft delete).
pub async fn bulk_destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let ids = validated_ids(&state.pool, &json_body(&body)).await?;
    let sql = format!(
        "DELETE FROM error_logs WHERE id IN ({})",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let affected = q
        .execute(&state.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    Ok(Json(json!({ "success": true, "affected": affected })).into_response())
}

/// `POST /api/error-logs/empty` dan `DELETE /api/error-logs/empty`: hapus SEMUA baris `error_logs`.
pub async fn destroy_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let affected = sqlx::query("DELETE FROM error_logs")
        .execute(&state.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    Ok(Json(json!({ "success": true, "affected": affected })).into_response())
}
