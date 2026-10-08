//! `/api/penerima`: port `PenerimaController` dan `PenerimaResource`.
//!
//! - `nik` dan `alamat` disimpan terenkripsi (cast `encrypted` Laravel, kunci dari `APP_KEY`).
//! - Respon di-mask kecuali PIN (`X-PIN` atau query `pin`) cocok dengan `app_settings.penerima_pin`.
//! - Baca tidak memakai scope `byUserRole()` (sama dengan Laravel, lihat T36). Tulis memakai scope (T31).

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row};
use std::collections::BTreeMap;

use crate::{access, crypt, media::internal, pagination, pekerjaan, require_auth, AppState};

const PIN_KEY: &str = "penerima_pin";
const DEFAULT_PIN: &str = "123456";
const SELECT_PENERIMA: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, nama, \
     CAST(jumlah_jiwa AS SIGNED) AS jumlah_jiwa, CAST(nik AS CHAR) AS nik, CAST(alamat AS CHAR) AS alamat, \
     is_komunal, created_at, updated_at FROM tbl_penerima";

/// Baris `tbl_penerima` seperti tersimpan (nik dan alamat masih terenkripsi).
#[derive(Debug, Clone)]
pub struct PenerimaRow {
    pub id: i64,
    pub pekerjaan_id: i64,
    pub nama: String,
    pub jumlah_jiwa: Option<i64>,
    pub nik: Option<String>,
    pub alamat: Option<String>,
    pub is_komunal: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<PenerimaRow, sqlx::Error> {
    Ok(PenerimaRow {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        nama: r.try_get("nama")?,
        jumlah_jiwa: r.try_get("jumlah_jiwa")?,
        nik: r.try_get("nik")?,
        alamat: r.try_get("alamat")?,
        is_komunal: r.try_get("is_komunal")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<PenerimaRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_PENERIMA} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

// ---------------------------------------------------------------------------
// Enkripsi, PIN, dan mask
// ---------------------------------------------------------------------------

/// Kunci `APP_KEY`. Dibutuhkan bila ada nik atau alamat yang harus dibaca atau ditulis.
fn app_key() -> Result<Vec<u8>, ApiError> {
    let raw = std::env::var("APP_KEY")
        .map_err(|_| internal("APP_KEY belum di-set: nik dan alamat penerima tidak bisa dibaca"))?;
    crypt::key_from_app_key(&raw).map_err(|e| internal(format!("APP_KEY tidak valid: {e:?}")))
}

fn decrypt_field(payload: Option<&str>, key: &[u8]) -> Result<Option<String>, ApiError> {
    match payload {
        None => Ok(None),
        Some(p) => crypt::decrypt_string(key, p)
            .map(Some)
            .map_err(|e| internal(format!("gagal mendekripsi: {e:?}"))),
    }
}

/// `empty()` di PHP: null, "", dan "0" dianggap kosong.
fn php_empty(v: Option<&str>) -> bool {
    matches!(v, None | Some("") | Some("0"))
}

/// `$request->header('X-PIN') ?? $request->query('pin')`, dengan fallback `request()->header(..) ?: query(..)`
/// bila hasil pertama kosong (PenerimaResource).
fn pin_from(headers: &HeaderMap, query_pin: Option<&str>) -> Option<String> {
    let header = headers
        .get("x-pin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let first = header.clone().or_else(|| query_pin.map(str::to_string));
    if !php_empty(first.as_deref()) {
        return first;
    }
    let truthy_header = header.filter(|h| !php_empty(Some(h.as_str())));
    truthy_header.or_else(|| query_pin.map(str::to_string))
}

/// `$pin === AppSetting::getValue(penerima_pin, '123456')`. Setting yang ada tetapi bernilai NULL
/// menghasilkan `None`, sehingga PIN kosong dianggap cocok (perilaku Laravel, lihat T37).
async fn pin_matches(pool: &MySqlPool, pin: Option<&str>) -> Result<bool, ApiError> {
    let row: Option<Option<String>> = sqlx::query_scalar(
        "SELECT CAST(`value` AS CHAR) FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(PIN_KEY)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let expected = match row {
        Some(v) => v,
        None => Some(DEFAULT_PIN.to_string()),
    };
    Ok(pin.map(str::to_string) == expected)
}

/// `PenerimaResource::mask`: tampilkan `$show` karakter pertama, sisanya `*` (maks 12).
fn mask(val: Option<&str>, show: usize) -> Option<String> {
    let v = match val {
        None => return None,
        Some(v) if php_empty(Some(v)) => return Some(v.to_string()),
        Some(v) => v,
    };
    let bytes = v.as_bytes();
    let len = bytes.len();
    if len <= show {
        return Some("*".repeat(len));
    }
    let prefix = String::from_utf8_lossy(&bytes[..show]).into_owned();
    Some(format!("{prefix}{}", "*".repeat((len - show).min(12))))
}

/// Pekerjaan bersarang (`PekerjaanResource` dengan relasi dasar saja).
fn pekerjaan_json(row: Option<&pekerjaan::PekerjaanRow>) -> Value {
    match row {
        Some(p) => pekerjaan::to_resource(
            p,
            &pekerjaan::Loaded::empty(pekerjaan::Mode {
                summary: false,
                unbounded: false,
            }),
        ),
        None => Value::Null,
    }
}

/// `PenerimaResource::toArray`. `pekerjaan` diambil dari `cache` (dimuat sekali per request).
fn resource(
    row: &PenerimaRow,
    unmasked: bool,
    key: &[u8],
    cache: &HashMap<i64, pekerjaan::PekerjaanRow>,
) -> Result<Value, ApiError> {
    let nik = decrypt_field(row.nik.as_deref(), key)?;
    let alamat = decrypt_field(row.alamat.as_deref(), key)?;
    let (nik_out, alamat_out) = if unmasked {
        (nik, alamat)
    } else {
        (mask(nik.as_deref(), 4), mask(alamat.as_deref(), 6))
    };
    Ok(json!({
        "id": row.id,
        "nama": row.nama,
        "jumlah_jiwa": row.jumlah_jiwa,
        "nik": nik_out,
        "alamat": alamat_out,
        "is_komunal": row.is_komunal,
        "pekerjaan_id": row.pekerjaan_id,
        "pekerjaan": pekerjaan_json(cache.get(&row.pekerjaan_id)),
        // Carbon `format('Y-m-d H:i:s')`, bukan ISO 8601.
        "created_at": row.created_at.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
        "updated_at": row.updated_at.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
    }))
}

/// Ubah sekumpulan baris menjadi resource. Memuat pekerjaan induk sekali untuk semua baris.
async fn resources(
    pool: &MySqlPool,
    rows: &[PenerimaRow],
    unmasked: bool,
) -> Result<Vec<Value>, ApiError> {
    let ids: HashSet<i64> = rows.iter().map(|r| r.pekerjaan_id).collect();
    let mut cache = HashMap::new();
    for id in ids {
        if let Some(p) = pekerjaan::find(pool, id as u64).await.map_err(internal)? {
            cache.insert(id, p);
        }
    }
    let key = if rows.iter().any(|r| r.nik.is_some() || r.alamat.is_some()) {
        app_key()?
    } else {
        Vec::new()
    };
    rows.iter()
        .map(|r| resource(r, unmasked, &key, &cache))
        .collect()
}

fn pin_query(query: &HashMap<String, String>) -> Option<&str> {
    query.get("pin").map(String::as_str)
}

// ---------------------------------------------------------------------------
// Validasi input JSON (aturan Laravel: integer, string, boolean, min, max, exists)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Input {
    pekerjaan_id: Option<Option<i64>>,
    nama: Option<Option<String>>,
    jumlah_jiwa: Option<Option<i64>>,
    nik: Option<Option<String>>,
    alamat: Option<Option<String>>,
    is_komunal: Option<Option<bool>>,
}

fn attr(key: &str) -> String {
    key.replace('_', " ")
}

fn add(errs: &mut BTreeMap<String, Vec<String>>, key: &str, msg: String) {
    errs.entry(key.to_string()).or_default().push(msg);
}

/// Nilai JSON sebagai integer (angka bulat, atau string angka bulat), seperti aturan `integer`.
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Aturan `boolean`: true/false, 1/0, "1"/"0", "true"/"false".
fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64()? {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Input JSON. `required` untuk store: pekerjaan_id dan nama wajib.
fn parse_input(body: &Value, store: bool) -> Result<Input, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs = BTreeMap::new();
    let mut input = Input::default();

    let field = |key: &str| -> Option<&Value> { obj.get(key) };
    let is_null = |key: &str| matches!(field(key), None | Some(Value::Null));

    if store && is_null("pekerjaan_id") {
        add(
            &mut errs,
            "pekerjaan_id",
            "The pekerjaan id field is required.".into(),
        );
    } else if let Some(v) = field("pekerjaan_id") {
        if v.is_null() {
            input.pekerjaan_id = Some(None);
        } else {
            match as_int(v) {
                Some(n) => input.pekerjaan_id = Some(Some(n)),
                None => add(
                    &mut errs,
                    "pekerjaan_id",
                    "The pekerjaan id field must be an integer.".into(),
                ),
            }
        }
    }

    if store && is_null("nama") {
        add(&mut errs, "nama", "The nama field is required.".into());
    } else if let Some(v) = field("nama") {
        match v {
            Value::Null => input.nama = Some(None),
            Value::String(s) if s.chars().count() <= 255 => input.nama = Some(Some(s.clone())),
            Value::String(_) => add(
                &mut errs,
                "nama",
                "The nama field must not be greater than 255 characters.".into(),
            ),
            _ => add(&mut errs, "nama", "The nama field must be a string.".into()),
        }
    }

    if let Some(v) = field("jumlah_jiwa") {
        if v.is_null() {
            input.jumlah_jiwa = Some(None);
        } else {
            match as_int(v) {
                Some(n) if n >= 1 => input.jumlah_jiwa = Some(Some(n)),
                Some(_) => add(
                    &mut errs,
                    "jumlah_jiwa",
                    "The jumlah jiwa field must be at least 1.".into(),
                ),
                None => add(
                    &mut errs,
                    "jumlah_jiwa",
                    "The jumlah jiwa field must be an integer.".into(),
                ),
            }
        }
    }

    for key in ["nik", "alamat"] {
        if let Some(v) = field(key) {
            let slot = match key {
                "nik" => &mut input.nik,
                _ => &mut input.alamat,
            };
            match v {
                Value::Null => *slot = Some(None),
                Value::String(s) if s.chars().count() <= 255 => *slot = Some(Some(s.clone())),
                Value::String(_) => add(
                    &mut errs,
                    key,
                    format!(
                        "The {} field must not be greater than 255 characters.",
                        attr(key)
                    ),
                ),
                _ => add(
                    &mut errs,
                    key,
                    format!("The {} field must be a string.", attr(key)),
                ),
            }
        }
    }

    if let Some(v) = field("is_komunal") {
        if v.is_null() {
            input.is_komunal = Some(None);
        } else {
            match as_bool(v) {
                Some(b) => input.is_komunal = Some(Some(b)),
                None => add(
                    &mut errs,
                    "is_komunal",
                    "The is komunal field must be true or false.".into(),
                ),
            }
        }
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
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
    add(
        &mut errs,
        "pekerjaan_id",
        "The selected pekerjaan id is invalid.".into(),
    );
    Err(ApiError::validation("The given data was invalid.", errs))
}

async fn ensure_access(state: &AppState, actor: u64, pekerjaan_id: i64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    if access::user_can_access(&state.pool, actor, &roles, pekerjaan_id as u64)
        .await
        .map_err(internal)?
    {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ))
    }
}

fn encrypt_opt(value: Option<&str>, key: &[u8]) -> Option<String> {
    value.map(|v| crypt::encrypt_string(key, v))
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Paginasi seperti `->paginate($perPage)`.
fn paginate_rows(
    rows: Vec<Value>,
    total: u64,
    page: u64,
    per_page: u64,
    base: &str,
    extra: &str,
) -> Value {
    pagination::paginate_with_query(
        rows,
        total,
        pagination::PageParams { page, per_page },
        base,
        extra,
    )
}

fn per_page_of(query: &HashMap<String, String>, default: u64) -> Option<u64> {
    match query.get("per_page").map(|v| v.trim()) {
        None => Some(default),
        Some("-1") => None,
        Some(v) => Some(v.parse::<u64>().ok().filter(|n| *n >= 1).unwrap_or(default)),
    }
}

fn page_of(query: &HashMap<String, String>) -> u64 {
    query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1)
}

/// `$request->boolean(...)`: true untuk "1", "true", "on", "yes".
pub(crate) fn request_boolean(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// Klausa daftar penerima dan parameternya (urutan sama dengan `where` di Laravel).
fn list_filter(query: &HashMap<String, String>) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if query
        .get("tahun")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        clauses.push("p.pekerjaan_id IN (SELECT pk.id FROM tbl_pekerjaan pk JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id WHERE k.tahun_anggaran = ?)".into());
        binds.push(query["tahun"].clone());
    }
    if let Some(pid) = query.get("pekerjaan_id").filter(|v| !v.is_empty()) {
        clauses.push("p.pekerjaan_id = ?".into());
        binds.push(pid.clone());
    }
    if query.contains_key("komunal") {
        clauses.push("p.is_komunal = ?".into());
        binds.push(
            if request_boolean(&query["komunal"]) {
                "1"
            } else {
                "0"
            }
            .into(),
        );
    }
    if let Some(search) = query.get("search").filter(|v| !v.is_empty()) {
        clauses.push("p.nama LIKE ?".into());
        binds.push(format!("%{search}%"));
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, binds)
}

/// `GET /api/penerima`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let (where_sql, binds) = list_filter(&query);
    let unmasked = pin_matches(
        &state.pool,
        pin_from(&headers, pin_query(&query)).as_deref(),
    )
    .await?;
    let base = format!("{}/api/penerima", state.app_url.trim_end_matches('/'));
    list_response(
        &state,
        &query,
        unmasked,
        &where_sql,
        &binds,
        default_per_page(),
        &base,
    )
    .await
}

