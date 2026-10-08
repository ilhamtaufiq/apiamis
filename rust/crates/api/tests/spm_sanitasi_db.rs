//! SPM sanitasi lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test spm_sanitasi_db -- --include-ignored
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

/// Hapus user uji beserta peran dan token Sanctum-nya (email unik per tes), supaya tes bisa diulang.
async fn remove_user(pool: &MySqlPool, email: &str) {
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
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
}

/// User admin uji dengan email unik per tes, lalu token Sanctum-nya.
async fn admin_token(pool: &MySqlPool, email: &str) -> String {
    remove_user(pool, email).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji SPM', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(pool)
        .await
        .unwrap();
    let role: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    auth::login::create_token(pool, uid, "uji-spm")
        .await
        .unwrap()
}

/// User tanpa peran admin, untuk memeriksa pemblokiran mutasi.
async fn plain_token(pool: &MySqlPool, email: &str) -> String {
    remove_user(pool, email).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji SPM Biasa', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    auth::login::create_token(pool, uid, "uji-spm")
        .await
        .unwrap()
}

/// Kecamatan, dua desa wilayah resmi, dan satu desa placeholder bernama `NULL` (bukan wilayah resmi).
struct Seed {
    kecamatan: i64,
    desa1: i64,
    desa2: i64,
    desa_null: i64,
}

async fn insert_id(pool: &MySqlPool, sql: &str, binds: Vec<Value>) -> i64 {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = match b {
            Value::String(s) => q.bind(s),
            Value::Number(n) => q.bind(n.as_i64().unwrap()),
            other => panic!("nilai uji tak didukung: {other}"),
        };
    }
    q.execute(pool).await.unwrap().last_insert_id() as i64
}

async fn seed(pool: &MySqlPool, tag: &str) -> Seed {
    let kecamatan = insert_id(
        pool,
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
        vec![json!(format!("uji-spm-{tag}-kec"))],
    )
    .await;
    let desa1 = insert_id(
        pool,
        "INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES (?, 1000, 300, ?, NOW(), NOW())",
        vec![json!(format!("uji-spm-{tag}-desa-1")), json!(kecamatan)],
    )
    .await;
    let desa2 = insert_id(
        pool,
        "INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES (?, 500, 50, ?, NOW(), NOW())",
        vec![json!(format!("uji-spm-{tag}-desa-2")), json!(kecamatan)],
    )
    .await;
    let desa_null = insert_id(
        pool,
        "INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES (?, 300, 30, ?, NOW(), NOW())",
        vec![json!("NULL"), json!(kecamatan)],
    )
    .await;
    Seed {
        kecamatan,
        desa1,
        desa2,
        desa_null,
    }
}

/// Baris SPM uji. `pembiayaan` `None` berarti kolom NULL.
async fn spm(
    pool: &MySqlPool,
    desa: i64,
    jenis: &str,
    kk: i64,
    tahun: i64,
    status: Option<&str>,
    pembiayaan: Option<f64>,
    nama: &str,
) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_spm_sanitasi (jenis, desa_id, nama_infrastruktur, jumlah_pemanfaat_kk, tahun_konstruksi, \
         status_keberfungsian, pembiayaan_total, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(jenis)
    .bind(desa)
    .bind(nama)
    .bind(kk)
    .bind(tahun)
    .bind(status)
    .bind(pembiayaan)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64
}

