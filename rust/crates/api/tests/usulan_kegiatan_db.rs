//! Rute `/api/usulan-kegiatan` lewat router terhadap MySQL: 401 tanpa token, CRUD, export Excel, notifikasi
//! admin saat simpan, 422, dan pembatasan baris untuk non-admin.
//!
//! Skema: `rust/fixtures/usulan_kegiatan_schema.sql`. Tabel lain yang dipakai: `users`, `roles`,
//! `model_has_roles`, `personal_access_tokens`, `tbl_kecamatan`, `tbl_desa`, `media`, `notifications`,
//! dan `tbl_audit_logs`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test usulan_kegiatan_db -- --include-ignored
//! ```
//!
//! Setiap tes memakai penanda `uji-uk-<tes>-<pid>` di nama pengusul dan email `uji-uk-<peran>-<tes>-<pid>@example.test`.
//! Cleanup hanya menghapus baris yang dicatat tes ini (id usulan, notifikasi bertanda, audit, media, dan akun uji).

use std::io::{Cursor, Read};

use api::{app, media, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const USULAN: &str = "/api/usulan-kegiatan";
const EXPORT: &str = "/api/usulan-kegiatan/export-excel";
const BOUNDARY: &str = "----ujiukboundary";
/// Nilai `model_type` seperti yang ditulis Laravel.
const MODEL: &str = "App\\Models\\UsulanKegiatan";
const USER_MODEL: &str = "App\\Models\\User";
const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
/// Berkas uji untuk `dokumen`. Isinya tidak diperiksa selain ekstensi.
const DOKUMEN: &[u8] = b"%PDF-1.4\n% uji usulan kegiatan\n%%EOF\n";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Respons mentah: status, header, dan body.
struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    fn header_str(&self, name: header::HeaderName) -> String {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    }
}

async fn dispatch(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    content_type: Option<String>,
    body: Vec<u8>,
) -> Resp {
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
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    Resp {
        status,
        headers,
        body,
    }
}

/// Kirim body JSON (atau tanpa body bila `None`).
async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Resp {
    match body {
        None => dispatch(pool, method, uri, token, None, Vec::new()).await,
        Some(v) => {
            dispatch(
                pool,
                method,
                uri,
                token,
                Some("application/json".to_string()),
                v.to_string().into_bytes(),
            )
            .await
        }
    }
}

