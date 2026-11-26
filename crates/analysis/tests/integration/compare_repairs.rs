//! Compare Repairs through the public tool dispatch (#2003): one row for each feature Repair
//! Geometry would repair, with what it would write under each rule, the areas on the ground, the
//! area Even-Odd adds and takes out, parts and holes, and whether the rules differ; valid features
//! left out; selections; a layer held in a projected system measured on the ground; and the
//! notices, clean and not.
use rustgis_analysis::{Inputs, Output, ToolResult};
use rustgis_data::raster::Crs;
use rustgis_data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value};
use serde_json::json;

struct Layer {
    ds: Dataset,
    selected: Vec<usize>,
}
impl Inputs for Layer {
    fn dataset(&self, name: &str) -> Option<&Dataset> {
        (name == "input").then_some(&self.ds)
    }
    fn selection(&self, _: &str) -> &[usize] {
        &self.selected
    }
}

type Ring<'a> = &'a [(f64, f64)];

/// The size of a grid cell, in degrees: shapes are drawn on a grid of these near (15° E, 0°).
const CELL: f64 = 0.001;

/// `r` on the grid near (15° E, 0°), in degrees.
fn pts(r: Ring) -> Vec<Coord> {
    r.iter().map(|&(x, y)| Coord::new(15.0 + x * CELL, y * CELL)).collect()
}
fn multi(parts: &[&[Ring]]) -> Geometry {
    Geometry::Polygon(parts.iter().map(|rings| rings.iter().map(|r| pts(r)).collect()).collect())
}
fn layer(ty: GeometryType, geometries: Vec<Option<Geometry>>) -> Layer {
    let mut ds = Dataset::new("input", ty, vec![Field::new("name", FieldType::Text)]);
    for (i, g) in geometries.into_iter().enumerate() {
        ds.push_next(g, vec![Value::Text(format!("f{i}"))]);
    }
    Layer { ds, selected: vec![] }
}
fn run(l: &Layer) -> ToolResult {
    rustgis_analysis::run("compareRepairs", l, &json!({"input": "input"})).unwrap()
}
fn table(r: &ToolResult) -> &Dataset {
    let Output::Table(ds) = &r.output else { panic!("expected a table") };
    ds
}
fn value<'a>(ds: &'a Dataset, row: usize, field: &str) -> &'a Value {
    ds.value(row, ds.field_index(field).unwrap_or_else(|| panic!("no field {field}")))
}
fn float(ds: &Dataset, row: usize, field: &str) -> f64 {
    match value(ds, row, field) {
        Value::Float(x) => *x,
        v => panic!("{field} is {v:?}"),
    }
}
fn int(ds: &Dataset, row: usize, field: &str) -> i64 {
    match value(ds, row, field) {
        Value::Int(n) => *n,
        v => panic!("{field} is {v:?}"),
    }
}
fn text(ds: &Dataset, row: usize, field: &str) -> String {
    match value(ds, row, field) {
        Value::Text(t) => t.clone(),
        v => panic!("{field} is {v:?}"),
    }
}

/// The ground area of one grid cell near (15° E, 0°), in m², on the WGS 84 ellipsoid.
fn cell_m2() -> f64 {
    let (_, a) = rustgis_geom::wgs84_ring_perimeter_area_m2(&pts(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]));
    a.abs()
}

/// `got` is `cells` grid cells of area, to within a millionth of the cell (the overlay rounds).
fn cells(got: f64, cells: f64, what: &str) {
    let c = cell_m2();
    assert!((got - cells * c).abs() <= c * 1e-6, "{what}: {got} m² is not {cells} cells of {c} m²");
}

const A: Ring = &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)];
const B: Ring = &[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0), (1.0, 1.0)];
const BIG: Ring = &[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0), (0.0, 0.0)];
const INNER: Ring = &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0), (1.0, 1.0)];
const BOWTIE: Ring = &[(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0), (0.0, 0.0)];
const OPEN: Ring = &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];

