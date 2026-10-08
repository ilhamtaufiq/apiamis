//! Impor dan templat Pekerjaan lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_import_db -- --include-ignored
//! ```

use std::io::Cursor;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use rust_xlsxwriter::Workbook;
use serde_json::Value;
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const MARK: &str = "uji-pimp-";
const BOUNDARY: &str = "----ujiimportboundary";

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
    token: &str,
    content_type: Option<String>,
    body: Vec<u8>,
) -> (StatusCode, header::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::USER_AGENT, "uji-agent");
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(builder.body(Body::from(body)).unwrap())
    .await
    .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

/// Body multipart dengan satu field `file`.
fn multipart(filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

/// User baru dengan satu role; email unik per tes. Mengulang bila kena deadlock.
async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> u64 {
    for attempt in 0..10u64 {
        let result: Result<u64, sqlx::Error> = async {
            sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await?;
            sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
                .bind(format!("Uji {email}"))
                .bind(email)
                .execute(pool)
                .await?;
            let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?").bind(email).fetch_one(pool).await?;
            sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
                .bind(role)
                .execute(pool)
                .await?;
            let rid: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
                .bind(role)
                .fetch_one(pool)
                .await?;
            sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
                .bind(rid)
                .bind(uid)
                .execute(pool)
                .await?;
            Ok(uid)
        }
        .await;
        match result {
            Ok(id) => return id,
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("40001") => {
                tokio::time::sleep(std::time::Duration::from_millis(20 * (attempt + 1))).await;
            }
            Err(e) => panic!("make_user: {e}"),
        }
    }
    panic!("make_user: deadlock berulang")
}

/// Jalankan satu statement tulis dan ulangi bila kena deadlock dengan tes lain (hanya untuk pembersihan).
async fn retry_db<F, Fut>(mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>>,
{
    for attempt in 1..=10u64 {
        match f().await {
            Ok(_) => return,
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("40001") => {
                tokio::time::sleep(std::time::Duration::from_millis(20 * attempt)).await;
            }
            Err(e) => panic!("statement gagal: {e}"),
        }
    }
    panic!("deadlock berulang pada statement tes")
}

/// Hapus sisa tes sebelumnya dengan awalan `tag` sendiri.
async fn purge(pool: &MySqlPool, tag: &str) {
    let like = format!("{MARK}{tag}%");
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
}

/// Kecamatan dan desa khusus tes. Mengembalikan (kec_id, nama kecamatan, nama desa).
async fn seed_region(pool: &MySqlPool, tag: &str) -> (u64, String, String) {
    purge(pool, tag).await;
    let kec_name = format!("{MARK}{tag}-kec");
    let desa_name = format!("{MARK}{tag}-desa");
    let kec = sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(&kec_name)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    sqlx::query("INSERT INTO tbl_desa (n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(&desa_name)
        .bind(kec)
        .execute(pool)
        .await
        .unwrap();
    (kec, kec_name, desa_name)
}

/// Satu kegiatan yang ada di DB lokal: (tahun, nama sub kegiatan).
async fn any_kegiatan(pool: &MySqlPool) -> (String, String) {
    let row = sqlx::query(
        "SELECT tahun_anggaran, nama_sub_kegiatan FROM tbl_kegiatan ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (
        row.try_get("tahun_anggaran").unwrap(),
        row.try_get("nama_sub_kegiatan").unwrap(),
    )
}

/// Berkas xlsx dengan heading standar dan baris data. `None` berarti sel kosong.
fn build_xlsx(rows: &[Vec<Option<String>>]) -> Vec<u8> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    for (col, h) in [
        "Kode Rekening",
        "Nama Paket",
        "Kecamatan",
        "Desa",
        "Kegiatan",
        "Tahun",
        "Pagu",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string(0, col as u16, *h).unwrap();
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if let Some(v) = cell {
                if let Ok(n) = v.parse::<f64>() {
                    if c == 6 {
                        ws.write_number((r + 1) as u32, c as u16, n).unwrap();
                        continue;
                    }
                }
                ws.write_string((r + 1) as u32, c as u16, v).unwrap();
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

async fn cleanup(pool: &MySqlPool, tag: &str, users: &[u64]) {
    purge(pool, tag).await;
    for u in users {
        retry_db(|| async move {
            sqlx::query("DELETE FROM notifications WHERE notifiable_id = ? AND data LIKE '%Import Pekerjaan Berhasil%'")
                .bind(u)
                .execute(pool)
                .await
        })
        .await;
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_inserts_rows_resolves_region_and_notifies_other_admins() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_user(&pool, "uji-pimp-a1@example.test", "admin").await;
    let other = make_user(&pool, "uji-pimp-a2@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pimp")
        .await
        .unwrap();
    let (kec, kec_name, desa_name) = seed_region(&pool, "ok").await;
    let (tahun, kegiatan) = any_kegiatan(&pool).await;

    let xlsx = build_xlsx(&[
        vec![
            s("1.01"),
            s("Paket Uji Import A"),
            s(&kec_name),
            s(&desa_name),
            s(&kegiatan),
            s(&tahun),
            s("Rp 500.000"),
        ],
        vec![
            None,
            s("Paket Uji Import B"),
            s(&kec_name),
            s("desa tidak ada"),
            None,
            None,
            s("1000"),
        ],
    ]);
    let (ct, body) = multipart("pekerjaan.xlsx", &xlsx);
    let (status, _, bytes) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan/import",
        &token,
        Some(ct),
        body,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        json_of(&bytes)["message"],
        "Data pekerjaan berhasil diimport"
    );

    let rows = sqlx::query("SELECT nama_paket, kode_rekening, kecamatan_id, desa_id, kegiatan_id, pagu, status FROM tbl_pekerjaan WHERE nama_paket LIKE 'Paket Uji Import%' AND kecamatan_id = ? ORDER BY nama_paket")
        .bind(kec)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let a = &rows[0];
    assert_eq!(
        a.try_get::<String, _>("nama_paket").unwrap(),
        "Paket Uji Import A"
    );
    assert_eq!(a.try_get::<String, _>("kode_rekening").unwrap(), "1.01");
    assert!(
        a.try_get::<Option<i64>, _>("desa_id").unwrap().is_some(),
        "desa cocok dengan LIKE"
    );
    assert!(
        a.try_get::<Option<i64>, _>("kegiatan_id")
            .unwrap()
            .is_some(),
        "kegiatan cocok tahun dan nama"
    );
    assert_eq!(a.try_get::<f32, _>("pagu").unwrap(), 500.0);
    assert_eq!(a.try_get::<String, _>("status").unwrap(), "active");
    let b = &rows[1];
    assert!(
        b.try_get::<Option<String>, _>("kode_rekening")
            .unwrap()
            .is_none(),
        "kode kosong menjadi NULL"
    );
    assert!(
        b.try_get::<Option<i64>, _>("desa_id").unwrap().is_none(),
        "desa tidak cocok menjadi NULL"
    );
    assert!(b
        .try_get::<Option<i64>, _>("kegiatan_id")
        .unwrap()
        .is_none());
    assert_eq!(b.try_get::<f32, _>("pagu").unwrap(), 1000.0);

    let audit: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND new_values LIKE '%Paket Uji Import%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        audit, 0,
        "impor tidak menulis audit per baris (withoutEvents)"
    );

    let notif: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND data LIKE '%Import Pekerjaan Berhasil%'")
        .bind(other)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(notif, 1, "admin lain diberi notifikasi");
    let self_notif: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND data LIKE '%Import Pekerjaan Berhasil%'")
        .bind(actor)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(self_notif, 0, "pengimpor tidak diberi notifikasi");

    sqlx::query("DELETE FROM tbl_pekerjaan WHERE kecamatan_id = ?")
        .bind(kec)
        .execute(&pool)
        .await
        .unwrap();
    cleanup(&pool, "ok", &[actor, other]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_with_bad_rows_returns_422_and_keeps_valid_rows() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_user(&pool, "uji-pimp-b1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pimp")
        .await
        .unwrap();
    let (_kec, kec_name, desa_name) = seed_region(&pool, "bad").await;

    let xlsx = build_xlsx(&[
        vec![
            None,
            s(&format!("{MARK}bad-valid")),
            s(&kec_name),
            s(&desa_name),
            None,
            None,
            s("10"),
        ],
        vec![None, None, s(&kec_name), None, None, None, s("20")],
    ]);
    let (ct, body) = multipart("rusak.xlsx", &xlsx);
    let (status, _, bytes) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan/import",
        &token,
        Some(ct),
        body,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let body = json_of(&bytes);
    assert_eq!(body["message"], "Import selesai dengan beberapa error");
    assert_eq!(body["error_count"], 1);
    let first = body["errors"][0].as_str().unwrap();
    assert!(first.starts_with("Baris 3: "), "{first}");
    assert!(
        first.contains("The nama paket field is required."),
        "{first}"
    );
    assert!(first.contains("The desa field is required."), "{first}");

    let saved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE nama_paket = ?")
        .bind(format!("{MARK}bad-valid"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, 1, "baris valid tetap tersimpan seperti Laravel");

    cleanup(&pool, "bad", &[actor]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn import_rejects_missing_or_wrong_file_and_template_downloads_three_sheets() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_user(&pool, "uji-pimp-c1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pimp")
        .await
        .unwrap();

    let (ct, body) = multipart("catatan.pdf", b"%PDF");
    let (status, _, bytes) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan/import",
        &token,
        Some(ct),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_of(&bytes)["errors"]["file"][0],
        "The file field must be a file of type: xlsx, xls, csv."
    );

    let empty = format!("--{BOUNDARY}--\r\n").into_bytes();
    let (status, _, bytes) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan/import",
        &token,
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
        empty,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_of(&bytes)["errors"]["file"][0],
        "The file field is required."
    );

    let (status, headers, bytes) = send(
        &pool,
        Method::GET,
        "/api/pekerjaan/import/template",
        &token,
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"template_import_pekerjaan.xlsx\""
    );
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes)).unwrap();
    let names = wb.sheet_names().to_vec();
    assert_eq!(
        names,
        vec!["Template", "Ref Kecamatan & Desa", "Ref Kegiatan"]
    );
    let template = wb.worksheet_range("Template").unwrap();
    let heading: Vec<String> = template
        .rows()
        .next()
        .unwrap()
        .iter()
        .map(|c| match c {
            Data::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect();
    assert_eq!(
        heading,
        vec![
            "Kode Rekening",
            "Nama Paket",
            "Kecamatan",
            "Desa",
            "Kegiatan",
            "Tahun",
            "Pagu"
        ]
    );
    assert_eq!(
        template.rows().count(),
        1,
        "sheet Template hanya berisi heading"
    );

    cleanup(&pool, "c", &[actor]).await;
}
