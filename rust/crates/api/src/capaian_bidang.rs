//! `GET /api/dashboard/capaian-bidang`: capaian tahun anggaran per sub bidang (air minum dan sanitasi).
//!
//! Dipakai poster capaian di halaman Laporan. `GET /api/dashboard/stats` mengelompokkan per nama sub
//! kegiatan, bukan per `tbl_kegiatan.sub_bidang`, sehingga angka per bidang dihitung di sini.
//!
//! Aturan hitung:
//! - Paket: paket aktif (status bukan `canceled`) pada kegiatan tahun anggaran terpilih.
//! - Progres fisik: rata-rata realisasi fisik TERBARU per paket (tanggal lalu id terbesar), hanya paket
//!   yang punya realisasi. Ini berbeda dari `stats`, yang mengikuti Laravel dan memakai realisasi TERLAMA.
//! - Realisasi keuangan: total `nilai` realisasi keuangan (SP2D) dibagi pagu paket aktif.
//! - `tahun` dipakai apa adanya sebagai bind string. Bila kosong, tahun berjalan.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use chrono::{Datelike, NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    dashboard::{rows, stats_scope, Bind, Scope, WITH_KONTRAK},
    desa::internal,
    php, require_auth, AppState,
};

/// Bidang yang selalu tampil di respons, berurutan, walau belum punya paket.
const BIDANG_TETAP: [&str; 2] = ["air minum", "sanitasi"];

const BIDANG_TERISI: &str =
    "tbl_kegiatan.sub_bidang IS NOT NULL AND TRIM(tbl_kegiatan.sub_bidang) <> ''";
const BIDANG_NAMA: &str = "LOWER(TRIM(tbl_kegiatan.sub_bidang))";

/// Satu baris realisasi fisik: paket, bidangnya, tanggal, id riwayat, dan persen.
pub type BarisFisik = (i64, String, NaiveDate, i64, f64);

/// Rata-rata progres fisik per bidang dari realisasi TERBARU tiap paket.
/// Hasil: bidang -> (jumlah persen, jumlah paket).
pub fn progres_terbaru_per_bidang(baris: &[BarisFisik]) -> HashMap<String, (f64, i64)> {
    let mut terbaru: HashMap<i64, &BarisFisik> = HashMap::new();
    for b in baris {
        let ganti = match terbaru.get(&b.0) {
            None => true,
            Some(lama) => (b.2, b.3) > (lama.2, lama.3),
        };
        if ganti {
            terbaru.insert(b.0, b);
        }
    }
    let mut per_bidang: HashMap<String, (f64, i64)> = HashMap::new();
    for b in terbaru.values() {
        let e = per_bidang.entry(b.1.clone()).or_insert((0.0, 0));
        e.0 += b.4;
        e.1 += 1;
    }
    per_bidang
}

/// Persen `bagian / total` dengan satu desimal; 0 bila total tidak positif.
pub fn persen(bagian: f64, total: f64) -> f64 {
    if total > 0.0 {
        php::round(bagian / total * 100.0, 1)
    } else {
        0.0
    }
}

/// Urutan bidang: air minum, sanitasi, lalu bidang lain berurutan abjad.
fn urutan_bidang(ada: impl Iterator<Item = String>) -> Vec<String> {
    let mut semua: BTreeMap<String, ()> = BTreeMap::new();
    for b in BIDANG_TETAP {
        semua.insert(b.to_string(), ());
    }
    for b in ada {
        semua.insert(b, ());
    }
    let mut hasil: Vec<String> = BIDANG_TETAP.iter().map(|b| b.to_string()).collect();
    hasil.extend(
        semua
            .into_keys()
            .filter(|b| !BIDANG_TETAP.contains(&b.as_str())),
    );
    hasil
}

/// `GET /api/dashboard/capaian-bidang?tahun=2026`.
pub async fn capaian_bidang(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let p = php::Params::parse(raw.as_deref());
    let mut scope = stats_scope(&p);
    let tahun = match &scope.tahun {
        Some(Bind::Text(t)) => t.clone(),
        _ => {
            let sekarang = Utc::now().year().to_string();
            scope.tahun = Some(Bind::Text(sekarang.clone()));
            sekarang
        }
    };
    let data = build_capaian_bidang(&state.pool, &scope, &tahun)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "data": data })))
}

