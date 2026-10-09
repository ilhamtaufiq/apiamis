//! Rute publik tanpa login (`public_routes`) lewat router dan MySQL: form kelembagaan SPAM dan
//! hubungi kami.
//!
//! Tidak pernah mengirim email. Jalur kontak hanya diuji untuk honeypot, validasi, dan
//! 503 saat SMTP tidak aktif (`mail_enabled` bukan `1`). Test menolak jalan bila `mail_enabled = 1`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test public_routes_db -- --include-ignored
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

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

/// Kirim satu request tanpa token ke router. Mengembalikan status dan body JSON (atau `Null`).
async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
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

/// Unit uji dengan desa induk. Nama dan desa memakai awalan `uji-pr-{tag}-`.
async fn seed_unit(pool: &MySqlPool, tag: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_desa (n_desa, created_at, updated_at) VALUES (?, NOW(), NOW())")
        .bind(format!("uji-pr-{tag}-desa"))
        .execute(pool)
        .await
        .unwrap();
    let desa_id: u64 = sqlx::query_scalar("SELECT id FROM tbl_desa WHERE n_desa = ? LIMIT 1")
        .bind(format!("uji-pr-{tag}-desa"))
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tbl_unit_spam (name, desa_id, is_simspam, sumber_dana, created_at, updated_at) \
         VALUES (?, ?, 1, 'APBDes', NOW(), NOW())",
    )
    .bind(format!("uji-pr-{tag}-unit"))
    .bind(desa_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_unit_spam WHERE name = ? LIMIT 1")
        .bind(format!("uji-pr-{tag}-unit"))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Link berbagi untuk unit uji. `token` harus berawalan `uji-pr-{tag}-`.
async fn seed_link(
    pool: &MySqlPool,
    unit_id: u64,
    token: &str,
    is_active: bool,
    max_submissions: Option<i64>,
) -> u64 {
    sqlx::query(
        "INSERT INTO spam_kelembagaan_share_links \
         (unit_spam_id, token, label, is_active, max_submissions, submission_count, created_at, updated_at) \
         VALUES (?, ?, 'Uji PR', ?, ?, 0, NOW(), NOW())",
    )
    .bind(unit_id)
    .bind(token)
    .bind(is_active)
    .bind(max_submissions)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM spam_kelembagaan_share_links WHERE token = ? LIMIT 1")
        .bind(token)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus hanya baris milik `tag` ini: submisi, link, unit, lalu desa.
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let token_like = format!("uji-pr-{tag}-%");
    sqlx::query(
        "DELETE s FROM spam_kelembagaan_submissions s \
         JOIN spam_kelembagaan_share_links l ON l.id = s.share_link_id \
         WHERE l.token LIKE ?",
    )
    .bind(&token_like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM spam_kelembagaan_share_links WHERE token LIKE ?")
        .bind(&token_like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_unit_spam WHERE name = ?")
        .bind(format!("uji-pr-{tag}-unit"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa = ?")
        .bind(format!("uji-pr-{tag}-desa"))
        .execute(pool)
        .await
        .unwrap();
}

async fn submission_count_for_token(pool: &MySqlPool, token: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM spam_kelembagaan_submissions s \
         JOIN spam_kelembagaan_share_links l ON l.id = s.share_link_id WHERE l.token = ?",
    )
    .bind(token)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_form_unknown_token_is_not_found_on_show_and_submit() {
    let pool = pool().await;
    let uri = "/api/public/spam-kelembagaan/form/uji-pr-tidak-ada-token";

    // Token yang tidak ada di tabel adalah 404 (`firstOrFail`), bukan 410 atau 422.
    let (status, body) = send(&pool, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(json!({ "payload": { "sumber_dana": "APBDes" }, "submitter_name": "Uji PR" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_form_show_active_link_returns_unit_form_data() {
    let pool = pool().await;
    cleanup(&pool, "show").await;
    let unit_id = seed_unit(&pool, "show").await;
    seed_link(&pool, unit_id, "uji-pr-show-token", true, None).await;

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/public/spam-kelembagaan/form/uji-pr-show-token",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["link"]["token"], "uji-pr-show-token");
    assert_eq!(body["data"]["link"]["is_usable"], true);
    assert_eq!(body["data"]["unit"]["id"], json!(unit_id));
    assert_eq!(body["data"]["unit"]["name"], "uji-pr-show-unit");
    assert_eq!(body["data"]["unit"]["desa"], "uji-pr-show-desa");
    assert_eq!(body["data"]["unit"]["current"]["sumber_dana"], "APBDes");
    assert_eq!(
        body["data"]["fields"]["unit"].as_array().map(Vec::len),
        Some(12)
    );
    assert_eq!(
        body["data"]["fields"]["pengelola"].as_array().map(Vec::len),
        Some(5)
    );

    cleanup(&pool, "show").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_form_inactive_link_is_gone_on_show_and_rejected_on_submit() {
    let pool = pool().await;
    cleanup(&pool, "off").await;
    let unit_id = seed_unit(&pool, "off").await;
    let link_id = seed_link(&pool, unit_id, "uji-pr-off-token", false, None).await;
    let uri = "/api/public/spam-kelembagaan/form/uji-pr-off-token";

    // GET: link nonaktif memberi 410 dengan data form tetap dikembalikan.
    let (status, body) = send(&pool, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert_eq!(body["success"], false);
    assert_eq!(body["data"]["link"]["is_usable"], false);
    assert_eq!(body["data"]["unit"]["id"], json!(unit_id));

    // POST dengan payload yang valid tetap ditolak 422 pada kunci `token`.
    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(json!({ "payload": { "sumber_dana": "APBDes" }, "submitter_name": "Uji PR" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["token"].is_array(), "{body}");

    // Tidak ada submisi dan hitungan link tidak naik.
    assert_eq!(
        submission_count_for_token(&pool, "uji-pr-off-token").await,
        0
    );
    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(submission_count AS SIGNED) FROM spam_kelembagaan_share_links WHERE id = ?",
    )
    .bind(link_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);

    cleanup(&pool, "off").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_form_submit_stores_pending_submission_and_counts_it() {
    let pool = pool().await;
    cleanup(&pool, "kirim").await;
    let unit_id = seed_unit(&pool, "kirim").await;
    let link_id = seed_link(&pool, unit_id, "uji-pr-kirim-token", true, Some(5)).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/public/spam-kelembagaan/form/uji-pr-kirim-token",
        Some(json!({
            "payload": { "sumber_dana": "  Dana Desa  ", "lainnya": "dibuang" },
            "submitter_name": "Uji PR Pengirim",
            "submitter_phone": "0812",
            "pokmas": "Pokmas Uji",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["status"], "pending");
    let submission_id = body["data"]["id"].as_u64().expect("id submisi");

    let (status_col, payload, snapshot): (String, String, String) = sqlx::query_as(
        "SELECT status, CAST(payload AS CHAR), CAST(snapshot_before AS CHAR) \
         FROM spam_kelembagaan_submissions WHERE id = ?",
    )
    .bind(submission_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status_col, "pending");
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["sumber_dana"], "Dana Desa", "nilai di-trim");
    assert_eq!(payload["pokmas"], "Pokmas Uji");
    assert!(
        payload.get("lainnya").is_none(),
        "field di luar unit/pengelola dibuang"
    );
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(
        snapshot["sumber_dana"], "APBDes",
        "snapshot unit sebelum usulan"
    );

    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(submission_count AS SIGNED) FROM spam_kelembagaan_share_links WHERE id = ?",
    )
    .bind(link_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    cleanup(&pool, "kirim").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_form_submit_with_only_unknown_fields_is_empty_payload_422() {
    let pool = pool().await;
    cleanup(&pool, "kosong").await;
    let unit_id = seed_unit(&pool, "kosong").await;
    seed_link(&pool, unit_id, "uji-pr-kosong-token", true, None).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/public/spam-kelembagaan/form/uji-pr-kosong-token",
        Some(json!({ "payload": { "lainnya": "x" }, "submitter_name": "Uji PR" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["errors"]["payload"][0],
        "Tidak ada data yang diisi untuk diusulkan."
    );
    assert_eq!(
        submission_count_for_token(&pool, "uji-pr-kosong-token").await,
        0
    );

    cleanup(&pool, "kosong").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn contact_honeypot_is_success_without_validation_or_mail() {
    let pool = pool().await;
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/public/contact",
        Some(json!({ "website": "bot", "name": "", "email": "bukan-email" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "success");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn contact_invalid_input_is_422_before_any_mail() {
    let pool = pool().await;
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/public/contact",
        Some(json!({ "name": "", "email": "bukan-email", "subject": "Uji", "message": "Halo" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["name"].is_array(), "{body}");
    assert!(body["errors"]["email"].is_array(), "{body}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn contact_without_smtp_is_503_and_sends_nothing() {
    let pool = pool().await;

    // Pengaman: bila SMTP aktif di database ini, test berhenti agar tidak mengirim email sungguhan.
    let enabled: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT value FROM app_settings WHERE `key` = 'mail_enabled' LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .unwrap()
    .flatten();
    assert_ne!(
        enabled.as_deref(),
        Some("1"),
        "mail_enabled = 1 di database uji: test dihentikan agar tidak mengirim email"
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/public/contact",
        Some(json!({
            "name": "Uji PR",
            "email": "uji-pr-pengirim@example.test",
            "subject": "Uji",
            "message": "Pesan uji, tidak dikirim.",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["status"], "error");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|m| m.contains("Layanan email")),
        "{body}"
    );
}
