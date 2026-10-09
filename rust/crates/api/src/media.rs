//! Berkas media ala Spatie Media Library (disk `public`), dipakai foto dan berkas.
//!
//! Path: `{storage}/{media_id}/{file_name}`, URL: `{APP_URL}/storage/{media_id}/{file_name}`.
//! Akar storage dibaca dari env `PUBLIC_STORAGE_PATH`, default `storage/app/public` di repo Laravel.
//! Thumbnail `thumb` dibuat sinkron seperti `->nonQueued()` di Laravel: crop 120x120 ke `conversions/`.

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::notify::new_uuid;

/// Batas `max:51200` (KB) pada aturan `file`.
pub const MAX_FILE_BYTES: usize = 51_200 * 1024;

/// Berkas unggahan: nama asli dan isinya.
#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub original_name: String,
    pub bytes: Vec<u8>,
}

impl Upload {
    /// `getClientOriginalExtension()`: ekstensi dari nama asli, apa adanya.
    pub fn extension(&self) -> String {
        Path::new(&self.original_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string()
    }

    /// `pathinfo(..., PATHINFO_FILENAME)`: nama tanpa ekstensi, dipakai sebagai `name` media.
    pub fn stem(&self) -> String {
        Path::new(&self.original_name)
            .file_stem()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string()
    }
}

/// Tipe MIME JPEG atau PNG dari isi berkas (magic number). `None` bila bukan keduanya.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else {
        None
    }
}

/// Akar `storage/app/public`.
pub fn storage_root() -> PathBuf {
    std::env::var_os("PUBLIC_STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/public"
            ))
        })
}

/// Direktori satu media: `{storage}/{media_id}`.
pub fn media_dir(media_id: u64) -> PathBuf {
    storage_root().join(media_id.to_string())
}

pub fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Media yang sudah disimpan. `dir` perlu dihapus bila transaksi gagal.
pub struct Stored {
    pub media_id: u64,
    pub file_name: String,
    pub dir: PathBuf,
}

/// Simpan berkas ke disk dan tulis satu baris `media` dalam transaksi.
/// Berkas ditulis sebelum commit: pemanggil menghapus `Stored::dir` bila transaksi gagal.
/// `thumb`: buat konversi `thumb` untuk gambar (hanya foto, `Foto::registerMediaConversions`).
pub async fn attach(
    tx: &mut Transaction<'_, MySql>,
    model_type: &str,
    model_id: u64,
    collection: &str,
    upload: &Upload,
    mime: &str,
    thumb: bool,
) -> Result<Stored, ApiError> {
    attach_named(tx, model_type, model_id, collection, upload, mime, thumb, None).await
}

/// Sama dengan `attach`, tetapi nama berkas di disk bisa ditentukan (`usingFileName` di Laravel).
/// `file_name` `None` memakai `{uuid}.{ext}`.
#[allow(clippy::too_many_arguments)]
pub async fn attach_named(
    tx: &mut Transaction<'_, MySql>,
    model_type: &str,
    model_id: u64,
    collection: &str,
    upload: &Upload,
    mime: &str,
    thumb: bool,
    file_name: Option<String>,
) -> Result<Stored, ApiError> {
    let next: i64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(order_column), 0) + 1 AS SIGNED) FROM media WHERE model_type = ? AND model_id = ?",
    )
    .bind(model_type)
    .bind(model_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)?;

    let uuid = new_uuid();
    let file_name = file_name.unwrap_or_else(|| {
        let ext = upload.extension();
        if ext.is_empty() {
            uuid.clone()
        } else {
            format!("{uuid}.{ext}")
        }
    });
    let res = sqlx::query(
        "INSERT INTO media (model_type, model_id, uuid, collection_name, name, file_name, mime_type, disk, conversions_disk, \
         size, manipulations, custom_properties, generated_conversions, responsive_images, order_column, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'public', 'public', ?, '[]', '[]', '[]', '[]', ?, NOW(), NOW())",
    )
    .bind(model_type)
    .bind(model_id)
    .bind(&uuid)
    .bind(collection)
    .bind(upload.stem())
    .bind(&file_name)
    .bind(mime)
    .bind(upload.bytes.len() as u64)
    .bind(next)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    let media_id = res.last_insert_id();

    let dir = media_dir(media_id);
    tokio::fs::create_dir_all(&dir).await.map_err(internal)?;
    let written = write_original(&dir, &file_name, upload, media_id, mime, thumb, tx).await;
    if let Err(e) = written {
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return Err(e);
    }

    Ok(Stored {
        media_id,
        file_name,
        dir,
    })
}

