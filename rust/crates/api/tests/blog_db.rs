//! Blog lewat router terhadap MySQL: baca publik, tulis, fitur utama, dan unggah video.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test blog_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const BOUNDARY: &str = "ujiBlogBoundary";

/// Email uji per tes. Awalan `uji-blog-<tag>-` dipakai juga untuk pembersihan, jadi tes lain tidak tersentuh.
fn email(tag: &str, who: &str) -> String {
    format!("uji-blog-{tag}-{who}@example.test")
}

/// Kategori uji per tes.
fn category(tag: &str) -> String {
    format!("uji-blog-{tag}")
}

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Body,
    content_type: Option<String>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(body).unwrap())
    .await
    .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn send_json(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    send(
        pool,
        method,
        uri,
        token,
        Body::from(value.to_string()),
        Some("application/json".to_string()),
    )
    .await
}

async fn get(pool: &MySqlPool, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    send(pool, Method::GET, uri, token, Body::empty(), None).await
}

/// Body multipart dengan field teks dan berkas opsional (`file`, `poster`).
fn multipart(fields: &[(&str, &str)], files: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    for (name, filename, bytes) in files {
        out.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

fn multipart_ct() -> Option<String> {
    Some(format!("multipart/form-data; boundary={BOUNDARY}"))
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)",
    )
    .bind(email)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Blog', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if admin {
        sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
            .execute(pool)
            .await
            .unwrap();
        let role: u64 = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
            .bind(role)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    auth::login::create_token(pool, uid, "uji-blog")
        .await
        .unwrap()
}