pub async fn build_capaian_bidang(
    pool: &MySqlPool,
    s: &Scope,
    tahun: &str,
) -> Result<Value, sqlx::Error> {
    let aktif = s.pekerjaan(true);
    let semua = s.pekerjaan(false);
    let kc = s.kegiatan();

    // Paket aktif per bidang: jumlah, yang sudah berkontrak, dan pagu.
    let paket_rows = rows(
        pool,
        &format!(
            "SELECT {BIDANG_NAMA} AS bidang, COUNT(*) AS paket, \
             CAST(SUM(CASE WHEN {WITH_KONTRAK} THEN 1 ELSE 0 END) AS SIGNED) AS berkontrak, \
             CAST(SUM(tbl_pekerjaan.pagu) AS DOUBLE) AS pagu \
             FROM tbl_pekerjaan INNER JOIN tbl_kegiatan ON tbl_pekerjaan.kegiatan_id = tbl_kegiatan.id \
             WHERE {} AND {BIDANG_TERISI} GROUP BY {BIDANG_NAMA}",
            aktif.clause()
        ),
        &aktif.binds,
    )
    .await?;

    // Paket batal per bidang (semua paket dengan status canceled).
    let batal_rows = rows(
        pool,
        &format!(
            "SELECT {BIDANG_NAMA} AS bidang, COUNT(*) AS batal \
             FROM tbl_pekerjaan INNER JOIN tbl_kegiatan ON tbl_pekerjaan.kegiatan_id = tbl_kegiatan.id \
             WHERE {} AND tbl_pekerjaan.status = 'canceled' AND {BIDANG_TERISI} GROUP BY {BIDANG_NAMA}",
            semua.clause()
        ),
        &semua.binds,
    )
    .await?;

    // Kegiatan dan pagu kegiatan per bidang.
    let kegiatan_rows = rows(
        pool,
        &format!(
            "SELECT {BIDANG_NAMA} AS bidang, COUNT(*) AS kegiatan, \
             CAST(SUM(tbl_kegiatan.pagu) AS DOUBLE) AS pagu_kegiatan \
             FROM tbl_kegiatan WHERE {} AND {BIDANG_TERISI} GROUP BY {BIDANG_NAMA}",
            kc.clause()
        ),
        &kc.binds,
    )
    .await?;

    // Realisasi fisik paket aktif; yang terbaru per paket dipilih di Rust.
    let fisik_rows = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS pid, {BIDANG_NAMA} AS bidang, h.tanggal AS tgl, \
             CAST(h.id AS SIGNED) AS hid, CAST(h.persen AS DOUBLE) AS persen \
             FROM pekerjaan_progress_estimasi_history h \
             INNER JOIN tbl_pekerjaan ON tbl_pekerjaan.id = h.pekerjaan_id \
             INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
             WHERE h.tipe = 'realisasi' AND h.jenis = 'fisik' AND {} AND {BIDANG_TERISI}",
            aktif.clause()
        ),
        &aktif.binds,
    )
    .await?;
    let mut baris_fisik: Vec<BarisFisik> = Vec::with_capacity(fisik_rows.len());
    for r in &fisik_rows {
        baris_fisik.push((
            r.try_get("pid")?,
            r.try_get("bidang")?,
            r.try_get("tgl")?,
            r.try_get("hid")?,
            r.try_get::<Option<f64>, _>("persen")?.unwrap_or(0.0),
        ));
    }
    let progres = progres_terbaru_per_bidang(&baris_fisik);

    // Total realisasi keuangan (SP2D) paket aktif per bidang.
    let sp2d_rows = rows(
        pool,
        &format!(
            "SELECT {BIDANG_NAMA} AS bidang, CAST(SUM(h.nilai) AS DOUBLE) AS total \
             FROM pekerjaan_progress_estimasi_history h \
             INNER JOIN tbl_pekerjaan ON tbl_pekerjaan.id = h.pekerjaan_id \
             INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
             WHERE h.tipe = 'realisasi' AND h.jenis = 'keuangan' AND {} AND {BIDANG_TERISI} \
             GROUP BY {BIDANG_NAMA}",
            aktif.clause()
        ),
        &aktif.binds,
    )
    .await?;

    let mut paket: HashMap<String, (i64, i64, f64)> = HashMap::new();
    for r in &paket_rows {
        paket.insert(
            r.try_get("bidang")?,
            (
                r.try_get("paket")?,
                r.try_get("berkontrak")?,
                r.try_get::<Option<f64>, _>("pagu")?.unwrap_or(0.0),
            ),
        );
    }
    let mut batal: HashMap<String, i64> = HashMap::new();
    for r in &batal_rows {
        batal.insert(r.try_get("bidang")?, r.try_get("batal")?);
    }
    let mut kegiatan: HashMap<String, (i64, f64)> = HashMap::new();
    for r in &kegiatan_rows {
        kegiatan.insert(
            r.try_get("bidang")?,
            (
                r.try_get("kegiatan")?,
                r.try_get::<Option<f64>, _>("pagu_kegiatan")?.unwrap_or(0.0),
            ),
        );
    }
    let mut sp2d: HashMap<String, f64> = HashMap::new();
    for r in &sp2d_rows {
        sp2d.insert(
            r.try_get("bidang")?,
            r.try_get::<Option<f64>, _>("total")?.unwrap_or(0.0),
        );
    }

    let ada = paket
        .keys()
        .chain(batal.keys())
        .chain(kegiatan.keys())
        .chain(progres.keys())
        .chain(sp2d.keys())
        .cloned()
        .collect::<Vec<_>>();

    let bidang: Vec<Value> = urutan_bidang(ada.into_iter())
        .into_iter()
        .map(|nama| {
            let (jumlah, berkontrak, pagu) = paket.get(&nama).copied().unwrap_or((0, 0, 0.0));
            let (jumlah_kegiatan, pagu_kegiatan) = kegiatan.get(&nama).copied().unwrap_or((0, 0.0));
            let (jumlah_persen, paket_berprogres) = progres.get(&nama).copied().unwrap_or((0.0, 0));
            let total_sp2d = sp2d.get(&nama).copied().unwrap_or(0.0);
            json!({
                "name": nama,
                "kegiatan": jumlah_kegiatan,
                "paguKegiatan": pagu_kegiatan,
                "paket": jumlah,
                "berkontrak": berkontrak,
                "belumBerkontrak": jumlah - berkontrak,
                "batal": batal.get(&nama).copied().unwrap_or(0),
                "pagu": pagu,
                "progressFisik": if paket_berprogres > 0 { php::round(jumlah_persen / paket_berprogres as f64, 1) } else { 0.0 },
                "paketBerprogres": paket_berprogres,
                "sp2dTotal": total_sp2d,
                "realisasiKeuanganPersen": persen(total_sp2d, pagu),
            })
        })
        .collect();

    Ok(json!({ "tahun": tahun, "bidang": bidang }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgl(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    #[test]
    fn progres_memakai_realisasi_terbaru_per_paket() {
        let baris = vec![
            (1, "air minum".to_string(), tgl(1), 10, 20.0),
            (1, "air minum".to_string(), tgl(15), 11, 60.0),
            (2, "air minum".to_string(), tgl(10), 12, 40.0),
            (3, "sanitasi".to_string(), tgl(5), 13, 100.0),
        ];
        let hasil = progres_terbaru_per_bidang(&baris);
        assert_eq!(hasil["air minum"], (100.0, 2)); // 60 (terbaru paket 1) + 40
        assert_eq!(hasil["sanitasi"], (100.0, 1));
    }

    #[test]
    fn tanggal_sama_dibedakan_id_riwayat() {
        let baris = vec![
            (1, "sanitasi".to_string(), tgl(7), 5, 30.0),
            (1, "sanitasi".to_string(), tgl(7), 9, 45.0),
        ];
        assert_eq!(progres_terbaru_per_bidang(&baris)["sanitasi"], (45.0, 1));
    }

    #[test]
    fn persen_nol_bila_total_nol() {
        assert_eq!(persen(50.0, 0.0), 0.0);
        assert_eq!(persen(25.0, 200.0), 12.5);
    }

    #[test]
    fn urutan_bidang_selalu_memuat_air_minum_dan_sanitasi() {
        let urutan = urutan_bidang(vec!["lainnya".to_string(), "sanitasi".to_string()].into_iter());
        assert_eq!(urutan, vec!["air minum", "sanitasi", "lainnya"]);
        assert_eq!(
            urutan_bidang(std::iter::empty()),
            vec!["air minum", "sanitasi"]
        );
    }
}
