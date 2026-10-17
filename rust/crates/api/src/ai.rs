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

use std::convert::Infallible;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
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

#[derive(Clone)]
pub(crate) struct AiConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: Option<String>,
    pub(crate) model: String,
}

fn valid_base_url(v: &str) -> bool {
    (v.starts_with("https://") || v.starts_with("http://")) && !v.contains(char::is_whitespace)
}

/// Membaca konfigurasi dari env setiap permintaan, supaya perubahan env tidak perlu restart.
pub(crate) fn ai_config() -> Result<AiConfig, ApiError> {
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
/// Konfigurasi untuk ringkasan memori: model murah dari `AI_COMPACT_MODEL` bila diset, selain itu model utama.
pub(crate) fn compact_config(cfg: &AiConfig) -> AiConfig {
    let model = std::env::var("AI_COMPACT_MODEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| cfg.model.clone());
    AiConfig { base_url: cfg.base_url.clone(), api_key: cfg.api_key.clone(), model }
}

pub(crate) async fn panggil_model(cfg: &AiConfig, messages: Value, max_tokens: u32) -> Result<String, ApiError> {
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
pub(crate) fn parse_json_dari_model(text: &str) -> Option<Value> {
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

/// Daftar tool yang boleh dipilih model: (nama, deskripsi untuk prompt, parameter, hanya admin).
/// Nama dan parameter harus sama dengan dispatch di `jalankan_tool`.
pub(crate) const TOOLS: &[(&str, &str, &str, bool)] = &[
    ("statistik_dashboard", "Jumlah pekerjaan aktif dan total pagu untuk tahun anggaran.", "{}", false),
    ("pekerjaan_progres_rendah", "Pekerjaan aktif dengan progres realisasi di bawah batas, dari yang paling rendah (maksimal 20).", "{\"batas\": angka 0 sampai 100, default 50}", false),
    ("pekerjaan_cari", "Cari pekerjaan aktif berdasarkan kata dalam nama paket (maksimal 10).", "{\"kata\": teks}", false),
    ("kontrak_ringkasan", "Jumlah pekerjaan aktif yang sudah berkontrak, jumlah kontrak dan total nilainya, serta kontrak yang selesai dalam 90 hari ke depan (maksimal 10 daftar).", "{}", false),
    ("realisasi_keuangan", "Total realisasi keuangan (SP2D) dan 10 pekerjaan dengan realisasi terbesar, serta 10 SP2D terbaru lengkap dengan nomor dan tanggalnya. Bisa dibatasi periode tanggal.", "{\"dari\": \"YYYY-MM-DD opsional\", \"sampai\": \"YYYY-MM-DD opsional\"}", false),
    ("rekap_kecamatan", "Jumlah pekerjaan aktif dan pagu per kecamatan (maksimal 50 kecamatan).", "{}", false),
    ("penerima_agregat", "Jumlah penerima dan jumlah jiwa per desa. Hanya agregat: nama, NIK, dan alamat tidak pernah ditampilkan, dan desa dengan kurang dari 5 penerima dirahasiakan.", "{}", false),
    ("rekap_desa", "Jumlah pekerjaan aktif dan pagu per desa (maksimal 50 desa), beserta berapa desa yang sudah tercakup dari total desa.", "{}", false),
    ("cari_surat", "Cari surat di tahun anggaran berdasarkan nomor atau perihal (maksimal 10), beserta jumlah surat tahun itu.", "{\"kata\": teks}", false),
    ("kualitas_data", "Ringkasan temuan kualitas data pekerjaan. Hanya untuk admin.", "{}", true),
];

/// Daftar tool untuk prompt, tanpa tool admin bila pengguna bukan admin.
pub(crate) fn daftar_tool(is_admin: bool) -> String {
    TOOLS
        .iter()
        .filter(|t| is_admin || !t.3)
        .map(|t| format!("- {}: {} Parameter: {}", t.0, t.1, t.2))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escape wildcard LIKE agar kata pengguna dicari apa adanya.
fn escape_like(kata: &str) -> String {
    kata.replace('\\', "").replace('%', "").replace('_', "")
}

fn filter_tahun(tahun: &str) -> PekerjaanFilter {
    PekerjaanFilter {
        tahun: Some(tahun.to_string()),
        status: Some("active".to_string()),
        ..Default::default()
    }
}

/// Menjalankan tool yang dipilih model dengan hak akses pengguna. Mengembalikan data dan jumlah baris.
pub(crate) async fn jalankan_tool(
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
        "kontrak_ringkasan" => {
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut binds) = f.where_clause();
            binds.extend(scope.binds.iter().map(u64::to_string));
            let kondisi = format!("k.id_pekerjaan IN (SELECT p.id FROM tbl_pekerjaan p{where_sql}{})", scope.sql);
            // Pekerjaan yang berkontrak: lewat kolom `id_pekerjaan` atau tabel penghubung `kontrak_pekerjaan`.
            let berkontrak = format!(
                "SELECT COUNT(*) FROM tbl_pekerjaan p{where_sql} AND p.id IN \
                 (SELECT k.id_pekerjaan FROM tbl_kontrak k WHERE k.id_pekerjaan IS NOT NULL \
                  UNION SELECT kp.pekerjaan_id FROM kontrak_pekerjaan kp){}",
                scope.sql
            );
            let mut bq = sqlx::query_scalar::<_, i64>(&berkontrak);
            for b in &binds {
                bq = bq.bind(b);
            }
            let jumlah_pekerjaan: i64 = bq.fetch_one(pool).await.map_err(internal)?;

            let sql = format!(
                "SELECT CAST(COUNT(*) AS SIGNED) AS n, CAST(COALESCE(SUM(k.nilai_kontrak), 0) AS DOUBLE) AS total, \
                 CAST(COALESCE(SUM(CASE WHEN k.tgl_selesai BETWEEN CURDATE() AND DATE_ADD(CURDATE(), INTERVAL 90 DAY) THEN 1 ELSE 0 END), 0) AS SIGNED) AS berakhir \
                 FROM tbl_kontrak k WHERE {kondisi}"
            );
            let mut q = sqlx::query(&sql);
            for b in &binds {
                q = q.bind(b);
            }
            let row = q.fetch_one(pool).await.map_err(internal)?;
            let n: i64 = row.try_get("n").map_err(internal)?;
            let total: f64 = row.try_get("total").map_err(internal)?;
            let berakhir: i64 = row.try_get("berakhir").map_err(internal)?;

            let daftar_sql = format!(
                "SELECT k.kode_paket AS kode_paket, CAST(k.nilai_kontrak AS DOUBLE) AS nilai, DATE_FORMAT(k.tgl_selesai, '%Y-%m-%d') AS tgl_selesai \
                 FROM tbl_kontrak k WHERE {kondisi} AND k.tgl_selesai BETWEEN CURDATE() AND DATE_ADD(CURDATE(), INTERVAL 90 DAY) \
                 ORDER BY k.tgl_selesai LIMIT 10"
            );
            let mut dq = sqlx::query(&daftar_sql);
            for b in &binds {
                dq = dq.bind(b);
            }
            let daftar: Vec<Value> = dq
                .fetch_all(pool)
                .await
                .map_err(internal)?
                .iter()
                .map(|r| {
                    json!({
                        "kode_paket": r.try_get::<Option<String>, _>("kode_paket").ok().flatten(),
                        "nilai": r.try_get::<Option<f64>, _>("nilai").ok().flatten(),
                        "tgl_selesai": r.try_get::<Option<String>, _>("tgl_selesai").ok().flatten(),
                    })
                })
                .collect();
            Ok((
                json!({
                    "jumlahPekerjaanBerkontrak": jumlah_pekerjaan,
                    "jumlahKontrak": n,
                    "totalNilaiKontrak": total,
                    "berakhirDalam90Hari": berakhir,
                    "daftarBerakhir": daftar,
                }),
                Some(jumlah_pekerjaan),
            ))
        }
        "realisasi_keuangan" => {
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut where_binds) = f.where_clause();
            where_binds.extend(scope.binds.iter().map(u64::to_string));

            // Periode opsional, dicek sebagai tanggal agar input tidak sembarang teks.
            let tanggal = |k: &str| -> Result<Option<String>, ApiError> {
                match args.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
                    None => Ok(None),
                    Some(v) => chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d")
                        .map(|d| Some(d.format("%Y-%m-%d").to_string()))
                        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, format!("Format {k} harus YYYY-MM-DD."))),
                }
            };
            let dari = tanggal("dari")?;
            let sampai = tanggal("sampai")?;

            let mut kondisi = String::from("h.tipe = 'realisasi' AND h.jenis = 'keuangan' AND h.tahun_anggaran = ?");
            let mut binds: Vec<String> = vec![tahun.to_string()];
            if let Some(d) = &dari {
                kondisi.push_str(" AND h.tanggal >= ?");
                binds.push(d.clone());
            }
            if let Some(d) = &sampai {
                kondisi.push_str(" AND h.tanggal <= ?");
                binds.push(d.clone());
            }
            kondisi.push_str(&format!(" AND h.pekerjaan_id IN (SELECT p.id FROM tbl_pekerjaan p{where_sql}{})", scope.sql));
            binds.extend(where_binds);

            let total_sql = format!("SELECT CAST(COALESCE(SUM(h.nilai), 0) AS DOUBLE) AS total FROM pekerjaan_progress_estimasi_history h WHERE {kondisi}");
            let mut tq = sqlx::query(&total_sql);
            for b in &binds {
                tq = tq.bind(b);
            }
            let total: f64 = tq.fetch_one(pool).await.map_err(internal)?.try_get("total").map_err(internal)?;

            let top_sql = format!(
                "SELECT CAST(h.pekerjaan_id AS SIGNED) AS pid, p.nama_paket AS nama, CAST(SUM(h.nilai) AS DOUBLE) AS total \
                 FROM pekerjaan_progress_estimasi_history h JOIN tbl_pekerjaan p ON p.id = h.pekerjaan_id \
                 WHERE {kondisi} GROUP BY h.pekerjaan_id, p.nama_paket ORDER BY total DESC LIMIT 10"
            );
            let mut hq = sqlx::query(&top_sql);
            for b in &binds {
                hq = hq.bind(b);
            }
            let top: Vec<Value> = hq
                .fetch_all(pool)
                .await
                .map_err(internal)?
                .iter()
                .map(|r| {
                    json!({
                        "id": r.try_get::<i64, _>("pid").ok(),
                        "nama": r.try_get::<Option<String>, _>("nama").ok().flatten(),
                        "realisasi": r.try_get::<f64, _>("total").ok(),
                    })
                })
                .collect();

            let sp2d_sql = format!(
                "SELECT h.nomor_sp2d AS nomor, DATE_FORMAT(h.tanggal, '%Y-%m-%d') AS tanggal, \
                 DATE_FORMAT(h.tanggal_pencairan, '%Y-%m-%d') AS tanggal_pencairan, CAST(h.nilai AS DOUBLE) AS nilai, p.nama_paket AS nama \
                 FROM pekerjaan_progress_estimasi_history h JOIN tbl_pekerjaan p ON p.id = h.pekerjaan_id \
                 WHERE {kondisi} ORDER BY h.tanggal DESC, h.id DESC LIMIT 10"
            );
            let mut sq = sqlx::query(&sp2d_sql);
            for b in &binds {
                sq = sq.bind(b);
            }
            let sp2d: Vec<Value> = sq
                .fetch_all(pool)
                .await
                .map_err(internal)?
                .iter()
                .map(|r| {
                    json!({
                        "nomor_sp2d": r.try_get::<Option<String>, _>("nomor").ok().flatten(),
                        "tanggal": r.try_get::<Option<String>, _>("tanggal").ok().flatten(),
                        "tanggal_pencairan": r.try_get::<Option<String>, _>("tanggal_pencairan").ok().flatten(),
                        "nilai": r.try_get::<Option<f64>, _>("nilai").ok().flatten(),
                        "pekerjaan": r.try_get::<Option<String>, _>("nama").ok().flatten(),
                    })
                })
                .collect();

            let jumlah = sp2d.len() as i64;
            Ok((
                json!({
                    "tahun": tahun,
                    "dari": dari,
                    "sampai": sampai,
                    "totalRealisasiKeuangan": total,
                    "terbesar": top,
                    "sp2dTerbaru": sp2d,
                }),
                Some(jumlah),
            ))
        }
        "rekap_kecamatan" => {
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut binds) = f.where_clause();
            binds.extend(scope.binds.iter().map(u64::to_string));
            let sql = format!(
                "SELECT COALESCE(k.n_kec, '(tanpa kecamatan)') AS nama, CAST(COUNT(*) AS SIGNED) AS jumlah, \
                 CAST(COALESCE(SUM(p.pagu), 0) AS DOUBLE) AS pagu \
                 FROM tbl_pekerjaan p LEFT JOIN tbl_kecamatan k ON k.id = p.kecamatan_id{where_sql}{} \
                 GROUP BY p.kecamatan_id, k.n_kec ORDER BY pagu DESC LIMIT 50",
                scope.sql
            );
            let mut q = sqlx::query(&sql);
            for b in &binds {
                q = q.bind(b);
            }
            let rows = q.fetch_all(pool).await.map_err(internal)?;
            let hasil: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "kecamatan": r.try_get::<Option<String>, _>("nama").ok().flatten(),
                        "jumlahPekerjaan": r.try_get::<i64, _>("jumlah").ok(),
                        "pagu": r.try_get::<f64, _>("pagu").ok(),
                    })
                })
                .collect();
            let jumlah = hasil.len() as i64;
            Ok((json!({ "kecamatan": hasil }), Some(jumlah)))
        }
        "penerima_agregat" => {
            const MIN_PENERIMA: i64 = 5;
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut binds) = f.where_clause();
            binds.extend(scope.binds.iter().map(u64::to_string));
            let kondisi = format!("pr.pekerjaan_id IN (SELECT p.id FROM tbl_pekerjaan p{where_sql}{})", scope.sql);

            // Hanya kolom agregat yang di-select. Kolom identitas tidak disentuh sama sekali.
            let total_sql = format!(
                "SELECT CAST(COUNT(*) AS SIGNED) AS jumlah, CAST(COALESCE(SUM(pr.jumlah_jiwa), 0) AS SIGNED) AS jiwa FROM tbl_penerima pr WHERE {kondisi}"
            );
            let mut tq = sqlx::query(&total_sql);
            for b in &binds {
                tq = tq.bind(b);
            }
            let total = tq.fetch_one(pool).await.map_err(internal)?;
            let total_penerima: i64 = total.try_get("jumlah").map_err(internal)?;
            let total_jiwa: i64 = total.try_get("jiwa").map_err(internal)?;

            let per_desa_sql = format!(
                "SELECT COALESCE(d.n_desa, '(tanpa desa)') AS desa, CAST(COUNT(*) AS SIGNED) AS jumlah, \
                 CAST(COALESCE(SUM(pr.jumlah_jiwa), 0) AS SIGNED) AS jiwa \
                 FROM tbl_penerima pr JOIN tbl_pekerjaan p ON p.id = pr.pekerjaan_id LEFT JOIN tbl_desa d ON d.id = p.desa_id \
                 WHERE {kondisi} GROUP BY p.desa_id, d.n_desa ORDER BY jiwa DESC LIMIT 100"
            );
            let mut dq = sqlx::query(&per_desa_sql);
            for b in &binds {
                dq = dq.bind(b);
            }
            let mut tampil: Vec<Value> = Vec::new();
            let mut disembunyikan = 0_i64;
            for r in dq.fetch_all(pool).await.map_err(internal)? {
                let jumlah: i64 = r.try_get("jumlah").map_err(internal)?;
                if jumlah < MIN_PENERIMA {
                    disembunyikan += 1;
                    continue;
                }
                tampil.push(json!({
                    "desa": r.try_get::<Option<String>, _>("desa").ok().flatten(),
                    "jumlahPenerima": jumlah,
                    "jumlahJiwa": r.try_get::<i64, _>("jiwa").ok(),
                }));
            }
            let jumlah_hasil = tampil.len() as i64;
            Ok((
                json!({
                    "totalPenerima": total_penerima,
                    "totalJiwa": total_jiwa,
                    "perDesa": tampil,
                    "desaDirahasiakan": disembunyikan,
                    "catatan": "Desa dengan kurang dari 5 penerima dirahasiakan.",
                }),
                Some(jumlah_hasil),
            ))
        }
        "rekap_desa" => {
            let scope = access::restriction(user_id, roles, "p");
            let (where_sql, mut binds) = f.where_clause();
            binds.extend(scope.binds.iter().map(u64::to_string));
            let sql = format!(
                "SELECT COALESCE(d.n_desa, '(tanpa desa)') AS desa, COALESCE(k.n_kec, '') AS kecamatan, \
                 CAST(COUNT(*) AS SIGNED) AS jumlah, CAST(COALESCE(SUM(p.pagu), 0) AS DOUBLE) AS pagu \
                 FROM tbl_pekerjaan p LEFT JOIN tbl_desa d ON d.id = p.desa_id LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id{where_sql}{} \
                 GROUP BY p.desa_id, d.n_desa, k.n_kec ORDER BY pagu DESC LIMIT 50",
                scope.sql
            );
            let mut q = sqlx::query(&sql);
            for b in &binds {
                q = q.bind(b);
            }
            let rows = q.fetch_all(pool).await.map_err(internal)?;
            let hasil: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "desa": r.try_get::<Option<String>, _>("desa").ok().flatten(),
                        "kecamatan": r.try_get::<Option<String>, _>("kecamatan").ok().flatten(),
                        "jumlahPekerjaan": r.try_get::<i64, _>("jumlah").ok(),
                        "pagu": r.try_get::<f64, _>("pagu").ok(),
                    })
                })
                .collect();

            // Cakupan: desa yang punya pekerjaan aktif (dalam scope) dibanding seluruh desa.
            let cakupan_sql = format!(
                "SELECT CAST(COUNT(DISTINCT p.desa_id) AS SIGNED) FROM tbl_pekerjaan p{where_sql}{} AND p.desa_id IS NOT NULL",
                scope.sql
            );
            let mut cq = sqlx::query_scalar::<_, i64>(&cakupan_sql);
            for b in &binds {
                cq = cq.bind(b);
            }
            let tercakup = cq.fetch_one(pool).await.map_err(internal)?;
            let total_desa: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_desa")
                .fetch_one(pool)
                .await
                .map_err(internal)?;

            let jumlah = hasil.len() as i64;
            Ok((
                json!({ "jumlahDesaTercakup": tercakup, "jumlahDesaTotal": total_desa, "desa": hasil }),
                Some(jumlah),
            ))
        }
        "cari_surat" => {
            let kata = escape_like(args.get("kata").and_then(Value::as_str).unwrap_or("").trim());
            let kata: String = kata.chars().take(100).collect();
            let tahun_angka: i64 = tahun.parse().map_err(internal)?;
            let jumlah_tahun: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM surat WHERE tahun = ?")
                .bind(tahun_angka)
                .fetch_one(pool)
                .await
                .map_err(internal)?;
            if kata.trim().is_empty() {
                return Ok((json!({ "jumlahSuratTahunIni": jumlah_tahun, "hasil": [] }), Some(0)));
            }
            let pola = format!("%{kata}%");
            let rows = sqlx::query(
                "SELECT s.nomor AS nomor, s.perihal AS perihal, s.tujuan AS tujuan, DATE_FORMAT(s.tanggal, '%Y-%m-%d') AS tanggal \
                 FROM surat s WHERE s.tahun = ? AND (s.nomor LIKE ? OR s.perihal LIKE ?) ORDER BY s.tanggal DESC, s.id DESC LIMIT 10",
            )
            .bind(tahun_angka)
            .bind(&pola)
            .bind(&pola)
            .fetch_all(pool)
            .await
            .map_err(internal)?;
            let hasil: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "nomor": r.try_get::<Option<String>, _>("nomor").ok().flatten(),
                        "perihal": r.try_get::<Option<String>, _>("perihal").ok().flatten(),
                        "tujuan": r.try_get::<Option<String>, _>("tujuan").ok().flatten(),
                        "tanggal": r.try_get::<Option<String>, _>("tanggal").ok().flatten(),
                    })
                })
                .collect();
            let jumlah = hasil.len() as i64;
            Ok((json!({ "jumlahSuratTahunIni": jumlah_tahun, "hasil": hasil }), Some(jumlah)))
        }
        "kualitas_data" => {
            crate::notifications::require_admin(pool, user_id).await?;
            let stats = crate::quality_insight::stats_value(pool, Some(tahun)).await?;
            Ok((Value::Object(stats), None))
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
    let is_admin = crate::notifications::require_admin(&state.pool, user.user_id).await.is_ok();
    let mulai = Instant::now();

    // Langkah 1: model memilih tool. Nama yang tidak ada di daftar ditolak.
    let pilihan_text = panggil_model(
        &cfg,
        json!([
            {
                "role": "system",
                "content": format!(
                    "Pilih satu tool untuk menjawab pertanyaan tentang data pembangunan air minum dan sanitasi. \
                     Balas HANYA dengan JSON: {{\"tool\": \"<nama>\", \"args\": {{...}}}} atau {{\"tool\": null, \"alasan\": \"<singkat>\"}} bila tidak ada yang cocok.\nTools:\n{daftar}", daftar = daftar_tool(is_admin)
                )
            },
            { "role": "user", "content": pertanyaan }
        ]),
        200,
    )
    .await?;

    let pilihan = parse_json_dari_model(&pilihan_text);
    let nama = pilihan.as_ref().and_then(|p| p["tool"].as_str()).map(str::to_string);
    let nama_valid = TOOLS
        .iter()
        .any(|t| Some(t.0) == nama.as_deref() && (is_admin || !t.3));
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

/// Batas panjang pertanyaan chat.
const CHAT_MAX_CHARS: usize = 2_000;

/// `POST /api/ai/chat`: satu giliran chat. Riwayat disimpan server (`ai_memory`), klien hanya mengirim pertanyaan.
/// Event SSE: `status` (saat mengambil data), `done` (jawaban dan sumber), `error`.
pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let cfg = ai_config()?;
    cek_batas(&state, user.user_id)?;

    let tahun = body["tahun"].as_str().unwrap_or("").trim().to_string();
    if tahun.len() != 4 || !tahun.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Tahun anggaran tidak valid."));
    }
    let pertanyaan = body["pertanyaan"].as_str().unwrap_or("").trim().to_string();
    if pertanyaan.is_empty() || pertanyaan.chars().count() > CHAT_MAX_CHARS {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "Pertanyaan wajib diisi, maksimal 2.000 karakter."));
    }
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    let is_admin = crate::notifications::require_admin(&state.pool, user.user_id).await.is_ok();
    let user_id = user.user_id;

    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        let mulai = Instant::now();
        let hasil = crate::ai_memory::giliran(&state, &cfg, user_id, &roles, is_admin, &tahun, &pertanyaan, &tx).await;
        match hasil {
            Ok((done, sumber)) => {
                let tool = sumber.last().and_then(|s| s["tool"].as_str()).map(str::to_string);
                let jumlah: i64 = sumber.iter().filter_map(|s| s["jumlah_baris"].as_i64()).sum();
                catat(&state.pool, user_id, "chat", tool.as_deref(), &json!(sumber), Some(&pertanyaan), Some(jumlah), &cfg.model, mulai.elapsed().as_millis(), "ok").await;
                let _ = tx.try_send(Event::default().event("done").data(done.to_string()));
            }
            Err(e) => {
                catat(&state.pool, user_id, "chat", None, &Value::Null, Some(&pertanyaan), None, &cfg.model, mulai.elapsed().as_millis(), "gagal").await;
                let _ = tx.try_send(Event::default().event("error").data(json!({ "message": e.message }).to_string()));
            }
        }
        // Ringkasan dibangun setelah jawaban terkirim, supaya tidak menambah waktu tunggu.
        if let Err(e) = crate::ai_memory::bangun_tertunda(&state.pool, &cfg, user_id).await {
            eprintln!("[AI] gagal membangun memori chat: {}", e.message);
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|ev| (Ok::<Event, Infallible>(ev), rx))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `GET /api/ai/chat/history`: pesan pengguna dan jawaban asisten, untuk ditampilkan saat halaman dibuka.
pub async fn history(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let data = crate::ai_memory::riwayat(&state.pool, user.user_id, 60).await?;
    Ok(Json(json!({ "data": data })))
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
