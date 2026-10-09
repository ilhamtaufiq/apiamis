//! Survei lokasi (`SurveyLokasiController`): tabel `tbl_survey_lokasi`, model `App\Models\SurveyLokasi`
//! (`Auditable`, koleksi media `foto`). Tugas induk (`tbl_survey_tugas`) ikut diperbarui seperti Laravel.
//!
//! Rute: stats, index, store, show, update, destroy, verifikasi (admin), foto (unggah dan hapus).
//! Input bisa JSON atau multipart (`foto[]`, `foto_kategori[]`, `detail` sebagai string JSON).
//!
//! Perbedaan dengan Laravel:
//! - Validasi hanya menyimpan pesan pertama per field. Laravel bisa mengirim beberapa pesan.
//! - `date` menerima format umum (`YYYY-MM-DD`, datetime, RFC 3339), bukan seluruh `strtotime`.
//! - `mimes` memeriksa ekstensi nama berkas, tidak membaca tipe dari isi berkas.
//! - `detail[kunci]` bentuk bersarang di multipart tidak didukung. Kirim `detail` sebagai string JSON.
//! - Urutan daftar `created_at DESC, id DESC` (Laravel `latest()` tanpa tie-breaker).
//! - Rute dengan `{survey_lokasi}` mencari baris dulu (404), baru memeriksa role atau pemilik.
//! - `verified_by` memakai `required_if` yang juga berlaku bila `catatan_verifikasi` tidak dikirim.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    changes,
    format::iso8601_utc,
    foto, media,
    pagination::{self, PageParams},
    require_auth, AppState,
};

const MODEL: &str = "App\\Models\\SurveyLokasi";
const TUGAS_MODEL: &str = "App\\Models\\SurveyTugas";
const COLLECTION: &str = "foto";
const SURVEY_TABLE: &str = "tbl_survey_lokasi";
const TUGAS_TABLE: &str = "tbl_survey_tugas";
/// `MODEL::SURVEY_ROLES` di `SurveyLokasiController`.
const SURVEY_ROLES: &[&str] = &["tfl", "operator", "pengawas", "konsultan_pengawas"];
const JENIS: &[&str] = &[
    "spam_perpipaan",
    "spam_pengeboran",
    "mck_individu",
    "mck_komunal",
];
/// `mimes:jpg,jpeg,png,webp,gif,pdf,doc,docx,xls,xlsx|max:10240`.
const FOTO_EXT: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "pdf", "doc", "docx", "xls", "xlsx",
];
const FOTO_MAX_KB: usize = 10_240;
const DENY_ROLE: &str = "Forbidden. Hanya admin, TFL, operator, pengawas, atau konsultan pengawas yang dapat mengisi survey.";
const UPDATABLE: &[&str] = &[
    "tugas_id",
    "jenis",
    "nama_lokasi",
    "kecamatan_id",
    "desa_id",
    "alamat",
    "latitude",
    "longitude",
    "detail",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("survey-lokasi: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, e.to_string())
}

fn forbidden(message: &str) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, message)
}

fn deny_role() -> ApiError {
    forbidden(DENY_ROLE)
}

fn tugas_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "Tugas survey tidak ditemukan.")
}

fn jenis_mismatch() -> ApiError {
    let mut map = BTreeMap::new();
    map.insert(
        "jenis".to_string(),
        vec!["Jenis survey harus sesuai tugas.".to_string()],
    );
    ApiError::validation("Validation error", map)
}

// ---------------------------------------------------------------------------
// Aktor dan otorisasi
// ---------------------------------------------------------------------------

struct Actor {
    id: u64,
    is_admin: bool,
    can_survey: bool,
}

