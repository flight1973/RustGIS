//! L0: coordinates, extents, the Web Mercator projection, geodesic measurement and map scale.
//!
//! Everything here is plain math with no I/O. Longitude/latitude are WGS 84 degrees; projected
//! coordinates are Web Mercator metres (EPSG:3857).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod body;
pub mod sinusoidal;
pub mod triangulate;

pub use body::{BODIES, Body};
pub use sinusoidal::Sinusoidal;

use std::sync::LazyLock;

use geographiclib_rs::{DirectGeodesic, Geodesic, InverseGeodesic};
use serde::{Deserialize, Serialize};

/// Mean Earth radius used by the haversine formula (metres).
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// The smallest radius of curvature of the WGS 84 ellipsoid, b²/a, along the meridian at the
/// equator (metres). Every path on the ellipsoid is at least as long as on a sphere of this radius
/// through the same longitudes and latitudes, so a reach worked out on that sphere is never short.
pub const WGS84_MIN_RADIUS_M: f64 = 6_335_439.327_293;
/// Web Mercator sphere radius (metres).
pub const MERCATOR_R: f64 = 6_378_137.0;
/// Latitude limit of Web Mercator.
pub const MAX_LAT: f64 = 85.051_128_78;
/// Screen pixel size assumed when converting resolution to a map scale (0.28 mm, OGC).
pub const PIXEL_M: f64 = 0.000_28;

/// Metres per international foot (not a US survey foot).
pub const METRES_PER_FOOT: f64 = 0.3048;
/// Metres per international mile.
pub const METRES_PER_MILE: f64 = 1609.344;
/// Square metres per international square foot.
pub const SQUARE_METRES_PER_SQUARE_FOOT: f64 = 0.09290304;
/// Square metres per international square mile.
pub const SQUARE_METRES_PER_SQUARE_MILE: f64 = 2_589_988.110336;
/// Square metres per international acre.
pub const SQUARE_METRES_PER_ACRE: f64 = 4046.8564224;

/// Metres per unit for a lowercase distance unit name. Callers choose defaults and normalize input.
/// Feet are international feet, not US survey feet.
pub fn distance_unit_metres(unit: &str) -> Option<f64> {
    match unit {
        "meters" => Some(1.0),
        "feet" => Some(METRES_PER_FOOT),
        "kilometers" => Some(1000.0),
        "miles" => Some(METRES_PER_MILE),
        _ => None,
    }
}

/// A 2-D coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
}

impl Coord {
    pub const fn new(x: f64, y: f64) -> Self {
        Coord { x, y }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// An axis-aligned rectangle. An "empty" extent has `min > max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Default for Extent {
    fn default() -> Self {
        Extent::EMPTY
    }
}

impl Extent {
    pub const EMPTY: Extent = Extent { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    /// The whole world in longitude/latitude.
    pub const WORLD_LONLAT: Extent = Extent { min_x: -180.0, min_y: -85.0, max_x: 180.0, max_y: 85.0 };

    pub const fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Extent { min_x, min_y, max_x, max_y }
    }
    pub fn from_points(a: Coord, b: Coord) -> Self {
        Extent { min_x: a.x.min(b.x), min_y: a.y.min(b.y), max_x: a.x.max(b.x), max_y: a.y.max(b.y) }
    }
    pub fn is_empty(&self) -> bool {
        !(self.min_x <= self.max_x && self.min_y <= self.max_y)
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max_x - self.min_x }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max_y - self.min_y }
    }
    pub fn center(&self) -> Coord {
        Coord::new((self.min_x + self.max_x) / 2.0, (self.min_y + self.max_y) / 2.0)
    }
    pub fn expand(&mut self, c: Coord) {
        if !c.is_finite() {
            return;
        }
        self.min_x = self.min_x.min(c.x);
        self.min_y = self.min_y.min(c.y);
        self.max_x = self.max_x.max(c.x);
        self.max_y = self.max_y.max(c.y);
    }
    pub fn union(&self, o: &Extent) -> Extent {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Extent::new(self.min_x.min(o.min_x), self.min_y.min(o.min_y), self.max_x.max(o.max_x), self.max_y.max(o.max_y))
    }
    pub fn intersects(&self, o: &Extent) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && self.min_x <= o.max_x
            && o.min_x <= self.max_x
            && self.min_y <= o.max_y
            && o.min_y <= self.max_y
    }
    pub fn contains(&self, c: Coord) -> bool {
        c.x >= self.min_x && c.x <= self.max_x && c.y >= self.min_y && c.y <= self.max_y
    }
    /// Grows every side by `f` of the size (0.1 = 10 %), with a minimum pad for points.
    pub fn padded(&self, f: f64, min_pad: f64) -> Extent {
        if self.is_empty() {
            return *self;
        }
        let px = (self.width() * f).max(min_pad);
        let py = (self.height() * f).max(min_pad);
        Extent::new(self.min_x - px, self.min_y - py, self.max_x + px, self.max_y + py)
    }
}

