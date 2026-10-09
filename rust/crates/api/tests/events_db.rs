//! Rute `/api/events` lewat router terhadap MySQL: CRUD pemilik, 403 untuk pengguna lain, upload
//! lampiran ke media, dan validasi 422.
//!
//! Membutuhkan tabel `tbl_events` (lihat `rust/fixtures/events_schema.sql`).
//! Setiap tes memakai email `uji-ev-*` sendiri dan hanya menghapus data milik email itu.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test events_db -- --include-ignored
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

const BOUNDARY: &str = "----ujievboundary";
const EVENT_MODEL: &str = "App\\Models\\Event";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn run(pool: &MySqlPool, req: Request<Body>) -> (StatusCode, Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req)
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

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
    };
    run(pool, req.body(body).unwrap()).await
}

async fn send_multipart(
    pool: &MySqlPool,
    uri: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    run(pool, req).await
}

/// Body multipart dengan satu field `file`.
fn multipart_file(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Body multipart tanpa field `file`.
fn multipart_empty() -> Vec<u8> {
    format!("--{BOUNDARY}--\r\n").into_bytes()
}

/// Pengguna uji biasa (tanpa peran) dengan token Sanctum.
async fn user_token(pool: &MySqlPool, email: &str) -> (u64, String) {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Ev', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-ev")
        .await
        .unwrap();
    (uid, token)
}

/// Hapus hanya data milik `emails`: audit, media lampiran, event, lalu pengguna.
/// Berkas di disk dihapus oleh tes upload sendiri.
async fn clean(pool: &MySqlPool, emails: &[&str]) {
    for &email in emails {
        sqlx::query(
            "DELETE FROM tbl_audit_logs WHERE auditable_type = ? \
             AND user_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(EVENT_MODEL)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM media WHERE model_type = ? AND model_id IN \
             (SELECT e.id FROM tbl_events e JOIN users u ON u.id = e.user_id WHERE u.email = ?)",
        )
        .bind(EVENT_MODEL)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM tbl_events WHERE user_id IN (SELECT id FROM users WHERE email = ?)",
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
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_events"]
async fn events_requires_login_and_validates_input() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const OWNER: &str = "uji-ev-validasi@example.test";
    clean(&pool, &[OWNER]).await;
    let (_, token) = user_token(&pool, OWNER).await;

    // Tanpa token: 401 untuk daftar, buat, dan detail.
    let (status, _) = send(&pool, Method::GET, "/api/events", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/events",
        None,
        Some(json!({"title": "uji-ev-tanpa-token"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&pool, Method::GET, "/api/events/1", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Body kosong: 422 dengan pesan Laravel dan error per field.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The given data was invalid.", "{body}");
    assert_eq!(body["errors"]["title"][0], "The title field is required.", "{body}");
    assert_eq!(body["errors"]["start"][0], "The start field is required.", "{body}");
    assert_eq!(body["errors"]["end"][0], "The end field is required.", "{body}");

    // Tanggal tidak valid dan kategori tidak dikenal.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&token),
        Some(json!({
            "title": "uji-ev-salah",
            "start": "bukan-tanggal",
            "end": "2026-10-10T10:00:00",
            "category": "salah"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["start"][0], "The start field must be a valid date.", "{body}");
    assert_eq!(body["errors"]["category"][0], "The selected category is invalid.", "{body}");

    // End sebelum start.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&token),
        Some(json!({
            "title": "uji-ev-urut",
            "start": "2026-10-10T10:00:00",
            "end": "2026-10-10T09:00:00"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["errors"]["end"][0],
        "The end field must be a date after or equal to start.",
        "{body}"
    );

    // Event valid untuk tes update di bawah.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&token),
        Some(json!({"title": "uji-ev-valid", "start": "2026-10-10T08:00:00", "end": "2026-10-10T09:00:00"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/events/{id}"),
        Some(&token),
        Some(json!({"title": 5})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["title"][0], "The title field must be a string.", "{body}");

    clean(&pool, &[OWNER]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_events"]
async fn events_owner_crud_and_other_user_gets_403() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const OWNER: &str = "uji-ev-crud-owner@example.test";
    const OTHER: &str = "uji-ev-crud-lain@example.test";
    clean(&pool, &[OWNER, OTHER]).await;
    let (_, owner) = user_token(&pool, OWNER).await;
    let (_, other) = user_token(&pool, OTHER).await;

    // Create: judul dan lokasi di-trim, kategori dari request, start disimpan sebagai jam WIB.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&owner),
        Some(json!({
            "title": "  uji-ev-rapat  ",
            "start": "2026-10-10T08:00:00",
            "end": "2026-10-10T10:00:00",
            "category": "task",
            "location": "  Ruang rapat  ",
            "is_allday": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "uji-ev-rapat", "{body}");
    assert_eq!(body["data"]["category"], "task", "{body}");
    assert_eq!(body["data"]["location"], "Ruang rapat", "{body}");
    assert_eq!(body["data"]["isAllday"], false, "{body}");
    // 08:00 WIB dikembalikan sebagai 01:00 UTC.
    assert!(
        body["data"]["start"].as_str().unwrap().starts_with("2026-10-10T01:00:00"),
        "{body}"
    );
    let id = body["data"]["id"].as_i64().unwrap();
    let jam: String = sqlx::query_scalar("SELECT DATE_FORMAT(`start`, '%H:%i') FROM tbl_events WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jam, "08:00");

    // List: event manual milik pemilik ada di daftar. Pengguna lain tidak melihatnya.
    let (status, body) = send(&pool, Method::GET, "/api/events", Some(&owner), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"].as_array().unwrap().iter().any(|e| e["id"] == id),
        "{body}"
    );
    let (status, body) = send(&pool, Method::GET, "/api/events", Some(&other), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body["data"].as_array().unwrap().iter().any(|e| e["id"] == id),
        "{body}"
    );

    // Show oleh pemilik.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/events/{id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "uji-ev-rapat", "{body}");

    // Update oleh pemilik: PUT mengubah judul dan kategori, PATCH mengosongkan lokasi dan mengubah isAllday.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/events/{id}"),
        Some(&owner),
        Some(json!({"title": "uji-ev-rapat-ubah", "category": "milestone"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "uji-ev-rapat-ubah", "{body}");
    assert_eq!(body["data"]["category"], "milestone", "{body}");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/events/{id}"),
        Some(&owner),
        Some(json!({"location": null, "is_allday": 1})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"]["location"].is_null(), "{body}");
    assert_eq!(body["data"]["isAllday"], true, "{body}");

    // Pengguna lain: show, update, dan delete ditolak 403. Baris tidak berubah.
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/events/{id}"),
        Some(&other),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/events/{id}"),
        Some(&other),
        Some(json!({"title": "diambil orang lain"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/events/{id}"),
        Some(&other),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let judul: String = sqlx::query_scalar("SELECT title FROM tbl_events WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(judul, "uji-ev-rapat-ubah");

    // Delete oleh pemilik: baris hilang, detail berikutnya 404.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/events/{id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Event deleted successfully", "{body}");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/events/{id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let sisa: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_events WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sisa, 0);

    clean(&pool, &[OWNER, OTHER]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_events"]
async fn events_upload_stores_media_and_attachment() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const OWNER: &str = "uji-ev-up-owner@example.test";
    const OTHER: &str = "uji-ev-up-lain@example.test";
    let storage = std::env::temp_dir().join(format!("uji-ev-upload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    clean(&pool, &[OWNER, OTHER]).await;
    let (_, owner) = user_token(&pool, OWNER).await;
    let (_, other) = user_token(&pool, OTHER).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/events",
        Some(&owner),
        Some(json!({
            "title": "uji-ev-lampiran",
            "start": "2026-10-11T08:00:00",
            "end": "2026-10-11T09:00:00"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let uri = format!("/api/events/{id}/upload");

    // Upload berkas kecil oleh pemilik: lampiran masuk ke media koleksi event/attachments.
    let isi: &[u8] = b"%PDF-uji-ev";
    let (status, body) = send_multipart(
        &pool,
        &uri,
        &owner,
        multipart_file("catatan.pdf", isi),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let lampiran = body["data"]["attachments"].as_array().unwrap();
    assert_eq!(lampiran.len(), 1, "{body}");
    let item = &lampiran[0];
    let mid = item["id"].as_u64().unwrap();
    let nama = item["name"].as_str().unwrap().to_string();
    assert!(nama.ends_with(".pdf"), "{body}");
    assert_eq!(item["type"], "application/pdf", "{body}");
    assert_eq!(item["size"], isi.len() as u64, "{body}");
    assert_eq!(item["url"], format!("http://localhost/storage/{mid}/{nama}"), "{body}");

    let (koleksi, nama_disk): (String, String) =
        sqlx::query_as("SELECT collection_name, file_name FROM media WHERE id = ?")
            .bind(mid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(koleksi, "event/attachments");
    assert_eq!(nama_disk, nama);
    let berkas = storage.join(mid.to_string()).join(&nama_disk);
    assert_eq!(std::fs::read(&berkas).unwrap(), isi.to_vec());

    // Upload kedua menambah lampiran, tidak mengganti yang lama.
    let (status, body) = send_multipart(
        &pool,
        &uri,
        &owner,
        multipart_file("kedua.pdf", b"%PDF-kedua"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["attachments"].as_array().unwrap().len(), 2, "{body}");
    let jumlah_media: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media WHERE model_type = ? AND model_id = ?",
    )
    .bind(EVENT_MODEL)
    .bind(id as u64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(jumlah_media, 2);

    // Tanpa field file: 422.
    let (status, body) = send_multipart(&pool, &uri, &owner, multipart_empty()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The given data was invalid.", "{body}");
    assert_eq!(body["errors"]["file"][0], "The file field is required.", "{body}");

    // Pengguna lain: 403, dan media tidak bertambah.
    let (status, _) = send_multipart(
        &pool,
        &uri,
        &other,
        multipart_file("lain.pdf", b"%PDF-lain"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let jumlah_media: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media WHERE model_type = ? AND model_id = ?",
    )
    .bind(EVENT_MODEL)
    .bind(id as u64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(jumlah_media, 2);

    // Delete event oleh pemilik: media ikut terhapus, dan direktori berkasnya juga.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/events/{id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sisa: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media WHERE model_type = ? AND model_id = ?",
    )
    .bind(EVENT_MODEL)
    .bind(id as u64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sisa, 0);
    assert!(!storage.join(mid.to_string()).exists());

    clean(&pool, &[OWNER, OTHER]).await;
    let _ = std::fs::remove_dir_all(&storage);
}