/// `auth()->user()` plus `hasRole('admin')` dan `canSurvey()`. Tidak menolak non-admin.
async fn actor(state: &AppState, headers: &HeaderMap) -> Result<Actor, ApiError> {
    let user = require_auth(state, headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let is_admin = roles.iter().any(|(_, n)| n == "admin");
    let can_survey = is_admin
        || roles
            .iter()
            .any(|(_, n)| SURVEY_ROLES.contains(&n.as_str()));
    Ok(Actor {
        id: user.user_id,
        is_admin,
        can_survey,
    })
}

/// Pemeriksaan update, destroy, unggah dan hapus foto: role, pemilik, lalu status `diajukan`.
fn ensure_editable(actor: &Actor, row: &SurveyRow, status_message: &str) -> Result<(), ApiError> {
    if actor.is_admin {
        return Ok(());
    }
    if !actor.can_survey {
        return Err(deny_role());
    }
    if row.user_id != actor.id as i64 {
        return Err(forbidden("Forbidden"));
    }
    if row.status != "diajukan" {
        return Err(forbidden(status_message));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Baris dan respons
// ---------------------------------------------------------------------------

const SELECT_SURVEY: &str = "SELECT CAST(s.id AS SIGNED) AS id, CAST(s.user_id AS SIGNED) AS user_id, \
     CAST(s.tugas_id AS SIGNED) AS tugas_id, CAST(s.jenis AS CHAR) AS jenis, s.nama_lokasi, \
     CAST(s.kecamatan_id AS SIGNED) AS kecamatan_id, CAST(s.desa_id AS SIGNED) AS desa_id, s.alamat, \
     CAST(s.latitude AS CHAR) AS latitude, CAST(s.longitude AS CHAR) AS longitude, CAST(s.detail AS CHAR) AS detail, \
     CAST(s.status AS CHAR) AS status, s.catatan_verifikasi, CAST(s.verified_by AS SIGNED) AS verified_by, \
     s.verified_at, s.created_at, s.updated_at, \
     CAST(kec.id AS SIGNED) AS kec_id, kec.n_kec AS kec_nama, \
     CAST(desa.id AS SIGNED) AS desa_row_id, desa.n_desa AS desa_nama, \
     CAST(u.id AS SIGNED) AS surveyor_id, u.name AS surveyor_name, \
     CAST(v.id AS SIGNED) AS verifier_id, v.name AS verifier_name, \
     CAST(t.id AS SIGNED) AS tugas_row_id, t.judul AS tugas_judul, CAST(t.status AS CHAR) AS tugas_status \
     FROM tbl_survey_lokasi s \
     LEFT JOIN tbl_kecamatan kec ON kec.id = s.kecamatan_id \
     LEFT JOIN tbl_desa desa ON desa.id = s.desa_id \
     LEFT JOIN users u ON u.id = s.user_id \
     LEFT JOIN users v ON v.id = s.verified_by \
     LEFT JOIN tbl_survey_tugas t ON t.id = s.tugas_id";

struct SurveyRow {
    id: i64,
    user_id: i64,
    tugas_id: Option<i64>,
    jenis: String,
    nama_lokasi: String,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
    alamat: Option<String>,
    latitude: Option<String>,
    longitude: Option<String>,
    detail: Option<String>,
    status: String,
    catatan_verifikasi: Option<String>,
    verified_at: Option<DateTime<Utc>>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    kec_id: Option<i64>,
    kec_nama: Option<String>,
    desa_row_id: Option<i64>,
    desa_nama: Option<String>,
    surveyor_id: Option<i64>,
    surveyor_name: Option<String>,
    verifier_id: Option<i64>,
    verifier_name: Option<String>,
    tugas_row_id: Option<i64>,
    tugas_judul: Option<String>,
    tugas_status: Option<String>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<SurveyRow, sqlx::Error> {
    Ok(SurveyRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        tugas_id: r.try_get("tugas_id")?,
        jenis: r.try_get("jenis")?,
        nama_lokasi: r.try_get("nama_lokasi")?,
        kecamatan_id: r.try_get("kecamatan_id")?,
        desa_id: r.try_get("desa_id")?,
        alamat: r.try_get("alamat")?,
        latitude: r.try_get("latitude")?,
        longitude: r.try_get("longitude")?,
        detail: r.try_get("detail")?,
        status: r.try_get("status")?,
        catatan_verifikasi: r.try_get("catatan_verifikasi")?,
        verified_at: r.try_get("verified_at")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        kec_id: r.try_get("kec_id")?,
        kec_nama: r.try_get("kec_nama")?,
        desa_row_id: r.try_get("desa_row_id")?,
        desa_nama: r.try_get("desa_nama")?,
        surveyor_id: r.try_get("surveyor_id")?,
        surveyor_name: r.try_get("surveyor_name")?,
        verifier_id: r.try_get("verifier_id")?,
        verifier_name: r.try_get("verifier_name")?,
        tugas_row_id: r.try_get("tugas_row_id")?,
        tugas_judul: r.try_get("tugas_judul")?,
        tugas_status: r.try_get("tugas_status")?,
    })
}

async fn load<'c, E>(exec: E, id: i64) -> Result<Option<SurveyRow>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let sql = format!("{SELECT_SURVEY} WHERE s.id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(internal)?
        .map(|r| map_row(&r))
        .transpose()
        .map_err(internal)
}

/// Foto koleksi `foto` per survei, urut `order_column` lalu `id` (`getMedia('foto')`).
async fn fotos_of<'c, E>(
    exec: E,
    app_url: &str,
    ids: &[i64],
) -> Result<HashMap<i64, Vec<Value>>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let mut out: HashMap<i64, Vec<Value>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "SELECT CAST(id AS SIGNED) AS id, CAST(model_id AS SIGNED) AS model_id, file_name, \
         CAST(size AS SIGNED) AS size, CAST(custom_properties AS CHAR) AS custom_properties \
         FROM media WHERE model_type = ? AND collection_name = ? AND model_id IN ({placeholders}) \
         ORDER BY order_column, id"
    );
    let mut q = sqlx::query(&sql).bind(MODEL).bind(COLLECTION);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(exec).await.map_err(internal)?;
    let base = app_url.trim_end_matches('/');
    for r in rows {
        let media_id: i64 = r.try_get("id").map_err(internal)?;
        let model_id: i64 = r.try_get("model_id").map_err(internal)?;
        let file_name: String = r.try_get("file_name").map_err(internal)?;
        let size: i64 = r.try_get("size").map_err(internal)?;
        let props: Option<String> = r.try_get("custom_properties").map_err(internal)?;
        let kategori = props
            .as_deref()
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .and_then(|v| {
                v.get("kategori")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        out.entry(model_id).or_default().push(json!({
            "id": media_id,
            "url": format!("{base}/storage/{media_id}/{file_name}"),
            "name": file_name,
            "size": size,
            "kategori": kategori,
        }));
    }
    Ok(out)
}

/// `(float) $this->latitude`: angka, atau null.
fn num_json(s: Option<&str>) -> Value {
    s.and_then(|t| t.parse::<f64>().ok())
        .map_or(Value::Null, Value::from)
}

/// Kolom `detail` (JSON). Objek kosong menjadi `[]` seperti array kosong PHP.
fn detail_value(s: Option<&str>) -> Value {
    match s.and_then(|t| serde_json::from_str::<Value>(t).ok()) {
        Some(Value::Object(m)) if m.is_empty() => json!([]),
        Some(v) => v,
        None => Value::Null,
    }
}

fn jenis_label(jenis: &str) -> &str {
    match jenis {
        "spam_perpipaan" => "SPAM Perpipaan",
        "spam_pengeboran" => "SPAM Pengeboran",
        "mck_individu" => "MCK Individu",
        "mck_komunal" => "MCK Komunal",
        other => other,
    }
}

/// Bentuk `SurveyLokasiResource`. `tugas` hanya ada bila relasinya dimuat (tidak di respons unggah foto).
fn resource(r: &SurveyRow, fotos: Vec<Value>, with_tugas: bool) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(r.id));
    m.insert("jenis".into(), json!(r.jenis));
    m.insert("jenis_label".into(), json!(jenis_label(&r.jenis)));
    m.insert("nama_lokasi".into(), json!(r.nama_lokasi));
    m.insert(
        "kecamatan".into(),
        r.kec_id
            .map_or(Value::Null, |id| json!({ "id": id, "nama": r.kec_nama })),
    );
    m.insert(
        "desa".into(),
        r.desa_row_id
            .map_or(Value::Null, |id| json!({ "id": id, "nama": r.desa_nama })),
    );
    m.insert("kecamatan_id".into(), json!(r.kecamatan_id));
    m.insert("desa_id".into(), json!(r.desa_id));
    m.insert("alamat".into(), json!(r.alamat));
    m.insert("latitude".into(), num_json(r.latitude.as_deref()));
    m.insert("longitude".into(), num_json(r.longitude.as_deref()));
    m.insert("detail".into(), detail_value(r.detail.as_deref()));
    m.insert("status".into(), json!(r.status));
    m.insert("catatan_verifikasi".into(), json!(r.catatan_verifikasi));
    m.insert(
        "verified_by".into(),
        r.verifier_id.map_or(
            Value::Null,
            |id| json!({ "id": id, "name": r.verifier_name }),
        ),
    );
    m.insert("verified_at".into(), iso8601_utc(r.verified_at));
    m.insert(
        "surveyor".into(),
        r.surveyor_id.map_or(
            Value::Null,
            |id| json!({ "id": id, "name": r.surveyor_name }),
        ),
    );
    if with_tugas {
        m.insert(
            "tugas".into(),
            r.tugas_row_id.map_or(
                Value::Null,
                |id| json!({ "id": id, "judul": r.tugas_judul, "status": r.tugas_status }),
            ),
        );
    }
    m.insert("foto".into(), Value::Array(fotos));
    m.insert("created_at".into(), iso8601_utc(r.created_at));
    m.insert("updated_at".into(), iso8601_utc(r.updated_at));
    Value::Object(m)
}

/// Muat satu survei lengkap untuk respons. Tidak ada = 404.
async fn respond_survey(state: &AppState, id: i64, with_tugas: bool) -> Result<Value, ApiError> {
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut fotos = fotos_of(&state.pool, &state.app_url, &[id]).await?;
    Ok(resource(
        &row,
        fotos.remove(&id).unwrap_or_default(),
        with_tugas,
    ))
}

fn base_url(state: &AppState) -> String {
    foto::base_url(state)
}

/// `parse_id`: id yang bukan angka dianggap tidak ditemukan (MySQL membacanya sebagai 0).
fn parse_id(raw: &str) -> Result<i64, ApiError> {
    foto::parse_id(raw)
}

