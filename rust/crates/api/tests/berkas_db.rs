//! CRUD berkas lewat router terhadap MySQL dan folder storage sementara.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test berkas_db -- --ignored
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

const ACTOR: &str = "uji-berkas-admin@example.test";
const BOUNDARY: &str = "----ujiberkasboundary";

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
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
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
    body: Option<Vec<u8>>,
    json: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = if let Some(body) = body {
        b = b.header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        );
        b.body(Body::from(body)).unwrap()
    } else if let Some(v) = json {
        b = b.header(header::CONTENT_TYPE, "application/json");
        b.body(Body::from(v.to_string())).unwrap()
    } else {
        b.body(Body::empty()).unwrap()
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Berkas', ?, 'x', NOW(), NOW())")
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
async fn berkas_store_update_destroy_with_media_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let storage = std::env::temp_dir().join(format!("uji-berkas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);

    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-berkas")
        .await
        .unwrap();
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let pid = pekerjaan.to_string();
    let jenis = format!("UJI-JENIS-{}", std::process::id());

    // Store (PDF bukan gambar: tanpa thumbnail).
    let pdf = b"%PDF-1.4 uji berkas".to_vec();
    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/berkas",
        &token,
        Some(multipart(
            &[("pekerjaan_id", &pid), ("jenis_dokumen", &jenis)],
            Some(("RAB Final.pdf", &pdf)),
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let data = &created["data"];
    let id = data["id"].as_i64().unwrap();
    assert_eq!(data["jenis_dokumen"], jenis.as_str());
    assert_eq!(data["original_name"], "RAB Final");
    assert_eq!(data["mime_type"], "application/pdf");
    assert_eq!(data["size"], pdf.len() as u64);
    assert_eq!(data["uploader"]["id"], actor);
    assert_eq!(data["pekerjaan"]["id"], pekerjaan);
    let media_id = data["media_id"].as_u64().unwrap();
    let file_name = data["file_name"].as_str().unwrap().to_string();
    assert!(storage.join(media_id.to_string()).join(&file_name).exists());
    assert!(
        !storage
            .join(media_id.to_string())
            .join("conversions")
            .exists(),
        "berkas tidak punya thumbnail"
    );

    // Show: tanpa `uploader`, seperti resource Laravel saat relasi tidak dimuat.
    let (status, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/berkas/{id}"),
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(shown["data"].get("uploader").is_none());
    assert_eq!(
        shown["data"]["berkas_url"],
        format!("http://localhost/storage/{media_id}/{file_name}").as_str()
    );

    // Daftar dan jenis dokumen.
    let (status, list) = send(
        &pool,
        Method::GET,
        &format!("/api/berkas?search={jenis}"),
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == id));
    assert_eq!(list["meta"]["per_page"], 20);
    let (status, jenis_list) = send(
        &pool,
        Method::GET,
        "/api/berkas/jenis-dokumen",
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(jenis_list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|j| j == jenis.as_str()));

    // Update dengan _method=PUT dan berkas baru: berkas lama diganti.
    let png = b"\x89PNG\r\n\x1a\nbaru".to_vec();
    let (status, updated) = send(
        &pool,
        Method::POST,
        &format!("/api/berkas/{id}"),
        &token,
        Some(multipart(&[("_method", "PUT")], Some(("baru.png", &png)))),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["mime_type"], "image/png");
    assert!(
        !storage.join(media_id.to_string()).exists(),
        "direktori berkas lama dihapus"
    );
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM media WHERE model_type = 'App\\\\Models\\\\Berkas' AND model_id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(count, 1);

    // POST tanpa _method=PUT ditolak.
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/berkas/{id}"),
        &token,
        Some(multipart(&[("jenis_dokumen", "X")], None)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // Audit created, updated, dan notifikasi admin untuk admin lain tidak dibuat oleh pelaku sendiri.
    // Hanya ganti berkas tanpa ubah atribut: Laravel tidak memicu event `updated` (save() tidak dirty).
    for (event, expected) in [("created", 1), ("updated", 0)] {
        let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Berkas' AND auditable_id = ? AND event = ?")
            .bind(id)
            .bind(event)
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
        assert_eq!(n, expected, "audit {event}");
    }

    // Destroy.
    let (status, msg) = send(
        &pool,
        Method::DELETE,
        &format!("/api/berkas/{id}"),
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(msg["message"], "Berkas deleted successfully");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/berkas/{id}"),
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(&storage);
}

/// Pengawas melihat berkas miliknya dan berkas berjudul bersama (RAB), tidak berkas orang lain yang tidak bersama.
#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn pengawas_sees_own_and_shared_titles_only() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let tag = format!("UJI-SHARE-{}", std::process::id());
    let mail = "uji-berkas-pengawas@example.test";

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(mail)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Pengawas', ?, 'x', NOW(), NOW())")
        .bind(mail)
        .execute(&pool)
        .await
        .unwrap();
    let pengawas: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(mail)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('pengawas', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar(
        "SELECT id FROM roles WHERE name = 'pengawas' AND guard_name = 'web' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(pengawas)
        .execute(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, pengawas, "uji-berkas-pengawas")
        .await
        .unwrap();

    // Setting judul RAB aktif untuk pengawas (dikembalikan di akhir).
    let prev: Option<String> = sqlx::query_scalar("SELECT CAST(`value` AS CHAR) FROM app_settings WHERE `key` = 'pengawas_berkas_show_rab' LIMIT 1")
        .fetch_optional(&pool)
        .await
        .unwrap()
        .flatten();
    sqlx::query("DELETE FROM app_settings WHERE `key` = 'pengawas_berkas_show_rab'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES ('pengawas_berkas_show_rab', '1', 'text', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();

    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    // Pemilik berkas lain cukup user biasa dengan email sendiri (tidak menyentuh user ACTOR di test lain).
    let other_mail = "uji-berkas-lain@example.test";
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(other_mail)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Lain', ?, 'x', NOW(), NOW())")
        .bind(other_mail)
        .execute(&pool)
        .await
        .unwrap();
    let admin_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(other_mail)
        .fetch_one(&pool)
        .await
        .unwrap();
    let insert = |jenis: String, by: u64| {
        let pool = pool.clone();
        async move {
            sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
                .bind(pekerjaan)
                .bind(jenis)
                .bind(by)
                .execute(&pool)
                .await
                .unwrap()
                .last_insert_id() as i64
        }
    };
    let shared_other = insert(format!("RAB {tag}"), admin_id).await;
    let private_other = insert(format!("Lainnya {tag}"), admin_id).await;
    let own = insert(format!("Lainnya {tag}"), pengawas).await;

    let (status, list) = send(
        &pool,
        Method::GET,
        &format!("/api/berkas?search={tag}"),
        &token,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<i64> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_i64().unwrap())
        .collect();
    assert!(ids.contains(&shared_other), "judul RAB bersama terlihat");
    assert!(ids.contains(&own), "berkas sendiri terlihat");
    assert!(
        !ids.contains(&private_other),
        "berkas orang lain yang tidak bersama tidak terlihat"
    );

    // Bersihkan.
    sqlx::query("DELETE FROM tbl_berkas WHERE jenis_dokumen LIKE ?")
        .bind(format!("%{tag}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM app_settings WHERE `key` = 'pengawas_berkas_show_rab'")
        .execute(&pool)
        .await
        .unwrap();
    if let Some(v) = prev {
        sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES ('pengawas_berkas_show_rab', ?, 'text', NOW(), NOW())")
            .bind(v)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM model_has_roles WHERE model_id = ?")
        .bind(pengawas)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(pengawas)
        .execute(&pool)
        .await
        .unwrap();
}
