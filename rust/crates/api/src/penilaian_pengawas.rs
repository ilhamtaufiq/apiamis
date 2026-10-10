//! `GET /api/dashboard/penilaian-pengawas`: skor kinerja pengawas dan konsultan pengawas.
//!
//! Skor dihitung dari enam parameter per paket yang diawasi. Hasil per paket dirata-rata per orang,
//! lalu digabung dengan bobot. Bila sebuah parameter tidak punya data untuk seluruh paket orang
//! itu, parameter tersebut tidak ikut, dan bobot sisanya dinormalisasi.
//!
//! | Kode | Parameter | Bobot | Aturan skor per paket (0–100) |
//! |---|---|---|---|
//! | `fisik` | Deviasi progres fisik | 30 | 100 dikurangi 2 poin per 1% tertinggal dari rencana |
//! | `keuangan` | Deviasi progres keuangan | 20 | sama seperti fisik, untuk keuangan |
//! | `waktu` | Ketepatan waktu | 20 | 0 bila lewat tanggal selesai kontrak dan fisik belum 100%, selain itu 100 |
//! | `dokumentasi` | Kelengkapan foto | 7.5 | foto dibagi target, maksimal 100 |
//! | `berkas` | Kelengkapan berkas wajib | 7.5 | berkas wajib yang ada dibagi jumlah berkas wajib |
//! | `frekuensi` | Frekuensi pembaruan | 15 | 100 bila pembaruan terakhir ≤ 14 hari, turun ke 0 pada 60 hari |

use std::collections::HashMap;

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use chrono::{NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{
    access,
    dashboard::{self, Bind, Cond},
    dashboard_progres::{self, Assignment},
    desa::internal,
    php, require_auth, AppState,
};

/// Target jumlah foto per paket untuk skor dokumentasi.
pub const TARGET_FOTO: f64 = 5.0;
/// Jenis berkas yang wajib ada per paket.
pub const BERKAS_WAJIB: &[&str] = &["Berita Acara", "Laporan Harian"];

/// Bobot tiap parameter. Jumlahnya 100.
pub const BOBOT: &[(&str, &str, f64)] = &[
    ("fisik", "Deviasi progres fisik", 30.0),
    ("keuangan", "Deviasi progres keuangan", 20.0),
    ("waktu", "Ketepatan waktu", 20.0),
    ("dokumentasi", "Kelengkapan foto", 7.5),
    ("berkas", "Kelengkapan berkas wajib", 7.5),
    ("frekuensi", "Frekuensi pembaruan", 15.0),
];

/// Data satu paket yang dibutuhkan untuk menilai.
#[derive(Debug, Clone, PartialEq)]
pub struct PaketNilai {
    pub fisik_realisasi: Option<f64>,
    pub fisik_rencana: Option<f64>,
    pub keuangan_realisasi: Option<f64>,
    pub keuangan_rencana: Option<f64>,
    pub tgl_selesai: Option<NaiveDate>,
    pub foto: i64,
    /// Jumlah berkas wajib yang sudah ada (0..=`BERKAS_WAJIB.len()`).
    pub berkas_wajib_ada: usize,
    pub update_terakhir: Option<NaiveDate>,
}

/// Skor per paket. `None` berarti parameter itu tidak bisa dinilai untuk paket tersebut.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkorPaket {
    pub fisik: Option<f64>,
    pub keuangan: Option<f64>,
    pub waktu: Option<f64>,
    pub dokumentasi: Option<f64>,
    pub berkas: Option<f64>,
    pub frekuensi: Option<f64>,
}