fn default_per_page() -> u64 {
    20
}

/// Daftar dengan `WHERE` tambahan, paginasi atau semua baris (`per_page=-1`).
async fn list_response(
    state: &AppState,
    query: &HashMap<String, String>,
    unmasked: bool,
    where_sql: &str,
    binds: &[String],
    default_pp: u64,
    base: &str,
) -> Result<Response, ApiError> {
    let per_page = per_page_of(query, default_pp);
    let total = {
        let sql = format!("SELECT COUNT(*) FROM tbl_penerima p{where_sql}");
        let mut q = sqlx::query_scalar::<_, i64>(&sql);
        for b in binds {
            q = q.bind(b);
        }
        q.fetch_one(&state.pool).await.map_err(internal)? as u64
    };
    let page = page_of(query);
    let mut sql =
        format!("{SELECT_PENERIMA_ALIAS}{where_sql} ORDER BY p.created_at DESC, p.id DESC");
    if per_page.is_some() {
        sql.push_str(" LIMIT ? OFFSET ?");
    }
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    if let Some(pp) = per_page {
        q = q.bind(pp).bind((page - 1) * pp);
    }
    let rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    let data = resources(&state.pool, &rows, unmasked).await?;

    match per_page {
        None => Ok(Json(json!({ "data": data })).into_response()),
        Some(pp) => {
            let extra = pekerjaan::query_without_page(query_string(query).as_deref());
            Ok(Json(paginate_rows(data, total, page, pp, base, &extra)).into_response())
        }
    }
}

