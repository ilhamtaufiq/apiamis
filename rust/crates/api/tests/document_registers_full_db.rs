//! `GET /api/document-registers`: bentuk lengkap `kontrak` (dengan `pekerjaan`, `penyedia`), `type`, dan
//! `addendum` terhadap MySQL. Data uji memakai awalan `uji-drf-` dan hanya baris itu yang dihapus.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test document_registers_full_db -- --include-ignored
//! ```

use std::collections::BTreeSet;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-drf-admin@example.test";
const CODE: &str = "UJI-DRF";
const PREFIX: &str = "uji-drf-";
const YEAR: i64 = 2098;

/// Kolom `SELECT *` tabel `tbl_kontrak` (urutan tabel), plus relasi `pekerjaan` dan `penyedia`.
/// Tidak ada `pekerjaans`: Laravel tidak memuatnya di indeks ini.
const KONTRAK_KEYS: &[&str] = &[
    "id", "id_kegiatan", "id_pekerjaan", "id_penyedia", "kode_rup", "kode_paket", "nomor_penawaran",
    "tanggal_penawaran", "nilai_kontrak", "tgl_sppbj", "tgl_spk", "tgl_spmk", "tgl_selesai", "sppbj", "spk",
    "spmk", "spse_sppbj_id", "spse_spk_id", "spse_rekanan_id", "spse_pushed_at", "spse_push_log", "created_at",
    "updated_at", "pekerjaan", "penyedia",
];
const PEKERJAAN_KEYS: &[&str] = &[
    "id", "kode_rekening", "nama_paket", "kecamatan_id", "desa_id", "kegiatan_id", "pagu", "is_konsultan",
    "status", "catatan", "created_at", "updated_at", "pengawas_id", "pendamping_id",
];
const PENYEDIA_KEYS: &[&str] = &[
    "id", "nama", "direktur", "no_akta", "notaris", "tanggal_akta", "alamat", "npwp", "bank", "norek",
    "created_at", "updated_at",
];
const ADDENDUM_KEYS: &[&str] = &[
    "id", "kontrak_id", "addendum_ke", "nomor_addendum", "attachment_nomors", "tanggal_addendum",
    "jenis_addendum", "alasan", "deskripsi_perubahan", "nilai_kontrak_sebelum", "nilai_kontrak_sesudah",
    "tgl_selesai_sebelum", "tgl_selesai_sesudah", "status", "kelengkapan_override", "created_by",
    "approved_by", "approved_at", "created_at", "updated_at",
];
const TYPE_KEYS: &[&str] = &["id", "name", "code", "format_template", "created_at", "updated_at"];
const REGISTER_KEYS: &[&str] = &[
    "id", "kontrak_id", "type_id", "addendum_id", "attachment_type", "nomor", "tanggal", "sequence_number",
    "year", "description", "nilai", "created_at", "updated_at", "kontrak", "type", "addendum",
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

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req)
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn admin_token(pool: &MySqlPool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(ADMIN).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji DRF', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-drf").await.unwrap()
}

/// Hapus hanya baris berawalan `uji-drf-` dan tipe `UJI-DRF`, urut dari yang bergantung.
async fn cleanup(pool: &MySqlPool) {
    let like = format!("{PREFIX}%");
    sqlx::query("DELETE FROM tbl_document_registers WHERE nomor LIKE ? OR year = ?")
        .bind(&like)
        .bind(YEAR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(YEAR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM tbl_kontrak_addendums WHERE nomor_addendum LIKE ? \
         OR kontrak_id IN (SELECT id FROM tbl_kontrak WHERE kode_paket LIKE ?)",
    )
    .bind(&like)
    .bind(&like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_kontrak WHERE kode_paket LIKE ?").bind(&like).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE ?").bind(&like).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama LIKE ?").bind(&like).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_document_types WHERE code = ?").bind(CODE).execute(pool).await.unwrap();
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().expect("objek").keys().cloned().collect()
}

fn assert_keys(v: &Value, expected: &[&str], what: &str) {
    let want: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(expected.len(), want.len(), "{what}: kunci ganda di daftar uji");
    assert_eq!(keys(v), want, "{what}: kunci tidak sama dengan model Laravel");
}

/// Cast `date` Laravel: `YYYY-MM-DDT00:00:00.000000Z`.
fn assert_date(v: &Value, expected_day: &str, what: &str) {
    assert_eq!(v, &json!(format!("{expected_day}T00:00:00.000000Z")), "{what}");
}

