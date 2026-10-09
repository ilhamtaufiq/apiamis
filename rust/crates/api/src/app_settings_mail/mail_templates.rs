//! Katalog template email (`MailTemplateService`): daftar template, isi default, template tersimpan
//! (`mail_templates`), penggantian placeholder, dan penyimpanan.
//!
//! Isi default per format dicocokkan dengan PHP. Bagian HTML dibangun dari `mail_layout`.
//! Template legacy `smtp_test` (`mail_subject`, `mail_body`, `mail_body_format`) dibaca bila belum ada
//! entri `mail_templates` untuk kunci itu, seperti Laravel.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use super::mail_layout::{self, frontend_to, get_setting, pengawas_app, Brand, Tile, WriteCtx};
use crate::format;

pub const STORAGE_KEY: &str = "mail_templates";
const FORMAT_PLAIN: &str = "plain";
const FORMAT_MARKDOWN: &str = "markdown";
const FORMAT_HTML: &str = "html";

pub struct TemplateDef {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub placeholders: &'static [&'static str],
}

/// Daftar template, urutan sama dengan `MailTemplateService::TEMPLATES`. Semua default `html`.
pub const TEMPLATES: &[TemplateDef] = &[
    TemplateDef {
        key: "smtp_test",
        label: "Uji Koneksi SMTP",
        description: "Email uji saat mengkonfigurasi atau memverifikasi SMTP.",
        category: "sistem",
        placeholders: &["app_name"],
    },
    TemplateDef {
        key: "forgot_password",
        label: "Lupa Password",
        description: "Email reset password saat pengguna meminta pemulihan akses.",
        category: "autentikasi",
        placeholders: &[
            "user_name",
            "user_email",
            "reset_link",
            "expiry_minutes",
            "app_name",
        ],
    },
    TemplateDef {
        key: "welcome",
        label: "Selamat Datang",
        description: "Email sambutan untuk pengguna baru yang terdaftar.",
        category: "autentikasi",
        placeholders: &["user_name", "user_email", "login_url", "app_name"],
    },
    TemplateDef {
        key: "broadcast",
        label: "Notifikasi Broadcast",
        description: "Pengumuman massal ke banyak pengguna sekaligus.",
        category: "notifikasi",
        placeholders: &["title", "message", "action_url", "app_name"],
    },
    TemplateDef {
        key: "ticket_created",
        label: "Tiket Baru",
        description: "Notifikasi saat tiket bantuan baru dibuat.",
        category: "notifikasi",
        placeholders: &[
            "user_name",
            "ticket_id",
            "ticket_title",
            "ticket_url",
            "app_name",
        ],
    },
    TemplateDef {
        key: "ticket_updated",
        label: "Update Tiket",
        description: "Notifikasi perubahan status atau balasan tiket.",
        category: "notifikasi",
        placeholders: &[
            "user_name",
            "ticket_id",
            "ticket_title",
            "status",
            "ticket_url",
            "app_name",
        ],
    },
    TemplateDef {
        key: "task_assigned",
        label: "Penugasan Pekerjaan",
        description: "Pemberitahuan saat pengguna ditugaskan pada pekerjaan.",
        category: "operasional",
        placeholders: &["user_name", "pekerjaan_name", "pekerjaan_url", "app_name"],
    },
    TemplateDef {
        key: "data_reminder",
        label: "Pengingat Kelengkapan Data",
        description: "Reminder profil atau data pekerjaan yang belum lengkap.",
        category: "operasional",
        placeholders: &["user_name", "missing_fields", "profile_url", "app_name"],
    },
    TemplateDef {
        key: "contract_ready",
        label: "Dokumen Kontrak Siap",
        description: "Informasi dokumen kontrak yang siap diunduh atau ditinjau.",
        category: "operasional",
        placeholders: &["user_name", "kontrak_name", "download_url", "app_name"],
    },
    TemplateDef {
        key: "report_submitted",
        label: "Laporan Dikirim",
        description: "Konfirmasi laporan pekerjaan berhasil dikirim.",
        category: "operasional",
        placeholders: &["user_name", "report_name", "report_url", "app_name"],
    },
];