/// Tulis berkas asli dan, untuk gambar, thumbnail `thumb` (`generated_conversions`).
async fn write_original(
    dir: &Path,
    file_name: &str,
    upload: &Upload,
    media_id: u64,
    mime: &str,
    thumb: bool,
    tx: &mut Transaction<'_, MySql>,
) -> Result<(), ApiError> {
    tokio::fs::write(dir.join(file_name), &upload.bytes)
        .await
        .map_err(internal)?;
    if !thumb || !mime.starts_with("image/") {
        return Ok(());
    }
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_string();
    let source = upload.bytes.clone();
    let thumb = tokio::task::spawn_blocking(move || make_thumb(&source))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    let thumb_dir = dir.join("conversions");
    tokio::fs::create_dir_all(&thumb_dir)
        .await
        .map_err(internal)?;
    let stem = file_name
        .strip_suffix(&format!(".{ext}"))
        .unwrap_or(file_name);
    tokio::fs::write(thumb_dir.join(thumb_file_name(stem, &ext)), thumb)
        .await
        .map_err(internal)?;
    sqlx::query("UPDATE media SET generated_conversions = '{\"thumb\": true}' WHERE id = ?")
        .bind(media_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

/// Nama berkas thumbnail seperti Spatie: `{nama}-thumb.{ext}`.
pub fn thumb_file_name(stem: &str, ext: &str) -> String {
    format!("{stem}-thumb.{ext}")
}

/// Crop 120x120 (`Fit::Crop`), lalu encode dengan format sumber.
pub fn make_thumb(source: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory(source)?;
    let thumb = img.resize_to_fill(120, 120, image::imageops::FilterType::Lanczos3);
    let format = image::guess_format(source)?;
    let mut out = std::io::Cursor::new(Vec::new());
    thumb.write_to(&mut out, format)?;
    Ok(out.into_inner())
}

/// Ringkasan `regenerate_missing_thumbs`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ThumbReport {
    /// Baris media gambar yang diperiksa.
    pub checked: u64,
    /// Thumbnail sudah ada.
    pub present: u64,
    /// Thumbnail dibuat (atau akan dibuat, pada dry-run).
    pub created: u64,
    /// Berkas asli tidak ada di disk, jadi tidak bisa dibuatkan thumbnail.
    pub missing_original: u64,
    /// Gagal membaca atau men-decode berkas asli.
    pub failed: u64,
}

/// Membuat ulang thumbnail yang hilang untuk satu koleksi (`model_type` + `collection`).
/// Hanya menulis berkas thumbnail yang belum ada. Berkas asli tidak diubah.
/// Dengan `dry_run`, tidak ada yang ditulis: hanya dihitung.
pub async fn regenerate_missing_thumbs(
    pool: &MySqlPool,
    model_type: &str,
    collection: &str,
    dry_run: bool,
) -> Result<ThumbReport, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, file_name, mime_type FROM media \
         WHERE model_type = ? AND collection_name = ? ORDER BY id",
    )
    .bind(model_type)
    .bind(collection)
    .fetch_all(pool)
    .await?;

    let mut report = ThumbReport::default();
    for row in rows {
        let mime: String = row.try_get("mime_type")?;
        if !mime.starts_with("image/") {
            continue;
        }
        report.checked += 1;
        let id = row.try_get::<i64, _>("id")? as u64;
        let file_name: String = row.try_get("file_name")?;

        let dir = media_dir(id);
        let original = dir.join(&file_name);
        if !tokio::fs::try_exists(&original).await.unwrap_or(false) {
            report.missing_original += 1;
            continue;
        }

        let ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_string();
        let stem = file_name
            .strip_suffix(&format!(".{ext}"))
            .unwrap_or(&file_name)
            .to_string();
        let thumb_path = dir.join("conversions").join(thumb_file_name(&stem, &ext));
        if tokio::fs::try_exists(&thumb_path).await.unwrap_or(false) {
            report.present += 1;
            continue;
        }
        if dry_run {
            report.created += 1;
            continue;
        }

        let made = match tokio::fs::read(&original).await {
            Ok(bytes) => tokio::task::spawn_blocking(move || make_thumb(&bytes))
                .await
                .ok()
                .and_then(Result::ok),
            Err(_) => None,
        };
        let Some(thumb) = made else {
            tracing::warn!(media_id = id, file = %file_name, "gagal membuat thumbnail");
            report.failed += 1;
            continue;
        };

        tokio::fs::create_dir_all(thumb_path.parent().unwrap_or(&dir)).await.ok();
        if tokio::fs::write(&thumb_path, thumb).await.is_err() {
            report.failed += 1;
            continue;
        }
        sqlx::query("UPDATE media SET generated_conversions = '{\"thumb\": true}' WHERE id = ?")
            .bind(id as i64)
            .execute(pool)
            .await?;
        report.created += 1;
    }
    Ok(report)
}

