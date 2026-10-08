//! Berkas media ala Spatie Media Library (disk `public`), dipakai foto dan berkas.
//!
//! Path: `{storage}/{media_id}/{file_name}`, URL: `{APP_URL}/storage/{media_id}/{file_name}`.
//! Akar storage dibaca dari env `PUBLIC_STORAGE_PATH`, default `storage/app/public` di repo Laravel.
//! Thumbnail tidak dibuat (lihat `docs/migration/foto-berkas.md`).

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
pub async fn attach(
    tx: &mut Transaction<'_, MySql>,
    model_type: &str,
    model_id: u64,
    collection: &str,
    upload: &Upload,
    mime: &str,
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
    let ext = upload.extension();
    let file_name = if ext.is_empty() {
        uuid.clone()
    } else {
        format!("{uuid}.{ext}")
    };
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
    tokio::fs::write(dir.join(&file_name), &upload.bytes)
        .await
        .map_err(internal)?;

    Ok(Stored {
        media_id,
        file_name,
        dir,
    })
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

/// URL berkas pertama koleksi (`getFirstMediaUrl`). Hanya disk `public` yang URL-nya diketahui.
pub async fn first_url(
    pool: &MySqlPool,
    app_url: &str,
    model_type: &str,
    model_id: u64,
    collection: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, disk, file_name FROM media \
         WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY order_column, id LIMIT 1",
    )
    .bind(model_type)
    .bind(model_id)
    .bind(collection)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    let disk: String = r.try_get("disk")?;
    if disk != "public" {
        return Ok(None);
    }
    let media_id: u64 = r.try_get("id")?;
    let file_name: String = r.try_get("file_name")?;
    Ok(Some(format!(
        "{}/storage/{media_id}/{file_name}",
        app_url.trim_end_matches('/')
    )))
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
