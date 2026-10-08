//! Daftar pekerjaan per kecamatan, desa, kegiatan, dan kecamatan+desa, total pagu per kecamatan dan kegiatan,
//! serta `GET /api/pekerjaan/{id}/media`. Setara `PekerjaanController@byKecamatan`, `byDesa`, `byKegiatan`,
//! `byKecamatanDesa`, `totalPaguByKecamatan`, `totalPaguByKegiatan`, dan `media`.
//!
//! Semua daftar memakai `scopeByUserRole()` lewat `access::restriction`, dan bentuk datanya sama dengan
//! `GET /api/pekerjaan` (`PekerjaanResource`) dengan 20 baris per halaman. Berbeda dari Laravel: daftar
//! ini tidak memuat `progress_estimasi` (summary tidak aktif), dan `links` tidak membawa query lain
//! karena Laravel juga tidak menambah `appends()` di sini.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    access,
    berkas::{self, BerkasRow},
    desa::internal,
    format::number_like_php,
    foto,
    pagination::{self, PageParams},
    pekerjaan::{self, PekerjaanFilter},
    pekerjaan_rel, require_auth, AppState,
};

const PER_PAGE: u64 = 20;

/// Halaman pekerjaan dengan filter tertentu, urut id naik (seperti paginate tanpa `orderBy` di Laravel).
async fn paginated(
    state: &AppState,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    filter: PekerjaanFilter,
    base: &str,
) -> Result<Response, ApiError> {
    let user = require_auth(state, headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");
    let viewer = pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(internal)?;

    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let offset = (page - 1) * PER_PAGE;
    let (rows, total) =
        pekerjaan::list(&state.pool, &filter, &scope, Some((PER_PAGE, offset)), None)
            .await
            .map_err(internal)?;
    let rel = pekerjaan::load(
        &state.pool,
        &rows,
        pekerjaan::Mode {
            summary: false,
            unbounded: false,
        },
        &viewer,
    )
    .await
    .map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(|p| pekerjaan::to_resource(p, &rel))
        .collect();
    let body = pagination::paginate(
        data,
        total,
        PageParams {
            page,
            per_page: PER_PAGE,
        },
        base,
    );
    Ok(Json(body).into_response())
}

/// Filter `tahun` seperti `whereHas('kegiatan', ...)`; nilai kosong diabaikan.
fn tahun_filter(query: &HashMap<String, String>) -> Option<String> {
    query.get("tahun").filter(|v| !v.is_empty()).cloned()
}

/// Filter dasar untuk daftar `byKecamatan`, `byDesa`, dan lainnya: urut id naik, tanpa pencarian.
fn base_filter() -> PekerjaanFilter {
    PekerjaanFilter {
        sort_by: "id".to_string(),
        sort_desc: false,
        ..Default::default()
    }
}

fn base_url(state: &AppState, path: &str) -> String {
    format!("{}{path}", state.app_url.trim_end_matches('/'))
}

/// `GET /api/pekerjaan/kecamatan/{kecamatanId}?tahun=`.
pub async fn by_kecamatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kecamatan_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let filter = PekerjaanFilter {
        kecamatan_id: Some(kecamatan_id.clone()),
        tahun: tahun_filter(&query),
        ..base_filter()
    };
    let base = base_url(&state, &format!("/api/pekerjaan/kecamatan/{kecamatan_id}"));
    paginated(&state, &headers, &query, filter, &base).await
}

/// `GET /api/pekerjaan/desa/{desaId}`.
pub async fn by_desa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(desa_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let filter = PekerjaanFilter {
        desa_id: Some(desa_id.clone()),
        ..base_filter()
    };
    let base = base_url(&state, &format!("/api/pekerjaan/desa/{desa_id}"));
    paginated(&state, &headers, &query, filter, &base).await
}

/// `GET /api/pekerjaan/kegiatan/{kegiatanId}`. Tidak memakai `tahun`, seperti Laravel.
pub async fn by_kegiatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kegiatan_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let filter = PekerjaanFilter {
        kegiatan_id: Some(kegiatan_id.clone()),
        ..base_filter()
    };
    let base = base_url(&state, &format!("/api/pekerjaan/kegiatan/{kegiatan_id}"));
    paginated(&state, &headers, &query, filter, &base).await
}

