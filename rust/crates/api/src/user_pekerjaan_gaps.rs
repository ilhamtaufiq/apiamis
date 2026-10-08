//! Analisis kelengkapan data pengawas: `GET /api/user-pekerjaan/completeness-gaps`.
//! Setara `UserPekerjaanCompletenessService::analyze` (Laravel).
//!
//! Aturan per pekerjaan yang di-assign ke user:
//! - `foto`: status `resolveFotoMetrics` (cabang lengkap) selain `selesai`;
//! - `penerima`: jumlah baris `tbl_penerima` di bawah kebutuhan unit output yang tidak opsional
//!   (`max(1, ceil(volume))` per output). Tanpa output, kebutuhan 0 dan tidak ada gap;
//! - `progress`: total progres `tbl_progress.content.items` tidak > 0 DAN tidak ada estimasi
//!   fisik atau keuangan pada tahun anggaran kegiatan (atau tahun sekarang bila kegiatan kosong).
//!
//! Pekerjaan yang kegiatannya bertahun anggaran lain dilewati bila `tahun` diminta.
//!
//! Berbeda dari Laravel:
//! - total progres memakai `bobot` dari data, seperti `calculateProgressTotal` di PHP. Ringkasan
//!   `progress_metrics` (bobot berbasis RAB) sengaja tidak dipakai;
//! - urutan assignment diurutkan `users.name`, lalu `user_id`, `pekerjaan_id` agar stabil saat nama sama;
//! - kirim email (`send_email`) belum dipindah, jadi rute `broadcast-reminders` tidak ada di sini.

use std::collections::HashMap;

use chrono::Datelike;
use serde_json::{json, Map, Value};
use sqlx::{MySqlPool, Row};

use crate::{
    pekerjaan::{self, PekerjaanRow},
    pekerjaan_rel::{self, FotoGroup, OutputRow},
    php,
    progress_metrics::php_float,
};

pub const GAP_FOTO: &str = "foto";
pub const GAP_PENERIMA: &str = "penerima";
pub const GAP_PROGRESS: &str = "progress";
const ALL_GAPS: [&str; 3] = [GAP_FOTO, GAP_PENERIMA, GAP_PROGRESS];

/// `normalizeGapFilters`: kosong, atau tidak ada yang valid, berarti semua gap.
/// Urutan selalu foto, penerima, progress.
pub fn normalize_filters(gaps: Option<&[String]>) -> Vec<&'static str> {
    let picked: Vec<&'static str> = match gaps {
        Some(list) if !list.is_empty() => ALL_GAPS
            .iter()
            .copied()
            .filter(|g| list.iter().any(|x| x == g))
            .collect(),
        _ => Vec::new(),
    };
    if picked.is_empty() {
        ALL_GAPS.to_vec()
    } else {
        picked
    }
}

/// `calculateProgressTotal` di PHP: jumlah (persen realisasi x bobot) per item, dibulatkan 2 digit.
/// `bobot` dan `target_volume` diambil langsung dari JSON, tanpa pembobotan RAB.
pub fn calculate_progress_total(content: Option<&Value>) -> f64 {
    let items: Vec<&Value> = match content.and_then(|c| c.get("items")) {
        Some(Value::Array(a)) => a.iter().collect(),
        _ => return 0.0,
    };
    let mut total = 0.0;
    for item in items {
        let bobot = php_float(item.get("bobot").unwrap_or(&Value::Null));
        let target = php_float(item.get("target_volume").unwrap_or(&Value::Null));
        let mut real_sum = 0.0;
        if let Some(weeks) = item.get("weekly_data").and_then(Value::as_object) {
            for data in weeks.values() {
                match data.get("realisasi") {
                    None | Some(Value::Null) => {}
                    Some(v) => real_sum += php_float(v),
                }
            }
        }
        let percent = if target > 0.0 {
            real_sum / target * 100.0
        } else {
            0.0
        };
        total += percent * bobot / 100.0;
    }
    php::round(total, 2)
}

