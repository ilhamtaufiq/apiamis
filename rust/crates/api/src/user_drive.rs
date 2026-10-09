//! `/api/user-drive`: port `UserDriveController` dan `UserDriveItemResource`.
//!
//! Rute: daftar, buat folder, unggah berkas, detail, ganti nama, bagikan, hapus, dan hapus massal.
//! Akses: `auth:sanctum` saja. Pemilik atau admin boleh mengubah item (`canManage`); daftar menampilkan
//! item milik sendiri, item yang dibagikan, dan semua item untuk admin.
//!
//! Berbeda dari Laravel atau belum dicocokkan dengan Laravel (vendor tidak ada):
//! - MIME berkas diambil dari ekstensi nama asli (`media::mime_for_name`), bukan dari isi berkas (`finfo`).
//! - Batas unggahan `max:204800` KB dipasang sebagai `DefaultBodyLimit` pada `POST /api/user-drive/files`
//!   dan berkas dibaca utuh ke memori. Batas PHP (`upload_max_filesize`, `post_max_size`) belum dicek.
//! - `index` mengurutkan folder dulu, lalu `updated_at` menurun, lalu `id` menurun. `id` hanya pemutus seri.
//! - Pesan 422 di tingkat atas memakai pesan pertama (`validation::Errors`). Teks pesan Laravel belum dicek.
//! - Hapus rekursif dan hapus massal mengikuti urutan Laravel. Bila id dan turunannya sama-sama dipilih,
//!   Laravel memanggil `delete()` lagi pada model yang sudah terhapus, sehingga audit `deleted` ganda dan
//!   `deleted` ikut terhitung dua kali. Perilaku ini dipertahankan.
//! - `owner` hanya ada di `index`, seperti `whenLoaded('user')`.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    changes, foto, format::iso8601_utc, lookup::carbon_json, media,
    pagination::{self, PageParams},
    require_auth,
    validation::Errors,
    AppState,
};

/// Nama kelas Laravel, dipakai sebagai `model_type` media dan `auditable_type`.
const MODEL: &str = "App\\Models\\UserDriveItem";
const COLLECTION: &str = "drive-file";
const KIND_FOLDER: &str = "folder";
const KIND_FILE: &str = "file";
const DEFAULT_PER_PAGE: u64 = 48;
/// Aturan `max:204800` pada `file`, dalam KB.
const MAX_UPLOAD_KB: usize = 204_800;
/// Batas body rute unggah: berkas maksimum ditambah ruang untuk bagian multipart lain.
pub const BODY_LIMIT: usize = MAX_UPLOAD_KB * 1024 + 1024 * 1024;

const SELECT_ITEM: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, \
     CAST(parent_id AS SIGNED) AS parent_id, name, kind, original_filename, created_at, updated_at, deleted_at \
     FROM user_drive_items";

/// Baris `user_drive_items`. Kolom id dibaca sebagai `i64` (lihat `CAST ... AS SIGNED`).
#[derive(Debug, Clone)]
struct Item {
    id: i64,
    user_id: i64,
    parent_id: Option<i64>,
    name: String,
    kind: String,
    original_filename: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

impl Item {
    fn is_folder(&self) -> bool {
        self.kind == KIND_FOLDER
    }
}

fn map_item(r: &MySqlRow) -> Result<Item, sqlx::Error> {
    Ok(Item {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        parent_id: r.try_get("parent_id")?,
        name: r.try_get("name")?,
        kind: r.try_get("kind")?,
        original_filename: r.try_get("original_filename")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        deleted_at: r.try_get("deleted_at")?,
    })
}

/// Pengguna yang sedang login dan apakah ia admin (`hasRole('admin')`).
#[derive(Debug, Clone, Copy)]
struct Viewer {
    id: i64,
    admin: bool,
}

impl Viewer {
    /// `UserDriveItem::canManage`: pemilik, atau admin.
    fn can_manage(&self, item: &Item) -> bool {
        item.user_id == self.id || self.admin
    }
}

/// Bearer token, lalu role user. Dipakai semua handler.
async fn viewer(state: &AppState, headers: &HeaderMap) -> Result<Viewer, ApiError> {
    let user = require_auth(state, headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(media::internal)?;
    Ok(Viewer {
        id: user.user_id as i64,
        admin: roles.iter().any(|(_, name)| name.as_str() == "admin"),
    })
}

fn not_found_model(raw_id: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        format!("No query results for model [{}] {}", MODEL, raw_id),
    )
}

fn forbidden() -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "Forbidden")
}

