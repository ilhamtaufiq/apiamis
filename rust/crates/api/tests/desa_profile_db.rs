//! `GET /api/desa/{id}/profile` lewat router terhadap MySQL.
//!
//! Data uji memakai id tinggi (9900101) dan dihapus lagi di awal dan akhir tes.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test desa_profile_db -- --include-ignored
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

const ID: i64 = 9_900_101;
const EMAIL: &str = "uji-desa-profile@example.test";

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
    let res = app(&config(), AppState::new(pool.clone(), "http://localhost".to_string()))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Admin uji dengan token Sanctum (admin melewati route permission).
async fn admin_token(pool: &MySqlPool) -> (u64, String) {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(EMAIL)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(EMAIL).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Profil', ?, 'x', NOW(), NOW())")
        .bind(EMAIL)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(EMAIL)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES ('admin', 'web', NOW(), NOW())")
        .execute(pool)
        .await
        .unwrap();
    let role: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-profil").await.unwrap();
    (uid, token)
}

/// Menghapus data uji. Urutan mengikuti relasi (anak lebih dulu).
async fn cleanup(pool: &MySqlPool) {
    for sql in [
        "DELETE FROM tbl_usulan_kegiatan WHERE desa_id = ?",
        "DELETE FROM tbl_unit_spam WHERE desa_id = ?",
        "DELETE FROM tbl_spm_sanitasi WHERE desa_id = ?",
        "DELETE FROM tbl_pekerjaan WHERE desa_id = ?",
        "DELETE FROM tbl_desa WHERE id = ?",
    ] {
        sqlx::query(sql).bind(ID).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_kegiatan WHERE id = ?").bind(ID).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id = ?").bind(ID).execute(pool).await.unwrap();
}

