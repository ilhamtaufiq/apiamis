//! Validasi input override Ringkasan Kontrak (`KontrakController::validatedRingkasanOverrides`).
//!
//! Aturan sama dengan Laravel: `persen_tagih` integer 100/95/5/30, `pembayaran_lalu` array maksimal 5 item
//! (`jenis` string maks 100, `tanggal` tanggal, `nominal` numerik >= 0), serta nomor dan tanggal jaminan
//! uang muka dan pelaksanaan. Item pembayaran yang seluruh isinya kosong dibuang, nilai kosong dilewati.
//! Hasilnya berupa `Overrides` untuk `kontrak_document_data::build`.
//!
//! Catatan: di Laravel `ValidationException` tertangkap `catch (\Exception)` dan menjadi 500. Di sini
//! input tidak valid menjadi 422 dengan daftar error per field.

use std::collections::BTreeMap;

use serde_json::Value;
use shared::ApiError;

use crate::kontrak_document_data::Overrides;

type Errors = BTreeMap<String, Vec<String>>;

fn push(errors: &mut Errors, field: &str, message: String) {
    errors.entry(field.to_string()).or_default().push(message);
}

fn label(field: &str) -> String {
    field.replace('_', " ")
}

/// Nilai kosong (null atau string kosong/spasi) diperlakukan sebagai tidak diisi (`ConvertEmptyStringsToNull`).
fn present(v: Option<&Value>) -> Option<&Value> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        other => other,
    }
}

fn is_date(raw: &str) -> bool {
    let t = raw.trim();
    let head = t.get(..10).unwrap_or(t);
    ["%Y-%m-%d", "%d-%m-%Y", "%d/%m/%Y", "%Y/%m/%d"]
        .iter()
        .any(|f| chrono::NaiveDate::parse_from_str(head, f).is_ok())
}

fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            if !t.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            t.parse::<f64>().ok().filter(|n| n.is_finite())
        }
        _ => None,
    }
}

fn string_field(
    errors: &mut Errors,
    field: &str,
    shown: &str,
    v: Option<&Value>,
    max: usize,
    date: bool,
) -> Option<String> {
    let v = present(v)?;
    let Value::String(s) = v else {
        push(errors, field, format!("The {shown} field must be a string."));
        return None;
    };
    if s.chars().count() > max {
        push(
            errors,
            field,
            format!("The {shown} field must not be greater than {max} characters."),
        );
        return None;
    }
    if date && !is_date(s) {
        push(errors, field, format!("The {shown} field must be a valid date."));
        return None;
    }
    Some(s.trim().to_string())
}

