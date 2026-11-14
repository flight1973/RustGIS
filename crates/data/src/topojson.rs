//! TopoJSON: geometries sharing arcs, quantized and delta-encoded when the topology has a
//! transform. Each object of the topology becomes a layer (its geometries rebuilt from their arcs
//! as GeoJSON features, then read as GeoJSON).
//!
//! The file is hostile: arc indices are checked, nesting is fixed by the geometry types, and the
//! number of coordinates rebuilt is bounded.
//!
//! A position may hold more than x and y (TopoJSON specification §2.1.1, which leaves their meaning
//! open). The first further number (a height or measure) is carried into the rebuilt GeoJSON as is,
//! neither scaled nor delta-decoded (§2.1.2: the reference implementation copies it), so the
//! GeoJSON reading counts the features that had one and says it dropped them. (Positions take 24
//! bytes rather than 16 for it, within the `MAX_COORDS` bound.)
//!
//! An arc with a position that can't be read is not read: in a quantized topology every later
//! position of the arc is a delta from it, so leaving it out would move the rest of the line.
//! Geometries using it are counted and said, as are collection members left out.

use serde_json::{Map, Value as Json, json};

use crate::{DataError, Dataset, ReadReport, Result};

/// The most coordinates rebuilt from arcs.
const MAX_COORDS: usize = 20_000_000;
/// The most objects (layers) read.
const MAX_OBJECTS: usize = 64;

pub fn is_topojson_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".topojson") || n.ends_with(".topo.json")
}

/// Whether JSON text is a TopoJSON topology.
pub fn looks_like_topojson(text: &str) -> bool {
    // The first 512 bytes, cut at a character boundary.
    let end = (0..=text.len().min(512)).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    let head = text.get(..end).unwrap_or("");
    // `"type": "Topology"`, whatever the spacing (a property value "Topology" doesn't count).
    let squeezed: String = head.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains("\"type\":\"Topology\"")
}

/// A position: x, y and its first further number (a height or measure), or NaN for none (JSON
/// holds no NaN, so it can't be one read).
type Pos = [f64; 3];

/// The first number after a position's x and y, or NaN.
fn extra(a: &[Json]) -> f64 {
    a.iter().skip(2).find_map(Json::as_f64).unwrap_or(f64::NAN)
}

/// A position as GeoJSON: `[x, y]`, or `[x, y, extra]` when it had a further number.
fn pos_json(p: &Pos) -> Json {
    let [x, y, e] = *p;
    if e.is_nan() { json!([x, y]) } else { json!([x, y, e]) }
}

/// Positions as a GeoJSON coordinate array.
fn line_json(l: &[Pos]) -> Json {
    Json::Array(l.iter().map(pos_json).collect())
}

/// Rings (or lines) as nested GeoJSON coordinate arrays.
fn rings_json(r: &[Vec<Pos>]) -> Json {
    Json::Array(r.iter().map(|l| line_json(l)).collect())
}

struct Topology {
    /// The arcs, decoded to coordinates; `None` for one with a position that can't be read.
    arcs: Vec<Option<Vec<Pos>>>,
    transform: Option<([f64; 2], [f64; 2])>,
    budget: usize,
    /// Members of geometry collections left out, since the last object was read.
    members_lost: usize,
}

impl Topology {
    fn point(&self, p: &Json) -> Option<Pos> {
        let a = p.as_array()?;
        let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
        Some(match self.transform {
            Some(([sx, sy], [tx, ty])) => [x * sx + tx, y * sy + ty, extra(a)],
            None => [x, y, extra(a)],
        })
    }

    /// A line from arc indices (`~i` is arc `i` reversed), shared ends written once.
    fn line(&mut self, ids: &Json) -> Option<Vec<Pos>> {
        let mut out: Vec<Pos> = Vec::new();
        for id in ids.as_array()? {
            let i = id.as_i64()?;
            let (k, rev) = if i < 0 { (!i, true) } else { (i, false) };
            let arc = self.arcs.get(usize::try_from(k).ok()?)?.as_ref()?;
            self.budget = self.budget.checked_sub(arc.len())?;
            let pts: Vec<Pos> = if rev { arc.iter().rev().copied().collect() } else { arc.clone() };
            let mut pts = pts.into_iter();
            // The joint, written once, keeps a further number either arc gives it.
            if let Some(last) = out.last_mut()
                && let Some(first) = pts.next()
                && last[2].is_nan()
            {
                last[2] = first[2];
            }
            out.extend(pts);
        }
        Some(out)
    }

