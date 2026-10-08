//! Data quality (`DataQualityController`) dan `POST /api/client-error-reports` lewat router terhadap MySQL.
//!
//! Data uji memakai tahun anggaran `9871` dan nama `uji-dq*` / `uji-err*`, supaya hitungan dengan
//! `tahun=9871` hanya melihat baris uji. Tiket dan kontrak tidak difilter tahun, jadi hitungannya
//! dibandingkan dengan SQL yang sama di dalam tes.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test quality_insight_db -- --include-ignored
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

const ADMIN: &str = "uji-dq-admin@example.test";
const VIEWER: &str = "uji-dq-viewer@example.test";
const ERR_USER: &str = "uji-err-user@example.test";
const TAHUN: &str = "9871";

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
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(pool: &MySqlPool, uri: &str, token: &str) -> (StatusCode, Value) {
    send(pool, Method::GET, uri, Some(token), &[], None).await
}

/// Pengguna uji dengan peran `admin` atau tanpa peran, lalu token Sanctum.
async fn user_token(pool: &MySqlPool, email: &str, admin: bool) -> (u64, String) {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji DQ', ?, 'x', NOW(), NOW())")
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
    let token = auth::login::create_token(pool, uid, "uji-dq")
        .await
        .unwrap();
    (uid, token)
}

