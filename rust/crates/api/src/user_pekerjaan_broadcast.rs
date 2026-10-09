//! Pengingat kelengkapan data: `POST /api/user-pekerjaan/broadcast-reminders`. Setara
//! `UserPekerjaanController::broadcastReminders` dengan bagian `UserPekerjaanCompletenessService`
//! (`buildReminderMessage`, `pengawasActionUrl`, `sendReminderEmail`).
//!
//! Alur per penerima (sama dengan Laravel):
//! 1. satu baris `broadcast_histories` (`type` = `single`, `recipient_count` = 1);
//! 2. satu notifikasi database dengan `broadcast_history_id` di `data`;
//! 3. bila `send_email`, email lewat `app_settings_mail::render_deliverable` (`broadcast`) dan `MailSender`.
//!
//! Hanya admin (403 untuk non-admin). Produksi memakai `SmtpSender`. Tes memakai stub `MailSender`.
//!
//! Berbeda dari Laravel:
//! - tidak ada transaksi, sama dengan Laravel: bila insert gagal di tengah, baris penerima sebelumnya tetap ada;
//! - nama penerima tidak dikirim ke header email, karena `MailSender` belum membawa nama;
//! - body formulir (`application/x-www-form-urlencoded`) dibaca seperti JSON. Multipart ditolak 415;
//! - JSON yang tidak valid dibalas 400.

use std::collections::BTreeMap;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    app_settings,
    app_settings_mail::{
        self, mail_layout, mail_templates, normalize_json, MailSender, SmtpSender, WriteCtx,
    },
    mailer::SmtpSettings,
    notifications::{self, NOTIFIABLE_TYPE, NOTIFICATION_TYPE},
    notify::new_uuid,
    php, require_auth, user_pekerjaan_gaps, AppState,
};

const JSON_LIMIT: usize = 4 * 1024 * 1024;
const DEFAULT_TITLE: &str = "Pengingat Kelengkapan Data Pekerjaan";
const DEFAULT_TYPE: &str = "warning";
const NOTIFICATION_TYPES: &[&str] = &["info", "success", "warning", "error"];
const GAP_NAMES: &[&str] = &["foto", "penerima", "progress"];
const NO_USERS: &str = "Tidak ada pengawas dengan data belum lengkap untuk dikirimi pengingat";

type Errors = BTreeMap<String, Vec<String>>;

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("user-pekerjaan broadcast-reminders: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

fn add(errs: &mut Errors, field: &str, message: impl Into<String>) {
    errs.entry(field.to_string())
        .or_default()
        .push(message.into());
}

fn validation(errs: Errors) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

/// Pembacaan input seperti `$request->input()`: JSON atau formulir. String dipangkas dan kosong menjadi null
/// (middleware `TrimStrings` dan `ConvertEmptyStringsToNull`).
async fn read_input(request: Request) -> Result<Map<String, Value>, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.starts_with("multipart/form-data") {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Format multipart belum didukung di rute ini.",
        ));
    }
    let bytes = axum::body::to_bytes(request.into_body(), JSON_LIMIT)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("Permintaan tidak valid: {e}"),
            )
        })?;
    let raw = if content_type.starts_with("application/json") {
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "Permintaan tidak valid: JSON tidak terbaca",
                )
            })?
        }
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        form_to_value(&String::from_utf8_lossy(&bytes))
    } else {
        Value::Null
    };
    Ok(match normalize_json(raw) {
        Value::Object(m) => m,
        _ => Map::new(),
    })
}

/// Formulir ke objek JSON. Kunci `gaps[]` dan `user_ids[]` menjadi array, kunci lain skalar.
fn form_to_value(raw: &str) -> Value {
    let params = php::Params::parse(Some(raw));
    let mut m = Map::new();
    for key in [
        "title",
        "message_prefix",
        "notification_type",
        "tahun",
        "send_email",
    ] {
        if let Some(v) = params.get(key) {
            m.insert(key.to_string(), Value::String(v.to_string()));
        }
    }
    for key in ["gaps", "user_ids"] {
        let list = params.array(key);
        if !list.is_empty() {
            m.insert(
                key.to_string(),
                Value::Array(list.into_iter().map(Value::String).collect()),
            );
        } else if let Some(v) = params.get(key) {
            // Skalar untuk kunci array: validasi menolaknya dengan "must be an array".
            m.insert(key.to_string(), Value::String(v.to_string()));
        }
    }
    Value::Object(m)
}

