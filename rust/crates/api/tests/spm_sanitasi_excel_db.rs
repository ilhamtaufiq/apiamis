//! SPM sanitasi Excel lewat router: ekspor, template, dan impor (xlsx dibangun di tes).
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test spm_sanitasi_excel_db -- --include-ignored
//! ```
//!
//! Jalur `replace=true` tidak diuji: ia menghapus seluruh `tbl_spm_sanitasi` yang dipakai bersama.

use std::io::Cursor;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use rust_xlsxwriter::Workbook;
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const BOUNDARY: &str = "uji-spm-xl-boundary";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 4 * 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Kirim permintaan dan kembalikan status, header, dan badan mentah.
async fn raw(pool: &MySqlPool, req: Request<Body>) -> (StatusCode, header::HeaderMap, Vec<u8>) {
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req)
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, headers, bytes.to_vec())
}

async fn send_json(pool: &MySqlPool, method: Method, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let (status, _, bytes) = raw(pool, req.body(Body::empty()).unwrap()).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Badan `multipart/form-data`: `(nama field, nama berkas, isi)`.
fn multipart_body(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, file, bytes) in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match file {
            Some(f) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
            ),
            None => body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes()),
        }
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn import_req(pool: &MySqlPool, token: Option<&str>, parts: &[(&str, Option<&str>, &[u8])]) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/spm-sanitasi/import")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"));
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let (status, _, bytes) = raw(pool, req.body(Body::from(multipart_body(parts))).unwrap()).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji SPM XL', ?, 'x', NOW(), NOW())")
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
    }
    auth::login::create_token(pool, uid, "uji-spm-xl").await.unwrap()
}

fn kec_name() -> &'static str {
    "uji-sm-xl-kec"
}

async fn cleanup(pool: &MySqlPool) {
    // Baris uji: nama berprefiks `uji-sm-xl-` (ekspor, impor), atau tertaut ke kecamatan uji.
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(s.id AS SIGNED) FROM tbl_spm_sanitasi s WHERE s.nama_infrastruktur LIKE 'uji-sm-xl-%' \
         UNION SELECT CAST(s.id AS SIGNED) FROM tbl_spm_sanitasi s JOIN tbl_desa d ON d.id = s.desa_id \
         JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?",
    )
    .bind(kec_name())
    .fetch_all(pool)
    .await
    .unwrap();
    // Audit dan notifikasi dihapus sebelum baris SPM, karena audit dicari lewat id SPM.
    for id in &ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        let notif: Vec<String> = sqlx::query_scalar("SELECT id FROM notifications WHERE data LIKE ?")
            .bind(format!("%Model SpmSanitasi dengan ID #{id} %"))
            .fetch_all(pool)
            .await
            .unwrap();
        for n in notif {
            sqlx::query("DELETE FROM notifications WHERE id = ?").bind(n).execute(pool).await.unwrap();
        }
    }
    for id in &ids {
        sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE d FROM tbl_desa d JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?")
        .bind(kec_name())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?").bind(kec_name()).execute(pool).await.unwrap();
}

