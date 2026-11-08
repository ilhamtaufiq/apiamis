//! Manajemen kunci API layanan untuk admin (dipakai halaman "API Keys" di frontend Arumanis).
//! Logika ada di `public_key`; modul ini hanya autentikasi admin dan bentuk JSON.
//!
//! - `GET    /api/admin/api-keys`             daftar (tanpa nilai kunci)
//! - `POST   /api/admin/api-keys`             buat; kunci asli hanya ada di respons ini
//! - `POST   /api/admin/api-keys/{id}/revoke` cabut

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::ApiError;

use crate::{notifications::require_admin, public_key, require_auth, AppState};

#[derive(Deserialize)]
pub struct NewKeyBody {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_days: Option<i64>,
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn fmt_time(t: Option<chrono::DateTime<chrono::Utc>>) -> Value {
    match t {
        Some(t) => json!(t.to_rfc3339()),
        None => Value::Null,
    }
}

/// Status turunan supaya frontend tidak perlu menghitung tanggal sendiri.
fn status(revoked: bool, expired: bool) -> &'static str {
    if revoked {
        "dicabut"
    } else if expired {
        "kedaluwarsa"
    } else {
        "aktif"
    }
}

/// `GET /api/admin/api-keys`.
pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let rows = public_key::list(&state.pool).await.map_err(internal)?;
    let now = chrono::Utc::now();
    let data: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let expired = r.expires_at.is_some_and(|e| e < now);
            let scopes: Value = serde_json::from_str(&r.scopes).unwrap_or(json!([]));
            json!({
                "id": r.id,
                "name": r.name,
                "prefix": r.prefix,
                "scopes": scopes,
                "status": status(r.revoked_at.is_some(), expired),
                "expires_at": fmt_time(r.expires_at),
                "revoked_at": fmt_time(r.revoked_at),
                "last_used_at": fmt_time(r.last_used_at),
            })
        })
        .collect();
    Ok(Json(json!({ "data": data })).into_response())
}

/// `POST /api/admin/api-keys`. 201 dengan `key` yang hanya tampil sekali.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewKeyBody>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let (id, key) = public_key::create(&state.pool, &body.name, &body.scopes, body.expires_days)
        .await
        .map_err(|m| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m))?;
    tracing::info!(admin_user_id = user.user_id, id, "kunci API dibuat");
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "key": key }))).into_response())
}

/// `POST /api/admin/api-keys/{id}/revoke`.
pub async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    if !public_key::revoke(&state.pool, id).await.map_err(internal)? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "kunci tidak ditemukan atau sudah dicabut"));
    }
    tracing::info!(admin_user_id = user.user_id, id, "kunci API dicabut");
    Ok(Json(json!({ "id": id, "ok": true })).into_response())
}
