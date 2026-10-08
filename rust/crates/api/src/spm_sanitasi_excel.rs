//! Excel SPM sanitasi: `GET /api/spm-sanitasi/export`, `GET /api/spm-sanitasi/import/template`, dan
//! `POST /api/spm-sanitasi/import`. Mengikuti `SpmSanitasiExport`, `SpmSanitasiImportService`, dan
//! `SpmSanitasiController::export|downloadTemplate|import`.
//!
//! Format workbook: tiga lembar `SPALDT`, `SPALDS`, `IPLT`. Judul di A1, header di baris 4, data mulai
//! baris 5. Impor membaca lembar yang sama, melewati baris 1 sampai 4, dan melewati baris yang kolom
//! pertamanya kosong, diawali `(`, diawali `no`, atau tidak numerik.
//!
//! Perbedaan kecil:
//! - Sel angka diambil sebagai nilai mentah. Laravel memakai teks terformat (`formatData`), sehingga
//!   angka dengan pemisah ribuan di file pengguna bisa berbeda hasilnya. Hasil ekspor kita tidak terpengaruh.
//! - Setiap baris diimpor dalam transaksinya sendiri, dan audit `created` ditulis di transaksi itu. Ini
//!   sama dengan `create()` per baris di Laravel, dan baris gagal dicatat di `errors` lalu dilewati.
//! - Pesan error database tidak identik dengan pesan `SQLSTATE` Laravel.
//! - Berkas CSV dimuat tanpa lembar bernama, seperti PhpSpreadsheet, sehingga hasilnya 0 baris.
//! - `replace=true` menghapus seluruh `tbl_spm_sanitasi` (sama dengan Laravel). Jalur ini tidak diuji
//!   karena tes menghapus tabel bersama.

use std::{collections::{BTreeMap, HashMap}, io::Cursor};

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use rust_xlsxwriter::Workbook;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Transaction};

use crate::{
    changes::{self, Target},
    kontrak_xlsx::xlsx_response,
    require_auth,
    spm_sanitasi::{self, internal, Arg, COLUMNS},
    AppState,
};

const SPM_TARGET: Target = Target {
    model_type: "App\\Models\\SpmSanitasi",
    label: "SpmSanitasi",
    tab: "",
};

/// Lembar workbook: `(nama lembar, jenis, judul di A1)`.
const SHEETS: &[(&str, &str, &str)] = &[
    ("SPALDT", "spaldt", "FORMAT DATA SPALDT"),
    ("SPALDS", "spalds", "FORMAT DATA SPALDS"),
    ("IPLT", "iplt", "FORMAT DATA IPLT"),
];

const MAX_UPLOAD_KB: u64 = 20480;
const ALLOWED_EXT: [&str; 3] = ["xlsx", "xls", "csv"];

// ---------------------------------------------------------------------------
// Ekspor
// ---------------------------------------------------------------------------

/// Filter ekspor: `kecamatan_id`, `desa_id`, `search`, `tahun` (nilai kosong dianggap tidak ada).
#[derive(Default)]
struct ExportFilter {
    kecamatan: Option<i64>,
    desa: Option<i64>,
    search: Option<String>,
    tahun: Option<String>,
}

