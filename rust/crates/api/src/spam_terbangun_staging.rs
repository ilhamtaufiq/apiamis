//! Staging SPAM terbangun (`/api/spam-terbangun/staging`). Tujuannya `tbl_unit_spam`. Tabel staging ada di migrasi `0017_spam_terbangun_staging.sql`.
//!
//! Alur:
//! 1. `POST /{id}/map`: admin memilih unit secara manual untuk baris `unmatched` atau `invalid` (nama desa
//!    rusak). Setelah dipetakan, status menjadi `matched`. Baris yang sudah diterapkan tidak bisa diubah.
//! 2. `POST /apply`: untuk tiap unit, ambil baris `matched` dengan `id` terkecil dalam batch. Hanya field unit
//!    yang masih kosong yang diisi (sistem layanan, sumber dana, tahun pembangunan, sumber mata air).
//!    Capaian tidak disentuh; itu diisi manual di tabel capaian.
//!
//! Hanya admin. Setelah terapkan, cache unit SPAM dinaikkan seperti impor SPAM.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{notifications::require_admin, require_auth, spam_units, AppState};

const STATUS: [&str; 3] = ["matched", "unmatched", "invalid"];

#[derive(Deserialize)]
pub struct FilterIndex {
    pub batch: Option<String>,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct BodyMap {
    pub unit_spam_id: i64,
}

#[derive(Deserialize)]
pub struct BodyApply {
    pub batch_id: String,
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, message)
}

async fn admin_user(state: &AppState, headers: &HeaderMap) -> Result<u64, ApiError> {
    let user = require_auth(state, headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    Ok(user.user_id)
}

/// Hitung baris per status untuk satu batch. Status yang tidak ada tampil dengan 0.
async fn hitung_status(pool: &MySqlPool, batch: &str) -> Result<Value, ApiError> {
    let rows = sqlx::query(
        "SELECT match_status, CAST(COUNT(*) AS SIGNED) AS n FROM tbl_spam_terbangun_staging \
         WHERE batch_id = ? GROUP BY match_status",
    )
    .bind(batch)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let mut out = serde_json::Map::new();
    for s in STATUS {
        out.insert(s.to_string(), json!(0));
    }
    for r in rows {
        let status: String = r.try_get("match_status").map_err(internal)?;
        let n: i64 = r.try_get("n").map_err(internal)?;
        out.insert(status, json!(n));
    }
    Ok(Value::Object(out))
}

/// Batch terbaru bila tidak dipilih.
async fn batch_terbaru(pool: &MySqlPool) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar("SELECT batch_id FROM tbl_spam_terbangun_staging ORDER BY id DESC LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(internal)
}

/// `GET /api/spam-terbangun/staging?batch=&status=`. Maks. 500 baris per permintaan.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<FilterIndex>,
) -> Result<Response, ApiError> {
    admin_user(&state, &headers).await?;
    let pool = &state.pool;
    let batch = match filter.batch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => Some(b.to_string()),
        None => batch_terbaru(pool).await?,
    };
    let Some(batch) = batch else {
        return Ok(Json(json!({ "batch_id": null, "ringkasan": hitung_status(pool, "").await?, "data": [] })).into_response());
    };
    let status = filter.status.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(s) = &status {
        if !STATUS.contains(&s.as_str()) {
            return Err(invalid("Status tidak dikenal."));
        }
    }

    let rows = sqlx::query(
        "SELECT CAST(s.id AS SIGNED) AS id, CAST(s.raw_id AS SIGNED) AS raw_id, s.kecamatan, s.desa_kelurahan, \
                s.sistem_aliran, s.sumber_dana_raw, s.tahun_pembangunan_awal, s.kondisi_normalized, \
                CAST(s.desa_id AS SIGNED) AS desa_id, CAST(s.unit_spam_id AS SIGNED) AS unit_spam_id, \
                d.n_desa AS nama_desa, s.match_status, (s.applied_at IS NOT NULL) AS sudah_diterapkan \
         FROM tbl_spam_terbangun_staging s \
         LEFT JOIN tbl_desa d ON d.id = s.desa_id \
         WHERE s.batch_id = ? AND (? IS NULL OR s.match_status = ?) \
         ORDER BY s.id LIMIT 500",
    )
    .bind(&batch)
    .bind(&status)
    .bind(&status)
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let sudah: i64 = r.try_get::<Option<i64>, _>("sudah_diterapkan").map_err(internal)?.unwrap_or(0);
        data.push(json!({
            "id": r.try_get::<i64, _>("id").map_err(internal)?,
            "raw_id": r.try_get::<i64, _>("raw_id").map_err(internal)?,
            "kecamatan": r.try_get::<Option<String>, _>("kecamatan").map_err(internal)?,
            "desa_kelurahan": r.try_get::<Option<String>, _>("desa_kelurahan").map_err(internal)?,
            "nama_desa": r.try_get::<Option<String>, _>("nama_desa").map_err(internal)?,
            "sistem_aliran": r.try_get::<Option<String>, _>("sistem_aliran").map_err(internal)?,
            "sumber_dana_raw": r.try_get::<Option<String>, _>("sumber_dana_raw").map_err(internal)?,
            "tahun_pembangunan_awal": r.try_get::<Option<i16>, _>("tahun_pembangunan_awal").map_err(internal)?,
            "kondisi_normalized": r.try_get::<Option<String>, _>("kondisi_normalized").map_err(internal)?,
            "desa_id": r.try_get::<Option<i64>, _>("desa_id").map_err(internal)?,
            "unit_spam_id": r.try_get::<Option<i64>, _>("unit_spam_id").map_err(internal)?,
            "match_status": r.try_get::<String, _>("match_status").map_err(internal)?,
            "sudah_diterapkan": sudah == 1,
        }));
    }

    Ok(Json(json!({
        "batch_id": batch,
        "ringkasan": hitung_status(pool, &batch).await?,
        "data": data,
    }))
    .into_response())
}

