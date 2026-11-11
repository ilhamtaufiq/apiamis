//! Surat Pernyataan Pencatatan Barang Milik Daerah (BMD) untuk kontrak: validasi body
//! `POST /api/kontrak/{id}/export-bmd` dan penyusunan nilai placeholder template
//! `surat_pernyataan_bmd_template.docx`.
//!
//! Data kontrak diambil dari `DocCtx` (dimuat di `kontrak_document::load_ctx`). Tanggal, uang, dan
//! terbilang memakai helper dari `kontrak_document_data`. Nilai kosong dibiarkan kosong; penggantian ke
//! `-` dilakukan di `docx_template::fill`.

use axum::http::StatusCode;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use shared::ApiError;

use crate::{
    kontrak::AddendumRow,
    kontrak_document_data::{number_format, tanggal, terbilang, DocCtx, PenyediaDoc, MONTHS},
};

const HARI: [&str; 7] = [
    "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu",
];
const JABATAN_DEFAULT: &str = "Pengelola Barang";
const NOMOR_SURAT_MAKS: usize = 200;

#[derive(Debug, Deserialize)]
struct RawInput {
    nomor_surat: Option<String>,
    tanggal_surat: Option<String>,
    tanggal_perolehan: Option<String>,
}

/// Input `export-bmd` setelah validasi. Data pejabat tidak dari body, melainkan dari pengaturan (`BmdPejabat`).
#[derive(Debug, Clone, PartialEq)]
pub struct BmdForm {
    pub nomor_surat: String,
    pub tanggal_surat: NaiveDate,
    pub tanggal_perolehan: NaiveDate,
}

/// Pejabat penandatangan dari pengaturan aplikasi (`bmd_kasubag_*`, `bmd_pengelola_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct BmdPejabat {
    pub kasubag_nama: String,
    pub kasubag_nip: String,
    pub pengelola_nama: String,
    pub pengelola_nip: String,
    pub pengelola_pangkat: String,
    pub pengelola_jabatan: String,
}

/// Nilai mentah pengaturan pejabat; `None` bila belum diisi.
#[derive(Debug, Clone, Default)]
pub struct RawPejabat {
    pub kasubag_nama: Option<String>,
    pub kasubag_nip: Option<String>,
    pub pengelola_nama: Option<String>,
    pub pengelola_nip: Option<String>,
    pub pengelola_pangkat: Option<String>,
    pub pengelola_jabatan: Option<String>,
}

/// Kunci pengaturan untuk pejabat BMD. Urutannya sama dengan `RawPejabat`.
pub const PEJABAT_KEYS: [&str; 6] = [
    "bmd_kasubag_nama",
    "bmd_kasubag_nip",
    "bmd_pengelola_nama",
    "bmd_pengelola_nip",
    "bmd_pengelola_pangkat",
    "bmd_pengelola_jabatan",
];

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

fn wajib(value: Option<String>, field: &str) -> Result<String, ApiError> {
    non_empty(value.as_deref()).ok_or_else(|| invalid(format!("{field} wajib diisi.")))
}

fn tanggal_wajib(value: Option<String>, field: &str) -> Result<NaiveDate, ApiError> {
    let raw = wajib(value, field)?;
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
        .map_err(|_| invalid(format!("{field} harus berformat YYYY-MM-DD.")))
}

/// Membaca dan memvalidasi body `export-bmd`. Semua error memakai status 422.
pub fn parse_form(body: &[u8]) -> Result<BmdForm, ApiError> {
    let raw: RawInput = serde_json::from_slice(body)
        .map_err(|_| invalid("Body permintaan harus berupa JSON yang valid."))?;

    let nomor_surat = wajib(raw.nomor_surat, "nomor_surat")?;
    if nomor_surat.chars().count() > NOMOR_SURAT_MAKS {
        return Err(invalid("nomor_surat maksimal 200 karakter."));
    }
    let tanggal_surat = tanggal_wajib(raw.tanggal_surat, "tanggal_surat")?;
    let tanggal_perolehan = tanggal_wajib(raw.tanggal_perolehan, "tanggal_perolehan")?;

    Ok(BmdForm {
        nomor_surat,
        tanggal_surat,
        tanggal_perolehan,
    })
}

