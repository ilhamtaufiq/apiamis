//! `/api/sk`: port `SkController`, admin saja (`auth:sanctum,role:admin`).
//!
//! Daftar, tambah, detail, ubah, dan hapus SK. Berkas disimpan sebagai media koleksi `sk/dokumen`
//! (lihat `media.rs`). `Sk` memakai trait `Auditable` saja, tanpa `NotifiesAdminsOnChanges`,
//! jadi perubahan hanya diaudit dan tidak memicu notifikasi.
//!
//! Input boleh multipart (berkas) atau JSON (tanpa berkas), seperti validasi Laravel.
//! `POST /api/sk/{id}` dengan `_method=PUT` mengikuti method spoofing Laravel.

use std::{collections::BTreeMap, collections::HashMap, path::PathBuf};

use axum::{
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    audit, format::iso8601_utc, foto, lookup::carbon_json, media, pagination, require_auth,
    AppState,
};

/// Nama kelas Laravel, dipakai sebagai `auditable_type` dan `model_type` media.
pub const MODEL: &str = "App\\Models\\Sk";
pub const COLLECTION: &str = "sk/dokumen";
/// Batas body: sama dengan rute foto (berkas 50 MB ditambah ruang untuk field lain).
pub const BODY_LIMIT: usize = foto::BODY_LIMIT;
/// Pesan `Spatie\Permission\Middleware\RoleMiddleware` (belum diverifikasi: vendor tidak ada).
const FORBIDDEN_MESSAGE: &str = "User does not have the right roles.";
const DEFAULT_PER_PAGE: i64 = 20;
const MAX_PER_PAGE: i64 = 100;

const SELECT_SK: &str = "SELECT CAST(s.id AS SIGNED) AS id, s.nomor_sk, s.nama, s.tanggal_sk, \
     CAST(s.uploaded_by AS SIGNED) AS uploaded_by, s.created_at, s.updated_at, \
     CAST(u.id AS SIGNED) AS uploader_id, u.name AS uploader_name, u.email AS uploader_email \
     FROM sk s LEFT JOIN users u ON u.id = s.uploaded_by";

