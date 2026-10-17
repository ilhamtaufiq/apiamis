//! Indeks pencarian pekerjaan berbasis tantivy (in-RAM), untuk peringkat relevansi dan toleransi typo.
//!
//! Indeks hanya menentukan kandidat dan urutannya. Hak akses (`access::restriction`), filter tahun,
//! dan data yang ditampilkan tetap diambil dari MySQL, sehingga indeks tidak pernah memperluas akses.
//! Indeks dibangun ulang bila sudah lebih tua dari `TTL_INDEKS`. Pekerjaan baru baru terlihat setelah
//! itu. Bila pembangunan gagal, pemanggil kembali ke pencarian LIKE.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use shared::ApiError;
use sqlx::{MySqlPool, Row};
use tantivy::{
    collector::TopDocs,
    doc,
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, RegexQuery},
    schema::{Field, Schema, Value, STORED, TEXT},
    Index, TantivyDocument, Term,
};
use tokio::sync::Mutex;

/// Umur maksimum indeks sebelum dibangun ulang saat pencarian berikutnya.
const TTL_INDEKS: Duration = Duration::from_secs(120);
/// Batas kata dari query yang dipakai. Sisanya diabaikan.
const MAKS_KATA: usize = 8;
/// Jumlah kandidat dari indeks. Urutan final diambil dari SQL setelah difilter.
pub const MAKS_KANDIDAT: usize = 200;

/// Satu pekerjaan beserta nama penyedia dari seluruh kontraknya.
#[derive(Debug, Clone)]
pub struct Dokumen {
    pub id: u64,
    pub nama_paket: String,
    pub kode_rekening: String,
    pub penyedia: String,
}

#[derive(Clone, Copy)]
struct Bidang {
    id: Field,
    nama_paket: Field,
    kode_rekening: Field,
    penyedia: Field,
}

fn skema() -> (Schema, Bidang) {
    let mut b = Schema::builder();
    let id = b.add_u64_field("id", STORED);
    let nama_paket = b.add_text_field("nama_paket", TEXT);
    let kode_rekening = b.add_text_field("kode_rekening", TEXT);
    let penyedia = b.add_text_field("penyedia", TEXT);
    (b.build(), Bidang { id, nama_paket, kode_rekening, penyedia })
}

/// Membuang akhiran partikel, kata ganti milik, dan imbuhan turunan Bahasa Indonesia dari satu kata.
/// Sederhana (tanpa kamus): cukup agar "pembangunan", "membangun", "dibangun", dan "bangunan" sama-sama
/// menjadi "bangun". Dipakai sama untuk dokumen dan query, jadi kesalahan kecil tetap konsisten.
pub fn stem(kata: &str) -> String {
    let asli = kata.to_lowercase();
    if asli.chars().count() <= 3 {
        return asli;
    }
    let mut w = asli.clone();
    for akhiran in ["lah", "kah", "tah", "pun", "nya", "ku", "mu"] {
        if let Some(r) = w.strip_suffix(akhiran).filter(|r| r.chars().count() >= 3) {
            w = r.to_string();
            break;
        }
    }
    for akhiran in ["kan", "an", "i"] {
        // "perbaikan" berakhiran "kan" secara huruf, tetapi akhiran sebenarnya "an" (dari "baik").
        if akhiran == "kan" && w.starts_with("per") {
            continue;
        }
        if let Some(r) = w.strip_suffix(akhiran).filter(|r| r.chars().count() >= 3) {
            w = r.to_string();
            break;
        }
    }
    for _ in 0..2 {
        match lepas_awalan(&w) {
            Some(r) => w = r,
            None => break,
        }
    }
    if w.chars().count() < 3 { asli } else { w }
}

/// Satu awalan dibuang bila sisanya cukup panjang. Nasal "meng-/peng-" + vokal dikembalikan menjadi "k",
/// dan "meny-/peny-" + vokal menjadi "s". Mengembalikan None bila tidak ada awalan yang cocok.
fn lepas_awalan(w: &str) -> Option<String> {
    const VOKAL: &str = "aiueo";
    let mulai_vokal = |r: &str| r.chars().next().is_some_and(|c| VOKAL.contains(c));
    // (awalan, pengganti bila diikuti vokal, batas panjang sisa)
    // Urutan penting: "per-" harus dicoba sebelum "pe-", dan "ber-"/"ter-" sebelum "be-"/"te-".
    let aturan: [(&str, &str, usize); 12] = [
        ("peng", "k", 3),
        ("meng", "k", 3),
        ("peny", "s", 3),
        ("meny", "s", 3),
        ("pem", "p", 3),
        ("mem", "p", 3),
        ("pen", "t", 3),
        ("men", "t", 3),
        ("per", "", 4),
        ("ber", "", 3),
        ("ter", "", 3),
        ("pe", "", 3),
    ];
    for (awalan, pengganti, minimal) in aturan {
        let Some(r) = w.strip_prefix(awalan) else { continue };
        if r.chars().count() < minimal {
            continue;
        }
        if mulai_vokal(r) {
            return Some(format!("{pengganti}{r}"));
        }
        // Konsonan: awalan dibuang, misal "membangun" -> "bangun", "pekerjaan" -> "kerja".
        return Some(r.to_string());
    }
    // "di-" dan "ke-" dibuang bila sisanya cukup panjang. "se-" sengaja tidak dipakai (merusak "sedia").
    for awalan in ["di", "ke"] {
        if let Some(r) = w.strip_prefix(awalan).filter(|r| r.chars().count() >= 4) {
            return Some(r.to_string());
        }
    }
    None
}

