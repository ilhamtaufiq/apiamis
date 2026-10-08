//! Tulis SPM sanitasi: `POST /api/spm-sanitasi`, `PUT`/`PATCH`/`DELETE /api/spm-sanitasi/{id}`.
//!
//! Mengikuti `SpmSanitasiController::store|update|destroy` dan `validatePayload`. Model `SpmSanitasi`
//! memakai `Auditable` dan `NotifiesAdminsOnChanges`, jadi setiap perubahan menulis audit dan notifikasi
//! ke admin lain dalam transaksi yang sama (lewat `changes::log_linked`, tanpa tautan).
//!
//! Perbedaan kecil:
//! - Audit `created` memuat kolom yang dibaca ulang dari DB, termasuk default (flag integrasi 0). Laravel
//!   hanya memuat atribut yang diisi model.
//! - Audit `updated` memakai nilai sebelum dan sesudah dari DB, bukan nilai mentah `getRawOriginal()`.
//! - Untuk `update`, pemanggilan tanpa perubahan atribut tidak menulis apa pun (sama dengan Eloquent).

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{
    mysql::MySqlArguments, query::Query as SqlQuery, MySql, MySqlPool,
};

use crate::{
    changes::{self, Target},
    require_auth,
    spm_sanitasi::{find, internal, resource_of, Kind, COLUMNS, JENIS},
    validation::Errors,
    AppState,
};

const SPM_TARGET: Target = Target {
    model_type: "App\\Models\\SpmSanitasi",
    label: "SpmSanitasi",
    tab: "",
};

/// Kolom yang dicek `changedAny` untuk mengosongkan flag `pemanfaat_dari_integrasi`.
const PEMANFAAT_FIELDS: &[&str] = &["jumlah_pemanfaat_kk", "jumlah_pemanfaat_jiwa"];
/// Kolom yang dicek `changedAny` untuk mengosongkan flag `pembiayaan_dari_integrasi`.
const PEMBIAYAAN_FIELDS: &[&str] = &[
    "pembiayaan_apbn",
    "pembiayaan_apbd",
    "pembiayaan_dak",
    "pembiayaan_hibah",
    "pembiayaan_csr",
    "pembiayaan_lain",
    "pembiayaan_total",
];

/// Aturan satu kolom (semua `nullable` kecuali `nama_infrastruktur` dan `jenis`).
#[derive(Clone, Copy)]
enum Rule {
    /// `string|max:N`.
    Text(usize),
    /// `numeric|min:N` (`NEG_INFINITY` bila tanpa `min`).
    Num(f64),
    /// `integer|min:N|max:M`.
    Int(i64, Option<i64>),
}

/// Urutan aturan sama dengan `validatePayload`, karena pesan pertama yang dikembalikan.
const RULES: &[(&str, Rule)] = &[
    ("skala_pelayanan", Rule::Text(255)),
    ("latitude", Rule::Num(f64::NEG_INFINITY)),
    ("longitude", Rule::Num(f64::NEG_INFINITY)),
    ("alamat_lengkap", Rule::Text(usize::MAX)),
    ("jumlah_pemanfaat_kk", Rule::Int(0, None)),
    ("jumlah_pemanfaat_jiwa", Rule::Int(0, None)),
    ("tahun_konstruksi", Rule::Int(1900, Some(2100))),
    ("pembiayaan_apbn", Rule::Num(0.0)),
    ("pembiayaan_apbd", Rule::Num(0.0)),
    ("pembiayaan_dak", Rule::Num(0.0)),
    ("pembiayaan_hibah", Rule::Num(0.0)),
    ("pembiayaan_csr", Rule::Num(0.0)),
    ("pembiayaan_lain", Rule::Num(0.0)),
    ("pembiayaan_total", Rule::Num(0.0)),
    ("status_keberfungsian", Rule::Text(100)),
    ("kualitas_keberfungsian", Rule::Text(100)),
    ("pengelola", Rule::Text(255)),
    ("kapasitas_desain", Rule::Num(0.0)),
    ("kapasitas_terpakai", Rule::Num(0.0)),
    ("kapasitas_tidak_terpakai", Rule::Num(0.0)),
    ("jenis_pengolahan", Rule::Text(255)),
    ("peta_cakupan", Rule::Text(100)),
    ("status_lahan", Rule::Text(100)),
    ("luas_lahan_ha", Rule::Text(50)),
    ("opsi_teknologi", Rule::Text(255)),
    ("jumlah_stasiun_pompa", Rule::Text(50)),
    ("biaya_operasional", Rule::Num(0.0)),
    ("jenis_pengelola", Rule::Text(255)),
    ("sistem_pengolahan", Rule::Text(255)),
    ("truk_tinja_unit", Rule::Int(0, None)),
    ("kapasitas_truk_m3", Rule::Num(0.0)),
    ("jumlah_ritasi", Rule::Int(0, None)),
    ("jarak_maksimal_pelayanan_km", Rule::Num(0.0)),
    ("alokasi_biaya_operasional", Rule::Num(0.0)),
];