/// Sel untuk membangun xlsx uji.
enum C {
    S(&'static str),
    N(f64),
    Owned(String),
}

/// Susun workbook dengan lembar `SPALDT`, `SPALDS`, `IPLT`. Setiap lembar: judul di A1, data dari baris 5.
fn workbook(spald_t: &[Vec<C>], spald_s: &[Vec<C>], iplt: &[Vec<C>]) -> Vec<u8> {
    let mut wb = Workbook::new();
    for (name, title, rows) in [
        ("SPALDT", "FORMAT DATA SPALDT", spald_t),
        ("SPALDS", "FORMAT DATA SPALDS", spald_s),
        ("IPLT", "FORMAT DATA IPLT", iplt),
    ] {
        let ws = wb.add_worksheet();
        ws.set_name(name).unwrap();
        ws.write_string(0, 0, title).unwrap();
        ws.write_string(3, 0, "No.").unwrap();
        for (i, row) in rows.iter().enumerate() {
            for (col, cell) in row.iter().enumerate() {
                let r = (4 + i) as u32;
                let c = col as u16;
                match cell {
                    C::S(s) => ws.write_string(r, c, *s).map(|_| ()).unwrap(),
                    C::Owned(s) => ws.write_string(r, c, s).map(|_| ()).unwrap(),
                    C::N(n) => ws.write_number(r, c, *n).map(|_| ()).unwrap(),
                }
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

/// Baris SPALDT 33 kolom (lihat `map_spald`): kolom 0 nomor, 4 kecamatan, 5 desa, 6 nama.
fn spald_t_row(no: f64, nama: &str, kk: f64, jiwa: f64, tahun: f64, total: f64) -> Vec<C> {
    let mut row: Vec<C> = Vec::new();
    row.push(C::N(no)); // 0
    row.push(C::S("Sedang")); // 1 skala pelayanan
    row.push(C::S("Jawa Barat")); // 2
    row.push(C::S("Cianjur")); // 3
    row.push(C::S("uji-sm-xl-kec")); // 4 kecamatan
    row.push(C::S("uji-sm-xl-desa-1")); // 5 desa
    row.push(C::Owned(nama.to_string())); // 6 nama
    row.push(C::N(-6.5)); // 7 lat
    row.push(C::N(107.1)); // 8 long
    row.push(C::S("-")); // 9 alamat: tanda "-" menjadi null
    row.push(C::N(kk)); // 10
    row.push(C::N(jiwa)); // 11
    row.push(C::N(tahun)); // 12
    row.push(C::N(100.0)); // 13 apbn
    row.push(C::N(0.0)); // 14 apbd
    row.push(C::N(0.0)); // 15 dak
    row.push(C::N(0.0)); // 16 hibah
    row.push(C::N(0.0)); // 17 csr
    row.push(C::N(0.0)); // 18 lain
    row.push(C::N(total)); // 19 total
    row.push(C::S("Berfungsi")); // 20 status
    row.push(C::S("Baik")); // 21 kualitas
    row.push(C::S("Pemda")); // 22 pengelola
    row
}

/// Baris SPALDS 31 kolom (tanpa kolom jiwa): tahun di kolom 11, total di 18.
fn spald_s_row(no: f64, nama: &str, kk: f64, tahun: f64, total: f64) -> Vec<C> {
    let mut row: Vec<C> = Vec::new();
    row.push(C::N(no));
    row.push(C::S("Sedang"));
    row.push(C::S("Jawa Barat"));
    row.push(C::S("Cianjur"));
    row.push(C::S("uji-sm-xl-kec"));
    row.push(C::S("uji-sm-xl-desa-1"));
    row.push(C::Owned(nama.to_string()));
    row.push(C::N(-6.6));
    row.push(C::N(107.2));
    row.push(C::S("Jl. Uji"));
    row.push(C::N(kk));
    row.push(C::N(tahun));
    row.push(C::N(50.0)); // 12 apbn
    for _ in 0..5 {
        row.push(C::N(0.0));
    }
    row.push(C::N(total)); // 18 total
    row
}

/// Baris IPLT: kecamatan 3, desa 4, nama 5, tahun 8, kk 21, total 29.
fn iplt_row(no: f64, nama: &str, tahun: f64, kk: f64, total: f64) -> Vec<C> {
    let mut row: Vec<C> = Vec::new();
    row.push(C::N(no));
    row.push(C::S("Jawa Barat"));
    row.push(C::S("Cianjur"));
    row.push(C::S("uji-sm-xl-kec"));
    row.push(C::S("uji-sm-xl-desa-1"));
    row.push(C::Owned(nama.to_string()));
    row.push(C::N(-6.7));
    row.push(C::N(107.3));
    row.push(C::N(tahun));
    row.push(C::S("Pemda"));
    for _ in 0..11 {
        row.push(C::N(1.0));
    }
    // indeks 21: jumlah pemanfaat KK, 22: tahun cadangan (kosong), 23..29: pembiayaan.
    while row.len() < 21 {
        row.push(C::S(""));
    }
    row.truncate(21);
    row.push(C::N(kk));
    row.push(C::S("")); // 22
    row.push(C::N(10.0)); // 23 apbn
    for _ in 0..5 {
        row.push(C::N(0.0));
    }
    row.push(C::N(total)); // 29
    row
}

fn sheet_value(bytes: &[u8], sheet: &str, row: u32, col: u32) -> Data {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).unwrap();
    let range = wb.worksheet_range(sheet).unwrap();
    range.get_value((row, col)).cloned().unwrap_or(Data::Empty)
}

fn as_f64(d: &Data) -> f64 {
    match d {
        Data::Float(f) => *f,
        Data::Int(i) => *i as f64,
        other => panic!("bukan angka: {other:?}"),
    }
}

fn as_text(d: &Data) -> String {
    match d {
        Data::String(s) => s.clone(),
        Data::Empty => String::new(),
        other => panic!("bukan teks: {other:?}"),
    }
}

/// Jumlah baris data (mulai baris 5) yang tidak kosong pada sebuah lembar.
fn data_rows(bytes: &[u8], sheet: &str) -> usize {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).unwrap();
    let range = wb.worksheet_range(sheet).unwrap();
    range.rows().skip(4).filter(|r| r.iter().any(|c| !matches!(c, Data::Empty))).count()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spm_excel_export_template_and_import_match_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let admin = user_token(&pool, "uji-spm-xl-admin@example.test", true).await;
    let biasa = user_token(&pool, "uji-spm-xl-biasa@example.test", false).await;

    // Kecamatan dan desa uji, serta satu baris SPALDT untuk diekspor.
    let kec = sqlx::query("INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())")
        .bind(kec_name())
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id() as i64;
    let desa1 = sqlx::query("INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES ('uji-sm-xl-desa-1', 900, 100, ?, NOW(), NOW())")
        .bind(kec)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id() as i64;
    sqlx::query("INSERT INTO tbl_spm_sanitasi (jenis, desa_id, nama_infrastruktur, jumlah_pemanfaat_kk, jumlah_pemanfaat_jiwa, tahun_konstruksi, pembiayaan_total, created_at, updated_at) VALUES ('spaldt', ?, 'uji-sm-xl-ekspor-1', 12, 60, 2020, 1000.5, NOW(), NOW())")
        .bind(desa1)
        .execute(&pool)
        .await
        .unwrap();

    // Ekspor per kecamatan: tiga lembar, judul di A1, header di baris 4, data mulai baris 5.
    let (st, headers, bytes) = raw(
        &pool,
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/spm-sanitasi/export?kecamatan_id={kec}"))
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment; filename=\"data_spm_sanitasi.xlsx\"");
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(as_text(&sheet_value(&bytes, "SPALDT", 0, 0)), "FORMAT DATA SPALDT");
    assert_eq!(as_text(&sheet_value(&bytes, "SPALDT", 3, 6)), "Nama Infrastruktur");
    assert_eq!(as_text(&sheet_value(&bytes, "SPALDT", 4, 4)), "uji-sm-xl-kec");
    assert_eq!(as_text(&sheet_value(&bytes, "SPALDT", 4, 5)), "uji-sm-xl-desa-1");
    assert_eq!(as_text(&sheet_value(&bytes, "SPALDT", 4, 6)), "uji-sm-xl-ekspor-1");
    assert_eq!(as_f64(&sheet_value(&bytes, "SPALDT", 4, 10)), 12.0, "kk");
    assert_eq!(as_f64(&sheet_value(&bytes, "SPALDT", 4, 11)), 60.0, "jiwa");
    assert_eq!(as_f64(&sheet_value(&bytes, "SPALDT", 4, 12)), 2020.0, "tahun");
    assert_eq!(as_f64(&sheet_value(&bytes, "SPALDT", 4, 19)), 1000.5, "total pembiayaan");
    assert_eq!(data_rows(&bytes, "SPALDS"), 0);
    assert_eq!(data_rows(&bytes, "IPLT"), 0);
    assert_eq!(as_text(&sheet_value(&bytes, "IPLT", 0, 0)), "FORMAT DATA IPLT");

    // Ekspor dengan pencarian: hanya baris yang cocok.
    let (st, _, bytes) = raw(
        &pool,
        Request::builder()
            .method(Method::GET)
            .uri("/api/spm-sanitasi/export?search=uji-sm-xl-ekspor&tahun=2020")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data_rows(&bytes, "SPALDT"), 1);

    // Template: semua baris, nama berkas template.
    let (st, headers, bytes) = raw(
        &pool,
        Request::builder()
            .method(Method::GET)
            .uri("/api/spm-sanitasi/import/template")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment; filename=\"template_spm_sanitasi.xlsx\"");
    assert!(data_rows(&bytes, "SPALDT") >= 1);

    // Impor: 2 baris SPALDT (1 dilewati karena nama kosong), 1 SPALDS, 1 IPLT, plus baris catatan.
    let spald_t = vec![
        spald_t_row(1.0, "uji-sm-xl-imp-spaldt-1", 15.0, 70.0, 2021.0, 500000.0),
        vec![C::S("(catatan)")],
        vec![],
        {
            let mut r = spald_t_row(3.0, "", 1.0, 1.0, 2021.0, 1.0);
            r[6] = C::S("");
            r
        },
    ];
    let spald_s = vec![spald_s_row(1.0, "uji-sm-xl-imp-spalds-1", 8.0, 2019.0, 300000.0)];
    let iplt = vec![iplt_row(1.0, "uji-sm-xl-imp-iplt-1", 2018.0, 22.0, 900000.0)];
    let xlsx = workbook(&spald_t, &spald_s, &iplt);

    let (st, body) = import_req(&pool, Some(&admin), &[("file", Some("impor.xlsx"), &xlsx), ("replace", None, b"0")]).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Data SPM Sanitasi berhasil diimport");
    assert_eq!(body["imported_rows"], 3, "{body}");
    assert_eq!(body["skipped_rows"], 1, "baris SPALDT tanpa nama dilewati");
    assert_eq!(body["errors"], json!([]));

    // Baris tersimpan: desa dari kecamatan dan desa, flag jiwa, tahun, dan audit created.
    let spaldt: (i64, Option<i64>, Option<i64>, Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT CAST(desa_id AS SIGNED), CAST(jumlah_pemanfaat_kk AS SIGNED), CAST(jumlah_pemanfaat_jiwa AS SIGNED), \
         CAST(tahun_konstruksi AS SIGNED), CAST(pembiayaan_total AS DOUBLE) FROM tbl_spm_sanitasi WHERE nama_infrastruktur = 'uji-sm-xl-imp-spaldt-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(spaldt.0, desa1);
    assert_eq!(spaldt.1, Some(15));
    assert_eq!(spaldt.2, Some(70));
    assert_eq!(spaldt.3, Some(2021));
    assert_eq!(spaldt.4, Some(500000.0));
    let spalds_jiwa: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(jumlah_pemanfaat_jiwa AS SIGNED) FROM tbl_spm_sanitasi WHERE nama_infrastruktur = 'uji-sm-xl-imp-spalds-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(spalds_jiwa, None, "lembar SPALDS tidak punya kolom jiwa");
    let iplt_kk: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(jumlah_pemanfaat_kk AS SIGNED) FROM tbl_spm_sanitasi WHERE nama_infrastruktur = 'uji-sm-xl-imp-iplt-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(iplt_kk, Some(22));
    let audits: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND event = 'created' \
         AND auditable_id IN (SELECT CAST(id AS SIGNED) FROM tbl_spm_sanitasi WHERE nama_infrastruktur LIKE 'uji-sm-xl-imp-%')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 3, "setiap baris impor diaudit seperti create()");

    // Ekspor setelah impor memuat baris impor, dan pencarian per nama.
    let (st, _, bytes) = raw(
        &pool,
        Request::builder()
            .method(Method::GET)
            .uri("/api/spm-sanitasi/export?search=uji-sm-xl-imp")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(data_rows(&bytes, "SPALDT"), 1);
    assert_eq!(data_rows(&bytes, "SPALDS"), 1);
    assert_eq!(data_rows(&bytes, "IPLT"), 1);

    // Validasi berkas dan opsi replace.
    let (st, body) = import_req(&pool, Some(&admin), &[("replace", None, b"0")]).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The file field is required.");
    let (st, body) = import_req(&pool, Some(&admin), &[("file", Some("impor.pdf"), b"%PDF")]).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The file field must be a file of type: xlsx, xls, csv.");
    let (st, body) = import_req(&pool, Some(&admin), &[("file", Some("impor.xlsx"), &xlsx), ("replace", None, b"mungkin")]).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The replace field must be true or false.");

    // Berkas rusak menghasilkan 500 dengan pesan impor. CSV tidak punya lembar bernama, jadi 0 baris.
    let (st, body) = import_req(&pool, Some(&admin), &[("file", Some("rusak.xlsx"), b"bukan xlsx")]).await;
    assert_eq!(st, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["success"], false);
    assert!(body["message"].as_str().unwrap().starts_with("Gagal mengimport data: "));
    let (st, body) = import_req(&pool, Some(&admin), &[("file", Some("data.csv"), b"No.,Nama\n1,uji-sm-xl-csv\n")]).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["imported_rows"], 0);
    assert_eq!(body["skipped_rows"], 0);

    // Non-admin diblokir oleh pemeriksaan rute.
    let (st, _) = import_req(&pool, Some(&biasa), &[("file", Some("impor.xlsx"), &xlsx)]).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send_json(&pool, Method::GET, "/api/spm-sanitasi/export", None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    cleanup(&pool).await;
    let left: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi WHERE nama_infrastruktur LIKE 'uji-sm-xl-%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
