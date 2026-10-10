//! Peran dan izinnya (`RoleController`, tabel Spatie `roles`, `permissions`, `role_has_permissions`).
//! Grup rute `role:admin`, jadi semua rute di sini hanya untuk admin.
//!
//! Respons memuat `permissions` dengan `pivot` (`role_id`, `permission_id`), seperti relasi Eloquent.

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
    lookup::carbon_json,
    notifications::require_admin,
    pagination::{self, PageParams},
    require_auth,
    validation::{Errors},
    AppState,
};

/// `paginate(15)` di Laravel: jumlah per halaman tetap, tidak bisa diubah lewat query.
const PER_PAGE: u64 = 15;
const GUARD: &str = "web";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

struct RoleRow {
    id: i64,
    name: String,
    guard_name: String,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_role(r: &sqlx::mysql::MySqlRow) -> Result<RoleRow, sqlx::Error> {
    Ok(RoleRow {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        guard_name: r.try_get("guard_name")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

const SELECT_ROLE: &str = "SELECT CAST(id AS SIGNED) AS id, name, guard_name, created_at, updated_at FROM roles";

/// Izin satu peran beserta `pivot`-nya.
async fn permissions_of(pool: &MySqlPool, role_id: i64) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(p.id AS SIGNED) AS id, p.name, p.guard_name, p.created_at, p.updated_at, \
         CAST(rhp.role_id AS SIGNED) AS role_id, CAST(rhp.permission_id AS SIGNED) AS permission_id \
         FROM role_has_permissions rhp JOIN permissions p ON p.id = rhp.permission_id \
         WHERE rhp.role_id = ? ORDER BY p.id",
    )
    .bind(role_id)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    rows.iter()
        .map(|r| {
            let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
            let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
            Ok(json!({
                "id": r.try_get::<i64, _>("id").map_err(internal)?,
                "name": r.try_get::<String, _>("name").map_err(internal)?,
                "guard_name": r.try_get::<String, _>("guard_name").map_err(internal)?,
                "created_at": carbon_json(created),
                "updated_at": carbon_json(updated),
                "pivot": {
                    "role_id": r.try_get::<i64, _>("role_id").map_err(internal)?,
                    "permission_id": r.try_get::<i64, _>("permission_id").map_err(internal)?,
                },
            }))
        })
        .collect()
}

fn role_json(r: &RoleRow, permissions: Vec<Value>) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "guard_name": r.guard_name,
        "created_at": carbon_json(r.created_at),
        "updated_at": carbon_json(r.updated_at),
        "permissions": permissions,
    })
}

/// Peran dengan izinnya, atau `None` bila id tidak ada.
async fn load(pool: &MySqlPool, id: i64) -> Result<Option<Value>, ApiError> {
    let sql = format!("{SELECT_ROLE} WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let role = map_role(&row).map_err(internal)?;
    let perms = permissions_of(pool, role.id).await?;
    Ok(Some(role_json(&role, perms)))
}

async fn load_or_404(pool: &MySqlPool, id: &str) -> Result<Value, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    load(pool, id).await?.ok_or_else(ApiError::not_found)
}

/// `syncPermissions`: hapus semua izin lalu pasang daftar nama. Nama yang tidak ada untuk guard `web`
/// menghasilkan error seperti `PermissionDoesNotExist` di Spatie.
async fn sync_permissions(pool: &MySqlPool, role_id: i64, names: &[Value]) -> Result<(), ApiError> {
    let mut ids: Vec<i64> = Vec::with_capacity(names.len());
    for v in names {
        let name = v.as_str().map_or_else(|| v.to_string(), str::to_string);
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM permissions WHERE name = ? AND guard_name = ? LIMIT 1",
        )
        .bind(&name)
        .bind(GUARD)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
        match id {
            Some(id) => ids.push(id),
            None => {
                return Err(ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("There is no permission named `{name}` for guard `{GUARD}`."),
                ))
            }
        }
    }
    sqlx::query("DELETE FROM role_has_permissions WHERE role_id = ?")
        .bind(role_id)
        .execute(pool)
        .await
        .map_err(internal)?;
    for id in ids {
        sqlx::query("INSERT IGNORE INTO role_has_permissions (permission_id, role_id) VALUES (?, ?)")
            .bind(id)
            .bind(role_id)
            .execute(pool)
            .await
            .map_err(internal)?;
    }
    Ok(())
}

fn parse_body(body: &Bytes) -> serde_json::Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    }
}

/// `array` untuk `permissions` (`sometimes|array`). Tidak memeriksa isi item, sama dengan Laravel.
fn permissions_array(e: &mut Errors, v: &Value) -> Option<Vec<Value>> {
    match v {
        Value::Array(items) => Some(items.clone()),
        _ => {
            e.add("permissions", "The permissions field must be an array.");
            None
        }
    }
}