/// Relasi `uploader` (dimuat selalu, seperti `with('uploader')`).
#[derive(Debug, Clone, PartialEq)]
pub struct Uploader {
    pub id: i64,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// Baris `sk` beserta uploader-nya.
#[derive(Debug, Clone, PartialEq)]
pub struct SkRow {
    pub id: i64,
    pub nomor_sk: String,
    pub nama: String,
    pub tanggal_sk: Option<NaiveDate>,
    pub uploaded_by: Option<i64>,
    pub uploader: Option<Uploader>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &MySqlRow) -> Result<SkRow, sqlx::Error> {
    let uploader = match r.try_get::<Option<i64>, _>("uploader_id")? {
        Some(id) => Some(Uploader {
            id,
            name: r.try_get("uploader_name")?,
            email: r.try_get("uploader_email")?,
        }),
        None => None,
    };
    Ok(SkRow {
        id: r.try_get("id")?,
        nomor_sk: r.try_get("nomor_sk")?,
        nama: r.try_get("nama")?,
        tanggal_sk: r.try_get("tanggal_sk")?,
        uploaded_by: r.try_get("uploaded_by")?,
        uploader,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<SkRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_SK} WHERE s.id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

/// Tanggal sebagai `Y-m-d`, seperti kolom `date` di MySQL.
fn date_json(d: Option<NaiveDate>) -> Value {
    match d {
        Some(d) => Value::String(d.format("%Y-%m-%d").to_string()),
        None => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(row: &SkRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("nomor_sk".into(), json!(row.nomor_sk));
    m.insert("nama".into(), json!(row.nama));
    m.insert("tanggal_sk".into(), date_json(row.tanggal_sk));
    m.insert("uploaded_by".into(), json!(row.uploaded_by));
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `getDirty()` dan `getRawOriginal()` untuk audit `updated`. `updated_at` ikut karena
/// Laravel menyetelnya sebelum event `updated`.
fn dirty_maps(old: &SkRow, new: &SkRow) -> (Map<String, Value>, Map<String, Value>) {
    let mut o = Map::new();
    let mut n = Map::new();
    if old.nomor_sk != new.nomor_sk {
        o.insert("nomor_sk".into(), json!(old.nomor_sk));
        n.insert("nomor_sk".into(), json!(new.nomor_sk));
    }
    if old.nama != new.nama {
        o.insert("nama".into(), json!(old.nama));
        n.insert("nama".into(), json!(new.nama));
    }
    if old.tanggal_sk != new.tanggal_sk {
        o.insert("tanggal_sk".into(), date_json(old.tanggal_sk));
        n.insert("tanggal_sk".into(), date_json(new.tanggal_sk));
    }
    o.insert("updated_at".into(), carbon_json(old.updated_at));
    n.insert("updated_at".into(), carbon_json(new.updated_at));
    (o, n)
}

fn uploader_json(u: Option<&Uploader>) -> Value {
    match u {
        Some(u) => json!({ "id": u.id, "name": u.name, "email": u.email }),
        None => Value::Null,
    }
}

/// `SkResource`. `first` dan `file_url` berasal dari media pertama koleksi `sk/dokumen`.
fn resource_value(
    row: &SkRow,
    first: Option<&media::MediaInfo>,
    file_url: Option<String>,
) -> Value {
    json!({
        "id": row.id,
        "nomor_sk": row.nomor_sk,
        "nama": row.nama,
        "tanggal_sk": date_json(row.tanggal_sk),
        "uploaded_by": row.uploaded_by,
        "file_url": file_url,
        "file_name": first.map(|m| m.file_name.clone()),
        "mime_type": first.map(|m| m.mime_type.clone()),
        "size": first.map(|m| m.size),
        "media_id": first.map(|m| m.id),
        "uploader": uploader_json(row.uploader.as_ref()),
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    })
}

/// `SkResource` lengkap dengan URL berkas dari tabel `media`.
pub async fn resource(pool: &MySqlPool, app_url: &str, row: &SkRow) -> Result<Value, ApiError> {
    let first = media::first_media(pool, MODEL, row.id as u64, COLLECTION)
        .await
        .map_err(media::internal)?;
    let file_url = if first.is_some() {
        let (url, _thumb) = media::first_urls(pool, app_url, MODEL, row.id as u64, COLLECTION)
            .await
            .map_err(media::internal)?;
        (!url.is_empty()).then_some(url)
    } else {
        None
    };
    Ok(resource_value(row, first.as_ref(), file_url))
}

// ---------------------------------------------------------------------------
// Otorisasi dan input
// ---------------------------------------------------------------------------

/// Setara middleware `role:admin`: 403 bila user tidak punya role `admin`.
async fn ensure_admin(state: &AppState, user_id: u64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(&state.pool, user_id)
        .await
        .map_err(media::internal)?;
    if roles.iter().any(|(_, name)| name == "admin") {
        return Ok(());
    }
    Err(ApiError::new(StatusCode::FORBIDDEN, FORBIDDEN_MESSAGE))
}

fn bad_multipart(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan multipart tidak valid: {e}"),
    )
}

fn bad_json(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan JSON tidak valid: {e}"),
    )
}

/// Field teks dan berkas dari multipart atau JSON. `Null` berarti tidak dikirim atau kosong
/// (`ConvertEmptyStringsToNull`, string di-trim seperti `TrimStrings`).
struct Input {
    fields: Map<String, Value>,
    file: Option<media::Upload>,
}

fn normalize(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        other => other,
    }
}

async fn read_input(state: &AppState, req: Request) -> Result<Input, ApiError> {
    let is_multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("multipart/form-data")
        });
    if is_multipart {
        let multipart = Multipart::from_request(req, state)
            .await
            .map_err(bad_multipart)?;
        let raw = foto::read_form(multipart).await?;
        let fields = raw
            .fields
            .into_iter()
            .map(|(k, v)| (k, v.map_or(Value::Null, Value::String)))
            .collect();
        return Ok(Input {
            fields,
            file: raw.file,
        });
    }

    let bytes = axum::body::to_bytes(req.into_body(), BODY_LIMIT)
        .await
        .map_err(bad_json)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Input {
            fields: Map::new(),
            file: None,
        });
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(bad_json)?;
    let fields = match value {
        Value::Object(o) => o.into_iter().map(|(k, v)| (k, normalize(v))).collect(),
        _ => Map::new(),
    };
    Ok(Input { fields, file: None })
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Store,
    Update,
}

type Errors = BTreeMap<String, Vec<String>>;

fn attribute(key: &str) -> String {
    key.replace('_', " ")
}

fn invalid(errs: Errors) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