fn url_for(state: &AppState, path: &str) -> String {
    format!("{}{}", foto::base_url(state), path)
}

fn data_response(status: StatusCode, data: Value) -> Response {
    (status, Json(json!({ "data": data }))).into_response()
}

/// Waktu mentah seperti `DB::table` (`Y-m-d H:i:s`), dipakai untuk nilai lama pada audit `updated`.
fn raw_ts(ts: Option<DateTime<Utc>>) -> Value {
    match ts {
        Some(t) => Value::String(t.format("%Y-%m-%d %H:%M:%S").to_string()),
        None => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(item: &Item) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(item.id));
    m.insert("user_id".into(), json!(item.user_id));
    m.insert("parent_id".into(), json!(item.parent_id));
    m.insert("name".into(), json!(item.name));
    m.insert("kind".into(), json!(item.kind));
    m.insert("original_filename".into(), json!(item.original_filename));
    m.insert("created_at".into(), carbon_json(item.created_at));
    m.insert("updated_at".into(), carbon_json(item.updated_at));
    m
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// `integer` Laravel: `filter_var(FILTER_VALIDATE_INT)`. Tanda opsional, tanpa nol di depan.
fn strict_int(raw: &str) -> Option<i64> {
    let (sign, digits) = match raw.as_bytes().first().copied() {
        Some(b'-') => ("-", &raw[1..]),
        Some(b'+') => ("", &raw[1..]),
        _ => ("", raw),
    };
    let ok = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if !ok {
        return None;
    }
    format!("{sign}{digits}").parse::<i64>().ok()
}

/// `nullable|integer` untuk teks (query string atau field multipart). Kosong dianggap tidak ada.
fn int_text(e: &mut Errors, field: &str, raw: Option<&str>) -> Option<i64> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty())?;
    match strict_int(raw) {
        Some(v) => Some(v),
        None => {
            e.add(field, format!("The {} field must be an integer.", attr(field)));
            None
        }
    }
}

/// `nullable|integer` untuk nilai JSON.
fn json_int(e: &mut Errors, field: &str, v: Option<&Value>) -> Option<i64> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => Some(i),
            None => {
                e.add(field, format!("The {} field must be an integer.", attr(field)));
                None
            }
        },
        Some(Value::String(s)) => int_text(e, field, Some(s.as_str())),
        Some(_) => {
            e.add(field, format!("The {} field must be an integer.", attr(field)));
            None
        }
    }
}

/// `required|string|max:N` untuk nilai JSON. Teks di-trim dan kosong dianggap tidak ada.
fn required_string(e: &mut Errors, field: &str, v: Option<&Value>, max: usize) -> Option<String> {
    let a = attr(field);
    match v {
        None | Some(Value::Null) => {
            e.add(field, format!("The {} field is required.", a));
            None
        }
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                e.add(field, format!("The {} field is required.", a));
                None
            } else if t.chars().count() > max {
                e.add(
                    field,
                    format!("The {} field must not be greater than {} characters.", a, max),
                );
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(_) => {
            e.add(field, format!("The {} field must be a string.", a));
            None
        }
    }
}

/// Aturan `exists:user_drive_items,id` dengan `where user_id = ? and kind = folder`.
/// Query builder Laravel tidak memakai scope soft delete, jadi folder terhapus tetap lolos di sini.
async fn is_own_folder(pool: &MySqlPool, owner: i64, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_drive_items WHERE id = ? AND user_id = ? AND kind = ?",
    )
    .bind(id)
    .bind(owner)
    .bind(KIND_FOLDER)
    .fetch_one(pool)
    .await
    .map_err(media::internal)?;
    Ok(n > 0)
}

/// Pesan validasi `parent_id` bila folder induk tidak valid.
fn parent_invalid(e: &mut Errors) {
    e.add("parent_id", "The selected parent id is invalid.");
}

// ---------------------------------------------------------------------------
// Pencarian dan resource
// ---------------------------------------------------------------------------

async fn find_live<'e, E>(exec: E, id: i64) -> Result<Option<Item>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{} WHERE id = ? AND deleted_at IS NULL", SELECT_ITEM);
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_item).transpose()
}

