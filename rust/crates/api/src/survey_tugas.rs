//! Tugas survei (`SurveyTugasController`): tabel `tbl_survey_tugas`, pivot `tbl_survey_tugas_assignees`,
//! model `App\Models\SurveyTugas` (`Auditable`).
//!
//! Rute:
//! - `GET /api/survey-tugas`: daftar 15 per halaman (`per_page` 1..=100). Non-admin hanya melihat tugas
//!   yang ia tangani (`assignee_id` atau pivot).
//! - `GET /api/survey-tugas/{id}`: non-admin yang bukan penanggung jawab mendapat 403 `Forbidden`.
//! - `POST`, `PUT`, `PATCH`, `DELETE`: hanya admin (`role:admin`, 403 `User does not have the right roles.`).
//!
//! Urutan pemeriksaan sama dengan Laravel: `{id}` dicari dulu (404), lalu login (401), lalu role admin (403),
//! lalu validasi (422). `CheckRoutePermission` dijalankan lebih dulu oleh `route_permission::check`.
//!
//! Perbedaan dengan Laravel:
//! - Validasi hanya mencatat satu pesan per field. Laravel bisa mencatat lebih dari satu.
//! - `batas_waktu` menerima `YYYY-MM-DD`, datetime umum, dan RFC 3339. Format lain ditolak.
//! - `assignees` diurutkan menurut `tbl_survey_tugas_assignees.id` (Laravel tidak mengurutkan).
//! - Daftar diurutkan `created_at DESC, id DESC` (Laravel hanya `created_at`).
//! - `store`, `update`, dan `destroy` memakai transaksi.
//! - `old_values` dan `new_values` audit diambil dari baris DB, jadi tipe kecil (mis. angka vs string) bisa berbeda.
//! - 404 karena `{id}` tidak ada memakai `Not Found.` seperti modul Rust lain. Laravel mengirim pesan
//!   `No query results for model [...]` (perlu dicek terhadap produksi).
//! - `surveys_count` dan `sudah_disurvey` dihitung di SQL (subquery `COUNT`).

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    changes,
    format::iso8601_utc,
    media::internal,
    pagination::{self, PageParams},
    require_auth,
    survey_lokasi::audit_update,
    AppState,
};

const TABLE: &str = "tbl_survey_tugas";
const MODEL: &str = "App\\Models\\SurveyTugas";
const MAX_PER_PAGE: i64 = 100;
const MAX_ASSIGNEES: usize = 20;
const ROLE_FORBIDDEN: &str = "User does not have the right roles.";
const ROLE_ASSIGNEE: &str =
    "Penanggung jawab harus memiliki salah satu role: admin, tfl, pengawas, konsultan_pengawas, operator.";
/// `SURVEY_ROLE_NAMES` di `SurveyTugasController`.
const SURVEY_ROLES: &[&str] = &["admin", "tfl", "pengawas", "konsultan_pengawas", "operator"];
const JENIS: &[&str] = &["spam_perpipaan", "spam_pengeboran", "mck_individu", "mck_komunal"];
const STATUS: &[&str] = &["ditugaskan", "dikerjakan", "selesai"];
/// Kolom dalam `$request->only([...])` pada store (`status` diatur terpisah).
const STORE_FIELDS: &[&str] = &[
    "pekerjaan_id",
    "judul",
    "tahun_anggaran",
    "jenis",
    "kecamatan_id",
    "desa_id",
    "lokasi_catatan",
    "assignee_id",
    "batas_waktu",
    "catatan_admin",
];
/// Kolom dalam `$request->only([...])` pada update.
const UPDATE_FIELDS: &[&str] = &[
    "pekerjaan_id",
    "judul",
    "tahun_anggaran",
    "jenis",
    "kecamatan_id",
    "desa_id",
    "lokasi_catatan",
    "assignee_id",
    "status",
    "batas_waktu",
    "catatan_admin",
];

/// Bentuk `SurveyTugasResource` pada satu respons.
/// `creator` hanya ada bila dimuat (show, store, update). `surveys_count` hanya ada bila `withCount`
/// (index, store, update). `sudah_disurvey` selalu ada.
#[derive(Clone, Copy)]
struct View {
    creator: bool,
    with_count: bool,
}

const INDEX_VIEW: View = View {
    creator: false,
    with_count: true,
};
const SHOW_VIEW: View = View {
    creator: true,
    with_count: false,
};
const WRITE_VIEW: View = View {
    creator: true,
    with_count: true,
};

// ---------------------------------------------------------------------------
// Helper umum
// ---------------------------------------------------------------------------

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>().map_err(|_| ApiError::not_found())
}

