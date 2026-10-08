//! Penugasan user-pekerjaan lewat router terhadap MySQL: index, store, destroy, byUser, byPekerjaan,
//! availableUsers, dan completenessGaps. Semua data uji memakai marker `uji-up-` / `UJI-UP`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test user_pekerjaan_db -- --include-ignored
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

const MODEL_USER: &str = r"App\Models\User";

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
    token: Option<&String>,
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

/// User dengan satu role opsional (role dibuat bila belum ada). Email unik per tes.
async fn make_user(pool: &MySqlPool, email: &str, role: Option<&str>) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(format!("Uji {email}"))
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if let Some(role) = role {
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
        sqlx::query(
            "INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)",
        )
        .bind(rid)
        .bind(MODEL_USER)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    }
    uid
}

async fn make_kegiatan(pool: &MySqlPool, nama: &str, tahun: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_kegiatan (nama_program, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES (?, ?, 'APBD', 1000, NOW(), NOW())")
        .bind(nama)
        .bind(tahun)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar(
        "SELECT id FROM tbl_kegiatan WHERE nama_program = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(nama)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn make_pekerjaan(pool: &MySqlPool, nama: &str, kegiatan: Option<u64>) -> u64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, ?, 1500000, 0, 'active', NOW(), NOW())")
        .bind(nama)
        .bind(kegiatan)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_pekerjaan WHERE nama_paket = ? ORDER BY id DESC LIMIT 1")
        .bind(nama)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus data uji milik tes ini saja: berdasarkan email user dan marker nama pekerjaan/kegiatan.
async fn purge(pool: &MySqlPool, emails: &[&str], marker: &str) {
    let like = format!("{marker}%");
    sqlx::query("DELETE FROM tbl_foto WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    for table in ["tbl_penerima", "tbl_output", "tbl_progress"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)"
        ))
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query("DELETE FROM user_pekerjaan WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_program LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    for email in emails {
        sqlx::query("DELETE FROM notifications WHERE notifiable_type = ? AND notifiable_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(MODEL_USER)
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "DELETE FROM user_pekerjaan WHERE user_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM model_has_roles WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(MODEL_USER)
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
}

async fn token_for(pool: &MySqlPool, uid: u64) -> String {
    auth::login::create_token(pool, uid, "uji-up")
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn index_store_destroy_flow() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let marker = "UJI-UP-flow";
    let admin_email = "uji-up-flow-admin@example.test";
    let plain_email = "uji-up-flow-plain@example.test";
    let target_email = "uji-up-flow-target@example.test";
    purge(&pool, &[admin_email, plain_email, target_email], marker).await;

    let admin = make_user(&pool, admin_email, Some("admin")).await;
    let plain = make_user(&pool, plain_email, Some("user")).await;
    let target = make_user(&pool, target_email, None).await;
    let admin_tok = token_for(&pool, admin).await;
    let plain_tok = token_for(&pool, plain).await;
    let kegiatan = make_kegiatan(&pool, &format!("{marker} keg"), "2099").await;
    let p1 = make_pekerjaan(&pool, &format!("{marker} A"), Some(kegiatan)).await;
    let p2 = make_pekerjaan(&pool, &format!("{marker} B"), None).await;

    // Akses: tanpa token 401, non-admin 403.
    let (s, _) = send(&pool, Method::GET, "/api/user-pekerjaan", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan",
        Some(&plain_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&plain_tok),
        Some(json!({ "user_id": target, "pekerjaan_ids": [p1] })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Validasi.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        Some(json!({ "pekerjaan_ids": [p1] })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(b["errors"]["user_id"][0], "The user id field is required.");
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        Some(json!({ "user_id": target, "pekerjaan_ids": "abc" })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        b["errors"]["pekerjaan_ids"][0],
        "The pekerjaan ids field must be an array."
    );
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        Some(json!({ "user_id": target, "pekerjaan_ids": [p1, 999_999_999] })),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        b["errors"]["pekerjaan_ids.1"][0],
        "The selected pekerjaan ids.1 is invalid."
    );

    // Store: id duplikat hanya satu baris; user tanpa peran mendapat `pengawas`; notifikasi terkirim.
    let (s, b) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        Some(json!({ "user_id": target, "pekerjaan_ids": [p1, p2, p1] })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    assert_eq!(b["status"], "success");
    assert_eq!(b["message"], "Pekerjaan berhasil di-assign ke user");

    let rows: Vec<(u64, String)> = sqlx::query_as(
        "SELECT pekerjaan_id, CAST(created_at AS CHAR) FROM user_pekerjaan WHERE user_id = ? ORDER BY pekerjaan_id",
    )
    .bind(target)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "pekerjaan duplikat harus satu baris");
    let created_p1 = rows[0].1.clone();

    let has_pengawas: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM model_has_roles mr JOIN roles r ON r.id = mr.role_id WHERE mr.model_type = ? AND mr.model_id = ? AND r.name = 'pengawas'",
    )
    .bind(MODEL_USER)
    .bind(target)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(has_pengawas, 1);

    // Store pertama menulis satu notifikasi dengan jumlah input 3 dan dua nama pekerjaan.
    let notifs: Vec<String> = sqlx::query_scalar(
        "SELECT CAST(data AS CHAR) FROM notifications WHERE notifiable_type = ? AND notifiable_id = ?",
    )
    .bind(MODEL_USER)
    .bind(target)
    .fetch_all(&pool)
    .await
    .unwrap();
    let expected = format!("Anda telah di-assign ke 3 pekerjaan baru: {marker} A, {marker} B");
    let notif = notifs
        .iter()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .find(|n| n["message"] == json!(expected))
        .expect("notifikasi store pertama tidak ditemukan");
    assert_eq!(notif["title"], "Penugasan Pekerjaan Baru");
    assert_eq!(notif["url"], "/pekerjaan");
    assert_eq!(notif["type"], "info");
    assert_eq!(notifs.len(), 1);

    // Store ulang: pekerjaan yang sudah ada tidak diubah timestamp-nya.
    let (s, _) = send(
        &pool,
        Method::POST,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        Some(json!({ "user_id": target, "pekerjaan_ids": [p1] })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let created_again: String = sqlx::query_scalar(
        "SELECT CAST(created_at AS CHAR) FROM user_pekerjaan WHERE user_id = ? AND pekerjaan_id = ?",
    )
    .bind(target)
    .bind(p1)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(created_again, created_p1);

    // Index: baris uji ada, berurutan terbaru dulu, dengan nama user dan pekerjaan.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let ours: Vec<&Value> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["user_id"] == json!(target))
        .collect();
    assert_eq!(ours.len(), 2);
    let first = ours[0];
    assert_eq!(first["user_email"], target_email);
    assert!(first["pekerjaan_nama"]
        .as_str()
        .unwrap()
        .starts_with(marker));
    assert_eq!(first["pekerjaan_pagu"], json!(1500000));

    // Destroy: 404 untuk id tidak ada, 200 untuk baris uji, 404 saat diulang.
    let (s, b) = send(
        &pool,
        Method::DELETE,
        "/api/user-pekerjaan/999999999",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(b["status"], "error");
    assert_eq!(b["message"], "Assignment tidak ditemukan");
    let id_p2: u64 =
        sqlx::query_scalar("SELECT id FROM user_pekerjaan WHERE user_id = ? AND pekerjaan_id = ?")
            .bind(target)
            .bind(p2)
            .fetch_one(&pool)
            .await
            .unwrap();
    let (s, b) = send(
        &pool,
        Method::DELETE,
        &format!("/api/user-pekerjaan/{id_p2}"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["message"], "Assignment berhasil dihapus");
    let (s, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/user-pekerjaan/{id_p2}"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    purge(&pool, &[admin_email, plain_email, target_email], marker).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn by_user_and_by_pekerjaan_shapes() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let marker = "UJI-UP-shape";
    let admin_email = "uji-up-shape-admin@example.test";
    let user_email = "uji-up-shape-user@example.test";
    purge(&pool, &[admin_email, user_email], marker).await;

    let admin = make_user(&pool, admin_email, Some("admin")).await;
    let user = make_user(&pool, user_email, Some("pengawas")).await;
    let admin_tok = token_for(&pool, admin).await;
    let kegiatan = make_kegiatan(&pool, &format!("{marker} keg"), "2099").await;
    let p = make_pekerjaan(&pool, &format!("{marker} P"), Some(kegiatan)).await;
    sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(format!("{marker} kec"))
    .execute(&pool)
    .await
    .unwrap();
    let kec: u64 =
        sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec = ? ORDER BY id DESC LIMIT 1")
            .bind(format!("{marker} kec"))
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO tbl_desa (n_desa, luas, jumlah_penduduk, jumlah_kk, target, bjp_master, kecamatan_id, created_at, updated_at) VALUES (?, 12.5, 100, 30, 1, 0, ?, NOW(), NOW())")
        .bind(format!("{marker} desa"))
        .bind(kec)
        .execute(&pool)
        .await
        .unwrap();
    let desa: u64 =
        sqlx::query_scalar("SELECT id FROM tbl_desa WHERE n_desa = ? ORDER BY id DESC LIMIT 1")
            .bind(format!("{marker} desa"))
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE tbl_pekerjaan SET kecamatan_id = ?, desa_id = ? WHERE id = ?")
        .bind(kec)
        .bind(desa)
        .bind(p)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, '2026-01-02 03:04:05', '2026-01-02 03:04:05')")
        .bind(user)
        .bind(p)
        .execute(&pool)
        .await
        .unwrap();

    // byUser
    let (s, b) = send(
        &pool,
        Method::GET,
        &format!("/api/user-pekerjaan/user/{user}"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["data"]["user"]["email"], user_email);
    let item = &b["data"]["pekerjaan"][0];
    assert_eq!(item["id"], json!(p));
    assert_eq!(item["pagu"], json!(1500000));
    assert_eq!(item["kegiatan"]["tahun_anggaran"], "2099");
    assert_eq!(item["kecamatan"]["n_kec"], format!("{marker} kec"));
    assert_eq!(item["desa"]["n_desa"], format!("{marker} desa"));
    assert_eq!(item["desa"]["luas"], json!(12.5));
    assert_eq!(item["pivot"]["user_id"], json!(user));
    assert_eq!(item["pivot"]["created_at"], "2026-01-02 03:04:05");

    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/user/999999999",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(
        b["message"],
        format!(r"No query results for model [{MODEL_USER}] 999999999")
    );

    // byPekerjaan
    let (s, b) = send(
        &pool,
        Method::GET,
        &format!("/api/user-pekerjaan/pekerjaan/{p}"),
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["data"]["pekerjaan"]["nama_paket"], format!("{marker} P"));
    let users = b["data"]["users"].as_array().unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["email"], user_email);
    assert!(
        users[0].get("password").is_none(),
        "password tidak boleh bocor"
    );
    assert!(users[0].get("remember_token").is_none());
    assert_eq!(users[0]["pivot"]["pekerjaan_id"], json!(p));

    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/pekerjaan/999999999",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    purge(&pool, &[admin_email, user_email], marker).await;
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa = ?")
        .bind(format!("{marker} desa"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(format!("{marker} kec"))
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn available_users_excludes_admins() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let marker = "uji-up-avail";
    let req_email = "uji-up-avail-req@example.test";
    let admin_email = "uji-up-avail-admin@example.test";
    let plain_email = "uji-up-avail-plain@example.test";
    let emails = [req_email, admin_email, plain_email];
    purge(&pool, &emails, "UJI-UP-avail").await;

    let req = make_user(&pool, req_email, Some("admin")).await;
    let other_admin = make_user(&pool, admin_email, Some("admin")).await;
    let plain = make_user(&pool, plain_email, Some("pengawas")).await;
    let tok = token_for(&pool, req).await;

    let (s, b) = send(
        &pool,
        Method::GET,
        &format!("/api/user-pekerjaan/available-users?search={marker}"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let ids: Vec<u64> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![plain], "admin tidak boleh muncul");
    assert!(b["data"][0].get("password").is_none());
    assert!(!ids.contains(&other_admin));

    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/available-users",
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    purge(&pool, &emails, "UJI-UP-avail").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn completeness_gaps_scenarios() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let marker = "UJI-UP-gap";
    let admin_email = "uji-up-gap-admin@example.test";
    let user_email = "uji-up-gap-user@example.test";
    let emails = [admin_email, user_email];
    purge(&pool, &emails, marker).await;

    let admin = make_user(&pool, admin_email, Some("admin")).await;
    let user = make_user(&pool, user_email, Some("pengawas")).await;
    let admin_tok = token_for(&pool, admin).await;
    let plain_tok = token_for(&pool, user).await;
    let keg_2099 = make_kegiatan(&pool, &format!("{marker} k99"), "2099").await;
    let keg_2088 = make_kegiatan(&pool, &format!("{marker} k88"), "2088").await;

    // P_A: kosong semua. Gap foto dan progress.
    let p_a = make_pekerjaan(&pool, &format!("{marker} A"), Some(keg_2099)).await;
    // P_B: output 2 unit dengan 1 penerima. Gap foto, penerima (1/2), dan progress.
    let p_b = make_pekerjaan(&pool, &format!("{marker} B"), Some(keg_2099)).await;
    sqlx::query("INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, penerima_is_optional, created_at, updated_at) VALUES (?, 'K', 'unit', 2, 0, NOW(), NOW())")
        .bind(p_b)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_penerima (pekerjaan_id, nama, is_komunal, created_at, updated_at) VALUES (?, 'Warga', 0, NOW(), NOW())")
        .bind(p_b)
        .execute(&pool)
        .await
        .unwrap();
    // P_C: lengkap. Output 1 unit, 1 penerima, 5 foto, progres terisi. Tidak boleh muncul.
    let p_c = make_pekerjaan(&pool, &format!("{marker} C"), Some(keg_2099)).await;
    sqlx::query("INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, penerima_is_optional, created_at, updated_at) VALUES (?, 'K', 'unit', 1, 0, NOW(), NOW())")
        .bind(p_c)
        .execute(&pool)
        .await
        .unwrap();
    let out_c: u64 = sqlx::query_scalar("SELECT id FROM tbl_output WHERE pekerjaan_id = ?")
        .bind(p_c)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_penerima (pekerjaan_id, nama, is_komunal, created_at, updated_at) VALUES (?, 'Warga C', 0, NOW(), NOW())")
        .bind(p_c)
        .execute(&pool)
        .await
        .unwrap();
    let pen_c: u64 = sqlx::query_scalar("SELECT id FROM tbl_penerima WHERE pekerjaan_id = ?")
        .bind(p_c)
        .fetch_one(&pool)
        .await
        .unwrap();
    for _ in 0..5 {
        sqlx::query("INSERT INTO tbl_foto (pekerjaan_id, komponen_id, penerima_id, keterangan, koordinat, created_at, updated_at) VALUES (?, ?, ?, '0%', '0,0', NOW(), NOW())")
            .bind(p_c)
            .bind(out_c)
            .bind(pen_c)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(p_c)
        .bind(r#"{"items":[{"bobot":100,"target_volume":10,"weekly_data":{"1":{"realisasi":10}}}]}"#)
        .execute(&pool)
        .await
        .unwrap();
    // P_D: tahun anggaran 2088, tidak ikut filter tahun 2099.
    let p_d = make_pekerjaan(&pool, &format!("{marker} D"), Some(keg_2088)).await;

    for p in [p_a, p_b, p_c, p_d] {
        sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(user)
            .bind(p)
            .execute(&pool)
            .await
            .unwrap();
    }

    let find_user = |b: &Value| -> Value {
        b["data"]["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["user_id"] == json!(user))
            .cloned()
            .unwrap_or(Value::Null)
    };

    // Semua gap untuk tahun 2099: P_C tidak muncul.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/completeness-gaps?tahun=2099",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["status"], "success");
    let u = find_user(&b);
    assert_eq!(u["user_email"], user_email);
    let rows = u["pekerjaan"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{u}");
    let a = rows
        .iter()
        .find(|r| r["pekerjaan_id"] == json!(p_a))
        .unwrap();
    assert_eq!(a["gaps"], json!(["foto", "progress"]));
    assert_eq!(a["gap_details"]["foto"], "Belum ada foto dokumentasi");
    assert_eq!(
        a["gap_details"]["progress"],
        "Progress estimasi belum terinput"
    );
    let bb = rows
        .iter()
        .find(|r| r["pekerjaan_id"] == json!(p_b))
        .unwrap();
    assert_eq!(bb["gaps"], json!(["foto", "penerima", "progress"]));
    assert_eq!(bb["gap_details"]["penerima"], "Penerima 1/2 unit");
    assert_eq!(
        u["gap_counts"],
        json!({ "foto": 2, "penerima": 1, "progress": 2 })
    );

    // Filter gaps[]=progress: hanya progress yang dihitung.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/completeness-gaps?tahun=2099&gaps%5B%5D=progress",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let u = find_user(&b);
    let rows = u["pekerjaan"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["gaps"] == json!(["progress"])));
    assert_eq!(u["gap_counts"], json!({ "progress": 2 }));
    assert!(b["data"]["summary"]["by_gap"]["progress"].as_i64().unwrap() >= 2);

    // Tahun 2088: hanya P_D.
    let (s, b) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/completeness-gaps?tahun=2088",
        Some(&admin_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let u = find_user(&b);
    let rows = u["pekerjaan"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["pekerjaan_id"], json!(p_d));

    // Validasi: gaps skalar, gap tidak dikenal, tahun di luar rentang.
    for (uri, msg) in [
        (
            "/api/user-pekerjaan/completeness-gaps?gaps=foto",
            "The gaps field must be an array.",
        ),
        (
            "/api/user-pekerjaan/completeness-gaps?gaps%5B%5D=bogus",
            "The selected gaps.0 is invalid.",
        ),
        (
            "/api/user-pekerjaan/completeness-gaps?tahun=1999",
            "The tahun field must be at least 2000.",
        ),
        (
            "/api/user-pekerjaan/completeness-gaps?tahun=abc",
            "The tahun field must be an integer.",
        ),
    ] {
        let (s, b) = send(&pool, Method::GET, uri, Some(&admin_tok), None).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        let all: Vec<&str> = b["errors"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|v| v.as_array().unwrap().iter().map(|x| x.as_str().unwrap()))
            .collect();
        assert!(all.contains(&msg), "{uri}: {b}");
    }

    // Non-admin ditolak.
    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/user-pekerjaan/completeness-gaps?tahun=2099",
        Some(&plain_tok),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    purge(&pool, &emails, marker).await;
}
