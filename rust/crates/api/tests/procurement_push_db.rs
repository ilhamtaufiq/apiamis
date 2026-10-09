//! Push kontrak ke SPSE lewat router terhadap MySQL dan stub SPSE lokal (`127.0.0.1`).
//!
//! Setiap test memakai slug stub sendiri (`uji-push-*`), sehingga state dan log permintaan tidak
//! saling mengganggu. Slug `uji-mati` melayani 401. SPSE asli tidak pernah dihubungi.
//! Nilai `SPSE_PPK_*` di test adalah nilai palsu yang di-set lewat env, bukan nilai produksi.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test procurement_push_db -- --include-ignored
//! ```

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
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
const PUSH_URI: &str = "/api/procurement/spse/kontrak/push";

// ---------------------------------------------------------------------------
// Stub SPSE
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Progress {
    sppbj: bool,
    spk: bool,
    sskk: bool,
    spmk: bool,
}

/// State per slug. `uji-push-done` mulai dengan semua dokumen sudah ada di SPSE.
fn initial_progress(slug: &str) -> Progress {
    if slug == "uji-push-done" {
        Progress { sppbj: true, spk: true, sskk: true, spmk: true }
    } else {
        Progress::default()
    }
}

fn states() -> &'static Mutex<HashMap<String, Progress>> {
    static S: OnceLock<Mutex<HashMap<String, Progress>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Log permintaan per slug: `METHOD path?query`.
fn request_log() -> &'static Mutex<Vec<(String, String)>> {
    static L: OnceLock<Mutex<Vec<(String, String)>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(Vec::new()))
}

fn progress(slug: &str) -> Progress {
    let mut m = states().lock().unwrap();
    m.entry(slug.to_string())
        .or_insert_with(|| initial_progress(slug))
        .clone()
}

fn mark(slug: &str, f: impl FnOnce(&mut Progress)) {
    let mut m = states().lock().unwrap();
    let p = m
        .entry(slug.to_string())
        .or_insert_with(|| initial_progress(slug));
    f(p);
}

fn sent(slug: &str) -> Vec<String> {
    request_log()
        .lock()
        .unwrap()
        .iter()
        .filter(|(s, _)| s == slug)
        .map(|(_, r)| r.clone())
        .collect()
}

/// Nilai bagian multipart `name` (tanpa berkas). `None` bila field tidak ada.
fn mp_value(body: &str, name: &str) -> Option<String> {
    let marker = format!("name=\"{name}\"\r\n\r\n");
    let start = body.find(&marker)? + marker.len();
    let rest = &body[start..];
    let end = rest.find("\r\n--")?;
    Some(rest[..end].to_string())
}

/// Memeriksa field yang harus dikirim dengan nilai tertentu. Galat berisi nama field.
fn expect_fields(body: &str, pairs: &[(&str, &str)]) -> Result<(), String> {
    for (name, want) in pairs {
        match mp_value(body, name) {
            Some(got) if got == *want => {}
            other => return Err(format!("field {name}: dapat {other:?}, harus {want:?}")),
        }
    }
    Ok(())
}

fn html(body: &str) -> Response {
    reply(200, "text/html", body.as_bytes().to_vec(), None)
}

fn reply(status: u16, ctype: &str, body: Vec<u8>, location: Option<String>) -> Response {
    let mut b = Response::builder().status(status).header(header::CONTENT_TYPE, ctype);
    if let Some(l) = location {
        b = b.header(header::LOCATION, l);
    }
    b.body(Body::from(body)).unwrap()
}