/// `required|string|max:255`.
fn string_field(fields: &Map<String, Value>, key: &str, errs: &mut Errors) -> Option<String> {
    match fields.get(key) {
        None | Some(Value::Null) => {
            foto::add(
                errs,
                key,
                format!("The {} field is required.", attribute(key)),
            );
            None
        }
        Some(Value::String(s)) if s.chars().count() <= 255 => Some(s.clone()),
        Some(Value::String(_)) => {
            foto::add(
                errs,
                key,
                format!(
                    "The {} field must not be greater than 255 characters.",
                    attribute(key)
                ),
            );
            None
        }
        Some(_) => {
            foto::add(
                errs,
                key,
                format!("The {} field must be a string.", attribute(key)),
            );
            None
        }
    }
}

/// Bentuk tanggal yang diterima: `Y-m-d`, RFC 3339, dan `Y-m-d H:i:s` / `Y-m-dTH:i:s`.
/// Laravel `date` memakai `strtotime` dan menerima lebih banyak format (batasan yang dicatat).
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .or_else(|| DateTime::parse_from_rfc3339(s).ok().map(|d| d.date_naive()))
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|d| d.date())
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|d| d.date())
        })
}

/// `nullable|date`. Tidak dikirim atau null menghasilkan `None`.
fn date_field(fields: &Map<String, Value>, key: &str, errs: &mut Errors) -> Option<NaiveDate> {
    match fields.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match parse_date(s) {
            Some(d) => Some(d),
            None => {
                foto::add(
                    errs,
                    key,
                    format!("The {} field must be a valid date.", attribute(key)),
                );
                None
            }
        },
        Some(_) => {
            foto::add(
                errs,
                key,
                format!("The {} field must be a valid date.", attribute(key)),
            );
            None
        }
    }
}

struct Valid {
    nomor_sk: String,
    nama: String,
    tanggal_sk: Option<NaiveDate>,
    file: Option<media::Upload>,
}

/// `store`: `file` wajib. `update`: `file` opsional.
fn validate(input: Input, mode: Mode) -> Result<Valid, ApiError> {
    let mut errs = Errors::new();
    let nomor_sk = string_field(&input.fields, "nomor_sk", &mut errs);
    let nama = string_field(&input.fields, "nama", &mut errs);
    let tanggal_sk = date_field(&input.fields, "tanggal_sk", &mut errs);

    let mut file = input.file;
    let text_file = input.fields.get("file").is_some_and(|v| !v.is_null());
    if file
        .as_ref()
        .is_some_and(|u| u.bytes.len() > media::MAX_FILE_BYTES)
    {
        foto::add(
            &mut errs,
            "file",
            "The file field must not be greater than 51200 kilobytes.".into(),
        );
        file = None;
    } else if file.is_none() {
        if text_file {
            foto::add(&mut errs, "file", "The file field must be a file.".into());
        } else if mode == Mode::Store {
            foto::add(&mut errs, "file", "The file field is required.".into());
        }
    }

    if !errs.is_empty() {
        return Err(invalid(errs));
    }
    let (Some(nomor_sk), Some(nama)) = (nomor_sk, nama) else {
        return Err(media::internal("validasi SK tidak lengkap"));
    };
    Ok(Valid {
        nomor_sk,
        nama,
        tanggal_sk,
        file,
    })
}

/// `Str::uuid()`-style nama berkas dan `usingName($nama)`: media dibuat lalu namanya diganti `nama`.
/// Bila gagal, direktori berkas yang sudah ditulis dihapus. Pemanggil menghapus direktori
/// yang dikembalikan bila commit gagal.
async fn attach_named(
    tx: &mut Transaction<'_, MySql>,
    id: i64,
    upload: &media::Upload,
    name: &str,
) -> Result<PathBuf, ApiError> {
    let mime = media::mime_for_name(&upload.original_name);
    let stored = media::attach(tx, MODEL, id as u64, COLLECTION, upload, mime, false).await?;
    let renamed = sqlx::query("UPDATE media SET name = ? WHERE id = ?")
        .bind(name)
        .bind(stored.media_id)
        .execute(&mut **tx)
        .await;
    if let Err(e) = renamed {
        media::remove_dirs(std::slice::from_ref(&stored.dir)).await;
        return Err(media::internal(e));
    }
    Ok(stored.dir)
}

#[allow(clippy::too_many_arguments)]
async fn log_audit(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: MODEL,
            auditable_id: id as u64,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(media::internal)
}

