//! Tulis penyedia: `POST`, `PUT`/`PATCH`, dan `DELETE` `/api/penyedia`.
//!
//! Mengikuti `PenyediaController` dan model `Penyedia` (`Auditable`, `NotifiesAdminsOnChanges`,
//! dan `InteractsWithMedia` untuk koleksi `penyedia/dokumen`). Input bisa JSON atau multipart
//! (`dokumen[]` berkas, `delete_dokumen[]` id media).

use std::collections::BTreeMap;

use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    Json,
};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Row, Transaction};

use crate::{changes, foto, lookup::carbon_json, media, penyedia, require_auth, AppState};

const MODEL: &str = "App\\Models\\Penyedia";
const COLLECTION: &str = "penyedia/dokumen";
const MAX_DOKUMEN_KB: usize = 51_200;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn attr_name(key: &str) -> String {
    key.replace('_', " ")
}

/// Input mentah dari JSON atau multipart, sebelum validasi.
#[derive(Default)]
struct Raw {
    fields: BTreeMap<String, Value>,
    dokumen: Vec<media::Upload>,
    delete_dokumen: Vec<u64>,
}

async fn read_raw(state: &AppState, req: Request) -> Result<Raw, ApiError> {
    let is_multipart = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    let mut raw = Raw::default();
    if is_multipart {
        let mut mp = Multipart::from_request(req, state)
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        while let Some(field) = mp
            .next_field()
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
        {
            let name = field.name().unwrap_or_default().to_string();
            if name == "dokumen" || name == "dokumen[]" {
                let original = field.file_name().unwrap_or_default().to_string();
                let bytes: Bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                if !bytes.is_empty() {
                    raw.dokumen.push(media::Upload {
                        original_name: original,
                        bytes: bytes.to_vec(),
                    });
                }
            } else if name == "delete_dokumen" || name == "delete_dokumen[]" {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                if let Ok(id) = text.trim().parse::<u64>() {
                    raw.delete_dokumen.push(id);
                }
            } else {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                raw.fields.insert(name, json!(text));
            }
        }
    } else {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            if let Some(obj) = v.as_object() {
                for (k, val) in obj {
                    if k == "delete_dokumen" {
                        raw.delete_dokumen = val
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| {
                                        x.as_u64()
                                            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                    } else {
                        raw.fields.insert(k.clone(), val.clone());
                    }
                }
            }
        }
    }
    Ok(raw)
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Field teks yang diterima `Penyedia` (kolom tabel, sudah divalidasi).
#[derive(Default)]
struct Input {
    nama: Option<String>,
    direktur: Option<String>,
    no_akta: Option<String>,
    notaris: Option<String>,
    tanggal_akta: Option<NaiveDate>,
    alamat: Option<String>,
    npwp: Option<Option<String>>,
    bank: Option<Option<String>>,
    norek: Option<Option<String>>,
}

/// Aturan `required` (store dan update sama): nama, direktur, no_akta, notaris, tanggal_akta, alamat.
fn validate(raw: &Raw) -> Result<Input, ApiError> {
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut input = Input::default();
    let required =
        |key: &str, max: usize, errs: &mut BTreeMap<String, Vec<String>>| -> Option<String> {
            match text(raw.fields.get(key)) {
                None => {
                    foto::add(
                        errs,
                        key,
                        format!("The {} field is required.", attr_name(key)),
                    );
                    None
                }
                Some(s) if s.chars().count() > max => {
                    foto::add(
                        errs,
                        key,
                        format!(
                            "The {} field must not be greater than {max} characters.",
                            attr_name(key)
                        ),
                    );
                    None
                }
                Some(s) => Some(s),
            }
        };
    input.nama = required("nama", 255, &mut errs);
    input.direktur = required("direktur", 255, &mut errs);
    input.no_akta = required("no_akta", 255, &mut errs);
    input.notaris = required("notaris", 255, &mut errs);
    input.alamat = required("alamat", 255, &mut errs);
    match text(raw.fields.get("tanggal_akta")) {
        None => foto::add(
            &mut errs,
            "tanggal_akta",
            "The tanggal akta field is required.".into(),
        ),
        Some(s) => match parse_date(&s) {
            Some(d) => input.tanggal_akta = Some(d),
            None => foto::add(
                &mut errs,
                "tanggal_akta",
                "The tanggal akta field must be a valid date.".into(),
            ),
        },
    }
    // Nullable: npwp (32), bank (255), norek (255). Kunci yang tidak dikirim tidak diubah.
    for (key, max) in [("npwp", 32usize), ("bank", 255), ("norek", 255)] {
        if let Some(v) = raw.fields.get(key) {
            match text(Some(v)) {
                None => {
                    let slot = match key {
                        "npwp" => &mut input.npwp,
                        "bank" => &mut input.bank,
                        _ => &mut input.norek,
                    };
                    *slot = Some(None);
                }
                Some(s) if s.chars().count() > max => foto::add(
                    &mut errs,
                    key,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        attr_name(key)
                    ),
                ),
                Some(s) => {
                    let slot = match key {
                        "npwp" => &mut input.npwp,
                        "bank" => &mut input.bank,
                        _ => &mut input.norek,
                    };
                    *slot = Some(Some(s));
                }
            }
        }
    }
    if raw
        .dokumen
        .iter()
        .any(|d| d.bytes.len() > MAX_DOKUMEN_KB * 1024)
    {
        foto::add(
            &mut errs,
            "dokumen.0",
            format!("The dokumen.0 field must not be greater than {MAX_DOKUMEN_KB} kilobytes."),
        );
    }
    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

fn parse_date(raw: &str) -> Option<NaiveDate> {
    let date_part = raw.trim().split(['T', ' ']).next()?;
    ["%Y-%m-%d", "%Y/%m/%d", "%d-%m-%Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(date_part, f).ok())
}

/// Kolom tabel untuk audit.
async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), nama, direktur, no_akta, notaris, tanggal_akta, alamat, npwp, bank, norek, created_at, updated_at \
         FROM tbl_penyedia WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let tanggal: Option<NaiveDate> = r.try_get(5).map_err(internal)?;
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(10).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(11).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "nama".into(),
        json!(r.try_get::<Option<String>, _>(1).map_err(internal)?),
    );
    m.insert(
        "direktur".into(),
        json!(r.try_get::<Option<String>, _>(2).map_err(internal)?),
    );
    m.insert(
        "no_akta".into(),
        json!(r.try_get::<Option<String>, _>(3).map_err(internal)?),
    );
    m.insert(
        "notaris".into(),
        json!(r.try_get::<Option<String>, _>(4).map_err(internal)?),
    );
    m.insert(
        "tanggal_akta".into(),
        json!(tanggal.map(|d| d.format("%Y-%m-%d").to_string())),
    );
    m.insert(
        "alamat".into(),
        json!(r.try_get::<Option<String>, _>(6).map_err(internal)?),
    );
    m.insert(
        "npwp".into(),
        json!(r.try_get::<Option<String>, _>(7).map_err(internal)?),
    );
    m.insert(
        "bank".into(),
        json!(r.try_get::<Option<String>, _>(8).map_err(internal)?),
    );
    m.insert(
        "norek".into(),
        json!(r.try_get::<Option<String>, _>(9).map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(Some(m))
}

/// Simpan dokumen baru ke koleksi `penyedia/dokumen` dalam transaksi.
async fn attach_dokumen(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
    files: &[media::Upload],
) -> Result<Vec<std::path::PathBuf>, ApiError> {
    let mut dirs = Vec::new();
    for f in files {
        let mime = media::mime_for_name(&f.original_name);
        let stored = media::attach(tx, MODEL, id, COLLECTION, f, mime, false).await?;
        dirs.push(stored.dir);
    }
    Ok(dirs)
}

/// Respons: `PenyediaResource` dengan daftar dokumen terbaru.
async fn respond(state: &AppState, id: u64) -> Result<Json<Value>, ApiError> {
    let row = penyedia::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let v = penyedia::with_dokumen(state, &row)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "data": v })))
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

