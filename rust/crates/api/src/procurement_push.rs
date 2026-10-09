//! Push kontrak ke SPSE: `POST /api/procurement/spse/kontrak/push`.
//!
//! Setara `SpseProcurementController@pushKontrak`, `SpseKontrakPushService` (SPPBJ, blacklist, SPK,
//! cara pembayaran, SPMK, verifikasi), `SpseKontrakHtmlParser`, dan `SpseKontrakFormatter`.
//! Urutan permintaan ke SPSE sama dengan Laravel.
//!
//! Perbedaan yang diketahui:
//! - Lock per kontrak hanya berlaku dalam satu proses. `Cache::lock` Laravel bisa lintas proses.
//! - Parser HTML memakai regex, bukan DOM. `form_fields` mengikuti urutan dan aturan `extractFormFields`
//!   untuk input, textarea, dan select. HTML yang rusak ditangani lebih lemah daripada `DOMDocument`.
//! - `SPSE_PPK_*` hanya dari env, tanpa nilai bawaan di kode. Laravel punya nilai bawaan di
//!   `config/services.php`. Rust menolak push (500, sebelum simpan pertama) bila salah satu belum di-set.
//! - Perubahan kontrak memakai `changes::log` (audit dan notifikasi admin). Cache `dashboard_stats_version`
//!   tidak dipindah (lihat `spam_import.rs`).

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    changes,
    kontrak::decode_entities,
    procurement_spse::{self as spse, regex_once, Resp, Session, SpseError, PAGE_ACCEPT},
    require_auth,
    validation::Errors,
    AppState,
};

type Fields = Vec<(String, String)>;

const EXPIRED_MSG: &str = "Session SPSE expired. Login ulang di SPSE lalu kirim cookie lagi.";
const PUSH_FAILED: &str = "Push kontrak ke SPSE gagal: ";
const LAMP: &str = "-";
const JAMINAN: &str = "0,00";
const MASA_JAMINAN: &str = "0";
const LINGKUP: &str = "<p>Sesuai Spesifikasi Teknis Pekerjaan</p>";
const JABATAN_WAKIL: &str = "Direktur";
const BANK_DEFAULT: &str = "BJB";
const NOREK_DEFAULT: &str = "0";
const KOTA_DEFAULT: &str = "Cianjur";
const ALAMAT_DEFAULT: &str = "Jl. Adi Sucipta No. 7 - Cianjur";
const CARA_BAYAR_DEFAULT: &str = "Sekaligus";
const BOUNDARY: &str = "----apiamis-spse-push-boundary";

// ---------------------------------------------------------------------------
// Galat dan lock
// ---------------------------------------------------------------------------

/// Galat alur push, dipetakan ke respons seperti `pushKontrak` di Laravel.
enum PushErr {
    /// `InvalidArgumentException`: 422.
    Invalid(String),
    /// `SpseSessionExpiredException` di luar langkah: 401, dan sesi dinonaktifkan.
    Expired,
    /// `RuntimeException` atau galat di dalam langkah: 500 dengan awalan `PUSH_FAILED`.
    Failed(String),
    /// Galat basis data atau internal: 500 tanpa detail ke klien.
    Server(ApiError),
}

impl From<SpseError> for PushErr {
    fn from(e: SpseError) -> Self {
        match e {
            SpseError::Expired => PushErr::Expired,
            SpseError::Failed(m) => PushErr::Failed(m),
        }
    }
}

impl PushErr {
    /// Pesan galat yang dipakai bila galat terjadi di dalam `runStep`.
    fn message(&self) -> String {
        match self {
            PushErr::Invalid(m) | PushErr::Failed(m) => m.clone(),
            PushErr::Expired => EXPIRED_MSG.to_string(),
            PushErr::Server(_) => "Server Error".to_string(),
        }
    }

    fn into_api(self) -> ApiError {
        match self {
            PushErr::Invalid(m) => ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, m),
            PushErr::Expired => ApiError::new(StatusCode::UNAUTHORIZED, EXPIRED_MSG),
            PushErr::Failed(m) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{PUSH_FAILED}{m}")),
            PushErr::Server(e) => e,
        }
    }
}

fn db(e: sqlx::Error) -> PushErr {
    PushErr::Server(spse::internal(e))
}

/// `runStep`: galat di dalam langkah dibungkus dan menjadi 500, termasuk sesi expired dan validasi.
fn wrap<T>(step: &str, r: Result<T, PushErr>) -> Result<T, PushErr> {
    r.map_err(|e| PushErr::Failed(format!("Langkah {step} gagal: {}", e.message())))
}

fn in_flight() -> &'static Mutex<HashSet<i64>> {
    static SET: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// `Cache::lock('spse-push:{id}')`: satu push per kontrak dalam satu proses.
struct PushLock(i64);

impl PushLock {
    fn acquire(id: i64) -> Option<Self> {
        let mut set = in_flight().lock().unwrap_or_else(|e| e.into_inner());
        // Jangan pakai then_some(Self(id)): Self dibuat walau insert gagal, lalu di-drop
        // saat guard masih dipegang, dan Drop mengunci mutex yang sama (deadlock).
        if set.insert(id) {
            Some(Self(id))
        } else {
            None
        }
    }
}

