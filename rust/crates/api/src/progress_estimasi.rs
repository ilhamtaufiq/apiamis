//! Ringkasan estimasi fisik dan keuangan per tahun anggaran,
//! setara `PekerjaanProgressEstimasiSummaryService` (Laravel).
//! Belum dipasang ke respon: butuh parameter `summary` yang belum dipindah.

use chrono::NaiveDate;
use serde_json::{json, Value};

use crate::progress_metrics::round2;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub id: u64,
    pub tahun_anggaran: i64,
    pub jenis: String,
    pub tipe: String,
    pub tanggal: NaiveDate,
    pub persen: f64,
    pub nilai: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub fisik_realisasi: Option<f64>,
    pub fisik_rencana: Option<f64>,
    pub fisik_deviasi: Option<f64>,
    pub keuangan_realisasi: Option<f64>,
    pub keuangan_rencana: Option<f64>,
    pub keuangan_deviasi: Option<f64>,
    pub keuangan_realisasi_nilai: Option<f64>,
}

impl Summary {
    /// Key `PekerjaanResource` yang memakai ringkasan ini.
    pub fn resource_fields(&self) -> Value {
        let n = |v: Option<f64>| v.map_or(Value::Null, crate::format::number_like_php);
        json!({
            "progress_estimasi_fisik": n(self.fisik_realisasi),
            "progress_estimasi_keuangan": n(self.keuangan_realisasi),
            "progress_estimasi_keuangan_nilai": n(self.keuangan_realisasi_nilai),
            "deviasi_estimasi_fisik": n(self.fisik_deviasi),
            "deviasi_estimasi_keuangan": n(self.keuangan_deviasi),
        })
    }
}

struct Section {
    latest_rencana: Option<Entry>,
    latest_realisasi: Option<Entry>,
}

#[derive(Clone, Copy)]
struct Entry {
    id: u64,
    tanggal: NaiveDate,
    persen: f64,
    nilai: Option<f64>,
}

/// `latestEntry()`: tanggal terbaru, lalu id terbesar.
fn latest(entries: &[Entry]) -> Option<Entry> {
    entries
        .iter()
        .copied()
        .max_by(|a, b| (a.tanggal, a.id).cmp(&(b.tanggal, b.id)))
}

fn section(rows: &[&HistoryRow], jenis: &str) -> Section {
    let of = |tipe: &str| -> Vec<Entry> {
        rows.iter()
            .filter(|r| r.jenis == jenis && r.tipe == tipe)
            .map(|r| Entry {
                id: r.id,
                tanggal: r.tanggal,
                persen: r.persen,
                nilai: r.nilai,
            })
            .collect()
    };
    Section {
        latest_rencana: latest(&of("rencana")),
        latest_realisasi: latest(&of("realisasi")),
    }
}

pub fn summarize(histories: &[HistoryRow], tahun_anggaran: i64) -> Summary {
    let rows: Vec<&HistoryRow> = histories
        .iter()
        .filter(|h| h.tahun_anggaran == tahun_anggaran)
        .collect();
    let fisik = section(&rows, "fisik");
    let keuangan = section(&rows, "keuangan");

    let deviasi = |s: &Section| match (s.latest_realisasi, s.latest_rencana) {
        (Some(r), Some(p)) => Some(round2(r.persen - p.persen)),
        _ => None,
    };

    Summary {
        fisik_realisasi: fisik.latest_realisasi.map(|e| e.persen),
        fisik_rencana: fisik.latest_rencana.map(|e| e.persen),
        fisik_deviasi: deviasi(&fisik),
        keuangan_realisasi: keuangan.latest_realisasi.map(|e| e.persen),
        keuangan_rencana: keuangan.latest_rencana.map(|e| e.persen),
        keuangan_deviasi: deviasi(&keuangan),
        keuangan_realisasi_nilai: keuangan.latest_realisasi.and_then(|e| e.nilai),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(
        id: u64,
        jenis: &str,
        tipe: &str,
        tgl: (i32, u32, u32),
        persen: f64,
        nilai: Option<f64>,
    ) -> HistoryRow {
        HistoryRow {
            id,
            tahun_anggaran: 2025,
            jenis: jenis.into(),
            tipe: tipe.into(),
            tanggal: NaiveDate::from_ymd_opt(tgl.0, tgl.1, tgl.2).unwrap(),
            persen,
            nilai,
        }
    }

    #[test]
    fn uses_latest_by_date_then_id_and_computes_deviasi() {
        let rows = vec![
            h(1, "fisik", "rencana", (2025, 3, 1), 30.0, None),
            h(2, "fisik", "rencana", (2025, 6, 1), 60.0, None),
            h(3, "fisik", "realisasi", (2025, 2, 1), 20.0, None),
            h(4, "fisik", "realisasi", (2025, 6, 1), 55.5, None),
            h(5, "fisik", "realisasi", (2025, 6, 1), 58.0, None),
            h(
                6,
                "keuangan",
                "realisasi",
                (2025, 5, 1),
                40.0,
                Some(1_000_000.0),
            ),
        ];
        let s = summarize(&rows, 2025);
        assert_eq!(s.fisik_rencana, Some(60.0));
        assert_eq!(
            s.fisik_realisasi,
            Some(58.0),
            "tanggal sama: id terbesar menang"
        );
        assert_eq!(s.fisik_deviasi, Some(-2.0));
        assert_eq!(s.keuangan_realisasi, Some(40.0));
        assert_eq!(s.keuangan_rencana, None);
        assert_eq!(s.keuangan_deviasi, None);
        assert_eq!(s.keuangan_realisasi_nilai, Some(1_000_000.0));
    }

    #[test]
    fn other_years_are_ignored() {
        let mut other = h(9, "fisik", "realisasi", (2024, 1, 1), 99.0, None);
        other.tahun_anggaran = 2024;
        let s = summarize(&[other], 2025);
        assert_eq!(s.fisik_realisasi, None);
        assert_eq!(s.resource_fields()["progress_estimasi_fisik"], Value::Null);
    }
}
