//! Register dokumen (`DocumentRegisterController`, tabel `tbl_document_registers`): daftar, buat,
//! ubah, dan hapus. Tipe dokumen (`types`, `storeType`, dst.) ada di `document_types_write.rs`.
//!
//! Catatan paritas:
//! - `tanggal` (cast `date`) dikirim sebagai `YYYY-MM-DDT00:00:00.000000Z`, seperti serialisasi Carbon.
//! - `index` memuat `kontrak` (dengan `pekerjaan` dan `penyedia`), `type`, dan `addendum` sebagai model
//!   lengkap, dengan kolom dan cast dari tabel. Tanggal cast `date` memakai bentuk Carbon (`toJSON`),
//!   termasuk pada `addendum`. `kontrak.pekerjaans` tidak dimuat Laravel, jadi tidak dikirim.
//! - `store` menomori dengan `tbl_document_sequences` (tipe `berita-acara`) di dalam transaksi, sama
//!   dengan Laravel. Error `RuntimeException` menjadi 422 dan transaksi dibatalkan.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    format::number_like_php,
    lookup::{carbon_json, document_type_json, DocumentTypeRow},
    pagination::{self, PageParams},
    pekerjaan::{laravel_page_url, laravel_query_pairs},
    pekerjaan_doc_register::{read, select_list, K, KONTRAK_COLS, PEKERJAAN_COLS, PENYEDIA_COLS},
    require_auth,
    validation::Errors,
    AppState,
};

const SEQ_TYPE: &str = "berita-acara";
const DEFAULT_TEMPLATE: &str = "{sequence}/{code}-AMIS/{month}/{year}";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// Tanggal cast `date` Laravel. Menerima `YYYY-MM-DD`, `YYYY-MM-DD HH:MM:SS`, dan RFC 3339.
fn parse_date(raw: &str) -> Option<NaiveDate> {
    let t = raw.trim();
    NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|d| d.date())
        })
        .or_else(|| DateTime::parse_from_rfc3339(t).ok().map(|d| d.date_naive()))
}

fn date_json(d: Option<NaiveDate>) -> Value {
    match d {
        Some(d) => json!(format!("{}T00:00:00.000000Z", d.format("%Y-%m-%d"))),
        None => Value::Null,
    }
}