fn app_base(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

fn forbidden(message: &str) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, message)
}

/// `TrimStrings` dan `ConvertEmptyStringsToNull`, rekursif seperti middleware Laravel.
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
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

/// Body JSON sebagai objek yang sudah dinormalisasi. Selain objek dianggap kosong.
fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m.into_iter().map(|(k, v)| (k, normalize(v))).collect(),
        _ => Map::new(),
    }
}

/// Bilangan bulat seperti `FILTER_VALIDATE_INT`: angka JSON bulat atau string bilangan.
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_int(input: &Map<String, Value>, key: &str) -> Option<i64> {
    input.get(key).and_then(as_int)
}

fn opt_text(input: &Map<String, Value>, key: &str) -> Option<String> {
    input.get(key).and_then(text_of)
}

/// Tanggal seperti aturan `date` dan cast `date` Laravel: hasilnya selalu `YYYY-MM-DD`.
fn parse_date(s: &str) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d);
    }
    for f in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
            return Some(dt.date());
        }
    }
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.date_naive())
}

fn opt_date(input: &Map<String, Value>, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .and_then(parse_date)
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// `$request->filled(key)` untuk query string: sudah di-trim, dan kosong berarti tidak diisi.
fn filled_q(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
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

fn dedupe(ids: impl Iterator<Item = i64>) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Ambil kunci tertentu dari atribut penuh (untuk `new_values` audit `created`).
fn pick(full: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    full.iter()
        .filter(|(k, _)| keys.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Nama atribut untuk pesan validasi: underscore menjadi spasi, seperti Laravel (`pekerjaan id`).
fn attr(field: &str) -> String {
    field.replace('_', " ")
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Errs(BTreeMap<String, Vec<String>>);

impl Errs {
    fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0
            .entry(field.to_string())
            .or_default()
            .push(message.into());
    }

    fn finish(self) -> Result<(), ApiError> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(ApiError::validation("Validation error", self.0))
    }
}

/// `exists:tabel,id`. Nilai yang bukan bilangan bulat dianggap tidak ada.
/// `table` selalu konstanta dari kode, bukan input pengguna.
async fn exists(pool: &MySqlPool, table: &str, v: &Value) -> Result<bool, ApiError> {
    let Some(id) = as_int(v) else {
        return Ok(false);
    };
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// Aturan `store` (`required`) dan `update` (`sometimes|required`). Pesan memakai bahasa Inggris Laravel.
async fn validate(
    pool: &MySqlPool,
    input: &Map<String, Value>,
    update: bool,
) -> Result<(), ApiError> {
    let mut e = Errs::default();

    // judul: required|string|max:255
    match input.get("judul") {
        None if update => {}
        None | Some(Value::Null) => e.add("judul", "The judul field is required."),
        Some(Value::String(s)) if s.chars().count() > 255 => e.add(
            "judul",
            "The judul field must not be greater than 255 characters.",
        ),
        Some(Value::String(_)) => {}
        Some(_) => e.add("judul", "The judul field must be a string."),
    }

    // tahun_anggaran: required|integer|min:2020|max:2100
    match input.get("tahun_anggaran") {
        None if update => {}
        None | Some(Value::Null) => e.add("tahun_anggaran", "The tahun anggaran field is required."),
        Some(v) => match as_int(v) {
            None => e.add(
                "tahun_anggaran",
                "The tahun anggaran field must be an integer.",
            ),
            Some(n) if n < 2020 => e.add(
                "tahun_anggaran",
                "The tahun anggaran field must be at least 2020.",
            ),
            Some(n) if n > 2100 => e.add(
                "tahun_anggaran",
                "The tahun anggaran field must not be greater than 2100.",
            ),
            Some(_) => {}
        },
    }

    // pekerjaan_id, kecamatan_id, desa_id: nullable|exists
    for (field, table) in [
        ("pekerjaan_id", "tbl_pekerjaan"),
        ("kecamatan_id", "tbl_kecamatan"),
        ("desa_id", "tbl_desa"),
    ] {
        if let Some(v) = input.get(field).filter(|v| !v.is_null()) {
            if !exists(pool, table, v).await? {
                e.add(field, format!("The selected {} is invalid.", attr(field)));
            }
        }
    }

    // jenis, status: nullable|in:...
    if let Some(v) = input.get("jenis").filter(|v| !v.is_null()) {
        if !v.as_str().is_some_and(|s| JENIS.contains(&s)) {
            e.add("jenis", "The selected jenis is invalid.");
        }
    }
    if let Some(v) = input.get("status").filter(|v| !v.is_null()) {
        if !v.as_str().is_some_and(|s| STATUS.contains(&s)) {
            e.add("status", "The selected status is invalid.");
        }
    }

    // lokasi_catatan, catatan_admin: nullable|string
    for (field, message) in [
        ("lokasi_catatan", "The lokasi catatan must be a string."),
        ("catatan_admin", "The catatan admin must be a string."),
    ] {
        if let Some(v) = input.get(field).filter(|v| !v.is_null()) {
            if !v.is_string() {
                e.add(field, message);
            }
        }
    }

    // batas_waktu: nullable|date
    if let Some(v) = input.get("batas_waktu").filter(|v| !v.is_null()) {
        if v.as_str().and_then(parse_date).is_none() {
            e.add(
                "batas_waktu",
                "The batas waktu field must be a valid date.",
            );
        }
    }

    // assignee_id: required|exists:users,id (update: sometimes)
    match input.get("assignee_id") {
        None if update => {}
        None | Some(Value::Null) => e.add("assignee_id", "The assignee id field is required."),
        Some(v) => {
            if !exists(pool, "users", v).await? {
                e.add("assignee_id", "The selected assignee id is invalid.");
            }
        }
    }

    // assignee_ids: nullable|array|min:1|max:20, assignee_ids.*: exists:users,id
    if let Some(v) = input.get("assignee_ids").filter(|v| !v.is_null()) {
        match v {
            Value::Array(items) => {
                if items.is_empty() {
                    e.add(
                        "assignee_ids",
                        "The assignee ids field must have at least 1 items.",
                    );
                }
                if items.len() > MAX_ASSIGNEES {
                    e.add(
                        "assignee_ids",
                        "The assignee ids field must not have more than 20 items.",
                    );
                }
                for (i, item) in items.iter().enumerate() {
                    if !exists(pool, "users", item).await? {
                        let field = format!("assignee_ids.{i}");
                        let message = format!("The selected {} is invalid.", attr(&field));
                        e.add(&field, message);
                    }
                }
            }
            _ => e.add("assignee_ids", "The assignee ids field must be an array."),
        }
    }

    e.finish()
}

// ---------------------------------------------------------------------------
// Aktor, role, dan penanggung jawab
// ---------------------------------------------------------------------------

/// Nama role user. `user_id` tidak positif berarti tidak ada user.
async fn role_names(pool: &MySqlPool, user_id: i64) -> Result<Vec<String>, ApiError> {
    if user_id <= 0 {
        return Ok(Vec::new());
    }
    let roles = auth::login::roles_of(pool, user_id as u64)
        .await
        .map_err(internal)?;
    Ok(roles.into_iter().map(|(_, name)| name).collect())
}

async fn is_admin(pool: &MySqlPool, user_id: u64) -> Result<bool, ApiError> {
    Ok(role_names(pool, user_id as i64)
        .await?
        .iter()
        .any(|r| r.as_str() == "admin"))
}

/// Setara middleware `role:admin` (Spatie): 403 bila user tidak punya role `admin`.
async fn ensure_admin(pool: &MySqlPool, user_id: u64) -> Result<(), ApiError> {
    if is_admin(pool, user_id).await? {
        Ok(())
    } else {
        Err(forbidden(ROLE_FORBIDDEN))
    }
}

/// Setara `Route Model Binding`: baris dengan id tersebut harus ada.
async fn ensure_exists(pool: &MySqlPool, id: i64) -> Result<(), ApiError> {
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {TABLE} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n == 0 {
        return Err(ApiError::not_found());
    }
    Ok(())
}

/// `isAssignee`: `assignee_id` utama atau ada di pivot.
async fn is_assignee(pool: &MySqlPool, tugas_id: i64, user_id: u64) -> Result<bool, ApiError> {
    let uid = user_id as i64;
    let n: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas t WHERE t.id = ? \
         AND (COALESCE(t.assignee_id, 0) = ? OR EXISTS (SELECT 1 FROM tbl_survey_tugas_assignees pv \
         WHERE pv.survey_tugas_id = t.id AND pv.user_id = ?))",
    )
    .bind(tugas_id)
    .bind(uid)
    .bind(uid)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(n > 0)
}

fn assignee_role_error() -> ApiError {
    let mut map = BTreeMap::new();
    map.insert("assignee_ids".to_string(), vec![ROLE_ASSIGNEE.to_string()]);
    ApiError::validation("Validation error", map)
}

/// `resolveAssignees`: daftar penanggung jawab tanpa duplikat dan urutan asli. Semua harus punya role survei.
/// `None` bila tidak ada daftar yang bisa dipakai (sama seperti `ids = null` di Laravel).
async fn resolve_assignees(
    pool: &MySqlPool,
    input: &Map<String, Value>,
) -> Result<Option<Vec<i64>>, ApiError> {
    let raw: Vec<i64> = match input.get("assignee_ids") {
        Some(Value::Array(items)) => items.iter().map(|v| as_int(v).unwrap_or(0)).collect(),
        Some(Value::Null) | None => match opt_int(input, "assignee_id") {
            Some(id) => vec![id],
            None => return Ok(None),
        },
        Some(_) => return Ok(None),
    };
    let ids = dedupe(raw.into_iter());
    if ids.is_empty() {
        return Ok(None);
    }
    for id in &ids {
        let roles = role_names(pool, *id).await?;
        if !roles.iter().any(|r| SURVEY_ROLES.contains(&r.as_str())) {
            return Err(assignee_role_error());
        }
    }
    Ok(Some(ids))
}

// ---------------------------------------------------------------------------
// Baca data
// ---------------------------------------------------------------------------

/// Relasi satu baris: id dan nama (`nama_paket`, `n_kec`, `n_desa`, atau `name`).
struct Rel {
    id: i64,
    name: Option<String>,
}

struct TugasRow {
    id: i64,
    judul: String,
    tahun_anggaran: i64,
    jenis: Option<String>,
    pekerjaan: Option<Rel>,
    kecamatan: Option<Rel>,
    desa: Option<Rel>,
    assignee: Option<Rel>,
    creator: Option<Rel>,
    status: Option<String>,
    batas_waktu: Option<NaiveDate>,
    catatan_admin: Option<String>,
    surveys_count: i64,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

/// Satu baris dengan semua relasi `LEFT JOIN`. Relasi ada bila id-nya tidak NULL.
const SELECT_TUGAS: &str = "SELECT CAST(t.id AS SIGNED) AS id, t.judul, \
    CAST(t.tahun_anggaran AS SIGNED) AS tahun_anggaran, CAST(t.jenis AS CHAR) AS jenis, \
    CAST(p.id AS SIGNED) AS pekerjaan_id, p.nama_paket AS pekerjaan_nama, \
    CAST(k.id AS SIGNED) AS kecamatan_id, k.n_kec AS kecamatan_nama, \
    CAST(d.id AS SIGNED) AS desa_id, d.n_desa AS desa_nama, \
    CAST(a.id AS SIGNED) AS assignee_id, a.name AS assignee_nama, \
    CAST(c.id AS SIGNED) AS creator_id, c.name AS creator_nama, \
    CAST(t.status AS CHAR) AS status, t.batas_waktu, t.catatan_admin, \
    (SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi s WHERE s.tugas_id = t.id) AS surveys_count, \
    t.created_at, t.updated_at \
    FROM tbl_survey_tugas t \
    LEFT JOIN tbl_pekerjaan p ON p.id = t.pekerjaan_id \
    LEFT JOIN tbl_kecamatan k ON k.id = t.kecamatan_id \
    LEFT JOIN tbl_desa d ON d.id = t.desa_id \
    LEFT JOIN users a ON a.id = t.assignee_id \
    LEFT JOIN users c ON c.id = t.created_by";

fn rel(row: &MySqlRow, id_col: &str, name_col: &str) -> Result<Option<Rel>, sqlx::Error> {
    let id: Option<i64> = row.try_get(id_col)?;
    match id {
        Some(id) => Ok(Some(Rel {
            id,
            name: row.try_get(name_col)?,
        })),
        None => Ok(None),
    }
}

fn map_row(row: &MySqlRow) -> Result<TugasRow, sqlx::Error> {
    Ok(TugasRow {
        id: row.try_get("id")?,
        judul: row.try_get("judul")?,
        tahun_anggaran: row.try_get("tahun_anggaran")?,
        jenis: row.try_get("jenis")?,
        pekerjaan: rel(row, "pekerjaan_id", "pekerjaan_nama")?,
        kecamatan: rel(row, "kecamatan_id", "kecamatan_nama")?,
        desa: rel(row, "desa_id", "desa_nama")?,
        assignee: rel(row, "assignee_id", "assignee_nama")?,
        creator: rel(row, "creator_id", "creator_nama")?,
        status: row.try_get("status")?,
        batas_waktu: row.try_get("batas_waktu")?,
        catatan_admin: row.try_get("catatan_admin")?,
        surveys_count: row.try_get("surveys_count")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// `with('assignees')`: penanggung jawab per tugas, diurutkan menurut id pivot.
async fn assignees_of<'c, E>(exec: E, ids: &[i64]) -> Result<HashMap<i64, Vec<Rel>>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let mut out: HashMap<i64, Vec<Rel>> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let marks = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "SELECT CAST(a.survey_tugas_id AS SIGNED) AS tugas_id, CAST(u.id AS SIGNED) AS user_id, \
         u.name AS user_name FROM tbl_survey_tugas_assignees a JOIN users u ON u.id = a.user_id \
         WHERE a.survey_tugas_id IN ({marks}) ORDER BY a.id"
    );
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(exec).await.map_err(internal)?;
    for row in rows {
        let tugas_id: i64 = row.try_get("tugas_id").map_err(internal)?;
        let user = Rel {
            id: row.try_get("user_id").map_err(internal)?,
            name: row.try_get("user_name").map_err(internal)?,
        };
        out.entry(tugas_id).or_default().push(user);
    }
    Ok(out)
}

fn rel_obj(rel: &Option<Rel>, name_key: &str) -> Value {
    match rel {
        Some(r) => {
            let mut m = Map::new();
            m.insert("id".into(), json!(r.id));
            m.insert(name_key.into(), json!(r.name));
            Value::Object(m)
        }
        None => Value::Null,
    }
}

fn jenis_label_of(jenis: &str) -> String {
    let label: &str = match jenis {
        "spam_perpipaan" => "SPAM Perpipaan",
        "spam_pengeboran" => "SPAM Pengeboran",
        "mck_individu" => "MCK Individu",
        "mck_komunal" => "MCK Komunal",
        other => other,
    };
    label.to_string()
}

fn status_label_of(status: &str) -> String {
    let label: &str = match status {
        "ditugaskan" => "Ditugaskan",
        "dikerjakan" => "Dikerjakan",
        "selesai" => "Selesai",
        other => other,
    };
    label.to_string()
}

/// Bentuk JSON `SurveyTugasResource`.
fn tugas_json(r: &TugasRow, assignees: &[Rel], view: View) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(r.id));
    m.insert("judul".into(), json!(r.judul));
    m.insert("tahun_anggaran".into(), json!(r.tahun_anggaran));
    m.insert("jenis".into(), json!(r.jenis));
    m.insert(
        "jenis_label".into(),
        json!(r
            .jenis
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(jenis_label_of)),
    );
    m.insert("pekerjaan".into(), rel_obj(&r.pekerjaan, "nama_paket"));
    m.insert("kecamatan".into(), rel_obj(&r.kecamatan, "nama"));
    m.insert("desa".into(), rel_obj(&r.desa, "nama"));
    m.insert("assignee".into(), rel_obj(&r.assignee, "name"));
    m.insert(
        "assignees".into(),
        Value::Array(
            assignees
                .iter()
                .map(|u| json!({ "id": u.id, "name": u.name }))
                .collect(),
        ),
    );
    if view.creator {
        m.insert("creator".into(), rel_obj(&r.creator, "name"));
    }
    m.insert("status".into(), json!(r.status));
    m.insert(
        "status_label".into(),
        json!(r.status.as_deref().map(status_label_of)),
    );
    m.insert(
        "batas_waktu".into(),
        json!(r.batas_waktu.map(|d| d.format("%Y-%m-%d").to_string())),
    );
    m.insert("catatan_admin".into(), json!(r.catatan_admin));
    if view.with_count {
        m.insert("surveys_count".into(), json!(r.surveys_count));
    }
    m.insert("sudah_disurvey".into(), json!(r.surveys_count > 0));
    m.insert("created_at".into(), iso8601_utc(r.created_at));
    m.insert("updated_at".into(), iso8601_utc(r.updated_at));
    Value::Object(m)
}

/// Muat satu tugas untuk respons. `None` bila tidak ada.
async fn load_view(pool: &MySqlPool, id: i64, view: View) -> Result<Option<Value>, ApiError> {
    let sql = format!("{SELECT_TUGAS} WHERE t.id = ?");
    let Some(row) = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
    else {
        return Ok(None);
    };
    let t = map_row(&row).map_err(internal)?;
    let mut by_tugas = assignees_of(pool, &[id]).await?;
    let assignees = by_tugas.remove(&id).unwrap_or_default();
    Ok(Some(tugas_json(&t, &assignees, view)))
}

/// Atribut seperti `getAttributes()` untuk audit (kunci = kolom).
const SELECT_ATTRS: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
    judul, CAST(tahun_anggaran AS SIGNED) AS tahun_anggaran, CAST(jenis AS CHAR) AS jenis, \
    CAST(kecamatan_id AS SIGNED) AS kecamatan_id, CAST(desa_id AS SIGNED) AS desa_id, lokasi_catatan, \
    CAST(assignee_id AS SIGNED) AS assignee_id, CAST(status AS CHAR) AS status, \
    CAST(batas_waktu AS CHAR) AS batas_waktu, catatan_admin, CAST(created_by AS SIGNED) AS created_by, \
    CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at \
    FROM tbl_survey_tugas WHERE id = ?";

async fn attributes<'c, E>(exec: E, id: i64) -> Result<Map<String, Value>, ApiError>
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
        "pekerjaan_id",
        "tahun_anggaran",
        "kecamatan_id",
        "desa_id",
        "assignee_id",
        "created_by",
    ] {
        m.insert(
            col.into(),
            json!(row.try_get::<Option<i64>, _>(col).map_err(internal)?),
        );
    }
    for col in [
        "judul",
        "jenis",
        "lokasi_catatan",
        "status",
        "batas_waktu",
        "catatan_admin",
        "created_at",
        "updated_at",
    ] {
        m.insert(
            col.into(),
            json!(row.try_get::<Option<String>, _>(col).map_err(internal)?),
        );
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// Tulis data
// ---------------------------------------------------------------------------

/// Nilai untuk `UPDATE ... SET kolom = ?`.
#[derive(Clone)]
enum Bind {
    Int(Option<i64>),
    Text(Option<String>),
}

/// `pekerjaan` untuk `store`: kecamatan dan desa pekerjaan (`Pekerjaan::find`). `None` bila tidak ada.
async fn pekerjaan_location(
    pool: &MySqlPool,
    id: i64,
) -> Result<Option<(Option<i64>, Option<i64>)>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(kecamatan_id AS SIGNED) AS kecamatan_id, CAST(desa_id AS SIGNED) AS desa_id \
         FROM tbl_pekerjaan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    match row {
        Some(r) => Ok(Some((
            r.try_get("kecamatan_id").map_err(internal)?,
            r.try_get("desa_id").map_err(internal)?,
        ))),
        None => Ok(None),
    }
}

/// Menambah satu penanggung jawab ke pivot (timestamp pivot ikut diisi, seperti `withTimestamps()`).
async fn attach(tx: &mut Transaction<'_, MySql>, tugas_id: i64, uid: i64) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO tbl_survey_tugas_assignees (survey_tugas_id, user_id, created_at, updated_at) \
         VALUES (?, ?, NOW(), NOW())",
    )
    .bind(tugas_id)
    .bind(uid)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// `syncWithoutDetaching([uid])`: tambah bila belum ada.
async fn attach_missing(
    tx: &mut Transaction<'_, MySql>,
    tugas_id: i64,
    uid: i64,
) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas_assignees \
         WHERE survey_tugas_id = ? AND user_id = ?",
    )
    .bind(tugas_id)
    .bind(uid)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    if n == 0 {
        attach(tx, tugas_id, uid).await?;
    }
    Ok(())
}

