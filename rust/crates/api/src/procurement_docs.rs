//! Dokumen paket SPSE: pemindaian daftar dokumen, impor ke berkas, dan unduhan ZIP.
//!
//! Setara `SpseProcurementController::packageDocuments`, `importPackageDocuments`, dan
//! `downloadPackageZip`, dengan `SpseDocumentScanner`, `SpseBerkasImportService`,
//! `SpseDocumentDownloader`, dan `SpseDocumentZipService`. Sesi dan HTTP ada di `procurement_spse`.
//!
//! Perbedaan dengan Laravel:
//! - Impor mengunduh dulu, baru menulis `tbl_berkas`. Laravel menulis baris dulu lalu menghapusnya
//!   bila unduhan gagal, sehingga Laravel meninggalkan pasangan audit `created` dan `deleted` serta
//!   notifikasi. Rust tidak menulis apa pun untuk unduhan yang gagal.
//! - ZIP dibuat di memori, bukan file sementara.
//! - Entitas HTML pada tautan memakai `kontrak::decode_entities` (entitas umum saja).
//! - Pemindaian mengambil halaman satu per satu, sama dengan Laravel. Tidak ada cache halaman.

use std::{collections::HashMap, collections::HashSet, io::Cursor, io::Write};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use shared::ApiError;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{
    berkas, changes, kontrak::decode_entities, media, notify::new_uuid,
    procurement_spse::{
        absolute_url, download_binary, fetch_page, full_url, internal, is_downloadable_binary,
        mb_take, php_empty, regex_once, require_session, strip_tags, url_decode, Session,
        SpseError,
    },
    require_auth,
    validation::Errors,
    AppState,
};

const MAX_DOCUMENTS: usize = 50;

/// Seksi lama `/nontender/{id}/{seksi}`. Di SPSE sekarang ini bukan file (selalu 404).
const LEGACY_SECTION_PATTERN: &str = r"(?i)/nontender/\d+/(pengumumanlelang|beritaacara|dokumenkualifikasi|suratpenawaran|administrasiteknis|dokumenharga|evaluasiteknis|persyaratankualifikasi)/?$";

/// Endpoint lama yang hanya diperiksa bila masih berupa file (`LEGACY_NONTENDER_ENDPOINTS`).
const LEGACY_NONTENDER: [(&str, &str, &str); 8] = [
    ("pengumumanlelang", "Summary Non Tender", "summary"),
    ("beritaacara", "Berita Acara Hasil Pengadaan", "berita_acara"),
    ("dokumenkualifikasi", "Dokumen Kualifikasi", "kualifikasi"),
    ("suratpenawaran", "Surat Penawaran", "surat_penawaran"),
    ("administrasiteknis", "Administrasi dan Teknis", "admin_teknis"),
    ("dokumenharga", "Dokumen Harga", "harga"),
    ("evaluasiteknis", "Evaluasi Teknis", "evaluasi_teknis"),
    ("persyaratankualifikasi", "Persyaratan Kualifikasi Lainnya", "persyaratan_kualifikasi"),
];

/// Dokumen yang ditemukan sebelum dicatat (`$doc` di PHP).
#[derive(Debug, Clone)]
struct Doc {
    url: String,
    label: String,
    kind: &'static str,
    doc_type: &'static str,
}

/// Dokumen pada hasil pindai (`pushDocument`).
struct Found {
    id: String,
    url: String,
    label: String,
    source_page: String,
    kind: &'static str,
    doc_type: &'static str,
}

impl Found {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "url": self.url,
            "label": self.label,
            "source_page": self.source_page,
            "kind": self.kind,
            "doc_type": self.doc_type,
        })
    }
}

/// Basis nama dan path dari URL (`parse_url(..., PHP_URL_PATH)`).
fn url_path(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => u.path().to_string(),
        Err(_) => url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_string(),
    }
}

/// `basename()` dari path. Garis miring di akhir diabaikan.
fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// `labelFromUrl`: nama file dari URL, atau `Dokumen SPSE`.
fn label_from_url(url: &str) -> String {
    let path = url_path(url);
    let name = basename(&path);
    if name.is_empty() {
        "Dokumen SPSE".to_string()
    } else {
        url_decode(&name)
    }
}

