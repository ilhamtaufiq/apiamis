//! Pengaturan aplikasi lewat router terhadap MySQL: `store` (teks, secret, berkas), template kontrak,
//! statistik penyimpanan, dan backup lokal.
//!
//! Tes berjalan berurutan (`LOCK`) karena membaca dan menulis baris `app_settings` yang sama. Setiap tes
//! mengembalikan baris setting dan media yang disentuh ke keadaan semula. Backup dan berkas memakai
//! direktori sementara di `CARGO_TARGET_TMPDIR`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test app_settings_db -- --include-ignored
//! ```

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tokio::sync::Mutex;
use tower::ServiceExt;

const ADMIN: &str = "uji-set-admin@example.test";
const PLAIN: &str = "uji-set-plain@example.test";
const BOUNDARY: &str = "ujiSettingsBoundary";
const APP_MODEL: &str = "App\\Models\\AppSetting";

/// Setting yang disentuh tes; nilainya dikembalikan setelah tes.
const TOUCHED: &[&str] = &[
    "app_name",
    "app_description",
    "mail_host",
    "mail_password",
    "mail_encryption",
    "mail_from_address",
    "kontrak_nama_ppk",
    "logo",
    "favicon",
    "login_cover",
    "kontrak_template_bap",
    "s3_backup_enabled",
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

/// Direktori sementara tes; `PUBLIC_STORAGE_PATH` dan `PRIVATE_STORAGE_PATH` diarahkan ke sini.
fn setup_env() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("uji-app-settings");
    std::env::set_var("PUBLIC_STORAGE_PATH", base.join("public"));
    std::env::set_var("PRIVATE_STORAGE_PATH", base.join("private"));
    base
}

fn kontrak_default(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../storage/app/templates")
        .join(name);
    std::fs::read(path).expect("template default ada di repo")
}

const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, b'u', b'j', b'i',
];
const JPG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, b'u', b'j', b'i'];
const DOCX: &[u8] = b"PK\x03\x04uji-docx";

/// Body multipart: field teks dan berkas `(name, filename, bytes)`.
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

const MULTIPART_CT: &str = "multipart/form-data; boundary=ujiSettingsBoundary";

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
    body: Vec<u8>,
    content_type: Option<&str>,
) -> (StatusCode, Value) {
    let (status, _, bytes) = send_raw(pool, method, uri, token, body, content_type).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Set', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-set")
        .await
        .unwrap()
}

/// Keadaan awal yang dikembalikan di akhir tes.
struct Snapshot {
    settings: BTreeMap<String, (Option<String>, String)>,
    media_ids: HashSet<u64>,
}

