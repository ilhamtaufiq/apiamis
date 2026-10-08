//! Unit SPAM (`SpamUnitController`, tabel `tbl_unit_spam`, `tbl_pengelola`, `tbl_spam_achievements`,
//! `tbl_spam_budgets`, `tbl_unit_spam_pekerjaan`, `tbl_unit_checklists`).
//!
//! Layanan integrasi pekerjaan ada di `spam_integration`. Rute yang TIDAK dipindah:
//! - `POST /api/spam-units/import` (`spam:import-data`, mengimpor CSV lewat Artisan).
//!
//! Catatan paritas:
//! - Binding model `{spamUnit}` dan `{unitSpam}` dijalankan sebelum auth (404 lebih dulu), seperti
//!   pola `desa_profile`. `{id}` pada show/update/destroy juga 404 sebelum auth.
//! - Setiap `save()` yang mengubah baris menulis audit dan notifikasi admin (`Auditable`,
//!   `NotifiesAdminsOnChanges`). Hapus massal tidak memicu event.
//! - `GET /api/public/*` tanpa token: `byUserRole()` menghasilkan `1 = 0`, sama dengan Laravel.
//! - `stats/series` dan `public/.../map-stats/series` memakai tahun dari `years` (4 digit, unik, maks 20).

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlConnection, MySqlPool, Row, Transaction};

use crate::{
    desa_profile::{raw_model, Cast},
    format::number_like_php,
    foto::base_url,
    pagination::{self},
    require_auth,
    spam_integration::{
        self as si, Ctx, ModelChange, B, MODEL_ACHIEVEMENT, MODEL_BUDGET, MODEL_UNIT, SUMBER_MANUAL,
    },
    validation::Errors,
    AppState,
};

const MODEL_PENGELOLA: &str = "App\\Models\\Pengelola";

const UNIT_CASTS: &[(&str, Cast)] = &[("is_simspam", Cast::Bool)];
const DESA_CASTS: &[(&str, Cast)] = &[
    ("luas", Cast::Float),
    ("jumlah_penduduk", Cast::Int),
    ("jumlah_kk", Cast::Int),
    ("target", Cast::Int),
    ("bjp_master", Cast::Int),
];
const PEKERJAAN_CASTS: &[(&str, Cast)] = &[
    ("pagu", Cast::Float),
    ("is_konsultan", Cast::Bool),
    ("kecamatan_id", Cast::Int),
    ("desa_id", Cast::Int),
    ("kegiatan_id", Cast::Int),
    ("pengawas_id", Cast::Int),
    ("pendamping_id", Cast::Int),
];
const OUTPUT_CASTS: &[(&str, Cast)] = &[
    ("pekerjaan_id", Cast::Int),
    ("penerima_is_optional", Cast::Bool),
];
const KONTRAK_CASTS: &[(&str, Cast)] = &[
    ("id_kegiatan", Cast::Int),
    ("id_pekerjaan", Cast::Int),
    ("id_penyedia", Cast::Int),
    ("nilai_kontrak", Cast::Float),
];
const BUDGET_CASTS: &[(&str, Cast)] =
    &[("unit_spam_id", Cast::Int), ("nilai_kontrak", Cast::Float)];
const CHECKLIST_CASTS: &[(&str, Cast)] = &[("is_checked", Cast::Bool)];

const PENGELOLA_FIELDS: &[&str] = &["pokmas", "perdes", "kepala", "bendahara", "sekretaris"];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

fn sql_err(e: sqlx::Error) -> ApiError {
    internal(e)
}

/// Konteks untuk pemanggil yang login.
fn ctx_for<'a>(
    user_id: u64,
    roles: &'a [(u64, String)],
    url: &'a str,
    headers: &'a HeaderMap,
) -> Ctx<'a> {
    Ctx {
        user: Some(user_id),
        roles,
        url,
        headers,
    }
}

/// Konteks tamu untuk rute publik.
fn guest_ctx<'a>(url: &'a str, headers: &'a HeaderMap) -> Ctx<'a> {
    Ctx {
        user: None,
        roles: &[],
        url,
        headers,
    }
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

/// `nullable|string|max:N`. Mengembalikan `Some(Some(v))` (ada), `Some(None)` (null), `None` (tidak dikirim).
fn opt_string(
    e: &mut Errors,
    field: &str,
    v: Option<&Value>,
    max: usize,
) -> Option<Option<String>> {
    match v {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) if s.chars().count() <= max => Some(Some(s.clone())),
        Some(Value::String(_)) => {
            e.add(
                field,
                format!(
                    "The {} field must not be greater than {max} characters.",
                    attr(field)
                ),
            );
            None
        }
        Some(_) => {
            e.add(
                field,
                format!("The {} field must be a string.", attr(field)),
            );
            None
        }
    }
}

/// `required|string|max:N`.
fn req_string(e: &mut Errors, field: &str, v: Option<&Value>, max: usize) -> Option<String> {
    match v {
        None | Some(Value::Null) => {
            e.add(field, format!("The {} field is required.", attr(field)));
            None
        }
        Some(Value::String(s)) if s.chars().count() <= max => Some(s.clone()),
        Some(Value::String(_)) => {
            e.add(
                field,
                format!(
                    "The {} field must not be greater than {max} characters.",
                    attr(field)
                ),
            );
            None
        }
        Some(_) => {
            e.add(
                field,
                format!("The {} field must be a string.", attr(field)),
            );
            None
        }
    }
}

/// Angka bulat dari JSON (angka atau string angka, seperti `FILTER_VALIDATE_INT`).
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => {
            let t = s.trim();
            let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                t.parse::<i64>().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `required|integer|min:N` atau `nullable|integer|min:N`.
fn int_rule(
    e: &mut Errors,
    field: &str,
    v: Option<&Value>,
    min: i64,
    required: bool,
) -> Option<i64> {
    match v {
        None | Some(Value::Null) => {
            if required {
                e.add(field, format!("The {} field is required.", attr(field)));
            }
            None
        }
        Some(v) => match as_int(v) {
            None => {
                e.add(
                    field,
                    format!("The {} field must be an integer.", attr(field)),
                );
                None
            }
            Some(n) if n < min => {
                e.add(
                    field,
                    format!("The {} field must be at least {min}.", attr(field)),
                );
                None
            }
            Some(n) => Some(n),
        },
    }
}

/// `numeric|min:N`.
fn numeric_rule(e: &mut Errors, field: &str, v: Option<&Value>, min: f64) -> Option<f64> {
    let parsed = match v {
        None | Some(Value::Null) => {
            e.add(field, format!("The {} field is required.", attr(field)));
            return None;
        }
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    };
    match parsed {
        None => {
            e.add(
                field,
                format!("The {} field must be a number.", attr(field)),
            );
            None
        }
        Some(x) if x < min => {
            e.add(
                field,
                format!("The {} field must be at least {min}.", attr(field)),
            );
            None
        }
        Some(x) => Some(x),
    }
}

/// `boolean` Laravel: true/false, 1/0, "1"/"0".
fn bool_rule(e: &mut Errors, field: &str, v: Option<&Value>, required: bool) -> Option<bool> {
    let parsed = match v {
        None | Some(Value::Null) => {
            if required {
                e.add(field, format!("The {} field is required.", attr(field)));
            }
            return None;
        }
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Some(Value::String(s)) => match s.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        },
        _ => None,
    };
    if parsed.is_none() {
        e.add(
            field,
            format!("The {} field must be true or false.", attr(field)),
        );
    }
    parsed
}

/// `exists:tbl_desa,id` untuk `desa_id` yang wajib.
async fn check_desa_exists(
    pool: &MySqlPool,
    e: &mut Errors,
    v: Option<&Value>,
) -> Result<Option<i64>, ApiError> {
    let Some(id) = v.and_then(as_int) else {
        if v.is_none() || v == Some(&Value::Null) {
            e.add("desa_id", "The desa id field is required.");
        } else {
            e.add("desa_id", "The selected desa id is invalid.");
        }
        return Ok(None);
    };
    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_desa WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(sql_err)?;
    if n == 0 {
        e.add("desa_id", "The selected desa id is invalid.");
        return Ok(None);
    }
    Ok(Some(id))
}