struct RegRow {
    id: i64,
    kontrak_id: i64,
    type_id: i64,
    addendum_id: Option<i64>,
    attachment_type: Option<String>,
    nomor: String,
    tanggal: Option<NaiveDate>,
    sequence_number: i64,
    year: i64,
    description: Option<String>,
    nilai: Option<f64>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_REG: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(kontrak_id AS SIGNED) AS kontrak_id, \
     CAST(type_id AS SIGNED) AS type_id, CAST(addendum_id AS SIGNED) AS addendum_id, attachment_type, nomor, \
     tanggal, CAST(sequence_number AS SIGNED) AS sequence_number, CAST(year AS SIGNED) AS year, description, \
     CAST(nilai AS DOUBLE) AS nilai, created_at, updated_at FROM tbl_document_registers";

fn map_reg(r: &sqlx::mysql::MySqlRow) -> Result<RegRow, sqlx::Error> {
    Ok(RegRow {
        id: r.try_get("id")?,
        kontrak_id: r.try_get("kontrak_id")?,
        type_id: r.try_get("type_id")?,
        addendum_id: r.try_get("addendum_id")?,
        attachment_type: r.try_get("attachment_type")?,
        nomor: r.try_get("nomor")?,
        tanggal: r.try_get("tanggal")?,
        sequence_number: r.try_get("sequence_number")?,
        year: r.try_get("year")?,
        description: r.try_get("description")?,
        nilai: r.try_get("nilai")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

fn reg_attributes(r: &RegRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(r.id));
    m.insert("kontrak_id".into(), json!(r.kontrak_id));
    m.insert("type_id".into(), json!(r.type_id));
    m.insert("addendum_id".into(), json!(r.addendum_id));
    m.insert("attachment_type".into(), json!(r.attachment_type));
    m.insert("nomor".into(), json!(r.nomor));
    m.insert("tanggal".into(), date_json(r.tanggal));
    m.insert("sequence_number".into(), json!(r.sequence_number));
    m.insert("year".into(), json!(r.year));
    m.insert("description".into(), json!(r.description));
    m.insert("nilai".into(), r.nilai.map_or(Value::Null, number_like_php));
    m.insert("created_at".into(), carbon_json(r.created_at));
    m.insert("updated_at".into(), carbon_json(r.updated_at));
    m
}

async fn type_json(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, name, code, format_template, created_at, updated_at \
         FROM tbl_document_types WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let t = DocumentTypeRow {
        id: r.try_get::<i64, _>("id").map_err(internal)? as u64,
        name: r.try_get("name").map_err(internal)?,
        code: r.try_get("code").map_err(internal)?,
        format_template: r.try_get("format_template").map_err(internal)?,
        created_at: r.try_get("created_at").map_err(internal)?,
        updated_at: r.try_get("updated_at").map_err(internal)?,
    };
    Ok(document_type_json(&t))
}

/// Bentuk respons `load('type')`: atribut register dan `type`.
async fn with_type(pool: &MySqlPool, r: &RegRow) -> Result<Value, ApiError> {
    let mut m = reg_attributes(r);
    m.insert("type".into(), type_json(pool, r.type_id).await?);
    Ok(Value::Object(m))
}

async fn find(pool: &MySqlPool, id: i64) -> Result<RegRow, ApiError> {
    let sql = format!("{SELECT_REG} WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_reg(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// Kolom `SELECT *` model `KontrakAddendum` beserta cast Eloquent-nya (urutan kolom tabel).
const ADDENDUM_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("kontrak_id", K::Int),
    ("addendum_ke", K::Int),
    ("nomor_addendum", K::Txt),
    ("attachment_nomors", K::Json),
    ("tanggal_addendum", K::Date),
    ("jenis_addendum", K::Txt),
    ("alasan", K::Txt),
    ("deskripsi_perubahan", K::Txt),
    ("nilai_kontrak_sebelum", K::Flt),
    ("nilai_kontrak_sesudah", K::Flt),
    ("tgl_selesai_sebelum", K::Date),
    ("tgl_selesai_sesudah", K::Date),
    ("status", K::Txt),
    ("kelengkapan_override", K::Bool),
    ("created_by", K::Int),
    ("approved_by", K::Int),
    ("approved_at", K::Ts),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
];

/// Satu baris model Eloquent sebagai atribut JSON (`toArray()`), atau null bila baris tidak ada.
async fn model_json(
    pool: &MySqlPool,
    table: &str,
    cols: &[(&str, K)],
    id: i64,
) -> Result<Value, ApiError> {
    let sql = format!("SELECT {} FROM {table} WHERE id = ?", select_list("", cols));
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match row {
        Some(r) => Ok(Value::Object(read(&r, cols).map_err(internal)?)),
        None => Ok(Value::Null),
    }
}

/// Relasi `kontrak` untuk indeks: atribut lengkap `Kontrak`, plus `pekerjaan` dan `penyedia`
/// (`with('kontrak.pekerjaan', 'kontrak.penyedia')`). Laravel tidak memuat `kontrak.pekerjaans`
/// di indeks ini, jadi kunci itu tidak ada di respons.
async fn kontrak_json(pool: &MySqlPool, kontrak_id: i64) -> Result<Value, ApiError> {
    let Value::Object(mut m) = model_json(pool, "tbl_kontrak", KONTRAK_COLS, kontrak_id).await? else {
        return Ok(Value::Null);
    };
    let id_pekerjaan = m.get("id_pekerjaan").and_then(Value::as_i64);
    let id_penyedia = m.get("id_penyedia").and_then(Value::as_i64);
    let pekerjaan = match id_pekerjaan {
        Some(id) => model_json(pool, "tbl_pekerjaan", PEKERJAAN_COLS, id).await?,
        None => Value::Null,
    };
    let penyedia = match id_penyedia {
        Some(id) => model_json(pool, "tbl_penyedia", PENYEDIA_COLS, id).await?,
        None => Value::Null,
    };
    m.insert("pekerjaan".into(), pekerjaan);
    m.insert("penyedia".into(), penyedia);
    Ok(Value::Object(m))
}

/// `GET /api/document-registers?tahun=&type_id=&addendum_id=&search=&per_page=`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    // `$request->has()` bernilai false untuk string kosong.
    let has = |k: &str| query.get(k).filter(|v| !v.is_empty()).cloned();
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(t) = has("tahun") {
        clauses.push("r.year = ?".into());
        binds.push(t);
    }
    if let Some(t) = has("type_id") {
        clauses.push("r.type_id = ?".into());
        binds.push(t);
    }
    if let Some(a) = has("addendum_id") {
        clauses.push("r.addendum_id = ?".into());
        binds.push(a);
    }
    if let Some(s) = has("search") {
        let like = format!("%{s}%");
        clauses.push(
            "(r.nomor LIKE ? OR r.description LIKE ? OR EXISTS (SELECT 1 FROM tbl_kontrak k \
             JOIN tbl_pekerjaan p ON p.id = k.id_pekerjaan WHERE k.id = r.kontrak_id \
             AND (p.nama_paket LIKE ? OR p.kode_rekening LIKE ?)))"
                .into(),
        );
        binds.extend([like.clone(), like.clone(), like.clone(), like]);
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    let per: i64 = query
        .get("per_page")
        .map(|v| v.trim().parse().unwrap_or(0))
        .unwrap_or(20);
    let per_page = if per <= 0 { 20 } else { per as u64 };
    let page = pagination::page_params(&query).page;

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_registers r{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let sql = format!(
        "{SELECT_REG} r{where_sql} ORDER BY r.created_at DESC, r.id DESC LIMIT {per_page} OFFSET {}",
        (page - 1) * per_page
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let r = map_reg(row).map_err(internal)?;
        let mut m = reg_attributes(&r);
        m.insert("kontrak".into(), kontrak_json(&state.pool, r.kontrak_id).await?);
        m.insert("type".into(), type_json(&state.pool, r.type_id).await?);
        m.insert(
            "addendum".into(),
            match r.addendum_id {
                Some(aid) => addendum_json(&state.pool, aid).await?,
                None => Value::Null,
            },
        );
        data.push(Value::Object(m));
    }
    let base = format!("{}/api/document-registers", state.app_url.trim_end_matches('/'));
    let params = PageParams { page, per_page };
    // Paginator Laravel mentah (`paginate()` lalu `response()->json`), tanpa pembungkus `meta`.
    let pairs = laravel_query_pairs(raw.as_deref());
    Ok(Json(pagination::paginate_flat(
        data,
        total as u64,
        params,
        &base,
        &|p| laravel_page_url(&base, &pairs, p),
    ))
    .into_response())
}

async fn addendum_json(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    model_json(pool, "tbl_kontrak_addendums", ADDENDUM_COLS, id).await
}

/// Bentuk generik `{sequence}` dst. dari `generateNumber`.
fn generate_number(
    template: Option<&str>,
    code: &str,
    sequence: i64,
    date: NaiveDate,
    kontrak_id: i64,
    id_pekerjaan: Option<i64>,
) -> String {
    let template = template.filter(|t| !t.is_empty()).unwrap_or(DEFAULT_TEMPLATE);
    const ROMAN: [&str; 12] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"];
    use chrono::Datelike;
    let year = date.year().to_string();
    let month = ROMAN[(date.month() - 1) as usize];
    let mut out = template
        .replace("{sequence}", &format!("{sequence:03}"))
        .replace("{nomor_urut_surat}", &sequence.to_string())
        .replace("{code}", code)
        .replace("{year}", &year)
        .replace("{tahun}", &year)
        .replace("{month}", month)
        .replace("{day}", &format!("{:02}", date.day()))
        .replace("{kontrak_id}", &kontrak_id.to_string());
    out = out.replace("{id_pekerjaan}", &id_pekerjaan.map(|v| v.to_string()).unwrap_or_default());
    out
}

struct NewReg<'a> {
    kontrak_id: i64,
    type_id: i64,
    addendum_id: Option<i64>,
    nomor: Option<String>,
    tanggal: NaiveDate,
    description: Option<&'a str>,
    nilai: Option<f64>,
    sequence: Option<i64>,
}

/// `POST /api/document-registers`: 201 dengan `type`. Nomor dan urutan dari tabel sequence.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let kontrak_id = id_field(&state.pool, &mut e, &input, "kontrak_id", "kontrak", "tbl_kontrak", true).await?;
    let type_id = id_field(&state.pool, &mut e, &input, "type_id", "type", "tbl_document_types", true).await?;
    let addendum_id = id_field(&state.pool, &mut e, &input, "addendum_id", "addendum", "tbl_kontrak_addendums", false).await?;
    let tanggal = date_field(&mut e, &input, "tanggal", true);
    let description = match input.get("description") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            e.add("description", "The description field must be a string.");
            None
        }
    };
    let nilai = nilai_field(&mut e, &input);
    let sequence = sequence_field(&mut e, &input);
    let nomor = match input.get("nomor") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.chars().count() <= 255 => Some(s.clone()),
        Some(Value::String(_)) => {
            e.add("nomor", "The nomor field must not be greater than 255 characters.");
            None
        }
        Some(_) => {
            e.add("nomor", "The nomor field must be a string.");
            None
        }
    };
    e.finish()?;
    let (Some(kontrak_id), Some(type_id), Some(tanggal)) = (kontrak_id, type_id, tanggal) else {
        return Err(internal("validasi tidak lengkap"));
    };
    let new = NewReg {
        kontrak_id,
        type_id,
        addendum_id,
        nomor,
        tanggal,
        description,
        nilai,
        sequence,
    };

