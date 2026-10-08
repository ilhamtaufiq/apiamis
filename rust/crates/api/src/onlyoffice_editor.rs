//! Editor ONLYOFFICE: health, config editor, dan callback simpan dari Document Server.
//!
//! Setara `OnlyOfficeController::health`, `config`, dan `callback` di Laravel, beserta
//! `OnlyOfficeService::buildEditorPayload`, `OnlyOfficeMediaAuthorizer`, dan `OnlyOfficeJwt::decode`.
//! Konfigurasi dibaca dari environment lewat `onlyoffice::Settings` (lihat modul `onlyoffice`).
//!
//! Perbedaan yang belum bisa disamakan dengan Laravel:
//! - Pemilik media `UserDriveItem`: `canManage` belum dipindah. Non-admin ditolak (fail-closed),
//!   sedangkan callback hanya memeriksa baris ada dan belum soft-deleted.
//! - Pemilik media selain Berkas, Kontrak, KontrakAddendum, dan UserDriveItem ditolak (fail-closed).
//!   Laravel menimpa file untuk pemilik apa pun yang ada.
//! - Pesan error koneksi pada health check berasal dari reqwest, bukan Guzzle.
//! - Pengecekan `Media` lebih dulu sebelum auth (binding rute Laravel jalan sebelum `auth:sanctum`).
//! - Penimpaan file tidak menghapus konversi turunan (`deleteGeneratedConversions`). Dokumen tidak
//!   memiliki konversi, jadi tidak ada yang perlu dihapus.

use std::{collections::HashMap, time::Duration};

use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    access,
    media,
    onlyoffice::{self, Settings},
    require_auth, AppState,
};

type HmacSha256 = Hmac<Sha256>;

const BERKAS_MODEL: &str = "App\\Models\\Berkas";
const KONTRAK_MODEL: &str = "App\\Models\\Kontrak";
const ADDENDUM_MODEL: &str = "App\\Models\\KontrakAddendum";
const DRIVE_ITEM_MODEL: &str = "App\\Models\\UserDriveItem";
const CALLBACK_PATH: &str = "/api/onlyoffice/callback";
/// `OnlyOfficeService::SUPPORTED_EXTENSIONS`.
const SUPPORTED_EXTENSIONS: [&str; 13] = [
    "doc", "docx", "odt", "rtf", "txt", "xls", "xlsx", "ods", "csv", "ppt", "pptx", "odp", "pdf",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `OnlyOfficeDownloadToken::secret()`: JWT secret, atau `APP_KEY` bila JWT secret kosong.
/// Dipakai untuk memverifikasi callback. Config editor hanya ditandatangani bila JWT secret ada.
fn callback_secret(settings: &Settings) -> String {
    if settings.jwt_secret.is_empty() {
        std::env::var("APP_KEY").unwrap_or_default()
    } else {
        settings.jwt_secret.clone()
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------------------
// Padanan semantik PHP untuk nilai JSON
// ---------------------------------------------------------------------------

/// Truthiness PHP (`!empty` dan `!$x`): null, false, 0, "", "0", dan array kosong adalah false.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !(s.is_empty() || s == "0"),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `(int)` PHP untuk nilai JSON.
fn php_int(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().map(|f| f as i64).unwrap_or(0)),
        Some(Value::String(s)) => leading_int(s),
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Array(a)) => i64::from(!a.is_empty()),
        Some(Value::Object(o)) => i64::from(!o.is_empty()),
        _ => 0,
    }
}

/// Angka di awal string, seperti `(int)"2abc"` di PHP.
fn leading_int(s: &str) -> i64 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return 0;
    }
    t[..end].parse().unwrap_or(0)
}

/// `(string)` PHP untuk nilai JSON. Array menjadi "Array", seperti PHP.
fn php_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(true)) => "1".to_string(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => "Array".to_string(),
        _ => String::new(),
    }
}