/// Hapus semua media satu koleksi: baris `media` dalam transaksi, direktori berkasnya dikembalikan
/// untuk dihapus setelah commit. `keep` tidak ikut dihapus (dipakai saat mengganti berkas).
pub async fn delete_collection(
    tx: &mut Transaction<'_, MySql>,
    model_type: &str,
    model_id: u64,
    collection: &str,
    keep: Option<u64>,
) -> Result<Vec<PathBuf>, ApiError> {
    let ids: Vec<u64> = sqlx::query_scalar(
        "SELECT id FROM media WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY id",
    )
    .bind(model_type)
    .bind(model_id)
    .bind(collection)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)?;

    let mut dirs = Vec::new();
    for id in ids.into_iter().filter(|id| Some(*id) != keep) {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
        dirs.push(media_dir(id));
    }
    Ok(dirs)
}

/// Hapus direktori berkas. Gagal dibaca tidak menggagalkan request (sama dengan Spatie yang
/// hanya memberi peringatan).
pub async fn remove_dirs(dirs: &[PathBuf]) {
    for dir in dirs {
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}

/// Metadata satu media (untuk `BerkasResource`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub id: u64,
    pub file_name: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
}

/// Media pertama koleksi (`getFirstMedia`), urut `order_column`.
pub async fn first_media(
    pool: &MySqlPool,
    model_type: &str,
    model_id: u64,
    collection: &str,
) -> Result<Option<MediaInfo>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, file_name, name, COALESCE(mime_type, '') AS mime_type, \
         CAST(size AS UNSIGNED) AS size FROM media \
         WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY order_column, id LIMIT 1",
    )
    .bind(model_type)
    .bind(model_id)
    .bind(collection)
    .fetch_optional(pool)
    .await?;
    row.map(|r| -> Result<MediaInfo, sqlx::Error> {
        Ok(MediaInfo {
            id: r.try_get("id")?,
            file_name: r.try_get("file_name")?,
            name: r.try_get("name")?,
            mime_type: r.try_get("mime_type")?,
            size: r.try_get("size")?,
        })
    })
    .transpose()
}

/// Tipe MIME dari ekstensi nama berkas (perkiraan `finfo`). Tidak dikenal: `application/octet-stream`.
pub fn mime_for_name(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// URL berkas pertama koleksi (`getFirstMediaUrl`) dan URL thumbnail-nya (`getFirstMediaUrl(..., 'thumb')`).
/// Kosong bila tidak ada (string kosong, seperti Laravel). Hanya disk `public` yang URL-nya diketahui.
pub async fn first_urls(
    pool: &MySqlPool,
    app_url: &str,
    model_type: &str,
    model_id: u64,
    collection: &str,
) -> Result<(String, String), sqlx::Error> {
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, disk, file_name, CAST(generated_conversions AS CHAR) AS generated_conversions FROM media \
         WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY order_column, id LIMIT 1",
    )
    .bind(model_type)
    .bind(model_id)
    .bind(collection)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok((String::new(), String::new()));
    };
    let disk: String = r.try_get("disk")?;
    if disk != "public" {
        return Ok((String::new(), String::new()));
    }
    let media_id: u64 = r.try_get("id")?;
    let file_name: String = r.try_get("file_name")?;
    let generated: String = r.try_get("generated_conversions")?;
    let base = app_url.trim_end_matches('/');
    let original = format!("{base}/storage/{media_id}/{file_name}");
    let thumb = if generated.contains("\"thumb\"") {
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let stem = file_name
            .strip_suffix(&format!(".{ext}"))
            .unwrap_or(&file_name);
        format!(
            "{base}/storage/{media_id}/conversions/{}",
            thumb_file_name(stem, ext)
        )
    } else {
        String::new()
    };
    Ok((original, thumb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_jpeg_and_png_by_content_not_name() {
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0, 0]), Some("image/jpeg"));
        assert_eq!(
            image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]),
            Some("image/png")
        );
        assert_eq!(image_mime(b"%PDF-1.4"), None);
        assert_eq!(image_mime(&[]), None);
    }

    #[test]
    fn name_and_extension_follow_laravel_pathinfo() {
        let u = Upload {
            original_name: "Foto Lapangan.v2.JPG".into(),
            bytes: vec![],
        };
        assert_eq!(u.stem(), "Foto Lapangan.v2");
        assert_eq!(u.extension(), "JPG");
        let none = Upload {
            original_name: "tanpa-ekstensi".into(),
            bytes: vec![],
        };
        assert_eq!(none.extension(), "");
    }
}
