//! Komentar blog (`BlogCommentController`, `BlogCommentQueryService`, `BlogCommentAccessService`).
//!
//! Rute publik: `index`, `thread`, dan `count` (login opsional). Rute tulis memakai login dan
//! throttle `blog-comments` (10 per menit per pengguna, dipakai bersama `store` dan `update`).
//!
//! Perbedaan dengan Laravel (dicatat juga di laporan):
//! - Binding `{blog}` memakai `slug`, seperti `Blog::getRouteKeyName()`. `{comment}` memakai `id`
//!   dan tidak menemukan komentar yang sudah dihapus lunak (default scope), sehingga 404.
//! - Urutan: binding (404) lalu login (401) lalu throttle (429) lalu validasi, sama dengan middleware Laravel.
//! - Throttle memakai `AppState::limiter` di proses ini. Di Laravel cache bersama lintas proses.
//! - Urutan akar `index` diberi tie-breaker `id` agar stabil untuk `created_at` yang sama detiknya.
//! - Urutan balasan dalam `collectThreadComments` mengikuti `sortBy('created_at')` yang stabil,
//!   dengan urutan awal per `id` lalu per tingkat.
//! - `store` dan `update` membuat notifikasi di transaksi yang sama dengan komentar.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};
use std::collections::HashMap;

use crate::{
    audit,
    blog::{self, internal, optional_user, php_int, BlogRow},
    format::iso8601_utc,
    notify, require_auth, AppState,
};

const COMMENT_MODEL: &str = "App\\Models\\BlogComment";
/// `BlogComment::MAX_DEPTH`.
const MAX_DEPTH: i64 = 10;
/// `ROOT_PER_PAGE`.
const ROOT_PER_PAGE: i64 = 20;
/// `max:5000` pada `body`.
const BODY_MAX_CHARS: usize = 5000;
const THROTTLE_PER_MINUTE: usize = 10;

const SELECT_COMMENT: &str = "SELECT c.id, c.blog_id, c.user_id, c.parent_id, c.body, \
     CAST(c.depth AS SIGNED) AS depth, CAST(c.deleted_at IS NOT NULL AS SIGNED) AS is_deleted, \
     c.created_at, c.updated_at, u.name AS user_name, u.avatar AS user_avatar, u.gender AS user_gender, \
     u.jabatan AS user_jabatan, bl.title AS blog_title, bl.slug AS blog_slug, \
     COALESCE(CAST(bl.is_published AS SIGNED), 0) AS blog_published, CAST(bl.id IS NOT NULL AS SIGNED) AS blog_exists \
     FROM tbl_blog_comment c \
     LEFT JOIN users u ON u.id = c.user_id \
     LEFT JOIN tbl_blog bl ON bl.id = c.blog_id";

/// Baris komentar beserta pengguna dan blog induknya (untuk resource admin).
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub id: u64,
    pub blog_id: u64,
    pub user_id: u64,
    pub parent_id: Option<u64>,
    pub body: String,
    pub depth: i64,
    pub is_deleted: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub user_name: Option<String>,
    pub user_avatar: Option<String>,
    pub user_gender: Option<String>,
    pub user_jabatan: Option<String>,
    pub blog_title: Option<String>,
    pub blog_slug: Option<String>,
    pub blog_published: bool,
    pub blog_exists: bool,
}

impl CommentRow {
    fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: r.try_get("id")?,
            blog_id: r.try_get("blog_id")?,
            user_id: r.try_get("user_id")?,
            parent_id: r.try_get("parent_id")?,
            body: r.try_get("body")?,
            depth: r.try_get("depth")?,
            is_deleted: r.try_get::<i64, _>("is_deleted")? != 0,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
            user_name: r.try_get("user_name")?,
            user_avatar: r.try_get("user_avatar")?,
            user_gender: r.try_get("user_gender")?,
            user_jabatan: r.try_get("user_jabatan")?,
            blog_title: r.try_get("blog_title")?,
            blog_slug: r.try_get("blog_slug")?,
            blog_published: r.try_get::<i64, _>("blog_published")? != 0,
            blog_exists: r.try_get::<i64, _>("blog_exists")? != 0,
        })
    }
}

/// Pengguna yang melihat respons: `None` untuk tamu. `admin` dari role `admin`.
#[derive(Clone, Copy)]
struct Viewer {
    id: Option<u64>,
    admin: bool,
}

