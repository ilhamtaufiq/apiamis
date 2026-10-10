//! Bagian tulis `BackupController`: buat backup lokal, batalkan job, restore, dan uji koneksi S3.
//!
//! Rute:
//! - `POST /api/app-settings/backups` (`store`): job dijalankan sebagai tugas Tokio di proses ini
//!   (Laravel memanggil `artisan backup:run`). Respons 202 dengan status awal.
//! - `DELETE /api/app-settings/backups/jobs/{jobId}` (`cancelJob`): menandai `cancel_requested`.
//!   Worker memeriksa tanda itu di setiap langkah, seperti Laravel. PID dikirim sinyal bila ada.
//! - `POST /api/app-settings/backups/restore` (`restore`): destruktif. Menjalankan ulang SQL dump
//!   ke database aktif dan menyalin media kembali.
//! - `POST /api/app-settings/backups/s3/test` (`testS3Connection`): `ListObjectsV2` berpenandatangan
//!   SigV4 dengan `reqwest`.
//!
//! Perbedaan dengan Laravel:
//! - Dump database ditulis lewat sqlx, tanpa `mysqldump`. Format berkas sama dengan Laravel
//!   (penanda `/*__ARUMANIS_STMT__*/`). Nilai yang bukan UTF-8 ditulis sebagai `X'...'`.
//! - `s3_direct` saat S3 aktif: ZIP dibuat di berkas sementara (Laravel memakai `php://temp`), lalu
//!   diunggah dengan `PutObject` di bawah 16 MiB, selain itu multipart (part = max(5 MiB, size/10000)).
//!   Bila unggah multipart gagal, multipart dibatalkan (Laravel tidak membatalkannya).
//! - Restore dari berkas yang hanya ada di S3: diunduh ke `system-backups` lalu dihapus setelah restore.
//!   Media di disk selain `local` dan `public` ditolak sebelum database diubah.
//! - Status job ditulis atomik (berkas sementara lalu rename). Job tidak punya PID sendiri, karena
//!   berjalan di proses API. Job yang dimulai Laravel tetap bisa dibatalkan dengan kirim sinyal.
//! - Validasi `store`/`s3/test` memakai `parse_body`: JSON atau `x-www-form-urlencoded`.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::{Multipart, Path as UrlPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use futures_util::TryStreamExt;
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySqlConnection, MySqlPool, Row};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    mailer, media, notifications::require_admin, notify::new_uuid, require_auth, AppState,
};

const BACKUP_DIR: &str = "system-backups";
const SQL_NAME: &str = "database.sql";
const SQL_MARKER: &str = "/*__ARUMANIS_STMT__*/\n";
const RESTORE_MAX_BYTES: u64 = 50 * 1024 * 1024;
/// Batas body route restore: 50 MB berkas ditambah overhead multipart.
pub const RESTORE_BODY_LIMIT: usize = 52 * 1024 * 1024;
const MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;
const INSERT_BATCH: usize = 100;

type Errs = BTreeMap<String, Vec<String>>;
type Status = Map<String, Value>;

// ---------------------------------------------------------------------------
// Galat dan utilitas umum
// ---------------------------------------------------------------------------

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan tidak valid: {e}"),
    )
}

fn unprocessable(msg: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, msg.into())
}

fn invalid(errors: Errs) -> ApiError {
    ApiError::validation("The given data was invalid.", errors)
}

fn add(errs: &mut Errs, key: &str, message: String) {
    errs.entry(key.to_string()).or_default().push(message);
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

fn obj(v: Value) -> Status {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// Akar disk `local` (`storage/app/private`). Dapat diganti dengan `PRIVATE_STORAGE_PATH`.
fn private_root() -> PathBuf {
    std::env::var_os("PRIVATE_STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/private"
            ))
        })
}

fn backup_dir() -> PathBuf {
    private_root().join(BACKUP_DIR)
}

fn jobs_dir() -> PathBuf {
    backup_dir().join("jobs")
}

fn job_path(job_id: &str) -> PathBuf {
    jobs_dir().join(format!("{job_id}.json"))
}

fn tmp_dir() -> PathBuf {
    private_root().join("tmp")
}

/// Akar disk untuk media. Disk lain belum didukung.
fn disk_root(disk: &str) -> Option<PathBuf> {
    match disk {
        "public" => Some(media::storage_root()),
        "local" => Some(private_root()),
        _ => None,
    }
}

/// `guardFilename`: `^[A-Za-z0-9._-]+\.zip$`.
fn guard_filename(name: &str) -> Result<(), ApiError> {
    let ok = name.len() > 4
        && name.ends_with(".zip")
        && name[..name.len() - 4]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(unprocessable("Nama backup tidak valid"))
    }
}

/// `guardJobId`: `^[A-Za-z0-9-]+$`.
fn guard_job_id(job_id: &str) -> Result<(), ApiError> {
    let ok = !job_id.is_empty()
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(unprocessable("ID backup tidak valid"))
    }
}

/// `empty()` PHP untuk string: kosong atau "0".
fn php_truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

async fn s3_backup_enabled(pool: &MySqlPool) -> Result<bool, ApiError> {
    Ok(mailer::setting(pool, "s3_backup_enabled")
        .await
        .map_err(internal)?
        .as_deref()
        == Some("1"))
}

/// Ruang bebas (byte) pada sistem berkas tempat `path` berada, lewat `df -Pk`.
fn free_bytes(path: &Path) -> Option<u64> {
    let out = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let avail_kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb * 1024)
}

/// Badan permintaan: JSON objek, atau `x-www-form-urlencoded`. Badan kosong berarti tanpa input.
fn parse_body(headers: &HeaderMap, body: &[u8]) -> Result<Map<String, Value>, ApiError> {
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Map::new());
    }
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ct.contains("x-www-form-urlencoded") {
        let mut map = Map::new();
        for pair in body.split(|b| *b == b'&') {
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, |b| *b == b'=');
            let key = percent_decode(parts.next().unwrap_or(&[]));
            let value = percent_decode(parts.next().unwrap_or(&[]));
            map.insert(key, Value::String(value));
        }
        return Ok(map);
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Ok(Map::new()),
        Err(e) => Err(bad_request(e)),
    }
}

fn percent_decode(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < raw.len() => {
                let hex = std::str::from_utf8(&raw[i + 1..i + 3]).unwrap_or("zz");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Aturan `nullable|string|max:n`. Null, kosong, dan tidak ada menghasilkan `None`.
fn str_field(input: &Map<String, Value>, key: &str, max: usize, errs: &mut Errs) -> Option<String> {
    match input.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                add(
                    errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                );
                None
            } else if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        Some(_) => {
            add(errs, key, format!("The {key} field must be a string."));
            None
        }
    }
}

/// `$request->boolean($key, $default)` dengan aturan `nullable|boolean`.
fn bool_input(input: &Map<String, Value>, key: &str, default: bool, errs: &mut Errs) -> bool {
    match input.get(key) {
        None => default,
        Some(Value::Null) => false,
        Some(Value::String(s)) if s.is_empty() => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) if n.as_i64() == Some(1) => true,
        Some(Value::Number(n)) if n.as_i64() == Some(0) => false,
        Some(Value::String(s)) if s == "1" => true,
        Some(Value::String(s)) if s == "0" => false,
        Some(_) => {
            add(errs, key, format!("The {key} field must be true or false."));
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Status job (berkas JSON di `system-backups/jobs`)
// ---------------------------------------------------------------------------

fn read_job(job_id: &str) -> Option<Status> {
    let bytes = std::fs::read(job_path(job_id)).ok()?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(m)) => Some(m),
        _ => None,
    }
}

