//! Unduh semua berkas pekerjaan sebagai zip lewat router terhadap MySQL dan disk lokal.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_download_db -- --include-ignored
//! ```

use std::{io::Cursor, io::Read};

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const MARK: &str = "uji-pdl-";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Folder penyimpanan media khusus tes. Setiap tes memakai folder yang sama.
fn storage_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("uji-pdl-storage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PUBLIC_STORAGE_PATH", &dir);
    dir
}

async fn get_raw(
    pool: &MySqlPool,
    uri: &str,
    token: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::USER_AGENT, "uji-agent")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(format!("Uji {email}"))
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let rid: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
            .bind(role)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(rid)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    uid
}

/// Berkas dengan satu media. `on_disk` menentukan apakah file ikut ditulis ke folder penyimpanan.
async fn add_berkas(
    pool: &MySqlPool,
    storage: &std::path::Path,
    pekerjaan: u64,
    jenis: &str,
    file: &str,
    contents: &[u8],
    on_disk: bool,
) -> (u64, u64) {
    let berkas = sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pekerjaan)
        .bind(jenis)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
    let media = sqlx::query(
        "INSERT INTO media (model_type, model_id, collection_name, name, file_name, mime_type, disk, size, manipulations, custom_properties, generated_conversions, responsive_images, created_at, updated_at) \
         VALUES ('App\\\\Models\\\\Berkas', ?, 'berkas/dokumen', ?, ?, 'text/plain', 'public', ?, '{}', '{}', '{}', '[]', NOW(), NOW())",
    )
    .bind(berkas)
    .bind(file)
    .bind(file)
    .bind(contents.len() as u64)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    if on_disk {
        let dir = storage.join(media.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), contents).unwrap();
    }
    (berkas, media)
}

async fn cleanup(pool: &MySqlPool, pekerjaan: &[u64], users: &[u64]) {
    for p in pekerjaan {
        let berkas: Vec<u64> =
            sqlx::query_scalar("SELECT id FROM tbl_berkas WHERE pekerjaan_id = ?")
                .bind(p)
                .fetch_all(pool)
                .await
                .unwrap();
        for b in berkas {
            sqlx::query(
                "DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Berkas' AND model_id = ?",
            )
            .bind(b)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query("DELETE FROM tbl_berkas WHERE pekerjaan_id = ?")
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_pekerjaan WHERE pekerjaan_id = ?")
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
    }
    for u in users {
        sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
            .bind(u)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn new_pekerjaan(pool: &MySqlPool, nama: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, 1, 1, 'active', NOW(), NOW())")
        .bind(format!("{MARK}{nama}"))
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn zips_every_berkas_with_stored_entries_and_sanitized_name() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = storage_dir();
    let admin = make_user(&pool, "uji-pdl-a1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, admin, "uji-pdl")
        .await
        .unwrap();
    let p = new_pekerjaan(&pool, "Paket / Uji A").await;
    let (_, m1) = add_berkas(&pool, &storage, p, "SPK", "spk.txt", b"isi spk", true).await;
    let (_, m2) = add_berkas(&pool, &storage, p, "BAP Serah", "bap.txt", b"isi bap", true).await;
    add_berkas(&pool, &storage, p, "Tanpa File", "hilang.txt", b"x", false).await;

    let (status, headers, bytes) = get_raw(
        &pool,
        &format!("/api/pekerjaan/{p}/download-all-berkas"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/zip"
    );
    assert_eq!(
        headers.get(header::CONTENT_DISPOSITION).unwrap(),
        format!("attachment; filename=\"{MARK}Paket_Uji_A.zip\"").as_str()
    );
    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "no-store, private"
    );

    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(zip.len(), 2, "berkas tanpa file dilewati");
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        assert_eq!(f.compression(), zip::CompressionMethod::Stored);
        let name = f.name().to_string();
        let mut content = String::new();
        f.read_to_string(&mut content).unwrap();
        names.push((name, content));
    }
    assert!(
        names.contains(&(format!("SPK_{m1}.txt"), "isi spk".to_string())),
        "{names:?}"
    );
    assert!(
        names.contains(&(format!("BAP_Serah_{m2}.txt"), "isi bap".to_string())),
        "{names:?}"
    );

    let (status, headers, bytes) = get_raw(
        &pool,
        &format!("/api/pekerjaan/{p}/download-all-berkas?format=pdf"),
        &token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(headers
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap()
        .ends_with("_PDF.zip\""));

    cleanup(&pool, &[p], &[admin]).await;
    let _ = std::fs::remove_dir_all(&storage);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn empty_or_unreadable_berkas_return_laravel_messages_and_scope_is_enforced() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = storage_dir();
    let admin = make_user(&pool, "uji-pdl-b1@example.test", "admin").await;
    let user = make_user(&pool, "uji-pdl-b2@example.test", "user").await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-pdl")
        .await
        .unwrap();
    let user_token = auth::login::create_token(&pool, user, "uji-pdl")
        .await
        .unwrap();

    let kosong = new_pekerjaan(&pool, "Kosong").await;
    let (status, _, bytes) = get_raw(
        &pool,
        &format!("/api/pekerjaan/{kosong}/download-all-berkas"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["message"],
        "Tidak ada berkas untuk diunduh"
    );

    let hilang = new_pekerjaan(&pool, "Hilang").await;
    add_berkas(&pool, &storage, hilang, "SPK", "tidak-ada.txt", b"x", false).await;
    let (status, _, bytes) = get_raw(
        &pool,
        &format!("/api/pekerjaan/{hilang}/download-all-berkas"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["message"],
        "Tidak ada file berkas yang dapat diunduh"
    );

    let (status, _, bytes) = get_raw(
        &pool,
        &format!("/api/pekerjaan/{kosong}/download-all-berkas"),
        &user_token,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&bytes)
    );

    let (status, _, _) = get_raw(
        &pool,
        "/api/pekerjaan/999999999/download-all-berkas",
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, &[kosong, hilang], &[admin, user]).await;
    let _ = std::fs::remove_dir_all(&storage);
}
