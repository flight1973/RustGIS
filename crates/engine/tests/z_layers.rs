//! 3-D layers (#726, option E): a layer read with heights keeps them in `CoordZ` positions, so
//! reading, drawing, editing, tools, exports and projects either keep each height with its point
//! or say that it was dropped.
use rustgis_engine::{
    Project, Session,
    data::{CoordZ, Dataset, Geom},
};
use serde_json::{Value, json};

/// A private scratch folder for one test.
fn scratch(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rustgis-z-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A line with a different height at each vertex, a polygon whose ring is drawn clockwise, and a
/// point, all with heights that a float round trip would show if they moved.
const TRACK: &str = r#"{"type": "FeatureCollection", "features": [
  {"type": "Feature", "properties": {"name": "climb"}, "geometry": {"type": "LineString",
    "coordinates": [[7.0, 46.0, 512.25], [7.1, 46.05, 1033.5], [7.2, 46.1, 2101.125], [7.3, 46.12, 3007.0625]]}},
  {"type": "Feature", "properties": {"name": "roof"}, "geometry": {"type": "Polygon",
    "coordinates": [[[7.0, 46.0, 10.5], [7.0, 46.01, 11.5], [7.01, 46.01, 12.5], [7.01, 46.0, 13.5], [7.0, 46.0, 10.5]]]}},
  {"type": "Feature", "properties": {"name": "summit"}, "geometry": {"type": "Point", "coordinates": [7.25, 46.11, 4478.0]}}
]}"#;

/// Every position of every feature's geometry in a GeoJSON text, as written.
fn positions(text: &str) -> Vec<Vec<Value>> {
    fn walk(v: &Value, out: &mut Vec<Vec<Value>>) {
        match v.as_array() {
            Some(a) if a.first().is_some_and(Value::is_number) => out.push(a.clone()),
            Some(a) => a.iter().for_each(|x| walk(x, out)),
            None => {}
        }
    }
    let doc: Value = serde_json::from_str(text).unwrap();
    let mut out = Vec::new();
    for f in doc["features"].as_array().unwrap() {
        walk(&f["geometry"]["coordinates"], &mut out);
    }
    out
}

/// A session with `text` added from `name` in a scratch folder: the session, the notices the read
/// gave, and the layer's id.
fn added(dir: &std::path::Path, name: &str, text: &str) -> (Session, Vec<String>, u32) {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let mut s = Session::new(Project::default());
    let mark = s.notice_mark();
    s.run("layer.addData", &json!({"path": path})).unwrap();
    let said = s.notices_since(mark).iter().map(|n| n.text.clone()).collect();
    let id = s.map().layers.last().unwrap().id;
    (s, said, id)
}

fn layer(s: &Session, id: u32) -> &Dataset {
    &s.map().layer(id).unwrap().dataset
}

/// Exports layer `id` as GeoJSON: the text and what the export said (besides naming the file).
fn export(s: &mut Session, id: u32, path: &std::path::Path) -> (String, Vec<String>) {
    let mark = s.notice_mark();
    s.run("share.exportLayer", &json!({"layer": id, "path": path})).unwrap();
    let said = s.notices_since(mark).iter().map(|n| n.text.clone()).filter(|t| !t.starts_with("Exported ")).collect();
    (std::fs::read_to_string(path).unwrap(), said)
}

#[test]
fn a_geojson_with_heights_reads_as_a_3d_layer_draws_and_exports_every_height_exactly() {
    let dir = scratch("roundtrip");
    let (mut s, said, id) = added(&dir, "track.geojson", TRACK);
    assert!(said.iter().all(|n| !n.contains("Z or M") && !n.contains("height")), "{said:?}");
    let ds = layer(&s, id);
    assert!(ds.has_z() && !ds.zm_dropped);
    assert!(ds.features.iter().all(|f| f.geometry_z().is_some()));
    // The map draws the 2-D view: x and y of the same points.
    let summit = ds.features.get(2).unwrap();
    assert_eq!(summit.geometry, Some(Geom::Point(rustgis_engine::data::Coord::new(7.25, 46.11))));
    let png = dir.join("map.png");
    s.run("share.exportMap", &json!({"path": png})).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 0);
    // Every height comes back, exactly, with its point.
    let (text, said) = export(&mut s, id, &dir.join("out.geojson"));
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(positions(&text), positions(TRACK));
}