/// Tulis atomik: berkas sementara lalu rename, agar pembaca tidak melihat JSON setengah jadi.
fn write_job(job_id: &str, status: &Status) -> std::io::Result<()> {
    std::fs::create_dir_all(jobs_dir())?;
    let tmp = jobs_dir().join(format!("{job_id}.{}.tmp", new_uuid()));
    let body = serde_json::to_vec_pretty(status).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, job_path(job_id))
}

fn save(job_id: &str, status: Status) {
    if let Err(e) = write_job(job_id, &status) {
        tracing::error!(job_id, error = %e, "gagal menulis status backup");
    }
}

fn is_cancel_requested(job_id: &str) -> bool {
    read_job(job_id)
        .and_then(|s| s.get("cancel_requested").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Galat worker: dibatalkan, atau gagal dengan pesan.
#[derive(Debug)]
enum Stop {
    Cancelled,
    Failed(String),
}

impl<E: std::fmt::Display> From<E> for Stop {
    fn from(e: E) -> Self {
        Stop::Failed(e.to_string())
    }
}

type JobResult<T> = Result<T, Stop>;

/// `patchJob`: gabungkan `patch` ke status, lalu gagal bila job sudah diminta batal.
fn patch_job(job_id: &str, patch: Value) -> JobResult<()> {
    if is_cancel_requested(job_id) {
        return Err(Stop::Cancelled);
    }
    let mut current = read_job(job_id).unwrap_or_default();
    for (k, v) in obj(patch) {
        current.insert(k, v);
    }
    current.insert("job_id".into(), json!(job_id));
    if !current.contains_key("status") {
        current.insert("status".into(), json!("running"));
    }
    write_job(job_id, &current)?;
    Ok(())
}

fn finalize_cancelled(
    job_id: &str,
    filename: &str,
    zip_path: &Path,
    include_media: bool,
    prev: &Status,
) {
    let _ = std::fs::remove_file(zip_path);
    save(
        job_id,
        obj(json!({
            "job_id": job_id,
            "status": "cancelled",
            "filename": filename,
            "include_media": include_media,
            "created_at": prev.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
            "started_at": prev.get("started_at").cloned().unwrap_or(Value::Null),
            "finished_at": now_iso(),
            "message": "Backup dibatalkan",
            "progress": 0,
            "cancel_requested": true,
        })),
    );
}

// ---------------------------------------------------------------------------
// Pembuatan backup (`store`)
// ---------------------------------------------------------------------------

/// Slug seperti `Str::slug`: huruf kecil ASCII dan angka, pemisah `-`.
fn slugify(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn build_backup_filename(label: Option<&str>) -> String {
    let mut parts = vec![
        "arumanis".to_string(),
        Utc::now().format("%Y%m%d_%H%M%S").to_string(),
    ];
    if let Some(label) = label.filter(|l| !l.trim().is_empty()) {
        let slug = slugify(label);
        if !slug.is_empty() {
            parts.push(slug);
        }
    }
    format!("{}.zip", parts.join("_"))
}

/// `POST /api/app-settings/backups`: admin. Respons 202 dengan status awal job.
pub async fn backup_store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let input = parse_body(&headers, &body)?;
    let mut errs = Errs::new();
    let label = str_field(&input, "label", 80, &mut errs);
    let include_media = bool_input(&input, "include_media", true, &mut errs);
    let s3_direct = bool_input(&input, "s3_direct", false, &mut errs);
    if !errs.is_empty() {
        return Err(invalid(errs));
    }

    let job_id = new_uuid();
    let filename = build_backup_filename(label.as_deref());
    let queued = obj(json!({
        "job_id": job_id,
        "status": "queued",
        "filename": filename,
        "include_media": include_media,
        "s3_direct": s3_direct,
        "created_at": now_iso(),
        "message": "Backup masuk antrean",
        "progress": 0,
    }));
    write_job(&job_id, &queued).map_err(internal)?;

    let pool = state.pool.clone();
    let app_url = state.app_url.clone();
    let worker_job = job_id.clone();
    let worker_file = filename.clone();
    tokio::spawn(async move {
        run_backup_job(
            pool,
            app_url,
            worker_job,
            worker_file,
            include_media,
            s3_direct,
        )
        .await;
    });

    let data = read_job(&job_id)
        .map(Value::Object)
        .unwrap_or(Value::Object(queued));
    let message = if s3_direct {
        "Backup sedang diproses langsung ke S3"
    } else {
        "Backup sedang diproses di server"
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "data": data, "message": message })),
    )
        .into_response())
}

async fn run_backup_job(
    pool: MySqlPool,
    app_url: String,
    job_id: String,
    filename: String,
    include_media: bool,
    s3_requested: bool,
) {
    let zip_path = backup_dir().join(&filename);
    let initial = read_job(&job_id).unwrap_or_default();
    if is_cancel_requested(&job_id) {
        finalize_cancelled(&job_id, &filename, &zip_path, include_media, &initial);
        return;
    }

    // `s3_direct && isS3BackupEnabled()`: konfigurasi S3 yang belum lengkap tetap memakai jalur S3
    // dan gagal di sana, seperti Laravel.
    let s3_direct = s3_requested && s3_backup_enabled(&pool).await.unwrap_or(false);
    save(
        &job_id,
        obj(json!({
            "job_id": job_id,
            "status": "running",
            "filename": filename,
            "include_media": include_media,
            "s3_direct": s3_direct,
            "created_at": initial.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
            "started_at": now_iso(),
            "message": "Menyiapkan dump database",
            "progress": 5,
        })),
    );

    let outcome = if s3_direct {
        create_archive_s3(&pool, &app_url, &job_id, &filename, include_media).await
    } else {
        create_archive(&pool, &app_url, &job_id, &filename, include_media).await
    };
    match outcome {
        Ok(result) => {
            let prev = read_job(&job_id).unwrap_or_default();
            save(
                &job_id,
                obj(json!({
                    "job_id": job_id,
                    "status": "completed",
                    "filename": filename,
                    "include_media": include_media,
                    "created_at": prev.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
                    "started_at": prev.get("started_at").cloned().unwrap_or(Value::Null),
                    "finished_at": now_iso(),
                    "message": "Backup berhasil dibuat",
                    "progress": 100,
                    "result": result,
                })),
            );
        }
        Err(Stop::Cancelled) => {
            let prev = read_job(&job_id).unwrap_or_default();
            finalize_cancelled(&job_id, &filename, &zip_path, include_media, &prev);
        }
        Err(Stop::Failed(msg)) => {
            if is_cancel_requested(&job_id) {
                let prev = read_job(&job_id).unwrap_or_default();
                finalize_cancelled(&job_id, &filename, &zip_path, include_media, &prev);
                return;
            }
            tracing::error!(job_id = %job_id, filename = %filename, error = %msg, "backup job gagal");
            let _ = std::fs::remove_file(&zip_path);
            let prev = read_job(&job_id).unwrap_or_default();
            save(
                &job_id,
                obj(json!({
                    "job_id": job_id,
                    "status": "failed",
                    "filename": filename,
                    "include_media": include_media,
                    "created_at": prev.get("created_at").cloned().unwrap_or_else(|| json!(now_iso())),
                    "started_at": prev.get("started_at").cloned().unwrap_or(Value::Null),
                    "finished_at": now_iso(),
                    "message": "Backup gagal dibuat",
                    "progress": 0,
                    "error": msg,
                })),
            );
        }
    }
}

