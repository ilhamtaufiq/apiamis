//! Foto bukti per RHK per bulan. Setara folder `bukti/<BULAN>/rhkN*.jpg` di generator Python.
//!
//! Tabel `skp_bukti` berisi satu baris per RHK per bulan milik pengguna. Fotonya disimpan di tabel
//! `media` dengan model `App\Models\SkpBukti` dan `model_id` = `skp_bukti.id`.
//!
//! Rute:
//! - `GET /api/skp/bukti?tahun=&bulan=`: foto per RHK pada bulan itu (hanya RHK yang punya foto).
//! - `POST /api/skp/bukti?tahun=&bulan=&rhk=`: unggah foto (multipart `foto` atau `foto[]`), JPG/PNG.
//! - `DELETE /api/skp/bukti/foto/{media_id}`: hapus satu foto.
//!
//! Foto kegiatan jurnal (`/api/skp/jurnal/{id}/foto`) tidak masuk sini. Di laporan, foto itu tampil
//! sebagai lampiran jurnal, bukan bukti RHK.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::{self, internal},
    require_auth,
    skp_jurnal::{self, baca_foto, gagal, VALIDATION_MESSAGE},
    skp_rhk, AppState,
};

pub(crate) const MODEL: &str = "App\\Models\\SkpBukti";
const COLLECTION: &str = "foto";
const MAX_FOTO_PER_RHK: usize = 10;
const MAX_FOTO_BYTES: usize = 10 * 1024 * 1024;
const FOTO_EXT: &[&str] = &["jpg", "jpeg", "png"];
/// Batas body satu request unggah, sama dengan modul foto lain.
pub const BODY_LIMIT: usize = media::MAX_FILE_BYTES + 1024 * 1024;

/// Satu baris foto: `(rhk, media_id, nama berkas)`.
pub struct FotoBukti {
    pub rhk: i64,
    pub media_id: u64,
    pub file_name: String,
    pub generated: String,
}

/// Semua foto bukti satu bulan milik pengguna, urut RHK lalu urutan unggah.
pub async fn baris_bulan(
    pool: &MySqlPool,
    user_id: u64,
    tahun: i32,
    bulan: u32,
) -> Result<Vec<FotoBukti>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(b.rhk_no AS SIGNED) AS rhk, CAST(m.id AS UNSIGNED) AS media_id, m.file_name, \
         CAST(m.generated_conversions AS CHAR) AS generated FROM skp_bukti b \
         JOIN media m ON m.model_id = b.id AND m.model_type = ? AND m.collection_name = ? \
         WHERE b.user_id = ? AND b.tahun = ? AND b.bulan = ? ORDER BY b.rhk_no, m.order_column, m.id",
    )
    .bind(MODEL)
    .bind(COLLECTION)
    .bind(user_id)
    .bind(tahun)
    .bind(bulan)
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    rows.iter()
        .map(|r| -> Result<FotoBukti, sqlx::Error> {
            Ok(FotoBukti {
                rhk: r.try_get("rhk")?,
                media_id: r.try_get("media_id")?,
                file_name: r.try_get("file_name")?,
                generated: r.try_get("generated")?,
            })
        })
        .collect::<Result<_, _>>()
        .map_err(internal)
}

fn ke_json(baris: &[FotoBukti], app_url: &str, rhk: Option<i64>) -> Vec<Value> {
    let mut grup: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for b in baris.iter().filter(|b| rhk.map_or(true, |n| n == b.rhk)) {
        let (url, thumb) = media::public_urls(app_url, b.media_id, &b.file_name, &b.generated);
        grup.entry(b.rhk).or_default().push(json!({ "id": b.media_id, "url": url, "thumb": thumb }));
    }
    grup.into_iter()
        .map(|(rhk, foto)| json!({ "rhk": rhk, "foto": foto }))
        .collect()
}

/// `GET /api/skp/bukti`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (tahun, bulan) = skp_jurnal::periode(&query)?;
    let baris = baris_bulan(&state.pool, user.user_id, tahun, bulan).await?;
    Ok(Json(json!({ "data": ke_json(&baris, &state.app_url, None) })))
}

