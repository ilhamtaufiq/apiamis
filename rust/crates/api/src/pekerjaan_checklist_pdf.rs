//! Ekspor PDF checklist pekerjaan (`PekerjaanChecklistController::exportPdf`, Dompdf).
//!
//! Tidak ada mesin HTML ke PDF di Rust, jadi tabel digambar langsung sebagai PDF 1.4 dengan font
//! standar Helvetica dan Helvetica-Bold (WinAnsi). Font standar tidak perlu disematkan, sehingga
//! tidak ada berkas font dan tidak ada dependensi baru. Tata letak mengikuti
//! `exports/pekerjaan-checklist-pdf.blade.php`: A4 landscape, judul, baris meta, tabel dengan
//! border, header berulang di tiap halaman, baris genap diberi warna latar, dan kolom dari indeks 3
//! (serta kolom No) rata tengah.
//!
//! Lebar teks dihitung dari tabel lebar AFM Helvetica, sehingga pemenggalan baris sesuai isi.
//! Helvetica-Bold tidak dihitung terpisah; lebarnya diperkirakan dengan faktor 1,1 (lebih lebar
//! dari yang sebenarnya, jadi tidak ada teks yang keluar dari sel).

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
};
use shared::ApiError;

use crate::{
    checklist::require_full_access, pekerjaan_checklist_write::checklist_table,
    pekerjaan_checklist_write::ChecklistTable, require_auth, AppState,
};

const PAGE_W: f64 = 841.89;
const PAGE_H: f64 = 595.28;
const MARGIN: f64 = 28.0;
const TITLE_PT: f64 = 14.0;
const BODY_PT: f64 = 8.0;
const META_PT: f64 = 8.0;
/// Padding sel Blade: `4px 5px` (sekitar 3pt vertikal, 3,75pt horizontal).
const PAD_X: f64 = 3.75;
const PAD_Y: f64 = 3.0;
const COL_MAX: f64 = 180.0;
const COL_MIN: f64 = 36.0;
const BOLD_FACTOR: f64 = 1.1;
/// Warna dari Blade: header `#f3f4f6`, baris genap `#fafafa`, border `#ccc`, teks `#111`.
const HEAD_FILL: f64 = 0.953;
const ZEBRA_FILL: f64 = 0.980;
const BORDER_GRAY: f64 = 0.8;
const TEXT_GRAY: f64 = 0.067;

/// Lebar glyph Helvetica (per 1000 em) untuk ASCII 32..=126.
const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32-47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 48-57
    278, 278, 584, 584, 584, 556, 1015, // 58-64
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // 65-77
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // 78-90
    278, 278, 278, 469, 556, 333, // 91-96
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, // 97-122
    334, 260, 334, 584, // 123-126
];

/// Baris layout satu sel: teks, posisi kiri, lebar, dan rata tengah atau kiri.
struct Span<'a> {
    text: &'a str,
    x: f64,
    w: f64,
    center: bool,
}

fn glyph_width(c: char) -> f64 {
    match c {
        ' '..='~' => f64::from(HELVETICA_WIDTHS[c as usize - 32]),
        '·' => 278.0,
        '™' | '‰' => 1000.0,
        _ => 556.0,
    }
}

/// Lebar teks dalam pt.
fn measure(s: &str, pt: f64, bold: bool) -> f64 {
    let units: f64 = s.chars().map(glyph_width).sum();
    units / 1000.0 * pt * if bold { BOLD_FACTOR } else { 1.0 }
}

