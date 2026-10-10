//! `/api/berkas`: port `BerkasController` untuk daftar, jenis dokumen, store, show, update, destroy, dan bulk.
//!
//! Belum dipindah (tetap di Laravel): `quick-share`. `export-pdf` ada di `onlyoffice.rs`, lewat
//! ONLYOFFICE Document Server. `upload-from-url` ada di `berkas_upload_url.rs`.
//!
//! Pengawas dan konsultan pengawas melihat berkas miliknya plus berkas berjudul yang diaktifkan
//! di pengaturan (`AppSetting::applyPengawasSharedBerkasJudulFilter`, lihat `shared_clause`).

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    access, changes,
    format::iso8601_utc,
    foto,
    lookup::carbon_json,
    media::{self, internal},
    pagination, penerima, require_auth, AppState,
};

const COLLECTION: &str = "berkas/dokumen";
const MODEL: &str = "App\\Models\\Berkas";
const ROLE_PRIVILEGED: &[&str] = &["admin", "manager", "super-admin", "operator"];
const ROLE_FIELD: &[&str] = &["pengawas", "konsultan_pengawas"];

const SELECT_BERKAS: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
     jenis_dokumen, CAST(uploaded_by AS SIGNED) AS uploaded_by, created_at, updated_at FROM tbl_berkas";

/// Kolom yang bisa diubah lewat update, urut tetap untuk audit.
const COLUMNS: &[&str] = &["pekerjaan_id", "jenis_dokumen", "uploaded_by"];

