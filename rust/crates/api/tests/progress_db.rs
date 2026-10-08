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
