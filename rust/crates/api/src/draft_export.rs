//! `GET /api/draft-pekerjaan/export/excel?tahun=&search=` (`DraftPekerjaanController::exportExcel`
//! dan `App\Exports\DraftPekerjaanExport`).
//!
//! Setara Laravel: pekerjaan dibatasi dengan `byUserRole()`, filter `tahun` lewat relasi kegiatan,
//! `search` pada `nama_paket` atau `kode_rekening`, 9 kolom, baris pertama tebal, nama berkas
//! `data_draft_pekerjaan.xlsx`.
//!
//! Perbedaan yang diketahui:
//! - Kolom Kecamatan dan Desa kosong, sama dengan Laravel. Model memakai atribut `nama_kecamatan`
//!   dan `nama_desa` yang tidak ada (kolom asli `n_kec` dan `n_desa`), jadi Laravel juga menulis null.
//! - Draft per pekerjaan diambil dari baris dengan id terbesar (hasOne eager load Laravel tanpa urutan).
//! - Urutan baris: `ORDER BY p.id`. Laravel tanpa urutan eksplisit.

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{access, require_auth, AppState};

const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const FILE_NAME: &str = "data_draft_pekerjaan.xlsx";
const HEADINGS: [&str; 9] = [
    "Nama Pekerjaan",
    "Kode Rekening",
    "Kecamatan",
    "Desa",
    "Pagu",
    "Kode RUP",
    "Kode Paket",
    "Nama Pelaksana",
    "Penyedia",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `$request->tahun` dan `$request->search`: string kosong dan "0" dianggap tidak ada (truthy PHP).
fn truthy(value: Option<&String>) -> Option<&str> {
    value
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty() && *s != "0")
}

struct ExportRow {
    nama_paket: Option<String>,
    kode_rekening: Option<String>,
    pagu: Option<f64>,
    kode_rup: Option<String>,
    kode_paket: Option<String>,
    nama_pelaksana: Option<String>,
    penyedia_nama: Option<String>,
}

async fn export_rows(
    pool: &MySqlPool,
    user_id: u64,
    roles: &[(u64, String)],
    tahun: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<ExportRow>, ApiError> {
    let restriction = access::restriction(user_id, roles, "p");
    let mut sql = format!(
        "SELECT p.nama_paket AS nama_paket, p.kode_rekening AS kode_rekening, \
         CAST(p.pagu AS DOUBLE) AS pagu, dr.kode_rup AS kode_rup, dr.kode_paket AS kode_paket, \
         dr.nama_pelaksana AS nama_pelaksana, py.nama AS penyedia_nama \
         FROM tbl_pekerjaan p \
         LEFT JOIN tbl_draft_pekerjaan dr ON dr.id = (SELECT MAX(d.id) FROM tbl_draft_pekerjaan d WHERE d.pekerjaan_id = p.id) \
         LEFT JOIN tbl_penyedia py ON py.id = dr.penyedia_id \
         WHERE 1 = 1{}",
        restriction.sql
    );
    let mut binds: Vec<String> = Vec::new();
    let user_binds = restriction.binds.clone();
    if let Some(t) = tahun {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM tbl_kegiatan kg WHERE kg.id = p.kegiatan_id AND kg.tahun_anggaran = ?)",
        );
        binds.push(t.to_string());
    }
    let search_pattern = search.map(|s| format!("%{s}%"));
    if let Some(pattern) = &search_pattern {
        sql.push_str(" AND (p.nama_paket LIKE ? OR p.kode_rekening LIKE ?)");
        binds.push(pattern.clone());
        binds.push(pattern.clone());
    }
    sql.push_str(" ORDER BY p.id");

    // Urutan bind: restriction (di WHERE), lalu tahun, lalu search.
    let mut query = sqlx::query(&sql);
    for id in user_binds {
        query = query.bind(id);
    }
    for value in binds {
        query = query.bind(value);
    }
    let rows = query.fetch_all(pool).await.map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(ExportRow {
                nama_paket: r.try_get("nama_paket").map_err(internal)?,
                kode_rekening: r.try_get("kode_rekening").map_err(internal)?,
                pagu: r.try_get("pagu").map_err(internal)?,
                kode_rup: r.try_get("kode_rup").map_err(internal)?,
                kode_paket: r.try_get("kode_paket").map_err(internal)?,
                nama_pelaksana: r.try_get("nama_pelaksana").map_err(internal)?,
                penyedia_nama: r.try_get("penyedia_nama").map_err(internal)?,
            })
        })
        .collect()
}

fn text_cell(ws: &mut Worksheet, row: u32, col: u16, value: Option<&str>) -> Result<(), ApiError> {
    match value {
        Some(v) if !v.is_empty() => ws.write_string(row, col, v).map_err(internal)?,
        _ => ws.write_blank(row, col, &Format::new()).map_err(internal)?,
    };
    Ok(())
}

/// `GET /api/draft-pekerjaan/export/excel`.
pub async fn export_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let tahun = truthy(query.get("tahun"));
    let search = truthy(query.get("search"));
    let rows = export_rows(&state.pool, user.user_id, &roles, tahun, search).await?;

    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let ws = wb.add_worksheet();
    for (col, heading) in HEADINGS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *heading, &bold)
            .map_err(internal)?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        text_cell(ws, row, 0, r.nama_paket.as_deref())?;
        text_cell(ws, row, 1, r.kode_rekening.as_deref())?;
        // Kecamatan dan Desa: null di Laravel (atribut tidak ada), sengaja dikosongkan.
        text_cell(ws, row, 2, None)?;
        text_cell(ws, row, 3, None)?;
        match r.pagu {
            Some(pagu) => ws.write_number(row, 4, pagu).map_err(internal)?,
            None => ws.write_blank(row, 4, &Format::new()).map_err(internal)?,
        };
        text_cell(ws, row, 5, r.kode_rup.as_deref())?;
        text_cell(ws, row, 6, r.kode_paket.as_deref())?;
        text_cell(ws, row, 7, r.nama_pelaksana.as_deref())?;
        text_cell(ws, row, 8, r.penyedia_nama.as_deref())?;
    }
    ws.autofit();
    let bytes = wb.save_to_buffer().map_err(internal)?;

    Ok((
        [
            (header::CONTENT_TYPE, XLSX_MIME.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{FILE_NAME}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