/// Baris SPM untuk satu jenis, urut id, dengan relasi `desa.kecamatan` (`with('desa.kecamatan')`).
async fn export_rows(pool: &MySqlPool, jenis: &str, f: &ExportFilter) -> Result<Vec<Map<String, Value>>, ApiError> {
    let mut sql = format!(
        "SELECT {} FROM tbl_spm_sanitasi s WHERE s.jenis = ?",
        spm_sanitasi::select_list(COLUMNS)
    );
    let mut args = vec![Arg::S(jenis.to_string())];
    if let Some(k) = f.kecamatan {
        sql.push_str(" AND EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = s.desa_id AND d.kecamatan_id = ?)");
        args.push(Arg::I(k));
    }
    if let Some(d) = f.desa {
        sql.push_str(" AND s.desa_id = ?");
        args.push(Arg::I(d));
    }
    if let Some(s) = &f.search {
        sql.push_str(
            " AND (s.nama_infrastruktur LIKE ? OR s.alamat_lengkap LIKE ? \
             OR EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = s.desa_id AND d.n_desa LIKE ?))",
        );
        let like = format!("%{s}%");
        args.push(Arg::S(like.clone()));
        args.push(Arg::S(like.clone()));
        args.push(Arg::S(like));
    }
    if let Some(t) = &f.tahun {
        sql.push_str(" AND s.tahun_konstruksi = ?");
        args.push(Arg::I(spm_sanitasi::php_int(t)));
    }
    sql.push_str(" ORDER BY s.id");

    let rows = spm_sanitasi::bind(sqlx::query(&sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    let mut cache = HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let attrs = spm_sanitasi::read_map(r, COLUMNS).map_err(internal)?;
        let with = spm_sanitasi::with_desa(pool, attrs, &mut cache).await?;
        out.push(with);
    }
    Ok(out)
}

/// Sel yang ditulis ke workbook (`null` menjadi sel kosong).
enum Cell {
    Empty,
    Num(f64),
    Text(String),
}

fn cell_of(v: Option<&Value>) -> Cell {
    match v {
        None | Some(Value::Null) => Cell::Empty,
        Some(Value::Number(n)) => Cell::Num(n.as_f64().unwrap_or(0.0)),
        Some(Value::String(s)) => Cell::Text(s.clone()),
        Some(Value::Bool(b)) => Cell::Num(f64::from(u8::from(*b))),
        Some(other) => Cell::Text(other.to_string()),
    }
}

fn text_of(v: Option<&Value>) -> Cell {
    match v {
        None | Some(Value::Null) => Cell::Text(String::new()),
        Some(Value::String(s)) => Cell::Text(s.clone()),
        other => cell_of(other),
    }
}

/// Susun satu baris data seperti `SpmSanitasiSheetExport::mapItem`.
fn map_item(jenis: &str, item: &Map<String, Value>, no: i64) -> Vec<Cell> {
    let kec = item
        .get("desa")
        .and_then(|d| d.get("kecamatan"))
        .and_then(|k| k.get("n_kec"))
        .cloned();
    let desa = item.get("desa").and_then(|d| d.get("n_desa")).cloned();
    let g = |k: &str| item.get(k);
    let mut row = vec![Cell::Num(no as f64)];
    if jenis == "iplt" {
        row.extend([
            Cell::Text("Jawa Barat".into()),
            Cell::Text("Cianjur".into()),
            text_of(kec.as_ref()),
            text_of(desa.as_ref()),
            text_of(g("nama_infrastruktur")),
            cell_of(g("latitude")),
            cell_of(g("longitude")),
            cell_of(g("tahun_konstruksi")),
            cell_of(g("jenis_pengelola")),
            cell_of(g("kapasitas_desain")),
            cell_of(g("kapasitas_terpakai")),
            cell_of(g("kapasitas_tidak_terpakai")),
            cell_of(g("status_keberfungsian")),
            cell_of(g("kualitas_keberfungsian")),
            cell_of(g("sistem_pengolahan")),
            cell_of(g("truk_tinja_unit")),
            cell_of(g("kapasitas_truk_m3")),
            cell_of(g("jumlah_ritasi")),
            cell_of(g("jarak_maksimal_pelayanan_km")),
            cell_of(g("alokasi_biaya_operasional")),
            cell_of(g("jumlah_pemanfaat_kk")),
            cell_of(g("tahun_konstruksi")),
            cell_of(g("pembiayaan_apbn")),
            cell_of(g("pembiayaan_apbd")),
            cell_of(g("pembiayaan_dak")),
            cell_of(g("pembiayaan_hibah")),
            cell_of(g("pembiayaan_csr")),
            cell_of(g("pembiayaan_lain")),
            cell_of(g("pembiayaan_total")),
        ]);
        return row;
    }
    row.extend([
        text_of(g("skala_pelayanan")),
        Cell::Text("Jawa Barat".into()),
        Cell::Text("Cianjur".into()),
        text_of(kec.as_ref()),
        text_of(desa.as_ref()),
        text_of(g("nama_infrastruktur")),
        cell_of(g("latitude")),
        cell_of(g("longitude")),
        text_of(g("alamat_lengkap")),
        cell_of(g("jumlah_pemanfaat_kk")),
    ]);
    if jenis == "spaldt" {
        row.push(cell_of(g("jumlah_pemanfaat_jiwa")));
    }
    row.extend([
        cell_of(g("tahun_konstruksi")),
        cell_of(g("pembiayaan_apbn")),
        cell_of(g("pembiayaan_apbd")),
        cell_of(g("pembiayaan_dak")),
        cell_of(g("pembiayaan_hibah")),
        cell_of(g("pembiayaan_csr")),
        cell_of(g("pembiayaan_lain")),
        cell_of(g("pembiayaan_total")),
        text_of(g("status_keberfungsian")),
        text_of(g("kualitas_keberfungsian")),
        text_of(g("pengelola")),
        cell_of(g("kapasitas_desain")),
        cell_of(g("kapasitas_terpakai")),
        cell_of(g("kapasitas_tidak_terpakai")),
        text_of(g("jenis_pengolahan")),
        text_of(g("peta_cakupan")),
        text_of(g("status_lahan")),
        text_of(g("luas_lahan_ha")),
        text_of(g("opsi_teknologi")),
        text_of(g("jumlah_stasiun_pompa")),
        cell_of(g("biaya_operasional")),
    ]);
    row
}

/// Header kolom seperti `spaldHeaders` dan `ipltHeaders`.
fn headers(jenis: &str) -> Vec<&'static str> {
    if jenis == "iplt" {
        return vec![
            "No.",
            "Provinsi",
            "Kabupaten",
            "Kecamatan",
            "Desa/Kelurahan",
            "Nama IPLT",
            "Latitude",
            "Longitude",
            "Tahun Konstruksi",
            "Jenis Pengelola",
            "Kapasitas Terpasang\n(m3/hari)",
            "Kapasitas Terpakai\n(m3/hari)",
            "Kapasitas Tidak Terpakai\n(m3/hari)",
            "Status Keberfungsian",
            "Kualitas Keberfungsian",
            "Sistem Pengolahan",
            "Truk Tinja\n(unit)",
            "Kapasitas Truk\n(m3)",
            "Jumlah Ritasi\n(rit/hari)",
            "Jarak maksimal Pelayanan\n(km)",
            "Alokasi Biaya Operasional\n(Rp/tahun)",
            "Jumlah Pemanfaat\n(KK)",
            "Tahun Konstruksi",
            "Data Pembiayaan\n(APBN)",
            "Data Pembiayaan\n(APBD)",
            "Data Pembiayaan\n(DAK)",
            "Data Pembiayaan\n(Hibah)",
            "Data Pembiayaan\n(CSR)",
            "Data Pembiayaan\n(Lain-Lain)",
            "Data Pembiayaan\n(TOTAL)",
        ];
    }
    let mut h = vec![
        "No.",
        "Skala Pelayanan",
        "Provinsi",
        "Kabupaten",
        "Kecamatan",
        "Desa/Kelurahan",
        "Nama Infrastruktur",
        "Latitude",
        "Longitude",
        "Alamat Lengkap",
        "Jumlah Pemanfaat (KK)",
    ];
    if jenis == "spaldt" {
        h.push("Jumlah Pemanfaat (Jiwa)");
    }
    h.extend([
        "Tahun Konstruksi",
        "Data Pembiayaan\n(APBN)",
        "Data Pembiayaan\n(APBD)",
        "Data Pembiayaan\n(DAK)",
        "Data Pembiayaan\n(Hibah)",
        "Data Pembiayaan\n(CSR)",
        "Data Pembiayaan\n(Lain-Lain)",
        "Data Pembiayaan\n(TOTAL)",
        "Status Keberfungsian",
        "Kualitas Keberfungsian",
        "Pengelola",
        "Kapasitas Desain terpasang (m3/hari)",
        "Kapasitas Terpakai (m3/hari)",
        "Kapasitas Tidak Terpakai (m3/hari)",
        "Jenis Pengolahan",
        "Peta Cakupan Air Limbah",
        "Status Lahan",
        "Luas Lahan (ha)",
        "Opsi Teknologi",
        "Jumlah Stasiun Pompa (unit)",
        "Biaya Operasional (Rp)",
    ]);
    h
}

