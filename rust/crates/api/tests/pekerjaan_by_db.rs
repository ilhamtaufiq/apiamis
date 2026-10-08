//! Daftar pekerjaan per kecamatan, desa, kegiatan, total pagu, dan media lewat router terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_by_db -- --include-ignored
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

const MARK: &str = "uji-pby-";

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

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(
        Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::USER_AGENT, "uji-agent")
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

/// User baru dengan satu role; email unik per tes.
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

/// Hapus sisa tes sebelumnya dengan awalan `tag` sendiri (tidak menyentuh data tes lain).
async fn purge_stale(pool: &MySqlPool, tag: &str) {
    let like = format!("{MARK}{tag}%");
    retry_db(|| {
        let like = like.clone();
        async move {
    sqlx::query("DELETE FROM tbl_berkas WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)")
        .bind(&like)
        .execute(pool)
        .await
        }
    })
    .await;
    retry_db(|| {
        let like = like.clone();
        async move {
    sqlx::query("DELETE FROM user_pekerjaan WHERE pekerjaan_id IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)")
        .bind(&like)
        .execute(pool)
        .await
        }
    })
    .await;
    retry_db(|| {
        let like = like.clone();
        async move {
            sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE ?")
                .bind(&like)
                .execute(pool)
                .await
        }
    })
    .await;
    retry_db(|| {
        let like = like.clone();
        async move {
            sqlx::query("DELETE FROM tbl_desa WHERE n_desa LIKE ?")
                .bind(&like)
                .execute(pool)
                .await
        }
    })
    .await;
    retry_db(|| {
        let like = like.clone();
        async move {
            sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE ?")
                .bind(&like)
                .execute(pool)
                .await
        }
    })
    .await;
}

