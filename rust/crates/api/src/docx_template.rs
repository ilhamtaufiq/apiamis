//! Pengisian template Word (.docx) untuk dokumen kontrak, setara `DocumentExportService::export`.
//!
//! Langkah yang sama dengan Laravel:
//! 1. Tag yang terpecah markup XML di antara `{` dan `}` disatukan (`joinFragmentedTags`).
//! 2. `{{key}}` dan `{key}` diganti nilainya, berurutan per kunci, di `word/document.xml`, header, dan footer.
//!    Nilai kosong (atau NULL) menjadi `-`.
//!
//! Perbedaan dari PHP: nilai di-escape sebagai teks XML (`&`, `<`, `>`) dan diganti secara literal
//! (tanpa interpretasi `$1` seperti `preg_replace`). Di Laravel, nilai dengan `&` merusak berkas.

use std::io::{Cursor, Read, Write};

use regex::{NoExpand, Regex};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

/// `word/document.xml`, `word/headerN.xml`, dan `word/footerN.xml` (N boleh kosong atau angka).
fn is_template_part(name: &str) -> bool {
    if name == "word/document.xml" {
        return true;
    }
    ["word/header", "word/footer"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".xml"))
            .is_some_and(|digits| digits.chars().all(|c| c.is_ascii_digit()))
    })
}

/// Escape teks XML untuk nilai placeholder.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Pola pencarian satu kunci: `{{key}}` lalu `{key}`, dengan spasi di dalam kurung diabaikan.
struct Placeholder {
    double: Regex,
    single: Regex,
    value: String,
}

fn placeholders(data: &[(String, String)]) -> Result<Vec<Placeholder>, String> {
    data.iter()
        .map(|(key, value)| {
            let k = regex::escape(key);
            let double =
                Regex::new(&format!(r"\{{\{{\s*{k}\s*\}}\}}")).map_err(|e| e.to_string())?;
            let single = Regex::new(&format!(r"\{{\s*{k}\s*\}}")).map_err(|e| e.to_string())?;
            let shown = if value.is_empty() {
                "-"
            } else {
                value.as_str()
            };
            Ok(Placeholder {
                double,
                single,
                value: xml_escape(shown),
            })
        })
        .collect()
}

/// `joinFragmentedTags`: di dalam `{...}` (tanpa kurung di dalamnya), semua tag XML dibuang.
fn join_fragmented_tags(xml: &str, frag: &Regex, tag: &Regex) -> String {
    frag.replace_all(xml, |caps: &regex::Captures| {
        tag.replace_all(&caps[0], "").into_owned()
    })
    .into_owned()
}

fn fill_part(xml: &str, frag: &Regex, tag: &Regex, places: &[Placeholder]) -> String {
    let mut out = join_fragmented_tags(xml, frag, tag);
    for p in places {
        out = p.double.replace_all(&out, NoExpand(&p.value)).into_owned();
        out = p.single.replace_all(&out, NoExpand(&p.value)).into_owned();
    }
    out
}

/// Mengisi placeholder di template `.docx` dan mengembalikan berkas `.docx` baru.
/// Entri lain di dalam arsip (styles, media, relasi) disalin apa adanya.
pub fn fill(template: &[u8], data: &[(String, String)]) -> Result<Vec<u8>, String> {
    let frag = Regex::new(r"\{[^{}]*\}").map_err(|e| e.to_string())?;
    let tag = Regex::new(r"<[^>]*>").map_err(|e| e.to_string())?;
    let places = placeholders(data)?;

    let mut archive = ZipArchive::new(Cursor::new(template)).map_err(|e| e.to_string())?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            writer
                .add_directory(name, options())
                .map_err(|e| e.to_string())?;
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        drop(entry);

        if is_template_part(&name) {
            let xml = String::from_utf8(bytes).map_err(|e| format!("{name}: {e}"))?;
            bytes = fill_part(&xml, &frag, &tag, &places).into_bytes();
        }
        writer
            .start_file(name, options())
            .map_err(|e| e.to_string())?;
        writer.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let out = writer.finish().map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arsip docx minimal: satu bagian isi dan satu header, tag `{nama}` terpecah oleh run XML.
    fn sample_docx() -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("[Content_Types].xml", options()).unwrap();
        w.write_all(b"<Types/>").unwrap();
        w.start_file("word/document.xml", options()).unwrap();
        w.write_all(
            b"<w:document><w:body><w:p><w:r><w:t>Paket {na</w:t></w:r><w:r><w:t>ma}</w:t></w:r></w:p>\
              <w:p><w:r><w:t>{{ nilai }} dan {kosong}</w:t></w:r></w:p></w:body></w:document>",
        )
        .unwrap();
        w.start_file("word/header1.xml", options()).unwrap();
        w.write_all(b"<w:hdr><w:t>{tahun}</w:t></w:hdr>").unwrap();
        w.start_file("word/styles.xml", options()).unwrap();
        w.write_all(b"<w:styles>{nama}</w:styles>").unwrap();
        w.finish().unwrap().into_inner()
    }

    fn read_entry(docx: &[u8], name: &str) -> String {
        let mut archive = ZipArchive::new(Cursor::new(docx)).unwrap();
        let mut s = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    #[test]
    fn fills_fragmented_tags_in_body_and_header_only() {
        let data = vec![
            ("nama".to_string(), "Jalan & Jembatan".to_string()),
            ("nilai".to_string(), "Rp. 1.000".to_string()),
            ("kosong".to_string(), String::new()),
            ("tahun".to_string(), "2026".to_string()),
        ];
        let out = fill(&sample_docx(), &data).unwrap();

        let body = read_entry(&out, "word/document.xml");
        assert!(body.contains("Paket Jalan &amp; Jembatan"), "{body}");
        assert!(body.contains("Rp. 1.000 dan -"), "{body}");
        assert!(read_entry(&out, "word/header1.xml").contains(">2026<"));
        // Bagian selain dokumen dan header tidak disentuh.
        assert_eq!(
            read_entry(&out, "word/styles.xml"),
            "<w:styles>{nama}</w:styles>"
        );
        assert_eq!(read_entry(&out, "[Content_Types].xml"), "<Types/>");
    }

    #[test]
    fn dollar_in_value_is_literal() {
        let data = vec![("nama".to_string(), "Rp $1 \\0".to_string())];
        let out = fill(&sample_docx(), &data).unwrap();
        assert!(read_entry(&out, "word/document.xml").contains("Rp $1 \\0"));
    }
}
