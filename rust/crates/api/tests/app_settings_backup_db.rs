//! Backup sistem lewat router terhadap MySQL: buat backup lokal, batalkan job, uji S3 (stub SigV4),
//! dan restore.
//!
//! Restore HANYA dijalankan terhadap database terpisah `apiamis_uji_backup` (dibuat dan dihapus tes).
//! Database `apiamis` hanya dibaca (dump) dan tidak diubah. Berkas backup dan media memakai
//! direktori sementara di `CARGO_TARGET_TMPDIR`. Baris yang ditulis memakai awalan `uji-asb-`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test app_settings_backup_db -- --include-ignored
//! ```

use std::{
    io::{Cursor, Write as _},
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use api::{app, app_settings_backup::restore_from_zip, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use tower::ServiceExt;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const ADMIN: &str = "uji-asb-admin@example.test";
const PLAIN: &str = "uji-asb-plain@example.test";
const UJI_DB: &str = "apiamis_uji_backup";
const BOUNDARY: &str = "ujiAsbBoundary";
const MULTIPART_CT: &str = "multipart/form-data; boundary=ujiAsbBoundary";
const MARKER: &str = "/*__ARUMANIS_STMT__*/\n";

static LOCK: Mutex<()> = Mutex::const_new(());

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 16 * 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// `PUBLIC_STORAGE_PATH` dan `PRIVATE_STORAGE_PATH` diarahkan ke direktori sementara tes.
fn setup_env() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uji-app-settings-backup");
    std::env::set_var("PUBLIC_STORAGE_PATH", base.join("public"));
    std::env::set_var("PRIVATE_STORAGE_PATH", base.join("private"));
    // Panggilan ke stub di 127.0.0.1 tidak boleh lewat proxy HTTP.
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    base
}

fn database_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL harus diisi (lihat docs tes)")
}

/// URL database `apiamis_uji_backup`: sama dengan `DATABASE_URL`, hanya nama database diganti.
/// Tes berhenti bila nama database asal bukan `apiamis`.
fn uji_url() -> String {
    let url = database_url();
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b.to_string(), Some(q.to_string())),
        None => (url.clone(), None),
    };
    let (prefix, name) = base
        .rsplit_once('/')
        .expect("DATABASE_URL memuat nama database");
    assert_eq!(
        name, "apiamis",
        "tes restore hanya mau berjalan dari DATABASE_URL apiamis"
    );
    let mut out = format!("{prefix}/{UJI_DB}");
    if let Some(q) = query {
        out.push('?');
        out.push_str(&q);
    }
    out
}

async fn pool() -> MySqlPool {
    MySqlPool::connect(&database_url())
        .await
        .expect("koneksi MySQL")
}