/// Route model binding `UserDriveItem`: item tidak ada atau terhapus, 404 dengan pesan Laravel.
async fn bound_item(pool: &MySqlPool, raw_id: &str) -> Result<Item, ApiError> {
    let id = raw_id.parse::<i64>().map_err(|_| not_found_model(raw_id))?;
    find_live(pool, id)
        .await
        .map_err(media::internal)?
        .ok_or_else(|| not_found_model(raw_id))
}

async fn owner_json(pool: &MySqlPool, user_id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query("SELECT CAST(id AS SIGNED) AS id, name FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(media::internal)?;
    match row {
        Some(r) => {
            let id: i64 = r.try_get("id").map_err(media::internal)?;
            let name: Option<String> = r.try_get("name").map_err(media::internal)?;
            Ok(json!({ "id": id, "name": name }))
        }
        None => Ok(Value::Null),
    }
}

/// `UserDriveItemResource::toArray`. `owner` hanya ada bila relasi `user` dimuat (`index`).
async fn resource(
    pool: &MySqlPool,
    app_url: &str,
    me: Viewer,
    item: &Item,
    with_owner: bool,
) -> Result<Value, ApiError> {
    let mut file_url: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut file_size: Option<u64> = None;
    let mut media_id: Option<u64> = None;
    if item.kind == KIND_FILE {
        let uid = item.id as u64;
        if let Some(info) = media::first_media(pool, MODEL, uid, COLLECTION)
            .await
            .map_err(media::internal)?
        {
            let (url, _thumb) = media::first_urls(pool, app_url, MODEL, uid, COLLECTION)
                .await
                .map_err(media::internal)?;
            file_url = (!url.is_empty()).then_some(url);
            mime_type = Some(info.mime_type);
            file_size = Some(info.size);
            media_id = Some(info.id);
        }
    }

    // `shares->contains(fn ($s) => $s->shared_to_user_id === null)`
    let shared_to_all: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_drive_shares WHERE item_id = ? AND shared_to_user_id IS NULL",
    )
    .bind(item.id)
    .fetch_one(pool)
    .await
    .map_err(media::internal)?;

    let mut out = Map::new();
    out.insert("id".into(), json!(item.id));
    out.insert("parent_id".into(), json!(item.parent_id));
    out.insert("name".into(), json!(item.name));
    out.insert("kind".into(), json!(item.kind));
    out.insert("original_filename".into(), json!(item.original_filename));
    out.insert("file_url".into(), json!(file_url));
    out.insert("mime_type".into(), json!(mime_type));
    out.insert("file_size".into(), json!(file_size));
    out.insert("media_id".into(), json!(media_id));
    out.insert("can_manage".into(), json!(me.can_manage(item)));
    if with_owner {
        out.insert("owner".into(), owner_json(pool, item.user_id).await?);
    }
    out.insert("is_owner".into(), json!(item.user_id == me.id));
    out.insert("shared_to_all".into(), json!(shared_to_all > 0));
    out.insert("created_at".into(), iso8601_utc(item.created_at));
    out.insert("updated_at".into(), iso8601_utc(item.updated_at));
    Ok(Value::Object(out))
}

