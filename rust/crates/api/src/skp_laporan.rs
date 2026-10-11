//! Ekspor laporan capaian SKP bulanan ke PPTX: `GET /api/skp/laporan.pptx?tahun=&bulan=&feedback=&strategi=`.
//!
//! Data diambil dari master RHK (`skp_rhk`), realisasi per bulan (jumlah output jurnal per RHK),
//! kegiatan jurnal bulan laporan, dan foto jurnal dari disk. Penyusunan deck ada di `skp_pptx`.
//!
//! Foto bukti: diambil dari slot bukti per RHK per bulan (`skp_bukti`), seperti folder
//! `bukti/<BULAN>/rhkN*.jpg` di generator Python. Foto kegiatan jurnal tampil di lampiran.
//!
//! Pegawai diambil dari tabel `users` (nama, NIP, jabatan). Pangkat, unit kerja, dan data atasan
//! belum tersimpan di database, jadi dikosongkan. Nama instansi dari env `SKP_INSTANSI`.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::NaiveDate;
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media,
    require_auth,
    skp_jurnal::{self, gagal, COLLECTION, MODEL, VALIDATION_MESSAGE},
    skp_pptx::{self, Gambar, JurnalLaporan, Laporan, Orang, RhkLaporan},
    skp_atasan, skp_bukti, skp_rhk, AppState,
};

/// Nama instansi di cover dan footer, tetap untuk seluruh laporan.
const INSTANSI: &str = "Dinas Perumahan dan Kawasan Permukiman - Kabupaten Cianjur";
const MAX_FEEDBACK: usize = 2000;
const MAX_STRATEGI: usize = 10;
const PPTX_MIME: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

struct KegiatanRow {
    id: u64,
    tgl: NaiveDate,
    rhk: Option<i64>,
    kegiatan: String,
    output: Option<f64>,
    satuan: String,
    keterangan: String,
}

/// Realisasi per RHK per bulan dalam satu tahun: `(rhk, bulan) -> jumlah output`.
async fn realisasi_tahun(
    pool: &MySqlPool,
    user_id: u64,
    tahun: i32,
) -> Result<HashMap<(i64, i64), f64>, ApiError> {
    let (awal, akhir) = rentang_tahun(tahun)?;
    let rows = sqlx::query(
        // `MONTH(tanggal)` di SELECT harus persis sama dengan ekspresi di GROUP BY. Dengan CAST
        // di SELECT, ONLY_FULL_GROUP_BY (default MySQL 8) menolak query: error 1055.
        "SELECT CAST(rhk AS SIGNED) AS rhk, MONTH(tanggal) AS bulan, \
         COALESCE(SUM(output), 0) AS total FROM skp_jurnal_kegiatan \
         WHERE user_id = ? AND tanggal >= ? AND tanggal < ? AND rhk IS NOT NULL \
         GROUP BY rhk, MONTH(tanggal)",
    )
    .bind(user_id)
    .bind(awal)
    .bind(akhir)
    .fetch_all(pool)
    .await
    .map_err(crate::media::internal)?;

    let mut hasil = HashMap::new();
    for r in rows {
        let rhk: i64 = r.try_get("rhk").map_err(crate::media::internal)?;
        let bulan: i64 = r.try_get("bulan").map_err(crate::media::internal)?;
        let total: f64 = r.try_get("total").map_err(crate::media::internal)?;
        hasil.insert((rhk, bulan), total);
    }
    Ok(hasil)
}

/// Kegiatan jurnal pada bulan laporan, urut tanggal lalu id.
async fn kegiatan_bulan(
    pool: &MySqlPool,
    user_id: u64,
    tahun: i32,
    bulan: u32,
) -> Result<Vec<KegiatanRow>, ApiError> {
    let (awal, akhir) = rentang_bulan(tahun, bulan)?;
    let rows = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, tanggal, CAST(rhk AS SIGNED) AS rhk, kegiatan, output, \
         satuan, keterangan FROM skp_jurnal_kegiatan \
         WHERE user_id = ? AND tanggal >= ? AND tanggal < ? ORDER BY tanggal, id",
    )
    .bind(user_id)
    .bind(awal)
    .bind(akhir)
    .fetch_all(pool)
    .await
    .map_err(crate::media::internal)?;

    rows.iter()
        .map(|r| -> Result<KegiatanRow, sqlx::Error> {
            Ok(KegiatanRow {
                id: r.try_get("id")?,
                tgl: r.try_get("tanggal")?,
                rhk: r.try_get("rhk")?,
                kegiatan: r.try_get("kegiatan")?,
                output: r.try_get("output")?,
                satuan: r.try_get::<Option<String>, _>("satuan")?.unwrap_or_default(),
                keterangan: r.try_get::<Option<String>, _>("keterangan")?.unwrap_or_default(),
            })
        })
        .collect::<Result<_, _>>()
        .map_err(crate::media::internal)
}