pub fn is_valid_key(key: &str) -> bool {
    TEMPLATES.iter().any(|t| t.key == key)
}

fn template_def(key: &str) -> Option<&'static TemplateDef> {
    TEMPLATES.iter().find(|t| t.key == key)
}

pub fn default_subject(key: &str) -> String {
    default_subject_arms(key)
        .unwrap_or(DEFAULT_SUBJECT_FALLBACK)
        .to_string()
}

/// `defaultBody($key, $format)`.
pub fn default_body(brand: &Brand, key: &str, format: &str) -> String {
    match format {
        FORMAT_HTML => html_body(brand, key),
        FORMAT_PLAIN => default_plain_arms(key)
            .unwrap_or(DEFAULT_PLAIN_FALLBACK)
            .to_string(),
        _ => default_markdown_arms(key)
            .unwrap_or(DEFAULT_MARKDOWN_FALLBACK)
            .to_string(),
    }
}

// ---------------------------------------------------------------------------
// Isi default (markdown dan plain dibangkitkan dari sumber PHP)
// ---------------------------------------------------------------------------

fn default_subject_arms(key: &str) -> Option<&'static str> {
    match key {
        "smtp_test" => Some("Uji Koneksi SMTP {{app_name}}"),
        "forgot_password" => Some("Reset Password — {{app_name}}"),
        "welcome" => Some("Selamat datang di {{app_name}}"),
        "broadcast" => Some("{{title}} — {{app_name}}"),
        "ticket_created" => Some("Tiket #{{ticket_id}} dibuat — {{app_name}}"),
        "ticket_updated" => Some("Update tiket #{{ticket_id}} — {{app_name}}"),
        "task_assigned" => Some("Penugasan: {{pekerjaan_name}}"),
        "data_reminder" => Some("Lengkapi data Anda — {{app_name}}"),
        "contract_ready" => Some("Dokumen {{kontrak_name}} siap"),
        "report_submitted" => Some("Laporan {{report_name}} terkirim"),
        _ => None,
    }
}

const DEFAULT_SUBJECT_FALLBACK: &str = "Notifikasi {{app_name}}";

fn default_markdown_arms(key: &str) -> Option<&'static str> {
    match key {
        "smtp_test" => Some(
            r#"## Uji Koneksi SMTP

Email ini dikirim dari **Pengaturan Aplikasi** untuk memverifikasi konfigurasi SMTP **{{app_name}}**.

- Host, port, dan kredensial sudah benar jika email ini sampai ke Anda.
- Header logo dan footer ditambahkan otomatis saat dikirim."#,
        ),
        "forgot_password" => Some(
            r#"## Reset Password

Halo **{{user_name}}**,

Kami menerima permintaan reset password untuk akun **{{user_email}}**.

[Buat password baru]({{reset_link}})

Tautan berlaku selama **{{expiry_minutes}} menit**. Jika Anda tidak meminta reset, abaikan email ini."#,
        ),
        "welcome" => Some(
            r#"## Selamat datang, {{user_name}}!

Akun Anda di **{{app_name}}** telah aktif.

- Email: {{user_email}}
- [Masuk ke aplikasi]({{login_url}})

Jika Anda tidak merasa mendaftar, hubungi administrator."#,
        ),
        "broadcast" => Some(
            r#"## {{title}}

{{message}}

[Buka aplikasi pengawasan]({{action_url}})"#,
        ),
        "ticket_created" => Some(
            r#"## Tiket #{{ticket_id}} dibuat

Halo **{{user_name}}**,

Tiket **{{ticket_title}}** telah dicatat di sistem.

[Buka tiket]({{ticket_url}})"#,
        ),
        "ticket_updated" => Some(
            r#"## Update tiket #{{ticket_id}}

Halo **{{user_name}}**,

Status tiket **{{ticket_title}}** berubah menjadi **{{status}}**.

[Lihat tiket]({{ticket_url}})"#,
        ),
        "task_assigned" => Some(
            r#"## Penugasan baru

Halo **{{user_name}}**,

Anda ditugaskan pada pekerjaan **{{pekerjaan_name}}**.

[Buka pekerjaan]({{pekerjaan_url}})"#,
        ),
        "data_reminder" => Some(
            r#"## Lengkapi data Anda

Halo **{{user_name}}**,

Data berikut belum lengkap: **{{missing_fields}}**.

[Perbarui profil]({{profile_url}})"#,
        ),
        "contract_ready" => Some(
            r#"## Dokumen siap

Halo **{{user_name}}**,

Dokumen **{{kontrak_name}}** sudah dapat diunduh.

[Unduh dokumen]({{download_url}})"#,
        ),
        "report_submitted" => Some(
            r#"## Laporan terkirim

Halo **{{user_name}}**,

Laporan **{{report_name}}** berhasil dikirim.

[Lihat laporan]({{report_url}})"#,
        ),
        _ => None,
    }
}

