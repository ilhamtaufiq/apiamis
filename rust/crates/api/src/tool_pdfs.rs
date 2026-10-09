//! `/api/tool-pdfs`: port `ToolPdfController` (index, store, sign, bulkDownload, download, destroy),
//! `ToolPdfResource`, dan model `ToolPdf` (soft delete, audit `created`/`deleted`).
//!
//! Sign tidak membuat PDF baru: berkas yang diunggah disimpan apa adanya, lalu baris placement tanda tangan
//! ditulis. Tidak ada mesin HTML-ke-PDF. Bulk download menyusun zip dari berkas sumber (`zip` crate).
//!
//! Akses: index hanya milik user sendiri, termasuk untuk admin. Download dan destroy memakai `canManage`
//! (pemilik atau admin). Gate `route_permission::check` tidak diubah: `/tool-pdfs` ada di
//! `MUTATION_RESOURCE_PREFIXES`, jadi mutasi dari non-admin tetap lolos seperti di Laravel.
//!
//! Deviasi (dicatat, bukan diputuskan ulang):
//! - `mimes:pdf` di Laravel memeriksa tipe MIME dari isi berkas. Di sini: ekstensi `.pdf` dan awalan `%PDF-`.
//! - `placements` berupa objek JSON ditolak dengan pesan format yang sama dengan array yang tidak valid.
//!   Di PHP, objek lolos `is_array()` dan item-nya divalidasi. Perilaku itu belum diverifikasi.
//! - `ids` pada bulk download diurutkan `id`. Laravel tidak memberi `ORDER BY` pada `whereIn`.
//! - Slug nama berkas di bulk download hanya menangani ASCII. `Str::slug` juga men-transliterasi
//!   karakter non-ASCII.
//! - `created_at`, `updated_at`, dan `deleted_at` memakai bentuk Carbon UTC (`carbon_json`).
//! - Audit `deleted` memuat atribut sebelum hapus, ditambah `deleted_at` dan `updated_at` seperti
//!   `runSoftDelete()`. Perilaku pasti belum diverifikasi tanpa `vendor/`.
//! - Rute tulis memakai batas body `BODY_LIMIT` (50 MB plus ruang multipart). Batas global default 10 MB.

use std::{
    collections::HashMap,
    io::{Cursor, Write},
};

use axum::{
    extract::{Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{
    changes,
    foto::{self, RawForm},
    lookup::carbon_json,
    media::{self, internal, Upload},
    require_auth, AppState,
};

const MODEL: &str = "App\\Models\\ToolPdf";
const COLLECTION: &str = "pdf";
const VALIDATION_MESSAGE: &str = "The given data was invalid.";
const FORMAT_MESSAGE: &str = "Format placement tanda tangan tidak valid";
/// Batas body untuk store dan sign: `max:51200` (kilobyte) plus ruang untuk field multipart.
pub const BODY_LIMIT: usize = media::MAX_FILE_BYTES + 1024 * 1024;
/// Batas body JSON untuk bulk download (hanya daftar id).
const JSON_LIMIT: usize = 1024 * 1024;

static NULL: Value = Value::Null;

const SELECT_PDF: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, \
     CAST(parent_id AS SIGNED) AS parent_id, name, original_filename, kind, created_at, updated_at, deleted_at \
     FROM tool_pdfs";

const SELECT_PLACEMENT: &str = "SELECT CAST(id AS SIGNED) AS id, signature_id, \
     CAST(page_number AS SIGNED) AS page_number, CAST(x_ratio AS DOUBLE) AS x_ratio, \
     CAST(y_ratio AS DOUBLE) AS y_ratio, CAST(scale AS DOUBLE) AS scale, \
     CAST(sort_order AS SIGNED) AS sort_order, signature_name, signature_file_name, signature_mime_type, \
     CAST(signature_width AS SIGNED) AS signature_width, CAST(signature_height AS SIGNED) AS signature_height, \
     signature_data_url, CAST(signature_source_type AS CHAR) AS signature_source_type, signature_source_id \
     FROM tool_pdf_signature_placements WHERE tool_pdf_id = ? ORDER BY id";

/// Baris `tool_pdfs`. `deleted_at` ikut dibaca supaya audit `deleted` bisa memakainya.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolPdfRow {
    pub id: i64,
    pub user_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub original_filename: Option<String>,
    pub kind: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

fn map_pdf(r: &sqlx::mysql::MySqlRow) -> Result<ToolPdfRow, sqlx::Error> {
    Ok(ToolPdfRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        original_filename: r.try_get("original_filename")?,
        kind: r.try_get("kind")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        deleted_at: r.try_get("deleted_at")?,
    })
}

/// Baris `tool_pdf_signature_placements`. `x_ratio`, `y_ratio`, dan `scale` sudah di-cast ke DOUBLE.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementRow {
    pub id: i64,
    pub signature_id: String,
    pub page_number: i64,
    pub x_ratio: f64,
    pub y_ratio: f64,
    pub scale: f64,
    pub sort_order: i64,
    pub signature_name: String,
    pub signature_file_name: String,
    pub signature_mime_type: String,
    pub signature_width: i64,
    pub signature_height: i64,
    pub signature_data_url: Option<String>,
    pub signature_source_type: Option<String>,
    pub signature_source_id: Option<String>,
}