/// `(int)` PHP: angka di awal teks, selain itu 0.
fn php_intval(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().map(|n| sign * n).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tugas survei
// ---------------------------------------------------------------------------

struct TugasAccess {
    jenis: Option<String>,
    status: String,
    is_assignee: bool,
}

/// Tugas beserta apakah `user_id` penanggung jawabnya (`isAssignee`). `None` = tugas tidak ada.
async fn tugas_access<'c, E>(
    exec: E,
    tugas_id: i64,
    user_id: u64,
) -> Result<Option<TugasAccess>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(jenis AS CHAR) AS jenis, CAST(status AS CHAR) AS status, \
         CAST((COALESCE(assignee_id, 0) = ? OR EXISTS (SELECT 1 FROM tbl_survey_tugas_assignees a \
         WHERE a.survey_tugas_id = tbl_survey_tugas.id AND a.user_id = ?)) AS SIGNED) AS is_assignee \
         FROM tbl_survey_tugas WHERE id = ?",
    )
    .bind(user_id as i64)
    .bind(user_id as i64)
    .bind(tugas_id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else { return Ok(None) };
    Ok(Some(TugasAccess {
        jenis: r.try_get("jenis").map_err(internal)?,
        status: r.try_get("status").map_err(internal)?,
        is_assignee: r.try_get::<i64, _>("is_assignee").map_err(internal)? != 0,
    }))
}

// ---------------------------------------------------------------------------
// Atribut dan audit
// ---------------------------------------------------------------------------

const SELECT_ATTRS: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, \
     CAST(tugas_id AS SIGNED) AS tugas_id, CAST(jenis AS CHAR) AS jenis, nama_lokasi, \
     CAST(kecamatan_id AS SIGNED) AS kecamatan_id, CAST(desa_id AS SIGNED) AS desa_id, alamat, \
     CAST(latitude AS CHAR) AS latitude, CAST(longitude AS CHAR) AS longitude, CAST(detail AS CHAR) AS detail, \
     CAST(status AS CHAR) AS status, catatan_verifikasi, CAST(verified_by AS SIGNED) AS verified_by, \
     CAST(verified_at AS CHAR) AS verified_at, CAST(created_at AS CHAR) AS created_at, \
     CAST(updated_at AS CHAR) AS updated_at FROM tbl_survey_lokasi WHERE id = ?";

/// Atribut survei seperti `getAttributes()` untuk audit (kunci = kolom).
async fn attributes_survey<'c, E>(exec: E, id: i64) -> Result<Map<String, Value>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let row = sqlx::query(SELECT_ATTRS)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let mut m = Map::new();
    for col in [
        "id",
        "user_id",
        "tugas_id",
        "kecamatan_id",
        "desa_id",
        "verified_by",
    ] {
        m.insert(
            col.into(),
            json!(row.try_get::<Option<i64>, _>(col).map_err(internal)?),
        );
    }
    for col in [
        "jenis",
        "nama_lokasi",
        "alamat",
        "latitude",
        "longitude",
        "status",
        "catatan_verifikasi",
        "verified_at",
        "created_at",
        "updated_at",
    ] {
        m.insert(
            col.into(),
            json!(row.try_get::<Option<String>, _>(col).map_err(internal)?),
        );
    }
    let detail: Option<String> = row.try_get("detail").map_err(internal)?;
    m.insert(
        "detail".into(),
        detail
            .as_deref()
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .unwrap_or(Value::Null),
    );
    Ok(m)
}

async fn attributes_tugas<'c, E>(exec: E, id: i64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(status AS CHAR) AS status, CAST(updated_at AS CHAR) AS updated_at \
         FROM tbl_survey_tugas WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else { return Ok(None) };
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert(
        "status".into(),
        json!(r.try_get::<Option<String>, _>("status").map_err(internal)?),
    );
    m.insert(
        "updated_at".into(),
        json!(r
            .try_get::<Option<String>, _>("updated_at")
            .map_err(internal)?),
    );
    Ok(Some(m))
}

/// Selisih atribut seperti `getDirty()`: `updated_at` ikut dicatat hanya bila ada perubahan lain.
fn diff(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<(Map<String, Value>, Map<String, Value>)> {
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if new.is_empty() {
        return None;
    }
    old.insert(
        "updated_at".into(),
        before.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    new.insert(
        "updated_at".into(),
        after.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    Some((old, new))
}

/// Setelah `UPDATE` tanpa `updated_at`: bila ada perubahan, set `updated_at = NOW()` dan catat audit `updated`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn audit_update(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    table: &str,
    model: &str,
    id: i64,
    before: &Map<String, Value>,
    mut after: Map<String, Value>,
    url: &str,
) -> Result<(), ApiError> {
    if diff(before, &after).is_none() {
        return Ok(());
    }
    sqlx::query(&format!(
        "UPDATE {table} SET updated_at = NOW() WHERE id = ?"
    ))
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    let now: Option<String> = sqlx::query_scalar(&format!(
        "SELECT CAST(updated_at AS CHAR) FROM {table} WHERE id = ?"
    ))
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    after.insert("updated_at".into(), json!(now));
    if let Some((old, new)) = diff(before, &after) {
        changes::audit_only(
            tx,
            headers,
            actor,
            model,
            "updated",
            id,
            Some(old),
            Some(new),
            url,
        )
        .await?;
    }
    Ok(())
}

/// Ubah status tugas induk (`$tugas->update(['status' => ...])`) dan catat audit bila berubah.
async fn set_tugas_status(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    tugas_id: i64,
    status: &str,
    url: &str,
) -> Result<(), ApiError> {
    let Some(before) = attributes_tugas(&mut **tx, tugas_id).await? else {
        return Ok(());
    };
    sqlx::query("UPDATE tbl_survey_tugas SET status = ? WHERE id = ?")
        .bind(status)
        .bind(tugas_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    let after = attributes_tugas(&mut **tx, tugas_id)
        .await?
        .unwrap_or_default();
    audit_update(
        tx,
        headers,
        actor,
        TUGAS_TABLE,
        TUGAS_MODEL,
        tugas_id,
        &before,
        after,
        url,
    )
    .await
}

// ---------------------------------------------------------------------------
// Input JSON atau multipart
// ---------------------------------------------------------------------------

/// Field teks dinormalisasi seperti `TrimStrings` dan `ConvertEmptyStringsToNull`.
fn text_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        Value::Null
    } else {
        Value::String(t.to_string())
    }
}

/// Untuk JSON: rapikan string di semua kedalaman (seperti `TrimStrings` yang rekursif).
fn normalize(v: Value) -> Value {
    match v {
        Value::String(s) => text_value(&s),
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

#[derive(Default)]
struct Input {
    fields: Map<String, Value>,
    /// Berkas dari `foto` atau `foto[]` (multipart). Berkas kosong diabaikan.
    files: Vec<media::Upload>,
}

async fn read_input(state: &AppState, req: Request) -> Result<Input, ApiError> {
    let is_multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    let mut input = Input::default();
    if is_multipart {
        let mut mp = Multipart::from_request(req, state)
            .await
            .map_err(bad_request)?;
        while let Some(field) = mp.next_field().await.map_err(bad_request)? {
            let name = field.name().unwrap_or_default().to_string();
            if name == "foto" || name == "foto[]" {
                let original_name = field.file_name().unwrap_or_default().to_string();
                let bytes = field.bytes().await.map_err(bad_request)?;
                if !bytes.is_empty() {
                    input.files.push(media::Upload {
                        original_name,
                        bytes: bytes.to_vec(),
                    });
                }
                continue;
            }
            let text = field.text().await.map_err(bad_request)?;
            let value = text_value(&text);
            match name.strip_suffix("[]") {
                Some(base) => {
                    let entry = input
                        .fields
                        .entry(base.to_string())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if let Value::Array(items) = entry {
                        items.push(value);
                    }
                }
                None => {
                    input.fields.insert(name, value);
                }
            }
        }
    } else {
        let bytes = Bytes::from_request(req, state).await.map_err(bad_request)?;
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes).map_err(bad_request)?;
            if let Value::Object(obj) = v {
                for (k, val) in obj {
                    input.fields.insert(k, normalize(val));
                }
            }
        }
    }
    Ok(input)
}

/// `normalizeDetailInput`: `detail` berupa string JSON yang berisi array/objek didekodekan.
fn normalize_detail(fields: &mut Map<String, Value>) {
    let decoded = match fields.get("detail") {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .ok()
            .filter(|v| v.is_array() || v.is_object()),
        _ => None,
    };
    if let Some(v) = decoded {
        fields.insert("detail".into(), v);
    }
}

fn text_of(f: &Map<String, Value>, key: &str) -> Option<String> {
    match f.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn int_of(f: &Map<String, Value>, key: &str) -> Option<i64> {
    int_value(f.get(key)?)
}

/// Kolom `detail` sebagai JSON teks. Objek kosong disimpan sebagai `[]` (seperti PHP).
fn detail_json(f: &Map<String, Value>) -> Option<String> {
    match f.get("detail")? {
        Value::Null => None,
        Value::Object(m) if m.is_empty() => Some("[]".to_string()),
        v => Some(v.to_string()),
    }
}

/// Kategori per berkas, sejajar dengan urutan `foto[]`. Kosong = tanpa kategori.
fn kategori_list(f: &Map<String, Value>) -> Vec<Option<String>> {
    match f.get("foto_kategori") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Store,
    Update,
}

/// Aturan satu field. Field yang tidak ada atau `null` dilewati (`nullable`).
enum Rule {
    Str(usize),
    Num,
    /// `numeric|min:x`
    NumMin(f64),
    /// `numeric|min:x|max:y`
    NumRange(f64, f64),
    /// `numeric|between:x,y`
    Between(f64, f64),
    /// `integer|min:x`
    Int(i64),
    In(&'static [&'static str]),
    Bool,
    Date,
    Arr,
}

fn attribute(key: &str) -> String {
    key.replace('_', " ")
}

fn fmt_num(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

fn is_numeric(v: &Value) -> bool {
    match v {
        Value::Number(_) => true,
        Value::String(s) => {
            let t = s.trim();
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
                && t.parse::<f64>().is_ok()
        }
        _ => false,
    }
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Nilai skalar sebagai teks, untuk `in:` (seperti `(string)` PHP).
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) => int_value(v)
            .map(|i| i.to_string())
            .or_else(|| Some(v.to_string())),
        Value::Bool(b) => Some(if *b { "1" } else { "" }.to_string()),
        _ => None,
    }
}

