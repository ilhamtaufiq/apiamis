//! Tulis blog (`BlogController`): `store`, `update`, `destroy`, `feature`, `unfeature`, dan
//! `upload-video`. Baca publik ada di `blog.rs`, komentar di `blog_comments.rs`.
//!
//! Perbedaan dengan Laravel (dicatat juga di laporan):
//! - `feature` memakai transaksi dan menyimpan `is_featured = 1`. Laravel memuat model sebelum
//!   `update massal` yang mengosongkan `is_featured`, lalu `update()` pada model itu tidak melihat
//!   perubahan `is_featured` (nilai di memori masih 1), sehingga artikel yang sudah utama justru
//!   tersimpan `is_featured = 0`. Di sini nilai akhirnya selalu 1.
//! - `update` dan `store` menerima JSON saja. Field `null` pada `is_published` dan `is_internal`
//!   dianggap tidak ada, karena Laravel akan mengirim NULL ke kolom NOT NULL dan gagal dengan 500.
//! - `slug` untuk `update` memakai segmen URL mentah sebagai pengecualian `unique`, sama seperti
//!   `$this->route('blog')` di Laravel. Segmen berupa slug tidak mengecualikan baris mana pun.
//! - `upload-video` memakai transaksi. Laravel bisa menyisakan `tbl_blog_assets` bila poster gagal.
//! - Berkas video dicek lewat isi (magic bytes), bukan tipe dari klien.
//! - Konversi `thumb` tidak dibuat untuk video dan poster, sama dengan Laravel.
//!
//! Auditable: `created`, `updated` (hanya kolom yang berubah), dan `deleted` tercatat di
//! `tbl_audit_logs`. Pembaruan massal `feature` tidak tercatat, sama dengan query builder Laravel.

use std::sync::OnceLock;

use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rand::{distributions::Alphanumeric, Rng};
use regex::Regex;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Transaction};

use crate::{
    audit,
    blog::{self, internal, BlogRow},
    format::iso8601_utc,
    foto, media, require_auth,
    tags_write::slugify,
    validation::Errors,
    AppState,
};

const BLOG_MODEL: &str = "App\\Models\\Blog";
const ASSET_MODEL: &str = "App\\Models\\BlogAsset";
const VIDEO_COLLECTION: &str = "blog/videos";
const POSTER_COLLECTION: &str = "blog/video-posters";
/// `max:102400` (KB) pada `file`.
const VIDEO_MAX_BYTES: usize = 102_400 * 1024;
/// `max:5120` (KB) pada `poster`.
const POSTER_MAX_BYTES: usize = 5_120 * 1024;
/// Batas badan untuk `upload-video`: video, poster, dan sedikit ruang untuk bagian multipart.
pub const VIDEO_BODY_LIMIT: usize = VIDEO_MAX_BYTES + POSTER_MAX_BYTES + 1024 * 1024;

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// `ConvertEmptyStringsToNull`: kunci absen, `null`, dan string kosong dianggap tidak ada.
fn value_of<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    match input.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(v) => Some(v),
    }
}

fn string_rule(e: &mut Errors, field: &str, v: &Value, max: Option<usize>) -> Option<String> {
    match v {
        Value::String(s) => match max {
            Some(m) if s.chars().count() > m => {
                e.add(
                    field,
                    format!(
                        "The {} field must not be greater than {m} characters.",
                        attr(field)
                    ),
                );
                None
            }
            _ => Some(s.clone()),
        },
        _ => {
            e.add(
                field,
                format!("The {} field must be a string.", attr(field)),
            );
            None
        }
    }
}

/// `boolean`: hanya `true`, `false`, `1`, `0`, `"1"`, dan `"0"`.
fn bool_rule(e: &mut Errors, field: &str, v: &Value) -> Option<bool> {
    let parsed = match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) if n.as_i64() == Some(1) => Some(true),
        Value::Number(n) if n.as_i64() == Some(0) => Some(false),
        Value::String(s) if s == "1" => Some(true),
        Value::String(s) if s == "0" => Some(false),
        _ => None,
    };
    if parsed.is_none() {
        e.add(
            field,
            format!("The {} field must be true or false.", attr(field)),
        );
    }
    parsed
}

