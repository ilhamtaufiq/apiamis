//! `POST /api/pekerjaan/import` dan `GET /api/pekerjaan/import/template`, setara `PekerjaanController@import`,
//! `downloadTemplate`, `PekerjaanImport`, dan `PekerjaanTemplateExport`.
//!
//! Aturan impor mengikuti Laravel: baris pertama adalah heading, `nama_paket`, `kecamatan`, dan `desa` wajib,
//! kecamatan dan desa dicocokkan dengan `LIKE '%nama%'`, dan baris yang gagal validasi dilewati sementara baris
//! lain tetap tersimpan. Jika ada baris gagal, respon 422 dengan 10 pesan pertama, seperti Laravel.
//!
//! Berbeda dari Laravel:
//! - Impor memakai `Pekerjaan::withoutEvents` di Laravel, sehingga tidak ada audit per baris. Notifikasi
//!   ringkasan ke admin tetap dikirim setelah sukses.
//! - Berkas di atas batas body default axum (2 MB) ditolak, sama dengan impor kontrak.
//! - Tidak ada pekerjaan yang dibuat ulang bila sebagian baris gagal. Itu perilaku yang sama dengan Laravel.

use std::collections::BTreeMap;

use axum::{
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::json;
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    foto,
    kontrak_xlsx::{cell_text, php_float_prefix, read_rows, xlsx_response, Cell, ImportRow},
    require_auth, AppState,
};

const ALLOWED_EXT: [&str; 3] = ["xlsx", "xls", "csv"];
const TEMPLATE_FILE: &str = "template_import_pekerjaan.xlsx";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Pesan validasi satu baris, dalam urutan aturan Laravel. Kosong berarti baris valid.
///
/// Sel kosong yang ada headingnya memicu `required` dan `string`, seperti `null` di Laravel.
/// Heading yang tidak ada, atau teks kosong setelah dipangkas, hanya memicu `required`.
fn row_errors(row: &ImportRow) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["nama_paket", "kecamatan", "desa"] {
        let attr = key.replace('_', " ");
        match row.values.get(key) {
            None => out.push(format!("The {attr} field is required.")),
            Some(Cell::Empty) => {
                out.push(format!("The {attr} field is required."));
                out.push(format!("The {attr} must be a string."));
            }
            Some(Cell::Text(t)) if t.trim().is_empty() => {
                out.push(format!("The {attr} field is required."));
            }
            Some(Cell::Text(_)) => {}
            Some(Cell::Number(_)) => out.push(format!("The {attr} must be a string.")),
        }
    }
    out
}

/// Teks dengan spasi di ujung dibuang (`prepareForValidation`). Kosong menjadi `None`.
fn trimmed(row: &ImportRow, key: &str) -> Option<String> {
    row.values
        .get(key)
        .and_then(cell_text)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Nilai PHP yang truthy untuk `$row['tahun']`, `$kegiatanName`, dan sejenisnya.
fn php_truthy(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty() && *v != "0")
}

/// `parsePagu`: angka dipakai langsung. Teks lain dibersihkan dengan koma menjadi titik, lalu hanya angka dan titik
/// yang dipertahankan, dan dibaca sebagai awalan angka seperti `(float)` PHP.
fn parse_pagu(cell: Option<&Cell>) -> f64 {
    match cell {
        None | Some(Cell::Empty) => 0.0,
        Some(Cell::Number(n)) => *n,
        Some(Cell::Text(s)) => {
            let t = s.trim();
            if is_numeric(t) {
                return t.parse::<f64>().unwrap_or(0.0);
            }
            let cleaned: String = s
                .replace(',', ".")
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            php_float_prefix(&cleaned)
        }
    }
}

/// `is_numeric` PHP untuk teks sederhana (tanda, digit, titik, dan eksponen).
fn is_numeric(t: &str) -> bool {
    !t.is_empty()
        && t.chars().any(|c| c.is_ascii_digit())
        && t.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
        && t.parse::<f64>().map(|f| f.is_finite()).unwrap_or(false)
}

/// Jumlah percobaan untuk satu penulisan bila terjadi deadlock InnoDB (SQLSTATE 40001).
const WRITE_ATTEMPTS: u64 = 5;

/// Jalankan penulisan yang diulang bila kalah deadlock, dengan jeda singkat.
async fn retry_deadlock<T, F, Fut>(mut f: F) -> Result<T, sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, sqlx::Error>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match f().await {
            Err(sqlx::Error::Database(e))
                if e.code().as_deref() == Some("40001") && attempt < WRITE_ATTEMPTS =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(10 * attempt)).await;
            }
            other => return other,
        }
    }
}

