//! `kontrak-addendums` dan rute addendum di bawah `kontrak/{id}` lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kontrak_addendum_db -- --include-ignored
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

const ADMIN: &str = "uji-addendum-admin@example.test";
const SIN_ROLE: &str = "uji-addendum-norole@example.test";
const PENYEDIA: &str = "UJI-ADD penyedia";
const KODE: &str = "UJI-ADD-KONTRAK";
const TAHUN: i32 = 2099;
const BOUNDARY: &str = "----ujiaddendumboundary";

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
    let storage = std::env::temp_dir().join(format!("uji-addendum-{}", std::process::id()));
    std::env::set_var("PUBLIC_STORAGE_PATH", &storage);
}

enum Payload {
    None,
    Json(Value),
    Multipart(Vec<u8>),
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    payload: Payload,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match payload {
        Payload::None => Body::empty(),
        Payload::Json(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        Payload::Multipart(bytes) => {
            req = req.header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            );
            Body::from(bytes)
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

/// Body multipart dengan field teks dan satu berkas `file`.
fn multipart(fields: &[(&str, &str)], file: (&str, &[u8])) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            file.0
        )
        .as_bytes(),
    );
    body.extend_from_slice(file.1);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn make_user(pool: &MySqlPool, email: &str, admin: bool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Addendum', ?, 'x', NOW(), NOW())")
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

/// Bersihkan sisa uji: addendum (item, media, register, audit), kontrak, penyedia, dan sekuens tahun uji.
async fn cleanup(pool: &MySqlPool) {
    let kontrak_ids: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM tbl_kontrak WHERE kode_paket = ?")
            .bind(KODE)
            .fetch_all(pool)
            .await
            .unwrap();
    for kid in kontrak_ids {
        let add_ids: Vec<u64> =
            sqlx::query_scalar("SELECT id FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
                .bind(kid)
                .fetch_all(pool)
                .await
                .unwrap();
        for aid in add_ids {
            sqlx::query("DELETE FROM media WHERE model_type = 'App\\\\Models\\\\KontrakAddendum' AND model_id = ?").bind(aid).execute(pool).await.unwrap();
            sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KontrakAddendum' AND auditable_id = ?").bind(aid).execute(pool).await.unwrap();
            sqlx::query("DELETE FROM tbl_kontrak_addendum_items WHERE addendum_id = ?")
                .bind(aid)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM tbl_document_registers WHERE addendum_id = ?")
                .bind(aid)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE id = ?")
                .bind(aid)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Kontrak' AND auditable_id = ?").bind(kid).execute(pool).await.unwrap();
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
    sqlx::query("DELETE FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'")
        .bind(TAHUN)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn addendum_flow_numbers_registers_attachments_and_access() {
    use_storage();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    // Tipe dokumen ADD: dibuat hanya bila belum ada, dan dihapus lagi di akhir.
    let add_type_existing: Option<u64> = sqlx::query_scalar("SELECT id FROM tbl_document_types WHERE LOWER(code) IN ('add','addendum') ORDER BY id LIMIT 1")
        .fetch_optional(&pool)
        .await
        .unwrap();
    let add_type = match add_type_existing {
        Some(id) => id,
        None => {
            sqlx::query("INSERT INTO tbl_document_types (name, code, created_at, updated_at) VALUES ('Addendum uji', 'ADD', NOW(), NOW())")
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

    // Kontrak uji: spk berpola DISPERKIM-AMS.098 agar prefix lampiran bisa diuji.
    sqlx::query("INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) VALUES (?, 'Direktur Uji', '0', 'Notaris', 'Alamat', NOW(), NOW())")
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
    sqlx::query(
        "INSERT INTO tbl_kontrak (id_pekerjaan, id_penyedia, kode_paket, spk, nilai_kontrak, tgl_spk, created_at, updated_at) \
         VALUES (?, ?, ?, 'DISPERKIM-AMS.098.390/2099', 1000000, '2099-01-10', NOW(), NOW())",
    )
    .bind(pekerjaan)
    .bind(penyedia)
    .bind(KODE)
    .execute(&pool)
    .await
    .unwrap();
    let kontrak: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_kontrak WHERE kode_paket = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(KODE)
    .fetch_one(&pool)
    .await
    .unwrap();

    let admin = make_user(&pool, ADMIN, true).await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-addendum")
        .await
        .unwrap();
    let norole = make_user(&pool, SIN_ROLE, false).await;
    let norole_token = auth::login::create_token(&pool, norole, "uji-addendum")
        .await
        .unwrap();

    // Nomor: pratinjau tiga nomor, prefix SPK 098, dan sekuens dimulai dari 1.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak/{kontrak}/addendum-numbers"),
        Some(&admin_token),
        Payload::Json(json!({ "tanggal": "2099-03-01", "count": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let numbers = body["numbers"].as_array().unwrap().clone();
    assert_eq!(numbers.len(), 3, "{body}");
    assert!(
        numbers[0].as_str().unwrap().ends_with("/III/2099"),
        "{body}"
    );
    assert_eq!(
        numbers[1],
        json!(format!("{pekerjaan}.002/098/AMS/{TAHUN}")),
        "{body}"
    );
    assert_eq!(
        numbers[2],
        json!(format!("{pekerjaan}.003/098/AMS/{TAHUN}")),
        "{body}"
    );
    let nomor1 = numbers[0].as_str().unwrap().to_string();

    // Create draft dengan item dan nomor lampiran.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak/{kontrak}/addendums"),
        Some(&admin_token),
        Payload::Json(json!({
            "addendum_ke": 1,
            "nomor_addendum": nomor1,
            "tanggal_addendum": "2099-03-01",
            "jenis_addendum": "biaya",
            "nilai_kontrak_sebelum": "1000000",
            "nilai_kontrak_sesudah": 1200000,
            "items": [{ "nama_item": "Item uji", "volume_sebelum": 1, "volume_sesudah": 2, "harga_sebelum": 1000, "harga_sesudah": 1200, "subtotal_sebelum": 1000, "subtotal_sesudah": 2400 }],
            "attachment_nomor": { "cco": "CCO-UJI-1" },
            "attachment_tanggal": { "cco": "2099-03-02" }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let add1 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(body["data"]["status"], "draft", "{body}");
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(
        body["data"]["attachment_nomors"]["cco"]["nomor"], "CCO-UJI-1",
        "{body}"
    );
    assert_eq!(body["data"]["can_submit"], true, "{body}");
    assert!(
        body["data"].get("creator").is_none(),
        "store tidak memuat creator: {body}"
    );

    let audit: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KontrakAddendum' AND auditable_id = ? ORDER BY id")
        .bind(add1)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(audit, vec!["created"]);

    // Addendum ke yang sama ditolak.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak/{kontrak}/addendums"),
        Some(&admin_token),
        Payload::Json(json!({ "addendum_ke": 1, "tanggal_addendum": "2099-03-05", "jenis_addendum": "waktu" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["addendum_ke"].is_array(), "{body}");

    // Index dan detail memuat kontrak dan pembuat.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak/{kontrak}/addendums"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 1, "{body}");
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak-addendums/{add1}"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["kontrak"]["id"], kontrak, "{body}");
    assert_eq!(body["data"]["creator"]["name"], "Uji Addendum", "{body}");

    // Update: tanggal berubah dan tercatat di audit.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kontrak-addendums/{add1}"),
        Some(&admin_token),
        Payload::Json(json!({ "addendum_ke": 1, "nomor_addendum": nomor1, "tanggal_addendum": "2099-03-05", "jenis_addendum": "waktu" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["tanggal_addendum"], "2099-03-05", "{body}");
    let events: Vec<String> = sqlx::query_scalar("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KontrakAddendum' AND auditable_id = ? ORDER BY id")
        .bind(add1)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, vec!["created", "updated"]);

    // Alur: submit, lalu submit lagi ditolak, lalu process, lalu approved tidak bisa diubah.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/submit"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "diajukan", "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/submit"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Addendum hanya bisa diajukan dari status draft atau ditolak"
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/process"),
        Some(&admin_token),
        Payload::Json(json!({
            "nomor_addendum": nomor1,
            "dokumen": [{ "type": "cco", "nomor": "CCO-UJI-2", "tanggal": "2099-03-03" }, { "type": "cco_kosong", "nomor": null }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "disetujui", "{body}");
    assert_eq!(body["data"]["approved_by"], admin, "{body}");
    let reg: (u64, String, String) = sqlx::query_as("SELECT addendum_id, attachment_type, nomor FROM tbl_document_registers WHERE addendum_id = ?")
        .bind(add1)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        (reg.0, reg.1.as_str(), reg.2.as_str()),
        (add1, "cco", "CCO-UJI-2")
    );
    let type_id: u64 =
        sqlx::query_scalar("SELECT type_id FROM tbl_document_registers WHERE addendum_id = ?")
            .bind(add1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(type_id, add_type);

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kontrak-addendums/{add1}"),
        Some(&admin_token),
        Payload::Json(json!({ "addendum_ke": 1, "tanggal_addendum": "2099-03-05", "jenis_addendum": "waktu" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Addendum yang sudah disetujui tidak bisa diubah"
    );
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/kontrak-addendums/{add1}/attachment-numbers"),
        Some(&admin_token),
        Payload::Json(json!({ "numbers": { "cco": { "nomor": "X" } } })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/override-kelengkapan"),
        Some(&admin_token),
        Payload::Json(json!({ "kelengkapan_override": true })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Upload lampiran pada addendum yang sudah disetujui tetap diizinkan (arsip).
    let pdf = multipart(
        &[
            ("type", "cco"),
            ("nomor", "CCO-UJI-3"),
            ("tanggal", "2099-03-04"),
        ],
        ("cco.pdf", b"%PDF-1.4 uji"),
    );
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/upload"),
        Some(&admin_token),
        Payload::Multipart(pdf),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let attachments = body["data"]["attachments"].as_array().unwrap();
    assert_eq!(attachments.len(), 1, "{body}");
    assert_eq!(attachments[0]["document_type"], "cco", "{body}");
    assert_eq!(attachments[0]["nomor"], "CCO-UJI-3", "{body}");
    assert_eq!(attachments[0]["label"], "CCO", "{body}");

    // Jenis tidak dikenal ditolak; lampiran tanpa berkas juga.
    let bad = multipart(&[("type", "tidak_ada")], ("x.pdf", b"%PDF"));
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add1}/upload"),
        Some(&admin_token),
        Payload::Multipart(bad),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Jenis lampiran tidak valid");

    // Addendum kedua: submit, tolak, submit lagi, lalu hapus.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak/{kontrak}/addendums"),
        Some(&admin_token),
        Payload::Json(json!({ "addendum_ke": 2, "tanggal_addendum": "2099-04-01", "jenis_addendum": "lainnya" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let add2 = body["data"]["id"].as_u64().unwrap();
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add2}/submit"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add2}/reject"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "ditolak", "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak-addendums/{add2}/submit"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "ditolak boleh diajukan ulang: {body}"
    );
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kontrak-addendums/{add2}"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Addendum kontrak berhasil dihapus");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/kontrak-addendums/{add2}"),
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Daftar admin: filter status dan pencarian.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/kontrak-addendums?status=disetujui&search=ADD-UJI",
        Some(&admin_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["per_page"], 20, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "disetujui"),
        "{body}"
    );

    // Tanpa role: tidak boleh melihat daftar admin, dan tidak boleh membuat pengajuan.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/kontrak-addendums",
        Some(&norole_token),
        Payload::None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Hanya admin yang boleh melakukan aksi ini");
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/kontrak/{kontrak}/addendums"),
        Some(&norole_token),
        Payload::Json(json!({ "addendum_ke": 9, "tanggal_addendum": "2099-04-01", "jenis_addendum": "lainnya" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body["message"],
        "Hanya admin atau Pengawas yang boleh membuat pengajuan addendum"
    );

    cleanup(&pool).await;
    if add_type_existing.is_none() {
        sqlx::query("DELETE FROM tbl_document_types WHERE id = ?")
            .bind(add_type)
            .execute(&pool)
            .await
            .unwrap();
    }
}
