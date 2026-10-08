//! Tes integrasi progres Pekerjaan: baris `tbl_progress` buatan dibaca lewat loader yang sebenarnya.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test progress_db -- --ignored
//! ```

use api::pekerjaan::progress_for;
use sqlx::{MySqlPool, Row};

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn progress_is_read_from_tbl_progress_content() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    let pekerjaan_id: u64 = sqlx::query("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    sqlx::query("DELETE FROM tbl_progress WHERE pekerjaan_id = ?")
        .bind(pekerjaan_id)
        .execute(&pool)
        .await
        .unwrap();

    let content = r#"{"items":[{"target_volume":10,"harga_satuan":100,"weekly_data":{"1":{"realisasi":5,"rencana":4}}}]}"#;
    sqlx::query("INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pekerjaan_id)
        .bind(content)
        .execute(&pool)
        .await
        .unwrap();

    let m = progress_for(&pool, &[pekerjaan_id]).await.unwrap();
    let got = m.get(&pekerjaan_id).expect("ada progres");
    assert_eq!(got.progress_total, 50.0);
    assert_eq!(got.progress_rencana, 40.0);
    assert_eq!(got.deviasi, 10.0);

    sqlx::query("DELETE FROM tbl_progress WHERE pekerjaan_id = ?")
        .bind(pekerjaan_id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(progress_for(&pool, &[pekerjaan_id])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn estimasi_summary_uses_kegiatan_year_and_latest_entries() {
    use api::pekerjaan::{estimasi_for, find};

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let p = find(
        &pool,
        sqlx::query("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get::<u64, _>("id")
            .unwrap(),
    )
    .await
    .unwrap()
    .expect("ada");
    let tahun: i64 =
        sqlx::query_scalar::<_, String>("SELECT tahun_anggaran FROM tbl_kegiatan WHERE id = ?")
            .bind(p.kegiatan_id.unwrap() as u64)
            .fetch_one(&pool)
            .await
            .unwrap()
            .parse()
            .unwrap();

    sqlx::query("DELETE FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id = ?")
        .bind(p.id)
        .execute(&pool)
        .await
        .unwrap();
    let rows = [
        ("fisik", "rencana", "2024-03-01", 30.0, None::<f64>),
        ("fisik", "realisasi", "2024-02-01", 20.0, None),
        ("fisik", "realisasi", "2024-06-01", 25.0, None),
        (
            "keuangan",
            "realisasi",
            "2024-05-01",
            40.0,
            Some(1_000_000.0),
        ),
    ];
    for (jenis, tipe, tgl, persen, nilai) in rows {
        sqlx::query("INSERT INTO pekerjaan_progress_estimasi_history (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, nilai, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())")
            .bind(p.id)
            .bind(tahun)
            .bind(jenis)
            .bind(tipe)
            .bind(tgl)
            .bind(persen)
            .bind(nilai)
            .execute(&pool)
            .await
            .unwrap();
    }

    let m = estimasi_for(&pool, std::slice::from_ref(&p)).await.unwrap();
    let s = m.get(&p.id).expect("ada");
    assert_eq!(s.fisik_realisasi, Some(25.0));
    assert_eq!(s.fisik_rencana, Some(30.0));
    assert_eq!(s.fisik_deviasi, Some(-5.0));
    assert_eq!(s.keuangan_realisasi, Some(40.0));
    assert_eq!(s.keuangan_deviasi, None);
    assert_eq!(s.keuangan_realisasi_nilai, Some(1_000_000.0));

    sqlx::query("DELETE FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id = ?")
        .bind(p.id)
        .execute(&pool)
        .await
        .unwrap();
}