/// `classifyKind`: unduhan (`/dl/`, `/dlsec/`), atau dokumen generated (`viewpdfpl`, `cetak`).
fn classify_kind(url: &str) -> &'static str {
    if regex_once!(r"(?i)/(dl|dlsec)/").is_match(url) {
        "download"
    } else if regex_once!(r"(?i)viewpdfpl|cetak").is_match(url) {
        "generated"
    } else {
        "download"
    }
}

/// Anchor `<a href>` yang href-nya cocok dengan `href_pattern`: (href, label).
fn match_anchors(html: &str, href_pattern: &str) -> Vec<(String, String)> {
    let Ok(href_re) = regex::Regex::new(href_pattern) else {
        return Vec::new();
    };
    let anchor = regex_once!(r#"(?is)<a[^>]+href=["']([^"']+)["'][^>]*>(.*?)</a>"#);
    anchor
        .captures_iter(html)
        .filter_map(|m| {
            let href = decode_entities(m[1].trim());
            if href.is_empty() || !href_re.is_match(&href) {
                return None;
            }
            Some((href, normalize_label(&m[2])))
        })
        .collect()
}

/// Href pertama yang cocok (`extractFirstHref`).
fn first_href(html: &str, href_pattern: &str) -> Option<String> {
    match_anchors(html, href_pattern)
        .into_iter()
        .next()
        .map(|(href, _)| href)
}

/// Teks anchor: tag dibuang, spasi dirapikan, maksimal 255 karakter.
fn normalize_label(inner: &str) -> String {
    let stripped = strip_tags(inner);
    let text = regex_once!(r"\s+").replace_all(&stripped, " ");
    mb_take(text.trim(), 255)
}

/// `dedupeDocs`: satu entri per URL. Label yang lebih panjang (dalam byte) menggantikan.
fn dedupe(docs: Vec<Doc>) -> Vec<Doc> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, Doc> = std::collections::HashMap::new();
    for doc in docs {
        match map.get(&doc.url) {
            None => {
                order.push(doc.url.clone());
                map.insert(doc.url.clone(), doc);
            }
            Some(existing) if doc.label.len() > existing.label.len() => {
                map.insert(doc.url.clone(), doc);
            }
            Some(_) => {}
        }
    }
    order.into_iter().filter_map(|u| map.remove(&u)).collect()
}

fn label_or(label: &str, fallback: impl FnOnce() -> String) -> String {
    if label.is_empty() {
        fallback()
    } else {
        label.to_string()
    }
}

fn discover_nontender(html: &str) -> Vec<Doc> {
    let mut docs = Vec::new();
    for (url, label) in match_anchors(html, r"(?i)viewpdfpl") {
        docs.push(Doc {
            url,
            label: label_or(&label, || "Summary Non Tender".into()),
            kind: "generated",
            doc_type: "summary",
        });
    }
    for (url, label) in match_anchors(html, r"(?i)/dl/") {
        docs.push(Doc {
            label: label_or(&label, || label_from_url(&url)),
            url,
            kind: "download",
            doc_type: "dl",
        });
    }
    for (url, label) in match_anchors(html, r"(?i)/dlsec/") {
        docs.push(Doc {
            label: label_or(&label, || label_from_url(&url)),
            url,
            kind: "download",
            doc_type: "dlsec",
        });
    }
    dedupe(docs)
}

fn discover_penawaran(html: &str) -> Vec<Doc> {
    let mut docs = Vec::new();
    for (url, label) in match_anchors(html, r"(?i)cetaksuratpenawaranpeserta|cetak") {
        docs.push(Doc {
            url,
            label: label_or(&label, || "Surat Penawaran".into()),
            kind: "generated",
            doc_type: "surat_penawaran",
        });
    }
    for (url, _) in match_anchors(html, r"(?i)rincian_adminteknis") {
        docs.push(Doc {
            url,
            label: "Administrasi dan Teknis".into(),
            kind: "html_page",
            doc_type: "admin_teknis",
        });
    }
    for (url, _) in match_anchors(html, r"(?i)rincian_penawaran") {
        docs.push(Doc {
            url,
            label: "Harga".into(),
            kind: "html_page",
            doc_type: "harga",
        });
    }
    for (url, label) in match_anchors(html, r"(?i)/dl/") {
        docs.push(Doc {
            label: label_or(&label, || label_from_url(&url)),
            url,
            kind: "download",
            doc_type: "dl",
        });
    }
    dedupe(docs)
}