fn list_html(slug: &str, p: &Progress) -> String {
    let mut rows = String::new();
    if p.sppbj {
        rows += &format!(r#"<tr><td><a href="/{slug}/sppbj-pl/sppbjppkpl?plId=uji-pp&amp;sppbjId=9001">SPPBJ</a></td></tr>"#);
    }
    if p.spk {
        rows += &format!(r#"<tr><td><a href="/{slug}/spk-pl/spkpl?sppbjId=9001&amp;spkId=9101">SPK</a></td></tr>"#);
    }
    if p.sskk {
        rows += "<tr><td>Sekaligus</td></tr>";
    }
    if p.spmk {
        rows += &format!(r#"<tr><td><a href="/{slug}/spk-pl/cetak?pesananId=9201">SPMK</a></td></tr>"#);
    }
    format!(r#"<html><body><table id="tblsppbj"><tbody>{rows}</tbody></table></body></html>"#)
}

const SPPBJ_FORM: &str = r#"<html><body><form id="formSppbj">
  <select name="rekananId"><option value="0">Pilih</option>
    <option value="777">Uji PP Penyedia</option><option value="778">PT Uji PP Lain</option></select>
</form></body></html>"#;

const SPK_FORM: &str = r#"<html><body><form id="formPesanan" method="post">
  <input type="hidden" name="authenticityToken" value="tok-uji">
  <input type="hidden" name="spk.spk_id" value="">
  <input type="text" name="spk.nama_ppk_kontrak" value="PPK DARI FORM">
  <input type="text" name="spk.nip_ppk_kontrak" value="">
  <input type="checkbox" name="cb_tidak" value="1">
  <textarea name="spk.catatan">Catatan &amp; uji</textarea>
  <select name="spk.jenis"><option value="a">A</option><option value="b" selected>B</option></select>
</form>
<input id="nilaiKontrak_f" type="text" value="12.345.678,90">
</body></html>"#;

const SPMK_FORM: &str = r#"<html><body><form>
  <input type="hidden" name="authenticityToken" value="tok-uji">
  <input type="hidden" name="pesanan.jenis" value="barang">
  <select name="pesanan.kategori"><option value="x">X</option><option value="y" selected>Y</option></select>
</form></body></html>"#;

async fn stub(method: Method, uri: Uri, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let trimmed = uri.path().trim_start_matches('/').to_string();
    let (slug, rest) = match trimmed.split_once('/') {
        Some((s, r)) => (s.to_string(), format!("/{r}")),
        None => (trimmed.clone(), "/".to_string()),
    };
    let query = uri.query().unwrap_or("").to_string();
    let entry = if query.is_empty() {
        format!("{} {rest}", method.as_str())
    } else {
        format!("{} {rest}?{query}", method.as_str())
    };
    request_log().lock().unwrap().push((slug.clone(), entry));

    let body = String::from_utf8_lossy(&body).into_owned();
    let has_cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.contains("SPSE_SESSION="));
    if slug == "uji-mati" || !has_cookie {
        return reply(401, "text/plain", b"denied".to_vec(), None);
    }

    let p = progress(&slug);
    let base = format!("/{slug}");
    match (method.as_str(), rest.as_str()) {
        ("GET", "/sppbj-pl/listsppbjpl") => html(&list_html(&slug, &p)),
        ("GET", "/sppbj-pl/sppbjppkpl") => html(SPPBJ_FORM),
        ("POST", "/sppbj-pl/pengecekanblacklist") => reply(200, "text/html", b"ok".to_vec(), None),
        ("POST", "/sppbj-pl/simpansppbjpl") => {
            if slug == "uji-push-tolak" {
                return reply(500, "text/plain", b"gagal".to_vec(), None);
            }
            if let Err(e) = expect_fields(
                &body,
                &[
                    ("authenticityToken", "tok-uji"),
                    ("sppbj.sppbj_no", "UJI-SPPBJ-PP-1"),
                    ("sppbj.sppbj_tgl_kirim", "01-10-2026"),
                    ("sppbj.sppbj_kota", "Cianjur"),
                    ("sppbj.jabatan_ppk_sppbj", "uji-jabatan-ppk"),
                    ("rekananId", "777"),
                ],
            ) {
                return reply(400, "text/plain", e.into_bytes(), None);
            }
            mark(&slug, |p| p.sppbj = true);
            reply(302, "text/html", Vec::new(), Some(format!("{base}/sppbj-pl/sppbjppkpl?plId=uji-pp&sppbjId=9001")))
        }
        ("GET", "/spk-pl/spkpl") => html(SPK_FORM),
        ("POST", "/spk-pl/simpanspk") => {
            if let Err(e) = expect_fields(
                &body,
                &[
                    ("authenticityToken", "tok-uji"),
                    ("spk.spk_no", "UJI-SPK-PP-1"),
                    ("spk.spk_tgl", "02-10-2026"),
                    ("spk.nama_ppk_kontrak", "PPK DARI FORM"),
                    ("spk.nip_ppk_kontrak", "uji-nip-ppk"),
                    ("spk.jabatan_ppk_kontrak", "uji-jabatan-ppk"),
                    ("spk.no_sk_ppk_kontrak", "uji-sk-ppk"),
                    ("spk.spk_wakil_penyedia", "Uji Direktur"),
                    ("spk.spk_nama_bank", "BJB"),
                    ("spk.spk_norekening", "123456"),
                    ("spk.spk_nilai", "12.345.678,90"),
                    ("spk.nilai_pdn", "12.345.678,90"),
                    ("content.waktu_penyelesaian", "90 Hari Kalender"),
                    ("tgl_diterima", "03-10-2026"),
                    ("tgl_selesai", "31-12-2026"),
                    ("spk.kontrak_lingkup_pekerjaan", "<p>Sesuai Spesifikasi Teknis Pekerjaan</p>"),
                ],
            ) {
                return reply(400, "text/plain", e.into_bytes(), None);
            }
            mark(&slug, |p| p.spk = true);
            reply(302, "text/html", Vec::new(), Some(format!("{base}/spk-pl/spkpl?sppbjId=9001&spkId=9101")))
        }
        ("POST", "/sskk-pl/simpancarapembayaran") => {
            if let Err(e) = expect_fields(&body, &[("authenticityToken", "tok-uji"), ("cara_pembayaran", "Sekaligus")]) {
                return reply(400, "text/plain", e.into_bytes(), None);
            }
            mark(&slug, |p| p.sskk = true);
            reply(302, "text/html", Vec::new(), Some(format!("{base}/sskk-pl/lihat?id=9001")))
        }
        ("GET", "/spk-pl/spmknon") => html(SPMK_FORM),
        ("POST", "/spk-pl/simpansuratpesanannon") => {
            if let Err(e) = expect_fields(
                &body,
                &[
                    ("pesanan.pes_no", "UJI-SPMK-PP-1"),
                    ("pesanan.pes_tgl", "03-10-2026"),
                    ("pesanan.jenis", "barang"),
                    ("pesanan.kategori", "y"),
                    ("content.wakil_sah_rekanan", "Uji Direktur"),
                    ("simpan", ""),
                ],
            ) {
                return reply(400, "text/plain", e.into_bytes(), None);
            }
            mark(&slug, |p| p.spmk = true);
            reply(302, "text/html", Vec::new(), Some(format!("{base}/spk-pl/cetak?pesananId=9201")))
        }
        _ => reply(404, "text/plain", b"tidak ditemukan".to_vec(), None),
    }
}

/// Menyalakan stub di thread sendiri dan mengatur env. Dipanggil di awal setiap test.
fn spse_base() {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("runtime stub");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
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
        // Nilai uji, bukan nilai produksi. Harus di-set agar push tidak menolak sebelum simpan.
        std::env::set_var("SPSE_PPK_NAMA", "Uji PPK Nama");
        std::env::set_var("SPSE_PPK_NIP", "uji-nip-ppk");
        std::env::set_var("SPSE_PPK_JABATAN", "uji-jabatan-ppk");
        std::env::set_var("SPSE_PPK_NO_SK", "uji-sk-ppk");
        base
    });
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

async fn call(pool: &MySqlPool, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
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
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Tes DB berbagi tabel (users, roles, tbl_kontrak, notifications); jalankan satu per satu agar tidak deadlock.
static SERIAL: Mutex<()> = Mutex::new(());

fn db_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

fn email(tag: &str) -> String {
    format!("uji-pp-{tag}-user@example.test")
}

async fn make_user(pool: &MySqlPool, tag: &str) -> u64 {
    let email = email(tag);
    sqlx::query("DELETE FROM users WHERE email = ?").bind(&email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Push', ?, 'x', NOW(), NOW())")
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_one(pool)
        .await
        .unwrap();
    // Rute mutasi memerlukan permission; pengguna uji diberi role admin seperti procurement_spse_db.
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
    uid
}

async fn insert_session(pool: &MySqlPool, user_id: u64, slug: &str) {
    let key = crypt::key_from_app_key(APP_KEY).unwrap();
    let payload = crypt::encrypt_string(&key, COOKIE_JSON);
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

/// Penyedia dan kontrak uji untuk satu test. Tanggal SPPBJ tetap agar tanggal blacklist bisa diperiksa.
async fn insert_kontrak(pool: &MySqlPool, kode: &str) -> (i64, i64) {
    let penyedia = sqlx::query(
        "INSERT INTO tbl_penyedia (nama, direktur, bank, norek, created_at, updated_at) VALUES ('Uji PP Penyedia', 'Uji Direktur', 'BJB', '123456', NOW(), NOW())",
    )
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    let kontrak = sqlx::query(
        "INSERT INTO tbl_kontrak (id_penyedia, kode_paket, sppbj, spk, spmk, tgl_sppbj, tgl_spk, tgl_spmk, tgl_selesai, created_at, updated_at) \
         VALUES (?, ?, 'UJI-SPPBJ-PP-1', 'UJI-SPK-PP-1', 'UJI-SPMK-PP-1', '2026-10-01', '2026-10-02', '2026-10-03', '2026-12-31', NOW(), NOW())",
    )
    .bind(penyedia)
    .bind(kode)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    (kontrak, penyedia)
}

/// Hapus data uji satu test: user `uji-pp-{tag}-*`, kontrak `kode`, penyedia, audit, dan notifikasi.
async fn cleanup(pool: &MySqlPool, tag: &str, kode: &str) {
    let kontrak_ids: Vec<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_kontrak WHERE kode_paket = ?")
        .bind(kode)
        .fetch_all(pool)
        .await
        .unwrap();
    let penyedia_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(id_penyedia AS SIGNED) FROM tbl_kontrak WHERE kode_paket = ? AND id_penyedia IS NOT NULL",
    )
    .bind(kode)
    .fetch_all(pool)
    .await
    .unwrap();
    for k in &kontrak_ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ?")
            .bind("App\\Models\\Kontrak")
            .bind(k)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM notifications WHERE data LIKE ?")
            .bind(format!("%Model Kontrak dengan ID #{k} %"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?").bind(k).execute(pool).await.unwrap();
    }
    for p in &penyedia_ids {
        sqlx::query("DELETE FROM tbl_penyedia WHERE id = ?").bind(p).execute(pool).await.unwrap();
    }
    let users: Vec<u64> = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email(tag))
        .fetch_all(pool)
        .await
        .unwrap();
    for u in &users {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE user_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM tbl_spse_sessions WHERE user_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM model_has_roles WHERE model_type = 'App\\\\Models\\\\User' AND model_id = ?").bind(u).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM users WHERE id = ?").bind(u).execute(pool).await.unwrap();
    }
}

async fn kontrak_state(pool: &MySqlPool, id: i64) -> (Option<String>, Option<String>, Option<String>, bool, Option<String>) {
    let r = sqlx::query(
        "SELECT spse_sppbj_id, spse_spk_id, spse_rekanan_id, spse_pushed_at IS NOT NULL AS pushed, \
         CAST(spse_push_log AS CHAR) AS log FROM tbl_kontrak WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    (
        r.try_get("spse_sppbj_id").unwrap(),
        r.try_get("spse_spk_id").unwrap(),
        r.try_get("spse_rekanan_id").unwrap(),
        r.try_get::<i64, _>("pushed").unwrap() == 1,
        r.try_get("log").unwrap(),
    )
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

const EXPECTED_SENT: [&str; 11] = [
    "GET /sppbj-pl/listsppbjpl?plId=uji-pp-ok",
    "GET /sppbj-pl/sppbjppkpl?plId=uji-pp-ok",
    "POST /sppbj-pl/pengecekanblacklist?rknId=777&tglbuat=01-10-2026&llsId=uji-pp-ok",
    "POST /sppbj-pl/simpansppbjpl?plId=uji-pp-ok",
    "GET /sppbj-pl/listsppbjpl?plId=uji-pp-ok",
    "GET /spk-pl/spkpl?sppbjId=9001",
    "POST /spk-pl/simpanspk?sppbjId=9001",
    "POST /sskk-pl/simpancarapembayaran?id=9001",
    "GET /spk-pl/spmknon?sppbjId=9001",
    "POST /spk-pl/simpansuratpesanannon?spkId=9101&sppbjId=9001",
    "GET /sppbj-pl/listsppbjpl?plId=uji-pp-ok",
];

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn push_berurutan_simpan_id_audit_dan_tolak_ulang() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "ok", "uji-pp-ok").await;

    let user = make_user(&pool, "ok").await;
    let token = auth::login::create_token(&pool, user, "uji-push").await.unwrap();
    insert_session(&pool, user, "uji-push-ok").await;
    let (kontrak, _) = insert_kontrak(&pool, "uji-pp-ok").await;

    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["message"], "Push kontrak ke SPSE selesai.");
    assert_eq!(body["spse_ids"]["sppbj_id"], "9001");
    assert_eq!(body["spse_ids"]["spk_id"], "9101");
    assert_eq!(body["spse_ids"]["rekanan_id"], "777");
    assert_eq!(body["nilai_kontrak_spse"], "12.345.678,90");
    let steps: Vec<(String, String)> = body["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["step"].as_str().unwrap().to_string(), s["status"].as_str().unwrap().to_string()))
        .collect();
    let want: Vec<(String, String)> = [
        "pengecekan_blacklist",
        "simpan_sppbj",
        "simpan_spk",
        "simpan_cara_pembayaran",
        "simpan_spmk",
    ]
    .iter()
    .map(|s| (s.to_string(), "ok".to_string()))
    .collect();
    assert_eq!(steps, want);

    // Urutan permintaan ke SPSE sama dengan Laravel.
    assert_eq!(sent("uji-push-ok"), EXPECTED_SENT.to_vec());

    let (sppbj, spk, rekanan, pushed, log) = kontrak_state(&pool, kontrak).await;
    assert_eq!(sppbj.as_deref(), Some("9001"));
    assert_eq!(spk.as_deref(), Some("9101"));
    assert_eq!(rekanan.as_deref(), Some("777"));
    assert!(pushed, "spse_pushed_at harus terisi");
    let log: Value = serde_json::from_str(&log.unwrap()).unwrap();
    assert_eq!(log["pl_id"], "uji-pp-ok");
    assert_eq!(log["steps"].as_array().unwrap().len(), 5);
    assert_eq!(log["spse_status_before_push"]["sppbj_complete"], true);
    assert_eq!(log["spse_status_before_push"]["spk_complete"], false);

    let audit: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = 'updated'",
    )
    .bind("App\\Models\\Kontrak")
    .bind(kontrak)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 1, "perubahan kontrak harus tercatat di audit");

    // Push kedua ditolak sebelum permintaan apa pun ke SPSE.
    let before = sent("uji-push-ok").len();
    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["message"].as_str().unwrap().starts_with("Kontrak sudah di-push ke SPSE pada "));
    assert_eq!(sent("uji-push-ok").len(), before);

    cleanup(&pool, "ok", "uji-pp-ok").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn push_ditolak_bila_sudah_lengkap_di_spse() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "done", "uji-pp-done").await;

    let user = make_user(&pool, "done").await;
    let token = auth::login::create_token(&pool, user, "uji-push").await.unwrap();
    insert_session(&pool, user, "uji-push-done").await;
    let (kontrak, _) = insert_kontrak(&pool, "uji-pp-done").await;

    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["message"],
        "Kontrak sudah lengkap di SPSE (SPPBJ, SPK, SSKK, SPMK). Push dibatalkan agar data tidak ditimpa."
    );
    // Hanya satu GET daftar: tidak ada simpan.
    assert_eq!(sent("uji-push-done"), vec!["GET /sppbj-pl/listsppbjpl?plId=uji-pp-done".to_string()]);
    let (_, _, _, pushed, _) = kontrak_state(&pool, kontrak).await;
    assert!(!pushed);

    cleanup(&pool, "done", "uji-pp-done").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn push_sesi_expired_menonaktifkan_sesi() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "mati", "uji-pp-mati").await;

    let user = make_user(&pool, "mati").await;
    let token = auth::login::create_token(&pool, user, "uji-push").await.unwrap();
    insert_session(&pool, user, "uji-mati").await;
    let (kontrak, _) = insert_kontrak(&pool, "uji-pp-mati").await;

    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Session SPSE expired. Login ulang di SPSE lalu kirim cookie lagi.");
    let active: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spse_sessions WHERE user_id = ? AND is_active = 1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active, 0, "sesi yang expired harus dinonaktifkan");

    cleanup(&pool, "mati", "uji-pp-mati").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn push_simpan_ditolak_menjadi_500_tanpa_mengubah_kontrak() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "tolak", "uji-pp-tolak").await;

    let user = make_user(&pool, "tolak").await;
    let token = auth::login::create_token(&pool, user, "uji-push").await.unwrap();
    insert_session(&pool, user, "uji-push-tolak").await;
    let (kontrak, _) = insert_kontrak(&pool, "uji-pp-tolak").await;

    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body["message"],
        "Push kontrak ke SPSE gagal: Langkah simpan_sppbj gagal: Simpan SPPBJ gagal: HTTP 500"
    );
    let (sppbj, _, _, pushed, _) = kontrak_state(&pool, kontrak).await;
    assert!(sppbj.is_none() && !pushed, "kontrak tidak boleh berubah bila simpan gagal");

    cleanup(&pool, "tolak", "uji-pp-tolak").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn push_tanpa_sesi_dan_validasi_input() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    spse_base();
    let pool = MySqlPool::connect(&db_url()).await.unwrap();
    cleanup(&pool, "val", "uji-pp-val").await;

    let user = make_user(&pool, "val").await;
    let token = auth::login::create_token(&pool, user, "uji-push").await.unwrap();
    let (kontrak, _) = insert_kontrak(&pool, "uji-pp-val").await;

    // Tanpa session SPSE aktif: 401, sebelum validasi.
    let (status, body) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": kontrak }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Session SPSE tidak aktif. Login ulang di SPSE.");

    insert_session(&pool, user, "uji-push-val").await;

    let (status, _) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = call(&pool, Method::POST, PUSH_URI, Some(&token), Some(json!({ "kontrak_id": 999_999_999 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    assert!(sent("uji-push-val").is_empty(), "validasi gagal tidak boleh memanggil SPSE");

    cleanup(&pool, "val", "uji-pp-val").await;
}
