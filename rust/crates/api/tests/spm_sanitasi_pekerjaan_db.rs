//! SPM sanitasi: show, mck-pekerjaan, attach/detach pekerjaan, dan sinkron master lewat router.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test spm_sanitasi_pekerjaan_db -- --include-ignored
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
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn remove_user(pool: &MySqlPool, email: &str) {
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

async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> String {
    remove_user(pool, email).await;
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji SPM PK', ?, 'x', NOW(), NOW())")
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
        let role: u64 =
            sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin' AND guard_name = 'web' LIMIT 1")
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
    auth::login::create_token(pool, uid, "uji-spm-pk")
        .await
        .unwrap()
}

/// Data uji satu tes: kecamatan dengan dua desa, dan kegiatan tahun 2024.
struct Seed {
    kecamatan: i64,
    desa1: i64,
    desa2: i64,
    kegiatan: i64,
}

fn kec_name(tag: &str) -> String {
    format!("uji-sm-{tag}-kec")
}

async fn seed(pool: &MySqlPool, tag: &str) -> Seed {
    let kecamatan = sqlx::query("INSERT INTO tbl_kecamatan (n_kec, created_at, updated_at) VALUES (?, NOW(), NOW())")
        .bind(kec_name(tag))
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id() as i64;
    let mut desa = [0i64; 2];
    for (i, slot) in desa.iter_mut().enumerate() {
        *slot = sqlx::query(
            "INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES (?, 1000, 300, ?, NOW(), NOW())",
        )
        .bind(format!("uji-sm-{tag}-desa-{}", i + 1))
        .bind(kecamatan)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id() as i64;
    }
    let kegiatan = sqlx::query(
        "INSERT INTO tbl_kegiatan (nama_kegiatan, tahun_anggaran, pagu, created_at, updated_at) VALUES (?, '2024', 1000.5, NOW(), NOW())",
    )
    .bind(format!("uji-sm-{tag}-keg"))
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    Seed {
        kecamatan,
        desa1: desa[0],
        desa2: desa[1],
        kegiatan,
    }
}

async fn spm(pool: &MySqlPool, desa: i64, jenis: &str, nama: &str) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_spm_sanitasi (jenis, desa_id, nama_infrastruktur, created_at, updated_at) VALUES (?, ?, ?, NOW(), NOW())",
    )
    .bind(jenis)
    .bind(desa)
    .bind(nama)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64
}

async fn pekerjaan(pool: &MySqlPool, s: &Seed, desa: i64, kegiatan: i64, nama: &str, pagu: f64) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_pekerjaan (nama_paket, pagu, is_konsultan, status, kecamatan_id, desa_id, kegiatan_id, created_at, updated_at) \
         VALUES (?, ?, 0, 'aktif', ?, ?, ?, NOW(), NOW())",
    )
    .bind(nama)
    .bind(pagu)
    .bind(s.kecamatan)
    .bind(desa)
    .bind(kegiatan)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64
}

