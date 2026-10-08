//! Tes integrasi Pekerjaan (GET) terhadap MySQL dengan data `tbl_pekerjaan`.
//! Butuh data dump (bukan skema kosong), jadi tidak dijalankan di CI.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test pekerjaan_db -- --ignored
//! ```

use api::pekerjaan::{find, list, to_resource, Loaded, PekerjaanFilter};
use sqlx::MySqlPool;
use std::collections::HashMap;

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn list_paginates_and_find_returns_same_row() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let filter = PekerjaanFilter::from_query(&HashMap::new());

    let (page1, total) = list(&pool, &filter, Some((5, 0)), None).await.unwrap();
    assert!(total > 5, "data uji harus lebih dari satu halaman");
    assert_eq!(page1.len(), 5);
    // Default: created_at desc, jadi baris pertama tidak lebih lama dari baris kedua.
    let (page2, _) = list(&pool, &filter, Some((5, 5)), None).await.unwrap();
    assert!(page1.iter().all(|a| page2.iter().all(|b| a.id != b.id)));

    let first = &page1[0];
    let shown = find(&pool, first.id).await.unwrap().expect("ada");
    assert_eq!(&shown, first);
    assert!(find(&pool, 999_999_999).await.unwrap().is_none());

    let rel = Loaded {
        kecamatan: HashMap::new(),
        desa: HashMap::new(),
        kegiatan: HashMap::new(),
        pengawas: HashMap::new(),
        tags: HashMap::new(),
    };
    let v = to_resource(&shown, &rel);
    assert_eq!(v["assignment_sources"], serde_json::Value::Null);
    assert_eq!(v["kontrak"], serde_json::Value::Null);
}