    fn rings(&mut self, v: &Json) -> Option<Vec<Vec<Pos>>> {
        v.as_array()?.iter().map(|r| self.line(r)).collect()
    }

    /// A TopoJSON geometry as GeoJSON (`None` when it can't be read).
    fn geometry(&mut self, g: &Json, depth: usize) -> Option<Json> {
        let ty = g.get("type")?.as_str()?;
        let arcs = g.get("arcs");
        let coords = g.get("coordinates");
        Some(match ty {
            "Point" => json!({"type": ty, "coordinates": pos_json(&self.point(coords?)?)}),
            "MultiPoint" => {
                let pts: Option<Vec<Pos>> = coords?.as_array()?.iter().map(|p| self.point(p)).collect();
                json!({"type": ty, "coordinates": line_json(&pts?)})
            }
            "LineString" => json!({"type": ty, "coordinates": line_json(&self.line(arcs?)?)}),
            "MultiLineString" | "Polygon" => json!({"type": ty, "coordinates": rings_json(&self.rings(arcs?)?)}),
            "MultiPolygon" => {
                let polys: Option<Vec<Vec<Vec<Pos>>>> = arcs?.as_array()?.iter().map(|p| self.rings(p)).collect();
                json!({"type": ty, "coordinates": polys?.iter().map(|p| rings_json(p)).collect::<Vec<_>>()})
            }
            "GeometryCollection" if depth < 4 => {
                let members = g.get("geometries")?.as_array()?;
                let gs: Vec<Json> = members.iter().filter_map(|x| self.geometry(x, depth + 1)).collect();
                self.members_lost += members.len().saturating_sub(gs.len());
                json!({"type": ty, "geometries": gs})
            }
            _ => return None,
        })
    }
}

