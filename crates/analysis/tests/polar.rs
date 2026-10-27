//! Polar features keep their shape and their relations through the analysis tools (#1033):
//! nothing is clamped onto 85.05°, where Web Mercator ends.
use rustgis_analysis::{Inputs, Output, ToolResult};
use rustgis_data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value};
use std::collections::HashMap;

struct Map(HashMap<String, Dataset>);
impl Inputs for Map {
    fn dataset(&self, layer: &str) -> Option<&Dataset> {
        self.0.get(layer)
    }
    fn selection(&self, _: &str) -> &[usize] {
        &[]
    }
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Geometry {
    let c = Coord::new;
    Geometry::Polygon(vec![vec![vec![c(x0, y0), c(x1, y0), c(x1, y1), c(x0, y1), c(x0, y0)]]])
}

fn layer(name: &str, geometries: Vec<Geometry>) -> (String, Dataset) {
    let mut ds = Dataset::new(name, GeometryType::Polygon, vec![Field::new("k", FieldType::Text)]);
    for g in geometries {
        ds.push(Some(g), vec![Value::Text("a".into())]);
    }
    (name.to_string(), ds)
}

/// Two overlapping polygons between 87°S and 89°S, a mask over them, and polygons from 86°S to 87°S
/// and 88°S to 89°S, about 111 km apart.
fn polar() -> Map {
    Map(HashMap::from([
        layer("polar", vec![square(0.0, -89.0, 1.0, -87.0), square(0.5, -89.0, 1.5, -87.0)]),
        layer("mask", vec![square(-10.0, -89.9, 10.0, -86.0)]),
        layer("upper", vec![square(0.0, -87.0, 1.0, -86.0)]),
        layer("lower", vec![square(0.0, -89.0, 1.0, -88.0)]),
    ]))
}

fn dataset(r: ToolResult) -> Dataset {
    match r.output {
        Output::Layer(d) | Output::Table(d) => d,
        _ => panic!("expected a layer"),
    }
}

/// Every latitude of every feature's geometry, with the feature count.
fn latitudes(ds: &Dataset) -> (usize, Vec<f64>) {
    let mut lats = Vec::new();
    for f in ds.features.iter() {
        if let Some(g) = &f.geometry {
            g.for_each_coord(|c| lats.push(c.y));
        }
    }
    (ds.features.len(), lats)
}

fn run(m: &Map, tool: &str, params: serde_json::Value) -> Dataset {
    dataset(rustgis_analysis::run(tool, m, &params).unwrap_or_else(|e| panic!("{tool}: {e}")))
}

#[test]
fn polar_polygons_keep_their_shape_through_the_tools() {
    let m = polar();
    let between = |lats: &[f64]| !lats.is_empty() && lats.iter().all(|y| (-89.0 - 1e-9..=-87.0 + 1e-9).contains(y));
    let reach = |lats: &[f64], y: f64| lats.iter().any(|v| (v - y).abs() < 1e-9);

    // Clip by a mask that covers them: both come through whole.
    let (n, lats) = latitudes(&run(&m, "clip", serde_json::json!({"input": "polar", "clip": "mask"})));
    assert!(n == 2 && between(&lats) && reach(&lats, -89.0) && reach(&lats, -87.0), "clip: {n} {lats:?}");
    // Dissolve unions them into one polygon spanning both (to the overlay's grid, under a
    // millimeter: 1e-7° here).
    let d = run(&m, "dissolve", serde_json::json!({"input": "polar", "field": "k"}));
    let (n, lats) = latitudes(&d);
    let e = d.features.first().and_then(|f| f.geometry.as_ref()).map(|g| g.extent()).unwrap();
    assert!(n == 1 && between(&lats) && e.min_x.abs() < 1e-7 && (e.max_x - 1.5).abs() < 1e-7, "dissolve: {lats:?} {e:?}");
    // Simplify keeps a square's corners, hull and points stay inside.
    let (_, lats) = latitudes(&run(&m, "simplify", serde_json::json!({"input": "polar", "tolerance": 0.001})));
    assert!(between(&lats) && reach(&lats, -89.0) && reach(&lats, -87.0), "simplify: {lats:?}");
    let (_, lats) = latitudes(&run(&m, "convexHull", serde_json::json!({"input": "polar"})));
    assert!(between(&lats) && reach(&lats, -89.0) && reach(&lats, -87.0), "convexHull: {lats:?}");
    let (n, lats) = latitudes(&run(&m, "featureToPoint", serde_json::json!({"input": "polar"})));
    assert!(n == 2 && lats.iter().all(|y| (-89.0..-87.0).contains(y)), "featureToPoint: {lats:?}");
}

#[test]
fn polar_polygons_apart_are_apart() {
    let m = polar();
    // 86–87°S and 88–89°S don't meet: they did when both were clamped onto 85.05°S.
    let r = rustgis_analysis::run("selectByLocation", &m, &serde_json::json!({"input": "upper", "select": "lower"})).unwrap();
    let Output::Selection { rows, .. } = r.output else { panic!("expected a selection") };
    assert!(rows.is_empty(), "{rows:?}");
    // The closest one is 1° of latitude away near 88°S: 111.69 km on the WGS 84 meridian, not 0.
    let joined =
        run(&m, "spatialJoin", serde_json::json!({"target": "upper", "join": "lower", "match": "Closest", "distanceField": "gap"}));
    let gap = joined.field_index("gap").map(|i| joined.value(0, i).clone());
    let km = match gap {
        Some(Value::Float(v)) => v,
        other => panic!("gap: {other:?}"),
    };
    assert!((km - 111.69).abs() < 0.05, "{km}");
}

#[test]
fn a_radius_near_a_pole_reaches_as_far_as_it_says() {
    let c = Coord::new;
    let mut pts = Dataset::new("pts", GeometryType::Point, vec![]);
    // 33.5 km and 55.8 km north of 89°N, and 447 km away at 85°N, on the same meridian.
    for y in [89.3, 89.5, 85.0] {
        pts.push(Some(Geometry::Point(c(10.0, y))), vec![]);
    }
    let found = rustgis_analysis::network::within(&pts, &[0, 1, 2], c(10.0, 89.0), 50_000.0);
    assert_eq!(found.iter().map(|(r, _)| *r).collect::<Vec<_>>(), [0], "{found:?}");
    let d = found.first().map(|(_, d)| *d).unwrap_or_default();
    assert!((d - 33_507.0).abs() < 50.0, "{d}");
}

#[test]
fn features_reaching_the_same_pole_meet_there() {
    // Wedges from 89°S to the South Pole at 0°E and 90°E: apart in the plane, where the pole is a
    // line, but one place on the ground. One at 0°E reaching the North Pole meets neither.
    let wedge = |x: f64, s: f64| square(x, s * 90.0, x + 10.0, s * 89.0);
    let m = Map(HashMap::from([
        layer("south0", vec![wedge(0.0, -1.0)]),
        layer("south90", vec![wedge(90.0, -1.0)]),
        layer("north0", vec![wedge(0.0, 1.0)]),
    ]));
    let selected = |input: &str, select: &str| {
        let r = rustgis_analysis::run("selectByLocation", &m, &serde_json::json!({"input": input, "select": select})).unwrap();
        let Output::Selection { rows, .. } = r.output else { panic!("expected a selection") };
        rows
    };
    assert_eq!((selected("south0", "south90"), selected("north0", "south90")), (vec![0], vec![]));
    let joined = |target: &str, join: &str, how: &str| {
        let mut params = serde_json::json!({"target": target, "join": join, "match": how});
        if how == "Closest" {
            params["distanceField"] = serde_json::json!("gap");
        }
        run(&m, "spatialJoin", params)
    };
    let sj = joined("south0", "south90", "Intersects");
    assert_eq!(sj.value(0, sj.field_index("join_count").unwrap()), &Value::Int(1));
    let sj = joined("north0", "south90", "Intersects");
    assert_eq!(sj.value(0, sj.field_index("join_count").unwrap()), &Value::Int(0));
    let sj = joined("south0", "south90", "Closest");
    assert_eq!(sj.value(0, sj.field_index("gap").unwrap()), &Value::Float(0.0));
}
