//! Izin Spatie (`PermissionController`, tabel `permissions`).
//!
//! Di `routes/api.php` baris 342-343, `apiResource('permissions')` didaftarkan dua kali. Pendaftaran
//! kedua identik dan tidak mengubah apa pun, jadi di Rust cukup satu set rute.
//!
//! Akses: grup rute hanya `auth:sanctum`. Pembatasan admin datang dari `CheckRoutePermission`
//! (`/permissions` ada di `ADMIN_ONLY_ROUTES`), yang di Rust dijalankan `route_permission::check`.
//! Karena itu handler hanya memeriksa login (tidak `require_admin`), supaya rule `route_permissions`
//! yang memberi akses ke non-admin tetap berlaku seperti di Laravel.
//!
//! Setiap penulisan (store, update, destroy) juga menghapus kunci cache Spatie
//! (`spatie.permission.cache` di tabel `cache`), seperti `RefreshesPermissionCache` dan
//! `forgetCachedPermissions`. Laravel dan Rust memakai prefix kunci yang sama.
//!
//! Catatan: `destroy` menghapus baris pivot secara eksplisit (`role_has_permissions`,
//! `model_has_permissions`) dalam satu transaksi, selain FK `ON DELETE CASCADE` yang sudah ada.

use std::collections::HashMap;

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
    auth_oauth::cache_prefix,
    lookup::carbon_json,
    pagination::{self, PageParams},
    require_auth,
    validation::Errors,
    AppState,
};

/// `Permission::query()->paginate(15)`: jumlah per halaman tetap.
const PER_PAGE: u64 = 15;
/// Guard yang dipakai `store` (`firstOrCreate` dengan `guard_name` = `web`).
const GUARD: &str = "web";
/// `permission.cache.key` di config Spatie.
const SPATIE_CACHE_KEY: &str = "spatie.permission.cache";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

struct PermissionRow {
    id: i64,
    name: String,
    guard_name: String,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<PermissionRow, sqlx::Error> {
    Ok(PermissionRow {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        guard_name: r.try_get("guard_name")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

const SELECT_PERMISSION: &str =
    "SELECT CAST(id AS SIGNED) AS id, name, guard_name, created_at, updated_at FROM permissions";

/// Bentuk JSON model `Permission` (tanpa atribut tersembunyi).
fn permission_json(p: &PermissionRow) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "guard_name": p.guard_name,
        "created_at": carbon_json(p.created_at),
        "updated_at": carbon_json(p.updated_at),
    })
}

async fn load(pool: &MySqlPool, id: i64) -> Result<Option<PermissionRow>, ApiError> {
    let sql = format!("{SELECT_PERMISSION} WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.map(|r| map_row(&r).map_err(internal)).transpose()
}

/// Route model binding `Permission $permission`: id tidak valid atau tidak ada berarti 404.
async fn load_or_404(pool: &MySqlPool, id: &str) -> Result<PermissionRow, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    load(pool, id).await?.ok_or_else(ApiError::not_found)
}

/// `Cache::forget` untuk kunci izin Spatie. Dipanggil setelah setiap penulisan yang berhasil.
pub(crate) async fn forget_permission_cache(pool: &MySqlPool) -> Result<(), ApiError> {
    let key = format!("{}{SPATIE_CACHE_KEY}", cache_prefix());
    sqlx::query("DELETE FROM `cache` WHERE `key` = ?")
        .bind(key)
        .execute(pool)
        .await
        .map_err(internal)?;
    Ok(())
}

fn parse_body(body: &Bytes) -> serde_json::Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// `required|string` setelah `TrimStrings` dan `ConvertEmptyStringsToNull`:
/// spasi saja dianggap kosong, dan nama yang disimpan sudah di-trim.
fn name_field(e: &mut Errors, input: &serde_json::Map<String, Value>) -> Option<String> {
    match input.get("name") {
        None | Some(Value::Null) => {
            e.add("name", "The name field is required.");
            None
        }
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                e.add("name", "The name field is required.");
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(_) => {
            e.add("name", "The name field must be a string.");
            None
        }
    }
}

/// `unique:permissions,name[,id]`. `except` adalah id yang dikecualikan. Id mulai dari 1,
/// jadi `0` berarti tidak ada yang dikecualikan.
async fn name_taken(pool: &MySqlPool, name: &str, except: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM permissions WHERE name = ? AND id <> ?",
    )
    .bind(name)
    .bind(except)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(n > 0)
}

