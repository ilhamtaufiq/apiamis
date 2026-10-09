//! Profil, avatar, dan impersonasi (`auth_profile`) lewat router dan MySQL.
//!
//! Tidak ada email yang dikirim. Berkas avatar ditulis ke folder sementara lewat `PUBLIC_STORAGE_PATH`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test auth_profile_db -- --include-ignored
//! ```

use std::io::Cursor;
use std::path::PathBuf;

use api::{app, media, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const BOUNDARY: &str = "----uji-ap-boundary";
const MODEL_USER: &str = "App\\Models\\User";

const PROFILE_EMAIL: &str = "uji-ap-profil@example.test";
const PROFILE_INVALID_EMAIL: &str = "uji-ap-profil-422@example.test";
const AVATAR_EMAIL: &str = "uji-ap-avatar@example.test";
const ROLE_USER_EMAIL: &str = "uji-ap-rolebiasa@example.test";
const ROLE_ADMIN_EMAIL: &str = "uji-ap-roleadmin@example.test";
const IMP_ADMIN_EMAIL: &str = "uji-ap-adm@example.test";
const IMP_TARGET_EMAIL: &str = "uji-ap-tgt@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

enum Payload {
    None,
    Json(Value),
    Multipart(Vec<u8>),
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

/// Kirim satu request ke router. Mengembalikan status dan body JSON (atau `Null`).
async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    payload: Payload,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match payload {
        Payload::None => Body::empty(),
        Payload::Json(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        Payload::Multipart(bytes) => {
            req = req.header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            );
            Body::from(bytes)
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

/// Body multipart dengan satu berkas pada field `field`.
fn multipart_file(field: &str, filename: &str, content_type: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// PNG 4x4 yang dibuat di dalam test.
fn png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 128, 255, 255]));
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("PNG kecil harus bisa dibuat");
    cursor.into_inner()
}

/// Buat user uji. `admin = true` menambahkan role `admin`.
async fn make_user(pool: &MySqlPool, email: &str, name: &str, admin: bool) -> u64 {
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(name)
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
        sqlx::query(
            "INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)",
        )
        .bind(role)
        .bind(MODEL_USER)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    }
    uid
}

