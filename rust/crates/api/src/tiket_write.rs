//! Tulis tiket: store (multipart dengan lampiran gambar), update, destroy, bulk-update, dan komentar.
//!
//! Dua celah di Laravel tidak ditiru (lihat T46 dan T47):
//! - Update oleh pemilik memakai `$request->except(...)`, sehingga `status`, `admin_notes`, dan `user_id`
//!   (semuanya `$fillable`) bisa diubah pemilik. Di sini hanya field yang tervalidasi yang diterapkan.
//! - Komentar di Laravel tidak memeriksa kepemilikan. Di sini hanya pemilik dan admin.

use std::collections::BTreeMap;

use axum::{
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, QueryBuilder, Row};

use crate::{
    audit, foto, lookup::carbon_json, media, notify, require_auth, tiket, users, AppState,
};

const TIKET_MODEL: &str = "App\\Models\\Tiket";
const COMMENT_MODEL: &str = "App\\Models\\TiketComment";
const ATTACHMENT: &str = "attachment";
/// `image|max:2048` (KB).
const MAX_ATTACHMENT_BYTES: usize = 2048 * 1024;
const KATEGORI: &[&str] = &["bug", "request", "lapangan", "document", "other"];
const PRIORITAS: &[&str] = &["low", "medium", "high"];
const STATUS: &[&str] = &["open", "pending", "closed"];
/// Kolom tiket yang bisa diubah, urut tetap untuk audit.
const COLUMNS: &[&str] = &[
    "user_id",
    "pekerjaan_id",
    "subjek",
    "deskripsi",
    "kategori",
    "prioritas",
    "status",
    "admin_notes",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    media::internal(e)
}

/// Respon `ApiError` dengan pesan `Validation error` seperti `Validator::fails()` di Laravel.
fn validation(errs: BTreeMap<String, Vec<String>>) -> ApiError {
    ApiError::validation("Validation error", errs)
}

#[derive(Debug, Clone, PartialEq)]
struct TiketRow {
    id: i64,
    user_id: i64,
    pekerjaan_id: Option<i64>,
    subjek: String,
    deskripsi: String,
    kategori: Option<String>,
    prioritas: String,
    status: String,
    admin_notes: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_TIKET: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, \
     CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, subjek, deskripsi, kategori, prioritas, status, admin_notes, \
     created_at, updated_at FROM tbl_tiket";

fn map_tiket(r: &sqlx::mysql::MySqlRow) -> Result<TiketRow, sqlx::Error> {
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

async fn find_tiket<'e, E>(exec: E, id: i64) -> Result<Option<TiketRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_TIKET} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_tiket).transpose()
}

fn col_json(row: &TiketRow, col: &str) -> Value {
    match col {
        "user_id" => json!(row.user_id),
        "pekerjaan_id" => json!(row.pekerjaan_id),
        "subjek" => json!(row.subjek),
        "deskripsi" => json!(row.deskripsi),
        "kategori" => json!(row.kategori),
        "prioritas" => json!(row.prioritas),
        "status" => json!(row.status),
        "admin_notes" => json!(row.admin_notes),
        _ => Value::Null,
    }
}

fn attributes(row: &TiketRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// Role admin (nama `admin`) dan ID user.
async fn actor_roles(state: &AppState, user_id: u64) -> Result<(bool, String), ApiError> {
    let roles = auth::login::roles_of(&state.pool, user_id)
        .await
        .map_err(internal)?;
    let admin = roles.iter().any(|(_, n)| n == "admin");
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .flatten();
    Ok((admin, name.unwrap_or_else(|| "System".into())))
}

/// `TiketResource` tanpa `comments` (relasi tidak dimuat pada respon store dan update).
async fn resource_without_comments(
    state: &AppState,
    actor: u64,
    id: i64,
) -> Result<Value, ApiError> {
    let row = tiket::find(&state.pool, id as u64)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    let viewer = crate::pekerjaan_rel::viewer(&state.pool, actor, &roles)
        .await
        .map_err(internal)?;
    let mut v = tiket::to_resource(&state.pool, &state.app_url, &row, &viewer)
        .await
        .map_err(internal)?;
    if let Some(o) = v.as_object_mut() {
        o.remove("comments");
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Multipart dan lampiran gambar
// ---------------------------------------------------------------------------

/// Tipe gambar dari isi berkas: `image` Laravel (jpeg, png, gif, bmp, webp). SVG tidak didukung.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Field teks (trim, kosong jadi null) dan lampiran `attachment`.
struct Form {
    fields: BTreeMap<String, Option<String>>,
    attachment: Option<media::Upload>,
}

async fn read_form(mut multipart: Multipart) -> Result<Form, ApiError> {
    let mut form = Form {
        fields: BTreeMap::new(),
        attachment: None,
    };
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Permintaan multipart tidak valid: {e}"),
        )
    })? {
        let name = field.name().unwrap_or_default().to_string();
        if name == ATTACHMENT {
            let original_name = field.file_name().unwrap_or_default().to_string();
            let bytes = field.bytes().await.map_err(|e| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("Permintaan multipart tidak valid: {e}"),
                )
            })?;
            if !bytes.is_empty() {
                form.attachment = Some(media::Upload {
                    original_name,
                    bytes: bytes.to_vec(),
                });
            }
        } else {
            let text = field.text().await.map_err(|e| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    format!("Permintaan multipart tidak valid: {e}"),
                )
            })?;
            let trimmed = text.trim();
            form.fields
                .insert(name, (!trimmed.is_empty()).then(|| trimmed.to_string()));
        }
    }
    Ok(form)
}