/// Nilai JSON sebagai daftar. Objek (array asosiatif PHP) dibaca dari nilainya.
fn as_list(v: &Value) -> Option<Vec<Value>> {
    match v {
        Value::Array(a) => Some(a.clone()),
        Value::Object(m) => Some(m.values().cloned().collect()),
        _ => None,
    }
}

/// Bilangan bulat seperti rule `integer` (angka bulat atau string bilangan bulat).
fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
                .map(|f| f as i64)
        }),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// Hasil validasi bentuk input. Pengecekan `exists:users,id` dilakukan terpisah karena butuh database.
struct Parsed {
    gaps: Option<Vec<String>>,
    tahun: Option<i64>,
    /// Kandidat `user_ids`; `None` bila nilainya bukan bilangan bulat.
    user_ids: Vec<Option<i64>>,
    title: Option<String>,
    message_prefix: Option<String>,
    notification_type: String,
    send_email: bool,
}

fn parse_input(input: &Map<String, Value>, errs: &mut Errors) -> Parsed {
    // gaps: nullable|array, gaps.*: in:foto,penerima,progress
    let gaps = match input.get("gaps").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match as_list(v) {
            None => {
                add(errs, "gaps", "The gaps field must be an array.");
                None
            }
            Some(items) => {
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    match item.as_str() {
                        Some(g) if GAP_NAMES.contains(&g) => out.push(g.to_string()),
                        _ => add(
                            errs,
                            &format!("gaps.{i}"),
                            format!("The selected gaps.{i} is invalid."),
                        ),
                    }
                }
                Some(out)
            }
        },
    };

    // tahun: nullable|integer|min:2000|max:2100
    let tahun = match input.get("tahun").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match int_value(v) {
            None => {
                add(errs, "tahun", "The tahun field must be an integer.");
                None
            }
            Some(n) if n < 2000 => {
                add(errs, "tahun", "The tahun field must be at least 2000.");
                None
            }
            Some(n) if n > 2100 => {
                add(
                    errs,
                    "tahun",
                    "The tahun field must not be greater than 2100.",
                );
                None
            }
            Some(n) => Some(n),
        },
    };

    // user_ids: nullable|array, user_ids.*: exists:users,id
    let user_ids = match input.get("user_ids").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(v) => match as_list(v) {
            None => {
                add(errs, "user_ids", "The user ids field must be an array.");
                Vec::new()
            }
            Some(items) => items.iter().map(int_value).collect(),
        },
    };

    // title: nullable|string|max:255
    let title = string_field(input, errs, "title", 255);
    // message_prefix: nullable|string|max:1000
    let message_prefix = string_field(input, errs, "message_prefix", 1000);

    // notification_type: nullable|in:info,success,warning,error
    let notification_type = match input.get("notification_type").filter(|v| !v.is_null()) {
        None => DEFAULT_TYPE.to_string(),
        Some(Value::String(s)) if NOTIFICATION_TYPES.contains(&s.as_str()) => s.clone(),
        Some(_) => {
            add(
                errs,
                "notification_type",
                "The selected notification type is invalid.",
            );
            DEFAULT_TYPE.to_string()
        }
    };

    // send_email: nullable|boolean. Rule `boolean` menerima true, false, 1, 0, "1", "0" (tanpa longgar).
    let send_email = match input.get("send_email").filter(|v| !v.is_null()) {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) if n.as_i64() == Some(1) => true,
        Some(Value::Number(n)) if n.as_i64() == Some(0) => false,
        Some(Value::String(s)) if s == "1" => true,
        Some(Value::String(s)) if s == "0" => false,
        Some(_) => {
            add(
                errs,
                "send_email",
                "The send email field must be true or false.",
            );
            false
        }
    };

    Parsed {
        gaps,
        tahun,
        user_ids,
        title,
        message_prefix,
        notification_type,
        send_email,
    }
}