/// Baris `tbl_berkas`.
#[derive(Debug, Clone, PartialEq)]
pub struct BerkasRow {
    pub id: i64,
    pub pekerjaan_id: i64,
    pub jenis_dokumen: String,
    pub uploaded_by: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<BerkasRow, sqlx::Error> {
    Ok(BerkasRow {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        jenis_dokumen: r.try_get("jenis_dokumen")?,
        uploaded_by: r.try_get("uploaded_by")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

pub(crate) async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<BerkasRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_BERKAS} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn col_json(row: &BerkasRow, col: &str) -> Value {
    match col {
        "pekerjaan_id" => json!(row.pekerjaan_id),
        "jenis_dokumen" => json!(row.jenis_dokumen),
        "uploaded_by" => json!(row.uploaded_by),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
pub(crate) fn attributes(row: &BerkasRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `BerkasResource`. `uploader` hanya ada bila relasi dimuat (daftar dan store, bukan show dan update).
pub(crate) async fn resource(
    pool: &MySqlPool,
    app_url: &str,
    row: &BerkasRow,
    with_uploader: bool,
) -> Result<Value, ApiError> {
    let base = app_url.trim_end_matches('/');
    let media = media::first_media(pool, MODEL, row.id as u64, COLLECTION)
        .await
        .map_err(internal)?;
    // `getFirstMediaUrl` mengembalikan string kosong bila tidak ada media.
    let berkas_url = media
        .as_ref()
        .map(|m| format!("{base}/storage/{}/{}", m.id, m.file_name))
        .unwrap_or_default();

    let pekerjaan =
        sqlx::query("SELECT CAST(id AS SIGNED) AS id, nama_paket FROM tbl_pekerjaan WHERE id = ?")
            .bind(row.pekerjaan_id)
            .fetch_optional(pool)
            .await
            .map_err(internal)?
            .map(|r| -> Result<Value, sqlx::Error> {
                Ok(json!({
                    "id": r.try_get::<i64, _>("id")?,
                    "nama_paket": r.try_get::<Option<String>, _>("nama_paket")?,
                }))
            })
            .transpose()
            .map_err(internal)?
            .unwrap_or(Value::Null);

    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("jenis_dokumen".into(), json!(row.jenis_dokumen));
    out.insert("pekerjaan_id".into(), json!(row.pekerjaan_id));
    out.insert("uploaded_by".into(), json!(row.uploaded_by));
    out.insert("berkas_url".into(), json!(berkas_url));
    out.insert(
        "file_name".into(),
        json!(media.as_ref().map(|m| m.file_name.clone())),
    );
    out.insert(
        "original_name".into(),
        json!(media.as_ref().map(|m| m.name.clone())),
    );
    out.insert(
        "mime_type".into(),
        json!(media.as_ref().map(|m| m.mime_type.clone())),
    );
    out.insert("size".into(), json!(media.as_ref().map(|m| m.size)));
    out.insert("media_id".into(), json!(media.as_ref().map(|m| m.id)));
    out.insert("pekerjaan".into(), pekerjaan);
    if with_uploader {
        let uploader = match row.uploaded_by {
            Some(u) => {
                sqlx::query("SELECT CAST(id AS SIGNED) AS id, name, email FROM users WHERE id = ?")
                    .bind(u)
                    .fetch_optional(pool)
                    .await
                    .map_err(internal)?
                    .map(|r| -> Result<Value, sqlx::Error> {
                        Ok(json!({
                            "id": r.try_get::<i64, _>("id")?,
                            "name": r.try_get::<Option<String>, _>("name")?,
                            "email": r.try_get::<Option<String>, _>("email")?,
                        }))
                    })
                    .transpose()
                    .map_err(internal)?
                    .unwrap_or(Value::Null)
            }
            None => Value::Null,
        };
        out.insert("uploader".into(), uploader);
    }
    out.insert("created_at".into(), iso8601_utc(row.created_at));
    out.insert("updated_at".into(), iso8601_utc(row.updated_at));
    Ok(Value::Object(out))
}

/// `(int)` PHP untuk string: awalan angka, selain itu 0.
fn php_intval(s: &str) -> i64 {
    let t = s.trim_start();
    let end = t
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))))
        .map_or(t.len(), |(i, _)| i);
    t[..end].parse().unwrap_or(0)
}

/// Judul berkas yang bisa dibuka ke role lapangan, dengan kunci setting (urut seperti Laravel).
const JUDUL_KEYS: &[(&str, &str)] = &[
    ("RAB", "pengawas_berkas_show_rab"),
    ("GAMBAR", "pengawas_berkas_show_gambar"),
    ("NEGO", "pengawas_berkas_show_nego"),
];

/// `AppSetting::PENGAWAS_BERKAS_JUDUL_ALIASES`.
const JUDUL_ALIASES: &[(&str, &[&str])] = &[
    ("RAB", &["rab", "r.a.b", "r a b"]),
    ("GAMBAR", &["gambar", "gbr", "g.b.r", "g b r", "drawing"]),
    (
        "NEGO",
        &[
            "nego",
            "negosiasi",
            "negos",
            "hasil nego",
            "hasil negosiasi",
        ],
    ),
];

/// `AppSetting::pengawasVisibleBerkasJuduls()`: judul yang settingnya bernilai `1`.
/// Setting yang tidak ada (default `0`) atau bernilai NULL tidak terlihat.
async fn visible_titles(pool: &MySqlPool) -> Result<Vec<&'static str>, ApiError> {
    let mut out = Vec::new();
    for (judul, key) in JUDUL_KEYS {
        let value: Option<Option<String>> = sqlx::query_scalar(
            "SELECT CAST(`value` AS CHAR) FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
        if matches!(value, Some(Some(ref v)) if v == "1") {
            out.push(*judul);
        }
    }
    Ok(out)
}

/// `AppSetting::pengawasBerkasJudulAliases`.
fn judul_aliases(judul: &str) -> Vec<String> {
    let key = judul.trim().to_uppercase();
    match JUDUL_ALIASES.iter().find(|(k, _)| *k == key) {
        Some((_, list)) => list.iter().map(|a| a.to_string()).collect(),
        None => vec![judul.trim().to_lowercase()],
    }
}

/// `AppSetting::compactBerkasJudul`: huruf kecil dan hanya alfanumerik ASCII.
fn compact_judul(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Port `applyPengawasSharedBerkasJudulFilter`: jenis dokumen cocok dengan salah satu alias
/// (tepat, awalan dengan spasi, `-`, `_`, `.`), atau dengan bentuk kompak tanpa pemisah.
/// Tanpa judul aktif, klausa `1 = 0` (tidak ada berkas bersama).
fn shared_clause(titles: &[&str]) -> (String, Vec<String>) {
    const TRIMMED: &str = "LOWER(TRIM(jenis_dokumen))";
    const COMPACT: &str = "LOWER(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(TRIM(jenis_dokumen), ' ', ''), '.', ''), '-', ''), '_', ''), '/', ''))";
    let mut parts: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    for title in titles {
        for alias in judul_aliases(title) {
            let alias = alias.trim().to_lowercase();
            if alias.is_empty() {
                continue;
            }
            let compact = compact_judul(&alias);
            let mut part = format!(
                "({TRIMMED} = ? OR {TRIMMED} LIKE ? OR {TRIMMED} LIKE ? OR {TRIMMED} LIKE ? OR {TRIMMED} LIKE ?"
            );
            binds.extend([
                alias.clone(),
                format!("{alias} %"),
                format!("{alias}-%"),
                format!("{alias}_%"),
                format!("{alias}.%"),
            ]);
            if !compact.is_empty() {
                part.push_str(&format!(" OR {COMPACT} = ? OR {COMPACT} LIKE ?"));
                binds.push(compact.clone());
                binds.push(format!("{compact}%"));
            }
            part.push(')');
            parts.push(part);
        }
    }
    if parts.is_empty() {
        ("1 = 0".into(), Vec::new())
    } else {
        (format!("({})", parts.join(" OR ")), binds)
    }
}

/// Otorisasi tulis: `Pekerjaan::userCanAccess` (sama dengan T31 untuk pekerjaan).
async fn ensure_scope(state: &AppState, actor: u64, pekerjaan_id: i64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    foto::ensure_access(state, actor, &roles, Some(pekerjaan_id)).await
}

async fn ensure_pekerjaan_exists(pool: &MySqlPool, id: i64) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n > 0 {
        return Ok(());
    }
    let mut errs = BTreeMap::new();
    foto::add(
        &mut errs,
        "pekerjaan_id",
        "The selected pekerjaan id is invalid.".into(),
    );
    Err(ApiError::validation("The given data was invalid.", errs))
}

// ---------------------------------------------------------------------------
// Daftar dan jenis dokumen
// ---------------------------------------------------------------------------

/// Berkas satu pekerjaan (urut id), untuk relasi `berkas` pada detail pekerjaan.
pub(crate) async fn rows_for_pekerjaan(
    pool: &MySqlPool,
    pekerjaan_id: i64,
) -> Result<Vec<BerkasRow>, ApiError> {
    let sql = format!("{SELECT_BERKAS} WHERE pekerjaan_id = ? ORDER BY id");
    sqlx::query(&sql)
        .bind(pekerjaan_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

/// `BerkasResource` dengan `uploader`, tanpa `pekerjaan` (sama dengan relasi yang dimuat pada detail).
pub(crate) async fn nested_resource(
    pool: &MySqlPool,
    app_url: &str,
    row: &BerkasRow,
) -> Result<Value, ApiError> {
    let mut v = resource(pool, app_url, row, true).await?;
    if let Some(o) = v.as_object_mut() {
        o.remove("pekerjaan");
    }
    Ok(v)
}

/// Berkas terlihat untuk pengawas: milik sendiri, atau berjudul bersama yang aktif (sama dengan daftar).
async fn visible_to_field(pool: &MySqlPool, actor: u64, row: &BerkasRow) -> Result<bool, ApiError> {
    if row.uploaded_by == Some(actor as i64) {
        return Ok(true);
    }
    let titles = visible_titles(pool).await?;
    if titles.is_empty() {
        return Ok(false);
    }
    let (shared, shared_binds) = shared_clause(&titles);
    let sql = format!("SELECT COUNT(*) FROM tbl_berkas WHERE id = ? AND ({shared})");
    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(row.id);
    for b in &shared_binds {
        q = q.bind(b);
    }
    Ok(q.fetch_one(pool).await.map_err(internal)? > 0)
}

/// `GET /api/berkas`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let names: Vec<&str> = roles.iter().map(|(_, n)| n.as_str()).collect();
    let privileged = names.iter().any(|n| ROLE_PRIVILEGED.contains(n));
    let field = names.iter().any(|n| ROLE_FIELD.contains(n));

    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    // T36: role selain pengawas dibatasi ke pekerjaan yang bisa diakses (`byUserRole()`).
    // Pengawas tetap memakai aturan milik sendiri + judul bersama (lintas pekerjaan, sesuai Laravel).
    if !field {
        let scope = access::restriction(user.user_id, &roles, "sp");
        clauses.push(format!(
            "pekerjaan_id IN (SELECT sp.id FROM tbl_pekerjaan sp WHERE 1=1{})",
            scope.sql
        ));
        binds.extend(scope.binds.iter().map(u64::to_string));
    }
    if query
        .get("tahun")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        clauses.push("pekerjaan_id IN (SELECT pk.id FROM tbl_pekerjaan pk JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id WHERE k.tahun_anggaran = ?)".into());
        binds.push(query["tahun"].clone());
    }
    if let Some(pid) = query.get("pekerjaan_id") {
        clauses.push("pekerjaan_id = ?".into());
        binds.push(pid.clone());
    }
    if let Some(search) = query.get("search").filter(|v| !v.is_empty()) {
        clauses.push("jenis_dokumen LIKE ?".into());
        binds.push(format!("%{search}%"));
    }
    let wants_own = query
        .get("mine")
        .is_some_and(|v| penerima::request_boolean(v))
        || query.get("uploaded_by").is_some_and(|v| v == "me");
    // Role lapangan murni selalu dibatasi ke milik sendiri + berkas bersama.
    let force_field = field && !privileged;
    if wants_own || force_field {
        // Laravel: where(uploaded_by = me) OR (judul bersama); bersama hanya untuk role lapangan.
        let shared = if field {
            visible_titles(&state.pool).await?
        } else {
            Vec::new()
        };
        if shared.is_empty() {
            clauses.push("uploaded_by = ?".into());
            binds.push(user.user_id.to_string());
        } else {
            let (sql, sbinds) = shared_clause(&shared);
            clauses.push(format!("(uploaded_by = ? OR {sql})"));
            binds.push(user.user_id.to_string());
            binds.extend(sbinds);
        }
    } else if let Some(u) = query.get("uploaded_by").filter(|v| !v.is_empty()) {
        clauses.push("uploaded_by = ?".into());
        binds.push(php_intval(u).to_string());
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let per_page =
        php_intval(query.get("per_page").map(String::as_str).unwrap_or("20")).clamp(1, 100) as u64;
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);

    let total: i64 = {
        let count_sql = format!("SELECT COUNT(*) FROM tbl_berkas{where_sql}");
        let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
        for b in &binds {
            q = q.bind(b);
        }
        q.fetch_one(&state.pool).await.map_err(internal)?
    };
    let sql = format!("{SELECT_BERKAS}{where_sql} ORDER BY id DESC LIMIT ? OFFSET ?");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(resource(&state.pool, &state.app_url, row, true).await?);
    }
    // Paginator Laravel tanpa `withQueryString()`: tautan hanya membawa `page`.
    let base = format!("{}/api/berkas", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate_with_query(
        data,
        total as u64,
        pagination::PageParams { page, per_page },
        &base,
        "",
    ))
    .into_response())
}