/// `createBackupArchiveLocal`: dump database, lalu arsip ZIP berisi `database.sql` dan media.
async fn create_archive(
    pool: &MySqlPool,
    app_url: &str,
    job_id: &str,
    filename: &str,
    include_media: bool,
) -> JobResult<Value> {
    let dir = backup_dir();
    std::fs::create_dir_all(&dir)?;
    if let Some(free) = free_bytes(&dir) {
        if free < MIN_FREE_BYTES {
            return Err(Stop::Failed(
                "Ruang disk server kurang dari 1 GB. Bebaskan ruang sebelum backup besar.".into(),
            ));
        }
    }
    let zip_path = dir.join(filename);
    let sql_path = std::env::temp_dir().join(format!("arumanis_sql_{}.sql", new_uuid()));
    let outcome = archive_inner(
        pool,
        app_url,
        job_id,
        filename,
        include_media,
        &zip_path,
        &sql_path,
    )
    .await;
    let _ = std::fs::remove_file(&sql_path);
    if outcome.is_err() {
        let _ = std::fs::remove_file(&zip_path);
    }
    outcome
}

/// `createBackupArchiveS3`: dump, ZIP sementara di luar `system-backups`, lalu unggah ke S3.
/// Tidak ada salinan lokal di `system-backups`. Galat diberi awalan `Backup ke S3 gagal:` seperti Laravel.
async fn create_archive_s3(
    pool: &MySqlPool,
    app_url: &str,
    job_id: &str,
    filename: &str,
    include_media: bool,
) -> JobResult<Value> {
    let tmp = std::env::temp_dir();
    let sql_path = tmp.join(format!("arumanis_sql_{}.sql", new_uuid()));
    let zip_path = tmp.join(format!("arumanis_s3_{}.zip", new_uuid()));
    let outcome = s3_archive_inner(
        pool,
        app_url,
        job_id,
        filename,
        include_media,
        &zip_path,
        &sql_path,
    )
    .await;
    let _ = std::fs::remove_file(&sql_path);
    let _ = std::fs::remove_file(&zip_path);
    outcome.map_err(|stop| match stop {
        Stop::Failed(msg) => Stop::Failed(format!("Backup ke S3 gagal: {msg}")),
        Stop::Cancelled => Stop::Cancelled,
    })
}

async fn s3_archive_inner(
    pool: &MySqlPool,
    app_url: &str,
    job_id: &str,
    filename: &str,
    include_media: bool,
    zip_path: &Path,
    sql_path: &Path,
) -> JobResult<Value> {
    patch_job(
        job_id,
        json!({ "message": "Membuat dump database…", "progress": 10 }),
    )?;
    let db_name: Option<String> = sqlx::query_scalar("SELECT DATABASE()")
        .fetch_one(pool)
        .await?;
    let db_name = db_name.unwrap_or_default();
    dump_database(pool, &db_name, sql_path, job_id).await?;

    patch_job(
        job_id,
        json!({ "message": "Streaming arsip ZIP langsung ke S3…", "progress": 30 }),
    )?;
    let cfg = s3_config(pool).await?;
    let media_dirs = if include_media {
        Some(collect_media_dirs(pool).await?)
    } else {
        None
    };
    let (jid, zp, sp) = (
        job_id.to_string(),
        zip_path.to_path_buf(),
        sql_path.to_path_buf(),
    );
    let media_count =
        tokio::task::spawn_blocking(move || write_archive(&jid, &zp, &sp, media_dirs, &db_name))
            .await
            .map_err(|e| Stop::Failed(e.to_string()))??;

    patch_job(
        job_id,
        json!({ "message": "Mengunggah arsip ke S3 (multipart)…", "progress": 95 }),
    )?;
    let key = object_key(filename);
    s3_upload_file(&cfg, &key, zip_path, S3_MUP_THRESHOLD, S3_PART_MIN).await?;
    let size = s3_head_size(&cfg, &key)
        .await?
        .ok_or_else(|| "Objek backup tidak ditemukan di S3 setelah diunggah".to_string())?;

    Ok(json!({
        "filename": filename,
        "download_url": format!("{}/api/app-settings/backups/{filename}", app_url.trim_end_matches('/')),
        "size": size,
        "include_media": include_media,
        "media_files": media_count,
        "storage": "s3",
    }))
}

#[allow(clippy::too_many_arguments)]
async fn archive_inner(
    pool: &MySqlPool,
    app_url: &str,
    job_id: &str,
    filename: &str,
    include_media: bool,
    zip_path: &Path,
    sql_path: &Path,
) -> JobResult<Value> {
    patch_job(
        job_id,
        json!({ "message": "Membuat dump database…", "progress": 10 }),
    )?;
    let db_name: Option<String> = sqlx::query_scalar("SELECT DATABASE()")
        .fetch_one(pool)
        .await?;
    let db_name = db_name.unwrap_or_default();
    dump_database(pool, &db_name, sql_path, job_id).await?;

    patch_job(
        job_id,
        json!({ "message": "Membuka arsip ZIP…", "progress": 30 }),
    )?;
    let media_dirs = if include_media {
        Some(collect_media_dirs(pool).await?)
    } else {
        None
    };
    let (jid, zp, sp) = (
        job_id.to_string(),
        zip_path.to_path_buf(),
        sql_path.to_path_buf(),
    );
    let media_count =
        tokio::task::spawn_blocking(move || write_archive(&jid, &zp, &sp, media_dirs, &db_name))
            .await
            .map_err(|e| Stop::Failed(e.to_string()))??;

    let size = std::fs::metadata(zip_path).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        return Err(Stop::Failed(
            "Arsip backup kosong atau gagal ditulis".into(),
        ));
    }
    Ok(json!({
        "filename": filename,
        "download_url": format!("{}/api/app-settings/backups/{filename}", app_url.trim_end_matches('/')),
        "size": size,
        "include_media": include_media,
        "media_files": media_count,
    }))
}

/// Tulis `database.sql` ke `path`. Setiap tabel diawali `DROP` dan `CREATE`, lalu `INSERT` per 100 baris.
/// Tabel yang strukturnya ikut dibackup tapi datanya tidak (isinya sementara).
const DATA_SKIP_TABLES: &[&str] = &["cache"];

async fn dump_database(
    pool: &MySqlPool,
    db_name: &str,
    path: &Path,
    job_id: &str,
) -> JobResult<()> {
    let mut conn = pool.acquire().await?;
    let tables = sqlx::query("SHOW FULL TABLES WHERE Table_type = 'BASE TABLE'")
        .fetch_all(&mut *conn)
        .await?;

    let mut out = BufWriter::new(std::fs::File::create(path)?);
    write!(
        out,
        "-- Arumanis backup\n-- Database: {db_name}\nSET FOREIGN_KEY_CHECKS=0;\nSET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n{SQL_MARKER}"
    )?;

    for table_row in &tables {
        if is_cancel_requested(job_id) {
            return Err(Stop::Cancelled);
        }
        let table: Vec<u8> = table_row.try_get(0)?;
        let table = String::from_utf8_lossy(&table).into_owned();
        let esc = table.replace('`', "``");

        let create_row = sqlx::query(&format!("SHOW CREATE TABLE `{esc}`"))
            .fetch_one(&mut *conn)
            .await?;
        let create_sql: Vec<u8> = create_row.try_get(1)?;
        write!(out, "DROP TABLE IF EXISTS `{esc}`;\n{SQL_MARKER}")?;
        write!(
            out,
            "{};\n{SQL_MARKER}",
            String::from_utf8_lossy(&create_sql)
        )?;

        // Tabel sementara (state OAuth, kode handoff): struktur dibackup, isinya tidak. Mengurangi
        // beban dump yang membaca seluruh tabel ke memori.
        if DATA_SKIP_TABLES.contains(&table.as_str()) {
            continue;
        }

        let column_rows = sqlx::query(&format!("SHOW COLUMNS FROM `{esc}`"))
            .fetch_all(&mut *conn)
            .await?;
        let mut columns = Vec::with_capacity(column_rows.len());
        for r in &column_rows {
            let name: Vec<u8> = r.try_get(0)?;
            columns.push(String::from_utf8_lossy(&name).into_owned());
        }
        if columns.is_empty() {
            continue;
        }

        // CAST ke BINARY agar setiap nilai bisa dibaca sebagai bytes, termasuk tanggal dan angka.
        let select_list = columns
            .iter()
            .map(|c| format!("CAST(`{}` AS BINARY)", c.replace('`', "``")))
            .collect::<Vec<_>>()
            .join(", ");
        let column_list = columns
            .iter()
            .map(|c| format!("`{}`", c.replace('`', "``")))
            .collect::<Vec<_>>()
            .join(", ");
        let select = format!("SELECT {select_list} FROM `{esc}`");

        let mut batch: Vec<String> = Vec::with_capacity(INSERT_BATCH);
        for_each_row(&mut *conn, &select, |row| {
            let mut tuple = String::from("(");
            for i in 0..columns.len() {
                if i > 0 {
                    tuple.push_str(", ");
                }
                let value: Option<Vec<u8>> = row.try_get(i)?;
                push_sql_value(&mut tuple, value.as_deref());
            }
            tuple.push(')');
            batch.push(tuple);
            if batch.len() >= INSERT_BATCH {
                write_insert(&mut out, &esc, &column_list, &batch)?;
                batch.clear();
            }
            Ok(())
        })
        .await?;
        if !batch.is_empty() {
            write_insert(&mut out, &esc, &column_list, &batch)?;
        }
    }

    write!(out, "SET FOREIGN_KEY_CHECKS=1;\n")?;
    out.flush()?;
    Ok(())
}

