//! `GET /api/desa/{desa}/profile` (`DesaController::profile`).
//!
//! Sumber port: method `profile` pada commit `845ca6b^`. Di HEAD Laravel method itu sudah hilang
//! (ikut terhapus bersama rute sync-kk), sehingga rute saat ini gagal di Laravel. Port ini mengikuti
//! versi terakhir yang berfungsi. Perlu konfirmasi sebelum rute ditandai selesai.
//!
//! Isi respons: `desa` (DesaResource dengan kecamatan), `ringkasan`, `pekerjaan` (PekerjaanResource),
//! `spm_sanitasi`, dan `unit_spam` (model mentah, tanpa resource).
//!
//! Perbedaan yang diketahui:
//! - `spm_sanitasi` dan `unit_spam` dibentuk dari tipe kolom MySQL dan `$casts` model. Vendor tidak
//!   tersedia, jadi format tanggal dan decimal belum dibandingkan dengan `toArray()` Laravel.
//! - `pekerjaan` memakai `pekerjaan::to_resource` dengan relasi yang dimuat `pekerjaan::load`.
//!   Himpunan key bisa berbeda dari `PekerjaanResource` dengan `load(['kecamatan','kegiatan'])`.
//! - Baris `spm_sanitasi`, `unit_spam`, dan pekerjaan diurutkan `id`. Laravel tanpa urutan.
//! - Perbandingan status (`active`, `completed`, `Berfungsi`) tidak peka huruf, seperti collation MySQL.

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, Column, MySqlPool, Row, TypeInfo, ValueRef};

use crate::{access, desa, format::number_like_php, lookup::carbon_json, pekerjaan, require_auth, AppState};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Cast dari `$casts` model, hanya untuk tipe yang dipakai di profil.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Cast {
    Float,
    Int,
    Bool,
}

/// `SpmSanitasi::$casts`.
const SPM_CASTS: &[(&str, Cast)] = &[
    ("latitude", Cast::Float),
    ("longitude", Cast::Float),
    ("jumlah_pemanfaat_kk", Cast::Int),
    ("jumlah_pemanfaat_jiwa", Cast::Int),
    ("tahun_konstruksi", Cast::Int),
    ("pembiayaan_apbn", Cast::Float),
    ("pembiayaan_apbd", Cast::Float),
    ("pembiayaan_dak", Cast::Float),
    ("pembiayaan_hibah", Cast::Float),
    ("pembiayaan_csr", Cast::Float),
    ("pembiayaan_lain", Cast::Float),
    ("pembiayaan_total", Cast::Float),
    ("kapasitas_desain", Cast::Float),
    ("kapasitas_terpakai", Cast::Float),
    ("kapasitas_tidak_terpakai", Cast::Float),
    ("biaya_operasional", Cast::Float),
    ("truk_tinja_unit", Cast::Int),
    ("kapasitas_truk_m3", Cast::Float),
    ("jumlah_ritasi", Cast::Int),
    ("jarak_maksimal_pelayanan_km", Cast::Float),
    ("alokasi_biaya_operasional", Cast::Float),
    ("pemanfaat_dari_integrasi", Cast::Bool),
    ("pembiayaan_dari_integrasi", Cast::Bool),
];

/// `UnitSpam::$casts`.
const UNIT_SPAM_CASTS: &[(&str, Cast)] = &[("is_simspam", Cast::Bool)];

/// Nilai satu kolom sebagai JSON. Cast model lebih dulu, lalu tipe kolom MySQL.
fn column_json(row: &MySqlRow, i: usize, ty: &str, cast: Option<Cast>) -> Result<Value, ApiError> {
    if row.try_get_raw(i).map_err(internal)?.is_null() {
        return Ok(Value::Null);
    }
    let ty = ty.to_uppercase();
    match cast {
        Some(Cast::Bool) => {
            let v: i64 = row.try_get(i).map_err(internal)?;
            return Ok(json!(v != 0));
        }
        Some(Cast::Int) => {
            // Kolom id MySQL sering BIGINT UNSIGNED: sqlx tidak mengizinkan i64 untuk kolom itu.
            if let Ok(v) = row.try_get::<i64, _>(i) {
                return Ok(json!(v));
            }
            let v: u64 = row.try_get(i).map_err(internal)?;
            return Ok(json!(v));
        }
        Some(Cast::Float) => {
            let v = float_of(row, i, &ty)?;
            return Ok(number_like_php(v));
        }
        None => {}
    }
    if ty.contains("DECIMAL") {
        // Decimal tanpa cast dikirim Laravel sebagai string.
        // Teks desimal dibaca langsung: sqlx tidak mengizinkan DECIMAL sebagai String lewat jalur aman.
        let text: String = row.try_get_unchecked(i).map_err(internal)?;
        return Ok(json!(text));
    }
    if ty.contains("INT") || ty.contains("YEAR") {
        if let Ok(v) = row.try_get::<i64, _>(i) {
            return Ok(json!(v));
        }
        let v: u64 = row.try_get(i).map_err(internal)?;
        return Ok(json!(v));
    }
    if ty.contains("DOUBLE") || ty.contains("FLOAT") {
        return Ok(number_like_php(float_of(row, i, &ty)?));
    }
    if ty.contains("TIMESTAMP") {
        // Kolom TIMESTAMP MySQL hanya bisa dibaca sebagai DateTime<Utc> oleh sqlx.
        let v: DateTime<Utc> = row.try_get(i).map_err(internal)?;
        return Ok(carbon_json(Some(v)));
    }
    if ty.contains("DATETIME") {
        let v: NaiveDateTime = row.try_get(i).map_err(internal)?;
        return Ok(carbon_json(Some(v.and_utc())));
    }
    if ty == "DATE" {
        let v: NaiveDate = row.try_get(i).map_err(internal)?;
        return Ok(json!(v.format("%Y-%m-%d").to_string()));
    }
    if ty.contains("JSON") {
        let v: String = row.try_get(i).map_err(internal)?;
        return Ok(serde_json::from_str(&v).unwrap_or(json!(v)));
    }
    let v: String = row.try_get(i).map_err(internal)?;
    Ok(json!(v))
}

