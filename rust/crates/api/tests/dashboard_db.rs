//! Dashboard `stats`, `analytics`, dan `executive-progress` lewat router terhadap MySQL.
//!
//! Dijalankan di basis data uji terpisah (lihat `rust/fixtures/uji_dashboard_schema.sql`),
//! tidak menyentuh `apiamis`. Setiap tes memakai tahun anggaran unik sendiri, jadi aman
//! dijalankan paralel.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis_uji_dashboard?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test dashboard_db -- --include-ignored
//! ```

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
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

/// Pengguna admin uji dengan token Sanctum (admin melewati route permission).
async fn admin_token(pool: &MySqlPool, email: &str) -> String {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Dash', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
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
    auth::login::create_token(pool, uid, "uji-dash").await.unwrap()
}

async fn exec(pool: &MySqlPool, sql: &str, binds: Vec<Value>) -> u64 {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = match b {
            Value::Null => q.bind(None::<String>),
            Value::String(s) => q.bind(s),
            Value::Number(n) if n.is_f64() => q.bind(n.as_f64().unwrap()),
            Value::Number(n) => q.bind(n.as_i64().unwrap()),
            other => q.bind(other.to_string()),
        };
    }
    q.execute(pool).await.unwrap().last_insert_id()
}

/// Hapus semua data uji untuk satu tahun anggaran (urutan aman terhadap relasi logis).
async fn cleanup_tahun(pool: &MySqlPool, tahun: &str) {
    let pk = "SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE tbl_pekerjaan.kegiatan_id IN \
              (SELECT tbl_kegiatan.id FROM tbl_kegiatan WHERE tbl_kegiatan.tahun_anggaran = ?)";
    for sql in [
        format!("DELETE FROM tbl_output WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_penerima WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_progress WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM kontrak_pekerjaan WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_kontrak WHERE id_pekerjaan IN ({pk})"),
        format!("DELETE FROM tbl_pekerjaan WHERE kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran = ?)"),
        "DELETE FROM tbl_kegiatan WHERE tahun_anggaran = ?".to_string(),
    ] {
        sqlx::query(&sql).bind(tahun).execute(pool).await.unwrap();
    }
}

async fn setup_kecamatan_desa(pool: &MySqlPool, kec: i64, kec_name: &str, desa: i64, desa_name: &str) {
    sqlx::query("DELETE FROM tbl_desa WHERE id = ?").bind(desa).execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id = ?").bind(kec).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO tbl_kecamatan (id, n_kec, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(kec)
        .bind(kec_name)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_desa (id, n_desa, kecamatan_id, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())")
        .bind(desa)
        .bind(desa_name)
        .bind(kec)
        .execute(pool)
        .await
        .unwrap();
}

