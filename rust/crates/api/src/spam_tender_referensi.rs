//! Referensi tender SPSE untuk paket SPAM (`tbl_spam_tender_referensi`). Hanya dibaca: impor tidak membuat
//! paket pekerjaan dan tidak mengubah unit SPAM.
//!
//! - `POST /api/spam-tender-referensi/import` (admin, multipart `file` berformat CSV SPSE). Baris diunggah ulang
//!   dengan `kode_paket` yang sama akan diperbarui (idempotent). Baris dicocokkan ke desa dari kolom `lokasi`
//!   (`Desa X Kec. Y - Cianjur`), lalu ke unit SPAM pertama pada desa itu. Lokasi yang hanya kabupaten atau
//!   kecamatan tidak dicocokkan.
//! - `GET /api/spam-units/{id}/tender-referensi`: referensi yang cocok ke satu unit.

use std::sync::OnceLock;

use axum::{
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use regex::Regex;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{notifications::require_admin, require_auth, AppState};

/// Batas berkas unggahan. CSV 2013–2025 sekitar 0,3 MB, dibatasi 10 MB.
pub const BODY_LIMIT: usize = 10 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub struct Baris {
    pub kode_paket: String,
    pub sumber: &'static str,
    pub nama_paket: String,
    pub jenis_paket: Option<String>,
    pub metode_pengadaan: Option<String>,
    pub tahun_anggaran: Option<String>,
    pub nilai_pagu: Option<f64>,
    pub nilai_hps: Option<f64>,
    pub nilai_kontrak: Option<f64>,
    pub tahap_paket: Option<String>,
    pub lokasi: Option<String>,
    pub kecamatan_nama: Option<String>,
    pub desa_nama: Option<String>,
    pub nama_pemenang: Option<String>,
    pub url_detail: Option<String>,
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn lokasi_rx() -> &'static Regex {
    static RX: OnceLock<Regex> = OnceLock::new();
    RX.get_or_init(|| {
        Regex::new(r"(?i)(?:desa|ds\.?|kel\.?|kelurahan)\s+(.+?)\s+kec\.?\s*(.+?)\s+-\s+cianjur").expect("regex valid")
    })
}

/// Desa dan kecamatan dari kolom `lokasi`. Hanya kabupaten atau kecamatan menghasilkan `None`.
pub fn urai_lokasi(lokasi: &str) -> Option<(String, String)> {
    let c = lokasi_rx().captures(lokasi)?;
    let desa = c.get(1)?.as_str().trim().to_string();
    let kec = c.get(2)?.as_str().trim().to_string();
    (!desa.is_empty() && !kec.is_empty()).then_some((desa, kec))
}

fn teks(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn angka(v: Option<&str>) -> Option<f64> {
    teks(v).and_then(|s| s.parse::<f64>().ok())
}

/// Membaca CSV SPSE. Kolom dicari dari header, sehingga urutan kolom tidak penting.
pub fn parse_csv(bytes: &[u8]) -> Result<Vec<Baris>, String> {
    let bersih = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(bersih);
    let header: Vec<String> = rdr.headers().map_err(|e| e.to_string())?.iter().map(str::to_string).collect();
    let idx = |nama: &str| header.iter().position(|h| h.trim() == nama);
    let i_kode = idx("kode_paket").ok_or("Kolom kode_paket tidak ada")?;
    let i_nama = idx("nama_paket").ok_or("Kolom nama_paket tidak ada")?;

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let get = |i: Option<usize>| i.and_then(|i| rec.get(i));
        let Some(kode) = teks(get(Some(i_kode))) else { continue };
        let jenis = teks(get(idx("jenis_paket")));
        let lokasi = teks(get(idx("lokasi")));
        let (desa_nama, kecamatan_nama) = match lokasi.as_deref().and_then(urai_lokasi) {
            Some((d, k)) => (Some(d), Some(k)),
            None => (None, None),
        };
        out.push(Baris {
            kode_paket: kode,
            sumber: if jenis.as_deref() == Some("Tender") { "tender" } else { "non_tender" },
            nama_paket: teks(get(Some(i_nama))).unwrap_or_default(),
            jenis_paket: jenis,
            metode_pengadaan: teks(get(idx("metode_pengadaan"))),
            tahun_anggaran: teks(get(idx("tahun_anggaran"))),
            nilai_pagu: angka(get(idx("nilai_pagu"))),
            nilai_hps: angka(get(idx("nilai_hps"))),
            nilai_kontrak: angka(get(idx("nilai_kontrak"))),
            tahap_paket: teks(get(idx("tahap_paket"))),
            lokasi,
            kecamatan_nama,
            desa_nama,
            nama_pemenang: teks(get(idx("nama_pemenang"))),
            url_detail: teks(get(idx("url_detail"))),
        });
    }
    Ok(out)
}

async fn baca_berkas(multipart: &mut Multipart) -> Result<Option<Vec<u8>>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        return Ok(Some(bytes.to_vec()));
    }
    Ok(None)
}

