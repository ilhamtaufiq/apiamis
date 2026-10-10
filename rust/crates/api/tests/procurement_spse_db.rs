//! Procurement SPSE lewat router terhadap MySQL dan stub SPSE lokal (`127.0.0.1`).
//!
//! Stub melayani slug `uji-ok` (valid), `uji-mati` (401), dan `uji-login` (halaman login).
//! `SPSE_BASE_URL` diarahkan ke stub, sehingga tidak ada permintaan keluar.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test procurement_spse_db -- --include-ignored
//! ```

use std::{
    collections::HashMap,
    io::{Cursor, Read},
    sync::OnceLock,
};

use api::{app, crypt, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode, Uri},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const APP_KEY: &str = "0123456789abcdef0123456789abcdef";
const COOKIE_JSON: &str = r#"[{"name":"SPSE_SESSION","value":"sesi-uji___AT=tok-uji","domain":"spse.inaproc.id","path":"/"}]"#;

// ---------------------------------------------------------------------------
// Stub SPSE
// ---------------------------------------------------------------------------

static SPSE: OnceLock<String> = OnceLock::new();

/// Baris DataTables `paket-ppk-pl`: pencocokan kode, nama, tanpa kode, dan status batal.
fn pl_rows() -> Vec<Value> {
    vec![
        json!(["UJI-PS-KODE-1", "<b>Pengadaan</b> Jalan &amp; Drainase Uji", "Selesai", "x", "y", "Pengadaan Langsung"]),
        json!(["UJI-PS-NAMA-2", "UJI PS sync Rehabilitasi Irigasi Zqw", "Selesai", "x", "y", "Pengadaan Langsung"]),
        json!(["UJI-PS-KODE-3", "UJI PS sync Tidak Ada Padanan Qxv", "Batal", null, "", "Pengadaan Langsung"]),
        json!(["", "tanpa kode", "x", "x", "x", "x"]),
    ]
}

fn form_field(form: &str, key: &str) -> Option<String> {
    form.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

fn reply(status: u16, ctype: &str, body: Vec<u8>, disposition: Option<&str>) -> Response {
    let mut b = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ctype);
    if let Some(d) = disposition {
        b = b.header(header::CONTENT_DISPOSITION, d);
    }
    b.body(Body::from(body)).unwrap()
}

fn datatable(form: &str, rows: &[Value]) -> Response {
    if form_field(form, "authenticityToken").as_deref() != Some("tok-uji") {
        return reply(401, "text/plain", b"token".to_vec(), None);
    }
    let start: usize = form_field(form, "start").and_then(|v| v.parse().ok()).unwrap_or(0);
    let length: usize = form_field(form, "length").and_then(|v| v.parse().ok()).unwrap_or(10);
    let draw = form_field(form, "draw").unwrap_or_else(|| "1".into());
    let page: Vec<Value> = rows
        .iter()
        .skip(start)
        .take(length)
        .cloned()
        .collect();
    let body = json!({
        "draw": draw,
        "recordsTotal": rows.len(),
        "recordsFiltered": rows.len(),
        "data": page,
    });
    reply(200, "application/json", body.to_string().into_bytes(), None)
}