fn attr(name: &str) -> String {
    name.replace('_', " ")
}

/// Input setelah `TrimStrings` dan `ConvertEmptyStringsToNull`: teks dipangkas, kosong menjadi null.
fn norm(v: Option<&Value>) -> Option<Value> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let t = s.trim();
            (!t.is_empty()).then(|| Value::String(t.to_string()))
        }
        Some(other) => Some(other.clone()),
    }
}

/// `numeric`: angka JSON atau teks numerik PHP.
fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            let plain = !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'));
            if plain {
                t.parse::<f64>().ok().filter(|x| x.is_finite())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `integer`: angka JSON bulat atau teks bilangan bulat (tanpa nol di depan), seperti `FILTER_VALIDATE_INT`.
fn integer(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        Value::String(s) => {
            let digits = s.strip_prefix('+').or_else(|| s.strip_prefix('-')).unwrap_or(s);
            let ok = !digits.is_empty()
                && digits.chars().all(|c| c.is_ascii_digit())
                && (digits == "0" || !digits.starts_with('0'));
            if ok {
                s.parse::<i64>().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `boolean`: `true`, `false`, `1`, `0`, `"1"`, atau `"0"`.
fn boolean(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Terapkan satu aturan. Nilai null (setelah normalisasi) disimpan sebagai null tanpa cek lain.
fn check(e: &mut Errors, out: &mut Map<String, Value>, name: &str, raw: Option<Value>, rule: Rule) {
    let Some(v) = raw else {
        out.insert(name.to_string(), Value::Null);
        return;
    };
    let a = attr(name);
    match rule {
        Rule::Text(max) => match &v {
            Value::String(s) if s.chars().count() <= max => {
                out.insert(name.to_string(), v.clone());
            }
            Value::String(_) => e.add(
                name,
                format!("The {a} field must not be greater than {max} characters."),
            ),
            _ => e.add(name, format!("The {a} field must be a string.")),
        },
        Rule::Num(min) => match numeric(&v) {
            None => e.add(name, format!("The {a} field must be a number.")),
            Some(x) if x < min => e.add(name, format!("The {a} field must be at least {min}.")),
            Some(x) => {
                out.insert(name.to_string(), json!(x));
            }
        },
        Rule::Int(min, max) => match integer(&v) {
            None => e.add(name, format!("The {a} field must be an integer.")),
            Some(x) if x < min => e.add(name, format!("The {a} field must be at least {min}.")),
            Some(x) if max.is_some_and(|m| x > m) => e.add(
                name,
                format!("The {a} field must not be greater than {}.", max.unwrap_or(0)),
            ),
            Some(x) => {
                out.insert(name.to_string(), json!(x));
            }
        },
    }
}

/// Validasi `validatePayload` (dan `jenis` `required` saat membuat, `sometimes` saat mengubah).
async fn validate(
    pool: &MySqlPool,
    body: &Map<String, Value>,
    creating: bool,
) -> Result<Map<String, Value>, ApiError> {
    let mut e = Errors::default();
    let mut out = Map::new();

    if creating || body.contains_key("jenis") {
        match norm(body.get("jenis")) {
            None if creating => e.add("jenis", "The jenis field is required."),
            Some(Value::String(s)) if JENIS.contains(&s.as_str()) => {
                out.insert("jenis".into(), json!(s));
            }
            _ => e.add("jenis", "The selected jenis is invalid."),
        }
    }

    if body.contains_key("desa_id") {
        match norm(body.get("desa_id")) {
            None => {
                out.insert("desa_id".into(), Value::Null);
            }
            Some(v) => {
                let id = integer(&v);
                let exists = match id {
                    Some(id) => {
                        sqlx::query_scalar::<_, i64>(
                            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_desa WHERE id = ?",
                        )
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .map_err(internal)?
                            > 0
                    }
                    None => false,
                };
                match id {
                    Some(id) if exists => {
                        out.insert("desa_id".into(), json!(id));
                    }
                    _ => e.add("desa_id", "The selected desa id is invalid."),
                }
            }
        }
    }

    match norm(body.get("nama_infrastruktur")) {
        None => e.add("nama_infrastruktur", "The nama infrastruktur field is required."),
        Some(v) => check(&mut e, &mut out, "nama_infrastruktur", Some(v), Rule::Text(500)),
    }

    for (name, rule) in RULES {
        if body.contains_key(*name) {
            check(&mut e, &mut out, name, norm(body.get(*name)), *rule);
        }
    }

    e.finish()?;
    Ok(out)
}

/// `sometimes|boolean` untuk flag integrasi, hanya saat membuat (`store`).
fn validate_flags(body: &Map<String, Value>) -> Result<Map<String, Value>, ApiError> {
    let mut e = Errors::default();
    let mut out = Map::new();
    for name in ["pemanfaat_dari_integrasi", "pembiayaan_dari_integrasi"] {
        if body.contains_key(name) {
            match norm(body.get(name)).as_ref().and_then(boolean) {
                Some(b) => {
                    out.insert(name.to_string(), json!(b));
                }
                None => e.add(name, format!("The {} field must be true or false.", attr(name))),
            }
        }
    }
    e.finish()?;
    Ok(out)
}

fn parse_object(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// `changedAny`: bandingkan angka sebagai float; null sama dengan null.
fn any_changed(current: &Map<String, Value>, validated: &Map<String, Value>, fields: &[&str]) -> bool {
    fields.iter().any(|f| match validated.get(*f) {
        Some(after) => !same(Kind::Num, current.get(*f).unwrap_or(&Value::Null), after),
        None => false,
    })
}

/// Apakah nilai baru sama dengan atribut yang tersimpan, memakai tipe kolom.
fn same(kind: Kind, before: &Value, after: &Value) -> bool {
    match kind {
        Kind::Int => before.as_i64() == after.as_i64() && before.is_null() == after.is_null(),
        Kind::Num => match (before.as_f64(), after.as_f64()) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            _ => false,
        },
        Kind::Text => before.as_str() == after.as_str(),
        Kind::Bool => before.as_bool() == after.as_bool(),
        Kind::Time => before == after,
    }
}

/// Ikat satu nilai ke query menurut tipe kolom. Nilai yang bukan tipe kolom menjadi NULL.
fn bind_cell<'q>(
    q: SqlQuery<'q, MySql, MySqlArguments>,
    kind: Kind,
    v: &Value,
) -> SqlQuery<'q, MySql, MySqlArguments> {
    match kind {
        Kind::Int => q.bind(v.as_i64()),
        Kind::Num => q.bind(v.as_f64()),
        Kind::Text => q.bind(v.as_str().map(str::to_owned)),
        Kind::Bool => q.bind(v.as_bool()),
        Kind::Time => q,
    }
}

fn kind_of(name: &str) -> Kind {
    COLUMNS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, k)| *k)
        .unwrap_or(Kind::Text)
}

/// Kolom yang ditulis, dalam urutan tabel, tanpa id dan timestamp.
fn writable(validated: &Map<String, Value>) -> Vec<&'static str> {
    COLUMNS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !matches!(*n, "id" | "created_at" | "updated_at") && validated.contains_key(*n))
        .collect()
}

