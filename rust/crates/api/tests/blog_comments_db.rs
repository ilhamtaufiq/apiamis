//! Komentar blog lewat router terhadap MySQL: baca publik, tulis, ubah, hapus, throttle, dan kedalaman.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test blog_comments_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Email uji per tes. Awalan `uji-blogc-<tag>-` dipakai juga untuk pembersihan.
fn email(tag: &str, who: &str) -> String {
    format!("uji-blogc-{tag}-{who}@example.test")
}

fn router(pool: &MySqlPool) -> Router {
    app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
}

/// Kirim satu request ke router yang sama (agar limiter dalam proses ikut dipakai).
async fn send(
    router: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    value: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match value {
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Komentar', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid = user_id(pool, email).await;
    if admin {
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
    }
    auth::login::create_token(pool, uid, "uji-blogc")
        .await
        .unwrap()
}

async fn user_id(pool: &MySqlPool, email: &str) -> u64 {
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Hapus baris uji milik satu tes (`tag`).
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let slug = format!("uji-blogc-{tag}%");
    let mail = format!("uji-blogc-{tag}-%@example.test");
    let users = "SELECT id FROM users WHERE email LIKE ?";
    let blogs = "SELECT id FROM tbl_blog WHERE slug LIKE ?";
    sqlx::query(&format!(
        "DELETE FROM tbl_blog_comment WHERE blog_id IN ({blogs})"
    ))
    .bind(&slug)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_blog_comment WHERE user_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM tbl_blog WHERE slug LIKE ?")
        .bind(&slug)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!(
        "DELETE FROM notifications WHERE notifiable_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM tbl_audit_logs WHERE auditable_type IN ('App\\\\Models\\\\Blog', 'App\\\\Models\\\\BlogComment') AND user_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "DELETE FROM model_has_roles WHERE model_id IN ({users})"
    ))
    .bind(&mail)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE ?")
        .bind(&mail)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_blog(
    pool: &MySqlPool,
    slug: &str,
    user: u64,
    published: bool,
    internal: bool,
) -> u64 {
    sqlx::query(
        "INSERT INTO tbl_blog (title, slug, content, category, user_id, is_published, is_internal, published_at, created_at, updated_at) \
         VALUES (?, ?, 'isi uji', 'uji-blogc', ?, ?, ?, IF(?, NOW(), NULL), NOW(), NOW())",
    )
    .bind(format!("Judul {slug}"))
    .bind(slug)
    .bind(user)
    .bind(published as i64)
    .bind(internal as i64)
    .bind(published as i64)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn notif_count(pool: &MySqlPool, user: u64, needle: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM notifications WHERE notifiable_id = ? AND data LIKE ?",
    )
    .bind(user)
    .bind(format!("%{needle}%"))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn audit_count(pool: &MySqlPool, model: &str, event: &str, id: u64) -> i64 {
    sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = ?",
    )
    .bind(model)
    .bind(id)
    .bind(event)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn komentar_publik_thread_dan_notifikasi() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let tag = "cm";
    cleanup(&pool, tag).await;
    let author_token = user_token(&pool, &email(tag, "author"), false).await;
    let commenter = user_token(&pool, &email(tag, "commenter"), false).await;
    let author = user_id(&pool, &email(tag, "author")).await;
    let commenter_id = user_id(&pool, &email(tag, "commenter")).await;
    let pub_id = insert_blog(&pool, "uji-blogc-cm-pub", author, true, false).await;
    let draft_id = insert_blog(&pool, "uji-blogc-cm-draft", author, false, false).await;
    insert_blog(&pool, "uji-blogc-cm-int", author, true, true).await;
    let r = router(&pool);

    // Binding `{blog}` memakai slug: id numerik tidak ditemukan.
    let (status, _, body) = send(
        &r,
        Method::GET,
        &format!("/api/blog/{pub_id}/comments"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "Not Found.");

    // Tamu melihat daftar kosong.
    let (status, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-pub/comments",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"], json!([]));
    assert_eq!(body["meta"]["total"], 0);
    assert_eq!(body["meta"]["root_total"], 0);
    assert_eq!(body["meta"]["per_page"], 20);
    assert_eq!(body["meta"]["sort"], "oldest");

    // Menulis butuh login.
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        None,
        Some(json!({"body": "tamu"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Unauthenticated.");

    // Komentar akar: HTML dibuang, lalu dipangkas. Penulis artikel diberi tahu.
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "  <b>Halo</b> dunia uji  "})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["message"], "Komentar berhasil dikirim");
    assert_eq!(body["data"]["body"], "Halo dunia uji");
    assert_eq!(body["data"]["depth"], 0);
    assert_eq!(body["data"]["can_delete"], true);
    assert_eq!(body["data"]["can_edit"], true);
    assert_eq!(body["data"]["is_deleted"], false);
    let c1 = body["data"]["id"].as_u64().unwrap();
    assert_eq!(
        notif_count(&pool, author, "Komentar baru pada publikasi").await,
        1
    );

    // Balasan dari penulis: pemilik komentar induk diberi tahu, penulis sendiri tidak.
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&author_token),
        Some(json!({"body": "balasan penulis", "parent_id": c1})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["depth"], 1);
    assert_eq!(body["data"]["parent_id"], c1);
    let reply = body["data"]["id"].as_u64().unwrap();
    assert_eq!(
        notif_count(&pool, commenter_id, "Balasan komentar baru").await,
        1
    );
    assert_eq!(
        notif_count(&pool, author, "Komentar baru pada publikasi").await,
        1
    );

    // Daftar untuk tamu: dua komentar, satu akar. Tanpa hak hapus atau ubah.
    let (status, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-pub/comments",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 2, "{body}");
    assert_eq!(body["data"][0]["id"], c1);
    assert_eq!(body["data"][1]["id"], reply);
    assert_eq!(body["data"][0]["can_delete"], false);
    assert_eq!(body["data"][0]["can_edit"], false);
    assert_eq!(body["meta"]["total"], 2);
    assert_eq!(body["meta"]["root_total"], 1);
    assert_eq!(body["meta"]["sort"], "oldest");

    // Urutan terbaru dulu hanya mengubah urutan akar.
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-pub/comments?sort=newest",
        None,
        None,
    )
    .await;
    assert_eq!(body["meta"]["sort"], "newest");
    assert_eq!(body["data"].as_array().unwrap().len(), 2);

    // Thread dari balasan memuat akarnya.
    let (status, _, body) = send(
        &r,
        Method::GET,
        &format!("/api/blog/uji-blogc-cm-pub/comments/thread/{reply}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 2);

    let (status, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-pub/comments/count",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);

    // Artikel internal: tamu ditolak, yang login boleh.
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-int/comments",
        Some(&commenter),
        Some(json!({"body": "komentar internal"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let int_comment = body["data"]["id"].as_u64().unwrap();
    let (status, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-int/comments",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Komentar hanya dapat diakses oleh pengguna yang login."
    );
    let (status, _, _) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-int/comments/count",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = send(
        &r,
        Method::GET,
        &format!("/api/blog/uji-blogc-cm-int/comments/thread/{int_comment}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cm-int/comments",
        Some(&commenter),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    // Artikel draf tidak menerima komentar.
    let _ = draft_id;
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-draft/comments",
        Some(&commenter),
        Some(json!({"body": "ke draf"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Komentar hanya tersedia untuk artikel yang sudah terbit."
    );

    // Validasi dan aturan lain.
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "Validasi gagal");
    assert_eq!(body["errors"]["body"][0], "The body field is required.");

    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "<br>"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "Isi komentar tidak boleh kosong.");

    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "uji parent tidak ada", "parent_id": 999_999_999})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["parent_id"][0],
        "The selected parent id is invalid."
    );

    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "uji parent artikel lain", "parent_id": int_comment})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "Komentar induk tidak ditemukan.");

    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "uji parent teks", "parent_id": "abc"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["parent_id"][0],
        "The parent id field must be an integer."
    );

    let too_long = "x".repeat(5001);
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": too_long})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["body"][0],
        "The body field must not be greater than 5000 characters."
    );

    // Komentar identik dalam 30 detik: 429 (isi dibandingkan setelah dibersihkan dari HTML).
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cm-pub/comments",
        Some(&commenter),
        Some(json!({"body": "Halo dunia uji"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        body["message"],
        "Komentar identik baru saja dikirim. Tunggu sebentar sebelum mengirim ulang."
    );

    // Admin: daftar lintas artikel dengan pratinjau dan info artikel.
    let (status, _, body) = send(&r, Method::GET, "/api/blog/comments", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["message"], "Unauthenticated.");
    let admin_token = user_token(&pool, &email(tag, "admin"), true).await;
    let (status, _, body) = send(
        &r,
        Method::GET,
        &format!("/api/blog/comments?blog_id={pub_id}&per_page=10"),
        Some(&admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 2, "{body}");
    assert_eq!(body["meta"]["per_page"], 10);
    let first = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == c1)
        .unwrap();
    assert_eq!(first["body_preview"], "Halo dunia uji");
    assert_eq!(first["blog"]["slug"], "uji-blogc-cm-pub");
    assert_eq!(first["blog"]["is_published"], true);
    assert_eq!(first["can_delete"], true);

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn komentar_ubah_hapus_dan_moderasi_admin() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let tag = "cu";
    cleanup(&pool, tag).await;
    let author_token = user_token(&pool, &email(tag, "author"), false).await;
    let author = user_id(&pool, &email(tag, "author")).await;
    let commenter_token = user_token(&pool, &email(tag, "commenter"), false).await;
    let admin_token = user_token(&pool, &email(tag, "admin"), true).await;
    insert_blog(&pool, "uji-blogc-cu-pub", author, true, false).await;
    let r = router(&pool);

    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cu-pub/comments",
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu komentar awal"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let c = body["data"]["id"].as_u64().unwrap();
    let uri = format!("/api/blog/comments/{c}");

    // Bukan pemilik: ubah dan hapus ditolak (hapus juga untuk penulis artikel).
    let (status, _, body) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&author_token),
        Some(json!({"body": "ubah"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Anda tidak memiliki izin mengedit komentar ini."
    );
    let (status, _, body) = send(&r, Method::DELETE, &uri, Some(&author_token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Anda tidak memiliki izin menghapus komentar ini."
    );

    // Isi sama: tidak ada perubahan, tanpa audit.
    let (status, _, body) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu komentar awal"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Tidak ada perubahan pada komentar.");

    let (status, _, body) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&commenter_token),
        Some(json!({"body": ""})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"]["body"][0], "The body field is required.");

    // Ubah isi: audit `updated`. `is_edited` baru true bila selisih lebih dari 2 detik.
    let (status, _, body) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu komentar revisi"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Komentar berhasil diperbarui");
    assert_eq!(body["data"]["body"], "uji-blogc-cu komentar revisi");
    assert_eq!(body["data"]["is_edited"], false);
    sqlx::query("UPDATE tbl_blog_comment SET created_at = NOW() - INTERVAL 1 HOUR WHERE id = ?")
        .bind(c)
        .execute(&pool)
        .await
        .unwrap();
    let (_, _, body) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu komentar revisi kedua"})),
    )
    .await;
    assert_eq!(body["data"]["is_edited"], true, "{body}");
    assert_eq!(
        audit_count(&pool, "App\\Models\\BlogComment", "updated", c).await,
        2
    );

    // Hapus oleh admin (bukan pemilik): lunak. Tampil sebagai terhapus.
    let (status, _, body) = send(&r, Method::DELETE, &uri, Some(&admin_token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Komentar berhasil dihapus");
    assert_eq!(
        audit_count(&pool, "App\\Models\\BlogComment", "deleted", c).await,
        1
    );

    let (status, _, body) = send(
        &r,
        Method::GET,
        &format!("/api/blog/uji-blogc-cu-pub/comments/thread/{c}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // Akar yang dihapus tidak tampil di daftar (roots query memakai default scope).
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cu-pub/comments",
        None,
        None,
    )
    .await;
    assert_eq!(body["data"], json!([]), "{body}");
    assert_eq!(body["meta"]["total"], 0);
    assert_eq!(body["meta"]["root_total"], 0);
    let (status, _, _) = send(
        &r,
        Method::PUT,
        &uri,
        Some(&commenter_token),
        Some(json!({"body": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(&r, Method::DELETE, &uri, Some(&admin_token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Balasan yang dihapus tetap tampil di bawah akar hidup, sebagai placeholder.
    let (_, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cu-pub/comments",
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu akar hidup"})),
    )
    .await;
    let root2 = body["data"]["id"].as_u64().unwrap();
    let (_, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-cu-pub/comments",
        Some(&commenter_token),
        Some(json!({"body": "uji-blogc-cu balasan dihapus", "parent_id": root2})),
    )
    .await;
    let reply2 = body["data"]["id"].as_u64().unwrap();
    let (status, _, body) = send(
        &r,
        Method::DELETE,
        &format!("/api/blog/comments/{reply2}"),
        Some(&commenter_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/uji-blogc-cu-pub/comments",
        None,
        None,
    )
    .await;
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{body}");
    let placeholder = rows.iter().find(|x| x["id"] == reply2).unwrap();
    assert_eq!(placeholder["is_deleted"], true);
    assert_eq!(placeholder["body"], Value::Null);
    assert_eq!(placeholder["user"], Value::Null);
    assert_eq!(placeholder["can_edit"], false);
    assert_eq!(body["meta"]["total"], 1, "{body}");
    assert_eq!(body["meta"]["root_total"], 1);

    // Moderasi: filter status dan pencarian.
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/comments?status=deleted",
        Some(&admin_token),
        None,
    )
    .await;
    let found: Vec<u64> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_u64().unwrap())
        .collect();
    assert!(found.contains(&c), "{body}");
    assert_eq!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == c)
            .unwrap()["body_preview"],
        Value::Null
    );
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/comments?status=active",
        Some(&admin_token),
        None,
    )
    .await;
    let found: Vec<u64> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_u64().unwrap())
        .collect();
    assert!(!found.contains(&c), "{body}");
    let (_, _, body) = send(
        &r,
        Method::GET,
        "/api/blog/comments?search=uji-blogc-cu&status=deleted",
        Some(&admin_token),
        None,
    )
    .await;
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == c),
        "{body}"
    );

    cleanup(&pool, tag).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn komentar_throttle_dan_kedalaman() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let tag = "ct";
    cleanup(&pool, tag).await;
    let _author_token = user_token(&pool, &email(tag, "author"), false).await;
    let author = user_id(&pool, &email(tag, "author")).await;
    let spammer_token = user_token(&pool, &email(tag, "spam"), false).await;
    let spammer = user_id(&pool, &email(tag, "spam")).await;
    insert_blog(&pool, "uji-blogc-ct-pub", author, true, false).await;
    // Satu router untuk seluruh tes: limiter ada di `AppState`.
    let r = router(&pool);

    // Sepuluh komentar per menit per pengguna, lalu 429 dengan `Retry-After`.
    let mut last = 0u64;
    for i in 1..=10 {
        let (status, _, body) = send(
            &r,
            Method::POST,
            "/api/blog/uji-blogc-ct-pub/comments",
            Some(&spammer_token),
            Some(json!({"body": format!("uji-blogc-ct pesan nomor {i}")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        last = body["data"]["id"].as_u64().unwrap();
    }
    let (status, headers, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-ct-pub/comments",
        Some(&spammer_token),
        Some(json!({"body": "uji-blogc-ct pesan kesebelas"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["message"], "Too Many Attempts.");
    assert!(headers.get("retry-after").is_some());

    // Pengguna lain tidak terpengaruh.
    let other_token = user_token(&pool, &email(tag, "other"), false).await;
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-ct-pub/comments",
        Some(&other_token),
        Some(json!({"body": "uji-blogc-ct dari pengguna lain"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Kedalaman: rantai 10 tingkat, lalu balasan ke tingkat 10 ditolak.
    let mut parent = last;
    for depth in 1..=10 {
        parent = sqlx::query(
            "INSERT INTO tbl_blog_comment (blog_id, user_id, parent_id, body, depth, created_at, updated_at) \
             SELECT blog_id, ?, ?, ?, ?, NOW(), NOW() FROM tbl_blog_comment WHERE id = ?",
        )
        .bind(spammer)
        .bind(parent)
        .bind(format!("uji-blogc-ct tingkat {depth}"))
        .bind(depth)
        .bind(last)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    }
    let (status, _, body) = send(
        &r,
        Method::POST,
        "/api/blog/uji-blogc-ct-pub/comments",
        Some(&other_token),
        Some(json!({"body": "uji-blogc-ct balasan terlalu dalam", "parent_id": parent})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["message"],
        "Balasan terlalu dalam (maksimal 10 level)."
    );

    cleanup(&pool, tag).await;
}
