//! Excel kontrak: ekspor daftar (`GET /api/kontrak/export/excel`), template impor
//! (`GET /api/kontrak/import/template`), dan impor (`POST /api/kontrak/import`).
//!
//! Mengikuti `KontrakExport`, `KontrakTemplateExport`, dan `KontrakImport` di Laravel.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;

use axum::{
    extract::{Multipart, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use chrono::{Datelike, NaiveDate};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{changes, foto, kontrak, require_auth, AppState};

const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const ALLOWED_EXT: [&str; 3] = ["xlsx", "xls", "csv"];
const HEADING_PAKET: &str = "nama_paket_pisahkan_dengan_koma_jika_konsolidasi";
const HEADING_PAKET_LAMA: &str = "nama_paket";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `$request->tahun` dan `$request->search` dianggap ada bila tidak kosong dan bukan "0" (aturan PHP).
fn truthy(v: Option<&String>) -> Option<&str> {
    v.map(|s| s.as_str()).filter(|s| !s.is_empty() && *s != "0")
}

/// Sama dengan `preg_replace('/[^\pL\pN]+/u', '', ...)` lalu `mb_strtolower`.
pub(crate) fn normalize_lookup(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Sama dengan `Str::slug($heading, '_')` dari Laravel, dipakai `WithHeadingRow`.
pub(crate) fn heading_slug(value: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in value.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(c);
        } else {
            pending = true;
        }
    }
    out
}

/// Tanggal Excel (hari sejak 1899-12-30) menjadi tanggal kalender.
fn excel_serial_to_date(serial: f64) -> Option<NaiveDate> {
    if !serial.is_finite() || serial < 1.0 {
        return None;
    }
    let base = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    base.checked_add_signed(chrono::Duration::days(serial.floor() as i64))
}

/// Parser tanggal yang meniru `Carbon::parse` untuk format umum (`strtotime` PHP):
/// `YYYY-MM-DD`, `YYYY/MM/DD`, `MM/DD/YYYY` (slash = m/d/Y), dan `DD-MM-YYYY` (dash = d-m-Y).
fn parse_text_date(raw: &str) -> Option<NaiveDate> {
    let s = raw.trim();
    let date_part = s.split(['T', ' ']).next()?;
    if let Ok(d) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
        return Some(d);
    }
    if let Ok(d) = NaiveDate::parse_from_str(date_part, "%Y/%m/%d") {
        return Some(d);
    }
    if let Ok(d) = NaiveDate::parse_from_str(date_part, "%m/%d/%Y") {
        return Some(d);
    }
    if let Ok(d) = NaiveDate::parse_from_str(date_part, "%d-%m-%Y") {
        return Some(d);
    }
    None
}

/// Nilai sel sebagai teks, seperti PHP mengubah angka ke string (`12345.0` menjadi `12345`).
pub(crate) fn cell_text(cell: &Cell) -> Option<String> {
    match cell {
        Cell::Empty => None,
        Cell::Text(s) if s.is_empty() => None,
        Cell::Text(s) => Some(s.clone()),
        Cell::Number(n) => Some(number_text(*n)),
    }
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Sel setelah dibaca dari file: angka (termasuk tanggal Excel) atau teks.
#[derive(Debug, Clone)]
pub(crate) enum Cell {
    Empty,
    Text(String),
    Number(f64),
}

impl Cell {
    fn to_json(&self) -> Value {
        match self {
            Cell::Empty => Value::Null,
            Cell::Text(s) if s.is_empty() => Value::Null,
            Cell::Text(s) => Value::String(s.clone()),
            Cell::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    json!(*n as i64)
                } else {
                    json!(n)
                }
            }
        }
    }
}

fn from_calamine(data: &Data) -> Cell {
    match data {
        Data::Empty | Data::Error(_) => Cell::Empty,
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => {
            if s.is_empty() {
                Cell::Empty
            } else {
                Cell::Text(s.clone())
            }
        }
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(b) => Cell::Text(if *b { "1".into() } else { "0".into() }),
        Data::DateTime(dt) => Cell::Number(dt.as_f64()),
    }
}

// ---------------------------------------------------------------------------
// Ekspor daftar
// ---------------------------------------------------------------------------

struct ExportRow {
    nama_paket: Option<String>,
    kode_rekening: Option<String>,
    pagu: Option<f64>,
    sumber_dana: Option<String>,
    penyedia_nama: Option<String>,
    nilai_kontrak: Option<f64>,
    spk: Option<String>,
    tgl_spk: Option<NaiveDate>,
    spmk: Option<String>,
    tgl_spmk: Option<NaiveDate>,
    tgl_selesai: Option<NaiveDate>,
}

