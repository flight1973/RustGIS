//! Moving, rotating or scaling a polygon poleward of 85.05°S keeps it there, rigidly (#1033).
use rustgis_engine::{
    Project, Session,
    data::{Dataset, Feature, Field, FieldType, Geometry, GeometryType, Value},
    geom::{Coord, spherical_centre, wgs84_distance_m},
};
use serde_json::json;

/// A session whose one layer holds `ring` as a selected polygon, ready to edit.
fn session(ring: Vec<Coord>) -> (Session, u32) {
    let mut data = Dataset::new("Polar", GeometryType::Polygon, vec![Field::new("n", FieldType::Integer)]);
    data.features = vec![Feature { id: 1, geometry: Some(Geometry::Polygon(vec![vec![ring]])), attrs: vec![Value::Int(0)] }].into();
    let mut s = Session::new(Project::new("Polar edits"));
    let id = s.map_mut().add_dataset(data);
    s.active_layer = Some(id);
    s.map_mut().editing.settings.enabled = true;
    s.map_mut().layer_mut(id).unwrap().selection = vec![0];
    (s, id)
}

/// The polygon from 87°S to 89°S of the issue.
fn polar_ring() -> Vec<Coord> {
    let c = Coord::new;
    vec![c(0.0, -89.0), c(1.0, -89.0), c(1.0, -87.0), c(0.0, -87.0), c(0.0, -89.0)]
}

fn stored(s: &Session, id: u32) -> Vec<Coord> {
    match &s.map().layer(id).unwrap().dataset.features[0].geometry {
        Some(Geometry::Polygon(p)) => p[0][0].clone(),
        g => panic!("{g:?}"),
    }
}

#[test]
fn small_edits_keep_a_polar_polygon_where_it_is() {
    let mut wrong = Vec::new();
    for (cmd, params) in
        [("edit.move", json!({"dx": 1000.0, "dy": 0.0})), ("edit.rotate", json!({"angle": 10.0})), ("edit.scale", json!({"factor": 1.01}))]
    {
        let (mut s, id) = session(polar_ring());
        s.run(cmd, &params).unwrap();
        let lats: Vec<f64> = stored(&s, id).iter().map(|c| c.y).collect();
        // The polygon spans 87°S to 89°S, about 222 km; none of these edits moves it 50 km north.
        if lats.iter().any(|y| *y > -86.5) {
            wrong.push(format!("{cmd}: latitudes now {lats:.4?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn polar_edits_are_rigid_on_the_ground() {
    let before = polar_ring();
    let center = spherical_centre(&before[..4]).unwrap();
    let sides = |r: &[Coord]| r.windows(2).map(|w| wgs84_distance_m(w[0], w[1])).collect::<Vec<_>>();
    let reach = |r: &[Coord], c: Coord| r[..4].iter().map(|v| wgs84_distance_m(c, *v)).collect::<Vec<_>>();
    let close = |a: &[f64], b: &[f64], k: f64| a.iter().zip(b).all(|(x, y)| ((x * k) / y - 1.0).abs() < 1e-3);

    // 10 km east: the center goes 10 km, and the sides keep their lengths (to 0.1 %).
    let (mut s, id) = session(before.clone());
    s.run("edit.move", &json!({"dx": 10.0, "dy": 0.0, "units": "Kilometers"})).unwrap();
    let moved = stored(&s, id);
    let gone = wgs84_distance_m(center, spherical_centre(&moved[..4]).unwrap());
    assert!((gone - 10_000.0).abs() < 10.0 && close(&sides(&before), &sides(&moved), 1.0), "{gone} {moved:?}");

    // Rotated, every corner stays as far from the center; scaled, 1.5 times as far.
    for (cmd, params, k) in [("edit.rotate", json!({"angle": 30.0}), 1.0), ("edit.scale", json!({"factor": 1.5}), 1.5)] {
        let (mut s, id) = session(before.clone());
        s.run(cmd, &params).unwrap();
        let after = stored(&s, id);
        assert!(close(&reach(&before, center), &reach(&after, center), k), "{cmd}: {after:?}");
    }
}

#[test]
fn a_feature_reaching_a_pole_is_refused_and_left_alone() {
    let c = Coord::new;
    // A cap round the South Pole, as files cut it: along 89°S, down to the pole, and back.
    let cap = vec![c(-180.0, -89.0), c(180.0, -89.0), c(180.0, -90.0), c(-180.0, -90.0), c(-180.0, -89.0)];
    let (mut s, id) = session(cap.clone());
    let e = s.run("edit.move", &json!({"dx": 1000.0})).unwrap_err();
    assert!(e.contains("reaches a pole"), "{e}");
    assert_eq!(stored(&s, id), cap);
}