/// Body `multipart/form-data` dengan field teks dan satu berkas opsional `(nama field, nama berkas, isi)`.
fn multipart_body(fields: &[(&str, &str)], file: Option<(&str, &str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    if let Some((field, filename, bytes)) = file {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn send_multipart(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    fields: &[(&str, &str)],
    file: Option<(&str, &str, &[u8])>,
) -> Resp {
    dispatch(
        pool,
        method,
        uri,
        token,
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
        multipart_body(fields, file),
    )
    .await
}

fn pool_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

async fn connect() -> MySqlPool {
    MySqlPool::connect(&pool_url()).await.unwrap()
}

/// Penanda unik per tes dan proses, dipakai di nama pengusul.
fn penanda(tes: &str) -> String {
    format!("uji-uk-{tes}-{}", std::process::id())
}

fn surel(tes: &str, peran: &str) -> String {
    format!("uji-uk-{peran}-{tes}-{}@example.test", std::process::id())
}

/// Satu direktori media untuk semua tes di berkas ini. Nilainya sama di setiap tes, jadi tes paralel tidak berbeda.
fn use_storage() {
    let dir = std::env::temp_dir().join(format!("uji-uk-storage-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", dir);
}

/// Pengguna uji dengan peran `admin` atau tanpa peran, lalu token.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> (u64, String) {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji UK', ?, 'x', NOW(), NOW())")
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
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)")
            .bind(role)
            .bind(USER_MODEL)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    let token = auth::login::create_token(pool, uid, "uji-uk").await.unwrap();
    (uid, token)
}

/// Id kecamatan dan desa yang sudah ada (tidak diubah oleh tes).
async fn lokasi(pool: &MySqlPool) -> (u64, u64) {
    let kec: u64 = sqlx::query_scalar("SELECT id FROM tbl_kecamatan ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("tbl_kecamatan kosong");
    let desa: u64 = sqlx::query_scalar("SELECT id FROM tbl_desa ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("tbl_desa kosong");
    (kec, desa)
}

/// Baris uji `tbl_usulan_kegiatan` langsung lewat SQL (untuk data awal; non-admin tidak bisa POST).
async fn insert_usulan(
    pool: &MySqlPool,
    user_id: u64,
    sub_bidang: &str,
    pengusul: &str,
    perihal: &str,
) -> i64 {
    let (kec, desa) = lokasi(pool).await;
    let res = sqlx::query(
        "INSERT INTO tbl_usulan_kegiatan (user_id, sub_bidang, nama_pengusul, kecamatan_id, desa_id, perihal, \
         ringkasan, tanggal_surat_masuk, nomor_surat_masuk, tanggal_surat, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, '', '2026-01-05', 'UJI/001', '2026-01-04', NOW(), NOW())",
    )
    .bind(user_id)
    .bind(sub_bidang)
    .bind(pengusul)
    .bind(kec)
    .bind(desa)
    .bind(perihal)
    .execute(pool)
    .await
    .unwrap();
    res.last_insert_id() as i64
}

/// `perihal` dan `sub_bidang` sebuah usulan. `sub_bidang` (enum) dibaca sebagai teks.
async fn usulan_row(pool: &MySqlPool, id: i64) -> Option<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT perihal, CAST(sub_bidang AS CHAR) FROM tbl_usulan_kegiatan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn usulan_count_by_penanda(pool: &MySqlPool, penanda: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM tbl_usulan_kegiatan WHERE nama_pengusul LIKE ?")
        .bind(format!("{penanda}%"))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Baris audit `(event, old_values, new_values)` untuk satu usulan, urut id.
async fn audit_rows(pool: &MySqlPool, id: i64) -> Vec<(String, Option<String>, Option<String>)> {
    sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT event, CAST(old_values AS CHAR), CAST(new_values AS CHAR) FROM tbl_audit_logs \
         WHERE auditable_type = ? AND auditable_id = ? ORDER BY id",
    )
    .bind(MODEL)
    .bind(id as u64)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Isi `data` notifikasi milik `user_id` yang memuat penanda, sudah di-parse sebagai JSON.
async fn notifikasi(pool: &MySqlPool, user_id: u64, penanda: &str) -> Vec<Value> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT data FROM notifications WHERE notifiable_type = ? AND notifiable_id = ? AND data LIKE ? \
         ORDER BY created_at, id",
    )
    .bind(USER_MODEL)
    .bind(user_id)
    .bind(format!("%{penanda}%"))
    .fetch_all(pool)
    .await
    .unwrap();
    rows.iter().map(|s| serde_json::from_str(s).unwrap()).collect()
}

async fn media_ids(pool: &MySqlPool, id: i64) -> Vec<u64> {
    sqlx::query_scalar(
        "SELECT id FROM media WHERE model_type = ? AND model_id = ? AND collection_name = 'dokumen' ORDER BY id",
    )
    .bind(MODEL)
    .bind(id as u64)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Hapus hanya baris yang dicatat tes ini: notifikasi bertanda, audit, media (beserta berkasnya),
/// usulan, lalu akun uji.
async fn cleanup(pool: &MySqlPool, penanda: &str, usulan_ids: &[i64], emails: &[&str]) {
    sqlx::query("DELETE FROM notifications WHERE data LIKE ?")
        .bind(format!("%{penanda}%"))
        .execute(pool)
        .await
        .unwrap();
    for id in usulan_ids {
        for mid in media_ids_all(pool, *id).await {
            let _ = std::fs::remove_dir_all(media::media_dir(mid));
        }
        sqlx::query("DELETE FROM media WHERE model_type = ? AND model_id = ?")
            .bind(MODEL)
            .bind(*id as u64)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ?")
            .bind(MODEL)
            .bind(*id as u64)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_usulan_kegiatan WHERE id = ?")
            .bind(*id)
            .execute(pool)
            .await
            .unwrap();
    }
    for email in emails {
        sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
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

/// Semua id media milik usulan (dipakai cleanup, termasuk yang sudah diganti).
async fn media_ids_all(pool: &MySqlPool, id: i64) -> Vec<u64> {
    sqlx::query_scalar("SELECT id FROM media WHERE model_type = ? AND model_id = ?")
        .bind(MODEL)
        .bind(id as u64)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Sheet pertama `xlsx` sebagai teks: string dibaca dari `xl/sharedStrings.xml`.
fn shared_strings(xlsx: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(xlsx.to_vec())).expect("berkas xlsx");
    let mut xml = String::new();
    zip.by_name("xl/sharedStrings.xml")
        .expect("sharedStrings.xml")
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

// ---------------------------------------------------------------------------
// Tes
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tanpa_token_401() {
    let pool = connect().await;
    let cases: [(Method, &str, Option<Value>); 6] = [
        (Method::GET, USULAN, None),
        (Method::GET, EXPORT, None),
        (Method::GET, "/api/usulan-kegiatan/1", None),
        (Method::POST, USULAN, Some(json!({}))),
        (Method::PUT, "/api/usulan-kegiatan/1", Some(json!({"perihal": "x"}))),
        (Method::DELETE, "/api/usulan-kegiatan/1", None),
    ];
    for (method, uri, body) in cases {
        let label = format!("{method} {uri}");
        let res = send(&pool, method, uri, None, body).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{label}");
        assert_eq!(res.json(), json!({"message": "Unauthenticated."}), "{label}");
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_dokumen_dengan_audit_dan_notifikasi_admin() {
    use_storage();
    let pool = connect().await;
    let tes = "store";
    let penanda = penanda(tes);
    let email = surel(tes, "admin");
    let (admin, token) = user_token(&pool, &email, true).await;
    let (kec, desa) = lokasi(&pool).await;
    let kec = kec.to_string();
    let desa = desa.to_string();
    let perihal = format!("{penanda} perihal");

    let fields = [
        ("sub_bidang", "sanitasi"),
        ("nama_pengusul", penanda.as_str()),
        ("kecamatan_id", kec.as_str()),
        ("desa_id", desa.as_str()),
        ("perihal", perihal.as_str()),
        ("ringkasan", "Ringkasan uji"),
        ("tanggal_surat_masuk", "2026-03-01"),
        ("nomor_surat_masuk", "UJI/STORE/1"),
        ("tanggal_surat", "2026-02-28"),
    ];
    let res = send_multipart(
        &pool,
        Method::POST,
        USULAN,
        Some(token.as_str()),
        &fields,
        Some(("dokumen", "uji-uk.pdf", DOKUMEN)),
    )
    .await;
    // Laravel mengembalikan resource dengan status 200, bukan 201.
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    let id = body["data"]["id"].as_i64().expect("id usulan");
    assert_eq!(body["data"]["user_id"], json!(admin));
    assert_eq!(body["data"]["sub_bidang"], json!("sanitasi"));
    assert_eq!(body["data"]["perihal"], json!(perihal));
    assert_eq!(body["data"]["ringkasan"], json!("Ringkasan uji"));
    assert!(body["data"]["user"].is_object(), "{body}");
    assert!(
        body["data"]["dokumen_url"]
            .as_str()
            .unwrap_or("")
            .contains("/storage/"),
        "{body}"
    );

    // Baris dan berkas tersimpan.
    let (db_perihal, db_sub) = usulan_row(&pool, id).await.expect("baris usulan");
    assert_eq!(db_perihal, perihal);
    assert_eq!(db_sub, "sanitasi");
    assert_eq!(media_ids(&pool, id).await.len(), 1);

    // Audit `created` memuat atribut baru.
    let audit = audit_rows(&pool, id).await;
    assert_eq!(audit.len(), 1, "{audit:?}");
    assert_eq!(audit[0].0, "created");
    let new: Value = serde_json::from_str(audit[0].2.as_deref().expect("new_values")).unwrap();
    assert_eq!(new["perihal"], json!(perihal));
    assert_eq!(new["nama_pengusul"], json!(penanda));

    // Notifikasi ke semua admin, termasuk pelaku: pengguna uji ini satu-satunya yang diuji di sini.
    let notif = notifikasi(&pool, admin, &penanda).await;
    assert_eq!(notif.len(), 1, "{notif:?}");
    assert_eq!(notif[0]["title"], json!("Usulan Kegiatan Baru"));
    assert_eq!(notif[0]["type"], json!("info"));
    assert_eq!(notif[0]["url"], json!(format!("/usulan-kegiatan?id={id}")));
    assert_eq!(
        notif[0]["message"],
        json!(format!("Usulan baru \"{perihal}\" telah diajukan oleh {penanda}"))
    );

    cleanup(&pool, &penanda, &[id], &[email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn daftar_admin_semua_non_admin_hanya_miliknya() {
    let pool = connect().await;
    let tes = "daftar";
    let penanda = penanda(tes);
    let admin_email = surel(tes, "admin");
    let user_email = surel(tes, "user");
    let (admin, admin_token) = user_token(&pool, &admin_email, true).await;
    let (owner, owner_token) = user_token(&pool, &user_email, false).await;
    let milik_admin = insert_usulan(&pool, admin, "sanitasi", &penanda, &format!("{penanda} A")).await;
    let milik_user = insert_usulan(&pool, owner, "air minum", &penanda, &format!("{penanda} B")).await;
    let uri = format!("{USULAN}?search={penanda}");

    // Admin melihat kedua baris.
    let res = send(&pool, Method::GET, &uri, Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["meta"]["total"], json!(2));
    assert_eq!(res.json()["data"].as_array().unwrap().len(), 2);

    // Non-admin hanya melihat miliknya sendiri.
    let res = send(&pool, Method::GET, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["meta"]["total"], json!(1));
    assert_eq!(res.json()["data"][0]["id"], json!(milik_user));

    // Filter `sub_bidang` dan paginasi (per_page=1, halaman 2 dari 2).
    let uri = format!("{USULAN}?search={penanda}&sub_bidang=air%20minum");
    let res = send(&pool, Method::GET, &uri, Some(admin_token.as_str()), None).await;
    assert_eq!(res.json()["meta"]["total"], json!(1));
    assert_eq!(res.json()["data"][0]["id"], json!(milik_user));

    let uri = format!("{USULAN}?search={penanda}&per_page=1&page=2");
    let res = send(&pool, Method::GET, &uri, Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["data"].as_array().unwrap().len(), 1);
    assert_eq!(res.json()["meta"]["current_page"], json!(2));
    assert_eq!(res.json()["meta"]["last_page"], json!(2));
    assert_eq!(res.json()["meta"]["total"], json!(2));

    cleanup(
        &pool,
        &penanda,
        &[milik_admin, milik_user],
        &[admin_email.as_str(), user_email.as_str()],
    )
    .await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn show_dan_akses_non_admin() {
    let pool = connect().await;
    let tes = "show";
    let penanda = penanda(tes);
    let admin_email = surel(tes, "admin");
    let user_email = surel(tes, "user");
    let (admin, admin_token) = user_token(&pool, &admin_email, true).await;
    let (owner, owner_token) = user_token(&pool, &user_email, false).await;
    let milik_admin = insert_usulan(&pool, admin, "sanitasi", &penanda, &format!("{penanda} A")).await;
    let milik_user = insert_usulan(&pool, owner, "air minum", &penanda, &format!("{penanda} B")).await;

    // Non-admin: miliknya boleh, milik admin 403 `Forbidden`.
    let uri = format!("{USULAN}/{milik_user}");
    let res = send(&pool, Method::GET, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["data"]["id"], json!(milik_user));
    assert_eq!(res.json()["data"]["user_id"], json!(owner));

    let uri = format!("{USULAN}/{milik_admin}");
    let res = send(&pool, Method::GET, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.json(), json!({"message": "Forbidden"}));

    // Admin melihat milik siapa pun.
    let res = send(&pool, Method::GET, &uri, Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["data"]["id"], json!(milik_admin));

    // Id tidak ada atau bukan angka: 404.
    let res = send(&pool, Method::GET, &format!("{USULAN}/999999999"), Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.json(), json!({"message": "Not Found."}));
    let res = send(&pool, Method::GET, &format!("{USULAN}/abc"), Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);

    cleanup(
        &pool,
        &penanda,
        &[milik_admin, milik_user],
        &[admin_email.as_str(), user_email.as_str()],
    )
    .await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn update_json_dengan_audit_dan_ganti_dokumen() {
    use_storage();
    let pool = connect().await;
    let tes = "update";
    let penanda = penanda(tes);
    let email = surel(tes, "admin");
    let (admin, token) = user_token(&pool, &email, true).await;
    let id = insert_usulan(&pool, admin, "sanitasi", &penanda, &format!("{penanda} awal")).await;
    let uri = format!("{USULAN}/{id}");
    let baru = format!("{penanda} diubah");

    // PUT JSON: `ringkasan` dikirim tetapi tidak disimpan (seperti `only()` di Laravel).
    let res = send(
        &pool,
        Method::PUT,
        &uri,
        Some(token.as_str()),
        Some(json!({
            "perihal": baru,
            "sub_bidang": "air minum",
            "ringkasan": "tidak disimpan",
            "tanggal_surat": "2026-04-10"
        })),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["data"]["perihal"], json!(baru));
    assert_eq!(res.json()["data"]["sub_bidang"], json!("air minum"));
    assert_eq!(res.json()["data"]["tanggal_surat"], json!("2026-04-10"));

    let (perihal, sub) = usulan_row(&pool, id).await.expect("baris usulan");
    assert_eq!(perihal, baru);
    assert_eq!(sub, "air minum");
    let ringkasan: String = sqlx::query_scalar("SELECT ringkasan FROM tbl_usulan_kegiatan WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ringkasan, "");

    // Audit `updated` hanya memuat kolom yang berubah.
    let audit = audit_rows(&pool, id).await;
    let updated: Vec<_> = audit.iter().filter(|a| a.0 == "updated").collect();
    assert_eq!(updated.len(), 1, "{audit:?}");
    let old: Value = serde_json::from_str(updated[0].1.as_deref().expect("old_values")).unwrap();
    let new: Value = serde_json::from_str(updated[0].2.as_deref().expect("new_values")).unwrap();
    assert_eq!(old["perihal"], json!(format!("{penanda} awal")));
    assert_eq!(new["perihal"], json!(baru));
    assert_eq!(new["sub_bidang"], json!("air minum"));
    assert!(new.get("ringkasan").is_none(), "{new}");

    // Nilai yang sama tidak menambah audit.
    let res = send(&pool, Method::PUT, &uri, Some(token.as_str()), Some(json!({"perihal": baru}))).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let updated_now = audit_rows(&pool, id).await.iter().filter(|a| a.0 == "updated").count();
    assert_eq!(updated_now, 1);

    // Validasi tetap berjalan pada update.
    let res = send(&pool, Method::PUT, &uri, Some(token.as_str()), Some(json!({"sub_bidang": "lainnya"}))).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["errors"]["sub_bidang"][0], json!("The selected sub bidang is invalid."));

    // PUT multipart mengganti dokumen: media lama dihapus, media baru dibuat.
    let res = send_multipart(
        &pool,
        Method::PUT,
        &uri,
        Some(token.as_str()),
        &[],
        Some(("dokumen", "uji-uk-1.pdf", DOKUMEN)),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let m1 = media_ids(&pool, id).await;
    assert_eq!(m1.len(), 1);

    let res = send_multipart(
        &pool,
        Method::PUT,
        &uri,
        Some(token.as_str()),
        &[],
        Some(("dokumen", "uji-uk-2.pdf", DOKUMEN)),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let m2 = media_ids(&pool, id).await;
    assert_eq!(m2.len(), 1);
    assert_ne!(m1, m2);

    cleanup(&pool, &penanda, &[id], &[email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn hapus_dengan_audit() {
    let pool = connect().await;
    let tes = "hapus";
    let penanda = penanda(tes);
    let email = surel(tes, "admin");
    let (admin, token) = user_token(&pool, &email, true).await;
    let id = insert_usulan(&pool, admin, "sanitasi", &penanda, &format!("{penanda} hapus")).await;
    let uri = format!("{USULAN}/{id}");

    let res = send(&pool, Method::DELETE, &uri, Some(token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json(), json!({"message": "Usulan kegiatan berhasil dihapus."}));
    assert!(usulan_row(&pool, id).await.is_none());

    // Audit `deleted` memuat atribut sebelum hapus.
    let audit = audit_rows(&pool, id).await;
    let deleted: Vec<_> = audit.iter().filter(|a| a.0 == "deleted").collect();
    assert_eq!(deleted.len(), 1, "{audit:?}");
    let old: Value = serde_json::from_str(deleted[0].1.as_deref().expect("old_values")).unwrap();
    assert_eq!(old["nama_pengusul"], json!(penanda));

    // Hapus kedua: baris sudah tidak ada.
    let res = send(&pool, Method::DELETE, &uri, Some(token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);

    cleanup(&pool, &penanda, &[id], &[email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn export_excel_mengembalikan_xlsx() {
    let pool = connect().await;
    let tes = "export";
    let penanda = penanda(tes);
    let admin_email = surel(tes, "admin");
    let user_email = surel(tes, "user");
    let (admin, admin_token) = user_token(&pool, &admin_email, true).await;
    let (owner, owner_token) = user_token(&pool, &user_email, false).await;
    let milik_admin = insert_usulan(&pool, admin, "sanitasi", &penanda, &format!("{penanda} A")).await;
    let milik_user = insert_usulan(&pool, owner, "air minum", &penanda, &format!("{penanda} B")).await;

    let res = send(&pool, Method::GET, EXPORT, Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.header_str(header::CONTENT_TYPE), XLSX_MIME);
    assert!(
        res.header_str(header::CONTENT_DISPOSITION)
            .contains("rekap_usulan_kegiatan.xlsx"),
        "{:?}",
        res.headers
    );
    // Berkas xlsx adalah arsip zip.
    assert!(res.body.starts_with(b"PK\x03\x04"));
    let xml = shared_strings(&res.body);
    assert!(xml.contains(&penanda), "{xml}");
    assert!(xml.contains("Sub Bidang"));
    assert!(xml.contains("Dokumen"));

    // Export tidak memakai scope (sama dengan Laravel): non-admin juga mendapat semua baris.
    let res = send(&pool, Method::GET, EXPORT, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert!(shared_strings(&res.body).contains(&penanda));

    cleanup(
        &pool,
        &penanda,
        &[milik_admin, milik_user],
        &[admin_email.as_str(), user_email.as_str()],
    )
    .await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_422() {
    let pool = connect().await;
    let tes = "validasi";
    let penanda = penanda(tes);
    let email = surel(tes, "admin");
    let (_admin, token) = user_token(&pool, &email, true).await;
    let (_kec, desa) = lokasi(&pool).await;

    // Semua field wajib kosong: satu pesan per field, bentuk `Validator::errors()`.
    let res = send(&pool, Method::POST, USULAN, Some(token.as_str()), Some(json!({}))).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    let body = res.json();
    assert_eq!(body["message"], json!("Validation error"));
    assert_eq!(body["errors"]["sub_bidang"][0], json!("The sub bidang field is required."));
    assert_eq!(body["errors"]["nama_pengusul"][0], json!("The nama pengusul field is required."));
    assert_eq!(body["errors"]["kecamatan_id"][0], json!("The kecamatan id field is required."));
    assert_eq!(body["errors"]["desa_id"][0], json!("The desa id field is required."));
    assert_eq!(body["errors"]["perihal"][0], json!("The perihal field is required."));
    assert_eq!(body["errors"]["tanggal_surat_masuk"][0], json!("The tanggal surat masuk field is required."));
    assert_eq!(body["errors"]["nomor_surat_masuk"][0], json!("The nomor surat masuk field is required."));
    assert_eq!(body["errors"]["tanggal_surat"][0], json!("The tanggal surat field is required."));

    // Nilai tidak valid: enum, panjang, id yang tidak ada, dan tanggal.
    let nama_panjang = format!("{penanda}{}", "x".repeat(256));
    let res = send(
        &pool,
        Method::POST,
        USULAN,
        Some(token.as_str()),
        Some(json!({
            "sub_bidang": "lainnya",
            "nama_pengusul": nama_panjang,
            "kecamatan_id": "999999999",
            "desa_id": desa.to_string(),
            "perihal": penanda,
            "tanggal_surat_masuk": "31-12-2026",
            "nomor_surat_masuk": "UJI/422",
            "tanggal_surat": "2026-01-01"
        })),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    let body = res.json();
    assert_eq!(body["errors"]["sub_bidang"][0], json!("The selected sub bidang is invalid."));
    assert_eq!(
        body["errors"]["nama_pengusul"][0],
        json!("The nama pengusul field must not be greater than 255 characters.")
    );
    assert_eq!(body["errors"]["kecamatan_id"][0], json!("The selected kecamatan id is invalid."));
    assert_eq!(
        body["errors"]["tanggal_surat_masuk"][0],
        json!("The tanggal surat masuk field must be a valid date.")
    );
    assert!(body["errors"].get("desa_id").is_none(), "{body}");

    // Berkas dengan ekstensi di luar daftar.
    let fields = [
        ("sub_bidang", "sanitasi"),
        ("nama_pengusul", penanda.as_str()),
        ("kecamatan_id", "1"),
        ("desa_id", "1"),
        ("perihal", penanda.as_str()),
        ("tanggal_surat_masuk", "2026-03-01"),
        ("nomor_surat_masuk", "UJI/422"),
        ("tanggal_surat", "2026-02-28"),
    ];
    let res = send_multipart(
        &pool,
        Method::POST,
        USULAN,
        Some(token.as_str()),
        &fields,
        Some(("dokumen", "uji-uk.exe", &b"MZ uji"[..])),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(
        res.json()["errors"]["dokumen"][0],
        json!("The dokumen field must be a file of type: pdf, doc, docx, xls, xlsx, png, jpg, jpeg.")
    );

    // Tidak ada baris yang tersimpan dari permintaan yang gagal validasi.
    assert_eq!(usulan_count_by_penanda(&pool, &penanda).await, 0);

    cleanup(&pool, &penanda, &[], &[email.as_str()]).await;
}
