//! `GET /api/pekerjaan/document-register` terhadap MySQL: auth, bentuk baris, filter, konsolidasi, dan scope.
//!
//! Semua baris uji memakai awalan `uji-pdr-`. Tes membersihkan baris itu sebelum dan sesudah berjalan.
//! Tabel `tbl_berita_acara` dibuat dari `rust/fixtures/pekerjaan_doc_register_schema.sql` bila belum ada.
//!
//! ```bash
//! DATABASE_URL='mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock' \
//!     cargo test -p api --test pekerjaan_doc_register_db -- --include-ignored
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

const ADMIN: &str = "uji-pdr-admin@example.test";
const PENGAWAS: &str = "uji-pdr-pengawas@example.test";
const TAHUN: &str = "2099";

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

async fn user_token(pool: &MySqlPool, email: &str, role: &str) -> String {
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
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES ('Uji PDR', ?, 'x', NOW(), NOW())")
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
    let role_id: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
            .bind(role)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role_id)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    auth::login::create_token(pool, uid, "uji-pdr")
        .await
        .unwrap()
}

/// Hapus semua baris uji. Urutan mengikuti ketergantungan kunci asing.
async fn cleanup(pool: &MySqlPool) {
    let pk = "SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE 'uji-pdr-%'";
    let statements = [
        format!("DELETE FROM tbl_penerima WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_output WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_berkas WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM tbl_berita_acara WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM user_pekerjaan WHERE pekerjaan_id IN ({pk})"),
        format!("DELETE FROM kontrak_pekerjaan WHERE pekerjaan_id IN ({pk})"),
        "DELETE FROM tbl_document_registers WHERE nomor LIKE 'uji-pdr-%'".to_string(),
        "DELETE FROM tbl_pekerjaan WHERE nama_paket LIKE 'uji-pdr-%'".to_string(),
        "DELETE FROM kontrak_pekerjaan WHERE kontrak_id IN (SELECT id FROM tbl_kontrak WHERE sppbj LIKE 'uji-pdr-%')"
            .to_string(),
        "DELETE FROM tbl_kontrak WHERE sppbj LIKE 'uji-pdr-%'".to_string(),
        "DELETE FROM tbl_penyedia WHERE nama LIKE 'uji-pdr-%'".to_string(),
        "DELETE FROM tbl_kegiatan WHERE nama_kegiatan LIKE 'uji-pdr-%'".to_string(),
        "DELETE FROM tbl_document_types WHERE code = 'UJI-PDR'".to_string(),
        "DELETE FROM model_has_roles WHERE model_id IN (SELECT id FROM users WHERE email LIKE 'uji-pdr-%')".to_string(),
        "DELETE FROM users WHERE email LIKE 'uji-pdr-%'".to_string(),
    ];
    for sql in statements {
        sqlx::query(&sql).execute(pool).await.unwrap();
    }
}