fn judul(jenis: &str) -> &'static str {
    SHEETS
        .iter()
        .find(|(_, j, _)| *j == jenis)
        .map(|(_, _, t)| *t)
        .unwrap_or("")
}

fn excel_err(e: impl std::fmt::Display) -> ApiError {
    internal(format!("Gagal membuat berkas Excel: {e}"))
}

/// Tulis tiga lembar untuk satu filter. `filter` kosong untuk template.
async fn build_workbook(pool: &MySqlPool, f: &ExportFilter) -> Result<Vec<u8>, ApiError> {
    let mut wb = Workbook::new();
    for (sheet_name, jenis, _) in SHEETS {
        let rows = export_rows(pool, jenis, f).await?;
        let ws = wb.add_worksheet();
        ws.set_name(*sheet_name).map_err(excel_err)?;
        ws.write_string(0, 0, judul(jenis)).map_err(excel_err)?;
        for (col, h) in headers(jenis).iter().enumerate() {
            ws.write_string(3, col as u16, *h).map_err(excel_err)?;
        }
        for (i, item) in rows.iter().enumerate() {
            let r = (4 + i) as u32;
            for (col, cell) in map_item(jenis, item, i as i64 + 1).into_iter().enumerate() {
                let c = col as u16;
                match cell {
                    Cell::Empty => {}
                    Cell::Num(v) => {
                        ws.write_number(r, c, v).map_err(excel_err)?;
                    }
                    Cell::Text(t) => {
                        ws.write_string(r, c, &t).map_err(excel_err)?;
                    }
                }
            }
        }
    }
    wb.save_to_buffer().map_err(excel_err)
}

