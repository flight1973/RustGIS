//! Tests of the AIXM reader: the Donlon 2025 sample (BSD-2-Clause, EUROCONTROL & FAA) and small
//! messages written here for each rule of the time slices, the arcs and hostile input.

use super::*;

const DONLON: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/donlon-aixm.xml"));

fn at(date: &str) -> Options {
    Options { at: crate::time::parse_text(date), ..Options::default() }
}

fn layer<'a>(out: &'a [(Dataset, ReadReport)], name: &str) -> &'a Dataset {
    match out.iter().find(|(d, _)| d.name == name) {
        Some((d, _)) => d,
        None => panic!("no layer {name}: {:?}", out.iter().map(|(d, _)| d.name.as_str()).collect::<Vec<_>>()),
    }
}

fn said(out: &[(Dataset, ReadReport)]) -> Vec<String> {
    out.iter().flat_map(|(_, r)| r.messages.iter().map(|m| m.text.clone())).collect()
}

fn value(ds: &Dataset, row: usize, field: &str) -> Value {
    let k = ds.fields.iter().position(|f| f.name == field).unwrap_or_else(|| panic!("no field {field} in {}", ds.name));
    ds.features[row].attrs[k].clone()
}

fn row_where(ds: &Dataset, field: &str, text: &str) -> usize {
    (0..ds.features.len()).find(|&i| value(ds, i, field) == Value::Text(text.into())).unwrap_or_else(|| panic!("no {field} = {text}"))
}

fn ring(g: &Geometry) -> &[Coord] {
    match g {
        Geometry::Polygon(p) => &p[0][0],
        other => panic!("not a polygon: {other:?}"),
    }
}

fn meters_between(a: Coord, b: Coord) -> f64 {
    rustgis_geom::wgs84_distance_m(a, b)
}

const NS: &str = r#"xmlns:message="http://www.aixm.aero/schema/5.1.1/message" xmlns:aixm="http://www.aixm.aero/schema/5.1.1" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#;

fn message(members: &str) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?>\n<message:AIXMBasicMessage {NS}>{members}</message:AIXMBasicMessage>").into_bytes()
}

fn period(begin: &str, end: Option<&str>) -> String {
    let end = match end {
        Some(e) => format!("<gml:endPosition>{e}</gml:endPosition>"),
        None => r#"<gml:endPosition indeterminatePosition="unknown"/>"#.to_string(),
    };
    format!(
        r#"<gml:validTime><gml:TimePeriod gml:id="t"><gml:beginPosition>{begin}</gml:beginPosition>{end}</gml:TimePeriod></gml:validTime>"#
    )
}

fn slice(kind: &str, interpretation: &str, seq: u32, corr: u32, valid: &str, body: &str) -> String {
    format!(
        r#"<aixm:timeSlice><aixm:{kind}TimeSlice gml:id="s">{valid}<aixm:interpretation>{interpretation}</aixm:interpretation><aixm:sequenceNumber>{seq}</aixm:sequenceNumber><aixm:correctionNumber>{corr}</aixm:correctionNumber>{body}</aixm:{kind}TimeSlice></aixm:timeSlice>"#
    )
}

fn feature(kind: &str, id: &str, slices: &[String]) -> String {
    format!(
        r#"<message:hasMember><aixm:{kind} gml:id="f"><gml:identifier codeSpace="urn:uuid:">{id}</gml:identifier>{}</aixm:{kind}></message:hasMember>"#,
        slices.concat()
    )
}

fn named_point(name: &str) -> String {
    format!(
        r#"<aixm:name>{name}</aixm:name><aixm:location><aixm:Point srsName="urn:ogc:def:crs:EPSG::4326" gml:id="g"><gml:pos>45.71 -35.17</gml:pos></aixm:Point></aixm:location>"#
    )
}

fn point(id: &str, slices: &[String]) -> String {
    feature("DesignatedPoint", id, slices)
}

fn baseline(seq: u32, corr: u32, begin: &str, end: Option<&str>, name: &str) -> String {
    slice("DesignatedPoint", "BASELINE", seq, corr, &period(begin, end), &named_point(name))
}

fn names(bytes: &[u8], date: &str) -> (Vec<Value>, Vec<String>) {
    let out = read("t.xml", bytes, &at(date)).unwrap_or_else(|e| panic!("{e}"));
    let ds = layer(&out, "t DesignatedPoint");
    ((0..ds.features.len()).map(|i| value(ds, i, "name")).collect(), said(&out))
}

// ---------------------------------------------------------------------------------------------
// The Donlon sample.
// ---------------------------------------------------------------------------------------------

#[test]
fn donlon_gives_a_layer_per_kind_and_shape_property() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let mut got: Vec<&str> = out.iter().map(|(d, _)| d.name.as_str()).collect();
    got.sort_unstable();
    assert_eq!(
        got,
        [
            "donlon-aixm AirportHeliport ARP",
            "donlon-aixm AirportHeliport aviationBoundary",
            "donlon-aixm Airspace",
            "donlon-aixm DesignatedPoint",
            "donlon-aixm Navaid",
            "donlon-aixm RouteSegment",
            "donlon-aixm Runway",
            "donlon-aixm VerticalStructure",
        ]
    );
    let dp = layer(&out, "donlon-aixm DesignatedPoint");
    assert_eq!(dp.geometry_type, GeometryType::Point);
    assert_eq!(value(dp, 0, "designator"), Value::Text("ABOLA".into()));
    assert_eq!(value(dp, 0, "identifier"), Value::Text("9fcb360c-6933-46d6-9c85-a337202efe70".into()));
    // Latitude first in the file, longitude first here.
    assert_eq!(dp.features[0].geometry, Some(Geometry::Point(Coord::new(-35.17, 45.71))));
    let arp = layer(&out, "donlon-aixm AirportHeliport ARP");
    assert_eq!(value(arp, 0, "designator"), Value::Text("EADD".into()));
    assert_eq!(arp.features[0].geometry, Some(Geometry::Point(Coord::new(-31.94944444, 52.37166667))));
    // The elevation written on the reference point, with its unit and datum.
    assert_eq!(value(arp, 0, "elevation"), Value::Int(30));
    assert_eq!(value(arp, 0, "elevation_uom"), Value::Text("M".into()));
    assert_eq!(value(arp, 0, "verticalDatum"), Value::Text("EGM_96".into()));
    assert_eq!(layer(&out, "donlon-aixm AirportHeliport aviationBoundary").geometry_type, GeometryType::Polygon);
    // A runway has no geometry of its own: a table, its references kept as identifiers.
    let rwy = layer(&out, "donlon-aixm Runway");
    assert_eq!(rwy.geometry_type, GeometryType::None);
    assert_eq!(value(rwy, 0, "nominalLength"), Value::Int(3200));
    assert_eq!(value(rwy, 0, "associatedAirportHeliport"), Value::Text("1b54b2d6-a5ff-4e57-94c2-f4047a381c64".into()));
    assert_eq!(value(rwy, 0, "associatedAirportHeliport_title"), Value::Text("EADD DONLON/INTL.".into()));
    assert!(!said(&out).iter().any(|m| m.contains("Runway") && m.contains("no geometry")), "{:?}", said(&out));
}

#[test]
fn the_bravo_arc_runs_clockwise_from_west_through_north_to_southeast() {
    // ASE_BRAVO_R: from its center west along a geodesic, an ArcByCenterPoint of 25 NM from
    // bearing -90 to 137 (clockwise, as the AIXM guidance reads a latitude-first system), and back.
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let asp = layer(&out, "donlon-aixm Airspace");
    let i = row_where(asp, "name", "BRAVO");
    let ring = ring(asp.features[i].geometry.as_ref().unwrap());
    let center = Coord::new(-36.166666666666664, 55.2333333333333);
    let radius = 25.0 * 1852.0;
    let on_arc: Vec<Coord> = ring.iter().copied().filter(|c| (meters_between(*c, center) - radius).abs() < 1.0).collect();
    assert!(on_arc.len() > 100, "{} points on the arc", on_arc.len());
    // It passes north of the center, which a counter-clockwise arc wouldn't.
    let north = on_arc.iter().map(|c| c.y).fold(f64::MIN, f64::max);
    assert!(north > 55.64, "northernmost {north}");
    // It joins the geodesics before and after it, within the file's rounding.
    let near = |p: Coord| ring.iter().any(|c| meters_between(*c, p) < 30.0);
    assert!(near(Coord::new(-36.89437338, 55.23116373)) && near(Coord::new(-35.67411607, 54.92816351)));
    // The limits as written.
    assert_eq!(value(asp, i, "upperLimit"), Value::Int(1525));
    assert_eq!(value(asp, i, "upperLimit_uom"), Value::Text("M".into()));
    assert_eq!(value(asp, i, "upperLimitReference"), Value::Text("SFC".into()));
    assert_eq!(value(asp, i, "lowerLimitReference"), Value::Text("SFC".into()));
}

#[test]
fn the_vaardnor_circle_stays_within_a_meter_of_its_radius() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let asp = layer(&out, "donlon-aixm Airspace");
    let i = row_where(asp, "name", "VAARDNOR");
    let ring = ring(asp.features[i].geometry.as_ref().unwrap());
    let center = Coord::new(-22.1, 52.36666667);
    assert!(ring.len() > 100);
    for c in ring {
        let d = meters_between(*c, center);
        assert!((d - 15.0 * 1852.0).abs() < 0.01, "{d}");
    }
}

#[test]
fn an_airspace_built_from_others_is_kept_without_a_shape_and_said() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let asp = layer(&out, "donlon-aixm Airspace");
    let i = row_where(asp, "name", "MAGNETO TMA");
    assert_eq!(asp.features[i].geometry, None);
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 airspace built from other airspace") && m.contains("contributorAirspace")), "{notes:?}");
}

#[test]
fn a_vertical_structure_gives_a_row_per_part() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let vs = layer(&out, "donlon-aixm VerticalStructure");
    assert_eq!(vs.features.len(), 5);
    let parts: Vec<Value> = (0..5).map(|i| value(vs, i, "part_designator")).collect();
    assert_eq!(parts[0], Value::Text("OBST-EA-0002-1".into()));
    assert_eq!(parts[4], Value::Text("OBST-EA-0002-5".into()));
    assert!((0..5).all(|i| value(vs, i, "name") == Value::Text("OBST-EA-0002".into())));
    assert_eq!(value(vs, 0, "part_verticalExtent"), Value::Int(150));
    assert_eq!(value(vs, 0, "elevation"), Value::Int(264));
}

#[test]
fn a_long_route_segment_follows_its_geodesic() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let rs = layer(&out, "donlon-aixm RouteSegment");
    let Some(Geometry::Line(parts)) = &rs.features[0].geometry else { panic!("not a line") };
    let line = &parts[0];
    let (a, b) = (Coord::new(-33.495, 44.39666667), Coord::new(-28.64166667, 47.13666667));
    assert_eq!((line[0], *line.last().unwrap()), (a, b));
    // Points every 10 km at most, each on the geodesic: the lengths along it add up.
    let whole = meters_between(a, b);
    assert!(line.len() as f64 >= (whole / GEODESIC_STEP_M).ceil());
    let along: f64 = line.windows(2).map(|w| meters_between(w[0], w[1])).sum();
    assert!((along - whole).abs() < 0.01, "{along} vs {whole}");
    assert_eq!(value(rs, 0, "upperLimit_uom"), Value::Text("FL".into()));
}

// ---------------------------------------------------------------------------------------------
// Time slices.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_highest_correction_holds_and_a_tempdelta_overlays_while_in_force() {
    let temp = slice(
        "DesignatedPoint",
        "TEMPDELTA",
        1,
        0,
        &period("2026-02-01T00:00:00Z", Some("2026-03-01T00:00:00Z")),
        "<aixm:name>TEMPORARY</aixm:name>",
    );
    let bytes = message(&point(
        "a",
        &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "FIRST"), baseline(1, 1, "2025-01-01T00:00:00Z", None, "CORRECTED"), temp],
    ));
    let (got, notes) = names(&bytes, "2026-01-15");
    assert_eq!(got, [Value::Text("CORRECTED".into())]);
    assert!(notes.is_empty(), "{notes:?}");
    let out = read("t.xml", &bytes, &at("2026-02-10")).unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    assert_eq!(value(ds, 0, "name"), Value::Text("TEMPORARY".into()));
    // What the change doesn't hold stays the BASELINE's.
    assert_eq!(ds.features[0].geometry, Some(Geometry::Point(Coord::new(-35.17, 45.71))));
    assert!(said(&out).iter().any(|m| m.contains("1 feature is shown with a temporary change (TEMPDELTA) in force on 2026-02-10")));
    // A period excludes its end.
    assert_eq!(names(&bytes, "2026-03-01").0, [Value::Text("CORRECTED".into())]);
}

#[test]
fn a_tempdelta_with_a_nil_property_removes_its_value() {
    let temp = slice(
        "DesignatedPoint",
        "TEMPDELTA",
        1,
        0,
        &period("2026-02-01T00:00:00Z", None),
        r#"<aixm:name xsi:nil="true" nilReason="unknown"/>"#,
    );
    let bytes = message(&point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "NAMED"), temp]));
    assert_eq!(names(&bytes, "2026-02-10").0, [Value::Null]);
}

#[test]
fn a_permanent_change_takes_over_from_its_date() {
    let bytes = message(&point(
        "a",
        &[
            baseline(1, 1, "2025-01-01T00:00:00Z", Some("2026-01-01T00:00:00Z"), "BEFORE"),
            baseline(2, 0, "2026-01-01T00:00:00Z", None, "AFTER"),
        ],
    ));
    assert_eq!(names(&bytes, "2025-06-01").0, [Value::Text("BEFORE".into())]);
    assert_eq!(names(&bytes, "2026-01-01").0, [Value::Text("AFTER".into())]);
}

#[test]
fn features_left_out_at_the_date_are_counted() {
    let lifetime = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>2020-01-01T00:00:00Z</gml:beginPosition><gml:endPosition>2025-12-31T00:00:00Z</gml:endPosition></gml:TimePeriod></aixm:featureLifetime>"#;
    let withdrawn = slice(
        "DesignatedPoint",
        "BASELINE",
        1,
        0,
        &period("2025-01-01T00:00:00Z", None),
        &(lifetime.to_string() + &named_point("WITHDRAWN")),
    );
    let canceled = slice("DesignatedPoint", "BASELINE", 1, 1, r#"<gml:validTime nilReason="inapplicable"/>"#, &named_point("CANCELED"));
    let delta_only = slice("DesignatedPoint", "PERMDELTA", 1, 0, &period("2025-01-01T00:00:00Z", None), "<aixm:name>DELTA</aixm:name>");
    let members = [
        point("kept", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]),
        point("later", &[baseline(1, 0, "2027-01-01T00:00:00Z", None, "LATER")]),
        point("withdrawn", &[withdrawn]),
        point("canceled", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "CANCELED"), canceled]),
        point("delta", &[delta_only]),
    ]
    .concat();
    let (got, notes) = names(&message(&members), "2026-01-15");
    assert_eq!(got, [Value::Text("KEPT".into())]);
    for want in [
        "1 feature not in force on 2026-01-15 00:00 UTC",
        "1 feature withdrawn by 2026-01-15 00:00 UTC",
        "1 feature whose time slices were all canceled",
        "1 feature given only as changes (PERMDELTA or TEMPDELTA)",
    ] {
        assert!(notes.iter().any(|m| m.starts_with(want)), "{want}: {notes:?}");
    }
}

#[test]
fn nothing_in_force_is_an_error_that_says_why() {
    let bytes = message(&point("later", &[baseline(1, 0, "2027-01-01T00:00:00Z", None, "LATER")]));
    let e = read("t.xml", &bytes, &at("2026-01-15")).unwrap_err().to_string();
    assert!(e.contains("not in force"), "{e}");
}

#[test]
fn a_snapshot_is_read_when_there_is_no_baseline_and_said() {
    let valid = r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition>2025-06-01T00:00:00Z</gml:timePosition></gml:TimeInstant></gml:validTime>"#;
    let snap = format!(
        r#"<aixm:timeSlice><aixm:DesignatedPointTimeSlice gml:id="s">{valid}<aixm:interpretation>SNAPSHOT</aixm:interpretation>{}</aixm:DesignatedPointTimeSlice></aixm:timeSlice>"#,
        named_point("SNAP")
    );
    let (got, notes) = names(&message(&point("s", &[snap])), "2026-01-15");
    assert_eq!(got, [Value::Text("SNAP".into())]);
    assert!(notes.iter().any(|m| m.contains("SNAPSHOT")), "{notes:?}");
}

#[test]
fn a_clean_message_says_nothing() {
    let bytes = message(&point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "CLEAN")]));
    let (got, notes) = names(&bytes, "2026-01-15");
    assert_eq!(got.len(), 1);
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_feature_split_across_members_is_put_together() {
    let temp = slice("DesignatedPoint", "TEMPDELTA", 1, 0, &period("2026-01-01T00:00:00Z", None), "<aixm:name>LATER MEMBER</aixm:name>");
    let members = point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "FIRST MEMBER")]) + &point("a", &[temp]);
    assert_eq!(names(&message(&members), "2026-01-15").0, [Value::Text("LATER MEMBER".into())]);
}

// ---------------------------------------------------------------------------------------------
// Geometry.
// ---------------------------------------------------------------------------------------------

