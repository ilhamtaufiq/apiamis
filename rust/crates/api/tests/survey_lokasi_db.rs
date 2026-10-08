//! Survei lokasi lewat router terhadap MySQL (JSON, multipart, tugas, audit, dan foto).
//!
//! Setiap tes memakai awalan nama sendiri (`uji-sl-<tes>`) dan email sendiri, sehingga tes yang
//! berjalan paralel tidak saling menghapus baris.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test survey_lokasi_db -- --include-ignored
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

const BOUNDARY: &str = "ujiSurveyLokasiBoundary";
const MODEL: &str = "App\\Models\\SurveyLokasi";
const TUGAS_MODEL: &str = "App\\Models\\SurveyTugas";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Satu bagian multipart: teks atau berkas.
enum Part<'a> {
    Text(&'a str, &'a str),
    File(&'a str, &'a str, &'a [u8]),
}

fn multipart(parts: &[Part]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            Part::Text(name, value) => out.extend_from_slice(
                format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
            ),
            Part::File(name, filename, bytes) => {
                out.extend_from_slice(
                    format!(
                        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(bytes);
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Body,
    content_type: Option<String>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
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

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    send(pool, Method::GET, uri, Some(token), Body::empty(), None).await
}

async fn post_json(pool: &MySqlPool, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        uri,
        Some(token),
        Body::from(body.to_string()),
        Some("application/json".into()),
    )
    .await
}

async fn put_json(pool: &MySqlPool, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::PUT,
        uri,
        Some(token),
        Body::from(body.to_string()),
        Some("application/json".into()),
    )
    .await
}

async fn post_multipart(
    pool: &MySqlPool,
    uri: &str,
    token: &str,
    parts: &[Part<'_>],
) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        uri,
        Some(token),
        Body::from(multipart(parts)),
        Some(format!("multipart/form-data; boundary={BOUNDARY}")),
    )
    .await
}

async fn delete(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    send(pool, Method::DELETE, uri, Some(token), Body::empty(), None).await
}

/// User dengan role tertentu (`roles` boleh kosong). Mengembalikan `(id, token)`.
async fn user(pool: &MySqlPool, email: &str, roles: &[&str]) -> (u64, String) {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Survei', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    for role in roles {
        sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
            .bind(role)
            .execute(pool)
            .await
            .unwrap();
        let rid: u64 = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1",
        )
        .bind(role)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
            .bind(rid)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    let token = auth::login::create_token(pool, uid, "uji-sl")
        .await
        .unwrap();
    (uid, token)
}

async fn kecamatan(pool: &MySqlPool, nama: &str) -> u64 {
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(nama)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(nama)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec = ? LIMIT 1")
        .bind(nama)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus baris milik awalan `p` saja: media, audit, survei, tugas, kecamatan, dan user.
/// Akar penyimpanan media, sama dengan `media::storage_root` (env `PUBLIC_STORAGE_PATH`, atau default repo).
fn storage_root() -> std::path::PathBuf {
    std::env::var_os("PUBLIC_STORAGE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/public"
            ))
        })
}

async fn cleanup(pool: &MySqlPool, p: &str) {
    let like = format!("{p}%");
    let user_like = format!("{p}%@example.test");
    let survey_ids = "SELECT id FROM tbl_survey_lokasi WHERE nama_lokasi LIKE ?";
    let tugas_ids = "SELECT id FROM tbl_survey_tugas WHERE judul LIKE ?";
    let media_ids: Vec<i64> = sqlx::query_scalar(&format!(
        "SELECT CAST(id AS SIGNED) FROM media WHERE model_type = ? AND model_id IN ({survey_ids})"
    ))
    .bind(MODEL)
    .bind(&like)
    .fetch_all(pool)
    .await
    .unwrap();
    for id in media_ids {
        let _ = std::fs::remove_dir_all(storage_root().join(id.to_string()));
    }
    sqlx::query(&format!(
        "DELETE FROM media WHERE model_type = ? AND model_id IN ({survey_ids})"
    ))
    .bind(MODEL)
    .bind(&like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id IN ({survey_ids})"
    ))
    .bind(MODEL)
    .bind(&like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id IN ({tugas_ids})"
    ))
    .bind(TUGAS_MODEL)
    .bind(&like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_survey_lokasi WHERE nama_lokasi LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_survey_tugas WHERE judul LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email LIKE ?)",
    )
    .bind(&user_like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE ?")
        .bind(&user_like)
        .execute(pool)
        .await
        .unwrap();
}

