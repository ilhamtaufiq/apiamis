//! Unit SPAM dan integrasi pekerjaan lewat router terhadap MySQL.
//!
//! Data uji memakai penanda `uji-spam-` dan tahun 9201/9202 (di luar data nyata), lalu dibersihkan
//! hanya untuk baris yang dibuat tes ini.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test spam_units_db -- --include-ignored
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

const MARK: &str = "uji-spam";
/// Tahun integrasi (>= 2026) dan tahun manual untuk tes, di luar data nyata.
const TAHUN_INTEGRASI: &str = "9202";
const TAHUN_MANUAL: &str = "9201";

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

/// Buat user admin uji dan kembalikan token Sanctum-nya.
async fn admin_token(pool: &MySqlPool, admin: &str) -> String {
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)",
    )
    .bind(admin)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(admin)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Spam', ?, 'x', NOW(), NOW())")
        .bind(admin)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(admin)
        .fetch_one(pool)
        .await
        .unwrap();
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
    auth::login::create_token(pool, uid, &format!("uji-spam-{admin}"))
        .await
        .unwrap()
}

/// Kecamatan dan desa uji dengan nama penanda. Mengembalikan `(kecamatan_id, desa_id)`.
async fn seed_wilayah(pool: &MySqlPool, tag: &str) -> (i64, i64) {
    sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(format!("{MARK}-kec-{tag}"))
    .execute(pool)
    .await
    .unwrap();
    let kec: i64 =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_kecamatan WHERE n_kec = ?")
            .bind(format!("{MARK}-kec-{tag}"))
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO tbl_desa (n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(format!("{MARK}-desa-{tag}"))
        .bind(kec)
        .execute(pool)
        .await
        .unwrap();
    let desa: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_desa WHERE n_desa = ?")
        .bind(format!("{MARK}-desa-{tag}"))
        .fetch_one(pool)
        .await
        .unwrap();
    (kec, desa)
}

async fn exec(pool: &MySqlPool, sql: &str, binds: Vec<Value>) -> u64 {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = match b {
            Value::String(s) => q.bind(s),
            Value::Number(n) => match n.as_i64() {
                Some(i) => q.bind(i),
                None => q.bind(n.as_f64().unwrap()),
            },
            Value::Null => q.bind(None::<String>),
            other => q.bind(other.to_string()),
        };
    }
    q.execute(pool).await.unwrap().last_insert_id()
}

