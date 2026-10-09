//! Tugas survei (`SurveyTugasController`) lewat router terhadap MySQL.
//!
//! Setiap tes memakai awalan nama sendiri (`uji-st-<tes>-`) dan email sendiri, sehingga tes yang
//! berjalan paralel tidak saling menghapus baris.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test survey_tugas_db -- --include-ignored
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

const MODEL: &str = "App\\Models\\SurveyTugas";
const FORBIDDEN_ROLE: &str = "User does not have the right roles.";

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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Tugas', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(pool, uid, "uji-st")
        .await
        .unwrap();
    (uid, token)
}

/// Hapus hanya baris milik awalan `p`: audit, tugas (pivot ikut cascade), dan pengguna uji.
async fn cleanup(pool: &MySqlPool, p: &str) {
    let like = format!("{p}%");
    let user_like = format!("{p}%@example.test");
    sqlx::query(
        "DELETE FROM tbl_audit_logs WHERE auditable_type = ? AND \
         (auditable_id IN (SELECT id FROM tbl_survey_tugas WHERE judul LIKE ?) \
          OR user_id IN (SELECT id FROM users WHERE email LIKE ?))",
    )
    .bind(MODEL)
    .bind(&like)
    .bind(&user_like)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_survey_tugas WHERE judul LIKE ?")
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

/// Tugas langsung lewat SQL (untuk menyiapkan data). Mengembalikan id-nya.
async fn insert_tugas(pool: &MySqlPool, judul: &str, assignee: u64, creator: u64) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_survey_tugas (judul, tahun_anggaran, jenis, assignee_id, status, created_by, created_at, updated_at) \
         VALUES (?, 2026, 'spam_perpipaan', ?, 'ditugaskan', ?, NOW(), NOW())",
    )
    .bind(judul)
    .bind(assignee)
    .bind(creator)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_survey_tugas WHERE judul = ?")
        .bind(judul)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Penanggung jawab tambahan lewat pivot saja (bukan `assignee_id`).
async fn add_pivot(pool: &MySqlPool, tugas: i64, uid: u64) {
    sqlx::query(
        "INSERT INTO tbl_survey_tugas_assignees (survey_tugas_id, user_id, created_at, updated_at) \
         VALUES (?, ?, NOW(), NOW())",
    )
    .bind(tugas)
    .bind(uid)
    .execute(pool)
    .await
    .unwrap();
}

