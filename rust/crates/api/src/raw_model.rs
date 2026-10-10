//! Pembentuk JSON dari baris mentah MySQL dengan `$casts` model Laravel.
//!
//! Dipakai oleh `spam_units`, `spam_integration`, dan `desa_profile` (`spm_sanitasi` dan `unit_spam`).
//!
//! Pertimbangan: tipe kolom MySQL dibaca dari metadata baris, lalu di-cast sesuai `Cast`.

use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, Column, Row, TypeInfo, ValueRef};

use crate::{format::number_like_php, lookup::carbon_json};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Cast dari `$casts` model, hanya untuk tipe yang dipakai di profil.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Cast {
    Float,
    Int,
    Bool,
}

/// Nilai satu kolom sebagai JSON. Cast model lebih dulu, lalu tipe kolom MySQL.
fn column_json(row: &MySqlRow, i: usize, ty: &str, cast: Option<Cast>) -> Result<Value, ApiError> {
    if row.try_get_raw(i).map_err(internal)?.is_null() {
        return Ok(Value::Null);
    }
    let ty = ty.to_uppercase();
    match cast {
        Some(Cast::Bool) => {
            let v: i64 = row.try_get(i).map_err(internal)?;
            return Ok(json!(v != 0));
        }
        Some(Cast::Int) => {
            // Kolom id MySQL sering BIGINT UNSIGNED: sqlx tidak mengizinkan i64 untuk kolom itu.
            if let Ok(v) = row.try_get::<i64, _>(i) {
                return Ok(json!(v));
            }
            let v: u64 = row.try_get(i).map_err(internal)?;
            return Ok(json!(v));
        }
        Some(Cast::Float) => {
            let v = float_of(row, i, &ty)?;
            return Ok(number_like_php(v));
        }
        None => {}
    }
    if ty.contains("DECIMAL") {
        // Decimal tanpa cast dikirim Laravel sebagai string.
        // Teks desimal dibaca langsung: sqlx tidak mengizinkan DECIMAL sebagai String lewat jalur aman.
        let text: String = row.try_get_unchecked(i).map_err(internal)?;
        return Ok(json!(text));
    }
    if ty.contains("INT") || ty.contains("YEAR") {
        if let Ok(v) = row.try_get::<i64, _>(i) {
            return Ok(json!(v));
        }
        let v: u64 = row.try_get(i).map_err(internal)?;
        return Ok(json!(v));
    }
    if ty.contains("DOUBLE") || ty.contains("FLOAT") {
        return Ok(number_like_php(float_of(row, i, &ty)?));
    }
    if ty.contains("TIMESTAMP") {
        // Kolom TIMESTAMP MySQL hanya bisa dibaca sebagai DateTime<Utc> oleh sqlx.
        let v: DateTime<Utc> = row.try_get(i).map_err(internal)?;
        return Ok(carbon_json(Some(v)));
    }
    if ty.contains("DATETIME") {
        let v: NaiveDateTime = row.try_get(i).map_err(internal)?;
        return Ok(carbon_json(Some(v.and_utc())));
    }
    if ty == "DATE" {
        let v: NaiveDate = row.try_get(i).map_err(internal)?;
        return Ok(json!(v.format("%Y-%m-%d").to_string()));
    }
    if ty.contains("JSON") {
        let v: String = row.try_get(i).map_err(internal)?;
        return Ok(serde_json::from_str(&v).unwrap_or(json!(v)));
    }
    let v: String = row.try_get(i).map_err(internal)?;
    Ok(json!(v))
}

/// Angka pecahan dari DECIMAL (teks) atau DOUBLE/FLOAT (biner).
fn float_of(row: &MySqlRow, i: usize, ty: &str) -> Result<f64, ApiError> {
    if ty.contains("DECIMAL") {
        let text: String = row.try_get_unchecked(i).map_err(internal)?;
        return text.trim().parse::<f64>().map_err(internal);
    }
    row.try_get::<f64, _>(i).map_err(internal)
}

/// Model mentah (`toArray()` tanpa relasi): semua kolom, nilai dibentuk dari tipe dan cast.
pub(crate) fn raw_model(row: &MySqlRow, casts: &[(&str, Cast)]) -> Result<Value, ApiError> {
    let mut map = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        let name = col.name();
        let cast = casts.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
        map.insert(
            name.to_string(),
            column_json(row, i, col.type_info().name(), cast)?,
        );
    }
    Ok(Value::Object(map))
}
