//! Tes integrasi penyedia dan document types terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test penyedia_db -- --ignored
//! ```

use api::lookup::{document_type_json, document_types};
use api::penyedia::{dokumen, find, list, PenyediaFilter};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn cleanup_penyedia(pool: &MySqlPool) {
    sqlx::query("DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Penyedia' AND model_id IN (SELECT id FROM tbl_penyedia WHERE nama LIKE 'Uji Penyedia%')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama LIKE 'Uji Penyedia%'")
        .execute(pool)
        .await
        .unwrap();
}

async fn cleanup_document_types(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_document_types WHERE code LIKE 'uji-%'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn penyedia_search_filters_and_documents_from_media() {
    let pool = pool().await;
    cleanup_penyedia(&pool).await;
    for nama in ["Uji Penyedia Alfa", "Uji Penyedia Beta"] {
        sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES (?, 'Direktur Uji', '1', 'Notaris Uji', 'Jl. Uji', NOW(), NOW())")
            .bind(nama)
            .execute(&pool)
            .await
            .unwrap();
    }
    let alfa: u64 = sqlx::query("SELECT id FROM tbl_penyedia WHERE nama = 'Uji Penyedia Alfa'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    sqlx::query("INSERT INTO media (model_type, model_id, uuid, collection_name, name, file_name, mime_type, disk, conversions_disk, size, manipulations, custom_properties, generated_conversions, responsive_images, order_column, created_at, updated_at) VALUES ('App\\\\Models\\\\Penyedia', ?, UUID(), 'penyedia/dokumen', 'akta', 'akta.pdf', 'application/pdf', 'public', 'public', 2048, '[]', '[]', '[]', '[]', 1, NOW(), NOW())")
        .bind(alfa)
        .execute(&pool)
        .await
        .unwrap();

    let filter = PenyediaFilter {
        search: Some("Uji Penyedia".into()),
    };
    let (rows, total) = list(&pool, &filter, None).await.unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|r| r.nama.as_str())
        .filter(|n| n.starts_with("Uji"))
        .collect();
    assert_eq!(names, vec!["Uji Penyedia Alfa", "Uji Penyedia Beta"]);
    assert!(total >= 2);

    let (page, _) = list(&pool, &filter, Some((1, 0))).await.unwrap();
    assert_eq!(page.len(), 1);

    let docs = dokumen(&pool, "http://apiamis.cianjur.space", alfa)
        .await
        .unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["name"], "akta.pdf");
    assert_eq!(
        docs[0]["url"],
        format!(
            "http://apiamis.cianjur.space/storage/{}/akta.pdf",
            docs[0]["id"]
        )
    );

    let row = find(&pool, alfa).await.unwrap().expect("ada");
    assert_eq!(row.tanggal_akta, None);
    assert!(find(&pool, 999_999_999).await.unwrap().is_none());

    cleanup_penyedia(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_types_are_plain_eloquent_json() {
    let pool = pool().await;
    cleanup_document_types(&pool).await;
    sqlx::query("INSERT INTO tbl_document_types (name, code, format_template, created_at, updated_at) VALUES ('Uji Dokumen', 'uji-dok', NULL, NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let all = document_types(&pool).await.unwrap();
    let mine = all.iter().find(|d| d.code == "uji-dok").expect("ada");
    let v = document_type_json(mine);
    assert_eq!(v["name"], "Uji Dokumen");
    assert_eq!(v["format_template"], serde_json::Value::Null);
    assert!(v["created_at"].as_str().unwrap().ends_with('Z'));
    cleanup_document_types(&pool).await;
}
