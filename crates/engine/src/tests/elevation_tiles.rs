//! SRTM height tiles and DTED, added natively by path and as uploads (#2076).
use super::*;

/// A 1201 × 1201 SRTM tile: heights by row, one void.
fn hgt_tile() -> Vec<u8> {
    let mut b = Vec::with_capacity(1201 * 1201 * 2);
    for row in 0..1201usize {
        for col in 0..1201usize {
            let v: i16 = if (row, col) == (5, 5) { i16::MIN } else { (row % 300) as i16 };
            b.extend_from_slice(&v.to_be_bytes());
        }
    }
    b
}

/// A DTED level 1 cell of 1201 × 1201 posts at 3″ from 6°E, 45°S (MIL-PRF-89020B: UHL, DSI, ACC,
/// then one record a longitude line).
fn dt1_cell() -> Vec<u8> {
    let mut b = format!("UHL10060000E0450000S00300030NA  U  {:12}120112010{:24}", "", "").into_bytes();
    let mut dsi = vec![b' '; 648];
    dsi[..3].copy_from_slice(b"DSI");
    dsi[141..144].copy_from_slice(b"E96");
    dsi[144..149].copy_from_slice(b"WGS84");
    b.extend(dsi);
    let mut acc = vec![b' '; 2700];
    acc[..3].copy_from_slice(b"ACC");
    b.extend(acc);
    for i in 0..1201u16 {
        let mut rec = vec![0xaa, 0, (i >> 8) as u8, i as u8];
        rec.extend(i.to_be_bytes());
        rec.extend([0, 0]);
        for j in 0..1201u16 {
            // Negative heights in signed magnitude.
            let v = if j % 2 == 0 { j % 100 } else { 0x8000 | (j % 100) };
            rec.extend(v.to_be_bytes());
        }
        let sum: u32 = rec.iter().map(|b| u32::from(*b)).sum();
        rec.extend(sum.to_be_bytes());
        b.extend(rec);
    }
    b
}

/// The layer `result` names: its raster's extent and pixel size.
fn placed(s: &Session, result: &Value) -> (rustgis_data::Extent, u32, u32) {
    let layer = s.map().layer(result["layer"].as_u64().unwrap() as u32).unwrap();
    let r = layer.raster.as_ref().unwrap().data.as_ref().unwrap();
    assert!(r.georeferenced);
    assert_eq!(r.crs, rustgis_data::raster::Crs::LonLat);
    (r.extent(), r.width, r.height)
}

fn near(e: rustgis_data::Extent, [w, s, east, n]: [f64; 4]) -> bool {
    [(e.min_x, w), (e.min_y, s), (e.max_x, east), (e.max_y, n)].iter().all(|(a, b)| (a - b).abs() < 1e-9)
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn srtm_and_dted_are_added_by_path_with_their_extent() {
    let dir = std::env::temp_dir().join(format!("rustgis-elevation-tiles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let half = 1.5 / 3600.0;
    for (name, bytes, bbox) in [
        ("N37W122.hgt", hgt_tile(), [-122.0 - half, 37.0 - half, -121.0 + half, 38.0 + half]),
        ("S45E006.dt1", dt1_cell(), [6.0 - half, -45.0 - half, 7.0 + half, -44.0 + half]),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut s = Session::new(Project::default());
        let result = s.run("layer.addData", &json!({"path": path})).unwrap();
        let (extent, w, h) = placed(&s, &result);
        assert_eq!((w, h), (1201, 1201), "{name}");
        assert!(near(extent, bbox), "{name}: {extent:?}");
        // The vertical reference is said once, as the layer is added.
        assert_eq!(s.notices.iter().filter(|n| n.text.contains("EGM96 geoid")).count(), 1, "{name}: {:?}", s.notices);
        let voids = s.notices.iter().filter(|n| n.text.contains("void")).count();
        assert_eq!(voids, usize::from(name.ends_with(".hgt")), "{name}: {:?}", s.notices);
    }
    // A tile's name places it: one that doesn't is refused, saying so, not handed to GDAL.
    let unnamed = dir.join("srtm.hgt");
    std::fs::write(&unnamed, hgt_tile()).unwrap();
    let e = Session::new(Project::default()).run("layer.addData", &json!({"path": unnamed})).unwrap_err().to_string();
    assert!(e.contains("doesn't say where the tile is"), "{e}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_srtm_upload_keeps_its_name_and_place() {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(hgt_tile());
    let mut s = Session::new(Project::default());
    let result = s.run("layer.addDataBytes", &json!({"fileName": "S01E006.hgt", "data": data})).unwrap();
    let (extent, _, _) = placed(&s, &result);
    let half = 1.5 / 3600.0;
    assert!(near(extent, [6.0 - half, -1.0 - half, 7.0 + half, 0.0 + half]), "{extent:?}");
    assert!(s.notices.iter().any(|n| n.text.contains("1 of its 1442401 heights is a void")), "{:?}", s.notices);
    // A cut-short upload is refused.
    let cut = base64::engine::general_purpose::STANDARD.encode(&hgt_tile()[..1000]);
    assert!(Session::new(Project::default()).run("layer.addDataBytes", &json!({"fileName": "S01E006.hgt", "data": cut})).is_err());
}