fn is_bool(v: &Value) -> bool {
    match v {
        Value::Bool(_) => true,
        Value::Number(_) => matches!(int_value(v), Some(0 | 1)),
        Value::String(s) => s == "0" || s == "1",
        _ => false,
    }
}

fn is_date(s: &str) -> bool {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
        || [
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M",
        ]
        .iter()
        .any(|f| NaiveDateTime::parse_from_str(s, f).is_ok())
        || DateTime::parse_from_rfc3339(s).is_ok()
}

/// Pesan pertama untuk satu aturan. `None` = lolos.
fn rule_error(key: &str, v: &Value, rule: &Rule) -> Option<String> {
    let a = attribute(key);
    let not_number = || format!("The {a} field must be a number.");
    match rule {
        Rule::Str(max) => match v {
            Value::String(s) if s.chars().count() > *max => Some(format!(
                "The {a} field must not be greater than {max} characters."
            )),
            Value::String(_) => None,
            _ => Some(format!("The {a} field must be a string.")),
        },
        Rule::Num => (!is_numeric(v)).then(not_number),
        Rule::NumMin(lo) => {
            if !is_numeric(v) {
                return Some(not_number());
            }
            (num(v) < *lo).then(|| format!("The {a} field must be at least {}.", fmt_num(*lo)))
        }
        Rule::NumRange(lo, hi) => {
            if !is_numeric(v) {
                return Some(not_number());
            }
            let n = num(v);
            if n < *lo {
                Some(format!("The {a} field must be at least {}.", fmt_num(*lo)))
            } else if n > *hi {
                Some(format!(
                    "The {a} field must not be greater than {}.",
                    fmt_num(*hi)
                ))
            } else {
                None
            }
        }
        Rule::Between(lo, hi) => {
            if !is_numeric(v) {
                return Some(not_number());
            }
            let n = num(v);
            (n < *lo || n > *hi).then(|| {
                format!(
                    "The {a} field must be between {} and {}.",
                    fmt_num(*lo),
                    fmt_num(*hi)
                )
            })
        }
        Rule::Int(min) => match int_value(v) {
            None => Some(format!("The {a} field must be an integer.")),
            Some(n) if n < *min => Some(format!("The {a} field must be at least {min}.")),
            Some(_) => None,
        },
        Rule::In(list) => match scalar(v) {
            Some(s) if list.contains(&s.as_str()) => None,
            _ => Some(format!("The selected {a} is invalid.")),
        },
        Rule::Bool => (!is_bool(v)).then(|| format!("The {a} field must be true or false.")),
        Rule::Date => match v {
            Value::String(s) if is_date(s) => None,
            _ => Some(format!("The {a} field must be a valid date.")),
        },
        Rule::Arr => (!matches!(v, Value::Array(_) | Value::Object(_)))
            .then(|| format!("The {a} field must be an array.")),
    }
}

#[derive(Default)]
struct Errs(BTreeMap<String, String>);

impl Errs {
    fn add(&mut self, key: &str, message: String) {
        self.0.entry(key.to_string()).or_insert(message);
    }

    fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    fn finish(self) -> Result<(), ApiError> {
        if self.0.is_empty() {
            return Ok(());
        }
        let map = self.0.into_iter().map(|(k, v)| (k, vec![v])).collect();
        Err(ApiError::validation("Validation error", map))
    }
}

/// `nullable`: null atau tidak ada dilewati.
fn check(e: &mut Errs, key: &str, v: Option<&Value>, rule: &Rule) {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return;
    };
    if e.has(key) {
        return;
    }
    if let Some(message) = rule_error(key, v, rule) {
        e.add(key, message);
    }
}

/// `required` (store), atau `sometimes|required` (update: hanya bila field ada).
fn require(e: &mut Errs, key: &str, v: Option<&Value>, rule: &Rule, mode: Mode) {
    match v.filter(|v| !v.is_null()) {
        Some(v) => {
            if let Some(message) = rule_error(key, v, rule) {
                e.add(key, message);
            }
        }
        None if mode == Mode::Update && v.is_none() => {}
        None => e.add(key, format!("The {} field is required.", attribute(key))),
    }
}

/// `exists:{table},id`.
async fn check_exists(
    pool: &MySqlPool,
    e: &mut Errs,
    key: &str,
    v: Option<&Value>,
    table: &str,
) -> Result<(), ApiError> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let found = match int_value(v) {
        Some(id) => {
            let n: i64 = sqlx::query_scalar(&format!(
                "SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?"
            ))
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(internal)?;
            n > 0
        }
        None => false,
    };
    if !found {
        e.add(key, format!("The selected {} is invalid.", attribute(key)));
    }
    Ok(())
}

