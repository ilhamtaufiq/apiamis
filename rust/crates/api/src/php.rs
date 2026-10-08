//! Helper semantik PHP untuk port dashboard, analitik, dan pencarian.
//!
//! Berisi hal-hal kecil yang Laravel lakukan secara implisit: `round()`, `(int)`/`(float)`
//! pada nilai campuran, `empty()`/truthiness string, `trim()` dengan karakter default,
//! `rawurlencode()`, `stripos()`, dan parser `$request->query()` (nilai terakhir menang,
//! `kunci[]` menjadi array).

use serde_json::Value;

/// `round($value, $places)` PHP 8: setengah menjauhi nol dengan pra-pembulatan 15 digit.
/// Nilai di luar presisi (>= 1e15 setelah dikali) dikembalikan apa adanya, seperti PHP.
pub fn round(value: f64, places: i32) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let factor = 10f64.powi(places);
    let tmp = value * factor;
    if !tmp.is_finite() || tmp.abs() >= 1e15 {
        return value;
    }
    let pre: f64 = format!("{tmp:.14e}").parse().unwrap_or(tmp);
    pre.round() / factor
}

/// `(int)` pada string: awalan numerik, selain itu 0. Notasi ilmiah dan titik desimal dipotong.
pub fn intval_str(s: &str) -> i64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let bytes = t.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return 0;
    }
    // Bentuk desimal atau eksponen: PHP memotong ke integer lewat float.
    let has_frac_or_exp = end < bytes.len() && (bytes[end] == b'.' || bytes[end] == b'e' || bytes[end] == b'E');
    if has_frac_or_exp {
        let mut e = end;
        if e < bytes.len() && bytes[e] == b'.' {
            e += 1;
            while e < bytes.len() && bytes[e].is_ascii_digit() {
                e += 1;
            }
        }
        let mut num_end = e;
        if e < bytes.len() && (bytes[e] == b'e' || bytes[e] == b'E') {
            let mut x = e + 1;
            if x < bytes.len() && (bytes[x] == b'+' || bytes[x] == b'-') {
                x += 1;
            }
            if x < bytes.len() && bytes[x].is_ascii_digit() {
                while x < bytes.len() && bytes[x].is_ascii_digit() {
                    x += 1;
                }
                num_end = x;
            }
        }
        return t[..num_end].parse::<f64>().map(|f| f.trunc() as i64).unwrap_or(0);
    }
    t[..end].parse::<i64>().unwrap_or(if bytes[0] == b'-' { i64::MIN } else { i64::MAX })
}

/// `(int)` pada nilai JSON (dari kolom `content`).
pub fn intval(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n
            .as_i64()
            .unwrap_or_else(|| n.as_f64().map(|f| f.trunc() as i64).unwrap_or(0)),
        Value::String(s) => intval_str(s),
        Value::Bool(b) => i64::from(*b),
        Value::Null => 0,
        Value::Array(a) => i64::from(!a.is_empty()),
        Value::Object(o) => i64::from(!o.is_empty()),
    }
}

/// Truthiness PHP untuk string yang mungkin tidak ada: `null`, `""`, dan `"0"` itu falsy.
pub fn truthy(s: Option<&str>) -> bool {
    matches!(s, Some(v) if !v.is_empty() && v != "0")
}

/// `trim($s)` PHP dengan karakter default (` \t\n\r\0\x0B`).
pub fn trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0B'])
}

/// `trim($s, $chars)` untuk daftar karakter ASCII.
pub fn trim_chars<'a>(s: &'a str, chars: &[char]) -> &'a str {
    s.trim_matches(|c| chars.contains(&c))
}

/// `rawurlencode()`: semua byte kecuali `A-Za-z0-9-_.~` diberi `%XX` (huruf besar).
pub fn rawurlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `stripos($hay, $needle) !== false` untuk ASCII. Needle kosong selalu cocok (PHP 8).
pub fn stripos_found(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// Akses `$arr[$key] ?? null` pada nilai JSON. Null dianggap tidak ada (sama dengan `??`).
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let found = match v {
        Value::Object(m) => m.get(key),
        Value::Array(a) => key.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    };
    found.filter(|x| !x.is_null())
}

