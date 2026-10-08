//! Tes modul notifikasi. Bagian tanpa database berjalan biasa; bagian yang butuh MySQL
//! ditandai `ignored` dan dijalankan dengan `--include-ignored`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test notifications_db -- --include-ignored
//! ```

use api::notifications::{
    delete_broadcast_for, history_page, list_page, mark_all_read, mark_read, parse_broadcast,
    require_admin, send_broadcast_for, unread_count, unread_list,
};
use api::notify::new_uuid;
use api::pagination::{self, PageParams};
use serde_json::{json, Value};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

/// Semua tes DB memakai satu database dan membersihkan data ber-prefix yang sama,
/// jadi dijalankan satu per satu meskipun `cargo test` berjalan paralel.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

const EMAIL_PREFIX: &str = "uji-notif-";
const TITLE_PREFIX: &str = "Uji Notif";
/// Nilai yang ditulis Laravel. Diikat sebagai parameter: backslash di literal SQL akan di-escape MySQL.
const APP_USER: &str = r"App\Models\User";
const APP_NOTIFICATION: &str = r"App\Notifications\AppNotification";

async fn insert_user(pool: &MySqlPool, slug: &str) -> u64 {
    let email = format!("{EMAIL_PREFIX}{slug}@uji.test");
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(format!("Uji Notif {slug}"))
        .bind(&email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// User dengan role `admin`.
async fn insert_admin(pool: &MySqlPool, slug: &str) -> u64 {
    let uid = insert_user(pool, slug).await;
    let role_id: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin'")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)",
    )
    .bind(role_id)
    .bind(APP_USER)
    .bind(uid)
    .execute(pool)
    .await
    .unwrap();
    uid
}

/// Notifikasi biasa (tanpa `broadcast_history_id`), opsional sudah dibaca.
async fn insert_notification(pool: &MySqlPool, uid: u64, title: &str, read: bool) -> String {
    let id = new_uuid();
    let data = json!({
        "title": title,
        "message": "pesan uji",
        "url": null,
        "type": "info",
        "is_banner": false,
        "broadcast_history_id": null,
    })
    .to_string();
    sqlx::query("INSERT INTO notifications (id, type, notifiable_type, notifiable_id, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, NOW(), NOW())")
        .bind(&id)
        .bind(APP_NOTIFICATION)
        .bind(APP_USER)
        .bind(uid)
        .bind(&data)
        .execute(pool)
        .await
        .unwrap();
    if read {
        sqlx::query("UPDATE notifications SET read_at = NOW() WHERE id = ?")
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
    }
    id
}

/// Hapus semua data uji: notifikasi (termasuk hasil broadcast ke semua user), history,
/// role, dan user ber-prefix email `uji-notif-`.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query(
        "DELETE FROM notifications WHERE JSON_UNQUOTE(JSON_EXTRACT(data, '$.\"broadcast_history_id\"')) \
         IN (SELECT id FROM broadcast_histories WHERE title LIKE 'Uji Notif%')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM broadcast_histories WHERE title LIKE 'Uji Notif%'")
        .execute(pool)
        .await
        .unwrap();
    let like = format!("{EMAIL_PREFIX}%");
    sqlx::query("DELETE FROM notifications WHERE notifiable_type = ? AND notifiable_id IN (SELECT id FROM users WHERE email LIKE ?)")
        .bind(APP_USER)
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM model_has_roles WHERE model_type = ? AND model_id IN (SELECT id FROM users WHERE email LIKE ?)")
        .bind(APP_USER)
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email LIKE ?")
        .bind(&like)
        .execute(pool)
        .await
        .unwrap();
}

fn admin_input(kind: &str, user_ids: Option<Vec<Value>>) -> Value {
    let mut v = json!({
        "title": format!("{TITLE_PREFIX} judul"),
        "message": "isi uji",
        "type": kind,
    });
    if let Some(ids) = user_ids {
        v["user_ids"] = Value::Array(ids);
    }
    v
}

// ---------------------------------------------------------------------------
// Validasi (tanpa database)
// ---------------------------------------------------------------------------

#[test]
fn validation_reports_required_fields_for_empty_body() {
    let errors = parse_broadcast(&json!({})).unwrap_err();
    for key in ["title", "message", "type"] {
        assert!(errors.contains_key(key), "harus ada error untuk {key}");
    }
    assert!(
        !errors.contains_key("user_ids"),
        "type tidak ada, user_ids tidak wajib"
    );
    assert_eq!(errors["title"], vec!["The title field is required."]);
}