impl ExportRow {
    fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<Self, ApiError> {
        Ok(Self {
            nama_paket: r.try_get(0).map_err(internal)?,
            kode_rekening: r.try_get(1).map_err(internal)?,
            pagu: r.try_get(2).map_err(internal)?,
            sumber_dana: r.try_get(3).map_err(internal)?,
            penyedia_nama: r.try_get(4).map_err(internal)?,
            nilai_kontrak: r.try_get(5).map_err(internal)?,
            spk: r.try_get(6).map_err(internal)?,
            tgl_spk: r.try_get(7).map_err(internal)?,
            spmk: r.try_get(8).map_err(internal)?,
            tgl_spmk: r.try_get(9).map_err(internal)?,
            tgl_selesai: r.try_get(10).map_err(internal)?,
        })
    }
}

async fn export_rows(
    pool: &MySqlPool,
    tahun: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<ExportRow>, ApiError> {
    let mut sql = String::from(
        "SELECT p.nama_paket AS nama_paket, p.kode_rekening AS kode_rekening, CAST(p.pagu AS DOUBLE) AS pagu, \
         kg.sumber_dana AS sumber_dana, py.nama AS penyedia_nama, CAST(k.nilai_kontrak AS DOUBLE) AS nilai_kontrak, \
         k.spk AS spk, k.tgl_spk AS tgl_spk, k.spmk AS spmk, k.tgl_spmk AS tgl_spmk, k.tgl_selesai AS tgl_selesai \
         FROM tbl_kontrak k \
         LEFT JOIN tbl_pekerjaan p ON p.id = k.id_pekerjaan \
         LEFT JOIN tbl_kegiatan kg ON kg.id = p.kegiatan_id \
         LEFT JOIN tbl_penyedia py ON py.id = k.id_penyedia WHERE 1 = 1",
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(t) = tahun {
        sql.push_str(
            " AND k.id_kegiatan IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran = ?)",
        );
        binds.push(t.to_string());
    }
    if let Some(q) = search {
        let like = format!("%{q}%");
        sql.push_str(
            " AND (k.kode_rup LIKE ? OR k.nomor_penawaran LIKE ? OR k.kode_paket LIKE ? \
             OR k.id_pekerjaan IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?) \
             OR k.id_penyedia IN (SELECT id FROM tbl_penyedia WHERE nama LIKE ?))",
        );
        binds.extend(std::iter::repeat_n(like, 5));
    }
    sql.push_str(" ORDER BY k.created_at DESC, k.id DESC");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    rows.iter().map(ExportRow::from_row).collect()
}

/// Selisih hari inklusif `tgl_spmk` sampai `tgl_selesai` (`diff()->days + 1`), atau "-".
fn masa_pelaksanaan(spmk: Option<NaiveDate>, selesai: Option<NaiveDate>) -> Option<i64> {
    match (spmk, selesai) {
        (Some(a), Some(b)) => Some((b - a).num_days().abs() + 1),
        _ => None,
    }
}

fn excel_error(e: impl std::fmt::Display) -> ApiError {
    internal(format!("Gagal membuat berkas Excel: {e}"))
}

fn date_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    date: Option<NaiveDate>,
    fmt: &Format,
) -> Result<(), ApiError> {
    match date {
        Some(d) => {
            let dt = rust_xlsxwriter::ExcelDateTime::from_ymd(
                d.year() as u16,
                d.month() as u8,
                d.day() as u8,
            )
            .map_err(excel_error)?;
            ws.write_datetime_with_format(row, col, &dt, fmt)
                .map_err(excel_error)?;
        }
        None => {
            ws.write_blank(row, col, fmt).map_err(excel_error)?;
        }
    }
    Ok(())
}

fn text_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    value: Option<&str>,
    fmt: &Format,
) -> Result<(), ApiError> {
    match value {
        Some(v) if !v.is_empty() => ws
            .write_string_with_format(row, col, v, fmt)
            .map_err(excel_error)?,
        _ => ws.write_blank(row, col, fmt).map_err(excel_error)?,
    };
    Ok(())
}

fn number_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    value: Option<f64>,
    fmt: &Format,
) -> Result<(), ApiError> {
    match value {
        Some(v) => ws
            .write_number_with_format(row, col, v, fmt)
            .map_err(excel_error)?,
        None => ws.write_blank(row, col, fmt).map_err(excel_error)?,
    };
    Ok(())
}

