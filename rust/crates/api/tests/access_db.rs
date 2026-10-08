//! Scope `byUserRole()` terhadap MySQL: pengawas hanya pekerjaan assign, role lain juga lewat kegiatan_role.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test access_db -- --ignored
//! ```

use api::access::user_can_access;
use auth::login::roles_of;
use sqlx::MySqlPool;

async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> (u64, Vec<(u64, String)>) {
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
    let roles = roles_of(pool, uid).await.unwrap();
    (uid, roles)
}

async fn cleanup(pool: &MySqlPool, email: &str, uid: u64) {
    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id = ? AND model_type = 'App\\\\Models\\\\User'",
    )
    .bind(uid)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn scope_matches_laravel_by_role() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let pid: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let kegiatan: i64 = sqlx::query_scalar("SELECT kegiatan_id FROM tbl_pekerjaan WHERE id = ?")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Pengawas: tidak boleh tanpa assign, boleh setelah di-assign, walau ada kegiatan_role.
    let pengawas_mail = "uji-scope-pengawas@example.test";
    let (pengawas, roles) = make_user(&pool, pengawas_mail, "pengawas").await;
    sqlx::query("INSERT IGNORE INTO kegiatan_role (kegiatan_id, role_id) SELECT ?, id FROM roles WHERE name = 'pengawas' AND guard_name = 'web' LIMIT 1")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!user_can_access(&pool, pengawas, &roles, pid).await.unwrap());
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pengawas)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    assert!(user_can_access(&pool, pengawas, &roles, pid).await.unwrap());
    assert!(!user_can_access(&pool, pengawas, &roles, 999_999_999)
        .await
        .unwrap());

    // Role biasa: lewat kegiatan_role tanpa assign.
    let user_mail = "uji-scope-user@example.test";
    let (biasa, roles) = make_user(&pool, user_mail, "user").await;
    assert!(!user_can_access(&pool, biasa, &roles, pid).await.unwrap());
    sqlx::query("INSERT IGNORE INTO kegiatan_role (kegiatan_id, role_id) SELECT ?, id FROM roles WHERE name = 'user' AND guard_name = 'web' LIMIT 1")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
    assert!(user_can_access(&pool, biasa, &roles, pid).await.unwrap());
    sqlx::query("DELETE FROM kegiatan_role WHERE kegiatan_id = ? AND role_id IN (SELECT id FROM roles WHERE name = 'user' AND guard_name = 'web')")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();

    // Admin: semua pekerjaan yang ada.
    let (admin, roles) = make_user(&pool, "uji-scope-admin@example.test", "admin").await;
    assert!(user_can_access(&pool, admin, &roles, pid).await.unwrap());
    assert!(!user_can_access(&pool, admin, &roles, 999_999_999)
        .await
        .unwrap());

    cleanup(&pool, pengawas_mail, pengawas).await;
    cleanup(&pool, user_mail, biasa).await;
    cleanup(&pool, "uji-scope-admin@example.test", admin).await;
    sqlx::query("DELETE FROM kegiatan_role WHERE kegiatan_id = ? AND role_id IN (SELECT id FROM roles WHERE name = 'pengawas' AND guard_name = 'web')")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
}

/// Pengawas lewat HTTP: daftar hanya memuat pekerjaan assign, detail di luar assign ditolak 403.
#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn pengawas_sees_only_assigned_pekerjaan_over_http() {
    use api::{app, AppState};
    use axum::{
        body::Body,
        http::{header, Method, Request, StatusCode},
    };
    use serde_json::Value;
    use shared::Config;
    use tower::ServiceExt;

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let mail = "uji-scope-http@example.test";
    let (uid, _) = make_user(&pool, mail, "pengawas").await;
    let token = auth::login::create_token(&pool, uid, "uji-scope-http")
        .await
        .unwrap();
    let assigned: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let other: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_pekerjaan WHERE id <> ? ORDER BY id DESC LIMIT 1",
    )
    .bind(assigned)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(uid)
        .bind(assigned)
        .execute(&pool)
        .await
        .unwrap();

    let config = Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    };
    let get = |uri: String| {
        let config = config.clone();
        let pool = pool.clone();
        let token = token.clone();
        async move {
            let res = app(&config, AppState::new(pool, "http://localhost".into()))
                .oneshot(
                    Request::builder()
                        .method(Method::GET)
                        .uri(uri)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = res.status();
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
        }
    };

    let (status, list) = get("/api/pekerjaan?per_page=-1".into()).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<u64> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_u64().unwrap())
        .collect();
    assert!(ids.contains(&assigned));
    assert!(!ids.contains(&other), "pekerjaan tanpa assign tidak boleh muncul");

    let (status, _) = get(format!("/api/pekerjaan/{assigned}")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = get(format!("/api/pekerjaan/{other}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    cleanup(&pool, mail, uid).await;
}
