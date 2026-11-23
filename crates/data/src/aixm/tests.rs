//! Tests of the AIXM reader: the Donlon 2025 sample (BSD-2-Clause, EUROCONTROL & FAA) and small
//! messages written here for each rule of the time slices, the arcs and hostile input.

use super::*;

const DONLON: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/donlon-aixm.xml"));

fn at(date: &str) -> Options {
    Options { at: crate::time::parse_text(date) }
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
        "1 route segment with no curve is drawn as a geodesic between the points its start and end name",
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
    assert!(said(&out).iter().any(|m| m.contains("2 geometries in a geographic system other than WGS 84")), "{:?}", said(&out));
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
}
