//! Analisa RAB: membaca rincian RAB (sumur bor, MCK) dari file Excel atau teks tempel.
//!
//! Format rincian: header berisi kolom uraian, satuan, volume, harga satuan, dan jumlah. Setiap bagian
//! romawi (I, II, ...) diakhiri baris SUB TOTAL atau JUMLAH. Bagian dengan subtotal 0 adalah alternatif
//! yang tidak masuk rekap, sehingga dilewati. Hasil dipakai hanya bila setiap bagian punya subtotal dan
//! jumlah item cocok (`lengkap`).

use std::io::Cursor;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use serde_json::{json, Value};
use shared::ApiError;

use crate::{require_auth, AppState};

/// Batas unggahan file RAB.
pub const BODY_LIMIT: usize = 20 * 1024 * 1024;
/// Tarif PPN yang dipakai untuk analisa.
const PPN_RATE: f64 = 0.11;
/// Nama sheet rincian pada file RAB.
const SHEET_RAB: &str = "rab";

#[derive(Debug, Clone, PartialEq)]
pub enum Sel {
    Kosong,
    Angka(f64),
    Teks(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub grup: String,
    pub uraian: String,
    pub satuan: String,
    pub volume: f64,
    pub harga_satuan: f64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Rincian {
    pub items: Vec<Item>,
    pub warnings: Vec<String>,
    pub lengkap: bool,
}

struct Kolom {
    no: usize,
    uraian: usize,
    sat: usize,
    volume: usize,
    harga: usize,
    jumlah: usize,
    header: usize,
}

fn teks(sel: &Sel) -> String {
    match sel {
        Sel::Kosong => String::new(),
        Sel::Angka(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Sel::Angka(f) => format!("{f}"),
        Sel::Teks(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// Angka dari sel. Teks yang mengandung digit dibaca dengan format angka Indonesia.
fn angka(sel: Option<&Sel>) -> Option<f64> {
    match sel? {
        Sel::Angka(f) => Some(*f),
        Sel::Teks(s) => parse_angka_teks(s),
        Sel::Kosong => None,
    }
}

/// Membaca angka format Indonesia: "1.175.950", "1.175.950,5", "Rp 300.000", "0,5".
pub fn parse_angka_teks(s: &str) -> Option<f64> {
    let mut t = s.to_lowercase().replace("rp", "").replace(' ', "");
    if !t.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let ada_koma = t.contains(',');
    let ada_titik = t.contains('.');
    if ada_koma && ada_titik {
        if t.rfind(',') > t.rfind('.') {
            t = t.replace('.', "").replace(',', ".");
        } else {
            t = t.replace(',', "");
        }
    } else if ada_koma {
        t = t.replace(',', ".");
    } else if ada_titik {
        let bagian: Vec<&str> = t.split('.').collect();
        let ribuan = bagian.len() > 2 || (bagian.len() == 2 && bagian[1].len() == 3);
        if ribuan {
            t = t.replace('.', "");
        }
    }
    t.parse::<f64>().ok().filter(|f| f.is_finite())
}

/// Angka romawi untuk 1..=39.
fn romawi(mut n: u32) -> String {
    const NILAI: [(u32, &str); 13] = [
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
        (0, ""), (0, ""), (0, ""), (0, ""), (0, ""), (0, ""),
    ];
    let mut hasil = String::new();
    for (nilai, lambang) in NILAI.iter() {
        while *nilai > 0 && n >= *nilai {
            hasil.push_str(lambang);
            n -= nilai;
        }
    }
    hasil
}

/// Penanda bagian: angka romawi I sampai XXXIX. Huruf A, B, C adalah sub-bagian.
fn penanda_bagian(s: &str) -> bool {
    !s.is_empty() && (1..=39).any(|n| romawi(n) == s)
}

fn cari_kolom(rows: &[Vec<Sel>]) -> Option<Kolom> {
    let mut hasil = None;
    for (h, row) in rows.iter().enumerate() {
        let cells: Vec<String> = row.iter().map(|c| teks(c).to_uppercase()).collect();
        let cari = |nama: &[&str]| cells.iter().position(|c| nama.contains(&c.as_str()));
        let uraian = cari(&["URAIAN PEKERJAAN", "ITEM PEKERJAAN"]);
        let sat = cari(&["SAT", "SATUAN"]);
        let volume = cari(&["VOLUME"]);
        let harga = cari(&["HARGA", "HARGA SATUAN"]);
        let jumlah = cells.iter().position(|c| c == "JUMLAH" || c.starts_with("JUMLAH HARGA"));
        if let (Some(uraian), Some(sat), Some(volume), Some(harga), Some(jumlah)) =
            (uraian, sat, volume, harga, jumlah)
        {
            // Header NO bisa muncul dua kali (nomor urut dan penanda bagian); yang dipakai terakhir.
            let no = cells.iter().rposition(|c| c == "NO").unwrap_or(0);
            hasil = Some(Kolom { no, uraian, sat, volume, harga, jumlah, header: h });
        }
    }
    hasil
}

/// Membaca rincian dari grid sel (baris, kolom mulai dari 0).
pub fn baca_rincian(rows: &[Vec<Sel>]) -> Rincian {
    let mut hasil = Rincian { lengkap: true, ..Default::default() };
    let Some(kolom) = cari_kolom(rows) else {
        hasil.lengkap = false;
        return hasil;
    };

    let mut bagian: Option<(String, Vec<Item>)> = None;
    let sel = |row: &Vec<Sel>, i: usize| row.get(i).cloned().unwrap_or(Sel::Kosong);

    for row in rows.iter().skip(kolom.header + 1) {
        let no = teks(&sel(row, kolom.no));
        let uraian_c = teks(&sel(row, kolom.uraian));
        let atas = uraian_c.to_uppercase();

        if atas.starts_with("SUB TOTAL") || atas == "JUMLAH" {
            if let Some((judul, items)) = bagian.take() {
                tutup_bagian(&mut hasil, judul, items, angka(row.get(kolom.jumlah)));
            }
            continue;
        }

        if !no.is_empty() && penanda_bagian(&no) && !uraian_c.is_empty() {
            if let Some((judul, items)) = bagian.take() {
                tutup_bagian(&mut hasil, judul, items, None);
            }
            bagian = Some((format!("{no}. {uraian_c}"), Vec::new()));
            continue;
        }

        let Some((judul, items)) = bagian.as_mut() else { continue };
        let satuan = teks(&sel(row, kolom.sat));
        let volume = angka(row.get(kolom.volume));
        let harga = angka(row.get(kolom.harga));
        let (Some(volume), Some(harga)) = (volume, harga) else { continue };
        // Volume 0 tidak ikut dalam rincian: jumlahnya 0 dan tidak perlu tampil atau diunduh.
        if volume == 0.0 {
            continue;
        }
        if satuan.is_empty() {
            continue;
        }
        let uraian = uraian_dari_baris(row, &kolom, uraian_c);
        if uraian.is_empty() {
            continue;
        }
        items.push(Item { grup: judul.clone(), uraian, satuan, volume, harga_satuan: harga });
    }
    if let Some((judul, items)) = bagian.take() {
        tutup_bagian(&mut hasil, judul, items, None);
    }
    hasil
}

/// Nama item dari baris: sel teks pertama di antara kolom uraian dan satuan yang bukan angka.
/// Nomor urut bisa berpindah kolom (mis. blok APD: nomor di kolom D, nama di kolom E), jadi angka dilewati.
fn uraian_dari_baris(row: &[Sel], kolom: &Kolom, uraian_c: String) -> String {
    let mut kandidat: Vec<String> = (kolom.uraian + 1..kolom.sat)
        .map(|i| teks(&row.get(i).cloned().unwrap_or(Sel::Kosong)))
        .collect();
    kandidat.push(uraian_c);
    let bukan_angka = |t: &String| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',');
    kandidat
        .iter()
        .find(|t| bukan_angka(t))
        .or_else(|| kandidat.iter().find(|t| !t.is_empty()))
        .cloned()
        .unwrap_or_default()
}

fn tutup_bagian(hasil: &mut Rincian, judul: String, items: Vec<Item>, subtotal: Option<f64>) {
    let jumlah_item: f64 = items.iter().map(|i| i.volume * i.harga_satuan).sum();
    match subtotal {
        None => {
            if !items.is_empty() {
                hasil.lengkap = false;
                hasil.warnings.push(format!(
                    "Bagian \"{judul}\" tidak punya SUB TOTAL, jumlahnya tidak bisa dicocokkan."
                ));
                hasil.items.extend(items);
            }
        }
        Some(sub) if sub > 0.0 => {
            if (jumlah_item - sub).abs() > 1.0 {
                hasil.lengkap = false;
                hasil.warnings.push(format!(
                    "Bagian \"{judul}\": jumlah item {} tidak sama dengan subtotal {}.",
                    jumlah_item.round(),
                    sub.round()
                ));
            }
            hasil.items.extend(items);
        }
        Some(_) => {
            if !items.is_empty() {
                hasil.warnings.push(format!(
                    "Bagian \"{judul}\" dilewati karena subtotalnya 0 (tidak masuk rekap)."
                ));
            }
        }
    }
}

/// Grid sel dari sheet Excel. Kolom sebelum kolom awal sheet diisi kosong, supaya huruf kolom tetap sama.
fn grid_dari_excel(data: calamine::Range<Data>) -> Vec<Vec<Sel>> {
    let start_col = data.start().map(|(_, c)| c as usize).unwrap_or(0);
    data.rows()
        .map(|cells| {
            let mut row = vec![Sel::Kosong; start_col];
            row.extend(cells.iter().map(|c| match c {
                Data::Float(f) => Sel::Angka(*f),
                Data::Int(i) => Sel::Angka(*i as f64),
                Data::String(s) => Sel::Teks(s.clone()),
                Data::Bool(b) => Sel::Teks(b.to_string().to_uppercase()),
                Data::Empty | Data::Error(_) => Sel::Kosong,
                other => Sel::Teks(other.to_string()),
            }));
            row
        })
        .collect()
}

/// Grid sel dari teks tempel (dipisah tab per kolom dan baris baru per baris).
pub fn grid_dari_teks(teks_tempel: &str) -> Vec<Vec<Sel>> {
    teks_tempel
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            line.split('\t')
                .map(|c| {
                    let t = c.trim();
                    if t.is_empty() {
                        Sel::Kosong
                    } else {
                        let bersih = t.to_lowercase().replace("rp", "").replace(' ', "");
                        match parse_angka_teks(t) {
                            Some(f) if bersih.chars().all(|ch| ch.is_ascii_digit() || ".,-".contains(ch)) => Sel::Angka(f),
                            _ => Sel::Teks(t.to_string()),
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// Membaca sheet RAB dari berkas Excel.
fn grid_dari_berkas(bytes: Vec<u8>) -> Result<Vec<Vec<Sel>>, String> {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let nama = wb
        .sheet_names()
        .into_iter()
        .find(|n| n.trim().eq_ignore_ascii_case(SHEET_RAB))
        .ok_or("Sheet RAB tidak ditemukan di berkas ini.")?;
    let range = wb.worksheet_range(&nama).map_err(|e| e.to_string())?;
    Ok(grid_dari_excel(range))
}

/// Total pada rekap di atas rincian: baris `JUMLAH` atau `SUB JUMLAH`, nilainya di kolom jumlah.
/// `None` bila file tidak punya rekap seperti itu (mis. teks tempel).
pub fn jumlah_rekap(rows: &[Vec<Sel>]) -> Option<f64> {
    let kolom = cari_kolom(rows)?;
    let sel = |row: &Vec<Sel>, i: usize| row.get(i).cloned().unwrap_or(Sel::Kosong);
    rows.iter().take(kolom.header).find_map(|row| {
        let uraian = teks(&sel(row, kolom.uraian)).to_uppercase();
        if uraian == "JUMLAH" || uraian == "SUB JUMLAH" {
            angka(row.get(kolom.jumlah))
        } else {
            None
        }
    })
}

fn analisa(rincian: &Rincian) -> Value {
    let mut dpp = 0.0;
    let mut ppn_total = 0.0;
    let items: Vec<Value> = rincian
        .items
        .iter()
        .map(|i| {
            let jumlah = i.volume * i.harga_satuan;
            let ppn = jumlah * PPN_RATE;
            dpp += jumlah;
            ppn_total += ppn;
            json!({
                "grup": i.grup,
                "uraian": i.uraian,
                "satuan": i.satuan,
                "volume": i.volume,
                "hargaSatuan": i.harga_satuan,
                "jumlah": jumlah,
                "ppn": ppn,
                "total": jumlah + ppn,
            })
        })
        .collect();
    json!({
        "items": items,
        "summary": {
            "subtotalDpp": dpp,
            "totalPpn": ppn_total,
            "grandTotal": dpp + ppn_total,
            "itemCount": rincian.items.len(),
        },
        "warnings": rincian.warnings,
        "lengkap": rincian.lengkap,
    })
}

/// `POST /api/rab/analisa`: multipart dengan field `file` (berkas .xlsx) atau `teks` (teks tempel).
pub async fn analisa_rab(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;

    let mut file: Option<Vec<u8>> = None;
    let mut teks_tempel: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
    {
        match field.name() {
            Some("file") => {
                file = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?
                        .to_vec(),
                );
            }
            Some("teks") => {
                teks_tempel = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?,
                );
            }
            _ => {}
        }
    }

    let grid = match (file, teks_tempel) {
        (Some(bytes), _) => grid_dari_berkas(bytes)
            .map_err(|e| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, e))?,
        (None, Some(t)) if !t.trim().is_empty() => grid_dari_teks(&t),
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "Unggah berkas .xlsx atau tempel data RAB.",
            ))
        }
    };

    let rincian = baca_rincian(&grid);
    if rincian.items.is_empty() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Tidak ada item RAB yang terbaca. Pastikan formatnya rincian RAB (sumur bor atau MCK).",
        ));
    }
    let mut hasil = analisa(&rincian);
    let dpp = hasil["summary"]["subtotalDpp"].as_f64().unwrap_or(0.0);
    hasil["rekap"] = match jumlah_rekap(&grid) {
        Some(jumlah) => json!({ "jumlah": jumlah, "selisih": dpp - jumlah }),
        None => Value::Null,
    };
    Ok(Json(hasil))
}

/// Pembatas ukuran body untuk rute analisa RAB.
pub fn body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(BODY_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> Sel {
        Sel::Teks(t.to_string())
    }
    fn n(f: f64) -> Sel {
        Sel::Angka(f)
    }

    fn baris(sel: &[(usize, Sel)]) -> Vec<Sel> {
        let maks = sel.iter().map(|(i, _)| *i).max().unwrap_or(0);
        let mut row = vec![Sel::Kosong; maks + 1];
        for (i, v) in sel {
            row[*i] = v.clone();
        }
        row
    }

    fn contoh() -> Vec<Vec<Sel>> {
        vec![
            baris(&[(1, s("NO")), (2, s("URAIAN PEKERJAAN")), (7, s("SAT")), (8, s("VOLUME")), (9, s("HARGA")), (10, s("JUMLAH"))]),
            baris(&[(1, s("I")), (2, s("PEKERJAAN PERSIAPAN"))]),
            baris(&[(1, s("1")), (2, s("Pembersihan lokasi")), (7, s("Ls")), (8, n(1.0)), (9, n(300000.0)), (10, n(300000.0))]),
            baris(&[(2, s("a.")), (3, s("Foto Kegiatan")), (7, s("set")), (8, n(3.0)), (9, n(150000.0))]),
            baris(&[(2, s("SUB TOTAL  I")), (10, n(750000.0))]),
            baris(&[(1, s("V")), (2, s("DUDUKAN TORN"))]),
            baris(&[(1, s("1")), (2, s("Kran")), (7, s("unit")), (8, n(2.0)), (9, n(500000.0))]),
            baris(&[(2, s("SUB TOTAL  V")), (10, n(0.0))]),
        ]
    }

    #[test]
    fn romawi_dan_penanda() {
        assert_eq!(romawi(4), "IV");
        assert_eq!(romawi(14), "XIV");
        assert!(penanda_bagian("XIV"));
        assert!(!penanda_bagian("C"));
        assert!(!penanda_bagian("a."));
    }

    #[test]
    fn angka_format_indonesia() {
        assert_eq!(parse_angka_teks("1.175.950"), Some(1_175_950.0));
        assert_eq!(parse_angka_teks("1.175.950,5"), Some(1_175_950.5));
        assert_eq!(parse_angka_teks("Rp 300.000"), Some(300_000.0));
        assert_eq!(parse_angka_teks("0,5"), Some(0.5));
        assert_eq!(parse_angka_teks("Pekerjaan"), None);
    }

    #[test]
    fn rincian_dengan_subtotal_cocok() {
        let hasil = baca_rincian(&contoh());
        assert!(hasil.lengkap);
        assert_eq!(hasil.items.len(), 2);
        assert_eq!(hasil.items[0].grup, "I. PEKERJAAN PERSIAPAN");
        assert_eq!(hasil.items[1].uraian, "Foto Kegiatan");
        assert!(hasil.warnings.iter().any(|w| w.contains("DUDUKAN TORN")));
    }

    #[test]
    fn bagian_tanpa_subtotal_membuat_tidak_lengkap() {
        let mut grid = contoh();
        grid.retain(|r| !teks(&r[2]).starts_with("SUB TOTAL I"));
        let hasil = baca_rincian(&grid);
        assert!(!hasil.lengkap);
    }

    #[test]
    fn jumlah_rekap_diambil_dari_baris_jumlah_sebelum_rincian() {
        let mut grid = vec![baris(&[(1, s("No")), (2, s("Item Pekerjaan")), (7, s("Satuan")), (8, s("Volume")), (9, s("Harga Satuan")), (10, s("Jumlah Harga Satuan"))])];
        grid.push(baris(&[(2, s("Pekerjaan Persiapan")), (7, s("paket")), (8, n(1.0)), (9, n(300.0)), (10, n(300.0))]));
        grid.push(baris(&[(2, s("Jumlah")), (10, n(300.0))]));
        grid.extend(contoh());
        assert_eq!(jumlah_rekap(&grid), Some(300.0));
    }

    #[test]
    fn uraian_angka_pindah_ke_kolom_teks() {
        let teks = "NO\tURAIAN PEKERJAAN\t\t\tSATUAN\tVOLUME\tHARGA\tJUMLAH\nI\tPEKERJAAN PERSIAPAN\n\t\t1\tPembatas Area\tBh\t2\t10000\t20000\n1\tTopi\t\t\tBh\t2\t10000\t20000";
        let rincian = baca_rincian(&grid_dari_teks(teks));
        let uraian: Vec<&str> = rincian.items.iter().map(|i| i.uraian.as_str()).collect();
        assert_eq!(uraian, vec!["Pembatas Area", "Topi"]);
    }

    #[test]
    fn volume_nol_tidak_masuk_rincian() {
        let teks = "NO\tURAIAN PEKERJAAN\t\t\tSATUAN\tVOLUME\tHARGA\tJUMLAH\nI\tPEKERJAAN PERSIAPAN\n1\tPoster\t\t\tLbr\t0\t14000\t0\n2\tTopi\t\t\tBh\t2\t10000\t20000";
        let rincian = baca_rincian(&grid_dari_teks(teks));
        let uraian: Vec<&str> = rincian.items.iter().map(|i| i.uraian.as_str()).collect();
        assert_eq!(uraian, vec!["Topi"]);
    }

    #[test]
    fn teks_tempel_dibaca_sebagai_grid() {
        let grid = grid_dari_teks("NO\tURAIAN PEKERJAAN\t\t\t\t\tSAT\tVOLUME\tHARGA\tJUMLAH\nI\tPEKERJAAN PERSIAPAN\n1\tPembersihan lokasi\t\t\t\t\tLs\t1\t300.000\t300.000");
        let hasil = baca_rincian(&grid);
        assert_eq!(hasil.items.len(), 1);
        assert_eq!(hasil.items[0].harga_satuan, 300_000.0);
    }
}