    let code: String = sqlx::query_scalar("SELECT code FROM tbl_document_types WHERE id = ?")
        .bind(type_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let template: Option<String> = sqlx::query_scalar("SELECT format_template FROM tbl_document_types WHERE id = ?")
        .bind(type_id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    let id_pekerjaan: Option<i64> = sqlx::query_scalar("SELECT CAST(id_pekerjaan AS SIGNED) FROM tbl_kontrak WHERE id = ?")
        .bind(kontrak_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .flatten();

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    let created = match create_in_tx(&mut tx, &new, &code, template.as_deref(), id_pekerjaan).await {
        Ok(id) => id,
        Err(msg) => {
            tx.rollback().await.map_err(internal)?;
            return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "message": msg }))).into_response());
        }
    };
    tx.commit().await.map_err(internal)?;

    let row = find(&state.pool, created).await?;
    Ok((StatusCode::CREATED, Json(with_type(&state.pool, &row).await?)).into_response())
}

/// Isi transaksi `store`. `Err(pesan)` untuk `RuntimeException` (422).
async fn create_in_tx(
    tx: &mut Transaction<'_, MySql>,
    new: &NewReg<'_>,
    code: &str,
    template: Option<&str>,
    id_pekerjaan: Option<i64>,
) -> Result<i64, String> {
    use chrono::Datelike;
    let year = new.tanggal.year() as i64;
    let sequence = match new.sequence {
        Some(seq) => {
            let used: i64 = sqlx::query_scalar(
                "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_registers WHERE year = ? AND sequence_number = ?",
            )
            .bind(year)
            .bind(seq)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
            if used > 0 {
                return Err(format!("Sequence nomor {seq} untuk tahun {year} sudah digunakan."));
            }
            seq
        }
        None => {
            // `lockForUpdate()` pada baris sequence tahun ini.
            let last: Option<Option<i64>> = sqlx::query_scalar(
                "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = ? FOR UPDATE",
            )
            .bind(year)
            .bind(SEQ_TYPE)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
            last.flatten().map_or(1, |l| l + 1)
        }
    };
    sqlx::query(
        "INSERT INTO tbl_document_sequences (year, type, last_number) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE last_number = VALUES(last_number)",
    )
    .bind(year)
    .bind(SEQ_TYPE)
    .bind(sequence)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    let nomor = match new.nomor.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => generate_number(template, code, sequence, new.tanggal, new.kontrak_id, id_pekerjaan),
    };
    let taken: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_registers WHERE nomor = ?")
        .bind(&nomor)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    if taken > 0 {
        return Err(format!("Nomor dokumen {nomor} sudah terdaftar."));
    }

    let res = sqlx::query(
        "INSERT INTO tbl_document_registers (kontrak_id, type_id, addendum_id, nomor, tanggal, sequence_number, year, \
         description, nilai, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(new.kontrak_id)
    .bind(new.type_id)
    .bind(new.addendum_id)
    .bind(&nomor)
    .bind(new.tanggal)
    .bind(sequence)
    .bind(year)
    .bind(new.description)
    .bind(new.nilai)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    Ok(res.last_insert_id() as i64)
}