/// Angka pecahan dari DECIMAL (teks) atau DOUBLE/FLOAT (biner).
fn float_of(row: &MySqlRow, i: usize, ty: &str) -> Result<f64, ApiError> {
    if ty.contains("DECIMAL") {
        let text: String = row.try_get_unchecked(i).map_err(internal)?;
        return text.trim().parse::<f64>().map_err(internal);
    }
    row.try_get::<f64, _>(i).map_err(internal)
}

/// Model mentah (`toArray()` tanpa relasi): semua kolom, nilai dibentuk dari tipe dan cast.
pub(crate) fn raw_model(row: &MySqlRow, casts: &[(&str, Cast)]) -> Result<Value, ApiError> {
    let mut map = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name();
        let cast = casts.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
        map.insert(
            name.to_string(),
            column_json(row, i, col.type_info().name(), cast)?,
        );
    }
    Ok(Value::Object(map))
}

async fn raw_rows(
    pool: &MySqlPool,
    table: &str,
    desa_id: u64,
    casts: &[(&str, Cast)],
) -> Result<Vec<Value>, ApiError> {
    let sql = format!("SELECT * FROM {table} WHERE desa_id = ? ORDER BY id");
    let rows = sqlx::query(&sql)
        .bind(desa_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter().map(|row| raw_model(row, casts)).collect()
}

/// `round($x, 2)` PHP: pembulatan setengah menjauhi nol.
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Kolom `key` pada objek JSON sebagai teks (kosong bila bukan string).
fn text_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `GET /api/desa/{id}/profile`.
pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Binding model dulu (404), lalu auth:sanctum, seperti urutan middleware Laravel.
    let desa_id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let desa_row = desa::find(&state.pool, desa_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let user = require_auth(&state, &headers).await?;

    // `Pekerjaan::where('desa_id', ...)->get()`: tanpa scope byUserRole, seperti Laravel.
    let filter = pekerjaan::PekerjaanFilter::from_query(&HashMap::from([(
        "desa_id".to_string(),
        desa_id.to_string(),
    )]));
    let (pekerjaan_rows, _) = pekerjaan::list(
        &state.pool,
        &filter,
        &access::Restriction::none(),
        None,
        None,
    )
    .await
    .map_err(internal)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let viewer = crate::pekerjaan_rel::Viewer {
        user_id: user.user_id,
        is_admin: roles.iter().any(|(_, n)| n == "admin"),
        nip: None,
        role_ids: roles.iter().map(|(id, _)| *id).collect(),
    };
    let mode = pekerjaan::Mode {
        summary: false,
        unbounded: true,
    };
    let loaded = pekerjaan::load(&state.pool, &pekerjaan_rows, mode, &viewer)
        .await
        .map_err(internal)?;
    let pekerjaan_json: Vec<Value> = pekerjaan_rows
        .iter()
        .map(|p| pekerjaan::to_resource(p, &loaded))
        .collect();

    let spm = raw_rows(&state.pool, "tbl_spm_sanitasi", desa_id, SPM_CASTS).await?;
    let unit = raw_rows(&state.pool, "tbl_unit_spam", desa_id, UNIT_SPAM_CASTS).await?;
    let usulan_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tbl_usulan_kegiatan WHERE desa_id = ?")
            .bind(desa_id)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;

    // Ringkasan (`DesaController::profile`).
    let kepadatan = match desa_row.luas {
        Some(luas) if luas > 0.0 => {
            let penduduk = desa_row.jumlah_penduduk.unwrap_or(0) as f64;
            number_like_php(round2(penduduk / luas))
        }
        _ => Value::Null,
    };
    let total_pagu: f64 = pekerjaan_rows.iter().filter_map(|p| p.pagu).sum();
    let status_count = |status: &str| {
        pekerjaan_rows
            .iter()
            .filter(|p| p.status.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(status)))
            .count()
    };
    let sum_int = |rows: &[Value], key: &str| -> i64 {
        rows.iter()
            .map(|r| r.get(key).and_then(Value::as_i64).unwrap_or(0))
            .sum()
    };
    let simspam = unit
        .iter()
        .filter(|u| u.get("is_simspam") == Some(&json!(true)))
        .count();
    let berfungsi = spm
        .iter()
        .filter(|s| text_of(s, "status_keberfungsian").eq_ignore_ascii_case("Berfungsi"))
        .count();

    Ok(Json(json!({
        "data": {
            "desa": desa::to_resource(&desa_row),
            "ringkasan": {
                "kepadatan_penduduk": kepadatan,
                "total_pekerjaan": pekerjaan_rows.len(),
                "pekerjaan_aktif": status_count("active"),
                "pekerjaan_selesai": status_count("completed"),
                "total_pagu": number_like_php(round2(total_pagu)),
                "total_unit_spam": unit.len(),
                "unit_spam_simspam": simspam,
                "total_infrastruktur_sanitasi": spm.len(),
                "infrastruktur_berfungsi": berfungsi,
                "total_pemanfaat_kk": sum_int(&spm, "jumlah_pemanfaat_kk"),
                "total_pemanfaat_jiwa": sum_int(&spm, "jumlah_pemanfaat_jiwa"),
                "total_usulan_kegiatan": usulan_count,
            },
            "pekerjaan": pekerjaan_json,
            "spm_sanitasi": spm,
            "unit_spam": unit,
        }
    })))
}