/// Respon `{ "data": SkResource }` untuk satu SK.
async fn respond_one(state: &AppState, id: i64) -> Result<Response, ApiError> {
    let row = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `(int)` PHP untuk query `per_page`: angka di depan, selain itu 0.
fn php_int(raw: &str) -> i64 {
    let t = raw.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n = if digits.is_empty() {
        0
    } else {
        digits.parse::<i64>().unwrap_or(i64::MAX)
    };
    if neg {
        -n
    } else {
        n
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/sk`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state, user.user_id).await?;

    let per_page = match query.get("per_page") {
        Some(v) => php_int(v),
        None => DEFAULT_PER_PAGE,
    }
    .clamp(1, MAX_PER_PAGE) as u64;
    let page = pagination::page_params(&query).page;
    let search = query
        .get("search")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let (where_sql, pattern) = match &search {
        Some(s) => (
            " WHERE (s.nomor_sk LIKE ? OR s.nama LIKE ?)",
            Some(format!("%{s}%")),
        ),
        None => ("", None),
    };

    let count_sql = format!("SELECT COUNT(*) FROM sk s{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(p) = &pattern {
        cq = cq.bind(p.clone()).bind(p.clone());
    }
    let total = cq.fetch_one(&state.pool).await.map_err(media::internal)?;

    let sql = format!("{SELECT_SK}{where_sql} ORDER BY s.id DESC LIMIT ? OFFSET ?");
    let mut q = sqlx::query(&sql);
    if let Some(p) = &pattern {
        q = q.bind(p.clone()).bind(p.clone());
    }
    let offset = (page - 1).saturating_mul(per_page);
    let rows = q
        .bind(per_page as i64)
        .bind(offset as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(media::internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(media::internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(resource(&state.pool, &state.app_url, row).await?);
    }
    // Paginator Laravel tanpa `appends`: tautan hanya membawa `page`.
    let base = format!("{}/api/sk", foto::base_url(&state));
    Ok(Json(pagination::paginate(
        data,
        total as u64,
        pagination::PageParams { page, per_page },
        &base,
    ))
    .into_response())
}

/// `POST /api/sk`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state, user.user_id).await?;
    let input = read_input(&state, req).await?;
    let valid = validate(input, Mode::Store)?;
    let upload = valid
        .file
        .as_ref()
        .ok_or_else(|| media::internal("berkas SK hilang setelah validasi"))?;

    let url = format!("{}/api/sk", foto::base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let id = sqlx::query(
        "INSERT INTO sk (nomor_sk, nama, tanggal_sk, uploaded_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(&valid.nomor_sk)
    .bind(&valid.nama)
    .bind(valid.tanggal_sk)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await
    .map_err(media::internal)?
    .last_insert_id() as i64;

    let row = find_row(&mut *tx, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(|| media::internal("sk hilang setelah insert"))?;
    log_audit(
        &mut tx,
        &headers,
        user.user_id,
        "created",
        id,
        None,
        Some(attributes(&row)),
        &url,
    )
    .await?;

    let new_dir = attach_named(&mut tx, id, upload, &valid.nama).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[new_dir]).await;
        return Err(media::internal(e));
    }
    respond_one(&state, id).await
}

/// `GET /api/sk/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state, user.user_id).await?;
    let id = foto::parse_id(&id)?;
    respond_one(&state, id).await
}

/// `PUT/PATCH /api/sk/{id}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response, ApiError> {
    update_impl(state, headers, id, req, false).await
}

/// `POST /api/sk/{id}` dengan `_method=PUT`, seperti method spoofing Laravel.
pub async fn update_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response, ApiError> {
    update_impl(state, headers, id, req, true).await
}