pub(crate) fn xlsx_response(bytes: Vec<u8>, filename: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, XLSX_MIME.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// `GET /api/kontrak/export/excel?tahun=&search=`: 12 kolom, baris pertama tebal.
pub async fn export_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let tahun = truthy(query.get("tahun"));
    let search = truthy(query.get("search"));
    let rows = export_rows(&state.pool, tahun, search).await?;

    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");
    let plain = Format::new();
    let ws = wb.add_worksheet();
    let headings = [
        "Nama Pekerjaan",
        "Kode Rekening",
        "Pagu",
        "Sumber Dana",
        "Penyedia",
        "Nilai Kontrak",
        "Nomor SPK",
        "Tanggal SPK",
        "Nomor SPMK",
        "Tanggal SPMK",
        "Masa Pelaksanaan (Hari)",
        "Tanggal Selesai",
    ];
    for (col, h) in headings.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(excel_error)?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        text_cell(ws, row, 0, r.nama_paket.as_deref(), &plain)?;
        text_cell(ws, row, 1, r.kode_rekening.as_deref(), &plain)?;
        number_cell(ws, row, 2, r.pagu, &plain)?;
        text_cell(ws, row, 3, r.sumber_dana.as_deref(), &plain)?;
        text_cell(ws, row, 4, r.penyedia_nama.as_deref(), &plain)?;
        number_cell(ws, row, 5, r.nilai_kontrak, &plain)?;
        text_cell(ws, row, 6, r.spk.as_deref(), &plain)?;
        date_cell(ws, row, 7, r.tgl_spk, &date_fmt)?;
        text_cell(ws, row, 8, r.spmk.as_deref(), &plain)?;
        date_cell(ws, row, 9, r.tgl_spmk, &date_fmt)?;
        match masa_pelaksanaan(r.tgl_spmk, r.tgl_selesai) {
            Some(days) => ws
                .write_number_with_format(row, 10, days as f64, &plain)
                .map_err(excel_error)?,
            None => ws
                .write_string_with_format(row, 10, "-", &plain)
                .map_err(excel_error)?,
        };
        date_cell(ws, row, 11, r.tgl_selesai, &date_fmt)?;
    }
    ws.autofit();
    let bytes = wb.save_to_buffer().map_err(excel_error)?;
    Ok(xlsx_response(bytes, "data_kontrak.xlsx"))
}

// ---------------------------------------------------------------------------
// Template impor
// ---------------------------------------------------------------------------

const TEMPLATE_HEADINGS: [&str; 14] = [
    "Nama Paket (pisahkan dengan koma jika konsolidasi)",
    "Nama Penyedia",
    "Kode RUP",
    "Kode Paket",
    "Nomor Penawaran",
    "Tanggal Penawaran",
    "Nilai Kontrak",
    "Tanggal SPPBJ",
    "Nomor SPPBJ",
    "Tanggal SPK",
    "Nomor SPK",
    "Tanggal SPMK",
    "Nomor SPMK",
    "Tanggal Selesai Kontrak",
];

const VALIDASI_HEADINGS: [&str; 6] = [
    "No",
    "Nama Paket",
    "Status Pekerjaan",
    "Nama Penyedia",
    "Status Penyedia",
    "Keterangan",
];

/// Rumus sheet "Validasi Import" untuk satu baris; `{r}` diganti nomor baris.
const VALIDASI_FORMULAS: [&str; 6] = [
    "=ROW()-1",
    "='Import Kontrak'!A{r}",
    r#"=IF(B{r}="","",IF(COUNTIF('Referensi Pekerjaan'!$B:$B,LOWER(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(TRIM('Import Kontrak'!A{r}),"  ",""),".",""),"-",""),"/",""),",",""),CHAR(160),"")))>0,"OK","Tidak ditemukan"))"#,
    "='Import Kontrak'!B{r}",
    r#"=IF(D{r}="","",IF(COUNTIF('Referensi Penyedia'!$B:$B,LOWER(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(SUBSTITUTE(TRIM('Import Kontrak'!B{r}),"  ",""),".",""),"-",""),"/",""),",",""),CHAR(160),"")))>0,"OK","Tidak ditemukan"))"#,
    r#"=TRIM(IF(C{r}<>"OK","Pekerjaan tidak ditemukan; ","")&IF(E{r}<>"OK","Penyedia tidak ditemukan",""))"#,
];

/// Baris data sheet "Validasi Import" yang diisi rumus (sampai baris 501).
const VALIDASI_LAST_ROW: u32 = 501;

async fn pekerjaan_ref(
    pool: &MySqlPool,
    tahun: Option<&str>,
) -> Result<Vec<(String, String, Option<f64>)>, ApiError> {
    let mut sql = String::from(
        "SELECT p.nama_paket, p.kode_rekening, CAST(p.pagu AS DOUBLE) FROM tbl_pekerjaan p",
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(t) = tahun {
        sql.push_str(
            " WHERE p.kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran = ?)",
        );
        binds.push(t.to_string());
    }
    sql.push_str(" ORDER BY p.id");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    rows.iter()
        .map(|r| {
            let nama: Option<String> = r.try_get(0).map_err(internal)?;
            let kode: Option<String> = r.try_get(1).map_err(internal)?;
            let pagu: Option<f64> = r.try_get(2).map_err(internal)?;
            Ok((nama.unwrap_or_default(), kode.unwrap_or_default(), pagu))
        })
        .collect()
}

async fn penyedia_ref(pool: &MySqlPool) -> Result<Vec<(String, String, String)>, ApiError> {
    let rows = sqlx::query("SELECT nama, direktur, alamat FROM tbl_penyedia ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get::<String, _>(0).map_err(internal)?,
                r.try_get::<String, _>(1).map_err(internal)?,
                r.try_get::<String, _>(2).map_err(internal)?,
            ))
        })
        .collect()
}

