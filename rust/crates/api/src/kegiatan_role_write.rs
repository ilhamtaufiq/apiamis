//! Pemetaan kegiatan ke role: `GET /api/kegiatan-role`, `POST /api/kegiatan-role`, dan
//! `DELETE /api/kegiatan-role/{id}`. Mengikuti `KegiatanRoleController` dan model `KegiatanRole`
//! (hanya `Auditable`, tanpa notifikasi admin).

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Row, Transaction};

use crate::{audit, foto, kegiatan_write, lookup::carbon_json, pagination, require_auth, AppState};

const MODEL: &str = "App\\Models\\KegiatanRole";
const PER_PAGE: u64 = 20;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Kolom `kegiatan_role` untuk audit.
async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), CAST(role_id AS SIGNED), CAST(kegiatan_id AS SIGNED), created_at, updated_at \
         FROM kegiatan_role WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(3).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(4).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "role_id".into(),
        json!(r.try_get::<i64, _>(1).map_err(internal)?),
    );
    m.insert(
        "kegiatan_id".into(),
        json!(r.try_get::<i64, _>(2).map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(Some(m))
}

/// Bentuk model `KegiatanRole` dengan relasi `role` (Spatie) dan `kegiatan`, seperti `toArray()` Laravel.
async fn resource(pool: &sqlx::MySqlPool, id: u64) -> Result<Option<Value>, ApiError> {
    let Some(base) = attributes(pool, id).await? else {
        return Ok(None);
    };
    let role_id = base["role_id"].as_i64().unwrap_or(0) as u64;
    let kegiatan_id = base["kegiatan_id"].as_i64().unwrap_or(0) as u64;
    let role = role_json(pool, role_id).await?;
    let kegiatan = match crate::kegiatan::find(pool, kegiatan_id)
        .await
        .map_err(internal)?
    {
        Some(_) => kegiatan_write::attributes(pool, kegiatan_id)
            .await?
            .map(Value::Object)
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let mut out = base;
    out.insert("role".into(), role);
    out.insert("kegiatan".into(), kegiatan);
    Ok(Some(Value::Object(out)))
}

async fn role_json(pool: &sqlx::MySqlPool, role_id: u64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), name, guard_name, created_at, updated_at FROM roles WHERE id = ?",
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => {
            let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(3).map_err(internal)?;
            let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(4).map_err(internal)?;
            json!({
                "id": r.try_get::<i64, _>(0).map_err(internal)?,
                "name": r.try_get::<String, _>(1).map_err(internal)?,
                "guard_name": r.try_get::<String, _>(2).map_err(internal)?,
                "created_at": carbon_json(created),
                "updated_at": carbon_json(updated),
            })
        }
    })
}

/// `GET /api/kegiatan-role`: paginasi 20 dengan relasi `role` dan `kegiatan`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kegiatan_role")
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM kegiatan_role ORDER BY id LIMIT ? OFFSET ?",
    )
    .bind(PER_PAGE as i64)
    .bind(((page - 1) * PER_PAGE) as i64)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = resource(&state.pool, id as u64).await? {
            data.push(v);
        }
    }
    let base = format!("{}/api/kegiatan-role", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate_with_query(
        data,
        total as u64,
        pagination::PageParams {
            page,
            per_page: PER_PAGE,
        },
        &base,
        "",
    ))
    .into_response())
}

/// `POST /api/kegiatan-role`: `role_id` wajib dan ada di `roles`; `kegiatan_id` wajib, ada, dan unik per role.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let role_id = body.get("role_id").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });
    let kegiatan_id = body.get("kegiatan_id").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });

    match role_id {
        None => foto::add(
            &mut errs,
            "role_id",
            "The role id field is required.".into(),
        ),
        Some(r) => {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM roles WHERE id = ?")
                .bind(r)
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?;
            if n == 0 {
                foto::add(
                    &mut errs,
                    "role_id",
                    "The selected role id is invalid.".into(),
                );
            }
        }
    }
    match kegiatan_id {
        None => foto::add(
            &mut errs,
            "kegiatan_id",
            "The kegiatan id field is required.".into(),
        ),
        Some(k) => {
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kegiatan WHERE id = ?")
                .bind(k)
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?;
            if exists == 0 {
                foto::add(
                    &mut errs,
                    "kegiatan_id",
                    "The selected kegiatan id is invalid.".into(),
                );
            } else if let Some(r) = role_id {
                let taken: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM kegiatan_role WHERE kegiatan_id = ? AND role_id = ?",
                )
                .bind(k)
                .bind(r)
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?;
                if taken > 0 {
                    foto::add(
                        &mut errs,
                        "kegiatan_id",
                        "The kegiatan id has already been taken.".into(),
                    );
                }
            }
        }
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let (Some(role_id), Some(kegiatan_id)) = (role_id, kegiatan_id) else {
        return Err(internal("validasi kegiatan-role tidak lengkap"));
    };

    let url = format!("{}/api/kegiatan-role", state.app_url.trim_end_matches('/'));
    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query("INSERT INTO kegiatan_role (role_id, kegiatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(role_id)
        .bind(kegiatan_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let id = res.last_insert_id();
    let created = attributes(&mut *tx, id)
        .await?
        .ok_or_else(|| internal("kegiatan-role baru tidak terbaca"))?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "created",
            auditable_type: MODEL,
            auditable_id: id,
            old: None,
            new: Some(created),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    let out = resource(&state.pool, id)
        .await?
        .ok_or_else(|| internal("kegiatan-role tidak terbaca"))?;
    Ok((StatusCode::CREATED, Json(out)).into_response())
}

/// `DELETE /api/kegiatan-role/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let url = format!(
        "{}/api/kegiatan-role/{id}",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM kegiatan_role WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "deleted",
            auditable_type: MODEL,
            auditable_id: id,
            old: Some(before),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Kegiatan-role mapping deleted" })))
}
