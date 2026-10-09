//! Pengaturan aplikasi (`AppSettingController`) dan bagian lokal `BackupController`.
//!
//! Rute:
//! - `POST /api/app-settings` (`store`): teks, secret, dan berkas (`logo`, `favicon`, `login_cover`,
//!   template kontrak). Respons = daftar seluruh setting seperti `GET /api/app-settings`.
//! - `GET /api/app-settings/storage-stats`
//! - `GET /api/app-settings/kontrak-templates` dan `GET .../{key}/download`
//! - `GET /api/app-settings/backups` (daftar lokal), `GET .../backups/jobs/{jobId}` (status job),
//!   `GET .../backups/{filename}` (unduh), dan `DELETE .../backups/{filename}` (hapus).
//!
//! Perbedaan dengan Laravel:
//! - Logo: `BrandColorService::syncFromLogoUpload` belum dipindah. Berkas logo tersimpan, tetapi
//!   `brand_primary_color` tidak dihitung ulang.
//! - Audit: setting bertipe `secret` dicatat tanpa nilainya. Laravel menulis nilai plain ke `tbl_audit_logs`.
//!   Audit hanya memuat field yang berubah (tanpa `updated_at`).
//! - Backup: daftar, unduh, dan hapus hanya untuk berkas lokal di `system-backups`. Bila
//!   `s3_backup_enabled = 1`, daftar tidak memuat berkas S3, unduh berkas yang tidak lokal dan hapus
//!   menjawab 501 (tidak mengubah apa pun). Backup create, restore, cancel job, uji S3, dan Google
//!   Drive belum dipindah.
//! - Validasi berkas: ekstensi dan tanda awal isi berkas (magic number), bukan `finfo` penuh.
//!   SVG hanya diperiksa ada tag `<svg`.
//! - Batas body route `store` 12 MB (template kontrak 10 MB ditambah overhead multipart).
//! - Respons `store` 200. Unduh berkas memakai `Content-Disposition: attachment`.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::UNIX_EPOCH,
};

use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, Multipart, Path as UrlPath, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Form, Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};
use tokio::io::AsyncReadExt;

use crate::{
    audit, format::iso8601_utc, kontrak_document, lookup, mailer, media,
    notifications::require_admin, notify::new_uuid, require_auth, AppState,
};

/// Batas body route `store` (template kontrak 10 MB ditambah multipart).
pub const BODY_LIMIT: usize = 12 * 1024 * 1024;
const APP_MODEL: &str = "App\\Models\\AppSetting";
const COLLECTION: &str = "app-settings";
const BACKUP_DIR: &str = "system-backups";
const CHUNK: usize = 256 * 1024;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan tidak valid: {e}"),
    )
}

fn invalid(errors: Errs) -> ApiError {
    ApiError::validation("The given data was invalid.", errors)
}

/// `Content-Disposition` dengan nama berkas yang sudah dibersihkan dari tanda kutip dan kontrol.
fn attachment(name: &str) -> String {
    let safe: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    format!("attachment; filename=\"{safe}\"")
}

// ---------------------------------------------------------------------------
// Input dan validasi `store`
// ---------------------------------------------------------------------------

/// Field yang berkas di formulir `store` (dibaca sebagai berkas bila dikirim dengan nama berkas).
const FILE_FIELDS: &[&str] = &[
    "logo",
    "favicon",
    "login_cover",
    "kontrak_template_spk",
    "kontrak_template_ringkasan",
    "kontrak_template_bap",
    "kontrak_template_cover_am",
    "kontrak_template_cover_san",
];

/// Setting teks di `AppSettingController@store`, dalam urutan penulisan Laravel.
/// `mail_password` dan `s3_secret_access_key` hanya ditulis bila terisi.
const TEXT_KEYS: &[&str] = &[
    "app_name",
    "app_description",
    "tahun_anggaran",
    "landing_page_active",
    "spm_detail_page_active",
    "capaian_publik_section_active",
    "penerima_pin",
    "pengawas_berkas_show_rab",
    "pengawas_berkas_show_gambar",
    "pengawas_berkas_show_nego",
    "maintenance_mode",
    "maintenance_bypass_emails",
    "mail_enabled",
    "mail_host",
    "mail_port",
    "mail_encryption",
    "mail_username",
    "mail_password",
    "mail_from_address",
    "mail_from_name",
    "contact_email",
    "mail_body_format",
    "mail_subject",
    "mail_body",
    "kontrak_nama_ppk",
    "kontrak_nip_ppk",
    "kontrak_nama_pptk",
    "kontrak_nip_pptk",
    "kontrak_skpd",
    "kontrak_nomor_dpa",
    "kontrak_tanggal_dpa",
    "kontrak_cara_pembayaran",
    "kontrak_masa_pemeliharaan_hari",
    "s3_backup_enabled",
    "s3_endpoint",
    "s3_region",
    "s3_bucket",
    "s3_access_key_id",
    "s3_secret_access_key",
];

