//! Celah register addendum (`register-gaps`) lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kontrak_register_gap_db -- --include-ignored
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

const ADMIN: &str = "uji-gap-admin@example.test";
const SIN_ROLE: &str = "uji-gap-norole@example.test";
const PENYEDIA: &str = "UJI-GAP penyedia";
const KODE_A: &str = "UJI-GAP-KONTRAK-A";
const KODE_B: &str = "UJI-GAP-KONTRAK-B";
const NOMOR_GAP: &str = "UJI-GAP  nomor   2099/01";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
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

async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Gap', ?, 'x', NOW(), NOW())")
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

async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_document_registers WHERE nomor LIKE 'UJI-GAP%'")
        .execute(pool)
        .await
        .unwrap();
    let kontraks: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE kode_paket LIKE 'UJI-GAP-%'")
            .fetch_all(pool)
            .await
            .unwrap();
    for kid in kontraks {
        sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
            .bind(kid)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?")
            .bind(kid)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = ?")
        .bind(PENYEDIA)
        .execute(pool)
        .await
        .unwrap();
}

async fn make_kontrak(pool: &MySqlPool, kode: &str, penyedia: u64, pekerjaan: u64) -> u64 {
    sqlx::query("INSERT INTO tbl_kontrak (id_pekerjaan, id_penyedia, kode_paket, spk, nilai_kontrak, tgl_spk, created_at, updated_at) VALUES (?, ?, ?, 'SPK-UJI-GAP', 500000, '2099-01-10', NOW(), NOW())")
        .bind(pekerjaan)
        .bind(penyedia)
        .bind(kode)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE kode_paket = ? ORDER BY id DESC LIMIT 1")
        .bind(kode)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn register_gaps_hide_resolved_and_respect_access() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    // Tipe ADD: dibuat bila belum ada, dan dihapus di akhir.
    let type_existing: Option<u64> = sqlx::query_scalar("SELECT id FROM tbl_document_types WHERE LOWER(TRIM(code)) IN ('add','addendum') ORDER BY id LIMIT 1")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let type_id = match type_existing {
        Some(id) => id,
        None => {
            sqlx::query("INSERT INTO tbl_document_types (name, code, created_at, updated_at) VALUES ('Addendum uji gap', 'ADD', NOW(), NOW())")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query_scalar(
                "SELECT id FROM tbl_document_types WHERE code = 'ADD' ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES (?, 'Direktur', '0', 'Notaris', 'Alamat', NOW(), NOW())")
        .bind(PENYEDIA)
        .execute(&pool)
        .await
        .unwrap();
    let penyedia: u64 =
        sqlx::query_scalar("SELECT id FROM tbl_penyedia WHERE nama = ? ORDER BY id DESC LIMIT 1")
            .bind(PENYEDIA)
            .fetch_one(&pool)
            .await
            .unwrap();
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Kontrak A: register tanpa addendum, muncul sebagai celah.
    let kontrak_a = make_kontrak(&pool, KODE_A, penyedia, pekerjaan).await;
    sqlx::query("INSERT INTO tbl_document_registers (kontrak_id, type_id, addendum_id, attachment_type, nomor, tanggal, sequence_number, year, description, nilai, created_at, updated_at) VALUES (?, ?, NULL, NULL, ?, '2099-01-15', 1, 2099, 'Keterangan uji', 750000, NOW(), NOW())")
        .bind(kontrak_a)
        .bind(type_id)
        .bind(NOMOR_GAP)
        .execute(&pool)
        .await
        .unwrap();
    let register_a: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_document_registers WHERE kontrak_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(kontrak_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Kontrak B: register yang sudah punya addendum (apa pun statusnya) bukan celah.
    let kontrak_b = make_kontrak(&pool, KODE_B, penyedia, pekerjaan).await;
    sqlx::query("INSERT INTO tbl_document_registers (kontrak_id, type_id, addendum_id, attachment_type, nomor, tanggal, sequence_number, year, created_at, updated_at) VALUES (?, ?, NULL, NULL, 'UJI-GAP-B-001', '2099-01-16', 2, 2099, NOW(), NOW())")
        .bind(kontrak_b)
        .bind(type_id)
        .execute(&pool)
        .await
        .unwrap();
    let register_b: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_document_registers WHERE kontrak_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(kontrak_b)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tbl_kontrak_addendums (kontrak_id, addendum_ke, tanggal_addendum, jenis_addendum, status, kelengkapan_override, created_at, updated_at) VALUES (?, 1, '2099-01-20', 'lainnya', 'draft', 0, NOW(), NOW())")
        .bind(kontrak_b)
        .execute(&pool)
        .await
        .unwrap();

    let admin = make_user(&pool, ADMIN, true).await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-gap")
        .await
        .unwrap();
    let norole = make_user(&pool, SIN_ROLE, false).await;
    let norole_token = auth::login::create_token(&pool, norole, "uji-gap")
        .await
        .unwrap();

    // Daftar celah admin: memuat register A dengan nomor dirapikan spasinya, tanpa register B.
    let (status, body) = get(&pool, "/api/kontrak-addendums/register-gaps", &admin_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["type_codes"], json!(["add", "addendum"]), "{body}");
    let items = body["items"].as_array().unwrap();
    let a = items
        .iter()
        .find(|i| i["register_id"] == json!(register_a))
        .unwrap_or_else(|| panic!("register A harus ada: {body}"));
    assert_eq!(
        a["nomor_register"], NOMOR_GAP,
        "nomor asli dipertahankan: {body}"
    );
    assert_eq!(a["kontrak_id"], json!(kontrak_a), "{body}");
    assert_eq!(a["addendum_count"], 0, "{body}");
    assert_eq!(a["tanggal_register"], "2099-01-15", "{body}");
    assert_eq!(a["description"], "Keterangan uji", "{body}");
    assert_eq!(a["type_code"], "ADD", "{body}");
    assert_eq!(a["penyedia"]["id"], json!(penyedia), "{body}");
    assert!(
        items.iter().all(|i| i["register_id"] != json!(register_b)),
        "register B sudah punya addendum: {body}"
    );
    assert_eq!(body["total"], json!(items.len()), "{body}");

    // Per kontrak: hanya milik kontrak A.
    let (status, body) = get(
        &pool,
        &format!("/api/kontrak/{kontrak_a}/addendum-register-gaps"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let own = body["items"].as_array().unwrap();
    assert_eq!(own.len(), 1, "{body}");
    assert_eq!(own[0]["register_id"], json!(register_a), "{body}");
    let (status, body) = get(
        &pool,
        &format!("/api/kontrak/{kontrak_b}/addendum-register-gaps"),
        &admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 0, "{body}");

    // Akses: tanpa role tidak boleh daftar admin, dan tidak boleh melihat kontrak yang bukan miliknya.
    let (status, body) = get(&pool, "/api/kontrak-addendums/register-gaps", &norole_token).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Hanya admin yang boleh melakukan aksi ini");
    let (status, body) = get(
        &pool,
        &format!("/api/kontrak/{kontrak_a}/addendum-register-gaps"),
        &norole_token,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Anda tidak memiliki akses ke kontrak ini");

    cleanup(&pool).await;
    if type_existing.is_none() {
        sqlx::query("DELETE FROM tbl_document_types WHERE id = ?")
            .bind(type_id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
