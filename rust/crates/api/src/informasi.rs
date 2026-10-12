//! Informasi Arumanis: pengumuman dan informasi untuk pengguna, seperti fitur baru dan pemeliharaan sistem.
//! Tabel `informasi` (lihat migrasi 0006).
//!
//! Rute:
//! - `GET /api/informasi`: pengguna biasa melihat yang terbit dan berada dalam rentang tampil. Admin melihat semua.
//! - `GET /api/informasi/{id}`: sama dengan aturan daftar.
//! - `POST /api/informasi`, `PUT /api/informasi/{id}`, `DELETE /api/informasi/{id}`: hanya admin.
//!
//! Penulisan tidak memakai prefix mutasi di `permission.rs`, jadi non-admin tetap ditolak oleh middleware.
//! Pemeriksaan admin juga dilakukan di handler sebagai lapisan kedua.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::internal,
    notifications::require_admin,
    require_auth,
    skp_jurnal::{as_int, normalize, VALIDATION_MESSAGE},
    AppState,
};

const JUDUL_MAX: usize = 255;
const ISI_MAX: usize = 10_000;
const LIMIT_DEFAULT: u32 = 100;
const LIMIT_MAX: u32 = 200;
/// Kategori yang dikenali frontend. Di luar daftar ini, nilai ditolak agar label tidak kacau.
pub const KATEGORI: &[&str] = &["fitur-baru", "pemeliharaan", "pengumuman", "lainnya"];

const SELECT_BARIS: &str = "SELECT id, judul, isi, kategori, terbit, \
    DATE_FORMAT(tampil_dari, '%Y-%m-%d') AS tampil_dari, \
    DATE_FORMAT(tampil_sampai, '%Y-%m-%d') AS tampil_sampai, \
    created_by, \
    DATE_FORMAT(created_at, '%Y-%m-%dT%H:%i:%sZ') AS created_at, \
    DATE_FORMAT(updated_at, '%Y-%m-%dT%H:%i:%sZ') AS updated_at \
    FROM informasi";

/// Isi informasi yang bisa diubah admin.
#[derive(Debug, Clone, PartialEq)]
pub struct Masukan {
    pub judul: String,
    pub isi: String,
    pub kategori: String,
    pub terbit: bool,
    pub tampil_dari: Option<NaiveDate>,
    pub tampil_sampai: Option<NaiveDate>,
}

fn tanggal_opsional(body: &Value, kunci: &str, errors: &mut BTreeMap<String, Vec<String>>) -> Option<NaiveDate> {
    let teks = body.get(kunci).map(|v| normalize(v.clone()))?;
    let Value::String(s) = teks else { return None };
    match NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d") {
        Ok(tanggal) => Some(tanggal),
        Err(_) => {
            errors.insert(kunci.to_string(), vec![format!("Field {kunci} harus berupa tanggal YYYY-MM-DD.")]);
            None
        }
    }
}

/// Membaca body dan memeriksa isinya. Semua kesalahan dikumpulkan per field.
pub fn parse(body: &Value) -> Result<Masukan, ApiError> {
    let mut errors = BTreeMap::new();

    let judul = match body.get("judul").map(|v| normalize(v.clone())) {
        Some(Value::String(s)) if s.chars().count() <= JUDUL_MAX => s,
        Some(Value::String(_)) => {
            errors.insert("judul".into(), vec![format!("Field judul maksimal {JUDUL_MAX} karakter.")]);
            String::new()
        }
        _ => {
            errors.insert("judul".into(), vec!["Field judul wajib diisi.".into()]);
            String::new()
        }
    };

    let isi = match body.get("isi").map(|v| normalize(v.clone())) {
        Some(Value::String(s)) if s.chars().count() <= ISI_MAX => s,
        Some(Value::String(_)) => {
            errors.insert("isi".into(), vec![format!("Field isi maksimal {ISI_MAX} karakter.")]);
            String::new()
        }
        _ => {
            errors.insert("isi".into(), vec!["Field isi wajib diisi.".into()]);
            String::new()
        }
    };

    let kategori = match body.get("kategori").map(|v| normalize(v.clone())) {
        Some(Value::String(s)) if KATEGORI.contains(&s.as_str()) => s,
        None | Some(Value::Null) => "pengumuman".to_string(),
        _ => {
            errors.insert("kategori".into(), vec![format!("Field kategori harus salah satu dari: {}.", KATEGORI.join(", "))]);
            String::new()
        }
    };

    let terbit = match body.get("terbit") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(v) => as_int(v).map_or_else(|| {
            errors.insert("terbit".into(), vec!["Field terbit harus berupa true atau false.".into()]);
            true
        }, |n| n != 0),
    };

    let tampil_dari = tanggal_opsional(body, "tampil_dari", &mut errors);
    let tampil_sampai = tanggal_opsional(body, "tampil_sampai", &mut errors);
    if let (Some(dari), Some(sampai)) = (tampil_dari, tampil_sampai) {
        if sampai < dari {
            errors.insert("tampil_sampai".into(), vec!["Tanggal selesai tampil tidak boleh sebelum tanggal mulai.".into()]);
        }
    }

    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(Masukan { judul, isi, kategori, terbit, tampil_dari, tampil_sampai })
}