/// Setting yang nilainya tidak ditulis ke audit (tipe `secret`).
const SECRET_KEYS: &[&str] = &["penerima_pin", "mail_password", "s3_secret_access_key"];

/// Berkas yang disimpan di koleksi `app-settings` (`setting`, aturan `mimes`, `max`, nama file).
struct FileSpec {
    key: &'static str,
    mimes: &'static [&'static str],
    max_kb: usize,
    /// Template kontrak: ekstensi di-lowercase dan memakai `default_ext` bila kosong.
    kontrak: Option<&'static str>,
}

const FILE_SPECS: &[FileSpec] = &[
    FileSpec {
        key: "logo",
        mimes: &["jpg", "jpeg", "png", "svg"],
        max_kb: 2048,
        kontrak: None,
    },
    FileSpec {
        key: "favicon",
        mimes: &["jpg", "jpeg", "png", "svg", "ico"],
        max_kb: 1024,
        kontrak: None,
    },
    FileSpec {
        key: "login_cover",
        mimes: &["jpg", "jpeg", "png", "webp"],
        max_kb: 5120,
        kontrak: None,
    },
    FileSpec {
        key: "kontrak_template_spk",
        mimes: &["docx"],
        max_kb: 10240,
        kontrak: Some("docx"),
    },
    FileSpec {
        key: "kontrak_template_ringkasan",
        mimes: &["docx", "xlsx"],
        max_kb: 10240,
        kontrak: Some("xlsx"),
    },
    FileSpec {
        key: "kontrak_template_bap",
        mimes: &["docx"],
        max_kb: 10240,
        kontrak: Some("docx"),
    },
    FileSpec {
        key: "kontrak_template_cover_am",
        mimes: &["docx"],
        max_kb: 10240,
        kontrak: Some("docx"),
    },
    FileSpec {
        key: "kontrak_template_cover_san",
        mimes: &["docx"],
        max_kb: 10240,
        kontrak: Some("docx"),
    },
];

/// Batas JSON `store` (hanya teks).
const JSON_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
struct Input {
    /// Ada di map = dikirim. `None` = dikirim kosong (ConvertEmptyStringsToNull).
    fields: BTreeMap<String, Option<String>>,
    files: BTreeMap<String, media::Upload>,
}

impl Input {
    fn has(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    /// Nilai teks; `None` bila tidak dikirim atau kosong.
    fn text(&self, key: &str) -> Option<String> {
        self.fields.get(key).cloned().flatten()
    }

    /// `$request->boolean()`: `1`, `true`, `on`, dan `yes` bernilai benar.
    fn boolean(&self, key: &str) -> bool {
        matches!(
            self.text(key).map(|s| s.to_ascii_lowercase()).as_deref(),
            Some("1" | "true" | "on" | "yes")
        )
    }
}

/// Trim seperti `TrimStrings`, lalu kosong menjadi null.
fn normalize(raw: &str) -> Option<String> {
    let t = raw.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn json_field(v: Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => normalize(&s),
        Value::Bool(b) => Some(if b { "1" } else { "0" }.to_string()),
        other => Some(other.to_string()),
    }
}

/// Field formulir/JSON sebagai `Map` (nilai kosong = `null`), untuk rute lain yang memakai `read_input`.
pub(crate) async fn flat_input(
    state: &AppState,
    request: Request,
) -> Result<Map<String, Value>, ApiError> {
    let input = read_input(state, request).await?;
    Ok(input
        .fields
        .into_iter()
        .map(|(k, v)| (k, v.map_or(Value::Null, Value::String)))
        .collect())
}

async fn read_input(state: &AppState, request: Request) -> Result<Input, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut input = Input::default();
    if content_type.starts_with("multipart/form-data") {
        let mut multipart = Multipart::from_request(request, state)
            .await
            .map_err(bad_request)?;
        while let Some(field) = multipart.next_field().await.map_err(bad_request)? {
            let name = field.name().unwrap_or_default().to_string();
            let original = field.file_name().map(str::to_string);
            match original {
                Some(original_name) if FILE_FIELDS.contains(&name.as_str()) => {
                    let bytes = field.bytes().await.map_err(bad_request)?;
                    if !bytes.is_empty() {
                        input.files.insert(
                            name,
                            media::Upload {
                                original_name,
                                bytes: bytes.to_vec(),
                            },
                        );
                    }
                }
                Some(_) => {
                    // Berkas pada field yang tidak dikenal: diabaikan (Laravel tidak memakainya).
                    field.bytes().await.map_err(bad_request)?;
                }
                None => {
                    let text = field.text().await.map_err(bad_request)?;
                    input.fields.insert(name, normalize(&text));
                }
            }
        }
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        let Form(map) = Form::<HashMap<String, String>>::from_request(request, state)
            .await
            .map_err(bad_request)?;
        for (k, v) in map {
            input.fields.insert(k, normalize(&v));
        }
    } else if content_type.starts_with("application/json") {
        let bytes = axum::body::to_bytes(request.into_body(), JSON_LIMIT)
            .await
            .map_err(bad_request)?;
        if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&bytes) {
            for (k, v) in map {
                input.fields.insert(k, json_field(v));
            }
        }
    }
    Ok(input)
}