/// `POST /api/spam-terbangun/staging/{id}/map`. Memilih unit untuk baris `unmatched` atau `invalid`.
pub async fn map_row(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<BodyMap>,
) -> Result<Response, ApiError> {
    admin_user(&state, &headers).await?;
    let pool = &state.pool;

    let row = sqlx::query(
        "SELECT (applied_at IS NOT NULL) AS sudah FROM tbl_spam_terbangun_staging WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Baris staging tidak ditemukan."))?;
    let sudah: i64 = row.try_get::<Option<i64>, _>("sudah").map_err(internal)?.unwrap_or(0);
    if sudah == 1 {
        return Err(ApiError::new(StatusCode::CONFLICT, "Baris sudah diterapkan, tidak bisa dipetakan ulang."));
    }

    let desa_id: Option<i64> = sqlx::query_scalar("SELECT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE id = ?")
        .bind(body.unit_spam_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| invalid("Unit SPAM tidak ditemukan."))?;

    sqlx::query(
        "UPDATE tbl_spam_terbangun_staging SET desa_id = ?, unit_spam_id = ?, match_status = 'matched', updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(desa_id)
    .bind(body.unit_spam_id)
    .bind(id)
    .execute(pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({ "id": id, "unit_spam_id": body.unit_spam_id, "match_status": "matched" })).into_response())
}

/// `POST /api/spam-terbangun/staging/apply`. Mengisi field unit yang masih kosong dari baris `matched` pertama
/// tiap unit dalam batch. Capaian tidak diubah.
pub async fn apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<BodyApply>,
) -> Result<Response, ApiError> {
    admin_user(&state, &headers).await?;
    let pool = &state.pool;
    let batch = body.batch_id.trim().to_string();
    if batch.is_empty() {
        return Err(invalid("batch_id wajib diisi."));
    }

    let mut tx = pool.begin().await.map_err(internal)?;
    let unit = sqlx::query(
        "UPDATE tbl_unit_spam u \
         JOIN (SELECT unit_spam_id, MIN(id) AS sid FROM tbl_spam_terbangun_staging \
               WHERE batch_id = ? AND match_status = 'matched' GROUP BY unit_spam_id) m ON m.unit_spam_id = u.id \
         JOIN tbl_spam_terbangun_staging s ON s.id = m.sid \
         SET u.sistem_layanan = COALESCE(NULLIF(u.sistem_layanan, ''), s.sistem_aliran), \
             u.sumber_dana = COALESCE(NULLIF(u.sumber_dana, ''), s.sumber_dana_raw), \
             u.tahun_pembangunan = COALESCE(NULLIF(u.tahun_pembangunan, ''), CAST(s.tahun_pembangunan_awal AS CHAR)), \
             u.sumber_mata_air_kap = COALESCE(NULLIF(u.sumber_mata_air_kap, ''), s.sumber_air_baku), \
             u.updated_at = NOW()",
    )
    .bind(&batch)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let baris = sqlx::query(
        "UPDATE tbl_spam_terbangun_staging s \
         JOIN (SELECT MIN(id) AS sid FROM tbl_spam_terbangun_staging \
               WHERE batch_id = ? AND match_status = 'matched' GROUP BY unit_spam_id) m ON m.sid = s.id \
         SET s.applied_at = NOW(), s.updated_at = NOW()",
    )
    .bind(&batch)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    spam_units::bump_spam_generation();

    Ok(Json(json!({
        "batch_id": batch,
        "unit_diubah": unit.rows_affected(),
        "baris_diterapkan": baris.rows_affected(),
    }))
    .into_response())
}
