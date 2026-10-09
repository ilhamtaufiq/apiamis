//! Rute uji email `AppSettingController` (semua admin):
//! - `POST /api/app-settings/test-mail-connection` (`testMailConnection`)
//! - `GET` dan `POST /api/app-settings/mail-templates` (`mailTemplates`, `storeMailTemplates`)
//! - `POST /api/app-settings/mail-templates/{key}/test` (`testMailTemplate`)
//!
//! Pengiriman lewat trait `MailSender`. Produksi memakai `SmtpSender` (`mailer::send`). Tes memakai stub
//! yang mencatat penerima dan subjek, sehingga tidak ada email sungguhan yang dikirim.
//!
//! Perbedaan dengan Laravel:
//! - Body `POST mail-templates` hanya JSON. Bentuk `templates[key][field]` lewat formulir tidak didukung.
//! - Field formulir dan JSON datar dibaca sebagai teks (konvensi `read_input`). Angka JSON pada
//!   `mail_port` diterima, sedangkan Laravel menolak non-string.
//! - Markdown: CommonMark + tabel, coretan, dan daftar tugas (`pulldown-cmark`). HTML mentah diteruskan.
//!   Perilaku `Str::markdown` Laravel belum diverifikasi terhadap vendor.
//! - `strip_tags` dan decode entitas hanya mencakup kasus umum (bagian teks email).
//! - Brand: lihat `app_settings_mail::brand_color` untuk perbedaan ekstraksi logo raster.

mod brand_color;
mod mail_layout;
mod mail_templates;

use std::{collections::BTreeMap, future::Future, pin::Pin};

use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use pulldown_cmark::{html, Options, Parser};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySqlPool;

pub use mail_layout::WriteCtx;

use crate::{
    app_settings,
    mailer::{self, SmtpSettings},
    notifications::require_admin,
    require_auth, AppState,
};
use mail_layout::{get_setting, php_trim, Brand};

const JSON_LIMIT: usize = 4 * 1024 * 1024;
const SMTP_BELUM: &str = "SMTP belum lengkap. Isi host, username Gmail, dan App Password lalu simpan atau kirim saat uji koneksi.";
const FORMATS: &[&str] = &["plain", "markdown", "html"];

// ---------------------------------------------------------------------------
// Pengiriman
// ---------------------------------------------------------------------------

/// Pengirim email. Produksi: `SmtpSender`. Tes: stub yang mencatat panggilan.
pub trait MailSender: Sync {
    fn send<'a>(
        &'a self,
        settings: &'a SmtpSettings,
        to: &'a str,
        subject: &'a str,
        text: &'a str,
        html: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

/// Pengirim SMTP sungguhan (`mailer::send`).
pub struct SmtpSender;

impl MailSender for SmtpSender {
    fn send<'a>(
        &'a self,
        settings: &'a SmtpSettings,
        to: &'a str,
        subject: &'a str,
        text: &'a str,
        html: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { mailer::send(settings, to, None, subject, text, html).await })
    }
}

/// `MailContentService::render`: HTML (bila ada) dan teks.
struct Rendered {
    html: Option<String>,
    text: String,
}

fn markdown_html(src: &str) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(src, opts));
    out
}

fn render(brand: &Brand, body: &str, format: &str) -> Rendered {
    let normalized = php_trim(body);
    let preheader = php_trim(&mail_layout::strip_tags(normalized)).to_string();
    match format {
        "html" => Rendered {
            html: Some(mail_layout::wrap_document(
                brand,
                normalized,
                Some(&preheader),
            )),
            text: php_trim(&mail_layout::entity_decode(&mail_layout::strip_tags(
                normalized,
            )))
            .to_string(),
        },
        "markdown" => {
            let inner = markdown_html(normalized);
            Rendered {
                html: Some(mail_layout::wrap_document(brand, &inner, Some(&preheader))),
                text: php_trim(&mail_layout::entity_decode(&mail_layout::strip_tags(
                    &inner,
                )))
                .to_string(),
            }
        }
        _ => Rendered {
            html: None,
            text: mail_layout::wrap_plain_document(brand, normalized),
        },
    }
}