fn attribute(key: &str) -> String {
    key.replace('_', " ")
}

type Errs = BTreeMap<String, Vec<String>>;

fn add(errs: &mut Errs, key: &str, message: String) {
    errs.entry(key.to_string()).or_default().push(message);
}

fn check_max_chars(errs: &mut Errs, input: &Input, key: &str, max: usize) {
    if let Some(v) = input.text(key) {
        if v.chars().count() > max {
            add(
                errs,
                key,
                format!(
                    "The {} field must not be greater than {max} characters.",
                    attribute(key)
                ),
            );
        }
    }
}

fn check_in(errs: &mut Errs, input: &Input, key: &str, allowed: &[&str]) {
    if let Some(v) = input.text(key) {
        if !allowed.contains(&v.as_str()) {
            add(
                errs,
                key,
                format!("The selected {} is invalid.", attribute(key)),
            );
        }
    }
}

fn check_email(errs: &mut Errs, input: &Input, key: &str, max: usize) {
    if let Some(v) = input.text(key) {
        if !is_email(&v) || v.chars().count() > max {
            add(
                errs,
                key,
                format!(
                    "The {} field must be a valid email address.",
                    attribute(key)
                ),
            );
        }
    }
}

/// Pendekatan untuk aturan `email` Laravel (tanpa validasi DNS).
pub(crate) fn is_email(v: &str) -> bool {
    let re =
        regex::Regex::new(r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$")
            .expect("regex email valid");
    re.is_match(v)
}

fn check_int(errs: &mut Errs, input: &Input, key: &str, min: i64, max: i64) {
    if let Some(v) = input.text(key) {
        match v.parse::<i64>() {
            Err(_) => add(
                errs,
                key,
                format!("The {} field must be an integer.", attribute(key)),
            ),
            Ok(n) => {
                if n < min {
                    add(
                        errs,
                        key,
                        format!("The {} field must be at least {min}.", attribute(key)),
                    );
                }
                if n > max {
                    add(
                        errs,
                        key,
                        format!(
                            "The {} field must not be greater than {max}.",
                            attribute(key)
                        ),
                    );
                }
            }
        }
    }
}

/// Tanda awal isi berkas untuk setiap ekstensi yang diizinkan.
fn magic_ok(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "jpg" | "jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
        "webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "ico" => bytes.starts_with(&[0, 0, 1, 0]),
        "docx" | "xlsx" => bytes.starts_with(b"PK\x03\x04"),
        "svg" => String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
            .to_ascii_lowercase()
            .contains("<svg"),
        _ => false,
    }
}

fn check_file(errs: &mut Errs, input: &Input, spec: &FileSpec) {
    let Some(upload) = input.files.get(spec.key) else {
        return;
    };
    let ext = upload.extension().to_ascii_lowercase();
    if !spec.mimes.contains(&ext.as_str()) || !magic_ok(&ext, &upload.bytes) {
        add(
            errs,
            spec.key,
            format!(
                "The {} field must be a file of type: {}.",
                attribute(spec.key),
                spec.mimes.join(", ")
            ),
        );
    }
    if upload.bytes.len() > spec.max_kb * 1024 {
        add(
            errs,
            spec.key,
            format!(
                "The {} field must not be greater than {} kilobytes.",
                attribute(spec.key),
                spec.max_kb
            ),
        );
    }
}