/// Validasi lampiran `image|max:2048`. Mengembalikan MIME bila valid.
fn check_attachment(
    upload: Option<&media::Upload>,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<&'static str> {
    let upload = upload?;
    if upload.bytes.len() > MAX_ATTACHMENT_BYTES {
        foto::add(
            errs,
            ATTACHMENT,
            "The attachment field must not be greater than 2048 kilobytes.".into(),
        );
        return None;
    }
    match sniff_image(&upload.bytes) {
        Some(mime) => Some(mime),
        None => {
            foto::add(
                errs,
                ATTACHMENT,
                "The attachment field must be an image.".into(),
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Field teks yang wajib / opsional untuk store dan update.
fn text_rule(
    fields: &BTreeMap<String, Option<String>>,
    key: &str,
    required: bool,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    match fields.get(key).cloned().flatten() {
        Some(v) => Some(v),
        None => {
            if required {
                foto::add(
                    errs,
                    key,
                    format!("The {} field is required.", key.replace('_', " ")),
                );
            }
            None
        }
    }
}

/// `POST /api/tiket` (multipart): `subjek`, `deskripsi`, `kategori`, `prioritas`, `pekerjaan_id?`, `attachment?`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let form = read_form(multipart).await?;
    let mut errs = BTreeMap::new();

    let subjek = text_rule(&form.fields, "subjek", true, &mut errs);
    if subjek.as_ref().is_some_and(|s| s.chars().count() > 255) {
        foto::add(
            &mut errs,
            "subjek",
            "The subjek field must not be greater than 255 characters.".into(),
        );
    }
    let deskripsi = text_rule(&form.fields, "deskripsi", true, &mut errs);
    let kategori = text_rule(&form.fields, "kategori", true, &mut errs);
    if kategori
        .as_ref()
        .is_some_and(|k| !KATEGORI.contains(&k.as_str()))
    {
        foto::add(
            &mut errs,
            "kategori",
            "The selected kategori is invalid.".into(),
        );
    }
    let prioritas = text_rule(&form.fields, "prioritas", true, &mut errs);
    if prioritas
        .as_ref()
        .is_some_and(|p| !PRIORITAS.contains(&p.as_str()))
    {
        foto::add(
            &mut errs,
            "prioritas",
            "The selected prioritas is invalid.".into(),
        );
    }
    let pekerjaan_id = match form.fields.get("pekerjaan_id").cloned().flatten() {
        None => None,
        Some(v) => match v.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => {
                foto::add(
                    &mut errs,
                    "pekerjaan_id",
                    "The selected pekerjaan id is invalid.".into(),
                );
                None
            }
        },
    };
    let mime = check_attachment(form.attachment.as_ref(), &mut errs);
    if !errs.is_empty() {
        return Err(validation(errs));
    }
    let (Some(subjek), Some(deskripsi), Some(kategori), Some(prioritas)) =
        (subjek, deskripsi, kategori, prioritas)
    else {
        return Err(internal("validasi tiket tidak lengkap"));
    };
    if let Some(p) = pekerjaan_id {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
            .bind(p)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
        if n == 0 {
            let mut e = BTreeMap::new();
            foto::add(
                &mut e,
                "pekerjaan_id",
                "The selected pekerjaan id is invalid.".into(),
            );
            return Err(validation(e));
        }
        let roles = auth::login::roles_of(&state.pool, user.user_id)
            .await
            .map_err(internal)?;
        foto::ensure_access(&state, user.user_id, &roles, Some(p)).await?;
    }

    let (_, actor_name) = actor_roles(&state, user.user_id).await?;
    let url = format!("{}/api/tiket", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_tiket (user_id, pekerjaan_id, subjek, deskripsi, kategori, prioritas, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'open', NOW(), NOW())",
    )
    .bind(user.user_id)
    .bind(pekerjaan_id)
    .bind(&subjek)
    .bind(&deskripsi)
    .bind(&kategori)
    .bind(&prioritas)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let row = find_tiket(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("tiket baru tidak terbaca"))?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "created",
            auditable_type: TIKET_MODEL,
            auditable_id: id as u64,
            old: None,
            new: Some(attributes(&row)),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;

    let mut created_dir = None;
    if let (Some(upload), Some(mime)) = (form.attachment.as_ref(), mime) {
        created_dir = Some(
            media::attach(
                &mut tx,
                TIKET_MODEL,
                id as u64,
                ATTACHMENT,
                upload,
                mime,
                false,
            )
            .await?
            .dir,
        );
    }
    // Notifikasi ke semua admin, termasuk pelaku bila ia admin (`Notification::send`).
    let admins = notify::admin_ids(&mut tx).await.map_err(internal)?;
    notify::to_users(
        &mut tx,
        &admins,
        &format!("Tiket Baru: {subjek}"),
        &format!("Tiket baru telah dibuat oleh {actor_name}"),
        Some(&format!("/tiket?ticketId={id}")),
        "info",
    )
    .await
    .map_err(internal)?;
    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(internal(e));
    }

    Ok(
        Json(json!({ "data": resource_without_comments(&state, user.user_id, id).await? }))
            .into_response(),
    )
}

/// `PUT` dan `PATCH /api/tiket/{id}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find_tiket(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (admin, _) = actor_roles(&state, user.user_id).await?;
    if !admin && current.user_id != user.user_id as i64 {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "Unauthorized"));
    }
    let form = read_form(multipart).await?;
    let mut errs = BTreeMap::new();
    let mut next = current.clone();
    let url = format!("{}/api/tiket/{id}", state.app_url.trim_end_matches('/'));
    let mut owner_notice: Option<(String, String)> = None;

    if admin {
        if let Some(v) = form.fields.get("status").cloned() {
            match v {
                Some(s) if STATUS.contains(&s.as_str()) => next.status = s,
                _ => foto::add(
                    &mut errs,
                    "status",
                    "The selected status is invalid.".into(),
                ),
            }
        }
        if let Some(v) = form.fields.get("admin_notes").cloned() {
            next.admin_notes = v;
        }
        if !errs.is_empty() {
            return Err(validation(errs));
        }
        if next.status != current.status {
            owner_notice = Some((
                format!(
                    "Tiket Anda \"{}\" telah diubah statusnya menjadi {}",
                    current.subjek,
                    next.status.to_uppercase()
                ),
                "Update Status Tiket".into(),
            ));
        }
    } else {
        if current.status != "open" {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "Tiket yang sudah diproses tidak dapat diubah",
            ));
        }
        if let Some(v) = form.fields.get("subjek").cloned().flatten() {
            if v.chars().count() > 255 {
                foto::add(
                    &mut errs,
                    "subjek",
                    "The subjek field must not be greater than 255 characters.".into(),
                );
            }
            next.subjek = v;
        }
        if let Some(v) = form.fields.get("deskripsi").cloned().flatten() {
            next.deskripsi = v;
        }
        if let Some(v) = form.fields.get("kategori").cloned().flatten() {
            if KATEGORI.contains(&v.as_str()) {
                next.kategori = Some(v);
            } else {
                foto::add(
                    &mut errs,
                    "kategori",
                    "The selected kategori is invalid.".into(),
                );
            }
        }
        if let Some(v) = form.fields.get("prioritas").cloned().flatten() {
            if PRIORITAS.contains(&v.as_str()) {
                next.prioritas = v;
            } else {
                foto::add(
                    &mut errs,
                    "prioritas",
                    "The selected prioritas is invalid.".into(),
                );
            }
        }
        if let Some(v) = form.fields.get("pekerjaan_id").cloned() {
            match v {
                None => next.pekerjaan_id = None,
                Some(s) => match s.parse::<i64>() {
                    Ok(n) => {
                        let n_exists: i64 =
                            sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
                                .bind(n)
                                .fetch_one(&state.pool)
                                .await
                                .map_err(internal)?;
                        if n_exists == 0 {
                            foto::add(
                                &mut errs,
                                "pekerjaan_id",
                                "The selected pekerjaan id is invalid.".into(),
                            );
                        } else {
                            let roles = auth::login::roles_of(&state.pool, user.user_id)
                                .await
                                .map_err(internal)?;
                            foto::ensure_access(&state, user.user_id, &roles, Some(n)).await?;
                            next.pekerjaan_id = Some(n);
                        }
                    }
                    Err(_) => foto::add(
                        &mut errs,
                        "pekerjaan_id",
                        "The selected pekerjaan id is invalid.".into(),
                    ),
                },
            }
        }
    }
    let mime = if admin {
        None
    } else {
        check_attachment(form.attachment.as_ref(), &mut errs)
    };
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut created_dir = None;
    let mut obsolete = Vec::new();
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_tiket SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "pekerjaan_id" => qb.push_bind(next.pekerjaan_id),
                "kategori" => qb.push_bind(next.kategori.clone()),
                "admin_notes" => qb.push_bind(next.admin_notes.clone()),
                "status" => qb.push_bind(next.status.clone()),
                "prioritas" => qb.push_bind(next.prioritas.clone()),
                "deskripsi" => qb.push_bind(next.deskripsi.clone()),
                "subjek" => qb.push_bind(next.subjek.clone()),
                _ => qb.push_bind(next.user_id),
            };
        }
        qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
        qb.build().execute(&mut *tx).await.map_err(internal)?;
        let after = find_tiket(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("tiket hilang"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        audit::write(
            &mut tx,
            audit::Entry {
                actor: user.user_id,
                event: "updated",
                auditable_type: TIKET_MODEL,
                auditable_id: id as u64,
                old: Some(old),
                new: Some(new),
                url: &url,
            },
            &headers,
        )
        .await
        .map_err(internal)?;
    }
    if let (Some(upload), Some(mime)) = (form.attachment.as_ref(), mime) {
        obsolete =
            media::delete_collection(&mut tx, TIKET_MODEL, id as u64, ATTACHMENT, None).await?;
        created_dir = Some(
            media::attach(
                &mut tx,
                TIKET_MODEL,
                id as u64,
                ATTACHMENT,
                upload,
                mime,
                false,
            )
            .await?
            .dir,
        );
    }
    if let Some((message, title)) = owner_notice {
        notify::to_users(
            &mut tx,
            &[current.user_id as u64],
            &title,
            &message,
            Some(&format!("/tiket?ticketId={id}")),
            "success",
        )
        .await
        .map_err(internal)?;
    }
    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(internal(e));
    }
    media::remove_dirs(&obsolete).await;

    Ok(
        Json(json!({ "data": resource_without_comments(&state, user.user_id, id).await? }))
            .into_response(),
    )
}