/// Two overlapping parts: Shells Minus Holes merges them (7 cells), Even-Odd leaves the overlap out
/// (6 cells), so the rules differ and Even-Odd takes out one cell and adds none.
#[test]
fn overlapping_parts_differ_by_their_overlap() {
    let r = run(&layer(GeometryType::Polygon, vec![Some(multi(&[&[A], &[B]]))]));
    let t = table(&r);
    assert_eq!(t.features.len(), 1);
    // Its id: the first feature's is 1.
    assert_eq!(int(t, 0, "feature"), 1);
    assert_eq!(value(t, 0, "differ"), &Value::Bool(true));
    assert_eq!(text(t, 0, "shells_repairs"), "parts_overlap");
    assert_eq!(text(t, 0, "even_odd_repairs"), "parts_overlap, even_odd");
    cells(float(t, 0, "shells_m2"), 7.0, "shells_m2");
    cells(float(t, 0, "even_odd_m2"), 6.0, "even_odd_m2");
    cells(float(t, 0, "added_m2"), 0.0, "added_m2");
    cells(float(t, 0, "removed_m2"), 1.0, "removed_m2");
    assert_eq!((int(t, 0, "shells_parts"), int(t, 0, "shells_holes")), (1, 0));
    assert_eq!(int(t, 0, "even_odd_holes"), 0);
    assert_eq!(r.messages, ["Compare Repairs: 1 of 1 feature needs repairs, and the two rules differ on 1"]);
    assert!(r.losses.is_empty());
}

/// A hole stored as a part of its own inside another: Shells Minus Holes fills it, Even-Odd keeps
/// it a hole, so Even-Odd takes out its cell.
#[test]
fn a_hole_stored_as_a_part_differs_by_the_hole() {
    let r = run(&layer(GeometryType::Polygon, vec![Some(multi(&[&[BIG], &[INNER]]))]));
    let t = table(&r);
    assert_eq!(value(t, 0, "differ"), &Value::Bool(true));
    cells(float(t, 0, "shells_m2"), 9.0, "shells_m2");
    cells(float(t, 0, "even_odd_m2"), 8.0, "even_odd_m2");
    cells(float(t, 0, "removed_m2"), 1.0, "removed_m2");
    cells(float(t, 0, "added_m2"), 0.0, "added_m2");
    assert_eq!((int(t, 0, "shells_parts"), int(t, 0, "shells_holes")), (1, 0));
    assert_eq!((int(t, 0, "even_odd_parts"), int(t, 0, "even_odd_holes")), (1, 1));
}

/// Features both rules repair the same way: a bowtie (both triangles kept either way), a ring left
/// open (closed without moving a coordinate) and a missing geometry. Each has a row, and the rules
/// don't differ on any; valid features have none.
#[test]
fn the_rules_agree_where_they_mean_the_same() {
    let square = Some(multi(&[&[A]]));
    let r = run(&layer(GeometryType::Polygon, vec![square.clone(), Some(multi(&[&[BOWTIE]])), Some(multi(&[&[OPEN]])), None, square]));
    let t = table(&r);
    assert_eq!(t.features.len(), 3);
    let ids: Vec<i64> = (0..3).map(|row| int(t, row, "feature")).collect();
    assert_eq!(ids, [2, 3, 4], "valid features have no row");
    for row in 0..3 {
        assert_eq!(value(t, row, "differ"), &Value::Bool(false), "row {row}");
        cells(float(t, row, "added_m2"), 0.0, "added_m2");
        cells(float(t, row, "removed_m2"), 0.0, "removed_m2");
    }
    // The bowtie's two triangles, a cell each.
    cells(float(t, 0, "shells_m2"), 2.0, "bowtie");
    cells(float(t, 0, "even_odd_m2"), 2.0, "bowtie");
    assert_eq!((int(t, 0, "shells_parts"), int(t, 0, "even_odd_parts")), (2, 2));
    // Closed with its first point under either rule, so not rebuilt: no even_odd.
    assert_eq!(text(t, 1, "shells_repairs"), "ring_not_closed");
    assert_eq!(text(t, 1, "even_odd_repairs"), "ring_not_closed");
    cells(float(t, 1, "shells_m2"), 4.0, "closed ring");
    // Written without geometry either way: no area, no parts.
    assert_eq!(text(t, 2, "shells_repairs"), "null_geometry");
    assert_eq!(float(t, 2, "shells_m2"), 0.0);
    assert_eq!(int(t, 2, "even_odd_parts"), 0);
    assert_eq!(r.messages, ["Compare Repairs: 3 of 5 features need repairs, and the two rules differ on 0"]);
}

