//! `/api/spam-kelembagaan`: port `SpamKelembagaanShareController` untuk rute admin (`auth:sanctum`).
//! Mencakup share link (daftar, buat, ubah, nonaktifkan) dan usulan (daftar, detail, setujui, tolak).
//!
//! Form publik `/api/public/spam-kelembagaan/form/{token}` belum dipindah dan tetap di Laravel.
//!
//! Akses: handler hanya memastikan login. Mutasi oleh non-admin ditolak 403 oleh middleware route
//! permission global (`route_permission::check`), sama dengan `CheckRoutePermission` di Laravel, karena
//! `/spam-kelembagaan` tidak ada di daftar prefix mutasi. Aturan tabel `route_permissions` tetap berlaku.
//!
//! Berbeda dari Laravel atau belum dicocokkan dengan Laravel (vendor tidak ada):
//! - `approve` menjalankan `UPDATE ... WHERE status = 'pending'` lebih dulu sebagai penjaga, sehingga dua
//!   persetujuan bersamaan tidak menerapkan usulan dua kali. Laravel hanya memeriksa status di memori.
//! - Validasi `date` memakai RFC 3339, `Y-m-d H:i:s`, `Y-m-dTH:i:s`, `Y-m-d H:i`, atau `Y-m-d`, bukan
//!   `strtotime`. Waktu tanpa zona dianggap UTC (`app.timezone` = UTC).
//! - `per_page` bernilai 0 atau negatif memakai 15 (default model untuk nilai falsy). Perilaku Laravel
//!   untuk nilai negatif belum diverifikasi.
//! - Urutan daftar dengan `created_at` sama diurutkan `id` menurun sebagai pemutus seri.
//! - Token link: 48 karakter huruf kecil dan angka (`Str::random(48)` lalu `Str::lower`).
//! - Usulan yang disetujui menaikkan generasi cache statistik spam (`bump_spam_generation`). Cache
//!   `dashboard_stats_version` tidak dipindah (lihat `spam_import.rs`).
//! - `payload` dan `snapshot_before` dikembalikan sebagai objek JSON. Urutan kunci mengikuti serde
//!   (alfabetis), bukan urutan yang dikembalikan MySQL.
//! - Nilai 422 di tingkat atas memakai pesan pertama (`validation::Errors`).

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rand::{distributions::Alphanumeric, Rng};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    foto::base_url,
    format::iso8601_utc,
    lookup::carbon_json,
    media::internal,
    require_auth,
    spam_integration::{self as si, Ctx, ModelChange, MODEL_UNIT},
    spam_units::bump_spam_generation,
    validation::Errors,
    AppState,
};

const MODEL_LINK: &str = "App\\Models\\SpamKelembagaanShareLink";
const MODEL_SUBMISSION: &str = "App\\Models\\SpamKelembagaanSubmission";
const MODEL_PENGELOLA: &str = "App\\Models\\Pengelola";
/// `UnitSpam::$fillable` yang diisi usulan (`SpamKelembagaanShareService::UNIT_FIELDS`).
const UNIT_FIELDS: &[&str] = &[
    "name",
    "tahun_pembangunan",
    "sumber_dana",
    "program",
    "sistem_layanan",
    "sumber_mata_air_kap",
    "sumber_air_tanah_kap",
    "lain_lain_kap",
    "tarif_dasar_hukum",
    "iuran_nominal",
    "pendapatan_bulan",
    "biaya_operasional",
];
/// Field pengelola POKMAS (`SpamKelembagaanShareService::PENGELOLA_FIELDS`).
const PENGELOLA_FIELDS: &[&str] = &["pokmas", "perdes", "kepala", "bendahara", "sekretaris"];
const STATUS_PENDING: &str = "pending";
const STATUS_APPROVED: &str = "approved";
const STATUS_REJECTED: &str = "rejected";
const DEFAULT_PER_PAGE: i64 = 20;
/// Nilai `per_page` yang falsy (0) atau tidak valid dipakai `paginate()` sebagai default model.
const FALLBACK_PER_PAGE: i64 = 15;
const FORM_PATH_PREFIX: &str = "/kelembagaan-spam/form/";
const ALREADY_PROCESSED: &str = "Usulan ini sudah diproses sebelumnya.";

const SELECT_LINK: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(unit_spam_id AS SIGNED) AS unit_spam_id, \
     CAST(created_by AS SIGNED) AS created_by, token, label, CAST(is_active AS SIGNED) AS is_active, expires_at, \
     CAST(max_submissions AS SIGNED) AS max_submissions, CAST(submission_count AS SIGNED) AS submission_count, \
     admin_note, created_at FROM spam_kelembagaan_share_links";

