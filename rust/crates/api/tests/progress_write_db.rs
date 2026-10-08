//! Progres pekerjaan lewat router terhadap MySQL: laporan (dengan baris default), simpan, total, dan audit.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test progress_write_db -- --include-ignored
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

const ADMIN: &str = "uji-prg-admin@example.test";

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
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn progress_report_store_totals_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Progres', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    let admin: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, admin, "uji-prg")
        .await
        .unwrap();

    // Pekerjaan tanpa baris progres, supaya laporan pertama membuat baris default.
    let pekerjaan: u64 = sqlx::query_scalar(
        "SELECT p.id FROM tbl_pekerjaan p WHERE NOT EXISTS (SELECT 1 FROM tbl_progress t WHERE t.pekerjaan_id = p.id) ORDER BY p.id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Laporan pertama: baris default dibuat, audit created.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true, "{body}");
    assert_eq!(body["data"]["items"], json!([]), "{body}");
    assert_eq!(body["data"]["max_minggu"], 4, "{body}");
    let prg: u64 = sqlx::query_scalar("SELECT id FROM tbl_progress WHERE pekerjaan_id = ?")
        .bind(pekerjaan)
        .fetch_one(&pool)
        .await
        .unwrap();
    let events = |pool: MySqlPool| async move {
        sqlx::query_scalar::<_, String>("SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Progress' AND auditable_id = ? ORDER BY id")
            .bind(prg)
            .fetch_all(&pool)
            .await
            .unwrap()
    };
    assert_eq!(events(pool.clone()).await, vec!["created"]);

    // Validasi: satuan wajib untuk setiap item, dan items harus ada.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        Some(json!({ "items": [{ "nama_item": "Galian" }], "week_count": 4 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["items.0.satuan"].is_array(), "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        Some(json!({ "week_count": 4 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["items"].is_array(), "{body}");

    // Simpan satu item: bobot 50, target volume 10, realisasi minggu 1 dan 2 (7 total).
    let payload = json!({
        "items": [{
            "nama_item": "Galian",
            "satuan": "m3",
            "harga_satuan": 100,
            "bobot": 50,
            "target_volume": 10,
            "weekly_data": { "1": { "realisasi": 4 }, "2": { "realisasi": 3 } },
            "kolom_tidak_dikenal": "dibuang"
        }],
        "week_count": 4
    });
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Progress berhasil disimpan", "{body}");
    assert!(
        body["data"]["items"][0]
            .get("kolom_tidak_dikenal")
            .is_none(),
        "{body}"
    );
    assert_eq!(events(pool.clone()).await, vec!["created", "updated"]);

    // Laporan: total bobot 50, realisasi 7, progres tertimbang 35 (7/10 × 50).
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["totals"]["total_bobot"], 50.0, "{body}");
    assert_eq!(
        body["data"]["totals"]["total_accumulated_real"], 7.0,
        "{body}"
    );
    assert_eq!(
        body["data"]["totals"]["total_weighted_progress"], 35.0,
        "{body}"
    );
    assert_eq!(body["data"]["max_minggu"], 4, "{body}");

    // Simpan dengan isi yang sama: tidak ada audit baru.
    let (status, _) = send(
        &pool,
        Method::POST,
        &format!("/api/progress/pekerjaan/{pekerjaan}"),
        &token,
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(events(pool.clone()).await, vec!["created", "updated"]);

    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Progress' AND auditable_id = ?").bind(prg).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_progress WHERE id = ?")
        .bind(prg)
        .execute(&pool)
        .await
        .unwrap();
}
