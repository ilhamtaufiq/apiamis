//! Nomor WhatsApp: normalisasi ke format internasional tanpa tanda baca (mis. `6281234567890`).
//!
//! Input yang diterima: `081234567890`, `+62 812-3456-7890`, `62 812 3456 7890`, `(0812) 3456.7890`.
//! Hasil selalu diawali `62` dan berisi 10 sampai 15 digit.

/// Normalisasi nomor. `Err` berisi pesan yang bisa ditampilkan ke pengguna.
pub fn normalize(raw: &str) -> Result<String, String> {
    let bersih: String = raw
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')' | '+'))
        .collect();
    if bersih.is_empty() || !bersih.chars().all(|c| c.is_ascii_digit()) {
        return Err("Nomor WhatsApp hanya boleh berisi angka.".to_string());
    }
    let internasional = if let Some(sisa) = bersih.strip_prefix('0') {
        format!("62{sisa}")
    } else if bersih.starts_with("62") {
        bersih
    } else {
        return Err("Nomor WhatsApp harus diawali 0 atau 62.".to_string());
    };
    let panjang = internasional.len();
    if !(10..=15).contains(&panjang) {
        return Err("Nomor WhatsApp harus 10 sampai 15 digit.".to_string());
    }
    Ok(internasional)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menerima_berbagai_bentuk_penulisan() {
        assert_eq!(normalize("081234567890").unwrap(), "6281234567890");
        assert_eq!(normalize("+62 812-3456-7890").unwrap(), "6281234567890");
        assert_eq!(normalize("62 812 3456 7890").unwrap(), "6281234567890");
        assert_eq!(normalize("(0812) 3456.7890").unwrap(), "6281234567890");
    }

    #[test]
    fn menolak_nomor_tidak_valid() {
        assert!(normalize("").is_err());
        assert!(normalize("08123abc").is_err());
        assert!(normalize("1234567890").is_err());
        assert!(normalize("0812345").is_err());
        assert!(normalize("0812345678901234567").is_err());
    }
}