fn validate(input: &Input) -> Result<(), ApiError> {
    let mut e = Errs::new();
    check_max_chars(&mut e, input, "app_name", 255);
    check_max_chars(&mut e, input, "app_description", 500);
    check_max_chars(&mut e, input, "tahun_anggaran", 4);
    for key in [
        "landing_page_active",
        "spm_detail_page_active",
        "capaian_publik_section_active",
        "pengawas_berkas_show_rab",
        "pengawas_berkas_show_gambar",
        "pengawas_berkas_show_nego",
        "maintenance_mode",
        "mail_enabled",
        "s3_backup_enabled",
    ] {
        check_in(&mut e, input, key, &["0", "1"]);
    }
    check_max_chars(&mut e, input, "maintenance_bypass_emails", 1000);
    check_max_chars(&mut e, input, "mail_host", 255);
    check_max_chars(&mut e, input, "mail_port", 5);
    check_in(&mut e, input, "mail_encryption", &["tls", "ssl", "none"]);
    check_email(&mut e, input, "mail_username", 255);
    check_max_chars(&mut e, input, "mail_password", 2000);
    check_email(&mut e, input, "mail_from_address", 255);
    check_max_chars(&mut e, input, "mail_from_name", 255);
    check_email(&mut e, input, "contact_email", 255);
    check_in(
        &mut e,
        input,
        "mail_body_format",
        &["plain", "markdown", "html"],
    );
    check_max_chars(&mut e, input, "mail_subject", 255);
    check_max_chars(&mut e, input, "mail_body", 50000);
    check_max_chars(&mut e, input, "kontrak_nama_ppk", 255);
    check_max_chars(&mut e, input, "kontrak_nip_ppk", 32);
    check_max_chars(&mut e, input, "kontrak_nama_pptk", 255);
    check_max_chars(&mut e, input, "kontrak_nip_pptk", 32);
    check_int(&mut e, input, "kontrak_masa_pemeliharaan_hari", 1, 3650);
    check_max_chars(&mut e, input, "kontrak_skpd", 255);
    check_max_chars(&mut e, input, "kontrak_nomor_dpa", 255);
    check_max_chars(&mut e, input, "kontrak_tanggal_dpa", 255);
    check_in(
        &mut e,
        input,
        "kontrak_cara_pembayaran",
        &["sekaligus", "termin", "bulan"],
    );
    check_max_chars(&mut e, input, "s3_endpoint", 255);
    check_max_chars(&mut e, input, "s3_region", 64);
    check_max_chars(&mut e, input, "s3_bucket", 64);
    check_max_chars(&mut e, input, "s3_access_key_id", 128);
    check_max_chars(&mut e, input, "s3_secret_access_key", 255);
    for spec in FILE_SPECS {
        check_file(&mut e, input, spec);
    }
    if e.is_empty() {
        Ok(())
    } else {
        Err(invalid(e))
    }
}

// ---------------------------------------------------------------------------
// Penulisan setting dan berkas
// ---------------------------------------------------------------------------

/// Berkas lama (dihapus setelah commit) dan baru (dihapus bila transaksi gagal).
#[derive(Default)]
struct Pending {
    new_dirs: Vec<PathBuf>,
    old_dirs: Vec<PathBuf>,
}

/// Setting teks/secret yang ikut ditulis, dalam urutan Laravel.
fn text_settings(input: &Input) -> Vec<(&'static str, Option<String>, &'static str)> {
    let mut out = Vec::new();
    for &key in TEXT_KEYS {
        let kind = if SECRET_KEYS.contains(&key) {
            "secret"
        } else {
            "text"
        };
        match key {
            "mail_password" | "s3_secret_access_key" => {
                if let Some(v) = input.text(key) {
                    out.push((key, Some(v), kind));
                }
            }
            _ => {
                if input.has(key) {
                    out.push((key, input.text(key), kind));
                }
            }
        }
    }
    out
}

/// `AppSetting::setValue` (`updateOrCreate`). Audit hanya bila ada perubahan.
pub(crate) async fn upsert_setting(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
    url: &str,
    headers: &HeaderMap,
    key: &str,
    value: Option<&str>,
    kind: &str,
) -> Result<u64, ApiError> {
    let secret = |k: &str| k == "secret";
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, `value`, `type` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1 FOR UPDATE",
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;

    match row {
        None => {
            let res = sqlx::query(
                "INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
            )
            .bind(key)
            .bind(value)
            .bind(kind)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
            let id = res.last_insert_id();
            let mut new = Map::new();
            new.insert("id".into(), json!(id));
            new.insert("key".into(), json!(key));
            new.insert("type".into(), json!(kind));
            if !secret(kind) {
                new.insert("value".into(), json!(value));
            }
            audit::write(
                tx,
                audit::Entry {
                    actor,
                    event: "created",
                    auditable_type: APP_MODEL,
                    auditable_id: id,
                    old: None,
                    new: Some(new),
                    url,
                },
                headers,
            )
            .await
            .map_err(internal)?;
            Ok(id)
        }
        Some(r) => {
            let id: u64 = r.try_get("id").map_err(internal)?;
            let old_value: Option<String> = r.try_get("value").map_err(internal)?;
            let old_kind: String = r.try_get("type").map_err(internal)?;
            let value_changed = old_value.as_deref() != value;
            let kind_changed = old_kind != kind;
            if !value_changed && !kind_changed {
                return Ok(id);
            }
            sqlx::query(
                "UPDATE app_settings SET `value` = ?, `type` = ?, updated_at = NOW() WHERE id = ?",
            )
            .bind(value)
            .bind(kind)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;

            let mut old = Map::new();
            let mut new = Map::new();
            if kind_changed {
                old.insert("type".into(), json!(old_kind));
                new.insert("type".into(), json!(kind));
            }
            if value_changed && !secret(kind) && !secret(&old_kind) {
                old.insert("value".into(), json!(old_value));
                new.insert("value".into(), json!(value));
            }
            audit::write(
                tx,
                audit::Entry {
                    actor,
                    event: "updated",
                    auditable_type: APP_MODEL,
                    auditable_id: id,
                    old: Some(old),
                    new: Some(new),
                    url,
                },
                headers,
            )
            .await
            .map_err(internal)?;
            Ok(id)
        }
    }
}