/// An airspace whose outline is the BRAVO arc, in a latitude-first or longitude-first system.
fn arc_airspace(lat_first: bool) -> Vec<u8> {
    // In a longitude-first system angles run counter-clockwise from east.
    let (srs, start, end) =
        if lat_first { ("urn:ogc:def:crs:EPSG::4326", "-90.0", "137.0") } else { ("urn:ogc:def:crs:OGC:1.3:CRS84", "180.0", "-47.0") };
    let pos = |lat: &str, lon: &str| if lat_first { format!("{lat} {lon}") } else { format!("{lon} {lat}") };
    let (c, w, se) = (pos("55.23333333", "-36.16666667"), pos("55.23116373", "-36.89437338"), pos("54.92816351", "-35.67411607"));
    let body = format!(
        r#"<aixm:name>ARC</aixm:name><aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c"><aixm:operation>BASE</aixm:operation><aixm:operationSequence>1</aixm:operationSequence><aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v"><aixm:upperLimit uom="FL">95</aixm:upperLimit><aixm:upperLimitReference>STD</aixm:upperLimitReference><aixm:horizontalProjection><aixm:Surface srsName="{srs}" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember><aixm:Curve gml:id="k"><gml:segments><gml:GeodesicString><gml:posList>{c} {w}</gml:posList></gml:GeodesicString><gml:ArcByCenterPoint numArc="1"><gml:pos>{c}</gml:pos><gml:radius uom="[nmi_i]">25.0</gml:radius><gml:startAngle uom="deg">{start}</gml:startAngle><gml:endAngle uom="deg">{end}</gml:endAngle></gml:ArcByCenterPoint><gml:GeodesicString><gml:posList>{se} {c}</gml:posList></gml:GeodesicString></gml:segments></aixm:Curve></gml:curveMember></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume></aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#
    );
    message(&feature("Airspace", "arc", &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]))
}

#[test]
fn an_arc_in_a_longitude_first_system_is_the_same_arc() {
    let shape = |lat_first| {
        let out = read("t.xml", &arc_airspace(lat_first), &at("2026-01-15")).unwrap();
        layer(&out, "t Airspace").features[0].geometry.clone().unwrap()
    };
    let (a, b) = (shape(true), shape(false));
    let (a, b) = (ring(&a), ring(&b));
    assert_eq!(a.len(), b.len());
    for (p, q) in a.iter().zip(b) {
        assert!(meters_between(*p, *q) < 0.001, "{p:?} {q:?}");
    }
    let out = read("t.xml", &arc_airspace(true), &at("2026-01-15")).unwrap();
    assert_eq!(said(&out), ["t Airspace: 1 feature has arcs or circles, drawn as points along them within a meter"]);
    let ds = layer(&out, "t Airspace");
    assert_eq!(value(ds, 0, "upperLimit"), Value::Int(95));
    assert_eq!(value(ds, 0, "upperLimit_uom"), Value::Text("FL".into()));
}

#[test]
fn an_arc_through_three_points_passes_through_them() {
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    let mut notes = ShapeNotes::default();
    let (a, m, b) = (Coord::new(10.0, 50.0), Coord::new(10.1, 50.07), Coord::new(10.2, 50.0));
    let arc = shapes.arc_through(a, m, b, false, &mut notes);
    assert!(meters_between(arc[0], a) < 0.01 && meters_between(*arc.last().unwrap(), b) < 0.01);
    assert!(arc.iter().any(|c| meters_between(*c, m) < 5.0));
    // Bulging north, through the middle point, not around the other way.
    assert!(arc.iter().all(|c| c.y >= 49.999));
}

#[test]
fn arcs_past_the_point_allowance_are_drawn_coarser_and_said() {
    let mut shapes = Shapes::default();
    let mut notes = ShapeNotes::default();
    let n = shapes.pieces(50_000.0, 180.0, &mut notes);
    assert_eq!(n, 1);
    assert!(notes.coarse);
}

#[test]
fn a_radius_in_an_unknown_unit_or_past_the_limit_is_not_drawn() {
    for (radius, uom) in [("25.0", "furlong"), ("1e9", "m"), ("-5", "m"), ("NaN", "m")] {
        let bytes = String::from_utf8(arc_airspace(true))
            .unwrap()
            .replace(r#"<gml:radius uom="[nmi_i]">25.0"#, &format!(r#"<gml:radius uom="{uom}">{radius}"#));
        let out = read("t.xml", bytes.as_bytes(), &at("2026-01-15")).unwrap();
        assert_eq!(layer(&out, "t Airspace").features[0].geometry, None, "{radius} {uom}");
        assert!(said(&out).iter().any(|m| m.contains("couldn't be read")), "{:?}", said(&out));
    }
}

#[test]
fn geometry_in_a_projected_system_is_not_read_and_said() {
    let bytes = String::from_utf8(message(&point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "P")])))
        .unwrap()
        .replace("urn:ogc:def:crs:EPSG::4326", "urn:ogc:def:crs:EPSG::32633");
    let out = read("t.xml", bytes.as_bytes(), &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t DesignatedPoint").features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("EPSG::32633")), "{:?}", said(&out));
}

#[test]
fn union_volumes_are_drawn_as_parts_and_subtractions_are_not_drawn() {
    let volume = |op: &str, seq: u32, lat: f64| {
        let ring = format!("{lat} 10 {lat} 11 {} 11 {} 10 {lat} 10", lat + 1.0, lat + 1.0);
        format!(
            r#"<aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c"><aixm:operation>{op}</aixm:operation><aixm:operationSequence>{seq}</aixm:operationSequence><aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v"><aixm:horizontalProjection><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>{ring}</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume></aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#
        )
    };
    let airspace = |id: &str, second: &str| {
        let body = format!("<aixm:name>{id}</aixm:name>{}{}", volume("BASE", 1, 50.0), volume(second, 2, 52.0));
        feature("Airspace", id, &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])
    };
    let out = read("t.xml", &message(&(airspace("U", "UNION") + &airspace("S", "SUBTR"))), &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t Airspace");
    let Some(Geometry::Polygon(parts)) = &ds.features[row_where(ds, "name", "U")].geometry else { panic!() };
    assert_eq!(parts.len(), 2);
    assert_eq!(ds.features[row_where(ds, "name", "S")].geometry, None);
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 airspace is drawn as the parts of its BASE and UNION volumes")), "{notes:?}");
    assert!(
        notes.iter().any(|m| m.contains("1 airspace built from other airspace or by subtraction or intersection") && m.contains("SUBTR"))
    );
}

// ---------------------------------------------------------------------------------------------
// The file.
// ---------------------------------------------------------------------------------------------

#[test]
fn aixm_is_told_by_its_root() {
    assert!(looks_like_aixm(DONLON));
    assert!(looks_like_aixm(&message("")));
    let v52 = br#"<message:AIXMBasicMessage xmlns:message="http://www.aixm.aero/schema/5.2/message"/>"#;
    assert!(looks_like_aixm(v52));
    assert!(!looks_like_aixm(br#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"/>"#));
    assert!(!looks_like_aixm(br#"<wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0"/>"#));
    assert!(!looks_like_aixm(br#"<AIXMBasicMessage xmlns="http://example.com/other"/>"#));
}

#[test]
fn a_file_that_isnt_an_aixm_message_or_has_no_members_is_refused() {
    let e = read("t.xml", br#"<gml:FeatureCollection xmlns:gml="http://www.opengis.net/gml/3.2"/>"#, &at("2026-01-15")).unwrap_err();
    assert!(e.to_string().contains("not an AIXM message"), "{e}");
    let e = read("t.xml", &message(""), &at("2026-01-15")).unwrap_err();
    assert!(e.to_string().contains("no features"), "{e}");
}

#[test]
fn a_file_cut_short_keeps_the_members_before_and_says_so() {
    let whole = message(
        &(point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "A")])
            + &point("b", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "B")])),
    );
    let cut = &whole[..whole.len() - 120];
    let out = read("t.xml", cut, &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t DesignatedPoint").features.len(), 1);
    assert!(said(&out).iter().any(|m| m.contains("stops being well-formed XML")), "{:?}", said(&out));
}

#[test]
fn entities_and_doctypes_are_not_expanded() {
    let doc = format!(
        "<?xml version=\"1.0\"?><!DOCTYPE x [<!ENTITY a \"AAAAAAAAAA\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">]><message:AIXMBasicMessage {NS}>{}{}</message:AIXMBasicMessage>",
        point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "&b;")]),
        point("b", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "PLAIN")])
    );
    let out = read("t.xml", doc.as_bytes(), &at("2026-01-15")).unwrap();
    assert_eq!(names(doc.as_bytes(), "2026-01-15").0, [Value::Text("PLAIN".into())]);
    assert!(said(&out).iter().any(|m| m.contains("1 member of the message couldn't be read")), "{:?}", said(&out));
}

#[test]
fn junk_never_panics() {
    let base = String::from_utf8(DONLON.to_vec()).unwrap();
    let mut seed = 7u64;
    for _ in 0..200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let at_byte = (seed >> 33) as usize % base.len();
        let mut bytes = base.as_bytes().to_vec();
        bytes.truncate(at_byte);
        let _ = read("t.xml", &bytes, &at("2026-01-15"));
        let mut swapped = base.as_bytes().to_vec();
        if let Some(b) = swapped.get_mut(at_byte) {
            *b = b"<>/\"=0-.9 x"[(seed as usize) % 11];
        }
        let _ = read("t.xml", &swapped, &at("2026-01-15"));
    }
}

#[test]
fn an_effective_date_is_read_from_text() {
    assert_eq!(Options::at_text("2026-01-15").unwrap(), at("2026-01-15"));
    assert!(Options::at_text("next tuesday").unwrap_err().contains("isn't a date"));
}

// ---------------------------------------------------------------------------------------------
// Bounds and counts from review.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_structure_of_many_parts_and_values_stays_within_the_files_allowance() {
    let values: String = (0..2000).map(|i| format!("<aixm:p{i}>x</aixm:p{i}>")).collect();
    let parts = "<aixm:part><aixm:VerticalStructurePart gml:id=\"q\"/></aixm:part>".repeat(10_000);
    let body = format!("<aixm:name>MANY</aixm:name>{values}{parts}");
    let bytes = message(&feature(
        "VerticalStructure",
        "v",
        &[slice("VerticalStructure", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)],
    ));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let rows: usize = out.iter().map(|(d, _)| d.features.len()).sum();
    assert!(rows < 10_000, "{rows} rows");
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("past the memory the file's size allows")), "{notes:?}");
    assert!(notes.iter().any(|m| m.contains(&format!("past the first {MAX_COLUMNS} columns"))), "{notes:?}");
}

#[test]
fn kinds_past_the_layer_limit_are_counted_once_a_feature() {
    let members: String = (0..70)
        .map(|k| {
            let kind = format!("Kind{k}");
            feature(&kind, &format!("id{k}"), &[slice(&kind, "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &named_point("P"))])
        })
        .collect();
    let out = read("t.xml", &message(&members), &at("2026-01-15")).unwrap();
    assert_eq!(out.len(), MAX_LAYERS);
    let notes = said(&out);
    assert!(notes.iter().any(|m| m == &format!("6 features of kinds past the first {MAX_LAYERS} left out")), "{notes:?}");
}

#[test]
fn members_with_no_aixm_feature_are_counted() {
    let foreign = r#"<message:hasMember><ext:Thing xmlns:ext="http://example.org/ext"/></message:hasMember>"#;
    let only_link = r#"<message:hasMember xlink:href="urn:uuid:x"></message:hasMember>"#;
    let empty = "<message:hasMember></message:hasMember>";
    let members = point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "A")]) + foreign + only_link + empty;
    let notes = names(&message(&members), "2026-01-15").1;
    for want in [
        "1 element of members that aren't AIXM features (another schema's) not read",
        "1 member of the message only point elsewhere (xlink:href) and weren't followed",
        "1 member of the message hold nothing",
    ] {
        assert!(notes.iter().any(|m| m == want), "{want}: {notes:?}");
    }
}

#[test]
fn an_arc_from_0_to_360_is_a_full_circle() {
    let bytes = String::from_utf8(arc_airspace(true)).unwrap().replace(
        r#"<gml:startAngle uom="deg">-90.0</gml:startAngle><gml:endAngle uom="deg">137.0</gml:endAngle>"#,
        r#"<gml:startAngle uom="deg">0</gml:startAngle><gml:endAngle uom="deg">360</gml:endAngle>"#,
    );
    let out = read("t.xml", bytes.as_bytes(), &at("2026-01-15")).unwrap();
    let ring = ring(layer(&out, "t Airspace").features[0].geometry.as_ref().unwrap());
    let center = Coord::new(-36.16666667, 55.23333333);
    let south = ring.iter().filter(|c| (meters_between(**c, center) - 46_300.0).abs() < 1.0).map(|c| c.y).fold(f64::MAX, f64::min);
    assert!(south < 54.82, "the circle reaches south of its center: {south}");
}

#[test]
fn a_ring_across_the_antimeridian_stays_continuous() {
    let body = r#"<aixm:name>PACIFIC</aixm:name><aixm:location><aixm:Point srsName="urn:ogc:def:crs:EPSG::4326" gml:id="g"><gml:pos>0 179.5</gml:pos></aixm:Point></aixm:location><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>0 179 0 -179 1 -179 1 179 0 179</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#;
    let bytes = message(&feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t WorkArea extent");
    let ring = ring(ds.features[0].geometry.as_ref().unwrap());
    assert!(ring.windows(2).all(|w| (w[0].x - w[1].x).abs() < 3.0), "{ring:?}");
}

// ---------------------------------------------------------------------------------------------
// From /code-review.
// ---------------------------------------------------------------------------------------------

/// An airspace of one volume, its component written as `component` around the volume.
fn one_volume(component: &str) -> Vec<u8> {
    let volume = r#"<aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v"><aixm:horizontalProjection><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>50 10 50 11 51 11 51 10 50 10</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume>"#;
    let body = format!("<aixm:name>ONE</aixm:name>{}", component.replace("VOLUME", volume));
    message(&feature("Airspace", "one", &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]))
}

#[test]
fn a_lone_volume_with_no_operation_is_drawn() {
    let bytes = one_volume(
        r#"<aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c">VOLUME</aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#,
    );
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    assert!(layer(&out, "t Airspace").features[0].geometry.is_some());
    assert!(said(&out).is_empty(), "{:?}", said(&out));
}

#[test]
fn a_volume_given_by_reference_keeps_the_airspace_from_being_drawn_partly() {
    let bytes = one_volume(
        r#"<aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c"><aixm:operation>BASE</aixm:operation><aixm:operationSequence>1</aixm:operationSequence>VOLUME</aixm:AirspaceGeometryComponent></aixm:geometryComponent><aixm:geometryComponent xlink:href="urn:uuid:elsewhere"/>"#,
    );
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t Airspace").features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("a volume given by reference")), "{:?}", said(&out));
}

#[test]
fn a_border_given_by_reference_is_said_once() {
    let body = r#"<aixm:name>REF</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember xlink:href="urn:uuid:border"/></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#;
    let bytes = message(&feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]));
    let notes = said(&read("t.xml", &bytes, &at("2026-01-15")).unwrap());
    assert!(notes.iter().any(|m| m.contains("1 border is given by reference")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("ring or surface patch was left out")), "{notes:?}");
}

#[test]
fn a_part_with_no_shape_is_counted() {
    let located = r#"<aixm:part><aixm:VerticalStructurePart gml:id="a"><aixm:designator>A</aixm:designator><aixm:horizontalProjection_location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>47 -27</gml:pos></aixm:ElevatedPoint></aixm:horizontalProjection_location></aixm:VerticalStructurePart></aixm:part>"#;
    let bare = r#"<aixm:part><aixm:VerticalStructurePart gml:id="b"><aixm:designator>B</aixm:designator></aixm:VerticalStructurePart></aixm:part>"#;
    let body = format!("<aixm:name>VS</aixm:name>{located}{bare}");
    let bytes = message(&feature(
        "VerticalStructure",
        "v",
        &[slice("VerticalStructure", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)],
    ));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t VerticalStructure").features.len(), 2);
    assert!(said(&out).iter().any(|m| m.contains("1 feature has no geometry")), "{:?}", said(&out));
}

#[test]
fn a_layers_z_note_comes_first() {
    let bytes = String::from_utf8(message(&point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "Z")])))
        .unwrap()
        .replace("<gml:pos>45.71 -35.17</gml:pos>", "<gml:pos>45.71 -35.17 12</gml:pos>")
        + "";
    let later = point("late", &[baseline(1, 0, "2030-01-01T00:00:00Z", None, "LATER")]);
    let bytes = bytes.replacen("</message:AIXMBasicMessage>", &format!("{later}</message:AIXMBasicMessage>"), 1);
    let out = read("t.xml", bytes.as_bytes(), &at("2026-01-15")).unwrap();
    let first = &out[0].1.messages;
    assert!(first.first().is_some_and(|m| m.text.contains("Z or M")), "{:?}", first.iter().map(|m| &m.text).collect::<Vec<_>>());
    assert!(first.iter().any(|m| m.text.contains("not in force")));
}

// ---------------------------------------------------------------------------------------------
// Every value kept: nested records as path columns.
// ---------------------------------------------------------------------------------------------

