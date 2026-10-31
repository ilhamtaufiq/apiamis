//! Buku kontrak: konversi dokumen non-PDF ke PDF untuk digabung di browser. Hanya admin.

use axum::{
    extract::{Multipart, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use shared::ApiError;

use crate::{media, notifications::require_admin, onlyoffice, require_auth, AppState};

/// `POST /api/buku-kontrak/konversi` (multipart `file`): kembalikan PDF.
/// PDF dikembalikan apa adanya. Dokumen lain dikonversi lewat ONLYOFFICE.
pub async fn konversi(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let mut berkas: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?
    {
        if field.name() == Some("file") {
            let nama = field.file_name().unwrap_or("berkas").to_string();
            let isi = field
                .bytes()
                .await
                .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
            berkas = Some((nama, isi.to_vec()));
        }
    }
    let Some((nama, isi)) = berkas else {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Berkas wajib diisi."));
    };
    if isi.len() > media::MAX_FILE_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Berkas melebihi batas 50 MB."));
    }

    let ext = nama.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    if ext == "pdf" {
        return Ok(([(header::CONTENT_TYPE, "application/pdf")], isi).into_response());
    }

    let Some(pdf) = onlyoffice::convert_upload_to_pdf(&state.app_url, &isi, &ext, &nama).await else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Berkas .{ext} tidak bisa dikonversi ke PDF. Pastikan ONLYOFFICE aktif dan format didukung."),
        ));
    };
    Ok(([(header::CONTENT_TYPE, "application/pdf")], pdf).into_response())
}
