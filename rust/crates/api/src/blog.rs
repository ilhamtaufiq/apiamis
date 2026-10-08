//! Blog publik (`BlogController::index` dan `show`) dan muatan bersama untuk `blog_write.rs`
//! dan `blog_comments.rs`.
//!
//! Disalin dari Laravel:
//! - `index`: tanpa login hanya artikel terbit yang tidak internal. Dengan login semua artikel, dan
//!   `published` dipakai bila key ada. `category` dengan nilai kosong menjadi `IS NULL`, seperti
//!   `where('category', null)` setelah `ConvertEmptyStringsToNull`. 15 per halaman, terbaru dulu.
//! - `show`: `{id}` dicocokkan ke kolom `id` ATAU `slug`. Artikel internal tanpa login: 403.
//! - `comments_count` hanya ada bila pemanggil sudah login (`when(auth('sanctum')->check())`).
//!
//! Login opsional: token dibaca dari Bearer atau cookie sesi, sama dengan `auth('sanctum')->check()`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row};

use crate::{format::iso8601_utc, pagination, session, AppState};

/// `paginate(15)` pada `index`. Parameter `per_page` tidak dipakai di sini.
const PER_PAGE: u64 = 15;

pub(crate) fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Pengguna dari token bila valid, `None` bila tamu. Setara `auth('sanctum')->check()`.
pub async fn optional_user(state: &AppState, headers: &HeaderMap) -> Option<auth::AuthUser> {
    let token = session::token_from_headers(headers, &state.session.name)?;
    auth::authenticate(&state.pool, &token).await.ok()
}

/// `filter_var(..., FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE)`, dipakai `Request::boolean`.
/// `None` bila nilai tidak dikenali (Laravel lalu membandingkan dengan `null`).
pub fn filter_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" | "" => Some(false),
        _ => None,
    }
}

/// `(int)` PHP untuk string: spasi di depan, tanda, lalu digit. Selain itu 0.
pub fn php_int(raw: &str) -> i64 {
    let s = raw.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let end = digits.bytes().take_while(|b| b.is_ascii_digit()).count();
    let value: i64 = digits[..end].parse().unwrap_or(0);
    if neg {
        -value
    } else {
        value
    }
}

/// Baris `tbl_blog` beserta nama, avatar, gender, dan jabatan penulis.
#[derive(Debug, Clone)]
pub struct BlogRow {
    pub id: u64,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub category: Option<String>,
    pub cover_image: Option<String>,
    pub user_id: u64,
    pub is_published: bool,
    pub is_internal: bool,
    pub is_featured: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub featured_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub user_name: Option<String>,
    pub user_avatar: Option<String>,
    pub user_gender: Option<String>,
    pub user_jabatan: Option<String>,
    /// `comments()->count()`: komentar yang belum dihapus (soft delete tidak ikut).
    pub comments_count: i64,
}

pub const SELECT_BLOG: &str = "SELECT b.id, b.title, b.slug, b.content, b.category, b.cover_image, b.user_id, \
     CAST(b.is_published AS SIGNED) AS is_published, CAST(b.is_internal AS SIGNED) AS is_internal, \
     CAST(b.is_featured AS SIGNED) AS is_featured, b.published_at, b.featured_at, b.created_at, b.updated_at, \
     u.name AS user_name, u.avatar AS user_avatar, u.gender AS user_gender, u.jabatan AS user_jabatan, \
     (SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment c WHERE c.blog_id = b.id AND c.deleted_at IS NULL) \
     AS comments_count \
     FROM tbl_blog b LEFT JOIN users u ON u.id = b.user_id";

impl BlogRow {
    pub fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: r.try_get("id")?,
            title: r.try_get("title")?,
            slug: r.try_get("slug")?,
            content: r.try_get("content")?,
            category: r.try_get("category")?,
            cover_image: r.try_get("cover_image")?,
            user_id: r.try_get("user_id")?,
            is_published: r.try_get::<i64, _>("is_published")? != 0,
            is_internal: r.try_get::<i64, _>("is_internal")? != 0,
            is_featured: r.try_get::<i64, _>("is_featured")? != 0,
            published_at: r.try_get("published_at")?,
            featured_at: r.try_get("featured_at")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
            user_name: r.try_get("user_name")?,
            user_avatar: r.try_get("user_avatar")?,
            user_gender: r.try_get("user_gender")?,
            user_jabatan: r.try_get("user_jabatan")?,
            comments_count: r.try_get("comments_count")?,
        })
    }
}

/// `Blog::where('id', $id)->orWhere('slug', $id)->first()`. Perbandingan `id` memakai aturan MySQL
/// untuk string, sama seperti Laravel.
pub async fn find_by_key(pool: &MySqlPool, key: &str) -> Result<Option<BlogRow>, sqlx::Error> {
    let sql = format!("{SELECT_BLOG} WHERE b.id = ? OR b.slug = ? LIMIT 1");
    sqlx::query(&sql)
        .bind(key)
        .bind(key)
        .fetch_optional(pool)
        .await?
        .map(|r| BlogRow::from_row(&r))
        .transpose()
}

/// Blog berdasarkan `id`, dalam pool atau transaksi.
pub async fn find_by_id<'e, E>(exec: E, id: u64) -> Result<Option<BlogRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_BLOG} WHERE b.id = ?");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?
        .map(|r| BlogRow::from_row(&r))
        .transpose()
}