/// `GET /api/berkas/jenis-dokumen`: nilai `jenis_dokumen` yang berbeda, dipangkas, diurutkan.
pub async fn jenis_dokumen(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let values: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT CAST(jenis_dokumen AS CHAR) FROM tbl_berkas \
         WHERE jenis_dokumen IS NOT NULL AND jenis_dokumen <> '' ORDER BY jenis_dokumen",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?
    .into_iter()
    .map(|v: String| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .collect();
    Ok(Json(json!({ "data": values })).into_response())
}

// ---------------------------------------------------------------------------
// Tampil, simpan, ubah, hapus
// ---------------------------------------------------------------------------

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

/// `GET /api/berkas/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let row = find_row(&state.pool, parse_id(&id)?)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let names: Vec<&str> = roles.iter().map(|(_, n)| n.as_str()).collect();
    let privileged = names.iter().any(|n| ROLE_PRIVILEGED.contains(n));
    let field = names.iter().any(|n| ROLE_FIELD.contains(n));
    if field && !privileged {
        if !visible_to_field(&state.pool, user.user_id, &row).await? {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "Anda tidak memiliki akses untuk berkas ini",
            ));
        }
    } else {
        ensure_scope(&state, user.user_id, row.pekerjaan_id).await?;
    }
    let data = resource(&state.pool, &state.app_url, &row, false).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `POST /api/berkas` (multipart): `pekerjaan_id`, `jenis_dokumen`, `file`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut raw = foto::read_form(multipart).await?;
    let mut errs = BTreeMap::new();

    let pekerjaan_id = match raw.presence("pekerjaan_id") {
        foto::Presence::Absent | foto::Presence::Null => {
            foto::add(
                &mut errs,
                "pekerjaan_id",
                "The pekerjaan id field is required.".into(),
            );
            None
        }
        foto::Presence::Value(v) => match v.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => {
                foto::add(
                    &mut errs,
                    "pekerjaan_id",
                    "The selected pekerjaan id is invalid.".into(),
                );
                None
            }
        },
    };
    let jenis = match raw.presence("jenis_dokumen") {
        foto::Presence::Absent | foto::Presence::Null => {
            foto::add(
                &mut errs,
                "jenis_dokumen",
                "The jenis dokumen field is required.".into(),
            );
            None
        }
        foto::Presence::Value(v) if v.chars().count() > 255 => {
            foto::add(
                &mut errs,
                "jenis_dokumen",
                "The jenis dokumen field must not be greater than 255 characters.".into(),
            );
            None
        }
        foto::Presence::Value(v) => Some(v.to_string()),
    };
    let upload = raw.file.take();
    match &upload {
        None => foto::add(&mut errs, "file", "The file field is required.".into()),
        Some(u) if u.bytes.len() > media::MAX_FILE_BYTES => foto::add(
            &mut errs,
            "file",
            "The file field must not be greater than 51200 kilobytes.".into(),
        ),
        Some(_) => {}
    }
    let (Some(pekerjaan_id), Some(jenis), Some(upload)) = (pekerjaan_id, jenis, upload) else {
        return Err(ApiError::validation("The given data was invalid.", errs));
    };
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    ensure_pekerjaan_exists(&state.pool, pekerjaan_id).await?;
    ensure_scope(&state, user.user_id, pekerjaan_id).await?;

    let url = format!("{}/api/berkas", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(&jenis)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let row = find_row(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("berkas baru tidak terbaca"))?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::BERKAS,
        "created",
        id,
        None,
        Some(attributes(&row)),
        Some(pekerjaan_id),
        &url,
    )
    .await?;
    let mime = media::mime_for_name(&upload.original_name);
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &upload, mime, false).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }

    let data = resource(&state.pool, &state.app_url, &row, true).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `PUT` dan `PATCH /api/berkas/{id}`. Field yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    update_impl(state, headers, id, multipart, false).await
}

