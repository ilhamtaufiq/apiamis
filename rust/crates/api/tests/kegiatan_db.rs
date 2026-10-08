//! Paritas `GET /api/kegiatan` terhadap fixture produksi, memakai data lokal `tbl_kegiatan`.
//!
//! Butuh database yang berisi data kegiatan dari dump (bukan skema kosong):
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test kegiatan_db -- --ignored
//! ```
//!
//! Timestamp dan field pribadi (`nama_pptk`, `nip_pptk`) tidak dibandingkan:
//! timestamp di dump lokal berbeda dari produksi, dan field pribadi di fixture sudah dihapus.

use api::kegiatan::{list, to_resource};
use serde_json::Value;
use sqlx::MySqlPool;

const FIXTURE: &str = include_str!("../../../fixtures/live/kegiatan_index.json");

/// Selisih data yang sudah diketahui antara dump lokal dan produksi (lihat log T14).
/// Jangan menambah entri tanpa mencatatnya di log migrasi.
const KNOWN_DATA_DRIFT: &[(u64, &str)] = &[(29, "sumber_dana")];

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_kegiatan"]
async fn rows_match_production_fixture_except_timestamps_and_personal_fields() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();

    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let items = fixture["body"]["data"].as_array().unwrap();

    let (rows, total) = list(&pool, None, Some((15, 0))).await.unwrap();
    assert_eq!(total, 20, "jumlah kegiatan di DB lokal");

    let mut mismatches = Vec::new();
    for item in items {
        let id = item["id"].as_u64().unwrap();
        let row = rows.iter().find(|r| r.id == id).expect("id ada di DB");
        let got = to_resource(row);
        for key in [
            "nama_program",
            "sub_bidang",
            "nama_kegiatan",
            "nama_sub_kegiatan",
            "tahun_anggaran",
            "sumber_dana",
            "pagu",
            "kode_rekening",
            "sipd_id_sub_bl",
            "kode_sub_giat",
        ] {
            if KNOWN_DATA_DRIFT.contains(&(id, key)) {
                continue;
            }
            if got[key] != item[key] {
                mismatches.push(format!(
                    "id {id} {key}: db={} fixture={}",
                    got[key], item[key]
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "selisih:\n{}", mismatches.join("\n"));
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_kegiatan"]
async fn show_matches_list_row_and_missing_id_is_none() {
    use api::kegiatan::{find, to_resource};

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let (rows, _) = list(&pool, None, Some((15, 0))).await.unwrap();
    let first = rows.first().expect("ada data kegiatan");

    let shown = find(&pool, first.id).await.unwrap().expect("id ada");
    assert_eq!(to_resource(&shown), to_resource(first));
    assert!(find(&pool, 999_999_999).await.unwrap().is_none());
}