impl Viewer {
    fn guest() -> Self {
        Self {
            id: None,
            admin: false,
        }
    }

    /// `BlogCommentAccessService::canDeleteComment`: pemilik atau admin.
    fn can_delete(&self, owner: u64) -> bool {
        self.id.is_some() && (self.id == Some(owner) || self.admin)
    }

    /// `canEditComment`: pemilik dan belum dihapus.
    fn can_edit(&self, c: &CommentRow) -> bool {
        !c.is_deleted && self.id == Some(c.user_id)
    }
}

async fn viewer_of(state: &AppState, headers: &HeaderMap) -> Result<Viewer, ApiError> {
    let Some(user) = optional_user(state, headers).await else {
        return Ok(Viewer::guest());
    };
    let roles = auth::permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    Ok(Viewer {
        id: Some(user.user_id),
        admin: roles.iter().any(|r| r == "admin"),
    })
}

/// `BlogCommentResource`. Komentar terhapus tidak menampilkan isi dan pengguna.
fn comment_json(c: &CommentRow, v: Viewer) -> Value {
    let deleted = c.is_deleted;
    let is_edited = !deleted
        && matches!((c.updated_at, c.created_at), (Some(u), Some(cr)) if u > cr + ChronoDuration::seconds(2));
    json!({
        "id": c.id,
        "blog_id": c.blog_id,
        "parent_id": c.parent_id,
        "depth": c.depth,
        "body": if deleted { Value::Null } else { json!(c.body) },
        "is_deleted": deleted,
        "user": if deleted { Value::Null } else { json!({
            "id": c.user_id,
            "name": c.user_name,
            "avatar": c.user_avatar,
            "gender": c.user_gender,
            "jabatan": c.user_jabatan,
        }) },
        "can_delete": v.can_delete(c.user_id),
        "can_edit": v.can_edit(c),
        "is_edited": is_edited,
        "created_at": iso8601_utc(c.created_at),
        "updated_at": iso8601_utc(c.updated_at),
    })
}

/// `BlogCommentAdminResource`: `BlogCommentResource` plus `body_preview` dan `blog`.
fn admin_json(c: &CommentRow, v: Viewer) -> Value {
    let mut out = comment_json(c, v);
    let deleted = c.is_deleted;
    let preview = if deleted {
        Value::Null
    } else {
        json!(body_preview(&c.body))
    };
    out["body_preview"] = preview;
    out["blog"] = json!({
        "id": if c.blog_exists { json!(c.blog_id) } else { Value::Null },
        "title": c.blog_title,
        "slug": c.blog_slug,
        "is_published": c.blog_published,
    });
    out
}

/// `Str::limit(preg_replace('/\s+/', ' ', body), 160)`. Spasi PCRE tanpa mode `u` (ASCII).
fn body_preview(body: &str) -> String {
    let mut collapsed = String::new();
    let mut in_space = false;
    for ch in body.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r') {
            if !in_space {
                collapsed.push(' ');
            }
            in_space = true;
        } else {
            collapsed.push(ch);
            in_space = false;
        }
    }
    const LIMIT: usize = 160;
    if collapsed.chars().count() <= LIMIT {
        return collapsed;
    }
    let cut: String = collapsed.chars().take(LIMIT).collect();
    format!("{}...", cut.trim_end())
}