/// Teks dipecah menjadi kata, lalu tiap kata di-stem. Hasilnya dipakai untuk field TEXT.
fn stem_teks(teks: &str) -> String {
    teks.split(|c: char| !c.is_alphanumeric())
        .filter(|k| !k.is_empty())
        .map(stem)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Membangun indeks dari dokumen. Dipanggil di luar runtime async karena CPU-bound.
pub fn bangun(docs: &[Dokumen]) -> tantivy::Result<Index> {
    let (schema, f) = skema();
    let index = Index::create_in_ram(schema);
    let mut writer = index.writer(15_000_000)?;
    for d in docs {
        writer.add_document(doc!(
            f.id => d.id,
            f.nama_paket => stem_teks(&d.nama_paket),
            f.kode_rekening => d.kode_rekening.as_str(),
            f.penyedia => stem_teks(&d.penyedia),
        ))?;
    }
    writer.commit()?;
    Ok(index)
}

/// Kata dari query: huruf dan angka saja, lalu di-stem seperti dokumen.
fn kata_query(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|k| !k.is_empty())
        .map(stem)
        .take(MAKS_KATA)
        .collect()
}

/// Setiap kata harus cocok (AND). Pada tiap bidang, kata cocok bila awalan atau mirip (typo satu huruf
/// untuk kata lebih dari 4 karakter).
fn bangun_query(index_fields: Bidang, q: &str) -> Option<Box<dyn Query>> {
    let kata = kata_query(q);
    if kata.is_empty() {
        return None;
    }
    let mut per_kata: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    for k in &kata {
        let distance = if k.chars().count() > 4 { 1 } else { 0 };
        let mut pilihan: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for field in [index_fields.nama_paket, index_fields.kode_rekening, index_fields.penyedia] {
            let term = Term::from_field_text(field, k);
            pilihan.push((Occur::Should, Box::new(FuzzyTermQuery::new(term, distance, true))));
            if let Ok(prefix) = RegexQuery::from_pattern(&format!("{k}.*"), field) {
                pilihan.push((Occur::Should, Box::new(prefix)));
            }
        }
        per_kata.push((Occur::Must, Box::new(BooleanQuery::new(pilihan))));
    }
    Some(Box::new(BooleanQuery::new(per_kata)))
}

/// Mengembalikan id pekerjaan dengan skor tertinggi lebih dulu, paling banyak `limit`.
pub fn cari(index: &Index, q: &str, limit: usize) -> tantivy::Result<Vec<u64>> {
    let (_, f) = skema();
    let Some(query) = bangun_query(f, q) else {
        return Ok(Vec::new());
    };
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let hasil = searcher.search(&query, &TopDocs::with_limit(limit))?;
    let mut ids = Vec::with_capacity(hasil.len());
    for (_, alamat) in hasil {
        let dok: TantivyDocument = searcher.doc(alamat)?;
        if let Some(id) = dok.get_first(f.id).and_then(|v| v.as_u64()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

struct Snapshot {
    dibuat: Instant,
    index: Index,
}

static CACHE: OnceLock<Mutex<Option<Snapshot>>> = OnceLock::new();

/// Memuat seluruh pekerjaan beserta penyedianya dari MySQL.
async fn muat_dokumen(pool: &MySqlPool) -> Result<Vec<Dokumen>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(p.id AS SIGNED) AS id, p.nama_paket AS nama_paket, p.kode_rekening AS kode_rekening, \
         pen.nama AS penyedia \
         FROM tbl_pekerjaan p \
         LEFT JOIN kontrak_pekerjaan kp ON kp.pekerjaan_id = p.id \
         LEFT JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
         LEFT JOIN tbl_penyedia pen ON pen.id = k.id_penyedia",
    )
    .fetch_all(pool)
    .await
    .map_err(crate::media::internal)?;

    let mut per_id: std::collections::BTreeMap<u64, Dokumen> = std::collections::BTreeMap::new();
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(crate::media::internal)?;
        let id = id as u64;
        let entri = per_id.entry(id).or_insert_with(|| Dokumen {
            id,
            nama_paket: String::new(),
            kode_rekening: String::new(),
            penyedia: String::new(),
        });
        if entri.nama_paket.is_empty() {
            entri.nama_paket = r
                .try_get::<Option<String>, _>("nama_paket")
                .map_err(crate::media::internal)?
                .unwrap_or_default();
            entri.kode_rekening = r
                .try_get::<Option<String>, _>("kode_rekening")
                .map_err(crate::media::internal)?
                .unwrap_or_default();
        }
        if let Some(nama) = r.try_get::<Option<String>, _>("penyedia").map_err(crate::media::internal)? {
            if !entri.penyedia.is_empty() {
                entri.penyedia.push(' ');
            }
            entri.penyedia.push_str(&nama);
        }
    }
    Ok(per_id.into_values().collect())
}

