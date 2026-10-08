//! Data placeholder dokumen kontrak: port `KontrakDocumentDataBuilder::build` dan
//! `KontrakDocumentSettingsService`. Modul ini tidak menyentuh database; data dimuat di `kontrak_document`.
//!
//! Format mengikuti Laravel: uang `Rp. 1.000.000` (pembulatan setengah ke atas), tanggal `05 Januari 2026`,
//! dan `terbilang` dalam bahasa Indonesia. Nilai kosong dibiarkan kosong; penggantian ke `-` dilakukan
//! di `docx_template::fill`, sama dengan `null`/'' di Laravel.

use chrono::{Datelike, Days, NaiveDate};

use crate::kontrak::{AddendumRow, KontrakRow};

const MONTHS: [&str; 12] = [
    "Januari",
    "Februari",
    "Maret",
    "April",
    "Mei",
    "Juni",
    "Juli",
    "Agustus",
    "September",
    "Oktober",
    "November",
    "Desember",
];

const CHECKED: &str = "✓";
const UNCHECKED: &str = "";
const PAYMENT_CHECKED: &str = "☑";
const PAYMENT_UNCHECKED: &str = "☐";
const STATUS_DISETUJUI: &str = "disetujui";
const PERSEN_TAGIH: [i64; 4] = [100, 95, 5, 30];
const PEMBAYARAN_SLOTS: usize = 5;

/// Kunci query yang dipakai builder sendiri, tidak ikut ke loop override umum.
const RESERVED_OVERRIDES: [&str; 9] = [
    "persen_tagih",
    "pembayaran_lalu",
    "nilai_tagih",
    "nomor_jaminan_uang_muka",
    "tanggal_jaminan_uang_muka",
    "nomor_jaminan_pelaksanaan",
    "tanggal_jaminan_pelaksanaan",
    "tgl_jaminan_uang_muka",
    "tgl_jaminan_pelaksanaan",
];

#[derive(Debug, Clone, PartialEq)]
pub struct PekerjaanDoc {
    pub id: i64,
    pub nama_paket: Option<String>,
    pub pagu: Option<f64>,
    pub kode_rekening: Option<String>,
    pub nama_kecamatan: Option<String>,
    pub nama_desa: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KegiatanDoc {
    pub nama_program: Option<String>,
    pub nama_kegiatan: Option<String>,
    pub nama_sub_kegiatan: Option<String>,
    pub sub_bidang: Option<String>,
    pub tahun_anggaran: Option<i64>,
    pub sumber_dana: Option<String>,
    pub nama_pptk: Option<String>,
    pub nip_pptk: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PenyediaDoc {
    pub nama: Option<String>,
    pub direktur: Option<String>,
    pub alamat: Option<String>,
    pub bank: Option<String>,
    pub norek: Option<String>,
    pub npwp: Option<String>,
    pub no_akta: Option<String>,
    pub notaris: Option<String>,
    pub tanggal_akta: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegisterDoc {
    pub id: i64,
    pub code: Option<String>,
    pub name: Option<String>,
    pub nomor: Option<String>,
    pub tanggal: Option<NaiveDate>,
    pub nilai: Option<f64>,
}

/// Semua relasi yang dibutuhkan dokumen satu kontrak.
#[derive(Debug, Clone, PartialEq)]
pub struct DocCtx {
    pub kontrak: KontrakRow,
    /// Pekerjaan pertama pada pivot `kontrak_pekerjaan`.
    pub pekerjaan: Option<PekerjaanDoc>,
    /// Kegiatan milik pekerjaan (bukan kegiatan kontrak).
    pub kegiatan: Option<KegiatanDoc>,
    /// `sub_bidang` kegiatan kontrak (`id_kegiatan`), dipakai bila kegiatan pekerjaan tidak punya.
    pub kontrak_sub_bidang: Option<String>,
    pub penyedia: Option<PenyediaDoc>,
    pub registers: Vec<RegisterDoc>,
    /// Semua addendum kontrak, urut `addendum_ke`.
    pub addendums: Vec<AddendumRow>,
}

impl DocCtx {
    /// Addendum yang disetujui, urut `addendum_ke` (`approvedAddendums`).
    pub fn approved(&self) -> Vec<&AddendumRow> {
        self.addendums
            .iter()
            .filter(|a| a.status == STATUS_DISETUJUI)
            .collect()
    }

    /// Addendum disetujui terbaru (`latestApprovedAddendum`).
    pub fn latest_approved(&self) -> Option<&AddendumRow> {
        self.addendums
            .iter()
            .filter(|a| a.status == STATUS_DISETUJUI)
            .last()
    }

    /// Register pertama dengan kode tipe itu (`findRegisterByCode`, tanpa membedakan huruf besar).
    pub fn find_register(&self, code: &str) -> Option<&RegisterDoc> {
        self.registers.iter().find(|r| {
            r.code
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(code))
        })
    }

    /// `sub_bidang` untuk memilih template cover (`exportCover`).
    pub fn sub_bidang(&self) -> String {
        self.kegiatan
            .as_ref()
            .and_then(|k| k.sub_bidang.clone())
            .or_else(|| self.kontrak_sub_bidang.clone())
            .unwrap_or_default()
    }
}

/// Pengaturan dokumen dari `app_settings`, dengan default SPSE bila kosong.
#[derive(Debug, Clone, PartialEq)]
pub struct DocSettings {
    pub nama_ppk: String,
    pub nip_ppk: String,
    /// PPTK default (`kontrak_nama_pptk`/`kontrak_nip_pptk`, atau `-`).
    pub nama_pptk: String,
    pub nip_pptk: String,
    pub skpd: String,
    pub nomor_dpa: String,
    pub tanggal_dpa: String,
    pub masa_pemeliharaan_hari: i64,
    /// `sekaligus`, `termin`, atau `bulan`.
    pub cara_pembayaran: String,
}

/// Input override dari query `export-bap`. Setara `$request->all()` di Laravel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    pub flat: Vec<(String, String)>,
    /// Item `pembayaran_lalu[i][field]`, urut kemunculan pertama indeks (seperti `array_values`).
    pub pembayaran_lalu: Vec<Vec<(String, String)>>,
}

impl Overrides {
    pub fn from_pairs(pairs: Vec<(String, String)>) -> Self {
        let mut flat = Vec::new();
        // Setiap item: indeks asli (None untuk `[]`) dan pasangan field-nilai.
        let mut items: Vec<(Option<String>, Vec<(String, String)>)> = Vec::new();
        for (key, value) in pairs {
            let Some(rest) = key.strip_prefix("pembayaran_lalu[") else {
                flat.push((key, value));
                continue;
            };
            let Some((index, tail)) = rest.split_once(']') else {
                continue;
            };
            let Some(field) = tail.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
                continue;
            };
            let pos = if index.is_empty() {
                None
            } else {
                items.iter().position(|(i, _)| i.as_deref() == Some(index))
            };
            let pos = match pos {
                Some(p) => p,
                None => {
                    let idx = (!index.is_empty()).then(|| index.to_string());
                    items.push((idx, Vec::new()));
                    items.len() - 1
                }
            };
            items[pos].1.push((field.to_string(), value));
        }
        Self {
            flat,
            pembayaran_lalu: items.into_iter().map(|(_, fields)| fields).collect(),
        }
    }

