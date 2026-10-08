//! Tulis kegiatan: `POST /api/kegiatan`, `PUT`/`PATCH`/`DELETE /api/kegiatan/{id}`.
//!
//! Mengikuti `KegiatanController` dan model `Kegiatan` (`Auditable`, `NotifiesAdminsOnChanges`).
//! Angka `pagu` dibaca dari DB sebagai teks desimal (`decimal(15,2)`), seperti cast Laravel.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Row};

use crate::{changes, foto, kegiatan, require_auth, AppState};

const SUMBER_DANA: [&str; 12] = [
    "APBD",
    "APBN",
    "DAU",
    "DAK",
    "DID",
    "Bantuan Provinsi",
    "DBH",
    "SILPA",
    "DBH Pajak Rokok",
    "PAD",
    "DBHCT",
    "DBH Prov",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn attr_name(key: &str) -> String {
    key.replace('_', " ")
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Kolom yang ditulis: `None` = tidak dikirim (update tidak menyentuhnya), `Some(None)` = null.
#[derive(Default)]
struct Input {
    cols: Vec<(String, Col)>,
}

#[derive(Clone)]
enum Col {
    Null,
    Text(String),
    Decimal(f64),
    Int(i64),
    Json(String),
}

/// Aturan `nullable` untuk kegiatan. Hanya kunci yang ada di body yang masuk ke `cols`.
fn validate(body: &Value) -> Result<Input, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut input = Input::default();

    for (key, max) in [
        ("nama_program", 255usize),
        ("sub_bidang", 255),
        ("nama_kegiatan", 255),
        ("nama_sub_kegiatan", 255),
        ("tahun_anggaran", 50),
        ("nama_pptk", 255),
        ("nip_pptk", 50),
        ("kode_sub_giat", 64),
    ] {
        if let Some(v) = obj.get(key) {
            match text(v) {
                None => input.cols.push((key.to_string(), Col::Null)),
                Some(s) if s.chars().count() > max => foto::add(
                    &mut errs,
                    key,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        attr_name(key)
                    ),
                ),
                Some(s) => input.cols.push((key.to_string(), Col::Text(s))),
            }
        }
    }

    if let Some(v) = obj.get("sumber_dana") {
        match text(v) {
            None => input.cols.push(("sumber_dana".to_string(), Col::Null)),
            Some(s) if SUMBER_DANA.contains(&s.as_str()) => {
                input.cols.push(("sumber_dana".to_string(), Col::Text(s)))
            }
            Some(_) => foto::add(
                &mut errs,
                "sumber_dana",
                "The selected sumber dana is invalid.".into(),
            ),
        }
    }

    if let Some(v) = obj.get("pagu") {
        match v {
            Value::Null => input.cols.push(("pagu".to_string(), Col::Null)),
            _ => match text(v)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite())
            {
                Some(n) if n >= 0.0 => input.cols.push(("pagu".to_string(), Col::Decimal(n))),
                Some(_) => foto::add(
                    &mut errs,
                    "pagu",
                    "The pagu field must be at least 0.".into(),
                ),
                None => foto::add(&mut errs, "pagu", "The pagu field must be a number.".into()),
            },
        }
    }

    if let Some(v) = obj.get("kode_rekening") {
        match v {
            Value::Null => input.cols.push(("kode_rekening".to_string(), Col::Null)),
            Value::Array(_) => input
                .cols
                .push(("kode_rekening".to_string(), Col::Json(v.to_string()))),
            _ => foto::add(
                &mut errs,
                "kode_rekening",
                "The kode rekening field must be an array.".into(),
            ),
        }
    }

    if let Some(v) = obj.get("sipd_id_sub_bl") {
        match v {
            Value::Null => input.cols.push(("sipd_id_sub_bl".to_string(), Col::Null)),
            _ => match v.as_i64().or_else(|| text(v).and_then(|s| s.parse().ok())) {
                Some(n) if n >= 1 => input.cols.push(("sipd_id_sub_bl".to_string(), Col::Int(n))),
                Some(_) => foto::add(
                    &mut errs,
                    "sipd_id_sub_bl",
                    "The sipd id sub bl field must be at least 1.".into(),
                ),
                None => foto::add(
                    &mut errs,
                    "sipd_id_sub_bl",
                    "The sipd id sub bl field must be an integer.".into(),
                ),
            },
        }
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

/// Atribut untuk audit, dibaca dari DB supaya format `pagu` dan JSON sama dengan Laravel.
async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), nama_program, sub_bidang, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, \
         CAST(pagu AS CHAR), CAST(kode_rekening AS CHAR), nama_pptk, nip_pptk, CAST(sipd_id_sub_bl AS SIGNED), kode_sub_giat, \
         created_at, updated_at FROM tbl_kegiatan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let text_at = |i: usize| -> Result<Option<String>, ApiError> { r.try_get(i).map_err(internal) };
    let kode: Option<String> = text_at(8)?;
    let kode_value = kode
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(13).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(14).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert("nama_program".into(), json!(text_at(1)?));
    m.insert("sub_bidang".into(), json!(text_at(2)?));
    m.insert("nama_kegiatan".into(), json!(text_at(3)?));
    m.insert("nama_sub_kegiatan".into(), json!(text_at(4)?));
    m.insert("tahun_anggaran".into(), json!(text_at(5)?));
    m.insert("sumber_dana".into(), json!(text_at(6)?));
    m.insert("pagu".into(), json!(text_at(7)?));
    m.insert("kode_rekening".into(), kode_value);
    m.insert("nama_pptk".into(), json!(text_at(9)?));
    m.insert("nip_pptk".into(), json!(text_at(10)?));
    m.insert(
        "sipd_id_sub_bl".into(),
        json!(r.try_get::<Option<i64>, _>(11).map_err(internal)?),
    );
    m.insert("kode_sub_giat".into(), json!(text_at(12)?));
    m.insert("created_at".into(), crate::lookup::carbon_json(created));
    m.insert("updated_at".into(), crate::lookup::carbon_json(updated));
    Ok(Some(m))
}

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

