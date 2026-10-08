//! `/api/foto`: port `FotoController` untuk show, store, update, destroy, dan bulk destroy.
//! Daftar (`index`) belum dipindah.
//!
//! Thumbnail dibuat saat upload (lihat `media.rs`). Relasi `penerima` dan `komponen` yang tidak ada
//! dikirim `null` (lihat T33).

use std::{collections::BTreeMap, collections::HashMap, path::PathBuf};

use axum::{
    extract::{Multipart, Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    access, changes, crypt, format::iso8601_utc, koordinat, lookup::carbon_json, media, pagination,
    require_auth, AppState,
};

pub const COLLECTION: &str = "foto/pekerjaan";
/// Nama kelas Laravel, dipakai sebagai `model_type` media dan `auditable_type`.
const MODEL: &str = "App\\Models\\Foto";
const KETERANGAN: &[&str] = &["0%", "25%", "50%", "75%", "100%"];
const FORBIDDEN_MESSAGE: &str = "Anda tidak memiliki akses untuk pekerjaan ini";
/// Batas body rute foto: berkas maksimum ditambah ruang untuk bagian multipart lain.
pub const BODY_LIMIT: usize = media::MAX_FILE_BYTES + 1024 * 1024;

const SELECT_FOTO: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
     CAST(komponen_id AS SIGNED) AS komponen_id, CAST(penerima_id AS SIGNED) AS penerima_id, keterangan, koordinat, \
     validasi_koordinat, validasi_koordinat_message, CAST(unit_index AS SIGNED) AS unit_index, created_at, updated_at \
     FROM tbl_foto";

/// Kolom yang bisa diubah lewat update, urut tetap agar audit konsisten.
const COLUMNS: &[&str] = &[
    "pekerjaan_id",
    "komponen_id",
    "penerima_id",
    "keterangan",
    "koordinat",
    "validasi_koordinat",
    "validasi_koordinat_message",
    "unit_index",
];

/// Baris `tbl_foto`. Kolom id dibaca sebagai `i64` (lihat `CAST ... AS SIGNED` untuk kolom unsigned).
#[derive(Debug, Clone, PartialEq)]
pub struct FotoRow {
    pub id: i64,
    pub pekerjaan_id: Option<i64>,
    pub komponen_id: i64,
    pub penerima_id: Option<i64>,
    pub keterangan: String,
    pub koordinat: String,
    pub validasi_koordinat: bool,
    pub validasi_koordinat_message: Option<String>,
    pub unit_index: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &MySqlRow) -> Result<FotoRow, sqlx::Error> {
    Ok(FotoRow {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        komponen_id: r.try_get("komponen_id")?,
        penerima_id: r.try_get("penerima_id")?,
        keterangan: r.try_get("keterangan")?,
        koordinat: r.try_get("koordinat")?,
        validasi_koordinat: r.try_get("validasi_koordinat")?,
        validasi_koordinat_message: r.try_get("validasi_koordinat_message")?,
        unit_index: r.try_get("unit_index")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<FotoRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_FOTO} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn col_json(row: &FotoRow, col: &str) -> Value {
    match col {
        "pekerjaan_id" => json!(row.pekerjaan_id),
        "komponen_id" => json!(row.komponen_id),
        "penerima_id" => json!(row.penerima_id),
        "keterangan" => json!(row.keterangan),
        "koordinat" => json!(row.koordinat),
        "validasi_koordinat" => json!(row.validasi_koordinat),
        "validasi_koordinat_message" => json!(row.validasi_koordinat_message),
        "unit_index" => json!(row.unit_index),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(row: &FotoRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

// ---------------------------------------------------------------------------
// Input multipart dan validasi
// ---------------------------------------------------------------------------

/// Nilai satu field: `Absent` tidak dikirim, `Null` dikirim kosong (ConvertEmptyStringsToNull).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Presence<'a> {
    Absent,
    Null,
    Value(&'a str),
}

/// `None` = tidak ada di `validated`, `Some(None)` = null, `Some(Some(v))` = nilai.
type Field<T> = Option<Option<T>>;

#[derive(Debug, Default)]
pub(crate) struct RawForm {
    pub(crate) fields: BTreeMap<String, Option<String>>,
    pub(crate) file: Option<media::Upload>,
}

impl RawForm {
    pub(crate) fn presence(&self, key: &str) -> Presence<'_> {
        match self.fields.get(key) {
            None => Presence::Absent,
            Some(None) => Presence::Null,
            Some(Some(v)) => Presence::Value(v),
        }
    }
}

#[derive(Debug)]
struct Form {
    pekerjaan_id: Field<i64>,
    komponen_id: Field<i64>,
    penerima_id: Field<i64>,
    keterangan: Field<String>,
    koordinat: Field<String>,
    unit_index: Field<i64>,
    file: Option<media::Upload>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Store,
    Update,
}

fn attribute(key: &str) -> String {
    key.replace('_', " ")
}

pub(crate) fn add(errs: &mut BTreeMap<String, Vec<String>>, key: &str, message: String) {
    errs.entry(key.to_string()).or_default().push(message);
}

fn int_field(
    raw: &RawForm,
    key: &str,
    required: bool,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Field<i64> {
    match raw.presence(key) {
        Presence::Absent if !required => None,
        Presence::Null if !required => Some(None),
        Presence::Absent | Presence::Null => {
            add(
                errs,
                key,
                format!("The {} field is required.", attribute(key)),
            );
            None
        }
        Presence::Value(v) => match v.parse::<i64>() {
            Ok(n) => Some(Some(n)),
            Err(_) => {
                add(
                    errs,
                    key,
                    format!("The {} field must be an integer.", attribute(key)),
                );
                None
            }
        },
    }
}

fn keterangan_field(
    raw: &RawForm,
    required: bool,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Field<String> {
    match raw.presence("keterangan") {
        Presence::Absent if !required => None,
        Presence::Null if !required => Some(None),
        Presence::Absent | Presence::Null => {
            add(
                errs,
                "keterangan",
                "The keterangan field is required.".into(),
            );
            None
        }
        Presence::Value(v) if KETERANGAN.contains(&v) => Some(Some(v.to_string())),
        Presence::Value(_) => {
            add(
                errs,
                "keterangan",
                "The selected keterangan is invalid.".into(),
            );
            None
        }
    }
}

fn koordinat_field(
    raw: &RawForm,
    required: bool,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Field<String> {
    match raw.presence("koordinat") {
        Presence::Absent if !required => None,
        Presence::Null if !required => Some(None),
        Presence::Absent | Presence::Null => {
            add(errs, "koordinat", "The koordinat field is required.".into());
            None
        }
        Presence::Value(v) if v.chars().count() > 255 => {
            add(
                errs,
                "koordinat",
                "The koordinat field must not be greater than 255 characters.".into(),
            );
            None
        }
        Presence::Value(v) => Some(Some(v.to_string())),
    }
}

/// Aturan `file|mimes:jpg,jpeg,png|max:51200`. Tipe dibaca dari isi berkas, bukan nama.
fn file_field(
    raw: &mut RawForm,
    required: bool,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<media::Upload> {
    let Some(upload) = raw.file.take() else {
        if required {
            add(errs, "file", "The file field is required.".into());
        }
        return None;
    };
    if upload.bytes.len() > media::MAX_FILE_BYTES {
        add(
            errs,
            "file",
            "The file field must not be greater than 51200 kilobytes.".into(),
        );
        return None;
    }
    if media::image_mime(&upload.bytes).is_none() {
        add(
            errs,
            "file",
            "The file field must be a file of type: jpg, jpeg, png.".into(),
        );
        return None;
    }
    Some(upload)
}

fn parse_form(mut raw: RawForm, mode: Mode) -> Result<Form, ApiError> {
    let required = mode == Mode::Store;
    let mut errs = BTreeMap::new();
    let form = Form {
        pekerjaan_id: int_field(&raw, "pekerjaan_id", required, &mut errs),
        komponen_id: int_field(&raw, "komponen_id", required, &mut errs),
        penerima_id: int_field(&raw, "penerima_id", false, &mut errs),
        keterangan: keterangan_field(&raw, required, &mut errs),
        koordinat: koordinat_field(&raw, required, &mut errs),
        unit_index: int_field(&raw, "unit_index", false, &mut errs),
        file: file_field(&mut raw, required, &mut errs),
    };
    if errs.is_empty() {
        Ok(form)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

fn bad_multipart(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan multipart tidak valid: {e}"),
    )
}

/// Membaca multipart. Field teks di-trim dan kosong menjadi null. Berkas kosong dianggap tidak ada.
pub(crate) async fn read_form(mut multipart: Multipart) -> Result<RawForm, ApiError> {
    let mut raw = RawForm::default();
    while let Some(field) = multipart.next_field().await.map_err(bad_multipart)? {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            let original_name = field.file_name().unwrap_or_default().to_string();
            let bytes = field.bytes().await.map_err(bad_multipart)?;
            if !bytes.is_empty() {
                raw.file = Some(media::Upload {
                    original_name,
                    bytes: bytes.to_vec(),
                });
            }
        } else {
            let text = field.text().await.map_err(bad_multipart)?;
            let trimmed = text.trim();
            let value = (!trimmed.is_empty()).then(|| trimmed.to_string());
            raw.fields.insert(name, value);
        }
    }
    Ok(raw)
}

pub(crate) fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

pub(crate) fn base_url(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

// ---------------------------------------------------------------------------
// Otorisasi
// ---------------------------------------------------------------------------

/// `Pekerjaan::userCanAccess`. Pekerjaan kosong atau tidak diizinkan menghasilkan 403.
pub(crate) async fn ensure_access(
    state: &AppState,
    actor: u64,
    roles: &[(u64, String)],
    pekerjaan_id: Option<i64>,
) -> Result<(), ApiError> {
    let allowed = match pekerjaan_id {
        Some(p) => access::user_can_access(&state.pool, actor, roles, p as u64)
            .await
            .map_err(media::internal)?,
        None => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, FORBIDDEN_MESSAGE))
    }
}

async fn pekerjaan_exists(pool: &MySqlPool, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(media::internal)?;
    Ok(n > 0)
}

/// Aturan `exists:tbl_pekerjaan,id`: dicek setelah validasi bentuk, dengan pesan 422 yang sama.
async fn check_pekerjaan_exists(pool: &MySqlPool, form: &Form) -> Result<(), ApiError> {
    if let Some(Some(id)) = form.pekerjaan_id {
        if !pekerjaan_exists(pool, id).await? {
            let mut errs = BTreeMap::new();
            add(
                &mut errs,
                "pekerjaan_id",
                "The selected pekerjaan id is invalid.".into(),
            );
            return Err(ApiError::validation("The given data was invalid.", errs));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Audit, notifikasi, dan resource
// ---------------------------------------------------------------------------

/// Audit dan notifikasi admin untuk satu perubahan foto, dalam transaksi yang sama.
#[allow(clippy::too_many_arguments)]
async fn log_change(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    pekerjaan_id: Option<i64>,
    url: &str,
) -> Result<(), ApiError> {
    changes::log(
        tx,
        headers,
        actor,
        &changes::FOTO,
        event,
        id,
        old,
        new,
        pekerjaan_id,
        url,
    )
    .await
}

/// Nama `penerima.nik` didekripsi dengan `APP_KEY`, seperti cast `encrypted` di Laravel.
fn decrypt_nik(nik: Option<String>) -> Result<Value, ApiError> {
    let Some(payload) = nik else {
        return Ok(Value::Null);
    };
    let app_key = std::env::var("APP_KEY")
        .map_err(|_| media::internal("APP_KEY belum di-set: nik penerima tidak bisa dibaca"))?;
    let key = crypt::key_from_app_key(&app_key).map_err(|e| media::internal(format!("{e:?}")))?;
    let plain =
        crypt::decrypt_string(&key, &payload).map_err(|e| media::internal(format!("{e:?}")))?;
    Ok(json!(plain))
}

/// `FotoResource` (dengan relasi pekerjaan, penerima, dan komponen).
pub async fn resource(pool: &MySqlPool, app_url: &str, row: &FotoRow) -> Result<Value, ApiError> {
    let (foto_url, thumb_url) = media::first_urls(pool, app_url, MODEL, row.id as u64, COLLECTION)
        .await
        .map_err(media::internal)?;

    let pekerjaan = match row.pekerjaan_id {
        Some(p) => sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, nama_paket FROM tbl_pekerjaan WHERE id = ?",
        )
        .bind(p)
        .fetch_optional(pool)
        .await
        .map_err(media::internal)?
        .map(|r| -> Result<Value, sqlx::Error> {
            Ok(json!({
                "id": r.try_get::<i64, _>("id")?,
                "nama_paket": r.try_get::<Option<String>, _>("nama_paket")?,
            }))
        })
        .transpose()
        .map_err(media::internal)?
        .unwrap_or(Value::Null),
        None => Value::Null,
    };

    let penerima = match row.penerima_id {
        Some(p) => match sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, nama, nik FROM tbl_penerima WHERE id = ?",
        )
        .bind(p)
        .fetch_optional(pool)
        .await
        .map_err(media::internal)?
        {
            Some(r) => json!({
                "id": r.try_get::<i64, _>("id").map_err(media::internal)?,
                "nama": r.try_get::<Option<String>, _>("nama").map_err(media::internal)?,
                "nik": decrypt_nik(r.try_get("nik").map_err(media::internal)?)?,
            }),
            None => Value::Null,
        },
        None => Value::Null,
    };

    let komponen =
        sqlx::query("SELECT CAST(id AS SIGNED) AS id, komponen FROM tbl_output WHERE id = ?")
            .bind(row.komponen_id)
            .fetch_optional(pool)
            .await
            .map_err(media::internal)?
            .map(|r| -> Result<Value, sqlx::Error> {
                Ok(json!({
                    "id": r.try_get::<i64, _>("id")?,
                    "komponen": r.try_get::<Option<String>, _>("komponen")?,
                }))
            })
            .transpose()
            .map_err(media::internal)?
            .unwrap_or(Value::Null);

    Ok(json!({
        "id": row.id,
        "pekerjaan_id": row.pekerjaan_id,
        "komponen_id": row.komponen_id,
        "penerima_id": row.penerima_id,
        "keterangan": row.keterangan,
        "koordinat": row.koordinat,
        "validasi_koordinat": row.validasi_koordinat,
        "validasi_koordinat_message": row.validasi_koordinat_message,
        "unit_index": row.unit_index,
        "foto_url": foto_url,
        // `getFirstMediaUrl(.., 'thumb') ?: getFirstMediaUrl(..)`.
        "foto_thumb_url": if thumb_url.is_empty() { foto_url.clone() } else { thumb_url },
        "pekerjaan": pekerjaan,
        "penerima": penerima,
        "komponen": komponen,
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    }))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/foto/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    ensure_access(&state, user.user_id, &roles, row.pekerjaan_id).await?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Filter daftar foto yang bernilai benar seperti PHP: kosong dan `"0"` dianggap tidak aktif.
fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// `GET /api/foto`. Foto difilter lewat pekerjaan induknya dengan scope `byUserRole()`.
/// `pekerjaan_id` (jika ada) mengembalikan semua foto tanpa paginasi, seperti Laravel.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");

    // Setiap klausa punya nilai bind berurutan (semua dikirim sebagai string, MySQL mengonversinya).
    let mut clauses: Vec<(String, Vec<String>)> = vec![(
        format!(
            "f.pekerjaan_id IN (SELECT p.id FROM tbl_pekerjaan p WHERE 1=1{})",
            scope.sql
        ),
        scope.binds.iter().map(u64::to_string).collect(),
    )];
    if truthy(query.get("tahun")) {
        clauses.push((
            "f.pekerjaan_id IN (SELECT p.id FROM tbl_pekerjaan p JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE k.tahun_anggaran = ?)"
                .into(),
            vec![query["tahun"].clone()],
        ));
    }
    if let Some(term) = query.get("search").filter(|v| !v.is_empty()) {
        let like = format!("%{term}%");
        clauses.push((
            "f.pekerjaan_id IN (SELECT p.id FROM tbl_pekerjaan p WHERE (p.nama_paket LIKE ? OR p.kode_rekening LIKE ? \
             OR p.id IN (SELECT kp.pekerjaan_id FROM kontrak_pekerjaan kp JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
             JOIN tbl_penyedia py ON py.id = k.id_penyedia WHERE py.nama LIKE ?)))"
                .into(),
            vec![like.clone(), like.clone(), like],
        ));
    }
    if truthy(query.get("latest_only")) {
        clauses.push((
            "f.id IN (SELECT MAX(id) FROM tbl_foto GROUP BY pekerjaan_id)".into(),
            Vec::new(),
        ));
    }
    let by_pekerjaan = query.contains_key("pekerjaan_id");
    if by_pekerjaan {
        clauses.push((
            "f.pekerjaan_id = ?".into(),
            vec![query["pekerjaan_id"].clone()],
        ));
    }

    let where_sql = format!(
        " WHERE {}",
        clauses
            .iter()
            .map(|(c, _)| c.as_str())
            .collect::<Vec<_>>()
            .join(" AND ")
    );
    let binds: Vec<String> = clauses.into_iter().flat_map(|(_, b)| b).collect();
    let select = SELECT_FOTO.replace(" FROM tbl_foto", " FROM tbl_foto f");

    let sql = format!("{select}{where_sql} ORDER BY f.id");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let all = q
        .fetch_all(&state.pool)
        .await
        .map_err(media::internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(media::internal)?;

    if by_pekerjaan {
        let mut data = Vec::with_capacity(all.len());
        for row in &all {
            data.push(resource(&state.pool, &state.app_url, row).await?);
        }
        return Ok(Json(json!({ "data": data })).into_response());
    }

    let per_page_raw = query.get("per_page").map(String::as_str).unwrap_or("20");
    if per_page_raw.trim() == "-1" {
        let mut data = Vec::with_capacity(all.len());
        for row in &all {
            data.push(resource(&state.pool, &state.app_url, row).await?);
        }
        return Ok(Json(json!({ "data": data })).into_response());
    }
    let per_page = per_page_raw
        .parse::<u64>()
        .ok()
        .filter(|v| *v >= 1)
        .unwrap_or(20);
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let total = all.len() as u64;
    let offset = ((page - 1) * per_page) as usize;
    let mut data = Vec::new();
    for row in all.iter().skip(offset).take(per_page as usize) {
        data.push(resource(&state.pool, &state.app_url, row).await?);
    }
    let base = format!("{}/api/foto", state.app_url.trim_end_matches('/'));
    let body = pagination::paginate_with_query(
        data,
        total,
        pagination::PageParams { page, per_page },
        &base,
        &crate::pekerjaan::query_without_page(raw.as_deref()),
    );
    Ok(Json(body).into_response())
}

/// `POST /api/foto`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let form = parse_form(read_form(multipart).await?, Mode::Store)?;
    check_pekerjaan_exists(&state.pool, &form).await?;

    let pekerjaan_id = form.pekerjaan_id.flatten().unwrap_or_default();
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    ensure_access(&state, user.user_id, &roles, Some(pekerjaan_id)).await?;

    let komponen_id = form.komponen_id.flatten().unwrap_or_default();
    let penerima_id = form.penerima_id.flatten();
    let keterangan = form.keterangan.flatten().unwrap_or_default();
    let koordinat_input = form.koordinat.flatten().unwrap_or_default();
    let unit_index = form.unit_index.flatten();
    let upload = form
        .file
        .ok_or_else(|| media::internal("berkas wajib ada setelah validasi"))?;
    let mime = media::image_mime(&upload.bytes)
        .ok_or_else(|| media::internal("tipe berkas tidak valid setelah validasi"))?;

    let check =
        koordinat::validate_for_pekerjaan(&state.pool, pekerjaan_id as u64, &koordinat_input)
            .await
            .map_err(media::internal)?;

    let url = format!("{}/api/foto", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_foto (pekerjaan_id, komponen_id, penerima_id, keterangan, koordinat, validasi_koordinat, \
         validasi_koordinat_message, unit_index, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(komponen_id)
    .bind(penerima_id)
    .bind(&keterangan)
    .bind(&koordinat_input)
    .bind(check.valid)
    .bind(&check.message)
    .bind(unit_index)
    .execute(&mut *tx)
    .await
    .map_err(media::internal)?;
    let id = res.last_insert_id() as i64;

    let row = find_row(&mut *tx, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(|| media::internal("foto baru tidak terbaca"))?;
    log_change(
        &mut tx,
        &headers,
        user.user_id,
        "created",
        id,
        None,
        Some(attributes(&row)),
        Some(pekerjaan_id),
        &url,
    )
    .await?;

    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &upload, mime, true).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(media::internal(e));
    }

    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `PUT` dan `PATCH /api/foto/{id}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    update_impl(state, headers, id, multipart, false).await
}

/// `POST /api/foto/{id}` dengan `_method=PUT`, seperti method spoofing Laravel.
pub async fn update_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    update_impl(state, headers, id, multipart, true).await
}

async fn update_impl(
    state: AppState,
    headers: HeaderMap,
    id: String,
    multipart: Multipart,
    require_method_override: bool,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    ensure_access(&state, user.user_id, &roles, current.pekerjaan_id).await?;

    let raw = read_form(multipart).await?;
    if require_method_override && raw.presence("_method") != Presence::Value("PUT") {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "The POST method is not supported for this route. Supported methods: GET, HEAD, PUT, PATCH, DELETE.",
        ));
    }
    let form = parse_form(raw, Mode::Update)?;
    check_pekerjaan_exists(&state.pool, &form).await?;

    let mut next = current.clone();
    // `validated['pekerjaan_id'] ?? $foto->pekerjaan_id`: null tidak menimpa kolom.
    let target_pekerjaan = match form.pekerjaan_id {
        Some(Some(p)) => Some(p),
        _ => current.pekerjaan_id,
    };
    if let Some(Some(p)) = form.pekerjaan_id {
        next.pekerjaan_id = Some(p);
    }
    if let Some(Some(v)) = form.komponen_id {
        next.komponen_id = v;
    }
    set_nullable(form.penerima_id, &mut next.penerima_id);
    set_nullable(form.unit_index, &mut next.unit_index);
    if let Some(Some(v)) = &form.keterangan {
        next.keterangan = v.clone();
    }
    if let Some(Some(v)) = &form.koordinat {
        // Validasi koordinat hanya jika koordinat dikirim, memakai pekerjaan target.
        let target = target_pekerjaan.ok_or_else(ApiError::not_found)?;
        if !pekerjaan_exists(&state.pool, target).await? {
            return Err(ApiError::not_found());
        }
        ensure_access(&state, user.user_id, &roles, Some(target)).await?;
        let check = koordinat::validate_for_pekerjaan(&state.pool, target as u64, v)
            .await
            .map_err(media::internal)?;
        next.koordinat = v.clone();
        next.validasi_koordinat = check.valid;
        next.validasi_koordinat_message = Some(check.message);
    }

    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let url = format!("{}/api/foto/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let (created_dir, obsolete_dirs) = update_in_tx(
        &mut tx,
        &headers,
        user.user_id,
        &current,
        &next,
        &changed,
        form.file.as_ref(),
        &url,
    )
    .await?;
    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(media::internal(e));
    }
    media::remove_dirs(&obsolete_dirs).await;

    let row = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

fn set_nullable<T>(field: Field<T>, slot: &mut Option<T>) {
    match field {
        Some(Some(v)) => *slot = Some(v),
        Some(None) => *slot = None,
        None => {}
    }
}

/// Menulis perubahan, audit, dan berkas baru. Mengembalikan direktori berkas baru (untuk dibersihkan
/// bila commit gagal) dan direktori berkas lama (dihapus setelah commit).
#[allow(clippy::too_many_arguments)]
async fn update_in_tx(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    current: &FotoRow,
    next: &FotoRow,
    changed: &[&str],
    upload: Option<&media::Upload>,
    url: &str,
) -> Result<(Option<PathBuf>, Vec<PathBuf>), ApiError> {
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_foto SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "pekerjaan_id" => {
                    qb.push_bind(next.pekerjaan_id);
                }
                "komponen_id" => {
                    qb.push_bind(next.komponen_id);
                }
                "penerima_id" => {
                    qb.push_bind(next.penerima_id);
                }
                "keterangan" => {
                    qb.push_bind(next.keterangan.clone());
                }
                "koordinat" => {
                    qb.push_bind(next.koordinat.clone());
                }
                "validasi_koordinat" => {
                    qb.push_bind(next.validasi_koordinat);
                }
                "validasi_koordinat_message" => {
                    qb.push_bind(next.validasi_koordinat_message.clone());
                }
                "unit_index" => {
                    qb.push_bind(next.unit_index);
                }
                _ => {}
            }
        }
        qb.push(", updated_at = NOW() WHERE id = ")
            .push_bind(current.id);
        qb.build()
            .execute(&mut **tx)
            .await
            .map_err(media::internal)?;

        let after = find_row(&mut **tx, current.id)
            .await
            .map_err(media::internal)?
            .ok_or_else(|| media::internal("foto hilang saat update"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in changed {
            old.insert((*col).into(), col_json(current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        log_change(
            tx,
            headers,
            actor,
            "updated",
            current.id,
            Some(old),
            Some(new),
            next.pekerjaan_id,
            url,
        )
        .await?;
    }

    // Berkas lama dihapus dulu, lalu berkas baru disimpan: hasil akhirnya sama dengan Laravel
    // (add media baru, lalu hapus yang lain), dan tidak ada langkah gagal setelah berkas baru ditulis.
    let mut obsolete = Vec::new();
    let mut created = None;
    if let Some(up) = upload {
        obsolete = media::delete_collection(tx, MODEL, current.id as u64, COLLECTION, None).await?;
        let mime = media::image_mime(&up.bytes)
            .ok_or_else(|| media::internal("tipe berkas tidak valid setelah validasi"))?;
        let stored =
            media::attach(tx, MODEL, current.id as u64, COLLECTION, up, mime, true).await?;
        created = Some(stored.dir);
    }
    Ok((created, obsolete))
}

/// `DELETE /api/foto/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    ensure_access(&state, user.user_id, &roles, row.pekerjaan_id).await?;

    let url = format!("{}/api/foto/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let dirs = delete_in_tx(&mut tx, &headers, user.user_id, &row, &url).await?;
    tx.commit().await.map_err(media::internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({ "message": "Foto deleted successfully" })).into_response())
}

/// Menghapus media dan baris satu foto beserta audit `deleted`. Mengembalikan direktori berkas.
async fn delete_in_tx(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    row: &FotoRow,
    url: &str,
) -> Result<Vec<PathBuf>, ApiError> {
    let dirs = media::delete_collection(tx, MODEL, row.id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_foto WHERE id = ?")
        .bind(row.id)
        .execute(&mut **tx)
        .await
        .map_err(media::internal)?;
    log_change(
        tx,
        headers,
        actor,
        "deleted",
        row.id,
        Some(attributes(row)),
        None,
        row.pekerjaan_id,
        url,
    )
    .await?;
    Ok(dirs)
}

/// `ids` untuk bulk delete: `required|array|min:1`, `ids.*` integer.
pub(crate) fn parse_ids(body: &Value) -> Result<Vec<i64>, ApiError> {
    let mut errs = BTreeMap::new();
    let items = match body.get("ids") {
        None | Some(Value::Null) => {
            add(&mut errs, "ids", "The ids field is required.".into());
            None
        }
        Some(Value::Array(a)) if a.is_empty() => {
            add(
                &mut errs,
                "ids",
                "The ids field must have at least 1 items.".into(),
            );
            None
        }
        Some(Value::Array(a)) => Some(a),
        Some(_) => {
            add(&mut errs, "ids", "The ids field must be an array.".into());
            None
        }
    };
    let mut ids = Vec::new();
    if let Some(items) = items {
        for (i, v) in items.iter().enumerate() {
            let parsed = v
                .as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            match parsed {
                Some(n) => ids.push(n),
                None => add(
                    &mut errs,
                    &format!("ids.{i}"),
                    format!("The ids.{i} field must be an integer."),
                ),
            }
        }
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// `DELETE /api/foto/bulk` dengan body `{"ids": [...]}`.
pub async fn bulk_destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let ids = parse_ids(&body)?;

    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!("{SELECT_FOTO} WHERE id IN ({placeholders}) ORDER BY id");
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(media::internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(media::internal)?;
    if rows.is_empty() {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "Foto tidak ditemukan"));
    }

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    for row in &rows {
        ensure_access(&state, user.user_id, &roles, row.pekerjaan_id).await?;
    }

    let url = format!("{}/api/foto/bulk", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let mut dirs = Vec::new();
    for row in &rows {
        dirs.extend(delete_in_tx(&mut tx, &headers, user.user_id, row, &url).await?);
    }
    tx.commit().await.map_err(media::internal)?;
    media::remove_dirs(&dirs).await;

    let deleted = rows.len();
    Ok(Json(json!({
        "message": format!("{deleted} foto dihapus"),
        "deleted": deleted,
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pairs: &[(&str, Option<&str>)]) -> RawForm {
        RawForm {
            fields: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.map(str::to_string)))
                .collect(),
            file: None,
        }
    }

    fn jpeg() -> media::Upload {
        media::Upload {
            original_name: "foto.jpg".into(),
            bytes: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00],
        }
    }

    fn messages(err: ApiError) -> BTreeMap<String, Vec<String>> {
        let body = err.body();
        serde_json::from_value(body["errors"].clone()).unwrap()
    }

    #[test]
    fn store_requires_core_fields_and_file() {
        let err = parse_form(RawForm::default(), Mode::Store).unwrap_err();
        let errs = messages(err);
        for key in [
            "pekerjaan_id",
            "komponen_id",
            "keterangan",
            "koordinat",
            "file",
        ] {
            assert!(errs.contains_key(key), "{key} harus wajib");
        }
        assert!(!errs.contains_key("penerima_id"), "penerima opsional");
        assert_eq!(
            errs["pekerjaan_id"],
            vec!["The pekerjaan id field is required."]
        );
    }

    #[test]
    fn keterangan_must_be_a_listed_percentage() {
        let mut r = raw(&[
            ("pekerjaan_id", Some("1")),
            ("komponen_id", Some("2")),
            ("koordinat", Some("-6.8, 107.2")),
            ("keterangan", Some("50")),
        ]);
        r.file = Some(jpeg());
        let errs = messages(parse_form(r, Mode::Store).unwrap_err());
        assert_eq!(
            errs["keterangan"],
            vec!["The selected keterangan is invalid."]
        );
    }

    #[test]
    fn valid_store_input_parses_and_nulls_are_kept_for_nullable_fields() {
        let mut r = raw(&[
            ("pekerjaan_id", Some("1")),
            ("komponen_id", Some("2")),
            ("koordinat", Some("-6.8, 107.2")),
            ("keterangan", Some("100%")),
            ("penerima_id", None),
        ]);
        r.file = Some(jpeg());
        let form = parse_form(r, Mode::Store).unwrap();
        assert_eq!(form.pekerjaan_id, Some(Some(1)));
        assert_eq!(form.penerima_id, Some(None));
        assert!(form.unit_index.is_none());
    }

    #[test]
    fn update_accepts_absent_fields_and_rejects_wrong_file_type() {
        let mut r = raw(&[("keterangan", Some("25%"))]);
        r.file = Some(media::Upload {
            original_name: "dokumen.pdf".into(),
            bytes: b"%PDF-1.4".to_vec(),
        });
        let errs = messages(parse_form(r, Mode::Update).unwrap_err());
        assert_eq!(
            errs["file"],
            vec!["The file field must be a file of type: jpg, jpeg, png."]
        );
        let ok = parse_form(raw(&[("keterangan", Some("25%"))]), Mode::Update).unwrap();
        assert!(ok.pekerjaan_id.is_none(), "tidak dikirim = tidak diubah");
    }

    #[test]
    fn koordinat_is_limited_to_255_characters() {
        let long = "1".repeat(256);
        let mut r = raw(&[
            ("pekerjaan_id", Some("1")),
            ("komponen_id", Some("2")),
            ("keterangan", Some("0%")),
            ("koordinat", Some(&long)),
        ]);
        r.file = Some(jpeg());
        let errs = messages(parse_form(r, Mode::Store).unwrap_err());
        assert!(errs["koordinat"][0].contains("255"));
    }

    #[test]
    fn bulk_ids_follow_laravel_rules() {
        assert!(parse_ids(&json!({})).is_err());
        assert!(parse_ids(&json!({"ids": []})).is_err());
        assert_eq!(parse_ids(&json!({"ids": [3, "1", 3]})).unwrap(), vec![1, 3]);
        assert!(parse_ids(&json!({"ids": ["x"]})).is_err());
    }

    #[test]
    fn changed_columns_only_lists_real_differences() {
        let base = FotoRow {
            id: 1,
            pekerjaan_id: Some(7),
            komponen_id: 2,
            penerima_id: None,
            keterangan: "0%".into(),
            koordinat: "a".into(),
            validasi_koordinat: false,
            validasi_koordinat_message: None,
            unit_index: None,
            created_at: None,
            updated_at: None,
        };
        let mut next = base.clone();
        next.keterangan = "25%".into();
        let changed: Vec<&str> = COLUMNS
            .iter()
            .copied()
            .filter(|c| col_json(&base, c) != col_json(&next, c))
            .collect();
        assert_eq!(changed, vec!["keterangan"]);
    }
}
