//! Pesan WhatsApp masuk dari open-wa dijawab oleh Asisten Data, dengan data dan hak akses user yang sama
//! seperti di aplikasi. Alurnya: webhook `message.received` -> cari user lewat nomor -> `ai_memory::giliran`
//! -> balasan lewat `wa_gateway`.
//!
//! Aturan:
//! - Nomor harus sudah ada di `users.whatsapp`. Nomor lain dijawab dengan petunjuk mendaftarkan nomor.
//! - Di grup hanya dijawab bila bot di-mention atau pesan dibalas dari bot.
//! - Pesan dari akun sendiri (`fromMe`) diabaikan agar tidak terjadi loop.
//! - Webhook harus membawa `?secret=` yang sama dengan `WA_WEBHOOK_SECRET`.
//!
//! Bentuk payload open-wa tidak dijelaskan di spesifikasi. Parser di sini menerima beberapa bentuk umum
//! dan mencatat payload mentah bila tidak bisa dibaca, supaya bisa dicocokkan dengan payload asli.

use std::{collections::HashMap, time::Duration};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Datelike, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use tokio::sync::mpsc;

use crate::{
    ai, ai_memory, ai_model, catatan, jadwal_wa,
    notifications::require_admin,
    whatsapp,
    wa_gateway::{self, WaConfig, TEKS_MAKS},
    AppState,
};

/// Pesan masuk yang sudah dibaca dari payload webhook.
#[derive(Debug, PartialEq)]
pub(crate) struct PesanMasuk {
    /// Chat tempat pesan datang: perorangan (`...@c.us` atau `...@lid`) atau grup (`...@g.us`).
    pub chat_id: String,
    /// Pengirim dalam bentuk mentah: nomor (`628...@c.us`) atau LID (`...@lid`).
    pub pengirim: String,
    pub is_grup: bool,
    /// Teks tanpa mention bot.
    pub teks: String,
    /// Bot di-mention atau pesan membalas bot.
    pub menyebut_bot: bool,
    /// Teks pesan yang dibalas, bila ada.
    pub kutipan: Option<String>,
}

/// Perintah "catat" dari pesan yang menyebut bot. Hanya dikenali bila kalimatnya pendek, supaya
/// pertanyaan yang kebetulan memuat kata "catat" tetap dijawab oleh Asisten Data.
pub(crate) fn perintah_catat(teks: &str) -> bool {
    let kata: Vec<String> = teks.split_whitespace().map(|k| k.to_lowercase()).collect();
    kata.len() <= 5 && kata.iter().any(|k| k.starts_with("catat"))
}