/// Field unit dan pengelola dari body, dengan validasi `store` atau `update`.
struct UnitInput {
    /// Field unit yang dikirim (termasuk null), urut `UNIT_FIELDS`.
    unit: Vec<(&'static str, Value)>,
    /// Field pengelola, selalu lima kolom (absen = null).
    pengelola: [Option<String>; 5],
}

async fn validate_unit(pool: &MySqlPool, body: &Map<String, Value>) -> Result<UnitInput, ApiError> {
    let mut e = Errors::default();
    let g = |k: &str| body.get(k);

    let desa = check_desa_exists(pool, &mut e, g("desa_id")).await?;
    let name = opt_string(&mut e, "name", g("name"), 255);
    let is_simspam = bool_rule(&mut e, "is_simspam", g("is_simspam"), true);
    let mut text: Vec<(&'static str, Option<Option<String>>)> = Vec::new();
    text.push((
        "sistem_layanan",
        opt_string(&mut e, "sistem_layanan", g("sistem_layanan"), 255),
    ));
    text.push((
        "sumber_mata_air_kap",
        opt_string(&mut e, "sumber_mata_air_kap", g("sumber_mata_air_kap"), 255),
    ));
    text.push((
        "sumber_air_tanah_kap",
        opt_string(
            &mut e,
            "sumber_air_tanah_kap",
            g("sumber_air_tanah_kap"),
            255,
        ),
    ));
    text.push((
        "lain_lain_kap",
        opt_string(&mut e, "lain_lain_kap", g("lain_lain_kap"), 255),
    ));
    text.push((
        "tahun_pembangunan",
        opt_string(&mut e, "tahun_pembangunan", g("tahun_pembangunan"), 10),
    ));
    text.push((
        "sumber_dana",
        opt_string(&mut e, "sumber_dana", g("sumber_dana"), 255),
    ));
    text.push(("program", opt_string(&mut e, "program", g("program"), 255)));
    text.push((
        "tarif_dasar_hukum",
        opt_string(&mut e, "tarif_dasar_hukum", g("tarif_dasar_hukum"), 255),
    ));
    text.push((
        "iuran_nominal",
        opt_string(&mut e, "iuran_nominal", g("iuran_nominal"), 255),
    ));
    text.push((
        "pendapatan_bulan",
        opt_string(&mut e, "pendapatan_bulan", g("pendapatan_bulan"), 255),
    ));
    text.push((
        "biaya_operasional",
        opt_string(&mut e, "biaya_operasional", g("biaya_operasional"), 255),
    ));

    let mut pengelola: [Option<String>; 5] = Default::default();
    for (i, f) in PENGELOLA_FIELDS.iter().enumerate() {
        let max = 255;
        if let Some(v) = opt_string(&mut e, f, g(f), max).flatten() {
            pengelola[i] = Some(v);
        }
    }
    e.finish()?;

    // Urutan field mengikuti aturan validasi; hanya field yang dikirim yang masuk.
    let mut unit: Vec<(&'static str, Value)> = Vec::new();
    if let Some(d) = desa {
        unit.push(("desa_id", json!(d)));
    }
    if let Some(v) = name {
        unit.push(("name", json!(v)));
    }
    if let Some(b) = is_simspam {
        unit.push(("is_simspam", json!(b)));
    }
    for (field, v) in text {
        if let Some(v) = v {
            unit.push((field, json!(v)));
        }
    }
    Ok(UnitInput { unit, pengelola })
}

// ---------------------------------------------------------------------------
// Pembacaan (bentuk JSON model)
// ---------------------------------------------------------------------------

async fn raw_one(
    c: &mut MySqlConnection,
    sql: &str,
    id: i64,
    casts: &[(&str, Cast)],
) -> Result<Option<Value>, ApiError> {
    let row = sqlx::query(sql)
        .bind(id)
        .fetch_optional(c)
        .await
        .map_err(sql_err)?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some(raw_model(&r, casts)?)),
    }
}

async fn raw_many(
    c: &mut MySqlConnection,
    sql: &str,
    binds: &[i64],
    casts: &[(&str, Cast)],
) -> Result<Vec<Value>, ApiError> {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    let rows = q.fetch_all(c).await.map_err(sql_err)?;
    rows.iter().map(|r| raw_model(r, casts)).collect()
}

async fn unit_raw(c: &mut MySqlConnection, id: i64) -> Result<Option<Value>, ApiError> {
    raw_one(
        c,
        "SELECT * FROM tbl_unit_spam WHERE id = ?",
        id,
        UNIT_CASTS,
    )
    .await
}

async fn desa_with_kecamatan(c: &mut MySqlConnection, desa_id: i64) -> Result<Value, ApiError> {
    let desa = raw_one(
        c,
        "SELECT * FROM tbl_desa WHERE id = ?",
        desa_id,
        DESA_CASTS,
    )
    .await?;
    let Some(mut desa) = desa else {
        return Ok(Value::Null);
    };
    let kec_id = desa.get("kecamatan_id").and_then(Value::as_i64);
    let kec = match kec_id {
        Some(k) => raw_one(c, "SELECT * FROM tbl_kecamatan WHERE id = ?", k, &[])
            .await?
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    if let Value::Object(m) = &mut desa {
        m.insert("kecamatan".into(), kec);
    }
    Ok(desa)
}

async fn pengelola_of(c: &mut MySqlConnection, unit_id: i64) -> Result<Value, ApiError> {
    Ok(raw_one(
        c,
        "SELECT * FROM tbl_pengelola WHERE unit_spam_id = ?",
        unit_id,
        &[],
    )
    .await?
    .unwrap_or(Value::Null))
}

async fn budgets_of(c: &mut MySqlConnection, unit_id: i64) -> Result<Vec<Value>, ApiError> {
    raw_many(
        c,
        "SELECT * FROM tbl_spam_budgets WHERE unit_spam_id = ? ORDER BY tahun DESC, id DESC",
        &[unit_id],
        BUDGET_CASTS,
    )
    .await
}

async fn achievements_of(c: &mut MySqlConnection, unit_id: i64) -> Result<Vec<Value>, ApiError> {
    raw_many(
        c,
        "SELECT * FROM tbl_spam_achievements WHERE unit_spam_id = ? ORDER BY tahun DESC, id DESC",
        &[unit_id],
        &[],
    )
    .await
}

async fn checklists_of(c: &mut MySqlConnection, unit_id: i64) -> Result<Vec<Value>, ApiError> {
    raw_many(
        c,
        "SELECT * FROM tbl_unit_checklists WHERE unit_spam_id = ? ORDER BY id",
        &[unit_id],
        CHECKLIST_CASTS,
    )
    .await
}

/// Pekerjaan tertaut dengan pivot. `full` = relasi kegiatan, output, dan kontrak lengkap.
async fn linked_pekerjaan(
    c: &mut MySqlConnection,
    unit_id: i64,
    full: bool,
) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(p.id AS SIGNED) AS id, up.id AS pivot_row_id FROM tbl_unit_spam_pekerjaan up JOIN tbl_pekerjaan p ON p.id = up.pekerjaan_id \
         WHERE up.unit_spam_id = ? ORDER BY p.id",
    )
    .bind(unit_id)
    .fetch_all(&mut *c)
    .await
    .map_err(sql_err)?;
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
        .collect();
    let mut out = Vec::new();
    for pid in ids {
        let Some(mut p) = raw_one(
            c,
            "SELECT * FROM tbl_pekerjaan WHERE id = ?",
            pid,
            PEKERJAAN_CASTS,
        )
        .await?
        else {
            continue;
        };
        let pivot = sqlx::query(
            "SELECT unit_spam_id, pekerjaan_id, output_id, capaian_metric, created_at, updated_at \
             FROM tbl_unit_spam_pekerjaan WHERE unit_spam_id = ? AND pekerjaan_id = ?",
        )
        .bind(unit_id)
        .bind(pid)
        .fetch_optional(&mut *c)
        .await
        .map_err(sql_err)?;
        let pivot_json = match pivot {
            Some(r) => raw_model(&r, &[])?,
            None => Value::Null,
        };

        let kegiatan_id = p.get("kegiatan_id").and_then(Value::as_i64);
        let kegiatan = match kegiatan_id {
            None => Value::Null,
            Some(kid) if full => raw_one(c, "SELECT * FROM tbl_kegiatan WHERE id = ?", kid, &[]).await?.unwrap_or(Value::Null),
            Some(kid) => match raw_one(
                c,
                "SELECT id, nama_sub_kegiatan, nama_kegiatan, nama_program, tahun_anggaran, sumber_dana FROM tbl_kegiatan WHERE id = ?",
                kid,
                &[],
            )
            .await?
            {
                Some(v) => v,
                None => Value::Null,
            },
        };
        if let Value::Object(m) = &mut p {
            m.insert("kegiatan".into(), kegiatan);
            m.insert("pivot".into(), pivot_json);
            if full {
                let outputs = raw_many(
                    c,
                    "SELECT * FROM tbl_output WHERE pekerjaan_id = ? ORDER BY id",
                    &[pid],
                    OUTPUT_CASTS,
                )
                .await?;
                let kontrak = raw_many(
                    c,
                    "SELECT kt.* FROM kontrak_pekerjaan kp JOIN tbl_kontrak kt ON kt.id = kp.kontrak_id WHERE kp.pekerjaan_id = ? ORDER BY kt.id",
                    &[pid],
                    KONTRAK_CASTS,
                )
                .await?;
                m.insert("output".into(), Value::Array(outputs));
                m.insert("kontrak".into(), Value::Array(kontrak));
            }
        }
        out.push(p);
    }
    Ok(out)
}

