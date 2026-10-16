//! Fitur AI: status konfigurasi, analisa dashboard, dan tanya-jawab data.
//!
//! Rute:
//! - `GET /api/ai/status`: apakah AI sudah dikonfigurasi dan model apa yang dipakai. Key tidak pernah dikirim.
//! - `POST /api/ai/analyze`: analisa dari ringkasan angka dashboard.
//! - `POST /api/ai/tanya`: tanya-jawab. Model memilih satu tool dari daftar tetap, backend menjalankannya
//!   dengan hak akses pengguna (`access::restriction`), lalu model merangkum hasilnya.
//!
//! Konfigurasi env: `AI_BASE_URL` (mis. `https://host/v1`), `AI_API_KEY`, `AI_MODEL`.
//! Setiap pemanggilan dicatat di `ai_logs`, termasuk pertanyaannya. Batas 20 permintaan per jam per pengguna
//! (per proses, memakai `state.limiter`).

use std::time::{Duration, Instant};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    access,
    media::internal,
    pekerjaan::{self, PekerjaanFilter},
    require_auth, AppState,
};

/// Batas permintaan AI per pengguna per jam.
const AI_MAX_PER_HOUR: usize = 20;
/// Batas ukuran ringkasan dashboard yang dikirim ke model.
const RINGKASAN_MAX_CHARS: usize = 12_000;
/// Batas data hasil tool yang dikirim ke model.
const DATA_MAX_CHARS: usize = 8_000;
/// Jumlah baris maksimum yang dipindai tool progres. Bila terlampaui, jawaban menyebutkannya.
const PINDAI_MAX: u64 = 500;
/// Jumlah hasil maksimum yang ditampilkan tool pencarian dan progres.
const HASIL_MAX: usize = 20;

struct AiConfig {
    base_url: String,
    api_key: Option<String>,
    model: String,
}

fn valid_base_url(v: &str) -> bool {
    (v.starts_with("https://") || v.starts_with("http://")) && !v.contains(char::is_whitespace)
}

/// Membaca konfigurasi dari env setiap permintaan, supaya perubahan env tidak perlu restart.
fn ai_config() -> Result<AiConfig, ApiError> {
    let base_url = std::env::var("AI_BASE_URL").unwrap_or_default().trim().trim_end_matches('/').to_string();
    let model = std::env::var("AI_MODEL").unwrap_or_default().trim().to_string();
    let api_key = std::env::var("AI_API_KEY")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if base_url.is_empty() || model.is_empty() || !valid_base_url(&base_url) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Fitur AI belum dikonfigurasi. Set AI_BASE_URL dan AI_MODEL di server.",
        ));
    }
    Ok(AiConfig { base_url, api_key, model })
}

fn cek_batas(state: &AppState, user_id: u64) -> Result<(), ApiError> {
    state
        .limiter
        .hit(&format!("ai:{user_id}"), AI_MAX_PER_HOUR, Duration::from_secs(3600))
        .map_err(|detik| {
            ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                format!("Batas analisa AI tercapai ({AI_MAX_PER_HOUR} per jam). Coba lagi dalam {} menit.", detik / 60 + 1),
            )
        })
}

/// Chat completion ke layanan OpenAI-compatible. Pesan galat dari layanan dipotong agar tidak panjang.
async fn panggil_model(cfg: &AiConfig, messages: Value, max_tokens: u32) -> Result<String, ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(internal)?;
    let mut req = client
        .post(format!("{}/chat/completions", cfg.base_url))
        .json(&json!({
            "model": cfg.model,
            "temperature": 0.1,
            "max_tokens": max_tokens,
            "messages": messages,
        }));
    if let Some(key) = &cfg.api_key {
        req = req.bearer_auth(key);
    }
    let response = req
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Gagal terhubung ke layanan AI: {e}")))?;
    let status = response.status();
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let pesan = payload["error"]["message"]
            .as_str()
            .or_else(|| payload["message"].as_str())
            .unwrap_or("permintaan gagal");
        let pesan: String = pesan.chars().take(200).collect();
        return Err(ApiError::new(StatusCode::BAD_GATEWAY, format!("Layanan AI gagal: {pesan}")));
    }
    payload["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_GATEWAY, "Model tidak mengembalikan jawaban."))
}

/// Ambil JSON dari balasan model, termasuk bila dibungkus pagar kode.
fn parse_json_dari_model(text: &str) -> Option<Value> {
    let bersih = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str::<Value>(bersih).ok().filter(|v| v.is_object())
}