/// Kecamatan dengan dua desa khusus tes. `tag` unik per tes.
async fn seed_region(pool: &MySqlPool, tag: &str) -> (u64, u64, u64) {
    purge_stale(pool, tag).await;
    let kec = sqlx::query(
        "INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())",
    )
    .bind(format!("{MARK}{tag}-kec"))
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    let mut desa = Vec::new();
    for n in ["satu", "dua"] {
        let id = sqlx::query("INSERT INTO tbl_desa (n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(format!("{MARK}{tag}-desa-{n}"))
            .bind(kec)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
        desa.push(id);
    }
    (kec, desa[0], desa[1])
}

/// Pekerjaan langsung lewat SQL; hanya dipakai untuk menyiapkan data tes. `nama` memuat tag tes.
async fn insert_pekerjaan(
    pool: &MySqlPool,
    nama: &str,
    kec: u64,
    desa: u64,
    kegiatan: u64,
    pagu: f64,
) -> u64 {
    sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 0, 'active', NOW(), NOW())")
        .bind(format!("{MARK}{nama}"))
        .bind(kec)
        .bind(desa)
        .bind(kegiatan)
        .bind(pagu)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn cleanup(pool: &MySqlPool, pekerjaan: &[u64], kec: u64, desa: &[u64], users: &[u64]) {
    for id in pekerjaan {
        retry_db(|| async move {
            sqlx::query("DELETE FROM tbl_berkas WHERE pekerjaan_id = ?")
                .bind(id)
                .execute(pool)
                .await
        })
        .await;
        retry_db(|| async move {
            sqlx::query("DELETE FROM user_pekerjaan WHERE pekerjaan_id = ?")
                .bind(id)
                .execute(pool)
                .await
        })
        .await;
        retry_db(|| async move {
            sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
        })
        .await;
    }
    for d in desa {
        retry_db(|| async move {
            sqlx::query("DELETE FROM tbl_desa WHERE id = ?")
                .bind(d)
                .execute(pool)
                .await
        })
        .await;
    }
    retry_db(|| async move {
        sqlx::query("DELETE FROM tbl_kecamatan WHERE id = ?")
            .bind(kec)
            .execute(pool)
            .await
    })
    .await;
    for u in users {
        retry_db(|| async move {
            sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
                .bind(u)
                .execute(pool)
                .await
        })
        .await;
    }
}

/// Satu kegiatan yang sudah ada di DB lokal, dan tahunnya.
async fn any_kegiatan(pool: &MySqlPool) -> (u64, String) {
    let row = sqlx::query("SELECT id, tahun_anggaran FROM tbl_kegiatan ORDER BY id LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    let id: u64 = row.try_get("id").unwrap();
    let tahun: String = row
        .try_get::<String, _>("tahun_anggaran")
        .unwrap_or_else(|_| row.try_get::<i64, _>("tahun_anggaran").unwrap().to_string());
    (id, tahun)
}

fn pekerjaan_ids(body: &Value) -> Vec<u64> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_u64().unwrap())
        .collect()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn lists_by_region_and_kegiatan_with_pagination_and_tahun() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = make_user(&pool, "uji-pby-a1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, admin, "uji-pby")
        .await
        .unwrap();
    let (kegiatan, tahun) = any_kegiatan(&pool).await;
    let (kec, d1, d2) = seed_region(&pool, "list").await;
    let p1 = insert_pekerjaan(&pool, "list-1", kec, d1, kegiatan, 100.0).await;
    let p2 = insert_pekerjaan(&pool, "list-2", kec, d1, kegiatan, 200.0).await;
    let p3 = insert_pekerjaan(&pool, "list-3", kec, d2, kegiatan, 300.0).await;

    let (status, body) = get(&pool, &format!("/api/pekerjaan/kecamatan/{kec}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 3);
    assert_eq!(pekerjaan_ids(&body), vec![p1, p2, p3], "urut id naik");

    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/kecamatan/{kec}/desa/{d1}"),
        &token,
    )
    .await;
    assert_eq!(pekerjaan_ids(&body), vec![p1, p2]);

    let (_, body) = get(&pool, &format!("/api/pekerjaan/desa/{d2}"), &token).await;
    assert_eq!(pekerjaan_ids(&body), vec![p3]);

    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/kecamatan/{kec}?tahun={tahun}"),
        &token,
    )
    .await;
    assert_eq!(body["meta"]["total"], 3, "tahun kegiatan cocok");
    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/kecamatan/{kec}?tahun=0001"),
        &token,
    )
    .await;
    assert_eq!(body["meta"]["total"], 0, "tahun yang tidak ada");

    // Kegiatan ini bisa punya banyak pekerjaan, jadi telusuri semua halaman.
    let (_, first) = get(
        &pool,
        &format!("/api/pekerjaan/kegiatan/{kegiatan}"),
        &token,
    )
    .await;
    let last_page = first["meta"]["last_page"].as_u64().unwrap();
    let mut ids = Vec::new();
    for page in 1..=last_page {
        let (_, body) = get(
            &pool,
            &format!("/api/pekerjaan/kegiatan/{kegiatan}?page={page}"),
            &token,
        )
        .await;
        ids.extend(pekerjaan_ids(&body));
    }
    assert!(ids.contains(&p1) && ids.contains(&p2) && ids.contains(&p3));

    let (_, body) = get(&pool, &format!("/api/pekerjaan/kecamatan/{kec}"), &token).await;
    assert_eq!(body["meta"]["last_page"], 1);
    assert_eq!(body["meta"]["per_page"], 20);

    let (status, body) = get(
        &pool,
        &format!("/api/pekerjaan/stats/pagu-kecamatan/{kec}"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kecamatan_id"], kec.to_string());
    assert_eq!(body["total_pagu"], 600);

    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/stats/pagu-kecamatan/{}", kec + 999_999),
        &token,
    )
    .await;
    assert_eq!(body["total_pagu"], 0, "tanpa pekerjaan");

    let expected: f64 = sqlx::query_scalar(
        "SELECT CAST(SUM(pagu) AS DOUBLE) FROM tbl_pekerjaan WHERE kegiatan_id = ?",
    )
    .bind(kegiatan)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/stats/pagu-kegiatan/{kegiatan}"),
        &token,
    )
    .await;
    assert_eq!(body["kegiatan_id"], kegiatan.to_string());
    assert!((body["total_pagu"].as_f64().unwrap() - expected).abs() < 0.01);

    cleanup(&pool, &[p1, p2, p3], kec, &[d1, d2], &[admin]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn scope_limits_lists_stats_and_media_to_assigned_pekerjaan() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let user = make_user(&pool, "uji-pby-u1@example.test", "user").await;
    let token = auth::login::create_token(&pool, user, "uji-pby")
        .await
        .unwrap();
    let (kegiatan, _) = any_kegiatan(&pool).await;
    let (kec, d1, _) = seed_region(&pool, "scope").await;
    let p1 = insert_pekerjaan(&pool, "scope-1", kec, d1, kegiatan, 50.0).await;
    let p2 = insert_pekerjaan(&pool, "scope-2", kec, d1, kegiatan, 70.0).await;

    let (status, body) = get(&pool, &format!("/api/pekerjaan/kecamatan/{kec}"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["meta"]["total"], 0,
        "tanpa assignment tidak melihat apa pun"
    );
    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/stats/pagu-kecamatan/{kec}"),
        &token,
    )
    .await;
    assert_eq!(body["total_pagu"], 0);
    let (status, _) = get(&pool, &format!("/api/pekerjaan/{p1}/media"), &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(user)
        .bind(p2)
        .execute(&pool)
        .await
        .unwrap();

    let (_, body) = get(&pool, &format!("/api/pekerjaan/kecamatan/{kec}"), &token).await;
    assert_eq!(
        pekerjaan_ids(&body),
        vec![p2],
        "hanya pekerjaan yang di-assign"
    );
    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/stats/pagu-kecamatan/{kec}"),
        &token,
    )
    .await;
    assert_eq!(body["total_pagu"], 70);
    let (_, body) = get(
        &pool,
        &format!("/api/pekerjaan/kecamatan/{kec}/desa/{d1}"),
        &token,
    )
    .await;
    assert_eq!(pekerjaan_ids(&body), vec![p2]);
    let (status, _) = get(&pool, &format!("/api/pekerjaan/{p2}/media"), &token).await;
    assert_eq!(status, StatusCode::OK);

    cleanup(&pool, &[p1, p2], kec, &[d1], &[user]).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn media_lists_foto_and_berkas_and_404s_unknown_pekerjaan() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let admin = make_user(&pool, "uji-pby-m1@example.test", "admin").await;
    let token = auth::login::create_token(&pool, admin, "uji-pby")
        .await
        .unwrap();
    let (kegiatan, _) = any_kegiatan(&pool).await;
    let (kec, d1, _) = seed_region(&pool, "media").await;
    let p1 = insert_pekerjaan(&pool, "media-1", kec, d1, kegiatan, 10.0).await;
    sqlx::query("INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, uploaded_by, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
        .bind(p1)
        .bind(format!("{MARK}SPK"))
        .bind(admin)
        .execute(&pool)
        .await
        .unwrap();

    let (status, body) = get(&pool, &format!("/api/pekerjaan/{p1}/media"), &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["foto"], json!([]));
    assert_eq!(body["berkas"].as_array().unwrap().len(), 1);
    assert_eq!(body["berkas"][0]["jenis_dokumen"], format!("{MARK}SPK"));
    assert_eq!(body["berkas"][0]["berkas_url"], "");
    assert!(body["berkas"][0]["media_id"].is_null());
    assert!(
        body["berkas"][0].get("uploader").is_none(),
        "uploader tidak dimuat di media"
    );

    let (status, _) = get(&pool, "/api/pekerjaan/999999999/media", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    cleanup(&pool, &[p1], kec, &[d1], &[admin]).await;
}