#[test]
fn records_are_kept_as_path_columns_numbered_alike_on_every_row() {
    let authority = |t: &str, org: &str| {
        format!(
            r#"<aixm:authority><aixm:AuthorityForNavaidEquipment gml:id="a"><aixm:type>{t}</aixm:type><aixm:theOrganisationAuthority xlink:href="urn:uuid:{org}" xlink:title="ORG {org}" xlink:type="simple"/></aixm:AuthorityForNavaidEquipment></aixm:authority>"#
        )
    };
    let note = r#"<aixm:annotation><aixm:Note gml:id="n"><aixm:purpose>REMARK</aixm:purpose><aixm:translatedNote><aixm:LinguisticNote gml:id="l"><aixm:note lang="ENG">Coverage: 250 KM</aixm:note></aixm:LinguisticNote></aixm:translatedNote></aixm:Note></aixm:annotation>"#;
    let body =
        |name: &str, auths: &str| format!("{}{auths}{note}<aixm:class xsi:nil=\"true\" nilReason=\"inapplicable\"/>", named_point(name));
    let two = slice(
        "DesignatedPoint",
        "BASELINE",
        1,
        0,
        &period("2025-01-01T00:00:00Z", None),
        &body("TWO", &(authority("OWN", "o1") + &authority("OPERATE", "o2"))),
    );
    let one = slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body("ONE", &authority("OWN", "o3")));
    let bytes = message(&(point("two", &[two]) + &point("one", &[one])));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    let (two, one) = (row_where(ds, "name", "TWO"), row_where(ds, "name", "ONE"));
    let text = |row: usize, field: &str| value(ds, row, field);
    // Repeating on one row, so numbered on both.
    assert_eq!(text(two, "authority_1_type"), Value::Text("OWN".into()));
    assert_eq!(text(two, "authority_2_type"), Value::Text("OPERATE".into()));
    assert_eq!(text(two, "authority_2_theOrganisationAuthority"), Value::Text("o2".into()));
    assert_eq!(text(two, "authority_2_theOrganisationAuthority_title"), Value::Text("ORG o2".into()));
    assert_eq!(text(one, "authority_1_type"), Value::Text("OWN".into()));
    assert_eq!(text(one, "authority_2_type"), Value::Null);
    // Never repeating: no numbers. A note's language and a missing value's reason are kept.
    assert_eq!(text(one, "annotation_translatedNote_note"), Value::Text("Coverage: 250 KM".into()));
    assert_eq!(text(one, "annotation_translatedNote_note_lang"), Value::Text("ENG".into()));
    assert_eq!(text(one, "annotation_purpose"), Value::Text("REMARK".into()));
    assert_eq!(text(one, "class"), Value::Null);
    assert_eq!(text(one, "class_nilReason"), Value::Text("inapplicable".into()));
    // Nothing is left out, so nothing is said.
    assert!(said(&out).is_empty(), "{:?}", said(&out));
}

#[test]
fn donlon_keeps_every_value_of_its_features() {
    let out = read("donlon-aixm.xml", DONLON, &at("2026-01-15")).unwrap();
    let asp = layer(&out, "donlon-aixm Airspace");
    let i = row_where(asp, "name", "BRAVO");
    let has = |want: &str| {
        asp.fields.iter().enumerate().any(|(k, f)| f.name.starts_with("activation") && asp.features[i].attrs[k] == Value::Text(want.into()))
    };
    assert!(has("Activity periods to be published by NOTAM"));
    assert!(has("SHOOTING") && has("AVBL_FOR_ACTIVATION"));
    let op = asp.fields.iter().position(|f| f.name.starts_with("geometryComponent") && f.name.ends_with("operation")).unwrap();
    assert_eq!(asp.features[i].attrs[op], Value::Text("BASE".into()));
    assert_eq!(value(asp, i, "class_nilReason"), Value::Text("inapplicable".into()));
    // The route segment's start and end points, by reference.
    let rs = layer(&out, "donlon-aixm RouteSegment");
    assert!(
        rs.fields.iter().any(|f| f.name.starts_with("start_") && f.name.ends_with("pointChoice_fixDesignatedPoint")),
        "{:?}",
        rs.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    // Nothing the features hold is said as not read.
    assert!(!said(&out).iter().any(|m| m.contains("not read as attributes") || m.contains("columns")), "{:?}", said(&out));
}

#[test]
fn a_light_system_gives_a_row_per_light_with_its_own_values() {
    let light = |color: &str, lat: &str| {
        format!(
            r#"<aixm:element><aixm:GroundLightingElement gml:id="e"><aixm:type>LIGHT</aixm:type><aixm:colour>{color}</aixm:colour><aixm:location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>{lat} -32.0</gml:pos><aixm:elevation uom="M">29</aixm:elevation></aixm:ElevatedPoint></aixm:location></aixm:GroundLightingElement></aixm:element>"#
        )
    };
    let body = format!(
        "<aixm:emergencyLighting>YES</aixm:emergencyLighting>{}{}{}",
        light("WHITE", "52.1"),
        light("RED", "52.2"),
        light("GREEN", "52.3")
    );
    let bytes = message(&feature(
        "RunwayDirectionLightSystem",
        "l",
        &[slice("RunwayDirectionLightSystem", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)],
    ));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t RunwayDirectionLightSystem");
    assert_eq!(ds.features.len(), 3);
    let colors: Vec<Value> = (0..3).map(|i| value(ds, i, "element_colour")).collect();
    assert_eq!(colors, ["WHITE", "RED", "GREEN"].map(|c| Value::Text(c.into())));
    assert!((0..3).all(|i| value(ds, i, "emergencyLighting") == Value::Text("YES".into())));
    assert_eq!(value(ds, 1, "element_location_elevation"), Value::Int(29));
    assert_eq!(ds.features[2].geometry, Some(Geometry::Point(Coord::new(-32.0, 52.3))));
    assert!(said(&out).is_empty(), "{:?}", said(&out));
}

#[test]
fn objects_nested_past_the_limit_are_counted_not_followed() {
    let open: String = (0..150).map(|i| format!("<aixm:O{i}>")).collect();
    let close: String = (0..150).rev().map(|i| format!("</aixm:O{i}>")).collect();
    let body = format!("{}<aixm:deep>{open}<aixm:leaf>x</aixm:leaf>{close}</aixm:deep>", named_point("DEEP"));
    let out = read(
        "t.xml",
        &message(&point("d", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert_eq!(layer(&out, "t DesignatedPoint").features.len(), 1);
    assert!(said(&out).iter().any(|m| m.contains("nested deeper")), "{:?}", said(&out));
}

#[test]
fn a_long_value_copied_onto_many_rows_stays_within_the_files_allowance() {
    let long = "N".repeat(1 << 20);
    let light = r#"<aixm:element><aixm:GroundLightingElement gml:id="e"><aixm:location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>52 -32</gml:pos></aixm:ElevatedPoint></aixm:location></aixm:GroundLightingElement></aixm:element>"#;
    let body = format!("<aixm:remark>{long}</aixm:remark>{}", light.repeat(1000));
    let bytes = message(&feature(
        "RunwayDirectionLightSystem",
        "l",
        &[slice("RunwayDirectionLightSystem", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)],
    ));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let rows = layer(&out, "t RunwayDirectionLightSystem").features.len();
    assert!(rows < 1000, "{rows} rows of a megabyte each");
    assert!(said(&out).iter().any(|m| m.contains("past the memory the file's size allows")), "{:?}", said(&out));
}

#[test]
fn a_border_given_by_reference_is_followed_between_the_points_either_side() {
    let border = r#"<aixm:name>NORTH-SOUTH</aixm:name><aixm:border><aixm:Curve gml:id="B1" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:LineStringSegment><gml:posList>50 9 50 10.5 50 12</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#;
    let geo = feature("GeoBorder", "border-1", &[slice("GeoBorder", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), border)]);
    let ring_xml = r#"<gml:Ring><gml:curveMember xlink:href="urn:uuid:border-1"/><gml:curveMember><aixm:Curve gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>50 11 50.5 11 50.5 10 50 10</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></gml:curveMember></gml:Ring>"#;
    let body = format!(
        r#"<aixm:name>FOLLOWS</aixm:name><aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="g"><aixm:operation>BASE</aixm:operation><aixm:operationSequence>1</aixm:operationSequence><aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v"><aixm:horizontalProjection><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior>{ring_xml}</gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume></aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#
    );
    // The airspace comes first in the file: the border it follows is read first all the same.
    let asp = feature("Airspace", "a", &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&(asp + &geo)), &at("2026-01-15")).unwrap();
    let ring = ring(layer(&out, "t Airspace").features[0].geometry.as_ref().unwrap());
    // From the point before (10, 50) along the border through its vertex at 10.5 to the next (11, 50).
    assert!(ring.iter().any(|c| (c.x - 10.5).abs() < 1e-9 && (c.y - 50.0).abs() < 1e-9), "{ring:?}");
    assert!(!ring.iter().any(|c| c.x < 9.99 || c.x > 11.01), "{ring:?}");
    assert_eq!(ring.first(), ring.last());
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 border given by reference is drawn along the GeoBorder")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("can't be followed") || m.contains("more than")), "{notes:?}");
}

#[test]
fn a_closed_border_is_cut_the_shorter_way_round() {
    let square = [Coord::new(0.0, 0.0), Coord::new(1.0, 0.0), Coord::new(1.0, 1.0), Coord::new(0.0, 1.0), Coord::new(0.0, 0.0)];
    let (part, off) = cut(&square, Coord::new(0.5, 0.0), Coord::new(1.0, 0.5)).unwrap();
    assert_eq!(part, [Coord::new(0.5, 0.0), Coord::new(1.0, 0.0), Coord::new(1.0, 0.5)]);
    assert!(off < 1e-6);
    let (part, _) = cut(&square, Coord::new(1.0, 0.5), Coord::new(0.5, 0.0)).unwrap();
    assert_eq!(part, [Coord::new(1.0, 0.5), Coord::new(1.0, 0.0), Coord::new(0.5, 0.0)]);
}

#[test]
fn an_faa_subscriber_file_is_read_and_its_references_name_the_features() {
    let file = r##"<?xml version="1.0" encoding="UTF-8"?>
<faa:SubscriberFile gml:id="Subscriber-0000000" AIXMVersion="AIXM5.1" SubscriberFileType="NAV" xmlns:faa="http://www.faa.gov/aixm5.1" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:aixm="http://www.aixm.aero/schema/5.1" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <faa:Member>
    <aixm:Navaid gml:id="NAVAID_0000001">
      <aixm:timeSlice>
        <aixm:NavaidTimeSlice gml:id="NAVAID_TIME_SLICE_0000001">
          <gml:validTime><gml:TimePeriod gml:id="p1"><gml:beginPosition>2026-10-29T00:00:00.000-04:00</gml:beginPosition><gml:endPosition indeterminatePosition="unknown"/></gml:TimePeriod></gml:validTime>
          <aixm:interpretation>BASELINE</aixm:interpretation>
          <aixm:designator>ABC</aixm:designator>
          <aixm:location><aixm:ElevatedPoint gml:id="e1" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>32.9 -97.0</gml:pos></aixm:ElevatedPoint></aixm:location>
          <aixm:navaidEquipment><aixm:NavaidComponent gml:id="c1"><aixm:theNavaidEquipment xlink:href="#/faa:SubscriberFile/faa:Member/aixm:TACAN%5B@gml:id='TACAN_0000001'%5D"/></aixm:NavaidComponent></aixm:navaidEquipment>
        </aixm:NavaidTimeSlice>
      </aixm:timeSlice>
    </aixm:Navaid>
  </faa:Member>
</faa:SubscriberFile>
"##;
    assert!(looks_like_aixm(file.as_bytes()));
    let out = read("NAV_AIXM.xml", file.as_bytes(), &at("2026-11-23")).unwrap();
    let ds = layer(&out, "NAV_AIXM Navaid");
    assert_eq!(value(ds, 0, "identifier"), Value::Text("NAVAID_0000001".into()));
    assert_eq!(value(ds, 0, "designator"), Value::Text("ABC".into()));
    assert_eq!(value(ds, 0, "navaidEquipment_theNavaidEquipment"), Value::Text("TACAN_0000001".into()));
    assert_eq!(ds.features[0].geometry, Some(Geometry::Point(Coord::new(-97.0, 32.9))));
    // A subscriber file not of AIXM 5 isn't taken for one.
    assert!(!looks_like_aixm(file.replace("AIXM5.1\"", "AIXM4.5\"").as_bytes()));
}

#[test]
fn many_references_to_a_long_border_stay_within_the_files_allowance() {
    let line: String = (0..5000).map(|i| format!("50 {} ", 9.0 + f64::from(i) * 0.0005)).collect();
    let border = format!(
        r#"<aixm:name>LONG</aixm:name><aixm:border><aixm:Curve gml:id="B1" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:LineStringSegment><gml:posList>{line}</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#
    );
    let geo = feature("GeoBorder", "border-1", &[slice("GeoBorder", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &border)]);
    // Two borders named in turn: a repeated one is followed once, so each of these is a cut.
    let geo2 = geo.replace("border-1", "border-2");
    let geo = geo + &geo2;
    let refs = r#"<gml:curveMember xlink:href="urn:uuid:border-1"/><gml:curveMember xlink:href="urn:uuid:border-2"/>"#.repeat(10_000);
    let ring_xml = format!(
        r#"<gml:Ring>{refs}<gml:curveMember><aixm:Curve gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>50 11 50.5 11 50.5 10 50 10</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></gml:curveMember></gml:Ring>"#
    );
    let body = format!(
        r#"<aixm:name>MANY</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior>{ring_xml}</gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
    );
    let area = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&(area + &geo)), &at("2026-01-15")).unwrap();
    // 20,000 cuts of a 5,000-point border would be 100 million steps; the allowance stops them.
    assert!(said(&out).iter().any(|m| m.contains("would pass the work the file's size allows")), "{:?}", said(&out));
}

#[test]
fn faa_route_segments_are_drawn_between_their_points_in_nad83() {
    let point = |id: &str, lat: f64, lon: f64| {
        format!(
            r#"<faa:Member><aixm:DesignatedPoint gml:id="{id}"><aixm:timeSlice><aixm:DesignatedPointTimeSlice gml:id="t{id}"><gml:validTime><gml:TimePeriod gml:id="p{id}"><gml:beginPosition>2026-10-29T00:00:00.000-04:00</gml:beginPosition><gml:endPosition/></gml:TimePeriod></gml:validTime><aixm:interpretation>BASELINE</aixm:interpretation><aixm:designator>{id}</aixm:designator><aixm:location><aixm:Point gml:id="g{id}" srsName="urn:ogc:def:crs:OGC:1.3:CRS83"><gml:pos>{lon} {lat}</gml:pos></aixm:Point></aixm:location></aixm:DesignatedPointTimeSlice></aixm:timeSlice></aixm:DesignatedPoint></faa:Member>"#
        )
    };
    let xpath = |id: &str| format!("#/faa:SubscriberFile/faa:Member/aixm:DesignatedPoint%5B@gml:id='{id}'%5D");
    let segment = format!(
        r#"<faa:Member><aixm:RouteSegment gml:id="RS1"><aixm:timeSlice><aixm:RouteSegmentTimeSlice gml:id="tRS1"><gml:validTime><gml:TimePeriod gml:id="pRS1"><gml:beginPosition>2026-10-29T00:00:00.000-04:00</gml:beginPosition><gml:endPosition indeterminatePosition="unknown"/></gml:TimePeriod></gml:validTime><aixm:interpretation>BASELINE</aixm:interpretation><aixm:start><aixm:EnRouteSegmentPoint gml:id="s1"><aixm:pointChoice_fixDesignatedPoint xlink:href="{}"/></aixm:EnRouteSegmentPoint></aixm:start><aixm:end><aixm:EnRouteSegmentPoint gml:id="e1"><aixm:pointChoice_fixDesignatedPoint xlink:href="{}"/></aixm:EnRouteSegmentPoint></aixm:end></aixm:RouteSegmentTimeSlice></aixm:timeSlice></aixm:RouteSegment></faa:Member>"#,
        xpath("A"),
        xpath("B")
    );
    // The segment comes before the points it names.
    let file = format!(
        r#"<faa:SubscriberFile gml:id="S" AIXMVersion="AIXM5.1" xmlns:faa="http://www.faa.gov/aixm5.1" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:aixm="http://www.aixm.aero/schema/5.1" xmlns:xlink="http://www.w3.org/1999/xlink">{segment}{}{}</faa:SubscriberFile>"#,
        point("A", 32.0, -97.0),
        point("B", 33.0, -96.0)
    );
    let out = read("AWY_AIXM.xml", file.as_bytes(), &at("2026-11-23")).unwrap();
    let rs = layer(&out, "AWY_AIXM RouteSegment");
    let Some(Geometry::Line(parts)) = &rs.features[0].geometry else { panic!("{:?}", rs.features[0].geometry) };
    assert_eq!(parts[0].first(), Some(&Coord::new(-97.0, 32.0)));
    assert_eq!(parts[0].last(), Some(&Coord::new(-96.0, 33.0)));
    assert!(parts[0].len() > 10, "drawn along the geodesic");
    let notes = said(&out);
    for want in [
        "1 route segment with no curve is drawn between the points its start and end name",
        "2 geometries in a geographic system other than WGS 84",
        "2 time positions left empty (not said to be unknown) read as open",
    ] {
        assert!(notes.iter().any(|m| m.contains(want)), "{want}: {notes:?}");
    }
}

const DFS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dfs-airspace-aixm.xml"));

#[test]
fn dfs_airspace_follows_the_state_border_it_names() {
    let out = read("dfs-airspace-aixm.xml", DFS, &at("2026-11-23")).unwrap();
    let asp = layer(&out, "dfs-airspace-aixm Airspace");
    let i = row_where(asp, "name", "MASKIRCHEN B AREA");
    let ring = ring(asp.features[i].geometry.as_ref().unwrap());
    // From the written point at 51.0875 N 6.005 E, along the Dutch border, back to 50.92 N 6.06 E:
    // the stretch of border between them, not all of it (which runs to the North Sea).
    assert!(ring.len() > 10, "{ring:?}");
    assert!(ring.iter().all(|c| (5.8..6.3).contains(&c.x) && (50.85..51.15).contains(&c.y)), "{ring:?}");
    assert_eq!(ring.first(), ring.last());
    assert_eq!(value(asp, i, "geometryComponent_theAirspaceVolume_lowerLimit"), Value::Int(95));
    assert_eq!(value(asp, i, "lowerLimit_uom"), Value::Text("FL".into()));
    // BERLIN X is drawn from its arcs.
    assert!(asp.features[row_where(asp, "name", "BERLIN X")].geometry.is_some());
    let notes = said(&out);
    for want in ["1 border given by reference is drawn along the GeoBorder", "1 feature has arcs or circles", "SNAPSHOT"] {
        assert!(notes.iter().any(|m| m.contains(want)), "{want}: {notes:?}");
    }
    assert!(!notes.iter().any(|m| m.contains("more than 50 m")), "{notes:?}");
}