/// Hapus semua baris uji milik `tag`: kecamatan `uji-spm-{tag}-kec`, desanya, dan spm di dalamnya,
/// beserta audit dan notifikasi untuk spm tersebut. `extra` berisi id spm yang sudah terhapus saat tes.
async fn cleanup(pool: &MySqlPool, tag: &str, extra: &[i64]) {
    let kec_name = format!("uji-spm-{tag}-kec");
    let spm_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(s.id AS SIGNED) FROM tbl_spm_sanitasi s \
         JOIN tbl_desa d ON d.id = s.desa_id JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?",
    )
    .bind(&kec_name)
    .fetch_all(pool)
    .await
    .unwrap();
    let mut all = spm_ids;
    all.extend_from_slice(extra);
    for id in &all {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        // Cari id dulu (baca tanpa kunci), lalu hapus per primary key: hapus berdasarkan LIKE memegang
        // kunci celah pada tabel yang dipakai bersama dan memicu deadlock saat tes berjalan paralel.
        let notif_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM notifications WHERE data LIKE ?")
            .bind(format!("%Model SpmSanitasi dengan ID #{id} %"))
            .fetch_all(pool)
            .await
            .unwrap();
        for nid in notif_ids {
            sqlx::query("DELETE FROM notifications WHERE id = ?")
                .bind(nid)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE d FROM tbl_desa d JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?")
        .bind(&kec_name)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(&kec_name)
        .execute(pool)
        .await
        .unwrap();
}

fn assert_f(v: &Value, expected: f64) {
    let got = v.as_f64().unwrap_or_else(|| panic!("bukan angka: {v}"));
    assert!((got - expected).abs() < 1e-9, "dapat {got}, harus {expected}: {v}");
}

fn db_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spm_sanitasi_stats_capaian_and_public_match_laravel() {
    let pool = sqlx::MySqlPool::connect(&db_url()).await.unwrap();
    let tag = "stat";
    let tahun = 1971;
    cleanup(&pool, tag, &[]).await;
    let token = admin_token(&pool, "uji-spm-stat-admin@example.test").await;
    let s = seed(&pool, tag).await;
    let a = spm(&pool, s.desa1, "spaldt", 100, tahun, Some("Berfungsi"), Some(1_000_000.0), "uji-spm-stat-a").await;
    let b = spm(&pool, s.desa1, "mck_individu", 50, tahun, Some("Tidak Berfungsi"), Some(500_000.0), "uji-spm-stat-b").await;
    let c = spm(&pool, s.desa2, "iplt", 10, tahun, Some("Berfungsi"), Some(250_000.5), "uji-spm-stat-c").await;
    // Desa placeholder: masuk statistik (tanpa filter wilayah), tidak masuk capaian maupun peta.
    let d = spm(&pool, s.desa_null, "spalds", 7, tahun, None, None, "uji-spm-stat-d").await;

    // Stats: filter kecamatan dan tahun, dengan ringkasan capaian di dalamnya.
    let kec = s.kecamatan;
    let uri = format!("/api/spm-sanitasi/stats?kecamatan_id={kec}&tahun={tahun}");
    let (status, body) = send(&pool, Method::GET, &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    assert_eq!(data["spaldt_count"], 1, "{body}");
    assert_eq!(data["spalds_count"], 1, "{body}");
    assert_eq!(data["iplt_count"], 1, "{body}");
    assert_eq!(data["mck_individu_count"], 1, "{body}");
    assert_eq!(data["total_count"], 4, "{body}");
    assert_eq!(data["berfungsi_count"], 2, "{body}");
    // `array_merge` memakai nilai capaian untuk kunci yang sama, sehingga 160 (bukan 167 dari statistik).
    assert_eq!(data["total_pemanfaat_kk"], 160, "{body}");
    assert_f(&data["total_investasi"], 1_750_000.5);
    assert_eq!(data["total_desa"], 2, "{body}");
    assert_eq!(data["desa_with_infrastruktur"], 2, "{body}");
    assert_eq!(data["desa_without_infrastruktur"], 0, "{body}");
    assert_eq!(data["total_penduduk"], 1500, "{body}");
    assert_f(&data["coverage_percentage"], 53.33);

    // Capaian: placeholder tidak ikut; ringkasan total_pemanfaat_kk 160 (tanpa baris placeholder).
    let uri = format!("/api/spm-sanitasi/capaian?kecamatan_id={kec}&tahun={tahun}");
    let (status, body) = send(&pool, Method::GET, &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["summary"]["total_pemanfaat_kk"], 160, "{body}");
    assert_eq!(body["summary"]["total_pemanfaat_jiwa"], 800, "{body}");
    assert_eq!(body["summary"]["gap_kk"], 190, "{body}");
    assert_eq!(body["summary"]["by_jenis"]["spaldt"]["pemanfaat_kk"], 100, "{body}");
    assert_eq!(body["summary"]["by_jenis"]["spalds"]["unit_count"], 0, "{body}");
    assert_f(&body["summary"]["coverage_kk_percentage"], 45.71);
    assert_eq!(body["meta"]["total"], 2, "{body}");
    // Urut coverage naik: desa2 (10%) dulu, lalu desa1 (75%).
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows[0]["desa"]["id"], s.desa2, "{body}");
    assert_f(&rows[0]["coverage_percentage"], 10.0);
    assert_f(&rows[0]["coverage_kk_percentage"], 20.0);
    assert_eq!(rows[1]["desa"]["id"], s.desa1, "{body}");
    assert_f(&rows[1]["coverage_percentage"], 75.0);
    assert_eq!(rows[1]["pemanfaat_kk"], 150, "{body}");
    assert_eq!(rows[1]["unit_count"], 2, "{body}");
    assert_eq!(rows[1]["by_jenis"]["mck_individu_kk"], 50, "{body}");

    // Urut nama desa menurun dan pagination 1 per halaman.
    let uri = format!("/api/spm-sanitasi/capaian?kecamatan_id={kec}&tahun={tahun}&sort=n_desa&direction=desc&per_page=1&page=2");
    let (status, body) = send(&pool, Method::GET, &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["last_page"], 2, "{body}");
    // Urut nama menurun: desa-2 di halaman 1, desa-1 di halaman 2.
    assert_eq!(body["data"][0]["desa"]["id"], s.desa1, "{body}");

    // Validasi parameter capaian.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/spm-sanitasi/capaian?sort=ngawur",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The selected sort is invalid.", "{body}");

    // Publik: tanpa token, tahun unik, hanya baris yang punya desa.
    let wilayah: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kecamatan WHERE n_kec IS NOT NULL AND n_kec <> '' AND LOWER(TRIM(n_kec)) NOT IN ('null','nulls')")
        .fetch_one(&pool)
        .await
        .unwrap();
    let uri = format!("/api/public/spm-sanitasi/stats?tahun={tahun}");
    let (status, body) = send(&pool, Method::GET, &uri, None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    assert_eq!(data["scope_label"], format!("Infrastruktur tahun konstruksi {tahun}"), "{body}");
    assert_eq!(data["total_count"], 4, "{body}");
    assert_eq!(data["berfungsi_count"], 2, "{body}");
    assert_eq!(data["total_pemanfaat_kk"], 160, "{body}");
    assert_eq!(data["spaldt_count"], 1, "{body}");
    assert_eq!(data["wilayah_total_kecamatan"], wilayah, "{body}");
    assert_f(&data["total_investasi"], 1_750_000.5);
    assert!(data["stats_generated_at"].as_str().unwrap().ends_with("+00:00"), "{body}");

    // Peta publik: satu baris per desa wilayah resmi, tanpa placeholder.
    let uri = format!("/api/public/spm-sanitasi/map-stats?tahun={tahun}");
    let (status, body) = send(&pool, Method::GET, &uri, None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["data"].as_array().unwrap();
    let r1 = rows.iter().find(|r| r["desa_id"] == s.desa1).expect("desa1 ada");
    assert_eq!(r1["unit_count"], 2, "{body}");
    assert_eq!(r1["pemanfaat_kk"], 150, "{body}");
    assert_eq!(r1["pemanfaat_jiwa"], 750, "{body}");
    assert_eq!(r1["kecamatan"], format!("uji-spm-{tag}-kec"), "{body}");
    let r2 = rows.iter().find(|r| r["desa_id"] == s.desa2).expect("desa2 ada");
    assert_eq!(r2["pemanfaat_kk"], 10, "{body}");
    assert!(rows.iter().all(|r| r["desa_id"] != s.desa_null), "placeholder ikut: {body}");

    // Tanpa token: 401 untuk rute terautentikasi.
    let (status, _) = send(&pool, Method::GET, "/api/spm-sanitasi/stats", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup(&pool, tag, &[a, b, c, d]).await;
    remove_user(&pool, "uji-spm-stat-admin@example.test").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spm_sanitasi_crud_validates_audits_and_blocks_non_admin() {
    let pool = sqlx::MySqlPool::connect(&db_url()).await.unwrap();
    let tag = "crud";
    let tahun = 1972;
    cleanup(&pool, tag, &[]).await;
    let token = admin_token(&pool, "uji-spm-crud-admin@example.test").await;
    let biasa = plain_token(&pool, "uji-spm-crud-biasa@example.test").await;
    let s = seed(&pool, tag).await;
    let mut extra: Vec<i64> = Vec::new();

    // Validasi: pesan pertama sesuai urutan aturan Laravel.
    let cases: Vec<(Value, &str)> = vec![
        (json!({}), "The jenis field is required."),
        (json!({"jenis": "xx"}), "The selected jenis is invalid."),
        (json!({"jenis": "spaldt"}), "The nama infrastruktur field is required."),
        (
            json!({"jenis": "spaldt", "nama_infrastruktur": "uji", "desa_id": 999999999}),
            "The selected desa id is invalid.",
        ),
        (
            json!({"jenis": "spaldt", "nama_infrastruktur": "uji", "tahun_konstruksi": 1800}),
            "The tahun konstruksi field must be at least 1900.",
        ),
        (
            json!({"jenis": "spaldt", "nama_infrastruktur": "uji", "pembiayaan_total": "abc"}),
            "The pembiayaan total field must be a number.",
        ),
        (
            json!({"jenis": "spaldt", "nama_infrastruktur": "uji", "jumlah_pemanfaat_kk": 1.5}),
            "The jumlah pemanfaat kk field must be an integer.",
        ),
    ];
    for (body, expected) in cases {
        let (status, resp) = send(&pool, Method::POST, "/api/spm-sanitasi", Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
        assert_eq!(resp["message"], expected, "{resp}");
    }

    // Tanpa token dan non-admin.
    let (status, _) = send(&pool, Method::POST, "/api/spm-sanitasi", None, Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/spm-sanitasi",
        Some(&biasa),
        Some(json!({"jenis": "spaldt", "nama_infrastruktur": "uji"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Simpan: 201, relasi desa dan kecamatan dimuat, flag integrasi dari input.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spm-sanitasi",
        Some(&token),
        Some(json!({
            "jenis": "spalds",
            "desa_id": s.desa1,
            "nama_infrastruktur": "uji-spm-crud-instalasi",
            "latitude": "-6.5",
            "jumlah_pemanfaat_kk": 20,
            "tahun_konstruksi": tahun,
            "pembiayaan_total": 1000.5,
            "pemanfaat_dari_integrasi": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let data = &body["data"];
    assert_eq!(body["message"], "Data SPM Sanitasi berhasil ditambahkan");
    let id = data["id"].as_i64().unwrap();
    extra.push(id);
    assert_eq!(data["desa"]["n_desa"], format!("uji-spm-{tag}-desa-1"), "{body}");
    assert_eq!(data["desa"]["kecamatan"]["n_kec"], format!("uji-spm-{tag}-kec"), "{body}");
    assert_f(&data["latitude"], -6.5);
    assert_f(&data["pembiayaan_total"], 1000.5);
    assert_eq!(data["pemanfaat_dari_integrasi"], true, "{body}");
    assert_eq!(data["pembiayaan_dari_integrasi"], false, "{body}");
    assert!(data["status_keberfungsian"].is_null(), "{body}");

    let audit_created: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ? AND event = 'created'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_created, 1);

    // Baca daftar dengan pencarian nama dan pembungkus paginator.
    let uri = format!("/api/spm-sanitasi?search=uji-spm-crud&jenis=spalds&desa_id={}", s.desa1);
    let (status, body) = send(&pool, Method::GET, &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1, "{body}");
    assert_eq!(body["meta"]["per_page"], 15, "{body}");
    assert_eq!(body["data"][0]["id"], id, "{body}");
    assert!(body.get("links").is_none(), "{body}");

    // Ubah tanpa perubahan nilai: 200, tanpa audit baru.
    let uri = format!("/api/spm-sanitasi/{id}");
    let (status, body) = send(
        &pool,
        Method::PUT,
        &uri,
        Some(&token),
        Some(json!({"nama_infrastruktur": "uji-spm-crud-instalasi", "jenis": "spalds"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let audit_updated: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ? AND event = 'updated'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_updated, 0);

    // Ubah jumlah pemanfaat KK: flag integrasi pemanfaat ikut kembali ke false.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &uri,
        Some(&token),
        Some(json!({"nama_infrastruktur": "uji-spm-crud-instalasi-2", "jumlah_pemanfaat_kk": 25})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_infrastruktur"], "uji-spm-crud-instalasi-2", "{body}");
    assert_eq!(body["data"]["jumlah_pemanfaat_kk"], 25, "{body}");
    assert_eq!(body["data"]["pemanfaat_dari_integrasi"], false, "{body}");
    let audit_updated: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ? AND event = 'updated'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_updated, 1);

    // Tanpa nama infrastruktur: wajib (aturan `required` berlaku juga saat ubah).
    let (status, body) = send(&pool, Method::PUT, &uri, Some(&token), Some(json!({"jumlah_pemanfaat_kk": 3}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The nama infrastruktur field is required.", "{body}");

    // Id tidak ada: 404.
    let (status, _) = send(&pool, Method::PUT, "/api/spm-sanitasi/999999999", Some(&token), Some(json!({"nama_infrastruktur": "x"}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Hapus: 200, baris hilang, audit deleted tercatat.
    let (status, body) = send(&pool, Method::DELETE, &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Data SPM Sanitasi berhasil dihapus");
    let remaining: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    let audit_deleted: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ? AND event = 'deleted'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_deleted, 1);

    cleanup(&pool, tag, &extra).await;
    remove_user(&pool, "uji-spm-crud-admin@example.test").await;
    remove_user(&pool, "uji-spm-crud-biasa@example.test").await;
}