/// Pecah teks menjadi baris yang muat di `max_w`. Kata yang lebih panjang dari satu baris dipenggal per karakter.
fn wrap(text: &str, max_w: f64, pt: f64, bold: bool) -> Vec<String> {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in clean.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if measure(&candidate, pt, bold) <= max_w {
            cur = candidate;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if measure(word, pt, bold) <= max_w {
            cur = word.to_string();
            continue;
        }
        for ch in word.chars() {
            let mut next = cur.clone();
            next.push(ch);
            if !cur.is_empty() && measure(&next, pt, bold) > max_w {
                lines.push(std::mem::take(&mut cur));
                cur.push(ch);
            } else {
                cur = next;
            }
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Karakter di luar ASCII dan Latin-1 yang punya kode di WinAnsi (rentang 0x80-0x9F).
const WINANSI_HIGH: [(char, u8); 27] = [
    ('€', 0x80),
    ('‚', 0x82),
    ('ƒ', 0x83),
    ('„', 0x84),
    ('…', 0x85),
    ('†', 0x86),
    ('‡', 0x87),
    ('ˆ', 0x88),
    ('‰', 0x89),
    ('Š', 0x8A),
    ('‹', 0x8B),
    ('Œ', 0x8C),
    ('Ž', 0x8E),
    ('‘', 0x91),
    ('’', 0x92),
    ('“', 0x93),
    ('”', 0x94),
    ('•', 0x95),
    ('–', 0x96),
    ('—', 0x97),
    ('˜', 0x98),
    ('™', 0x99),
    ('š', 0x9A),
    ('›', 0x9B),
    ('œ', 0x9C),
    ('ž', 0x9E),
    ('Ÿ', 0x9F),
];

/// Kode byte WinAnsi untuk satu karakter. Karakter yang tidak ada di WinAnsi menjadi `?`.
fn winansi(c: char) -> u8 {
    match c {
        ' '..='~' | '\u{a0}'..='\u{ff}' => c as u8,
        _ => WINANSI_HIGH
            .iter()
            .find(|(k, _)| *k == c)
            .map_or(b'?', |(_, b)| *b),
    }
}

/// Teks sebagai isi string literal PDF `( … )`, dengan escape untuk `\`, `(`, dan `)`.
fn pdf_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'(');
    for c in text.chars() {
        let b = winansi(if c.is_control() { ' ' } else { c });
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
}

fn line_h(pt: f64) -> f64 {
    pt * 1.25
}

fn row_height(spans: &[Span], pt: f64, bold: bool) -> f64 {
    let lines = spans
        .iter()
        .map(|s| wrap(s.text, s.w - 2.0 * PAD_X, pt, bold).len())
        .max()
        .unwrap_or(1);
    lines as f64 * line_h(pt) + 2.0 * PAD_Y
}

/// Satu halaman: isi content stream dalam bentuk byte.
#[derive(Default)]
struct Page {
    ops: Vec<u8>,
}

impl Page {
    /// Koordinat dari atas halaman (`top`) diubah ke koordinat PDF (dari bawah).
    fn fill_rect(&mut self, x: f64, top: f64, w: f64, h: f64, gray: f64) {
        let y = PAGE_H - top - h;
        self.ops
            .extend(format!("q {gray} g {x:.2} {y:.2} {w:.2} {h:.2} re f Q\n").into_bytes());
    }

    fn stroke_rect(&mut self, x: f64, top: f64, w: f64, h: f64) {
        let y = PAGE_H - top - h;
        self.ops.extend(
            format!("q {BORDER_GRAY} G 0.5 w {x:.2} {y:.2} {w:.2} {h:.2} re S Q\n").into_bytes(),
        );
    }

    /// Teks dengan baseline di `baseline` (dari atas halaman).
    fn text(&mut self, x: f64, baseline: f64, pt: f64, bold: bool, text: &str) {
        let y = PAGE_H - baseline;
        let font = if bold { "F2" } else { "F1" };
        self.ops.extend(
            format!("q {TEXT_GRAY} g BT /{font} {pt:.2} Tf {x:.2} {y:.2} Td ").into_bytes(),
        );
        pdf_string(text, &mut self.ops);
        self.ops.extend(b" Tj ET Q\n");
    }

    /// Gambar satu baris tabel pada `top`, mengembalikan tingginya.
    fn row(&mut self, top: f64, spans: &[Span], pt: f64, bold: bool, fill: Option<f64>) -> f64 {
        let h = row_height(spans, pt, bold);
        if let Some(gray) = fill {
            let first = spans.first().map_or(MARGIN, |s| s.x);
            let last = spans.last().map_or(first, |s| s.x + s.w);
            self.fill_rect(first, top, last - first, h, gray);
        }
        for s in spans {
            self.stroke_rect(s.x, top, s.w, h);
            let lines = wrap(s.text, s.w - 2.0 * PAD_X, pt, bold);
            for (i, line) in lines.iter().enumerate() {
                let x = if s.center {
                    s.x + (s.w - measure(line, pt, bold)) / 2.0
                } else {
                    s.x + PAD_X
                };
                let baseline = top + PAD_Y + i as f64 * line_h(pt) + pt;
                self.text(x, baseline, pt, bold, line);
            }
        }
        h
    }
}

/// Lebar kolom: lebar alami (teks terpanjang, dibatasi `COL_MAX`) lalu diskalakan agar pas dengan `avail`.
fn column_widths(headings: &[String], rows: &[Vec<String>], avail: f64) -> Vec<f64> {
    let natural: Vec<f64> = headings
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let head = measure(h, BODY_PT, true);
            let body = rows
                .iter()
                .filter_map(|r| r.get(i))
                .map(|c| measure(c, BODY_PT, false))
                .fold(0.0_f64, f64::max);
            (head.max(body).min(COL_MAX) + 2.0 * PAD_X).max(COL_MIN)
        })
        .collect();
    let sum: f64 = natural.iter().sum();
    natural.iter().map(|w| w * avail / sum).collect()
}

