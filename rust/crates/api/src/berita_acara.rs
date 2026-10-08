//! Nomor urut berita acara per tahun (`BeritaAcaraController`). Baris di `tbl_document_sequences`
//! dengan `type = 'berita-acara'`, sama dengan penomoran di `kontrak_addendum`.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{
    require_auth,
    validation::{int_rule, json_text, Errors},
    AppState,
};

const SEQUENCE_TYPE: &str = "berita-acara";
const MIN_YEAR: i64 = 2020;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn last_number(pool: &MySqlPool, year: i64) -> Result<i64, ApiError> {
    let value: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = ?",
    )
    .bind(year)
    .bind(SEQUENCE_TYPE)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(value.flatten().unwrap_or(0))
}

/// `GET /api/berita-acara/sequence?year=`: nomor terakhir tahun itu (0 bila belum ada).
pub async fn get_sequence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let mut errors = Errors::default();
    let year = errors.check(
        "year",
        int_rule(query.get("year").cloned(), "year", MIN_YEAR),
        0,
    );
    errors.finish()?;
    let last = last_number(&state.pool, year).await?;
    Ok(Json(json!({ "year": year, "last_number": last })).into_response())
}

/// `POST /api/berita-acara/sequence` dengan `{year, last_number}`: menyimpan nomor terakhir (upsert).
pub async fn update_sequence(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut errors = Errors::default();
    let year = errors.check(
        "year",
        int_rule(json_text(input.get("year")), "year", MIN_YEAR),
        0,
    );
    let last = errors.check(
        "last_number",
        int_rule(json_text(input.get("last_number")), "last number", 0),
        0,
    );
    errors.finish()?;

    // `updateOrInsert` dengan unique key (year, type). Query builder Laravel tidak mengisi timestamp.
    sqlx::query(
        "INSERT INTO tbl_document_sequences (year, type, last_number) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE last_number = VALUES(last_number)",
    )
    .bind(year)
    .bind(SEQUENCE_TYPE)
    .bind(last)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    Ok((
        StatusCode::OK,
        Json(json!({ "year": year, "last_number": last })),
    )
        .into_response())
}
