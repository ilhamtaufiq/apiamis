//! Naskah Perjanjian Hibah Daerah (NPHD) dengan Berita Acara Serah Terima Fisik & Pemeliharaan dalam satu
//! berkas `.docx`. Template ada di `storage/app/templates/nphd_template.docx` dan memakai placeholder
//! `{{NAMA}}`. Nama bidang, program, kegiatan, sub kegiatan, desa, kecamatan, penyedia, dan nomor SPK
//! diambil dari data pekerjaan dan kontrak. Nomor NPHD diberikan pemanggil dari Register Dokumen.

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
    /// Uraian objek hibah. Beberapa item dipisah dengan `;`.
    pub objek: String,
    /// Nilai aset bangunan dalam rupiah.
    pub nilai: f64,
    /// Sebutan pengelola, misalnya `Pengelola SPAM`. Kosong berarti `Pengelola`.
    #[serde(default)]
    pub pengelola_label: String,
    /// Nama penerima manfaat, dipisah `;` atau baris baru. Ditulis sebagai daftar `- nama`.
    #[serde(default)]
    pub penerima: String,
}

/// Daftar penerima manfaat sebagai baris `- nama`, dipisah baris baru di dokumen.
pub(crate) fn format_penerima(penerima: &str) -> String {
    daftar_penerima(penerima)
        .iter()
        .map(|nama| format!("- {nama}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Nama penerima manfaat, dipisah `;` atau baris baru, tanpa baris kosong.
pub(crate) fn daftar_penerima(penerima: &str) -> Vec<String> {
    penerima
        .split(|c| c == ';' || c == '\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Minimal baris pada tabel tanda tangan penerima, seperti contoh berita acara.
const MIN_BARIS_TTD: usize = 8;
const PLACEHOLDER_TABEL: &str = "{{TABEL PENERIMA}}";

fn xml_teks(teks: &str) -> String {
    teks.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Tabel tanda tangan penerima: No, Nama, Tanda Tangan. Jumlah baris mengikuti jumlah penerima,
/// dengan minimal `MIN_BARIS_TTD` baris.
pub(crate) fn tabel_penerima(nama: &[String]) -> String {
    let baris = nama.len().max(MIN_BARIS_TTD);
    let garis = |sisi: &str| format!(r#"<w:{sisi} w:val="single" w:sz="4" w:space="0" w:color="000000"/>"#);
    let rpr = |tebal: bool| {
        format!(
            r#"<w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman"/>{}<w:sz w:val="22"/></w:rPr>"#,
            if tebal { "<w:b/>" } else { "" }
        )
    };
    let sel = |lebar: u32, jc: &str, teks: &str, tebal: bool| {
        let isi = if teks.is_empty() {
            String::new()
        } else {
            format!(r#"<w:r>{}<w:t xml:space="preserve">{}</w:t></w:r>"#, rpr(tebal), xml_teks(teks))
        };
        format!(
            r#"<w:tc><w:tcPr><w:tcW w:w="{lebar}" w:type="dxa"/><w:vAlign w:val="center"/></w:tcPr><w:p><w:pPr><w:keepNext/><w:spacing w:before="40" w:after="40"/><w:jc w:val="{jc}"/></w:pPr>{isi}</w:p></w:tc>"#
        )
    };
    let mut xml = String::from(
        r#"<w:tbl><w:tblPr><w:tblW w:w="8700" w:type="dxa"/><w:tblLayout w:type="fixed"/><w:tblBorders>"#,
    );
    for sisi in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        xml.push_str(&garis(sisi));
    }
    xml.push_str(r#"</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="700"/><w:gridCol w:w="4700"/><w:gridCol w:w="3300"/></w:tblGrid>"#);
    xml.push_str(r#"<w:tr><w:trPr><w:tblHeader/></w:trPr>"#);
    xml.push_str(&sel(700, "center", "No", true));
    xml.push_str(&sel(4700, "center", "Nama", true));
    xml.push_str(&sel(3300, "center", "Tanda Tangan", true));
    xml.push_str("</w:tr>");
    for i in 0..baris {
        let nomor = (i + 1).to_string();
        let isi_nama = nama.get(i).map(String::as_str).unwrap_or("");
        xml.push_str(r#"<w:tr><w:trPr><w:cantSplit/><w:trHeight w:val="560"/></w:trPr>"#);
        xml.push_str(&sel(700, "center", &nomor, false));
        xml.push_str(&sel(4700, "left", isi_nama, false));
        xml.push_str(&sel(3300, "left", "", false));
        xml.push_str("</w:tr>");
    }
    xml.push_str("</w:tbl>");
    xml
}

/// Ganti paragraf yang memuat `{{TABEL PENERIMA}}` dengan tabel. Template tanpa placeholder dibiarkan.
pub(crate) fn sisipkan_tabel(template: &[u8], tabel: &str) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let mut arsip = zip::ZipArchive::new(std::io::Cursor::new(template)).map_err(|e| e.to_string())?;
    let mut keluaran = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..arsip.len() {
        let mut berkas = arsip.by_index(i).map_err(|e| e.to_string())?;
        let nama = berkas.name().to_string();
        let mut isi = Vec::new();
        berkas.read_to_end(&mut isi).map_err(|e| e.to_string())?;
        drop(berkas);
        if nama == "word/document.xml" {
            let xml = String::from_utf8(isi).map_err(|e| e.to_string())?;
            if let Some(posisi) = xml.find(PLACEHOLDER_TABEL) {
                let mulai = xml[..posisi].rfind("<w:p>").or_else(|| xml[..posisi].rfind("<w:p ")).ok_or("paragraf placeholder tidak ditemukan")?;
                let selesai = xml[posisi..].find("</w:p>").map(|k| posisi + k + "</w:p>".len()).ok_or("paragraf placeholder tidak ditutup")?;
                let hasil = format!("{}{}{}", &xml[..mulai], tabel, &xml[selesai..]);
                isi = hasil.into_bytes();
            } else {
                isi = xml.into_bytes();
            }
        }
        let opsi = zip::write::SimpleFileOptions::default();
        keluaran.start_file(nama, opsi).map_err(|e| e.to_string())?;
        keluaran.write_all(&isi).map_err(|e| e.to_string())?;
    }
    Ok(keluaran.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Data yang berasal dari pekerjaan dan kontrak.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DataDasar {
    pub desa: String,
    pub kecamatan: String,
    pub penyedia: String,
    pub nomor_spk: String,
    pub bidang: String,
    pub program: String,
    pub kegiatan: String,
    pub sub_kegiatan: String,
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

    let data = data_dasar(&ctx);
    let values = nilai_dari(&data, &q, tanggal);
    let path = kontrak_document::resolve_template(&state.pool, kontrak_document::NPHD_KEY).await?;
    let bytes = tokio::fs::read(path).await.map_err(internal)?;
    let bytes = sisipkan_tabel(&bytes, &tabel_penerima(&daftar_penerima(&q.penerima))).map_err(internal)?;
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

/// Objek hibah sebagai daftar bernomor huruf: `a. ...`, `b. ...`, setiap item di baris sendiri.
/// Item dipisah `;` atau baris baru.
pub(crate) fn format_objek(objek: &str) -> String {
    objek
        .split(|c| c == ';' || c == '\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, item)| {
            let label = if i < 26 { ((b'a' + i as u8) as char).to_string() } else { (i + 1).to_string() };
            format!("{label}. {item}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mengambil data dasar dari konteks kontrak dan pekerjaan.
fn data_dasar(ctx: &DocCtx) -> DataDasar {
    let kegiatan = ctx.kegiatan.as_ref();
    DataDasar {
        desa: ctx.pekerjaan.as_ref().and_then(|p| p.nama_desa.clone()).unwrap_or_default(),
        kecamatan: ctx.pekerjaan.as_ref().and_then(|p| p.nama_kecamatan.clone()).unwrap_or_default(),
        penyedia: ctx.penyedia.as_ref().and_then(|p| p.nama.clone()).unwrap_or_default(),
        nomor_spk: ctx.kontrak.spk.clone().unwrap_or_default(),
        bidang: ctx.sub_bidang(),
        program: kegiatan.and_then(|k| k.nama_program.clone()).unwrap_or_default(),
        kegiatan: kegiatan.and_then(|k| k.nama_kegiatan.clone()).unwrap_or_default(),
        sub_kegiatan: kegiatan.and_then(|k| k.nama_sub_kegiatan.clone()).unwrap_or_default(),
    }
}

/// Pasangan placeholder `{{NAMA}}` dan nilainya. Dipisah dari akses database supaya bisa diuji.
/// Nilai kosong ditampilkan `-` oleh `docx_template::fill`.
pub(crate) fn nilai_dari(data: &DataDasar, q: &NphdQuery, tanggal: NaiveDate) -> Vec<(String, String)> {
    let desa = judul(&data.desa);
    let kecamatan = judul(&data.kecamatan);
    let label = match q.pengelola_label.trim() {
        "" => "Pengelola",
        l => l,
    };
    let pairs = [
        ("HARI TANGGAL", hari_tanggal(tanggal)),
        ("TANGGAL", tanggal.format("%d-%m-%Y").to_string()),
        ("NOMOR NPHD", q.nomor.trim().to_string()),
        ("NOMOR BAST FISIK DAN PEMELIHARAAN", String::new()),
        ("NAMA BIDANG", data.bidang.trim().to_string()),
        ("NAMA PROGRAM", data.program.trim().to_string()),
        ("NAMA KEGIATAN", data.kegiatan.trim().to_string()),
        ("NAMA SUB KEGIATAN", data.sub_kegiatan.trim().to_string()),
        ("NAMA DESA - NAMA KECAMATAN", format!("Desa {desa} Kecamatan {kecamatan}")),
        ("NAMA DESA", desa.clone()),
        ("NAMA KECAMATAN", kecamatan.clone()),
        ("NAMAPENGELOLA", format!("{label} Desa {desa} Kecamatan {kecamatan}")),
        ("NAMA PENERIMA MANFAAT", format_penerima(&q.penerima)),
        ("NAMA PENYEDIA", data.penyedia.trim().to_string()),
        ("NOMOR SPK", data.nomor_spk.trim().to_string()),
        ("Nilaikontrak", format!("Rp.{},00", number_format(q.nilai))),
        ("OUTPUT PEKERJAAN", format_objek(&q.objek)),
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

    fn query() -> NphdQuery {
        NphdQuery {
            nomor: "001/NPHD/2026".into(),
            tanggal: "2026-07-13".into(),
            objek: "1 Paket Pembangunan (MCK 2 unit/pintu dan Tangki Septik 2 unit)".into(),
            nilai: 49_501_500.0,
            pengelola_label: "Penerima Manfaat MCK Individu".into(),
            penerima: "Indra Permana; Ani Sutisna".into(),
        }
    }

    fn data() -> DataDasar {
        DataDasar {
            desa: "CILOTO".into(),
            kecamatan: "CIPANAS".into(),
            penyedia: "CV. CAHAYA MANDIRI".into(),
            nomor_spk: "602.4/SPK/PPK/DISPERKIM-AMS.088.350/2026".into(),
            bidang: "Air Minum".into(),
            program: "PROGRAM PENGELOLAAN DAN PENGEMBANGAN SISTEM AIR LIMBAH".into(),
            kegiatan: "Pengelolaan dan Pengembangan Sistem Air Limbah Domestik".into(),
            sub_kegiatan: "Sub Sistem Penyediaan Unit Pengolahan Setempat".into(),
        }
    }

    #[test]
    fn hari_tanggal_sesuai_naskah_contoh() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        assert_eq!(hari_tanggal(d), "Senin tanggal Tiga Belas bulan Juli tahun Dua Ribu Dua Puluh Enam");
    }

    #[test]
    fn objek_diberi_nomor_huruf_per_baris() {
        assert_eq!(
            format_objek("10 Unit Sambungan Rumah; 2520 Meter Pipa; 1 Unit Broncaptering"),
            "a. 10 Unit Sambungan Rumah\nb. 2520 Meter Pipa\nc. 1 Unit Broncaptering"
        );
        assert_eq!(format_objek(" satu \n\n dua ; "), "a. satu\nb. dua");
        assert_eq!(format_objek("  "), "");
        let banyak: Vec<String> = (0..27).map(|i| format!("item {i}")).collect();
        let hasil = format_objek(&banyak.join(";"));
        assert!(hasil.starts_with("a. item 0\n"));
        assert!(hasil.ends_with("\n27. item 26"));
    }

    #[test]
    fn tabel_ttd_penerima_minimal_delapan_baris() {
        let tiga: Vec<String> = ["Indra", "Ani", "Budi"].iter().map(|s| s.to_string()).collect();
        let tabel = tabel_penerima(&tiga);
        assert_eq!(tabel.matches("<w:tr>").count(), 9);
        let sepuluh: Vec<String> = (0..10).map(|i| format!("Nama {i}")).collect();
        let tabel = tabel_penerima(&sepuluh);
        assert_eq!(tabel.matches("<w:tr>").count(), 11);
        let khusus = tabel_penerima(&["Ani & Budi".to_string()]);
        assert!(khusus.contains("Ani &amp; Budi"));
        assert!(khusus.contains(">Tanda Tangan<"));
    }

    #[test]
    fn tabel_penerima_disisipkan_ke_template_dan_terisi() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../storage/app/templates/nphd_template.docx");
        let template = std::fs::read(path).expect("template NPHD");
        let nama = vec!["Indra Permana".to_string(), "Ani Sutisna".to_string()];
        let disisipkan = sisipkan_tabel(&template, &tabel_penerima(&nama)).expect("sisipkan tabel");
        let mut arsip = zip::ZipArchive::new(std::io::Cursor::new(&disisipkan)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut arsip.by_name("word/document.xml").unwrap(), &mut xml).unwrap();
        assert!(!xml.contains("{{TABEL PENERIMA}}"));
        assert!(xml.contains("Tanda Tangan"));

        let isi = docx_template::fill(&disisipkan, &[("NAMA PENERIMA MANFAAT".into(), format_penerima("Indra Permana;Ani Sutisna"))]).unwrap();
        let mut arsip = zip::ZipArchive::new(std::io::Cursor::new(isi)).unwrap();
        let mut hasil = String::new();
        std::io::Read::read_to_string(&mut arsip.by_name("word/document.xml").unwrap(), &mut hasil).unwrap();
        assert!(!hasil.contains("{{TABEL"));
        assert!(hasil.contains("Ani Sutisna"));
    }

    #[test]
    fn judul_mengecilkan_huruf_selain_awal_kata() {
        assert_eq!(judul("CILOTO"), "Ciloto");
        assert_eq!(judul("kp. pasirkuntul  rt"), "Kp. Pasirkuntul Rt");
    }

    #[test]
    fn validasi_menolak_tanggal_dan_nilai_tidak_valid() {
        let mut q = query();
        assert!(validasi(&q).is_ok());
        q.tanggal = "13-07-2026".into();
        assert!(validasi(&q).is_err());
        q.tanggal = "2026-07-13".into();
        q.nilai = 0.0;
        assert!(validasi(&q).is_err());
    }

    #[test]
    fn template_terisi_semua_placeholder() {
        let q = query();
        let tanggal = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
        let values = nilai_dari(&data(), &q, tanggal);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../storage/app/templates/nphd_template.docx");
        let bytes = std::fs::read(path).expect("template NPHD");
        let bytes = sisipkan_tabel(&bytes, &tabel_penerima(&daftar_penerima(&q.penerima))).expect("sisipkan tabel");
        let filled = docx_template::fill(&bytes, &values).expect("isi template");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(filled)).expect("docx valid");
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").expect("document.xml"), &mut xml).unwrap();
        // Tidak boleh ada sisa placeholder `{{NAMA}}`.
        assert!(!xml.contains("{{"), "placeholder belum terisi");
        for nilai in [
            "Desa Ciloto Kecamatan Cipanas",
            "Indra Permana",
            "CV. CAHAYA MANDIRI",
            "602.4/SPK/PPK/DISPERKIM-AMS.088.350/2026",
            "Rp.49.501.500,00",
            "Senin tanggal Tiga Belas bulan Juli tahun Dua Ribu Dua Puluh Enam",
            "001/NPHD/2026",
            "Pengelolaan dan Pengembangan Sistem Air Limbah Domestik",
        ] {
            assert!(xml.contains(nilai), "tidak ada di dokumen: {nilai}");
        }
    }
}
