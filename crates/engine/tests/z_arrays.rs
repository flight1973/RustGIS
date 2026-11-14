//! Heights (Z) kept beside 2D geometry (#726, a prototype of storing Z and M as arrays beside the
//! geometry): GeoJSON with heights read with `layer.addData`, exported with `share.exportLayer`
//! and read again keeps every height exactly; what can't be kept is said; an edit that changes a
//! feature's vertices exports that feature in 2D and says so; a saved project keeps the heights.
#[path = "portable_projects/common.rs"]
#[allow(dead_code)]
mod common;

use common::Fixture;
use rustgis_engine::{Project, Session, data::Dataset};
use serde_json::{Value, json};

/// Points, a line, a polygon with a hole and a multipolygon with heights, and one flat point.
/// The heights lean on edges: a tiny one, a negative one with all its digits, a fraction.
const HEIGHTS: &str = r#"{"type":"FeatureCollection","features":[
{"type":"Feature","properties":{"name":"well"},"geometry":{"type":"Point","coordinates":[8.5,47.3,0.1]}},
{"type":"Feature","properties":{"name":"track"},"geometry":{"type":"LineString","coordinates":[[8.5,47.3,-12345.678901234567],[8.6,47.4,1e-300],[8.7,47.5,8848.86]]}},
{"type":"Feature","properties":{"name":"roof"},"geometry":{"type":"Polygon","coordinates":[[[0,0,10],[1,0,11],[1,1,12],[0,0,10]],[[0.2,0.2,5],[0.4,0.2,6],[0.4,0.4,7],[0.2,0.2,5]]]}},
{"type":"Feature","properties":{"name":"sites"},"geometry":{"type":"MultiPolygon","coordinates":[[[[2,2,1],[3,2,2],[3,3,3],[2,2,1]]],[[[4,4,4],[5,4,5],[5,5,6],[4,4,4]]]]}},
{"type":"Feature","properties":{"name":"flat"},"geometry":{"type":"Point","coordinates":[1,1]}}
]}"#;

/// Each feature's heights, as bits so that equality is exact.
fn heights(ds: &Dataset) -> Vec<Option<Vec<u64>>> {
    ds.features.iter().map(|f| f.zm.as_ref().and_then(|zm| zm.z.as_ref()).map(|z| z.iter().map(|h| h.to_bits()).collect())).collect()
}

/// Adds `path` to a new map of `s`: the layer added, and what was said.
fn add(s: &mut Session, path: &std::path::Path) -> (Dataset, Vec<String>) {
    let before: Vec<u32> = s.map().layers.iter().map(|l| l.id).collect();
    let mark = s.notice_mark();
    s.run("layer.addData", &json!({"path": path})).unwrap();
    let said = s.notices_since(mark).iter().map(|n| n.text.clone()).collect();
    let layer = s.map().layers.iter().find(|l| !before.contains(&l.id)).unwrap();
    (layer.dataset.clone(), said)
}

/// Exports layer `name` of `s` to `path`: what was said besides naming the file.
fn export(s: &mut Session, name: &str, path: &std::path::Path) -> Vec<String> {
    let mark = s.notice_mark();
    s.run("share.exportLayer", &json!({"layer": name, "path": path})).unwrap();
    s.notices_since(mark).iter().map(|n| n.text.clone()).filter(|t| !t.starts_with("Exported ")).collect()
}

/// Whether a notice speaks of heights or measures.
fn of_heights(said: &[String]) -> Vec<&String> {
    said.iter().filter(|t| t.contains("Z") || t.contains("height") || t.contains("measure")).collect()
}

#[test]
fn heights_survive_a_geojson_round_trip_exactly() {
    let f = Fixture::new("z-trip");
    let src = f.file("heights.geojson");
    std::fs::write(&src, HEIGHTS).unwrap();
    let mut s = Session::new(Project::default());
    let (read, said) = add(&mut s, &src);
    assert!(of_heights(&said).is_empty(), "nothing lost on reading: {said:?}");
    assert!(!read.zm_dropped);
    let expected = [
        Some(vec![0.1f64]),
        Some(vec![-12345.678901234567, 1e-300, 8848.86]),
        Some(vec![10., 11., 12., 10., 5., 6., 7., 5.]),
        Some(vec![1., 2., 3., 1., 4., 5., 6., 4.]),
        None,
    ];
    let bits: Vec<Option<Vec<u64>>> = expected.iter().map(|z| z.as_ref().map(|z| z.iter().map(|h| h.to_bits()).collect())).collect();
    assert_eq!(heights(&read), bits);
    assert!(read.features.iter().all(|f| f.zm_fits()));

    let out = f.file("out.geojson");
    let said = export(&mut s, "heights", &out);
    assert!(of_heights(&said).is_empty(), "nothing lost on export: {said:?}");
    // Written as [x, y, z] positions, the flat point as [x, y].
    let text: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(text["features"][1]["geometry"]["coordinates"][1], json!([8.6, 47.4, 1e-300]));
    assert_eq!(text["features"][4]["geometry"]["coordinates"], json!([1.0, 1.0]));

    let (back, said) = add(&mut s, &out);
    assert!(of_heights(&said).is_empty(), "{said:?}");
    assert_eq!(heights(&back), bits);
    assert_eq!(
        back.features.iter().map(|f| &f.geometry).collect::<Vec<_>>(),
        read.features.iter().map(|f| &f.geometry).collect::<Vec<_>>()
    );
}