/// Id setting berdasarkan kunci, bila ada.
async fn setting_id(tx: &mut Transaction<'_, MySql>, key: &str) -> Result<Option<u64>, ApiError> {
    sqlx::query_scalar::<_, u64>(
        "SELECT CAST(id AS UNSIGNED) FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)
}

/// Ganti isi koleksi `app-settings` dengan satu berkas baru (`clearMediaCollection` + `addMediaFromRequest`).
async fn replace_file(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
    url: &str,
    headers: &HeaderMap,
    pending: &mut Pending,
    key: &str,
    upload: &media::Upload,
    file_name: String,
) -> Result<(), ApiError> {
    let id = upsert_setting(tx, actor, url, headers, key, None, "file").await?;
    pending
        .old_dirs
        .extend(media::delete_collection(tx, APP_MODEL, id, COLLECTION, None).await?);
    let mime = media::mime_for_name(&upload.original_name);
    let stored = media::attach_named(
        tx,
        APP_MODEL,
        id,
        COLLECTION,
        upload,
        mime,
        false,
        Some(file_name),
    )
    .await?;
    pending.new_dirs.push(stored.dir);
    Ok(())
}

async fn apply(
    tx: &mut Transaction<'_, MySql>,
    input: &Input,
    actor: u64,
    url: &str,
    headers: &HeaderMap,
    pending: &mut Pending,
) -> Result<(), ApiError> {
    for (key, value, kind) in text_settings(input) {
        upsert_setting(tx, actor, url, headers, key, value.as_deref(), kind).await?;
    }
    for spec in FILE_SPECS {
        if let Some(upload) = input.files.get(spec.key) {
            let ext = upload.extension();
            let file_name = match spec.kontrak {
                // `strtolower($ext ?: $default)` pada template kontrak.
                Some(default_ext) => {
                    let ext = if ext.is_empty() {
                        default_ext.to_string()
                    } else {
                        ext.to_ascii_lowercase()
                    };
                    format!("{}_{}.{ext}", spec.key, new_uuid())
                }
                // `'logo_' . Str::uuid() . '.' . getClientOriginalExtension()`.
                None => format!("{}_{}.{ext}", spec.key, new_uuid()),
            };
            replace_file(
                tx, actor, url, headers, pending, spec.key, upload, file_name,
            )
            .await?;
        } else if spec.key == "login_cover" && input.boolean("login_cover_remove") {
            if let Some(id) = setting_id(tx, spec.key).await? {
                pending
                    .old_dirs
                    .extend(media::delete_collection(tx, APP_MODEL, id, COLLECTION, None).await?);
            }
        }
    }
    Ok(())
}

/// `POST /api/app-settings`: admin. Respons 200 dengan daftar seluruh setting.
pub async fn store(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let input = read_input(&state, request).await?;
    validate(&input)?;

    let url = format!("{}/api/app-settings", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut pending = Pending::default();
    if let Err(e) = apply(&mut tx, &input, user.user_id, &url, &headers, &mut pending).await {
        media::remove_dirs(&pending.new_dirs).await;
        return Err(e);
    }
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&pending.new_dirs).await;
        return Err(internal(e));
    }
    media::remove_dirs(&pending.old_dirs).await;

    lookup::index(State(state.clone()))
        .await
        .map(IntoResponse::into_response)
}

// ---------------------------------------------------------------------------
// Statistik penyimpanan
// ---------------------------------------------------------------------------

