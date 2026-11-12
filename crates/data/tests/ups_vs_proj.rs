//! UPS in `rustgis-geom` against proj4rs, which reprojects rasters and layers: polar
//! stereographic at UPS's scale of 0.994 and 2,000,000 m false origin (EPSG:5041 north, 5042
//! south), at random places in both polar areas and at the poles, within a millimeter.

use rustgis_data::raster::Crs;
use rustgis_geom::{Coord, ups};

/// A small deterministic generator (no dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn proj(north: bool) -> rustgis_data::raster::Transformer {
    let lat = if north { 90 } else { -90 };
    Crs::parse(&format!("+proj=stere +lat_0={lat} +lat_ts={lat} +k=0.994 +x_0=2000000 +y_0=2000000 +datum=WGS84 +units=m +no_defs"))
        .unwrap()
        .transformer()
        .unwrap()
}

/// Compares one place both ways; returns the largest difference in meters.
fn compare(t: &rustgis_data::raster::Transformer, p: Coord, north: bool) -> f64 {
    let (e, n) = ups::to_ups_in_zone(p, north).unwrap();
    let want = t.from_lonlat(p).unwrap();
    let dm = (e - want.x).abs().max((n - want.y).abs());
    assert!(dm < 1e-3, "north {north} at {p:?}: ours {e} {n}, proj4rs {} {}", want.x, want.y);
    let back = ups::from_ups(north, want.x, want.y).unwrap();
    let theirs = t.to_lonlat(want).unwrap();
    // Compare on the ground: near the pole a millimeter is many degrees of longitude.
    let (x, y) = ups::to_ups_in_zone(back, north).unwrap();
    let (tx, ty) = ups::to_ups_in_zone(theirs, north).unwrap();
    assert!((x - tx).abs().max((y - ty).abs()) < 1e-3, "north {north} at {p:?}: ours {back:?}, proj4rs {theirs:?}");
    dm
}

#[test]
fn random_polar_places_agree_with_proj4rs() {
    let mut r = Lcg(2026);
    let (north_t, south_t) = (proj(true), proj(false));
    let mut worst = 0.0f64;
    for _ in 0..5_000 {
        let lon = -180.0 + r.next() * 360.0;
        worst = worst.max(compare(&north_t, Coord::new(lon, 84.0 + r.next() * 6.0), true));
        worst = worst.max(compare(&south_t, Coord::new(lon, -80.0 - 1e-9 - r.next() * (10.0 - 2e-9)), false));
    }
    for (p, north) in [(Coord::new(0.0, 90.0), true), (Coord::new(0.0, -90.0), false), (Coord::new(180.0, 84.0), true)] {
        worst = worst.max(compare(if north { &north_t } else { &south_t }, p, north));
    }
    assert!(worst < 1e-3, "worst {worst} m");
}
