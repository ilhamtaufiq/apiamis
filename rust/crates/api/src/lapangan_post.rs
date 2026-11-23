//! Posting lapangan (`/api/lapangan-post`): cerita dan foto dari lapangan. Terpisah dari foto pekerjaan
//! (`/api/foto`, tabel `tbl_foto`), yang berisi tahap progres dan komponen.
//!
//! - `GET`: daftar terbaru, filter `tahun` dan `pekerjaan_id`, dibatasi ruang lingkup paket pengguna.
//! - `POST` (multipart): `file` (gambar), `pekerjaan_id`, `caption` (opsional, maks. 500 karakter),
//!   `koordinat` (opsional).
//! - `DELETE /{id}`: hanya pemilik posting atau admin.

use std::path::PathBuf;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{access, foto, format::iso8601_utc, media, require_auth, AppState};

pub const MODEL: &str = "App\\Models\\LapanganPost";
pub const COLLECTION: &str = "lapangan/post";
pub const CAPTION_MAKS: usize = 500;
/// Batas body rute: foto lapangan dari kamera ponsel bisa cukup besar.
pub const BODY_LIMIT: usize = 12 * 1024 * 1024;
const PER_HALAMAN_MAKS: i64 = 50;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn unprocessable(message: &str) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, message)
}

/// Caption dipangkas, kosong dibuang, dan dibatasi `CAPTION_MAKS` karakter.
pub fn caption_bersih(raw: Option<&str>) -> Option<String> {
    let t = raw?.trim();
    (!t.is_empty()).then(|| t.chars().take(CAPTION_MAKS).collect())
}

/// Hanya gambar yang diterima sebagai posting lapangan.
pub fn gambar_valid(nama: &str) -> bool {
    media::mime_for_name(nama).starts_with("image/")
}

async fn resource(pool: &MySqlPool, app_url: &str, id: i64) -> Result<Option<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(lp.id AS SIGNED) AS id, CAST(lp.pekerjaan_id AS SIGNED) AS pekerjaan_id, \
                CAST(lp.user_id AS SIGNED) AS user_id, lp.caption, lp.koordinat, lp.created_at, \
                p.nama_paket, u.name AS user_name \
         FROM tbl_lapangan_post lp \
         JOIN tbl_pekerjaan p ON p.id = lp.pekerjaan_id \
         LEFT JOIN users u ON u.id = lp.user_id \
         WHERE lp.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else { return Ok(None) };
    Ok(Some(to_json(pool, app_url, &r).await?))
}

