//! Rute admin `/api/spam-kelembagaan` lewat router terhadap MySQL: share link (daftar, buat, ubah, nonaktifkan),
//! usulan (daftar, detail, setujui, tolak), status guard (usulan yang sudah diproses tidak bisa diproses lagi),
//! dan validasi 422.
//!
//! Membutuhkan tabel `spam_kelembagaan_share_links` dan `spam_kelembagaan_submissions` (fixture
//! `rust/fixtures/spam_kelembagaan_schema.sql`), `tbl_unit_spam`, `tbl_desa`, `tbl_kecamatan`,
//! `tbl_pengelola`, `tbl_audit_logs`, `notifications`, `users`, `roles`, `model_has_roles`, dan `personal_access_tokens`.
//! Usulan dibuat langsung ke tabel karena form publik masih di Laravel. Setiap tes memakai tag sendiri
//! (`uji-sk-<tag>`), dan hanya menghapus baris dengan tag itu. Tag tidak boleh menjadi awalan tag lain.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test spam_kelembagaan_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    response::Response,
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

async fn connect() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn finish(res: Response) -> (StatusCode, Value) {
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Permintaan JSON (atau tanpa body bila `body` `None`). `token` `None` berarti tanpa header Authorization.
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
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(body).unwrap())
    .await
    .unwrap();
    finish(res).await
}

