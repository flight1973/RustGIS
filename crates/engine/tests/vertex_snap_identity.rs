//! Source-addressed snapping for ordinary in-memory feature layers.
use rustgis_engine::{
    Project, Session,
    cmd::design::{snap_except, snap_vertex_except},
    data::{Dataset, Feature, Geometry, GeometryType, VertexId},
    geom::{Coord, merc_to_lonlat},
    map::{Projection, design::Snapping},
};
use serde_json::json;

/// Makes small fixtures in Mercator metres while storing the original longitude/latitude nodes.
fn c(x: f64, y: f64) -> Coord {
    merc_to_lonlat(Coord::new(x, y))
}

/// A public upstream session with one source geometry and a two-pixel snapping tolerance.
fn fixture(g: Geometry) -> (Session, u32) {
    let mut s = Session::new(Project::new("Source vertex snapping"));
    let mut data = Dataset::new("Paths", g.kind(), vec![]);
    data.features = vec![Feature { id: 1, geometry: Some(g), attrs: vec![], zm: None }].into();
    let id = s.map_mut().add_dataset(data);
    s.map_mut().camera.res = 1.0;
    s.map_mut().editing.snapping = Snapping { enabled: true, tolerance_px: 2.0, vertex: false, edge: true, end: false };
    (s, id)
}

