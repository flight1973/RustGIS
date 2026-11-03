//! Thinned display copies of a layer's geometry, for drawing it zoomed out.
//!
//! A detailed layer seen whole puts many vertices in each pixel: walking them all on every frame
//! makes panning and zooming stall. A thinned copy keeps a vertex only when it is a set distance
//! from the last one kept, so the map draws from far fewer vertices and looks the same. Copies
//! are made per level (a tolerance of 2^level projection units) on first use and kept, a few
//! at a time; the renderer picks the level just under a device pixel at the view's zoom.

use rustgis_data::{Coord, Geometry};

/// The tolerance, in device pixels, a thinned copy keeps within.
pub const THIN_PX: f64 = 1.0;

/// Extrusion footprint tolerance in logical pixels, kept tighter than flat display thinning.
pub const EXTRUSION_THIN_PX: f64 = 0.3;

/// Layers with fewer line and ring vertices than this are always drawn from the full geometry.
pub const THIN_ABOVE: usize = 200_000;

/// A level is kept as a copy only when it drops at least this share of the vertices; otherwise
/// it draws from the full geometry (zoomed in past the vertex spacing, or many tiny features).
pub const MIN_SAVING: f64 = 0.25;

/// How many levels a layer keeps.
pub const LEVELS_KEPT: usize = 3;

/// Additional geometry retained for display levels per layer, excluding original/projected data.
pub const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Inputs of an unsuccessful display attempt, so unchanged frames do not repeat the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rejected {
    pub revision: u64,
    pub level: i32,
    pub rows: usize,
    pub budget: usize,
    pub whole_layer_eligible: bool,
}

