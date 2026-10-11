//! Master RHK SKP per pengguna dan tahun: tabel `skp_rhk` (nama, target, satuan) dan
//! `skp_rhk_rencana` (rencana aksi per bulan). Modul baru, tidak ada padanannya di Laravel.
//!
//! Rute:
//! - `GET /api/skp/rhk?tahun=YYYY`: daftar RHK tahun itu, diurutkan menurut `no`.
//! - `PUT /api/skp/rhk?tahun=YYYY`: mengganti seluruh daftar RHK tahun itu. RHK yang tidak dikirim
//!   dihapus beserta rencananya. Jurnal yang sudah memakai nomor itu tidak diubah.
//!
//! Rencana bulanan `rencana[0..12]` mulai dari Januari. Setiap pengguna hanya mengelola datanya sendiri.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::internal,
    require_auth,
    skp_jurnal::{as_int, as_number, gagal, normalize, VALIDATION_MESSAGE},
    AppState,
};

const MAX_NO: i64 = 99;
const MAX_NAMA: usize = 255;
const MAX_SATUAN: usize = 50;
const DEFAULT_SATUAN: &str = "Dokumen";

#[derive(Debug, Clone, PartialEq)]
pub struct RhkItem {
    pub no: i64,
    pub rhk: String,
    pub target: f64,
    pub satuan: String,
    /// Rencana aksi per bulan, index 0 = Januari.
    pub rencana: [f64; 12],
}

/// `tahun` dari query. Wajib dan harus 2000..=2100.
pub fn tahun_dari_query(query: &HashMap<String, String>) -> Result<i32, ApiError> {
    query
        .get("tahun")
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|t| (2000..=2100).contains(t))
        .ok_or_else(|| {
            let mut errors = BTreeMap::new();
            gagal(&mut errors, "tahun", "Tahun harus antara 2000 dan 2100.");
            ApiError::validation(VALIDATION_MESSAGE, errors)
        })
}

/// Daftar RHK satu tahun milik pengguna, termasuk rencana bulanannya.
pub async fn daftar(pool: &MySqlPool, user_id: u64, tahun: i32) -> Result<Vec<RhkItem>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(no AS SIGNED) AS no, rhk, target, satuan FROM skp_rhk \
         WHERE user_id = ? AND tahun = ? ORDER BY no",
    )
    .bind(user_id)
    .bind(tahun)
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    let mut items: Vec<RhkItem> = rows
        .iter()
        .map(|r| -> Result<RhkItem, sqlx::Error> {
            Ok(RhkItem {
                no: r.try_get("no")?,
                rhk: r.try_get("rhk")?,
                target: r.try_get("target")?,
                satuan: r.try_get("satuan")?,
                rencana: [0.0; 12],
            })
        })
        .collect::<Result<_, _>>()
        .map_err(internal)?;

    let rencana = sqlx::query(
        "SELECT CAST(rhk_no AS SIGNED) AS rhk_no, CAST(bulan AS SIGNED) AS bulan, rencana \
         FROM skp_rhk_rencana WHERE user_id = ? AND tahun = ?",
    )
    .bind(user_id)
    .bind(tahun)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    for r in &rencana {
        let no: i64 = r.try_get("rhk_no").map_err(internal)?;
        let bulan: i64 = r.try_get("bulan").map_err(internal)?;
        let nilai: f64 = r.try_get("rencana").map_err(internal)?;
        if let (Some(item), true) = (items.iter_mut().find(|i| i.no == no), (1..=12).contains(&bulan)) {
            item.rencana[(bulan - 1) as usize] = nilai;
        }
    }
    Ok(items)
}

pub fn ke_json(items: &[RhkItem]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|i| {
                json!({
                    "no": i.no,
                    "rhk": i.rhk,
                    "target": i.target,
                    "satuan": i.satuan,
                    "rencana": i.rencana,
                })
            })
            .collect(),
    )
}

