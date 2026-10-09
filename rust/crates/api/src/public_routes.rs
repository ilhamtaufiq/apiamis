//! Rute publik tanpa login: form kelembagaan SPAM dan formulir hubungi kami.
//!
//! - `GET /api/public/spam-kelembagaan/form/{token}` (`SpamKelembagaanShareController@publicShow`):
//!   throttle 60 per menit per IP. 200 `{success, data}`, atau 410 bila link tidak bisa dipakai.
//! - `POST /api/public/spam-kelembagaan/form/{token}` (`publicSubmit`): throttle 10 per menit per IP.
//!   201 `{success, message, data}`. 422 bila link tidak bisa dipakai, bila payload kosong, atau validasi gagal.
//! - `POST /api/public/contact` (`ContactController@store`): throttle `contact-inquiries`
//!   (3 per menit dan 10 per jam per IP, 5 per jam per email). 200 atau 503 `{status, message}`.
//!   Honeypot `website` yang terisi dibalas 200 tanpa mengirim email.
//!
//! Deviasi dan catatan:
//! - Throttle memakai `Limiter` (jendela geser per kunci). Laravel memakai counter per kunci, dan
//!   batas per menit dan per jam yang kuncinya sama berbagi counter. Rust memakai kunci terpisah
//!   per batas. Batas kontak dihitung satu per satu, jadi permintaan yang ditolak batas per jam tetap
//!   menambah hitungan batas per menit. Vendor Laravel tidak ada di repo, jadi ini belum diverifikasi.
//!   Laravel mungkin juga membagi `throttle:N,M` per IP di antara rute. Rust memakai kunci per rute.
//! - IP memakai `X-Forwarded-For` (elemen pertama). Tanpa header itu, Rust memakai kunci `unknown`.
//!   Laravel memakai alamat socket. `submitter_ip` juga kosong dalam kasus itu.
//! - `user_agent` dipotong 500 karakter (Laravel memotong 500 byte).
//! - `isUsable` dan `max_submissions` dicek tanpa kunci, sama seperti Laravel. Dua pengajuan bersamaan
//!   bisa melewati batas kuota.
//! - Kontak: `mail_layout::brand` ikut menulis `brand_primary_color` ke `app_settings`
//!   (sama dengan `BrandColorService::palette()`). Audit yang ditulis memakai `user_id = 0`,
//!   Laravel menulis `NULL` karena tidak ada user yang login.
//! - Kontak: alamat `Reply-To` memakai nama dan email pengirim, seperti `$mail->replyTo()`.
//!   Pengiriman memakai SMTP dari `mailer::transport`.
//! - Payload form kelembagaan: nilai `payload` dinormalisasi secara rekursif (`normalize`), seperti
//!   `TrimStrings` dan `ConvertEmptyStringsToNull`. Isi `payload` yang bukan field unit atau pengelola dibuang.
//! - Body JSON yang tidak valid diperlakukan sebagai input kosong (seperti `$request->all()` di Laravel).

use std::collections::BTreeMap;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use lettre::{
    address::Address,
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    AsyncTransport, Message,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySqlPool, Row};

use crate::{
    app_settings_mail::mail_layout::{self, Brand, WriteCtx},
    audit,
    mailer::{self, SmtpSettings},
    AppState,
};

const SHOW_MAX_PER_MINUTE: usize = 60;
const SUBMIT_MAX_PER_MINUTE: usize = 10;
const LINK_UNUSABLE: &str = "Link form tidak aktif, kedaluwarsa, atau kuota sudah penuh.";
const SUBMIT_OK: &str = "Usulan berhasil dikirim dan menunggu verifikasi admin.";
const EMPTY_PAYLOAD: &str = "Tidak ada data yang diisi untuk diusulkan.";
const CONTACT_OK: &str = "Pesan Anda telah terkirim. Tim kami akan menghubungi Anda segera.";
const CONTACT_MAIL_DISABLED: &str = "Layanan email belum diaktifkan. Silakan hubungi kami melalui Instagram atau datang langsung ke kantor.";
const CONTACT_NO_RECIPIENT: &str = "Email tujuan hubungi kami belum dikonfigurasi di pengaturan aplikasi.";
const CONTACT_SEND_FAILED: &str = "Pesan tidak dapat dikirim saat ini. Silakan coba lagi nanti.";