/// `nullable|string|max:N`. Mengembalikan nilai bila string dan lolos batas.
fn string_field(
    input: &Map<String, Value>,
    errs: &mut Errors,
    key: &str,
    max: usize,
) -> Option<String> {
    match input.get(key).filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                add(
                    errs,
                    key,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        key.replace('_', " ")
                    ),
                );
                None
            } else {
                Some(s.clone())
            }
        }
        Some(_) => {
            add(
                errs,
                key,
                format!("The {} field must be a string.", key.replace('_', " ")),
            );
            None
        }
    }
}

/// `buildReminderMessage`.
fn reminder_message(row: &Value, custom_prefix: Option<&str>) -> String {
    let user_name = row["user_name"].as_str().unwrap_or_default();
    let lines: Vec<String> = row["pekerjaan"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|p| {
                    let gaps: Vec<&str> = p["gaps"]
                        .as_array()
                        .map(|g| g.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    format!(
                        "• {} — belum lengkap: {}",
                        p["nama_paket"].as_str().unwrap_or_default(),
                        gaps.join(", ")
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let prefix = match custom_prefix.map(app_settings_mail::mail_layout::php_trim) {
        Some(p) if !p.is_empty() => format!("{p}\n\n"),
        _ => format!(
            "Halo {user_name},\n\nBeberapa pekerjaan yang ditugaskan kepada Anda masih belum lengkap:\n\n"
        ),
    };
    format!(
        "{prefix}{}\n\nSilakan lengkapi data di aplikasi pengawasan.",
        lines.join("\n")
    )
}

/// `pengawasActionUrl`: `pekerjaan/{id}` bila ada id (dan tidak nol), selain itu beranda pengawasan.
fn pengawas_action_url(base: &str, pekerjaan_id: Option<u64>) -> String {
    match pekerjaan_id.filter(|id| *id != 0) {
        Some(id) => mail_layout::pengawas_app(base, &format!("pekerjaan/{id}")),
        None => mail_layout::pengawas_app(base, "/"),
    }
}

/// Id pekerjaan pertama pada baris pengguna, seperti `$userRow['pekerjaan'][0]['pekerjaan_id']`.
fn first_pekerjaan_id(row: &Value) -> Option<u64> {
    row["pekerjaan"][0]["pekerjaan_id"].as_u64()
}

/// Hasil `sendReminderEmail`.
struct EmailOutcome {
    sent: bool,
    skipped_reason: Option<&'static str>,
    email: Option<String>,
}

/// `sendReminderEmail`. Kesalahan pengiriman atau render menjadi `send_failed`. Kesalahan database pada
/// pengecekan SMTP tidak ditangkap, sama dengan Laravel (`applyFromSettings` berada di luar `try`).
#[allow(clippy::too_many_arguments)]
async fn send_reminder_email(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    recipient_id: u64,
    recipient_email: &str,
    title: &str,
    message: &str,
    action_url: &str,
) -> Result<EmailOutcome, ApiError> {
    let email = recipient_email.trim().to_ascii_lowercase();
    if email.is_empty() || !app_settings::is_email(&email) {
        return Ok(EmailOutcome {
            sent: false,
            skipped_reason: Some("no_email"),
            email: None,
        });
    }

    let Some(settings) = app_settings_mail::mail_settings(pool, &BTreeMap::new()).await? else {
        return Ok(EmailOutcome {
            sent: false,
            skipped_reason: Some("smtp_disabled"),
            email: Some(email),
        });
    };

    match deliver(
        pool, app_url, ctx, sender, &settings, &email, title, message, action_url,
    )
    .await
    {
        Ok(()) => Ok(EmailOutcome {
            sent: true,
            skipped_reason: None,
            email: Some(email),
        }),
        Err(e) => {
            tracing::warn!(
                "Gagal mengirim email pengingat kelengkapan: user_id={recipient_id} email={email} error={}",
                e.message
            );
            Ok(EmailOutcome {
                sent: false,
                skipped_reason: Some("send_failed"),
                email: Some(email),
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn deliver(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    settings: &SmtpSettings,
    email: &str,
    title: &str,
    message: &str,
    action_url: &str,
) -> Result<(), ApiError> {
    let brand = mail_layout::brand(pool, app_url, ctx).await?;
    let content = mail_templates::render_deliverable(
        pool,
        &brand,
        "broadcast",
        &[
            ("title", title.to_string()),
            ("message", message.to_string()),
            ("action_url", action_url.to_string()),
        ],
    )
    .await?;
    app_settings_mail::send_rendered(
        &brand,
        sender,
        settings,
        email,
        &content.subject,
        &content.body,
        &content.format,
    )
    .await
    .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e))
}

/// Pemeriksaan `exists:users,id` untuk satu kandidat. Mengembalikan true bila ada.
async fn user_exists(pool: &MySqlPool, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
        .bind(id as u64)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// Pengguna penerima: `User::find`. Mengembalikan (nama, email) atau `None`.
async fn recipient(
    pool: &MySqlPool,
    id: u64,
) -> Result<Option<(Option<String>, Option<String>)>, ApiError> {
    let row = sqlx::query("SELECT name, email FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some((
            r.try_get("name").map_err(internal)?,
            r.try_get("email").map_err(internal)?,
        ))),
    }
}

/// Inti `broadcastReminders` tanpa HTTP. Admin diperiksa pemanggil. `sender` menerima pengiriman email.
pub async fn broadcast_reminders_with(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    input: &Map<String, Value>,
) -> Result<(StatusCode, Value), ApiError> {
    let mut errs = Errors::new();
    let parsed = parse_input(input, &mut errs);

    let mut user_ids: Vec<u64> = Vec::new();
    for (i, candidate) in parsed.user_ids.iter().enumerate() {
        let found = match candidate {
            Some(id) if *id > 0 => user_exists(pool, *id).await?,
            _ => false,
        };
        match candidate {
            Some(id) if found => user_ids.push(*id as u64),
            _ => add(
                &mut errs,
                &format!("user_ids.{i}"),
                format!("The selected user_ids.{i} is invalid."),
            ),
        }
    }
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    // `$service->analyze(gaps, tahun)`, lalu filter `user_ids` bila tidak kosong (`filled`).
    let analysis = user_pekerjaan_gaps::analyze(pool, parsed.gaps.as_deref(), parsed.tahun)
        .await
        .map_err(internal)?;
    let mut users: Vec<Value> = analysis["users"].as_array().cloned().unwrap_or_default();
    if !user_ids.is_empty() {
        users.retain(|row| {
            row["user_id"]
                .as_u64()
                .is_some_and(|id| user_ids.contains(&id))
        });
    }

    if users.is_empty() {
        return Ok((
            StatusCode::NOT_FOUND,
            json!({ "status": "error", "message": NO_USERS }),
        ));
    }

    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| DEFAULT_TITLE.to_string());
    let notification_type = parsed.notification_type.clone();
    let custom_prefix = parsed.message_prefix.clone();
    let base = mail_layout::frontend_base(pool, app_url).await?;

    let mut sent_count = 0u64;
    let mut email_sent = 0u64;
    let mut email_failed = 0u64;
    let mut email_skipped = 0u64;
    let mut smtp_unavailable = false;
    let mut email_recipients: Vec<String> = Vec::new();

    for row in &users {
        let user_id = row["user_id"].as_u64().unwrap_or_default();
        let Some((_name, email)) = recipient(pool, user_id).await? else {
            continue;
        };

        let message = reminder_message(row, custom_prefix.as_deref());
        let pekerjaan_id = first_pekerjaan_id(row).filter(|id| *id != 0);
        let url = match pekerjaan_id {
            Some(id) => format!("/pekerjaan/{id}"),
            None => "/pekerjaan".to_string(),
        };
        let action_url = pengawas_action_url(&base, pekerjaan_id);

        let history = sqlx::query(
            "INSERT INTO broadcast_histories (title, message, type, notification_type, url, is_banner, \
             recipient_count, created_at, updated_at) VALUES (?, ?, 'single', ?, ?, 0, 1, NOW(), NOW())",
        )
        .bind(&title)
        .bind(&message)
        .bind(&notification_type)
        .bind(&url)
        .execute(pool)
        .await
        .map_err(internal)?;
        let history_id = history.last_insert_id();

        // `AppNotification::toArray()` untuk kanal database.
        let data = json!({
            "title": title,
            "message": message,
            "url": url,
            "type": notification_type,
            "is_banner": false,
            "broadcast_history_id": history_id,
        })
        .to_string();
        sqlx::query(
            "INSERT INTO notifications (id, type, notifiable_type, notifiable_id, data, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
        )
        .bind(new_uuid())
        .bind(NOTIFICATION_TYPE)
        .bind(NOTIFIABLE_TYPE)
        .bind(user_id)
        .bind(&data)
        .execute(pool)
        .await
        .map_err(internal)?;

        sent_count += 1;

        if parsed.send_email {
            let outcome = send_reminder_email(
                pool,
                app_url,
                ctx,
                sender,
                user_id,
                email.as_deref().unwrap_or_default(),
                &title,
                &message,
                &action_url,
            )
            .await?;
            if outcome.sent {
                email_sent += 1;
                if let Some(e) = outcome.email.filter(|e| !e.is_empty()) {
                    email_recipients.push(e);
                }
            } else {
                match outcome.skipped_reason {
                    Some("smtp_disabled") => {
                        smtp_unavailable = true;
                        email_skipped += 1;
                    }
                    Some("send_failed") => email_failed += 1,
                    _ => email_skipped += 1,
                }
            }
        }
    }

    let mut message = "Pengingat kelengkapan berhasil dikirim".to_string();
    if parsed.send_email {
        if smtp_unavailable && email_sent == 0 {
            message.push_str(". Email tidak terkirim karena SMTP belum diaktifkan");
        } else if email_failed > 0 {
            message.push_str(&format!(
                ". Email terkirim ke {email_sent} pengawas, {email_failed} gagal"
            ));
        } else if email_sent > 0 {
            message.push_str(&format!(". Email terkirim ke {email_sent} pengawas"));
        }
    }

    let mut unique: Vec<String> = Vec::new();
    for e in email_recipients {
        if !unique.contains(&e) {
            unique.push(e);
        }
    }

    let action_url_sample = pengawas_action_url(&base, first_pekerjaan_id(&users[0]));

    Ok((
        StatusCode::OK,
        json!({
            "status": "success",
            "message": message,
            "recipient_count": sent_count,
            "email_sent_count": email_sent,
            "email_failed_count": email_failed,
            "email_skipped_count": email_skipped,
            "send_email": parsed.send_email,
            "smtp_unavailable": smtp_unavailable,
            "email_recipients": unique,
            "action_url_sample": action_url_sample,
        }),
    ))
}

/// `POST /api/user-pekerjaan/broadcast-reminders`: admin. 403 untuk non-admin, 404 bila tidak ada penerima.
pub async fn broadcast_reminders(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let headers: HeaderMap = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    notifications::require_admin(&state.pool, user.user_id).await?;
    let input = read_input(request).await?;
    let url = format!(
        "{}/api/user-pekerjaan/broadcast-reminders",
        state.app_url.trim_end_matches('/')
    );
    let ctx = WriteCtx {
        actor: user.user_id,
        url: &url,
        headers: &headers,
    };
    let (status, body) =
        broadcast_reminders_with(&state.pool, &state.app_url, &ctx, &SmtpSender, &input).await?;
    Ok((status, Json(body)).into_response())
}
