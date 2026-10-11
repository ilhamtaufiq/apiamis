//! Generator deck PPTX laporan capaian SKP bulanan, ditulis ulang dari `generate_skp.py`.
//!
//! Modul ini hanya menyusun berkas `.pptx` (OOXML) dari `Laporan`. Pengambilan data dari
//! database ada di `skp_jurnal`. Tidak ada template: setiap slide digambar langsung.
//!
//! Kontrak data: `Laporan` dan turunannya di bawah. Perhitungan capaian ada di fungsi ini,
//! bukan di pemanggil: capaian = realisasi / rencana per RHK per bulan, dan RHK tanpa rencana
//! tidak dihitung dalam rata-rata (lihat `capaian`).

use chrono::NaiveDate;

/// Gambar biner (JPEG atau PNG). `png` false berarti JPEG.
#[derive(Debug, Clone, PartialEq)]
pub struct Gambar {
    pub bytes: Vec<u8>,
    pub png: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Orang {
    pub nama: String,
    pub nip: String,
    pub pangkat: String,
    pub jabatan: String,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RhkLaporan {
    /// Nomor RHK (1..99), sama dengan `skp_jurnal_kegiatan.rhk`.
    pub no: i64,
    pub rhk: String,
    pub satuan: String,
    /// Target tahunan.
    pub target: f64,
    /// Rencana aksi per bulan, index 0 = Januari .. 11 = Desember.
    pub rencana_per_bulan: [f64; 12],
    /// Realisasi per bulan (jumlah output jurnal per RHK per bulan), index sama seperti di atas.
    pub realisasi_per_bulan: [f64; 12],
    /// Foto bukti RHK pada bulan laporan. Slide Bukti memakai yang pertama, sisanya di Lampiran.
    pub bukti: Vec<Gambar>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JurnalLaporan {
    pub tgl: NaiveDate,
    pub rhk: Option<i64>,
    pub kegiatan: String,
    pub output: Option<f64>,
    pub satuan: String,
    pub keterangan: String,
    pub foto: Vec<Gambar>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Laporan {
    pub instansi: String,
    pub tahun: i32,
    /// Bulan laporan, 1..=12.
    pub bulan: u32,
    pub pegawai: Orang,
    pub atasan: Orang,
    /// Diurutkan menurut `no`.
    pub rhk: Vec<RhkLaporan>,
    /// Kegiatan jurnal pada bulan laporan, diurutkan menurut `tgl`.
    pub jurnal: Vec<JurnalLaporan>,
    /// Umpan balik atasan. Kosong = tampilkan kotak placeholder.
    pub feedback: String,
    /// Strategi tambahan dari pengguna, ditampilkan di slide Rencana.
    pub strategi: Vec<String>,
}

/// Capaian satu RHK pada bulan tertentu: `None` bila tidak ada rencana dan tidak ada realisasi.
/// Rencana 0 dengan realisasi > 0 dihitung 100% (sama dengan `capaian` di Python).
pub fn capaian(rencana: f64, realisasi: f64) -> Option<f64> {
    if rencana > 0.0 {
        Some(realisasi / rencana)
    } else if realisasi > 0.0 {
        Some(1.0)
    } else {
        None
    }
}

/// Susun deck lengkap dan kembalikan isi berkas `.pptx`.
pub fn build_pptx(laporan: &Laporan) -> Result<Vec<u8>, String> {
    let _ = laporan;
    Err("belum diimplementasi".to_string())
}
