//! Konversi berkas KML/KMZ peta peripaan menjadi GeoJSON `FeatureCollection` di sisi server.
//!
//! Menggantikan `KmzParser.ts` di browser, supaya GeoJSON yang tersimpan berasal dari berkas
//! asli dan tidak bisa dibuat-buat oleh client. Hanya geometri `Point`, `LineString`,
//! `LinearRing`, `Polygon`, dan `MultiGeometry` (diratakan jadi fitur terpisah) yang diambil.

use std::io::{Cursor, Read};

use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};

/// Batas ukuran KML setelah dibuka dari KMZ (mencegah zip bomb).
pub const MAX_KML_BYTES: u64 = 50 * 1024 * 1024;
/// Batas jumlah fitur hasil konversi.
pub const MAX_FEATURES: usize = 20_000;

/// Simpul XML minimal: nama elemen lokal, teks langsung, dan anak.
#[derive(Default)]
struct Node {
    name: String,
    text: String,
    children: Vec<Node>,
}

/// Ambil teks KML dari berkas `.kml` (UTF-8) atau `.kmz` (zip; dipakai `.kml` pertama).
pub fn kml_text(file_name: &str, bytes: &[u8]) -> Result<String, String> {
    if !file_name.to_ascii_lowercase().ends_with(".kmz") {
        return String::from_utf8(bytes.to_vec())
            .map_err(|_| "KML harus berupa teks UTF-8".to_string());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "KMZ tidak bisa dibuka".to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| "KMZ tidak valid".to_string())?;
        if entry.is_dir() || !entry.name().to_ascii_lowercase().ends_with(".kml") {
            continue;
        }
        let mut buf = String::new();
        entry
            .by_ref()
            .take(MAX_KML_BYTES + 1)
            .read_to_string(&mut buf)
            .map_err(|_| "KML di dalam KMZ tidak valid".to_string())?;
        if buf.len() as u64 > MAX_KML_BYTES {
            return Err(format!(
                "KML di dalam KMZ melebihi {} MB",
                MAX_KML_BYTES / 1024 / 1024
            ));
        }
        return Ok(buf);
    }
    Err("Tidak ada berkas .kml di dalam KMZ".to_string())
}

/// Konversi teks KML menjadi GeoJSON. Placemark tanpa geometri yang valid dilewati;
/// jika tidak ada fitur sama sekali, hasilnya error.
pub fn kml_to_geojson(xml: &str) -> Result<Value, String> {
    let root = parse_tree(xml)?;
    let mut placemarks = Vec::new();
    collect_named(&root, "Placemark", &mut placemarks);

    let mut features = Vec::new();
    for pm in placemarks {
        let mut props = Map::new();
        props.insert(
            "name".into(),
            json!(child_text(pm, "name").unwrap_or_default()),
        );
        if let Some(desc) = child_text(pm, "description").filter(|d| !d.is_empty()) {
            props.insert("description".into(), json!(desc));
        }
        let mut geoms = Vec::new();
        for child in &pm.children {
            geometries(child, &mut geoms)?;
        }
        for geometry in geoms {
            features.push(json!({ "type": "Feature", "properties": props, "geometry": geometry }));
            if features.len() > MAX_FEATURES {
                return Err(format!("Jumlah fitur melebihi {MAX_FEATURES}"));
            }
        }
    }
    if features.is_empty() {
        return Err("Tidak ada Placemark dengan geometri di dalam KML".to_string());
    }
    Ok(json!({ "type": "FeatureCollection", "features": features }))
}

