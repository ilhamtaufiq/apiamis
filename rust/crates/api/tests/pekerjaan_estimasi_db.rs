//! Progress estimasi per pekerjaan (GET dan PUT) lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_estimasi_db -- --include-ignored
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

const MARK: &str = "uji-pest-";

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
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "uji-agent");
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(builder.body(Body::from(body)).unwrap())
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

/// User baru dengan satu role; email unik per tes.
async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> u64 {
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
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let rid: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
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
    uid
}

async fn insert_pekerjaan(pool: &MySqlPool, nama: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, 1000, 1, 'active', NOW(), NOW())")
        .bind(format!("{MARK}{nama}"))
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn cleanup(pool: &MySqlPool, pekerjaan: &[u64], users: &[u64]) {
    for id in pekerjaan {
        sqlx::query("DELETE FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_pekerjaan WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    for u in users {
        sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
            .bind(u)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn payload_2025() -> Value {
    json!({
        "tahun": 2025,
        "fisik": {
            "rencana": [
                {"tanggal": "2025-03-01", "persen": 30},
                {"tanggal": "2025-06-01", "persen": "60,00"}
            ],
            "realisasi": [
                {"tanggal": "2025-06-01", "persen": "55,5"}
            ]
        },
        "keuangan": {
            "rencana": [
                {"tanggal": "2025-02-01", "persen": "20"}
            ],
            "realisasi": [
                {
                    "tanggal": "2025-05-01",
                    "persen": "40",
                    "nilai": 1000000,
                    "nomor_sp2d": "SP2D-UJI-1",
                    "tanggal_pembuatan": "2025-04-30",
                    "tanggal_pencairan": "2025-05-02"
                }
            ]
        }
    })
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn put_replaces_history_and_get_summarises_latest_and_deviasi() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = make_user(&pool, "uji-pest-a1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, admin, "uji-pest")
        .await
        .unwrap();
    let pid = insert_pekerjaan(&pool, "ringkasan").await;

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/pekerjaan/{pid}/progress-estimasi"),
        &token,
        Some(payload_2025()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["message"],
        "Riwayat progress estimasi berhasil disimpan"
    );
    assert_eq!(body["data"]["fisik"]["latest_rencana"], 60);
    assert_eq!(body["data"]["fisik"]["latest_realisasi"], 55.5);
    assert_eq!(body["data"]["fisik"]["deviasi"], -4.5);
    assert_eq!(body["data"]["keuangan"]["latest_rencana"], 20);
    assert_eq!(body["data"]["keuangan"]["latest_realisasi"], 40);
    assert_eq!(body["data"]["keuangan"]["deviasi"], 20);
    assert_eq!(body["data"]["keuangan"]["realisasi"][0]["nilai"], 1_000_000);
    assert_eq!(
        body["data"]["keuangan"]["realisasi"][0]["nomor_sp2d"],
        "SP2D-UJI-1"
    );
    assert_eq!(
        body["data"]["keuangan"]["realisasi"][0]["tanggal_pencairan"],
        "2025-05-02"
    );
    assert!(body["data"]["fisik"]["rencana"][0].get("nilai").is_some());
    assert!(
        body["data"]["fisik"]["rencana"][0]["nilai"].is_null(),
        "nilai hanya untuk keuangan realisasi"
    );
    assert!(body["data"]["updated_at"].is_string());

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/pekerjaan/{pid}/progress-estimasi?tahun=2025"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan_id"], pid);
    assert_eq!(body["data"]["tahun_anggaran"], 2025);
    assert_eq!(
        body["data"]["fisik"]["rencana"].as_array().unwrap().len(),
        2
    );
    assert_eq!(body["data"]["keuangan"]["latest_realisasi"], 40);

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/pekerjaan/{pid}/progress-estimasi"),
        &token,
        Some(
            json!({"tahun": 2025, "fisik": {"rencana": [{"tanggal": "2025-01-01", "persen": 5}]}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["fisik"]["latest_rencana"], 5);
    assert!(body["data"]["fisik"]["latest_realisasi"].is_null());
    assert!(body["data"]["fisik"]["deviasi"].is_null());
    assert_eq!(
        body["data"]["keuangan"]["rencana"],
        json!([]),
        "riwayat lama dihapus"
    );

    let (_, body) = send(
        &pool,
        Method::GET,
        &format!("/api/pekerjaan/{pid}/progress-estimasi?tahun=2024"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        body["data"]["fisik"]["rencana"],
        json!([]),
        "tahun lain kosong"
    );
    assert!(body["data"]["updated_at"].is_null());

    cleanup(&pool, &[pid], &[admin]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validation_errors_use_laravel_wording_and_nested_keys() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = make_user(&pool, "uji-pest-v1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, admin, "uji-pest")
        .await
        .unwrap();
    let pid = insert_pekerjaan(&pool, "validasi").await;
    let uri = format!("/api/pekerjaan/{pid}/progress-estimasi");

    let (status, body) = send(
        &pool,
        Method::PUT,
        &uri,
        &token,
        Some(json!({"tahun": 1999, "fisik": {"rencana": [{"tanggal": "2025-01-01", "persen": "101"}]}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["errors"]["tahun"][0],
        "The tahun field must be at least 2000."
    );
    assert_eq!(
        body["errors"]["fisik.rencana.0.persen"][0],
        "Field fisik.rencana.0.persen harus berada antara 0 dan 100."
    );

    let (_, body) = send(
        &pool,
        Method::PUT,
        &uri,
        &token,
        Some(json!({"tahun": 2025, "keuangan": {"realisasi": [{"tanggal": "2025-01-01", "persen": "12.345"}]}})),
    )
    .await;
    assert_eq!(
        body["errors"]["keuangan.realisasi.0.persen"][0],
        "Field keuangan.realisasi.0.persen harus berupa angka desimal dengan maksimal 2 angka di belakang koma."
    );

    let (_, body) = send(
        &pool,
        Method::PUT,
        &uri,
        &token,
        Some(json!({"tahun": 2025, "fisik": {"rencana": [{"persen": 10}]}})),
    )
    .await;
    assert_eq!(
        body["errors"]["fisik.rencana.0.tanggal"][0],
        "The fisik.rencana.0.tanggal field is required."
    );

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("{uri}?tahun=abc"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["tahun"][0],
        "The tahun field must be an integer."
    );
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("{uri}?tahun=2101"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    cleanup(&pool, &[pid], &[admin]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn out_of_scope_and_unknown_pekerjaan_return_404() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let user = make_user(&pool, "uji-pest-u1@example.test", "user").await;
    let token = auth::login::create_token(&pool, user, "uji-pest")
        .await
        .unwrap();
    let pid = insert_pekerjaan(&pool, "scope").await;
    let uri = format!("/api/pekerjaan/{pid}/progress-estimasi");

    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("{uri}?tahun=2025"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "tanpa assignment");
    let (status, _) = send(&pool, Method::PUT, &uri, &token, Some(payload_2025())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "tanpa assignment");

    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(user)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("{uri}?tahun=2025"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/pekerjaan/999999999/progress-estimasi",
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, &[pid], &[user]).await;
}