#[test]
fn a_2d_geojson_reads_as_a_2d_layer_with_no_new_notice() {
    let dir = scratch("flat");
    let flat = TRACK.replace(", 512.25]", "]").replace(", 1033.5]", "]").replace(", 2101.125]", "]").replace(", 3007.0625]", "]");
    let flat =
        flat.replace(", 10.5]", "]").replace(", 11.5]", "]").replace(", 12.5]", "]").replace(", 13.5]", "]").replace(", 4478.0]", "]");
    let (mut s, said, id) = added(&dir, "flat.geojson", &flat);
    assert!(said.is_empty(), "{said:?}");
    assert!(!layer(&s, id).has_z());
    let (text, said) = export(&mut s, id, &dir.join("out.geojson"));
    assert!(said.is_empty(), "{said:?}");
    assert!(positions(&text).iter().all(|p| p.len() == 2));
}

#[test]
fn a_file_with_some_features_without_heights_is_a_2d_layer_and_says_so() {
    let dir = scratch("mixed");
    let mixed = TRACK.replace("[7.25, 46.11, 4478.0]", "[7.25, 46.11]");
    let (s, said, id) = added(&dir, "mixed.geojson", &mixed);
    assert!(!layer(&s, id).has_z());
    assert!(said.iter().any(|n| n.contains("2 features had Z or M values")), "{said:?}");
}

#[test]
fn flipping_a_3d_line_keeps_each_height_with_its_point() {
    let dir = scratch("flip");
    let (mut s, _, id) = added(&dir, "track.geojson", TRACK);
    s.run("select.rows", &json!({"layer": id, "rows": [0]})).unwrap();
    let mark = s.notice_mark();
    s.run("edit.flip", &Value::Null).unwrap();
    assert!(s.notices_since(mark).iter().all(|n| !n.text.contains("height")));
    let f = layer(&s, id).features.first().unwrap();
    let Some(Geom::Line(parts)) = f.geometry_z() else { panic!("no heights: {f:?}") };
    let heights: Vec<(f64, f64, f64)> = parts[0].iter().map(|c| (c.x, c.y, c.z)).collect();
    assert_eq!(heights, [(7.3, 46.12, 3007.0625), (7.2, 46.1, 2101.125), (7.1, 46.05, 1033.5), (7.0, 46.0, 512.25)]);
    // Undo brings back the line as read, heights and all.
    s.run("edit.undo", &Value::Null).unwrap();
    let Some(Geom::Line(parts)) = layer(&s, id).features.first().unwrap().geometry_z() else { panic!("no heights after undo") };
    assert_eq!(parts[0][0], CoordZ::new(7.0, 46.0, 512.25));
}

#[test]
fn moving_a_vertex_keeps_its_height_and_an_inserted_one_takes_the_height_along_its_edge() {
    let dir = scratch("vertex");
    let (mut s, _, id) = added(&dir, "track.geojson", TRACK);
    s.run("edit.vertex", &json!({"layer": id, "row": 0, "action": "move", "index": 1, "lon": 7.11, "lat": 46.06})).unwrap();
    let z = |s: &Session| match layer(s, id).features.first().unwrap().geometry_z() {
        Some(Geom::Line(parts)) => parts[0].clone(),
        other => panic!("no heights: {other:?}"),
    };
    assert_eq!(z(&s)[1], CoordZ::new(7.11, 46.06, 1033.5));
    // Or takes the height it is given.
    s.run("edit.vertex", &json!({"layer": id, "row": 0, "action": "move", "index": 1, "lon": 7.11, "lat": 46.06, "z": 1100.0})).unwrap();
    assert_eq!(z(&s)[1].z, 1100.0);
    // Halfway along the edge from vertex 2 to vertex 3: halfway between their heights.
    s.run("edit.vertex", &json!({"layer": id, "row": 0, "action": "insert", "index": 2, "lon": 7.25, "lat": 46.11})).unwrap();
    let line = z(&s);
    assert_eq!(line.len(), 5);
    assert!((line[3].z - (2101.125 + 3007.0625) / 2.0).abs() < 1e-6, "{:?}", line[3]);
    assert_eq!((line[2].z, line[4].z), (2101.125, 3007.0625));
    // Deleting one takes its height with it.
    s.run("edit.vertex", &json!({"layer": id, "row": 0, "action": "delete", "index": 0})).unwrap();
    assert_eq!(z(&s)[0], CoordZ::new(7.11, 46.06, 1100.0));
}