/// Hasil validasi. `None` berarti tidak ada di input (untuk `update`) atau tidak diisi.
#[derive(Default)]
struct Fields {
    title: Option<String>,
    slug: Option<String>,
    content: Option<String>,
    category: Option<Option<String>>,
    cover_image: Option<Option<String>>,
    is_published: Option<bool>,
    is_internal: Option<bool>,
}

enum Mode<'a> {
    Store,
    /// `ignore` adalah segmen URL mentah, dipakai sebagai nilai pengecualian `unique`.
    Update {
        ignore: &'a str,
    },
}

/// `StoreBlogRequest` dan `UpdateBlogRequest`. Urutan error mengikuti urutan aturan.
async fn validate_fields(
    pool: &sqlx::MySqlPool,
    input: &Map<String, Value>,
    mode: Mode<'_>,
) -> Result<Fields, ApiError> {
    let update = matches!(mode, Mode::Update { .. });
    let mut e = Errors::default();
    let mut f = Fields::default();

    // title: store `required`, update `sometimes|required`.
    if !update || input.contains_key("title") {
        match value_of(input, "title") {
            None => e.add("title", "The title field is required."),
            Some(v) => f.title = string_rule(&mut e, "title", v, Some(255)),
        }
    }

    // slug: store `nullable` (dibuat dari judul bila kosong), update `sometimes|required`.
    let slug_input = value_of(input, "slug");
    let slug_skipped = update && !input.contains_key("slug");
    if slug_skipped {
        // Tidak ada di input `update`: slug dibiarkan.
    } else if let Some(v) = slug_input {
        f.slug = string_rule(&mut e, "slug", v, None);
    } else if update {
        e.add("slug", "The slug field is required.");
    } else if let Some(Value::String(title)) = value_of(input, "title") {
        if !title.is_empty() && title != "0" {
            let suffix: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(5)
                .map(char::from)
                .collect();
            f.slug = Some(format!("{}-{suffix}", slugify(title)));
        }
    }
    if let Some(slug) = &f.slug {
        let ignore = match &mode {
            Mode::Update { ignore } => Some(*ignore),
            Mode::Store => None,
        };
        if slug_taken(pool, slug, ignore).await? {
            e.add("slug", "The slug has already been taken.");
            f.slug = None;
        }
    }

    // content: store `required|string`, update `sometimes|required|string`.
    if !update || input.contains_key("content") {
        match value_of(input, "content") {
            None => e.add("content", "The content field is required."),
            Some(v) => f.content = string_rule(&mut e, "content", v, None),
        }
    }

    for field in ["category", "cover_image"] {
        if update && !input.contains_key(field) {
            continue;
        }
        let parsed = match value_of(input, field) {
            None => Some(None),
            Some(v) => string_rule(&mut e, field, v, Some(255)).map(Some),
        };
        match field {
            "category" => f.category = parsed,
            _ => f.cover_image = parsed,
        }
    }

    for field in ["is_published", "is_internal"] {
        if update && !input.contains_key(field) {
            continue;
        }
        // `nullable`: null dianggap tidak ada (lihat catatan di atas).
        let parsed = value_of(input, field).and_then(|v| bool_rule(&mut e, field, v));
        match field {
            "is_published" => f.is_published = parsed,
            _ => f.is_internal = parsed,
        }
    }

    e.finish()?;
    Ok(f)
}

/// Aturan `unique:tbl_blog,slug[,ignore]`. Perbandingan `id <> ?` memakai aturan MySQL untuk string.
async fn slug_taken(
    pool: &sqlx::MySqlPool,
    slug: &str,
    ignore: Option<&str>,
) -> Result<bool, ApiError> {
    let n: i64 = match ignore {
        Some(id) => {
            sqlx::query_scalar(
                "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog WHERE slug = ? AND id <> ?",
            )
            .bind(slug)
            .bind(id)
            .fetch_one(pool)
            .await
        }
        None => {
            sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_blog WHERE slug = ?")
                .bind(slug)
                .fetch_one(pool)
                .await
        }
    }
    .map_err(internal)?;
    Ok(n > 0)
}