    fn flat_get(&self, key: &str) -> Option<&str> {
        self.flat
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Kumpulan pasangan kunci-nilai berurutan. `set` menimpa nilai lama dan mempertahankan posisinya
/// (perilaku `array_merge` di Laravel).
#[derive(Default)]
struct Data(Vec<(String, String)>);

impl Data {
    fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }

    fn into_vec(self) -> Vec<(String, String)> {
        self.0
    }
}

/// Bentuk `(int) $value` PHP untuk string: awalan digit, selain itu 0.
fn php_int(raw: &str) -> i64 {
    let t = raw.trim_start();
    let bytes = t.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return 0;
    }
    t[..end].parse::<i64>().unwrap_or(0)
}

/// `is_numeric` PHP untuk string: angka dengan spasi di pinggir, tanpa hex atau `inf`/`nan`.
fn php_numeric(raw: &str) -> Option<f64> {
    let t = raw.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}'));
    if !t.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
    {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `number_format($v, 0, ',', '.')`: pembulatan setengah menjauhi nol, pemisah ribuan titik.
pub fn number_format(value: f64) -> String {
    let rounded = value.round();
    let digits = format!("{:.0}", rounded.abs());
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    if rounded < 0.0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn money(value: f64) -> String {
    format!("Rp. {}", number_format(value))
}

/// `translatedFormat('d F Y')` dengan locale `id`.
fn tanggal(date: NaiveDate) -> String {
    format!(
        "{:02} {} {}",
        date.day(),
        MONTHS[date.month0() as usize],
        date.year()
    )
}

fn date_or_dash(date: Option<NaiveDate>) -> String {
    date.map(tanggal).unwrap_or_else(|| "-".to_string())
}

/// `?: '-'`: nilai kosong, `0`, dan NULL menjadi `-`.
fn falsy_dash(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() && v != "0" => v.to_string(),
        _ => "-".to_string(),
    }
}

/// `Carbon::parse` untuk format yang umum di form: `Y-m-d` (dengan waktu opsional), `d-m-Y`, `d/m/Y`, `Y/m/d`.
fn parse_date(raw: &str) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    let head = trimmed.get(..10).unwrap_or(trimmed);
    ["%Y-%m-%d", "%d-%m-%Y", "%d/%m/%Y", "%Y/%m/%d"]
        .iter()
        .find_map(|format| NaiveDate::parse_from_str(head, format).ok())
}

/// `formatOverrideDate`: tanggal terformat, atau teks asli bila tidak bisa diurai.
fn format_override_date(raw: &str) -> String {
    parse_date(raw)
        .map(tanggal)
        .unwrap_or_else(|| raw.to_string())
}