/// Foto semua kegiatan yang diberikan, dibaca dari disk. Berkas yang hilang atau bukan gambar dilewati.
async fn foto_kegiatan(pool: &MySqlPool, ids: &[u64]) -> Result<HashMap<u64, Vec<Gambar>>, ApiError> {
    let mut hasil: HashMap<u64, Vec<Gambar>> = HashMap::new();
    if ids.is_empty() {
        return Ok(hasil);
    }
    let marks = vec!["?"; ids.len()].join(", ");
    let sql = format!(
        "SELECT CAST(id AS UNSIGNED) AS id, CAST(model_id AS UNSIGNED) AS model_id, file_name \
         FROM media WHERE model_type = ? AND collection_name = ? AND model_id IN ({marks}) \
         ORDER BY order_column, id"
    );
    let mut q = sqlx::query(&sql).bind(MODEL).bind(COLLECTION);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(pool).await.map_err(crate::media::internal)?;

    for r in rows {
        let media_id: u64 = r.try_get("id").map_err(crate::media::internal)?;
        let kegiatan_id: u64 = r.try_get("model_id").map_err(crate::media::internal)?;
        let file_name: String = r.try_get("file_name").map_err(crate::media::internal)?;
        if let Some(g) = gambar_disk(media_id, &file_name).await {
            hasil.entry(kegiatan_id).or_default().push(g);
        }
    }
    Ok(hasil)
}

/// Gambar dari disk. `None` bila berkas hilang atau bukan JPEG/PNG.
async fn gambar_disk(media_id: u64, file_name: &str) -> Option<Gambar> {
    let bytes = tokio::fs::read(media::media_dir(media_id).join(file_name)).await.ok()?;
    let mime = media::image_mime(&bytes)?;
    Some(Gambar {
        bytes,
        png: mime == "image/png",
    })
}

/// Foto bukti per RHK pada bulan laporan. Foto yang tidak terbaca dilewati.
async fn bukti_per_rhk(
    pool: &MySqlPool,
    user_id: u64,
    tahun: i32,
    bulan: u32,
) -> Result<HashMap<i64, Vec<Gambar>>, ApiError> {
    let mut hasil: HashMap<i64, Vec<Gambar>> = HashMap::new();
    for b in skp_bukti::baris_bulan(pool, user_id, tahun, bulan).await? {
        if let Some(g) = gambar_disk(b.media_id, &b.file_name).await {
            hasil.entry(b.rhk).or_default().push(g);
        }
    }
    Ok(hasil)
}

fn rentang_tahun(tahun: i32) -> Result<(NaiveDate, NaiveDate), ApiError> {
    match (NaiveDate::from_ymd_opt(tahun, 1, 1), NaiveDate::from_ymd_opt(tahun + 1, 1, 1)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(crate::media::internal("tahun tidak valid")),
    }
}

fn rentang_bulan(tahun: i32, bulan: u32) -> Result<(NaiveDate, NaiveDate), ApiError> {
    let (ny, nm) = if bulan == 12 { (tahun + 1, 1) } else { (tahun, bulan + 1) };
    match (NaiveDate::from_ymd_opt(tahun, bulan, 1), NaiveDate::from_ymd_opt(ny, nm, 1)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(crate::media::internal("bulan tidak valid")),
    }
}

