//! Tulis tag: `POST /api/tags`, `PUT/PATCH/DELETE /api/tags/{id}`.
//! Setara `TagController` dan trait `Auditable` (event created, updated, deleted).

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySql;
use std::collections::BTreeMap;

use crate::{
    audit,
    desa::internal,
    lookup::{self, TagRow},
    require_auth, AppState,
};

const URL_BASE: &str = "/api/tags";

/// `Str::slug` untuk teks ASCII: huruf kecil, selain alfanumerik menjadi `-`.
/// Huruf non-ASCII tidak ditransliterasi seperti Laravel (batasan yang dicatat).
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.trim().chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

type Errors = BTreeMap<String, Vec<String>>;

fn invalid(errors: Errors) -> ApiError {
    ApiError::validation("The given data was invalid.", errors)
}

/// `color`: nullable, maks 7, format `#RRGGBB`.
fn color_ok(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => {
            s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
        }
        _ => false,
    }
}

/// `name`: string, maks 100, tidak boleh kosong, unik (kecuali `except`).
async fn name_ok(
    tx_or_pool: &sqlx::MySqlPool,
    name: &str,
    except: Option<u64>,
) -> Result<bool, sqlx::Error> {
    let count: i64 = match except {
        Some(id) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM tbl_tags WHERE name = ? AND id <> ?")
                .bind(name)
                .bind(id)
                .fetch_one(tx_or_pool)
                .await?
        }
        None => {
            sqlx::query_scalar("SELECT COUNT(*) FROM tbl_tags WHERE name = ?")
                .bind(name)
                .fetch_one(tx_or_pool)
                .await?
        }
    };
    Ok(count == 0)
}

fn name_value(v: &Value, errors: &mut Errors) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() && s.chars().count() <= 100 => Some(s.clone()),
        _ => {
            errors
                .entry("name".into())
                .or_default()
                .push("The name field is invalid.".into());
            None
        }
    }
}

fn attributes(t: &TagRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(t.id));
    m.insert("name".into(), json!(t.name));
    m.insert("slug".into(), json!(t.slug));
    m.insert("color".into(), json!(t.color));
    m.insert("created_at".into(), lookup::carbon_json(t.created_at));
    m.insert("updated_at".into(), lookup::carbon_json(t.updated_at));
    m
}

async fn find(pool: &sqlx::MySqlPool, id: u64) -> Result<Option<TagRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, slug, color, created_at, updated_at FROM tbl_tags WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    use sqlx::Row;
    row.map(|r| {
        Ok(TagRow {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            slug: r.try_get("slug")?,
            color: r.try_get("color")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
        })
    })
    .transpose()
}

/// `POST /api/tags`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errors = Errors::new();
    let name = match obj.get("name") {
        Some(v) => name_value(v, &mut errors),
        None => {
            errors
                .entry("name".into())
                .or_default()
                .push("The name field is required.".into());
            None
        }
    };
    if let Some(c) = obj.get("color") {
        if !color_ok(c) {
            errors
                .entry("color".into())
                .or_default()
                .push("The color field format is invalid.".into());
        }
    }
    if let Some(n) = &name {
        if !name_ok(&state.pool, n, None).await.map_err(internal)? {
            errors
                .entry("name".into())
                .or_default()
                .push("The name has already been taken.".into());
        }
    }
    if !errors.is_empty() {
        return Err(invalid(errors));
    }
    let name = name.unwrap_or_default();
    let color = obj
        .get("color")
        .and_then(|c| c.as_str())
        .map(str::to_string);
    let slug = slugify(&name);

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_tags (name, slug, color, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(&name)
    .bind(&slug)
    .bind(&color)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id();
    let row = find_in_tx(&mut tx, id).await?;
    audit_tag(
        &mut tx,
        user.user_id,
        "created",
        id,
        None,
        Some(attributes(&row)),
        &headers,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "data": lookup::tag_resource(&row) })).into_response())
}