#[test]
fn single_and_multiple_require_non_empty_user_ids() {
    for kind in ["single", "multiple"] {
        let missing = parse_broadcast(&admin_input(kind, None)).unwrap_err();
        assert_eq!(
            missing["user_ids"],
            vec!["The user ids field is required when type is single, multiple."]
        );
        let empty = parse_broadcast(&admin_input(kind, Some(vec![]))).unwrap_err();
        assert!(empty.contains_key("user_ids"));
    }
}

#[test]
fn all_does_not_require_user_ids_and_applies_defaults() {
    let b = parse_broadcast(&admin_input("all", None)).unwrap();
    assert_eq!(b.kind, "all");
    assert!(b.user_ids.is_empty());
    assert_eq!(b.notification_type, "info");
    assert_eq!(b.url, None);
    assert!(!b.is_banner);
}

#[test]
fn empty_strings_are_treated_as_null() {
    let mut input = admin_input("all", None);
    input["url"] = json!("");
    input["notification_type"] = json!("");
    input["is_banner"] = json!("");
    let b = parse_broadcast(&input).unwrap();
    assert_eq!(b.url, None);
    assert_eq!(b.notification_type, "info");
    assert!(!b.is_banner);
}

#[test]
fn is_banner_accepts_laravel_boolean_values_only() {
    for (raw, expected) in [
        (json!(true), true),
        (json!(false), false),
        (json!(1), true),
        (json!(0), false),
        (json!("1"), true),
        (json!("0"), false),
    ] {
        let mut input = admin_input("all", None);
        input["is_banner"] = raw.clone();
        assert_eq!(
            parse_broadcast(&input).unwrap().is_banner,
            expected,
            "{raw}"
        );
    }
    let mut input = admin_input("all", None);
    input["is_banner"] = json!("yes");
    assert!(parse_broadcast(&input)
        .unwrap_err()
        .contains_key("is_banner"));
}

#[test]
fn enum_and_length_rules() {
    let mut input = admin_input("all", None);
    input["type"] = json!("everyone");
    input["notification_type"] = json!("urgent");
    input["title"] = json!("x".repeat(256));
    input["url"] = json!("u".repeat(256));
    let errors = parse_broadcast(&input).unwrap_err();
    assert_eq!(errors["type"], vec!["The selected type is invalid."]);
    assert_eq!(
        errors["notification_type"],
        vec!["The selected notification type is invalid."]
    );
    assert!(errors.contains_key("title"));
    assert!(errors.contains_key("url"));
}

#[test]
fn title_limit_counts_characters_not_bytes() {
    let mut input = admin_input("all", None);
    input["title"] = json!("é".repeat(255));
    assert!(parse_broadcast(&input).is_ok());
}

#[test]
fn user_ids_must_be_integers_and_array() {
    let errors =
        parse_broadcast(&admin_input("single", Some(vec![json!("abc"), json!(7)]))).unwrap_err();
    assert_eq!(
        errors["user_ids.0"],
        vec!["The selected user_ids.0 is invalid."]
    );
    assert!(!errors.contains_key("user_ids.1"));

    let mut input = admin_input("single", None);
    input["user_ids"] = json!("7");
    assert_eq!(
        parse_broadcast(&input).unwrap_err()["user_ids"],
        vec!["The user ids field must be an array."]
    );
}

