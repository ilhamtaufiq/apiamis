//! Tes integrasi Tiket dan Checklist terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test tiket_checklist_db -- --ignored
//! ```

use api::checklist::{items_in_context, pekerjaan_entry, pekerjaan_page, PekerjaanFilter};
use api::pekerjaan_rel::Viewer;
use api::tiket::{self, TiketFilter};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn insert_user(pool: &MySqlPool, name: &str) -> u64 {
    let email = format!("{}@uji.test", name.replace(' ', "-").to_lowercase());
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(name)
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn cleanup_tiket(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_tiket_comment WHERE tiket_id IN (SELECT id FROM tbl_tiket WHERE subjek LIKE 'Uji Tiket%')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_tiket WHERE subjek LIKE 'Uji Tiket%'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tiket_index_scopes_non_admin_and_loads_comments() {
    let pool = pool().await;
    cleanup_tiket(&pool).await;
    let a = insert_user(&pool, "Uji Tiket A").await;
    let b = insert_user(&pool, "Uji Tiket B").await;
    for (uid, subjek) in [(a, "Uji Tiket 1"), (b, "Uji Tiket 2")] {
        sqlx::query("INSERT INTO tbl_tiket (user_id, subjek, deskripsi, kategori, prioritas, status, created_at, updated_at) VALUES (?, ?, 'd', 'bug', 'high', 'open', NOW(), NOW())")
            .bind(uid)
            .bind(subjek)
            .execute(&pool)
            .await
            .unwrap();
    }
    let t1: u64 = sqlx::query_scalar("SELECT id FROM tbl_tiket WHERE subjek = 'Uji Tiket 1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for msg in ["pertama", "kedua"] {
        sqlx::query("INSERT INTO tbl_tiket_comment (tiket_id, user_id, message, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
            .bind(t1)
            .bind(a)
            .bind(msg)
            .execute(&pool)
            .await
            .unwrap();
    }

    // Non-admin A: hanya tiketnya sendiri.
    let own = TiketFilter {
        user_id: Some(a),
        ..Default::default()
    };
    let (rows, _) = tiket::list(&pool, &own, 1, 50).await.unwrap();
    let subjects: Vec<&str> = rows
        .iter()
        .map(|r| r.subjek.as_str())
        .filter(|s| s.starts_with("Uji Tiket"))
        .collect();
    assert_eq!(subjects, vec!["Uji Tiket 1"]);

    // Admin: kedua tiket terlihat.
    let (all, _) = tiket::list(&pool, &TiketFilter::default(), 1, 500)
        .await
        .unwrap();
    assert!(
        all.iter()
            .filter(|r| r.subjek.starts_with("Uji Tiket"))
            .count()
            >= 2
    );

    let viewer = Viewer {
        user_id: a,
        is_admin: false,
        nip: None,
        role_ids: vec![],
    };
    let row = tiket::find(&pool, t1).await.unwrap().expect("ada");
    let v = tiket::to_resource(&pool, "http://apiamis.test", &row, &viewer)
        .await
        .unwrap();
    assert_eq!(v["user"]["name"], "Uji Tiket A");
    assert_eq!(v["pekerjaan"], serde_json::Value::Null);
    assert_eq!(v["image_url"], "");
    let comments = v["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0]["message"], "pertama");
    assert_eq!(comments[1]["user"]["name"], "Uji Tiket A");
    assert!(v["created_at"].as_str().unwrap().ends_with('Z'));

    cleanup_tiket(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn checklist_entry_reports_checks_and_last_update() {
    let pool = pool().await;
    let uid = insert_user(&pool, "Uji Checklist").await;
    sqlx::query("DELETE FROM pekerjaan_checklist WHERE checklist_item_id IN (SELECT id FROM tbl_checklist_items WHERE context = 'uji')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE context = 'uji'")
        .execute(&pool)
        .await
        .unwrap();
    for (name, order) in [("Uji Dokumen", 2), ("Uji Foto", 1)] {
        sqlx::query("INSERT INTO tbl_checklist_items (name, sort_order, context, created_at, updated_at) VALUES (?, ?, 'uji', NOW(), NOW())")
            .bind(name)
            .bind(order)
            .execute(&pool)
            .await
            .unwrap();
    }
    let items = items_in_context(&pool, "uji").await.unwrap();
    assert_eq!(
        items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        vec!["Uji Foto", "Uji Dokumen"]
    );

    let pid: u64 = sqlx::query("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    let foto = items[0].id;
    sqlx::query("INSERT INTO pekerjaan_checklist (pekerjaan_id, checklist_item_id, is_checked, checked_at, checked_by, notes, created_at, updated_at) VALUES (?, ?, 1, '2025-06-01 08:00:00', ?, 'catatan', '2025-06-01 08:00:00', '2025-06-02 09:30:00')")
        .bind(pid)
        .bind(foto)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

    let (page, total) = pekerjaan_page(&pool, &PekerjaanFilter::default(), 1, 15)
        .await
        .unwrap();
    assert!(total >= 1);
    let p = page.iter().find(|p| p.id == pid).expect("ada");
    let entry = pekerjaan_entry(&pool, p, &items).await.unwrap();
    let checklist = entry["checklist"].as_object().unwrap();
    assert_eq!(checklist.len(), 2);
    let c = &checklist[&foto.to_string()];
    assert_eq!(c["is_checked"], true);
    assert_eq!(c["checked_by_name"], "Uji Checklist");
    assert_eq!(c["updated_at"], "2025-06-02 09:30:00");
    assert_eq!(entry["last_updated_at"], "2025-06-02 09:30:00");
    assert_eq!(entry["last_updated_by_name"], "Uji Checklist");
    let dokumen = &checklist[&items[1].id.to_string()];
    assert_eq!(dokumen["is_checked"], false);
    assert_eq!(dokumen["updated_at"], serde_json::Value::Null);

    sqlx::query("DELETE FROM pekerjaan_checklist WHERE checklist_item_id IN (SELECT id FROM tbl_checklist_items WHERE context = 'uji')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE context = 'uji'")
        .execute(&pool)
        .await
        .unwrap();
}