async fn snapshot(pool: &MySqlPool) -> Snapshot {
    let mut settings = BTreeMap::new();
    for key in TOUCHED {
        let row = sqlx::query(
            "SELECT `value`, `type` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap();
        if let Some(r) = row {
            settings.insert(
                (*key).to_string(),
                (r.try_get("value").unwrap(), r.try_get("type").unwrap()),
            );
        }
    }
    let media_ids: Vec<u64> =
        sqlx::query_scalar("SELECT CAST(id AS UNSIGNED) FROM media WHERE model_type = ?")
            .bind(APP_MODEL)
            .fetch_all(pool)
            .await
            .unwrap();
    Snapshot {
        settings,
        media_ids: media_ids.into_iter().collect(),
    }
}

async fn restore(pool: &MySqlPool, base: &PathBuf, before: &Snapshot) {
    let ids: Vec<u64> =
        sqlx::query_scalar("SELECT CAST(id AS UNSIGNED) FROM media WHERE model_type = ?")
            .bind(APP_MODEL)
            .fetch_all(pool)
            .await
            .unwrap();
    for id in ids.into_iter().filter(|id| !before.media_ids.contains(id)) {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(base.join("public").join(id.to_string()));
    }
    for key in TOUCHED {
        match before.settings.get(*key) {
            Some((value, kind)) => {
                sqlx::query("UPDATE app_settings SET `value` = ?, `type` = ? WHERE `key` = ?")
                    .bind(value)
                    .bind(kind)
                    .bind(key)
                    .execute(pool)
                    .await
                    .unwrap();
            }
            None => {
                sqlx::query("DELETE FROM app_settings WHERE `key` = ?")
                    .bind(key)
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }
    sqlx::query(
        "DELETE FROM tbl_audit_logs WHERE user_id IN (SELECT id FROM users WHERE email IN (?, ?))",
    )
    .bind(ADMIN)
    .bind(PLAIN)
    .execute(pool)
    .await
    .unwrap();
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn setting_value(pool: &MySqlPool, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT `value` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .unwrap()
    .flatten()
}

fn entry<'a>(data: &'a Value, key: &str) -> &'a Value {
    data["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("setting {key} tidak ada"))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_text_secret_and_validation() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let before = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;
    let uri = "/api/app-settings";

    // Non-admin dan tanpa token.
    let body = multipart(&[("app_name", "Uji")], &[]);
    let (st, _) = send(
        &pool,
        Method::POST,
        uri,
        Some(&plain),
        body.clone(),
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send(&pool, Method::POST, uri, None, body, Some(MULTIPART_CT)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Simpan teks, secret, dan field kosong (menjadi null).
    let body = multipart(
        &[
            ("app_name", "  Uji Aplikasi  "),
            ("app_description", "   "),
            ("mail_host", "smtp.uji.test"),
            ("mail_password", "rahasia-uji-123"),
            ("mail_encryption", "tls"),
        ],
        &[],
    );
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{data}");
    assert_eq!(entry(&data, "app_name")["value"], "Uji Aplikasi");
    assert_eq!(entry(&data, "app_description")["value"], Value::Null);
    assert_eq!(entry(&data, "mail_host")["value"], "smtp.uji.test");
    let pw = entry(&data, "mail_password");
    assert_eq!(pw["value"], Value::Null, "secret tidak dikirim ke klien");
    assert_eq!(pw["is_configured"], true);
    assert_eq!(
        setting_value(&pool, "mail_password").await.as_deref(),
        Some("rahasia-uji-123")
    );

    // Audit tidak memuat nilai secret.
    let audit: Vec<String> = sqlx::query_scalar(
        "SELECT CAST(COALESCE(new_values, '') AS CHAR) FROM tbl_audit_logs \
         WHERE auditable_type = ? AND user_id = (SELECT id FROM users WHERE email = ?)",
    )
    .bind(APP_MODEL)
    .bind(ADMIN)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!audit.is_empty(), "audit created/updated ditulis");
    assert!(
        audit.iter().all(|a| !a.contains("rahasia-uji-123")),
        "nilai secret tidak masuk audit"
    );

    // Setelah field kosong, nilai di database tetap sama bila tidak dikirim ulang.
    let body = multipart(&[("app_name", "Uji Aplikasi 2")], &[]);
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(entry(&data, "app_name")["value"], "Uji Aplikasi 2");
    assert_eq!(entry(&data, "mail_host")["value"], "smtp.uji.test");

    // Body JSON juga diterima (field teks saja).
    let json_body = json!({ "app_name": "Uji JSON", "app_description": null }).to_string();
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        json_body.into_bytes(),
        Some("application/json"),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(entry(&data, "app_name")["value"], "Uji JSON");

    // Validasi: semua error dikumpulkan dalam satu respons 422.
    let long = "x".repeat(256);
    let body = multipart(
        &[
            ("mail_encryption", "foo"),
            ("mail_from_address", "bukan-email"),
            ("kontrak_masa_pemeliharaan_hari", "0"),
            ("app_name", long.as_str()),
        ],
        &[],
    );
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(data["message"], "The given data was invalid.");
    assert_eq!(
        data["errors"]["mail_encryption"][0],
        "The selected mail encryption is invalid."
    );
    assert_eq!(
        data["errors"]["mail_from_address"][0],
        "The mail from address field must be a valid email address."
    );
    assert_eq!(
        data["errors"]["kontrak_masa_pemeliharaan_hari"][0],
        "The kontrak masa pemeliharaan hari field must be at least 1."
    );
    assert_eq!(
        data["errors"]["app_name"][0],
        "The app name field must not be greater than 255 characters."
    );
    // Tidak ada yang tertulis saat validasi gagal.
    assert_eq!(
        setting_value(&pool, "app_name").await.as_deref(),
        Some("Uji JSON")
    );

    restore(&pool, &base, &before).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_files_replace_remove_and_kontrak_download() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let before = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let uri = "/api/app-settings";

    // Logo: unggah pertama, lalu diganti. Berkas lama hilang dari disk dan tabel media.
    let body = multipart(&[], &[("logo", "Logo Uji.PNG", PNG)]);
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{data}");
    let logo = entry(&data, "logo");
    assert_eq!(logo["type"], "file");
    assert_eq!(logo["is_configured"], true);
    let url = logo["value"].as_str().unwrap().to_string();
    assert!(url.starts_with("http://localhost/storage/"), "{url}");
    assert!(
        url.contains("/logo_") && url.ends_with(".PNG"),
        "nama berkas: {url}"
    );
    let first_media: u64 = sqlx::query_scalar(
        "SELECT CAST(m.id AS UNSIGNED) FROM media m JOIN app_settings s ON s.id = m.model_id \
         WHERE s.`key` = 'logo' AND m.model_type = ? AND m.collection_name = 'app-settings'",
    )
    .bind(APP_MODEL)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(base.join("public").join(first_media.to_string()).is_dir());

    let body = multipart(&[], &[("logo", "logo2.jpg", JPG)]);
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(entry(&data, "logo")["value"]
        .as_str()
        .unwrap()
        .ends_with(".jpg"));
    let media_count: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media m JOIN app_settings s ON s.id = m.model_id \
         WHERE s.`key` = 'logo' AND m.model_type = ? AND m.collection_name = 'app-settings'",
    )
    .bind(APP_MODEL)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(media_count, 1, "koleksi single-file");
    assert!(
        !base.join("public").join(first_media.to_string()).exists(),
        "berkas lama dihapus"
    );

    // Berkas dengan tipe tidak sesuai: ekstensi GIF untuk favicon ditolak.
    let body = multipart(&[], &[("favicon", "ikon.gif", PNG)]);
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        data["errors"]["favicon"][0],
        "The favicon field must be a file of type: jpg, jpeg, png, svg, ico."
    );

    // Login cover: unggah lalu hapus dengan login_cover_remove.
    let body = multipart(
        &[],
        &[("login_cover", "cover.webp", b"RIFF\0\0\0\0WEBPVP8 ")],
    );
    let (st, _) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let body = multipart(&[("login_cover_remove", "1")], &[]);
    let (st, data) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(entry(&data, "login_cover")["value"], Value::Null);
    assert_eq!(entry(&data, "login_cover")["is_configured"], false);

    // Template kontrak: unggahan (ekstensi di-lowercase) dan unduhan memuat isi yang sama.
    let body = multipart(&[], &[("kontrak_template_bap", "BAP Uji.DOCX", DOCX)]);
    let (st, _) = send(
        &pool,
        Method::POST,
        uri,
        Some(&admin),
        body,
        Some(MULTIPART_CT),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, list) = send(
        &pool,
        Method::GET,
        "/api/app-settings/kontrak-templates",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let bap = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["key"] == "kontrak_template_bap")
        .unwrap();
    assert_eq!(bap["has_custom"], true);
    assert_eq!(bap["default_filename"], "bap_template.docx");
    assert_eq!(bap["format"], "docx");
    assert_eq!(bap["form_field"], "kontrak_template_bap");
    let custom_name = bap["filename"].as_str().unwrap().to_string();
    assert!(
        custom_name.starts_with("kontrak_template_bap_") && custom_name.ends_with(".docx"),
        "{custom_name}"
    );
    assert!(bap["updated_at"].as_str().unwrap().ends_with("+00:00"));

    let uri_bap = "/api/app-settings/kontrak-templates/kontrak_template_bap/download";
    let (st, headers, bytes) =
        send_raw(&pool, Method::GET, uri_bap, Some(&admin), vec![], None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, DOCX);
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        format!("attachment; filename=\"{custom_name}\"").as_str()
    );

    // Tanpa berkas unggahan: berkas default dari repo.
    sqlx::query(
        "DELETE m FROM media m JOIN app_settings s ON s.id = m.model_id \
         WHERE s.`key` = 'kontrak_template_bap' AND m.model_type = ?",
    )
    .bind(APP_MODEL)
    .execute(&pool)
    .await
    .unwrap();
    let (st, headers, bytes) =
        send_raw(&pool, Method::GET, uri_bap, Some(&admin), vec![], None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, kontrak_default("bap_template.docx"));
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"bap_template.docx\""
    );

    // Kunci tidak dikenal.
    let (st, data) = send(
        &pool,
        Method::GET,
        "/api/app-settings/kontrak-templates/kontrak_nope/download",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(data["message"], "Template tidak dikenal.");

    restore(&pool, &base, &before).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn storage_stats_shape_and_admin_only() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let before = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;
    let uri = "/api/app-settings/storage-stats";

    let (st, _) = send(&pool, Method::GET, uri, Some(&plain), vec![], None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, first) = send(&pool, Method::GET, uri, Some(&admin), vec![], None).await;
    assert_eq!(st, StatusCode::OK);
    for key in [
        "foto",
        "foto_count",
        "berkas",
        "berkas_count",
        "database",
        "media_total",
        "app_total",
    ] {
        assert!(first["data"].get(key).is_some(), "kunci {key}");
    }
    assert!(first["data"]["database"].as_f64().unwrap() >= 0.0);

    // Satu media foto berukuran 1234 byte menambah `foto` tepat 1234 dan `foto_count` 1.
    sqlx::query(
        "INSERT INTO media (model_type, model_id, uuid, collection_name, name, file_name, mime_type, disk, conversions_disk, \
         size, manipulations, custom_properties, generated_conversions, responsive_images, order_column, created_at, updated_at) \
         VALUES ('App\\\\Models\\\\Foto', 0, 'uji-set-stats', 'foto/pekerjaan', 'uji', 'uji.jpg', 'image/jpeg', 'public', 'public', \
         1234, '[]', '[]', '[]', '[]', 1, NOW(), NOW())",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (st, second) = send(&pool, Method::GET, uri, Some(&admin), vec![], None).await;
    assert_eq!(st, StatusCode::OK);
    let f0 = first["data"]["foto"].as_f64().unwrap();
    let f1 = second["data"]["foto"].as_f64().unwrap();
    assert_eq!(f1 - f0, 1234.0);
    assert_eq!(
        second["data"]["foto_count"].as_i64().unwrap(),
        first["data"]["foto_count"].as_i64().unwrap() + 1
    );
    assert_eq!(
        second["data"]["media_total"].as_f64().unwrap(),
        second["data"]["foto"].as_f64().unwrap() + second["data"]["berkas"].as_f64().unwrap()
    );
    sqlx::query("DELETE FROM media WHERE uuid = 'uji-set-stats'")
        .execute(&pool)
        .await
        .unwrap();

    restore(&pool, &base, &before).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn backups_local_index_job_download_and_destroy() {
    let _guard = LOCK.lock().await;
    let base = setup_env();
    let pool = pool().await;
    let before = snapshot(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;

    // Tanpa S3: sisanya memakai berkas lokal.
    sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES ('s3_backup_enabled', '0', 'text', NOW(), NOW()) \
                 ON DUPLICATE KEY UPDATE `value` = '0'")
        .execute(&pool)
        .await
        .unwrap();

    let dir = base.join("private").join("system-backups");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("jobs")).unwrap();
    std::fs::write(dir.join("uji-backup-a.zip"), b"PK\x03\x04a").unwrap();
    std::fs::write(dir.join("uji-backup-b.zip"), b"PK\x03\x04bb").unwrap();
    std::fs::write(dir.join("catatan.txt"), b"bukan backup").unwrap();
    std::fs::write(
        dir.join("jobs").join("uji-job-1.json"),
        br#"{"job_id":"uji-job-1","status":"completed","progress":100,"filename":"uji-backup-a.zip"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("jobs").join("uji-job-rusak.json"), b"\"string\"").unwrap();

    // Daftar: hanya `.zip`, dengan download_url absolut.
    let (st, _) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups",
        Some(&plain),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, list) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let names: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["filename"].as_str().unwrap())
        .filter(|n| n.starts_with("uji-"))
        .collect();
    assert_eq!(names.len(), 2);
    let a = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["filename"] == "uji-backup-a.zip")
        .unwrap();
    assert_eq!(a["size"], 5);
    assert_eq!(a["storage"], "local");
    assert_eq!(
        a["download_url"],
        "http://localhost/api/app-settings/backups/uji-backup-a.zip"
    );
    assert!(a["last_modified"].as_i64().unwrap() > 0);

    // Status job.
    let (st, job) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups/jobs/uji-job-1",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(job["data"]["status"], "completed");
    assert_eq!(job["data"]["progress"], 100);
    let (st, data) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups/jobs/uji-job-tidak-ada",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(data["message"], "Status backup tidak ditemukan");
    let (st, data) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups/jobs/uji-job-rusak",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "JSON bukan objek/array dianggap tidak ada: {data}"
    );
    let (st, data) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups/jobs/a.b",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(data["message"], "ID backup tidak valid");

    // Unduh: header dan isi sama dengan berkas.
    let (st, headers, bytes) = send_raw(
        &pool,
        Method::GET,
        "/api/app-settings/backups/uji-backup-a.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, b"PK\x03\x04a");
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store, private");
    assert_eq!(
        headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"uji-backup-a.zip\""
    );
    let (st, _, _) = send_raw(
        &pool,
        Method::GET,
        "/api/app-settings/backups/uji-tidak-ada.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, data) = send(
        &pool,
        Method::GET,
        "/api/app-settings/backups/a%20b.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(data["message"], "Nama backup tidak valid");
    // Nama yang tidak berakhiran `.zip` tidak cocok dengan rute: 404 tanpa cek login.
    let (st, _, _) = send_raw(
        &pool,
        Method::GET,
        "/api/app-settings/backups/uji.json",
        None,
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Hapus: berkas hilang, lalu hapus lagi menjawab 404.
    let (st, _) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-backup-b.zip",
        Some(&plain),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, data) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-backup-b.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data["message"], "Backup berhasil dihapus");
    assert!(!dir.join("uji-backup-b.zip").exists());
    let (st, data) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-backup-b.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(data["message"], "Backup tidak ditemukan");

    // Dengan S3 aktif, hapus dan unduh berkas yang tidak lokal ditolak tanpa mengubah apa pun.
    sqlx::query("UPDATE app_settings SET `value` = '1' WHERE `key` = 's3_backup_enabled'")
        .execute(&pool)
        .await
        .unwrap();
    let (st, _) = send(
        &pool,
        Method::DELETE,
        "/api/app-settings/backups/uji-backup-a.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_IMPLEMENTED);
    assert!(dir.join("uji-backup-a.zip").exists());
    let (st, _, _) = send_raw(
        &pool,
        Method::GET,
        "/api/app-settings/backups/uji-hanya-s3.zip",
        Some(&admin),
        vec![],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_IMPLEMENTED);

    let _ = std::fs::remove_dir_all(&dir);
    restore(&pool, &base, &before).await;
}
