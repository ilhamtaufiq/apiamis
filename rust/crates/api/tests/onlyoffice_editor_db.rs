//! Editor ONLYOFFICE lewat router terhadap MySQL: health, config editor, dan callback simpan.
//!
//! Document Server tiruan berjalan di TCP loopback. Semua skenario ada dalam satu fungsi karena
//! konfigurasi lewat environment bersifat global untuk proses tes.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test onlyoffice_editor_db -- --include-ignored
//! ```

use api::{app, media, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use shared::Config;
use sqlx::MySqlPool;
use tokio::net::TcpListener;
use tower::ServiceExt;

const SECRET: &str = "uji-jwt-editor-rahasia";
const ADMIN: &str = "uji-oe-admin@example.test";
const OUTSIDER: &str = "uji-oe-outsider@example.test";
const JENIS: &str = "uji-oe";
const BERKAS_MODEL: &str = "App\\Models\\Berkas";
const USER_MODEL: &str = "App\\Models\\User";
const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// JWT HS256 dengan secret tertentu, dibuat tanpa memakai kode aplikasi.
fn sign(payload: &Value, secret: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
    let body = URL_SAFE_NO_PAD.encode(payload.to_string());
    let input = format!("{header}.{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
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
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Hapus data uji sisa dari percobaan sebelumnya. Hanya baris dengan penanda `uji-oe`.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query(
        "DELETE m FROM media m JOIN tbl_berkas b ON m.model_id = b.id \
         WHERE m.model_type = ? AND b.jenis_dokumen = ?",
    )
    .bind(BERKAS_MODEL)
    .bind(JENIS)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_berkas WHERE jenis_dokumen = ?")
        .bind(JENIS)
        .execute(pool)
        .await
        .unwrap();
    for email in [ADMIN, OUTSIDER] {
        sqlx::query(
            "DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM model_has_roles WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(USER_MODEL)
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

async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji OE', ?, 'x', NOW(), NOW())")
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
    uid
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn onlyoffice_editor_health_config_and_callback() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = std::env::temp_dir().join(format!("uji-oe-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    std::env::set_var("ONLYOFFICE_JWT_SECRET", SECRET);
    std::env::set_var(
        "APP_KEY",
        "base64:dW5pLWFwcC1rZXktc2VjcmV0LXVuaS1hcHAta2V5MTIzNDU=",
    );
    std::env::remove_var("ONLYOFFICE_CALLBACK_URL");

    // Document Server tiruan: healthcheck dan berkas hasil edit.
    let ds_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ds_url = format!("http://{}", ds_listener.local_addr().unwrap());
    let ds_app = Router::new()
        .route("/healthcheck", get(|| async { "true" }))
        .route("/out.docx", get(|| async { "EDITED-UJI" }));
    tokio::spawn(async move { axum::serve(ds_listener, ds_app).await.unwrap() });
    std::env::set_var("ONLYOFFICE_DOCUMENT_SERVER_URL", &ds_url);

    cleanup(&pool).await;
    let admin = make_user(&pool, ADMIN, true).await;
    let outsider = make_user(&pool, OUTSIDER, false).await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-oe")
        .await
        .unwrap();
    let outsider_token = auth::login::create_token(&pool, outsider, "uji-oe")
        .await
        .unwrap();

    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
        .bind(pekerjaan)
        .bind(JENIS)
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let berkas: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_berkas WHERE jenis_dokumen = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(JENIS)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let docx = media::attach(
        &mut tx,
        BERKAS_MODEL,
        berkas,
        "berkas/dokumen",
        &media::Upload {
            original_name: "uji-oe.docx".into(),
            bytes: b"DOCX-UJI".to_vec(),
        },
        DOCX_MIME,
        false,
    )
    .await
    .unwrap();
    let png = media::attach(
        &mut tx,
        BERKAS_MODEL,
        berkas,
        "berkas/dokumen",
        &media::Upload {
            original_name: "uji-oe.png".into(),
            bytes: b"PNG-UJI".to_vec(),
        },
        "image/png",
        false,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let docx_id = docx.media_id;
    let png_id = png.media_id;
    let docx_path = docx.dir.join(&docx.file_name);

    // A. Health: Document Server tiruan menjawab `true`.
    let (s, v) = send(&pool, Method::GET, "/api/onlyoffice/health", None, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["enabled"], true);
    assert_eq!(v["data"]["reachable"], true);
    assert_eq!(v["data"]["document_server_url"], format!("{ds_url}/"));

    // B. Callback tanpa token: error 1 dengan HTTP 200, seperti Laravel.
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "status": 2 })),
    )
    .await;
    assert_eq!((s, v.clone()), (StatusCode::OK, json!({ "error": 1 })));

    // C. Token dengan secret salah: ditolak.
    let forged = sign(
        &json!({ "status": 2, "key": format!("media_{docx_id}_1"), "url": format!("{ds_url}/out.docx") }),
        "salah",
    );
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "token": forged })),
    )
    .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "error": 1 })));

    // D. Status 1 (sedang diedit): error 0 tanpa menyimpan.
    let editing = sign(
        &json!({ "status": 1, "key": format!("media_{docx_id}_1"), "url": format!("{ds_url}/out.docx") }),
        SECRET,
    );
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "token": editing })),
    )
    .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "error": 0 })));
    assert_eq!(std::fs::read(&docx_path).unwrap(), b"DOCX-UJI");

    // E. URL dengan host lain: SSRF guard menolak.
    let foreign = sign(
        &json!({ "status": 2, "key": format!("media_{docx_id}_1"), "url": "http://evil.example/x.docx" }),
        SECRET,
    );
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "token": foreign })),
    )
    .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "error": 1 })));
    assert_eq!(std::fs::read(&docx_path).unwrap(), b"DOCX-UJI");

    // F. Kunci dokumen tidak dikenali: error 1.
    let bad_key = sign(
        &json!({ "status": 2, "key": "foto_1_1", "url": format!("{ds_url}/out.docx") }),
        SECRET,
    );
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "token": bad_key })),
    )
    .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "error": 1 })));

    // G. Status 2 dengan URL Document Server: file ditimpa dan `size` diperbarui.
    let save = sign(
        &json!({ "status": 6, "key": format!("media_{docx_id}_1"), "url": format!("{ds_url}/out.docx") }),
        SECRET,
    );
    let (s, v) = send(
        &pool,
        Method::POST,
        "/api/onlyoffice/callback",
        None,
        Some(json!({ "token": save })),
    )
    .await;
    assert_eq!((s, v), (StatusCode::OK, json!({ "error": 0 })));
    assert_eq!(std::fs::read(&docx_path).unwrap(), b"EDITED-UJI");
    let size: u64 = sqlx::query_scalar("SELECT size FROM media WHERE id = ?")
        .bind(docx_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(size, 10);

    // H. Config untuk admin dalam mode edit, dengan token JWT yang memuat config.
    let (s, v) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{docx_id}/config?mode=edit"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let data = &v["data"];
    assert_eq!(data["mode"], "edit");
    assert_eq!(data["can_edit"], true);
    assert_eq!(data["config"]["document"]["fileType"], "docx");
    assert_eq!(data["config"]["documentType"], "word");
    assert_eq!(data["config"]["editorConfig"]["customization"]["compactToolbar"], false);
    assert_eq!(data["media"]["extension"], "docx");
    assert!(data["config"]["document"]["key"]
        .as_str()
        .unwrap()
        .starts_with(&format!("media_{docx_id}_")));
    let token = data["config"]["token"].as_str().expect("token JWT");
    let body_part = token.split('.').nth(1).unwrap();
    let signed: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body_part).unwrap()).unwrap();
    assert_eq!(signed["document"]["key"], data["config"]["document"]["key"]);
    assert_eq!(sign(&signed, SECRET), token, "token ditandatangani dengan secret");

    // I. Mode tidak valid dan format tidak didukung: 422.
    let (s, v) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{docx_id}/config?mode=hapus"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "Mode tidak valid. Gunakan view atau edit.");
    let (s, v) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{png_id}/config"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "Format file tidak didukung ONLYOFFICE.");

    // J. Pengguna tanpa penugasan ke pekerjaan: 403. Tanpa token: 401.
    let (s, v) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{docx_id}/config"),
        Some(&outsider_token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["message"], "Anda tidak memiliki akses ke dokumen ini.");
    let (s, _) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{docx_id}/config"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // K. Media tidak ada: 404 (binding dulu, sebelum auth).
    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/onlyoffice/media/999999999/config",
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // L. Document Server tidak dikonfigurasi: health 503, config 503.
    std::env::set_var("ONLYOFFICE_DOCUMENT_SERVER_URL", "");
    let (s, v) = send(&pool, Method::GET, "/api/onlyoffice/health", None, None).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["data"]["enabled"], false);
    assert!(v["data"]["document_server_url"].is_null());
    let (s, _) = send(
        &pool,
        Method::GET,
        &format!("/api/onlyoffice/media/{docx_id}/config"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);

    let _ = std::fs::remove_dir_all(&storage);
    cleanup(&pool).await;
}
