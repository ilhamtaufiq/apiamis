//! Master fase pekerjaan (`MasterFasePekerjaanController`). Tabel `master_fase_pekerjaans`.
//!
//! Respons `data` mengikuti model Eloquent: `durasi_faktor` float (`1.0`), `keywords` array JSON, dan
//! timestamp dalam format Carbon (`.000000Z`). Rute ini hanya `auth:sanctum`, tanpa `role:admin`.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    lookup::carbon_json,
    require_auth,
    validation::Errors,
    AppState,
};

const SELECT_FASE: &str = "SELECT CAST(id AS SIGNED) AS id, jenis_proyek, kode_fase, nama_fase, \
     CAST(prioritas AS SIGNED) AS prioritas, CAST(overlap_persen AS SIGNED) AS overlap_persen, \
     durasi_faktor, CAST(keywords AS CHAR) AS keywords, deskripsi, is_active, created_at, updated_at \
     FROM master_fase_pekerjaans";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

struct Fase {
    id: i64,
    jenis_proyek: String,
    kode_fase: String,
    nama_fase: String,
    prioritas: i64,
    overlap_persen: i64,
    durasi_faktor: f32,
    keywords: String,
    deskripsi: Option<String>,
    is_active: bool,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_fase(r: &sqlx::mysql::MySqlRow) -> Result<Fase, sqlx::Error> {
    Ok(Fase {
        id: r.try_get("id")?,
        jenis_proyek: r.try_get("jenis_proyek")?,
        kode_fase: r.try_get("kode_fase")?,
        nama_fase: r.try_get("nama_fase")?,
        prioritas: r.try_get("prioritas")?,
        overlap_persen: r.try_get("overlap_persen")?,
        durasi_faktor: r.try_get("durasi_faktor")?,
        keywords: r.try_get("keywords")?,
        deskripsi: r.try_get("deskripsi")?,
        is_active: r.try_get("is_active")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// `float` MySQL dibaca sebagai string PHP lalu dikirim sebagai float: `1.1`, bukan `1.100000023841858`.
fn float_json(v: f32) -> Value {
    let d: f64 = v.to_string().parse().unwrap_or(v as f64);
    serde_json::Number::from_f64(d)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn resource(f: &Fase) -> Value {
    json!({
        "id": f.id,
        "jenis_proyek": f.jenis_proyek,
        "kode_fase": f.kode_fase,
        "nama_fase": f.nama_fase,
        "prioritas": f.prioritas,
        "overlap_persen": f.overlap_persen,
        "durasi_faktor": float_json(f.durasi_faktor),
        "keywords": serde_json::from_str::<Value>(&f.keywords).unwrap_or(json!([])),
        "deskripsi": f.deskripsi,
        "is_active": f.is_active,
        "created_at": carbon_json(f.created_at),
        "updated_at": carbon_json(f.updated_at),
    })
}

async fn find(pool: &MySqlPool, id: i64) -> Result<Fase, ApiError> {
    let sql = format!("{SELECT_FASE} WHERE id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_fase(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// `filter_var(FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE)`: `None` bila nilai tidak dikenali.
fn php_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" | "" => Some(false),
        _ => None,
    }
}

/// `GET /api/master-fase-pekerjaan?jenis_proyek=&is_active=`, urut jenis lalu prioritas.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let mut clauses: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(j) = query.get("jenis_proyek").filter(|v| !v.is_empty()) {
        clauses.push("jenis_proyek = ?");
        binds.push(j.clone());
    }
    // `$request->has('is_active')` bernilai false untuk string kosong.
    if let Some(active) = query.get("is_active").filter(|v| !v.is_empty()).and_then(|v| php_bool(v)) {
        clauses.push("is_active = ?");
        binds.push(if active { "1" } else { "0" }.to_string());
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let sql = format!("{SELECT_FASE}{where_sql} ORDER BY jenis_proyek, prioritas");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(map_fase)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .iter()
        .map(resource)
        .collect();
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `GET /api/master-fase-pekerjaan/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let fase = find(&state.pool, id).await?;
    Ok(Json(json!({ "success": true, "data": resource(&fase) })).into_response())
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// Teks `string` dengan batas panjang. `None` bila kosong atau tidak valid (error sudah dicatat).
fn rule_string(e: &mut Errors, field: &str, v: Option<&Value>, max: usize) -> Option<String> {
    match v {
        Some(Value::String(s)) if s.chars().count() <= max => Some(s.clone()),
        Some(Value::String(_)) => {
            e.add(field, format!("The {} field must not be greater than {max} characters.", attr(field)));
            None
        }
        _ => {
            e.add(field, format!("The {} field must be a string.", attr(field)));
            None
        }
    }
}

/// `integer|min:N` (atau `max:N`). Menerima angka JSON atau string angka.
fn rule_int(e: &mut Errors, field: &str, v: Option<&Value>, min: i64, max: Option<i64>) -> Option<i64> {
    let parsed = match v {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    let a = attr(field);
    match parsed {
        None => {
            e.add(field, format!("The {a} field must be an integer."));
            None
        }
        Some(v) if v < min => {
            e.add(field, format!("The {a} field must be at least {min}."));
            None
        }
        Some(v) if max.is_some_and(|m| v > m) => {
            e.add(field, format!("The {a} field must not be greater than {}.", max.unwrap_or(0)));
            None
        }
        Some(v) => Some(v),
    }
}

/// `numeric|min:0.1`.
fn rule_numeric(e: &mut Errors, field: &str, v: Option<&Value>, min: f64) -> Option<f64> {
    let parsed = match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    };
    let a = attr(field);
    match parsed {
        None => {
            e.add(field, format!("The {a} field must be a number."));
            None
        }
        Some(v) if v < min => {
            e.add(field, format!("The {a} field must be at least {min}."));
            None
        }
        Some(v) => Some(v),
    }
}

/// `nullable|array` dengan item `string|max:100`.
fn rule_keywords(e: &mut Errors, v: &Value) -> Option<Vec<String>> {
    let Value::Array(items) = v else {
        e.add("keywords", "The keywords field must be an array.");
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::String(s) if s.chars().count() <= 100 => out.push(s.clone()),
            Value::String(_) => {
                e.add(&format!("keywords.{i}"), format!("The keywords.{i} field must not be greater than 100 characters."));
                ok = false;
            }
            _ => {
                e.add(&format!("keywords.{i}"), format!("The keywords.{i} field must be a string."));
                ok = false;
            }
        }
    }
    ok.then_some(out)
}

/// `nullable|boolean`.
fn rule_bool(e: &mut Errors, field: &str, v: &Value) -> Option<bool> {
    let parsed = match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "1" | "true" | "on" | "yes" => Some(true),
            "0" | "false" | "off" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    };
    if parsed.is_none() {
        e.add(field, format!("The {} field must be true or false.", attr(field)));
    }
    parsed
}

/// Unique `(jenis_proyek, kode_fase)`, dengan pengecualian id sendiri pada update.
async fn kode_taken(pool: &MySqlPool, jenis: &str, kode: &str, except: Option<i64>) -> Result<bool, ApiError> {
    let count: i64 = match except {
        Some(id) => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM master_fase_pekerjaans WHERE jenis_proyek = ? AND kode_fase = ? AND id <> ?",
        )
        .bind(jenis)
        .bind(kode)
        .bind(id)
        .fetch_one(pool)
        .await,
        None => sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM master_fase_pekerjaans WHERE jenis_proyek = ? AND kode_fase = ?",
        )
        .bind(jenis)
        .bind(kode)
        .fetch_one(pool)
        .await,
    }
    .map_err(internal)?;
    Ok(count > 0)
}

fn parse_body(body: &Bytes) -> Result<serde_json::Map<String, Value>, ApiError> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Ok(serde_json::Map::new()),
    }
}

/// `POST /api/master-fase-pekerjaan`: 201 dengan data yang dibaca ulang dari DB.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body)?;
    let get = |k: &str| input.get(k);
    let mut e = Errors::default();

    let jenis = match get("jenis_proyek") {
        None | Some(Value::Null) => {
            e.add("jenis_proyek", "The jenis proyek field is required.");
            None
        }
        v => rule_string(&mut e, "jenis_proyek", v, 50),
    };
    let kode = match get("kode_fase") {
        None | Some(Value::Null) => {
            e.add("kode_fase", "The kode fase field is required.");
            None
        }
        v => rule_string(&mut e, "kode_fase", v, 30),
    };
    match get("nama_fase") {
        None | Some(Value::Null) => e.add("nama_fase", "The nama fase field is required."),
        v => {
            rule_string(&mut e, "nama_fase", v, 100);
        }
    }
    let prioritas = match get("prioritas") {
        None | Some(Value::Null) => {
            e.add("prioritas", "The prioritas field is required.");
            None
        }
        v => rule_int(&mut e, "prioritas", v, 0, None),
    };
    let overlap = match get("overlap_persen") {
        None | Some(Value::Null) => None,
        v => rule_int(&mut e, "overlap_persen", v, 0, Some(100)),
    };
    let durasi = match get("durasi_faktor") {
        None | Some(Value::Null) => None,
        v => rule_numeric(&mut e, "durasi_faktor", v, 0.1),
    };
    let keywords = match get("keywords") {
        None | Some(Value::Null) => None,
        Some(v) => rule_keywords(&mut e, v),
    };
    if let Some(v) = get("deskripsi").filter(|v| !v.is_null()) {
        if !v.is_string() {
            e.add("deskripsi", "The deskripsi field must be a string.");
        }
    }
    let is_active = match get("is_active") {
        None | Some(Value::Null) => None,
        Some(v) => rule_bool(&mut e, "is_active", v),
    };

    // Unique di Laravel ikut diperiksa dalam validasi yang sama.
    if let (Some(j), Some(k)) = (&jenis, &kode) {
        if kode_taken(&state.pool, j, k, None).await? {
            e.add("kode_fase", "The kode fase has already been taken.");
        }
    }
    e.finish()?;

    let keywords: Vec<String> = keywords.unwrap_or_default();
    let deskripsi = get("deskripsi").and_then(Value::as_str).map(str::to_string);
    let result = sqlx::query(
        "INSERT INTO master_fase_pekerjaans (jenis_proyek, kode_fase, nama_fase, prioritas, overlap_persen, \
         durasi_faktor, keywords, deskripsi, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(jenis.unwrap_or_default())
    .bind(kode.unwrap_or_default())
    .bind(get("nama_fase").and_then(Value::as_str).unwrap_or_default())
    .bind(prioritas.unwrap_or(0))
    .bind(overlap.unwrap_or(0))
    .bind(durasi.unwrap_or(1.0))
    .bind(serde_json::to_string(&keywords).map_err(internal)?)
    .bind(deskripsi)
    .bind(is_active.unwrap_or(true))
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let fase = find(&state.pool, result.last_insert_id() as i64).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": resource(&fase) })),
    )
        .into_response())
}

/// `PUT` dan `PATCH /api/master-fase-pekerjaan/{id}`: hanya field yang dikirim yang diperbarui.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id).await?;
    let input = parse_body(&body)?;
    let mut e = Errors::default();

    // `sometimes`: hanya diperiksa bila key ada. Null tidak diterima untuk field yang tidak nullable.
    let mut set_jenis: Option<String> = None;
    let mut set_kode: Option<String> = None;
    let mut set_nama: Option<String> = None;
    let mut set_prioritas: Option<i64> = None;
    let mut set_overlap: Option<Option<i64>> = None;
    let mut set_durasi: Option<Option<f64>> = None;
    let mut set_keywords: Option<Vec<String>> = None;
    let mut set_deskripsi: Option<Option<String>> = None;
    let mut set_active: Option<bool> = None;

    if let Some(v) = input.get("jenis_proyek") {
        set_jenis = rule_string(&mut e, "jenis_proyek", Some(v), 50);
    }
    if let Some(v) = input.get("kode_fase") {
        set_kode = rule_string(&mut e, "kode_fase", Some(v), 30);
    }
    if let Some(v) = input.get("nama_fase") {
        set_nama = rule_string(&mut e, "nama_fase", Some(v), 100);
    }
    if let Some(v) = input.get("prioritas") {
        set_prioritas = rule_int(&mut e, "prioritas", Some(v), 0, None);
    }
    if let Some(v) = input.get("overlap_persen") {
        set_overlap = Some(if v.is_null() { None } else { rule_int(&mut e, "overlap_persen", Some(v), 0, Some(100)) });
    }
    if let Some(v) = input.get("durasi_faktor") {
        set_durasi = Some(if v.is_null() { None } else { rule_numeric(&mut e, "durasi_faktor", Some(v), 0.1) });
    }
    if let Some(v) = input.get("keywords") {
        set_keywords = if v.is_null() {
            Some(Vec::new())
        } else {
            rule_keywords(&mut e, v)
        };
    }
    if let Some(v) = input.get("deskripsi") {
        if v.is_null() {
            set_deskripsi = Some(None);
        } else if let Some(s) = v.as_str() {
            set_deskripsi = Some(Some(s.to_string()));
        } else {
            e.add("deskripsi", "The deskripsi field must be a string.");
        }
    }
    if let Some(v) = input.get("is_active") {
        set_active = if v.is_null() { None } else { rule_bool(&mut e, "is_active", v) };
    }

    // Unique (jenis_proyek, kode_fase) memakai jenis yang dikirim atau yang tersimpan.
    let jenis_final = set_jenis.clone().unwrap_or_else(|| current.jenis_proyek.clone());
    let kode_final = set_kode.clone().unwrap_or_else(|| current.kode_fase.clone());
    if (set_kode.is_some() || set_jenis.is_some())
        && kode_taken(&state.pool, &jenis_final, &kode_final, Some(current.id)).await?
    {
        e.add("kode_fase", "The kode fase has already been taken.");
    }
    e.finish()?;

    // Urutan bind harus sama dengan urutan kolom di `SET`.
    enum Bind {
        S(Option<String>),
        I(Option<i64>),
        F(Option<f64>),
        B(bool),
    }
    let mut ordered: Vec<(&str, Bind)> = Vec::new();
    if let Some(v) = set_jenis { ordered.push(("jenis_proyek = ?", Bind::S(Some(v)))); }
    if let Some(v) = set_kode { ordered.push(("kode_fase = ?", Bind::S(Some(v)))); }
    if let Some(v) = set_nama { ordered.push(("nama_fase = ?", Bind::S(Some(v)))); }
    if let Some(v) = set_prioritas { ordered.push(("prioritas = ?", Bind::I(Some(v)))); }
    if let Some(v) = set_overlap { ordered.push(("overlap_persen = ?", Bind::I(v))); }
    if let Some(v) = set_durasi { ordered.push(("durasi_faktor = ?", Bind::F(v))); }
    if let Some(v) = set_keywords {
        ordered.push(("keywords = ?", Bind::S(Some(serde_json::to_string(&v).map_err(internal)?))));
    }
    if let Some(v) = set_deskripsi { ordered.push(("deskripsi = ?", Bind::S(v))); }
    if let Some(v) = set_active { ordered.push(("is_active = ?", Bind::B(v))); }

    if !ordered.is_empty() {
        let set_sql: Vec<&str> = ordered.iter().map(|(s, _)| *s).collect();
        let sql = format!(
            "UPDATE master_fase_pekerjaans SET {}, updated_at = NOW() WHERE id = ?",
            set_sql.join(", ")
        );
        let mut q = sqlx::query(&sql);
        for (_, b) in &ordered {
            q = match b {
                Bind::S(v) => q.bind(v.clone()),
                Bind::I(v) => q.bind(*v),
                Bind::F(v) => q.bind(*v),
                Bind::B(v) => q.bind(*v),
            };
        }
        q.bind(current.id).execute(&state.pool).await.map_err(internal)?;
    }
    let fresh = find(&state.pool, current.id).await?;
    Ok(Json(json!({ "success": true, "data": resource(&fresh) })).into_response())
}

/// `DELETE /api/master-fase-pekerjaan/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    find(&state.pool, id).await?;
    sqlx::query("DELETE FROM master_fase_pekerjaans WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "success": true, "message": "Data deleted successfully" })).into_response())
}
