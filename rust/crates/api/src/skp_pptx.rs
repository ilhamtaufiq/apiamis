//! Generator deck PPTX laporan capaian SKP bulanan, ditulis ulang dari `generate_skp.py`.
//!
//! Modul ini hanya menyusun berkas `.pptx` (OOXML) dari `Laporan`. Pengambilan data dari
//! database ada di `skp_jurnal`. Tidak ada template: setiap slide digambar langsung.
//!
//! Kontrak data: `Laporan` dan turunannya di bawah. Perhitungan capaian ada di fungsi ini,
//! bukan di pemanggil: capaian = realisasi / rencana per RHK per bulan, dan RHK tanpa rencana
//! tidak dihitung dalam rata-rata (lihat `capaian`).

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Write};

use chrono::NaiveDate;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

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

// ===================================================================================
// Konstanta desain (dari bagian atas generate_skp.py)
// ===================================================================================

type Warna = &'static str;

const NAVY: Warna = "0B3B78";
const NAVY_D: Warna = "0A3163";
const NAVY_SOFT: Warna = "A9C1E6";
const ORANGE: Warna = "F5A623";
const GREEN: Warna = "1E8E4E";
const GREEN_T: Warna = "E3F4EA";
const AMBER: Warna = "C97A00";
const AMBER_T: Warna = "FDF1DC";
const RED: Warna = "C63B3B";
const RED_T: Warna = "FBE7E7";
const GREY: Warna = "6B7280";
const GREY_T: Warna = "EEF0F3";
const DARK: Warna = "1F2937";
const WHITE: Warna = "FFFFFF";
const BG: Warna = "F4F6FA";
const ROW_ALT: Warna = "F8FAFD";
const GARIS: Warna = "DDE3EC";
const FONT: &str = "Google Sans Flex";
const W: f64 = 13.333;
const H: f64 = 7.5;
const SLIDE_CX: i64 = 12_192_000;
const SLIDE_CY: i64 = 6_858_000;

const NAMA_BULAN: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September",
    "Oktober", "November", "Desember",
];
const SINGKAT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
];

/// Judul singkat + uraian kegiatan per RHK (nomor sesuai Excel).
const BUKTI: [(i64, &str, &str); 4] = [
    (1, "Perencanaan", "Membuat spesifikasi teknis, rancangan kontrak, dan daftar kuantitas harga satuan biaya."),
    (2, "Input Aplikasi", "Membuat paket dan mengunggah dokumen pengadaan barang/jasa pada aplikasi SPSE, SIMSPAM, SICALMERS, dan e-mondak."),
    (3, "Kontrak Pekerjaan", "Menyusun dan mengelola dokumen kontrak pekerjaan."),
    (4, "Tugas Tambahan", "Melaksanakan direktif dan tugas tambahan sesuai perintah pimpinan."),
];

const BERAKHLAK: [(&str, &str, &str); 7] = [
    ("B", "Berorientasi Pelayanan", "Memahami dan memenuhi kebutuhan masyarakat dengan ramah dan solutif."),
    ("A", "Akuntabel", "Jujur, bertanggung jawab, cermat, dan disiplin dalam bertugas."),
    ("K", "Kompeten", "Terus belajar dan mengembangkan kemampuan untuk hasil terbaik."),
    ("H", "Harmonis", "Saling peduli, menghargai perbedaan, dan menjaga suasana kerja kondusif."),
    ("L", "Loyal", "Berdedikasi dan mengutamakan kepentingan bangsa dan negara."),
    ("A", "Adaptif", "Cepat menyesuaikan diri dan antusias terhadap perubahan."),
    ("K", "Kolaboratif", "Membangun kerja sama yang sinergis."),
];

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const GRUP: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
const TABLE_STYLE: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";
const REL_OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_CORE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
const REL_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
const REL_PRESPROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
const REL_VIEWPROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps";
const REL_TABLESTYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_CHART: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";

// ===================================================================================
// Helper teks, angka, dan perhitungan
// ===================================================================================

/// Escape XML dan buang karakter yang tidak boleh ada dalam XML 1.0.
fn xml(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) >= 0x20 && !(0xD800..=0xDFFF).contains(&(c as u32)) && c != '\u{FFFE}' && c != '\u{FFFF}' => o.push(c),
            _ => {}
        }
    }
    o
}

/// Inci ke EMU, dipotong seperti `Inches()` di python-pptx.
fn emu(inci: f64) -> i64 {
    (inci * 914_400.0) as i64
}

/// Meniru `str.title()` dari Python.
fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        if prev_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        prev_cased = c.is_lowercase() || c.is_uppercase();
    }
    out
}

/// Potong ke `maks` karakter (bukan byte); bila terpotong, akhiri dengan "...".
fn potong(s: &str, maks: usize) -> String {
    if s.chars().count() <= maks {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(maks.saturating_sub(3)).collect();
        o.push_str("...");
        o
    }
}

fn awal(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Buang " " dan "·" di ujung, seperti `rstrip(" ·")`.
fn rstrip_judul(s: &str) -> String {
    s.trim_end_matches(|c: char| c == ' ' || c == '·').to_string()
}

/// Format angka ala `num()` di Python: bulat tanpa desimal, selain itu satu desimal dengan koma.
fn num(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{:.1}", v).replace('.', ",")
    }
}

/// Format persen ala `pct()` di Python.
fn pct(v: Option<f64>) -> String {
    match v {
        None => "–".to_string(),
        Some(v) => {
            let x = v * 100.0;
            if (x - x.round()).abs() < 0.05 {
                format!("{:.0}%", x.round())
            } else {
                format!("{:.1}%", x).replace('.', ",")
            }
        }
    }
}

/// Label, warna teks, dan warna tint untuk status capaian.
fn status(c: Option<f64>) -> (&'static str, Warna, Warna) {
    match c {
        None => ("Tidak ada target", GREY, GREY_T),
        Some(c) if c >= 1.0 => ("Tercapai", GREEN, GREEN_T),
        Some(c) if c >= 0.75 => ("Hampir tercapai", AMBER, AMBER_T),
        Some(_) => ("Belum tercapai", RED, RED_T),
    }
}

fn inisial(nama: &str) -> String {
    let depan = nama.split(',').next().unwrap_or("");
    depan
        .split_whitespace()
        .filter(|k| k.chars().next().is_some_and(|c| c.is_alphabetic()))
        .take(2)
        .filter_map(|k| k.chars().next())
        .collect::<String>()
        .to_uppercase()
}

fn info_bukti(no: i64) -> Option<(&'static str, &'static str)> {
    BUKTI.iter().find(|b| b.0 == no).map(|b| (b.1, b.2))
}

fn judul_rhk(no: i64, cadangan: &str) -> String {
    info_bukti(no).map(|b| b.0.to_string()).unwrap_or_else(|| cadangan.to_string())
}

fn cap_bln(r: &RhkLaporan, idx: usize) -> Option<f64> {
    capaian(r.rencana_per_bulan[idx], r.realisasi_per_bulan[idx])
}

/// Realisasi kumulatif Januari s.d. bulan `idx` (ikut bulan itu).
fn kum(r: &RhkLaporan, idx: usize) -> f64 {
    r.realisasi_per_bulan[..=idx].iter().sum()
}

/// Rencana dari bulan setelah `idx` sampai Desember.
fn sisa_rencana(r: &RhkLaporan, idx: usize) -> f64 {
    r.rencana_per_bulan[idx + 1..].iter().sum()
}

/// Rencana bulan depan; 0 bila bulan laporan sudah Desember.
fn rencana_depan(r: &RhkLaporan, idx: usize) -> f64 {
    r.rencana_per_bulan.get(idx + 1).copied().unwrap_or(0.0)
}

fn dimensi(bytes: &[u8]) -> Result<(u32, u32), String> {
    let baca = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("gambar tidak terbaca: {e}"))?;
    let (w, h) = baca
        .into_dimensions()
        .map_err(|e| format!("dimensi gambar tidak terbaca: {e}"))?;
    if w == 0 || h == 0 {
        return Err("gambar berukuran nol".to_string());
    }
    Ok((w, h))
}

/// Data turunan untuk satu laporan (bulan `idx`, 0 = Januari).
struct Konteks<'a> {
    l: &'a Laporan,
    idx: usize,
    bln: &'static str,
    label: String,
    nama_pendek: String,
    /// Rata-rata capaian per bulan, index 0..=idx.
    bulanan: Vec<Option<f64>>,
    avg: Option<f64>,
    strategi: Vec<String>,
}

