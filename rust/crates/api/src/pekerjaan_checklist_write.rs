//! Centang checklist pekerjaan (`PekerjaanChecklistController::toggle`) dan ekspor Excel
//! (`exportExcel`, `PekerjaanChecklistExport`). Baca dan riwayat ada di `checklist.rs`.
//!
//! Ekspor PDF ada di `pekerjaan_checklist_pdf.rs`; tabelnya memakai `checklist_table` di sini.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use rust_xlsxwriter::{Format, Workbook};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    checklist::{checks_for, items_in_context, pekerjaan_page, require_full_access, user_name, PekerjaanFilter},
    require_auth,
    validation::Errors,
    AppState,
};

const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `boolean` Laravel: bool, 0/1, atau teks "0"/"1"/"true"/"false".
fn boolean(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// `exists:tabel,id` untuk id bilangan bulat. `false` bila tidak ada atau bukan angka.
async fn exists(pool: &MySqlPool, table: &str, id: &Value) -> Result<bool, ApiError> {
    let Some(id) = id.as_i64().or_else(|| id.as_str().and_then(|s| s.trim().parse().ok())) else {
        return Ok(false);
    };
    // Nama tabel hanya berasal dari konstanta di modul ini.
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// `POST /api/pekerjaan-checklist/toggle`: simpan centang, catat riwayat, dan balas status terbaru.
pub async fn toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let pekerjaan = match input.get("pekerjaan_id") {
        None | Some(Value::Null) => {
            e.add("pekerjaan_id", "The pekerjaan id field is required.");
            None
        }
        Some(v) => {
            if exists(&state.pool, "tbl_pekerjaan", v).await? {
                v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            } else {
                e.add("pekerjaan_id", "The selected pekerjaan id is invalid.");
                None
            }
        }
    };
    let item = match input.get("checklist_item_id") {
        None | Some(Value::Null) => {
            e.add("checklist_item_id", "The checklist item id field is required.");
            None
        }
        Some(v) => {
            if exists(&state.pool, "tbl_checklist_items", v).await? {
                v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            } else {
                e.add("checklist_item_id", "The selected checklist item id is invalid.");
                None
            }
        }
    };
    let checked = match input.get("is_checked") {
        None | Some(Value::Null) => {
            e.add("is_checked", "The is checked field is required.");
            None
        }
        Some(v) => match boolean(v) {
            Some(b) => Some(b),
            None => {
                e.add("is_checked", "The is checked field must be true or false.");
                None
            }
        },
    };
    let notes = match input.get("notes") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            e.add("notes", "The notes field must be a string.");
            None
        }
    };
    e.finish()?;
    let (Some(pekerjaan), Some(item), Some(checked)) = (pekerjaan, item, checked) else {
        return Err(internal("validasi tidak lengkap"));
    };

    // Satu waktu untuk semua kolom, seperti `now()` di Laravel.
    let now: String = sqlx::query_scalar("SELECT DATE_FORMAT(NOW(), '%Y-%m-%d %H:%i:%s')")
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let existing: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT CAST(id AS SIGNED), CAST(checked_at AS CHAR) FROM pekerjaan_checklist \
         WHERE pekerjaan_id = ? AND checklist_item_id = ?",
    )
    .bind(pekerjaan)
    .bind(item)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let uid = user.user_id as i64;
    let checked_flag = if checked { 1i8 } else { 0 };
    match existing {
        Some((row_id, prev_checked_at)) => {
            // `checked_at` ikut waktu centang terakhir: diganti hanya saat dicentang.
            let checked_at = if checked { Some(now.clone()) } else { prev_checked_at };
            sqlx::query(
                "UPDATE pekerjaan_checklist SET is_checked = ?, checked_at = ?, checked_by = ?, notes = ?, updated_at = ? \
                 WHERE id = ?",
            )
            .bind(checked_flag)
            .bind(checked_at)
            .bind(uid)
            .bind(&notes)
            .bind(&now)
            .bind(row_id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
        }
        None => {
            let checked_at = if checked { Some(now.clone()) } else { None };
            sqlx::query(
                "INSERT INTO pekerjaan_checklist (is_checked, checked_at, checked_by, notes, updated_at, pekerjaan_id, checklist_item_id, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(checked_flag)
            .bind(checked_at)
            .bind(uid)
            .bind(&notes)
            .bind(&now)
            .bind(pekerjaan)
            .bind(item)
            .bind(&now)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
        }
    }

    sqlx::query(
        "INSERT INTO pekerjaan_checklist_histories (pekerjaan_id, checklist_item_id, is_checked, notes, user_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(pekerjaan)
    .bind(item)
    .bind(checked_flag)
    .bind(&notes)
    .bind(uid)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let name = user_name(&state.pool, user.user_id).await.map_err(internal)?;
    Ok(Json(json!({
        "message": "Checklist updated",
        "is_checked": checked,
        "checked_by": user.user_id,
        "checked_by_name": name,
        "updated_at": now,
    }))
    .into_response())
}

/// Format `date('d/m/Y H:i', strtotime($teks))`. Teks yang tidak terbaca dikembalikan apa adanya.
fn fmt_dt(raw: &str) -> String {
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .map(|d| d.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_else(|_| raw.to_string())
}

/// Tabel ekspor checklist (`PekerjaanChecklistExport`: `headings` dan `map`). Sel sudah berupa teks,
/// dipakai bersama oleh ekspor Excel dan PDF.
pub(crate) struct ChecklistTable {
    pub headings: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Konteks checklist: pekerjaan biasa, atau post pekerjaan (hanya pekerjaan yang sudah berkontrak).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Konteks {
    Pekerjaan,
    PostPekerjaan,
}

impl Konteks {
    /// Nilai `context` pada `checklist_items`.
    pub(crate) fn nama(self) -> &'static str {
        match self {
            Konteks::Pekerjaan => "pekerjaan",
            Konteks::PostPekerjaan => "post_pekerjaan",
        }
    }
}

/// Satu baris pekerjaan untuk tabel ekspor.
struct BarisEkspor {
    id: u64,
    nama_paket: String,
    kegiatan: String,
}

/// Pekerjaan post pekerjaan yang cocok dengan filter, tanpa paginasi. Filter sama dengan `post_index`.
async fn post_pekerjaan_rows(
    pool: &MySqlPool,
    query: &HashMap<String, String>,
) -> Result<Vec<BarisEkspor>, ApiError> {
    let filter_tahun = query.get("tahun").filter(|v| !v.is_empty()).cloned();
    let filter_keg = query.get("kegiatan_id").filter(|v| !v.is_empty()).cloned();
    let filter_search = query.get("search").filter(|v| !v.is_empty()).cloned();

    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT p.id, p.nama_paket, k.nama_sub_kegiatan FROM tbl_pekerjaan p \
         LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id \
         WHERE EXISTS (SELECT 1 FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = p.id)",
    );
    if let Some(t) = &filter_tahun {
        qb.push(" AND k.tahun_anggaran = ").push_bind(t.clone());
    }
    if let Some(g) = &filter_keg {
        qb.push(" AND p.kegiatan_id = ").push_bind(g.clone());
    }
    if let Some(s) = &filter_search {
        let like = format!("%{s}%");
        qb.push(" AND (p.nama_paket LIKE ")
            .push_bind(like.clone())
            .push(" OR EXISTS (SELECT 1 FROM kontrak_pekerjaan kp2 JOIN tbl_kontrak kt ON kt.id = kp2.kontrak_id WHERE kp2.pekerjaan_id = p.id AND (kt.nomor_penawaran LIKE ")
            .push_bind(like.clone())
            .push(" OR kt.spk LIKE ")
            .push_bind(like.clone())
            .push(" OR kt.kode_paket LIKE ")
            .push_bind(like)
            .push(")))");
    }
    qb.push(" ORDER BY p.id");

    let rows = qb.build().fetch_all(pool).await.map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(BarisEkspor {
                id: r.try_get("id").map_err(internal)?,
                nama_paket: r
                    .try_get::<Option<String>, _>("nama_paket")
                    .map_err(internal)?
                    .unwrap_or_default(),
                kegiatan: r
                    .try_get::<Option<String>, _>("nama_sub_kegiatan")
                    .map_err(internal)?
                    .unwrap_or_else(|| "-".into()),
            })
        })
        .collect()
}