async fn collection_size(pool: &MySqlPool, collection: &str) -> Result<(f64, i64), ApiError> {
    let row = sqlx::query(
        "SELECT CAST(COALESCE(SUM(size), 0) AS DOUBLE) AS total, CAST(COUNT(*) AS SIGNED) AS n \
         FROM media WHERE collection_name = ?",
    )
    .bind(collection)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok((
        row.try_get("total").map_err(internal)?,
        row.try_get("n").map_err(internal)?,
    ))
}

/// Ukuran database (`information_schema`). Gagal dibaca dihitung 0 seperti try/catch di Laravel.
async fn database_size(pool: &MySqlPool) -> f64 {
    sqlx::query_scalar::<_, f64>(
        "SELECT CAST(COALESCE(SUM(data_length + index_length), 0) AS DOUBLE) \
         FROM information_schema.TABLES WHERE table_schema = DATABASE()",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0.0)
}

/// `GET /api/app-settings/storage-stats`: admin.
pub async fn storage_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let (foto, foto_count) = collection_size(&state.pool, "foto/pekerjaan").await?;
    let (berkas, berkas_count) = collection_size(&state.pool, "berkas/dokumen").await?;
    let database = database_size(&state.pool).await;
    Ok(Json(json!({
        "data": {
            "foto": foto,
            "foto_count": foto_count,
            "berkas": berkas,
            "berkas_count": berkas_count,
            "database": database,
            "media_total": foto + berkas,
            "app_total": foto + berkas + database,
        }
    })))
}

// ---------------------------------------------------------------------------
// Template kontrak
// ---------------------------------------------------------------------------

struct KontrakTemplate {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    default_name: &'static str,
    format: &'static str,
}

/// `KontrakTemplateService::TEMPLATES`.
const KONTRAK_TEMPLATES: &[KontrakTemplate] = &[
    KontrakTemplate {
        key: "kontrak_template_spk",
        label: "SPK / Surat Perintah Kerja",
        description: "Template utama generate dokumen kontrak (SPK).",
        default_name: "SPK_Template.docx",
        format: "docx",
    },
    KontrakTemplate {
        key: "kontrak_template_ringkasan",
        label: "Ringkasan Kontrak",
        description: "Template Excel ringkasan kontrak (.xlsx).",
        default_name: "ringkasan_kontrak_template.xlsx",
        format: "xlsx",
    },
    KontrakTemplate {
        key: "kontrak_template_bap",
        label: "BAP (Berita Acara Pembayaran)",
        description: "Template BAP / berita acara pembayaran.",
        default_name: "bap_template.docx",
        format: "docx",
    },
    KontrakTemplate {
        key: "kontrak_template_cover_am",
        label: "Cover Kontrak (Air Minum)",
        description: "Cover kontrak untuk sub bidang air minum.",
        default_name: "cover_kontrak_am.docx",
        format: "docx",
    },
    KontrakTemplate {
        key: "kontrak_template_cover_san",
        label: "Cover Kontrak (Sanitasi)",
        description: "Cover kontrak untuk sub bidang sanitasi.",
        default_name: "cover_kontrak_san.docx",
        format: "docx",
    },
];

fn kontrak_template(key: &str) -> Option<&'static KontrakTemplate> {
    KONTRAK_TEMPLATES.iter().find(|t| t.key == key)
}

/// Baris setting (`id`, `updated_at`) untuk kunci, bila ada.
async fn setting_row(
    pool: &MySqlPool,
    key: &str,
) -> Result<Option<(u64, Option<DateTime<Utc>>)>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, updated_at FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some((
            r.try_get("id").map_err(internal)?,
            r.try_get("updated_at").map_err(internal)?,
        ))),
    }
}

