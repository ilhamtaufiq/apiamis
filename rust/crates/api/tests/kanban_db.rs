//! Rute `/api/kanban/*` lewat router terhadap MySQL: papan dibaca pengguna biasa, tulisan kartu
//! hanya admin (403 untuk yang lain), import dari tiket (409 bila dobel), dan sinkron status tiket
//! saat kartu dipindah.
//!
//! Membutuhkan tabel `tbl_kanban_*` (lihat `rust/fixtures/kanban_schema.sql`, termasuk seed papan
//! `organisasi`) dan `tbl_tiket`. Setiap tes memakai email `uji-kb-*` dan subjek tiket `uji-kb-*`,
//! dan hanya menghapus data milik itu.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test kanban_db -- --include-ignored
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

const USER_MODEL: &str = "App\\Models\\User";
const TIKET_MODEL: &str = "App\\Models\\Tiket";

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

/// Pengguna uji biasa (tanpa peran) dengan token Sanctum.
async fn user_token(pool: &MySqlPool, email: &str) -> (u64, String) {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Kb', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-kb")
        .await
        .unwrap();
    (uid, token)
}

/// Pengguna uji dengan peran admin.
async fn admin_token(pool: &MySqlPool, email: &str) -> (u64, String) {
    let (uid, token) = user_token(pool, email).await;
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
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)")
        .bind(role)
        .bind(USER_MODEL)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    (uid, token)
}

/// Id kolom di papan `organisasi` berdasarkan judulnya (Baru, Proses, Selesai).
async fn column_id(pool: &MySqlPool, title: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT CAST(c.id AS SIGNED) FROM tbl_kanban_columns c \
         JOIN tbl_kanban_boards b ON b.id = c.board_id \
         WHERE b.slug = 'organisasi' AND c.title = ?",
    )
    .bind(title)
    .fetch_one(pool)
    .await
    .expect("kolom papan organisasi harus ada (lihat rust/fixtures/kanban_schema.sql)")
}