fn clamp100(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

fn skor_deviasi(realisasi: Option<f64>, rencana: Option<f64>) -> Option<f64> {
    match (realisasi, rencana) {
        (Some(r), Some(p)) => Some(clamp100(100.0 - 2.0 * (p - r).max(0.0))),
        _ => None,
    }
}

/// Hitung skor satu paket pada tanggal `today`.
pub fn skor_paket(p: &PaketNilai, today: NaiveDate) -> SkorPaket {
    let fisik_selesai = p.fisik_realisasi.is_some_and(|v| v >= 100.0);
    let waktu = p.tgl_selesai.map(|selesai| {
        if today > selesai && !fisik_selesai {
            0.0
        } else {
            100.0
        }
    });
    let frekuensi = p.update_terakhir.map(|t| {
        let hari = (today - t).num_days().max(0) as f64;
        if hari <= 14.0 {
            100.0
        } else {
            clamp100(100.0 - (hari - 14.0) * 100.0 / 46.0)
        }
    });
    SkorPaket {
        fisik: skor_deviasi(p.fisik_realisasi, p.fisik_rencana),
        keuangan: skor_deviasi(p.keuangan_realisasi, p.keuangan_rencana),
        waktu,
        dokumentasi: Some(clamp100(p.foto as f64 / TARGET_FOTO * 100.0)),
        berkas: Some(p.berkas_wajib_ada as f64 / BERKAS_WAJIB.len() as f64 * 100.0),
        frekuensi,
    }
}

/// Hasil gabungan untuk satu orang.
#[derive(Debug, Clone, PartialEq)]
pub struct SkorOrang {
    pub paket: usize,
    /// Rata-rata per parameter (`None` bila tidak ada paket yang bisa dinilai).
    pub rata: HashMap<&'static str, Option<f64>>,
    /// Skor total dengan bobot yang dinormalisasi. `None` bila tidak ada parameter yang bisa dinilai.
    pub total: Option<f64>,
}

fn rata_parameter(paket: &[SkorPaket], kode: &str) -> Option<f64> {
    let nilai: Vec<f64> = paket
        .iter()
        .filter_map(|s| match kode {
            "fisik" => s.fisik,
            "keuangan" => s.keuangan,
            "waktu" => s.waktu,
            "dokumentasi" => s.dokumentasi,
            "berkas" => s.berkas,
            "frekuensi" => s.frekuensi,
            _ => None,
        })
        .collect();
    if nilai.is_empty() {
        None
    } else {
        Some(nilai.iter().sum::<f64>() / nilai.len() as f64)
    }
}

/// Gabungkan skor paket-paket satu orang menjadi skor total.
pub fn skor_orang(paket: &[SkorPaket]) -> SkorOrang {
    let mut rata = HashMap::new();
    let mut berbobot = 0.0;
    let mut bobot_total = 0.0;
    for (kode, _, bobot) in BOBOT {
        let r = rata_parameter(paket, kode);
        if let Some(v) = r {
            berbobot += v * bobot;
            bobot_total += bobot;
        }
        rata.insert(*kode, r);
    }
    SkorOrang {
        paket: paket.len(),
        rata,
        total: if bobot_total > 0.0 {
            Some(berbobot / bobot_total)
        } else {
            None
        },
    }
}

/// Kategori dari skor total.
pub fn kategori(total: Option<f64>) -> &'static str {
    match total {
        None => "Belum dinilai",
        Some(t) if t >= 85.0 => "Sangat baik",
        Some(t) if t >= 70.0 => "Baik",
        Some(t) if t >= 55.0 => "Cukup",
        Some(_) => "Perlu perhatian",
    }
}

fn round1(v: Option<f64>) -> Value {
    match v {
        Some(x) => json!((x * 10.0).round() / 10.0),
        None => Value::Null,
    }
}

/// `GET /api/dashboard/penilaian-pengawas`.
pub async fn penilaian(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let p = php::Params::parse(raw.as_deref());
    let scope = dashboard::stats_scope(&p);

    let mut cond = scope.pekerjaan(true);
    let access = access::restriction(user.user_id, &roles, "sp");
    if !access.sql.is_empty() {
        cond = cond.and(&Cond {
            sql: vec![format!(
                "tbl_pekerjaan.id IN (SELECT sp.id FROM tbl_pekerjaan sp WHERE 1=1{})",
                access.sql
            )],
            binds: access.binds.iter().map(|b| Bind::Int(*b as i64)).collect(),
        });
    }

    let today = Utc::now().date_naive();
    let nilai = load_nilai(&state.pool, &cond).await.map_err(internal)?;
    let assignments = dashboard_progres::load_assignments(&state.pool, &cond)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "data": summarize(&nilai, &assignments, today),
    })))
}

