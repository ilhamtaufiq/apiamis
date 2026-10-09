//! `/api/kanban/*`: port `KanbanController` dan `KanbanTiketSyncService`.
//!
//! Satu papan, `tbl_kanban_boards.slug = 'organisasi'`. `GET board` boleh untuk setiap user yang
//! login. Semua tulisan kartu hanya untuk admin (`ensureAdmin`, 403). Kartu tidak memakai
//! `Auditable` maupun `NotifiesAdminsOnChanges`, jadi tidak ada audit, notifikasi, atau invalidasi
//! cache untuk kartu. Sinkronisasi ke `tbl_tiket` tetap mengaudit baris Tiket (`Auditable`).
//!
//! Deviasi yang dicatat:
//! - Input string di-trim, dan string kosong menjadi null (`TrimStrings`, `ConvertEmptyStringsToNull`).
//! - Tiap field hanya menghasilkan satu pesan validasi. Laravel bisa menghasilkan lebih dari satu.
//! - `updateCard` dan `destroyCard` di Laravel tidak memakai transaksi. Di sini dibungkus transaksi.
//! - Binding 404 kartu dicek sebelum 403 admin, sama seperti Laravel. Middleware izin Rust
//!   (`route_permission`) tetap berjalan lebih dulu.
//! - Relasi yang tidak dimuat di Laravel (`whenLoaded`) dihapus dari JSON. Kolom hitungan pekerjaan
//!   memakai nilai tanpa relasi, sama dengan `tiket::pekerjaan_resource`.
//! - `PUT /api/kanban/cards/from-tiket` menghasilkan 405 di Rust. Laravel menganggapnya `updateCard`
//!   dengan id `from-tiket`, lalu 404.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    changes, foto, lookup::carbon_json, media, pekerjaan, pekerjaan_rel, penerima, require_auth,
    tiket, users, AppState,
};

const BOARD_SLUG: &str = "organisasi";
const FORBIDDEN_ADMIN: &str = "Hanya admin yang boleh melakukan aksi ini";
const TIKET_MODEL: &str = "App\\Models\\Tiket";
const TIKET_ATTACHMENT: &str = "attachment";
const COLUMN_EXISTS: &str = "SELECT COUNT(*) FROM tbl_kanban_columns WHERE id = ?";
const PEKERJAAN_EXISTS: &str = "SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?";
const TIKET_EXISTS: &str = "SELECT COUNT(*) FROM tbl_tiket WHERE id = ?";

pub(crate) type Errs = BTreeMap<String, Vec<String>>;

fn internal(e: impl std::fmt::Display) -> ApiError {
    media::internal(e)
}

/// Respon 422 dengan pesan `Validation error`, seperti `Validator::fails()` di controller ini.
fn validation(errs: Errs) -> ApiError {
    ApiError::validation("Validation error", errs)
}

fn attr(key: &str) -> String {
    key.replace('_', " ")
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

/// Setara `ConvertEmptyStringsToNull` dan `TrimStrings`: rekursif, string kosong menjadi null.
pub(crate) fn clean(v: &Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(clean).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), clean(v))).collect())
        }
        other => other.clone(),
    }
}

