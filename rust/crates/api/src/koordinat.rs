//! Port `KoordinatValidationService`: membaca koordinat dan mengecek apakah titik berada
//! di dalam batas desa pada GeoJSON.
//!
//! Lokasi GeoJSON: env `VILLAGE_GEOJSON_PATH`, bila tidak ada memakai
//! `resources/geojson/id3203_cianjur_simplified.geojson` di repo Laravel.

use std::{collections::HashMap, path::PathBuf, sync::OnceLock};

use serde_json::Value;
use std::collections::BTreeMap;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::json;
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{foto, require_auth, AppState};

#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub valid: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

/// Port `validateForPekerjaan`. Nama desa dan kecamatan diambil dari tabel wilayah.
pub async fn validate_for_pekerjaan(
    pool: &MySqlPool,
    pekerjaan_id: u64,
    koordinat: &str,
) -> Result<Validation, sqlx::Error> {
    let names: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT d.n_desa, k.n_kec FROM tbl_pekerjaan p \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         LEFT JOIN tbl_kecamatan k ON k.id = p.kecamatan_id \
         WHERE p.id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await?;
    let (desa, kec) = names.unwrap_or((None, None));
    Ok(validate(desa.as_deref(), kec.as_deref(), koordinat))
}

/// Inti validasi tanpa database, urutan pemeriksaan sama dengan Laravel.
pub fn validate(desa: Option<&str>, kec: Option<&str>, koordinat: &str) -> Validation {
    let Some(coords) = parse_koordinat(koordinat) else {
        return Validation {
            valid: false,
            message: "Koordinat tidak dapat dibaca. Gunakan format lat, lng.".into(),
        };
    };

    let (Some(desa), Some(kec)) = (
        desa.filter(|s| !s.is_empty()),
        kec.filter(|s| !s.is_empty()),
    ) else {
        return Validation {
            valid: false,
            message: "Data desa/kecamatan pekerjaan belum lengkap.".into(),
        };
    };

    let Some(geometry) = village_index().get(&index_key(kec, desa)) else {
        return Validation {
            valid: false,
            message: format!("Batas desa {desa} tidak ditemukan di peta."),
        };
    };

    if point_inside(coords.lng, coords.lat, geometry) {
        Validation {
            valid: true,
            message: format!("Koordinat berada di Desa {desa}, Kec. {kec}."),
        }
    } else {
        Validation {
            valid: false,
            message: format!("Koordinat di luar Desa {desa}, Kec. {kec}."),
        }
    }
}

/// Port `parseKoordinat`. Menerima `lat, lng` dan format tanpa koma (`-7.16539810 7.1545`).
pub fn parse_koordinat(value: &str) -> Option<LatLng> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("manual") {
        return None;
    }

    let bytes = trimmed.as_bytes();
    for start in 0..bytes.len() {
        let Some(end1) = number_end(bytes, start) else {
            continue;
        };
        let comma = skip_ws(bytes, end1);
        if bytes.get(comma) != Some(&b',') {
            continue;
        }
        let second = skip_ws(bytes, comma + 1);
        if let Some(end2) = number_end(bytes, second) {
            let lat = trimmed[start..end1].parse::<f64>().ok()?;
            let lng = trimmed[second..end2].parse::<f64>().ok()?;
            return normalize(lat, lng);
        }
    }

    // Tanpa koma: hapus semua spasi lalu cari "10\d\.\d+" sebagai awal longitude.
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let cb = cleaned.as_bytes();
    let marker = (0..cb.len()).find(|&i| is_lng_marker(cb, i))?;
    let lat = php_float(&cleaned[..marker]);
    let lng = php_float(&cleaned[marker..]);
    normalize(lat, lng)
}

/// Pola `-?\d+(\.\d+)?` dimulai di `i`. Mengembalikan posisi akhir match.
fn number_end(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    let digits_start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    Some(i)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// Pola `10\d\.\d+` dimulai di `i`.
fn is_lng_marker(b: &[u8], i: usize) -> bool {
    b.get(i) == Some(&b'1')
        && b.get(i + 1) == Some(&b'0')
        && b.get(i + 2).is_some_and(u8::is_ascii_digit)
        && b.get(i + 3) == Some(&b'.')
        && b.get(i + 4).is_some_and(u8::is_ascii_digit)
}

/// `(float)` PHP: ambil prefiks numerik terpanjang, selain itu 0.
fn php_float(s: &str) -> f64 {
    (1..=s.len())
        .rev()
        .filter_map(|n| s.get(..n))
        .find_map(|p| p.parse::<f64>().ok().filter(|v| v.is_finite()))
        .unwrap_or(0.0)
}

fn normalize(lat: f64, lng: f64) -> Option<LatLng> {
    if !lat.is_finite() || !lng.is_finite() {
        return None;
    }
    let (lat, lng) = correct_indonesia_signs(lat, lng);
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return None;
    }
    Some(LatLng { lat, lng })
}