/// `trim()` PHP: spasi, tab, LF, CR, NUL, dan VT.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// `replaceMatches('/<[^>]*>/', '')` lalu `trim()`.
pub fn sanitize_body(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('<') {
        match rest[start..].find('>') {
            Some(rel) => {
                out.push_str(&rest[..start]);
                rest = &rest[start + rel + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    php_trim(&out).to_string()
}

/// Binding `{blog}`: `Blog::where('slug', $value)->firstOrFail()`.
async fn bind_blog(pool: &MySqlPool, slug: &str) -> Result<BlogRow, ApiError> {
    blog::find_by_slug(pool, slug)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// Binding `{comment}`: komentar yang belum dihapus lunak (default scope).
async fn bind_comment(pool: &MySqlPool, id: &str) -> Result<CommentRow, ApiError> {
    let sql = format!("{SELECT_COMMENT} WHERE c.id = ? AND c.deleted_at IS NULL LIMIT 1");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| CommentRow::from_row(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// Komentar apa pun (termasuk terhapus), untuk menelusuri induk.
async fn find_any(pool: &MySqlPool, blog_id: u64, id: u64) -> Result<Option<CommentRow>, ApiError> {
    let sql = format!("{SELECT_COMMENT} WHERE c.blog_id = ? AND c.id = ? LIMIT 1");
    sqlx::query(&sql)
        .bind(blog_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .map(|r| CommentRow::from_row(&r))
        .transpose()
        .map_err(internal)
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Akar (`parent_id IS NULL`) dengan id tertentu, termasuk yang terhapus lunak.
async fn roots_by_ids(
    pool: &MySqlPool,
    blog_id: u64,
    ids: &[u64],
) -> Result<Vec<CommentRow>, ApiError> {
    let sql = format!(
        "{SELECT_COMMENT} WHERE c.blog_id = ? AND c.id IN ({}) ORDER BY c.id",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql).bind(blog_id);
    for id in ids {
        q = q.bind(*id);
    }
    q.fetch_all(pool)
        .await
        .map_err(internal)?
        .iter()
        .map(CommentRow::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

/// Balasan langsung dari sekumpulan induk, termasuk yang terhapus lunak.
async fn children_of(
    pool: &MySqlPool,
    blog_id: u64,
    parent_ids: &[u64],
) -> Result<Vec<CommentRow>, ApiError> {
    let sql = format!(
        "{SELECT_COMMENT} WHERE c.blog_id = ? AND c.parent_id IN ({}) ORDER BY c.id",
        placeholders(parent_ids.len())
    );
    let mut q = sqlx::query(&sql).bind(blog_id);
    for id in parent_ids {
        q = q.bind(*id);
    }
    q.fetch_all(pool)
        .await
        .map_err(internal)?
        .iter()
        .map(CommentRow::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)
}

/// `collectThreadComments`: akar, lalu balasan per tingkat hingga habis, diurutkan `created_at`.
async fn collect_threads(
    pool: &MySqlPool,
    blog_id: u64,
    root_ids: &[u64],
) -> Result<Vec<CommentRow>, ApiError> {
    if root_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut comments = roots_by_ids(pool, blog_id, root_ids).await?;
    let mut frontier: Vec<u64> = root_ids.to_vec();
    loop {
        let children = children_of(pool, blog_id, &frontier).await?;
        if children.is_empty() {
            break;
        }
        frontier = children.iter().map(|c| c.id).collect();
        comments.extend(children);
    }
    comments.sort_by_key(|c| c.created_at);
    Ok(comments)
}

/// Query string `?key=` sebagai angka dengan aturan `(int)` PHP. Kosong menjadi 0.
fn query_int(q: &HashMap<String, String>, key: &str, default: i64) -> i64 {
    match q.get(key) {
        None => default,
        Some(v) => php_int(v),
    }
}

/// `filled()`: ada dan tidak hanya spasi.
fn filled(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key).filter(|v| !v.trim().is_empty()).cloned()
}

fn forbidden_login() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "Komentar hanya dapat diakses oleh pengguna yang login.",
    )
}

/// `canViewBlogComments`: artikel internal hanya untuk yang login.
fn can_view(blog: &BlogRow, v: Viewer) -> bool {
    !blog.is_internal || v.id.is_some()
}

/// `GET /api/blog/comments` (`BlogCommentController::adminIndex`), dengan login.
pub async fn admin_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let viewer = viewer_of(&state, &headers).await?;
    let viewer = Viewer {
        id: Some(user.user_id),
        ..viewer
    };

    let per_page = query_int(&q, "per_page", 20).clamp(1, 50);
    let page = query_int(&q, "page", 1).max(1);
    let blog_id = filled(&q, "blog_id")
        .map(|v| php_int(&v))
        .filter(|n| *n != 0);
    let search = filled(&q, "search");
    let status = match q.get("status").map(String::as_str) {
        Some("active") => "active",
        Some("deleted") => "deleted",
        _ => "all",
    };

    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<blog::Bind> = Vec::new();
    if let Some(id) = blog_id {
        conds.push("c.blog_id = ?".into());
        binds.push(blog::Bind::Int(id));
    }
    if let Some(s) = search {
        let like = format!("%{s}%");
        conds.push(
            "(c.body LIKE ? OR c.user_id IN (SELECT id FROM users WHERE name LIKE ?) \
             OR c.blog_id IN (SELECT id FROM tbl_blog WHERE title LIKE ?))"
                .into(),
        );
        binds.push(blog::Bind::Str(like.clone()));
        binds.push(blog::Bind::Str(like.clone()));
        binds.push(blog::Bind::Str(like));
    }
    match status {
        "deleted" => conds.push("c.deleted_at IS NOT NULL".into()),
        "active" => conds.push("c.deleted_at IS NULL".into()),
        _ => {}
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment c{where_sql}");
    let total: i64 = blog::bind_all(sqlx::query(&count_sql), &binds)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?
        .try_get(0)
        .map_err(internal)?;

    let offset = (page - 1) * per_page;
    let items_sql = format!(
        "{SELECT_COMMENT}{where_sql} ORDER BY c.created_at DESC, c.id DESC LIMIT {per_page} OFFSET {offset}"
    );
    let rows = blog::bind_all(sqlx::query(&items_sql), &binds)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let data = rows
        .iter()
        .map(CommentRow::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .iter()
        .map(|c| admin_json(c, viewer))
        .collect::<Vec<_>>();

    let total = total.max(0);
    let last_page = (total + per_page - 1) / per_page;
    Ok(Json(json!({
        "data": data,
        "meta": {
            "current_page": page,
            "last_page": last_page.max(1),
            "per_page": per_page,
            "total": total,
        },
    }))
    .into_response())
}

/// `GET /api/blog/{blog}/comments`: akar terhalaman beserta seluruh balasannya.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let blog = bind_blog(&state.pool, &slug).await?;
    let viewer = viewer_of(&state, &headers).await?;
    if !can_view(&blog, viewer) {
        return Err(forbidden_login());
    }

    let per_page = query_int(&q, "per_page", ROOT_PER_PAGE).clamp(1, 50);
    let page = query_int(&q, "page", 1).max(1);
    let newest = q.get("sort").map(String::as_str) == Some("newest");
    let sort = if newest { "newest" } else { "oldest" };

    let root_total: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment WHERE blog_id = ? AND parent_id IS NULL AND deleted_at IS NULL",
    )
    .bind(blog.id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;

    let order = if newest { "DESC" } else { "ASC" };
    let offset = (page - 1) * per_page;
    let root_sql = format!(
        "SELECT id FROM tbl_blog_comment WHERE blog_id = ? AND parent_id IS NULL AND deleted_at IS NULL \
         ORDER BY created_at {order}, id {order} LIMIT {per_page} OFFSET {offset}"
    );
    let root_ids: Vec<u64> = sqlx::query_scalar(&root_sql)
        .bind(blog.id)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;

    let thread = collect_threads(&state.pool, blog.id, &root_ids).await?;
    let total = total_comments(&state.pool, blog.id).await?;
    let last_page = (root_total + per_page - 1) / per_page;
    let data = thread
        .iter()
        .map(|c| comment_json(c, viewer))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": data,
        "meta": {
            "total": total,
            "root_total": root_total,
            "current_page": page,
            "last_page": last_page.max(1),
            "per_page": per_page,
            "sort": sort,
        },
    }))
    .into_response())
}

/// `totalComments`: semua komentar yang belum dihapus lunak pada artikel.
async fn total_comments(pool: &MySqlPool, blog_id: u64) -> Result<i64, ApiError> {
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment WHERE blog_id = ? AND deleted_at IS NULL")
        .bind(blog_id)
        .fetch_one(pool)
        .await
        .map_err(internal)
}

/// `GET /api/blog/{blog}/comments/thread/{comment}`.
pub async fn thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, comment_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let blog = bind_blog(&state.pool, &slug).await?;
    let comment = bind_comment(&state.pool, &comment_id).await?;
    let viewer = viewer_of(&state, &headers).await?;
    if !can_view(&blog, viewer) {
        return Err(forbidden_login());
    }
    if comment.blog_id != blog.id {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Komentar tidak ditemukan.",
        ));
    }

    // Telusuri ke akar. Induk yang hilang menghentikan penelusuran, seperti Laravel.
    let mut root_id = comment.id;
    let mut parent = comment.parent_id;
    while let Some(pid) = parent {
        match find_any(&state.pool, blog.id, pid).await? {
            Some(p) => {
                root_id = p.id;
                parent = p.parent_id;
            }
            None => break,
        }
    }

    let thread = collect_threads(&state.pool, blog.id, &[root_id]).await?;
    let data = thread
        .iter()
        .map(|c| comment_json(c, viewer))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": data })).into_response())
}

/// `GET /api/blog/{blog}/comments/count`.
pub async fn count(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let blog = bind_blog(&state.pool, &slug).await?;
    let viewer = viewer_of(&state, &headers).await?;
    if !can_view(&blog, viewer) {
        return Err(forbidden_login());
    }
    let total = total_comments(&state.pool, blog.id).await?;
    Ok(Json(json!({ "total": total })).into_response())
}

/// Throttle `blog-comments`: `Limit::perMinute(10)->by('user:'.id)`. `Some` bila sudah melewati batas.
fn throttled(state: &AppState, user_id: u64) -> Option<Response> {
    match state.limiter.hit(
        &format!("blog-comments:user:{user_id}"),
        THROTTLE_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        Ok(()) => None,
        Err(retry) => Some(
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", retry.to_string())],
                Json(json!({ "message": "Too Many Attempts." })),
            )
                .into_response(),
        ),
    }
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// `ConvertEmptyStringsToNull`: `null` dan string kosong tidak ada.
fn value_of<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    match input.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(v) => Some(v),
    }
}