// ---------------------------------------------------------------------------
// Notifikasi milik user (database)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn index_paginates_and_scopes_to_user() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-index").await;
    let b = insert_user(&pool, "b-index").await;
    for i in 0..25 {
        insert_notification(&pool, a, &format!("{TITLE_PREFIX} a{i}"), false).await;
    }
    insert_notification(&pool, b, &format!("{TITLE_PREFIX} b"), false).await;

    let (rows, total) = list_page(&pool, a, 1, 20).await.unwrap();
    assert_eq!(total, 25);
    assert_eq!(rows.len(), 20);
    let base = "http://x/api/notifications";
    let body = pagination::paginate(
        rows,
        total,
        PageParams {
            page: 1,
            per_page: 20,
        },
        base,
    );
    assert_eq!(body["meta"]["last_page"], 2);
    assert_eq!(body["meta"]["total"], 25);

    let (rows2, _) = list_page(&pool, a, 2, 20).await.unwrap();
    assert_eq!(rows2.len(), 5);
    assert!(rows2.iter().all(|n| n["notifiable_id"] == a));
    assert_eq!(rows2[0]["data"]["message"], "pesan uji");
    assert!(rows2[0]["read_at"].is_null());

    assert_eq!(unread_count(&pool, a).await.unwrap(), 25);
    assert_eq!(unread_count(&pool, b).await.unwrap(), 1);
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn unread_list_is_capped_and_excludes_read_rows() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-unread").await;
    for i in 0..55 {
        insert_notification(&pool, a, &format!("{TITLE_PREFIX} u{i}"), false).await;
    }
    for i in 0..3 {
        insert_notification(&pool, a, &format!("{TITLE_PREFIX} r{i}"), true).await;
    }
    let rows = unread_list(&pool, a).await.unwrap();
    assert_eq!(rows.len(), 50, "take(50)");
    assert!(rows.iter().all(|n| n["read_at"].is_null()));
    assert_eq!(unread_count(&pool, a).await.unwrap(), 55);
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn mark_read_only_own_and_keeps_first_read_time() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-read").await;
    let b = insert_user(&pool, "b-read").await;
    let own = insert_notification(&pool, a, &format!("{TITLE_PREFIX} own"), false).await;
    let other = insert_notification(&pool, b, &format!("{TITLE_PREFIX} other"), false).await;

    mark_read(&pool, a, &own).await.unwrap();
    let read_at: Option<String> =
        sqlx::query_scalar("SELECT CAST(read_at AS CHAR) FROM notifications WHERE id = ?")
            .bind(&own)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(read_at.is_some());
    assert_eq!(unread_count(&pool, a).await.unwrap(), 0);

    // Panggilan kedua tidak mengubah read_at (markAsRead hanya menyimpan bila masih null).
    mark_read(&pool, a, &own).await.unwrap();
    let again: Option<String> =
        sqlx::query_scalar("SELECT CAST(read_at AS CHAR) FROM notifications WHERE id = ?")
            .bind(&own)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(read_at, again);

    // Notifikasi milik user lain: 500 "Server Error", seperti Laravel.
    let err = mark_read(&pool, a, &other).await.unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(err.body(), json!({ "message": "Server Error" }));
    assert_eq!(unread_count(&pool, b).await.unwrap(), 1);
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn mark_all_read_touches_only_own_unread_rows() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-all").await;
    let b = insert_user(&pool, "b-all").await;
    for i in 0..3 {
        insert_notification(&pool, a, &format!("{TITLE_PREFIX} a{i}"), false).await;
    }
    insert_notification(&pool, a, &format!("{TITLE_PREFIX} a-read"), true).await;
    insert_notification(&pool, b, &format!("{TITLE_PREFIX} b"), false).await;

    mark_all_read(&pool, a).await.unwrap();
    assert_eq!(unread_count(&pool, a).await.unwrap(), 0);
    assert_eq!(unread_count(&pool, b).await.unwrap(), 1);
    cleanup(&pool).await;
}

