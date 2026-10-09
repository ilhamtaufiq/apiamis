//! Profil desa lewat router terhadap MySQL: bentuk respon, 404 untuk desa tidak dikenal, dan 401 tanpa token.
//!
//! Setiap baris uji memakai prefiks `uji-dp-` dan hanya baris itu yang dihapus.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test desa_profile_db -- --include-ignored
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

const ADMIN: &str = "uji-dp-admin@example.test";
const KEC: &str = "uji-dp-kecamatan";
const DESA: &str = "uji-dp-desa";

fn config() -> Config {
    Config {
        app_env: "testing".to_string(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".to_string(),
    }
}

async fn get(pool: &MySqlPool, uri: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(Method::GET).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = app(
        &config(),
        AppState::new(pool.clone(), "http://localhost".to_string()),
    )
    .oneshot(req.body(Body::empty()).unwrap())
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
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Desa Profil', ?, 'x', NOW(), NOW())")
        .bind(ADMIN)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(ADMIN)
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

/// Hapus hanya baris dengan prefiks `uji-dp-`.
async fn cleanup(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE nama_infrastruktur LIKE 'uji-dp-%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_unit_spam WHERE name LIKE 'uji-dp-%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE 'uji-dp-%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE n_desa LIKE 'uji-dp-%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec LIKE 'uji-dp-%'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn desa_profile_success_shape_404_and_401() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let uid = make_admin(&pool).await;
    let token = auth::login::create_token(&pool, uid, "uji-dp")
        .await
        .unwrap();
    cleanup(&pool).await;

    sqlx::query("INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())")
        .bind(KEC)
        .execute(&pool)
        .await
        .unwrap();
    let kec: u64 = sqlx::query_scalar("SELECT id FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(KEC)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_desa (n_desa, luas, jumlah_penduduk, jumlah_kk, kecamatan_id, created_at, updated_at) VALUES (?, 12.5, 1000, 250, ?, NOW(), NOW())")
        .bind(DESA)
        .bind(kec)
        .execute(&pool)
        .await
        .unwrap();
    let desa: u64 = sqlx::query_scalar("SELECT id FROM tbl_desa WHERE n_desa = ?")
        .bind(DESA)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Tiga pekerjaan: active 100.5, completed 200, canceled 50 (tidak dihitung aktif/selesai).
    for (nama, pagu, status) in [
        ("uji-dp-paket-a", 100.5_f64, "active"),
        ("uji-dp-paket-b", 200.0, "completed"),
        ("uji-dp-paket-c", 50.0, "canceled"),
    ] {
        sqlx::query("INSERT INTO tbl_pekerjaan (nama_paket, desa_id, kecamatan_id, pagu, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, NOW(), NOW())")
            .bind(nama)
            .bind(desa)
            .bind(kec)
            .bind(pagu)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    // Dua infrastruktur sanitasi: satu Berfungsi (kk 10, jiwa 40), satu tidak (kk 5, jiwa 20).
    for (nama, status, kk, jiwa) in [
        ("uji-dp-spm-a", "Berfungsi", 10, 40),
        ("uji-dp-spm-b", "Tidak Berfungsi", 5, 20),
    ] {
        sqlx::query("INSERT INTO tbl_spm_sanitasi (jenis, nama_infrastruktur, desa_id, status_keberfungsian, jumlah_pemanfaat_kk, jumlah_pemanfaat_jiwa, created_at, updated_at) VALUES ('uji-dp', ?, ?, ?, ?, ?, NOW(), NOW())")
            .bind(nama)
            .bind(desa)
            .bind(status)
            .bind(kk)
            .bind(jiwa)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO tbl_unit_spam (name, desa_id, is_simspam, created_at, updated_at) VALUES ('uji-dp-unit', ?, 1, NOW(), NOW())")
        .bind(desa)
        .execute(&pool)
        .await
        .unwrap();

    let uri = format!("/api/desa/{desa}/profile");
    let (status, body) = get(&pool, &uri, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let d = &body["data"];
    assert_eq!(d["desa"]["id"], desa, "{body}");
    assert_eq!(d["desa"]["nama_desa"], DESA, "{body}");
    assert_eq!(d["desa"]["kecamatan"]["nama_kecamatan"], KEC, "{body}");

    let r = &d["ringkasan"];
    assert_eq!(r["kepadatan_penduduk"], 80.0, "{body}");
    assert_eq!(r["total_pekerjaan"], 3, "{body}");
    assert_eq!(r["pekerjaan_aktif"], 1, "{body}");
    assert_eq!(r["pekerjaan_selesai"], 1, "{body}");
    assert_eq!(r["total_pagu"], 350.5, "{body}");
    assert_eq!(r["total_unit_spam"], 1, "{body}");
    assert_eq!(r["unit_spam_simspam"], 1, "{body}");
    assert_eq!(r["total_infrastruktur_sanitasi"], 2, "{body}");
    assert_eq!(r["infrastruktur_berfungsi"], 1, "{body}");
    assert_eq!(r["total_pemanfaat_kk"], 15, "{body}");
    assert_eq!(r["total_pemanfaat_jiwa"], 60, "{body}");
    assert_eq!(r["total_usulan_kegiatan"], 0, "{body}");

    assert_eq!(d["pekerjaan"].as_array().unwrap().len(), 3, "{body}");
    let spm = d["spm_sanitasi"].as_array().unwrap();
    assert_eq!(spm.len(), 2, "{body}");
    assert_eq!(spm[0]["nama_infrastruktur"], "uji-dp-spm-a", "{body}");
    assert_eq!(spm[0]["jumlah_pemanfaat_kk"], 10, "{body}");
    let unit = d["unit_spam"].as_array().unwrap();
    assert_eq!(unit.len(), 1, "{body}");
    assert_eq!(unit[0]["is_simspam"], true, "{body}");

    // 404 untuk id yang tidak ada dan id yang bukan angka.
    let (status, _) = get(&pool, "/api/desa/999999999/profile", Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&pool, "/api/desa/abc/profile", Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 401 tanpa token, baik untuk desa yang ada maupun yang tidak ada.
    let (status, _) = get(&pool, &uri, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = get(&pool, "/api/desa/999999999/profile", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup(&pool).await;
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ADMIN)
        .execute(&pool)
        .await
        .unwrap();
}
