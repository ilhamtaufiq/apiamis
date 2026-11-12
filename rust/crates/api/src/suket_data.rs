//! Surat Keterangan Kepala Desa (suket desa) untuk kontrak: validasi body
//! `POST /api/kontrak/{id}/export-suket-desa` dan penyusunan nilai placeholder template
//! `suket_desa_template.docx`.
//!
//! Data desa, kecamatan, paket, penyedia, dan tanggal kontrak diambil dari `DocCtx`
//! (dimuat di `kontrak_document::load_ctx`). Nilai kosong dibiarkan kosong; penggantian ke `-`
//! dilakukan di `docx_template::fill`.

use axum::http::StatusCode;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use shared::ApiError;

use crate::{
    kontrak_document_data::{tanggal, DocCtx},
    nphd::judul,
};

const NOMOR_SURAT_MAKS: usize = 200;
const NAMA_KEPALA_DESA_MAKS: usize = 200;
const ALAMAT_KANTOR_MAKS: usize = 300;
const SERAH_TERIMA_MAKS: usize = 300;
const NOMOR_BA_MAKS: usize = 200;

#[derive(Debug, Deserialize)]
struct RawInput {
    nomor_surat: Option<String>,
    tanggal_surat: Option<String>,
    nama_kepala_desa: Option<String>,
    alamat_kantor: Option<String>,
    serah_terima_kepada: Option<String>,
    nomor_ba: Option<String>,
    tanggal_ba: Option<String>,
}

/// Input `export-suket-desa` setelah validasi. Semua field wajib diisi.
#[derive(Debug, Clone, PartialEq)]
pub struct SuketForm {
    pub nomor_surat: String,
    pub tanggal_surat: NaiveDate,
    pub nama_kepala_desa: String,
    pub alamat_kantor: String,
    pub serah_terima_kepada: String,
    pub nomor_ba: String,
    pub tanggal_ba: NaiveDate,
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, message)
}

/// Teks yang dipangkas; `None` bila kosong atau hanya spasi.
fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Field teks wajib dengan batas panjang dalam karakter.
fn teks(value: Option<String>, field: &str, maks: usize) -> Result<String, ApiError> {
    let isi =
        non_empty(value.as_deref()).ok_or_else(|| invalid(format!("{field} wajib diisi.")))?;
    if isi.chars().count() > maks {
        return Err(invalid(format!("{field} maksimal {maks} karakter.")));
    }
    Ok(isi)
}

/// Field tanggal wajib dengan format `YYYY-MM-DD`.
fn tanggal_wajib(value: Option<String>, field: &str) -> Result<NaiveDate, ApiError> {
    let raw =
        non_empty(value.as_deref()).ok_or_else(|| invalid(format!("{field} wajib diisi.")))?;
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
        .map_err(|_| invalid(format!("{field} harus berformat YYYY-MM-DD.")))
}

/// Membaca dan memvalidasi body `export-suket-desa`. Semua error memakai status 422.
pub fn parse_form(body: &[u8]) -> Result<SuketForm, ApiError> {
    let raw: RawInput = serde_json::from_slice(body)
        .map_err(|_| invalid("Body permintaan harus berupa JSON yang valid."))?;

    Ok(SuketForm {
        nomor_surat: teks(raw.nomor_surat, "nomor_surat", NOMOR_SURAT_MAKS)?,
        tanggal_surat: tanggal_wajib(raw.tanggal_surat, "tanggal_surat")?,
        nama_kepala_desa: teks(
            raw.nama_kepala_desa,
            "nama_kepala_desa",
            NAMA_KEPALA_DESA_MAKS,
        )?,
        alamat_kantor: teks(raw.alamat_kantor, "alamat_kantor", ALAMAT_KANTOR_MAKS)?,
        serah_terima_kepada: teks(
            raw.serah_terima_kepada,
            "serah_terima_kepada",
            SERAH_TERIMA_MAKS,
        )?,
        nomor_ba: teks(raw.nomor_ba, "nomor_ba", NOMOR_BA_MAKS)?,
        tanggal_ba: tanggal_wajib(raw.tanggal_ba, "tanggal_ba")?,
    })
}

