//! Endpoint baca `/api/public/v1/*` untuk layanan lain (mis. arsgis), dilindungi kunci API `X-API-Key`
//! (lihat `public_key`). Bukan untuk pengguna: tidak ada peran, hanya scope per endpoint.
//!
//! - `GET /api/public/v1/spam-units` (scope `spam-units:read`): sama dengan `GET /api/spam-units`.
//! - `GET /api/public/v1/peripaan` (scope `peripaan:read`): daftar peta perpipaan per pekerjaan, ditambah
//!   `unit_spam_ids` (unit SPAM yang tertaut ke pekerjaannya). Nama pengunggah dibuang.
//!
//! Berkas KMZ/KML diambil lewat `file_url` pada daftar (alamat `/storage/...` dengan nama UUID).

use std::collections::{BTreeSet, HashMap};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{peripaan, public_key, spam_units, AppState};

/// Batas `per_page` untuk daftar publik, supaya satu permintaan tidak bisa menarik seluruh tabel.
const MAX_PER_PAGE: i64 = 200;
/// Batas bila `include=geojson` (geometri membuat tiap baris jauh lebih besar).
const MAX_PER_PAGE_GEOJSON: i64 = 50;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `per_page` yang aman: kosong menjadi `default`, nol, negatif (termasuk `-1` = semua), dan terlalu besar dijepit.
fn clamp_per_page(raw: Option<&str>, default: i64, max: i64) -> i64 {
    match raw.and_then(|v| v.trim().parse::<i64>().ok()) {
        None => default.min(max),
        Some(n) if n <= 0 || n > max => max,
        Some(n) => n,
    }
}

/// `GET /api/public/v1/spam-units`.
pub async fn spam_units_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(mut query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    public_key::authenticate(&state.pool, &headers, "spam-units:read").await?;
    let per = clamp_per_page(query.get("per_page").map(String::as_str), 15, MAX_PER_PAGE);
    query.insert("per_page".into(), per.to_string());
    spam_units::index_impl(&state, &query).await
}

/// `GET /api/public/v1/peripaan?pekerjaan_id=&per_page=&include=geojson`.
pub async fn peripaan_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(mut query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    public_key::authenticate(&state.pool, &headers, "peripaan:read").await?;
    let max = if query.get("include").is_some_and(|v| v == "geojson") {
        MAX_PER_PAGE_GEOJSON
    } else {
        MAX_PER_PAGE
    };
    let per = clamp_per_page(query.get("per_page").map(String::as_str), 50, max);
    query.insert("per_page".into(), per.to_string());

    let mut body = peripaan::index_data(&state, &query).await?;
    enrich(&state, &mut body).await?;
    Ok(Json(body).into_response())
}

/// Membuang bidang pengunggah dan menambahkan `unit_spam_ids` pada tiap baris `data`.
async fn enrich(state: &AppState, body: &mut Value) -> Result<(), ApiError> {
    let Some(items) = body.get_mut("data").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let ids: Vec<i64> = items
        .iter()
        .filter_map(|i| i.get("pekerjaan_id").and_then(Value::as_i64))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut links: HashMap<i64, BTreeSet<i64>> = HashMap::new();
    if !ids.is_empty() {
        let placeholders = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(unit_spam_id AS SIGNED) AS unit_spam_id \
             FROM tbl_unit_spam_pekerjaan WHERE pekerjaan_id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql);
        for id in &ids {
            q = q.bind(*id);
        }
        for row in q.fetch_all(&state.pool).await.map_err(internal)? {
            let p: i64 = row.try_get("pekerjaan_id").map_err(internal)?;
            let u: i64 = row.try_get("unit_spam_id").map_err(internal)?;
            links.entry(p).or_default().insert(u);
        }
    }

    for item in items {
        let Some(obj) = item.as_object_mut() else { continue };
        obj.remove("uploader");
        obj.remove("uploaded_by");
        let linked: Vec<i64> = obj
            .get("pekerjaan_id")
            .and_then(Value::as_i64)
            .and_then(|p| links.get(&p))
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        obj.insert("unit_spam_ids".into(), json!(linked));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_page_is_always_bounded() {
        assert_eq!(clamp_per_page(None, 50, 200), 50);
        assert_eq!(clamp_per_page(Some("10"), 50, 200), 10);
        assert_eq!(clamp_per_page(Some("200"), 50, 200), 200);
        assert_eq!(clamp_per_page(Some("201"), 50, 200), 200);
        assert_eq!(clamp_per_page(Some("99999"), 50, 200), 200);
        // -1 berarti "semua" pada rute internal; di sini dijepit.
        assert_eq!(clamp_per_page(Some("-1"), 50, 200), 200);
        assert_eq!(clamp_per_page(Some("0"), 50, 200), 200);
        assert_eq!(clamp_per_page(Some("abc"), 50, 200), 50);
        assert_eq!(clamp_per_page(Some(" 25 "), 50, 200), 25);
        assert_eq!(clamp_per_page(None, 500, 200), 200);
    }
}