/// `GET /api/kontrak/import/template?tahun=`: sheet Import, Validasi, dan dua referensi.
pub async fn download_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let tahun = truthy(query.get("tahun"));
    let pekerjaan = pekerjaan_ref(&state.pool, tahun).await?;
    let penyedia = penyedia_ref(&state.pool).await?;

    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let grey = Format::new().set_font_color(rust_xlsxwriter::Color::RGB(0x666666));

    let ws = wb
        .add_worksheet()
        .set_name("Import Kontrak")
        .map_err(excel_error)?;
    for (col, h) in TEMPLATE_HEADINGS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(excel_error)?;
    }
    ws.autofit();

    let ws = wb
        .add_worksheet()
        .set_name("Validasi Import")
        .map_err(excel_error)?;
    for (col, h) in VALIDASI_HEADINGS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(excel_error)?;
    }
    for row in 1..=VALIDASI_LAST_ROW {
        for (col, template) in VALIDASI_FORMULAS.iter().enumerate() {
            let formula = template.replace("{r}", &(row + 1).to_string());
            ws.write_formula(row, col as u16, formula.as_str())
                .map_err(excel_error)?;
        }
    }
    ws.set_freeze_panes(1, 0).map_err(excel_error)?;
    ws.autofilter(0, 0, VALIDASI_LAST_ROW, 5)
        .map_err(excel_error)?;
    ws.autofit();

    let ws = wb
        .add_worksheet()
        .set_name("Referensi Pekerjaan")
        .map_err(excel_error)?;
    for (col, h) in [
        "Nama Paket",
        "Nama Paket Normalized",
        "Kode Rekening",
        "Pagu",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(excel_error)?;
    }
    for (i, (nama, kode, pagu)) in pekerjaan.iter().enumerate() {
        let row = (i + 1) as u32;
        text_cell(ws, row, 0, Some(nama), &Format::new())?;
        ws.write_string_with_format(row, 1, normalize_lookup(nama), &grey)
            .map_err(excel_error)?;
        text_cell(ws, row, 2, Some(kode), &Format::new())?;
        number_cell(ws, row, 3, *pagu, &Format::new())?;
    }
    ws.autofit();

    let ws = wb
        .add_worksheet()
        .set_name("Referensi Penyedia")
        .map_err(excel_error)?;
    for (col, h) in [
        "Nama Penyedia",
        "Nama Normalized",
        "Direktur",
        "Direktur Normalized",
        "Alamat",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(excel_error)?;
    }
    for (i, (nama, direktur, alamat)) in penyedia.iter().enumerate() {
        let row = (i + 1) as u32;
        text_cell(ws, row, 0, Some(nama), &Format::new())?;
        ws.write_string_with_format(row, 1, normalize_lookup(nama), &grey)
            .map_err(excel_error)?;
        text_cell(ws, row, 2, Some(direktur), &Format::new())?;
        ws.write_string_with_format(row, 3, normalize_lookup(direktur), &grey)
            .map_err(excel_error)?;
        text_cell(ws, row, 4, Some(alamat), &Format::new())?;
    }
    ws.autofit();

    let bytes = wb.save_to_buffer().map_err(excel_error)?;
    Ok(xlsx_response(bytes, "template_kontrak.xlsx"))
}

// ---------------------------------------------------------------------------
// Impor
// ---------------------------------------------------------------------------

/// Satu baris data: heading (slug) ke nilai sel, dalam urutan kolom.
pub(crate) struct ImportRow {
    pub(crate) row_number: u64,
    pub(crate) values: BTreeMap<String, Cell>,
}

impl ImportRow {
    pub(crate) fn get(&self, key: &str) -> Option<&Cell> {
        self.values.get(key).filter(|c| !matches!(c, Cell::Empty))
    }

    pub(crate) fn text(&self, key: &str) -> Option<String> {
        self.get(key).and_then(cell_text)
    }

    fn values_json(&self) -> Value {
        Value::Object(
            self.values
                .iter()
                .map(|(k, v)| (k.clone(), v.to_json()))
                .collect::<Map<_, _>>(),
        )
    }
}