#[test]
fn heights_on_only_some_points_or_a_fourth_number_are_said() {
    let f = Fixture::new("z-mixed");
    let src = f.file("mixed.geojson");
    std::fs::write(
        &src,
        r#"{"type":"FeatureCollection","features":[
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[1,2,3],[3,4]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[1,2,3,4],[3,4,5,6]]}},
{"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[1,2,null],[3,4,null]]}}
]}"#,
    )
    .unwrap();
    let mut s = Session::new(Project::default());
    let (ds, said) = add(&mut s, &src);
    assert!(ds.zm_dropped);
    let notes = of_heights(&said);
    assert!(notes.iter().any(|t| t.contains("1 feature had heights (Z) on only some of their points")), "{said:?}");
    assert!(notes.iter().any(|t| t.contains("1 feature had a fourth number")), "{said:?}");
    // The mixed feature keeps no heights; the one with a measure keeps its heights; null isn't one.
    let kept: Vec<bool> = ds.features.iter().map(|f| f.zm.is_some()).collect();
    assert_eq!(kept, [false, true, false]);
    assert_eq!(heights(&ds)[1], Some(vec![3f64.to_bits(), 5f64.to_bits()]));
    // Exported, the import's loss is said, without claiming all heights were dropped.
    let said = export(&mut s, "mixed", &f.file("out.geojson"));
    assert!(said.iter().any(|t| t.contains("lacks Z or M values that were dropped when mixed was read")), "{said:?}");
}

#[test]
fn flat_geojson_says_nothing_new() {
    let f = Fixture::new("z-flat");
    let src = f.file("flat.geojson");
    std::fs::write(
        &src,
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"n":1},"geometry":{"type":"LineString","coordinates":[[1,2],[3,4]]}}]}"#,
    )
    .unwrap();
    let mut s = Session::new(Project::default());
    let (ds, said) = add(&mut s, &src);
    assert!(said.is_empty(), "{said:?}");
    assert!(!ds.zm_dropped && ds.features.iter().all(|f| f.zm.is_none()));
    let said = export(&mut s, "flat", &f.file("out.geojson"));
    assert!(said.is_empty(), "{said:?}");
}

#[test]
fn an_edit_that_changes_vertices_exports_that_feature_flat_and_says_so() {
    let f = Fixture::new("z-edit");
    let src = f.file("tracks.geojson");
    let track = |y: f64| json!({"type": "Feature", "properties": {}, "geometry": {"type": "LineString", "coordinates": [[0, y, 1], [1, y, 2], [2, y, 3]]}});
    std::fs::write(&src, json!({"type": "FeatureCollection", "features": [track(0.0), track(1.0), track(2.0)]}).to_string()).unwrap();
    let mut s = Session::new(Project::default());
    add(&mut s, &src);
    s.map_mut().editing.settings.enabled = true;
    // Replaced by a line of as many vertices: the heights would fit, but aren't its own.
    let other = json!({"type": "LineString", "coordinates": [[9, 48], [9, 49], [9, 50]]});
    s.run("edit.setGeometry", &json!({"layer": "tracks", "row": 0, "geometry": other})).unwrap();
    // A vertex added: three heights no longer fit four vertices.
    s.run("edit.vertex", &json!({"layer": "tracks", "row": 1, "action": "insert", "index": 0, "lon": 0.5, "lat": 1.0})).unwrap();
    // A vertex moved: the same vertices in the same order keep their heights.
    s.run("edit.vertex", &json!({"layer": "tracks", "row": 2, "action": "move", "index": 1, "lon": 1.1, "lat": 2.0})).unwrap();
    let out = f.file("out.geojson");
    let said = export(&mut s, "tracks", &out);
    assert_eq!(
        of_heights(&said),
        [&format!(
            "{}: heights (Z) of 2 features weren't written: an edit or a tool changed their vertices, so their heights no longer fit them",
            out.display()
        )],
        "{said:?}"
    );
    let text: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(text["features"][0]["geometry"]["coordinates"], json!([[9.0, 48.0], [9.0, 49.0], [9.0, 50.0]]));
    assert_eq!(text["features"][1]["geometry"]["coordinates"][1], json!([0.5, 1.0]));
    assert_eq!(text["features"][2]["geometry"]["coordinates"], json!([[0.0, 2.0, 1.0], [1.1, 2.0, 2.0], [2.0, 2.0, 3.0]]));
    // Undone, every track's heights fit again.
    for _ in 0..3 {
        s.run("edit.undo", &Value::Null).unwrap();
    }
    let said = export(&mut s, "tracks", &f.file("again.geojson"));
    assert!(of_heights(&said).is_empty(), "{said:?}");
}

#[test]
fn other_formats_say_they_leave_heights_out() {
    let f = Fixture::new("z-other");
    let src = f.file("heights.geojson");
    std::fs::write(&src, HEIGHTS).unwrap();
    let mut s = Session::new(Project::default());
    add(&mut s, &src);
    let out = f.file("out.csv");
    let said = export(&mut s, "heights", &out);
    assert!(
        said.iter().any(|t| t.contains("heights or measures (Z or M) of 4 features weren't written: only GeoJSON exports keep heights")),
        "{said:?}"
    );
}

#[test]
fn a_saved_project_keeps_the_heights() {
    let f = Fixture::new("z-project");
    let src = f.file("heights.geojson");
    std::fs::write(&src, HEIGHTS).unwrap();
    let mut s = Session::new(Project::default());
    let (read, _) = add(&mut s, &src);
    let project = f.file("heights.rgis");
    s.run("project.saveAs", &json!({"path": project})).unwrap();
    // The heights are in the project, not read again from the file.
    std::fs::remove_file(&src).unwrap();
    let mut opened = Session::new(Project::default());
    opened.run("project.open", &json!({"path": project})).unwrap();
    let layer = opened.map().layer_by_name("heights").unwrap();
    assert_eq!(heights(&layer.dataset), heights(&read));
    assert!(heights(&read).iter().flatten().count() == 4);
}