/// Susun halaman-halaman PDF dari judul, baris meta, dan tabel.
fn layout(title: &str, meta: &str, table: &ChecklistTable) -> Vec<Page> {
    let avail = PAGE_W - 2.0 * MARGIN;
    let widths = column_widths(&table.headings, &table.rows, avail);
    let mut xs = Vec::with_capacity(widths.len());
    let mut x = MARGIN;
    for w in &widths {
        xs.push(x);
        x += w;
    }
    let bottom = PAGE_H - MARGIN;

    let mut pages = vec![Page::default()];
    let mut y = MARGIN;
    let page = pages.last_mut().expect("halaman pertama");
    page.text(MARGIN, y + TITLE_PT, TITLE_PT, true, title);
    y += TITLE_PT + 6.0;
    page.text(MARGIN, y + META_PT, META_PT, false, meta);
    y += META_PT + 8.0;

    let head_spans = spans(&table.headings, &xs, &widths, true);
    let head_h = row_height(&head_spans, BODY_PT, true);
    page.row(y, &head_spans, BODY_PT, true, Some(HEAD_FILL));
    y += head_h;

    if table.rows.is_empty() {
        let span = [Span {
            text: "Tidak ada data",
            x: MARGIN,
            w: avail,
            center: true,
        }];
        let h = row_height(&span, BODY_PT, false);
        if y + h > bottom {
            pages.push(Page::default());
            y = MARGIN;
        }
        pages
            .last_mut()
            .expect("halaman")
            .row(y, &span, BODY_PT, false, None);
        return pages;
    }

    for (i, cells) in table.rows.iter().enumerate() {
        let spans = spans(cells, &xs, &widths, false);
        let h = row_height(&spans, BODY_PT, false);
        if y + h > bottom {
            // Header berulang di tiap halaman, seperti `<thead>` pada Dompdf.
            pages.push(Page::default());
            y = MARGIN;
            let page = pages.last_mut().expect("halaman");
            page.row(y, &head_spans, BODY_PT, true, Some(HEAD_FILL));
            y += head_h;
        }
        // `tr:nth-child(even)`: baris ke-2, 4, ... pada tbody.
        let fill = (i % 2 == 1).then_some(ZEBRA_FILL);
        pages
            .last_mut()
            .expect("halaman")
            .row(y, &spans, BODY_PT, false, fill);
        y += h;
    }
    pages
}

/// Sel satu baris. Rata tengah: header selalu; sel isi pada kolom 0 dan indeks >= 3 (`$i === 0 || $i > 2`).
fn spans<'a>(cells: &'a [String], xs: &[f64], widths: &[f64], header: bool) -> Vec<Span<'a>> {
    cells
        .iter()
        .enumerate()
        .map(|(i, text)| Span {
            text,
            x: xs[i],
            w: widths[i],
            center: header || i == 0 || i > 2,
        })
        .collect()
}

/// Bungkus halaman menjadi berkas PDF lengkap dengan tabel xref.
fn assemble(pages: &[Page], title: &str) -> Vec<u8> {
    let n = pages.len();
    let kids = (0..n)
        .map(|i| format!("{} 0 R", 6 + 2 * i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    let mut info = b"<< /Title ".to_vec();
    pdf_string(title, &mut info);
    info.extend(b" /Producer (apiamis) >>");
    objects.push(info);
    for (i, page) in pages.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                7 + 2 * i
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", page.ops.len()).into_bytes();
        stream.extend_from_slice(&page.ops);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }

    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        out.extend_from_slice(body);
        out.extend(b"\nendobj\n");
    }
    let xref_at = out.len();
    let size = objects.len() + 1;
    out.extend(format!("xref\n0 {size}\n0000000000 65535 f \n").into_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").into_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .into_bytes(),
    );
    out
}

/// Susun berkas PDF checklist dari tabel yang sudah dibaca.
pub(crate) fn render(title: &str, meta: &str, table: &ChecklistTable) -> Vec<u8> {
    assemble(&layout(title, meta, table), title)
}