const SELECT_PENERIMA_ALIAS: &str = "SELECT CAST(p.id AS SIGNED) AS id, CAST(p.pekerjaan_id AS SIGNED) AS pekerjaan_id, p.nama, \
     CAST(p.jumlah_jiwa AS SIGNED) AS jumlah_jiwa, CAST(p.nik AS CHAR) AS nik, CAST(p.alamat AS CHAR) AS alamat, \
     p.is_komunal, p.created_at, p.updated_at FROM tbl_penerima p";

/// Query string mentah dari map (dipakai untuk `appends`). Urutan map tidak dijamin sama dengan URL asli.
fn query_string(query: &HashMap<String, String>) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    let mut pairs: Vec<String> = query.iter().map(|(k, v)| format!("{k}={v}")).collect();
    pairs.sort();
    Some(pairs.join("&"))
}

/// `GET /api/penerima/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let unmasked = pin_matches(
        &state.pool,
        pin_from(&headers, pin_query(&query)).as_deref(),
    )
    .await?;
    let data = resources(&state.pool, std::slice::from_ref(&row), unmasked).await?;
    Ok(Json(json!({ "data": data.into_iter().next() })).into_response())
}

/// `POST /api/penerima`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_input(&body, true)?;
    let pekerjaan_id = input.pekerjaan_id.flatten().unwrap_or_default();
    ensure_pekerjaan_exists(&state.pool, pekerjaan_id).await?;
    ensure_access(&state, user.user_id, pekerjaan_id).await?;

    let key = if input.nik.as_ref().is_some_and(Option::is_some)
        || input.alamat.as_ref().is_some_and(Option::is_some)
    {
        app_key()?
    } else {
        Vec::new()
    };
    let nik = input.nik.clone().flatten();
    let alamat = input.alamat.clone().flatten();
    let nama = input.nama.clone().flatten().unwrap_or_default();
    let is_komunal = input.is_komunal.flatten().unwrap_or(false);

    let res = sqlx::query(
        "INSERT INTO tbl_penerima (pekerjaan_id, nama, jumlah_jiwa, nik, alamat, is_komunal, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(&nama)
    .bind(input.jumlah_jiwa.flatten())
    .bind(encrypt_opt(nik.as_deref(), &key))
    .bind(encrypt_opt(alamat.as_deref(), &key))
    .bind(is_komunal)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("penerima baru tidak terbaca"))?;
    let unmasked = pin_matches(
        &state.pool,
        pin_from(&headers, pin_query(&query)).as_deref(),
    )
    .await?;
    let data = resources(&state.pool, std::slice::from_ref(&row), unmasked).await?;
    Ok(Json(json!({ "data": data.into_iter().next() })).into_response())
}