/// Atribut yang dicatat audit, sama dengan kolom model `Blog`.
fn attrs(row: &BlogRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("title".into(), json!(row.title));
    m.insert("slug".into(), json!(row.slug));
    m.insert("content".into(), json!(row.content));
    m.insert("category".into(), json!(row.category));
    m.insert("cover_image".into(), json!(row.cover_image));
    m.insert("user_id".into(), json!(row.user_id));
    m.insert("is_published".into(), json!(row.is_published));
    m.insert("is_internal".into(), json!(row.is_internal));
    m.insert("is_featured".into(), json!(row.is_featured));
    m.insert("published_at".into(), iso8601_utc(row.published_at));
    m.insert("featured_at".into(), iso8601_utc(row.featured_at));
    m.insert("created_at".into(), iso8601_utc(row.created_at));
    m.insert("updated_at".into(), iso8601_utc(row.updated_at));
    m
}

async fn audit_blog(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: BLOG_MODEL,
            auditable_id: id,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(internal)
}

fn blog_url(state: &AppState, segment: &str) -> String {
    format!("{}/api/blog/{segment}", foto::base_url(state))
}

/// `attachReferencedVideoAssets`: tautkan aset video yang belum terpakai dan URL-nya ada di `content`.
pub async fn attach_referenced_video_assets(
    pool: &sqlx::MySqlPool,
    app_url: &str,
    blog_id: u64,
    content: &str,
) -> Result<(), ApiError> {
    static VIDEO_SRC: OnceLock<Regex> = OnceLock::new();
    let re = VIDEO_SRC
        .get_or_init(|| Regex::new(r#"(?i)<video[^>]+src=["']([^"']+)["']"#).expect("regex"));
    let urls: Vec<&str> = re
        .captures_iter(content)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str())
        .collect();
    if urls.is_empty() {
        return Ok(());
    }

    let asset_ids: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM tbl_blog_assets WHERE blog_id IS NULL ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(internal)?;
    for asset_id in asset_ids {
        let (url, _) = media::first_urls(pool, app_url, ASSET_MODEL, asset_id, VIDEO_COLLECTION)
            .await
            .map_err(internal)?;
        if !url.is_empty() && urls.contains(&url.as_str()) {
            sqlx::query("UPDATE tbl_blog_assets SET blog_id = ?, updated_at = NOW() WHERE id = ?")
                .bind(blog_id)
                .bind(asset_id)
                .execute(pool)
                .await
                .map_err(internal)?;
        }
    }
    Ok(())
}

/// `POST /api/blog`. Respons 201.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let f = validate_fields(&state.pool, &input, Mode::Store).await?;
    let (Some(title), Some(slug), Some(content)) = (f.title, f.slug, f.content) else {
        return Err(internal("validasi tidak lengkap"));
    };
    let published = f.is_published == Some(true);
    let published_sql = if published { "NOW()" } else { "NULL" };
    let sql = format!(
        "INSERT INTO tbl_blog (title, slug, content, category, cover_image, user_id, is_published, is_internal, \
         published_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, {published_sql}, NOW(), NOW())"
    );

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(&sql)
        .bind(&title)
        .bind(&slug)
        .bind(&content)
        .bind(f.category.clone().flatten())
        .bind(f.cover_image.clone().flatten())
        .bind(user.user_id)
        .bind(published as i64)
        .bind(f.is_internal.unwrap_or(false) as i64)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let id = res.last_insert_id();
    let created = blog::find_by_id(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("blog hilang setelah insert"))?;
    let url = format!("{}/api/blog", foto::base_url(&state));
    audit_blog(
        &mut tx,
        &headers,
        user.user_id,
        "created",
        id,
        None,
        Some(attrs(&created)),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    attach_referenced_video_assets(&state.pool, &state.app_url, id, &content).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "data": blog::resource(&created, true),
            "message": "Artikel berhasil dibuat",
        })),
    )
        .into_response())
}

#[derive(Clone)]
enum SetVal {
    Str(Option<String>),
    Bool(bool),
    Now,
}