async fn stub(method: Method, uri: Uri, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let path = uri.path().to_string();
    let trimmed = path.trim_start_matches('/');
    let (slug, rest) = match trimmed.split_once('/') {
        Some((s, r)) => (s.to_string(), format!("/{r}")),
        None => (trimmed.to_string(), "/".to_string()),
    };
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let form = String::from_utf8_lossy(&body).into_owned();
    let m = method.as_str();

    match (slug.as_str(), m, rest.as_str()) {
        ("uji-mati", _, _) => reply(401, "text/plain", b"denied".to_vec(), None),
        ("uji-login", _, "/home") => reply(
            200,
            "text/html",
            b"<html><body><form id=\"loginCtr\"></form></body></html>".to_vec(),
            None,
        ),
        ("uji-ok", _, "/home") if cookie.contains("SPSE_SESSION=") => reply(
            200,
            "text/html",
            b"<html><body>beranda</body></html>".to_vec(),
            None,
        ),
        ("uji-ok", "POST", "/dt/paket-ppk-pl") => datatable(&form, &pl_rows()),
        ("uji-ok", "POST", "/dt/paket-ppk") => datatable(&form, &[]),
        ("uji-ok", "GET", r) if r.starts_with("/nontender/") && r.matches('/').count() == 2 => reply(
            200,
            "text/html",
            b"<html><body><a href=\"/uji-ok/viewpdfpl/77\">Summary Uji</a> \
              <a href=\"/uji-ok/dl/rab-uji.pdf\">RAB Uji</a> \
              <a href=\"/uji-ok/dl/rusak.pdf\">Rusak</a></body></html>"
                .to_vec(),
            None,
        ),
        ("uji-ok", "GET", "/viewpdfpl/77") => {
            reply(200, "application/pdf", b"%PDF-1.4 summary uji".to_vec(), None)
        }
        ("uji-ok", "GET", "/dl/rab-uji.pdf") => reply(
            200,
            "application/pdf",
            b"%PDF-1.4 rab uji isi".to_vec(),
            Some("attachment; filename=\"RAB Uji.pdf\""),
        ),
        _ => reply(404, "text/plain", b"tidak ditemukan".to_vec(), None),
    }
}

/// Menyalakan stub di thread sendiri dan mengatur env. Dipanggil di awal setiap test.
fn spse_base() -> &'static str {
    SPSE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("runtime stub");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind stub");
                tx.send(listener.local_addr().expect("alamat stub")).expect("kirim alamat");
                let router = Router::new().fallback(stub);
                axum::serve(listener, router).await.expect("serve stub");
            });
        });
        let addr: std::net::SocketAddr = rx.recv().expect("alamat stub");
        let base = format!("http://{addr}");
        std::env::set_var("SPSE_BASE_URL", &base);
        std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
        std::env::set_var("no_proxy", "127.0.0.1,localhost");
        std::env::set_var("APP_KEY", APP_KEY);
        let storage = std::env::temp_dir().join(format!("uji-procurement-{}", std::process::id()));
        std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
        base
    })
}

// ---------------------------------------------------------------------------
// Helper HTTP dan data
// ---------------------------------------------------------------------------

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn raw(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
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
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, headers, bytes.to_vec())
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, bytes) = raw(pool, method, uri, token, body).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn email(tag: &str, role: &str) -> String {
    format!("uji-procurement-{tag}-{role}@example.test")
}

async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Procurement', ?, 'x', NOW(), NOW())")
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
    uid
}

/// Sesi SPSE langsung di DB (POST `session` hanya untuk admin, seperti Laravel).
async fn insert_session(pool: &MySqlPool, user_id: u64, slug: &str, cookies_json: &str) {
    let key = crypt::key_from_app_key(APP_KEY).unwrap();
    let payload = crypt::encrypt_string(&key, cookies_json);
    sqlx::query(
        "INSERT INTO tbl_spse_sessions (user_id, encrypted_cookies, lpse_slug, expires_at, last_validated_at, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, DATE_ADD(NOW(), INTERVAL 8 HOUR), NOW(), 1, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(payload)
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
}

/// Hapus data uji milik satu test: user berawalan `tag`, kontrak `kode_like`, dan pekerjaan `nama_like`.
async fn cleanup(pool: &MySqlPool, tag: &str, kode_like: &str, nama_like: &[&str]) {
    let user_pattern = format!("uji-procurement-{tag}-%");
    let users: Vec<u64> = sqlx::query_scalar("SELECT id FROM users WHERE email LIKE ?")
        .bind(&user_pattern)
        .fetch_all(pool)
        .await
        .unwrap();

    let mut pek_ids: Vec<i64> = Vec::new();
    for pat in nama_like {
        let ids: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_pekerjaan WHERE nama_paket LIKE ?")
            .bind(*pat)
            .fetch_all(pool)
            .await
            .unwrap();
        pek_ids.extend(ids);
    }
    let kontrak_ids: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_kontrak WHERE kode_paket LIKE ?")
        .bind(kode_like)
        .fetch_all(pool)
        .await
        .unwrap();

    for u in &users {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE user_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_spse_sessions WHERE user_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_procurement_sync_runs WHERE user_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id = ?").bind(u).execute(pool).await.unwrap();
    }
    for k in &kontrak_ids {
        sqlx::query("DELETE FROM kontrak_pekerjaan WHERE kontrak_id = ?").bind(k).execute(pool).await.unwrap();
    }
    for p in &pek_ids {
        sqlx::query("DELETE FROM kontrak_pekerjaan WHERE pekerjaan_id = ?").bind(p).execute(pool).await.unwrap();
        let berkas: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_berkas WHERE pekerjaan_id = ?")
            .bind(p)
            .fetch_all(pool)
            .await
            .unwrap();
        for b in &berkas {
            sqlx::query("DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Berkas' AND model_id = ?")
                .bind(b)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM tbl_berkas WHERE pekerjaan_id = ?").bind(p).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_draft_pekerjaan WHERE pekerjaan_id = ?").bind(p).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ?")
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?").bind(p).execute(pool).await.unwrap();
    }
    for k in &kontrak_ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ?")
            .bind(k)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?").bind(k).execute(pool).await.unwrap();
    }
    for u in &users {
        sqlx::query("DELETE FROM users WHERE id = ?").bind(u).execute(pool).await.unwrap();
    }
}

