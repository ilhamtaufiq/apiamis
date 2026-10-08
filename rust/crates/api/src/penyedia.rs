//! `GET /api/penyedia` dan `GET /api/penyedia/{id}`, setara `PenyediaController` dan `PenyediaResource`.
//!
//! Dokumen (`dokumen`) dibaca dari tabel `media` koleksi `penyedia/dokumen`.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

use crate::{
    desa::internal,
    format::iso8601_utc,
    pagination::{self, PageParams},
    require_auth, AppState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PenyediaRow {
    pub id: u64,
    pub nama: String,
    pub direktur: String,
    pub no_akta: String,
    pub notaris: String,
    pub tanggal_akta: Option<NaiveDate>,
    pub alamat: String,
    pub npwp: Option<String>,
    pub bank: Option<String>,
    pub norek: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// `PenyediaResource`. `dokumen` berisi daftar media koleksi `penyedia/dokumen`.
pub fn to_resource(p: &PenyediaRow, dokumen: Vec<Value>) -> Value {
    json!({
        "id": p.id,
        "nama": p.nama,
        "direktur": p.direktur,
        "no_akta": p.no_akta,
        "notaris": p.notaris,
        "tanggal_akta": p.tanggal_akta.map(|d| d.format("%Y-%m-%d").to_string()),
        "alamat": p.alamat,
        "npwp": p.npwp,
        "bank": p.bank,
        "norek": p.norek,
        "dokumen": dokumen,
        "created_at": iso8601_utc(p.created_at),
        "updated_at": iso8601_utc(p.updated_at),
    })
}

/// Bentuk satu dokumen: `{id, url, name, mime_type, size}`.
pub fn dokumen_resource(
    media_id: u64,
    file_name: &str,
    mime: &str,
    size: u64,
    url: Option<String>,
) -> Value {
    json!({
        "id": media_id,
        "url": url,
        "name": file_name,
        "mime_type": mime,
        "size": size,
    })
}

const SELECT_COLS: &str = "id, nama, direktur, no_akta, notaris, tanggal_akta, alamat, npwp, bank, norek, created_at, updated_at";

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<PenyediaRow, sqlx::Error> {
    Ok(PenyediaRow {
        id: r.try_get("id")?,
        nama: r.try_get("nama")?,
        direktur: r.try_get("direktur")?,
        no_akta: r.try_get("no_akta")?,
        notaris: r.try_get("notaris")?,
        tanggal_akta: r.try_get("tanggal_akta")?,
        alamat: r.try_get("alamat")?,
        npwp: r.try_get("npwp")?,
        bank: r.try_get("bank")?,
        norek: r.try_get("norek")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Filter pencarian: `LIKE %search%` pada nama, direktur, alamat, notaris, dan npwp.
pub struct PenyediaFilter {
    pub search: Option<String>,
}

/// Daftar penyedia. Mengembalikan baris dan total. `page = None` berarti semua baris.
pub async fn list(
    pool: &MySqlPool,
    filter: &PenyediaFilter,
    page: Option<(u64, u64)>,
) -> Result<(Vec<PenyediaRow>, u64), sqlx::Error> {
    let (where_sql, like) = match filter.search.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => (
            " WHERE nama LIKE ? OR direktur LIKE ? OR alamat LIKE ? OR notaris LIKE ? OR npwp LIKE ?",
            Some(format!("%{s}%")),
        ),
        None => ("", None),
    };

    let count_sql = format!("SELECT COUNT(*) FROM tbl_penyedia{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(l) = &like {
        for _ in 0..5 {
            cq = cq.bind(l);
        }
    }
    let total = cq.fetch_one(pool).await? as u64;

    let mut sql = format!("SELECT {SELECT_COLS} FROM tbl_penyedia{where_sql} ORDER BY id");
    if page.is_some() {
        sql.push_str(" LIMIT ? OFFSET ?");
    }
    let mut q = sqlx::query(&sql);
    if let Some(l) = &like {
        for _ in 0..5 {
            q = q.bind(l);
        }
    }
    if let Some((limit, offset)) = page {
        q = q.bind(limit).bind(offset);
    }
    let rows = q.fetch_all(pool).await?;
    let items = rows.iter().map(map_row).collect::<Result<Vec<_>, _>>()?;
    Ok((items, total))
}

pub async fn find(pool: &MySqlPool, id: u64) -> Result<Option<PenyediaRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {SELECT_COLS} FROM tbl_penyedia WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| map_row(&r)).transpose()
}

/// Dokumen penyedia dari tabel `media`, urut `order_column` lalu `id`.
pub async fn dokumen(
    pool: &MySqlPool,
    app_url: &str,
    penyedia_id: u64,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, disk, file_name, mime_type, size FROM media \
         WHERE model_type = 'App\\\\Models\\\\Penyedia' AND model_id = ? AND collection_name = 'penyedia/dokumen' \
         ORDER BY order_column, id",
    )
    .bind(penyedia_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            let media_id: u64 = r.try_get("id")?;
            let disk: String = r.try_get("disk")?;
            let file_name: String = r.try_get("file_name")?;
            let mime: String = r.try_get("mime_type")?;
            let size: u64 = r.try_get("size")?;
            let url = (disk == "public").then(|| {
                format!(
                    "{}/storage/{media_id}/{file_name}",
                    app_url.trim_end_matches('/')
                )
            });
            Ok(dokumen_resource(media_id, &file_name, &mime, size, url))
        })
        .collect()
}

