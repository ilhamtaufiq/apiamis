//! `GET /api/pekerjaan` (PekerjaanController@index) lewat HTTP: filter, status, paginator Laravel,
//! `per_page=-1`, dan `assignment_sources` untuk pengawas. Data uji memakai awalan `uji-pix-`
//! dan tahun anggaran `9301`, lalu dibersihkan di awal dan akhir.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_index_full_db -- --include-ignored
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

const ADMIN: &str = "uji-pix-admin@example.test";
const PENGAWAS: &str = "uji-pix-pengawas@example.test";
const TAHUN: &str = "9301";
const BASE: &str = "http://localhost/api/pekerjaan";

async fn cleanup(pool: &MySqlPool) {
    let sql = [
        "DELETE pt FROM pekerjaan_tag pt JOIN tbl_pekerjaan p ON p.id = pt.pekerjaan_id WHERE p.nama_paket LIKE 'uji-pix-%'",
        "DELETE up FROM user_pekerjaan up JOIN tbl_pekerjaan p ON p.id = up.pekerjaan_id WHERE p.nama_paket LIKE 'uji-pix-%'",
        "DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE 'uji-pix-%'",
        "DELETE FROM tbl_kegiatan WHERE tahun_anggaran = '9301' AND nama_kegiatan LIKE 'uji-pix-%'",
        "DELETE FROM pengawas WHERE nama LIKE 'uji-pix-%'",
        "DELETE FROM tbl_tags WHERE name LIKE 'uji-pix-%'",
        "DELETE up FROM user_pekerjaan up JOIN users u ON u.id = up.user_id WHERE u.email LIKE 'uji-pix-%'",
        "DELETE mhr FROM model_has_roles mhr JOIN users u ON u.id = mhr.model_id WHERE mhr.model_type = 'App\\\\Models\\\\User' AND u.email LIKE 'uji-pix-%'",
        "DELETE pat FROM personal_access_tokens pat JOIN users u ON u.id = pat.tokenable_id WHERE pat.tokenable_type = 'App\\\\Models\\\\User' AND u.email LIKE 'uji-pix-%'",
        "DELETE FROM users WHERE email LIKE 'uji-pix-%'",
    ];
    for s in sql {
        sqlx::query(s).execute(pool).await.unwrap();
    }
}

/// Pengguna uji dengan satu role dan token. Role dibuat bila belum ada (seperti tes detail).
async fn make_user(pool: &MySqlPool, email: &str, role: &str, nip: Option<&str>) -> (u64, String) {
    sqlx::query("INSERT INTO users (name, email, password, nip, created_at, updated_at) VALUES ('Uji Pix', ?, 'x', ?, NOW(), NOW())")
        .bind(email)
        .bind(nip)
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
    let rid: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
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
    let token = auth::login::create_token(pool, uid, "uji-pix")
        .await
        .unwrap();
    (uid, token)
}

