//! Impor CSV `POST /api/spam-units/import` terhadap MySQL.
//!
//! Semua tulisan inti dijalankan di dalam transaksi tes yang selalu di-rollback, jadi tidak ada baris
//! yang tertinggal. Ini juga menguji `DELETE FROM tbl_spam_budgets` (hapus semua anggaran) tanpa menyentuh
//! data nyata. Penanda data uji: `uji-si-`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test spam_import_db -- --include-ignored
//! ```

use std::path::PathBuf;

use api::{
    app,
    spam_import::{import_rows, parse_records, run_command, Summary},
    spam_integration::Ctx,
    AppState,
};
use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const URL: &str = "http://localhost/api/spam-units/import";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

const CSV: &str = "\
Nilai Kontrak;QueryYear;Nama Paket;Desa/Kelurahan;Kecamatan
Rp 1.250.000,50;2025;uji-si-paket A;uji-si-desa-1;uji-si
Rp 2.000;2024;uji-si-paket B;uji-si-desa-baru;uji-si
;2024;uji-si-paket kosong;;uji-si
1000;2024;uji-si-paket C;uji-si-desa-1;uji-si-kec-tidak-ada
";

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_rows_writes_and_rolls_back() {
    let pool = pool().await;
    let mut tx = pool.begin().await.unwrap();

    // Aktor dan wilayah uji, hanya di dalam transaksi ini.
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Impor', 'uji-si-admin@example.test', 'x', NOW(), NOW())")
        .execute(&mut *tx)
        .await
        .unwrap();
    let actor: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = 'uji-si-admin@example.test'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES ('uji-si', NOW(), NOW())")
        .execute(&mut *tx)
        .await
        .unwrap();
    let kec: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_kecamatan WHERE n_kec = 'uji-si'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_desa (n_desa, kecamatan_id, bjp_master, created_at, updated_at) VALUES ('uji-si-desa-1', ?, 0, NOW(), NOW())")
        .bind(kec)
        .execute(&mut *tx)
        .await
        .unwrap();
    let desa: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_desa WHERE n_desa = 'uji-si-desa-1'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_unit_spam (desa_id, name, is_simspam, created_at, updated_at) VALUES (?, 'uji-si-unit-lama', 0, NOW(), NOW())")
        .bind(desa)
        .execute(&mut *tx)
        .await
        .unwrap();
    let unit_lama: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_unit_spam WHERE name = 'uji-si-unit-lama'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    // Anggaran lama: harus ikut terhapus oleh `DELETE` awal impor.
    sqlx::query("INSERT INTO tbl_spam_budgets (unit_spam_id, nilai_kontrak, tahun, nama_paket, sumber_dana, created_at, updated_at) VALUES (?, 5, '2020', 'uji-si-lama', 'APBD', NOW(), NOW())")
        .bind(unit_lama)
        .execute(&mut *tx)
        .await
        .unwrap();

    let records = parse_records(CSV.as_bytes());
    // Baris pertama adalah header, sama dengan `fgetcsv` pertama di command.
    let rows = &records[1..];
    let headers = HeaderMap::new();
    let ctx = Ctx {
        user: Some(actor),
        roles: &[],
        url: URL,
        headers: &headers,
    };
    let mut out = String::new();
    let summary = import_rows(&mut tx, &ctx, actor, rows, &mut out).await.unwrap();

    assert_eq!(
        summary,
        Summary {
            rows: 3,
            matched: 2,
            created_desa: 1,
            created_unit: 1,
        }
    );
    assert!(
        out.contains("Row 3: Kecamatan 'uji-si-kec-tidak-ada' not found. Skipping."),
        "output: {out}"
    );

    // Hanya dua anggaran dari CSV yang tersisa: anggaran lama terhapus.
    let budgets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_spam_budgets")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(budgets, 2);
    let lama: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_spam_budgets WHERE nama_paket = 'uji-si-lama'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(lama, 0);

    let nilai: f64 = sqlx::query_scalar("SELECT nilai_kontrak FROM tbl_spam_budgets WHERE nama_paket = 'uji-si-paket A'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(nilai, 1250000.5);

    // Desa baru dibuat di kecamatan yang sama, lengkap dengan unit dan pengelola default.
    let desa_baru_kec: i64 = sqlx::query_scalar("SELECT CAST(kecamatan_id AS SIGNED) FROM tbl_desa WHERE n_desa = 'uji-si-desa-baru'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(desa_baru_kec, kec);
    let unit_name: String = sqlx::query_scalar("SELECT name FROM tbl_unit_spam WHERE desa_id = (SELECT id FROM tbl_desa WHERE n_desa = 'uji-si-desa-baru')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(unit_name, "SPAM UJI-SI-DESA-BARU UJI-SI");
    let pokmas: String = sqlx::query_scalar("SELECT p.pokmas FROM tbl_pengelola p JOIN tbl_unit_spam u ON u.id = p.unit_spam_id WHERE u.name = 'SPAM UJI-SI-DESA-BARU UJI-SI'")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(pokmas, "KPSPAM UJI-SI-DESA-BARU UJI-SI");

    // Audit `created` per model yang dibuat, dengan url impor (`Auditable` saat request HTTP).
    let audit_budget: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpamBudget' AND event = 'created' AND user_id = ? AND url = ?")
        .bind(actor)
        .bind(URL)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(audit_budget, 2);

    tx.rollback().await.unwrap();

    // Setelah rollback, tidak ada sisa data uji di database.
    let sisa: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kecamatan WHERE n_kec = 'uji-si'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sisa, 0);
    let sisa_desa: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_desa WHERE n_desa LIKE 'uji-si-%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sisa_desa, 0);
    let sisa_user: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = 'uji-si-admin@example.test'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sisa_user, 0);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn run_command_reports_missing_and_empty_file() {
    let pool = pool().await;
    let headers = HeaderMap::new();
    let ctx = Ctx {
        user: Some(1),
        roles: &[],
        url: URL,
        headers: &headers,
    };

    let dir: PathBuf = std::env::temp_dir().join(format!("uji-si-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let missing = dir.join("tidak-ada.csv");
    let out = run_command(&pool, &ctx, 1, &missing).await.unwrap();
    assert_eq!(out, format!("CSV file not found at: {}\n", missing.display()));

    let empty = dir.join("kosong.csv");
    std::fs::write(&empty, b"").unwrap();
    let out = run_command(&pool, &ctx, 1, &empty).await.unwrap();
    assert_eq!(
        out,
        format!(
            "Opening and parsing SPSE CSV file from: {}...\nCSV file is empty.\n",
            empty.display()
        )
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_without_token_is_401() {
    let pool = pool().await;
    let res = app(
        &config(),
        AppState::new(pool, "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/api/spam-units/import")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

/// Admin uji dengan token. Baris user, role, dan token dihapus lagi di akhir tes.
async fn admin_with_token(pool: &MySqlPool) -> (u64, String) {
    const EMAIL: &str = "uji-si-http@example.test";
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Impor HTTP', ?, 'x', NOW(), NOW())")
        .bind(EMAIL)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(EMAIL)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-si-http").await.unwrap();
    (uid, token)
}

async fn cleanup_admin(pool: &MySqlPool, uid: u64) {
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_type = 'App\\\\Models\\\\User' AND tokenable_id = ?")
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_id = ? AND model_type = 'App\\\\Models\\\\User'")
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
}

fn multipart_body(filename: Option<&str>, content: &[u8]) -> (String, Vec<u8>) {
    let boundary = "ujisiBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    match filename {
        Some(name) => body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: text/csv\r\n\r\n"
            )
            .as_bytes(),
        ),
        None => body.extend_from_slice(b"Content-Disposition: form-data; name=\"catatan\"\r\n\r\n"),
    }
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn post_import(pool: &MySqlPool, token: &str, content_type: &str, body: Vec<u8>) -> (StatusCode, serde_json::Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/api/spam-units/import")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_validation_is_422_like_laravel() {
    let pool = pool().await;
    let (uid, token) = admin_with_token(&pool).await;

    // Tanpa field `file`.
    let (ct, body) = multipart_body(None, b"x");
    let (status, json) = post_import(&pool, &token, &ct, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["message"], "The given data was invalid.");
    assert_eq!(json["errors"]["file"][0], "The file field is required.");

    // Ekstensi bukan csv/txt.
    let (ct, body) = multipart_body(Some("data.xlsx"), b"PK");
    let (status, json) = post_import(&pool, &token, &ct, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json["errors"]["file"][0],
        "The file field must be a file of type: csv, txt."
    );

    // Bukan multipart sama sekali: Laravel menganggap berkas tidak ada.
    let (status, json) = post_import(
        &pool,
        &token,
        "application/json",
        b"{\"file\":\"x\"}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json["errors"]["file"][0], "The file field is required.");

    cleanup_admin(&pool, uid).await;
}
