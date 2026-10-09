//! Ekspor PDF checklist pekerjaan terhadap MySQL. Isi PDF dicocokkan dengan ekspor Excel
//! (sumber data yang sama) lewat `pdftotext -raw` (poppler).
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_checklist_pdf_db -- --include-ignored
//! ```

use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const USER: &str = "uji-cp-user@example.test";
const ADMIN: &str = "uji-cp-admin@example.test";
const ITEM_NAME: &str = "uji-cp-item";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Kirim permintaan; kembalikan status, header content-type, header content-disposition, dan body mentah.
async fn send_raw(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Option<String>, Option<String>, Vec<u8>) {
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
    let get = |name| {
        res.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let ct = get(header::CONTENT_TYPE);
    let cd = get(header::CONTENT_DISPOSITION);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, ct, cd, bytes.to_vec())
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (s, _, _, b) = send_raw(pool, method, uri, token, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// User uji dan token. Bila `admin`, diberi peran admin.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)",
    )
    .bind(email)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji CP', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-cp")
        .await
        .unwrap()
}

/// Hapus hanya baris milik tes ini: centang dan riwayat untuk item uji, lalu item uji itu sendiri.
async fn cleanup(pool: &MySqlPool, item: i64) {
    sqlx::query("DELETE FROM pekerjaan_checklist_histories WHERE checklist_item_id = ?")
        .bind(item)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM pekerjaan_checklist WHERE checklist_item_id = ?")
        .bind(item)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE id = ? AND name = ?")
        .bind(item)
        .bind(ITEM_NAME)
        .execute(pool)
        .await
        .unwrap();
}

/// Isi sel xlsx sebagai teks, seperti yang terlihat di Excel.
fn cell_text(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 => (*f as i64).to_string(),
        Data::Float(f) => f.to_string(),
        other => panic!("sel tak terduga di ekspor excel: {other:?}"),
    }
}

/// Baca sheet pertama ekspor Excel menjadi baris-baris teks (baris 0 = heading).
fn excel_rows(bytes: Vec<u8>) -> Vec<Vec<String>> {
    let mut wb: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes)).unwrap();
    let range = wb.worksheet_range_at(0).unwrap().unwrap();
    range
        .rows()
        .map(|r| r.iter().map(cell_text).collect())
        .collect()
}

/// Teks PDF lewat `pdftotext -raw` (urutan aliran isi, sama dengan urutan penggambaran).
fn pdf_text(bytes: &[u8], tag: &str) -> String {
    let path: PathBuf =
        std::env::temp_dir().join(format!("uji-cp-{tag}-{}.pdf", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new("pdftotext")
        .args(["-raw", path.to_str().unwrap(), "-"])
        .output()
        .expect("pdftotext (poppler-utils) harus terpasang untuk tes ini");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "pdftotext gagal");
    String::from_utf8(out.stdout).unwrap()
}

/// Buang semua spasi: pemenggalan baris pada PDF tidak mengubah isi.
fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn pekerjaan_checklist_pdf_matches_excel_rows() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();

    // Sisa tes sebelumnya: hanya item uji dan centangnya.
    sqlx::query("DELETE FROM pekerjaan_checklist WHERE checklist_item_id IN (SELECT id FROM tbl_checklist_items WHERE name = ?)")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_checklist_items WHERE name = ?")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_checklist_items (name, sort_order, context, created_at, updated_at) VALUES (?, 998, 'pekerjaan', NOW(), NOW())")
        .bind(ITEM_NAME)
        .execute(&pool)
        .await
        .unwrap();
    let item: i64 =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_checklist_items WHERE name = ?")
            .bind(ITEM_NAME)
            .fetch_one(&pool)
            .await
            .unwrap();
    let pekerjaan: i64 = sqlx::query_scalar("SELECT CAST(MIN(id) AS SIGNED) FROM tbl_pekerjaan")
        .fetch_one(&pool)
        .await
        .unwrap();
    let admin = user_token(&pool, ADMIN, true).await;
    let user = user_token(&pool, USER, false).await;

    // Centang oleh admin supaya kolom "Diubah Oleh" dan tanggal terisi.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan-checklist/toggle",
        Some(&admin),
        Some(json!({ "pekerjaan_id": pekerjaan, "checklist_item_id": item, "is_checked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Excel sebagai acuan isi tabel.
    let (status, _, _, xlsx) = send_raw(
        &pool,
        Method::GET,
        "/api/pekerjaan-checklist/export/excel",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let table = excel_rows(xlsx);
    let (headings, body_rows) = table.split_first().expect("heading excel");
    assert!(
        headings.iter().any(|h| h == ITEM_NAME),
        "kolom item uji tidak ada: {headings:?}"
    );

    // PDF: status, header, dan isi.
    let (status, ct, cd, pdf) = send_raw(
        &pool,
        Method::GET,
        "/api/pekerjaan-checklist/export/pdf",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&pdf));
    assert_eq!(ct.as_deref(), Some("application/pdf"));
    let cd = cd.unwrap();
    assert!(
        cd.starts_with("attachment; filename=\"checklist_pekerjaan_") && cd.ends_with(".pdf\""),
        "{cd}"
    );
    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(pdf.ends_with(b"%%EOF\n"));

    let text = pdf_text(&pdf, "semua");
    let squashed = squash(&text);
    assert!(
        squashed.contains(&squash(&format!("Total baris: {}", body_rows.len()))),
        "meta total baris tidak cocok: {}",
        body_rows.len()
    );
    assert!(squashed.contains(&squash("Checklist Pekerjaan")));
    for h in headings {
        assert!(squashed.contains(&squash(h)), "heading hilang di PDF: {h}");
    }
    // Setiap baris excel harus muncul di PDF dengan urutan sel yang sama.
    let mut cursor = 0usize;
    for (n, row) in body_rows.iter().enumerate() {
        let line = squash(&row.concat());
        match squashed[cursor..].find(&line) {
            Some(at) => cursor += at + line.len(),
            None => panic!(
                "baris {} excel tidak ditemukan berurutan di PDF: {line}",
                n + 1
            ),
        }
    }

    // Filter tanpa hasil: tabel kosong dengan baris "Tidak ada data".
    let (status, _, _, empty) = send_raw(
        &pool,
        Method::GET,
        "/api/pekerjaan-checklist/export/pdf?search=uji-cp-tidak-ada-paket",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let empty_text = squash(&pdf_text(&empty, "kosong"));
    assert!(
        empty_text.contains(&squash("Total baris: 0")),
        "{empty_text}"
    );
    assert!(
        empty_text.contains(&squash("Tidak ada data")),
        "{empty_text}"
    );

    // Pengguna biasa: ditolak sama seperti ekspor Excel.
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/pekerjaan-checklist/export/pdf",
        Some(&user),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    cleanup(&pool, item).await;
}