fn map_placement(r: &sqlx::mysql::MySqlRow) -> Result<PlacementRow, sqlx::Error> {
    Ok(PlacementRow {
        id: r.try_get("id")?,
        signature_id: r.try_get("signature_id")?,
        page_number: r.try_get("page_number")?,
        x_ratio: r.try_get("x_ratio")?,
        y_ratio: r.try_get("y_ratio")?,
        scale: r.try_get("scale")?,
        sort_order: r.try_get("sort_order")?,
        signature_name: r.try_get("signature_name")?,
        signature_file_name: r.try_get("signature_file_name")?,
        signature_mime_type: r.try_get("signature_mime_type")?,
        signature_width: r.try_get("signature_width")?,
        signature_height: r.try_get("signature_height")?,
        signature_data_url: r.try_get("signature_data_url")?,
        signature_source_type: r.try_get("signature_source_type")?,
        signature_source_id: r.try_get("signature_source_id")?,
    })
}

/// Cari satu baris. `alive = true` mengikuti global scope `SoftDeletes` (baris terhapus dianggap tidak ada).
async fn find_pdf<'e, E>(exec: E, id: i64, alive: bool) -> Result<Option<ToolPdfRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = if alive {
        format!("{SELECT_PDF} WHERE id = ? AND deleted_at IS NULL")
    } else {
        format!("{SELECT_PDF} WHERE id = ?")
    };
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_pdf).transpose()
}

/// `ToolPdf::ownedBy($user)->findOrFail($id)`: milik user dan belum terhapus.
async fn find_owned(pool: &MySqlPool, id: i64, user_id: u64) -> Result<Option<ToolPdfRow>, ApiError> {
    let sql = format!("{SELECT_PDF} WHERE id = ? AND user_id = ? AND deleted_at IS NULL");
    let row = sqlx::query(&sql)
        .bind(id)
        .bind(user_id as i64)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(map_pdf).transpose().map_err(internal)
}

/// `whereIn('id', $ids)` pada milik user, belum terhapus, urut `id`.
async fn find_owned_many(pool: &MySqlPool, user_id: u64, ids: &[i64]) -> Result<Vec<ToolPdfRow>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "{SELECT_PDF} WHERE user_id = ? AND deleted_at IS NULL AND id IN ({placeholders}) ORDER BY id"
    );
    let mut q = sqlx::query(&sql).bind(user_id as i64);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    rows.iter()
        .map(map_pdf)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