impl<'a> Konteks<'a> {
    fn baru(l: &'a Laporan, idx: usize) -> Self {
        let bulanan: Vec<Option<f64>> = (0..=idx)
            .map(|i| {
                let nilai: Vec<f64> = l
                    .rhk
                    .iter()
                    .filter_map(|r| capaian(r.rencana_per_bulan[i], r.realisasi_per_bulan[i]))
                    .collect();
                if nilai.is_empty() {
                    None
                } else {
                    Some(nilai.iter().sum::<f64>() / nilai.len() as f64)
                }
            })
            .collect();
        let avg = bulanan[idx];
        let strategi = l
            .strategi
            .iter()
            .map(|s| s.replace(['\n', '\r'], " ").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Konteks {
            l,
            idx,
            bln: NAMA_BULAN[idx],
            label: format!("{} {}", NAMA_BULAN[idx], l.tahun),
            nama_pendek: py_title(l.pegawai.nama.split(',').next().unwrap_or("").trim()),
            bulanan,
            avg,
            strategi,
        }
    }

    fn kat(&self) -> Vec<String> {
        SINGKAT[..=self.idx].iter().map(|s| s.to_string()).collect()
    }
}

// ===================================================================================
// Model slide dan paket OOXML
// ===================================================================================

#[derive(Clone, Copy, PartialEq)]
enum Rata {
    Kiri,
    Tengah,
    Kanan,
}

#[derive(Clone, Copy, PartialEq)]
enum Anchor {
    Atas,
    Tengah,
}

#[derive(Clone, Copy, PartialEq)]
enum Bentuk {
    Persegi,
    /// Persegi panjang sudut bulat (`ROUND` di Python), radius dalam fraksi.
    Sudut,
    Oval,
}

/// Format teks, setara dengan argumen `tulis()` di Python.
#[derive(Clone, Copy)]
struct Gaya {
    ukuran: u32,
    tebal: bool,
    miring: bool,
    warna: Warna,
    rata: Rata,
    spasi: u32,
}

fn gaya(ukuran: u32) -> Gaya {
    Gaya { ukuran, tebal: false, miring: false, warna: DARK, rata: Rata::Kiri, spasi: 0 }
}

impl Gaya {
    fn tebal(mut self) -> Self {
        self.tebal = true;
        self
    }
    fn miring(mut self) -> Self {
        self.miring = true;
        self
    }
    fn warna(mut self, warna: Warna) -> Self {
        self.warna = warna;
        self
    }
    fn rata(mut self, rata: Rata) -> Self {
        self.rata = rata;
        self
    }
    fn spasi(mut self, spasi: u32) -> Self {
        self.spasi = spasi;
        self
    }
}

/// Satu sel tabel.
struct Sel {
    teks: String,
    gaya: Gaya,
    fill: Warna,
}

fn sel(teks: impl Into<String>, gaya: Gaya, fill: Warna) -> Sel {
    Sel { teks: teks.into(), gaya, fill }
}

/// Baris header tabel: teks putih tebal di atas NAVY, rata tengah.
fn baris_hdr(labels: &[&str], tinggi: f64) -> (f64, Vec<Sel>) {
    (
        tinggi,
        labels
            .iter()
            .map(|l| sel(*l, gaya(11).tebal().warna(WHITE).rata(Rata::Tengah), NAVY))
            .collect(),
    )
}

#[derive(Default)]
struct Halaman {
    body: String,
    nid: u32,
    bg: Option<Warna>,
    /// (jenis relasi, target). rId yang dihasilkan = indeks + 2 (rId1 untuk layout).
    rels: Vec<(&'static str, String)>,
}

#[derive(Default)]
struct Deck {
    halaman: Vec<Halaman>,
    grafik: Vec<String>,
    media: Vec<(String, Vec<u8>)>,
}

fn paragrafs(teks: &str, g: Gaya) -> String {
    let rata = match g.rata {
        Rata::Kiri => "l",
        Rata::Tengah => "ctr",
        Rata::Kanan => "r",
    };
    let spasi = if g.spasi > 0 {
        format!("<a:spcAft><a:spcPts val=\"{}\"/></a:spcAft>", g.spasi * 100)
    } else {
        String::new()
    };
    let mut out = String::new();
    for baris in teks.split('\n') {
        let baris = baris.trim_end_matches('\r');
        out += &format!(
            r#"<a:p><a:pPr algn="{rata}">{spasi}</a:pPr><a:r><a:rPr lang="id-ID" sz="{}" b="{}" i="{}" dirty="0"><a:solidFill><a:srgbClr val="{}"/></a:solidFill><a:latin typeface="{FONT}"/></a:rPr><a:t>{}</a:t></a:r></a:p>"#,
            g.ukuran * 100,
            g.tebal as u8,
            g.miring as u8,
            g.warna,
            xml(baris)
        );
    }
    out
}

impl Deck {
    fn baru(&mut self) {
        self.halaman.push(Halaman { nid: 1, ..Default::default() });
    }

    fn hal(&mut self) -> &mut Halaman {
        self.halaman.last_mut().expect("halaman sudah dibuat")
    }

    fn id(&mut self) -> u32 {
        let h = self.hal();
        h.nid += 1;
        h.nid
    }

    fn push(&mut self, xml: String) {
        self.hal().body.push_str(&xml);
    }

    fn rel(&mut self, jenis: &'static str, target: String) -> String {
        let h = self.hal();
        h.rels.push((jenis, target));
        format!("rId{}", h.rels.len() + 1)
    }

    fn latar(&mut self, warna: Warna) {
        self.hal().bg = Some(warna);
    }

