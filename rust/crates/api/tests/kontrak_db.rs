//! `/api/kontrak` lewat router terhadap MySQL: CRUD, relasi pekerjaan, addendum, audit, dan validasi.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kontrak_db -- --include-ignored
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

const ADMIN: &str = "uji-kontrak-admin@example.test";
const PREFIX: &str = "UJI-KTR-";
const PENYEDIA_NAMA: &str = "UJI-KTR penyedia";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
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
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Buat user admin uji. Role `admin` memberi akses penuh ke paket.
async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kontrak', ?, 'x', NOW(), NOW())")
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

/// Pastikan ada penyedia uji; kembalikan idnya.
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

/// Hapus sisa uji (kontrak bertanda `kode_paket` berawalan `UJI-KTR-`, addendumnya, dan auditnya).
async fn cleanup(pool: &MySqlPool) {
    let pattern = format!("{PREFIX}%");
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE kode_paket LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await
        .unwrap();
    for id in ids {
        sqlx::query("DELETE FROM tbl_kontrak_addendum_items WHERE addendum_id IN (SELECT id FROM tbl_kontrak_addendums WHERE kontrak_id = ?)")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ?")
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
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = ?")
        .bind(PENYEDIA_NAMA)
        .execute(pool)
        .await
        .unwrap();
}

async fn audit_events(pool: &MySqlPool, kontrak_id: u64) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ? ORDER BY id",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kontrak_crud_relations_addendum_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    let admin = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, admin, "uji-kontrak")
        .await
        .unwrap();
    let penyedia = seed_penyedia(&pool).await;
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Validasi: penyedia wajib saat create.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kontrak",
        Some(&token),
        Some(json!({ "pekerjaan_ids": [pekerjaan] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["id_penyedia"].is_array(), "{body}");

    // Validasi: pekerjaan wajib dipilih.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kontrak",
        Some(&token),
        Some(json!({ "id_penyedia": penyedia, "kode_paket": format!("{PREFIX}TANPA-PEKERJAAN") })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Minimal satu pekerjaan harus dipilih");

    // Create dengan relasi pekerjaan dan nilai string seperti form Laravel.
    let kode = format!("{PREFIX}A");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kontrak",
        Some(&token),
        Some(json!({
            "id_penyedia": penyedia,
            "pekerjaan_ids": [pekerjaan],
            "kode_paket": kode,
            "nilai_kontrak": "1000000.50",
            "tgl_spk": "2026-01-10",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_u64().expect("id kontrak");
    assert_eq!(body["data"]["kode_paket"], kode, "{body}");

    let linked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM kontrak_pekerjaan WHERE kontrak_id = ? AND pekerjaan_id = ?",
    )
    .bind(id)
    .bind(pekerjaan)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(linked, 1, "pivot kontrak_pekerjaan harus terisi");
    assert_eq!(audit_events(&pool, id).await, vec!["created"]);

    // Show: detail dengan addendum kosong.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], id, "{body}");
    assert_eq!(body["data"]["addendums"], json!([]), "{body}");

    // Addendum disetujui langsung di DB; show harus menampilkannya.
    sqlx::query(
        "INSERT INTO tbl_kontrak_addendums (kontrak_id, addendum_ke, nomor_addendum, tanggal_addendum, jenis_addendum, \
         nilai_kontrak_sebelum, nilai_kontrak_sesudah, status, created_by, approved_by, approved_at, created_at, updated_at) \
         VALUES (?, 1, 'UJI-ADD-1', '2026-03-01', 'biaya', 1000000.50, 1200000.50, 'disetujui', ?, ?, NOW(), NOW(), NOW())",
    )
    .bind(id)
    .bind(admin)
    .bind(admin)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let addendums = body["data"]["addendums"]
        .as_array()
        .expect("addendums array");
    assert_eq!(addendums.len(), 1, "{body}");
    assert_eq!(addendums[0]["nomor_addendum"], "UJI-ADD-1", "{body}");

    // Daftar dan relasi pekerjaan/kegiatan/penyedia memuat kontrak ini.
    let (status, body) = send(&pool, Method::GET, "/api/kontrak", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["id"] == id),
        "{body}"
    );
    assert_eq!(body["meta"]["per_page"], 20, "{body}");

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/pekerjaan/{pekerjaan}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["id"] == id),
        "{body}"
    );

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/penyedia/{penyedia}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["id"] == id),
        "{body}"
    );

    // Update: nilai berubah dan tercatat di audit; PATCH juga diterima.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kontrak/{id}"),
        Some(&token),
        Some(json!({ "nilai_kontrak": "2000000" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let nilai: f64 =
        sqlx::query_scalar("SELECT CAST(nilai_kontrak AS DOUBLE) FROM tbl_kontrak WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        (nilai - 2_000_000.0).abs() < 0.01,
        "nilai_kontrak = {nilai}"
    );
    assert_eq!(audit_events(&pool, id).await, vec!["created", "updated"]);

    // Delete: baris dan pivot hilang.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kontrak/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kontrak deleted successfully");
    let remain: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kontrak WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remain, 0);
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool).await;
}