const DEFAULT_MARKDOWN_FALLBACK: &str = "Notifikasi dari **{{app_name}}**.";

fn default_plain_arms(key: &str) -> Option<&'static str> {
    match key {
        "smtp_test" => Some("Uji Koneksi SMTP\n\nEmail ini memverifikasi konfigurasi SMTP {{app_name}}.\n\nJika Anda menerima pesan ini, pengaturan email sudah benar."),
        "forgot_password" => Some("Reset Password\n\nHalo {{user_name}},\n\nKami menerima permintaan reset password untuk {{user_email}}.\n\nBuat password baru: {{reset_link}}\n\nTautan berlaku {{expiry_minutes}} menit. Abaikan email ini jika Anda tidak meminta reset."),
        "welcome" => Some("Selamat datang, {{user_name}}!\n\nAkun Anda di {{app_name}} telah aktif.\n\nEmail: {{user_email}}\nMasuk: {{login_url}}"),
        "broadcast" => Some("{{title}}\n\n{{message}}\n\nBuka aplikasi: {{action_url}}"),
        "ticket_created" => Some("Tiket #{{ticket_id}} dibuat\n\nHalo {{user_name}},\n\nTiket {{ticket_title}} telah dicatat.\n\nBuka: {{ticket_url}}"),
        "ticket_updated" => Some("Update tiket #{{ticket_id}}\n\nHalo {{user_name}},\n\nStatus {{ticket_title}}: {{status}}\n\nLihat: {{ticket_url}}"),
        "task_assigned" => Some("Penugasan baru\n\nHalo {{user_name}},\n\nAnda ditugaskan pada {{pekerjaan_name}}.\n\nBuka: {{pekerjaan_url}}"),
        "data_reminder" => Some("Lengkapi data Anda\n\nHalo {{user_name}},\n\nBelum lengkap: {{missing_fields}}\n\nPerbarui: {{profile_url}}"),
        "contract_ready" => Some("Dokumen siap\n\nHalo {{user_name}},\n\n{{kontrak_name}} dapat diunduh.\n\nUnduh: {{download_url}}"),
        "report_submitted" => Some("Laporan terkirim\n\nHalo {{user_name}},\n\n{{report_name}} berhasil dikirim.\n\nLihat: {{report_url}}"),
        _ => None,
    }
}

const DEFAULT_PLAIN_FALLBACK: &str = "Notifikasi dari {{app_name}}.";

