//! Tipe dokumen: tambah, ubah, dan hapus (`DocumentRegisterController::storeType`, `updateType`,
//! `destroyType`). Daftar tipe (`GET /api/document-types`) ada di `lookup::document_types_index`.
//!
//! Respons memakai bentuk `lookup::document_type_json`. Kunci `format_template` pada respons `store`
//! hanya ada bila dikirim, karena Eloquent `create($validated)` hanya mengisi kolom yang divalidasi.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    lookup::{document_type_json, DocumentTypeRow},
    require_auth,
    validation::Errors,
    AppState,
};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const SELECT_TYPE: &str =
    "SELECT CAST(id AS SIGNED) AS id, name, code, format_template, created_at, updated_at FROM tbl_document_types";

fn map_type(r: &sqlx::mysql::MySqlRow) -> Result<DocumentTypeRow, sqlx::Error> {
    Ok(DocumentTypeRow {
        id: r.try_get::<i64, _>("id")? as u64,
        name: r.try_get("name")?,
        code: r.try_get("code")?,
        format_template: r.try_get("format_template")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find(pool: &MySqlPool, id: &str) -> Result<DocumentTypeRow, ApiError> {
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let sql = format!("{SELECT_TYPE} WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_type(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `required|string` untuk `name`. Mengembalikan teks bila valid; error dicatat bila tidak.
fn required_string(e: &mut Errors, input: &Map<String, Value>, field: &str) -> Option<String> {
    match input.get(field) {
        None | Some(Value::Null) => {
            e.add(field, format!("The {} field is required.", field.replace('_', " ")));
            None
        }
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            e.add(field, format!("The {} field must be a string.", field.replace('_', " ")));
            None
        }
    }
}

/// `nullable|string` untuk `format_template`. Dikembalikan `Some(None)` bila null, `None` bila key tidak ada.
fn nullable_string(e: &mut Errors, input: &Map<String, Value>, field: &str) -> Option<Option<String>> {
    match input.get(field) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some(Some(s.clone())),
        Some(_) => {
            e.add(field, format!("The {} field must be a string.", field.replace('_', " ")));
            None
        }
    }
}

/// `unique:tbl_document_types,code` dengan pengecualian id sendiri.
async fn code_taken(pool: &MySqlPool, code: &str, except: Option<u64>) -> Result<bool, ApiError> {
    let count: i64 = match except {
        Some(id) => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_types WHERE code = ? AND id <> ?",
        )
        .bind(code)
        .bind(id as i64)
        .fetch_one(pool)
        .await,
        None => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_types WHERE code = ?",
        )
        .bind(code)
        .fetch_one(pool)
        .await,
    }
    .map_err(internal)?;
    Ok(count > 0)
}

/// `POST /api/document-types`: 201.
pub async fn store_type(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let name = required_string(&mut e, &input, "name");
    let code = required_string(&mut e, &input, "code");
    let format = nullable_string(&mut e, &input, "format_template");
    if let Some(c) = &code {
        if code_taken(&state.pool, c, None).await? {
            e.add("code", "The code has already been taken.");
        }
    }
    e.finish()?;

    let format_value = format.clone().flatten();
    let result = sqlx::query(
        "INSERT INTO tbl_document_types (name, code, format_template, created_at, updated_at) \
         VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(name.unwrap_or_default())
    .bind(code.unwrap_or_default())
    .bind(format_value)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let created = find(&state.pool, &result.last_insert_id().to_string()).await?;

    let mut out = document_type_json(&created);
    if let (None, Some(o)) = (&format, out.as_object_mut()) {
        o.remove("format_template");
    }
    Ok((StatusCode::CREATED, Json(out)).into_response())
}

/// `PUT` dan `PATCH /api/document-types/{id}`. `name` dan `code` wajib, `format_template` boleh null.
pub async fn update_type(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let current = find(&state.pool, &id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let name = required_string(&mut e, &input, "name");
    let code = required_string(&mut e, &input, "code");
    let format = nullable_string(&mut e, &input, "format_template");
    if let Some(c) = &code {
        if code_taken(&state.pool, c, Some(current.id)).await? {
            e.add("code", "The code has already been taken.");
        }
    }
    e.finish()?;

    let new_name = name.unwrap_or_default();
    let new_code = code.unwrap_or_default();
    let new_format = match &format {
        Some(v) => v.clone(),
        None => current.format_template.clone(),
    };
    // Eloquent hanya menyentuh `updated_at` bila ada atribut yang berubah.
    let changed = new_name != current.name
        || new_code != current.code
        || new_format != current.format_template;
    if changed {
        sqlx::query(
            "UPDATE tbl_document_types SET name = ?, code = ?, format_template = ?, updated_at = NOW() WHERE id = ?",
        )
        .bind(&new_name)
        .bind(&new_code)
        .bind(&new_format)
        .bind(current.id as i64)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    }
    let fresh = find(&state.pool, &id).await?;
    Ok(Json(document_type_json(&fresh)).into_response())
}

/// `DELETE /api/document-types/{id}`: ditolak 422 bila masih dipakai register.
pub async fn destroy_type(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let current = find(&state.pool, &id).await?;
    let used: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_registers WHERE type_id = ?",
    )
    .bind(current.id as i64)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    if used > 0 {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "message": "Tidak dapat menghapus tipe yang sudah memiliki data register" })),
        )
            .into_response());
    }
    sqlx::query("DELETE FROM tbl_document_types WHERE id = ?")
        .bind(current.id as i64)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "message": "Tipe berhasil dihapus" })).into_response())
}