/// Unit dengan relasi untuk `index` (`full = false`) atau `show` (`full = true`, plus checklist).
async fn unit_with_relations(
    c: &mut MySqlConnection,
    unit_id: i64,
    show: bool,
) -> Result<Option<Value>, ApiError> {
    let Some(mut unit) = unit_raw(c, unit_id).await? else {
        return Ok(None);
    };
    let desa_id = unit.get("desa_id").and_then(Value::as_i64).unwrap_or(0);
    let desa = desa_with_kecamatan(c, desa_id).await?;
    let pengelola = pengelola_of(c, unit_id).await?;
    let budgets = budgets_of(c, unit_id).await?;
    let achievements = achievements_of(c, unit_id).await?;
    let pekerjaan = linked_pekerjaan(c, unit_id, show).await?;
    if let Value::Object(m) = &mut unit {
        m.insert("desa".into(), desa);
        m.insert("pengelola".into(), pengelola);
        m.insert("budgets".into(), Value::Array(budgets));
        m.insert("achievements".into(), Value::Array(achievements));
        m.insert("pekerjaan".into(), Value::Array(pekerjaan));
        if show {
            let checklists = checklists_of(c, unit_id).await?;
            m.insert("checklists".into(), Value::Array(checklists));
        }
    }
    Ok(Some(unit))
}

/// Unit dengan `pengelola` saja (respons `store` dan `update`).
async fn unit_with_pengelola(c: &mut MySqlConnection, unit_id: i64) -> Result<Value, ApiError> {
    let mut unit = unit_raw(c, unit_id).await?.unwrap_or(Value::Null);
    let pengelola = pengelola_of(c, unit_id).await?;
    if let Value::Object(m) = &mut unit {
        m.insert("pengelola".into(), pengelola);
    }
    Ok(unit)
}

// ---------------------------------------------------------------------------
// Perubahan model dengan audit dan notifikasi
// ---------------------------------------------------------------------------

fn map_of(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// Nilai kolom yang berbeda, seperti `getDirty()` dibanding nilai asli (perbandingan longgar angka/string).
fn dirty(old: &Value, new: &Value) -> bool {
    match (old, new) {
        (Value::Null, Value::Null) => false,
        (Value::Null, _) | (_, Value::Null) => true,
        (a, b) => {
            let norm = |v: &Value| -> String {
                match v {
                    Value::Bool(true) => "1".into(),
                    Value::Bool(false) => "0".into(),
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }
            };
            norm(a) != norm(b)
        }
    }
}

/// Data unit: kolom yang diisi di `fields` (urut) dari `input`, untuk audit lama/baru.
async fn update_unit_row(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    unit_id: i64,
    fields: &[(&'static str, Value)],
) -> Result<(), ApiError> {
    let current = unit_raw(&mut **tx, unit_id).await?.unwrap_or(Value::Null);
    let cur = map_of(current);
    let mut changed_old = Map::new();
    let mut changed_new = Map::new();
    for (k, v) in fields {
        let before = cur.get(*k).cloned().unwrap_or(Value::Null);
        if dirty(&before, v) {
            changed_old.insert((*k).to_string(), before);
            changed_new.insert((*k).to_string(), v.clone());
        }
    }
    if changed_new.is_empty() {
        return Ok(());
    }
    let sets: Vec<String> = changed_new.keys().map(|k| format!("{k} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_unit_spam SET {}, updated_at = NOW() WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for k in changed_new.keys() {
        q = bind_json(q, &changed_new[k]);
    }
    q.bind(unit_id).execute(&mut **tx).await.map_err(sql_err)?;
    si::log_model_change(
        tx,
        ctx,
        actor,
        ModelChange {
            event: "updated",
            model: MODEL_UNIT,
            id: unit_id as u64,
            old: Some(changed_old),
            new: Some(changed_new),
        },
    )
    .await
}

fn bind_json<'q>(
    q: sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments>,
    v: &Value,
) -> sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments> {
    match v {
        Value::Null => q.bind(None::<String>),
        Value::Bool(b) => q.bind(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => q.bind(i),
            None => q.bind(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// CRUD unit
// ---------------------------------------------------------------------------

/// Nilai SQL `LIKE` untuk search. Tidak di-escape, sama dengan Laravel.
fn like_term(s: &str) -> String {
    format!("%{s}%")
}

fn truthy_param(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `FILTER_VALIDATE_BOOLEAN`: true untuk 1/true/on/yes, selain itu false.
fn filter_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// `GET /api/spam-units`: paginator dengan `success`, `data`, dan `meta` ringkas.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut where_sql = String::from(" WHERE 1 = 1");
    let mut binds: Vec<B> = Vec::new();
    if let Some(kec) = truthy_param(query.get("kecamatan_id")) {
        where_sql.push_str(
            " AND EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = u.desa_id AND d.kecamatan_id = ?)",
        );
        binds.push(B::I(si::php_int(&kec)));
    }
    if let Some(desa) = truthy_param(query.get("desa_id")) {
        where_sql.push_str(" AND u.desa_id = ?");
        binds.push(B::I(si::php_int(&desa)));
    }
    if let Some(raw) = query.get("is_simspam").filter(|v| !v.is_empty()) {
        where_sql.push_str(" AND u.is_simspam = ?");
        binds.push(B::I(i64::from(filter_bool(raw))));
    }
    if let Some(search) = truthy_param(query.get("search")) {
        let term = like_term(&search);
        where_sql.push_str(
            " AND (u.name LIKE ? OR u.sistem_layanan LIKE ? OR u.program LIKE ? OR u.sumber_dana LIKE ? OR u.tahun_pembangunan LIKE ? \
             OR EXISTS (SELECT 1 FROM tbl_desa dq WHERE dq.id = u.desa_id AND dq.n_desa LIKE ?) \
             OR EXISTS (SELECT 1 FROM tbl_pengelola pq WHERE pq.unit_spam_id = u.id AND (pq.pokmas LIKE ? OR pq.kepala LIKE ? OR pq.perdes LIKE ?)))",
        );
        for _ in 0..9 {
            binds.push(B::S(term.clone()));
        }
    }

    let per_raw = query.get("per_page").map(String::as_str).unwrap_or("15");
    let per_page = si::php_int(per_raw).max(1) as u64;
    let params = pagination::page_params(&query);

    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_unit_spam u{where_sql}");
    let total = sum_with(&mut conn, &count_sql, &binds).await?;

    let list_sql = format!(
        "SELECT CAST(u.id AS SIGNED) AS id FROM tbl_unit_spam u{where_sql} ORDER BY u.id LIMIT {per_page} OFFSET {}",
        (params.page - 1) * per_page
    );
    let mut lq = sqlx::query(&list_sql);
    for b in &binds {
        lq = match b {
            B::S(v) => lq.bind(v.clone()),
            B::I(v) => lq.bind(*v),
            B::F(v) => lq.bind(*v),
        };
    }
    let ids: Vec<i64> = lq
        .fetch_all(&mut *conn)
        .await
        .map_err(sql_err)?
        .iter()
        .map(|r| r.try_get::<i64, _>("id").unwrap_or(0))
        .collect();

    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = unit_with_relations(&mut conn, id, false).await? {
            data.push(v);
        }
    }
    let last_page = std::cmp::max(1, (total as u64).div_ceil(per_page));
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": {
            "current_page": params.page,
            "last_page": last_page,
            "per_page": per_page,
            "total": total,
        }
    }))
    .into_response())
}

/// `GET /api/spam-units/{id}`: binding 404 dulu, lalu auth.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let Some(unit) = unit_with_relations(&mut conn, id, true).await? else {
        return Err(ApiError::not_found());
    };
    require_auth(&state, &headers).await?;
    Ok(Json(json!({ "success": true, "data": unit })).into_response())
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>().map_err(|_| ApiError::not_found())
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// `POST /api/spam-units`: 201.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = validate_unit(&state.pool, &parse_body(&body)).await?;
    let url = format!("{}/api/spam-units", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    let mut cols: Vec<&str> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();
    for (k, v) in &input.unit {
        cols.push(k);
        vals.push(v.clone());
    }
    let sql = format!(
        "INSERT INTO tbl_unit_spam ({}, created_at, updated_at) VALUES ({}, NOW(), NOW())",
        cols.join(", "),
        vec!["?"; cols.len()].join(", ")
    );
    let mut q = sqlx::query(&sql);
    for v in &vals {
        q = bind_json(q, v);
    }
    let unit_id = q.execute(&mut *tx).await.map_err(sql_err)?.last_insert_id() as i64;
    let mut new_map = map_of(json!({ "id": unit_id }));
    for (k, v) in &input.unit {
        new_map.insert((*k).to_string(), v.clone());
    }
    si::log_model_change(
        &mut tx,
        &ctx,
        user.user_id,
        ModelChange {
            event: "created",
            model: MODEL_UNIT,
            id: unit_id as u64,
            old: None,
            new: Some(new_map),
        },
    )
    .await?;

    insert_pengelola(&mut tx, &ctx, user.user_id, unit_id, &input.pengelola).await?;
    tx.commit().await.map_err(sql_err)?;

    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let data = unit_with_pengelola(&mut conn, unit_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": data, "message": "Unit SPAM berhasil ditambahkan" })),
    )
        .into_response())
}

