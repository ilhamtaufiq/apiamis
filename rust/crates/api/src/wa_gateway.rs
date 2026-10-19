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
use sqlx::Row;

use crate::{media::internal, require_auth, AppState};

/// Batas panjang teks, sama dengan `maxLength` pada `SendTextMessageDto` di open-wa.
pub const TEKS_MAKS: usize = 4096;

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

/// Mengirim satu pesan teks. Mengembalikan galat yang bisa ditampilkan bila open-wa menolak.
pub async fn kirim_teks(cfg: &WaConfig, nomor_62: &str, teks: &str) -> Result<(), ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(internal)?;
    let url = format!("{}/api/sessions/{}/messages/send-text", cfg.base_url, cfg.session_id);
    let response = client
        .post(url)
        .header("X-API-Key", &cfg.api_key)
        .json(&json!({ "chatId": chat_id(nomor_62), "text": teks }))
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
    fn config_kosong_jika_salah_satu_variabel_belum_diisi() {
        // Tidak menyentuh environment proses lain: variabel ini tidak pernah diset di tes.
        std::env::remove_var("WA_API_URL");
        assert!(config().is_none());
    }
}
