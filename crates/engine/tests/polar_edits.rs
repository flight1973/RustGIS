//! Moving, rotating or scaling a polygon poleward of 85.05°S keeps it there, rigidly (#1033).
use rustgis_engine::{
    Project, Session,
    data::{Dataset, Feature, Field, FieldType, Geometry, GeometryType, Value},
    geom::{Aeqd, Coord, spherical_centre, wgs84_distance_m},
};
use serde_json::json;

/// A session whose one layer holds `geometries`, all selected, ready to edit.
fn layer(kind: GeometryType, geometries: Vec<Geometry>) -> (Session, u32) {
    let mut data = Dataset::new("Polar", kind, vec![Field::new("n", FieldType::Integer)]);
    let n = geometries.len();
    data.features = geometries
        .into_iter()
        .zip(1..)
        .map(|(g, id)| Feature { id, geometry: Some(g), attrs: vec![Value::Int(0)] })
        .collect::<Vec<_>>()
        .into();
    let mut s = Session::new(Project::new("Polar edits"));
    let id = s.map_mut().add_dataset(data);
    s.active_layer = Some(id);
    s.map_mut().editing.settings.enabled = true;
    s.map_mut().layer_mut(id).unwrap().selection = (0..n).collect();
    (s, id)
}

/// A session whose one layer holds `ring` as a selected polygon, ready to edit.
fn session(ring: Vec<Coord>) -> (Session, u32) {
    layer(GeometryType::Polygon, vec![Geometry::Polygon(vec![vec![ring]])])
}

/// The polygon from 87°S to 89°S of the issue.
fn polar_ring() -> Vec<Coord> {
    let c = Coord::new;
    vec![c(0.0, -89.0), c(1.0, -89.0), c(1.0, -87.0), c(0.0, -87.0), c(0.0, -89.0)]
}

fn ring_of(s: &Session, id: u32, row: usize) -> Vec<Coord> {
    match &s.map().layer(id).unwrap().dataset.features[row].geometry {
        Some(Geometry::Polygon(p)) => p[0][0].clone(),
        g => panic!("{g:?}"),
    }
}

fn stored(s: &Session, id: u32) -> Vec<Coord> {
    ring_of(s, id, 0)
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
    let plane = Aeqd::new(center);
    let sides = |r: &[Coord]| r.windows(2).map(|w| wgs84_distance_m(w[0], w[1])).collect::<Vec<_>>();
    let reach = |r: &[Coord], c: Coord| r[..4].iter().map(|v| wgs84_distance_m(c, *v)).collect::<Vec<_>>();
    let close = |a: &[f64], b: &[f64], k: f64| a.iter().zip(b).all(|(x, y)| ((x * k) / y - 1.0).abs() < 1e-3);
    // Each corner's bearing from the center, clockwise from north (degrees).
    let bearings = |r: &[Coord]| r[..4].iter().map(|v| plane.forward(*v)).map(|p| p.x.atan2(p.y).to_degrees()).collect::<Vec<_>>();

    // 10 km east: the center goes 10 km, east, and the sides keep their lengths (to 0.1 %).
    let (mut s, id) = session(before.clone());
    s.run("edit.move", &json!({"dx": 10.0, "dy": 0.0, "units": "Kilometers"})).unwrap();
    let moved = stored(&s, id);
    let to = plane.forward(spherical_centre(&moved[..4]).unwrap());
    assert!((to.x - 10_000.0).abs() < 10.0 && to.y.abs() < 10.0 && close(&sides(&before), &sides(&moved), 1.0), "{to:?} {moved:?}");

    // Rotated 30°, every corner stays as far from the center and turns 30° clockwise about it;
    // scaled, it goes 1.5 times as far.
    let (mut s, id) = session(before.clone());
    s.run("edit.rotate", &json!({"angle": 30.0})).unwrap();
    let after = stored(&s, id);
    let turned =
        bearings(&before).iter().zip(bearings(&after)).all(|(a, b)| ((b - a + 540.0).rem_euclid(360.0) - 180.0 - 30.0).abs() < 0.01);
    assert!(turned && close(&reach(&before, center), &reach(&after, center), 1.0), "{after:?}");
    let (mut s, id) = session(before.clone());
    s.run("edit.scale", &json!({"factor": 1.5})).unwrap();
    let after = stored(&s, id);
    assert!(close(&reach(&before, center), &reach(&after, center), 1.5), "{after:?}");
}