async fn with_dokumen(state: &AppState, p: &PenyediaRow) -> Result<Value, sqlx::Error> {
    let docs = dokumen(&state.pool, &state.app_url, p.id).await?;
    Ok(to_resource(p, docs))
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let filter = PenyediaFilter {
        search: query.get("search").cloned(),
    };

    if query.get("per_page").map(String::as_str) == Some("-1") {
        let (rows, _) = list(&state.pool, &filter, None).await.map_err(internal)?;
        let mut data = Vec::with_capacity(rows.len());
        for p in &rows {
            data.push(with_dokumen(&state, p).await.map_err(internal)?);
        }
        return Ok(Json(json!({ "data": data })));
    }

    let params: PageParams = pagination::page_params(&query);
    let offset = (params.page - 1) * params.per_page;
    let (rows, total) = list(&state.pool, &filter, Some((params.per_page, offset)))
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for p in &rows {
        data.push(with_dokumen(&state, p).await.map_err(internal)?);
    }
    let base = format!("{}/api/penyedia", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total, params, &base)))
}

pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let row = match id.parse::<u64>() {
        Ok(id) => find(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    let v = with_dokumen(&state, &row).await.map_err(internal)?;
    Ok(Json(json!({ "data": v })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_formats_date_and_keeps_dokumen_list() {
        let p = PenyediaRow {
            id: 4,
            nama: "CV Uji".into(),
            direktur: "Budi".into(),
            no_akta: "12".into(),
            notaris: "Notaris X".into(),
            tanggal_akta: NaiveDate::from_ymd_opt(2024, 3, 9),
            alamat: "Jl. Uji".into(),
            npwp: None,
            bank: Some("0".into()),
            norek: Some("0".into()),
            created_at: None,
            updated_at: None,
        };
        let docs = vec![dokumen_resource(
            10,
            "akta.pdf",
            "application/pdf",
            2048,
            Some("http://x/storage/10/akta.pdf".into()),
        )];
        let v = to_resource(&p, docs);
        assert_eq!(v["tanggal_akta"], "2024-03-09");
        assert_eq!(v["dokumen"][0]["name"], "akta.pdf");
        assert_eq!(v["dokumen"][0]["size"], 2048);
        assert_eq!(v["created_at"], Value::Null);
    }

    #[test]
    fn missing_akta_date_is_null() {
        let p = PenyediaRow {
            id: 1,
            nama: String::new(),
            direktur: String::new(),
            no_akta: String::new(),
            notaris: String::new(),
            tanggal_akta: None,
            alamat: String::new(),
            npwp: None,
            bank: None,
            norek: None,
            created_at: None,
            updated_at: None,
        };
        assert_eq!(to_resource(&p, vec![])["tanggal_akta"], Value::Null);
    }
}