/// `PUT` dan `PATCH /api/blog/{id}`. `{id}` dicocokkan ke `id` atau `slug`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let f = validate_fields(&state.pool, &input, Mode::Update { ignore: &id }).await?;
    let before = blog::find_by_key(&state.pool, &id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let mut sets: Vec<(&str, SetVal)> = Vec::new();
    if let Some(t) = &f.title {
        if *t != before.title {
            sets.push(("title", SetVal::Str(Some(t.clone()))));
        }
    }
    if let Some(s) = &f.slug {
        if *s != before.slug {
            sets.push(("slug", SetVal::Str(Some(s.clone()))));
        }
    }
    if let Some(c) = &f.content {
        if *c != before.content {
            sets.push(("content", SetVal::Str(Some(c.clone()))));
        }
    }
    if let Some(c) = &f.category {
        if *c != before.category {
            sets.push(("category", SetVal::Str(c.clone())));
        }
    }
    if let Some(c) = &f.cover_image {
        if *c != before.cover_image {
            sets.push(("cover_image", SetVal::Str(c.clone())));
        }
    }
    if let Some(p) = f.is_published {
        if p != before.is_published {
            sets.push(("is_published", SetVal::Bool(p)));
            if p && !before.is_published {
                sets.push(("published_at", SetVal::Now));
            }
        }
    }
    if let Some(i) = f.is_internal {
        if i != before.is_internal {
            sets.push(("is_internal", SetVal::Bool(i)));
        }
    }

    let url = blog_url(&state, &id);
    let after = if sets.is_empty() {
        before.clone()
    } else {
        let mut parts: Vec<String> = sets
            .iter()
            .map(|(col, v)| match v {
                SetVal::Now => format!("{col} = NOW()"),
                _ => format!("{col} = ?"),
            })
            .collect();
        parts.push("updated_at = NOW()".into());
        let sql = format!("UPDATE tbl_blog SET {} WHERE id = ?", parts.join(", "));
        let mut q = sqlx::query(&sql);
        for (_, v) in &sets {
            q = match v {
                SetVal::Str(s) => q.bind(s.clone()),
                SetVal::Bool(b) => q.bind(*b as i64),
                SetVal::Now => q,
            };
        }
        let mut tx = state.pool.begin().await.map_err(internal)?;
        q.bind(before.id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let after = blog::find_by_id(&mut *tx, before.id)
            .await
            .map_err(internal)?
            .ok_or_else(ApiError::not_found)?;
        let (old, new) = blog::diff(&attrs(&before), &attrs(&after));
        audit_blog(
            &mut tx,
            &headers,
            user.user_id,
            "updated",
            before.id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
        tx.commit().await.map_err(internal)?;
        after
    };

    attach_referenced_video_assets(&state.pool, &state.app_url, after.id, &after.content).await?;
    Ok(Json(json!({
        "data": blog::resource(&after, true),
        "message": "Artikel berhasil diperbarui",
    }))
    .into_response())
}

/// `DELETE /api/blog/{id}`. Komentar ikut terhapus lewat `ON DELETE CASCADE`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let before = blog::find_by_key(&state.pool, &id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let url = blog_url(&state, &id);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    audit_blog(
        &mut tx,
        &headers,
        user.user_id,
        "deleted",
        before.id,
        Some(attrs(&before)),
        None,
        &url,
    )
    .await?;
    sqlx::query("DELETE FROM tbl_blog WHERE id = ?")
        .bind(before.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "Artikel berhasil dihapus" })).into_response())
}

/// `POST /api/blog/{id}/feature`. Hanya artikel terbit dan publik yang dapat dijadikan utama.
pub async fn feature(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let before = blog::find_by_key(&state.pool, &id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if !before.is_published || before.is_internal {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Hanya publikasi yang sudah terbit dan bersifat publik yang dapat dijadikan artikel utama.",
        ));
    }

    let url = blog_url(&state, &id);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_blog SET is_featured = 0, featured_at = NULL WHERE is_featured = 1 AND id <> ?",
    )
    .bind(before.id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_blog SET is_featured = 1, featured_at = NOW(), updated_at = NOW() WHERE id = ?",
    )
    .bind(before.id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = blog::find_by_id(&mut *tx, before.id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (old, new) = blog::diff(&attrs(&before), &attrs(&after));
    audit_blog(
        &mut tx,
        &headers,
        user.user_id,
        "updated",
        before.id,
        Some(old),
        Some(new),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({
        "data": blog::resource(&after, true),
        "message": "Artikel utama berhasil diperbarui",
    }))
    .into_response())
}