/// `GET /api/pekerjaan-checklist/export/pdf`: filter dan gate sama dengan ekspor Excel.
/// Judul, baris meta, dan nama berkas mengikuti `exportPdf` di Laravel.
pub async fn export_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    require_full_access(&state, &headers).await?;
    let table = checklist_table(&state.pool, &query).await?;

    let now = chrono::Utc::now();
    let tahun = query
        .get("tahun")
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && *t != "0");
    let mut meta = String::new();
    if let Some(t) = tahun {
        meta.push_str(&format!("Tahun anggaran: {t} · "));
    }
    meta.push_str(&format!(
        "Diekspor: {} · Total baris: {}",
        now.format("%d/%m/%Y %H:%M"),
        table.rows.len()
    ));

    let bytes = render("Checklist Pekerjaan", &meta, &table);
    let filename = format!("checklist_pekerjaan_{}.pdf", now.format("%Y%m%d_%H%M%S"));
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChecklistTable {
        ChecklistTable {
            headings: vec![
                "No".into(),
                "Nama Paket".into(),
                "Kegiatan".into(),
                "Item A".into(),
                "Tanggal".into(),
                "Diubah Oleh".into(),
            ],
            rows: vec![
                vec![
                    "1".into(),
                    "Rehab (jalan) \\ desa".into(),
                    "Kegiatan X".into(),
                    "Ya (01/02/2026 10:00 · Budi)".into(),
                    "01/02/2026 10:00".into(),
                    "Budi".into(),
                ],
                vec![
                    "2".into(),
                    "Paket kedua".into(),
                    "-".into(),
                    "Tidak".into(),
                    "-".into(),
                    "-".into(),
                ],
            ],
        }
    }

    #[test]
    fn wrap_stays_within_width_and_keeps_all_text() {
        let text = "satu dua tiga empat lima enam tujuh delapan sembilan sepuluh";
        let max = 60.0;
        let lines = wrap(text, max, 8.0, false);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(
                measure(l, 8.0, false) <= max + 1e-9,
                "baris terlalu lebar: {l}"
            );
        }
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn wrap_breaks_overlong_word_without_losing_chars() {
        let word = "abcdefghijklmnopqrstuvwxyz0123456789";
        let lines = wrap(word, 40.0, 8.0, false);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), word);
    }

    #[test]
    fn pdf_string_escapes_delimiters() {
        let mut out = Vec::new();
        pdf_string("a(b)\\c·d", &mut out);
        assert_eq!(out, b"(a\\(b\\)\\\\c\xb7d)".to_vec());
    }

    #[test]
    fn xref_offsets_point_at_their_objects() {
        let pdf = render("Checklist Pekerjaan", "Total baris: 2", &sample());
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let key = b"startxref\n";
        let at = pdf.windows(key.len()).rposition(|w| w == key).unwrap() + key.len();
        let digits: String = pdf[at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        let start: usize = digits.parse().unwrap();
        assert!(pdf[start..].starts_with(b"xref\n"));
        let entries: Vec<&[u8]> = pdf[start..]
            .split(|&b| b == b'\n')
            .skip(3)
            .take_while(|l| l.ends_with(b" n "))
            .collect();
        assert!(!entries.is_empty());
        for (i, entry) in entries.iter().enumerate() {
            let off: usize = std::str::from_utf8(&entry[..10]).unwrap().parse().unwrap();
            assert!(
                pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()),
                "objek {} tidak di offset {off}",
                i + 1
            );
        }
    }

    #[test]
    fn table_text_is_in_content_in_row_order() {
        let pdf = render("Checklist Pekerjaan", "Total baris: 2", &sample());
        let text = String::from_utf8_lossy(&pdf).to_string();
        // Tanpa spasi dan tanpa escape `\`: pemenggalan baris dan escape tidak mengubah isi.
        let compact: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '\\')
            .collect();
        for needle in [
            "Rehab(jalan)desa",
            "KegiatanX",
            "Paketkedua",
            "Ya(01/02/2026",
        ] {
            assert!(compact.contains(needle), "hilang: {needle}");
        }
        let first = compact.find("Rehab").unwrap();
        let second = compact.find("Paketkedua").unwrap();
        assert!(first < second, "urutan baris salah");
    }

    #[test]
    fn empty_table_shows_placeholder_row() {
        let table = ChecklistTable {
            headings: vec!["No".into(), "Nama Paket".into(), "Kegiatan".into()],
            rows: vec![],
        };
        let pdf = render("Checklist Pekerjaan", "Total baris: 0", &table);
        assert!(String::from_utf8_lossy(&pdf).contains("Tidak ada data"));
    }

    #[test]
    fn long_table_repeats_header_on_new_page() {
        let rows: Vec<Vec<String>> = (1..=120)
            .map(|i| vec![i.to_string(), format!("Paket {i}"), "K".into()])
            .collect();
        let table = ChecklistTable {
            headings: vec!["No".into(), "Nama Paket".into(), "Kegiatan".into()],
            rows,
        };
        let pages = layout("Checklist Pekerjaan", "Total baris: 120", &table);
        assert!(pages.len() > 1);
        for page in &pages {
            assert!(String::from_utf8_lossy(&page.ops).contains("(Nama Paket)"));
        }
    }
}
