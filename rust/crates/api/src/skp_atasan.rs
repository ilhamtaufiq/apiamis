//! Pejabat penilai kinerja (atasan) per pengguna untuk laporan SKP: tabel `skp_atasan`.
//! Modul baru, tidak ada padanannya di Laravel.
//!
//! Rute:
//! - `GET /api/skp/atasan`: data pejabat penilai pengguna. Belum diisi berarti semua field kosong.
//! - `PUT /api/skp/atasan`: simpan data pejabat penilai (menggantikan seluruh isi).

use std::collections::BTreeMap;

use axum::{extract::State, http::HeaderMap, Json};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::internal,
    require_auth,
    skp_jurnal::{gagal, normalize, VALIDATION_MESSAGE},
    skp_pptx::Orang,
    AppState,
};

const MAX_NAMA: usize = 255;
const MAX_NIP: usize = 50;
const MAX_LAIN: usize = 255;

/// Ambil pejabat penilai pengguna. `Orang::default()` bila belum diisi.
pub async fn ambil(pool: &MySqlPool, user_id: u64) -> Result<Orang, ApiError> {
    let row = sqlx::query("SELECT nama, nip, pangkat, jabatan, unit FROM skp_atasan WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Orang::default());
    };
    let teks = |kolom: &str| -> Result<String, ApiError> {
        Ok(r.try_get::<Option<String>, _>(kolom).map_err(internal)?.unwrap_or_default())
    };
    Ok(Orang {
        nama: teks("nama")?,
        nip: teks("nip")?,
        pangkat: teks("pangkat")?,
        jabatan: teks("jabatan")?,
        unit: teks("unit")?,
    })
}

fn ke_json(o: &Orang) -> Value {
    json!({
        "nama": o.nama,
        "nip": o.nip,
        "pangkat": o.pangkat,
        "jabatan": o.jabatan,
        "unit": o.unit,
    })
}

/// Validasi body PUT. Semua field opsional; string kosong dianggap tidak diisi.
fn validasi(body: &Value) -> Result<Orang, ApiError> {
    let mut errors = BTreeMap::new();
    let mut teks = |kunci: &str, maks: usize, label: &str| -> String {
        match normalize(body.get(kunci).cloned().unwrap_or(Value::Null)) {
            Value::Null => String::new(),
            Value::String(s) if s.chars().count() <= maks => s,
            Value::String(_) => {
                gagal(&mut errors, kunci, &format!("{label} maksimal {maks} karakter."));
                String::new()
            }
            _ => {
                gagal(&mut errors, kunci, &format!("{label} harus berupa teks."));
                String::new()
            }
        }
    };
    let orang = Orang {
        nama: teks("nama", MAX_NAMA, "Nama"),
        nip: teks("nip", MAX_NIP, "NIP"),
        pangkat: teks("pangkat", MAX_LAIN, "Pangkat/golongan"),
        jabatan: teks("jabatan", MAX_LAIN, "Jabatan"),
        unit: teks("unit", MAX_LAIN, "Unit kerja"),
    };
    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(orang)
}

/// `GET /api/skp/atasan`.
pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let orang = ambil(&state.pool, user.user_id).await?;
    Ok(Json(json!({ "data": ke_json(&orang) })))
}

/// `PUT /api/skp/atasan`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let orang = validasi(&body)?;

    sqlx::query(
        "INSERT INTO skp_atasan (user_id, nama, nip, pangkat, jabatan, unit, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE nama = VALUES(nama), nip = VALUES(nip), pangkat = VALUES(pangkat), \
         jabatan = VALUES(jabatan), unit = VALUES(unit), updated_at = NOW()",
    )
    .bind(user.user_id)
    .bind(nullable(&orang.nama))
    .bind(nullable(&orang.nip))
    .bind(nullable(&orang.pangkat))
    .bind(nullable(&orang.jabatan))
    .bind(nullable(&orang.unit))
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let tersimpan = ambil(&state.pool, user.user_id).await?;
    Ok(Json(json!({ "data": ke_json(&tersimpan) })))
}

fn nullable(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validasi_menerima_teks_dan_mengosongkan_string_kosong() {
        let o = validasi(&json!({ "nama": "  Agung  ", "nip": "", "pangkat": "Pembina / IV/a" })).unwrap();
        assert_eq!(o.nama, "Agung");
        assert_eq!(o.nip, "");
        assert_eq!(o.pangkat, "Pembina / IV/a");
    }

    #[test]
    fn validasi_menolak_panjang_berlebih_dan_bukan_teks() {
        let errs = validasi(&json!({ "nip": "1".repeat(51), "jabatan": 5 })).unwrap_err().errors.unwrap();
        assert!(errs.contains_key("nip"));
        assert!(errs.contains_key("jabatan"));
    }
}