fn non_empty_trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `terbilang` dalam bahasa Indonesia untuk bilangan bulat tak negatif. Di luar jangkauan (≥ 10^15) hasilnya kosong.
pub fn terbilang(angka: i64) -> String {
    const BACA: [&str; 12] = [
        "", "Satu", "Dua", "Tiga", "Empat", "Lima", "Enam", "Tujuh", "Delapan", "Sembilan",
        "Sepuluh", "Sebelas",
    ];
    let a = angka.saturating_abs();
    let raw = if a < 12 {
        BACA[a as usize].to_string()
    } else if a < 20 {
        format!("{} Belas", terbilang(a - 10))
    } else if a < 100 {
        format!("{} Puluh {}", terbilang(a / 10), terbilang(a % 10))
    } else if a < 200 {
        format!("Seratus {}", terbilang(a - 100))
    } else if a < 1000 {
        format!("{} Ratus {}", terbilang(a / 100), terbilang(a % 100))
    } else if a < 2000 {
        format!("Seribu {}", terbilang(a - 1000))
    } else if a < 1_000_000 {
        format!("{} Ribu {}", terbilang(a / 1000), terbilang(a % 1000))
    } else if a < 1_000_000_000 {
        format!(
            "{} Juta {}",
            terbilang(a / 1_000_000),
            terbilang(a % 1_000_000)
        )
    } else if a < 1_000_000_000_000 {
        format!(
            "{} Milyar {}",
            terbilang(a / 1_000_000_000),
            terbilang(a % 1_000_000_000)
        )
    } else if a < 1_000_000_000_000_000 {
        format!(
            "{} Trilyun {}",
            terbilang(a / 1_000_000_000_000),
            terbilang(a % 1_000_000_000_000)
        )
    } else {
        String::new()
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `terbilang` untuk nilai uang: bagian bulat dari nilai absolut.
fn terbilang_money(value: f64) -> String {
    terbilang(value.abs() as i64)
}

/// `formatMasaHariTerbilang`: `120 (Seratus Dua Puluh) Hari Kalender`, atau `-` bila tidak positif.
fn masa_hari_terbilang(hari: Option<i64>) -> String {
    match hari {
        Some(h) if h > 0 => format!("{h} ({}) Hari Kalender", terbilang(h)),
        _ => "-".to_string(),
    }
}

fn add_days(date: NaiveDate, days: i64) -> NaiveDate {
    if days >= 0 {
        date.checked_add_days(Days::new(days as u64))
            .unwrap_or(date)
    } else {
        date.checked_sub_days(Days::new(days.unsigned_abs()))
            .unwrap_or(date)
    }
}

/// Override uang generik dari query: angka diformat rupiah kecuali kunci bernama nomor, tanggal, dan sejenisnya.
fn override_value(key: &str, value: &str) -> String {
    let lower = key.to_lowercase();
    let is_money = ["nilai", "jumlah", "dpp", "ppn", "total", "tagihan"]
        .iter()
        .any(|k| lower.contains(k));
    let excluded = !is_money
        && [
            "nomor", "tgl", "tanggal", "tahun", "kode", "id", "rate", "hari", "persen",
        ]
        .iter()
        .any(|k| lower.contains(k));
    match php_numeric(value) {
        Some(number) if is_money || !excluded => money(number),
        _ => value.to_string(),
    }
}

/// `sumberDanaCheckboxData`: centang sumber dana dari teks bebas (APBD, APBN, DAK, ...).
fn sumber_dana_checkbox(d: &mut Data, sumber: Option<&str>) {
    let raw = sumber.unwrap_or("");
    d.set("sumber_dana", falsy_dash(Some(raw)));

    // strtoupper lalu `preg_replace('/[^A-Z0-9]+/', ' ')`: setiap rangkaian karakter lain menjadi satu spasi.
    let mut normalized = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_uppercase());
        } else if !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    let has = |needle: &str| normalized.contains(needle);

    let flags: [(&[&str], bool); 12] = [
        (
            &[
                "checkbox_apbd",
                "checkbox_ apbd",
                "check_apbd",
                "check_ apbd",
                "APBD_CHECK",
            ],
            has("APBD"),
        ),
        (
            &[
                "checkbox_apbn",
                "checkbox_ apbn",
                "check_apbn",
                "check_ apbn",
                "APBN_CHECK",
            ],
            has("APBN"),
        ),
        (&["checkbox_dak", "check_dak", "DAK_CHECK"], has("DAK")),
        (&["checkbox_dau", "check_dau", "DAU_CHECK"], has("DAU")),
        (&["checkbox_did", "check_did", "DID_CHECK"], has("DID")),
        (
            &[
                "checkbox_banprov",
                "checkbox_bantuan_provinsi",
                "check_banprov",
                "check_ banprov",
                "check_bantuan_provinsi",
                "BANPROV_CHECK",
                "BANTUAN_PROVINSI_CHECK",
            ],
            has("BANPROV") || has("BANTUAN PROVINSI"),
        ),
        (
            &["checkbox_dbh", "check_dbh", "DBH_CHECK"],
            has("DBH") && !has("DBHCT") && !has("PAJAK ROKOK") && !has("PROV"),
        ),
        (
            &["checkbox_silpa", "check_silpa", "SILPA_CHECK"],
            has("SILPA"),
        ),
        (
            &[
                "checkbox_dbh_pajak_rokok",
                "check_dbh_pajak_rokok",
                "DBH_PAJAK_ROKOK_CHECK",
            ],
            has("DBH PAJAK ROKOK") || has("PAJAK ROKOK"),
        ),
        (&["checkbox_pad", "check_pad", "PAD_CHECK"], has("PAD")),
        (
            &["checkbox_dbhct", "check_dbhct", "DBHCT_CHECK"],
            has("DBHCT"),
        ),
        (
            &["checkbox_dbh_prov", "check_dbh_prov", "DBH_PROV_CHECK"],
            has("DBH PROV") || has("DBH PROVINSI"),
        ),
    ];
    for (keys, on) in flags {
        for key in keys {
            d.set(*key, if on { CHECKED } else { UNCHECKED });
        }
    }
}

/// `caraPembayaranCheckboxData`: pilihan cara pembayaran dengan kotak centang.
fn cara_pembayaran_checkbox(d: &mut Data, selected: &str) {
    let mut label = selected.to_string();
    if let Some(first) = label.get(..1).map(str::to_uppercase) {
        label.replace_range(..1, &first);
    }
    d.set("cara_pembayaran", label);
    for (key, option) in [
        ("check_pembayaran_sekaligus", "sekaligus"),
        ("check_pembayaran_termin", "termin"),
        ("check_pembayaran_bulan", "bulan"),
    ] {
        d.set(
            key,
            if selected == option {
                PAYMENT_CHECKED
            } else {
                PAYMENT_UNCHECKED
            },
        );
    }
}

/// `addendumData`: nomor dan tanggal addendum yang disetujui, minimal 10 slot. Spasi ekstra pada kunci mengikuti Laravel.
fn addendum_data(d: &mut Data, approved: &[&AddendumRow]) {
    let max_slots = approved.len().max(10);
    for slot in 1..=max_slots {
        for prefix in [
            "nomor_addendum",
            "nomor _addendum",
            "tgl_addendum",
            "tgl _addendum",
            "tanggal_addendum",
            "tanggal _addendum",
        ] {
            d.set(format!("{prefix}{slot}"), "-");
        }
    }
    for (index, addendum) in approved.iter().enumerate() {
        let slot = index + 1;
        let nomor = falsy_dash(addendum.nomor_addendum.as_deref());
        let tanggal = date_or_dash(addendum.tanggal_addendum);
        for prefix in ["nomor_addendum", "nomor _addendum"] {
            d.set(format!("{prefix}{slot}"), nomor.clone());
        }
        for prefix in [
            "tgl_addendum",
            "tgl _addendum",
            "tanggal_addendum",
            "tanggal _addendum",
        ] {
            d.set(format!("{prefix}{slot}"), tanggal.clone());
        }
    }
}