/// `mimes` dan `max:10240` untuk satu berkas.
fn file_error(key: &str, u: &media::Upload) -> Option<String> {
    let a = attribute(key);
    let ext = u.extension().to_ascii_lowercase();
    if !FOTO_EXT.contains(&ext.as_str()) {
        return Some(format!(
            "The {a} field must be a file of type: {}.",
            FOTO_EXT.join(", ")
        ));
    }
    if u.bytes.len() > FOTO_MAX_KB * 1024 {
        return Some(format!(
            "The {a} field must not be greater than {FOTO_MAX_KB} kilobytes."
        ));
    }
    None
}

/// Aturan `detail.*` (urut seperti `SurveyLokasiController::rules()`).
const DETAIL: &[(&str, Rule)] = &[
    ("sumber_air", Rule::Str(255)),
    ("debit_liter_detik", Rule::NumMin(0.0)),
    ("jumlah_kk", Rule::Int(0)),
    ("kebutuhan", Rule::In(&["baru", "rehab", "perluasan"])),
    ("tipe", Rule::In(&["individu", "komunal"])),
    ("jumlah_bilik", Rule::Int(0)),
    ("kedalaman_rencana_m", Rule::NumMin(0.0)),
    ("tanggal_survei", Rule::Date),
    ("nama_surveyor_tim", Rule::Str(255)),
    ("dusun", Rule::Str(255)),
    ("rt", Rule::Str(20)),
    ("rw", Rule::Str(20)),
    ("broncaptering_lat", Rule::Between(-90.0, 90.0)),
    ("broncaptering_lng", Rule::Between(-180.0, 180.0)),
    ("broncaptering_elevasi", Rule::Num),
    ("reservoir_lat", Rule::Between(-90.0, 90.0)),
    ("reservoir_lng", Rule::Between(-180.0, 180.0)),
    ("reservoir_elevasi", Rule::Num),
    (
        "sumber_air_jenis",
        Rule::In(&[
            "mata_air_terjun",
            "rembesan",
            "umbul",
            "sungai",
            "sumur_bor",
        ]),
    ),
    ("debit_hujan_lps", Rule::NumMin(0.0)),
    ("debit_hujan_bulan", Rule::Str(50)),
    ("debit_kemarau_lps", Rule::NumMin(0.0)),
    ("debit_kemarau_bulan", Rule::Str(50)),
    ("kejernihan", Rule::In(&["jernih", "keruh", "berwarna"])),
    ("bau_rasa", Rule::In(&["berbau_berasa", "tidak"])),
    ("ph", Rule::NumRange(0.0, 14.0)),
    (
        "lahan_broncaptering_status",
        Rule::In(&["tanah_desa", "milik_warga", "hutan"]),
    ),
    ("elevasi_kelayakan", Rule::In(&["gravitasi", "pompa"])),
    ("jarak_sumber_reservoir_m", Rule::NumMin(0.0)),
    (
        "reservoir_lahan_status",
        Rule::In(&["tanah_desa", "hibah_warga", "lainnya"]),
    ),
    ("reservoir_lahan_lainnya", Rule::Str(255)),
    ("topografi", Rule::In(&["datar", "miring", "rawan_longsor"])),
    ("selisih_elevasi_m", Rule::Num),
    (
        "akses_material",
        Rule::In(&["mobil_truk", "motor_roda3", "jalan_kaki"]),
    ),
    ("pipa_trunk_m", Rule::NumMin(0.0)),
    ("pipa_cabang_m", Rule::NumMin(0.0)),
    ("lintas_tanah_m", Rule::NumMin(0.0)),
    ("lintas_paving_m", Rule::NumMin(0.0)),
    ("lintas_aspal_m", Rule::NumMin(0.0)),
    ("lintas_sungai_titik", Rule::Int(0)),
    ("lintas_sungai_lebar_m", Rule::NumMin(0.0)),
    ("washout_titik", Rule::Int(0)),
    ("air_valve_titik", Rule::Int(0)),
    ("total_jiwa", Rule::Int(0)),
    ("sumber_eksisting", Rule::In(&["sumur", "irigasi", "beli"])),
    ("kesediaan_pelanggan", Rule::In(&["ya", "tidak"])),
    ("kesediaan_persen", Rule::Between(0.0, 100.0)),
    ("kesediaan_iuran", Rule::In(&["setuju", "tidak_setuju"])),
    ("tarif_perkiraan", Rule::NumMin(0.0)),
    ("bnba", Rule::Str(10_000)),
    ("sumur_lat", Rule::Between(-90.0, 90.0)),
    ("sumur_lng", Rule::Between(-180.0, 180.0)),
    ("sumur_elevasi", Rule::Num),
    ("ku1_lat", Rule::Between(-90.0, 90.0)),
    ("ku1_lng", Rule::Between(-180.0, 180.0)),
    ("ku2_lat", Rule::Between(-90.0, 90.0)),
    ("ku2_lng", Rule::Between(-180.0, 180.0)),
    (
        "lahan_sumur_status",
        Rule::In(&["tanah_kas_desa", "hibah_warga", "lainnya"]),
    ),
    ("lahan_sumur_lainnya", Rule::Str(255)),
    ("lahan_panjang_m", Rule::NumMin(0.0)),
    ("lahan_lebar_m", Rule::NumMin(0.0)),
    ("akuifer_kedalaman_m", Rule::NumMin(0.0)),
    ("sumur_warga_kedalaman_m", Rule::NumMin(0.0)),
    (
        "air_warga_kualitas",
        Rule::In(&["jernih", "berbau", "asin_payau", "besi_mangan"]),
    ),
    ("listrik_jarak_m", Rule::NumMin(0.0)),
    (
        "listrik_daya",
        Rule::In(&["belum_ada", "900", "1300", "2200"]),
    ),
    ("akses_rig", Rule::In(&["truk", "pickup", "portable"])),
    (
        "tanah_menara",
        Rule::In(&["keras", "sawah_rawa", "miring_tebing"]),
    ),
    ("menara_tinggi", Rule::In(&["3", "5", "6"])),
    (
        "toren_kapasitas",
        Rule::In(&["1000", "2000", "4000", "lainnya"]),
    ),
    ("toren_kapasitas_lainnya", Rule::Str(50)),
    ("toren_bahan", Rule::In(&["pe", "stainless"])),
    ("jumlah_ku_titik", Rule::Int(0)),
    ("ku_rincian", Rule::Str(5_000)),
    ("kran_per_titik", Rule::In(&["2", "4"])),
    ("drainase", Rule::In(&["ada_saluran", "perlu_resapan"])),
    ("kesediaan_kelompok", Rule::In(&["ya", "tidak"])),
    ("iuran_listrik", Rule::In(&["ya", "tidak"])),
    ("mck_lat", Rule::Between(-90.0, 90.0)),
    ("mck_lng", Rule::Between(-180.0, 180.0)),
    ("mck_elevasi", Rule::Num),
    ("jumlah_pintu", Rule::In(&["1", "2", "3", "4", "lainnya"])),
    ("jumlah_pintu_lainnya", Rule::Str(100)),
    ("bilik1_fungsi", Rule::In(&["jongkok", "duduk", "mandi"])),
    ("bilik2_fungsi", Rule::In(&["jongkok", "duduk", "mandi"])),
    ("bilik3_fungsi", Rule::In(&["jongkok", "duduk", "mandi"])),
    ("bilik4_fungsi", Rule::In(&["jongkok", "duduk", "mandi"])),
    ("kloset_jenis", Rule::In(&["leher_angsa", "duduk"])),
    ("wudhu_ada", Rule::In(&["ada", "tidak"])),
    ("wudhu_keran", Rule::Int(0)),
    (
        "wudhu_desain",
        Rule::In(&["dinding_luar", "kanopi", "duduk_beton"]),
    ),
    (
        "mck_sumber_air",
        Rule::In(&["spam_desa", "sumur", "mata_air"]),
    ),
    ("toren_menara", Rule::In(&["ada", "tidak"])),
    ("toren_dak", Rule::In(&["tidak", "500", "1000", "2000"])),
    ("septik_jenis", Rule::In(&["biofilter", "konvensional"])),
    ("septik_bio_kapasitas_m3", Rule::NumMin(0.0)),
    ("septik_bio_pengguna", Rule::Int(0)),
    ("septik_panjang_m", Rule::NumMin(0.0)),
    ("septik_lebar_m", Rule::NumMin(0.0)),
    ("septik_dalam_m", Rule::NumMin(0.0)),
    ("resapan_jenis", Rule::In(&["sumur", "trench", "drainase"])),
    ("resapan_diameter_m", Rule::NumMin(0.0)),
    ("resapan_dalam_m", Rule::NumMin(0.0)),
    ("tanah_jenis", Rule::In(&["pasir", "liat", "batuan"])),
    ("muka_air_tanah_m", Rule::NumMin(0.0)),
    ("jarak_septik_sumur_m", Rule::NumMin(0.0)),
    ("nama_kk", Rule::Str(255)),
    ("anggota_jiwa", Rule::Int(0)),
    ("status_ekonomi", Rule::In(&["mbr", "non_mbr"])),
    ("target_warga_kk", Rule::Int(0)),
    ("target_warga_jiwa", Rule::Int(0)),
    ("target_jamaah", Rule::Int(0)),
    ("target_santri", Rule::Int(0)),
    ("pengelola", Rule::In(&["musala", "ksm", "bumdes"])),
    ("dok_foto_broncaptering", Rule::Bool),
    ("dok_foto_reservoir", Rule::Bool),
    ("dok_peta_jalur", Rule::Bool),
    ("dok_surat_hibah", Rule::Bool),
    ("dok_bnba", Rule::Bool),
];

