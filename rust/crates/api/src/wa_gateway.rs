//! Pengiriman WhatsApp lewat open-wa (`wa-api`). Hanya dipakai backend. Kunci API tidak pernah dikirim ke klien.
//!
//! Konfigurasi dari environment: `WA_API_URL` (mis. `https://wa-api.cianjur.space`), `WA_API_KEY`,
//! dan `WA_SESSION_ID` (sesi WhatsApp yang sudah tersambung). Bila salah satunya kosong, fitur
//! kirim dinonaktifkan dengan 503, tidak ada yang dikirim.

use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{media::internal, require_auth, AppState};

/// Batas panjang teks, sama dengan `maxLength` pada `SendTextMessageDto` di open-wa.
pub const TEKS_MAKS: usize = 4096;
/// Jeda antar pesan broadcast, supaya layanan WhatsApp tidak dibanjiri.
const JEDA_BROADCAST: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub struct WaConfig {
    pub base_url: String,
    pub api_key: String,
    pub session_id: String,
}

fn env_nonkosong(nama: &str) -> Option<String> {
    std::env::var(nama).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Konfigurasi dari environment. `None` bila salah satu variabel belum diisi.
pub fn config() -> Option<WaConfig> {
    Some(WaConfig {
        base_url: env_nonkosong("WA_API_URL")?.trim_end_matches('/').to_string(),
        api_key: env_nonkosong("WA_API_KEY")?,
        session_id: env_nonkosong("WA_SESSION_ID")?,
    })
}

/// Chat ID perorangan di open-wa: `628...@c.us`. Nomor harus sudah dinormalisasi ke format `62...`.
pub fn chat_id(nomor_62: &str) -> String {
    format!("{nomor_62}@c.us")
}

/// Mengirim satu pesan teks ke nomor perorangan (`62...`).
pub async fn kirim_teks(cfg: &WaConfig, nomor_62: &str, teks: &str) -> Result<(), ApiError> {
    kirim_ke_chat(cfg, &chat_id(nomor_62), teks).await
}

/// Mengunduh media sebuah pesan dari open-wa (`GET .../messages/{chatId}/{messageId}/media`).
/// Menolak berkas lebih besar dari batas media, supaya memori tidak habis oleh satu pesan.
pub async fn unduh_media(cfg: &WaConfig, chat_id: &str, message_id: &str) -> Result<Vec<u8>, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(internal)?;
    let url = format!(
        "{}/api/sessions/{}/messages/{}/{}/media",
        cfg.base_url,
        cfg.session_id,
        encode_segmen(chat_id),
        encode_segmen(message_id)
    );
    let response = client
        .get(url)
        .header("X-API-Key", &cfg.api_key)
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Gagal terhubung ke layanan WhatsApp: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, format!("Media tidak bisa diunduh (status {}).", response.status())));
    }
    if response.content_length().is_some_and(|n| n as usize > crate::media::MAX_FILE_BYTES) {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Media terlalu besar untuk disimpan."));
    }
    let bytes = response.bytes().await.map_err(internal)?;
    if bytes.is_empty() || bytes.len() > crate::media::MAX_FILE_BYTES {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "Media kosong atau terlalu besar."));
    }
    Ok(bytes.to_vec())
}

/// Mengkodekan satu segmen path URL. Karakter selain huruf, angka, dan `-_.~` diubah ke persen-kode.
fn encode_segmen(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Mengirim satu pesan teks ke chat ID apa pun: perorangan (`@c.us`) atau grup (`@g.us`).
pub async fn kirim_ke_chat(cfg: &WaConfig, chat_id: &str, teks: &str) -> Result<(), ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(internal)?;
    let url = format!("{}/api/sessions/{}/messages/send-text", cfg.base_url, cfg.session_id);
    let response = client
        .post(url)
        .header("X-API-Key", &cfg.api_key)
        .json(&json!({ "chatId": chat_id, "text": teks }))
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Gagal terhubung ke layanan WhatsApp: {e}")))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    let pesan = payload["message"].as_str().unwrap_or("permintaan gagal");
    let pesan: String = pesan.chars().take(200).collect();
    Err(ApiError::new(StatusCode::BAD_GATEWAY, format!("Layanan WhatsApp menolak pesan ({status}): {pesan}")))
}

/// Memecah teks panjang menjadi beberapa pesan, masing-masing paling banyak `maks` karakter.
pub fn potong_pesan(teks: &str, maks: usize) -> Vec<String> {
    let kar: Vec<char> = teks.chars().collect();
    kar.chunks(maks.max(1)).map(|c| c.iter().collect()).collect()
}

/// Teks broadcast untuk WhatsApp: judul tebal, isi polos (tag HTML dibuang), lalu tautan bila ada.
pub fn pesan_broadcast(title: &str, message: &str, url: Option<&str>, frontend_url: &str) -> String {
    let isi = if message.contains('<') { hapus_tag(message) } else { message.trim().to_string() };
    let mut out = format!("*{}*\n\n{}", title.trim(), isi);
    if let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) {
        let tautan = if u.starts_with("http://") || u.starts_with("https://") {
            u.to_string()
        } else {
            format!("{}/{}", frontend_url.trim_end_matches('/'), u.trim_start_matches('/'))
        };
        out.push_str(&format!("\n\n{tautan}"));
    }
    out.chars().take(TEKS_MAKS).collect()
}