/// `POST /api/skp/bukti?tahun=&bulan=&rhk=`. Respons: foto RHK itu setelah unggah.
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    req: Request,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (tahun, bulan) = skp_jurnal::periode(&query)?;
    let rhk = query
        .get("rhk")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| (1..=99).contains(n));

    let mut errors = BTreeMap::new();
    let Some(rhk) = rhk else {
        gagal(&mut errors, "rhk", "Nomor RHK harus antara 1 dan 99.");
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    };

    let daftar = skp_rhk::daftar(&state.pool, user.user_id, tahun).await?;
    if !daftar.iter().any(|r| r.no == rhk) {
        gagal(&mut errors, "rhk", "RHK ini belum ada di daftar RHK tahun ini.");
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }

    let sudah_ada = baris_bulan(&state.pool, user.user_id, tahun, bulan)
        .await?
        .iter()
        .filter(|b| b.rhk == rhk)
        .count();
    let files = baca_foto(&state, req).await?;
    validasi_foto(&files, sudah_ada)?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "INSERT INTO skp_bukti (user_id, tahun, bulan, rhk_no, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE id = LAST_INSERT_ID(id), updated_at = NOW()",
    )
    .bind(user.user_id)
    .bind(tahun)
    .bind(bulan)
    .bind(rhk)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let slot = sqlx::query("SELECT CAST(LAST_INSERT_ID() AS UNSIGNED) AS id")
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
    let slot_id: u64 = slot.try_get("id").map_err(internal)?;

    let mut dirs = Vec::new();
    for file in &files {
        let mime = media::image_mime(&file.bytes).unwrap_or("image/jpeg");
        match media::attach(&mut tx, MODEL, slot_id, COLLECTION, file, mime, true).await {
            Ok(stored) => dirs.push(stored.dir),
            Err(e) => {
                media::remove_dirs(&dirs).await;
                return Err(e);
            }
        }
    }
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(e));
    }

    let baris = baris_bulan(&state.pool, user.user_id, tahun, bulan).await?;
    let hasil = ke_json(&baris, &state.app_url, Some(rhk));
    Ok((
        StatusCode::OK,
        Json(json!({ "data": hasil.into_iter().next().unwrap_or(json!({ "rhk": rhk, "foto": [] })) })),
    )
        .into_response())
}

/// `DELETE /api/skp/bukti/foto/{media_id}`.
pub async fn delete_foto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(media_id): Path<String>,
) -> Result<Response, ApiError> {
    let media_id = media_id.parse::<u64>().map_err(|_| ApiError::not_found())?;
    let user = require_auth(&state, &headers).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let found: Option<u64> = sqlx::query_scalar(
        "SELECT CAST(m.id AS UNSIGNED) FROM media m JOIN skp_bukti b ON b.id = m.model_id \
         WHERE m.id = ? AND m.model_type = ? AND m.collection_name = ? AND b.user_id = ?",
    )
    .bind(media_id)
    .bind(MODEL)
    .bind(COLLECTION)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if found.is_none() {
        return Err(ApiError::not_found());
    }
    sqlx::query("DELETE FROM media WHERE id = ?")
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&[media::media_dir(media_id)]).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Aturan foto bukti: JPG/PNG, maksimal 10 MB per berkas, dan maksimal 10 foto per RHK per bulan.
fn validasi_foto(files: &[media::Upload], sudah_ada: usize) -> Result<(), ApiError> {
    let mut errors = BTreeMap::new();
    if files.is_empty() {
        gagal(&mut errors, "foto", "Pilih minimal satu foto.");
    } else if sudah_ada + files.len() > MAX_FOTO_PER_RHK {
        gagal(&mut errors, "foto", "Satu RHK maksimal 10 foto per bulan.");
    }
    for f in files {
        let ext = f.extension().to_ascii_lowercase();
        if !FOTO_EXT.contains(&ext.as_str()) || media::image_mime(&f.bytes).is_none() {
            gagal(&mut errors, "foto", "Foto harus berupa gambar JPG atau PNG.");
        }
        if f.bytes.len() > MAX_FOTO_BYTES {
            gagal(&mut errors, "foto", "Ukuran foto maksimal 10 MB.");
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berkas(nama: &str, bytes: Vec<u8>) -> media::Upload {
        media::Upload {
            original_name: nama.to_string(),
            bytes,
        }
    }

    #[test]
    fn validasi_menolak_bukan_gambar_dan_melebihi_batas() {
        let png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert!(validasi_foto(&[berkas("a.png", png.clone())], 0).is_ok());
        assert!(validasi_foto(&[berkas("a.txt", png.clone())], 0).is_err());
        assert!(validasi_foto(&[berkas("a.png", vec![1, 2, 3])], 0).is_err());
        assert!(validasi_foto(&[berkas("a.png", png.clone())], MAX_FOTO_PER_RHK).is_err());
        assert!(validasi_foto(&[], 0).is_err());
    }

    #[test]
    fn ke_json_mengelompokkan_per_rhk_dan_menyaring() {
        let baris = vec![
            FotoBukti { rhk: 2, media_id: 7, file_name: "b.jpg".into(), generated: String::new() },
            FotoBukti { rhk: 1, media_id: 5, file_name: "a.jpg".into(), generated: "{\"thumb\": true}".into() },
            FotoBukti { rhk: 1, media_id: 6, file_name: "c.png".into(), generated: String::new() },
        ];
        let semua = ke_json(&baris, "http://x", None);
        assert_eq!(semua.len(), 2);
        assert_eq!(semua[0]["rhk"], 1);
        assert_eq!(semua[0]["foto"].as_array().unwrap().len(), 2);
        let satu = ke_json(&baris, "http://x", Some(2));
        assert_eq!(satu.len(), 1);
        assert_eq!(satu[0]["rhk"], 2);
    }
}