const SELECT_SUBMISSION: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(share_link_id AS SIGNED) AS share_link_id, \
     CAST(unit_spam_id AS SIGNED) AS unit_spam_id, CAST(payload AS CHAR) AS payload, \
     CAST(snapshot_before AS CHAR) AS snapshot_before, submitter_name, submitter_phone, submitter_instansi, \
     submitter_note, status, CAST(reviewed_by AS SIGNED) AS reviewed_by, reviewed_at, review_note, created_at \
     FROM spam_kelembagaan_submissions";

/// Baris `spam_kelembagaan_share_links`.
#[derive(Debug, Clone)]
struct Link {
    id: i64,
    unit_spam_id: i64,
    created_by: Option<i64>,
    token: String,
    label: Option<String>,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
    max_submissions: Option<i64>,
    submission_count: i64,
    admin_note: Option<String>,
    created_at: Option<DateTime<Utc>>,
}

/// Baris `spam_kelembagaan_submissions`. `payload` dan `snapshot_before` berupa teks JSON.
#[derive(Debug, Clone)]
struct Submission {
    id: i64,
    share_link_id: i64,
    unit_spam_id: i64,
    payload: Option<String>,
    snapshot_before: Option<String>,
    submitter_name: Option<String>,
    submitter_phone: Option<String>,
    submitter_instansi: Option<String>,
    submitter_note: Option<String>,
    status: String,
    reviewed_by: Option<i64>,
    reviewed_at: Option<DateTime<Utc>>,
    review_note: Option<String>,
    created_at: Option<DateTime<Utc>>,
}

fn map_link(r: &MySqlRow) -> Result<Link, sqlx::Error> {
    Ok(Link {
        id: r.try_get("id")?,
        unit_spam_id: r.try_get("unit_spam_id")?,
        created_by: r.try_get("created_by")?,
        token: r.try_get("token")?,
        label: r.try_get("label")?,
        is_active: r.try_get::<i64, _>("is_active")? != 0,
        expires_at: r.try_get("expires_at")?,
        max_submissions: r.try_get("max_submissions")?,
        submission_count: r.try_get("submission_count")?,
        admin_note: r.try_get("admin_note")?,
        created_at: r.try_get("created_at")?,
    })
}

fn map_submission(r: &MySqlRow) -> Result<Submission, sqlx::Error> {
    Ok(Submission {
        id: r.try_get("id")?,
        share_link_id: r.try_get("share_link_id")?,
        unit_spam_id: r.try_get("unit_spam_id")?,
        payload: r.try_get("payload")?,
        snapshot_before: r.try_get("snapshot_before")?,
        submitter_name: r.try_get("submitter_name")?,
        submitter_phone: r.try_get("submitter_phone")?,
        submitter_instansi: r.try_get("submitter_instansi")?,
        submitter_note: r.try_get("submitter_note")?,
        status: r.try_get("status")?,
        reviewed_by: r.try_get("reviewed_by")?,
        reviewed_at: r.try_get("reviewed_at")?,
        review_note: r.try_get("review_note")?,
        created_at: r.try_get("created_at")?,
    })
}

/// Bind dinamis untuk query daftar dan hitung.
enum Bind {
    I(i64),
    S(String),
}

/// Nilai kolom untuk `UPDATE` pada `PATCH` share link.
enum PatchVal {
    S(Option<String>),
    I(Option<i64>),
    T(Option<DateTime<Utc>>),
}

/// `page` dan `per_page` seperti `Paginator` Laravel.
struct ListParams {
    page: i64,
    per: i64,
}

impl ListParams {
    fn offset(&self) -> i64 {
        (self.page - 1).saturating_mul(self.per)
    }
}

// ---------------------------------------------------------------------------
// Helper umum
// ---------------------------------------------------------------------------

/// `filter_var(..., FILTER_VALIDATE_INT)`: tanda opsional, tanpa nol di depan.
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

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

fn not_found_model(model: &str, raw_id: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        format!("No query results for model [{}] {}", model, raw_id),
    )
}

/// Usulan sudah diproses: 422 dengan pesan pada `status` (`ValidationException::withMessages`).
fn already_processed() -> ApiError {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    map.insert("status".to_string(), vec![ALREADY_PROCESSED.to_string()]);
    ApiError::validation(ALREADY_PROCESSED, map)
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Objek `payload` dari kolom JSON. Bukan objek dianggap kosong (`Arr::only` tidak menemukan kunci).
fn payload_map(raw: Option<&str>) -> Map<String, Value> {
    match raw.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Kolom JSON sebagai nilai; kosong atau tidak valid menjadi `null`.
fn json_column(raw: &Option<String>) -> Value {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null)
}

/// Nilai JSON sebagai teks kolom MySQL. `null` menjadi NULL; bool menjadi "1"/"0" seperti PHP.
fn db_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "1".to_string() } else { "0".to_string() }),
        other => Some(other.to_string()),
    }
}