impl Drop for PushLock {
    fn drop(&mut self) {
        in_flight().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Data kontrak, konfigurasi, dan format
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Penyedia {
    nama: Option<String>,
    direktur: Option<String>,
    bank: Option<String>,
    norek: Option<String>,
}

#[derive(Clone)]
struct KontrakRow {
    id: i64,
    id_pekerjaan: Option<i64>,
    id_penyedia: Option<i64>,
    kode_paket: Option<String>,
    sppbj: Option<String>,
    spk: Option<String>,
    spmk: Option<String>,
    tgl_sppbj: Option<NaiveDate>,
    tgl_spk: Option<NaiveDate>,
    tgl_spmk: Option<NaiveDate>,
    tgl_selesai: Option<NaiveDate>,
    spse_sppbj_id: Option<String>,
    spse_spk_id: Option<String>,
    spse_rekanan_id: Option<String>,
    spse_pushed_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    penyedia: Option<Penyedia>,
}

const KONTRAK_SQL: &str = "SELECT CAST(k.id AS SIGNED) AS id, CAST(k.id_pekerjaan AS SIGNED) AS id_pekerjaan, \
    CAST(k.id_penyedia AS SIGNED) AS id_penyedia, k.kode_paket, k.sppbj, k.spk, k.spmk, \
    k.tgl_sppbj, k.tgl_spk, k.tgl_spmk, k.tgl_selesai, k.spse_sppbj_id, k.spse_spk_id, k.spse_rekanan_id, \
    k.spse_pushed_at, k.updated_at, CAST(p.id AS SIGNED) AS penyedia_id, p.nama AS penyedia_nama, \
    p.direktur AS penyedia_direktur, p.bank AS penyedia_bank, p.norek AS penyedia_norek \
    FROM tbl_kontrak k LEFT JOIN tbl_penyedia p ON p.id = k.id_penyedia WHERE k.id = ?";

async fn load_kontrak(pool: &MySqlPool, id: i64) -> Result<Option<KontrakRow>, sqlx::Error> {
    let Some(r) = sqlx::query(KONTRAK_SQL).bind(id).fetch_optional(pool).await? else {
        return Ok(None);
    };
    let penyedia = if r.try_get::<Option<i64>, _>("penyedia_id")?.is_some() {
        Some(Penyedia {
            nama: r.try_get("penyedia_nama")?,
            direktur: r.try_get("penyedia_direktur")?,
            bank: r.try_get("penyedia_bank")?,
            norek: r.try_get("penyedia_norek")?,
        })
    } else {
        None
    };
    Ok(Some(KontrakRow {
        id: r.try_get("id")?,
        id_pekerjaan: r.try_get("id_pekerjaan")?,
        id_penyedia: r.try_get("id_penyedia")?,
        kode_paket: r.try_get("kode_paket")?,
        sppbj: r.try_get("sppbj")?,
        spk: r.try_get("spk")?,
        spmk: r.try_get("spmk")?,
        tgl_sppbj: r.try_get("tgl_sppbj")?,
        tgl_spk: r.try_get("tgl_spk")?,
        tgl_spmk: r.try_get("tgl_spmk")?,
        tgl_selesai: r.try_get("tgl_selesai")?,
        spse_sppbj_id: r.try_get("spse_sppbj_id")?,
        spse_spk_id: r.try_get("spse_spk_id")?,
        spse_rekanan_id: r.try_get("spse_rekanan_id")?,
        spse_pushed_at: r.try_get("spse_pushed_at")?,
        updated_at: r.try_get("updated_at")?,
        penyedia,
    }))
}

/// Konfigurasi dari env. `SPSE_PPK_*` wajib (tanpa nilai bawaan). Sisanya memakai nilai bawaan Laravel.
struct Ppk {
    nama: String,
    nip: String,
    jabatan: String,
    no_sk: String,
}

struct Settings {
    kota: String,
    alamat: String,
    cara_bayar: String,
    ppk: Ppk,
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn settings() -> Result<Settings, PushErr> {
    let ppk_fields = [
        ("SPSE_PPK_NAMA", env_value("SPSE_PPK_NAMA")),
        ("SPSE_PPK_NIP", env_value("SPSE_PPK_NIP")),
        ("SPSE_PPK_JABATAN", env_value("SPSE_PPK_JABATAN")),
        ("SPSE_PPK_NO_SK", env_value("SPSE_PPK_NO_SK")),
    ];
    let missing: Vec<&str> = ppk_fields.iter().filter(|(_, v)| v.is_none()).map(|(n, _)| *n).collect();
    if !missing.is_empty() {
        return Err(PushErr::Failed(format!("konfigurasi {} belum di-set di server.", missing.join(", "))));
    }
    let [nama, nip, jabatan, no_sk] = ppk_fields.map(|(_, v)| v.unwrap_or_default());
    Ok(Settings {
        kota: env_value("SPSE_SATKER_KOTA").unwrap_or_else(|| KOTA_DEFAULT.to_string()),
        alamat: env_value("SPSE_SATKER_ALAMAT").unwrap_or_else(|| ALAMAT_DEFAULT.to_string()),
        cara_bayar: env_value("SPSE_CARA_PEMBAYARAN").unwrap_or_else(|| CARA_BAYAR_DEFAULT.to_string()),
        ppk: Ppk { nama, nip, jabatan, no_sk },
    })
}

/// `SpseKontrakFormatter::formatDate`: `d-m-Y`, atau kosong bila null.
fn format_date(d: Option<NaiveDate>) -> String {
    d.map(|d| d.format("%d-%m-%Y").to_string()).unwrap_or_default()
}

/// `waktuPenyelesaian`: selisih hari (bertanda) + 1, atau kosong bila salah satu tanggal null.
fn waktu_penyelesaian(mulai: Option<NaiveDate>, selesai: Option<NaiveDate>) -> String {
    match (mulai, selesai) {
        (Some(m), Some(s)) => format!("{} Hari Kalender", (s - m).num_days() + 1),
        _ => String::new(),
    }
}

/// `SpseKontrakFormatter::parseNilai`: titik ribuan dibuang, koma jadi titik desimal.
fn parse_nilai(raw: &str) -> Option<f64> {
    let t = raw.trim().replace('.', "").replace(',', ".");
    let t = t.trim();
    if !regex_once!(r"^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?$").is_match(t) {
        return None;
    }
    t.parse().ok()
}

/// `php_empty`: null, "", dan "0" dianggap kosong.
fn truthy(v: Option<&str>) -> bool {
    !spse::php_empty(v)
}

/// `$a ?: $b` untuk nilai opsional.
fn or_php(a: Option<String>, b: Option<String>) -> Option<String> {
    if truthy(a.as_deref()) {
        a
    } else {
        b
    }
}

fn is_digits(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
}

/// Path dan query dari URL redirect SPSE (`parse_url` path + query).
fn path_from_spse_url(url: &str) -> Option<String> {
    let rest = match url.find("://") {
        Some(i) => {
            let after = &url[i + 3..];
            &after[after.find('/')?..]
        }
        None => url,
    };
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let path = path.split('#').next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    Some(match query {
        Some(q) => format!("{path}?{}", q.split('#').next().unwrap_or("")),
        None => path.to_string(),
    })
}

/// Simpan nilai di posisi field yang sudah ada, atau tambahkan di akhir (seperti `array_merge`).
fn put_field(fields: &mut Fields, name: &str, value: String) {
    if let Some(i) = fields.iter().position(|e| e.0 == name) {
        fields[i].1 = value;
    } else {
        fields.push((name.to_string(), value));
    }
}

fn merge(base: Fields, overrides: Vec<(&str, String)>) -> Fields {
    let mut out = base;
    for (k, v) in overrides {
        put_field(&mut out, k, v);
    }
    out
}

fn form_get<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|e| e.0 == key).map(|e| e.1.as_str())
}

/// `direktur ?: nama ?? ''`, dipakai untuk wakil penyedia.
fn wakil(p: Option<&Penyedia>) -> String {
    match p {
        None => String::new(),
        Some(p) if truthy(p.direktur.as_deref()) => p.direktur.clone().unwrap_or_default(),
        Some(p) => p.nama.clone().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Parser HTML (SpseKontrakHtmlParser)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ListStatus {
    sppbj_id: Option<String>,
    spk_id: Option<String>,
    pesanan_id: Option<String>,
    sppbj_complete: bool,
    spk_complete: bool,
    sskk_complete: bool,
    spmk_complete: bool,
    all_complete: bool,
}

impl ListStatus {
    fn to_json(&self) -> Value {
        json!({
            "sppbj_id": self.sppbj_id,
            "spk_id": self.spk_id,
            "pesanan_id": self.pesanan_id,
            "sppbj_complete": self.sppbj_complete,
            "spk_complete": self.spk_complete,
            "sskk_complete": self.sskk_complete,
            "spmk_complete": self.spmk_complete,
            "all_complete": self.all_complete,
        })
    }
}

mod html {
    use regex::Regex;

    use super::{decode_entities, put_field, regex_once, truthy, Fields, ListStatus};
    use crate::procurement_spse::strip_tags;

    fn first_capture(html: &str, pattern: &str) -> Option<String> {
        Regex::new(pattern).ok()?.captures(html).map(|c| c[1].to_string())
    }

    /// `isValidSpseId`: tidak kosong dan bukan "0" setelah trim.
    pub(super) fn valid_id(v: Option<&str>) -> bool {
        v.map(str::trim).is_some_and(|t| !t.is_empty() && t != "0")
    }

    fn element_html(html: &str, tag: &str, id: &str) -> Option<String> {
        let pattern = format!(r#"(?is)<{tag}[^>]*id=["']{}["'][^>]*>.*?</{tag}>"#, regex::escape(id));
        Regex::new(&pattern).ok()?.find(html).map(|m| m.as_str().to_string())
    }

    pub(super) fn list_status(html: &str) -> ListStatus {
        let table = element_html(html, "table", "tblsppbj").unwrap_or_else(|| html.to_string());
        let sppbj_id = first_capture(&table, r"(?i)sppbjId=([0-9]+)")
            .or_else(|| first_capture(&table, r"(?i)simpancarapembayaran\?id=([0-9]+)"))
            .or_else(|| sppbj_id_from_html(&table));
        let spk_id = first_capture(&table, r"(?i)spkId=([0-9]+)");
        let pesanan_id = first_capture(&table, r"(?i)pesananId=([0-9]+)");

        let sppbj_complete = sppbj_id.is_some();
        let spk_complete = spk_id.is_some();
        let spmk_complete = pesanan_id.is_some()
            || Regex::new(r#"(?i)editspmknonpl\?[^"']*spkId=[0-9]+"#).is_ok_and(|re| re.is_match(&table));
        let sskk_complete = spmk_complete
            || regex_once!(r"(?i)sskk-pl/(?:cetak|lihat)").is_match(&table)
            || regex_once!(r"(?i)>\s*Sekaligus\s*<").is_match(&table);

        ListStatus {
            sppbj_id,
            spk_id,
            pesanan_id,
            sppbj_complete,
            spk_complete,
            sskk_complete,
            spmk_complete,
            all_complete: sppbj_complete && spk_complete && sskk_complete && spmk_complete,
        }
    }

    pub(super) fn sppbj_id_from_html(html: &str) -> Option<String> {
        const PATTERNS: [&str; 5] = [
            r#"(?i)name=["']sppbj\.sppbj_id["'][^>]*value=["']([0-9]+)["']"#,
            r#"(?i)value=["']([0-9]+)["'][^>]*name=["']sppbj\.sppbj_id["']"#,
            r#"(?i)sppbj-pl/sppbjppkpl\?[^"']*sppbjId=([0-9]+)"#,
            r#"(?i)spk-pl/spkpl\?sppbjId=([0-9]+)"#,
            r#"(?i)sppbjId=([0-9]+)"#,
        ];
        PATTERNS
            .iter()
            .find_map(|p| first_capture(html, p).filter(|id| valid_id(Some(id.as_str()))))
    }

    /// `extractQueryParam`: angka dari `?name=` atau `&name=`.
    pub(super) fn query_param(text: &str, name: &str) -> Option<String> {
        first_capture(text, &format!("(?i)[?&]{}=([0-9]+)", regex::escape(name)))
    }

    /// `extractInputValue`: nilai `value` dari `name`, dua urutan atribut. Kosong dianggap null.
    pub(super) fn hidden_value(html: &str, name: &str) -> Option<String> {
        let e = regex::escape(name);
        let forward = format!(r#"(?i)name=["']{e}["'][^>]*value=["']([^"']*)["']"#);
        let backward = format!(r#"(?i)value=["']([^"']*)["'][^>]*name=["']{e}["']"#);
        for pattern in [forward, backward] {
            if let Some(c) = Regex::new(&pattern).ok().and_then(|re| re.captures(html)) {
                let v = c[1].to_string();
                return if v.is_empty() { None } else { Some(v) };
            }
        }
        None
    }

    /// `extractInputById`: sama seperti `hidden_value` tetapi memakai `id=`.
    pub(super) fn input_by_id(html: &str, id: &str) -> Option<String> {
        let e = regex::escape(id);
        let forward = format!(r#"(?i)id=["']{e}["'][^>]*value=["']([^"']*)["']"#);
        let backward = format!(r#"(?i)value=["']([^"']*)["'][^>]*id=["']{e}["']"#);
        for pattern in [forward, backward] {
            if let Some(c) = Regex::new(&pattern).ok().and_then(|re| re.captures(html)) {
                let v = c[1].to_string();
                return if v.is_empty() { None } else { Some(v) };
            }
        }
        None
    }

    /// `extractNilaiKontrak`: `nilaiKontrak_f`, lalu `spk.spk_nilai`.
    pub(super) fn nilai_kontrak(html: &str) -> Option<String> {
        input_by_id(html, "nilaiKontrak_f").or_else(|| hidden_value(html, "spk.spk_nilai"))
    }

    /// `extractFormFields`: input (tanpa submit/button/image/file/reset, checkbox/radio hanya bila
    /// `checked`), lalu textarea, lalu select (opsi terakhir yang `selected`, atau opsi pertama).
    /// Bila `form_id` tidak ditemukan, seluruh dokumen dipakai.
    pub(super) fn form_fields(html: &str, form_id: Option<&str>) -> Fields {
        let context = match form_id {
            Some(id) => {
                let pattern = format!(r#"(?is)<form[^>]*id=["']{}["'][^>]*>(.*?)</form>"#, regex::escape(id));
                Regex::new(&pattern)
                    .ok()
                    .and_then(|re| re.captures(html))
                    .map(|c| c[1].to_string())
                    .unwrap_or_else(|| html.to_string())
            }
            None => html.to_string(),
        };

        let mut fields: Fields = Vec::new();

        for c in regex_once!(r"(?is)<input\b([^>]*)>").captures_iter(&context) {
            let attrs = parse_attrs(&c[1]);
            let Some(name) = attr(&attrs, "name") else { continue };
            let ty = attr(&attrs, "type").filter(|t| !t.is_empty()).unwrap_or("text").to_lowercase();
            if ["submit", "button", "image", "file", "reset"].contains(&ty.as_str()) {
                continue;
            }
            if (ty == "checkbox" || ty == "radio") && !has_attr(&attrs, "checked") {
                continue;
            }
            let value = attr(&attrs, "value").unwrap_or("").to_string();
            put_field(&mut fields, name, value);
        }

        for c in regex_once!(r"(?is)<textarea\b([^>]*)>(.*?)</textarea>").captures_iter(&context) {
            let attrs = parse_attrs(&c[1]);
            let Some(name) = attr(&attrs, "name") else { continue };
            put_field(&mut fields, name, decode_entities(&c[2]));
        }

        for c in regex_once!(r"(?is)<select\b([^>]*)>(.*?)</select>").captures_iter(&context) {
            let attrs = parse_attrs(&c[1]);
            let Some(name) = attr(&attrs, "name") else { continue };
            let mut value: Option<String> = None;
            for o in regex_once!(r"(?is)<option\b([^>]*)>").captures_iter(&c[2]) {
                let oattrs = parse_attrs(&o[1]);
                let v = attr(&oattrs, "value").unwrap_or("").to_string();
                if value.is_none() || has_attr(&oattrs, "selected") {
                    value = Some(v);
                }
            }
            put_field(&mut fields, name, value.unwrap_or_default());
        }

        fields
    }

    fn parse_attrs(raw: &str) -> Vec<(String, Option<String>)> {
        regex_once!(r#"([^\s"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?"#)
            .captures_iter(raw)
            .map(|c| {
                let name = c[1].to_lowercase();
                let value = c.get(2).or(c.get(3)).or(c.get(4)).map(|m| decode_entities(m.as_str()));
                (name, value)
            })
            .collect()
    }

    fn attr<'a>(attrs: &'a [(String, Option<String>)], name: &str) -> Option<&'a str> {
        attrs.iter().find(|a| a.0 == name).map(|a| a.1.as_deref().unwrap_or(""))
    }

    fn has_attr(attrs: &[(String, Option<String>)], name: &str) -> bool {
        attrs.iter().any(|a| a.0 == name)
    }

    /// `extractSpseUserMessages`: pesan dari alert-danger, alert-warning, dan span.error. Tanpa duplikat.
    pub(super) fn spse_user_messages(html: &str) -> Vec<String> {
        const PATTERNS: [&str; 3] = [
            r#"(?is)<div[^>]*class=["'][^"']*alert-danger[^"']*["'][^>]*>(.*?)</div>"#,
            r#"(?is)<div[^>]*class=["'][^"']*alert-warning[^"']*["'][^>]*>(.*?)</div>"#,
            r#"(?is)<span[^>]*class=["'][^"']*error[^"']*["'][^>]*>(.*?)</span>"#,
        ];
        let mut out: Vec<String> = Vec::new();
        for p in PATTERNS {
            let Ok(re) = Regex::new(p) else { continue };
            for c in re.captures_iter(html) {
                let text = decode_entities(&strip_tags(&c[1])).trim().to_string();
                if !text.is_empty() && !out.contains(&text) {
                    out.push(text);
                }
            }
        }
        out
    }

    /// `resolveRekananId`: ID tersimpan, lalu rekananId tersembunyi, lalu opsi terpilih, lalu opsi
    /// tunggal, lalu pencocokan nama (sama, lalu saling memuat).
    pub(super) fn resolve_rekanan_id(html: &str, nama: &str, preferred: Option<&str>) -> Option<String> {
        if let Some(p) = preferred.filter(|p| truthy(Some(*p))) {
            return Some(p.to_string());
        }
        if let Some(h) = hidden_rekanan_id(html) {
            return Some(h);
        }
        if let Some(s) = selected_rekanan_id(html) {
            return Some(s);
        }
        let labeled = labeled_rekanan_options(html);
        if labeled.len() == 1 {
            return Some(labeled[0].0.clone());
        }
        let target = normalize_name(nama);
        if target.is_empty() {
            return None;
        }
        if let Some((id, _)) = labeled.iter().find(|(_, label)| normalize_name(label) == target) {
            return Some(id.clone());
        }
        labeled
            .iter()
            .find(|(_, label)| {
                let n = normalize_name(label);
                !n.is_empty() && (n.contains(&target) || target.contains(&n))
            })
            .map(|(id, _)| id.clone())
    }

    pub(super) fn hidden_rekanan_id(html: &str) -> Option<String> {
        hidden_value(html, "rekananId").filter(|v| valid_id(Some(v.as_str())))
    }

    pub(super) fn selected_rekanan_id(html: &str) -> Option<String> {
        if let Some(c) = regex_once!(
            r#"(?is)<select[^>]*name=["']rekananId["'][^>]*>.*?<option[^>]*selected[^>]*value=["']([0-9]+)["']"#
        )
        .captures(html)
        {
            return valid_id(Some(&c[1])).then(|| c[1].to_string());
        }
        if let Some(c) =
            regex_once!(r#"(?is)<option[^>]*value=["']([0-9]+)["'][^>]*selected[^>]*>.*?</select>"#).captures(html)
        {
            return valid_id(Some(&c[1])).then(|| c[1].to_string());
        }
        None
    }

    /// `(id, label)` untuk opsi berangka. Label kosong dan "Pilih" dibuang.
    pub(super) fn labeled_rekanan_options(html: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for c in regex_once!(r#"(?is)<option[^>]*value=["']([0-9]+)["'][^>]*>(.*?)</option>"#).captures_iter(html) {
            let id = c[1].trim().to_string();
            if !valid_id(Some(id.as_str())) {
                continue;
            }
            let label = decode_entities(&strip_tags(&c[2])).trim().to_string();
            if label.is_empty() || label.to_lowercase() == "pilih" {
                continue;
            }
            out.push((id, label));
        }
        out
    }

    pub(super) fn rekanan_labels(html: &str) -> Vec<String> {
        labeled_rekanan_options(html).into_iter().map(|(_, label)| label).collect()
    }

    /// `normalizeName`: huruf besar, spasi dirapikan, titik, koma, dan kurung dibuang.
    pub(super) fn normalize_name(name: &str) -> String {
        let upper = name.trim().to_uppercase();
        let collapsed = regex_once!(r"\s+").replace_all(&upper, " ").into_owned();
        collapsed.chars().filter(|c| !matches!(c, '.' | ',' | '(' | ')')).collect()
    }
}

// ---------------------------------------------------------------------------
// HTTP ke SPSE (SpseHttpClient: POST dan token)
// ---------------------------------------------------------------------------

/// Klien tanpa pengikutan redirect, untuk POST (`allow_redirects => false` di Laravel).
fn no_redirect_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(spse::USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// `asMultipart()` tanpa berkas: setiap field menjadi bagian form-data berurutan.
fn multipart_body(fields: &Fields) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn post_multipart(s: &Session, path: &str, fields: &Fields, referer: &str) -> Result<Resp, SpseError> {
    let rb = spse::with_cookies(no_redirect_client().post(spse::absolute_url(s, path)), &s.cookies)
        .header(header::REFERER, spse::absolute_url(s, referer))
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"))
        .body(multipart_body(fields));
    let resp = spse::send(rb).await.map_err(|e| SpseError::Failed(e.to_string()))?;
    spse::assert_authenticated(&resp)?;
    Ok(resp)
}

async fn post_action(s: &Session, path: &str, referer: &str) -> Result<Resp, SpseError> {
    let rb = spse::with_cookies(no_redirect_client().post(spse::absolute_url(s, path)), &s.cookies)
        .header(header::REFERER, spse::absolute_url(s, referer));
    let resp = spse::send(rb).await.map_err(|e| SpseError::Failed(e.to_string()))?;
    spse::assert_authenticated(&resp)?;
    Ok(resp)
}

/// `fetchAuthenticityToken`: cookie `___AT=` lebih dulu, lalu halaman referer, beranda, dan home.
async fn fetch_token(s: &Session, referer: &str) -> Result<String, SpseError> {
    if let Some(t) = spse::token_from_cookies(&s.cookies) {
        return Ok(t);
    }
    let mut pages: Vec<String> = Vec::new();
    for p in [referer, "/beranda/nontender", "/home"] {
        let url = spse::absolute_url(s, p);
        if !pages.contains(&url) {
            pages.push(url);
        }
    }
    for url in pages {
        let rb = spse::with_cookies(spse::client().get(&url), &s.cookies).header(header::ACCEPT, PAGE_ACCEPT);
        let resp = spse::send(rb).await.map_err(|e| SpseError::Failed(e.to_string()))?;
        if !(200..300).contains(&resp.status) {
            continue;
        }
        if let Some(t) = spse::token_from_html(&String::from_utf8_lossy(&resp.body)) {
            return Ok(t);
        }
    }
    Err(SpseError::Failed("Tidak dapat mengambil authenticityToken dari SPSE.".to_string()))
}

/// `assertSaveOk`: status 200, 302, atau 303, dan body tidak berisi "error" bersama "exception".
fn assert_save_ok(resp: &Resp, label: &str) -> Result<(), PushErr> {
    if ![200u16, 302, 303].contains(&resp.status) {
        return Err(PushErr::Failed(format!("Simpan {label} gagal: HTTP {}", resp.status)));
    }
    let body = String::from_utf8_lossy(&resp.body).to_lowercase();
    if body.contains("error") && body.contains("exception") {
        return Err(PushErr::Failed(format!("Simpan {label} ditolak SPSE.")));
    }
    Ok(())
}

/// Satu push: token per referer (cache seperti `$tokenCache` di Laravel), dan langkah yang sudah jalan.
struct Run<'a> {
    s: &'a Session,
    tokens: HashMap<String, String>,
    steps: Vec<Value>,
}

impl<'a> Run<'a> {
    async fn fetch(&self, path: &str, referer: Option<&str>) -> Result<String, PushErr> {
        Ok(spse::fetch_page(self.s, path, referer).await?)
    }

    async fn token(&mut self, referer: &str) -> Result<String, PushErr> {
        if let Some(t) = self.tokens.get(referer) {
            return Ok(t.clone());
        }
        let t = fetch_token(self.s, referer).await?;
        self.tokens.insert(referer.to_string(), t.clone());
        Ok(t)
    }

    async fn action(&self, path: &str, referer: &str) -> Result<Resp, PushErr> {
        Ok(post_action(self.s, path, referer).await?)
    }

    async fn save(&self, path: &str, fields: &Fields, referer: &str, label: &str) -> Result<Resp, PushErr> {
        let resp = post_multipart(self.s, path, fields, referer).await?;
        assert_save_ok(&resp, label)?;
        Ok(resp)
    }

    async fn simpan_sppbj(
        &mut self,
        k: &KontrakRow,
        cfg: &Settings,
        pl_id: &str,
        rekanan: &str,
        form_path: &str,
    ) -> Result<(Resp, Value), PushErr> {
        let token = self.token(form_path).await?;
        let fields: Fields = vec![
            ("authenticityToken".into(), token),
            ("sppbj.sppbj_no".into(), k.sppbj.clone().unwrap_or_default()),
            ("sppbj.sppbj_lamp".into(), LAMP.into()),
            ("sppbj.sppbj_tgl_kirim".into(), format_date(k.tgl_sppbj.or(k.tgl_spk))),
            ("sppbj.sppbj_kota".into(), cfg.kota.clone()),
            ("sppbj.jabatan_ppk_sppbj".into(), cfg.ppk.jabatan.clone()),
            ("sppbj.alamat_satker".into(), cfg.alamat.clone()),
            ("rekananId".into(), rekanan.to_string()),
            ("sppbj.jaminan_pelaksanaan".into(), JAMINAN.into()),
            ("sppbj.masa_berlaku_jaminan".into(), MASA_JAMINAN.into()),
        ];
        let resp = self
            .save(&format!("/sppbj-pl/simpansppbjpl?plId={pl_id}"), &fields, form_path, "SPPBJ")
            .await?;
        let detail = json!({ "status_code": resp.status, "location": resp.location });
        Ok((resp, detail))
    }

    /// `simpan_spk`. Token sudah diambil pemanggil (di luar `runStep`, seperti Laravel).
    #[allow(clippy::too_many_arguments)]
    async fn simpan_spk(
        &self,
        k: &KontrakRow,
        cfg: &Settings,
        form_html: &str,
        token: &str,
        sppbj_id: &str,
        existing_spk: Option<String>,
        nilai: &str,
        form_path: &str,
    ) -> Result<(Resp, Value), PushErr> {
        let form = html::form_fields(form_html, Some("formPesanan"));
        let from_form = |key: &str, default: &str| -> String {
            match form_get(&form, key) {
                Some(v) if !v.trim().is_empty() => v.to_string(),
                _ => default.to_string(),
            }
        };
        let nama_ppk = from_form("spk.nama_ppk_kontrak", &cfg.ppk.nama);
        let nip_ppk = from_form("spk.nip_ppk_kontrak", &cfg.ppk.nip);
        let jabatan_ppk = from_form("spk.jabatan_ppk_kontrak", &cfg.ppk.jabatan);
        let no_sk_ppk = from_form("spk.no_sk_ppk_kontrak", &cfg.ppk.no_sk);

        let penyedia = k.penyedia.as_ref();
        let bank = penyedia.and_then(|p| p.bank.as_deref()).map(str::trim).unwrap_or("");
        let norek = penyedia.and_then(|p| p.norek.as_deref()).map(str::trim).unwrap_or("");
        let bank = if bank.is_empty() { BANK_DEFAULT } else { bank };
        let norek = if norek.is_empty() { NOREK_DEFAULT } else { norek };

        let fields = merge(
            form.clone(),
            vec![
                ("authenticityToken", token.to_string()),
                ("spk.kontrak_lingkup_pekerjaan", LINGKUP.to_string()),
                ("spk.spk_id", existing_spk.unwrap_or_default()),
                ("spk.spk_no", k.spk.clone().unwrap_or_default()),
                ("spk.spk_tgl", format_date(k.tgl_spk)),
                ("content.kota_pesanan", cfg.kota.clone()),
                ("spk.nama_ppk_kontrak", nama_ppk),
                ("spk.nip_ppk_kontrak", nip_ppk),
                ("spk.jabatan_ppk_kontrak", jabatan_ppk),
                ("spk.no_sk_ppk_kontrak", no_sk_ppk),
                ("spk.spk_wakil_penyedia", wakil(penyedia)),
                ("spk.spk_jabatan_wakil", JABATAN_WAKIL.to_string()),
                ("spk.spk_nama_bank", bank.to_string()),
                ("spk.spk_norekening", norek.to_string()),
                ("spk.spk_nilai", nilai.to_string()),
                ("spk.nilai_pdn", nilai.to_string()),
                ("spk.nilai_umk", nilai.to_string()),
                ("content.waktu_penyelesaian", waktu_penyelesaian(k.tgl_spmk, k.tgl_selesai)),
                ("tgl_diterima", format_date(k.tgl_spmk)),
                ("tgl_selesai", format_date(k.tgl_selesai)),
                ("ubahnilai", "false".to_string()),
                ("spk.alasanubah_s", String::new()),
            ],
        );
        let resp = self
            .save(&format!("/spk-pl/simpanspk?sppbjId={sppbj_id}"), &fields, form_path, "SPK")
            .await?;
        let detail = json!({
            "status_code": resp.status,
            "sppbj_id": sppbj_id,
            "spk_nilai": nilai,
            "nilai_pdn": nilai,
            "nilai_umk": nilai,
        });
        Ok((resp, detail))
    }

    async fn simpan_spmk(
        &self,
        k: &KontrakRow,
        cfg: &Settings,
        form: Fields,
        token: &str,
        spk_id: &str,
        sppbj_id: &str,
        form_path: &str,
    ) -> Result<(Resp, Value), PushErr> {
        let penyedia = k.penyedia.as_ref();
        let fields = merge(
            form,
            vec![
                ("authenticityToken", token.to_string()),
                ("pesanan.pes_no", k.spmk.clone().unwrap_or_default()),
                ("pesanan.pes_tgl", format_date(k.tgl_spmk)),
                ("tgl_diterima", format_date(k.tgl_spmk)),
                ("content.waktu_penyelesaian", waktu_penyelesaian(k.tgl_spmk, k.tgl_selesai)),
                ("tgl_selesai", format_date(k.tgl_selesai)),
                ("content.kota_pesanan", cfg.kota.clone()),
                ("content.wakil_sah_rekanan", wakil(penyedia)),
                ("content.jabatan_wakil_rekanan", JABATAN_WAKIL.to_string()),
                ("simpan", String::new()),
            ],
        );
        let resp = self
            .save(
                &format!("/spk-pl/simpansuratpesanannon?spkId={spk_id}&sppbjId={sppbj_id}"),
                &fields,
                form_path,
                "SPMK",
            )
            .await?;
        let detail = json!({ "status_code": resp.status, "spk_id": spk_id, "sppbj_id": sppbj_id });
        Ok((resp, detail))
    }
}

// ---------------------------------------------------------------------------
// Alur push (SpseKontrakPushService::doPush)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ids {
    pl_id: String,
    sppbj_id: Option<String>,
    spk_id: Option<String>,
    rekanan_id: Option<String>,
}

impl Ids {
    fn to_json(&self) -> Value {
        json!({
            "pl_id": self.pl_id,
            "sppbj_id": self.sppbj_id,
            "spk_id": self.spk_id,
            "rekanan_id": self.rekanan_id,
        })
    }
}

struct Outcome {
    ids: Ids,
    spk_nilai: Option<String>,
    list: ListStatus,
}

fn skipped(step: &str, reason: &str) -> Value {
    json!({ "step": step, "status": "skipped", "reason": reason })
}

fn ok_step(step: &str, detail: Value) -> Value {
    let mut m = Map::new();
    m.insert("step".into(), json!(step));
    m.insert("status".into(), json!("ok"));
    if let Value::Object(d) = detail {
        m.extend(d);
    }
    Value::Object(m)
}

fn assert_pushable(k: &KontrakRow) -> Result<(), PushErr> {
    let kode = k.kode_paket.as_deref().unwrap_or("").trim();
    if spse::php_empty(Some(kode)) {
        return Err(PushErr::Invalid("kode_paket wajib diisi sebelum push ke SPSE.".into()));
    }
    if k.penyedia.is_none() || k.id_penyedia.unwrap_or(0) == 0 {
        return Err(PushErr::Invalid("Penyedia wajib dipilih pada kontrak.".into()));
    }
    let spk = k.spk.as_deref().unwrap_or("").trim();
    let sppbj = k.sppbj.as_deref().unwrap_or("").trim();
    if spse::php_empty(Some(spk)) && spse::php_empty(Some(sppbj)) {
        return Err(PushErr::Invalid("Minimal nomor SPK atau SPPBJ harus diisi.".into()));
    }
    Ok(())
}

fn assert_not_already_pushed(k: &KontrakRow) -> Result<(), PushErr> {
    match k.spse_pushed_at {
        None => Ok(()),
        Some(at) => Err(PushErr::Invalid(format!(
            "Kontrak sudah di-push ke SPSE pada {}. Push ulang diblokir agar data tidak ditimpa.",
            at.format("%d-%m-%Y %H:%M")
        ))),
    }
}

fn assert_not_complete_in_spse(list: &ListStatus) -> Result<(), PushErr> {
    if list.all_complete {
        return Err(PushErr::Invalid(
            "Kontrak sudah lengkap di SPSE (SPPBJ, SPK, SSKK, SPMK). Push dibatalkan agar data tidak ditimpa.".into(),
        ));
    }
    Ok(())
}

fn rekanan_message(form_html: &str) -> String {
    let hidden = html::hidden_rekanan_id(form_html);
    let labels = html::rekanan_labels(form_html);
    let detail = if hidden.is_some() {
        String::new()
    } else if labels.is_empty() {
        " Form SPPBJ tidak memuat rekananId atau daftar penyedia.".to_string()
    } else {
        format!(" Rekanan di SPSE: {}.", labels.join("; "))
    };
    format!(
        "rekananId tidak ditemukan di form SPPBJ SPSE.{detail} Pastikan paket sudah punya pemenang di SPSE (rekanan biasanya sudah terisi otomatis, tidak perlu input nama)."
    )
}

/// `resolveSppbjIdAfterSave`.
async fn resolve_sppbj_id_after_save(run: &Run<'_>, save: &Resp, list_path: &str, ids: &Ids) -> Result<String, PushErr> {
    let location = save.location.clone().unwrap_or_default();
    let body = String::from_utf8_lossy(&save.body).into_owned();
    let redirect_path = path_from_spse_url(&location);

    if let Some(v) = html::query_param(&location, "sppbjId") {
        return Ok(v);
    }
    if let Some(v) = html::sppbj_id_from_html(&body) {
        return Ok(v);
    }

    let list_html = run
        .fetch(list_path, Some(redirect_path.as_deref().unwrap_or("/beranda/nontender")))
        .await?;
    if let Some(v) = html::sppbj_id_from_html(&list_html) {
        return Ok(v);
    }
    if truthy(ids.sppbj_id.as_deref()) {
        return Ok(ids.sppbj_id.clone().unwrap_or_default());
    }

    let mut form_html: Option<String> = None;
    if let Some(rp) = &redirect_path {
        let h = run.fetch(rp, Some(list_path)).await?;
        if let Some(v) = html::sppbj_id_from_html(&h) {
            return Ok(v);
        }
        form_html = Some(h);
    }

    let messages = form_html.as_deref().map(html::spse_user_messages).unwrap_or_default();
    let hint = if messages.is_empty() {
        String::new()
    } else {
        format!(" Pesan SPSE: {}", messages.join(" | "))
    };
    if !location.is_empty() && !location.contains("sppbjId=") {
        return Err(PushErr::Invalid(format!(
            "SPSE mengembalikan form SPPBJ tanpa ID — simpan kemungkinan ditolak.{hint} Periksa nomor/tanggal SPPBJ di SPSE atau buka paket {} di menu kontrak SPSE.",
            ids.pl_id
        )));
    }
    let redirect = if location.is_empty() { String::new() } else { format!(" Redirect SPSE: {location}") };
    Err(PushErr::Invalid(format!(
        "sppbjId tidak ditemukan setelah simpan SPPBJ.{hint}{redirect}"
    )))
}

/// `resolveSpkIdAfterSave`.
async fn resolve_spk_id_after_save(
    run: &Run<'_>,
    save: &Resp,
    list_path: &str,
    spk_form_path: &str,
    fallback: Option<String>,
) -> Result<String, PushErr> {
    let location = save.location.clone().unwrap_or_default();
    let body = String::from_utf8_lossy(&save.body).into_owned();

    let from_save = html::query_param(&location, "spkId")
        .or_else(|| html::query_param(&body, "spkId"))
        .or_else(|| html::hidden_value(&body, "spk.spk_id"));
    if let Some(v) = from_save.filter(|v| is_digits(v)) {
        return Ok(v);
    }

    let list_html = run.fetch(list_path, Some("/beranda/nontender")).await?;
    let list_status = html::list_status(&list_html);
    let from_list = list_status.spk_id.or_else(|| html::query_param(&list_html, "spkId"));
    if truthy(from_list.as_deref()) {
        return Ok(from_list.unwrap_or_default());
    }

    let form_html = run.fetch(spk_form_path, Some(list_path)).await?;
    if let Some(v) = html::hidden_value(&form_html, "spk.spk_id").filter(|v| is_digits(v)) {
        return Ok(v);
    }
    if let Some(v) = fallback.filter(|v| is_digits(v)) {
        return Ok(v);
    }

    let mut messages = html::spse_user_messages(&body);
    for m in html::spse_user_messages(&form_html) {
        if !messages.contains(&m) {
            messages.push(m);
        }
    }
    let hint = if messages.is_empty() {
        String::new()
    } else {
        format!(" Pesan SPSE: {}.", messages.join(" | "))
    };
    let redirect = if location.is_empty() { String::new() } else { format!(", redirect {location}") };
    Err(PushErr::Failed(format!(
        "SPK tidak tercatat di SPSE setelah simpan (HTTP {}{redirect}).{hint} Periksa isian SPK (nomor/tanggal SPK, nilai) di SPSE untuk paket ini.",
        save.status
    )))
}

/// `doPush` (urutan langkah sama dengan Laravel).
async fn run_workflow(run: &mut Run<'_>, k: &KontrakRow) -> Result<Outcome, PushErr> {
    assert_pushable(k)?;
    assert_not_already_pushed(k)?;

    let pl_id = k.kode_paket.as_deref().unwrap_or("").trim().to_string();
    let mut ids = Ids {
        pl_id: pl_id.clone(),
        sppbj_id: k.spse_sppbj_id.clone(),
        spk_id: k.spse_spk_id.clone(),
        rekanan_id: k.spse_rekanan_id.clone(),
    };

    let list_path = format!("/sppbj-pl/listsppbjpl?plId={pl_id}");
    let list_html = run.fetch(&list_path, Some("/beranda/nontender")).await?;
    let mut list = html::list_status(&list_html);
    assert_not_complete_in_spse(&list)?;
    let cfg = settings()?;

    ids.sppbj_id = or_php(ids.sppbj_id.take(), list.sppbj_id.clone());
    ids.spk_id = or_php(ids.spk_id.take(), list.spk_id.clone());

    let sppbj_form_path = format!("/sppbj-pl/sppbjppkpl?plId={pl_id}");
    let mut sppbj_form_html: Option<String> = None;
    let mut sppbj_id = ids.sppbj_id.clone();
    if !truthy(sppbj_id.as_deref()) {
        let h = run.fetch(&sppbj_form_path, Some(&list_path)).await?;
        sppbj_id = html::sppbj_id_from_html(&h);
        sppbj_form_html = Some(h);
    }

    if truthy(sppbj_id.as_deref()) {
        ids.sppbj_id = sppbj_id;
        run.steps.push(skipped("pengecekan_blacklist", "SPPBJ sudah ada di SPSE."));
        run.steps.push(skipped("simpan_sppbj", "SPPBJ sudah ada di SPSE."));
    } else {
        let form_html = match sppbj_form_html {
            Some(h) => h,
            None => run.fetch(&sppbj_form_path, Some(&list_path)).await?,
        };
        let nama = k.penyedia.as_ref().and_then(|p| p.nama.clone()).unwrap_or_default();
        let rekanan = html::resolve_rekanan_id(&form_html, &nama, k.spse_rekanan_id.as_deref())
            .ok_or_else(|| PushErr::Invalid(rekanan_message(&form_html)))?;
        ids.rekanan_id = Some(rekanan.clone());

        let tgl = format_date(k.tgl_sppbj.or(k.tgl_spk).or(Some(Utc::now().date_naive())));
        let blacklist = format!("/sppbj-pl/pengecekanblacklist?rknId={rekanan}&tglbuat={tgl}&llsId={pl_id}");
        let r = run.action(&blacklist, &sppbj_form_path).await.map(|_| ());
        wrap("pengecekan_blacklist", r)?;
        run.steps.push(ok_step("pengecekan_blacklist", json!({ "rekanan_id": rekanan })));

        let (save, detail) = wrap(
            "simpan_sppbj",
            run.simpan_sppbj(k, &cfg, &pl_id, &rekanan, &sppbj_form_path).await,
        )?;
        run.steps.push(ok_step("simpan_sppbj", detail));
        ids.sppbj_id = Some(resolve_sppbj_id_after_save(run, &save, &list_path, &ids).await?);
    }

    let list_html = run.fetch(&list_path, Some("/beranda/nontender")).await?;
    list = html::list_status(&list_html);
    ids.spk_id = or_php(ids.spk_id.take(), list.spk_id.clone());

    let sppbj_id = ids.sppbj_id.clone().unwrap_or_default();
    let mut spk_nilai: Option<String> = None;
    let mut existing_spk = ids.spk_id.clone();

    if list.spk_complete {
        run.steps.push(skipped("simpan_spk", "SPK sudah ada di SPSE."));
    } else {
        let spk_form_path = format!("/spk-pl/spkpl?sppbjId={sppbj_id}");
        let spk_form_html = run.fetch(&spk_form_path, Some(&list_path)).await?;
        existing_spk = or_php(k.spse_spk_id.clone(), html::hidden_value(&spk_form_html, "spk.spk_id"));
        let nilai = match html::nilai_kontrak(&spk_form_html) {
            Some(v) if !v.trim().is_empty() => v,
            _ => {
                return Err(PushErr::Invalid(
                    "Nilai kontrak tidak ditemukan di form SPK SPSE. Pastikan paket sudah memiliki nilai penawaran/pemenang di SPSE.".into(),
                ))
            }
        };
        match parse_nilai(&nilai) {
            Some(n) if n != 0.0 => {}
            _ => {
                return Err(PushErr::Invalid(
                    "Nilai kontrak di SPSE kosong atau 0. PDN/UMK tidak dapat diisi otomatis.".into(),
                ))
            }
        }
        spk_nilai = Some(nilai.clone());

        let token = run.token(&spk_form_path).await?;
        let (save, detail) = wrap(
            "simpan_spk",
            run.simpan_spk(k, &cfg, &spk_form_html, &token, &sppbj_id, existing_spk.clone(), &nilai, &spk_form_path)
                .await,
        )?;
        run.steps.push(ok_step("simpan_spk", detail));
        existing_spk = Some(resolve_spk_id_after_save(run, &save, &list_path, &spk_form_path, existing_spk.clone()).await?);
    }

    let spk_id = existing_spk
        .filter(|v| truthy(Some(v.as_str())))
        .ok_or_else(|| PushErr::Failed("spkId tidak ditemukan di SPSE. Simpan SPK terlebih dahulu.".into()))?;
    ids.spk_id = Some(spk_id.clone());

    if list.sskk_complete {
        run.steps.push(skipped("simpan_cara_pembayaran", "SSKK/cara pembayaran sudah ada di SPSE."));
    } else {
        let token = run.token(&list_path).await?;
        let fields: Fields = vec![
            ("authenticityToken".into(), token),
            ("cara_pembayaran".into(), cfg.cara_bayar.clone()),
            ("jumlah_termin".into(), String::new()),
            ("jumlah_bulan".into(), String::new()),
            ("simpan".into(), "simpan".into()),
        ];
        let path = format!("/sskk-pl/simpancarapembayaran?id={sppbj_id}");
        let resp = wrap(
            "simpan_cara_pembayaran",
            run.save(&path, &fields, &list_path, "cara pembayaran").await,
        )?;
        run.steps.push(ok_step("simpan_cara_pembayaran", json!({ "status_code": resp.status })));
    }

    if list.spmk_complete {
        run.steps.push(skipped("simpan_spmk", "SPMK sudah ada di SPSE."));
    } else {
        let spmk_path = format!("/spk-pl/spmknon?sppbjId={sppbj_id}");
        let spmk_html = run.fetch(&spmk_path, Some(&list_path)).await?;
        let form = html::form_fields(&spmk_html, None);
        let token = match form_get(&form, "authenticityToken") {
            Some(t) => t.to_string(),
            None => run.token(&spmk_path).await?,
        };
        let (_save, detail) = wrap(
            "simpan_spmk",
            run.simpan_spmk(k, &cfg, form, &token, &spk_id, &sppbj_id, &spmk_path).await,
        )?;
        run.steps.push(ok_step("simpan_spmk", detail));
    }

    let verify_html = run.fetch(&list_path, Some("/beranda/nontender")).await?;
    let verify = html::list_status(&verify_html);
    let mut missing: Vec<&str> = Vec::new();
    if !verify.sppbj_complete {
        missing.push("SPPBJ");
    }
    if !verify.spk_complete {
        missing.push("SPK");
    }
    if !verify.spmk_complete {
        missing.push("SPMK");
    }
    if !missing.is_empty() {
        return Err(PushErr::Failed(format!(
            "SPSE belum mencatat {} setelah simpan (kemungkinan ditolak validasi SPSE). Cek paket di SPSE lalu coba lagi.",
            missing.join(", ")
        )));
    }

    Ok(Outcome { ids, spk_nilai, list })
}

fn fmt_ts(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn put(old: &mut Map<String, Value>, new: &mut Map<String, Value>, key: &str, o: Value, n: Value) {
    if o != n {
        old.insert(key.to_string(), o);
        new.insert(key.to_string(), n);
    }
}

/// Simpan ID SPSE dan log ke kontrak, dengan audit dan notifikasi (`changes::log`), dalam satu transaksi.
async fn save_kontrak(
    state: &AppState,
    headers: &HeaderMap,
    actor: u64,
    url: &str,
    k: &KontrakRow,
    ids: &Ids,
    log: &Value,
) -> Result<(), PushErr> {
    let mut tx = state.pool.begin().await.map_err(db)?;
    sqlx::query(
        "UPDATE tbl_kontrak SET spse_sppbj_id = ?, spse_spk_id = ?, spse_rekanan_id = ?, \
         spse_pushed_at = NOW(), spse_push_log = ?, updated_at = NOW() WHERE id = ?",
    )
    .bind(ids.sppbj_id.as_deref())
    .bind(ids.spk_id.as_deref())
    .bind(ids.rekanan_id.as_deref())
    .bind(log.to_string())
    .bind(k.id)
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    let after = sqlx::query("SELECT spse_pushed_at, updated_at FROM tbl_kontrak WHERE id = ?")
        .bind(k.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let pushed_after: Option<DateTime<Utc>> = after.try_get("spse_pushed_at").map_err(db)?;
    let updated_after: Option<DateTime<Utc>> = after.try_get("updated_at").map_err(db)?;

    let mut old = Map::new();
    let mut new = Map::new();
    put(&mut old, &mut new, "spse_sppbj_id", json!(k.spse_sppbj_id), json!(ids.sppbj_id));
    put(&mut old, &mut new, "spse_spk_id", json!(k.spse_spk_id), json!(ids.spk_id));
    put(&mut old, &mut new, "spse_rekanan_id", json!(k.spse_rekanan_id), json!(ids.rekanan_id));
    put(
        &mut old,
        &mut new,
        "spse_pushed_at",
        json!(k.spse_pushed_at.map(fmt_ts)),
        json!(pushed_after.map(fmt_ts)),
    );
    put(&mut old, &mut new, "spse_push_log", Value::Null, log.clone());
    put(
        &mut old,
        &mut new,
        "updated_at",
        json!(k.updated_at.map(fmt_ts)),
        json!(updated_after.map(fmt_ts)),
    );

    changes::log(
        &mut tx,
        headers,
        actor,
        &changes::KONTRAK,
        "updated",
        k.id,
        Some(old),
        Some(new),
        k.id_pekerjaan,
        url,
    )
    .await
    .map_err(PushErr::Server)?;
    tx.commit().await.map_err(db)?;
    Ok(())
}

/// `SpseProcurementController@pushKontrak` + `SpseKontrakPushService::push`.
async fn push(
    state: &AppState,
    headers: &HeaderMap,
    actor: u64,
    session: &Session,
    kontrak_id: i64,
    url: &str,
) -> Result<Value, PushErr> {
    let pool = &state.pool;
    let loaded = load_kontrak(pool, kontrak_id)
        .await
        .map_err(db)?
        .ok_or_else(|| PushErr::Server(ApiError::not_found()))?;

    let _lock = PushLock::acquire(kontrak_id).ok_or_else(|| {
        PushErr::Invalid("Push kontrak ini sedang berjalan. Tunggu hingga selesai.".into())
    })?;
    let kontrak = load_kontrak(pool, kontrak_id).await.map_err(db)?.unwrap_or(loaded);

    let mut run = Run {
        s: session,
        tokens: HashMap::new(),
        steps: Vec::new(),
    };
    let out = run_workflow(&mut run, &kontrak).await?;
    let steps = std::mem::take(&mut run.steps);

    let log = json!({
        "pushed_at": Utc::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        "pl_id": out.ids.pl_id,
        "spk_nilai_spse": out.spk_nilai,
        "spse_status_before_push": out.list.to_json(),
        "steps": steps,
        "ids": out.ids.to_json(),
    });
    save_kontrak(state, headers, actor, url, &kontrak, &out.ids, &log).await?;

    Ok(json!({
        "message": "Push kontrak ke SPSE selesai.",
        "kontrak_id": kontrak.id,
        "spse_ids": out.ids.to_json(),
        "nilai_kontrak_spse": out.spk_nilai,
        "steps": steps,
    }))
}

/// `kontrak_id`: `required|integer|exists:tbl_kontrak,id`.
async fn kontrak_id_from_body(pool: &MySqlPool, body: &Value) -> Result<i64, ApiError> {
    let mut errors = Errors::default();
    let id = match body.get("kontrak_id") {
        None | Some(Value::Null) => {
            errors.add("kontrak_id", "The kontrak id field is required.");
            0
        }
        Some(v) => {
            let parsed = match v {
                Value::Number(n) if n.is_i64() => n.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            match parsed {
                Some(id) => {
                    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kontrak WHERE id = ?")
                        .bind(id)
                        .fetch_one(pool)
                        .await
                        .map_err(|e| spse::internal(e))?;
                    if n == 0 {
                        errors.add("kontrak_id", "The selected kontrak id is invalid.");
                    }
                    id
                }
                None => {
                    errors.add("kontrak_id", "The kontrak id field must be an integer.");
                    0
                }
            }
        }
    };
    errors.finish()?;
    Ok(id)
}

/// `POST /api/procurement/spse/kontrak/push`.
pub async fn push_kontrak(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let session = spse::require_session(pool, user.user_id).await?;
    let kontrak_id = kontrak_id_from_body(pool, &body).await?;
    let url = spse::full_url(&state, "/api/procurement/spse/kontrak/push");

    match push(&state, &headers, user.user_id, &session, kontrak_id, &url).await {
        Ok(v) => Ok(Json(v).into_response()),
        Err(PushErr::Expired) => {
            spse::deactivate_session(pool, session.id).await?;
            Err(PushErr::Expired.into_api())
        }
        Err(e) => Err(e.into_api()),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(y, m, day)
    }

    #[test]
    fn list_status_membaca_id_dan_kelengkapan() {
        let parsial = r#"<table id="tblsppbj"><tr><td><a href="/sppbj-pl/sppbjppkpl?plId=P1&amp;sppbjId=9001">SPPBJ</a></td>
            <td><a href="/spk-pl/spkpl?sppbjId=9001&spkId=9101">SPK</a></td></tr></table>"#;
        let s = html::list_status(parsial);
        assert_eq!(s.sppbj_id.as_deref(), Some("9001"));
        assert_eq!(s.spk_id.as_deref(), Some("9101"));
        assert!(s.sppbj_complete && s.spk_complete);
        assert!(!s.sskk_complete && !s.spmk_complete && !s.all_complete);

        let lengkap = r#"<table id="tblsppbj"><tr><td>sppbjId=9001</td><td>spkId=9101</td>
            <td><a href="/spk-pl/cetak?pesananId=9201">SPMK</a></td><td>Sekaligus</td></tr></table>"#;
        let s = html::list_status(lengkap);
        assert!(s.spmk_complete && s.sskk_complete && s.all_complete);
        assert_eq!(s.pesanan_id.as_deref(), Some("9201"));
    }

    #[test]
    fn list_status_tanpa_tabel_memakai_seluruh_html() {
        let s = html::list_status(r#"<a href="/x?simpancarapembayaran?id=77">x</a>"#);
        assert_eq!(s.sppbj_id.as_deref(), Some("77"));
        assert!(s.sppbj_complete && !s.spk_complete);
    }

    #[test]
    fn sppbj_id_dari_input_dan_tautan_dengan_pengecualian_nol() {
        let input = r#"<input type="hidden" name="sppbj.sppbj_id" value="4242">"#;
        assert_eq!(html::sppbj_id_from_html(input).as_deref(), Some("4242"));
        let terbalik = r#"<input value='555' name='sppbj.sppbj_id'>"#;
        assert_eq!(html::sppbj_id_from_html(terbalik).as_deref(), Some("555"));
        let nol = r#"<input name="sppbj.sppbj_id" value="0"> <a href="/sppbj-pl/sppbjppkpl?plId=1&sppbjId=0">x</a>"#;
        assert_eq!(html::sppbj_id_from_html(nol), None);
        let tautan = r#"<a href="/spk-pl/spkpl?sppbjId=8080">x</a>"#;
        assert_eq!(html::sppbj_id_from_html(tautan).as_deref(), Some("8080"));
    }

    #[test]
    fn query_param_dan_path_redirect() {
        assert_eq!(
            html::query_param("/spk-pl/spkpl?sppbjId=9001&spkId=9101", "spkId").as_deref(),
            Some("9101")
        );
        assert_eq!(html::query_param("tanpa id", "spkId"), None);
        assert_eq!(
            path_from_spse_url("https://spse.example/uji/spk-pl/spkpl?sppbjId=1#frag").as_deref(),
            Some("/uji/spk-pl/spkpl?sppbjId=1")
        );
        assert_eq!(
            path_from_spse_url("/sppbj-pl/sppbjppkpl?plId=P&sppbjId=2").as_deref(),
            Some("/sppbj-pl/sppbjppkpl?plId=P&sppbjId=2")
        );
        assert_eq!(path_from_spse_url("https://spse.example"), None);
        assert_eq!(path_from_spse_url(""), None);
    }

    #[test]
    fn resolve_rekanan_urutan_pilihan() {
        // 1. ID tersimpan kontrak menang atas form.
        assert_eq!(
            html::resolve_rekanan_id("<input name=\"rekananId\" value=\"11\">", "X", Some("77")).as_deref(),
            Some("77")
        );
        // 2. rekananId tersembunyi.
        assert_eq!(
            html::resolve_rekanan_id(r#"<input type="hidden" name="rekananId" value="11">"#, "X", None).as_deref(),
            Some("11")
        );
        // 3. Opsi terpilih.
        let terpilih = r#"<select name="rekananId"><option value="0">Pilih</option>
            <option value="21">CV A</option><option value="22" selected>CV B</option></select>"#;
        assert_eq!(html::resolve_rekanan_id(terpilih, "X", None).as_deref(), Some("22"));
        // 4. Satu opsi bernilai (Pilih dibuang).
        let tunggal = r#"<select><option value="0">Pilih</option><option value="31">PT Satu</option></select>"#;
        assert_eq!(html::resolve_rekanan_id(tunggal, "Lain", None).as_deref(), Some("31"));
        // 5. Nama sama persis setelah normalisasi (titik dan kurung dibuang).
        let dua = r#"<select><option value="41">CV. Uji (Pusat)</option><option value="42">PT Lain</option></select>"#;
        assert_eq!(html::resolve_rekanan_id(dua, "cv uji pusat", None).as_deref(), Some("41"));
        // 6. Saling memuat.
        assert_eq!(html::resolve_rekanan_id(dua, "PT Lain Jaya", None).as_deref(), Some("42"));
        // 7. Tidak ada yang cocok.
        assert_eq!(html::resolve_rekanan_id(dua, "Tidak Ada", None), None);
        // Nama kosong dan banyak opsi: berhenti tanpa hasil.
        assert_eq!(html::resolve_rekanan_id(dua, "  ", None), None);
    }

    #[test]
    fn rekanan_labels_dan_pesan_galat() {
        let form = r#"<select><option value="0">Pilih</option><option value="5">CV Alfa &amp; Beta</option></select>"#;
        assert_eq!(html::rekanan_labels(form), vec!["CV Alfa & Beta".to_string()]);
        // Seperti PHP: spasi dirapikan sebelum tanda baca dibuang, jadi spasi ganda tetap ada.
        assert_eq!(html::normalize_name(" cv. alfa ( beta ) "), "CV ALFA  BETA ");
        let pesan = rekanan_message("<html>tanpa rekanan</html>");
        assert!(pesan.starts_with("rekananId tidak ditemukan di form SPPBJ SPSE. Form SPPBJ tidak memuat"));
    }

    #[test]
    fn form_fields_input_checkbox_textarea_select() {
        let page = r#"
            <form id="formPesanan" method="post">
              <input type="hidden" name="authenticityToken" value="tok&amp;1">
              <input name="spk.nip_ppk_kontrak" value="">
              <input type="submit" name="simpan" value="Simpan">
              <input type="checkbox" name="cb_off" value="1">
              <input type="checkbox" name="cb_on" value="2" checked>
              <input type="radio" name="rd" value="x">
              <input name="tanpa_tipe" value="isi">
              <textarea name="content.catatan">Baris &lt;satu&gt;</textarea>
              <select name="spk.jenis"><option value="a">A</option><option value="b" selected>B</option><option value="c">C</option></select>
              <select name="spk.kosong"><option value="p">P</option><option value="q">Q</option></select>
            </form>
            <input name="di_luar" value="tidak">"#;
        let f = html::form_fields(page, Some("formPesanan"));
        let get = |k: &str| form_get(&f, k).map(str::to_string);
        assert_eq!(get("authenticityToken").as_deref(), Some("tok&1"));
        assert_eq!(get("spk.nip_ppk_kontrak").as_deref(), Some(""));
        assert_eq!(get("simpan"), None, "submit dilewati");
        assert_eq!(get("cb_off"), None, "checkbox tanpa checked dilewati");
        assert_eq!(get("cb_on").as_deref(), Some("2"));
        assert_eq!(get("rd"), None, "radio tanpa checked dilewati");
        assert_eq!(get("tanpa_tipe").as_deref(), Some("isi"), "tipe default text");
        assert_eq!(get("content.catatan").as_deref(), Some("Baris <satu>"));
        assert_eq!(get("spk.jenis").as_deref(), Some("b"), "opsi terpilih terakhir");
        assert_eq!(get("spk.kosong").as_deref(), Some("p"), "tanpa terpilih: opsi pertama");
        assert_eq!(get("di_luar"), None, "di luar form tidak ikut");

        // Tanpa form id: seluruh dokumen dipakai.
        let semua = html::form_fields(page, None);
        assert_eq!(form_get(&semua, "di_luar"), Some("tidak"));
    }

    #[test]
    fn form_fields_nama_ganda_ditimpa_di_posisi_awal() {
        let page = r#"<input name="a" value="1"><input name="b" value="2"><input name="a" value="3">"#;
        let f = html::form_fields(page, None);
        assert_eq!(f, vec![("a".to_string(), "3".to_string()), ("b".to_string(), "2".to_string())]);
    }

    #[test]
    fn nilai_kontrak_dan_parse_nilai() {
        let by_id = r#"<input id="nilaiKontrak_f" name="x" value="12.345.678,90">"#;
        assert_eq!(html::nilai_kontrak(by_id).as_deref(), Some("12.345.678,90"));
        let fallback = r#"<input name="spk.spk_nilai" value="1.000,5">"#;
        assert_eq!(html::nilai_kontrak(fallback).as_deref(), Some("1.000,5"));
        assert_eq!(html::nilai_kontrak("<p>kosong</p>"), None);
        assert_eq!(html::nilai_kontrak(r#"<input id="nilaiKontrak_f" value="">"#), None);

        assert_eq!(parse_nilai("12.345.678,90"), Some(12_345_678.9));
        assert_eq!(parse_nilai(" 1.000 "), Some(1000.0));
        assert_eq!(parse_nilai("0,00"), Some(0.0));
        assert_eq!(parse_nilai("abc"), None);
        assert_eq!(parse_nilai("INF"), None);
        assert_eq!(parse_nilai(""), None);
    }

    #[test]
    fn pesan_spse_diambil_tanpa_duplikat() {
        let page = r#"<div class="alert alert-danger">NIP <b>salah</b></div>
            <div class="x alert-warning">Tanggal kosong</div>
            <span class="error">NIP salah</span>
            <div class="alert-danger">NIP  salah</div>"#;
        assert_eq!(
            html::spse_user_messages(page),
            vec!["NIP salah".to_string(), "NIP  salah".to_string(), "Tanggal kosong".to_string()]
        );
    }

    #[test]
    fn format_tanggal_dan_waktu_penyelesaian() {
        assert_eq!(format_date(d(2026, 10, 9)), "09-10-2026");
        assert_eq!(format_date(None), "");
        assert_eq!(waktu_penyelesaian(d(2026, 10, 1), d(2026, 10, 10)), "10 Hari Kalender");
        assert_eq!(waktu_penyelesaian(d(2026, 10, 1), d(2026, 10, 1)), "1 Hari Kalender");
        assert_eq!(waktu_penyelesaian(d(2026, 10, 10), d(2026, 10, 1)), "-8 Hari Kalender");
        assert_eq!(waktu_penyelesaian(None, d(2026, 10, 1)), "");
    }

    #[test]
    fn multipart_urut_dan_berakhir_boundary() {
        let fields: Fields = vec![
            ("authenticityToken".into(), "tok-1".into()),
            ("simpan".into(), String::new()),
        ];
        let body = String::from_utf8(multipart_body(&fields)).unwrap();
        let expected = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"authenticityToken\"\r\n\r\ntok-1\r\n\
             --{b}\r\nContent-Disposition: form-data; name=\"simpan\"\r\n\r\n\r\n--{b}--\r\n",
            b = BOUNDARY
        );
        assert_eq!(body, expected);
    }

    #[test]
    fn merge_menimpa_dan_menambah() {
        let base: Fields = vec![("a".into(), "1".into()), ("b".into(), "2".into())];
        let out = merge(base, vec![("b", "B".into()), ("c", "C".into())]);
        assert_eq!(
            out,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "B".to_string()),
                ("c".to_string(), "C".to_string()),
            ]
        );
    }

    #[test]
    fn assert_save_ok_status_dan_body() {
        let resp = |status: u16, body: &str| Resp {
            status,
            url: String::new(),
            location: None,
            content_type: None,
            content_disposition: None,
            body: body.as_bytes().to_vec(),
        };
        assert!(assert_save_ok(&resp(302, ""), "SPK").is_ok());
        assert!(matches!(assert_save_ok(&resp(500, ""), "SPK"), Err(PushErr::Failed(m)) if m == "Simpan SPK gagal: HTTP 500"));
        assert!(matches!(
            assert_save_ok(&resp(200, "Error: Exception di server"), "SPPBJ"),
            Err(PushErr::Failed(m)) if m == "Simpan SPPBJ ditolak SPSE."
        ));
        assert!(assert_save_ok(&resp(200, "error saja"), "SPPBJ").is_ok());
    }

    #[test]
    fn wakil_direktur_lalu_nama() {
        let p = |direktur: Option<&str>, nama: Option<&str>| Penyedia {
            nama: nama.map(str::to_string),
            direktur: direktur.map(str::to_string),
            bank: None,
            norek: None,
        };
        assert_eq!(wakil(Some(&p(Some("Budi"), Some("CV X")))), "Budi");
        assert_eq!(wakil(Some(&p(Some("0"), Some("CV X")))), "CV X");
        assert_eq!(wakil(Some(&p(None, None))), "");
        assert_eq!(wakil(None), "");
    }

    #[test]
    fn settings_wajib_env_ppk() {
        // Nama variabel unik untuk test ini tidak dipakai; cukup pastikan pesan galat menyebut kunci yang kosong.
        std::env::remove_var("SPSE_PPK_NAMA");
        std::env::remove_var("SPSE_PPK_NIP");
        std::env::remove_var("SPSE_PPK_JABATAN");
        std::env::remove_var("SPSE_PPK_NO_SK");
        match settings() {
            Err(PushErr::Failed(m)) => assert!(m.contains("SPSE_PPK_NAMA") && m.contains("SPSE_PPK_NO_SK")),
            _ => panic!("harus gagal bila env PPK kosong"),
        }
    }

    #[test]
    fn push_lock_satu_per_kontrak() {
        let a = PushLock::acquire(900_001).expect("kunci pertama");
        assert!(PushLock::acquire(900_001).is_none(), "kontrak yang sama tidak boleh ganda");
        drop(a);
        assert!(PushLock::acquire(900_001).is_some(), "setelah dilepas bisa dipakai lagi");
    }
}
