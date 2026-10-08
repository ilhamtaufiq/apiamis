//! Tambah dan hapus Pekerjaan lewat router terhadap MySQL: validasi, audit, notifikasi, dan scope.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_store_db -- --include-ignored
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

const MARK: &str = "uji-pstore-";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Jalankan satu statement tulis; mengulang bila kena deadlock dengan tes lain di binary yang sama.
async fn retry_db<F, Fut>(mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<sqlx::mysql::MySqlQueryResult, sqlx::Error>>,
{
    for attempt in 1..=10u64 {
        match f().await {
            Ok(_) => return,
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("40001") => {
                tokio::time::sleep(std::time::Duration::from_millis(20 * attempt)).await;
            }
            Err(e) => panic!("statement gagal: {e}"),
        }
    }
    panic!("deadlock berulang pada statement tes")
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "uji-agent");
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(builder.body(Body::from(body)).unwrap())
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

/// User baru dengan satu role (dibuat ulang setiap tes).
async fn try_make_user(pool: &MySqlPool, email: &str, role: &str) -> Result<u64, sqlx::Error> {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(format!("Uji {email}"))
        .bind(email)
        .execute(pool)
        .await?;
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await?;
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(role)
        .execute(pool)
        .await?;
    let rid: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
            .bind(role)
            .fetch_one(pool)
            .await?;
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(rid)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(uid)
}

/// User baru dengan satu role; email unik per tes. Mengulang bila kena deadlock dengan tes lain.
async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> u64 {
    for attempt in 0..10u64 {
        match try_make_user(pool, email, role).await {
            Ok(id) => return id,
            Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("40001") => {
                tokio::time::sleep(std::time::Duration::from_millis(20 * (attempt + 1))).await;
            }
            Err(e) => panic!("make_user: {e}"),
        }
    }
    panic!("make_user: deadlock berulang untuk {email}")
}

/// Kecamatan dan desa khusus tes (tabel referensi di DB lokal kosong).
async fn seed_region(pool: &MySqlPool, tag: &str) -> (u64, u64) {
    sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(format!("{MARK}kec-{tag}"))
    .execute(pool)
    .await
    .unwrap();
    let kec: u64 = sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(format!("{MARK}kec-{tag}"))
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_desa (n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(format!("{MARK}desa-{tag}"))
        .bind(kec)
        .execute(pool)
        .await
        .unwrap();
    let desa: u64 = sqlx::query_scalar("SELECT id FROM tbl_desa WHERE n_desa = ?")
        .bind(format!("{MARK}desa-{tag}"))
        .fetch_one(pool)
        .await
        .unwrap();
    (kec, desa)
}

async fn cleanup_region(pool: &MySqlPool, kec: u64, desa: u64) {
    retry_db(|| async move {
        sqlx::query("DELETE FROM tbl_desa WHERE id = ?")
            .bind(desa)
            .execute(pool)
            .await
    })
    .await;
    retry_db(|| async move {
        sqlx::query("DELETE FROM tbl_kecamatan WHERE id = ?")
            .bind(kec)
            .execute(pool)
            .await
    })
    .await;
}

/// Hapus pekerjaan milik tes ini beserta jejak audit dan notifikasinya.
/// Notifikasi dicari lewat `notifiable_id` (indeks) agar tidak mengunci baris tes lain.
async fn cleanup_pekerjaan(pool: &MySqlPool, ids: &[u64], users: &[u64]) {
    for id in ids {
        retry_db(|| async move {
    sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ?")
            .bind(id)
        .execute(pool)
        .await
    })
    .await;
        for user in users {
            retry_db(|| async move {
                sqlx::query("DELETE FROM notifications WHERE notifiable_id = ? AND data LIKE ?")
                    .bind(user)
                    .bind(format!("%Model Pekerjaan dengan ID #{id} %"))
                    .execute(pool)
                    .await
            })
            .await;
        }
        retry_db(|| async move {
            sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
        })
        .await;
    }
}

fn pool_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_creates_pekerjaan_with_audit_and_admin_notification() {
    let pool = MySqlPool::connect(&pool_url()).await.unwrap();
    let actor = make_user(&pool, "uji-pstore-a1@example.test", "admin").await;
    let other = make_user(&pool, "uji-pstore-a2@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pstore")
        .await
        .unwrap();
    let (kec, desa) = seed_region(&pool, "store").await;
    let nama = format!("{MARK}paket-store");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &token,
        Some(json!({
            "nama_paket": format!("  {nama}  "),
            "pagu": 1500000,
            "kecamatan_id": kec,
            "desa_id": desa,
            "catatan": "catatan uji",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_u64().expect("id pekerjaan");

    let row = sqlx::query("SELECT nama_paket, kode_rekening, pagu, is_konsultan, status, catatan, kecamatan_id, desa_id FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        row.try_get::<String, _>("nama_paket").unwrap(),
        nama,
        "nama dipangkas"
    );
    assert_eq!(
        row.try_get::<String, _>("kode_rekening").unwrap(),
        "0",
        "default DB berlaku bila tidak dikirim"
    );
    assert_eq!(row.try_get::<f32, _>("pagu").unwrap(), 1_500_000.0);
    assert_eq!(row.try_get::<i8, _>("is_konsultan").unwrap(), 0);
    assert_eq!(row.try_get::<String, _>("status").unwrap(), "active");
    assert_eq!(row.try_get::<String, _>("catatan").unwrap(), "catatan uji");
    assert_eq!(row.try_get::<i64, _>("kecamatan_id").unwrap(), kec as i64);

    let audit: String = sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(actor)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, "created");

    let notif: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND data LIKE ? AND data LIKE '%Data Pekerjaan dibuat%'",
    )
    .bind(other)
    .bind(format!("%Model Pekerjaan dengan ID #{id} %"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(notif, 1, "admin lain mendapat notifikasi");
    let self_notif: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE notifiable_id = ? AND data LIKE ?",
    )
    .bind(actor)
    .bind(format!("%Model Pekerjaan dengan ID #{id} %"))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(self_notif, 0, "pelaku tidak diberi notifikasi");

    cleanup_pekerjaan(&pool, &[id], &[actor, other]).await;
    cleanup_region(&pool, kec, desa).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_rejects_missing_required_fields_with_laravel_messages() {
    let pool = MySqlPool::connect(&pool_url()).await.unwrap();
    let actor = make_user(&pool, "uji-pstore-b1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pstore")
        .await
        .unwrap();

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The given data was invalid.");
    assert_eq!(
        body["errors"]["nama_paket"][0],
        "The nama paket field is required."
    );
    assert_eq!(body["errors"]["pagu"][0], "The pagu field is required.");
    assert_eq!(
        body["errors"]["kecamatan_id"][0],
        "The kecamatan id field is required."
    );
    assert_eq!(
        body["errors"]["desa_id"][0],
        "The desa id field is required."
    );
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn store_konsultan_skips_region_and_rejects_unknown_foreign_keys() {
    let pool = MySqlPool::connect(&pool_url()).await.unwrap();
    let actor = make_user(&pool, "uji-pstore-c1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pstore")
        .await
        .unwrap();
    let nama = format!("{MARK}konsultan");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &token,
        Some(json!({"nama_paket": nama, "pagu": 0, "is_konsultan": true, "kecamatan_id": 999_999_999})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "kecamatan_id harus ada di tabel"
    );
    assert_eq!(
        body["errors"]["kecamatan_id"][0],
        "The selected kecamatan id is invalid."
    );

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &token,
        Some(json!({"nama_paket": nama, "pagu": 250.5, "is_konsultan": "1"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["data"]["id"].as_u64().unwrap();
    let row =
        sqlx::query("SELECT is_konsultan, kecamatan_id, desa_id FROM tbl_pekerjaan WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.try_get::<i8, _>("is_konsultan").unwrap(), 1);
    assert!(row
        .try_get::<Option<i64>, _>("kecamatan_id")
        .unwrap()
        .is_none());
    assert!(row.try_get::<Option<i64>, _>("desa_id").unwrap().is_none());

    cleanup_pekerjaan(&pool, &[id], &[actor]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn destroy_removes_row_and_writes_deleted_audit() {
    let pool = MySqlPool::connect(&pool_url()).await.unwrap();
    let actor = make_user(&pool, "uji-pstore-d1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, actor, "uji-pstore")
        .await
        .unwrap();
    let (kec, desa) = seed_region(&pool, "destroy").await;

    let (_, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &token,
        Some(json!({"nama_paket": format!("{MARK}paket-hapus"), "pagu": 10, "kecamatan_id": kec, "desa_id": desa})),
    )
    .await;
    let id = body["data"]["id"].as_u64().unwrap();

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/pekerjaan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Pekerjaan deleted successfully");

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let event: String = sqlx::query_scalar(
        "SELECT event FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\Pekerjaan' AND auditable_id = ? AND event = 'deleted'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(event, "deleted");

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/pekerjaan/{id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "pekerjaan yang sudah hilang");

    cleanup_pekerjaan(&pool, &[id], &[actor]).await;
    cleanup_region(&pool, kec, desa).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn destroy_is_forbidden_outside_scope_and_blocked_by_spm_sanitasi_link() {
    let pool = MySqlPool::connect(&pool_url()).await.unwrap();
    let admin = make_user(&pool, "uji-pstore-e1@example.test", "admin").await;
    let user = make_user(&pool, "uji-pstore-e3@example.test", "user").await;
    let admin_token = auth::login::create_token(&pool, admin, "uji-pstore")
        .await
        .unwrap();
    let user_token = auth::login::create_token(&pool, user, "uji-pstore")
        .await
        .unwrap();
    let (kec, desa) = seed_region(&pool, "scope").await;

    let (_, body) = send(
        &pool,
        Method::POST,
        "/api/pekerjaan",
        &admin_token,
        Some(json!({"nama_paket": format!("{MARK}paket-scope"), "pagu": 10, "kecamatan_id": kec, "desa_id": desa})),
    )
    .await;
    let id = body["data"]["id"].as_u64().unwrap();

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/pekerjaan/{id}"),
        &user_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "user tanpa assignment");
    let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 1);

    sqlx::query("INSERT INTO tbl_spm_sanitasi (jenis, nama_infrastruktur, created_at, updated_at) VALUES ('mck', ?, NOW(), NOW())")
        .bind(format!("{MARK}spm"))
        .execute(&pool)
        .await
        .unwrap();
    let spm: u64 =
        sqlx::query_scalar("SELECT id FROM tbl_spm_sanitasi WHERE nama_infrastruktur = ?")
            .bind(format!("{MARK}spm"))
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO tbl_spm_sanitasi_pekerjaan (spm_sanitasi_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(spm)
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/pekerjaan/{id}"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    sqlx::query("DELETE FROM tbl_spm_sanitasi_pekerjaan WHERE spm_sanitasi_id = ?")
        .bind(spm)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE id = ?")
        .bind(spm)
        .execute(&pool)
        .await
        .unwrap();
    cleanup_pekerjaan(&pool, &[id], &[admin]).await;
    cleanup_region(&pool, kec, desa).await;
}