/// Parameter daftar: `page` tidak valid atau < 1 menjadi 1; `per_page` memakai `(int)` seperti `integer()`.
fn list_params(query: &HashMap<String, String>) -> ListParams {
    let page = query
        .get("page")
        .and_then(|v| strict_int(v.trim()))
        .filter(|p| *p >= 1)
        .unwrap_or(1);
    let per = match query.get("per_page") {
        None => DEFAULT_PER_PAGE,
        Some(v) => {
            let n = si::php_int(v.trim());
            if n <= 0 {
                FALLBACK_PER_PAGE
            } else {
                n
            }
        }
    };
    ListParams { page, per }
}

/// Meta paginator Laravel (`current_page`, `last_page`, `per_page`, `total`).
fn page_meta(p: &ListParams, total: i64) -> Map<String, Value> {
    let last = if total <= 0 {
        1
    } else {
        (total - 1) / p.per + 1
    };
    let mut m = Map::new();
    m.insert("current_page".into(), json!(p.page));
    m.insert("last_page".into(), json!(last));
    m.insert("per_page".into(), json!(p.per));
    m.insert("total".into(), json!(total));
    m
}

/// `$request->filled($key)`: ada dan tidak kosong setelah trim.
fn filled<'a>(query: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    query.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

async fn count_where(
    pool: &MySqlPool,
    table: &str,
    where_sql: &str,
    binds: &[Bind],
) -> Result<i64, ApiError> {
    let sql = format!("SELECT COUNT(*) FROM {} WHERE {}", table, where_sql);
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = match b {
            Bind::I(v) => q.bind(*v),
            Bind::S(s) => q.bind(s.clone()),
        };
    }
    let row = q.fetch_one(pool).await.map_err(internal)?;
    row.try_get::<i64, _>(0).map_err(internal)
}

async fn exists_by_id(pool: &MySqlPool, sql: &str, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// Token share link: 48 karakter alfanumerik, huruf kecil (`Str::lower(Str::random(48))`).
fn random_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}

/// Tanggal seperti `strtotime` untuk format umum. Tanpa zona dianggap UTC.
fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(d) = DateTime::parse_from_rfc3339(raw) {
        return Some(d.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    let d = NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
    Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?))
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

/// `nullable|string|max:N`. Teks di-trim, kosong menjadi tidak ada.
fn opt_string(e: &mut Errors, field: &str, v: Option<&Value>, max: usize) -> Option<String> {
    let s = match v {
        None | Some(Value::Null) => return None,
        Some(Value::String(s)) => s.trim().to_string(),
        Some(_) => {
            e.add(field, format!("The {} field must be a string.", attr(field)));
            return None;
        }
    };
    if s.is_empty() {
        return None;
    }
    if s.chars().count() > max {
        e.add(
            field,
            format!(
                "The {} field must not be greater than {} characters.",
                attr(field),
                max
            ),
        );
        return None;
    }
    Some(s)
}

/// `nullable|integer|min:1|max:1000` untuk `max_submissions`.
fn opt_max(e: &mut Errors, v: Option<&Value>) -> Option<i64> {
    let parsed: Option<i64> = match v {
        None | Some(Value::Null) => return None,
        Some(Value::String(s)) if s.trim().is_empty() => return None,
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => strict_int(s.trim()),
        Some(_) => None,
    };
    match parsed {
        None => {
            e.add("max_submissions", "The max submissions field must be an integer.");
            None
        }
        Some(n) if n < 1 => {
            e.add("max_submissions", "The max submissions field must be at least 1.");
            None
        }
        Some(n) if n > 1000 => {
            e.add(
                "max_submissions",
                "The max submissions field must not be greater than 1000.",
            );
            None
        }
        Some(n) => Some(n),
    }
}

/// `nullable|date` dan, bila `must_be_after` diisi, `after:now`.
fn expires_value(
    e: &mut Errors,
    v: Option<&Value>,
    must_be_after: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let raw = match v {
        None | Some(Value::Null) => return None,
        Some(Value::String(s)) if s.trim().is_empty() => return None,
        Some(Value::String(s)) => s.trim().to_string(),
        Some(_) => {
            e.add("expires_at", "The expires at field must be a valid date.");
            return None;
        }
    };
    let Some(dt) = parse_date(&raw) else {
        e.add("expires_at", "The expires at field must be a valid date.");
        return None;
    };
    if let Some(now) = must_be_after {
        if dt <= now {
            e.add("expires_at", "The expires at field must be a date after now.");
            return None;
        }
    }
    Some(dt)
}

