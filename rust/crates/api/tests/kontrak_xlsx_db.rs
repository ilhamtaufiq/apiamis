//! Ekspor, template, dan impor Excel kontrak terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kontrak_xlsx_db -- --include-ignored
//! ```

use std::io::Cursor;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use rust_xlsxwriter::{ExcelDateTime, Workbook};
use serde_json::Value;
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ADMIN: &str = "uji-kontrak-xlsx@example.test";
const PENYEDIA: &str = "UJI-KTR-XLSX penyedia";
const PAKET: &str = "UJI-KTR-XLSX Paket Impor";
const SPK: &str = "UJI-KTR-XLSX-SPK-1";
const BOUNDARY: &str = "----ujiktrxlsx";

const HEADINGS: [&str; 14] = [
    "Nama Paket (pisahkan dengan koma jika konsolidasi)",
    "Nama Penyedia",
    "Kode RUP",
    "Kode Paket",
    "Nomor Penawaran",
    "Tanggal Penawaran",
    "Nilai Kontrak",
    "Tanggal SPPBJ",
    "Nomor SPPBJ",
    "Tanggal SPK",
    "Nomor SPK",
    "Tanggal SPMK",
    "Nomor SPMK",
    "Tanggal Selesai Kontrak",
];

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
    body: Body,
    content_type: Option<String>,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let disposition = res
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, disposition, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

/// Berkas xlsx dengan header dan baris data. Kolom tanggal ditulis sebagai tanggal Excel asli.
fn build_import_xlsx(rows: &[(&str, &str, &str, (i32, u8, u8), &str)]) -> Vec<u8> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    for (c, h) in HEADINGS.iter().enumerate() {
        ws.write_string(0, c as u16, *h).unwrap();
    }
    for (i, (paket, penyedia, rup, (y, m, d), nilai)) in rows.iter().enumerate() {
        let r = (i + 1) as u32;
        ws.write_string(r, 0, *paket).unwrap();
        ws.write_string(r, 1, *penyedia).unwrap();
        ws.write_string(r, 2, *rup).unwrap();
        ws.write_string(r, 6, *nilai).unwrap();
        // Tanggal SPK (kolom J) sebagai tanggal Excel: penentu tahun acuan.
        let dt = ExcelDateTime::from_ymd(*y as u16, *m, *d).unwrap();
        ws.write_datetime(r, 9, &dt).unwrap();
        ws.write_string(r, 10, SPK).unwrap();
    }
    wb.save_to_buffer().unwrap()
}

fn multipart_file(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?").bind(ADMIN).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kontrak Xlsx', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?").bind(ADMIN).fetch_one(pool).await.unwrap();
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
    uid
}