async fn bind_cols<'q>(
    mut q: sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments>,
    cols: &[(String, Col)],
) -> sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments> {
    for (_, c) in cols {
        q = match c.clone() {
            Col::Null => q.bind(None::<String>),
            Col::Text(s) => q.bind(s),
            Col::Decimal(n) => q.bind(n),
            Col::Int(n) => q.bind(n),
            Col::Json(s) => q.bind(s),
        };
    }
    q
}

async fn respond(pool: &sqlx::MySqlPool, id: u64) -> Result<Json<Value>, ApiError> {
    let row = kegiatan::find(pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": kegiatan::to_resource(&row) })))
}

/// `POST /api/kegiatan`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = validate(&body)?;
    let url = format!("{}/api/kegiatan", state.app_url.trim_end_matches('/'));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    // Kolom yang tidak dikirim tetap NULL (default kolom), sama dengan `Kegiatan::create($validated)`.
    let names: Vec<&str> = input.cols.iter().map(|(k, _)| k.as_str()).collect();
    let placeholders = vec!["?"; names.len()].join(", ");
    let sql = if names.is_empty() {
        "INSERT INTO tbl_kegiatan (created_at, updated_at) VALUES (NOW(), NOW())".to_string()
    } else {
        format!(
            "INSERT INTO tbl_kegiatan ({}, created_at, updated_at) VALUES ({placeholders}, NOW(), NOW())",
            names.join(", ")
        )
    };
    let res = bind_cols(sqlx::query(&sql), &input.cols)
        .await
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let id = res.last_insert_id();
    let created = attributes(&mut *tx, id)
        .await?
        .ok_or_else(|| internal("kegiatan baru tidak terbaca"))?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KEGIATAN,
        "created",
        id as i64,
        None,
        Some(created),
        Some(format!("/kegiatan/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state.pool, id).await
}

/// `PUT` dan `PATCH /api/kegiatan/{id}`. Kunci yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let input = validate(&body)?;
    if input.cols.is_empty() {
        return respond(&state.pool, id).await;
    }
    let url = format!("{}/api/kegiatan/{id}", state.app_url.trim_end_matches('/'));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let set_sql: Vec<String> = input.cols.iter().map(|(k, _)| format!("{k} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_kegiatan SET {}, updated_at = NOW() WHERE id = ?",
        set_sql.join(", ")
    );
    bind_cols(sqlx::query(&sql), &input.cols)
        .await
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let after = attributes(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if let Some((old, new)) = diff(&before, &after) {
        changes::log_linked(
            &mut tx,
            &headers,
            user.user_id,
            &changes::KEGIATAN,
            "updated",
            id as i64,
            Some(old),
            Some(new),
            Some(format!("/kegiatan/{id}/edit")),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    respond(&state.pool, id).await
}

/// `DELETE /api/kegiatan/{id}`. `kegiatan_role` ikut terhapus lewat FK cascade.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let url = format!("{}/api/kegiatan/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_kegiatan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KEGIATAN,
        "deleted",
        id as i64,
        Some(before),
        None,
        Some(format!("/kegiatan/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Kegiatan deleted successfully" })))
}

/// `GET /api/kegiatan/tahun/{tahun}`: paginasi 15, seperti `Kegiatan::where(tahun)->paginate(15)`.
pub async fn by_tahun(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tahun): Path<String>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let per_page = 15u64;
    let (rows, total) = kegiatan::list(
        &state.pool,
        Some(&tahun),
        Some((per_page, (page - 1) * per_page)),
    )
    .await
    .map_err(internal)?;
    let data: Vec<Value> = rows.iter().map(kegiatan::to_resource).collect();
    let base = format!(
        "{}/api/kegiatan/tahun/{tahun}",
        state.app_url.trim_end_matches('/')
    );
    Ok(Json(crate::pagination::paginate_with_query(
        data,
        total,
        crate::pagination::PageParams { page, per_page },
        &base,
        "",
    )))
}