/// `PUT` dan `PATCH /api/penerima/{id}`. Field nullable yang dikirim null disimpan sebagai null.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state, user.user_id, current.pekerjaan_id).await?;
    let input = parse_input(&body, false)?;

    if let Some(Some(p)) = input.pekerjaan_id {
        ensure_pekerjaan_exists(&state.pool, p).await?;
        ensure_access(&state, user.user_id, p).await?;
    }

    let key = if input.nik.is_some() || input.alamat.is_some() {
        app_key()?
    } else {
        Vec::new()
    };

    let mut sets: Vec<&str> = Vec::new();
    let mut qb = sqlx::QueryBuilder::<MySql>::new("UPDATE tbl_penerima SET ");
    let mut first = true;
    let mut push_set = |qb: &mut sqlx::QueryBuilder<MySql>, col: &'static str| {
        if !first {
            qb.push(", ");
        }
        first = false;
        qb.push(col).push(" = ");
        sets.push(col);
    };
    if let Some(v) = input.pekerjaan_id {
        push_set(&mut qb, "pekerjaan_id");
        qb.push_bind(v);
    }
    if let Some(v) = input.nama.clone() {
        push_set(&mut qb, "nama");
        qb.push_bind(v);
    }
    if let Some(v) = input.jumlah_jiwa {
        push_set(&mut qb, "jumlah_jiwa");
        qb.push_bind(v);
    }
    if let Some(v) = input.nik.clone() {
        push_set(&mut qb, "nik");
        qb.push_bind(encrypt_opt(v.as_deref(), &key));
    }
    if let Some(v) = input.alamat.clone() {
        push_set(&mut qb, "alamat");
        qb.push_bind(encrypt_opt(v.as_deref(), &key));
    }
    if let Some(v) = input.is_komunal {
        push_set(&mut qb, "is_komunal");
        qb.push_bind(v);
    }
    if !sets.is_empty() {
        qb.push(", updated_at = NOW()");
        qb.push(" WHERE id = ").push_bind(id);
        qb.build().execute(&state.pool).await.map_err(internal)?;
    }

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let unmasked = pin_matches(
        &state.pool,
        pin_from(&headers, pin_query(&query)).as_deref(),
    )
    .await?;
    let data = resources(&state.pool, std::slice::from_ref(&row), unmasked).await?;
    Ok(Json(json!({ "data": data.into_iter().next() })).into_response())
}