/// Reads a TopoJSON topology into a layer per object.
pub fn read(file_name: &str, text: &str) -> Result<Vec<(Dataset, ReadReport)>> {
    let stem = std::path::Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("topology");
    let stem = stem.strip_suffix(".topo").unwrap_or(stem);
    let v: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| DataError::Format(format!("{file_name}: {e}")))?;
    if v.get("type").and_then(Json::as_str) != Some("Topology") {
        return Err(DataError::Format(format!("{file_name}: not a TopoJSON topology")));
    }
    let transform = v.get("transform").and_then(|t| {
        let s = t.get("scale")?.as_array()?;
        let tr = t.get("translate")?.as_array()?;
        Some(([s.first()?.as_f64()?, s.get(1)?.as_f64()?], [tr.first()?.as_f64()?, tr.get(1)?.as_f64()?]))
    });
    // Arcs: x and y delta-encoded integers with a transform, plain coordinates without; a further
    // number is copied as is.
    let mut total = 0usize;
    let mut arcs = Vec::new();
    for a in v.get("arcs").and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default() {
        let positions = a.as_array().map(Vec::as_slice).unwrap_or_default();
        total += positions.len();
        if total > MAX_COORDS {
            return Err(DataError::Format(format!("{file_name}: too many arc coordinates")));
        }
        let (mut x, mut y) = (0.0f64, 0.0f64);
        let pts: Option<Vec<Pos>> = positions
            .iter()
            .map(|p| {
                let a = p.as_array()?;
                let (px, py) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
                Some(match transform {
                    Some(([sx, sy], [tx, ty])) => {
                        x += px;
                        y += py;
                        [x * sx + tx, y * sy + ty, extra(a)]
                    }
                    None => [px, py, extra(a)],
                })
            })
            .collect();
        // An arc that isn't an array of positions can't be read either.
        arcs.push(pts.filter(|_| a.is_array()));
    }
    let mut topo = Topology { arcs, transform, budget: MAX_COORDS, members_lost: 0 };
    let objects = v.get("objects").and_then(Json::as_object).ok_or_else(|| DataError::Format(format!("{file_name}: no objects")))?;
    let mut out = Vec::new();
    for (name, obj) in objects.iter().take(MAX_OBJECTS) {
        let members: Vec<&Json> = match obj.get("type").and_then(Json::as_str) {
            Some("GeometryCollection") => obj.get("geometries").and_then(Json::as_array).map(|g| g.iter().collect()).unwrap_or_default(),
            _ => vec![obj],
        };
        let mut bad = 0usize;
        topo.members_lost = 0;
        let features: Vec<Json> = members
            .into_iter()
            .map(|g| {
                let geometry = if g.get("type").and_then(Json::as_str).is_some_and(|t| t != "null") {
                    topo.geometry(g, 0).unwrap_or_else(|| {
                        bad += 1;
                        Json::Null
                    })
                } else {
                    Json::Null
                };
                let mut props = g.get("properties").and_then(Json::as_object).cloned().unwrap_or_else(Map::new);
                if let Some(id) = g.get("id")
                    && !props.contains_key("id")
                {
                    props.insert("id".into(), id.clone());
                }
                json!({"type": "Feature", "properties": props, "geometry": geometry})
            })
            .collect();
        let fc = json!({"type": "FeatureCollection", "features": features});
        let title = if objects.len() == 1 { stem.to_string() } else { format!("{stem} {name}") };
        let (ds, mut rep) = crate::io::read_geojson(&fc.to_string(), &title)?;
        if bad > 0 {
            rep.skipped += bad;
            rep.messages.push(format!("{bad} geometries had arcs that couldn't be read"));
        }
        if topo.members_lost > 0 {
            rep.messages.push(format!(
                "{} members of geometry collections couldn't be read (an arc that couldn't be read, or nested too deep) and were left out",
                topo.members_lost
            ));
        }
        out.push((ds, rep));
    }
    if objects.len() > MAX_OBJECTS
        && let Some((_, rep)) = out.first_mut()
    {
        rep.messages.push(format!("only the first {MAX_OBJECTS} objects are read"));
    }
    if out.is_empty() {
        return Err(DataError::Format(format!("{file_name}: no objects")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Geometry, GeometryType};
    use rustgis_geom::Coord;

    // The TopoJSON specification's example: a point, a line and a polygon, quantized.
    const TOPO: &str = r#"{"type":"Topology","transform":{"scale":[0.0005000500050005,0.00010001000100010001],"translate":[100,0]},
"objects":{"example":{"type":"GeometryCollection","geometries":[
{"type":"Point","properties":{"prop0":"value0"},"coordinates":[4000,5000]},
{"type":"LineString","properties":{"prop0":"value0","prop1":0},"arcs":[0]},
{"type":"Polygon","properties":{"prop0":"value0","prop1":{"this":"that"}},"arcs":[[-2]]}]}},
"arcs":[[[4000,0],[1999,9999],[2000,-9999],[2000,9999]],[[0,0],[0,9999],[2000,0],[0,-9999],[-2000,0]]]}"#;

    #[test]
    fn reads_the_specification_example() {
        assert!(looks_like_topojson(TOPO));
        assert!(!looks_like_topojson(r#"{"type": "Feature", "properties": {"kind": "Topology"}}"#));
        // A multi-byte character across byte 512.
        let padded = format!("{{\"type\": \"Topology\", \"x\": \"{}é\"}}", "a".repeat(480));
        assert!(looks_like_topojson(&padded));
        let layers = read("example.topojson", TOPO).unwrap();
        assert_eq!(layers.len(), 1);
        let ds = &layers[0].0;
        assert_eq!(ds.name, "example");
        assert_eq!(ds.features.len(), 3);
        let Some(Geometry::Point(p)) = &ds.features[0].geometry else { panic!() };
        assert!((p.x - 102.0).abs() < 1e-3 && (p.y - 0.5).abs() < 1e-3, "{p:?}");
        let Some(Geometry::Line(l)) = &ds.features[1].geometry else { panic!() };
        assert_eq!(l[0].len(), 4);
        assert!((l[0][3].x - 105.0).abs() < 1e-3);
        // The polygon is arc 1 reversed: still a closed ring of 5.
        let Some(Geometry::Polygon(poly)) = &ds.features[2].geometry else { panic!() };
        assert_eq!(poly[0][0].len(), 5);
        assert_eq!(poly[0][0].first(), poly[0][0].last());
        assert!(poly[0][0].iter().all(|c: &Coord| (100.0 - 1e-3..=101.0 + 1e-3).contains(&c.x)), "{:?}", poly[0][0]);
        assert_ne!(ds.geometry_type, GeometryType::None);
    }

    #[test]
    fn json_topologies_read_every_object() {
        let two = r#"{"type":"Topology","objects":{"a":{"type":"Point","coordinates":[1,2]},"b":{"type":"Point","coordinates":[3,4]}},"arcs":[]}"#;
        let layers = crate::io::read_layers("us.json", two.as_bytes()).unwrap();
        assert_eq!(layers.len(), 2);
        // The first alone says what else the file holds.
        let (ds, rep) = crate::io::read_bytes("us.json", two.as_bytes()).unwrap();
        assert_eq!(ds.features.len(), 1);
        assert!(rep.messages.iter().any(|m| m.contains("also holds")), "{:?}", rep.messages);
    }

    /// What reading `text` says of dropped heights, and whether the layer is marked as having lost
    /// them.
    fn zm_said(text: &str) -> (bool, Vec<String>) {
        let (ds, rep) = read("z.topojson", text).unwrap().remove(0);
        (ds.zm_dropped, rep.messages)
    }

    #[test]
    fn heights_dropped_are_said() {
        // The specification's example says nothing: it has no heights.
        let (lost, said) = zm_said(TOPO);
        assert!(!lost && said.is_empty(), "{said:?}");

        // A quantized arc with a height on one position; the point and the line use it, the
        // polygon doesn't. The height is copied, not delta-decoded or scaled.
        let quantized = r#"{"type":"Topology","transform":{"scale":[1,1],"translate":[0,0]},
"objects":{"o":{"type":"GeometryCollection","geometries":[
{"type":"Point","coordinates":[1,2,30]},
{"type":"LineString","arcs":[0]},
{"type":"Polygon","arcs":[[1]]}]}},
"arcs":[[[0,0],[1,1,12.5]],[[0,0],[0,1],[1,0],[-1,-1]]]}"#;
        let (lost, said) = zm_said(quantized);
        assert!(lost && said == [crate::zm_note(Some(2))], "{said:?}");

        // Without a transform: a height on a plain arc, read through a reversed index.
        let plain = r#"{"type":"Topology","objects":{"o":{"type":"LineString","arcs":[-1]}},
"arcs":[[[10,10,100],[11,11,101]]]}"#;
        let (lost, said) = zm_said(plain);
        assert!(lost && said == [crate::zm_note(Some(1))], "{said:?}");
        // The x and y read are unchanged by the height.
        let (ds, _) = read("z.topojson", plain).unwrap().remove(0);
        let Some(Geometry::Line(l)) = &ds.features[0].geometry else { panic!() };
        assert_eq!((l[0][0].x, l[0][0].y, l[0][1].x), (11.0, 11.0, 10.0));

        // A measure alone (the third value null, a fourth a number) is dropped too.
        let measure = r#"{"type":"Topology","objects":{"o":{"type":"MultiPoint","coordinates":[[1,2,null,7],[3,4]]}},"arcs":[]}"#;
        let (lost, said) = zm_said(measure);
        assert!(lost && said == [crate::zm_note(Some(1))], "{said:?}");

        // A null third value is no height.
        let null = r#"{"type":"Topology","objects":{"o":{"type":"Point","coordinates":[1,2,null]}},"arcs":[]}"#;
        let (lost, said) = zm_said(null);
        assert!(!lost && said.is_empty(), "{said:?}");
    }

    #[test]
    fn a_height_on_a_shared_joint_is_kept() {
        // Only the second arc's first position, the joint written once, has a height.
        let text = r#"{"type":"Topology","objects":{"o":{"type":"LineString","arcs":[0,1]}},
"arcs":[[[0,0],[1,1]],[[1,1,5],[2,2]]]}"#;
        let (lost, said) = zm_said(text);
        assert!(lost && said == [crate::zm_note(Some(1))], "{said:?}");
    }

    #[test]
    fn an_arc_with_an_unreadable_position_is_not_read() {
        // Quantized: leaving out the bad delta would move (1,1) to where (x+1, 6) should be.
        let quantized = r#"{"type":"Topology","transform":{"scale":[1,1],"translate":[0,0]},
"objects":{"o":{"type":"GeometryCollection","geometries":[{"type":"LineString","arcs":[0]},{"type":"LineString","arcs":[1]}]}},
"arcs":[[[0,0],["x",5],[1,1]],[[0,0],[1,1]]]}"#;
        let (ds, rep) = read("q.topojson", quantized).unwrap().remove(0);
        assert_eq!(rep.skipped, 1);
        assert!(rep.messages.iter().any(|m| m.contains("arcs that couldn't be read")), "{:?}", rep.messages);
        assert!(ds.features[0].geometry.is_none());
        let Some(Geometry::Line(l)) = &ds.features[1].geometry else { panic!() };
        assert_eq!((l[0][1].x, l[0][1].y), (1.0, 1.0));
        // Plain coordinates, and an arc that isn't an array: the same.
        for arcs in [r#"[[[0,0],[null,1],[2,2]]]"#, r#"[5]"#] {
            let text = format!(r#"{{"type":"Topology","objects":{{"o":{{"type":"LineString","arcs":[0]}}}},"arcs":{arcs}}}"#);
            let (_, rep) = read("p.topojson", &text).unwrap().remove(0);
            assert_eq!(rep.skipped, 1, "{arcs}");
        }
    }

    #[test]
    fn collection_members_left_out_are_said() {
        let text = r#"{"type":"Topology","objects":{"o":{"type":"GeometryCollection","geometries":[
{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]},{"type":"LineString","arcs":[9]}]}]}},"arcs":[]}"#;
        let (_, rep) = read("c.topojson", text).unwrap().remove(0);
        assert!(rep.messages.iter().any(|m| m.starts_with("1 members of geometry collections")), "{:?}", rep.messages);
        // Counted for each object: a second object without losses says nothing of them.
        let two = r#"{"type":"Topology","objects":{