/// `POST /api/spam-tender-referensi/import`. Admin.
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let pool = &state.pool;
    let Some(bytes) = baca_berkas(&mut multipart).await? else {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Berkas CSV wajib diunggah."));
    };
    let baris = parse_csv(&bytes).map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let mut tx = pool.begin().await.map_err(internal)?;
    for b in &baris {
        sqlx::query(
            "INSERT INTO tbl_spam_tender_referensi \
             (kode_paket, sumber, nama_paket, jenis_paket, metode_pengadaan, tahun_anggaran, nilai_pagu, nilai_hps, \
              nilai_kontrak, tahap_paket, lokasi, kecamatan_nama, desa_nama, nama_pemenang, url_detail, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW()) \
             ON DUPLICATE KEY UPDATE sumber = VALUES(sumber), nama_paket = VALUES(nama_paket), \
               jenis_paket = VALUES(jenis_paket), metode_pengadaan = VALUES(metode_pengadaan), \
               tahun_anggaran = VALUES(tahun_anggaran), nilai_pagu = VALUES(nilai_pagu), nilai_hps = VALUES(nilai_hps), \
               nilai_kontrak = VALUES(nilai_kontrak), tahap_paket = VALUES(tahap_paket), lokasi = VALUES(lokasi), \
               kecamatan_nama = VALUES(kecamatan_nama), desa_nama = VALUES(desa_nama), \
               nama_pemenang = VALUES(nama_pemenang), url_detail = VALUES(url_detail), updated_at = NOW()",
        )
        .bind(&b.kode_paket)
        .bind(b.sumber)
        .bind(&b.nama_paket)
        .bind(&b.jenis_paket)
        .bind(&b.metode_pengadaan)
        .bind(&b.tahun_anggaran)
        .bind(b.nilai_pagu)
        .bind(b.nilai_hps)
        .bind(b.nilai_kontrak)
        .bind(&b.tahap_paket)
        .bind(&b.lokasi)
        .bind(&b.kecamatan_nama)
        .bind(&b.desa_nama)
        .bind(&b.nama_pemenang)
        .bind(&b.url_detail)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }
    // Pencocokan hanya membaca tabel desa dan unit. Unit SPAM tidak diubah.
    sqlx::query(
        "UPDATE tbl_spam_tender_referensi r \
         JOIN tbl_kecamatan k ON LOWER(TRIM(k.n_kec)) = LOWER(TRIM(r.kecamatan_nama)) \
         JOIN tbl_desa d ON d.kecamatan_id = k.id AND LOWER(TRIM(d.n_desa)) = LOWER(TRIM(r.desa_nama)) \
         SET r.desa_id = d.id WHERE r.desa_nama IS NOT NULL",
    )
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_spam_tender_referensi r \
         JOIN (SELECT desa_id, MIN(id) AS unit_id FROM tbl_unit_spam GROUP BY desa_id) u ON u.desa_id = r.desa_id \
         SET r.unit_spam_id = u.unit_id",
    )
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let cocok: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spam_tender_referensi WHERE unit_spam_id IS NOT NULL")
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    let dilewati = baris.iter().filter(|b| b.desa_nama.is_none()).count();
    Ok(Json(json!({
        "dibaca": baris.len(),
        "lokasi_desa_dikenali": baris.len() - dilewati,
        "lokasi_hanya_kabupaten_atau_kecamatan": dilewati,
        "terhubung_ke_unit": cocok,
    }))
    .into_response())
}

