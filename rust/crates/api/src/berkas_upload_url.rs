//! `POST /api/berkas/upload-from-url` (`BerkasController::uploadFromUrl`).
//!
//! Body JSON: `pekerjaan_id` (wajib, harus ada di `tbl_pekerjaan`), `jenis_dokumen` (wajib, string
//! maks 255), dan `url` (wajib, URL valid). Berkas diunduh lalu disimpan sebagai media pada koleksi
//! `berkas/dokumen`, seperti `addMediaFromUrl` di Spatie.
//!
//! Perbedaan dengan Laravel:
//! - Pengunduhan dilakukan sebelum baris `tbl_berkas` dibuat. Laravel membuat baris dulu lalu
//!   menghapusnya bila gagal. Hasil akhirnya sama, tetapi id auto-increment bisa berbeda.
//! - Keamanan (SSRF): hanya http dan https. Alamat loopback, private, link-local, dan lainnya ditolak.
//!   Hostname diselesaikan lebih dulu, dan hasilnya dipakai untuk koneksi (`resolve_to_addrs`), jadi
//!   DNS tidak dibaca dua kali. Laravel tidak punya pemeriksaan ini.
//! - Redirect tidak diikuti (Laravel mengikuti). Alasannya, redirect bisa menuju alamat internal.
//! - Batas ukuran `media::MAX_FILE_BYTES` (50 MB). Laravel tidak membatasi di sini.
//! - Tipe MIME ditebak dari ekstensi nama berkas (`media::mime_for_name`), bukan dari isi berkas.
//! - Seperti Laravel, tidak ada pemeriksaan scope pekerjaan di sini (`ensure_scope` tidak dipanggil).
//!   Pembatasan hanya lewat middleware permission route.
//! - `uploaded_by` tidak diisi, dan relasi `uploader` tidak dimuat. Laravel juga tidak mengisinya.

use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{berkas, changes, media, require_auth, AppState};

const MODEL: &str = "App\\Models\\Berkas";
const COLLECTION: &str = "berkas/dokumen";
const DOWNLOAD_MESSAGE: &str =
    "Gagal mendownload file dari URL. Pastikan URL mengarah langsung ke file.";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn add(errs: &mut BTreeMap<String, Vec<String>>, field: &str, message: &str) {
    errs.entry(field.to_string())
        .or_default()
        .push(message.to_string());
}

/// Alamat yang boleh diunduh: bukan loopback, private, link-local, multicast, atau tak tentu.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_unspecified()
        || o[0] == 0
        || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64.0.0/10 (CGNAT)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
        || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18.0.0/15
        || o[0] >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique local
        || (first & 0xffc0) == 0xfe80) // link-local
}

/// Unduh berkas. Setiap kegagalan (skema, alamat, jaringan, status, ukuran) memakai pesan yang sama.
async fn download(raw_url: &str) -> Result<Vec<u8>, String> {
    let url = reqwest::Url::parse(raw_url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("skema tidak didukung".to_string());
    }
    let host = url.host_str().ok_or("URL tanpa host")?.to_string();
    let port = url.port_or_known_default().ok_or("URL tanpa port")?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((bare, port))
            .await
            .map_err(|e| e.to_string())?
            .collect(),
    };
    if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
        return Err("alamat tujuan tidak diizinkan".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(bare, &addrs)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > media::MAX_FILE_BYTES as u64)
    {
        return Err("berkas terlalu besar".to_string());
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > media::MAX_FILE_BYTES {
        return Err("berkas terlalu besar".to_string());
    }
    Ok(bytes.to_vec())
}

/// Nama berkas dari segmen terakhir path URL, seperti `pathinfo(parse_url(...), PATHINFO_BASENAME)`.
fn file_name_from_url(raw_url: &str) -> String {
    reqwest::Url::parse(raw_url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.next_back().map(str::to_string))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "berkas".to_string())
}

async fn pekerjaan_exists(pool: &MySqlPool, id: i64) -> Result<bool, ApiError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(count > 0)
}

/// `POST /api/berkas/upload-from-url`.
pub async fn upload_from_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut errs = BTreeMap::new();

    let pekerjaan_id = match input.get("pekerjaan_id") {
        None | Some(Value::Null) => {
            add(&mut errs, "pekerjaan_id", "The pekerjaan id field is required.");
            None
        }
        Some(v) => {
            let parsed = match v {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            let found = match parsed {
                Some(id) => pekerjaan_exists(&state.pool, id).await?.then_some(id),
                None => None,
            };
            if found.is_none() {
                add(&mut errs, "pekerjaan_id", "The selected pekerjaan id is invalid.");
            }
            found
        }
    };

    let jenis = match input.get("jenis_dokumen") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) if s.chars().count() > 255 => {
            add(
                &mut errs,
                "jenis_dokumen",
                "The jenis dokumen field must not be greater than 255 characters.",
            );
            None
        }
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            add(&mut errs, "jenis_dokumen", "The jenis dokumen field must be a string.");
            None
        }
    };
    if jenis.is_none() && !errs.contains_key("jenis_dokumen") {
        add(&mut errs, "jenis_dokumen", "The jenis dokumen field is required.");
    }

    let url = match input.get("url") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) => {
            let valid = reqwest::Url::parse(s).is_ok_and(|u| u.host_str().is_some());
            if valid {
                Some(s.clone())
            } else {
                add(&mut errs, "url", "The url field must be a valid URL.");
                None
            }
        }
        Some(_) => {
            add(&mut errs, "url", "The url field must be a valid URL.");
            None
        }
    };
    if url.is_none() && !errs.contains_key("url") {
        add(&mut errs, "url", "The url field is required.");
    }

    let (Some(pekerjaan_id), Some(jenis), Some(url)) = (pekerjaan_id, jenis, url) else {
        return Err(ApiError::validation("The given data was invalid.", errs));
    };
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }

    // Unduh dulu. Bila gagal, tidak ada baris yang perlu dihapus.
    let bytes = download(&url)
        .await
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, DOWNLOAD_MESSAGE))?;
    let upload = media::Upload {
        original_name: file_name_from_url(&url),
        bytes,
    };
    let mime = media::mime_for_name(&upload.original_name);

    let api_url = format!("{}/api/berkas", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(&jenis)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let row = berkas::find_row(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("berkas baru tidak terbaca"))?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::BERKAS,
        "created",
        id,
        None,
        Some(berkas::attributes(&row)),
        Some(pekerjaan_id),
        &api_url,
    )
    .await?;
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, &upload, mime, false).await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }

    let data = berkas::resource(&state.pool, &state.app_url, &row, false).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_loopback_addresses_are_refused() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.0.5", "172.16.9.9", "169.254.1.1", "0.0.0.0", "100.64.1.1", "::1", "fc00::1", "fe80::1", "::ffff:10.0.0.1"] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip} harus ditolak");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip} harus diizinkan");
        }
    }

    #[test]
    fn file_name_comes_from_last_path_segment() {
        assert_eq!(file_name_from_url("https://x.test/a/b/surat.pdf?x=1"), "surat.pdf");
        assert_eq!(file_name_from_url("https://x.test/"), "berkas");
    }
}
