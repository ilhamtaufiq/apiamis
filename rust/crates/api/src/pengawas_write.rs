//! Pengawas: `GET /api/pengawas`, `GET /api/pengawas/{id}`, `POST /api/pengawas`,
//! `PUT`/`PATCH`/`DELETE /api/pengawas/{id}`, dan `GET /api/pengawas/statistics`.
//!
//! Mengikuti `Api\PengawasController`. Model `Pengawas` hanya memakai `Auditable` (tanpa notifikasi admin).
//! Bentuk resource memakai `pekerjaan::pengawas_resource`.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Row, Transaction};

use crate::{audit, foto, lookup::carbon_json, pekerjaan, require_auth, AppState};

const MODEL: &str = "App\\Models\\Pengawas";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Aturan `nama` (wajib saat store, nullable saat update), `nip`, `jabatan`, `telepon` (nullable, maks 255).
fn validate(body: &Value, store: bool) -> Result<Vec<(&'static str, Option<String>)>, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cols: Vec<(&'static str, Option<String>)> = Vec::new();
    match text(obj.get("nama")) {
        None if store => foto::add(&mut errs, "nama", "The nama field is required.".into()),
        None => {}
        Some(s) if s.chars().count() > 255 => foto::add(
            &mut errs,
            "nama",
            "The nama field must not be greater than 255 characters.".into(),
        ),
        Some(s) => cols.push(("nama", Some(s))),
    }
    for key in ["nip", "jabatan", "telepon"] {
        if let Some(v) = obj.get(key) {
            match text(Some(v)) {
                None => cols.push((key, None)),
                Some(s) if s.chars().count() > 255 => foto::add(
                    &mut errs,
                    key,
                    format!("The {} field must not be greater than 255 characters.", key),
                ),
                Some(s) => cols.push((key, Some(s))),
            }
        }
    }
    if errs.is_empty() {
        Ok(cols)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query("SELECT CAST(id AS SIGNED), nama, nip, jabatan, telepon, created_at, updated_at FROM pengawas WHERE id = ?")
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(5).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(6).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "nama".into(),
        json!(r.try_get::<Option<String>, _>(1).map_err(internal)?),
    );
    m.insert(
        "nip".into(),
        json!(r.try_get::<Option<String>, _>(2).map_err(internal)?),
    );
    m.insert(
        "jabatan".into(),
        json!(r.try_get::<Option<String>, _>(3).map_err(internal)?),
    );
    m.insert(
        "telepon".into(),
        json!(r.try_get::<Option<String>, _>(4).map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(Some(m))
}

async fn respond(state: &AppState, id: u64) -> Result<Json<Value>, ApiError> {
    let v = pekerjaan::pengawas_resource(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": v })))
}

async fn audit_write(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: MODEL,
            auditable_id: id,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(internal)
}

/// `GET /api/pengawas`: semua pengawas, tanpa paginasi.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM pengawas ORDER BY id")
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = pekerjaan::pengawas_resource(&state.pool, id as u64)
            .await
            .map_err(internal)?
        {
            data.push(v);
        }
    }
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/pengawas/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    respond(&state, id).await
}

/// `POST /api/pengawas`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let cols = validate(&body, true)?;
    let url = format!("{}/api/pengawas", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let names: Vec<&str> = cols.iter().map(|(k, _)| *k).collect();
    let marks = vec!["?"; names.len()].join(", ");
    let sql = format!(
        "INSERT INTO pengawas ({}, created_at, updated_at) VALUES ({marks}, NOW(), NOW())",
        names.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &cols {
        q = q.bind(v.clone());
    }
    let id = q
        .execute(&mut *tx)
        .await
        .map_err(internal)?
        .last_insert_id();
    let created = attributes(&mut *tx, id)
        .await?
        .ok_or_else(|| internal("pengawas baru tidak terbaca"))?;
    audit_write(
        &mut tx,
        &headers,
        user.user_id,
        "created",
        id,
        None,
        Some(created),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state, id).await
}

/// `PUT` dan `PATCH /api/pengawas/{id}`. Kunci yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let cols = validate(&body, false)?;
    if cols.iter().any(|(k, v)| *k == "nama" && v.is_none()) {
        return Err(internal("Column 'nama' cannot be null"));
    }
    if cols.is_empty() {
        return respond(&state, id).await;
    }
    let url = format!("{}/api/pengawas/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let sets: Vec<String> = cols.iter().map(|(k, _)| format!("{k} = ?")).collect();
    let sql = format!(
        "UPDATE pengawas SET {}, updated_at = NOW() WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &cols {
        q = q.bind(v.clone());
    }
    q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    let after = attributes(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in &after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if !new.is_empty() {
        old.insert(
            "updated_at".into(),
            before.get("updated_at").cloned().unwrap_or(Value::Null),
        );
        new.insert(
            "updated_at".into(),
            after.get("updated_at").cloned().unwrap_or(Value::Null),
        );
        audit_write(
            &mut tx,
            &headers,
            user.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    respond(&state, id).await
}

/// `DELETE /api/pengawas/{id}`.
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
    let url = format!("{}/api/pengawas/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM pengawas WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit_write(
        &mut tx,
        &headers,
        user.user_id,
        "deleted",
        id,
        Some(before),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Pengawas deleted successfully" })))
}

/// `GET /api/pengawas/statistics`: jumlah pengawas, lokasi (kecamatan distinct), dan total pagu
/// dari pekerjaan yang punya pengawas atau pendamping.
pub async fn statistics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let total_pengawas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pengawas")
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let total_lokasi: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT kecamatan_id) FROM tbl_pekerjaan WHERE pengawas_id IS NOT NULL OR pendamping_id IS NOT NULL",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    let total_pagu: f64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(SUM(pagu), 0) AS DOUBLE) FROM tbl_pekerjaan WHERE pengawas_id IS NOT NULL OR pendamping_id IS NOT NULL",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "data": {
            "total_pengawas": total_pengawas,
            "total_lokasi": total_lokasi,
            "total_pagu": crate::format::number_like_php(total_pagu),
        }
    })))
}
