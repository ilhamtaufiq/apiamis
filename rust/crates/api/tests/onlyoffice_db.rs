//! Konversi berkas ke PDF lewat ONLYOFFICE: aplikasi Rust dan Document Server tiruan berjalan di TCP.
//!
//! Document Server tiruan memverifikasi JWT, mengunduh berkas dari URL token milik aplikasi, lalu
//! menjawab dengan polling dua kali sebelum mengembalikan PDF. Tes ini memakai satu fungsi karena
//! konfigurasi lewat environment bersifat global untuk proses tes.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test onlyoffice_db -- --include-ignored
//! ```

use std::sync::{Arc, Mutex};

use api::{app, media, AppState};
use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use shared::Config;
use sqlx::MySqlPool;
use tokio::net::TcpListener;

const SECRET: &str = "uji-jwt-onlyoffice-rahasia";
const ADMIN: &str = "uji-oo-admin@example.test";

#[derive(Clone, Default)]
struct FakeDs {
    calls: Arc<Mutex<u32>>,
    output: Arc<Mutex<Vec<u8>>>,
}

/// Verifikasi JWT HS256 dan kembalikan payload-nya bila tanda tangan cocok.
fn verify_jwt(token: &str, secret: &str) -> Option<Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if expected != parts[2] {
        return None;
    }
    let body = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&body).ok()
}

async fn converter(State(ds): State<FakeDs>, Json(body): Json<Value>) -> Json<Value> {
    let Some(token) = body.get("token").and_then(Value::as_str) else {
        return Json(json!({ "error": -8 }));
    };
    let Some(payload) = verify_jwt(token, SECRET) else {
        return Json(json!({ "error": -8 }));
    };
    let source_url = payload["url"].as_str().unwrap_or_default().to_string();
    // DS mengambil berkas sumber dari aplikasi lewat URL bertoken.
    let source = reqwest::get(&source_url)
        .await
        .expect("unduh berkas sumber")
        .bytes()
        .await
        .unwrap();
    let mut out = b"%PDF-fake:".to_vec();
    out.extend_from_slice(&source);
    *ds.output.lock().unwrap() = out;

    let mut calls = ds.calls.lock().unwrap();
    *calls += 1;
    if *calls == 1 {
        return Json(json!({ "endConvert": false }));
    }
    let addr = std::env::var("ONLYOFFICE_DOCUMENT_SERVER_URL").unwrap();
    Json(json!({ "endConvert": true, "fileUrl": format!("{addr}/out.pdf") }))
}