/// `sync($ids)`: lepas yang tidak ada di daftar, tambah yang belum ada. Yang sudah ada tidak diubah.
async fn sync_pivot(
    tx: &mut Transaction<'_, MySql>,
    tugas_id: i64,
    ids: &[i64],
) -> Result<(), ApiError> {
    let current: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(user_id AS SIGNED) FROM tbl_survey_tugas_assignees WHERE survey_tugas_id = ?",
    )
    .bind(tugas_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)?;
    for uid in current.iter().copied().filter(|u| !ids.contains(u)) {
        sqlx::query("DELETE FROM tbl_survey_tugas_assignees WHERE survey_tugas_id = ? AND user_id = ?")
            .bind(tugas_id)
            .bind(uid)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
    }
    for uid in ids.iter().copied().filter(|u| !current.contains(u)) {
        attach(tx, tugas_id, uid).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/survey-tugas`: `per_page` (PHP `intval`, 1..=100), filter `tahun_anggaran`, `status`,
/// `assignee_id` (admin), dan `search` pada `judul` atau `lokasi_catatan`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let admin = is_admin(&state.pool, user.user_id).await?;
    let uid = user.user_id.to_string();

    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if !admin {
        conds.push(
            "(t.assignee_id = ? OR EXISTS (SELECT 1 FROM tbl_survey_tugas_assignees pv \
             WHERE pv.survey_tugas_id = t.id AND pv.user_id = ?))"
                .into(),
        );
        binds.push(uid.clone());
        binds.push(uid);
    }
    if let Some(v) = filled_q(&q, "tahun_anggaran") {
        conds.push("t.tahun_anggaran = ?".into());
        binds.push(v);
    }
    if let Some(v) = filled_q(&q, "status") {
        conds.push("t.status = ?".into());
        binds.push(v);
    }
    if admin {
        if let Some(v) = filled_q(&q, "assignee_id") {
            conds.push(
                "(t.assignee_id = ? OR EXISTS (SELECT 1 FROM tbl_survey_tugas_assignees pv \
                 WHERE pv.survey_tugas_id = t.id AND pv.user_id = ?))"
                    .into(),
            );
            binds.push(v.clone());
            binds.push(v);
        }
    }
    if let Some(s) = filled_q(&q, "search") {
        conds.push("(t.judul LIKE ? OR t.lokasi_catatan LIKE ?)".into());
        let pattern = format!("%{s}%");
        binds.push(pattern.clone());
        binds.push(pattern);
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas t{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b.clone());
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let per_page = php_intval(q.get("per_page").map(String::as_str).unwrap_or("15"))
        .clamp(1, MAX_PER_PAGE) as u64;
    let page = pagination::page_params(&q).page;
    let sql = format!(
        "{SELECT_TUGAS}{where_sql} ORDER BY t.created_at DESC, t.id DESC LIMIT {per_page} OFFSET {}",
        (page - 1).saturating_mul(per_page)
    );
    let mut dq = sqlx::query(&sql);
    for b in &binds {
        dq = dq.bind(b.clone());
    }
    let rows = dq.fetch_all(&state.pool).await.map_err(internal)?;
    let tugas: Vec<TugasRow> = rows
        .iter()
        .map(map_row)
        .collect::<Result<_, _>>()
        .map_err(internal)?;

    let ids: Vec<i64> = tugas.iter().map(|t| t.id).collect();
    let mut by_tugas = assignees_of(&state.pool, &ids).await?;
    let data: Vec<Value> = tugas
        .iter()
        .map(|t| {
            let assignees = by_tugas.remove(&t.id).unwrap_or_default();
            tugas_json(t, &assignees, INDEX_VIEW)
        })
        .collect();

    let base = format!("{}/api/survey-tugas", app_base(&state));
    Ok(Json(pagination::paginate(
        data,
        total as u64,
        PageParams { page, per_page },
        &base,
    ))
    .into_response())
}

/// `GET /api/survey-tugas/{id}`: admin, atau penanggung jawab. Selain itu 403 `Forbidden`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    let user = require_auth(&state, &headers).await?;
    if !is_admin(&state.pool, user.user_id).await?
        && !is_assignee(&state.pool, id, user.user_id).await?
    {
        return Err(forbidden("Forbidden"));
    }
    let data = load_view(&state.pool, id, SHOW_VIEW)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `POST /api/survey-tugas` (admin): 201 dengan `{data}`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state.pool, user.user_id).await?;
    let input = parse_body(&body);
    validate(&state.pool, &input, false).await?;
    let resolved = resolve_assignees(&state.pool, &input).await?;

    let pekerjaan_id = opt_int(&input, "pekerjaan_id");
    let mut kecamatan_id = opt_int(&input, "kecamatan_id");
    let mut desa_id = opt_int(&input, "desa_id");
    // `!empty(pekerjaan_id) && empty(kecamatan_id) && empty(desa_id)`: ambil lokasi dari pekerjaan.
    let mut derived = false;
    if pekerjaan_id.unwrap_or(0) != 0 && kecamatan_id.unwrap_or(0) == 0 && desa_id.unwrap_or(0) == 0 {
        if let Some((kec, desa)) = pekerjaan_location(&state.pool, pekerjaan_id.unwrap_or(0)).await? {
            kecamatan_id = kec;
            desa_id = desa;
            derived = true;
        }
    }

    let assignees: Vec<i64> = match &resolved {
        Some(ids) => ids.clone(),
        None => vec![opt_int(&input, "assignee_id").unwrap_or(0)],
    };
    let assignee_id = assignees[0];
    // `$request->get('status', 'ditugaskan')`: kunci yang ada tetapi null tetap menjadi null.
    let status = match input.get("status") {
        None => Some("ditugaskan".to_string()),
        Some(v) => text_of(v),
    };

    let url = format!("{}/api/survey-tugas", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_survey_tugas (pekerjaan_id, judul, tahun_anggaran, jenis, kecamatan_id, desa_id, \
         lokasi_catatan, assignee_id, status, batas_waktu, catatan_admin, created_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(opt_text(&input, "judul").unwrap_or_default())
    .bind(opt_int(&input, "tahun_anggaran").unwrap_or_default())
    .bind(opt_text(&input, "jenis"))
    .bind(kecamatan_id)
    .bind(desa_id)
    .bind(opt_text(&input, "lokasi_catatan"))
    .bind(assignee_id)
    .bind(status)
    .bind(opt_date(&input, "batas_waktu"))
    .bind(opt_text(&input, "catatan_admin"))
    .bind(user.user_id as i64)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    // Audit `created`: `getAttributes()` hanya berisi kunci yang di-insert, plus id dan timestamp.
    let mut keys: Vec<&str> = STORE_FIELDS
        .iter()
        .copied()
        .filter(|k| input.contains_key(*k))
        .collect();
    keys.extend(["status", "created_by", "assignee_id", "id", "created_at", "updated_at"]);
    if derived {
        keys.extend(["kecamatan_id", "desa_id"]);
    }
    let full = attributes(&mut *tx, id).await?;
    let new = pick(&full, &keys);
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "created",
        id,
        None,
        Some(new),
        &url,
    )
    .await?;

    // `syncAssignees($resolved ?? [assignee_id])`: baris pivot baru.
    for uid in dedupe(assignees.iter().copied()) {
        attach(&mut tx, id, uid).await?;
    }
    tx.commit().await.map_err(internal)?;

    let data = load_view(&state.pool, id, WRITE_VIEW)
        .await?
        .unwrap_or(Value::Null);
    Ok((StatusCode::CREATED, Json(json!({ "data": data }))).into_response())
}

/// `PUT` dan `PATCH /api/survey-tugas/{id}` (admin). Hanya kolom yang dikirim yang diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state.pool, user.user_id).await?;
    let input = parse_body(&body);
    validate(&state.pool, &input, true).await?;

    // `if (filled('assignee_id') || has('assignee_ids'))`: resolve, lalu `assignee_id` = ids[0].
    let override_id = if input.get("assignee_id").is_some_and(|v| !v.is_null())
        || input.contains_key("assignee_ids")
    {
        resolve_assignees(&state.pool, &input)
            .await?
            .map(|ids| ids[0])
    } else {
        None
    };

    let mut sets: Vec<(&str, Bind)> = Vec::new();
    for &key in UPDATE_FIELDS {
        let present = input.contains_key(key) || (key == "assignee_id" && override_id.is_some());
        if !present {
            continue;
        }
        let bind = match key {
            "pekerjaan_id" | "kecamatan_id" | "desa_id" | "tahun_anggaran" => {
                Bind::Int(opt_int(&input, key))
            }
            "assignee_id" => Bind::Int(override_id.or_else(|| opt_int(&input, key))),
            "batas_waktu" => Bind::Text(opt_date(&input, key)),
            _ => Bind::Text(opt_text(&input, key)),
        };
        sets.push((key, bind));
    }

    let url = format!("{}/api/survey-tugas/{id}", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes(&mut *tx, id).await?;
    if !sets.is_empty() {
        let cols: Vec<String> = sets.iter().map(|(k, _)| format!("{k} = ?")).collect();
        let sql = format!("UPDATE {TABLE} SET {} WHERE id = ?", cols.join(", "));
        let mut q = sqlx::query(&sql);
        for (_, b) in &sets {
            q = match b {
                Bind::Int(v) => q.bind(*v),
                Bind::Text(v) => q.bind(v.clone()),
            };
        }
        q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    }
    let after = attributes(&mut *tx, id).await?;
    audit_update(
        &mut tx,
        &headers,
        user.user_id,
        TABLE,
        MODEL,
        id,
        &before,
        after,
        &url,
    )
    .await?;

    // `assignee_ids` (tidak kosong): `sync`. Bila tidak, `assignee_id`: `syncWithoutDetaching`.
    if let Some(list) = input
        .get("assignee_ids")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    {
        let ids = dedupe(list.iter().filter_map(as_int).filter(|v| *v != 0));
        if !ids.is_empty() {
            sync_pivot(&mut tx, id, &ids).await?;
        }
    } else if let Some(aid) = input.get("assignee_id").and_then(as_int) {
        attach_missing(&mut tx, id, aid).await?;
    }
    tx.commit().await.map_err(internal)?;

    let data = load_view(&state.pool, id, WRITE_VIEW)
        .await?
        .unwrap_or(Value::Null);
    Ok(Json(json!({ "data": data })).into_response())
}

/// `DELETE /api/survey-tugas/{id}` (admin). Survei lokasi yang terkait dilepas dari tugas (`tugas_id` = NULL).
/// Pivot ikut terhapus lewat `ON DELETE CASCADE` di database.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    let user = require_auth(&state, &headers).await?;
    ensure_admin(&state.pool, user.user_id).await?;

    let url = format!("{}/api/survey-tugas/{id}", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes(&mut *tx, id).await?;
    sqlx::query("UPDATE tbl_survey_lokasi SET tugas_id = NULL WHERE tugas_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "deleted",
        id,
        Some(before),
        None,
        &url,
    )
    .await?;
    sqlx::query("DELETE FROM tbl_survey_tugas WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "message": "Tugas survey berhasil dihapus." })).into_response())
}