/// `nullable|boolean`. Yang diterima: true, false, 1, 0, "1", "0". `null` menjadi false (`$request->boolean`).
fn bool_rule(e: &mut Errors, v: Option<&Value>) -> Option<bool> {
    match v {
        None => None,
        Some(Value::Null) => Some(false),
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Number(n)) if n.as_i64() == Some(1) => Some(true),
        Some(Value::Number(n)) if n.as_i64() == Some(0) => Some(false),
        Some(Value::String(s)) if s.as_str() == "1" => Some(true),
        Some(Value::String(s)) if s.as_str() == "0" => Some(false),
        Some(_) => {
            e.add("is_active", "The is active field must be true or false.");
            None
        }
    }
}

/// `required|integer|exists:tbl_unit_spam,id`. Mengembalikan id bila valid.
async fn unit_spam_rule(
    pool: &MySqlPool,
    e: &mut Errors,
    v: Option<&Value>,
) -> Result<Option<i64>, ApiError> {
    let raw: Option<String> = match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(other) => Some(other.to_string()),
    };
    let Some(raw) = raw else {
        e.add("unit_spam_id", "The unit spam id field is required.");
        return Ok(None);
    };
    let id = strict_int(&raw);
    let exists = match id {
        Some(i) => exists_by_id(pool, "SELECT COUNT(*) FROM tbl_unit_spam WHERE id = ?", i).await?,
        None => false,
    };
    if !exists {
        e.add("unit_spam_id", "The selected unit spam id is invalid.");
        return Ok(None);
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Pencarian dan serialisasi
// ---------------------------------------------------------------------------

async fn find_link(pool: &MySqlPool, id: i64) -> Result<Option<Link>, ApiError> {
    let sql = format!("{} WHERE id = ?", SELECT_LINK);
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(map_link).transpose().map_err(internal)
}

async fn find_submission(pool: &MySqlPool, id: i64) -> Result<Option<Submission>, ApiError> {
    let sql = format!("{} WHERE id = ?", SELECT_SUBMISSION);
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(map_submission).transpose().map_err(internal)
}

/// Route model binding `SpamKelembagaanShareLink`: tidak ada berarti 404 dengan pesan Laravel.
async fn bound_link(pool: &MySqlPool, raw: &str) -> Result<Link, ApiError> {
    let id = raw
        .parse::<i64>()
        .map_err(|_| not_found_model(MODEL_LINK, raw))?;
    find_link(pool, id)
        .await?
        .ok_or_else(|| not_found_model(MODEL_LINK, raw))
}

/// Route model binding `SpamKelembagaanSubmission`.
async fn bound_submission(pool: &MySqlPool, raw: &str) -> Result<Submission, ApiError> {
    let id = raw
        .parse::<i64>()
        .map_err(|_| not_found_model(MODEL_SUBMISSION, raw))?;
    find_submission(pool, id)
        .await?
        .ok_or_else(|| not_found_model(MODEL_SUBMISSION, raw))
}

/// `UnitSpam` dengan `desa` dan `kecamatan` (`n_desa`, `n_kec`). `null` bila unit tidak ada.
async fn unit_json(pool: &MySqlPool, unit_id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(u.id AS SIGNED) AS id, u.name AS name, d.n_desa AS n_desa, k.n_kec AS n_kec \
         FROM tbl_unit_spam u \
         LEFT JOIN tbl_desa d ON d.id = u.desa_id \
         LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id \
         WHERE u.id = ?",
    )
    .bind(unit_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let id: i64 = r.try_get("id").map_err(internal)?;
    let name: Option<String> = r.try_get("name").map_err(internal)?;
    let desa: Option<String> = r.try_get("n_desa").map_err(internal)?;
    let kecamatan: Option<String> = r.try_get("n_kec").map_err(internal)?;
    Ok(json!({
        "id": id,
        "name": name,
        "desa": desa,
        "kecamatan": kecamatan,
    }))
}

/// `{id, name}` user, atau `null` bila id kosong atau user tidak ada.
async fn user_brief(pool: &MySqlPool, user_id: Option<i64>) -> Result<Value, ApiError> {
    let Some(uid) = user_id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query("SELECT CAST(id AS SIGNED) AS id, name FROM users WHERE id = ?")
        .bind(uid)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let id: i64 = r.try_get("id").map_err(internal)?;
    let name: Option<String> = r.try_get("name").map_err(internal)?;
    Ok(json!({ "id": id, "name": name }))
}

/// `{id, token, label}` share link, atau `null`.
async fn share_link_brief(pool: &MySqlPool, link_id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query("SELECT CAST(id AS SIGNED) AS id, token, label FROM spam_kelembagaan_share_links WHERE id = ?")
        .bind(link_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let id: i64 = r.try_get("id").map_err(internal)?;
    let token: String = r.try_get("token").map_err(internal)?;
    let label: Option<String> = r.try_get("label").map_err(internal)?;
    Ok(json!({ "id": id, "token": token, "label": label }))
}

/// `SpamKelembagaanShareLink::isUsable()`.
fn is_usable(link: &Link, now: DateTime<Utc>) -> bool {
    if !link.is_active {
        return false;
    }
    if let Some(exp) = link.expires_at {
        if exp < now {
            return false;
        }
    }
    if let Some(max) = link.max_submissions {
        if link.submission_count >= max {
            return false;
        }
    }
    true
}

/// `serializeLink`.
async fn link_json(pool: &MySqlPool, link: &Link) -> Result<Value, ApiError> {
    let unit = unit_json(pool, link.unit_spam_id).await?;
    let creator = user_brief(pool, link.created_by).await?;
    Ok(json!({
        "id": link.id,
        "token": link.token,
        "label": link.label,
        "is_active": link.is_active,
        "is_usable": is_usable(link, Utc::now()),
        "expires_at": iso8601_utc(link.expires_at),
        "max_submissions": link.max_submissions,
        "submission_count": link.submission_count,
        "admin_note": link.admin_note,
        "path": format!("{}{}", FORM_PATH_PREFIX, link.token),
        "created_at": iso8601_utc(link.created_at),
        "unit_spam_id": link.unit_spam_id,
        "unit": unit,
        "creator": creator,
    }))
}

/// `serializeSubmission`.
async fn submission_json(pool: &MySqlPool, s: &Submission) -> Result<Value, ApiError> {
    let unit = unit_json(pool, s.unit_spam_id).await?;
    let reviewer = user_brief(pool, s.reviewed_by).await?;
    let share_link = share_link_brief(pool, s.share_link_id).await?;
    Ok(json!({
        "id": s.id,
        "share_link_id": s.share_link_id,
        "unit_spam_id": s.unit_spam_id,
        "payload": json_column(&s.payload),
        "snapshot_before": json_column(&s.snapshot_before),
        "submitter_name": s.submitter_name,
        "submitter_phone": s.submitter_phone,
        "submitter_instansi": s.submitter_instansi,
        "submitter_note": s.submitter_note,
        "status": s.status,
        "review_note": s.review_note,
        "reviewed_at": iso8601_utc(s.reviewed_at),
        "reviewer": reviewer,
        "created_at": iso8601_utc(s.created_at),
        "unit": unit,
        "share_link": share_link,
    }))
}

async fn submission_json_by_id(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    let sub = find_submission(pool, id)
        .await?
        .ok_or_else(|| not_found_model(MODEL_SUBMISSION, &id.to_string()))?;
    submission_json(pool, &sub).await
}

// ---------------------------------------------------------------------------
// Share link
// ---------------------------------------------------------------------------

/// `GET /api/spam-kelembagaan/share-links`.
pub async fn index_links(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut where_parts: Vec<&str> = Vec::new();
    let mut binds: Vec<Bind> = Vec::new();
    if let Some(u) = filled(&query, "unit_spam_id") {
        where_parts.push("unit_spam_id = ?");
        binds.push(Bind::I(si::php_int(u)));
    }
    if let Some(a) = filled(&query, "is_active") {
        // `FILTER_VALIDATE_BOOLEAN`: hanya 1, true, on, yes yang bernilai benar.
        let on = matches!(a.to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes");
        where_parts.push("is_active = ?");
        binds.push(Bind::I(if on { 1 } else { 0 }));
    }
    let where_sql = if where_parts.is_empty() {
        "1 = 1".to_string()
    } else {
        where_parts.join(" AND ")
    };

    let total = count_where(&state.pool, "spam_kelembagaan_share_links", &where_sql, &binds).await?;
    let p = list_params(&query);
    let sql = format!(
        "{} WHERE {} ORDER BY created_at DESC, id DESC LIMIT {} OFFSET {}",
        SELECT_LINK,
        where_sql,
        p.per,
        p.offset()
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = match b {
            Bind::I(v) => q.bind(*v),
            Bind::S(s) => q.bind(s.clone()),
        };
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let mut data: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let link = map_link(r).map_err(internal)?;
        data.push(link_json(&state.pool, &link).await?);
    }
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": Value::Object(page_meta(&p, total)),
    }))
    .into_response())
}