/// `DELETE /api/penerima/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state, user.user_id, current.pekerjaan_id).await?;
    sqlx::query("DELETE FROM tbl_penerima WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok((
        StatusCode::OK,
        Json(json!({ "message": "Penerima berhasil dihapus" })),
    )
        .into_response())
}

/// `GET /api/penerima/pekerjaan/{pekerjaan_id}` (per_page default 50).
pub async fn by_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pekerjaan_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let base = format!(
        "{}/api/penerima/pekerjaan/{pekerjaan_id}",
        state.app_url.trim_end_matches('/')
    );
    let unmasked = pin_matches(
        &state.pool,
        pin_from(&headers, pin_query(&query)).as_deref(),
    )
    .await?;
    let pid: i64 = pekerjaan_id.parse().unwrap_or(-1);
    list_response(
        &state,
        &query,
        unmasked,
        " WHERE p.pekerjaan_id = ?",
        &[pid.to_string()],
        50,
        &base,
    )
    .await
}

/// `GET /api/penerima/pekerjaan/{pekerjaan_id}/stats/komunal`. `pekerjaan_id` dikembalikan sebagai string.
pub async fn komunal_count(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pekerjaan_id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let pid: i64 = pekerjaan_id.parse().unwrap_or(-1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_penerima WHERE pekerjaan_id = ?")
        .bind(pid)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let komunal: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_penerima WHERE pekerjaan_id = ? AND is_komunal = 1",
    )
    .bind(pid)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "pekerjaan_id": pekerjaan_id,
        "total_penerima": total,
        "komunal_count": komunal,
        "non_komunal_count": total - komunal,
    }))
    .into_response())
}

