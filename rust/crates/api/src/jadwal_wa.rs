//! Perintah "@arumanis buat jadwal" di WhatsApp: membuat event kalender dari pesan yang dibalas.
//! Format bebas. Judul diambil dari pesan yang dibalas. Tanggal, jam, dan tempat dibaca dari perintahnya
//! dengan pola yang paling umum. Kalau tanggal tidak terbaca, perintah ditolak dengan petunjuk.
//! Waktu disimpan sebagai jam dinding WIB, sama seperti `events.rs`.

use chrono::{Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;
use shared::ApiError;
use sqlx::MySqlPool;

use crate::media::internal;

const BULAN: [(&str, u32); 12] = [
    ("januari", 1),
    ("februari", 2),
    ("maret", 3),
    ("april", 4),
    ("mei", 5),
    ("juni", 6),
    ("juli", 7),
    ("agustus", 8),
    ("september", 9),
    ("oktober", 10),
    ("november", 11),
    ("desember", 12),
];

#[derive(Debug, PartialEq)]
pub(crate) struct Jadwal {
    pub judul: String,
    pub mulai: NaiveDateTime,
    pub selesai: NaiveDateTime,
    pub seharian: bool,
    pub lokasi: Option<String>,
    pub deskripsi: String,
}

/// Perintah "buat jadwal" dalam teks. Kata lain di depan atau belakangnya tidak masalah.
pub(crate) fn perintah_jadwal(teks: &str) -> bool {
    Regex::new(r"(?i)\bbuat\s+jadwal\b").map(|r| r.is_match(teks)).unwrap_or(false)
}

/// Hari ini menurut WIB.
pub(crate) fn hari_ini_wib() -> NaiveDate {
    let wib = FixedOffset::east_opt(7 * 3600).expect("offset WIB valid");
    Utc::now().with_timezone(&wib).date_naive()
}

/// Judul dari baris pertama pesan yang dibalas, dipangkas dan dibatasi 255 karakter.
fn judul_dari(kutipan: &str) -> Result<String, String> {
    let baris = kutipan.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let judul: String = baris.split_whitespace().collect::<Vec<_>>().join(" ");
    if judul.is_empty() {
        return Err("Pesan yang dibalas kosong, jadi tidak ada judul untuk jadwal.".to_string());
    }
    Ok(judul.chars().take(255).collect())
}

/// Mencari tanggal dalam teks. Mengembalikan tanggalnya dan teks tanpa bagian tanggal.
fn cari_tanggal(teks: &str, hari_ini: NaiveDate) -> Option<(NaiveDate, String)> {
    let pola = [
        r"(?i)\b(hari ini|besok|lusa)\b",
        r"\b(\d{4})-(\d{1,2})-(\d{1,2})\b",
        r"\b(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})\b",
        r"(?i)\b(\d{1,2})\s+(januari|februari|maret|april|mei|juni|juli|agustus|september|oktober|november|desember)(?:\s+(\d{4}))?\b",
    ];
    for (i, p) in pola.iter().enumerate() {
        let re = Regex::new(p).ok()?;
        let Some(c) = re.captures(teks) else { continue };
        let tanggal = match i {
            0 => match c[1].to_lowercase().as_str() {
                "hari ini" => hari_ini,
                "besok" => hari_ini + Duration::days(1),
                _ => hari_ini + Duration::days(2),
            },
            1 => NaiveDate::from_ymd_opt(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)?,
            2 => NaiveDate::from_ymd_opt(c[3].parse().ok()?, c[2].parse().ok()?, c[1].parse().ok()?)?,
            _ => {
                let bulan = BULAN.iter().find(|(n, _)| *n == c[2].to_lowercase())?.1;
                let tahun: i32 = match c.get(3) {
                    Some(t) => t.as_str().parse().ok()?,
                    None => hari_ini.year(),
                };
                NaiveDate::from_ymd_opt(tahun, bulan, c[1].parse().ok()?)?
            }
        };
        let sisa = format!("{} {}", &teks[..c.get(0)?.start()], &teks[c.get(0)?.end()..]);
        return Some((tanggal, sisa));
    }
    None
}

fn jam_menit(h: &str, m: Option<&str>) -> Option<NaiveTime> {
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.map(|x| x.parse().ok()).unwrap_or(Some(0))?;
    NaiveTime::from_hms_opt(h, m, 0)
}

/// Mencari jam mulai dan jam selesai. Jam tanpa kata "jam"/"pukul" hanya diterima bila memakai ":" atau ".".
fn cari_waktu(teks: &str) -> Option<(NaiveTime, Option<NaiveTime>)> {
    let ada_kata_jam = Regex::new(r"(?i)\b(jam|pukul)\b").ok()?.is_match(teks);
    let rentang = Regex::new(
        r"(?i)(\d{1,2})(?:[:.](\d{2}))?\s*(?:-|–|sampai|s/d|sd|hingga)\s*(\d{1,2})(?:[:.](\d{2}))?",
    )
    .ok()?;
    if let Some(c) = rentang.captures(teks) {
        if ada_kata_jam || c[0].contains(':') || c[0].contains('.') {
            let mulai = jam_menit(&c[1], c.get(2).map(|m| m.as_str()))?;
            let selesai = jam_menit(&c[3], c.get(4).map(|m| m.as_str()))?;
            return Some((mulai, Some(selesai)));
        }
    }
    let tunggal = Regex::new(r"(?i)(?:\b(?:jam|pukul)\s*(\d{1,2})(?:[:.](\d{2}))?\b|\b(\d{1,2})[:.](\d{2})\b)").ok()?;
    let c = tunggal.captures(teks)?;
    let mulai = if let Some(h) = c.get(1) {
        jam_menit(h.as_str(), c.get(2).map(|m| m.as_str()))?
    } else {
        jam_menit(&c[3], c.get(4).map(|m| m.as_str()))?
    };
    Some((mulai, None))
}

/// Tempat dari kata "tempat", "lokasi", atau "di". Dipotong sebelum kata jam.
fn cari_lokasi(teks: &str) -> Option<String> {
    let re = Regex::new(r"(?i)\b(?:tempat|lokasi|di)\s*:?\s+([^\n]+)").ok()?;
    let c = re.captures(teks)?;
    let mut lokasi = c[1].to_string();
    if let Ok(potong) = Regex::new(r"(?i)\s(?:jam|pukul)\b") {
        if let Some(m) = potong.find(&lokasi) {
            lokasi.truncate(m.start());
        }
    }
    let bersih = lokasi.trim().trim_end_matches(|ch: char| ",.;:".contains(ch)).trim().to_string();
    if bersih.is_empty() {
        None
    } else {
        Some(bersih.chars().take(255).collect())
    }
}

/// Membaca perintah dan pesan yang dibalas menjadi jadwal. Galat berupa teks untuk pengguna.
pub(crate) fn parse(perintah: &str, kutipan: Option<&str>, hari_ini: NaiveDate) -> Result<Jadwal, String> {
    let judul = judul_dari(kutipan.unwrap_or(""))?;
    let (tanggal, sisa) = cari_tanggal(perintah, hari_ini).ok_or_else(|| {
        "Tanggal tidak terbaca. Contoh: besok jam 9, 25/10/2026 09:00-11:00, atau 2026-10-25.".to_string()
    })?;
    let lokasi = cari_lokasi(&sisa);
    let deskripsi = kutipan.unwrap_or("").trim().chars().take(2000).collect::<String>();

    match cari_waktu(&sisa) {
        None => Ok(Jadwal {
            judul,
            mulai: tanggal.and_hms_opt(0, 0, 0).ok_or("Tanggal tidak valid.")?,
            selesai: tanggal.and_hms_opt(23, 59, 0).ok_or("Tanggal tidak valid.")?,
            seharian: true,
            lokasi,
            deskripsi,
        }),
        Some((mulai, selesai)) => {
            let mulai_dt = tanggal.and_time(mulai);
            let selesai_dt = match selesai {
                Some(s) => tanggal.and_time(s),
                None => mulai_dt + Duration::hours(1),
            };
            if selesai_dt <= mulai_dt {
                return Err("Jam selesai harus setelah jam mulai.".to_string());
            }
            Ok(Jadwal {
                judul,
                mulai: mulai_dt,
                selesai: selesai_dt,
                seharian: false,
                lokasi,
                deskripsi,
            })
        }
    }
}

/// Menyimpan jadwal ke `tbl_events`, sama seperti event yang dibuat dari kalender.
pub(crate) async fn simpan(pool: &MySqlPool, user_id: u64, j: &Jadwal) -> Result<u64, ApiError> {
    let hasil = sqlx::query(
        "INSERT INTO tbl_events (user_id, title, is_allday, start, end, category, location, description, \
         color, bg_color, border_color, attachments, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, 'event', ?, ?, NULL, NULL, NULL, NULL, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(&j.judul)
    .bind(j.seharian)
    .bind(j.mulai)
    .bind(j.selesai)
    .bind(&j.lokasi)
    .bind(&j.deskripsi)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(hasil.last_insert_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hari(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn besok_jam_dan_tempat() {
        let j = parse("@arumanis buat jadwal besok jam 9 di Aula Dinas", Some("Rapat koordinasi PPK\nbawa laporan"), hari(2026, 10, 18)).unwrap();
        assert_eq!(j.judul, "Rapat koordinasi PPK");
        assert_eq!(j.mulai, hari(2026, 10, 19).and_hms_opt(9, 0, 0).unwrap());
        assert_eq!(j.selesai, hari(2026, 10, 19).and_hms_opt(10, 0, 0).unwrap());
        assert_eq!(j.lokasi.as_deref(), Some("Aula Dinas"));
        assert!(!j.seharian);
    }

    #[test]
    fn tanggal_format_slash_dan_rentang_jam_dengan_tempat() {
        let j = parse("buat jadwal 25/10/2026 09:00-11:00 tempat: Kantor Dinas.", Some("Monitoring SPAM"), hari(2026, 10, 18)).unwrap();
        assert_eq!(j.mulai, hari(2026, 10, 25).and_hms_opt(9, 0, 0).unwrap());
        assert_eq!(j.selesai, hari(2026, 10, 25).and_hms_opt(11, 0, 0).unwrap());
        assert_eq!(j.lokasi.as_deref(), Some("Kantor Dinas"));
    }

    #[test]
    fn nama_bulan_tanpa_tahun_memakai_tahun_ini() {
        let j = parse("buat jadwal 25 Oktober jam 14", Some("Survei lokasi"), hari(2026, 10, 18)).unwrap();
        assert_eq!(j.mulai, hari(2026, 10, 25).and_hms_opt(14, 0, 0).unwrap());
        assert_eq!(j.lokasi, None);
    }

    #[test]
    fn tanpa_jam_jadi_seharian() {
        let j = parse("buat jadwal 2026-10-25", Some("Libur kerja"), hari(2026, 10, 18)).unwrap();
        assert!(j.seharian);
        assert_eq!(j.selesai, hari(2026, 10, 25).and_hms_opt(23, 59, 0).unwrap());
    }

    #[test]
    fn tanggal_tidak_terbaca_ditolak() {
        assert!(parse("buat jadwal nanti saja", Some("x"), hari(2026, 10, 18)).is_err());
    }

    #[test]
    fn tanpa_pesan_yang_dibalas_ditolak() {
        assert!(parse("buat jadwal besok jam 9", None, hari(2026, 10, 18)).is_err());
    }

    #[test]
    fn jam_selesai_sebelum_mulai_ditolak() {
        assert!(parse("buat jadwal besok 11:00-09:00", Some("x"), hari(2026, 10, 18)).is_err());
    }

    #[test]
    fn perintah_dikenali() {
        assert!(perintah_jadwal("@arumanis buat jadwal besok"));
        assert!(!perintah_jadwal("berapa jadwal besok?"));
    }
}