/// `getAttributes()` untuk audit `created`.
fn attributes(row: &ToolPdfRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("user_id".into(), json!(row.user_id));
    m.insert("parent_id".into(), json!(row.parent_id));
    m.insert("name".into(), json!(row.name));
    m.insert("original_filename".into(), json!(row.original_filename));
    m.insert("kind".into(), json!(row.kind));
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

fn placement_json(p: &PlacementRow) -> Value {
    json!({
        "id": p.id.to_string(),
        "signature_id": p.signature_id,
        "page_number": p.page_number,
        "x_ratio": p.x_ratio,
        "y_ratio": p.y_ratio,
        "scale": p.scale,
        "sort_order": p.sort_order,
        "signature_name": p.signature_name,
        "signature_file_name": p.signature_file_name,
        "signature_mime_type": p.signature_mime_type,
        "signature_width": p.signature_width,
        "signature_height": p.signature_height,
        "signature_data_url": p.signature_data_url,
        "signature_source_type": p.signature_source_type,
        "signature_source_id": p.signature_source_id,
    })
}

async fn placements_of(pool: &MySqlPool, pdf_id: i64) -> Result<Vec<PlacementRow>, ApiError> {
    let rows = sqlx::query(SELECT_PLACEMENT)
        .bind(pdf_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter()
        .map(map_placement)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

/// `ToolPdfResource`. `signature_placements` selalu ada: Laravel selalu memuat relasinya.
async fn resource(pool: &MySqlPool, app_url: &str, row: &ToolPdfRow) -> Result<Value, ApiError> {
    let (pdf_url, _) = media::first_urls(pool, app_url, MODEL, row.id as u64, COLLECTION)
        .await
        .map_err(internal)?;
    let placements: Vec<Value> = placements_of(pool, row.id)
        .await?
        .iter()
        .map(placement_json)
        .collect();
    Ok(json!({
        "id": row.id.to_string(),
        "name": row.name,
        "original_filename": row.original_filename,
        "kind": row.kind,
        "parent_id": row.parent_id.map(|p| p.to_string()),
        "pdf_url": pdf_url,
        "signature_placements": placements,
        "created_at": carbon_json(row.created_at),
        "updated_at": carbon_json(row.updated_at),
    }))
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

/// Kumpulan error validasi per field. `into_error` memakai pesan `The given data was invalid.` (`validate()`).
#[derive(Default)]
struct Errs(std::collections::BTreeMap<String, Vec<String>>);

impl Errs {
    fn add(&mut self, key: &str, message: String) {
        self.0.entry(key.to_string()).or_default().push(message);
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn len(&self) -> usize {
        self.0.values().map(Vec::len).sum()
    }

    fn into_error(self) -> ApiError {
        ApiError::validation(VALIDATION_MESSAGE, self.0)
    }
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, format!("Permintaan tidak valid: {e}"))
}

/// Nilai teks dari form (sudah di-trim dan kosong dianggap null oleh `read_form`).
fn text_of(raw: &RawForm, key: &str) -> Option<String> {
    raw.fields.get(key).cloned().flatten()
}

/// `mimes:pdf`: ekstensi `.pdf` dan isi diawali `%PDF-`.
fn is_pdf(upload: &Upload) -> bool {
    upload.extension().eq_ignore_ascii_case("pdf") && upload.bytes.starts_with(b"%PDF-")
}

async fn owned_exists(pool: &MySqlPool, id: i64, user_id: u64) -> Result<bool, ApiError> {
    // `exists:tool_pdfs,id` dengan `where user_id`: tidak memfilter `deleted_at` (sama dengan Laravel).
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_pdfs WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id as i64)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// Input store dan sign setelah aturan `validate()` lolos.
struct PdfInput {
    upload: Upload,
    name: Option<String>,
    placements: Option<String>,
    source_id: Option<i64>,
}

/// Aturan: `file` (wajib, pdf, maks 51200 KB), `name` (nullable, maks 255), `placements` (nullable string),
/// dan untuk sign juga `source_id` (nullable, exists milik user).
async fn validate_input(
    pool: &MySqlPool,
    raw: &mut RawForm,
    user_id: u64,
    with_source: bool,
) -> Result<PdfInput, ApiError> {
    let mut errs = Errs::default();
    let upload = raw.file.take();
    match &upload {
        None => errs.add("file", "The file field is required.".to_string()),
        Some(u) => {
            if !is_pdf(u) {
                errs.add("file", "The file field must be a file of type: pdf.".to_string());
            }
            if u.bytes.len() > media::MAX_FILE_BYTES {
                errs.add(
                    "file",
                    "The file field must not be greater than 51200 kilobytes.".to_string(),
                );
            }
        }
    }
    let name = text_of(raw, "name");
    if name.as_deref().is_some_and(|n| n.chars().count() > 255) {
        errs.add("name", "The name field must not be greater than 255 characters.".to_string());
    }
    let placements = text_of(raw, "placements");
    let mut source_id = None;
    if with_source {
        if let Some(s) = text_of(raw, "source_id") {
            let parsed = s.parse::<i64>().ok();
            let owned = match parsed {
                Some(id) => owned_exists(pool, id, user_id).await?,
                None => false,
            };
            match (owned, parsed) {
                (true, Some(id)) => source_id = Some(id),
                _ => errs.add("source_id", "The selected source id is invalid.".to_string()),
            }
        }
    }
    let Some(upload) = upload.filter(|_| errs.is_empty()) else {
        return Err(errs.into_error());
    };
    Ok(PdfInput {
        upload,
        name,
        placements,
        source_id,
    })
}

/// Satu placement tanda tangan yang sudah lolos validasi.
struct PlacementInput {
    signature_id: String,
    page_number: i64,
    x_ratio: f64,
    y_ratio: f64,
    scale: f64,
    sort_order: Option<i64>,
    signature_name: String,
    signature_file_name: String,
    signature_mime_type: String,
    signature_width: i64,
    signature_height: i64,
    signature_data_url: Option<String>,
    signature_source_type: Option<String>,
    signature_source_id: Option<String>,
}

/// `raw_of(item, key)`: nilai field, atau null bila tidak ada (sama dengan `$placement['x'] ?? null`).
fn raw_of<'a>(item: &'a Value, key: &str) -> &'a Value {
    item.get(key).unwrap_or(&NULL)
}

fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15).map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn ph_string(errs: &mut Errs, i: usize, field: &str, item: &Value, required: bool, max: usize) -> Option<String> {
    let key = format!("placements.{i}.{field}");
    let attr = format!("placements.{i}.{}", field.replace('_', " "));
    match raw_of(item, field) {
        Value::Null => {
            if required {
                errs.add(&key, format!("The {attr} field is required."));
            }
            None
        }
        Value::String(s) if required && s.trim().is_empty() => {
            errs.add(&key, format!("The {attr} field is required."));
            None
        }
        Value::String(s) if s.chars().count() > max => {
            errs.add(
                &key,
                format!("The {attr} field must not be greater than {max} characters."),
            );
            None
        }
        Value::String(s) => Some(s.clone()),
        _ => {
            errs.add(&key, format!("The {attr} field must be a string."));
            None
        }
    }
}

fn ph_int(
    errs: &mut Errs,
    i: usize,
    field: &str,
    item: &Value,
    required: bool,
    min: i64,
    max: Option<i64>,
) -> Option<i64> {
    let key = format!("placements.{i}.{field}");
    let attr = format!("placements.{i}.{}", field.replace('_', " "));
    let v = raw_of(item, field);
    if v.is_null() {
        if required {
            errs.add(&key, format!("The {attr} field is required."));
        }
        return None;
    }
    if required && matches!(v, Value::String(s) if s.trim().is_empty()) {
        errs.add(&key, format!("The {attr} field is required."));
        return None;
    }
    let Some(n) = as_int(v) else {
        errs.add(&key, format!("The {attr} field must be an integer."));
        return None;
    };
    if n < min {
        errs.add(&key, format!("The {attr} field must be at least {min}."));
        return None;
    }
    if let Some(mx) = max {
        if n > mx {
            errs.add(&key, format!("The {attr} field must not be greater than {mx}."));
            return None;
        }
    }
    Some(n)
}

fn ph_num(errs: &mut Errs, i: usize, field: &str, item: &Value, min: f64, max: f64) -> Option<f64> {
    let key = format!("placements.{i}.{field}");
    let attr = format!("placements.{i}.{}", field.replace('_', " "));
    let v = raw_of(item, field);
    if v.is_null() {
        errs.add(&key, format!("The {attr} field is required."));
        return None;
    }
    if matches!(v, Value::String(s) if s.trim().is_empty()) {
        errs.add(&key, format!("The {attr} field is required."));
        return None;
    }
    let Some(n) = as_num(v) else {
        errs.add(&key, format!("The {attr} field must be a number."));
        return None;
    };
    if n < min {
        errs.add(&key, format!("The {attr} field must be at least {min}."));
        return None;
    }
    if n > max {
        errs.add(&key, format!("The {attr} field must not be greater than {max}."));
        return None;
    }
    Some(n)
}

/// `nullable|string|regex:/^data:image\/(png|jpe?g|webp);base64,/i`.
fn is_data_image_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("data:image/") else {
        return false;
    };
    ["png;base64,", "jpg;base64,", "jpeg;base64,", "webp;base64,"]
        .iter()
        .any(|p| rest.starts_with(*p))
}

