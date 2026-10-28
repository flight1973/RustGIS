//! Shapefile text is decoded at the same public boundaries used by desktop and browser imports.

use rustgis_data::{Dataset, Field, FieldType, Geometry, GeometryType, Value, io};

/// A single point with one 32-byte character field. The bytes are supplied directly rather
/// than encoded by the decoder under test; byte 29 is the DBF language-driver identifier.
fn parts(text: &[u8], ldid: u8, cpg: Option<&[u8]>) -> Vec<(String, Vec<u8>)> {
    let mut ds = Dataset::new("places", GeometryType::Point, vec![Field::new("name", FieldType::Text)]);
    ds.push(Some(Geometry::Point(rustgis_data::Coord::new(1.0, 2.0))), vec![Value::Text("x".repeat(32))]);
    io::shapefile_parts(&ds)
        .unwrap()
        .0
        .into_iter()
        .filter_map(|(ext, mut bytes)| {
            if ext == "cpg" {
                return cpg.map(|value| ("places.cpg".into(), value.to_vec()));
            }
            if ext == "dbf" {
                bytes[29] = ldid;
                let start = usize::from(u16::from_le_bytes([bytes[8], bytes[9]])) + 1;
                bytes[start..start + 32].fill(b' ');
                bytes[start..start + text.len()].copy_from_slice(text);
            }
            Some((format!("places.{ext}"), bytes))
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn loose_shapefile_honors_cpg_before_dbf_code_page() {
    let dir = tempfile::tempdir().unwrap();
    // LDID 0xF0 claims UTF-8; the sidecar correctly identifies Windows-1252.
    for (name, bytes) in parts(b"Montr\xe9al \x96 Qu\xe9bec", 0xf0, Some(b"1252")) {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let (ds, _) = io::read_path(&dir.path().join("places.shp")).unwrap();
    assert_eq!(ds.value(0, 0), &Value::Text("Montréal – Québec".into()));
}

#[test]
fn zipped_shapefile_honors_its_own_cpg_in_a_folder() {
    let entries: Vec<_> = parts(b"Montr\xe9al \x96 Qu\xe9bec", 0xf0, Some("\u{feff} Windows-1252\r\n".as_bytes()))
        .into_iter()
        .map(|(name, bytes)| (format!("data/{}", name.to_ascii_uppercase()), bytes))
        .chain([("elsewhere/PLACES.CPG".into(), b"UTF-8".to_vec())])
        .collect();
    let zip = rustgis_data::zip::write(&entries).unwrap();
    let layers = io::read_zip("upload.zip", &zip).unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].0.value(0, 0), &Value::Text("Montréal – Québec".into()));
    assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn streamed_shapefile_decodes_cpg_in_sampled_and_complete_reads() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in parts(b"Montr\xe9al \x96 Qu\xe9bec", 0xf0, Some(b"1252")) {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let src = ShpSource::open_path(&dir.path().join("places.shp"), "places").unwrap();
    let expected = vec![Value::Text("Montréal – Québec".into())];
    assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, expected);
    let mut attrs = Vec::new();
    src.visit_chunks(None, 1024, &mut |features| {
        attrs.extend(features.into_iter().map(|f| f.attrs));
        Ok(())
    })
    .unwrap();
    assert_eq!(attrs, vec![expected]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shapefile_uses_dbf_code_page_when_cpg_is_absent() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    // Esri documents both 0x03 and 0x57 as Windows-1252; C9 is Windows-1251.
    for (ldid, text, expected) in [
        (0x03, b"Montr\xe9al \x96 Qu\xe9bec".as_slice(), "Montréal – Québec"),
        (0x57, b"Montr\xe9al \x96 Qu\xe9bec".as_slice(), "Montréal – Québec"),
        (0xc9, b"\xcc\xee\xf1\xea\xe2\xe0".as_slice(), "Москва"),
    ] {
        let files = parts(text, ldid, None);
        let zip = rustgis_data::zip::write(&files).unwrap();
        assert_eq!(io::read_zip("places.zip", &zip).unwrap()[0].0.value(0, 0), &Value::Text(expected.into()), "LDID {ldid:02X}");
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        assert_eq!(io::read_path(&path).unwrap().0.value(0, 0), &Value::Text(expected.into()), "LDID {ldid:02X}");
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text(expected.into())], "LDID {ldid:02X}");
    }
}

/// Language driver IDs dbase doesn't know, as GDAL reads them (#1079): Spanish and Western
/// European ANSI, the Baltic, Korean and Mac pages, and DOS ones by their other numbers.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn language_drivers_dbase_lacks_decode_text_without_cpg_in_all_readers() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    for (ldid, text, expected) in [
        (0x59, b"\xc1guilas".as_slice(), "Águilas"),
        (0x58, b"\xc1guilas".as_slice(), "Águilas"),
        (0xcc, b"\xd0iauliai".as_slice(), "Šiauliai"),
        (0x4e, b"\xbc\xad\xbf\xef".as_slice(), "서울"),
        (0x26, b"\x8c\xae\xe1\xaa\xa2\xa0".as_slice(), "Москва"),
        (0x1f, b"\xacesk\xec".as_slice(), "Český"),
        (0x37, b"Se\xa4or".as_slice(), "Señor"),
        (0x04, b"Caf\x8e".as_slice(), "Café"),
        (0x96, b"\x8a\xe8\xe5\xe2".as_slice(), "Киев"),
    ] {
        let files = parts(text, ldid, None);
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.value(0, 0), &Value::Text(expected.into()), "LDID {ldid:02X}");
        assert!(layers[0].1.messages.is_empty(), "LDID {ldid:02X}: {:?}", layers[0].1.messages);
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        assert_eq!(io::read_path(&path).unwrap().0.value(0, 0), &Value::Text(expected.into()), "LDID {ldid:02X}");
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text(expected.into())], "LDID {ldid:02X}");
        assert!(src.info().notes.is_empty(), "LDID {ldid:02X}: {:?}", src.info().notes);
    }
}