/// Hanya angka dan teks pendek dari ringkasan yang dikirim ke model.
pub fn sanitize_ringkasan(raw: &Value) -> Option<Value> {
    let obj = raw.as_object()?;
    let statistik: serde_json::Map<String, Value> = obj
        .get("statistik")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.as_f64().is_some_and(f64::is_finite))
                .take(20)
                .map(|(k, v)| (k.chars().take(64).collect(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let perhatian: Vec<Value> = obj
        .get("pekerjaanPerluPerhatian")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .take(10)
                .filter_map(|item| {
                    let p = item.as_object()?;
                    let angka = |k: &str| p.get(k).and_then(Value::as_f64).filter(|f| f.is_finite()).unwrap_or(0.0);
                    let nama: String = p.get("nama").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
                    Some(json!({ "nama": nama, "fisik": angka("fisik"), "keuangan": angka("keuangan") }))
                })
                .collect()
        })
        .unwrap_or_default();
    let tahun: String = obj.get("tahun").and_then(Value::as_str).unwrap_or("").chars().take(4).collect();
    Some(json!({ "tahun": tahun, "statistik": statistik, "pekerjaanPerluPerhatian": perhatian }))
}

/// Catat pemanggilan AI. Kegagalan mencatat tidak menggagalkan permintaan pengguna.
#[allow(clippy::too_many_arguments)]
async fn catat(
    pool: &MySqlPool,
    user_id: u64,
    fitur: &str,
    tool: Option<&str>,
    args: &Value,
    pertanyaan: Option<&str>,
    jumlah_baris: Option<i64>,
    model: &str,
    durasi_ms: u128,
    status: &str,
) {
    let res = sqlx::query(
        "INSERT INTO ai_logs (user_id, fitur, tool, args_json, pertanyaan, jumlah_baris, model, durasi_ms, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW())",
    )
    .bind(user_id)
    .bind(fitur)
    .bind(tool)
    .bind(if args.is_null() { None } else { Some(args.to_string()) })
    .bind(pertanyaan)
    .bind(jumlah_baris)
    .bind(model)
    .bind(durasi_ms.min(u32::MAX as u128) as u32)
    .bind(status)
    .execute(pool)
    .await;
    if let Err(e) = res {
        eprintln!("[AI] gagal mencatat ai_logs: {e}");
    }
}

/// `GET /api/ai/status`: konfigurasi AI tanpa membuka key.
pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    match ai_config() {
        Ok(cfg) => Ok(Json(json!({ "configured": true, "model": cfg.model }))),
        Err(_) => Ok(Json(json!({ "configured": false, "model": null }))),
    }
}

/// `POST /api/ai/analyze`: analisa dari ringkasan dashboard.
pub async fn analyze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let cfg = ai_config()?;
    cek_batas(&state, user.user_id)?;

    let ringkasan = sanitize_ringkasan(&body["ringkasan"])
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "Ringkasan dashboard tidak valid."))?;
    let serialized = ringkasan.to_string();
    if serialized.len() > RINGKASAN_MAX_CHARS {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "Ringkasan dashboard terlalu besar."));
    }

    let mulai = Instant::now();
    let hasil = panggil_model(
        &cfg,
        json!([
            {
                "role": "system",
                "content": "Anda analis progres pembangunan infrastruktur air minum dan sanitasi. Tulis analisa singkat dalam Bahasa Indonesia berisi: (1) kondisi umum, (2) 2-3 temuan utama, (3) 2-3 rekomendasi tindakan. Gunakan hanya angka dari data yang diberikan dan jangan mengarang data. Jika data tidak cukup untuk suatu kesimpulan, katakan demikian."
            },
            { "role": "user", "content": format!("Data dashboard (JSON):\n{serialized}") }
        ]),
        900,
    )
    .await;
    let status = if hasil.is_ok() { "ok" } else { "gagal" };
    catat(&state.pool, user.user_id, "analisa", None, &Value::Null, None, None, &cfg.model, mulai.elapsed().as_millis(), status).await;

    Ok(Json(json!({ "analisa": hasil?, "model": cfg.model })))
}

/// Daftar tool untuk prompt model. Nama dan parameter harus sama dengan `jalankan_tool`.
const DAFTAR_TOOL: &str = "- statistik_dashboard: Jumlah pekerjaan aktif dan total pagu untuk tahun anggaran. Parameter: {}\n\
- pekerjaan_progres_rendah: Pekerjaan aktif dengan progres realisasi di bawah batas, dari yang paling rendah (maksimal 20). Parameter: {\"batas\": angka 0 sampai 100, default 50}\n\
- pekerjaan_cari: Cari pekerjaan aktif berdasarkan kata dalam nama paket (maksimal 10). Parameter: {\"kata\": teks}";

fn filter_tahun(tahun: &str) -> PekerjaanFilter {
    PekerjaanFilter {
        tahun: Some(tahun.to_string()),
        status: Some("active".to_string()),
        ..Default::default()
    }
}

