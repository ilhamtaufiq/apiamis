//! `GET /api/tiket` dan `GET /api/tiket/{id}`, setara `TiketController@index` dan `show`
//! dengan `TiketResource`. Relasi yang dimuat: `user`, `pekerjaan`, dan `comments.user`.
//!
//! Hanya GET. Tulis (store, update, destroy, bulk-update, komentar) belum dipindah.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    desa::internal,
    lookup::carbon_json,
    pagination::{self, PageParams},
    pekerjaan, pekerjaan_rel, require_auth, users, AppState,
};

/// `$request->get('per_page', 20)`. Tidak ada batas atas di Laravel.
pub const DEFAULT_PER_PAGE: u64 = 20;

#[derive(Debug, Clone)]
pub struct TiketRow {
    pub id: u64,
    pub user_id: u64,
    pub pekerjaan_id: Option<u64>,
    pub subjek: String,
    pub deskripsi: String,
    pub kategori: Option<String>,
    pub prioritas: String,
    pub status: String,
    pub admin_notes: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Filter daftar. `user_id` diisi untuk non-admin (hanya tiket sendiri).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TiketFilter {
    pub user_id: Option<u64>,
    pub status: Option<String>,
    pub kategori: Option<String>,
    pub pekerjaan_id: Option<u64>,
}

/// Nilai dianggap ada bila tidak kosong dan bukan `"0"` (truthy di PHP).
fn truthy(v: Option<&String>) -> Option<&str> {
    v.map(String::as_str).filter(|s| !s.is_empty() && *s != "0")
}

impl TiketFilter {
    pub fn from_query(query: &HashMap<String, String>, own_user: Option<u64>) -> Self {
        Self {
            user_id: own_user,
            status: truthy(query.get("status")).map(str::to_string),
            kategori: truthy(query.get("kategori")).map(str::to_string),
            // Angka yang tidak valid tidak cocok dengan id apa pun (MySQL membacanya sebagai 0).
            pekerjaan_id: truthy(query.get("pekerjaan_id")).map(|v| v.parse::<u64>().unwrap_or(0)),
        }
    }
}

fn push_filter(qb: &mut QueryBuilder<MySql>, f: &TiketFilter) {
    if let Some(uid) = f.user_id {
        qb.push(" AND user_id = ").push_bind(uid);
    }
    if let Some(s) = &f.status {
        qb.push(" AND status = ").push_bind(s.clone());
    }
    if let Some(k) = &f.kategori {
        qb.push(" AND kategori = ").push_bind(k.clone());
    }
    if let Some(p) = f.pekerjaan_id {
        qb.push(" AND pekerjaan_id = ").push_bind(p);
    }
}

/// Satu halaman tiket terbaru (`latest()`: `created_at` menurun, `id` sebagai pemutus seri).
pub async fn list(
    pool: &MySqlPool,
    f: &TiketFilter,
    page: u64,
    per_page: u64,
) -> Result<(Vec<TiketRow>, u64), sqlx::Error> {
    let mut count = QueryBuilder::<MySql>::new("SELECT COUNT(*) FROM tbl_tiket WHERE 1=1");
    push_filter(&mut count, f);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT id, user_id, pekerjaan_id, subjek, deskripsi, kategori, prioritas, status, \
         admin_notes, created_at, updated_at FROM tbl_tiket WHERE 1=1",
    );
    push_filter(&mut qb, f);
    qb.push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1) * per_page);
    let rows = qb.build().fetch_all(pool).await?;
    let items = rows.iter().map(map_row).collect::<Result<Vec<_>, _>>()?;
    Ok((items, total as u64))
}

pub async fn find(pool: &MySqlPool, id: u64) -> Result<Option<TiketRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, user_id, pekerjaan_id, subjek, deskripsi, kategori, prioritas, status, \
         admin_notes, created_at, updated_at FROM tbl_tiket WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_row).transpose()
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<TiketRow, sqlx::Error> {
    Ok(TiketRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        subjek: r.try_get("subjek")?,
        deskripsi: r.try_get("deskripsi")?,
        kategori: r.try_get("kategori")?,
        prioritas: r.try_get("prioritas")?,
        status: r.try_get("status")?,
        admin_notes: r.try_get("admin_notes")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// `getFirstMediaUrl('attachment')`: string kosong bila belum ada lampiran.
async fn image_url(pool: &MySqlPool, app_url: &str, tiket_id: u64) -> Result<String, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, disk, file_name FROM media WHERE model_type = 'App\\\\Models\\\\Tiket' \
         AND model_id = ? AND collection_name = 'attachment' ORDER BY order_column, id LIMIT 1",
    )
    .bind(tiket_id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(String::new());
    };
    let disk: String = r.try_get("disk")?;
    // Hanya disk `public` yang URL-nya diketahui dari kode.
    if disk != "public" {
        return Ok(String::new());
    }
    let media_id: u64 = r.try_get("id")?;
    let file_name: String = r.try_get("file_name")?;
    Ok(format!(
        "{}/storage/{media_id}/{file_name}",
        app_url.trim_end_matches('/')
    ))
}