#[test]
fn a_chain_of_borders_meets_where_they_join_and_a_repeated_one_is_followed_once() {
    let geo = |id: &str, pos: &str| {
        let body = format!(
            r#"<aixm:name>{id}</aixm:name><aixm:border><aixm:Curve gml:id="c{id}" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:LineStringSegment><gml:posList>{pos}</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#
        );
        feature("GeoBorder", id, &[slice("GeoBorder", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])
    };
    // A runs west to east along 50 N beyond the corner at 10 E; B runs north from that corner.
    let borders = geo("south", "50 8.5 50 9 50 9.5 50 10 50 10.5") + &geo("east", "50 10 50.5 10 51 10");
    let ring_xml = r#"<gml:Ring><gml:curveMember xlink:href="urn:uuid:south"/><gml:curveMember xlink:href="urn:uuid:south"/><gml:curveMember xlink:href="urn:uuid:east"/><gml:curveMember><aixm:Curve gml:id="w"><gml:segments><gml:LineStringSegment><gml:posList>51 10 51 9 50 9</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></gml:curveMember></gml:Ring>"#;
    let body = format!(
        r#"<aixm:name>CORNER</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior>{ring_xml}</gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
    );
    let area = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&(area + &borders)), &at("2026-01-15")).unwrap();
    let ring = ring(layer(&out, "t WorkArea").features[0].geometry.as_ref().unwrap());
    // The square from 9 to 10 E, 50 to 51 N: south border from 9 to the corner, east border up.
    assert!(ring.iter().all(|c| (8.999..=10.001).contains(&c.x) && (49.999..=51.001).contains(&c.y)), "{ring:?}");
    let a: f64 = ring.windows(2).map(|w| (w[1].x - w[0].x) * (w[1].y + w[0].y) / 2.0).sum::<f64>().abs();
    assert!((a - 1.0).abs() < 1e-6, "area {a}: {ring:?}");
    let notes = said(&out);
    assert!(
        notes.iter().any(|m| m.contains("2 borders given by reference are drawn along the GeoBorders they name, between")),
        "{notes:?}"
    );
    assert!(!notes.iter().any(|m| m.contains("drawn whole")), "{notes:?}");
}

#[test]
fn a_ring_of_two_borders_follows_each_whole() {
    let geo = |id: &str, pos: &str| {
        let body = format!(
            r#"<aixm:name>{id}</aixm:name><aixm:border><aixm:Curve gml:id="c{id}" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:LineStringSegment><gml:posList>{pos}</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#
        );
        feature("GeoBorder", id, &[slice("GeoBorder", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])
    };
    // A land border from (50 N, 9 E) round by the east to (51 N, 9 E), and a coast back by the west.
    let borders = geo("land", "50 9 50 10 51 10 51 9") + &geo("coast", "50 9 50.5 8.5 51 9");
    let ring_xml = r#"<gml:Ring><gml:curveMember xlink:href="urn:uuid:land"/><gml:curveMember xlink:href="urn:uuid:coast"/></gml:Ring>"#;
    let body = format!(
        r#"<aixm:name>ISLAND</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior>{ring_xml}</gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
    );
    let area = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&(area + &borders)), &at("2026-01-15")).unwrap();
    let ring = ring(layer(&out, "t WorkArea").features[0].geometry.as_ref().unwrap());
    let a: f64 = ring.windows(2).map(|w| (w[1].x - w[0].x) * (w[1].y + w[0].y) / 2.0).sum::<f64>().abs();
    assert!((a - 1.25).abs() < 1e-6, "area {a}: {ring:?}");
    let notes = said(&out);
    assert!(!notes.iter().any(|m| m.contains("has no geometry") || m.contains("left out")), "{notes:?}");
    assert!(notes.iter().any(|m| m.contains("2 borders given by reference are drawn whole along the GeoBorders they name")), "{notes:?}");
}

#[test]
fn a_point_named_by_its_gml_id_is_found_though_it_has_an_identifier() {
    let point = |gid: &str, uuid: &str, lat: f64| {
        format!(
            r#"<message:hasMember><aixm:DesignatedPoint gml:id="{gid}"><gml:identifier codeSpace="urn:uuid:">{uuid}</gml:identifier>{}</aixm:DesignatedPoint></message:hasMember>"#,
            slice(
                "DesignatedPoint",
                "BASELINE",
                1,
                0,
                &period("2025-01-01T00:00:00Z", None),
                &format!(
                    r#"<aixm:location><aixm:Point srsName="urn:ogc:def:crs:EPSG::4326" gml:id="g{gid}"><gml:pos>{lat} 10</gml:pos></aixm:Point></aixm:location>"#
                )
            )
        )
    };
    let body = r##"<aixm:start><aixm:EnRouteSegmentPoint gml:id="s"><aixm:pointChoice_fixDesignatedPoint xlink:href="#P1"/></aixm:EnRouteSegmentPoint></aixm:start><aixm:end><aixm:EnRouteSegmentPoint gml:id="e"><aixm:pointChoice_fixDesignatedPoint xlink:href="urn:uuid:u2"/></aixm:EnRouteSegmentPoint></aixm:end>"##;
    let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]);
    let out = read("t.xml", &message(&(seg + &point("P1", "u1", 50.0) + &point("P2", "u2", 51.0))), &at("2026-01-15")).unwrap();
    let Some(Geometry::Line(parts)) = &layer(&out, "t RouteSegment").features[0].geometry else { panic!("not drawn") };
    assert_eq!(parts[0].first(), Some(&Coord::new(10.0, 50.0)));
    assert_eq!(parts[0].last(), Some(&Coord::new(10.0, 51.0)));
}

#[test]
fn route_segments_whose_points_are_elsewhere_are_kept_as_a_table() {
    let body = r#"<aixm:length uom="NM">12</aixm:length><aixm:start><aixm:EnRouteSegmentPoint gml:id="s"><aixm:pointChoice_fixDesignatedPoint xlink:href="urn:uuid:not-here"/></aixm:EnRouteSegmentPoint></aixm:start>"#;
    let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]);
    let out = read("t.xml", &message(&seg), &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t RouteSegment");
    assert_eq!(ds.geometry_type, GeometryType::None);
    assert_eq!(value(ds, 0, "length"), Value::Int(12));
    assert_eq!(value(ds, 0, "start_pointChoice_fixDesignatedPoint"), Value::Text("not-here".into()));
}