/// Kecamatan pertama yang namanya mengandung teks (`where LIKE ... first()`).
async fn find_kecamatan(pool: &MySqlPool, name: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_kecamatan WHERE n_kec LIKE ? ORDER BY id LIMIT 1",
    )
    .bind(format!("%{name}%"))
    .fetch_optional(pool)
    .await
}

async fn find_desa(
    pool: &MySqlPool,
    kecamatan_id: i64,
    name: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_desa WHERE kecamatan_id = ? AND n_desa LIKE ? ORDER BY id LIMIT 1",
    )
    .bind(kecamatan_id)
    .bind(format!("%{name}%"))
    .fetch_optional(pool)
    .await
}

async fn find_kegiatan(
    pool: &MySqlPool,
    tahun: &str,
    name: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_kegiatan WHERE tahun_anggaran = ? AND nama_sub_kegiatan LIKE ? ORDER BY id LIMIT 1",
    )
    .bind(tahun)
    .bind(format!("%{name}%"))
    .fetch_optional(pool)
    .await
}

/// Satu baris valid: resolusi kecamatan, desa, dan kegiatan, lalu simpan. Tanpa audit (`withoutEvents`).
async fn insert_row(pool: &MySqlPool, row: &ImportRow) -> Result<(), sqlx::Error> {
    let kecamatan_name = trimmed(row, "kecamatan");
    let kecamatan_id = match php_truthy(&kecamatan_name) {
        Some(n) => find_kecamatan(pool, n).await?,
        None => None,
    };
    let desa_name = trimmed(row, "desa");
    let desa_id = match (kecamatan_id, php_truthy(&desa_name)) {
        (Some(k), Some(n)) => find_desa(pool, k, n).await?,
        _ => None,
    };
    let kegiatan_name = trimmed(row, "kegiatan");
    let tahun = trimmed(row, "tahun");
    let kegiatan_id = match (php_truthy(&tahun), php_truthy(&kegiatan_name)) {
        (Some(t), Some(n)) => find_kegiatan(pool, t, n).await?,
        _ => None,
    };
    let pagu = parse_pagu(row.values.get("pagu"));

    sqlx::query(
        "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(trimmed(row, "kode_rekening"))
    .bind(trimmed(row, "nama_paket").unwrap_or_default())
    .bind(kecamatan_id)
    .bind(desa_id)
    .bind(kegiatan_id)
    .bind(pagu)
    .execute(pool)
    .await?;
    Ok(())
}

/// Nama pengguna untuk pesan notifikasi, atau `System` bila tidak ada.
async fn actor_name(pool: &MySqlPool, user_id: u64) -> Result<String, sqlx::Error> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    Ok(name.unwrap_or_else(|| "System".to_string()))
}

/// `POST /api/pekerjaan/import` (multipart, field `file`).
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

    let rows = match read_rows(&bytes, &ext) {
        Ok((_, rows)) => rows,
        Err(e) => return Ok(import_failed(&e)),
    };

    let mut failures: Vec<(u64, Vec<String>)> = Vec::new();
    for row in &rows {
        let errs = row_errors(row);
        if !errs.is_empty() {
            failures.push((row.row_number, errs));
            continue;
        }
        if let Err(e) = retry_deadlock(|| insert_row(&state.pool, row)).await {
            return Ok(import_failed(&e.to_string()));
        }
    }

    if !failures.is_empty() {
        let messages: Vec<String> = failures
            .iter()
            .take(10)
            .map(|(n, errs)| format!("Baris {n}: {}", errs.join(", ")))
            .collect();
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "message": "Import selesai dengan beberapa error",
                "errors": messages,
                "error_count": failures.len(),
            })),
        )
            .into_response());
    }

    notify_success(&state, user.user_id).await?;
    Ok(Json(json!({ "message": "Data pekerjaan berhasil diimport" })).into_response())
}

/// Respon 500 untuk kegagalan di luar validasi, sama dengan `try/catch` di Laravel.
fn import_failed(detail: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "message": format!("Gagal mengimport data: {detail}") })),
    )
        .into_response()
}

/// Notifikasi `Import Pekerjaan Berhasil` ke semua admin kecuali pengimpor.
async fn notify_success(state: &AppState, actor: u64) -> Result<(), ApiError> {
    let name = actor_name(&state.pool, actor).await.map_err(internal)?;
    let message = format!("Sejumlah data pekerjaan telah berhasil diimport oleh {name}.");
    retry_deadlock(|| async {
        let mut tx = state.pool.begin().await?;
        let admins: Vec<u64> = crate::notify::admin_ids(&mut tx)
            .await?
            .into_iter()
            .filter(|id| *id != actor)
            .collect();
        crate::notify::to_users(
            &mut tx,
            &admins,
            "Import Pekerjaan Berhasil",
            &message,
            Some("/pekerjaan"),
            "success",
        )
        .await?;
        tx.commit().await
    })
    .await
    .map_err(internal)
}