/// `defaultHtmlBody($key)`.
fn html_body(b: &Brand, key: &str) -> String {
    let h = |t: &str, s: Option<&str>| mail_layout::heading(b, t, s, false);
    let p = |t: &str| mail_layout::paragraph(b, t);
    let info = |c: &str| mail_layout::info_box(b, c);
    let btn = |l: &str, u: &str| mail_layout::button(b, l, u);
    let greet = |n: &str| mail_layout::greeting(b, n);
    match key {
        "smtp_test" => {
            h("Uji Koneksi SMTP", Some("Verifikasi konfigurasi email {{app_name}}"))
                + &p("Email ini dikirim dari Pengaturan Aplikasi untuk memastikan host, port, dan kredensial SMTP sudah benar.")
                + &info(
                    &(mail_layout::check_item(b, "Jika Anda membaca pesan ini, pengiriman email berfungsi.")
                        + &mail_layout::check_item(
                            b,
                            "Header memakai logo dari Pengaturan Aplikasi; warna disesuaikan otomatis.",
                        )),
                )
                + &p("Tidak perlu membalas email ini.")
        }
        "forgot_password" => {
            h("Reset Password", Some("Permintaan pemulihan akses akun Anda"))
                + &greet("{{user_name}}")
                + &p("Kami menerima permintaan reset password untuk akun {{user_email}}. Klik tombol di bawah untuk membuat password baru.")
                + &btn("Buat Password Baru", "{{reset_link}}")
                + &info("Tautan berlaku <strong>{{expiry_minutes}} menit</strong>. Jika Anda tidak meminta reset, abaikan email ini.")
        }
        "welcome" => {
            mail_layout::badge(b, "Selamat Datang")
                + &mail_layout::heading(b, "Akun Anda Aktif", Some("Terima kasih telah bergabung di {{app_name}}"), true)
                + &greet("{{user_name}}")
                + &p("Berikut ringkasan akun Anda:")
                + &mail_layout::bullet_list(b, &["Email: <strong>{{user_email}}</strong>"])
                + &btn("Masuk ke Aplikasi", "{{login_url}}")
                + &p("Jika Anda tidak merasa mendaftar, hubungi administrator.")
        }
        "broadcast" => {
            mail_layout::badge(b, "Pengumuman")
                + &mail_layout::heading(b, "{{title}}", Some("Dari {{app_name}}"), true)
                + &mail_layout::message_block(b, "{{message}}")
                + &btn("Buka Aplikasi Pengawasan", "{{action_url}}")
                + &mail_layout::info_tiles(
                    b,
                    &[
                        Tile {
                            icon: "📍",
                            title: "Aplikasi Pengawasan",
                            description: "Kelola pekerjaan dan laporan dari satu tempat.",
                        },
                        Tile {
                            icon: "📋",
                            title: "Kelengkapan Data",
                            description: "Pastikan profil dan data pekerjaan Anda selalu mutakhir.",
                        },
                    ],
                )
        }
        "ticket_created" => {
            h("Tiket Baru Dicatat", Some("Nomor tiket #{{ticket_id}}"))
                + &greet("{{user_name}}")
                + &p("Tiket bantuan Anda telah tercatat di sistem dan akan segera ditindaklanjuti.")
                + &info("<strong>Judul:</strong> {{ticket_title}}<br><strong>Nomor:</strong> #{{ticket_id}}")
                + &btn("Buka Tiket", "{{ticket_url}}")
        }
        "ticket_updated" => {
            h("Update Tiket", Some("Perubahan status tiket #{{ticket_id}}"))
                + &greet("{{user_name}}")
                + &p("Ada pembaruan pada tiket bantuan Anda.")
                + &info("<strong>Judul:</strong> {{ticket_title}}<br><strong>Status:</strong> {{status}}")
                + &btn("Lihat Tiket", "{{ticket_url}}")
        }
        "task_assigned" => {
            h("Penugasan Baru", Some("Anda ditugaskan pada pekerjaan"))
                + &greet("{{user_name}}")
                + &p("Anda ditugaskan untuk mengawasi pekerjaan berikut:")
                + &info("<strong>{{pekerjaan_name}}</strong>")
                + &btn("Buka Pekerjaan", "{{pekerjaan_url}}")
        }
        "data_reminder" => {
            h("Lengkapi Data Anda", Some("Profil atau data pekerjaan belum lengkap"))
                + &greet("{{user_name}}")
                + &p("Mohon lengkapi data berikut agar proses pengawasan berjalan lancar:")
                + &info("<strong>Belum lengkap:</strong> {{missing_fields}}")
                + &btn("Perbarui Profil", "{{profile_url}}")
        }
        "contract_ready" => {
            h("Dokumen Kontrak Siap", Some("{{kontrak_name}}"))
                + &greet("{{user_name}}")
                + &p("Dokumen kontrak yang Anda butuhkan sudah tersedia dan dapat diunduh.")
                + &btn("Unduh Dokumen", "{{download_url}}")
        }
        "report_submitted" => {
            h("Laporan Terkirim", Some("{{report_name}}"))
                + &greet("{{user_name}}")
                + &p("Laporan Anda telah berhasil dikirim dan tercatat di sistem.")
                + &btn("Lihat Laporan", "{{report_url}}")
        }
        _ => p("Notifikasi dari {{app_name}}."),
    }
}