/// `required|exists:tabel,id` (atau `nullable`). Mengembalikan id bila valid.
async fn id_field(
    pool: &MySqlPool,
    e: &mut Errors,
    input: &Map<String, Value>,
    field: &str,
    label: &str,
    table: &str,
    required: bool,
) -> Result<Option<i64>, ApiError> {
    let attribute = attr(field);
    let raw = match input.get(field) {
        None | Some(Value::Null) => {
            if required {
                e.add(field, format!("The {attribute} field is required."));
            }
            return Ok(None);
        }
        Some(v) => v,
    };
    let parsed = match raw {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    };
    let Some(id) = parsed else {
        e.add(field, format!("The selected {label} id is invalid."));
        return Ok(None);
    };
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n == 0 {
        e.add(field, format!("The selected {label} id is invalid."));
        return Ok(None);
    }
    Ok(Some(id))
}

/// Tanggal `required|date`.
fn date_field(e: &mut Errors, input: &Map<String, Value>, field: &str, required: bool) -> Option<NaiveDate> {
    match input.get(field) {
        None | Some(Value::Null) => {
            if required {
                e.add(field, format!("The {} field is required.", attr(field)));
            }
            None
        }
        Some(Value::String(s)) => match parse_date(s) {
            Some(d) => Some(d),
            None => {
                e.add(field, format!("The {} field must be a valid date.", attr(field)));
                None
            }
        },
        Some(_) => {
            e.add(field, format!("The {} field must be a valid date.", attr(field)));
            None
        }
    }
}