fn is_numeric(v: &Value) -> bool {
    match v {
        Value::Number(_) => true,
        Value::String(s) => s.trim().parse::<f64>().is_ok(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Input request dan JWT
// ---------------------------------------------------------------------------

/// Pasangan kunci-nilai dari query string. Nilai terakhir menang, seperti PHP.
fn query_pairs(raw: &str) -> Vec<(String, String)> {
    let Ok(url) = reqwest::Url::parse(&format!("http://localhost/?{raw}")) else {
        return Vec::new();
    };
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// `$request->all()`: query string digabung body. Body menang bila kuncinya sama.
/// Body JSON dibaca bila Content-Type memuat `json`, selain itu sebagai form.
fn request_input(raw_query: Option<&str>, headers: &HeaderMap, body: &[u8]) -> Map<String, Value> {
    let mut input = Map::new();
    if let Some(q) = raw_query {
        for (k, v) in query_pairs(q) {
            input.insert(k, Value::String(v));
        }
    }
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.to_ascii_lowercase().contains("json"));
    if is_json {
        if let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(body) {
            for (k, v) in obj {
                input.insert(k, v);
            }
        }
    } else if !body.is_empty() {
        for (k, v) in query_pairs(&String::from_utf8_lossy(body)) {
            input.insert(k, Value::String(v));
        }
    }
    input
}

/// `$request->bearerToken()`: nilai setelah `Bearer `, termasuk string kosong.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

/// `$request->input('token') ?? $request->bearerToken() ?? extractJwtFromBody($payload)`.
fn callback_token(input: &Map<String, Value>, headers: &HeaderMap) -> Option<Value> {
    match input.get("token") {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => match bearer_token(headers) {
            Some(t) => Some(Value::String(t)),
            None => ["token", "payload", "body"]
                .iter()
                .find_map(|key| match input.get(*key) {
                    Some(v @ Value::String(_)) => Some(v.clone()),
                    _ => None,
                }),
        },
    }
}

/// `OnlyOfficeJwt::decode`: verifikasi HS256 lewat signature (tanpa cek header), lalu cek `exp`.
fn jwt_decode(token: &str, secret: &str) -> Option<Map<String, Value>> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let (header, body, signature) = (parts[0], parts[1], parts[2]);

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(format!("{header}.{body}").as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if !constant_time_eq(&expected, signature) {
        return None;
    }

    let raw = URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')).ok()?;
    let Value::Object(map) = serde_json::from_slice::<Value>(&raw).ok()? else {
        return None;
    };
    if let Some(exp) = map.get("exp") {
        if is_numeric(exp) && php_int(Some(exp)) < now_secs() {
            return None;
        }
    }
    Some(map)
}

// ---------------------------------------------------------------------------
// Media dan otorisasi
// ---------------------------------------------------------------------------

struct MediaRow {
    id: u64,
    model_type: String,
    model_id: u64,
    file_name: String,
    mime_type: Option<String>,
    updated_ts: Option<i64>,
}

impl MediaRow {
    /// `pathinfo($file_name, PATHINFO_EXTENSION)` dalam huruf kecil.
    fn extension(&self) -> String {
        extension_of(&self.file_name)
    }
}

fn extension_of(file_name: &str) -> String {
    file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

async fn find_media(pool: &MySqlPool, id: u64) -> Result<Option<MediaRow>, ApiError> {
    let row = sqlx::query(
        "SELECT id, model_type, model_id, file_name, mime_type, \
         CAST(UNIX_TIMESTAMP(updated_at) AS SIGNED) AS updated_ts FROM media WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(MediaRow {
        id: row.try_get("id").map_err(internal)?,
        model_type: row.try_get("model_type").map_err(internal)?,
        model_id: row.try_get("model_id").map_err(internal)?,
        file_name: row.try_get("file_name").map_err(internal)?,
        mime_type: row.try_get("mime_type").map_err(internal)?,
        updated_ts: row.try_get("updated_ts").map_err(internal)?,
    }))
}

/// `buildDocumentKey`: `media_{id}_{updated_at}`, atau waktu sekarang bila `updated_at` kosong.
fn document_key(media: &MediaRow) -> String {
    format!(
        "media_{}_{}",
        media.id,
        media.updated_ts.unwrap_or_else(now_secs)
    )
}

/// `OnlyOfficeMediaAuthorizer::canAccess`. Admin selalu boleh, lalu pekerjaan diperiksa
/// dengan aturan `byUserRole()`.
struct Actor {
    user_id: u64,
    roles: Vec<(u64, String)>,
}

impl Actor {
    fn has_role(&self, name: &str) -> bool {
        self.roles.iter().any(|(_, n)| n == name)
    }

    fn is_pengawas(&self) -> bool {
        self.roles
            .iter()
            .any(|(_, n)| access::PENGAWAS_ROLES.contains(&n.as_str()))
    }
}

async fn any_pekerjaan_accessible(
    pool: &MySqlPool,
    actor: &Actor,
    ids: &[i64],
) -> Result<bool, ApiError> {
    for id in ids.iter().filter(|id| **id != 0) {
        let ok = access::user_can_access(pool, actor.user_id, &actor.roles, *id as u64)
            .await
            .map_err(internal)?;
        if ok {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `$kontrak->pekerjaans` ditambah `id_pekerjaan`, lalu `unique()->filter()`.
async fn kontrak_pekerjaan_ids(pool: &MySqlPool, kontrak_id: u64) -> Result<Vec<i64>, ApiError> {
    let id_pekerjaan: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT CAST(id_pekerjaan AS SIGNED) FROM tbl_kontrak WHERE id = ?",
    )
    .bind(kontrak_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(id_pekerjaan) = id_pekerjaan else {
        return Ok(Vec::new());
    };
    let mut ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(pekerjaan_id AS SIGNED) FROM kontrak_pekerjaan WHERE kontrak_id = ?",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    if let Some(p) = id_pekerjaan {
        ids.push(p);
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

async fn can_access(pool: &MySqlPool, actor: &Actor, media: &MediaRow) -> Result<bool, ApiError> {
    if actor.has_role("admin") {
        return Ok(true);
    }
    match media.model_type.as_str() {
        BERKAS_MODEL => {
            let pekerjaan: Option<Option<i64>> = sqlx::query_scalar(
                "SELECT CAST(pekerjaan_id AS SIGNED) FROM tbl_berkas WHERE id = ?",
            )
            .bind(media.model_id)
            .fetch_optional(pool)
            .await
            .map_err(internal)?;
            match pekerjaan.flatten() {
                Some(p) => any_pekerjaan_accessible(pool, actor, &[p]).await,
                None => Ok(false),
            }
        }
        KONTRAK_MODEL => {
            let ids = kontrak_pekerjaan_ids(pool, media.model_id).await?;
            any_pekerjaan_accessible(pool, actor, &ids).await
        }
        ADDENDUM_MODEL => {
            let kontrak_id: Option<i64> = sqlx::query_scalar(
                "SELECT CAST(kontrak_id AS SIGNED) FROM tbl_kontrak_addendums WHERE id = ?",
            )
            .bind(media.model_id)
            .fetch_optional(pool)
            .await
            .map_err(internal)?
            .flatten();
            match kontrak_id {
                Some(k) if k > 0 => {
                    let ids = kontrak_pekerjaan_ids(pool, k as u64).await?;
                    any_pekerjaan_accessible(pool, actor, &ids).await
                }
                _ => Ok(false),
            }
        }
        // `UserDriveItem::canManage` belum dipindah: non-admin ditolak.
        _ => Ok(false),
    }
}

/// `OnlyOfficeMediaAuthorizer::canEdit`: admin dan operator boleh edit. Pengawas boleh edit
/// bila punya akses, sedangkan pemilik drive ditolak karena `canManage` belum dipindah.
fn can_edit(actor: &Actor, media: &MediaRow, can_access: bool) -> bool {
    if !can_access {
        return false;
    }
    if actor.has_role("admin") || actor.has_role("operator") {
        return true;
    }
    if media.model_type == DRIVE_ITEM_MODEL {
        return false;
    }
    actor.is_pengawas()
}

/// Pemilik media ada dan belum dihapus, seperti `$media->model` di Eloquent.
async fn owner_exists(pool: &MySqlPool, media: &MediaRow) -> Result<bool, ApiError> {
    let table = match media.model_type.as_str() {
        BERKAS_MODEL => "tbl_berkas",
        KONTRAK_MODEL => "tbl_kontrak",
        ADDENDUM_MODEL => "tbl_kontrak_addendums",
        DRIVE_ITEM_MODEL => "user_drive_items",
        _ => return Ok(false),
    };
    let soft = if table == "user_drive_items" {
        " AND deleted_at IS NULL"
    } else {
        ""
    };
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE id = ?{soft}");
    let count: i64 = sqlx::query_scalar(&sql)
        .bind(media.model_id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(count > 0)
}

// ---------------------------------------------------------------------------
// GET /api/onlyoffice/health
// ---------------------------------------------------------------------------

/// Health check Document Server (`OnlyOfficeController::health`). 200 bila aktif dan siap, 503 selain itu.
pub async fn health() -> (StatusCode, Json<Value>) {
    let settings = Settings::from_env();
    let enabled = settings.enabled();
    let server = settings.server_url.clone();
    let mut reachable = false;
    let mut message = if enabled {
        "Document Server dikonfigurasi.".to_string()
    } else {
        "ONLYOFFICE belum dikonfigurasi.".to_string()
    };

    if enabled {
        match health_probe(&server).await {
            Ok(true) => {
                reachable = true;
                message = "Document Server siap.".to_string();
            }
            Ok(false) => {
                message = "Document Server tidak merespons healthcheck.".to_string();
            }
            Err(e) => {
                message = format!("Document Server tidak terjangkau: {e}");
            }
        }
    }

    let status = if enabled && reachable {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "data": {
                "enabled": enabled,
                "reachable": reachable,
                "document_server_url": if server.is_empty() { Value::Null } else { json!(format!("{server}/")) },
                "message": message,
            }
        })),
    )
}

/// `successful() || str_contains(strtolower(body), 'true')`, dengan timeout 5 detik.
async fn health_probe(server: &str) -> Result<bool, reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client.get(format!("{server}/healthcheck")).send().await?;
    if response.status().is_success() {
        return Ok(true);
    }
    let body = response.text().await.unwrap_or_default();
    Ok(body.to_lowercase().contains("true"))
}

// ---------------------------------------------------------------------------
// GET /api/onlyoffice/media/{id}/config
// ---------------------------------------------------------------------------

fn document_type(extension: &str) -> &'static str {
    match extension {
        "xls" | "xlsx" | "ods" | "csv" => "cell",
        "ppt" | "pptx" | "odp" => "slide",
        _ => "word",
    }
}

/// `OnlyOfficeService::resolveCallbackUrl`: `ONLYOFFICE_CALLBACK_URL` bila diisi, selain itu APP_URL.
fn callback_url(app_url: &str) -> String {
    let override_url = std::env::var("ONLYOFFICE_CALLBACK_URL")
        .unwrap_or_default()
        .trim()
        .to_string();
    let base = if override_url.is_empty() {
        app_url.to_string()
    } else {
        override_url
    };
    format!("{}{CALLBACK_PATH}", base.trim_end_matches('/'))
}

/// `GET /api/onlyoffice/media/{id}/config?mode=view|edit`: payload untuk editor ONLYOFFICE.
pub async fn config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    // Binding rute dulu (404), baru auth, seperti middleware Laravel.
    let media_id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let media = find_media(&state.pool, media_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let user = require_auth(&state, &headers).await?;

    let settings = Settings::from_env();
    if !settings.enabled() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "ONLYOFFICE Document Server belum dikonfigurasi.",
        ));
    }
    let extension = media.extension();
    if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Format file tidak didukung ONLYOFFICE.",
        ));
    }

    let query: HashMap<String, String> = raw_query
        .as_deref()
        .map(query_pairs)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let requested_mode = query.get("mode").map(|m| m.to_lowercase());
    if let Some(mode) = &requested_mode {
        if mode != "view" && mode != "edit" {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Mode tidak valid. Gunakan view atau edit.",
            ));
        }
    }

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let actor = Actor {
        user_id: user.user_id,
        roles,
    };
    let accessible = can_access(&state.pool, &actor, &media).await?;
    if !accessible {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses ke dokumen ini.",
        ));
    }

    // PDF selalu view-only, seperti `buildEditorPayload`.
    let can_edit_doc = extension != "pdf" && can_edit(&actor, &media, accessible);
    let mode = match requested_mode.as_deref() {
        Some("edit") => {
            if !can_edit_doc {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "Anda tidak memiliki izin mengedit dokumen ini.",
                ));
            }
            "edit"
        }
        Some("view") => "view",
        _ => {
            if can_edit_doc {
                "edit"
            } else {
                "view"
            }
        }
    };
    let is_edit = mode == "edit";

    let user_name: String = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(user.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .unwrap_or_default();

    let key = document_key(&media);
    let download_url = onlyoffice::download_url(&state.app_url, media.id, &settings);
    let mut config = json!({
        "document": {
            "fileType": extension,
            "key": key,
            "title": media.file_name,
            "url": download_url,
            "permissions": {
                "edit": is_edit,
                "download": true,
                "print": true,
                "review": is_edit,
                "comment": is_edit,
                "fillForms": is_edit,
                "editCommentAuthorOnly": false,
            },
        },
        "documentType": document_type(&extension),
        "editorConfig": {
            "mode": mode,
            "lang": "id",
            "callbackUrl": callback_url(&state.app_url),
            "user": {
                "id": user.user_id.to_string(),
                "name": user_name,
            },
            "customization": {
                "forcesave": is_edit,
                // compactToolbar saja, jangan toolbarNoTabs (lihat OnlyOfficeService).
                "compactToolbar": !is_edit,
                "feedback": false,
                "help": false,
                "compactHeader": true,
                "autosave": is_edit,
            },
        },
    });
    if !settings.jwt_secret.is_empty() {
        let token = onlyoffice::jwt_encode(&config, &settings.jwt_secret);
        config["token"] = json!(token);
    }

    Ok(Json(json!({
        "data": {
            "documentServerUrl": format!("{}/", settings.server_url),
            "config": config,
            "mode": mode,
            "can_edit": can_edit_doc,
            "download_url": download_url,
            "media": {
                "id": media.id,
                "file_name": media.file_name,
                "mime_type": media.mime_type,
                "extension": extension,
            },
        }
    })))
}