/// `DELETE /api/tiket/{id}`: admin, atau pemilik selama masih `open`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find_tiket(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (admin, _) = actor_roles(&state, user.user_id).await?;
    if !admin {
        if current.user_id != user.user_id as i64 {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "Unauthorized"));
        }
        if current.status != "open" {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "Tiket yang sudah diproses tidak dapat dihapus",
            ));
        }
    }
    let url = format!("{}/api/tiket/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs = media::delete_collection(&mut tx, TIKET_MODEL, id as u64, ATTACHMENT, None).await?;
    sqlx::query("DELETE FROM tbl_tiket WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "deleted",
            auditable_type: TIKET_MODEL,
            auditable_id: id as u64,
            old: Some(attributes(&current)),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Tiket berhasil dihapus" })).into_response())
}

/// `POST /api/tiket/bulk-update`: admin saja. Pembaruan massal tidak memicu audit atau notifikasi (sama dengan Laravel).
pub async fn bulk_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (admin, _) = actor_roles(&state, user.user_id).await?;
    if !admin {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "Unauthorized"));
    }
    let mut errs = BTreeMap::new();
    let raw_ids: Vec<i64> = match body.get("ids") {
        Some(Value::Array(items)) => {
            let mut ids = Vec::new();
            for (i, v) in items.iter().enumerate() {
                match v
                    .as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                {
                    Some(n) => ids.push(n),
                    None => foto::add(
                        &mut errs,
                        &format!("ids.{i}"),
                        format!("The ids.{i} field must be an integer."),
                    ),
                }
            }
            ids
        }
        Some(_) => {
            foto::add(&mut errs, "ids", "The ids field must be an array.".into());
            Vec::new()
        }
        None => {
            foto::add(&mut errs, "ids", "The ids field is required.".into());
            Vec::new()
        }
    };
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    match &status {
        Some(s) if STATUS.contains(&s.as_str()) => {}
        Some(_) => foto::add(
            &mut errs,
            "status",
            "The selected status is invalid.".into(),
        ),
        None => foto::add(&mut errs, "status", "The status field is required.".into()),
    }
    for id in raw_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_tiket WHERE id = ?")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
        if n == 0 {
            foto::add(&mut errs, "ids", "The selected ids is invalid.".into());
            break;
        }
    }
    if !errs.is_empty() {
        return Err(validation(errs));
    }
    let status = status.unwrap_or_default();
    let placeholders = vec!["?"; raw_ids.len()].join(",");
    let sql = format!("UPDATE tbl_tiket SET status = ? WHERE id IN ({placeholders})");
    let mut q = sqlx::query(&sql).bind(&status);
    for id in &raw_ids {
        q = q.bind(id);
    }
    q.execute(&state.pool).await.map_err(internal)?;
    Ok(
        Json(json!({ "message": format!("{} tiket berhasil diperbarui", raw_ids.len()) }))
            .into_response(),
    )
}