/// `SpamKelembagaanShareService::UNIT_FIELDS`.
const UNIT_FIELDS: [&str; 12] = [
    "name",
    "tahun_pembangunan",
    "sumber_dana",
    "program",
    "sistem_layanan",
    "sumber_mata_air_kap",
    "sumber_air_tanah_kap",
    "lain_lain_kap",
    "tarif_dasar_hukum",
    "iuran_nominal",
    "pendapatan_bulan",
    "biaya_operasional",
];

/// `SpamKelembagaanShareService::PENGELOLA_FIELDS`.
const PENGELOLA_FIELDS: [&str; 5] = ["pokmas", "perdes", "kepala", "bendahara", "sekretaris"];

/// Field datar pada `publicSubmit` beserta batas `max` (urutan sama dengan aturan Laravel).
const FLAT_FIELDS: [(&str, usize); 17] = [
    ("name", 255),
    ("tahun_pembangunan", 100),
    ("sumber_dana", 255),
    ("program", 255),
    ("sistem_layanan", 255),
    ("sumber_mata_air_kap", 255),
    ("sumber_air_tanah_kap", 255),
    ("lain_lain_kap", 255),
    ("tarif_dasar_hukum", 255),
    ("iuran_nominal", 255),
    ("pendapatan_bulan", 255),
    ("biaya_operasional", 255),
    ("pokmas", 255),
    ("perdes", 255),
    ("kepala", 255),
    ("bendahara", 255),
    ("sekretaris", 255),
];

const UNIT_SQL: &str = "SELECT CAST(u.id AS UNSIGNED) AS id, u.name, d.n_desa, k.n_kec, \
    u.tahun_pembangunan, u.sumber_dana, u.program, u.sistem_layanan, u.sumber_mata_air_kap, \
    u.sumber_air_tanah_kap, u.lain_lain_kap, u.tarif_dasar_hukum, u.iuran_nominal, \
    u.pendapatan_bulan, u.biaya_operasional, p.pokmas, p.perdes, p.kepala, p.bendahara, p.sekretaris \
    FROM tbl_unit_spam u \
    LEFT JOIN tbl_desa d ON d.id = u.desa_id \
    LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id \
    LEFT JOIN tbl_pengelola p ON p.unit_spam_id = u.id \
    WHERE u.id = ? LIMIT 1";

// ---------------------------------------------------------------------------
// Bantuan umum
// ---------------------------------------------------------------------------

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn validation(errs: BTreeMap<String, Vec<String>>) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

fn single(key: &str, message: &str) -> BTreeMap<String, Vec<String>> {
    let mut errs = BTreeMap::new();
    add(&mut errs, key, message);
    errs
}

fn add(errs: &mut BTreeMap<String, Vec<String>>, key: &str, message: impl Into<String>) {
    errs.entry(key.to_string()).or_default().push(message.into());
}

/// Body JSON sebagai objek. Body kosong, tidak valid, atau bukan objek menjadi input kosong.
fn parse_body(bytes: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn client_ip(headers: &HeaderMap) -> String {
    audit::client_info(headers)
        .0
        .unwrap_or_else(|| "unknown".to_string())
}

/// Respon 429 seperti `throttle` Laravel: pesan `Too Many Attempts.` dan `retry-after`.
fn too_many(retry: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", retry.to_string())],
        Json(json!({ "message": "Too Many Attempts." })),
    )
        .into_response()
}

/// `Carbon::toIso8601String()`, misalnya `2026-10-09T10:00:00+00:00`.
fn iso8601(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// `Request::filled()`: ada dan bukan string kosong atau spasi saja.
fn is_filled(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(_) => true,
    }
}

