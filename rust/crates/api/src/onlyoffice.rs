//! Integrasi ONLYOFFICE Document Server untuk konversi berkas ke PDF.
//!
//! Setara `OnlyOffice\OnlyOfficeConverter`, `OnlyOfficeDownloadToken`, `OnlyOfficeJwt`,
//! `DocumentPdfConverter`, dan `OnlyOfficeController::download` di Laravel.
//! Konfigurasi dibaca dari environment: `ONLYOFFICE_DOCUMENT_SERVER_URL`, `ONLYOFFICE_JWT_SECRET`,
//! dan `ONLYOFFICE_DOWNLOAD_TOKEN_TTL` (menit, default 120). Bila `ONLYOFFICE_JWT_SECRET` kosong,
//! token unduhan memakai `APP_KEY`, sama dengan `OnlyOfficeDownloadToken::secret()`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use shared::ApiError;
use sqlx::Row;

use crate::{media, require_auth, AppState};

type HmacSha256 = Hmac<Sha256>;

const BERKAS_MODEL: &str = "App\\Models\\Berkas";
const BERKAS_COLLECTION: &str = "berkas/dokumen";
const CONVERTIBLE: [&str; 18] = [
    "doc", "docx", "odt", "rtf", "txt", "xls", "xlsx", "ods", "csv", "ppt", "pptx", "odp", "jpg",
    "jpeg", "png", "gif", "bmp", "webp",
];
const MAX_ATTEMPTS: usize = 40;
const CONVERT_MESSAGE: &str = "Gagal mengonversi berkas ke PDF melalui ONLYOFFICE. Pastikan Document Server aktif dan format file didukung.";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Klien HTTP bersama. Batas waktu per permintaan 10 detik, sama dengan Laravel.
fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("klien HTTP ONLYOFFICE")
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// URL Document Server tanpa garis miring di akhir. Kosong berarti tidak aktif.
    pub server_url: String,
    /// Secret JWT. Kosong berarti request konversi tidak diberi token.
    pub jwt_secret: String,
}

impl Settings {
    pub fn from_env() -> Self {
        Self {
            server_url: std::env::var("ONLYOFFICE_DOCUMENT_SERVER_URL")
                .unwrap_or_default()
                .trim()
                .trim_end_matches('/')
                .to_string(),
            jwt_secret: std::env::var("ONLYOFFICE_JWT_SECRET").unwrap_or_default(),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.server_url.is_empty()
    }

    /// Secret untuk token unduhan: JWT secret, atau `APP_KEY` bila JWT secret kosong.
    fn download_secret(&self) -> String {
        if self.jwt_secret.is_empty() {
            std::env::var("APP_KEY").unwrap_or_default()
        } else {
            self.jwt_secret.clone()
        }
    }
}

fn download_ttl_minutes() -> i64 {
    std::env::var("ONLYOFFICE_DOWNLOAD_TOKEN_TTL")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(120)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// JWT HS256 dengan header dan body base64url tanpa padding (`OnlyOfficeJwt::encode`).
pub fn jwt_encode(payload: &Value, secret: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "HS256", "typ": "JWT" }).to_string());
    let body = URL_SAFE_NO_PAD.encode(payload.to_string());
    let signing_input = format!("{header}.{body}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC menerima kunci dengan panjang apa pun");
    mac.update(signing_input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{signing_input}.{signature}")
}