/// `GET /api/pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}?tahun=`.
pub async fn by_kecamatan_desa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((kecamatan_id, desa_id)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let filter = PekerjaanFilter {
        kecamatan_id: Some(kecamatan_id.clone()),
        desa_id: Some(desa_id.clone()),
        tahun: tahun_filter(&query),
        ..base_filter()
    };
    let base = base_url(
        &state,
        &format!("/api/pekerjaan/kecamatan/{kecamatan_id}/desa/{desa_id}"),
    );
    paginated(&state, &headers, &query, filter, &base).await
}

/// Total `pagu` pekerjaan dalam scope pengguna untuk satu kolom (`kecamatan_id` atau `kegiatan_id`).
async fn total_pagu(
    pool: &MySqlPool,
    column: &str,
    id: &str,
    scope: &access::Restriction,
) -> Result<Value, ApiError> {
    let sql = format!(
        "SELECT CAST(COALESCE(SUM(p.pagu), 0) AS DOUBLE) FROM tbl_pekerjaan p WHERE p.{column} = ?{}",
        scope.sql
    );
    let mut q = sqlx::query_scalar::<_, f64>(&sql).bind(id);
    for b in &scope.binds {
        q = q.bind(*b);
    }
    let total = q.fetch_one(pool).await.map_err(internal)?;
    Ok(number_like_php(total))
}

/// `GET /api/pekerjaan/stats/pagu-kecamatan/{kecamatanId}`.
pub async fn pagu_by_kecamatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kecamatan_id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");
    let total = total_pagu(&state.pool, "kecamatan_id", &kecamatan_id, &scope).await?;
    Ok(Json(json!({ "kecamatan_id": kecamatan_id, "total_pagu": total })).into_response())
}

/// `GET /api/pekerjaan/stats/pagu-kegiatan/{kegiatanId}`.
pub async fn pagu_by_kegiatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kegiatan_id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");
    let total = total_pagu(&state.pool, "kegiatan_id", &kegiatan_id, &scope).await?;
    Ok(Json(json!({ "kegiatan_id": kegiatan_id, "total_pagu": total })).into_response())
}

/// `GET /api/pekerjaan/{id}/media`: foto dan berkas pekerjaan, setelah cek akses `userCanAccess`.
pub async fn media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !access::user_can_access(&state.pool, user.user_id, &roles, id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ));
    }

    let app_url = state.app_url.as_str();
    let mut foto = Vec::new();
    for row in foto::rows_for_pekerjaan(&state.pool, id as i64).await? {
        foto.push(foto::nested_resource(&state.pool, app_url, &row).await?);
    }

    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
         jenis_dokumen, CAST(uploaded_by AS SIGNED) AS uploaded_by, created_at, updated_at \
         FROM tbl_berkas WHERE pekerjaan_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let mut berkas_out = Vec::new();
    for r in &rows {
        let row = BerkasRow {
            id: r.try_get("id").map_err(internal)?,
            pekerjaan_id: r.try_get("pekerjaan_id").map_err(internal)?,
            jenis_dokumen: r.try_get("jenis_dokumen").map_err(internal)?,
            uploaded_by: r.try_get("uploaded_by").map_err(internal)?,
            created_at: r.try_get("created_at").map_err(internal)?,
            updated_at: r.try_get("updated_at").map_err(internal)?,
        };
        berkas_out.push(berkas::resource(&state.pool, app_url, &row, false).await?);
    }

    Ok(Json(json!({ "foto": foto, "berkas": berkas_out })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tahun_empty_is_ignored() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), String::new());
        assert_eq!(tahun_filter(&q), None);
        q.insert("tahun".to_string(), "2025".to_string());
        assert_eq!(tahun_filter(&q).as_deref(), Some("2025"));
    }

    #[test]
    fn base_filter_sorts_by_id_ascending() {
        let f = base_filter();
        assert_eq!(f.sort_by, "id");
        assert!(!f.sort_desc);
    }
}