/// Ringkasan untuk semua pengawas dan konsultan pengawas, diurutkan dari skor tertinggi.
pub fn summarize(
    nilai: &HashMap<i64, PaketNilai>,
    assignments: &[Assignment],
    today: NaiveDate,
) -> Value {
    // Per orang per peran: kumpulan paket yang diawasi (tanpa paket duplikat).
    let mut per_orang: HashMap<(i64, String), (String, Vec<i64>)> = HashMap::new();
    for a in assignments {
        if !nilai.contains_key(&a.pekerjaan_id) {
            continue;
        }
        let e = per_orang
            .entry((a.user_id, a.role.clone()))
            .or_insert((a.nama.clone(), Vec::new()));
        if !e.1.contains(&a.pekerjaan_id) {
            e.1.push(a.pekerjaan_id);
        }
    }

    let mut hasil: Vec<Value> = per_orang
        .into_iter()
        .map(|((user_id, role), (nama, ids))| {
            let paket: Vec<SkorPaket> = ids
                .iter()
                .filter_map(|id| nilai.get(id).map(|n| skor_paket(n, today)))
                .collect();
            let s = skor_orang(&paket);
            let breakdown: serde_json::Map<String, Value> = BOBOT
                .iter()
                .map(|(kode, _, _)| {
                    (
                        kode.to_string(),
                        round1(s.rata.get(kode).copied().flatten()),
                    )
                })
                .collect();
            json!({
                "user_id": user_id,
                "nama": nama,
                "role": role,
                "jumlah_paket": s.paket,
                "total": round1(s.total),
                "kategori": kategori(s.total),
                "breakdown": breakdown,
            })
        })
        .collect();

    hasil.sort_by(|a, b| {
        let av = a["total"].as_f64().unwrap_or(-1.0);
        let bv = b["total"].as_f64().unwrap_or(-1.0);
        bv.partial_cmp(&av)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b["jumlah_paket"].as_u64().cmp(&a["jumlah_paket"].as_u64()))
    });

    let parameter: Vec<Value> = BOBOT
        .iter()
        .map(|(kode, nama, bobot)| json!({ "kode": kode, "nama": nama, "bobot": bobot }))
        .collect();

    json!({
        "parameter": parameter,
        "pengawas": hasil,
    })
}

