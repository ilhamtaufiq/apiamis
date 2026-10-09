//! Rute `/api/user-drive` lewat router terhadap MySQL: daftar, folder, unggah berkas, detail, ganti nama,
//! bagikan, hapus, dan hapus massal. Termasuk aturan kepemilikan (403) dan validasi (422).
//!
//! Membutuhkan tabel `user_drive_items` dan `user_drive_shares` (fixture `rust/fixtures/user_drive_schema.sql`),
//! `media`, `tbl_audit_logs`, `users`, `roles`, `model_has_roles`, dan `personal_access_tokens`.
//! Berkas uji ditulis ke folder sementara (`PUBLIC_STORAGE_PATH`). Setiap tes memakai email dan nama
//! berawalan `uji-ud-<tes>`, memakai kata `search` sendiri, dan hanya menghapus baris miliknya.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test user_drive_db -- --include-ignored
//! ```

use std::path::PathBuf;

use api::{app, media, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    response::Response,
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const BOUNDARY: &str = "----ujiuddboundary";
const MODEL: &str = "App\\Models\\UserDriveItem";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

/// Folder penyimpanan media uji. Diatur sekali per proses agar tes paralel tidak saling menimpa env.
fn storage_dir() -> PathBuf {
    std::env::temp_dir().join(format!("uji-ud-{}", std::process::id()))
}

fn setup_storage() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("PUBLIC_STORAGE_PATH", storage_dir()));
}

async fn connect() -> MySqlPool {
    setup_storage();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn finish(res: Response) -> (StatusCode, Value) {
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Permintaan JSON (atau tanpa body bila `body` `None`). `token` `None` berarti tanpa header Authorization.
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
    finish(res).await
}

/// Body `multipart/form-data` dengan field teks dan satu berkas opsional.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

/// Unggahan `POST /api/user-drive/files` dengan body multipart yang sudah dibuat.
async fn send_multipart(pool: &MySqlPool, token: Option<&str>, body: Vec<u8>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/user-drive/files")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        );
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(Body::from(body)).unwrap())
    .await
    .unwrap();
    finish(res).await
}

/// Unggah satu berkas. `name` kosong dan `parent` `None` berarti tidak diisi.
async fn upload(
    pool: &MySqlPool,
    token: Option<&str>,
    name: &str,
    parent: Option<u64>,
    file_name: &str,
    bytes: &[u8],
) -> (StatusCode, Value) {
    let parent_s = parent.map(|p| p.to_string()).unwrap_or_default();
    let body = multipart(
        &[("name", name), ("parent_id", parent_s.as_str())],
        Some((file_name, bytes)),
    );
    send_multipart(pool, token, body).await
}

/// Pengguna uji, dengan peran `admin` bila diminta, lalu token Sanctum.
async fn make_user(pool: &MySqlPool, name: &str, email: &str, admin: bool) -> (u64, String) {
    cleanup(pool, &[email]).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(name)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
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
    let token = auth::login::create_token(pool, uid, "uji-ud").await.unwrap();
    (uid, token)
}