fn baris_json(r: &sqlx::mysql::MySqlRow) -> Result<Value, sqlx::Error> {
    let terbit: i8 = r.try_get("terbit")?;
    Ok(json!({
        "id": r.try_get::<u64, _>("id")?,
        "judul": r.try_get::<String, _>("judul")?,
        "isi": r.try_get::<String, _>("isi")?,
        "kategori": r.try_get::<String, _>("kategori")?,
        "terbit": terbit != 0,
        "tampil_dari": r.try_get::<Option<String>, _>("tampil_dari")?,
        "tampil_sampai": r.try_get::<Option<String>, _>("tampil_sampai")?,
        "created_by": r.try_get::<Option<u64>, _>("created_by")?,
        "created_at": r.try_get::<Option<String>, _>("created_at")?,
        "updated_at": r.try_get::<Option<String>, _>("updated_at")?,
    }))
}

/// Pengguna biasa hanya melihat yang terbit dan berada dalam rentang tampil hari ini.
const FILTER_TAMPIL: &str = "terbit = 1 \
    AND (tampil_dari IS NULL OR tampil_dari <= CURDATE()) \
    AND (tampil_sampai IS NULL OR tampil_sampai >= CURDATE())";

async fn ambil(pool: &MySqlPool, id: i64, admin: bool) -> Result<Option<Value>, ApiError> {
    let sql = if admin {
        format!("{SELECT_BARIS} WHERE id = ?")
    } else {
        format!("{SELECT_BARIS} WHERE id = ? AND {FILTER_TAMPIL}")
    };
    let row = sqlx::query(&sql).bind(id).fetch_optional(pool).await.map_err(internal)?;
    row.map(|r| baris_json(&r).map_err(internal)).transpose()
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let admin = require_admin(&state.pool, user.user_id).await.is_ok();

    let limit = query
        .get("per_page")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(LIMIT_DEFAULT)
        .clamp(1, LIMIT_MAX);
    let mut sql = format!("{SELECT_BARIS} WHERE 1 = 1");
    if !admin {
        sql.push_str(&format!(" AND {FILTER_TAMPIL}"));
    }
    if let Some(kategori) = query.get("kategori").filter(|k| KATEGORI.contains(&k.as_str())) {
        sql.push_str(&format!(" AND kategori = '{kategori}'"));
    }
    sql.push_str(" ORDER BY COALESCE(tampil_dari, DATE(created_at)) DESC, id DESC LIMIT ?");

    let rows = sqlx::query(&sql)
        .bind(limit)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let data = rows
        .iter()
        .map(|r| baris_json(r).map_err(internal))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": data, "is_admin": admin, "today": Utc::now().date_naive().to_string() })))
}

pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let admin = require_admin(&state.pool, user.user_id).await.is_ok();
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let data = ambil(&state.pool, id, admin).await?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": data })))
}

pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let m = parse(&body)?;

    let hasil = sqlx::query(
        "INSERT INTO informasi (judul, isi, kategori, terbit, tampil_dari, tampil_sampai, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(&m.judul)
    .bind(&m.isi)
    .bind(&m.kategori)
    .bind(m.terbit)
    .bind(m.tampil_dari)
    .bind(m.tampil_sampai)
    .bind(user.user_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let data = ambil(&state.pool, hasil.last_insert_id() as i64, true)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let m = parse(&body)?;

    sqlx::query(
        "UPDATE informasi SET judul = ?, isi = ?, kategori = ?, terbit = ?, tampil_dari = ?, tampil_sampai = ?, updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(&m.judul)
    .bind(&m.isi)
    .bind(&m.kategori)
    .bind(m.terbit)
    .bind(m.tampil_dari)
    .bind(m.tampil_sampai)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    // Dibaca ulang untuk memastikan baris ada. Tidak memakai rows_affected, karena 0 juga berarti nilainya sama.
    let data = ambil(&state.pool, id, true).await?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": data })))
}

pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let hasil = sqlx::query("DELETE FROM informasi WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    if hasil.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Value {
        json!({ "judul": "Pemeliharaan server", "isi": "Sistem tidak dapat diakses Sabtu malam." })
    }

    #[test]
    fn terima_isian_minimal_dengan_default() {
        let m = parse(&valid()).unwrap();
        assert_eq!(m.kategori, "pengumuman");
        assert!(m.terbit);
        assert_eq!(m.tampil_dari, None);
    }

    #[test]
    fn tolak_judul_dan_isi_kosong_dengan_pesan_per_field() {
        let err = parse(&json!({ "judul": "  ", "isi": "" })).unwrap_err();
        let errors = err.errors.unwrap();
        assert!(errors.contains_key("judul"));
        assert!(errors.contains_key("isi"));
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn tolak_kategori_di_luar_daftar() {
        let mut body = valid();
        body["kategori"] = json!("gosip");
        let errors = parse(&body).unwrap_err().errors.unwrap();
        assert!(errors.contains_key("kategori"));
    }

    #[test]
    fn tolak_rentang_tampil_terbalik() {
        let mut body = valid();
        body["tampil_dari"] = json!("2026-10-20");
        body["tampil_sampai"] = json!("2026-10-10");
        let errors = parse(&body).unwrap_err().errors.unwrap();
        assert!(errors.contains_key("tampil_sampai"));
    }

    #[test]
    fn terima_tanggal_dan_terbit_false() {
        let mut body = valid();
        body["tampil_dari"] = json!("2026-10-10");
        body["tampil_sampai"] = json!("2026-10-20");
        body["terbit"] = json!(false);
        let m = parse(&body).unwrap();
        assert_eq!(m.tampil_dari, NaiveDate::from_ymd_opt(2026, 10, 10));
        assert!(!m.terbit);
    }
}