/// `POST /api/spam-kelembagaan/share-links`: 201.
pub async fn store_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let now = Utc::now();

    let mut e = Errors::default();
    let unit_spam_id = unit_spam_rule(&state.pool, &mut e, input.get("unit_spam_id")).await?;
    let label = opt_string(&mut e, "label", input.get("label"), 255);
    let expires_at = expires_value(&mut e, input.get("expires_at"), Some(now));
    let max_submissions = opt_max(&mut e, input.get("max_submissions"));
    let admin_note = opt_string(&mut e, "admin_note", input.get("admin_note"), 2000);
    e.finish()?;
    let unit_spam_id = unit_spam_id
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error"))?;

    let token = random_token();
    let res = sqlx::query(
        "INSERT INTO spam_kelembagaan_share_links (unit_spam_id, created_by, token, label, is_active, \
         expires_at, max_submissions, submission_count, admin_note, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1, ?, ?, 0, ?, ?, ?)",
    )
    .bind(unit_spam_id)
    .bind(user.user_id as i64)
    .bind(token)
    .bind(label)
    .bind(expires_at)
    .bind(max_submissions)
    .bind(admin_note)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    let link = find_link(&state.pool, id)
        .await?
        .ok_or_else(|| not_found_model(MODEL_LINK, &id.to_string()))?;
    let data = link_json(&state.pool, &link).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": "Link form berhasil dibuat.",
            "data": data,
        })),
    )
        .into_response())
}