fn assert_carbon_ts(v: &Value, what: &str) {
    let s = v.as_str().unwrap_or_else(|| panic!("{what} harus string, dapat {v}"));
    // Carbon `toJSON()` dalam UTC: `YYYY-MM-DDTHH:MM:SS.000000Z`.
    assert_eq!(s.len(), 27, "{what}: {s}");
    assert!(s.ends_with(".000000Z"), "{what}: {s}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_register_index_full_model_shape() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let token = admin_token(&pool).await;

    // Penyedia, paket, dan kontrak dengan kedua relasi.
    let penyedia = sqlx::query(
        "INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, tanggal_akta, alamat, npwp, bank, norek, \
         created_at, updated_at) VALUES (?, 'Direktur Uji', 'AKTA-UJI-1', 'Notaris Uji', '2020-02-03', \
         'Jl. Uji 1', NULL, 'BRI', '123456', NOW(), NOW())",
    )
    .bind(format!("{PREFIX}penyedia"))
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    let pekerjaan = sqlx::query(
        "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, \
         is_konsultan, status, catatan, created_at, updated_at, pengawas_id, pendamping_id) \
         VALUES ('uji-drf-rek', ?, NULL, NULL, NULL, 1500000.5, 0, 'berjalan', 'catatan uji', NOW(), NOW(), NULL, NULL)",
    )
    .bind(format!("{PREFIX}paket"))
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    let kontrak = sqlx::query(
        "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, kode_rup, kode_paket, nomor_penawaran, \
         tanggal_penawaran, nilai_kontrak, tgl_sppbj, tgl_spk, tgl_spmk, tgl_selesai, sppbj, spk, spmk, \
         spse_sppbj_id, spse_spk_id, spse_rekanan_id, spse_pushed_at, spse_push_log, created_at, updated_at) \
         VALUES (NULL, ?, ?, 'RUP-UJI', 'uji-drf-kode', 'PEN-UJI', '2099-01-02', 1500000.5, '2099-01-03', \
         '2099-01-04', NULL, '2099-12-31', 'SPPBJ-UJI', 'SPK-UJI', NULL, NULL, NULL, NULL, NULL, '[\"uji\"]', \
         NOW(), NOW())",
    )
    .bind(pekerjaan)
    .bind(penyedia)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    // Kontrak kedua tanpa pekerjaan dan penyedia.
    let kontrak_kosong = sqlx::query("INSERT INTO tbl_kontrak (kode_paket, created_at, updated_at) VALUES ('uji-drf-kosong', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id() as i64;

    sqlx::query(
        "INSERT INTO tbl_kontrak_addendums (kontrak_id, addendum_ke, nomor_addendum, attachment_nomors, \
         tanggal_addendum, jenis_addendum, alasan, deskripsi_perubahan, nilai_kontrak_sebelum, \
         nilai_kontrak_sesudah, tgl_selesai_sebelum, tgl_selesai_sesudah, status, kelengkapan_override, \
         created_by, approved_by, approved_at, created_at, updated_at) \
         VALUES (?, 1, ?, '[\"uji-drf-att\"]', '2099-02-01', 'biaya', 'alasan uji', 'deskripsi uji', \
         1500000.5, 1600000, '2099-12-31', '2100-01-31', 'disetujui', 1, NULL, NULL, NULL, NOW(), NOW())",
    )
    .bind(kontrak)
    .bind(format!("{PREFIX}addendum-1"))
    .execute(&pool)
    .await
    .unwrap();
    let addendum: i64 = sqlx::query_scalar("SELECT CAST(MAX(id) AS SIGNED) FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
        .bind(kontrak)
        .fetch_one(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO tbl_document_types (name, code, format_template, created_at, updated_at) VALUES ('Uji DRF', ?, NULL, NOW(), NOW())")
        .bind(CODE)
        .execute(&pool)
        .await
        .unwrap();
    let type_id: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_document_types WHERE code = ?")
        .bind(CODE)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Register 1: dengan addendum. Register 2: tanpa addendum, kontrak tanpa relasi.
    sqlx::query(
        "INSERT INTO tbl_document_registers (kontrak_id, type_id, addendum_id, attachment_type, nomor, tanggal, \
         sequence_number, year, description, nilai, created_at, updated_at) \
         VALUES (?, ?, ?, 'uji', ?, '2099-03-05', 1, ?, 'deskripsi uji', 2500.75, NOW(), NOW())",
    )
    .bind(kontrak)
    .bind(type_id)
    .bind(addendum)
    .bind(format!("{PREFIX}001"))
    .bind(YEAR)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tbl_document_registers (kontrak_id, type_id, addendum_id, attachment_type, nomor, tanggal, \
         sequence_number, year, description, nilai, created_at, updated_at) \
         VALUES (?, ?, NULL, NULL, ?, '2099-03-06', 2, ?, NULL, NULL, NOW(), NOW())",
    )
    .bind(kontrak_kosong)
    .bind(type_id)
    .bind(format!("{PREFIX}002"))
    .bind(YEAR)
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = get(&pool, "/api/document-registers?search=uji-drf-&per_page=50", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = body["data"].as_array().expect("data");
    let find = |nomor: &str| data.iter().find(|r| r["nomor"] == nomor).cloned().unwrap_or_else(|| panic!("{nomor} tidak ada: {body}"));

    // Register 1: kunci dan nilai lengkap.
    let r1 = find(&format!("{PREFIX}001"));
    assert_keys(&r1, REGISTER_KEYS, "register");
    assert_eq!(r1["nilai"].as_f64(), Some(2500.75));
    assert_date(&r1["tanggal"], "2099-03-05", "register.tanggal");

    let k = &r1["kontrak"];
    assert_keys(k, KONTRAK_KEYS, "kontrak");
    assert_eq!(k["id"], json!(kontrak));
    assert_eq!(k["id_pekerjaan"], json!(pekerjaan));
    assert_eq!(k["id_penyedia"], json!(penyedia));
    assert_eq!(k["nilai_kontrak"].as_f64(), Some(1500000.5));
    assert_date(&k["tgl_spk"], "2099-01-04", "kontrak.tgl_spk");
    assert_date(&k["tanggal_penawaran"], "2099-01-02", "kontrak.tanggal_penawaran");
    assert_eq!(k["tgl_spmk"], Value::Null);
    assert_eq!(k["spse_pushed_at"], Value::Null);
    assert_eq!(k["spse_push_log"], json!(["uji"]), "array cast dari JSON");
    assert_carbon_ts(&k["created_at"], "kontrak.created_at");
    assert!(k.get("pekerjaans").is_none(), "pekerjaans tidak dimuat Laravel");

    let p = &k["pekerjaan"];
    assert_keys(p, PEKERJAAN_KEYS, "kontrak.pekerjaan");
    assert_eq!(p["id"], json!(pekerjaan));
    assert_eq!(p["nama_paket"], json!(format!("{PREFIX}paket")));
    assert_eq!(p["pagu"].as_f64(), Some(1500000.5));
    assert_eq!(p["is_konsultan"], json!(false), "cast boolean");
    assert_eq!(p["catatan"], json!("catatan uji"));
    assert_eq!(p["pengawas_id"], Value::Null);
    assert_carbon_ts(&p["created_at"], "kontrak.pekerjaan.created_at");

    let y = &k["penyedia"];
    assert_keys(y, PENYEDIA_KEYS, "kontrak.penyedia");
    assert_eq!(y["nama"], json!(format!("{PREFIX}penyedia")));
    assert_date(&y["tanggal_akta"], "2020-02-03", "kontrak.penyedia.tanggal_akta");
    assert_eq!(y["npwp"], Value::Null);
    assert_eq!(y["norek"], json!("123456"));

    let t = &r1["type"];
    assert_keys(t, TYPE_KEYS, "type");
    assert_eq!(t["code"], json!(CODE));

    let a = &r1["addendum"];
    assert_keys(a, ADDENDUM_KEYS, "addendum");
    assert_eq!(a["id"], json!(addendum));
    assert_eq!(a["addendum_ke"], json!(1));
    assert_eq!(a["attachment_nomors"], json!(["uji-drf-att"]), "array cast dari JSON");
    assert_date(&a["tanggal_addendum"], "2099-02-01", "addendum.tanggal_addendum");
    assert_date(&a["tgl_selesai_sesudah"], "2100-01-31", "addendum.tgl_selesai_sesudah");
    assert_eq!(a["kelengkapan_override"], json!(true), "cast boolean");
    assert_eq!(a["nilai_kontrak_sebelum"].as_f64(), Some(1500000.5));
    assert_eq!(a["nilai_kontrak_sesudah"].as_f64(), Some(1600000.0));
    assert_eq!(a["jenis_addendum"], json!("biaya"));
    assert_eq!(a["status"], json!("disetujui"));
    assert_eq!(a["approved_at"], Value::Null);
    assert_carbon_ts(&a["created_at"], "addendum.created_at");

    // Register 2: relasi kosong tetap berupa kunci dengan null.
    let r2 = find(&format!("{PREFIX}002"));
    assert_keys(&r2, REGISTER_KEYS, "register kedua");
    assert_eq!(r2["addendum"], Value::Null);
    assert_eq!(r2["nilai"], Value::Null);
    let k2 = &r2["kontrak"];
    assert_keys(k2, KONTRAK_KEYS, "kontrak kosong");
    assert_eq!(k2["pekerjaan"], Value::Null);
    assert_eq!(k2["penyedia"], Value::Null);
    assert_eq!(k2["spse_push_log"], Value::Null);

    cleanup(&pool).await;
}