/// Aturan `body`: `required|string|max:5000`. Mengembalikan teks asli bila lolos.
fn body_rules(
    input: &Map<String, Value>,
    errors: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    let mut add = |msg: String| errors.entry("body".into()).or_default().push(msg);
    match value_of(input, "body") {
        None => {
            add("The body field is required.".into());
            None
        }
        Some(Value::String(s)) => {
            if s.chars().count() > BODY_MAX_CHARS {
                add(format!(
                    "The body field must not be greater than {BODY_MAX_CHARS} characters."
                ));
                None
            } else {
                Some(s.clone())
            }
        }
        Some(_) => {
            add("The body field must be a string.".into());
            None
        }
    }
}

/// Angka utuh untuk aturan `integer`: JSON angka bulat atau string berisi bilangan bulat.
fn as_integer(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn validation_failed(errors: BTreeMap<String, Vec<String>>) -> ApiError {
    ApiError::validation("Validasi gagal", errors)
}

fn body_error(message: &str) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, message)
}

/// `POST /api/blog/{blog}/comments`. Respons 201.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let blog = bind_blog(&state.pool, &slug).await?;
    let user = require_auth(&state, &headers).await?;
    if let Some(resp) = throttled(&state, user.user_id) {
        return Ok(resp);
    }
    if !blog.is_published {
        return Err(body_error(
            "Komentar hanya tersedia untuk artikel yang sudah terbit.",
        ));
    }

    let input = parse_body(&body);
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let raw_body = body_rules(&input, &mut errors);
    let parent_id = value_of(&input, "parent_id");
    let mut parent_value: Option<i64> = None;
    if let Some(v) = parent_id {
        match as_integer(v) {
            None => errors
                .entry("parent_id".into())
                .or_default()
                .push("The parent id field must be an integer.".into()),
            Some(n) => {
                let exists: i64 = sqlx::query_scalar(
                    "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment WHERE id = ?",
                )
                .bind(n)
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?;
                if exists == 0 {
                    errors
                        .entry("parent_id".into())
                        .or_default()
                        .push("The selected parent id is invalid.".into());
                } else {
                    parent_value = Some(n);
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(validation_failed(errors));
    }
    let Some(raw_body) = raw_body else {
        return Err(internal("validasi tidak lengkap"));
    };

    let text = sanitize_body(&raw_body);
    if text.is_empty() {
        return Err(body_error("Isi komentar tidak boleh kosong."));
    }

    let dup: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog_comment WHERE blog_id = ? AND user_id = ? AND body = ? \
         AND created_at >= NOW() - INTERVAL 30 SECOND",
    )
    .bind(blog.id)
    .bind(user.user_id)
    .bind(&text)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    if dup > 0 {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Komentar identik baru saja dikirim. Tunggu sebentar sebelum mengirim ulang.",
        ));
    }

    // `if ($parentId)`: nilai 0 dianggap tidak ada, tetapi sudah ditolak `exists` di atas.
    let mut depth = 0;
    let mut parent: Option<CommentRow> = None;
    if let Some(pid) = parent_value.filter(|n| *n != 0) {
        let found = find_any(&state.pool, blog.id, pid as u64).await?;
        match found {
            Some(p) if !p.is_deleted => {
                if p.depth >= MAX_DEPTH {
                    return Err(body_error(&format!(
                        "Balasan terlalu dalam (maksimal {MAX_DEPTH} level)."
                    )));
                }
                depth = p.depth + 1;
                parent = Some(p);
            }
            _ => return Err(body_error("Komentar induk tidak ditemukan.")),
        }
    }
    let parent_db_id = parent.as_ref().map(|p| p.id);

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_blog_comment (blog_id, user_id, parent_id, body, depth, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(blog.id)
    .bind(user.user_id)
    .bind(parent_db_id)
    .bind(&text)
    .bind(depth)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id();
    let created = find_in_tx(&mut tx, id).await?;

    let url = format!(
        "{}/api/blog/{}/comments",
        state.app_url.trim_end_matches('/'),
        blog.slug
    );
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "created",
            auditable_type: COMMENT_MODEL,
            auditable_id: id,
            old: None,
            new: Some(comment_attrs(&created)),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    notify_participants(&mut tx, &blog, id, user.user_id, parent.as_ref()).await?;
    tx.commit().await.map_err(internal)?;

    let viewer = Viewer {
        id: Some(user.user_id),
        admin: false,
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "data": comment_json(&created, viewer),
            "message": "Komentar berhasil dikirim",
        })),
    )
        .into_response())
}

