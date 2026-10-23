//! Uji `capaian_bidang` terhadap MySQL/MariaDB asli dengan data uji sendiri (dibersihkan sebelum dan sesudah).
//!
//! ```text
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!   cargo test -p api --test capaian_bidang_db -- --ignored
//! ```

use api::capaian_bidang::build_capaian_bidang;
use api::dashboard::{Bind, Scope};
use sqlx::MySqlPool;

const PENANDA: &str = "TEST-CAPAIAN";

async fn bersihkan(pool: &MySqlPool) {
    sqlx::query(
        "DELETE FROM tbl_kontrak WHERE id_pekerjaan IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE 'TEST-CAPAIAN%')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE 'TEST-CAPAIAN%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_kegiatan = ?")
        .bind(PENANDA)
        .execute(pool)
        .await
        .unwrap();
}

async fn kegiatan(pool: &MySqlPool, bidang: &str, tahun: &str, pagu: f64) -> u64 {
    sqlx::query("INSERT INTO tbl_kegiatan (nama_kegiatan, sub_bidang, tahun_anggaran, pagu) VALUES (?, ?, ?, ?)")
        .bind(PENANDA)
        .bind(bidang)
        .bind(tahun)
        .bind(pagu)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn paket(pool: &MySqlPool, nama: &str, kegiatan_id: u64, pagu: f64, status: &str) -> u64 {
    sqlx::query(
        "INSERT INTO tbl_pekerjaan (nama_paket, kegiatan_id, pagu, status) VALUES (?, ?, ?, ?)",
    )
    .bind(format!("{PENANDA}-{nama}"))
    .bind(kegiatan_id)
    .bind(pagu)
    .bind(status)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn riwayat(
    pool: &MySqlPool,
    pekerjaan: u64,
    jenis: &str,
    tanggal: &str,
    persen: f64,
    nilai: Option<f64>,
) {
    sqlx::query(
        "INSERT INTO pekerjaan_progress_estimasi_history (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, nilai) \
         VALUES (?, 2026, ?, 'realisasi', ?, ?, ?)",
    )
    .bind(pekerjaan)
    .bind(jenis)
    .bind(tanggal)
    .bind(persen)
    .bind(nilai)
    .execute(pool)
    .await
    .unwrap();
}

fn cari<'a>(data: &'a serde_json::Value, nama: &str) -> &'a serde_json::Value {
    data["bidang"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == nama)
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL (MySQL/MariaDB dengan skema apiamis)"]
async fn capaian_per_bidang_menghitung_paket_progres_dan_realisasi() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    bersihkan(&pool).await;

    // 2026: air minum (K1) dan sanitasi dengan penulisan berbeda (K2); 2025 air minum (K3) harus terabaikan.
    let k1 = kegiatan(&pool, "air minum", "2026", 1_000_000_000.0).await;
    let k2 = kegiatan(&pool, " Sanitasi ", "2026", 500_000_000.0).await;
    let k3 = kegiatan(&pool, "air minum", "2025", 700_000_000.0).await;

    let p1 = paket(&pool, "p1", k1, 400_000_000.0, "active").await;
    let p2 = paket(&pool, "p2", k1, 200_000_000.0, "active").await;
    let p3 = paket(&pool, "p3", k1, 100_000_000.0, "canceled").await;
    let p4 = paket(&pool, "p4", k2, 300_000_000.0, "active").await;
    let _p5 = paket(&pool, "p5", k3, 999_000_000.0, "active").await;

    sqlx::query("INSERT INTO tbl_kontrak (id_pekerjaan) VALUES (?)")
        .bind(p1)
        .execute(&pool)
        .await
        .unwrap();

    riwayat(&pool, p1, "fisik", "2026-03-01", 20.0, None).await;
    riwayat(&pool, p1, "fisik", "2026-06-01", 60.0, None).await; // terbaru paket 1
    riwayat(&pool, p2, "fisik", "2026-04-01", 40.0, None).await;
    riwayat(&pool, p3, "fisik", "2026-05-01", 90.0, None).await; // paket batal: terabaikan
    riwayat(&pool, p4, "fisik", "2026-05-01", 10.0, None).await;
    riwayat(
        &pool,
        p1,
        "keuangan",
        "2026-06-01",
        0.0,
        Some(100_000_000.0),
    )
    .await;
    riwayat(&pool, p4, "keuangan", "2026-06-01", 0.0, Some(30_000_000.0)).await;

    let scope = Scope {
        tahun: Some(Bind::Text("2026".into())),
        ..Default::default()
    };
    let data = build_capaian_bidang(&pool, &scope, "2026").await.unwrap();
    bersihkan(&pool).await;

    assert_eq!(data["tahun"], "2026");
    assert_eq!(data["bidang"].as_array().unwrap().len(), 2);

    let air = cari(&data, "air minum");
    assert_eq!(air["kegiatan"], 1);
    assert_eq!(air["paguKegiatan"], 1_000_000_000.0);
    assert_eq!(air["paket"], 2);
    assert_eq!(air["berkontrak"], 1);
    assert_eq!(air["belumBerkontrak"], 1);
    assert_eq!(air["batal"], 1);
    assert_eq!(air["pagu"], 600_000_000.0);
    assert_eq!(air["progressFisik"], 50.0);
    assert_eq!(air["paketBerprogres"], 2);
    assert_eq!(air["sp2dTotal"], 100_000_000.0);
    assert_eq!(air["realisasiKeuanganPersen"], 16.7);

    let san = cari(&data, "sanitasi");
    assert_eq!(san["kegiatan"], 1);
    assert_eq!(san["paket"], 1);
    assert_eq!(san["berkontrak"], 0);
    assert_eq!(san["belumBerkontrak"], 1);
    assert_eq!(san["batal"], 0);
    assert_eq!(san["pagu"], 300_000_000.0);
    assert_eq!(san["progressFisik"], 10.0);
    assert_eq!(san["sp2dTotal"], 30_000_000.0);
    assert_eq!(san["realisasiKeuanganPersen"], 10.0);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL (MySQL/MariaDB dengan skema apiamis)"]
async fn tahun_tanpa_data_tetap_mengembalikan_dua_bidang_bernilai_nol() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    bersihkan(&pool).await;

    let scope = Scope {
        tahun: Some(Bind::Text("1999".into())),
        ..Default::default()
    };
    let data = build_capaian_bidang(&pool, &scope, "1999").await.unwrap();

    for nama in ["air minum", "sanitasi"] {
        let b = cari(&data, nama);
        assert_eq!(b["paket"], 0);
        assert_eq!(b["pagu"], 0.0);
        assert_eq!(b["progressFisik"], 0.0);
        assert_eq!(b["realisasiKeuanganPersen"], 0.0);
    }
}
