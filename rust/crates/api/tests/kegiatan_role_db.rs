//! Rute `kegiatan-role` hanya untuk admin (`role:admin` di `routes/api.php`): non-admin mendapat 403
//! pada GET, POST, dan DELETE, sedangkan admin tetap berhasil.
//!
//! Non-admin diberi rule `route_permissions` agar lolos gerbang permission dan benar-benar sampai ke
//! handler, sehingga yang menolak adalah pemeriksaan role. Semua baris memakai prefix `uji-kr-`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test kegiatan_role_db -- --include-ignored
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

const ADMIN: &str = "uji-kr-admin@example.test";
const BIASA: &str = "uji-kr-biasa@example.test";
const ROLE_BIASA: &str = "uji-kr-pengguna";
const KEG_SATU: &str = "UJI-KR Kegiatan Satu";
const KEG_DUA: &str = "UJI-KR Kegiatan Dua";
const PESAN_SPATIE: &str = "User does not have the right roles.";

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
    body: Option<String>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = match body {
        None => Body::empty(),
        Some(s) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(s)
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

/// Hapus hanya baris uji (`uji-kr-`) dan dampaknya: audit, pemetaan, kegiatan, rule, token, user.
async fn bersihkan(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\KegiatanRole' AND auditable_id IN (SELECT id FROM kegiatan_role WHERE role_id IN (SELECT id FROM roles WHERE name = ?))")
        .bind(ROLE_BIASA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM kegiatan_role WHERE role_id IN (SELECT id FROM roles WHERE name = ?)")
        .bind(ROLE_BIASA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_program IN (?, ?)")
        .bind(KEG_SATU)
        .bind(KEG_DUA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM route_permissions WHERE allowed_roles LIKE ?")
        .bind(format!("%{ROLE_BIASA}%"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_type = 'App\\\\Models\\\\User' AND tokenable_id IN (SELECT id FROM users WHERE email IN (?, ?))")
        .bind(ADMIN)
        .bind(BIASA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_type = 'App\\\\Models\\\\User' AND model_id IN (SELECT id FROM users WHERE email IN (?, ?))")
        .bind(ADMIN)
        .bind(BIASA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email IN (?, ?)")
        .bind(ADMIN)
        .bind(BIASA)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM roles WHERE name = ? AND guard_name = 'web'")
        .bind(ROLE_BIASA)
        .execute(pool)
        .await
        .unwrap();
}

async fn role_id(pool: &MySqlPool, name: &str) -> u64 {
    sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Membuat user uji dengan satu role dan mengembalikan (id, token).
async fn pengguna(pool: &MySqlPool, email: &str, role: &str) -> (u64, String) {
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji KR', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role_id(pool, role).await)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, id, "uji-kr").await.unwrap();
    (id, token)
}

async fn kegiatan(pool: &MySqlPool, nama: &str) -> u64 {
    sqlx::query("INSERT INTO tbl_kegiatan (nama_program, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES (?, '2099', 'APBD', 1000, NOW(), NOW())")
        .bind(nama)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar(
        "SELECT id FROM tbl_kegiatan WHERE nama_program = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(nama)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn jumlah_pemetaan(pool: &MySqlPool, kegiatan_id: u64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM kegiatan_role WHERE kegiatan_id = ?")
        .bind(kegiatan_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn kegiatan_role_hanya_admin() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    bersihkan(&pool).await;

    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(ROLE_BIASA)
        .execute(&pool)
        .await
        .unwrap();
    let role_biasa = role_id(&pool, ROLE_BIASA).await;
    let (_, token_admin) = pengguna(&pool, ADMIN, "admin").await;
    let (_, token_biasa) = pengguna(&pool, BIASA, ROLE_BIASA).await;
    let keg_satu = kegiatan(&pool, KEG_SATU).await;
    let keg_dua = kegiatan(&pool, KEG_DUA).await;

    // Pemetaan yang sudah ada, target uji DELETE.
    sqlx::query("INSERT INTO kegiatan_role (role_id, kegiatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(role_biasa)
        .bind(keg_satu)
        .execute(&pool)
        .await
        .unwrap();
    let id: u64 =
        sqlx::query_scalar("SELECT id FROM kegiatan_role WHERE role_id = ? AND kegiatan_id = ?")
            .bind(role_biasa)
            .bind(keg_satu)
            .fetch_one(&pool)
            .await
            .unwrap();

    // Rule DB agar pengguna biasa lolos gerbang permission dan mencapai handler.
    for (path, method) in [
        ("/kegiatan-role", "GET"),
        ("/kegiatan-role", "POST"),
        ("/kegiatan-role/:kegiatanRoleId", "DELETE"),
    ] {
        sqlx::query("INSERT INTO route_permissions (route_path, route_method, allowed_roles, is_active, created_at, updated_at) VALUES (?, ?, ?, 1, NOW(), NOW())")
            .bind(path)
            .bind(method)
            .bind(json!([ROLE_BIASA]).to_string())
            .execute(&pool)
            .await
            .unwrap();
    }

    let body_post = json!({ "role_id": role_biasa, "kegiatan_id": keg_dua }).to_string();

    // Non-admin: GET, POST, dan DELETE ditolak 403 dengan pesan Spatie.
    let (status, body) = send(&pool, Method::GET, "/api/kegiatan-role", &token_biasa, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], PESAN_SPATIE, "{body}");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token_biasa,
        Some(body_post.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], PESAN_SPATIE, "{body}");
    assert_eq!(
        jumlah_pemetaan(&pool, keg_dua).await,
        0,
        "POST non-admin tidak boleh menyimpan"
    );

    // Body JSON rusak tetap 403 (role diperiksa sebelum validasi), bukan 400 dari ekstraktor.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token_biasa,
        Some("{".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], PESAN_SPATIE, "{body}");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kegiatan-role/{id}"),
        &token_biasa,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], PESAN_SPATIE, "{body}");
    assert_eq!(
        jumlah_pemetaan(&pool, keg_satu).await,
        1,
        "DELETE non-admin tidak boleh menghapus"
    );

    // Tanpa token yang valid: 401 (auth:sanctum dijalankan sebelum role).
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/kegiatan-role",
        "token-tidak-valid",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    // Admin: GET, POST, dan DELETE tetap berhasil.
    let (status, body) = send(&pool, Method::GET, "/api/kegiatan-role", &token_admin, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == json!(id)),
        "{body}"
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kegiatan-role",
        &token_admin,
        Some(body_post),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["role"]["name"], ROLE_BIASA, "{body}");
    assert_eq!(body["kegiatan"]["nama_program"], KEG_DUA, "{body}");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kegiatan-role/{id}"),
        &token_admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kegiatan-role mapping deleted", "{body}");
    assert_eq!(jumlah_pemetaan(&pool, keg_satu).await, 0);

    bersihkan(&pool).await;
}