/// Hapus semua baris uji, dari anak ke induk. Hanya baris yang ditandai `uji-dq` / `uji-err`.
async fn cleanup(pool: &MySqlPool) {
    for sql in [
        "DELETE FROM tbl_tiket WHERE subjek LIKE 'uji-dq%'",
        "DELETE kp FROM kontrak_pekerjaan kp JOIN tbl_kontrak k ON k.id = kp.kontrak_id WHERE k.kode_paket LIKE 'uji-dq%'",
        "DELETE FROM tbl_kontrak WHERE kode_paket LIKE 'uji-dq%'",
        "DELETE f FROM tbl_foto f JOIN tbl_pekerjaan p ON p.id = f.pekerjaan_id WHERE p.nama_paket LIKE 'uji-dq%'",
        "DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE 'uji-dq%'",
        "DELETE FROM tbl_kegiatan WHERE nama_kegiatan = 'uji-dq' AND tahun_anggaran = '9871'",
    ] {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
    sqlx::query("DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email IN (?, ?))")
        .bind(ADMIN)
        .bind(VIEWER)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email IN (?, ?)")
        .bind(ADMIN)
        .bind(VIEWER)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_pekerjaan(
    pool: &MySqlPool,
    kegiatan: u64,
    nama: &str,
    kode: &str,
    status: &str,
) -> u64 {
    sqlx::query(
        "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kecamatan_id, desa_id, kegiatan_id, pagu, \
         is_konsultan, status, created_at, updated_at) VALUES (?, ?, NULL, NULL, ?, 1500000, 0, ?, NOW(), NOW())",
    )
    .bind(kode)
    .bind(nama)
    .bind(kegiatan)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query_scalar("SELECT id FROM tbl_pekerjaan WHERE nama_paket = ? ORDER BY id DESC LIMIT 1")
        .bind(nama)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_foto(pool: &MySqlPool, pekerjaan: u64) {
    sqlx::query(
        "INSERT INTO tbl_foto (pekerjaan_id, komponen_id, keterangan, koordinat, validasi_koordinat, created_at, updated_at) \
         VALUES (?, 1, '0%', '-6.82,107.14', 1, NOW(), NOW())",
    )
    .bind(pekerjaan)
    .execute(pool)
    .await
    .unwrap();
}

/// Kontrak uji. `legacy` mengisi `id_pekerjaan`, `pivot` mengisi `kontrak_pekerjaan`.
async fn insert_kontrak(
    pool: &MySqlPool,
    kegiatan: u64,
    legacy: Option<u64>,
    pivot: Option<u64>,
    selesai_hari: i64,
) -> u64 {
    sqlx::query(
        "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, kode_paket, tgl_selesai, created_at, updated_at) \
         VALUES (?, ?, 'uji-dq', DATE_ADD(CURDATE(), INTERVAL ? DAY), NOW(), NOW())",
    )
    .bind(kegiatan)
    .bind(legacy)
    .bind(selesai_hari)
    .execute(pool)
    .await
    .unwrap();
    let id: u64 = sqlx::query_scalar(
        "SELECT id FROM tbl_kontrak WHERE kode_paket = 'uji-dq' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    if let Some(p) = pivot {
        sqlx::query("INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(id)
            .bind(p)
            .execute(pool)
            .await
            .unwrap();
    }
    id
}

async fn insert_tiket(
    pool: &MySqlPool,
    user: u64,
    pekerjaan: Option<u64>,
    prioritas: &str,
    status: &str,
) {
    sqlx::query(
        "INSERT INTO tbl_tiket (user_id, pekerjaan_id, subjek, deskripsi, prioritas, status, created_at, updated_at) \
         VALUES (?, ?, 'uji-dq tiket', 'uji', ?, ?, NOW(), NOW())",
    )
    .bind(user)
    .bind(pekerjaan)
    .bind(prioritas)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
}

/// Hitungan tiket dan kontrak global (tanpa filter tahun), dihitung ulang dengan SQL yang sama.
async fn expected_tiket(pool: &MySqlPool, high: bool) -> i64 {
    let prioritas = if high {
        " AND t.prioritas = 'high'"
    } else {
        ""
    };
    sqlx::query_scalar(&format!(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_tiket t WHERE t.status IN ('open','pending'){prioritas} \
         AND (t.pekerjaan_id IS NULL OR EXISTS (SELECT 1 FROM tbl_pekerjaan p WHERE p.id = t.pekerjaan_id \
         AND (p.status IS NULL OR p.status <> 'canceled')))"
    ))
    .fetch_one(pool)
    .await
    .unwrap()
}

fn action<'a>(actions: &'a [Value], id: &str) -> Option<&'a Value> {
    actions.iter().find(|a| a["id"] == id)
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn data_quality_stats_items_and_inbox_match_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    cleanup(&pool).await;

    let (admin_id, admin) = user_token(&pool, ADMIN, true).await;
    let (_, viewer) = user_token(&pool, VIEWER, false).await;

    // Kegiatan uji dengan tahun 9871.
    sqlx::query("INSERT INTO tbl_kegiatan (nama_kegiatan, tahun_anggaran, created_at, updated_at) VALUES ('uji-dq', ?, NOW(), NOW())")
        .bind(TAHUN)
        .execute(&pool)
        .await
        .unwrap();
    let kegiatan: u64 = sqlx::query_scalar("SELECT id FROM tbl_kegiatan WHERE nama_kegiatan = 'uji-dq' AND tahun_anggaran = ? ORDER BY id DESC LIMIT 1")
        .bind(TAHUN)
        .fetch_one(&pool)
        .await
        .unwrap();

    // P1: tanpa foto, tanpa kontrak -> no_coordinates, no_photos, no_contracts.
    let p1 = insert_pekerjaan(&pool, kegiatan, "uji-dq paket 1", "uji-dq-001", "active").await;
    // P2: ada foto dengan koordinat, tanpa kontrak -> no_contracts saja.
    let p2 = insert_pekerjaan(&pool, kegiatan, "uji-dq paket 2", "uji-dq-002", "active").await;
    insert_foto(&pool, p2).await;
    // P3: kontrak legacy, tanpa foto -> started_no_photos, no_coordinates, no_photos.
    let p3 = insert_pekerjaan(&pool, kegiatan, "uji-dq paket 3", "uji-dq-003", "active").await;
    // P4: dibatalkan -> tidak dihitung di mana pun.
    let p4 = insert_pekerjaan(&pool, kegiatan, "uji-dq paket 4", "uji-dq-004", "canceled").await;
    // P5: kontrak lewat pivot, tanpa foto -> started_no_photos, no_coordinates, no_photos.
    let p5 = insert_pekerjaan(&pool, kegiatan, "uji-dq paket 5", "uji-dq-005", "active").await;

    insert_kontrak(&pool, kegiatan, Some(p3), None, 10).await;
    insert_kontrak(&pool, kegiatan, None, Some(p5), 5).await;
    // Kontrak untuk paket dibatalkan: tidak masuk kotak masuk.
    insert_kontrak(&pool, kegiatan, Some(p4), None, 7).await;
    // Kontrak selesai di luar 30 hari, tanpa paket: tidak masuk.
    insert_kontrak(&pool, kegiatan, None, None, 45).await;

    // Tiket: satu terbuka prioritas tinggi untuk P1, satu untuk paket dibatalkan P4 (tidak dihitung).
    insert_tiket(&pool, admin_id, Some(p1), "high", "open").await;
    insert_tiket(&pool, admin_id, Some(p4), "high", "open").await;
    insert_tiket(&pool, admin_id, None, "low", "pending").await;

    // ---- Auth ----
    let (s, _) = send(
        &pool,
        Method::GET,
        "/api/data-quality/stats",
        None,
        &[],
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, body) = get(&pool, "/api/data-quality/stats", &viewer).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(
        body["message"],
        "Akses ditolak. Route ini hanya dapat diakses oleh admin."
    );

    // ---- Stats dengan tahun uji ----
    let (s, body) = get(
        &pool,
        &format!("/api/data-quality/stats?tahun={TAHUN}"),
        &admin,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(
        body["data"],
        json!({
            "no_coordinates": 3,
            "no_photos": 3,
            "started_no_photos": 2,
            "no_contracts": 2,
            "total_jobs": 4,
        })
    );

    // tahun=0 dianggap tidak ada (truthiness PHP), jadi hitungan global dan minimal sebanyak data uji.
    let (_, body0) = get(&pool, "/api/data-quality/stats?tahun=0", &admin).await;
    assert!(body0["data"]["total_jobs"].as_i64().unwrap() >= 4);

    // ---- Items ----
    let (s, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=no_coordinates&tahun={TAHUN}"),
        &admin,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let data = body["data"].as_array().unwrap();
    let names: Vec<&str> = data
        .iter()
        .map(|d| d["nama_paket"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["uji-dq paket 1", "uji-dq paket 3", "uji-dq paket 5"]
    );
    assert_eq!(data[0]["id"], p1);
    assert_eq!(data[0]["kode_rekening"], "uji-dq-001");
    assert_eq!(data[0]["pagu"], json!(1500000.0));
    assert_eq!(data[0]["kecamatan"], Value::Null);
    assert_eq!(data[0]["desa"], Value::Null);
    assert_eq!(data[0]["pengawas"], Value::Null);
    assert_eq!(data[0]["issue"], "no_coordinates");
    assert_eq!(data[0]["href"], format!("/pekerjaan/{p1}"));
    assert_eq!(
        body["meta"],
        json!({ "current_page": 1, "last_page": 1, "per_page": 25, "total": 3 })
    );

    // Paginasi: per_page=2, halaman 2 berisi satu item.
    let (_, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=no_coordinates&tahun={TAHUN}&per_page=2&page=2"),
        &admin,
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["id"], p5);
    assert_eq!(
        body["meta"],
        json!({ "current_page": 2, "last_page": 2, "per_page": 2, "total": 3 })
    );

    // Isu lain dan href-nya.
    let (_, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=started_no_photos&tahun={TAHUN}"),
        &admin,
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 2);
    assert_eq!(body["data"][0]["href"], format!("/foto?pekerjaanId={p3}"));

    let (_, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=no_contracts&tahun={TAHUN}"),
        &admin,
    )
    .await;
    let ids: Vec<i64> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [p1 as i64, p2 as i64]);
    assert_eq!(
        body["data"][0]["href"],
        format!("/kontrak/new?pekerjaanId={p1}")
    );

    // Pencarian pada kode rekening, dan search "0" tidak memfilter (empty() di PHP).
    let (_, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=no_coordinates&tahun={TAHUN}&search=uji-dq-005"),
        &admin,
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["id"], p5);
    let (_, body) = get(
        &pool,
        &format!("/api/data-quality/items?issue=no_coordinates&tahun={TAHUN}&search=0"),
        &admin,
    )
    .await;
    assert_eq!(body["data"].as_array().unwrap().len(), 3);

    // Validasi 422 dengan pesan Laravel.
    let long = "x".repeat(201);
    for uri in [
        "/api/data-quality/items".to_string(),
        "/api/data-quality/items?issue=bogus".to_string(),
        format!("/api/data-quality/items?issue=no_photos&tahun=abc"),
        format!("/api/data-quality/items?issue=no_photos&per_page=0"),
        format!("/api/data-quality/items?issue=no_photos&per_page=101"),
        format!("/api/data-quality/items?issue=no_photos&per_page=abc"),
        format!("/api/data-quality/items?issue=no_photos&search={long}"),
    ] {
        let (s, body) = get(&pool, &uri, &admin).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert_eq!(body["message"], "The given data was invalid.", "{uri}");
        assert!(body["errors"].is_object(), "{uri}");
    }
    let (_, body) = get(&pool, "/api/data-quality/items?issue=bogus", &admin).await;
    assert_eq!(
        body["errors"]["issue"],
        json!(["The selected issue is invalid."])
    );

    // ---- Kotak masuk ----
    let (s, body) = get(
        &pool,
        &format!("/api/data-quality/action-inbox?tahun={TAHUN}"),
        &admin,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["excludes_canceled_pekerjaan"], true);
    assert!(data["generated_at"].as_str().unwrap().ends_with("+00:00"));
    assert_eq!(data["stats"]["no_coordinates"], 3);
    assert_eq!(data["stats"]["total_jobs"], 4);
    let actions = data["actions"].as_array().unwrap();
    assert_eq!(
        data["total_actions"].as_u64().unwrap(),
        actions.len() as u64
    );

    let nc = action(actions, "dq-no_coordinates").expect("dq-no_coordinates");
    assert_eq!(nc["title"], "3 pekerjaan Tanpa koordinat");
    assert_eq!(nc["severity"], "high");
    assert_eq!(nc["source"], "data_quality");
    assert_eq!(nc["count"], 3);
    assert_eq!(
        nc["href"],
        format!("/data-quality?issue=no_coordinates&tahun={TAHUN}")
    );

    let sn = action(actions, "dq-started_no_photos").expect("dq-started_no_photos");
    assert_eq!(sn["title"], "2 pekerjaan Berkontrak tanpa foto");
    assert_eq!(sn["severity"], "high");

    let np = action(actions, "dq-no_photos").expect("dq-no_photos");
    assert_eq!(np["severity"], "medium");
    assert_eq!(np["count"], 3);

    let nk = action(actions, "dq-no_contracts").expect("dq-no_contracts");
    assert_eq!(nk["severity"], "medium");
    assert_eq!(nk["count"], 2);

    // Kontrak selesai <= 30 hari: P3 (legacy, 10 hari) dan P5 (pivot, 5 hari). P4 dan 45 hari tidak.
    let kh = action(actions, "kontrak-h30").expect("kontrak-h30");
    assert_eq!(kh["title"], "2 kontrak berakhir ≤ 30 hari");
    assert_eq!(kh["count"], 2);
    assert_eq!(kh["href"], "/kontrak");

    // Tiket: hitungan global, dibandingkan dengan SQL yang sama.
    let expected_high = expected_tiket(&pool, true).await;
    let expected_all = expected_tiket(&pool, false).await;
    assert!(expected_high >= 1);
    let th = action(actions, "tiket-high").expect("tiket-high");
    assert_eq!(th["count"], expected_high);
    assert_eq!(
        th["title"],
        format!("{expected_high} tiket prioritas tinggi terbuka")
    );
    let to = action(actions, "tiket-open").expect("tiket-open");
    assert_eq!(to["count"], expected_all);
    let expected_sev = if expected_all > 20 { "medium" } else { "low" };
    assert_eq!(to["severity"], expected_sev);

    // Urutan: severity high, lalu medium, lalu low.
    let rank = |s: &str| match s {
        "high" => 0,
        "medium" => 1,
        _ => 2,
    };
    let ranks: Vec<i32> = actions
        .iter()
        .map(|a| rank(a["severity"].as_str().unwrap()))
        .collect();
    assert!(
        ranks.windows(2).all(|w| w[0] <= w[1]),
        "urutan severity: {ranks:?}"
    );

    // Tanpa tahun: href dasar tanpa &tahun.
    let (_, body) = get(&pool, "/api/data-quality/action-inbox", &admin).await;
    let global_nc = action(
        body["data"]["actions"].as_array().unwrap(),
        "dq-no_coordinates",
    );
    if let Some(g) = global_nc {
        assert_eq!(g["href"], "/data-quality?issue=no_coordinates");
    }

    // Akses non-admin ditolak juga di kotak masuk.
    let (s, _) = get(&pool, "/api/data-quality/action-inbox", &viewer).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn client_error_report_stores_and_validates_like_laravel() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    sqlx::query("DELETE FROM error_logs WHERE message LIKE 'uji-err%'")
        .execute(&pool)
        .await
        .unwrap();
    let (uid, token) = user_token(&pool, ERR_USER, false).await;

    // Tanpa token: 401.
    let (s, _) = send(
        &pool,
        Method::POST,
        "/api/client-error-reports",
        None,
        &[],
        Some(json!({ "source": "manual", "message": "uji-err tanpa token" })),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Simpan lengkap, dengan X-Forwarded-For dan teks yang perlu di-trim.
    let (s, body) = send(
        &pool,
        Method::POST,
        "/api/client-error-reports",
        Some(&token),
        &[
            ("x-forwarded-for", "203.0.113.9, 10.0.0.1"),
            ("user-agent", "uji-agent"),
        ],
        Some(json!({
            "source": "react",
            "message": "  uji-err pesan  ",
            "stack": "Error: x\n at y",
            "component_stack": "",
            "url": "https://example.test/halaman",
            "user_agent": "uji-browser",
            "metadata": { "route": "/foto", "count": 2 },
            "app": "arumanis",
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(body, json!({ "success": true }));

    let row = sqlx::query(
        "SELECT user_id, source, message, stack, component_stack, url, user_agent, ip_address, \
         CAST(metadata AS CHAR) AS metadata, resolved_at FROM error_logs WHERE message = 'uji-err pesan' \
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.try_get::<u64, _>("user_id").unwrap(), uid);
    assert_eq!(row.try_get::<String, _>("source").unwrap(), "react");
    assert_eq!(
        row.try_get::<Option<String>, _>("stack")
            .unwrap()
            .as_deref(),
        Some("Error: x\n at y")
    );
    // component_stack kosong menjadi NULL (ConvertEmptyStringsToNull).
    assert_eq!(
        row.try_get::<Option<String>, _>("component_stack").unwrap(),
        None
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("url").unwrap().as_deref(),
        Some("https://example.test/halaman")
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("user_agent")
            .unwrap()
            .as_deref(),
        Some("uji-browser")
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("ip_address")
            .unwrap()
            .as_deref(),
        Some("203.0.113.9")
    );
    assert_eq!(
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("resolved_at")
            .unwrap(),
        None
    );
    let meta: Value = serde_json::from_str(&row.try_get::<String, _>("metadata").unwrap()).unwrap();
    assert_eq!(
        meta,
        json!({ "route": "/foto", "count": 2, "app": "arumanis" })
    );

    // Metadata kosong dan tanpa app: disimpan NULL.
    let (s, _) = send(
        &pool,
        Method::POST,
        "/api/client-error-reports",
        Some(&token),
        &[],
        Some(json!({ "source": "manual", "message": "uji-err kosong", "metadata": {} })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let meta: Option<String> = sqlx::query_scalar(
        "SELECT CAST(metadata AS CHAR) FROM error_logs WHERE message = 'uji-err kosong' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(meta, None);

    // Validasi: setiap kasus 422 dengan pesan Laravel untuk field terkait.
    let long = "x".repeat(5001);
    let cases: Vec<(Value, &str)> = vec![
        (json!({ "message": "uji-err a" }), "source"),
        (
            json!({ "source": "bogus", "message": "uji-err a" }),
            "source",
        ),
        (json!({ "source": 5, "message": "uji-err a" }), "source"),
        (json!({ "source": "manual" }), "message"),
        (json!({ "source": "manual", "message": "   " }), "message"),
        (json!({ "source": "manual", "message": long }), "message"),
        (
            json!({ "source": "manual", "message": "uji-err a", "stack": 12 }),
            "stack",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "component_stack": ["x"] }),
            "component_stack",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "url": "x".repeat(5001) }),
            "url",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "user_agent": "x".repeat(2001) }),
            "user_agent",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "metadata": "bukan array" }),
            "metadata",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "metadata": 3 }),
            "metadata",
        ),
        (
            json!({ "source": "manual", "message": "uji-err a", "app": "x".repeat(65) }),
            "app",
        ),
    ];
    for (payload, field) in cases {
        let (s, body) = send(
            &pool,
            Method::POST,
            "/api/client-error-reports",
            Some(&token),
            &[],
            Some(payload.clone()),
        )
        .await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{payload}");
        assert_eq!(body["message"], "The given data was invalid.", "{payload}");
        assert!(
            body["errors"][field].is_array(),
            "field {field} pada {payload}: {body}"
        );
    }

    // Body bukan JSON diperlakukan kosong: source dan message wajib.
    let (s, body) = send(
        &pool,
        Method::POST,
        "/api/client-error-reports",
        Some(&token),
        &[],
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["errors"]["source"],
        json!(["The source field is required."])
    );
    assert_eq!(
        body["errors"]["message"],
        json!(["The message field is required."])
    );

    // Pesan error sesuai aturan pertama yang gagal untuk source tidak dikenal.
    let (_, body) = send(
        &pool,
        Method::POST,
        "/api/client-error-reports",
        Some(&token),
        &[],
        Some(json!({ "source": "bogus", "message": "uji-err a" })),
    )
    .await;
    assert_eq!(
        body["errors"]["source"],
        json!(["The selected source is invalid."])
    );

    // Tidak ada baris tersimpan dari kasus gagal.
    let leftover: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM error_logs WHERE message LIKE 'uji-err%' AND message NOT IN ('uji-err pesan', 'uji-err kosong')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(leftover, 0);

    // Bersihkan baris uji dan pengguna uji.
    sqlx::query("DELETE FROM error_logs WHERE message LIKE 'uji-err%'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(ERR_USER)
        .execute(&pool)
        .await
        .unwrap();
}
