//! `GET /api/pekerjaan/{id}/download-all-berkas?format=original|pdf`, setara `PekerjaanController@downloadAllBerkas`.
//!
//! Semua berkas pekerjaan dimasukkan ke satu zip dengan metode STORE. Nama berkas di dalam zip memakai
//! `{jenis_dokumen}_{media_id}.{ext}`, dengan akhiran `_2`, `_3`, dan seterusnya bila bentrok.
//! `format=pdf` memakai konversi ONLYOFFICE yang sama dengan `export-pdf`. Berkas yang gagal dikonversi
//! atau tidak terbaca dilewati, seperti Laravel.
//!
//! Berbeda dari Laravel:
//! - Pekerjaan di luar scope `byUserRole()` mendapat 403 (seperti `media`).
//! - Zip disusun di memori, bukan distream. Laravel memakai ZipStream dengan batas memori 512M.
//! - Preflight dan pesan 404 sama. Format selain `pdf` dianggap `original`, seperti Laravel.

use std::{collections::HashSet, io::Cursor, path::PathBuf};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use regex::Regex;
use serde_json::json;
use shared::ApiError;
use sqlx::Row;
use std::collections::HashMap;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{access, media, media::internal, onlyoffice, pekerjaan, require_auth, AppState};

const COLLECTION: &str = "berkas/dokumen";
const BERKAS_MODEL: &str = "App\\Models\\Berkas";

/// Berkas pekerjaan dengan media pertamanya (`getFirstMedia('berkas/dokumen')`).
struct Item {
    jenis_dokumen: String,
    media: media::MediaInfo,
}

/// `preg_replace('/[^\w\-.]+/u', '_', ...)`: setiap rangkaian karakter di luar huruf, angka, `_`, `-`, dan `.` menjadi `_`.
fn sanitize(value: &str, re: &Regex) -> String {
    re.replace_all(value, "_").into_owned()
}

/// `GET /api/pekerjaan/{id}/download-all-berkas`.
pub async fn download_all_berkas(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let pekerjaan = pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !access::user_can_access(&state.pool, user.user_id, &roles, id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ));
    }

    let want_pdf = query.get("format").map(|f| f.to_lowercase()).as_deref() == Some("pdf");

    let berkas = sqlx::query("SELECT CAST(id AS SIGNED) AS id, jenis_dokumen FROM tbl_berkas WHERE pekerjaan_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    if berkas.is_empty() {
        return Ok(not_found("Tidak ada berkas untuk diunduh"));
    }

    let mut items = Vec::new();
    for r in &berkas {
        let berkas_id: i64 = r.try_get("id").map_err(internal)?;
        let jenis: String = r
            .try_get::<Option<String>, _>("jenis_dokumen")
            .map_err(internal)?
            .unwrap_or_default();
        if let Some(m) = media::first_media(&state.pool, BERKAS_MODEL, berkas_id as u64, COLLECTION)
            .await
            .map_err(internal)?
        {
            items.push(Item {
                jenis_dokumen: jenis,
                media: m,
            });
        }
    }

    // Preflight: minimal satu berkas harus bisa dibaca dari disk.
    let readable = items.iter().any(|it| {
        media::media_dir(it.media.id)
            .join(&it.media.file_name)
            .is_file()
    });
    if !readable {
        return Ok(not_found("Tidak ada file berkas yang dapat diunduh"));
    }

    let unsafe_chars = Regex::new(r"[^\w\-.]+").map_err(internal)?;
    let base = sanitize(pekerjaan.nama_paket.as_deref().unwrap_or(""), &unsafe_chars);
    let base = if base.is_empty() {
        format!("berkas_{id}")
    } else {
        base
    };
    let suffix = if want_pdf { "_PDF" } else { "" };
    let file_name = format!("{base}{suffix}.zip");

    let archive = build_zip(&state, &items, want_pdf, &unsafe_chars).await?;

    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            ),
            (header::CACHE_CONTROL, "no-store, private".to_string()),
            (
                axum::http::HeaderName::from_static("x-accel-buffering"),
                "no".to_string(),
            ),
        ],
        Body::from(archive),
    )
        .into_response())
}

/// Isi zip: satu entri per berkas, STORE, dengan nama yang tidak bentrok.
async fn build_zip(
    state: &AppState,
    items: &[Item],
    want_pdf: bool,
    unsafe_chars: &Regex,
) -> Result<Vec<u8>, ApiError> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut used: HashSet<String> = HashSet::new();
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for item in items {
        let original: PathBuf = media::media_dir(item.media.id).join(&item.media.file_name);
        let mut extension = item
            .media
            .file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default();

        let bytes = if want_pdf {
            match onlyoffice::media_pdf(
                &state.pool,
                &state.app_url,
                item.media.id,
                &item.media.file_name,
                &original,
            )
            .await
            {
                Some(pdf) => {
                    extension = "pdf".to_string();
                    Some(pdf)
                }
                None => tokio::fs::read(&original).await.ok(),
            }
        } else {
            tokio::fs::read(&original).await.ok()
        };
        let Some(bytes) = bytes else {
            // File tidak terbaca: dilewati, seperti `is_readable` di Laravel.
            continue;
        };

        let label = {
            let s = sanitize(&item.jenis_dokumen, unsafe_chars);
            if s.is_empty() {
                "berkas".to_string()
            } else {
                s
            }
        };
        let ext_part = if extension.is_empty() {
            String::new()
        } else {
            format!(".{extension}")
        };
        let mut inner = format!("{label}_{}{ext_part}", item.media.id);
        let mut n = 2;
        while used.contains(&inner) {
            inner = format!("{label}_{}_{n}{ext_part}", item.media.id);
            n += 1;
        }
        used.insert(inner.clone());

        // Gagal menulis satu entri tidak menghentikan entri lain, seperti `report($e)` di Laravel.
        if zip.start_file(inner, options).is_ok() {
            use std::io::Write;
            let _ = zip.write_all(&bytes);
        }
    }

    let archive = zip.finish().map_err(internal)?.into_inner();
    Ok(archive)
}

fn not_found(message: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "message": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_runs_of_unsafe_characters() {
        let re = Regex::new(r"[^\w\-.]+").unwrap();
        assert_eq!(
            sanitize("Rehab / Jembatan (Tahap 1)", &re),
            "Rehab_Jembatan_Tahap_1_"
        );
        assert_eq!(sanitize("SPK-2025.v2", &re), "SPK-2025.v2");
        assert_eq!(sanitize("", &re), "");
    }
}