async fn count_tugas_id(pool: &MySqlPool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_judul(pool: &MySqlPool, judul: &str) -> i64 {
    sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas WHERE judul = ?")
        .bind(judul)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_pivot(pool: &MySqlPool, tugas: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_survey_tugas_assignees WHERE survey_tugas_id = ?",
    )
    .bind(tugas)
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn survey_tugas_requires_token() {
    let pool = pool().await;
    let p = "uji-st-auth-";
    cleanup(&pool, p).await;
    let (admin, _) = user(&pool, "uji-st-auth-admin@example.test", &["admin"]).await;
    let (assignee, _) = user(&pool, "uji-st-auth-tfl@example.test", &["tfl"]).await;
    let tugas = insert_tugas(&pool, "uji-st-auth-tugas", assignee, admin).await;

    // Tanpa token semua rute tugas survei menjawab 401, termasuk mutasi.
    let cases: Vec<(Method, String, Option<Value>)> = vec![
        (Method::GET, "/api/survey-tugas".into(), None),
        (
            Method::POST,
            "/api/survey-tugas".into(),
            Some(json!({"judul": "uji-st-auth-baru", "tahun_anggaran": 2026, "assignee_id": assignee})),
        ),
        (Method::GET, format!("/api/survey-tugas/{tugas}"), None),
        (
            Method::PUT,
            format!("/api/survey-tugas/{tugas}"),
            Some(json!({"judul": "uji-st-auth-ubah"})),
        ),
        (Method::DELETE, format!("/api/survey-tugas/{tugas}"), None),
    ];
    for (method, uri, body) in cases {
        let label = format!("{method} {uri}");
        let (status, res) = send(&pool, method, &uri, None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{label}: {res}");
    }

    // Tidak ada yang berubah oleh permintaan tanpa token.
    assert_eq!(count_tugas_id(&pool, tugas).await, 1);
    assert_eq!(count_judul(&pool, "uji-st-auth-baru").await, 0);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn admin_creates_updates_lists_shows_and_deletes_tugas() {
    let pool = pool().await;
    let p = "uji-st-crud-";
    cleanup(&pool, p).await;
    let (admin_id, admin) = user(&pool, "uji-st-crud-admin@example.test", &["admin"]).await;
    let (a_id, _) = user(&pool, "uji-st-crud-a@example.test", &["tfl"]).await;
    let (b_id, _) = user(&pool, "uji-st-crud-b@example.test", &["operator"]).await;

    // Store 201: penanggung jawab tunggal, status default `ditugaskan`, creator terisi.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/survey-tugas",
        Some(&admin),
        Some(json!({
            "judul": "uji-st-crud-tugas",
            "tahun_anggaran": 2026,
            "jenis": "spam_perpipaan",
            "lokasi_catatan": "Dusun uji",
            "assignee_id": a_id,
            "batas_waktu": "2026-12-31",
            "catatan_admin": "catatan uji",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let data = &body["data"];
    let id = data["id"].as_i64().unwrap();
    assert_eq!(data["judul"], "uji-st-crud-tugas", "{body}");
    assert_eq!(data["status"], "ditugaskan", "{body}");
    assert_eq!(data["status_label"], "Ditugaskan", "{body}");
    assert_eq!(data["jenis_label"], "SPAM Perpipaan", "{body}");
    assert_eq!(data["batas_waktu"], "2026-12-31", "{body}");
    assert_eq!(data["assignee"]["id"], a_id, "{body}");
    assert_eq!(data["assignees"][0]["id"], a_id, "{body}");
    assert_eq!(data["creator"]["id"], admin_id, "{body}");
    assert_eq!(data["surveys_count"], 0, "{body}");
    assert_eq!(data["sudah_disurvey"], false, "{body}");
    assert_eq!(count_pivot(&pool, id).await, 1);
    assert_eq!(audit_count(&pool, id, "created").await, 1);

    // Update 200: judul dan status berubah, pivot disinkronkan menjadi dua penanggung jawab.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/survey-tugas/{id}"),
        Some(&admin),
        Some(json!({
            "judul": "uji-st-crud-tugas-ubah",
            "status": "dikerjakan",
            "assignee_ids": [a_id, b_id],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    assert_eq!(data["judul"], "uji-st-crud-tugas-ubah", "{body}");
    assert_eq!(data["status"], "dikerjakan", "{body}");
    assert_eq!(data["status_label"], "Dikerjakan", "{body}");
    assert_eq!(data["assignee"]["id"], a_id, "{body}");
    let ids: Vec<u64> = data["assignees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![a_id, b_id], "{body}");
    assert_eq!(count_pivot(&pool, id).await, 2);
    assert_eq!(audit_count(&pool, id, "updated").await, 1);

    // Daftar: filter search, status, dan tahun menemukan tugas ini.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/survey-tugas?search=uji-st-crud-&status=dikerjakan&tahun_anggaran=2026",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["meta"]["total"].as_i64().unwrap() >= 1, "{body}");
    assert!(
        body["data"].as_array().unwrap().iter().any(|t| t["id"] == id),
        "{body}"
    );

    // Show 200 untuk admin, dengan creator (rute show memuat creator).
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/survey-tugas/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], id, "{body}");
    assert_eq!(body["data"]["creator"]["id"], admin_id, "{body}");

    // Delete: pesan Laravel, pivot ikut terhapus, audit `deleted` dicatat, lalu show 404.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/survey-tugas/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Tugas survey berhasil dihapus.", "{body}");
    assert_eq!(count_tugas_id(&pool, id).await, 0);
    assert_eq!(count_pivot(&pool, id).await, 0);
    assert_eq!(audit_count(&pool, id, "deleted").await, 1);
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/survey-tugas/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn non_admin_gets_403_on_store_update_and_destroy() {
    let pool = pool().await;
    let p = "uji-st-403-";
    cleanup(&pool, p).await;
    let (admin_id, _admin) = user(&pool, "uji-st-403-admin@example.test", &["admin"]).await;
    let (a_id, a_token) = user(&pool, "uji-st-403-tfl@example.test", &["tfl"]).await;
    let tugas = insert_tugas(&pool, "uji-st-403-tugas", a_id, admin_id).await;

    // Store: pengguna dengan role survei tetap ditolak karena bukan admin.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/survey-tugas",
        Some(&a_token),
        Some(json!({"judul": "uji-st-403-baru", "tahun_anggaran": 2026, "assignee_id": a_id})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], FORBIDDEN_ROLE, "{body}");

    // Update dengan PUT dan PATCH.
    for method in [Method::PUT, Method::PATCH] {
        let label = format!("{method}");
        let (status, body) = send(
            &pool,
            method,
            &format!("/api/survey-tugas/{tugas}"),
            Some(&a_token),
            Some(json!({"judul": "uji-st-403-diubah"})),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {body}");
        assert_eq!(body["message"], FORBIDDEN_ROLE, "{label}: {body}");
    }

    // Destroy.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/survey-tugas/{tugas}"),
        Some(&a_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], FORBIDDEN_ROLE, "{body}");

    // Basis data tidak berubah.
    assert_eq!(count_tugas_id(&pool, tugas).await, 1);
    assert_eq!(count_judul(&pool, "uji-st-403-baru").await, 0);
    assert_eq!(count_judul(&pool, "uji-st-403-tugas").await, 1);
    assert_eq!(count_judul(&pool, "uji-st-403-diubah").await, 0);

    cleanup(&pool, p).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn assignee_sees_show_and_non_assignee_gets_403() {
    let pool = pool().await;
    let p = "uji-st-show-";
    cleanup(&pool, p).await;
    let (admin_id, admin) = user(&pool, "uji-st-show-admin@example.test", &["admin"]).await;
    let (a_id, a_token) = user(&pool, "uji-st-show-a@example.test", &["tfl"]).await;
    let (b_id, b_token) = user(&pool, "uji-st-show-b@example.test", &["operator"]).await;
    let (_c_id, c_token) = user(&pool, "uji-st-show-c@example.test", &["tfl"]).await;
    let tugas = insert_tugas(&pool, "uji-st-show-tugas", a_id, admin_id).await;
    // B hanya penanggung jawab lewat pivot, bukan `assignee_id`.
    add_pivot(&pool, tugas, b_id).await;
    let uri = format!("/api/survey-tugas/{tugas}");

    // Penanggung jawab utama.
    let (status, body) = send(&pool, Method::GET, &uri, Some(&a_token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], tugas, "{body}");

    // Penanggung jawab lewat pivot.
    let (status, body) = send(&pool, Method::GET, &uri, Some(&b_token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], tugas, "{body}");

    // Bukan penanggung jawab: 403 `Forbidden`.
    let (status, body) = send(&pool, Method::GET, &uri, Some(&c_token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Forbidden", "{body}");

    // Admin selalu boleh melihat.
    let (status, body) = send(&pool, Method::GET, &uri, Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    cleanup(&pool, p).await;
}
