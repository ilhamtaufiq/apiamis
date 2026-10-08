//! `GET /api/search` lewat router terhadap MySQL.
//!
//! Dijalankan di basis data uji terpisah `apiamis_uji_dashboard` (lihat
//! `rust/fixtures/uji_dashboard_schema.sql`), yang juga punya FULLTEXT `ft_output_search`.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis_uji_dashboard?socket=/run/mysqld/mysqld.sock' \
//!     CARGO_INCREMENTAL=0 cargo test -p api --test search_db -- --include-ignored
//! ```
//!
//! Satu fungsi tes saja: data uji dipakai bersama oleh semua skenario, jadi urutannya dibuat berurutan.

use api::{app, AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use serde_json::{json, Value};
use shared::Config;
use sqlx::MySqlPool;
use tower::ServiceExt;

const TAHUN: &str = "9201";
const TAHUN_KOSONG: &str = "9202";
const APP_KEY_RAW: &str = "base64:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

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

async fn user_with_role(pool: &MySqlPool, email: &str, role: &str) -> (u64, String) {
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email = ?)")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?").bind(email).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji Search', ?, 'x', NOW(), NOW())")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let rid: u64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
        .bind(role)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(rid)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    let token = auth::login::create_token(pool, uid, "uji-search").await.unwrap();
    (uid, token)
}

async fn insert(pool: &MySqlPool, sql: &str, binds: Vec<Value>) -> i64 {
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
    q.execute(pool).await.unwrap().last_insert_id() as i64
}

async fn cleanup(pool: &MySqlPool) {
    let pk = "SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE tbl_pekerjaan.kegiatan_id IN \
              (SELECT tbl_kegiatan.id FROM tbl_kegiatan WHERE tbl_kegiatan.tahun_anggaran IN (?, ?))";
    for sql in [
        format!("DELETE FROM tbl_output WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_penerima WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_foto WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_progress WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM kontrak_pekerjaan WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_kontrak WHERE id_pekerjaan IN ({pk})"),
        format!("DELETE FROM user_pekerjaan WHERE pekerjaan_id IN ({pk})"),
        "DELETE FROM tbl_pekerjaan WHERE kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran IN (?, ?))".to_string(),
        "DELETE FROM tbl_kegiatan WHERE tahun_anggaran IN (?, ?)".to_string(),
    ] {
        let mut q = sqlx::query(&sql);
        if sql.contains('?') {
            q = q.bind(TAHUN).bind(TAHUN_KOSONG);
        }
        // Subquery `pk` memakai dua `?`; sisanya memakai dua `?` juga.
        q.execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM tbl_penyedia WHERE nama = 'ujialpha PT'").execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_desa WHERE id = 92001").execute(pool).await.unwrap();
    sqlx::query("DELETE FROM tbl_kecamatan WHERE id = 92001").execute(pool).await.unwrap();
}

/// Tipe setiap hasil, berurutan seperti respon.
fn types(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["type"].as_str().unwrap().to_string())
        .collect()
}