/// Aturan `store` dan `update` (`rules($isUpdate)`).
async fn validate_survey(pool: &MySqlPool, input: &Input, mode: Mode) -> Result<(), ApiError> {
    let f = &input.fields;
    let mut e = Errs::default();
    require(&mut e, "jenis", f.get("jenis"), &Rule::In(JENIS), mode);
    check_exists(pool, &mut e, "tugas_id", f.get("tugas_id"), TUGAS_TABLE).await?;
    require(
        &mut e,
        "nama_lokasi",
        f.get("nama_lokasi"),
        &Rule::Str(255),
        mode,
    );
    check_exists(
        pool,
        &mut e,
        "kecamatan_id",
        f.get("kecamatan_id"),
        "tbl_kecamatan",
    )
    .await?;
    check_exists(pool, &mut e, "desa_id", f.get("desa_id"), "tbl_desa").await?;
    check(&mut e, "alamat", f.get("alamat"), &Rule::Str(usize::MAX));
    check(
        &mut e,
        "latitude",
        f.get("latitude"),
        &Rule::Between(-90.0, 90.0),
    );
    check(
        &mut e,
        "longitude",
        f.get("longitude"),
        &Rule::Between(-180.0, 180.0),
    );
    check(&mut e, "detail", f.get("detail"), &Rule::Arr);
    if let Some(Value::Object(detail)) = f.get("detail") {
        for (key, rule) in DETAIL {
            check(&mut e, &format!("detail.{key}"), detail.get(*key), rule);
        }
    }
    for (i, file) in input.files.iter().enumerate() {
        let key = format!("foto.{i}");
        if let Some(message) = file_error(&key, file) {
            e.add(&key, message);
        }
    }
    check(&mut e, "foto_kategori", f.get("foto_kategori"), &Rule::Arr);
    if let Some(Value::Array(items)) = f.get("foto_kategori") {
        for (i, item) in items.iter().enumerate() {
            check(
                &mut e,
                &format!("foto_kategori.{i}"),
                Some(item),
                &Rule::Str(50),
            );
        }
    }
    e.finish()
}

/// Aturan unggah foto: `foto` wajib berkas tunggal, `kategori` opsional.
fn validate_upload(input: &Input) -> Result<(), ApiError> {
    let mut e = Errs::default();
    match input.files.as_slice() {
        [] => e.add("foto", "The foto field is required.".into()),
        [file] => {
            if let Some(message) = file_error("foto", file) {
                e.add("foto", message);
            }
        }
        _ => e.add("foto", "The foto field must be a file.".into()),
    }
    check(
        &mut e,
        "kategori",
        input.fields.get("kategori"),
        &Rule::Str(50),
    );
    e.finish()
}

