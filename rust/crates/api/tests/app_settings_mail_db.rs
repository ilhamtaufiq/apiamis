//! Rute uji email `app-settings` lewat router terhadap MySQL: `test-mail-connection`, `mail-templates`
//! (GET dan POST), dan `mail-templates/{key}/test`.
//!
//! Tidak ada email sungguhan. Pengiriman memakai `StubSender` yang mencatat penerima dan subjek. Kredensial
//! SMTP di tes hanya placeholder yang dibuat saat runtime. Tes berjalan berurutan (`LOCK`). Setelah tes,
//! baris `app_settings` yang disentuh dikembalikan ke keadaan semula dan audit yang dibuat tes dihapus.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test app_settings_mail_db -- --include-ignored
//! ```

use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use api::{
    app,
    app_settings_mail::{test_mail_connection_with, test_mail_template_with, MailSender, WriteCtx},
    mailer::SmtpSettings,
    AppState,
};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Map, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tokio::sync::Mutex as AsyncMutex;
use tower::ServiceExt;

const ADMIN: &str = "uji-asm-admin@example.test";
const PLAIN: &str = "uji-asm-plain@example.test";
const TO: &str = "uji-asm-penerima@example.test";
const FROM: &str = "uji-asm-pengirim@example.test";
const APP_MODEL: &str = "App\\Models\\AppSetting";

/// Setting yang disentuh tes; nilainya (atau ketiadaannya) dikembalikan setelah tes.
const TOUCHED: &[&str] = &[
    "mail_templates",
    "brand_primary_color",
    "mail_enabled",
    "mail_host",
    "mail_port",
    "mail_encryption",
    "mail_username",
    "mail_password",
    "mail_from_address",
    "mail_from_name",
    "mail_subject",
    "mail_body",
    "mail_body_format",
];

static LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Pengirim palsu: mencatat penerima, subjek, dan nama pengguna SMTP yang dipakai.
#[derive(Default)]
struct StubSender {
    calls: Mutex<Vec<(String, String, String)>>,
    fail_with: Option<String>,
}

impl MailSender for StubSender {
    fn send<'a>(
        &'a self,
        settings: &'a SmtpSettings,
        to: &'a str,
        subject: &'a str,
        _text: &'a str,
        _html: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self.calls.lock().unwrap().push((
            to.to_string(),
            subject.to_string(),
            settings.username.clone(),
        ));
        let outcome = match &self.fail_with {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        };
        Box::pin(async move { outcome })
    }
}

impl StubSender {
    fn calls(&self) -> Vec<(String, String, String)> {
        self.calls.lock().unwrap().clone()
    }
}

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 16 * 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

/// Kata sandi placeholder yang dibuat saat runtime. Bukan kredensial.
fn placeholder_password() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("uji-asm-{nanos}")
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji ASM', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-asm")
        .await
        .unwrap()
}