async fn insert_pekerjaan(pool: &MySqlPool, nama: &str) -> i64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, pagu, is_konsultan, created_at, updated_at) VALUES (?, 0, 1, NOW(), NOW())")
        .bind(nama)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id() as i64
}

async fn insert_kontrak(pool: &MySqlPool, kode: &str, pekerjaan_id: Option<i64>) -> i64 {
    let id = sqlx::query("INSERT INTO tbl_kontrak (id_pekerjaan, kode_paket, spk, created_at, updated_at) VALUES (?, ?, NULL, NOW(), NOW())")
        .bind(pekerjaan_id)
        .bind(kode)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id() as i64;
    if let Some(p) = pekerjaan_id {
        sqlx::query("INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(id)
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
    }
    id
}

async fn insert_run(pool: &MySqlPool, user_id: u64) -> i64 {
    sqlx::query("INSERT INTO tbl_procurement_sync_runs (user_id, status, item_count, matched_count, started_at) VALUES (?, 'completed', 0, 0, NOW())")
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id() as i64
}

async fn insert_staging(
    pool: &MySqlPool,
    run_id: i64,
    kode: &str,
    nama: &str,
    raw_row: Option<&str>,
) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_procurement_staging_paket (sync_run_id, sumber, jenis_paket, kode_paket, nama_paket, raw_row, fetched_at, match_status) \
         VALUES (?, 'spse', 'pengadaan_langsung', ?, ?, ?, NOW(), 'unmatched')",
    )
    .bind(run_id)
    .bind(kode)
    .bind(nama)
    .bind(raw_row)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64
}

// ---------------------------------------------------------------------------
// Tes
// ---------------------------------------------------------------------------