/// Bind dinamis untuk query `index`.
enum Bind {
    I(i64),
    S(String),
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/user-drive`: paginasi `paginate(per_page)`, tanpa parameter tambahan di tautan halaman.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;

    let mut e = Errors::default();
    let parent_id = int_text(&mut e, "parent_id", query.get("parent_id").map(String::as_str));
    let search = query
        .get("search")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    if let Some(s) = search {
        if s.chars().count() > 100 {
            e.add("search", "The search field must not be greater than 100 characters.");
        }
    }
    let page = int_text(&mut e, "page", query.get("page").map(String::as_str));
    if let Some(p) = page {
        if p < 1 {
            e.add("page", "The page field must be at least 1.");
        }
    }
    let per_page = int_text(&mut e, "per_page", query.get("per_page").map(String::as_str));
    if let Some(p) = per_page {
        if p < 1 {
            e.add("per_page", "The per page field must be at least 1.");
        } else if p > 100 {
            e.add("per_page", "The per page field must not be greater than 100.");
        }
    }
    e.finish()?;

    // `empty("0")` di PHP bernilai true, jadi pencarian "0" dilewati seperti Laravel.
    let search_term = search.filter(|s| !s.is_empty() && *s != "0");
    let page = page.unwrap_or(1) as u64;
    let per_page = per_page.map(|p| p as u64).unwrap_or(DEFAULT_PER_PAGE);
    let offset = (page - 1).saturating_mul(per_page);

    let mut clauses: Vec<String> = vec!["deleted_at IS NULL".to_string()];
    let mut binds: Vec<Bind> = Vec::new();
    if !me.admin {
        clauses.push(
            "(user_id = ? OR EXISTS (SELECT 1 FROM user_drive_shares s WHERE s.item_id = user_drive_items.id \
             AND (s.shared_to_user_id IS NULL OR s.shared_to_user_id = ?)))"
                .to_string(),
        );
        binds.push(Bind::I(me.id));
        binds.push(Bind::I(me.id));
    }
    match parent_id {
        Some(p) => {
            clauses.push("parent_id = ?".to_string());
            binds.push(Bind::I(p));
        }
        None => clauses.push("parent_id IS NULL".to_string()),
    }
    if let Some(term) = search_term {
        clauses.push("(name LIKE ? OR original_filename LIKE ?)".to_string());
        let like = format!("%{}%", term);
        binds.push(Bind::S(like.clone()));
        binds.push(Bind::S(like));
    }
    let where_sql = clauses.join(" AND ");

    let count_sql = format!("SELECT COUNT(*) FROM user_drive_items WHERE {}", where_sql);
    let mut cq = sqlx::query(&count_sql);
    for b in &binds {
        cq = match b {
            Bind::I(v) => cq.bind(*v),
            Bind::S(s) => cq.bind(s.clone()),
        };
    }
    let count_row = cq.fetch_one(&state.pool).await.map_err(media::internal)?;
    let total: i64 = count_row.try_get(0).map_err(media::internal)?;

    let list_sql = format!(
        "{} WHERE {} ORDER BY CASE WHEN kind = 'folder' THEN 0 ELSE 1 END, updated_at DESC, id DESC LIMIT {} OFFSET {}",
        SELECT_ITEM, where_sql, per_page, offset
    );
    let mut lq = sqlx::query(&list_sql);
    for b in &binds {
        lq = match b {
            Bind::I(v) => lq.bind(*v),
            Bind::S(s) => lq.bind(s.clone()),
        };
    }
    let rows = lq.fetch_all(&state.pool).await.map_err(media::internal)?;

    let mut data: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let item = map_item(r).map_err(media::internal)?;
        data.push(resource(&state.pool, &state.app_url, me, &item, true).await?);
    }

    let base = format!("{}/api/user-drive", foto::base_url(&state));
    let body = pagination::paginate_laravel(
        data,
        total as u64,
        PageParams { page, per_page },
        &base,
        &|p| format!("{}?page={}", base, p),
    );
    Ok(Json(body).into_response())
}

/// Validasi `storeFolder` dan `storeFile` atas `parent_id`: harus folder milik user (`exists`), lalu
/// `findOrFail` yang menolak folder terhapus dengan 404.
async fn load_parent(
    state: &AppState,
    me: Viewer,
    parent_id: i64,
) -> Result<Item, ApiError> {
    match find_live(&state.pool, parent_id).await.map_err(media::internal)? {
        Some(p) if p.user_id == me.id && p.is_folder() => Ok(p),
        _ => Err(not_found_model(&parent_id.to_string())),
    }
}

/// `POST /api/user-drive/folders`: 201.
pub async fn store_folder(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let input = parse_body(&body);

    let mut e = Errors::default();
    let name = required_string(&mut e, "name", input.get("name"), 255);
    let parent_id = json_int(&mut e, "parent_id", input.get("parent_id"));
    if let Some(pid) = parent_id {
        if !is_own_folder(&state.pool, me.id, pid).await? {
            parent_invalid(&mut e);
        }
    }
    e.finish()?;
    let name = name.unwrap_or_default();

    let parent = match parent_id {
        Some(pid) => Some(load_parent(&state, me, pid).await?),
        None => None,
    };
    let parent_id = parent.map(|p| p.id);

    let url = url_for(&state, "/api/user-drive/folders");
    let now = Utc::now();
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let res = sqlx::query(
        "INSERT INTO user_drive_items (user_id, parent_id, name, kind, original_filename, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NULL, ?, ?)",
    )
    .bind(me.id)
    .bind(parent_id)
    .bind(&name)
    .bind(KIND_FOLDER)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(media::internal)?;
    let id = res.last_insert_id() as i64;

    let item = Item {
        id,
        user_id: me.id,
        parent_id,
        name,
        kind: KIND_FOLDER.to_string(),
        original_filename: None,
        created_at: Some(now),
        updated_at: Some(now),
        deleted_at: None,
    };
    changes::audit_only(
        &mut tx,
        &headers,
        me.id as u64,
        MODEL,
        "created",
        id,
        None,
        Some(attributes(&item)),
        &url,
    )
    .await?;
    tx.commit().await.map_err(media::internal)?;

    let data = resource(&state.pool, &state.app_url, me, &item, false).await?;
    Ok(data_response(StatusCode::CREATED, data))
}

/// `POST /api/user-drive/files`: multipart dengan field `file`, `name` (opsional), dan `parent_id`. 201.
pub async fn store_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let mut raw = foto::read_form(multipart).await?;

    let name_raw = raw.fields.get("name").cloned().flatten();
    let parent_raw = raw.fields.get("parent_id").cloned().flatten();
    let upload = raw.file.take();

    let mut e = Errors::default();
    let upload = match upload {
        None => {
            e.add("file", "The file field is required.");
            None
        }
        Some(u) if u.bytes.len() > MAX_UPLOAD_KB * 1024 => {
            e.add(
                "file",
                format!(
                    "The file field must not be greater than {} kilobytes.",
                    MAX_UPLOAD_KB
                ),
            );
            None
        }
        Some(u) => Some(u),
    };
    let name = name_raw
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(n) = &name {
        if n.chars().count() > 255 {
            e.add("name", "The name field must not be greater than 255 characters.");
        }
    }
    let parent_id = int_text(&mut e, "parent_id", parent_raw.as_deref());
    if let Some(pid) = parent_id {
        if !is_own_folder(&state.pool, me.id, pid).await? {
            parent_invalid(&mut e);
        }
    }
    e.finish()?;
    let upload = upload.ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error"))?;

    // `$displayName` kosong memakai nama berkas tanpa ekstensi; `?: 'file'` juga mengganti "0".
    let display = match name {
        Some(n) => n,
        None => php_stem_or_file(&upload.original_name),
    };
    let parent_id = match parent_id {
        Some(pid) => Some(load_parent(&state, me, pid).await?.id),
        None => None,
    };

    let url = url_for(&state, "/api/user-drive/files");
    let now = Utc::now();
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let res = sqlx::query(
        "INSERT INTO user_drive_items (user_id, parent_id, name, kind, original_filename, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(me.id)
    .bind(parent_id)
    .bind(&display)
    .bind(KIND_FILE)
    .bind(&upload.original_name)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(media::internal)?;
    let id = res.last_insert_id() as i64;

    let item = Item {
        id,
        user_id: me.id,
        parent_id,
        name: display,
        kind: KIND_FILE.to_string(),
        original_filename: Some(upload.original_name.clone()),
        created_at: Some(now),
        updated_at: Some(now),
        deleted_at: None,
    };
    changes::audit_only(
        &mut tx,
        &headers,
        me.id as u64,
        MODEL,
        "created",
        id,
        None,
        Some(attributes(&item)),
        &url,
    )
    .await?;

    let mime = media::mime_for_name(&upload.original_name);
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &upload, mime, false).await?;
    if let Err(err) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(media::internal(err));
    }

    let data = resource(&state.pool, &state.app_url, me, &item, false).await?;
    Ok(data_response(StatusCode::CREATED, data))
}

/// `pathinfo($name, PATHINFO_FILENAME) ?: 'file'`: nama tanpa ekstensi di Linux, `file` bila kosong atau "0".
fn php_stem_or_file(original: &str) -> String {
    let base = original.rsplit('/').next().unwrap_or("");
    let stem = match base.rfind('.') {
        Some(i) => &base[..i],
        None => base,
    };
    if stem.is_empty() || stem == "0" {
        "file".to_string()
    } else {
        stem.to_string()
    }
}

/// `GET /api/user-drive/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let item = bound_item(&state.pool, &id).await?;
    if !me.can_manage(&item) {
        return Err(forbidden());
    }
    let data = resource(&state.pool, &state.app_url, me, &item, false).await?;
    Ok(data_response(StatusCode::OK, data))
}