/// `MailContentService::sendRendered` (tanpa nama penerima).
async fn send_rendered(
    brand: &Brand,
    sender: &dyn MailSender,
    settings: &SmtpSettings,
    to: &str,
    subject: &str,
    body: &str,
    format: &str,
) -> Result<(), String> {
    let rendered = render(brand, body, format);
    let to = to.trim().to_ascii_lowercase();
    sender
        .send(
            settings,
            &to,
            subject,
            &rendered.text,
            rendered.html.as_deref(),
        )
        .await
}

/// `MailConfigService::applyFromSettings` dengan override dari permintaan. `None` bila email nonaktif
/// atau host, username, atau password kosong.
async fn mail_settings(
    pool: &MySqlPool,
    ov: &BTreeMap<String, String>,
) -> Result<Option<SmtpSettings>, ApiError> {
    let enabled = match ov.get("mail_enabled") {
        Some(v) => v.clone(),
        None => get_setting(pool, "mail_enabled", Some("0"))
            .await?
            .unwrap_or_default(),
    };
    if enabled != "1" {
        return Ok(None);
    }

    let stored = |key: &'static str, default: &'static str| async move {
        get_setting(pool, key, Some(default))
            .await
            .map(|v| v.unwrap_or_default())
    };
    let host = match ov.get("mail_host") {
        Some(v) => v.clone(),
        None => stored("mail_host", "smtp.gmail.com").await?,
    };
    let port = match ov.get("mail_port") {
        Some(v) => php_int(v),
        None => php_int(&stored("mail_port", "587").await?),
    };
    let encryption = match ov.get("mail_encryption") {
        Some(v) => v.clone(),
        None => stored("mail_encryption", "tls").await?,
    };
    let username = match ov.get("mail_username") {
        Some(v) => v.clone(),
        None => stored("mail_username", "").await?,
    };
    let password = match ov.get("mail_password") {
        Some(v) => v.clone(),
        None => stored("mail_password", "").await?,
    };
    let from_address = match ov.get("mail_from_address") {
        Some(v) => v.clone(),
        None => get_setting(pool, "mail_from_address", Some(&username))
            .await?
            .unwrap_or_default(),
    };
    let app_name = get_setting(pool, "app_name", Some("Arumanis")).await?;
    let from_name = match ov.get("mail_from_name") {
        Some(v) => v.clone(),
        None => get_setting(pool, "mail_from_name", app_name.as_deref())
            .await?
            .unwrap_or_default(),
    };

    let host = host.trim().to_string();
    if host.is_empty() || username.is_empty() || password.is_empty() {
        return Ok(None);
    }
    let default_port = if encryption == "ssl" { 465 } else { 587 };
    let port = if port > 0 { port } else { default_port };
    let from_address = if from_address.is_empty() {
        username.clone()
    } else {
        from_address
    };
    let from_name = if from_name.trim().is_empty() {
        "Arumanis".to_string()
    } else {
        from_name
    };
    Ok(Some(SmtpSettings {
        host,
        port: u16::try_from(port).unwrap_or(0),
        encryption,
        username,
        password,
        from_address,
        from_name,
    }))
}

/// `(int)` PHP untuk string: spasi, tanda, lalu digit. Selain itu 0.
fn php_int(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    n.parse::<i64>().map(|v| sign * v).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

type Errs = BTreeMap<String, Vec<String>>;

fn attr(key: &str) -> String {
    key.replace('_', " ")
}

fn add(errs: &mut Errs, key: &str, msg: String) {
    errs.entry(key.to_string()).or_default().push(msg);
}

fn validation(errs: Errs) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

fn text<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    match m.get(key) {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Aturan `string|max`. `attribute` dipakai sebagai kunci dan nama pada pesan.
fn rule_string(errs: &mut Errs, attribute: &str, v: Option<&Value>, max: usize) {
    match v {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                add(
                    errs,
                    attribute,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        attr(attribute)
                    ),
                );
            }
        }
        Some(_) => add(
            errs,
            attribute,
            format!("The {} field must be a string.", attr(attribute)),
        ),
    }
}

/// Aturan `in:...`.
fn rule_in(errs: &mut Errs, attribute: &str, v: Option<&Value>, allowed: &[&str]) {
    match v {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => {}
        Some(_) => add(
            errs,
            attribute,
            format!("The selected {} is invalid.", attr(attribute)),
        ),
    }
}