/// The repairs fields say what Repair Geometry writes in its repairs field for each rule.
#[test]
fn the_repairs_fields_are_repair_geometrys() {
    let l = layer(
        GeometryType::Polygon,
        vec![Some(multi(&[&[A], &[B]])), Some(multi(&[&[BIG], &[INNER]])), Some(multi(&[&[BOWTIE]])), Some(multi(&[&[OPEN]])), None],
    );
    let t = table(&run(&l)).clone();
    for (rule, field) in [("Shells Minus Holes", "shells_repairs"), ("Even-Odd", "even_odd_repairs")] {
        let r = rustgis_analysis::run("repairGeometry", &l, &json!({"input": "input", "rule": rule})).unwrap();
        let Output::Layer(repaired) = &r.output else { panic!("a layer") };
        for row in 0..t.features.len() {
            // Repair Geometry writes every feature, in order, keeping its id.
            let id = int(&t, row, "feature") as u64;
            let at = (0..repaired.features.len()).find(|&k| repaired.features[k].id == id).unwrap();
            assert_eq!(text(&t, row, field), text(repaired, at, "repairs"), "{rule}, feature {id}");
        }
    }
}

/// Only the selected features are compared.
#[test]
fn only_the_selection_is_compared() {
    let mut l = layer(GeometryType::Polygon, vec![Some(multi(&[&[A], &[B]])), Some(multi(&[&[BOWTIE]]))]);
    l.selected = vec![1, 1];
    let r = run(&l);
    let t = table(&r);
    assert_eq!(t.features.len(), 1);
    assert_eq!(int(t, 0, "feature"), 2);
    assert_eq!(r.messages, ["Compare Repairs: 1 of 1 feature needs repairs, and the two rules differ on 0"]);
}

/// A layer held in a projected system is repaired in its own coordinates and measured on the
/// ground: the same areas as the layer in longitude/latitude, to within the conversion.
#[test]
fn a_projected_layer_is_measured_on_the_ground() {
    let lonlat = layer(GeometryType::Polygon, vec![Some(multi(&[&[A], &[B]]))]);
    let mut held = layer(GeometryType::Polygon, vec![Some(multi(&[&[A], &[B]]))]);
    held.ds.hold_in(Crs::Epsg(32633)).unwrap();
    assert_eq!(held.ds.coord_crs(), Crs::Epsg(32633));
    let (a, b) = (run(&lonlat), run(&held));
    let (a, b) = (table(&a), table(&b));
    for field in ["shells_m2", "even_odd_m2", "removed_m2"] {
        let (x, y) = (float(a, 0, field), float(b, 0, field));
        assert!((x - y).abs() <= x * 1e-6, "{field}: {x} in longitude/latitude, {y} in UTM");
    }
    assert_eq!(value(b, 0, "differ"), &Value::Bool(true));
}

/// Lines: only repeated vertices and short parts are repaired, the same under both rules, with
/// no area.
#[test]
fn lines_have_parts_and_no_area() {
    let line = Geometry::Line(vec![pts(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)])]);
    let r = run(&layer(GeometryType::Polyline, vec![Some(line)]));
    let t = table(&r);
    assert_eq!(text(t, 0, "shells_repairs"), "repeated_vertex");
    assert_eq!(value(t, 0, "differ"), &Value::Bool(false));
    assert_eq!(value(t, 0, "shells_m2"), &Value::Null);
    assert_eq!(value(t, 0, "shells_holes"), &Value::Null);
    assert_eq!(int(t, 0, "even_odd_parts"), 1);
}

/// Clean input says so, with an empty table; a table input is refused.
#[test]
fn clean_and_empty_input() {
    let r = run(&layer(GeometryType::Polygon, vec![Some(multi(&[&[A]])), Some(multi(&[&[BIG, INNER]]))]));
    assert!(table(&r).features.is_empty());
    assert_eq!(r.messages, ["Compare Repairs: no repairs needed in 2 features"]);
    let r = run(&layer(GeometryType::Polygon, vec![]));
    assert_eq!(r.messages, ["Compare Repairs: no features to compare"]);
    assert_eq!(table(&r).name, "input_compare");
    let e = rustgis_analysis::run("compareRepairs", &layer(GeometryType::None, vec![]), &json!({"input": "input"})).unwrap_err();
    assert_eq!(e, "Compare Repairs needs a layer with geometry: 'input' is a table");
    let r = rustgis_analysis::run("compareRepairs", &layer(GeometryType::Polygon, vec![]), &json!({"input": "input", "output": "Mine"}))
        .unwrap();
    assert_eq!(table(&r).name, "Mine");
}
