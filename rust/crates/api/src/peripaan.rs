//! Peta peripaan (`PetaPeripaanController`, tabel `tbl_peta_peripaan`, berkas KML/KMZ di koleksi
//! media `peripaan/kml`). Model `App\Models\PetaPeripaan`.
//!
//! `index` memakai paginator (`per_page`, default 50; `-1` berarti semua tanpa paginasi). Daftar
//! tidak memuat `geojson` (berat); ambil per berkas lewat `show`, atau minta `?include=geojson`.
//! `store` hanya menerima berkas KML/KMZ. GeoJSON dibuat di server dari berkas itu, bukan dari
//! field `geojson` kiriman client. Respons 200 (bukan 201), karena `PetaPeripaanResource` dikembalikan langsung.
//! `destroy` hanya untuk role `admin` dan `operator`. Menghapus baris dan media-nya, dan mencatat audit `deleted`.

use std::collections::HashMap;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    audit,
    format::iso8601_utc,
    foto::{self, read_form},
    media,
    pagination::{self, PageParams},
    peripaan_kml, require_auth, AppState,
};

const MODEL: &str = "App\\Models\\PetaPeripaan";
const COLLECTION: &str = "peripaan/kml";
/// `file|max:51200` (KB).
const MAX_KB: usize = 51_200;
/// Role yang boleh menghapus peta peripaan (selain itu 403).
const ROLE_CAN_DELETE: &[&str] = &["admin", "operator"];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const SELECT_PERI: &str = "SELECT CAST(p.id AS SIGNED) AS id, CAST(p.pekerjaan_id AS SIGNED) AS pekerjaan_id, p.nama, \
     CAST(p.geojson AS CHAR) AS geojson, CAST(p.uploaded_by AS SIGNED) AS uploaded_by, p.created_at, p.updated_at, \
     pk.nama_paket AS pekerjaan_nama, u.name AS uploader_name \
     FROM tbl_peta_peripaan p \
     LEFT JOIN tbl_pekerjaan pk ON pk.id = p.pekerjaan_id \
     LEFT JOIN users u ON u.id = p.uploaded_by";