/// Data pegawai dari tabel users. Pangkat dan unit kerja dari profil.
async fn pegawai(pool: &MySqlPool, user_id: u64) -> Result<Orang, ApiError> {
    let row = sqlx::query("SELECT name, nip, jabatan, unit_kerja, pangkat_golongan FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(crate::media::internal)?;
    let Some(r) = row else {
        return Ok(Orang::default());
    };
    let teks = |kolom: &str| -> Result<String, ApiError> {
        Ok(r.try_get::<Option<String>, _>(kolom).map_err(crate::media::internal)?.unwrap_or_default())
    };
    Ok(Orang {
        nama: r.try_get::<String, _>("name").map_err(crate::media::internal)?,
        nip: teks("nip")?,
        pangkat: teks("pangkat_golongan")?,
        jabatan: teks("jabatan")?,
        unit: teks("unit_kerja")?,
    })
}

/// Feedback dan strategi dari query. Strategi dipisah dengan `;`.
fn input_teks(query: &HashMap<String, String>) -> Result<(String, Vec<String>), ApiError> {
    let mut errors = BTreeMap::new();
    let feedback = query.get("feedback").map(|s| s.trim().to_string()).unwrap_or_default();
    if feedback.chars().count() > MAX_FEEDBACK {
        gagal(&mut errors, "feedback", "Umpan balik maksimal 2000 karakter.");
    }
    let strategi: Vec<String> = query
        .get("strategi")
        .map(|s| {
            s.split(';')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if strategi.len() > MAX_STRATEGI {
        gagal(&mut errors, "strategi", "Strategi maksimal 10 butir.");
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok((feedback, strategi))
}

/// Nama berkas yang aman di header: hanya huruf, angka, dan tanda hubung.
fn nama_berkas(tahun: i32, bulan: u32) -> String {
    format!("Laporan-SKP-{tahun}-{bulan:02}.pptx")
}

/// `GET /api/skp/laporan.pptx`.
pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (tahun, bulan) = skp_jurnal::periode(&query)?;
    let (feedback, strategi) = input_teks(&query)?;

    let daftar = skp_rhk::daftar(&state.pool, user.user_id, tahun).await?;
    if daftar.is_empty() {
        let mut errors = BTreeMap::new();
        gagal(&mut errors, "rhk", "Atur daftar RHK untuk tahun ini dulu.");
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }

    let realisasi = realisasi_tahun(&state.pool, user.user_id, tahun).await?;
    let kegiatan = kegiatan_bulan(&state.pool, user.user_id, tahun, bulan).await?;
    let ids: Vec<u64> = kegiatan.iter().map(|k| k.id).collect();
    let mut foto = foto_kegiatan(&state.pool, &ids).await?;

    let mut rhk: Vec<RhkLaporan> = daftar
        .iter()
        .map(|item| {
            let mut realisasi_per_bulan = [0.0f64; 12];
            for (b, nilai) in realisasi_per_bulan.iter_mut().enumerate() {
                *nilai = realisasi.get(&(item.no, (b + 1) as i64)).copied().unwrap_or(0.0);
            }
            RhkLaporan {
                no: item.no,
                rhk: item.rhk.clone(),
                satuan: item.satuan.clone(),
                target: item.target,
                rencana_per_bulan: item.rencana,
                realisasi_per_bulan,
                bukti: Vec::new(),
            }
        })
        .collect();

    // Bukti per RHK: foto slot bukti bulan ini (setara bukti/<BULAN>/rhkN*.jpg di Python).
    let mut bukti = bukti_per_rhk(&state.pool, user.user_id, tahun, bulan).await?;
    for r in rhk.iter_mut() {
        r.bukti = bukti.remove(&r.no).unwrap_or_default();
    }

    // Lampiran jurnal: semua foto kegiatan bulan ini (setara galeri di Python).
    let mut jurnal = Vec::with_capacity(kegiatan.len());
    for k in kegiatan {
        jurnal.push(JurnalLaporan {
            tgl: k.tgl,
            rhk: k.rhk,
            kegiatan: k.kegiatan,
            output: k.output,
            satuan: k.satuan,
            keterangan: k.keterangan,
            foto: foto.remove(&k.id).unwrap_or_default(),
        });
    }

    let pegawai = pegawai(&state.pool, user.user_id).await?;
    let atasan = skp_atasan::ambil(&state.pool, user.user_id).await?;

    let laporan = Laporan {
        instansi: INSTANSI.to_string(),
        tahun,
        bulan,
        pegawai,
        atasan,
        rhk,
        jurnal,
        feedback,
        strategi,
    };

    let bytes = skp_pptx::build_pptx(&laporan).map_err(crate::media::internal)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, PPTX_MIME.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", nama_berkas(tahun, bulan)),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response())
}
