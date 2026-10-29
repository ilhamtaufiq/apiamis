//! Model AI yang dipakai asisten WhatsApp. URL dan API key tetap dari env (`AI_BASE_URL`, `AI_API_KEY`).
//! Model dibaca dari `app_settings` dengan kunci `ai_model`. Bila belum ada, model dari env `AI_MODEL` dipakai.

use sqlx::MySqlPool;

use crate::ai::AiConfig;
use shared::ApiError;

/// Kunci `app_settings` untuk model yang dipilih.
pub const KUNCI_MODEL: &str = "ai_model";

/// Model yang tersimpan di database, bila ada dan tidak kosong.
async fn model_tersimpan(pool: &MySqlPool) -> Result<Option<String>, ApiError> {
    let nilai: Option<Option<String>> =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1")
            .bind(KUNCI_MODEL)
            .fetch_optional(pool)
            .await
            .map_err(crate::media::internal)?;
    Ok(nilai.flatten().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
}

/// Model aktif: pilihan tersimpan, atau `AI_MODEL` bila belum ada pilihan.
pub async fn model_aktif(pool: &MySqlPool) -> Result<String, ApiError> {
    if let Some(m) = model_tersimpan(pool).await? {
        return Ok(m);
    }
    Ok(std::env::var("AI_MODEL").unwrap_or_default().trim().to_string())
}

/// Konfigurasi AI untuk satu permintaan. Dipanggil setiap kali, jadi perubahan langsung berlaku.
pub(crate) async fn ai_config_untuk(pool: &MySqlPool) -> Result<AiConfig, ApiError> {
    crate::ai::ai_config_dasar(model_aktif(pool).await?)
}