/// Akses `$arr[$i]` dengan kunci integer (objek dengan kunci "1" atau array berindeks 0).
pub fn get_int(v: &Value, i: i64) -> Option<&Value> {
    match v {
        Value::Object(m) => m.get(&i.to_string()),
        Value::Array(a) => usize::try_from(i).ok().and_then(|i| a.get(i)),
        _ => None,
    }
    .filter(|x| !x.is_null())
}

/// Nilai `$_GET`-like: kunci skalar (nilai terakhir menang) dan kunci `kunci[]` (array).
#[derive(Debug, Default, Clone)]
pub struct Params {
    pairs: Vec<(String, String)>,
}

impl Params {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut pairs = Vec::new();
        if let Some(raw) = raw {
            for part in raw.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = part.split_once('=').unwrap_or((part, ""));
                pairs.push((url_decode(k), url_decode(v)));
            }
        }
        Self { pairs }
    }

    /// Nilai skalar terakhir untuk `key`. `None` bila tidak ada.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Nilai `key[]` dalam urutan kemunculan. Kosong bila tidak ada.
    pub fn array(&self, key: &str) -> Vec<String> {
        let k = format!("{key}[]");
        self.pairs
            .iter()
            .filter(|(name, _)| *name == k)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

/// Percent-decoding `application/x-www-form-urlencoded` (`+` menjadi spasi).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_matches_php_half_away_from_zero() {
        assert_eq!(round(2.5, 0), 3.0);
        assert_eq!(round(-2.5, 0), -3.0);
        assert_eq!(round(1.955, 2), 1.96);
        assert_eq!(round(1.005, 2), 1.01);
        assert_eq!(round(0.0, 2), 0.0);
        assert_eq!(round(12.34, 1), 12.3);
    }

    #[test]
    fn intval_follows_php_string_rules() {
        assert_eq!(intval_str("2026"), 2026);
        assert_eq!(intval_str("  -7abc"), -7);
        assert_eq!(intval_str("abc"), 0);
        assert_eq!(intval_str("1.9"), 1);
        assert_eq!(intval_str("1e3"), 1000);
        assert_eq!(intval(&json!(true)), 1);
        assert_eq!(intval(&json!("12.7")), 12);
        assert_eq!(intval(&json!([])), 0);
        assert_eq!(intval(&json!([1])), 1);
    }

    #[test]
    fn truthy_treats_zero_string_as_false() {
        assert!(!truthy(None));
        assert!(!truthy(Some("")));
        assert!(!truthy(Some("0")));
        assert!(truthy(Some("00")));
        assert!(truthy(Some("a")));
    }

    #[test]
    fn rawurlencode_keeps_unreserved_only() {
        assert_eq!(rawurlencode("a b/c~d-e_f.g"), "a%20b%2Fc~d-e_f.g");
        assert_eq!(rawurlencode("é"), "%C3%A9");
    }

    #[test]
    fn stripos_empty_needle_matches() {
        assert!(stripos_found("PT Maju", "maj"));
        assert!(stripos_found("abc", ""));
        assert!(!stripos_found("abc", "z"));
    }

    #[test]
    fn query_last_scalar_wins_and_arrays_collect() {
        let p = Params::parse(Some("tahun=2025&tahun=2026&kecamatan_ids[]=1&kecamatan_ids[]=2&q=a+b%21"));
        assert_eq!(p.get("tahun"), Some("2026"));
        assert_eq!(p.array("kecamatan_ids"), vec!["1", "2"]);
        assert_eq!(p.get("q"), Some("a b!"));
        assert!(!p.has("nope"));
    }

    #[test]
    fn json_access_treats_null_as_missing() {
        let v = json!({"a": null, "b": 3});
        assert!(get(&v, "a").is_none());
        assert_eq!(get(&v, "b"), Some(&json!(3)));
        let arr = json!([10, 20]);
        assert_eq!(get_int(&arr, 1), Some(&json!(20)));
        assert!(get_int(&arr, 2).is_none());
    }
}