async fn insert_pengelola(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    unit_id: i64,
    p: &[Option<String>; 5],
) -> Result<(), ApiError> {
    let res = sqlx::query(
        "INSERT INTO tbl_pengelola (unit_spam_id, pokmas, perdes, kepala, bendahara, sekretaris, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(unit_id)
    .bind(&p[0])
    .bind(&p[1])
    .bind(&p[2])
    .bind(&p[3])
    .bind(&p[4])
    .execute(&mut **tx)
    .await
    .map_err(sql_err)?;
    let id = res.last_insert_id();
    let new_map = map_of(json!({
        "id": id,
        "unit_spam_id": unit_id,
        "pokmas": p[0],
        "perdes": p[1],
        "kepala": p[2],
        "bendahara": p[3],
        "sekretaris": p[4],
    }));
    si::log_model_change(
        tx,
        ctx,
        actor,
        ModelChange {
            event: "created",
            model: MODEL_PENGELOLA,
            id,
            old: None,
            new: Some(new_map),
        },
    )
    .await
}

/// `PUT` dan `PATCH /api/spam-units/{id}`: binding 404 dulu, lalu auth.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let unit_id = parse_id(&id)?;
    {
        let mut conn = state.pool.acquire().await.map_err(sql_err)?;
        if unit_raw(&mut conn, unit_id).await?.is_none() {
            return Err(ApiError::not_found());
        }
    }
    let user = require_auth(&state, &headers).await?;
    let input = validate_unit(&state.pool, &parse_body(&body)).await?;
    let url = format!("{}/api/spam-units/{unit_id}", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    update_unit_row(&mut tx, &ctx, user.user_id, unit_id, &input.unit).await?;

    let existing: Option<i64> =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_pengelola WHERE unit_spam_id = ?")
            .bind(unit_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_err)?;
    match existing {
        Some(pid) => {
            let cur: Option<(Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT pokmas, perdes, kepala, bendahara, sekretaris FROM tbl_pengelola WHERE id = ?",
            )
            .bind(pid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_err)?;
            let cur = cur.unwrap_or_default();
            let before = [cur.0, cur.1, cur.2, cur.3, cur.4];
            let mut old = Map::new();
            let mut new = Map::new();
            for (i, f) in PENGELOLA_FIELDS.iter().enumerate() {
                let b = json!(before[i]);
                let a = json!(input.pengelola[i]);
                if dirty(&b, &a) {
                    old.insert((*f).to_string(), b);
                    new.insert((*f).to_string(), a);
                }
            }
            if !new.is_empty() {
                sqlx::query(
                    "UPDATE tbl_pengelola SET pokmas = ?, perdes = ?, kepala = ?, bendahara = ?, sekretaris = ?, updated_at = NOW() WHERE id = ?",
                )
                .bind(&input.pengelola[0])
                .bind(&input.pengelola[1])
                .bind(&input.pengelola[2])
                .bind(&input.pengelola[3])
                .bind(&input.pengelola[4])
                .bind(pid)
                .execute(&mut *tx)
                .await
                .map_err(sql_err)?;
                si::log_model_change(
                    &mut tx,
                    &ctx,
                    user.user_id,
                    ModelChange {
                        event: "updated",
                        model: MODEL_PENGELOLA,
                        id: pid as u64,
                        old: Some(old),
                        new: Some(new),
                    },
                )
                .await?;
            }
        }
        None => insert_pengelola(&mut tx, &ctx, user.user_id, unit_id, &input.pengelola).await?,
    }
    tx.commit().await.map_err(sql_err)?;

    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let data = unit_with_pengelola(&mut conn, unit_id).await?;
    Ok(
        Json(json!({ "success": true, "data": data, "message": "Unit SPAM berhasil diperbarui" }))
            .into_response(),
    )
}

/// `DELETE /api/spam-units/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let unit_id = parse_id(&id)?;
    let old = {
        let mut conn = state.pool.acquire().await.map_err(sql_err)?;
        match unit_raw(&mut conn, unit_id).await? {
            Some(v) => v,
            None => return Err(ApiError::not_found()),
        }
    };
    let user = require_auth(&state, &headers).await?;
    let url = format!("{}/api/spam-units/{unit_id}", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    si::log_model_change(
        &mut tx,
        &ctx,
        user.user_id,
        ModelChange {
            event: "deleted",
            model: MODEL_UNIT,
            id: unit_id as u64,
            old: Some(map_of(old)),
            new: None,
        },
    )
    .await?;
    sqlx::query("DELETE FROM tbl_unit_spam WHERE id = ?")
        .bind(unit_id)
        .execute(&mut *tx)
        .await
        .map_err(sql_err)?;
    tx.commit().await.map_err(sql_err)?;
    Ok(Json(json!({ "success": true, "message": "Unit SPAM berhasil dihapus" })).into_response())
}

// ---------------------------------------------------------------------------
// Achievement dan budget manual
// ---------------------------------------------------------------------------

/// Route model binding untuk `{unitSpam}`: 404 bila unit tidak ada.
async fn bind_unit(state: &AppState, raw: &str) -> Result<i64, ApiError> {
    let id = parse_id(raw)?;
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    if unit_raw(&mut conn, id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    Ok(id)
}

/// `POST /api/spam-units/{unitSpam}/achievements`: updateOrCreate sumber manual, 201.
pub async fn add_achievement(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(unit): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let g = |k: &str| input.get(k);
    let tahun = req_string(&mut e, "tahun", g("tahun"), 4);
    let sr = int_rule(&mut e, "jumlah_sr", g("jumlah_sr"), 0, true);
    let kk = int_rule(&mut e, "jumlah_kk", g("jumlah_kk"), 0, true);
    let jiwa = int_rule(&mut e, "jumlah_jiwa", g("jumlah_jiwa"), 0, true);
    let bjp_kk_in = int_rule(&mut e, "jumlah_bjp_kk", g("jumlah_bjp_kk"), 0, false);
    let bjp_jiwa_in = int_rule(&mut e, "jumlah_bjp_jiwa", g("jumlah_bjp_jiwa"), 0, false);
    let catatan = opt_string(&mut e, "catatan", g("catatan"), usize::MAX);
    e.finish()?;
    let (Some(tahun), Some(sr), Some(kk), Some(jiwa)) = (tahun, sr, kk, jiwa) else {
        return Err(ApiError::not_found());
    };
    let bjp_kk = bjp_kk_in.unwrap_or(0);
    let bjp_jiwa = bjp_jiwa_in.unwrap_or(bjp_kk * 5);

    let url = format!("{}/api/spam-units/{unit_id}/achievements", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    let existing: Option<(i64, i64, i64, i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT CAST(id AS SIGNED), CAST(jumlah_sr AS SIGNED), CAST(jumlah_kk AS SIGNED), CAST(jumlah_jiwa AS SIGNED), \
         CAST(jumlah_bjp_kk AS SIGNED), CAST(jumlah_bjp_jiwa AS SIGNED), catatan FROM tbl_spam_achievements \
         WHERE unit_spam_id = ? AND tahun = ? AND sumber = ?",
    )
    .bind(unit_id)
    .bind(&tahun)
    .bind(SUMBER_MANUAL)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sql_err)?;

    let target_catatan: Option<Option<String>> = catatan.clone();
    match existing {
        Some((id, o_sr, o_kk, o_jiwa, o_bkk, o_bjiwa, o_cat)) => {
            let mut old = Map::new();
            let mut new = Map::new();
            let mut set_vals: Vec<(&str, Value)> = vec![
                ("jumlah_sr", json!(sr)),
                ("jumlah_kk", json!(kk)),
                ("jumlah_jiwa", json!(jiwa)),
                ("jumlah_bjp_kk", json!(bjp_kk)),
                ("jumlah_bjp_jiwa", json!(bjp_jiwa)),
            ];
            if let Some(c) = &target_catatan {
                set_vals.push(("catatan", json!(c)));
            }
            let current = [
                ("jumlah_sr", json!(o_sr)),
                ("jumlah_kk", json!(o_kk)),
                ("jumlah_jiwa", json!(o_jiwa)),
                ("jumlah_bjp_kk", json!(o_bkk)),
                ("jumlah_bjp_jiwa", json!(o_bjiwa)),
                ("catatan", json!(o_cat)),
            ];
            for (k, v) in &set_vals {
                let before = current
                    .iter()
                    .find(|(ck, _)| ck == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Null);
                if dirty(&before, v) {
                    old.insert((*k).to_string(), before);
                    new.insert((*k).to_string(), v.clone());
                }
            }
            if !new.is_empty() {
                let mut sets = Vec::new();
                for k in new.keys() {
                    sets.push(format!("{k} = ?"));
                }
                let sql = format!(
                    "UPDATE tbl_spam_achievements SET {}, updated_at = NOW() WHERE id = ?",
                    sets.join(", ")
                );
                let mut q = sqlx::query(&sql);
                for k in new.keys() {
                    q = bind_json(q, &new[k]);
                }
                q.bind(id).execute(&mut *tx).await.map_err(sql_err)?;
                si::log_model_change(
                    &mut tx,
                    &ctx,
                    user.user_id,
                    ModelChange {
                        event: "updated",
                        model: MODEL_ACHIEVEMENT,
                        id: id as u64,
                        old: Some(old),
                        new: Some(new),
                    },
                )
                .await?;
            }
        }
        None => {
            let res = sqlx::query(
                "INSERT INTO tbl_spam_achievements (unit_spam_id, tahun, sumber, jumlah_sr, jumlah_kk, jumlah_jiwa, jumlah_bjp_kk, \
                 jumlah_bjp_jiwa, catatan, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(unit_id)
            .bind(&tahun)
            .bind(SUMBER_MANUAL)
            .bind(sr)
            .bind(kk)
            .bind(jiwa)
            .bind(bjp_kk)
            .bind(bjp_jiwa)
            .bind(catatan.clone().flatten())
            .execute(&mut *tx)
            .await
            .map_err(sql_err)?;
            let id = res.last_insert_id();
            let new_map = map_of(json!({
                "unit_spam_id": unit_id, "tahun": tahun, "sumber": SUMBER_MANUAL, "jumlah_sr": sr,
                "jumlah_kk": kk, "jumlah_jiwa": jiwa, "jumlah_bjp_kk": bjp_kk, "jumlah_bjp_jiwa": bjp_jiwa,
                "catatan": catatan.clone().flatten(), "id": id,
            }));
            si::log_model_change(
                &mut tx,
                &ctx,
                user.user_id,
                ModelChange {
                    event: "created",
                    model: MODEL_ACHIEVEMENT,
                    id,
                    old: None,
                    new: Some(new_map),
                },
            )
            .await?;
        }
    }
    let row_id: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_spam_achievements WHERE unit_spam_id = ? AND tahun = ? AND sumber = ?")
        .bind(unit_id)
        .bind(&tahun)
        .bind(SUMBER_MANUAL)
        .fetch_one(&mut *tx)
        .await
        .map_err(sql_err)?;
    let data = si::fetch_raw_rows(&mut tx, "tbl_spam_achievements", &[row_id])
        .await?
        .into_iter()
        .next()
        .unwrap_or(Value::Null);
    tx.commit().await.map_err(sql_err)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": data, "message": "Histori achievement berhasil ditambahkan!" })),
    )
        .into_response())
}

/// `POST /api/spam-units/{unitSpam}/budgets`: 201.
pub async fn add_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(unit): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let g = |k: &str| input.get(k);
    let tahun = req_string(&mut e, "tahun", g("tahun"), 4);
    let nilai = numeric_rule(&mut e, "nilai_kontrak", g("nilai_kontrak"), 0.0);
    let nama = opt_string(&mut e, "nama_paket", g("nama_paket"), 255);
    let sumber = opt_string(&mut e, "sumber_dana", g("sumber_dana"), 255);
    e.finish()?;
    let (Some(tahun), Some(nilai)) = (tahun, nilai) else {
        return Err(ApiError::not_found());
    };
    let nama_v = nama.flatten();
    let sumber_v = sumber.flatten().unwrap_or_else(|| "APBD".to_string());

    let url = format!("{}/api/spam-units/{unit_id}/budgets", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    let res = sqlx::query(
        "INSERT INTO tbl_spam_budgets (unit_spam_id, nilai_kontrak, tahun, nama_paket, sumber_dana, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(unit_id)
    .bind(nilai)
    .bind(&tahun)
    .bind(nama_v.clone().unwrap_or_default())
    .bind(&sumber_v)
    .execute(&mut *tx)
    .await
    .map_err(sql_err)?;
    let id = res.last_insert_id();
    let new_map = map_of(json!({
        "unit_spam_id": unit_id, "nilai_kontrak": nilai, "tahun": tahun,
        "nama_paket": nama_v, "sumber_dana": sumber_v, "id": id,
    }));
    si::log_model_change(
        &mut tx,
        &ctx,
        user.user_id,
        ModelChange {
            event: "created",
            model: MODEL_BUDGET,
            id,
            old: None,
            new: Some(new_map),
        },
    )
    .await?;
    let data = si::fetch_raw_rows(&mut tx, "tbl_spam_budgets", &[id as i64])
        .await?
        .into_iter()
        .next()
        .unwrap_or(Value::Null);
    tx.commit().await.map_err(sql_err)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": data, "message": "Data anggaran berhasil ditambahkan!" })),
    )
        .into_response())
}

