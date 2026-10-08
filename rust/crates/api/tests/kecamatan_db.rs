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

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url)
        .await
        .expect("gagal konek ke MySQL")
}

async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_desa WHERE id >= 9000000")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id >= 9000000")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn list_counts_desa_per_kecamatan_and_orders_by_id() {
    let pool = pool().await;
    cleanup(&pool).await;

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
    let mine: Vec<_> = rows.iter().filter(|r| r.id >= 9_000_000).collect();

    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0].id, K1);
    assert_eq!(mine[0].n_kec, "Uji A");
    assert_eq!(mine[0].jumlah_desa, 2);
    assert!(mine[0].created_at.is_none());
    assert_eq!(mine[1].id, K2);
    assert_eq!(mine[1].jumlah_desa, 1);
    assert!(mine[1].created_at.is_some());

    cleanup(&pool).await;
}
