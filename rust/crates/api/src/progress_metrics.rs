//! Metrik progres fisik, setara `ProgressTabMetricsService` (Laravel).
//!
//! Bobot dihitung dari nilai RAB item (volume × harga × 1,11), lalu bobot itu
//! dipakai untuk menimbang realisasi dan rencana mingguan.
//! Semantik PHP yang ditiru: cast `(float)` untuk string, `??` untuk null,
//! dan `round(x, 2)`.
//! Belum diverifikasi terhadap data produksi (tabel `tbl_progress` belum ada datanya).

use serde_json::Value;

pub const RAB_PPN_RATE: f64 = 0.11;

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub progress_total: f64,
    pub progress_rencana: f64,
    pub deviasi: f64,
    pub max_reported_week: i64,
}

/// `(float)` di PHP: ambil awalan numerik, selain itu 0.
pub fn php_float(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => {
            let t = s.trim_start();
            let mut end = 0;
            let bytes = t.as_bytes();
            if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
                end += 1;
            }
            let mut seen_digit = false;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
                seen_digit = true;
            }
            if end < bytes.len() && bytes[end] == b'.' {
                end += 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                    seen_digit = true;
                }
            }
            if !seen_digit {
                return 0.0;
            }
            t[..end].parse().unwrap_or(0.0)
        }
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// `$x !== null && $x !== ''`
fn present(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `(int)` untuk kunci minggu: angka awal, selain itu 0.
fn week_key(k: &str) -> i64 {
    k.trim().parse::<i64>().unwrap_or(0)
}

/// Iterasi `weekly_data` (objek atau array) sebagai pasangan (minggu, data).
fn weeks(v: Option<&Value>) -> Vec<(i64, &Value)> {
    match v {
        Some(Value::Object(m)) => m.iter().map(|(k, d)| (week_key(k), d)).collect(),
        Some(Value::Array(a)) => a.iter().enumerate().map(|(i, d)| (i as i64, d)).collect(),
        _ => Vec::new(),
    }
}

/// `round($x, 2)` di PHP (pembulatan setengah menjauhi nol).
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn item_rab_value(volume: f64, harga_satuan: f64) -> f64 {
    volume * harga_satuan * (1.0 + RAB_PPN_RATE)
}

/// `summarize()`: `content` adalah JSON `tbl_progress.content`.
pub fn summarize(content: Option<&Value>) -> Metrics {
    let zero = Metrics {
        progress_total: 0.0,
        progress_rencana: 0.0,
        deviasi: 0.0,
        max_reported_week: 0,
    };
    let items: Vec<&Value> = match content.and_then(|c| c.get("items")) {
        Some(Value::Array(a)) => a.iter().collect(),
        _ => return zero,
    };
    if items.is_empty() {
        return zero;
    }

    let mut max_week = 0i64;
    for item in &items {
        for (minggu, data) in weeks(item.get("weekly_data")) {
            if data.is_object() && data.get("realisasi").is_some() && present(data.get("realisasi"))
            {
                max_week = max_week.max(minggu);
            }
        }
    }

    let rab_of = |item: &Value| {
        item_rab_value(
            php_float(item.get("target_volume").unwrap_or(&Value::Null)),
            php_float(item.get("harga_satuan").unwrap_or(&Value::Null)),
        )
    };
    let total_rab_base: f64 = items.iter().map(|i| rab_of(i)).sum();

    let mut progress_total = 0.0;
    let mut progress_rencana = 0.0;
    for item in &items {
        let target = php_float(item.get("target_volume").unwrap_or(&Value::Null));
        let item_rab = rab_of(item);
        let bobot = if total_rab_base > 0.0 && item_rab > 0.0 {
            (item_rab / total_rab_base) * 100.0
        } else {
            php_float(item.get("bobot").unwrap_or(&Value::Null))
        };

        let mut total_real = 0.0;
        let mut total_rencana = 0.0;
        for (minggu, data) in weeks(item.get("weekly_data")) {
            if !data.is_object() {
                continue;
            }
            if present(data.get("realisasi")) {
                total_real += php_float(&data["realisasi"]);
            }
            if max_week > 0 && minggu <= max_week && present(data.get("rencana")) {
                total_rencana += php_float(&data["rencana"]);
            }
        }

        let pct_real = if target > 0.0 {
            total_real / target * 100.0
        } else {
            0.0
        };
        progress_total += pct_real * bobot / 100.0;
        let pct_rencana = if target > 0.0 {
            total_rencana / target * 100.0
        } else {
            0.0
        };
        progress_rencana += pct_rencana * bobot / 100.0;
    }

    let progress_total = round2(progress_total);
    let progress_rencana = round2(progress_rencana);
    Metrics {
        progress_total,
        progress_rencana,
        deviasi: round2(progress_total - progress_rencana),
        max_reported_week: max_week,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn php_float_matches_cast_rules() {
        assert_eq!(php_float(&json!("12,5")), 12.0);
        assert_eq!(php_float(&json!(" 3.25abc")), 3.25);
        assert_eq!(php_float(&json!("abc")), 0.0);
        assert_eq!(php_float(&Value::Null), 0.0);
        assert_eq!(php_float(&json!(7)), 7.0);
    }

    #[test]
    fn empty_or_missing_items_give_zeros() {
        let zero = summarize(None);
        assert_eq!(zero.progress_total, 0.0);
        assert_eq!(summarize(Some(&json!({ "items": [] }))).deviasi, 0.0);
        assert_eq!(
            summarize(Some(&json!({ "items": "x" }))).max_reported_week,
            0
        );
    }

    #[test]
    fn single_item_progress_uses_rab_weight() {
        // RAB = 10 × 100 × 1,11 = 1110 → bobot 100%. Realisasi 5 dari target 10 → 50%.
        // Rencana minggu 1..2 = 6 dari 10 → 60%, tetapi maksimal minggu terlapor = 1 (realisasi minggu 1).
        let content = json!({
            "items": [{
                "target_volume": 10,
                "harga_satuan": 100,
                "weekly_data": {
                    "1": { "realisasi": 5, "rencana": 4 },
                    "2": { "realisasi": null, "rencana": 2 }
                }
            }]
        });
        let m = summarize(Some(&content));
        assert_eq!(m.max_reported_week, 1);
        assert_eq!(m.progress_total, 50.0);
        // Rencana hanya minggu <= 1: 4 dari 10 → 40%.
        assert_eq!(m.progress_rencana, 40.0);
        assert_eq!(m.deviasi, 10.0);
    }

    #[test]
    fn two_items_are_weighted_by_rab() {
        // Item A: RAB 1110 (bobot 50%), realisasi 100% → 50.
        // Item B: RAB 1110 (bobot 50%), realisasi 0% → 0.
        let content = json!({
            "items": [
                { "target_volume": 1, "harga_satuan": 1000, "weekly_data": { "1": { "realisasi": 1 } } },
                { "target_volume": 1, "harga_satuan": 1000, "weekly_data": { "1": { "realisasi": 0 } } }
            ]
        });
        let m = summarize(Some(&content));
        assert_eq!(m.progress_total, 50.0);
    }

    #[test]
    fn falls_back_to_saved_bobot_without_price() {
        let content = json!({
            "items": [{ "target_volume": 4, "harga_satuan": 0, "bobot": "25", "weekly_data": { "1": { "realisasi": 2 } } }]
        });
        // Bobot tersimpan 25; realisasi 2 dari 4 → 50%; progres = 50 × 25 / 100 = 12,5.
        assert_eq!(summarize(Some(&content)).progress_total, 12.5);
    }
}