/// `DELETE /api/spam-units/{unitSpam}/budgets/{budgetId}`: budget milik unit, 404 bila tidak ada.
pub async fn delete_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((unit, budget)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let budget_id = parse_id(&budget)?;
    let url = format!(
        "{}/api/spam-units/{unit_id}/budgets/{budget_id}",
        base_url(&state)
    );
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, unit_spam_id, pekerjaan_id, nilai_kontrak, tahun, nama_paket, sumber_dana \
         FROM tbl_spam_budgets WHERE id = ? AND unit_spam_id = ?",
    )
    .bind(budget_id)
    .bind(unit_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sql_err)?;
    let Some(row) = row else {
        return Err(ApiError::not_found());
    };
    let old = raw_model(&row, BUDGET_CASTS)?;
    si::log_model_change(
        &mut tx,
        &ctx,
        user.user_id,
        ModelChange {
            event: "deleted",
            model: MODEL_BUDGET,
            id: budget_id as u64,
            old: Some(map_of(old)),
            new: None,
        },
    )
    .await?;
    sqlx::query("DELETE FROM tbl_spam_budgets WHERE id = ?")
        .bind(budget_id)
        .execute(&mut *tx)
        .await
        .map_err(sql_err)?;
    tx.commit().await.map_err(sql_err)?;
    Ok(
        Json(json!({ "success": true, "message": "Data anggaran berhasil dihapus!" }))
            .into_response(),
    )
}

// ---------------------------------------------------------------------------
// Tautan pekerjaan
// ---------------------------------------------------------------------------

/// Pesan respons attach sesuai `accumulationStartTahun` dan tahun anggaran paket.
fn attach_message(tahun: &str) -> String {
    if si::is_accumulation_tahun(tahun) {
        format!("Pekerjaan berhasil ditautkan. Capaian SR/KK/jiwa dan anggaran tahun {tahun} diakumulasi ke unit (kontrak, atau pagu jika kontrak kosong).")
    } else {
        format!(
            "Pekerjaan berhasil ditautkan sebagai referensi. Capaian unit s/d {} tidak diubah; akumulasi otomatis berlaku mulai tahun {}.",
            si::BASELINE_CAP_TAHUN,
            si::ACCUMULATION_START_TAHUN
        )
    }
}

/// `POST /api/spam-units/{unitSpam}/pekerjaan`.
pub async fn attach_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(unit): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let g = |k: &str| input.get(k);

    let pekerjaan_id = match g("pekerjaan_id").filter(|v| !v.is_null()) {
        None => {
            e.add("pekerjaan_id", "The pekerjaan id field is required.");
            None
        }
        Some(v) => {
            let found = match as_int(v) {
                Some(id) if exists_in(&state.pool, "tbl_pekerjaan", id).await? => Some(id),
                _ => None,
            };
            if found.is_none() {
                e.add("pekerjaan_id", "The selected pekerjaan id is invalid.");
            }
            found
        }
    };
    let output_id = match g("output_id").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let found = match as_int(v) {
                Some(id) if exists_in(&state.pool, "tbl_output", id).await? => Some(id),
                _ => None,
            };
            if found.is_none() {
                e.add("output_id", "The selected output id is invalid.");
            }
            found
        }
    };
    let metric = match g("capaian_metric") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s == "jp" || s == "bjp" => Some(s.clone()),
        Some(_) => {
            e.add("capaian_metric", "The selected capaian metric is invalid.");
            None
        }
    };
    e.finish()?;
    let Some(pekerjaan_id) = pekerjaan_id else {
        return Err(ApiError::not_found());
    };

    let url = format!("{}/api/spam-units/{unit_id}/pekerjaan", base_url(&state));
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let unit_desa: Option<i64> =
        sqlx::query_scalar("SELECT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE id = ?")
            .bind(unit_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(sql_err)?;

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    match si::attach_pekerjaan(
        &mut tx,
        &ctx,
        unit_id,
        unit_desa,
        pekerjaan_id,
        output_id,
        metric.as_deref(),
    )
    .await
    {
        Ok(()) => {}
        Err(si::AttachError::Invalid(msg)) => {
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "success": false, "message": msg })),
            )
                .into_response());
        }
        Err(si::AttachError::NotFound) => return Err(ApiError::not_found()),
        Err(si::AttachError::Db(e)) => return Err(e),
    }

    let tahun_anggaran: String = sqlx::query_scalar(
        "SELECT COALESCE(k.tahun_anggaran, '') FROM tbl_pekerjaan p LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE p.id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(sql_err)?
    .unwrap_or_default();
    tx.commit().await.map_err(sql_err)?;

    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let data = unit_with_relations(&mut conn, unit_id, true)
        .await?
        .unwrap_or(Value::Null);
    let mut data = data;
    if let Value::Object(m) = &mut data {
        m.remove("checklists");
    }
    Ok(Json(json!({
        "success": true,
        "message": attach_message(&tahun_anggaran),
        "data": data,
    }))
    .into_response())
}

async fn exists_in(pool: &MySqlPool, table: &str, id: i64) -> Result<bool, ApiError> {
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM {table} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(sql_err)?;
    Ok(n > 0)
}

