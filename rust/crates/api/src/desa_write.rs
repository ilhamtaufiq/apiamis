//! Tulis desa: `POST /api/desa`, `PUT`/`PATCH`/`DELETE /api/desa/{id}`, dan `GET /api/desa/kecamatan/{id}`.
//!
//! Mengikuti `DesaController` dan model `Desa` (`Auditable` dan `NotifiesAdminsOnChanges`).
//! `profile` dan `sync-kk` belum dipindah: keduanya mengagregasi modul yang belum ada di Rust.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{changes, desa, foto, kecamatan, lookup::carbon_json, require_auth, AppState};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Kolom tabel untuk audit, termasuk `target` dan `bjp_master` yang tidak ada di `DesaRow`.
async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = sqlx::MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), n_desa, luas, CAST(jumlah_penduduk AS SIGNED), CAST(jumlah_kk AS SIGNED), \
         CAST(target AS SIGNED), CAST(bjp_master AS SIGNED), CAST(kecamatan_id AS SIGNED), created_at, updated_at \
         FROM tbl_desa WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let ts = |i: usize| -> Result<Option<chrono::DateTime<chrono::Utc>>, ApiError> {
        r.try_get(i).map_err(internal)
    };
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "n_desa".into(),
        json!(r.try_get::<Option<String>, _>(1).map_err(internal)?),
    );
    m.insert(
        "luas".into(),
        json!(r.try_get::<Option<f64>, _>(2).map_err(internal)?),
    );
    m.insert(
        "jumlah_penduduk".into(),
        json!(r.try_get::<Option<i64>, _>(3).map_err(internal)?),
    );
    m.insert(
        "jumlah_kk".into(),
        json!(r.try_get::<Option<i64>, _>(4).map_err(internal)?),
    );
    m.insert(
        "target".into(),
        json!(r.try_get::<i64, _>(5).map_err(internal)?),
    );
    m.insert(
        "bjp_master".into(),
        json!(r.try_get::<i64, _>(6).map_err(internal)?),
    );
    m.insert(
        "kecamatan_id".into(),
        json!(r.try_get::<Option<i64>, _>(7).map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(ts(8)?));
    m.insert("updated_at".into(), carbon_json(ts(9)?));
    Ok(Some(m))
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

/// Angka: integer untuk `integer`, dan numeric untuk `numeric` (sama dengan aturan Laravel).
fn as_integer(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn as_numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// Input yang sudah divalidasi. `None` = tidak dikirim, `Some(None)` = dikirim null.
#[derive(Default)]
struct DesaInput {
    nama_desa: Option<Option<String>>,
    luas: Option<Option<f64>>,
    jumlah_penduduk: Option<Option<i64>>,
    jumlah_kk: Option<Option<i64>>,
    kecamatan_id: Option<Option<i64>>,
}

async fn validate(pool: &MySqlPool, body: &Value, store: bool) -> Result<DesaInput, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut input = DesaInput::default();
    let present = |k: &str| obj.get(k).filter(|v| !v.is_null()).is_some();

    // nama_desa: wajib saat store, maksimal 100 karakter.
    match obj.get("nama_desa").and_then(text) {
        Some(n) if n.chars().count() > 100 => foto::add(
            &mut errs,
            "nama_desa",
            "The nama desa field must not be greater than 100 characters.".into(),
        ),
        Some(n) => input.nama_desa = Some(Some(n)),
        None if store => foto::add(
            &mut errs,
            "nama_desa",
            "The nama desa field is required.".into(),
        ),
        None => {}
    }

    // luas: wajib saat store, numeric.
    match obj.get("luas").filter(|v| !v.is_null()) {
        None if store => foto::add(&mut errs, "luas", "The luas field is required.".into()),
        None => {}
        Some(v) => match as_numeric(v) {
            Some(n) => input.luas = Some(Some(n)),
            None => foto::add(&mut errs, "luas", "The luas field must be a number.".into()),
        },
    }
    if !store && obj.contains_key("luas") && obj["luas"].is_null() {
        input.luas = Some(None);
    }

    // jumlah_penduduk: wajib saat store, integer.
    match obj.get("jumlah_penduduk").filter(|v| !v.is_null()) {
        None if store => foto::add(
            &mut errs,
            "jumlah_penduduk",
            "The jumlah penduduk field is required.".into(),
        ),
        None => {}
        Some(v) => match as_integer(v) {
            Some(n) => input.jumlah_penduduk = Some(Some(n)),
            None => foto::add(
                &mut errs,
                "jumlah_penduduk",
                "The jumlah penduduk field must be an integer.".into(),
            ),
        },
    }
    if !store && obj.contains_key("jumlah_penduduk") && obj["jumlah_penduduk"].is_null() {
        input.jumlah_penduduk = Some(None);
    }

    // jumlah_kk: nullable, integer minimal 0.
    if present("jumlah_kk") {
        match as_integer(&obj["jumlah_kk"]) {
            Some(n) if n >= 0 => input.jumlah_kk = Some(Some(n)),
            Some(_) => foto::add(
                &mut errs,
                "jumlah_kk",
                "The jumlah kk field must be at least 0.".into(),
            ),
            None => foto::add(
                &mut errs,
                "jumlah_kk",
                "The jumlah kk field must be an integer.".into(),
            ),
        }
    } else if obj.contains_key("jumlah_kk") {
        input.jumlah_kk = Some(None);
    }

    // kecamatan_id: wajib saat store, harus ada di tbl_kecamatan.
    let kecamatan = match obj.get("kecamatan_id").filter(|v| !v.is_null()) {
        None if store => {
            foto::add(
                &mut errs,
                "kecamatan_id",
                "The kecamatan id field is required.".into(),
            );
            None
        }
        None => {
            if obj.contains_key("kecamatan_id") {
                input.kecamatan_id = Some(None);
            }
            None
        }
        Some(v) => as_integer(v),
    };
    if let Some(kid) = kecamatan {
        if kecamatan::find(pool, kid as u64)
            .await
            .map_err(internal)?
            .is_none()
        {
            foto::add(
                &mut errs,
                "kecamatan_id",
                "The selected kecamatan id is invalid.".into(),
            );
        } else {
            input.kecamatan_id = Some(Some(kid));
        }
    } else if obj.get("kecamatan_id").is_some_and(|v| !v.is_null()) {
        foto::add(
            &mut errs,
            "kecamatan_id",
            "The kecamatan id must be an integer.".into(),
        );
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

/// `DesaResource` dengan relasi `kecamatan` dimuat, seperti `$desa->load('kecamatan')`.
async fn respond(pool: &MySqlPool, id: u64) -> Result<Json<Value>, ApiError> {
    let mut row = desa::find(pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    row.kecamatan = match row.kecamatan_id {
        Some(k) => kecamatan::find(pool, k as u64).await.map_err(internal)?,
        None => None,
    };
    row.kecamatan_loaded = true;
    Ok(Json(json!({ "data": desa::to_resource(&row) })))
}

/// `POST /api/desa`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = validate(&state.pool, &body, true).await?;
    let url = format!("{}/api/desa", state.app_url.trim_end_matches('/'));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_desa (n_desa, luas, jumlah_penduduk, jumlah_kk, kecamatan_id, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(input.nama_desa.clone().flatten())
    .bind(input.luas.flatten())
    .bind(input.jumlah_penduduk.flatten())
    .bind(input.jumlah_kk.flatten())
    .bind(input.kecamatan_id.flatten())
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id();
    let created = attributes(&mut *tx, id).await?;
    let created = created.ok_or_else(|| internal("desa baru tidak terbaca"))?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::DESA,
        "created",
        id as i64,
        None,
        Some(created),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state.pool, id).await
}

/// `PUT` dan `PATCH /api/desa/{id}`. Field yang tidak dikirim tidak diubah.
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
    let input = validate(&state.pool, &body, false).await?;

    // Pemetaan Laravel: `nama_desa` null diabaikan (isset), kolom lain ditulis apa adanya.
    let mut next = before.clone();
    let mut sets: Vec<(&str, Value)> = Vec::new();
    if let Some(Some(n)) = &input.nama_desa {
        sets.push(("n_desa", json!(n)));
    }
    if let Some(v) = input.luas {
        sets.push(("luas", json!(v)));
    }
    if let Some(v) = input.jumlah_penduduk {
        sets.push(("jumlah_penduduk", json!(v)));
    }
    if let Some(v) = input.jumlah_kk {
        sets.push(("jumlah_kk", json!(v)));
    }
    if let Some(v) = input.kecamatan_id {
        sets.push(("kecamatan_id", json!(v)));
    }
    for (k, v) in &sets {
        next.insert((*k).to_string(), v.clone());
    }
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in &next {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if new.is_empty() {
        return respond(&state.pool, id).await;
    }

    let url = format!("{}/api/desa/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let set_sql: Vec<String> = sets.iter().map(|(k, _)| format!("{k} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_desa SET {}, updated_at = NOW() WHERE id = ?",
        set_sql.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &sets {
        q = match v {
            Value::Null => q.bind(None::<String>),
            Value::String(s) => q.bind(s.clone()),
            Value::Number(n) if n.is_f64() => q.bind(n.as_f64()),
            Value::Number(n) => q.bind(n.as_i64()),
            other => q.bind(other.to_string()),
        };
    }
    q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    let after = attributes(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut old_after = Map::new();
    let mut new_after = Map::new();
    for (k, v) in &after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old_after.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new_after.insert(k.clone(), v.clone());
        }
    }
    old_after.insert(
        "updated_at".into(),
        before.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    new_after.insert(
        "updated_at".into(),
        after.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::DESA,
        "updated",
        id as i64,
        Some(old_after),
        Some(new_after),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state.pool, id).await
}

/// `DELETE /api/desa/{id}`.
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
    let url = format!("{}/api/desa/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_desa WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::DESA,
        "deleted",
        id as i64,
        Some(before),
        None,
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Desa deleted successfully" })))
}

/// `GET /api/desa/kecamatan/{kecamatan_id}`: `{"data": [DesaResource]}` tanpa relasi kecamatan.
pub async fn by_kecamatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kecamatan_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let kid: u64 = kecamatan_id.parse().map_err(|_| ApiError::not_found())?;
    let rows = desa::list_for_kecamatan(&state.pool, kid)
        .await
        .map_err(internal)?;
    let data: Vec<Value> = rows.iter().map(desa::to_resource).collect();
    Ok(Json(json!({ "data": data })))
}