/// OCR sering menghilangkan tanda minus pada lintang Jawa/Bali dan bujur barat.
fn correct_indonesia_signs(mut lat: f64, mut lng: f64) -> (f64, f64) {
    if lat > 0.0 && lat <= 12.0 && (104.0..=115.0).contains(&lng) {
        lat = -lat;
    }
    if lng < 0.0 && (-115.0..=-104.0).contains(&lng) {
        lng = -lng;
    }
    (lat, lng)
}

/// Port `normalizeWilayahName` + pembentukan kunci `kecamatan|desa`.
fn index_key(kec: &str, desa: &str) -> String {
    format!("{}|{}", normalize_wilayah(kec), normalize_wilayah(desa))
}

fn normalize_wilayah(value: &str) -> String {
    let mut s = value.to_string();
    for word in ["kecamatan", "desa", "kelurahan"] {
        s = replace_ci(&s, word);
    }
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// `str_ireplace($pattern, '', $s)`: hapus kemunculan tanpa membedakan huruf besar-kecil.
fn replace_ci(s: &str, pattern: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (idx, _) in lower.match_indices(pattern) {
        out.push_str(&s[last..idx]);
        last = idx + pattern.len();
    }
    out.push_str(&s[last..]);
    out
}

fn geojson_path() -> PathBuf {
    std::env::var_os("VILLAGE_GEOJSON_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../resources/geojson/id3203_cianjur_simplified.geojson"
            ))
        })
}

static VILLAGE_INDEX: OnceLock<HashMap<String, Value>> = OnceLock::new();

/// Indeks `kecamatan|desa` ke `geometry`. File tidak terbaca menghasilkan indeks kosong (seperti Laravel).
fn village_index() -> &'static HashMap<String, Value> {
    VILLAGE_INDEX.get_or_init(|| {
        let raw = std::fs::read_to_string(geojson_path()).unwrap_or_default();
        let data: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let mut index = HashMap::new();
        for feature in data["features"].as_array().into_iter().flatten() {
            let props = &feature["properties"];
            let district = props["district"].as_str().unwrap_or("");
            let village = props["village"].as_str().unwrap_or("");
            if let Some(geometry) = feature.get("geometry") {
                index.insert(index_key(district, village), geometry.clone());
            }
        }
        index
    })
}

fn point_inside(lng: f64, lat: f64, geometry: &Value) -> bool {
    let Some(coords) = geometry["coordinates"].as_array() else {
        return false;
    };
    match geometry["type"].as_str() {
        Some("Polygon") => rings_contain(lng, lat, coords),
        Some("MultiPolygon") => coords
            .iter()
            .filter_map(Value::as_array)
            .any(|poly| rings_contain(lng, lat, poly)),
        _ => false,
    }
}

/// Ring pertama adalah batas luar; ring berikutnya adalah lubang.
fn rings_contain(lng: f64, lat: f64, rings: &[Value]) -> bool {
    let Some(outer) = rings.first().and_then(Value::as_array) else {
        return false;
    };
    if !ring_contains(lng, lat, outer) {
        return false;
    }
    !rings[1..]
        .iter()
        .filter_map(Value::as_array)
        .any(|hole| ring_contains(lng, lat, hole))
}