/// Entri objek dengan `name` tertentu dari sebuah array JSON.
fn by_name<'a>(arr: &'a Value, name: &str) -> &'a Value {
    arr.as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("tidak ada entri {name} dalam {arr}"))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn stats_totals_sub_kegiatan_and_types() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let t = "9101";
    cleanup_tahun(&pool, t).await;
    setup_kecamatan_desa(&pool, 91101, "uji-kec-stats", 91101, "uji-desa-stats").await;
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = 'uji penyedia stats'").execute(&pool).await.unwrap();
    let token = admin_token(&pool, "uji-dash-stats@example.test").await;

    let ka = exec(&pool, "INSERT INTO tbl_kegiatan (nama_program, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES ('uji','uji keg A','uji-sub','9101','uji-APBD',1000000.00,NOW(),NOW())", vec![]).await;
    let kb = exec(&pool, "INSERT INTO tbl_kegiatan (nama_program, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES ('uji','uji keg B','uji-sub','9101',NULL,500000.50,NOW(),NOW())", vec![]).await;
    let pekerjaan_sql = "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, ?, 91101, 91101, ?, ?, ?, ?, NOW(), NOW())";
    let p1 = exec(&pool, pekerjaan_sql, vec![json!("5.2.1"), json!("uji paket satu"), json!(ka), json!(100.0), json!(0), json!("active")]).await as i64;
    let p2 = exec(&pool, pekerjaan_sql, vec![json!("5.2.2"), json!("uji paket dua"), json!(ka), json!(200.0), json!(0), json!("canceled")]).await as i64;
    let p3 = exec(&pool, pekerjaan_sql, vec![json!("5.2.3"), json!("uji paket tiga"), json!(kb), json!(300.0), json!(1), json!("active")]).await as i64;
    let py = exec(&pool, "INSERT INTO tbl_penyedia (nama, direktur, created_at, updated_at) VALUES ('uji penyedia stats','uji direktur',NOW(),NOW())", vec![]).await as i64;
    // C1: tautan legacy ke P1. C2: tautan pivot ke P3.
    let c1 = exec(&pool, "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, nilai_kontrak, spk, kode_paket, created_at, updated_at) VALUES (?, ?, ?, 1000.00, 'uji-spk-1', 'uji-kp-1', NOW(), NOW())", vec![json!(ka), json!(p1), json!(py)]).await as i64;
    let c2 = exec(&pool, "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, nilai_kontrak, spk, kode_paket, created_at, updated_at) VALUES (?, 0, ?, 500.00, 'uji-spk-2', 'uji-kp-2', NOW(), NOW())", vec![json!(kb), json!(py)]).await as i64;
    exec(&pool, "INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())", vec![json!(c2), json!(p3)]).await;
    let _ = c1;

    for (komp, satuan, vol) in [("uji-komp", "unit", 2.5), ("uji-komp2", "orang", 4.0)] {
        exec(&pool, "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, created_at, updated_at) VALUES (?, ?, ?, ?, NOW(), NOW())", vec![json!(p1), json!(komp), json!(satuan), json!(vol)]).await;
    }
    for (jiwa, komunal) in [(2, 1), (3, 0), (4, 0)] {
        exec(&pool, "INSERT INTO tbl_penerima (pekerjaan_id, nama, jumlah_jiwa, is_komunal, created_at, updated_at) VALUES (?, 'uji r', ?, ?, NOW(), NOW())", vec![json!(p1), json!(jiwa), json!(komunal)]).await;
    }
    // Realisasi fisik P1: Jan 10% dan Mar 30%. `keyBy` Laravel memakai yang paling lama (Jan).
    for (tgl, persen) in [("9101-01-10", 10.0), ("9101-03-10", 30.0)] {
        exec(&pool, "INSERT INTO pekerjaan_progress_estimasi_history (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, created_at, updated_at) VALUES (?, 9101, 'fisik', 'realisasi', ?, ?, NOW(), NOW())", vec![json!(p1), json!(tgl), json!(persen)]).await;
    }
    for (tgl, nilai) in [("9101-02-01", 1000.00), ("9101-04-01", 250.00)] {
        exec(&pool, "INSERT INTO pekerjaan_progress_estimasi_history (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, nilai, created_at, updated_at) VALUES (?, 9101, 'keuangan', 'realisasi', ?, 0, ?, NOW(), NOW())", vec![json!(p1), json!(tgl), json!(nilai)]).await;
    }

    let (status, body) = get(&pool, &format!("/api/dashboard/stats?tahun={t}"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let d = &body["data"];

    // Kegiatan: SUM(DECIMAL) keluar sebagai string, seperti PHP dengan driver native.
    assert_eq!(d["totalKegiatan"], 2);
    assert_eq!(d["totalPagu"], "1500000.50");
    assert_eq!(d["kegiatanPerTahun"], json!([{"name": "9101", "value": 2}]));
    assert_eq!(by_name(&d["kegiatanPerSumberDana"], "uji-APBD")["value"], 1);
    assert_eq!(by_name(&d["kegiatanPerSumberDana"], "N/A")["value"], 1, "sumber_dana NULL menjadi N/A");

    // Pekerjaan: P2 dibatalkan dan tidak masuk rekap aktif.
    assert_eq!(d["totalPekerjaan"], 2);
    assert_eq!(d["pekerjaanAktif"], 2);
    assert_eq!(d["pekerjaanBatal"], 1);
    assert_eq!(d["pekerjaanBerkontrak"], 2, "legacy (P1) dan pivot (P3)");
    assert_eq!(d["pekerjaanBelumBerkontrak"], 0);
    assert_eq!(d["pekerjaanFisik"], 1);
    assert_eq!(d["pekerjaanKonsultan"], 1);
    assert_eq!(d["pekerjaanFisikBerkontrak"], 1);
    assert_eq!(d["pekerjaanFisikBelumBerkontrak"], 0);
    // SUM(FLOAT) keluar sebagai float.
    assert_eq!(d["totalPaguPekerjaan"], json!(400.0));
    assert_eq!(d["totalPaguPekerjaanFisik"], json!(100.0));
    assert_eq!(d["totalPaguPekerjaanKonsultan"], json!(300.0));
    assert_eq!(by_name(&d["pekerjaanPerKecamatan"], "uji-kec-stats")["value"], 2);
    let desa = by_name(&d["pekerjaanPerDesa"], "uji-desa-stats");
    assert_eq!(desa["value"], 2);
    assert_eq!(desa["paguJt"], json!(0.0));
    assert_eq!(by_name(&d["paguPekerjaanPerKecamatan"], "uji-kec-stats")["value"], json!(0.0));

    // Kontrak.
    assert_eq!(d["totalKontrak"], 2);
    assert_eq!(d["totalNilaiKontrak"], "1500.00");
    assert_eq!(by_name(&d["kontrakPerPenyedia"], "uji penyedia stats")["value"], 2);
    assert_eq!(by_name(&d["nilaiKontrakPerPenyedia"], "uji penyedia stats")["value"], json!(0.0));

    // Output, penerima.
    assert_eq!(d["totalOutput"], 2);
    assert_eq!(by_name(&d["outputPerSatuan"], "unit")["value"], 1);
    assert_eq!(d["totalPenerima"], 3);
    assert_eq!(d["totalJiwa"], "9", "SUM(INT) keluar sebagai string");
    assert_eq!(by_name(&d["penerimaKomunalVsIndividu"], "Komunal")["value"], 1);
    assert_eq!(by_name(&d["penerimaKomunalVsIndividu"], "Individu")["value"], 2);

    // Rincian per sub kegiatan.
    assert_eq!(d["paguPerTahun"], json!([{"name": "9101", "value": 1.5}]));
    assert!(d["availableYears"].as_array().unwrap().contains(&json!("9101")));
    let sub = by_name(&d["subKegiatanStats"], "uji-sub");
    assert_eq!(sub["count"], 2);
    assert_eq!(sub["paguM"], json!(0.0));
    assert_eq!(sub["progress"], json!(10.0), "keyBy Laravel: realisasi fisik tertua (Jan)");
    assert_eq!(sub["hasProgress"], true);
    assert_eq!(sub["sp2dTotal"], json!(1250.0));
    assert_eq!(sub["kontrakTotal"], json!(1500.0));
    assert_eq!(sub["batal"], 1);
    assert_eq!(sub["belumBerkontrak"], 1, "whereDoesntHave('kontraks') hanya pivot: P1 dihitung belum");

    // Tanpa token: 401.
    let (status, _) = get(&pool, "/api/dashboard/stats", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup_tahun(&pool, t).await;
    sqlx::query("DELETE FROM tbl_penyedia WHERE id = ?").bind(py).execute(&pool).await.unwrap();
    let _ = (p2, c2);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn analytics_trend_regions_and_categories() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let t = "9102";
    cleanup_tahun(&pool, t).await;
    setup_kecamatan_desa(&pool, 91102, "uji-kec-an", 91102, "uji-desa-an").await;
    let token = admin_token(&pool, "uji-dash-an@example.test").await;

    let k = exec(&pool, "INSERT INTO tbl_kegiatan (nama_sub_kegiatan, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES ('x','9102','uji-sumber-an',1000.00,NOW(),NOW())", vec![]).await as i64;
    let content = json!({
        "week_count": 3,
        "items": [{"bobot": 50, "target_volume": 10, "weekly_data": {"1": {"rencana": 2, "realisasi": 1}, "2": {"rencana": 3, "realisasi": 4}}}]
    });
    let p1 = exec(&pool, "INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES ('uji an 1', 91102, 0, ?, 100.0, 0, 'active', NOW(), NOW())", vec![json!(k)]).await as i64;
    let p2 = exec(&pool, "INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES ('uji an 2', 91102, 0, ?, 100.0, 0, 'canceled', NOW(), NOW())", vec![json!(k)]).await as i64;
    exec(&pool, "INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, ?, NOW(), NOW())", vec![json!(p1), json!(content.to_string())]).await;
    // Paket dibatalkan tetap ikut analitik (tanpa filter status). Content NULL.
    exec(&pool, "INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, NULL, NOW(), NOW())", vec![json!(p2)]).await;

    let (status, body) = get(&pool, &format!("/api/dashboard/analytics?tahun={t}"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    let d = &body["data"];

    // Dua baris progres: rata-rata dibagi 2 (termasuk baris content NULL).
    let trend = d["trend"].as_array().unwrap();
    assert_eq!(trend.len(), 3, "week_count 3");
    assert_eq!(trend[0], json!({"week": "M1", "rencana": 5.0, "realisasi": 2.5}));
    assert_eq!(trend[1], json!({"week": "M2", "rencana": 12.5, "realisasi": 12.5}));
    assert_eq!(trend[2], json!({"week": "M3", "rencana": 12.5, "realisasi": 12.5}));

    let region = by_name(&d["regions"], "uji-kec-an");
    assert_eq!(region["value"], json!(12.5));
    assert_eq!(region["hasProgress"], true);

    assert_eq!(d["categories"], json!([{"name": "uji-sumber-an", "value": 2}]));

    cleanup_tahun(&pool, t).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn executive_progress_carry_forward_and_rounding() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let t = "9103";
    cleanup_tahun(&pool, t).await;
    let token = admin_token(&pool, "uji-dash-exec@example.test").await;

    let k = exec(&pool, "INSERT INTO tbl_kegiatan (nama_sub_kegiatan, tahun_anggaran, created_at, updated_at) VALUES ('x','9103',NOW(),NOW())", vec![]).await as i64;
    let p = exec(&pool, "INSERT INTO tbl_pekerjaan (nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES ('uji exec', 0, 0, ?, 10.0, 0, 'active', NOW(), NOW())", vec![json!(k)]).await as i64;
    let rows = [
        ("realisasi", "fisik", "9103-01-10", 20.0, None),
        ("realisasi", "fisik", "9103-03-05", 50.0, None),
        ("realisasi", "fisik", "9103-03-20", 60.0, None),
        ("rencana", "fisik", "9103-02-15", 30.0, None),
        ("realisasi", "keuangan", "9103-03-10", 0.0, Some(1000.50)),
    ];
    for (tipe, jenis, tgl, persen, nilai) in rows {
        exec(&pool, "INSERT INTO pekerjaan_progress_estimasi_history (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, nilai, created_at, updated_at) VALUES (?, 9103, ?, ?, ?, ?, ?, NOW(), NOW())", vec![json!(p), json!(jenis), json!(tipe), json!(tgl), json!(persen), json!(nilai)]).await;
    }

    let (status, body) = get(&pool, &format!("/api/dashboard/executive-progress?tahun={t}"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    let trend = body["data"]["monthly_trend"].as_array().unwrap();
    assert_eq!(trend.len(), 12);
    // Realisasi fisik per bulan: entri terbaru di bulan itu, lalu terbawa ke bulan berikutnya.
    assert_eq!(trend[0], json!({"month": "Jan", "fisik_avg": 20.0, "rencana_avg": 0, "keuangan_sum": 0.0}));
    assert_eq!(trend[1]["fisik_avg"], json!(20.0));
    assert_eq!(trend[1]["rencana_avg"], json!(30.0));
    assert_eq!(trend[2]["month"], "Mar");
    assert_eq!(trend[2]["fisik_avg"], json!(60.0), "03-20 mengalahkan 03-05");
    assert_eq!(trend[2]["keuangan_sum"], json!(1001.0), "round(1000.50) PHP = 1001");
    assert_eq!(trend[11]["fisik_avg"], json!(60.0), "carry forward sampai Desember");
    assert_eq!(body["data"]["totals"]["keuangan_total"], json!(1001.0));

    // pekerjaan_ids yang tidak aktif: respon kosong dengan keuangan_total int 0.
    let (status, body) = get(&pool, &format!("/api/dashboard/executive-progress?tahun={t}&pekerjaan_ids=999999999"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"success": true, "data": {"monthly_trend": [], "totals": {"keuangan_total": 0}}}));

    cleanup_tahun(&pool, t).await;
}