/// `GET /api/permissions?search=`: 15 per halaman, `search` mencari `name` yang mengandung teks.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let page = pagination::page_params(&query).page;
    let params = PageParams {
        page,
        per_page: PER_PAGE,
    };

    // `$request->has('search') && $request->search`: input di-trim, dan "" atau "0" tidak memfilter.
    let pattern = query
        .get("search")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "0")
        .map(|s| format!("%{s}%"));
    let where_sql = if pattern.is_some() { " WHERE name LIKE ?" } else { "" };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM permissions{where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(p) = &pattern {
        count_q = count_q.bind(p);
    }
    let total = count_q.fetch_one(&state.pool).await.map_err(internal)?;

    let sql = format!(
        "{SELECT_PERMISSION}{where_sql} ORDER BY id LIMIT {PER_PAGE} OFFSET {}",
        (page - 1).saturating_mul(PER_PAGE)
    );
    let mut q = sqlx::query(&sql);
    if let Some(p) = &pattern {
        q = q.bind(p);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let data = rows
        .iter()
        .map(|r| map_row(r).map(|p| permission_json(&p)).map_err(internal))
        .collect::<Result<Vec<_>, _>>()?;

    let base = format!("{}/api/permissions", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total as u64, params, &base)).into_response())
}

/// `POST /api/permissions`: 201. `name` unik. Guard selalu `web`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let name = name_field(&mut e, &input);
    if let Some(n) = &name {
        if name_taken(&state.pool, n, 0).await? {
            e.add("name", "The name has already been taken.");
        }
    }
    e.finish()?;

    let name = name.unwrap_or_default();
    // `firstOrCreate` dengan `name` dan `guard_name` = `web`.
    let res = sqlx::query(
        "INSERT INTO permissions (name, guard_name, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
    )
    .bind(&name)
    .bind(GUARD)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    forget_permission_cache(&state.pool).await?;

    let created = load(&state.pool, id)
        .await?
        .map_or(Value::Null, |p| permission_json(&p));
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

/// `GET /api/permissions/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let p = load_or_404(&state.pool, &id).await?;
    Ok(Json(permission_json(&p)).into_response())
}

/// `PUT` dan `PATCH /api/permissions/{id}`: `name` wajib dan unik, selain id ini sendiri.
/// `save()` di Laravel hanya menulis bila nama berubah (dan `updated_at` ikut berubah),
/// tetapi event `saved` tetap jalan, jadi cache tetap dihapus.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let current = load_or_404(&state.pool, &id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let name = name_field(&mut e, &input);
    if let Some(n) = &name {
        if name_taken(&state.pool, n, current.id).await? {
            e.add("name", "The name has already been taken.");
        }
    }
    e.finish()?;

    let name = name.unwrap_or_default();
    if name != current.name {
        sqlx::query("UPDATE permissions SET name = ?, updated_at = NOW() WHERE id = ?")
            .bind(&name)
            .bind(current.id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    }
    forget_permission_cache(&state.pool).await?;

    let updated = load(&state.pool, current.id)
        .await?
        .map_or(Value::Null, |p| permission_json(&p));
    Ok(Json(updated).into_response())
}

/// `DELETE /api/permissions/{id}`: hapus izin beserta pivot ke peran dan user.
/// Izin yang masih dipakai peran tetap bisa dihapus, sama seperti di Laravel.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let p = load_or_404(&state.pool, &id).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM role_has_permissions WHERE permission_id = ?")
        .bind(p.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM model_has_permissions WHERE permission_id = ?")
        .bind(p.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM permissions WHERE id = ?")
        .bind(p.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    forget_permission_cache(&state.pool).await?;

    Ok(Json(json!({ "message": "Permission deleted" })).into_response())
}