/// Membaca sheet pertama. Baris pertama adalah heading; baris kosong dilewati.
pub(crate) fn read_rows(bytes: &[u8], ext: &str) -> Result<(Vec<String>, Vec<ImportRow>), String> {
    let (headings, rows): (Vec<String>, Vec<(u64, Vec<Cell>)>) = if ext == "csv" {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(bytes);
        let mut all: Vec<(u64, Vec<Cell>)> = Vec::new();
        for (i, rec) in reader.records().enumerate() {
            let rec = rec.map_err(|e| e.to_string())?;
            let cells = rec
                .iter()
                .map(|v| {
                    if v.is_empty() {
                        Cell::Empty
                    } else {
                        Cell::Text(v.to_string())
                    }
                })
                .collect();
            all.push(((i + 1) as u64, cells));
        }
        if all.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let headings = all
            .remove(0)
            .1
            .iter()
            .map(|c| cell_text(c).unwrap_or_default())
            .collect();
        (headings, all)
    } else {
        let mut wb =
            open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
        let range = wb
            .worksheet_range_at(0)
            .ok_or("Berkas tidak memiliki sheet")?
            .map_err(|e| e.to_string())?;
        let start_row = range.start().map(|(r, _)| r as u64).unwrap_or(0);
        let mut all: Vec<(u64, Vec<Cell>)> = range
            .rows()
            .enumerate()
            .map(|(i, cells)| {
                (
                    start_row + i as u64 + 1,
                    cells.iter().map(from_calamine).collect(),
                )
            })
            .collect();
        if all.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let headings = all
            .remove(0)
            .1
            .iter()
            .map(|c| cell_text(c).unwrap_or_default())
            .collect();
        (headings, all)
    };

    let keys: Vec<String> = headings.iter().map(|h| heading_slug(h)).collect();
    let mut out = Vec::new();
    for (row_number, cells) in rows {
        if cells
            .iter()
            .all(|c| matches!(c, Cell::Empty) || matches!(c, Cell::Text(t) if t.trim().is_empty()))
        {
            continue;
        }
        let mut values = BTreeMap::new();
        for (i, key) in keys.iter().enumerate() {
            if key.is_empty() {
                continue;
            }
            let cell = cells.get(i).cloned().unwrap_or(Cell::Empty);
            values.insert(key.clone(), cell);
        }
        out.push(ImportRow { row_number, values });
    }
    Ok((keys, out))
}

/// `parseDate`: angka dianggap tanggal Excel, selain itu diparse sebagai teks.
fn parse_date(cell: Option<&Cell>) -> Option<NaiveDate> {
    match cell? {
        Cell::Empty => None,
        Cell::Number(n) => {
            if *n == 0.0 {
                None
            } else {
                excel_serial_to_date(*n)
            }
        }
        Cell::Text(s) => parse_text_date(s),
    }
}

/// `parseNumber`: angka apa adanya; teks dibersihkan (koma jadi titik, selain digit dan titik dibuang).
fn parse_number(cell: Option<&Cell>) -> f64 {
    match cell {
        None | Some(Cell::Empty) => 0.0,
        Some(Cell::Number(n)) => *n,
        Some(Cell::Text(s)) => {
            if let Ok(n) = s.trim().parse::<f64>() {
                return n;
            }
            let clean: String = s
                .replace(',', ".")
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            php_float_prefix(&clean)
        }
    }
}

/// `(float)` PHP: angka dari awal teks, berhenti pada karakter yang tidak cocok.
pub(crate) fn php_float_prefix(s: &str) -> f64 {
    let mut end = 0;
    let mut seen_dot = false;
    let bytes = s.as_bytes();
    while end < bytes.len() {
        match bytes[end] {
            b'0'..=b'9' => end += 1,
            b'.' if !seen_dot => {
                seen_dot = true;
                end += 1
            }
            _ => break,
        }
    }
    s[..end].parse::<f64>().unwrap_or(0.0)
}

#[derive(Clone)]
struct PekerjaanCandidate {
    id: i64,
    nama_paket: String,
    kegiatan_id: Option<i64>,
    tahun: Option<i64>,
}

#[derive(Clone)]
struct PenyediaCandidate {
    id: i64,
    nama: String,
    direktur: String,
}

async fn load_pekerjaan(pool: &MySqlPool) -> Result<Vec<PekerjaanCandidate>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(p.id AS SIGNED), p.nama_paket, CAST(p.kegiatan_id AS SIGNED), CAST(kg.tahun_anggaran AS SIGNED) \
         FROM tbl_pekerjaan p LEFT JOIN tbl_kegiatan kg ON kg.id = p.kegiatan_id ORDER BY p.id",
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(PekerjaanCandidate {
                id: r.try_get(0).map_err(internal)?,
                nama_paket: r
                    .try_get::<Option<String>, _>(1)
                    .map_err(internal)?
                    .unwrap_or_default(),
                kegiatan_id: r.try_get(2).map_err(internal)?,
                tahun: r.try_get(3).map_err(internal)?,
            })
        })
        .collect()
}