/// `pembayaranLaluData`: lima slot pembayaran sebelumnya dari override.
fn pembayaran_lalu_data(d: &mut Data, items: &[Vec<(String, String)>]) {
    for slot in 1..=PEMBAYARAN_SLOTS {
        d.set(format!("pembayaran_lalu_{slot}_jenis"), "-");
        d.set(format!("pembayaran_lalu_{slot}_tanggal"), "-");
        d.set(format!("pembayaran_lalu_{slot}_nominal"), "Rp -");
    }
    for (index, fields) in items.iter().take(PEMBAYARAN_SLOTS).enumerate() {
        let slot = index + 1;
        let get = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let jenis = get("jenis").unwrap_or("").trim();
        d.set(
            format!("pembayaran_lalu_{slot}_jenis"),
            if jenis.is_empty() { "-" } else { jenis },
        );
        // `if ($tanggalRaw)`: kosong dan "0" dilewati, tanggal yang tidak bisa diurai dipakai apa adanya.
        if let Some(raw) = get("tanggal").filter(|v| !v.is_empty() && *v != "0") {
            let formatted = parse_date(raw)
                .map(tanggal)
                .unwrap_or_else(|| raw.to_string());
            d.set(format!("pembayaran_lalu_{slot}_tanggal"), formatted);
        }
        if let Some(number) = get("nominal").and_then(php_numeric) {
            d.set(format!("pembayaran_lalu_{slot}_nominal"), money(number));
        }
    }
}