/// Baca baris `select` satu per satu dan serahkan ke `on_row` (tanpa memuat seluruh tabel).
async fn for_each_row<F>(conn: &mut MySqlConnection, select: &str, mut on_row: F) -> JobResult<()>
where
    F: FnMut(&MySqlRow) -> JobResult<()>,
{
    let mut stream = sqlx::query(select).fetch(conn);
    while let Some(row) = stream.try_next().await? {
        on_row(&row)?;
    }
    Ok(())
}

fn write_insert(
    out: &mut impl Write,
    esc_table: &str,
    column_list: &str,
    rows: &[String],
) -> std::io::Result<()> {
    write!(
        out,
        "INSERT INTO `{esc_table}` ({column_list}) VALUES\n{};\n{SQL_MARKER}",
        rows.join(",\n")
    )
}

/// Literal SQL untuk satu nilai. Teks UTF-8 dikutip dengan escape MySQL. Selain itu `X'...'`.
fn push_sql_value(out: &mut String, value: Option<&[u8]>) {
    let Some(bytes) = value else {
        out.push_str("NULL");
        return;
    };
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            out.push('\'');
            for ch in text.chars() {
                match ch {
                    '\0' => out.push_str("\\0"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\x1a' => out.push_str("\\Z"),
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    '"' => out.push_str("\\\""),
                    c => out.push(c),
                }
            }
            out.push('\'');
        }
        Err(_) => {
            out.push_str("X'");
            for b in bytes {
                out.push_str(&format!("{b:02x}"));
            }
            out.push('\'');
        }
    }
}

/// Pasangan (disk, akar disk, id media) untuk setiap folder media `{id}/` yang perlu diarsipkan.
async fn collect_media_dirs(pool: &MySqlPool) -> JobResult<Vec<(String, PathBuf, u64)>> {
    let rows = sqlx::query("SELECT id, disk FROM media")
        .fetch_all(pool)
        .await?;
    let mut keys = BTreeSet::new();
    for row in rows {
        let id: u64 = row.try_get("id")?;
        let disk: Option<String> = row.try_get("disk")?;
        let disk = disk
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| "public".to_string());
        keys.insert((disk, id));
    }
    let mut out = Vec::with_capacity(keys.len());
    for (disk, id) in keys {
        let root = disk_root(&disk).ok_or_else(|| {
            Stop::Failed(format!(
                "Media di disk '{disk}' belum didukung di layanan Rust."
            ))
        })?;
        out.push((disk, root, id));
    }
    Ok(out)
}

/// Berkas di bawah `dir`, dengan path relatif `rel` (diawali nama folder media).
fn walk_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut entries = read.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let child = format!("{rel}/{name}");
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_files(&path, &child, out)?;
        } else if meta.is_file() {
            out.push((child, path));
        }
    }
    Ok(())
}