fn parse_tree(xml: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(xml);
    // Jangan trim per event: spasi di sekitar entitas (`A &amp; B`) ikut terpotong. Teks dirapikan saat dibaca.
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("KML tidak valid: {e}"))?;
        match event {
            Event::Start(e) => stack.push(new_node(e.local_name().as_ref())),
            Event::Empty(e) => attach(&mut stack, new_node(e.local_name().as_ref()))?,
            Event::End(_) => {
                let node = stack.pop().ok_or("KML tidak valid: tag penutup berlebih")?;
                attach(&mut stack, node)?;
            }
            Event::Text(t) => {
                let s = t.decode().map_err(|e| format!("KML tidak valid: {e}"))?;
                current(&mut stack)?.text.push_str(&s);
            }
            Event::CData(t) => {
                let s = t.decode().map_err(|e| format!("KML tidak valid: {e}"))?;
                current(&mut stack)?.text.push_str(&s);
            }
            // Entitas (`&amp;`, `&#38;`) datang sebagai event terpisah dan harus di-resolve.
            Event::GeneralRef(r) => {
                let ch = match r
                    .resolve_char_ref()
                    .map_err(|e| format!("KML tidak valid: {e}"))?
                {
                    Some(c) => c,
                    None => match r
                        .decode()
                        .map_err(|e| format!("KML tidak valid: {e}"))?
                        .as_ref()
                    {
                        "amp" => '&',
                        "lt" => '<',
                        "gt" => '>',
                        "quot" => '"',
                        "apos" => '\'',
                        other => {
                            return Err(format!("KML tidak valid: entitas tidak dikenal &{other};"))
                        }
                    },
                };
                current(&mut stack)?.text.push(ch);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("KML tidak lengkap".to_string());
    }
    stack.pop().ok_or_else(|| "KML tidak valid".to_string())
}

fn new_node(local: &[u8]) -> Node {
    Node {
        name: String::from_utf8_lossy(local).into_owned(),
        ..Node::default()
    }
}

fn current(stack: &mut [Node]) -> Result<&mut Node, String> {
    stack
        .last_mut()
        .ok_or_else(|| "KML tidak valid".to_string())
}

fn attach(stack: &mut Vec<Node>, node: Node) -> Result<(), String> {
    current(stack)?.children.push(node);
    Ok(())
}

fn collect_named<'a>(node: &'a Node, name: &str, out: &mut Vec<&'a Node>) {
    for child in &node.children {
        if child.name == name {
            out.push(child);
        }
        collect_named(child, name, out);
    }
}

fn child<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    node.children.iter().find(|c| c.name == name)
}

fn child_text(node: &Node, name: &str) -> Option<String> {
    child(node, name).map(|c| c.text.trim().to_string())
}