/// Hapus semua baris uji dengan tag ini: notifikasi, audit, usulan, link, pengelola, unit, desa, kecamatan,
/// lalu akun admin uji. Nama unit memakai awalan `uji-sk-<tag>` karena tes bisa mengganti nama unit.
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let prefix = format!("uji-sk-{tag}%");
    let admin_email = format!("uji-sk-{tag}-admin@example.test");
    let admin_name_like = format!("%uji-sk-{tag}-admin%");
    sqlx::query("DELETE FROM notifications WHERE data LIKE ?")
        .bind(&admin_name_like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_audit_logs WHERE user_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(&admin_email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM spam_kelembagaan_submissions WHERE unit_spam_id IN (SELECT id FROM tbl_unit_spam WHERE name LIKE ?)",
    )
    .bind(&prefix)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM spam_kelembagaan_share_links WHERE unit_spam_id IN (SELECT id FROM tbl_unit_spam WHERE name LIKE ?)",
    )
    .bind(&prefix)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_pengelola WHERE unit_spam_id IN (SELECT id FROM tbl_unit_spam WHERE name LIKE ?)")
        .bind(&prefix)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_unit_spam WHERE name LIKE ?")
        .bind(&prefix)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa LIKE ?")
        .bind(&prefix)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE ?")
        .bind(&prefix)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(&admin_email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(&admin_email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(&admin_email)
        .execute(pool)
        .await
        .unwrap();
}

/// Bersihkan sisa tag ini, lalu buat admin uji dengan peran `admin` dan token Sanctum.
async fn start(pool: &MySqlPool, tag: &str) -> (u64, String) {
    cleanup(pool, tag).await;
    let email = format!("uji-sk-{tag}-admin@example.test");
    let name = format!("uji-sk-{tag}-admin");
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(&name)
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-sk").await.unwrap();
    (uid, token)
}

/// Kecamatan, desa, dan unit SPAM uji dengan nama `uji-sk-<tag>`. Mengembalikan id unit.
async fn setup_unit(pool: &MySqlPool, tag: &str) -> u64 {
    let name = format!("uji-sk-{tag}");
    let kec = sqlx::query("INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())")
        .bind(&name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
    let desa = sqlx::query(
        "INSERT INTO tbl_desa (n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
    )
    .bind(&name)
    .bind(kec)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    sqlx::query(
        "INSERT INTO tbl_unit_spam (desa_id, name, sumber_dana, tahun_pembangunan, created_at, updated_at) \
         VALUES (?, ?, 'APBDes', '2019', NOW(), NOW())",
    )
    .bind(desa)
    .bind(&name)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

/// Buat share link lewat API dengan `unit_spam_id` saja. Mengembalikan (id, token).
async fn create_link(pool: &MySqlPool, token: &str, unit: u64) -> (u64, String) {
    let (status, body) = send(
        pool,
        Method::POST,
        "/api/spam-kelembagaan/share-links",
        Some(token),
        Some(json!({ "unit_spam_id": unit })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["data"]["id"].as_u64().unwrap(),
        body["data"]["token"].as_str().unwrap().to_string(),
    )
}

/// Usulan langsung ke tabel (form publik belum dipindah). Mengembalikan id usulan.
async fn insert_submission(
    pool: &MySqlPool,
    link_id: u64,
    unit: u64,
    payload: &Value,
    status: &str,
) -> u64 {
    sqlx::query(
        "INSERT INTO spam_kelembagaan_submissions (share_link_id, unit_spam_id, payload, submitter_name, \
         submitter_phone, submitter_instansi, submitter_note, status, created_at, updated_at) \
         VALUES (?, ?, ?, 'Uji SK Pengirim', '081200000000', 'Instansi Uji', 'catatan uji', ?, NOW(), NOW())",
    )
    .bind(link_id)
    .bind(unit)
    .bind(payload.to_string())
    .bind(status)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn db_status(pool: &MySqlPool, id: u64) -> String {
    sqlx::query_scalar("SELECT status FROM spam_kelembagaan_submissions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn db_unit_name(pool: &MySqlPool, unit: u64) -> String {
    sqlx::query_scalar("SELECT name FROM tbl_unit_spam WHERE id = ?")
        .bind(unit)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn pengelola_count(pool: &MySqlPool, unit: u64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pengelola WHERE unit_spam_id = ?")
        .bind(unit)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Setujui atau tolak usulan lewat API. `verb` adalah `approve` atau `reject`.
async fn decide(
    pool: &MySqlPool,
    token: &str,
    id: u64,
    verb: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        &format!("/api/spam-kelembagaan/submissions/{id}/{verb}"),
        Some(token),
        body,
    )
    .await
}

/// Pesan 422 Laravel: `message` dan `errors.<field>[0]` sama dengan pesan yang diharapkan.
fn assert_422(status: StatusCode, body: &Value, field: &str, message: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], message, "{body}");
    assert_eq!(body["errors"][field][0], message, "{body}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tanpa_token_ditolak_401() {
    let pool = connect().await;
    let cases: [(Method, &str); 8] = [
        (Method::GET, "/api/spam-kelembagaan/share-links"),
        (Method::POST, "/api/spam-kelembagaan/share-links"),
        (Method::PUT, "/api/spam-kelembagaan/share-links/1"),
        (Method::DELETE, "/api/spam-kelembagaan/share-links/1"),
        (Method::GET, "/api/spam-kelembagaan/submissions"),
        (Method::GET, "/api/spam-kelembagaan/submissions/1"),
        (Method::POST, "/api/spam-kelembagaan/submissions/1/approve"),
        (Method::POST, "/api/spam-kelembagaan/submissions/1/reject"),
    ];
    for (method, uri) in cases {
        let (status, body) = send(&pool, method.clone(), uri, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}: {body}");
        assert_eq!(body["message"], "Unauthenticated.", "{method} {uri}");
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn buat_link_daftar_ubah_dan_nonaktifkan() {
    let pool = connect().await;
    let tag = "link";
    let (admin_id, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;

    // Buat link dengan semua field opsional.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spam-kelembagaan/share-links",
        Some(token),
        Some(json!({
            "unit_spam_id": unit,
            "label": "uji-sk-link formulir",
            "expires_at": "2037-12-31T23:59:59Z",
            "max_submissions": 5,
            "admin_note": "catatan uji",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "Link form berhasil dibuat.");
    let data = &body["data"];
    let link_id = data["id"].as_u64().unwrap();
    let link_token = data["token"].as_str().unwrap().to_string();
    assert_eq!(link_token.len(), 48);
    assert!(
        link_token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "{link_token}"
    );
    assert_eq!(data["path"], format!("/kelembagaan-spam/form/{link_token}"));
    assert_eq!(data["is_active"], true);
    assert_eq!(data["is_usable"], true);
    assert_eq!(data["submission_count"], 0);
    assert_eq!(data["max_submissions"], 5);
    assert_eq!(data["label"], "uji-sk-link formulir");
    assert_eq!(data["admin_note"], "catatan uji");
    assert_eq!(data["unit_spam_id"], json!(unit));
    assert_eq!(data["unit"]["id"], json!(unit));
    assert_eq!(data["unit"]["name"], "uji-sk-link");
    assert_eq!(data["unit"]["desa"], "uji-sk-link");
    assert_eq!(data["unit"]["kecamatan"], "uji-sk-link");
    assert_eq!(data["creator"]["id"], json!(admin_id));
    assert!(data["expires_at"].is_string(), "{data}");
    let expires_at = data["expires_at"].clone();

    // Daftar per unit: satu link, meta paginator Laravel.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/share-links?unit_spam_id={unit}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["id"], json!(link_id));
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["meta"]["current_page"], 1);
    assert_eq!(body["meta"]["last_page"], 1);
    assert_eq!(body["meta"]["per_page"], 20);

    // Ubah label dan batas kiriman. Tanggal kedaluwarsa tidak dikirim, jadi tidak berubah.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/spam-kelembagaan/share-links/{link_id}"),
        Some(token),
        Some(json!({ "label": "uji-sk-link revisi", "max_submissions": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["label"], "uji-sk-link revisi");
    assert_eq!(body["data"]["max_submissions"], 1);
    assert_eq!(body["data"]["expires_at"], expires_at);
    assert_eq!(body["data"]["is_active"], true);

    // Nonaktifkan lewat `is_active`: tidak lagi bisa dipakai.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/spam-kelembagaan/share-links/{link_id}"),
        Some(token),
        Some(json!({ "is_active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["is_active"], false);
    assert_eq!(body["data"]["is_usable"], false);

    // Hapus hanya menonaktifkan; baris tetap ada.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spam-kelembagaan/share-links/{link_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "Link form dinonaktifkan.");
    let still: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM spam_kelembagaan_share_links WHERE id = ? AND is_active = 0",
    )
    .bind(link_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(still, 1);

    // Filter `is_active` memisahkan link aktif dan nonaktif.
    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/share-links?unit_spam_id={unit}&is_active=1"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(body["meta"]["total"], 0);
    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/share-links?unit_spam_id={unit}&is_active=0"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(body["meta"]["total"], 1);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_link_422() {
    let pool = connect().await;
    let tag = "valid";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let uri = "/api/spam-kelembagaan/share-links";

    let (status, body) = send(&pool, Method::POST, uri, Some(token), Some(json!({}))).await;
    assert_422(status, &body, "unit_spam_id", "The unit spam id field is required.");

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": 999_999_999 })),
    )
    .await;
    assert_422(status, &body, "unit_spam_id", "The selected unit spam id is invalid.");

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": unit, "expires_at": "2000-01-01 00:00:00" })),
    )
    .await;
    assert_422(
        status,
        &body,
        "expires_at",
        "The expires at field must be a date after now.",
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": unit, "expires_at": "bukan tanggal" })),
    )
    .await;
    assert_422(status, &body, "expires_at", "The expires at field must be a valid date.");

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": unit, "max_submissions": 0 })),
    )
    .await;
    assert_422(
        status,
        &body,
        "max_submissions",
        "The max submissions field must be at least 1.",
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": unit, "max_submissions": 1001 })),
    )
    .await;
    assert_422(
        status,
        &body,
        "max_submissions",
        "The max submissions field must not be greater than 1000.",
    );

    let panjang = "a".repeat(256);
    let (status, body) = send(
        &pool,
        Method::POST,
        uri,
        Some(token),
        Some(json!({ "unit_spam_id": unit, "label": panjang })),
    )
    .await;
    assert_422(
        status,
        &body,
        "label",
        "The label field must not be greater than 255 characters.",
    );

    // Validasi PUT: nilai yang salah tidak mengubah link yang sudah ada.
    let (link_id, _) = create_link(&pool, token, unit).await;
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("{uri}/{link_id}"),
        Some(token),
        Some(json!({ "max_submissions": 0 })),
    )
    .await;
    assert_422(
        status,
        &body,
        "max_submissions",
        "The max submissions field must be at least 1.",
    );
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("{uri}/{link_id}"),
        Some(token),
        Some(json!({ "is_active": "mungkin" })),
    )
    .await;
    assert_422(status, &body, "is_active", "The is active field must be true or false.");

    // Hanya link yang valid yang tersimpan.
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM spam_kelembagaan_share_links WHERE unit_spam_id = ?")
        .bind(unit)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 1);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn daftar_usulan_dan_detail() {
    let pool = connect().await;
    let tag = "daftar";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, link_token) = create_link(&pool, token, unit).await;

    let payload = json!({ "name": "uji-sk-daftar baru", "kepala": "Kepala Uji" });
    let pending = insert_submission(&pool, link_id, unit, &payload, "pending").await;
    let approved = insert_submission(&pool, link_id, unit, &payload, "approved").await;

    // Daftar pending untuk unit ini: satu usulan, dengan detail link dan unit.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/submissions?status=pending&unit_spam_id={unit}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    let row = &body["data"][0];
    assert_eq!(row["id"], json!(pending));
    assert_eq!(row["status"], "pending");
    assert_eq!(row["payload"]["name"], "uji-sk-daftar baru");
    assert_eq!(row["share_link"]["id"], json!(link_id));
    assert_eq!(row["share_link"]["token"], link_token);
    assert_eq!(row["unit"]["name"], "uji-sk-daftar");
    assert_eq!(row["reviewer"], Value::Null);
    assert_eq!(row["reviewed_at"], Value::Null);
    assert!(body["meta"]["pending_count"].as_i64().unwrap() >= 1);

    // Filter status approved hanya memuat baris yang disetujui.
    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/submissions?status=approved&unit_spam_id={unit}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], json!(approved));

    // Detail: payload berupa objek JSON.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-kelembagaan/submissions/{pending}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["payload"]["kepala"], "Kepala Uji");
    assert_eq!(body["data"]["submitter_name"], "Uji SK Pengirim");
    assert_eq!(body["data"]["share_link"]["token"], link_token);

    // Usulan tidak ada: 404 dengan pesan model Laravel.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/spam-kelembagaan/submissions/999999999",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["message"],
        "No query results for model [App\\Models\\SpamKelembagaanSubmission] 999999999"
    );

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn setujui_usulan_memperbarui_unit_dan_pengelola() {
    let pool = connect().await;
    let tag = "setuju";
    let (admin_id, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({
        "name": "uji-sk-setuju baru",
        "sumber_dana": "DAK",
        "kepala": "Kepala uji-sk-setuju",
        "bendahara": "Bendahara uji-sk-setuju",
    });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    let (status, body) = decide(
        &pool,
        token,
        sub,
        "approve",
        Some(json!({ "review_note": "disetujui uji" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "Usulan disetujui dan data unit SPAM diperbarui.");
    assert_eq!(body["data"]["status"], "approved");
    assert_eq!(body["data"]["review_note"], "disetujui uji");
    assert_eq!(body["data"]["reviewer"]["id"], json!(admin_id));
    assert!(body["data"]["reviewed_at"].is_string(), "{body}");
    assert_eq!(body["data"]["unit"]["name"], "uji-sk-setuju baru");

    // Kolom unit yang dikirim berubah; kolom lain tetap.
    assert_eq!(db_status(&pool, sub).await, "approved");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-setuju baru");
    let sumber: Option<String> = sqlx::query_scalar("SELECT sumber_dana FROM tbl_unit_spam WHERE id = ?")
        .bind(unit)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sumber.as_deref(), Some("DAK"));
    let tahun: Option<String> = sqlx::query_scalar("SELECT tahun_pembangunan FROM tbl_unit_spam WHERE id = ?")
        .bind(unit)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tahun.as_deref(), Some("2019"));

    // Pengelola belum ada, jadi dibuat satu baris dengan kolom yang dikirim.
    assert_eq!(pengelola_count(&pool, unit).await, 1);
    let (kepala, bendahara, pokmas): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT kepala, bendahara, pokmas FROM tbl_pengelola WHERE unit_spam_id = ?",
    )
    .bind(unit)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kepala.as_deref(), Some("Kepala uji-sk-setuju"));
    assert_eq!(bendahara.as_deref(), Some("Bendahara uji-sk-setuju"));
    assert_eq!(pokmas, None);

    // Perubahan tercatat di audit: unit diperbarui dan pengelola dibuat.
    let audit_unit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_audit_logs WHERE user_id = ? AND event = 'updated' AND auditable_id = ?",
    )
    .bind(admin_id)
    .bind(unit)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audit_unit >= 1, "audit updated unit harus ada");
    let audit_peng: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_audit_logs WHERE user_id = ? AND event = 'created' AND auditable_type = 'App\\\\Models\\\\Pengelola'",
    )
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audit_peng >= 1, "audit created pengelola harus ada");

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tolak_usulan_tanpa_mengubah_unit() {
    let pool = connect().await;
    let tag = "tolak";
    let (admin_id, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({ "name": "uji-sk-tolak baru", "kepala": "Kepala Tolak" });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    let (status, body) = decide(
        &pool,
        token,
        sub,
        "reject",
        Some(json!({ "review_note": "data tidak sesuai" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Usulan ditolak.");
    assert_eq!(body["data"]["status"], "rejected");
    assert_eq!(body["data"]["review_note"], "data tidak sesuai");
    assert_eq!(body["data"]["reviewer"]["id"], json!(admin_id));

    // Unit dan pengelola tidak disentuh.
    assert_eq!(db_status(&pool, sub).await, "rejected");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-tolak");
    assert_eq!(pengelola_count(&pool, unit).await, 0);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn status_guard_tidak_menerapkan_usulan_dua_kali() {
    let pool = connect().await;
    let tag = "guard";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({ "name": "uji-sk-guard pertama", "kepala": "Kepala Pertama" });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    let (status, body) = decide(&pool, token, sub, "approve", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Setujui lagi: ditolak dengan pesan status, tanpa menerapkan ulang.
    let (status, body) = decide(&pool, token, sub, "approve", None).await;
    assert_422(
        status,
        &body,
        "status",
        "Usulan ini sudah diproses sebelumnya.",
    );

    // Tolak setelah disetujui: juga ditolak.
    let (status, body) = decide(&pool, token, sub, "reject", None).await;
    assert_422(
        status,
        &body,
        "status",
        "Usulan ini sudah diproses sebelumnya.",
    );

    assert_eq!(db_status(&pool, sub).await, "approved");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-guard pertama");
    assert_eq!(pengelola_count(&pool, unit).await, 1);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn urutan_tolak_lalu_setujui_ditolak() {
    let pool = connect().await;
    let tag = "urutan";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({ "name": "uji-sk-urutan baru", "kepala": "Kepala Urutan" });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    let (status, body) = decide(&pool, token, sub, "reject", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = decide(&pool, token, sub, "approve", None).await;
    assert_422(
        status,
        &body,
        "status",
        "Usulan ini sudah diproses sebelumnya.",
    );

    assert_eq!(db_status(&pool, sub).await, "rejected");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-urutan");
    assert_eq!(pengelola_count(&pool, unit).await, 0);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn setujui_bersamaan_hanya_satu_berhasil() {
    let pool = connect().await;
    let tag = "serentak";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({ "name": "uji-sk-serentak baru", "kepala": "Kepala Serentak" });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    // Dua persetujuan dikirim bersamaan: penjaga `status = 'pending'` hanya meloloskan satu.
    let (a, b) = tokio::join!(
        decide(&pool, token, sub, "approve", None),
        decide(&pool, token, sub, "approve", None),
    );
    let mut codes = vec![a.0.as_u16(), b.0.as_u16()];
    codes.sort_unstable();
    assert_eq!(codes, vec![200u16, 422u16], "{:?} / {:?}", a.1, b.1);

    assert_eq!(db_status(&pool, sub).await, "approved");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-serentak baru");
    assert_eq!(pengelola_count(&pool, unit).await, 1);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_catatan_keputusan_422() {
    let pool = connect().await;
    let tag = "catatan";
    let (_, token) = start(&pool, tag).await;
    let token = token.as_str();
    let unit = setup_unit(&pool, tag).await;
    let (link_id, _) = create_link(&pool, token, unit).await;
    let payload = json!({ "name": "uji-sk-catatan baru", "kepala": "Kepala Catatan" });
    let sub = insert_submission(&pool, link_id, unit, &payload, "pending").await;

    let panjang = "c".repeat(2001);
    let note = json!({ "review_note": panjang });
    let (status, body) = decide(&pool, token, sub, "approve", Some(note.clone())).await;
    assert_422(
        status,
        &body,
        "review_note",
        "The review note field must not be greater than 2000 characters.",
    );
    let (status, body) = decide(&pool, token, sub, "reject", Some(note)).await;
    assert_422(
        status,
        &body,
        "review_note",
        "The review note field must not be greater than 2000 characters.",
    );

    // Validasi gagal sebelum ada perubahan: usulan tetap pending dan unit tidak berubah.
    assert_eq!(db_status(&pool, sub).await, "pending");
    assert_eq!(db_unit_name(&pool, unit).await, "uji-sk-catatan");
    assert_eq!(pengelola_count(&pool, unit).await, 0);

    cleanup(&pool, tag).await;
}