/// Validasi body PUT. Semua error dikumpulkan di key `items`.
pub fn validasi(body: &Value) -> Result<Vec<RhkItem>, ApiError> {
    let mut errors = BTreeMap::new();
    let Some(arr) = body.get("items").and_then(Value::as_array) else {
        gagal(&mut errors, "items", "Daftar RHK harus berupa array.");
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    };

    let mut hasil = Vec::new();
    let mut nomor_terpakai = std::collections::HashSet::new();
    for (i, raw) in arr.iter().enumerate() {
        let baris = i + 1;
        let no = as_int(&normalize(raw.get("no").cloned().unwrap_or(Value::Null)));
        match no {
            Some(n) if (1..=MAX_NO).contains(&n) => {
                if !nomor_terpakai.insert(n) {
                    gagal(&mut errors, "items", &format!("Baris {baris}: nomor RHK {n} dipakai lebih dari sekali."));
                }
            }
            _ => gagal(&mut errors, "items", &format!("Baris {baris}: nomor RHK harus angka 1 sampai 99.")),
        }

        let rhk = match normalize(raw.get("rhk").cloned().unwrap_or(Value::Null)) {
            Value::String(s) if s.chars().count() <= MAX_NAMA => Some(s),
            Value::String(_) => {
                gagal(&mut errors, "items", &format!("Baris {baris}: nama RHK maksimal 255 karakter."));
                None
            }
            _ => {
                gagal(&mut errors, "items", &format!("Baris {baris}: nama RHK wajib diisi."));
                None
            }
        };

        let target = match normalize(raw.get("target").cloned().unwrap_or(Value::Null)) {
            Value::Null => Some(0.0),
            v => match as_number(&v).filter(|n| *n >= 0.0) {
                Some(n) => Some(n),
                None => {
                    gagal(&mut errors, "items", &format!("Baris {baris}: target harus angka tidak negatif."));
                    None
                }
            },
        };

        let satuan = match normalize(raw.get("satuan").cloned().unwrap_or(Value::Null)) {
            Value::Null => Some(DEFAULT_SATUAN.to_string()),
            Value::String(s) if s.chars().count() <= MAX_SATUAN => Some(s),
            _ => {
                gagal(&mut errors, "items", &format!("Baris {baris}: satuan maksimal 50 karakter."));
                None
            }
        };

        let rencana = match raw.get("rencana").and_then(Value::as_array) {
            Some(v) if v.len() == 12 => {
                let mut arr = [0.0f64; 12];
                let mut ok = true;
                for (b, nilai) in v.iter().enumerate() {
                    match as_number(&normalize(nilai.clone())).filter(|n| *n >= 0.0) {
                        Some(n) => arr[b] = n,
                        None => ok = false,
                    }
                }
                if !ok {
                    gagal(&mut errors, "items", &format!("Baris {baris}: rencana tiap bulan harus angka tidak negatif."));
                }
                Some(arr)
            }
            _ => {
                gagal(&mut errors, "items", &format!("Baris {baris}: rencana harus berisi 12 bulan."));
                None
            }
        };

        if let (Some(n), Some(rhk), Some(target), Some(satuan), Some(rencana)) = (no, rhk, target, satuan, rencana) {
            hasil.push(RhkItem {
                no: n,
                rhk,
                target,
                satuan,
                rencana,
            });
        }
    }

    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    hasil.sort_by_key(|i| i.no);
    Ok(hasil)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let tahun = tahun_dari_query(&query)?;
    let items = daftar(&state.pool, user.user_id, tahun).await?;
    Ok(Json(json!({ "data": ke_json(&items) })))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let tahun = tahun_dari_query(&query)?;
    let items = validasi(&normalize_body(body))?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM skp_rhk_rencana WHERE user_id = ? AND tahun = ?")
        .bind(user.user_id)
        .bind(tahun)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM skp_rhk WHERE user_id = ? AND tahun = ?")
        .bind(user.user_id)
        .bind(tahun)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    for item in &items {
        sqlx::query(
            "INSERT INTO skp_rhk (user_id, tahun, no, rhk, target, satuan, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
        )
        .bind(user.user_id)
        .bind(tahun)
        .bind(item.no)
        .bind(item.rhk.as_str())
        .bind(item.target)
        .bind(item.satuan.as_str())
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        for (b, nilai) in item.rencana.iter().enumerate() {
            if *nilai == 0.0 {
                continue;
            }
            sqlx::query(
                "INSERT INTO skp_rhk_rencana (user_id, tahun, bulan, rhk_no, rencana, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(user.user_id)
            .bind(tahun)
            .bind((b + 1) as i64)
            .bind(item.no)
            .bind(nilai)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }
    }
    tx.commit().await.map_err(internal)?;

    let tersimpan = daftar(&state.pool, user.user_id, tahun).await?;
    Ok(Json(json!({ "data": ke_json(&tersimpan) })))
}

/// Body PUT harus objek JSON. Selain itu dianggap tidak ada `items`.
fn normalize_body(body: Value) -> Value {
    if body.is_object() {
        body
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baris(no: i64, rencana: Vec<f64>) -> Value {
        json!({ "no": no, "rhk": format!("RHK {no}"), "target": 12, "satuan": "", "rencana": rencana })
    }

    fn dua_belas(n: f64) -> Vec<f64> {
        vec![n; 12]
    }

    #[test]
    fn validasi_menerima_daftar_dan_mengurutkan_no() {
        let body = json!({ "items": [baris(2, dua_belas(1.0)), baris(1, dua_belas(0.0))] });
        let hasil = validasi(&body).unwrap();
        assert_eq!(hasil.iter().map(|i| i.no).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(hasil[0].satuan, DEFAULT_SATUAN, "satuan kosong memakai bawaan");
    }

    #[test]
    fn validasi_menolak_nomor_ganda_dan_rencana_tidak_lengkap() {
        let body = json!({ "items": [baris(1, dua_belas(1.0)), baris(1, vec![1.0; 3])] });
        let errs = validasi(&body).unwrap_err().errors.unwrap();
        let pesan = errs.get("items").unwrap();
        assert!(pesan.iter().any(|m| m.contains("dipakai lebih dari sekali")));
        assert!(pesan.iter().any(|m| m.contains("rencana harus berisi 12 bulan")));
    }

    #[test]
    fn validasi_menolak_tanpa_array_items() {
        let errs = validasi(&json!({})).unwrap_err().errors.unwrap();
        assert!(errs.contains_key("items"));
    }

    #[test]
    fn tahun_wajib_dalam_rentang() {
        let mut q = HashMap::new();
        assert!(tahun_dari_query(&q).is_err());
        q.insert("tahun".to_string(), "1999".to_string());
        assert!(tahun_dari_query(&q).is_err());
        q.insert("tahun".to_string(), "2026".to_string());
        assert_eq!(tahun_dari_query(&q).unwrap(), 2026);
    }
}