/// `requiredPenerimaUnits`: unit penerima yang wajib, dari output non-opsional.
fn required_penerima_units(outputs: &[OutputRow]) -> i64 {
    outputs
        .iter()
        .filter(|o| !o.penerima_is_optional)
        .map(|o| {
            let vol = o.volume.parse::<f64>().unwrap_or(0.0);
            (vol.ceil() as i64).max(1)
        })
        .sum()
}

fn describe_foto(status: &str, count: i64, required: i64) -> String {
    if status == "belum_ada_foto" {
        return "Belum ada foto dokumentasi".to_string();
    }
    if count < required {
        return format!("Foto {count}/{required} slot");
    }
    "Foto dokumentasi belum lengkap".to_string()
}

/// Gap satu pekerjaan dalam urutan foto, penerima, progress. `gap_details` bentuk objek.
async fn detect(
    pool: &MySqlPool,
    p: &PekerjaanRow,
    filters: &[&str],
) -> Result<Vec<(&'static str, String)>, sqlx::Error> {
    let mut gaps = Vec::new();
    let need_outputs = filters.contains(&GAP_FOTO) || filters.contains(&GAP_PENERIMA);
    let outputs = if need_outputs {
        pekerjaan_rel::outputs_for(pool, p.id).await?
    } else {
        Vec::new()
    };

    if filters.contains(&GAP_FOTO) {
        let groups: Vec<FotoGroup> = pekerjaan_rel::foto_groups_for(pool, p.id).await?;
        let total: i64 = groups.iter().map(|g| g.count).sum();
        let m = pekerjaan_rel::foto_full(&outputs, &groups, total);
        if m.status != "selesai" {
            gaps.push((GAP_FOTO, describe_foto(m.status, total, m.required)));
        }
    }

    if filters.contains(&GAP_PENERIMA) {
        let required = required_penerima_units(&outputs);
        if required > 0 {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM tbl_penerima WHERE pekerjaan_id = ?")
                    .bind(p.id)
                    .fetch_one(pool)
                    .await?;
            if count < required {
                let detail = if count == 0 {
                    "Daftar penerima masih kosong".to_string()
                } else {
                    format!("Penerima {count}/{required} unit")
                };
                gaps.push((GAP_PENERIMA, detail));
            }
        }
    }

    if filters.contains(&GAP_PROGRESS) {
        let raw: Option<Option<String>> = sqlx::query_scalar(
            "SELECT CAST(content AS CHAR) FROM tbl_progress WHERE pekerjaan_id = ? ORDER BY id LIMIT 1",
        )
        .bind(p.id)
        .fetch_optional(pool)
        .await?;
        let content = raw
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let progress_total = calculate_progress_total(content.as_ref());
        if progress_total <= 0.0 {
            let estimasi = pekerjaan::estimasi_for(pool, std::slice::from_ref(p)).await?;
            let has_input = estimasi.get(&p.id).is_some_and(|s| {
                s.fisik_realisasi.is_some()
                    || s.fisik_rencana.is_some()
                    || s.keuangan_realisasi.is_some()
                    || s.keuangan_rencana.is_some()
            });
            if !has_input {
                gaps.push((GAP_PROGRESS, "Progress estimasi belum terinput".to_string()));
            }
        }
    }

    Ok(gaps)
}

