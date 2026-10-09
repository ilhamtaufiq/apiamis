//! Izin menu (`MenuPermissionController`) lewat router terhadap MySQL.
//!
//! Setiap tes memakai awalan nama sendiri (`uji-mp-<tes>-`) dan email sendiri, sehingga tes yang
//! berjalan paralel tidak saling menghapus baris. Tabel `menu_permissions` dibuat dari
//! `rust/fixtures/menu_permissions_schema.sql`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test menu_permissions_db -- --include-ignored
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

const MODEL: &str = "App\\Models\\MenuPermission";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
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

/// Pengguna dengan role tertentu (boleh kosong). Mengembalikan `(id, token)`.
async fn user(pool: &MySqlPool, email: &str, roles: &[&str]) -> (u64, String) {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Menu', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    for role in roles {
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
        sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
            .bind(rid)
            .bind(uid)
            .execute(pool)
            .await
            .unwrap();
    }
    let token = auth::login::create_token(pool, uid, "uji-mp")
        .await
        .unwrap();
    (uid, token)
}

/// Hapus hanya baris milik awalan `p`: audit, menu, dan pengguna uji.
async fn cleanup(pool: &MySqlPool, p: &str) {
    let like = format!("{p}%");
    let user_like = format!("{p}%@example.test");
    sqlx::query(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND \
         (auditable_id IN (SELECT id FROM menu_permissions WHERE menu_key LIKE ?) \
          OR user_id IN (SELECT id FROM users WHERE email LIKE ?))",
    )
    .bind(MODEL)
    .bind(&like)
    .bind(&user_like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM menu_permissions WHERE menu_key LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email LIKE ?)",
    )
    .bind(&user_like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE ?")
        .bind(&user_like)
        .execute(pool)
        .await
        .unwrap();
}