/// `GET /api/penerima/summary`.
pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let mut clauses = vec!["p.pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE is_konsultan = 0 OR is_konsultan IS NULL)".to_string()];
    let mut binds: Vec<String> = Vec::new();
    if query
        .get("tahun")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        clauses.push("p.pekerjaan_id IN (SELECT pk.id FROM tbl_pekerjaan pk JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id WHERE k.tahun_anggaran = ?)".into());
        binds.push(query["tahun"].clone());
    }
    if let Some(b) = query.get("bidang").filter(|v| !v.is_empty()) {
        clauses.push("p.pekerjaan_id IN (SELECT pk.id FROM tbl_pekerjaan pk JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id WHERE k.sub_bidang = ?)".into());
        binds.push(b.clone());
    }
    let sql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED), CAST(COALESCE(SUM(CASE WHEN p.is_komunal = 1 THEN 1 ELSE 0 END), 0) AS SIGNED), \
         CAST(COALESCE(SUM(p.jumlah_jiwa), 0) AS SIGNED) FROM tbl_penerima p WHERE {}",
        clauses.join(" AND ")
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let r = q.fetch_one(&state.pool).await.map_err(internal)?;
    let total: i64 = r.try_get(0).map_err(internal)?;
    let komunal: i64 = r.try_get(1).map_err(internal)?;
    let total_jiwa: i64 = r.try_get(2).map_err(internal)?;
    Ok(Json(json!({
        "total_penerima": total,
        "komunal_count": komunal,
        "individu_count": total - komunal,
        "total_jiwa": total_jiwa,
    }))
    .into_response())
}