/// Aturan `verifikasi`. Mengembalikan status yang sudah valid.
fn validate_verifikasi(input: &Input) -> Result<String, ApiError> {
    let f = &input.fields;
    let mut e = Errs::default();
    require(
        &mut e,
        "status",
        f.get("status"),
        &Rule::In(&["diverifikasi", "ditolak"]),
        Mode::Store,
    );
    check(
        &mut e,
        "catatan_verifikasi",
        f.get("catatan_verifikasi"),
        &Rule::Str(usize::MAX),
    );
    let ditolak = text_of(f, "status").as_deref() == Some("ditolak");
    let catatan_kosong = f
        .get("catatan_verifikasi")
        .filter(|v| !v.is_null())
        .is_none();
    if ditolak && catatan_kosong {
        e.add(
            "catatan_verifikasi",
            "Catatan verifikasi wajib diisi jika survei ditolak.".into(),
        );
    }
    e.finish()?;
    Ok(text_of(f, "status").unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Penyimpanan berkas
// ---------------------------------------------------------------------------

/// Simpan berkas foto dalam transaksi dan isi kategori (`withCustomProperties`).
/// Pada kegagalan, direktori berkas yang sudah ditulis dihapus.
async fn attach_fotos(
    tx: &mut Transaction<'_, MySql>,
    survey_id: i64,
    files: &[media::Upload],
    kategori: &[Option<String>],
) -> Result<Vec<PathBuf>, ApiError> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let mime = media::mime_for_name(&file.original_name);
        let stored =
            match media::attach(tx, MODEL, survey_id as u64, COLLECTION, file, mime, false).await {
                Ok(s) => s,
                Err(e) => {
                    media::remove_dirs(&dirs).await;
                    return Err(e);
                }
            };
        dirs.push(stored.dir.clone());
        if let Some(Some(k)) = kategori.get(i) {
            let props = json!({ "kategori": k }).to_string();
            let res = sqlx::query("UPDATE media SET custom_properties = ? WHERE id = ?")
                .bind(props)
                .bind(stored.media_id)
                .execute(&mut **tx)
                .await;
            if let Err(e) = res {
                media::remove_dirs(&dirs).await;
                return Err(internal(e));
            }
        }
    }
    Ok(dirs)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/survey-lokasi/stats`: jumlah per jenis dan per status.
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let by_jenis_rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT CAST(jenis AS CHAR), CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi GROUP BY jenis",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let by_status_rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT CAST(status AS CHAR), CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi GROUP BY status",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    let count_of = |rows: &[(String, i64)], key: &str| -> i64 {
        rows.iter().filter(|(k, _)| k == key).map(|(_, n)| *n).sum()
    };
    let mut by_jenis = Map::new();
    for k in JENIS {
        by_jenis.insert((*k).to_string(), json!(count_of(&by_jenis_rows, k)));
    }
    let mut by_status = Map::new();
    let mut total = 0;
    for k in ["diajukan", "diverifikasi", "ditolak"] {
        let n = count_of(&by_status_rows, k);
        total += n;
        by_status.insert(k.to_string(), json!(n));
    }
    Ok(Json(json!({
        "by_jenis": by_jenis,
        "by_status": by_status,
        "total": total,
    })))
}

/// `GET /api/survey-lokasi`: paginator, terbaru dulu. Filter `jenis`, `status`, `kecamatan_id`,
/// `desa_id`, `tugas_id`, `search` (nama lokasi atau alamat, `LIKE`). `per_page` 1 sampai 100.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let filled = |key: &str| q.get(key).map(|v| v.trim()).filter(|v| !v.is_empty());
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    for (param, col) in [
        ("jenis", "s.jenis"),
        ("status", "s.status"),
        ("kecamatan_id", "s.kecamatan_id"),
        ("desa_id", "s.desa_id"),
        ("tugas_id", "s.tugas_id"),
    ] {
        if let Some(v) = filled(param) {
            conds.push(format!("{col} = ?"));
            binds.push(v.to_string());
        }
    }
    if let Some(s) = filled("search") {
        conds.push("(s.nama_lokasi LIKE ? OR s.alamat LIKE ?)".into());
        binds.push(format!("%{s}%"));
        binds.push(format!("%{s}%"));
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let per_raw = match q.get("per_page") {
        Some(v) => php_intval(v),
        None => 15,
    };
    let per_page = per_raw.clamp(1, 100) as u64;
    let page = pagination::page_params(&q).page;

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi s{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b.clone());
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)? as u64;

    let sql = format!(
        "{SELECT_SURVEY}{where_sql} ORDER BY s.created_at DESC, s.id DESC LIMIT {per_page} OFFSET {}",
        (page - 1) * per_page
    );
    let mut dq = sqlx::query(&sql);
    for b in &binds {
        dq = dq.bind(b.clone());
    }
    let rows = dq.fetch_all(&state.pool).await.map_err(internal)?;
    let rows: Vec<SurveyRow> = rows
        .iter()
        .map(map_row)
        .collect::<Result<_, _>>()
        .map_err(internal)?;
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let mut fotos = fotos_of(&state.pool, &state.app_url, &ids).await?;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| resource(r, fotos.remove(&r.id).unwrap_or_default(), true))
        .collect();
    let base = format!("{}/api/survey-lokasi", base_url(&state));
    Ok(Json(pagination::paginate(
        data,
        total,
        PageParams { page, per_page },
        &base,
    )))
}

/// `POST /api/survey-lokasi` (JSON atau multipart). Respons 201.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
) -> Result<Response, ApiError> {
    let actor = actor(&state, &headers).await?;
    if !actor.can_survey {
        return Err(deny_role());
    }
    let mut input = read_input(&state, req).await?;
    normalize_detail(&mut input.fields);

    // `$request->filled('tugas_id')`: jenis diambil dari tugas bila kosong.
    if let Some(tid) = int_of(&input.fields, "tugas_id") {
        if let Some(tugas) = tugas_access(&state.pool, tid, actor.id).await? {
            if let Some(tj) = tugas.jenis {
                if text_of(&input.fields, "jenis").is_none() {
                    input.fields.insert("jenis".into(), json!(tj));
                }
            }
        }
    }
    validate_survey(&state.pool, &input, Mode::Store).await?;

    let f = &input.fields;
    let tugas_id = int_of(f, "tugas_id");
    let tugas = match tugas_id {
        Some(tid) => {
            let t = tugas_access(&state.pool, tid, actor.id)
                .await?
                .ok_or_else(tugas_not_found)?;
            if !actor.is_admin && !t.is_assignee {
                return Err(forbidden("Forbidden"));
            }
            Some(t)
        }
        None => None,
    };
    let jenis = text_of(f, "jenis").unwrap_or_default();
    if let Some(tj) = tugas.as_ref().and_then(|t| t.jenis.as_ref()) {
        if jenis != *tj {
            return Err(jenis_mismatch());
        }
    }

    let url = format!("{}/api/survey-lokasi", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_survey_lokasi (user_id, tugas_id, jenis, nama_lokasi, kecamatan_id, desa_id, alamat, \
         latitude, longitude, detail, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'diajukan', NOW(), NOW())",
    )
    .bind(actor.id as i64)
    .bind(tugas_id)
    .bind(&jenis)
    .bind(text_of(f, "nama_lokasi").unwrap_or_default())
    .bind(int_of(f, "kecamatan_id"))
    .bind(int_of(f, "desa_id"))
    .bind(text_of(f, "alamat"))
    .bind(text_of(f, "latitude"))
    .bind(text_of(f, "longitude"))
    .bind(detail_json(f))
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    let created = attributes_survey(&mut *tx, id).await?;
    changes::audit_only(
        &mut tx,
        &headers,
        actor.id,
        MODEL,
        "created",
        id,
        None,
        Some(created),
        &url,
    )
    .await?;

    if let (Some(tid), Some(t)) = (tugas_id, &tugas) {
        if t.status == "ditugaskan" {
            set_tugas_status(&mut tx, &headers, actor.id, tid, "dikerjakan", &url).await?;
        }
    }

    let kategori = kategori_list(f);
    let dirs = attach_fotos(&mut tx, id, &input.files, &kategori).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(e));
    }

    let data = respond_survey(&state, id, true).await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))).into_response())
}

/// `GET /api/survey-lokasi/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let data = respond_survey(&state, id, true).await?;
    Ok(Json(json!({ "data": data })))
}

/// `PUT`/`PATCH /api/survey-lokasi/{id}` (JSON atau multipart).
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_editable(
        &actor,
        &row,
        "Hanya survei berstatus diajukan yang dapat diubah.",
    )?;

    let mut input = read_input(&state, req).await?;
    normalize_detail(&mut input.fields);
    validate_survey(&state.pool, &input, Mode::Update).await?;

    // `tugas_id` yang ada di request (termasuk null) menggantikan tugas lama.
    let tugas_id = if input.fields.contains_key("tugas_id") {
        int_of(&input.fields, "tugas_id")
    } else {
        row.tugas_id
    };
    if let Some(tid) = tugas_id {
        let tugas = tugas_access(&state.pool, tid, actor.id)
            .await?
            .ok_or_else(tugas_not_found)?;
        if !actor.is_admin && !tugas.is_assignee {
            return Err(forbidden("Forbidden"));
        }
        if let Some(tj) = tugas.jenis {
            let jenis_baru = if input.fields.contains_key("jenis") {
                text_of(&input.fields, "jenis")
            } else {
                Some(row.jenis.clone())
            };
            match jenis_baru {
                None => {
                    input.fields.insert("jenis".into(), json!(tj));
                }
                Some(j) if j != tj => return Err(jenis_mismatch()),
                Some(_) => {}
            }
        }
    }

    let f = &input.fields;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes_survey(&mut *tx, id).await?;

    let cols: Vec<&str> = UPDATABLE
        .iter()
        .copied()
        .filter(|k| f.contains_key(*k))
        .collect();
    if !cols.is_empty() {
        let sets = cols
            .iter()
            .map(|c| format!("{c} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("UPDATE tbl_survey_lokasi SET {sets} WHERE id = ?");
        let mut q = sqlx::query(&sql);
        for c in &cols {
            q = match *c {
                "tugas_id" | "kecamatan_id" | "desa_id" => q.bind(int_of(f, c)),
                "detail" => q.bind(detail_json(f)),
                _ => q.bind(text_of(f, c)),
            };
        }
        q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    }

    let after = attributes_survey(&mut *tx, id).await?;
    let url = format!("{}/api/survey-lokasi/{id}", base_url(&state));
    audit_update(
        &mut tx,
        &headers,
        actor.id,
        SURVEY_TABLE,
        MODEL,
        id,
        &before,
        after,
        &url,
    )
    .await?;

    let kategori = kategori_list(f);
    let dirs = attach_fotos(&mut tx, id, &input.files, &kategori).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(e));
    }

    let data = respond_survey(&state, id, true).await?;
    Ok(Json(json!({ "data": data })))
}