/// Memastikan pejabat sudah diisi di pengaturan. Bila ada kunci wajib yang kosong, error 422 yang
/// menyebut kunci pengaturan tersebut. `bmd_pengelola_jabatan` kosong memakai `Pengelola Barang`.
pub fn pejabat(raw: RawPejabat) -> Result<BmdPejabat, ApiError> {
    let values = [
        non_empty(raw.kasubag_nama.as_deref()),
        non_empty(raw.kasubag_nip.as_deref()),
        non_empty(raw.pengelola_nama.as_deref()),
        non_empty(raw.pengelola_nip.as_deref()),
        non_empty(raw.pengelola_pangkat.as_deref()),
    ];
    let kosong: Vec<&str> = values
        .iter()
        .zip(PEJABAT_KEYS.iter())
        .filter(|(v, _)| v.is_none())
        .map(|(_, k)| *k)
        .collect();
    if !kosong.is_empty() {
        return Err(invalid(format!(
            "Pengaturan pejabat surat pernyataan BMD belum lengkap. Isi di Pengaturan Aplikasi: {}.",
            kosong.join(", ")
        )));
    }
    let [kasubag_nama, kasubag_nip, pengelola_nama, pengelola_nip, pengelola_pangkat] =
        values.map(|v| v.unwrap_or_default());
    Ok(BmdPejabat {
        kasubag_nama,
        kasubag_nip,
        pengelola_nama,
        pengelola_nip,
        pengelola_pangkat,
        pengelola_jabatan: non_empty(raw.pengelola_jabatan.as_deref())
            .unwrap_or_else(|| JABATAN_DEFAULT.to_string()),
    })
}

/// Tanggal dalam kalimat terbilang: `Hari Selasa Tanggal Satu Bulan September Tahun Dua Ribu Dua Puluh Enam`.
pub fn tanggal_terbilang(date: NaiveDate) -> String {
    let hari = HARI[date.weekday().num_days_from_monday() as usize];
    format!(
        "Hari {hari} Tanggal {} Bulan {} Tahun {}",
        terbilang(i64::from(date.day())),
        MONTHS[date.month0() as usize],
        terbilang(i64::from(date.year())),
    )
}

/// Rentang masa kontrak: `05 Juni s/d 4 Agustus 2026`. Tahun awal ditulis hanya bila berbeda dengan tahun akhir.
pub fn tanggal_kontrak(mulai: Option<NaiveDate>, selesai: Option<NaiveDate>) -> String {
    match (mulai, selesai) {
        (None, None) => String::new(),
        (Some(m), Some(s)) => {
            let awal = if m.year() == s.year() {
                format!("{:02} {}", m.day(), MONTHS[m.month0() as usize])
            } else {
                tanggal(m)
            };
            format!(
                "{awal} s/d {} {} {}",
                s.day(),
                MONTHS[s.month0() as usize],
                s.year()
            )
        }
        (Some(m), None) => format!("{} s/d", tanggal(m)),
        (None, Some(s)) => format!("s/d {}", tanggal(s)),
    }
}

/// Nilai kontrak: `Rp.  299.343.300` (dua spasi setelah titik, sesuai dokumen acuan).
pub fn nilai_kontrak(nilai: Option<f64>) -> String {
    nilai
        .map(|v| format!("Rp.  {}", number_format(v)))
        .unwrap_or_default()
}

/// Nomor dan tanggal addendum terbaru yang disetujui. Kosong bila tidak ada (ditulis `-` oleh `fill`).
pub fn nomor_kontrak_add(addendum: Option<&AddendumRow>) -> String {
    let Some(a) = addendum else {
        return String::new();
    };
    let Some(nomor) = non_empty(a.nomor_addendum.as_deref()) else {
        return String::new();
    };
    match a.tanggal_addendum {
        Some(t) => format!("{nomor} Tanggal {}", tanggal(t)),
        None => nomor,
    }
}