fn discover_kualifikasi(html: &str) -> Vec<Doc> {
    let mut docs = Vec::new();
    for (url, _) in match_anchors(html, r"(?i)cetakkualifikasipl") {
        docs.push(Doc {
            url,
            label: "Dokumen Kualifikasi".into(),
            kind: "generated",
            doc_type: "kualifikasi",
        });
    }
    for (url, label) in match_anchors(html, r"(?i)/dl/") {
        docs.push(Doc {
            label: label_or(&label, || label_from_url(&url)),
            url,
            kind: "download",
            doc_type: "dl_kualifikasi",
        });
    }
    dedupe(docs)
}

fn discover_evaluasi(html: &str) -> Vec<Doc> {
    let docs = match_anchors(html, r"(?i)/dlsec/|/dl/")
        .into_iter()
        .filter(|(_, label)| !label.is_empty())
        .map(|(url, label)| Doc {
            url,
            label,
            kind: "download",
            doc_type: "dlsec",
        })
        .collect();
    dedupe(docs)
}

fn discover_rincian(html: &str) -> Vec<Doc> {
    let docs = match_anchors(html, r"(?i)/dl/|/dlsec/|dokumennontender")
        .into_iter()
        .map(|(url, label)| Doc {
            label: label_or(&label, || label_from_url(&url)),
            url,
            kind: "download",
            doc_type: "dl_rincian",
        })
        .collect();
    dedupe(docs)
}

fn extract_generic(html: &str) -> Vec<Doc> {
    let docs = match_anchors(html, r"(?i)viewpdfpl|/dl/|/dlsec/|cetak|download|dokumennontender")
        .into_iter()
        .map(|(url, label)| Doc {
            label: label_or(&label, || label_from_url(&url)),
            kind: classify_kind(&url),
            url,
            doc_type: "generic",
        })
        .collect();
    dedupe(docs)
}

/// Tambahkan dokumen bila URL absolutnya belum ada (`pushDocument`).
fn push_document(
    s: &Session,
    docs: &mut Vec<Found>,
    seen: &mut HashSet<String>,
    doc: &Doc,
    source_page: &str,
) {
    let absolute = absolute_url(s, &doc.url);
    let id = format!("{:x}", Md5::digest(absolute.as_bytes()));
    if !seen.insert(id.clone()) {
        return;
    }
    docs.push(Found {
        id,
        url: absolute,
        label: mb_take(&doc.label, 255),
        source_page: source_page.to_string(),
        kind: doc.kind,
        doc_type: doc.doc_type,
    });
}

/// Jenis untuk semua dokumen yang diunduh dari SPSE.
const JENIS_DOKUMEN_SPSE: &str = "SPSE Dokumen";

async fn try_fetch(s: &Session, path: &str, referer: &str) -> Option<String> {
    fetch_page(s, path, Some(referer)).await.ok()
}

fn has_doc_type(docs: &[Found], doc_type: &str) -> bool {
    docs.iter().any(|d| d.doc_type == doc_type)
}