async fn load_penyedia(pool: &MySqlPool) -> Result<Vec<PenyediaCandidate>, ApiError> {
    let rows =
        sqlx::query("SELECT CAST(id AS SIGNED), nama, direktur FROM tbl_penyedia ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(PenyediaCandidate {
                id: r.try_get(0).map_err(internal)?,
                nama: r
                    .try_get::<Option<String>, _>(1)
                    .map_err(internal)?
                    .unwrap_or_default(),
                direktur: r
                    .try_get::<Option<String>, _>(2)
                    .map_err(internal)?
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// `findPekerjaan`: cocok nama (setelah normalisasi) di tahun anggaran acuan, lalu di semua paket.
fn find_pekerjaan<'a>(
    all: &'a [PekerjaanCandidate],
    name: &str,
    ref_year: Option<i64>,
) -> Option<&'a PekerjaanCandidate> {
    let target = normalize_lookup(name);
    if let Some(year) = ref_year {
        if let Some(m) = all
            .iter()
            .find(|p| p.tahun == Some(year) && normalize_lookup(&p.nama_paket) == target)
        {
            return Some(m);
        }
    }
    all.iter()
        .find(|p| normalize_lookup(&p.nama_paket) == target)
}

fn find_penyedia<'a>(all: &'a [PenyediaCandidate], name: &str) -> Option<&'a PenyediaCandidate> {
    let target = normalize_lookup(name);
    all.iter()
        .find(|p| normalize_lookup(&p.nama) == target || normalize_lookup(&p.direktur) == target)
}

/// Sel teks yang dipakai sebagai nilai kolom (`?? null` di Laravel).
fn col_text(row: &ImportRow, key: &str) -> Option<String> {
    row.text(key)
}

/// PHP `empty()` untuk string: kosong atau "0".
fn php_empty(s: &str) -> bool {
    s.is_empty() || s == "0"
}

struct Failure {
    row: u64,
    message: String,
    values: Value,
    debug: Option<Value>,
}

/// `POST /api/kontrak/import`: multipart dengan field `file` (xlsx, xls, atau csv).
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut upload: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| internal(e.to_string()))?
    {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or_default().to_string();
            let bytes = field.bytes().await.map_err(|e| internal(e.to_string()))?;
            if !bytes.is_empty() {
                upload = Some((name, bytes.to_vec()));
            }
        }
    }
    let Some((file_name, bytes)) = upload else {
        let mut errs = BTreeMap::new();
        foto::add(&mut errs, "file", "The file field is required.".into());
        return Err(ApiError::validation("The given data was invalid.", errs));
    };
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        let mut errs = BTreeMap::new();
        foto::add(
            &mut errs,
            "file",
            "The file field must be a file of type: xlsx, xls, csv.".into(),
        );
        return Err(ApiError::validation("The given data was invalid.", errs));
    }

    let (_, rows) = match read_rows(&bytes, &ext) {
        Ok(v) => v,
        Err(e) => {
            return Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": format!("Gagal mengimport kontrak: {e}") })),
            )
                .into_response())
        }
    };

    let pekerjaan_all = load_pekerjaan(&state.pool).await?;
    let penyedia_all = load_penyedia(&state.pool).await?;
    let url = format!("{}/api/kontrak", state.app_url.trim_end_matches('/'));

    let mut total_rows = 0u64;
    let mut imported = 0u64;
    let mut failures: Vec<Failure> = Vec::new();

    for row in &rows {
        let raw_names = row
            .get(HEADING_PAKET)
            .or_else(|| row.get(HEADING_PAKET_LAMA))
            .and_then(cell_text)
            .unwrap_or_default();
        if raw_names.trim().is_empty() {
            continue;
        }
        total_rows += 1;

        let tgl_sppbj = parse_date(row.get("tanggal_sppbj"));
        let tgl_spk = parse_date(row.get("tanggal_spk"));
        let tgl_spmk = parse_date(row.get("tanggal_spmk"));
        let tgl_selesai = parse_date(row.get("tanggal_selesai_kontrak"));
        let tgl_penawaran = parse_date(row.get("tanggal_penawaran"));

        let ref_date = tgl_spk.or(tgl_spmk).or(tgl_sppbj);
        let ref_year = ref_date.map(|d| d.year() as i64);

        let names: Vec<String> = raw_names
            .trim()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !php_empty(s))
            .collect();

        let mut pekerjaans: Vec<&PekerjaanCandidate> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        for name in &names {
            match find_pekerjaan(&pekerjaan_all, name, ref_year) {
                Some(p) => pekerjaans.push(p),
                None => missing.push(name.clone()),
            }
        }

        let penyedia_name = col_text(row, "nama_penyedia")
            .map(|s| s.trim().to_string())
            .filter(|s| !php_empty(s));
        let penyedia = penyedia_name
            .as_deref()
            .and_then(|n| find_penyedia(&penyedia_all, n));

        if !missing.is_empty() || pekerjaans.is_empty() || penyedia.is_none() {
            let mut reason = String::new();
            if !missing.is_empty() {
                let missing_str = missing.join(", ");
                match ref_year {
                    Some(y) => reason.push_str(&format!("Pekerjaan '{missing_str}' tidak ditemukan pada tahun anggaran {y}. ")),
                    None => reason.push_str(&format!(
                        "Pekerjaan '{missing_str}' tidak ditemukan (Tahun anggaran tidak dapat ditentukan dari tanggal SPK/SPMK). "
                    )),
                }
            }
            if penyedia.is_none() {
                reason.push_str(&format!(
                    "Penyedia '{}' tidak ditemukan. ",
                    penyedia_name.clone().unwrap_or_default()
                ));
            }
            failures.push(Failure {
                row: row.row_number,
                message: reason,
                values: row.values_json(),
                debug: Some(json!({
                    "normalized_pekerjaan": names.iter().map(|n| normalize_lookup(n)).collect::<Vec<_>>(),
                    "normalized_penyedia": normalize_lookup(penyedia_name.as_deref().unwrap_or("")),
                    "ref_year": ref_year,
                })),
            });
            continue;
        }

        let penyedia = penyedia.expect("penyedia dicek di atas");
        let first = pekerjaans[0];
        let ids: Vec<i64> = pekerjaans.iter().map(|p| p.id).collect();
        let result = save_row(
            &state,
            &headers,
            user.user_id,
            &url,
            row,
            penyedia.id,
            first,
            &ids,
            NewFields {
                tgl_penawaran,
                tgl_sppbj,
                tgl_spk,
                tgl_spmk,
                tgl_selesai,
            },
        )
        .await;
        match result {
            Ok(()) => imported += 1,
            Err(e) => failures.push(Failure {
                row: row.row_number,
                message: e.message.clone(),
                values: row.values_json(),
                debug: None,
            }),
        }
    }

    let errors: Vec<Value> = failures
        .iter()
        .map(
            |f| json!({ "row": f.row, "message": f.message, "values": f.values, "debug": f.debug }),
        )
        .collect();
    Ok(Json(json!({
        "message": "Import selesai",
        "success_count": imported,
        "error_count": errors.len(),
        "errors": errors,
        "debug": { "total_rows_excel": total_rows, "total_skipped": 0 },
    }))
    .into_response())
}