async fn update_impl(
    state: AppState,
    headers: HeaderMap,
    id: String,
    req: Request,
    require_method_override: bool,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state, user.user_id).await?;
    let id = foto::parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;

    let input = read_input(&state, req).await?;
    if require_method_override && input.fields.get("_method") != Some(&json!("PUT")) {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "The POST method is not supported for this route. Supported methods: GET, HEAD, PUT, PATCH, DELETE.",
        ));
    }
    let valid = validate(input, Mode::Update)?;

    let url = format!("{}/api/sk/{id}", foto::base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;

    // `update([...])` menyetel `tanggal_sk` ke null bila tidak dikirim (`?? null`).
    let changed = valid.nomor_sk != current.nomor_sk
        || valid.nama != current.nama
        || valid.tanggal_sk != current.tanggal_sk;
    if changed {
        sqlx::query(
            "UPDATE sk SET nomor_sk = ?, nama = ?, tanggal_sk = ?, updated_at = NOW() WHERE id = ?",
        )
        .bind(&valid.nomor_sk)
        .bind(&valid.nama)
        .bind(valid.tanggal_sk)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(media::internal)?;
        let after = find_row(&mut *tx, id)
            .await
            .map_err(media::internal)?
            .ok_or_else(|| media::internal("sk hilang saat update"))?;
        let (old, new) = dirty_maps(&current, &after);
        log_audit(
            &mut tx,
            &headers,
            user.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }

    // `clearMediaCollection` lalu `addMediaFromRequest`. Berkas lama dihapus setelah commit.
    let mut old_dirs = Vec::new();
    let mut new_dir = None;
    if let Some(upload) = valid.file.as_ref() {
        old_dirs = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
        new_dir = Some(attach_named(&mut tx, id, upload, &valid.nama).await?);
    }
    if let Err(e) = tx.commit().await {
        if let Some(dir) = new_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(media::internal(e));
    }
    media::remove_dirs(&old_dirs).await;
    respond_one(&state, id).await
}

/// `DELETE /api/sk/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state, user.user_id).await?;
    let id = foto::parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(ApiError::not_found)?;

    let url = format!("{}/api/sk/{id}", foto::base_url(&state));
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let dirs: Vec<PathBuf> =
        media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM sk WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(media::internal)?;
    log_audit(
        &mut tx,
        &headers,
        user.user_id,
        "deleted",
        id,
        Some(attributes(&current)),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(media::internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({ "message": "SK deleted successfully" })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_int_reads_leading_digits_like_php_cast() {
        assert_eq!(php_int("25"), 25);
        assert_eq!(php_int(" 12abc"), 12);
        assert_eq!(php_int("abc"), 0);
        assert_eq!(php_int(""), 0);
        assert_eq!(php_int("-5"), -5);
        assert_eq!(php_int("+7"), 7);
    }

    #[test]
    fn per_page_is_clamped_like_laravel() {
        let clamp = |raw: &str| php_int(raw).clamp(1, MAX_PER_PAGE);
        assert_eq!(clamp("0"), 1);
        assert_eq!(clamp("500"), 100);
        assert_eq!(clamp("abc"), 1);
    }

    #[test]
    fn parses_common_date_forms() {
        assert_eq!(
            parse_date("2026-01-05"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        assert_eq!(
            parse_date("2026-01-05T10:00:00+07:00"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        assert_eq!(
            parse_date("2026-01-05 10:00:00"),
            NaiveDate::from_ymd_opt(2026, 1, 5)
        );
        assert_eq!(parse_date("bukan tanggal"), None);
    }

    #[test]
    fn store_requires_file_and_update_does_not() {
        let fields: Map<String, Value> = [
            ("nomor_sk".to_string(), json!("SK-1/2026")),
            ("nama".to_string(), json!("Pengangkatan")),
        ]
        .into_iter()
        .collect();
        let input = || Input {
            fields: fields.clone(),
            file: None,
        };
        let err = validate(input(), Mode::Store).err().unwrap();
        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(validate(input(), Mode::Update).is_ok());
    }

    #[test]
    fn missing_required_fields_are_reported_with_laravel_messages() {
        let mut errs = Errors::new();
        let fields = Map::new();
        assert_eq!(string_field(&fields, "nomor_sk", &mut errs), None);
        assert_eq!(
            errs["nomor_sk"],
            vec!["The nomor sk field is required.".to_string()]
        );
    }

    #[test]
    fn resource_without_media_has_nulls_like_laravel() {
        let row = SkRow {
            id: 3,
            nomor_sk: "SK-3".into(),
            nama: "Uji".into(),
            tanggal_sk: NaiveDate::from_ymd_opt(2026, 2, 1),
            uploaded_by: None,
            uploader: None,
            created_at: None,
            updated_at: None,
        };
        let v = resource_value(&row, None, None);
        assert_eq!(v["tanggal_sk"], json!("2026-02-01"));
        assert_eq!(v["file_url"], Value::Null);
        assert_eq!(v["media_id"], Value::Null);
        assert_eq!(v["uploader"], Value::Null);
    }

    #[test]
    fn updated_audit_includes_only_dirty_columns_and_updated_at() {
        let old = SkRow {
            id: 1,
            nomor_sk: "A".into(),
            nama: "N".into(),
            tanggal_sk: None,
            uploaded_by: Some(2),
            uploader: None,
            created_at: None,
            updated_at: None,
        };
        let mut new = old.clone();
        new.nama = "B".into();
        let (o, n) = dirty_maps(&old, &new);
        assert!(o.contains_key("nama") && n.contains_key("nama"));
        assert!(!o.contains_key("nomor_sk"));
        assert!(o.contains_key("updated_at") && n.contains_key("updated_at"));
    }
}