/// `GET /api/spam-units/{id}/tender-referensi`.
pub async fn index_unit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(unit_id): Path<i64>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let rows = sqlx::query(
        "SELECT kode_paket, sumber, nama_paket, jenis_paket, metode_pengadaan, tahun_anggaran, \
                CAST(nilai_pagu AS DOUBLE) AS nilai_pagu, CAST(nilai_kontrak AS DOUBLE) AS nilai_kontrak, \
                tahap_paket, lokasi, nama_pemenang, url_detail \
         FROM tbl_spam_tender_referensi WHERE unit_spam_id = ? ORDER BY tahun_anggaran, kode_paket LIMIT 200",
    )
    .bind(unit_id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    let mut data: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        data.push(json!({
            "kode_paket": r.try_get::<String, _>("kode_paket").map_err(internal)?,
            "sumber": r.try_get::<String, _>("sumber").map_err(internal)?,
            "nama_paket": r.try_get::<String, _>("nama_paket").map_err(internal)?,
            "jenis_paket": r.try_get::<Option<String>, _>("jenis_paket").map_err(internal)?,
            "metode_pengadaan": r.try_get::<Option<String>, _>("metode_pengadaan").map_err(internal)?,
            "tahun_anggaran": r.try_get::<Option<String>, _>("tahun_anggaran").map_err(internal)?,
            "nilai_pagu": r.try_get::<Option<f64>, _>("nilai_pagu").map_err(internal)?,
            "nilai_kontrak": r.try_get::<Option<f64>, _>("nilai_kontrak").map_err(internal)?,
            "tahap_paket": r.try_get::<Option<String>, _>("tahap_paket").map_err(internal)?,
            "lokasi": r.try_get::<Option<String>, _>("lokasi").map_err(internal)?,
            "nama_pemenang": r.try_get::<Option<String>, _>("nama_pemenang").map_err(internal)?,
            "url_detail": r.try_get::<Option<String>, _>("url_detail").map_err(internal)?,
        }));
    }
    Ok(Json(json!({ "data": data })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lokasi_desa_dan_kecamatan_dikenali() {
        assert_eq!(
            urai_lokasi("Ds. Campakamulya Kec. Campakamulya - Cianjur (Kab.)"),
            Some(("Campakamulya".to_string(), "Campakamulya".to_string()))
        );
        assert_eq!(
            urai_lokasi("Mesjid Al-Amin Kp. Nanggeleng Rt 02/01 Desa Neglasari Kec.Cikalongkulon - Cianjur (Kab.)"),
            Some(("Neglasari".to_string(), "Cikalongkulon".to_string()))
        );
    }

    #[test]
    fn lokasi_tingkat_kabupaten_dan_kecamatan_dilewati() {
        assert_eq!(urai_lokasi("Kabupaten Cianjur - Cianjur (Kab.)"), None);
        assert_eq!(urai_lokasi("Kec. Cianjur - Cianjur (Kab.)"), None);
    }

    #[test]
    fn csv_dengan_bom_dan_kolom_berurutan_bebas() {
        let csv = "\u{feff}kode_paket,nama_paket,jenis_paket,lokasi,nilai_kontrak,tahun_anggaran\n\
                   1,Paket A,Tender,Desa Mangunkerta Kec. Cugenang - Cianjur (Kab.),1234000,APBD 2020\n\
                   2,Paket B,Non Tender,Kabupaten Cianjur - Cianjur (Kab.),5000000,APBD 2021\n";
        let rows = parse_csv(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sumber, "tender");
        assert_eq!(rows[0].desa_nama.as_deref(), Some("Mangunkerta"));
        assert_eq!(rows[0].nilai_kontrak, Some(1_234_000.0));
        assert_eq!(rows[1].sumber, "non_tender");
        assert_eq!(rows[1].desa_nama, None);
        assert_eq!(rows[1].nilai_kontrak, Some(5_000_000.0));
    }
}