/// `scanNontender`: halaman nontender, penawaran, kualifikasi, evaluasi, rincian, lalu seksi lama.
async fn scan_nontender(s: &Session, kode: &str) -> Vec<Found> {
    let referer = "/beranda/nontender";
    let mut docs: Vec<Found> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut kual_url: Option<String> = None;
    let mut eval_url: Option<String> = None;
    let mut followups: Vec<Doc> = Vec::new();
    let kual_pat = r"(?i)kualifikasinontender/\d+";
    let eval_pat = r"(?i)evaluasinontender/\d+";

    // 1. Halaman nontender.
    let nontender_path = format!("/nontender/{kode}");
    if let Some(html) = try_fetch(s, &nontender_path, referer).await {
        for doc in discover_nontender(&html) {
            push_document(s, &mut docs, &mut seen, &doc, &nontender_path);
            if matches!(doc.doc_type, "admin_teknis" | "harga") {
                followups.push(doc);
            }
        }
        kual_url = first_href(&html, kual_pat);
        eval_url = first_href(&html, eval_pat);
    }

    // 2. Halaman penawaran peserta.
    let penawaran_path = format!("/pesertanontender/{kode}/penawaran");
    if let Some(html) = try_fetch(s, &penawaran_path, referer).await {
        for doc in discover_penawaran(&html) {
            push_document(s, &mut docs, &mut seen, &doc, &penawaran_path);
            if matches!(doc.doc_type, "admin_teknis" | "harga") {
                followups.push(doc);
            }
        }
        kual_url = kual_url.or_else(|| first_href(&html, kual_pat));
        if let Some(e) = first_href(&html, eval_pat) {
            eval_url = Some(e);
        }
    }

    // 3. Kualifikasi, atau evaluasi bila tidak ada kualifikasi.
    if let Some(kual) = kual_url.clone() {
        let kual_path = url_path(&kual);
        if let Some(html) = try_fetch(s, &kual_path, referer).await {
            for doc in discover_kualifikasi(&html) {
                push_document(s, &mut docs, &mut seen, &doc, &kual_path);
            }
        }
        if let Some(pid) = regex_once!(r"(?i)/kualifikasinontender/(\d+)/")
            .captures(&kual)
            .map(|m| m[1].to_string())
        {
            let eval_path = format!("/evaluasinontender/{pid}/detail");
            if let Some(html) = try_fetch(s, &eval_path, referer).await {
                for doc in discover_evaluasi(&html) {
                    push_document(s, &mut docs, &mut seen, &doc, &eval_path);
                }
            }
        }
    } else if let Some(eval) = eval_url {
        let eval_path = url_path(&eval);
        if let Some(html) = try_fetch(s, &eval_path, referer).await {
            for doc in discover_evaluasi(&html) {
                push_document(s, &mut docs, &mut seen, &doc, &eval_path);
            }
        }
    }

    // 4. Rincian admin teknis dan harga: file penyedia per dokumen.
    for followup in dedupe(followups) {
        let rincian_path = url_path(&followup.url);
        let Some(html) = try_fetch(s, &rincian_path, referer).await else {
            continue;
        };
        for doc in discover_rincian(&html) {
            push_document(s, &mut docs, &mut seen, &doc, &rincian_path);
        }
    }

    // 5. Seksi lama `/nontender/{id}/{seksi}`, hanya bila masih berupa file dan belum ada tipe yang sama.
    for (segment, label, doc_type) in LEGACY_NONTENDER {
        let path = format!("/nontender/{kode}/{segment}");
        if !is_downloadable_binary(s, &path, Some(referer)).await {
            continue;
        }
        if has_doc_type(&docs, doc_type) {
            continue;
        }
        let doc = Doc {
            url: path.clone(),
            label: label.to_string(),
            kind: "endpoint",
            doc_type,
        };
        push_document(s, &mut docs, &mut seen, &doc, &path);
    }

    docs.sort_by(|a, b| a.label.cmp(&b.label));
    docs
}

/// `scanTender`: tiga halaman tender, dengan tautan generik.
async fn scan_tender(s: &Session, kode: &str) -> Vec<Found> {
    let referer = "/home";
    let mut docs: Vec<Found> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for page in [
        format!("/tender/{kode}"),
        format!("/evaluasi/{kode}"),
        format!("/peserta/{kode}/penawaran"),
    ] {
        if let Some(html) = try_fetch(s, &page, referer).await {
            for doc in extract_generic(&html) {
                push_document(s, &mut docs, &mut seen, &doc, &page);
            }
        }
    }
    docs.sort_by(|a, b| a.label.cmp(&b.label));
    docs
}

/// `GET /api/procurement/spse/packages/{kode_paket}/documents`.
pub async fn package_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(kode_paket): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let session = require_session(&state.pool, user.user_id).await?;

    // Laravel membaca `jenis_paket` dari input (query string pada GET ini).
    let mut errors = Errors::default();
    let jenis = match query.get("jenis_paket").filter(|v| !v.is_empty()) {
        None => None,
        Some(s) if s == "pengadaan_langsung" || s == "tender_seleksi" => Some(s.clone()),
        Some(_) => {
            errors.add("jenis_paket", "The selected jenis paket is invalid.");
            None
        }
    };
    errors.finish()?;

    let kode = kode_paket.trim();
    if kode.is_empty() {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "kode_paket wajib diisi."));
    }
    let documents = if jenis.as_deref() == Some("tender_seleksi") {
        scan_tender(&session, kode).await
    } else {
        scan_nontender(&session, kode).await
    };

    Ok(Json(json!({
        "kode_paket": kode_paket,
        "count": documents.len(),
        "data": documents.iter().map(Found::json).collect::<Vec<_>>(),
    })))
}