/// Menjalankan tool yang dipilih model dengan hak akses pengguna. Mengembalikan data dan jumlah baris.
async fn jalankan_tool(
    state: &AppState,
    user_id: u64,
    roles: &[(u64, String)],
    nama: &str,
    args: &Value,
    tahun: &str,
) -> Result<(Value, Option<i64>), ApiError> {
    let pool = &state.pool;
    let mut f = filter_tahun(tahun);
    match nama {
        "statistik_dashboard" => {
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut binds) = f.where_clause();
            binds.extend(scope.binds.iter().map(u64::to_string));
            let sql = format!(
                "SELECT COUNT(*) AS n, CAST(COALESCE(SUM(p.pagu), 0) AS DOUBLE) AS pagu FROM tbl_pekerjaan p{where_sql}{}",
                scope.sql
            );
            let mut q = sqlx::query(&sql);
            for b in &binds {
                q = q.bind(b);
            }
            let row = q.fetch_one(pool).await.map_err(internal)?;
            let n: i64 = row.try_get("n").map_err(internal)?;
            let pagu: f64 = row.try_get("pagu").map_err(internal)?;
            Ok((json!({ "jumlahPekerjaanAktif": n, "totalPagu": pagu }), Some(n)))
        }
        "pekerjaan_cari" => {
            let kata = args.get("kata").and_then(Value::as_str).unwrap_or("").trim().chars().take(100).collect::<String>();
            if kata.is_empty() {
                return Ok((json!({ "hasil": [], "jumlahTotal": 0 }), Some(0)));
            }
            f.search = Some(kata);
            let scope = access::restriction(user_id, roles, "p");
            let (rows, total) = pekerjaan::list(pool, &f, &scope, None, Some(10)).await.map_err(internal)?;
            let hasil: Vec<Value> = rows
                .iter()
                .map(|r| json!({ "id": r.id, "nama": r.nama_paket, "pagu": r.pagu, "status": r.status }))
                .collect();
            Ok((json!({ "hasil": hasil, "jumlahTotal": total }), Some(hasil.len() as i64)))
        }
        "pekerjaan_progres_rendah" => {
            let batas = args.get("batas").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(50.0).clamp(0.0, 100.0);
            let scope = access::restriction(user_id, roles, "p");
            let (rows, total) = pekerjaan::list(pool, &f, &scope, None, Some(PINDAI_MAX)).await.map_err(internal)?;
            let ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
            let progres = pekerjaan::progress_for(pool, &ids).await.map_err(internal)?;
            let mut hasil: Vec<(f64, Value)> = rows
                .iter()
                .filter_map(|r| {
                    let fisik = progres.get(&r.id).map(|m| m.progress_total).unwrap_or(0.0);
                    (fisik < batas).then(|| {
                        (fisik, json!({ "id": r.id, "nama": r.nama_paket, "fisik": fisik, "pagu": r.pagu }))
                    })
                })
                .collect();
            hasil.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let hasil: Vec<Value> = hasil.into_iter().take(HASIL_MAX).map(|(_, v)| v).collect();
            let jumlah = hasil.len() as i64;
            Ok((
                json!({
                    "batas": batas,
                    "hasil": hasil,
                    "jumlahPekerjaanAktif": total,
                    "jumlahDipindai": ids.len(),
                    "terpotong": total > ids.len() as u64,
                }),
                Some(jumlah),
            ))
        }
        _ => Err(ApiError::new(StatusCode::BAD_REQUEST, "Tool tidak dikenal.")),
    }
}