// ---------------------------------------------------------------------------
// Data template: default, tersimpan, dan legacy
// ---------------------------------------------------------------------------

/// Satu baris katalog setelah default dan data tersimpan digabung.
pub struct Entry {
    pub key: &'static str,
    pub format: String,
    pub subject: String,
    pub body: String,
    pub is_custom: bool,
    pub updated_at: Option<DateTime<Utc>>,
}

/// `(string)` PHP untuk nilai JSON.
fn php_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "1".to_string(),
        Value::Bool(false) | Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        _ => "Array".to_string(),
    }
}

/// `$custom[$field] ?? null` lalu `(string)`. Selain objek menghasilkan `None`.
fn field(custom: &Value, name: &str) -> Option<String> {
    match custom {
        Value::Object(m) => m.get(name).filter(|v| !v.is_null()).map(php_str),
        _ => None,
    }
}

/// `storedMap()`: JSON di `mail_templates`. Tidak valid atau kosong menjadi map kosong.
async fn stored_map(pool: &MySqlPool) -> Result<Map<String, Value>, ApiError> {
    let raw = get_setting(pool, STORAGE_KEY, Some(""))
        .await?
        .unwrap_or_default();
    if raw.is_empty() {
        return Ok(Map::new());
    }
    Ok(match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    })
}

