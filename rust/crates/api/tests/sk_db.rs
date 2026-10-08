//! `/api/sk` lewat router terhadap MySQL: CRUD admin, media, audit, dan penolakan non-admin.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test sk_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ADMIN_A: &str = "uji-sk-admin-a@example.test";
const ADMIN_B: &str = "uji-sk-admin-b@example.test";
const BIASA_B: &str = "uji-sk-biasa-b@example.test";
const BOUNDARY: &str = "----ujiskboundary";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

fn use_storage() {
    let storage = std::env::temp_dir().join(format!("uji-sk-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
}

fn storage_dir(media_id: u64) -> std::path::PathBuf {
    api::media::media_dir(media_id)
}

/// Body multipart dengan field teks dan (opsional) berkas `file`.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

enum Payload {
    None,
    Json(Value),
    Multipart(Vec<u8>),
}

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

/// Buat user. `admin = true` menambahkan role `admin`.
async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji SK', ?, 'x', NOW(), NOW())")
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

/// Hapus sisa uji dari run sebelumnya. `prefix` memisahkan data tiap tes yang berjalan paralel.
async fn cleanup(pool: &MySqlPool, prefix: &str) {
    let pattern = format!("{prefix}%");
    sqlx::query(
        "DELETE FROM media WHERE model_type = ? AND model_id IN (SELECT id FROM sk WHERE nomor_sk LIKE ?)",
    )
    .bind("App\\Models\\Sk")
    .bind(&pattern)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM sk WHERE nomor_sk LIKE ?")
        .bind(&pattern)
        .execute(pool)
        .await
        .unwrap();
}

async fn audit_rows(pool: &MySqlPool, sk_id: u64) -> Vec<(String, Option<String>, Option<String>)> {
    sqlx::query(
        "SELECT event, CAST(old_values AS CHAR) AS old_values, CAST(new_values AS CHAR) AS new_values \
         FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Sk' AND auditable_id = ? ORDER BY id",
    )
    .bind(sk_id)
    .fetch_all(pool)
    .await
    .unwrap()
    .iter()
    .map(|r| {
        (
            r.try_get("event").unwrap(),
            r.try_get("old_values").unwrap(),
            r.try_get("new_values").unwrap(),
        )
    })
    .collect()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sk_admin_crud_with_media_and_audit() {
    use_storage();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool, "UJI-SK-A-").await;
    let admin = make_user(&pool, ADMIN_A, true).await;
    let token = auth::login::create_token(&pool, admin, "uji-sk")
        .await
        .unwrap();

    // Tambah dengan berkas.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/sk",
        Some(&token),
        Payload::Multipart(multipart(
            &[
                ("nomor_sk", "UJI-SK-A-001/2026"),
                ("nama", "Uji SK Satu"),
                ("tanggal_sk", "2026-01-05"),
            ],
            Some(("sk.pdf", b"%PDF-1.4 uji")),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    let id = data["id"].as_u64().unwrap();
    assert_eq!(data["nomor_sk"], "UJI-SK-A-001/2026");
    assert_eq!(data["tanggal_sk"], "2026-01-05");
    assert_eq!(data["uploaded_by"], admin);
    assert_eq!(data["uploader"]["id"], admin);
    assert_eq!(data["mime_type"], "application/pdf");
    assert_eq!(data["size"], 12);
    assert!(data["file_name"].as_str().unwrap().ends_with(".pdf"));
    let media_id = data["media_id"].as_u64().unwrap();
    assert!(data["file_url"].as_str().unwrap().ends_with(&format!(
        "/storage/{media_id}/{}",
        data["file_name"].as_str().unwrap()
    )));
    let file_name = data["file_name"].as_str().unwrap().to_string();
    assert!(storage_dir(media_id).join(&file_name).exists());
    let media_name: String = sqlx::query_scalar("SELECT name FROM media WHERE id = ?")
        .bind(media_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(media_name, "Uji SK Satu");

    let events: Vec<String> = audit_rows(&pool, id)
        .await
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(events, vec!["created"]);

    // Detail dan daftar dengan pencarian.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama"], "Uji SK Satu");

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/sk?search=UJI-SK-A-001&per_page=5",
        Some(&token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["per_page"], 5);
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], id);

    // Ubah tanpa berkas dan tanpa tanggal: tanggal_sk dikosongkan seperti `?? null`.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::Json(json!({ "nomor_sk": "UJI-SK-A-001/2026", "nama": "Uji SK Revisi" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama"], "Uji SK Revisi");
    assert_eq!(body["data"]["tanggal_sk"], Value::Null);
    assert_eq!(body["data"]["media_id"], media_id);
    let rows = audit_rows(&pool, id).await;
    assert_eq!(rows.last().unwrap().0, "updated");
    let new_values: Value = serde_json::from_str(rows.last().unwrap().2.as_ref().unwrap()).unwrap();
    assert_eq!(new_values["nama"], "Uji SK Revisi");
    assert!(new_values.get("nomor_sk").is_none());
    let old_values: Value = serde_json::from_str(rows.last().unwrap().1.as_ref().unwrap()).unwrap();
    assert_eq!(old_values["nama"], "Uji SK Satu");

    // Ganti berkas lewat POST dengan _method=PUT (method spoofing).
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::Multipart(multipart(
            &[
                ("_method", "PUT"),
                ("nomor_sk", "UJI-SK-A-001/2026"),
                ("nama", "Uji SK Dua"),
                ("tanggal_sk", "2026-03-01"),
            ],
            Some(("sk-2.docx", b"isi docx uji")),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let new_media = body["data"]["media_id"].as_u64().unwrap();
    assert_ne!(new_media, media_id);
    assert_eq!(body["data"]["tanggal_sk"], "2026-03-01");
    assert!(body["data"]["file_name"]
        .as_str()
        .unwrap()
        .ends_with(".docx"));
    assert!(!storage_dir(media_id).exists(), "berkas lama harus dihapus");
    let new_file = body["data"]["file_name"].as_str().unwrap().to_string();
    assert!(storage_dir(new_media).join(&new_file).exists());
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM media WHERE model_type = ? AND model_id = ? AND collection_name = 'sk/dokumen'",
    )
    .bind("App\\Models\\Sk")
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    // POST tanpa _method=PUT ditolak seperti Laravel.
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::Multipart(multipart(
            &[("nomor_sk", "UJI-SK-A-001/2026"), ("nama", "X")],
            None,
        )),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // Hapus: media dan baris hilang, audit `deleted` ditulis.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "SK deleted successfully" }));
    assert!(!storage_dir(new_media).exists());
    let left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE model_type = ? AND model_id = ?")
            .bind("App\\Models\\Sk")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(left, 0);
    let events: Vec<String> = audit_rows(&pool, id)
        .await
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(events, vec!["created", "updated", "updated", "deleted"]);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/sk/{id}"),
        Some(&token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "message": "Not Found." }));
    cleanup(&pool, "UJI-SK-A-").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sk_rejects_non_admin_and_invalid_input() {
    use_storage();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool, "UJI-SK-B-").await;
    let admin = make_user(&pool, ADMIN_B, true).await;
    let biasa = make_user(&pool, BIASA_B, false).await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-sk")
        .await
        .unwrap();
    let biasa_token = auth::login::create_token(&pool, biasa, "uji-sk")
        .await
        .unwrap();

    sqlx::query("INSERT INTO sk (nomor_sk, nama, uploaded_by, created_at, updated_at) VALUES ('UJI-SK-B-900', 'Ada', ?, NOW(), NOW())")
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();
    let id: u64 = sqlx::query_scalar("SELECT id FROM sk WHERE nomor_sk = 'UJI-SK-B-900'")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Bukan admin: 403 di semua rute. Tanpa token: 401.
    for (method, uri) in [
        (Method::GET, "/api/sk".to_string()),
        (Method::GET, format!("/api/sk/{id}")),
        (Method::DELETE, format!("/api/sk/{id}")),
    ] {
        let (status, _) = send(&pool, method, &uri, Some(&biasa_token), Payload::None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}");
    }
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/sk",
        Some(&biasa_token),
        Payload::Multipart(multipart(
            &[("nomor_sk", "UJI-SK-B-901"), ("nama", "X")],
            Some(("a.pdf", b"x")),
        )),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&pool, Method::GET, "/api/sk", None, Payload::None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sk WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 1, "DELETE dari non-admin tidak boleh menghapus SK");

    // Validasi: berkas wajib saat tambah, tanggal harus valid, dan 404 untuk id yang tidak ada.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/sk",
        Some(&admin_token),
        Payload::Multipart(multipart(
            &[("nomor_sk", "UJI-SK-B-902"), ("nama", "Tanpa berkas")],
            None,
        )),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["file"][0], "The file field is required.");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/sk",
        Some(&admin_token),
        Payload::Json(
            json!({ "nomor_sk": "UJI-SK-B-903", "nama": "Tanggal salah", "tanggal_sk": "bukan" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["file"][0], "The file field is required.");
    assert_eq!(
        body["errors"]["tanggal_sk"][0],
        "The tanggal sk field must be a valid date."
    );

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/sk/{id}"),
        Some(&admin_token),
        Payload::Json(json!({ "nomor_sk": "", "nama": "Ada" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["errors"]["nomor_sk"][0],
        "The nomor sk field is required."
    );

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/sk/999999999",
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "message": "Not Found." }));
    cleanup(&pool, "UJI-SK-B-").await;
}
