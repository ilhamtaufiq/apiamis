//! Lookup dan konfigurasi: `app-settings` (index dan maintenance) dan `tags`.
//!
//! Setara `AppSettingController@index`, `@maintenanceStatus`, `AppSettingResource`,
//! `MaintenanceModeService::statusPayload`, `TagController@index`, `@show`, dan `TagResource`.

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

use crate::{desa::internal, format::iso8601_utc, maintenance, require_auth, AppState};

/// Bentuk Carbon mentah (`toJSON`) untuk kolom datetime model Eloquent.
/// Format belum diverifikasi terhadap produksi (lihat T17).
pub fn carbon_json(ts: Option<DateTime<Utc>>) -> Value {
    match ts {
        Some(t) => Value::String(t.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()),
        None => Value::Null,
    }
}

/// Setting yang nilainya tidak pernah dikirim ke klien (`AppSettingResource`).
pub fn is_hidden_setting(key: &str) -> bool {
    key.starts_with("chat_api_key_")
        || key == "mail_password"
        || key.starts_with("google_drive_")
        || key == "s3_secret_access_key"
}

/// `filled()` di Laravel: tidak null, tidak kosong, dan tidak hanya spasi.
pub fn is_filled(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.trim().is_empty())
}

/// Nilai dan `is_configured` yang dikirim untuk satu setting.
/// `media_url` dipakai untuk tipe `file`: `Some` berarti ada berkas.
pub fn setting_view(
    key: &str,
    kind: &str,
    value: Option<&str>,
    media_url: Option<String>,
) -> (Value, bool) {
    let (mut out, mut configured) = if is_hidden_setting(key) {
        (Value::Null, is_filled(value))
    } else {
        (
            value.map_or(Value::Null, |v| Value::String(v.to_string())),
            false,
        )
    };
    if kind == "file" {
        out = media_url.clone().map_or(Value::Null, Value::String);
        configured = media_url.is_some();
    }
    (out, configured)
}

#[derive(Debug, Clone)]
pub struct SettingRow {
    pub id: u64,
    pub key: String,
    pub value: Option<String>,
    pub kind: String,
    pub updated_at: Option<DateTime<Utc>>,
}