/// Ray casting, sama dengan `pointInRing` di PHP.
fn ring_contains(lng: f64, lat: f64, ring: &[Value]) -> bool {
    let pts: Vec<(f64, f64)> = ring
        .iter()
        .filter_map(|p| {
            let a = p.as_array()?;
            Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
        })
        .collect();
    if pts.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        let denominator = yj - yi;
        if denominator.abs() >= 1e-12
            && ((yi > lat) != (yj > lat))
            && lng < (xj - xi) * (lat - yi) / denominator + xi
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// `POST /api/koordinat/validate` (`KoordinatValidationController@validateKoordinat`).
pub async fn validate_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let pekerjaan_id = body.get("pekerjaan_id").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    });
    let koordinat = body
        .get("koordinat")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    match pekerjaan_id {
        None => foto::add(
            &mut errs,
            "pekerjaan_id",
            "The pekerjaan id field is required.".into(),
        ),
        Some(p) => {
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
                .bind(p)
                .fetch_one(&state.pool)
                .await
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if exists == 0 {
                foto::add(
                    &mut errs,
                    "pekerjaan_id",
                    "The selected pekerjaan id is invalid.".into(),
                );
            }
        }
    }
    match koordinat {
        None => foto::add(
            &mut errs,
            "koordinat",
            "The koordinat field is required.".into(),
        ),
        Some(k) if k.chars().count() > 255 => foto::add(
            &mut errs,
            "koordinat",
            "The koordinat field must not be greater than 255 characters.".into(),
        ),
        Some(_) => {}
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let (Some(pekerjaan_id), Some(koordinat)) = (
        pekerjaan_id,
        body.get("koordinat")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    ) else {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "validasi koordinat tidak lengkap",
        ));
    };
    let hasil = validate_for_pekerjaan(&state.pool, pekerjaan_id, &koordinat)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "validasi_koordinat": hasil.valid,
        "validasi_koordinat_message": hasil.message,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_separated_pair() {
        let c = parse_koordinat("-7.165398, 107.154517").unwrap();
        assert!((c.lat + 7.165398).abs() < 1e-9);
        assert!((c.lng - 107.154517).abs() < 1e-9);
    }

    #[test]
    fn fixes_missing_minus_only_when_longitude_is_in_java_range() {
        // Sama dengan PHP: lintang diperbaiki hanya bila bujur 104..=115.
        let java = parse_koordinat("6.794353, 107.228834").unwrap();
        assert!(
            (java.lat + 6.794353).abs() < 1e-9,
            "lintang Jawa diberi minus"
        );
        let west = parse_koordinat("7.165398, -107.154517").unwrap();
        assert!((west.lat - 7.165398).abs() < 1e-9, "lintang tidak diubah");
        assert!(
            (west.lng - 107.154517).abs() < 1e-9,
            "bujur diberi tanda plus"
        );
        let out = parse_koordinat("12.5,110").unwrap();
        assert!(
            (out.lat - 12.5).abs() < 1e-9,
            "lintang 12.5 di luar koreksi"
        );
    }

    #[test]
    fn parses_pair_without_comma() {
        let c = parse_koordinat("-7.16539841 107.1545166").unwrap();
        assert!((c.lat + 7.16539841).abs() < 1e-9);
        assert!((c.lng - 107.1545166).abs() < 1e-9);
        let spaced = parse_koordinat("7.16539841 107.1545166").unwrap();
        assert!((spaced.lat + 7.16539841).abs() < 1e-9);
    }

    #[test]
    fn rejects_manual_and_garbage() {
        assert!(parse_koordinat("manual").is_none());
        assert!(parse_koordinat("  ").is_none());
        assert!(parse_koordinat("not-a-coordinate").is_none());
        assert!(
            parse_koordinat("95, 107").is_none(),
            "lintang di luar rentang"
        );
    }

    #[test]
    fn normalizes_wilayah_names_like_laravel() {
        assert_eq!(
            normalize_wilayah("Kecamatan Babakancaringin"),
            "babakancaringin"
        );
        assert_eq!(normalize_wilayah("DESA Sukamaju"), "sukamaju");
        assert_eq!(normalize_wilayah("Kelurahan Cibeber-1"), "cibeber1");
    }

    #[test]
    fn validate_checks_names_before_geometry() {
        let no_name = validate(None, Some("Cianjur"), "-7.1, 107.1");
        assert!(!no_name.valid);
        assert_eq!(
            no_name.message,
            "Data desa/kecamatan pekerjaan belum lengkap."
        );
        let bad = validate(Some("X"), Some("Y"), "tidak-valid");
        assert_eq!(
            bad.message,
            "Koordinat tidak dapat dibaca. Gunakan format lat, lng."
        );
    }

    #[test]
    fn ring_with_hole_excludes_hole_points() {
        let square: Value = serde_json::json!([[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]);
        let hole: Value = serde_json::json!([[4, 4], [4, 6], [6, 6], [6, 4], [4, 4]]);
        let rings = vec![square, hole];
        assert!(rings_contain(1.0, 1.0, &rings));
        assert!(!rings_contain(5.0, 5.0, &rings), "titik di lubang");
        assert!(!rings_contain(11.0, 1.0, &rings), "di luar batas");
    }

    /// Memakai GeoJSON asli di repo, sama seperti test PHP `pointInsideFeature`.
    #[test]
    fn real_village_boundary_matches_laravel() {
        let inside = validate(
            Some("Babakancaringin"),
            Some("Karangtengah"),
            "-6.8, 107.21",
        );
        assert!(inside.valid, "{}", inside.message);
        assert_eq!(
            inside.message,
            "Koordinat berada di Desa Babakancaringin, Kec. Karangtengah."
        );
        let outside = validate(Some("Babakancaringin"), Some("Karangtengah"), "-7.5, 106.5");
        assert!(!outside.valid);
        assert_eq!(
            outside.message,
            "Koordinat di luar Desa Babakancaringin, Kec. Karangtengah."
        );
    }

    #[test]
    fn multipolygon_matches_any_part() {
        let geometry = serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0, 0], [0, 1], [1, 1], [1, 0], [0, 0]]],
                [[[5, 5], [5, 6], [6, 6], [6, 5], [5, 5]]]
            ]
        });
        assert!(point_inside(5.5, 5.5, &geometry));
        assert!(!point_inside(3.0, 3.0, &geometry));
    }
}