/// A language driver whose code page RustGIS can't decode (DOS 863, French Canadian) is named in
/// the note, and the text read as UTF-8.
#[test]
fn undecodable_dbf_code_page_is_named_in_the_utf8_fallback() {
    let files = parts("Montréal".as_bytes(), 0x6c, None);
    let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
    assert_eq!(layers[0].0.value(0, 0), &Value::Text("Montréal".into()));
    let said = &layers[0].1.messages;
    assert!(said.iter().any(|m| m.contains("0x6C (code page 863, DOS French Canadian)") && m.contains("UTF-8")), "{said:?}");
}

#[test]
fn unknown_dbf_code_page_reports_utf8_fallback() {
    let files = parts("Montréal".as_bytes(), 0xff, None);
    let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
    assert_eq!(layers[0].0.value(0, 0), &Value::Text("Montréal".into()));
    assert!(layers[0].1.messages.iter().any(|m| m.contains("0xFF") && m.contains("UTF-8")), "{:?}", layers[0].1.messages);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unusable_cpg_reports_fallback_to_dbf_in_all_readers() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    let mut oversized = b"1252".to_vec();
    oversized.resize(2048, b' ');
    for cpg in [Vec::new(), b"not-an-encoding".to_vec(), b"-437".to_vec(), b"UTF-16LE".to_vec(), vec![0xff, 0xfe], oversized] {
        let files = parts(b"Montr\xe9al", 0x03, Some(&cpg));
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.value(0, 0), &Value::Text("Montréal".into()));
        assert!(layers[0].1.messages.iter().any(|m| m.contains(".cpg")), "{:?}", layers[0].1.messages);
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.value(0, 0), &Value::Text("Montréal".into()));
        assert!(report.messages.iter().any(|m| m.contains(".cpg")), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text("Montréal".into())]);
        assert!(src.info().notes.iter().any(|m| m.contains(".cpg")), "{:?}", src.info().notes);
    }
}