#[cfg(test)]
thread_local! {
    /// Counts display construction on this test thread, including candidates later rejected.
    pub(crate) static DISPLAY_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One bounded display level, independent of the full geometry used for interaction.
#[derive(Debug)]
pub(crate) struct Display {
    pub revision: u64,
    pub level: i32,
    pub whole_layer_eligible: bool,
    pub geometries: Vec<Option<Geometry>>,
    pub extents: Vec<rustgis_geom::Extent>,
    pub bytes: usize,
}

/// A display copy projected only after thinning. Mercator's largest derivative over the
/// bounds of each line or ring converts the tolerance to degrees, including polar clamping.
/// Other projections retain projected-space thinning: sampled scales cannot bound their seams
/// or singularities. Non-finite vertices survive thinning and still break paths after projection.
pub fn project_thinned(g: &Geometry, flat: &crate::projection::Flat, tol: f64) -> Geometry {
    if !tol.is_finite() || tol <= 0.0 {
        return flat.project(g);
    }
    if !matches!(flat, crate::projection::Flat::Mercator) {
        return thin(&flat.project(g), tol);
    }
    flat.project(&thin_with(g, |run| {
        let latitude = run.iter().filter(|c| c.is_finite()).fold(0.0f64, |lat, c| lat.max(c.y.abs().min(rustgis_geom::MAX_LAT)));
        // |dx| + |dy| in degrees times this derivative bounds projected Euclidean distance.
        let per_degree = rustgis_geom::MERCATOR_R * std::f64::consts::PI / 180.0 / latitude.to_radians().cos();
        thin_run(run, tol / per_degree)
    }))
}

/// Builds a Mercator display level without ever constructing the full projected geometry.
/// Original extents are transformed separately so thinning cannot change culling or splats.
/// Both the geometry and its extent table fit in the optional display-cache budget.
pub(crate) fn display(
    features: &[rustgis_data::Feature],
    revision: u64,
    level: i32,
    budget: usize,
    whole_layer_eligible: bool,
) -> Option<Display> {
    #[cfg(test)]
    DISPLAY_BUILDS.set(DISPLAY_BUILDS.get() + 1);
    let eligible: usize = features.iter().filter_map(|f| f.geometry.as_ref()).map(thinnable).sum();
    if !whole_layer_eligible && eligible <= THIN_ABOVE {
        return None;
    }
    let extent_bytes = features.len().checked_mul(std::mem::size_of::<rustgis_geom::Extent>())?;
    let geometries = project_budgeted(features, level, budget.checked_sub(extent_bytes)?)?;
    let all: usize = features.iter().filter_map(|f| f.geometry.as_ref()).map(vertices).sum();
    let kept: usize = geometries.iter().flatten().map(vertices).sum();
    if !whole_layer_eligible && kept as f64 > all as f64 * (1.0 - MIN_SAVING) {
        return None;
    }
    // Mercator is monotone in both axes; two opposite corners bound every source vertex.
    let extents = features
        .iter()
        .map(|f| {
            let e = f.geometry.as_ref().map_or(rustgis_geom::Extent::EMPTY, Geometry::extent);
            if e.is_empty() {
                e
            } else {
                rustgis_geom::Extent::from_points(
                    rustgis_geom::lonlat_to_merc(Coord::new(e.min_x, e.min_y)),
                    rustgis_geom::lonlat_to_merc(Coord::new(e.max_x, e.max_y)),
                )
            }
        })
        .collect();
    let bytes = cache_bytes(&geometries).checked_add(extent_bytes)?;
    Some(Display { revision, level, whole_layer_eligible, geometries, extents, bytes })
}

/// Builds only Mercator display geometry within `budget`. Callers apply their own eligibility
/// and saving rules; exact consumers already have extents and need no second extent table.
pub(crate) fn project_budgeted(features: &[rustgis_data::Feature], level: i32, budget: usize) -> Option<Vec<Option<Geometry>>> {
    let mut bytes = features.len().checked_mul(std::mem::size_of::<Option<Geometry>>())?;
    if bytes > budget {
        return None;
    }
    let mut geometries = Vec::with_capacity(features.len());
    for f in features {
        let g = f.geometry.as_ref().map(|g| project_thinned(g, &crate::projection::Flat::Mercator, tolerance(level)));
        bytes = bytes.checked_add(g.as_ref().map_or(0, heap_bytes))?;
        if bytes > budget {
            return None;
        }
        geometries.push(g);
    }
    Some(geometries)
}

/// Heap allocation held by a geometry's vectors (the enum itself is in its containing array).
fn heap_bytes(g: &Geometry) -> usize {
    rustgis_data::source::memory::geometry_bytes(g)
}

pub(crate) fn cache_bytes(geoms: &Vec<Option<Geometry>>) -> usize {
    geoms
        .iter()
        .flatten()
        .fold(geoms.capacity().saturating_mul(std::mem::size_of::<Option<Geometry>>()), |n, g| n.saturating_add(heap_bytes(g)))
}

/// Build one level without first allocating a whole window's extra geometry. Running out of
/// budget discards only this optional display cache; drawing still uses the full projection.
pub(crate) fn thin_budgeted(geoms: &[Option<Geometry>], tol: f64, budget: usize) -> Option<Vec<Option<Geometry>>> {
    let mut bytes = geoms.len().checked_mul(std::mem::size_of::<Option<Geometry>>())?;
    if bytes > budget {
        return None;
    }
    let mut copy = Vec::with_capacity(geoms.len());
    for g in geoms {
        let thinned = g.as_ref().map(|g| thin(g, tol));
        bytes = bytes.checked_add(thinned.as_ref().map_or(0, heap_bytes))?;
        if bytes > budget {
            return None;
        }
        copy.push(thinned);
    }
    Some(copy)
}

/// The level for a tolerance in projection units: the largest whose tolerance (2^level) is at
/// most `tol`. `None` for a tolerance that is not a positive number.
pub fn level(tol: f64) -> Option<i32> {
    (tol.is_finite() && tol > 0.0).then(|| tol.log2().floor().clamp(-60.0, 60.0) as i32)
}

/// A level's tolerance in projection units.
pub fn tolerance(level: i32) -> f64 {
    2f64.powi(level)
}

/// A copy of `g` keeping, along each line and ring, only vertices at least `tol` (measured as
/// |dx| + |dy|) from the last vertex kept, and always each one's first and last. A ring that
/// would keep fewer than four vertices stays whole, so a small polygon still has an area to fill.
pub fn thin(g: &Geometry, tol: f64) -> Geometry {
    thin_with(g, |run| thin_run(run, tol))
}

/// Applies a run-specific thinning rule while preserving points and rings with too few kept vertices.
fn thin_with(g: &Geometry, run: impl Fn(&[Coord]) -> Vec<Coord>) -> Geometry {
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => g.clone(),
        Geometry::Line(parts) => Geometry::Line(parts.iter().map(|p| run(p)).collect()),
        Geometry::Polygon(polys) => Geometry::Polygon(
            polys
                .iter()
                .map(|rings| {
                    rings
                        .iter()
                        .map(|r| {
                            let kept = run(r);
                            if kept.len() < 4 { r.clone() } else { kept }
                        })
                        .collect()
                })
                .collect(),
        ),
    }
}

