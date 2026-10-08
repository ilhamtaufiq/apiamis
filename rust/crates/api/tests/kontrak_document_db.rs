//! Dokumen kontrak lewat router terhadap MySQL: SPK (`export`), BAP dan `bap-context`, serta validasi cover.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kontrak_document_db -- --include-ignored
//! ```

use std::io::{Cursor, Read};

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-dokumen-admin@example.test";
const PREFIX: &str = "UJI-DOC-";
const PENYEDIA_NAMA: &str = "UJI-DOC penyedia";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn send_raw(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
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
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, bytes) = send_raw(pool, method, uri, token, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Dokumen', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
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

async fn seed_penyedia(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = ?")
        .bind(PENYEDIA_NAMA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES (?, 'Direktur Uji', '0', 'Notaris Uji', 'Alamat Uji', NOW(), NOW())")
        .bind(PENYEDIA_NAMA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_penyedia WHERE nama = ? ORDER BY id DESC LIMIT 1")
        .bind(PENYEDIA_NAMA)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus kontrak uji (`kode_paket` berawalan `UJI-DOC-`) beserta pivot dan addendumnya.
async fn cleanup(pool: &MySqlPool) {
    let pattern = format!("{PREFIX}%");
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE kode_paket LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await
        .unwrap();
    for id in ids {
        sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM kontrak_pekerjaan WHERE kontrak_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_endpoints_follow_bastp_and_template_rules() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    let admin = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, admin, "uji-dokumen")
        .await
        .unwrap();
    let penyedia = seed_penyedia(&pool).await;
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kontrak",
        Some(&token),
        Some(json!({
            "id_penyedia": penyedia,
            "pekerjaan_ids": [pekerjaan],
            "kode_paket": format!("{PREFIX}A"),
            "nilai_kontrak": "1000000",
            "tgl_spk": "2026-01-10",
            "tgl_spmk": "2026-01-12",
            "tgl_selesai": "2026-04-11",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_u64().expect("id kontrak");

    // bap-context: tanpa register BASTP, dokumen BAP belum bisa dibuat.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}/bap-context"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["can_generate"], json!(false), "{body}");
    assert_eq!(body["missing"], json!(["bastp"]), "{body}");
    assert_eq!(body["nilai_kontrak_awal"], json!(1000000.0), "{body}");

    // export-bap: 422 dengan daftar yang kurang.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}/export-bap"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["missing"], json!(["bastp"]), "{body}");

    // SPK: berkas .docx yang placeholdernya sudah terisi.
    let (status, headers, bytes) = send_raw(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}/export"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("wordprocessingml"),
        "{headers:?}"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Rp. 1.000.000"), "nilai kontrak terisi");
    assert!(!xml.contains("{nilai_kontrak}"), "placeholder tersisa");

    // Kontrak tidak ada, dan id pekerjaan yang tidak terkait juga tidak ada: 404.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/kontrak/999999999/export",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "Kontrak not found", "{body}");

    // Tahun di luar rentang validasi cover.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/kontrak/export-all-covers?tahun=1999",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    cleanup(&pool).await;
}