/// `POST /api/berkas/{id}` dengan `_method=PUT` (method spoofing Laravel).
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
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_scope(&state, user.user_id, current.pekerjaan_id).await?;

    let mut raw = foto::read_form(multipart).await?;
    if require_method_override && raw.presence("_method") != foto::Presence::Value("PUT") {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "The POST method is not supported for this route. Supported methods: GET, HEAD, PUT, PATCH, DELETE.",
        ));
    }
    let mut errs = BTreeMap::new();
    let pekerjaan_new: Option<i64> = match raw.presence("pekerjaan_id") {
        foto::Presence::Absent => None,
        foto::Presence::Null => {
            foto::add(
                &mut errs,
                "pekerjaan_id",
                "The pekerjaan id must not be null.".into(),
            );
            None
        }
        foto::Presence::Value(v) => match v.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => {
                foto::add(
                    &mut errs,
                    "pekerjaan_id",
                    "The selected pekerjaan id is invalid.".into(),
                );
                None
            }
        },
    };
    let jenis_new: Option<String> = match raw.presence("jenis_dokumen") {
        foto::Presence::Absent => None,
        foto::Presence::Null => {
            foto::add(
                &mut errs,
                "jenis_dokumen",
                "The jenis dokumen must not be null.".into(),
            );
            None
        }
        foto::Presence::Value(v) if v.chars().count() > 255 => {
            foto::add(
                &mut errs,
                "jenis_dokumen",
                "The jenis dokumen field must not be greater than 255 characters.".into(),
            );
            None
        }
        foto::Presence::Value(v) => Some(v.to_string()),
    };
    let upload = raw.file.take();
    if let Some(u) = &upload {
        if u.bytes.len() > media::MAX_FILE_BYTES {
            foto::add(
                &mut errs,
                "file",
                "The file field must not be greater than 51200 kilobytes.".into(),
            );
        }
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    if let Some(p) = pekerjaan_new {
        ensure_pekerjaan_exists(&state.pool, p).await?;
        ensure_scope(&state, user.user_id, p).await?;
    }

    let mut next = current.clone();
    if let Some(p) = pekerjaan_new {
        next.pekerjaan_id = p;
    }
    if let Some(j) = jenis_new {
        next.jenis_dokumen = j;
    }
    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let url = format!("{}/api/berkas/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_berkas SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "pekerjaan_id" => {
                    qb.push_bind(next.pekerjaan_id);
                }
                _ => {
                    qb.push_bind(next.jenis_dokumen.clone());
                }
            }
        }
        qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
        qb.build().execute(&mut *tx).await.map_err(internal)?;
        let after = find_row(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("berkas hilang saat update"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        changes::log(
            &mut tx,
            &headers,
            user.user_id,
            &changes::BERKAS,
            "updated",
            id,
            Some(old),
            Some(new),
            Some(next.pekerjaan_id),
            &url,
        )
        .await?;
    }

    // Berkas lama dihapus dulu, lalu berkas baru disimpan (clearMediaCollection, lalu addMedia).
    let mut obsolete: Vec<PathBuf> = Vec::new();
    let mut created_dir = None;
    if let Some(up) = &upload {
        obsolete = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
        let mime = media::mime_for_name(&up.original_name);
        created_dir = Some(
            media::attach(&mut tx, MODEL, id as u64, COLLECTION, up, mime, false)
                .await?
                .dir,
        );
    }
    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(internal(e));
    }
    media::remove_dirs(&obsolete).await;

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let data = resource(&state.pool, &state.app_url, &row, false).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Hapus media dan baris satu berkas beserta audit `deleted`. Mengembalikan direktori berkas.
async fn delete_in_tx(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    row: &BerkasRow,
    url: &str,
) -> Result<Vec<PathBuf>, ApiError> {
    let dirs = media::delete_collection(tx, MODEL, row.id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_berkas WHERE id = ?")
        .bind(row.id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    changes::log(
        tx,
        headers,
        actor,
        &changes::BERKAS,
        "deleted",
        row.id,
        Some(attributes(row)),
        None,
        Some(row.pekerjaan_id),
        url,
    )
    .await?;
    Ok(dirs)
}

/// `DELETE /api/berkas/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_scope(&state, user.user_id, row.pekerjaan_id).await?;
    let url = format!("{}/api/berkas/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs = delete_in_tx(&mut tx, &headers, user.user_id, &row, &url).await?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Berkas deleted successfully" })).into_response())
}