/// Membuang tag HTML dan mengganti beberapa entitas umum. Cukup untuk isi dari editor.
fn hapus_tag(html: &str) -> String {
    let mut teks = String::with_capacity(html.len());
    let mut dalam_tag = false;
    for c in html.chars() {
        match c {
            '<' => dalam_tag = true,
            '>' if dalam_tag => {
                dalam_tag = false;
                if !teks.ends_with('\n') {
                    teks.push('\n');
                }
            }
            _ if !dalam_tag => teks.push(c),
            _ => {}
        }
    }
    teks.replace("&nbsp;", " ").replace("&amp;", "&").trim().to_string()
}

/// Menyiapkan broadcast WhatsApp: nomor penerima diambil sekarang, lalu pengiriman berjalan di latar
/// belakang. Mengembalikan jumlah yang masuk antrean dan jumlah penerima tanpa nomor.
pub async fn antre_broadcast(
    pool: &MySqlPool,
    cfg: WaConfig,
    ids: Vec<u64>,
    title: String,
    message: String,
    url: Option<String>,
) -> Result<(usize, usize), sqlx::Error> {
    if ids.is_empty() {
        return Ok((0, 0));
    }
    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT whatsapp FROM users WHERE whatsapp IS NOT NULL AND whatsapp <> '' AND id IN (",
    );
    let mut pemisah = qb.separated(", ");
    for id in &ids {
        pemisah.push_bind(*id);
    }
    pemisah.push_unseparated(")");
    let nomor: Vec<String> = qb.build_query_scalar().fetch_all(pool).await?;

    let antrean = nomor.len();
    let tanpa_nomor = ids.len().saturating_sub(antrean);
    let frontend_url = std::env::var("FRONTEND_URL").unwrap_or_default();
    let teks = pesan_broadcast(&title, &message, url.as_deref(), &frontend_url);

    tokio::spawn(async move {
        let (mut terkirim, mut gagal) = (0usize, 0usize);
        for n in &nomor {
            match kirim_teks(&cfg, n, &teks).await {
                Ok(()) => terkirim += 1,
                Err(e) => {
                    gagal += 1;
                    eprintln!("[WA] broadcast gagal: {}", e.message);
                }
            }
            tokio::time::sleep(JEDA_BROADCAST).await;
        }
        eprintln!("[WA] broadcast selesai: {terkirim} terkirim, {gagal} gagal");
    });
    Ok((antrean, tanpa_nomor))
}

/// `POST /api/users/{id}/whatsapp/kirim` (admin): kirim pesan teks ke nomor WhatsApp user.
pub async fn kirim_ke_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<u64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let admin = require_auth(&state, &headers).await?;
    crate::notifications::require_admin(&state.pool, admin.user_id).await?;

    let teks = body["pesan"].as_str().unwrap_or("").trim().to_string();
    if teks.is_empty() {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Pesan wajib diisi."));
    }
    if teks.chars().count() > TEKS_MAKS {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Pesan maksimal 4.096 karakter."));
    }

    let row = sqlx::query("SELECT whatsapp FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "User tidak ditemukan."))?;
    let nomor: Option<String> = row.try_get("whatsapp").map_err(internal)?;
    let nomor = nomor
        .filter(|n| !n.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "User belum punya nomor WhatsApp."))?;

    let cfg = config().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Pengiriman WhatsApp belum dikonfigurasi. Set WA_API_URL, WA_API_KEY, dan WA_SESSION_ID.",
        )
    })?;
    kirim_teks(&cfg, &nomor, &teks).await?;
    eprintln!("[WA] pesan terkirim ke user {user_id} oleh admin {}", admin.user_id);
    Ok(Json(json!({ "status": "terkirim" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_id_memakai_akhiran_perorangan() {
        assert_eq!(chat_id("6281234567890"), "6281234567890@c.us");
    }

    #[test]
    fn pesan_broadcast_membuang_html_dan_menambah_tautan() {
        let teks = pesan_broadcast(
            "Pemeliharaan",
            "<p>Sistem &amp; data</p><p>Besok pukul 20.00</p>",
            Some("/informasi/1"),
            "https://arumanis.cianjur.space/",
        );
        assert!(teks.starts_with("*Pemeliharaan*\n\n"));
        assert!(teks.contains("Sistem & data"));
        assert!(!teks.contains('<'));
        assert!(teks.ends_with("https://arumanis.cianjur.space/informasi/1"));
    }

    #[test]
    fn pesan_broadcast_dipotong_sesuai_batas() {
        let panjang = "a".repeat(TEKS_MAKS + 50);
        assert_eq!(pesan_broadcast("Judul", &panjang, None, "").chars().count(), TEKS_MAKS);
    }

    #[test]
    fn config_kosong_jika_salah_satu_variabel_belum_diisi() {
        // Tidak menyentuh environment proses lain: variabel ini tidak pernah diset di tes.
        std::env::remove_var("WA_API_URL");
        assert!(config().is_none());
    }
}