/// Komentar dalam transaksi (termasuk yang belum dibaca di luar transaksi).
async fn find_in_tx(tx: &mut Transaction<'_, MySql>, id: u64) -> Result<CommentRow, ApiError> {
    let sql = format!("{SELECT_COMMENT} WHERE c.id = ? LIMIT 1");
    sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal)?
        .map(|r| CommentRow::from_row(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// Atribut yang dicatat audit (`getAttributes`).
fn comment_attrs(c: &CommentRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(c.id));
    m.insert("blog_id".into(), json!(c.blog_id));
    m.insert("user_id".into(), json!(c.user_id));
    m.insert("parent_id".into(), json!(c.parent_id));
    m.insert("body".into(), json!(c.body));
    m.insert("depth".into(), json!(c.depth));
    // Komentar yang diaudit selalu belum dihapus (`bind_comment` memakai default scope).
    m.insert("deleted_at".into(), Value::Null);
    m.insert("created_at".into(), iso8601_utc(c.created_at));
    m.insert("updated_at".into(), iso8601_utc(c.updated_at));
    m
}

/// `notifyParticipants`: pemilik komentar induk, lalu pemilik artikel bila belum diberi tahu.
async fn notify_participants(
    tx: &mut Transaction<'_, MySql>,
    blog: &BlogRow,
    comment_id: u64,
    actor: u64,
    parent: Option<&CommentRow>,
) -> Result<(), ApiError> {
    let actor_name = notify::actor_name(tx, actor).await.map_err(internal)?;
    let url = format!("/publikasi/{}#comment-{comment_id}", blog.slug);

    if let Some(p) = parent {
        if p.user_id != actor {
            let message = format!("{actor_name} membalas komentar Anda di \"{}\".", blog.title);
            notify::to_users(
                tx,
                &[p.user_id],
                "Balasan komentar baru",
                &message,
                Some(&url),
                "info",
            )
            .await
            .map_err(internal)?;
        }
    }

    let parent_is_post_author = parent.is_some_and(|p| p.user_id == blog.user_id);
    if blog.user_id != actor && !parent_is_post_author {
        let message = format!("{actor_name} berkomentar di \"{}\".", blog.title);
        notify::to_users(
            tx,
            &[blog.user_id],
            "Komentar baru pada publikasi",
            &message,
            Some(&url),
            "info",
        )
        .await
        .map_err(internal)?;
    }
    Ok(())
}

/// `PUT` dan `PATCH /api/blog/comments/{comment}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(comment_id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let comment = bind_comment(&state.pool, &comment_id).await?;
    let user = require_auth(&state, &headers).await?;
    if let Some(resp) = throttled(&state, user.user_id) {
        return Ok(resp);
    }
    let viewer = Viewer {
        id: Some(user.user_id),
        admin: false,
    };
    if comment.is_deleted || comment.user_id != user.user_id {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki izin mengedit komentar ini.",
        ));
    }

    let input = parse_body(&body);
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let raw_body = body_rules(&input, &mut errors);
    if !errors.is_empty() {
        return Err(validation_failed(errors));
    }
    let Some(raw_body) = raw_body else {
        return Err(internal("validasi tidak lengkap"));
    };
    let text = sanitize_body(&raw_body);
    if text.is_empty() {
        return Err(body_error("Isi komentar tidak boleh kosong."));
    }

    if text == comment.body {
        return Ok(Json(json!({
            "data": comment_json(&comment, viewer),
            "message": "Tidak ada perubahan pada komentar.",
        }))
        .into_response());
    }

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE tbl_blog_comment SET body = ?, updated_at = NOW() WHERE id = ?")
        .bind(&text)
        .bind(comment.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let after = find_in_tx(&mut tx, comment.id).await?;
    let (old, new) = blog::diff(&comment_attrs(&comment), &comment_attrs(&after));
    let url = format!(
        "{}/api/blog/comments/{}",
        state.app_url.trim_end_matches('/'),
        comment.id
    );
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "updated",
            auditable_type: COMMENT_MODEL,
            auditable_id: comment.id,
            old: Some(old),
            new: Some(new),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({
        "data": comment_json(&after, viewer),
        "message": "Komentar berhasil diperbarui",
    }))
    .into_response())
}

