//! Halaman panduan publik (`PanduanPageController::publicIndex` dan `publicShow`). Tabel `panduan_pages`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{format::iso8601_utc, lookup::carbon_json, AppState};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const SELECT_PAGES: &str = "SELECT CAST(id AS SIGNED) AS id, slug, title, description, section, \
     CAST(sort_order AS SIGNED) AS sort_order, body, is_published, CAST(updated_by AS SIGNED) AS updated_by, \
     created_at, updated_at FROM panduan_pages";

struct Page {
    id: i64,
    slug: String,
    title: String,
    description: Option<String>,
    section: String,
    sort_order: i64,
    body: String,
    is_published: bool,
    updated_by: Option<i64>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_page(r: &sqlx::mysql::MySqlRow) -> Result<Page, sqlx::Error> {
    Ok(Page {
        id: r.try_get("id")?,
        slug: r.try_get("slug")?,
        title: r.try_get("title")?,
        description: r.try_get("description")?,
        section: r.try_get("section")?,
        sort_order: r.try_get("sort_order")?,
        body: r.try_get("body")?,
        is_published: r.try_get("is_published")?,
        updated_by: r.try_get("updated_by")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// `PanduanPageResource` untuk halaman publik. `editor` tidak dimuat, sehingga kuncinya tidak ada.
fn resource(p: &Page) -> Value {
    json!({
        "id": p.id,
        "slug": p.slug,
        "title": p.title,
        "description": p.description,
        "section": p.section,
        "sort_order": p.sort_order,
        "body": p.body,
        "is_published": p.is_published,
        "updated_by": p.updated_by,
        "created_at": iso8601_utc(p.created_at),
        "updated_at": iso8601_utc(p.updated_at),
    })
}

/// `boolean()` Laravel untuk query: `1`, `true`, `on`, dan `yes` (tanpa membedakan huruf).
fn query_bool(value: Option<&String>) -> bool {
    value.is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
}

/// `GET /api/panduan?section=&summary=`: halaman terbit, urut section, sort_order, lalu judul.
/// `summary` hanya mengirim kolom navigasi tanpa isi.
pub async fn public_index(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let section = query
        .get("section")
        .filter(|s| !s.is_empty() && s.as_str() != "0");
    let summary = query_bool(query.get("summary"));

    let mut sql = format!("{SELECT_PAGES} WHERE is_published = 1");
    if section.is_some() {
        sql.push_str(" AND section = ?");
    }
    sql.push_str(" ORDER BY section, sort_order, title");
    let mut q = sqlx::query(&sql);
    if let Some(s) = section {
        q = q.bind(s);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let pages: Vec<Page> = rows
        .iter()
        .map(map_page)
        .collect::<Result<_, _>>()
        .map_err(internal)?;

    let data: Vec<Value> = if summary {
        pages
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "slug": p.slug,
                    "title": p.title,
                    "description": p.description,
                    "section": p.section,
                    "sort_order": p.sort_order,
                    "is_published": p.is_published,
                    "updated_at": carbon_json(p.updated_at),
                })
            })
            .collect()
    } else {
        pages.iter().map(resource).collect()
    };
    Ok(Json(json!({ "data": data })).into_response())
}

/// `GET /api/panduan/{slug}`: satu halaman terbit. Slug tidak ada atau belum terbit: 404.
pub async fn public_show(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let sql = format!("{SELECT_PAGES} WHERE is_published = 1 AND slug = ?");
    let row = sqlx::query(&sql)
        .bind(&slug)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .map(|r| map_page(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": resource(&row) })).into_response())
}
