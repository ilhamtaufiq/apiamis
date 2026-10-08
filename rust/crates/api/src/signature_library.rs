//! Pustaka tanda tangan per user (`SignatureLibraryController`, tabel `signature_libraries`, soft delete).
//!
//! `store` setara `updateOrCreate` pada `(user_id, name)` dengan soft delete: baris terhapus tidak
//! dicari, jadi nama yang sama membuat baris baru. Respons `201` untuk buat, `200` untuk ubah.
//! Kunci `deleted_at` ikut di respons daftar dan ubah (kolom dibaca), tetapi tidak di respons buat,
//! seperti atribut model yang baru dibuat di Eloquent.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    lookup::carbon_json,
    require_auth,
    validation::Errors,
    AppState,
};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const SELECT_SIG: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, name, mime_type, \
     data_url, CAST(width AS SIGNED) AS width, CAST(height AS SIGNED) AS height, created_at, updated_at, deleted_at \
     FROM signature_libraries";

struct Sig {
    id: i64,
    user_id: i64,
    name: String,
    mime_type: String,
    data_url: String,
    width: i64,
    height: i64,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

fn map_sig(r: &sqlx::mysql::MySqlRow) -> Result<Sig, sqlx::Error> {
    Ok(Sig {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        name: r.try_get("name")?,
        mime_type: r.try_get("mime_type")?,
        data_url: r.try_get("data_url")?,
        width: r.try_get("width")?,
        height: r.try_get("height")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        deleted_at: r.try_get("deleted_at")?,
    })
}

/// Bentuk model. `with_deleted` false untuk baris yang baru dibuat (tanpa atribut `deleted_at`).
fn resource(s: &Sig, with_deleted: bool) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(s.id));
    m.insert("user_id".into(), json!(s.user_id));
    m.insert("name".into(), json!(s.name));
    m.insert("mime_type".into(), json!(s.mime_type));
    m.insert("data_url".into(), json!(s.data_url));
    m.insert("width".into(), json!(s.width));
    m.insert("height".into(), json!(s.height));
    m.insert("created_at".into(), carbon_json(s.created_at));
    m.insert("updated_at".into(), carbon_json(s.updated_at));
    if with_deleted {
        m.insert("deleted_at".into(), carbon_json(s.deleted_at));
    }
    Value::Object(m)
}

async fn find_live(pool: &MySqlPool, id: i64) -> Result<Option<Sig>, ApiError> {
    let sql = format!("{SELECT_SIG} WHERE id = ? AND deleted_at IS NULL");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_sig(&r))
        .transpose()
        .map_err(internal)
}

/// `isAdmin` untuk `canManage`: peran `admin` di Spatie.
async fn is_admin(pool: &MySqlPool, user_id: u64) -> Result<bool, ApiError> {
    let roles = auth::login::roles_of(pool, user_id).await.map_err(internal)?;
    Ok(roles.iter().any(|(_, n)| n == "admin"))
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `GET /api/signature-libraries`: milik user sendiri, terbaru diubah dulu.
pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let sql = format!(
        "{SELECT_SIG} WHERE user_id = ? AND deleted_at IS NULL ORDER BY updated_at DESC"
    );
    let rows = sqlx::query(&sql)
        .bind(user.user_id as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| map_sig(r).map(|s| resource(&s, true)))
        .collect::<Result<_, _>>()
        .map_err(internal)?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `POST /api/signature-libraries`: buat atau perbarui berdasarkan nama.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let uid = user.user_id as i64;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let text = |e: &mut Errors, field: &str, max: usize| -> Option<String> {
        let attr = field.replace('_', " ");
        match input.get(field) {
            None | Some(Value::Null) => {
                e.add(field, format!("The {attr} field is required."));
                None
            }
            Some(Value::String(s)) if s.chars().count() <= max => Some(s.clone()),
            Some(Value::String(_)) => {
                e.add(field, format!("The {attr} field must not be greater than {max} characters."));
                None
            }
            Some(_) => {
                e.add(field, format!("The {attr} field must be a string."));
                None
            }
        }
    };
    let name = text(&mut e, "name", 255);
    let mime = text(&mut e, "mime_type", 100);
    let data_url = match input.get("data_url") {
        None | Some(Value::Null) => {
            e.add("data_url", "The data url field is required.");
            None
        }
        Some(Value::String(s)) if data_url_ok(s) => Some(s.clone()),
        Some(Value::String(_)) => {
            e.add("data_url", "The data url field format is invalid.");
            None
        }
        Some(_) => {
            e.add("data_url", "The data url field must be a string.");
            None
        }
    };
    let width = dimension(&mut e, &input, "width");
    let height = dimension(&mut e, &input, "height");
    e.finish()?;

    let (Some(name), Some(mime), Some(data_url), Some(width), Some(height)) =
        (name, mime, data_url, width, height)
    else {
        return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "validasi tidak lengkap"));
    };

    let existing_id: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM signature_libraries WHERE user_id = ? AND name = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(uid)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let (status, message, data) = match existing_id {
        Some(id) => {
            sqlx::query(
                "UPDATE signature_libraries SET mime_type = ?, data_url = ?, width = ?, height = ?, updated_at = NOW() \
                 WHERE id = ?",
            )
            .bind(&mime)
            .bind(&data_url)
            .bind(width)
            .bind(height)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
            let sig = find_live(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
            (StatusCode::OK, "Signature berhasil diperbarui", resource(&sig, true))
        }
        None => {
            let res = sqlx::query(
                "INSERT INTO signature_libraries (user_id, name, mime_type, data_url, width, height, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(uid)
            .bind(&name)
            .bind(&mime)
            .bind(&data_url)
            .bind(width)
            .bind(height)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
            let id = res.last_insert_id() as i64;
            let sig = find_live(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
            (StatusCode::CREATED, "Signature berhasil disimpan", resource(&sig, false))
        }
    };
    Ok((
        status,
        Json(json!({ "success": true, "message": message, "data": data })),
    )
        .into_response())
}

/// `regex:/^data:image\/(png|jpe?g|webp);base64,/i`.
fn data_url_ok(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/jpg;base64,", "data:image/webp;base64,"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// `integer|min:1|max:20000`.
fn dimension(e: &mut Errors, input: &Map<String, Value>, field: &str) -> Option<i64> {
    let attr = field.replace('_', " ");
    let parsed = match input.get(field) {
        None | Some(Value::Null) => {
            e.add(field, format!("The {attr} field is required."));
            return None;
        }
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match parsed {
        None => {
            e.add(field, format!("The {attr} field must be an integer."));
            None
        }
        Some(v) if v < 1 => {
            e.add(field, format!("The {attr} field must be at least 1."));
            None
        }
        Some(v) if v > 20000 => {
            e.add(field, format!("The {attr} field must not be greater than 20000."));
            None
        }
        Some(v) => Some(v),
    }
}

/// `DELETE /api/signature-libraries/{id}`: pemilik atau admin. Soft delete.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    // `findOrFail` tidak memakai scope soft delete di sini: baris terhapus juga 404.
    let sig = find_live(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
    if sig.user_id != user.user_id as i64 && !is_admin(&state.pool, user.user_id).await? {
        return Ok((
            StatusCode::FORBIDDEN,
            Json(json!({
                "success": false,
                "message": "Anda tidak memiliki izin untuk menghapus signature ini",
            })),
        )
            .into_response());
    }
    sqlx::query("UPDATE signature_libraries SET deleted_at = NOW() WHERE id = ?")
        .bind(sig.id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "success": true,
        "message": "Signature berhasil dihapus",
    }))
    .into_response())
}