fn ph_data_url(errs: &mut Errs, i: usize, item: &Value) -> Option<String> {
    let key = format!("placements.{i}.signature_data_url");
    match raw_of(item, "signature_data_url") {
        Value::Null => None,
        Value::String(s) if is_data_image_url(s) => Some(s.clone()),
        Value::String(_) => {
            errs.add(&key, format!("The placements.{i}.signature data url format is invalid."));
            None
        }
        _ => {
            errs.add(&key, format!("The placements.{i}.signature data url must be a string."));
            None
        }
    }
}

fn ph_source_type(errs: &mut Errs, i: usize, item: &Value) -> Option<String> {
    let key = format!("placements.{i}.signature_source_type");
    match raw_of(item, "signature_source_type") {
        Value::Null => None,
        Value::String(s) if matches!(s.as_str(), "upload" | "library") => Some(s.clone()),
        _ => {
            errs.add(
                &key,
                format!("The selected placements.{i}.signature source type is invalid."),
            );
            None
        }
    }
}

/// Aturan per item `placements.*` dari `storeSignaturePlacements`.
fn parse_placement(errs: &mut Errs, i: usize, item: &Value) -> Option<PlacementInput> {
    let before = errs.len();
    let signature_id = ph_string(errs, i, "signature_id", item, true, 64);
    let page_number = ph_int(errs, i, "page_number", item, true, 1, None);
    let x_ratio = ph_num(errs, i, "x_ratio", item, 0.0, 1.0);
    let y_ratio = ph_num(errs, i, "y_ratio", item, 0.0, 1.0);
    let scale = ph_num(errs, i, "scale", item, 0.01, 1.0);
    let sort_order = ph_int(errs, i, "sort_order", item, false, 0, None);
    let signature_name = ph_string(errs, i, "signature_name", item, true, 255);
    let signature_file_name = ph_string(errs, i, "signature_file_name", item, true, 255);
    let signature_mime_type = ph_string(errs, i, "signature_mime_type", item, true, 100);
    let signature_width = ph_int(errs, i, "signature_width", item, true, 1, Some(20_000));
    let signature_height = ph_int(errs, i, "signature_height", item, true, 1, Some(20_000));
    let signature_data_url = ph_data_url(errs, i, item);
    let signature_source_type = ph_source_type(errs, i, item);
    let signature_source_id = ph_string(errs, i, "signature_source_id", item, false, 64);
    if errs.len() > before {
        return None;
    }
    Some(PlacementInput {
        signature_id: signature_id?,
        page_number: page_number?,
        x_ratio: x_ratio?,
        y_ratio: y_ratio?,
        scale: scale?,
        sort_order,
        signature_name: signature_name?,
        signature_file_name: signature_file_name?,
        signature_mime_type: signature_mime_type?,
        signature_width: signature_width?,
        signature_height: signature_height?,
        signature_data_url,
        signature_source_type,
        signature_source_id,
    })
}

