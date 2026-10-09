//! Sinkron rule permission (`POST /api/route-permissions/sync`) lewat router terhadap MySQL.
//!
//! Tes ini tidak memakai `prefix` default `api` karena akan membuat rule untuk ratusan rute di
//! database bersama. Semua pemindaian memakai `api/route-permissions`, dengan 10 pasangan
//! path + method. Baris yang dibuat tes dihapus lagi dengan membandingkan id sebelum dan sesudah.
//! Pengguna uji memakai email `uji-rps-`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test route_permissions_sync_db -- --include-ignored
//! ```

use std::collections::BTreeSet;

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-rps-admin@example.test";
const PENGAWAS: &str = "uji-rps-pengawas@example.test";
const SYNC_URI: &str = "/api/route-permissions/sync";
const SCOPE: &str = "api/route-permissions";
/// Pasangan path + method di bawah `api/route-permissions` (lihat `ROUTES`).
const SCOPE_RULES: i64 = 10;

/// Tes di file ini memakai baris tabel yang sama, jadi dijalankan satu per satu.
static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> (std::sync::MutexGuard<'static, ()>, MySqlPool) {
    let guard = DB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    (guard, MySqlPool::connect(&url).await.unwrap())
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

/// Pengguna uji dengan satu peran dan token Sanctum.
async fn user_token(pool: &MySqlPool, email: &str, role: &str) -> String {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji RPS', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
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
    auth::login::create_token(pool, uid, "uji-rps")
        .await
        .unwrap()
}

async fn drop_users(pool: &MySqlPool) {
    for email in [ADMIN, PENGAWAS] {
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
    }
}

async fn rp_ids(pool: &MySqlPool) -> BTreeSet<u64> {
    sqlx::query_scalar::<_, u64>("SELECT id FROM route_permissions")
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

async fn count_scope(pool: &MySqlPool) -> i64 {
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM route_permissions WHERE route_path LIKE '/route-permissions%'")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus baris yang dibuat tes (id yang tidak ada di `before`) beserta audit `created`-nya.
async fn remove_created(pool: &MySqlPool, before: &BTreeSet<u64>) {
    let now = rp_ids(pool).await;
    for id in now.difference(before) {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\RoutePermission' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM route_permissions WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sync_is_admin_only() {
    let (_guard, pool) = pool().await;
    let token = user_token(&pool, PENGAWAS, "pengawas").await;
    let (status, _) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "uji-rps-" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    drop_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sync_validates_input_before_writing() {
    let (_guard, pool) = pool().await;
    let token = user_token(&pool, ADMIN, "admin").await;
    let before = rp_ids(&pool).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "a".repeat(51) })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["prefix"].is_array(), "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "uji-rps-", "default_role": "uji-rps-tidak-ada" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The selected default role is invalid.");

    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "uji-rps-", "clean": "maybe" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["errors"]["clean"].is_array(), "{body}");

    assert_eq!(
        rp_ids(&pool).await,
        before,
        "validasi gagal tidak boleh menulis"
    );
    drop_users(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sync_prefix_without_routes_scans_nothing() {
    let (_guard, pool) = pool().await;
    let token = user_token(&pool, ADMIN, "admin").await;
    let before = rp_ids(&pool).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "uji-rps-" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Route permission berhasil disinkronkan.");
    assert_eq!(
        body["data"],
        json!({
            "scanned": 0,
            "created": 0,
            "removed": 0,
            "prefix": "uji-rps-",
            "default_role": "admin",
        })
    );
    assert_eq!(rp_ids(&pool).await, before);
    drop_users(&pool).await;
}

/// Satu alur berurutan agar tidak berebut baris dengan tes lain.
#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn sync_creates_rules_once_and_keeps_existing() {
    let (_guard, pool) = pool().await;
    let token = user_token(&pool, ADMIN, "admin").await;
    let before = rp_ids(&pool).await;
    let scope_before = count_scope(&pool).await;

    // Pertama: rule baru untuk setiap pasangan yang belum ada.
    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": SCOPE })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["scanned"], SCOPE_RULES);
    assert_eq!(body["data"]["prefix"], SCOPE);
    assert_eq!(body["data"]["default_role"], "admin");
    let created = body["data"]["created"].as_i64().unwrap();
    assert_eq!(created, SCOPE_RULES - scope_before.min(SCOPE_RULES));
    assert_eq!(body["data"]["removed"], 0);

    let new_ids: BTreeSet<u64> = rp_ids(&pool).await.difference(&before).copied().collect();
    assert_eq!(new_ids.len() as i64, created);

    for id in &new_ids {
        let (path, method, description, roles, active): (String, String, String, String, i64) =
            sqlx::query_as("SELECT route_path, route_method, description, CAST(allowed_roles AS CHAR), CAST(is_active AS SIGNED) FROM route_permissions WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(path.starts_with("/route-permissions"), "{path}");
        assert!(["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()));
        assert!(
            description.starts_with("Auto generated for "),
            "{description}"
        );
        assert_eq!(roles, r#"["admin"]"#);
        assert_eq!(active, 1);
    }

    // Deskripsi memakai path bila rute tanpa nama.
    let (sync_id, description): (u64, String) = sqlx::query_as(
        "SELECT id, description FROM route_permissions WHERE route_path = '/route-permissions/sync' AND route_method = 'POST'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    if new_ids.contains(&sync_id) {
        assert_eq!(description, "Auto generated for /route-permissions/sync");
    }

    // Setiap baris yang dibuat punya satu audit `created`.
    let mut audits = 0;
    for id in &new_ids {
        let n: i64 = sqlx::query_scalar(
            "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE event = 'created' AND auditable_type = 'App\\\\Models\\\\RoutePermission' AND auditable_id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        audits += n;
    }
    assert_eq!(audits, created);

    // Kedua: tidak ada yang dibuat ulang.
    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": SCOPE })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["scanned"], SCOPE_RULES);
    assert_eq!(body["data"]["created"], 0);
    let after_second = rp_ids(&pool).await;
    assert_eq!(
        after_second.difference(&before).count() as i64,
        created,
        "sinkron kedua tidak boleh menambah baris"
    );

    // Ketiga: rule yang sudah ada tidak diubah, meski default_role berbeda.
    let (status, body) = send(
        &pool,
        Method::POST,
        SYNC_URI,
        Some(&token),
        Some(json!({ "prefix": "api/route-permissions/sync", "default_role": "pengawas" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["scanned"], 1);
    assert_eq!(body["data"]["created"], 0);
    let roles: String = sqlx::query_scalar(
        "SELECT CAST(allowed_roles AS CHAR) FROM route_permissions WHERE route_path = '/route-permissions/sync' AND route_method = 'POST'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        roles, r#"["admin"]"#,
        "rule yang sudah ada tidak boleh diubah"
    );

    remove_created(&pool, &before).await;
    assert_eq!(rp_ids(&pool).await, before, "hanya baris uji yang dihapus");
    drop_users(&pool).await;
}