async fn send_raw(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Vec<u8>,
    content_type: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
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
    .oneshot(req.body(Body::from(body)).unwrap())
    .await
    .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let (status, _, bytes) = send_raw(
        pool,
        method,
        uri,
        token,
        serde_json::to_vec(&body).unwrap(),
        Some("application/json"),
    )
    .await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    let (status, _, bytes) = send_raw(pool, Method::GET, uri, Some(token), Vec::new(), None).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Multipart: field teks dan satu berkas opsional (`backup_file`).
fn multipart(fields: &[(&str, &str)], file: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    if let Some(bytes) = file {
        out.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"backup_file\"; filename=\"uji-asb.zip\"\r\nContent-Type: application/zip\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Asb', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-asb")
        .await
        .unwrap()
}

async fn remove_users(pool: &MySqlPool) {
    for email in [ADMIN, PLAIN] {
        sqlx::query("DELETE FROM personal_access_tokens WHERE name = 'uji-asb' AND tokenable_type = 'App\\\\Models\\\\User' AND tokenable_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
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
    }
}

/// Tunggu job sampai status final. Mengembalikan objek `data` terakhir.
async fn wait_job(pool: &MySqlPool, token: &str, job_id: &str) -> Value {
    for _ in 0..240 {
        let (status, body) = get(
            pool,
            &format!("/api/app-settings/backups/jobs/{job_id}"),
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let state = body["data"]["status"].as_str().unwrap_or("").to_string();
        if matches!(state.as_str(), "completed" | "failed" | "cancelled") {
            return body["data"].clone();
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("job {job_id} tidak selesai dalam batas waktu");
}

/// Stub S3: melayani `responses` berurutan (satu koneksi per permintaan) dan mencatat header permintaan.
async fn s3_stub(responses: Vec<(u16, &'static str)>) -> (String, Arc<StdMutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_task = seen.clone();
    tokio::spawn(async move {
        for (status, body) in responses {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = sock.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            seen_task
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf).into_owned());
            let reason = if status == 200 { "OK" } else { "Forbidden" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            let _ = sock.shutdown().await;
        }
    });
    (format!("http://{addr}"), seen)
}

/// ZIP berisi `database.sql` (dan media opsional) untuk tes restore.
fn make_zip(path: &PathBuf, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let data = zip.finish().unwrap().into_inner();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

/// Dump kecil: satu tabel dengan dua baris (satu NULL) dan satu pernyataan terpisah dengan penanda.
fn small_dump(rows: &str) -> String {
    format!(
        "-- Arumanis backup\n-- Database: apiamis\nSET FOREIGN_KEY_CHECKS=0;\n{MARKER}\
DROP TABLE IF EXISTS `uji_asb_items`;\n{MARKER}\
CREATE TABLE `uji_asb_items` (`id` int NOT NULL, `nama` varchar(50) NULL, PRIMARY KEY (`id`));\n{MARKER}\
INSERT INTO `uji_asb_items` (`id`, `nama`) VALUES\n{rows};\n{MARKER}\
SET FOREIGN_KEY_CHECKS=1;\n"
    )
}

async fn count(pool: &MySqlPool, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn backup_create_poll_and_cancel() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    // Hanya admin.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&plain),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Validasi.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "include_media": "mungkin" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": 5 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Job lokal tanpa media: 202, lalu selesai dengan database.sql di dalam ZIP.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-asb-lokal", "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["message"], "Backup sedang diproses di server");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let done = wait_job(&pool, &admin, &job_id).await;
    assert_eq!(done["status"], "completed", "{done}");
    let filename = done["result"]["filename"].as_str().unwrap().to_string();
    assert!(
        filename.starts_with("arumanis_")
            && filename.contains("uji-asb-lokal")
            && filename.ends_with(".zip")
    );
    assert_eq!(done["result"]["include_media"], false);

    let zip_path = base.join("private").join("system-backups").join(&filename);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
    let mut sql = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("database.sql").unwrap(), &mut sql).unwrap();
    assert!(
        sql.contains("CREATE TABLE `users`"),
        "dump memuat tabel users"
    );
    assert!(sql.contains(MARKER));
    let comment = String::from_utf8_lossy(archive.comment()).into_owned();
    assert!(
        comment.contains("\"include_media\":false"),
        "komentar arsip: {comment}"
    );

    // Daftar lokal memuat berkas ini.
    let (status, body) = get(&pool, "/api/app-settings/backups", &admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["filename"] == filename.as_str()));

    // Batalkan: job yang masih berjalan (ditulis manual) menerima tanda pembatalan.
    let running_id = "uji-asb-job-berjalan";
    let jobs_dir = base.join("private").join("system-backups").join("jobs");
    std::fs::create_dir_all(&jobs_dir).unwrap();
    std::fs::write(
        jobs_dir.join(format!("{running_id}.json")),
        json!({ "job_id": running_id, "status": "running", "filename": "uji-asb-none.zip", "progress": 40 }).to_string(),
    )
    .unwrap();

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/app-settings/backups/jobs/{running_id}"),
        Some(&plain),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/app-settings/backups/jobs/{running_id}"),
        Some(&admin),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Permintaan pembatalan backup dikirim");
    assert_eq!(body["data"]["cancel_requested"], true);
    let (_, body) = get(
        &pool,
        &format!("/api/app-settings/backups/jobs/{running_id}"),
        &admin,
    )
    .await;
    assert_eq!(body["data"]["cancel_requested"], true);
    assert_eq!(body["data"]["message"], "Membatalkan backup…");

    // Job yang sudah selesai tidak berubah.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/app-settings/backups/jobs/{job_id}"),
        Some(&admin),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["status"], "completed");

    // Job yang tidak ada dan ID tidak valid.
    let (status, _) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/jobs/uji-asb-tidak-ada",
        Some(&admin),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/jobs/bad_id",
        Some(&admin),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let _ = std::fs::remove_file(jobs_dir.join(format!("{running_id}.json")));
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn s3_test_uses_signed_list_request_against_stub() {
    let _guard = LOCK.lock().await;
    setup_env();
    let pool = pool().await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    let (endpoint, seen) = s3_stub(vec![
        (200, "<ListBucketResult></ListBucketResult>"),
        (
            403,
            "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
        ),
    ])
    .await;
    let creds = json!({
        "s3_endpoint": endpoint,
        "s3_region": "us-east-1",
        "s3_bucket": "uji-asb-bucket",
        "s3_access_key_id": "uji-asb-key",
        "s3_secret_access_key": "uji-asb-secret",
    });

    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups/s3/test",
        Some(&plain),
        creds.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups/s3/test",
        Some(&admin),
        creds.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "ok": true, "used_stored_key": false }));

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups/s3/test",
        Some(&admin),
        creds,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["ok"], false);
    assert_eq!(
        body["error"],
        "Koneksi S3 gagal: HTTP 403: AccessDenied: Access Denied"
    );

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let first = &requests[0];
    assert!(
        first.starts_with("GET /uji-asb-bucket/?delimiter=%2F&list-type=2&prefix= HTTP/1.1"),
        "baris permintaan: {first}"
    );
    let lower = first.to_ascii_lowercase();
    assert!(lower.contains("authorization: aws4-hmac-sha256 credential=uji-asb-key/"));
    assert!(lower.contains("x-amz-date: "));
    assert!(lower.contains(
        "x-amz-content-sha256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    ));
    assert!(
        !first.contains("uji-asb-secret"),
        "rahasia tidak boleh ikut dikirim"
    );

    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn restore_requests_validation_and_admin_only_on_apiamis() {
    let _guard = LOCK.lock().await;
    setup_env();
    let pool = pool().await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;
    let uri = "/api/app-settings/backups/restore";

    // Dibaca sebagai non-admin: 403 sebelum body diproses.
    let (status, _, _) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&plain),
        multipart(&[("backup_name", "uji-asb-x.zip")], None),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Tanpa berkas dan tanpa nama.
    let (status, _, bytes) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[], None),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["message"],
        "Pilih file backup atau nama backup yang tersimpan"
    );

    // Berkas bukan ZIP: ditolak sebelum database disentuh.
    let (status, _, _) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[], Some(b"bukan zip sama sekali")),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nama tidak valid.
    let (status, _, _) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[("backup_name", "../rahasia.zip")], None),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Nama yang tidak ada: 404, atau 501 bila S3 backup aktif (belum dipindah).
    let s3_on: Option<String> = sqlx::query_scalar("SELECT CAST(`value` AS CHAR) FROM app_settings WHERE `key` = 's3_backup_enabled' ORDER BY id LIMIT 1")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let expected = if s3_on.as_deref() == Some("1") {
        StatusCode::NOT_IMPLEMENTED
    } else {
        StatusCode::NOT_FOUND
    };
    let (status, _, _) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[("backup_name", "uji-asb-tidak-ada.zip")], None),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, expected);

    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn restore_into_isolated_database_only() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let apiamis = pool().await;
    let uji_url = uji_url();
    let admin = user_token(&apiamis, ADMIN, true).await;

    // Database uji dibuat ulang setiap tes dan dihapus di akhir.
    sqlx::query(&format!("DROP DATABASE IF EXISTS `{UJI_DB}`"))
        .execute(&apiamis)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE `{UJI_DB}` CHARACTER SET utf8mb4"))
        .execute(&apiamis)
        .await
        .unwrap();
    let uji = MySqlPool::connect(&uji_url)
        .await
        .expect("koneksi ke database uji");

    let apiamis_tables_before = count(
        &apiamis,
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'apiamis' AND table_type = 'BASE TABLE'",
    )
    .await;
    let apiamis_users_before = count(&apiamis, "SELECT COUNT(*) FROM users").await;

    // 1. Restore dari dump kecil, termasuk satu berkas media.
    let small = base.join("zip").join("uji-asb-kecil.zip");
    make_zip(
        &small,
        &[
            (
                "database.sql",
                small_dump("(1, 'satu'),\n(2, NULL)").as_bytes(),
            ),
            ("media/public/987654/uji.txt", b"uji-asb media"),
        ],
    );
    let result = restore_from_zip(&uji, &small)
        .await
        .expect("restore dump kecil");
    assert_eq!(result["source"], "uji-asb-kecil.zip");
    assert_eq!(count(&uji, "SELECT COUNT(*) FROM uji_asb_items").await, 2);
    let nama: Option<String> = sqlx::query_scalar("SELECT nama FROM uji_asb_items WHERE id = 2")
        .fetch_one(&uji)
        .await
        .unwrap();
    assert!(nama.is_none());
    let media_file = base.join("public").join("987654").join("uji.txt");
    assert_eq!(std::fs::read(&media_file).unwrap(), b"uji-asb media");

    // 2. Media dengan disk tidak didukung ditolak sebelum database diubah.
    let bad_disk = base.join("zip").join("uji-asb-disk.zip");
    make_zip(
        &bad_disk,
        &[
            ("database.sql", small_dump("(9, 'hilang')").as_bytes()),
            ("media/s3/1/x.txt", b"x"),
        ],
    );
    assert!(restore_from_zip(&uji, &bad_disk).await.is_err());
    assert_eq!(
        count(&uji, "SELECT COUNT(*) FROM uji_asb_items WHERE id = 9").await,
        0
    );
    assert_eq!(
        count(&uji, "SELECT COUNT(*) FROM uji_asb_items").await,
        2,
        "data tetap"
    );

    // 3. Path media yang keluar dari folder ditolak.
    let traversal = base.join("zip").join("uji-asb-traversal.zip");
    make_zip(
        &traversal,
        &[
            ("database.sql", small_dump("(8, 'x')").as_bytes()),
            ("media/public/../../bocor.txt", b"x"),
        ],
    );
    assert!(restore_from_zip(&uji, &traversal).await.is_err());
    assert!(!base.join("bocor.txt").exists());
    assert_eq!(
        count(&uji, "SELECT COUNT(*) FROM uji_asb_items WHERE id = 8").await,
        0
    );

    // 4. ZIP tanpa database.sql ditolak.
    let no_sql = base.join("zip").join("uji-asb-tanpa-sql.zip");
    make_zip(&no_sql, &[("media/public/1/a.txt", b"x")]);
    assert!(restore_from_zip(&uji, &no_sql).await.is_err());
    assert_eq!(count(&uji, "SELECT COUNT(*) FROM uji_asb_items").await, 2);

    // 5. Round trip: dump database apiamis (hanya dibaca) lalu restore ke database uji.
    let (status, body) = send(
        &apiamis,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-asb-roundtrip", "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let done = wait_job(&apiamis, &admin, &job_id).await;
    assert_eq!(done["status"], "completed", "{done}");
    let filename = done["result"]["filename"].as_str().unwrap().to_string();
    let roundtrip = base.join("private").join("system-backups").join(&filename);

    // Tabel uji dari langkah di atas dihapus dulu agar jumlah tabel dibanding apiamis.
    sqlx::query("DROP TABLE IF EXISTS uji_asb_items")
        .execute(&uji)
        .await
        .unwrap();
    restore_from_zip(&uji, &roundtrip)
        .await
        .expect("restore round trip");
    let uji_tables = count(
        &uji,
        &format!("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = '{UJI_DB}' AND table_type = 'BASE TABLE'"),
    )
    .await;
    assert_eq!(
        uji_tables, apiamis_tables_before,
        "jumlah tabel sama dengan apiamis"
    );
    assert_eq!(
        count(&uji, "SELECT COUNT(*) FROM users").await,
        apiamis_users_before,
        "jumlah users sama dengan apiamis"
    );

    // Database apiamis tidak berubah oleh restore.
    assert_eq!(
        count(&apiamis, "SELECT COUNT(*) FROM users").await,
        apiamis_users_before
    );
    assert_eq!(
        count(
            &apiamis,
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'apiamis' AND table_type = 'BASE TABLE'"
        )
        .await,
        apiamis_tables_before
    );

    // Bersih-bersih: database uji, berkas uji, dan baris pengguna uji.
    uji.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS `{UJI_DB}`"))
        .execute(&apiamis)
        .await
        .unwrap();
    let _ = std::fs::remove_file(&roundtrip);
    let _ = std::fs::remove_file(&media_file);
    remove_users(&apiamis).await;
}