/// `storeSignaturePlacements`: kosong berarti tidak ada placement. JSON yang bukan array: 422 format.
/// Semua pemeriksaan dilakukan sebelum ada penulisan, jadi tidak perlu rollback.
fn parse_placements(raw: Option<&str>) -> Result<Vec<PlacementInput>, ApiError> {
    let Some(text) = raw else {
        return Ok(Vec::new());
    };
    let items = match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => items,
        _ => return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, FORMAT_MESSAGE)),
    };
    let mut errs = Errs::default();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if let Some(p) = parse_placement(&mut errs, i, item) {
            out.push(p);
        }
    }
    if !errs.is_empty() {
        return Err(errs.into_error());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Pembantu
// ---------------------------------------------------------------------------

async fn is_admin(pool: &MySqlPool, user_id: u64) -> Result<bool, ApiError> {
    let roles = auth::login::roles_of(pool, user_id).await.map_err(internal)?;
    Ok(roles.iter().any(|(_, name)| name == "admin"))
}

/// `ToolPdf::canManage`: pemilik atau admin.
async fn ensure_can_manage(
    pool: &MySqlPool,
    row: &ToolPdfRow,
    user_id: u64,
    message: &str,
) -> Result<(), ApiError> {
    if row.user_id == user_id as i64 || is_admin(pool, user_id).await? {
        return Ok(());
    }
    Err(ApiError::new(StatusCode::FORBIDDEN, message))
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

fn base_url(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

/// Nilai truthy PHP untuk string: tidak kosong dan bukan `"0"`.
fn php_truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

/// `pathinfo($path, PATHINFO_FILENAME)`: basename tanpa ekstensi terakhir.
fn php_filename(path: &str) -> &str {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match base.rfind('.') {
        Some(i) => &base[..i],
        None => base,
    }
}

/// `Str::slug($title, '_')` untuk ASCII. Karakter lain menjadi pemisah.
fn slug_underscore(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

/// Nama entri zip: `{NN}_{slug}.pdf`, dengan NN dari posisi dalam daftar yang ditemukan (termasuk yang dilewati).
fn entry_name(index: usize, row: &ToolPdfRow) -> String {
    let candidate = if php_truthy(&row.name) {
        row.name.clone()
    } else {
        match row.original_filename.as_deref() {
            Some(o) if php_truthy(o) => o.to_string(),
            _ => "document".to_string(),
        }
    };
    let slug = slug_underscore(php_filename(&candidate));
    let safe = if slug.is_empty() {
        "document".to_string()
    } else {
        slug
    };
    format!("{:02}_{}.pdf", index + 1, safe)
}

fn build_zip(entries: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, ApiError> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(internal)?;
        zip.write_all(&bytes).map_err(internal)?;
    }
    let cursor = zip.finish().map_err(internal)?;
    Ok(cursor.into_inner())
}

/// Validasi `ids` untuk bulk download. Aturan: `required|array|min:1`, `ids.*` required dan exists milik user.
async fn validate_ids(pool: &MySqlPool, user_id: u64, body: &Value) -> Result<Vec<i64>, ApiError> {
    let mut errs = Errs::default();
    let mut ids = Vec::new();
    match body.get("ids") {
        None | Some(Value::Null) => errs.add("ids", "The ids field is required.".to_string()),
        Some(Value::Array(items)) => {
            if items.is_empty() {
                errs.add("ids", "The ids field is required.".to_string());
                errs.add("ids", "The ids field must have at least 1 items.".to_string());
            }
            for (i, item) in items.iter().enumerate() {
                let key = format!("ids.{i}");
                let text = match item {
                    Value::Null => None,
                    Value::String(s) if s.trim().is_empty() => None,
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => {
                        errs.add(&key, format!("The selected {key} is invalid."));
                        continue;
                    }
                };
                match text {
                    None => errs.add(&key, format!("The {key} field is required.")),
                    Some(t) => {
                        let parsed = t.parse::<i64>().ok();
                        let owned = match parsed {
                            Some(id) => owned_exists(pool, id, user_id).await?,
                            None => false,
                        };
                        match (owned, parsed) {
                            (true, Some(id)) => ids.push(id),
                            _ => errs.add(&key, format!("The selected {key} is invalid.")),
                        }
                    }
                }
            }
        }
        Some(_) => errs.add("ids", "The ids field must be an array.".to_string()),
    }
    if !errs.is_empty() {
        return Err(errs.into_error());
    }
    Ok(ids)
}

async fn insert_placements(
    tx: &mut Transaction<'_, MySql>,
    pdf_id: i64,
    placements: &[PlacementInput],
) -> Result<(), ApiError> {
    for (index, p) in placements.iter().enumerate() {
        sqlx::query(
            "INSERT INTO tool_pdf_signature_placements (tool_pdf_id, signature_id, page_number, x_ratio, y_ratio, \
             scale, sort_order, signature_name, signature_file_name, signature_mime_type, signature_width, \
             signature_height, signature_data_url, signature_source_type, signature_source_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
        )
        .bind(pdf_id)
        .bind(&p.signature_id)
        .bind(p.page_number)
        .bind(p.x_ratio)
        .bind(p.y_ratio)
        .bind(p.scale)
        .bind(p.sort_order.unwrap_or(index as i64))
        .bind(&p.signature_name)
        .bind(&p.signature_file_name)
        .bind(&p.signature_mime_type)
        .bind(p.signature_width)
        .bind(p.signature_height)
        .bind(p.signature_data_url.clone())
        .bind(p.signature_source_type.clone())
        .bind(p.signature_source_id.clone())
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }
    Ok(())
}

/// Simpan `ToolPdf` beserta berkas dan placement dalam satu transaksi, lalu respon 201 `{data}`.
#[allow(clippy::too_many_arguments)]
async fn create_pdf(
    state: &AppState,
    headers: &HeaderMap,
    actor: u64,
    input: &PdfInput,
    kind: &str,
    parent_id: Option<i64>,
    placements: &[PlacementInput],
    url: &str,
) -> Result<Response, ApiError> {
    // `$validated['name'] ?: pathinfo(...)`.
    let name = match input.name.as_deref() {
        Some(n) if php_truthy(n) => n.to_string(),
        _ => php_filename(&input.upload.original_name).to_string(),
    };

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tool_pdfs (user_id, parent_id, name, original_filename, kind, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(actor as i64)
    .bind(parent_id)
    .bind(&name)
    .bind(&input.upload.original_name)
    .bind(kind)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    let row = find_pdf(&mut *tx, id, false)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("file PDF baru tidak terbaca"))?;
    changes::audit_only(
        &mut tx,
        headers,
        actor,
        MODEL,
        "created",
        id,
        None,
        Some(attributes(&row)),
        url,
    )
    .await?;

    let mime = media::mime_for_name(&input.upload.original_name);
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &input.upload, mime, false).await?;
    if let Err(e) = insert_placements(&mut tx, id, placements).await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(e);
    }
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }

    let row = find_pdf(&state.pool, id, false)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("file PDF tidak terbaca setelah simpan"))?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))).into_response())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/tool-pdfs`: `kind` (kecuali `all`) dan `search` pada `name` atau `original_filename`.
/// Tidak dipaginasi, seperti Laravel (`->get()`).
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let kind = filled(&query, "kind").filter(|k| *k != "all");
    let search = filled(&query, "search");

    let mut sql = format!("{SELECT_PDF} WHERE user_id = ? AND deleted_at IS NULL");
    if kind.is_some() {
        sql.push_str(" AND kind = ?");
    }
    if search.is_some() {
        sql.push_str(" AND (name LIKE ? OR original_filename LIKE ?)");
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC");

    let mut q = sqlx::query(&sql).bind(user.user_id as i64);
    if let Some(k) = kind {
        q = q.bind(k);
    }
    if let Some(s) = search {
        let pattern = format!("%{s}%");
        q = q.bind(pattern.clone()).bind(pattern);
    }
    let raw_rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let rows = raw_rows
        .iter()
        .map(map_pdf)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(resource(&state.pool, &state.app_url, row).await?);
    }
    Ok(Json(json!({ "data": data })).into_response())
}

fn filled<'a>(query: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    query.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// `POST /api/tool-pdfs`: 201 dengan `{data}`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut raw = foto::read_form(multipart).await?;
    let input = validate_input(&state.pool, &mut raw, user.user_id, false).await?;
    let placements = parse_placements(input.placements.as_deref())?;
    let url = format!("{}/api/tool-pdfs", base_url(&state));
    create_pdf(
        &state,
        &headers,
        user.user_id,
        &input,
        "source",
        None,
        &placements,
        &url,
    )
    .await
}

/// `POST /api/tool-pdfs/sign`: berkas hasil tanda tangan, dengan `parent_id` bila `source_id` dikirim.
pub async fn sign(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut raw = foto::read_form(multipart).await?;
    let input = validate_input(&state.pool, &mut raw, user.user_id, true).await?;
    let parent_id = match input.source_id {
        Some(source_id) => {
            let source = find_owned(&state.pool, source_id, user.user_id)
                .await?
                .ok_or_else(ApiError::not_found)?;
            Some(source.id)
        }
        None => None,
    };
    let placements = parse_placements(input.placements.as_deref())?;
    let url = format!("{}/api/tool-pdfs/sign", base_url(&state));
    create_pdf(
        &state,
        &headers,
        user.user_id,
        &input,
        "signed",
        parent_id,
        &placements,
        &url,
    )
    .await
}

/// `POST /api/tool-pdfs/bulk-download`: zip berisi berkas yang ditemukan. Entri yang hilang di disk dilewati.
pub async fn bulk_download(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    let body = axum::body::to_bytes(request.into_body(), JSON_LIMIT)
        .await
        .map_err(bad_request)?;
    // JSON yang tidak valid diperlakukan seperti input kosong (`ids` wajib, jadi 422).
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let ids = validate_ids(&state.pool, user.user_id, &json).await?;

    let rows = find_owned_many(&state.pool, user.user_id, &ids).await?;
    if rows.is_empty() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "File PDF tidak ditemukan"));
    }

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(stored) = media::first_media(&state.pool, MODEL, row.id as u64, COLLECTION)
            .await
            .map_err(internal)?
        else {
            continue;
        };
        let path = media::media_dir(stored.id).join(&stored.file_name);
        let Ok(bytes) = tokio::fs::read(&path).await else {
            continue;
        };
        entries.push((entry_name(index, row), bytes));
    }
    let archive = build_zip(entries)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"tool-pdfs-bulk.zip\"",
            ),
        ],
        archive,
    )
        .into_response())
}

/// `GET /api/tool-pdfs/{id}/download`: berkas inline dengan tipe dari media.
pub async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let pdf = find_pdf(&state.pool, id, true)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_can_manage(
        &state.pool,
        &pdf,
        user.user_id,
        "Anda tidak memiliki akses ke file PDF ini",
    )
    .await?;

    let stored = media::first_media(&state.pool, MODEL, pdf.id as u64, COLLECTION)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "File PDF tidak ditemukan"))?;
    let bytes = tokio::fs::read(media::media_dir(stored.id).join(&stored.file_name))
        .await
        .map_err(internal)?;
    let content_type = if stored.mime_type.is_empty() {
        "application/pdf".to_string()
    } else {
        stored.mime_type.clone()
    };
    let disposition = format!("inline; filename=\"{}\"", stored.file_name);
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response())
}

/// `DELETE /api/tool-pdfs/{id}`: soft delete. Berkas dan baris media tetap ada, seperti Laravel.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let pdf = find_pdf(&state.pool, id, true)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_can_manage(
        &state.pool,
        &pdf,
        user.user_id,
        "Anda tidak memiliki akses untuk menghapus file ini",
    )
    .await?;

    let url = format!("{}/api/tool-pdfs/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE tool_pdfs SET deleted_at = NOW(), updated_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let after = find_pdf(&mut *tx, id, false)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("file PDF hilang saat dihapus"))?;
    let mut old = attributes(&pdf);
    old.insert("deleted_at".into(), carbon_json(after.deleted_at));
    old.insert("updated_at".into(), carbon_json(after.updated_at));
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "deleted",
        id,
        Some(old),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({
        "success": true,
        "message": "File PDF berhasil dihapus",
    }))
    .into_response())
}