"a":{"type":"GeometryCollection","geometries":[{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]},{"type":"LineString","arcs":[9]}]}]},
"b":{"type":"Point","coordinates":[3,4]}},"arcs":[]}"#;
        let layers = read("c.topojson", two).unwrap();
        assert!(layers[1].1.messages.iter().all(|m| !m.contains("members")), "{:?}", layers[1].1.messages);
    }

    #[test]
    fn a_geometry_not_read_isnt_counted_as_dropping_heights() {
        // The first geometry's arc doesn't exist; the second has a height.
        let text = r#"{"type":"Topology","objects":{"o":{"type":"GeometryCollection","geometries":[
{"type":"LineString","arcs":[5]},
{"type":"Point","coordinates":[1,2,3]}]}},"arcs":[[[0,0,9],[1,1,9]]]}"#;
        let (ds, rep) = read("z.topojson", text).unwrap().remove(0);
        assert_eq!(rep.skipped, 1);
        assert!(ds.zm_dropped && rep.messages.contains(&crate::zm_note(Some(1))), "{:?}", rep.messages);
    }

    #[test]
    fn junk_is_refused() {
        assert!(read("x.topojson", "{}").is_err());
        assert!(read("x.topojson", "not json").is_err());
        let bad_arc = r#"{"type":"Topology","objects":{"a":{"type":"LineString","arcs":[7]}},"arcs":[]}"#;
        let (_, rep) = read("x.topojson", bad_arc).unwrap().remove(0);
        assert_eq!(rep.skipped, 1);
        for cut in (0..TOPO.len()).step_by(11) {
            let _ = read("x.topojson", TOPO.get(..cut).unwrap_or(""));
        }
    }
}