/// Pekerjaan pekerjaan biasa yang cocok dengan filter, dibaca per halaman besar.
async fn pekerjaan_rows(
    pool: &MySqlPool,
    query: &HashMap<String, String>,
) -> Result<Vec<BarisEkspor>, ApiError> {
    let filter = PekerjaanFilter::from_query(query);
    let mut hasil = Vec::new();
    let mut page = 1u64;
    loop {
        let (rows, total) = pekerjaan_page(pool, &filter, page, 1000)
            .await
            .map_err(internal)?;
        let got = rows.len() as u64;
        hasil.extend(rows.into_iter().map(|p| BarisEkspor {
            id: p.id,
            nama_paket: p.nama_paket.unwrap_or_default(),
            kegiatan: p
                .kegiatan
                .and_then(|(_, n)| n)
                .unwrap_or_else(|| "-".into()),
        }));
        if got == 0 || hasil.len() as u64 >= total {
            break;
        }
        page += 1;
    }
    Ok(hasil)
}

/// Susun tabel checklist dengan filter sama dengan daftar (`tahun`, `kegiatan_id`, `search`).
pub(crate) async fn checklist_table(
    pool: &MySqlPool,
    query: &HashMap<String, String>,
    konteks: Konteks,
) -> Result<ChecklistTable, ApiError> {
    let columns = items_in_context(pool, konteks.nama()).await.map_err(internal)?;
    let baris = match konteks {
        Konteks::Pekerjaan => pekerjaan_rows(pool, query).await?,
        Konteks::PostPekerjaan => post_pekerjaan_rows(pool, query).await?,
    };

    let mut headings: Vec<String> = vec!["No".into(), "Nama Paket".into(), "Kegiatan".into()];
    headings.extend(columns.iter().map(|c| c.name.clone()));
    headings.push("Tanggal Update Terakhir".into());
    headings.push("Diubah Oleh".into());

    let mut rows = Vec::with_capacity(baris.len());
    for (i, p) in baris.iter().enumerate() {
        let checks = checks_for(pool, p.id).await.map_err(internal)?;
        // Baris terbaru: `sortByDesc(updated_at ?? checked_at)->first()`.
        let latest = checks
            .iter()
            .max_by_key(|c| c.updated_at.clone().or_else(|| c.checked_at.clone()));
        let mut row = vec![
            (i + 1).to_string(),
            p.nama_paket.clone(),
            p.kegiatan.clone(),
        ];

        for item in &columns {
            let data = checks.iter().find(|c| c.item_id == item.id);
            let text = match data {
                Some(d) if d.is_checked => {
                    let at = d.updated_at.clone().or_else(|| d.checked_at.clone());
                    let by = match d.checked_by {
                        Some(uid) => user_name(pool, uid).await.map_err(internal)?.unwrap_or_else(|| "-".into()),
                        None => "-".into(),
                    };
                    match at {
                        Some(at) => {
                            let suffix = if by != "-" { format!(" · {by}") } else { String::new() };
                            format!("Ya ({}{suffix})", fmt_dt(&at))
                        }
                        None => "Ya".into(),
                    }
                }
                _ => "Tidak".into(),
            };
            row.push(text);
        }

        let at = latest.and_then(|c| c.updated_at.clone().or_else(|| c.checked_at.clone()));
        row.push(at.as_deref().map(fmt_dt).unwrap_or_else(|| "-".into()));
        let diubah = match latest.and_then(|c| c.checked_by) {
            Some(uid) => user_name(pool, uid).await.map_err(internal)?.unwrap_or_else(|| "-".into()),
            None => "-".into(),
        };
        row.push(diubah);
        rows.push(row);
    }

    Ok(ChecklistTable { headings, rows })
}