/// Id kandidat pekerjaan untuk `q`, dari indeks yang masih segar. Indeks dibangun ulang bila kedaluwarsa.
pub async fn kandidat_pekerjaan(pool: &MySqlPool, q: &str) -> Result<Vec<u64>, ApiError> {
    let mut cache = CACHE.get_or_init(|| Mutex::new(None)).lock().await;
    let segar = cache.as_ref().is_some_and(|s| s.dibuat.elapsed() < TTL_INDEKS);
    if !segar {
        let dokumen = muat_dokumen(pool).await?;
        let index = tokio::task::spawn_blocking(move || bangun(&dokumen))
            .await
            .map_err(crate::media::internal)?
            .map_err(crate::media::internal)?;
        *cache = Some(Snapshot { dibuat: Instant::now(), index });
    }
    let index = &cache.as_ref().expect("indeks baru saja dibangun").index;
    let ids = cari(index, q, MAKS_KANDIDAT).map_err(crate::media::internal)?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contoh() -> Vec<Dokumen> {
        vec![
            Dokumen {
                id: 1,
                nama_paket: "Pembangunan Bendungan Cianjur Utara".into(),
                kode_rekening: "5.02.01".into(),
                penyedia: "PT Air Jernih".into(),
            },
            Dokumen {
                id: 2,
                nama_paket: "Rehabilitasi Jaringan Air Bersih Desa Sukamaju".into(),
                kode_rekening: "5.02.02".into(),
                penyedia: "CV Tirta Mandiri".into(),
            },
            Dokumen {
                id: 3,
                nama_paket: "Pengadaan Pipa Distribusi".into(),
                kode_rekening: "5.02.03".into(),
                penyedia: "PT Air Jernih".into(),
            },
        ]
    }

    #[test]
    fn stem_menyatukan_bentuk_turunan() {
        for (kata, akar) in [
            ("bangun", "bangun"),
            ("membangun", "bangun"),
            ("dibangun", "bangun"),
            ("bangunan", "bangun"),
            ("pembangunan", "bangun"),
            ("pekerjaan", "kerja"),
            ("mengerjakan", "kerja"),
            ("penyediaan", "sedia"),
            ("perbaikan", "baik"),
            ("mempertahankan", "tahan"),
            ("air", "air"),
        ] {
            assert_eq!(stem(kata), akar, "stem({kata})");
        }
    }

    #[test]
    fn cari_bentuk_turunan_menemukan_dokumen() {
        let index = bangun(&contoh()).unwrap();
        assert_eq!(cari(&index, "membangun", 10).unwrap(), vec![1]);
        assert_eq!(cari(&index, "pembangunan bendungan", 10).unwrap(), vec![1]);
    }

    #[test]
    fn cocok_dengan_awalan_kata() {
        let index = bangun(&contoh()).unwrap();
        assert_eq!(cari(&index, "bendu", 10).unwrap(), vec![1]);
    }

    #[test]
    fn toleran_satu_huruf_salah() {
        let index = bangun(&contoh()).unwrap();
        assert_eq!(cari(&index, "bendungan", 10).unwrap(), vec![1]);
        assert_eq!(cari(&index, "bendugan", 10).unwrap(), vec![1]);
    }

    #[test]
    fn mencari_lewat_nama_penyedia() {
        let index = bangun(&contoh()).unwrap();
        let mut ids = cari(&index, "jernih", 10).unwrap();
        ids.sort();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn semua_kata_harus_cocok() {
        let index = bangun(&contoh()).unwrap();
        // "air" ada di nama penyedia dokumen 3, "pipa" di nama paketnya.
        assert_eq!(cari(&index, "air pipa", 10).unwrap(), vec![3]);
        assert_eq!(cari(&index, "pipa bendungan", 10).unwrap(), Vec::<u64>::new());
        assert_eq!(cari(&index, "air bersih", 10).unwrap(), vec![2]);
    }

    #[test]
    fn query_kosong_dan_simbol_tidak_menghasilkan_apa_apa() {
        let index = bangun(&contoh()).unwrap();
        assert!(cari(&index, "  --  ", 10).unwrap().is_empty());
    }
}