/// `DELETE /api/spam-units/{unitSpam}/pekerjaan/{pekerjaanId}`.
pub async fn detach_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((unit, pekerjaan)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let pekerjaan_id = parse_id(&pekerjaan)?;
    let url = format!(
        "{}/api/spam-units/{unit_id}/pekerjaan/{pekerjaan_id}",
        base_url(&state)
    );
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    si::detach_pekerjaan(&mut tx, &ctx, unit_id, pekerjaan_id).await?;
    tx.commit().await.map_err(sql_err)?;

    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let data = unit_raw(&mut conn, unit_id).await?.unwrap_or(Value::Null);
    Ok(Json(json!({
        "success": true,
        "message": "Tautan pekerjaan berhasil dihapus. Akumulasi capaian dan anggaran disesuaikan ulang.",
        "data": data,
    }))
    .into_response())
}

/// `POST /api/spam-units/{unitSpam}/sync-pekerjaan`: `tahun` wajib (maks 4), `mode` achievement|budget|all.
pub async fn sync_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(unit): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let unit_id = bind_unit(&state, &unit).await?;
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let tahun = req_string(&mut e, "tahun", input.get("tahun"), 4);
    match input.get("mode") {
        None | Some(Value::Null) => e.add("mode", "The mode field is required."),
        Some(Value::String(m)) if ["achievement", "budget", "all"].contains(&m.as_str()) => {}
        Some(_) => e.add("mode", "The selected mode is invalid."),
    }
    e.finish()?;
    let Some(tahun) = tahun else {
        return Err(ApiError::not_found());
    };

    let url = format!(
        "{}/api/spam-units/{unit_id}/sync-pekerjaan",
        base_url(&state)
    );
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);

    let mut tx = state.pool.begin().await.map_err(sql_err)?;
    let (ach_ids, bud_ids) = si::sync_to_unit(&mut tx, &ctx, unit_id, &tahun).await?;
    let achievements = si::fetch_raw_rows(&mut tx, "tbl_spam_achievements", &ach_ids).await?;
    let budgets = si::fetch_raw_rows(&mut tx, "tbl_spam_budgets", &bud_ids).await?;
    tx.commit().await.map_err(sql_err)?;

    Ok(Json(json!({
        "success": true,
        "message": "Data pekerjaan berhasil disinkronkan ke unit SPAM",
        "data": { "achievements": achievements, "budgets": budgets },
    }))
    .into_response())
}

// ---------------------------------------------------------------------------
// Statistik
// ---------------------------------------------------------------------------

/// Desa "nyata" (`Desa::scopeRealWilayah`), alias `d`, dengan kecamatan nyata.
const REAL_DESA: &str = "d.n_desa IS NOT NULL AND d.n_desa <> '' AND LOWER(TRIM(d.n_desa)) NOT IN ('null', 'nulls') \
     AND EXISTS (SELECT 1 FROM tbl_kecamatan kr WHERE kr.id = d.kecamatan_id AND kr.n_kec IS NOT NULL AND kr.n_kec <> '' \
     AND LOWER(TRIM(kr.n_kec)) NOT IN ('null', 'nulls'))";

const REAL_KEC: &str =
    "n_kec IS NOT NULL AND n_kec <> '' AND LOWER(TRIM(n_kec)) NOT IN ('null', 'nulls')";

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

async fn count_i64(c: &mut MySqlConnection, sql: &str) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(c)
        .await
        .map_err(sql_err)
}

/// `buildStats` (`SpamUnitController`). Mengembalikan objek `data` persis seperti PHP.
async fn build_stats(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
) -> Result<Value, ApiError> {
    let tahun = si::truthy_tahun(tahun);
    let total_units = count_i64(c, "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_unit_spam").await?;
    let simspam = count_i64(
        c,
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_unit_spam WHERE is_simspam = 1",
    )
    .await?;
    let non_simspam = total_units - simspam;

    let scope_label = si::combined_scope_label(tahun.as_deref());
    let cap = si::manual_cap_tahun();
    let integration = si::integration_summary(c, ctx, tahun.as_deref(), kecamatan_id, None).await?;
    let enrichment = si::stats_enrichment(c, ctx, tahun.as_deref(), kecamatan_id).await?;
    let manual_global =
        si::aggregate_manual_global(c, tahun.as_deref(), kecamatan_id, None, None).await?;

    // Target dan BJP master dari desa nyata.
    let mut tq = si::Sq {
        sql: format!("SELECT CAST(COALESCE(SUM(d.target), 0) AS SIGNED), CAST(COALESCE(SUM(d.bjp_master), 0) AS SIGNED) FROM tbl_desa d WHERE {REAL_DESA}"),
        binds: vec![],
    };
    if let Some(kec) = kecamatan_id {
        tq.push(" AND d.kecamatan_id = ?", vec![B::I(kec)]);
    }
    let trow = si::fetch_rows(c, &tq)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| internal("ringkasan desa kosong"))?;
    let total_target: i64 = trow.try_get(0).map_err(sql_err)?;
    let bjp_master_kk: i64 = trow.try_get(1).map_err(sql_err)?;

    // Cakupan achievement: tahun tertentu, atau acuan + integrasi.
    let mut ach_scope = String::new();
    let mut ach_binds: Vec<B> = Vec::new();
    match &tahun {
        Some(t) => {
            ach_scope.push_str(" AND a.tahun = ?");
            ach_binds.push(B::S(t.clone()));
        }
        None => {
            ach_scope.push_str(" AND (a.tahun <= ? OR a.tahun >= ?)");
            ach_binds.push(B::S(si::BASELINE_CAP_TAHUN.to_string()));
            ach_binds.push(B::S(si::ACCUMULATION_START_TAHUN.to_string()));
        }
    }
    if let Some(kec) = kecamatan_id {
        ach_scope.push_str(" AND EXISTS (SELECT 1 FROM tbl_unit_spam u JOIN tbl_desa dk ON dk.id = u.desa_id WHERE u.id = a.unit_spam_id AND dk.kecamatan_id = ?)");
        ach_binds.push(B::I(kec));
    }
    let bjp_unit_kk = sum_with(c, &format!("SELECT CAST(COALESCE(SUM(a.jumlah_bjp_kk), 0) AS SIGNED) FROM tbl_spam_achievements a WHERE 1 = 1{ach_scope}"), &ach_binds).await?;
    let achievement_count = count_i64(
        c,
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spam_achievements",
    )
    .await?;

    let (total_sr, total_kk, total_jiwa, display_nilai) = if tahun.is_some() {
        (
            manual_global.sr,
            manual_global.kk,
            manual_global.jiwa,
            manual_global.nilai,
        )
    } else {
        let g = |k: &str| enrichment.get(k).cloned().unwrap_or(Value::Null);
        (
            g("capaian_sr").as_i64().unwrap_or(0),
            g("capaian_kk").as_i64().unwrap_or(0),
            g("capaian_jiwa").as_i64().unwrap_or(0),
            g("capaian_nilai_kontrak").as_f64().unwrap_or(0.0),
        )
    };

    let total_bjp_kk = bjp_master_kk + bjp_unit_kk;
    let total_bjp_jiwa = total_bjp_kk * 5;
    let coverage: Value = if total_target > 0 {
        number_like_php(round2(
            ((total_kk + total_bjp_kk) as f64 / total_target as f64) * 100.0,
        ))
    } else {
        json!(0)
    };

    // Distribusi sumber dana anggaran.
    let mut fq = si::Sq {
        sql: "SELECT b.sumber_dana, CAST(COUNT(DISTINCT b.unit_spam_id) AS SIGNED) AS cnt FROM tbl_spam_budgets b WHERE 1 = 1".to_string(),
        binds: vec![],
    };
    match &tahun {
        Some(t) => fq.push(" AND b.tahun = ?", vec![B::S(t.clone())]),
        None => fq.push(
            " AND (b.tahun <= ? OR b.tahun >= ?)",
            vec![
                B::S(si::BASELINE_CAP_TAHUN.to_string()),
                B::S(si::ACCUMULATION_START_TAHUN.to_string()),
            ],
        ),
    }
    if let Some(kec) = kecamatan_id {
        fq.push(
            " AND EXISTS (SELECT 1 FROM tbl_unit_spam u JOIN tbl_desa dk ON dk.id = u.desa_id WHERE u.id = b.unit_spam_id AND dk.kecamatan_id = ?)",
            vec![B::I(kec)],
        );
    }
    fq.push(" GROUP BY b.sumber_dana ORDER BY cnt DESC", vec![]);
    let mut funding = Vec::new();
    for r in si::fetch_rows(c, &fq).await? {
        funding.push(json!({
            "sumber_dana": r.try_get::<Option<String>, _>(0).map_err(sql_err)?,
            "count": r.try_get::<i64, _>(1).map_err(sql_err)?,
        }));
    }

    let wilayah_desa = count_i64(
        c,
        &format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_desa d WHERE {REAL_DESA}"),
    )
    .await?;
    let wilayah_kec = count_i64(
        c,
        &format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kecamatan WHERE {REAL_KEC}"),
    )
    .await?;
    let pekerjaan_all = count_i64(c, "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan").await?;
    let foto_all = count_i64(c, "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_foto").await?;

    let e = |k: &str| enrichment.get(k).cloned().unwrap_or(Value::Null);
    let i = |k: &str| integration.get(k).cloned().unwrap_or(json!(0));
    let baseline_tahun = si::BASELINE_CAP_TAHUN;
    let start_tahun = si::ACCUMULATION_START_TAHUN;

    let mut out = Map::new();
    out.insert("total_units".into(), json!(total_units));
    out.insert("simspam_count".into(), json!(simspam));
    out.insert("non_simspam_count".into(), json!(non_simspam));
    out.insert("target_year".into(), json!(scope_label));
    out.insert("manual_scope_label".into(), json!(scope_label));
    out.insert("manual_cap_tahun".into(), json!(cap));
    out.insert("total_target".into(), json!(total_target));
    out.insert("total_sr".into(), json!(total_sr));
    out.insert("total_kk".into(), json!(total_kk));
    out.insert("total_jiwa".into(), json!(total_jiwa));
    out.insert("total_bjp_kk".into(), json!(total_bjp_kk));
    out.insert("total_bjp_jiwa".into(), json!(total_bjp_jiwa));
    out.insert("funding_distribution".into(), Value::Array(funding));
    out.insert("coverage_percentage".into(), coverage.clone());
    out.insert("wilayah_total_desa".into(), json!(wilayah_desa));
    out.insert("wilayah_total_kecamatan".into(), json!(wilayah_kec));
    out.insert("achievement_records".into(), json!(achievement_count));
    out.insert("total_pekerjaan_all".into(), json!(pekerjaan_all));
    out.insert("total_foto_dokumentasi".into(), json!(foto_all));
    out.insert("stats_generated_at".into(), json!(now_iso()));
    for (k, v) in &integration {
        out.insert(k.clone(), v.clone());
    }
    for (k, v) in &enrichment {
        out.insert(k.clone(), v.clone());
    }
    out.insert("manual_sr".into(), json!(manual_global.sr));
    out.insert("manual_kk".into(), json!(manual_global.kk));
    out.insert("manual_jiwa".into(), json!(manual_global.jiwa));
    out.insert(
        "manual_nilai_kontrak".into(),
        number_like_php(display_nilai),
    );
    out.insert("total_linked".into(), i("total_linked"));

    let ringkasan = json!({
        "scope_label": scope_label,
        "baseline_cap_tahun": baseline_tahun,
        "accumulation_start_tahun": start_tahun,
        "baseline": {
            "label": format!("Acuan master s/d {baseline_tahun}"),
            "keterangan": "Data awal unit SPAM (import). Tidak ditimpa oleh integrasi pekerjaan.",
            "sr": e("capaian_baseline_sr"), "kk": e("capaian_baseline_kk"),
            "jiwa": e("capaian_baseline_jiwa"), "nilai_kontrak": e("capaian_baseline_nilai_kontrak"),
        },
        "capaian": {
            "label": "Capaian unit SPAM tercatat (total)",
            "keterangan": format!("Acuan s/d {baseline_tahun} + capaian integrasi {start_tahun} ke atas"),
            "sr": e("capaian_sr"), "kk": e("capaian_kk"),
            "jiwa": e("capaian_jiwa"), "nilai_kontrak": e("capaian_nilai_kontrak"),
        },
        "integrasi": {
            "label": "Status tautan pekerjaan",
            "paket_tertaut": e("linked_pekerjaan_count"),
            "paket_tersedia": integration.get("pekerjaan_air_minum_count").cloned().unwrap_or(json!(0)),
            "paket_belum_tertaut": e("paket_belum_tertaut"),
            "unit_dengan_tautan": e("linked_units_count"),
            "desa_terintegrasi": i("matched_count"),
            "desa_partial": i("partial_count"),
            "desa_tanpa_unit": i("no_unit_count"),
            "desa_tanpa_pekerjaan": i("no_pekerjaan_count"),
        },
        "capaian_integrasi": {
            "label": format!("Capaian integrasi {start_tahun} ke atas"),
            "keterangan": "Hanya tahun integrasi; dipakai untuk perbandingan dengan potensi pekerjaan",
            "sr": e("capaian_integrasi_sr"), "kk": e("capaian_integrasi_kk"),
            "jiwa": e("capaian_integrasi_jiwa"), "nilai_kontrak": e("capaian_integrasi_nilai_kontrak"),
        },
        "potensi": {
            "label": format!("Potensi pekerjaan AM ({start_tahun} ke atas)"),
            "keterangan": format!("Paket sub bidang air minum tahun {start_tahun} ke atas (belum tentu sudah ditaut)"),
            "sr": e("potensi_sr"), "kk": e("potensi_kk"),
            "jiwa": e("potensi_jiwa"), "nilai_kontrak": e("potensi_nilai_kontrak"),
        },
        "dari_tautan": {
            "label": "Akumulasi paket yang sudah ditaut",
            "sr": e("linked_sr"), "kk": e("linked_kk"),
            "jiwa": e("linked_jiwa"), "nilai_kontrak": e("linked_nilai_kontrak"),
        },
        "selisih_potensi_capaian": {
            "sr": e("selisih_sr"), "kk": e("selisih_kk"),
            "jiwa": e("selisih_jiwa"), "nilai_kontrak": e("selisih_nilai_kontrak"),
        },
        "spm": {
            "target_kk": total_target,
            "jp_kk": total_kk,
            "bjp_master_kk": bjp_master_kk,
            "bjp_unit_kk": bjp_unit_kk,
            "total_bjp_kk": total_bjp_kk,
            "coverage_percentage": coverage,
        },
    });
    out.insert("ringkasan".into(), ringkasan);
    Ok(Value::Object(out))
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