#[test]
fn utf8_shapefile_text_survives_missing_metadata_and_utf8_cpg() {
    for cpg in [None, Some(b"UTF-8".as_slice()), Some(b"65001".as_slice())] {
        let files = parts("München – 東京".as_bytes(), if cpg.is_some() { 0x03 } else { 0 }, cpg);
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.value(0, 0), &Value::Text("München – 東京".into()));
        assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_imports_discover_every_cpg_extension_case() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    for ext in ["cpg", "CPG", "Cpg", "cPg", "cpG", "CPg", "CpG", "cPG"] {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in parts(b"Montr\xe9al", 0xf0, Some(b"1252")) {
            let name = if name.ends_with(".cpg") { format!("places.{ext}") } else { name };
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.value(0, 0), &Value::Text("Montréal".into()), "extension {ext}");
        assert!(report.messages.is_empty(), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text("Montréal".into())], "extension {ext}");
        assert!(src.info().notes.is_empty(), "{:?}", src.info().notes);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unreadable_cpg_reports_fallback_without_preventing_native_imports() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    for (ldid, text, expected) in [(0x03, b"Montr\xe9al".as_slice(), "Montréal"), (0, "München".as_bytes(), "München")] {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in parts(text, ldid, None) {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        // A directory at the sidecar path deterministically causes an I/O failure
        // on both Unix and Windows, without depending on the test user's privileges.
        std::fs::create_dir(dir.path().join("places.cpg")).unwrap();
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.value(0, 0), &Value::Text(expected.into()));
        assert!(report.messages.iter().any(|m| m.contains(".cpg") && m.contains("read")), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text(expected.into())]);
        let mut attrs = Vec::new();
        src.visit_chunks(None, 1024, &mut |features| {
            attrs.extend(features.into_iter().map(|f| f.attrs));
            Ok(())
        })
        .unwrap();
        assert_eq!(attrs, vec![vec![Value::Text(expected.into())]]);
        assert!(src.info().notes.iter().any(|m| m.contains(".cpg") && m.contains("read")), "{:?}", src.info().notes);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shapefile_decodes_legacy_field_names_in_all_readers() {
    use rustgis_data::source::{FeatureSource, shp::ShpSource};
    let mut files = parts(b"Montr\xe9al", 0x03, None);
    let dbf = &mut files.iter_mut().find(|(n, _)| n.ends_with(".dbf")).unwrap().1;
    dbf[32..43].fill(0);
    dbf[32..36].copy_from_slice(b"caf\xe9");
    let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
    assert_eq!(layers[0].0.fields[0].name, "café");
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let path = dir.path().join("places.shp");
    assert_eq!(io::read_path(&path).unwrap().0.fields[0].name, "café");
    assert_eq!(ShpSource::open_path(&path, "places").unwrap().info().fields[0].name, "café");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dos_cpg_decodes_attributes_and_field_names_in_all_readers() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    // The same E0 byte is Greek alpha in CP437 and capital O acute in CP850.
    for (cpg, expected) in [("437", "Café α"), ("CP437", "Café α"), ("850", "Café Ó"), ("IBM850", "Café Ó")] {
        let mut files = parts(b"Caf\x82 \xe0", 0xf0, Some(cpg.as_bytes()));
        let dbf = &mut files.iter_mut().find(|(n, _)| n.ends_with(".dbf")).unwrap().1;
        dbf[32..43].fill(0);
        dbf[32..36].copy_from_slice(b"caf\x82");
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.fields[0].name, "café", "CPG {cpg}");
        assert_eq!(layers[0].0.value(0, 0), &Value::Text(expected.into()), "CPG {cpg}");
        assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.fields[0].name, "café");
        assert_eq!(ds.value(0, 0), &Value::Text(expected.into()));
        assert!(report.messages.is_empty(), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.info().fields[0].name, "café");
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text(expected.into())]);
        let mut attrs = Vec::new();
        src.visit_chunks(None, 1024, &mut |features| {
            attrs.extend(features.into_iter().map(|f| f.attrs));
            Ok(())
        })
        .unwrap();
        assert_eq!(attrs, vec![vec![Value::Text(expected.into())]]);
        assert!(src.info().notes.is_empty(), "{:?}", src.info().notes);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dos_dbf_markers_decode_text_without_cpg_in_all_readers() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    for (ldid, expected) in [(0x01, "Café α"), (0x02, "Café Ó"), (0x09, "Café α"), (0x0a, "Café Ó")] {
        let files = parts(b"Caf\x82 \xe0", ldid, None);
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.value(0, 0), &Value::Text(expected.into()), "LDID {ldid:02X}");
        assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.value(0, 0), &Value::Text(expected.into()));
        assert!(report.messages.is_empty(), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text(expected.into())]);
        assert!(src.info().notes.is_empty(), "{:?}", src.info().notes);
    }
}