fn angka(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Membaca payload `message.received`. `None` bila bukan pesan masuk yang perlu diproses.
/// `bot_lid` adalah ID LID bot (`...@lid`), dipakai karena WhatsApp sering memakai LID untuk mention di grup.
pub(crate) fn baca_pesan(payload: &Value, bot_nomor: &str, bot_lid: &str) -> Option<PesanMasuk> {
    if let Some(event) = payload["event"].as_str().or_else(|| payload["type"].as_str()) {
        if event != "message.received" {
            return None;
        }
    }
    let mut m = if payload["data"].is_object() { &payload["data"] } else { payload };
    if m["message"].is_object() {
        m = &m["message"];
    }
    if m["fromMe"].as_bool() == Some(true) {
        return None;
    }

    let chat_id = m["chatId"].as_str().or_else(|| m["from"].as_str())?.to_string();
    let is_grup = chat_id.ends_with("@g.us");
    let pengirim = m["author"]
        .as_str()
        .or_else(|| m["sender"]["id"].as_str())
        .or_else(|| m["from"].as_str())
        .unwrap_or(&chat_id)
        .to_string();

    let teks_mentah = m["body"]
        .as_str()
        .or_else(|| m["content"].as_str())
        .or_else(|| m["text"].as_str())?
        .to_string();

    let bot_ids: Vec<String> = [format!("{bot_nomor}@c.us"), bot_lid.to_string()]
        .into_iter()
        .filter(|id| !id.starts_with('@') && id.contains('@'))
        .collect();
    let mention_list = m["mentionedJidList"]
        .as_array()
        .or_else(|| m["mentions"].as_array())
        .map(|l| l.iter().filter_map(|x| x.as_str()).any(|x| bot_ids.iter().any(|b| b == x)))
        .unwrap_or(false);
    let lid_digit = angka(bot_lid);
    let mention_teks = (!bot_nomor.is_empty() && teks_mentah.contains(&format!("@{bot_nomor}")))
        || (!lid_digit.is_empty() && teks_mentah.contains(&format!("@{lid_digit}")));
    let balas_bot = m["quotedMsg"]["fromMe"].as_bool() == Some(true)
        || m["quotedMsgObj"]["fromMe"].as_bool() == Some(true);
    let menyebut_bot = mention_list || mention_teks || balas_bot;

    let kutipan = m["quotedMsg"]["body"]
        .as_str()
        .or_else(|| m["quotedMsg"]["content"].as_str())
        .or_else(|| m["quotedMsgObj"]["body"].as_str())
        .or_else(|| m["quotedMsgObj"]["content"].as_str())
        .map(str::to_string)
        .filter(|k| !k.trim().is_empty());

    let mut teks = teks_mentah.clone();
    for token in [bot_nomor.to_string(), lid_digit.clone()] {
        if !token.is_empty() {
            teks = teks.replace(&format!("@{token}"), " ");
        }
    }
    let teks = teks.trim().to_string();

    Some(PesanMasuk { chat_id, pengirim, is_grup, teks, menyebut_bot, kutipan })
}

/// Mengubah pengirim menjadi nomor `62...`. LID diterjemahkan lewat open-wa (best-effort).
async fn nomor_pengirim(cfg: &WaConfig, pengirim: &str) -> Option<String> {
    if pengirim.ends_with("@lid") {
        return nomor_dari_lid(cfg, pengirim).await;
    }
    whatsapp::normalize(&angka(pengirim)).ok()
}

async fn nomor_dari_lid(cfg: &WaConfig, lid: &str) -> Option<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().ok()?;
    let url = format!(
        "{}/api/sessions/{}/contacts/{}/phone",
        cfg.base_url,
        cfg.session_id,
        lid.replace('@', "%40")
    );
    let resp = client.get(url).header("X-API-Key", &cfg.api_key).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let raw = v["phone"]
        .as_str()
        .or_else(|| v["number"].as_str())
        .or_else(|| v.as_str())?;
    whatsapp::normalize(&angka(raw)).ok()
}

async fn balas(cfg: &WaConfig, chat_id: &str, teks: &str) {
    for bagian in wa_gateway::potong_pesan(teks, TEKS_MAKS) {
        if let Err(e) = wa_gateway::kirim_ke_chat(cfg, chat_id, &bagian).await {
            eprintln!("[WA] balasan gagal: {}", e.message);
        }
    }
}

const PESAN_BELUM_TERDAFTAR: &str = "Nomor ini belum terdaftar di Arumanis. Tambahkan nomor WhatsApp Anda di menu Profil (arumanis.cianjur.space) agar bisa memakai Asisten Data.";

