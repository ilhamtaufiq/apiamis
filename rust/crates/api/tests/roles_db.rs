//! Peran dan izin (`RoleController`) lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test roles_db -- --include-ignored
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

const ADMIN: &str = "uji-role-admin@example.test";
const ROLE_PREFIX: &str = "uji-role-";
const PERM: &str = "uji-role-perm";

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

/// User admin uji dengan token Sanctum.
async fn admin_token(pool: &MySqlPool) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Role', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
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
    auth::login::create_token(pool, uid, "uji-role")
        .await
        .unwrap()
}

/// Bersihkan peran uji beserta relasinya, dan pastikan izin uji ada.
async fn prepare(pool: &MySqlPool) {
    sqlx::query(
        "DELETE rhp FROM role_has_permissions rhp JOIN roles r ON r.id = rhp.role_id WHERE r.name LIKE ?",
    )
    .bind(format!("{ROLE_PREFIX}%"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM roles WHERE name LIKE ?")
        .bind(format!("{ROLE_PREFIX}%"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO permissions (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(PERM)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn roles_crud_validates_and_syncs_permissions() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    prepare(&pool).await;
    let token = admin_token(&pool).await;

    // Tanpa token: 401.
    let (status, _) = send(&pool, Method::GET, "/api/roles", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Validasi: name wajib, permissions harus array.
    let (status, body) = send(&pool, Method::POST, "/api/roles", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name field is required.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/roles",
        Some(&token),
        Some(json!({ "name": format!("{ROLE_PREFIX}a"), "permissions": "bukan-array" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The permissions field must be an array.", "{body}");

    // Simpan dengan izin: 201, izin ikut dengan `pivot`.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/roles",
        Some(&token),
        Some(json!({ "name": format!("{ROLE_PREFIX}a"), "permissions": [PERM] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], format!("{ROLE_PREFIX}a"));
    assert_eq!(body["guard_name"], "web");
    assert_eq!(body["permissions"][0]["name"], PERM);
    assert_eq!(body["permissions"][0]["pivot"]["role_id"], body["id"]);
    let id = body["id"].as_i64().unwrap();

    // Nama sama: ditolak.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/roles",
        Some(&token),
        Some(json!({ "name": format!("{ROLE_PREFIX}a") })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The name has already been taken.", "{body}");

    // Izin tidak dikenal: 500 dengan pesan Spatie.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/roles",
        Some(&token),
        Some(json!({ "name": format!("{ROLE_PREFIX}b"), "permissions": ["tidak-ada-izin"] })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(
        body["message"],
        "There is no permission named `tidak-ada-izin` for guard `web`.",
        "{body}"
    );

    // Daftar dengan pencarian, lalu detail.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/roles?search={ROLE_PREFIX}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["per_page"], 15);
    // `uji-role-b` tetap ada: sama dengan Laravel, peran dibuat dulu lalu sync izin gagal.
    assert_eq!(body["data"].as_array().unwrap().len(), 2, "{body}");

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/roles/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["permissions"].as_array().unwrap().len(), 1);

    // PATCH hanya nama: izin tetap. PUT dengan permissions kosong: izin dihapus.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/roles/{id}"),
        Some(&token),
        Some(json!({ "name": format!("{ROLE_PREFIX}a2") })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], format!("{ROLE_PREFIX}a2"));
    assert_eq!(body["permissions"].as_array().unwrap().len(), 1);

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/roles/{id}"),
        Some(&token),
        Some(json!({ "permissions": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["permissions"], json!([]));

    // Hapus, lalu detail: 404.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/roles/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "message": "Role deleted" }));
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/roles/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    prepare(&pool).await;
}