// ---------------------------------------------------------------------------
// Unduhan (SpseDocumentDownloader)
// ---------------------------------------------------------------------------

struct FileInfo {
    body: Vec<u8>,
    filename: String,
}

/// `sanitizeFilename`: karakter terlarang menjadi `-`, tepi dipangkas, maksimal 200 karakter.
fn sanitize_filename(name: &str) -> String {
    let replaced = regex_once!(r#"[\\/:*?"<>|]+"#).replace_all(name, "-");
    let trimmed = replaced.trim_matches(|c: char| " \t\n\r\0\x0B.-".contains(c));
    mb_take(trimmed, 200)
}

fn has_extension(name: &str) -> bool {
    regex_once!(r"(?i)\.[a-z0-9]{2,5}$").is_match(name)
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "application/pdf" => ".pdf",
        "application/zip" | "application/x-zip-compressed" => ".zip",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "text/html" => ".html",
        _ => ".bin",
    }
}

/// `resolveMimeType`: header `Content-Type`, lalu tebakan dari isi.
fn resolve_mime(content_type: Option<&str>, body: &[u8]) -> String {
    if let Some(ct) = content_type.filter(|c| !c.is_empty()) {
        let mime = ct.split(';').next().unwrap_or("").trim().to_lowercase();
        if !mime.is_empty() && mime != "application/octet-stream" {
            return mime;
        }
    }
    if body.starts_with(b"%PDF") {
        return "application/pdf".to_string();
    }
    let head = String::from_utf8_lossy(&body[..body.len().min(1024)]);
    if regex_once!(r"(?i)^\s*(<!doctype html|<html)").is_match(&head) {
        return "text/html".to_string();
    }
    "application/octet-stream".to_string()
}

/// `resolveFilename`: `Content-Disposition`, lalu label, lalu nama dari URL.
fn resolve_filename(
    content_disposition: Option<&str>,
    url: &str,
    label: Option<&str>,
    mime: &str,
) -> String {
    if let Some(cd) = content_disposition {
        if let Some(m) = regex_once!(r#"(?i)filename\*?=(?:UTF-8'')?"?([^";]+)"?"#).captures(cd) {
            let from_header = url_decode(&m[1]);
            let from_header = from_header.trim();
            if !from_header.is_empty() {
                return sanitize_filename(from_header);
            }
        }
    }
    if let Some(label) = label.filter(|l| !php_empty(Some(*l))) {
        let mut from_label = sanitize_filename(label);
        if !has_extension(&from_label) {
            from_label.push_str(extension_for_mime(mime));
        }
        if !from_label.is_empty() {
            return from_label;
        }
    }
    let name = basename(&url_path(url));
    if !name.is_empty() && name != "/" {
        return sanitize_filename(&url_decode(&name));
    }
    format!("dokumen-spse{}", extension_for_mime(mime))
}

/// `SpseDocumentDownloader::download`.
async fn download_file(s: &Session, url: &str, label: Option<&str>) -> Result<FileInfo, SpseError> {
    let d = download_binary(s, url).await?;
    if d.body.is_empty() {
        return Err(SpseError::Failed("File SPSE kosong.".to_string()));
    }
    let mime = resolve_mime(d.content_type.as_deref(), &d.body);
    let filename = resolve_filename(
        d.content_disposition.as_deref(),
        &d.final_url,
        label,
        &mime,
    );
    Ok(FileInfo {
        body: d.body,
        filename,
    })
}

// ---------------------------------------------------------------------------
// Impor ke berkas (SpseBerkasImportService)
// ---------------------------------------------------------------------------