async fn to_json(pool: &MySqlPool, app_url: &str, r: &sqlx::mysql::MySqlRow) -> Result<Value, ApiError> {
    let id: i64 = r.try_get("id").map_err(internal)?;
    let base = app_url.trim_end_matches('/');
    let foto_url = media::first_media(pool, MODEL, id as u64, COLLECTION)
        .await
        .map_err(internal)?
        .map(|m| format!("{base}/storage/{}/{}", m.id, m.file_name));
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get("created_at").map_err(internal)?;
    Ok(json!({
        "id": id,
        "pekerjaan_id": r.try_get::<i64, _>("pekerjaan_id").map_err(internal)?,
        "nama_paket": r.try_get::<Option<String>, _>("nama_paket").map_err(internal)?,
        "caption": r.try_get::<Option<String>, _>("caption").map_err(internal)?,
        "koordinat": r.try_get::<Option<String>, _>("koordinat").map_err(internal)?,
        "foto_url": foto_url,
        "user_id": r.try_get::<Option<i64>, _>("user_id").map_err(internal)?,
        "user_name": r.try_get::<Option<String>, _>("user_name").map_err(internal)?,
        "created_at": iso8601_utc(created),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct FilterIndex {
    pub tahun: Option<String>,
    pub pekerjaan_id: Option<i64>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

/// `GET /api/lapangan-post`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FilterIndex>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    let r = access::restriction(user.user_id, &roles, "p");

    let tahun = q.tahun.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let per_halaman = q.per_page.unwrap_or(15).clamp(1, PER_HALAMAN_MAKS);
    let halaman = q.page.unwrap_or(1).max(1);
    let filter = "WHERE (? IS NULL OR k.tahun_anggaran = ?) AND (? IS NULL OR lp.pekerjaan_id = ?)";
    let dari = "FROM tbl_lapangan_post lp \
                JOIN tbl_pekerjaan p ON p.id = lp.pekerjaan_id \
                LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id";

    // Ruang lingkup paket: parameter `restriction` ditambahkan setelah filter, dengan urutan bind yang sama.
    let total_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) {dari} {filter}{}", r.sql);
    let mut total_q = sqlx::query_scalar::<_, i64>(&total_sql)
        .bind(tahun.clone())
        .bind(tahun.clone())
        .bind(q.pekerjaan_id)
        .bind(q.pekerjaan_id);
    for b in &r.binds {
        total_q = total_q.bind(*b as i64);
    }
    let total = total_q.fetch_one(&state.pool).await.map_err(internal)?;

    let list_sql = format!(
        "SELECT CAST(lp.id AS SIGNED) AS id, CAST(lp.pekerjaan_id AS SIGNED) AS pekerjaan_id, \
                CAST(lp.user_id AS SIGNED) AS user_id, lp.caption, lp.koordinat, lp.created_at, \
                p.nama_paket, u.name AS user_name \
         {dari} LEFT JOIN users u ON u.id = lp.user_id {filter}{} \
         ORDER BY lp.created_at DESC, lp.id DESC LIMIT ? OFFSET ?",
        r.sql
    );
    let mut list_q = sqlx::query(&list_sql)
        .bind(tahun.clone())
        .bind(tahun.clone())
        .bind(q.pekerjaan_id)
        .bind(q.pekerjaan_id);
    for b in &r.binds {
        list_q = list_q.bind(*b as i64);
    }
    let rows = list_q
        .bind(per_halaman)
        .bind((halaman - 1) * per_halaman)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(to_json(&state.pool, &state.app_url, row).await?);
    }
    let last = ((total + per_halaman - 1) / per_halaman).max(1);
    Ok(Json(json!({
        "data": data,
        "meta": { "current_page": halaman, "last_page": last, "per_page": per_halaman, "total": total },
    }))
    .into_response())
}

/// `POST /api/lapangan-post` (multipart).
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;

    let mut pekerjaan_id: Option<i64> = None;
    let mut caption: Option<String> = None;
    let mut koordinat: Option<String> = None;
    let mut berkas: Option<media::Upload> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
    {
        match field.name().unwrap_or_default().to_string().as_str() {
            "file" => {
                let nama = field.file_name().unwrap_or_default().to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                if !nama.is_empty() {
                    berkas = Some(media::Upload { original_name: nama, bytes: bytes.to_vec() });
                }
            }
            "pekerjaan_id" => {
                let t = field.text().await.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                pekerjaan_id = t.trim().parse::<i64>().ok();
            }
            "caption" => {
                let t = field.text().await.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                caption = caption_bersih(Some(&t));
            }
            "koordinat" => {
                let t = field.text().await.map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                koordinat = caption_bersih(Some(&t)).map(|s| s.chars().take(64).collect());
            }
            _ => {}
        }
    }

    let pid = pekerjaan_id.ok_or_else(|| unprocessable("Paket wajib dipilih."))?;
    let upload = berkas.ok_or_else(|| unprocessable("Foto wajib diunggah."))?;
    if !gambar_valid(&upload.original_name) {
        return Err(unprocessable("Berkas harus berupa gambar."));
    }
    foto::ensure_access(&state, user.user_id, &roles, Some(pid)).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_lapangan_post (pekerjaan_id, user_id, caption, koordinat, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pid)
    .bind(user.user_id as i64)
    .bind(&caption)
    .bind(&koordinat)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;

    let mime = media::mime_for_name(&upload.original_name);
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &upload, mime, false).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }

    let data = resource(&state.pool, &state.app_url, id).await?.ok_or_else(|| internal("posting baru tidak terbaca"))?;
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))).into_response())
}

/// `DELETE /api/lapangan-post/{id}`: pemilik atau admin.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    let pemilik: Option<i64> = sqlx::query_scalar("SELECT CAST(user_id AS SIGNED) FROM tbl_lapangan_post WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let admin = roles.iter().any(|(_, n)| n == "admin");
    if !admin && pemilik != Some(user.user_id as i64) {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "Hanya pemilik posting atau admin yang bisa menghapus."));
    }

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs: Vec<PathBuf> = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_lapangan_post WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Posting dihapus." })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_dipangkas_dan_dibatasi() {
        assert_eq!(caption_bersih(Some("  Pipa tersambung  ")), Some("Pipa tersambung".to_string()));
        assert_eq!(caption_bersih(Some("   ")), None);
        assert_eq!(caption_bersih(None), None);
        let panjang = "a".repeat(600);
        assert_eq!(caption_bersih(Some(&panjang)).unwrap().chars().count(), CAPTION_MAKS);
    }

    #[test]
    fn hanya_gambar_yang_diterima() {
        assert!(gambar_valid("foto.jpg"));
        assert!(gambar_valid("foto.PNG"));
        assert!(!gambar_valid("dokumen.pdf"));
    }
}