/// `POST /api/penyedia`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let raw = read_raw(&state, req).await?;
    let input = validate(&raw)?;
    let url = format!("{}/api/penyedia", state.app_url.trim_end_matches('/'));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, tanggal_akta, alamat, npwp, bank, norek, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(input.nama)
    .bind(input.direktur)
    .bind(input.no_akta)
    .bind(input.notaris)
    .bind(input.tanggal_akta)
    .bind(input.alamat)
    .bind(input.npwp.flatten())
    .bind(input.bank.flatten())
    .bind(input.norek.flatten())
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id();
    attach_dokumen(&mut tx, id, &raw.dokumen).await?;
    let created = attributes(&mut *tx, id)
        .await?
        .ok_or_else(|| internal("penyedia baru tidak terbaca"))?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::PENYEDIA,
        "created",
        id as i64,
        None,
        Some(created),
        Some(format!("/penyedia/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    respond(&state, id).await
}

/// `PUT` dan `PATCH /api/penyedia/{id}`: field wajib harus dikirim, dokumen bisa ditambah dan dihapus.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let raw = read_raw(&state, req).await?;
    update_impl(&state, &headers, &id, raw).await
}

/// `POST /api/penyedia/{id}` dengan `_method=PUT` (multipart dari frontend, method override Laravel).
/// POST tanpa override ditolak 405, seperti Laravel yang tidak punya rute POST ke `/penyedia/{id}`.
pub async fn update_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let raw = read_raw(&state, req).await?;
    let method = raw
        .fields
        .get("_method")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_uppercase();
    if method != "PUT" && method != "PATCH" {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "The POST method is not supported for this route.",
        ));
    }
    update_impl(&state, &headers, &id, raw).await
}