// ---------------------------------------------------------------------------
// POST /api/onlyoffice/callback
// ---------------------------------------------------------------------------

/// `parseDocumentKey`: `^media_(\d+)_`.
fn parse_document_key(key: &str) -> Option<u64> {
    let rest = key.strip_prefix("media_")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || rest[digits.len()..].chars().next() != Some('_') {
        return None;
    }
    digits.parse().ok()
}

/// SSRF guard: host URL harus sama dengan host Document Server (port diabaikan).
fn url_from_document_server(download_url: &str, server_url: &str) -> bool {
    let host = |raw: &str| {
        reqwest::Url::parse(raw)
            .ok()
            .and_then(|u| u.host_str().map(str::to_lowercase))
    };
    match (host(server_url), host(download_url)) {
        (Some(ds), Some(url)) => ds == url,
        _ => false,
    }
}

/// Ambil dokumen dari Document Server, timpa file, dan perbarui `size` serta `updated_at`.
/// Setiap kegagalan menghasilkan `error: 1`, seperti blok `try` di Laravel.
async fn save_document(
    pool: &MySqlPool,
    media: &MediaRow,
    download_url: &str,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let url = reqwest::Url::parse(download_url).map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err("Gagal mengunduh dokumen dari ONLYOFFICE.".to_string());
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    let dir = media::media_dir(media.id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(dir.join(&media.file_name), &bytes)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE media SET size = ?, updated_at = NOW() WHERE id = ?")
        .bind(bytes.len() as u64)
        .bind(media.id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// `POST /api/onlyoffice/callback`: callback Document Server (tanpa auth, dilindungi JWT).
/// Semua kegagalan memakai HTTP 200 dengan `error: 1`, sama dengan Laravel.
pub async fn callback(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let settings = Settings::from_env();
    let secret = callback_secret(&settings);
    if secret.is_empty() {
        tracing::error!("ONLYOFFICE callback rejected: no JWT secret configured");
        return Ok(Json(json!({ "error": 1 })));
    }

    let mut payload = request_input(raw_query.as_deref(), &headers, &body);
    let decoded = callback_token(&payload, &headers).and_then(|token| match token {
        Value::String(t) => jwt_decode(&t, &secret),
        _ => None,
    });
    let Some(decoded) = decoded else {
        tracing::warn!("ONLYOFFICE callback rejected: invalid or missing JWT");
        return Ok(Json(json!({ "error": 1 })));
    };
    for (k, v) in decoded {
        payload.insert(k, v);
    }

    // 2 = siap disimpan, 6 = force save. Status lain dijawab error 0.
    let status = php_int(payload.get("status"));
    if status != 2 && status != 6 {
        return Ok(Json(json!({ "error": 0 })));
    }

    let key = php_string(payload.get("key"));
    let download_url = match payload.get("url") {
        Some(Value::String(u)) if truthy(payload.get("url")) => u.clone(),
        _ => String::new(),
    };
    if download_url.is_empty() || key.is_empty() {
        tracing::warn!("ONLYOFFICE callback missing url/key");
        return Ok(Json(json!({ "error": 1 })));
    }

    if !url_from_document_server(&download_url, &settings.server_url) {
        tracing::warn!("ONLYOFFICE callback rejected: url host mismatch");
        return Ok(Json(json!({ "error": 1 })));
    }

    let Some(media_id) = parse_document_key(&key) else {
        return Ok(Json(json!({ "error": 1 })));
    };
    let Some(media) = find_media(&state.pool, media_id).await? else {
        return Ok(Json(json!({ "error": 1 })));
    };
    if !owner_exists(&state.pool, &media).await? {
        return Ok(Json(json!({ "error": 1 })));
    }

    match save_document(&state.pool, &media, &download_url).await {
        Ok(()) => Ok(Json(json!({ "error": 0 }))),
        Err(message) => {
            tracing::error!("ONLYOFFICE callback save failed: media_id={media_id} {message}");
            Ok(Json(json!({ "error": 1 })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_key_parses_only_media_prefix() {
        assert_eq!(parse_document_key("media_12_1700000000"), Some(12));
        assert_eq!(parse_document_key("media_12"), None);
        assert_eq!(parse_document_key("media__1_x"), None);
        assert_eq!(parse_document_key("foto_12_1"), None);
    }

    #[test]
    fn php_int_matches_cast_rules() {
        assert_eq!(php_int(Some(&json!("2abc"))), 2);
        assert_eq!(php_int(Some(&json!(6))), 6);
        assert_eq!(php_int(Some(&json!(null))), 0);
        assert_eq!(php_int(Some(&json!(true))), 1);
    }

    #[test]
    fn truthy_matches_php_empty() {
        assert!(!truthy(Some(&json!("0"))));
        assert!(!truthy(Some(&json!(""))));
        assert!(truthy(Some(&json!("http://x"))));
    }

    #[test]
    fn jwt_round_trip_and_rejects_bad_signature() {
        let payload = json!({"status": 2, "key": "media_1_2"});
        let token = onlyoffice::jwt_encode(&payload, "rahasia");
        let decoded = jwt_decode(&token, "rahasia").expect("token valid");
        assert_eq!(decoded.get("status"), Some(&json!(2)));
        assert!(jwt_decode(&token, "salah").is_none());
        assert!(jwt_decode("a.b", "rahasia").is_none());
    }

    #[test]
    fn jwt_rejects_expired_exp_claim() {
        let token = onlyoffice::jwt_encode(&json!({"exp": 1}), "rahasia");
        assert!(jwt_decode(&token, "rahasia").is_none());
    }

    #[test]
    fn same_host_ignores_port_and_case() {
        assert!(url_from_document_server(
            "http://DS.local:8443/cache/x",
            "http://ds.local"
        ));
        assert!(!url_from_document_server("http://evil.local/x", "http://ds.local"));
    }

    #[test]
    fn extension_is_lowercase_suffix() {
        assert_eq!(extension_of("Laporan.DOCX"), "docx");
        assert_eq!(extension_of("tanpa-ekstensi"), "");
    }
}