/// Berkas Excel dari tabel checklist: baris judul di baris pertama, lalu data.
pub(crate) fn excel_bytes(table: &ChecklistTable) -> Result<Vec<u8>, ApiError> {
    let bold = Format::new().set_bold();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    for (col, h) in table.headings.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, h.as_str(), &bold)
            .map_err(internal)?;
    }
    for (i, row) in table.rows.iter().enumerate() {
        let r = (i + 1) as u32;
        for (col, cell) in row.iter().enumerate() {
            if col == 0 {
                ws.write_number(r, 0, (i + 1) as f64).map_err(internal)?;
            } else {
                ws.write_string(r, col as u16, cell.as_str()).map_err(internal)?;
            }
        }
    }
    wb.save_to_buffer().map_err(internal)
}

/// Respons unduhan Excel dengan nama berkas `{prefix}_{waktu}.xlsx`.
fn lampiran_excel(prefix: &str, bytes: Vec<u8>) -> Response {
    let filename = format!("{prefix}_{}.xlsx", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
    (
        [
            (header::CONTENT_TYPE, XLSX_MIME.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        bytes,
    )
        .into_response()
}

/// `GET /api/pekerjaan-checklist/export/excel`: filter sama dengan daftar (`tahun`, `kegiatan_id`, `search`).
pub async fn export_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    require_full_access(&state, &headers).await?;
    let table = checklist_table(&state.pool, &query, Konteks::Pekerjaan).await?;
    Ok(lampiran_excel("checklist_pekerjaan", excel_bytes(&table)?))
}

/// `GET /api/post-pekerjaan-checklist/export/excel`: sama seperti ekspor pekerjaan, untuk post pekerjaan.
pub async fn export_post_excel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    require_full_access(&state, &headers).await?;
    let table = checklist_table(&state.pool, &query, Konteks::PostPekerjaan).await?;
    Ok(lampiran_excel("post_pekerjaan", excel_bytes(&table)?))
}