fn db_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn status_tanpa_sesi_dan_sesi_kedaluwarsa() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "status", "UJI-PS-STATUS%", &[]).await;

    let user = make_user(&pool, &email("status", "user"), false).await;
    let token = auth::login::create_token(&pool, user, "uji-procurement").await.unwrap();

    let (status, body) = send(&pool, Method::GET, "/api/procurement/spse/status", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connected"], false);
    assert_eq!(body["message"], "Belum ada session SPSE. Login manual di SPSE lalu kirim cookie.");

    // Sesi yang sudah tidak diterima SPSE: status menonaktifkannya.
    insert_session(&pool, user, "uji-mati", COOKIE_JSON).await;
    let (status, body) = send(&pool, Method::GET, "/api/procurement/spse/status", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connected"], false);
    assert_eq!(body["message"], "Session SPSE expired. Login ulang di SPSE.");
    assert!(body["expired_at"].is_string());
    let active: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spse_sessions WHERE user_id = ? AND is_active = 1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(active, 0, "sesi yang expired harus nonaktif");

    cleanup(&pool, "status", "UJI-PS-STATUS%", &[]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn simpan_validasi_session_dan_enkripsi() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "simpan", "UJI-PS-SIMPAN%", &[]).await;

    let admin = make_user(&pool, &email("simpan", "admin"), true).await;
    let token = auth::login::create_token(&pool, admin, "uji-procurement").await.unwrap();
    let uri = "/api/procurement/spse/session";

    let (s, b) = send(&pool, Method::POST, uri, Some(&token), Some(json!({}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["message"], "cookie_header atau cookies wajib diisi.");

    let (s, b) = send(&pool, Method::POST, uri, Some(&token), Some(json!({ "cookie_header": "a=1" }))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["message"], "Cookie SPSE_SESSION wajib ada. Pastikan sudah login ke SPSE.");

    let (s, b) = send(
        &pool,
        Method::POST,
        uri,
        Some(&token),
        Some(json!({ "cookie_header": "SPSE_SESSION=x", "lpse_slug": "uji-mati" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(b["message"], "Session SPSE tidak valid. SPSE menolak cookie (HTTP 401).");

    let (s, b) = send(
        &pool,
        Method::POST,
        uri,
        Some(&token),
        Some(json!({ "cookie_header": "SPSE_SESSION=x", "lpse_slug": "uji-login" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["message"].as_str().unwrap().contains("diarahkan ke halaman login"));

    // Header dari DevTools, dengan prefix `Cookie:`.
    let (s, b) = send(
        &pool,
        Method::POST,
        uri,
        Some(&token),
        Some(json!({
            "cookie_header": "Cookie: SPSE_SESSION=sesi-uji___AT=tok-uji; uji_pref=1",
            "lpse_slug": "uji-ok",
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["message"], "Session SPSE tersimpan.");
    assert_eq!(b["session"]["lpse_slug"], "uji-ok");

    // Cookie tersimpan terenkripsi dan bisa didekripsi dengan APP_KEY.
    let payload: String = sqlx::query_scalar("SELECT encrypted_cookies FROM tbl_spse_sessions WHERE user_id = ? AND is_active = 1")
        .bind(admin)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!payload.contains("SPSE_SESSION"), "cookie tidak boleh tersimpan polos");
    let key = crypt::key_from_app_key(APP_KEY).unwrap();
    let plain = crypt::decrypt_string(&key, &payload).unwrap();
    let cookies: Value = serde_json::from_str(&plain).unwrap();
    assert_eq!(cookies[0]["name"], "SPSE_SESSION");
    assert_eq!(cookies[1]["name"], "uji_pref");

    let (s, b) = send(&pool, Method::GET, "/api/procurement/spse/status", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["connected"], true);
    assert_eq!(b["lpse_slug"], "uji-ok");

    // Cookie terstruktur menggantikan sesi aktif yang lama.
    let (s, _) = send(
        &pool,
        Method::POST,
        uri,
        Some(&token),
        Some(json!({ "cookies": [{ "name": "SPSE_SESSION", "value": "sesi-uji___AT=tok-uji" }], "lpse_slug": "uji-ok" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let aktif: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spse_sessions WHERE user_id = ? AND is_active = 1")
        .bind(admin)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(aktif, 1);

    let (s, b) = send(&pool, Method::DELETE, uri, Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["message"], "Session SPSE dihapus.");
    let (_, b) = send(&pool, Method::GET, "/api/procurement/spse/status", Some(&token), None).await;
    assert_eq!(b["connected"], false);

    cleanup(&pool, "simpan", "UJI-PS-SIMPAN%", &[]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sinkron_halaman_staging_dan_pencocokan() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "sync", "UJI-PS-KODE%", &["UJI PS sync%"]).await;

    let admin = make_user(&pool, &email("sync", "admin"), true).await;
    let lain = make_user(&pool, &email("sync", "lain"), false).await;
    let token = auth::login::create_token(&pool, admin, "uji-procurement").await.unwrap();
    let token_lain = auth::login::create_token(&pool, lain, "uji-procurement").await.unwrap();
    insert_session(&pool, admin, "uji-ok", COOKIE_JSON).await;

    // Pekerjaan untuk pencocokan nama (persis) dan kontrak untuk pencocokan kode.
    let pek_nama = insert_pekerjaan(&pool, "UJI PS sync Rehabilitasi Irigasi Zqw").await;
    let pek_kode = insert_pekerjaan(&pool, "UJI PS sync Lain").await;
    let kontrak = insert_kontrak(&pool, "UJI-PS-KODE-1", Some(pek_kode)).await;

    // Halaman dibatasi 2 baris: tiga halaman DataTables sampai total terpenuhi.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/sync",
        Some(&token),
        Some(json!({ "page_length": 2 })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["message"], "Sync SPSE selesai.");
    let run = &b["run"];
    assert_eq!(run["status"], "completed", "{run}");
    assert_eq!(run["item_count"], 3, "baris tanpa kode dilewati");
    assert_eq!(run["matched_count"], 2);
    assert!(run["error_log"].is_null());
    let run_id = run["id"].as_i64().unwrap();

    let (s, b) = send(&pool, Method::GET, "/api/procurement/spse/sync/runs", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["data"][0]["id"], run_id);

    let (s, b) = send(
        &pool,
        Method::GET,
        &format!("/api/procurement/spse/staging?sync_run_id={run_id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["meta"]["total"], 3);
    assert_eq!(b["meta"]["per_page"], 20);
    let items = b["data"].as_array().unwrap();
    let by_kode: HashMap<String, Value> = items
        .iter()
        .map(|i| (i["kode_paket"].as_str().unwrap().to_string(), i.clone()))
        .collect();
    let a = &by_kode["UJI-PS-KODE-1"];
    assert_eq!(a["match_status"], "exact_kode_paket");
    assert_eq!(a["matched_kontrak_id"], kontrak);
    assert_eq!(a["matched_pekerjaan_id"], pek_kode);
    assert_eq!(a["nama_paket"], "Pengadaan Jalan & Drainase Uji");
    assert_eq!(a["pekerjaan"]["id"], pek_kode);
    assert_eq!(a["kontrak"]["spk"], Value::Null);
    let n = &by_kode["UJI-PS-NAMA-2"];
    assert_eq!(n["match_status"], "fuzzy_nama_paket");
    assert_eq!(n["matched_pekerjaan_id"], pek_nama);
    assert!(n["matched_kontrak_id"].is_null());
    let c = &by_kode["UJI-PS-KODE-3"];
    assert_eq!(c["match_status"], "unmatched");
    assert!(c["pekerjaan"].is_null());
    assert_eq!(c["status_paket"], "Batal");

    let (s, b) = send(
        &pool,
        Method::GET,
        &format!("/api/procurement/spse/staging?sync_run_id={run_id}&match_status=unmatched"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["meta"]["total"], 1);
    assert_eq!(b["data"][0]["kode_paket"], "UJI-PS-KODE-3");

    // Filter tahun: baris belum tercocokkan selalu ikut.
    let (_, b) = send(
        &pool,
        Method::GET,
        &format!("/api/procurement/spse/staging?sync_run_id={run_id}&tahun=2099"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(b["meta"]["total"], 1);

    // Detail: URL SPSE dibentuk dari base dan slug default.
    let id_a = a["id"].as_i64().unwrap();
    let (s, b) = send(&pool, Method::GET, &format!("/api/procurement/spse/staging/{id_a}"), Some(&token), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let data = &b["data"];
    assert!(data["spse_url"].as_str().unwrap().ends_with("/nontender/UJI-PS-KODE-1"), "{data}");
    assert!(data["spse_url"].as_str().unwrap().starts_with(spse_base()));
    assert_eq!(data["pekerjaan"]["id"], pek_kode);
    assert_eq!(data["kontrak"]["id"], kontrak);
    assert!(data["sync_run"]["error_log"].is_null());
    assert_eq!(data["sync_run"]["status"], "completed");

    // Pemilik lain dan id tidak valid: 404.
    let (s, _) = send(&pool, Method::GET, &format!("/api/procurement/spse/staging/{id_a}"), Some(&token_lain), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(&pool, Method::GET, "/api/procurement/spse/staging/abc", Some(&token), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Milik user lain tidak tampil di daftar.
    let (_, b) = send(&pool, Method::GET, "/api/procurement/spse/staging", Some(&token_lain), None).await;
    assert_eq!(b["meta"]["total"], 0);

    cleanup(&pool, "sync", "UJI-PS-KODE%", &["UJI PS sync%"]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn map_apply_dan_promosi_draft() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    let kode_like = "UJI-PRM-%";
    let nama_like = ["UJI PRM apply%", "UJI PRM map%", "Paket SPSE UJI-PRM-PROMO%"];
    cleanup(&pool, "promo", kode_like, &nama_like).await;

    let admin = make_user(&pool, &email("promo", "admin"), true).await;
    let token = auth::login::create_token(&pool, admin, "uji-procurement").await.unwrap();
    let run = insert_run(&pool, admin).await;

    // S1: sudah tercocokkan ke kontrak tanpa kode paket. S2: belum tercocokkan.
    // S3: belum tercocokkan, dilewati saat apply. S4: draft baru, dengan kode RUP dari raw_row.
    let pek_apply = insert_pekerjaan(&pool, "UJI PRM apply Jalan").await;
    let kontrak_apply = insert_kontrak(&pool, "", Some(pek_apply)).await;
    let s1 = insert_staging(&pool, run, "UJI-PRM-APPLY-1", "UJI PRM apply Jalan", None).await;
    sqlx::query("UPDATE tbl_procurement_staging_paket SET matched_pekerjaan_id = ?, matched_kontrak_id = ?, match_status = 'exact_kode_paket' WHERE id = ?")
        .bind(pek_apply)
        .bind(kontrak_apply)
        .bind(s1)
        .execute(&pool)
        .await
        .unwrap();
    let s2 = insert_staging(&pool, run, "UJI-PRM-MAP-3", "UJI PRM map Gedung", None).await;
    let s3 = insert_staging(&pool, run, "UJI-PRM-APPLY-9", "UJI PRM apply Tanpa", None).await;
    let s4 = insert_staging(
        &pool,
        run,
        "UJI-PRM-PROMO-4",
        "",
        Some(r#"{"kode_rup":"RUP-UJI-4"}"#),
    )
    .await;
    let pek_map = insert_pekerjaan(&pool, "UJI PRM map Gedung").await;
    let kontrak_map = insert_kontrak(&pool, "UJI-PRM-MAP-K3", Some(pek_map)).await;

    // Mapping manual.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/map",
        Some(&token),
        Some(json!({ "id": s2, "pekerjaan_id": pek_map })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["message"], "Mapping manual tersimpan.");
    assert_eq!(b["staging"]["match_status"], "manual_map");
    assert_eq!(b["staging"]["matched_kontrak_id"], kontrak_map);
    assert_eq!(b["staging"]["pekerjaan"]["id"], pek_map);

    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/map",
        Some(&token),
        Some(json!({ "id": s2 })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["message"], "The pekerjaan id field is required.");

    // Apply: S1 mengisi kode paket kontrak yang kosong; S3 dilewati karena belum tercocokkan.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/apply",
        Some(&token),
        Some(json!({ "ids": [s1, s3], "overwrite": false })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["applied"], 1);
    assert_eq!(b["skipped"], 1);
    let results = b["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "applied");
    assert_eq!(results[0]["kode_paket"], "UJI-PRM-APPLY-1");
    assert_eq!(results[1]["reason"], "unmatched");

    let kode: String = sqlx::query_scalar("SELECT kode_paket FROM tbl_kontrak WHERE id = ?")
        .bind(kontrak_apply)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kode, "UJI-PRM-APPLY-1");
    let audit: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ? AND event = 'updated' AND user_id = ?",
    )
    .bind(kontrak_apply)
    .bind(admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 1, "perubahan kontrak diaudit");

    // Promosi: S4 menjadi pekerjaan, kontrak, dan draft baru. Kedua kalinya dilewati.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/promote-draft",
        Some(&token),
        Some(json!({ "ids": [s4] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["created"], 1);
    let res = &b["results"][0];
    assert_eq!(res["status"], "created");
    let pek_baru = res["pekerjaan_id"].as_i64().unwrap();
    let kontrak_baru = res["kontrak_id"].as_i64().unwrap();

    let nama: String = sqlx::query_scalar("SELECT nama_paket FROM tbl_pekerjaan WHERE id = ?")
        .bind(pek_baru)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(nama, "Paket SPSE UJI-PRM-PROMO-4", "nama kosong memakai nama default");
    let row = sqlx::query("SELECT kode_paket, kode_rup, CAST(id_pekerjaan AS SIGNED) AS p FROM tbl_kontrak WHERE id = ?")
        .bind(kontrak_baru)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.try_get::<String, _>("kode_paket").unwrap(), "UJI-PRM-PROMO-4");
    assert_eq!(row.try_get::<String, _>("kode_rup").unwrap(), "RUP-UJI-4");
    assert_eq!(row.try_get::<i64, _>("p").unwrap(), pek_baru);
    let draft: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_draft_pekerjaan WHERE pekerjaan_id = ? AND kode_paket = 'UJI-PRM-PROMO-4'")
        .bind(pek_baru)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(draft, 1);
    let status: String = sqlx::query_scalar("SELECT match_status FROM tbl_procurement_staging_paket WHERE id = ?")
        .bind(s4)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "promoted_draft");

    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/promote-draft",
        Some(&token),
        Some(json!({ "ids": [s4] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["skipped"], 1);
    assert_eq!(b["results"][0]["reason"], "already_matched");

    // Validasi: id tidak ada.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/promote-draft",
        Some(&token),
        Some(json!({ "ids": [999_999_999] })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["message"], "The selected ids.0 is invalid.");

    cleanup(&pool, "promo", kode_like, &nama_like).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn dokumen_impor_dan_zip() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    let base = spse_base();
    cleanup(&pool, "dokumen", "UJI-PS-DOKUMEN%", &["UJI PS dokumen%"]).await;

    let admin = make_user(&pool, &email("dokumen", "admin"), true).await;
    let user = make_user(&pool, &email("dokumen", "user"), false).await;
    let token = auth::login::create_token(&pool, admin, "uji-procurement").await.unwrap();
    let token_user = auth::login::create_token(&pool, user, "uji-procurement").await.unwrap();
    insert_session(&pool, admin, "uji-ok", COOKIE_JSON).await;
    insert_session(&pool, user, "uji-ok", COOKIE_JSON).await;

    // Daftar dokumen: halaman nontender memuat tautan generated dan unduhan.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/procurement/spse/packages/UJI-PS-KODE-1/documents",
        Some(&token_user),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["count"], 3);
    let docs = b["data"].as_array().unwrap();
    assert!(docs.iter().any(|d| d["kind"] == "generated" && d["doc_type"] == "summary"));
    assert!(docs.iter().any(|d| d["url"] == format!("{base}/uji-ok/dl/rab-uji.pdf")));
    assert!(docs.iter().all(|d| d["id"].as_str().unwrap().len() == 32));

    // Jenis tender: halaman tender tidak ada di stub, jadi daftar kosong.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/procurement/spse/packages/UJI-PS-KODE-1/documents?jenis_paket=tender_seleksi",
        Some(&token_user),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["count"], 0);

    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/procurement/spse/packages/UJI-PS-KODE-1/documents?jenis_paket=lainnya",
        Some(&token_user),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["message"], "The selected jenis paket is invalid.");

    // Impor: satu berhasil, satu 404, satu seksi lama (ditolak sebelum unduh).
    let pekerjaan = insert_pekerjaan(&pool, "UJI PS dokumen Irigasi").await;
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/packages/import-documents",
        Some(&token),
        Some(json!({
            "pekerjaan_id": pekerjaan,
            "kode_paket": "UJI-PS-KODE-1",
            "documents": [
                { "url": format!("{base}/uji-ok/dl/rab-uji.pdf"), "jenis_dokumen": "SPSE Dokumen", "label": "RAB Uji" },
                { "url": format!("{base}/uji-ok/dl/rusak.pdf"), "jenis_dokumen": "SPSE Dokumen" },
                { "url": format!("{base}/uji-ok/nontender/77/pengumumanlelang"), "jenis_dokumen": "SPSE Dokumen" },
                { "url": format!("{base}/uji-ok/dl/rab-uji.pdf"), "jenis_dokumen": "RAB" },
            ],
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["imported"], 1);
    assert_eq!(b["failed"], 3);
    assert_eq!(b["message"], "Import selesai: 1 berhasil, 3 gagal.");
    let results = b["results"].as_array().unwrap();
    assert_eq!(results[0]["status"], "imported");
    assert_eq!(results[1]["reason"], "SPSE unduh gagal: HTTP 404 (URL tidak ada atau butuh sesi berbeda).");
    assert!(results[2]["reason"].as_str().unwrap().starts_with("URL section SPSE"));
    // Jenis selain "SPSE Dokumen" ditolak, tidak masuk ke tbl_berkas.
    assert!(results[3]["reason"].as_str().unwrap().contains("tidak dikenal"));

    let jenis: String = sqlx::query_scalar("SELECT jenis_dokumen FROM tbl_berkas WHERE pekerjaan_id = ?")
        .bind(pekerjaan)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jenis, "SPSE Dokumen", "hanya unduhan yang berhasil yang tersimpan, berjenis SPSE Dokumen");
    let media: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media m JOIN tbl_berkas bk ON bk.id = m.model_id \
         WHERE m.model_type = 'App\\\\Models\\\\Berkas' AND m.collection_name = 'berkas/dokumen' AND bk.pekerjaan_id = ?",
    )
    .bind(pekerjaan)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(media, 1);

    // ZIP: berkas yang berhasil plus _gagal.json.
    let (s, headers, bytes) = raw(
        &pool,
        Method::POST,
        "/api/procurement/spse/packages/download-zip",
        Some(&token),
        Some(json!({
            "kode_paket": "UJI-PS-KODE-1",
            "documents": [
                { "url": format!("{base}/uji-ok/dl/rab-uji.pdf"), "label": "RAB Uji" },
                { "url": format!("{base}/uji-ok/dl/rusak.pdf"), "label": "Rusak" },
            ],
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment; filename=spse_UJI-PS-KODE-1.zip");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"01_RAB Uji.pdf".to_string()), "{names:?}");
    assert!(names.contains(&"_gagal.json".to_string()), "{names:?}");
    let mut isi = String::new();
    archive.by_name("01_RAB Uji.pdf").unwrap().read_to_string(&mut isi).unwrap();
    assert_eq!(isi, "%PDF-1.4 rab uji isi");

    // ZIP tanpa unduhan berhasil: 422 dengan rincian kegagalan.
    let (s, _, bytes) = raw(
        &pool,
        Method::POST,
        "/api/procurement/spse/packages/download-zip",
        Some(&token),
        Some(json!({
            "kode_paket": "UJI-PS-KODE-1",
            "documents": [{ "url": format!("{base}/uji-ok/dl/rusak.pdf") }],
        })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let b: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(b["message"], "Tidak ada dokumen berhasil diunduh.");
    assert_eq!(b["failed"], 1);

    cleanup(&pool, "dokumen", "UJI-PS-DOKUMEN%", &["UJI PS dokumen%"]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn mutasi_non_admin_ditolak_dan_session_wajib() {
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "akses", "UJI-PS-AKSES%", &[]).await;

    let user = make_user(&pool, &email("akses", "user"), false).await;
    let token = auth::login::create_token(&pool, user, "uji-procurement").await.unwrap();

    // Mutasi tanpa aturan permission: 403 seperti CheckRoutePermission.
    let (s, _) = send(&pool, Method::POST, "/api/procurement/spse/sync", Some(&token), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/staging/map",
        Some(&token),
        Some(json!({ "id": 1, "pekerjaan_id": 1 })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Tanpa token: 401.
    let (s, _) = send(&pool, Method::GET, "/api/procurement/spse/staging", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Sinkron tanpa sesi aktif: 401 sebelum validasi.
    let admin = make_user(&pool, &email("akses", "admin"), true).await;
    let token_admin = auth::login::create_token(&pool, admin, "uji-procurement").await.unwrap();
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/procurement/spse/sync",
        Some(&token_admin),
        Some(json!({ "page_length": 0 })),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(b["message"], "Session SPSE tidak aktif. Login ulang di SPSE.");

    cleanup(&pool, "akses", "UJI-PS-AKSES%", &[]).await;
}
