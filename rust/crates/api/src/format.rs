//! Format nilai yang sama dengan serialisasi Laravel.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// `Carbon::toIso8601String()` untuk zona UTC: `2025-11-30T10:00:00+00:00`.
pub fn iso8601_utc(ts: Option<DateTime<Utc>>) -> Value {
    match ts {
        Some(t) => Value::String(t.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()),
        None => Value::Null,
    }
}

/// Angka utuh ditulis sebagai integer (seperti PHP), selain itu sebagai float.
pub fn number_like_php(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}