/// `PUT /api/spam-kelembagaan/share-links/{id}`. Hanya field yang dikirim yang diubah.
pub async fn update_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let link = bound_link(&state.pool, &id).await?;
    let input = parse_body(&body);

    let mut e = Errors::default();
    // Urutan validasi mengikuti Laravel: label, is_active, expires_at, max_submissions, admin_note.
    let label = if input.contains_key("label") {
        Some(opt_string(&mut e, "label", input.get("label"), 255))
    } else {
        None
    };
    let is_active = if input.contains_key("is_active") {
        bool_rule(&mut e, input.get("is_active"))
    } else {
        None
    };
    let expires_at = if input.contains_key("expires_at") {
        Some(expires_value(&mut e, input.get("expires_at"), None))
    } else {
        None
    };
    let max_submissions = if input.contains_key("max_submissions") {
        Some(opt_max(&mut e, input.get("max_submissions")))
    } else {
        None
    };
    let admin_note = if input.contains_key("admin_note") {
        Some(opt_string(&mut e, "admin_note", input.get("admin_note"), 2000))
    } else {
        None
    };
    e.finish()?;

    // `save()` hanya menulis kolom yang berubah (dirty).
    let mut sets: Vec<&str> = Vec::new();
    let mut vals: Vec<PatchVal> = Vec::new();
    if let Some(v) = label {
        if v != link.label {
            sets.push("label = ?");
            vals.push(PatchVal::S(v));
        }
    }
    if let Some(v) = is_active {
        if v != link.is_active {
            sets.push("is_active = ?");
            vals.push(PatchVal::I(Some(if v { 1 } else { 0 })));
        }
    }
    if let Some(v) = expires_at {
        if v != link.expires_at {
            sets.push("expires_at = ?");
            vals.push(PatchVal::T(v));
        }
    }
    if let Some(v) = max_submissions {
        if v != link.max_submissions {
            sets.push("max_submissions = ?");
            vals.push(PatchVal::I(v));
        }
    }
    if let Some(v) = admin_note {
        if v != link.admin_note {
            sets.push("admin_note = ?");
            vals.push(PatchVal::S(v));
        }
    }

    if !sets.is_empty() {
        let sql = format!(
            "UPDATE spam_kelembagaan_share_links SET {}, updated_at = ? WHERE id = ?",
            sets.join(", ")
        );
        let mut q = sqlx::query(&sql);
        for v in vals {
            q = match v {
                PatchVal::S(x) => q.bind(x),
                PatchVal::I(x) => q.bind(x),
                PatchVal::T(x) => q.bind(x),
            };
        }
        q.bind(Utc::now())
            .bind(link.id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    }

    let fresh = find_link(&state.pool, link.id)
        .await?
        .ok_or_else(|| not_found_model(MODEL_LINK, &id))?;
    let data = link_json(&state.pool, &fresh).await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `DELETE /api/spam-kelembagaan/share-links/{id}`: nonaktifkan, tidak menghapus baris.
pub async fn destroy_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let link = bound_link(&state.pool, &id).await?;
    if link.is_active {
        sqlx::query("UPDATE spam_kelembagaan_share_links SET is_active = 0, updated_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(link.id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    }
    Ok(Json(json!({
        "success": true,
        "message": "Link form dinonaktifkan.",
    }))
    .into_response())
}

// ---------------------------------------------------------------------------
// Usulan
// ---------------------------------------------------------------------------

/// `GET /api/spam-kelembagaan/submissions`. `meta.pending_count` menghitung semua usulan pending.
pub async fn index_submissions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut where_parts: Vec<&str> = Vec::new();
    let mut binds: Vec<Bind> = Vec::new();
    if let Some(s) = filled(&query, "status") {
        where_parts.push("status = ?");
        binds.push(Bind::S(s.to_string()));
    }
    if let Some(u) = filled(&query, "unit_spam_id") {
        where_parts.push("unit_spam_id = ?");
        binds.push(Bind::I(si::php_int(u)));
    }
    let where_sql = if where_parts.is_empty() {
        "1 = 1".to_string()
    } else {
        where_parts.join(" AND ")
    };

    let total = count_where(&state.pool, "spam_kelembagaan_submissions", &where_sql, &binds).await?;
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM spam_kelembagaan_submissions WHERE status = ?")
        .bind(STATUS_PENDING)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;

    let p = list_params(&query);
    let sql = format!(
        "{} WHERE {} ORDER BY created_at DESC, id DESC LIMIT {} OFFSET {}",
        SELECT_SUBMISSION,
        where_sql,
        p.per,
        p.offset()
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = match b {
            Bind::I(v) => q.bind(*v),
            Bind::S(s) => q.bind(s.clone()),
        };
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let mut data: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let sub = map_submission(r).map_err(internal)?;
        data.push(submission_json(&state.pool, &sub).await?);
    }
    let mut meta = page_meta(&p, total);
    meta.insert("pending_count".into(), json!(pending));
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": Value::Object(meta),
    }))
    .into_response())
}