/// Memproses satu pesan. Dijalankan di latar belakang agar webhook langsung dibalas 200.
async fn proses(state: AppState, cfg: WaConfig, pesan: PesanMasuk) {
    if pesan.is_grup && !pesan.menyebut_bot {
        eprintln!("[WA] pesan grup diabaikan: bot tidak disebut (chat {}, pengirim {})", pesan.chat_id, pesan.pengirim);
        return;
    }
    let tujuan = if pesan.is_grup {
        pesan.chat_id.clone()
    } else {
        match nomor_pengirim(&cfg, &pesan.pengirim).await {
            Some(n) => wa_gateway::chat_id(&n),
            None => pesan.chat_id.clone(),
        }
    };
    let Some(nomor) = nomor_pengirim(&cfg, &pesan.pengirim).await else {
        balas(&cfg, &tujuan, PESAN_BELUM_TERDAFTAR).await;
        return;
    };

    let user_id: Option<u64> = match sqlx::query_scalar("SELECT id FROM users WHERE whatsapp = ? LIMIT 1")
        .bind(&nomor)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => Some(id),
        Ok(None) => None,
        Err(e) => {
            eprintln!("[WA] gagal mencari user: {e}");
            return;
        }
    };
    let Some(user_id) = user_id else {
        balas(&cfg, &tujuan, PESAN_BELUM_TERDAFTAR).await;
        return;
    };

    if perintah_catat(&pesan.teks) {
        match &pesan.kutipan {
            Some(isi) => {
                let balasan = match catatan::simpan_dari_wa(&state.pool, user_id, isi, &pesan.chat_id).await {
                    Ok(_) => "Catatan tersimpan. Lihat di menu Catatan Arumanis.".to_string(),
                    Err(e) => e.message,
                };
                balas(&cfg, &tujuan, &balasan).await;
            }
            None => {
                balas(
                    &cfg,
                    &tujuan,
                    "Balas pesan yang ingin dicatat, lalu tulis @arumanis catat.",
                )
                .await
            }
        }
        return;
    }

    if jadwal_wa::perintah_jadwal(&pesan.teks) {
        let balasan = match jadwal_wa::parse(&pesan.teks, pesan.kutipan.as_deref(), jadwal_wa::hari_ini_wib()) {
            Ok(j) => match jadwal_wa::simpan(&state.pool, user_id, &j).await {
                Ok(_) => format!(
                    "Jadwal dibuat: {} pada {}{}. Lihat di kalender Arumanis.",
                    j.judul,
                    j.mulai.format("%d/%m/%Y %H:%M"),
                    j.lokasi.as_ref().map(|l| format!(" di {l}")).unwrap_or_default()
                ),
                Err(e) => e.message,
            },
            Err(pesan_galat) => pesan_galat,
        };
        balas(&cfg, &tujuan, &balasan).await;
        return;
    }

    if pesan.teks.trim().is_empty() {
        balas(&cfg, &tujuan, "Silakan tulis pertanyaan Anda.").await;
        return;
    }
    if let Err(e) = ai::cek_batas(&state, user_id) {
        balas(&cfg, &tujuan, &e.message).await;
        return;
    }

    let roles = match auth::login::roles_of(&state.pool, user_id).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[WA] gagal membaca peran: {e}");
            return;
        }
    };
    let is_admin = require_admin(&state.pool, user_id).await.is_ok();
    let ai_cfg = match ai_model::ai_config_untuk(&state.pool).await {
        Ok(c) => c,
        Err(e) => {
            balas(&cfg, &tujuan, &e.message).await;
            return;
        }
    };
    let tahun = Utc::now().year().to_string();

    // Hasil status tidak dipakai di jalur WhatsApp, jadi penerimanya dibuang.
    let (tx, mut rx) = mpsc::channel(32);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let hasil = ai_memory::giliran(&state, &ai_cfg, user_id, &roles, is_admin, &tahun, &pesan.teks, ai_memory::Kanal::WhatsApp, &tx).await;
    drop(tx);

    match hasil {
        Ok((done, _, _)) => {
            let jawaban = done["jawaban"].as_str().unwrap_or("").to_string();
            balas(&cfg, &tujuan, &jawaban).await;
        }
        Err(e) => balas(&cfg, &tujuan, &format!("Maaf, Asisten Data belum bisa menjawab: {}", e.message)).await,
    }

    let pool = state.pool.clone();
    tokio::spawn(async move {
        if let Err(e) = ai_memory::bangun_tertunda(&pool, &ai_cfg, user_id).await {
            eprintln!("[WA] gagal membangun memori: {}", e.message);
        }
    });
}