/// `POST /api/tiket/{id}/comments`. Hanya pemilik dan admin.
pub async fn store_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let tiket = find_tiket(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (admin, actor_name) = actor_roles(&state, user.user_id).await?;
    if !admin && tiket.user_id != user.user_id as i64 {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "Unauthorized"));
    }
    let message = match body.get("message") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(Value::String(_)) | None | Some(Value::Null) => {
            let mut errs = BTreeMap::new();
            foto::add(
                &mut errs,
                "message",
                "The message field is required.".into(),
            );
            return Err(validation(errs));
        }
        Some(_) => {
            let mut errs = BTreeMap::new();
            foto::add(
                &mut errs,
                "message",
                "The message field must be a string.".into(),
            );
            return Err(validation(errs));
        }
    };

    let url = format!(
        "{}/api/tiket/{id}/comments",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let comment_id = sqlx::query(
        "INSERT INTO tbl_tiket_comment (tiket_id, user_id, message, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(id)
    .bind(user.user_id)
    .bind(&message)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let attrs = comment_attributes(&mut tx, comment_id).await?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "created",
            auditable_type: COMMENT_MODEL,
            auditable_id: comment_id as u64,
            old: None,
            new: Some(attrs),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;

    let link = format!("/tiket?ticketId={id}");
    if user.user_id as i64 == tiket.user_id {
        let admins = notify::admin_ids(&mut tx).await.map_err(internal)?;
        notify::to_users(
            &mut tx,
            &admins,
            &format!("Komentar Baru pada Tiket ({})", tiket.subjek),
            &format!("{actor_name} menambahkan komentar baru."),
            Some(&link),
            "info",
        )
        .await
        .map_err(internal)?;
    } else {
        notify::to_users(
            &mut tx,
            &[tiket.user_id as u64],
            "Komentar Baru pada Tiket",
            &format!("Ada komentar baru pada tiket Anda: \"{}\"", tiket.subjek),
            Some(&link),
            "info",
        )
        .await
        .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;

    let data = comment_json(&state, comment_id).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

async fn comment_attributes(
    tx: &mut sqlx::Transaction<'_, MySql>,
    id: i64,
) -> Result<Map<String, Value>, ApiError> {
    let r = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, CAST(tiket_id AS SIGNED) AS tiket_id, CAST(user_id AS SIGNED) AS user_id, \
         message, created_at, updated_at FROM tbl_tiket_comment WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>("id").map_err(internal)?),
    );
    m.insert(
        "tiket_id".into(),
        json!(r.try_get::<i64, _>("tiket_id").map_err(internal)?),
    );
    m.insert(
        "user_id".into(),
        json!(r.try_get::<i64, _>("user_id").map_err(internal)?),
    );
    m.insert(
        "message".into(),
        json!(r.try_get::<String, _>("message").map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(m)
}

/// `TiketCommentResource` dengan `user`.
async fn comment_json(state: &AppState, id: i64) -> Result<Value, ApiError> {
    let r = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, CAST(tiket_id AS SIGNED) AS tiket_id, CAST(user_id AS SIGNED) AS user_id, \
         message, created_at, updated_at FROM tbl_tiket_comment WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    let user_id: i64 = r.try_get("user_id").map_err(internal)?;
    let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
    Ok(json!({
        "id": r.try_get::<i64, _>("id").map_err(internal)?,
        "tiket_id": r.try_get::<i64, _>("tiket_id").map_err(internal)?,
        "user_id": user_id,
        "user": users::resource(&state.pool, &state.app_url, user_id as u64).await.map_err(internal)?.unwrap_or(Value::Null),
        "message": r.try_get::<String, _>("message").map_err(internal)?,
        "created_at": carbon_json(created),
        "updated_at": carbon_json(updated),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_supported_images_only() {
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a.."), Some("image/gif"));
        assert_eq!(
            sniff_image(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_image(b"%PDF-1.4"), None);
        assert_eq!(sniff_image(b"<svg xmlns"), None, "svg belum didukung");
    }
}