/// `TiketCommentResource` untuk semua komentar satu tiket, urut `id`.
async fn comments(
    pool: &MySqlPool,
    app_url: &str,
    tiket_id: u64,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, tiket_id, user_id, message, created_at, updated_at FROM tbl_tiket_comment \
         WHERE tiket_id = ? ORDER BY id",
    )
    .bind(tiket_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let user_id: u64 = r.try_get("user_id")?;
        let created: Option<DateTime<Utc>> = r.try_get("created_at")?;
        let updated: Option<DateTime<Utc>> = r.try_get("updated_at")?;
        out.push(json!({
            "id": r.try_get::<u64, _>("id")?,
            "tiket_id": r.try_get::<u64, _>("tiket_id")?,
            "user_id": user_id,
            "user": users::resource(pool, app_url, user_id).await?.unwrap_or(Value::Null),
            "message": r.try_get::<String, _>("message")?,
            "created_at": carbon_json(created),
            "updated_at": carbon_json(updated),
        }));
    }
    Ok(out)
}

/// Relasi `pekerjaan` pada `TiketResource`: `PekerjaanResource` hanya dengan relasi `pekerjaan`
/// yang dimuat. Relasi lain dihilangkan dan hitungan yang bergantung padanya dibuat seperti Laravel
/// (tanpa `withCount`, tanpa progres, tanpa foto yang dimuat).
async fn pekerjaan_resource(
    pool: &MySqlPool,
    pekerjaan_id: Option<u64>,
    viewer: &pekerjaan_rel::Viewer,
) -> Result<Value, sqlx::Error> {
    let Some(pid) = pekerjaan_id else {
        return Ok(Value::Null);
    };
    let Some(p) = pekerjaan::find(pool, pid).await? else {
        return Ok(Value::Null);
    };
    let mode = pekerjaan::Mode {
        summary: false,
        unbounded: true,
    };
    let rel = pekerjaan::load(pool, std::slice::from_ref(&p), mode, viewer).await?;
    let mut v = pekerjaan::to_resource(&p, &rel);
    let obj = v
        .as_object_mut()
        .expect("PekerjaanResource berbentuk objek");
    for key in [
        "kecamatan",
        "desa",
        "kegiatan",
        "pengawas",
        "pendamping",
        "tags",
        "kontrak",
        "output",
    ] {
        obj.remove(key);
    }
    // Tanpa relasi yang dimuat, Laravel memakai nilai dasar: tidak ada hitungan dan foto.
    obj.insert("foto_count".into(), Value::Null);
    obj.insert("foto_required_count".into(), Value::Null);
    obj.insert("foto_status".into(), json!("belum_ada_foto"));
    obj.insert("has_kontrak".into(), json!(false));
    obj.insert("kontrak_count".into(), json!(0));
    obj.insert("penerima_count".into(), Value::Null);
    obj.insert("sipd_links_count".into(), json!(0));
    obj.insert("progress_total".into(), json!(0));
    obj.insert("deviasi".into(), json!(0));
    for key in [
        "progress_estimasi_fisik",
        "progress_estimasi_keuangan",
        "progress_estimasi_keuangan_nilai",
        "deviasi_estimasi_fisik",
        "deviasi_estimasi_keuangan",
    ] {
        obj.insert(key.into(), Value::Null);
    }
    Ok(v)
}

/// `TiketResource` lengkap dengan `user`, `pekerjaan`, `comments`, dan `image_url`.
pub async fn to_resource(
    pool: &MySqlPool,
    app_url: &str,
    t: &TiketRow,
    viewer: &pekerjaan_rel::Viewer,
) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": t.id,
        "user_id": t.user_id,
        "user": users::resource(pool, app_url, t.user_id).await?.unwrap_or(Value::Null),
        "pekerjaan_id": t.pekerjaan_id,
        "pekerjaan": pekerjaan_resource(pool, t.pekerjaan_id, viewer).await?,
        "subjek": t.subjek,
        "deskripsi": t.deskripsi,
        "kategori": t.kategori,
        "prioritas": t.prioritas,
        "status": t.status,
        "admin_notes": t.admin_notes,
        "comments": comments(pool, app_url, t.id).await?,
        "image_url": image_url(pool, app_url, t.id).await?,
        "created_at": carbon_json(t.created_at),
        "updated_at": carbon_json(t.updated_at),
    }))
}

/// Admin: peran bernama `admin`. Selain itu hanya tiket sendiri.
async fn is_admin(pool: &MySqlPool, user_id: u64) -> Result<(bool, Vec<(u64, String)>), ApiError> {
    let roles = auth::login::roles_of(pool, user_id)
        .await
        .map_err(internal)?;
    let admin = roles.iter().any(|(_, n)| n == "admin");
    Ok((admin, roles))
}

/// `GET /api/tiket`: paginasi Laravel dengan `per_page` default 20.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (admin, roles) = is_admin(&state.pool, user.user_id).await?;
    let filter = TiketFilter::from_query(&query, (!admin).then_some(user.user_id));
    let page = pagination::page_params(&query).page;
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(DEFAULT_PER_PAGE);

    let (rows, total) = list(&state.pool, &filter, page, per_page)
        .await
        .map_err(internal)?;
    let viewer = pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for t in &rows {
        data.push(
            to_resource(&state.pool, &state.app_url, t, &viewer)
                .await
                .map_err(internal)?,
        );
    }
    let base = format!("{}/api/tiket", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(
        data,
        total,
        PageParams { page, per_page },
        &base,
    )))
}

/// `GET /api/tiket/{id}`: `{"data": TiketResource}`. Selain admin, 403 bila tiket milik orang lain.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (admin, roles) = is_admin(&state.pool, user.user_id).await?;
    let row = match id.parse::<u64>() {
        Ok(id) => find(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    if !admin && row.user_id != user.user_id {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Unauthorized".to_string(),
        ));
    }
    let viewer = pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(internal)?;
    let data = to_resource(&state.pool, &state.app_url, &row, &viewer)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "data": data })))
}