async fn load_nilai(
    pool: &sqlx::MySqlPool,
    cond: &Cond,
) -> Result<HashMap<i64, PaketNilai>, sqlx::Error> {
    // Jumlah jenis berkas wajib yang ada (satu jenis dihitung sekali walau berkasnya lebih dari satu).
    let wajib = BERKAS_WAJIB
        .iter()
        .map(|j| format!("'{}'", j.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS id, \
         lh.fisik_realisasi, lh.fisik_rencana, lh.keuangan_realisasi, lh.keuangan_rencana, \
         (SELECT MAX(k.tgl_selesai) FROM tbl_kontrak k WHERE k.id_pekerjaan = tbl_pekerjaan.id) AS tgl_selesai, \
         (SELECT COUNT(*) FROM tbl_foto f WHERE f.pekerjaan_id = tbl_pekerjaan.id) AS foto, \
         (SELECT CAST(COUNT(DISTINCT b.jenis_dokumen) AS SIGNED) FROM tbl_berkas b \
            WHERE b.pekerjaan_id = tbl_pekerjaan.id AND b.jenis_dokumen IN ({wajib})) AS berkas_wajib_ada, \
         (SELECT MAX(h.tanggal) FROM pekerjaan_progress_estimasi_history h WHERE h.pekerjaan_id = tbl_pekerjaan.id) AS update_terakhir \
         FROM tbl_pekerjaan \
         LEFT JOIN ( \
            SELECT pekerjaan_id, \
              MAX(CASE WHEN jenis = 'fisik' AND tipe = 'realisasi' THEN persen END) AS fisik_realisasi, \
              MAX(CASE WHEN jenis = 'fisik' AND tipe = 'rencana' THEN persen END) AS fisik_rencana, \
              MAX(CASE WHEN jenis = 'keuangan' AND tipe = 'realisasi' THEN persen END) AS keuangan_realisasi, \
              MAX(CASE WHEN jenis = 'keuangan' AND tipe = 'rencana' THEN persen END) AS keuangan_rencana \
            FROM ( \
              SELECT h.pekerjaan_id, h.jenis, h.tipe, CAST(h.persen AS DOUBLE) AS persen, \
                     ROW_NUMBER() OVER (PARTITION BY h.pekerjaan_id, h.jenis, h.tipe ORDER BY h.tanggal DESC, h.id DESC) AS rn \
              FROM pekerjaan_progress_estimasi_history h \
              WHERE h.tipe = 'realisasi' OR (h.tipe = 'rencana' AND h.tanggal <= CURDATE()) \
            ) x WHERE x.rn = 1 GROUP BY x.pekerjaan_id \
         ) lh ON lh.pekerjaan_id = tbl_pekerjaan.id \
         WHERE {}",
        cond.clause()
    );
    let mut q = sqlx::query(&sql);
    for b in &cond.binds {
        q = match b {
            Bind::Text(t) => q.bind(t.clone()),
            Bind::Int(i) => q.bind(*i),
        };
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = HashMap::new();
    for r in rows {
        let id: i64 = r.try_get("id")?;
        out.insert(
            id,
            PaketNilai {
                fisik_realisasi: r.try_get("fisik_realisasi")?,
                fisik_rencana: r.try_get("fisik_rencana")?,
                keuangan_realisasi: r.try_get("keuangan_realisasi")?,
                keuangan_rencana: r.try_get("keuangan_rencana")?,
                tgl_selesai: r.try_get("tgl_selesai")?,
                foto: r.try_get("foto")?,
                berkas_wajib_ada: r.try_get::<i64, _>("berkas_wajib_ada")?.max(0) as usize,
                update_terakhir: r.try_get("update_terakhir")?,
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn paket() -> PaketNilai {
        PaketNilai {
            fisik_realisasi: Some(40.0),
            fisik_rencana: Some(50.0),
            keuangan_realisasi: Some(30.0),
            keuangan_rencana: Some(30.0),
            tgl_selesai: Some(d(2026, 12, 31)),
            foto: 5,
            berkas_wajib_ada: 2,
            update_terakhir: Some(d(2026, 10, 1)),
        }
    }

    #[test]
    fn deviasi_tertinggal_dua_poin_per_persen() {
        let s = skor_paket(&paket(), d(2026, 10, 5));
        assert_eq!(s.fisik, Some(80.0)); // tertinggal 10% → 100 - 20
        assert_eq!(s.keuangan, Some(100.0)); // sesuai rencana
    }

    #[test]
    fn deviasi_tidak_pernah_di_atas_seratus() {
        let mut p = paket();
        p.fisik_realisasi = Some(90.0);
        p.fisik_rencana = Some(50.0);
        assert_eq!(skor_paket(&p, d(2026, 10, 5)).fisik, Some(100.0));
    }

    #[test]
    fn terlambat_dan_belum_selesai_nilai_nol() {
        let mut p = paket();
        p.tgl_selesai = Some(d(2026, 9, 30));
        assert_eq!(skor_paket(&p, d(2026, 10, 5)).waktu, Some(0.0));
        p.fisik_realisasi = Some(100.0);
        assert_eq!(skor_paket(&p, d(2026, 10, 5)).waktu, Some(100.0));
    }

    #[test]
    fn frekuensi_turun_setelah_dua_minggu() {
        let mut p = paket();
        p.update_terakhir = Some(d(2026, 10, 1));
        assert_eq!(skor_paket(&p, d(2026, 10, 10)).frekuensi, Some(100.0));
        let hari_60 = skor_paket(&p, d(2026, 11, 30)).frekuensi.unwrap();
        assert!(hari_60.abs() < 1e-9, "60 hari harus 0, dapat {hari_60}");
    }

    #[test]
    fn dokumentasi_dan_berkas_dihitung_dari_target() {
        let mut p = paket();
        p.foto = 2;
        p.berkas_wajib_ada = 1;
        let s = skor_paket(&p, d(2026, 10, 5));
        assert_eq!(s.dokumentasi, Some(40.0));
        assert_eq!(s.berkas, Some(50.0));
    }

    #[test]
    fn total_memakai_bobot_dan_normalisasi_bila_parameter_hilang() {
        let semua = skor_paket(&paket(), d(2026, 10, 5));
        let orang = skor_orang(&[semua.clone()]);
        // Semua parameter ada: bobot total 100.
        let manual =
            (80.0 * 30.0 + 100.0 * 20.0 + 100.0 * 20.0 + 100.0 * 7.5 + 100.0 * 7.5 + 100.0 * 15.0)
                / 100.0;
        assert!((orang.total.unwrap() - manual).abs() < 1e-9);

        // Tanpa rencana (fisik dan keuangan kosong): bobot dinormalisasi ke 50.
        let tanpa_rencana = SkorPaket {
            fisik: None,
            keuangan: None,
            ..semua
        };
        let orang2 = skor_orang(&[tanpa_rencana]);
        let manual2 = (100.0 * 20.0 + 100.0 * 7.5 + 100.0 * 7.5 + 100.0 * 15.0) / 50.0;
        assert!((orang2.total.unwrap() - manual2).abs() < 1e-9);
    }

    #[test]
    fn kategori_sesuai_ambang() {
        assert_eq!(kategori(Some(90.0)), "Sangat baik");
        assert_eq!(kategori(Some(72.0)), "Baik");
        assert_eq!(kategori(Some(60.0)), "Cukup");
        assert_eq!(kategori(Some(10.0)), "Perlu perhatian");
        assert_eq!(kategori(None), "Belum dinilai");
    }
}