/// `POST /api/procurement/spse/packages/import-documents`.
pub async fn import_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let session = require_session(&state.pool, user.user_id).await?;
    let pool = &state.pool;

    let mut errors = Errors::default();
    let pekerjaan_id = match body.get("pekerjaan_id") {
        None | Some(Value::Null) => {
            errors.add("pekerjaan_id", "The pekerjaan id field is required.");
            0
        }
        Some(v) => {
            let parsed = match v {
                Value::Number(n) if n.is_i64() => n.as_i64(),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            match parsed {
                None => {
                    errors.add("pekerjaan_id", "The pekerjaan id field must be an integer.");
                    0
                }
                Some(id) => {
                    let n: i64 = sqlx::query_scalar(
                        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan WHERE id = ?",
                    )
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(internal)?;
                    if n == 0 {
                        errors.add("pekerjaan_id", "The selected pekerjaan id is invalid.");
                    }
                    id
                }
            }
        }
    };
    let kode_paket = required_string(&mut errors, &body, "kode_paket", 64);
    let documents = document_list(&mut errors, &body, true);
    errors.finish()?;

    let mut imported = 0;
    let mut failed = 0;
    let mut results: Vec<Value> = Vec::new();
    let request_url = full_url(&state, "/api/procurement/spse/packages/import-documents");

    for (index, doc) in documents.iter().enumerate() {
        let url = doc.url.clone();
        // Semua dokumen dari SPSE berjenis sama. Nilai lain ditolak supaya daftar jenis tidak membengkak.
        let jenis = doc.jenis_dokumen.clone().unwrap_or_default();
        let jenis = jenis.trim().to_string();
        if url.is_empty() {
            failed += 1;
            results.push(json!({ "index": index, "status": "failed", "url": url, "reason": "url kosong" }));
            continue;
        }
        if !jenis.eq_ignore_ascii_case(JENIS_DOKUMEN_SPSE) {
            failed += 1;
            results.push(json!({
                "index": index,
                "status": "failed",
                "url": url,
                "reason": format!("jenis_dokumen '{jenis}' tidak dikenal. Dokumen SPSE harus memakai '{JENIS_DOKUMEN_SPSE}'."),
            }));
            continue;
        }
        let jenis = JENIS_DOKUMEN_SPSE.to_string();
        if regex_once!(LEGACY_SECTION_PATTERN).is_match(&url) {
            failed += 1;
            results.push(json!({
                "index": index,
                "status": "failed",
                "url": url,
                "reason": "URL section SPSE (bukan file). Pakai link /dl, /dlsec, viewpdfpl, atau cetak*.",
            }));
            continue;
        }

        match import_one(&state, &headers, user.user_id, &session, pekerjaan_id, &jenis, &url, doc.label.as_deref(), &request_url).await {
            Ok(resource) => {
                imported += 1;
                results.push(json!({ "index": index, "status": "imported", "url": url, "berkas": resource }));
            }
            Err(reason) => {
                failed += 1;
                let reason = if reason.contains("HTTP 404") {
                    "SPSE unduh gagal: HTTP 404 (URL tidak ada atau butuh sesi berbeda).".to_string()
                } else {
                    reason
                };
                results.push(json!({ "index": index, "status": "failed", "url": url, "reason": reason }));
            }
        }
    }

    Ok(Json(json!({
        "message": format!("Import selesai: {imported} berhasil, {failed} gagal."),
        "kode_paket": kode_paket,
        "pekerjaan_id": pekerjaan_id,
        "imported": imported,
        "failed": failed,
        "results": results,
    }))
    .into_response())
}