/// `GET /api/app-settings/kontrak-templates`: admin. `{"data": [...]}`.
pub async fn kontrak_templates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let mut data = Vec::with_capacity(KONTRAK_TEMPLATES.len());
    for t in KONTRAK_TEMPLATES {
        let (media, updated) = match setting_row(&state.pool, t.key).await? {
            Some((id, updated)) => (
                media::first_media(&state.pool, APP_MODEL, id, COLLECTION)
                    .await
                    .map_err(internal)?,
                updated,
            ),
            None => (None, None),
        };
        data.push(json!({
            "key": t.key,
            "label": t.label,
            "description": t.description,
            "default_filename": t.default_name,
            "form_field": t.key,
            "format": t.format,
            "has_custom": media.is_some(),
            "filename": media.as_ref().map(|m| m.file_name.clone()),
            "updated_at": iso8601_utc(updated),
        }));
    }
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/app-settings/kontrak-templates/{key}/download`: admin. Berkas unggahan bila ada
/// dan masih ada di disk, selain itu berkas default.
pub async fn download_kontrak_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(key): UrlPath<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let Some(t) = kontrak_template(&key) else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Template tidak dikenal.",
        ));
    };

    let media = match setting_row(&state.pool, t.key).await? {
        Some((id, _)) => media::first_media(&state.pool, APP_MODEL, id, COLLECTION)
            .await
            .map_err(internal)?,
        None => None,
    };
    let custom = media
        .as_ref()
        .map(|m| media::media_dir(m.id).join(&m.file_name))
        .filter(|p| p.exists());
    let path = match custom {
        Some(p) => p,
        None => {
            let p = kontrak_document::template_dir().join(t.default_name);
            if !p.exists() {
                return Err(ApiError::new(
                    StatusCode::NOT_FOUND,
                    format!("Template tidak ditemukan: {}", t.default_name),
                ));
            }
            p
        }
    };
    let filename = media
        .map(|m| m.file_name)
        .unwrap_or_else(|| t.default_name.to_string());
    let bytes = tokio::fs::read(&path).await.map_err(internal)?;

    Ok((
        [
            (
                header::CONTENT_TYPE,
                media::mime_for_name(&filename).to_string(),
            ),
            (header::CONTENT_DISPOSITION, attachment(&filename)),
        ],
        bytes,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Backup lokal
// ---------------------------------------------------------------------------

/// Akar disk `local` (`storage/app/private`). Dapat diganti dengan `PRIVATE_STORAGE_PATH`.
pub(crate) fn private_root() -> PathBuf {
    std::env::var_os("PRIVATE_STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/private"
            ))
        })
}

pub(crate) fn backup_dir() -> PathBuf {
    private_root().join(BACKUP_DIR)
}

fn job_path(job_id: &str) -> PathBuf {
    backup_dir().join("jobs").join(format!("{job_id}.json"))
}

/// `guardFilename`: `^[A-Za-z0-9._-]+\.zip$`.
pub(crate) fn guard_filename(name: &str) -> Result<(), ApiError> {
    let ok = name.len() > 4
        && name.ends_with(".zip")
        && name[..name.len() - 4]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Nama backup tidak valid",
        ))
    }
}

/// `guardJobId`: `^[A-Za-z0-9-]+$`.
pub(crate) fn guard_job_id(job_id: &str) -> Result<(), ApiError> {
    let ok = !job_id.is_empty()
        && job_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ID backup tidak valid",
        ))
    }
}

/// Route Laravel `{filename}` hanya cocok bila berakhiran `.zip`; selain itu 404 tanpa cek auth.
fn zip_route(name: &str) -> Result<(), ApiError> {
    if name.ends_with(".zip") {
        Ok(())
    } else {
        Err(ApiError::not_found())
    }
}

async fn s3_backup_enabled(pool: &MySqlPool) -> Result<bool, ApiError> {
    Ok(mailer::setting(pool, "s3_backup_enabled")
        .await
        .map_err(internal)?
        .as_deref()
        == Some("1"))
}

/// `GET /api/app-settings/backups`: admin. Daftar berkas `.zip` lokal, terbaru dulu.
pub async fn backups_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let dir = backup_dir();
    tokio::fs::create_dir_all(&dir).await.map_err(internal)?;

    let base = state.app_url.trim_end_matches('/').to_string();
    let mut items: Vec<(i64, Value)> = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await.map_err(internal)?;
    while let Some(entry) = entries.next_entry().await.map_err(internal)? {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().await.map_err(internal)?;
        if !meta.is_file() || !name.ends_with(".zip") {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        items.push((
            mtime,
            json!({
                "filename": name,
                "size": meta.len(),
                "last_modified": mtime,
                "storage": "local",
                "download_url": format!("{base}/api/app-settings/backups/{name}"),
            }),
        ));
    }
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let data: Vec<Value> = items.into_iter().map(|(_, v)| v).collect();
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/app-settings/backups/jobs/{jobId}`: admin. Status job dari berkas JSON.
pub async fn backup_show_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(job_id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    guard_job_id(&job_id)?;
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, "Status backup tidak ditemukan");
    let bytes = match tokio::fs::read(job_path(&job_id)).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(internal(e)),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => Ok(Json(json!({ "data": v }))),
        _ => Err(not_found()),
    }
}

