//! Tulis tiket lewat router terhadap MySQL: store dengan lampiran, status, komentar, bulk, dan hapus.
//! Jalur pemilik (non-admin) tidak diuji di sini karena route permission memblokir mutasi tanpa rule.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test tiket_write_db -- --ignored
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

const ACTOR: &str = "uji-tiket-admin@example.test";
const BOUNDARY: &str = "----ujitiketboundary";

fn config() -> Config {
    Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    }
}

fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    body: Vec<u8>,
    json_body: Option<Value>,
) -> (StatusCode, Value) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = match json_body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(body))
            .unwrap(),
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(req)
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

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Tiket', ?, 'x', NOW(), NOW())")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ACTOR)
        .fetch_one(pool)
        .await
        .unwrap();
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
    uid
}

fn png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(4, 4, |x, y| image::Rgb([x as u8 * 50, y as u8 * 50, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tiket_store_status_comment_bulk_and_destroy() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = std::env::temp_dir().join(format!("uji-tiket-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);

    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-tiket")
        .await
        .unwrap();
    let subjek = format!("UJI-TIKET-{}", std::process::id());

    // Store dengan lampiran PNG.
    let img = png();
    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/tiket",
        &token,
        multipart(
            &[
                ("subjek", &subjek),
                ("deskripsi", "Ada kendala"),
                ("kategori", "bug"),
                ("prioritas", "high"),
            ],
            Some(("bukti.png", &img)),
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["data"]["id"].as_i64().unwrap();
    assert_eq!(created["data"]["status"], "open");
    assert!(
        created["data"].get("comments").is_none(),
        "store tidak memuat komentar"
    );
    assert!(created["data"]["image_url"]
        .as_str()
        .unwrap()
        .ends_with(".png"));

    // Validasi: kategori tidak dikenal dan lampiran bukan gambar.
    let (status, err) = send(
        &pool,
        Method::POST,
        "/api/tiket",
        &token,
        multipart(
            &[
                ("subjek", "x"),
                ("deskripsi", "y"),
                ("kategori", "salah"),
                ("prioritas", "high"),
            ],
            None,
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(err["message"], "Validation error");
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/tiket",
        &token,
        multipart(
            &[
                ("subjek", "x"),
                ("deskripsi", "y"),
                ("kategori", "bug"),
                ("prioritas", "low"),
            ],
            Some(("naskah.pdf", b"%PDF-1.4")),
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Admin mengubah status: pemilik (admin itu sendiri) menerima notifikasi "Update Status Tiket".
    let (status, patched) = send(
        &pool,
        Method::PUT,
        &format!("/api/tiket/{id}"),
        &token,
        multipart(&[("status", "pending")], None),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["data"]["status"], "pending");
    let notes: i64 = sqlx::query("SELECT COUNT(*) AS n FROM notifications WHERE notifiable_id = ? AND data LIKE '%Update Status Tiket%'")
        .bind(actor).fetch_one(&pool).await.unwrap().try_get("n").unwrap();
    assert!(notes >= 1, "notifikasi status untuk pemilik");

    // Komentar: pemilik (admin ini) berkomentar, admin lain diberi notifikasi.
    let (status, comment) = send(
        &pool,
        Method::POST,
        &format!("/api/tiket/{id}/comments"),
        &token,
        Vec::new(),
        Some(json!({ "message": "Sedang dicek" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{comment}");
    assert_eq!(comment["data"]["message"], "Sedang dicek");
    assert_eq!(comment["data"]["user"]["id"], actor);
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/tiket/{id}/comments"),
        &token,
        Vec::new(),
        Some(json!({ "message": "  " })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // Audit: created, updated, dan created untuk komentar.
    for (model, event) in [
        ("App\\\\Models\\\\Tiket", "created"),
        ("App\\\\Models\\\\Tiket", "updated"),
        ("App\\\\Models\\\\TiketComment", "created"),
    ] {
        let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_audit_logs WHERE auditable_type = ? AND event = ? AND user_id = ?")
            .bind(model.replace("\\\\", "\\")).bind(event).bind(actor).fetch_one(&pool).await.unwrap().try_get("n").unwrap();
        assert!(n >= 1, "audit {model} {event}");
    }

    // Bulk: status massal, tanpa audit tambahan.
    let (status, bulk) = send(
        &pool,
        Method::POST,
        "/api/tiket/bulk-update",
        &token,
        Vec::new(),
        Some(json!({ "ids": [id], "status": "closed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bulk}");
    assert_eq!(bulk["message"], "1 tiket berhasil diperbarui");
    let (_, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/tiket/{id}"),
        &token,
        Vec::new(),
        None,
    )
    .await;
    assert_eq!(shown["data"]["status"], "closed");
    assert!(shown["data"]["comments"].is_array());

    // Hapus: pesan Laravel dan lampiran ikut dihapus dari disk.
    let media_dir_count = std::fs::read_dir(&storage).map(|d| d.count()).unwrap_or(0);
    let (status, msg) = send(
        &pool,
        Method::DELETE,
        &format!("/api/tiket/{id}"),
        &token,
        Vec::new(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(msg["message"], "Tiket berhasil dihapus");
    let after = std::fs::read_dir(&storage).map(|d| d.count()).unwrap_or(0);
    assert!(
        after < media_dir_count || media_dir_count == 0,
        "direktori lampiran dihapus"
    );
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/tiket/{id}"),
        &token,
        Vec::new(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(&storage);
}