/// `DELETE /api/berkas/bulk` dengan body `{"ids": [...]}`.
pub async fn bulk_destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let ids = foto::parse_ids(&body)?;
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!("{SELECT_BERKAS} WHERE id IN ({placeholders}) ORDER BY id");
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    if rows.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Berkas tidak ditemukan",
        ));
    }
    for row in &rows {
        ensure_scope(&state, user.user_id, row.pekerjaan_id).await?;
    }
    let url = format!("{}/api/berkas/bulk", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut dirs = Vec::new();
    for row in &rows {
        dirs.extend(delete_in_tx(&mut tx, &headers, user.user_id, row, &url).await?);
    }
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    let deleted = rows.len();
    Ok(Json(json!({
        "message": format!("{deleted} berkas dihapus"),
        "deleted": deleted,
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_clause_binds_match_placeholders() {
        let (sql, binds) = shared_clause(&["RAB", "GAMBAR"]);
        assert_eq!(sql.matches('?').count(), binds.len());
        assert!(binds.contains(&"r.a.b".to_string()));
        assert!(binds.contains(&"gbr".to_string()));
        assert!(
            binds.contains(&"gbr%".to_string()),
            "bentuk kompak untuk alias gbr"
        );
        let (empty, none) = shared_clause(&[]);
        assert_eq!(empty, "1 = 0");
        assert!(none.is_empty());
    }

    #[test]
    fn compact_and_aliases_follow_laravel() {
        assert_eq!(compact_judul("G.B.R"), "gbr");
        assert_eq!(compact_judul(" g b r "), "gbr");
        assert_eq!(judul_aliases("rab"), vec!["rab", "r.a.b", "r a b"]);
        assert_eq!(judul_aliases("Lain"), vec!["lain"]);
    }

    #[test]
    fn php_intval_reads_leading_digits() {
        assert_eq!(php_intval("25"), 25);
        assert_eq!(php_intval("abc"), 0);
        assert_eq!(php_intval("12abc"), 12);
        assert_eq!(php_intval(" -3"), -3);
        assert_eq!(php_intval(""), 0);
    }
}
