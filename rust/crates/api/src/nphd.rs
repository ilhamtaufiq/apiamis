//! Naskah Perjanjian Hibah Daerah (NPHD) dengan Berita Acara Serah Terima Fisik & Pemeliharaan dalam satu
//! berkas `.docx`. Data diambil dari pekerjaan dan kontrak. Nomor NPHD diberikan pemanggil, yang
//! mengambilnya dari Register Dokumen. Template ada di `storage/app/templates/nphd_mck_template.docx`.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::{Datelike, NaiveDate, Weekday};
use serde::Deserialize;
use shared::ApiError;

use crate::{
    docx_template,
    kontrak_document::{self, file_response},
    kontrak_document_data::{number_format, terbilang, DocCtx},
    media::internal,
    require_auth, AppState,
};

const TEMPLATE_MCK: &str = "nphd_mck_template.docx";
const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const BULAN: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober",
    "November", "Desember",
];

#[derive(Debug, Deserialize)]
pub struct NphdQuery {
    /// Nomor NPHD dari Register Dokumen.
    pub nomor: String,
    /// Tanggal NPHD, format `YYYY-MM-DD`.
    pub tanggal: String,
    /// Uraian objek hibah, misalnya `1 Paket Pembangunan (MCK 2 unit/pintu dan Tangki Septik 2 unit)`.
    pub objek: String,
    /// Nilai aset bangunan dalam rupiah.
    pub nilai: f64,
    /// Sebutan penerima, misalnya `Penerima Manfaat MCK Individu`.
    #[serde(default)]
    pub pengelola_label: String,
    #[serde(default)]
    pub pengelola_1: String,
    #[serde(default)]
    pub pengelola_2: String,
}

/// `GET /api/pekerjaan/{id}/nphd`: unduh NPHD dan BA serah terima sebagai `.docx`.
pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<NphdQuery>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let tanggal = validasi(&q)?;

    let kontrak = kontrak_document::kontrak_of_pekerjaan(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Pekerjaan ini belum punya kontrak."))?;
    let ctx = kontrak_document::load_ctx(&state.pool, kontrak).await?;

    let values = nilai_template(&ctx, &q, tanggal);
    let bytes = tokio::fs::read(kontrak_document::template_dir().join(TEMPLATE_MCK))
        .await
        .map_err(internal)?;
    let filled = docx_template::fill(&bytes, &values).map_err(internal)?;
    let nama = ctx.pekerjaan.as_ref().and_then(|p| p.nama_paket.clone()).unwrap_or_default();
    let name = format!("NPHD_{}_{}.docx", slug(&nama), tanggal.format("%Y%m%d"));
    Ok(file_response(filled, &name, DOCX_MIME))
}

fn validasi(q: &NphdQuery) -> Result<NaiveDate, ApiError> {
    let salah = |pesan: &str| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, pesan.to_string());
    if q.nomor.trim().is_empty() || q.nomor.chars().count() > 255 {
        return Err(salah("Nomor NPHD wajib diisi (maks 255 karakter)."));
    }
    if q.objek.trim().is_empty() {
        return Err(salah("Objek hibah wajib diisi."));
    }
    if !q.nilai.is_finite() || q.nilai <= 0.0 {
        return Err(salah("Nilai aset harus lebih dari nol."));
    }
    NaiveDate::parse_from_str(&q.tanggal, "%Y-%m-%d").map_err(|_| salah("Tanggal NPHD tidak valid."))
}

/// Nilai placeholder untuk template NPHD MCK. Nama kosong menjadi `-` (perilaku `docx_template::fill`).
fn nilai_template(ctx: &DocCtx, q: &NphdQuery, tanggal: NaiveDate) -> Vec<(String, String)> {
    let desa = ctx.pekerjaan.as_ref().and_then(|p| p.nama_desa.clone()).unwrap_or_default();
    let kecamatan = ctx.pekerjaan.as_ref().and_then(|p| p.nama_kecamatan.clone()).unwrap_or_default();
    let penyedia = ctx.penyedia.as_ref().and_then(|p| p.nama.clone()).unwrap_or_default();
    let nomor_spk = ctx.kontrak.spk.clone().unwrap_or_default();
    nilai_dari(&desa, &kecamatan, &penyedia, &nomor_spk, q, tanggal)
}

