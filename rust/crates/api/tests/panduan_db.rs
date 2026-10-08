//! Halaman panduan publik lewat router terhadap MySQL: daftar, ringkasan, dan detail terbit.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test panduan_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const SECTION: &str = "uji-panduan";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn get(pool: &MySqlPool, uri: &str) -> (StatusCode, Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    )
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

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn public_panduan_lists_published_pages_in_order() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM panduan_pages WHERE section = ?")
        .bind(SECTION)
        .execute(&pool)
        .await
        .unwrap();
    for (slug, title, order, published) in [
        ("uji-b", "Bab B", 1, 1),
        ("uji-a", "Bab A", 2, 1),
        ("uji-draft", "Draft", 0, 0),
    ] {
        sqlx::query("INSERT INTO panduan_pages (slug, title, description, section, sort_order, body, is_published, created_at, updated_at) VALUES (?, ?, 'Ringkas', ?, ?, '# Isi', ?, NOW(), NOW())")
            .bind(slug)
            .bind(title)
            .bind(SECTION)
            .bind(order)
            .bind(published)
            .execute(&pool)
            .await
            .unwrap();
    }

    // Daftar lengkap: hanya yang terbit, urut sort_order.
    let (status, body) = get(&pool, &format!("/api/panduan?section={SECTION}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["data"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{body}");
    assert_eq!(items[0]["slug"], "uji-b", "{body}");
    assert_eq!(items[1]["slug"], "uji-a", "{body}");
    assert_eq!(items[0]["body"], "# Isi");
    assert_eq!(items[0]["is_published"], true);
    assert!(
        items[0].get("editor").is_none(),
        "editor tidak dimuat: {body}"
    );
    assert!(
        items[0]["created_at"].as_str().unwrap().ends_with("+00:00"),
        "{body}"
    );

    // Ringkasan: tanpa isi, dengan updated_at mentah.
    let (status, body) = get(&pool, &format!("/api/panduan?section={SECTION}&summary=1")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"][0].get("body").is_none(), "{body}");
    assert!(
        body["data"][0]["updated_at"]
            .as_str()
            .unwrap()
            .ends_with("Z"),
        "{body}"
    );

    // Detail terbit: 200; belum terbit atau slug salah: 404.
    let (status, body) = get(&pool, "/api/panduan/uji-a").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["title"], "Bab A");
    let (status, _) = get(&pool, "/api/panduan/uji-draft").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    sqlx::query("DELETE FROM panduan_pages WHERE section = ?")
        .bind(SECTION)
        .execute(&pool)
        .await
        .unwrap();
}
