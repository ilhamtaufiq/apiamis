//! `GET /api/pekerjaan/{id}/download-all-berkas?format=original|pdf`, setara `PekerjaanController@downloadAllBerkas`.
//!
//! Semua berkas pekerjaan dimasukkan ke satu zip dengan metode STORE. Nama berkas di dalam zip memakai
//! `{jenis_dokumen}_{media_id}.{ext}`, dengan akhiran `_2`, `_3`, dan seterusnya bila bentrok.
//! `format=pdf` memakai konversi ONLYOFFICE yang sama dengan `export-pdf`. Berkas yang gagal dikonversi
//! atau tidak terbaca dilewati, seperti Laravel.
//!
//! Berbeda dari Laravel:
//! - Pekerjaan di luar scope `byUserRole()` mendapat 403 (seperti `media`).
//! - Zip didistream ke klien saat disusun (tanpa menampung seluruh arsip di memori). Penulis zip berjalan di
//!   thread blocking dan mengirim potongan lewat channel terbatas. Laravel memakai ZipStream dengan batas 512M.
//! - Preflight dan pesan 404 sama. Format selain `pdf` dianggap `original`, seperti Laravel.

use std::{
    collections::{HashMap, HashSet},
    io::{self, Cursor, Read, Write},
    path::{Path as FsPath, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::stream;
use regex::Regex;
use serde_json::json;
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use tokio::sync::mpsc;
use zip::{
    write::{SimpleFileOptions, StreamWriter},
    CompressionMethod, ZipWriter,
};

use crate::{access, media, media::internal, onlyoffice, pekerjaan, require_auth, AppState};

/// Ukuran potongan yang dikirim ke klien.
const CHUNK_BYTES: usize = 64 * 1024;
/// Jumlah potongan yang boleh antre sebelum penulis menunggu klien membaca.
const CHANNEL_CAPACITY: usize = 4;

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

    // Penulisan zip berjalan di thread blocking dan mengirim potongan lewat channel terbatas.
    // Bila klien memutus koneksi, `rx` di-drop dan penulisan berhenti pada pengiriman berikutnya.
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, io::Error>>(CHANNEL_CAPACITY);
    let job = ZipJob {
        pool: state.pool.clone(),
        app_url: state.app_url.clone(),
        items,
        want_pdf,
        unsafe_chars,
    };
    let handle = tokio::runtime::Handle::current();
    let worker_tx = tx.clone();
    let worker = tokio::task::spawn_blocking(move || {
        if let Err(e) = stream_zip(&handle, &worker_tx, &job) {
            if e.kind() != io::ErrorKind::BrokenPipe {
                tracing::warn!(pekerjaan_id = id, error = %e, "unduh semua berkas berhenti");
            }
            // Gagal mengirim berarti klien sudah pergi, dan tidak ada yang perlu diberi tahu.
            let _ = worker_tx.blocking_send(Err(e));
        }
    });
    // Bila thread penulis panik, kirim error agar respons tidak tampak lengkap.
    tokio::spawn(async move {
        if worker.await.is_err() {
            let _ = tx
                .send(Err(io::Error::other("penulisan zip berhenti tak terduga")))
                .await;
        }
    });
    let chunks = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });

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
        Body::from_stream(chunks),
    )
        .into_response())
}

/// Isi pekerjaan untuk penulis zip yang berjalan di thread blocking.
struct ZipJob {
    pool: MySqlPool,
    app_url: String,
    items: Vec<Item>,
    want_pdf: bool,
    unsafe_chars: Regex,
}

/// `Write` yang mengumpulkan byte menjadi potongan dan mengirimnya ke channel respons.
///
/// Setelah penerima tertutup atau setelah error, `aborted` menjadi true dan setiap penulisan gagal.
/// Dengan begitu `ZipWriter` yang di-drop saat error tidak bisa mengirim sisa arsip yang tampak lengkap.
struct ChunkSender {
    tx: mpsc::Sender<Result<Vec<u8>, io::Error>>,
    buf: Vec<u8>,
    aborted: Arc<AtomicBool>,
}

impl ChunkSender {
    fn aborted_error() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "unduhan dihentikan")
    }

    /// Mengirim isi `buf` sebagai satu potongan. Blocking bila channel penuh.
    fn send_pending(&mut self) -> io::Result<()> {
        if self.aborted.load(Ordering::Relaxed) {
            return Err(Self::aborted_error());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK_BYTES));
        self.tx.blocking_send(Ok(chunk)).map_err(|_| {
            self.aborted.store(true, Ordering::Relaxed);
            io::Error::new(io::ErrorKind::BrokenPipe, "klien memutus unduhan")
        })
    }
}