struct Row0 {
    id: i64,
    pekerjaan_id: Option<i64>,
    nama: String,
    geojson: Option<String>,
    uploaded_by: Option<i64>,
    created_at: Option<DateTime<Utc>>,
    pekerjaan_nama: Option<String>,
    uploader_name: Option<String>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<Row0, sqlx::Error> {
    Ok(Row0 {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        nama: r.try_get("nama")?,
        geojson: r.try_get("geojson")?,
        uploaded_by: r.try_get("uploaded_by")?,
        created_at: r.try_get("created_at")?,
        pekerjaan_nama: r.try_get("pekerjaan_nama")?,
        uploader_name: r.try_get("uploader_name")?,
    })
}

/// Bentuk `PetaPeripaanResource`. `pekerjaan` dan `uploader` selalu dimuat (`whenLoaded`).
/// `with_geojson = false` membuang `geojson` dari respons (dipakai daftar).
async fn resource(
    pool: &MySqlPool,
    app_url: &str,
    r: &Row0,
    with_geojson: bool,
) -> Result<Value, ApiError> {
    let media = media::first_media(pool, MODEL, r.id as u64, COLLECTION)
        .await
        .map_err(internal)?;
    let base = app_url.trim_end_matches('/');
    let geojson: Value = if with_geojson {
        r.geojson
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(json!({
        "id": r.id,
        "nama": r.nama,
        "pekerjaan_id": r.pekerjaan_id,
        "geojson": geojson,
        "uploaded_by": r.uploaded_by,
        "file_url": media.as_ref().map(|m| format!("{base}/storage/{}/{}", m.id, m.file_name)),
        "file_name": media.as_ref().map(|m| m.file_name.clone()),
        "size": media.as_ref().map(|m| m.size),
        "media_id": media.as_ref().map(|m| m.id),
        "pekerjaan": r.pekerjaan_id.map_or(Value::Null, |id| {
            json!({ "id": id, "nama_paket": r.pekerjaan_nama })
        }),
        "uploader": r.uploaded_by.map_or(Value::Null, |id| {
            json!({ "id": id, "name": r.uploader_name })
        }),
        "created_at": iso8601_utc(r.created_at),
    }))
}

async fn find(pool: &MySqlPool, id: i64) -> Result<Row0, ApiError> {
    let sql = format!("{SELECT_PERI} WHERE p.id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| map_row(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// `GET /api/peripaan?pekerjaan_id=&per_page=`: terbaru dulu. Tanpa `per_page=-1`, bentuk paginator.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    // `$request->filled('pekerjaan_id')`. Nilai yang bukan angka ditolak (422), bukan diam-diam jadi 0.
    let filter: Option<i64> = match query.get("pekerjaan_id").filter(|v| !v.is_empty()) {
        None => None,
        Some(v) => match v.trim().parse::<i64>() {
            Ok(id) if id > 0 => Some(id),
            _ => {
                let mut e = crate::validation::Errors::default();
                e.add(
                    "pekerjaan_id",
                    "The pekerjaan_id must be a positive integer.",
                );
                e.finish()?;
                None
            }
        },
    };
    let with_geojson = query.get("include").is_some_and(|v| v == "geojson");
    let where_sql = if filter.is_some() {
        " WHERE p.pekerjaan_id = ?"
    } else {
        ""
    };

    let per_raw = query.get("per_page").map(String::as_str).unwrap_or("50");
    let per: i64 = per_raw.trim().parse().unwrap_or(0);
    let all = per == -1;
    let params = pagination::page_params(&query);
    let per_page = if per <= 0 { 50 } else { per as u64 };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_peta_peripaan p{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(id) = filter {
        cq = cq.bind(id);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let mut sql = format!("{SELECT_PERI}{where_sql} ORDER BY p.created_at DESC, p.id DESC");
    if !all {
        sql.push_str(&format!(
            " LIMIT {per_page} OFFSET {}",
            (params.page - 1) * per_page
        ));
    }
    let mut q = sqlx::query(&sql);
    if let Some(id) = filter {
        q = q.bind(id);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let r = map_row(row).map_err(internal)?;
        data.push(resource(&state.pool, &state.app_url, &r, with_geojson).await?);
    }
    if all {
        return Ok(Json(json!({ "data": data })).into_response());
    }
    let base = format!("{}/api/peripaan", state.app_url.trim_end_matches('/'));
    let params = PageParams {
        page: params.page,
        per_page,
    };
    Ok(Json(pagination::paginate(data, total as u64, params, &base)).into_response())
}

/// Validasi `store` (diurutkan seperti Laravel). Mengembalikan `(pekerjaan_id, nama, file)`.
/// Field `geojson` dari client sengaja diabaikan: GeoJSON dibuat dari berkas di `store`.
async fn validate_store(
    pool: &MySqlPool,
    raw: &mut foto::RawForm,
) -> Result<(Option<i64>, String, media::Upload), ApiError> {
    let mut e = crate::validation::Errors::default();
    let pekerjaan = match raw.fields.get("pekerjaan_id").cloned().flatten() {
        None => None,
        Some(text) => {
            let id: Option<i64> = text.parse().ok();
            let ok = match id {
                Some(id) => {
                    sqlx::query_scalar::<_, i64>(
                        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan WHERE id = ?",
                    )
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(internal)?
                        > 0
                }
                None => false,
            };
            if ok {
                id
            } else {
                e.add("pekerjaan_id", "The selected pekerjaan id is invalid.");
                None
            }
        }
    };
    let nama = match raw.fields.get("nama").cloned().flatten() {
        None => {
            e.add("nama", "The nama field is required.");
            None
        }
        Some(s) if s.chars().count() > 255 => {
            e.add(
                "nama",
                "The nama field must not be greater than 255 characters.",
            );
            None
        }
        Some(s) => Some(s),
    };
    let file = match raw.file.take() {
        None => {
            e.add("file", "The file field is required.");
            None
        }
        Some(f) if !has_kml_extension(&f.original_name) => {
            e.add("file", "The file must be a file of type: kml, kmz.");
            None
        }
        Some(f) if f.bytes.len() > MAX_KB * 1024 => {
            e.add(
                "file",
                format!("The file field must not be greater than {MAX_KB} kilobytes."),
            );
            None
        }
        Some(f) => Some(f),
    };
    e.finish()?;
    let (Some(nama), Some(file)) = (nama, file) else {
        return Err(internal("validasi tidak lengkap"));
    };
    Ok((pekerjaan, nama, file))
}

fn has_kml_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".kml") || lower.ends_with(".kmz")
}

/// Ubah berkas menjadi GeoJSON di thread blocking (parsing bisa berat untuk berkas besar).
async fn kml_to_geojson_json(file: &media::Upload) -> Result<String, ApiError> {
    let name = file.original_name.clone();
    let bytes = file.bytes.clone();
    let parsed = tokio::task::spawn_blocking(move || {
        let text = peripaan_kml::kml_text(&name, &bytes)?;
        peripaan_kml::kml_to_geojson(&text)
    })
    .await
    .map_err(internal)?;
    match parsed {
        Ok(v) => Ok(v.to_string()),
        Err(msg) => {
            let mut e = crate::validation::Errors::default();
            e.add("file", format!("Berkas KML/KMZ tidak valid: {msg}"));
            e.finish().map(|_| String::new())
        }
    }
}

/// `POST /api/peripaan` (multipart: `pekerjaan_id`, `nama`, `file`). Respons 200.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut raw = read_form(multipart).await?;
    let (pekerjaan, nama, file) = validate_store(&state.pool, &mut raw).await?;
    let mime = media::mime_for_name(&file.original_name);
    let geojson_text = Some(kml_to_geojson_json(&file).await?);

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_peta_peripaan (pekerjaan_id, nama, geojson, uploaded_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan)
    .bind(&nama)
    .bind(&geojson_text)
    .bind(user.user_id as i64)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    let url = format!("{}/api/peripaan", foto::base_url(&state));
    let new_values = attributes(id, pekerjaan, &nama, &geojson_text, user.user_id as i64);
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "created",
            auditable_type: MODEL,
            auditable_id: id as u64,
            old: None,
            new: Some(new_values),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;

    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &file, mime, false).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }

