//! Uji integrasi query kecamatan terhadap MySQL sungguhan.
//!
//! Butuh struktur `tbl_kecamatan` dan `tbl_desa` (`fixtures/kecamatan_schema.sql`):
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test kecamatan_db -- --ignored
//! ```
//!
//! Test memakai id tinggi (>= 9_000_000) dan menghapus datanya sendiri.

use api::kecamatan::list;
use sqlx::MySqlPool;

const K1: u64 = 9_000_001;
const K2: u64 = 9_000_002;
const DETAIL_KEC: u64 = 9_000_150;

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url)
        .await
        .expect("gagal konek ke MySQL")
}

/// Menghapus baris uji dalam rentang id `[lo, hi)`. Tiap tes punya rentang sendiri,
/// supaya tes yang berjalan paralel tidak saling menghapus data.
async fn cleanup(pool: &MySqlPool, lo: u64, hi: u64) {
    sqlx::query("DELETE FROM tbl_desa WHERE id >= ? AND id < ?")
        .bind(lo)
        .bind(hi)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id >= ? AND id < ?")
        .bind(lo)
        .bind(hi)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn list_counts_desa_per_kecamatan_and_orders_by_id() {
    let pool = pool().await;
    cleanup(&pool, 9_000_000, 9_000_100).await;

    sqlx::query(
        "INSERT INTO tbl_kecamatan (id, n_kec, created_at, updated_at) VALUES \
         (?, 'Uji A', NULL, NULL), (?, 'Uji B', '2025-12-18 11:05:36', NULL)",
    )
    .bind(K1)
    .bind(K2)
    .execute(&pool)
    .await
    .unwrap();
    for (id, kec) in [(9_000_101u64, K1), (9_000_102, K1), (9_000_103, K2)] {
        sqlx::query("INSERT INTO tbl_desa (id, kecamatan_id, n_desa) VALUES (?, ?, 'desa uji')")
            .bind(id)
            .bind(kec)
            .execute(&pool)
            .await
            .unwrap();
    }

    let rows = list(&pool).await.unwrap();
    let mine: Vec<_> = rows
        .iter()
        .filter(|r| (9_000_000..9_000_100).contains(&r.id))
        .collect();

    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].id, K1);
    assert_eq!(mine[0].n_kec, "Uji A");
    assert_eq!(mine[0].jumlah_desa, 2);
    assert!(mine[0].created_at.is_none());
    assert_eq!(mine[1].id, K2);
    assert_eq!(mine[1].jumlah_desa, 1);
    assert!(mine[1].created_at.is_some());

    cleanup(&pool, 9_000_000, 9_000_100).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn detail_nests_desa_without_kecamatan_key() {
    use api::kecamatan::{detail_resource, find};

    let pool = pool().await;
    cleanup(&pool, 9_000_100, 9_000_300).await;
    sqlx::query("INSERT INTO tbl_kecamatan (id, n_kec, created_at, updated_at) VALUES (?, 'Uji Detail', NULL, NULL)")
        .bind(DETAIL_KEC)
        .execute(&pool)
        .await
        .unwrap();
    for id in [9_000_201u64, 9_000_202] {
        sqlx::query("INSERT INTO tbl_desa (id, kecamatan_id, n_desa) VALUES (?, ?, 'desa detail')")
            .bind(id)
            .bind(DETAIL_KEC)
            .execute(&pool)
            .await
            .unwrap();
    }

    let row = find(&pool, DETAIL_KEC)
        .await
        .unwrap()
        .expect("kecamatan ada");
    let desa = api::desa::list_for_kecamatan(&pool, DETAIL_KEC)
        .await
        .unwrap();
    let v = detail_resource(&row, &desa);

    assert_eq!(v["nama_kecamatan"], "Uji Detail");
    assert_eq!(v["jumlah_desa"], 2);
    assert_eq!(v["desa"].as_array().unwrap().len(), 2);
    assert!(
        v["desa"][0].get("kecamatan").is_none(),
        "relasi kecamatan tidak dimuat di detail"
    );

    assert!(find(&pool, 9_999_999).await.unwrap().is_none());
    cleanup(&pool, 9_000_100, 9_000_300).await;
}
