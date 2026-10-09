//! Tata letak email (`MailLayoutService`), konteks merek (`branding()`), dan `FrontendUrlService`.
//!
//! `Brand` dihitung sekali per permintaan, setara cache statis `branding()` di Laravel. Menghitung brand
//! ikut menulis `brand_primary_color` bila warnanya berasal dari setting atau logo (lihat `brand_color`).
//!
//! Fungsi `php_*` meniru `htmlspecialchars`, `nl2br`, `strip_tags`, dan `html_entity_decode` untuk
//! kebutuhan email. `strip_tags` dan decode entitas hanya mencakup kasus umum.

use axum::http::HeaderMap;
use chrono::{Datelike, Utc};
use shared::ApiError;
use sqlx::MySqlPool;

use super::brand_color::{self, Palette};
use crate::{app_settings, media};

pub const APP_MODEL: &str = "App\\Models\\AppSetting";

/// Konteks penulisan (audit): pengguna yang login, URL permintaan, dan header klien.
pub struct WriteCtx<'a> {
    pub actor: u64,
    pub url: &'a str,
    pub headers: &'a HeaderMap,
}

/// `AppSetting::getValue($key, $default)`. Baris tidak ada: `default`. Baris dengan nilai NULL: `None`.
pub async fn get_setting(
    pool: &MySqlPool,
    key: &str,
    default: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let row: Option<Option<String>> =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(media::internal)?;
    Ok(match row {
        Some(v) => v,
        None => default.map(str::to_string),
    })
}

/// `(string) AppSetting::getValue(...)`: nilai NULL menjadi string kosong.
pub async fn get_setting_str(
    pool: &MySqlPool,
    key: &str,
    default: &str,
) -> Result<String, ApiError> {
    Ok(get_setting(pool, key, Some(default))
        .await?
        .unwrap_or_default())
}

/// `AppSetting::setValue($key, $value, 'text')` dengan audit, dalam satu transaksi.
pub async fn write_setting(
    pool: &MySqlPool,
    ctx: &WriteCtx<'_>,
    key: &str,
    value: &str,
) -> Result<(), ApiError> {
    let mut tx = pool.begin().await.map_err(media::internal)?;
    app_settings::upsert_setting(
        &mut tx,
        ctx.actor,
        ctx.url,
        ctx.headers,
        key,
        Some(value),
        "text",
    )
    .await?;
    tx.commit().await.map_err(media::internal)?;
    Ok(())
}

/// Konteks merek yang dipakai satu permintaan.
pub struct Brand {
    pub app_name: String,
    /// Kosong bila tidak ada logo (`logo_url` null atau kosong di Laravel).
    pub logo_url: String,
    pub frontend_url: String,
    pub year: String,
    pub palette: Palette,
}

/// `MailLayoutService::branding()`.
pub async fn brand(pool: &MySqlPool, app_url: &str, ctx: &WriteCtx<'_>) -> Result<Brand, ApiError> {
    let app_name = get_setting_str(pool, "app_name", "Arumanis").await?;
    let logo_url = logo_url(pool, app_url).await?;
    let palette = brand_color::palette(pool, ctx).await?;
    let frontend_url = frontend_base(pool, app_url).await?;
    Ok(Brand {
        app_name,
        logo_url,
        frontend_url,
        year: Utc::now().year().to_string(),
        palette,
    })
}

async fn logo_url(pool: &MySqlPool, app_url: &str) -> Result<String, ApiError> {
    let id: Option<u64> = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM app_settings WHERE `key` = 'logo' ORDER BY id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(media::internal)?;
    let Some(id) = id else {
        return Ok(String::new());
    };
    let (url, _thumb) = media::first_urls(pool, app_url, APP_MODEL, id, "app-settings")
        .await
        .map_err(media::internal)?;
    Ok(absolute_url(&url, app_url))
}

/// `MailLayoutService::absoluteUrl`.
pub fn absolute_url(url: &str, app_url: &str) -> String {
    if url.is_empty() || url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    format!(
        "{}/{}",
        app_url.trim_end_matches('/'),
        url.trim_start_matches('/')
    )
}