/// Requires the expected candidate kind and coordinate, allowing only inverse-projection rounding.
fn hit(actual: Option<(Coord, &'static str)>, expected: Coord, kind: &str) {
    let (at, what) = actual.expect("eligible source target");
    assert_eq!(what, kind);
    assert!((at.x - expected.x).abs() < 1e-7 && (at.y - expected.y).abs() < 1e-7, "{at:?} != {expected:?}");
}

/// The legacy whole-feature exclusion demonstrates the limitation on the very same fixture.
/// Addressed exclusion keeps a different part and a distinct node at the same coordinate.
#[test]
fn another_part_and_a_coincident_node_remain_targets() {
    let (mut s, id) = fixture(Geometry::Line(vec![vec![c(0., 0.), c(0., 100.)], vec![c(0., 0.), c(-100., 0.)]]));
    let node = Some((id, 0, VertexId::new(0, 0, 0)));
    assert!(snap_except(&mut s, c(-50., 0.5), Some((id, 0))).is_none());
    hit(snap_vertex_except(&mut s, c(-50., 0.5), node), c(-50., 0.), "edge");
    assert!(snap_vertex_except(&mut s, c(0.5, 50.), node).is_none(), "the moving node's edge remains excluded");
    s.map_mut().editing.snapping.vertex = true;
    s.map_mut().editing.snapping.edge = false;
    hit(snap_vertex_except(&mut s, c(0.2, 0.1), node), c(0., 0.), "vertex");
}

/// Repeated coordinates within a line are distinct addresses, and a nonincident edge is eligible.
#[test]
fn repeated_coordinates_in_one_part_do_not_exclude_other_nodes() {
    let (mut s, id) = fixture(Geometry::Line(vec![vec![c(0., 0.), c(100., 0.), c(0., 0.), c(0., 100.)]]));
    let node = Some((id, 0, VertexId::new(0, 0, 0)));
    hit(snap_vertex_except(&mut s, c(0.5, 50.), node), c(0., 50.), "edge");
    hit(snap_vertex_except(&mut s, c(50., 0.5), node), c(50., 0.), "edge");
}

/// A polygon's first node and its stored closing copy exclude both incident edges, but neither
/// another polygon's coincident node nor a hole's unrelated boundary is removed.
#[test]
fn polygon_closing_aliases_and_hole_addresses_stay_local() {
    let outer = vec![c(0., 0.), c(100., 0.), c(100., 100.), c(0., 100.), c(0., 0.)];
    let hole = vec![c(20., 20.), c(40., 20.), c(40., 40.), c(20., 40.), c(20., 20.)];
    let other = vec![c(0., 0.), c(-100., 0.), c(-100., -100.), c(0., -100.), c(0., 0.)];
    let (mut s, id) = fixture(Geometry::Polygon(vec![vec![outer, hole], vec![other]]));
    let outer_node = Some((id, 0, VertexId::new(0, 0, 0)));
    assert!(snap_vertex_except(&mut s, c(50., 0.5), outer_node).is_none());
    assert!(snap_vertex_except(&mut s, c(0.5, 50.), outer_node).is_none());
    hit(snap_vertex_except(&mut s, c(30., 20.5), outer_node), c(30., 20.), "edge");
    let hole_node = Some((id, 0, VertexId::new(0, 1, 0)));
    assert!(snap_vertex_except(&mut s, c(30., 20.5), hole_node).is_none());
    hit(snap_vertex_except(&mut s, c(50., 0.5), hole_node), c(50., 0.), "edge");
    s.map_mut().editing.snapping.vertex = true;
    s.map_mut().editing.snapping.edge = false;
    hit(snap_vertex_except(&mut s, c(0.2, 0.1), outer_node), c(0., 0.), "vertex");
    let geometry = s.map().layer(id).unwrap().dataset.features[0].geometry.as_ref().unwrap();
    let moved = geometry.move_vertex(VertexId::new(0, 0, 0), c(-5., -5.)).unwrap();
    let Geometry::Polygon(polygons) = moved else { panic!("polygon") };
    assert_eq!(polygons[0][0].first(), Some(&c(-5., -5.)));
    assert_eq!(polygons[0][0].first(), polygons[0][0].last());
    assert_eq!(polygons[0][1].first(), Some(&c(20., 20.)));
}

/// Unlike polygon closure, a closed line has two independent end addresses. The unchanged
/// edit command moves only the requested end, and still makes one undoable edit.
#[test]
fn closed_line_endpoints_keep_the_upstream_edit_contract() {
    let (mut s, id) = fixture(Geometry::Line(vec![vec![c(0., 0.), c(100., 0.), c(100., 100.), c(0., 100.), c(0., 0.)]]));
    s.map_mut().editing.snapping = Snapping { edge: false, vertex: false, end: true, ..s.map().editing.snapping };
    for index in [0, 4] {
        hit(snap_vertex_except(&mut s, c(0.2, 0.1), Some((id, 0, VertexId::new(0, 0, index)))), c(0., 0.), "end");
    }
    s.clear_history();
    let at = c(-20., 0.);
    s.run("edit.vertex", &json!({"layer":id,"row":0,"action":"move","index":0,"lon":at.x,"lat":at.y})).unwrap();
    let geometry = s.map().layer(id).unwrap().dataset.features[0].geometry.as_ref().unwrap();
    assert_eq!(geometry.vertex(VertexId::new(0, 0, 0)), Some(at));
    assert_eq!(geometry.vertex(VertexId::new(0, 0, 4)), Some(c(0., 0.)));
    assert_eq!(s.undo_depth(), 1);
    s.run("edit.undo", &json!(null)).unwrap();
    let geometry = s.map().layer(id).unwrap().dataset.features[0].geometry.as_ref().unwrap();
    assert_eq!(geometry.vertex(VertexId::new(0, 0, 0)), Some(c(0., 0.)));
}

/// The existing point-before-edge choice, type switches, tolerance and disable switch survive.
#[test]
fn switches_tolerance_and_point_priority_remain_unchanged() {
    let (mut s, id) =
        fixture(Geometry::Line(vec![vec![c(0., 0.), c(0., 100.)], vec![c(100., 0.), c(100., 100.)], vec![c(80., 0.2), c(120., 0.2)]]));
    let node = Some((id, 0, VertexId::new(0, 0, 0)));
    s.map_mut().editing.snapping.vertex = true;
    hit(snap_vertex_except(&mut s, c(99., 0.), node), c(100., 0.), "vertex");
    s.map_mut().editing.snapping.vertex = false;
    hit(snap_vertex_except(&mut s, c(99., 0.), node), c(99., 0.2), "edge");
    s.map_mut().editing.snapping.edge = false;
    s.map_mut().editing.snapping.end = true;
    hit(snap_vertex_except(&mut s, c(98.01, 0.), node), c(100., 0.), "end");
    assert!(snap_vertex_except(&mut s, c(97.99, 0.), node).is_none());
    s.map_mut().editing.snapping.end = false;
    assert!(snap_vertex_except(&mut s, c(99., 0.), node).is_none());
    s.map_mut().editing.snapping.enabled = false;
    assert!(snap_vertex_except(&mut s, c(100., 0.), node).is_none());
}

/// No exclusion, feature exclusion and the existing Point/MultiPoint policy are unchanged.
/// Invalid source addresses are ignored and cannot mutate the project or add undo entries.
#[test]
fn legacy_policy_and_invalid_addresses_are_preserved() {
    for geometry in [Geometry::Point(c(0., 0.)), Geometry::MultiPoint(vec![c(0., 0.)]), Geometry::Line(vec![vec![c(0., 0.), c(100., 0.)]])]
    {
        let (mut s, id) = fixture(geometry);
        let expected = snap_except(&mut s, c(0., 0.), None);
        let before = serde_json::to_value(&s.project).unwrap();
        let revision = s.revision;
        for skip in [
            None,
            Some((u32::MAX, 0, VertexId::new(0, 0, 0))),
            Some((id, usize::MAX, VertexId::new(0, 0, 0))),
            Some((id, 0, VertexId::new(usize::MAX, usize::MAX, usize::MAX))),
        ] {
            assert_eq!(snap_vertex_except(&mut s, c(0., 0.), skip), expected);
        }
        assert!(snap_except(&mut s, c(0., 0.), Some((id, 0))).is_none());
        assert_eq!(serde_json::to_value(&s.project).unwrap(), before);
        assert_eq!(s.revision, revision);
        assert_eq!(s.undo_depth(), 0);
    }
}

/// The addressed row keeps its place in the original layer/row tie order; hidden layers stay out.
#[test]
fn source_row_order_and_visibility_still_choose_the_first_tie() {
    let (mut s, id) = fixture(Geometry::Line(vec![vec![c(100., 100.), c(-1., 0.)]]));
    let layer = s.map_mut().layer_mut(id).unwrap();
    layer.dataset.features.push(Feature {
        id: 2,
        geometry: Some(Geometry::Line(vec![vec![c(200., 100.), c(1., 0.)]])),
        attrs: vec![],
        zm: None,
    });
    s.map_mut().editing.snapping.vertex = true;
    s.map_mut().editing.snapping.edge = false;
    hit(snap_vertex_except(&mut s, c(0., 0.), Some((id, 1, VertexId::new(0, 0, 0)))), c(-1., 0.), "vertex");
    hit(snap_vertex_except(&mut s, c(0., 0.), Some((id, 0, VertexId::new(0, 0, 0)))), c(-1., 0.), "vertex");
    s.map_mut().layer_mut(id).unwrap().visible = false;
    assert!(snap_vertex_except(&mut s, c(0., 0.), Some((id, 0, VertexId::new(0, 0, 0)))).is_none());
}

/// World and regional map projections keep source identities and inverse-projected targets.
#[test]
fn world_and_regional_projections_use_source_nodes() {
    for code in ["equirectangular", "EPSG:4549"] {
        let at = Coord::new(120.001, 31.);
        let (mut s, id) =
            fixture(Geometry::Line(vec![vec![Coord::new(120., 31.), Coord::new(120., 31.01)], vec![at, Coord::new(120.001, 31.01)]]));
        s.map_mut().projection = Projection::from_code(code).unwrap();
        s.map_mut().camera.center = s.map().to_map(at).unwrap();
        s.map_mut().camera.res = 1.0;
        s.map_mut().editing.snapping.vertex = true;
        s.map_mut().editing.snapping.edge = false;
        let node = Some((id, 0, VertexId::new(0, 0, 0)));
        hit(snap_vertex_except(&mut s, at, node), at, "vertex");
        assert!(snap_vertex_except(&mut s, Coord::new(120., 31.), node).is_none());
    }
}

/// A world-edge cut adds rendered parts but cannot renumber source exclusions, invent node
/// targets at cut intersections, or create an edge across the middle of the world.
#[test]
fn projection_cuts_do_not_supply_source_addresses_or_bridge_the_world() {
    let (mut s, id) = fixture(Geometry::Line(vec![
        vec![Coord::new(80., -20.), Coord::new(100., -20.)],
        vec![Coord::new(-100., 20.), Coord::new(-110., 20.), Coord::new(-110., 30.)],
    ]));
    s.map_mut().projection = Projection::from_code("mollweide +lon_0=-90").unwrap();
    s.map_mut().camera.res = 1000.;
    s.map_mut().editing.snapping.vertex = true;
    s.map_mut().editing.snapping.edge = false;
    let node = Some((id, 0, VertexId::new(1, 0, 0)));
    hit(snap_vertex_except(&mut s, Coord::new(-110., 20.), node), Coord::new(-110., 20.), "vertex");
    assert!(snap_vertex_except(&mut s, Coord::new(-100., 20.), node).is_none());
    assert!(snap_vertex_except(&mut s, Coord::new(90., -20.), node).is_none());
    let Geometry::Line(parts) = s.map().layer(id).unwrap().projected_cache()[0].as_ref().unwrap() else { panic!("line") };
    assert!(parts.len() > 2, "the fixture must actually split at the world edge");
    s.map_mut().editing.snapping.vertex = false;
    s.map_mut().editing.snapping.edge = true;
    assert!(snap_vertex_except(&mut s, Coord::new(-90., -20.), node).is_none());
    hit(snap_vertex_except(&mut s, Coord::new(89., -20.), node), Coord::new(89., -20.), "edge");
    // This remains an ordinary in-memory feature layer.
    assert_eq!(s.map().layer(id).unwrap().geometry_type(), GeometryType::Polyline);
}