/// Hapus hanya data milik `emails` dan tiket uji (subjek `uji-kb-*`): audit tiket, kartu, tiket, lalu pengguna.
async fn clean(pool: &MySqlPool, emails: &[&str]) {
    for &email in emails {
        sqlx::query(
            "DELETE FROM tbl_audit_logs WHERE auditable_type = ? \
             AND user_id IN (SELECT id FROM users WHERE email = ?)",
        )
        .bind(TIKET_MODEL)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM tbl_kanban_cards WHERE created_by IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = ? \
         AND auditable_id IN (SELECT id FROM tbl_tiket WHERE subjek LIKE 'uji-kb-%')",
    )
    .bind(TIKET_MODEL)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_tiket WHERE subjek LIKE 'uji-kb-%'")
        .execute(pool)
        .await
        .unwrap();
    for &email in emails {
        sqlx::query("DELETE FROM model_has_roles WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(USER_MODEL)
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_kanban_*"]
async fn kanban_board_readable_by_user_and_requires_login() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const USER: &str = "uji-kb-papan@example.test";
    clean(&pool, &[USER]).await;
    let (_, token) = user_token(&pool, USER).await;

    // Tanpa token: 401, untuk baca papan maupun tulis kartu.
    let (status, _) = send(&pool, Method::GET, "/api/kanban/board", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards",
        None,
        Some(json!({"column_id": 1, "title": "uji-kb-tanpa-token"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Pengguna biasa boleh membaca papan: tiga kolom berurutan dengan daftar kartu.
    let (status, body) = send(&pool, Method::GET, "/api/kanban/board", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["slug"], "organisasi", "{body}");
    let kolom = body["data"]["columns"].as_array().unwrap();
    assert!(kolom.len() >= 3, "{body}");
    assert_eq!(kolom[0]["title"], "Baru", "{body}");
    assert!(kolom.iter().all(|k| k["cards"].is_array()), "{body}");

    clean(&pool, &[USER]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_kanban_*"]
async fn kanban_card_create_update_move_delete_admin_only() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const ADMIN: &str = "uji-kb-admin@example.test";
    const USER: &str = "uji-kb-user@example.test";
    clean(&pool, &[ADMIN, USER]).await;
    let (admin_id, admin) = admin_token(&pool, ADMIN).await;
    let (_, user) = user_token(&pool, USER).await;
    let baru = column_id(&pool, "Baru").await;
    let proses = column_id(&pool, "Proses").await;

    // Pengguna biasa: create ditolak 403.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards",
        Some(&user),
        Some(json!({"column_id": baru, "title": "uji-kb-tolak"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admin create: string dipangkas, string kosong jadi null, sumber `manual`.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards",
        Some(&admin),
        Some(json!({
            "column_id": baru,
            "title": "  uji-kb-kartu  ",
            "description": "  Catatan uji  ",
            "status_label": "   ",
            "metadata": {"tag": "uji"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let card = &body["data"];
    assert_eq!(card["title"], "uji-kb-kartu", "{body}");
    assert_eq!(card["description"], "Catatan uji", "{body}");
    assert!(card["status_label"].is_null(), "{body}");
    assert_eq!(card["source"], "manual", "{body}");
    assert_eq!(card["column_id"], baru, "{body}");
    assert_eq!(card["created_by"], admin_id, "{body}");
    assert_eq!(card["metadata"]["tag"], "uji", "{body}");
    let id = card["id"].as_i64().unwrap();

    // Validasi create: judul wajib, kolom harus ada di papan.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards",
        Some(&admin),
        Some(json!({"column_id": baru})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], "Validation error", "{body}");
    assert_eq!(body["errors"]["title"][0], "The title field is required.", "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards",
        Some(&admin),
        Some(json!({"column_id": 999999999, "title": "uji-kb-kolom"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["column_id"][0], "The selected column id is invalid.", "{body}");

    // Update: pengguna biasa 403. Admin mengubah judul, deskripsi yang tidak dikirim tetap.
    let (status, _) = send(
        &pool,
        Method::PUT,
        &format!("/api/kanban/cards/{id}"),
        Some(&user),
        Some(json!({"title": "uji-kb-tolak"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/kanban/cards/{id}"),
        Some(&admin),
        Some(json!({"title": "uji-kb-kartu-ubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "uji-kb-kartu-ubah", "{body}");
    assert_eq!(body["data"]["description"], "Catatan uji", "{body}");

    // Move: pengguna biasa 403. Validasi posisi negatif 422. Admin pindah ke Proses.
    let (status, _) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kanban/cards/{id}/move"),
        Some(&user),
        Some(json!({"column_id": proses, "position": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kanban/cards/{id}/move"),
        Some(&admin),
        Some(json!({"column_id": proses, "position": -1})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["position"][0], "The position field must be at least 0.", "{body}");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kanban/cards/{id}/move"),
        Some(&admin),
        Some(json!({"column_id": proses, "position": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["column_id"], proses, "{body}");
    assert_eq!(body["data"]["position"], 0, "{body}");

    // Delete: pengguna biasa 403 dan kartu tetap ada. Admin menghapus, lalu 404 untuk id yang sama.
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kanban/cards/{id}"),
        Some(&user),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let ada: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kanban_cards WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ada, 1);
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kanban/cards/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Kartu kanban berhasil dihapus", "{body}");
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/kanban/cards/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    clean(&pool, &[ADMIN, USER]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan tabel tbl_kanban_* serta tbl_tiket"]
async fn kanban_import_from_tiket_once_then_conflict_and_move_syncs_status() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    const ADMIN: &str = "uji-kb-tiket-admin@example.test";
    const USER: &str = "uji-kb-tiket-user@example.test";
    clean(&pool, &[ADMIN, USER]).await;
    let (admin_id, admin) = admin_token(&pool, ADMIN).await;
    let (_, user) = user_token(&pool, USER).await;
    let proses = column_id(&pool, "Proses").await;
    let selesai = column_id(&pool, "Selesai").await;

    // Tiket uji berstatus pending: kartu dari tiket ini masuk ke kolom "Proses" lewat status.
    let tiket_id = sqlx::query(
        "INSERT INTO tbl_tiket (user_id, subjek, deskripsi, kategori, prioritas, status, created_at, updated_at) \
         VALUES (?, 'uji-kb-tiket', 'Deskripsi uji tiket', 'bug', 'high', 'pending', NOW(), NOW())",
    )
    .bind(admin_id)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id() as i64;

    // Pengguna biasa: 403.
    let (status, _) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards/from-tiket",
        Some(&user),
        Some(json!({"tiket_id": tiket_id})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Validasi: tiket_id wajib dan harus ada.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards/from-tiket",
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["tiket_id"][0], "The tiket id field is required.", "{body}");
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards/from-tiket",
        Some(&admin),
        Some(json!({"tiket_id": 999999999})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"]["tiket_id"][0], "The selected tiket id is invalid.", "{body}");

    // Import pertama: 200, sumber `tiket`, judul dari subjek, kolom dari status tiket.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards/from-tiket",
        Some(&admin),
        Some(json!({"tiket_id": tiket_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let card = &body["data"];
    assert_eq!(card["source"], "tiket", "{body}");
    assert_eq!(card["tiket_id"], tiket_id, "{body}");
    assert_eq!(card["title"], "uji-kb-tiket", "{body}");
    assert_eq!(card["description"], "Deskripsi uji tiket", "{body}");
    assert_eq!(card["column_id"], proses, "{body}");
    assert_eq!(card["metadata"]["kategori"], "bug", "{body}");
    assert_eq!(card["tiket"]["id"], tiket_id, "{body}");
    assert_eq!(card["tiket"]["status"], "pending", "{body}");
    let card_id = card["id"].as_i64().unwrap();

    // Import kedua untuk tiket yang sama: 409.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/kanban/cards/from-tiket",
        Some(&admin),
        Some(json!({"tiket_id": tiket_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["message"], "Tiket sudah ada di kanban", "{body}");

    // Pindah kartu ke "Selesai" menyinkronkan status tiket menjadi closed, dan tercatat audit.
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/kanban/cards/{card_id}/move"),
        Some(&admin),
        Some(json!({"column_id": selesai, "position": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["column_id"], selesai, "{body}");
    assert_eq!(body["data"]["tiket"]["status"], "closed", "{body}");
    let status_db: String = sqlx::query_scalar("SELECT status FROM tbl_tiket WHERE id = ?")
        .bind(tiket_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status_db, "closed");
    let audit: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = 'updated'",
    )
    .bind(TIKET_MODEL)
    .bind(tiket_id as u64)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audit >= 1, "perubahan status tiket harus diaudit");

    clean(&pool, &[ADMIN, USER]).await;
}
