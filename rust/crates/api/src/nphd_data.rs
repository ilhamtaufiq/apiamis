//! Data NPHD per paket pekerjaan: jenis pengelola, daftar penerima manfaat, dan nomor NPHD terakhir.
//! Admin mengisinya dari halaman NPHD. Form pembuatan naskah membacanya supaya terisi otomatis.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{media::internal, notifications::require_admin, require_auth, AppState};

const MAKS_PENERIMA: usize = 50;
const MAKS_TEKS: usize = 255;

#[derive(Debug, Default, Deserialize)]
pub struct NphdDataInput {
    #[serde(default)]
    pub jenis_pengelola: Option<String>,
    #[serde(default)]
    pub penerima_manfaat: Vec<String>,
    /// Hanya diisi saat NPHD dibuat. Kosong berarti nomor sebelumnya tetap dipakai.
    #[serde(default)]
    pub nomor_nphd: Option<String>,
}

/// Hasil validasi siap disimpan.
#[derive(Debug, Clone, PartialEq)]
pub struct NphdDataValid {
    pub jenis_pengelola: Option<String>,
    pub penerima_manfaat: Vec<String>,
    pub nomor_nphd: Option<String>,
}

fn bersih(teks: Option<&str>) -> Option<String> {
    teks.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Validasi dan rapikan input. Penerima kosong dibuang, dan batas jumlah serta panjang dijaga.
pub(crate) fn cek_input(input: NphdDataInput) -> Result<NphdDataValid, ApiError> {
    let salah = |pesan: &str| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, pesan.to_string());
    let jenis_pengelola = bersih(input.jenis_pengelola.as_deref());
    if jenis_pengelola.as_ref().is_some_and(|j| j.chars().count() > MAKS_TEKS) {
        return Err(salah("Jenis pengelola maksimal 255 karakter."));
    }
    let penerima_manfaat: Vec<String> = input
        .penerima_manfaat
        .iter()
        .filter_map(|p| bersih(Some(p)))
        .collect();
    if penerima_manfaat.len() > MAKS_PENERIMA {
        return Err(salah("Penerima manfaat maksimal 50 nama."));
    }
    if penerima_manfaat.iter().any(|p| p.chars().count() > MAKS_TEKS) {
        return Err(salah("Nama penerima manfaat maksimal 255 karakter."));
    }
    let nomor_nphd = bersih(input.nomor_nphd.as_deref());
    if nomor_nphd.as_ref().is_some_and(|n| n.chars().count() > MAKS_TEKS) {
        return Err(salah("Nomor NPHD maksimal 255 karakter."));
    }
    Ok(NphdDataValid {
        jenis_pengelola,
        penerima_manfaat,
        nomor_nphd,
    })
}

/// Bentuk JSON satu baris. Paket yang belum punya data mendapat nilai kosong.
fn ke_json(pekerjaan_id: u64, jenis: Option<String>, penerima: Vec<String>, nomor: Option<String>) -> Value {
    json!({
        "pekerjaan_id": pekerjaan_id,
        "jenis_pengelola": jenis,
        "penerima_manfaat": penerima,
        "nomor_nphd": nomor,
    })
}

async fn baca_baris(pool: &MySqlPool, pekerjaan_id: u64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT jenis_pengelola, CAST(penerima_manfaat AS CHAR) AS penerima_manfaat, nomor_nphd \
         FROM nphd_pekerjaan WHERE pekerjaan_id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(row) = row else {
        return Ok(ke_json(pekerjaan_id, None, Vec::new(), None));
    };
    let jenis: Option<String> = row.try_get("jenis_pengelola").map_err(internal)?;
    let penerima_json: Option<String> = row.try_get("penerima_manfaat").map_err(internal)?;
    let penerima: Vec<String> = penerima_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let nomor: Option<String> = row.try_get("nomor_nphd").map_err(internal)?;
    Ok(ke_json(pekerjaan_id, jenis, penerima, nomor))
}