async fn apply_fixture(pool: &MySqlPool) {
    sqlx::query(include_str!(
        "../../../fixtures/pekerjaan_doc_register_schema.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_id(pool: &MySqlPool, sql: &str, binds: Vec<String>) -> i64 {
    let mut q = sqlx::query(sql);
    for b in &binds {
        q = q.bind(b);
    }
    q.execute(pool).await.unwrap().last_insert_id() as i64
}

struct Seed {
    pekerjaan: [i64; 4],
    kontrak_a: i64,
    pengawas_token: String,
    admin_token: String,
}

/// Dua kontrak. Kontrak A dipakai P1 dan P2 (konsolidasi). Kontrak B dipakai P3 dengan SPK kosong.
/// P4 tidak punya kontrak dan tidak boleh muncul.
async fn seed(pool: &MySqlPool) -> Seed {
    let kegiatan = insert_id(
        pool,
        "INSERT INTO tbl_kegiatan (nama_program, nama_kegiatan, tahun_anggaran, sumber_dana, pagu, kode_rekening, nama_pptk, created_at, updated_at) \
         VALUES ('uji-pdr-program', 'uji-pdr-kegiatan', ?, 'APBD', 2500000.00, '[\"1.02.03\"]', 'uji-pdr-pptk', NOW(), NOW())",
        vec![TAHUN.into()],
    )
    .await;
    let pen_a = insert_id(
        pool,
        "INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, tanggal_akta, alamat, npwp, bank, norek, created_at, updated_at) \
         VALUES ('uji-pdr-penyedia-A', 'Direktur A', 'AKTA-A', 'Notaris A', '2020-01-02', 'Jl. A', NULL, NULL, NULL, NOW(), NOW())",
        vec![],
    )
    .await;
    let pen_b = insert_id(
        pool,
        "INSERT INTO tbl_penyedia (nama, direktur, no_akta, notaris, alamat, created_at, updated_at) \
         VALUES ('uji-pdr-penyedia-B', 'Direktur B', 'AKTA-B', 'Notaris B', 'Jl. B', NOW(), NOW())",
        vec![],
    )
    .await;
    let kontrak_a = insert_id(
        pool,
        "INSERT INTO tbl_kontrak (id_kegiatan, id_penyedia, sppbj, spk, spmk, nilai_kontrak, tgl_spk, created_at, updated_at) \
         VALUES (?, ?, 'uji-pdr-sppbj-A', 'uji-pdr-spk-A', 'uji-pdr-spmk-A', 1500000.50, '2026-04-23', NOW(), NOW())",
        vec![kegiatan.to_string(), pen_a.to_string()],
    )
    .await;
    let kontrak_b = insert_id(
        pool,
        "INSERT INTO tbl_kontrak (id_kegiatan, id_penyedia, sppbj, spk, spmk, created_at, updated_at) \
         VALUES (?, ?, 'uji-pdr-sppbj-B', '   ', NULL, NOW(), NOW())",
        vec![kegiatan.to_string(), pen_b.to_string()],
    )
    .await;

    let mut pekerjaan = [0i64; 4];
    for (i, slot) in pekerjaan.iter_mut().enumerate() {
        *slot = insert_id(
            pool,
            "INSERT INTO tbl_pekerjaan (kode_rekening, nama_paket, kegiatan_id, pagu, is_konsultan, status, created_at, updated_at) \
             VALUES ('1.02.03', ?, ?, 1500000.5, 0, 'active', NOW(), NOW())",
            vec![format!("uji-pdr-paket-{}", i + 1), kegiatan.to_string()],
        )
        .await;
    }
    let [p1, p2, p3, _p4] = pekerjaan;
    for (kontrak, p) in [(kontrak_a, p1), (kontrak_a, p2), (kontrak_b, p3)] {
        insert_id(
            pool,
            "INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
            vec![kontrak.to_string(), p.to_string()],
        )
        .await;
    }

    let type_id = insert_id(
        pool,
        "INSERT INTO tbl_document_types (name, code, format_template, created_at, updated_at) \
         VALUES ('Uji PDR', 'UJI-PDR', '{sequence}/{code}-UJI/{month}/{year}', NOW(), NOW())",
        vec![],
    )
    .await;
    insert_id(
        pool,
        "INSERT INTO tbl_document_registers (kontrak_id, type_id, nomor, tanggal, sequence_number, year, description, nilai, created_at, updated_at) \
         VALUES (?, ?, 'uji-pdr-nomor-1', '2099-03-05', 1, 2099, 'uji-pdr-ket', 2000000.00, NOW(), NOW())",
        vec![kontrak_a.to_string(), type_id.to_string()],
    )
    .await;
    insert_id(
        pool,
        "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, penerima_is_optional, created_at, updated_at) \
         VALUES (?, 'uji-pdr-komponen', 'M3', 12, 0, NOW(), NOW())",
        vec![p1.to_string()],
    )
    .await;
    insert_id(
        pool,
        "INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, created_at, updated_at) VALUES (?, 'uji-pdr-dokumen', NOW(), NOW())",
        vec![p1.to_string()],
    )
    .await;
    insert_id(
        pool,
        "INSERT INTO tbl_penerima (pekerjaan_id, nama) VALUES (?, 'uji-pdr-penerima')",
        vec![p1.to_string()],
    )
    .await;
    // P1: serah terima "0" (PHP empty, tidak dihitung). P3: tanggal (dihitung).
    insert_id(
        pool,
        "INSERT INTO tbl_berita_acara (pekerjaan_id, data, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
        vec![p1.to_string(), r#"{"serah_terima_pertama": "0"}"#.into()],
    )
    .await;
    insert_id(
        pool,
        "INSERT INTO tbl_berita_acara (pekerjaan_id, data, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
        vec![p3.to_string(), r#"{"serah_terima_pertama": "2099-01-01"}"#.into()],
    )
    .await;

    let pengawas_token = user_token(pool, PENGAWAS, "pengawas").await;
    let pengawas_id: i64 =
        sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM users WHERE email = ?")
            .bind(PENGAWAS)
            .fetch_one(pool)
            .await
            .unwrap();
    insert_id(
        pool,
        "INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
        vec![pengawas_id.to_string(), p3.to_string()],
    )
    .await;
    let admin_token = user_token(pool, ADMIN, "admin").await;

    Seed {
        pekerjaan,
        kontrak_a,
        pengawas_token,
        admin_token,
    }
}

fn ids(data: &Value) -> Vec<i64> {
    data.as_array()
        .unwrap_or_else(|| panic!("data bukan array: {data}"))
        .iter()
        .map(|d| d["id"].as_i64().unwrap())
        .collect()
}

fn sorted_keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_register_requires_auth() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let (status, _) = get(&pool, "/api/pekerjaan/document-register", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn document_register_shape_filters_and_scope() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    apply_fixture(&pool).await;
    cleanup(&pool).await;
    let s = seed(&pool).await;
    let [p1, p2, p3, _p4] = s.pekerjaan;
    let admin = s.admin_token.as_str();
    let base = "/api/pekerjaan/document-register?tahun=2099";

    // Halaman 1 (per_page=1): P1 di halaman, P2 ditambahkan karena berbagi kontrak A.
    let (status, body) = get(&pool, &format!("{base}&per_page=1"), Some(admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    assert_eq!(ids(&body["data"]), vec![p1, p2], "{body}");
    assert_eq!(
        body["meta"],
        json!({
            "current_page": 1, "last_page": 3, "per_page": 1, "total": 3,
            "from": 1, "to": 1,
            "summary": { "spk_missing": 1, "spmk_missing": 1, "pho_completed": 1 }
        }),
        "{body}"
    );

    // Bentuk baris P1 (model Pekerjaan mentah).
    let p = &body["data"][0];
    assert_eq!(
        sorted_keys(p),
        vec![
            "beritaAcara",
            "berkas",
            "catatan",
            "created_at",
            "desa_id",
            "foto_count",
            "id",
            "is_konsultan",
            "kecamatan_id",
            "kegiatan",
            "kegiatan_id",
            "kode_rekening",
            "kontrak",
            "nama_paket",
            "output",
            "pagu",
            "pendamping_id",
            "penerima_count",
            "pengawas_id",
            "status",
            "updated_at",
        ],
        "{p}"
    );
    assert_eq!(p["nama_paket"], "uji-pdr-paket-1");
    assert_eq!(p["pagu"], json!(1500000.5));
    assert_eq!(p["is_konsultan"], json!(false));
    assert_eq!(p["penerima_count"], json!(1));
    assert_eq!(p["foto_count"], json!(0));
    assert!(
        p["created_at"].as_str().unwrap().ends_with("000000Z"),
        "{p}"
    );
    assert_eq!(p["kegiatan"]["tahun_anggaran"], TAHUN);
    assert_eq!(p["kegiatan"]["pagu"], "2500000.00");
    assert_eq!(p["kegiatan"]["kode_rekening"], json!(["1.02.03"]));
    assert_eq!(
        p["beritaAcara"]["data"],
        json!({ "serah_terima_pertama": "0" })
    );
    assert_eq!(p["output"][0]["volume"], "12.00");
    assert_eq!(p["output"][0]["penerima_is_optional"], json!(false));
    assert_eq!(
        sorted_keys(&p["output"][0]),
        vec![
            "id",
            "komponen",
            "pekerjaan_id",
            "penerima_is_optional",
            "satuan",
            "volume"
        ]
    );
    assert_eq!(p["berkas"][0]["jenis_dokumen"], "uji-pdr-dokumen");

    let k = &p["kontrak"][0];
    assert_eq!(k["id"], json!(s.kontrak_a));
    assert_eq!(k["pivot"]["pekerjaan_id"], json!(p1));
    assert_eq!(k["pivot"]["kontrak_id"], json!(s.kontrak_a));
    assert_eq!(k["penyedia"]["nama"], "uji-pdr-penyedia-A");
    assert_eq!(k["penyedia"]["tanggal_akta"], "2020-01-02T00:00:00.000000Z");
    assert_eq!(k["nilai_kontrak"], json!(1500000.5));
    assert_eq!(k["tgl_spk"], "2026-04-23T00:00:00.000000Z");
    assert_eq!(k["tgl_spmk"], Value::Null);
    assert_eq!(k["spse_push_log"], Value::Null);
    assert!(
        k.get("addendums").is_none(),
        "addendum tidak dimuat di endpoint ini"
    );
    assert_eq!(k["registers"][0]["nomor"], "uji-pdr-nomor-1");
    assert_eq!(k["registers"][0]["tanggal"], "2099-03-05T00:00:00.000000Z");
    assert_eq!(k["registers"][0]["nilai"], json!(2000000));
    assert_eq!(k["registers"][0]["type"]["code"], "UJI-PDR");
    assert!(k["registers"][0].get("addendum").is_none());

    // P2 (dari konsolidasi, tanpa berita acara dan output).
    let p2_row = &body["data"][1];
    assert_eq!(p2_row["beritaAcara"], Value::Null);
    assert_eq!(p2_row["output"], json!([]));

    // Halaman 2 (per_page=1): P2 di halaman, P1 ditambahkan sebagai konsolidasi.
    let (status, body) = get(&pool, &format!("{base}&per_page=1&page=2"), Some(admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body["data"]), vec![p2, p1], "{body}");
    assert_eq!(body["meta"]["current_page"], 2);
    assert_eq!(body["meta"]["from"], 2);
    assert_eq!(body["meta"]["to"], 2);

    // per_page=-1: semua, tanpa `success`, dengan summary atas seluruh hasil filter.
    let (status, body) = get(&pool, &format!("{base}&per_page=-1"), Some(admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("success").is_none(), "{body}");
    assert_eq!(ids(&body["data"]), vec![p1, p2, p3], "{body}");
    assert_eq!(
        body["meta"],
        json!({
            "total": 3,
            "summary": { "spk_missing": 1, "spmk_missing": 1, "pho_completed": 1 }
        }),
        "{body}"
    );

    // Filter search: penyedia, nomor register, nama paket, dan SPPBJ.
    let search = |term: &str| {
        format!("/api/pekerjaan/document-register?per_page=-1&tahun=2099&search={term}")
    };
    let (_, body) = get(&pool, &search("uji-pdr-penyedia-B"), Some(admin)).await;
    assert_eq!(ids(&body["data"]), vec![p3], "{body}");
    let (_, body) = get(&pool, &search("uji-pdr-nomor"), Some(admin)).await;
    assert_eq!(ids(&body["data"]), vec![p1, p2], "{body}");
    let (_, body) = get(&pool, &search("uji-pdr-paket-3"), Some(admin)).await;
    assert_eq!(ids(&body["data"]), vec![p3], "{body}");
    let (_, body) = get(&pool, &search("uji-pdr-sppbj-A"), Some(admin)).await;
    assert_eq!(ids(&body["data"]), vec![p1, p2], "{body}");

    // Tahun tanpa data: tidak ada halaman, from dan to null.
    let (status, body) = get(
        &pool,
        "/api/pekerjaan/document-register?tahun=2098",
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"], json!([]));
    assert_eq!(body["meta"]["total"], 0);
    assert_eq!(body["meta"]["last_page"], 1);
    assert_eq!(body["meta"]["from"], Value::Null);
    assert_eq!(body["meta"]["to"], Value::Null);

    // Pengawas hanya melihat paket yang di-assign (P3). Paket konsolidasi tidak ada di sini.
    let (status, body) = get(
        &pool,
        "/api/pekerjaan/document-register?per_page=-1&tahun=2099",
        Some(&s.pengawas_token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body["data"]), vec![p3], "{body}");
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["meta"]["summary"]["pho_completed"], 1);

    // per_page=0: `paginate()` memakai `Model::$perPage` (15), bukan error dan bukan 20.
    let (status, body) = get(&pool, &format!("{base}&per_page=0"), Some(admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body["data"]), vec![p1, p2, p3], "{body}");
    assert_eq!(body["meta"]["per_page"], 15, "{body}");
    assert_eq!(body["meta"]["last_page"], 1, "{body}");
    assert_eq!(body["meta"]["from"], 1, "{body}");
    assert_eq!(body["meta"]["to"], 3, "{body}");

    // per_page negatif selain -1: tanpa batas (semua baris), last_page 1, from/to dari rumus Laravel.
    let (status, body) = get(&pool, &format!("{base}&per_page=-2"), Some(admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true, "{body}");
    assert_eq!(ids(&body["data"]), vec![p1, p2, p3], "{body}");
    assert_eq!(body["meta"]["per_page"], -2, "{body}");
    assert_eq!(body["meta"]["last_page"], 1, "{body}");
    assert_eq!(body["meta"]["current_page"], 1, "{body}");
    assert_eq!(body["meta"]["total"], 3, "{body}");
    assert_eq!(body["meta"]["from"], 1, "{body}");
    assert_eq!(body["meta"]["to"], 3, "{body}");
    let (_, body) = get(&pool, &format!("{base}&per_page=-2&page=2"), Some(admin)).await;
    assert_eq!(ids(&body["data"]), vec![p1, p2, p3], "{body}");
    assert_eq!(body["meta"]["current_page"], 2, "{body}");
    assert_eq!(body["meta"]["from"], -1, "{body}");
    assert_eq!(body["meta"]["to"], 1, "{body}");

    // Datetime: `created_at` dan timestamp pivot memakai Carbon toJSON (UTC, 6 digit).
    let all = body_all(&pool, admin, base).await;
    let p1_row = &all["data"][0];
    assert_eq!(p1_row["id"], p1);
    let ts = |v: &Value| -> bool {
        let s = v.as_str().unwrap_or_default();
        s.len() == 27 && s.as_bytes()[10] == b'T' && s.ends_with(".000000Z")
    };
    assert!(ts(&p1_row["created_at"]), "{p1_row}");
    assert!(ts(&p1_row["kontrak"][0]["pivot"]["created_at"]), "{p1_row}");
    assert!(ts(&p1_row["kontrak"][0]["created_at"]), "{p1_row}");

    // Kolom `array` cast: list dari kunci "0..n-1" berurutan, `{}` menjadi [], objek tetap objek.
    sqlx::query(
        "UPDATE tbl_berita_acara SET data = ? WHERE pekerjaan_id = ?",
    )
    .bind(r#"{"serah_terima_pertama": "0", "lampiran": {"0": "x", "1": "y"}, "kosong": {}, "urut": {"1": "b", "0": "a"}}"#)
    .bind(p1)
    .execute(&pool)
    .await
    .unwrap();
    let body = body_all(&pool, admin, base).await;
    let data = &body["data"][0]["beritaAcara"]["data"];
    assert_eq!(data["lampiran"], json!(["x", "y"]), "{body}");
    assert_eq!(data["kosong"], json!([]), "{body}");
    assert!(data["urut"].is_object(), "{body}");
    assert_eq!(data["urut"]["0"], "a", "{body}");
    assert_eq!(body["meta"]["summary"]["pho_completed"], 1, "{body}");

    cleanup(&pool).await;
}

/// Semua baris untuk `base` (per_page=-1), dipakai untuk memeriksa bentuk baris.
async fn body_all(pool: &MySqlPool, token: &str, base: &str) -> Value {
    let (status, body) = get(pool, &format!("{base}&per_page=-1"), Some(token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}