async fn out_pdf(State(ds): State<FakeDs>) -> impl IntoResponse {
    ds.output.lock().unwrap().clone()
}

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji OO', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn berkas_export_pdf_converts_through_document_server() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = std::env::temp_dir().join(format!("uji-onlyoffice-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    std::env::set_var("ONLYOFFICE_JWT_SECRET", SECRET);
    std::env::set_var(
        "APP_KEY",
        "base64:dW5pLWFwcC1rZXktc2VjcmV0LXVuaS1hcHAta2V5MTIzNDU=",
    );

    // Document Server tiruan.
    let ds = FakeDs::default();
    let ds_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ds_addr = format!("http://{}", ds_listener.local_addr().unwrap());
    std::env::set_var("ONLYOFFICE_DOCUMENT_SERVER_URL", &ds_addr);
    let ds_app = Router::new()
        .route("/converter", post(converter))
        .route("/out.pdf", get(out_pdf))
        .with_state(ds.clone());
    tokio::spawn(async move { axum::serve(ds_listener, ds_app).await.unwrap() });

    // Aplikasi Rust di TCP, dengan APP_URL menunjuk ke alamatnya sendiri.
    let app_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app_addr = format!("http://{}", app_listener.local_addr().unwrap());
    let state = AppState::new(pool.clone(), app_addr.clone());
    let router = app(&config(), state);
    tokio::spawn(async move { axum::serve(app_listener, router).await.unwrap() });

    let admin = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, admin, "uji-oo")
        .await
        .unwrap();
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, 'uji-oo', ?, NOW(), NOW())")
        .bind(pekerjaan)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let berkas: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_berkas WHERE jenis_dokumen = 'uji-oo' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let docx = media::attach(
        &mut tx,
        "App\\Models\\Berkas",
        berkas,
        "berkas/dokumen",
        &media::Upload {
            original_name: "uji.docx".into(),
            bytes: b"DOCX-UJI".to_vec(),
        },
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        false,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let client = reqwest::Client::new();
    let export = |id: u64| format!("{app_addr}/api/berkas/{id}/export-pdf");

    // Konversi sungguhan: DS dipanggil dua kali (polling), PDF yang kembali memuat isi berkas.
    let res = client
        .get(export(berkas))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "application/pdf");
    // Nama unduhan dari `file_name` media (UUID di disk), sama dengan `getSuggestedDownloadName`.
    let disposition = res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.starts_with("attachment; filename=\"") && disposition.ends_with(".pdf\""),
        "{disposition}"
    );
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"%PDF-fake:DOCX-UJI");
    assert_eq!(*ds.calls.lock().unwrap(), 2, "polling sampai endConvert");

    // Berkas dengan ekstensi yang tidak bisa dikonversi: 500 dengan pesan dari Laravel.
    sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, 'uji-oo-exe', ?, NOW(), NOW())")
        .bind(pekerjaan)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let berkas_exe: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_berkas WHERE jenis_dokumen = 'uji-oo-exe' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    media::attach(
        &mut tx,
        "App\\Models\\Berkas",
        berkas_exe,
        "berkas/dokumen",
        &media::Upload {
            original_name: "uji.exe".into(),
            bytes: b"MZ".to_vec(),
        },
        "application/octet-stream",
        false,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let exe = client
        .get(export(berkas_exe))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(exe.status(), 500);
    let exe_body: Value = exe.json().await.unwrap();
    assert!(
        exe_body["message"]
            .as_str()
            .unwrap()
            .starts_with("Gagal mengonversi berkas ke PDF"),
        "{exe_body}"
    );

    // Unduhan untuk Document Server: token salah, token kedaluwarsa, dan token benar.
    let media_id = docx.media_id;
    let bad = client
        .get(format!(
            "{app_addr}/api/onlyoffice/media/{media_id}/download?expires=9999999999&token=salah"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 403);
    let expired_ts = 1_000_000_000i64;
    let expired_token = api::onlyoffice::download_token(media_id, expired_ts, SECRET);
    let expired = client
        .get(format!("{app_addr}/api/onlyoffice/media/{media_id}/download?expires={expired_ts}&token={expired_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(expired.status(), 403);
    let future = 9_999_999_999i64;
    let good_token = api::onlyoffice::download_token(media_id, future, SECRET);
    let good = client
        .get(format!("{app_addr}/api/onlyoffice/media/{media_id}/download?expires={future}&token={good_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(good.status(), 200);
    assert_eq!(good.bytes().await.unwrap().as_ref(), b"DOCX-UJI");

    // Berkas tidak ada dan tanpa autentikasi.
    let missing = client
        .get(export(99_999_999))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    let unauth = client.get(export(berkas)).send().await.unwrap();
    assert_eq!(unauth.status(), 401);

    // Bersihkan data uji.
    for mid in [docx.media_id] {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(mid)
            .execute(&pool)
            .await
            .unwrap();
    }
    for b in [berkas, berkas_exe] {
        sqlx::query(
            "DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Berkas' AND model_id = ?",
        )
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM tbl_berkas WHERE id = ?")
            .bind(b)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&storage);
}

/// Konversi ke Document Server nyata. Butuh `ONLYOFFICE_DOCUMENT_SERVER_URL` dan `ONLYOFFICE_JWT_SECRET`
/// di environment, serta sumber publik yang bisa diunduh Document Server (`ONLYOFFICE_TEST_SOURCE`).
/// Hasil konversi diunduh dari `fileUrl` yang dikembalikan Document Server. Di sandbox yang hanya
/// bisa HTTPS, unduhan `http://` gagal, jadi tes ini perlu dijalankan di jaringan yang bisa HTTP.
#[tokio::test]
#[ignore = "butuh Document Server nyata dan ONLYOFFICE_TEST_SOURCE"]
async fn real_document_server_converts_public_docx() {
    let settings = api::onlyoffice::Settings::from_env();
    assert!(
        settings.enabled(),
        "ONLYOFFICE_DOCUMENT_SERVER_URL belum di-set"
    );
    let source =
        std::env::var("ONLYOFFICE_TEST_SOURCE").expect("ONLYOFFICE_TEST_SOURCE belum di-set");
    let key = format!("uji-rust-{}", std::process::id());
    let pdf = api::onlyoffice::convert_to_pdf(&settings, &source, "docx", &key, "sample.docx")
        .await
        .expect("konversi berhasil");
    assert!(pdf.starts_with(b"%PDF"), "hasil harus PDF");
}