/// `OnlyOfficeDownloadToken::make`: HMAC-SHA256 hex dari `onlyoffice:{id}:{expires}`.
pub fn download_token(media_id: u64, expires: i64, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC menerima kunci dengan panjang apa pun");
    mac.update(format!("onlyoffice:{media_id}:{expires}").as_bytes());
    hex(&mac.finalize().into_bytes())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// URL unduhan untuk Document Server (`OnlyOfficeDownloadToken::buildDownloadUrl`).
pub fn download_url(app_url: &str, media_id: u64, settings: &Settings) -> String {
    let expires = now_secs() + download_ttl_minutes() * 60;
    let token = download_token(media_id, expires, &settings.download_secret());
    format!(
        "{}/api/onlyoffice/media/{media_id}/download?expires={expires}&token={token}",
        app_url.trim_end_matches('/')
    )
}

/// Permintaan konversi ke `/converter` sampai `endConvert`, lalu unduh PDF hasilnya.
/// Mengembalikan `None` untuk setiap kegagalan (sama dengan `OnlyOfficeConverter`).
pub async fn convert_to_pdf(
    settings: &Settings,
    file_url: &str,
    filetype: &str,
    key: &str,
    title: &str,
) -> Option<Vec<u8>> {
    if !settings.enabled() {
        return None;
    }
    let payload = json!({
        "async": true,
        "filetype": filetype.to_lowercase(),
        "key": key,
        "outputtype": "pdf",
        "title": title,
        "url": file_url,
    });
    let body = if settings.jwt_secret.is_empty() {
        payload
    } else {
        json!({ "token": jwt_encode(&payload, &settings.jwt_secret) })
    };

    let mut converter = reqwest::Url::parse(&format!("{}/converter", settings.server_url)).ok()?;
    converter.query_pairs_mut().append_pair("shardkey", key);

    let mut converted: Option<String> = None;
    for _ in 0..MAX_ATTEMPTS {
        let response = match http_client()
            .post(converter.clone())
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("ONLYOFFICE converter request failed: {e}");
                return None;
            }
        };
        if !response.status().is_success() {
            tracing::warn!("ONLYOFFICE converter HTTP error: {}", response.status());
            return None;
        }
        let result: Value = response.json().await.ok()?;
        let object = result.as_object()?;
        // `!empty($result['error'])`: nilai kosong, 0, atau false dianggap tidak ada error.
        if object.get("error").is_some_and(|e| {
            !matches!(e, Value::Null | Value::Bool(false)) && e != &json!(0) && e != &json!("")
        }) {
            tracing::warn!("ONLYOFFICE converter returned error: {result}");
            return None;
        }
        if object.get("endConvert").and_then(Value::as_bool) == Some(true) {
            converted = object
                .get("fileUrl")
                .and_then(Value::as_str)
                .map(str::to_string);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let file_url = converted.filter(|u| !u.is_empty())?;

    // SSRF guard: URL hasil harus memakai host Document Server (port diabaikan, sama dengan Laravel).
    let server_host = reqwest::Url::parse(&settings.server_url)
        .ok()?
        .host_str()?
        .to_lowercase();
    let result_url = reqwest::Url::parse(&file_url).ok()?;
    if result_url.host_str().map(str::to_lowercase).as_deref() != Some(server_host.as_str()) {
        tracing::warn!("ONLYOFFICE converter rejected: host mismatch");
        return None;
    }
    let response = http_client().get(result_url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().await.ok().map(|b| b.to_vec())
}

/// `GET /api/onlyoffice/media/{id}/download?expires=&token=`: rute publik untuk Document Server.
pub async fn media_download(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let expires: i64 = query
        .get("expires")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let token = query.get("token").cloned().unwrap_or_default();
    let settings = Settings::from_env();
    let expected = download_token(id, expires, &settings.download_secret());
    let valid = expires > 0
        && expires >= now_secs()
        && !token.is_empty()
        && constant_time_eq(&expected, &token);
    if !valid {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Tautan unduhan tidak valid atau kedaluwarsa.",
        ));
    }

    let row = sqlx::query("SELECT file_name, COALESCE(mime_type, '') FROM media WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "File tidak ditemukan."))?;
    let file_name: String = row.try_get(0).map_err(internal)?;
    let mime: String = row.try_get(1).map_err(internal)?;
    let path = media::media_dir(id).join(&file_name);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::new(StatusCode::NOT_FOUND, "File tidak ditemukan."))?;

    // Sanitasi nama berkas dari karakter kutip dan CRLF, seperti Laravel.
    let safe_name: String = file_name
        .chars()
        .filter(|c| !matches!(c, '"' | '\r' | '\n'))
        .collect();
    let content_type = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{safe_name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// `GET /api/berkas/{id}/export-pdf` (`BerkasController::convertToPdf`).
pub async fn berkas_export_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_berkas WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    if exists == 0 {
        return Err(ApiError::not_found());
    }
    let media_info = media::first_media(&state.pool, BERKAS_MODEL, id, BERKAS_COLLECTION)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Berkas tidak ditemukan"))?;

    let file_name = media_info.file_name.clone();
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_else(|| file_name.clone());
    let download_name = format!("{stem}.pdf");
    let path: PathBuf = media::media_dir(media_info.id).join(&file_name);

    // Berkas PDF dikembalikan apa adanya. File yang hilang dari disk memakai pesan dari Laravel.
    if !path.exists() {
        let message = if extension == "pdf" {
            "Berkas sudah berformat PDF."
        } else {
            CONVERT_MESSAGE
        };
        return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, message));
    }
    if extension == "pdf" {
        let bytes = tokio::fs::read(&path).await.map_err(internal)?;
        return Ok(pdf_response(bytes, &download_name));
    }

    let settings = Settings::from_env();
    if !settings.enabled() || !CONVERTIBLE.contains(&extension.as_str()) {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            CONVERT_MESSAGE,
        ));
    }
    let updated_ts: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(UNIX_TIMESTAMP(updated_at) AS SIGNED) FROM media WHERE id = ?",
    )
    .bind(media_info.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?
    .flatten();
    let key = format!(
        "media_{}_{}",
        media_info.id,
        updated_ts.unwrap_or_else(now_secs)
    );
    let source_url = download_url(&state.app_url, media_info.id, &settings);

    match convert_to_pdf(&settings, &source_url, &extension, &key, &file_name).await {
        Some(bytes) => Ok(pdf_response(bytes, &download_name)),
        None => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            CONVERT_MESSAGE,
        )),
    }
}

fn pdf_response(bytes: Vec<u8>, download_name: &str) -> Response {
    let safe: String = download_name
        .chars()
        .filter(|c| !matches!(c, '"' | '\r' | '\n'))
        .collect();
    (
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}