/// `DELETE /api/survey-lokasi/{id}`: hapus foto koleksi dan baris, lalu catat audit `deleted`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_editable(
        &actor,
        &row,
        "Hanya survei berstatus diajukan yang dapat dihapus.",
    )?;

    let url = format!("{}/api/survey-lokasi/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes_survey(&mut *tx, id).await?;
    changes::audit_only(
        &mut tx,
        &headers,
        actor.id,
        MODEL,
        "deleted",
        id,
        Some(before),
        None,
        &url,
    )
    .await?;
    let dirs = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_survey_lokasi WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(
        json!({ "message": "Survei lokasi berhasil dihapus." }),
    ))
}

/// `POST /api/survey-lokasi/{id}/verifikasi` (admin). Status `diverifikasi` atau `ditolak`.
pub async fn verifikasi(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !actor.is_admin {
        return Err(forbidden("User does not have the right roles."));
    }
    let input = read_input(&state, req).await?;
    let status = validate_verifikasi(&input)?;
    let catatan = text_of(&input.fields, "catatan_verifikasi");

    let url = format!("{}/api/survey-lokasi/{id}/verifikasi", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes_survey(&mut *tx, id).await?;
    sqlx::query(
        "UPDATE tbl_survey_lokasi SET status = ?, catatan_verifikasi = ?, verified_by = ?, verified_at = NOW() \
         WHERE id = ?",
    )
    .bind(&status)
    .bind(catatan)
    .bind(actor.id as i64)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = attributes_survey(&mut *tx, id).await?;
    audit_update(
        &mut tx,
        &headers,
        actor.id,
        SURVEY_TABLE,
        MODEL,
        id,
        &before,
        after,
        &url,
    )
    .await?;

    if let (Some(tid), "diverifikasi") = (row.tugas_id, status.as_str()) {
        set_tugas_status(&mut tx, &headers, actor.id, tid, "selesai", &url).await?;
    }
    tx.commit().await.map_err(internal)?;

    let data = respond_survey(&state, id, true).await?;
    Ok(Json(json!({ "data": data })))
}

/// `POST /api/survey-lokasi/{id}/foto` (multipart: `foto`, `kategori`). Respons 200 tanpa `tugas`.
pub async fn upload_foto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_editable(
        &actor,
        &row,
        "Hanya survei berstatus diajukan yang dapat ditambah fotonya.",
    )?;

    let input = read_input(&state, req).await?;
    validate_upload(&input)?;
    let kategori = vec![text_of(&input.fields, "kategori")];

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs = attach_fotos(&mut tx, id, &input.files, &kategori).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(e));
    }

    let data = respond_survey(&state, id, false).await?;
    Ok(Json(json!({ "data": data })))
}

/// `DELETE /api/survey-lokasi/{id}/foto/{mediaId}`.
pub async fn delete_foto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, media_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let actor = actor(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = load(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    ensure_editable(
        &actor,
        &row,
        "Hanya survei berstatus diajukan yang dapat dihapus fotonya.",
    )?;

    let mid = php_intval(&media_id);
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM media WHERE id = ? AND model_type = ? AND model_id = ? AND collection_name = ?",
    )
    .bind(mid)
    .bind(MODEL)
    .bind(id)
    .bind(COLLECTION)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;
    if found.is_none() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Foto tidak ditemukan.",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM media WHERE id = ?")
        .bind(mid)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&[media::media_dir(mid as u64)]).await;

    Ok(Json(json!({ "message": "Foto berhasil dihapus." })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_intval_reads_leading_digits() {
        assert_eq!(php_intval("12abc"), 12);
        assert_eq!(php_intval("  -7"), -7);
        assert_eq!(php_intval("abc"), 0);
        assert_eq!(php_intval(""), 0);
    }

    #[test]
    fn detail_rules_follow_laravel_ranges() {
        let ph = rule_error("detail.ph", &json!(15), &Rule::NumRange(0.0, 14.0));
        assert_eq!(
            ph.as_deref(),
            Some("The detail.ph field must not be greater than 14.")
        );
        assert!(rule_error("detail.ph", &json!("7.5"), &Rule::NumRange(0.0, 14.0)).is_none());
        let lat = rule_error("broncaptering_lat", &json!(91), &Rule::Between(-90.0, 90.0));
        assert_eq!(
            lat.as_deref(),
            Some("The broncaptering lat field must be between -90 and 90.")
        );
    }

    #[test]
    fn enum_and_bool_rules_accept_laravel_forms() {
        assert!(rule_error("menara_tinggi", &json!(3), &Rule::In(&["3", "5", "6"])).is_none());
        assert!(rule_error("menara_tinggi", &json!("4"), &Rule::In(&["3", "5", "6"])).is_some());
        assert!(rule_error("dok_bnba", &json!(true), &Rule::Bool).is_none());
        assert!(rule_error("dok_bnba", &json!("1"), &Rule::Bool).is_none());
        assert!(rule_error("dok_bnba", &json!("true"), &Rule::Bool).is_some());
    }

    #[test]
    fn date_rule_accepts_common_formats() {
        assert!(is_date("2026-10-08"));
        assert!(is_date("2026-10-08 09:30:00"));
        assert!(is_date("2026-10-08T09:30:00+07:00"));
        assert!(!is_date("08/10/2026 nonsense"));
        assert!(!is_date("2026-02-30"));
    }

    #[test]
    fn diff_includes_updated_at_only_when_something_changed() {
        let before: Map<String, Value> = json!({"nama_lokasi": "a", "updated_at": "t1"})
            .as_object()
            .unwrap()
            .clone();
        let same = before.clone();
        assert!(diff(&before, &same).is_none());
        let changed: Map<String, Value> = json!({"nama_lokasi": "b", "updated_at": "t1"})
            .as_object()
            .unwrap()
            .clone();
        let (old, new) = diff(&before, &changed).unwrap();
        assert_eq!(old.get("nama_lokasi"), Some(&json!("a")));
        assert_eq!(new.get("nama_lokasi"), Some(&json!("b")));
        assert!(old.contains_key("updated_at") && new.contains_key("updated_at"));
    }

    #[test]
    fn detail_string_is_decoded_only_when_array_or_object() {
        let mut f = Map::new();
        f.insert("detail".into(), json!("{\"dusun\":\"A\"}"));
        normalize_detail(&mut f);
        assert_eq!(f.get("detail"), Some(&json!({"dusun": "A"})));
        let mut g = Map::new();
        g.insert("detail".into(), json!("5"));
        normalize_detail(&mut g);
        assert_eq!(g.get("detail"), Some(&json!("5")));
    }
}