/// Hapus baris uji yang dibuat `seed_wilayah` dan data pekerjaan uji.
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let desa_name = format!("{MARK}-desa-{tag}");
    let kec_name = format!("{MARK}-kec-{tag}");
    let desa_ids: Vec<i64> =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_desa WHERE n_desa = ?")
            .bind(&desa_name)
            .fetch_all(pool)
            .await
            .unwrap();
    for d in &desa_ids {
        sqlx::query("DELETE FROM tbl_penerima WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE desa_id = ? AND nama_paket LIKE ?)")
            .bind(d)
            .bind(format!("{MARK}%"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_output WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE desa_id = ? AND nama_paket LIKE ?)")
            .bind(d)
            .bind(format!("{MARK}%"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_pekerjaan WHERE desa_id = ? AND nama_paket LIKE ?")
            .bind(d)
            .bind(format!("{MARK}%"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_unit_spam WHERE desa_id = ?")
            .bind(d)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_sub_kegiatan = ?")
        .bind(format!("{MARK}-sub-{tag}"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa = ?")
        .bind(&desa_name)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(&kec_name)
        .execute(pool)
        .await
        .unwrap();
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_units_requires_auth_and_validates_like_laravel() {
    let pool = pool().await;
    let token = admin_token(&pool, "uji-spam-admin-auth@example.test").await;

    let (status, _) = send(&pool, Method::GET, "/api/spam-units", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/spam-units",
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spam-units",
        Some(&token),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The desa id field is required.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spam-units",
        Some(&token),
        Some(json!({ "desa_id": 987654321, "is_simspam": true })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"], "The selected desa id is invalid.",
        "{body}"
    );

    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/spam-units/987654321",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::GET, "/api/spam-units/987654321", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "404 dulu sebelum auth");

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/spam-units/integration?sync_status=salah",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_unit_crud_achievement_and_budget() {
    let pool = pool().await;
    let tag = "crud";
    cleanup(&pool, tag).await;
    let token = admin_token(&pool, "uji-spam-admin-crud@example.test").await;
    let (_kec, desa) = seed_wilayah(&pool, tag).await;

    // Simpan: 201, pengelola ikut tersimpan.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spam-units",
        Some(&token),
        Some(json!({
            "desa_id": desa, "name": format!("{MARK}-unit-1"), "is_simspam": true,
            "pokmas": format!("{MARK}-pokmas"), "kepala": format!("{MARK}-kepala")
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["message"], "Unit SPAM berhasil ditambahkan");
    let unit = body["data"]["id"].as_i64().unwrap();
    assert_eq!(body["data"]["is_simspam"], true);
    assert_eq!(
        body["data"]["pengelola"]["kepala"],
        format!("{MARK}-kepala")
    );

    // Baca: relasi desa dan kecamatan.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["desa"]["n_desa"], format!("{MARK}-desa-{tag}"));
    assert_eq!(
        body["data"]["desa"]["kecamatan"]["n_kec"],
        format!("{MARK}-kec-{tag}")
    );
    assert_eq!(body["data"]["pekerjaan"], json!([]));

    // Daftar dengan pencarian: unit ini ada di halaman hasil.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units?search={MARK}-unit-1&per_page=50"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["id"] == unit),
        "{body}"
    );
    assert!(body["meta"]["total"].as_i64().unwrap() >= 1);
    assert!(body["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|u| u["pengelola"].is_object() || u["pengelola"].is_null()));

    // Ubah: nama dan pokmas berubah, pengelola ikut diperbarui.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        Some(json!({
            "desa_id": desa, "name": format!("{MARK}-unit-2"), "is_simspam": false,
            "pokmas": format!("{MARK}-pokmas-baru")
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Unit SPAM berhasil diperbarui");
    assert_eq!(body["data"]["name"], format!("{MARK}-unit-2"));
    assert_eq!(body["data"]["is_simspam"], false);
    assert_eq!(
        body["data"]["pengelola"]["pokmas"],
        format!("{MARK}-pokmas-baru")
    );
    assert_eq!(
        body["data"]["pengelola"]["kepala"],
        Value::Null,
        "field pengelola yang tidak dikirim dikosongkan"
    );

    // Achievement manual: tahun sama diperbarui (bukan dobel).
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spam-units/{unit}/achievements"),
        Some(&token),
        Some(json!({ "tahun": TAHUN_MANUAL, "jumlah_sr": 5, "jumlah_kk": 4, "jumlah_jiwa": 20 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["jumlah_bjp_jiwa"], 0);
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spam-units/{unit}/achievements"),
        Some(&token),
        Some(json!({ "tahun": TAHUN_MANUAL, "jumlah_sr": 7, "jumlah_kk": 6, "jumlah_jiwa": 30, "jumlah_bjp_kk": 2 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["jumlah_sr"], 7);
    assert_eq!(body["data"]["jumlah_bjp_jiwa"], 10, "bjp jiwa = bjp kk x 5");
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["achievements"].as_array().unwrap().len(),
        1,
        "{body}"
    );

    // Anggaran: tambah, hapus, lalu 404 untuk id yang sama.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spam-units/{unit}/budgets"),
        Some(&token),
        Some(json!({ "tahun": TAHUN_MANUAL, "nilai_kontrak": 1000.5, "nama_paket": format!("{MARK}-anggaran") })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["sumber_dana"], "APBD");
    let budget = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spam-units/{unit}/budgets/{budget}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Data anggaran berhasil dihapus!");
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spam-units/{unit}/budgets/{budget}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Hapus unit.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_unit_pekerjaan_integration_end_to_end() {
    let pool = pool().await;
    let tag = "integrasi";
    cleanup(&pool, tag).await;
    let token = admin_token(&pool, "uji-spam-admin-integrasi@example.test").await;
    let (kec, desa) = seed_wilayah(&pool, tag).await;

    // Kegiatan air minum tahun integrasi, dua penerima, satu output SR.
    let kegiatan = exec(
        &pool,
        "INSERT INTO tbl_kegiatan (nama_program, sub_bidang, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) \
         VALUES ('uji', 'Air Minum', 'uji kegiatan', ?, ?, 'uji-APBD-spam', 1000000.00, NOW(), NOW())",
        vec![json!(format!("{MARK}-sub-{tag}")), json!(TAHUN_INTEGRASI)],
    )
    .await as i64;
    let pekerjaan = exec(
        &pool,
        "INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 1000000.0, 0, 'active', NOW(), NOW())",
        vec![json!(format!("{MARK}-paket-1")), json!(kec), json!(desa), json!(kegiatan)],
    )
    .await as i64;
    let output = exec(
        &pool,
        "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, created_at, updated_at) VALUES (?, 'Sambungan Rumah (SR)', 'unit', 10, NOW(), NOW())",
        vec![json!(pekerjaan)],
    )
    .await as i64;
    for jiwa in [3, 4] {
        exec(
            &pool,
            "INSERT INTO tbl_penerima (pekerjaan_id, nama, jumlah_jiwa, is_komunal, created_at, updated_at) VALUES (?, 'uji penerima', ?, 0, NOW(), NOW())",
            vec![json!(pekerjaan), json!(jiwa)],
        )
        .await;
    }

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/spam-units",
        Some(&token),
        Some(json!({ "desa_id": desa, "name": format!("{MARK}-unit-int"), "is_simspam": true })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let unit = body["data"]["id"].as_i64().unwrap();

    // Tautkan paket: akumulasi SR 10, KK 2, jiwa 7, anggaran pagu.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spam-units/{unit}/pekerjaan"),
        Some(&token),
        Some(json!({ "pekerjaan_id": pekerjaan, "output_id": output })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("diakumulasi"),
        "{body}"
    );
    assert_eq!(body["data"]["pekerjaan"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["pekerjaan"][0]["pivot"]["output_id"], output);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ach = &body["data"]["achievements"][0];
    assert_eq!(ach["tahun"], TAHUN_INTEGRASI, "{body}");
    assert_eq!(ach["sumber"], "integrasi");
    assert_eq!(ach["jumlah_sr"], 10);
    assert_eq!(ach["jumlah_kk"], 2);
    assert_eq!(ach["jumlah_jiwa"], 7);
    let budget = &body["data"]["budgets"][0];
    assert_eq!(budget["pekerjaan_id"], pekerjaan);
    assert_eq!(budget["nilai_kontrak"], 1000000.0);
    assert_eq!(budget["sumber_dana"], "uji-APBD-spam");

    // Integrasi per desa dan daftar paket.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/integration?tahun={TAHUN_INTEGRASI}&desa_id={desa}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["sync_status"], "matched");
    assert_eq!(body["data"][0]["pekerjaan"][0]["sr"], 10);
    assert_eq!(body["summary"]["matched_count"], 1);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/integration/desa/{desa}?tahun={TAHUN_INTEGRASI}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan_count"], 1);
    assert_eq!(body["data"]["linked_count"], 1);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/air-minum-pekerjaan?tahun={TAHUN_INTEGRASI}&unit_spam_id={unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["id"], pekerjaan);
    assert_eq!(body["data"][0]["is_linked"], true);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/integration/output-options?tahun={TAHUN_INTEGRASI}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["komponen"] == "Sambungan Rumah (SR)"
                && o["output_type"] == "sambungan_rumah"),
        "{body}"
    );

    // Statistik: capaian tahun integrasi (manual + integrasi).
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/stats?tahun={TAHUN_INTEGRASI}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["total_sr"], 10, "{body}");
    assert_eq!(body["data"]["total_kk"], 2);
    assert_eq!(body["data"]["manual_sr"], 10);
    assert_eq!(body["data"]["ringkasan"]["capaian"]["sr"], 10);

    // Tamu: capaian manual tetap tampil, paket tidak (byUserRole = 1 = 0).
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/public/spam-units/stats?tahun={TAHUN_INTEGRASI}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["total_sr"], 10);
    assert_eq!(body["data"]["ringkasan"]["integrasi"]["paket_tersedia"], 0);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/public/spam-units/map-stats?tahun={TAHUN_INTEGRASI}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["desa_id"] == desa)
        .cloned();
    let row = row.unwrap_or_else(|| panic!("desa uji tidak ada di map-stats: {body}"));
    assert_eq!(row["sr"], 10);
    assert_eq!(row["unit_count"], 1);

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/public/spam-units/map-stats/series?years={TAHUN_INTEGRASI},abcd"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"][TAHUN_INTEGRASI].is_array(), "{body}");

    // Sinkron ulang: hasil sama, dan rekam integrasi tetap satu.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spam-units/{unit}/sync-pekerjaan"),
        Some(&token),
        Some(json!({ "tahun": TAHUN_INTEGRASI, "mode": "all" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["achievements"].as_array().unwrap().len(),
        1,
        "{body}"
    );

    // Lepas paket: rekam dan anggaran integrasi ikut hilang.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spam-units/{unit}/pekerjaan/{pekerjaan}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], unit);
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/{unit}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan"], json!([]));
    assert_eq!(body["data"]["achievements"], json!([]), "{body}");
    assert_eq!(body["data"]["budgets"], json!([]), "{body}");

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spam_units_stats_series_keeps_only_four_digit_years() {
    let pool = pool().await;
    let token = admin_token(&pool, "uji-spam-admin-series@example.test").await;
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spam-units/stats/series?years={TAHUN_MANUAL},abcd,{TAHUN_MANUAL}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"][TAHUN_MANUAL]["total_units"].is_number(),
        "{body}"
    );
    assert!(body["data"].get("abcd").is_none());

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/spam-units/stats/series?years=abcd",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"], json!([]));
}