/// Membangun seluruh placeholder dokumen (`KontrakDocumentDataBuilder::build`).
pub fn build(
    ctx: &DocCtx,
    settings: &DocSettings,
    overrides: &Overrides,
) -> Result<Vec<(String, String)>, String> {
    let pekerjaan = ctx
        .pekerjaan
        .as_ref()
        .ok_or_else(|| "Kontrak belum terhubung dengan pekerjaan.".to_string())?;
    let kontrak = &ctx.kontrak;
    let kegiatan = ctx.kegiatan.as_ref();
    let penyedia = ctx.penyedia.as_ref();
    let approved = ctx.approved();
    let latest = ctx.latest_approved();

    let bastp = ctx.find_register("BASTP");
    let jaminan_um = ctx.find_register("JAMINAN_UM");
    let jaminan_pel = ctx.find_register("JAMINAN_PEL");
    let jaminan_pem = ctx.find_register("JAMINAN_PEM");
    let bap = ctx.find_register("BAP");

    let nilai_efektif = latest
        .and_then(|a| a.nilai_kontrak_sesudah)
        .or(kontrak.nilai_kontrak)
        .unwrap_or(0.0);
    let selesai_efektif = latest
        .and_then(|a| a.tgl_selesai_sesudah)
        .or(kontrak.tgl_selesai);
    let masa_hari = kontrak
        .tgl_spmk
        .zip(selesai_efektif)
        .map(|(spmk, selesai)| (selesai - spmk).num_days());
    let lima_persen = (nilai_efektif * 0.05).round() as i64;
    let masa_pemeliharaan = settings.masa_pemeliharaan_hari;

    let persen_tagih = overrides
        .flat_get("persen_tagih")
        .map(php_int)
        .unwrap_or(100);
    let persen_tagih = if PERSEN_TAGIH.contains(&persen_tagih) {
        persen_tagih
    } else {
        100
    };
    let nilai_tagih = (nilai_efektif * persen_tagih as f64 / 100.0).round() as i64;

    // Pekerjaan kontrak dipakai untuk kecamatan, desa, dan kegiatan.
    let kecamatan_nama = pekerjaan
        .nama_kecamatan
        .clone()
        .unwrap_or_else(|| "-".to_string());
    let desa_nama = pekerjaan
        .nama_desa
        .clone()
        .unwrap_or_else(|| "-".to_string());
    let kecamatan = if kecamatan_nama.is_empty() {
        "-".to_string()
    } else {
        kecamatan_nama.clone()
    };
    let desa = if desa_nama.is_empty() {
        "-".to_string()
    } else {
        desa_nama
    };
    let kota = if kecamatan_nama.is_empty() || kecamatan_nama == "-" {
        "Cianjur".to_string()
    } else {
        kecamatan_nama
    };

    // PPTK: dari kegiatan bila terisi, selain itu default pengaturan.
    let kegiatan_pptk = kegiatan
        .and_then(|k| non_empty_trimmed(k.nama_pptk.as_deref()))
        .unwrap_or_else(|| settings.nama_pptk.clone());
    let kegiatan_nip_pptk = kegiatan
        .and_then(|k| non_empty_trimmed(k.nip_pptk.as_deref()))
        .unwrap_or_else(|| settings.nip_pptk.clone());

    let nama_penyedia = penyedia.and_then(|p| p.nama.clone()).unwrap_or_default();
    let direktur = penyedia
        .and_then(|p| p.direktur.clone())
        .unwrap_or_default();
    let bank = penyedia.and_then(|p| p.bank.clone()).unwrap_or_default();
    let norek = penyedia.and_then(|p| p.norek.clone()).unwrap_or_default();
    let alamat_penyedia = penyedia.and_then(|p| p.alamat.clone()).unwrap_or_default();
    let no_akta = penyedia.and_then(|p| p.no_akta.clone()).unwrap_or_default();
    let notaris = penyedia.and_then(|p| p.notaris.clone()).unwrap_or_default();
    let npwp = penyedia
        .and_then(|p| p.npwp.clone())
        .unwrap_or_else(|| "-".to_string());

    let nama_program = kegiatan
        .and_then(|k| k.nama_program.clone())
        .unwrap_or_default();
    let nama_kegiatan = kegiatan
        .and_then(|k| k.nama_kegiatan.clone())
        .unwrap_or_default();
    let nama_sub_kegiatan = kegiatan
        .and_then(|k| k.nama_sub_kegiatan.clone())
        .unwrap_or_default();
    let tahun = kegiatan
        .and_then(|k| k.tahun_anggaran)
        .map(|t| t.to_string())
        .unwrap_or_else(|| "-".to_string());

    let mut d = Data::default();
    let pagu = pekerjaan.pagu.unwrap_or(0.0);
    let nilai_kontrak = kontrak.nilai_kontrak.unwrap_or(0.0);
    let nilai_addendum = latest
        .and_then(|a| a.nilai_kontrak_sesudah)
        .filter(|v| *v != 0.0);

    d.set(
        "nama_paket",
        pekerjaan.nama_paket.clone().unwrap_or_default(),
    );
    d.set("pagu", money(pagu));
    d.set("pagu_terbilang", terbilang_money(pagu));
    d.set(
        "kode_rekening",
        pekerjaan.kode_rekening.clone().unwrap_or_default(),
    );
    d.set("kecamatan", kecamatan);
    d.set("desa", desa);
    d.set("nama_program", nama_program);
    d.set("nama_kegiatan", nama_kegiatan);
    d.set("sub_kegiatan", nama_sub_kegiatan.clone());
    d.set("nama_subkegiatan", nama_sub_kegiatan.clone());
    d.set("nama_sub_kegiatan", nama_sub_kegiatan);
    d.set("tahun", tahun);
    d.set("nilai_kontrak", money(nilai_kontrak));
    d.set("nilai_kontrak_efektif", money(nilai_efektif));
    d.set(
        "nilai_kontrak_addendum",
        nilai_addendum.map(money).unwrap_or_else(|| "-".to_string()),
    );
    d.set("nilai_kontrak_5persen", money(lima_persen as f64));
    d.set("persen_tagih", persen_tagih.to_string());
    d.set("nilai_tagih", money(nilai_tagih as f64));
    d.set("nilai_kontrak_terbilang", terbilang_money(nilai_kontrak));
    d.set("terbilang_nilai_kontrak", terbilang_money(nilai_kontrak));
    d.set("tgl_sppbj", date_or_dash(kontrak.tgl_sppbj));
    d.set("tgl_spk", date_or_dash(kontrak.tgl_spk));
    d.set("tgl_selesai", date_or_dash(selesai_efektif));
    d.set("tgl_spmk", date_or_dash(kontrak.tgl_spmk));
    d.set("tanggal_spk", date_or_dash(kontrak.tgl_spk));
    d.set("tanggal_mulai", date_or_dash(kontrak.tgl_spmk));
    d.set("tanggal_selesai", date_or_dash(selesai_efektif));
    d.set("nomor_sppbj", falsy_dash(kontrak.sppbj.as_deref()));
    d.set("nomor_spk", falsy_dash(kontrak.spk.as_deref()));
    d.set("nomor_spmk", falsy_dash(kontrak.spmk.as_deref()));
    d.set("kode_rup", falsy_dash(kontrak.kode_rup.as_deref()));
    d.set("kode_paket", falsy_dash(kontrak.kode_paket.as_deref()));
    d.set(
        "nomor_penawaran",
        falsy_dash(kontrak.nomor_penawaran.as_deref()),
    );
    d.set("tanggal_penawaran", date_or_dash(kontrak.tanggal_penawaran));
    d.set("nama_penyedia", nama_penyedia.clone());
    d.set("direktur", direktur.clone());
    d.set("nama_direktur", direktur);
    d.set("alamat_penyedia", alamat_penyedia);
    d.set("bank", bank.clone());
    d.set("bank_penyedia", bank);
    d.set("norek", norek.clone());
    d.set("rekening_penyedia", norek);
    d.set("npwp_penyedia", npwp);
    d.set("no_akta", no_akta);
    d.set("notaris", notaris);
    d.set(
        "tanggal_akta",
        date_or_dash(penyedia.and_then(|p| p.tanggal_akta)),
    );
    d.set("nama_ppk", settings.nama_ppk.clone());
    d.set("nip_ppk", settings.nip_ppk.clone());
    d.set("nama_pptk", kegiatan_pptk);
    d.set("nip_pptk", kegiatan_nip_pptk);
    d.set("skpd", settings.skpd.clone());
    d.set("nomor_dpa", settings.nomor_dpa.clone());
    d.set("tanggal_dpa", settings.tanggal_dpa.clone());
    d.set(
        "nomor_bastp",
        falsy_dash(bastp.and_then(|r| r.nomor.as_deref())),
    );
    d.set("tgl_bastp", date_or_dash(bastp.and_then(|r| r.tanggal)));
    let um_nomor = falsy_dash(jaminan_um.and_then(|r| r.nomor.as_deref()));
    let um_tanggal = date_or_dash(jaminan_um.and_then(|r| r.tanggal));
    d.set("nomor_jaminan_uang_muka", um_nomor);
    d.set("tanggal_jaminan_uang_muka", um_tanggal.clone());
    d.set("tgl_jaminan_uang_muka", um_tanggal);
    let pel_nomor = falsy_dash(jaminan_pel.and_then(|r| r.nomor.as_deref()));
    let pel_tanggal = date_or_dash(jaminan_pel.and_then(|r| r.tanggal));
    d.set("nomor_jaminan_pelaksanaan", pel_nomor);
    d.set("tanggal_jaminan_pelaksanaan", pel_tanggal.clone());
    d.set("tgl_jaminan_pelaksanaan", pel_tanggal);
    let pem_nomor = falsy_dash(jaminan_pem.and_then(|r| r.nomor.as_deref()));
    let pem_tanggal = date_or_dash(jaminan_pem.and_then(|r| r.tanggal));
    d.set("nomor_jaminan_pemeliharaan", pem_nomor);
    d.set("tanggal_jaminan_pemeliharaan", pem_tanggal.clone());
    d.set("tgl_jaminan_pemeliharaan", pem_tanggal);
    d.set(
        "nomor_bap",
        falsy_dash(bap.and_then(|r| r.nomor.as_deref())),
    );
    d.set("tgl_bap", date_or_dash(bap.and_then(|r| r.tanggal)));
    d.set(
        "masa_hari",
        masa_hari
            .map(|h| h.to_string())
            .unwrap_or_else(|| "-".to_string()),
    );
    d.set(
        "masa",
        masa_hari
            .map(|h| format!("{h} Hari"))
            .unwrap_or_else(|| "-".to_string()),
    );
    d.set("masa_hari_terbilang", masa_hari_terbilang(masa_hari));
    d.set(
        "masa_hari_addendum",
        masa_hari_addendum(kontrak.tgl_spmk, latest.and_then(|a| a.tgl_selesai_sesudah)),
    );
    d.set(
        "jangka_pemeliharaan",
        masa_hari_terbilang(Some(masa_pemeliharaan)),
    );
    d.set(
        "mulai_selesai_pemeliharaan",
        mulai_selesai_pemeliharaan(selesai_efektif, masa_pemeliharaan),
    );
    d.set(
        "masa_pemeliharaan",
        masa_pemeliharaan_dari_bastp(bastp.and_then(|r| r.tanggal), masa_pemeliharaan),
    );

    // Alias untuk placeholder lama di template.
    let alias_pairs = {
        let get = |key: &str| {
            d.0.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        vec![
            ("Pekerjaan", get("nama_paket")),
            ("Penyedia", get("nama_penyedia")),
            ("Nama_SubKegiatan", get("nama_subkegiatan")),
            ("Nama_Sub_Kegiatan", get("nama_sub_kegiatan")),
            ("Nilai_Kontrak", get("nilai_kontrak")),
            ("Terbilang", get("nilai_kontrak_terbilang")),
            ("Kota", kota),
            ("SPK", get("tgl_spk")),
            ("SPK1", get("nomor_spk")),
            ("SPPBJ", get("tgl_sppbj")),
            ("SPPBJ1", get("nomor_sppbj")),
            ("Masa", get("masa_hari")),
            ("Selesai", get("tgl_selesai")),
            ("tgl_spl", get("tgl_spk")),
        ]
    };
    for (key, value) in alias_pairs {
        d.set(key, value);
    }

    cara_pembayaran_checkbox(&mut d, &settings.cara_pembayaran);
    sumber_dana_checkbox(&mut d, kegiatan.and_then(|k| k.sumber_dana.as_deref()));
    addendum_data(&mut d, &approved);
    pembayaran_lalu_data(&mut d, &overrides.pembayaran_lalu);

    // Override jaminan dari modal ringkasan (hanya yang tidak kosong).
    if let Some(nomor) = non_empty_trimmed(overrides.flat_get("nomor_jaminan_uang_muka")) {
        d.set("nomor_jaminan_uang_muka", nomor);
    }
    if let Some(tanggal_um) = non_empty_trimmed(overrides.flat_get("tanggal_jaminan_uang_muka")) {
        let formatted = format_override_date(&tanggal_um);
        d.set("tanggal_jaminan_uang_muka", formatted.clone());
        d.set("tgl_jaminan_uang_muka", formatted);
    }
    if let Some(nomor) = non_empty_trimmed(overrides.flat_get("nomor_jaminan_pelaksanaan")) {
        d.set("nomor_jaminan_pelaksanaan", nomor);
    }
    if let Some(tanggal_pel) = non_empty_trimmed(overrides.flat_get("tanggal_jaminan_pelaksanaan"))
    {
        let formatted = format_override_date(&tanggal_pel);
        d.set("tanggal_jaminan_pelaksanaan", formatted.clone());
        d.set("tgl_jaminan_pelaksanaan", formatted);
    }

    // Override umum dari query: nilai yang tidak dikenali tetap sebagai teks, tanpa placeholder khusus.
    for (key, value) in &overrides.flat {
        if RESERVED_OVERRIDES.contains(&key.as_str()) {
            continue;
        }
        d.set(key.clone(), override_value(key, value));
    }

    // Nilai tagih dan persen tidak boleh tertimpa format override umum.
    d.set("persen_tagih", persen_tagih.to_string());
    d.set("nilai_tagih", money(nilai_tagih as f64));

    Ok(d.into_vec())
}

/// `formatAddendumMasaHari`: masa kontrak sesudah addendum, dihitung dari SPMK.
fn masa_hari_addendum(spmk: Option<NaiveDate>, selesai_sesudah: Option<NaiveDate>) -> String {
    match (spmk, selesai_sesudah) {
        (Some(spmk), Some(selesai)) => masa_hari_terbilang(Some((selesai - spmk).num_days())),
        _ => "-".to_string(),
    }
}

/// `formatMasaPemeliharaan`: masa pemeliharaan mulai sehari setelah tanggal selesai efektif.
fn mulai_selesai_pemeliharaan(selesai: Option<NaiveDate>, masa: i64) -> String {
    match selesai {
        Some(selesai) => {
            let mulai = add_days(selesai, 1);
            let akhir = add_days(mulai, masa - 1);
            format!(
                "dari Tanggal {} s.d Tanggal {}",
                tanggal(mulai),
                tanggal(akhir)
            )
        }
        None => "-".to_string(),
    }
}

/// `formatMasaPemeliharaanDariBastp`: masa pemeliharaan dihitung dari tanggal BASTP.
fn masa_pemeliharaan_dari_bastp(bastp: Option<NaiveDate>, masa: i64) -> String {
    match bastp {
        Some(mulai) => format!(
            "{} sampai dengan {}",
            tanggal(mulai),
            tanggal(add_days(mulai, masa))
        ),
        None => "-".to_string(),
    }
}

/// Nilai mentah dari `app_settings` (`None` bila kunci tidak ada).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawDocSettings {
    pub nama_ppk: Option<String>,
    pub nip_ppk: Option<String>,
    pub nama_pptk: Option<String>,
    pub nip_pptk: Option<String>,
    pub skpd: Option<String>,
    pub nomor_dpa: Option<String>,
    pub tanggal_dpa: Option<String>,
    pub masa_pemeliharaan_hari: Option<String>,
    pub cara_pembayaran: Option<String>,
}