/// `analyze(gaps, tahun)`. `tahun = None` memakai tahun sekarang (zona UTC, sama dengan `app.timezone`).
pub async fn analyze(
    pool: &MySqlPool,
    gaps: Option<&[String]>,
    tahun: Option<i64>,
) -> Result<Value, sqlx::Error> {
    let filters = normalize_filters(gaps);
    let tahun = tahun.unwrap_or(chrono::Utc::now().year() as i64);

    let assignments = sqlx::query(
        "SELECT up.user_id, up.pekerjaan_id, u.name, u.email FROM user_pekerjaan up \
         JOIN users u ON u.id = up.user_id ORDER BY u.name, up.user_id, up.pekerjaan_id",
    )
    .fetch_all(pool)
    .await?;

    let mut pekerjaan_cache: HashMap<u64, Option<PekerjaanRow>> = HashMap::new();
    let mut kegiatan_tahun: HashMap<i64, i64> = HashMap::new();
    let mut user_index: HashMap<u64, usize> = HashMap::new();
    // (user_id, nama, email, daftar pekerjaan bergap, hitungan per gap)
    let mut users: Vec<(u64, String, String, Vec<Value>, Map<String, Value>)> = Vec::new();
    let mut by_gap: Map<String, Value> = Map::new();
    for g in &filters {
        by_gap.insert((*g).to_string(), json!(0));
    }
    let mut total_pekerjaan = 0i64;

    for a in &assignments {
        let user_id: u64 = a.try_get("user_id")?;
        let pekerjaan_id: u64 = a.try_get("pekerjaan_id")?;
        if !pekerjaan_cache.contains_key(&pekerjaan_id) {
            let row = pekerjaan::find(pool, pekerjaan_id).await?;
            pekerjaan_cache.insert(pekerjaan_id, row);
        }
        let Some(p) = pekerjaan_cache.get(&pekerjaan_id).cloned().flatten() else {
            continue;
        };

        // Filter tahun: kegiatan dengan tahun anggaran lain dilewati. Tahun kosong (0) tidak difilter.
        let keg_year = match p.kegiatan_id {
            Some(kid) => match kegiatan_tahun.get(&kid) {
                Some(t) => *t,
                None => {
                    let raw: Option<Option<String>> =
                        sqlx::query_scalar("SELECT tahun_anggaran FROM tbl_kegiatan WHERE id = ?")
                            .bind(kid as u64)
                            .fetch_optional(pool)
                            .await?;
                    let t = raw.flatten().map_or(0, |s| php::intval_str(&s));
                    kegiatan_tahun.insert(kid, t);
                    t
                }
            },
            None => 0,
        };
        if tahun > 0 && keg_year > 0 && keg_year != tahun {
            continue;
        }

        let found = detect(pool, &p, &filters).await?;
        if found.is_empty() {
            continue;
        }

        let idx = match user_index.get(&user_id) {
            Some(i) => *i,
            None => {
                let name: String = a.try_get("name")?;
                let email: String = a.try_get("email")?;
                let mut counts = Map::new();
                for g in &filters {
                    counts.insert((*g).to_string(), json!(0));
                }
                users.push((user_id, name, email, Vec::new(), counts));
                user_index.insert(user_id, users.len() - 1);
                users.len() - 1
            }
        };

        let entry = &mut users[idx];
        let mut names = Vec::new();
        let mut details = Map::new();
        for (gap, detail) in &found {
            let c = entry.4.get(*gap).and_then(Value::as_i64).unwrap_or(0);
            entry.4.insert((*gap).to_string(), json!(c + 1));
            let s = by_gap.get(*gap).and_then(Value::as_i64).unwrap_or(0);
            by_gap.insert((*gap).to_string(), json!(s + 1));
            names.push(json!(gap));
            details.insert((*gap).to_string(), json!(detail));
        }
        entry.3.push(json!({
            "pekerjaan_id": p.id,
            "nama_paket": p.nama_paket,
            "gaps": names,
            "gap_details": details,
        }));
        total_pekerjaan += 1;
    }

    let users_json: Vec<Value> = users
        .into_iter()
        .map(|(id, name, email, pekerjaan, counts)| {
            json!({
                "user_id": id,
                "user_name": name,
                "user_email": email,
                "pekerjaan": pekerjaan,
                "gap_counts": counts,
            })
        })
        .collect();

    Ok(json!({
        "users": users_json,
        "summary": {
            "total_users": user_index.len(),
            "total_pekerjaan_with_gaps": total_pekerjaan,
            "by_gap": by_gap,
        },
    }))
}