/// `FrontendUrlService::base()`: setting `frontend_url`, lalu env `FRONTEND_URL`, lalu `APP_URL`.
pub async fn frontend_base(pool: &MySqlPool, app_url: &str) -> Result<String, ApiError> {
    let from_setting = get_setting(pool, "frontend_url", None)
        .await?
        .unwrap_or_default();
    let from_setting = from_setting.trim();
    if !from_setting.is_empty() {
        return Ok(from_setting.trim_end_matches('/').to_string());
    }
    let from_env = std::env::var("FRONTEND_URL").unwrap_or_default();
    let from_env = from_env.trim();
    if !from_env.is_empty() {
        return Ok(from_env.trim_end_matches('/').to_string());
    }
    let base = app_url.trim_end_matches('/');
    Ok(if base.is_empty() {
        "http://localhost".to_string()
    } else {
        base.to_string()
    })
}

/// `FrontendUrlService::to($path)`.
pub fn frontend_to(base: &str, path: &str) -> String {
    format!("{}/{}", base, path.trim_start_matches('/'))
}

/// `FrontendUrlService::pengawasApp($path)`.
pub fn pengawas_app(base: &str, path: &str) -> String {
    let configured = std::env::var("PENGAWAS_APP_BASE_URL").unwrap_or_default();
    let configured = configured.trim();
    if !configured.is_empty() && configured.starts_with("http") {
        return format!(
            "{}/{}",
            configured.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
    }
    let suffix = if configured.is_empty() {
        "pengawasan"
    } else {
        configured.trim_matches('/')
    };
    frontend_to(
        base,
        &format!("{}/{}", suffix, path.trim_start_matches('/')),
    )
}

// ---------------------------------------------------------------------------
// Helper string seperti PHP
// ---------------------------------------------------------------------------

/// `trim()` PHP: spasi, tab, CR, LF, NUL, dan VT.
pub fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')`.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            other => out.push(other),
        }
    }
    out
}