/// Pasangan placeholder dan nilainya. Dipisah dari `nilai_template` supaya bisa diuji tanpa database.
pub(crate) fn nilai_dari(
    desa: &str,
    kecamatan: &str,
    penyedia: &str,
    nomor_spk: &str,
    q: &NphdQuery,
    tanggal: NaiveDate,
) -> Vec<(String, String)> {
    let label = if q.pengelola_label.trim().is_empty() {
        "Penerima Manfaat MCK Individu".to_string()
    } else {
        q.pengelola_label.trim().to_string()
    };
    let pengelola: Vec<&str> = [q.pengelola_1.trim(), q.pengelola_2.trim()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let desa = judul(desa);
    let kecamatan = judul(kecamatan);

    let pairs = [
        ("nomor_nphd", q.nomor.trim().to_string()),
        ("hari_tanggal", hari_tanggal(tanggal)),
        ("tanggal", tanggal.format("%d-%m-%Y").to_string()),
        ("objek", q.objek.trim().to_string()),
        ("nilai_aset", format!("Rp.{},00", number_format(q.nilai))),
        ("nomor_spk", nomor_spk.to_string()),
        ("penyedia", penyedia.to_string()),
        ("pengelola_label", label.clone()),
        ("pengelola_label_upper", label.to_uppercase()),
        ("pengelola_1", q.pengelola_1.trim().to_string()),
        ("pengelola_2", q.pengelola_2.trim().to_string()),
        ("daftar_pengelola", pengelola.join(", ")),
        ("desa", desa.clone()),
        ("desa_upper", desa.to_uppercase()),
        ("kecamatan", kecamatan.clone()),
        ("kecamatan_upper", kecamatan.to_uppercase()),
    ];
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Contoh: `Senin tanggal Tiga Belas bulan Juli tahun Dua Ribu Dua Puluh Enam`.
pub(crate) fn hari_tanggal(tanggal: NaiveDate) -> String {
    format!(
        "{} tanggal {} bulan {} tahun {}",
        hari(tanggal.weekday()),
        terbilang(tanggal.day() as i64),
        BULAN[tanggal.month0() as usize],
        terbilang(tanggal.year() as i64),
    )
}

fn hari(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Senin",
        Weekday::Tue => "Selasa",
        Weekday::Wed => "Rabu",
        Weekday::Thu => "Kamis",
        Weekday::Fri => "Jumat",
        Weekday::Sat => "Sabtu",
        Weekday::Sun => "Minggu",
    }
}

/// Huruf awal setiap kata menjadi kapital, sisanya kecil (`CILOTO` menjadi `Ciloto`).
pub(crate) fn judul(nilai: &str) -> String {
    nilai
        .split_whitespace()
        .map(|kata| {
            let mut c = kata.chars();
            match c.next() {
                Some(depan) => depan.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn slug(value: &str) -> String {
    let s: String = value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() {
        "pekerjaan".to_string()
    } else {
        trimmed.chars().take(60).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hari_tanggal_sesuai_naskah_contoh() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        assert_eq!(hari_tanggal(d), "Senin tanggal Tiga Belas bulan Juli tahun Dua Ribu Dua Puluh Enam");
    }

    #[test]
    fn judul_mengecilkan_huruf_selain_awal_kata() {
        assert_eq!(judul("CILOTO"), "Ciloto");
        assert_eq!(judul("kp. pasirkuntul  rt"), "Kp. Pasirkuntul Rt");
    }

    #[test]
    fn validasi_menolak_tanggal_dan_nilai_tidak_valid() {
        let mut q = NphdQuery {
            nomor: "001/NPHD".into(),
            tanggal: "2026-07-13".into(),
            objek: "1 Paket".into(),
            nilai: 49_501_500.0,
            pengelola_label: String::new(),
            pengelola_1: String::new(),
            pengelola_2: String::new(),
        };
        assert!(validasi(&q).is_ok());
        q.tanggal = "13-07-2026".into();
        assert!(validasi(&q).is_err());
        q.tanggal = "2026-07-13".into();
        q.nilai = 0.0;
        assert!(validasi(&q).is_err());
    }

    #[test]
    fn template_mck_terisi_semua_placeholder() {
        let q = NphdQuery {
            nomor: "001/NPHD/2026".into(),
            tanggal: "2026-07-13".into(),
            objek: "1 Paket Pembangunan (MCK 2 unit/pintu dan Tangki Septik 2 unit)".into(),
            nilai: 49_501_500.0,
            pengelola_label: "Penerima Manfaat MCK Individu".into(),
            pengelola_1: "Indra Permana".into(),
            pengelola_2: "Jajang Suhendar".into(),
        };
        let tanggal = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        let values = nilai_dari("CILOTO", "CIPANAS", "CV. CAHAYA MANDIRI", "602.4/SPK/PPK/DISPERKIM-AMS.088.350/2026", &q, tanggal);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../storage/app/templates/nphd_mck_template.docx");
        let bytes = std::fs::read(path).expect("template NPHD MCK");
        let filled = docx_template::fill(&bytes, &values).expect("isi template");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(filled)).expect("docx valid");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").expect("document.xml"), &mut xml).unwrap();
        // Pola placeholder saja (`{nama_snake}`), bukan GUID di atribut XML.
        let placeholder_sisa = |teks: &str| -> Option<String> {
            let mut mulai = teks.match_indices('{');
            mulai.find_map(|(i, _)| {
                let sisa = &teks[i + 1..];
                let akhir = sisa.find('}')?;
                let nama = &sisa[..akhir];
                let valid = !nama.is_empty() && nama.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                valid.then(|| format!("{{{nama}}}"))
            })
        };
        assert_eq!(placeholder_sisa(&xml), None, "placeholder belum terisi");
        for nilai in [
            "Desa Ciloto Kecamatan Cipanas",
            "Indra Permana",
            "Jajang Suhendar",
            "CV. CAHAYA MANDIRI",
            "602.4/SPK/PPK/DISPERKIM-AMS.088.350/2026",
            "Rp.49.501.500,00",
            "Senin tanggal Tiga Belas bulan Juli tahun Dua Ribu Dua Puluh Enam",
            "13-07-2026",
            "001/NPHD/2026",
            "Penerima Manfaat MCK Individu Desa Ciloto Kecamatan Cipanas",
        ] {
            assert!(xml.contains(nilai), "tidak ada di dokumen: {nilai}");
        }
    }
}