/// `GET /api/spam-kelembagaan/submissions/{id}`.
pub async fn show_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let sub = bound_submission(&state.pool, &id).await?;
    let data = submission_json(&state.pool, &sub).await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// Nilai `review_note` untuk approve dan reject: `nullable|string|max:2000`.
fn review_note(e: &mut Errors, input: &Map<String, Value>) -> Option<String> {
    opt_string(e, "review_note", input.get("review_note"), 2000)
}

/// Menerapkan usulan: tandai `approved`, lalu perbarui `UnitSpam` dan `Pengelola` sesuai `payload`.
/// Model yang berubah ditulis ke audit dan notifikasi admin (`si::log_model_change`).
async fn apply_approval(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    sub: &Submission,
    note: Option<String>,
) -> Result<(), ApiError> {
    let now = Utc::now();
    let res = sqlx::query(
        "UPDATE spam_kelembagaan_submissions SET status = ?, reviewed_by = ?, reviewed_at = ?, \
         review_note = ?, updated_at = ? WHERE id = ? AND status = ?",
    )
    .bind(STATUS_APPROVED)
    .bind(actor as i64)
    .bind(now)
    .bind(note)
    .bind(now)
    .bind(sub.id)
    .bind(STATUS_PENDING)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    if res.rows_affected() == 0 {
        return Err(already_processed());
    }

    let payload = payload_map(sub.payload.as_deref());

    let unit_data: Vec<(&str, Value)> = UNIT_FIELDS
        .iter()
        .filter_map(|f| payload.get(*f).map(|v| (*f, v.clone())))
        .collect();
    if !unit_data.is_empty() {
        update_row(
            tx,
            ctx,
            actor,
            "tbl_unit_spam",
            MODEL_UNIT,
            sub.unit_spam_id,
            &unit_data,
            now,
        )
        .await?;
    }

    let peng_data: Vec<(&str, Value)> = PENGELOLA_FIELDS
        .iter()
        .filter_map(|f| payload.get(*f).map(|v| (*f, v.clone())))
        .collect();
    if !peng_data.is_empty() {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM tbl_pengelola WHERE unit_spam_id = ? ORDER BY id LIMIT 1",
        )
        .bind(sub.unit_spam_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal)?;
        match existing {
            Some(pid) => {
                update_row(
                    tx,
                    ctx,
                    actor,
                    "tbl_pengelola",
                    MODEL_PENGELOLA,
                    pid,
                    &peng_data,
                    now,
                )
                .await?
            }
            None => {
                create_pengelola(tx, ctx, actor, sub.unit_spam_id, &peng_data, now).await?
            }
        }
    }
    Ok(())
}