/// Menu langsung lewat SQL. `roles` berupa JSON teks atau `None` (NULL). Mengembalikan id-nya.
async fn insert_menu(
    pool: &MySqlPool,
    key: &str,
    label: &str,
    roles: Option<&str>,
    active: bool,
) -> i64 {
    sqlx::query(
        "INSERT INTO menu_permissions (menu_key, menu_label, menu_parent, allowed_roles, is_active, created_at, updated_at) \
         VALUES (?, ?, NULL, ?, ?, NOW(), NOW())",
    )
    .bind(key)
    .bind(label)
    .bind(roles)
    .bind(active)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM menu_permissions WHERE menu_key = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn menu_label(pool: &MySqlPool, id: i64) -> String {
    sqlx::query_scalar("SELECT menu_label FROM menu_permissions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_key(pool: &MySqlPool, key: &str) -> i64 {
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM menu_permissions WHERE menu_key = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit_count(pool: &MySqlPool, id: i64, event: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = ?",
    )
    .bind(MODEL)
    .bind(id)
    .bind(event)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Apakah `key` ada dalam larik string `list` pada respons.
fn has_key(body: &Value, list: &str, key: &str) -> bool {
    body[list]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k.as_str() == Some(key))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn menu_permissions_requires_token() {
    let pool = pool().await;
    let p = "uji-mp-auth-";
    cleanup(&pool, p).await;

    // Tanpa token: index dan store 401. `user/menus` juga butuh login walaupun masuk whitelist.
    let (status, body) = send(&pool, Method::GET, "/api/menu-permissions", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        None,
        Some(json!({"menu_key": "uji-mp-auth-baru", "menu_label": "Baru"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(count_key(&pool, "uji-mp-auth-baru").await, 0);

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/menu-permissions/user/menus",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn non_admin_gets_403_on_index_store_and_update() {
    let pool = pool().await;
    let p = "uji-mp-acl-";
    cleanup(&pool, p).await;
    let (_admin_id, admin) = user(&pool, "uji-mp-acl-admin@example.test", &["admin"]).await;
    let (_uid, token) = user(&pool, "uji-mp-acl-tfl@example.test", &["tfl"]).await;
    let menu = insert_menu(&pool, "uji-mp-acl-menu", "Uji ACL", None, true).await;

    // Non-admin: index, store, dan update ditolak dengan 403 (menu-permissions admin-only).
    let (status, body) = send(&pool, Method::GET, "/api/menu-permissions", Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        Some(&token),
        Some(json!({"menu_key": "uji-mp-acl-baru", "menu_label": "Baru"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(count_key(&pool, "uji-mp-acl-baru").await, 0);

    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/menu-permissions/{menu}"),
        Some(&token),
        Some(json!({"menu_label": "Diubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(menu_label(&pool, menu).await, "Uji ACL");

    // Pembanding: admin boleh membaca daftar yang sama.
    let (status, body) = send(&pool, Method::GET, "/api/menu-permissions?search=uji-mp-acl-", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["meta"]["total"].as_i64().unwrap() >= 1, "{body}");

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn normal_user_reads_the_menus_it_may_access() {
    let pool = pool().await;
    let p = "uji-mp-me-";
    cleanup(&pool, p).await;
    let (_uid, token) = user(&pool, "uji-mp-me-tfl@example.test", &["tfl"]).await;
    insert_menu(&pool, "uji-mp-me-tfl", "Uji TFL", Some("[\"tfl\"]"), true).await;
    insert_menu(&pool, "uji-mp-me-admin", "Uji Admin", Some("[\"admin\"]"), true).await;
    insert_menu(&pool, "uji-mp-me-publik", "Uji Publik", None, true).await;
    insert_menu(&pool, "uji-mp-me-off", "Uji Nonaktif", Some("[\"tfl\"]"), false).await;

    // Login cukup; `user/menus` tidak butuh role admin.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/menu-permissions/user/menus",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Menu aktif terdaftar; menu nonaktif tidak.
    assert!(has_key(&body, "configured_menus", "uji-mp-me-tfl"), "{body}");
    assert!(has_key(&body, "configured_menus", "uji-mp-me-admin"), "{body}");
    assert!(has_key(&body, "configured_menus", "uji-mp-me-publik"), "{body}");
    assert!(!has_key(&body, "configured_menus", "uji-mp-me-off"), "{body}");

    // Yang boleh: role cocok atau tanpa batasan role. Role admin tidak cocok dengan user ini.
    assert!(has_key(&body, "allowed_menus", "uji-mp-me-tfl"), "{body}");
    assert!(has_key(&body, "allowed_menus", "uji-mp-me-publik"), "{body}");
    assert!(!has_key(&body, "allowed_menus", "uji-mp-me-admin"), "{body}");
    assert!(!has_key(&body, "allowed_menus", "uji-mp-me-off"), "{body}");

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_and_update_enforce_unique_key_and_required_label() {
    let pool = pool().await;
    let p = "uji-mp-val-";
    cleanup(&pool, p).await;
    let (_admin_id, admin) = user(&pool, "uji-mp-val-admin@example.test", &["admin"]).await;

    // Store tanpa field wajib: 422 dengan kedua pesan.
    let (status, body) = send(&pool, Method::POST, "/api/menu-permissions", Some(&admin), Some(json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "The given data was invalid.", "{body}");
    assert_eq!(body["errors"]["menu_key"][0], "The menu key field is required.", "{body}");
    assert_eq!(body["errors"]["menu_label"][0], "The menu label field is required.", "{body}");

    // Label hanya spasi dianggap kosong (TrimStrings + ConvertEmptyStringsToNull).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-satu", "menu_label": "   "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["menu_label"][0], "The menu label field is required.", "{body}");
    assert_eq!(count_key(&pool, "uji-mp-val-satu").await, 0);

    // Store 201 menghasilkan resource langsung (tanpa `data`).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-satu", "menu_label": "Satu"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["menu_key"], "uji-mp-val-satu", "{body}");
    assert_eq!(body["menu_label"], "Satu", "{body}");
    assert_eq!(body["is_active"], true, "{body}");
    let satu = body["id"].as_i64().unwrap();
    assert_eq!(audit_count(&pool, satu, "created").await, 1);

    // Kunci yang sama ditolak pada store.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-satu", "menu_label": "Duplikat"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["menu_key"][0], "The menu key has already been taken.", "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/menu-permissions",
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-dua", "menu_label": "Dua"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let dua = body["id"].as_i64().unwrap();

    // Update: kunci milik menu lain ditolak.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/menu-permissions/{dua}"),
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-satu"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["menu_key"][0], "The menu key has already been taken.", "{body}");

    // Update: label kosong (null setelah normalisasi) ditolak. Status 422 yang dipastikan di sini.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/menu-permissions/{dua}"),
        Some(&admin),
        Some(json!({"menu_label": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["menu_label"].is_array(), "{body}");
    assert_eq!(menu_label(&pool, dua).await, "Dua");

    // Kunci sendiri tidak dihitung duplikat: PUT dan PATCH sama-sama 200.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/menu-permissions/{dua}"),
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-dua", "menu_label": "Dua ubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["menu_label"], "Dua ubah", "{body}");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/menu-permissions/{dua}"),
        Some(&admin),
        Some(json!({"menu_key": "uji-mp-val-dua"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["menu_key"], "uji-mp-val-dua", "{body}");
    assert_eq!(audit_count(&pool, dua, "updated").await, 1);

    cleanup(&pool, p).await;
}