/// `DELETE /api/blog/{id}/feature`. Tidak mengubah apa pun bila artikel memang belum utama.
pub async fn unfeature(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let before = blog::find_by_key(&state.pool, &id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let after = if before.is_featured || before.featured_at.is_some() {
        let url = blog_url(&state, &id);
        let mut tx = state.pool.begin().await.map_err(internal)?;
        sqlx::query("UPDATE tbl_blog SET is_featured = 0, featured_at = NULL, updated_at = NOW() WHERE id = ?")
            .bind(before.id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let after = blog::find_by_id(&mut *tx, before.id)
            .await
            .map_err(internal)?
            .ok_or_else(ApiError::not_found)?;
        let (old, new) = blog::diff(&attrs(&before), &attrs(&after));
        audit_blog(
            &mut tx,
            &headers,
            user.user_id,
            "updated",
            before.id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
        tx.commit().await.map_err(internal)?;
        after
    } else {
        before
    };

    Ok(Json(json!({
        "data": blog::resource(&after, true),
        "message": "Artikel tidak lagi menjadi artikel utama",
    }))
    .into_response())
}

/// Berkas multipart: `file` (video), `poster` (opsional). Field teks dengan nama yang sama
/// dicatat sebagai bukan berkas.
#[derive(Default)]
struct VideoForm {
    file: Option<media::Upload>,
    file_is_text: bool,
    poster: Option<media::Upload>,
    poster_is_text: bool,
}

fn bad_multipart(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan multipart tidak valid: {e}"),
    )
}

async fn read_video_form(mut multipart: Multipart) -> Result<VideoForm, ApiError> {
    let mut form = VideoForm::default();
    while let Some(field) = multipart.next_field().await.map_err(bad_multipart)? {
        let name = field.name().unwrap_or_default().to_string();
        let original = field.file_name().map(str::to_string);
        let bytes = field.bytes().await.map_err(bad_multipart)?;
        let (is_text, slot) = match name.as_str() {
            "file" => (&mut form.file_is_text, &mut form.file),
            "poster" => (&mut form.poster_is_text, &mut form.poster),
            _ => continue,
        };
        match original {
            Some(original_name) if !bytes.is_empty() => {
                *slot = Some(media::Upload {
                    original_name,
                    bytes: bytes.to_vec(),
                });
            }
            Some(_) => {}
            None if !bytes.is_empty() => *is_text = true,
            None => {}
        }
    }
    Ok(form)
}

/// Tipe video dari isi berkas, setara `mimetypes:video/mp4,video/webm,video/quicktime`.
pub fn sniff_video(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    if b.len() >= 8 && &b[4..8] == b"ftyp" {
        if b.len() < 12 {
            return None;
        }
        return match &b[8..12] {
            b"qt  " => Some("video/quicktime"),
            b"M4V " | b"M4VH" | b"M4VP" | b"M4A " | b"M4B " | b"3gp4" | b"3gp5" | b"3gp6"
            | b"3g2a" => None,
            _ => Some("video/mp4"),
        };
    }
    // Atom QuickTime lama tanpa `ftyp`.
    if b.len() >= 8 && matches!(&b[4..8], b"moov" | b"mdat" | b"wide" | b"free" | b"skip") {
        return Some("video/quicktime");
    }
    None
}

/// Gambar menurut `image`: jpeg, png, gif, webp, atau bmp. Hanya jpeg, png, dan webp yang lolos `mimes`.
pub fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

/// `POST /api/blog/upload-video` (multipart: `file`, `poster`). Respons 200.
pub async fn upload_video(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let form = read_video_form(multipart).await?;

    let mut e = Errors::default();
    let file = match form.file {
        Some(f) => Some(f),
        None => {
            if form.file_is_text {
                e.add("file", "The file field must be a file.");
            } else {
                e.add("file", "The file field is required.");
            }
            None
        }
    };
    if let Some(f) = &file {
        if sniff_video(&f.bytes).is_none() {
            e.add(
                "file",
                "The file field must be a file of type: video/mp4, video/webm, video/quicktime.",
            );
        }
        if f.bytes.len() > VIDEO_MAX_BYTES {
            e.add(
                "file",
                "The file field must not be greater than 102400 kilobytes.",
            );
        }
    }
    match &form.poster {
        Some(p) => {
            let kind = sniff_image(&p.bytes);
            if kind.is_none() {
                e.add("poster", "The poster field must be an image.");
            }
            if !matches!(kind, Some("image/jpeg" | "image/png" | "image/webp")) {
                e.add(
                    "poster",
                    "The poster field must be a file of type: jpeg, jpg, png, webp.",
                );
            }
            if p.bytes.len() > POSTER_MAX_BYTES {
                e.add(
                    "poster",
                    "The poster field must not be greater than 5120 kilobytes.",
                );
            }
        }
        None if form.poster_is_text => e.add("poster", "The poster field must be an image."),
        None => {}
    }
    e.finish()?;
    let Some(file) = file else {
        return Err(internal("validasi tidak lengkap"));
    };
    let video_mime = sniff_video(&file.bytes).unwrap_or("video/mp4");

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let asset_id = {
        let res = sqlx::query("INSERT INTO tbl_blog_assets (user_id, blog_id, created_at, updated_at) VALUES (?, NULL, NOW(), NOW())")
            .bind(user.user_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        res.last_insert_id()
    };
    let video = media::attach(
        &mut tx,
        ASSET_MODEL,
        asset_id,
        VIDEO_COLLECTION,
        &file,
        video_mime,
        false,
    )
    .await?;
    let mut dirs = vec![video.dir.clone()];
    let poster = match &form.poster {
        Some(p) => {
            let mime = sniff_image(&p.bytes).unwrap_or("image/jpeg");
            let stored = media::attach(
                &mut tx,
                ASSET_MODEL,
                asset_id,
                POSTER_COLLECTION,
                p,
                mime,
                false,
            )
            .await?;
            dirs.push(stored.dir.clone());
            Some(stored)
        }
        None => None,
    };
    if let Err(err) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(err));
    }

    let base = foto::base_url(&state);
    let url = format!("{base}/storage/{}/{}", video.media_id, video.file_name);
    let poster_url = poster
        .as_ref()
        .map(|p| format!("{base}/storage/{}/{}", p.media_id, p.file_name));
    Ok(Json(json!({
        "url": url,
        "media_id": video.media_id,
        "poster_url": poster_url,
        "message": "Video berhasil diunggah",
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_video_containers() {
        let mut mp4 = vec![0, 0, 0, 0x18];
        mp4.extend_from_slice(b"ftypisom");
        assert_eq!(sniff_video(&mp4), Some("video/mp4"));
        let mut mov = vec![0, 0, 0, 0x14];
        mov.extend_from_slice(b"ftypqt  ");
        assert_eq!(sniff_video(&mov), Some("video/quicktime"));
        assert_eq!(
            sniff_video(&[0x1A, 0x45, 0xDF, 0xA3, 0x01]),
            Some("video/webm")
        );
        assert_eq!(sniff_video(b"%PDF-1.4 bukan video"), None);
        let mut m4v = vec![0, 0, 0, 0x18];
        m4v.extend_from_slice(b"ftypM4V ");
        assert_eq!(sniff_video(&m4v), None);
    }

    #[test]
    fn sniffs_images_for_poster_rules() {
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a...."), Some("image/gif"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"%PDF"), None);
    }

    #[test]
    fn slug_suffix_keeps_laravel_shape() {
        assert_eq!(slugify("Berita Desa 2026!"), "berita-desa-2026");
    }

    #[test]
    fn diff_keeps_only_changed_keys() {
        let mut a = Map::new();
        a.insert("title".into(), json!("lama"));
        a.insert("slug".into(), json!("x"));
        let mut b = a.clone();
        b.insert("title".into(), json!("baru"));
        let (old, new) = blog::diff(&a, &b);
        assert_eq!(old.len(), 1);
        assert_eq!(new.get("title"), Some(&json!("baru")));
        assert_eq!(old.get("title"), Some(&json!("lama")));
    }
}