/// `POST /api/ai/tanya`: tanya-jawab data dengan tool baca yang sudah ditentukan.
pub async fn tanya(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let cfg = ai_config()?;
    cek_batas(&state, user.user_id)?;

    let pertanyaan = body["pertanyaan"].as_str().unwrap_or("").trim().to_string();
    if pertanyaan.is_empty() || pertanyaan.chars().count() > 500 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Pertanyaan wajib diisi, maksimal 500 karakter."));
    }
    let tahun = body["tahun"].as_str().unwrap_or("").trim().to_string();
    if tahun.len() != 4 || !tahun.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Tahun anggaran tidak valid."));
    }

    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    let mulai = Instant::now();

    // Langkah 1: model memilih tool. Nama yang tidak ada di daftar ditolak.
    let pilihan_text = panggil_model(
        &cfg,
        json!([
            {
                "role": "system",
                "content": format!(
                    "Pilih satu tool untuk menjawab pertanyaan tentang data pembangunan air minum dan sanitasi. \
                     Balas HANYA dengan JSON: {{\"tool\": \"<nama>\", \"args\": {{...}}}} atau {{\"tool\": null, \"alasan\": \"<singkat>\"}} bila tidak ada yang cocok.\nTools:\n{DAFTAR_TOOL}", DAFTAR_TOOL = DAFTAR_TOOL
                )
            },
            { "role": "user", "content": pertanyaan }
        ]),
        200,
    )
    .await?;

    let pilihan = parse_json_dari_model(&pilihan_text);
    let nama = pilihan.as_ref().and_then(|p| p["tool"].as_str()).map(str::to_string);
    let nama_valid = matches!(nama.as_deref(), Some("statistik_dashboard" | "pekerjaan_progres_rendah" | "pekerjaan_cari"));
    if !nama_valid {
        catat(&state.pool, user.user_id, "tanya", None, &Value::Null, Some(&pertanyaan), None, &cfg.model, mulai.elapsed().as_millis(), "tidak_cocok").await;
        return Ok(Json(json!({
            "jawaban": "Pertanyaan ini belum bisa dijawab dari data yang tersedia. Coba tanyakan tentang ringkasan dashboard, progres pekerjaan rendah, atau mencari pekerjaan dengan nama tertentu.",
            "sumber": null,
            "model": cfg.model,
        })));
    }
    let nama = nama.unwrap_or_default();
    let args = pilihan.as_ref().and_then(|p| p.get("args")).cloned().unwrap_or(json!({}));

    // Langkah 2: jalankan tool dengan hak akses pengguna, lalu model merangkum hasilnya.
    let (data, jumlah) = match jalankan_tool(&state, user.user_id, &roles, &nama, &args, &tahun).await {
        Ok(v) => v,
        Err(e) => {
            catat(&state.pool, user.user_id, "tanya", Some(&nama), &args, Some(&pertanyaan), None, &cfg.model, mulai.elapsed().as_millis(), "gagal").await;
            return Err(e);
        }
    };
    let data_text = data.to_string();
    let data_text: String = data_text.chars().take(DATA_MAX_CHARS).collect();

    let jawaban = panggil_model(
        &cfg,
        json!([
            {
                "role": "system",
                "content": "Jawab pertanyaan dalam Bahasa Indonesia HANYA berdasarkan data JSON yang diberikan. Sebut angka persis seperti di data. Bila data bersifat terpotong (terpotong: true) atau tidak menjawab pertanyaan, katakan itu. Jangan mengarang."
            },
            { "role": "user", "content": format!("Pertanyaan: {pertanyaan}\n\nData ({nama}):\n{data_text}") }
        ]),
        700,
    )
    .await;
    let status = if jawaban.is_ok() { "ok" } else { "gagal" };
    catat(&state.pool, user.user_id, "tanya", Some(&nama), &args, Some(&pertanyaan), jumlah, &cfg.model, mulai.elapsed().as_millis(), status).await;

    Ok(Json(json!({
        "jawaban": jawaban?,
        "sumber": { "tool": nama, "args": args, "jumlah_baris": jumlah },
        "model": cfg.model,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_base_url_hanya_http_dan_https_tanpa_spasi() {
        assert!(valid_base_url("https://ai.example.com/v1"));
        assert!(valid_base_url("http://127.0.0.1:8000/v1"));
        assert!(!valid_base_url("ftp://x/v1"));
        assert!(!valid_base_url("https://x y/v1"));
        assert!(!valid_base_url("x"));
    }

    #[test]
    fn parse_json_dari_model_menerima_pagar_kode() {
        let v = parse_json_dari_model("```json\n{\"tool\":\"pekerjaan_cari\",\"args\":{\"kata\":\"air\"}}\n```").unwrap();
        assert_eq!(v["tool"], "pekerjaan_cari");
        assert!(parse_json_dari_model("bukan json").is_none());
        assert!(parse_json_dari_model("[1,2]").is_none());
    }

    #[test]
    fn sanitize_ringkasan_membuang_nilai_tak_valid_dan_memotong_teks() {
        let raw = json!({
            "tahun": "2026xxxx",
            "statistik": { "totalPagu": 1000.5, "rusak": "abc", "nan": null },
            "pekerjaanPerluPerhatian": [{ "nama": "A".repeat(300), "fisik": 12, "keuangan": "x" }],
        });
        let s = sanitize_ringkasan(&raw).unwrap();
        assert_eq!(s["tahun"], "2026");
        assert_eq!(s["statistik"].as_object().unwrap().len(), 1);
        assert_eq!(s["pekerjaanPerluPerhatian"][0]["nama"].as_str().unwrap().chars().count(), 200);
        assert_eq!(s["pekerjaanPerluPerhatian"][0]["keuangan"], 0.0);
    }

    #[test]
    fn sanitize_ringkasan_menolak_bukan_objek() {
        assert!(sanitize_ringkasan(&json!("teks")).is_none());
        assert!(sanitize_ringkasan(&Value::Null).is_none());
    }
}