// ---------------------------------------------------------------------------
// Broadcast (database)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn require_admin_rejects_non_admin() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let plain = insert_user(&pool, "plain").await;
    let admin = insert_admin(&pool, "admin").await;
    assert_eq!(
        require_admin(&pool, plain).await.unwrap_err().status,
        axum::http::StatusCode::FORBIDDEN
    );
    require_admin(&pool, admin).await.unwrap();
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn broadcast_to_selected_users_then_delete_removes_only_its_rows() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-bc").await;
    let b = insert_user(&pool, "b-bc").await;
    let c = insert_user(&pool, "c-bc").await;
    let plain_row = insert_notification(&pool, a, &format!("{TITLE_PREFIX} biasa"), false).await;

    let mut input = admin_input("multiple", Some(vec![json!(a), json!(b), json!(a)]));
    input["notification_type"] = json!("warning");
    input["url"] = json!("/pekerjaan");
    input["is_banner"] = json!(true);
    let body = send_broadcast_for(&pool, &input).await.unwrap();
    assert_eq!(body["message"], "Notification broadcasted successfully");
    assert_eq!(
        body["recipient_count"], 2,
        "id duplikat hanya dihitung sekali"
    );

    let hid: u64 = sqlx::query_scalar("SELECT id FROM broadcast_histories WHERE title = ?")
        .bind(format!("{TITLE_PREFIX} judul"))
        .fetch_one(&pool)
        .await
        .unwrap();
    let hist = sqlx::query("SELECT type, notification_type, url, is_banner, recipient_count FROM broadcast_histories WHERE id = ?")
        .bind(hid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hist.try_get::<String, _>("type").unwrap(), "multiple");
    assert_eq!(
        hist.try_get::<String, _>("notification_type").unwrap(),
        "warning"
    );
    assert_eq!(
        hist.try_get::<Option<String>, _>("url").unwrap().as_deref(),
        Some("/pekerjaan")
    );
    assert!(hist.try_get::<bool, _>("is_banner").unwrap());
    assert_eq!(hist.try_get::<i32, _>("recipient_count").unwrap(), 2);

    let rows = unread_list(&pool, a).await.unwrap();
    let mine: Vec<&Value> = rows
        .iter()
        .filter(|n| n["data"]["broadcast_history_id"] == hid)
        .collect();
    assert_eq!(mine.len(), 1);
    let data = &mine[0]["data"];
    assert_eq!(data["title"], format!("{TITLE_PREFIX} judul"));
    assert_eq!(
        data["type"], "warning",
        "kunci type berisi notification_type"
    );
    assert_eq!(data["url"], "/pekerjaan");
    assert_eq!(data["is_banner"], true);
    assert_eq!(mine[0]["type"], r"App\Notifications\AppNotification");
    assert_eq!(mine[0]["notifiable_type"], r"App\Models\User");

    // Notifikasi biasa tidak ikut terhapus.
    assert_eq!(unread_count(&pool, c).await.unwrap(), 0);
    delete_broadcast_for(&pool, &hid.to_string()).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE data LIKE ?")
        .bind(format!("%\"broadcast_history_id\":{hid}%"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE id = ?")
        .bind(&plain_row)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 1);

    let err = delete_broadcast_for(&pool, &hid.to_string())
        .await
        .unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn broadcast_rejects_unknown_user_ids_with_422() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-unknown").await;
    let missing = 4_000_000_000_u64;
    let input = admin_input("single", Some(vec![json!(a), json!(missing)]));
    let err = send_broadcast_for(&pool, &input).await.unwrap_err();
    assert_eq!(err.status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(err.message, "Validation error");
    let errors = err.errors.unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors.contains_key("user_ids.1"));
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM broadcast_histories WHERE title LIKE 'Uji Notif%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "tidak ada history bila validasi gagal");
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn broadcast_all_reaches_every_user_and_delete_cleans_up() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    let body = send_broadcast_for(&pool, &admin_input("all", None))
        .await
        .unwrap();
    assert_eq!(body["recipient_count"], users);

    let hid: u64 = sqlx::query_scalar("SELECT id FROM broadcast_histories WHERE title = ?")
        .bind(format!("{TITLE_PREFIX} judul"))
        .fetch_one(&pool)
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE data LIKE ?")
        .bind(format!("%\"broadcast_history_id\":{hid}%"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, users);

    delete_broadcast_for(&pool, &hid.to_string()).await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE data LIKE ?")
        .bind(format!("%\"broadcast_history_id\":{hid}%"))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn broadcast_history_is_latest_first_and_paginated_by_ten() {
    let _serial = serial().await;
    let pool = pool().await;
    cleanup(&pool).await;
    let a = insert_user(&pool, "a-hist").await;
    for i in 0..11 {
        let mut input = admin_input("single", Some(vec![json!(a)]));
        input["title"] = json!(format!("{TITLE_PREFIX} hist {i}"));
        send_broadcast_for(&pool, &input).await.unwrap();
    }
    let (page1, total) = history_page(&pool, 1, 10).await.unwrap();
    assert!(total >= 11);
    assert_eq!(page1.len(), 10);
    let (page2, _) = history_page(&pool, 2, 10).await.unwrap();
    assert!(!page2.is_empty());
    let ids1: Vec<&Value> = page1.iter().map(|h| &h["id"]).collect();
    assert!(page2.iter().all(|h| !ids1.contains(&&h["id"])));
    assert_eq!(
        page1[0]["title"],
        format!("{TITLE_PREFIX} hist 10"),
        "terbaru dulu"
    );
    assert_eq!(page1[0]["is_banner"], false);
    assert_eq!(page1[0]["recipient_count"], 1);
    cleanup(&pool).await;
}
