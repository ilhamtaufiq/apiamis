//! Nomor surat pernyataan BMD per kontrak: `POST /api/kontrak/{id}/surat-bmd`.
//!
//! Nomor dibuat otomatis dari Penomoran Surat (jenis dengan kode `KEU-SP`) pada pemanggilan pertama.
//! Id surat disimpan di `tbl_kontrak.surat_bmd_id`. Pemanggilan berikutnya memakai surat yang sudah
//! ada dan tidak mengambil nomor urut baru. Baris kontrak dikunci `FOR UPDATE` dalam transaksi, jadi
//! dua permintaan bersamaan untuk kontrak yang sama tidak membuat dua nomor.
//!
//! Bila surat dihapus dari Penomoran Surat, FK `ON DELETE SET NULL` mengosongkan `surat_bmd_id`,
//! sehingga pemanggilan berikutnya membuat nomor baru.

use std::collections::BTreeMap;

use axum::{
    body::Bytes,
    extract::{Path, State},
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
    require_auth,
    skp_jurnal::{gagal, VALIDATION_MESSAGE},
    surat::{self, Masukan},
    AppState,
};

/// Kode jenis dokumen untuk surat pernyataan BMD. Dicocokkan tanpa membedakan huruf besar kecil.
pub const KODE_JENIS: &str = "KEU-SP";
const PERIHAL_AWAL: &str = "Surat Pernyataan Barang Milik Daerah";
const PERIHAL_MAKS: usize = 500;

/// Id jenis dengan kode `KEU-SP` dari daftar `(id, kode)`. Bila ada lebih dari satu, yang id-nya terkecil.
pub fn pilih_jenis(daftar: &[(i64, String)]) -> Option<i64> {
    daftar
        .iter()
        .filter(|(_, kode)| kode.trim().eq_ignore_ascii_case(KODE_JENIS))
        .map(|(id, _)| *id)
        .min()
}

/// Hasil keputusan idempoten untuk satu kontrak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keputusan {
    /// Kontrak sudah punya surat BMD yang masih ada: pakai nomornya.
    PakaiYangAda,
    /// Belum ada (atau surat sudah dihapus): buat nomor baru.
    BuatBaru,
}

/// Surat yang sudah tersimpan dipakai hanya bila `surat_bmd_id` terisi dan baris surat itu ada.
pub fn keputusan(surat_bmd_id: Option<i64>, surat_masih_ada: bool) -> Keputusan {
    match surat_bmd_id {
        Some(_) if surat_masih_ada => Keputusan::PakaiYangAda,
        _ => Keputusan::BuatBaru,
    }
}

/// Perihal surat: `Surat Pernyataan Barang Milik Daerah — <nama paket>`. Tanpa nama paket, hanya judulnya.
pub fn perihal_bmd(nama_paket: Option<&str>) -> String {
    let nama = nama_paket.map(str::trim).unwrap_or("");
    let teks = if nama.is_empty() {
        PERIHAL_AWAL.to_string()
    } else {
        format!("{PERIHAL_AWAL} — {nama}")
    };
    teks.chars().take(PERIHAL_MAKS).collect()
}

/// Body opsional: kosong, atau `{ "tanggal_surat": "YYYY-MM-DD" }`. Tanpa isian, memakai `hari_ini`.
pub fn tanggal_dari_body(body: &[u8], hari_ini: NaiveDate) -> Result<NaiveDate, ApiError> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(hari_ini);
    }
    let raw: Value = serde_json::from_slice(body).map_err(|_| {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Body permintaan harus berupa JSON yang valid.")
    })?;
    let isian = match &raw {
        Value::Null => None,
        Value::Object(o) => o.get("tanggal_surat"),
        _ => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Body permintaan harus berupa objek JSON.",
            ))
        }
    };
    match isian {
        None | Some(Value::Null) => Ok(hari_ini),
        Some(Value::String(s)) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| {
            let mut errors = BTreeMap::new();
            gagal(&mut errors, "tanggal_surat", "Tanggal surat harus berformat YYYY-MM-DD.");
            ApiError::validation(VALIDATION_MESSAGE, errors)
        }),
        Some(_) => {
            let mut errors = BTreeMap::new();
            gagal(&mut errors, "tanggal_surat", "Tanggal surat harus berformat YYYY-MM-DD.");
            Err(ApiError::validation(VALIDATION_MESSAGE, errors))
        }
    }
}

/// Body respons `POST /api/kontrak/{id}/surat-bmd`.
pub fn respons_json(surat_id: i64, nomor: &str, tanggal: &str, dibuat_baru: bool) -> Value {
    json!({
        "data": {
            "surat_id": surat_id,
            "nomor": nomor,
            "tanggal": tanggal,
            "dibuat_baru": dibuat_baru,
        }
    })
}

