//! Backup S3 lewat router dan fungsi S3 terhadap MySQL. Semua panggilan S3 diarahkan ke stub HTTP
//! lokal (`127.0.0.1`) lewat `s3_endpoint`. Tes ini tidak pernah menyentuh S3 sungguhan.
//!
//! - `store` dengan `s3_direct`: PutObject, multipart, kegagalan, dan konfigurasi kosong.
//! - Unggah multipart langsung (`s3_upload_file`) dengan ambang kecil, termasuk pembatalan.
//! - Restore dari S3: unduh (`s3_download`) lalu restore HANYA ke `apiamis_uji_backup`. Jalur router
//!   yang sampai ke restore tidak diuji karena restore memakai database aktif (`apiamis`).
//! - Hapus berkas S3 (`DELETE`) dan akses admin.
//!
//! Baris yang ditulis memakai awalan `uji-s3b-`. Baris `app_settings` yang disentuh dikembalikan.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -j 2 -p api --test app_settings_backup_s3_db -- --include-ignored
//! ```

use std::{
    io::{Cursor, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use api::{
    app,
    app_settings_backup::{object_key, restore_from_zip, s3_download, s3_upload_file, S3Cfg},
    AppState,
};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shared::Config;
use sqlx::MySqlPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tower::ServiceExt;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const ADMIN: &str = "uji-s3b-admin@example.test";
const PLAIN: &str = "uji-s3b-plain@example.test";
const UJI_DB: &str = "apiamis_uji_backup";
const BOUNDARY: &str = "ujiS3bBoundary";
const MULTIPART_CT: &str = "multipart/form-data; boundary=ujiS3bBoundary";
const MARKER: &str = "/*__ARUMANIS_STMT__*/\n";
const BUCKET: &str = "uji-s3b-bucket";
const SETTING_KEYS: [&str; 6] = [
    "s3_backup_enabled",
    "s3_endpoint",
    "s3_region",
    "s3_bucket",
    "s3_access_key_id",
    "s3_secret_access_key",
];

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

/// `PRIVATE_STORAGE_PATH` dan `PUBLIC_STORAGE_PATH` diarahkan ke direktori sementara tes.
fn setup_env() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uji-app-settings-backup-s3");
    std::env::set_var("PUBLIC_STORAGE_PATH", base.join("public"));
    std::env::set_var("PRIVATE_STORAGE_PATH", base.join("private"));
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    base
}

fn backup_dir(base: &Path) -> PathBuf {
    base.join("private").join("system-backups")
}

fn database_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL harus diisi (lihat docs tes)")
}

/// URL database `apiamis_uji_backup`: sama dengan `DATABASE_URL`, nama database diganti.
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
    let raw = if body.is_null() {
        vec![]
    } else {
        serde_json::to_vec(&body).unwrap()
    };
    let ct = (!body.is_null()).then_some("application/json");
    let (status, _, bytes) = send_raw(pool, method, uri, token, raw, ct).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn multipart(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji S3b', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-s3b")
        .await
        .unwrap()
}