/// Aturan `email` yang disederhanakan (sama dengan `users_write`).
fn looks_like_email(s: &str) -> bool {
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !local.contains(' ')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(' ')
}

/// `required|string|max:N`. `None` bila tidak ada atau tidak valid (pesan sudah dicatat).
fn required_string(
    input: &Map<String, Value>,
    key: &str,
    max: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    match input.get(key) {
        None | Some(Value::Null) => {
            add(errs, key, format!("The {key} field is required."));
            None
        }
        Some(Value::String(s)) if s.trim().is_empty() => {
            add(errs, key, format!("The {key} field is required."));
            None
        }
        Some(Value::String(s)) => {
            let t = s.trim().to_string();
            if t.chars().count() > max {
                add(
                    errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                );
                None
            } else {
                Some(t)
            }
        }
        Some(_) => {
            add(errs, key, format!("The {key} field must be a string."));
            None
        }
    }
}

/// `required|email|max:255`. Email yang gagal dua aturan mendapat dua pesan, seperti Laravel.
fn required_email(
    input: &Map<String, Value>,
    key: &str,
    max: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    match input.get(key) {
        None | Some(Value::Null) => {
            add(errs, key, format!("The {key} field is required."));
            None
        }
        Some(Value::String(s)) if s.trim().is_empty() => {
            add(errs, key, format!("The {key} field is required."));
            None
        }
        Some(Value::String(s)) => {
            let t = s.trim().to_string();
            let mut ok = true;
            if !looks_like_email(&t) {
                add(
                    errs,
                    key,
                    format!("The {key} field must be a valid email address."),
                );
                ok = false;
            }
            if t.chars().count() > max {
                add(
                    errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                );
                ok = false;
            }
            ok.then_some(t)
        }
        Some(_) => {
            add(
                errs,
                key,
                format!("The {key} field must be a valid email address."),
            );
            None
        }
    }
}

/// `nullable|string|max:N`. Hasil luar `None` berarti field tidak ada atau tidak valid.
/// Hasil dalam `None` berarti nilai null (null atau string kosong).
fn nullable_string(
    input: &Map<String, Value>,
    key: &str,
    max: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<String>> {
    match input.get(key)? {
        Value::Null => Some(None),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Some(None)
            } else if t.chars().count() > max {
                add(
                    errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                );
                None
            } else {
                Some(Some(t.to_string()))
            }
        }
        _ => {
            add(errs, key, format!("The {key} field must be a string."));
            None
        }
    }
}

/// `TrimStrings` dan `ConvertEmptyStringsToNull` secara rekursif.
fn normalize(v: &Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), normalize(v))).collect()),
        other => other.clone(),
    }
}