/// `storageUpdatedAt()`.
async fn storage_updated_at(pool: &MySqlPool) -> Result<Option<DateTime<Utc>>, ApiError> {
    let row: Option<Option<DateTime<Utc>>> = sqlx::query_scalar(
        "SELECT updated_at FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(STORAGE_KEY)
    .fetch_optional(pool)
    .await
    .map_err(crate::media::internal)?;
    Ok(row.flatten())
}

/// `legacySmtpTestOverride()`: `mail_body_format`, `mail_subject`, `mail_body` lama.
async fn legacy_override(pool: &MySqlPool, brand: &Brand) -> Result<Option<Value>, ApiError> {
    let fmt = get_setting(pool, "mail_body_format", Some("")).await?;
    let subj = get_setting(pool, "mail_subject", Some("")).await?;
    let body = get_setting(pool, "mail_body", Some("")).await?;
    let empty = |v: &Option<String>| v.as_deref() == Some("");
    if empty(&fmt) && empty(&subj) && empty(&body) {
        return Ok(None);
    }
    let format = match fmt.as_deref() {
        Some(f @ (FORMAT_PLAIN | FORMAT_MARKDOWN | FORMAT_HTML)) => f.to_string(),
        _ => FORMAT_MARKDOWN.to_string(),
    };
    let subject = match subj {
        Some(s) if !s.is_empty() => s,
        _ => default_subject("smtp_test"),
    };
    let body = match body {
        Some(b) if !b.is_empty() => b,
        _ => default_body(brand, "smtp_test", &format),
    };
    Ok(Some(
        json!({ "format": format, "subject": subject, "body": body }),
    ))
}

/// `catalog()` tanpa presets: satu entri per template, urut seperti `TEMPLATES`.
pub async fn entries(pool: &MySqlPool, brand: &Brand) -> Result<Vec<Entry>, ApiError> {
    let stored = stored_map(pool).await?;
    let legacy = legacy_override(pool, brand).await?;
    let updated_at = storage_updated_at(pool).await?;
    let mut out = Vec::with_capacity(TEMPLATES.len());
    for def in TEMPLATES {
        let mut custom: Option<Value> = stored.get(def.key).filter(|v| !v.is_null()).cloned();
        if def.key == "smtp_test" && custom.is_none() {
            custom = legacy.clone();
        }
        let format = field_or(&custom, "format", def.default_format());
        let subject = field_or_else(&custom, "subject", || default_subject(def.key));
        let body = field_or_else(&custom, "body", || default_body(brand, def.key, &format));
        let is_custom = custom.is_some();
        out.push(Entry {
            key: def.key,
            format,
            subject,
            body,
            is_custom,
            updated_at: if is_custom { updated_at } else { None },
        });
    }
    Ok(out)
}

impl TemplateDef {
    fn default_format(&self) -> &'static str {
        FORMAT_HTML
    }
}

fn field_or(custom: &Option<Value>, name: &str, default: &str) -> String {
    custom
        .as_ref()
        .and_then(|c| field(c, name))
        .unwrap_or_else(|| default.to_string())
}

fn field_or_else(custom: &Option<Value>, name: &str, default: impl FnOnce() -> String) -> String {
    custom
        .as_ref()
        .and_then(|c| field(c, name))
        .unwrap_or_else(default)
}