/// `nl2br()`: sisipkan `<br />` sebelum setiap urutan baris baru.
pub fn nl2br(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\r' || c == '\n' {
            out.push_str("<br />");
            let next = chars.get(i + 1).copied();
            // "\r\n" dan "\n\r" dihitung sebagai satu urutan baris baru.
            if (c == '\r' && next == Some('\n')) || (c == '\n' && next == Some('\r')) {
                out.push(c);
                out.push(next.unwrap());
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `strip_tags()` (kasus umum): membuang `<...>` yang dimulai huruf, `/`, `!`, atau `?`.
pub fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            if let Some(&n) = chars.peek() {
                if n.is_ascii_alphabetic() || n == '/' || n == '!' || n == '?' {
                    for d in chars.by_ref() {
                        if d == '>' {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// `html_entity_decode()` untuk entitas umum.
pub fn entity_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let decoded = after.find(';').and_then(|end| {
            if end > 12 {
                return None;
            }
            let name = &after[1..end];
            let value = match name {
                "amp" => "&".to_string(),
                "lt" => "<".to_string(),
                "gt" => ">".to_string(),
                "quot" => "\"".to_string(),
                "apos" => "'".to_string(),
                "nbsp" => "\u{a0}".to_string(),
                _ if name.starts_with("#x") || name.starts_with("#X") => {
                    u32::from_str_radix(&name[2..], 16)
                        .ok()
                        .and_then(char::from_u32)?
                        .to_string()
                }
                _ if name.starts_with('#') => name[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)?
                    .to_string(),
                _ => return None,
            };
            Some((value, end + 1))
        });
        match decoded {
            Some((value, consumed)) => {
                out.push_str(&value);
                rest = &after[consumed..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Pembungkus dokumen
// ---------------------------------------------------------------------------

const DIVIDER_CHAR: &str = "─";

/// `MailLayoutService::wrapDocument`.
pub fn wrap_document(brand: &Brand, inner_html: &str, preheader: Option<&str>) -> String {
    let app_name = esc(&brand.app_name);
    let frontend_url = esc(&brand.frontend_url);
    let year = esc(&brand.year);
    let preheader_text = esc(&strip_tags_trimmed(preheader.unwrap_or("")));
    let p = &brand.palette;

    let logo_block = if !brand.logo_url.is_empty() {
        format!(
            r#"<img src="{}" alt="{}" width="160" style="display:block;margin:0 auto;max-width:160px;height:auto;border:0;" />"#,
            esc(&brand.logo_url),
            app_name
        )
    } else {
        format!(
            r#"<div style="font-size:28px;font-weight:800;color:{};letter-spacing:-0.02em;text-align:center;">{}</div>"#,
            esc(&p.primary),
            app_name
        )
    };

    WRAP_TEMPLATE
        .replace("$$APP_NAME$$", &app_name)
        .replace("$$FRONTEND_URL$$", &frontend_url)
        .replace("$$YEAR$$", &year)
        .replace("$$PREHEADER$$", &preheader_text)
        .replace("$$PRIMARY$$", &esc(&p.primary))
        .replace("$$PRIMARY_DARK$$", &esc(&p.primary_dark))
        .replace("$$PRIMARY_FIXED$$", &esc(&p.primary_fixed))
        .replace("$$PRIMARY_CONTAINER$$", &esc(&p.primary_container))
        .replace("$$SECONDARY$$", &esc(&p.secondary))
        .replace("$$SURFACE$$", &esc(&p.surface))
        .replace("$$SURFACE_CONTAINER_LOW$$", &esc(&p.surface_container_low))
        .replace("$$OUTLINE_VARIANT$$", &esc(&p.outline_variant))
        .replace("$$ON_SURFACE$$", &esc(&p.on_surface))
        .replace("$$ON_SURFACE_VARIANT$$", &esc(&p.on_surface_variant))
        .replace("$$LOGO_BLOCK$$", &logo_block)
        // Diganti terakhir: isi dari pengguna tidak ikut diproses ulang.
        .replace("$$INNER$$", inner_html)
}

fn strip_tags_trimmed(s: &str) -> String {
    php_trim(&strip_tags(s)).to_string()
}

const WRAP_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="id">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>$$APP_NAME$$</title>
<!--[if mso]>
<style type="text/css">
body, table, td {font-family: Arial, sans-serif !important;}
</style>
<![endif]-->
</head>
<body style="margin:0;padding:0;background-color:$$SURFACE$$;font-family:'Segoe UI',Arial,'Plus Jakarta Sans',sans-serif;color:#1f1926;-webkit-font-smoothing:antialiased;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent;">$$PREHEADER$$</div>
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:$$SURFACE$$;padding:24px 12px;">
<tr>
<td align="center">
<table role="presentation" width="600" cellspacing="0" cellpadding="0" border="0" style="max-width:600px;width:100%;background-color:$$SURFACE$$;border-radius:16px;overflow:hidden;box-shadow:0 4px 24px -8px $$PRIMARY_CONTAINER$$;">
<tr>
<td style="background-color:$$PRIMARY_FIXED$$;padding:36px 28px;text-align:center;position:relative;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0">
<tr>
<td align="center" style="padding:0 0 4px;">
<span style="display:inline-block;font-size:20px;line-height:1;color:$$PRIMARY$$;opacity:0.35;">&#10022;</span>
</td>
</tr>
<tr>
<td align="center">$$LOGO_BLOCK$$</td>
</tr>
<tr>
<td align="center" style="padding:8px 0 0;">
<span style="display:inline-block;font-size:11px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;color:$$ON_SURFACE_VARIANT$$;opacity:0.85;">Notifikasi Resmi</span>
</td>
</tr>
</table>
</td>
</tr>
<tr>
<td style="background-color:$$SURFACE$$;padding:28px 24px 24px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:#ffffff;border:1px solid $$OUTLINE_VARIANT$$;border-radius:16px;">
<tr>
<td style="padding:32px 28px;">
$$INNER$$
</td>
</tr>
</table>
</td>
</tr>
<tr>
<td style="background-color:$$SURFACE_CONTAINER_LOW$$;padding:24px 28px;text-align:center;border-top:1px solid $$OUTLINE_VARIANT$$;">
<p style="margin:0 0 6px;font-size:16px;line-height:1.4;font-weight:700;color:$$PRIMARY_DARK$$;">$$APP_NAME$$</p>
<p style="margin:0 0 14px;font-size:13px;line-height:1.6;color:$$ON_SURFACE$$;">
Pesan ini dikirim secara otomatis oleh sistem <strong style="color:$$PRIMARY_DARK$$;">$$APP_NAME$$</strong>.
Harap tidak membalas email ini.
</p>
<p style="margin:0 0 14px;font-size:12px;line-height:1.6;">
<a href="$$FRONTEND_URL$$" style="color:$$SECONDARY$$;text-decoration:none;font-weight:600;">$$FRONTEND_URL$$</a>
</p>
<p style="margin:0;font-size:11px;line-height:1.5;color:$$ON_SURFACE_VARIANT$$;opacity:0.85;letter-spacing:0.2px;">
© $$YEAR$$ $$APP_NAME$$. Seluruh hak cipta dilindungi.
</p>
</td>
</tr>
</table>
</td>
</tr>
</table>
</body>
</html>"##;

/// `MailLayoutService::wrapPlainDocument`. Elemen kosong atau "0" dibuang (`array_filter`).
pub fn wrap_plain_document(brand: &Brand, text: &str) -> String {
    let divider = DIVIDER_CHAR.repeat(48);
    let parts = [
        brand.app_name.clone(),
        divider.clone(),
        php_trim(text).to_string(),
        divider,
        format!(
            "Pesan ini dikirim secara otomatis oleh sistem {}. Harap tidak membalas email ini.",
            brand.app_name
        ),
        brand.frontend_url.clone(),
        format!(
            "© {} {}. Seluruh hak cipta dilindungi.",
            brand.year, brand.app_name
        ),
    ];
    parts
        .into_iter()
        .filter(|p| !p.is_empty() && p != "0")
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Fragmen HTML (`MailLayoutService::badge` dan seterusnya)
// ---------------------------------------------------------------------------

pub fn badge(brand: &Brand, text: &str) -> String {
    let bg = esc(&brand.palette.secondary_fixed);
    let color = esc(&brand.palette.on_secondary_fixed);
    let label = esc(&text.to_ascii_uppercase());
    format!(
        r#"<p style="margin:0 0 16px;text-align:center;"><span style="display:inline-block;padding:6px 16px;background-color:{bg};color:{color};font-size:11px;font-weight:700;letter-spacing:0.1em;border-radius:9999px;">{label}</span></p>"#
    )
}

pub fn heading(brand: &Brand, text: &str, subtitle: Option<&str>, centered: bool) -> String {
    let primary_dark = esc(&brand.palette.primary_dark);
    let title = esc(text);
    let align = if centered { "center" } else { "left" };
    let subtitle_html = match subtitle {
        Some(sub) if !sub.is_empty() && sub != "0" => format!(
            r#"<p style="margin:8px 0 0;font-size:15px;line-height:1.6;color:{};text-align:{align};">{}</p>"#,
            brand.palette.on_surface_variant,
            esc(sub)
        ),
        _ => String::new(),
    };
    format!(
        r#"<h1 style="margin:0 0 20px;font-size:28px;line-height:1.25;font-weight:800;color:{primary_dark};letter-spacing:-0.02em;text-align:{align};">{title}</h1>{subtitle_html}"#
    )
}

pub fn greeting(brand: &Brand, name: &str) -> String {
    format!(
        r#"<p style="margin:0 0 16px;font-size:16px;line-height:1.7;color:{};">Halo <strong>{}</strong>,</p>"#,
        esc(&brand.palette.on_surface),
        esc(name)
    )
}

pub fn paragraph(brand: &Brand, text: &str) -> String {
    format!(
        r#"<p style="margin:0 0 16px;font-size:16px;line-height:1.75;color:{};">{}</p>"#,
        esc(&brand.palette.on_surface_variant),
        nl2br(&esc(text))
    )
}

pub fn button(brand: &Brand, label: &str, url: &str) -> String {
    let secondary = esc(&brand.palette.secondary);
    let on_secondary = esc(&brand.palette.on_secondary);
    let primary = esc(&brand.palette.primary);
    let safe_label = esc(label);
    // URL berplaceholder `{{...}}` dibiarkan apa adanya, seperti Laravel.
    let safe_url = if url.contains("{{") {
        url.to_string()
    } else {
        esc(url)
    };
    format!(
        r#"<table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="margin:28px 0 8px;">
<tr>
<td align="center">
<table role="presentation" cellspacing="0" cellpadding="0" border="0">
<tr>
<td align="center" style="border-radius:9999px;background-color:{secondary};">
<a href="{safe_url}" style="display:inline-block;padding:14px 32px;font-size:16px;font-weight:700;color:{on_secondary};text-decoration:none;border-radius:9999px;">{safe_label}</a>
</td>
</tr>
</table>
</td>
</tr>
</table>
<p style="margin:8px 0 0;font-size:12px;line-height:1.6;color:#94a3b8;text-align:center;word-break:break-all;">Atau salin tautan: <a href="{safe_url}" style="color:{primary};">{safe_url}</a></p>"#
    )
}

pub fn info_box(brand: &Brand, content: &str) -> String {
    let p = &brand.palette;
    format!(
        r#"<div style="margin:20px 0;padding:16px 18px;background-color:{};border:1px solid {};border-left:4px solid {};border-radius:12px;font-size:14px;line-height:1.7;color:{};">{content}</div>"#,
        esc(&p.surface_container_low),
        esc(&p.outline_variant),
        esc(&p.primary),
        esc(&p.on_surface)
    )
}

pub fn check_item(brand: &Brand, text: &str) -> String {
    format!(
        r#"<div style="margin:0 0 8px;"><strong style="color:{};">✓</strong> {}</div>"#,
        esc(&brand.palette.primary),
        esc(text)
    )
}

pub fn bullet_list(brand: &Brand, items: &[&str]) -> String {
    let color = esc(&brand.palette.on_surface_variant);
    let lis: String = items
        .iter()
        .map(|item| {
            format!(
                r#"<li style="margin:0 0 8px;font-size:16px;line-height:1.6;color:{color};">{item}</li>"#
            )
        })
        .collect();
    format!(r#"<ul style="margin:0 0 16px;padding-left:20px;">{lis}</ul>"#)
}

pub fn message_block(brand: &Brand, message: &str) -> String {
    info_box(
        brand,
        &format!(
            r#"<div style="white-space:pre-wrap;">{}</div>"#,
            esc(message)
        ),
    )
}

/// Satu kartu di `infoTiles`.
pub struct Tile<'a> {
    pub icon: &'a str,
    pub title: &'a str,
    pub description: &'a str,
}

pub fn info_tiles(brand: &Brand, tiles: &[Tile<'_>]) -> String {
    if tiles.is_empty() {
        return String::new();
    }
    let p = &brand.palette;
    let bg = esc(&p.surface_container_low);
    let border = esc(&p.outline_variant);
    let primary = esc(&p.primary);
    let desc_color = esc(&p.on_surface_variant);
    let count = tiles.len();

    let mut cells = String::new();
    for (index, tile) in tiles.iter().enumerate() {
        let icon = esc(tile.icon);
        let title = esc(tile.title);
        let description = esc(tile.description);
        let width = if count > 1 { "50%" } else { "100%" };
        let pad_right = if index == 0 && count > 1 {
            " padding-right:8px;"
        } else {
            ""
        };
        let pad_left = if index > 0 { " padding-left:8px;" } else { "" };
        cells.push_str(&format!(
            r#"<td width="{width}" valign="top" style="width:{width};{pad_right}{pad_left}">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{bg};border:1px solid {border};border-radius:12px;">
<tr>
<td style="padding:16px;">
<div style="font-size:20px;line-height:1;margin:0 0 8px;">{icon}</div>
<div style="font-size:14px;font-weight:600;color:{primary};margin:0 0 4px;">{title}</div>
<div style="font-size:12px;line-height:1.5;color:{desc_color};">{description}</div>
</td>
</tr>
</table>
</td>"#
        ));
    }
    format!(
        r#"<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="margin:24px 0 0;"><tr>{cells}</tr></table>"#
    )
}