/// Tulis arsip ZIP. Dipanggil di `spawn_blocking` karena `zip` bersifat sinkron.
fn write_archive(
    job_id: &str,
    zip_path: &Path,
    sql_path: &Path,
    media_dirs: Option<Vec<(String, PathBuf, u64)>>,
    db_name: &str,
) -> JobResult<u64> {
    let include_media = media_dirs.is_some();
    let mut zip = ZipWriter::new(std::fs::File::create(zip_path)?);
    zip.start_file(
        SQL_NAME,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    std::io::copy(&mut std::fs::File::open(sql_path)?, &mut zip)?;

    let mut media_count: u64 = 0;
    if let Some(dirs) = media_dirs {
        patch_job(
            job_id,
            json!({ "message": "Mengemas file media (bisa memakan waktu untuk arsip multi-GB)…", "progress": 40 }),
        )?;
        let mut files = Vec::new();
        for (disk, root, id) in &dirs {
            let rel = id.to_string();
            let mut found = Vec::new();
            walk_files(&root.join(&rel), &rel, &mut found)?;
            files.extend(
                found
                    .into_iter()
                    .map(|(r, abs)| (format!("media/{disk}/{r}"), abs)),
            );
        }
        let total = files.len() as u64;
        let mut last_report = 0u64;
        for (name, abs) in files {
            let Ok(mut src) = std::fs::File::open(&abs) else {
                tracing::warn!(file = %abs.display(), "backup: gagal membuka file media");
                continue;
            };
            zip.start_file(
                &name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )?;
            std::io::copy(&mut src, &mut zip)?;
            media_count += 1;
            if media_count - last_report >= 25 || media_count == total {
                let pct = if total > 0 {
                    40 + media_count * 50 / total
                } else {
                    70
                };
                patch_job(
                    job_id,
                    json!({ "message": format!("Mengemas media {media_count}/{total}"), "progress": pct.min(90) }),
                )?;
                last_report = media_count;
            }
        }
    }

    patch_job(
        job_id,
        json!({ "message": "Menutup arsip ZIP…", "progress": 95 }),
    )?;
    let comment = json!({
        "created_at": now_iso(),
        "database": db_name,
        "include_media": include_media,
        "media_files": media_count,
    })
    .to_string();
    zip.set_raw_comment(comment.into_bytes().into_boxed_slice())?;
    zip.finish()?;
    Ok(media_count)
}

// ---------------------------------------------------------------------------
// Batalkan job (`cancelJob`)
// ---------------------------------------------------------------------------

/// `DELETE /api/app-settings/backups/jobs/{jobId}`: admin.
pub async fn backup_cancel_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(job_id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    guard_job_id(&job_id)?;

    let status = read_job(&job_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Status backup tidak ditemukan"))?;
    let current_state = status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let terminal = matches!(current_state.as_str(), "completed" | "failed" | "cancelled");

    let current = if terminal {
        status
    } else {
        let mut patched = status.clone();
        patched.insert("cancel_requested".into(), json!(true));
        patched.insert("cancel_requested_at".into(), json!(now_iso()));
        patched.insert("message".into(), json!("Membatalkan backup…"));
        write_job(&job_id, &patched).map_err(internal)?;

        let pid = status.get("pid").and_then(Value::as_i64);
        terminate_process(pid).await;

        read_job(&job_id).unwrap_or(patched)
    };

    let message = if current.get("status").and_then(Value::as_str) == Some("cancelled") {
        "Backup dibatalkan"
    } else {
        "Permintaan pembatalan backup dikirim"
    };
    Ok(Json(json!({ "data": current, "message": message })))
}

/// `terminateProcessPid`: SIGTERM, lalu SIGKILL bila proses masih hidup setelah 300 ms.
async fn terminate_process(pid: Option<i64>) {
    let Some(pid) = pid.filter(|p| *p > 0) else {
        return;
    };
    let signal = |sig: &str| {
        Command::new("kill")
            .arg(sig)
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    signal("-TERM");
    tokio::time::sleep(Duration::from_millis(300)).await;
    if signal("-0") {
        signal("-KILL");
    }
}

// ---------------------------------------------------------------------------
// Restore (destruktif)
// ---------------------------------------------------------------------------

/// `POST /api/app-settings/backups/restore`: admin. Mengganti isi database dan media.
pub async fn backup_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let upload_path = tmp_dir().join(format!("restore_{}.zip", new_uuid()));
    let mut stored = false;
    let result = restore_request(&state, &mut multipart, &upload_path, &mut stored).await;
    if stored {
        let _ = tokio::fs::remove_file(&upload_path).await;
    }
    result
}

async fn restore_request(
    state: &AppState,
    multipart: &mut Multipart,
    upload_path: &Path,
    stored: &mut bool,
) -> Result<Json<Value>, ApiError> {
    let mut backup_name: Option<String> = None;
    let mut has_file = false;
    let mut errs = Errs::new();
    let mut too_large = false;

    while let Some(mut field) = multipart.next_field().await.map_err(bad_request)? {
        match field.name().unwrap_or("") {
            "backup_name" => {
                backup_name = Some(field.text().await.map_err(bad_request)?);
            }
            "backup_file" => {
                if field.file_name().is_none() {
                    add(
                        &mut errs,
                        "backup_file",
                        "The backup file field must be a file.".into(),
                    );
                    continue;
                }
                has_file = true;
                if let Some(parent) = upload_path.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(internal)?;
                }
                let mut out = tokio::fs::File::create(upload_path)
                    .await
                    .map_err(internal)?;
                *stored = true;
                let mut size: u64 = 0;
                let mut head: Vec<u8> = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(bad_request)? {
                    size += chunk.len() as u64;
                    if size > RESTORE_MAX_BYTES {
                        too_large = true;
                        break;
                    }
                    if head.len() < 4 {
                        let take = (4 - head.len()).min(chunk.len());
                        head.extend_from_slice(&chunk[..take]);
                    }
                    out.write_all(&chunk).await.map_err(internal)?;
                }
                out.flush().await.map_err(internal)?;
                if too_large {
                    break;
                }
                let zip_magic = head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06");
                if !zip_magic {
                    add(
                        &mut errs,
                        "backup_file",
                        "The backup file field must be a file of type: zip.".into(),
                    );
                }
            }
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    if too_large {
        return Err(unprocessable(
            "Upload restore dibatasi 50 MB. Untuk backup besar, unggah lewat server atau pilih backup yang sudah tersimpan di daftar.",
        ));
    }
    if !errs.is_empty() {
        return Err(invalid(errs));
    }
    let name = backup_name.filter(|n| php_truthy(n));
    if !has_file && name.is_none() {
        return Err(unprocessable(
            "Pilih file backup atau nama backup yang tersimpan",
        ));
    }

    // Salinan lokal sementara bila backup hanya ada di S3 (dihapus setelah restore, seperti Laravel).
    let mut downloaded: Option<PathBuf> = None;
    let source = if has_file {
        upload_path.to_path_buf()
    } else {
        let filename = name.unwrap_or_default();
        guard_filename(&filename)?;
        let local = backup_dir().join(&filename);
        if local.is_file() {
            local
        } else {
            // `getBackupDisk`: S3 dipakai bila aktif dan objek ada. Galat konfigurasi atau HEAD
            // dianggap tidak ada, sehingga jawabannya 404.
            let cfg = if s3_backup_enabled(&state.pool).await? {
                s3_config(&state.pool).await.ok()
            } else {
                None
            };
            let key = object_key(&filename);
            let found = match &cfg {
                Some(c) => match s3_head_size(c, &key).await {
                    Ok(found) => found.is_some(),
                    Err(e) => {
                        tracing::warn!(filename = %filename, error = %e, "gagal memeriksa backup di S3");
                        false
                    }
                },
                None => false,
            };
            match cfg {
                Some(c) if found => {
                    tokio::fs::create_dir_all(backup_dir())
                        .await
                        .map_err(internal)?;
                    if s3_download(&c, &key, &local).await.is_err() {
                        let _ = tokio::fs::remove_file(&local).await;
                        return Err(ApiError::new(
                            StatusCode::BAD_GATEWAY,
                            "Gagal mengunduh backup dari S3",
                        ));
                    }
                    downloaded = Some(local.clone());
                    local
                }
                _ => {
                    return Err(ApiError::new(
                        StatusCode::NOT_FOUND,
                        "Backup tidak ditemukan",
                    ));
                }
            }
        }
    };

    let result = restore_from_zip(&state.pool, &source).await;
    if let Some(copy) = downloaded {
        let _ = tokio::fs::remove_file(&copy).await;
    }
    let data = result?;
    Ok(Json(
        json!({ "data": data, "message": "Restore backup berhasil dijalankan" }),
    ))
}

/// `restoreArchive`: cek ruang disk, ekstrak `database.sql`, jalankan SQL, lalu salin media.
/// Fungsi ini mengubah database dan berkas media. Jangan dipanggil dengan database produksi dalam tes.
pub async fn restore_from_zip(pool: &MySqlPool, zip_path: &Path) -> Result<Value, ApiError> {
    let size = tokio::fs::metadata(zip_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let tmp = tmp_dir();
    tokio::fs::create_dir_all(&tmp).await.map_err(internal)?;
    if size > 0 {
        if let Some(free) = free_bytes(&tmp) {
            if free < size * 3 {
                let gb = |b: u64| format!("{:.1}", b as f64 / 1_073_741_824.0);
                return Err(internal(format!(
                    "Ruang disk tidak cukup untuk restore. Butuh sekitar {} GB bebas; tersedia {} GB.",
                    gb(size * 3),
                    gb(free)
                )));
            }
        }
    }

    let extract_dir = tmp.join(format!("restore_{}", new_uuid()));
    let (zip_owned, extract_owned) = (zip_path.to_path_buf(), extract_dir.clone());
    let prepared = tokio::task::spawn_blocking(move || prepare_restore(&zip_owned, &extract_owned))
        .await
        .map_err(internal)?;
    let outcome = match prepared {
        Ok(media) => restore_steps(pool, zip_path, &extract_dir, media).await,
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_dir_all(&extract_dir).await;
    outcome?;

    Ok(json!({
        "restored_at": now_iso(),
        "source": zip_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    }))
}

/// Entri media di ZIP: indeks entri dan path tujuan.
type MediaEntry = (usize, PathBuf);

/// Periksa isi ZIP sebelum database diubah: ada `database.sql`, disk media didukung, path aman.
fn prepare_restore(zip_path: &Path, extract_dir: &Path) -> Result<Vec<MediaEntry>, ApiError> {
    std::fs::create_dir_all(extract_dir).map_err(internal)?;
    let file = std::fs::File::open(zip_path).map_err(|_| internal("Gagal membuka file backup"))?;
    let mut zip = ZipArchive::new(file).map_err(|_| internal("Gagal membuka file backup"))?;

    let mut has_sql = false;
    let mut media = Vec::new();
    for i in 0..zip.len() {
        let Some(name) = zip.name_for_index(i).map(str::to_string) else {
            continue;
        };
        if name == SQL_NAME {
            has_sql = true;
            continue;
        }
        let Some(rest) = name.strip_prefix("media/") else {
            continue;
        };
        if name.ends_with('/') {
            continue;
        }
        let Some((disk, rel)) = rest.split_once('/') else {
            continue;
        };
        let root = disk_root(disk).ok_or_else(|| {
            unprocessable(format!(
                "Disk media '{disk}' belum didukung di layanan Rust."
            ))
        })?;
        let safe = !rel.is_empty()
            && Path::new(rel)
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !safe {
            return Err(unprocessable("Path media di dalam backup tidak valid."));
        }
        media.push((i, root.join(rel)));
    }
    if !has_sql {
        return Err(unprocessable("Database dump tidak ditemukan di backup"));
    }

    let mut entry = zip.by_name(SQL_NAME).map_err(internal)?;
    let mut out = std::fs::File::create(extract_dir.join(SQL_NAME)).map_err(internal)?;
    std::io::copy(&mut entry, &mut out).map_err(internal)?;
    Ok(media)
}

async fn restore_steps(
    pool: &MySqlPool,
    zip_path: &Path,
    extract_dir: &Path,
    media: Vec<MediaEntry>,
) -> Result<(), ApiError> {
    let mut conn = pool.acquire().await.map_err(internal)?;
    sqlx::query("SET FOREIGN_KEY_CHECKS=0")
        .execute(&mut *conn)
        .await
        .map_err(internal)?;
    let sql = run_sql(&mut conn, &extract_dir.join(SQL_NAME)).await;
    let _ = sqlx::query("SET FOREIGN_KEY_CHECKS=1")
        .execute(&mut *conn)
        .await;
    sql?;

    let zp = zip_path.to_path_buf();
    tokio::task::spawn_blocking(move || restore_media(&zp, &media))
        .await
        .map_err(internal)??;
    Ok(())
}

/// Jalankan setiap pernyataan dari dump, dipisah penanda `SQL_MARKER`.
async fn run_sql(
    conn: &mut sqlx::pool::PoolConnection<sqlx::MySql>,
    path: &Path,
) -> Result<(), ApiError> {
    let mut file = tokio::fs::File::open(path).await.map_err(internal)?;
    let marker = SQL_MARKER.as_bytes();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut chunk).await.map_err(internal)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        while let Some(pos) = buf.windows(marker.len()).position(|w| w == marker) {
            let statement = buf[..pos].to_vec();
            buf.drain(..pos + marker.len());
            exec_statement(conn, &statement).await?;
        }
    }
    exec_statement(conn, &buf).await
}

async fn exec_statement(
    conn: &mut sqlx::pool::PoolConnection<sqlx::MySql>,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| internal("Dump database tidak berupa UTF-8"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.starts_with("--") {
        return Ok(());
    }
    let sql = trimmed.trim_end_matches([';', '\r', '\n', '\t', ' ']);
    sqlx::query(sql)
        .execute(&mut **conn)
        .await
        .map_err(internal)?;
    Ok(())
}

/// Salin `media/{disk}/{rel}` dari ZIP ke akar disk. Dipanggil di `spawn_blocking`.
fn restore_media(zip_path: &Path, entries: &[MediaEntry]) -> Result<(), ApiError> {
    let file = std::fs::File::open(zip_path).map_err(internal)?;
    let mut zip = ZipArchive::new(file).map_err(internal)?;
    for (index, target) in entries {
        let mut entry = zip.by_index(*index).map_err(internal)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(internal)?;
        }
        let mut out = std::fs::File::create(target).map_err(internal)?;
        std::io::copy(&mut entry, &mut out).map_err(internal)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Uji koneksi S3 (`testS3Connection`)
// ---------------------------------------------------------------------------

/// Konfigurasi S3 (`getS3Disk`). `endpoint` kosong berarti AWS dengan virtual-host.
#[derive(Debug, Clone)]
pub struct S3Cfg {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub key: String,
    pub secret: String,
}

/// Ambang unggah multipart `ObjectUploader` (`mup_threshold`, AWS SDK PHP): 16 MiB.
pub const S3_MUP_THRESHOLD: u64 = 16 * 1024 * 1024;
/// Ukuran part minimum `MultipartUploader::PART_MIN_SIZE`.
pub const S3_PART_MIN: u64 = 5 * 1024 * 1024;
/// Jumlah part maksimum `MultipartUploader::PART_MAX_NUM`.
const S3_PART_MAX_NUM: u64 = 10_000;
/// Batas waktu satu permintaan S3 (satu part bisa besar).
const S3_REQUEST_TIMEOUT: Duration = Duration::from_secs(900);

/// Kunci objek seperti Laravel: `system-backups/{filename}`.
pub fn object_key(filename: &str) -> String {
    format!("{BACKUP_DIR}/{filename}")
}

/// `getS3Disk`: konfigurasi lengkap dari `app_settings`. Nilai kosong atau "0" dianggap tidak ada.
pub async fn s3_config(pool: &MySqlPool) -> Result<S3Cfg, String> {
    let mut values: BTreeMap<&str, Option<String>> = BTreeMap::new();
    for key in [
        "s3_endpoint",
        "s3_region",
        "s3_bucket",
        "s3_access_key_id",
        "s3_secret_access_key",
    ] {
        let value = mailer::setting(pool, key)
            .await
            .map_err(|e| e.to_string())?;
        values.insert(key, value.filter(|s| php_truthy(s)));
    }
    let get = |k: &str| values.get(k).cloned().flatten();
    match (
        get("s3_region"),
        get("s3_bucket"),
        get("s3_access_key_id"),
        get("s3_secret_access_key"),
    ) {
        (Some(region), Some(bucket), Some(key), Some(secret)) => Ok(S3Cfg {
            endpoint: get("s3_endpoint"),
            region,
            bucket,
            key,
            secret,
        }),
        _ => Err("Pengaturan AWS S3 belum lengkap atau belum dikonfigurasi.".into()),
    }
}

/// Encoding URI SigV4: huruf, angka, `-._~` apa adanya. `/` dibiarkan bila `encode_slash` false.
fn aws_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let plain = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'.' | b'_' | b'~')
            || (b == b'/' && !encode_slash);
        if plain {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Dasar URL dan awalan path kanonik. Dengan endpoint: path-style (`use_path_style_endpoint`).
fn s3_base(cfg: &S3Cfg) -> (String, String) {
    match cfg.endpoint.as_deref() {
        Some(ep) => (
            ep.trim_end_matches('/').to_string(),
            format!("/{}", cfg.bucket),
        ),
        None => (
            format!("https://{}.s3.{}.amazonaws.com", cfg.bucket, cfg.region),
            String::new(),
        ),
    }
}

/// Satu permintaan S3 berpenandatangan SigV4. `query` boleh tidak terurut; `extra` adalah header
/// tambahan yang ikut ditandatangani (mis. `x-amz-acl`).
async fn s3_send(
    cfg: &S3Cfg,
    method: reqwest::Method,
    object: Option<&str>,
    query: &[(&str, &str)],
    extra: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> Result<reqwest::Response, String> {
    let (base, prefix) = s3_base(cfg);
    let path = match object {
        Some(o) => format!("/{o}"),
        None => "/".to_string(),
    };
    let canonical_uri = format!("{prefix}{}", aws_encode(&path, false));
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (aws_encode(k, true), aws_encode(v, true)))
        .collect();
    pairs.sort();
    let canonical_query = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let url_text = if canonical_query.is_empty() {
        format!("{base}{canonical_uri}")
    } else {
        format!("{base}{canonical_uri}?{canonical_query}")
    };
    let url = reqwest::Url::parse(&url_text).map_err(|e| e.to_string())?;
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => return Err("URL S3 tidak memiliki host".into()),
    };

    let payload_hash = sha256_hex(body.as_deref().unwrap_or(&[]));
    let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let authorization = sigv4_sign(
        cfg,
        method.as_str(),
        &canonical_uri,
        &canonical_query,
        &host,
        &amz_date,
        &payload_hash,
        extra,
    );
    let client = reqwest::Client::builder()
        .timeout(S3_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .request(method, url)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-date", amz_date)
        .header("authorization", authorization);
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    if let Some(bytes) = body {
        req = req.body(bytes);
    }
    req.send().await.map_err(|e| e.to_string())
}

/// Galat S3 dalam bentuk `HTTP {status}: {Code}: {Message}`.
async fn s3_fail(res: reqwest::Response) -> String {
    let status = res.status().as_u16();
    let body = res.text().await.unwrap_or_default();
    match (xml_tag(&body, "Code"), xml_tag(&body, "Message")) {
        (Some(code), Some(msg)) => format!("HTTP {status}: {code}: {msg}"),
        (Some(code), None) => format!("HTTP {status}: {code}"),
        _ => format!("HTTP {status}"),
    }
}

/// Unggah `path` ke `key`: `PutObject` bila ukuran di bawah `threshold`, selain itu multipart.
/// Ukuran part = max(`part_min`, ceil(ukuran / 10000)), seperti `MultipartUploader::determinePartSize`.
/// Multipart yang gagal dibatalkan.
pub async fn s3_upload_file(
    cfg: &S3Cfg,
    key: &str,
    path: &Path,
    threshold: u64,
    part_min: u64,
) -> Result<(), String> {
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|e| e.to_string())?
        .len();
    let acl = [("x-amz-acl", "private")];
    if size < threshold {
        let body = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
        let res = s3_send(cfg, reqwest::Method::PUT, Some(key), &[], &acl, Some(body)).await?;
        return if res.status().is_success() {
            Ok(())
        } else {
            Err(s3_fail(res).await)
        };
    }

    let part_size = part_min.max(size.div_ceil(S3_PART_MAX_NUM));
    let res = s3_send(
        cfg,
        reqwest::Method::POST,
        Some(key),
        &[("uploads", "")],
        &acl,
        None,
    )
    .await?;
    if !res.status().is_success() {
        return Err(s3_fail(res).await);
    }
    let text = res.text().await.map_err(|e| e.to_string())?;
    let upload_id = xml_tag(&text, "UploadId")
        .ok_or_else(|| "UploadId tidak ada pada respons S3".to_string())?;

    let parts = s3_upload_parts(cfg, key, &upload_id, path, part_size).await;
    let parts = match parts {
        Ok(parts) => parts,
        Err(e) => {
            abort_multipart(cfg, key, &upload_id).await;
            return Err(e);
        }
    };

    let mut xml = String::from("<CompleteMultipartUpload>");
    for (number, etag) in &parts {
        xml.push_str(&format!(
            "<Part><PartNumber>{number}</PartNumber><ETag>{etag}</ETag></Part>"
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");
    let res = s3_send(
        cfg,
        reqwest::Method::POST,
        Some(key),
        &[("uploadId", upload_id.as_str())],
        &[],
        Some(xml.into_bytes()),
    )
    .await;
    let outcome = match res {
        Ok(res) if res.status().is_success() => {
            let body = res.text().await.unwrap_or_default();
            if body.contains("<Error>") {
                Err(match (xml_tag(&body, "Code"), xml_tag(&body, "Message")) {
                    (Some(code), Some(msg)) => format!("{code}: {msg}"),
                    _ => "CompleteMultipartUpload gagal".to_string(),
                })
            } else {
                Ok(())
            }
        }
        Ok(res) => Err(s3_fail(res).await),
        Err(e) => Err(e),
    };
    if outcome.is_err() {
        abort_multipart(cfg, key, &upload_id).await;
    }
    outcome
}

/// Unggah part berurutan. Mengembalikan `(nomor part, ETag)`.
async fn s3_upload_parts(
    cfg: &S3Cfg,
    key: &str,
    upload_id: &str,
    path: &Path,
    part_size: u64,
) -> Result<Vec<(u32, String)>, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut parts = Vec::new();
    let mut number: u32 = 1;
    loop {
        let mut buf = vec![0u8; part_size as usize];
        let filled = read_full(&mut file, &mut buf).await?;
        if filled == 0 {
            break;
        }
        buf.truncate(filled);
        let number_text = number.to_string();
        let res = s3_send(
            cfg,
            reqwest::Method::PUT,
            Some(key),
            &[
                ("partNumber", number_text.as_str()),
                ("uploadId", upload_id),
            ],
            &[],
            Some(buf),
        )
        .await?;
        if !res.status().is_success() {
            return Err(s3_fail(res).await);
        }
        let etag = res
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        parts.push((number, etag));
        number += 1;
    }
    Ok(parts)
}

/// Isi `buf` sampai penuh atau sampai akhir berkas. Mengembalikan jumlah byte yang terisi.
async fn read_full(file: &mut tokio::fs::File, buf: &mut [u8]) -> Result<usize, String> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file
            .read(&mut buf[filled..])
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

async fn abort_multipart(cfg: &S3Cfg, key: &str, upload_id: &str) {
    if let Err(e) = s3_send(
        cfg,
        reqwest::Method::DELETE,
        Some(key),
        &[("uploadId", upload_id)],
        &[],
        None,
    )
    .await
    {
        tracing::warn!(key, error = %e, "gagal membatalkan multipart upload S3");
    }
}

/// `HeadObject`: ukuran objek, atau `None` bila tidak ada (404).
pub async fn s3_head_size(cfg: &S3Cfg, key: &str) -> Result<Option<u64>, String> {
    let res = s3_send(cfg, reqwest::Method::HEAD, Some(key), &[], &[], None).await?;
    let status = res.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let size = res
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| "Content-Length tidak ada pada respons S3".to_string())?;
    Ok(Some(size))
}

/// `GetObject` ke berkas `dest`. Galat HTTP menjadi `Err`; pemanggil membuang `dest` yang setengah jadi.
pub async fn s3_download(cfg: &S3Cfg, key: &str, dest: &Path) -> Result<(), String> {
    let mut res = s3_send(cfg, reqwest::Method::GET, Some(key), &[], &[], None).await?;
    if !res.status().is_success() {
        return Err(s3_fail(res).await);
    }
    let mut out = tokio::fs::File::create(dest)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        out.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    out.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// `DeleteObject`. S3 menjawab sukses juga bila objek memang tidak ada.
pub async fn s3_delete(cfg: &S3Cfg, key: &str) -> Result<(), String> {
    let res = s3_send(cfg, reqwest::Method::DELETE, Some(key), &[], &[], None).await?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(s3_fail(res).await)
    }
}

/// `deleteBackup` untuk S3: galat konfigurasi dan S3 hanya dicatat, seperti Laravel.
pub async fn s3_delete_backup(pool: &MySqlPool, filename: &str) {
    match s3_config(pool).await {
        Ok(cfg) => {
            if let Err(e) = s3_delete(&cfg, &object_key(filename)).await {
                tracing::warn!(filename, error = %e, "gagal menghapus backup dari S3");
            }
        }
        Err(e) => tracing::warn!(filename, error = %e, "gagal menghapus backup dari S3"),
    }
}

/// `input ?: stored` seperti PHP: nilai kosong atau "0" dianggap tidak ada.
fn pick(input: Option<String>, stored: Option<String>) -> Option<String> {
    input
        .filter(|s| php_truthy(s))
        .or_else(|| stored.filter(|s| php_truthy(s)))
}

/// `POST /api/app-settings/backups/s3/test`: admin. Hanya `ListObjectsV2` pada akar bucket.
pub async fn backup_test_s3(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let input = parse_body(&headers, &body)?;
    let mut errs = Errs::new();
    let endpoint_in = str_field(&input, "s3_endpoint", 255, &mut errs);
    let region_in = str_field(&input, "s3_region", 64, &mut errs);
    let bucket_in = str_field(&input, "s3_bucket", 64, &mut errs);
    let key_in = str_field(&input, "s3_access_key_id", 128, &mut errs);
    let secret_in = str_field(&input, "s3_secret_access_key", 255, &mut errs);
    if !errs.is_empty() {
        return Err(invalid(errs));
    }

    let pool = &state.pool;
    let stored = |k: &'static str| async move { mailer::setting(pool, k).await.map_err(internal) };
    let endpoint = pick(endpoint_in, stored("s3_endpoint").await?);
    let region = pick(region_in, stored("s3_region").await?);
    let bucket = pick(bucket_in, stored("s3_bucket").await?);
    let key = pick(key_in, stored("s3_access_key_id").await?);
    let secret = pick(secret_in.clone(), stored("s3_secret_access_key").await?);
    let used_stored_key = secret_in.map_or(true, |s| s.trim().is_empty());

    let (Some(region), Some(bucket), Some(key), Some(secret)) = (region, bucket, key, secret)
    else {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Region, Bucket, Access Key, dan Secret Key wajib diisi untuk uji koneksi.",
                "used_stored_key": used_stored_key,
            })),
        )
            .into_response());
    };

    let cfg = S3Cfg {
        endpoint,
        region,
        bucket,
        key,
        secret,
    };
    match s3_list_root(&cfg).await {
        Ok(()) => {
            Ok(Json(json!({ "ok": true, "used_stored_key": used_stored_key })).into_response())
        }
        Err(msg) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "ok": false,
                "error": format!("Koneksi S3 gagal: {msg}"),
                "used_stored_key": used_stored_key,
            })),
        )
            .into_response()),
    }
}