/// `$model->update($data)` dengan audit `updated` dan notifikasi bila ada kolom yang berbeda.
/// Perbandingan teks seperti `isDirty`. Nilai baru disimpan apa adanya (angka tetap angka di audit).
#[allow(clippy::too_many_arguments)]
async fn update_row(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    table: &str,
    model: &str,
    id: i64,
    data: &[(&str, Value)],
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let selects: Vec<String> = data
        .iter()
        .map(|(f, _)| format!("CAST({} AS CHAR) AS {}", f, f))
        .collect();
    let sql = format!(
        "SELECT {}, DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') AS updated_raw FROM {} WHERE id = ?",
        selects.join(", "),
        table
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal)?;
    let Some(row) = row else {
        return Err(not_found_model(model, &id.to_string()));
    };

    let mut dirty_old: Map<String, Value> = Map::new();
    let mut dirty_new: Map<String, Value> = Map::new();
    let mut updates: Vec<(String, Option<String>)> = Vec::new();
    for (field, value) in data {
        let current: Option<String> = row.try_get(*field).map_err(internal)?;
        let next = db_text(value);
        if current != next {
            dirty_old.insert(
                (*field).to_string(),
                current.clone().map(Value::String).unwrap_or(Value::Null),
            );
            dirty_new.insert((*field).to_string(), (*value).clone());
            updates.push(((*field).to_string(), next));
        }
    }
    if updates.is_empty() {
        return Ok(());
    }
    let raw_updated: Option<String> = row.try_get("updated_raw").map_err(internal)?;
    dirty_old.insert(
        "updated_at".to_string(),
        raw_updated.map(Value::String).unwrap_or(Value::Null),
    );
    dirty_new.insert("updated_at".to_string(), carbon_json(Some(now)));

    let sets: Vec<String> = updates
        .iter()
        .map(|(f, _)| format!("{} = ?", f))
        .collect();
    let usql = format!(
        "UPDATE {} SET {}, updated_at = ? WHERE id = ?",
        table,
        sets.join(", ")
    );
    let mut q = sqlx::query(&usql);
    for (_, v) in &updates {
        q = q.bind(v.clone());
    }
    q.bind(now)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;

    si::log_model_change(
        tx,
        ctx,
        actor,
        ModelChange {
            event: "updated",
            model,
            id: id as u64,
            old: Some(dirty_old),
            new: Some(dirty_new),
        },
    )
    .await
}

/// `$unit->pengelola()->create($data)`: audit `created` dengan `unit_spam_id` dan kolom yang dikirim.
async fn create_pengelola(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    unit_id: i64,
    data: &[(&str, Value)],
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let cols: Vec<String> = data.iter().map(|(f, _)| (*f).to_string()).collect();
    let placeholders = vec!["?"; data.len()].join(", ");
    let sql = format!(
        "INSERT INTO tbl_pengelola (unit_spam_id, {}, created_at, updated_at) VALUES (?, {}, ?, ?)",
        cols.join(", "),
        placeholders
    );
    let mut q = sqlx::query(&sql).bind(unit_id);
    for (_, v) in data {
        q = q.bind(db_text(v));
    }
    let res = q
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    let new_id = res.last_insert_id() as i64;

    let mut new_map: Map<String, Value> = Map::new();
    new_map.insert("unit_spam_id".to_string(), json!(unit_id));
    for (f, v) in data {
        new_map.insert((*f).to_string(), (*v).clone());
    }
    new_map.insert("id".to_string(), json!(new_id));
    new_map.insert("created_at".to_string(), carbon_json(Some(now)));
    new_map.insert("updated_at".to_string(), carbon_json(Some(now)));

    si::log_model_change(
        tx,
        ctx,
        actor,
        ModelChange {
            event: "created",
            model: MODEL_PENGELOLA,
            id: new_id as u64,
            old: None,
            new: Some(new_map),
        },
    )
    .await
}

/// `POST /api/spam-kelembagaan/submissions/{id}/approve`.
pub async fn approve_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let sub = bound_submission(&state.pool, &id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let note = review_note(&mut e, &input);
    e.finish()?;

    bump_spam_generation();
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let url = format!(
        "{}/api/spam-kelembagaan/submissions/{}/approve",
        base_url(&state),
        sub.id
    );
    let ctx = Ctx {
        user: Some(user.user_id),
        roles: &roles,
        url: &url,
        headers: &headers,
    };

    let mut tx = state.pool.begin().await.map_err(internal)?;
    apply_approval(&mut tx, &ctx, user.user_id, &sub, note).await?;
    tx.commit().await.map_err(internal)?;

    let data = submission_json_by_id(&state.pool, sub.id).await?;
    Ok(Json(json!({
        "success": true,
        "message": "Usulan disetujui dan data unit SPAM diperbarui.",
        "data": data,
    }))
    .into_response())
}

/// `POST /api/spam-kelembagaan/submissions/{id}/reject`. Tanpa audit dan notifikasi, sama dengan Laravel.
pub async fn reject_submission(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let sub = bound_submission(&state.pool, &id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let note = review_note(&mut e, &input);
    e.finish()?;

    let now = Utc::now();
    let res = sqlx::query(
        "UPDATE spam_kelembagaan_submissions SET status = ?, reviewed_by = ?, reviewed_at = ?, \
         review_note = ?, updated_at = ? WHERE id = ? AND status = ?",
    )
    .bind(STATUS_REJECTED)
    .bind(user.user_id as i64)
    .bind(now)
    .bind(note)
    .bind(now)
    .bind(sub.id)
    .bind(STATUS_PENDING)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    if res.rows_affected() == 0 {
        return Err(already_processed());
    }

    let data = submission_json_by_id(&state.pool, sub.id).await?;
    Ok(Json(json!({
        "success": true,
        "message": "Usulan ditolak.",
        "data": data,
    }))
    .into_response())
}