fn url_for(state: &AppState, suffix: &str) -> String {
    format!("{}/api/spm-sanitasi{suffix}", state.app_url.trim_end_matches('/'))
}

/// `POST /api/spm-sanitasi`: 201 dengan data beserta `desa.kecamatan`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_object(&body);
    let mut validated = validate(&state.pool, &input, true).await?;
    validated.extend(validate_flags(&input)?);

    let cols = writable(&validated);
    let placeholders = vec!["?"; cols.len()].join(", ");
    let sql = format!(
        "INSERT INTO tbl_spm_sanitasi ({}, created_at, updated_at) VALUES ({placeholders}, NOW(), NOW())",
        cols.join(", ")
    );
    let url = url_for(&state, "");

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut q = sqlx::query(&sql);
    for c in &cols {
        q = bind_cell(q, kind_of(c), &validated[*c]);
    }
    let res = q.execute(&mut *tx).await.map_err(internal)?;
    let id = res.last_insert_id() as i64;
    let attrs = find(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("spm sanitasi baru tidak terbaca"))?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &SPM_TARGET,
        "created",
        id,
        None,
        Some(attrs.clone()),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let data = resource_of(&state.pool, attrs).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "data": data,
            "message": "Data SPM Sanitasi berhasil ditambahkan",
        })),
    )
        .into_response())
}