/// `PUT/PATCH /api/tags/{id}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let obj = body.as_object().cloned().unwrap_or_default();

    let mut errors = Errors::new();
    let mut new_name: Option<String> = None;
    if let Some(v) = obj.get("name") {
        new_name = name_value(v, &mut errors);
        if let Some(n) = &new_name {
            if !name_ok(&state.pool, n, Some(id)).await.map_err(internal)? {
                errors
                    .entry("name".into())
                    .or_default()
                    .push("The name has already been taken.".into());
            }
        }
    }
    let mut new_color: Option<Option<String>> = None;
    if let Some(c) = obj.get("color") {
        if !color_ok(c) {
            errors
                .entry("color".into())
                .or_default()
                .push("The color field format is invalid.".into());
        } else {
            new_color = Some(c.as_str().map(str::to_string));
        }
    }
    if !errors.is_empty() {
        return Err(invalid(errors));
    }

    // Mengikuti Laravel: name dan slug berubah bersama; slug juga dihitung ulang bila name berubah.
    let mut next = current.clone();
    if let Some(n) = new_name {
        next.slug = slugify(&n);
        next.name = n;
    }
    if let Some(c) = new_color {
        next.color = c;
    }

    let changed =
        next.name != current.name || next.slug != current.slug || next.color != current.color;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if changed {
        sqlx::query(
            "UPDATE tbl_tags SET name = ?, slug = ?, color = ?, updated_at = NOW() WHERE id = ?",
        )
        .bind(&next.name)
        .bind(&next.slug)
        .bind(&next.color)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        let (old, new) = dirty_maps(&current, &next);
        audit_tag(
            &mut tx,
            user.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &headers,
        )
        .await?;
    }
    let row = find_in_tx(&mut tx, id).await?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "data": lookup::tag_resource(&row) })).into_response())
}

/// `DELETE /api/tags/{id}`. Pivot `pekerjaan_tag` ikut terhapus lewat foreign key.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_tags WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit_tag(
        &mut tx,
        user.user_id,
        "deleted",
        id,
        Some(attributes(&current)),
        None,
        &headers,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok((
        StatusCode::OK,
        Json(json!({ "message": "Tag deleted successfully" })),
    )
        .into_response())
}

fn dirty_maps(old: &TagRow, new: &TagRow) -> (Map<String, Value>, Map<String, Value>) {
    let mut o = Map::new();
    let mut n = Map::new();
    if old.name != new.name {
        o.insert("name".into(), json!(old.name));
        n.insert("name".into(), json!(new.name));
    }
    if old.slug != new.slug {
        o.insert("slug".into(), json!(old.slug));
        n.insert("slug".into(), json!(new.slug));
    }
    if old.color != new.color {
        o.insert("color".into(), json!(old.color));
        n.insert("color".into(), json!(new.color));
    }
    (o, n)
}

async fn find_in_tx(tx: &mut sqlx::Transaction<'_, MySql>, id: u64) -> Result<TagRow, ApiError> {
    use sqlx::Row;
    let r = sqlx::query(
        "SELECT id, name, slug, color, created_at, updated_at FROM tbl_tags WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(TagRow {
        id: r.try_get("id").map_err(internal)?,
        name: r.try_get("name").map_err(internal)?,
        slug: r.try_get("slug").map_err(internal)?,
        color: r.try_get("color").map_err(internal)?,
        created_at: r.try_get("created_at").map_err(internal)?,
        updated_at: r.try_get("updated_at").map_err(internal)?,
    })
}

async fn audit_tag(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    event: &str,
    id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let url = format!("{URL_BASE}/{id}");
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: "App\\Models\\Tag",
            auditable_id: id,
            old,
            new,
            url: &url,
        },
        headers,
    )
    .await
    .map_err(internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_laravel_for_ascii() {
        assert_eq!(slugify("Uji Tag 1!"), "uji-tag-1");
        assert_eq!(slugify("  Ganda   spasi "), "ganda-spasi");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn color_must_be_hex6() {
        assert!(color_ok(&json!(null)));
        assert!(color_ok(&json!("#A1b2C3")));
        assert!(!color_ok(&json!("#12345")));
        assert!(!color_ok(&json!("123456")));
        assert!(!color_ok(&json!("#zzzzzz")));
    }
}
