//! Tes integrasi Pekerjaan (GET) terhadap MySQL dengan data `tbl_pekerjaan`.
//! Butuh data dump (bukan skema kosong), jadi tidak dijalankan di CI.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test pekerjaan_db -- --ignored
//! ```

use api::pekerjaan::{find, list, to_resource, PekerjaanFilter};
use sqlx::{MySqlPool, Row};
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

    let viewer = api::pekerjaan_rel::Viewer {
        user_id: 0,
        is_admin: true,
        nip: None,
        role_ids: vec![],
    };
    let rel = api::pekerjaan::load(
        &pool,
        std::slice::from_ref(&shown),
        api::pekerjaan::Mode {
            summary: false,
            unbounded: false,
        },
        &viewer,
    )
    .await
    .unwrap();
    let v = to_resource(&shown, &rel);
    assert_eq!(v["assignment_sources"], serde_json::json!([]));
    assert!(
        v["kontrak"].is_array(),
        "kontrak dimuat di list non-unbounded"
    );
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn kontrak_addendum_and_assignment_sources() {
    use api::pekerjaan::{find, load, Mode};
    use api::pekerjaan_rel::Viewer;

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let pid: u64 = sqlx::query("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    let p = find(&pool, pid).await.unwrap().unwrap();

    // Bersihkan sisa uji sebelumnya.
    sqlx::query("DELETE FROM tbl_kontrak WHERE spk = 'SPK-UJI-PKJ'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = 'Uji Pkj Penyedia'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = 5 AND pekerjaan_id = ?")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES ('Uji Pkj Penyedia', 'D', '1', 'N', 'A', NOW(), NOW())")
        .execute(&pool).await.unwrap();
    let penyedia: u64 =
        sqlx::query_scalar("SELECT id FROM tbl_penyedia WHERE nama = 'Uji Pkj Penyedia'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO tbl_kontrak (spk, kode_paket, tgl_spk, nilai_kontrak, id_penyedia, created_at, updated_at) VALUES ('SPK-UJI-PKJ', 'KP-1', '2025-02-01', 1500000.00, ?, NOW(), NOW())")
        .bind(penyedia).execute(&pool).await.unwrap();
    let kontrak: u64 = sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE spk = 'SPK-UJI-PKJ'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(kontrak).bind(pid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO tbl_kontrak_addendums (kontrak_id, addendum_ke, nomor_addendum, tanggal_addendum, status, created_at, updated_at) VALUES (?, 1, 'ADD-1', '2025-05-01', 'disetujui', NOW(), NOW())")
        .bind(kontrak).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (5, ?, NOW(), NOW())")
        .bind(pid).execute(&pool).await.unwrap();

    let admin = Viewer {
        user_id: 0,
        is_admin: true,
        nip: None,
        role_ids: vec![],
    };
    let rel = load(
        &pool,
        std::slice::from_ref(&p),
        Mode {
            summary: false,
            unbounded: false,
        },
        &admin,
    )
    .await
    .unwrap();
    let v = to_resource(&p, &rel);
    assert_eq!(v["has_kontrak"], true);
    assert_eq!(v["kontrak_count"], 1);
    assert_eq!(v["kontrak"][0]["spk"], "SPK-UJI-PKJ");
    assert_eq!(v["kontrak"][0]["penyedia"]["nama"], "Uji Pkj Penyedia");
    assert_eq!(v["kontrak"][0]["addendums"][0]["nomor_addendum"], "ADD-1");
    assert_eq!(v["kontrak"][0]["registers"], serde_json::json!([]));
    assert_eq!(
        v["assignment_sources"],
        serde_json::json!([]),
        "admin tidak dihitung"
    );

    let user = Viewer {
        user_id: 5,
        is_admin: false,
        nip: None,
        role_ids: vec![],
    };
    let rel = load(
        &pool,
        std::slice::from_ref(&p),
        Mode {
            summary: false,
            unbounded: false,
        },
        &user,
    )
    .await
    .unwrap();
    let v = to_resource(&p, &rel);
    assert_eq!(v["assignment_sources"], serde_json::json!(["manual"]));

    // unbounded: kontrak dan tags tidak dimuat, jadi key-nya hilang.
    let rel = load(
        &pool,
        std::slice::from_ref(&p),
        Mode {
            summary: false,
            unbounded: true,
        },
        &user,
    )
    .await
    .unwrap();
    let v = to_resource(&p, &rel);
    assert!(v.get("kontrak").is_none());
    assert!(v.get("tags").is_none());
    assert_eq!(v["progress_total"], 0);

    sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
        .bind(kontrak)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM kontrak_pekerjaan WHERE kontrak_id = ?")
        .bind(kontrak)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?")
        .bind(kontrak)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_penyedia WHERE id = ?")
        .bind(penyedia)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = 5 AND pekerjaan_id = ?")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
}