/// `POST /api/wa/webhook?secret=...`: webhook `message.received` dari open-wa.
pub async fn webhook(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let secret = std::env::var("WA_WEBHOOK_SECRET").unwrap_or_default();
    if secret.trim().is_empty() {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "WA_WEBHOOK_SECRET belum diisi."));
    }
    if query.get("secret").map(String::as_str) != Some(secret.trim()) {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "Secret webhook tidak valid."));
    }
    let Some(cfg) = wa_gateway::config() else {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "WA belum dikonfigurasi."));
    };
    let bot_nomor = std::env::var("WA_BOT_NUMBER").unwrap_or_default().trim().to_string();
    let bot_lid = std::env::var("WA_BOT_LID").unwrap_or_default().trim().to_string();

    match baca_pesan(&payload, &bot_nomor, &bot_lid) {
        Some(pesan) => {
            tokio::spawn(proses(state, cfg, pesan));
        }
        None => {
            if payload["event"].as_str().is_none() && payload["data"].is_null() {
                eprintln!("[WA] payload webhook tidak dikenali: {payload}");
            }
        }
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "6289510000707";

    #[test]
    fn pesan_pribadi_dibaca() {
        let p = json!({"event":"message.received","data":{"chatId":"628553333200@c.us","from":"628553333200@c.us","body":"berapa pekerjaan?","fromMe":false}});
        let m = baca_pesan(&p, BOT, "").unwrap();
        assert_eq!(m.teks, "berapa pekerjaan?");
        assert!(!m.is_grup);
        assert!(!m.menyebut_bot);
    }

    #[test]
    fn pesan_sendiri_diabaikan() {
        let p = json!({"event":"message.received","data":{"chatId":"628553333200@c.us","body":"x","fromMe":true}});
        assert!(baca_pesan(&p, BOT, "").is_none());
    }

    #[test]
    fn event_lain_diabaikan() {
        let p = json!({"event":"session.status","data":{"status":"ready"}});
        assert!(baca_pesan(&p, BOT, "").is_none());
    }

    #[test]
    fn grup_dengan_mention_dibaca_dan_mention_dibuang() {
        let p = json!({"event":"message.received","data":{"chatId":"120363@g.us","author":"628553333200@c.us","body":"@6289510000707 berapa paket?","mentionedJidList":["6289510000707@c.us"]}});
        let m = baca_pesan(&p, BOT, "").unwrap();
        assert!(m.is_grup);
        assert!(m.menyebut_bot);
        assert_eq!(m.teks, "berapa paket?");
    }

    #[test]
    fn grup_tanpa_mention_terbaca_tapi_tidak_disebut() {
        let p = json!({"event":"message.received","data":{"chatId":"120363@g.us","author":"628553333200@c.us","body":"halo semua"}});
        let m = baca_pesan(&p, BOT, "").unwrap();
        assert!(m.is_grup);
        assert!(!m.menyebut_bot);
    }

    #[test]
    fn balas_pesan_bot_dihitung_sebagai_sebutan() {
        let p = json!({"event":"message.received","data":{"chatId":"120363@g.us","author":"628553333200@c.us","body":"lanjut","quotedMsg":{"fromMe":true}}});
        assert!(baca_pesan(&p, BOT, "").unwrap().menyebut_bot);
    }

    #[test]
    fn mention_di_grup_dengan_lid_bot_dikenali() {
        let p = json!({"event":"message.received","data":{"chatId":"120363@g.us","author":"628553333200@c.us","body":"@177125105643571 berapa paket?","mentionedJidList":["177125105643571@lid"]}});
        let m = baca_pesan(&p, BOT, "177125105643571@lid").unwrap();
        assert!(m.menyebut_bot);
        assert_eq!(m.teks, "berapa paket?");
    }

    #[test]
    fn perintah_catat_dikenali_dan_pertanyaan_tidak() {
        assert!(perintah_catat("tolong catat ini"));
        assert!(perintah_catat("catat"));
        assert!(perintah_catat("Catatkan ini ya"));
        assert!(!perintah_catat("berapa pekerjaan yang dicatat di SP2D bulan ini ya kan?"));
    }

    #[test]
    fn kutipan_dibaca_dari_pesan_yang_dibalas() {
        let p = json!({"event":"message.received","data":{"chatId":"120363@g.us","author":"628553333200@c.us","body":"@6289510000707 tolong catat ini","mentionedJidList":["6289510000707@c.us"],"quotedMsg":{"body":"Target realisasi 80% di Desa Sukamaju"}}});
        let m = baca_pesan(&p, BOT, "").unwrap();
        assert_eq!(m.kutipan.as_deref(), Some("Target realisasi 80% di Desa Sukamaju"));
        assert!(perintah_catat(&m.teks));
    }

    #[test]
    fn lid_dikenali_sebagai_pengirim() {
        let p = json!({"event":"message.received","data":{"chatId":"177125@lid","from":"177125@lid","body":"halo"}});
        let m = baca_pesan(&p, BOT, "").unwrap();
        assert_eq!(m.pengirim, "177125@lid");
    }
}