/// Aturan `email|max` (`required` bila diminta).
fn rule_email(errs: &mut Errs, attribute: &str, v: Option<&Value>, max: usize, required: bool) {
    match v {
        None | Some(Value::Null) => {
            if required {
                add(
                    errs,
                    attribute,
                    format!("The {} field is required.", attr(attribute)),
                );
            }
        }
        Some(Value::String(s)) => {
            if !app_settings::is_email(s) {
                add(
                    errs,
                    attribute,
                    format!(
                        "The {} field must be a valid email address.",
                        attr(attribute)
                    ),
                );
            } else if s.chars().count() > max {
                add(
                    errs,
                    attribute,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        attr(attribute)
                    ),
                );
            }
        }
        Some(_) => add(
            errs,
            attribute,
            format!(
                "The {} field must be a valid email address.",
                attr(attribute)
            ),
        ),
    }
}

fn check_string(errs: &mut Errs, m: &Map<String, Value>, key: &str, max: usize) {
    rule_string(errs, key, m.get(key), max);
}

fn check_in(errs: &mut Errs, m: &Map<String, Value>, key: &str, allowed: &[&str]) {
    rule_in(errs, key, m.get(key), allowed);
}

fn check_email(errs: &mut Errs, m: &Map<String, Value>, key: &str, max: usize, required: bool) {
    rule_email(errs, key, m.get(key), max, required);
}

/// Field teks opsional yang tidak kosong; `None` bila tidak ada.
fn opt(m: &Map<String, Value>, key: &str) -> Option<String> {
    text(m, key).map(str::to_string)
}

/// `$request->input($a) ?? $request->input($b)`.
fn coalesce(m: &Map<String, Value>, a: &str, b: &str) -> Option<String> {
    opt(m, a).or_else(|| opt(m, b))
}

fn normalize_json(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let t = php_trim(&s);
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        Value::Array(a) => Value::Array(a.into_iter().map(normalize_json).collect()),
        Value::Object(m) => {
            Value::Object(m.into_iter().map(|(k, v)| (k, normalize_json(v))).collect())
        }
        other => other,
    }
}

async fn read_json(request: Request) -> Result<Value, ApiError> {
    let is_json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json"));
    if !is_json {
        return Ok(Value::Null);
    }
    let bytes = axum::body::to_bytes(request.into_body(), JSON_LIMIT)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("Permintaan tidak valid: {e}"),
            )
        })?;
    Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Uji koneksi dan uji template (inti, dengan `MailSender` yang bisa diganti)
// ---------------------------------------------------------------------------

/// Kirim email uji dengan override dari permintaan. Mengembalikan status dan body JSON seperti Laravel.
async fn send_test_mail(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    to: &str,
    ov: &BTreeMap<String, String>,
    used_fresh_password: bool,
) -> Result<(StatusCode, Value), ApiError> {
    let Some(settings) = mail_settings(pool, ov).await? else {
        return Ok((
            StatusCode::BAD_REQUEST,
            json!({ "ok": false, "error": SMTP_BELUM }),
        ));
    };
    let brand = mail_layout::brand(pool, app_url, ctx).await?;
    let template_key = ov
        .get("template_key")
        .cloned()
        .unwrap_or_else(|| "smtp_test".to_string());
    let content = resolve_test_content(pool, &brand, ov).await?;

    match send_rendered(
        &brand,
        sender,
        &settings,
        to,
        &content.subject,
        &content.body,
        &content.format,
    )
    .await
    {
        Ok(()) => Ok((
            StatusCode::OK,
            json!({
                "ok": true,
                "to": to,
                "format": content.format,
                "template_key": template_key,
                "used_stored_password": !used_fresh_password,
            }),
        )),
        Err(e) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "ok": false,
                "error": format!("Gagal mengirim email: {e}"),
                "used_stored_password": !used_fresh_password,
            }),
        )),
    }
}

/// `MailContentService::resolveTestContent`: format, subjek, dan isi dari `format|mail_body_format` dan seterusnya.
async fn resolve_test_content(
    pool: &MySqlPool,
    brand: &Brand,
    ov: &BTreeMap<String, String>,
) -> Result<mail_templates::Resolved, ApiError> {
    let key = ov.get("template_key").map_or("smtp_test", String::as_str);
    let pick = |a: &str, b: &str| {
        ov.get(a)
            .or_else(|| ov.get(b))
            .filter(|s| !s.is_empty())
            .cloned()
    };
    let mut tov = BTreeMap::new();
    for (k, v) in [
        ("format", pick("format", "mail_body_format")),
        ("subject", pick("subject", "mail_subject")),
        ("body", pick("body", "mail_body")),
    ] {
        if let Some(v) = v {
            tov.insert(k.to_string(), v);
        }
    }
    mail_templates::resolve(pool, brand, key, &tov).await
}