/// `GET /api/app-settings/backups/{filename}`: admin. Berkas dikirim sebagai aliran.
pub async fn backup_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
) -> Result<Response, ApiError> {
    zip_route(&name)?;
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    guard_filename(&name)?;

    let path = backup_dir().join(&name);
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => m,
        _ => {
            if s3_backup_enabled(&state.pool).await? {
                return Err(ApiError::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "Unduh backup dari S3 belum didukung di layanan Rust.",
                ));
            }
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "Backup tidak ditemukan",
            ));
        }
    };
    let file = tokio::fs::File::open(&path).await.map_err(internal)?;
    let stream = futures_util::stream::try_unfold(file, |mut f| async move {
        let mut buf = vec![0u8; CHUNK];
        let n = f.read(&mut buf).await?;
        if n == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        buf.truncate(n);
        Ok(Some((Bytes::from(buf), f)))
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, attachment(&name)),
            (header::CONTENT_LENGTH, meta.len().to_string()),
            (
                header::HeaderName::from_static("x-accel-buffering"),
                "no".to_string(),
            ),
            (header::CACHE_CONTROL, "no-store, private".to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

/// `DELETE /api/app-settings/backups/{filename}`: admin. Menolak bila S3 aktif (tidak ada yang diubah).
pub async fn backup_destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    zip_route(&name)?;
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    guard_filename(&name)?;
    if s3_backup_enabled(&state.pool).await? {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "Menghapus backup di S3 belum didukung di layanan Rust.",
        ));
    }
    match tokio::fs::remove_file(backup_dir().join(&name)).await {
        Ok(()) => Ok(Json(json!({ "message": "Backup berhasil dihapus" }))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Backup tidak ditemukan",
        )),
        Err(e) => Err(internal(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_replaces_underscores_like_laravel() {
        assert_eq!(
            attribute("kontrak_masa_pemeliharaan_hari"),
            "kontrak masa pemeliharaan hari"
        );
    }

    #[test]
    fn filename_guard_matches_php_regex() {
        assert!(guard_filename("arumanis_20260101_010101.zip").is_ok());
        assert!(guard_filename("a.zip").is_ok());
        assert!(guard_filename(".zip").is_err());
        assert!(guard_filename("a b.zip").is_err());
        assert!(guard_filename("a/b.zip").is_err());
        assert!(guard_filename("a.tar").is_err());
    }

    #[test]
    fn job_id_guard_matches_php_regex() {
        assert!(guard_job_id("3f2a-b1c").is_ok());
        assert!(guard_job_id("a.b").is_err());
        assert!(guard_job_id("").is_err());
    }

    #[test]
    fn zip_route_rejects_other_extensions_as_not_found() {
        assert_eq!(
            zip_route("x.json").unwrap_err().status,
            StatusCode::NOT_FOUND
        );
        assert!(zip_route("x.zip").is_ok());
    }

    #[test]
    fn magic_bytes_decide_raster_and_office_files() {
        assert!(magic_ok("jpg", &[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!magic_ok("jpg", b"%PDF"));
        assert!(magic_ok(
            "png",
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        ));
        assert!(magic_ok("docx", b"PK\x03\x04rest"));
        assert!(magic_ok(
            "svg",
            b"  <?xml version=\"1.0\"?><svg xmlns=\"x\"/>"
        ));
        assert!(!magic_ok("svg", b"plain text"));
        assert!(magic_ok("webp", b"RIFF\0\0\0\0WEBPVP8 "));
    }

    #[test]
    fn disposition_strips_quotes_and_control_characters() {
        assert_eq!(
            attachment("a\"b\r\n.docx"),
            "attachment; filename=\"ab.docx\""
        );
    }

    #[test]
    fn kontrak_templates_follow_laravel_definitions() {
        let keys: Vec<&str> = KONTRAK_TEMPLATES.iter().map(|t| t.key).collect();
        assert_eq!(keys.len(), 5);
        let ringkasan = kontrak_template("kontrak_template_ringkasan").unwrap();
        assert_eq!(ringkasan.format, "xlsx");
        assert_eq!(ringkasan.default_name, "ringkasan_kontrak_template.xlsx");
        assert!(kontrak_template("kontrak_template_nope").is_none());
    }

    #[test]
    fn secret_settings_are_written_only_when_filled() {
        let mut input = Input::default();
        input.fields.insert("mail_password".into(), None);
        input.fields.insert("penerima_pin".into(), None);
        let keys: Vec<&str> = text_settings(&input).iter().map(|(k, _, _)| *k).collect();
        assert_eq!(keys, vec!["penerima_pin"]);
        let kinds: Vec<&str> = text_settings(&input).iter().map(|(_, _, t)| *t).collect();
        assert_eq!(kinds, vec!["secret"]);
    }

    #[test]
    fn integer_rule_reports_laravel_messages() {
        let mut input = Input::default();
        input
            .fields
            .insert("kontrak_masa_pemeliharaan_hari".into(), Some("0".into()));
        let err = validate(&input).unwrap_err();
        assert_eq!(
            err.errors.unwrap()["kontrak_masa_pemeliharaan_hari"],
            vec!["The kontrak masa pemeliharaan hari field must be at least 1.".to_string()]
        );
    }
}