async fn sum_with(c: &mut MySqlConnection, sql: &str, binds: &[B]) -> Result<i64, ApiError> {
    let mut q = sqlx::query_scalar::<_, i64>(sql);
    for b in binds {
        q = match b {
            B::S(v) => q.bind(v.clone()),
            B::I(v) => q.bind(*v),
            B::F(v) => q.bind(*v),
        };
    }
    q.fetch_one(c).await.map_err(sql_err)
}

fn stats_tahun(raw: Option<&String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn stats_payload(
    state: &AppState,
    ctx: &Ctx<'_>,
    tahun: Option<String>,
    kecamatan: Option<i64>,
) -> Result<Value, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    build_stats(&mut conn, ctx, tahun.as_deref(), kecamatan).await
}

fn kecamatan_param(query: &HashMap<String, String>) -> Option<i64> {
    query
        .get("kecamatan_id")
        .filter(|v| !v.is_empty() && v.as_str() != "0")
        .map(|v| si::php_int(v))
        .filter(|v| *v != 0)
}

/// `GET /api/spam-units/stats` (auth).
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let data = stats_payload(
        &state,
        &ctx,
        stats_tahun(query.get("tahun")),
        kecamatan_param(&query),
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `GET /api/public/spam-units/stats` (tanpa token: byUserRole = 1 = 0).
pub async fn public_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let url = String::new();
    let ctx = guest_ctx(&url, &headers);
    let data = stats_payload(
        &state,
        &ctx,
        stats_tahun(query.get("tahun")),
        kecamatan_param(&query),
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// Daftar tahun dari `years=2020,2021,...`: 4 digit, unik, maksimal 20.
fn years_param(raw: Option<&String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.map(String::as_str).unwrap_or("").split(',') {
        let y = part.trim();
        if y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit()) && !out.iter().any(|x| x == y) {
            out.push(y.to_string());
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

/// `GET /api/spam-units/stats/series` (auth).
pub async fn stats_series(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let kec = kecamatan_param(&query);
    let mut data = Map::new();
    for y in years_param(query.get("years")) {
        let v = stats_payload(&state, &ctx, Some(y.clone()), kec).await?;
        data.insert(y, v);
    }
    Ok(Json(json!({ "success": true, "data": if data.is_empty() { json!([]) } else { Value::Object(data) } })).into_response())
}

/// `GET /api/public/spam-units/map-stats/series`.
pub async fn public_map_stats_series(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let mut data = Map::new();
    for y in years_param(query.get("years")) {
        let rows = desa_map_stats(&state.pool, Some(&y)).await?;
        data.insert(y, Value::Array(rows));
    }
    Ok(Json(json!({ "success": true, "data": if data.is_empty() { json!([]) } else { Value::Object(data) } })).into_response())
}

/// `GET /api/public/spam-units/map-stats`.
pub async fn public_map_stats(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let tahun = stats_tahun(query.get("tahun"));
    let rows = desa_map_stats(&state.pool, tahun.as_deref()).await?;
    Ok(Json(json!({ "success": true, "data": rows })).into_response())
}

/// `desaMapStats`: per desa nyata, jumlah unit dan capaian (tahun tertentu, atau acuan + integrasi).
async fn desa_map_stats(pool: &MySqlPool, tahun: Option<&str>) -> Result<Vec<Value>, ApiError> {
    let tahun = si::truthy_tahun(tahun);
    let mut conn = pool.acquire().await.map_err(sql_err)?;

    let unit_counts: HashMap<i64, i64> = sqlx::query("SELECT CAST(desa_id AS SIGNED) AS desa_id, CAST(COUNT(*) AS SIGNED) AS cnt FROM tbl_unit_spam GROUP BY desa_id")
        .fetch_all(&mut *conn)
        .await
        .map_err(sql_err)?
        .iter()
        .map(|r| Ok((r.try_get::<i64, _>("desa_id").map_err(sql_err)?, r.try_get::<i64, _>("cnt").map_err(sql_err)?)))
        .collect::<Result<_, ApiError>>()?;

    let (primary, secondary) = match &tahun {
        Some(t) => (grouped_ach(&mut conn, Some(t), None, None).await?, None),
        None => (
            grouped_ach(
                &mut conn,
                None,
                Some(si::php_int(si::BASELINE_CAP_TAHUN)),
                None,
            )
            .await?,
            Some(
                grouped_ach(
                    &mut conn,
                    None,
                    None,
                    Some(si::php_int(si::ACCUMULATION_START_TAHUN)),
                )
                .await?,
            ),
        ),
    };

    let desas = sqlx::query(&format!(
        "SELECT CAST(d.id AS SIGNED) AS id, d.n_desa, CAST(d.kecamatan_id AS SIGNED) AS kecamatan_id, CAST(COALESCE(d.target, 0) AS SIGNED) AS target, \
         CAST(COALESCE(d.bjp_master, 0) AS SIGNED) AS bjp_master, k.n_kec FROM tbl_desa d \
         JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE {REAL_DESA} ORDER BY d.n_desa, d.id"
    ))
    .fetch_all(&mut *conn)
    .await
    .map_err(sql_err)?;

    let mut out = Vec::with_capacity(desas.len());
    for r in &desas {
        let id: i64 = r.try_get("id").map_err(sql_err)?;
        let (sr, kk, jiwa, bjp_unit) = match &secondary {
            None => {
                let v = primary.get(&id).copied().unwrap_or_default();
                (v.sr, v.kk, v.jiwa, v.bjp)
            }
            Some(integrasi) => {
                let a = primary.get(&id).copied().unwrap_or_default();
                let b = integrasi.get(&id).copied().unwrap_or_default();
                (a.sr + b.sr, a.kk + b.kk, a.jiwa + b.jiwa, a.bjp + b.bjp)
            }
        };
        out.push(json!({
            "desa_id": id,
            "desa": r.try_get::<Option<String>, _>("n_desa").map_err(sql_err)?,
            "kecamatan": r.try_get::<Option<String>, _>("n_kec").map_err(sql_err)?,
            "target": r.try_get::<i64, _>("target").map_err(sql_err)?,
            "unit_count": unit_counts.get(&id).copied().unwrap_or(0),
            "sr": sr,
            "kk": kk,
            "jiwa": jiwa,
            "bjp_master": r.try_get::<i64, _>("bjp_master").map_err(sql_err)?,
            "bjp_unit": bjp_unit,
        }));
    }
    Ok(out)
}

#[derive(Default, Clone, Copy)]
struct AchSum {
    sr: i64,
    kk: i64,
    jiwa: i64,
    bjp: i64,
}

/// `groupedAchievementsByDesa`.
async fn grouped_ach(
    c: &mut MySqlConnection,
    tahun: Option<&str>,
    max: Option<i64>,
    min: Option<i64>,
) -> Result<HashMap<i64, AchSum>, ApiError> {
    let mut sql = String::from(
        "SELECT CAST(u.desa_id AS SIGNED) AS desa_id, CAST(COALESCE(SUM(a.jumlah_sr), 0) AS SIGNED) AS sr, CAST(COALESCE(SUM(a.jumlah_kk), 0) AS SIGNED) AS kk, \
         CAST(COALESCE(SUM(a.jumlah_jiwa), 0) AS SIGNED) AS jiwa, CAST(COALESCE(SUM(a.jumlah_bjp_kk), 0) AS SIGNED) AS bjp \
         FROM tbl_spam_achievements a JOIN tbl_unit_spam u ON u.id = a.unit_spam_id WHERE 1 = 1",
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(t) = tahun {
        sql.push_str(" AND a.tahun = ?");
        binds.push(t.to_string());
    } else {
        if let Some(mx) = max {
            sql.push_str(" AND a.tahun <= ?");
            binds.push(mx.to_string());
        }
        if let Some(mn) = min {
            sql.push_str(" AND a.tahun >= ?");
            binds.push(mn.to_string());
        }
    }
    sql.push_str(" GROUP BY u.desa_id");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b.clone());
    }
    let mut map = HashMap::new();
    for r in q.fetch_all(c).await.map_err(sql_err)? {
        map.insert(
            r.try_get::<i64, _>("desa_id").map_err(sql_err)?,
            AchSum {
                sr: r.try_get("sr").map_err(sql_err)?,
                kk: r.try_get("kk").map_err(sql_err)?,
                jiwa: r.try_get("jiwa").map_err(sql_err)?,
                bjp: r.try_get("bjp").map_err(sql_err)?,
            },
        );
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Integrasi (baca)
// ---------------------------------------------------------------------------

const OUTPUT_TYPES: &[&str] = &[
    "sambungan_rumah",
    "pipa_jaringan",
    "reservoir",
    "sumber_air",
    "bjp",
];
const SYNC_STATUSES: &[&str] = &["matched", "partial", "no_unit", "no_pekerjaan", "no_data"];

fn opt_query(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query
        .get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Validasi `in:` untuk query opsional.
fn check_in(e: &mut Errors, field: &str, v: Option<&String>, allowed: &[&str]) {
    if let Some(val) = v.filter(|s| !s.is_empty()) {
        if !allowed.contains(&val.as_str()) {
            e.add(field, format!("The selected {} is invalid.", attr(field)));
        }
    }
}

/// `GET /api/spam-units/integration/output-options` (auth).
pub async fn integration_output_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let data = si::output_options(
        &mut conn,
        &ctx,
        opt_query(&query, "tahun").as_deref(),
        kecamatan_param(&query),
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `GET /api/spam-units/integration` (auth): paginasi desa dengan ringkasan.
pub async fn integration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut e = Errors::default();
    check_in(
        &mut e,
        "sync_status",
        query.get("sync_status"),
        SYNC_STATUSES,
    );
    check_in(
        &mut e,
        "output_type",
        query.get("output_type"),
        OUTPUT_TYPES,
    );
    if query
        .get("komponen")
        .is_some_and(|v| v.chars().count() > 255)
    {
        e.add(
            "komponen",
            "The komponen field must not be greater than 255 characters.",
        );
    }
    e.finish()?;

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let per_page = query.get("per_page").map(|v| si::php_int(v)).unwrap_or(15);
    let page = query.get("page").map(|v| si::php_int(v)).unwrap_or(1);
    let (data, meta, summary) = si::paginate_integration(
        &mut conn,
        &ctx,
        opt_query(&query, "tahun").as_deref(),
        kecamatan_param(&query),
        query
            .get("desa_id")
            .filter(|v| !v.is_empty())
            .map(|v| si::php_int(v)),
        opt_query(&query, "search").as_deref(),
        opt_query(&query, "sync_status").as_deref(),
        opt_query(&query, "output_type").as_deref(),
        opt_query(&query, "komponen").as_deref(),
        per_page,
        page.max(1),
    )
    .await?;
    Ok(
        Json(json!({ "success": true, "data": data, "meta": meta, "summary": summary }))
            .into_response(),
    )
}

/// `GET /api/spam-units/integration/desa/{desaId}` (auth). Validasi lebih dulu, lalu 404 desa.
pub async fn integration_by_desa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(desa): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut e = Errors::default();
    check_in(
        &mut e,
        "output_type",
        query.get("output_type"),
        OUTPUT_TYPES,
    );
    e.finish()?;
    let desa_id = parse_id(&desa)?;

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let Some(info) = si::desa_info(&mut conn, desa_id).await? else {
        return Err(ApiError::not_found());
    };
    let row = si::desa_integration_row(
        &mut conn,
        &ctx,
        &info,
        opt_query(&query, "tahun").as_deref(),
        opt_query(&query, "output_type").as_deref(),
        opt_query(&query, "komponen").as_deref(),
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": row })).into_response())
}

/// `GET /api/spam-units/air-minum-pekerjaan` (auth).
pub async fn air_minum_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut e = Errors::default();
    check_in(
        &mut e,
        "output_type",
        query.get("output_type"),
        OUTPUT_TYPES,
    );
    e.finish()?;

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(sql_err)?;
    let url = String::new();
    let ctx = ctx_for(user.user_id, &roles, &url, &headers);
    let mut conn = state.pool.acquire().await.map_err(sql_err)?;
    let unlinked = query
        .get("unlinked_only")
        .map(|v| filter_bool(v))
        .unwrap_or(false);
    let per_page = query.get("per_page").map(|v| si::php_int(v)).unwrap_or(15);
    let page = query.get("page").map(|v| si::php_int(v)).unwrap_or(1);
    let (data, meta) = si::paginate_air_minum_pekerjaan(
        &mut conn,
        &ctx,
        opt_query(&query, "tahun").as_deref(),
        kecamatan_param(&query),
        query
            .get("desa_id")
            .filter(|v| !v.is_empty())
            .map(|v| si::php_int(v)),
        opt_query(&query, "search").as_deref(),
        opt_query(&query, "output_type").as_deref(),
        query
            .get("unit_spam_id")
            .filter(|v| !v.is_empty())
            .map(|v| si::php_int(v)),
        unlinked,
        per_page,
        page.max(1),
    )
    .await?;
    Ok(Json(json!({ "success": true, "data": data, "meta": meta })).into_response())
}