async fn output(pool: &MySqlPool, pekerjaan_id: i64, komponen: &str, satuan: &str, volume: f64) -> i64 {
    sqlx::query(
        "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, created_at, updated_at) VALUES (?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(komponen)
    .bind(satuan)
    .bind(volume)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id() as i64
}

async fn penerima(pool: &MySqlPool, pekerjaan_id: i64, jiwa: i64) {
    sqlx::query("INSERT INTO tbl_penerima (pekerjaan_id, nama, jumlah_jiwa, is_komunal, created_at, updated_at) VALUES (?, 'uji-sm-pk-penerima', ?, 0, NOW(), NOW())")
        .bind(pekerjaan_id)
        .bind(jiwa)
        .execute(pool)
        .await
        .unwrap();
}

/// Hapus semua baris uji milik `tag`: audit dan notifikasi SPM, pivot, paket, output, kontrak, dan master.
async fn cleanup(pool: &MySqlPool, tag: &str) {
    let spm_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(s.id AS SIGNED) FROM tbl_spm_sanitasi s JOIN tbl_desa d ON d.id = s.desa_id \
         JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?",
    )
    .bind(kec_name(tag))
    .fetch_all(pool)
    .await
    .unwrap();
    let pekerjaan_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_pekerjaan WHERE nama_paket LIKE ?",
    )
    .bind(format!("uji-sm-{tag}-%"))
    .fetch_all(pool)
    .await
    .unwrap();
    for id in &spm_ids {
        sqlx::query("DELETE FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        let notif_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM notifications WHERE data LIKE ?")
            .bind(format!("%Model SpmSanitasi dengan ID #{id} %"))
            .fetch_all(pool)
            .await
            .unwrap();
        for nid in notif_ids {
            sqlx::query("DELETE FROM notifications WHERE id = ?")
                .bind(nid)
                .execute(pool)
                .await
                .unwrap();
        }
    }
    for id in &pekerjaan_ids {
        sqlx::query("DELETE FROM kontrak_pekerjaan WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_penerima WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_output WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_spm_sanitasi_pekerjaan WHERE pekerjaan_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM tbl_kontrak WHERE kode_paket = ?")
        .bind(format!("uji-sm-{tag}"))
        .execute(pool)
        .await
        .unwrap();
    for id in &spm_ids {
        sqlx::query("DELETE FROM tbl_spm_sanitasi WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM tbl_kegiatan WHERE nama_kegiatan = ?")
        .bind(format!("uji-sm-{tag}-keg"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE d FROM tbl_desa d JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE k.n_kec = ?")
        .bind(kec_name(tag))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE n_kec = ?")
        .bind(kec_name(tag))
        .execute(pool)
        .await
        .unwrap();
}

fn db_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set")
}

/// Baca satu kolom SPM sebagai JSON (nilai langsung dari DB, bukan respon).
async fn spm_row(pool: &MySqlPool, id: i64) -> Value {
    let row: (Option<i64>, Option<i64>, i8, Option<f64>, i8, Option<i64>) = sqlx::query_as(
        "SELECT CAST(jumlah_pemanfaat_kk AS SIGNED), CAST(jumlah_pemanfaat_jiwa AS SIGNED), pemanfaat_dari_integrasi, \
         CAST(pembiayaan_total AS DOUBLE), pembiayaan_dari_integrasi, CAST(tahun_konstruksi AS SIGNED) \
         FROM tbl_spm_sanitasi WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    json!({
        "kk": row.0,
        "jiwa": row.1,
        "pemanfaat": row.2 != 0,
        "pembiayaan": row.3,
        "pembiayaan_flag": row.4 != 0,
        "tahun": row.5,
    })
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spm_pekerjaan_show_mck_and_link_sync_match_laravel() {
    let pool = sqlx::MySqlPool::connect(&db_url()).await.unwrap();
    let tag = "pk";
    cleanup(&pool, tag).await;
    let admin = user_token(&pool, "uji-spm-pk-admin@example.test", true).await;
    let biasa = user_token(&pool, "uji-spm-pk-biasa@example.test", false).await;
    let s = seed(&pool, tag).await;

    // Master A dan B di desa 1 (SPALDS), C di desa 2 (MCK komunal).
    let a = spm(&pool, s.desa1, "spalds", "uji-sm-pk-spm-a").await;
    let b = spm(&pool, s.desa1, "spalds", "uji-sm-pk-spm-b").await;
    let c = spm(&pool, s.desa2, "mck_komunal", "uji-sm-pk-spm-c").await;

    // P1: tangki individu dengan 2 penerima (7 jiwa). P2: jamban komunal tanpa penerima.
    let p1 = pekerjaan(&pool, &s, s.desa1, s.kegiatan, "uji-sm-pk-paket-tangki", 5_000_000.0).await;
    let o1 = output(&pool, p1, "Tangki Septik Individu", "unit", 10.0).await;
    penerima(&pool, p1, 4).await;
    penerima(&pool, p1, 3).await;
    let p2 = pekerjaan(&pool, &s, s.desa1, s.kegiatan, "uji-sm-pk-paket-jamban", 2_000_000.0).await;
    let o2 = output(&pool, p2, "Jamban Komunal", "unit", 3.0).await;

    // Show tanpa tautan.
    let (st, body) = send(&pool, Method::GET, &format!("/api/spm-sanitasi/{a}"), Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["desa"]["n_desa"], format!("uji-sm-{tag}-desa-1"));
    assert_eq!(body["data"]["desa"]["kecamatan"]["n_kec"], kec_name(tag));
    assert_eq!(body["data"]["pekerjaan"], json!([]), "{body}");

    // Attach P1 tanpa output_id: output sejenis pertama dipakai sebagai pivot.
    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{a}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": p1 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["message"].as_str().unwrap().starts_with("Pekerjaan berhasil ditautkan."));
    let linked = &body["data"]["pekerjaan"][0];
    assert_eq!(linked["id"], p1);
    assert_eq!(linked["pivot"]["output_id"], o1, "{body}");
    assert_eq!(linked["kegiatan"]["tahun_anggaran"], "2024");
    assert_eq!(linked["kegiatan"]["pagu"], "1000.50", "decimal:2 sebagai teks");
    assert_eq!(linked["output"][0]["volume"], "10.00", "decimal:2 sebagai teks");
    assert_eq!(linked["kontrak"], json!([]));
    assert_eq!(body["data"]["desa"]["id"], s.desa1);

    // Sinkron A: KK 2 (penerima), jiwa 7, biaya pagu P1 (tanpa kontrak), tahun dari kegiatan.
    let va = spm_row(&pool, a).await;
    assert_eq!(va["kk"], 2, "{va}");
    assert_eq!(va["jiwa"], 7, "{va}");
    assert_eq!(va["pemanfaat"], true);
    assert_eq!(va["pembiayaan"], 5_000_000.0);
    assert_eq!(va["pembiayaan_flag"], true);
    assert_eq!(va["tahun"], 2024);
    let audits: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_audit_logs WHERE auditable_type = 'App\\\\Models\\\\SpmSanitasi' AND auditable_id = ? AND event = 'updated'",
    )
    .bind(a)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audits >= 1, "perubahan sinkron harus diaudit");

    // Attach P1 juga ke B: pemilik KK/biaya adalah master dengan id terkecil (A), jadi B tidak terisi KK.
    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{b}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": p1 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let vb = spm_row(&pool, b).await;
    assert_eq!(vb["kk"], Value::Null, "{vb}");
    assert_eq!(vb["pemanfaat"], false);
    assert_eq!(vb["tahun"], 2024, "tahun dari paket tertaut tetap diisi");
    let va_after = spm_row(&pool, a).await;
    assert_eq!(va_after["kk"], 2, "A tetap memegang KK");

    // Daftar: P2 belum tertaut, P1 tertaut ke A dan B.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/mck-pekerjaan?desa_id={}&spm_sanitasi_id={a}&unlinked_only=1", s.desa1),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1, "{body}");
    let row = &body["data"][0];
    assert_eq!(row["id"], p2);
    assert_eq!(row["is_linked"], false);
    assert_eq!(row["derived"]["unit"], 3);
    assert_eq!(row["derived"]["kk"], 3);
    assert_eq!(row["derived"]["jiwa"], 15);
    assert_eq!(row["derived"]["pembiayaan_suggested"], 2_000_000.0);
    assert_eq!(row["output_types"], json!(["mck_komunal"]));
    assert_eq!(row["target_jenis_list"], json!(["mck_komunal"]));

    // Tanpa unlinked_only, P1 ditandai tertaut ke A.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/mck-pekerjaan?desa_id={}&spm_sanitasi_id={a}", s.desa1),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 2);
    let p1_row = body["data"].as_array().unwrap().iter().find(|r| r["id"] == p1).unwrap();
    assert_eq!(p1_row["is_linked"], true);
    assert_eq!(p1_row["linked_spm_ids"], json!([a, b]));

    // Filter output type.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/mck-pekerjaan?desa_id={}&mck_type=mck_komunal", s.desa1),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["data"].as_array().unwrap().iter().all(|r| r["id"] != p1));
    let (st, body) = send(
        &pool,
        Method::GET,
        "/api/spm-sanitasi/mck-pekerjaan?mck_type=bogus",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The selected mck type is invalid.");

    // Lepas P1 dari A: A kosong (flag integrasi dibersihkan), B menjadi pemilik KK dan biaya.
    let (st, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/spm-sanitasi/{a}/pekerjaan/{p1}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body["message"].as_str().unwrap().starts_with("Tautan pekerjaan berhasil dihapus."));
    assert_eq!(body["data"]["pekerjaan"], json!([]), "tidak ada tautan tersisa");
    assert!(body["data"].get("desa").is_none(), "refresh tidak memuat desa");
    let va_detached = spm_row(&pool, a).await;
    assert_eq!(va_detached["kk"], Value::Null, "{va_detached}");
    assert_eq!(va_detached["pemanfaat"], false);
    assert_eq!(va_detached["pembiayaan"], Value::Null);
    let vb_owner = spm_row(&pool, b).await;
    assert_eq!(vb_owner["kk"], 2, "{vb_owner}");
    assert_eq!(vb_owner["jiwa"], 7);
    assert_eq!(vb_owner["pembiayaan"], 5_000_000.0);

    // Show B: relasi pekerjaan dan desa termuat.
    let (st, body) = send(&pool, Method::GET, &format!("/api/spm-sanitasi/{b}"), Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan"][0]["id"], p1);
    assert_eq!(body["data"]["pekerjaan"][0]["desa"]["n_desa"], format!("uji-sm-{tag}-desa-1"));

    // Validasi dan cakupan.
    let (st, body) = send(&pool, Method::POST, &format!("/api/spm-sanitasi/{b}/pekerjaan"), Some(&admin), Some(json!({}))).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The pekerjaan id field is required.");
    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{b}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": 999_999_999 })),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The selected pekerjaan id is invalid.");
    let (st, _) = send(&pool, Method::POST, "/api/spm-sanitasi/999999999/pekerjaan", Some(&admin), Some(json!({ "pekerjaan_id": p2 }))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // Paket di desa lain tidak terlihat dari SPM C (desa 2).
    let (st, _) = send(&pool, Method::POST, &format!("/api/spm-sanitasi/{c}/pekerjaan"), Some(&admin), Some(json!({ "pekerjaan_id": p1 }))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // Non-admin diblokir oleh pemeriksaan rute.
    let (st, _) = send(&pool, Method::POST, &format!("/api/spm-sanitasi/{b}/pekerjaan"), Some(&biasa), Some(json!({ "pekerjaan_id": p2 }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Output dari komponen lain di SPALDS ditolak bila tidak ada padanan.
    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{b}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": p2, "output_id": o2 })),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["success"], false);
    assert_eq!(
        body["message"],
        "Output tidak sesuai jenis infrastruktur. Tangki Septik/SPALDS → SPALDS, IPAL/IPLT/SPALDT → SPALDT/IPLT, MCK → MCK."
    );

    // Kontrak mengganti pagu sebagai biaya paket; respon attach memuat kontrak.
    let kontrak = sqlx::query(
        "INSERT INTO tbl_kontrak (nilai_kontrak, kode_paket, nomor_penawaran, tgl_spk, created_at, updated_at) VALUES (7000000, ?, 'uji', '2026-03-01', NOW(), NOW())",
    )
    .bind(format!("uji-sm-{tag}"))
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id() as i64;
    sqlx::query("INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(kontrak)
        .bind(p2)
        .execute(&pool)
        .await
        .unwrap();
    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{b}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": p2 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let p2_entry = body["data"]["pekerjaan"].as_array().unwrap().iter().find(|p| p["id"] == p2).unwrap();
    assert_eq!(p2_entry["pivot"]["output_id"], Value::Null, "tanpa output sejenis, pivot tanpa output");
    assert_eq!(p2_entry["kontrak"][0]["nilai_kontrak"], 7_000_000.0);
    assert_eq!(p2_entry["kontrak"][0]["tgl_spk"], "2026-03-01");
    assert_eq!(p2_entry["kontrak"][0]["pivot"]["kontrak_id"], kontrak);
    assert_eq!(p2_entry["kegiatan"]["tahun_anggaran"], "2024");
    let vb_total = spm_row(&pool, b).await;
    assert_eq!(vb_total["kk"], 5, "P1 2 + P2 3 (unit tanpa penerima)");
    assert_eq!(vb_total["jiwa"], 22, "P1 7 + P2 15");
    assert_eq!(vb_total["pembiayaan"], 12_000_000.0, "P1 pagu 5 juta + P2 kontrak 7 juta");

    // Show dengan id tidak ada dan non-numerik.
    let (st, _) = send(&pool, Method::GET, "/api/spm-sanitasi/999999999", Some(&admin), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = send(&pool, Method::GET, &format!("/api/spm-sanitasi/{o1}x"), Some(&admin), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Pembersihan hanya untuk baris uji.
    cleanup(&pool, tag).await;
    let left: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi WHERE id IN (?, ?, ?)")
        .bind(a)
        .bind(b)
        .bind(c)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn spm_integration_rows_status_filters_and_scope_match_laravel() {
    let pool = sqlx::MySqlPool::connect(&db_url()).await.unwrap();
    let tag = "ig";
    cleanup(&pool, tag).await;
    let admin = user_token(&pool, "uji-spm-ig-admin@example.test", true).await;
    let biasa = user_token(&pool, "uji-spm-ig-biasa@example.test", false).await;
    let s = seed(&pool, tag).await;
    // Desa 2 hanya punya infrastruktur, desa 1 punya infrastruktur tertaut.
    let desa3 = sqlx::query("INSERT INTO tbl_desa (n_desa, jumlah_penduduk, target, kecamatan_id, created_at, updated_at) VALUES (?, 200, 10, ?, NOW(), NOW())")
        .bind(format!("uji-sm-{tag}-desa-3"))
        .bind(s.kecamatan)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id() as i64;

    let s1 = spm(&pool, s.desa1, "spalds", "uji-sm-ig-spm-1").await; // tertaut (matched)
    let s2 = spm(&pool, s.desa2, "mck_individu", "uji-sm-ig-spm-2").await; // hanya infrastruktur (no_pekerjaan)
    let s3 = spm(&pool, desa3, "spalds", "uji-sm-ig-spm-3").await; // parsial: paket belum tertaut
    let p1 = pekerjaan(&pool, &s, s.desa1, s.kegiatan, "uji-sm-ig-paket-1", 5_000_000.0).await;
    output(&pool, p1, "Tangki Septik Individu", "unit", 10.0).await;
    let p3 = pekerjaan(&pool, &s, desa3, s.kegiatan, "uji-sm-ig-paket-3", 1_000_000.0).await;
    output(&pool, p3, "Tangki Septik Individu", "unit", 4.0).await;

    let (st, body) = send(
        &pool,
        Method::POST,
        &format!("/api/spm-sanitasi/{s1}/pekerjaan"),
        Some(&admin),
        Some(json!({ "pekerjaan_id": p1 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");

    // Daftar per kecamatan: desa 1 matched, desa 2 no_pekerjaan, desa 3 partial.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration?kecamatan_id={}", s.kecamatan),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 3, "{body}");
    assert_eq!(body["summary"]["total_desa"], 3);
    assert_eq!(body["summary"]["matched_count"], 1);
    assert_eq!(body["summary"]["partial_count"], 1);
    assert_eq!(body["summary"]["no_pekerjaan_count"], 1);
    assert_eq!(body["summary"]["no_infrastruktur_count"], 0);
    assert_eq!(body["summary"]["total_linked"], 1);
    assert_eq!(body["summary"]["total_infrastruktur"], 3, "1 + 1 + 1");
    let names: Vec<&str> = body["data"].as_array().unwrap().iter().map(|r| r["desa"]["n_desa"].as_str().unwrap()).collect();
    assert_eq!(names, vec![format!("uji-sm-{tag}-desa-1"), format!("uji-sm-{tag}-desa-2"), format!("uji-sm-{tag}-desa-3")]);

    // Filter status: hanya desa 1.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration?kecamatan_id={}&sync_status=matched", s.kecamatan),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["sync_status"], "matched");
    assert_eq!(body["data"][0]["derived"]["kk"], 10, "unit 10 tanpa penerima");
    assert_eq!(body["data"][0]["derived"]["jiwa"], 50);
    assert_eq!(body["data"][0]["derived"]["nilai_kontrak"], 5_000_000.0);
    assert_eq!(body["data"][0]["manual"]["kk"], 10, "jumlah pemanfaat hasil sinkron");
    assert_eq!(body["data"][0]["manual"]["jiwa"], 50);
    assert_eq!(body["data"][0]["manual"]["nilai_kontrak"], 5_000_000.0);
    assert_eq!(body["data"][0]["linked_count"], 1);
    assert_eq!(body["data"][0]["output_types"], json!(["tangki_septik_individu"]));
    assert_eq!(body["data"][0]["infrastruktur"][0]["linked_pekerjaan_count"], 1);

    // Pagination: per_page 2, halaman 2 memuat desa 3.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration?kecamatan_id={}&per_page=2&page=2", s.kecamatan),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["meta"], json!({"current_page": 2, "last_page": 2, "per_page": 2, "total": 3}));
    assert_eq!(body["data"][0]["desa"]["n_desa"], format!("uji-sm-{tag}-desa-3"));
    assert_eq!(body["summary"]["total_desa"], 3, "ringkasan atas seluruh baris terfilter");

    // Desa tunggal: detail satu desa.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration/desa/{}", s.desa2),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["sync_status"], "no_pekerjaan");
    assert_eq!(body["data"]["pekerjaan_count"], 0);
    assert_eq!(body["data"]["infrastruktur_count"], 1);
    assert_eq!(body["data"]["desa"]["kecamatan"]["n_kec"], kec_name(tag));

    // Filter output type: MCK individu memetakan ke jenis mck_individu / mck_komunal (desa 2 punya S2 mck_individu).
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration/desa/{}?output_type=mck_individu", s.desa2),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["output_type_filter"], "mck_individu");
    assert_eq!(body["data"]["pekerjaan_count"], 0, "{body}");
    assert_eq!(body["data"]["infrastruktur_count"], 1);
    assert_eq!(body["data"]["sync_status"], "no_pekerjaan");

    // Validasi.
    let (st, body) = send(
        &pool,
        Method::GET,
        "/api/spm-sanitasi/integration?sync_status=xx",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The selected sync status is invalid.");
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration/desa/{}?output_type=bogus", s.desa1),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["message"], "The selected output type is invalid.");
    let (st, _) = send(&pool, Method::GET, "/api/spm-sanitasi/integration/desa/999999999", Some(&admin), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = send(&pool, Method::GET, "/api/spm-sanitasi/integration", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // byUserRole: pengguna tanpa penugasan tidak melihat paket, jadi desa 1 jadi no_pekerjaan.
    let (st, body) = send(
        &pool,
        Method::GET,
        &format!("/api/spm-sanitasi/integration/desa/{}", s.desa1),
        Some(&biasa),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["pekerjaan_count"], 0);
    assert_eq!(body["data"]["sync_status"], "no_pekerjaan");
    assert_eq!(body["data"]["infrastruktur_count"], 1, "infrastruktur tidak dibatasi byUserRole");

    cleanup(&pool, tag).await;
    let left: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi WHERE id IN (?, ?, ?)")
        .bind(s1)
        .bind(s2)
        .bind(s3)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
