//! `GET /api/kecamatan`, setara `KecamatanController@index` + `KecamatanResource`.
//!
//! Kontrak respon (dari kode Laravel dan fixture `fixtures/live/kecamatan_index.json`):
//! `{"data": [{id, nama_kecamatan, jumlah_desa, created_at, updated_at}]}`.
//! `nama_kecamatan` berasal dari kolom `n_kec`, `jumlah_desa` dari hitungan
//! `tbl_desa`, dan timestamp memakai ISO 8601 dengan zona UTC (`+00:00`).

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{format::iso8601_utc, require_auth, AppState};

#[derive(Debug, Clone, PartialEq)]
pub struct KecamatanRow {
    pub id: u64,
    pub n_kec: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub jumlah_desa: i64,
}

/// Mengubah satu baris menjadi bentuk resource Laravel.
pub fn to_resource(row: &KecamatanRow) -> Value {
    json!({
        "id": row.id,
        "nama_kecamatan": row.n_kec,
        "jumlah_desa": row.jumlah_desa,
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    })
}

/// Bentuk `KecamatanDetailResource`: kecamatan beserta daftar desanya.
pub fn detail_resource(row: &KecamatanRow, desa: &[crate::desa::DesaRow]) -> Value {
    json!({
        "id": row.id,
        "nama_kecamatan": row.n_kec,
        "desa": desa.iter().map(crate::desa::base_resource).collect::<Vec<_>>(),
        "jumlah_desa": desa.len(),
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    })
}

/// Satu kecamatan berdasarkan id, dengan jumlah desanya.
pub async fn find(pool: &sqlx::MySqlPool, id: u64) -> Result<Option<KecamatanRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT k.id, k.n_kec, k.created_at, k.updated_at, \
         CAST((SELECT COUNT(*) FROM tbl_desa d WHERE d.kecamatan_id = k.id) AS SIGNED) AS jumlah_desa \
         FROM tbl_kecamatan k WHERE k.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(KecamatanRow {
            id: r.try_get("id")?,
            n_kec: r.try_get("n_kec")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
            jumlah_desa: r.try_get("jumlah_desa")?,
        })
    })
    .transpose()
}

/// `GET /api/kecamatan/{id}`: `{"data": KecamatanDetailResource}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let row = match id.parse::<u64>() {
        Ok(id) => find(&state.pool, id).await.map_err(crate::desa::internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    let desa = crate::desa::list_for_kecamatan(&state.pool, row.id)
        .await
        .map_err(crate::desa::internal)?;
    Ok(Json(json!({ "data": detail_resource(&row, &desa) })))
}

/// Membaca semua kecamatan, urutan `id` seperti `Kecamatan::all()` di Laravel.
pub async fn list(pool: &sqlx::MySqlPool) -> Result<Vec<KecamatanRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT k.id, k.n_kec, k.created_at, k.updated_at, \
         CAST((SELECT COUNT(*) FROM tbl_desa d WHERE d.kecamatan_id = k.id) AS SIGNED) AS jumlah_desa \
         FROM tbl_kecamatan k ORDER BY k.id",
    )
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|r| {
            Ok(KecamatanRow {
                id: r.try_get("id")?,
                n_kec: r.try_get("n_kec")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
                jumlah_desa: r.try_get("jumlah_desa")?,
            })
        })
        .collect()
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthenticated)?;

    auth::authenticate(&state.pool, bearer)
        .await
        .map_err(|_| ApiError::unauthenticated())?;

    let rows = list(&state.pool)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let data: Vec<Value> = rows.iter().map(to_resource).collect();
    Ok(Json(json!({ "data": data })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};

    /// Fixture hasil rekaman dari produksi (token uji, data pribadi dihapus).
    const FIXTURE: &str = include_str!("../../../fixtures/live/kecamatan_index.json");

    /// Kebalikan dari `iso8601_utc`: membaca timestamp dari fixture.
    fn parse_fixture_ts(v: &Value) -> Option<DateTime<Utc>> {
        v.as_str().map(|s| {
            NaiveDateTime::parse_from_str(&s[..19], "%Y-%m-%dT%H:%M:%S")
                .unwrap()
                .and_utc()
        })
    }

    #[test]
    fn every_fixture_item_is_reproduced_exactly() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let items = fixture["body"]["data"].as_array().unwrap();
        assert!(!items.is_empty());

        for item in items {
            let row = KecamatanRow {
                id: item["id"].as_u64().unwrap(),
                n_kec: item["nama_kecamatan"].as_str().unwrap().to_string(),
                created_at: parse_fixture_ts(&item["created_at"]),
                updated_at: parse_fixture_ts(&item["updated_at"]),
                jumlah_desa: item["jumlah_desa"].as_i64().unwrap(),
            };
            assert_eq!(&to_resource(&row), item, "id {}", row.id);
        }
    }

    #[test]
    fn timestamps_use_utc_iso8601_like_carbon() {
        let ts = NaiveDate::from_ymd_opt(2025, 11, 30)
            .unwrap()
            .and_hms_opt(10, 5, 0)
            .unwrap();
        assert_eq!(
            iso8601_utc(Some(ts.and_utc())),
            json!("2025-11-30T10:05:00+00:00")
        );
        assert_eq!(iso8601_utc(None), Value::Null);
    }
}