impl Write for ChunkSender {
    fn write(&mut self, mut data: &[u8]) -> io::Result<usize> {
        if self.aborted.load(Ordering::Relaxed) {
            return Err(Self::aborted_error());
        }
        let total = data.len();
        // `buf` selalu kurang dari CHUNK_BYTES di awal iterasi, jadi setiap potongan berukuran tetap.
        while !data.is_empty() {
            let take = (CHUNK_BYTES - self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buf.len() == CHUNK_BYTES {
                self.send_pending()?;
            }
        }
        Ok(total)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Membuka berkas untuk dibaca beserta ukurannya. `None` bila tidak terbaca atau bukan berkas biasa.
fn open_readable(path: &FsPath) -> Option<(std::fs::File, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    meta.is_file().then_some((file, meta.len()))
}

/// Menulis seluruh zip ke `tx`. Dipanggil di thread blocking.
fn stream_zip(
    handle: &tokio::runtime::Handle,
    tx: &mpsc::Sender<Result<Vec<u8>, io::Error>>,
    job: &ZipJob,
) -> io::Result<()> {
    let aborted = Arc::new(AtomicBool::new(false));
    let sink = ChunkSender {
        tx: tx.clone(),
        buf: Vec::with_capacity(CHUNK_BYTES),
        aborted: Arc::clone(&aborted),
    };
    let mut zip = ZipWriter::new_stream(sink);
    match write_entries(handle, tx, &mut zip, job) {
        Ok(()) => {
            let sink = zip.finish().map_err(|e| {
                aborted.store(true, Ordering::Relaxed);
                io::Error::from(e)
            })?;
            let mut sink = sink.into_inner();
            sink.send_pending()
        }
        Err(e) => {
            // Ditandai sebelum `zip` di-drop: Drop akan mencoba menutup arsip, dan itu tidak boleh terkirim.
            aborted.store(true, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Menulis setiap entri. Berkas dibaca dan ditulis per potongan, sehingga memori hanya memuat satu potongan.
fn write_entries(
    handle: &tokio::runtime::Handle,
    tx: &mpsc::Sender<Result<Vec<u8>, io::Error>>,
    zip: &mut ZipWriter<StreamWriter<ChunkSender>>,
    job: &ZipJob,
) -> io::Result<()> {
    let mut used: HashSet<String> = HashSet::new();

    for item in &job.items {
        // Klien sudah pergi: berhenti sebelum konversi ONLYOFFICE berikutnya.
        if tx.is_closed() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "klien memutus unduhan",
            ));
        }
        let original: PathBuf = media::media_dir(item.media.id).join(&item.media.file_name);
        let mut extension = item
            .media
            .file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default();

        let pdf = if job.want_pdf {
            handle.block_on(onlyoffice::media_pdf(
                &job.pool,
                &job.app_url,
                item.media.id,
                &item.media.file_name,
                &original,
            ))
        } else {
            None
        };
        let (mut source, size): (Box<dyn Read>, u64) = match pdf {
            Some(pdf) => {
                extension = "pdf".to_string();
                let len = pdf.len() as u64;
                (Box::new(Cursor::new(pdf)), len)
            }
            None => match open_readable(&original) {
                Some((file, len)) => (Box::new(file), len),
                // File tidak terbaca: dilewati, seperti `is_readable` di Laravel.
                None => continue,
            },
        };

        let label = {
            let s = sanitize(&item.jenis_dokumen, &job.unsafe_chars);
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

        // Berkas di atas 4 GiB memerlukan ZIP64. Opsi ini hanya dinyalakan untuk entri tersebut.
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(size > zip::ZIP64_BYTES_THR);
        // Gagal membuka entri (misalnya nama tidak valid) melewati entri ini, seperti sebelumnya.
        if zip.start_file(inner, options).is_err() {
            continue;
        }
        // Gagal membaca di tengah berkas tidak bisa dipulihkan karena entri sudah mulai ditulis.
        // Unduhan dihentikan dengan error, supaya klien tidak menerima arsip yang tampak lengkap.
        io::copy(&mut source, zip)?;
    }

    Ok(())
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

    fn sink(tx: mpsc::Sender<Result<Vec<u8>, io::Error>>) -> ChunkSender {
        ChunkSender {
            tx,
            buf: Vec::new(),
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn chunk_sender_sends_fixed_size_chunks_and_remainder() {
        let (tx, mut rx) = mpsc::channel(CHANNEL_CAPACITY);
        let mut out = sink(tx);
        let data: Vec<u8> = (0..CHUNK_BYTES * 2 + 10).map(|i| (i % 251) as u8).collect();
        out.write_all(&data).unwrap();
        out.send_pending().unwrap();

        let mut sizes = Vec::new();
        let mut joined = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            let chunk = chunk.unwrap();
            sizes.push(chunk.len());
            joined.extend_from_slice(&chunk);
        }
        assert_eq!(sizes, vec![CHUNK_BYTES, CHUNK_BYTES, 10]);
        assert_eq!(joined, data);
    }

    #[test]
    fn chunk_sender_fails_once_receiver_is_dropped() {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        drop(rx);
        let mut out = sink(tx);
        let err = out.write_all(&vec![1u8; CHUNK_BYTES]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        // Setelah gagal, penulisan berikutnya langsung gagal tanpa mencoba mengirim lagi.
        assert!(out.write(b"x").is_err());
        assert!(out.aborted.load(Ordering::Relaxed));
    }
}