/// `PUT /api/user-drive/{id}`: validasi dulu, lalu cek kepemilikan (urutan sama dengan Laravel).
pub async fn rename(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let item = bound_item(&state.pool, &id).await?;
    let input = parse_body(&body);

    let mut e = Errors::default();
    let name = required_string(&mut e, "name", input.get("name"), 255);
    e.finish()?;
    let name = name.unwrap_or_default();

    if !me.can_manage(&item) {
        return Err(forbidden());
    }

    // Tanpa perubahan nilai, Eloquent tidak menjalankan `update` dan tidak menulis audit.
    if name != item.name {
        let now = Utc::now();
        let url = url_for(&state, &format!("/api/user-drive/{}", item.id));
        let mut tx = state.pool.begin().await.map_err(media::internal)?;
        sqlx::query("UPDATE user_drive_items SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(now)
            .bind(item.id)
            .execute(&mut *tx)
            .await
            .map_err(media::internal)?;

        let mut old = Map::new();
        old.insert("name".into(), json!(item.name));
        old.insert("updated_at".into(), raw_ts(item.updated_at));
        let mut new = Map::new();
        new.insert("name".into(), json!(name));
        new.insert("updated_at".into(), carbon_json(Some(now)));
        changes::audit_only(
            &mut tx,
            &headers,
            me.id as u64,
            MODEL,
            "updated",
            item.id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
        tx.commit().await.map_err(media::internal)?;
    }

    let fresh = bound_item(&state.pool, &id).await?;
    let data = resource(&state.pool, &state.app_url, me, &fresh, false).await?;
    Ok(data_response(StatusCode::OK, data))
}

/// `POST /api/user-drive/{id}/share`. `user_id` kosong berarti dibagikan ke semua user.
pub async fn share(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let item = bound_item(&state.pool, &id).await?;
    let input = parse_body(&body);

    let mut e = Errors::default();
    let user_id = json_int(&mut e, "user_id", input.get("user_id"));
    if let Some(uid) = user_id {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&state.pool)
            .await
            .map_err(media::internal)?;
        if n == 0 {
            e.add("user_id", "The selected user id is invalid.");
        }
    }
    e.finish()?;

    if !me.can_manage(&item) {
        return Err(forbidden());
    }

    // `updateOrCreate` pada (item_id, shared_to_user_id). `<=>` mencocokkan NULL dengan NULL.
    let now = Utc::now();
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM user_drive_shares WHERE item_id = ? AND shared_to_user_id <=> ? LIMIT 1",
    )
    .bind(item.id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(media::internal)?;
    if existing.is_none() {
        sqlx::query(
            "INSERT INTO user_drive_shares (item_id, shared_to_user_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(item.id)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(media::internal)?;
    }
    tx.commit().await.map_err(media::internal)?;

    let fresh = bound_item(&state.pool, &id).await?;
    let data = resource(&state.pool, &state.app_url, me, &fresh, false).await?;
    Ok(data_response(StatusCode::OK, data))
}

/// `DELETE /api/user-drive/{id}`: hapus lunak item dan turunannya.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let item = bound_item(&state.pool, &id).await?;
    if !me.can_manage(&item) {
        return Err(forbidden());
    }

    let url = url_for(&state, &format!("/api/user-drive/{}", item.id));
    let now = Utc::now();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let deleted = delete_tree(&mut tx, &headers, me, &item, &url, now, &mut dirs).await?;
    tx.commit().await.map_err(media::internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({
        "success": true,
        "message": "Item drive berhasil dihapus",
        "deleted": deleted,
    }))
    .into_response())
}