async fn remove_users(pool: &MySqlPool) {
    for email in [ADMIN, PLAIN] {
        sqlx::query("DELETE FROM personal_access_tokens WHERE name = 'uji-s3b' AND tokenable_type = 'App\\\\Models\\\\User' AND tokenable_id IN (SELECT id FROM users WHERE email = ?)")
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
        let (status, body) = send(
            pool,
            Method::GET,
            &format!("/api/app-settings/backups/jobs/{job_id}"),
            Some(token),
            Value::Null,
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

async fn count(pool: &MySqlPool, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
// Pengaturan S3 di `app_settings` (disimpan dan dikembalikan oleh tes)
// ---------------------------------------------------------------------------

/// Nilai awal tiap kunci: `None` bila baris tidak ada, `Some(None)` bila ada dengan nilai NULL.
async fn snapshot(pool: &MySqlPool) -> Vec<(&'static str, Option<Option<String>>)> {
    let mut out = Vec::new();
    for key in SETTING_KEYS {
        let row: Option<Option<String>> = sqlx::query_scalar(
            "SELECT CAST(`value` AS CHAR) FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap();
        out.push((key, row));
    }
    out
}

async fn write_setting(pool: &MySqlPool, key: &str, value: Option<String>) {
    if count(
        pool,
        &format!("SELECT COUNT(*) FROM app_settings WHERE `key` = '{key}'"),
    )
    .await
        > 0
    {
        sqlx::query("UPDATE app_settings SET `value` = ?, updated_at = NOW() WHERE `key` = ?")
            .bind(value)
            .bind(key)
            .execute(pool)
            .await
            .unwrap();
    } else {
        sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, 'text', NOW(), NOW())")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn remove_setting(pool: &MySqlPool, key: &str) {
    sqlx::query("DELETE FROM app_settings WHERE `key` = ?")
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
}

async fn restore_settings(pool: &MySqlPool, snap: &[(&'static str, Option<Option<String>>)]) {
    for (key, row) in snap {
        match row {
            None => remove_setting(pool, key).await,
            Some(value) => write_setting(pool, key, value.clone()).await,
        }
    }
}

/// S3 aktif dengan konfigurasi lengkap yang menunjuk ke stub.
async fn configure_s3(pool: &MySqlPool, endpoint: &str) {
    write_setting(pool, "s3_backup_enabled", Some("1".into())).await;
    write_setting(pool, "s3_endpoint", Some(endpoint.into())).await;
    write_setting(pool, "s3_region", Some("us-east-1".into())).await;
    write_setting(pool, "s3_bucket", Some(BUCKET.into())).await;
    write_setting(pool, "s3_access_key_id", Some("uji-s3b-key".into())).await;
    write_setting(pool, "s3_secret_access_key", Some("uji-s3b-secret".into())).await;
}

fn test_cfg(endpoint: &str) -> S3Cfg {
    S3Cfg {
        endpoint: Some(endpoint.to_string()),
        region: "us-east-1".into(),
        bucket: BUCKET.into(),
        key: "uji-s3b-key".into(),
        secret: "uji-s3b-secret".into(),
    }
}

// ---------------------------------------------------------------------------
// Stub S3: mencatat baris permintaan, header, dan badan; membalas dari fungsi
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Recorded {
    line: String,
    head: String,
    body: Vec<u8>,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn new(status: u16) -> Self {
        Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    fn body(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.body = bytes.into();
        self
    }
}

async fn read_request(sock: &mut TcpStream) -> Recorded {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = sock.read(&mut chunk).await.unwrap();
        assert!(n > 0, "koneksi ditutup sebelum header lengkap");
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buf.len() < body_start + length {
        let n = sock.read(&mut chunk).await.unwrap();
        assert!(n > 0, "badan permintaan terpotong");
        buf.extend_from_slice(&chunk[..n]);
    }
    Recorded {
        line: head.lines().next().unwrap_or("").to_string(),
        head,
        body: buf[body_start..body_start + length].to_vec(),
    }
}

async fn write_reply(sock: &mut TcpStream, reply: Reply) {
    let mut out = format!("HTTP/1.1 {} OK\r\n", reply.status);
    let has_length = reply
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    for (k, v) in &reply.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_length {
        out.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    out.push_str("Connection: close\r\n\r\n");
    sock.write_all(out.as_bytes()).await.unwrap();
    sock.write_all(&reply.body).await.unwrap();
    let _ = sock.shutdown().await;
}

/// Stub yang melayani tepat `expect` koneksi. `handler` menerima permintaan saat ini dan riwayat
/// permintaan sebelumnya. Mengembalikan endpoint dan catatan semua permintaan.
async fn stub<F>(expect: usize, handler: F) -> (String, Arc<StdMutex<Vec<Recorded>>>)
where
    F: Fn(&Recorded, &[Recorded]) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<StdMutex<Vec<Recorded>>> = Arc::new(StdMutex::new(Vec::new()));
    let seen_task = seen.clone();
    tokio::spawn(async move {
        for _ in 0..expect {
            let (mut sock, _) = listener.accept().await.unwrap();
            let rec = read_request(&mut sock).await;
            let history = seen_task.lock().unwrap().clone();
            let reply = handler(&rec, &history);
            seen_task.lock().unwrap().push(rec);
            write_reply(&mut sock, reply).await;
        }
    });
    (format!("http://{addr}"), seen)
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `Authorization` SigV4 yang valid bentuknya dengan kredensial stub.
fn assert_signed(rec: &Recorded, signed_headers: &str) {
    let auth = rec.header("authorization").expect("header authorization");
    assert!(
        auth.starts_with("AWS4-HMAC-SHA256 Credential=uji-s3b-key/"),
        "kredensial: {auth}"
    );
    assert!(
        auth.contains(&format!("SignedHeaders={signed_headers},"))
            || auth.contains(&format!("SignedHeaders={signed_headers} ")),
        "signed headers: {auth}"
    );
    let sig = auth.rsplit("Signature=").next().unwrap_or("");
    assert_eq!(sig.len(), 64, "signature harus 64 hex: {auth}");
    assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(rec.header("x-amz-date").is_some(), "x-amz-date");
    assert!(
        !rec.head.contains("uji-s3b-secret"),
        "rahasia tidak boleh ikut dikirim"
    );
}

fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
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

/// Dump kecil untuk tabel `uji_s3b_items` (dua baris, satu NULL).
fn small_dump() -> String {
    format!(
        "-- Arumanis backup\n-- Database: apiamis\nSET FOREIGN_KEY_CHECKS=0;\n{MARKER}\
DROP TABLE IF EXISTS `uji_s3b_items`;\n{MARKER}\
CREATE TABLE `uji_s3b_items` (`id` int NOT NULL, `nama` varchar(50) NULL, PRIMARY KEY (`id`));\n{MARKER}\
INSERT INTO `uji_s3b_items` (`id`, `nama`) VALUES\n(1, 'satu'),\n(2, NULL);\n{MARKER}\
SET FOREIGN_KEY_CHECKS=1;\n"
    )
}

// ---------------------------------------------------------------------------
// Tes
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_s3_direct_puts_one_object_without_local_copy() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    let (endpoint, seen) = stub(2, |rec, history| {
        if rec.line.starts_with("PUT ") {
            Reply::new(200).with("ETag", "\"uji-s3b-etag\"")
        } else {
            let size = history
                .iter()
                .rev()
                .find(|r| r.line.starts_with("PUT "))
                .map_or(0, |r| r.body.len());
            Reply::new(200).with("Content-Length", size.to_string())
        }
    })
    .await;
    configure_s3(&pool, &endpoint).await;

    // Pengguna biasa ditolak sebelum menyentuh S3.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&plain),
        json!({ "s3_direct": true }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-s3b-put", "s3_direct": true, "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["message"], "Backup sedang diproses langsung ke S3");
    assert_eq!(body["data"]["s3_direct"], true);
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let filename = body["data"]["filename"].as_str().unwrap().to_string();
    assert!(filename.ends_with("_uji-s3b-put.zip"), "{filename}");

    let job = wait_job(&pool, &admin, &job_id).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(job["result"]["storage"], "s3");
    assert_eq!(job["result"]["filename"], filename.as_str());
    assert_eq!(job["result"]["include_media"], false);

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2, "PutObject lalu HeadObject");
    let put = &requests[0];
    assert_eq!(
        put.line,
        format!("PUT /{BUCKET}/system-backups/{filename} HTTP/1.1")
    );
    assert_signed(put, "host;x-amz-acl;x-amz-content-sha256;x-amz-date");
    assert_eq!(put.header("x-amz-acl").as_deref(), Some("private"));
    assert_eq!(
        put.header("x-amz-content-sha256").as_deref(),
        Some(hex_sha256(&put.body).as_str()),
        "hash payload harus sama dengan badan"
    );
    assert!(put.body.starts_with(b"PK\x03\x04"), "badan harus ZIP");
    assert!(
        put.body.windows(12).any(|w| w == b"database.sql"),
        "ZIP memuat database.sql"
    );
    assert_eq!(
        job["result"]["size"].as_u64(),
        Some(put.body.len() as u64),
        "ukuran dari HeadObject"
    );

    let head = &requests[1];
    assert_eq!(
        head.line,
        format!("HEAD /{BUCKET}/system-backups/{filename} HTTP/1.1")
    );
    assert_signed(head, "host;x-amz-content-sha256;x-amz-date");

    // Tidak ada salinan lokal di `system-backups`.
    assert!(!backup_dir(&base).join(&filename).exists());

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_s3_failure_marks_job_failed_with_s3_prefix() {
    let _guard = LOCK.lock().await;
    setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;

    let (endpoint, seen) = stub(1, |_, _| {
        Reply::new(403)
            .body("<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>")
    })
    .await;
    configure_s3(&pool, &endpoint).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-s3b-gagal", "s3_direct": true, "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let job = wait_job(&pool, &admin, &job_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(job["message"], "Backup gagal dibuat");
    assert_eq!(
        job["error"],
        "Backup ke S3 gagal: HTTP 403: AccessDenied: Access Denied"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_s3_without_config_fails_job_like_laravel() {
    let _guard = LOCK.lock().await;
    setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;

    // S3 aktif, tetapi konfigurasi kosong: tidak ada permintaan jaringan sama sekali.
    write_setting(&pool, "s3_backup_enabled", Some("1".into())).await;
    for key in [
        "s3_endpoint",
        "s3_region",
        "s3_bucket",
        "s3_access_key_id",
        "s3_secret_access_key",
    ] {
        remove_setting(&pool, key).await;
    }

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-s3b-kosong", "s3_direct": true, "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let job = wait_job(&pool, &admin, &job_id).await;
    assert_eq!(job["status"], "failed", "{job}");
    assert_eq!(
        job["error"],
        "Backup ke S3 gagal: Pengaturan AWS S3 belum lengkap atau belum dikonfigurasi."
    );

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_s3_direct_with_s3_disabled_stays_local() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;

    write_setting(&pool, "s3_backup_enabled", Some("0".into())).await;
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/backups",
        Some(&admin),
        json!({ "label": "uji-s3b-lokal", "s3_direct": true, "include_media": false }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Pesan mengikuti flag yang diminta (`s3_direct`), seperti controller Laravel.
    assert_eq!(body["message"], "Backup sedang diproses langsung ke S3");
    let job_id = body["data"]["job_id"].as_str().unwrap().to_string();
    let filename = body["data"]["filename"].as_str().unwrap().to_string();
    let job = wait_job(&pool, &admin, &job_id).await;
    assert_eq!(job["status"], "completed", "{job}");
    assert!(job["result"].get("storage").is_none());
    assert!(backup_dir(&base).join(&filename).is_file());
    let _ = std::fs::remove_file(backup_dir(&base).join(&filename));

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn multipart_upload_sends_parts_and_completes() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    std::fs::create_dir_all(&base).unwrap();
    // 600 KiB dengan ambang 64 KiB dan minimum part 256 KiB: tiga part (256, 256, 88 KiB).
    let path = base.join("uji-s3b-multipart.bin");
    let data: Vec<u8> = (0..600 * 1024).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &data).unwrap();

    let (endpoint, seen) = stub(5, |rec, _| {
        if rec.line.starts_with("POST ") && rec.line.contains("uploads=") {
            Reply::new(200).body(
                "<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>k</Key><UploadId>uji-s3b-upload-1</UploadId></InitiateMultipartUploadResult>",
            )
        } else if rec.line.starts_with("PUT ") {
            let n = rec.line.split("partNumber=").nth(1).unwrap_or("0");
            let n = n.split('&').next().unwrap_or("0");
            Reply::new(200).with("ETag", format!("\"etag-{n}\""))
        } else {
            Reply::new(200).body(
                "<CompleteMultipartUploadResult><Key>k</Key></CompleteMultipartUploadResult>",
            )
        }
    })
    .await;
    let cfg = test_cfg(&endpoint);
    let key = object_key("uji-s3b-multipart.zip");
    s3_upload_file(&cfg, &key, &path, 64 * 1024, 256 * 1024)
        .await
        .expect("unggah multipart");

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 5, "{requests:?}");
    assert_eq!(
        requests[0].line,
        format!("POST /{BUCKET}/system-backups/uji-s3b-multipart.zip?uploads= HTTP/1.1")
    );
    assert_signed(
        &requests[0],
        "host;x-amz-acl;x-amz-content-sha256;x-amz-date",
    );
    let sizes: Vec<usize> = requests[1..4].iter().map(|r| r.body.len()).collect();
    assert_eq!(sizes, vec![256 * 1024, 256 * 1024, 90112]);
    for (i, req) in requests[1..4].iter().enumerate() {
        assert_eq!(
            req.line,
            format!(
                "PUT /{BUCKET}/system-backups/uji-s3b-multipart.zip?partNumber={}&uploadId=uji-s3b-upload-1 HTTP/1.1",
                i + 1
            )
        );
        assert_signed(req, "host;x-amz-content-sha256;x-amz-date");
        assert_eq!(
            req.header("x-amz-content-sha256").as_deref(),
            Some(hex_sha256(&req.body).as_str())
        );
    }
    let complete = &requests[4];
    assert!(complete.line.starts_with(
        "POST /uji-s3b-bucket/system-backups/uji-s3b-multipart.zip?uploadId=uji-s3b-upload-1 "
    ));
    let xml = String::from_utf8_lossy(&complete.body).into_owned();
    assert!(xml.starts_with("<CompleteMultipartUpload>"), "{xml}");
    for n in 1..=3 {
        assert!(
            xml.contains(&format!(
                "<Part><PartNumber>{n}</PartNumber><ETag>\"etag-{n}\"</ETag></Part>"
            )),
            "{xml}"
        );
    }
    // Isi part sama dengan berkas: part pertama dan terakhir dicek byte per byte.
    assert_eq!(requests[1].body, data[..256 * 1024]);
    assert_eq!(requests[3].body, data[512 * 1024..]);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn multipart_failure_aborts_the_upload() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("uji-s3b-gagal-part.bin");
    std::fs::write(&path, vec![7u8; 300 * 1024]).unwrap();

    let (endpoint, seen) = stub(3, |rec, _| {
        if rec.line.contains("uploads=") {
            Reply::new(200).body("<InitiateMultipartUploadResult><UploadId>uji-s3b-upload-2</UploadId></InitiateMultipartUploadResult>")
        } else if rec.line.starts_with("PUT ") {
            Reply::new(500).body("<Error><Code>InternalError</Code><Message>We encountered an internal error</Message></Error>")
        } else {
            Reply::new(204)
        }
    })
    .await;
    let cfg = test_cfg(&endpoint);
    let err = s3_upload_file(
        &cfg,
        &object_key("uji-s3b-gagal-part.zip"),
        &path,
        64 * 1024,
        256 * 1024,
    )
    .await
    .expect_err("unggah harus gagal");
    assert_eq!(
        err,
        "HTTP 500: InternalError: We encountered an internal error"
    );

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].line.split(' ').next(), Some("DELETE"));
    assert!(requests[2].line.contains("?uploadId=uji-s3b-upload-2"));
    assert_signed(&requests[2], "host;x-amz-content-sha256;x-amz-date");

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn restore_downloads_from_s3_then_restores_into_isolated_database() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let apiamis = pool().await;
    let uji = {
        sqlx::query(&format!("DROP DATABASE IF EXISTS `{UJI_DB}`"))
            .execute(&apiamis)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE DATABASE `{UJI_DB}` CHARACTER SET utf8mb4"))
            .execute(&apiamis)
            .await
            .unwrap();
        MySqlPool::connect(&uji_url())
            .await
            .expect("koneksi ke database uji")
    };

    let zip_bytes = {
        let zip_path = base.join("zip").join("uji-s3b-restore.zip");
        make_zip(&zip_path, &[("database.sql", small_dump().as_bytes())]);
        std::fs::read(&zip_path).unwrap()
    };
    let served = zip_bytes.clone();
    let (endpoint, seen) = stub(1, move |_, _| Reply::new(200).body(served.clone())).await;
    let cfg = test_cfg(&endpoint);

    let apiamis_tables_before = count(
        &apiamis,
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'apiamis' AND table_type = 'BASE TABLE'",
    )
    .await;
    let apiamis_users_before = count(&apiamis, "SELECT COUNT(*) FROM users").await;

    let dest = backup_dir(&base).join("uji-s3b-restore.zip");
    std::fs::create_dir_all(backup_dir(&base)).unwrap();
    s3_download(&cfg, &object_key("uji-s3b-restore.zip"), &dest)
        .await
        .expect("unduh dari stub");
    assert_eq!(std::fs::read(&dest).unwrap(), zip_bytes);

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].line,
        format!("GET /{BUCKET}/system-backups/uji-s3b-restore.zip HTTP/1.1")
    );
    assert_signed(&requests[0], "host;x-amz-content-sha256;x-amz-date");

    // Restore hanya ke database uji, bukan `apiamis`.
    restore_from_zip(&uji, &dest)
        .await
        .expect("restore ke database uji");
    let rows = count(&uji, "SELECT COUNT(*) FROM uji_s3b_items").await;
    assert_eq!(rows, 2);
    let _ = std::fs::remove_file(&dest);

    assert_eq!(
        count(
            &apiamis,
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'apiamis' AND table_type = 'BASE TABLE'",
        )
        .await,
        apiamis_tables_before
    );
    assert_eq!(
        count(&apiamis, "SELECT COUNT(*) FROM users").await,
        apiamis_users_before
    );

    uji.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS `{UJI_DB}`"))
        .execute(&apiamis)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn restore_route_s3_missing_and_download_failure_leave_database_alone() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    // HEAD 404 untuk `tidak-ada`; HEAD 200 lalu GET 500 untuk `gagal`.
    let (endpoint, seen) = stub(3, |rec, _| {
        let missing = rec.line.contains("uji-s3b-tidak-ada.zip");
        if rec.line.starts_with("HEAD ") && missing {
            Reply::new(404)
        } else if rec.line.starts_with("HEAD ") {
            Reply::new(200).with("Content-Length", "10")
        } else {
            Reply::new(500).body("<Error><Code>InternalError</Code><Message>boom</Message></Error>")
        }
    })
    .await;
    configure_s3(&pool, &endpoint).await;
    let uri = "/api/app-settings/backups/restore";

    // Pengguna biasa: 403, tanpa menyentuh S3.
    let (status, _, _) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&plain),
        multipart(&[("backup_name", "uji-s3b-tidak-ada.zip")]),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, bytes) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[("backup_name", "uji-s3b-tidak-ada.zip")]),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["message"], "Backup tidak ditemukan");

    let (status, _, bytes) = send_raw(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        multipart(&[("backup_name", "uji-s3b-gagal.zip")]),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["message"], "Gagal mengunduh backup dari S3");
    assert!(
        !backup_dir(&base).join("uji-s3b-gagal.zip").exists(),
        "berkas setengah jadi tidak boleh tertinggal"
    );

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert!(requests[0]
        .line
        .starts_with("HEAD /uji-s3b-bucket/system-backups/uji-s3b-tidak-ada.zip"));
    assert!(requests[2]
        .line
        .starts_with("GET /uji-s3b-bucket/system-backups/uji-s3b-gagal.zip"));
    assert_signed(&requests[2], "host;x-amz-content-sha256;x-amz-date");

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn delete_s3_backup_removes_local_copy_and_object() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    // Hapus berhasil untuk `hapus`; untuk `gagal-hapus` S3 menjawab 500, tetapi respons tetap sukses.
    let (endpoint, seen) = stub(2, |rec, _| {
        if rec.line.contains("uji-s3b-gagal-hapus.zip") {
            Reply::new(500).body("<Error><Code>InternalError</Code><Message>boom</Message></Error>")
        } else {
            Reply::new(204)
        }
    })
    .await;
    configure_s3(&pool, &endpoint).await;

    let local = backup_dir(&base).join("uji-s3b-hapus.zip");
    make_zip(&local, &[("database.sql", b"-- uji")]);

    let (status, _) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-s3b-hapus.zip",
        Some(&plain),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(local.exists());

    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-s3b-hapus.zip",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Backup berhasil dihapus");
    assert!(!local.exists());

    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-s3b-gagal-hapus.zip",
        Some(&admin),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Backup berhasil dihapus");

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].line,
        format!("DELETE /{BUCKET}/system-backups/uji-s3b-hapus.zip HTTP/1.1")
    );
    assert_signed(&requests[0], "host;x-amz-content-sha256;x-amz-date");

    restore_settings(&pool, &snap).await;
    remove_users(&pool).await;
}