/// Unduh satu dokumen lalu simpan sebagai berkas. Galat mengembalikan pesan untuk `reason`.
#[allow(clippy::too_many_arguments)]
async fn import_one(
    state: &AppState,
    headers: &HeaderMap,
    actor: u64,
    session: &Session,
    pekerjaan_id: i64,
    jenis: &str,
    url: &str,
    label: Option<&str>,
    request_url: &str,
) -> Result<Value, String> {
    let file = download_file(session, url, label)
        .await
        .map_err(|e| e.message())?;
    let stored_name = format!("{}-{}", new_uuid(), file.filename);
    let mime = media::mime_for_name(&stored_name);
    let upload = media::Upload {
        original_name: stored_name,
        bytes: file.body,
    };

    let mut tx = state.pool.begin().await.map_err(|e| {
        tracing::error!("import berkas: {e}");
        "Gagal menyimpan berkas.".to_string()
    })?;
    let id = sqlx::query(
        "INSERT INTO tbl_berkas (pekerjaan_id, jenis_dokumen, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(jenis)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!("import berkas: {e}");
        "Gagal menyimpan berkas.".to_string()
    })?
    .last_insert_id() as i64;
    let row = berkas::find_row(&mut *tx, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Gagal menyimpan berkas.".to_string())?;
    changes::log(
        &mut tx,
        headers,
        actor,
        &changes::BERKAS,
        "created",
        id,
        None,
        Some(berkas::attributes(&row)),
        Some(pekerjaan_id),
        request_url,
    )
    .await
    .map_err(|_| "Gagal menyimpan berkas.".to_string())?;
    let stored = media::attach(&mut tx, "App\\Models\\Berkas", id as u64, "berkas/dokumen", &upload, &mime, false)
        .await
        .map_err(|_| "Gagal menyimpan berkas.".to_string())?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        tracing::error!("import berkas commit: {e}");
        return Err("Gagal menyimpan berkas.".to_string());
    }

    let resource = berkas::resource(&state.pool, &state.app_url, &row, true)
        .await
        .map_err(|e| e.message)?;
    Ok(resource)
}

/// Dokumen dari body: `url`, `jenis_dokumen`, dan `label` opsional.
struct DocInput {
    url: String,
    jenis_dokumen: Option<String>,
    label: Option<String>,
}

fn required_string(errors: &mut Errors, body: &Value, key: &str, max: usize) -> String {
    match body.get(key) {
        None | Some(Value::Null) => {
            errors.add(key, format!("The {} field is required.", key.replace('_', " ")));
            String::new()
        }
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                errors.add(
                    key,
                    format!(
                        "The {} field must not be greater than {max} characters.",
                        key.replace('_', " ")
                    ),
                );
            }
            s.clone()
        }
        Some(_) => {
            errors.add(key, format!("The {} field must be a string.", key.replace('_', " ")));
            String::new()
        }
    }
}

/// Daftar `documents` (wajib, 1 sampai 50 item). `with_jenis` menambah `jenis_dokumen` wajib.
fn document_list(errors: &mut Errors, body: &Value, with_jenis: bool) -> Vec<DocInput> {
    let list = match body.get("documents") {
        None | Some(Value::Null) => {
            errors.add("documents", "The documents field is required.");
            return Vec::new();
        }
        Some(Value::Array(a)) => a,
        Some(_) => {
            errors.add("documents", "The documents field must be an array.");
            return Vec::new();
        }
    };
    if list.is_empty() {
        errors.add("documents", "The documents field must have at least 1 items.");
    }
    if list.len() > MAX_DOCUMENTS {
        errors.add(
            "documents",
            format!("The documents field must not have more than {MAX_DOCUMENTS} items."),
        );
    }
    list.iter()
        .enumerate()
        .map(|(i, d)| {
            let url = match d.get("url") {
                None | Some(Value::Null) => {
                    errors.add(&format!("documents.{i}.url"), format!("The documents.{i}.url field is required."));
                    String::new()
                }
                Some(Value::String(s)) => {
                    if s.chars().count() > 2000 {
                        errors.add(&format!("documents.{i}.url"), format!("The documents.{i}.url field must not be greater than 2000 characters."));
                    }
                    s.clone()
                }
                Some(_) => {
                    errors.add(&format!("documents.{i}.url"), format!("The documents.{i}.url field must be a string."));
                    String::new()
                }
            };
            let jenis_dokumen = if with_jenis {
                let v = required_string_at(errors, d, i, "jenis_dokumen", 255);
                Some(v)
            } else {
                None
            };
            let label = match d.get("label") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => {
                    if s.chars().count() > 255 {
                        errors.add(&format!("documents.{i}.label"), format!("The documents.{i}.label field must not be greater than 255 characters."));
                    }
                    Some(s.clone())
                }
                Some(_) => {
                    errors.add(&format!("documents.{i}.label"), format!("The documents.{i}.label field must be a string."));
                    None
                }
            };
            DocInput {
                url,
                jenis_dokumen,
                label,
            }
        })
        .collect()
}

