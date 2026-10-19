//! Pemilihan model AI dari halaman Pengaturan. URL dan API key tetap dari env (`AI_BASE_URL`, `AI_API_KEY`),
//! jadi key tidak pernah sampai ke browser. Model yang dipilih disimpan di `app_settings` dengan kunci `ai_model`.
//! Bila belum ada, model dari env `AI_MODEL` dipakai.

use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{ai::AiConfig, media::internal, notifications::require_admin, require_auth, AppState};

/// Kunci `app_settings` untuk model yang dipilih.
pub const KUNCI_MODEL: &str = "ai_model";
/// Batas panjang nama model.
const MODEL_MAKS: usize = 200;

/// Model yang tersimpan di database, bila ada dan tidak kosong.
async fn model_tersimpan(pool: &MySqlPool) -> Result<Option<String>, ApiError> {
    let nilai: Option<Option<String>> =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1")
            .bind(KUNCI_MODEL)
            .fetch_optional(pool)
            .await
            .map_err(internal)?;
    Ok(nilai.flatten().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
}

/// Model aktif: pilihan dari Pengaturan, atau `AI_MODEL` bila belum ada pilihan.
pub async fn model_aktif(pool: &MySqlPool) -> Result<String, ApiError> {
    if let Some(m) = model_tersimpan(pool).await? {
        return Ok(m);
    }
    Ok(std::env::var("AI_MODEL").unwrap_or_default().trim().to_string())
}

/// Konfigurasi AI untuk satu permintaan. Dipanggil setiap kali, jadi pilihan baru langsung berlaku.
pub(crate) async fn ai_config_untuk(pool: &MySqlPool) -> Result<AiConfig, ApiError> {
    crate::ai::ai_config_dasar(model_aktif(pool).await?)
}

/// Mengambil id model dari `GET {base}/models` (format OpenAI). Hasil diurutkan dan tanpa duplikat.
pub(crate) fn daftar_model_dari(payload: &Value) -> Vec<String> {
    let mut ids: Vec<String> = payload["data"]
        .as_array()
        .map(|d| {
            d.iter()
                .filter_map(|m| m["id"].as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.chars().count() <= MODEL_MAKS)
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

/// Pilihan hanya diterima bila ada di daftar yang dikembalikan layanan AI.
pub(crate) fn model_ada_di_daftar(pilihan: &str, daftar: &[String]) -> bool {
    daftar.iter().any(|m| m == pilihan)
}

async fn ambil_daftar() -> Result<Vec<String>, ApiError> {
    let (base_url, api_key) = crate::ai::ai_endpoint()?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(internal)?;
    let mut req = client.get(format!("{base_url}/models"));
    if let Some(key) = &api_key {
        req = req.bearer_auth(key);
    }
    let response = req
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Gagal terhubung ke layanan AI: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("Layanan AI menolak permintaan daftar model ({status})."),
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "Daftar model dari layanan AI tidak valid."))?;
    let daftar = daftar_model_dari(&payload);
    if daftar.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, "Layanan AI tidak mengembalikan model."));
    }
    Ok(daftar)
}

/// `GET /api/ai/models` (admin): daftar model dari layanan AI dan model yang aktif sekarang.
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let daftar = ambil_daftar().await?;
    let aktif = model_aktif(&state.pool).await?;
    Ok(Json(json!({
        "model_aktif": aktif,
        "data": daftar.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>(),
    })))
}

/// `PUT /api/ai/model` (admin): simpan model. Model harus ada di daftar layanan AI saat ini.
pub async fn set(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    let pilihan = body["model"].as_str().unwrap_or("").trim().to_string();
    if pilihan.is_empty() || pilihan.chars().count() > MODEL_MAKS {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Pilih model dari daftar."));
    }
    let daftar = ambil_daftar().await?;
    if !model_ada_di_daftar(&pilihan, &daftar) {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Model itu tidak ada di layanan AI."));
    }

    sqlx::query(
        "INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) \
         VALUES (?, ?, 'text', NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE `value` = VALUES(`value`), updated_at = NOW()",
    )
    .bind(KUNCI_MODEL)
    .bind(&pilihan)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    eprintln!("[AI] model diubah ke {pilihan} oleh admin {}", user.user_id);
    Ok(Json(json!({ "model_aktif": pilihan })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daftar_model_diurutkan_tanpa_duplikat() {
        let payload = json!({ "data": [{"id": "b"}, {"id": "a"}, {"id": "b"}, {"id": ""}, {"x": 1}] });
        assert_eq!(daftar_model_dari(&payload), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn model_di_luar_daftar_ditolak() {
        let daftar = vec!["ag/gemini-3.8-flash".to_string()];
        assert!(model_ada_di_daftar("ag/gemini-3.8-flash", &daftar));
        assert!(!model_ada_di_daftar("ag/gemini-pro-agent", &daftar));
    }
}