    fn kotak(&mut self, x: f64, y: f64, w: f64, h: f64, fill: Option<Warna>, garis: Option<Warna>, bentuk: Bentuk, radius: f64) {
        self.sp(x, y, w, h, fill, garis, bentuk, radius, false, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn kotak_isi(&mut self, x: f64, y: f64, w: f64, h: f64, fill: Option<Warna>, garis: Option<Warna>, bentuk: Bentuk, radius: f64, teks: &str, g: Gaya) {
        self.sp(x, y, w, h, fill, garis, bentuk, radius, false, Some((teks, g)));
    }

    /// Bentuk dengan garis putus-putus dan isi teks (placeholder foto).
    #[allow(clippy::too_many_arguments)]
    fn kotak_putus(&mut self, x: f64, y: f64, w: f64, h: f64, fill: Warna, garis: Warna, radius: f64, teks: &str, g: Gaya) {
        self.sp(x, y, w, h, Some(fill), Some(garis), Bentuk::Sudut, radius, true, Some((teks, g)));
    }

    #[allow(clippy::too_many_arguments)]
    fn sp(&mut self, x: f64, y: f64, w: f64, h: f64, fill: Option<Warna>, garis: Option<Warna>, bentuk: Bentuk, radius: f64, dash: bool, isi: Option<(&str, Gaya)>) {
        let id = self.id();
        let geom = match bentuk {
            Bentuk::Persegi => r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>"#.to_string(),
            Bentuk::Sudut => format!(
                r#"<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val {}"/></a:avLst></a:prstGeom>"#,
                (radius * 100_000.0).round() as i64
            ),
            Bentuk::Oval => r#"<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom>"#.to_string(),
        };
        let fill_xml = match fill {
            Some(c) => format!(r#"<a:solidFill><a:srgbClr val="{c}"/></a:solidFill>"#),
            None => "<a:noFill/>".to_string(),
        };
        let ln = match garis {
            Some(c) => format!(
                r#"<a:ln w="9525"><a:solidFill><a:srgbClr val="{c}"/></a:solidFill>{}</a:ln>"#,
                if dash { r#"<a:prstDash val="dash"/>"# } else { "" }
            ),
            None => "<a:ln><a:noFill/></a:ln>".to_string(),
        };
        let txt = match isi {
            Some((t, g)) => format!(
                r#"<p:txBody><a:bodyPr wrap="square" lIns="45720" tIns="18288" rIns="45720" bIns="18288" rtlCol="0" anchor="ctr"/><a:lstStyle/>{}</p:txBody>"#,
                paragrafs(t, g)
            ),
            None => String::new(),
        };
        let xml = format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Bentuk {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>{geom}{fill_xml}{ln}</p:spPr>{txt}</p:sp>"#,
            emu(x),
            emu(y),
            emu(w),
            emu(h)
        );
        self.push(xml);
    }

    fn teks(&mut self, x: f64, y: f64, w: f64, h: f64, teks: &str, g: Gaya, anchor: Anchor) {
        let id = self.id();
        let anc = match anchor {
            Anchor::Atas => "t",
            Anchor::Tengah => "ctr",
        };
        let xml = format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Teks {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="45720" tIns="18288" rIns="45720" bIns="18288" rtlCol="0" anchor="{anc}"><a:spAutoFit/></a:bodyPr><a:lstStyle/>{}</p:txBody></p:sp>"#,
            emu(x),
            emu(y),
            emu(w),
            emu(h),
            paragrafs(teks, g)
        );
        self.push(xml);
    }

    /// Tabel; baris pertama adalah header. Setiap baris harus punya jumlah sel = `lebar.len()`.
    fn tabel(&mut self, x: f64, y: f64, lebar: &[f64], baris: Vec<(f64, Vec<Sel>)>) {
        let id = self.id();
        let total_h: f64 = baris.iter().map(|b| b.0).sum();
        let total_w: f64 = lebar.iter().sum();
        let mut xml = format!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Tabel {id}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr firstRow="1"><a:tableStyleId>{TABLE_STYLE}</a:tableStyleId></a:tblPr><a:tblGrid>"#,
            emu(x),
            emu(y),
            emu(total_w),
            emu(total_h)
        );
        for w in lebar {
            xml += &format!(r#"<a:gridCol w="{}"/>"#, emu(*w));
        }
        xml += "</a:tblGrid>";
        for (h, sels) in &baris {
            debug_assert_eq!(sels.len(), lebar.len());
            xml += &format!(r#"<a:tr h="{}">"#, emu(*h));
            for s in sels {
                xml += &format!(
                    r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL="{}" marR="{}" marT="{}" marB="{}" anchor="ctr"><a:solidFill><a:srgbClr val="{}"/></a:solidFill></a:tcPr></a:tc>"#,
                    paragrafs(&s.teks, s.gaya),
                    emu(0.08),
                    emu(0.08),
                    emu(0.03),
                    emu(0.03),
                    s.fill
                );
            }
            xml += "</a:tr>";
        }
        xml += "</a:tbl></a:graphicData></a:graphic></p:graphicFrame>";
        self.push(xml);
    }

    fn grafik(&mut self, x: f64, y: f64, w: f64, h: f64, xml_chart: String) {
        let n = self.grafik.len() + 1;
        self.grafik.push(xml_chart);
        let rid = self.rel(REL_CHART, format!("../charts/chart{n}.xml"));
        let id = self.id();
        let xml = format!(
            r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Bagan {id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="{rid}"/></a:graphicData></a:graphic></p:graphicFrame>"#,
            emu(x),
            emu(y),
            emu(w),
            emu(h)
        );
        self.push(xml);
    }

    /// Gambar di dalam kotak, dipusatkan dan diskala tanpa mengubah rasio (sama dengan `gambar_fit`).
    fn gambar_fit(&mut self, x: f64, y: f64, w: f64, h: f64, g: &Gambar) -> Result<(), String> {
        let (pw, ph) = dimensi(&g.bytes)?;
        let png = g.bytes.starts_with(&PNG_SIG);
        let r = (w / pw as f64).min(h / ph as f64);
        let (fw, fh) = (pw as f64 * r, ph as f64 * r);
        let (fx, fy) = (x + (w - fw) / 2.0, y + (h - fh) / 2.0);
        let n = self.media.len() + 1;
        let ext = if png { "png" } else { "jpeg" };
        let nama = format!("image{n}.{ext}");
        self.media.push((nama.clone(), g.bytes.clone()));
        let rid = self.rel(REL_IMAGE, format!("../media/{nama}"));
        let id = self.id();
        let xml = format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Gambar {id}" descr="Foto bukti"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            emu(fx),
            emu(fy),
            emu(fw),
            emu(fh)
        );
        self.push(xml);
        Ok(())
    }
}

// ===================================================================================
// Grafik (c:chartSpace) dengan cache nilai tanpa workbook tersemat
// ===================================================================================

fn txpr(ukuran: u32, tebal: bool, warna: Warna) -> String {
    format!(
        r#"<c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="{}" b="{}"><a:solidFill><a:srgbClr val="{warna}"/></a:solidFill><a:latin typeface="{FONT}"/></a:defRPr></a:pPr><a:endParaRPr lang="id-ID"/></a:p></c:txPr>"#,
        ukuran * 100,
        tebal as u8
    )
}

fn nama_seri(nama: &str) -> String {
    format!("<c:tx><c:v>{}</c:v></c:tx>", xml(nama))
}

fn kat_xml(kat: &[String]) -> String {
    let pts: String = kat
        .iter()
        .enumerate()
        .map(|(i, k)| format!(r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#, xml(k)))
        .collect();
    format!(r#"<c:cat><c:strLit><c:ptCount val="{}"/>{pts}</c:strLit></c:cat>"#, kat.len())
}

fn nilai_xml(nilai: &[Option<f64>]) -> String {
    let pts: String = nilai
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)))
        .collect();
    format!(
        r#"<c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>{pts}</c:numLit></c:val>"#,
        nilai.len()
    )
}

fn label_nilai(ukuran: u32, tebal: bool, warna: Warna, format_kode: &str, posisi: &str) -> String {
    format!(
        r#"<c:dLbls><c:numFmt formatCode="{format_kode}" sourceLinked="0"/><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>{}<c:dLblPos val="{posisi}"/><c:showLegendKey val="0"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/><c:showBubbleSize val="0"/></c:dLbls>"#,
        txpr(ukuran, tebal, warna)
    )
}

/// Sumbu kategori dan nilai. `vmax` hanya untuk grafik persen (skala 0..vmax, langkah 20).
fn eixos(vmax: Option<i64>, persen: bool) -> String {
    let format_kode = if persen { "0&quot;%&quot;" } else { "0" };
    let maks = vmax.map(|v| format!(r#"<c:max val="{v}"/>"#)).unwrap_or_default();
    let langkah = vmax.map(|_| r#"<c:majorUnit val="20"/>"#).unwrap_or_default();
    let garis = format!(r#"<a:ln w="9525"><a:solidFill><a:srgbClr val="{GARIS}"/></a:solidFill></a:ln>"#);
    format!(
        r#"<c:catAx><c:axId val="111"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:spPr>{garis}</c:spPr>{}<c:crossAx val="222"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx><c:valAx><c:axId val="222"/><c:scaling><c:orientation val="minMax"/>{maks}<c:min val="0"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:majorGridlines><c:spPr>{garis}</c:spPr></c:majorGridlines><c:numFmt formatCode="{format_kode}" sourceLinked="0"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:spPr><a:ln><a:noFill/></a:ln></c:spPr>{}<c:crossAx val="111"/><c:crosses val="autoZero"/><c:crossBetween val="between"/>{langkah}</c:valAx>"#,
        txpr(11, false, DARK),
        txpr(10, false, GREY)
    )
}

fn bungkus_grafik(plot: &str, sumbu: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><c:date1904 val="0"/><c:roundedCorners val="0"/><c:chart><c:autoTitleDeleted val="1"/><c:plotArea><c:layout/>{plot}{sumbu}<c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr></c:plotArea><c:legend><c:legendPos val="b"/><c:overlay val="0"/>{}</c:legend><c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr>{}</c:chartSpace>"#,
        txpr(11, false, GREY),
        txpr(11, false, GREY)
    )
}

/// Line chart tren capaian (garis capaian dengan label, garis target 100% putus-putus).
fn grafik_tren(kat: &[String], nilai: &[Option<f64>], vmax: i64) -> String {
    let dl = label_nilai(11, true, NAVY, "0&quot;%&quot;", "t");
    let ser0 = format!(
        r#"<c:ser><c:idx val="0"/><c:order val="0"/>{}<c:spPr><a:ln w="38100" cap="rnd"><a:solidFill><a:srgbClr val="{NAVY}"/></a:solidFill><a:round/></a:ln></c:spPr><c:marker><c:symbol val="circle"/><c:size val="9"/><c:spPr><a:solidFill><a:srgbClr val="{ORANGE}"/></a:solidFill><a:ln w="9525"><a:solidFill><a:srgbClr val="{WHITE}"/></a:solidFill></a:ln></c:spPr></c:marker>{dl}{}{}<c:smooth val="0"/></c:ser>"#,
        nama_seri("Capaian (%)"),
        kat_xml(kat),
        nilai_xml(nilai)
    );
    let target: Vec<Option<f64>> = vec![Some(100.0); kat.len()];
    let ser1 = format!(
        r#"<c:ser><c:idx val="1"/><c:order val="1"/>{}<c:spPr><a:ln w="19050" cap="rnd"><a:solidFill><a:srgbClr val="{GREEN}"/></a:solidFill><a:prstDash val="dash"/><a:round/></a:ln></c:spPr><c:marker><c:symbol val="none"/></c:marker>{}{}<c:smooth val="0"/></c:ser>"#,
        nama_seri("Target (100%)"),
        kat_xml(kat),
        nilai_xml(&target)
    );
    let plot = format!(
        r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>{ser0}{ser1}<c:marker val="1"/><c:axId val="111"/><c:axId val="222"/></c:lineChart>"#
    );
    bungkus_grafik(&plot, &eixos(Some(vmax), true))
}

/// Column chart rencana vs realisasi per RHK.
fn grafik_rhk(kat: &[String], rencana: &[f64], realisasi: &[f64]) -> String {
    let ren: Vec<Option<f64>> = rencana.iter().map(|v| Some(*v)).collect();
    let rea: Vec<Option<f64>> = realisasi.iter().map(|v| Some(*v)).collect();
    let seri = |idx: u32, nama: &str, warna: Warna, nilai: &[Option<f64>]| {
        format!(
            r#"<c:ser><c:idx val="{idx}"/><c:order val="{idx}"/>{}<c:spPr><a:solidFill><a:srgbClr val="{warna}"/></a:solidFill></c:spPr><c:invertIfNegative val="0"/>{}{}</c:ser>"#,
            nama_seri(nama),
            kat_xml(kat),
            nilai_xml(nilai)
        )
    };
    let dl = label_nilai(12, true, DARK, "0", "outEnd");
    let plot = format!(
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>{}{}{dl}<c:gapWidth val="70"/><c:overlap val="-15"/><c:axId val="111"/><c:axId val="222"/></c:barChart>"#,
        seri(0, "Rencana Aksi", NAVY_SOFT, &ren),
        seri(1, "Realisasi", NAVY, &rea)
    );
    bungkus_grafik(&plot, &eixos(None, false))
}

// ===================================================================================
// Slide
// ===================================================================================

/// Kerangka slide isi: strip atas, chip nomor, judul, label periode, dan footer.
fn kerangka(d: &mut Deck, c: &Konteks, judul: &str, sub: Option<&str>) {
    d.baru();
    let no = d.halaman.len();
    d.latar(BG);
    d.kotak(0.0, 0.0, W, 0.1, Some(NAVY), None, Bentuk::Persegi, 0.0);
    d.kotak_isi(0.6, 0.42, 0.62, 0.62, Some(ORANGE), None, Bentuk::Sudut, 0.2, &format!("{no:02}"), gaya(18).tebal().warna(WHITE).rata(Rata::Tengah));
    d.teks(1.38, 0.32, 8.6, 0.55, judul, gaya(26).tebal().warna(NAVY), Anchor::Atas);
    if let Some(s) = sub {
        d.teks(1.38, 0.84, 8.6, 0.35, s, gaya(12).warna(GREY), Anchor::Atas);
    }
    d.teks(9.6, 0.5, 3.13, 0.4, &c.label, gaya(13).tebal().warna(NAVY).rata(Rata::Kanan), Anchor::Atas);
    d.kotak(0.6, 7.0, W - 1.2, 0.01, Some(GARIS), None, Bentuk::Persegi, 0.0);
    d.teks(
        0.6,
        7.05,
        9.0,
        0.3,
        &format!("Laporan SKP Bulanan  ·  {}  ·  {}", c.nama_pendek, py_title(&c.l.instansi)),
        gaya(10).warna(GREY),
        Anchor::Atas,
    );
    d.teks(W - 1.6, 7.05, 1.0, 0.3, &no.to_string(), gaya(10).tebal().warna(NAVY).rata(Rata::Kanan), Anchor::Atas);
}

fn slide_cover(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    d.baru();
    d.latar(NAVY);
    d.kotak(8.6, -1.6, 6.4, 6.4, Some(NAVY_D), None, Bentuk::Oval, 0.0);
    d.kotak(11.2, 4.9, 3.6, 3.6, Some(NAVY_D), None, Bentuk::Oval, 0.0);
    d.kotak(0.0, H - 0.15, W, 0.15, Some(ORANGE), None, Bentuk::Persegi, 0.0);

    // Logo tidak ada di kontrak `Laporan`, jadi blok logo di Python dilewati.
    let y0 = 0.9;
    d.teks(0.8, y0, 7.5, 0.4, &l.instansi, gaya(14).tebal().warna(ORANGE), Anchor::Atas);
    d.teks(0.8, y0 + 0.45, 8.0, 1.6, "Laporan Capaian\nSasaran Kinerja Pegawai", gaya(40).tebal().warna(WHITE), Anchor::Atas);
    d.kotak_isi(0.8, y0 + 2.25, 3.0, 0.5, Some(ORANGE), None, Bentuk::Sudut, 0.5, &c.label, gaya(16).tebal().warna(NAVY).rata(Rata::Tengah));

    let mut y = y0 + 3.2;
    for (lab, val) in [("Nama", &l.pegawai.nama), ("Jabatan", &l.pegawai.jabatan), ("Unit Kerja", &l.pegawai.unit)] {
        d.teks(0.8, y, 1.6, 0.4, lab, gaya(13).warna(NAVY_SOFT), Anchor::Atas);
        d.teks(2.4, y, 6.0, 0.4, val, gaya(15).tebal().warna(WHITE), Anchor::Atas);
        y += 0.5;
    }

    let (lbl, warna, _) = status(c.avg);
    d.kotak(9.3, 1.75, 3.3, 3.3, Some(WHITE), None, Bentuk::Oval, 0.0);
    d.teks(9.3, 2.55, 3.3, 0.9, &pct(c.avg), gaya(48).tebal().warna(NAVY).rata(Rata::Tengah), Anchor::Atas);
    d.teks(
        9.55,
        3.5,
        2.8,
        0.7,
        &format!("Rata-rata capaian\n{}", c.bln),
        gaya(13).warna(GREY).rata(Rata::Tengah),
        Anchor::Atas,
    );
    d.kotak_isi(9.75, 5.35, 2.4, 0.42, Some(warna), None, Bentuk::Sudut, 0.5, lbl, gaya(13).tebal().warna(WHITE).rata(Rata::Tengah));
}

fn slide_profil(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    kerangka(d, c, "Profil Pegawai & Pejabat Penilai", None);
    let data = [
        ("Pegawai yang Dinilai", &l.pegawai, NAVY),
        ("Pejabat Penilai Kinerja", &l.atasan, ORANGE),
    ];
    for (i, (judul, p, warna)) in data.iter().enumerate() {
        let warna: Warna = *warna;
        let x = 0.6 + i as f64 * 6.18;
        let w = 5.95;
        d.kotak(x, 1.45, w, 5.3, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.03);
        d.kotak(x, 1.45, w, 0.1, Some(warna), None, Bentuk::Persegi, 0.0);
        d.kotak_isi(x + 0.35, 1.85, 1.0, 1.0, Some(warna), None, Bentuk::Oval, 0.0, &inisial(&p.nama), gaya(24).tebal().warna(WHITE).rata(Rata::Tengah));
        d.teks(x + 1.55, 1.9, w - 1.8, 0.35, &judul.to_uppercase(), gaya(11).tebal().warna(GREY), Anchor::Atas);
        d.teks(x + 1.55, 2.25, w - 1.8, 0.6, &p.nama, gaya(18).tebal().warna(NAVY), Anchor::Atas);
        let mut y = 3.2;
        for (lab, val) in [
            ("NIP", &p.nip),
            ("Pangkat/Golongan", &p.pangkat),
            ("Jabatan", &p.jabatan),
            ("Unit Kerja", &p.unit),
        ] {
            d.teks(x + 0.35, y, 1.9, 0.5, lab, gaya(12).warna(GREY), Anchor::Atas);
            let isi = if val.is_empty() { "-" } else { val.as_str() };
            d.teks(x + 2.25, y, w - 2.6, 0.6, isi, gaya(13).tebal(), Anchor::Atas);
            d.kotak(x + 0.35, y + 0.68, w - 0.7, 0.01, Some(GARIS), None, Bentuk::Persegi, 0.0);
            y += 0.8;
        }
    }
}

fn slide_ringkasan(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    let idx = c.idx;
    kerangka(d, c, "Ringkasan & Matriks Capaian", Some("Capaian = realisasi bulan ini ÷ rencana aksi (target bulan ini)"));
    let rows = &l.rhk;
    let ada: Vec<&RhkLaporan> = rows.iter().filter(|r| cap_bln(r, idx).is_some()).collect();
    let ok = ada.iter().filter(|r| cap_bln(r, idx).unwrap_or(0.0) >= 1.0).count();
    let prog: Vec<f64> = rows
        .iter()
        .filter(|r| r.target != 0.0)
        .map(|r| (kum(r, idx) / r.target).min(1.0))
        .collect();
    let rata_prog = if prog.is_empty() { None } else { Some(prog.iter().sum::<f64>() / prog.len() as f64) };
    let total_output: f64 = rows.iter().map(|r| r.realisasi_per_bulan[idx]).sum();
    let kpi = [
        ("Capaian Bulan Ini", pct(c.avg), status(c.avg).1),
        ("RHK Tercapai", format!("{} / {}", ok, ada.len()), NAVY),
        ("Total Output Bulan Ini", num(total_output), NAVY),
        ("Progres Target Tahunan", pct(rata_prog), ORANGE),
    ];
    let kw = (W - 1.2 - 3.0 * 0.2) / 4.0;
    for (i, (lab, val, warna)) in kpi.into_iter().enumerate() {
        let x = 0.6 + i as f64 * (kw + 0.2);
        d.kotak(x, 1.45, kw, 1.15, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.08);
        d.kotak(x, 1.6, 0.07, 0.85, Some(warna), None, Bentuk::Persegi, 0.0);
        d.teks(x + 0.25, 1.52, kw - 0.4, 0.6, &val, gaya(26).tebal().warna(warna), Anchor::Atas);
        d.teks(x + 0.25, 2.12, kw - 0.4, 0.35, lab, gaya(11).warna(GREY), Anchor::Atas);
    }

    let lebar = [0.55, 4.55, 1.3, 1.45, 1.3, 1.2, 1.78];
    let n = rows.len();
    let th = (3.4 / n.max(1) as f64).min(0.68);
    let mut tb = vec![baris_hdr(
        &["No", "Rencana Hasil Kerja", "Target Tahun", "Rencana Bulan Ini", "Realisasi", "Capaian", "Status"],
        0.5,
    )];
    for (i, r) in rows.iter().enumerate() {
        let fill = if (i + 1) % 2 == 1 { WHITE } else { ROW_ALT };
        let cp = cap_bln(r, idx);
        let (lbl, warna, tint) = status(cp);
        let sat = &r.satuan;
        tb.push((
            th,
            vec![
                sel(r.no.to_string(), gaya(12).tebal().warna(NAVY).rata(Rata::Tengah), fill),
                sel(r.rhk.as_str(), gaya(12).warna(DARK), fill),
                sel(format!("{} {}", num(r.target), sat), gaya(12).warna(DARK).rata(Rata::Tengah), fill),
                sel(
                    format!("{} {}", num(r.rencana_per_bulan[idx]), sat),
                    gaya(12).warna(DARK).rata(Rata::Tengah),
                    fill,
                ),
                sel(
                    format!("{} {}", num(r.realisasi_per_bulan[idx]), sat),
                    gaya(12).tebal().warna(DARK).rata(Rata::Tengah),
                    fill,
                ),
                sel(pct(cp), gaya(13).tebal().warna(warna).rata(Rata::Tengah), fill),
                sel(lbl, gaya(11).tebal().warna(warna).rata(Rata::Tengah), tint),
            ],
        ));
    }
    d.tabel(0.6, 2.85, &lebar, tb);
    d.teks(
        0.6,
        2.95 + 0.5 + th * n as f64,
        W - 1.2,
        0.3,
        "RHK tanpa rencana aksi pada bulan ini tidak dihitung dalam rata-rata capaian.",
        gaya(10).warna(GREY).miring(),
        Anchor::Atas,
    );
}

fn slide_tren(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    let kat = c.kat();
    kerangka(
        d,
        c,
        "Tren Capaian Bulanan",
        Some(&format!("Rata-rata capaian per bulan, {}–{} {}", kat[0], kat[kat.len() - 1], l.tahun)),
    );
    let nilai: Vec<Option<f64>> = c.bulanan.iter().map(|v| v.map(|x| (x * 1000.0).round() / 10.0)).collect();
    let mut vmax = 100.0_f64;
    for v in nilai.iter().flatten() {
        if *v > vmax {
            vmax = *v;
        }
    }
    let vmax = (((vmax + 10.0) / 20.0).ceil() * 20.0) as i64;

    d.kotak(0.6, 1.45, 8.35, 3.05, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.04);
    d.grafik(0.7, 1.55, 8.15, 2.85, grafik_tren(&kat, &nilai, vmax));

    // Kartu sorotan
    d.kotak(9.15, 1.45, 3.58, 3.05, Some(NAVY), None, Bentuk::Sudut, 0.04);
    d.teks(9.4, 1.55, 3.1, 0.35, "Sorotan", gaya(13).tebal().warna(ORANGE), Anchor::Atas);
    let valid: Vec<(usize, f64)> = c
        .bulanan
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .collect();
    let mut item: Vec<(String, String)> = Vec::new();
    if !valid.is_empty() {
        let rata = valid.iter().map(|(_, v)| v).sum::<f64>() / valid.len() as f64;
        item.push((pct(Some(rata)), format!("Rata-rata {}–{}", kat[0], kat[kat.len() - 1])));
        let tercapai = valid.iter().filter(|(_, v)| *v >= 1.0).count();
        item.push((format!("{} / {}", tercapai, valid.len()), "Bulan dengan capaian 100%".to_string()));
        // `min` pertama, sama dengan `min(valid, key=...)` di Python
        let mut terendah = valid[0];
        for &t in &valid[1..] {
            if t.1 < terendah.1 {
                terendah = t;
            }
        }
        item.push((format!("{} ({})", pct(Some(terendah.1)), kat[terendah.0]), "Capaian terendah".to_string()));
    }
    let mut y = 1.95;
    for (val, lab) in &item {
        d.teks(9.4, y, 3.1, 0.45, val, gaya(22).tebal().warna(WHITE), Anchor::Atas);
        d.teks(9.4, y + 0.45, 3.1, 0.3, lab, gaya(11).warna(NAVY_SOFT), Anchor::Atas);
        y += 0.82;
    }

    // Peta warna per RHK
    let rows = &l.rhk;
    d.teks(0.6, 4.6, 8.0, 0.3, "Peta capaian per RHK", gaya(12).tebal().warna(NAVY), Anchor::Atas);
    let n = kat.len();
    let lw = 2.6;
    let mw = (W - 1.2 - lw) / n as f64;
    let th = (1.85 / (rows.len() as f64 + 1.0)).min(0.38);
    let mut lebar = vec![lw];
    lebar.extend(std::iter::repeat(mw).take(n));
    let mut lbl: Vec<&str> = vec!["RHK"];
    lbl.extend(kat.iter().map(|s| s.as_str()));
    let mut tb = vec![baris_hdr(&lbl, th)];
    for r in rows {
        let nm = rstrip_judul(&format!("RHK {} · {}", r.no, judul_rhk(r.no, "")));
        let mut sels = vec![sel(nm, gaya(10).tebal().warna(NAVY), WHITE)];
        for i in 0..n {
            let cp = capaian(r.rencana_per_bulan[i], r.realisasi_per_bulan[i]);
            let (_, warna, tint) = status(cp);
            sels.push(sel(pct(cp), gaya(10).tebal().warna(warna).rata(Rata::Tengah), tint));
        }
        tb.push((th, sels));
    }
    d.tabel(0.6, 4.95, &lebar, tb);
}

fn slide_rhk(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    let idx = c.idx;
    let bln = c.bln;
    kerangka(
        d,
        c,
        "Capaian per RHK",
        Some(&format!("Rencana aksi vs realisasi {bln} dan progres terhadap target tahunan")),
    );
    let rows = &l.rhk;

    d.kotak(0.6, 1.45, 6.6, 5.35, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.03);
    d.teks(0.85, 1.6, 6.1, 0.35, &format!("Rencana Aksi vs Realisasi – {bln}"), gaya(13).tebal().warna(NAVY), Anchor::Atas);
    let kat: Vec<String> = rows.iter().map(|r| format!("RHK {}", r.no)).collect();
    let ren: Vec<f64> = rows.iter().map(|r| r.rencana_per_bulan[idx]).collect();
    let rea: Vec<f64> = rows.iter().map(|r| r.realisasi_per_bulan[idx]).collect();
    d.grafik(0.75, 2.0, 6.3, 4.65, grafik_rhk(&kat, &ren, &rea));

    let x0 = 7.4;
    let w0 = W - 0.6 - x0;
    d.kotak(x0, 1.45, w0, 5.35, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.03);
    d.teks(x0 + 0.25, 1.6, w0 - 0.5, 0.35, "Progres Target Tahunan", gaya(13).tebal().warna(NAVY), Anchor::Atas);
    let waktu = (idx + 1) as f64 / 12.0;
    d.teks(
        x0 + 0.25,
        1.95,
        w0 - 0.5,
        0.5,
        &format!(
            "Realisasi kumulatif Jan–{} ÷ target tahun. Garis tegak = waktu berjalan ({}/12 bulan).",
            SINGKAT[idx],
            idx + 1
        ),
        gaya(10).warna(GREY),
        Anchor::Atas,
    );
    let bw = w0 - 0.5;
    let gap = (4.1 / rows.len().max(1) as f64).min(1.05);
    let mut y = 2.6;
    for r in rows {
        let k = kum(r, idx);
        let p = if r.target != 0.0 { k / r.target } else { 0.0 };
        let warna = if p >= waktu {
            GREEN
        } else if p >= waktu * 0.75 {
            ORANGE
        } else {
            RED
        };
        d.teks(
            x0 + 0.25,
            y,
            bw - 1.0,
            0.3,
            &rstrip_judul(&format!("RHK {} · {}", r.no, judul_rhk(r.no, ""))),
            gaya(12).tebal().warna(DARK),
            Anchor::Atas,
        );
        d.teks(x0 + 0.25 + bw - 1.0, y, 1.0, 0.3, &pct(Some(p)), gaya(12).tebal().warna(warna).rata(Rata::Kanan), Anchor::Atas);
        d.kotak(x0 + 0.25, y + 0.36, bw, 0.2, Some(GREY_T), None, Bentuk::Sudut, 0.5);
        if p > 0.0 {
            d.kotak(x0 + 0.25, y + 0.36, (bw * p.min(1.0)).max(0.2), 0.2, Some(warna), None, Bentuk::Sudut, 0.5);
        }
        d.kotak(x0 + 0.25 + bw * waktu - 0.015, y + 0.3, 0.03, 0.32, Some(DARK), None, Bentuk::Persegi, 0.0);
        d.teks(
            x0 + 0.25,
            y + 0.6,
            bw,
            0.28,
            &format!("{} dari {} {}", num(k), num(r.target), r.satuan),
            gaya(10).warna(GREY),
            Anchor::Atas,
        );
        y += gap;
    }
}

fn slide_bukti(d: &mut Deck, c: &Konteks) -> Result<(), String> {
    let l = c.l;
    let idx = c.idx;
    kerangka(d, c, "Bukti Dukung & Realisasi Kerja", Some(&format!("Uraian kegiatan dan dokumentasi {}", c.bln)));
    let cw = (W - 1.2 - 0.2) / 2.0;
    let chh = 2.6;
    for (k, r) in l.rhk.iter().take(4).enumerate() {
        let no = r.no;
        let x = 0.6 + (k % 2) as f64 * (cw + 0.2);
        let y = 1.45 + (k / 2) as f64 * (chh + 0.15);
        d.kotak(x, y, cw, chh, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.04);
        d.kotak_isi(x + 0.25, y + 0.22, 0.85, 0.3, Some(NAVY), None, Bentuk::Sudut, 0.5, &format!("RHK {no}"), gaya(11).tebal().warna(WHITE).rata(Rata::Tengah));
        let (judul, uraian) = info_bukti(no).unwrap_or(("", ""));
        let judul = if judul.is_empty() { format!("RHK {no}") } else { judul.to_string() };
        d.teks(x + 0.25, y + 0.6, 3.25, 0.35, &judul, gaya(15).tebal().warna(NAVY), Anchor::Atas);
        d.teks(x + 0.25, y + 0.95, 3.25, 0.5, &r.rhk, gaya(10).warna(GREY), Anchor::Atas);
        d.teks(x + 0.25, y + 1.45, 3.25, 0.7, uraian, gaya(11).warna(DARK), Anchor::Atas);
        let (_, warna, _) = status(cap_bln(r, idx));
        d.teks(
            x + 0.25,
            y + 2.17,
            3.25,
            0.3,
            &format!(
                "Realisasi: {} / {} {}",
                num(r.realisasi_per_bulan[idx]),
                num(r.rencana_per_bulan[idx]),
                r.satuan
            ),
            gaya(11).tebal().warna(warna),
            Anchor::Atas,
        );

        let (fx, fy, fw, fh) = (x + 3.65, y + 0.22, cw - 3.9, chh - 0.44);
        match r.bukti.first() {
            Some(g) => d.gambar_fit(fx, fy, fw, fh, g)?,
            None => d.kotak_putus(
                fx,
                fy,
                fw,
                fh,
                BG,
                GARIS,
                0.04,
                &format!("Foto / screenshot\nBelum ada foto bukti RHK {no}"),
                gaya(9).warna(GREY).rata(Rata::Tengah),
            ),
        }
    }
    Ok(())
}

fn slide_jurnal(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    let bln = c.bln;
    let jr = &l.jurnal;
    let per = 7;
    let halaman: Vec<&[JurnalLaporan]> = if jr.is_empty() {
        vec![&[][..]]
    } else {
        jr.chunks(per).collect()
    };

    let hari = jr.iter().map(|j| j.tgl).collect::<HashSet<NaiveDate>>().len();
    let mut per_rhk: BTreeMap<i64, usize> = BTreeMap::new();
    for j in jr {
        if let Some(n) = j.rhk {
            *per_rhk.entry(n).or_insert(0) += 1;
        }
    }
    let rhk_txt = if per_rhk.is_empty() {
        "–".to_string()
    } else {
        per_rhk
            .iter()
            .map(|(k, v)| format!("RHK {k}: {v}"))
            .collect::<Vec<_>>()
            .join("  ·  ")
    };

    let jumlah = halaman.len();
    for (hal, bagian) in halaman.iter().enumerate() {
        let hal = hal + 1;
        let suf = if jumlah > 1 { format!(" ({hal}/{jumlah})") } else { String::new() };
        kerangka(d, c, &format!("Jurnal Kegiatan{suf}"), Some(&format!("Catatan kegiatan harian {bln} {}", l.tahun)));

        let stat = [
            ("Jumlah Kegiatan", jr.len().to_string()),
            ("Hari Tercatat", hari.to_string()),
            ("Kegiatan per RHK", rhk_txt.clone()),
        ];
        let lebar_s = [2.6, 2.6, W - 1.2 - 5.2 - 0.4];
        let mut x = 0.6;
        for ((lab, val), w) in stat.iter().zip(lebar_s) {
            d.kotak(x, 1.4, w, 0.75, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.1);
            let ukuran = if val.chars().count() < 12 { 18 } else { 13 };
            d.teks(x + 0.15, 1.42, w - 0.3, 0.4, val, gaya(ukuran).tebal().warna(NAVY), Anchor::Tengah);
            d.teks(x + 0.15, 1.82, w - 0.3, 0.28, lab, gaya(10).warna(GREY), Anchor::Atas);
            x += w + 0.2;
        }

        if bagian.is_empty() {
            d.teks(
                0.6,
                3.2,
                W - 1.2,
                0.6,
                &format!("Belum ada entri jurnal untuk {bln} {}.", l.tahun),
                gaya(14).warna(GREY).rata(Rata::Tengah).miring(),
                Anchor::Atas,
            );
            continue;
        }

        let lebar = [1.3, 0.9, 6.4, 1.5, 2.033];
        let mut tb = vec![baris_hdr(&["Tanggal", "RHK", "Uraian Kegiatan", "Output", "Keterangan"], 0.5)];
        for (i, j) in bagian.iter().enumerate() {
            let fill = if (i + 1) % 2 == 1 { WHITE } else { ROW_ALT };
            let uraian = potong(&j.kegiatan, 170);
            let out = match j.output {
                Some(v) if v != 0.0 => format!("{} {}", num(v), j.satuan).trim().to_string(),
                _ => "–".to_string(),
            };
            let rhk = match j.rhk {
                Some(n) if n != 0 => format!("RHK {n}"),
                _ => "–".to_string(),
            };
            let mut ket = if j.keterangan.trim().is_empty() { "–".to_string() } else { j.keterangan.trim().to_string() };
            if !j.foto.is_empty() {
                ket += &format!("  ·  {} foto", j.foto.len());
            }
            tb.push((
                0.55,
                vec![
                    sel(j.tgl.format("%d/%m/%Y").to_string(), gaya(11).tebal().warna(NAVY).rata(Rata::Tengah), fill),
                    sel(rhk, gaya(11).tebal().warna(NAVY).rata(Rata::Tengah), fill),
                    sel(uraian, gaya(11).warna(DARK), fill),
                    sel(out, gaya(11).warna(DARK).rata(Rata::Tengah), fill),
                    sel(ket, gaya(10).warna(GREY), fill),
                ],
            ));
        }
        d.tabel(0.6, 2.35, &lebar, tb);
    }
}

fn slide_galeri(d: &mut Deck, c: &Konteks) -> Result<(), String> {
    let l = c.l;
    let bln = c.bln;
    // Lampiran: foto bukti RHK ke-2 dst. dan semua foto jurnal bulan ini.
    let mut items: Vec<(String, &Gambar)> = Vec::new();
    for r in &l.rhk {
        for g in r.bukti.iter().skip(1) {
            let judul = judul_rhk(r.no, &awal(&r.rhk, 40));
            items.push((format!("RHK {} · {}", r.no, judul), g));
        }
    }
    for j in &l.jurnal {
        for g in &j.foto {
            items.push((format!("{} · {}", j.tgl.format("%d/%m/%Y"), potong(&j.kegiatan, 80)), g));
        }
    }
    if items.is_empty() {
        return Ok(());
    }

    let per = 6;
    let n = items.len().div_ceil(per);
    let cw = (W - 1.2 - 0.4) / 3.0;
    let chh = 2.55;
    for h in 0..n {
        let suf = if n > 1 { format!(" ({}/{})", h + 1, n) } else { String::new() };
        kerangka(d, c, &format!("Lampiran Dokumentasi{suf}"), Some(&format!("Foto bukti dukung dan jurnal {bln}")));
        let akhir = ((h + 1) * per).min(items.len());
        for (k, (cap, g)) in items[h * per..akhir].iter().enumerate() {
            let x = 0.6 + (k % 3) as f64 * (cw + 0.2);
            let y = 1.45 + (k / 3) as f64 * (chh + 0.2);
            d.kotak(x, y, cw, chh, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.04);
            d.gambar_fit(x + 0.12, y + 0.12, cw - 0.24, chh - 0.82, g)?;
            d.teks(x + 0.12, y + chh - 0.65, cw - 0.24, 0.58, cap, gaya(10).warna(DARK), Anchor::Atas);
        }
    }
    Ok(())
}

fn slide_perilaku(d: &mut Deck, c: &Konteks) {
    kerangka(d, c, "Realisasi Perilaku Kerja (BerAKHLAK)", Some("Ekspektasi khusus pimpinan terkait 7 Core Values ASN"));
    let n = BERAKHLAK.len();
    let gap = 0.15;
    let cw = (W - 1.2 - gap * (n as f64 - 1.0)) / n as f64;
    for (i, (huruf, judul, uraian)) in BERAKHLAK.iter().enumerate() {
        let x = 0.6 + i as f64 * (cw + gap);
        d.kotak(x, 1.45, cw, 2.75, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.06);
        let warna = if i % 2 == 0 { NAVY } else { ORANGE };
        d.kotak_isi(x + (cw - 0.62) / 2.0, 1.65, 0.62, 0.62, Some(warna), None, Bentuk::Oval, 0.0, huruf, gaya(20).tebal().warna(WHITE).rata(Rata::Tengah));
        d.teks(x + 0.08, 2.4, cw - 0.16, 0.55, judul, gaya(12).tebal().warna(NAVY).rata(Rata::Tengah), Anchor::Atas);
        d.teks(x + 0.08, 2.95, cw - 0.16, 1.2, uraian, gaya(10).warna(GREY).rata(Rata::Tengah), Anchor::Atas);
    }

    d.kotak(0.6, 4.45, W - 1.2, 2.35, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.04);
    d.kotak(0.6, 4.6, 0.07, 2.05, Some(ORANGE), None, Bentuk::Persegi, 0.0);
    d.teks(0.9, 4.6, 11.0, 0.35, "Catatan / Umpan Balik Atasan", gaya(14).tebal().warna(NAVY), Anchor::Atas);
    if c.l.feedback.is_empty() {
        d.teks(
            0.9,
            5.05,
            W - 1.8,
            1.6,
            "[Tuliskan umpan balik atau apresiasi atasan bulan ini]",
            gaya(13).warna(GREY).miring(),
            Anchor::Atas,
        );
    } else {
        d.teks(0.9, 5.05, W - 1.8, 1.6, &c.l.feedback, gaya(13).warna(DARK), Anchor::Atas);
    }
}

fn slide_rencana(d: &mut Deck, c: &Konteks) {
    let l = c.l;
    let idx = c.idx;
    let nxt: Option<&str> = if idx < 11 { Some(SINGKAT[idx + 1]) } else { None };
    let sub = match nxt {
        Some(_) => format!("Target {} dan sisa target tahunan", NAMA_BULAN[idx + 1]),
        None => "Evaluasi akhir tahun".to_string(),
    };
    kerangka(d, c, "Rencana Aksi Bulan Berikutnya", Some(&sub));
    let rows = &l.rhk;
    let sisa_bulan = (11 - idx) as f64;
    let hdr_depan = match nxt {
        Some(n) => format!("Target {n}"),
        None => "Target Bln Depan".to_string(),
    };
    let hdr_sisa = match nxt {
        Some(n) => format!("Rencana {n}–Des"),
        None => "Sisa Rencana".to_string(),
    };
    let hdr_real = format!("Realisasi Jan–{}", SINGKAT[idx]);
    let lebar = [3.9, 1.65, 1.35, 1.4, 1.6, 2.23];
    let th = (2.6 / rows.len().max(1) as f64).min(0.6);
    let mut tb = vec![baris_hdr(
        &["Rencana Hasil Kerja", hdr_real.as_str(), "Sisa Target", hdr_depan.as_str(), hdr_sisa.as_str(), "Keterangan"],
        0.5,
    )];

    let mut saran: Vec<String> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let no = r.no;
        let sat = r.satuan.as_str();
        let k = kum(r, idx);
        let sisa = (r.target - k).max(0.0);
        let rs = sisa_rencana(r, idx);
        let (ket, warna): (String, Warna) = if sisa <= 0.0 {
            ("Target tahunan tercapai".to_string(), GREEN)
        } else if nxt.is_none() {
            (format!("Belum tercapai (sisa {})", num(sisa)), RED)
        } else if rs >= sisa {
            ("Rencana mencukupi".to_string(), GREEN)
        } else {
            let per = (sisa / sisa_bulan).ceil() as i64;
            saran.push(format!(
                "RHK {no} ({}): sisa {} {sat} dalam {} bulan, perlu ±{per} {sat}/bulan. Usulkan revisi rencana aksi atau percepatan penyelesaian.",
                judul_rhk(no, &r.rhk),
                num(sisa),
                11 - idx
            ));
            (format!("Kurang {} {sat} dari rencana", num(sisa - rs)), RED)
        };
        let fill = if (i + 1) % 2 == 1 { WHITE } else { ROW_ALT };
        let prog = if r.target != 0.0 { pct(Some(k / r.target)) } else { "–".to_string() };
        let depan = rencana_depan(r, idx);
        tb.push((
            th,
            vec![
                sel(format!("{}. {}", no, r.rhk), gaya(11).warna(DARK), fill),
                sel(format!("{} / {} ({})", num(k), num(r.target), prog), gaya(11).warna(DARK).rata(Rata::Tengah), fill),
                sel(format!("{} {}", num(sisa), sat), gaya(11).tebal().warna(DARK).rata(Rata::Tengah), fill),
                sel(
                    if nxt.is_some() { format!("{} {}", num(depan), sat) } else { "–".to_string() },
                    gaya(12).tebal().warna(NAVY).rata(Rata::Tengah),
                    fill,
                ),
                sel(
                    if nxt.is_some() { format!("{} {}", num(rs), sat) } else { "–".to_string() },
                    gaya(11).warna(DARK).rata(Rata::Tengah),
                    fill,
                ),
                sel(ket, gaya(11).tebal().warna(warna).rata(Rata::Tengah), fill),
            ],
        ));
    }
    d.tabel(0.6, 1.45, &lebar, tb);

    if nxt.is_some() {
        let txt: Vec<String> = rows
            .iter()
            .filter(|r| rencana_depan(r, idx) > 0.0)
            .map(|r| format!("RHK {} {} {}", r.no, num(rencana_depan(r, idx)), r.satuan))
            .collect();
        if !txt.is_empty() {
            saran.insert(0, format!("Target {}: {}.", NAMA_BULAN[idx + 1], txt.join(", ")));
        }
    }
    saran.extend(c.strategi.iter().cloned());
    saran.push("Kumpulkan bukti dukung (dokumen, screenshot aplikasi, foto) segera setelah setiap kegiatan selesai.".to_string());

    d.kotak(0.6, 4.55, W - 1.2, 2.25, Some(WHITE), Some(GARIS), Bentuk::Sudut, 0.04);
    d.kotak(0.6, 4.7, 0.07, 1.95, Some(NAVY), None, Bentuk::Persegi, 0.0);
    d.teks(0.9, 4.65, 11.0, 0.35, "Strategi Pencapaian Target", gaya(14).tebal().warna(NAVY), Anchor::Atas);
    let body = saran
        .iter()
        .take(5)
        .map(|x| format!("•  {x}"))
        .collect::<Vec<_>>()
        .join("\n");
    d.teks(0.9, 5.05, W - 1.8, 1.7, &body, gaya(12).warna(DARK).spasi(4), Anchor::Atas);
}

fn slide_penutup(d: &mut Deck, c: &Konteks) {
    d.baru();
    d.latar(NAVY);
    d.kotak(-1.5, 3.2, 5.0, 5.0, Some(NAVY_D), None, Bentuk::Oval, 0.0);
    d.kotak(10.5, -1.8, 4.5, 4.5, Some(NAVY_D), None, Bentuk::Oval, 0.0);
    d.kotak(0.0, H - 0.15, W, 0.15, Some(ORANGE), None, Bentuk::Persegi, 0.0);
    d.teks(0.0, 2.6, W, 1.2, "Terima Kasih", gaya(54).tebal().warna(WHITE).rata(Rata::Tengah), Anchor::Atas);
    d.teks(
        0.0,
        3.8,
        W,
        0.5,
        &format!("{}  ·  {}", c.l.pegawai.nama, c.label),
        gaya(16).warna(NAVY_SOFT).rata(Rata::Tengah),
        Anchor::Atas,
    );
    d.teks(0.0, 4.3, W, 0.4, &c.l.instansi, gaya(13).tebal().warna(ORANGE).rata(Rata::Tengah), Anchor::Atas);
}

// ===================================================================================
// Paket OOXML
// ===================================================================================

fn rels_xml(lst: &[(&str, String)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (i, (jenis, target)) in lst.iter().enumerate() {
        s += &format!(r#"<Relationship Id="rId{}" Type="{}" Target="{}"/>"#, i + 1, jenis, xml(target));
    }
    s + "</Relationships>"
}

fn content_types(d: &Deck) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/>"#,
    );
    let mut over = |nama: String, tipe: &str| {
        s.push_str(&format!(r#"<Override PartName="{nama}" ContentType="{tipe}"/>"#));
    };
    over("/ppt/presentation.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml");
    over("/ppt/slideMasters/slideMaster1.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml");
    over("/ppt/slideLayouts/slideLayout1.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml");
    for i in 0..d.halaman.len() {
        over(format!("/ppt/slides/slide{}.xml", i + 1), "application/vnd.openxmlformats-officedocument.presentationml.slide+xml");
    }
    for i in 0..d.grafik.len() {
        over(format!("/ppt/charts/chart{}.xml", i + 1), "application/vnd.openxmlformats-officedocument.drawingml.chart+xml");
    }
    over("/ppt/theme/theme1.xml".into(), "application/vnd.openxmlformats-officedocument.theme+xml");
    over("/ppt/presProps.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml");
    over("/ppt/viewProps.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml");
    over("/ppt/tableStyles.xml".into(), "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml");
    over("/docProps/core.xml".into(), "application/vnd.openxmlformats-package.core-properties+xml");
    s + "</Types>"
}

fn presentation_xml(n: usize) -> String {
    let ids: String = (0..n)
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 2))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{SLIDE_CX}" cy="{SLIDE_CY}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    )
}

fn slide_master_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{GRUP}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"#
    )
}

fn slide_layout_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Kosong"><p:spTree>{GRUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    )
}

fn theme_xml() -> String {
    let skema = format!(
        r#"<a:clrScheme name="SKP"><a:dk1><a:srgbClr val="{DARK}"/></a:dk1><a:lt1><a:srgbClr val="{WHITE}"/></a:lt1><a:dk2><a:srgbClr val="{NAVY}"/></a:dk2><a:lt2><a:srgbClr val="{BG}"/></a:lt2><a:accent1><a:srgbClr val="{NAVY}"/></a:accent1><a:accent2><a:srgbClr val="{ORANGE}"/></a:accent2><a:accent3><a:srgbClr val="{GREEN}"/></a:accent3><a:accent4><a:srgbClr val="{RED}"/></a:accent4><a:accent5><a:srgbClr val="{GREY}"/></a:accent5><a:accent6><a:srgbClr val="{NAVY_SOFT}"/></a:accent6><a:hlink><a:srgbClr val="{NAVY}"/></a:hlink><a:folHlink><a:srgbClr val="{GREY}"/></a:folHlink></a:clrScheme>"#
    );
    let isi = format!(
        r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#
    );
    let ln = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="SKP"><a:themeElements>{skema}<a:fontScheme name="SKP"><a:majorFont><a:latin typeface="{FONT}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="{FONT}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="SKP"><a:fillStyleLst>{isi}{isi}{isi}</a:fillStyleLst><a:lnStyleLst>{ln}{ln}{ln}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{isi}{isi}{isi}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

fn slide_xml(h: &Halaman) -> String {
    let bg = match h.bg {
        Some(c) => format!(r#"<p:bg><p:bgPr><a:solidFill><a:srgbClr val="{c}"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>"#),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NS}><p:cSld>{bg}<p:spTree>{GRUP}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
        h.body
    )
}

fn paket(d: &Deck) -> Result<Vec<u8>, String> {
    let n = d.halaman.len();
    let mut bagian: Vec<(String, Vec<u8>)> = Vec::new();
    let mut tambah = |nama: String, isi: Vec<u8>| bagian.push((nama, isi));

    tambah("[Content_Types].xml".into(), content_types(d).into_bytes());
    tambah(
        "_rels/.rels".into(),
        rels_xml(&[(REL_OFFICE, "ppt/presentation.xml".into()), (REL_CORE, "docProps/core.xml".into())]).into_bytes(),
    );
    tambah(
        "docProps/core.xml".into(),
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Laporan Capaian SKP</dc:title></cp:coreProperties>"#.to_vec(),
    );

    // Presentation: rId1 master, rId2..n+1 slide, lalu theme, presProps, viewProps, tableStyles.
    let mut rels_pres: Vec<(&str, String)> = vec![(REL_MASTER, "slideMasters/slideMaster1.xml".into())];
    for i in 0..n {
        rels_pres.push((REL_SLIDE, format!("slides/slide{}.xml", i + 1)));
    }
    rels_pres.push((REL_THEME, "theme/theme1.xml".into()));
    rels_pres.push((REL_PRESPROPS, "presProps.xml".into()));
    rels_pres.push((REL_VIEWPROPS, "viewProps.xml".into()));
    rels_pres.push((REL_TABLESTYLES, "tableStyles.xml".into()));
    tambah("ppt/presentation.xml".into(), presentation_xml(n).into_bytes());
    tambah("ppt/_rels/presentation.xml.rels".into(), rels_xml(&rels_pres).into_bytes());
    tambah(
        "ppt/presProps.xml".into(),
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentationPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#.to_vec(),
    );
    tambah(
        "ppt/viewProps.xml".into(),
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:viewPr xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:gridSpacing cx="72008" cy="72008"/></p:viewPr>"#.to_vec(),
    );
    tambah(
        "ppt/tableStyles.xml".into(),
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" def="{TABLE_STYLE}"/>"#).into_bytes(),
    );
    tambah("ppt/theme/theme1.xml".into(), theme_xml().into_bytes());
    tambah("ppt/slideMasters/slideMaster1.xml".into(), slide_master_xml().into_bytes());
    tambah(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),
        rels_xml(&[(REL_LAYOUT, "../slideLayouts/slideLayout1.xml".into()), (REL_THEME, "../theme/theme1.xml".into())]).into_bytes(),
    );
    tambah("ppt/slideLayouts/slideLayout1.xml".into(), slide_layout_xml().into_bytes());
    tambah(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),
        rels_xml(&[(REL_MASTER, "../slideMasters/slideMaster1.xml".into())]).into_bytes(),
    );

    for (i, h) in d.halaman.iter().enumerate() {
        tambah(format!("ppt/slides/slide{}.xml", i + 1), slide_xml(h).into_bytes());
        let mut lst: Vec<(&str, String)> = vec![(REL_LAYOUT, "../slideLayouts/slideLayout1.xml".into())];
        lst.extend(h.rels.iter().cloned());
        tambah(format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), rels_xml(&lst).into_bytes());
    }
    for (i, g) in d.grafik.iter().enumerate() {
        tambah(format!("ppt/charts/chart{}.xml", i + 1), g.clone().into_bytes());
    }
    for (nama, bytes) in &d.media {
        tambah(format!("ppt/media/{nama}"), bytes.clone());
    }

    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    for (nama, isi) in &bagian {
        zw.start_file(nama.as_str(), SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))
            .map_err(|e| format!("gagal menulis {nama}: {e}"))?;
        zw.write_all(isi).map_err(|e| format!("gagal menulis {nama}: {e}"))?;
    }
    let cur = zw.finish().map_err(|e| format!("gagal menutup pptx: {e}"))?;
    Ok(cur.into_inner())
}

/// Susun deck lengkap dan kembalikan isi berkas `.pptx`.
///
/// Urutan slide sama dengan `generate_skp.py`: cover, profil, ringkasan, tren, rhk, bukti,
/// jurnal (bisa lebih dari satu halaman), galeri (dilewati bila tidak ada foto), perilaku,
/// rencana, penutup.
pub fn build_pptx(laporan: &Laporan) -> Result<Vec<u8>, String> {
    if !(1..=12).contains(&laporan.bulan) {
        return Err(format!("bulan laporan harus 1..=12, bukan {}", laporan.bulan));
    }
    let c = Konteks::baru(laporan, (laporan.bulan - 1) as usize);
    let mut d = Deck::default();
    slide_cover(&mut d, &c);
    slide_profil(&mut d, &c);
    slide_ringkasan(&mut d, &c);
    slide_tren(&mut d, &c);
    slide_rhk(&mut d, &c);
    slide_bukti(&mut d, &c)?;
    slide_jurnal(&mut d, &c);
    slide_galeri(&mut d, &c)?;
    slide_perilaku(&mut d, &c);
    slide_rencana(&mut d, &c);
    slide_penutup(&mut d, &c);
    paket(&d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orang(nama: &str) -> Orang {
        Orang {
            nama: nama.to_string(),
            nip: "199702222025211052".to_string(),
            pangkat: "-".to_string(),
            jabatan: "Penata Layanan Operasional".to_string(),
            unit: "Bidang Air Minum dan Sanitasi".to_string(),
        }
    }

    /// RHK dengan rencana tetap tiap bulan, dan realisasi di Januari..September (idx 0..=8).
    fn rhk_uji(no: i64, rencana: f64, realisasi: f64, bukti: Vec<Gambar>) -> RhkLaporan {
        let mut ren = [0.0; 12];
        let mut rea = [0.0; 12];
        for i in 0..12 {
            ren[i] = rencana;
        }
        for i in 0..=8 {
            rea[i] = realisasi;
        }
        RhkLaporan {
            no,
            rhk: format!("Uraian RHK nomor {no}"),
            satuan: "dokumen".to_string(),
            target: 120.0,
            rencana_per_bulan: ren,
            realisasi_per_bulan: rea,
            bukti,
        }
    }

    fn jurnal_uji(tgl: (i32, u32, u32), foto: Vec<Gambar>) -> JurnalLaporan {
        JurnalLaporan {
            tgl: NaiveDate::from_ymd_opt(tgl.0, tgl.1, tgl.2).unwrap(),
            rhk: Some(1),
            kegiatan: "Menyusun & mengecek \"spesifikasi\" <teknis> 'A'\u{1}".to_string(),
            output: Some(2.0),
            satuan: "dokumen".to_string(),
            keterangan: "Bukti: <lampiran> & surat".to_string(),
            foto,
        }
    }

    fn laporan_uji(bukti: Vec<Gambar>, jurnal: Vec<JurnalLaporan>) -> Laporan {
        Laporan {
            instansi: "DISPERKIM KABUPATEN CIANJUR".to_string(),
            tahun: 2026,
            bulan: 9,
            pegawai: orang("ILHAM TAUFIQ, S.T."),
            atasan: orang("AGUNG DELI SAHPUTRA, ST"),
            rhk: vec![rhk_uji(1, 4.0, 3.0, bukti), rhk_uji(2, 0.0, 0.0, vec![])],
            jurnal,
            feedback: "Kinerja baik & cepat, <terus> dipertahankan".to_string(),
            strategi: vec!["Koordinasi rutin dengan PPK".to_string()],
        }
    }

    fn png_uji(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([30, 90, 160]));
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    fn gambar_png(w: u32, h: u32) -> Gambar {
        Gambar { bytes: png_uji(w, h), png: true }
    }

    fn baca_bagian(pptx: &[u8]) -> Vec<(String, String)> {
        let mut arsip = zip::ZipArchive::new(Cursor::new(pptx)).unwrap();
        let mut hasil = Vec::new();
        for i in 0..arsip.len() {
            let mut berkas = arsip.by_index(i).unwrap();
            let nama = berkas.name().to_string();
            if nama.ends_with(".xml") || nama.ends_with(".rels") {
                let mut isi = String::new();
                std::io::Read::read_to_string(&mut berkas, &mut isi).unwrap();
                hasil.push((nama, isi));
            } else {
                hasil.push((nama, String::new()));
            }
        }
        hasil
    }

    fn jumlah_slide(bagian: &[(String, String)]) -> usize {
        bagian
            .iter()
            .filter(|(n, _)| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .count()
    }

    fn isi_slide<'a>(bagian: &'a [(String, String)], no: usize) -> &'a str {
        let nama = format!("ppt/slides/slide{no}.xml");
        &bagian.iter().find(|(n, _)| *n == nama).expect("slide ada").1
    }

    #[test]
    fn capaian_mengikuti_python() {
        assert_eq!(capaian(0.0, 0.0), None);
        assert_eq!(capaian(0.0, 5.0), Some(1.0));
        assert_eq!(capaian(4.0, 2.0), Some(0.5));
        assert_eq!(capaian(4.0, 0.0), Some(0.0));
        assert_eq!(capaian(2.0, 6.0), Some(3.0));
    }

    #[test]
    fn format_angka_dan_persen() {
        assert_eq!(num(3.0), "3");
        assert_eq!(num(2.5), "2,5");
        assert_eq!(pct(None), "–");
        assert_eq!(pct(Some(1.0)), "100%");
        assert_eq!(pct(Some(0.755)), "75,5%");
        assert_eq!(status(Some(0.8)).0, "Hampir tercapai");
    }

    #[test]
    fn build_tanpa_foto_dan_tanpa_galeri() {
        let laporan = laporan_uji(vec![], vec![jurnal_uji((2026, 9, 3), vec![])]);
        let pptx = build_pptx(&laporan).expect("build");
        let bagian = baca_bagian(&pptx);
        assert!(bagian.iter().any(|(n, _)| n == "ppt/presentation.xml"));
        // Tanpa foto, slide galeri dilewati sesuai Python: 10 slide.
        assert_eq!(jumlah_slide(&bagian), 10);
        let charts = bagian.iter().filter(|(n, _)| n.starts_with("ppt/charts/chart")).count();
        assert_eq!(charts, 2);
        assert!(!bagian.iter().any(|(n, _)| n.starts_with("ppt/media/")));
        assert!(isi_slide(&bagian, 1).contains("Laporan Capaian"));
        assert!(isi_slide(&bagian, 10).contains("Terima Kasih"));
        // Tanpa galeri, slide rencana jatuh di nomor 9; bulan 9 membahas target Oktober.
        assert!(isi_slide(&bagian, 9).contains("Target Oktober"));
    }

    #[test]
    fn build_dengan_foto_sebelas_slide() {
        let laporan = laporan_uji(
            vec![gambar_png(40, 30), gambar_png(60, 30)],
            vec![jurnal_uji((2026, 9, 3), vec![])],
        );
        let pptx = build_pptx(&laporan).expect("build");
        let bagian = baca_bagian(&pptx);
        assert_eq!(jumlah_slide(&bagian), 11);
        // Foto pertama untuk kartu bukti, foto kedua di lampiran galeri.
        assert!(bagian.iter().any(|(n, _)| n == "ppt/media/image1.png"));
        assert!(bagian.iter().any(|(n, _)| n == "ppt/media/image2.png"));
        assert!(isi_slide(&bagian, 6).contains("<p:pic>"));
        assert!(isi_slide(&bagian, 8).contains("Lampiran Dokumentasi"));
    }

    #[test]
    fn jurnal_berlipat_dan_urutan_slide() {
        let jurnal: Vec<JurnalLaporan> = (1..=9).map(|d| jurnal_uji((2026, 9, d), vec![])).collect();
        let pptx = build_pptx(&laporan_uji(vec![], jurnal)).expect("build");
        let bagian = baca_bagian(&pptx);
        // 9 entri = 2 halaman jurnal; tanpa foto galeri dilewati: 11 slide.
        assert_eq!(jumlah_slide(&bagian), 11);
        assert!(isi_slide(&bagian, 7).contains("Jurnal Kegiatan (1/2)"));
        assert!(isi_slide(&bagian, 8).contains("Jurnal Kegiatan (2/2)"));
        assert!(isi_slide(&bagian, 9).contains("BerAKHLAK"));
    }

    #[test]
    fn xml_selalu_well_formed_dan_teks_di_escape() {
        let pptx = build_pptx(&laporan_uji(
            vec![gambar_png(40, 30)],
            vec![jurnal_uji((2026, 9, 3), vec![gambar_png(20, 20)])],
        ))
        .expect("build");
        let bagian = baca_bagian(&pptx);
        for (nama, isi) in &bagian {
            if nama.ends_with(".xml") || nama.ends_with(".rels") {
                let mut reader = quick_xml::Reader::from_str(isi);
                loop {
                    match reader.read_event() {
                        Ok(quick_xml::events::Event::Eof) => break,
                        Ok(_) => {}
                        Err(e) => panic!("{nama} tidak well-formed: {e}"),
                    }
                }
            }
        }
        let jurnal_slide = isi_slide(&bagian, 7);
        assert!(jurnal_slide.contains("Menyusun &amp; mengecek &quot;spesifikasi&quot; &lt;teknis&gt;"));
        assert!(!jurnal_slide.contains('\u{1}'));
    }

    #[test]
    fn bulan_di_luar_rentang_ditolak() {
        let mut laporan = laporan_uji(vec![], vec![]);
        laporan.bulan = 13;
        assert!(build_pptx(&laporan).is_err());
        laporan.bulan = 0;
        assert!(build_pptx(&laporan).is_err());
    }

    /// Menulis contoh ke `SKP_PPTX_OUT` bila env itu di-set, untuk diperiksa dengan python-pptx.
    #[test]
    fn tulis_contoh_bila_diminta() {
        let Ok(path) = std::env::var("SKP_PPTX_OUT") else {
            return;
        };
        let laporan = laporan_uji(
            vec![gambar_png(640, 360), gambar_png(360, 640)],
            vec![jurnal_uji((2026, 9, 3), vec![gambar_png(200, 150)])],
        );
        let pptx = build_pptx(&laporan).expect("build");
        std::fs::write(path, pptx).expect("tulis berkas");
    }
}
