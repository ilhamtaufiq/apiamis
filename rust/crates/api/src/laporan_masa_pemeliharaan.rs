//! Rekap masa pemeliharaan per paket pekerjaan (`GET /api/laporan/masa-pemeliharaan`).
//!
//! Sumber tanggal adalah register dokumen BASTP (berita acara serah terima pekerjaan) pada kontrak paket.
//! Akhir masa pemeliharaan = tanggal BASTP + `kontrak_masa_pemeliharaan_hari`, sama dengan dokumen kontrak
//! (`masa_pemeliharaan_dari_bastp`). Pengaturan dibaca lewat `kontrak_document::load_settings`, jadi nilai
//! default dan validasinya ikut yang sudah dipakai dokumen.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{kontrak_document, require_auth, AppState};

const SQL_BASTP: &str = "SELECT CAST(p.id AS SIGNED) AS pekerjaan_id, p.nama_paket, r.nomor, r.tanggal \
     FROM tbl_document_registers r \
     JOIN tbl_document_types t ON t.id = r.type_id AND UPPER(t.code) = 'BASTP' \
     JOIN tbl_kontrak k ON k.id = r.kontrak_id \
     JOIN kontrak_pekerjaan kp ON kp.kontrak_id = k.id \
     JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id \
     WHERE r.tanggal IS NOT NULL \
     ORDER BY r.tanggal, p.nama_paket, r.id";

/// Akhir masa pemeliharaan dari tanggal BASTP dan lama masa dalam hari.
pub fn akhir_masa(bastp: NaiveDate, masa_hari: i64) -> NaiveDate {
    bastp + Duration::days(masa_hari)
}

/// Status relatif terhadap `hari_ini`: masa yang belum lewat tanggal akhirnya masih berjalan.
pub fn status_masa(akhir: NaiveDate, hari_ini: NaiveDate) -> &'static str {
    if akhir >= hari_ini {
        "berjalan"
    } else {
        "berakhir"
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `GET /api/laporan/masa-pemeliharaan`. Tanpa parameter; satu baris per paket yang punya BASTP.
pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let settings = kontrak_document::load_settings(&state.pool).await?;
    let masa = settings.masa_pemeliharaan_hari;
    let hari_ini = Utc::now().date_naive();
    let rows = rekap(&state.pool, masa, hari_ini).await.map_err(internal)?;
    Ok(Json(json!({
        "masa_pemeliharaan_hari": masa,
        "data": rows,
    }))
    .into_response())
}

async fn rekap(pool: &MySqlPool, masa: i64, hari_ini: NaiveDate) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(SQL_BASTP).fetch_all(pool).await?;
    rows.iter()
        .map(|r| {
            let tanggal: NaiveDate = r.try_get("tanggal")?;
            let akhir = akhir_masa(tanggal, masa);
            Ok(json!({
                "pekerjaan_id": r.try_get::<i64, _>("pekerjaan_id")?,
                "nama_paket": r.try_get::<Option<String>, _>("nama_paket")?,
                "nomor_bastp": r.try_get::<Option<String>, _>("nomor")?,
                "tanggal_bastp": tanggal.format("%Y-%m-%d").to_string(),
                "tanggal_akhir": akhir.format("%Y-%m-%d").to_string(),
                "status": status_masa(akhir, hari_ini),
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgl(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn akhir_masa_menambah_hari_dari_bastp() {
        assert_eq!(akhir_masa(tgl("2026-01-10"), 180), tgl("2026-07-09"));
        assert_eq!(akhir_masa(tgl("2026-01-10"), 0), tgl("2026-01-10"));
    }

    #[test]
    fn status_berjalan_sampai_hari_akhir_termasuk() {
        let akhir = tgl("2026-07-09");
        assert_eq!(status_masa(akhir, tgl("2026-07-09")), "berjalan");
        assert_eq!(status_masa(akhir, tgl("2026-07-10")), "berakhir");
    }
}