/// `GET /api/pekerjaan/{id}/nphd-data`: admin. Data NPHD satu paket.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    Ok(Json(json!({ "data": baca_baris(&state.pool, id).await? })))
}

/// `GET /api/nphd-data`: admin. Data NPHD semua paket yang sudah pernah diisi.
pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let rows = sqlx::query(
        "SELECT CAST(pekerjaan_id AS UNSIGNED) AS pekerjaan_id, jenis_pengelola, \
         CAST(penerima_manfaat AS CHAR) AS penerima_manfaat, nomor_nphd FROM nphd_pekerjaan",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for row in rows {
        let pekerjaan_id: u64 = row.try_get("pekerjaan_id").map_err(internal)?;
        let jenis: Option<String> = row.try_get("jenis_pengelola").map_err(internal)?;
        let penerima_json: Option<String> = row.try_get("penerima_manfaat").map_err(internal)?;
        let penerima: Vec<String> = penerima_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let nomor: Option<String> = row.try_get("nomor_nphd").map_err(internal)?;
        data.push(ke_json(pekerjaan_id, jenis, penerima, nomor));
    }
    Ok(Json(json!({ "data": data })))
}

/// `PUT /api/pekerjaan/{id}/nphd-data`: admin. Simpan jenis pengelola dan penerima manfaat.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(body): Json<NphdDataInput>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let valid = cek_input(body)?;

    let ada: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    if ada == 0 {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "Pekerjaan tidak ditemukan."));
    }

    let penerima_json = serde_json::to_string(&valid.penerima_manfaat).map_err(internal)?;
    sqlx::query(
        "INSERT INTO nphd_pekerjaan (pekerjaan_id, jenis_pengelola, penerima_manfaat, nomor_nphd, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE jenis_pengelola = VALUES(jenis_pengelola), \
         penerima_manfaat = VALUES(penerima_manfaat), \
         nomor_nphd = COALESCE(VALUES(nomor_nphd), nomor_nphd), updated_at = NOW()",
    )
    .bind(id)
    .bind(&valid.jenis_pengelola)
    .bind(&penerima_json)
    .bind(&valid.nomor_nphd)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({ "data": baca_baris(&state.pool, id).await? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(jenis: Option<&str>, penerima: &[&str], nomor: Option<&str>) -> NphdDataInput {
        NphdDataInput {
            jenis_pengelola: jenis.map(str::to_string),
            penerima_manfaat: penerima.iter().map(|s| s.to_string()).collect(),
            nomor_nphd: nomor.map(str::to_string),
        }
    }

    #[test]
    fn penerima_dirapikan_dan_yang_kosong_dibuang() {
        let v = cek_input(input(Some("  Penerima MCK  "), &[" Ani ", "", "  ", "Budi"], Some(" "))).unwrap();
        assert_eq!(v.jenis_pengelola.as_deref(), Some("Penerima MCK"));
        assert_eq!(v.penerima_manfaat, vec!["Ani", "Budi"]);
        assert_eq!(v.nomor_nphd, None);
    }

    #[test]
    fn batas_jumlah_dan_panjang_dijaga() {
        let banyak: Vec<String> = (0..51).map(|i| format!("nama {i}")).collect();
        let refs: Vec<&str> = banyak.iter().map(String::as_str).collect();
        assert!(cek_input(input(None, &refs, None)).is_err());

        let panjang = "x".repeat(256);
        assert!(cek_input(input(None, &[panjang.as_str()], None)).is_err());
        assert!(cek_input(input(Some(&panjang), &[], None)).is_err());
    }

    #[test]
    fn nomor_nphd_tetap_opsional() {
        let v = cek_input(input(None, &[], Some("NPHD/001/2026"))).unwrap();
        assert_eq!(v.nomor_nphd.as_deref(), Some("NPHD/001/2026"));
        assert!(v.penerima_manfaat.is_empty());
    }
}
