//! Pengingat kelengkapan `POST /api/user-pekerjaan/broadcast-reminders` terhadap MySQL. Tes lewat router
//! untuk akses, validasi, dan formulir. Tes pengiriman email memanggil `broadcast_reminders_with` dengan
//! stub `MailSender`, sehingga tidak ada email sungguhan. Lewat router hanya dipakai dengan email nonaktif.
//!
//! Semua data uji memakai marker `uji-br-` (email user dan nama pekerjaan). Tes berjalan berurutan (`LOCK`).
//! Setelah tiap tes, riwayat dan notifikasi milik user uji dihapus, lalu setting email dikembalikan.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -j 2 -p api --test user_pekerjaan_broadcast_db -- --include-ignored
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
    app_settings_mail::{MailSender, WriteCtx},
    mailer::SmtpSettings,
    user_pekerjaan_broadcast::broadcast_reminders_with,
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

const ADMIN: &str = "uji-br-admin@example.test";
const PLAIN: &str = "uji-br-plain@example.test";
const USER_A: &str = "uji-br-a@example.test";
/// Email yang kosong setelah dipangkas. Akun uji B memakai ini.
const USER_B_NO_EMAIL: &str = "   ";
const USER_C: &str = "uji-br-c@example.test";
const MARKER_LIKE: &str = "uji-br-%";
const APP_URL: &str = "http://localhost";
const NOTIF_MODEL_USER: &str = r"App\Models\User";
const NOTIF_TYPE: &str = r"App\Notifications\AppNotification";
const DEFAULT_TITLE: &str = "Pengingat Kelengkapan Data Pekerjaan";
const NO_USERS: &str = "Tidak ada pengawas dengan data belum lengkap untuk dikirimi pengingat";

/// Setting yang disentuh tes; nilai awalnya (atau ketiadaannya) dikembalikan setelah tiap tes.
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
];

static LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Satu email yang diterima stub.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct Sent {
    to: String,
    subject: String,
    text: String,
    html: Option<String>,
}

/// Pengirim palsu. Mencatat setiap panggilan, dan bisa dibuat gagal.
#[derive(Default)]
struct StubSender {
    calls: Mutex<Vec<Sent>>,
    fail_with: Option<String>,
}

impl MailSender for StubSender {
    fn send<'a>(
        &'a self,
        _settings: &'a SmtpSettings,
        to: &'a str,
        subject: &'a str,
        text: &'a str,
        html: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self.calls.lock().unwrap().push(Sent {
            to: to.to_string(),
            subject: subject.to_string(),
            text: text.to_string(),
            html: html.map(str::to_string),
        });
        let outcome = match &self.fail_with {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        };
        Box::pin(async move { outcome })
    }
}

impl StubSender {
    fn calls(&self) -> Vec<Sent> {
        self.calls.lock().unwrap().clone()
    }
}

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: APP_URL.to_string(),
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
    format!("uji-br-{nanos}")
}

/// Kirim permintaan lewat router. Mengembalikan status dan body JSON (`Null` bila bukan JSON).
async fn send(
    pool: &MySqlPool,
    token: Option<&str>,
    content_type: Option<&str>,
    body: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/user-pekerjaan/broadcast-reminders");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let res = app(&config(), AppState::new(pool.clone(), APP_URL.to_string()))
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
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

async fn json_post(pool: &MySqlPool, token: &str, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Some(token),
        Some("application/json"),
        &body.to_string(),
    )
    .await
}

/// Panggil inti tanpa HTTP dengan stub. Input berupa objek JSON.
async fn call_with(
    pool: &MySqlPool,
    sender: &StubSender,
    input: Value,
) -> Result<(StatusCode, Value), shared::ApiError> {
    let headers = HeaderMap::new();
    let url = "http://localhost/api/user-pekerjaan/broadcast-reminders";
    let ctx = WriteCtx {
        actor: 0,
        url,
        headers: &headers,
    };
    let map: Map<String, Value> = input.as_object().cloned().unwrap_or_default();
    broadcast_reminders_with(pool, APP_URL, &ctx, sender, &map).await
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji BR Admin', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = uid_of(pool, email).await;
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
    auth::login::create_token(pool, uid, "uji-br")
        .await
        .unwrap()
}

