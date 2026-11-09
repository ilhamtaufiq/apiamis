//! Pengisian template Excel (.xlsx) untuk Ringkasan Kontrak, setara `ExcelDocumentExportService::exportRingkasan`.
//!
//! Laravel memuat template dengan PhpSpreadsheet lalu mengganti `{{key}}` dan `{key}` pada setiap sel teks.
//! Di sini arsip .xlsx dibuka sebagai zip dan teks di dalam elemen `<t>` diganti langsung, sehingga gaya,
//! lebar kolom, sel gabungan, dan format sel lain tetap utuh. Bagian yang diproses:
//! - `xl/worksheets/*.xml`: teks sel `inlineStr` (`<is><t>`),
//! - `xl/sharedStrings.xml`: teks `sharedStrings` (`<si><t>`).
//!
//! Aturan penggantian sama dengan `replacePlaceholders`: berurutan per kunci, `{{ key }}` lalu `{ key }`
//! (spasi di dalam kurung diabaikan), nilai kosong menjadi `-`. Perbedaan dari PHP: nilai di-escape sebagai
//! teks XML (`&`, `<`, `>`) dan diganti secara literal tanpa interpretasi `$1`. Placeholder yang tidak
//! ada di data dibiarkan. Placeholder yang terpecah di beberapa run teks (rich text) tidak digabung.

use std::io::{Cursor, Read, Write};

use regex::{NoExpand, Regex};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

/// `<t>` atau `<t atribut...>` (bukan `<tableParts>` dsb.), isi tanpa markup.
const TEXT_NODE: &str = r"<t((?:\s[^>]*[^>/])?)>([^<]*)</t>";

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn is_template_part(name: &str) -> bool {
    name == "xl/sharedStrings.xml"
        || name
            .strip_prefix("xl/worksheets/")
            .is_some_and(|rest| rest.ends_with(".xml") && !rest.contains('/'))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

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
            let shown = if value.is_empty() { "-" } else { value.as_str() };
            Ok(Placeholder {
                double,
                single,
                value: xml_escape(shown),
            })
        })
        .collect()
}

/// Ganti placeholder pada satu teks sel yang sudah berupa teks XML (entitas masih ter-escape).
fn replace_text(text: &str, places: &[Placeholder]) -> String {
    let mut out = text.to_string();
    for p in places {
        out = p.double.replace_all(&out, NoExpand(&p.value)).into_owned();
        out = p.single.replace_all(&out, NoExpand(&p.value)).into_owned();
    }
    out
}

fn fill_part(xml: &str, text_node: &Regex, places: &[Placeholder]) -> String {
    text_node
        .replace_all(xml, |caps: &regex::Captures| {
            let body = &caps[2];
            if !body.contains('{') {
                return caps[0].to_string();
            }
            format!("<t{}>{}</t>", &caps[1], replace_text(body, places))
        })
        .into_owned()
}

/// Mengisi placeholder di template `.xlsx` dan mengembalikan berkas `.xlsx` baru.
/// Entri lain di dalam arsip disalin apa adanya.
pub fn fill(template: &[u8], data: &[(String, String)]) -> Result<Vec<u8>, String> {
    let text_node = Regex::new(TEXT_NODE).map_err(|e| e.to_string())?;
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
            bytes = fill_part(&xml, &text_node, &places).into_bytes();
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

    fn data(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn fill_xml(xml: &str, pairs: &[(&str, &str)]) -> String {
        let re = Regex::new(TEXT_NODE).unwrap();
        fill_part(xml, &re, &placeholders(&data(pairs)).unwrap())
    }

    #[test]
    fn replaces_single_and_double_braces_with_spaces() {
        let xml = r#"<is><t xml:space="preserve">A {nama} B {{ nama }} C {  nama}</t></is>"#;
        let out = fill_xml(xml, &[("nama", "Budi")]);
        assert_eq!(
            out,
            r#"<is><t xml:space="preserve">A Budi B Budi C Budi</t></is>"#
        );
    }

    #[test]
    fn escapes_xml_and_keeps_dollar_literal() {
        let out = fill_xml("<t>{x}</t>", &[("x", "CV A&B <1> $1")]);
        assert_eq!(out, "<t>CV A&amp;B &lt;1&gt; $1</t>");
    }

    #[test]
    fn empty_value_becomes_dash_and_unknown_is_kept() {
        let out = fill_xml("<t>{a}|{tidak_dikenal}</t>", &[("a", "")]);
        assert_eq!(out, "<t>-|{tidak_dikenal}</t>");
    }

    #[test]
    fn prefix_keys_do_not_clash() {
        let out = fill_xml(
            "<t>{nilai_kontrak} {nilai_kontrak_addendum}</t>",
            &[("nilai_kontrak", "1"), ("nilai_kontrak_addendum", "2")],
        );
        assert_eq!(out, "<t>1 2</t>");
    }

    #[test]
    fn text_without_placeholder_and_empty_tags_are_untouched() {
        let xml = "<t/><t>Halo</t><tableParts count=\"0\"/>";
        assert_eq!(fill_xml(xml, &[("a", "b")]), xml);
    }

    #[test]
    fn fill_rewrites_archive_parts_only() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file("xl/worksheets/sheet1.xml", options()).unwrap();
        w.write_all(b"<c t=\"inlineStr\"><is><t>{a}</t></is></c>")
            .unwrap();
        w.start_file("xl/styles.xml", options()).unwrap();
        w.write_all(b"<t>{a}</t>").unwrap();
        let bytes = w.finish().unwrap().into_inner();

        let out = fill(&bytes, &data(&[("a", "Z&Y")])).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(out)).unwrap();
        let mut s = String::new();
        ar.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(s.contains("<t>Z&amp;Y</t>"));
        s.clear();
        ar.by_name("xl/styles.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "<t>{a}</t>");
    }
}