/// `POST test-mail-connection` tanpa HTTP: validasi, override, lalu kirim lewat `sender`.
pub async fn test_mail_connection_with(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    fields: &Map<String, Value>,
) -> Result<(StatusCode, Value), ApiError> {
    let mut errs = Errs::new();
    check_email(&mut errs, fields, "to", 255, true);
    check_string(&mut errs, fields, "template_key", 64);
    check_in(&mut errs, fields, "mail_enabled", &["0", "1"]);
    check_string(&mut errs, fields, "mail_host", 255);
    check_string(&mut errs, fields, "mail_port", 5);
    check_in(
        &mut errs,
        fields,
        "mail_encryption",
        &["tls", "ssl", "none"],
    );
    check_email(&mut errs, fields, "mail_username", 255, false);
    check_string(&mut errs, fields, "mail_password", 2000);
    check_email(&mut errs, fields, "mail_from_address", 255, false);
    check_string(&mut errs, fields, "mail_from_name", 255);
    check_in(&mut errs, fields, "mail_body_format", FORMATS);
    check_string(&mut errs, fields, "mail_subject", 255);
    check_string(&mut errs, fields, "mail_body", 50000);
    check_in(&mut errs, fields, "format", FORMATS);
    check_string(&mut errs, fields, "subject", 255);
    check_string(&mut errs, fields, "body", 50000);
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    let to = opt(fields, "to").unwrap_or_default();
    let mut ov = BTreeMap::new();
    ov.insert(
        "template_key".to_string(),
        opt(fields, "template_key").unwrap_or_else(|| "smtp_test".to_string()),
    );
    // `$overrides['mail_enabled'] = '1'`: uji selalu mencoba mengirim.
    ov.insert("mail_enabled".to_string(), "1".to_string());
    for key in [
        "mail_host",
        "mail_port",
        "mail_encryption",
        "mail_username",
        "mail_from_address",
        "mail_from_name",
    ] {
        if let Some(v) = opt(fields, key) {
            ov.insert(key.to_string(), v);
        }
    }
    let optional = [
        (
            "mail_body_format",
            coalesce(fields, "mail_body_format", "format"),
        ),
        ("mail_subject", coalesce(fields, "mail_subject", "subject")),
        ("mail_body", coalesce(fields, "mail_body", "body")),
        ("mail_password", opt(fields, "mail_password")),
    ];
    for (key, value) in optional {
        if let Some(v) = value {
            ov.insert(key.to_string(), v);
        }
    }
    let used_fresh = ov.contains_key("mail_password");
    send_test_mail(pool, app_url, ctx, sender, &to, &ov, used_fresh).await
}

/// `POST mail-templates/{key}/test` tanpa HTTP.
pub async fn test_mail_template_with(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    sender: &dyn MailSender,
    key: &str,
    fields: &Map<String, Value>,
) -> Result<(StatusCode, Value), ApiError> {
    if !mail_templates::is_valid_key(key) {
        return Ok((
            StatusCode::NOT_FOUND,
            json!({ "ok": false, "error": "Template email tidak dikenal." }),
        ));
    }
    let mut errs = Errs::new();
    check_email(&mut errs, fields, "to", 255, true);
    check_in(&mut errs, fields, "format", FORMATS);
    check_string(&mut errs, fields, "subject", 255);
    check_string(&mut errs, fields, "body", 50000);
    check_string(&mut errs, fields, "mail_password", 2000);
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    let to = opt(fields, "to").unwrap_or_default();
    let mut ov = BTreeMap::new();
    ov.insert("template_key".to_string(), key.to_string());
    for k in ["format", "subject", "body", "mail_password"] {
        if let Some(v) = opt(fields, k) {
            ov.insert(k.to_string(), v);
        }
    }
    let used_fresh = ov.contains_key("mail_password");
    send_test_mail(pool, app_url, ctx, sender, &to, &ov, used_fresh).await
}

