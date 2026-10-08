//! CRUD output lewat router terhadap MySQL: audit, summary, dan volume dua desimal.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test output_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ACTOR: &str = "uji-output-admin@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(req)
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

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Output', ?, 'x', NOW(), NOW())")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ACTOR)
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
    uid
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn output_crud_summary_and_audit() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-output")
        .await
        .unwrap();
    let pid: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let komponen = format!("UJI-OUT-{}", std::process::id());

    // Validasi: volume negatif ditolak.
    let (status, err) = send(
        &pool,
        Method::POST,
        "/api/output",
        &token,
        Some(json!({
            "pekerjaan_id": pid, "komponen": komponen, "satuan": "m", "volume": -1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");

    let (status, created) = send(&pool, Method::POST, "/api/output", &token, Some(json!({
        "pekerjaan_id": pid, "komponen": komponen, "satuan": "m", "volume": 12.5, "penerima_is_optional": true
    }))).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let d = &created["data"];
    let id = d["id"].as_i64().unwrap();
    assert_eq!(d["volume"], "12.50", "decimal dua desimal");
    assert_eq!(d["penerima_is_optional"], true);
    assert_eq!(d["pekerjaan"]["id"], pid);
    assert!(d["created_at"].as_str().unwrap().ends_with("+00:00"));

    let (status, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/output/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown["data"]["komponen"], komponen.as_str());

    // Update volume: audit updated hanya memuat kolom yang berubah.
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/output/{id}"),
        &token,
        Some(json!({ "volume": 20 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["volume"], "20.00");
    let new_values: String = sqlx::query_scalar("SELECT CAST(new_values AS CHAR) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Output' AND auditable_id = ? AND event = 'updated' ORDER BY id DESC LIMIT 1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let nv: Value = serde_json::from_str(&new_values).unwrap();
    assert!(nv.get("volume").is_some());
    assert!(
        nv.get("komponen").is_none(),
        "kolom yang tidak berubah tidak masuk audit"
    );

    // Summary: komponen ini muncul di rekap dengan total volume.
    let (status, summary) = send(&pool, Method::GET, "/api/output/summary", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(summary["total_output"].as_i64().unwrap() >= 1);
    let rekap = summary["rekap"].as_array().unwrap();
    assert!(rekap
        .iter()
        .any(|g| g["komponen"] == komponen.as_str() && g["satuan"] == "m"));

    // Daftar: per_page=-1 dengan pekerjaan_id mengembalikan semua tanpa paginasi.
    let (status, list) = send(
        &pool,
        Method::GET,
        &format!("/api/output?pekerjaan_id={pid}&per_page=-1"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["id"] == id));
    assert!(list.get("meta").is_none());

    // Hapus dan audit deleted.
    let (status, msg) = send(
        &pool,
        Method::DELETE,
        &format!("/api/output/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(msg["message"], "Output deleted successfully");
    let deleted: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Output' AND auditable_id = ? AND event = 'deleted'")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(deleted, 1);
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/output/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
