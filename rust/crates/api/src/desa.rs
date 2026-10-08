//! `GET /api/desa`, setara `DesaController@index` + `DesaResource`.
//!
//! Filter: `search` (cocok ke `n_desa` atau `n_kec`), `kecamatan_id`, dan `per_page`.
//! Urutan: `id` (Laravel tidak memberi `orderBy`; di sini dibuat eksplisit).

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

use crate::{
    format::{iso8601_utc, number_like_php},
    kecamatan, pagination, require_auth, AppState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct DesaRow {
    pub id: u64,
    pub n_desa: Option<String>,
    pub luas: Option<f64>,
    pub jumlah_penduduk: Option<i64>,
    pub jumlah_kk: Option<i64>,
    pub kecamatan_id: Option<i64>,
    pub kecamatan: Option<kecamatan::KecamatanRow>,
    /// `true` bila relasi kecamatan dimuat (key `kecamatan` ikut muncul di respon).
    pub kecamatan_loaded: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Bentuk `DesaResource`. Key `kecamatan` hanya ada bila relasinya dimuat,
/// seperti `whenLoaded` di Laravel.
pub fn to_resource(row: &DesaRow) -> Value {
    let mut v = base_resource(row);
    if row.kecamatan_loaded {
        v["kecamatan"] = row
            .kecamatan
            .as_ref()
            .map(kecamatan::to_resource)
            .unwrap_or(Value::Null);
    }
    v
}

/// Bagian `DesaResource` tanpa relasi kecamatan (dipakai di detail kecamatan).
pub fn base_resource(row: &DesaRow) -> Value {
    json!({
        "id": row.id,
        "nama_desa": row.n_desa,
        "luas": row.luas.map(number_like_php),
        "jumlah_penduduk": row.jumlah_penduduk,
        "jumlah_kk": row.jumlah_kk,
        "kecamatan_id": row.kecamatan_id,
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    })
}

pub struct DesaFilter {
    pub search: Option<String>,
    pub kecamatan_id: Option<i64>,
    pub id: Option<u64>,
}

impl DesaFilter {
    pub fn from_query(q: &HashMap<String, String>) -> Self {
        Self {
            search: q.get("search").filter(|s| !s.is_empty()).cloned(),
            kecamatan_id: q.get("kecamatan_id").and_then(|v| v.parse().ok()),
            id: None,
        }
    }
}

/// WHERE dinamis. Mengembalikan fragmen SQL dan nilai bind berurutan.
fn where_clause(f: &DesaFilter) -> (String, Vec<String>) {
    let mut sql = String::from(" WHERE 1=1");
    let mut binds = Vec::new();
    if let Some(k) = f.kecamatan_id {
        sql.push_str(" AND d.kecamatan_id = ?");
        binds.push(k.to_string());
    }
    if let Some(id) = f.id {
        sql.push_str(" AND d.id = ?");
        binds.push(id.to_string());
    }
    if let Some(s) = &f.search {
        sql.push_str(" AND (d.n_desa LIKE ? OR k.n_kec LIKE ?)");
        let like = format!("%{s}%");
        binds.push(like.clone());
        binds.push(like);
    }
    (sql, binds)
}

pub async fn list(
    pool: &MySqlPool,
    f: &DesaFilter,
    limit: u64,
    offset: u64,
) -> Result<(Vec<DesaRow>, u64), sqlx::Error> {
    let (where_sql, binds) = where_clause(f);

    let count_sql = format!(
        "SELECT COUNT(*) FROM tbl_desa d LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id{where_sql}"
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total: i64 = count_q.fetch_one(pool).await?;

    let sql = format!(
        "SELECT d.id, d.n_desa, d.luas, d.jumlah_penduduk, d.jumlah_kk, d.kecamatan_id, \
         d.created_at, d.updated_at, \
         k.id AS k_id, k.n_kec AS k_n_kec, k.created_at AS k_created_at, k.updated_at AS k_updated_at, \
         CAST((SELECT COUNT(*) FROM tbl_desa x WHERE x.kecamatan_id = k.id) AS SIGNED) AS k_jumlah_desa \
         FROM tbl_desa d LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id{where_sql} \
         ORDER BY d.id LIMIT ? OFFSET ?"
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.bind(limit).bind(offset).fetch_all(pool).await?;

    let items = rows
        .iter()
        .map(|r| -> Result<DesaRow, sqlx::Error> {
            let k_id: Option<u64> = r.try_get("k_id")?;
            let kecamatan = match k_id {
                Some(id) => Some(kecamatan::KecamatanRow {
                    id,
                    n_kec: r
                        .try_get::<Option<String>, _>("k_n_kec")?
                        .unwrap_or_default(),
                    created_at: r.try_get("k_created_at")?,
                    updated_at: r.try_get("k_updated_at")?,
                    jumlah_desa: r.try_get("k_jumlah_desa")?,
                }),
                None => None,
            };
            Ok(DesaRow {
                id: r.try_get("id")?,
                n_desa: r.try_get("n_desa")?,
                luas: r.try_get("luas")?,
                jumlah_penduduk: r.try_get("jumlah_penduduk")?,
                jumlah_kk: r.try_get("jumlah_kk")?,
                kecamatan_id: r.try_get("kecamatan_id")?,
                kecamatan,
                kecamatan_loaded: true,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((items, total as u64))
}

/// Satu desa berdasarkan id, dengan relasi kecamatan.
pub async fn find(pool: &MySqlPool, id: u64) -> Result<Option<DesaRow>, sqlx::Error> {
    let filter = DesaFilter {
        search: None,
        kecamatan_id: None,
        id: Some(id),
    };
    let (mut rows, _) = list(pool, &filter, 1, 0).await?;
    Ok(rows.pop())
}

/// Semua desa milik satu kecamatan, tanpa relasi kecamatan (seperti detail kecamatan di Laravel).
pub async fn list_for_kecamatan(
    pool: &MySqlPool,
    kecamatan_id: u64,
) -> Result<Vec<DesaRow>, sqlx::Error> {
    let filter = DesaFilter {
        search: None,
        kecamatan_id: Some(kecamatan_id as i64),
        id: None,
    };
    let (mut rows, _) = list(pool, &filter, u32::MAX as u64, 0).await?;
    for r in &mut rows {
        r.kecamatan_loaded = false;
    }
    Ok(rows)
}

/// `GET /api/desa/{id}`: `{"data": DesaResource}`.
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
    Ok(Json(json!({ "data": to_resource(&row) })))
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;

    let params = pagination::page_params(&query);
    let filter = DesaFilter::from_query(&query);
    let offset = (params.page - 1) * params.per_page;

    let (rows, total) = list(&state.pool, &filter, params.per_page, offset)
        .await
        .map_err(internal)?;

    let data: Vec<Value> = rows.iter().map(to_resource).collect();
    let base = format!("{}/api/desa", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total, params, &base)))
}

pub(crate) fn internal(e: sqlx::Error) -> ApiError {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    const FIXTURE: &str = include_str!("../../../fixtures/live/desa_index.json");

    fn ts(v: &Value) -> Option<DateTime<Utc>> {
        v.as_str().map(|s| {
            NaiveDateTime::parse_from_str(&s[..19], "%Y-%m-%dT%H:%M:%S")
                .unwrap()
                .and_utc()
        })
    }

    /// Membangun baris dari item fixture, lalu mengharapkan bentuk resource identik.
    #[test]
    fn fixture_page_is_reproduced_exactly() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let body = &fixture["body"];
        let items = body["data"].as_array().unwrap();

        let rows: Vec<Value> = items
            .iter()
            .map(|item| {
                let k = &item["kecamatan"];
                let row = DesaRow {
                    id: item["id"].as_u64().unwrap(),
                    n_desa: item["nama_desa"].as_str().map(str::to_string),
                    luas: item["luas"].as_f64(),
                    jumlah_penduduk: item["jumlah_penduduk"].as_i64(),
                    jumlah_kk: item["jumlah_kk"].as_i64(),
                    kecamatan_id: item["kecamatan_id"].as_i64(),
                    kecamatan_loaded: true,
                    kecamatan: k.as_object().map(|_| kecamatan::KecamatanRow {
                        id: k["id"].as_u64().unwrap(),
                        n_kec: k["nama_kecamatan"].as_str().unwrap().to_string(),
                        created_at: ts(&k["created_at"]),
                        updated_at: ts(&k["updated_at"]),
                        jumlah_desa: k["jumlah_desa"].as_i64().unwrap(),
                    }),
                    created_at: ts(&item["created_at"]),
                    updated_at: ts(&item["updated_at"]),
                };
                to_resource(&row)
            })
            .collect();

        for (got, want) in rows.iter().zip(items) {
            assert_eq!(got, want, "id {}", want["id"]);
        }

        let params = pagination::PageParams {
            page: 1,
            per_page: 15,
        };
        let base = "http://apiamis.cianjur.space/api/desa";
        let built = pagination::paginate(rows, 365, params, base);
        assert_eq!(built["meta"], body["meta"]);
        assert_eq!(built["links"], body["links"]);
    }
}
