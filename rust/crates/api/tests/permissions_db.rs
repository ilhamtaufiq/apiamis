//! Izin Spatie (`PermissionController`) lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test permissions_db -- --include-ignored
//! ```

use api::{app, auth_oauth::cache_prefix, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ADMIN: &str = "uji-perm-admin@example.test";
const USER: &str = "uji-perm-user@example.test";
const PREFIX: &str = "uji-perm-";
const ROLE: &str = "uji-perm-role";

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

/// Pengguna uji dengan token Sanctum. `admin` menentukan apakah ia punya peran admin.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Perm', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if admin {
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
    }
    auth::login::create_token(pool, uid, "uji-perm")
        .await
        .unwrap()
}

/// Bersihkan hanya baris uji (nama berawalan `uji-perm-`) beserta pivot-nya.
async fn clean(pool: &MySqlPool) {
    let like = format!("{PREFIX}%");
    sqlx::query("DELETE rhp FROM role_has_permissions rhp JOIN permissions p ON p.id = rhp.permission_id WHERE p.name LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE mhp FROM model_has_permissions mhp JOIN permissions p ON p.id = mhp.permission_id WHERE p.name LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM permissions WHERE name LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM roles WHERE name = ?")
        .bind(ROLE)
        .execute(pool)
        .await
        .unwrap();
}

async fn cache_key_rows(pool: &MySqlPool) -> i64 {
    let key = format!("{}spatie.permission.cache", cache_prefix());
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM `cache` WHERE `key` = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn permissions_crud_validates_searches_and_resets_cache() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    clean(&pool).await;
    let admin = user_token(&pool, ADMIN, true).await;
    let user = user_token(&pool, USER, false).await;

    // Tanpa token: 401. Pengguna biasa tanpa rule: 403 (admin-only, dari CheckRoutePermission).
    let (status, _) = send(&pool, Method::GET, "/api/permissions", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&pool, Method::GET, "/api/permissions", Some(&user), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Validasi store: wajib, harus string, unik.
    let (status, body) = send(&pool, Method::POST, "/api/permissions", Some(&admin), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");
    let (status, body) = send(&pool, Method::POST, "/api/permissions", Some(&admin), Some(json!({"name": "   "}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");
    let (status, body) = send(&pool, Method::POST, "/api/permissions", Some(&admin), Some(json!({"name": 5}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field must be a string.", "{body}");

    // Store 201, nama di-trim, guard `web`. Cache Spatie dihapus.
    // Tanda bahwa cache Spatie ada; store harus menghapusnya.
    sqlx::query("INSERT INTO `cache` (`key`, `value`, `expiration`) VALUES (?, 'x', 2000000000) ON DUPLICATE KEY UPDATE `value` = 'x', `expiration` = 2000000000")
        .bind(format!("{}spatie.permission.cache", cache_prefix()))
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/permissions",
        Some(&admin),
        Some(json!({"name": "  uji-perm-baca  "})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], "uji-perm-baca", "{body}");
    assert_eq!(body["guard_name"], "web", "{body}");
    let id = body["id"].as_i64().unwrap();
    assert_eq!(cache_key_rows(&pool).await, 0, "cache Spatie harus dihapus setelah store");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/permissions",
        Some(&admin),
        Some(json!({"name": "uji-perm-baca"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name has already been taken.", "{body}");

    // Index: paginator, filter `search` mengandung teks.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/permissions?search=uji-perm-ba",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"].is_array(), "{body}");
    assert!(body["links"].is_object(), "{body}");
    assert!(body["meta"]["total"].as_i64().unwrap() >= 1, "{body}");
    assert!(
        body["data"].as_array().unwrap().iter().any(|p| p["id"] == id),
        "{body}"
    );

    // Show 200, id tidak ada 404.
    let (status, body) = send(&pool, Method::GET, &format!("/api/permissions/{id}"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "uji-perm-baca", "{body}");
    let (status, _) = send(&pool, Method::GET, "/api/permissions/999999999", Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Update: wajib, unik kecuali id sendiri, PUT dan PATCH.
    let (status, body) = send(&pool, Method::POST, "/api/permissions", Some(&admin), Some(json!({"name": "uji-perm-lain"}))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(&pool, Method::PUT, &format!("/api/permissions/{id}"), Some(&admin), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/permissions/{id}"),
        Some(&admin),
        Some(json!({"name": "uji-perm-lain"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name has already been taken.", "{body}");

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/permissions/{id}"),
        Some(&admin),
        Some(json!({"name": "uji-perm-ubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "uji-perm-ubah", "{body}");
    // Nama sendiri tidak dihitung sebagai duplikat.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/permissions/{id}"),
        Some(&admin),
        Some(json!({"name": "uji-perm-ubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "uji-perm-ubah", "{body}");

    // Delete izin yang masih dipegang peran: pivot ikut terhapus, respons pesan Laravel.
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(ROLE)
        .execute(&pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
        .bind(ROLE)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO role_has_permissions (permission_id, role_id) VALUES (?, ?)")
        .bind(id)
        .bind(role)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = send(&pool, Method::DELETE, &format!("/api/permissions/{id}"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Permission deleted", "{body}");
    let left: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM role_has_permissions WHERE permission_id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "pivot peran harus ikut terhapus");
    assert_eq!(cache_key_rows(&pool).await, 0, "cache Spatie harus dihapus setelah destroy");

    let (status, _) = send(&pool, Method::GET, &format!("/api/permissions/{id}"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&pool, Method::DELETE, &format!("/api/permissions/{id}"), Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    clean(&pool).await;
}
