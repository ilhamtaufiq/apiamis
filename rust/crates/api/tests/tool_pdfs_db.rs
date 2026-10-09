//! Rute `/api/tool-pdfs` lewat router terhadap MySQL: 401 tanpa token, store (unggah PDF multipart beserta
//! placement tanda tangan), daftar milik user, sign, bulk download (zip), dan hapus (soft delete).
//!
//! Skema: `rust/fixtures/tool_pdfs_schema.sql` (tabel `tool_pdfs` dan `tool_pdf_signature_placements`).
//! Tabel lain yang dipakai: `users`, `roles`, `model_has_roles`, `personal_access_tokens`, `media`, dan
//! `tbl_audit_logs`. Berkas disimpan di `PUBLIC_STORAGE_PATH`, yang diarahkan ke direktori sementara tes.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test tool_pdfs_db -- --include-ignored
//! ```
//!
//! Setiap tes memakai penanda `uji-tp-<tes>-<pid>` di nama berkas dan email `uji-tp-<peran>-<tes>-<pid>@example.test`.
//! Cleanup hanya menghapus baris yang dicatat tes ini (id tool_pdfs, audit, media beserta berkasnya, dan akun uji).

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

const TP: &str = "/api/tool-pdfs";
const BOUNDARY: &str = "----ujitpboundary";
/// Nilai `model_type` seperti yang ditulis Laravel.
const MODEL: &str = "App\\Models\\ToolPdf";
const USER_MODEL: &str = "App\\Models\\User";
/// UUID 36 karakter untuk `signature_id` (kolom `char(36)`).
const SIGNATURE_ID: &str = "4f6c1a2e-7b3d-4e8a-9c1f-2d5b6a7e8f90";
const FORMAT_MESSAGE: &str = "Format placement tanda tangan tidak valid";

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
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: application/pdf\r\n\r\n"
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

/// Penanda unik per tes dan proses, dipakai di nama berkas dan nama tampilan.
fn penanda(tes: &str) -> String {
    format!("uji-tp-{tes}-{}", std::process::id())
}

fn surel(tes: &str, peran: &str) -> String {
    format!("uji-tp-{peran}-{tes}-{}@example.test", std::process::id())
}