impl DocSettings {
    /// `KontrakDocumentSettingsService`: nilai kosong memakai default. `default_ppk` dan `default_cara`
    /// berasal dari konfigurasi SPSE (`SPSE_PPK_NAMA`, `SPSE_PPK_NIP`, `SPSE_CARA_PEMBAYARAN`).
    pub fn from_raw(
        raw: RawDocSettings,
        default_ppk: (String, String),
        default_cara: &str,
    ) -> Self {
        let or_default = |value: Option<String>, fallback: &str| {
            non_empty_trimmed(value.as_deref()).unwrap_or_else(|| fallback.to_string())
        };

        // `(int) ($configured ?: 180)`: nilai falsy (kosong atau "0") memakai 180; teks non-angka menjadi 0.
        let masa = match raw.masa_pemeliharaan_hari.as_deref() {
            Some(v) if !v.is_empty() && v != "0" => php_int(v),
            _ => 180,
        }
        .max(1);

        let cara_setting = raw
            .cara_pembayaran
            .as_deref()
            .map(|v| v.trim().to_lowercase())
            .unwrap_or_default();
        let cara = if matches!(cara_setting.as_str(), "sekaligus" | "termin" | "bulan") {
            cara_setting
        } else {
            match default_cara.trim().to_lowercase().as_str() {
                "termin" => "termin".to_string(),
                "bulan" => "bulan".to_string(),
                _ => "sekaligus".to_string(),
            }
        };

        Self {
            nama_ppk: or_default(raw.nama_ppk, &default_ppk.0),
            nip_ppk: or_default(raw.nip_ppk, &default_ppk.1),
            nama_pptk: or_default(raw.nama_pptk, "-"),
            nip_pptk: or_default(raw.nip_pptk, "-"),
            skpd: or_default(raw.skpd, "Dinas Perumahan dan Kawasan Permukiman"),
            nomor_dpa: or_default(raw.nomor_dpa, "-"),
            tanggal_dpa: or_default(raw.tanggal_dpa, "-"),
            masa_pemeliharaan_hari: masa,
            cara_pembayaran: cara,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terbilang_matches_laravel_rules() {
        assert_eq!(terbilang(0), "");
        assert_eq!(terbilang(11), "Sebelas");
        assert_eq!(terbilang(15), "Lima Belas");
        assert_eq!(terbilang(120), "Seratus Dua Puluh");
        assert_eq!(terbilang(2000), "Dua Ribu");
        assert_eq!(terbilang(1_500_000), "Satu Juta Lima Ratus Ribu");
        assert_eq!(terbilang(1_000_000_000_000_000), "");
    }

    #[test]
    fn number_format_rounds_half_away_from_zero() {
        assert_eq!(number_format(1234567.0), "1.234.567");
        assert_eq!(number_format(999.5), "1.000");
        assert_eq!(number_format(-1500.0), "-1.500");
        assert_eq!(number_format(0.0), "0");
        assert_eq!(money(0.0), "Rp. 0");
    }

    #[test]
    fn dates_use_indonesian_months_and_two_digit_day() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(tanggal(d), "05 Januari 2026");
        assert_eq!(
            parse_date("2026-08-17 10:00:00"),
            NaiveDate::from_ymd_opt(2026, 8, 17)
        );
        assert_eq!(format_override_date("bukan tanggal"), "bukan tanggal");
    }

    #[test]
    fn php_numeric_and_int_follow_php_casts() {
        assert_eq!(php_int(" 95abc"), 95);
        assert_eq!(php_int("abc"), 0);
        assert_eq!(php_numeric(" 1.5 "), Some(1.5));
        assert_eq!(php_numeric("12a"), None);
        assert_eq!(php_numeric("inf"), None);
    }

    #[test]
    fn overrides_group_pembayaran_items_by_first_index() {
        let o = Overrides::from_pairs(vec![
            ("pembayaran_lalu[1][jenis]".into(), "Uang Muka".into()),
            ("pembayaran_lalu[0][jenis]".into(), "Termin".into()),
            ("pembayaran_lalu[1][nominal]".into(), "100".into()),
            ("nilai_lain".into(), "5".into()),
        ]);
        assert_eq!(o.pembayaran_lalu.len(), 2);
        assert_eq!(
            o.pembayaran_lalu[0][0],
            ("jenis".to_string(), "Uang Muka".to_string())
        );
        assert_eq!(o.flat, vec![("nilai_lain".to_string(), "5".to_string())]);
    }

    #[test]
    fn money_override_only_for_money_like_keys() {
        assert_eq!(override_value("nilai_lain", "5000"), "Rp. 5.000");
        assert_eq!(override_value("nomor_surat", "123"), "123");
        assert_eq!(override_value("tahun", "2026"), "2026");
        assert_eq!(override_value("total_nilai", "5000"), "Rp. 5.000");
    }

    fn sample_ctx() -> DocCtx {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        DocCtx {
            kontrak: KontrakRow {
                id: 1,
                id_kegiatan: Some(1),
                id_pekerjaan: None,
                id_penyedia: Some(1),
                kode_rup: None,
                kode_paket: Some("KP-1".into()),
                nomor_penawaran: None,
                tanggal_penawaran: None,
                nilai_kontrak: Some(1_000_000.0),
                tgl_sppbj: None,
                tgl_spk: date(2026, 1, 10),
                tgl_spmk: date(2026, 1, 12),
                tgl_selesai: date(2026, 4, 11),
                sppbj: None,
                spk: Some("SPK/1".into()),
                spmk: None,
                spse_sppbj_id: None,
                spse_spk_id: None,
                spse_rekanan_id: None,
                spse_pushed_at: None,
                created_at: None,
                updated_at: None,
            },
            pekerjaan: Some(PekerjaanDoc {
                id: 7,
                nama_paket: Some("Pembangunan Jalan".into()),
                pagu: Some(1_500_000.0),
                kode_rekening: None,
                nama_kecamatan: Some("Cianjur".into()),
                nama_desa: None,
            }),
            kegiatan: Some(KegiatanDoc {
                nama_program: Some("Program A".into()),
                nama_kegiatan: None,
                nama_sub_kegiatan: Some("Sub A".into()),
                sub_bidang: Some("Air Minum".into()),
                tahun_anggaran: Some(2026),
                sumber_dana: Some("APBD, DAK".into()),
                nama_pptk: None,
                nip_pptk: None,
            }),
            kontrak_sub_bidang: None,
            penyedia: Some(PenyediaDoc {
                nama: Some("CV Maju".into()),
                direktur: Some("Budi".into()),
                alamat: None,
                bank: None,
                norek: None,
                npwp: None,
                no_akta: None,
                notaris: None,
                tanggal_akta: None,
            }),
            registers: vec![RegisterDoc {
                id: 1,
                code: Some("BASTP".into()),
                name: None,
                nomor: Some("BA-1".into()),
                tanggal: date(2026, 1, 20),
                nilai: None,
            }],
            addendums: Vec::new(),
        }
    }

    fn sample_settings() -> DocSettings {
        DocSettings {
            nama_ppk: "PPK Uji".into(),
            nip_ppk: "1234".into(),
            nama_pptk: "-".into(),
            nip_pptk: "-".into(),
            skpd: "SKPD Uji".into(),
            nomor_dpa: "-".into(),
            tanggal_dpa: "-".into(),
            masa_pemeliharaan_hari: 180,
            cara_pembayaran: "termin".into(),
        }
    }

    fn value<'a>(data: &'a [(String, String)], key: &str) -> &'a str {
        data.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("kunci {key} tidak ada"))
    }

    #[test]
    fn build_formats_contract_placeholders_like_laravel() {
        let data = build(&sample_ctx(), &sample_settings(), &Overrides::default()).unwrap();
        assert_eq!(value(&data, "nama_paket"), "Pembangunan Jalan");
        assert_eq!(value(&data, "pagu"), "Rp. 1.500.000");
        assert_eq!(value(&data, "nilai_kontrak"), "Rp. 1.000.000");
        assert_eq!(value(&data, "nilai_kontrak_terbilang"), "Satu Juta");
        assert_eq!(value(&data, "nilai_kontrak_5persen"), "Rp. 50.000");
        assert_eq!(value(&data, "nilai_tagih"), "Rp. 1.000.000");
        assert_eq!(value(&data, "tgl_spk"), "10 Januari 2026");
        assert_eq!(value(&data, "tgl_selesai"), "11 April 2026");
        assert_eq!(value(&data, "masa_hari"), "89");
        assert_eq!(
            value(&data, "masa_hari_terbilang"),
            "89 (Delapan Puluh Sembilan) Hari Kalender"
        );
        assert_eq!(value(&data, "Kota"), "Cianjur");
        assert_eq!(value(&data, "nomor_bastp"), "BA-1");
        assert_eq!(value(&data, "tgl_bastp"), "20 Januari 2026");
        assert_eq!(
            value(&data, "masa_pemeliharaan"),
            "20 Januari 2026 sampai dengan 19 Juli 2026"
        );
        assert_eq!(value(&data, "nama_ppk"), "PPK Uji");
        assert_eq!(value(&data, "nama_pptk"), "-");
        assert_eq!(value(&data, "nomor_addendum1"), "-");
        assert_eq!(value(&data, "tanggal _addendum10"), "-");
        assert_eq!(value(&data, "cara_pembayaran"), "Termin");
        assert_eq!(value(&data, "check_pembayaran_termin"), "☑");
        assert_eq!(value(&data, "check_pembayaran_sekaligus"), "☐");
        assert_eq!(value(&data, "APBD_CHECK"), "✓");
        assert_eq!(value(&data, "DAK_CHECK"), "✓");
        assert_eq!(value(&data, "DAU_CHECK"), "");
        assert_eq!(value(&data, "pembayaran_lalu_1_nominal"), "Rp -");
        assert_eq!(value(&data, "Terbilang"), "Satu Juta");
    }

    #[test]
    fn build_applies_persen_tagih_and_pembayaran_overrides() {
        let overrides = Overrides::from_pairs(vec![
            ("persen_tagih".into(), "95".into()),
            ("pembayaran_lalu[0][jenis]".into(), "Uang Muka".into()),
            ("pembayaran_lalu[0][tanggal]".into(), "2026-02-01".into()),
            ("pembayaran_lalu[0][nominal]".into(), "100000".into()),
            ("nomor_jaminan_pelaksanaan".into(), "JP-9".into()),
            ("tanggal_jaminan_pelaksanaan".into(), "2026-03-05".into()),
            ("nilai_lain".into(), "5000".into()),
        ]);
        let data = build(&sample_ctx(), &sample_settings(), &overrides).unwrap();
        assert_eq!(value(&data, "persen_tagih"), "95");
        assert_eq!(value(&data, "nilai_tagih"), "Rp. 950.000");
        assert_eq!(value(&data, "pembayaran_lalu_1_jenis"), "Uang Muka");
        assert_eq!(
            value(&data, "pembayaran_lalu_1_tanggal"),
            "01 Februari 2026"
        );
        assert_eq!(value(&data, "pembayaran_lalu_1_nominal"), "Rp. 100.000");
        assert_eq!(value(&data, "nomor_jaminan_pelaksanaan"), "JP-9");
        assert_eq!(value(&data, "tgl_jaminan_pelaksanaan"), "05 Maret 2026");
        assert_eq!(value(&data, "nilai_lain"), "Rp. 5.000");
    }

    #[test]
    fn build_needs_a_pekerjaan() {
        let mut ctx = sample_ctx();
        ctx.pekerjaan = None;
        assert!(build(&ctx, &sample_settings(), &Overrides::default()).is_err());
    }

    #[test]
    fn doc_settings_fall_back_when_blank() {
        let settings = DocSettings::from_raw(
            RawDocSettings {
                nama_ppk: Some("  ".into()),
                masa_pemeliharaan_hari: Some("0".into()),
                cara_pembayaran: Some("lainnya".into()),
                ..Default::default()
            },
            ("PPK Default".into(), "999".into()),
            "Bulan",
        );
        assert_eq!(settings.nama_ppk, "PPK Default");
        assert_eq!(settings.masa_pemeliharaan_hari, 180);
        assert_eq!(settings.cara_pembayaran, "bulan");
        assert_eq!(settings.skpd, "Dinas Perumahan dan Kawasan Permukiman");
    }
}
