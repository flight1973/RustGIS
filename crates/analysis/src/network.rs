//! Vector workflows: shortest routes over a line network, random points, field profiles,
//! attribute similarity and neighbourhood summaries.

use std::collections::{BinaryHeap, HashMap};

use rustgis_data::{Dataset, Geometry};
use rustgis_geom::{Coord, haversine_m};

/// A shortest route: its path, length in metres, cost, and the line rows it uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub path: Vec<Coord>,
    pub length_m: f64,
    pub cost: f64,
    pub rows: Vec<usize>,
}

#[derive(PartialEq)]
struct State(f64, usize);
impl Eq for State {}
impl PartialOrd for State {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for State {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Nodes join where line vertices coincide (to about a centimetre).
fn key(c: Coord) -> (i64, i64) {
    ((c.x * 1e7).round() as i64, (c.y * 1e7).round() as i64)
}

/// The shortest route between two points over the lines of `rows`, joining where vertices
/// coincide. Each edge costs its length times the row's `cost_field` value (1 when empty; lines
/// costing zero or less are closed). Each
/// end snaps to the nearest point on a line (within `snap_m`), where the route starts or stops.
pub fn shortest_route(
    ds: &Dataset,
    rows: &[usize],
    from: Coord,
    to: Coord,
    cost_field: Option<usize>,
    snap_m: f64,
) -> Result<Route, String> {
    let mut ids: HashMap<(i64, i64), usize> = HashMap::new();
    let mut nodes: Vec<Coord> = Vec::new();
    let mut adj: Vec<Vec<(usize, f64, f64, usize)>> = Vec::new(); // (to, cost, length, row)
    let mut segs: Vec<(usize, usize, f64, usize)> = Vec::new(); // (from, to, weight, row)
    let node = |c: Coord, ids: &mut HashMap<(i64, i64), usize>, nodes: &mut Vec<Coord>, adj: &mut Vec<Vec<(usize, f64, f64, usize)>>| {
        *ids.entry(key(c)).or_insert_with(|| {
            nodes.push(c);
            adj.push(Vec::new());
            nodes.len() - 1
        })
    };
    for r in rows {
        let Some(Geometry::Line(parts)) = ds.features.get(*r).and_then(|f| f.geometry.as_ref()) else { continue };
        // A cost multiplies each edge's length; no value means 1, and zero or less closes the
        // line (impassable).
        let weight = match cost_field.map(|fi| ds.value(*r, fi).as_f64()) {
            Some(Some(w)) if w.is_finite() && w > 0.0 => w,
            Some(Some(_)) => continue,
            _ => 1.0,
        };
        for part in parts {
            for w in part.windows(2) {
                let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
                let (ia, ib) = (node(*a, &mut ids, &mut nodes, &mut adj), node(*b, &mut ids, &mut nodes, &mut adj));
                let len = haversine_m(*a, *b);
                if let Some(v) = adj.get_mut(ia) {
                    v.push((ib, len * weight, len, *r));
                }
                if let Some(v) = adj.get_mut(ib) {
                    v.push((ia, len * weight, len, *r));
                }
                segs.push((ia, ib, weight, *r));
                // Checked per segment, so one huge feature can't grow the network unbounded.
                if segs.len() > 10_000_000 || nodes.len() > 5_000_000 {
                    return Err("the network is too large".into());
                }
            }
        }
    }
    // The closest point on any segment: (segment, fraction along it, the point, metres away).
    let nearest = |p: Coord, nodes: &[Coord]| -> Option<(usize, f64, Coord, f64)> {
        let k = p.y.to_radians().cos().max(0.01);
        segs.iter()
            .enumerate()
            .filter_map(|(i, (a, b, _, _))| {
                let (a, b) = (*nodes.get(*a)?, *nodes.get(*b)?);
                let (dx, dy) = ((b.x - a.x) * k, b.y - a.y);
                let len2 = dx * dx + dy * dy;
                let t = if len2 > 0.0 { (((p.x - a.x) * k * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let q = Coord::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
                Some((i, t, q, haversine_m(q, p)))
            })
            .min_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
    };
    // Each end becomes a node on its segment, joined to the segment's two ends.
    let attach = |p: Coord, nodes: &mut Vec<Coord>, adj: &mut Vec<Vec<(usize, f64, f64, usize)>>| -> Result<(usize, usize, f64), String> {
        let (si, t, q, d) = nearest(p, nodes).ok_or("the layer has no lines")?;
        if d > snap_m {
            return Err(format!("a route end is more than {} from the network", rustgis_geom::format_distance(snap_m)));
        }
        let &(a, b, weight, row) = segs.get(si).ok_or("the layer has no lines")?;
        if t <= 0.0 {
            return Ok((a, si, t));
        }
        if t >= 1.0 {
            return Ok((b, si, t));
        }
        nodes.push(q);
        adj.push(Vec::new());
        let n = nodes.len() - 1;
        for end in [a, b] {
            let len = haversine_m(q, nodes.get(end).copied().unwrap_or(q));
            if let Some(v) = adj.get_mut(n) {
                v.push((end, len * weight, len, row));
            }
            if let Some(v) = adj.get_mut(end) {
                v.push((n, len * weight, len, row));
            }
        }
        Ok((n, si, t))
    };
    let (s, s_seg, s_t) = attach(from, &mut nodes, &mut adj)?;
    let (t, t_seg, t_t) = attach(to, &mut nodes, &mut adj)?;
    // Both ends on one segment: they're joined directly along it.
    if s_seg == t_seg && s != t && (0.0..=1.0).contains(&s_t) && (0.0..=1.0).contains(&t_t) {
        let (Some(&(_, _, weight, row)), Some(a), Some(b)) = (segs.get(s_seg), nodes.get(s).copied(), nodes.get(t).copied()) else {
            return Err("the layer has no lines".into());
        };
        let len = haversine_m(a, b);
        if let Some(v) = adj.get_mut(s) {
            v.push((t, len * weight, len, row));
        }
        if let Some(v) = adj.get_mut(t) {
            v.push((s, len * weight, len, row));
        }
    }
    let n = nodes.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(usize, f64, usize)>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    if let Some(d) = dist.get_mut(s) {
        *d = 0.0;
    }
    heap.push(State(0.0, s));
    while let Some(State(d, u)) = heap.pop() {
        if u == t {
            break;
        }
        if d > dist.get(u).copied().unwrap_or(f64::INFINITY) {
            continue;
        }
        for &(v, c, len, row) in adj.get(u).map(Vec::as_slice).unwrap_or(&[]) {
            let nd = d + c;
            if nd < dist.get(v).copied().unwrap_or(f64::INFINITY) {
                if let Some(x) = dist.get_mut(v) {
                    *x = nd;
                }
                if let Some(x) = prev.get_mut(v) {
                    *x = Some((u, len, row));
                }
                heap.push(State(nd, v));
            }
        }
    }
    let cost = dist.get(t).copied().unwrap_or(f64::INFINITY);
    if !cost.is_finite() {
        return Err("the two points aren't connected by the network".into());
    }
    let mut path = Vec::new();
    let mut length = 0.0;
    let mut used = Vec::new();
    let mut at = t;
    path.push(nodes.get(t).copied().unwrap_or(to));
    while let Some(Some((p, len, row))) = prev.get(at) {
        path.push(nodes.get(*p).copied().unwrap_or(from));
        length += len;
        if used.last() != Some(row) {
            used.push(*row);
        }
        at = *p;
    }
    path.reverse();
    used.reverse();
    used.dedup();
    Ok(Route { path, length_m: length, cost, rows: used })
}

/// Most random points made in one call, and point-in-polygon tries spent making them.
pub const MAX_POINTS: usize = 1_000_000;
const MAX_TRIES: usize = 50_000_000;

/// Points spread at random (but reproducibly, from `seed`) inside polygons: `per` in each, up to
/// [`MAX_POINTS`] in all; thin polygons may get fewer (tries are bounded too).
pub fn random_points(ds: &Dataset, rows: &[usize], per: usize, seed: u64) -> Vec<(usize, Coord)> {
    let mut budget = MAX_TRIES;
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407) | 1;
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut out = Vec::new();
    for r in rows {
        let Some(Geometry::Polygon(polys)) = ds.features.get(*r).and_then(|f| f.geometry.as_ref()) else { continue };
        let e = ds.features.get(*r).and_then(|f| f.geometry.as_ref()).map(|g| g.extent()).unwrap_or_default();
        let mut placed = 0;
        let mut tries = 0usize;
        while placed < per && tries < per.saturating_mul(200) && budget > 0 && out.len() < MAX_POINTS {
            tries += 1;
            budget -= 1;
            let c = Coord::new(e.min_x + rnd() * e.width(), e.min_y + rnd() * e.height());
            if polys.iter().any(|rings| rustgis_data::geometry::polygon_contains(rings, c)) {
                out.push((*r, c));
                placed += 1;
            }
        }
    }
    out
}

/// A summary of one field: (name, type, count, nulls, distinct, min, max, mean, top values).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldProfile {
    pub name: String,
    pub ty: String,
    pub count: usize,
    pub nulls: usize,
    /// Distinct values, counted until 100,000 are seen (`truncated` says counting stopped there,
    /// so `distinct` and `top` cover only the rows before).
    pub distinct: usize,
    pub truncated: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
    pub top: Vec<(String, usize)>,
}

/// Profiles every field of a dataset.
pub fn profile_fields(ds: &Dataset) -> Vec<FieldProfile> {
    ds.fields
        .iter()
        .enumerate()
        .map(|(fi, f)| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            let (mut nulls, mut sum, mut n_num) = (0usize, 0.0, 0usize);
            let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
            let mut truncated = false;
            for r in 0..ds.features.len() {
                let v = ds.value(r, fi);
                if v.is_null() {
                    nulls += 1;
                    continue;
                }
                if let Some(x) = v.as_f64().filter(|_| f.ty.is_numeric()) {
                    sum += x;
                    n_num += 1;
                    min = min.min(x);
                    max = max.max(x);
                }
                // Past the cap, counting stops (no text is made for the remaining rows).
                if truncated {
                    continue;
                }
                if counts.len() >= 100_000 {
                    truncated = true;
                    continue;
                }
                *counts.entry(v.to_string()).or_insert(0) += 1;
            }
            let mut top: Vec<(String, usize)> = counts.iter().map(|(k, v)| (k.clone(), *v)).collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            top.truncate(5);
            FieldProfile {
                name: f.name.clone(),
                ty: f.ty.label().to_string(),
                count: ds.features.len(),
                nulls,
                distinct: counts.len(),
                truncated,
                min: (n_num > 0).then_some(min),
                max: (n_num > 0).then_some(max),
                mean: (n_num > 0).then(|| sum / n_num as f64),
                top,
            }
        })
        .collect()
}

/// Rows ranked by how close their numeric attributes are to a row's (z-scored Euclidean
/// distance over `fields`), nearest first, excluding the row itself.
pub fn similar(ds: &Dataset, row: usize, fields: &[usize], n: usize) -> Vec<(usize, f64)> {
    let stats: Vec<(f64, f64)> = fields
        .iter()
        .map(|fi| {
            let vals: Vec<f64> = (0..ds.features.len()).filter_map(|r| ds.value(r, *fi).as_f64()).filter(|v| v.is_finite()).collect();
            let m = vals.iter().sum::<f64>() / vals.len().max(1) as f64;
            let sd = (vals.iter().map(|v| (v - m).powi(2)).sum::<f64>() / vals.len().max(1) as f64).sqrt();
            (m, if sd > 0.0 { sd } else { 1.0 })
        })
        .collect();
    let vec_of = |r: usize| -> Option<Vec<f64>> {
        fields.iter().zip(&stats).map(|(fi, (m, sd))| ds.value(r, *fi).as_f64().filter(|v| v.is_finite()).map(|v| (v - m) / sd)).collect()
    };
    let Some(target) = vec_of(row) else { return Vec::new() };
    let mut out: Vec<(usize, f64)> = (0..ds.features.len())
        .filter(|r| *r != row)
        .filter_map(|r| vec_of(r).map(|v| (r, v.iter().zip(&target).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt())))
        .collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(n);
    out
}

fn radius_scale(at: Coord) -> f64 {
    1.0 / at.y.to_radians().cos().max(0.01)
}

fn radius_coord_supported(point: Coord) -> bool {
    point.is_finite() && (-180.0..=180.0).contains(&point.x) && (-rustgis_geom::MAX_LAT..=rustgis_geom::MAX_LAT).contains(&point.y)
}

/// Local Mercator distance in metres, using the same centre-latitude correction as [`within`].
///
/// This is not a geodesic distance and does not include `within`'s legacy geographic
/// bounding-box prefilter. Inputs must be canonical lon/lat within the Mercator domain.
pub fn radius_distance_m(at: Coord, point: Coord) -> Option<f64> {
    if !radius_coord_supported(at) || !radius_coord_supported(point) {
        return None;
    }
    let center = rustgis_geom::lonlat_to_merc(at);
    let point = rustgis_geom::lonlat_to_merc(point);
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    let distance = (dx.powi(2) + dy.powi(2)).sqrt() / radius_scale(at);
    distance.is_finite().then_some(distance)
}

/// A point on the local distance-threshold outline, at an angle counterclockwise from east.
///
/// Radii must be positive and at most 5,000 km. Points outside the canonical Mercator
/// domain are unsupported rather than wrapped or clamped. The outline is approximate:
/// [`within`] additionally preserves its historical geographic bounding-box prefilter.
pub fn radius_boundary_point(at: Coord, radius_m: f64, angle_rad: f64) -> Option<Coord> {
    if !radius_coord_supported(at) || !radius_m.is_finite() || radius_m <= 0.0 || radius_m > 5_000_000.0 || !angle_rad.is_finite() {
        return None;
    }
    let center = rustgis_geom::lonlat_to_merc(at);
    let radius = radius_m * radius_scale(at);
    let (sin, cos) = angle_rad.sin_cos();
    let point = Coord::new(center.x + radius * cos, center.y + radius * sin);
    let point = rustgis_geom::merc_to_lonlat(point);
    radius_coord_supported(point).then_some(point)
}

/// Rows of `ds` whose geometry comes within `radius_m` of a point, with their distances.
pub fn within(ds: &Dataset, rows: &[usize], at: Coord, radius_m: f64) -> Vec<(usize, f64)> {
    use rustgis_geom::MAX_LAT;
    let m = rustgis_geom::lonlat_to_merc(at);
    let k = radius_scale(at);
    // Web Mercator ends at 85.05°: where the point or a shape is beyond it, the distance is
    // measured on the ground, in an azimuthal equidistant plane centered on the point (#1033).
    let ground = rustgis_geom::Aeqd::new(at);
    let at_box = rustgis_geom::Extent::new(at.x, at.y, at.x, at.y);
    let mut out: Vec<(usize, f64)> = rows
        .iter()
        .filter_map(|r| {
            let g = ds.features.get(*r)?.geometry.as_ref()?;
            // Skip shapes whose box is farther than the radius before projecting them.
            let e = g.extent();
            let dlat = radius_m / 111_320.0;
            if at.y.abs() > MAX_LAT || e.min_y < -MAX_LAT || e.max_y > MAX_LAT {
                // Near a pole, longitudes converge: the join's bounds allow for it.
                if !crate::join::may_be_within(&at_box, &e, radius_m) {
                    return None;
                }
                let d = polar_reach(g, at, &ground);
                return (d <= radius_m).then_some((*r, d));
            }
            let dlon = dlat * k;
            if at.x < e.min_x - dlon || at.x > e.max_x + dlon || at.y < e.min_y - dlat || at.y > e.max_y + dlat {
                return None;
            }
            let pg = g.map_coords(&rustgis_geom::lonlat_to_merc);
            let d = if pg.hit(m, 0.0) { 0.0 } else { pg_dist(&pg, m) / k };
            (d <= radius_m).then_some((*r, d))
        })
        .collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The longest step, in degrees of longitude or latitude, that [`polar_reach`] follows an edge by.
const REACH_STEP_DEG: f64 = 0.5;
/// The most steps it takes along one edge (a hostile edge thousands of degrees long included).
const REACH_STEPS_MAX: f64 = 1024.0;

/// How far `g` comes from `at` on the ground (meters), as [`within`] measures it near a pole: zero
/// when `at` lies on or in it in the analysis plane, as the other tools see it; otherwise the least
/// distance to its edges, each followed as the plane draws it (straight in the plane: in longitude
/// and latitude beyond 85.05°) in steps of at most half a degree, in `ground`, an azimuthal
/// equidistant plane at `at`. Its vertices alone would draw a long edge as a chord, or one along a
/// parallel from −180° to 180° as a point.
fn polar_reach(g: &Geometry, at: Coord, ground: &rustgis_geom::Aeqd) -> f64 {
    use rustgis_geom::{lonlat_to_plane, plane_to_lonlat};
    if g.map_coords(&lonlat_to_plane).hit(lonlat_to_plane(at), 0.0) {
        return 0.0;
    }
    let origin = Coord::new(0.0, 0.0);
    let mut best = f64::INFINITY;
    let mut edge = |a: Coord, b: Coord| {
        let span = (b.x - a.x).abs().max((b.y - a.y).abs()) / REACH_STEP_DEG;
        // NaN (a coordinate missing) takes one step.
        let n = if span > 1.0 { span.ceil().min(REACH_STEPS_MAX) } else { 1.0 };
        let (pa, pb) = (lonlat_to_plane(a), lonlat_to_plane(b));
        let mut last = ground.forward(a);
        let mut k = 1.0;
        while k <= n {
            let t = k / n;
            let q = if k >= n { b } else { plane_to_lonlat(Coord::new(pa.x + (pb.x - pa.x) * t, pa.y + (pb.y - pa.y) * t)) };
            let next = ground.forward(q);
            best = best.min(rustgis_data::geometry::seg_dist2(&[last, next], origin));
            last = next;
            k += 1.0;
        }
    };
    let mut path = |pts: &[Coord]| match pts {
        [only] => edge(*only, *only),
        _ => pts.windows(2).for_each(|w| {
            if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                edge(*a, *b);
            }
        }),
    };
    match g {
        Geometry::Point(c) => path(std::slice::from_ref(c)),
        Geometry::MultiPoint(v) => v.iter().for_each(|c| path(std::slice::from_ref(c))),
        Geometry::Line(parts) => parts.iter().for_each(|p| path(p)),
        Geometry::Polygon(polys) => polys.iter().flatten().for_each(|r| path(r)),
    }
    best.sqrt()
}

fn pg_dist(g: &Geometry, p: Coord) -> f64 {
    let d2 = match g {
        Geometry::Point(c) => (c.x - p.x).powi(2) + (c.y - p.y).powi(2),
        Geometry::MultiPoint(v) => v.iter().map(|c| (c.x - p.x).powi(2) + (c.y - p.y).powi(2)).fold(f64::INFINITY, f64::min),
        Geometry::Line(parts) => parts.iter().map(|l| rustgis_data::geometry::seg_dist2(l, p)).fold(f64::INFINITY, f64::min),
        Geometry::Polygon(polys) => polys.iter().flatten().map(|r| rustgis_data::geometry::seg_dist2(r, p)).fold(f64::INFINITY, f64::min),
    };
    d2.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustgis_data::{Field, FieldType, GeometryType, Value};

    #[test]
    fn routes_points_profiles_and_similarity() {
        // A square of roads with a slow diagonal.
        let mut ds = Dataset::new("roads", GeometryType::Polyline, vec![Field::new("cost", FieldType::Double)]);
        let l = |a: (f64, f64), b: (f64, f64)| Some(Geometry::Line(vec![vec![Coord::new(a.0, a.1), Coord::new(b.0, b.1)]]));
        ds.push(l((0.0, 0.0), (1.0, 0.0)), vec![Value::Float(1.0)]);
        ds.push(l((1.0, 0.0), (1.0, 1.0)), vec![Value::Float(1.0)]);
        ds.push(l((0.0, 0.0), (1.0, 1.0)), vec![Value::Float(5.0)]);
        let rows = [0, 1, 2];
        let short = shortest_route(&ds, &rows, Coord::new(0.0, 0.0), Coord::new(1.0, 1.0), None, 1000.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(short.rows, vec![2], "by length the diagonal wins");
        let cheap =
            shortest_route(&ds, &rows, Coord::new(0.0, 0.0), Coord::new(1.0, 1.0), Some(0), 1000.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cheap.rows, vec![0, 1], "by cost the two sides win");
        assert!(shortest_route(&ds, &rows, Coord::new(5.0, 5.0), Coord::new(1.0, 1.0), None, 1000.0).is_err());
        let mut poly = Dataset::new("p", GeometryType::Polygon, vec![]);
        poly.push(
            Some(Geometry::Polygon(vec![vec![vec![
                Coord::new(0.0, 0.0),
                Coord::new(1.0, 0.0),
                Coord::new(1.0, 1.0),
                Coord::new(0.0, 1.0),
                Coord::new(0.0, 0.0),
            ]]])),
            vec![],
        );
        let pts = random_points(&poly, &[0], 25, 7);
        assert_eq!(pts.len(), 25);
        assert!(pts.iter().all(|(_, c)| c.x > 0.0 && c.x < 1.0 && c.y > 0.0 && c.y < 1.0));
        assert_eq!(random_points(&poly, &[0], 25, 7), pts, "reproducible");
        let p = profile_fields(&ds);
        assert_eq!((p[0].min, p[0].max, p[0].distinct), (Some(1.0), Some(5.0), 2));
        let sim = similar(&ds, 0, &[0], 2);
        assert_eq!(sim[0].0, 1);
        let near = within(&ds, &rows, Coord::new(0.5, 0.001), 500.0);
        assert_eq!(near.first().map(|x| x.0), Some(0));
    }

    #[test]
    fn route_ends_snap_onto_long_segments() {
        // One straight road 10 km long, with vertices only at its ends.
        let mut ds = Dataset::new("roads", GeometryType::Polyline, vec![]);
        ds.push(Some(Geometry::Line(vec![vec![Coord::new(0.0, 0.0), Coord::new(0.09, 0.0)]])), vec![]);
        // Clicks beside its middle, far from both vertices but close to the road.
        let r = shortest_route(&ds, &[0], Coord::new(0.03, 0.0001), Coord::new(0.06, -0.0001), None, 50.0).unwrap();
        assert!((r.length_m - 3336.0).abs() < 20.0, "{}", r.length_m);
        assert!((r.path[0].x - 0.03).abs() < 1e-9 && (r.path.last().unwrap().x - 0.06).abs() < 1e-9);
        assert!(shortest_route(&ds, &[0], Coord::new(0.03, 0.01), Coord::new(0.06, 0.0), None, 50.0).is_err(), "1 km away");
    }

    fn offset(at: Coord, x: f64, y: f64) -> Coord {
        let m = rustgis_geom::lonlat_to_merc(at);
        let scale = radius_scale(at);
        rustgis_geom::merc_to_lonlat(Coord::new(m.x + x * scale, m.y + y * scale))
    }

    #[test]
    fn radius_helpers_match_local_mercator_metric() {
        let at = Coord::new(0.0, 60.0);
        let m = rustgis_geom::lonlat_to_merc(at);
        let east = rustgis_geom::merc_to_lonlat(Coord::new(m.x + 2000.0, m.y));
        assert!((radius_distance_m(at, east).unwrap() - 1000.0).abs() < 1e-8);
        assert_eq!(radius_distance_m(at, at), Some(0.0));
        let boundary = radius_boundary_point(at, 1000.0, 0.0).unwrap();
        assert!((rustgis_geom::lonlat_to_merc(boundary).x - m.x - 2000.0).abs() < 1e-8);
        for latitude in [0.0, 60.0] {
            let center = Coord::new(3.0, latitude);
            for angle in [0.0, 0.5, 1.0, 2.0, 3.0, 4.5] {
                let point = radius_boundary_point(center, 1000.0, angle).unwrap();
                assert!((radius_distance_m(center, point).unwrap() - 1000.0).abs() < 1e-7);
            }
            let diagonal = offset(center, 600.0, 800.0);
            let radius = radius_distance_m(center, diagonal).unwrap();
            assert!((radius - 1000.0).abs() < 1e-7);
            let mut ds = Dataset::new("boundary", GeometryType::Point, vec![]);
            ds.push(Some(Geometry::Point(diagonal)), vec![]);
            assert_eq!(within(&ds, &[0], center, radius), vec![(0, radius)]);
        }
    }

    #[test]
    fn radius_helpers_reject_unsupported_inputs_without_clamping() {
        let center = Coord::new(0.0, 0.0);
        for invalid in [
            Coord::new(f64::NAN, 0.0),
            Coord::new(0.0, f64::INFINITY),
            Coord::new(181.0, 0.0),
            Coord::new(-181.0, 0.0),
            Coord::new(0.0, 86.0),
            Coord::new(0.0, -86.0),
        ] {
            assert!(radius_distance_m(center, invalid).is_none());
            assert!(radius_distance_m(invalid, center).is_none());
            assert!(radius_boundary_point(invalid, 1000.0, 0.0).is_none());
        }
        for radius in [0.0, -1.0, 5_000_001.0, f64::NAN, f64::INFINITY] {
            assert!(radius_boundary_point(center, radius, 0.0).is_none());
        }
        for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(radius_boundary_point(center, 1000.0, angle).is_none());
        }
        assert!(radius_boundary_point(Coord::new(179.999, 0.0), 1000.0, 0.0).is_none());
        assert!(radius_boundary_point(Coord::new(-179.999, 0.0), 1000.0, std::f64::consts::PI).is_none());
        assert!(radius_boundary_point(Coord::new(0.0, 85.05), 1000.0, std::f64::consts::FRAC_PI_2).is_none());
        assert!(radius_boundary_point(Coord::new(0.0, -85.05), 1000.0, -std::f64::consts::FRAC_PI_2).is_none());
        assert!(radius_boundary_point(center, 5_000_000.0, 0.0).is_some());
    }

    #[test]
    fn radius_geometry_distances_at_equator_and_high_latitude() {
        for latitude in [0.0, 60.0] {
            let center = Coord::new(0.0, latitude);
            let point = |x, y| offset(center, x, y);
            let square =
                |half: f64| vec![point(-half, -half), point(half, -half), point(half, half), point(-half, half), point(-half, -half)];
            let mut ds = Dataset::new("radius", GeometryType::Point, vec![]);
            for geometry in [
                Geometry::Point(center),
                Geometry::Point(point(300.0, 400.0)),
                Geometry::Point(point(660.0, 880.0)),
                Geometry::Line(vec![vec![point(-2000.0, 0.0), point(2000.0, 0.0)]]),
                Geometry::Polygon(vec![vec![square(2000.0)]]),
                Geometry::Polygon(vec![vec![square(2000.0), square(1500.0)]]),
                Geometry::MultiPoint(vec![point(4000.0, 4000.0), point(300.0, 400.0)]),
                Geometry::Line(vec![vec![point(-2000.0, 1500.0), point(2000.0, 1500.0)]]),
                Geometry::Polygon(vec![vec![vec![
                    point(600.0, 600.0),
                    point(800.0, 600.0),
                    point(800.0, 800.0),
                    point(600.0, 800.0),
                    point(600.0, 600.0),
                ]]]),
            ] {
                ds.push(Some(geometry), vec![]);
            }
            ds.push(None, vec![]);
            let rows: Vec<_> = (0..ds.features.len()).collect();
            let hits = within(&ds, &rows, center, 1000.0);
            let mut selected: Vec<_> = hits.iter().map(|hit| hit.0).collect();
            selected.sort_unstable();
            assert_eq!(selected, vec![0, 1, 3, 4, 6, 8]);
            for (row, distance) in hits {
                let expected = match row {
                    0 | 3 | 4 => 0.0,
                    1 | 6 => 500.0,
                    8 => (600.0_f64.powi(2) + 600.0_f64.powi(2)).sqrt(),
                    _ => panic!("unexpected radius hit"),
                };
                assert!((distance - expected).abs() < 1e-7, "{latitude}: {row}: {distance}");
            }
        }
    }

    #[test]
    fn radius_preserves_legacy_geographic_prefilter() {
        let center = Coord::new(0.0, 60.0);
        let point = Coord::new(0.0, 59.1);
        assert!(radius_distance_m(center, point).unwrap() < 100_000.0);
        let mut ds = Dataset::new("legacy prefilter", GeometryType::Point, vec![]);
        ds.push(Some(Geometry::Point(point)), vec![]);
        assert!(within(&ds, &[0], center, 100_000.0).is_empty());
    }
}