#[test]
fn a_structure_whose_parts_are_all_references_keeps_its_values() {
    let body = r#"<aixm:name>MAST</aixm:name><aixm:part xlink:href="urn:uuid:p1" xlink:title="MAST PART"/>"#;
    let vs =
        feature("VerticalStructure", "v", &[slice("VerticalStructure", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]);
    let point = point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "A")]);
    // A kind with nothing to draw is counted; its values reach the notice's count, not a table.
    let out = read("t.xml", &message(&(vs + &point)), &at("2026-01-15")).unwrap();
    assert!(
        said(&out).iter().any(|m| m.contains("1 feature of kinds with nothing to draw left out (VerticalStructure)")),
        "{:?}",
        said(&out)
    );
    // With a drawn part beside it, the structure's row keeps its name and the part's link.
    let located = r#"<aixm:part><aixm:VerticalStructurePart gml:id="a"><aixm:horizontalProjection_location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>47 -27</gml:pos></aixm:ElevatedPoint></aixm:horizontalProjection_location></aixm:VerticalStructurePart></aixm:part>"#;
    let body = format!("{body}{located}");
    let vs =
        feature("VerticalStructure", "v", &[slice("VerticalStructure", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&vs), &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t VerticalStructure");
    assert_eq!(value(ds, 0, "name"), Value::Text("MAST".into()));
    assert_eq!(value(ds, 0, "part"), Value::Text("p1".into()));
    assert_eq!(value(ds, 0, "part_title"), Value::Text("MAST PART".into()));
}

#[test]
fn a_route_segment_end_written_in_place_says_what_its_reading_finds() {
    let end = |name: &str, lat: f64| {
        format!(
            r#"<aixm:{name}><aixm:EnRouteSegmentPoint gml:id="{name}"><aixm:pointChoice_position><aixm:Point gml:id="g{name}" srsName="urn:ogc:def:crs:OGC:1.3:CRS83"><gml:pos>-97 {lat}</gml:pos></aixm:Point></aixm:pointChoice_position></aixm:EnRouteSegmentPoint></aixm:{name}>"#
        )
    };
    let body = end("start", 32.0) + &end("end", 33.0);
    let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let out = read("t.xml", &message(&seg), &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t RouteSegment");
    assert!(matches!(ds.features[0].geometry, Some(Geometry::Line(_))));
    assert!(said(&out).iter().any(|m| m.starts_with("2 geometries in a geographic system other than WGS 84")), "{:?}", said(&out));
    // One end good and the other in a projected system: not drawn, and each end said once.
    let bad = end("end", 33.0).replace("urn:ogc:def:crs:OGC:1.3:CRS83", "urn:ogc:def:crs:EPSG::32614");
    let body = end("start", 32.0) + &bad;
    let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let notes = said(&read("t.xml", &message(&seg), &at("2026-01-15")).unwrap());
    assert!(notes.iter().any(|m| m.starts_with("1 geometry in a geographic system other than WGS 84")), "{notes:?}");
    assert!(notes.iter().any(|m| m.starts_with("1 geometry in a system other than longitude/latitude")), "{notes:?}");
}

#[test]
fn an_empty_object_that_says_something_is_kept() {
    let body = format!("{}<aixm:record><aixm:Thing gml:id=\"t\" xsi:nil=\"true\" nilReason=\"unknown\"/></aixm:record>", named_point("N"));
    let out = read(
        "t.xml",
        &message(&point("a", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    assert_eq!(value(ds, 0, "record_Thing_nilReason"), Value::Text("unknown".into()));
    // One that only points elsewhere keeps its link.
    let body = format!("{}<aixm:record><aixm:Linked gml:id=\"l\" xlink:href=\"urn:uuid:abc\"/></aixm:record>", named_point("N"));
    let out = read(
        "t.xml",
        &message(&point("c", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert_eq!(value(layer(&out, "t DesignatedPoint"), 0, "record_Linked"), Value::Text("abc".into()));
    // One that links and holds values keeps both.
    let body = format!(
        "{}<aixm:record><aixm:Linked gml:id=\"l\" xlink:href=\"urn:uuid:abc\"><aixm:note>x</aixm:note></aixm:Linked></aixm:record>",
        named_point("N")
    );
    let out = read(
        "t.xml",
        &message(&point("d", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    assert_eq!(value(ds, 0, "record_Linked"), Value::Text("abc".into()));
    assert_eq!(value(ds, 0, "record_note"), Value::Text("x".into()));
    // One carrying only XML plumbing gives no column.
    let body = format!("{}<aixm:record><aixm:Plumbing gml:id=\"q\" xlink:type=\"simple\"/></aixm:record>", named_point("N"));
    let out = read(
        "t.xml",
        &message(&point("b", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert!(!layer(&out, "t DesignatedPoint").fields.iter().any(|f| f.name.starts_with("record_Plumbing")));
}

#[test]
fn a_ring_whose_borders_cant_be_joined_within_the_allowance_isnt_drawn_and_says_why() {
    let doc = format!(
        r#"<r {NS}><gml:exterior><gml:Ring><gml:curveMember xlink:href="urn:uuid:a"/><gml:curveMember xlink:href="urn:uuid:b"/><gml:curveMember><aixm:Curve gml:id="c" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:LineStringSegment><gml:posList>51 10 51 9 50 9</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></gml:curveMember></gml:Ring></gml:exterior></r>"#
    );
    let doc = roxmltree::Document::parse(&doc).unwrap();
    let boundary = doc.root_element().first_element_child().unwrap();
    let line = |pts: &[(f64, f64)]| pts.iter().map(|&(x, y)| Coord::new(x, y)).collect::<Vec<_>>();
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    shapes.add_border("a", [], line(&[(9.0, 50.0), (9.5, 50.0), (10.0, 50.0)]));
    shapes.add_border("b", [], line(&[(10.0, 50.0), (10.0, 50.5), (10.0, 51.0)]));
    // With room to look along both borders where they meet: drawn.
    shapes.scan_left = 1000;
    let mut notes = ShapeNotes::default();
    assert!(shapes.ring(boundary, &mut notes).is_some());
    assert_eq!((notes.followed, notes.border_allowance), (2, 0));
    // Without: not drawn, and said as past the work allowed, once: not also as a ring left out.
    let surface = format!(
        r#"<aixm:Surface {NS} gml:id="s" srsName="urn:ogc:def:crs:EPSG::4326"><gml:patches><gml:PolygonPatch>{}</gml:PolygonPatch></gml:patches></aixm:Surface>"#,
        &doc.input_text()[boundary.range()]
    );
    let surface = roxmltree::Document::parse(&surface).unwrap();
    shapes.scan_left = 4;
    let mut notes = ShapeNotes::default();
    assert!(shapes.surface(surface.root_element(), &mut notes).is_none());
    assert_eq!((notes.followed, notes.border_allowance, notes.rings_lost), (0, 1, 0));
}

// ---------------------------------------------------------------------------------------------
// From the review of PR #2819.
// ---------------------------------------------------------------------------------------------

fn snapshot_at(date: &str, name: &str) -> String {
    let valid = format!(
        r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition>{date}</gml:timePosition></gml:TimeInstant></gml:validTime>"#
    );
    format!(
        r#"<aixm:timeSlice><aixm:DesignatedPointTimeSlice gml:id="s">{valid}<aixm:interpretation>SNAPSHOT</aixm:interpretation>{}</aixm:DesignatedPointTimeSlice></aixm:timeSlice>"#,
        named_point(name)
    )
}

#[test]
fn snapshots_at_different_instants_are_all_kept() {
    let bytes = message(&point("s", &[snapshot_at("2026-01-01T00:00:00Z", "JANUARY"), snapshot_at("2026-02-01T00:00:00Z", "FEBRUARY")]));
    assert_eq!(names(&bytes, "2026-01-15").0, [Value::Text("JANUARY".into())]);
    assert_eq!(names(&bytes, "2026-02-15").0, [Value::Text("FEBRUARY".into())]);
}

#[test]
fn an_empty_valid_time_cancels_only_when_it_says_so() {
    let unknown = slice("DesignatedPoint", "BASELINE", 1, 1, r#"<gml:validTime nilReason="unknown"/>"#, &named_point("UNKNOWN"));
    let members = point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]) + &point("b", &[unknown]);
    let (got, notes) = names(&message(&members), "2026-01-15");
    assert_eq!(got, [Value::Text("KEPT".into())]);
    assert!(notes.iter().any(|m| m.starts_with("1 time slice with no validTime of the kind it needs")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("canceled")), "{notes:?}");
}

#[test]
fn a_sequence_number_that_isnt_one_is_said() {
    let s = baseline(1, 0, "2025-01-01T00:00:00Z", None, "A")
        .replace("<aixm:sequenceNumber>1</aixm:sequenceNumber>", "<aixm:sequenceNumber>one</aixm:sequenceNumber>");
    let notes = names(&message(&point("a", &[s])), "2026-01-15").1;
    assert!(
        notes.iter().any(|m| m.starts_with("1 time slice with a sequence or correction number that isn't a number read as 0")),
        "{notes:?}"
    );
}

#[test]
fn features_of_two_kinds_sharing_an_identifier_are_both_read() {
    let navaid =
        feature("Navaid", "same", &[slice("Navaid", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &named_point("NAV"))]);
    let out =
        read("t.xml", &message(&(point("same", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "POINT")]) + &navaid)), &at("2026-01-15"))
            .unwrap();
    assert_eq!(layer(&out, "t DesignatedPoint").features.len(), 1);
    assert_eq!(layer(&out, "t Navaid").features.len(), 1);
}

#[test]
fn a_permdelta_changes_an_open_baseline_from_its_instant() {
    let valid = r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition>2026-02-01T00:00:00Z</gml:timePosition></gml:TimeInstant></gml:validTime>"#;
    let perm = slice("DesignatedPoint", "PERMDELTA", 2, 0, valid, "<aixm:name>RENAMED</aixm:name>");
    let bytes = message(&point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "ORIGINAL"), perm]));
    assert_eq!(names(&bytes, "2026-01-15").0, [Value::Text("ORIGINAL".into())]);
    let out = read("t.xml", &bytes, &at("2026-02-01")).unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    assert_eq!(value(ds, 0, "name"), Value::Text("RENAMED".into()));
    assert_eq!(ds.features[0].geometry, Some(Geometry::Point(Coord::new(-35.17, 45.71))));
}

#[test]
fn an_airspace_hole_that_cant_be_read_leaves_the_airspace_undrawn() {
    let body = r#"<aixm:name>HOLED</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>50 9 50 11 52 11 52 9 50 9</gml:posList></gml:LinearRing></gml:exterior><gml:interior><gml:Ring><gml:curveMember xlink:href="urn:uuid:missing"/></gml:Ring></gml:interior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#;
    let bytes = message(&feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t WorkArea").features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("1 border is given by reference")), "{:?}", said(&out));
}

#[test]
fn a_border_segment_across_180_degrees_stays_whole() {
    // An edge from 179 E to 179 W, and a point at Greenwich: nowhere near it.
    let line = [Coord::new(179.0, 0.0), Coord::new(-179.0, 0.0)];
    let (.., d) = nearest_on(&line, Coord::new(0.0, 0.0)).unwrap();
    assert!(d > 19_000_000.0, "{d}");
    // And a point on it is on it.
    let (.., d) = nearest_on(&line, Coord::new(180.0, 0.0)).unwrap();
    assert!(d < 1.0, "{d}");
}

#[test]
fn a_tempdelta_replaces_extensions_within_their_namespace() {
    let ext = |ns: &str, value: &str| {
        format!(r#"<aixm:extension><x:Ext xmlns:x="urn:{ns}" gml:id="e{ns}"><x:v>{value}</x:v></x:Ext></aixm:extension>"#)
    };
    let base = slice(
        "DesignatedPoint",
        "BASELINE",
        1,
        0,
        &period("2025-01-01T00:00:00Z", None),
        &(named_point("P") + &ext("a", "A1") + &ext("b", "B1")),
    );
    let temp = slice("DesignatedPoint", "TEMPDELTA", 1, 0, &period("2026-01-01T00:00:00Z", None), &ext("a", "A2"));
    let out = read("t.xml", &message(&point("p", &[base, temp])), &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t DesignatedPoint");
    let all: Vec<String> =
        (0..ds.fields.len()).filter(|&k| ds.fields[k].name.starts_with("extension")).map(|k| ds.features[0].attrs[k].to_string()).collect();
    assert!(
        all.iter().any(|v| v.contains("A2")) && all.iter().any(|v| v.contains("B1")) && !all.iter().any(|v| v.contains("A1")),
        "{all:?}"
    );
}

#[test]
fn a_route_segment_follows_its_path_type() {
    let end = |name: &str, lat: f64, lon: f64| {
        format!(
            r#"<aixm:{name}><aixm:EnRouteSegmentPoint gml:id="{name}"><aixm:pointChoice_position><aixm:Point gml:id="g{name}" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>{lat} {lon}</gml:pos></aixm:Point></aixm:pointChoice_position></aixm:EnRouteSegmentPoint></aixm:{name}>"#
        )
    };
    let segment = |path: &str| {
        let body = format!("<aixm:pathType>{path}</aixm:pathType>{}{}", end("start", 60.0, 0.0), end("end", 60.0, 40.0));
        message(&feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]))
    };
    let line = |path: &str| {
        let out = read("t.xml", &segment(path), &at("2026-01-15")).unwrap();
        layer(&out, "t RouteSegment").features[0].geometry.clone()
    };
    // A rhumb line along 60 N stays on it; a geodesic between the same points bows north.
    let Some(Geometry::Line(rhl)) = line("RHL") else { panic!() };
    assert!(rhl[0].iter().all(|c| (c.y - 60.0).abs() < 1e-9));
    let Some(Geometry::Line(gds)) = line("GDS") else { panic!() };
    assert!(gds[0].iter().any(|c| c.y > 61.0));
    // A path type RustGIS doesn't draw: kept without a shape, and said.
    let out = read("t.xml", &segment("OTHER"), &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t RouteSegment").features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("1 route segment with no curve isn't drawn: its pathType")), "{:?}", said(&out));
}

#[test]
fn an_unlocated_record_beside_located_ones_is_a_row_of_its_own() {
    let light = |color: &str, at: &str| {
        format!(
            r#"<aixm:element><aixm:GroundLightingElement gml:id="e{color}"><aixm:colour>{color}</aixm:colour>{at}</aixm:GroundLightingElement></aixm:element>"#
        )
    };
    let located = r#"<aixm:location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>52 -32</gml:pos></aixm:ElevatedPoint></aixm:location>"#;
    let body = light("RED", "") + &light("GREEN", located);
    let bytes = message(&feature(
        "RunwayDirectionLightSystem",
        "l",
        &[slice("RunwayDirectionLightSystem", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)],
    ));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    let ds = layer(&out, "t RunwayDirectionLightSystem");
    assert_eq!(ds.features.len(), 2);
    let green = row_where(ds, "element_colour", "GREEN");
    assert!(ds.features[green].geometry.is_some());
    let red = row_where(ds, "element_colour", "RED");
    assert_eq!(ds.features[red].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("1 record with no shape")), "{:?}", said(&out));
}

#[test]
fn a_circle_past_the_point_allowance_keeps_eight_points_and_says_so() {
    let doc = format!(
        r#"<r {NS}><aixm:Curve gml:id="c" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:CircleByCenterPoint numArc="1"><gml:pos>50 10</gml:pos><gml:radius uom="km">10</gml:radius></gml:CircleByCenterPoint></gml:segments></aixm:Curve></r>"#
    );
    let doc = roxmltree::Document::parse(&doc).unwrap();
    let mut shapes = Shapes::default();
    let mut notes = ShapeNotes::default();
    let Some(Geometry::Line(l)) = shapes.shape(doc.root_element().first_element_child().unwrap(), &mut notes) else { panic!("not drawn") };
    assert_eq!(l[0].len(), 9);
    assert!(notes.coarse);
}

#[test]
fn an_airspace_of_a_surface_and_a_line_isnt_merged() {
    let volume = |op: &str, seq: u32, shape: &str| {
        format!(
            r#"<aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c"><aixm:operation>{op}</aixm:operation><aixm:operationSequence>{seq}</aixm:operationSequence><aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v"><aixm:horizontalProjection>{shape}</aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume></aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#
        )
    };
    let surface = r#"<aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>50 10 50 11 51 11 51 10 50 10</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface>"#;
    let curve = r#"<aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="k"><gml:segments><gml:LineStringSegment><gml:posList>52 10 52 11</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve>"#;
    let body = format!("<aixm:name>MIXED</aixm:name>{}{}", volume("BASE", 1, surface), volume("UNION", 2, curve));
    let out = read(
        "t.xml",
        &message(&feature("Airspace", "a", &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert_eq!(layer(&out, "t Airspace").features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("couldn't be read")), "{:?}", said(&out));
}

#[test]
fn an_effective_date_with_an_offset_past_a_day_is_refused() {
    assert!(Options::at_text("2026-01-15T12:00+99:99").is_err());
    assert!(Options::at_text("2026-01-15T12:00+05:30").is_ok());
}

#[test]
fn layer_names_match_the_layers_read() {
    let opts = at("2026-01-15");
    let mut read_names: Vec<String> = read("donlon-aixm.xml", DONLON, &opts).unwrap().into_iter().map(|(d, _)| d.name).collect();
    let mut listed = layer_names("donlon-aixm.xml", DONLON, &opts).unwrap();
    read_names.sort();
    listed.sort();
    assert_eq!(listed, read_names);
}

#[test]
fn a_rhumb_line_near_a_pole_ends_at_its_points() {
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    let mut notes = ShapeNotes::default();
    let (a, b) = (Coord::new(0.0, 88.0), Coord::new(20.0, 88.0));
    let line = shapes.rhumb(a, b, &mut notes);
    assert_eq!((line.first(), line.last()), (Some(&a), Some(&b)));
    assert!(line.iter().all(|c| (c.y - 88.0).abs() < 1e-6), "{line:?}");
}

#[test]
fn a_three_point_circle_past_the_allowance_keeps_eight_points_and_says_so() {
    let mut shapes = Shapes::default();
    let mut notes = ShapeNotes::default();
    let drawn = shapes.arcs_through(&[Coord::new(10.0, 50.0), Coord::new(10.1, 50.1), Coord::new(10.2, 50.0)], "Circle", &mut notes);
    assert_eq!(drawn.len(), 9);
    assert!(notes.coarse);
}

#[test]
fn an_arc_with_too_few_positions_is_drawn_through_them_and_an_empty_one_is_skipped_both_said() {
    let curve = |arc: &str| {
        format!(
            r#"<aixm:name>C</aixm:name><aixm:curveExtent><aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="k"><gml:segments><gml:LineStringSegment><gml:posList>50 10 50 11</gml:posList></gml:LineStringSegment><gml:Arc><gml:posList>{arc}</gml:posList></gml:Arc></gml:segments></aixm:Curve></aixm:curveExtent>"#
        )
    };
    let run = |arc: &str| {
        let bytes = message(&feature(
            "RouteSegment",
            "r",
            &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &curve(arc))],
        ));
        read("t.xml", &bytes, &at("2026-01-15")).unwrap()
    };
    let out = run("50 11 51 11");
    assert!(layer(&out, "t RouteSegment").features[0].geometry.is_some());
    assert!(said(&out).iter().any(|m| m.contains("1 feature has an arc or circle not given the three positions")), "{:?}", said(&out));
    let out = run("50 11 50.5 11.5 51 11");
    assert!(!said(&out).iter().any(|m| m.contains("not given the three positions")), "{:?}", said(&out));
    let out = run("");
    assert!(layer(&out, "t RouteSegment").features[0].geometry.is_some());
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 feature has an arc or circle with no positions, left out")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("not given the three positions") || m.contains("drawn as points along them")), "{notes:?}");
    // In a line straight (along a meridian), drawn straight and said; a real arc says nothing of it.
    let out = run("50 11 50.5 11 51 11");
    assert!(said(&out).iter().any(|m| m.contains("1 feature has an arc or circle drawn straight")), "{:?}", said(&out));
    let out = run("50 11 50.5 11.5 51 11");
    assert!(!said(&out).iter().any(|m| m.contains("drawn straight:") || m.contains("with no positions")), "{:?}", said(&out));
    // An Arc of five positions: all of them drawn, as an ArcString would be.
    let out = run("50 11 50.5 11.5 51 11 51.5 10.5 52 11");
    assert!(!said(&out).iter().any(|m| m.contains("not given the three positions") || m.contains("straight between")), "{:?}", said(&out));
    let Some(Geometry::Line(l)) = &layer(&out, "t RouteSegment").features[0].geometry else { panic!("not drawn") };
    assert!(l[0].last().is_some_and(|c| (c.x - 11.0).abs() < 1e-9 && (c.y - 52.0).abs() < 1e-9), "{:?}", l[0].last());
    // Of four: the last joined straight, said under its own name.
    let out = run("50 11 50.5 11.5 51 11 51.5 10.5");
    assert!(said(&out).iter().any(|m| m.contains("drawn straight between their positions (Arc)")), "{:?}", said(&out));
    // Two positions: no arc drawn, so none said.
    let out = run("50 11 51 11");
    assert!(!said(&out).iter().any(|m| m.contains("drawn as points along them")), "{:?}", said(&out));
}

#[test]
fn an_airspace_not_drawn_says_nothing_of_how_its_parts_were_drawn() {
    let volume = |op: &str, seq: u32, shape: &str| {
        format!(
            r#"<aixm:geometryComponent><aixm:AirspaceGeometryComponent gml:id="c{seq}"><aixm:operation>{op}</aixm:operation><aixm:operationSequence>{seq}</aixm:operationSequence><aixm:theAirspaceVolume><aixm:AirspaceVolume gml:id="v{seq}"><aixm:horizontalProjection>{shape}</aixm:horizontalProjection></aixm:AirspaceVolume></aixm:theAirspaceVolume></aixm:AirspaceGeometryComponent></aixm:geometryComponent>"#
        )
    };
    let circle = r#"<aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s1"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember><gml:Curve gml:id="k1"><gml:segments><gml:CircleByCenterPoint numArc="1"><gml:pos>50 10</gml:pos><gml:radius uom="km">10</gml:radius></gml:CircleByCenterPoint></gml:segments></gml:Curve></gml:curveMember></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface>"#;
    let bad = r#"<aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s2"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>not numbers</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface>"#;
    let body = format!("<aixm:name>HALF</aixm:name>{}{}", volume("BASE", 1, circle), volume("UNION", 2, bad));
    let out = read(
        "t.xml",
        &message(&feature("Airspace", "a", &[slice("Airspace", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert_eq!(layer(&out, "t Airspace").features[0].geometry, None);
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("couldn't be read")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("arcs or circles")), "{notes:?}");
}

#[test]
fn an_empty_arc_that_is_the_whole_line_says_nothing_of_arcs() {
    let body = r#"<aixm:name>E</aixm:name><aixm:curveExtent><aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="k"><gml:segments><gml:Arc><gml:posList></gml:posList></gml:Arc></gml:segments></aixm:Curve></aixm:curveExtent>"#;
    let bytes =
        message(&feature("RouteSegment", "r", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), body)]));
    let out = read("t.xml", &bytes, &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t RouteSegment").features[0].geometry, None);
    assert!(!said(&out).iter().any(|m| m.contains("arc")), "{:?}", said(&out));
}

#[test]
fn a_circle_given_more_than_three_positions_or_in_a_line_is_said() {
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    let mut notes = ShapeNotes::default();
    let c = [Coord::new(10.0, 50.0), Coord::new(10.1, 50.1), Coord::new(10.2, 50.0), Coord::new(10.1, 49.9)];
    shapes.arcs_through(&c, "Circle", &mut notes);
    assert!(notes.short_arcs && !notes.straight_arcs);
    let mut notes = ShapeNotes::default();
    shapes.arcs_through(&[Coord::new(10.0, 50.0), Coord::new(10.0, 50.1), Coord::new(10.0, 50.2)], "Arc", &mut notes);
    assert!(notes.straight_arcs && !notes.short_arcs);
    let mut notes = ShapeNotes::default();
    shapes.arcs_through(&c[..3], "Circle", &mut notes);
    assert!(!notes.straight_arcs && !notes.short_arcs);
}

#[test]
fn a_small_circle_with_points_to_spare_keeps_what_it_needs() {
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    let mut notes = ShapeNotes::default();
    let n = shapes.pieces(0.5, 360.0, &mut notes);
    assert_eq!(n, 4);
    assert!(!notes.coarse);
}

#[test]
fn a_patch_left_out_for_its_hole_is_counted_beside_the_holes_reason() {
    let patch = |hole: &str| {
        format!(
            r#"<gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>50 9 50 11 52 11 52 9 50 9</gml:posList></gml:LinearRing></gml:exterior>{hole}</gml:PolygonPatch>"#
        )
    };
    let hole = r#"<gml:interior><gml:Ring><gml:curveMember xlink:href="urn:uuid:missing"/></gml:Ring></gml:interior>"#;
    let body = format!(
        r#"<aixm:name>TWO</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches>{}{}</gml:patches></aixm:Surface></aixm:extent>"#,
        patch(hole),
        patch("")
    );
    let out = read(
        "t.xml",
        &message(&feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    let Some(Geometry::Polygon(parts)) = &layer(&out, "t WorkArea").features[0].geometry else { panic!("not drawn") };
    assert_eq!(parts.len(), 1);
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 ring or surface patch was left out")), "{notes:?}");
    assert!(notes.iter().any(|m| m.contains("1 border is given by reference")), "{notes:?}");
}

#[test]
fn a_chain_of_arcs_across_180_degrees_stays_whole() {
    let mut shapes = Shapes { budget: 1_000_000, ..Shapes::default() };
    let mut notes = ShapeNotes::default();
    let c = [
        Coord::new(179.90, 10.0),
        Coord::new(179.95, 10.02),
        Coord::new(179.98, 10.0),
        Coord::new(-179.97, 9.98),
        Coord::new(-179.92, 10.0),
    ];
    let line = shapes.arcs_through(&c, "ArcString", &mut notes);
    assert!(line.windows(2).all(|w| (w[1].x - w[0].x).abs() < 1.0), "{line:?}");
}

// ---------------------------------------------------------------------------------------------
// From the second review of PR #2819.
// ---------------------------------------------------------------------------------------------

fn change(kind: &str, seq: u32, valid: &str, body: &str) -> String {
    slice("DesignatedPoint", kind, seq, 0, valid, body)
}

fn instant(date: &str) -> String {
    format!(r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition>{date}</gml:timePosition></gml:TimeInstant></gml:validTime>"#)
}

#[test]
fn a_snapshot_is_shown_as_it_is_and_changes_since_are_said() {
    let snap = snapshot_at("2026-01-01T00:00:00Z", "SNAP");
    let temp = |from: &str| change("TEMPDELTA", 1, &period(from, Some("2026-01-20T00:00:00Z")), "<aixm:name>TEMP</aixm:name>");
    let perm = change("PERMDELTA", 1, &instant("2026-01-10T00:00:00Z"), "<aixm:name>PERM</aixm:name>");
    for (slices, said) in [
        (vec![snap.clone()], false),
        // In force at its instant: it holds it already.
        (vec![snap.clone(), temp("2025-12-20T00:00:00Z")], false),
        (vec![snap.clone(), temp("2026-01-10T00:00:00Z")], true),
        (vec![snap.clone(), perm], true),
        // In force at its instant, ended since.
        (vec![snap.clone(), change("TEMPDELTA", 1, &period("2025-12-20T00:00:00Z", Some("2026-01-10T00:00:00Z")), "")], true),
        // An end that isn't a date may be between the two; a begin after both rules it out.
        (vec![snap.clone(), change("TEMPDELTA", 1, &period("2025-12-20T00:00:00Z", Some("bad")), "")], true),
        (vec![snap.clone(), change("TEMPDELTA", 1, "", "")], true),
        (vec![snap.clone(), change("TEMPDELTA", 1, &period("2027-01-01T00:00:00Z", Some("bad")), "")], false),
    ] {
        let (got, notes) = names(&message(&point("s", &slices)), "2026-01-15");
        assert_eq!(got, [Value::Text("SNAP".into())]);
        assert_eq!(notes.iter().any(|m| m.contains("1 feature shown as a SNAPSHOT that doesn't reflect changes")), said, "{notes:?}");
    }
    // One after the date, holding a permanent change not in force yet.
    let ahead = change("PERMDELTA", 1, &instant("2025-12-10T00:00:00Z"), "<aixm:name>SNAP</aixm:name>");
    let notes = names(&message(&point("s", &[snap.clone(), ahead])), "2025-12-01").1;
    assert!(notes.iter().any(|m| m.contains("SNAPSHOT that doesn't reflect")), "{notes:?}");
    // Withdrawn since: not shown.
    let end = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>2020-01-01T00:00:00Z</gml:beginPosition><gml:endPosition>2026-01-10T00:00:00Z</gml:endPosition></gml:TimePeriod></aixm:featureLifetime>"#;
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let bytes = message(&(point("s", &[snap, change("PERMDELTA", 1, &instant("2026-01-10T00:00:00Z"), end)]) + &kept));
    assert_eq!(names(&bytes, "2026-01-05").0.len(), 2);
    let (got, notes) = names(&bytes, "2026-01-15");
    assert_eq!(got, [Value::Text("KEPT".into())]);
    assert!(notes.iter().any(|m| m.contains("featureLifetime") && !m.contains("can't be read")), "{notes:?}");
}

#[test]
fn a_slice_whose_period_isnt_said_withholds_its_feature_while_it_may_apply() {
    let base = baseline(1, 0, "2025-01-01T00:00:00Z", None, "BASE");
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let temp = |valid: &str| change("TEMPDELTA", 1, valid, "<aixm:name>TEMP</aixm:name>");
    let run = |t: String, date| names(&message(&(point("a", &[base.clone(), t]) + &kept)), date).0;
    assert_eq!(run(temp(""), "2026-01-15"), [Value::Text("KEPT".into())]);
    // An instant where a period is required: not before it, at least.
    let t = temp(&instant("2026-01-01T00:00:00Z"));
    assert_eq!(run(t.clone(), "2026-01-15"), [Value::Text("KEPT".into())]);
    assert_eq!(run(t, "2025-12-15").len(), 2);
    // A lifetime on a temporary change isn't used, read or not.
    let lifetime = r#"<aixm:name>TEMP</aixm:name><aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>bad</gml:beginPosition><gml:endPosition/></gml:TimePeriod></aixm:featureLifetime>"#;
    let t = change("TEMPDELTA", 1, &period("2026-01-01T00:00:00Z", None), lifetime);
    assert!(run(t, "2026-01-15").contains(&Value::Text("TEMP".into())));
}

#[test]
fn a_route_segment_is_never_drawn_as_a_point() {
    let written = r#"<aixm:end><aixm:EnRouteSegmentPoint gml:id="e"><aixm:pointChoice_position><aixm:Point gml:id="ge" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>50 10</gml:pos></aixm:Point></aixm:pointChoice_position></aixm:EnRouteSegmentPoint></aixm:end>"#;
    let missing = r#"<aixm:start><aixm:EnRouteSegmentPoint gml:id="s"><aixm:pointChoice_fixDesignatedPoint xlink:href="urn:uuid:no"/></aixm:EnRouteSegmentPoint></aixm:start>"#;
    let nil = r#"<aixm:start xsi:nil="true" nilReason="unknown"/>"#;
    for (start, why) in [(missing, "names isn't in the file"), (nil, "start or end isn't given")] {
        let body = format!("{start}{written}");
        let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
        let out = read("t.xml", &message(&seg), &at("2026-01-15")).unwrap();
        assert_eq!(layer(&out, "t RouteSegment").features[0].geometry, None);
        assert!(said(&out).iter().any(|m| m.contains(why)), "{:?}", said(&out));
        assert!(said(&out).iter().any(|m| m.contains("1 shape")), "{:?}", said(&out));
    }
}

#[test]
fn a_shape_lost_beside_one_drawn_is_counted() {
    let light = |pos: &str| {
        format!(r#"<aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>{pos}</gml:pos></aixm:ElevatedPoint>"#)
    };
    let run = |second: &str| {
        let body = format!(
            "<aixm:element><aixm:Group gml:id=\"g\"><aixm:a>{}</aixm:a><aixm:b>{}</aixm:b></aixm:Group></aixm:element>",
            light("52 -32"),
            light(second)
        );
        let f = feature("LightSystem", "l", &[slice("LightSystem", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
        said(&read("t.xml", &message(&f), &at("2026-01-15")).unwrap())
    };
    assert!(run("abc def").iter().any(|m| m.contains("1 shape couldn't be read and is left out")));
    assert!(!run("52.1 -32").iter().any(|m| m.contains("left out")));
    assert!(!run("").iter().any(|m| m.contains("left out")));
}

#[test]
fn a_patch_lost_to_a_border_beside_one_drawn_is_counted() {
    let lost = r#"<gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember xlink:href="urn:uuid:no"/></gml:Ring></gml:exterior></gml:PolygonPatch>"#;
    let square = r#"<gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>52 9 52 11 54 11 54 9 52 9</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch>"#;
    let body = format!(
        r#"<aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches>{lost}{square}</gml:patches></aixm:Surface></aixm:extent>"#
    );
    let f = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let notes = said(&read("t.xml", &message(&f), &at("2026-01-15")).unwrap());
    assert!(notes.iter().any(|m| m.contains("1 ring or surface patch was left out")), "{notes:?}");
}

#[test]
fn a_slice_that_cant_be_read_hides_nothing_it_cant_apply_to() {
    let base = baseline(1, 0, "2025-01-01T00:00:00Z", None, "BASE");
    // A change that begins after the date, its end not a date.
    let later = slice("DesignatedPoint", "TEMPDELTA", 2, 0, &period("2030-01-01T00:00:00Z", Some("bad")), "<aixm:name>X</aixm:name>");
    let bytes = message(&point("a", &[base.clone(), later]));
    assert_eq!(names(&bytes, "2026-01-15").0, [Value::Text("BASE".into())]);
    let e = read("t.xml", &bytes, &at("2031-01-15")).unwrap_err().to_string();
    assert!(e.contains("may apply"), "{e}");
    // A permanent change begun before the state on the date, its lifetime not a date.
    let instant = r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition>2024-01-01T00:00:00Z</gml:timePosition></gml:TimeInstant></gml:validTime>"#;
    let lifetime = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>bad</gml:beginPosition><gml:endPosition/></gml:TimePeriod></aixm:featureLifetime>"#;
    let old = slice("DesignatedPoint", "PERMDELTA", 2, 0, instant, lifetime);
    assert_eq!(names(&message(&point("a", &[base, old])), "2026-01-15").0, [Value::Text("BASE".into())]);
}

#[test]
fn a_canceled_correction_of_one_that_cant_be_read_is_canceled() {
    let bad = slice("DesignatedPoint", "BASELINE", 1, 0, &period("bad", None), &named_point("A"));
    let canceled = slice("DesignatedPoint", "BASELINE", 1, 1, r#"<gml:validTime nilReason="inapplicable"/>"#, "");
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let notes = names(&message(&(point("a", &[bad, canceled]) + &kept)), "2026-01-15").1;
    assert!(
        notes.iter().any(|m| m.contains("canceled")) && !notes.iter().any(|m| m.contains("feature left out: a time slice")),
        "{notes:?}"
    );
}

#[test]
fn a_change_or_snapshot_that_cant_be_read_withholds_its_feature() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let temp = |corr, valid: &str| slice("DesignatedPoint", "TEMPDELTA", 2, corr, valid, "<aixm:name>TEMP</aixm:name>");
    let unknown = r#"<gml:validTime nilReason="unknown"/>"#;
    let base = baseline(1, 0, "2025-01-01T00:00:00Z", None, "BASE");
    for slices in [
        vec![base.clone(), temp(0, unknown)],
        vec![base.clone(), temp(0, &period("2026-01-01T00:00:00Z", None)), temp(1, unknown)],
        vec![snapshot_at("not-a-date", "SNAP")],
        vec![snapshot_at("2025-01-01T00:00:00Z", "OLD"), snapshot_at("not-a-date", "NEW")],
    ] {
        let (got, notes) = names(&message(&(point("a", &slices) + &kept)), "2026-01-15");
        assert_eq!(got, [Value::Text("KEPT".into())]);
        assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice of it that may apply")), "{notes:?}");
        assert!(!notes.iter().any(|m| m.contains("canceled")), "{notes:?}");
    }
    // One superseded by a correction that can be read withholds nothing.
    let (got, _) = names(&message(&point("a", &[base, temp(0, unknown), temp(1, &period("2026-01-01T00:00:00Z", None))])), "2026-01-15");
    assert_eq!(got, [Value::Text("TEMP".into())]);
}

#[test]
fn a_correction_that_cant_be_read_withholds_its_feature() {
    let unknown = slice("DesignatedPoint", "BASELINE", 1, 1, r#"<gml:validTime nilReason="unknown"/>"#, &named_point("CORRECTED"));
    let bytes = message(
        &(point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "OLD"), unknown])
            + &point("b", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")])),
    );
    let (got, notes) = names(&bytes, "2026-01-15");
    assert_eq!(got, [Value::Text("KEPT".into())]);
    assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice of it that may apply")), "{notes:?}");
    // A clean file says nothing of it.
    let notes = names(&message(&point("b", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")])), "2026-01-15").1;
    assert!(!notes.iter().any(|m| m.contains("a time slice of it that may apply")), "{notes:?}");
}

fn structure(parts: &str) -> Vec<u8> {
    message(&feature(
        "VerticalStructure",
        "v",
        &[slice(
            "VerticalStructure",
            "BASELINE",
            1,
            0,
            &period("2025-01-01T00:00:00Z", None),
            &format!("<aixm:name>MAST</aixm:name>{parts}"),
        )],
    ))
}

fn part(id: &str, shapes: &str) -> String {
    format!(
        r#"<aixm:part><aixm:VerticalStructurePart gml:id="{id}"><aixm:designator>{id}</aixm:designator>{shapes}</aixm:VerticalStructurePart></aixm:part>"#
    )
}

const PART_POINT: &str = r#"<aixm:horizontalProjection_location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>52 -32</gml:pos></aixm:ElevatedPoint></aixm:horizontalProjection_location>"#;

#[test]
fn listing_names_gives_what_a_read_gives_for_a_structure_with_a_part_that_cant_be_drawn() {
    let bad_curve = r#"<aixm:horizontalProjection_linearExtent><aixm:ElevatedCurve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>52 -32</gml:posList></gml:LineStringSegment></gml:segments></aixm:ElevatedCurve></aixm:horizontalProjection_linearExtent>"#;
    let good_curve = r#"<aixm:horizontalProjection_linearExtent><aixm:ElevatedCurve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>52 -32 52.1 -32</gml:posList></gml:LineStringSegment></gml:segments></aixm:ElevatedCurve></aixm:horizontalProjection_linearExtent>"#;
    for bytes in [structure(&(part("A", PART_POINT) + &part("B", bad_curve))), structure(&(part("A", PART_POINT) + &part("B", good_curve)))]
    {
        let opts = at("2026-01-15");
        let mut read_names: Vec<String> = read("t.xml", &bytes, &opts).unwrap().into_iter().map(|(d, _)| d.name).collect();
        let mut listed = layer_names("t.xml", &bytes, &opts).unwrap();
        read_names.sort();
        listed.sort();
        assert_eq!(listed, read_names);
    }
}

#[test]
fn a_structure_part_with_two_shapes_draws_the_first_and_says_so() {
    let line = r#"<aixm:horizontalProjection_linearExtent><aixm:ElevatedCurve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>52 -32 52.1 -32</gml:posList></gml:LineStringSegment></gml:segments></aixm:ElevatedCurve></aixm:horizontalProjection_linearExtent>"#;
    let out = read("t.xml", &structure(&part("A", &format!("{PART_POINT}{line}"))), &at("2026-01-15")).unwrap();
    assert!(said(&out).iter().any(|m| m.contains("1 structure part holds more than one shape")), "{:?}", said(&out));
    let out = read("t.xml", &structure(&part("A", PART_POINT)), &at("2026-01-15")).unwrap();
    assert!(!said(&out).iter().any(|m| m.contains("more than one shape")), "{:?}", said(&out));
}

fn named_at(kind: &str, id: &str, lat: f64, lon: f64) -> String {
    let body = format!(
        r#"<aixm:name>{kind}</aixm:name><aixm:location><aixm:Point srsName="urn:ogc:def:crs:EPSG::4326" gml:id="g{kind}"><gml:pos>{lat} {lon}</gml:pos></aixm:Point></aixm:location>"#
    );
    feature(kind, id, &[slice(kind, "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])
}

fn route_to(choice: &str, start_id: &str) -> String {
    let end = |name: &str, href: &str| {
        format!(
            r#"<aixm:{name}><aixm:EnRouteSegmentPoint gml:id="{name}"><aixm:{choice} xlink:href="urn:uuid:{href}"/></aixm:EnRouteSegmentPoint></aixm:{name}>"#
        )
    };
    let body = format!("{}{}", end("start", start_id), end("end", "far"));
    feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])
}

#[test]
fn a_route_end_is_the_point_of_the_kind_it_names() {
    let members = named_at("DesignatedPoint", "same", 50.0, 10.0)
        + &named_at("Navaid", "same", 55.0, 20.0)
        + &named_at("DesignatedPoint", "far", 50.0, 12.0);
    let out = read("t.xml", &message(&(members + &route_to("pointChoice_fixDesignatedPoint", "same"))), &at("2026-01-15")).unwrap();
    let Some(Geometry::Line(l)) = &layer(&out, "t RouteSegment").features[0].geometry else { panic!("not drawn") };
    assert_eq!(l[0].first(), Some(&Coord::new(10.0, 50.0)));
}

#[test]
fn a_route_segment_whose_ends_arent_in_the_file_is_kept_and_said() {
    let out = read("t.xml", &message(&route_to("pointChoice_fixDesignatedPoint", "missing")), &at("2026-01-15")).unwrap();
    let rs = layer(&out, "t RouteSegment");
    assert_eq!(rs.features.len(), 1);
    assert_eq!(rs.features[0].geometry, None);
    assert!(said(&out).iter().any(|m| m.contains("1 route segment is kept without a line")), "{:?}", said(&out));
    // The kind named and not in the file: not drawn to a point of another kind.
    let members = named_at("DesignatedPoint", "same", 50.0, 10.0) + &named_at("DesignatedPoint", "far", 50.0, 12.0);
    let out = read("t.xml", &message(&(members.clone() + &route_to("pointChoice_navaidSystem", "same"))), &at("2026-01-15")).unwrap();
    assert_eq!(layer(&out, "t RouteSegment").features[0].geometry, None);
    // Both ends found: nothing said of it.
    let out = read("t.xml", &message(&(members + &route_to("pointChoice_fixDesignatedPoint", "same"))), &at("2026-01-15")).unwrap();
    assert!(!said(&out).iter().any(|m| m.contains("kept without a line")), "{:?}", said(&out));
}

#[test]
fn a_structure_part_is_drawn_from_its_first_shape_that_can_be() {
    let bad_point = r#"<aixm:horizontalProjection_location><aixm:ElevatedPoint srsName="urn:ogc:def:crs:EPSG::4326" gml:id="p"><gml:pos>abc def</gml:pos></aixm:ElevatedPoint></aixm:horizontalProjection_location>"#;
    let line = r#"<aixm:horizontalProjection_linearExtent><aixm:ElevatedCurve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>52 -32 52.1 -32</gml:posList></gml:LineStringSegment></gml:segments></aixm:ElevatedCurve></aixm:horizontalProjection_linearExtent>"#;
    let out = read("t.xml", &structure(&part("A", &format!("{bad_point}{line}"))), &at("2026-01-15")).unwrap();
    let vs = &out.iter().find(|(d, _)| d.name.starts_with("t VerticalStructure")).unwrap().0;
    assert!(matches!(vs.features[0].geometry, Some(Geometry::Line(_))));
    assert!(said(&out).iter().any(|m| m.contains("1 shape couldn't be read and is left out")), "{:?}", said(&out));
}

#[test]
fn a_patch_left_out_says_nothing_of_how_it_was_drawn() {
    let circle_with_hole = r#"<gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember><gml:Curve gml:id="k1"><gml:segments><gml:CircleByCenterPoint numArc="1"><gml:pos>50 10</gml:pos><gml:radius uom="km">10</gml:radius></gml:CircleByCenterPoint></gml:segments></gml:Curve></gml:curveMember></gml:Ring></gml:exterior><gml:interior><gml:Ring><gml:curveMember xlink:href="urn:uuid:missing"/></gml:Ring></gml:interior></gml:PolygonPatch>"#;
    let square = r#"<gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>52 9 52 11 54 11 54 9 52 9</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch>"#;
    let body = format!(
        r#"<aixm:name>TWO</aixm:name><aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches>{circle_with_hole}{square}</gml:patches></aixm:Surface></aixm:extent>"#
    );
    let out = read(
        "t.xml",
        &message(&feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)])),
        &at("2026-01-15"),
    )
    .unwrap();
    assert!(layer(&out, "t WorkArea").features[0].geometry.is_some());
    let notes = said(&out);
    assert!(notes.iter().any(|m| m.contains("1 ring or surface patch was left out")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("arcs or circles")), "{notes:?}");
}

#[test]
fn a_long_edge_ending_near_180_degrees_is_near_a_point_across_it() {
    let line = [Coord::new(0.0, 0.0), Coord::new(179.999755859375, 0.0)];
    let (.., d) = nearest_on(&line, Coord::new(-180.0, 0.0)).unwrap();
    assert!(d < 50.0, "{d}");
    let (.., d) = nearest_on(&[Coord::new(179.0, 0.0), Coord::new(-179.0, 0.0)], Coord::new(0.0, 0.0)).unwrap();
    assert!(d > 19_000_000.0, "{d}");
}

#[test]
fn slices_that_cant_be_read_are_judged_by_the_dates_they_have() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let run = |slices: &[String]| names(&message(&(point("a", slices) + &kept)), "2026-01-15");
    let shown = |slices: &[String], name: &str| run(slices).0.contains(&Value::Text(name.into()));
    let base = baseline(2, 0, "2025-01-01T00:00:00Z", None, "BASE");
    // A permanent change with no instant: not applied, and its feature withheld.
    let unplaced = r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition indeterminatePosition="unknown"/></gml:TimeInstant></gml:validTime>"#;
    assert_eq!(run(&[base.clone(), change("PERMDELTA", 1, unplaced, "<aixm:name>OTHER</aixm:name>")]).0, [Value::Text("KEPT".into())]);
    // A baseline that began before the one that holds, its end unread; and one that began after.
    assert!(shown(&[baseline(1, 0, "2020-01-01T00:00:00Z", Some("bad"), "OLD"), base.clone()], "BASE"));
    assert_eq!(run(&[baseline(1, 0, "2025-06-01T00:00:00Z", Some("bad"), "NEW"), base.clone()]).0, [Value::Text("KEPT".into())]);
    // A snapshot that isn't the nearest, its lifetime unread; and one that is.
    let lifetime = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>bad</gml:beginPosition><gml:endPosition/></gml:TimePeriod></aixm:featureLifetime>"#;
    let unread = |date: &str| snapshot_at(date, "UNREAD").replace("<aixm:interpretation>", &format!("{lifetime}<aixm:interpretation>"));
    let snap = snapshot_at("2026-01-01T00:00:00Z", "SNAP");
    assert!(shown(&[unread("2024-01-01T00:00:00Z"), snap.clone()], "SNAP"));
    assert!(shown(&[unread("2027-01-01T00:00:00Z"), snap.clone()], "SNAP"));
    assert_eq!(run(&[unread("2026-01-10T00:00:00Z"), snap.clone()]).0, [Value::Text("KEPT".into())]);
    // A snapshot with no instant can't be placed, so it isn't chosen over one that can.
    for date in ["", "\" indeterminatePosition=\"unknown"] {
        let dated = snapshot_at("2026-06-01T00:00:00Z", "LATER");
        let undated = snapshot_at("X", "UNDATED").replace(
            "<gml:timePosition>X</gml:timePosition>",
            if date.is_empty() { "<gml:timePosition/>" } else { r#"<gml:timePosition indeterminatePosition="unknown"/>"# },
        );
        assert_eq!(run(&[undated, dated]).0, [Value::Text("KEPT".into())]);
    }
    // A canceled change is canceled, whatever its lifetime says.
    let canceled = change("PERMDELTA", 1, r#"<gml:validTime nilReason="inapplicable"/>"#, lifetime);
    assert!(shown(&[base.clone(), canceled], "BASE"));
    // A number that isn't one is said on a slice that can't be read too.
    let odd = baseline(7, 0, "bad", None, "X").replace("<aixm:sequenceNumber>7<", "<aixm:sequenceNumber>abc<");
    let notes = run(&[base, odd]).1;
    assert!(notes.iter().any(|m| m.contains("number that isn't a number read as 0")), "{notes:?}");
}

#[test]
fn a_drawn_route_segment_says_the_other_shapes_of_its_records() {
    let area = r#"<aixm:evaluationArea><aixm:Area gml:id="a"><aixm:surface><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>52 9 52 11 54 11 52 9</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:surface></aixm:Area></aixm:evaluationArea>"#;
    let members = named_at("DesignatedPoint", "near", 50.0, 10.0) + &named_at("DesignatedPoint", "far", 50.0, 12.0);
    let plain = route_to("pointChoice_fixDesignatedPoint", "near");
    for (route, count) in [
        (plain.clone(), 0),
        (plain.replace("<aixm:start>", &format!("{area}<aixm:start>")), 1),
        (plain.replace("<aixm:end>", &format!("<aixm:end>{area}")), 1),
    ] {
        let out = read("t.xml", &message(&(members.clone() + &route)), &at("2026-01-15")).unwrap();
        assert!(layer(&out, "t RouteSegment").features[0].geometry.is_some());
        assert_eq!(said(&out).iter().filter(|m| m.contains("1 shape")).count(), count, "{:?}", said(&out));
    }
}

#[test]
fn a_second_position_in_a_route_end_is_counted() {
    let point = |id: &str, pos: &str| {
        format!(
            r#"<aixm:pointChoice_position><aixm:Point gml:id="{id}" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>{pos}</gml:pos></aixm:Point></aixm:pointChoice_position>"#
        )
    };
    let end = |name: &str, inner: &str| {
        format!(r#"<aixm:{name}><aixm:EnRouteSegmentPoint gml:id="{name}">{inner}</aixm:EnRouteSegmentPoint></aixm:{name}>"#)
    };
    for (second, count) in [("", 0), (point("g3", "40 10").as_str(), 1)] {
        let body = end("start", &format!("{}{second}", point("g1", "50 10"))) + &end("end", &point("g2", "50 12"));
        let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
        let out = read("t.xml", &message(&seg), &at("2026-01-15")).unwrap();
        assert!(layer(&out, "t RouteSegment").features[0].geometry.is_some());
        assert_eq!(said(&out).iter().filter(|m| m.contains("1 shape")).count(), count, "{:?}", said(&out));
    }
}

#[test]
fn a_route_segments_shapes_are_told_apart_across_time_slices() {
    // The same node of two documents: the change's start, in the line, and the state's area, not.
    let written = |name: &str, pos: &str| {
        format!(
            r#"<aixm:{name}><aixm:EnRouteSegmentPoint gml:id="{name}"><aixm:pointChoice_position><aixm:Point gml:id="g{name}" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>{pos}</gml:pos></aixm:Point></aixm:pointChoice_position></aixm:EnRouteSegmentPoint></aixm:{name}>"#
        )
    };
    let body = written("area", "40 10") + &written("end", "50 12");
    let base = slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body);
    let temp = slice("RouteSegment", "TEMPDELTA", 1, 0, &period("2026-01-01T00:00:00Z", None), &written("start", "50 10"));
    let out = read("t.xml", &message(&feature("RouteSegment", "rs", &[base, temp])), &at("2026-01-15")).unwrap();
    assert!(layer(&out, "t RouteSegment").features[0].geometry.is_some());
    assert!(said(&out).iter().any(|m| m.contains("1 shape")), "{:?}", said(&out));
}

#[test]
fn a_snapshot_after_the_date_is_judged_by_its_own_lifetime() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let snap = snapshot_at("2026-01-01T00:00:00Z", "SNAP");
    let life = |from: &str| {
        let life = format!(
            r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>{from}</gml:beginPosition><gml:endPosition indeterminatePosition="unknown"/></gml:TimePeriod></aixm:featureLifetime>"#
        );
        snap.replace("<aixm:interpretation>", &format!("{life}<aixm:interpretation>"))
    };
    let run = |s: String| names(&message(&(point("s", &[s]) + &kept)), "2025-12-01");
    assert_eq!(run(life("2025-12-10T00:00:00Z")).0, [Value::Text("KEPT".into())]);
    // Two at one instant: the later in the file, on either date.
    let two = message(&point("s", &[snap.clone(), snapshot_at("2026-01-01T00:00:00Z", "SECOND")]));
    for date in ["2025-12-01", "2026-01-15"] {
        assert_eq!(names(&two, date).0, [Value::Text("SECOND".into())]);
    }
    // A creation given by a change before it, read or not, isn't worked back: said.
    for from in ["2025-12-10T00:00:00Z", "bad"] {
        let created = format!(
            r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>{from}</gml:beginPosition><gml:endPosition indeterminatePosition="unknown"/></gml:TimePeriod></aixm:featureLifetime>"#
        );
        let perm = change("PERMDELTA", 1, &instant("2025-12-10T00:00:00Z"), &created);
        let (got, notes) = names(&message(&(point("s", &[snap.clone(), perm]) + &kept)), "2025-12-01");
        assert_eq!(got.len(), 2);
        assert!(notes.iter().any(|m| m.contains("may not have existed on it")), "{notes:?}");
        assert!(notes.iter().any(|m| m.contains("SNAPSHOT that doesn't reflect changes")), "{notes:?}");
    }
    for s in [life("2020-01-01T00:00:00Z"), snap.clone()] {
        let (got, notes) = run(s);
        assert_eq!(got.len(), 2);
        assert!(notes.iter().any(|m| m.contains("may not be its state on the date asked")), "{notes:?}");
    }
}

#[test]
fn an_empty_route_end_isnt_a_shape_that_isnt_drawn() {
    let empty = r#"<aixm:end><aixm:EnRouteSegmentPoint gml:id="e"><aixm:pointChoice_position><aixm:Point gml:id="ge" srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos></gml:pos></aixm:Point></aixm:pointChoice_position></aixm:EnRouteSegmentPoint></aixm:end>"#;
    let seg = feature("RouteSegment", "rs", &[slice("RouteSegment", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), empty)]);
    let notes = said(&read("t.xml", &message(&seg), &at("2026-01-15")).unwrap());
    assert!(!notes.iter().any(|m| m.contains("1 shape")), "{notes:?}");
}

#[test]
fn an_empty_instant_is_said_once() {
    let perm =
        change("PERMDELTA", 1, r#"<gml:validTime><gml:TimeInstant gml:id="i"><gml:timePosition/></gml:TimeInstant></gml:validTime>"#, "");
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let notes = names(&message(&(point("a", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "A"), perm]) + &kept)), "2026-01-15").1;
    assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice")), "{notes:?}");
    assert!(!notes.iter().any(|m| m.contains("read as open")), "{notes:?}");
}

#[test]
fn a_change_given_a_period_is_withheld_and_a_snapshot_is_read_at_its_start_and_said() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let base = baseline(1, 0, "2025-01-01T00:00:00Z", None, "BASE");
    let valid = period("2026-01-01T00:00:00Z", Some("2026-02-01T00:00:00Z"));
    let run = |s: &[String], d| names(&message(&(point("a", s) + &kept)), d);
    let perm = change("PERMDELTA", 1, &valid, "<aixm:name>PERM</aixm:name>");
    assert_eq!(run(&[base.clone(), perm.clone()], "2025-12-15").0, [Value::Text("BASE".into()), Value::Text("KEPT".into())]);
    for d in ["2026-01-15", "2026-06-15"] {
        let (got, notes) = run(&[base.clone(), perm.clone()], d);
        assert_eq!(got, [Value::Text("KEPT".into())]);
        assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice")), "{notes:?}");
    }
    // One whose period spans the state's start may be made after it; one that ended by then isn't.
    let spans = |end| change("PERMDELTA", 1, &period("2024-12-01T00:00:00Z", end), "<aixm:name>PERM</aixm:name>");
    for (end, count) in [(Some("2025-06-01T00:00:00Z"), 1), (None, 1), (Some("bad"), 1), (Some("2025-01-01T00:00:00Z"), 2)] {
        assert_eq!(run(&[base.clone(), spans(end)], "2026-01-15").0.len(), count, "{end:?}");
    }
    // On the state's own date there is no time for it since; nor one that ended before the state,
    // its start not known.
    assert_eq!(run(&[base.clone(), spans(None)], "2025-01-01").0.len(), 2);
    let ended = |end: &str| {
        let valid = format!(
            r#"<gml:validTime><gml:TimePeriod gml:id="t"><gml:beginPosition indeterminatePosition="unknown"/><gml:endPosition>{end}</gml:endPosition></gml:TimePeriod></gml:validTime>"#
        );
        change("PERMDELTA", 1, &valid, "<aixm:name>PERM</aixm:name>")
    };
    assert_eq!(run(&[base.clone(), ended("2020-01-01T00:00:00Z")], "2026-01-15").0.len(), 2);
    assert_eq!(run(&[base.clone(), ended("2025-06-01T00:00:00Z")], "2026-01-15").0.len(), 1);
    // One that ends before it begins, whichever date is wrong.
    let reversed = change("PERMDELTA", 1, &period("2026-01-01T00:00:00Z", Some("2024-02-01T00:00:00Z")), "<aixm:name>PERM</aixm:name>");
    assert_eq!(run(&[base.clone(), reversed], "2026-01-15").0.len(), 1);
    let reversed = change("PERMDELTA", 1, &period("2026-02-01T00:00:00Z", Some("2024-02-01T00:00:00Z")), "<aixm:name>PERM</aixm:name>");
    let notes = run(&[snapshot_at("2026-03-01T00:00:00Z", "SNAP"), reversed], "2026-01-15").1;
    assert!(notes.iter().any(|m| m.contains("doesn't reflect changes")), "{notes:?}");
    // One with no date at all: withheld since the state, shown and said before a later snapshot.
    let undated = change("PERMDELTA", 1, &instant("bad"), "<aixm:name>PERM</aixm:name>");
    assert_eq!(run(&[base.clone(), undated.clone()], "2026-01-15").0.len(), 1);
    assert_eq!(run(&[base.clone(), undated.clone()], "2025-01-01").0.len(), 2);
    let (got, notes) = run(&[snapshot_at("2026-03-01T00:00:00Z", "SNAP"), undated], "2026-01-15");
    assert!(got.len() == 2 && notes.iter().any(|m| m.contains("doesn't reflect changes")), "{got:?} {notes:?}");
    // Past a snapshot's instant, or open, and before one later than the date: shown, and said.
    for end in [Some("2026-04-01T00:00:00Z"), None, Some("bad")] {
        let p = change("PERMDELTA", 1, &period("2026-01-01T00:00:00Z", end), "<aixm:name>PERM</aixm:name>");
        let (got, notes) = run(&[snapshot_at("2026-03-01T00:00:00Z", "SNAP"), p.clone()], "2026-01-15");
        assert!(got.len() == 2 && notes.iter().any(|m| m.contains("doesn't reflect changes")), "{end:?} {got:?} {notes:?}");
        // On its own date it reflects them all.
        let notes = run(&[snapshot_at("2026-01-15T00:00:00Z", "SNAP"), p], "2026-01-15").1;
        assert!(!notes.iter().any(|m| m.contains("doesn't reflect changes")), "{end:?} {notes:?}");
    }
    // And before a later snapshot: said.
    let later = run(&[snapshot_at("2026-03-01T00:00:00Z", "SNAP"), perm], "2026-01-15").1;
    assert!(later.iter().any(|m| m.contains("doesn't reflect changes")), "{later:?}");
    let said = |notes: &[String]| notes.iter().any(|m| m.contains("1 SNAPSHOT given a period"));
    // Its end isn't used, read or not.
    for end in ["PERM", ""] {
        let valid = format!(
            r#"<gml:validTime><gml:TimePeriod gml:id="t"><gml:beginPosition>2026-01-01T00:00:00Z</gml:beginPosition><gml:endPosition>{end}</gml:endPosition></gml:TimePeriod></gml:validTime>"#
        );
        let (got, notes) = run(&[slice("DesignatedPoint", "SNAPSHOT", 1, 0, &valid, &named_point("SNAP"))], "2026-01-15");
        assert!(got.len() == 2 && said(&notes) && !notes.iter().any(|m| m.contains("read as open")), "{got:?} {notes:?}");
    }
    // Its instant is its start, as one given that instant.
    let from = |s: String| {
        let out = read("t.xml", &message(&point("a", &[s])), &at("2026-01-15")).unwrap();
        value(layer(&out, "t DesignatedPoint"), 0, "validFrom")
    };
    let given = from(snapshot_at("2026-01-01T00:00:00Z", "SNAP"));
    assert_ne!(given, Value::Null);
    assert_eq!(from(slice("DesignatedPoint", "SNAPSHOT", 1, 0, &valid, &named_point("SNAP"))), given);
    let (got, notes) = run(&[slice("DesignatedPoint", "SNAPSHOT", 1, 0, &valid, &named_point("SNAP"))], "2026-01-15");
    assert!(got.len() == 2 && said(&notes), "{got:?} {notes:?}");
    assert!(!said(&run(&[snapshot_at("2026-01-01T00:00:00Z", "SNAP")], "2026-01-15").1));
}

#[test]
fn a_later_snapshot_wins_a_tie_read_or_not() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let lifetime = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>bad</gml:beginPosition><gml:endPosition/></gml:TimePeriod></aixm:featureLifetime>"#;
    let snap = snapshot_at("2026-01-01T00:00:00Z", "SNAP");
    let unread = snap.replace("<aixm:interpretation>", &format!("{lifetime}<aixm:interpretation>"));
    for d in ["2025-12-15", "2026-01-15"] {
        let run = |s: &[String]| names(&message(&(point("a", s) + &kept)), d).0;
        assert_eq!(run(&[unread.clone(), snap.clone()]), [Value::Text("SNAP".into()), Value::Text("KEPT".into())]);
        assert_eq!(run(&[snap.clone(), unread.clone()]), [Value::Text("KEPT".into())]);
    }
}

#[test]
fn a_feature_is_found_by_the_gml_id_of_any_of_its_members() {
    let member = |gid: &str, seq: u32, from: &str, to: Option<&str>, pos: &str| {
        let b = format!(
            r#"<aixm:location><aixm:Point srsName="urn:ogc:def:crs:EPSG::4326" gml:id="g"><gml:pos>{pos}</gml:pos></aixm:Point></aixm:location>"#
        );
        feature("DesignatedPoint", "near", &[slice("DesignatedPoint", "BASELINE", seq, 0, &period(from, to), &b)])
            .replace(r#"gml:id="f""#, &format!(r#"gml:id="{gid}""#))
    };
    let members = member("p-old", 1, "2020-01-01T00:00:00Z", Some("2025-01-01T00:00:00Z"), "40 10")
        + &member("p-new", 2, "2025-01-01T00:00:00Z", None, "50 10")
        + &named_at("DesignatedPoint", "far", 50.0, 12.0);
    for id in ["p-old", "p-new"] {
        let route = route_to("pointChoice_fixDesignatedPoint", "X").replace("urn:uuid:X", &format!("#{id}"));
        let out = read("t.xml", &message(&(members.clone() + &route)), &at("2026-01-15")).unwrap();
        let Some(Geometry::Line(l)) = &layer(&out, "t RouteSegment").features[0].geometry else { panic!("{id} not drawn") };
        assert_eq!(l[0].first(), Some(&Coord::new(10.0, 50.0)));
    }
}

#[test]
fn rows_past_the_tables_memory_are_left_out_and_said() {
    let wide: String = (0..1500).map(|i| format!("<aixm:p{i}>1</aixm:p{i}>")).collect();
    let one = |i: usize, more: &str| {
        point(
            &format!("p{i}"),
            &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &(named_point("N") + more))],
        )
    };
    let rows = 400;
    let sparse = message(&(0..rows).map(|i| one(i, if i == 0 { &wide } else { "" })).collect::<String>());
    let narrow = message(&(0..rows).map(|i| one(i, "")).collect::<String>());
    let run = |bytes: &[u8], floor| {
        let out = read_within("t.xml", bytes, &at("2026-01-15"), floor).unwrap();
        (layer(&out, "t DesignatedPoint").features.len(), said(&out))
    };
    // Sparse and wide: what eight times the file allows, the rest said.
    let (kept, notes) = run(&sparse, 0);
    assert!(kept > 0 && kept < rows, "{kept}");
    assert!(notes.iter().any(|m| m.contains(&format!("{} rows are left out", rows - kept))), "{notes:?}");
    // The same within the floor, and tall and narrow without it: all kept, nothing said.
    for (n, notes) in [run(&sparse, 1 << 30), run(&narrow, 0)] {
        assert_eq!(n, rows);
        assert!(!notes.iter().any(|m| m.contains("left out")), "{notes:?}");
    }
}

#[test]
fn a_border_is_followed_by_the_gml_id_of_any_of_its_members() {
    // The past line goes by 50.2, the one in force by 50.7.
    let border = |gid: &str, seq: u32, from: &str, to: Option<&str>, mid: f64| {
        let b = format!(
            r#"<aixm:border><aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c-{gid}"><gml:segments><gml:LineStringSegment><gml:posList>50 9 {mid} 10 51 10</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#
        );
        feature("GeoBorder", "line", &[slice("GeoBorder", "BASELINE", seq, 0, &period(from, to), &b)])
            .replace(r#"gml:id="f""#, &format!(r#"gml:id="{gid}""#))
    };
    let members = border("b-old", 1, "2020-01-01T00:00:00Z", Some("2025-01-01T00:00:00Z"), 50.2)
        + &border("b-new", 2, "2025-01-01T00:00:00Z", None, 50.7);
    for id in ["b-old", "b-new", "c-b-old", "c-b-new"] {
        let area = format!(
            r##"<aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember xlink:href="#{id}"/><gml:curveMember><gml:Curve gml:id="k"><gml:segments><gml:LineStringSegment><gml:posList>51 10 51 9 50 9</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve></gml:curveMember></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"##
        );
        let f = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &area)]);
        let out = read("t.xml", &message(&(members.clone() + &f)), &at("2026-01-15")).unwrap();
        let g = layer(&out, "t WorkArea").features[0].geometry.as_ref();
        let r = ring(g.unwrap_or_else(|| panic!("{id}: {:?}", said(&out))));
        let by = |lat: f64| r.iter().any(|c| (c.x - 10.0).abs() < 1e-9 && (c.y - lat).abs() < 1e-9);
        assert!(by(50.7) && !by(50.2), "{id}: {r:?}");
    }
}

#[test]
fn a_border_named_two_ways_in_a_row_is_one_border() {
    let b = r#"<aixm:border><aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="c"><gml:segments><gml:LineStringSegment><gml:posList>50 8 50 9 50 9.5 50 10 50 11</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#;
    let border = |gid: &str, seq: u32, from: &str, to: Option<&str>| {
        feature("GeoBorder", "line", &[slice("GeoBorder", "BASELINE", seq, 0, &period(from, to), b)])
            .replace(r#"gml:id="f""#, &format!(r#"gml:id="{gid}""#))
    };
    let members =
        border("b-old", 1, "2020-01-01T00:00:00Z", Some("2025-01-01T00:00:00Z")) + &border("b-new", 2, "2025-01-01T00:00:00Z", None);
    let drawn = |refs: &[&str]| {
        let refs: String = refs.iter().map(|r| format!(r#"<gml:curveMember xlink:href="{r}"/>"#)).collect();
        let area = format!(
            r#"<aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring>{refs}<gml:curveMember><gml:Curve gml:id="k"><gml:segments><gml:LineStringSegment><gml:posList>50 10 51 10 51 9 50 9</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve></gml:curveMember></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
        );
        let f = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &area)]);
        let out = read("t.xml", &message(&(members.clone() + &f)), &at("2026-01-15")).unwrap();
        ring(layer(&out, "t WorkArea").features[0].geometry.as_ref().unwrap()).to_vec()
    };
    let once = drawn(&["#b-new"]);
    assert!(once.iter().all(|c| (8.999..=10.001).contains(&c.x)), "{once:?}");
    for refs in [["#b-old", "#b-new"], ["#c", "#b-new"], ["urn:uuid:line", "#b-old"]] {
        assert_eq!(drawn(&refs), once, "{refs:?}");
    }
}

#[test]
fn borders_of_alike_identifiers_are_told_apart_by_their_gml_ids() {
    let border = |id: &str, gid: &str, curve: &str, lat: f64| {
        let b = format!(
            r#"<aixm:border><aixm:Curve srsName="urn:ogc:def:crs:EPSG::4326" gml:id="{curve}"><gml:segments><gml:LineStringSegment><gml:posList>{lat} 9 {lat} 10</gml:posList></gml:LineStringSegment></gml:segments></aixm:Curve></aixm:border>"#
        );
        feature("GeoBorder", id, &[slice("GeoBorder", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &b)])
            .replace(r#"gml:id="f""#, &format!(r#"gml:id="{gid}""#))
    };
    let members = border("urn:uuid:X", "b1", "c1", 50.0) + &border("X", "b2", "c2", 40.0);
    let drawn = |name: &str, members: &str| {
        let area = format!(
            r#"<aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring><gml:curveMember xlink:href="{name}"/><gml:curveMember><gml:Curve gml:id="k"><gml:segments><gml:LineStringSegment><gml:posList>45 10 45 9</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve></gml:curveMember></gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
        );
        let f = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &area)]);
        let out = read("t.xml", &message(&(members.to_string() + &f)), &at("2026-01-15")).unwrap();
        let lats = layer(&out, "t WorkArea").features[0].geometry.as_ref().map(|g| {
            let mut l: Vec<i64> = ring(g).iter().map(|c| c.y.round() as i64).collect();
            l.sort_unstable();
            l.dedup();
            l
        });
        (lats, said(&out))
    };
    // Each by its own gml:ids, and by its identifier as it is written.
    for (name, lat) in
        [("#b1", 50), ("#c1", 50), ("urn:uuid:X", 50), ("#//aixm:GeoBorder[@gml:id='b1']", 50), ("#b2", 40), ("#c2", 40), ("X", 40)]
    {
        let (lats, _) = drawn(name, &members);
        let lats = lats.unwrap_or_else(|| panic!("{name} not drawn"));
        assert!(lats.contains(&lat) && !lats.contains(&(90 - lat)), "{name}: {lats:?}");
    }
    // A name two have is neither's, and that is said; an identifier that looks like a gml:id isn't one.
    let shared = members.replace("c2", "c1");
    let (lats, notes) = drawn("#c1", &shared);
    assert_eq!(lats, None);
    assert!(notes.iter().any(|m| m.contains("has a name others share")), "{notes:?}");
    // Nor when one of the two isn't in force.
    let later = shared.replacen("2025-01-01T00:00:00Z", "2030-01-01T00:00:00Z", 1);
    assert_eq!(drawn("#c1", &later).0, None);
    // A UUID is the same in either case; another identifier isn't.
    let uuid = "f81d4fae-7dec-11d0-a765-00a0c91e6bf6";
    let one = border(uuid, "u", "cu", 50.0) + &border("Abc", "a", "ca", 40.0);
    for name in [uuid.to_string(), format!("urn:uuid:{}", uuid.to_uppercase())] {
        assert!(drawn(&name, &one).0.is_some_and(|l| l.contains(&50)), "{name}");
    }
    assert_eq!(drawn("ABC", &one).0, None);
    // One not in force is no other: not drawn along the one whose identifier is alike.
    let past = members.replacen("2025-01-01T00:00:00Z", "2030-01-01T00:00:00Z", 1);
    for name in ["urn:uuid:X", "#b1"] {
        let (lats, notes) = drawn(name, &past);
        assert_eq!(lats, None, "{name}");
        assert!(notes.iter().any(|m| m.contains("not in force then")), "{notes:?}");
    }
    // Nor one of another document.
    for name in ["other.xml#//aixm:GeoBorder[@gml:id='b1']", "other.xml#b1"] {
        assert_eq!(drawn(name, &members).0, None, "{name}");
    }
    assert!(drawn("X", &past).0.is_some_and(|l| l.contains(&40)));
    let odd = members.clone() + &border("#b1", "b3", "c3", 40.0);
    assert!(drawn("#b1", &odd).0.is_some_and(|l| l.contains(&50)));
}

#[test]
fn a_border_not_in_the_file_spends_nothing_of_those_that_are() {
    let doc = format!(
        r#"<r {NS}><gml:exterior><gml:Ring><gml:curveMember xlink:href="urn:uuid:v"/><gml:curveMember xlink:href="urn:uuid:no1"/><gml:curveMember xlink:href="urn:uuid:no2"/></gml:Ring></gml:exterior></r>"#
    );
    let doc = roxmltree::Document::parse(&doc).unwrap();
    let mut shapes = Shapes { budget: 1000, scan_left: 1000, ..Shapes::default() };
    shapes.add_border("v", [], vec![Coord::new(9.0, 50.0), Coord::new(10.0, 50.0)]);
    let mut notes = ShapeNotes::default();
    assert!(shapes.ring(doc.root_element().first_element_child().unwrap(), &mut notes).is_none());
    assert_eq!((notes.referenced, notes.border_allowance, shapes.budget, shapes.scan_left), (1, 0, 1000, 1000));
    // Nor the curves written before it.
    let doc = format!(
        r#"<r {NS}><gml:exterior><gml:Ring><gml:curveMember><gml:Curve gml:id="k" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:CircleByCenterPoint numArc="1"><gml:pos>50 10</gml:pos><gml:radius uom="km">10</gml:radius></gml:CircleByCenterPoint></gml:segments></gml:Curve></gml:curveMember><gml:curveMember xlink:href="urn:uuid:no"/></gml:Ring></gml:exterior></r>"#
    );
    let doc = roxmltree::Document::parse(&doc).unwrap();
    assert!(shapes.ring(doc.root_element().first_element_child().unwrap(), &mut notes).is_none());
    assert_eq!((shapes.budget, shapes.scan_left), (1000, 1000));
}

#[test]
fn a_period_that_ends_before_it_begins_withholds_its_feature() {
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let base = baseline(1, 0, "2025-01-01T00:00:00Z", None, "BASE");
    for (begin, end) in [("2026-01-10T00:00:00Z", "2025-01-20T00:00:00Z"), ("2027-01-01T00:00:00Z", "2025-06-01T00:00:00Z")] {
        let valid = period(begin, Some(end));
        for kind in ["TEMPDELTA", "PERMDELTA"] {
            let (got, notes) =
                names(&message(&(point("a", &[base.clone(), change(kind, 1, &valid, "<aixm:name>X</aixm:name>")]) + &kept)), "2026-01-15");
            assert_eq!(got, [Value::Text("KEPT".into())], "{kind} {begin}");
            assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice")), "{notes:?}");
        }
        let (got, notes) = names(&message(&(point("a", &[baseline(1, 0, begin, Some(end), "BASE")]) + &kept)), "2026-01-15");
        assert_eq!(got, [Value::Text("KEPT".into())], "{begin}");
        assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice")), "{notes:?}");
    }
    // Before a later snapshot: shown, and said.
    let perm = change("PERMDELTA", 1, &period("2026-04-01T00:00:00Z", Some("2026-02-01T00:00:00Z")), "<aixm:name>X</aixm:name>");
    let (got, notes) = names(&message(&(point("a", &[snapshot_at("2026-03-01T00:00:00Z", "SNAP"), perm]) + &kept)), "2026-01-15");
    assert!(got.len() == 2 && notes.iter().any(|m| m.contains("doesn't reflect changes")), "{notes:?}");
}

#[test]
fn a_route_end_named_as_two_points_are_is_neither() {
    let far = named_at("DesignatedPoint", "far", 50.0, 12.0);
    let run = |points: &str, route: &str| {
        let out = read("t.xml", &message(&(points.to_string() + &far + route)), &at("2026-01-15")).unwrap();
        (layer(&out, "t RouteSegment").features[0].geometry.is_some(), said(&out))
    };
    let by_id = route_to("pointChoice_fixDesignatedPoint", "X");
    let by_gid = by_id.replace("urn:uuid:X", "#p");
    let two = |lat: f64| {
        (named_at("DesignatedPoint", "X", 50.0, 10.0) + &named_at("DesignatedPoint", "urn:uuid:X", lat, 10.0))
            .replace(r#"gml:id="f""#, r#"gml:id="p""#)
    };
    let (drawn, notes) = run(&two(40.0), &by_gid);
    assert!(!drawn && notes.iter().any(|m| m.contains("has a name others share")), "{notes:?}");
    // One written exactly so is that one.
    let start = |points: &str, route: &str| {
        let out = read("t.xml", &message(&(points.to_string() + &far + route)), &at("2026-01-15")).unwrap();
        match &layer(&out, "t RouteSegment").features[0].geometry {
            Some(Geometry::Line(l)) => l[0].first().copied(),
            _ => None,
        }
    };
    assert_eq!(start(&two(40.0), &by_id), Some(Coord::new(10.0, 40.0)));
    assert_eq!(start(&two(40.0), &by_id.replace("urn:uuid:X", "X")), Some(Coord::new(10.0, 50.0)));
    // One written without its prefix is found with it.
    assert_eq!(start(&named_at("DesignatedPoint", "X", 50.0, 10.0), &by_id), Some(Coord::new(10.0, 50.0)));
    // A choice of no known kind naming an identifier two kinds share: neither.
    let kinds = named_at("DesignatedPoint", "X", 50.0, 10.0) + &named_at("Navaid", "X", 40.0, 10.0);
    let unknown = by_id.replace("pointChoice_fixDesignatedPoint xlink:href=\"urn:uuid:X", "pointChoice_other xlink:href=\"urn:uuid:X");
    assert_eq!(start(&kinds, &unknown), None);
    assert_eq!(start(&kinds, &by_id), Some(Coord::new(10.0, 50.0)));
    // A UUID is the same in either case.
    let uuid = "f81d4fae-7dec-11d0-a765-00a0c91e6bf6";
    let upper = by_id.replace("urn:uuid:X", &format!("urn:uuid:{}", uuid.to_uppercase()));
    assert_eq!(start(&named_at("DesignatedPoint", uuid, 50.0, 10.0), &upper), Some(Coord::new(10.0, 50.0)));
    // One of another document is none of this one's; an XPath into this one is.
    let here = named_at("DesignatedPoint", "X", 50.0, 10.0).replace(r#"gml:id="f""#, r#"gml:id="p""#);
    let xpath = "#/m/aixm:DesignatedPoint[@gml:id='p']";
    assert_eq!(start(&here, &by_gid.replace("#p", xpath)), Some(Coord::new(10.0, 50.0)));
    assert_eq!(start(&here, &by_gid.replace("#p", &format!("other.xml{xpath}"))), None);
    // Any feature with a position, by its gml:id.
    let vor = named_at("VOR", "V", 50.0, 10.0).replace(r#"gml:id="f""#, r#"gml:id="p""#);
    assert_eq!(start(&vor, &by_gid), Some(Coord::new(10.0, 50.0)));
    // One not in force is no other of an alike identifier.
    let past =
        two(40.0).replace("2025-01-01T00:00:00Z", "2030-01-01T00:00:00Z").replacen("2030-01-01T00:00:00Z", "2025-01-01T00:00:00Z", 1);
    assert_eq!(start(&past, &by_id), None);
    assert_eq!(start(&past, &by_id.replace("urn:uuid:X", "X")), Some(Coord::new(10.0, 50.0)));
    // An identifier that looks like a gml:id isn't one.
    let odd = named_at("DesignatedPoint", "A", 50.0, 10.0).replace(r#"gml:id="f""#, r#"gml:id="p""#)
        + &named_at("DesignatedPoint", "#p", 40.0, 10.0);
    assert_eq!(start(&odd, &by_gid), Some(Coord::new(10.0, 50.0)));
}

#[test]
fn a_ring_member_that_names_nothing_isnt_a_missing_border() {
    for member in ["<gml:curveMember/>", r#"<gml:curveMember xlink:href=" "/>"#] {
        let area = format!(
            r#"<aixm:extent><aixm:Surface srsName="urn:ogc:def:crs:EPSG::4326" gml:id="s"><gml:patches><gml:PolygonPatch><gml:exterior><gml:Ring>{member}</gml:Ring></gml:exterior></gml:PolygonPatch></gml:patches></aixm:Surface></aixm:extent>"#
        );
        let f = feature("WorkArea", "w", &[slice("WorkArea", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &area)]);
        let notes = said(&read("t.xml", &message(&f), &at("2026-01-15")).unwrap());
        assert!(!notes.iter().any(|m| m.contains("border is given by reference")), "{notes:?}");
        assert!(notes.iter().any(|m| m.contains("1 ring or surface patch was left out")), "{notes:?}");
        // And spends nothing.
        let doc = format!(
            r#"<r {NS}><gml:exterior><gml:Ring><gml:curveMember><gml:Curve gml:id="k" srsName="urn:ogc:def:crs:EPSG::4326"><gml:segments><gml:CircleByCenterPoint numArc="1"><gml:pos>50 10</gml:pos><gml:radius uom="km">10</gml:radius></gml:CircleByCenterPoint></gml:segments></gml:Curve></gml:curveMember>{member}</gml:Ring></gml:exterior></r>"#
        );
        let doc = roxmltree::Document::parse(&doc).unwrap();
        let mut shapes = Shapes { budget: 1000, ..Shapes::default() };
        assert!(shapes.ring(doc.root_element().first_element_child().unwrap(), &mut ShapeNotes::default()).is_none());
        assert_eq!(shapes.budget, 1000);
    }
}

#[test]
fn a_lifetime_that_ends_before_it_begins_withholds_its_feature_and_says_so() {
    let life = r#"<aixm:featureLifetime><gml:TimePeriod gml:id="l"><gml:beginPosition>2020-06-01T00:00:00Z</gml:beginPosition><gml:endPosition>2020-03-01T00:00:00Z</gml:endPosition></gml:TimePeriod></aixm:featureLifetime>"#;
    let kept = point("k", &[baseline(1, 0, "2025-01-01T00:00:00Z", None, "KEPT")]);
    let body = named_point("A") + life;
    let a = point("a", &[slice("DesignatedPoint", "BASELINE", 1, 0, &period("2025-01-01T00:00:00Z", None), &body)]);
    let (got, notes) = names(&message(&(a + &kept)), "2026-01-15");
    assert_eq!(got, [Value::Text("KEPT".into())]);
    assert!(notes.iter().any(|m| m.contains("1 feature left out: a time slice") && m.contains("ends before it begins")), "{notes:?}");
}