#[test]
fn polar_neighbors_moved_together_still_share_their_corners() {
    // Side by side from 87°S to 89°S, sharing the edge along 10°E.
    let c = Coord::new;
    let ring = |x0: f64, x1: f64| Geometry::Polygon(vec![vec![vec![c(x0, -89.0), c(x1, -89.0), c(x1, -87.0), c(x0, -87.0), c(x0, -89.0)]]]);
    let (mut s, id) = layer(GeometryType::Polygon, vec![ring(0.0, 10.0), ring(10.0, 20.0)]);
    s.run("edit.move", &json!({"dx": 1.0, "dy": 0.5, "units": "Kilometers"})).unwrap();
    let (a, b) = (ring_of(&s, id, 0), ring_of(&s, id, 1));
    // The first's east side is the second's west side, vertex for vertex.
    assert!(a[1] == b[0] && a[2] == b[3], "{a:?} {b:?}");
}

#[test]
fn a_polygon_across_85_degrees_moves_without_being_clamped() {
    let c = Coord::new;
    let (mut s, id) = session(vec![c(0.0, -86.0), c(1.0, -86.0), c(1.0, -84.0), c(0.0, -84.0), c(0.0, -86.0)]);
    s.run("edit.move", &json!({"dx": 10.0, "units": "Kilometers"})).unwrap();
    let lats: Vec<f64> = stored(&s, id).iter().map(|c| c.y).collect();
    assert!(lats.iter().any(|y| *y < -85.9) && lats.iter().all(|y| (-86.1..-83.9).contains(y)), "{lats:?}");
}

#[test]
fn an_edit_that_would_carry_a_polygon_over_a_pole_is_refused() {
    // 300 km south takes the polygon across the South Pole: refused, and left as it was.
    let (mut s, id) = session(polar_ring());
    let e = s.run("edit.move", &json!({"dx": 0.0, "dy": -300.0, "units": "Kilometers"})).unwrap_err();
    assert!(e.contains("onto or around a pole"), "{e}");
    assert_eq!(stored(&s, id), polar_ring());
}

#[test]
fn a_point_at_a_pole_can_be_moved_off_it() {
    let (mut s, id) = layer(GeometryType::Point, vec![Geometry::Point(Coord::new(0.0, -90.0))]);
    s.run("edit.move", &json!({"dx": 0.0, "dy": 10.0, "units": "Kilometers"})).unwrap();
    let Some(Geometry::Point(p)) = &s.map().layer(id).unwrap().dataset.features[0].geometry else { panic!() };
    assert!((wgs84_distance_m(Coord::new(0.0, -90.0), *p) - 10_000.0).abs() < 1.0, "{p:?}");
}

#[test]
fn a_line_beyond_85_degrees_splits_where_it_is_asked() {
    let c = Coord::new;
    let (mut s, id) = layer(GeometryType::Polyline, vec![Geometry::Line(vec![vec![c(0.0, -86.0), c(0.0, -89.0)]])]);
    s.run("edit.split", &json!({"lon": 0.0, "lat": -87.5})).unwrap();
    let mut lats = Vec::new();
    for f in s.map().layer(id).unwrap().dataset.features.iter() {
        if let Some(g) = &f.geometry {
            g.for_each_coord(|c| lats.push(c.y));
        }
    }
    // Split at 87.5°S, not where Web Mercator stops (85.05°S).
    assert!(lats.iter().any(|y| (y + 87.5).abs() < 1e-9) && lats.iter().all(|y| *y < -85.9), "{lats:?}");
}

#[test]
fn a_feature_reaching_a_pole_is_refused_and_left_alone() {
    let c = Coord::new;
    // A cap around the South Pole, as files cut it: along 89°S, down to the pole, and back.
    let cap = vec![c(-180.0, -89.0), c(180.0, -89.0), c(180.0, -90.0), c(-180.0, -90.0), c(-180.0, -89.0)];
    let (mut s, id) = session(cap.clone());
    let e = s.run("edit.move", &json!({"dx": 1000.0})).unwrap_err();
    assert!(e.contains("reaches a pole"), "{e}");
    assert_eq!(stored(&s, id), cap);
}