/// The vertices of one line or ring kept at a tolerance. A point the projection couldn't show
/// (not finite) is kept, with the vertex after it, so the renderer still breaks the line there.
fn thin_run(run: &[Coord], tol: f64) -> Vec<Coord> {
    let mut out = Vec::new();
    let mut last: Option<Coord> = None;
    let n = run.len();
    for (i, c) in run.iter().enumerate() {
        let finite = c.x.is_finite() && c.y.is_finite();
        let keep = match last {
            Some(l) if finite && i + 1 != n => (c.x - l.x).abs() + (c.y - l.y).abs() >= tol,
            _ => true,
        };
        if keep {
            out.push(*c);
            last = finite.then_some(*c);
        }
    }
    out
}

/// The number of vertices thinning can drop: those of its lines and rings (points are drawn
/// as they are).
pub fn thinnable(g: &Geometry) -> usize {
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => 0,
        _ => vertices(g),
    }
}

/// The number of vertices in a geometry.
pub fn vertices(g: &Geometry) -> usize {
    match g {
        Geometry::Point(_) => 1,
        Geometry::MultiPoint(p) => p.len(),
        Geometry::Line(parts) => parts.iter().map(Vec::len).sum(),
        Geometry::Polygon(polys) => polys.iter().flatten().map(Vec::len).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprojection_thinning_bounds_mercator_error_at_all_latitudes() {
        let flat = crate::projection::Flat::Mercator;
        for (south, north) in [(-1.5, -1.0), (70.0, 85.0), (-90.0, 90.0), (89.0, 95.0)] {
            let run: Vec<_> = (0..10_000)
                .map(|i| {
                    let t = i as f64 / 9999.0;
                    Coord::new(36.0 + t * 0.5, south + (north - south) * t)
                })
                .collect();
            let original = Geometry::Line(vec![run]);
            let Geometry::Line(full) = flat.project(&original) else { panic!("line") };
            let Geometry::Line(kept) = project_thinned(&original, &flat, 10_000.0) else { panic!("line") };
            // Require savings only near the equator; a pole-spanning geometry may keep every
            // vertex under the larger polar bound. All runs must satisfy the error bound.
            if south == -1.5 {
                assert!(kept[0].len() < full[0].len() / 10);
            }
            assert_eq!(kept[0].first(), full[0].first());
            assert_eq!(kept[0].last(), full[0].last());
            // Each dropped vertex is within tolerance of its preceding retained source vertex.
            let mut at = 0;
            for c in &full[0] {
                if kept[0].get(at + 1) == Some(c) {
                    at += 1;
                }
                let anchor = kept[0][at];
                assert!((c.x - anchor.x).hypot(c.y - anchor.y) <= 10_000.0 + 1e-6);
            }
        }
    }

    /// A polar part must not force unrelated equatorial parts to retain their dense vertices.
    #[test]
    fn multipart_mercator_thinning_uses_each_runs_latitude_bound() {
        let equator: Vec<_> = (0..10_000).map(|i| Coord::new(i as f64 * 0.00001, 0.0)).collect();
        let polar = vec![Coord::new(1.0, 85.0), Coord::new(1.01, 85.0)];
        let flat = crate::projection::Flat::Mercator;
        let single = project_thinned(&Geometry::Line(vec![equator.clone()]), &flat, 100.0);
        let mixed = project_thinned(&Geometry::Line(vec![equator, polar]), &flat, 100.0);
        let (Geometry::Line(single), Geometry::Line(mixed)) = (single, mixed) else { panic!("lines") };
        assert_eq!(single[0], mixed[0]);
        assert!(mixed[0].len() < 200);
    }

    #[test]
    fn preprojection_preserves_breaks_holes_and_custom_projection_seams() {
        let flat = crate::projection::Flat::Mercator;
        let ring = vec![Coord::new(0.0, 0.0), Coord::new(0.01, 0.0), Coord::new(0.01, 0.01), Coord::new(0.0, 0.0)];
        let g = Geometry::Polygon(vec![vec![ring.clone(), ring]]);
        assert_eq!(project_thinned(&g, &flat, 1e6), flat.project(&g), "small rings and holes keep their area");
        let line = Geometry::Line(vec![vec![Coord::new(0.0, 0.0), Coord::new(f64::NAN, 1.0), Coord::new(0.001, 0.0)]]);
        let Geometry::Line(parts) = project_thinned(&line, &flat, 1e6) else { panic!("line") };
        assert_eq!(parts[0].len(), 3);
        assert!(parts[0][1].x.is_nan());
        for name in ["equalEarth", "northPolar", "EPSG:32633"] {
            let custom = crate::projection::Flat::Custom(std::sync::Arc::new(crate::projection::Projector::new(name).unwrap()));
            let g = Geometry::Line(vec![vec![Coord::new(170.0, 30.0), Coord::new(-170.0, 40.0)]]);
            let actual = project_thinned(&g, &custom, 1000.0);
            let expected = thin(&custom.project(&g), 1000.0);
            // Invalid coordinates compare through their serialized debug form, since NaN != NaN.
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{name}");
        }
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(project_thinned(&g, &flat, tolerance), flat.project(&g));
        }
    }

    #[test]
    fn optional_levels_respect_heap_budget_and_leave_original_geometry_intact() {
        let original = vec![Some(Geometry::Polygon(vec![vec![vec![
            Coord::new(0.0, 0.0),
            Coord::new(10.0, 0.0),
            Coord::new(10.0, 10.0),
            Coord::new(0.0, 0.0),
        ]]]))];
        let before = original.clone();
        assert!(thin_budgeted(&original, 0.1, 0).is_none());
        assert!(thin_budgeted(&original, 0.1, std::mem::size_of::<Option<Geometry>>()).is_none());
        let copy = thin_budgeted(&original, 0.1, 1024).unwrap();
        assert!(cache_bytes(&copy) <= 1024);
        assert_eq!(copy, before);
        assert_eq!(original, before);
    }

    #[test]
    fn thinning_keeps_ends_and_far_vertices() {
        let run: Vec<Coord> = (0..=100).map(|i| Coord::new(i as f64 * 0.1, 0.0)).collect();
        let Geometry::Line(parts) = thin(&Geometry::Line(vec![run]), 1.0) else { panic!("a line") };
        let kept = &parts[0];
        assert_eq!(kept.first(), Some(&Coord::new(0.0, 0.0)));
        assert_eq!(kept.last(), Some(&Coord::new(10.0, 0.0)));
        assert!(kept.len() <= 12, "{} vertices kept", kept.len());
        assert!(kept.windows(2).all(|w| (w[1].x - w[0].x).abs() <= 1.0 + 1e-9));
    }

    #[test]
    fn thinning_breaks_at_unprojectable_points() {
        let run =
            vec![Coord::new(0.0, 0.0), Coord::new(0.01, 0.0), Coord::new(f64::NAN, f64::NAN), Coord::new(0.02, 0.0), Coord::new(5.0, 0.0)];
        let Geometry::Line(parts) = thin(&Geometry::Line(vec![run]), 1.0) else { panic!("a line") };
        let kept = &parts[0];
        assert_eq!(kept.len(), 4);
        assert!(kept[1].x.is_nan());
        assert_eq!(kept[2], Coord::new(0.02, 0.0));
    }

    #[test]
    fn a_small_ring_stays_whole() {
        let ring = vec![Coord::new(0.0, 0.0), Coord::new(0.1, 0.0), Coord::new(0.1, 0.1), Coord::new(0.0, 0.1), Coord::new(0.0, 0.0)];
        let g = Geometry::Polygon(vec![vec![ring.clone()]]);
        assert_eq!(thin(&g, 1.0), g);
        assert_eq!(thinnable(&Geometry::MultiPoint(ring)), 0);
    }

    #[test]
    fn levels_are_powers_of_two_under_the_tolerance() {
        assert_eq!(level(1.0), Some(0));
        assert_eq!(level(3.9), Some(1));
        assert_eq!(level(0.3), Some(-2));
        assert!(tolerance(level(1234.5).unwrap_or(0)) <= 1234.5);
        assert_eq!(level(0.0), None);
        assert_eq!(level(f64::NAN), None);
        assert_eq!(level(-1.0), None);
    }
}