/// `nullable|numeric|min:0`.
fn nilai_field(e: &mut Errors, input: &Map<String, Value>) -> Option<f64> {
    let parsed = match input.get("nilai") {
        None | Some(Value::Null) => return None,
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        Some(_) => None,
    };
    match parsed {
        None => {
            e.add("nilai", "The nilai field must be a number.");
            None
        }
        Some(v) if v < 0.0 => {
            e.add("nilai", "The nilai field must be at least 0.");
            None
        }
        Some(v) => Some(v),
    }
}

/// `nullable|integer|min:1`. Teks kosong dianggap tidak diisi (`filled`).
fn sequence_field(e: &mut Errors, input: &Map<String, Value>) -> Option<i64> {
    let parsed = match input.get("sequence_number") {
        None | Some(Value::Null) => return None,
        Some(Value::String(s)) if s.trim().is_empty() => return None,
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(_) => None,
    };
    match parsed {
        None => {
            e.add("sequence_number", "The sequence number field must be an integer.");
            None
        }
        Some(v) if v < 1 => {
            e.add("sequence_number", "The sequence number field must be at least 1.");
            None
        }
        Some(v) => Some(v),
    }
}

/// `PUT` dan `PATCH /api/document-registers/{id}`: tanggal dan nomor wajib, sisanya nullable.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let tanggal = date_field(&mut e, &input, "tanggal", true);
    let nomor = match input.get("nomor") {
        None | Some(Value::Null) => {
            e.add("nomor", "The nomor field is required.");
            None
        }
        Some(Value::String(s)) if s.chars().count() <= 255 => Some(s.clone()),
        Some(Value::String(_)) => {
            e.add("nomor", "The nomor field must not be greater than 255 characters.");
            None
        }
        Some(_) => {
            e.add("nomor", "The nomor field must be a string.");
            None
        }
    };
    let description = match input.get("description") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some(Some(s.clone())),
        Some(_) => {
            e.add("description", "The description field must be a string.");
            None
        }
    };
    let nilai = nilai_field(&mut e, &input);
    let addendum = id_field(&state.pool, &mut e, &input, "addendum_id", "addendum", "tbl_kontrak_addendums", false).await?;
    if let Some(n) = &nomor {
        let taken: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_document_registers WHERE nomor = ? AND id <> ?",
        )
        .bind(n)
        .bind(current.id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
        if taken > 0 {
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "message": "Nomor dokumen sudah digunakan oleh registrasi lain." })),
            )
                .into_response());
        }
    }
    e.finish()?;

    // `update($validated)`: hanya kolom yang dikirim.
    let mut sets: Vec<&str> = Vec::new();
    if tanggal.is_some() {
        sets.push("tanggal = ?");
    }
    if nomor.is_some() {
        sets.push("nomor = ?");
    }
    if description.is_some() {
        sets.push("description = ?");
    }
    if nilai.is_some() || input.contains_key("nilai") {
        sets.push("nilai = ?");
    }
    if input.contains_key("addendum_id") {
        sets.push("addendum_id = ?");
    }
    if !sets.is_empty() {
        let sql = format!("UPDATE tbl_document_registers SET {}, updated_at = NOW() WHERE id = ?", sets.join(", "));
        let mut q = sqlx::query(&sql);
        if let Some(d) = tanggal {
            q = q.bind(d);
        }
        if let Some(n) = &nomor {
            q = q.bind(n);
        }
        if let Some(d) = &description {
            q = q.bind(d.clone());
        }
        if sets.iter().any(|s| s.starts_with("nilai")) {
            q = q.bind(nilai);
        }
        if input.contains_key("addendum_id") {
            q = q.bind(addendum);
        }
        q.bind(current.id).execute(&state.pool).await.map_err(internal)?;
    }
    let fresh = find(&state.pool, current.id).await?;
    Ok(Json(with_type(&state.pool, &fresh).await?).into_response())
}

/// `DELETE /api/document-registers/{id}`: setelah hapus, nomor urut tahun itu disetel ke maksimum sisa.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find(&state.pool, id).await?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_document_registers WHERE id = ?")
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let max: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(MAX(sequence_number) AS SIGNED) FROM tbl_document_registers WHERE year = ?",
    )
    .bind(row.year)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "INSERT INTO tbl_document_sequences (year, type, last_number) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE last_number = VALUES(last_number)",
    )
    .bind(row.year)
    .bind(SEQ_TYPE)
    .bind(max.unwrap_or(0))
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Register deleted" })).into_response())
}