/// Satu direktori media untuk semua tes di berkas ini. Nilainya sama di setiap tes, jadi tes paralel tidak berbeda.
fn use_storage() {
    let dir = std::env::temp_dir().join(format!("uji-tp-storage-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", dir);
}

/// PDF kecil yang valid: satu halaman kosong dengan tabel xref. `label` membuat isinya berbeda per berkas.
fn tiny_pdf(label: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ];
    let mut pdf = format!("%PDF-1.4\n% {label}\n");
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{obj}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in &offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

/// Satu placement tanda tangan yang lolos validasi.
fn placement(nama: &str) -> Value {
    json!({
        "signature_id": SIGNATURE_ID,
        "page_number": 1,
        "x_ratio": 0.5,
        "y_ratio": 0.25,
        "scale": 0.2,
        "signature_name": nama,
        "signature_file_name": "ttd.png",
        "signature_mime_type": "image/png",
        "signature_width": 120,
        "signature_height": 60,
        "signature_data_url": "data:image/png;base64,iVBORw0KGgo=",
        "signature_source_type": "upload",
        "signature_source_id": "uji-tp-ttd"
    })
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji TP', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(pool, uid, "uji-tp").await.unwrap();
    (uid, token)
}

/// `(kind, name, sudah dihapus)` sebuah baris `tool_pdfs`, termasuk yang soft-deleted.
async fn pdf_state(pool: &MySqlPool, id: i64) -> Option<(String, String, i64)> {
    sqlx::query_as::<_, (String, String, i64)>(
        "SELECT kind, name, CAST(deleted_at IS NOT NULL AS SIGNED) FROM tool_pdfs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn parent_of(pool: &MySqlPool, id: i64) -> Option<u64> {
    sqlx::query_scalar::<_, Option<u64>>("SELECT parent_id FROM tool_pdfs WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn placement_count(pool: &MySqlPool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM tool_pdf_signature_placements WHERE tool_pdf_id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn media_ids(pool: &MySqlPool, id: i64) -> Vec<u64> {
    sqlx::query_scalar(
        "SELECT id FROM media WHERE model_type = ? AND model_id = ? AND collection_name = 'pdf' ORDER BY id",
    )
    .bind(MODEL)
    .bind(id as u64)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Baris audit `(event, old_values, new_values)` untuk satu `ToolPdf`, urut id.
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

/// Hapus hanya baris yang dicatat tes ini: audit, placement, media (beserta berkasnya), tool_pdfs,
/// lalu akun uji. Anak (sign) dihapus lebih dulu karena id-nya lebih besar.
async fn cleanup(pool: &MySqlPool, pdf_ids: &[i64], emails: &[&str]) {
    let mut ids = pdf_ids.to_vec();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    for id in &ids {
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
        sqlx::query("DELETE FROM tool_pdf_signature_placements WHERE tool_pdf_id = ?")
            .bind(*id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tool_pdfs WHERE id = ?")
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

/// Semua id media milik `ToolPdf` (termasuk yang soft-deleted), untuk cleanup.
async fn media_ids_all(pool: &MySqlPool, id: i64) -> Vec<u64> {
    sqlx::query_scalar("SELECT id FROM media WHERE model_type = ? AND model_id = ?")
        .bind(MODEL)
        .bind(id as u64)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Entri zip sebagai `(nama, isi)`, urut sesuai arsip.
fn zip_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("arsip zip");
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.push((name, data));
    }
    out
}

/// Simpan satu PDF lewat `POST /api/tool-pdfs` dan kembalikan `(id, respons)`.
async fn store_pdf(
    pool: &MySqlPool,
    token: &str,
    filename: &str,
    name: Option<&str>,
    bytes: &[u8],
) -> i64 {
    let mut fields: Vec<(&str, &str)> = Vec::new();
    if let Some(n) = name {
        fields.push(("name", n));
    }
    let res = send_multipart(
        pool,
        Method::POST,
        TP,
        Some(token),
        &fields,
        Some(("file", filename, bytes)),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.text());
    res.json()["data"]["id"]
        .as_str()
        .expect("id berupa string")
        .parse()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Tes
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tanpa_token_401() {
    use_storage();
    let pool = connect().await;
    let pdf = tiny_pdf("tanpa-token");
    let body = Some(json!({"ids": [1]}));

    // Store dan sign membaca multipart dulu, jadi body multipart dikirim agar sampai ke pemeriksaan auth.
    for uri in [TP, "/api/tool-pdfs/sign"] {
        let res = send_multipart(
            &pool,
            Method::POST,
            uri,
            None,
            &[("name", "uji-tp-tanpa-token")],
            Some(("file", "uji-tp-tanpa-token.pdf", pdf.as_slice())),
        )
        .await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "POST {uri}: {}", res.text());
        assert_eq!(res.json(), json!({"message": "Unauthenticated."}), "POST {uri}");
    }

    let cases: [(Method, &str, Option<Value>); 4] = [
        (Method::GET, TP, None),
        (Method::POST, "/api/tool-pdfs/bulk-download", body),
        (Method::GET, "/api/tool-pdfs/1/download", None),
        (Method::DELETE, "/api/tool-pdfs/1", None),
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
async fn store_dengan_placement_download_dan_audit() {
    use_storage();
    let pool = connect().await;
    let tes = "store";
    let penanda = penanda(tes);
    let email = surel(tes, "user");
    let (owner, token) = user_token(&pool, &email, false).await;
    let filename = format!("{penanda}-kontrak.pdf");
    let pdf = tiny_pdf(&penanda);
    let placements = json!([placement("Uji TTD")]).to_string();

    // Store tanpa `name`: nama diambil dari nama berkas tanpa ekstensi.
    let res = send_multipart(
        &pool,
        Method::POST,
        TP,
        Some(token.as_str()),
        &[("placements", placements.as_str())],
        Some(("file", filename.as_str(), pdf.as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.text());
    let body = res.json();
    let id: i64 = body["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(body["data"]["name"], json!(penanda.clone() + "-kontrak"));
    assert_eq!(body["data"]["original_filename"], json!(filename));
    assert_eq!(body["data"]["kind"], json!("source"));
    assert_eq!(body["data"]["parent_id"], Value::Null);
    assert!(body["data"]["pdf_url"].as_str().unwrap_or("").contains("/storage/"), "{body}");
    let ps = body["data"]["signature_placements"].as_array().expect("placements");
    assert_eq!(ps.len(), 1, "{body}");
    assert_eq!(ps[0]["signature_id"], json!(SIGNATURE_ID));
    assert_eq!(ps[0]["signature_name"], json!("Uji TTD"));
    assert_eq!(ps[0]["page_number"], json!(1));
    assert_eq!(ps[0]["x_ratio"], json!(0.5));
    assert_eq!(ps[0]["sort_order"], json!(0));
    assert_eq!(ps[0]["signature_source_type"], json!("upload"));

    // Baris, placement, media, dan audit `created`.
    let (kind, name, deleted) = pdf_state(&pool, id).await.expect("baris tool_pdfs");
    assert_eq!(kind, "source");
    assert_eq!(name, penanda.clone() + "-kontrak");
    assert_eq!(deleted, 0);
    assert_eq!(placement_count(&pool, id).await, 1);
    assert_eq!(media_ids(&pool, id).await.len(), 1);
    let audit = audit_rows(&pool, id).await;
    assert_eq!(audit.len(), 1, "{audit:?}");
    assert_eq!(audit[0].0, "created");
    let new: Value = serde_json::from_str(audit[0].2.as_deref().unwrap()).unwrap();
    assert_eq!(new["user_id"], json!(owner));

    // Unduh berkas: isi sama dengan yang diunggah.
    let res = send(&pool, Method::GET, &format!("{TP}/{id}/download"), Some(token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert!(res.header_str(header::CONTENT_TYPE).starts_with("application/pdf"));
    assert_eq!(res.body, pdf);

    // Placement tidak valid (bukan array JSON): 422 dengan pesan format, tidak ada baris baru.
    let gagal = format!("{penanda}-gagal.pdf");
    let res = send_multipart(
        &pool,
        Method::POST,
        TP,
        Some(token.as_str()),
        &[("placements", "bukan json")],
        Some(("file", gagal.as_str(), pdf.as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json(), json!({"message": FORMAT_MESSAGE}));

    // Item placement tidak valid (page_number 0): 422 per field.
    let mut salah = placement("Uji TTD");
    salah["page_number"] = json!(0);
    let placements_salah = json!([salah]).to_string();
    let res = send_multipart(
        &pool,
        Method::POST,
        TP,
        Some(token.as_str()),
        &[("placements", placements_salah.as_str())],
        Some(("file", gagal.as_str(), pdf.as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["message"], json!("The given data was invalid."));
    assert!(res.json()["errors"]["placements.0.page_number"].is_array(), "{}", res.text());

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_pdfs WHERE original_filename = ?")
        .bind(&gagal)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    cleanup(&pool, &[id], &[email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_validasi_file() {
    use_storage();
    let pool = connect().await;
    let tes = "validasi";
    let penanda = penanda(tes);
    let email = surel(tes, "user");
    let (_owner, token) = user_token(&pool, &email, false).await;

    // Tanpa berkas.
    let res = send_multipart(&pool, Method::POST, TP, Some(token.as_str()), &[("name", "tanpa-berkas")], None).await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["message"], json!("The given data was invalid."));
    assert_eq!(res.json()["errors"]["file"][0], json!("The file field is required."));

    // Ekstensi .pdf tetapi isi bukan PDF.
    let palsu = format!("{penanda}-palsu.pdf");
    let res = send_multipart(
        &pool,
        Method::POST,
        TP,
        Some(token.as_str()),
        &[],
        Some(("file", palsu.as_str(), &b"bukan pdf"[..])),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["errors"]["file"][0], json!("The file field must be a file of type: pdf."));

    // Ekstensi lain.
    let teks = format!("{penanda}.txt");
    let pdf = tiny_pdf(&penanda);
    let res = send_multipart(
        &pool,
        Method::POST,
        TP,
        Some(token.as_str()),
        &[],
        Some(("file", teks.as_str(), pdf.as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["errors"]["file"][0], json!("The file field must be a file of type: pdf."));

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_pdfs WHERE original_filename LIKE ?")
        .bind(format!("{penanda}%"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    cleanup(&pool, &[], &[email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn daftar_hanya_milik_user_termasuk_admin() {
    use_storage();
    let pool = connect().await;
    let tes = "daftar";
    let penanda = penanda(tes);
    let owner_email = surel(tes, "user");
    let admin_email = surel(tes, "admin");
    let (_owner, owner_token) = user_token(&pool, &owner_email, false).await;
    let (_admin, admin_token) = user_token(&pool, &admin_email, true).await;

    let milik_user = store_pdf(&pool, owner_token.as_str(), &format!("{penanda}-user.pdf"), None, &tiny_pdf("user")).await;
    let milik_admin = store_pdf(&pool, admin_token.as_str(), &format!("{penanda}-admin.pdf"), None, &tiny_pdf("admin")).await;

    // Index hanya memuat milik pengguna yang login, termasuk untuk admin.
    let uri = format!("{TP}?search={penanda}");
    let res = send(&pool, Method::GET, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let data = res.json()["data"].as_array().unwrap().clone();
    assert_eq!(data.len(), 1, "{data:?}");
    assert_eq!(data[0]["id"], json!(milik_user.to_string()));
    assert_eq!(data[0]["kind"], json!("source"));

    let res = send(&pool, Method::GET, &uri, Some(admin_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let data = res.json()["data"].as_array().unwrap().clone();
    assert_eq!(data.len(), 1, "{data:?}");
    assert_eq!(data[0]["id"], json!(milik_admin.to_string()));

    // Filter `kind`: `signed` kosong, `all` dan `source` memuat berkas sumber.
    let res = send(&pool, Method::GET, &format!("{uri}&kind=signed"), Some(owner_token.as_str()), None).await;
    assert_eq!(res.json()["data"].as_array().unwrap().len(), 0);
    let res = send(&pool, Method::GET, &format!("{uri}&kind=all"), Some(owner_token.as_str()), None).await;
    assert_eq!(res.json()["data"].as_array().unwrap().len(), 1);
    let res = send(&pool, Method::GET, &format!("{uri}&kind=source"), Some(owner_token.as_str()), None).await;
    assert_eq!(res.json()["data"].as_array().unwrap().len(), 1);

    cleanup(
        &pool,
        &[milik_user, milik_admin],
        &[owner_email.as_str(), admin_email.as_str()],
    )
    .await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sign_dengan_induk_dan_placement() {
    use_storage();
    let pool = connect().await;
    let tes = "sign";
    let penanda = penanda(tes);
    let owner_email = surel(tes, "user");
    let admin_email = surel(tes, "admin");
    let (_owner, owner_token) = user_token(&pool, &owner_email, false).await;
    let (_admin, admin_token) = user_token(&pool, &admin_email, true).await;

    let sumber = store_pdf(&pool, owner_token.as_str(), &format!("{penanda}-sumber.pdf"), None, &tiny_pdf("sumber")).await;
    let milik_admin = store_pdf(&pool, admin_token.as_str(), &format!("{penanda}-admin.pdf"), None, &tiny_pdf("admin")).await;

    // Sign dengan `source_id` milik sendiri: `parent_id` menunjuk sumber, kind `signed`.
    let signed_pdf = tiny_pdf("ttd");
    let placements = json!([placement("Uji TTD")]).to_string();
    let sumber_s = sumber.to_string();
    let nama_signed = format!("{penanda}-ttd.pdf");
    let res = send_multipart(
        &pool,
        Method::POST,
        "/api/tool-pdfs/sign",
        Some(owner_token.as_str()),
        &[("placements", placements.as_str()), ("source_id", sumber_s.as_str())],
        Some(("file", nama_signed.as_str(), signed_pdf.as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.text());
    let body = res.json();
    let signed: i64 = body["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(body["data"]["kind"], json!("signed"));
    assert_eq!(body["data"]["parent_id"], json!(sumber.to_string()));
    assert_eq!(body["data"]["signature_placements"].as_array().unwrap().len(), 1);
    assert_eq!(parent_of(&pool, signed).await, Some(sumber as u64));
    assert_eq!(placement_count(&pool, signed).await, 1);
    assert_eq!(pdf_state(&pool, signed).await.unwrap().0, "signed");
    assert_eq!(audit_rows(&pool, signed).await[0].0, "created");

    // `source_id` milik user lain: 422.
    let milik_admin_s = milik_admin.to_string();
    let res = send_multipart(
        &pool,
        Method::POST,
        "/api/tool-pdfs/sign",
        Some(owner_token.as_str()),
        &[("source_id", milik_admin_s.as_str())],
        Some(("file", "uji-tp-lain.pdf", tiny_pdf("lain").as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["errors"]["source_id"][0], json!("The selected source id is invalid."));

    // Sign tanpa `source_id`: tidak ada induk.
    let tanpa_induk = format!("{penanda}-mandiri.pdf");
    let res = send_multipart(
        &pool,
        Method::POST,
        "/api/tool-pdfs/sign",
        Some(owner_token.as_str()),
        &[],
        Some(("file", tanpa_induk.as_str(), tiny_pdf("mandiri").as_slice())),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.text());
    let mandiri: i64 = res.json()["data"]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(parent_of(&pool, mandiri).await, None);

    cleanup(
        &pool,
        &[sumber, milik_admin, signed, mandiri],
        &[owner_email.as_str(), admin_email.as_str()],
    )
    .await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn bulk_download_zip_dan_kepemilikan() {
    use_storage();
    let pool = connect().await;
    let tes = "bulk";
    let penanda = penanda(tes);
    let owner_email = surel(tes, "user");
    let admin_email = surel(tes, "admin");
    let (_owner, owner_token) = user_token(&pool, &owner_email, false).await;
    let (_admin, admin_token) = user_token(&pool, &admin_email, true).await;

    let nama_a = format!("{penanda}-a");
    let nama_b = format!("{penanda}-b");
    let pdf_a = tiny_pdf("a");
    let pdf_b = tiny_pdf("b");
    let a = store_pdf(&pool, owner_token.as_str(), &format!("{nama_a}.pdf"), Some(nama_a.as_str()), &pdf_a).await;
    let b = store_pdf(&pool, owner_token.as_str(), &format!("{nama_b}.pdf"), Some(nama_b.as_str()), &pdf_b).await;

    // Zip berisi kedua berkas, diberi nomor urut dan nama dari `name` (slug, `_`).
    let res = send(
        &pool,
        Method::POST,
        "/api/tool-pdfs/bulk-download",
        Some(owner_token.as_str()),
        Some(json!({"ids": [a, b]})),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.header_str(header::CONTENT_TYPE), "application/zip");
    assert!(res.header_str(header::CONTENT_DISPOSITION).contains("tool-pdfs-bulk.zip"));
    assert!(res.body.starts_with(b"PK\x03\x04"));
    let entries = zip_entries(&res.body);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0].0, format!("01_{}.pdf", nama_a.replace('-', "_")));
    assert_eq!(entries[0].1, pdf_a);
    assert_eq!(entries[1].0, format!("02_{}.pdf", nama_b.replace('-', "_")));
    assert_eq!(entries[1].1, pdf_b);

    // Id milik user lain ditolak validasi (sama dengan `Rule::exists` ber-`where user_id`).
    let res = send(
        &pool,
        Method::POST,
        "/api/tool-pdfs/bulk-download",
        Some(admin_token.as_str()),
        Some(json!({"ids": [a]})),
    )
    .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
    assert_eq!(res.json()["errors"]["ids.0"][0], json!("The selected ids.0 is invalid."));

    // Daftar kosong dan `ids` yang hilang: 422.
    for body in [json!({"ids": []}), json!({})] {
        let res = send(
            &pool,
            Method::POST,
            "/api/tool-pdfs/bulk-download",
            Some(owner_token.as_str()),
            Some(body),
        )
        .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", res.text());
        assert!(res.json()["errors"]["ids"].is_array());
    }

    cleanup(&pool, &[a, b], &[owner_email.as_str(), admin_email.as_str()]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn hapus_soft_delete_dan_akses() {
    use_storage();
    let pool = connect().await;
    let tes = "hapus";
    let penanda = penanda(tes);
    let owner_email = surel(tes, "user");
    let other_email = surel(tes, "lain");
    let admin_email = surel(tes, "admin");
    let (_owner, owner_token) = user_token(&pool, &owner_email, false).await;
    let (_other, other_token) = user_token(&pool, &other_email, false).await;
    let (_admin, admin_token) = user_token(&pool, &admin_email, true).await;

    let milik = store_pdf(&pool, owner_token.as_str(), &format!("{penanda}-hapus.pdf"), None, &tiny_pdf("hapus")).await;
    let untuk_admin = store_pdf(&pool, owner_token.as_str(), &format!("{penanda}-admin.pdf"), None, &tiny_pdf("admin-hapus")).await;
    let uri = format!("{TP}/{milik}");

    // Pengguna lain: 403 dengan pesan dari controller.
    let res = send(&pool, Method::DELETE, &uri, Some(other_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN, "{}", res.text());
    assert_eq!(res.json(), json!({"message": "Anda tidak memiliki akses untuk menghapus file ini"}));

    // Pemilik: soft delete. Baris, placement, dan media tetap ada.
    let res = send(&pool, Method::DELETE, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json(), json!({"success": true, "message": "File PDF berhasil dihapus"}));
    let (_, _, deleted) = pdf_state(&pool, milik).await.expect("baris tetap ada");
    assert_eq!(deleted, 1);
    assert_eq!(media_ids(&pool, milik).await.len(), 1);

    // Setelah dihapus: tidak ada di daftar, unduh 404, bulk download 404, dan hapus lagi 404.
    let res = send(&pool, Method::GET, &format!("{TP}?search={penanda}"), Some(owner_token.as_str()), None).await;
    let ids: Vec<Value> = res.json()["data"].as_array().unwrap().iter().map(|d| d["id"].clone()).collect();
    assert!(!ids.contains(&json!(milik.to_string())), "{ids:?}");
    let res = send(&pool, Method::GET, &format!("{uri}/download"), Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    let res = send(
        &pool,
        Method::POST,
        "/api/tool-pdfs/bulk-download",
        Some(owner_token.as_str()),
        Some(json!({"ids": [milik]})),
    )
    .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "{}", res.text());
    assert_eq!(res.json(), json!({"message": "File PDF tidak ditemukan"}));
    let res = send(&pool, Method::DELETE, &uri, Some(owner_token.as_str()), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);

    // Audit `deleted` memuat atribut sebelum hapus, dengan `deleted_at` terisi.
    let audit = audit_rows(&pool, milik).await;
    let deleted_audit: Vec<_> = audit.iter().filter(|a| a.0 == "deleted").collect();
    assert_eq!(deleted_audit.len(), 1, "{audit:?}");
    let old: Value = serde_json::from_str(deleted_audit[0].1.as_deref().unwrap()).unwrap();
    assert_eq!(old["kind"], json!("source"));
    assert!(old["deleted_at"].is_string(), "{old}");

    // Admin boleh menghapus milik user lain.
    let res = send(
        &pool,
        Method::DELETE,
        &format!("{TP}/{untuk_admin}"),
        Some(admin_token.as_str()),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());

    cleanup(
        &pool,
        &[milik, untuk_admin],
        &[owner_email.as_str(), other_email.as_str(), admin_email.as_str()],
    )
    .await;
}
