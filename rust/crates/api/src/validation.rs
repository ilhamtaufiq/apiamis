//! Validasi input seperti `Validator` Laravel: kumpulan error berurutan dan aturan sederhana.

use std::collections::BTreeMap;

use serde_json::Value;
use shared::ApiError;

/// Error validasi dalam urutan aturan. Pesan pertama dipakai sebagai `message`.
#[derive(Default)]
pub struct Errors(Vec<(String, String)>);

impl Errors {
    pub fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0.push((field.to_string(), message.into()));
    }

    /// Nilai hasil aturan; bila gagal, error dicatat dan `fallback` dikembalikan (tidak dipakai bila ada error).
    pub fn check<T>(&mut self, field: &str, rule: Result<T, String>, fallback: T) -> T {
        match rule {
            Ok(v) => v,
            Err(message) => {
                self.add(field, message);
                fallback
            }
        }
    }

    pub fn finish(self) -> Result<(), ApiError> {
        let Some((_, first)) = self.0.first().cloned() else {
            return Ok(());
        };
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (field, message) in self.0 {
            map.entry(field).or_default().push(message);
        }
        Err(ApiError::validation(first, map))
    }
}

/// Aturan `required|integer|min:N`. Kosong dihitung tidak ada.
pub fn int_rule(raw: Option<String>, attribute: &str, min: i64) -> Result<i64, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Err(format!("The {attribute} field is required.")),
        Some(s) => match s.parse::<i64>() {
            Err(_) => Err(format!("The {attribute} field must be an integer.")),
            Ok(v) if v < min => Err(format!("The {attribute} field must be at least {min}.")),
            Ok(v) => Ok(v),
        },
    }
}

/// Nilai JSON sebagai teks input (string atau angka). Lainnya dianggap tidak ada.
pub fn json_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