pub async fn settings(pool: &MySqlPool) -> Result<Vec<SettingRow>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT id, `key`, `value`, `type`, updated_at FROM app_settings ORDER BY id")
            .fetch_all(pool)
            .await?;
    rows.iter()
        .map(|r| {
            Ok(SettingRow {
                id: r.try_get("id")?,
                key: r.try_get("key")?,
                value: r.try_get("value")?,
                kind: r.try_get("type")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect()
}

/// URL media pertama koleksi `app-settings` (hanya disk `public` yang diketahui).
async fn setting_media_url(
    pool: &MySqlPool,
    app_url: &str,
    setting_id: u64,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, disk, file_name FROM media WHERE model_type = 'App\\\\Models\\\\AppSetting' \
         AND model_id = ? AND collection_name = 'app-settings' ORDER BY order_column, id LIMIT 1",
    )
    .bind(setting_id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let disk: String = r.try_get("disk")?;
    let media_id: u64 = r.try_get("id")?;
    let file_name: String = r.try_get("file_name")?;
    Ok((disk == "public").then(|| {
        format!(
            "{}/storage/{media_id}/{file_name}",
            app_url.trim_end_matches('/')
        )
    }))
}

/// `GET /api/app-settings`: publik, `{"data": [...]}`.
pub async fn index(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = settings(&state.pool).await.map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for s in &rows {
        let media_url = if s.kind == "file" {
            setting_media_url(&state.pool, &state.app_url, s.id)
                .await
                .map_err(internal)?
        } else {
            None
        };
        let (value, configured) = setting_view(&s.key, &s.kind, s.value.as_deref(), media_url);
        data.push(json!({
            "id": s.id,
            "key": s.key,
            "value": value,
            "type": s.kind,
            "is_configured": configured,
            "updated_at": iso8601_utc(s.updated_at),
        }));
    }
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/app-settings/maintenance`: status ringkas, user dibaca dari token bila ada.
pub async fn maintenance_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let enabled = maintenance::is_enabled_db(&state.pool)
        .await
        .map_err(internal)?;
    let bypass_list = maintenance::bypass_list_db(&state.pool)
        .await
        .map_err(internal)?;
    let user_email = match crate::session::token_from_headers(&headers, &state.session.name) {
        Some(token) => match auth::authenticate(&state.pool, &token).await {
            Ok(u) => sqlx::query("SELECT email FROM users WHERE id = ?")
                .bind(u.user_id)
                .fetch_optional(&state.pool)
                .await
                .map_err(internal)?
                .and_then(|r| r.try_get::<String, _>("email").ok()),
            Err(_) => None,
        },
        None => None,
    };
    let bypass = allows(user_email.as_deref(), &bypass_list);
    Ok(Json(
        json!({ "data": maintenance_payload(enabled, bypass) }),
    ))
}

fn allows(email: Option<&str>, list: &[String]) -> bool {
    maintenance::allows_email(email, list)
}

/// `statusPayload()` di Laravel.
pub fn maintenance_payload(enabled: bool, bypass: bool) -> Value {
    json!({
        "enabled": enabled,
        "bypass": enabled && bypass,
        "message": if enabled {
            Value::String("Aplikasi sedang maintenance. Hanya akun bypass yang dapat mengakses.".into())
        } else {
            Value::Null
        },
        "can_access": !enabled || bypass,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagRow {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub color: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// `TagResource`.
pub fn tag_resource(t: &TagRow) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "slug": t.slug,
        "color": t.color,
        "created_at": iso8601_utc(t.created_at),
        "updated_at": iso8601_utc(t.updated_at),
    })
}

fn map_tag(r: &sqlx::mysql::MySqlRow) -> Result<TagRow, sqlx::Error> {
    Ok(TagRow {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        slug: r.try_get("slug")?,
        color: r.try_get("color")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Semua tag, urut nama. `search` memakai `LIKE %search%` seperti Laravel.
pub async fn tags(pool: &MySqlPool, search: Option<&str>) -> Result<Vec<TagRow>, sqlx::Error> {
    let rows = match search.filter(|s| !s.is_empty()) {
        Some(s) => {
            sqlx::query("SELECT id, name, slug, color, created_at, updated_at FROM tbl_tags WHERE name LIKE ? ORDER BY name")
                .bind(format!("%{s}%"))
                .fetch_all(pool)
                .await?
        }
        None => {
            sqlx::query("SELECT id, name, slug, color, created_at, updated_at FROM tbl_tags ORDER BY name")
                .fetch_all(pool)
                .await?
        }
    };
    rows.iter().map(map_tag).collect()
}

pub async fn tag(pool: &MySqlPool, id: u64) -> Result<Option<TagRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, slug, color, created_at, updated_at FROM tbl_tags WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| map_tag(&r)).transpose()
}

/// `GET /api/tags` (butuh login): `{"data": [...]}`.
pub async fn tags_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let rows = tags(&state.pool, query.get("search").map(String::as_str))
        .await
        .map_err(internal)?;
    let data: Vec<Value> = rows.iter().map(tag_resource).collect();
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/tags/{id}` (butuh login): `{"data": TagResource}`.
pub async fn tags_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let row = match id.parse::<u64>() {
        Ok(id) => tag(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": tag_resource(&row) })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_settings_never_expose_value() {
        for key in [
            "chat_api_key_minimax",
            "mail_password",
            "google_drive_token",
            "s3_secret_access_key",
        ] {
            let (v, configured) = setting_view(key, "text", Some("rahasia"), None);
            assert_eq!(v, Value::Null, "{key}");
            assert!(configured, "{key}");
        }
    }

    #[test]
    fn public_settings_expose_value_and_not_configured_flag() {
        let (v, configured) = setting_view("site_name", "text", Some("APIAMIS"), None);
        assert_eq!(v, json!("APIAMIS"));
        assert!(!configured);
    }

    #[test]
    fn filled_treats_blank_as_empty() {
        assert!(!is_filled(None));
        assert!(!is_filled(Some("   ")));
        assert!(is_filled(Some("x")));
    }

    #[test]
    fn file_setting_uses_media_url_and_flag() {
        let (v, configured) = setting_view(
            "logo",
            "file",
            Some("ignored"),
            Some("http://x/storage/1/a.png".into()),
        );
        assert_eq!(v, json!("http://x/storage/1/a.png"));
        assert!(configured);
        let (v, configured) = setting_view("logo", "file", None, None);
        assert_eq!(v, Value::Null);
        assert!(!configured);
    }

    #[test]
    fn maintenance_payload_matches_status_payload() {
        assert_eq!(
            maintenance_payload(false, false),
            json!({ "enabled": false, "bypass": false, "message": null, "can_access": true })
        );
        assert_eq!(
            maintenance_payload(true, false),
            json!({
                "enabled": true,
                "bypass": false,
                "message": "Aplikasi sedang maintenance. Hanya akun bypass yang dapat mengakses.",
                "can_access": false
            })
        );
        assert_eq!(maintenance_payload(true, true)["can_access"], true);
    }

    #[test]
    fn tag_resource_has_laravel_keys() {
        let t = TagRow {
            id: 3,
            name: "Air".into(),
            slug: "air".into(),
            color: Some("#00aaff".into()),
            created_at: None,
            updated_at: None,
        };
        let v = tag_resource(&t);
        assert_eq!(v["slug"], "air");
        assert_eq!(v["created_at"], Value::Null);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTypeRow {
    pub id: u64,
    pub name: String,
    pub code: String,
    pub format_template: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// `DocumentType::all()` di-serialisasi langsung oleh Eloquent (tanpa Resource).
pub fn document_type_json(d: &DocumentTypeRow) -> Value {
    json!({
        "id": d.id,
        "name": d.name,
        "code": d.code,
        "format_template": d.format_template,
        "created_at": carbon_json(d.created_at),
        "updated_at": carbon_json(d.updated_at),
    })
}

pub async fn document_types(pool: &MySqlPool) -> Result<Vec<DocumentTypeRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, code, format_template, created_at, updated_at FROM tbl_document_types ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(DocumentTypeRow {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                code: r.try_get("code")?,
                format_template: r.try_get("format_template")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect()
}

/// `GET /api/document-types` (butuh login): array langsung, tanpa pembungkus `data`.
pub async fn document_types_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let rows = document_types(&state.pool).await.map_err(internal)?;
    let items: Vec<Value> = rows.iter().map(document_type_json).collect();
    Ok(Json(Value::Array(items)))
}