async fn remove_users(pool: &MySqlPool) {
    for email in [ADMIN, PLAIN] {
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

/// Keadaan awal satu setting: `None` bila baris tidak ada.
type Snapshot = BTreeMap<&'static str, Option<(Option<String>, String)>>;

async fn snapshot(pool: &MySqlPool) -> Snapshot {
    let mut out = BTreeMap::new();
    for key in TOUCHED {
        let row = sqlx::query(
            "SELECT `value`, `type` FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap();
        out.insert(
            *key,
            row.map(|r| (r.try_get("value").unwrap(), r.try_get("type").unwrap())),
        );
    }
    out
}

async fn restore(pool: &MySqlPool, snap: &Snapshot) {
    for (key, original) in snap {
        sqlx::query("DELETE FROM app_settings WHERE `key` = ?")
            .bind(key)
            .execute(pool)
            .await
            .unwrap();
        if let Some((value, kind)) = original {
            sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
                .bind(key)
                .bind(value)
                .bind(kind)
                .execute(pool)
                .await
                .unwrap();
        }
    }
}

async fn set_setting(pool: &MySqlPool, key: &str, value: &str) {
    sqlx::query("DELETE FROM app_settings WHERE `key` = ?")
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, 'text', NOW(), NOW())")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .unwrap();
}

/// Hapus audit yang dibuat tes untuk setting yang disentuh (id audit di atas `max_before`).
async fn clean_audit(pool: &MySqlPool, max_before: u64) {
    for key in TOUCHED {
        let ids: Vec<u64> =
            sqlx::query_scalar("SELECT CAST(id AS UNSIGNED) FROM app_settings WHERE `key` = ?")
                .bind(key)
                .fetch_all(pool)
                .await
                .unwrap();
        for id in ids {
            sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND id > ?")
                .bind(APP_MODEL)
                .bind(id)
                .bind(max_before)
                .execute(pool)
                .await
                .unwrap();
        }
    }
}

async fn max_audit_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar("SELECT CAST(COALESCE(MAX(id), 0) AS UNSIGNED) FROM tbl_audit_logs")
        .fetch_one(pool)
        .await
        .unwrap()
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
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
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

fn ctx<'a>(headers: &'a HeaderMap, url: &'a str) -> WriteCtx<'a> {
    WriteCtx {
        actor: 1,
        url,
        headers,
    }
}

fn fields(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

// ---------------------------------------------------------------------------
// Akses admin
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn mail_routes_forbidden_for_plain_user() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let token = user_token(&pool, PLAIN, false).await;
    let cases = [
        (Method::GET, "/api/app-settings/mail-templates", None),
        (
            Method::POST,
            "/api/app-settings/mail-templates",
            Some(json!({ "templates": { "welcome": { "format": "html" } } })),
        ),
        (
            Method::POST,
            "/api/app-settings/test-mail-connection",
            Some(json!({ "to": TO })),
        ),
        (
            Method::POST,
            "/api/app-settings/mail-templates/welcome/test",
            Some(json!({ "to": TO })),
        ),
    ];
    for (method, uri, body) in cases {
        let (status, _) = send(&pool, method, uri, Some(&token), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }
    remove_users(&pool).await;
}

// ---------------------------------------------------------------------------
// GET dan POST mail-templates
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn get_mail_templates_lists_templates_with_presets_and_stores_brand_color() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let audit_before = max_audit_id(&pool).await;
    // Warna merah muda dibersihkan ke default (`sanitizeBrandPrimary`) dan ditulis ulang saat GET.
    set_setting(&pool, "brand_primary_color", "ff00aa").await;
    let token = user_token(&pool, ADMIN, true).await;

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/app-settings/mail-templates",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().expect("data array");
    assert_eq!(data.len(), 10);
    let keys: Vec<&str> = data.iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(keys[0], "smtp_test");
    assert!(keys.contains(&"report_submitted"));
    for t in data {
        for fmt in ["markdown", "html", "plain"] {
            assert!(
                t["presets"][fmt]["body"].is_string(),
                "preset {fmt} pada {}",
                t["key"]
            );
        }
        assert_eq!(t["is_custom"], json!(false));
    }
    let welcome_html = data.iter().find(|t| t["key"] == "welcome").unwrap()["presets"]["html"]
        ["body"]
        .as_str()
        .unwrap()
        .to_string();
    // Badge memakai strtoupper: "SELAMAT DATANG".
    assert!(welcome_html.contains("SELAMAT DATANG"));
    assert!(welcome_html.contains("{{user_name}}"));

    let stored: String =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = 'brand_primary_color'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, "#674bb5");

    clean_audit(&pool, audit_before).await;
    restore(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_mail_templates_saves_known_keys_and_validates() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let audit_before = max_audit_id(&pool).await;
    let token = user_token(&pool, ADMIN, true).await;

    // Validasi: templates wajib, format di daftar, dan panjang subjek.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates",
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["templates"].is_array());

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates",
        Some(&token),
        Some(json!({ "templates": { "welcome": { "format": "pdf" } } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["templates.welcome.format"].is_array());

    let long_subject = "s".repeat(256);
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates",
        Some(&token),
        Some(json!({ "templates": { "welcome": { "subject": long_subject } } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["templates.welcome.subject"].is_array());

    // Simpan: kunci tidak dikenal dilewati, subjek kosong memakai default.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates",
        Some(&token),
        Some(json!({
            "templates": {
                "welcome": { "format": "markdown", "subject": "uji-asm-judul", "body": "Halo **uji**" },
                "report_submitted": { "format": "plain", "subject": "" },
                "uji-asm-tidak-ada": { "format": "html", "subject": "x", "body": "y" }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Template email berhasil disimpan.");
    let data = body["data"].as_array().unwrap();
    let welcome = data.iter().find(|t| t["key"] == "welcome").unwrap();
    assert_eq!(welcome["is_custom"], json!(true));
    assert_eq!(welcome["format"], "markdown");
    assert_eq!(welcome["subject"], "uji-asm-judul");
    assert_eq!(welcome["body"], "Halo **uji**");
    assert!(welcome["updated_at"].is_string());
    let report = data
        .iter()
        .find(|t| t["key"] == "report_submitted")
        .unwrap();
    assert_eq!(report["subject"], "Laporan {{report_name}} terkirim");
    let untouched = data.iter().find(|t| t["key"] == "forgot_password").unwrap();
    assert_eq!(untouched["is_custom"], json!(false));
    assert!(data.iter().all(|t| t["key"] != "uji-asm-tidak-ada"));

    let raw: String =
        sqlx::query_scalar("SELECT `value` FROM app_settings WHERE `key` = 'mail_templates'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let stored: Value = serde_json::from_str(&raw).unwrap();
    assert!(stored.get("welcome").is_some());
    assert!(stored.get("uji-asm-tidak-ada").is_none());

    clean_audit(&pool, audit_before).await;
    restore(&pool, &snap).await;
    remove_users(&pool).await;
}

// ---------------------------------------------------------------------------
// Uji template dan uji koneksi (stub, tanpa email sungguhan)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn test_template_route_rejects_unknown_key_and_bad_input() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let token = user_token(&pool, ADMIN, true).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates/uji-asm-tidak-ada/test",
        Some(&token),
        Some(json!({ "to": TO })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({ "ok": false, "error": "Template email tidak dikenal." })
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/app-settings/mail-templates/welcome/test",
        Some(&token),
        Some(json!({ "to": "bukan-email" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["to"].is_array());

    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn test_mail_template_sends_through_stub_with_expected_recipient_and_subject() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    set_setting(&pool, "mail_enabled", "1").await;
    set_setting(&pool, "mail_username", FROM).await;
    let headers = HeaderMap::new();
    let url = "http://localhost/api/app-settings/mail-templates/welcome/test";
    let stub = StubSender::default();
    let password = placeholder_password();

    let (status, body) = test_mail_template_with(
        &pool,
        "http://localhost",
        &ctx(&headers, url),
        &stub,
        "welcome",
        &fields(json!({
            "to": "Uji-ASM-Penerima@Example.TEST",
            "format": "markdown",
            "subject": "uji-asm-judul {{user_name}}",
            "body": "Halo **{{user_name}}**",
            "mail_password": password,
        })),
    )
    .await
    .unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["to"], "Uji-ASM-Penerima@Example.TEST");
    assert_eq!(body["format"], "markdown");
    assert_eq!(body["template_key"], "welcome");
    assert_eq!(body["used_stored_password"], json!(false));
    // Penerima dinormalkan ke huruf kecil; placeholder pada subjek diganti dengan contoh.
    assert_eq!(
        stub.calls(),
        vec![(
            TO.to_string(),
            "uji-asm-judul Budi Santoso".to_string(),
            FROM.to_string()
        )]
    );

    restore(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn test_mail_connection_sends_through_stub_and_reports_failures() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let headers = HeaderMap::new();
    let url = "http://localhost/api/app-settings/test-mail-connection";
    let password = placeholder_password();
    let base = json!({
        "to": TO,
        "mail_username": FROM,
        "mail_password": password,
        "mail_body_format": "plain",
        "mail_subject": "uji-asm-subjek",
        "mail_body": "Isi uji",
    });

    // Berhasil: override permintaan dipakai, kata sandi dari permintaan (tanpa kata sandi tersimpan).
    let stub = StubSender::default();
    let (status, body) = test_mail_connection_with(
        &pool,
        "http://localhost",
        &ctx(&headers, url),
        &stub,
        &fields(base.clone()),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["template_key"], "smtp_test");
    assert_eq!(body["format"], "plain");
    assert_eq!(body["used_stored_password"], json!(false));
    assert_eq!(
        stub.calls(),
        vec![(
            TO.to_string(),
            "uji-asm-subjek".to_string(),
            FROM.to_string()
        )]
    );

    // Gagal kirim: 422 dengan pesan, dan kata sandi tersimpan dipakai bila tidak dikirim.
    let failing = StubSender {
        calls: Mutex::new(Vec::new()),
        fail_with: Some("uji-asm-gagal".to_string()),
    };
    let mut without_password = base.as_object().unwrap().clone();
    without_password.remove("mail_password");
    set_setting(&pool, "mail_password", &placeholder_password()).await;
    let (status, body) = test_mail_connection_with(
        &pool,
        "http://localhost",
        &ctx(&headers, url),
        &failing,
        &without_password,
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["error"], "Gagal mengirim email: uji-asm-gagal");
    assert_eq!(body["used_stored_password"], json!(true));

    restore(&pool, &snap).await;
    remove_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn test_mail_connection_without_smtp_is_400_and_never_sends() {
    let _guard = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    // Tanpa username tersimpan atau di permintaan, SMTP tidak lengkap.
    set_setting(&pool, "mail_username", "").await;
    let headers = HeaderMap::new();
    let url = "http://localhost/api/app-settings/test-mail-connection";
    let stub = StubSender::default();

    let (status, body) = test_mail_connection_with(
        &pool,
        "http://localhost",
        &ctx(&headers, url),
        &stub,
        &fields(json!({ "to": TO })),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], json!(false));
    assert!(body["error"]
        .as_str()
        .unwrap()
        .starts_with("SMTP belum lengkap."));
    assert!(stub.calls().is_empty());

    // Kunci templat tidak dikenal: error 500 setelah pengecekan SMTP, tanpa pengiriman.
    let stub = StubSender::default();
    let err = test_mail_connection_with(
        &pool,
        "http://localhost",
        &ctx(&headers, url),
        &stub,
        &fields(json!({
            "to": TO,
            "template_key": "uji-asm-tidak-ada",
            "mail_username": FROM,
            "mail_password": placeholder_password(),
        })),
    )
    .await
    .unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(stub.calls().is_empty());

    restore(&pool, &snap).await;
    remove_users(&pool).await;
}
