//! Tautan sub kegiatan SIPD ke pekerjaan (`SipdPekerjaanLinkController`, tabel `tbl_sipd_pekerjaan_links`).
//!
//! Unik `(id_sub_bl, id_rinci_sub_bl)`. `upsert` setara `updateOrCreate`. Respons hanya memuat
//! `id_rinci_sub_bl` dan `pekerjaan_id` (seperti `$link->only`), dan `index` memakai kolom yang sama.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    require_auth,
    validation::{int_rule, json_text, Errors},
    AppState,
};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn parse_body(body: &Bytes) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

/// `GET /api/sipd-pekerjaan-links?id_sub_bl=`: daftar tautan untuk satu sub kegiatan.
/// `id_sub_bl` bukan angka positif (termasuk kosong) menghasilkan 422 `abort_unless`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    // `(int) $request->query(...)`: teks yang diawali angka diambil, sisanya 0.
    let id_sub_bl = query
        .get("id_sub_bl")
        .map(|v| leading_int(v))
        .unwrap_or(0);
    if id_sub_bl <= 0 {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "message": "id_sub_bl wajib diisi" })),
        )
            .into_response());
    }
    let rows = sqlx::query(
        "SELECT CAST(id_rinci_sub_bl AS SIGNED), CAST(pekerjaan_id AS SIGNED) \
         FROM tbl_sipd_pekerjaan_links WHERE id_sub_bl = ? ORDER BY id",
    )
    .bind(id_sub_bl)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| {
            Ok(json!({
                "id_rinci_sub_bl": r.try_get::<i64, _>(0).map_err(internal)?,
                "pekerjaan_id": r.try_get::<i64, _>(1).map_err(internal)?,
            }))
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Angka di awal teks seperti `(int)` PHP: spasi di depan diabaikan, sisanya 0.
fn leading_int(raw: &str) -> i64 {
    let t = raw.trim_start();
    let end = t
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))))
        .map_or(t.len(), |(i, _)| i);
    t[..end].parse().unwrap_or(0)
}

/// Ambil tiga kolom bilangan bulat dari body dengan aturan `integer|min:1` (wajib).
fn required_ids(e: &mut Errors, input: &Value, fields: &[&str]) -> Vec<i64> {
    fields
        .iter()
        .map(|f| {
            let attr = f.replace('_', " ");
            e.check(
                f,
                int_rule(json_text(input.get(*f)), &attr, 1),
                0,
            )
        })
        .collect()
}

async fn find_link(pool: &MySqlPool, sub: i64, rinci: i64) -> Result<Option<i64>, ApiError> {
    sqlx::query_scalar(
        "SELECT CAST(pekerjaan_id AS SIGNED) FROM tbl_sipd_pekerjaan_links WHERE id_sub_bl = ? AND id_rinci_sub_bl = ?",
    )
    .bind(sub)
    .bind(rinci)
    .fetch_optional(pool)
    .await
    .map_err(internal)
}

/// `PUT /api/sipd-pekerjaan-links`: simpan atau ubah tautan.
pub async fn upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let ids = required_ids(&mut e, &input, &["id_sub_bl", "id_rinci_sub_bl", "pekerjaan_id"]);
    e.finish()?;
    let (sub, rinci, pekerjaan) = (ids[0], ids[1], ids[2]);

    if find_link(&state.pool, sub, rinci).await?.is_some() {
        sqlx::query(
            "UPDATE tbl_sipd_pekerjaan_links SET pekerjaan_id = ?, updated_at = NOW() \
             WHERE id_sub_bl = ? AND id_rinci_sub_bl = ?",
        )
        .bind(pekerjaan)
        .bind(sub)
        .bind(rinci)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    } else {
        sqlx::query(
            "INSERT INTO tbl_sipd_pekerjaan_links (id_sub_bl, id_rinci_sub_bl, pekerjaan_id, created_at, updated_at) \
             VALUES (?, ?, ?, NOW(), NOW())",
        )
        .bind(sub)
        .bind(rinci)
        .bind(pekerjaan)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    }
    Ok(Json(json!({
        "data": { "id_rinci_sub_bl": rinci, "pekerjaan_id": pekerjaan }
    }))
    .into_response())
}

/// `DELETE /api/sipd-pekerjaan-links`: lepas tautan (tidak error bila tidak ada).
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let ids = required_ids(&mut e, &input, &["id_sub_bl", "id_rinci_sub_bl"]);
    e.finish()?;
    sqlx::query("DELETE FROM tbl_sipd_pekerjaan_links WHERE id_sub_bl = ? AND id_rinci_sub_bl = ?")
        .bind(ids[0])
        .bind(ids[1])
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "data": null })).into_response())
}