/// `PUT` dan `PATCH /api/spm-sanitasi/{id}`. Model tidak disimpan bila tidak ada kolom yang berubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let input = parse_object(&body);
    let mut validated = validate(&state.pool, &input, false).await?;

    // Nilai yang diubah manual tidak lagi ditimpa oleh sinkronisasi paket tertaut.
    if any_changed(&current, &validated, PEMANFAAT_FIELDS) {
        validated.insert("pemanfaat_dari_integrasi".into(), json!(false));
    }
    if any_changed(&current, &validated, PEMBIAYAAN_FIELDS) {
        validated.insert("pembiayaan_dari_integrasi".into(), json!(false));
    }

    let dirty: Vec<&'static str> = writable(&validated)
        .into_iter()
        .filter(|n| {
            let before = current.get(*n).unwrap_or(&Value::Null);
            !same(kind_of(n), before, &validated[*n])
        })
        .collect();

    if dirty.is_empty() {
        let data = resource_of(&state.pool, current).await?;
        return Ok(Json(json!({
            "success": true,
            "data": data,
            "message": "Data SPM Sanitasi berhasil diperbarui",
        }))
        .into_response());
    }

    let url = url_for(&state, &format!("/{id}"));
    let sets: Vec<String> = dirty.iter().map(|n| format!("{n} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_spm_sanitasi SET {}, updated_at = NOW() WHERE id = ?",
        sets.join(", ")
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut q = sqlx::query(&sql);
    for n in &dirty {
        q = bind_cell(q, kind_of(n), &validated[*n]);
    }
    q.bind(id).execute(&mut *tx).await.map_err(internal)?;

    let after = find(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let mut old = Map::new();
    let mut new = Map::new();
    for n in dirty.iter().chain(std::iter::once(&"updated_at")) {
        old.insert((*n).to_string(), current.get(*n).cloned().unwrap_or(Value::Null));
        new.insert((*n).to_string(), after.get(*n).cloned().unwrap_or(Value::Null));
    }
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &SPM_TARGET,
        "updated",
        id,
        Some(old),
        Some(new),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let data = resource_of(&state.pool, after).await?;
    Ok(Json(json!({
        "success": true,
        "data": data,
        "message": "Data SPM Sanitasi berhasil diperbarui",
    }))
    .into_response())
}

/// `DELETE /api/spm-sanitasi/{id}`. Pivot pekerjaan ikut terhapus lewat FK `ON DELETE CASCADE`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let url = url_for(&state, &format!("/{id}"));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &SPM_TARGET,
        "deleted",
        id,
        Some(current),
        None,
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({
        "success": true,
        "message": "Data SPM Sanitasi berhasil dihapus",
    }))
    .into_response())
}