/// Nomor surat BMD yang tersimpan untuk kontrak, bila ada. Dipakai saat export untuk membandingkan nomor.
pub async fn nomor_tersimpan(pool: &MySqlPool, kontrak_id: i64) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar(
        "SELECT s.nomor FROM tbl_kontrak k JOIN surat s ON s.id = k.surat_bmd_id WHERE k.id = ?",
    )
    .bind(kontrak_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)
}

/// Export BMD menolak nomor yang berbeda dari nomor tersimpan, supaya satu kontrak tidak punya dua nomor.
pub fn cek_nomor_export(nomor_body: &str, tersimpan: Option<&str>) -> Result<(), ApiError> {
    match tersimpan {
        Some(t) if t.trim() != nomor_body.trim() => Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Nomor surat tidak sama dengan nomor yang tersimpan ({t})."),
        )),
        _ => Ok(()),
    }
}

/// `POST /api/kontrak/{id}/surat-bmd`. Body kosong atau `{ "tanggal_surat": "YYYY-MM-DD" }`.
pub async fn buat_nomor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let kontrak_id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let tanggal = tanggal_dari_body(&body, Utc::now().date_naive())?;

    let mut tx = state.pool.begin().await.map_err(internal)?;

    // Kunci baris kontrak sampai commit. Permintaan kedua menunggu, lalu melihat surat yang sudah dibuat.
    let surat_bmd_id: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT CAST(surat_bmd_id AS SIGNED) FROM tbl_kontrak WHERE id = ? FOR UPDATE",
    )
    .bind(kontrak_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let surat_bmd_id = surat_bmd_id.ok_or_else(ApiError::not_found)?;

    // Pekerjaan pertama kontrak, urut id pekerjaan seperti `load_ctx`.
    let pekerjaan = sqlx::query(
        "SELECT CAST(kp.pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(p.nama_paket AS CHAR) AS nama_paket \
         FROM kontrak_pekerjaan kp LEFT JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id \
         WHERE kp.kontrak_id = ? ORDER BY kp.pekerjaan_id LIMIT 1",
    )
    .bind(kontrak_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let Some(pekerjaan) = pekerjaan else {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Kontrak belum terhubung dengan pekerjaan."));
    };
    let pekerjaan_id: i64 = pekerjaan.try_get("pekerjaan_id").map_err(internal)?;
    let nama_paket: Option<String> = pekerjaan.try_get("nama_paket").map_err(internal)?;

    // Surat yang sudah tersimpan dipakai ulang tanpa mengambil nomor urut baru.
    let tersimpan = match surat_bmd_id {
        Some(sid) => sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, nomor, DATE_FORMAT(tanggal, '%Y-%m-%d') AS tanggal \
             FROM surat WHERE id = ?",
        )
        .bind(sid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?
        .map(|r| -> Result<(i64, String, String), ApiError> {
            Ok((
                r.try_get("id").map_err(internal)?,
                r.try_get("nomor").map_err(internal)?,
                r.try_get("tanggal").map_err(internal)?,
            ))
        })
        .transpose()?,
        None => None,
    };

    if let Keputusan::PakaiYangAda = keputusan(surat_bmd_id, tersimpan.is_some()) {
        tx.commit().await.map_err(internal)?;
        let (sid, nomor, tgl) = tersimpan.unwrap_or_default();
        return Ok(Json(respons_json(sid, &nomor, &tgl, false)).into_response());
    }

    let daftar_jenis: Vec<(i64, String)> = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, code FROM tbl_document_types WHERE UPPER(TRIM(code)) = ? ORDER BY id",
    )
    .bind(KODE_JENIS)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| Ok((r.try_get("id").map_err(internal)?, r.try_get("code").map_err(internal)?)))
    .collect::<Result<_, ApiError>>()?;
    let Some(type_id) = pilih_jenis(&daftar_jenis) else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Jenis surat {KODE_JENIS} belum ada di Penomoran Surat."),
        ));
    };

    let masukan = Masukan {
        type_id,
        tanggal,
        perihal: perihal_bmd(nama_paket.as_deref()),
        tujuan: None,
        keterangan: None,
        pekerjaan_id: Some(pekerjaan_id),
        nomor: None,
    };
    let baru = surat::buat_surat_dalam_tx(&mut tx, user.user_id, &masukan).await?;
    sqlx::query("UPDATE tbl_kontrak SET surat_bmd_id = ? WHERE id = ?")
        .bind(baru.id)
        .bind(kontrak_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    let tgl = tanggal.format("%Y-%m-%d").to_string();
    Ok(Json(respons_json(baru.id, &baru.nomor, &tgl, true)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgl(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn galat_field(err: &ApiError) -> Vec<String> {
        err.errors.as_ref().map(|e| e.keys().cloned().collect()).unwrap_or_default()
    }

    #[test]
    fn pilih_jenis_keu_sp_tanpa_peduli_huruf_besar() {
        let daftar = vec![(3, "ST".to_string()), (7, "keu-sp".to_string())];
        assert_eq!(pilih_jenis(&daftar), Some(7));
        let campur = vec![(12, "Keu-Sp".to_string())];
        assert_eq!(pilih_jenis(&campur), Some(12));
    }

    #[test]
    fn pilih_jenis_mengambil_id_terkecil_dan_mengabaikan_kode_mirip() {
        let daftar = vec![(9, "KEU-SP".to_string()), (4, "keu-sp".to_string()), (5, "KEU-SP2".to_string())];
        assert_eq!(pilih_jenis(&daftar), Some(4));
        assert_eq!(pilih_jenis(&[(5, "KEU-SP2".to_string()), (6, "KEU".to_string())]), None);
        assert_eq!(pilih_jenis(&[]), None);
    }

    #[test]
    fn keputusan_memakai_surat_tersimpan_hanya_bila_masih_ada() {
        assert_eq!(keputusan(Some(41), true), Keputusan::PakaiYangAda);
        assert_eq!(keputusan(Some(41), false), Keputusan::BuatBaru);
        assert_eq!(keputusan(None, false), Keputusan::BuatBaru);
        assert_eq!(keputusan(None, true), Keputusan::BuatBaru);
    }

    #[test]
    fn respons_menandai_dibuat_baru_hanya_saat_nomor_baru() {
        let baru = respons_json(7, "001/KEU-SP-AMIS/XI/2026", "2026-11-11", true);
        assert_eq!(baru["data"]["dibuat_baru"], json!(true));
        let lama = respons_json(7, "001/KEU-SP-AMIS/XI/2026", "2026-11-11", false);
        assert_eq!(lama["data"]["dibuat_baru"], json!(false));
        assert_eq!(lama["data"]["surat_id"], json!(7));
        assert_eq!(lama["data"]["nomor"], json!("001/KEU-SP-AMIS/XI/2026"));
        assert_eq!(lama["data"]["tanggal"], json!("2026-11-11"));
    }

    #[test]
    fn tanggal_body_kosong_atau_tanpa_isian_memakai_hari_ini() {
        let hari_ini = tgl("2026-11-11");
        assert_eq!(tanggal_dari_body(b"", hari_ini).unwrap(), hari_ini);
        assert_eq!(tanggal_dari_body(b"  \n", hari_ini).unwrap(), hari_ini);
        assert_eq!(tanggal_dari_body(b"{}", hari_ini).unwrap(), hari_ini);
        assert_eq!(tanggal_dari_body(b"{\"tanggal_surat\":null}", hari_ini).unwrap(), hari_ini);
    }

    #[test]
    fn tanggal_body_dibaca_dan_format_salah_ditolak() {
        let hari_ini = tgl("2026-11-11");
        assert_eq!(tanggal_dari_body(br#"{"tanggal_surat":"2026-01-05"}"#, hari_ini).unwrap(), tgl("2026-01-05"));

        let salah = tanggal_dari_body(br#"{"tanggal_surat":"05/01/2026"}"#, hari_ini).unwrap_err();
        assert_eq!(salah.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(galat_field(&salah), vec!["tanggal_surat".to_string()]);

        let bukan_teks = tanggal_dari_body(br#"{"tanggal_surat":20261105}"#, hari_ini).unwrap_err();
        assert_eq!(galat_field(&bukan_teks), vec!["tanggal_surat".to_string()]);

        let bukan_json = tanggal_dari_body(b"bukan json", hari_ini).unwrap_err();
        assert_eq!(bukan_json.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(tanggal_dari_body(b"[]", hari_ini).is_err());
    }

    #[test]
    fn perihal_menyambung_nama_paket_dan_dipotong_500_karakter() {
        assert_eq!(
            perihal_bmd(Some("  Rehabilitasi Jalan  ")),
            "Surat Pernyataan Barang Milik Daerah — Rehabilitasi Jalan"
        );
        assert_eq!(perihal_bmd(None), "Surat Pernyataan Barang Milik Daerah");
        assert_eq!(perihal_bmd(Some("   ")), "Surat Pernyataan Barang Milik Daerah");
        let panjang = "x".repeat(900);
        assert_eq!(perihal_bmd(Some(&panjang)).chars().count(), PERIHAL_MAKS);
    }

    #[test]
    fn export_menolak_nomor_yang_berbeda_dari_nomor_tersimpan() {
        let err = cek_nomor_export("099/KEU-SP-AMIS/XI/2026", Some("001/KEU-SP-AMIS/XI/2026")).unwrap_err();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            err.message,
            "Nomor surat tidak sama dengan nomor yang tersimpan (001/KEU-SP-AMIS/XI/2026)."
        );
    }

    #[test]
    fn export_menerima_nomor_sama_atau_tanpa_nomor_tersimpan() {
        assert!(cek_nomor_export("001/KEU-SP-AMIS/XI/2026", Some("001/KEU-SP-AMIS/XI/2026")).is_ok());
        assert!(cek_nomor_export(" 001/KEU-SP-AMIS/XI/2026 ", Some("001/KEU-SP-AMIS/XI/2026")).is_ok());
        assert!(cek_nomor_export("apa saja", None).is_ok());
    }
}