/// Longitude/latitude (degrees) → Web Mercator metres. Latitude is clamped to ±85.05°.
pub fn lonlat_to_merc(c: Coord) -> Coord {
    let lat = c.y.clamp(-MAX_LAT, MAX_LAT).to_radians();
    Coord::new(MERCATOR_R * c.x.to_radians(), MERCATOR_R * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln())
}

/// Web Mercator metres → longitude/latitude (degrees).
pub fn merc_to_lonlat(c: Coord) -> Coord {
    let lon = (c.x / MERCATOR_R).to_degrees();
    let lat = (2.0 * (c.y / MERCATOR_R).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
    Coord::new(lon, lat)
}

/// Projects a lon/lat extent to Web Mercator.
pub fn extent_to_merc(e: &Extent) -> Extent {
    if e.is_empty() {
        return *e;
    }
    Extent::from_points(lonlat_to_merc(Coord::new(e.min_x, e.min_y)), lonlat_to_merc(Coord::new(e.max_x, e.max_y)))
}

/// Great-circle distance between two lon/lat points (metres); NaN for non-finite coordinates.
pub fn haversine_m(a: Coord, b: Coord) -> f64 {
    let (la1, la2) = (a.y.to_radians(), b.y.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.x - a.x).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().clamp(0.0, 1.0).asin()
}

/// Length of a lon/lat polyline along great circles (metres).
pub fn geodesic_length_m(pts: &[Coord]) -> f64 {
    pts.windows(2)
        .map(|w| match w {
            [a, b] => haversine_m(*a, *b),
            _ => 0.0,
        })
        .sum()
}

/// A lon/lat point as a unit vector from the Earth's centre.
fn unit_vector(c: Coord) -> [f64; 3] {
    let (lon, lat) = (c.x.to_radians(), c.y.to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

/// A vector from the Earth's centre as lon/lat (degrees); `None` for the zero vector.
fn from_vector(v: [f64; 3]) -> Option<Coord> {
    let [x, y, z] = v;
    let h = x.hypot(y);
    (h.hypot(z) > 1e-12).then(|| Coord::new(y.atan2(x).to_degrees(), z.atan2(h).to_degrees()))
}

/// The point a fraction `t` of the way from `a` to `b` along the great circle between them.
pub fn geodesic_interpolate(a: Coord, b: Coord, t: f64) -> Coord {
    let (va, vb) = (unit_vector(a), unit_vector(b));
    let dot = (va[0] * vb[0] + va[1] * vb[1] + va[2] * vb[2]).clamp(-1.0, 1.0);
    let w = dot.acos();
    if w < 1e-12 || w.sin().abs() < 1e-12 {
        // The same point, or antipodes (no single great circle): straight in lon/lat.
        return Coord::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
    }
    let (ka, kb) = (((1.0 - t) * w).sin() / w.sin(), (t * w).sin() / w.sin());
    let v = [ka * va[0] + kb * vb[0], ka * va[1] + kb * vb[1], ka * va[2] + kb * vb[2]];
    match from_vector(v) {
        // Keep the longitude next to `a`'s, so a path across the antimeridian stays continuous.
        Some(c) => Coord::new(a.x + wrap_lon(c.x - a.x), c.y),
        None => a,
    }
}

/// A longitude (degrees) folded into −180..180, unchanged when it is already there. The remainder
/// is exact, as GeographicLib takes it, so a longitude far out of range still names the meridian
/// GeographicLib measures from. One that isn't finite is NaN.
pub fn fold_lon(x: f64) -> f64 {
    let r = x % 360.0;
    if r > 180.0 {
        r - 360.0
    } else if r < -180.0 {
        r + 360.0
    } else {
        r
    }
}

/// A longitude difference (degrees) folded into −180..180.
fn wrap_lon(d: f64) -> f64 {
    let w = (d + 180.0).rem_euclid(360.0) - 180.0;
    if w.is_finite() { w } else { 0.0 }
}

/// The centre of lon/lat points on the sphere (their mean direction from the Earth's centre),
/// or `None` when there are none or they balance out (such as two antipodes).
pub fn spherical_centre(pts: &[Coord]) -> Option<Coord> {
    let mut s = [0.0; 3];
    for v in pts.iter().filter(|c| c.is_finite()).map(|c| unit_vector(*c)) {
        s = [s[0] + v[0], s[1] + v[1], s[2] + v[2]];
    }
    from_vector(s)
}

/// The point `dist_m` metres from `c` along the geodesic leaving it at `azimuth_deg` (clockwise
/// from north): the direct geodesic problem on the WGS 84 ellipsoid.
pub fn geodesic_destination(c: Coord, azimuth_deg: f64, dist_m: f64) -> Coord {
    let (s, co) = azimuth_deg.to_radians().sin_cos();
    Aeqd::new(c).inverse(Coord::new(dist_m * s, dist_m * co))
}

/// The geodesic distance between two lon/lat points on the WGS 84 ellipsoid (metres): the inverse
/// geodesic problem, solved by GeographicLib.
pub fn wgs84_distance_m(a: Coord, b: Coord) -> f64 {
    let (s12, _): (f64, f64) = WGS84.inverse(a.y.clamp(-90.0, 90.0), a.x, b.y.clamp(-90.0, 90.0), b.x);
    s12
}

/// The geodesic from `a` to `b` on the WGS 84 ellipsoid: its length in meters, as
/// [`wgs84_distance_m`] gives it, and its azimuths at `a` and at `b` (degrees clockwise from north).
pub fn wgs84_inverse(a: Coord, b: Coord) -> (f64, f64, f64) {
    let (s12, azi1, azi2, _): (f64, f64, f64, f64) = WGS84.inverse(a.y.clamp(-90.0, 90.0), a.x, b.y.clamp(-90.0, 90.0), b.x);
    (s12, azi1, azi2)
}

/// The point `dist_m` meters from `c` along the geodesic leaving it at `azimuth_deg` (clockwise
/// from north) on the WGS 84 ellipsoid, and the geodesic's azimuth there. The longitude is in
/// −180..180 exactly as GeographicLib gives it, not moved next to `c`'s, which would round it.
pub fn wgs84_direct(c: Coord, azimuth_deg: f64, dist_m: f64) -> (Coord, f64) {
    let (lat, lon, azi2): (f64, f64, f64) = WGS84.direct(c.y.clamp(-90.0, 90.0), c.x, azimuth_deg, dist_m);
    (Coord::new(lon, lat), azi2)
}

/// The WGS 84 ellipsoid's geodesic solver.
static WGS84: LazyLock<Geodesic> = LazyLock::new(Geodesic::wgs84);

/// The azimuthal equidistant projection on the WGS 84 ellipsoid, centred on a lon/lat point:
/// distances and directions from the centre are true, so a buffer drawn in it is geodesic near
/// the centre. Coordinates are metres east and north of the centre.
///
/// It is Karney's ellipsoidal form (Karney 2013, "Algorithms for geodesics"): a point's
/// distance and azimuth from the centre are the inverse geodesic problem, and back again the
/// direct one, both solved by GeographicLib to a few nanometres.
#[derive(Clone, Copy, Debug)]
pub struct Aeqd {
    lon0: f64,
    lat0: f64,
}

impl Aeqd {
    pub fn new(centre: Coord) -> Self {
        Aeqd { lon0: centre.x, lat0: centre.y.clamp(-90.0, 90.0) }
    }

    /// Lon/lat (degrees) → metres from the centre.
    pub fn forward(&self, c: Coord) -> Coord {
        let (s12, azi1, _, _): (f64, f64, f64, f64) = WGS84.inverse(self.lat0, self.lon0, c.y.clamp(-90.0, 90.0), c.x);
        let (sa, ca) = azi1.to_radians().sin_cos();
        Coord::new(s12 * sa, s12 * ca)
    }

    /// Metres from the centre → lon/lat (degrees). The longitude is within 180° of the centre's,
    /// not folded into −180..180, so a shape across the antimeridian stays in one piece. Metres
    /// that aren't finite give no place (both coordinates NaN).
    pub fn inverse(&self, p: Coord) -> Coord {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return Coord::new(f64::NAN, f64::NAN);
        }
        // At a pole the azimuth is taken from the meridian `lon0`, as `forward` gives it.
        let (lat, lon): (f64, f64) = WGS84.direct(self.lat0, self.lon0, p.x.atan2(p.y).to_degrees(), p.x.hypot(p.y));
        Coord::new(self.lon0 + wrap_lon(lon - self.lon0), lat)
    }
}

/// Area of a lon/lat ring on the sphere (square metres, always positive).
///
/// Uses the spherical-excess approximation from Chamberlain & Duquette (JPL, 2007).
pub fn geodesic_ring_area_m2(ring: &[Coord]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 0..n {
        let (Some(p1), Some(p2)) = (ring.get(i), ring.get((i + 1) % n)) else { continue };
        // An edge that jumps across the antimeridian (179° to -179°) is a short hop east, not 358°
        // west. A full 360° edge (-180° to 180°, as a world-wide rectangle has) is kept as it is.
        let dlon = p2.x - p1.x;
        let dlon = if dlon.abs() > 180.0 && dlon.abs() < 360.0 { wrap_lon(dlon) } else { dlon };
        total += dlon.to_radians() * (2.0 + p1.y.to_radians().sin() + p2.y.to_radians().sin());
    }
    (total * EARTH_RADIUS_M * EARTH_RADIUS_M / 2.0).abs()
}

/// Map scale denominator (1:N) for a Web Mercator resolution (metres per screen pixel) at a latitude.
pub fn scale_denominator(merc_m_per_px: f64, lat_deg: f64) -> f64 {
    merc_m_per_px * lat_deg.to_radians().cos().abs() / PIXEL_M
}

/// Rounds a distance down to a "nice" 1, 2 or 5 × 10ⁿ for scale bars.
pub fn nice_distance(d: f64) -> f64 {
    if d <= 0.0 || !d.is_finite() {
        return 0.0;
    }
    let p = 10f64.powf(d.log10().floor());
    let f = d / p;
    let n = if f >= 5.0 {
        5.0
    } else if f >= 2.0 {
        2.0
    } else {
        1.0
    };
    n * p
}

/// Formats a distance in metres as `0.2 mm` / `5 mm` / `20 cm` / `850 m` / `12.5 km`: under a
/// metre in centimetres or millimetres, so a scale bar on a site plan (#902) doesn't read `0 m`.
/// Non-finite values display as `—`.
pub fn format_distance(m: f64) -> String {
    if !m.is_finite() {
        return "—".to_string();
    }
    // Each unit is picked, and shown, by the value rounded as it shows: formatting alone rounds
    // halves to even.
    let (mm, cm) = ((m * 1000.0).round(), (m * 100.0).round());
    if m != 0.0 && mm == 0.0 {
        // Under half a millimetre, which the closest zoom spreads over pixels: tenths, then a
        // bound rather than 0.
        let tenths = (m * 10_000.0).round() / 10.0;
        return if tenths != 0.0 { format!("{tenths:.1} mm") } else { "< 0.1 mm".to_string() };
    }
    if m != 0.0 && mm.abs() < 10.0 {
        format!("{mm:.0} mm")
    } else if m != 0.0 && cm.abs() < 100.0 {
        format!("{cm:.0} cm")
    } else if m.round() < 1000.0 {
        format!("{m:.0} m")
    } else if (m / 100.0).round() < 1000.0 {
        format!("{:.1} km", m / 1000.0)
    } else {
        format!("{:.0} km", m / 1000.0)
    }
}

/// Formats an area in square metres as `km²` or `m²`; `—` for non-finite values.
pub fn format_area(m2: f64) -> String {
    if !m2.is_finite() {
        return "—".to_string();
    }
    if m2.round() < 1.0e6 { format!("{m2:.0} m²") } else { format!("{:.1} km²", m2 / 1.0e6) }
}

/// Formats a lon/lat pair in decimal degrees with hemispheres: `12.345° E 45.678° N`.
/// Displays `—` if either coordinate is non-finite.
pub fn format_lonlat(c: Coord) -> String {
    if !c.is_finite() {
        return "—".to_string();
    }
    let ew = if c.x < 0.0 { 'W' } else { 'E' };
    let ns = if c.y < 0.0 { 'S' } else { 'N' };
    format!("{:.4}° {ew}  {:.4}° {ns}", c.x.abs(), c.y.abs())
}

/// Formats a lon/lat pair as degrees-minutes-seconds; `—` if either coordinate is non-finite.
pub fn format_dms(c: Coord) -> String {
    fn dms(v: f64) -> String {
        let tenths = (v.abs() * 36_000.0).round();
        let d = (tenths / 36_000.0).floor();
        let m = ((tenths % 36_000.0) / 600.0).floor();
        let s = (tenths % 600.0) / 10.0;
        format!("{d:.0}°{m:02.0}′{s:04.1}″")
    }
    if !c.is_finite() {
        return "—".to_string();
    }
    let ew = if c.x < 0.0 { 'W' } else { 'E' };
    let ns = if c.y < 0.0 { 'S' } else { 'N' };
    format!("{} {ew}  {} {ns}", dms(c.x), dms(c.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_roundtrip() {
        for (x, y) in [(0.0, 0.0), (12.5, 41.9), (-122.4, 37.8), (179.0, -60.0)] {
            let m = lonlat_to_merc(Coord::new(x, y));
            let back = merc_to_lonlat(m);
            assert!((back.x - x).abs() < 1e-9 && (back.y - y).abs() < 1e-9);
        }
    }

    #[test]
    fn azimuthal_equidistant_keeps_distances_from_its_centre() {
        // Distances and azimuths on WGS 84 from GeographicLib (pyproj.Geod(ellps="WGS84").inv).
        for (centre, (x, y), s12, azi1) in [
            ((10.0, 60.0), (10.0, 70.0), 1_114_907.908_280, 0.0),
            ((10.0, 60.0), (-170.0, 80.0), 4_464_718.767_198, 0.0),
            ((10.0, 60.0), (40.0, 20.0), 5_037_117.406_299, 138.557_618_275_686),
            ((10.0, 60.0), (179.5, -30.0), 16_578_966.423_682, 17.827_025_777_178),
            ((2.3522, 48.8566), (-0.1276, 51.5072), 343_896.891_267, -30.048_593_801_903),
        ] {
            let (centre, c) = (Coord::new(centre.0, centre.1), Coord::new(x, y));
            let p = Aeqd::new(centre);
            let m = p.forward(c);
            assert!((m.x.hypot(m.y) - s12).abs() < 1e-5, "{c:?}: {m:?}");
            assert!((m.x.atan2(m.y).to_degrees() - azi1).abs() < 1e-9, "{c:?}: {m:?}");
            let back = p.inverse(m);
            assert!((back.x - x).abs() < 1e-9 && (back.y - y).abs() < 1e-9, "{c:?} → {back:?}");
        }
        assert_eq!(Aeqd::new(Coord::new(10.0, 60.0)).forward(Coord::new(10.0, 60.0)), Coord::new(0.0, 0.0));
        assert!((wgs84_distance_m(Coord::new(2.3522, 48.8566), Coord::new(-0.1276, 51.5072)) - 343_896.891_267).abs() < 1e-5);
        // A tenth of a degree north from the equator is shorter than on the mean sphere, but not
        // than on the sphere of the smallest radius of curvature.
        let (a, b) = (Coord::new(0.0, 0.0), Coord::new(0.0, 0.1));
        assert!(wgs84_distance_m(a, b) < haversine_m(a, b));
        assert!(wgs84_distance_m(a, b) >= haversine_m(a, b) * WGS84_MIN_RADIUS_M / EARTH_RADIUS_M);
        // Metres that aren't finite are no place, not half of one.
        for m in [Coord::new(f64::NAN, 0.0), Coord::new(f64::INFINITY, 0.0), Coord::new(0.0, f64::NEG_INFINITY)] {
            let c = Aeqd::new(Coord::new(10.0, 45.0)).inverse(m);
            assert!(c.x.is_nan() && c.y.is_nan(), "{m:?} → {c:?}");
        }
        // The inverse stays next to the centre's longitude rather than folding into −180..180.
        assert!((Aeqd::new(Coord::new(179.0, 0.0)).inverse(Coord::new(300_000.0, 0.0)).x - 181.694_945_852_359).abs() < 1e-9);
        // Centred on a pole, the azimuth is taken from the centre's meridian, both ways.
        for pole in [90.0, -90.0] {
            let p = Aeqd::new(Coord::new(25.0, pole));
            for c in [Coord::new(25.0, pole * 0.9), Coord::new(-100.0, pole * 0.8), Coord::new(170.0, pole * 0.95)] {
                let back = p.inverse(p.forward(c));
                assert!((wrap_lon(back.x - c.x)).abs() < 1e-9 && (back.y - c.y).abs() < 1e-9, "{c:?} → {back:?}");
            }
        }
    }

    #[test]
    fn geodesic_destination_and_interpolation() {
        // Destinations on WGS 84 from GeographicLib (pyproj.Geod(ellps="WGS84").fwd): 10 km from
        // the equator and from 46.5° and 60°N (#888), 1,000 km from 60°N, and from either pole.
        for (lon, lat, az, d, lon2, lat2) in [
            (0.0, 0.0, 0.0, 10_000.0, 0.0, 0.090_436_946_951),
            (0.0, 0.0, 90.0, 10_000.0, 0.089_831_528_412, 0.0),
            (10.5, 46.5, 0.0, 10_000.0, 10.5, 46.589_958_829_711),
            (10.5, 46.5, 90.0, 10_000.0, 10.630_271_666_436, 46.499_925_816_591),
            (10.5, 46.5, 225.0, 10_000.0, 10.407_991_242_765, 46.436_351_607_689),
            (30.0, 60.0, 90.0, 10_000.0, 30.179_211_026_165, 59.999_878_434_646),
            (10.0, 60.0, 0.0, 1_000_000.0, 10.0, 68.969_947_795_748),
            (10.0, 60.0, 300.0, 1_000_000.0, -7.552_217_029_942, 63.435_692_055_327),
            (0.0, 90.0, 0.0, 1_000_000.0, 180.0, 81.046_232_815_951),
            (0.0, -90.0, 90.0, 1_000_000.0, 90.0, -81.046_232_815_951),
        ] {
            let p = geodesic_destination(Coord::new(lon, lat), az, d);
            // 1e-11° is about a micrometre.
            assert!(wrap_lon(p.x - lon2).abs() < 1e-11 && (p.y - lat2).abs() < 1e-11, "{lon} {lat} {az}: {p:?}");
        }
        let (a, b) = (Coord::new(2.3522, 48.8566), Coord::new(-0.1276, 51.5072));
        let mid = geodesic_interpolate(a, b, 0.5);
        assert!((haversine_m(a, mid) - haversine_m(mid, b)).abs() < 1e-3);
        // Across the antimeridian, the path stays continuous.
        assert!((geodesic_interpolate(Coord::new(179.0, 0.0), Coord::new(-179.0, 0.0), 0.5).x - 180.0).abs() < 1e-9);
        assert_eq!(spherical_centre(&[Coord::new(179.0, 0.0), Coord::new(-179.0, 0.0)]).map(|c| c.x.abs().round()), Some(180.0));
        assert!(spherical_centre(&[]).is_none());
    }

    #[test]
    fn wgs84_direct_and_inverse_agree_with_geographiclib() {
        // GeographicLib's destinations (pyproj.Geod(ellps="WGS84").fwd), as in the test above.
        for (lon, lat, az, d, lon2, lat2) in [
            (0.0, 0.0, 90.0, 10_000.0, 0.089_831_528_412, 0.0),
            (10.5, 46.5, 225.0, 10_000.0, 10.407_991_242_765, 46.436_351_607_689),
            (10.0, 60.0, 300.0, 1_000_000.0, -7.552_217_029_942, 63.435_692_055_327),
        ] {
            let (a, b) = (Coord::new(lon, lat), Coord::new(lon2, lat2));
            let (p, azi2) = wgs84_direct(a, az, d);
            assert!((p.x - lon2).abs() < 1e-11 && (p.y - lat2).abs() < 1e-11, "{lon} {lat} {az}: {p:?}");
            // Back again: the same length and azimuths, the one at `b` as the direct problem gave it.
            let (s12, azi1, back) = wgs84_inverse(a, b);
            assert!((s12 - d).abs() < 1e-6 && (azi1 - az).rem_euclid(360.0).min((az - azi1).rem_euclid(360.0)) < 1e-9, "{s12} {azi1}");
            assert!((back - azi2).abs() < 1e-9 && (s12 - wgs84_distance_m(a, b)).abs() < 1e-9, "{back} {azi2}");
        }
        // Folded: in range exactly as given; out of range by an exact remainder.
        for (x, folded) in
            [(0.01_f64, 0.01_f64), (-180.0, -180.0), (180.0, 180.0), (190.0, -170.0), (-190.0, 170.0), (900.25, 180.25 - 360.0)]
        {
            assert_eq!(fold_lon(x).to_bits(), folded.to_bits(), "{x}");
        }
        assert!((-180.0..=180.0).contains(&fold_lon(1e308)) && fold_lon(f64::INFINITY).is_nan());
        // Heading east along the equator, still east; across the antimeridian the longitude is
        // GeographicLib's, in -180..180 (one degree is 111,319.49 m there).
        let (p, azi2) = wgs84_direct(Coord::new(179.5, 0.0), 90.0, 111_319.490_793);
        assert!((p.x + 179.5).abs() < 1e-9 && p.y.abs() < 1e-12 && (azi2 - 90.0).abs() < 1e-12, "{p:?} {azi2}");
    }

    #[test]
    fn wgs84_keeps_its_precision_at_the_antimeridian_and_the_poles() {
        // GeographicLib's own values (Python geographiclib 2.1) where coordinates are hardest: a few
        // centimeters across the antimeridian and across a pole, near either pole, from a pole and
        // nearly to the antipode. Lengths agree to a nanometer, places to 1e-12° (0.1 µm).
        let inverse = [
            (179.999_999_9, 0.0, -179.999_999_9, 0.0, 0.022_263_896_836_887_686, 90.0, 90.0),
            (0.0, 89.999_999_9, 180.0, 89.999_999_9, 0.022_338_794_585_611_93, 0.0, 180.0),
            (10.0, 89.99, 100.0, 89.99, 1_579.591_403_108_320_3, 45.000_000_436_332_31, 134.999_999_563_667_7),
            (-179.99, -89.999, 179.99, -89.999, 0.038_988_553_759_428_43, -90.009_999_999_998_47, -89.990_000_000_001_53),
            (0.0, 0.0, 179.9999, 0.0001, 20_003_920.400_274_97, 0.009_500_228_497_032_422, 179.990_499_771_502_95),
        ];
        for (lon1, lat1, lon2, lat2, s12, azi1, azi2) in inverse {
            let (s, a1, a2) = wgs84_inverse(Coord::new(lon1, lat1), Coord::new(lon2, lat2));
            assert!((s - s12).abs() < 1e-9 && (a1 - azi1).abs() < 1e-9 && (a2 - azi2).abs() < 1e-9, "{lon1} {lat1}: {s} {a1} {a2}");
        }
        let direct = [
            (179.9999, -60.0, 80.0, 50.0, -179.999_217_557_877_2, -59.999_922_066_609_36, 79.999_235_783_004_34),
            (45.0, 89.9999, 225.0, 30.0, 11.771_982_732_812_425, 89.999_653_408_535_92, -168.228_017_267_021_84),
            (0.0, 90.0, 30.0, 1000.0, 150.0, 89.991_046_965_968_72, 180.0),
            (-180.0, -89.999_999_9, 0.0, 0.05, -180.0, -89.999_999_452_348_3, 0.0),
        ];
        for (lon1, lat1, azi1, s12, lon2, lat2, azi2) in direct {
            let (p, a2) = wgs84_direct(Coord::new(lon1, lat1), azi1, s12);
            // The same meridian: on the antimeridian, the Rust port says 180 where Python says -180.
            let dlon = wrap_lon(p.x - lon2).abs();
            assert!(dlon < 1e-12 && (p.y - lat2).abs() < 1e-12 && (a2 - azi2).abs() < 1e-9, "{lon1} {lat1}: {p:?} {a2}");
        }
    }

    #[test]
    fn nonfinite_coordinates_do_not_become_valid_distances() {
        let c = Coord::new;
        let origin = c(0.0, 0.0);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for invalid in [c(value, 0.0), c(0.0, value)] {
                assert!(haversine_m(origin, invalid).is_nan());
                assert!(haversine_m(invalid, origin).is_nan());
                assert!(geodesic_length_m(&[origin, invalid, c(1.0, 0.0)]).is_nan());
                assert!(geodesic_ring_area_m2(&[origin, invalid, c(1.0, 1.0)]).is_nan());
            }
        }
        assert_eq!(haversine_m(origin, origin), 0.0);
        let antipodal = haversine_m(origin, c(180.0, 0.0));
        assert!((antipodal - std::f64::consts::PI * EARTH_RADIUS_M).abs() < 1e-6);
        let across = haversine_m(c(179.0, 0.0), c(-179.0, 0.0));
        let unwrapped = haversine_m(c(179.0, 0.0), c(181.0, 0.0));
        assert!((across - unwrapped).abs() < 1e-6);
    }

    #[test]
    fn haversine_paris_london() {
        let d = haversine_m(Coord::new(2.3522, 48.8566), Coord::new(-0.1276, 51.5072));
        assert!((d - 343_500.0).abs() < 2_000.0, "{d}");
    }

    #[test]
    fn nonfinite_measurements_display_as_unavailable() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(format_distance(value), "—");
            assert_eq!(format_area(value), "—");
            for coord in [Coord::new(value, 50.0), Coord::new(12.0, value), Coord::new(value, value)] {
                assert_eq!(format_lonlat(coord), "—");
                assert_eq!(format_dms(coord), "—");
            }
        }
        assert_eq!(format_area(0.0), "0 m²");
        assert_eq!(format_lonlat(Coord::new(-12.5, 45.25)), "12.5000° W  45.2500° N");
        assert_eq!(format_lonlat(Coord::new(0.0, 0.0)), "0.0000° E  0.0000° N");
    }

    #[test]
    fn distances_and_areas_switch_unit_when_they_round_up() {
        assert_eq!(format_distance(850.0), "850 m");
        assert_eq!(format_distance(0.0), "0 m");
        assert_eq!(format_distance(0.005), "5 mm");
        assert_eq!(format_distance(0.0096), "1 cm");
        assert_eq!(format_distance(0.2), "20 cm");
        assert_eq!(format_distance(0.996), "1 m");
        // Ties round away from zero, as the unit is picked; signs stay.
        assert_eq!(format_distance(0.0045), "5 mm");
        assert_eq!(format_distance(0.0095), "1 cm");
        assert_eq!(format_distance(0.995), "1 m");
        assert_eq!(format_distance(-0.005), "-5 mm");
        assert_eq!(format_distance(-0.2), "-20 cm");
        // Under half a millimetre, never 0 (#923).
        assert_eq!(format_distance(0.0002), "0.2 mm");
        assert_eq!(format_distance(-0.0002), "-0.2 mm");
        assert_eq!(format_distance(0.00001), "< 0.1 mm");
        assert_eq!(format_distance(999.4), "999 m");
        assert_eq!(format_distance(999.6), "1.0 km");
        assert_eq!(format_distance(12_500.0), "12.5 km");
        assert_eq!(format_distance(99_960.0), "100 km");
        assert_eq!(format_distance(99_949.0), "99.9 km");
        assert_eq!(format_distance(1_000.0), "1.0 km");
        assert_eq!(format_area(999_999.4), "999999 m²");
        assert_eq!(format_area(999_999.6), "1.0 km²");
        assert_eq!(format_area(1.0e6), "1.0 km²");
    }

    #[test]
    fn destination_lies_at_its_distance_and_bearing() {
        let from = Coord::new(10.0, 60.0);
        for az in [0.0, 45.0, 90.0, 180.0, 270.0, 333.0] {
            let to = geodesic_destination(from, az, 500_000.0);
            let m = Aeqd::new(from).forward(to);
            assert!((m.x.hypot(m.y) - 500_000.0).abs() < 1e-6, "{az}: {to:?}");
            assert!(wrap_lon(m.x.atan2(m.y).to_degrees() - az).abs() < 1e-9, "{az}: {to:?}");
        }
        // North and east go where they say: 60° to 61°N is 111,420.728 m on WGS 84 (GeographicLib).
        assert!((geodesic_destination(from, 0.0, 111_420.727_870).y - 61.0).abs() < 1e-9);
        assert!(geodesic_destination(from, 90.0, 1000.0).x > 10.0);
        // Longitudes stay continuous across the antimeridian, so the points around one centre
        // form one shape (Tissot's indicatrix draws its circles with them).
        assert!(geodesic_destination(Coord::new(179.9, 0.0), 90.0, 100_000.0).x > 180.0);
    }

    #[test]
    fn ring_area_one_degree_square_at_equator() {
        let ring = [Coord::new(0.0, 0.0), Coord::new(1.0, 0.0), Coord::new(1.0, 1.0), Coord::new(0.0, 1.0)];
        let a = geodesic_ring_area_m2(&ring);
        assert!((a - 1.2364e10).abs() / 1.2364e10 < 0.01, "{a}");
    }

    #[test]
    fn ring_area_across_the_antimeridian() {
        let c = Coord::new;
        let across = geodesic_ring_area_m2(&[c(179.0, 0.0), c(-179.0, 0.0), c(-179.0, 2.0), c(179.0, 2.0)]);
        let unwrapped = geodesic_ring_area_m2(&[c(179.0, 0.0), c(181.0, 0.0), c(181.0, 2.0), c(179.0, 2.0)]);
        let reversed = geodesic_ring_area_m2(&[c(179.0, 2.0), c(-179.0, 2.0), c(-179.0, 0.0), c(179.0, 0.0)]);
        assert!((across - unwrapped).abs() / unwrapped < 1e-9, "{across} vs {unwrapped}");
        assert!((reversed - unwrapped).abs() / unwrapped < 1e-9, "{reversed}");
        // The same box away from the antimeridian has the same area to within the latitude band.
        let plain = geodesic_ring_area_m2(&[c(0.0, 0.0), c(2.0, 0.0), c(2.0, 2.0), c(0.0, 2.0)]);
        assert!((across - plain).abs() / plain < 1e-9, "{across} vs {plain}");
        // A full -180° to 180° edge is not collapsed: a world-wide band keeps its area.
        let band = geodesic_ring_area_m2(&[c(-180.0, -10.0), c(180.0, -10.0), c(180.0, 10.0), c(-180.0, 10.0)]);
        assert!(band > 1.0e13, "{band}");
    }

    #[test]
    fn nice_distances() {
        assert_eq!(nice_distance(730.0), 500.0);
        assert_eq!(nice_distance(2_400.0), 2_000.0);
        assert_eq!(nice_distance(0.0), 0.0);
        assert_eq!(nice_distance(f64::NAN), 0.0);
    }

    #[test]
    fn dms_carries_rounded_seconds() {
        assert_eq!(format_dms(Coord::new(12.5, -45.25)), "12°30′00.0″ E  45°15′00.0″ S");
        // 59.96″ rounds to 60.0″, which carries into the minutes and then the degrees.
        assert_eq!(format_dms(Coord::new(10.99999, -45.99999)), "11°00′00.0″ E  46°00′00.0″ S");
        assert_eq!(format_dms(Coord::new(-0.5 - 59.97 / 3600.0, 0.0)), "0°31′00.0″ W  0°00′00.0″ N");
    }

    #[test]
    fn extent_ops() {
        let mut e = Extent::EMPTY;
        assert!(e.is_empty());
        e.expand(Coord::new(1.0, 2.0));
        e.expand(Coord::new(f64::NAN, 0.0));
        e.expand(Coord::new(-1.0, 5.0));
        assert_eq!(e, Extent::new(-1.0, 2.0, 1.0, 5.0));
        assert!(e.intersects(&Extent::new(0.0, 0.0, 3.0, 3.0)));
        assert!(!e.intersects(&Extent::EMPTY));
    }
}
