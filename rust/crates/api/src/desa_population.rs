//! Port `php artisan desa:import-population` (`ImportDesaPopulation`).
//!
//! Membaca berkas Excel penduduk, lalu mengisi `tbl_desa.jumlah_penduduk`. Desa dicocokkan dengan
//! kecamatan + nama desa setelah dinormalisasi: huruf kecil, tanpa kata "kecamatan", "desa", dan
//! "kelurahan", dan tanpa karakter selain a-z0-9.
//!
//! Kolom yang dibaca (B = desa, C = kecamatan, D = jumlah penduduk). Baris 1 adalah judul dan
//! dilewati. Baris tanpa salah satu dari ketiga nilai itu dilewati.
//!
//! Berbeda dari Laravel:
//! - Laravel memakai sheet aktif. Rust memakai sheet pertama.
//! - Tidak ada audit dan notifikasi. Laravel juga melewatinya saat `runningInConsole()`.
//! - Laravel hanya menulis bila ada kolom yang berubah. Rust memakai klausa WHERE yang sama.
//!   `updated` tetap dihitung untuk setiap baris yang cocok, seperti Laravel.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use calamine::{open_workbook_auto_from_rs, Data, Reader};
use sqlx::{MySqlPool, Row};

/// Kolom dalam bentuk indeks 0 (absolut terhadap sheet): B, C, D.
const COL_DESA: usize = 1;
const COL_KECAMATAN: usize = 2;
const COL_PENDUDUK: usize = 3;

#[derive(Debug, Clone)]
pub struct PopRow {
    pub desa: String,
    pub kecamatan: String,
    pub jumlah_penduduk: i64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub rows: usize,
    pub matched: usize,
    pub updated: usize,
    pub unmatched: Vec<PopRow>,
    pub ambiguous: Vec<PopRow>,
}

/// Jalankan import. `dry_run` hanya mencocokkan tanpa menulis. Error berupa pesan teks.
pub async fn import(pool: &MySqlPool, path: &Path, dry_run: bool) -> Result<Report, String> {
    let rows = read_rows(path)?;
    let lookup = build_lookup(pool).await.map_err(|e| e.to_string())?;
    let mut rep = Report {
        rows: rows.len(),
        ..Default::default()
    };

    for row in rows {
        let key = format!("{}|{}", normalize(&row.kecamatan), normalize(&row.desa));
        let candidates: &[u64] = lookup.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        match candidates {
            [] => rep.unmatched.push(row),
            [id] => {
                rep.matched += 1;
                if !dry_run {
                    update_population(pool, *id, row.jumlah_penduduk)
                        .await
                        .map_err(|e| e.to_string())?;
                    rep.updated += 1;
                }
            }
            _ => rep.ambiguous.push(row),
        }
    }
    Ok(rep)
}

/// `buildDesaLookup()`: kunci `normalize(kecamatan)|normalize(desa)` ke daftar id desa.
async fn build_lookup(pool: &MySqlPool) -> Result<HashMap<String, Vec<u64>>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT d.id, d.n_desa, k.n_kec FROM tbl_desa d LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id",
    )
    .fetch_all(pool)
    .await?;

    let mut lookup: HashMap<String, Vec<u64>> = HashMap::new();
    for r in rows {
        let id: u64 = r.try_get("id")?;
        let desa: Option<String> = r.try_get("n_desa")?;
        let kec: Option<String> = r.try_get("n_kec")?;
        let key = format!(
            "{}|{}",
            normalize(kec.as_deref().unwrap_or("")),
            normalize(desa.as_deref().unwrap_or(""))
        );
        lookup.entry(key).or_default().push(id);
    }
    Ok(lookup)
}

async fn update_population(pool: &MySqlPool, id: u64, jumlah: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE tbl_desa SET jumlah_penduduk = ?, updated_at = NOW() \
         WHERE id = ? AND (jumlah_penduduk IS NULL OR jumlah_penduduk <> ?)",
    )
    .bind(jumlah)
    .bind(id)
    .bind(jumlah)
    .execute(pool)
    .await?;
    Ok(())
}

/// `normalize()` di Laravel. Nilai kosong atau `"0"` menjadi kosong, sama dengan `!$value` di PHP.
pub fn normalize(value: &str) -> String {
    if value.is_empty() || value == "0" {
        return String::new();
    }
    let mut s = value.to_string();
    for word in ["kecamatan", "desa", "kelurahan"] {
        s = remove_ci(&s, word);
    }
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Hapus semua kemunculan `word` tanpa membedakan huruf ASCII, seperti `str_ireplace`.
fn remove_ci(s: &str, word: &str) -> String {
    // Huruf ASCII diubah kecil tanpa mengubah panjang byte, jadi indeks hasilnya tetap valid di `s`.
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (idx, _) in lower.match_indices(word) {
        out.push_str(&s[last..idx]);
        last = idx + word.len();
    }
    out.push_str(&s[last..]);
    out
}

fn read_rows(path: &Path) -> Result<Vec<PopRow>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or("Berkas tidak memiliki sheet")?
        .map_err(|e| e.to_string())?;
    let (start_row, start_col) = range
        .start()
        .map(|(r, c)| (r as usize, c as usize))
        .unwrap_or((0, 0));

    let mut rows = Vec::new();
    for (i, cells) in range.rows().enumerate() {
        // Indeks baris absolut 0 adalah judul. Data mulai dari indeks 1 (baris 2 di Excel).
        if start_row + i < 1 {
            continue;
        }
        let desa = text(cell(cells, start_col, COL_DESA));
        let kecamatan = text(cell(cells, start_col, COL_KECAMATAN));
        let penduduk = int(cell(cells, start_col, COL_PENDUDUK));
        let (Some(desa), Some(kecamatan), Some(jumlah_penduduk)) = (desa, kecamatan, penduduk)
        else {
            continue;
        };
        rows.push(PopRow {
            desa,
            kecamatan,
            jumlah_penduduk,
        });
    }
    Ok(rows)
}

/// Sel pada kolom absolut `col`. Kolom sebelum `start_col` tidak ada di `cells`.
fn cell(cells: &[Data], start_col: usize, col: usize) -> Option<&Data> {
    cells.get(col.checked_sub(start_col)?)
}

/// `cleanString()`: rapikan spasi, dan kosong atau `"0"` dianggap tidak ada.
fn text(v: Option<&Data>) -> Option<String> {
    let s = match v? {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        _ => return None,
    };
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() || s == "0" {
        None
    } else {
        Some(s)
    }
}

/// `toInteger()`: angka dibulatkan. Teks yang bukan angka menjadi tidak ada.
fn int(v: Option<&Data>) -> Option<i64> {
    let f = match v? {
        Data::Int(i) => return Some(*i),
        Data::Float(f) => *f,
        Data::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if f.is_finite() {
        Some(f.round() as i64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalize_buang_kata_wilayah_dan_simbol() {
        assert_eq!(normalize("Kecamatan Cianjur"), "cianjur");
        assert_eq!(normalize("DESA Sukamaju"), "sukamaju");
        assert_eq!(normalize("Kelurahan Ci-Dang_Bayang"), "cidangbayang");
        assert_eq!(normalize("0"), "");
        assert_eq!(normalize(""), "");
    }
}