/// Nama penyedia lalu `/` lalu nama direktur bila ada.
pub fn nama_penyedia(penyedia: Option<&PenyediaDoc>) -> String {
    let nama = penyedia.and_then(|p| non_empty(p.nama.as_deref()));
    let direktur = penyedia.and_then(|p| non_empty(p.direktur.as_deref()));
    match (nama, direktur) {
        (Some(n), Some(d)) => format!("{n}/ {d}"),
        (Some(n), None) => n,
        (None, Some(d)) => d,
        (None, None) => String::new(),
    }
}

/// Nilai placeholder (key, value) untuk template BMD. Nama barang berisi satu baris per pekerjaan kontrak.
pub fn build(ctx: &DocCtx, form: &BmdForm, pej: &BmdPejabat) -> Vec<(String, String)> {
    let kontrak = &ctx.kontrak;
    let pekerjaan = ctx.pekerjaan.as_ref();
    let kegiatan = ctx.kegiatan.as_ref();

    let mut nama_barang: Vec<String> = ctx
        .pekerjaan_kontrak
        .iter()
        .filter_map(|p| non_empty(p.nama_paket.as_deref()))
        .collect();
    if nama_barang.is_empty() {
        nama_barang.extend(pekerjaan.and_then(|p| non_empty(p.nama_paket.as_deref())));
    }

    let values = [
        ("nomor_surat", form.nomor_surat.clone()),
        ("tanggal_surat_terbilang", tanggal_terbilang(form.tanggal_surat)),
        ("tanggal_surat", tanggal(form.tanggal_surat)),
        ("pengelola_nama", pej.pengelola_nama.clone()),
        ("pengelola_nip", pej.pengelola_nip.clone()),
        ("pengelola_pangkat", pej.pengelola_pangkat.clone()),
        ("pengelola_jabatan", pej.pengelola_jabatan.clone()),
        ("kasubag_nama", pej.kasubag_nama.clone()),
        ("kasubag_nip", pej.kasubag_nip.clone()),
        (
            "nama_kegiatan",
            kegiatan
                .and_then(|k| non_empty(k.nama_kegiatan.as_deref()))
                .unwrap_or_default(),
        ),
        (
            "nama_sub_kegiatan",
            kegiatan
                .and_then(|k| non_empty(k.nama_sub_kegiatan.as_deref()))
                .unwrap_or_default(),
        ),
        (
            "kode_belanja",
            pekerjaan
                .and_then(|p| non_empty(p.kode_rekening.as_deref()))
                .unwrap_or_default(),
        ),
        ("nama_barang", nama_barang.join("\n")),
        ("nama_penyedia", nama_penyedia(ctx.penyedia.as_ref())),
        (
            "nomor_kontrak",
            non_empty(kontrak.spk.as_deref()).unwrap_or_default(),
        ),
        (
            "tgl_nomor_kontrak",
            kontrak.tgl_spk.map(tanggal).unwrap_or_default(),
        ),
        ("nomor_kontrak_add", nomor_kontrak_add(ctx.latest_approved())),
        (
            "tanggal_kontrak",
            tanggal_kontrak(kontrak.tgl_spk, kontrak.tgl_selesai),
        ),
        (
            "tanggal_perolehan",
            form.tanggal_perolehan.format("%d/%m/%Y").to_string(),
        ),
        ("nilai", nilai_kontrak(kontrak.nilai_kontrak)),
    ];
    values
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use chrono::NaiveDate;
    use regex::Regex;
    use zip::ZipArchive;

    use super::*;
    use crate::{
        docx_template,
        kontrak::AddendumRow,
        kontrak_document::template_dir,
        kontrak_document_data::{tests::sample_ctx, PekerjaanKontrakDoc},
    };

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn body() -> String {
        r#"{"nomor_surat":"900/ /KEU-SP/DPKP/2026","tanggal_surat":"2026-09-01","tanggal_perolehan":"2026-09-01"}"#.to_string()
    }

    fn form() -> BmdForm {
        parse_form(body().as_bytes()).unwrap()
    }

    #[test]
    fn rentang_kontrak_satu_tahun_tanpa_tahun_awal() {
        assert_eq!(
            tanggal_kontrak(Some(d(2026, 6, 5)), Some(d(2026, 8, 4))),
            "05 Juni s/d 4 Agustus 2026"
        );
    }

    #[test]
    fn rentang_kontrak_beda_tahun_menulis_tahun_awal() {
        assert_eq!(
            tanggal_kontrak(Some(d(2025, 12, 1)), Some(d(2026, 1, 31))),
            "01 Desember 2025 s/d 31 Januari 2026"
        );
        assert_eq!(tanggal_kontrak(None, None), "");
    }

    #[test]
    fn nilai_kontrak_format_ribuan_dengan_dua_spasi() {
        assert_eq!(nilai_kontrak(Some(299_343_300.0)), "Rp.  299.343.300");
        assert_eq!(nilai_kontrak(None), "");
    }

    #[test]
    fn tanggal_surat_terbilang_sesuai_dokumen_acuan() {
        assert_eq!(
            tanggal_terbilang(d(2026, 9, 1)),
            "Hari Selasa Tanggal Satu Bulan September Tahun Dua Ribu Dua Puluh Enam"
        );
    }

    #[test]
    fn addendum_dan_penyedia_diformat() {
        assert_eq!(nomor_kontrak_add(None), "");
        let add = AddendumRow {
            id: 1,
            kontrak_id: 1,
            addendum_ke: 1,
            nomor_addendum: Some("602.4/SPK-ADD/PPK/DISPERKIM-AMS.330.799/2026".into()),
            attachment_nomors: None,
            tanggal_addendum: Some(d(2026, 7, 2)),
            jenis_addendum: None,
            alasan: None,
            deskripsi_perubahan: None,
            nilai_kontrak_sebelum: None,
            nilai_kontrak_sesudah: None,
            tgl_selesai_sebelum: None,
            tgl_selesai_sesudah: None,
            status: "disetujui".into(),
            kelengkapan_override: false,
            created_by: None,
            approved_by: None,
            approved_at: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(
            nomor_kontrak_add(Some(&add)),
            "602.4/SPK-ADD/PPK/DISPERKIM-AMS.330.799/2026 Tanggal 02 Juli 2026"
        );

        let penyedia = PenyediaDoc {
            nama: Some("CV. YUSUF RAHMAT HIDAYAT".into()),
            direktur: Some("YUSUF RAHMAT HIDAYAT".into()),
            alamat: None,
            bank: None,
            norek: None,
            npwp: None,
            no_akta: None,
            notaris: None,
            tanggal_akta: None,
        };
        assert_eq!(
            nama_penyedia(Some(&penyedia)),
            "CV. YUSUF RAHMAT HIDAYAT/ YUSUF RAHMAT HIDAYAT"
        );
        assert_eq!(nama_penyedia(None), "");
    }

    fn pejabat_lengkap() -> BmdPejabat {
        pejabat(RawPejabat {
            kasubag_nama: Some(" DIANA DAMAJANTI, SH ".into()),
            kasubag_nip: Some("19710422 199901 2001".into()),
            pengelola_nama: Some("Dadang Karya".into()),
            pengelola_nip: Some("197003022008011006".into()),
            pengelola_pangkat: Some("Penata Muda TK. I / III a".into()),
            pengelola_jabatan: None,
        })
        .unwrap()
    }

    #[test]
    fn body_valid() {
        let f = form();
        assert_eq!(f.nomor_surat, "900/ /KEU-SP/DPKP/2026");
        assert_eq!(f.tanggal_surat, d(2026, 9, 1));
        assert_eq!(f.tanggal_perolehan, d(2026, 9, 1));
    }

    #[test]
    fn body_tidak_valid_ditolak_dengan_422() {
        let status = |raw: &str| parse_form(raw.as_bytes()).unwrap_err().status;
        assert_eq!(status(""), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status("bukan json"), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status("[]"), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            status(&body().replace("\"tanggal_surat\":\"2026-09-01\"", "\"tanggal_surat\":\"01/09/2026\"")),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            status(&body().replace("\"tanggal_perolehan\":\"2026-09-01\"", "\"tanggal_perolehan\":\"2026-13-01\"")),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let nomor_panjang = "x".repeat(201);
        assert_eq!(
            status(&body().replace("900/ /KEU-SP/DPKP/2026", &nomor_panjang)),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        // Pejabat tidak lagi diterima dari body; body tanpa field itu tetap valid.
        assert!(parse_form(body().as_bytes()).is_ok());
    }

    #[test]
    fn pesan_error_menyebut_field() {
        let err = parse_form(body().replace("\"nomor_surat\":\"900/ /KEU-SP/DPKP/2026\"", "\"nomor_surat\":\"  \"").as_bytes())
            .unwrap_err();
        assert_eq!(err.message, "nomor_surat wajib diisi.");
    }

    #[test]
    fn pejabat_dari_pengaturan() {
        let p = pejabat_lengkap();
        assert_eq!(p.kasubag_nama, "DIANA DAMAJANTI, SH");
        assert_eq!(p.pengelola_jabatan, "Pengelola Barang");
    }

    #[test]
    fn pejabat_kosong_menyebut_kunci_pengaturan() {
        let err = pejabat(RawPejabat {
            kasubag_nama: Some("A".into()),
            kasubag_nip: None,
            pengelola_nama: Some("B".into()),
            pengelola_nip: Some("  ".into()),
            pengelola_pangkat: Some("C".into()),
            pengelola_jabatan: None,
        })
        .unwrap_err();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(err.message.contains("bmd_kasubag_nip"), "{}", err.message);
        assert!(err.message.contains("bmd_pengelola_nip"), "{}", err.message);
        assert!(!err.message.contains("bmd_kasubag_nama"), "{}", err.message);
    }

    fn sample_with_pekerjaan() -> DocCtx {
        let mut ctx = sample_ctx();
        ctx.pekerjaan_kontrak = vec![
            PekerjaanKontrakDoc {
                nama_paket: Some("Pembangunan Jalan".into()),
                kode_rekening: Some("1.03.05.2.01.0044.5.1.02.01.001.00039".into()),
            },
            PekerjaanKontrakDoc {
                nama_paket: Some("Pembangunan Drainase".into()),
                kode_rekening: None,
            },
        ];
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
    fn build_mengisi_semua_key_dengan_nama_barang_per_baris() {
        let values = build(&sample_with_pekerjaan(), &form(), &pejabat_lengkap());
        assert_eq!(value(&values, "nama_barang"), "Pembangunan Jalan\nPembangunan Drainase");
        assert_eq!(value(&values, "kode_belanja"), "1.03.05.2.01.0044.5.1.02.01.001.00039");
        assert_eq!(value(&values, "nomor_kontrak"), "SPK/1");
        assert_eq!(value(&values, "tgl_nomor_kontrak"), "10 Januari 2026");
        assert_eq!(value(&values, "tanggal_kontrak"), "10 Januari s/d 11 April 2026");
        assert_eq!(value(&values, "tanggal_perolehan"), "01/09/2026");
        assert_eq!(value(&values, "nilai"), "Rp.  1.000.000");
        assert_eq!(value(&values, "nama_penyedia"), "CV Maju/ Budi");
        assert_eq!(value(&values, "nomor_kontrak_add"), "");
        assert_eq!(value(&values, "pengelola_jabatan"), "Pengelola Barang");
    }

    #[test]
    fn template_default_terisi_penuh() {
        let template = std::fs::read(template_dir().join("surat_pernyataan_bmd_template.docx"))
            .expect("template BMD ada");
        let values = build(&sample_with_pekerjaan(), &form(), &pejabat_lengkap());

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
            assert!(values.iter().any(|(k, _)| k == key), "placeholder tak dikenal: {key}");
        }
        for (key, _) in &values {
            assert!(raw.contains(&format!("{{{key}}}")), "key tidak ada di template: {key}");
        }

        let filled = docx_template::fill(&template, &values).unwrap();
        let mut archive = ZipArchive::new(std::io::Cursor::new(&filled)).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(!xml.contains('{') && !xml.contains('}'), "placeholder tersisa");
        assert!(xml.contains("Pembangunan Jalan</w:t><w:br/><w:t xml:space=\"preserve\">Pembangunan Drainase"));
        assert!(xml.contains("Hari Selasa Tanggal Satu Bulan September Tahun Dua Ribu Dua Puluh Enam"));
        assert!(xml.contains("Rp.  1.000.000"));
    }
}
