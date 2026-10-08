//! CRUD foto lewat router terhadap MySQL dan folder storage sementara.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test foto_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ACTOR: &str = "uji-foto-admin@example.test";
const BOUNDARY: &str = "----ujifotoboundary";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        // Lebih kecil dari berkas uji: membuktikan rute foto punya batas body sendiri.
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Body `multipart/form-data` dengan field teks dan satu berkas opsional.
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

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .header(header::USER_AGENT, "uji-agent")
            .body(Body::from(body))
            .unwrap(),
    )
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

/// JPEG nyata berisi noise (tidak bisa dikompres kecil), kualitas 95.
fn real_jpeg(width: u32, height: u32) -> Vec<u8> {
    use image::{codecs::jpeg::JpegEncoder, ImageEncoder, Rgb, RgbImage};
    let mut img = RgbImage::new(width, height);
    for px in img.pixels_mut() {
        *px = Rgb([rand::random(), rand::random(), rand::random()]);
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 95)
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// PNG nyata kecil (gradien).
fn real_png(width: u32, height: u32) -> Vec<u8> {
    use image::{ImageFormat, Rgb, RgbImage};
    let img = RgbImage::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, 128]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Foto', ?, 'x', NOW(), NOW())")
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn store_show_update_destroy_with_media_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = std::env::temp_dir().join(format!("uji-foto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
    // Koordinat dan batas desa mengikuti GeoJSON repo (lihat koordinat.rs).
    std::env::remove_var("APP_KEY");

    // Sisa run yang gagal di tengah (komponen uji) dibersihkan lebih dulu.
    sqlx::query("DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Foto' AND model_id IN (SELECT id FROM tbl_foto WHERE komponen_id = 9001)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_foto WHERE komponen_id = 9001")
        .execute(&pool)
        .await
        .unwrap();

    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-foto")
        .await
        .unwrap();
    // Pekerjaan yang punya desa dan kecamatan, supaya validasi koordinat bisa dijalankan.
    let pekerjaan: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_pekerjaan WHERE desa_id IS NOT NULL AND kecamatan_id IS NOT NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let expected = api::koordinat::validate_for_pekerjaan(&pool, pekerjaan, "-6.8, 107.21")
        .await
        .unwrap();
    let pid = pekerjaan.to_string();

    // Store: berkas di atas 1 MB (batas body global) harus lolos lewat batas rute foto.
    let big = real_jpeg(1000, 1000);
    assert!(big.len() > 1024 * 1024, "berkas uji harus > 1 MB");
    let body = multipart(
        &[
            ("pekerjaan_id", &pid),
            ("komponen_id", "9001"),
            ("keterangan", "25%"),
            ("koordinat", "-6.8, 107.21"),
            ("penerima_id", ""),
        ],
        Some(("Foto Lapangan.jpg", &big)),
    );
    let (status, created) = send(&pool, Method::POST, "/api/foto", &token, body).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let data = &created["data"];
    let foto_id = data["id"].as_i64().unwrap();
    assert_eq!(data["keterangan"], "25%");
    // Respon harus sama dengan hasil validasi langsung (lokasi di luar GeoJSON lokal pun valid).
    assert_eq!(data["validasi_koordinat"], expected.valid);
    assert_eq!(
        data["validasi_koordinat_message"],
        expected.message.as_str()
    );
    assert!(data["penerima_id"].is_null());
    assert_eq!(data["penerima"], Value::Null);
    assert_eq!(data["pekerjaan"]["id"], pekerjaan);

    let media = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, file_name, name, size, mime_type FROM media \
         WHERE model_type = 'App\\\\Models\\\\Foto' AND model_id = ? AND collection_name = 'foto/pekerjaan'",
    )
    .bind(foto_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(media.len(), 1);
    let media_id: i64 = media[0].try_get("id").unwrap();
    let file_name: String = media[0].try_get("file_name").unwrap();
    assert!(file_name.ends_with(".jpg"));
    assert_eq!(
        media[0].try_get::<String, _>("name").unwrap(),
        "Foto Lapangan"
    );
    assert_eq!(
        media[0].try_get::<String, _>("mime_type").unwrap(),
        "image/jpeg"
    );
    assert_eq!(
        media[0].try_get::<u64, _>("size").unwrap(),
        big.len() as u64
    );
    let first_dir = storage.join(media_id.to_string());
    assert!(
        first_dir.join(&file_name).exists(),
        "berkas tersimpan di disk"
    );
    assert_eq!(
        data["foto_url"],
        format!("http://localhost/storage/{media_id}/{file_name}").as_str()
    );
    // Thumbnail 120x120 dibuat saat upload (`thumb`), seperti Laravel.
    let stem = file_name.trim_end_matches(".jpg");
    let thumb_path = first_dir
        .join("conversions")
        .join(format!("{stem}-thumb.jpg"));
    assert!(thumb_path.exists(), "thumbnail dibuat");
    let thumb = image::open(&thumb_path).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (120, 120));
    assert_eq!(
        data["foto_thumb_url"],
        format!("http://localhost/storage/{media_id}/conversions/{stem}-thumb.jpg").as_str()
    );

    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Foto' AND auditable_id = ? AND event = 'created' AND user_id = ?",
    )
    .bind(foto_id)
    .bind(actor)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);

    // Show.
    let (status, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/foto/{foto_id}"),
        &token,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["data"]["id"], foto_id);

    // Update tanpa _method=PUT ditolak (405), seperti Laravel.
    let body = multipart(&[("keterangan", "50%")], None);
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/foto/{foto_id}"),
        &token,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // Update dengan _method=PUT dan berkas baru: berkas lama diganti, audit updated ditulis.
    let new_bytes = real_png(300, 200);
    let body = multipart(
        &[
            ("_method", "PUT"),
            ("keterangan", "50%"),
            ("koordinat", "manual"),
        ],
        Some(("baru.png", &new_bytes)),
    );
    let (status, updated) = send(
        &pool,
        Method::POST,
        &format!("/api/foto/{foto_id}"),
        &token,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["keterangan"], "50%");
    assert_eq!(updated["data"]["validasi_koordinat"], false);
    assert_eq!(
        updated["data"]["validasi_koordinat_message"],
        "Koordinat tidak dapat dibaca. Gunakan format lat, lng."
    );
    let media_after: Vec<String> = sqlx::query_scalar(
        "SELECT file_name FROM media WHERE model_type = 'App\\\\Models\\\\Foto' AND model_id = ? AND collection_name = 'foto/pekerjaan'",
    )
    .bind(foto_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(media_after.len(), 1, "hanya berkas baru yang tersisa");
    assert!(media_after[0].ends_with(".png"));
    assert!(!first_dir.exists(), "direktori berkas lama dihapus");
    assert!(updated["data"]["foto_thumb_url"]
        .as_str()
        .unwrap()
        .ends_with(".png"));

    let updated_audit: String = sqlx::query_scalar(
        "SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Foto' AND auditable_id = ? AND event = 'updated' ORDER BY id DESC LIMIT 1",
    )
    .bind(foto_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let new_values: Value = serde_json::from_str(&updated_audit).unwrap();
    assert_eq!(new_values["keterangan"], "50%");
    assert_eq!(
        new_values["koordinat"], "manual",
        "kolom koordinat ikut berubah"
    );
    // Audit hanya memuat kolom yang berubah (`getDirty()`): validasi ikut hanya bila sebelumnya true.
    assert_eq!(
        new_values.get("validasi_koordinat").is_some(),
        expected.valid
    );

    // Destroy.
    let (status, msg) = send(
        &pool,
        Method::DELETE,
        &format!("/api/foto/{foto_id}"),
        &token,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{msg}");
    assert_eq!(msg["message"], "Foto deleted successfully");
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_foto WHERE id = ?")
        .bind(foto_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let deleted_audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Foto' AND auditable_id = ? AND event = 'deleted'",
    )
    .bind(foto_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(deleted_audit, 1);

    let _ = std::fs::remove_dir_all(&storage);
}