/// `ListObjectsV2` dengan prefix kosong dan delimiter `/`, seperti `files('', false)` di Laravel.
async fn s3_list_root(cfg: &S3Cfg) -> Result<(), String> {
    let query = "delimiter=%2F&list-type=2&prefix=";
    // Dengan endpoint: path-style (seperti `use_path_style_endpoint`). Tanpa endpoint: virtual-host.
    let (base, canonical_uri) = match cfg.endpoint.as_deref() {
        Some(ep) => (
            format!("{}/{}/", ep.trim_end_matches('/'), cfg.bucket),
            format!("/{}/", cfg.bucket),
        ),
        None => (
            format!("https://{}.s3.{}.amazonaws.com/", cfg.bucket, cfg.region),
            "/".to_string(),
        ),
    };
    let url = reqwest::Url::parse(&format!("{base}?{query}")).map_err(|e| e.to_string())?;
    let host_name = url.host_str().unwrap_or_default().to_string();
    let host = match url.port() {
        Some(p) => format!("{host_name}:{p}"),
        None => host_name,
    };

    let amz_date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let authorization = sigv4_authorization(cfg, &host, &canonical_uri, query, &amz_date);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(url)
        .header("x-amz-content-sha256", sha256_hex(b""))
        .header("x-amz-date", amz_date)
        .header("authorization", authorization)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = res.status();
    if status.is_success() {
        return Ok(());
    }
    let body = res.text().await.unwrap_or_default();
    Err(match (xml_tag(&body, "Code"), xml_tag(&body, "Message")) {
        (Some(code), Some(msg)) => format!("HTTP {}: {code}: {msg}", status.as_u16()),
        (Some(code), None) => format!("HTTP {}: {code}", status.as_u16()),
        _ => format!("HTTP {}", status.as_u16()),
    })
}