fn find<'a>(body: &'a Value, ty: &str) -> Vec<&'a Value> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["type"] == ty)
        .collect()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn global_search_scopes_sorting_and_formats() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    std::env::set_var("APP_KEY", APP_KEY_RAW);
    cleanup(&pool).await;

    sqlx::query("INSERT INTO tbl_kecamatan (id, n_kec, created_at, updated_at) VALUES (92001, 'uji-kec-search', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tbl_desa (id, n_desa, kecamatan_id, created_at, updated_at) VALUES (92001, 'ujialpha', 92001, NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();

    let k1 = insert(&pool, "INSERT INTO tbl_kegiatan (nama_program, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, created_at, updated_at) VALUES ('ujiprogram','ujialpha kegiatan','ujisub','9201','uji',NOW(),NOW())", vec![]).await;
    let pekerjaan_sql = "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) VALUES (?, ?, 92001, 92001, ?, 100.0, 0, 'active', NOW(), NOW())";
    let p1 = insert(&pool, pekerjaan_sql, vec![json!("5.2.1"), json!("ujialpha belanja"), json!(k1)]).await;
    let p2 = insert(&pool, pekerjaan_sql, vec![json!("5.2.2"), json!("ujialpha beta"), json!(k1)]).await;
    let py = insert(&pool, "INSERT INTO tbl_penyedia (nama, direktur, created_at, updated_at) VALUES ('ujialpha PT','ujidirektur',NOW(),NOW())", vec![]).await;
    // Kontrak C: tautan legacy dan pivot ke P2 (pencarian Kontrak memakai legacy).
    let c = insert(&pool, "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, nilai_kontrak, spk, kode_paket, created_at, updated_at) VALUES (?, ?, ?, 1000.50, 'ujialpha', 'uji-kp', NOW(), NOW())", vec![json!(k1), json!(p2), json!(py)]).await;
    insert(&pool, "INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())", vec![json!(c), json!(p2)]).await;

    let raw_key = api::crypt::key_from_app_key(APP_KEY_RAW).unwrap();
    let alamat = api::crypt::encrypt_string(&raw_key, "Jl. Uji 1");
    let nik = api::crypt::encrypt_string(&raw_key, "3201010101010001");
    insert(&pool, "INSERT INTO tbl_penerima (pekerjaan_id, nama, nik, alamat, jumlah_jiwa, is_komunal, created_at, updated_at) VALUES (?, 'ujialpha penerima', ?, ?, 2, 0, NOW(), NOW())", vec![json!(p1), json!(nik), json!(alamat)]).await;
    insert(&pool, "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, created_at, updated_at) VALUES (?, 'ujialpha', 'unit', 2.5, NOW(), NOW())", vec![json!(p1)]).await;
    insert(&pool, "INSERT INTO tbl_foto (pekerjaan_id, komponen_id, keterangan, koordinat, created_at, updated_at) VALUES (?, 1, '50%', '1,1', NOW(), NOW())", vec![json!(p1)]).await;
    insert(&pool, "INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, '{\"note\": \"ujialpha\"}', NOW(), NOW())", vec![json!(p1)]).await;

    // Pengguna admin: semua paket terlihat.
    let (_, admin) = user_with_role(&pool, "uji-search-admin@example.test", "admin").await;
    let (status, body) = get(&pool, &format!("/api/search?q=ujialpha&tahun={TAHUN}"), Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    // Urutan setelah sort stabil berdasarkan `type`.
    assert_eq!(
        types(&body),
        vec![
            "Desa", "Dokumentasi", "Kegiatan", "Kontrak", "Output", "Pekerjaan", "Pekerjaan",
            "Penerima Manfaat", "Penyedia", "Progress"
        ]
    );
    let pekerjaan = find(&body, "Pekerjaan");
    let p1_json = pekerjaan.iter().find(|v| v["id"] == p1).unwrap();
    assert_eq!(p1_json["title"], "ujialpha belanja");
    assert_eq!(p1_json["subtitle"], "5.2.1 - ujialpha");
    assert_eq!(p1_json["tahun"], TAHUN);
    assert_eq!(p1_json["url"], format!("/pekerjaan/{p1}"));
    assert_eq!(p1_json["map_url"], format!("/map?search=ujialpha%20belanja&tahun={TAHUN}"));
    let p2_json = pekerjaan.iter().find(|v| v["id"] == p2).unwrap();
    // Penyedia dari kontrak pertama mengganti nama paket di map_url (stripos cocok).
    assert_eq!(p2_json["subtitle"], "5.2.2 - ujialpha - ujialpha PT");
    assert_eq!(p2_json["map_url"], format!("/map?search=ujialpha%20PT&tahun={TAHUN}"));

    let kontrak = &find(&body, "Kontrak")[0];
    assert_eq!(kontrak["title"], "ujialpha");
    assert_eq!(kontrak["subtitle"], "ujialpha beta");
    assert_eq!(kontrak["penyedia"], "ujialpha PT");
    assert_eq!(kontrak["nilai"], json!(1000.5));
    assert_eq!(kontrak["tahun"], TAHUN);

    let penerima = &find(&body, "Penerima Manfaat")[0];
    assert_eq!(penerima["title"], "Penerima: ujialpha penerima");
    assert_eq!(penerima["subtitle"], "Alamat: Jl. Uji 1 | Pekerjaan: ujialpha belanja", "alamat didekripsi");

    let output = &find(&body, "Output")[0];
    assert_eq!(output["subtitle"], "Volume: 2.50 unit | Pekerjaan: ujialpha belanja");

    let foto = &find(&body, "Dokumentasi")[0];
    assert_eq!(foto["title"], "Dokumentasi: 50%");
    assert_eq!(foto["image_url"], "", "tanpa media");

    let penyedia = &find(&body, "Penyedia")[0];
    assert_eq!(penyedia["subtitle"], "Direktur: ujidirektur");
    assert_eq!(penyedia["map_url"], "/map?search=ujialpha%20PT");

    let desa = &find(&body, "Desa")[0];
    assert_eq!(desa["title"], "Desa ujialpha");
    assert_eq!(desa["subtitle"], "Kec. uji-kec-search");

    // Pengawas: hanya paket yang di-assign (P1). Cabang MATCH Kontrak tidak dibatasi.
    let (uid, pengawas) = user_with_role(&pool, "uji-search-pengawas@example.test", "pengawas").await;
    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?").bind(uid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(uid)
        .bind(p1)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = get(&pool, &format!("/api/search?q=ujialpha&tahun={TAHUN}"), Some(&pengawas)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let pekerjaan = find(&body, "Pekerjaan");
    assert_eq!(pekerjaan.len(), 1, "P2 tidak di-assign");
    assert_eq!(pekerjaan[0]["id"], p1);
    assert_eq!(find(&body, "Kontrak").len(), 1, "cabang MATCH tidak dibatasi byUserRole");
    assert_eq!(find(&body, "Penerima Manfaat").len(), 1);
    assert_eq!(types(&body).len(), 9);

    // Tahun tanpa data paket: hanya sumber tanpa filter tahun.
    let (_, body) = get(&pool, &format!("/api/search?q=ujialpha&tahun={TAHUN_KOSONG}"), Some(&admin)).await;
    assert_eq!(types(&body), vec!["Desa", "Kegiatan", "Penyedia"]);

    // `tahun=` kosong: tanpa filter tahun, sehingga paket ikut muncul.
    let (_, body) = get(&pool, "/api/search?q=ujialpha&tahun=", Some(&admin)).await;
    assert_eq!(find(&body, "Pekerjaan").len(), 2);
    assert_eq!(find(&body, "Pekerjaan")[0]["map_url"].as_str().unwrap().contains("&tahun="), false);

    // `q` kosong atau "0" (falsy PHP): data kosong, meski ada data yang cocok.
    for uri in ["/api/search?q=", "/api/search?q=0", "/api/search"] {
        let (status, body) = get(&pool, uri, Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"success": true, "data": []}), "{uri}");
    }

    // Tanpa token: 401.
    let (status, _) = get(&pool, "/api/search?q=ujialpha", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    cleanup(&pool).await;
}