async fn user_id(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus baris uji milik satu tes (`tag`). Hanya slug dan email dengan awalan `uji-blog-<tag>`.
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let slug = format!("uji-blog-{tag}%");
    let mail = format!("uji-blog-{tag}-%@example.test");
    let users = "SELECT id FROM users WHERE email LIKE ?";
    sqlx::query(&format!(
        "DELETE FROM media WHERE model_type = 'App\\\\Models\\\\BlogAsset' AND model_id IN \
         (SELECT id FROM tbl_blog_assets WHERE user_id IN ({users}))"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_blog_assets WHERE user_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_blog WHERE slug LIKE ?")
        .bind(&slug)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "DELETE FROM notifications WHERE notifiable_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Blog' AND user_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM model_has_roles WHERE model_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE ?")
        .bind(&mail)
        .execute(pool)
        .await
        .unwrap();
}

/// Artikel langsung lewat SQL, untuk menyiapkan kondisi (terbit, internal, dan kategori uji).
async fn insert_blog(
    pool: &MySqlPool,
    tag: &str,
    slug: &str,
    user: u64,
    published: bool,
    internal: bool,
) -> u64 {
    let res = sqlx::query(
        "INSERT INTO tbl_blog (title, slug, content, category, user_id, is_published, is_internal, published_at, created_at, updated_at) \
         VALUES (?, ?, 'isi uji', ?, ?, ?, ?, IF(?, NOW(), NULL), NOW(), NOW())",
    )
    .bind(format!("Judul {slug}"))
    .bind(slug)
    .bind(category(tag))
    .bind(user)
    .bind(published as i64)
    .bind(internal as i64)
    .bind(published as i64)
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_id()
}

async fn audit_count(pool: &MySqlPool, event: &str, id: u64) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Blog' AND auditable_id = ? AND event = ?",
    )
    .bind(id)
    .bind(event)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn blog_index_dan_show_mengikuti_visibilitas() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let cat_name = category("ix");
    cleanup(&pool, "ix").await;
    let admin = user_token(&pool, &email("ix", "admin"), true).await;
    let uid = user_id(&pool, &email("ix", "admin")).await;
    let pub_id = insert_blog(&pool, "ix", "uji-blog-ix-pub", uid, true, false).await;
    let draft_id = insert_blog(&pool, "ix", "uji-blog-ix-draft", uid, false, false).await;
    let _internal_id = insert_blog(&pool, "ix", "uji-blog-ix-int", uid, true, true).await;

    // Tamu: hanya artikel terbit dan publik. Tanpa `comments_count`.
    let (status, body) = get(&pool, &format!("/api/blog?category={cat_name}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let slugs: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["uji-blog-ix-pub"], "{body}");
    assert!(body["data"][0].get("comments_count").is_none(), "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["meta"]["per_page"], 15);
    assert!(body["data"][0]["published_at"]
        .as_str()
        .unwrap()
        .ends_with("+00:00"));

    // Login: semua artikel dan `comments_count` ada.
    let (status, body) = get(
        &pool,
        &format!("/api/blog?category={cat_name}"),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 3, "{body}");
    assert!(body["data"][0].get("comments_count").is_some(), "{body}");

    // `published=0` hanya untuk yang login.
    let (_, body) = get(
        &pool,
        &format!("/api/blog?category={cat_name}&published=0"),
        Some(&admin),
    )
    .await;
    assert_eq!(body["meta"]["total"], 1, "{body}");
    assert_eq!(body["data"][0]["slug"], "uji-blog-ix-draft");

    // Show: id atau slug. Artikel internal ditolak tanpa login.
    let (status, body) = get(&pool, &format!("/api/blog/{pub_id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["slug"], "uji-blog-ix-pub");
    let (status, body) = get(&pool, "/api/blog/uji-blog-pub", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Laravel tidak memeriksa `is_published` pada show: artikel draf tetap terbuka untuk tamu.
    let (status, _) = get(&pool, &format!("/api/blog/{draft_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get(&pool, "/api/blog/uji-blog-int", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "Postingan ini hanya untuk internal.");
    let (status, body) = get(&pool, "/api/blog/uji-blog-int", Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = get(&pool, "/api/blog/999999999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "Not Found.");

    cleanup(&pool, "ix").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn blog_store_update_destroy_dengan_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let cat_name = category("st");
    cleanup(&pool, "st").await;
    user_token(&pool, &email("st", "admin"), true).await;
    let plain = user_token(&pool, &email("st", "plain"), false).await;

    // Tanpa login: 401.
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        None,
        json!({"title": "uji-blog-st x", "content": "y"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    // Validasi.
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"content": "y"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The title field is required.");
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"title": "uji-blog-st x", "content": "y", "is_published": "true"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "The is published field must be true or false."
    );

    // Pengguna biasa boleh menulis (`/blog` ada di daftar izin). Slug dibuat dari judul.
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"title": "uji-blog-st Artikel Baru", "content": "<p>isi</p>", "category": cat_name, "is_published": true}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["message"], "Artikel berhasil dibuat");
    let slug = body["data"]["slug"].as_str().unwrap().to_string();
    assert!(slug.starts_with("uji-blog-st-artikel-baru-"), "{slug}");
    assert_eq!(slug.len(), "uji-blog-st-artikel-baru-".len() + 5, "{slug}");
    assert_eq!(body["data"]["comments_count"], 0);
    assert!(body["data"]["published_at"].is_string());
    let id = body["data"]["id"].as_u64().unwrap();
    assert_eq!(audit_count(&pool, "created", id).await, 1);

    // Slug yang sudah dipakai: 422 dengan pesan Laravel.
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"title": "uji-blog-st lain", "slug": slug, "content": "y"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The slug has already been taken.");

    // Draf lalu terbit: `published_at` terisi saat pertama kali terbit.
    let (_, draft) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"title": "uji-blog-st draf", "content": "y"}),
    )
    .await;
    let draft_id = draft["data"]["id"].as_u64().unwrap();
    assert!(draft["data"]["published_at"].is_null(), "{draft}");
    let (status, body) = send_json(
        &pool,
        Method::PUT,
        &format!("/api/blog/{draft_id}"),
        Some(&plain),
        json!({"is_published": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"]["published_at"].is_string(), "{body}");
    assert_eq!(body["message"], "Artikel berhasil diperbarui");

    // Ubah judul: audit `updated` hanya untuk kolom yang berubah.
    let (status, body) = send_json(
        &pool,
        Method::PUT,
        &format!("/api/blog/{id}"),
        Some(&plain),
        json!({"title": "uji-blog-st Judul Baru"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "uji-blog-st Judul Baru");
    assert_eq!(body["data"]["slug"], slug.as_str());
    assert_eq!(audit_count(&pool, "updated", id).await, 1);
    let new_values: String = sqlx::query_scalar(
        "SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Blog' AND auditable_id = ? AND event = 'updated' LIMIT 1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(new_values.contains("Judul Baru"), "{new_values}");
    assert!(!new_values.contains("slug"), "{new_values}");

    // Nilai yang sama tidak menulis apa pun.
    let (status, _) = send_json(
        &pool,
        Method::PUT,
        &format!("/api/blog/{id}"),
        Some(&plain),
        json!({"title": "uji-blog-st Judul Baru"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_count(&pool, "updated", id).await, 1);

    // Parit dengan Laravel: segmen URL berupa slug tidak mengecualikan slug itu sendiri dari `unique`.
    let (status, body) = send_json(
        &pool,
        Method::PUT,
        &format!("/api/blog/{slug}"),
        Some(&plain),
        json!({"slug": slug}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The slug has already been taken.");

    // Hapus: audit `deleted`, dan artikel hilang.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/blog/{id}"),
        Some(&plain),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Artikel berhasil dihapus");
    assert_eq!(audit_count(&pool, "deleted", id).await, 1);
    let (status, _) = get(&pool, &format!("/api/blog/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, "st").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn blog_feature_dan_unfeature() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let cat_name = category("ft");
    cleanup(&pool, "ft").await;
    let admin = user_token(&pool, &email("ft", "admin"), true).await;
    let uid = user_id(&pool, &email("ft", "admin")).await;
    let a = insert_blog(&pool, "ft", "uji-blog-ft-fa", uid, true, false).await;
    let b = insert_blog(&pool, "ft", "uji-blog-ft-fb", uid, true, false).await;
    let draft = insert_blog(&pool, "ft", "uji-blog-ft-fdraft", uid, false, false).await;
    let internal = insert_blog(&pool, "ft", "uji-blog-ft-fint", uid, true, true).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/blog/{draft}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Hanya publikasi yang sudah terbit dan bersifat publik yang dapat dijadikan artikel utama."
    );
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/blog/{internal}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Artikel utama hanya satu: menjadikan B utama mengosongkan A.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/blog/{a}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["is_featured"], true);
    assert_eq!(body["message"], "Artikel utama berhasil diperbarui");
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/blog/{b}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let featured_a: i64 =
        sqlx::query_scalar("SELECT CAST(is_featured AS SIGNED) FROM tbl_blog WHERE id = ?")
            .bind(a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(featured_a, 0);

    // Menjadikan artikel yang sudah utama sekali lagi tidak menurunkannya (perbaikan atas Laravel).
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/blog/{b}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["is_featured"], true, "{body}");
    let (_, list) = get(
        &pool,
        &format!("/api/blog?category={cat_name}&featured=1"),
        None,
    )
    .await;
    let slugs: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["uji-blog-ft-fb"], "{list}");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/blog/{b}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["is_featured"], false);
    assert_eq!(body["message"], "Artikel tidak lagi menjadi artikel utama");
    // Sudah tidak utama: tidak ada perubahan (updated_at dan audit tetap).
    let before = audit_count(&pool, "updated", a).await;
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/blog/{a}/feature"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_count(&pool, "updated", a).await, before);

    cleanup(&pool, "ft").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn blog_upload_video_dan_tautan_di_konten() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool, "up").await;
    let storage = std::env::temp_dir().join(format!("uji-blog-up-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    let plain = user_token(&pool, &email("up", "plain"), false).await;

    let mut mp4 = vec![0, 0, 0, 0x18];
    mp4.extend_from_slice(b"ftypisom");
    mp4.extend_from_slice(&[0u8; 64]);
    let jpeg = [&[0xFFu8, 0xD8, 0xFF, 0xE0][..], &[0u8; 32][..]].concat();

    // Tanpa login.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/blog/upload-video",
        None,
        Body::from(multipart(&[], &[("file", "uji.mp4", &mp4)])),
        multipart_ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Validasi.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/blog/upload-video",
        Some(&plain),
        Body::from(multipart(&[], &[])),
        multipart_ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The file field is required.");
    let pdf = b"%PDF-1.4 bukan video";
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/blog/upload-video",
        Some(&plain),
        Body::from(multipart(&[], &[("file", "uji.pdf", pdf)])),
        multipart_ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "The file field must be a file of type: video/mp4, video/webm, video/quicktime."
    );
    let gif = b"GIF89a....";
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/blog/upload-video",
        Some(&plain),
        Body::from(multipart(
            &[],
            &[("file", "uji.mp4", &mp4), ("poster", "p.gif", gif)],
        )),
        multipart_ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "The poster field must be a file of type: jpeg, jpg, png, webp."
    );

    // Berhasil dengan poster.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/blog/upload-video",
        Some(&plain),
        Body::from(multipart(
            &[],
            &[("file", "uji video.mp4", &mp4), ("poster", "p.jpg", &jpeg)],
        )),
        multipart_ct(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Video berhasil diunggah");
    let video_url = body["url"].as_str().unwrap().to_string();
    let media_id = body["media_id"].as_u64().unwrap();
    assert!(
        video_url.starts_with("http://localhost/storage/"),
        "{video_url}"
    );
    assert!(video_url.ends_with(".mp4"), "{video_url}");
    assert!(body["poster_url"].is_string(), "{body}");
    assert!(storage.join(media_id.to_string()).is_dir());
    let asset_id: u64 = sqlx::query_scalar(
        "SELECT a.id FROM tbl_blog_assets a JOIN media m ON m.model_type = 'App\\\\Models\\\\BlogAsset' AND m.model_id = a.id \
         WHERE m.id = ? AND m.collection_name = 'blog/videos'",
    )
    .bind(media_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Artikel yang memuat URL video menautkan aset itu ke artikel.
    let content = format!("<p>Lihat</p><video controls src=\"{video_url}\"></video>");
    let (status, body) = send_json(
        &pool,
        Method::POST,
        "/api/blog",
        Some(&plain),
        json!({"title": "uji-blog-up dengan video", "content": content}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let blog_id = body["data"]["id"].as_u64().unwrap();
    let linked: Option<u64> =
        sqlx::query_scalar("SELECT blog_id FROM tbl_blog_assets WHERE id = ?")
            .bind(asset_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(linked, Some(blog_id));

    let _ = std::fs::remove_dir_all(&storage);
    cleanup(&pool, "up").await;
}