/// `GET /api/pekerjaan/import/template`: sheet Template (heading), Ref Kecamatan & Desa, dan Ref Kegiatan.
pub async fn download_template(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let pool = &state.pool;

    let kec_desa: Vec<(String, Option<String>)> = sqlx::query(
        "SELECT k.n_kec, d.n_desa FROM tbl_kecamatan k \
         JOIN tbl_desa d ON d.kecamatan_id = k.id ORDER BY k.id, d.id",
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| -> Result<(String, Option<String>), sqlx::Error> {
        Ok((
            r.try_get::<Option<String>, _>("n_kec")?.unwrap_or_default(),
            r.try_get("n_desa")?,
        ))
    })
    .collect::<Result<_, _>>()
    .map_err(internal)?;

    let kegiatan: Vec<(Option<String>, Option<String>)> =
        sqlx::query("SELECT tahun_anggaran, nama_sub_kegiatan FROM tbl_kegiatan ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(internal)?
            .iter()
            .map(
                |r| -> Result<(Option<String>, Option<String>), sqlx::Error> {
                    Ok((
                        r.try_get("tahun_anggaran")?,
                        r.try_get("nama_sub_kegiatan")?,
                    ))
                },
            )
            .collect::<Result<_, _>>()
            .map_err(internal)?;

    let bold = Format::new().set_bold();
    let mut wb = Workbook::new();

    let ws = wb.add_worksheet().set_name("Template").map_err(internal)?;
    for (col, h) in [
        "Kode Rekening",
        "Nama Paket",
        "Kecamatan",
        "Desa",
        "Kegiatan",
        "Tahun",
        "Pagu",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string_with_format(0, col as u16, *h, &bold)
            .map_err(internal)?;
    }

    let ws = wb
        .add_worksheet()
        .set_name("Ref Kecamatan & Desa")
        .map_err(internal)?;
    ws.write_string_with_format(0, 0, "Nama Kecamatan", &bold)
        .map_err(internal)?;
    ws.write_string_with_format(0, 1, "Nama Desa", &bold)
        .map_err(internal)?;
    for (i, (kec, desa)) in kec_desa.iter().enumerate() {
        let row = (i + 1) as u32;
        ws.write_string(row, 0, kec).map_err(internal)?;
        if let Some(d) = desa {
            ws.write_string(row, 1, d).map_err(internal)?;
        }
    }

    let ws = wb
        .add_worksheet()
        .set_name("Ref Kegiatan")
        .map_err(internal)?;
    ws.write_string_with_format(0, 0, "Tahun Anggaran", &bold)
        .map_err(internal)?;
    ws.write_string_with_format(0, 1, "Nama Sub Kegiatan", &bold)
        .map_err(internal)?;
    for (i, (tahun, nama)) in kegiatan.iter().enumerate() {
        let row = (i + 1) as u32;
        if let Some(t) = tahun {
            ws.write_string(row, 0, t).map_err(internal)?;
        }
        if let Some(n) = nama {
            ws.write_string(row, 1, n).map_err(internal)?;
        }
    }

    let bytes = wb.save_to_buffer().map_err(internal)?;
    Ok(xlsx_response(bytes, TEMPLATE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagu_accepts_numbers_and_currency_text() {
        assert_eq!(parse_pagu(Some(&Cell::Number(1500.5))), 1500.5);
        assert_eq!(parse_pagu(Some(&Cell::Text("2500000".into()))), 2_500_000.0);
        assert_eq!(parse_pagu(Some(&Cell::Text("Rp 1.250,50".into()))), 1.25);
        assert_eq!(parse_pagu(None), 0.0);
        assert_eq!(parse_pagu(Some(&Cell::Empty)), 0.0);
    }

    #[test]
    fn row_errors_follow_laravel_rules_order() {
        let mut values = BTreeMap::new();
        values.insert("nama_paket".to_string(), Cell::Text("  ".into()));
        values.insert("kecamatan".to_string(), Cell::Number(12.0));
        let row = ImportRow {
            row_number: 4,
            values,
        };
        assert_eq!(
            row_errors(&row),
            vec![
                "The nama paket field is required.".to_string(),
                "The kecamatan must be a string.".to_string(),
                "The desa field is required.".to_string(),
            ]
        );
    }
}