/// Simpul pohon rekap: tahun → bidang → kecamatan → desa. Urutan sisipan dipertahankan.
#[derive(Default)]
struct Node {
    key: String,
    penerima_kk: i64,
    total_jiwa: i64,
    children: Vec<Node>,
    desa_leaf: Option<(i64, i64)>,
}

fn child<'a>(nodes: &'a mut Vec<Node>, key: &str) -> &'a mut Node {
    if let Some(i) = nodes.iter().position(|n| n.key == key) {
        return &mut nodes[i];
    }
    nodes.push(Node {
        key: key.to_string(),
        ..Default::default()
    });
    nodes.last_mut().unwrap()
}

/// `GET /api/penerima/rekap`.
pub async fn rekap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let mut clauses = vec!["(pk.is_konsultan = 0 OR pk.is_konsultan IS NULL)".to_string()];
    let mut binds: Vec<String> = Vec::new();
    if query.get("tahun").is_some_and(|v| !v.is_empty()) {
        clauses.push("k.tahun_anggaran = ?".into());
        binds.push(query["tahun"].clone());
    }
    if query.get("bidang").is_some_and(|v| !v.is_empty()) {
        clauses.push("k.sub_bidang = ?".into());
        binds.push(query["bidang"].clone());
    }
    let sql = format!(
        "SELECT CAST(k.tahun_anggaran AS CHAR) AS tahun, CAST(k.sub_bidang AS CHAR) AS bidang, \
         CAST(kc.n_kec AS CHAR) AS kecamatan, CAST(d.n_desa AS CHAR) AS desa, \
         CAST(COUNT(*) AS SIGNED) AS kk, CAST(COALESCE(SUM(p.jumlah_jiwa), 0) AS SIGNED) AS jiwa \
         FROM tbl_penerima p \
         JOIN tbl_pekerjaan pk ON pk.id = p.pekerjaan_id \
         LEFT JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id \
         LEFT JOIN tbl_kecamatan kc ON kc.id = pk.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = pk.desa_id \
         WHERE {} \
         GROUP BY k.tahun_anggaran, k.sub_bidang, kc.n_kec, d.n_desa \
         ORDER BY k.tahun_anggaran DESC, k.sub_bidang, kc.n_kec, d.n_desa",
        clauses.join(" AND ")
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let mut tahun: Vec<Node> = Vec::new();
    for r in &rows {
        // `?:` di PHP: nilai kosong dan "0" memakai label cadangan.
        let fallback = |v: Option<String>, label: &str| -> String {
            match v {
                Some(s) if !php_empty(Some(&s)) => s,
                _ => label.to_string(),
            }
        };
        let t = fallback(r.try_get("tahun").map_err(internal)?, "(Tanpa tahun)");
        let b = fallback(r.try_get("bidang").map_err(internal)?, "Lainnya");
        let kc = fallback(
            r.try_get("kecamatan").map_err(internal)?,
            "(Tanpa kecamatan)",
        );
        let ds = fallback(r.try_get("desa").map_err(internal)?, "(Tanpa desa)");
        let kk: i64 = r.try_get("kk").map_err(internal)?;
        let jiwa: i64 = r.try_get("jiwa").map_err(internal)?;

        let tn = child(&mut tahun, &t);
        tn.penerima_kk += kk;
        tn.total_jiwa += jiwa;
        let bn = child(&mut tn.children, &b);
        bn.penerima_kk += kk;
        bn.total_jiwa += jiwa;
        let kn = child(&mut bn.children, &kc);
        kn.penerima_kk += kk;
        kn.total_jiwa += jiwa;
        let dn = child(&mut kn.children, &ds);
        dn.desa_leaf = Some((kk, jiwa));
    }

    fn leaf_json(n: &Node) -> Value {
        let (kk, jiwa) = n.desa_leaf.unwrap_or((0, 0));
        json!({ "desa": n.key, "penerima_kk": kk, "total_jiwa": jiwa })
    }
    let data: Vec<Value> = tahun
        .iter()
        .map(|t| {
            let bidang: Vec<Value> = t
                .children
                .iter()
                .map(|b| {
                    let kecamatan: Vec<Value> = b
                        .children
                        .iter()
                        .map(|kc| {
                            let desa: Vec<Value> = kc.children.iter().map(leaf_json).collect();
                            json!({
                                "kecamatan": kc.key,
                                "penerima_kk": kc.penerima_kk,
                                "total_jiwa": kc.total_jiwa,
                                "desa": desa,
                            })
                        })
                        .collect();
                    json!({
                        "bidang": b.key,
                        "penerima_kk": b.penerima_kk,
                        "total_jiwa": b.total_jiwa,
                        "kecamatan": kecamatan,
                    })
                })
                .collect();
            json!({
                "tahun": t.key,
                "penerima_kk": t.penerima_kk,
                "total_jiwa": t.total_jiwa,
                "bidang": bidang,
            })
        })
        .collect();
    Ok(Json(json!({ "data": data })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_keeps_prefix_and_caps_stars() {
        assert_eq!(
            mask(Some("3203123456789012"), 4).unwrap(),
            "3203************"
        );
        assert_eq!(mask(Some("abc"), 4).unwrap(), "***");
        assert_eq!(mask(Some(""), 4).unwrap(), "");
        assert_eq!(mask(None, 4), None);
    }

    #[test]
    fn pin_prefers_header_then_query_like_php() {
        let mut h = HeaderMap::new();
        h.insert("x-pin", "111".parse().unwrap());
        assert_eq!(pin_from(&h, Some("222")).as_deref(), Some("111"));
        let empty = HeaderMap::new();
        assert_eq!(pin_from(&empty, Some("222")).as_deref(), Some("222"));
        let mut blank = HeaderMap::new();
        blank.insert("x-pin", "".parse().unwrap());
        assert_eq!(pin_from(&blank, Some("333")).as_deref(), Some("333"));
        assert_eq!(pin_from(&empty, None), None);
    }

    #[test]
    fn create_requires_pekerjaan_and_nama() {
        let errs = parse_input(&json!({}), true).unwrap_err().body();
        assert!(errs["errors"]["pekerjaan_id"].is_array());
        assert!(errs["errors"]["nama"].is_array());
        let ok = parse_input(
            &json!({"pekerjaan_id": "7", "nama": "Budi", "is_komunal": "1"}),
            true,
        )
        .unwrap();
        assert_eq!(ok.pekerjaan_id, Some(Some(7)));
        assert_eq!(ok.is_komunal, Some(Some(true)));
    }

    #[test]
    fn jumlah_jiwa_must_be_at_least_one() {
        let err = parse_input(&json!({"jumlah_jiwa": 0}), false)
            .unwrap_err()
            .body();
        assert_eq!(
            err["errors"]["jumlah_jiwa"][0],
            "The jumlah jiwa field must be at least 1."
        );
    }
}
