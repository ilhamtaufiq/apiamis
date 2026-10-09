//! Palet warna merek (`BrandColorService`).
//!
//! Warna utama diambil dari `brand_primary_color` bila valid, lalu dari piksel logo, lalu default ungu.
//! Hasilnya ditulis ulang ke `brand_primary_color` (dengan audit), seperti `persistBrandPrimary` di Laravel.
//!
//! Perbedaan dengan Laravel:
//! - Logo SVG: pola regex sama dengan Laravel.
//! - Logo raster: hanya JPEG dan PNG (crate `image` tanpa fitur lain). GIF dan WebP tidak diekstrak,
//!   sedangkan GD di PHP bisa. Pengecilan ke maksimal 64 px memakai rata-rata area (box filter), bukan
//!   `imagecopyresampled` GD, sehingga warna dominan bisa berbeda pada logo yang rumit.

use std::collections::HashMap;

use regex::Regex;
use shared::ApiError;
use sqlx::MySqlPool;

use super::mail_layout::{self, WriteCtx, APP_MODEL};
use crate::media;

pub const DEFAULT_PRIMARY: &str = "#674bb5";
const STORAGE_KEY: &str = "brand_primary_color";

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub primary: String,
    pub primary_dark: String,
    pub primary_light: String,
    pub primary_fixed: String,
    pub primary_container: String,
    pub secondary: String,
    pub secondary_fixed: String,
    pub on_secondary: String,
    pub on_secondary_fixed: String,
    pub surface: String,
    pub surface_container: String,
    pub surface_container_low: String,
    pub outline_variant: String,
    pub on_surface: String,
    pub on_surface_variant: String,
}

/// `BrandColorService::palette()`: menulis `brand_primary_color` bila warnanya dari setting atau logo.
pub async fn palette(pool: &MySqlPool, ctx: &WriteCtx<'_>) -> Result<Palette, ApiError> {
    let primary = resolve_primary_hex(pool, ctx).await?;
    Ok(palette_from_primary(&primary))
}

async fn resolve_primary_hex(pool: &MySqlPool, ctx: &WriteCtx<'_>) -> Result<String, ApiError> {
    if let Some(stored) = mail_layout::get_setting(pool, STORAGE_KEY, None).await? {
        if is_valid_hex(&stored) {
            return persist(pool, ctx, &normalize_hex(&stored)).await;
        }
    }
    if let Some(extracted) = extract_from_logo(pool).await? {
        return persist(pool, ctx, &extracted).await;
    }
    Ok(DEFAULT_PRIMARY.to_string())
}

async fn persist(pool: &MySqlPool, ctx: &WriteCtx<'_>, hex: &str) -> Result<String, ApiError> {
    let sanitized = sanitize_brand_primary(hex);
    mail_layout::write_setting(pool, ctx, STORAGE_KEY, &sanitized).await?;
    Ok(sanitized)
}

async fn extract_from_logo(pool: &MySqlPool) -> Result<Option<String>, ApiError> {
    let id: Option<u64> = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM app_settings WHERE `key` = 'logo' ORDER BY id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(media::internal)?;
    let Some(id) = id else {
        return Ok(None);
    };
    let Some(media) = media::first_media(pool, APP_MODEL, id, "app-settings")
        .await
        .map_err(media::internal)?
    else {
        return Ok(None);
    };
    let path = media::media_dir(media.id).join(&media.file_name);
    let is_svg = media.mime_type.contains("svg")
        || path
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".svg");
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    Ok(if is_svg {
        extract_from_svg(&String::from_utf8_lossy(&bytes))
    } else {
        extract_from_raster(&bytes)
    })
}