/// `DELETE /api/blog/comments/{comment}`: hapus lunak oleh pemilik atau admin.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(comment_id): Path<String>,
) -> Result<Response, ApiError> {
    let comment = bind_comment(&state.pool, &comment_id).await?;
    let user = require_auth(&state, &headers).await?;
    let viewer = viewer_of(&state, &headers).await?;
    let viewer = Viewer {
        id: Some(user.user_id),
        ..viewer
    };
    if !viewer.can_delete(comment.user_id) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki izin menghapus komentar ini.",
        ));
    }

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;
    let url = format!(
        "{}/api/blog/comments/{}",
        state.app_url.trim_end_matches('/'),
        comment.id
    );
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "deleted",
            auditable_type: COMMENT_MODEL,
            auditable_id: comment.id,
            old: Some(comment_attrs(&comment)),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE tbl_blog_comment SET deleted_at = NOW(), updated_at = NOW() WHERE id = ?")
        .bind(comment.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "message": "Komentar berhasil dihapus" })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_tags_then_trims_like_php() {
        assert_eq!(sanitize_body("  <b>halo</b> dunia  "), "halo dunia");
        assert_eq!(sanitize_body("<p></p>"), "");
        assert_eq!(sanitize_body("a < b"), "a < b");
        assert_eq!(sanitize_body("x <broken"), "x <broken");
        assert_eq!(sanitize_body("\0\x0B isi \t"), "isi");
    }

    #[test]
    fn body_preview_collapses_whitespace_and_limits_to_160() {
        assert_eq!(body_preview("a \n\t b"), "a b");
        let long = "x".repeat(200);
        let preview = body_preview(&long);
        assert_eq!(preview.chars().count(), 163);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn integer_rule_accepts_int_and_numeric_string() {
        assert_eq!(as_integer(&json!(7)), Some(7));
        assert_eq!(as_integer(&json!("7")), Some(7));
        assert_eq!(as_integer(&json!("7a")), None);
        assert_eq!(as_integer(&json!(true)), None);
    }

    #[tokio::test]
    async fn throttle_allows_ten_per_minute_then_blocks() {
        let state = AppState::new(
            sqlx::MySqlPool::connect_lazy("mysql://test:test@127.0.0.1:1/none").unwrap(),
            "http://localhost".to_string(),
        );
        for _ in 0..THROTTLE_PER_MINUTE {
            assert!(throttled(&state, 99_001).is_none());
        }
        assert!(throttled(&state, 99_001).is_some());
        assert!(throttled(&state, 99_002).is_none());
    }
}