#[test]
fn dos_support_preserves_other_encodings_and_lossy_utf8_fallback() {
    for (cpg, ldid, text, expected) in [
        (Some(b"ISO-8859-2".as_slice()), 0xf0, b"\xa1".as_slice(), "Ą"),
        (Some(b"CP932".as_slice()), 0xf0, b"\x93\x8c\x8b\x9e".as_slice(), "東京"),
        (Some(b"UTF-8".as_slice()), 0xf0, b"Caf\xff".as_slice(), "Caf�"),
        // With no encoding metadata, preserve the old UTF-8 fallback's treatment of
        // field content: a leading U+FEFF is data, and invalid bytes are replaced.
        (None, 0, b"\xef\xbb\xbfCaf\xff".as_slice(), "\u{feff}Caf�"),
    ] {
        let files = parts(text, ldid, cpg);
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.value(0, 0), &Value::Text(expected.into()));
        assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn utf8_metadata_preserves_leading_bom_in_fields_and_values() {
    use rustgis_data::source::{FeatureSource, Request, shp::ShpSource};
    for (cpg, ldid) in [(None, 0), (None, 0xf0), (Some(b"UTF-8".as_slice()), 0x03), (Some(b"65001".as_slice()), 0x03)] {
        // A BOM inside a fixed-width DBF field is content, not a file signature.
        let mut files = parts(b"\xef\xbb\xbfCaf\xff", ldid, cpg);
        let dbf = &mut files.iter_mut().find(|(n, _)| n.ends_with(".dbf")).unwrap().1;
        dbf[32..43].fill(0);
        dbf[32..39].copy_from_slice(b"\xef\xbb\xbfname");
        let layers = io::read_zip("places.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
        assert_eq!(layers[0].0.fields[0].name, "\u{feff}name");
        assert_eq!(layers[0].0.value(0, 0), &Value::Text("\u{feff}Caf�".into()));
        assert!(layers[0].1.messages.is_empty(), "{:?}", layers[0].1.messages);
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let path = dir.path().join("places.shp");
        let (ds, report) = io::read_path(&path).unwrap();
        assert_eq!(ds.fields[0].name, "\u{feff}name");
        assert_eq!(ds.value(0, 0), &Value::Text("\u{feff}Caf�".into()));
        assert!(report.messages.is_empty(), "{:?}", report.messages);
        let src = ShpSource::open_path(&path, "places").unwrap();
        assert_eq!(src.info().fields[0].name, "\u{feff}name");
        assert_eq!(src.read(&Request::all()).unwrap().features[0].attrs, [Value::Text("\u{feff}Caf�".into())]);
        let mut attrs = Vec::new();
        src.visit_chunks(None, 1024, &mut |features| {
            attrs.extend(features.into_iter().map(|f| f.attrs));
            Ok(())
        })
        .unwrap();
        assert_eq!(attrs, vec![vec![Value::Text("\u{feff}Caf�".into())]]);
        assert!(src.info().notes.is_empty(), "{:?}", src.info().notes);
    }
}
