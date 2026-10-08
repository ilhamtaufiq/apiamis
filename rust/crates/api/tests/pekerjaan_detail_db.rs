//! `GET /api/pekerjaan/{id}` memakai PekerjaanDetailResource: relasi lengkap, tanpa metrik daftar.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test pekerjaan_detail_db -- --ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const ACTOR: &str = "uji-detail-admin@example.test";

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn show_returns_detail_resource_with_relations() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ACTOR)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Detail', ?, 'x', NOW(), NOW())")
        .bind(ACTOR)
        .execute(&pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ACTOR)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar(
        "SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let token = auth::login::create_token(&pool, uid, "uji-detail")
        .await
        .unwrap();
    let pid: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    let config = Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    };
    let res = app(
        &config,
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/pekerjaan/{pid}"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers().get("x-partial-response").is_none(),
        "detail sudah lengkap"
    );
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let d = &body["data"];

    for key in [
        "foto",
        "berkas",
        "kontrak",
        "output",
        "penerima",
        "tags",
        "assignment_sources",
    ] {
        assert!(d[key].is_array(), "{key} harus array");
    }
    for key in [
        "kecamatan",
        "desa",
        "kegiatan",
        "pengawas",
        "pendamping",
        "progress",
    ] {
        assert!(d.get(key).is_some(), "{key} harus ada (boleh null)");
    }
    assert!(
        d.get("progress_total").is_none(),
        "metrik daftar tidak ada di detail"
    );
    assert!(d.get("penerima_count").is_none());
    assert!(d["foto_status"].is_string());

    // Kontrak yang terkait diberi penyedia dan checklist, tanpa kegiatan dan pekerjaans.
    for k in d["kontrak"].as_array().unwrap() {
        assert!(k.get("is_checklist_complete").is_some());
        assert!(k.get("kegiatan").is_none());
        assert!(k.get("pekerjaans").is_none());
    }
    sqlx::query("DELETE FROM model_has_roles WHERE model_id = ?")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
}