fn xml_tag(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = body.find(&open)? + open.len();
    let end = body[start..].find(&close)? + start;
    Some(body[start..end].to_string())
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC menerima kunci dengan panjang apa pun");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Kunci penandatanganan SigV4: `AWS4`+secret → tanggal → region → `s3` → `aws4_request`.
fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = hmac_sha256(&k, region.as_bytes());
    let k = hmac_sha256(&k, b"s3");
    hmac_sha256(&k, b"aws4_request")
}

/// Nilai header `Authorization` SigV4 untuk `GET` tanpa badan.
fn sigv4_authorization(
    cfg: &S3Cfg,
    host: &str,
    canonical_uri: &str,
    canonical_query: &str,
    amz_date: &str,
) -> String {
    sigv4_sign(
        cfg,
        "GET",
        canonical_uri,
        canonical_query,
        host,
        amz_date,
        &sha256_hex(b""),
        &[],
    )
}

/// Nilai `Authorization` SigV4. Header yang ditandatangani: `host`, `x-amz-content-sha256`,
/// `x-amz-date`, dan `extra` (nama huruf kecil), diurutkan menurut nama.
#[allow(clippy::too_many_arguments)]
fn sigv4_sign(
    cfg: &S3Cfg,
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    host: &str,
    amz_date: &str,
    payload_hash: &str,
    extra: &[(&str, &str)],
) -> String {
    let date = &amz_date[..8];
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), host.into()),
        ("x-amz-content-sha256".into(), payload_hash.into()),
        ("x-amz-date".into(), amz_date.into()),
    ];
    for (k, v) in extra {
        headers.push((k.to_ascii_lowercase(), v.trim().to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date}/{}/s3/aws4_request", cfg.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&cfg.secret, date, &cfg.region),
        string_to_sign.as_bytes(),
    ));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        cfg.key
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_key_matches_aws_documented_example() {
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "20130524",
            "us-east-1",
        );
        assert_eq!(
            hex(&key),
            "dbb893acc010964918f1fd433add87c70e8b0db6be30c1fbeafefa5ec6ba8378"
        );
    }

    #[test]
    fn slug_and_filename() {
        assert_eq!(
            slugify("Backup Akhir, Tahun 2026!"),
            "backup-akhir-tahun-2026"
        );
        assert_eq!(slugify("!!!"), "");
        let name = build_backup_filename(Some("!!!"));
        assert!(
            name.starts_with("arumanis_")
                && name.ends_with(".zip")
                && name.matches('_').count() == 2
        );
    }

    #[test]
    fn sql_value_escaping() {
        let mut out = String::new();
        push_sql_value(&mut out, Some(b"a'b\\c\nd"));
        assert_eq!(out, "'a\\'b\\\\c\\nd'");
        let mut out = String::new();
        push_sql_value(&mut out, Some(&[0xff, 0x00]));
        assert_eq!(out, "X'ff00'");
        let mut out = String::new();
        push_sql_value(&mut out, None);
        assert_eq!(out, "NULL");
    }

    #[test]
    fn form_and_json_bodies() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_TYPE,
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        let m = parse_body(&h, b"label=uji+a%21&include_media=0").unwrap();
        assert_eq!(m.get("label").unwrap(), "uji a!");
        let mut errs = Errs::new();
        assert!(!bool_input(&m, "include_media", true, &mut errs));
        assert!(bool_input(&m, "s3_direct", false, &mut errs) == false);
        assert!(errs.is_empty());
    }

    #[test]
    fn bool_and_pick_rules() {
        let mut errs = Errs::new();
        let m = obj(json!({ "a": "yes", "b": null, "c": "" }));
        bool_input(&m, "a", true, &mut errs);
        assert!(errs.contains_key("a"));
        assert!(bool_input(&obj(json!({})), "x", true, &mut Errs::new()));
        assert_eq!(
            pick(Some("0".into()), Some("stored".into())),
            Some("stored".into())
        );
        assert_eq!(
            pick(Some("in".into()), Some("stored".into())),
            Some("in".into())
        );
        assert_eq!(pick(None, None), None);
    }

    #[test]
    fn xml_error_fields() {
        let body = "<Error><Code>AccessDenied</Code><Message>Denied</Message></Error>";
        assert_eq!(xml_tag(body, "Code").as_deref(), Some("AccessDenied"));
        assert_eq!(xml_tag(body, "Missing"), None);
    }
}