/// Ubah satu simpul geometri (dan `MultiGeometry` secara rekursif) menjadi geometri GeoJSON.
fn geometries(node: &Node, out: &mut Vec<Value>) -> Result<(), String> {
    match node.name.as_str() {
        "Point" => {
            if let Some(pos) = parse_coords(&child_text(node, "coordinates").unwrap_or_default())?
                .into_iter()
                .next()
            {
                out.push(json!({ "type": "Point", "coordinates": pos }));
            }
        }
        "LineString" | "LinearRing" => {
            let coords = parse_coords(&child_text(node, "coordinates").unwrap_or_default())?;
            if coords.len() >= 2 {
                out.push(json!({ "type": "LineString", "coordinates": coords }));
            }
        }
        "Polygon" => {
            if let Some(rings) = polygon_rings(node)? {
                out.push(json!({ "type": "Polygon", "coordinates": rings }));
            }
        }
        "MultiGeometry" => {
            for c in &node.children {
                geometries(c, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn polygon_rings(node: &Node) -> Result<Option<Vec<Value>>, String> {
    let mut rings = Vec::new();
    for (boundary, required) in [("outerBoundaryIs", true), ("innerBoundaryIs", false)] {
        let Some(bound) = child(node, boundary) else {
            if required {
                return Ok(None);
            }
            continue;
        };
        let Some(ring) = child(bound, "LinearRing") else {
            if required {
                return Ok(None);
            }
            continue;
        };
        let coords = parse_coords(&child_text(ring, "coordinates").unwrap_or_default())?;
        // Cincin tertutup butuh minimal 4 titik (3 sudut + penutup).
        if coords.len() >= 4 {
            rings.push(Value::Array(coords));
        } else if required {
            return Ok(None);
        }
    }
    Ok(Some(rings))
}

/// Parse `lng,lat[,alt]` dipisah spasi. Rentang koordinat diperiksa.
fn parse_coords(text: &str) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for tuple in text.split_whitespace() {
        let parts: Vec<&str> = tuple.split(',').collect();
        if !(2..=3).contains(&parts.len()) {
            return Err(format!("Koordinat tidak valid: {tuple}"));
        }
        let num = |s: &str| {
            s.trim()
                .parse::<f64>()
                .map_err(|_| format!("Koordinat tidak valid: {tuple}"))
        };
        let lng = num(parts[0])?;
        let lat = num(parts[1])?;
        if !(-180.0..=180.0).contains(&lng) || !(-90.0..=90.0).contains(&lat) {
            return Err(format!("Koordinat di luar rentang: {tuple}"));
        }
        let mut pos = vec![json!(lng), json!(lat)];
        if parts.len() == 3 {
            pos.push(json!(num(parts[2])?));
        }
        out.push(Value::Array(pos));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2"><Document>
  <Placemark><name>Pipa A &amp; B</name><description>uji</description>
    <LineString><coordinates>107.1,-6.8,10 107.2,-6.9 107.3,-7.0</coordinates></LineString>
  </Placemark>
  <Placemark><name>Area</name>
    <Polygon>
      <outerBoundaryIs><LinearRing><coordinates>107.0,-6.0 107.1,-6.0 107.1,-6.1 107.0,-6.1 107.0,-6.0</coordinates></LinearRing></outerBoundaryIs>
      <innerBoundaryIs><LinearRing><coordinates>107.02,-6.02 107.03,-6.02 107.03,-6.03 107.02,-6.02</coordinates></LinearRing></innerBoundaryIs>
    </Polygon>
  </Placemark>
  <Placemark><name>Kumpulan</name>
    <MultiGeometry>
      <Point><coordinates>107.5,-6.5</coordinates></Point>
      <Point><coordinates>107.6,-6.6</coordinates></Point>
    </MultiGeometry>
  </Placemark>
</Document></kml>"#;

    #[test]
    fn converts_line_polygon_and_multigeometry() {
        let fc = kml_to_geojson(DOC).unwrap();
        let features = fc["features"].as_array().unwrap();
        assert_eq!(features.len(), 4);
        assert_eq!(features[0]["properties"]["name"], "Pipa A & B");
        assert_eq!(features[0]["properties"]["description"], "uji");
        assert_eq!(features[0]["geometry"]["type"], "LineString");
        assert_eq!(
            features[0]["geometry"]["coordinates"][0],
            json!([107.1, -6.8, 10.0])
        );
        assert_eq!(features[1]["geometry"]["type"], "Polygon");
        assert_eq!(
            features[1]["geometry"]["coordinates"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(features[2]["geometry"]["type"], "Point");
        assert_eq!(features[3]["geometry"]["coordinates"], json!([107.6, -6.6]));
    }

    #[test]
    fn rejects_out_of_range_coordinates() {
        let bad = DOC.replace("107.5,-6.5", "107.5,-96.5");
        assert!(kml_to_geojson(&bad)
            .unwrap_err()
            .contains("di luar rentang"));
    }

    #[test]
    fn rejects_document_without_geometry() {
        let xml = "<kml><Document><Placemark><name>x</name></Placemark></Document></kml>";
        assert!(kml_to_geojson(xml).is_err());
    }

    #[test]
    fn rejects_unbalanced_xml() {
        assert!(kml_to_geojson("<kml><Document></kml>").is_err());
    }

    #[test]
    fn kml_text_reads_plain_kml_and_rejects_non_utf8() {
        assert_eq!(kml_text("a.kml", b"<kml/>").unwrap(), "<kml/>");
        assert!(kml_text("a.kml", &[0xff, 0xfe]).is_err());
    }

    #[test]
    fn kml_text_reads_kmz_archive() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            zw.start_file("doc.kml", zip::write::SimpleFileOptions::default())
                .unwrap();
            zw.write_all(DOC.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let text = kml_text("peta.kmz", &buf).unwrap();
        assert!(text.contains("Pipa A"));
        assert!(kml_text("peta.kmz", b"bukan zip").is_err());
    }
}
