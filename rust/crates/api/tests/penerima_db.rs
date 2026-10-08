//! CRUD penerima lewat router terhadap MySQL: enkripsi `nik`/`alamat`, masking, dan PIN.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test penerima_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use base64::Engine;
use serde_json::{json, Value};
use shared::Config;
use sqlx::{MySqlPool, Row};
use tower::ServiceExt;

const ACTOR: &str = "uji-penerima-admin@example.test";

fn config() -> Config {
    Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    }
}

async fn send(
    pool: &MySqlPool,
    method: Method,
    uri: &str,
    token: &str,
    extra: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(req)
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

async fn make_admin(pool: &MySqlPool) -> u64 {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Penerima', ?, 'x', NOW(), NOW())")
        .bind(ACTOR)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ACTOR)
        .fetch_one(pool)
        .await
        .unwrap();
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
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    uid
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn penerima_crud_encrypts_and_masks() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    // Kunci uji 32 byte, dalam format `base64:` seperti APP_KEY Laravel.
    let app_key = format!(
        "base64:{}",
        base64::engine::general_purpose::STANDARD.encode([b'k'; 32])
    );
    std::env::set_var("APP_KEY", &app_key);
    let key = api::crypt::key_from_app_key(&app_key).unwrap();

    let actor = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, actor, "uji-penerima")
        .await
        .unwrap();
    let pekerjaan: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    // PIN yang berlaku: setting `penerima_pin` bila ada, selain itu default 123456.
    let pin: String = sqlx::query_scalar(
        "SELECT `value` FROM app_settings WHERE `key` = 'penerima_pin' ORDER BY id LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .unwrap()
    .flatten()
    .unwrap_or_else(|| "123456".into());
    let wrong = if pin == "000000" { "111111" } else { "000000" };

    // Create: nik dan alamat dimask tanpa PIN.
    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/penerima",
        &token,
        &[],
        Some(json!({
            "pekerjaan_id": pekerjaan,
            "nama": "Uji Penerima",
            "nik": "3203123456789012",
            "alamat": "Jl. Mawar No. 5",
            "jumlah_jiwa": 4,
            "is_komunal": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let data = &created["data"];
    let id = data["id"].as_i64().unwrap();
    assert_eq!(data["nik"], "3203************");
    assert_eq!(data["alamat"], "Jl. Ma*********");
    assert_eq!(data["is_komunal"], true);
    assert_eq!(data["pekerjaan"]["id"], pekerjaan);
    let created_at = data["created_at"].as_str().unwrap();
    assert_eq!(created_at.len(), 19, "format Y-m-d H:i:s: {created_at}");

    // Di DB tersimpan terenkripsi, dan bisa didekripsi dengan APP_KEY.
    let raw: String = sqlx::query_scalar("SELECT CAST(nik AS CHAR) FROM tbl_penerima WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(raw, "3203123456789012");
    assert_eq!(
        api::crypt::decrypt_string(&key, &raw).unwrap(),
        "3203123456789012"
    );

    // PIN benar: tampil utuh. PIN salah: tetap masked.
    let (_, plain) = send(
        &pool,
        Method::GET,
        &format!("/api/penerima/{id}"),
        &token,
        &[("X-PIN", &pin)],
        None,
    )
    .await;
    assert_eq!(plain["data"]["nik"], "3203123456789012");
    assert_eq!(plain["data"]["alamat"], "Jl. Mawar No. 5");
    let (_, masked) = send(
        &pool,
        Method::GET,
        &format!("/api/penerima/{id}"),
        &token,
        &[("X-PIN", wrong)],
        None,
    )
    .await;
    assert_eq!(masked["data"]["nik"], "3203************");

    // Daftar per pekerjaan dan statistik komunal.
    let (status, list) = send(
        &pool,
        Method::GET,
        &format!("/api/penerima/pekerjaan/{pekerjaan}?per_page=-1"),
        &token,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == id));
    let (_, stats) = send(
        &pool,
        Method::GET,
        &format!("/api/penerima/pekerjaan/{pekerjaan}/stats/komunal"),
        &token,
        &[],
        None,
    )
    .await;
    assert_eq!(
        stats["pekerjaan_id"],
        pekerjaan.to_string().as_str(),
        "pekerjaan_id dikembalikan sebagai string"
    );
    assert!(stats["komunal_count"].as_i64().unwrap() >= 1);

    // Update: nama dan status komunal berubah, nik tetap terenkripsi.
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/penerima/{id}"),
        &token,
        &[],
        Some(json!({ "nama": "Uji Ubah", "is_komunal": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["data"]["nama"], "Uji Ubah");
    assert_eq!(updated["data"]["is_komunal"], false);
    let raw_after: String =
        sqlx::query_scalar("SELECT CAST(nik AS CHAR) FROM tbl_penerima WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        api::crypt::decrypt_string(&key, &raw_after).unwrap(),
        "3203123456789012"
    );

    // Ringkasan dan rekap berjalan.
    let (status, summary) = send(
        &pool,
        Method::GET,
        "/api/penerima/summary",
        &token,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(summary["total_penerima"].as_i64().unwrap() >= 1);
    let (status, rekap) = send(&pool, Method::GET, "/api/penerima/rekap", &token, &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(rekap["data"].is_array());

    // Validasi: jumlah_jiwa minimal 1.
    let (status, err) = send(
        &pool,
        Method::PATCH,
        &format!("/api/penerima/{id}"),
        &token,
        &[],
        Some(json!({ "jumlah_jiwa": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");

    // Delete.
    let (status, msg) = send(
        &pool,
        Method::DELETE,
        &format!("/api/penerima/{id}"),
        &token,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(msg["message"], "Penerima berhasil dihapus");
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/penerima/{id}"),
        &token,
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let left: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_penerima WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(left, 0);
}