/// Objek body setelah dibersihkan. Body yang bukan objek dianggap kosong.
pub(crate) fn body_object(body: &Value) -> Map<String, Value> {
    match clean(body) {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// `array` pada Laravel menerima JSON array maupun objek. Objek kosong menjadi `[]` seperti PHP.
fn normalize_empty_object(v: Value) -> Value {
    match v {
        Value::Object(m) if m.is_empty() => json!([]),
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

/// Aturan `string` dan `max:N` untuk nilai yang tidak null.
pub(crate) fn text_rule(errs: &mut Errs, key: &str, v: &Value, max: usize) -> Option<String> {
    match v {
        Value::String(s) if s.chars().count() <= max => Some(s.clone()),
        Value::String(_) => {
            foto::add(
                errs,
                key,
                format!("The {} field must not be greater than {max} characters.", attr(key)),
            );
            None
        }
        _ => {
            foto::add(errs, key, format!("The {} field must be a string.", attr(key)));
            None
        }
    }
}

/// `required|string|max:N`.
pub(crate) fn required_text(
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Option<String> {
    match obj.get(key) {
        None | Some(Value::Null) => {
            foto::add(errs, key, format!("The {} field is required.", attr(key)));
            None
        }
        Some(v) => text_rule(errs, key, v, max),
    }
}

/// `sometimes|string|max:N` untuk update: absen berarti nilai sekarang dipakai.
fn sometimes_text(
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    max: usize,
    current: String,
) -> String {
    match obj.get(key) {
        None => current,
        Some(v) => text_rule(errs, key, v, max).unwrap_or_default(),
    }
}

/// `nullable|string|max:N`: absen tidak mengubah, null mengosongkan.
fn nullable_text(
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    max: usize,
    current: Option<String>,
) -> Option<String> {
    match obj.get(key) {
        None => current,
        Some(Value::Null) => None,
        Some(v) => text_rule(errs, key, v, max),
    }
}

/// `nullable|array`: JSON array atau objek.
fn nullable_array(
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    current: Option<Value>,
) -> Option<Value> {
    match obj.get(key) {
        None => current,
        Some(Value::Null) => None,
        Some(v @ (Value::Array(_) | Value::Object(_))) => Some(normalize_empty_object(v.clone())),
        Some(_) => {
            foto::add(errs, key, format!("The {} field must be an array.", attr(key)));
            None
        }
    }
}

async fn exists(pool: &MySqlPool, sql: &str, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// Aturan `exists:tabel,id` untuk nilai yang sudah tidak null. Gagal menghasilkan "The selected ... is invalid."
async fn check_id(
    pool: &MySqlPool,
    errs: &mut Errs,
    key: &str,
    v: &Value,
    sql: &str,
) -> Result<Option<i64>, ApiError> {
    let found = match penerima::as_int(v) {
        Some(id) => {
            if exists(pool, sql, id).await? {
                Some(id)
            } else {
                None
            }
        }
        None => None,
    };
    if found.is_none() {
        foto::add(
            errs,
            key,
            format!("The selected {} is invalid.", attr(key)),
        );
    }
    Ok(found)
}

/// `nullable|exists:...`: absen tidak mengubah, null mengosongkan.
async fn nullable_id(
    pool: &MySqlPool,
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    sql: &str,
    current: Option<i64>,
) -> Result<Option<i64>, ApiError> {
    match obj.get(key) {
        None => Ok(current),
        Some(Value::Null) => Ok(None),
        Some(v) => check_id(pool, errs, key, v, sql).await,
    }
}

/// `required|exists:tbl_kanban_columns,id`.
async fn required_column(
    pool: &MySqlPool,
    errs: &mut Errs,
    obj: &Map<String, Value>,
) -> Result<Option<i64>, ApiError> {
    match obj.get("column_id") {
        None | Some(Value::Null) => {
            foto::add(errs, "column_id", "The column id field is required.".into());
            Ok(None)
        }
        Some(v) => check_id(pool, errs, "column_id", v, COLUMN_EXISTS).await,
    }
}

/// `required|integer|min:0` untuk `position`.
fn position_field(errs: &mut Errs, obj: &Map<String, Value>) -> Option<i64> {
    match obj.get("position") {
        None | Some(Value::Null) => {
            foto::add(errs, "position", "The position field is required.".into());
            None
        }
        Some(v) => match penerima::as_int(v) {
            Some(p) if p >= 0 => Some(p),
            Some(_) => {
                foto::add(errs, "position", "The position field must be at least 0.".into());
                None
            }
            None => {
                foto::add(errs, "position", "The position field must be an integer.".into());
                None
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Baris dan pembaca
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Board {
    id: i64,
    slug: String,
    title: String,
    description: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_board(r: &MySqlRow) -> Result<Board, sqlx::Error> {
    Ok(Board {
        id: r.try_get("id")?,
        slug: r.try_get("slug")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_board(pool: &MySqlPool) -> Result<Board, ApiError> {
    // `organisasi()->firstOrFail()`: papan tidak ada berarti 404.
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, slug, title, description, created_at, updated_at \
         FROM tbl_kanban_boards WHERE slug = ?",
    )
    .bind(BOARD_SLUG)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let row = row.ok_or_else(ApiError::not_found)?;
    map_board(&row).map_err(internal)
}

#[derive(Debug, Clone, PartialEq)]
struct Column {
    id: i64,
    board_id: i64,
    title: String,
    position: i64,
    tiket_status: Option<String>,
    color: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_COLUMN: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(board_id AS SIGNED) AS board_id, \
     title, CAST(position AS SIGNED) AS position, tiket_status, color, created_at, updated_at \
     FROM tbl_kanban_columns";

fn map_column(r: &MySqlRow) -> Result<Column, sqlx::Error> {
    Ok(Column {
        id: r.try_get("id")?,
        board_id: r.try_get("board_id")?,
        title: r.try_get("title")?,
        position: r.try_get("position")?,
        tiket_status: r.try_get("tiket_status")?,
        color: r.try_get("color")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn list_columns(pool: &MySqlPool, board_id: i64) -> Result<Vec<Column>, ApiError> {
    // `columns()` memakai `orderBy('position')`. `id` ditambahkan agar urutan seri.
    let sql = format!("{SELECT_COLUMN} WHERE board_id = ? ORDER BY position, id");
    let rows = sqlx::query(&sql)
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter().map(map_column).collect::<Result<Vec<_>, _>>().map_err(internal)
}

/// Kolom di papan. Id di luar papan (atau tidak ada) menghasilkan 404, seperti `findOrFail`.
async fn column_in_board(pool: &MySqlPool, board_id: i64, id: i64) -> Result<Column, ApiError> {
    let sql = format!("{SELECT_COLUMN} WHERE board_id = ? AND id = ?");
    let row = sqlx::query(&sql)
        .bind(board_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let row = row.ok_or_else(ApiError::not_found)?;
    map_column(&row).map_err(internal)
}

/// Status tiket dari kolom mana pun (`$column?->tiket_status`).
async fn column_status<'e, E>(exec: E, id: i64) -> Result<Option<String>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let status = sqlx::query_scalar::<_, Option<String>>(
        "SELECT tiket_status FROM tbl_kanban_columns WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await?;
    Ok(status.flatten())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Card {
    id: i64,
    board_id: i64,
    column_id: i64,
    position: i64,
    title: String,
    description: Option<String>,
    status_label: Option<String>,
    pekerjaan_id: Option<i64>,
    tiket_id: Option<i64>,
    source: String,
    metadata: Option<Value>,
    created_by: i64,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_CARD: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(board_id AS SIGNED) AS board_id, \
     CAST(column_id AS SIGNED) AS column_id, CAST(position AS SIGNED) AS position, title, description, \
     status_label, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(tiket_id AS SIGNED) AS tiket_id, \
     source, CAST(metadata AS CHAR) AS metadata, CAST(created_by AS SIGNED) AS created_by, \
     created_at, updated_at FROM tbl_kanban_cards";

fn map_card(r: &MySqlRow) -> Result<Card, sqlx::Error> {
    let metadata: Option<String> = r.try_get("metadata")?;
    Ok(Card {
        id: r.try_get("id")?,
        board_id: r.try_get("board_id")?,
        column_id: r.try_get("column_id")?,
        position: r.try_get("position")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        status_label: r.try_get("status_label")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        tiket_id: r.try_get("tiket_id")?,
        source: r.try_get("source")?,
        metadata: metadata
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(normalize_empty_object),
        created_by: r.try_get("created_by")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_card<'e, E>(exec: E, id: i64) -> Result<Option<Card>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_CARD} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_card).transpose()
}

async fn list_cards(pool: &MySqlPool, board_id: i64) -> Result<Vec<Card>, ApiError> {
    let sql = format!("{SELECT_CARD} WHERE board_id = ? ORDER BY position, id");
    let rows = sqlx::query(&sql)
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter().map(map_card).collect::<Result<Vec<_>, _>>().map_err(internal)
}

/// `max(position)` dalam kolom, `(int) null + 1` untuk kolom kosong.
async fn next_position(
    tx: &mut Transaction<'_, MySql>,
    column_id: i64,
) -> Result<i64, ApiError> {
    let max = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT CAST(MAX(position) AS SIGNED) FROM tbl_kanban_cards WHERE column_id = ?",
    )
    .bind(column_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(max.unwrap_or(0) + 1)
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// Pekerjaan yang dimuat untuk satu respon. `full` pada `Pekerjaans::value` menentukan apakah
/// relasi `kecamatan` dan `desa` ikut ada (hanya papan yang memuatnya).
struct Pekerjaans {
    rows: Vec<pekerjaan::PekerjaanRow>,
    rel: pekerjaan::Loaded,
}

impl Pekerjaans {
    async fn load(
        pool: &MySqlPool,
        viewer: &pekerjaan_rel::Viewer,
        ids: &[i64],
    ) -> Result<Self, ApiError> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            if let Some(p) = pekerjaan::find(pool, *id as u64).await.map_err(internal)? {
                rows.push(p);
            }
        }
        let mode = pekerjaan::Mode {
            summary: false,
            unbounded: true,
        };
        let rel = pekerjaan::load(pool, &rows, mode, viewer)
            .await
            .map_err(internal)?;
        Ok(Self { rows, rel })
    }

    /// `PekerjaanResource` untuk `id`, atau null bila `id` kosong.
    fn value(&self, id: Option<i64>, full: bool) -> Value {
        let Some(id) = id else {
            return Value::Null;
        };
        let Some(p) = self.rows.iter().find(|p| p.id as i64 == id) else {
            return Value::Null;
        };
        let mut v = pekerjaan::to_resource(p, &self.rel);
        if let Some(obj) = v.as_object_mut() {
            for key in [
                "kegiatan",
                "pengawas",
                "pendamping",
                "tags",
                "kontrak",
                "output",
                "draft",
            ] {
                obj.remove(key);
            }
            if !full {
                obj.remove("kecamatan");
                obj.remove("desa");
            }
            // Relasi tanpa muatan di Laravel: nilai dasar, sama dengan `tiket::pekerjaan_resource`.
            obj.insert("foto_count".into(), Value::Null);
            obj.insert("foto_required_count".into(), Value::Null);
            obj.insert("foto_status".into(), json!("belum_ada_foto"));
            obj.insert("has_kontrak".into(), json!(false));
            obj.insert("kontrak_count".into(), json!(0));
            obj.insert("penerima_count".into(), Value::Null);
            obj.insert("sipd_links_count".into(), json!(0));
            obj.insert("progress_total".into(), json!(0));
            obj.insert("deviasi".into(), json!(0));
            for key in [
                "progress_estimasi_fisik",
                "progress_estimasi_keuangan",
                "progress_estimasi_keuangan_nilai",
                "deviasi_estimasi_fisik",
                "deviasi_estimasi_keuangan",
            ] {
                obj.insert(key.into(), Value::Null);
            }
        }
        v
    }
}

/// `TiketResource` tanpa `user`, `pekerjaan`, dan `comments` (relasi itu tidak dimuat di kanban).
async fn tiket_value(state: &AppState, id: i64) -> Result<Value, ApiError> {
    let t = tiket::find(&state.pool, id as u64)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (image_url, _) = media::first_urls(
        &state.pool,
        &state.app_url,
        TIKET_MODEL,
        t.id,
        TIKET_ATTACHMENT,
    )
    .await
    .map_err(internal)?;
    Ok(json!({
        "id": t.id,
        "user_id": t.user_id,
        "pekerjaan_id": t.pekerjaan_id,
        "subjek": t.subjek,
        "deskripsi": t.deskripsi,
        "kategori": t.kategori,
        "prioritas": t.prioritas,
        "status": t.status,
        "admin_notes": t.admin_notes,
        "image_url": image_url,
        "created_at": carbon_json(t.created_at),
        "updated_at": carbon_json(t.updated_at),
    }))
}

/// `UserResource` untuk `creator`, tanpa `roles` dan `permissions` (tidak dimuat).
async fn creator_value(
    state: &AppState,
    cache: &mut HashMap<i64, Value>,
    user_id: i64,
) -> Result<Value, ApiError> {
    if let Some(v) = cache.get(&user_id) {
        return Ok(v.clone());
    }
    let mut v = users::resource(&state.pool, &state.app_url, user_id as u64)
        .await
        .map_err(internal)?
        .unwrap_or(Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.remove("roles");
        obj.remove("permissions");
    }
    cache.insert(user_id, v.clone());
    Ok(v)
}

/// Relasi yang dimuat untuk satu `KanbanCardResource`.
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Pekerjaan memuat `kecamatan` dan `desa` (hanya `GET board`).
    pekerjaan_full: bool,
    /// Tiket dimuat (`GET board`, import, update, dan move).
    tiket: bool,
}

async fn card_json(
    state: &AppState,
    card: &Card,
    shape: Shape,
    pek: &Pekerjaans,
    creators: &mut HashMap<i64, Value>,
) -> Result<Value, ApiError> {
    let tiket_json = if shape.tiket {
        match card.tiket_id {
            Some(tid) => tiket_value(state, tid).await?,
            None => Value::Null,
        }
    } else {
        Value::Null
    };
    let creator = creator_value(state, creators, card.created_by).await?;
    let mut v = json!({
        "id": card.id,
        "board_id": card.board_id,
        "column_id": card.column_id,
        "position": card.position,
        "title": card.title,
        "description": card.description,
        "status_label": card.status_label,
        "pekerjaan_id": card.pekerjaan_id,
        "pekerjaan": pek.value(card.pekerjaan_id, shape.pekerjaan_full),
        "tiket_id": card.tiket_id,
        "source": card.source,
        "metadata": card.metadata,
        "created_by": card.created_by,
        "creator": creator,
        "created_at": carbon_json(card.created_at),
        "updated_at": carbon_json(card.updated_at),
    });
    if shape.tiket {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("tiket".into(), tiket_json);
        }
    }
    Ok(v)
}

/// Bungkus `{"data": ...}` seperti `JsonResource` Laravel.
fn data(v: Value) -> Response {
    Json(json!({ "data": v })).into_response()
}

async fn viewer_for(pool: &MySqlPool, user_id: u64) -> Result<pekerjaan_rel::Viewer, ApiError> {
    let roles = auth::login::roles_of(pool, user_id)
        .await
        .map_err(internal)?;
    pekerjaan_rel::viewer(pool, user_id, &roles)
        .await
        .map_err(internal)
}

/// Respon `KanbanCardResource` untuk kartu `card_id`, dibaca ulang setelah tulis.
async fn card_response(
    state: &AppState,
    card_id: i64,
    shape: Shape,
    user_id: u64,
) -> Result<Response, ApiError> {
    let card = find_card(&state.pool, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let viewer = viewer_for(&state.pool, user_id).await?;
    let ids: Vec<i64> = card.pekerjaan_id.into_iter().collect();
    let pek = Pekerjaans::load(&state.pool, &viewer, &ids).await?;
    let mut creators = HashMap::new();
    let v = card_json(state, &card, shape, &pek, &mut creators).await?;
    Ok(data(v))
}

// ---------------------------------------------------------------------------
// Otorisasi
// ---------------------------------------------------------------------------

/// Setara `ensureAdmin()`: 403 bila user tidak punya role `admin`.
async fn ensure_admin(pool: &MySqlPool, user_id: u64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(pool, user_id)
        .await
        .map_err(internal)?;
    if roles.iter().any(|(_, n)| n == "admin") {
        Ok(())
    } else {
        Err(ApiError::new(StatusCode::FORBIDDEN, FORBIDDEN_ADMIN))
    }
}

// ---------------------------------------------------------------------------
// Sinkronisasi tiket (`KanbanTiketSyncService::syncCardToTiket`)
// ---------------------------------------------------------------------------

/// Salin judul, deskripsi, pekerjaan, dan status kolom ke tiket terkait. Tiket hanya ditulis bila
/// ada nilai yang berubah. Perubahan itu diaudit sebagai `updated` pada `App\Models\Tiket`.
/// `url` adalah URL request kanban, karena Laravel mencatat `Request::fullUrl()` dari request itu.
pub(crate) async fn sync_card_to_tiket(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    url: &str,
    card: &Card,
    column_status: Option<&str>,
) -> Result<(), ApiError> {
    let Some(tiket_id) = card.tiket_id else {
        return Ok(());
    };
    let row = sqlx::query(
        "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, subjek, deskripsi, status, updated_at \
         FROM tbl_tiket WHERE id = ?",
    )
    .bind(tiket_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    // Tiket tidak ada: Laravel `find` mengembalikan null dan sinkron dilewati.
    let Some(row) = row else {
        return Ok(());
    };
    let cur_pekerjaan: Option<i64> = row.try_get("pekerjaan_id").map_err(internal)?;
    let cur_subjek: String = row.try_get("subjek").map_err(internal)?;
    let cur_deskripsi: String = row.try_get("deskripsi").map_err(internal)?;
    let cur_status: String = row.try_get("status").map_err(internal)?;
    let cur_updated: Option<DateTime<Utc>> = row.try_get("updated_at").map_err(internal)?;

    let next_pekerjaan = card.pekerjaan_id;
    let next_subjek = card.title.clone();
    let next_deskripsi = card
        .description
        .clone()
        .unwrap_or_else(|| cur_deskripsi.clone());
    let next_status = column_status
        .map(str::to_string)
        .unwrap_or_else(|| cur_status.clone());

    let mut old = Map::new();
    let mut new = Map::new();
    if cur_pekerjaan != next_pekerjaan {
        old.insert("pekerjaan_id".into(), json!(cur_pekerjaan));
        new.insert("pekerjaan_id".into(), json!(next_pekerjaan));
    }
    if cur_subjek != next_subjek {
        old.insert("subjek".into(), json!(cur_subjek));
        new.insert("subjek".into(), json!(next_subjek));
    }
    if cur_deskripsi != next_deskripsi {
        old.insert("deskripsi".into(), json!(cur_deskripsi));
        new.insert("deskripsi".into(), json!(next_deskripsi));
    }
    if cur_status != next_status {
        old.insert("status".into(), json!(cur_status));
        new.insert("status".into(), json!(next_status));
    }
    if old.is_empty() {
        return Ok(());
    }

    // Kolom yang tidak berubah ditulis dengan nilainya sendiri, sehingga hasilnya sama.
    sqlx::query(
        "UPDATE tbl_tiket SET pekerjaan_id = ?, subjek = ?, deskripsi = ?, status = ?, updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(next_pekerjaan)
    .bind(next_subjek)
    .bind(next_deskripsi)
    .bind(next_status)
    .bind(tiket_id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;

    let new_updated: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT updated_at FROM tbl_tiket WHERE id = ?")
            .bind(tiket_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(internal)?;
    old.insert("updated_at".into(), carbon_json(cur_updated));
    new.insert("updated_at".into(), carbon_json(new_updated));

    changes::audit_only(
        tx,
        headers,
        actor,
        TIKET_MODEL,
        "updated",
        tiket_id,
        Some(old),
        Some(new),
        url,
    )
    .await
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/kanban/board`. Semua kolom dengan kartu, kartu diurutkan per `position`.
pub async fn board(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let board = find_board(&state.pool).await?;
    let columns = list_columns(&state.pool, board.id).await?;
    let cards = list_cards(&state.pool, board.id).await?;
    let viewer = viewer_for(&state.pool, user.user_id).await?;
    let ids: Vec<i64> = cards.iter().filter_map(|c| c.pekerjaan_id).collect();
    let pek = Pekerjaans::load(&state.pool, &viewer, &ids).await?;

    let mut creators = HashMap::new();
    let mut columns_json = Vec::with_capacity(columns.len());
    for col in &columns {
        let mut cards_json = Vec::new();
        for card in cards.iter().filter(|c| c.column_id == col.id) {
            cards_json.push(
                card_json(
                    &state,
                    card,
                    Shape {
                        pekerjaan_full: true,
                        tiket: true,
                    },
                    &pek,
                    &mut creators,
                )
                .await?,
            );
        }
        columns_json.push(json!({
            "id": col.id,
            "board_id": col.board_id,
            "title": col.title,
            "position": col.position,
            "tiket_status": col.tiket_status,
            "color": col.color,
            "cards": cards_json,
            "created_at": carbon_json(col.created_at),
            "updated_at": carbon_json(col.updated_at),
        }));
    }

    Ok(data(json!({
        "id": board.id,
        "slug": board.slug,
        "title": board.title,
        "description": board.description,
        "columns": columns_json,
        "created_at": carbon_json(board.created_at),
        "updated_at": carbon_json(board.updated_at),
    })))
}

/// `POST /api/kanban/cards` (admin). Kartu baru ditaruh di akhir kolom, `source` = `manual`.
pub async fn store_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state.pool, user.user_id).await?;
    let board = find_board(&state.pool).await?;

    let obj = body_object(&body);
    let mut errs = Errs::new();
    let column_id = required_column(&state.pool, &mut errs, &obj).await?;
    let title = required_text(&mut errs, &obj, "title", 255);
    let description = nullable_text(&mut errs, &obj, "description", usize::MAX, None);
    let status_label = nullable_text(&mut errs, &obj, "status_label", 100, None);
    let pekerjaan_id =
        nullable_id(&state.pool, &mut errs, &obj, "pekerjaan_id", PEKERJAAN_EXISTS, None).await?;
    let metadata = nullable_array(&mut errs, &obj, "metadata", None);
    let (Some(column_id), Some(title), true) = (column_id, title, errs.is_empty()) else {
        return Err(validation(errs));
    };

    let column = column_in_board(&state.pool, board.id, column_id).await?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let position = next_position(&mut tx, column.id).await?;
    let metadata_text = metadata.as_ref().map(|m| m.to_string());
    let res = sqlx::query(
        "INSERT INTO tbl_kanban_cards (board_id, column_id, position, title, description, status_label, \
         pekerjaan_id, tiket_id, source, metadata, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, 'manual', ?, ?, NOW(), NOW())",
    )
    .bind(board.id)
    .bind(column.id)
    .bind(position)
    .bind(title)
    .bind(description)
    .bind(status_label)
    .bind(pekerjaan_id)
    .bind(metadata_text)
    .bind(user.user_id as i64)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    tx.commit().await.map_err(internal)?;

    card_response(
        &state,
        id,
        Shape {
            pekerjaan_full: false,
            tiket: false,
        },
        user.user_id,
    )
    .await
}

/// `POST /api/kanban/cards/from-tiket` (admin). Satu tiket hanya boleh satu kali per papan (409).
pub async fn import_from_tiket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state.pool, user.user_id).await?;
    let board = find_board(&state.pool).await?;

    let obj = body_object(&body);
    let mut errs = Errs::new();
    let tiket_id = match obj.get("tiket_id") {
        None | Some(Value::Null) => {
            foto::add(&mut errs, "tiket_id", "The tiket id field is required.".into());
            None
        }
        Some(v) => check_id(&state.pool, &mut errs, "tiket_id", v, TIKET_EXISTS).await?,
    };
    let column_req =
        nullable_id(&state.pool, &mut errs, &obj, "column_id", COLUMN_EXISTS, None).await?;
    let pekerjaan_req =
        nullable_id(&state.pool, &mut errs, &obj, "pekerjaan_id", PEKERJAAN_EXISTS, None).await?;
    let (Some(tiket_id), true) = (tiket_id, errs.is_empty()) else {
        return Err(validation(errs));
    };

    let t = tiket::find(&state.pool, tiket_id as u64)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let dup = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(id AS SIGNED) FROM tbl_kanban_cards WHERE board_id = ? AND tiket_id = ? LIMIT 1",
    )
    .bind(board.id)
    .bind(tiket_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;
    if dup.is_some() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "Tiket sudah ada di kanban",
        ));
    }

    // `$request->column_id ?? resolveColumnIdForTiketStatus(...) ?? kolom pertama`.
    let column_id = match column_req {
        Some(cid) => cid,
        None => {
            let by_status = sqlx::query_scalar::<_, i64>(
                "SELECT CAST(id AS SIGNED) FROM tbl_kanban_columns WHERE board_id = ? AND tiket_status = ? \
                 ORDER BY id LIMIT 1",
            )
            .bind(board.id)
            .bind(t.status.clone())
            .fetch_optional(&state.pool)
            .await
            .map_err(internal)?;
            let fallback = match by_status {
                Some(cid) => Some(cid),
                None => sqlx::query_scalar::<_, i64>(
                    "SELECT CAST(id AS SIGNED) FROM tbl_kanban_columns WHERE board_id = ? \
                     ORDER BY position, id LIMIT 1",
                )
                .bind(board.id)
                .fetch_optional(&state.pool)
                .await
                .map_err(internal)?,
            };
            fallback.ok_or_else(ApiError::not_found)?
        }
    };
    let column = column_in_board(&state.pool, board.id, column_id).await?;

    let pekerjaan_id = pekerjaan_req.or(t.pekerjaan_id.map(|p| p as i64));
    let metadata_text = json!({
        "kategori": t.kategori,
        "prioritas": t.prioritas,
    })
    .to_string();

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let position = next_position(&mut tx, column.id).await?;
    let res = sqlx::query(
        "INSERT INTO tbl_kanban_cards (board_id, column_id, position, title, description, status_label, \
         pekerjaan_id, tiket_id, source, metadata, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?, 'tiket', ?, ?, NOW(), NOW())",
    )
    .bind(board.id)
    .bind(column.id)
    .bind(position)
    .bind(t.subjek.clone())
    .bind(Some(t.deskripsi.clone()))
    .bind(pekerjaan_id)
    .bind(tiket_id)
    .bind(metadata_text)
    .bind(user.user_id as i64)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    tx.commit().await.map_err(internal)?;

    card_response(
        &state,
        id,
        Shape {
            pekerjaan_full: false,
            tiket: true,
        },
        user.user_id,
    )
    .await
}

/// `PUT /api/kanban/cards/{id}` (admin). Hanya field yang dikirim yang diubah. Tiket terkait disinkronkan.
pub async fn update_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let card_id = parse_id(&id)?;
    let board = find_board(&state.pool).await?;
    let current = find_card(&state.pool, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_admin(&state.pool, user.user_id).await?;
    if current.board_id != board.id {
        return Err(ApiError::not_found());
    }

    let obj = body_object(&body);
    let mut errs = Errs::new();
    let title = sometimes_text(&mut errs, &obj, "title", 255, current.title.clone());
    let description = nullable_text(
        &mut errs,
        &obj,
        "description",
        usize::MAX,
        current.description.clone(),
    );
    let status_label = nullable_text(
        &mut errs,
        &obj,
        "status_label",
        100,
        current.status_label.clone(),
    );
    let pekerjaan_id = nullable_id(
        &state.pool,
        &mut errs,
        &obj,
        "pekerjaan_id",
        PEKERJAAN_EXISTS,
        current.pekerjaan_id,
    )
    .await?;
    let metadata = nullable_array(&mut errs, &obj, "metadata", current.metadata.clone());
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    let next = Card {
        title,
        description,
        status_label,
        pekerjaan_id,
        metadata,
        ..current.clone()
    };
    let url = format!(
        "{}/api/kanban/cards/{card_id}",
        state.app_url.trim_end_matches('/')
    );

    let mut tx = state.pool.begin().await.map_err(internal)?;
    if next != current {
        sqlx::query(
            "UPDATE tbl_kanban_cards SET title = ?, description = ?, status_label = ?, pekerjaan_id = ?, \
             metadata = ?, updated_at = NOW() WHERE id = ?",
        )
        .bind(next.title.clone())
        .bind(next.description.clone())
        .bind(next.status_label.clone())
        .bind(next.pekerjaan_id)
        .bind(next.metadata.as_ref().map(|m| m.to_string()))
        .bind(card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }
    let card = find_card(&mut *tx, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let status = column_status(&mut *tx, card.column_id)
        .await
        .map_err(internal)?;
    sync_card_to_tiket(
        &mut tx,
        &headers,
        user.user_id,
        &url,
        &card,
        status.as_deref(),
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    card_response(
        &state,
        card_id,
        Shape {
            pekerjaan_full: false,
            tiket: true,
        },
        user.user_id,
    )
    .await
}

/// Geser kartu dalam kolom yang sama (`reorderWithinColumn`).
async fn reorder_within_column(
    tx: &mut Transaction<'_, MySql>,
    card: &Card,
    new_pos: i64,
) -> Result<(), ApiError> {
    let old = card.position;
    if new_pos == old {
        return Ok(());
    }
    if new_pos < old {
        sqlx::query(
            "UPDATE tbl_kanban_cards SET position = position + 1 \
             WHERE column_id = ? AND position BETWEEN ? AND ?",
        )
        .bind(card.column_id)
        .bind(new_pos)
        .bind(old - 1)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    } else {
        sqlx::query(
            "UPDATE tbl_kanban_cards SET position = position - 1 \
             WHERE column_id = ? AND position BETWEEN ? AND ?",
        )
        .bind(card.column_id)
        .bind(old + 1)
        .bind(new_pos)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }
    sqlx::query("UPDATE tbl_kanban_cards SET position = ?, updated_at = NOW() WHERE id = ?")
        .bind(new_pos)
        .bind(card.id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

/// `PATCH /api/kanban/cards/{id}/move` (admin). Pindah kolom atau posisi, lalu sinkron tiket.
pub async fn move_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let card_id = parse_id(&id)?;
    let board = find_board(&state.pool).await?;
    let current = find_card(&state.pool, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_admin(&state.pool, user.user_id).await?;
    if current.board_id != board.id {
        return Err(ApiError::not_found());
    }

    let obj = body_object(&body);
    let mut errs = Errs::new();
    let column_id = required_column(&state.pool, &mut errs, &obj).await?;
    let position = position_field(&mut errs, &obj);
    let (Some(column_id), Some(position), true) = (column_id, position, errs.is_empty()) else {
        return Err(validation(errs));
    };

    let target = column_in_board(&state.pool, board.id, column_id).await?;
    let url = format!(
        "{}/api/kanban/cards/{card_id}/move",
        state.app_url.trim_end_matches('/')
    );

    let mut tx = state.pool.begin().await.map_err(internal)?;
    if current.column_id == target.id {
        reorder_within_column(&mut tx, &current, position).await?;
    } else {
        sqlx::query(
            "UPDATE tbl_kanban_cards SET position = position - 1 WHERE column_id = ? AND position > ?",
        )
        .bind(current.column_id)
        .bind(current.position)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query(
            "UPDATE tbl_kanban_cards SET position = position + 1 WHERE column_id = ? AND position >= ?",
        )
        .bind(target.id)
        .bind(position)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query(
            "UPDATE tbl_kanban_cards SET column_id = ?, position = ?, updated_at = NOW() WHERE id = ?",
        )
        .bind(target.id)
        .bind(position)
        .bind(card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    let card = find_card(&mut *tx, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    sync_card_to_tiket(
        &mut tx,
        &headers,
        user.user_id,
        &url,
        &card,
        target.tiket_status.as_deref(),
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    card_response(
        &state,
        card_id,
        Shape {
            pekerjaan_full: false,
            tiket: true,
        },
        user.user_id,
    )
    .await
}

/// `DELETE /api/kanban/cards/{id}` (admin). Hard delete, lalu posisi kartu di bawahnya digeser.
pub async fn destroy_card(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let card_id = parse_id(&id)?;
    let board = find_board(&state.pool).await?;
    let current = find_card(&state.pool, card_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_admin(&state.pool, user.user_id).await?;
    if current.board_id != board.id {
        return Err(ApiError::not_found());
    }

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_kanban_cards WHERE id = ?")
        .bind(card_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_kanban_cards SET position = position - 1 WHERE column_id = ? AND position > ?",
    )
    .bind(current.column_id)
    .bind(current.position)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "message": "Kartu kanban berhasil dihapus" })).into_response())
}