/// `SpamKelembagaanShareService::sanitizePayload`: hanya field unit dan pengelola yang disimpan.
fn sanitize(input: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for field in UNIT_FIELDS.iter().chain(PENGELOLA_FIELDS.iter()) {
        if let Some(v) = input.get(*field) {
            out.insert(field.to_string(), normalize(v));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Form kelembagaan SPAM
// ---------------------------------------------------------------------------

/// Baris `spam_kelembagaan_share_links` yang dipakai form publik.
struct ShareLink {
    id: u64,
    unit_spam_id: u64,
    token: String,
    label: Option<String>,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
    max_submissions: Option<i64>,
    submission_count: i64,
}

impl ShareLink {
    /// `SpamKelembagaanShareLink::isUsable()`.
    fn is_usable(&self, now: DateTime<Utc>) -> bool {
        if !self.is_active {
            return false;
        }
        if let Some(exp) = self.expires_at {
            if exp < now {
                return false;
            }
        }
        if let Some(max) = self.max_submissions {
            if self.submission_count >= max {
                return false;
            }
        }
        true
    }
}

fn link_from_row(r: &MySqlRow) -> Result<ShareLink, sqlx::Error> {
    Ok(ShareLink {
        id: r.try_get("id")?,
        unit_spam_id: r.try_get("unit_spam_id")?,
        token: r.try_get("token")?,
        label: r.try_get("label")?,
        is_active: r.try_get::<i64, _>("is_active")? != 0,
        expires_at: r.try_get("expires_at")?,
        max_submissions: r.try_get("max_submissions")?,
        submission_count: r.try_get("submission_count")?,
    })
}

/// Unit dengan wilayah, pengelola, dan `snapshotUnit()`.
struct UnitRow {
    id: u64,
    name: Option<String>,
    desa: Option<String>,
    kecamatan: Option<String>,
    snapshot: Map<String, Value>,
}

fn unit_from_row(r: &MySqlRow) -> Result<UnitRow, sqlx::Error> {
    let mut snapshot = Map::new();
    for field in UNIT_FIELDS.iter().chain(PENGELOLA_FIELDS.iter()) {
        let value: Option<String> = r.try_get(*field)?;
        snapshot.insert(field.to_string(), value.map_or(Value::Null, Value::String));
    }
    Ok(UnitRow {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        // `$unit->desa?->n_desa ?? ...nama_desa`: kolom `nama_desa` tidak ada, jadi null.
        desa: r.try_get("n_desa")?,
        kecamatan: r.try_get("n_kec")?,
        snapshot,
    })
}

async fn find_link(pool: &MySqlPool, token: &str) -> Result<Option<ShareLink>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, CAST(unit_spam_id AS UNSIGNED) AS unit_spam_id, token, label, \
         CAST(is_active AS SIGNED) AS is_active, expires_at, CAST(max_submissions AS SIGNED) AS max_submissions, \
         CAST(submission_count AS SIGNED) AS submission_count \
         FROM spam_kelembagaan_share_links WHERE token = ? LIMIT 1",
    )
    .bind(token)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    row.as_ref().map(link_from_row).transpose().map_err(internal)
}

async fn load_unit(pool: &MySqlPool, unit_id: u64) -> Result<Option<UnitRow>, ApiError> {
    let row = sqlx::query(UNIT_SQL)
        .bind(unit_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(unit_from_row).transpose().map_err(internal)
}

/// `SpamKelembagaanShareService::publicFormData`.
fn form_data(link: &ShareLink, unit: &UnitRow, now: DateTime<Utc>) -> Value {
    json!({
        "link": {
            "token": link.token,
            "label": link.label,
            "expires_at": link.expires_at.map(iso8601),
            "is_usable": link.is_usable(now),
        },
        "unit": {
            "id": unit.id,
            "name": unit.name,
            "desa": unit.desa,
            "kecamatan": unit.kecamatan,
            "current": unit.snapshot,
        },
        "fields": {
            "unit": UNIT_FIELDS,
            "pengelola": PENGELOLA_FIELDS,
        },
    })
}

/// `GET /api/public/spam-kelembagaan/form/{token}`: `publicShow`.
pub async fn spam_kelembagaan_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let key = format!("spam-kelembagaan-show:{}", client_ip(&headers));
    if let Err(retry) = state
        .limiter
        .hit(&key, SHOW_MAX_PER_MINUTE, Duration::from_secs(60))
    {
        return Ok(too_many(retry));
    }

    let now = Utc::now();
    let link = find_link(&state.pool, &token)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let unit = load_unit(&state.pool, link.unit_spam_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let data = form_data(&link, &unit, now);

    if !link.is_usable(now) {
        return Ok((
            StatusCode::GONE,
            Json(json!({ "success": false, "message": LINK_UNUSABLE, "data": data })),
        )
            .into_response());
    }
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `POST /api/public/spam-kelembagaan/form/{token}`: `publicSubmit`.
/// Urutan cek mengikuti Laravel: link (404), validasi (422), `isUsable` (422 `token`),
/// payload kosong (422 `payload`), lalu penyimpanan dengan kenaikan `submission_count`.
pub async fn spam_kelembagaan_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let key = format!("spam-kelembagaan-submit:{}", client_ip(&headers));
    if let Err(retry) = state
        .limiter
        .hit(&key, SUBMIT_MAX_PER_MINUTE, Duration::from_secs(60))
    {
        return Ok(too_many(retry));
    }

    let link = find_link(&state.pool, &token)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let input = parse_body(&body);
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // `payload`: required|array. Array kosong dihitung tidak ada, seperti `required` di Laravel.
    let payload: Option<Map<String, Value>> = match input.get("payload") {
        None | Some(Value::Null) => {
            add(&mut errs, "payload", "The payload field is required.");
            None
        }
        Some(Value::String(s)) if s.trim().is_empty() => {
            add(&mut errs, "payload", "The payload field is required.");
            None
        }
        Some(Value::Object(m)) if m.is_empty() => {
            add(&mut errs, "payload", "The payload field is required.");
            None
        }
        Some(Value::Array(a)) if a.is_empty() => {
            add(&mut errs, "payload", "The payload field is required.");
            None
        }
        Some(Value::Object(m)) => Some(m.clone()),
        // Daftar tanpa nama field tidak menyumbang apa pun ke payload.
        Some(Value::Array(_)) => Some(Map::new()),
        Some(_) => {
            add(&mut errs, "payload", "The payload field must be an array.");
            None
        }
    };

    let submitter_name = required_string(&input, "submitter_name", 255, &mut errs);
    let submitter_phone = nullable_string(&input, "submitter_phone", 50, &mut errs).flatten();
    let submitter_instansi =
        nullable_string(&input, "submitter_instansi", 255, &mut errs).flatten();
    let submitter_note = nullable_string(&input, "submitter_note", 2000, &mut errs).flatten();

    let mut flat: Vec<(&str, Option<String>)> = Vec::new();
    for (field, max) in FLAT_FIELDS {
        if let Some(v) = nullable_string(&input, field, max, &mut errs) {
            flat.push((field, v));
        }
    }

    if !errs.is_empty() {
        return Err(validation(errs));
    }
    let (Some(submitter_name), Some(mut merged)) = (submitter_name, payload) else {
        return Err(internal("validasi form publik tidak lengkap"));
    };
    // `array_merge($validated['payload'], only(UNIT + PENGELOLA))`: field datar menimpa payload.
    for (field, v) in flat {
        merged.insert(field.to_string(), v.map_or(Value::Null, Value::String));
    }

    // `createSubmission`: `isUsable` dulu, lalu payload kosong.
    let now = Utc::now();
    if !link.is_usable(now) {
        return Err(validation(single("token", LINK_UNUSABLE)));
    }
    let clean = sanitize(&merged);
    if clean.is_empty() {
        return Err(validation(single("payload", EMPTY_PAYLOAD)));
    }

    let unit = load_unit(&state.pool, link.unit_spam_id)
        .await?
        .ok_or_else(ApiError::not_found)?;

    let ip = audit::client_info(&headers).0;
    let user_agent: String = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(500)
        .collect();

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO spam_kelembagaan_submissions \
         (share_link_id, unit_spam_id, payload, snapshot_before, submitter_name, submitter_phone, \
          submitter_instansi, submitter_note, status, submitter_ip, user_agent, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, NOW(), NOW())",
    )
    .bind(link.id)
    .bind(unit.id)
    .bind(Value::Object(clean).to_string())
    .bind(Value::Object(unit.snapshot.clone()).to_string())
    .bind(&submitter_name)
    .bind(submitter_phone)
    .bind(submitter_instansi)
    .bind(submitter_note)
    .bind(ip)
    .bind(user_agent)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let submission_id = res.last_insert_id();

    // `$link->increment('submission_count')` tidak mengubah `updated_at`.
    sqlx::query("UPDATE spam_kelembagaan_share_links SET submission_count = submission_count + 1 WHERE id = ?")
        .bind(link.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    let created_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT created_at FROM spam_kelembagaan_submissions WHERE id = ?")
            .bind(submission_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": SUBMIT_OK,
            "data": {
                "id": submission_id,
                "status": "pending",
                "created_at": created_at.map(iso8601),
            },
        })),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Hubungi kami
// ---------------------------------------------------------------------------

fn contact_error(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "status": "error", "message": message })),
    )
        .into_response()
}