/// `extractFromSvg`: pola pertama yang cocok dan tidak netral.
pub fn extract_from_svg(content: &str) -> Option<String> {
    let patterns = [
        Regex::new(r##"(?i)\bfill\s*=\s*["']?(#[0-9a-fA-F]{3,8})"##).expect("regex"),
        Regex::new(r##"(?i)\bstroke\s*=\s*["']?(#[0-9a-fA-F]{3,8})"##).expect("regex"),
        Regex::new(r"#([0-9a-fA-F]{6})\b").expect("regex"),
    ];
    for re in &patterns {
        if let Some(caps) = re.captures(content) {
            let m = caps.get(1).map_or("", |g| g.as_str());
            let candidate = if m.starts_with('#') {
                m.to_string()
            } else {
                format!("#{m}")
            };
            if is_valid_hex(&candidate) && !is_neutral_hex(&normalize_hex(&candidate)) {
                return Some(normalize_hex(&candidate));
            }
        }
    }
    None
}

/// `extractFromRaster`: warna dominan non-netral yang diberi bobot hue ungu.
pub fn extract_from_raster(data: &[u8]) -> Option<String> {
    let img = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let tw = w.clamp(1, 64);
    let th = h.clamp(1, 64);
    let small = box_resample(&img, tw, th);

    // Urutan penyisipan dipertahankan (seperti array PHP) untuk memutus seri skor.
    let mut buckets: Vec<(u32, u64)> = Vec::new();
    let mut index: HashMap<u32, usize> = HashMap::new();
    for [r, g, b, a] in small {
        // Alpha GD: 0 = opak, 127 = transparan. Piksel dengan alpha > 96 dilewati.
        let gd_alpha = ((255 - a as u32) * 127 + 127) / 255;
        if gd_alpha > 96 || is_neutral_rgb(r as i64, g as i64, b as i64) {
            continue;
        }
        let key = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        match index.get(&key) {
            Some(&i) => buckets[i].1 += 1,
            None => {
                index.insert(key, buckets.len());
                buckets.push((key, 1));
            }
        }
    }
    let mut best: Option<u32> = None;
    let mut best_score = -1.0_f64;
    for (key, count) in buckets {
        let r = ((key >> 16) & 0xFF) as i64;
        let g = ((key >> 8) & 0xFF) as i64;
        let b = (key & 0xFF) as i64;
        let score = count as f64 * purple_hue_score(r, g, b);
        if score > best_score {
            best_score = score;
            best = Some(key);
        }
    }
    best.map(|k| format!("#{k:06x}"))
}

/// Rata-rata area (box filter) untuk pengecilan. Hasil berurutan baris, `[r, g, b, a]`.
fn box_resample(img: &image::RgbaImage, tw: u32, th: u32) -> Vec<[u8; 4]> {
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity((tw * th) as usize);
    for dy in 0..th {
        let y0 = dy as f64 * h as f64 / th as f64;
        let y1 = (dy + 1) as f64 * h as f64 / th as f64;
        for dx in 0..tw {
            let x0 = dx as f64 * w as f64 / tw as f64;
            let x1 = (dx + 1) as f64 * w as f64 / tw as f64;
            let mut acc = [0f64; 4];
            let mut total = 0f64;
            let sy_end = (y1.ceil() as u32).min(h);
            let sx_end = (x1.ceil() as u32).min(w);
            for sy in (y0.floor() as u32)..sy_end {
                let wy = (y1.min(sy as f64 + 1.0) - y0.max(sy as f64)).max(0.0);
                for sx in (x0.floor() as u32)..sx_end {
                    let wx = (x1.min(sx as f64 + 1.0) - x0.max(sx as f64)).max(0.0);
                    let wgt = wx * wy;
                    if wgt <= 0.0 {
                        continue;
                    }
                    let p = img.get_pixel(sx, sy).0;
                    for c in 0..4 {
                        acc[c] += p[c] as f64 * wgt;
                    }
                    total += wgt;
                }
            }
            let px = if total > 0.0 {
                [0, 1, 2, 3].map(|c| (acc[c] / total).round().clamp(0.0, 255.0) as u8)
            } else {
                [0; 4]
            };
            out.push(px);
        }
    }
    out
}

fn purple_hue_score(r: i64, g: i64, b: i64) -> f64 {
    let hue = rgb_hue(r, g, b);
    if (230.0..=295.0).contains(&hue) {
        1.35
    } else if (295.0..=340.0).contains(&hue) {
        0.45
    } else {
        0.85
    }
}

fn rgb_hue(r: i64, g: i64, b: i64) -> f64 {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta < 0.00001 {
        return 0.0;
    }
    let mut hue = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    if hue < 0.0 {
        hue += 360.0;
    }
    hue
}

/// `BrandColorService::sanitizeBrandPrimary`: merah muda atau terlalu terang diganti default.
pub fn sanitize_brand_primary(hex: &str) -> String {
    let hex = normalize_hex(hex);
    if is_pinkish(&hex) || relative_luminance(&hex) > 0.42 {
        return DEFAULT_PRIMARY.to_string();
    }
    hex
}

pub fn is_valid_hex(color: &str) -> bool {
    let s = color.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    (s.len() == 3 || s.len() == 6) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `normalizeHex`: buang `#` di depan, perluas 3 digit, lalu huruf kecil dengan `#`.
pub fn normalize_hex(color: &str) -> String {
    let trimmed = color.trim().trim_start_matches('#');
    let hex: String = if trimmed.len() == 3 {
        trimmed.chars().flat_map(|c| [c, c]).collect()
    } else {
        trimmed.to_string()
    };
    format!("#{}", hex.to_ascii_lowercase())
}

fn rgb_of(hex: &str) -> (i64, i64, i64) {
    let h = normalize_hex(hex);
    let h = h.trim_start_matches('#');
    let p = |i: usize| i64::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    (p(0), p(2), p(4))
}

fn hex_of(r: i64, g: i64, b: i64) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// `darken`: kalikan kanal dengan `1 - ratio`.
pub fn darken(hex: &str, ratio: f64) -> String {
    let (r, g, b) = rgb_of(hex);
    let factor = (1.0 - ratio).clamp(0.0, 1.0);
    hex_of(
        (r as f64 * factor).round() as i64,
        (g as f64 * factor).round() as i64,
        (b as f64 * factor).round() as i64,
    )
}

/// `mixWithWhite`: campur dengan putih sebesar `white_ratio`.
pub fn mix_with_white(hex: &str, white_ratio: f64) -> String {
    let (r, g, b) = rgb_of(hex);
    let white = white_ratio.clamp(0.0, 1.0);
    let color = 1.0 - white;
    let mix = |c: i64| (c as f64 * color + 255.0 * white).round() as i64;
    hex_of(mix(r), mix(g), mix(b))
}

/// `paletteFromPrimary`.
pub fn palette_from_primary(primary: &str) -> Palette {
    let primary = normalize_hex(primary);
    let secondary = darken(&primary, 0.22);
    Palette {
        primary_dark: darken(&primary, 0.14),
        primary_light: mix_with_white(&primary, 0.78),
        primary_fixed: mix_with_white(&primary, 0.88),
        primary_container: mix_with_white(&primary, 0.86),
        secondary_fixed: mix_with_white(&secondary, 0.78),
        on_secondary: "#ffffff".to_string(),
        on_secondary_fixed: darken(&secondary, 0.08),
        surface: mix_with_white(&primary, 0.965),
        surface_container: mix_with_white(&primary, 0.9),
        surface_container_low: mix_with_white(&primary, 0.93),
        outline_variant: mix_with_white(&primary, 0.72),
        on_surface: "#1f1926".to_string(),
        on_surface_variant: "#50434b".to_string(),
        primary,
        secondary,
    }
}

fn is_pinkish(hex: &str) -> bool {
    let (r, _g, b) = rgb_of(hex);
    let (rf, gf, bf) = (r, _g, b);
    let hue = rgb_hue(rf, gf, bf);
    (300.0..=360.0).contains(&hue) && r > b
}

fn relative_luminance(hex: &str) -> f64 {
    let (r, g, b) = rgb_of(hex);
    let lin = |c: i64| {
        let v = c as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn is_neutral_hex(hex: &str) -> bool {
    let (r, g, b) = rgb_of(hex);
    is_neutral_rgb(r, g, b)
}

fn is_neutral_rgb(r: i64, g: i64, b: i64) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max - min < 28 {
        return true;
    }
    if max > 238 && min > 205 {
        return true;
    }
    max < 36
}
