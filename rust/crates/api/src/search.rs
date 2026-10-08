//! `GET /api/search?q=&tahun=` (`SearchController::index`): pencarian global.
//!
//! Urutan dan bentuk hasil mengikuti Laravel: sembilan sumber (Pekerjaan, Kontrak, Penyedia,
//! Kegiatan, Desa, Dokumentasi, Penerima Manfaat, Output, Progress), lalu di-sort stabil
//! berdasarkan `type`. Yang ditiru:
//! - `q` kosong atau `"0"` (falsy PHP) memberi `data: []`. `q` dipangkas dan dibatasi 60 karakter
//!   setelah pengecekan itu, seperti `mb_substr(trim(...), 0, 60)`.
//! - `tahun` default tahun berjalan (UTC). `tahun=` kosong berarti tanpa filter tahun.
//! - MATCH ... AGAINST memakai query yang karakter operator boolean-nya diganti spasi.
//! - Cabang MATCH pada Kontrak TIDAK dibatasi `byUserRole` (sama seperti Laravel). Cabang LIKE
//!   pada nama paket dibatasi.
//! - `penerima.nik` dan `penerima.alamat` terenkripsi (cast `encrypted`). MATCH berjalan pada
//!   ciphertext, dan `alamat` didekripsi dengan `APP_KEY` untuk subtitle.
//! - Pada Pekerjaan, penyedia diambil dari kontrak pertama lewat `kontrak_pekerjaan` (pivot saja),
//!   sama seperti `$item->kontrak->first()`. Urutannya diset `kontrak_pekerjaan.id`.
//!
//! Perbedaan yang diketahui: Laravel tidak memakai `ORDER BY` pada pencarian dan memakai `LIMIT`,
//! sehingga himpunan 10 atau 15 baris pertama bisa berbeda bila MySQL memilih rencana lain.
//! Kolom `tbl_output` memerlukan FULLTEXT `ft_output_search` (migrasi 2026_04_13), yang belum
//! ada di DB lokal `apiamis`.

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use chrono::{Datelike, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    access::{self, Restriction},
    dashboard::{rows, Bind},
    desa::internal,
    foto::COLLECTION as FOTO_COLLECTION,
    media, php, require_auth, AppState,
};

const FOTO_MODEL: &str = "App\\Models\\Foto";