/// `ContactInquiryService::resolveRecipientEmail`: `contact_email`, lalu `mail_from_address`,
/// lalu `mail_username`. Yang pertama valid dipakai.
async fn resolve_recipient(pool: &MySqlPool) -> Result<Option<String>, ApiError> {
    for key in ["contact_email", "mail_from_address", "mail_username"] {
        let raw = mailer::setting(pool, key)
            .await
            .map_err(internal)?
            .unwrap_or_default();
        let candidate = raw.trim().to_lowercase();
        if !candidate.is_empty() && looks_like_email(&candidate) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// `ContactInquiryService::buildMailContent`: HTML dan teks biasa.
fn contact_mail(
    brand: &Brand,
    name: &str,
    email: &str,
    phone: &str,
    subject: &str,
    message: &str,
) -> (String, String) {
    let safe_subject = mail_layout::esc(subject);
    let safe_email = mail_layout::esc(email);
    let mut contact_lines = format!(
        "<strong>Nama:</strong> {}<br><strong>Email:</strong> <a href=\"mailto:{safe_email}\">{safe_email}</a>",
        mail_layout::esc(name)
    );
    if !phone.is_empty() {
        contact_lines.push_str(&format!(
            "<br><strong>Telepon:</strong> {}",
            mail_layout::esc(phone)
        ));
    }
    contact_lines.push_str(&format!("<br><strong>Subjek:</strong> {safe_subject}"));

    let inner = format!(
        "{}{}{}{}",
        mail_layout::heading(
            brand,
            "Pesan Hubungi Kami",
            Some(&format!("Formulir landing page {safe_subject}")),
            false,
        ),
        mail_layout::info_box(brand, &contact_lines),
        mail_layout::paragraph(brand, "Pesan:"),
        mail_layout::message_block(brand, message),
    );
    let html = mail_layout::wrap_document(brand, &inner, Some(message));

    let mut lines: Vec<String> = vec![
        "Pesan Hubungi Kami".to_string(),
        format!("Nama: {name}"),
        format!("Email: {email}"),
    ];
    if !phone.is_empty() {
        lines.push(format!("Telepon: {phone}"));
    }
    lines.push(format!("Subjek: {subject}"));
    lines.push(message.to_string());
    // `array_filter` membuang string kosong dan "0" (falsy di PHP).
    lines.retain(|l| !l.is_empty() && l != "0");
    let text = mail_layout::wrap_plain_document(brand, &lines.join("\n"));
    (html, text)
}

/// Mengirim email multipart (teks dan HTML) dengan `Reply-To` ke pengirim formulir.
/// `mailer::send` belum mendukung `Reply-To`, jadi pesan dibuat di sini dengan transport yang sama.
async fn send_contact_mail(
    settings: &SmtpSettings,
    to: &str,
    subject: &str,
    text: &str,
    html: &str,
    reply_name: &str,
    reply_email: &str,
) -> Result<(), String> {
    let from_address: Address = settings
        .from_address
        .parse()
        .map_err(|e| format!("alamat pengirim tidak valid: {e}"))?;
    let to_address: Address = to
        .parse()
        .map_err(|e| format!("alamat penerima tidak valid: {e}"))?;
    let reply_address: Address = reply_email
        .parse()
        .map_err(|e| format!("alamat balasan tidak valid: {e}"))?;
    let reply_to = Mailbox::new(
        (!reply_name.is_empty()).then(|| reply_name.to_string()),
        reply_address,
    );
    let message = Message::builder()
        .from(Mailbox::new(Some(settings.from_name.clone()), from_address))
        .reply_to(reply_to)
        .to(Mailbox::new(None, to_address))
        .subject(subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text.to_string()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html.to_string()),
                ),
        )
        .map_err(|e| e.to_string())?;
    let transport = mailer::transport(settings)?;
    transport
        .send(message)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// `POST /api/public/contact`: `ContactController@store`.
pub async fn contact(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let input = parse_body(&body);

    // `throttle:contact-inquiries`: dicek sebelum validasi dan honeypot, seperti middleware Laravel.
    let ip = client_ip(&headers);
    let email_for_limit = input
        .get("email")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let email_key = if email_for_limit.is_empty() {
        format!("ip:{ip}")
    } else {
        format!("email:{email_for_limit}")
    };
    let limits = [
        (format!("contact-minute:{ip}"), 3, Duration::from_secs(60)),
        (format!("contact-hour:{ip}"), 10, Duration::from_secs(3600)),
        (
            format!("contact-email:{email_key}"),
            5,
            Duration::from_secs(3600),
        ),
    ];
    for (key, max, window) in limits {
        if let Err(retry) = state.limiter.hit(&key, max, window) {
            return Ok(too_many(retry));
        }
    }

    // Honeypot: pura-pura berhasil, tanpa mengirim email.
    if is_filled(input.get("website")) {
        return Ok((
            StatusCode::OK,
            Json(json!({ "status": "success", "message": CONTACT_OK })),
        )
            .into_response());
    }

    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let name = required_string(&input, "name", 120, &mut errs);
    let email = required_email(&input, "email", 255, &mut errs);
    let phone = nullable_string(&input, "phone", 30, &mut errs).flatten();
    let subject = required_string(&input, "subject", 200, &mut errs);
    let message = required_string(&input, "message", 5000, &mut errs);
    if !errs.is_empty() {
        return Err(validation(errs));
    }
    let (Some(name), Some(email), Some(subject), Some(message)) = (name, email, subject, message)
    else {
        return Err(internal("validasi hubungi kami tidak lengkap"));
    };
    let phone = phone.unwrap_or_default();

    // `ContactInquiryService::send`.
    let Some(settings) = mailer::load_settings(&state.pool).await.map_err(internal)? else {
        return Ok(contact_error(CONTACT_MAIL_DISABLED));
    };
    let Some(recipient) = resolve_recipient(&state.pool).await? else {
        return Ok(contact_error(CONTACT_NO_RECIPIENT));
    };
    let app_name = mail_layout::get_setting_str(&state.pool, "app_name", "Arumanis").await?;

    let url = format!("{}/api/public/contact", state.app_url.trim_end_matches('/'));
    let ctx = WriteCtx {
        actor: 0,
        url: &url,
        headers: &headers,
    };
    let brand = mail_layout::brand(&state.pool, &state.app_url, &ctx).await?;

    let subject_line = format!("[Hubungi Kami] {subject} — {app_name}");
    let (html, text) = contact_mail(&brand, &name, &email, &phone, &subject, &message);

    match send_contact_mail(
        &settings,
        &recipient,
        &subject_line,
        &text,
        &html,
        &name,
        &email,
    )
    .await
    {
        Ok(()) => Ok((
            StatusCode::OK,
            Json(json!({ "status": "success", "message": CONTACT_OK })),
        )
            .into_response()),
        Err(e) => {
            tracing::warn!(recipient = %recipient, sender_email = %email, error = %e, "Gagal mengirim pesan hubungi kami");
            Ok(contact_error(CONTACT_SEND_FAILED))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_matches_laravel_filled() {
        assert!(!is_filled(None));
        assert!(!is_filled(Some(&Value::Null)));
        assert!(!is_filled(Some(&json!("   "))));
        assert!(is_filled(Some(&json!("bot"))));
    }

    #[test]
    fn sanitize_keeps_only_known_fields_in_order() {
        let input = json!({ "name": "  Sumur  ", "kepala": "", "lainnya": 1 });
        let clean = sanitize(input.as_object().unwrap());
        assert_eq!(clean.len(), 2);
        assert_eq!(clean["name"], json!("Sumur"));
        assert_eq!(clean["kepala"], Value::Null);
    }

    #[test]
    fn iso8601_matches_carbon_format() {
        let t = DateTime::parse_from_rfc3339("2026-10-09T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso8601(t), "2026-10-09T10:00:00+00:00");
    }
}
