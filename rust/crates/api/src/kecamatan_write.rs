//! Tulis kecamatan: `POST /api/kecamatan`, `PUT`/`PATCH`/`DELETE /api/kecamatan/{id}`.
//!
//! Mengikuti `KecamatanController` dan model `Kecamatan` (`Auditable` dan `NotifiesAdminsOnChanges`).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{changes, foto, kecamatan, lookup::carbon_json, require_auth, AppState};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Atribut untuk audit (kolom tabel, timestamp seperti Carbon).
fn attributes(row: &kecamatan::KecamatanRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("n_kec".into(), json!(row.n_kec));
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `n_kec`: string, maksimal 255, unik di `tbl_kecamatan`.
async fn check_unique(pool: &MySqlPool, nama: &str, except: Option<u64>) -> Result<bool, ApiError> {
    let taken: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kecamatan WHERE n_kec = ? AND id <> ?")
            .bind(nama)
            .bind(except.unwrap_or(0))
            .fetch_one(pool)
            .await
            .map_err(internal)?;
    Ok(taken > 0)
}

fn kecamatan_name(
    body: &Value,
    errs: &mut BTreeMap<String, Vec<String>>,
    required: bool,
) -> Option<Option<String>> {
    match body.get("n_kec") {
        None | Some(Value::Null) => {
            if required {
                foto::add(errs, "n_kec", "The n kec field is required.".into());
                None
            } else if body.get("n_kec").is_some() {
                Some(None)
            } else {
                None
            }
        }
        Some(Value::String(s)) if s.trim().is_empty() => {
            foto::add(errs, "n_kec", "The n kec field is required.".into());
            None
        }
        Some(Value::String(s)) => {
            if s.chars().count() > 255 {
                foto::add(
                    errs,
                    "n_kec",
                    "The n kec field must not be greater than 255 characters.".into(),
                );
                None
            } else {
                Some(Some(s.clone()))
            }
        }
        Some(_) => {
            foto::add(errs, "n_kec", "The n kec field must be a string.".into());
            None
        }
    }
}

fn validation_error(errs: BTreeMap<String, Vec<String>>) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

async fn respond(state: &AppState, id: u64) -> Result<Json<Value>, ApiError> {
    let row = kecamatan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": kecamatan::to_resource(&row) })))
}

/// `POST /api/kecamatan`: respon 200 seperti `KecamatanResource` (tanpa `201`).
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut errs = BTreeMap::new();
    let nama = kecamatan_name(&body, &mut errs, true).flatten();
    let Some(nama) = nama else {
        return Err(validation_error(errs));
    };
    if check_unique(&state.pool, &nama, None).await? {
        foto::add(
            &mut errs,
            "n_kec",
            "The n kec has already been taken.".into(),
        );
        return Err(validation_error(errs));
    }

    let url = format!("{}/api/kecamatan", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(&nama)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id();
    let created = kecamatan::find(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("kecamatan baru tidak terbaca"))?;
    let link = Some(format!("/kecamatan/{id}/edit"));
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KECAMATAN,
        "created",
        id as i64,
        None,
        Some(attributes(&created)),
        link,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state, id).await
}

/// `PUT` dan `PATCH /api/kecamatan/{id}`. Nilai yang sama dengan sekarang tidak menulis apa pun.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = kecamatan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let mut errs = BTreeMap::new();
    let nama = kecamatan_name(&body, &mut errs, false);
    if !errs.is_empty() {
        return Err(validation_error(errs));
    }
    if let Some(Some(n)) = &nama {
        if check_unique(&state.pool, n, Some(id)).await? {
            foto::add(
                &mut errs,
                "n_kec",
                "The n kec has already been taken.".into(),
            );
            return Err(validation_error(errs));
        }
    }

    // Tanpa perubahan atribut, Laravel tidak menyimpan dan tidak mencatat audit.
    let changed = match &nama {
        Some(Some(n)) => n != &current.n_kec,
        Some(None) => return Err(internal("Column 'n_kec' cannot be null")),
        None => false,
    };
    if !changed {
        return respond(&state, id).await;
    }

    let url = format!("{}/api/kecamatan/{id}", state.app_url.trim_end_matches('/'));
    let before = attributes(&current);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE tbl_kecamatan SET n_kec = ?, updated_at = NOW() WHERE id = ?")
        .bind(nama.clone().flatten())
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let after = kecamatan::find(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let after_map = attributes(&after);
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in &after_map {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    old.insert(
        "updated_at".into(),
        before.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    new.insert(
        "updated_at".into(),
        after_map.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KECAMATAN,
        "updated",
        id as i64,
        Some(old),
        Some(new),
        Some(format!("/kecamatan/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state, id).await
}

/// `DELETE /api/kecamatan/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = kecamatan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let url = format!("{}/api/kecamatan/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KECAMATAN,
        "deleted",
        id as i64,
        Some(attributes(&current)),
        None,
        Some(format!("/kecamatan/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Kecamatan deleted successfully" })))
}