/// Bentuk JSON satu template untuk `GET` dan respons `POST`.
pub fn catalog_json(brand: &Brand, entries: &[Entry]) -> Vec<Value> {
    entries
        .iter()
        .map(|e| {
            let def = template_def(e.key).expect("kunci dari TEMPLATES");
            json!({
                "key": def.key,
                "label": def.label,
                "description": def.description,
                "category": def.category,
                "placeholders": def.placeholders,
                "format": e.format,
                "subject": e.subject,
                "body": e.body,
                "is_custom": e.is_custom,
                "updated_at": format::iso8601_utc(e.updated_at),
                "presets": {
                    "markdown": { "subject": default_subject(def.key), "body": default_body(brand, def.key, FORMAT_MARKDOWN) },
                    "html": { "subject": default_subject(def.key), "body": default_body(brand, def.key, FORMAT_HTML) },
                    "plain": { "subject": default_subject(def.key), "body": default_body(brand, def.key, FORMAT_PLAIN) },
                },
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Placeholder, resolve, dan simpan
// ---------------------------------------------------------------------------

/// `sampleVariables($key)` tanpa penimpaan. Urutan sama dengan Laravel (penting untuk penggantian berurutan).
pub fn sample_variables(brand: &Brand) -> Vec<(&'static str, String)> {
    let base = brand.frontend_url.as_str();
    vec![
        ("app_name", brand.app_name.clone()),
        ("user_name", "Budi Santoso".to_string()),
        ("user_email", "budi@example.com".to_string()),
        (
            "reset_link",
            frontend_to(base, "/reset-password?token=contoh-token"),
        ),
        ("expiry_minutes", "60".to_string()),
        ("login_url", frontend_to(base, "/login")),
        ("title", "Pengumuman Penting".to_string()),
        (
            "message",
            "Ini contoh isi broadcast untuk semua pengguna.".to_string(),
        ),
        ("action_url", pengawas_app(base, "/")),
        ("ticket_id", "1042".to_string()),
        ("ticket_title", "Permintaan akses modul kontrak".to_string()),
        ("ticket_url", frontend_to(base, "/tiket/1042")),
        ("status", "Diproses".to_string()),
        (
            "pekerjaan_name",
            "Pembangunan SPAM Desa Sukamaju".to_string(),
        ),
        ("pekerjaan_url", pengawas_app(base, "pekerjaan/12")),
        ("missing_fields", "Nomor telepon, foto profil".to_string()),
        ("profile_url", pengawas_app(base, "profile")),
        ("kontrak_name", "SPK-2026-001".to_string()),
        ("download_url", frontend_to(base, "/kontrak/1/download")),
        ("report_name", "Laporan Mingguan Maret 2026".to_string()),
        ("report_url", frontend_to(base, "/laporan/88")),
    ]
}

/// `MailContentService::applyPlaceholders`: penggantian `{{kunci}}` satu per satu, berurutan.
pub fn apply_placeholders(content: &str, vars: &[(&'static str, String)]) -> String {
    let mut out = content.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

/// Hasil `MailTemplateService::resolve`.
pub struct Resolved {
    pub subject: String,
    pub body: String,
    pub format: String,
}

/// `MailTemplateService::resolve($key, [], $overrides)`. Nilai di `overrides` (format, subject, body)
/// didahulukan, lalu katalog, lalu default. Kunci tidak dikenal menjadi error 500 (seperti exception Laravel).
pub async fn resolve(
    pool: &MySqlPool,
    brand: &Brand,
    key: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<Resolved, ApiError> {
    if !is_valid_key(key) {
        return Err(ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unknown mail template key: {key}"),
        ));
    }
    let entries = entries(pool, brand).await?;
    let entry = entries
        .iter()
        .find(|e| e.key == key)
        .expect("kunci sudah divalidasi");
    let format = overrides
        .get("format")
        .cloned()
        .unwrap_or_else(|| entry.format.clone());
    let subject = overrides
        .get("subject")
        .cloned()
        .unwrap_or_else(|| entry.subject.clone());
    let body = overrides
        .get("body")
        .cloned()
        .unwrap_or_else(|| entry.body.clone());
    let vars = sample_variables(brand);
    Ok(Resolved {
        subject: apply_placeholders(&subject, &vars),
        body: apply_placeholders(&body, &vars),
        format,
    })
}

/// `MailTemplateService::saveMany`. Kunci tidak dikenal dan payload non-array dilewati.
pub async fn save_many(
    pool: &MySqlPool,
    ctx: &WriteCtx<'_>,
    brand: &Brand,
    templates: &Map<String, Value>,
) -> Result<(), ApiError> {
    let mut stored = stored_map(pool).await?;
    for (key, payload) in templates {
        let Some(def) = template_def(key) else {
            continue;
        };
        if !matches!(payload, Value::Object(_) | Value::Array(_)) {
            continue;
        }
        let format = field(payload, "format").unwrap_or_else(|| def.default_format().to_string());
        let format = match format.as_str() {
            FORMAT_PLAIN | FORMAT_MARKDOWN | FORMAT_HTML => format,
            _ => def.default_format().to_string(),
        };
        let subject =
            mail_layout::php_trim(&field(payload, "subject").unwrap_or_default()).to_string();
        let subject = if subject.is_empty() {
            default_subject(def.key)
        } else {
            subject
        };
        let body = field(payload, "body").unwrap_or_default();
        let body = if body.is_empty() {
            default_body(brand, def.key, &format)
        } else {
            body
        };
        stored.insert(
            key.clone(),
            json!({ "format": format, "subject": subject, "body": body }),
        );
    }
    // `json_encode([])` menghasilkan `[]`, bukan `{}`.
    let encoded = if stored.is_empty() {
        "[]".to_string()
    } else {
        Value::Object(stored).to_string()
    };
    mail_layout::write_setting(pool, ctx, STORAGE_KEY, &encoded).await
}
