//! Tulis penyedia lewat router terhadap MySQL: JSON dan multipart (dokumen), audit, dan notifikasi admin.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test penyedia_write_db -- --include-ignored
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

const ADMIN_A: &str = "uji-pny-admin-a@example.test";
const ADMIN_B: &str = "uji-pny-admin-b@example.test";
const NAMA: &str = "UJI-PNY Penyedia";
const BOUNDARY: &str = "----ujipenyediaboundary";

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
    let storage = std::env::temp_dir().join(format!("uji-penyedia-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
}

/// Body multipart: field teks, `delete_dokumen` (id), dan berkas `dokumen[]`.
fn multipart(fields: &[(&str, &str)], files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    for (name, bytes) in files {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"dokumen[]\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
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
    body: Body,
    content_type: Option<String>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
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

async fn json_req(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    v: Value,
) -> (StatusCode, Value) {
    send(
        pool,
        method,
        uri,
        token,
        Body::from(v.to_string()),
        Some("application/json".into()),
    )
    .await
}

async fn make_admin(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Penyedia', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())").execute(pool).await.unwrap();
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

async fn cleanup(pool: &MySqlPool, admin_b: u64) {
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM tbl_penyedia WHERE nama = ?")
        .bind(NAMA)
        .fetch_all(pool)
        .await
        .unwrap();
    for id in ids {
        sqlx::query(
            "DELETE FROM media WHERE model_type = 'App\\\\Models\\\\Penyedia' AND model_id = ?",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Penyedia' AND auditable_id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = ?")
        .bind(NAMA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM notifications WHERE notifiable_id = ?")
        .bind(admin_b)
        .execute(pool)
        .await
        .unwrap();
}

async fn audit_events(pool: &MySqlPool, id: u64) -> Vec<String> {
    sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Penyedia' AND auditable_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}

fn base_payload() -> Value {
    json!({
        "nama": NAMA,
        "direktur": "Direktur Uji",
        "no_akta": "AKTA-UJI-1",
        "notaris": "Notaris Uji",
        "tanggal_akta": "2099-02-03",
        "alamat": "Jalan Uji 1",
        "npwp": "00.000",
    })
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn penyedia_write_json_and_multipart_documents_with_audit() {
    use_storage();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin_a = make_admin(&pool, ADMIN_A).await;
    let admin_b = make_admin(&pool, ADMIN_B).await;
    let token = auth::login::create_token(&pool, admin_a, "uji-pny")
        .await
        .unwrap();
    cleanup(&pool, admin_b).await;

    // Create JSON: respon memuat dokumen kosong, audit created, notifikasi dengan tautan edit.
    let (status, body) =
        json_req(&pool, Method::POST, "/api/penyedia", &token, base_payload()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama"], NAMA, "{body}");
    assert_eq!(body["data"]["dokumen"], json!([]), "{body}");
    let id: u64 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);
    let data: String = sqlx::query_scalar("SELECT CAST(data AS CHAR) FROM notifications WHERE notifiable_id = ? ORDER BY created_at DESC, id DESC LIMIT 1")
        .bind(admin_b)
        .fetch_one(&pool)
        .await
        .unwrap();
    let n: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(n["title"], "Data Penyedia dibuat");
    assert_eq!(n["url"], json!(format!("/penyedia/{id}/edit")));

    // Validasi: field wajib hilang ditolak.
    let (status, body) = json_req(
        &pool,
        Method::POST,
        "/api/penyedia",
        &token,
        json!({ "nama": NAMA }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["direktur"].is_array(), "{body}");

    // Update JSON tanpa perubahan nilai: tidak ada audit baru.
    let (status, body) = json_req(
        &pool,
        Method::PUT,
        &format!("/api/penyedia/{id}"),
        &token,
        base_payload(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Update: alamat berubah dan npwp dikosongkan (null eksplisit tersimpan).
    let mut next = base_payload();
    next["alamat"] = json!("Jalan Uji 2");
    next["npwp"] = Value::Null;
    let (status, body) = json_req(
        &pool,
        Method::PATCH,
        &format!("/api/penyedia/{id}"),
        &token,
        next,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["alamat"], "Jalan Uji 2", "{body}");
    assert!(body["data"]["npwp"].is_null(), "{body}");
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);

    // Multipart: tambah dua dokumen lalu hapus satu melalui delete_dokumen.
    let body_mp = multipart(
        &[
            ("nama", NAMA),
            ("direktur", "Direktur Uji"),
            ("no_akta", "AKTA-UJI-1"),
            ("notaris", "Notaris Uji"),
            ("tanggal_akta", "2099-02-03"),
            ("alamat", "Jalan Uji 2"),
        ],
        &[("akta-a.pdf", b"%PDF-1.4 a"), ("akta-b.pdf", b"%PDF-1.4 b")],
    );
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/penyedia/{id}"),
        &token,
        Body::from(body_mp),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await;
    // POST tanpa `_method` ditolak 405, seperti Laravel tanpa rute POST ke /penyedia/{id}.
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{body}");

    let body_mp = multipart(
        &[
            ("nama", NAMA),
            ("direktur", "Direktur Uji"),
            ("no_akta", "AKTA-UJI-1"),
            ("notaris", "Notaris Uji"),
            ("tanggal_akta", "2099-02-03"),
            ("alamat", "Jalan Uji 3"),
            ("_method", "PUT"),
        ],
        &[("akta-a.pdf", b"%PDF-1.4 a"), ("akta-b.pdf", b"%PDF-1.4 b")],
    );
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/penyedia/{id}"),
        &token,
        Body::from(body_mp),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let docs = body["data"]["dokumen"].as_array().unwrap();
    assert_eq!(docs.len(), 2, "{body}");
    // `name` adalah `file_name` (UUID di disk), sama dengan sisi baca yang sudah dipindah.
    assert!(
        docs[0]["name"].as_str().unwrap().ends_with(".pdf"),
        "{body}"
    );
    let first_id = docs[0]["id"].as_u64().unwrap();

    let body_mp = multipart(
        &[
            ("nama", NAMA),
            ("direktur", "Direktur Uji"),
            ("no_akta", "AKTA-UJI-1"),
            ("notaris", "Notaris Uji"),
            ("tanggal_akta", "2099-02-03"),
            ("alamat", "Jalan Uji 3"),
            ("_method", "PUT"),
            ("delete_dokumen", &first_id.to_string()),
        ],
        &[],
    );
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/penyedia/{id}"),
        &token,
        Body::from(body_mp),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["dokumen"].as_array().unwrap().len(),
        1,
        "{body}"
    );

    // Hapus: baris, audit deleted, dokumen ikut hilang, dan notifikasi hapus.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/penyedia/{id}"),
        &token,
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Penyedia deleted successfully");
    assert_eq!(
        audit_events(&pool, id).await,
        vec!["created", "updated", "updated", "deleted"]
    );
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE model_type = 'App\\\\Models\\\\Penyedia' AND model_id = ?").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(remaining, 0);
    let (status, _) = json_req(
        &pool,
        Method::PUT,
        &format!("/api/penyedia/{id}"),
        &token,
        base_payload(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, admin_b).await;
}
