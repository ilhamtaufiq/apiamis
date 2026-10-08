//! Pengiriman email SMTP, setara `MailConfigService::applyFromSettings` dan `MailContentService::sendRendered`.
//!
//! Pengaturan dibaca dari `app_settings` (`mail_enabled`, `mail_host`, `mail_port`, `mail_encryption`,
//! `mail_username`, `mail_password`, `mail_from_address`, `mail_from_name`). Email dikirim sebagai
//! multipart teks + HTML bila HTML diberikan, sama dengan `$message->html()` dan `$message->text()`.

use lettre::{
    address::Address,
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use sqlx::MySqlPool;

#[derive(Debug, Clone, PartialEq)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    /// `ssl`, `tls`, atau lainnya (opportunistic, seperti default Symfony Mailer).
    pub encryption: String,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub from_name: String,
}

async fn setting(pool: &MySqlPool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let value: Option<Option<String>> =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(value.flatten())
}

/// `MailConfigService::applyFromSettings` tanpa perubahan konfigurasi global. `None` bila email nonaktif
/// atau host, username, atau password kosong.
pub async fn load_settings(pool: &MySqlPool) -> Result<Option<SmtpSettings>, sqlx::Error> {
    let or = |v: Option<String>, default: &str| v.unwrap_or_else(|| default.to_string());

    if or(setting(pool, "mail_enabled").await?, "0") != "1" {
        return Ok(None);
    }
    let host = or(setting(pool, "mail_host").await?, "smtp.gmail.com");
    let port_raw: i64 = or(setting(pool, "mail_port").await?, "587")
        .trim()
        .parse()
        .unwrap_or(0);
    let encryption = or(setting(pool, "mail_encryption").await?, "tls");
    let username = or(setting(pool, "mail_username").await?, "");
    let password = or(setting(pool, "mail_password").await?, "");
    let from_address = or(setting(pool, "mail_from_address").await?, &username);
    let app_name = or(setting(pool, "app_name").await?, "Arumanis");
    let from_name = or(setting(pool, "mail_from_name").await?, &app_name);

    if host.trim().is_empty() || username.trim().is_empty() || password.is_empty() {
        return Ok(None);
    }
    let default_port = if encryption == "ssl" { 465 } else { 587 };
    let port = if port_raw > 0 {
        port_raw as u16
    } else {
        default_port
    };

    Ok(Some(SmtpSettings {
        host: host.trim().to_string(),
        port,
        encryption,
        username,
        password,
        from_address: if from_address.trim().is_empty() {
            String::new()
        } else {
            from_address.trim().to_string()
        },
        from_name: if from_name.trim().is_empty() {
            "Arumanis".to_string()
        } else {
            from_name.trim().to_string()
        },
    }))
}

/// Transport SMTP. `ssl` memakai TLS langsung, `tls` memakai STARTTLS wajib, dan lainnya opportunistic.
pub fn transport(settings: &SmtpSettings) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let tls = match settings.encryption.as_str() {
        "ssl" => Tls::Wrapper(
            TlsParameters::new_rustls(settings.host.clone()).map_err(|e| e.to_string())?,
        ),
        "tls" => Tls::Required(
            TlsParameters::new_rustls(settings.host.clone()).map_err(|e| e.to_string())?,
        ),
        _ => Tls::Opportunistic(
            TlsParameters::new_rustls(settings.host.clone()).map_err(|e| e.to_string())?,
        ),
    };
    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(settings.host.clone())
            .port(settings.port)
            .tls(tls)
            .credentials(Credentials::new(
                settings.username.clone(),
                settings.password.clone(),
            ))
            .build(),
    )
}

/// Kirim satu email. `html` opsional; bila ada, email multipart teks + HTML.
pub async fn send(
    settings: &SmtpSettings,
    to: &str,
    recipient_name: Option<&str>,
    subject: &str,
    text: &str,
    html: Option<&str>,
) -> Result<(), String> {
    let from_addr: Address = settings
        .from_address
        .parse()
        .map_err(|e| format!("alamat pengirim tidak valid: {e}"))?;
    let from = Mailbox::new(Some(settings.from_name.clone()), from_addr);
    let to_addr: Address = to
        .trim()
        .to_lowercase()
        .parse()
        .map_err(|e| format!("alamat penerima tidak valid: {e}"))?;
    let to_box = Mailbox::new(recipient_name.map(str::to_string), to_addr);

    let builder = Message::builder().from(from).to(to_box).subject(subject);
    let message = match html {
        Some(html) => builder
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
            .map_err(|e| e.to_string())?,
        None => builder
            .singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_PLAIN)
                    .body(text.to_string()),
            )
            .map_err(|e| e.to_string())?,
    };

    transport(settings)?
        .send(message)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