fn required_string_at(errors: &mut Errors, doc: &Value, i: usize, key: &str, max: usize) -> String {
    let full = format!("documents.{i}.{key}");
    match doc.get(key) {
        None | Some(Value::Null) => {
            errors.add(&full, format!("The {full} field is required."));
            String::new()
        }
        Some(Value::String(s)) => {
            if s.chars().count() > max {
                errors.add(&full, format!("The {full} field must not be greater than {max} characters."));
            }
            s.clone()
        }
        Some(_) => {
            errors.add(&full, format!("The {full} field must be a string."));
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------
// ZIP (SpseDocumentZipService)
// ---------------------------------------------------------------------------

/// `uniqueEntryName`: `NN_nama`, dengan sufiks `_2`, `_3`, ... bila bentrok.
fn unique_entry_name(used: &mut HashSet<String>, index: usize, filename: &str) -> String {
    let filename = if filename.trim().is_empty() {
        "dokumen.pdf".to_string()
    } else {
        filename.to_string()
    };
    let filename = regex_once!(r#"[\\/:*?"<>|]+"#).replace_all(&filename, "-").into_owned();
    let candidate = format!("{index:02}_{filename}");
    if !used.contains(&candidate) {
        return candidate;
    }
    let (base, ext) = match filename.rfind('.') {
        Some(pos) if pos > 0 => (filename[..pos].to_string(), filename[pos..].to_string()),
        _ => (filename.clone(), String::new()),
    };
    let mut suffix = 2;
    loop {
        let candidate = format!("{index:02}_{base}_{suffix}{ext}");
        if !used.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Nama aman untuk header unduhan: karakter selain huruf, angka, `.`, `-`, dan `_` menjadi `_`.
fn safe_name_part(kode: &str) -> String {
    let t = regex_once!(r#"[\\/:*?"<>|]+"#).replace_all(kode, "_");
    if t.is_empty() {
        "paket".to_string()
    } else {
        t.into_owned()
    }
}

/// Nilai `filename=` untuk `Content-Disposition` (tanpa tanda kutip bila aman).
fn disposition_value(filename: &str) -> String {
    if filename.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        format!("attachment; filename={filename}")
    } else {
        format!("attachment; filename=\"{filename}\"")
    }
}

/// `POST /api/procurement/spse/packages/download-zip`.
pub async fn download_zip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let session = require_session(&state.pool, user.user_id).await?;

    let mut errors = Errors::default();
    let kode_paket = required_string(&mut errors, &body, "kode_paket", 64);
    let documents = document_list(&mut errors, &body, false);
    errors.finish()?;

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut used: HashSet<String> = HashSet::new();
    let mut added = 0usize;
    let mut failed: Vec<Value> = Vec::new();

    for (index, doc) in documents.iter().enumerate() {
        let url = doc.url.clone();
        let label = doc.label.clone();
        if url.is_empty() {
            failed.push(json!({ "url": url, "label": label.clone().unwrap_or_default(), "reason": "url kosong" }));
            continue;
        }
        match download_file(&session, &url, label.as_deref()).await {
            Ok(file) => {
                let entry = unique_entry_name(&mut used, index + 1, &file.filename);
                let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                zip.start_file(entry.clone(), options).map_err(internal)?;
                zip.write_all(&file.body).map_err(internal)?;
                used.insert(entry);
                added += 1;
            }
            Err(e) => failed.push(json!({
                "url": url,
                "label": label.clone().unwrap_or_default(),
                "reason": e.message(),
            })),
        }
    }

    if added == 0 {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "message": "Tidak ada dokumen berhasil diunduh.",
                "failed": failed.len(),
                "failed_details": failed,
            })),
        )
            .into_response());
    }

    if !failed.is_empty() {
        let report = serde_json::to_string_pretty(&failed).unwrap_or_else(|_| "[]".to_string());
        zip.start_file(
            "_gagal.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(internal)?;
        zip.write_all(report.as_bytes()).map_err(internal)?;
    }
    let archive = zip.finish().map_err(internal)?.into_inner();

    let filename = format!("spse_{}.zip", safe_name_part(&kode_paket));
    let res = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, disposition_value(&filename))
        .body(Body::from(archive))
        .map_err(internal)?;
    Ok(res)
}
