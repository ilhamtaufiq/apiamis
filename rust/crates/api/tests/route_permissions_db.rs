//! Rute permission (`RoutePermissionController`) lewat router terhadap MySQL.
//!
//! Semua baris uji memakai path berawalan `/uji-rp-` supaya tidak mengganggu rule lain.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test route_permissions_db -- --include-ignored
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

const ADMIN_CRUD: &str = "uji-rp-admin-crud@example.test";
const ADMIN_CHK: &str = "uji-rp-admin-chk@example.test";
const PENGAWAS_CHK: &str = "uji-rp-pengawas-chk@example.test";

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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji RP', ?, 'x', NOW(), NOW())")
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
    auth::login::create_token(pool, uid, "uji-rp")
        .await
        .unwrap()
}

/// Hapus baris uji milik prefix path ini, beserta audit-nya.
async fn cleanup(pool: &MySqlPool, prefix: &str) {
    let ids: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM route_permissions WHERE route_path LIKE ?")
            .bind(format!("{prefix}%"))
            .fetch_all(pool)
            .await
            .unwrap();
    for id in &ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\RoutePermission' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM route_permissions WHERE route_path LIKE ?")
        .bind(format!("{prefix}%"))
        .execute(pool)
        .await
        .unwrap();
}

/// Nilai `data` dari respons, baik array biasa maupun objek ber-kunci (`accessible`).
fn values(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o.values().cloned().collect(),
        _ => Vec::new(),
    }
}

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn route_permissions_crud_validates_and_audits() {
    let pool = pool().await;
    cleanup(&pool, "/uji-rp-crud").await;
    let admin = user_token(&pool, ADMIN_CRUD, "admin").await;

    // Tanpa token: 401.
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/rules",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Create: description di-trim, is_active tidak dikirim sehingga tidak ada di respons.
    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({
            "route_path": "/uji-rp-crud/:id",
            "route_method": "GET",
            "description": "  uji  ",
            "allowed_roles": ["pengawas"],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["description"], "uji");
    assert_eq!(created["allowed_roles"], json!(["pengawas"]));
    assert!(created.get("is_active").is_none());
    let id1 = created["id"].as_u64().unwrap();

    // Duplikat path + method: 422 dengan pesan khusus.
    let (status, dup) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({
            "route_path": "/uji-rp-crud/:id",
            "route_method": "GET",
            "allowed_roles": ["admin"],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        dup["message"],
        "Route permission already exists for this path and method"
    );

    // Validasi.
    let (status, v) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({ "route_method": "TRACE", "allowed_roles": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "The route path field is required.");

    let (status, v) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/x", "route_method": "get", "allowed_roles": ["admin"] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        v["errors"]["route_method"][0],
        "The selected route method is invalid."
    );

    let (status, v) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/y", "route_method": "POST", "allowed_roles": ["role-tidak-ada"] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "The selected allowed roles.0 is invalid.");

    let (status, v) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(
            json!({ "route_path": "/uji-rp-crud/z", "route_method": "POST", "allowed_roles": [] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "The allowed roles field is required.");

    // Rule kedua: nonaktif saat dibuat, tanpa description.
    let (status, second) = send(
        &pool,
        Method::POST,
        "/api/route-permissions",
        Some(&admin),
        Some(json!({
            "route_path": "/uji-rp-crud/w",
            "route_method": "PATCH",
            "allowed_roles": ["admin"],
            "is_active": false,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(second["is_active"], false);
    assert!(second.get("description").is_none());
    let id2 = second["id"].as_u64().unwrap();

    // Show, termasuk 404 untuk id tidak ada dan id bukan angka.
    let (status, shown) = send(
        &pool,
        Method::GET,
        &format!("/api/route-permissions/{id1}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown["route_path"], "/uji-rp-crud/:id");
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/999999999",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/abc",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // check-access: pola `:id` cocok, rule kosong ditolak untuk admin tanpa peran pengawas.
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/77", "route_method": "GET" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["allowed"], false);
    assert_eq!(c["message"], "Access denied");
    assert_eq!(c["allowed_roles"], json!(["pengawas"]));
    assert_eq!(c["user_roles"], json!(["admin"]));

    // Method PATCH: rule exact nonaktif, jadi diabaikan.
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/w", "route_method": "PATCH" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["message"], "No restrictions for this route");

    // Default GET: rule PATCH tidak berlaku, tetapi pola `/uji-rp-crud/:id` (GET, aktif) cocok.
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/w" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["allowed"], false);
    assert_eq!(c["allowed_roles"], json!(["pengawas"]));

    // Tanpa rule untuk method DELETE.
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/w", "route_method": "DELETE" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["message"], "No restrictions for this route");

    // Index: pencarian, filter method, filter is_active, dan per_page=-1.
    let (status, page) = send(
        &pool,
        Method::GET,
        "/api/route-permissions?search=uji-rp-crud&per_page=1",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["data"].as_array().unwrap().len(), 1);
    assert_eq!(page["meta"]["per_page"], 1);
    assert_eq!(page["meta"]["total"], 2);

    let (_, all) = send(
        &pool,
        Method::GET,
        "/api/route-permissions?search=uji-rp-crud&per_page=-1",
        Some(&admin),
        None,
    )
    .await;
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["id"], id1);

    let (_, patch_only) = send(
        &pool,
        Method::GET,
        "/api/route-permissions?search=uji-rp-crud&method=patch",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(patch_only["data"][0]["id"], id2);
    assert_eq!(patch_only["meta"]["total"], 1);

    let (_, inactive) = send(
        &pool,
        Method::GET,
        "/api/route-permissions?search=uji-rp-crud&is_active=0",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(inactive["data"][0]["id"], id2);

    // Rules: hanya yang aktif.
    let (status, rules) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/rules",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let paths: Vec<&str> = rules
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["route_path"].as_str())
        .collect();
    assert!(paths.contains(&"/uji-rp-crud/:id"));
    assert!(
        !paths.contains(&"/uji-rp-crud/w"),
        "rule nonaktif tidak boleh muncul"
    );

    // Update: PATCH mengubah is_active, PUT tanpa perubahan tidak menulis audit.
    let (status, up) = send(
        &pool,
        Method::PATCH,
        &format!("/api/route-permissions/{id1}"),
        Some(&admin),
        Some(json!({ "is_active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(up["is_active"], false);
    assert_eq!(up["route_path"], "/uji-rp-crud/:id");

    let (status, same) = send(
        &pool,
        Method::PUT,
        &format!("/api/route-permissions/{id1}"),
        Some(&admin),
        Some(json!({ "is_active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(same["is_active"], false);

    let (status, v) = send(
        &pool,
        Method::PUT,
        &format!("/api/route-permissions/{id1}"),
        Some(&admin),
        Some(json!({ "route_path": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["message"], "The route path field must be a string.");

    // Aktifkan rule PATCH id2: admin boleh.
    let (status, _) = send(
        &pool,
        Method::PATCH,
        &format!("/api/route-permissions/{id2}"),
        Some(&admin),
        Some(json!({ "is_active": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-crud/w", "route_method": "PATCH" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["allowed"], true);
    assert_eq!(c["message"], "Access granted");

    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\RoutePermission' AND auditable_id = ? ORDER BY id",
    )
    .bind(id1)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(events, vec!["created", "updated"]);
    let events2: Vec<String> = sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\RoutePermission' AND auditable_id = ? ORDER BY id",
    )
    .bind(id2)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(events2, vec!["created", "updated"]);

    // Destroy.
    let (status, del) = send(
        &pool,
        Method::DELETE,
        &format!("/api/route-permissions/{id2}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(del["message"], "Route permission deleted");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/route-permissions/{id2}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/route-permissions/{id2}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, "/uji-rp-crud").await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn route_permissions_non_admin_reads_follow_laravel_middleware() {
    let pool = pool().await;
    cleanup(&pool, "/uji-rp-chk").await;
    let admin = user_token(&pool, ADMIN_CHK, "admin").await;
    let pengawas = user_token(&pool, PENGAWAS_CHK, "pengawas").await;

    for (path, roles) in [
        ("/uji-rp-chk/:id", json!(["pengawas"])),
        ("/uji-rp-chk/admin-only", json!(["admin"])),
    ] {
        sqlx::query("INSERT INTO route_permissions (route_path, route_method, allowed_roles, is_active, created_at, updated_at) VALUES (?, 'GET', ?, 1, NOW(), NOW())")
            .bind(path)
            .bind(roles.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }

    // `rules` dan `user/accessible` diizinkan untuk non-admin (whitelist middleware).
    let (status, rules) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/rules",
        Some(&pengawas),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        rules
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["route_path"] == "/uji-rp-chk/:id"
                && r["allowed_roles"] == json!(["pengawas"]))
    );

    let (status, acc) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/user/accessible",
        Some(&pengawas),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let visible: Vec<String> = values(&acc)
        .iter()
        .map(|r| {
            format!(
                "{} {}",
                r["route_method"].as_str().unwrap(),
                r["route_path"].as_str().unwrap()
            )
        })
        .collect();
    assert!(visible.contains(&"GET /uji-rp-chk/:id".to_string()));
    assert!(!visible.contains(&"GET /uji-rp-chk/admin-only".to_string()));

    // Rute admin-only tanpa rule eksplisit: 403 dari middleware, seperti Laravel.
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/route-permissions",
        Some(&pengawas),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/route-permissions/1",
        Some(&pengawas),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&pengawas),
        Some(json!({ "route_path": "/uji-rp-chk/5" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admin tetap bisa memakai check-access untuk rule pengawas.
    let (status, c) = send(
        &pool,
        Method::POST,
        "/api/route-permissions/check-access",
        Some(&admin),
        Some(json!({ "route_path": "/uji-rp-chk/5", "route_method": "GET" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(c["allowed"], false);

    cleanup(&pool, "/uji-rp-chk").await;
}