/// Hapus sisa uji: kontrak ber-SPK uji, penyedia uji, dan pulihkan nama paket yang dipinjam.
async fn cleanup(pool: &MySqlPool, pekerjaan_id: Option<u64>, nama_asli: Option<&str>) {
    let ids: Vec<u64> = sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE spk = ?").bind(SPK).fetch_all(pool).await.unwrap();
    for id in ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?").bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = ?").bind(PENYEDIA).execute(pool).await.unwrap();
    if let (Some(id), Some(nama)) = (pekerjaan_id, nama_asli) {
        sqlx::query("UPDATE tbl_pekerjaan SET nama_paket = ? WHERE id = ?")
            .bind(nama)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn template_import_and_export_excel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    // Pinjam satu pekerjaan yang punya tahun anggaran. Nama dan tahunnya dipakai sebagai data uji.
    let row = sqlx::query(
        "SELECT p.id, p.nama_paket, CAST(kg.tahun_anggaran AS SIGNED) FROM tbl_pekerjaan p \
         JOIN tbl_kegiatan kg ON kg.id = p.kegiatan_id WHERE kg.tahun_anggaran IS NOT NULL ORDER BY p.id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let pekerjaan_id: u64 = row.try_get(0).unwrap();
    let nama_asli: String = row.try_get::<Option<String>, _>(1).unwrap().unwrap_or_default();
    let tahun: i64 = row.try_get(2).unwrap();
    let tahun = tahun as i32;

    cleanup(&pool, Some(pekerjaan_id), Some(&nama_asli)).await;
    sqlx::query("UPDATE tbl_pekerjaan SET nama_paket = ? WHERE id = ?")
        .bind(PAKET)
        .bind(pekerjaan_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES (?, 'Direktur Xlsx', '0', 'Notaris', 'Alamat', NOW(), NOW())")
        .bind(PENYEDIA)
        .execute(&pool)
        .await
        .unwrap();

    let admin = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, admin, "uji-kontrak-xlsx").await.unwrap();

    // Template: berkas xlsx dengan empat sheet.
    let (status, disposition, bytes) = send(&pool, Method::GET, "/api/kontrak/import/template", Some(&token), Body::empty(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disposition.as_deref(), Some("attachment; filename=\"template_kontrak.xlsx\""));
    assert_eq!(&bytes[..2], b"PK", "template harus berupa zip xlsx");
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.clone())).unwrap();
    let names = wb.sheet_names().to_vec();
    assert_eq!(names, vec!["Import Kontrak", "Validasi Import", "Referensi Pekerjaan", "Referensi Penyedia"]);
    let sheet = wb.worksheet_range("Import Kontrak").unwrap();
    assert_eq!(sheet.get_value((0, 0)), Some(&Data::String(HEADINGS[0].to_string())));
    assert_eq!(sheet.get_value((0, 13)), Some(&Data::String(HEADINGS[13].to_string())));
    let referensi = wb.worksheet_range("Referensi Pekerjaan").unwrap();
    assert!(
        referensi.rows().any(|r| r.first() == Some(&Data::String(PAKET.to_string()))),
        "paket uji harus ada di referensi"
    );

    // Impor: satu baris valid, satu baris paket tidak ada, satu baris kosong.
    let xlsx = build_import_xlsx(&[
        (PAKET, PENYEDIA, "UJI-RUP-XLSX-1", (tahun, 3, 1), "2500000"),
        ("UJI-KTR-XLSX Paket Tidak Ada", PENYEDIA, "UJI-RUP-XLSX-2", (tahun, 3, 1), "100"),
    ]);
    let payload = multipart_file("impor.xlsx", &xlsx);
    let ct = format!("multipart/form-data; boundary={BOUNDARY}");
    let (status, _, bytes) = send(&pool, Method::POST, "/api/kontrak/import", Some(&token), Body::from(payload.clone()), Some(ct.clone())).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let body = json_of(&bytes);
    assert_eq!(body["message"], "Import selesai", "{body}");
    assert_eq!(body["success_count"], 1, "{body}");
    assert_eq!(body["error_count"], 1, "{body}");
    assert_eq!(body["errors"][0]["row"], 3, "{body}");
    assert!(body["errors"][0]["message"].as_str().unwrap().contains("tidak ditemukan pada tahun anggaran"), "{body}");
    assert_eq!(body["debug"]["total_rows_excel"], 2, "{body}");

    let kontrak_id: u64 = sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE spk = ?").bind(SPK).fetch_one(&pool).await.unwrap();
    let (nilai, rup): (f64, String) = sqlx::query_as("SELECT CAST(nilai_kontrak AS DOUBLE), kode_rup FROM tbl_kontrak WHERE id = ?")
        .bind(kontrak_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!((nilai - 2_500_000.0).abs() < 0.01, "nilai = {nilai}");
    assert_eq!(rup, "UJI-RUP-XLSX-1");
    let linked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kontrak_pekerjaan WHERE kontrak_id = ? AND pekerjaan_id = ?")
        .bind(kontrak_id)
        .bind(pekerjaan_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(linked, 1);

    // Impor ulang: kontrak yang sama diperbarui, bukan dibuat baru.
    let (status, _, bytes) = send(&pool, Method::POST, "/api/kontrak/import", Some(&token), Body::from(payload), Some(ct)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&bytes)["success_count"], 1);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kontrak WHERE spk = ?").bind(SPK).fetch_one(&pool).await.unwrap();
    assert_eq!(total, 1, "impor ulang tidak boleh membuat kontrak kedua");
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ? ORDER BY id",
    )
    .bind(kontrak_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(events, vec!["created", "updated"]);

    // Ekspor: berkas xlsx dengan header tebal dan baris kontrak uji.
    let (status, disposition, bytes) = send(
        &pool,
        Method::GET,
        "/api/kontrak/export/excel?search=UJI-KTR-XLSX",
        Some(&token),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disposition.as_deref(), Some("attachment; filename=\"data_kontrak.xlsx\""));
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes)).unwrap();
    let sheet = wb.worksheet_range_at(0).unwrap().unwrap();
    assert_eq!(sheet.get_value((0, 0)), Some(&Data::String("Nama Pekerjaan".into())));
    assert_eq!(sheet.get_value((0, 11)), Some(&Data::String("Tanggal Selesai".into())));
    assert_eq!(sheet.rows().count(), 2, "satu header dan satu kontrak");
    assert_eq!(sheet.get_value((1, 0)), Some(&Data::String(PAKET.into())));
    assert_eq!(sheet.get_value((1, 4)), Some(&Data::String(PENYEDIA.into())));

    // Berkas tanpa file atau dengan tipe salah ditolak sebagai validasi.
    let (status, _, _) = send(
        &pool,
        Method::POST,
        "/api/kontrak/import",
        Some(&token),
        Body::from(format!("--{BOUNDARY}--\r\n")),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let txt = multipart_file("data.pdf", b"%PDF");
    let (status, _, _) = send(
        &pool,
        Method::POST,
        "/api/kontrak/import",
        Some(&token),
        Body::from(txt),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    cleanup(&pool, Some(pekerjaan_id), Some(&nama_asli)).await;
}