async fn seed(pool: &MySqlPool, uid: u64) {
    let q = |sql: &'static str| sqlx::query(sql);
    q("INSERT INTO tbl_kecamatan (id, n_kec, created_at, updated_at) VALUES (?, 'Uji Kecamatan Profil', NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    // Luas 12.5 dan penduduk 1000: kepadatan 80.
    q("INSERT INTO tbl_desa (id, n_desa, kecamatan_id, luas, jumlah_penduduk, jumlah_kk, created_at, updated_at) VALUES (?, 'Uji Desa Profil', ?, 12.5, 1000, 300, NOW(), NOW())")
        .bind(ID)
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    q("INSERT INTO tbl_kegiatan (id, nama_program, tahun_anggaran, created_at, updated_at) VALUES (?, 'Uji Program Profil', 2026, NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    // Pagu 1234.56 (FLOAT, pecahan) dan 1000: total 2234.56. Satu aktif, satu selesai.
    q("INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES ('Uji Paket Aktif', ?, ?, ?, 1234.56, 0, 'active', NOW(), NOW())")
        .bind(ID)
        .bind(ID)
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    q("INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES ('Uji Paket Selesai', ?, ?, ?, 1000, 0, 'completed', NOW(), NOW())")
        .bind(ID)
        .bind(ID)
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    // Kemanfaatan: kk 10 + 5 = 15, jiwa 40 + 20 = 60, satu berfungsi.
    q("INSERT INTO tbl_spm_sanitasi (jenis, nama_infrastruktur, desa_id, status_keberfungsian, jumlah_pemanfaat_kk, jumlah_pemanfaat_jiwa, created_at, updated_at) VALUES ('Sanitasi', 'Uji IPAL 1', ?, 'Berfungsi', 10, 40, NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    q("INSERT INTO tbl_spm_sanitasi (jenis, nama_infrastruktur, desa_id, status_keberfungsian, jumlah_pemanfaat_kk, jumlah_pemanfaat_jiwa, created_at, updated_at) VALUES ('Sanitasi', 'Uji IPAL 2', ?, 'Tidak Berfungsi', 5, 20, NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    // Unit SPAM: dua unit, satu SIMSPAM.
    q("INSERT INTO tbl_unit_spam (desa_id, is_simspam, created_at, updated_at) VALUES (?, 1, NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    q("INSERT INTO tbl_unit_spam (desa_id, is_simspam, created_at, updated_at) VALUES (?, 0, NOW(), NOW())")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    q("INSERT INTO tbl_usulan_kegiatan (user_id, sub_bidang, nama_pengusul, kecamatan_id, desa_id, perihal, ringkasan, tanggal_surat_masuk, nomor_surat_masuk, tanggal_surat, created_at, updated_at) VALUES (?, 'air minum', 'Uji', ?, ?, 'Uji', 'Uji', '2026-01-01', 'UJI/1', '2026-01-01', NOW(), NOW())")
        .bind(uid)
        .bind(ID)
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
}

fn approx(v: &Value, want: f64) -> bool {
    v.as_f64().is_some_and(|x| (x - want).abs() < 1e-9)
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_desa, tbl_pekerjaan, tbl_spm_sanitasi, tbl_unit_spam"]
async fn profil_desa_sesuai_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;
    let (uid, token) = admin_token(&pool).await;
    seed(&pool, uid).await;

    let uri = format!("/api/desa/{ID}/profile");
    let (status, body) = get(&pool, &uri, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Lima key di dalam `data`: desa, ringkasan, pekerjaan, spm_sanitasi, unit_spam.
    let data = body["data"].as_object().expect("data objek");
    let mut keys: Vec<&str> = data.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["desa", "pekerjaan", "ringkasan", "spm_sanitasi", "unit_spam"]);

    // DesaResource dengan kecamatan.
    assert_eq!(data["desa"]["id"], ID);
    assert_eq!(data["desa"]["nama_desa"], "Uji Desa Profil");
    assert!(approx(&data["desa"]["luas"], 12.5));
    assert_eq!(data["desa"]["kecamatan"]["nama_kecamatan"], "Uji Kecamatan Profil");

    let r = &data["ringkasan"];
    assert_eq!(r["kepadatan_penduduk"], json!(80));
    assert_eq!(r["total_pekerjaan"], json!(2));
    assert_eq!(r["pekerjaan_aktif"], json!(1));
    assert_eq!(r["pekerjaan_selesai"], json!(1));
    assert!(approx(&r["total_pagu"], 2234.56), "total_pagu {}", r["total_pagu"]);
    assert_eq!(r["total_unit_spam"], json!(2));
    assert_eq!(r["unit_spam_simspam"], json!(1));
    assert_eq!(r["total_infrastruktur_sanitasi"], json!(2));
    assert_eq!(r["infrastruktur_berfungsi"], json!(1));
    assert_eq!(r["total_pemanfaat_kk"], json!(15));
    assert_eq!(r["total_pemanfaat_jiwa"], json!(60));
    assert_eq!(r["total_usulan_kegiatan"], json!(1));

    // PekerjaanResource dengan kecamatan dan kegiatan saja.
    let pekerjaan = data["pekerjaan"].as_array().unwrap();
    assert_eq!(pekerjaan.len(), 2);
    let aktif = &pekerjaan[0];
    assert_eq!(aktif["nama_paket"], "Uji Paket Aktif");
    assert_eq!(aktif["status"], "active");
    assert!(approx(&aktif["pagu"], 1234.56), "pagu {}", aktif["pagu"]);
    assert_eq!(aktif["kecamatan"]["id"], ID);
    assert_eq!(aktif["kegiatan"]["id"], ID);
    assert_eq!(aktif["foto_status"], "belum_ada_foto");
    assert!(aktif["foto_count"].is_null());
    assert_eq!(aktif["kontrak_count"], json!(0));
    for absen in ["desa", "pengawas", "pendamping", "tags", "kontrak", "output", "draft"] {
        assert!(aktif.get(absen).is_none(), "key {absen} seharusnya tidak ada");
    }

    // Model mentah: semua kolom ikut, dan cast `$casts` dipakai.
    let spm = data["spm_sanitasi"].as_array().unwrap();
    assert_eq!(spm.len(), 2);
    assert_eq!(spm[0]["jumlah_pemanfaat_kk"], json!(10));
    assert_eq!(spm[0]["pemanfaat_dari_integrasi"], json!(false));
    assert!(spm[0].get("nama_infrastruktur").is_some());
    let unit = data["unit_spam"].as_array().unwrap();
    assert_eq!(unit.len(), 2);
    assert_eq!(unit[0]["is_simspam"], json!(true));

    // Desa tidak ada: 404 seperti route-model binding.
    let (status, body) = get(&pool, "/api/desa/9999999/profile", Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({ "message": "Not Found." }));

    // Tanpa token: 401.
    let (status, _) = get(&pool, &uri, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup(&pool).await;
    // Pastikan tidak ada sisa data uji.
    let sisa: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tbl_desa WHERE id = ?")
        .bind(ID)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("n");
    assert_eq!(sisa, 0);
}