async fn get(pool: &MySqlPool, token: &str, uri: &str) -> (StatusCode, Value) {
    let config = Config {
        app_env: "testing".into(),
        app_port: 0,
        request_timeout_secs: 30,
        body_limit_bytes: 1024 * 1024,
        app_url: "http://localhost".into(),
    };
    let res = app(
        &config,
        AppState::new(pool.clone(), "http://localhost".into()),
    )
    .oneshot(
        Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
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

/// Nama paket uji dalam respon, terurut.
fn names(body: &Value) -> Vec<String> {
    let mut v: Vec<String> = body["data"]
        .as_array()
        .expect("data harus array")
        .iter()
        .map(|d| d["nama_paket"].as_str().unwrap_or_default().to_string())
        .collect();
    v.sort();
    v
}

fn item<'a>(body: &'a Value, nama: &str) -> &'a Value {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["nama_paket"] == nama)
        .unwrap_or_else(|| panic!("{nama} tidak ada di respon"))
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn index_filters_paging_and_scope_match_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    // Fixture: satu kegiatan (tahun 9301), dua pengawas, tiga paket, satu tag.
    let keg: u64 = sqlx::query("INSERT INTO tbl_kegiatan (nama_program, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, sumber_dana, pagu, created_at, updated_at) VALUES ('uji-pix', 'uji-pix-keg', 'uji-pix-sub', '9301', 'uji-pix', 1000000.00, NOW(), NOW())")
        .execute(&pool).await.unwrap().last_insert_id();
    let peng_x: u64 = sqlx::query("INSERT INTO pengawas (nama, nip, jabatan, created_at, updated_at) VALUES ('uji-pix-pengawas-x', 'uji-pix-nip-x', 'Pengawas', NOW(), NOW())")
        .execute(&pool).await.unwrap().last_insert_id();
    let peng_y: u64 = sqlx::query("INSERT INTO pengawas (nama, nip, jabatan, created_at, updated_at) VALUES ('uji-pix-pengawas-y', 'uji-pix-nip-y', 'Pendamping', NOW(), NOW())")
        .execute(&pool).await.unwrap().last_insert_id();
    let insert_paket = "INSERT INTO tbl_pekerjaan (nama_paket, kode_rekening, kegiatan_id, pagu, is_konsultan, status, pengawas_id, pendamping_id, created_at, updated_at) VALUES (?, ?, ?, 100.0, ?, ?, ?, ?, NOW(), NOW())";
    let alpha: u64 = sqlx::query(insert_paket)
        .bind("uji-pix-alpha")
        .bind("uji-pix-rek-a")
        .bind(keg)
        .bind(1_i64)
        .bind("active")
        .bind(peng_x)
        .bind(peng_y)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    let _beta: u64 = sqlx::query(insert_paket)
        .bind("uji-pix-beta")
        .bind("uji-pix-rek-b")
        .bind(keg)
        .bind(0_i64)
        .bind("canceled")
        .bind(peng_x)
        .bind(None::<u64>)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    let _gamma: u64 = sqlx::query(insert_paket)
        .bind("uji-pix-gamma")
        .bind("uji-pix-rek-c")
        .bind(keg)
        .bind(0_i64)
        .bind("active")
        .bind(None::<u64>)
        .bind(None::<u64>)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    let tag: u64 = sqlx::query("INSERT INTO tbl_tags (name, slug, created_at, updated_at) VALUES ('uji-pix-tag', 'uji-pix-tag', NOW(), NOW())")
        .execute(&pool).await.unwrap().last_insert_id();
    sqlx::query("INSERT INTO pekerjaan_tag (pekerjaan_id, tag_id) VALUES (?, ?)")
        .bind(alpha)
        .bind(tag)
        .execute(&pool)
        .await
        .unwrap();

    let (_, admin) = make_user(&pool, ADMIN, "admin", None).await;
    // Pengawas: hanya paket yang di-assign (`user_pekerjaan`), dan nip cocok dengan pengawas X.
    let (peng_uid, peng_token) =
        make_user(&pool, PENGAWAS, "pengawas", Some("uji-pix-nip-x")).await;
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(peng_uid).bind(alpha)
        .execute(&pool).await.unwrap();

    let t = format!("tahun={TAHUN}");

    // Filter status: active = notCanceled, canceled, all dan kosong.
    let (s, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&status=canceled"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(names(&b), ["uji-pix-beta"]);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&status=active")).await;
    assert_eq!(names(&b), ["uji-pix-alpha", "uji-pix-gamma"]);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&status=all")).await;
    assert_eq!(b["meta"]["total"], 3);

    // is_konsultan: has() + boolean(); "" menjadi false.
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&is_konsultan=1")).await;
    assert_eq!(names(&b), ["uji-pix-alpha"]);
    assert_eq!(item(&b, "uji-pix-alpha")["is_konsultan"], true);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&is_konsultan=0")).await;
    assert_eq!(names(&b), ["uji-pix-beta", "uji-pix-gamma"]);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&is_konsultan=")).await;
    assert_eq!(names(&b), ["uji-pix-beta", "uji-pix-gamma"]);

    // pengawas_id, pendamping_id, dan tag_id.
    let (_, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&pengawas_id={peng_x}"),
    )
    .await;
    assert_eq!(names(&b), ["uji-pix-alpha", "uji-pix-beta"]);
    let (_, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&pendamping_id={peng_y}"),
    )
    .await;
    assert_eq!(names(&b), ["uji-pix-alpha"]);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&tag_id={tag}")).await;
    assert_eq!(names(&b), ["uji-pix-alpha"]);
    // "0" dan spasi saja diabaikan (empty() dan filled() di Laravel).
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&tag_id=0")).await;
    assert_eq!(b["meta"]["total"], 3);
    let (_, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&sub_bidang=%20%20"),
    )
    .await;
    assert_eq!(b["meta"]["total"], 3);
    let (_, all) = get(&pool, &admin, "/api/pekerjaan?per_page=1").await;
    let (_, zero) = get(&pool, &admin, "/api/pekerjaan?per_page=1&tahun=0").await;
    assert_eq!(
        zero["meta"]["total"], all["meta"]["total"],
        "tahun=0 harus diabaikan"
    );

    // Paginator: urutan query dipertahankan, prev url ada di halaman 2.
    let (s, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?page=2&{t}&per_page=2"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["data"].as_array().unwrap().len(), 1);
    assert_eq!(b["meta"]["from"], 3);
    assert_eq!(b["meta"]["to"], 3);
    assert_eq!(b["meta"]["total"], 3);
    assert_eq!(b["meta"]["last_page"], 2);
    assert_eq!(b["meta"]["per_page"], 2);
    let prev = format!("{BASE}?page=1&{t}&per_page=2");
    assert_eq!(b["links"]["prev"], prev.as_str());
    assert_eq!(b["meta"]["links"][0]["url"], prev.as_str());
    assert_eq!(b["meta"]["links"][0]["page"], 1);
    assert!(b["links"]["next"].is_null());
    assert_eq!(
        b["links"]["first"],
        format!("{BASE}?page=1&{t}&per_page=2").as_str()
    );
    assert_eq!(
        b["links"]["last"],
        format!("{BASE}?page=2&{t}&per_page=2").as_str()
    );

    // Halaman di luar jumlah halaman: data kosong, from dan to null, prev ke halaman terakhir.
    let (_, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&per_page=2&page=9"),
    )
    .await;
    assert!(b["data"].as_array().unwrap().is_empty());
    assert!(b["meta"]["from"].is_null());
    assert!(b["meta"]["to"].is_null());
    assert_eq!(b["meta"]["current_page"], 9);
    assert_eq!(
        b["links"]["prev"],
        format!("{BASE}?{t}&per_page=2&page=8").as_str()
    );

    // per_page: (int) PHP, minimal 1 (selain itu 20), maksimal 100.
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&per_page=2abc")).await;
    assert_eq!(b["meta"]["per_page"], 2);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&per_page=1e1")).await;
    assert_eq!(b["meta"]["per_page"], 10);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&per_page=0")).await;
    assert_eq!(b["meta"]["per_page"], 20);
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&per_page=500")).await;
    assert_eq!(b["meta"]["per_page"], 100);

    // sort_direction tidak valid pada kolom yang diizinkan: 500 (orderBy Laravel). Kolom tak dikenal: diabaikan.
    let (s, _) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&sort_by=pagu&sort_direction=foo"),
    )
    .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
    let (s, _) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&sort_by=bogus&sort_direction=foo"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&sort_by=pagu&sort_direction=ASC"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["meta"]["total"], 3);

    // per_page=-1 dengan summary: tanpa `pendamping`, dengan `tags` dan `kontrak`, tanpa `output`.
    let (_, b) = get(
        &pool,
        &admin,
        &format!("/api/pekerjaan?{t}&per_page=-1&summary=TRUE"),
    )
    .await;
    assert_eq!(
        names(&b),
        ["uji-pix-alpha", "uji-pix-beta", "uji-pix-gamma"]
    );
    let a = item(&b, "uji-pix-alpha");
    assert!(
        a.get("pendamping").is_none(),
        "pendamping tidak dimuat pada per_page=-1"
    );
    assert_eq!(a["pengawas"]["nama"], "uji-pix-pengawas-x");
    assert_eq!(a["tags"].as_array().unwrap().len(), 1);
    assert!(a["kontrak"].is_array());
    assert!(a.get("output").is_none());
    assert_eq!(
        a["assignment_sources"],
        serde_json::json!([]),
        "admin tidak punya assignment_sources"
    );

    // per_page=-1 tanpa summary: tanpa tags dan kontrak.
    let (_, b) = get(&pool, &admin, &format!("/api/pekerjaan?{t}&per_page=-1")).await;
    let a = item(&b, "uji-pix-alpha");
    assert!(a.get("pendamping").is_none());
    assert!(a.get("tags").is_none());
    assert!(a.get("kontrak").is_none());

    // Pengawas: hanya paket yang di-assign, dengan assignment_sources manual dan pengawas (nip cocok).
    let (s, b) = get(&pool, &peng_token, &format!("/api/pekerjaan?{t}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(names(&b), ["uji-pix-alpha"]);
    assert_eq!(
        item(&b, "uji-pix-alpha")["assignment_sources"],
        serde_json::json!(["manual", "pengawas"])
    );
    let (_, b) = get(
        &pool,
        &peng_token,
        &format!("/api/pekerjaan?{t}&status=canceled"),
    )
    .await;
    assert!(
        b["data"].as_array().unwrap().is_empty(),
        "beta tidak di-assign ke pengawas"
    );

    cleanup(&pool).await;
}