/// Route model binding `Blog $blog`. `Blog::getRouteKeyName()` adalah `slug`, bukan `id`.
pub async fn find_by_slug(pool: &MySqlPool, slug: &str) -> Result<Option<BlogRow>, sqlx::Error> {
    let sql = format!("{SELECT_BLOG} WHERE b.slug = ? LIMIT 1");
    sqlx::query(&sql)
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .map(|r| BlogRow::from_row(&r))
        .transpose()
}

/// `getOriginal` dan `getAttributes` untuk kolom yang berbeda, seperti `Auditable::logAudit('updated')`.
pub fn diff(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> (Map<String, Value>, Map<String, Value>) {
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in after {
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    (old, new)
}

/// `BlogResource`. `comments_count` hanya bila `with_count` (pemanggil login).
pub fn resource(row: &BlogRow, with_count: bool) -> Value {
    let mut v = json!({
        "id": row.id,
        "title": row.title,
        "slug": row.slug,
        "content": row.content,
        "category": row.category,
        "cover_image": row.cover_image,
        "is_published": row.is_published,
        "is_internal": row.is_internal,
        "is_featured": row.is_featured,
        "published_at": iso8601_utc(row.published_at),
        "featured_at": iso8601_utc(row.featured_at),
        "user": {
            "id": row.user_id,
            "name": row.user_name,
            "avatar": row.user_avatar,
            "gender": row.user_gender,
            "jabatan": row.user_jabatan,
        },
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    });
    if with_count {
        v["comments_count"] = json!(row.comments_count);
    }
    v
}

/// Nilai parameter SQL dinamis.
pub enum Bind {
    Str(String),
    Int(i64),
}

pub fn bind_all<'q>(
    mut query: sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments>,
    binds: &[Bind],
) -> sqlx::query::Query<'q, MySql, sqlx::mysql::MySqlArguments> {
    for b in binds {
        query = match b {
            Bind::Str(s) => query.bind(s.clone()),
            Bind::Int(i) => query.bind(*i),
        };
    }
    query
}

/// `GET /api/blog`. Paginator 15 per halaman dengan `meta` tanpa `links`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let viewer = optional_user(&state, &headers).await;
    let authed = viewer.is_some();

    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<Bind> = Vec::new();
    if !authed {
        conds.push("b.is_published = 1".into());
        conds.push("b.is_internal = 0".into());
    } else if let Some(raw) = q.get("published") {
        match filter_bool(raw) {
            Some(v) => {
                conds.push("b.is_published = ?".into());
                binds.push(Bind::Int(v as i64));
            }
            // `where(col, null)` tidak mencocokkan baris apa pun.
            None => conds.push("1 = 0".into()),
        }
    }
    if let Some(raw) = q.get("category") {
        if raw.is_empty() {
            conds.push("b.category IS NULL".into());
        } else {
            conds.push("b.category = ?".into());
            binds.push(Bind::Str(raw.clone()));
        }
    }
    if q.get("featured")
        .is_some_and(|raw| filter_bool(raw) == Some(true))
    {
        conds.push("b.is_featured = 1".into());
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog b{where_sql}");
    let total: i64 = bind_all(sqlx::query(&count_sql), &binds)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?
        .try_get(0)
        .map_err(internal)?;

    let params = pagination::PageParams {
        page: pagination::page_params(&q).page,
        per_page: PER_PAGE,
    };
    let offset = (params.page - 1) * PER_PAGE;
    let items_sql = format!(
        "{SELECT_BLOG}{where_sql} ORDER BY b.created_at DESC, b.id DESC LIMIT {PER_PAGE} OFFSET {offset}"
    );
    let rows = bind_all(sqlx::query(&items_sql), &binds)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let data = rows
        .iter()
        .map(BlogRow::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .iter()
        .map(|row| resource(row, authed))
        .collect::<Vec<_>>();

    let total = total.max(0) as u64;
    let last_page = total.div_ceil(PER_PAGE).max(1);
    Ok(Json(json!({
        "data": data,
        "meta": {
            "current_page": params.page,
            "last_page": last_page,
            "per_page": PER_PAGE,
            "total": total,
        },
    }))
    .into_response())
}

/// `GET /api/blog/{id}`. `{id}` dicocokkan ke `id` atau `slug`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let viewer = optional_user(&state, &headers).await;
    let row = find_by_key(&state.pool, &id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if row.is_internal && viewer.is_none() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Postingan ini hanya untuk internal.",
        ));
    }
    Ok(Json(json!({ "data": resource(&row, viewer.is_some()) })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_bool_matches_php_filter_var() {
        assert_eq!(filter_bool("1"), Some(true));
        assert_eq!(filter_bool("true"), Some(true));
        assert_eq!(filter_bool("on"), Some(true));
        assert_eq!(filter_bool(""), Some(false));
        assert_eq!(filter_bool("0"), Some(false));
        assert_eq!(filter_bool("no"), Some(false));
        assert_eq!(filter_bool("maybe"), None);
    }

    #[test]
    fn php_int_reads_leading_digits_only() {
        assert_eq!(php_int("12"), 12);
        assert_eq!(php_int(" 7abc"), 7);
        assert_eq!(php_int("-3"), -3);
        assert_eq!(php_int("abc"), 0);
        assert_eq!(php_int(""), 0);
    }
}