/// `DELETE /api/user-drive/bulk`: `ids` wajib, array integer. Item yang tidak bisa dikelola dilewati.
pub async fn bulk_destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let me = viewer(&state, &headers).await?;
    let input = parse_body(&body);
    let ids = bulk_ids(&input)?;

    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "{} WHERE deleted_at IS NULL AND id IN ({}) ORDER BY id",
        SELECT_ITEM, placeholders
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(media::internal)?;
    let mut items: Vec<Item> = Vec::new();
    for r in &rows {
        let item = map_item(r).map_err(media::internal)?;
        if me.can_manage(&item) {
            items.push(item);
        }
    }
    if items.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Item drive tidak ditemukan",
        ));
    }

    let url = url_for(&state, "/api/user-drive/bulk");
    let now = Utc::now();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut tx = state.pool.begin().await.map_err(media::internal)?;
    let mut deleted: i64 = 0;
    for item in &items {
        deleted += delete_tree(&mut tx, &headers, me, item, &url, now, &mut dirs).await?;
    }
    tx.commit().await.map_err(media::internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({
        "success": true,
        "message": format!("{} item drive dihapus", deleted),
        "deleted": deleted,
    }))
    .into_response())
}

/// `ids` untuk hapus massal: wajib, array, tidak kosong, dan setiap elemen integer (`ids.*`).
fn bulk_ids(input: &Map<String, Value>) -> Result<Vec<i64>, ApiError> {
    let mut e = Errors::default();
    let mut ids: Vec<i64> = Vec::new();
    match input.get("ids") {
        None | Some(Value::Null) => e.add("ids", "The ids field is required."),
        Some(Value::Array(arr)) if arr.is_empty() => e.add("ids", "The ids field is required."),
        Some(Value::Array(arr)) => {
            for (i, v) in arr.iter().enumerate() {
                let field = format!("ids.{}", i);
                match v {
                    Value::Null => e.add(&field, format!("The {} field must be an integer.", field)),
                    _ => {
                        if let Some(id) = json_int(&mut e, &field, Some(v)) {
                            ids.push(id);
                        }
                    }
                }
            }
        }
        Some(_) => e.add("ids", "The ids field must be an array."),
    }
    e.finish()?;
    Ok(ids)
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

// ---------------------------------------------------------------------------
// Hapus rekursif
// ---------------------------------------------------------------------------

/// Anak langsung yang belum terhapus, milik pemilik folder (`ownedBy($item->user_id)`), urut id.
async fn live_children(tx: &mut Transaction<'_, MySql>, folder: &Item) -> Result<Vec<Item>, ApiError> {
    let sql = format!(
        "{} WHERE user_id = ? AND parent_id = ? AND deleted_at IS NULL ORDER BY id",
        SELECT_ITEM
    );
    let rows = sqlx::query(&sql)
        .bind(folder.user_id)
        .bind(folder.id)
        .fetch_all(&mut **tx)
        .await
        .map_err(media::internal)?;
    rows.iter()
        .map(map_item)
        .collect::<Result<Vec<_>, _>>()
        .map_err(media::internal)
}

/// Item beserta turunannya dalam urutan post-order (anak sebelum induk), seperti `deleteItemRecursive`.
async fn subtree(tx: &mut Transaction<'_, MySql>, root: &Item) -> Result<Vec<Item>, ApiError> {
    let mut out: Vec<Item> = Vec::new();
    let mut stack: Vec<(Item, bool)> = vec![(root.clone(), false)];
    while let Some((item, expanded)) = stack.pop() {
        if expanded {
            out.push(item);
            continue;
        }
        let children = if item.is_folder() {
            live_children(tx, &item).await?
        } else {
            Vec::new()
        };
        stack.push((item, true));
        for child in children.into_iter().rev() {
            stack.push((child, false));
        }
    }
    Ok(out)
}

/// Hapus lunak `root` dan turunannya, dengan media dan audit `deleted`. Mengembalikan jumlah item.
/// Direktori berkas yang harus dihapus setelah commit dikumpulkan di `dirs`.
async fn delete_tree(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: Viewer,
    root: &Item,
    url: &str,
    now: DateTime<Utc>,
    dirs: &mut Vec<PathBuf>,
) -> Result<i64, ApiError> {
    let nodes = subtree(tx, root).await?;
    let mut count: i64 = 0;
    for node in &nodes {
        if node.kind == KIND_FILE {
            let found = media::delete_collection(tx, MODEL, node.id as u64, COLLECTION, None).await?;
            dirs.extend(found);
        }
        sqlx::query("UPDATE user_drive_items SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(now)
            .bind(node.id)
            .execute(&mut **tx)
            .await
            .map_err(media::internal)?;

        let mut old = attributes(node);
        old.insert("deleted_at".into(), carbon_json(Some(now)));
        old.insert("updated_at".into(), carbon_json(Some(now)));
        changes::audit_only(
            tx,
            headers,
            actor.id as u64,
            MODEL,
            "deleted",
            node.id,
            Some(old),
            None,
            url,
        )
        .await?;
        count += 1;
    }
    Ok(count)
}