struct NewFields {
    tgl_penawaran: Option<NaiveDate>,
    tgl_sppbj: Option<NaiveDate>,
    tgl_spk: Option<NaiveDate>,
    tgl_spmk: Option<NaiveDate>,
    tgl_selesai: Option<NaiveDate>,
}

/// Satu baris impor dalam transaksi sendiri. Kontrak lama dari paket pertama diperbarui;
/// jika tidak ada, dibuat baru. Pivot disinkron ke semua paket baris itu.
#[allow(clippy::too_many_arguments)]
async fn save_row(
    state: &AppState,
    headers: &HeaderMap,
    actor: u64,
    url: &str,
    row: &ImportRow,
    penyedia_id: i64,
    first: &PekerjaanCandidate,
    pekerjaan_ids: &[i64],
    f: NewFields,
) -> Result<(), ApiError> {
    let nilai = parse_number(row.get("nilai_kontrak"));
    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;

    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(k.id AS SIGNED) FROM tbl_kontrak k \
         JOIN kontrak_pekerjaan kp ON kp.kontrak_id = k.id WHERE kp.pekerjaan_id = ? ORDER BY k.id LIMIT 1",
    )
    .bind(first.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;

    match existing {
        Some(id) => {
            let old = kontrak::find_row(&mut *tx, id)
                .await
                .map_err(internal)?
                .ok_or_else(ApiError::not_found)?;
            update_fields(
                &mut tx,
                id,
                penyedia_id,
                first.kegiatan_id,
                first.id,
                row,
                nilai,
                &f,
            )
            .await?;
            let after = kontrak::find_row(&mut *tx, id)
                .await
                .map_err(internal)?
                .ok_or_else(ApiError::not_found)?;
            kontrak::sync_pekerjaan(&mut tx, id, pekerjaan_ids).await?;
            changes::log(
                &mut tx,
                headers,
                actor,
                &changes::KONTRAK,
                "updated",
                id,
                Some(kontrak::attributes(&old)),
                Some(kontrak::attributes(&after)),
                after.id_pekerjaan,
                url,
            )
            .await?;
        }
        None => {
            let res = sqlx::query(
                "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, kode_rup, kode_paket, nomor_penawaran, \
                 tanggal_penawaran, nilai_kontrak, tgl_sppbj, tgl_spk, tgl_spmk, tgl_selesai, sppbj, spk, spmk, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(first.kegiatan_id)
            .bind(first.id)
            .bind(penyedia_id)
            .bind(col_text(row, "kode_rup"))
            .bind(col_text(row, "kode_paket"))
            .bind(col_text(row, "nomor_penawaran"))
            .bind(f.tgl_penawaran)
            .bind(nilai)
            .bind(f.tgl_sppbj)
            .bind(f.tgl_spk)
            .bind(f.tgl_spmk)
            .bind(f.tgl_selesai)
            .bind(col_text(row, "nomor_sppbj"))
            .bind(col_text(row, "nomor_spk"))
            .bind(col_text(row, "nomor_spmk"))
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            let id = res.last_insert_id() as i64;
            kontrak::sync_pekerjaan(&mut tx, id, pekerjaan_ids).await?;
            let created = kontrak::find_row(&mut *tx, id)
                .await
                .map_err(internal)?
                .ok_or_else(|| internal("kontrak baru tidak terbaca"))?;
            changes::log(
                &mut tx,
                headers,
                actor,
                &changes::KONTRAK,
                "created",
                id,
                None,
                Some(kontrak::attributes(&created)),
                created.id_pekerjaan,
                url,
            )
            .await?;
        }
    }
    tx.commit().await.map_err(internal)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update_fields(
    tx: &mut Transaction<'_, MySql>,
    id: i64,
    penyedia_id: i64,
    kegiatan_id: Option<i64>,
    pekerjaan_id: i64,
    row: &ImportRow,
    nilai: f64,
    f: &NewFields,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE tbl_kontrak SET id_penyedia = ?, id_kegiatan = ?, id_pekerjaan = ?, kode_rup = ?, kode_paket = ?, \
         nomor_penawaran = ?, tanggal_penawaran = ?, nilai_kontrak = ?, tgl_sppbj = ?, tgl_spk = ?, tgl_spmk = ?, \
         tgl_selesai = ?, sppbj = ?, spk = ?, spmk = ?, updated_at = NOW() WHERE id = ?",
    )
    .bind(penyedia_id)
    .bind(kegiatan_id)
    .bind(pekerjaan_id)
    .bind(col_text(row, "kode_rup"))
    .bind(col_text(row, "kode_paket"))
    .bind(col_text(row, "nomor_penawaran"))
    .bind(f.tgl_penawaran)
    .bind(nilai)
    .bind(f.tgl_sppbj)
    .bind(f.tgl_spk)
    .bind(f.tgl_spmk)
    .bind(f.tgl_selesai)
    .bind(col_text(row, "nomor_sppbj"))
    .bind(col_text(row, "nomor_spk"))
    .bind(col_text(row, "nomor_spmk"))
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_slug_matches_laravel_slug() {
        assert_eq!(
            heading_slug("Nama Paket (pisahkan dengan koma jika konsolidasi)"),
            "nama_paket_pisahkan_dengan_koma_jika_konsolidasi"
        );
        assert_eq!(
            heading_slug("Tanggal Selesai Kontrak"),
            "tanggal_selesai_kontrak"
        );
        assert_eq!(heading_slug("Nomor SPPBJ"), "nomor_sppbj");
    }

    #[test]
    fn normalize_drops_punctuation_and_case() {
        assert_eq!(normalize_lookup(" CV. Maju-Jaya / 2 "), "cvmajujaya2");
        assert_eq!(normalize_lookup("Rehab, Jalan."), "rehabjalan");
    }

    #[test]
    fn number_parsing_follows_php_cast() {
        assert_eq!(parse_number(Some(&Cell::Number(12.5))), 12.5);
        assert_eq!(
            parse_number(Some(&Cell::Text("1500000".into()))),
            1_500_000.0
        );
        assert_eq!(
            parse_number(Some(&Cell::Text("Rp 1.000.000,50".into()))),
            1.0
        );
        assert_eq!(parse_number(None), 0.0);
    }

    #[test]
    fn dates_from_serial_and_text() {
        assert_eq!(
            excel_serial_to_date(45292.0),
            NaiveDate::from_ymd_opt(2024, 1, 1)
        );
        assert_eq!(
            parse_text_date("2026-01-10"),
            NaiveDate::from_ymd_opt(2026, 1, 10)
        );
        assert_eq!(
            parse_text_date("2026-01-10 00:00:00"),
            NaiveDate::from_ymd_opt(2026, 1, 10)
        );
        assert_eq!(
            parse_text_date("01/10/2026"),
            NaiveDate::from_ymd_opt(2026, 1, 10)
        );
        assert_eq!(parse_text_date("bukan tanggal"), None);
    }

    #[test]
    fn masa_is_inclusive_days() {
        let a = NaiveDate::from_ymd_opt(2026, 1, 1);
        let b = NaiveDate::from_ymd_opt(2026, 1, 10);
        assert_eq!(masa_pelaksanaan(a, b), Some(10));
        assert_eq!(masa_pelaksanaan(a, None), None);
    }
}