/// Validasi body JSON dan ubah menjadi `Overrides`. Gagal: 422 dengan daftar error per field.
pub fn validate(input: &Value) -> Result<Overrides, ApiError> {
    let mut errors = Errors::new();
    let mut flat: Vec<(String, String)> = Vec::new();
    let mut items: Vec<Vec<(String, String)>> = Vec::new();

    if let Some(v) = present(input.get("persen_tagih")) {
        let parsed = match v {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        };
        match parsed {
            None => push(
                &mut errors,
                "persen_tagih",
                "The persen tagih field must be an integer.".into(),
            ),
            Some(p) if ![100, 95, 5, 30].contains(&p) => push(
                &mut errors,
                "persen_tagih",
                "The selected persen tagih is invalid.".into(),
            ),
            Some(p) => flat.push(("persen_tagih".into(), p.to_string())),
        }
    }

    if let Some(v) = present(input.get("pembayaran_lalu")) {
        match v.as_array() {
            None => push(
                &mut errors,
                "pembayaran_lalu",
                "The pembayaran lalu field must be an array.".into(),
            ),
            Some(rows) => {
                if rows.len() > 5 {
                    push(
                        &mut errors,
                        "pembayaran_lalu",
                        "The pembayaran lalu field must not have more than 5 items.".into(),
                    );
                }
                for (i, row) in rows.iter().enumerate().take(5) {
                    let Some(obj) = row.as_object() else { continue };
                    let name = |f: &str| format!("pembayaran_lalu.{i}.{f}");
                    let shown = |f: &str| format!("pembayaran_lalu.{i}.{f}");
                    let jenis = string_field(
                        &mut errors,
                        &name("jenis"),
                        &shown("jenis"),
                        obj.get("jenis"),
                        100,
                        false,
                    );
                    let tanggal = string_field(
                        &mut errors,
                        &name("tanggal"),
                        &shown("tanggal"),
                        obj.get("tanggal"),
                        usize::MAX,
                        true,
                    );
                    let mut nominal: Option<String> = None;
                    if let Some(n) = present(obj.get("nominal")) {
                        let field = name("nominal");
                        match numeric(n) {
                            None => push(
                                &mut errors,
                                &field,
                                format!("The {} field must be a number.", shown("nominal")),
                            ),
                            Some(x) if x < 0.0 => push(
                                &mut errors,
                                &field,
                                format!("The {} field must be at least 0.", shown("nominal")),
                            ),
                            Some(_) => {
                                nominal = Some(match n {
                                    Value::String(s) => s.trim().to_string(),
                                    other => other.to_string(),
                                })
                            }
                        }
                    }
                    let jenis = jenis.unwrap_or_default();
                    if jenis.is_empty() && tanggal.is_none() && nominal.is_none() {
                        continue;
                    }
                    let mut fields = vec![("jenis".to_string(), jenis)];
                    if let Some(t) = tanggal {
                        fields.push(("tanggal".to_string(), t));
                    }
                    if let Some(n) = nominal {
                        fields.push(("nominal".to_string(), n));
                    }
                    items.push(fields);
                }
            }
        }
    }

    for (field, date) in [
        ("nomor_jaminan_uang_muka", false),
        ("tanggal_jaminan_uang_muka", true),
        ("nomor_jaminan_pelaksanaan", false),
        ("tanggal_jaminan_pelaksanaan", true),
    ] {
        if let Some(value) = string_field(
            &mut errors,
            field,
            &label(field),
            input.get(field),
            255,
            date,
        ) {
            flat.push((field.to_string(), value));
        }
    }

    if !errors.is_empty() {
        let total: usize = errors.values().map(Vec::len).sum();
        let first = errors
            .values()
            .next()
            .and_then(|m| m.first())
            .cloned()
            .unwrap_or_else(|| "The given data was invalid.".to_string());
        let message = match total - 1 {
            0 => first,
            1 => format!("{first} (and 1 more error)"),
            n => format!("{first} (and {n} more errors)"),
        };
        return Err(ApiError::validation(message, errors));
    }
    Ok(Overrides {
        flat,
        pembayaran_lalu: items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_input_gives_no_overrides() {
        let o = validate(&json!({})).unwrap();
        assert!(o.flat.is_empty() && o.pembayaran_lalu.is_empty());
        let o = validate(
            &json!({"persen_tagih": null, "pembayaran_lalu": [], "nomor_jaminan_uang_muka": "  "}),
        )
        .unwrap();
        assert!(o.flat.is_empty() && o.pembayaran_lalu.is_empty());
    }

    #[test]
    fn valid_input_is_normalized() {
        let o = validate(&json!({
            "persen_tagih": "95",
            "pembayaran_lalu": [
                {"jenis": " Uang Muka ", "tanggal": "2026-02-01", "nominal": 100000},
                {"jenis": "", "tanggal": null, "nominal": ""},
                {"jenis": "Termin 1", "nominal": "250000.5"}
            ],
            "nomor_jaminan_uang_muka": " UM-1 ",
            "tanggal_jaminan_pelaksanaan": "2026-03-05"
        }))
        .unwrap();
        assert!(o.flat.contains(&("persen_tagih".into(), "95".into())));
        assert!(o
            .flat
            .contains(&("nomor_jaminan_uang_muka".into(), "UM-1".into())));
        assert!(o
            .flat
            .contains(&("tanggal_jaminan_pelaksanaan".into(), "2026-03-05".into())));
        assert_eq!(o.pembayaran_lalu.len(), 2);
        assert_eq!(o.pembayaran_lalu[0][0], ("jenis".into(), "Uang Muka".into()));
        assert_eq!(o.pembayaran_lalu[0][2], ("nominal".into(), "100000".into()));
        assert_eq!(o.pembayaran_lalu[1].len(), 2);
    }

    #[test]
    fn rejects_invalid_values() {
        let err = validate(&json!({
            "persen_tagih": 50,
            "pembayaran_lalu": [{"tanggal": "bukan tanggal", "nominal": -5, "jenis": 7}],
            "tanggal_jaminan_uang_muka": "xx"
        }))
        .unwrap_err();
        let errors = err.errors.unwrap();
        assert!(errors.contains_key("persen_tagih"));
        assert!(errors.contains_key("pembayaran_lalu.0.tanggal"));
        assert!(errors.contains_key("pembayaran_lalu.0.nominal"));
        assert!(errors.contains_key("pembayaran_lalu.0.jenis"));
        assert!(errors.contains_key("tanggal_jaminan_uang_muka"));
    }

    #[test]
    fn rejects_more_than_five_payments_and_long_strings() {
        let rows: Vec<Value> = (0..6).map(|_| json!({"jenis": "x"})).collect();
        let err = validate(
            &json!({"pembayaran_lalu": rows, "nomor_jaminan_pelaksanaan": "a".repeat(256)}),
        )
        .unwrap_err();
        let errors = err.errors.unwrap();
        assert!(errors.contains_key("pembayaran_lalu"));
        assert!(errors.contains_key("nomor_jaminan_pelaksanaan"));
    }
}
