//! Update Pekerjaan lewat router terhadap MySQL: perubahan kolom, audit, dan notifikasi admin.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test pekerjaan_write_db -- --ignored
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

const ACTOR: &str = "uji-write-actor@example.test";
const OTHER_ADMIN: &str = "uji-write-admin2@example.test";

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
    body: Value,
) -> (StatusCode, Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::USER_AGENT, "uji-agent")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
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

async fn make_admin(pool: &MySqlPool, email: &str) -> u64 {
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
async fn update_changes_columns_and_writes_audit_and_admin_notifications() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let actor = make_admin(&pool, ACTOR).await;
    let other = make_admin(&pool, OTHER_ADMIN).await;
    let token = auth::login::create_token(&pool, actor, "uji-write")
        .await
        .unwrap();

    let row = sqlx::query(
        "SELECT id, kecamatan_id, desa_id FROM tbl_pekerjaan \
         WHERE is_konsultan = 0 AND kecamatan_id IS NOT NULL AND desa_id IS NOT NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let pid: u64 = row.try_get("id").unwrap();
    let kec: i64 = row.try_get("kecamatan_id").unwrap();
    let desa: i64 = row.try_get("desa_id").unwrap();
    let before: Option<String> =
        sqlx::query_scalar("SELECT nama_paket FROM tbl_pekerjaan WHERE id = ?")
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ? AND user_id = ?")
        .bind(pid)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM notifications WHERE notifiable_id = ? AND data LIKE ?")
        .bind(other)
        .bind(format!("%ID #{pid} %"))
        .execute(&pool)
        .await
        .unwrap();

    let uji_name = format!(
        "Uji Update {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/pekerjaan/{pid}"),
        &token,
        json!({ "nama_paket": uji_name }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["nama_paket"], uji_name.as_str());

    let audit = sqlx::query(
        "SELECT event, user_agent, CAST(old_values AS CHAR) AS old_values, CAST(new_values AS CHAR) AS new_values FROM tbl_audit_logs \
         WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ? AND user_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(pid)
    .bind(actor)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.try_get::<String, _>("event").unwrap(), "updated");
    assert_eq!(
        audit.try_get::<String, _>("user_agent").unwrap(),
        "uji-agent"
    );
    let new: Value =
        serde_json::from_str(&audit.try_get::<String, _>("new_values").unwrap()).unwrap();
    assert_eq!(new["nama_paket"], uji_name.as_str());

    let other_notes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND notifiable_type = 'App\\\\Models\\\\User' AND data LIKE ?",
    )
    .bind(other)
    .bind(format!("%ID #{pid} %"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(other_notes, 1, "admin lain menerima notifikasi");

    let actor_notes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND data LIKE ?",
    )
    .bind(actor)
    .bind(format!("%ID #{pid} %"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor_notes, 0, "pelaku tidak diberi notifikasi");

    // Non-konsultan tanpa kecamatan dan desa: 422 dan tidak ada yang berubah.
    sqlx::query("UPDATE tbl_pekerjaan SET kecamatan_id = NULL, desa_id = NULL WHERE id = ?")
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/pekerjaan/{pid}"),
        &token,
        json!({ "is_konsultan": false }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["desa_id"].is_array());
    sqlx::query("UPDATE tbl_pekerjaan SET kecamatan_id = ?, desa_id = ? WHERE id = ?")
        .bind(kec)
        .bind(desa)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();

    // Kembalikan nama semula.
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/pekerjaan/{pid}"),
        &token,
        json!({ "nama_paket": before }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