/// Hapus hanya user dengan email yang diberikan beserta token, role, media, dan audit miliknya.
async fn cleanup(pool: &MySqlPool, emails: &[&str]) {
    for email in emails {
        let ids: Vec<u64> = sqlx::query_scalar::<_, u64>(
            "SELECT id FROM media WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(MODEL_USER)
        .bind(email)
        .fetch_all(pool)
        .await
        .unwrap();
        for id in ids {
            let _ = std::fs::remove_dir_all(media::media_dir(id));
        }
        sqlx::query(
            "DELETE FROM media WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(MODEL_USER)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM tbl_audit_logs WHERE user_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM personal_access_tokens WHERE tokenable_type = ? AND tokenable_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(MODEL_USER)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "DELETE FROM model_has_roles WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(MODEL_USER)
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

/// Folder penyimpanan media sementara untuk test avatar.
fn use_storage() -> PathBuf {
    let storage = std::env::temp_dir().join(format!("uji-ap-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    storage
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn profile_update_returns_changed_fields_and_writes_audit() {
    let pool = pool().await;
    cleanup(&pool, &[PROFILE_EMAIL]).await;
    let uid = make_user(&pool, PROFILE_EMAIL, "Uji AP Profil", false).await;
    let token = auth::login::create_token(&pool, uid, "uji-ap")
        .await
        .unwrap();

    let (status, body) = send(
        &pool,
        Method::PUT,
        "/api/auth/profile",
        Some(&token),
        Payload::Json(json!({
            "name": "  Uji AP Nama Baru  ",
            "nip": "198765432",
            "jabatan": "Operator Uji",
            "gender": "female",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], json!(uid));
    assert_eq!(body["name"], "Uji AP Nama Baru");
    assert_eq!(body["nip"], "198765432");
    assert_eq!(body["jabatan"], "Operator Uji");
    assert_eq!(body["gender"], "female");
    assert_eq!(body["email"], PROFILE_EMAIL);

    let (name, nip, jabatan, gender): (String, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT name, nip, jabatan, gender FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Uji AP Nama Baru");
    assert_eq!(nip.as_deref(), Some("198765432"));
    assert_eq!(jabatan.as_deref(), Some("Operator Uji"));
    assert_eq!(gender.as_deref(), Some("female"));

    // Audit `updated` berisi nilai baru kolom yang berubah.
    let new_values: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE event = 'updated' AND auditable_type = ? AND auditable_id = ? AND user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(MODEL_USER)
    .bind(uid)
    .bind(uid)
    .fetch_optional(&pool)
    .await
    .unwrap()
    .flatten();
    let new_values: Value =
        serde_json::from_str(&new_values.expect("audit `updated` harus ada")).unwrap();
    assert_eq!(new_values["name"], "Uji AP Nama Baru");
    assert_eq!(new_values["gender"], "female");

    cleanup(&pool, &[PROFILE_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn profile_update_rejects_invalid_input_without_partial_write() {
    let pool = pool().await;
    cleanup(&pool, &[PROFILE_INVALID_EMAIL]).await;
    let uid = make_user(&pool, PROFILE_INVALID_EMAIL, "Uji AP Tetap", false).await;
    let token = auth::login::create_token(&pool, uid, "uji-ap")
        .await
        .unwrap();

    let (status, body) = send(
        &pool,
        Method::PUT,
        "/api/auth/profile",
        Some(&token),
        Payload::Json(json!({
            "name": "Tidak Boleh Tersimpan",
            "email": "bukan-email",
            "gender": "alien",
            "password": "123",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The given data was invalid.");
    assert!(body["errors"]["email"].is_array(), "{body}");
    assert!(body["errors"]["gender"].is_array(), "{body}");
    assert!(body["errors"]["password"].is_array(), "{body}");
    assert!(body["errors"].get("name").is_none(), "{body}");

    // Tidak ada kolom yang tertulis walau field `name` sendiri valid.
    let name: String = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Uji AP Tetap");

    cleanup(&pool, &[PROFILE_INVALID_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn profile_update_without_token_is_unauthorized() {
    let pool = pool().await;
    let (status, _body) = send(
        &pool,
        Method::PUT,
        "/api/auth/profile",
        None,
        Payload::Json(json!({ "name": "Tanpa Token" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn avatar_upload_accepts_small_png_and_rejects_non_image() {
    let pool = pool().await;
    let storage = use_storage();
    cleanup(&pool, &[AVATAR_EMAIL]).await;
    let uid = make_user(&pool, AVATAR_EMAIL, "Uji AP Avatar", false).await;
    let token = auth::login::create_token(&pool, uid, "uji-ap")
        .await
        .unwrap();

    // PNG yang valid diterima dan tercatat sebagai media koleksi `avatar` di disk `public`.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/auth/avatar",
        Some(&token),
        Payload::Multipart(multipart_file(
            "avatar",
            "uji-ap.png",
            "image/png",
            &png_bytes(),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let avatar_url = body["avatar_url"]
        .as_str()
        .expect("avatar_url harus terisi");
    assert!(avatar_url.contains("/storage/"), "{avatar_url}");

    let rows: Vec<(u64, String, String)> = sqlx::query_as(
        "SELECT id, disk, file_name FROM media WHERE model_type = ? AND model_id = ? AND collection_name = 'avatar'",
    )
    .bind(MODEL_USER)
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "tepat satu avatar");
    let (media_id, disk, file_name) = rows[0].clone();
    assert_eq!(disk, "public");
    let stored = std::fs::read(media::media_dir(media_id).join(&file_name))
        .expect("berkas avatar harus ada di disk");
    assert!(stored.starts_with(&[0x89, b'P', b'N', b'G']));

    // Berkas teks bukan gambar ditolak 422 dan avatar yang sudah ada tidak berubah.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/auth/avatar",
        Some(&token),
        Payload::Multipart(multipart_file(
            "avatar",
            "catatan.txt",
            "text/plain",
            b"bukan gambar",
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["avatar"].is_array(), "{body}");

    let still: Vec<u64> = sqlx::query_scalar::<_, u64>(
        "SELECT id FROM media WHERE model_type = ? AND model_id = ? AND collection_name = 'avatar'",
    )
    .bind(MODEL_USER)
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(still, vec![media_id]);

    cleanup(&pool, &[AVATAR_EMAIL]).await;
    let _ = std::fs::remove_dir_all(&storage);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn impersonate_rejects_non_admin_and_self_without_issuing_token() {
    let pool = pool().await;
    cleanup(&pool, &[ROLE_USER_EMAIL, ROLE_ADMIN_EMAIL]).await;
    let user_id = make_user(&pool, ROLE_USER_EMAIL, "Uji AP Biasa", false).await;
    let admin_id = make_user(&pool, ROLE_ADMIN_EMAIL, "Uji AP Admin", true).await;
    let user_token = auth::login::create_token(&pool, user_id, "uji-ap")
        .await
        .unwrap();
    let admin_token = auth::login::create_token(&pool, admin_id, "uji-ap")
        .await
        .unwrap();

    // Non-admin: 403, meskipun target valid.
    let uri = format!("/api/auth/impersonate/{admin_id}");
    let (status, body) = send(&pool, Method::POST, &uri, Some(&user_token), Payload::None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Admin yang memilih dirinya sendiri: 422.
    let uri = format!("/api/auth/impersonate/{admin_id}");
    let (status, body) = send(&pool, Method::POST, &uri, Some(&admin_token), Payload::None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Cannot impersonate yourself");

    // Kedua penolakan tidak membuat token impersonasi.
    let issued: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM personal_access_tokens WHERE name = 'impersonation-token' AND tokenable_id IN (?, ?)",
    )
    .bind(user_id)
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(issued, 0);

    cleanup(&pool, &[ROLE_USER_EMAIL, ROLE_ADMIN_EMAIL]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn impersonate_admin_gets_token_for_target_and_audit_row() {
    let pool = pool().await;
    cleanup(&pool, &[IMP_ADMIN_EMAIL, IMP_TARGET_EMAIL]).await;
    let admin_id = make_user(&pool, IMP_ADMIN_EMAIL, "Uji AP Adm", true).await;
    let target_id = make_user(&pool, IMP_TARGET_EMAIL, "Uji AP Tujuan", false).await;
    let admin_token = auth::login::create_token(&pool, admin_id, "uji-ap")
        .await
        .unwrap();

    let uri = format!("/api/auth/impersonate/{target_id}");
    let (status, body) = send(&pool, Method::POST, &uri, Some(&admin_token), Payload::None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["id"], json!(target_id));
    assert_eq!(body["user"]["email"], IMP_TARGET_EMAIL);
    assert_eq!(body["message"], "Now impersonating Uji AP Tujuan");
    let token = body["token"].as_str().expect("token harus string");
    assert!(
        token.contains('|'),
        "format token Sanctum id|plain: {token}"
    );

    // Token itu benar-benar milik target dan bernama `impersonation-token`.
    let auth_user = auth::authenticate(&pool, token)
        .await
        .expect("token harus valid");
    assert_eq!(auth_user.user_id, target_id);
    let named: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM personal_access_tokens WHERE name = 'impersonation-token' AND tokenable_type = ? AND tokenable_id = ?",
    )
    .bind(MODEL_USER)
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(named, 1);

    // Audit `impersonation_started` mencatat pelaku dan target.
    let (auditable_type, auditable_id, new_values): (String, u64, Option<String>) =
        sqlx::query_as(
            "SELECT auditable_type, auditable_id, CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE event = 'impersonation_started' AND user_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(admin_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(auditable_type, MODEL_USER);
    assert_eq!(auditable_id, target_id);
    let new_values: Value =
        serde_json::from_str(&new_values.expect("new_values harus ada")).unwrap();
    assert_eq!(new_values["impersonator_id"], json!(admin_id));
    assert_eq!(new_values["target_user_id"], json!(target_id));
    assert_eq!(new_values["target_user_email"], IMP_TARGET_EMAIL);

    // Catatan: masa berlaku token impersonasi TIDAK diuji di sini. Kolom `expires_at` tidak diisi
    // oleh handler (lihat doc `auth_profile.rs`), jadi tidak ada perilaku kedaluwarsa yang dikunci.

    cleanup(&pool, &[IMP_ADMIN_EMAIL, IMP_TARGET_EMAIL]).await;
}