/// Tanggal dalam format `07 Mei 2026`; `-` bila kosong.
fn tanggal_atau_strip(date: Option<NaiveDate>) -> String {
    date.map(tanggal).unwrap_or_else(|| "-".to_string())
}

/// Jabatan penandatangan: `KEPALA DESA <nama desa dalam huruf besar>`.
pub fn jabatan(desa: &str) -> String {
    format!("KEPALA DESA {}", desa.to_uppercase())
}

/// Nilai placeholder (key, value) untuk template suket desa. Desa dan kecamatan memakai huruf awal
/// kapital (`judul`). Kegiatan yang tidak punya tahun anggaran memakai tahun dari tanggal SPK.
pub fn build(ctx: &DocCtx, form: &SuketForm) -> Result<Vec<(String, String)>, ApiError> {
    let pekerjaan = ctx
        .pekerjaan
        .as_ref()
        .ok_or_else(|| invalid("Kontrak belum terhubung dengan pekerjaan."))?;
    let desa_mentah = non_empty(pekerjaan.nama_desa.as_deref())
        .ok_or_else(|| invalid("Nama desa pada pekerjaan belum diisi."))?;
    let desa = judul(&desa_mentah);
    let kecamatan = judul(pekerjaan.nama_kecamatan.as_deref().unwrap_or(""));
    let nama_paket = non_empty(pekerjaan.nama_paket.as_deref()).unwrap_or_default();
    let penyedia = ctx
        .penyedia
        .as_ref()
        .and_then(|p| non_empty(p.nama.as_deref()))
        .unwrap_or_default();
    let tahun_anggaran = ctx
        .kegiatan
        .as_ref()
        .and_then(|k| k.tahun_anggaran)
        .or_else(|| ctx.kontrak.tgl_spk.map(|t| i64::from(t.year())))
        .map(|t| t.to_string())
        .unwrap_or_else(|| "-".to_string());

    let values = [
        ("nomor_surat", form.nomor_surat.clone()),
        ("tanggal_surat", tanggal(form.tanggal_surat)),
        ("nama_kepala_desa", form.nama_kepala_desa.clone()),
        ("jabatan", jabatan(&desa)),
        ("alamat_kantor", form.alamat_kantor.clone()),
        ("desa", desa.clone()),
        ("kecamatan", kecamatan),
        ("nama_paket", nama_paket),
        ("penyedia", penyedia),
        ("tahun_anggaran", tahun_anggaran),
        ("tanggal_mulai", tanggal_atau_strip(ctx.kontrak.tgl_spk)),
        (
            "tanggal_selesai",
            tanggal_atau_strip(ctx.kontrak.tgl_selesai),
        ),
        ("serah_terima_kepada", form.serah_terima_kepada.clone()),
        ("nomor_ba", form.nomor_ba.clone()),
        ("tanggal_ba", tanggal(form.tanggal_ba)),
    ];
    Ok(values
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use regex::Regex;
    use zip::ZipArchive;

    use super::*;
    use crate::{
        docx_template,
        kontrak_document::template_dir,
        kontrak_document_data::{tests::sample_ctx, KegiatanDoc, PekerjaanDoc, PenyediaDoc},
    };

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn body() -> String {
        r#"{"nomor_surat":"141/045-SKD/2026","tanggal_surat":"2026-11-12","nama_kepala_desa":"ASEP SUPRIATNA","alamat_kantor":"Jl. Raya Simpang No. 1, Takokak","serah_terima_kepada":"Pemerintah Desa Simpang","nomor_ba":"602/12/BA-SERAH/2026","tanggal_ba":"2026-07-06"}"#
            .to_string()
    }

    fn form() -> SuketForm {
        parse_form(body().as_bytes()).unwrap()
    }

    /// Kontrak dengan pekerjaan di Desa Simpang, Kecamatan Takokak, tanggal 07 Mei s/d 06 Juli 2026.
    fn ctx_simpang() -> DocCtx {
        let mut ctx = sample_ctx();
        ctx.kontrak.tgl_spk = Some(d(2026, 5, 7));
        ctx.kontrak.tgl_selesai = Some(d(2026, 7, 6));
        ctx.pekerjaan = Some(PekerjaanDoc {
            id: 7,
            nama_paket: Some("Pembangunan Jalan Usaha Tani".into()),
            pagu: Some(1_500_000.0),
            kode_rekening: None,
            nama_kecamatan: Some("TAKOKAK".into()),
            nama_desa: Some("SIMPANG".into()),
        });
        ctx.kegiatan = Some(KegiatanDoc {
            tahun_anggaran: Some(2026),
            ..ctx.kegiatan.clone().unwrap()
        });
        ctx.penyedia = Some(PenyediaDoc {
            nama: Some("CV MAJU JAYA".into()),
            direktur: None,
            alamat: None,
            bank: None,
            norek: None,
            npwp: None,
            no_akta: None,
            notaris: None,
            tanggal_akta: None,
        });
        ctx
    }

    fn value(values: &[(String, String)], key: &str) -> String {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("key {key} tidak ada"))
    }

    #[test]
    fn format_tanggal_dua_digit_hari_dan_bulan_indonesia() {
        assert_eq!(tanggal_atau_strip(Some(d(2026, 5, 7))), "07 Mei 2026");
        assert_eq!(tanggal_atau_strip(Some(d(2026, 7, 6))), "06 Juli 2026");
        assert_eq!(tanggal_atau_strip(None), "-");
    }

    #[test]
    fn jabatan_huruf_besar() {
        assert_eq!(jabatan("Simpang"), "KEPALA DESA SIMPANG");
        assert_eq!(jabatan("Ciloto Hilir"), "KEPALA DESA CILOTO HILIR");
    }

    #[test]
    fn judul_desa_dan_kalimat_contoh() {
        let values = build(&ctx_simpang(), &form()).unwrap();
        assert_eq!(value(&values, "desa"), "Simpang");
        assert_eq!(value(&values, "kecamatan"), "Takokak");
        assert_eq!(value(&values, "jabatan"), "KEPALA DESA SIMPANG");
        assert_eq!(value(&values, "tanggal_mulai"), "07 Mei 2026");
        assert_eq!(value(&values, "tanggal_selesai"), "06 Juli 2026");
        assert_eq!(value(&values, "tanggal_surat"), "12 November 2026");
        assert_eq!(value(&values, "tanggal_ba"), "06 Juli 2026");
        assert_eq!(value(&values, "tahun_anggaran"), "2026");
        assert_eq!(value(&values, "nama_paket"), "Pembangunan Jalan Usaha Tani");
        assert_eq!(value(&values, "penyedia"), "CV MAJU JAYA");
    }

    #[test]
    fn tahun_anggaran_fallback_ke_tahun_spk() {
        let mut ctx = ctx_simpang();
        ctx.kegiatan = None;
        let values = build(&ctx, &form()).unwrap();
        assert_eq!(value(&values, "tahun_anggaran"), "2026");

        ctx.kontrak.tgl_spk = None;
        let values = build(&ctx, &form()).unwrap();
        assert_eq!(value(&values, "tahun_anggaran"), "-");
        assert_eq!(value(&values, "tanggal_mulai"), "-");
    }

    #[test]
    fn pekerjaan_atau_desa_kosong_ditolak_422() {
        let mut ctx = ctx_simpang();
        ctx.pekerjaan = None;
        let err = build(&ctx, &form()).unwrap_err();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.message, "Kontrak belum terhubung dengan pekerjaan.");

        let mut ctx = ctx_simpang();
        if let Some(p) = ctx.pekerjaan.as_mut() {
            p.nama_desa = Some("   ".into());
        }
        let err = build(&ctx, &form()).unwrap_err();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err.message, "Nama desa pada pekerjaan belum diisi.");
    }

    #[test]
    fn body_valid_dan_field_teks_dipangkas() {
        let f = parse_form(
            body()
                .replace(
                    "\"nama_kepala_desa\":\"ASEP SUPRIATNA\"",
                    "\"nama_kepala_desa\":\"  ASEP SUPRIATNA  \"",
                )
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(f.nama_kepala_desa, "ASEP SUPRIATNA");
        assert_eq!(f.tanggal_ba, d(2026, 7, 6));
    }

    #[test]
    fn setiap_field_wajib_dan_pesan_menyebut_field() {
        let kosong = |field: &str| {
            let raw = body().replace(
                &format!("\"{field}\":"),
                &format!("\"{field}\":\"\",\"_x\":"),
            );
            parse_form(raw.as_bytes()).unwrap_err()
        };
        for field in [
            "nomor_surat",
            "tanggal_surat",
            "nama_kepala_desa",
            "alamat_kantor",
            "serah_terima_kepada",
            "nomor_ba",
            "tanggal_ba",
        ] {
            let err = kosong(field);
            assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
            assert!(err.message.contains(field), "{field}: {}", err.message);
        }
        let tanpa_nomor = body().replace("\"nomor_surat\":\"141/045-SKD/2026\",", "");
        assert_eq!(
            parse_form(tanpa_nomor.as_bytes()).unwrap_err().message,
            "nomor_surat wajib diisi."
        );
    }

    #[test]
    fn body_tidak_valid_ditolak_422() {
        let status = |raw: &str| parse_form(raw.as_bytes()).unwrap_err().status;
        assert_eq!(status(""), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status("bukan json"), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status("[]"), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            status(&body().replace("2026-11-12", "12/11/2026")),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            status(&body().replace("2026-07-06", "2026-02-30")),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn batas_panjang_per_field() {
        let dengan = |field: &str, nilai: &str| {
            let raw = body().replace(
                &format!("\"{field}\":"),
                &format!("\"{field}\":\"{nilai}\",\"_x\":"),
            );
            parse_form(raw.as_bytes())
        };
        // Batas tepat diterima, satu karakter lebih ditolak.
        assert!(dengan("nomor_surat", &"x".repeat(200)).is_ok());
        assert_eq!(
            dengan("nomor_surat", &"x".repeat(201)).unwrap_err().message,
            "nomor_surat maksimal 200 karakter."
        );
        assert!(dengan("alamat_kantor", &"x".repeat(300)).is_ok());
        assert_eq!(
            dengan("alamat_kantor", &"x".repeat(301))
                .unwrap_err()
                .message,
            "alamat_kantor maksimal 300 karakter."
        );
        assert!(dengan("serah_terima_kepada", &"x".repeat(300)).is_ok());
        assert_eq!(
            dengan("serah_terima_kepada", &"x".repeat(301))
                .unwrap_err()
                .message,
            "serah_terima_kepada maksimal 300 karakter."
        );
        assert!(dengan("nama_kepala_desa", &"x".repeat(200)).is_ok());
        assert_eq!(
            dengan("nama_kepala_desa", &"x".repeat(201))
                .unwrap_err()
                .message,
            "nama_kepala_desa maksimal 200 karakter."
        );
        assert!(dengan("nomor_ba", &"x".repeat(200)).is_ok());
        assert_eq!(
            dengan("nomor_ba", &"x".repeat(201)).unwrap_err().message,
            "nomor_ba maksimal 200 karakter."
        );
    }

    #[test]
    fn template_default_terisi_penuh() {
        let template = std::fs::read(template_dir().join("suket_desa_template.docx"))
            .expect("template suket desa ada");
        let values = build(&ctx_simpang(), &form()).unwrap();

        // Setiap placeholder di template harus dikenal oleh `build`, dan setiap key dari `build` harus ada di template.
        let mut archive = ZipArchive::new(std::io::Cursor::new(&template)).unwrap();
        let mut raw = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut raw)
            .unwrap();
        let re = Regex::new(r"\{([^{}]*)\}").unwrap();
        for cap in re.captures_iter(&raw) {
            let key = &cap[1];
            assert!(
                values.iter().any(|(k, _)| k == key),
                "placeholder tak dikenal: {key}"
            );
        }
        for (key, _) in &values {
            assert!(
                raw.contains(&format!("{{{key}}}")),
                "key tidak ada di template: {key}"
            );
        }

        let filled = docx_template::fill(&template, &values).unwrap();
        let mut archive = ZipArchive::new(std::io::Cursor::new(&filled)).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(
            !xml.contains('{') && !xml.contains('}'),
            "placeholder tersisa"
        );
        assert!(xml.contains("Di wilayah Desa Simpang Kecamatan Takokak"));
        assert!(xml.contains("KEPALA DESA SIMPANG"));
    }
}