async fn uid_of(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query_scalar("SELECT CAST(id AS UNSIGNED) FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Data uji: admin, user biasa, dua pengawas dengan tugas (A bersurel, B tanpa surel), dan C tanpa tugas.
struct Fixture {
    admin: String,
    plain: String,
    a: u64,
    b: u64,
    c: u64,
    p1: u64,
}

async fn make_pekerjaan(pool: &MySqlPool, nama: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, NULL, 1500000, 0, 'active', NOW(), NOW())")
        .bind(nama)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT CAST(id AS UNSIGNED) FROM tbl_pekerjaan WHERE nama_paket = ? ORDER BY id DESC LIMIT 1")
        .bind(nama)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn assign(pool: &MySqlPool, user: u64, pekerjaan: u64) {
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(user)
        .bind(pekerjaan)
        .execute(pool)
        .await
        .unwrap();
}

async fn fixture(pool: &MySqlPool) -> Fixture {
    purge(pool).await;
    let admin = user_token(pool, ADMIN, true).await;
    let plain = user_token(pool, PLAIN, false).await;
    // Pengguna pengawas dibuat langsung, tanpa token.
    for (name, email) in [
        ("Uji BR A", USER_A),
        ("Uji BR Tanpa Surel", USER_B_NO_EMAIL),
        ("Uji BR C", USER_C),
    ] {
        sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
            .bind(name)
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
    }
    let a = uid_of(pool, USER_A).await;
    let c = uid_of(pool, USER_C).await;
    let b: u64 = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM users WHERE name = 'Uji BR Tanpa Surel' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    // Tanpa foto dan tanpa progres: kedua pekerjaan punya gap foto dan progress.
    let p1 = make_pekerjaan(pool, "uji-br-P1 Rehab Jembatan").await;
    let p2 = make_pekerjaan(pool, "uji-br-P2 Irigasi").await;
    // P2 hanya untuk user B. Id-nya tidak dibaca tes, tetapi baris ini membuat data penugasan.
    assign(pool, a, p1).await;
    assign(pool, b, p2).await;
    Fixture {
        admin,
        plain,
        a,
        b,
        c,
        p1,
    }
}

/// Hapus data uji: notifikasi dan riwayat milik user uji (riwayat diambil dari `data` notifikasi dulu),
/// assignment, pekerjaan bermarker, dan user uji.
async fn purge(pool: &MySqlPool) {
    let user_ids: Vec<u64> = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM users WHERE email LIKE ? OR name LIKE 'Uji BR%'",
    )
    .bind(MARKER_LIKE)
    .fetch_all(pool)
    .await
    .unwrap();
    let mut history_ids: Vec<i64> = Vec::new();
    for uid in &user_ids {
        let rows = sqlx::query(
            "SELECT data FROM notifications WHERE notifiable_type = ? AND notifiable_id = ?",
        )
        .bind(NOTIF_MODEL_USER)
        .bind(uid)
        .fetch_all(pool)
        .await
        .unwrap();
        for r in rows {
            let data: String = r.try_get("data").unwrap_or_default();
            if let Ok(v) = serde_json::from_str::<Value>(&data) {
                if let Some(id) = v["broadcast_history_id"].as_i64() {
                    history_ids.push(id);
                }
            }
        }
    }
    for uid in &user_ids {
        sqlx::query("DELETE FROM notifications WHERE notifiable_type = ? AND notifiable_id = ?")
            .bind(NOTIF_MODEL_USER)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    for id in history_ids {
        sqlx::query("DELETE FROM broadcast_histories WHERE id = ? AND title LIKE ?")
            .bind(id)
            .bind(MARKER_LIKE)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM broadcast_histories WHERE id = ? AND title = ?")
            .bind(id)
            .bind(DEFAULT_TITLE)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE ?")
        .bind("uji-br-%")
        .execute(pool)
        .await
        .unwrap();
    for uid in &user_ids {
        sqlx::query("DELETE FROM model_has_roles WHERE model_id = ?")
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
}

/// Keadaan awal setting: `None` bila baris tidak ada.
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

/// Aktifkan SMTP dengan nilai placeholder. Tidak ada koneksi karena pengirim adalah stub.
async fn enable_smtp(pool: &MySqlPool) {
    set_setting(pool, "mail_enabled", "1").await;
    set_setting(pool, "mail_host", "smtp.uji.test").await;
    set_setting(pool, "mail_port", "587").await;
    set_setting(pool, "mail_encryption", "tls").await;
    set_setting(pool, "mail_username", "uji-br-smtp@example.test").await;
    set_setting(pool, "mail_password", &placeholder_password()).await;
    set_setting(pool, "mail_from_address", "uji-br-smtp@example.test").await;
    set_setting(pool, "mail_from_name", "Uji BR").await;
}

/// Riwayat dan notifikasi milik satu user: (judul, pesan, type notifikasi, url, broadcast_history_id, dan
/// baris `broadcast_histories`).
struct Delivered {
    data: Value,
    history: Option<(String, String, String, String, bool, i64)>,
    notif_type: String,
}

async fn delivered_to(pool: &MySqlPool, uid: u64) -> Vec<Delivered> {
    let rows = sqlx::query("SELECT data, type FROM notifications WHERE notifiable_type = ? AND notifiable_id = ? ORDER BY created_at, id")
        .bind(NOTIF_MODEL_USER)
        .bind(uid)
        .fetch_all(pool)
        .await
        .unwrap();
    let mut out = Vec::new();
    for r in rows {
        let data: Value = serde_json::from_str(&r.try_get::<String, _>("data").unwrap()).unwrap();
        let notif_type: String = r.try_get("type").unwrap();
        let history = match data["broadcast_history_id"].as_i64() {
            Some(hid) => {
                let h = sqlx::query("SELECT title, message, notification_type, url, is_banner, CAST(recipient_count AS SIGNED) AS rc, type FROM broadcast_histories WHERE id = ?")
                    .bind(hid)
                    .fetch_optional(pool)
                    .await
                    .unwrap();
                h.map(|h| {
                    (
                        h.try_get::<String, _>("title").unwrap(),
                        h.try_get::<String, _>("message").unwrap(),
                        h.try_get::<String, _>("notification_type").unwrap(),
                        h.try_get::<String, _>("url").unwrap(),
                        h.try_get::<bool, _>("is_banner").unwrap(),
                        hid,
                    )
                })
            }
            None => None,
        };
        out.push(Delivered {
            data,
            history,
            notif_type,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Akses dan validasi lewat router
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn akses_non_admin_403_tanpa_token_401() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let (status, _) = json_post(&pool, &f.plain, json!({ "user_ids": [f.a] })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(&pool, None, Some("application/json"), "{}").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Tidak ada yang tertulis untuk user A.
    assert!(delivered_to(&pool, f.a).await.is_empty());

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_422_dengan_pesan_laravel() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let (status, body) = json_post(
        &pool,
        &f.admin,
        json!({
            "notification_type": "bogus",
            "tahun": 1999,
            "send_email": "maybe",
            "gaps": ["x"],
            "user_ids": [999_999_999],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let errors = &body["errors"];
    assert_eq!(
        errors["notification_type"][0],
        "The selected notification type is invalid."
    );
    assert_eq!(errors["tahun"][0], "The tahun field must be at least 2000.");
    assert_eq!(
        errors["send_email"][0],
        "The send email field must be true or false."
    );
    assert_eq!(errors["gaps.0"][0], "The selected gaps.0 is invalid.");
    assert_eq!(
        errors["user_ids.0"][0],
        "The selected user_ids.0 is invalid."
    );
    assert!(delivered_to(&pool, f.a).await.is_empty());

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn notifikasi_dalam_aplikasi_tanpa_email() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let title = "uji-br-judul aplikasi";
    let (status, body) = json_post(
        &pool,
        &f.admin,
        json!({
            "user_ids": [f.a],
            "title": title,
            "notification_type": "info",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "success");
    assert_eq!(body["message"], "Pengingat kelengkapan berhasil dikirim");
    assert_eq!(body["recipient_count"], 1);
    assert_eq!(body["send_email"], false);
    assert_eq!(body["email_sent_count"], 0);
    assert_eq!(body["smtp_unavailable"], false);
    assert_eq!(body["email_recipients"], json!([]));
    // Basis URL bisa dikonfigurasi (frontend_url, PENGAWAS_APP_BASE_URL), jadi hanya akhirannya dicek.
    assert!(body["action_url_sample"]
        .as_str()
        .unwrap()
        .ends_with(&format!("/pekerjaan/{}", f.p1)));

    let delivered = delivered_to(&pool, f.a).await;
    assert_eq!(delivered.len(), 1);
    let d = &delivered[0];
    assert_eq!(d.notif_type, NOTIF_TYPE);
    assert_eq!(d.data["title"], title);
    assert_eq!(d.data["type"], "info");
    assert_eq!(d.data["url"], format!("/pekerjaan/{}", f.p1));
    assert_eq!(d.data["is_banner"], false);
    let message = d.data["message"].as_str().unwrap();
    assert!(message.starts_with(
        "Halo Uji BR A,\n\nBeberapa pekerjaan yang ditugaskan kepada Anda masih belum lengkap:\n\n"
    ));
    assert!(message.contains("• uji-br-P1 Rehab Jembatan — belum lengkap: foto, progress"));
    assert!(message.ends_with("\n\nSilakan lengkapi data di aplikasi pengawasan."));

    let (htitle, hmessage, htype, hurl, hbanner, hid) = d.history.clone().expect("riwayat ada");
    assert_eq!(hid, d.data["broadcast_history_id"].as_i64().unwrap());
    assert_eq!(htitle, title);
    assert_eq!(hmessage, message);
    assert_eq!(htype, "info");
    assert_eq!(hurl, format!("/pekerjaan/{}", f.p1));
    assert!(!hbanner);

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn prefix_kustom_dan_nilai_default() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let (status, body) = json_post(
        &pool,
        &f.admin,
        json!({ "user_ids": [f.a], "message_prefix": "  uji-br-pembuka  " }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let delivered = delivered_to(&pool, f.a).await;
    let d = &delivered[0];
    assert_eq!(d.data["title"], DEFAULT_TITLE);
    assert_eq!(d.data["type"], "warning");
    let message = d.data["message"].as_str().unwrap();
    assert!(
        message.starts_with("uji-br-pembuka\n\n• uji-br-P1"),
        "{message}"
    );
    assert!(!message.contains("Halo "));

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn penerima_tanpa_tugas_404() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let (status, body) = json_post(&pool, &f.admin, json!({ "user_ids": [f.c] })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "status": "error", "message": NO_USERS }));
    assert!(delivered_to(&pool, f.c).await.is_empty());

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn formulir_urlencoded_dibaca_seperti_json() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;

    let body = format!(
        "user_ids[]={}&gaps[]=foto&send_email=0&title=uji-br-form&notification_type=success",
        f.a
    );
    let (status, resp) = send(
        &pool,
        Some(&f.admin),
        Some("application/x-www-form-urlencoded"),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["recipient_count"], 1);

    let delivered = delivered_to(&pool, f.a).await;
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].data["title"], "uji-br-form");
    assert_eq!(delivered[0].data["type"], "success");
    // Hanya gap foto yang diminta, jadi pesan tidak memuat "progress".
    let message = delivered[0].data["message"].as_str().unwrap();
    assert!(message.contains("— belum lengkap: foto\n"), "{message}");

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn email_nonaktif_lewat_router_tidak_mengirim() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;
    set_setting(&pool, "mail_enabled", "0").await;

    let (status, body) = json_post(
        &pool,
        &f.admin,
        json!({ "user_ids": [f.a], "send_email": true, "title": "uji-br-router" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["send_email"], true);
    assert_eq!(body["smtp_unavailable"], true);
    assert_eq!(body["email_skipped_count"], 1);
    assert_eq!(
        body["message"],
        "Pengingat kelengkapan berhasil dikirim. Email tidak terkirim karena SMTP belum diaktifkan"
    );

    purge(&pool).await;
    restore(&pool, &snap).await;
}

// ---------------------------------------------------------------------------
// Pengiriman email dengan stub
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn email_stub_terkirim_ke_penerima_bersurel_saja() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;
    enable_smtp(&pool).await;
    let stub = StubSender::default();

    let title = "uji-br-judul email";
    let (status, body) = call_with(
        &pool,
        &stub,
        json!({
            "user_ids": [f.a, f.b],
            "title": title,
            "send_email": true,
            "notification_type": "error",
        }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["recipient_count"], 2);
    assert_eq!(body["send_email"], true);
    assert_eq!(body["email_sent_count"], 1);
    assert_eq!(body["email_failed_count"], 0);
    assert_eq!(body["email_skipped_count"], 1);
    assert_eq!(body["smtp_unavailable"], false);
    assert_eq!(body["email_recipients"], json!([USER_A]));
    assert_eq!(
        body["message"],
        "Pengingat kelengkapan berhasil dikirim. Email terkirim ke 1 pengawas"
    );

    let calls = stub.calls();
    assert_eq!(
        calls.len(),
        1,
        "hanya penerima bersurel yang menerima email"
    );
    let sent = &calls[0];
    assert_eq!(sent.to, USER_A);
    assert!(
        sent.subject.starts_with("uji-br-judul email — "),
        "{}",
        sent.subject
    );
    assert!(sent.text.contains("uji-br-judul email"), "{}", sent.text);
    assert!(
        sent.text.contains("belum lengkap: foto, progress"),
        "{}",
        sent.text
    );
    let html = sent
        .html
        .as_deref()
        .expect("format default broadcast adalah html");
    assert!(
        html.contains(&format!("pekerjaan/{}", f.p1)),
        "tautan aplikasi ada di html"
    );

    // Notifikasi tetap dibuat untuk kedua penerima, dengan email atau tanpa.
    assert_eq!(delivered_to(&pool, f.a).await.len(), 1);
    assert_eq!(delivered_to(&pool, f.b).await.len(), 1);

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn email_gagal_dihitung_dan_tidak_menghentikan_proses() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;
    enable_smtp(&pool).await;
    let stub = StubSender {
        fail_with: Some("uji-br-gagal".to_string()),
        ..Default::default()
    };

    let (status, body) = call_with(
        &pool,
        &stub,
        json!({ "user_ids": [f.a], "send_email": "1", "title": "uji-br-gagal" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email_failed_count"], 1);
    assert_eq!(body["email_sent_count"], 0);
    assert_eq!(body["email_recipients"], json!([]));
    assert_eq!(
        body["message"],
        "Pengingat kelengkapan berhasil dikirim. Email terkirim ke 0 pengawas, 1 gagal"
    );
    assert_eq!(stub.calls().len(), 1);
    // Notifikasi dalam aplikasi tetap ditulis walau email gagal.
    assert_eq!(delivered_to(&pool, f.a).await.len(), 1);

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn smtp_nonaktif_tidak_memanggil_pengirim() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;
    set_setting(&pool, "mail_enabled", "0").await;
    let stub = StubSender::default();

    let (status, body) = call_with(
        &pool,
        &stub,
        json!({ "user_ids": [f.a, f.b], "send_email": true, "title": "uji-br-smtp" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["smtp_unavailable"], true);
    assert_eq!(body["email_sent_count"], 0);
    // A: SMTP nonaktif. B: tanpa surel dicek lebih dulu, jadi dilewati sebagai no_email.
    assert_eq!(body["email_skipped_count"], 2);
    assert_eq!(
        body["message"],
        "Pengingat kelengkapan berhasil dikirim. Email tidak terkirim karena SMTP belum diaktifkan"
    );
    assert!(stub.calls().is_empty());

    purge(&pool).await;
    restore(&pool, &snap).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn template_broadcast_tersimpan_dipakai_untuk_subjek_dan_format() {
    let _g = LOCK.lock().await;
    let pool = pool().await;
    let snap = snapshot(&pool).await;
    let f = fixture(&pool).await;
    enable_smtp(&pool).await;
    set_setting(
        &pool,
        "mail_templates",
        r#"{"broadcast":{"format":"plain","subject":"Kustom {{title}}","body":"diabaikan"}}"#,
    )
    .await;
    let stub = StubSender::default();

    let (status, _) = call_with(
        &pool,
        &stub,
        json!({ "user_ids": [f.a], "send_email": true, "title": "uji-br-kustom" }),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK);

    let calls = stub.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].subject, "Kustom uji-br-kustom");
    assert!(calls[0].html.is_none(), "format plain tidak mengirim html");
    assert!(calls[0].text.contains("uji-br-kustom"));
    assert!(
        !calls[0].text.contains("diabaikan"),
        "isi selalu dari default"
    );

    purge(&pool).await;
    restore(&pool, &snap).await;
}
