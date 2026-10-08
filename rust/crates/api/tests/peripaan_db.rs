//! Peta peripaan lewat router terhadap MySQL (multipart dan media).
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test peripaan_db -- --include-ignored
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

const ADMIN: &str = "uji-peri-admin@example.test";
const PLAIN: &str = "uji-peri-plain@example.test";
const NAMA: &str = "uji-peri-peta";
const BOUNDARY: &str = "ujiPeripaanBoundary";
const KML: &str = "<kml><Document><name>uji</name></Document></kml>";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Body multipart dengan field teks dan berkas opsional.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        out.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/vnd.google-earth.kml+xml\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Body,
    content_type: Option<String>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Peri', ?, 'x', NOW(), NOW())")
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
        let role: u64 =
            sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
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
    auth::login::create_token(pool, uid, "uji-peri").await.unwrap()
}

async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM media WHERE model_type = 'App\\\\Models\\\\PetaPeripaan' AND model_id IN (SELECT id FROM tbl_peta_peripaan WHERE nama = ?)")
        .bind(NAMA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_peta_peripaan WHERE nama = ?")
        .bind(NAMA)
        .execute(pool)
        .await
        .unwrap();
}

fn ct() -> Option<String> {
    Some(format!("multipart/form-data; boundary={BOUNDARY}"))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn peripaan_store_index_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let plain = user_token(&pool, PLAIN, false).await;
    let pekerjaan: i64 = sqlx::query_scalar("SELECT CAST(MIN(id) AS SIGNED) FROM tbl_pekerjaan")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Pengguna biasa: mutasi ditolak middleware (sama dengan Laravel, /peripaan tidak di daftar mutasi).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/peripaan",
        Some(&plain),
        Body::from(multipart(&[("nama", NAMA)], Some(("peta.kml", KML.as_bytes())))),
        ct(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Validasi: berkas dan nama wajib.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/peripaan",
        Some(&admin),
        Body::from(multipart(&[("nama", NAMA)], None)),
        ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The file field is required.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/peripaan",
        Some(&admin),
        Body::from(multipart(&[], Some(("peta.kml", KML.as_bytes())))),
        ct(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The nama field is required.", "{body}");

    // Simpan dengan geojson dan pekerjaan: 200, berkas tersimpan sebagai media.
    let geojson = r#"{"type":"FeatureCollection","features":[]}"#;
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/peripaan",
        Some(&admin),
        Body::from(multipart(
            &[
                ("nama", NAMA),
                ("pekerjaan_id", &pekerjaan.to_string()),
                ("geojson", geojson),
            ],
            Some(("peta uji.kml", KML.as_bytes())),
        )),
        ct(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    assert_eq!(data["nama"], NAMA);
    assert_eq!(data["geojson"]["type"], "FeatureCollection");
    assert_eq!(data["file_name"].as_str().unwrap().ends_with(".kml"), true, "{body}");
    assert!(data["file_url"].as_str().unwrap().starts_with("http://localhost/storage/"), "{body}");
    assert_eq!(data["size"], KML.len() as u64);
    let id = data["id"].as_i64().unwrap();

    // Daftar dengan filter pekerjaan dan bentuk paginator.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/peripaan?pekerjaan_id={pekerjaan}"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"].as_array().unwrap().iter().any(|d| d["id"] == json!(id)), "{body}");
    assert_eq!(body["meta"]["per_page"], 50);

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/peripaan?per_page=-1",
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("meta").is_none(), "{body}");

    // Hapus: 200, baris dan media hilang.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/peripaan/{id}"),
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Peta peripaan deleted" }));
    let media: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM media WHERE model_type = 'App\\\\Models\\\\PetaPeripaan' AND model_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(media, 0);
    let (status, _) = send(&pool, Method::DELETE, &format!("/api/peripaan/{id}"), Some(&admin), Body::empty(), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