/// Buat folder lewat API dan kembalikan id-nya.
async fn make_folder(pool: &MySqlPool, token: &str, name: &str, parent: Option<u64>) -> u64 {
    let mut body = json!({ "name": name });
    if let Some(p) = parent {
        body["parent_id"] = json!(p);
    }
    let (status, res) = send(
        pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{res}");
    res["data"]["id"].as_u64().unwrap()
}

/// Hapus baris uji yang dibuat tes ini: berkas media, audit, item, token, peran, lalu akun.
/// Hanya email yang diberikan yang disentuh.
async fn cleanup(pool: &MySqlPool, emails: &[&str]) {
    for email in emails {
        let media_ids: Vec<u64> = sqlx::query_scalar(
            "SELECT id FROM media WHERE model_type = ? AND model_id IN \
             (SELECT i.id FROM user_drive_items i JOIN users u ON u.id = i.user_id WHERE u.email = ?)",
        )
        .bind(MODEL)
        .bind(email)
        .fetch_all(pool)
        .await
        .unwrap();
        for id in &media_ids {
            let _ = std::fs::remove_dir_all(media::media_dir(*id));
        }
        sqlx::query(
            "DELETE FROM media WHERE model_type = ? AND model_id IN \
             (SELECT i.id FROM user_drive_items i JOIN users u ON u.id = i.user_id WHERE u.email = ?)",
        )
        .bind(MODEL)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM tbl_audit_logs WHERE user_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_drive_items WHERE user_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM personal_access_tokens WHERE tokenable_id IN (SELECT id FROM users WHERE email = ?)")
            .bind(email)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
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

/// `true` bila `deleted_at` item terisi (baris tetap ada).
async fn is_deleted(pool: &MySqlPool, id: u64) -> bool {
    let v: i64 = sqlx::query_scalar(
        "SELECT CAST(deleted_at IS NOT NULL AS SIGNED) FROM user_drive_items WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    v == 1
}

/// Jumlah audit `tbl_audit_logs` untuk satu item dengan event tertentu.
async fn audit_count(pool: &MySqlPool, id: u64, event: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_audit_logs WHERE auditable_type = ? AND auditable_id = ? AND event = ?",
    )
    .bind(MODEL)
    .bind(id)
    .bind(event)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Jumlah baris `user_drive_shares` untuk item. `None` mencocokkan baris "semua user" (NULL).
async fn share_count(pool: &MySqlPool, item: u64, user: Option<u64>) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_drive_shares WHERE item_id = ? AND shared_to_user_id <=> ?",
    )
    .bind(item)
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Jumlah baris `media` dengan id tertentu.
async fn media_count(pool: &MySqlPool, media_id: u64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE id = ?")
        .bind(media_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Pesan 422 Laravel: `message` dan `errors.<field>[0]` sama dengan pesan yang diharapkan.
fn assert_422(status: StatusCode, body: &Value, field: &str, message: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["message"], message, "{body}");
    assert_eq!(body["errors"][field][0], message, "{body}");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tanpa_token_ditolak_401() {
    let pool = connect().await;

    let cases: [(Method, &str); 7] = [
        (Method::GET, "/api/user-drive"),
        (Method::POST, "/api/user-drive/folders"),
        (Method::DELETE, "/api/user-drive/bulk"),
        (Method::GET, "/api/user-drive/1"),
        (Method::PUT, "/api/user-drive/1"),
        (Method::DELETE, "/api/user-drive/1"),
        (Method::POST, "/api/user-drive/1/share"),
    ];
    for (method, uri) in cases {
        let (status, body) = send(&pool, method.clone(), uri, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}: {body}");
        assert_eq!(body["message"], "Unauthenticated.", "{method} {uri}");
    }

    let (status, body) = send_multipart(
        &pool,
        None,
        multipart(&[("name", "uji-ud-401")], Some(("uji.pdf", &b"%PDF-1.4"[..]))),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["message"], "Unauthenticated.");
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn buat_folder_dan_daftar_milik_sendiri() {
    let pool = connect().await;
    let email = "uji-ud-buat@example.test";
    let (uid, token) = make_user(&pool, "Uji UD Buat", email, false).await;
    let token = token.as_str();

    // Nama dipangkas spasinya; folder dibuat di root.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token),
        Some(json!({ "name": "  uji-ud-buat-induk  " })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let induk = &body["data"];
    let induk_id = induk["id"].as_u64().unwrap();
    assert_eq!(induk["name"], "uji-ud-buat-induk");
    assert_eq!(induk["kind"], "folder");
    assert_eq!(induk["parent_id"], Value::Null);
    assert_eq!(induk["is_owner"], true);
    assert_eq!(induk["can_manage"], true);
    assert_eq!(induk["shared_to_all"], false);
    assert_eq!(induk["file_url"], Value::Null);
    assert!(induk.get("owner").is_none(), "owner hanya ada di daftar");

    // Folder anak di dalam induk.
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token),
        Some(json!({ "name": "uji-ud-buat-anak", "parent_id": induk_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let anak_id = body["data"]["id"].as_u64().unwrap();
    assert_eq!(body["data"]["parent_id"], json!(induk_id));

    // Root hanya memuat induk; isi induk hanya memuat anak. `search` memisahkan data uji ini.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/user-drive?search=uji-ud-buat",
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], json!(induk_id));
    assert_eq!(body["data"][0]["owner"]["id"], json!(uid));
    assert_eq!(body["data"][0]["owner"]["name"], "Uji UD Buat");

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/user-drive?parent_id={induk_id}&search=uji-ud-buat"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], json!(anak_id));

    cleanup(&pool, &[email]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn unggah_berkas_nama_default_dan_nama_sendiri() {
    let pool = connect().await;
    let email = "uji-ud-unggah@example.test";
    let (_, token) = make_user(&pool, "Uji UD Unggah", email, false).await;
    let token = token.as_str();
    let folder = make_folder(&pool, token, "uji-ud-unggah-map", None).await;

    // Tanpa `name`: nama tampilan memakai nama berkas tanpa ekstensi.
    let isi: &[u8] = b"%PDF-1.4 uji unggah";
    let (status, body) = upload(&pool, Some(token), "", Some(folder), "laporan-uji.pdf", isi).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let file = &body["data"];
    let file_id = file["id"].as_u64().unwrap();
    assert_eq!(file["kind"], "file");
    assert_eq!(file["name"], "laporan-uji");
    assert_eq!(file["original_filename"], "laporan-uji.pdf");
    assert_eq!(file["parent_id"], json!(folder));
    assert_eq!(file["mime_type"], "application/pdf");
    assert_eq!(file["file_size"], json!(isi.len()));
    let media_id = file["media_id"].as_u64().unwrap();
    let url = file["file_url"].as_str().unwrap();
    assert!(url.starts_with("http://localhost/storage/"), "{url}");

    // Berkas tertulis di folder media dan nama berkasnya ada di URL.
    let file_name: String = sqlx::query_scalar("SELECT file_name FROM media WHERE id = ?")
        .bind(media_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(url.ends_with(&file_name), "{url} vs {file_name}");
    assert!(media::media_dir(media_id).join(&file_name).is_file());

    // Nama sendiri dipangkas; nama asli tetap tersimpan; tanpa induk berarti root.
    let (status, body) = upload(
        &pool,
        Some(token),
        "  Dokumen Akhir  ",
        None,
        "rekap.xlsx",
        b"xlsx uji",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["name"], "Dokumen Akhir");
    assert_eq!(body["data"]["original_filename"], "rekap.xlsx");
    assert_eq!(body["data"]["parent_id"], Value::Null);
    assert_eq!(
        body["data"]["mime_type"],
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );

    // Detail berkas sama dengan respon unggah.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/user-drive/{file_id}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["media_id"], json!(media_id));
    assert_eq!(body["data"]["file_size"], json!(isi.len()));

    // Pencarian menyentuh `original_filename`.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/user-drive?parent_id={folder}&search=laporan-uji"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], json!(file_id));

    cleanup(&pool, &[email]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn ganti_nama_dan_bagikan() {
    let pool = connect().await;
    let email_a = "uji-ud-bagi-a@example.test";
    let email_b = "uji-ud-bagi-b@example.test";
    let (uid_a, token_a) = make_user(&pool, "Uji UD Bagi A", email_a, false).await;
    let (uid_b, token_b) = make_user(&pool, "Uji UD Bagi B", email_b, false).await;
    let token_a = token_a.as_str();
    let token_b = token_b.as_str();
    let folder = make_folder(&pool, token_a, "uji-ud-bagi-map", None).await;

    // Ganti nama mengubah baris dan menulis satu audit `updated`.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/user-drive/{folder}"),
        Some(token_a),
        Some(json!({ "name": "uji-ud-bagi-baru" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], "uji-ud-bagi-baru");
    assert_eq!(audit_count(&pool, folder, "updated").await, 1);

    // Nama yang sama tidak menulis audit lagi.
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/user-drive/{folder}"),
        Some(token_a),
        Some(json!({ "name": "uji-ud-bagi-baru" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(audit_count(&pool, folder, "updated").await, 1);

    // Bagikan ke B dua kali: tetap satu baris (updateOrCreate).
    for _ in 0..2 {
        let (status, body) = send(
            &pool,
            Method::POST,
            &format!("/api/user-drive/{folder}/share"),
            Some(token_a),
            Some(json!({ "user_id": uid_b })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"]["shared_to_all"], false);
    }
    assert_eq!(share_count(&pool, folder, Some(uid_b)).await, 1);

    // B melihat item bagian dan pemiliknya, tetapi tidak boleh mengubahnya.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/user-drive?search=uji-ud-bagi",
        Some(token_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["id"], json!(folder));
    assert_eq!(body["data"][0]["is_owner"], false);
    assert_eq!(body["data"][0]["can_manage"], false);
    assert_eq!(body["data"][0]["owner"]["id"], json!(uid_a));

    // Bagikan ke semua user (tanpa user_id): baris NULL dan `shared_to_all`.
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/user-drive/{folder}/share"),
        Some(token_a),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["shared_to_all"], true);
    assert_eq!(share_count(&pool, folder, None).await, 1);

    cleanup(&pool, &[email_a, email_b]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn hapus_rekursif_folder_dan_berkas() {
    let pool = connect().await;
    let email = "uji-ud-hapus@example.test";
    let (_, token) = make_user(&pool, "Uji UD Hapus", email, false).await;
    let token = token.as_str();
    let induk = make_folder(&pool, token, "uji-ud-hapus-induk", None).await;
    let anak = make_folder(&pool, token, "uji-ud-hapus-anak", Some(induk)).await;

    let (status, body) = upload(&pool, Some(token), "", Some(anak), "isi.txt", b"isi uji").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let file = body["data"]["id"].as_u64().unwrap();
    let media_id = body["data"]["media_id"].as_u64().unwrap();
    assert!(media::media_dir(media_id).is_dir(), "folder media harus ada sebelum hapus");

    // Hapus induk: anak dan berkas ikut terhapus lunak, media dan foldernya dibersihkan.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/user-drive/{induk}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["message"], "Item drive berhasil dihapus");
    assert_eq!(body["deleted"], 3);

    for id in [induk, anak, file] {
        assert!(is_deleted(&pool, id).await, "item {id} harus terhapus lunak");
        assert_eq!(audit_count(&pool, id, "deleted").await, 1, "audit deleted {id}");
    }
    assert_eq!(media_count(&pool, media_id).await, 0);
    assert!(!media::media_dir(media_id).exists(), "folder media harus dihapus");

    // Detail induk sudah 404 dengan pesan model Laravel.
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/user-drive/{induk}"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["message"],
        format!("No query results for model [{MODEL}] {induk}")
    );

    cleanup(&pool, &[email]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn bulk_hapus_hanya_item_yang_bisa_dikelola() {
    let pool = connect().await;
    let email_a = "uji-ud-bulk-a@example.test";
    let email_b = "uji-ud-bulk-b@example.test";
    let (_, token_a) = make_user(&pool, "Uji UD Bulk A", email_a, false).await;
    let (_, token_b) = make_user(&pool, "Uji UD Bulk B", email_b, false).await;
    let token_a = token_a.as_str();
    let token_b = token_b.as_str();
    let milik_a = make_folder(&pool, token_a, "uji-ud-bulk-a1", None).await;
    let milik_b = make_folder(&pool, token_b, "uji-ud-bulk-b1", None).await;

    // Item milik B dilewati; hanya milik A yang terhapus.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token_a),
        Some(json!({ "ids": [milik_a, milik_b] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], 1);
    assert_eq!(body["message"], "1 item drive dihapus");
    assert!(is_deleted(&pool, milik_a).await);
    assert!(!is_deleted(&pool, milik_b).await, "item B tidak boleh terhapus");

    // Bila tidak ada yang bisa dikelola: 404.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token_a),
        Some(json!({ "ids": [milik_b] })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["message"], "Item drive tidak ditemukan");

    cleanup(&pool, &[email_a, email_b]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn non_pemilik_ditolak_403() {
    let pool = connect().await;
    let email_a = "uji-ud-403-a@example.test";
    let email_b = "uji-ud-403-b@example.test";
    let (uid_a, token_a) = make_user(&pool, "Uji UD 403 A", email_a, false).await;
    let (_, token_b) = make_user(&pool, "Uji UD 403 B", email_b, false).await;
    let token_a = token_a.as_str();
    let token_b = token_b.as_str();
    let folder = make_folder(&pool, token_a, "uji-ud-403-map", None).await;
    let uri = format!("/api/user-drive/{folder}");

    // B tidak boleh melihat detail, mengganti nama, membagikan, atau menghapus item A.
    let (status, body) = send(&pool, Method::GET, &uri, Some(token_b), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = send(
        &pool,
        Method::PUT,
        &uri,
        Some(token_b),
        Some(json!({ "name": "uji-ud-403-diubah" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("{uri}/share"),
        Some(token_b),
        Some(json!({ "user_id": uid_a })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Forbidden");

    let (status, body) = send(&pool, Method::DELETE, &uri, Some(token_b), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "Forbidden");

    // Tidak ada yang berubah: nama tetap, item tidak terhapus, dan tidak ada share baru.
    let (status, body) = send(&pool, Method::GET, &uri, Some(token_a), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], "uji-ud-403-map");
    assert!(!is_deleted(&pool, folder).await);
    assert_eq!(share_count(&pool, folder, Some(uid_a)).await, 0);
    assert_eq!(audit_count(&pool, folder, "updated").await, 0);

    cleanup(&pool, &[email_a, email_b]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn admin_bisa_mengubah_item_user_lain() {
    let pool = connect().await;
    let email_a = "uji-ud-admin-a@example.test";
    let email_admin = "uji-ud-admin-root@example.test";
    let (_, token_a) = make_user(&pool, "Uji UD Admin A", email_a, false).await;
    let (_, token_admin) = make_user(&pool, "Uji UD Admin Root", email_admin, true).await;
    let token_a = token_a.as_str();
    let token_admin = token_admin.as_str();
    let folder = make_folder(&pool, token_a, "uji-ud-admin-map", None).await;
    let uri = format!("/api/user-drive/{folder}");

    // Admin melihat item user lain di daftar, dan canManage bernilai true.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/user-drive?search=uji-ud-admin",
        Some(token_admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["is_owner"], false);
    assert_eq!(body["data"][0]["can_manage"], true);

    let (status, body) = send(&pool, Method::GET, &uri, Some(token_admin), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send(
        &pool,
        Method::PUT,
        &uri,
        Some(token_admin),
        Some(json!({ "name": "uji-ud-admin-diubah" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], "uji-ud-admin-diubah");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token_admin),
        Some(json!({ "ids": [folder] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["deleted"], 1);
    assert!(is_deleted(&pool, folder).await);

    cleanup(&pool, &[email_a, email_admin]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_422_folder_nama_dan_induk() {
    let pool = connect().await;
    let email_a = "uji-ud-val-a@example.test";
    let email_b = "uji-ud-val-b@example.test";
    let (_, token_a) = make_user(&pool, "Uji UD Val A", email_a, false).await;
    let (_, token_b) = make_user(&pool, "Uji UD Val B", email_b, false).await;
    let token_a = token_a.as_str();
    let token_b = token_b.as_str();
    let folder_a = make_folder(&pool, token_a, "uji-ud-val-map", None).await;
    let folder_b = make_folder(&pool, token_b, "uji-ud-val-map-b", None).await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token_a),
        Some(json!({ "name": "   " })),
    )
    .await;
    assert_422(status, &body, "name", "The name field is required.");

    let panjang = "a".repeat(256);
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token_a),
        Some(json!({ "name": panjang })),
    )
    .await;
    assert_422(
        status,
        &body,
        "name",
        "The name field must not be greater than 255 characters.",
    );

    // Induk milik user lain dianggap tidak valid (exists dengan where user_id).
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token_a),
        Some(json!({ "name": "uji-ud-val-anak", "parent_id": folder_b })),
    )
    .await;
    assert_422(status, &body, "parent_id", "The selected parent id is invalid.");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/user-drive/folders",
        Some(token_a),
        Some(json!({ "name": "uji-ud-val-anak", "parent_id": "abc" })),
    )
    .await;
    assert_422(status, &body, "parent_id", "The parent id field must be an integer.");

    // Ganti nama kosong: validasi dulu (422), sebelum cek kepemilikan (403).
    let (status, body) = send(
        &pool,
        Method::PUT,
        &format!("/api/user-drive/{folder_a}"),
        Some(token_b),
        Some(json!({ "name": "" })),
    )
    .await;
    assert_422(status, &body, "name", "The name field is required.");

    cleanup(&pool, &[email_a, email_b]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn validasi_422_berkas_bagikan_dan_bulk() {
    let pool = connect().await;
    let email = "uji-ud-val2@example.test";
    let (_, token) = make_user(&pool, "Uji UD Val2", email, false).await;
    let token = token.as_str();

    // Berkas wajib ada.
    let (status, body) = send_multipart(
        &pool,
        Some(token),
        multipart(&[("name", "uji-ud-val2-tanpa-berkas")], None),
    )
    .await;
    assert_422(status, &body, "file", "The file field is required.");

    // Pembagian ke user yang tidak ada.
    let folder = make_folder(&pool, token, "uji-ud-val2-map", None).await;
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/user-drive/{folder}/share"),
        Some(token),
        Some(json!({ "user_id": 999_999_999 })),
    )
    .await;
    assert_422(status, &body, "user_id", "The selected user id is invalid.");

    // Hapus massal: ids wajib, berupa array, dan setiap elemen integer.
    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token),
        Some(json!({ "ids": [] })),
    )
    .await;
    assert_422(status, &body, "ids", "The ids field is required.");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token),
        Some(json!({ "ids": "1" })),
    )
    .await;
    assert_422(status, &body, "ids", "The ids field must be an array.");

    let (status, body) = send(
        &pool,
        Method::DELETE,
        "/api/user-drive/bulk",
        Some(token),
        Some(json!({ "ids": [folder, "x"] })),
    )
    .await;
    assert_422(status, &body, "ids.1", "The ids.1 field must be an integer.");

    // Parameter daftar: per_page di atas 100.
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/user-drive?per_page=101",
        Some(token),
        None,
    )
    .await;
    assert_422(
        status,
        &body,
        "per_page",
        "The per page field must not be greater than 100.",
    );

    cleanup(&pool, &[email]).await;
}