async fn update_impl(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    raw: Raw,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(state, headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let input = validate(&raw)?;
    let url = format!("{}/api/penyedia/{id}", state.app_url.trim_end_matches('/'));

    // Field yang dikirim saja yang ditulis; null eksplisit pada nullable menulis NULL.
    let mut sets: Vec<(&str, Value)> = vec![
        ("nama", json!(input.nama)),
        ("direktur", json!(input.direktur)),
        ("no_akta", json!(input.no_akta)),
        ("notaris", json!(input.notaris)),
        (
            "tanggal_akta",
            json!(input.tanggal_akta.map(|d| d.format("%Y-%m-%d").to_string())),
        ),
        ("alamat", json!(input.alamat)),
    ];
    for (key, slot) in [
        ("npwp", &input.npwp),
        ("bank", &input.bank),
        ("norek", &input.norek),
    ] {
        if let Some(v) = slot {
            sets.push((key, json!(v)));
        }
    }
    let set_sql: Vec<String> = sets.iter().map(|(k, _)| format!("{k} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_penyedia SET {}, updated_at = NOW() WHERE id = ?",
        set_sql.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &sets {
        q = match v {
            Value::Null => q.bind(None::<String>),
            Value::String(s) => q.bind(s.clone()),
            other => q.bind(other.to_string()),
        };
    }

    let mut tx = state.pool.begin().await.map_err(internal)?;
    q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    attach_dokumen(&mut tx, id, &raw.dokumen).await?;
    for media_id in &raw.delete_dokumen {
        // Hanya baris media, berkas di disk tetap (sama dengan query builder `delete()` di Laravel).
        sqlx::query("DELETE FROM media WHERE id = ? AND model_type = ? AND model_id = ?")
            .bind(media_id)
            .bind(MODEL)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }
    let after = attributes(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if let Some((old, new)) = diff(&before, &after) {
        changes::log_linked(
            &mut tx,
            headers,
            user.user_id,
            &changes::PENYEDIA,
            "updated",
            id as i64,
            Some(old),
            Some(new),
            Some(format!("/penyedia/{id}/edit")),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    respond(state, id).await
}

/// `DELETE /api/penyedia/{id}`: baris, audit, dan berkas dokumen ikut dihapus (`InteractsWithMedia`).
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
    let url = format!("{}/api/penyedia/{id}", state.app_url.trim_end_matches('/'));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let media_ids: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM media WHERE model_type = ? AND model_id = ?")
            .bind(MODEL)
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(internal)?;
    for mid in &media_ids {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(mid)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }
    sqlx::query("DELETE FROM tbl_penyedia WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log_linked(
        &mut tx,
        &headers,
        user.user_id,
        &changes::PENYEDIA,
        "deleted",
        id as i64,
        Some(before),
        None,
        Some(format!("/penyedia/{id}/edit")),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    let dirs: Vec<std::path::PathBuf> = media_ids.iter().map(|m| media::media_dir(*m)).collect();
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Penyedia deleted successfully" })))
}