/// `POST mail-templates` tanpa HTTP: `saveMany` lalu katalog baru.
pub async fn store_mail_templates_with(
    pool: &MySqlPool,
    app_url: &str,
    ctx: &WriteCtx<'_>,
    templates: &Value,
) -> Result<Value, ApiError> {
    let mut errs = Errs::new();
    let entries: Vec<(String, Value)> = match templates {
        Value::Null => {
            add(
                &mut errs,
                "templates",
                "The templates field is required.".to_string(),
            );
            Vec::new()
        }
        Value::Object(m) if m.is_empty() => {
            add(
                &mut errs,
                "templates",
                "The templates field is required.".to_string(),
            );
            Vec::new()
        }
        Value::Array(a) if a.is_empty() => {
            add(
                &mut errs,
                "templates",
                "The templates field is required.".to_string(),
            );
            Vec::new()
        }
        Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Value::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        _ => {
            add(
                &mut errs,
                "templates",
                "The templates must be an array.".to_string(),
            );
            Vec::new()
        }
    };
    for (key, payload) in &entries {
        if let Value::Object(m) = payload {
            let name = |f: &str| format!("templates.{key}.{f}");
            rule_in(&mut errs, &name("format"), m.get("format"), FORMATS);
            rule_string(&mut errs, &name("subject"), m.get("subject"), 255);
            rule_string(&mut errs, &name("body"), m.get("body"), 50000);
        }
    }
    if !errs.is_empty() {
        return Err(validation(errs));
    }

    let map: Map<String, Value> = match templates {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    let brand = mail_layout::brand(pool, app_url, ctx).await?;
    mail_templates::save_many(pool, ctx, &brand, &map).await?;
    let entries = mail_templates::entries(pool, &brand).await?;
    Ok(json!({
        "data": mail_templates::catalog_json(&brand, &entries),
        "message": "Template email berhasil disimpan.",
    }))
}

// ---------------------------------------------------------------------------
// Handler HTTP
// ---------------------------------------------------------------------------

fn base(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

fn respond(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// `GET /api/app-settings/mail-templates`: admin.
pub async fn mail_templates_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let url = format!("{}/api/app-settings/mail-templates", base(&state));
    let ctx = WriteCtx {
        actor: user.user_id,
        url: &url,
        headers: &headers,
    };
    let brand = mail_layout::brand(&state.pool, &state.app_url, &ctx).await?;
    let entries = mail_templates::entries(&state.pool, &brand).await?;
    Ok(Json(
        json!({ "data": mail_templates::catalog_json(&brand, &entries) }),
    ))
}

/// `POST /api/app-settings/mail-templates`: admin. Body JSON `{"templates": {key: {format, subject, body}}}`.
pub async fn mail_templates_store(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let raw = read_json(request).await?;
    let raw = normalize_json(raw);
    let templates = match &raw {
        Value::Object(m) => m.get("templates").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let url = format!("{}/api/app-settings/mail-templates", base(&state));
    let ctx = WriteCtx {
        actor: user.user_id,
        url: &url,
        headers: &headers,
    };
    let body = store_mail_templates_with(&state.pool, &state.app_url, &ctx, &templates).await?;
    Ok(respond(StatusCode::OK, body))
}

/// `POST /api/app-settings/mail-templates/{key}/test`: admin.
pub async fn mail_template_test(
    State(state): State<AppState>,
    Path(key): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let fields = app_settings::flat_input(&state, request).await?;
    let url = format!(
        "{}/api/app-settings/mail-templates/{key}/test",
        base(&state)
    );
    let ctx = WriteCtx {
        actor: user.user_id,
        url: &url,
        headers: &headers,
    };
    let (status, body) = test_mail_template_with(
        &state.pool,
        &state.app_url,
        &ctx,
        &SmtpSender,
        &key,
        &fields,
    )
    .await?;
    Ok(respond(status, body))
}

/// `POST /api/app-settings/test-mail-connection`: admin.
pub async fn test_mail_connection(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let fields = app_settings::flat_input(&state, request).await?;
    let url = format!("{}/api/app-settings/test-mail-connection", base(&state));
    let ctx = WriteCtx {
        actor: user.user_id,
        url: &url,
        headers: &headers,
    };
    let (status, body) =
        test_mail_connection_with(&state.pool, &state.app_url, &ctx, &SmtpSender, &fields).await?;
    Ok(respond(status, body))
}
