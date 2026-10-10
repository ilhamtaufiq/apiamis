//! Dashboard progres MVP lewat router terhadap MySQL (cakupan admin dan pengawas).
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test dashboard_progres_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-prog-admin@example.test";
const PENGAWAS: &str = "uji-prog-pengawas@example.test";

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

async fn user_token(pool: &MySqlPool, email: &str, role: Option<&str>) -> String {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Peri', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if let Some(name) = role {
        sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        let role_id: u64 = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1",
        )
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
            .bind(role_id)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    auth::login::create_token(pool, uid, "uji-peri")
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn progres_mvp_cakupan_admin_dan_pengawas() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let admin = user_token(&pool, ADMIN, Some("admin")).await;
    let pengawas = user_token(&pool, PENGAWAS, Some("pengawas")).await;
    let pengawas_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(PENGAWAS)
        .fetch_one(&pool)
        .await
        .unwrap();
    let pekerjaan: i64 = sqlx::query_scalar("SELECT CAST(MIN(id) AS SIGNED) FROM tbl_pekerjaan")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pengawas_id)
        .bind(pekerjaan)
        .execute(&pool)
        .await
        .unwrap();
    // Penugasan pengawas untuk penilaian dan peringkat: pekerjaan.pengawas_id → tabel pengawas.
    let original_pengawas: Option<u64> =
        sqlx::query_scalar("SELECT pengawas_id FROM tbl_pekerjaan WHERE id = ?")
            .bind(pekerjaan)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO pengawas (nama, created_at, updated_at) VALUES ('Uji Pengawas Progres', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let master: u64 = sqlx::query_scalar(
        "SELECT CAST(MAX(id) AS UNSIGNED) FROM pengawas WHERE nama = 'Uji Pengawas Progres'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE tbl_pekerjaan SET pengawas_id = ? WHERE id = ?")
        .bind(master)
        .bind(pekerjaan)
        .execute(&pool)
        .await
        .unwrap();

    // Admin: cakupan penuh, struktur respons lengkap.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/dashboard/progres-mvp",
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    assert!(
        data["kpi"]["total_pekerjaan"].as_i64().unwrap() >= 1,
        "{body}"
    );
    assert!(
        data["pengawas"]["pengawas"]["aktif"].as_i64().unwrap() >= 1,
        "{body}"
    );
    assert!(data["pengawas"]["konsultan_pengawas"].is_object(), "{body}");
    assert!(data["per_kecamatan"].is_array(), "{body}");
    assert!(data["per_pengawas"].is_array(), "{body}");

    // Pengawas: hanya pekerjaan yang ditugaskan, jadi total = 1.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/dashboard/progres-mvp",
        Some(&pengawas),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["kpi"]["total_pekerjaan"], 1, "{body}");
    assert_eq!(
        body["data"]["pengawas"]["pengawas"]["pekerjaan_diawasi"], 1,
        "{body}"
    );

    // Filter tahun yang tidak ada: kosong, bukan error.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/dashboard/progres-mvp?tahun=1900",
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["kpi"]["total_pekerjaan"], 0, "{body}");

    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ? AND pekerjaan_id = ?")
        .bind(pengawas_id)
        .bind(pekerjaan)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE tbl_pekerjaan SET pengawas_id = ? WHERE id = ?")
        .bind(original_pengawas)
        .bind(pekerjaan)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM pengawas WHERE id = ?")
        .bind(master)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn penilaian_pengawas_bentuk_respons() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let admin = user_token(&pool, "uji-nilai-admin@example.test", Some("admin")).await;

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/dashboard/penilaian-pengawas",
        Some(&admin),
        Body::empty(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    let parameter = data["parameter"].as_array().unwrap();
    assert_eq!(parameter.len(), 6, "{body}");
    let bobot: f64 = parameter.iter().map(|p| p["bobot"].as_f64().unwrap()).sum();
    assert!((bobot - 100.0).abs() < 1e-9, "bobot harus 100: {body}");
    assert!(data["pengawas"].is_array(), "{body}");
    for orang in data["pengawas"].as_array().unwrap() {
        assert!(orang["breakdown"].is_object(), "{body}");
        assert!(orang["kategori"].is_string(), "{body}");
    }
}