/// `GET /api/roles?search=`: 15 per halaman, `search` mencari `name` mengandung teks.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let page = pagination::page_params(&query).page;
    let params = PageParams {
        page,
        per_page: PER_PAGE,
    };

    // `$request->has('search') && $request->search`: string kosong dan "0" tidak memfilter.
    let search = query
        .get("search")
        .filter(|s| !s.is_empty() && s.as_str() != "0")
        .cloned();
    let like = search.map(|s| format!("%{s}%"));
    let (where_sql, total): (&str, i64) = match &like {
        Some(_) => (
            " WHERE name LIKE ?",
            sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM roles WHERE name LIKE ?")
                .bind(like.as_deref().unwrap_or_default())
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?,
        ),
        None => (
            "",
            sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM roles")
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?,
        ),
    };
    let sql = format!("{SELECT_ROLE}{where_sql} ORDER BY id LIMIT {PER_PAGE} OFFSET {}", (params.page - 1) * PER_PAGE);
    let mut q = sqlx::query(&sql);
    if let Some(l) = &like {
        q = q.bind(l);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let role = map_role(row).map_err(internal)?;
        let perms = permissions_of(&state.pool, role.id).await?;
        data.push(role_json(&role, perms));
    }
    let base = format!("{}/api/roles", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total as u64, params, &base)).into_response())
}

/// `POST /api/roles`: 201. `name` unik; `permissions` opsional.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let name = match input.get("name") {
        None | Some(Value::Null) => {
            e.add("name", "The name field is required.");
            None
        }
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            e.add("name", "The name field must be a string.");
            None
        }
    };
    let permissions = match input.get("permissions") {
        None => None,
        Some(v) => permissions_array(&mut e, v),
    };
    if let Some(n) = &name {
        let taken: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM roles WHERE name = ?")
            .bind(n)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
        if taken > 0 {
            e.add("name", "The name has already been taken.");
        }
    }
    e.finish()?;

    let name = name.unwrap_or_default();
    // `firstOrCreate` dengan guard `web`.
    sqlx::query("INSERT INTO roles (name, guard_name, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(&name)
        .bind(GUARD)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    let role_id: i64 = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM roles WHERE name = ? AND guard_name = ? LIMIT 1",
    )
    .bind(&name)
    .bind(GUARD)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    if let Some(perms) = permissions {
        sync_permissions(&state.pool, role_id, &perms).await?;
    }
    crate::permissions::forget_permission_cache(&state.pool).await?;
    let role = load(&state.pool, role_id).await?.unwrap_or(Value::Null);
    Ok((StatusCode::CREATED, Json(role)).into_response())
}

/// `GET /api/roles/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    Ok(Json(load_or_404(&state.pool, &id).await?).into_response())
}

/// `PUT` dan `PATCH /api/roles/{id}`: `name` dan `permissions` bersifat `sometimes`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let role_id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    load(&state.pool, role_id).await?.ok_or_else(ApiError::not_found)?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let mut new_name: Option<String> = None;
    if let Some(v) = input.get("name") {
        match v {
            Value::String(s) => new_name = Some(s.clone()),
            _ => e.add("name", "The name field must be a string."),
        }
    }
    let permissions = match input.get("permissions") {
        None => None,
        Some(v) => permissions_array(&mut e, v),
    };
    if let Some(n) = &new_name {
        let taken: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM roles WHERE name = ? AND id <> ?",
        )
        .bind(n)
        .bind(role_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
        if taken > 0 {
            e.add("name", "The name has already been taken.");
        }
    }
    e.finish()?;

    if let Some(n) = &new_name {
        sqlx::query("UPDATE roles SET name = ?, updated_at = NOW() WHERE id = ?")
            .bind(n)
            .bind(role_id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    } else {
        sqlx::query("UPDATE roles SET updated_at = NOW() WHERE id = ?")
            .bind(role_id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    }
    if let Some(perms) = permissions {
        sync_permissions(&state.pool, role_id, &perms).await?;
    }
    crate::permissions::forget_permission_cache(&state.pool).await?;
    let role = load(&state.pool, role_id).await?.unwrap_or(Value::Null);
    Ok(Json(role).into_response())
}

/// `DELETE /api/roles/{id}`: hapus peran beserta izin dan penugasannya ke user.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let role_id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    load(&state.pool, role_id).await?.ok_or_else(ApiError::not_found)?;
    sqlx::query("DELETE FROM role_has_permissions WHERE role_id = ?")
        .bind(role_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM model_has_roles WHERE role_id = ?")
        .bind(role_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM roles WHERE id = ?")
        .bind(role_id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    crate::permissions::forget_permission_cache(&state.pool).await?;
    Ok(Json(json!({ "message": "Role deleted" })).into_response())
}