#[test]
fn a_2d_edit_of_a_3d_feature_drops_its_heights_and_says_so() {
    let dir = scratch("edit2d");
    let (mut s, _, id) = added(&dir, "track.geojson", TRACK);
    let mark = s.notice_mark();
    // The layer's kind is Polygon, its most common (the last of a tie).
    let roof = json!({"type": "Polygon", "coordinates": [[[7.0, 46.0], [7.0, 46.02], [7.02, 46.02], [7.0, 46.0]]]});
    s.run("edit.setGeometry", &json!({"layer": id, "row": 1, "geometry": roof})).unwrap();
    assert!(s.notices_since(mark).iter().any(|n| n.text.contains("1 feature(s) lost their heights")));
    assert!(!layer(&s, id).features.get(1).unwrap().has_z());
    // The export writes the others' heights, and says the one was written without.
    let (text, said) = export(&mut s, id, &dir.join("out.geojson"));
    assert!(said.iter().any(|n| n.contains("1 feature of track was written without heights")), "{said:?}");
    let written = positions(&text);
    assert_eq!(written.iter().filter(|p| p.len() == 2).count(), 4);
    assert_eq!(written.iter().filter(|p| p.len() == 3).count(), 5);
}

#[test]
fn a_2d_write_that_forgets_the_heights_loses_them_and_never_pairs_them_with_other_points() {
    // What code that knows nothing of heights does: reverse the 2-D line in place.
    let dir = scratch("forgot");
    let (mut s, _, id) = added(&dir, "track.geojson", TRACK);
    let l = s.map_mut().layer_mut(id).unwrap();
    if let Some(Geom::Line(parts)) = &mut l.dataset.features[0].geometry {
        parts[0].reverse();
    }
    let f = layer(&s, id).features.first().unwrap();
    assert!(f.lost_z() && f.geometry_z().is_none());
    let (text, said) = export(&mut s, id, &dir.join("out.geojson"));
    assert!(said.iter().any(|n| n.contains("1 feature of track was written without heights")), "{said:?}");
    assert_eq!(positions(&text)[0], [json!(7.3), json!(46.12)]);
}

#[test]
fn a_2d_tool_on_a_3d_layer_says_the_heights_were_dropped() {
    let dir = scratch("tool");
    let (mut s, _, _) = added(&dir, "track.geojson", TRACK);
    let mark = s.notice_mark();
    s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "track", "distance": 100}})).unwrap();
    let said: Vec<String> = s.notices_since(mark).iter().map(|n| n.text.clone()).collect();
    assert!(said.iter().any(|n| n.contains("the heights (Z) of 'track' weren't kept")), "{said:?}");
    // A 2-D input says nothing of heights.
    let flat = Dataset::new("flat", rustgis_engine::data::GeometryType::Point, vec![]);
    let mut flat = flat;
    flat.features.push(rustgis_engine::data::Feature::new(1, Some(Geom::Point(rustgis_engine::data::Coord::new(7.0, 46.0))), vec![]));
    s.map_mut().add_dataset(flat);
    let mark = s.notice_mark();
    s.run("gp.run", &json!({"tool": "buffer", "params": {"input": "flat", "distance": 100}})).unwrap();
    assert!(s.notices_since(mark).iter().all(|n| !n.text.contains("heights")));
}

#[test]
fn a_project_with_a_3d_layer_saves_and_reopens_with_every_height() {
    let dir = scratch("project");
    let (mut s, _, id) = added(&dir, "track.geojson", TRACK);
    // Its source is gone, so the project's own copy of the features is what reopens.
    std::fs::remove_file(dir.join("track.geojson")).unwrap();
    s.run("select.rows", &json!({"layer": id, "rows": [0]})).unwrap();
    s.run("edit.flip", &Value::Null).unwrap();
    let before = layer(&s, id).clone();
    let path = dir.join("project.rgis");
    s.run("project.saveAs", &json!({"path": path})).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved["requires"].as_array().is_some_and(|r| r.contains(&json!("feature-heights"))), "{}", saved["requires"]);
    let mut opened = Session::new(Project::new("Other"));
    opened.run("project.open", &json!({"path": path})).unwrap();
    let back = &opened.map().layer_by_name("track").unwrap().dataset;
    assert!(back.has_z());
    assert_eq!(back.features.len(), before.features.len());
    for (a, b) in before.features.iter().zip(back.features.iter()) {
        assert_eq!(a.geometry_z(), b.geometry_z());
        assert!(b.geometry_z().is_some());
    }
}