/// Survei langsung lewat SQL (untuk menyiapkan data pada tes statistik dan indeks).
async fn insert_survey(
    pool: &MySqlPool,
    owner: u64,
    nama: &str,
    alamat: &str,
    jenis: &str,
    status: &str,
) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_survey_lokasi (user_id, jenis, nama_lokasi, alamat, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(owner)
    .bind(jenis)
    .bind(nama)
    .bind(alamat)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_survey_lokasi WHERE nama_lokasi = ? ORDER BY id DESC LIMIT 1")
        .bind(nama)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit_count(pool: &MySqlPool, model: &str, id: i64, event: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = ?",
    )
    .bind(model)
    .bind(id)
    .bind(event)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Permintaan simpan standar dari sebuah tes.
fn survey_body(nama: &str, tugas_id: Option<i64>) -> Value {
    let mut v = json!({
        "jenis": "spam_perpipaan",
        "nama_lokasi": nama,
        "alamat": "Jl. uji 1",
        "latitude": -6.2,
        "longitude": 106.8,
        "detail": { "sumber_air": "Mata air", "ph": 7.2, "menara_tinggi": 3, "dok_bnba": true },
    });
    if let Some(t) = tugas_id {
        v["tugas_id"] = json!(t);
    }
    v
}

// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn stats_and_index_follow_filters_and_paginate() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-idx";
    cleanup(&pool, p).await;
    let (uid, token) = user(&pool, "uji-sl-idx-user@example.test", &[]).await;

    insert_survey(
        &pool,
        uid,
        &format!("{p}-a"),
        "uji-sl-idx-alamat",
        "spam_perpipaan",
        "diajukan",
    )
    .await;
    insert_survey(
        &pool,
        uid,
        &format!("{p}-b"),
        "uji-sl-idx-alamat",
        "mck_individu",
        "diverifikasi",
    )
    .await;
    insert_survey(
        &pool,
        uid,
        &format!("{p}-c"),
        "uji-sl-idx-alamat",
        "spam_perpipaan",
        "ditolak",
    )
    .await;

    let (status, body) = get(&pool, "/api/survey-lokasi?search=uji-sl-idx-alamat", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 3);
    assert_eq!(body["meta"]["per_page"], 15);
    // Terbaru dulu: baris yang disisipkan terakhir ada di depan.
    assert_eq!(body["data"][0]["nama_lokasi"], format!("{p}-c"));
    assert_eq!(body["data"][0]["jenis_label"], "SPAM Perpipaan");

    let (_, body) = get(
        &pool,
        "/api/survey-lokasi?search=uji-sl-idx-alamat&status=ditolak",
        &token,
    )
    .await;
    assert_eq!(body["meta"]["total"], 1);
    let (_, body) = get(
        &pool,
        "/api/survey-lokasi?search=uji-sl-idx-alamat&jenis=mck_individu",
        &token,
    )
    .await;
    assert_eq!(body["data"][0]["nama_lokasi"], format!("{p}-b"));

    let (_, body) = get(
        &pool,
        "/api/survey-lokasi?search=uji-sl-idx-alamat&per_page=2&page=2",
        &token,
    )
    .await;
    assert_eq!(body["meta"]["last_page"], 2);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    let (_, body) = get(
        &pool,
        "/api/survey-lokasi?search=uji-sl-idx-alamat&per_page=500",
        &token,
    )
    .await;
    assert_eq!(body["meta"]["per_page"], 100, "per_page dibatasi 100");

    // Statistik: dibandingkan dengan hitungan SQL pada saat yang sama.
    let (status, body) = get(&pool, "/api/survey-lokasi/stats", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let db_spam: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi WHERE jenis = 'spam_perpipaan'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(body["by_jenis"]["spam_perpipaan"], db_spam);
    let db_total: i64 =
        sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_lokasi")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(body["total"].as_i64().unwrap(), db_total);
    assert!(
        body["by_jenis"].get("mck_komunal").is_some(),
        "kunci jenis selalu ada"
    );

    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/survey-lokasi",
        None,
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn show_returns_resource_and_404_for_missing() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-show";
    cleanup(&pool, p).await;
    let (uid, token) = user(&pool, "uji-sl-show-user@example.test", &[]).await;
    let id = insert_survey(
        &pool,
        uid,
        &format!("{p}-a"),
        "alamat",
        "mck_komunal",
        "diajukan",
    )
    .await;

    let (status, body) = get(&pool, &format!("/api/survey-lokasi/{id}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let d = &body["data"];
    assert_eq!(d["jenis_label"], "MCK Komunal");
    assert_eq!(d["surveyor"]["id"], uid);
    assert_eq!(d["tugas"], Value::Null);
    assert_eq!(d["foto"], json!([]));
    assert_eq!(d["detail"], Value::Null);

    let (status, body) = get(&pool, "/api/survey-lokasi/abc", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "Not Found.");
    let (status, _) = get(&pool, "/api/survey-lokasi/999999999", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_checks_role_validation_and_writes_audit() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-store";
    cleanup(&pool, p).await;
    let kec = kecamatan(&pool, &format!("{p}-kecamatan")).await;
    let (_, plain) = user(&pool, "uji-sl-store-plain@example.test", &[]).await;
    let (tfl_id, tfl) = user(&pool, "uji-sl-store-tfl@example.test", &["tfl"]).await;
    let uri = "/api/survey-lokasi";

    let (status, body) = post_json(&pool, uri, &plain, survey_body(&format!("{p}-x"), None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .starts_with("Forbidden. Hanya admin"),
        "{body}"
    );

    let mut bad = survey_body(&format!("{p}-x"), None);
    bad["jenis"] = json!("spam");
    let (status, body) = post_json(&pool, uri, &tfl, bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "Validation error");
    assert_eq!(body["errors"]["jenis"][0], "The selected jenis is invalid.");

    let mut bad = survey_body(&format!("{p}-x"), None);
    bad["detail"] = json!({ "ph": 15, "kebutuhan": "lain" });
    bad["kecamatan_id"] = json!(999_999_999);
    bad["latitude"] = json!(91);
    let (status, body) = post_json(&pool, uri, &tfl, bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["detail.ph"][0],
        "The detail.ph field must not be greater than 14."
    );
    assert_eq!(
        body["errors"]["detail.kebutuhan"][0],
        "The selected detail.kebutuhan is invalid."
    );
    assert_eq!(
        body["errors"]["kecamatan_id"][0],
        "The selected kecamatan id is invalid."
    );
    assert_eq!(
        body["errors"]["latitude"][0],
        "The latitude field must be between -90 and 90."
    );

    let mut ok = survey_body(&format!("{p}-ok"), None);
    ok["kecamatan_id"] = json!(kec);
    let (status, body) = post_json(&pool, uri, &tfl, ok).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let d = &body["data"];
    assert_eq!(d["status"], "diajukan");
    assert_eq!(d["latitude"], -6.2);
    assert_eq!(d["detail"]["menara_tinggi"], 3);
    assert_eq!(d["detail"]["dok_bnba"], true);
    assert_eq!(d["kecamatan"]["nama"], format!("{p}-kecamatan"));
    assert_eq!(d["surveyor"]["id"], tfl_id);
    let id = d["id"].as_i64().unwrap();
    assert_eq!(audit_count(&pool, MODEL, id, "created").await, 1);

    // `detail` sebagai string JSON pada multipart, dengan dua foto dan kategorinya.
    let detail = json!({ "sumber_air": "Sumur", "tipe": "komunal" }).to_string();
    let parts = [
        Part::Text("jenis", "mck_komunal"),
        Part::Text("nama_lokasi", &format!("{p}-mp")),
        Part::Text("detail", &detail),
        Part::Text("foto_kategori[]", "Tampak depan"),
        Part::Text("foto_kategori[]", ""),
        Part::File("foto[]", "depan.jpg", b"\xFF\xD8\xFFuji-satu"),
        Part::File("foto[]", "belakang.pdf", b"%PDF-uji-dua"),
    ];
    let (status, body) = post_multipart(&pool, uri, &tfl, &parts).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["detail"]["tipe"], "komunal");
    let fotos = body["data"]["foto"].as_array().unwrap();
    assert_eq!(fotos.len(), 2, "{body}");
    assert_eq!(fotos[0]["kategori"], "Tampak depan");
    assert_eq!(fotos[1]["kategori"], Value::Null);
    assert_eq!(fotos[0]["name"].as_str().unwrap().ends_with(".jpg"), true);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_with_tugas_requires_assignee_and_matches_jenis() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-tugas";
    cleanup(&pool, p).await;
    let (assignee, a_token) = user(&pool, "uji-sl-tugas-a@example.test", &["tfl"]).await;
    let (_, other) = user(&pool, "uji-sl-tugas-b@example.test", &["tfl"]).await;
    sqlx::query(
        "INSERT INTO tbl_survey_tugas (judul, tahun_anggaran, jenis, assignee_id, status, created_at, updated_at) \
         VALUES (?, 2026, 'mck_komunal', ?, 'ditugaskan', NOW(), NOW())",
    )
    .bind(format!("{p}-t"))
    .bind(assignee)
    .execute(&pool)
    .await
    .unwrap();
    let tugas: i64 =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_survey_tugas WHERE judul = ?")
            .bind(format!("{p}-t"))
            .fetch_one(&pool)
            .await
            .unwrap();
    let uri = "/api/survey-lokasi";

    let (status, body) = post_json(
        &pool,
        uri,
        &other,
        survey_body(&format!("{p}-x"), Some(tugas)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = post_json(
        &pool,
        uri,
        &a_token,
        survey_body(&format!("{p}-x"), Some(999_999)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["tugas_id"][0],
        "The selected tugas id is invalid."
    );

    let mut mismatch = survey_body(&format!("{p}-x"), Some(tugas));
    mismatch["jenis"] = json!("spam_perpipaan");
    let (status, body) = post_json(&pool, uri, &a_token, mismatch).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["jenis"][0],
        "Jenis survey harus sesuai tugas."
    );

    let mut no_jenis = survey_body(&format!("{p}-ok"), Some(tugas));
    no_jenis.as_object_mut().unwrap().remove("jenis");
    let (status, body) = post_json(&pool, uri, &a_token, no_jenis).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["jenis"], "mck_komunal");
    assert_eq!(body["data"]["tugas"]["id"], tugas);
    assert_eq!(body["data"]["tugas"]["status"], "dikerjakan");
    let status_db: String =
        sqlx::query_scalar("SELECT CAST(status AS CHAR) FROM tbl_survey_tugas WHERE id = ?")
            .bind(tugas)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status_db, "dikerjakan");
    assert_eq!(audit_count(&pool, TUGAS_MODEL, tugas, "updated").await, 1);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn update_checks_owner_status_and_audits_changes() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-upd";
    cleanup(&pool, p).await;
    let (_, owner) = user(&pool, "uji-sl-upd-a@example.test", &["tfl"]).await;
    let (_, other) = user(&pool, "uji-sl-upd-b@example.test", &["tfl"]).await;
    let (_, admin) = user(&pool, "uji-sl-upd-admin@example.test", &["admin"]).await;
    let (status, body) = post_json(
        &pool,
        "/api/survey-lokasi",
        &owner,
        survey_body(&format!("{p}-a"), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let uri = format!("/api/survey-lokasi/{id}");

    let (status, body) = put_json(&pool, &uri, &other, json!({ "nama_lokasi": "x" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = put_json(
        &pool,
        &uri,
        &owner,
        json!({ "nama_lokasi": format!("{p}-baru"), "alamat": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_lokasi"], format!("{p}-baru"));
    assert_eq!(body["data"]["alamat"], Value::Null);
    assert_eq!(audit_count(&pool, MODEL, id, "updated").await, 1);

    // Nilai sama: tidak ada perubahan, tidak ada audit baru.
    let (status, _) = put_json(
        &pool,
        &uri,
        &owner,
        json!({ "nama_lokasi": format!("{p}-baru") }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_count(&pool, MODEL, id, "updated").await, 1);

    let (status, body) = put_json(&pool, &uri, &owner, json!({ "jenis": "bukan" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"]["jenis"][0], "The selected jenis is invalid.");

    // Setelah diverifikasi, pemilik tidak boleh mengubah lagi. Admin tetap boleh.
    sqlx::query("UPDATE tbl_survey_lokasi SET status = 'diverifikasi' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = put_json(&pool, &uri, &owner, json!({ "alamat": "baru" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Hanya survei berstatus diajukan yang dapat diubah."
    );
    let (status, body) = put_json(&pool, &uri, &admin, json!({ "alamat": "oleh admin" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["alamat"], "oleh admin");

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn destroy_checks_owner_status_and_audits_delete() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-del";
    cleanup(&pool, p).await;
    let (_, owner) = user(&pool, "uji-sl-del-a@example.test", &["tfl"]).await;
    let (_, other) = user(&pool, "uji-sl-del-b@example.test", &["tfl"]).await;
    let (status, body) = post_json(
        &pool,
        "/api/survey-lokasi",
        &owner,
        survey_body(&format!("{p}-a"), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let uri = format!("/api/survey-lokasi/{id}");

    let (status, body) = delete(&pool, &uri, &other).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = delete(&pool, &uri, &owner).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Survei lokasi berhasil dihapus.");
    assert_eq!(audit_count(&pool, MODEL, id, "deleted").await, 1);
    let (status, _) = get(&pool, &uri, &owner).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let owner_id: u64 =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'uji-sl-del-a@example.test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let id2 = insert_survey(
        &pool,
        owner_id,
        &format!("{p}-b"),
        "a",
        "spam_perpipaan",
        "ditolak",
    )
    .await;
    let (status, body) = delete(&pool, &format!("/api/survey-lokasi/{id2}"), &owner).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Hanya survei berstatus diajukan yang dapat dihapus."
    );

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn verifikasi_is_admin_only_and_moves_tugas_to_selesai() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-ver";
    cleanup(&pool, p).await;
    let (assignee, a_token) = user(&pool, "uji-sl-ver-a@example.test", &["tfl"]).await;
    let (_, admin) = user(&pool, "uji-sl-ver-admin@example.test", &["admin"]).await;
    sqlx::query(
        "INSERT INTO tbl_survey_tugas (judul, tahun_anggaran, jenis, assignee_id, status, created_at, updated_at) \
         VALUES (?, 2026, 'spam_perpipaan', ?, 'dikerjakan', NOW(), NOW())",
    )
    .bind(format!("{p}-t"))
    .bind(assignee)
    .execute(&pool)
    .await
    .unwrap();
    let tugas: i64 =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_survey_tugas WHERE judul = ?")
            .bind(format!("{p}-t"))
            .fetch_one(&pool)
            .await
            .unwrap();
    let (status, body) = post_json(
        &pool,
        "/api/survey-lokasi",
        &a_token,
        survey_body(&format!("{p}-a"), Some(tugas)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let uri = format!("/api/survey-lokasi/{id}/verifikasi");

    let (status, body) =
        post_json(&pool, &uri, &a_token, json!({ "status": "diverifikasi" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "User does not have the right roles.");

    let (status, body) = post_json(&pool, &uri, &admin, json!({ "status": "ditolak" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "Validation error");
    assert_eq!(
        body["errors"]["catatan_verifikasi"][0],
        "Catatan verifikasi wajib diisi jika survei ditolak."
    );

    let (status, body) = post_json(&pool, &uri, &admin, json!({ "status": "x" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["status"][0],
        "The selected status is invalid."
    );

    // Ditolak dengan catatan: tugas tidak berubah.
    let (status, body) = post_json(
        &pool,
        &uri,
        &admin,
        json!({ "status": "ditolak", "catatan_verifikasi": "Foto buram" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "ditolak");
    assert_eq!(body["data"]["catatan_verifikasi"], "Foto buram");
    let tugas_status: String =
        sqlx::query_scalar("SELECT CAST(status AS CHAR) FROM tbl_survey_tugas WHERE id = ?")
            .bind(tugas)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tugas_status, "dikerjakan");

    // Diverifikasi: verifikator tercatat dan tugas menjadi selesai.
    let (status, body) = post_json(&pool, &uri, &admin, json!({ "status": "diverifikasi" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["status"], "diverifikasi");
    assert_eq!(
        body["data"]["verified_by"]["id"],
        admin_id(&pool, "uji-sl-ver-admin@example.test").await
    );
    assert!(body["data"]["verified_at"].is_string());
    assert_eq!(body["data"]["tugas"]["status"], "selesai");
    // Tugas hanya berubah saat diverifikasi (satu audit); survei tercatat untuk kedua verifikasi.
    assert_eq!(audit_count(&pool, TUGAS_MODEL, tugas, "updated").await, 1);
    assert_eq!(audit_count(&pool, MODEL, id, "updated").await, 2);

    cleanup(&pool, p).await;
}

async fn admin_id(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn foto_upload_and_delete_follow_status_and_owner() {
    let pool = MySqlPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let p = "uji-sl-foto";
    cleanup(&pool, p).await;
    let (_, owner) = user(&pool, "uji-sl-foto-a@example.test", &["tfl"]).await;
    let (_, other) = user(&pool, "uji-sl-foto-b@example.test", &["tfl"]).await;
    let (status, body) = post_json(
        &pool,
        "/api/survey-lokasi",
        &owner,
        survey_body(&format!("{p}-a"), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["data"]["id"].as_i64().unwrap();
    let uri = format!("/api/survey-lokasi/{id}/foto");

    let (status, body) = post_multipart(
        &pool,
        &uri,
        &other,
        &[Part::File("foto", "x.jpg", b"\xFF\xD8\xFFx")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = post_multipart(
        &pool,
        &uri,
        &owner,
        &[Part::File("foto", "skrip.exe", b"MZ")],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"]["foto"][0], "The foto field must be a file of type: jpg, jpeg, png, webp, gif, pdf, doc, docx, xls, xlsx.");

    let (status, body) =
        post_multipart(&pool, &uri, &owner, &[Part::Text("kategori", "Sumber air")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"]["foto"][0], "The foto field is required.");

    let (status, body) = post_multipart(
        &pool,
        &uri,
        &owner,
        &[
            Part::Text("kategori", "Sumber air"),
            Part::File("foto", "sumber.png", b"\x89PNG\r\n\x1a\nuji"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let fotos = body["data"]["foto"].as_array().unwrap();
    assert_eq!(fotos.len(), 1);
    assert_eq!(fotos[0]["kategori"], "Sumber air");
    assert!(fotos[0]["url"]
        .as_str()
        .unwrap()
        .starts_with("http://localhost/storage/"));
    assert!(
        body["data"].get("tugas").is_none(),
        "respons unggah foto tidak memuat tugas"
    );
    let media_id = fotos[0]["id"].as_i64().unwrap();

    let (status, body) = delete(
        &pool,
        &format!("/api/survey-lokasi/{id}/foto/{media_id}"),
        &other,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = delete(
        &pool,
        &format!("/api/survey-lokasi/{id}/foto/999999999"),
        &owner,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "Foto tidak ditemukan.");

    let (status, body) = delete(
        &pool,
        &format!("/api/survey-lokasi/{id}/foto/{media_id}"),
        &owner,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Foto berhasil dihapus.");
    let (_, body) = get(&pool, &format!("/api/survey-lokasi/{id}"), &owner).await;
    assert_eq!(body["data"]["foto"], json!([]));

    cleanup(&pool, p).await;
}