/// `escapeBool()`: karakter operator boolean-mode diganti spasi.
fn escape_bool(q: &str) -> String {
    q.chars()
        .map(|c| {
            if matches!(c, '\\' | '+' | '-' | '>' | '<' | '(' | ')' | '~' | '*' | '"' | '@') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// `whereHas('pekerjaan.kegiatan', tahun)` untuk tabel yang punya kolom `pekerjaan_id`.
/// `join_col` menunjuk kolom id pekerjaan pada tabel luar.
fn pekerjaan_kegiatan_exists(join_col: &str, tahun: Option<&str>) -> (String, Vec<Bind>) {
    let (tahun_sql, binds) = match tahun {
        Some(t) => (
            " AND tbl_kegiatan.tahun_anggaran = ?".to_string(),
            vec![Bind::Text(t.to_string())],
        ),
        None => (String::new(), vec![]),
    };
    (
        format!(
            "EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = {join_col} AND EXISTS \
             (SELECT * FROM tbl_kegiatan WHERE tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id{tahun_sql}))"
        ),
        binds,
    )
}

/// `tahun` yang truthy (string non-kosong dan bukan "0") atau None.
fn tahun_filter(tahun: &str) -> Option<&str> {
    if php::truthy(Some(tahun)) {
        Some(tahun)
    } else {
        None
    }
}

fn to_binds(r: &Restriction) -> Vec<Bind> {
    r.binds.iter().map(|b| Bind::Int(*b as i64)).collect()
}

struct Ctx<'a> {
    pool: &'a MySqlPool,
    app_url: &'a str,
    user_id: u64,
    roles: Vec<(u64, String)>,
    /// Query setelah `trim` dan `mb_substr(0, 60)`.
    q: String,
    /// Parameter `tahun` (default tahun berjalan).
    tahun: String,
}

impl Ctx<'_> {
    fn restriction(&self) -> Restriction {
        access::restriction(self.user_id, &self.roles, "tbl_pekerjaan")
    }

    fn like(&self) -> String {
        format!("%{}%", self.q)
    }

    fn match_q(&self) -> Bind {
        Bind::Text(escape_bool(&self.q))
    }
}

/// `GET /api/search`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let p = php::Params::parse(raw.as_deref());

    // `if (! $query)` dicek sebelum trim: "" dan "0" menghasilkan data kosong.
    let raw_q = p.get("q");
    if !php::truthy(raw_q) {
        return Ok(Json(json!({ "success": true, "data": [] })));
    }
    let trimmed = php::trim(raw_q.unwrap_or_default());
    let q: String = trimmed.chars().take(60).collect();
    let tahun = p
        .get("tahun")
        .map(str::to_string)
        .unwrap_or_else(|| Utc::now().year().to_string());

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let ctx = Ctx {
        pool: &state.pool,
        app_url: &state.app_url,
        user_id: user.user_id,
        roles,
        q,
        tahun,
    };

    let mut results: Vec<(&'static str, Value)> = Vec::new();
    results.extend(search_pekerjaan(&ctx).await.map_err(internal)?);
    results.extend(search_kontrak(&ctx).await.map_err(internal)?);
    results.extend(search_penyedia(&ctx).await.map_err(internal)?);
    results.extend(search_kegiatan(&ctx).await.map_err(internal)?);
    results.extend(search_desa(&ctx).await.map_err(internal)?);
    results.extend(search_foto(&ctx).await.map_err(internal)?);
    results.extend(search_penerima(&ctx).await?);
    results.extend(search_output(&ctx).await.map_err(internal)?);
    results.extend(search_progress(&ctx).await.map_err(internal)?);

    // `sortBy('type')` pada Collection: stabil, urut byte string.
    results.sort_by(|a, b| a.0.cmp(b.0));
    Ok(Json(json!({
        "success": true,
        "data": results.into_iter().map(|(_, v)| v).collect::<Vec<Value>>(),
    })))
}

fn opt_str(v: Option<String>) -> Value {
    v.map_or(Value::Null, Value::String)
}

async fn search_pekerjaan(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let mut binds = vec![ctx.match_q(), Bind::Text(ctx.like())];
    binds.extend(to_binds(&r));
    let mut sql = format!(
        "SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS id, tbl_pekerjaan.kode_rekening AS kode_rekening, \
         tbl_pekerjaan.nama_paket AS nama_paket, tbl_desa.n_desa AS n_desa, \
         tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_pekerjaan \
         LEFT JOIN tbl_desa ON tbl_desa.id = tbl_pekerjaan.desa_id \
         INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         WHERE (MATCH(tbl_pekerjaan.nama_paket, tbl_pekerjaan.kode_rekening) AGAINST(? IN BOOLEAN MODE) \
         OR EXISTS (SELECT * FROM tbl_kontrak INNER JOIN kontrak_pekerjaan \
         ON tbl_kontrak.id = kontrak_pekerjaan.kontrak_id \
         WHERE tbl_pekerjaan.id = kontrak_pekerjaan.pekerjaan_id AND EXISTS \
         (SELECT * FROM tbl_penyedia WHERE tbl_kontrak.id_penyedia = tbl_penyedia.id AND tbl_penyedia.nama LIKE ?))) \
         {}",
        r.sql
    );
    if let Some(t) = tahun {
        sql.push_str(" AND tbl_kegiatan.tahun_anggaran = ?");
        binds.push(Bind::Text(t.to_string()));
    }
    sql.push_str(" LIMIT 10");
    let base = rows(ctx.pool, &sql, &binds).await?;

    let ids: Vec<i64> = base.iter().map(|r| r.try_get("id")).collect::<Result<_, _>>()?;
    // Kontrak pertama per paket (pivot), beserta nama penyedianya.
    let mut first_kontrak_penyedia: std::collections::HashMap<i64, Option<String>> =
        std::collections::HashMap::new();
    if !ids.is_empty() {
        let placeholders = vec!["?"; ids.len()].join(",");
        let kontrak_rows = rows(
            ctx.pool,
            &format!(
                "SELECT CAST(kontrak_pekerjaan.pekerjaan_id AS SIGNED) AS pid, tbl_penyedia.nama AS nama \
                 FROM kontrak_pekerjaan \
                 INNER JOIN tbl_kontrak ON tbl_kontrak.id = kontrak_pekerjaan.kontrak_id \
                 LEFT JOIN tbl_penyedia ON tbl_penyedia.id = tbl_kontrak.id_penyedia \
                 WHERE kontrak_pekerjaan.pekerjaan_id IN ({placeholders}) ORDER BY kontrak_pekerjaan.id"
            ),
            &ids.iter().map(|i| Bind::Int(*i)).collect::<Vec<_>>(),
        )
        .await?;
        for kr in &kontrak_rows {
            let pid: i64 = kr.try_get("pid")?;
            first_kontrak_penyedia
                .entry(pid)
                .or_insert(kr.try_get::<Option<String>, _>("nama")?);
        }
    }

    let tahun_param = ctx.tahun.as_str();
    let mut out = Vec::with_capacity(base.len());
    for r in &base {
        let id: i64 = r.try_get("id")?;
        let kode: Option<String> = r.try_get("kode_rekening")?;
        let nama_paket: String = r.try_get("nama_paket")?;
        let desa: Option<String> = r.try_get("n_desa")?;
        let tahun_row: Option<String> = r.try_get("tahun")?;
        let penyedia = first_kontrak_penyedia.get(&id).cloned().flatten();

        let map_search = match &penyedia {
            Some(nama) if php::stripos_found(nama, &ctx.q) => nama.clone(),
            _ => nama_paket.clone(),
        };
        let mut subtitle = format!(
            "{} - {}",
            kode.unwrap_or_default(),
            desa.unwrap_or_default()
        );
        if let Some(nama) = &penyedia {
            subtitle.push_str(&format!(" - {nama}"));
        }
        let subtitle = php::trim_chars(&subtitle, &[' ', '-']).to_string();

        let mut map_url = format!("/map?search={}", php::rawurlencode(&map_search));
        if let Some(t) = tahun_filter(tahun_param) {
            map_url.push_str(&format!("&tahun={}", php::rawurlencode(t)));
        }
        out.push((
            "Pekerjaan",
            json!({
                "id": id,
                "type": "Pekerjaan",
                "title": nama_paket,
                "subtitle": subtitle,
                "tahun": opt_str(tahun_row),
                "url": format!("/pekerjaan/{id}"),
                "map_url": map_url,
            }),
        ));
    }
    Ok(out)
}

async fn search_kontrak(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let mut binds = vec![ctx.match_q()];
    binds.extend(to_binds(&r));
    binds.push(Bind::Text(ctx.like()));
    let mut sql = format!(
        "SELECT CAST(tbl_kontrak.id AS SIGNED) AS id, tbl_kontrak.spk AS spk, tbl_kontrak.kode_paket AS kode_paket, \
         CAST(tbl_kontrak.nilai_kontrak AS DOUBLE) AS nilai, tbl_penyedia.nama AS penyedia_nama, \
         tbl_pekerjaan.nama_paket AS nama_paket, tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_kontrak \
         LEFT JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_kontrak.id_pekerjaan \
         LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         LEFT JOIN tbl_penyedia ON tbl_penyedia.id = tbl_kontrak.id_penyedia \
         WHERE (MATCH(tbl_kontrak.spk, tbl_kontrak.spmk, tbl_kontrak.kode_paket) AGAINST(? IN BOOLEAN MODE) \
         OR EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = tbl_kontrak.id_pekerjaan{} \
         AND tbl_pekerjaan.nama_paket LIKE ?)) \
         AND EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = tbl_kontrak.id_pekerjaan AND EXISTS \
         (SELECT * FROM tbl_kegiatan WHERE tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id",
        r.sql
    );
    if let Some(t) = tahun {
        sql.push_str(" AND tbl_kegiatan.tahun_anggaran = ?");
        binds.push(Bind::Text(t.to_string()));
    }
    sql.push_str(")) LIMIT 15");

    let out = rows(ctx.pool, &sql, &binds)
        .await?
        .iter()
        .map(|r| -> Result<(&'static str, Value), sqlx::Error> {
            let id: i64 = r.try_get("id")?;
            let spk: Option<String> = r.try_get("spk")?;
            let kode: Option<String> = r.try_get("kode_paket")?;
            let nilai: Option<f64> = r.try_get("nilai")?;
            let penyedia: Option<String> = r.try_get("penyedia_nama")?;
            let nama_paket: Option<String> = r.try_get("nama_paket")?;
            let tahun: Option<String> = r.try_get("tahun")?;
            Ok((
                "Kontrak",
                json!({
                    "id": id,
                    "type": "Kontrak",
                    "title": match spk { Some(s) => Value::String(s), None => opt_str(kode) },
                    "subtitle": nama_paket.unwrap_or_else(|| "N/A".into()),
                    "penyedia": penyedia.unwrap_or_else(|| "N/A".into()),
                    "nilai": nilai.map_or(Value::Null, |n| json!(n)),
                    "tahun": opt_str(tahun),
                    "url": format!("/kontrak/{id}"),
                }),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

async fn search_penyedia(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let binds = vec![ctx.match_q()];
    let out = rows(
        ctx.pool,
        "SELECT CAST(id AS SIGNED) AS id, nama, direktur FROM tbl_penyedia \
         WHERE MATCH(tbl_penyedia.nama, tbl_penyedia.direktur) AGAINST(? IN BOOLEAN MODE) LIMIT 5",
        &binds,
    )
    .await?
    .iter()
    .map(|r| -> Result<(&'static str, Value), sqlx::Error> {
        let id: i64 = r.try_get("id")?;
        let nama: String = r.try_get("nama")?;
        let direktur: String = r.try_get("direktur")?;
        Ok((
            "Penyedia",
            json!({
                "id": id,
                "type": "Penyedia",
                "title": nama,
                "subtitle": format!("Direktur: {direktur}"),
                "url": format!("/penyedia/{id}"),
                "map_url": format!("/map?search={}", php::rawurlencode(&nama)),
            }),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

async fn search_kegiatan(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let binds = vec![ctx.match_q()];
    let out = rows(
        ctx.pool,
        "SELECT CAST(id AS SIGNED) AS id, nama_kegiatan, nama_sub_kegiatan FROM tbl_kegiatan \
         WHERE MATCH(tbl_kegiatan.nama_kegiatan, tbl_kegiatan.nama_sub_kegiatan, tbl_kegiatan.nama_program) \
         AGAINST(? IN BOOLEAN MODE) LIMIT 5",
        &binds,
    )
    .await?
    .iter()
    .map(|r| -> Result<(&'static str, Value), sqlx::Error> {
        let id: i64 = r.try_get("id")?;
        Ok((
            "Kegiatan",
            json!({
                "id": id,
                "type": "Kegiatan",
                "title": opt_str(r.try_get("nama_kegiatan")?),
                "subtitle": opt_str(r.try_get("nama_sub_kegiatan")?),
                "url": format!("/kegiatan/{id}"),
            }),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

async fn search_desa(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let binds = vec![ctx.match_q()];
    let out = rows(
        ctx.pool,
        "SELECT CAST(tbl_desa.id AS SIGNED) AS id, tbl_desa.n_desa AS n_desa, tbl_kecamatan.n_kec AS n_kec \
         FROM tbl_desa LEFT JOIN tbl_kecamatan ON tbl_kecamatan.id = tbl_desa.kecamatan_id \
         WHERE MATCH(tbl_desa.n_desa) AGAINST(? IN BOOLEAN MODE) LIMIT 5",
        &binds,
    )
    .await?
    .iter()
    .map(|r| -> Result<(&'static str, Value), sqlx::Error> {
        let id: i64 = r.try_get("id")?;
        let n_desa: Option<String> = r.try_get("n_desa")?;
        let n_kec: Option<String> = r.try_get("n_kec")?;
        Ok((
            "Desa",
            json!({
                "id": id,
                "type": "Desa",
                "title": format!("Desa {}", n_desa.unwrap_or_default()),
                "subtitle": format!("Kec. {}", n_kec.unwrap_or_default()),
                "url": format!("/desa/{id}"),
            }),
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

async fn search_foto(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let (exists_sql, exists_binds) = pekerjaan_kegiatan_exists("tbl_foto.pekerjaan_id", tahun);
    // Urutan `?`: keterangan LIKE, nama_paket LIKE (sesudah restriction), lalu tahun.
    let mut binds = vec![Bind::Text(ctx.like())];
    binds.extend(to_binds(&r));
    binds.push(Bind::Text(ctx.like()));
    binds.extend(exists_binds);
    let sql = format!(
        "SELECT CAST(tbl_foto.id AS SIGNED) AS id, tbl_foto.keterangan AS keterangan, \
         CAST(tbl_foto.pekerjaan_id AS SIGNED) AS pekerjaan_id, tbl_pekerjaan.nama_paket AS nama_paket, \
         tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_foto \
         LEFT JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_foto.pekerjaan_id \
         LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         WHERE (tbl_foto.keterangan LIKE ? OR EXISTS (SELECT * FROM tbl_pekerjaan \
         WHERE tbl_foto.pekerjaan_id = tbl_pekerjaan.id{} AND tbl_pekerjaan.nama_paket LIKE ?)) \
         AND {exists_sql} LIMIT 10",
        r.sql
    );

    let mut out = Vec::new();
    for row in rows(ctx.pool, &sql, &binds).await? {
        let id: i64 = row.try_get("id")?;
        let keterangan: Option<String> = row.try_get("keterangan")?;
        let pekerjaan_id: Option<i64> = row.try_get("pekerjaan_id")?;
        let nama_paket: Option<String> = row.try_get("nama_paket")?;
        let tahun_row: Option<String> = row.try_get("tahun")?;
        let (image_url, _thumb) =
            media::first_urls(ctx.pool, ctx.app_url, FOTO_MODEL, id as u64, FOTO_COLLECTION).await?;
        out.push((
            "Dokumentasi",
            json!({
                "id": id,
                "type": "Dokumentasi",
                "title": format!("Dokumentasi: {}", keterangan.unwrap_or_else(|| "Tanpa Keterangan".into())),
                "subtitle": format!("Pekerjaan: {}", nama_paket.unwrap_or_default()),
                "tahun": opt_str(tahun_row),
                "image_url": image_url,
                "url": format!("/pekerjaan/{}", pekerjaan_id.map_or(String::new(), |p| p.to_string())),
            }),
        ));
    }
    Ok(out)
}

async fn search_penerima(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, ApiError> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let (exists_sql, exists_binds) = pekerjaan_kegiatan_exists("tbl_penerima.pekerjaan_id", tahun);
    let mut binds = vec![ctx.match_q()];
    binds.extend(to_binds(&r));
    binds.push(Bind::Text(ctx.like()));
    binds.extend(exists_binds);
    let sql = format!(
        "SELECT CAST(tbl_penerima.id AS SIGNED) AS id, tbl_penerima.nama AS nama, tbl_penerima.alamat AS alamat, \
         CAST(tbl_penerima.pekerjaan_id AS SIGNED) AS pekerjaan_id, tbl_pekerjaan.nama_paket AS nama_paket, \
         tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_penerima \
         LEFT JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_penerima.pekerjaan_id \
         LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         WHERE (MATCH(tbl_penerima.nama, tbl_penerima.nik, tbl_penerima.alamat) AGAINST(? IN BOOLEAN MODE) \
         OR EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_penerima.pekerjaan_id = tbl_pekerjaan.id{} \
         AND tbl_pekerjaan.nama_paket LIKE ?)) AND {exists_sql} LIMIT 10",
        r.sql
    );

    let mut out = Vec::new();
    for row in rows(ctx.pool, &sql, &binds).await.map_err(internal)? {
        let id: i64 = row.try_get("id").map_err(internal)?;
        let nama: String = row.try_get("nama").map_err(internal)?;
        let alamat: Option<String> = row.try_get("alamat").map_err(internal)?;
        let pekerjaan_id: Option<i64> = row.try_get("pekerjaan_id").map_err(internal)?;
        let nama_paket: Option<String> = row.try_get("nama_paket").map_err(internal)?;
        let tahun_row: Option<String> = row.try_get("tahun").map_err(internal)?;
        let alamat = match alamat {
            Some(payload) => decrypt(&payload)?,
            None => String::new(),
        };
        out.push((
            "Penerima Manfaat",
            json!({
                "id": id,
                "type": "Penerima Manfaat",
                "title": format!("Penerima: {nama}"),
                "subtitle": format!(
                    "Alamat: {alamat} | Pekerjaan: {}",
                    nama_paket.unwrap_or_default()
                ),
                "tahun": opt_str(tahun_row),
                "url": format!("/pekerjaan/{}", pekerjaan_id.map_or(String::new(), |p| p.to_string())),
            }),
        ));
    }
    Ok(out)
}

fn decrypt(payload: &str) -> Result<String, ApiError> {
    let fail = |msg: String| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg);
    let raw = std::env::var("APP_KEY")
        .map_err(|_| fail("APP_KEY belum di-set: alamat penerima tidak bisa dibaca".into()))?;
    let key = crate::crypt::key_from_app_key(&raw)
        .map_err(|e| fail(format!("APP_KEY tidak valid: {e:?}")))?;
    crate::crypt::decrypt_string(&key, payload)
        .map_err(|e| fail(format!("gagal mendekripsi: {e:?}")))
}

async fn search_output(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let (exists_sql, exists_binds) = pekerjaan_kegiatan_exists("tbl_output.pekerjaan_id", tahun);
    let mut binds = vec![ctx.match_q()];
    binds.extend(to_binds(&r));
    binds.push(Bind::Text(ctx.like()));
    binds.extend(exists_binds);
    let sql = format!(
        "SELECT CAST(tbl_output.id AS SIGNED) AS id, tbl_output.komponen AS komponen, \
         CAST(tbl_output.volume AS CHAR) AS volume, tbl_output.satuan AS satuan, \
         CAST(tbl_output.pekerjaan_id AS SIGNED) AS pekerjaan_id, tbl_pekerjaan.nama_paket AS nama_paket, \
         tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_output \
         LEFT JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_output.pekerjaan_id \
         LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         WHERE (MATCH(tbl_output.komponen, tbl_output.satuan) AGAINST(? IN BOOLEAN MODE) \
         OR EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_output.pekerjaan_id = tbl_pekerjaan.id{} \
         AND tbl_pekerjaan.nama_paket LIKE ?)) AND {exists_sql} LIMIT 10",
        r.sql
    );

    let out = rows(ctx.pool, &sql, &binds)
        .await?
        .iter()
        .map(|row| -> Result<(&'static str, Value), sqlx::Error> {
            let id: i64 = row.try_get("id")?;
            let komponen: String = row.try_get("komponen")?;
            let volume: Option<String> = row.try_get("volume")?;
            let satuan: String = row.try_get("satuan")?;
            let pekerjaan_id: Option<i64> = row.try_get("pekerjaan_id")?;
            let nama_paket: Option<String> = row.try_get("nama_paket")?;
            let tahun_row: Option<String> = row.try_get("tahun")?;
            Ok((
                "Output",
                json!({
                    "id": id,
                    "type": "Output",
                    "title": format!("Output: {komponen}"),
                    "subtitle": format!(
                        "Volume: {} {satuan} | Pekerjaan: {}",
                        volume.unwrap_or_default(),
                        nama_paket.unwrap_or_default()
                    ),
                    "tahun": opt_str(tahun_row),
                    "url": format!("/pekerjaan/{}", pekerjaan_id.map_or(String::new(), |p| p.to_string())),
                }),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

async fn search_progress(ctx: &Ctx<'_>) -> Result<Vec<(&'static str, Value)>, sqlx::Error> {
    let r = ctx.restriction();
    let tahun = tahun_filter(&ctx.tahun);
    let (exists_sql, exists_binds) = pekerjaan_kegiatan_exists("tbl_progress.pekerjaan_id", tahun);
    // Urutan `?`: content LIKE, nama_paket LIKE (sesudah restriction), lalu tahun.
    let mut binds = vec![Bind::Text(ctx.like())];
    binds.extend(to_binds(&r));
    binds.push(Bind::Text(ctx.like()));
    binds.extend(exists_binds);
    let sql = format!(
        "SELECT CAST(tbl_progress.id AS SIGNED) AS id, CAST(tbl_progress.pekerjaan_id AS SIGNED) AS pekerjaan_id, \
         tbl_pekerjaan.nama_paket AS nama_paket, tbl_kegiatan.tahun_anggaran AS tahun \
         FROM tbl_progress \
         LEFT JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_progress.pekerjaan_id \
         LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
         WHERE (tbl_progress.content LIKE ? OR EXISTS (SELECT * FROM tbl_pekerjaan \
         WHERE tbl_progress.pekerjaan_id = tbl_pekerjaan.id{} AND tbl_pekerjaan.nama_paket LIKE ?)) \
         AND EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_progress.pekerjaan_id = tbl_pekerjaan.id) \
         AND {exists_sql} LIMIT 10",
        r.sql
    );
    let out = rows(ctx.pool, &sql, &binds)
        .await?
        .iter()
        .map(|row| -> Result<(&'static str, Value), sqlx::Error> {
            let id: i64 = row.try_get("id")?;
            let pekerjaan_id: Option<i64> = row.try_get("pekerjaan_id")?;
            let nama_paket: Option<String> = row.try_get("nama_paket")?;
            let tahun_row: Option<String> = row.try_get("tahun")?;
            Ok((
                "Progress",
                json!({
                    "id": id,
                    "type": "Progress",
                    "title": "Progress Log Entry",
                    "subtitle": format!("Pekerjaan: {}", nama_paket.unwrap_or_default()),
                    "tahun": opt_str(tahun_row),
                    "url": format!("/pekerjaan/{}", pekerjaan_id.map_or(String::new(), |p| p.to_string())),
                }),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_bool_replaces_operators_with_space() {
        assert_eq!(escape_bool("a+b-c(d)\"e\"@f*"), "a b c d  e  f ");
        assert_eq!(escape_bool("plain words"), "plain words");
    }

    #[test]
    fn tahun_zero_is_no_filter() {
        assert_eq!(tahun_filter("0"), None);
        assert_eq!(tahun_filter(""), None);
        assert_eq!(tahun_filter("2026"), Some("2026"));
    }

    #[test]
    fn kegiatan_exists_binds_tahun_only_when_set() {
        let (sql, binds) = pekerjaan_kegiatan_exists("tbl_foto.pekerjaan_id", None);
        assert!(!sql.contains("tahun_anggaran"));
        assert!(binds.is_empty());
        let (sql, binds) = pekerjaan_kegiatan_exists("tbl_foto.pekerjaan_id", Some("2026"));
        assert!(sql.contains("tbl_kegiatan.tahun_anggaran = ?"));
        assert_eq!(binds, vec![Bind::Text("2026".into())]);
    }

}