/// `GET /api/spm-sanitasi/export`.
pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let f = ExportFilter {
        kecamatan: spm_sanitasi::int_or_null(&q, "kecamatan_id"),
        desa: spm_sanitasi::int_or_null(&q, "desa_id"),
        search: spm_sanitasi::input(&q, "search"),
        tahun: spm_sanitasi::input(&q, "tahun"),
    };
    let bytes = build_workbook(&state.pool, &f).await?;
    Ok(xlsx_response(bytes, "data_spm_sanitasi.xlsx"))
}

/// `GET /api/spm-sanitasi/import/template`: `SpmSanitasiExport` tanpa filter, sama dengan Laravel.
pub async fn download_template(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let bytes = build_workbook(&state.pool, &ExportFilter::default()).await?;
    Ok(xlsx_response(bytes, "template_spm_sanitasi.xlsx"))
}

// ---------------------------------------------------------------------------
// Impor
// ---------------------------------------------------------------------------

/// Sel impor seperti nilai PhpSpreadsheet (tanpa format tampilan).
#[derive(Clone)]
enum Raw {
    Empty,
    Num(f64),
    Text(String),
}

fn raw_of(d: &Data) -> Raw {
    match d {
        Data::Empty => Raw::Empty,
        Data::Int(i) => Raw::Num(*i as f64),
        Data::Float(f) => Raw::Num(*f),
        Data::String(s) => Raw::Text(s.clone()),
        Data::Bool(b) => Raw::Num(f64::from(u8::from(*b))),
        Data::DateTime(dt) => Raw::Num(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Raw::Text(s.clone()),
        Data::Error(_) => Raw::Text("#ERROR".into()),
    }
}

/// `(string)` PHP untuk sel: angka bulat tanpa desimal, selain itu representasi terpendek.
fn php_string(r: &Raw) -> String {
    match r {
        Raw::Empty => String::new(),
        Raw::Text(s) => s.clone(),
        Raw::Num(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
    }
}

/// `is_numeric` PHP untuk teks: angka dengan spasi tepi, tanda, dan eksponen opsional.
fn php_is_numeric_str(s: &str) -> bool {
    let t = s.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\x0b' || c == '\x0c');
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let fs = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - fs;
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let es = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == es {
            return false;
        }
    }
    i == b.len() && !t.is_empty()
}

/// `is_numeric` untuk sel: angka atau teks numerik.
fn is_numeric(r: &Raw) -> Option<f64> {
    match r {
        Raw::Num(f) => Some(*f),
        Raw::Text(s) if php_is_numeric_str(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `nullableString`: teks dipangkas, kosong dan `-` menjadi null.
fn nullable_string(r: &Raw) -> Option<String> {
    let t = php_string(r).trim().to_string();
    if t.is_empty() || t == "-" {
        None
    } else {
        Some(t)
    }
}

/// `nullableInt`: angka dipotong ke bilangan bulat (seperti `(int)`), selain itu null.
fn nullable_int(r: &Raw) -> Option<i64> {
    match r {
        Raw::Empty => None,
        Raw::Text(s) if s.is_empty() => None,
        _ => is_numeric(r).map(|f| f.trunc() as i64),
    }
}

/// `nullableFloat`: angka langsung. Teks lain dibersihkan dari karakter selain angka, koma, titik, dan minus.
fn nullable_float(r: &Raw) -> Option<f64> {
    match r {
        Raw::Empty => None,
        Raw::Text(s) if s.is_empty() => None,
        _ => {
            if let Some(f) = is_numeric(r) {
                return Some(f);
            }
            let text = php_string(r);
            let kept: String = text
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.' || *c == '-')
                .collect();
            let normalized = kept.replace(',', ".");
            if !normalized.is_empty() && php_is_numeric_str(&normalized) {
                normalized.parse::<f64>().ok()
            } else {
                None
            }
        }
    }
}

/// Nilai kolom impor setelah pemetaan. Tipe mengikuti kolom tabel.
#[derive(Clone)]
enum Val {
    I(Option<i64>),
    F(Option<f64>),
    S(Option<String>),
}

/// Pemetaan satu baris lembar `SPALDT`/`SPALDS` (`mapSpaldRow`). `None` berarti nama kosong.
/// `desa_id` tidak diisi di sini. Pemanggil mengisinya lewat `resolve_desa`.
fn map_spald(jenis: &str, row: &[Raw]) -> Option<Vec<(&'static str, Val)>> {
    let at = |i: usize| row.get(i).cloned().unwrap_or(Raw::Empty);
    let nama = php_string(&at(6)).trim().to_string();
    if nama.is_empty() {
        return None;
    }

    let tahun_i = if jenis == "spaldt" { 12 } else { 11 };
    let jiwa_i = if jenis == "spaldt" { Some(11) } else { None };
    let mut out: Vec<(&'static str, Val)> = vec![
        ("jenis", Val::S(Some(jenis.to_string()))),
        ("skala_pelayanan", Val::S(nullable_string(&at(1)))),
        ("nama_infrastruktur", Val::S(Some(nama))),
        ("latitude", Val::F(nullable_float(&at(7)))),
        ("longitude", Val::F(nullable_float(&at(8)))),
        ("alamat_lengkap", Val::S(nullable_string(&at(9)))),
        ("jumlah_pemanfaat_kk", Val::I(nullable_int(&at(10)))),
        (
            "jumlah_pemanfaat_jiwa",
            Val::I(jiwa_i.and_then(|i| nullable_int(&at(i)))),
        ),
        ("tahun_konstruksi", Val::I(nullable_int(&at(tahun_i)))),
        ("pembiayaan_apbn", Val::F(nullable_float(&at(tahun_i + 1)))),
        ("pembiayaan_apbd", Val::F(nullable_float(&at(tahun_i + 2)))),
        ("pembiayaan_dak", Val::F(nullable_float(&at(tahun_i + 3)))),
        ("pembiayaan_hibah", Val::F(nullable_float(&at(tahun_i + 4)))),
        ("pembiayaan_csr", Val::F(nullable_float(&at(tahun_i + 5)))),
        ("pembiayaan_lain", Val::F(nullable_float(&at(tahun_i + 6)))),
        ("pembiayaan_total", Val::F(nullable_float(&at(tahun_i + 7)))),
        ("status_keberfungsian", Val::S(nullable_string(&at(tahun_i + 8)))),
        ("kualitas_keberfungsian", Val::S(nullable_string(&at(tahun_i + 9)))),
        ("pengelola", Val::S(nullable_string(&at(tahun_i + 10)))),
        ("kapasitas_desain", Val::F(nullable_float(&at(tahun_i + 11)))),
        ("kapasitas_terpakai", Val::F(nullable_float(&at(tahun_i + 12)))),
        ("kapasitas_tidak_terpakai", Val::F(nullable_float(&at(tahun_i + 13)))),
        ("jenis_pengolahan", Val::S(nullable_string(&at(tahun_i + 14)))),
        ("peta_cakupan", Val::S(nullable_string(&at(tahun_i + 15)))),
        ("status_lahan", Val::S(nullable_string(&at(tahun_i + 16)))),
        ("luas_lahan_ha", Val::S(nullable_string(&at(tahun_i + 17)))),
        ("opsi_teknologi", Val::S(nullable_string(&at(tahun_i + 18)))),
        ("jumlah_stasiun_pompa", Val::S(nullable_string(&at(tahun_i + 19)))),
        ("biaya_operasional", Val::F(nullable_float(&at(tahun_i + 20)))),
    ];
    if jiwa_i.is_none() {
        // Payload spalds tidak memuat kolom jiwa. Nilai NULL sama dengan default kolom.
        out.retain(|(k, _)| *k != "jumlah_pemanfaat_jiwa");
    }
    Some(out)
}

/// Pemetaan satu baris lembar `IPLT` (`mapIpltRow`). `desa_id` diisi pemanggil seperti pada SPALDT/SPALDS.
fn map_iplt(row: &[Raw]) -> Option<Vec<(&'static str, Val)>> {
    let at = |i: usize| row.get(i).cloned().unwrap_or(Raw::Empty);
    let nama = php_string(&at(5)).trim().to_string();
    if nama.is_empty() {
        return None;
    }
    let tahun = nullable_int(&at(8)).or_else(|| nullable_int(&at(22)));
    Some(vec![
        ("jenis", Val::S(Some("iplt".into()))),
        ("nama_infrastruktur", Val::S(Some(nama))),
        ("latitude", Val::F(nullable_float(&at(6)))),
        ("longitude", Val::F(nullable_float(&at(7)))),
        ("tahun_konstruksi", Val::I(tahun)),
        ("jenis_pengelola", Val::S(nullable_string(&at(9)))),
        ("kapasitas_desain", Val::F(nullable_float(&at(10)))),
        ("kapasitas_terpakai", Val::F(nullable_float(&at(11)))),
        ("kapasitas_tidak_terpakai", Val::F(nullable_float(&at(12)))),
        ("status_keberfungsian", Val::S(nullable_string(&at(13)))),
        ("kualitas_keberfungsian", Val::S(nullable_string(&at(14)))),
        ("sistem_pengolahan", Val::S(nullable_string(&at(15)))),
        ("truk_tinja_unit", Val::I(nullable_int(&at(16)))),
        ("kapasitas_truk_m3", Val::F(nullable_float(&at(17)))),
        ("jumlah_ritasi", Val::I(nullable_int(&at(18)))),
        ("jarak_maksimal_pelayanan_km", Val::F(nullable_float(&at(19)))),
        ("alokasi_biaya_operasional", Val::F(nullable_float(&at(20)))),
        ("jumlah_pemanfaat_kk", Val::I(nullable_int(&at(21)))),
        ("pembiayaan_apbn", Val::F(nullable_float(&at(23)))),
        ("pembiayaan_apbd", Val::F(nullable_float(&at(24)))),
        ("pembiayaan_dak", Val::F(nullable_float(&at(25)))),
        ("pembiayaan_hibah", Val::F(nullable_float(&at(26)))),
        ("pembiayaan_csr", Val::F(nullable_float(&at(27)))),
        ("pembiayaan_lain", Val::F(nullable_float(&at(28)))),
        ("pembiayaan_total", Val::F(nullable_float(&at(29)))),
    ])
}

/// `resolveDesaId`: kecamatan dan desa dicocokkan tanpa memedulikan huruf besar kecil.
async fn resolve_desa(pool: &MySqlPool, kecamatan: &str, desa: &str) -> Result<Option<i64>, ApiError> {
    if kecamatan.is_empty() || desa.is_empty() {
        return Ok(None);
    }
    let kec: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_kecamatan WHERE LOWER(n_kec) = ? ORDER BY id LIMIT 1",
    )
    .bind(kecamatan.to_lowercase())
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(kec) = kec else {
        return Ok(None);
    };
    sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_desa WHERE kecamatan_id = ? AND LOWER(n_desa) = ? ORDER BY id LIMIT 1",
    )
    .bind(kec)
    .bind(desa.to_lowercase())
    .fetch_optional(pool)
    .await
    .map_err(internal)
}

/// `shouldSkipRow`: header, baris kosong, catatan `(...)`, dan baris yang kolom pertamanya bukan angka.
fn skip_row(row: &[Raw], index: usize) -> bool {
    if index < 4 {
        return true;
    }
    let first = php_string(&row.first().cloned().unwrap_or(Raw::Empty)).trim().to_string();
    if first.is_empty() || first.starts_with('(') || first.to_lowercase().starts_with("no") {
        return true;
    }
    !php_is_numeric_str(&first)
}

/// Baca semua sel lembar mulai A1, seperti `toArray()` PhpSpreadsheet.
fn sheet_grid(range: &calamine::Range<Data>) -> Vec<Vec<Raw>> {
    let (r0, c0) = range.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
    let mut grid: Vec<Vec<Raw>> = vec![Vec::new(); r0];
    for row in range.rows() {
        let mut line = vec![Raw::Empty; c0];
        line.extend(row.iter().map(raw_of));
        grid.push(line);
    }
    grid
}

/// Simpan satu baris dalam transaksinya, dengan audit `created` seperti `create()` di Laravel.
async fn insert_row(
    pool: &MySqlPool,
    headers: &HeaderMap,
    actor: u64,
    url: &str,
    payload: &[(&'static str, Val)],
) -> Result<(), ApiError> {
    let cols: Vec<&str> = payload.iter().map(|(k, _)| *k).collect();
    let placeholders = vec!["?"; cols.len()].join(", ");
    let sql = format!(
        "INSERT INTO tbl_spm_sanitasi ({}, created_at, updated_at) VALUES ({placeholders}, NOW(), NOW())",
        cols.join(", ")
    );
    let mut tx: Transaction<'_, MySql> = pool.begin().await.map_err(internal)?;
    let mut q = sqlx::query(&sql);
    for (_, v) in payload {
        q = match v {
            Val::I(x) => q.bind(*x),
            Val::F(x) => q.bind(*x),
            Val::S(x) => q.bind(x.clone()),
        };
    }
    let id = q.execute(&mut *tx).await.map_err(internal)?.last_insert_id() as i64;
    let attrs = spm_sanitasi::find(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("spm sanitasi baru tidak terbaca"))?;
    changes::log_linked(
        &mut tx,
        headers,
        actor,
        &SPM_TARGET,
        "created",
        id,
        None,
        Some(attrs),
        None,
        url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(())
}

/// `POST /api/spm-sanitasi/import`: impor tiga lembar dengan rekap `imported_rows`, `skipped_rows`, `errors`.
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;

    let mut upload: Option<(String, Vec<u8>)> = None;
    let mut replace_raw: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| internal(e.to_string()))?
    {
        match field.name() {
            Some("file") => {
                let name = field.file_name().unwrap_or_default().to_string();
                let bytes = field.bytes().await.map_err(|e| internal(e.to_string()))?;
                if !bytes.is_empty() {
                    upload = Some((name, bytes.to_vec()));
                }
            }
            Some("replace") => {
                replace_raw = Some(field.text().await.map_err(|e| internal(e.to_string()))?);
            }
            _ => {}
        }
    }

    // Validasi: file required|file|mimes:xlsx,xls,csv|max:20480, lalu replace sometimes|boolean.
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut first: Option<String> = None;
    let mut push = |field: &str, msg: String, errs: &mut BTreeMap<String, Vec<String>>| {
        errs.entry(field.to_string()).or_default().push(msg.clone());
        if first.is_none() {
            first = Some(msg);
        }
    };
    let mut file: Option<(String, Vec<u8>)> = None;
    match upload {
        None => push("file", "The file field is required.".into(), &mut errs),
        Some((name, bytes)) => {
            let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
            if !ALLOWED_EXT.contains(&ext.as_str()) {
                push("file", "The file field must be a file of type: xlsx, xls, csv.".into(), &mut errs);
            } else if bytes.len() as u64 > MAX_UPLOAD_KB * 1024 {
                push(
                    "file",
                    format!("The file field must not be greater than {MAX_UPLOAD_KB} kilobytes."),
                    &mut errs,
                );
            } else {
                file = Some((ext, bytes));
            }
        }
    }
    let replace = match replace_raw.as_deref().map(str::trim) {
        None | Some("") => false,
        Some(v) => match v.to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => true,
            "0" | "false" | "off" | "no" => false,
            _ => {
                push("replace", "The replace field must be true or false.".into(), &mut errs);
                false
            }
        },
    };
    if let Some(msg) = first {
        return Err(ApiError::validation(msg, errs));
    }
    let Some((ext, bytes)) = file else {
        return Err(ApiError::validation("The given data was invalid.", errs));
    };

    let gagal = |e: &dyn std::fmt::Display| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "message": format!("Gagal mengimport data: {e}") })),
        )
            .into_response()
    };

    // CSV dimuat tanpa lembar bernama, seperti PhpSpreadsheet: tidak ada yang diimpor.
    let mut workbook = if ext == "csv" {
        None
    } else {
        match open_workbook_auto_from_rs(Cursor::new(bytes)) {
            Ok(wb) => Some(wb),
            Err(e) => return Ok(gagal(&e)),
        }
    };

    if replace {
        if let Err(e) = sqlx::query("DELETE FROM tbl_spm_sanitasi").execute(&state.pool).await {
            return Ok(gagal(&e));
        }
    }

    let url = format!("{}/api/spm-sanitasi/import", state.app_url.trim_end_matches('/'));
    let mut imported: i64 = 0;
    let mut skipped: i64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for (sheet_name, jenis, _) in SHEETS {
        let Some(wb) = workbook.as_mut() else { break };
        let Ok(range) = wb.worksheet_range(sheet_name) else { continue };
        let grid = sheet_grid(&range);
        for (index, row) in grid.iter().enumerate() {
            if skip_row(row, index) {
                continue;
            }
            let payload = match *jenis {
                "iplt" => map_iplt(row),
                other => map_spald(other, row),
            };
            let Some(mut payload) = payload else {
                skipped += 1;
                continue;
            };
            // desa_id dari kecamatan (kolom 4) dan desa (kolom 5), kecuali IPLT yang memakai kolom 3 dan 4.
            let (kec_i, desa_i) = if *jenis == "iplt" { (3, 4) } else { (4, 5) };
            let kec = php_string(row.get(kec_i).unwrap_or(&Raw::Empty)).trim().to_string();
            let desa = php_string(row.get(desa_i).unwrap_or(&Raw::Empty)).trim().to_string();
            let desa_id = resolve_desa(&state.pool, &kec, &desa).await?;
            payload.push(("desa_id", Val::I(desa_id)));

            match insert_row(&state.pool, &headers, user.user_id, &url, &payload).await {
                Ok(()) => imported += 1,
                Err(e) => errors.push(format!("Sheet {sheet_name} baris {}: {}", index + 1, e.message)),
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": "Data SPM Sanitasi berhasil diimport",
        "imported_rows": imported,
        "skipped_rows": skipped,
        "errors": errors,
    }))
    .into_response())
}
