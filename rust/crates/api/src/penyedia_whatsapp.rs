//! Nomor WhatsApp penyedia. Satu penyedia boleh punya banyak nomor.
//!
//! Rute:
//! - `GET /api/penyedia/{id}/whatsapp`: daftar nomor.
//! - `POST /api/penyedia/{id}/whatsapp`: tambah nomor `{nomor, label?}`. Nomor dinormalisasi lewat `whatsapp::normalize`.
//! - `DELETE /api/penyedia/{id}/whatsapp/{wa_id}`: hapus satu nomor.
//!
//! Izin mengikuti penulisan penyedia: cukup sudah login.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{media::internal, penyedia, require_auth, whatsapp, AppState};

const LABEL_MAX: usize = 100;

fn parse_id(raw: &str) -> Result<u64, ApiError> {
    raw.parse::<u64>().map_err(|_| ApiError::not_found())
}

async fn penyedia_ada(pool: &MySqlPool, id: u64) -> Result<(), ApiError> {
    penyedia::find(pool, id)
        .await
        .map_err(internal)?
        .map(|_| ())
        .ok_or_else(ApiError::not_found)
}

fn bentuk(row: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": row.try_get::<u64, _>("id")?,
        "nomor": row.try_get::<String, _>("nomor")?,
        "label": row.try_get::<Option<String>, _>("label")?,
    }))
}

/// Daftar nomor WhatsApp satu penyedia. Dipakai juga oleh detail penyedia.
pub async fn daftar(pool: &MySqlPool, penyedia_id: u64) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, nomor, label FROM penyedia_whatsapp WHERE penyedia_id = ? ORDER BY id")
        .bind(penyedia_id)
        .fetch_all(pool)
        .await?;
    rows.iter().map(bentuk).collect()
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    penyedia_ada(&state.pool, id).await?;
    let data = daftar(&state.pool, id).await.map_err(internal)?;
    Ok(Json(json!({ "data": data })))
}

pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    penyedia_ada(&state.pool, id).await?;

    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let nomor = match body.get("nomor").and_then(Value::as_str) {
        None => {
            errors.insert("nomor".into(), vec!["The nomor field is required.".into()]);
            None
        }
        Some(raw) => match whatsapp::normalize(raw) {
            Ok(n) => Some(n),
            Err(pesan) => {
                errors.insert("nomor".into(), vec![pesan]);
                None
            }
        },
    };
    let label = body
        .get("label")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if label.as_ref().is_some_and(|l| l.chars().count() > LABEL_MAX) {
        errors.insert("label".into(), vec!["The label field must not be greater than 100 characters.".into()]);
    }
    if !errors.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errors));
    }
    let nomor = nomor.unwrap_or_default();

    let dipakai: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM penyedia_whatsapp WHERE penyedia_id = ? AND nomor = ?")
        .bind(id)
        .bind(&nomor)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    if dipakai > 0 {
        let mut errors = BTreeMap::new();
        errors.insert("nomor".into(), vec!["Nomor ini sudah ada untuk penyedia ini.".into()]);
        return Err(ApiError::validation("The given data was invalid.", errors));
    }

    let res = sqlx::query(
        "INSERT INTO penyedia_whatsapp (penyedia_id, nomor, label, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(id)
    .bind(&nomor)
    .bind(&label)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let baru = res.last_insert_id();
    Ok((StatusCode::CREATED, Json(json!({ "data": { "id": baru, "nomor": nomor, "label": label } }))))
}

pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, wa_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let wa_id = parse_id(&wa_id)?;
    let res = sqlx::query("DELETE FROM penyedia_whatsapp WHERE id = ? AND penyedia_id = ?")
        .bind(wa_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