    let row = find(&state.pool, id).await?;
    let data = resource(&state.pool, &state.app_url, &row, true).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `GET /api/peripaan/{id}`: satu berkas dengan `geojson` lengkap.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id = foto::parse_id(&id)?;
    let row = find(&state.pool, id).await?;
    let data = resource(&state.pool, &state.app_url, &row, true).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Atribut model yang dicatat audit (kunci sama dengan kolom).
fn attributes(
    id: i64,
    pekerjaan: Option<i64>,
    nama: &str,
    geojson: &Option<String>,
    uploaded_by: i64,
) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("pekerjaan_id".into(), json!(pekerjaan));
    m.insert("nama".into(), json!(nama));
    m.insert("geojson".into(), json!(geojson));
    m.insert("uploaded_by".into(), json!(uploaded_by));
    m.insert("id".into(), json!(id));
    m
}

/// `DELETE /api/peripaan/{id}`: hapus baris, media, dan audit `deleted`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let allowed = roles
        .iter()
        .any(|(_, n)| ROLE_CAN_DELETE.contains(&n.as_str()));
    if !allowed {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Hanya admin atau operator yang dapat menghapus peta peripaan",
        ));
    }
    let id = foto::parse_id(&id)?;
    let row = find(&state.pool, id).await?;

    let url = format!("{}/api/peripaan/{id}", foto::base_url(&state));
    let old = attributes(
        row.id,
        row.pekerjaan_id,
        &row.nama,
        &row.geojson,
        row.uploaded_by.unwrap_or(0),
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "deleted",
            auditable_type: MODEL,
            auditable_id: id as u64,
            old: Some(old),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    let dirs = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_peta_peripaan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({ "message": "Peta peripaan deleted" })).into_response())
}
