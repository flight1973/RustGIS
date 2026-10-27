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
fn a_cap_around_a_pole_keeps_its_pole_through_the_overlays() {
    let c = Coord::new;
    // As files cut a cap around the South Pole: along 85°S, down to the pole at 180°, and back.
    let cap = Geometry::Polygon(vec![vec![vec![c(-180.0, -85.0), c(180.0, -85.0), c(180.0, -90.0), c(-180.0, -90.0), c(-180.0, -85.0)]]]);
    let m = Map(HashMap::from([layer("cap", vec![cap]), layer("mask", vec![square(-180.0, -90.0, 180.0, -80.0)])]));
    // Within the overlays' grid of the pole and of 85°S, and nowhere in between them.
    let pole = |lats: &[f64]| lats.iter().any(|y| (y + 90.0).abs() <= rustgis_geom::POLE_TOLERANCE);
    let edge = |lats: &[f64]| lats.iter().any(|y| (y + 85.0).abs() < 1e-7);
    for (tool, params) in [
        ("clip", serde_json::json!({"input": "cap", "clip": "mask"})),
        ("dissolve", serde_json::json!({"input": "cap", "field": "k"})),
        ("simplify", serde_json::json!({"input": "cap", "tolerance": 0.001})),
    ] {
        let (n, lats) = latitudes(&run(&m, tool, params));
        assert!(n == 1 && pole(&lats) && edge(&lats) && lats.iter().all(|y| (-90.0 - 1e-9..=-85.0 + 1e-7).contains(y)), "{tool}: {lats:?}");
    }

    // A wedge from 0°E to 90°E, clipped, still reaches the pole, where it meets one at 120°E: the
    // overlay's grid moves its pole vertices, but not out of the pole's tolerance.
    let m = Map(HashMap::from([
        layer("wedge", vec![square(0.0, -90.0, 90.0, -89.0)]),
        layer("mask", vec![square(-180.0, -90.0, 180.0, -80.0)]),
    ]));
    let clipped = run(&m, "clip", serde_json::json!({"input": "wedge", "clip": "mask"}));
    let off = latitudes(&clipped).1.iter().map(|y| (y + 90.0).abs()).fold(f64::INFINITY, f64::min);
    let m = Map(HashMap::from([("clipped".to_string(), clipped), layer("other", vec![square(120.0, -90.0, 130.0, -89.0)])]));
    let r = rustgis_analysis::run("selectByLocation", &m, &serde_json::json!({"input": "clipped", "select": "other"})).unwrap();
    let Output::Selection { rows, .. } = r.output else { panic!("expected a selection") };
    assert_eq!(rows, [0], "pole vertices {off:e}° off");
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

    // 120 km around the South Pole reaches 89°S (111.7 km) and not 88.9°S (122.9 km).
    let mut pts = Dataset::new("pts", GeometryType::Point, vec![]);
    for (x, y) in [(0.0, -89.0), (10.0, -88.9)] {
        pts.push(Some(Geometry::Point(c(x, y))), vec![]);
    }
    let found = rustgis_analysis::network::within(&pts, &[0, 1], c(0.0, -90.0), 120_000.0);
    assert_eq!(found.iter().map(|(r, _)| *r).collect::<Vec<_>>(), [0], "{found:?}");
}

#[test]
fn a_radius_reaches_polar_shapes_along_their_edges() {
    let c = Coord::new;
    let ground = rustgis_geom::wgs84_distance_m;
    let shapes = |g: Geometry| {
        let mut ds = Dataset::new("shapes", GeometryType::Polygon, vec![]);
        ds.push(Some(g), vec![]);
        ds
    };
    let reach = |ds: &Dataset, at: Coord, r: f64| rustgis_analysis::network::within(ds, &[0], at, r).first().map(|(_, d)| *d);
    // Inside a box from 84°N to 89°N: no distance at all.
    assert_eq!(reach(&shapes(square(0.0, 84.0, 90.0, 89.0)), c(45.0, 85.0), 50_000.0), Some(0.0));
    // A cap cut at ±180°, its edge along 85.5°N half a degree north: not at the pole, 556 km away.
    let cap = Geometry::Polygon(vec![vec![vec![c(-180.0, 85.5), c(180.0, 85.5), c(180.0, 90.0), c(-180.0, 90.0), c(-180.0, 85.5)]]]);
    let d = reach(&shapes(cap), c(0.0, 85.0), 100_000.0);
    assert!(d.is_some_and(|d| (d - ground(c(0.0, 85.0), c(0.0, 85.5))).abs() < 10.0), "{d:?}");
    // A meridian from pole to pole, a degree west at 45°N: about 79 km off, not 0 by a chord.
    let mut line = Dataset::new("meridian", GeometryType::Polyline, vec![]);
    line.push(Some(Geometry::Line(vec![vec![c(0.0, -90.0), c(0.0, 90.0)]])), vec![]);
    let d = reach(&line, c(1.0, 45.0), 100_000.0);
    assert!(d.is_some_and(|d| (d - ground(c(1.0, 45.0), c(0.0, 45.0))).abs() < 100.0), "{d:?}");
}

#[test]
fn line_ends_near_a_pole_meet_within_the_tolerance() {
    // Two short lines at 89.6°N, 500 m apart in the plane, 3.5 m on the ground: no dangles at 10 m.
    let c = Coord::new;
    let dx = (500.0 / rustgis_geom::MERCATOR_R).to_degrees();
    let mut ds = Dataset::new("lines", GeometryType::Polyline, vec![]);
    for x in [0.0, dx] {
        ds.push(Some(Geometry::Line(vec![vec![c(x, 89.6), c(x, 89.6001)]])), vec![]);
    }
    let checked = rustgis_analysis::topology::dangles(&ds, &[0, 1], 10.0);
    assert!(checked.complete && checked.errors.is_empty(), "{:?}", checked.errors.iter().map(|e| e.at).collect::<Vec<_>>());
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
    let r = rustgis_analysis::run("selectByLocation", &m, &serde_json::json!({"input": "south0", "select": "south90", "invert": "Yes"}))
        .unwrap();
    let Output::Selection { rows, .. } = r.output else { panic!("expected a selection") };
    assert!(rows.is_empty(), "{rows:?}");
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

#[test]
fn a_spatial_join_finds_every_feature_at_the_pole_through_its_index() {
    // Twenty wedges reaching the South Pole all around it, and five squares far off: enough join
    // features (over 16) for Spatial Join to look them up in its grid, where a wedge's extent
    // meets only its neighbors'.
    let wedges = (0..20).map(|i| square(-180.0 + 18.0 * f64::from(i), -90.0, -172.0 + 18.0 * f64::from(i), -89.0));
    let far = (0..5).map(|i| square(f64::from(i), 0.0, f64::from(i) + 0.5, 0.5));
    let m = Map(HashMap::from([layer("target", vec![square(0.0, -90.0, 10.0, -89.0)]), layer("join", wedges.chain(far).collect())]));
    let sj = run(&m, "spatialJoin", serde_json::json!({"target": "target", "join": "join", "match": "Intersects"}));
    assert_eq!(sj.value(0, sj.field_index("join_count").unwrap()), &Value::Int(20));
}
