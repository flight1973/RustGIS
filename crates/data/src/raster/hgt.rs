//! SRTM height tiles (`.hgt`), read natively on the desktop and in the browser (#2076).
//!
//! The format, from NASA/USGS's SRTM documentation (the SRTM Topography readme
//! `SRTM_Topo.pdf`, and the user guide of the NASA SRTM Version 3.0 products, SRTMGL1/SRTMGL3,
//! at LP DAAC):
//!
//! - A file holds one 1° × 1° tile, named for its south-west corner: `N37W122.hgt` is the tile
//!   from 37°N to 38°N and from 122°W to 121°W (`S` and `E` likewise; latitude in two digits,
//!   longitude in three).
//! - It is a square grid of 16-bit signed, big-endian (Motorola order) heights in meters, with no
//!   header: 1201 × 1201 samples at 3 arc-seconds, or 3601 × 3601 at 1 arc-second. GDAL's SRTMHGT
//!   driver documentation adds 0.5 arc-second tiles (since GDAL 3.10), 7201 × 7201, read here too,
//!   on the same grid rule. Its size alone says which, and nothing else is read: another size is
//!   refused rather than guessed. A 7201 × 7201 tile holds more samples than the browser does, so
//!   there it is read at a step ([`step_within`]).
//! - Rows run north to south, and samples within a row west to east.
//! - −32768 is a void (no data).
//! - Horizontal datum WGS 84; heights are above the EGM96 geoid (mean sea level), not the
//!   ellipsoid.
//! - Samples are points on the grid lines, the first and last row and column lying exactly on
//!   the tile's whole-degree edges, so that 1° holds 1201 (3601, 7201) of them and neighboring tiles
//!   share an edge row and column.
//!
//! As a raster, each sample is a cell centered on its point: the raster's cells reach half a cell
//! beyond the whole-degree lines on every side, so a tile's extent is one degree plus one cell
//! wide ([`post_transform`]).

use super::{ColorHint, Crs, GeoTransform, Raster, max_samples};
use crate::{DataError, Result};

/// The no-data value of a tile: a void.
pub const VOID: i16 = i16::MIN;
/// The sides a tile can have: 3 arc-seconds (SRTM3), 1 arc-second (SRTM1) and 0.5 arc-second.
pub const SIDES: [usize; 3] = [1201, 3601, 7201];

/// Whether `len` bytes are a tile of one of [`SIDES`], and its side.
pub fn side_of(len: u64) -> Option<usize> {
    SIDES.into_iter().find(|s| (s * s * 2) as u64 == len)
}

/// The south-west corner (latitude, longitude, in whole degrees) of the tile a file name gives:
/// `N37W122.hgt` and `n37w122.hgt` are (37, −122); `S01E006.SRTMGL1.hgt` (the name NASA's
/// downloads have) is (−1, 6). `None` for a name that isn't a tile's, or one that lies outside
/// −90…90° and −180…180°.
pub fn tile_corner(file_name: &str) -> Option<(i32, i32)> {
    let base = std::path::Path::new(file_name).file_name()?.to_str()?;
    let tile = base.get(..7)?;
    // The tile code ends the name or a `.` follows it (`None` when byte 7 is inside a character).
    let rest = base.get(7..)?;
    if !(rest.is_empty() || rest.starts_with('.')) {
        return None;
    }
    let bytes = tile.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<i32> {
        let s = tile.get(r)?;
        if s.bytes().all(|b| b.is_ascii_digit()) { s.parse().ok() } else { None }
    };
    let lat = digits(1..3)?;
    let lon = digits(4..7)?;
    let lat = match bytes.first()?.to_ascii_uppercase() {
        b'N' => lat,
        b'S' => -lat,
        _ => return None,
    };
    let lon = match bytes.get(3)?.to_ascii_uppercase() {
        b'E' => lon,
        b'W' => -lon,
        _ => return None,
    };
    // The tile spans a degree north and east of its corner.
    ((-90..=89).contains(&lat) && (-180..=179).contains(&lon)).then_some((lat, lon))
}

/// The transform of a grid of posts (samples on grid lines) whose south-west post is at
/// (`west`, `south`), `cols` × `rows` of them `dx` and `dy` degrees apart, read every `step`-th
/// post from the north-west one: each post kept becomes a cell centered on it, `step` posts wide,
/// so the grid's edges lie half a cell beyond the outer posts kept. (At `step` 1, half a post's
/// spacing beyond the outer posts.)
pub(super) fn post_transform(west: f64, south: f64, rows: usize, dx: f64, dy: f64, step: usize) -> GeoTransform {
    let north = south + (rows.saturating_sub(1)) as f64 * dy;
    let k = step.max(1) as f64;
    let (cell_x, cell_y) = (dx * k, dy * k);
    GeoTransform { origin_x: west - cell_x / 2.0, origin_y: north + cell_y / 2.0, pixel_width: cell_x, pixel_height: -cell_y }
}

/// The step at which a `w` × `h` grid fits in `max` samples: 1, or the smallest power of two
/// whose every-`step`-th sample does ([`super::read_scan`] reduces pictures with it too). `None`
/// when none does, up to a step of 2^20.
pub(super) fn step_within(w: usize, h: usize, max: usize) -> Option<usize> {
    let mut k = 1usize;
    while w.div_ceil(k).saturating_mul(h.div_ceil(k)) > max {
        k = k.checked_mul(2)?;
        if k > 1 << 20 {
            return None;
        }
    }
    Some(k)
}

/// What a tile says of its heights' vertical reference, once a layer (a raster's `lost`): the
/// layer's coordinate system is horizontal only, so this is where it is said.
pub(super) fn vertical_note(surface: &str) -> String {
    format!(
        "heights are meters above {surface}, not the WGS 84 ellipsoid; the layer's coordinate system doesn't carry a vertical datum, so compare it with ellipsoidal heights (GPS, many lidar files) only after a geoid correction"
    )
}

/// What reading a grid's no-data posts says: `None` when there are none.
pub(super) fn void_note(voids: u64, total: u64, value: i16) -> Option<String> {
    match voids {
        0 => None,
        1 => Some(format!("1 of its {total} heights is a void ({value}), read as no data")),
        n => Some(format!("{n} of its {total} heights are voids ({value}), read as no data")),
    }
}

/// Reads an SRTM tile: `bytes` are the file, `file_name` its name (which places it).
pub fn read(file_name: &str, bytes: &[u8]) -> Result<Raster> {
    read_limited(file_name, bytes, max_samples())
}

fn bad(m: String) -> DataError {
    DataError::Format(format!("SRTM height tile: {m}"))
}

/// Where a tile named `file_name` of `len` bytes lies and its side: its south-west corner
/// (latitude, longitude) and 1201, 3601 or 7201. Checked before a file is read, so a wrong one is
/// refused unread, saying why: a name that isn't a tile's, or a size that isn't one.
pub fn check(file_name: &str, len: u64) -> Result<(i32, i32, usize)> {
    let (lat, lon) = tile_corner(file_name).ok_or_else(|| {
        bad(format!(
            "the file name {file_name:?} doesn't say where the tile is: it must start with its south-west corner, as N37W122.hgt does"
        ))
    })?;
    let side = side_of(len).ok_or_else(|| {
        bad(format!(
            "{len} bytes isn't a tile's size: a tile is 1201 × 1201 heights (2,884,802 bytes), 3601 × 3601 (25,934,402 bytes) or 7201 × 7201 (103,708,802 bytes){}",
            if len < 2 * 7201 * 7201 { "; the file may be cut short" } else { "" }
        ))
    })?;
    Ok((lat, lon, side))
}

/// [`read`], holding at most `max` samples (the tests' small limit exercises the reduction).
fn read_limited(file_name: &str, bytes: &[u8], max: usize) -> Result<Raster> {
    let (lat, lon, side) = check(file_name, bytes.len() as u64)?;
    let step = step_within(side, side, max).ok_or_else(|| bad("the tile is too large to open".into()))?;
    let out = side.div_ceil(step);
    let mut data = super::grid::<f32>(out, out)?;
    let height = |row: usize, col: usize| -> i16 {
        let at = (row * side + col) * 2;
        bytes.get(at..at + 2).and_then(|b| <[u8; 2]>::try_from(b).ok()).map_or(VOID, i16::from_be_bytes)
    };
    for row in (0..side).step_by(step) {
        for col in (0..side).step_by(step) {
            data.push(f32::from(height(row, col)));
        }
    }
    let voids = bytes.as_chunks::<2>().0.iter().filter(|b| **b == VOID.to_be_bytes()).count() as u64;
    let d = 1.0 / (side - 1) as f64;
    let transform = post_transform(f64::from(lon), f64::from(lat), side, d, d, step);
    let name = std::path::Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("srtm").to_string();
    let (w, h) = (out as u32, out as u32);
    let mut r = Raster::new(name, w, h, vec![data], transform, Crs::LonLat, Some(f64::from(VOID)), ColorHint::Gray, "Int16")?;
    if step > 1 {
        r.overview = Some((side as u32, side as u32));
        r.step = step as u32;
    }
    r.lost.push(vertical_note("the EGM96 geoid (mean sea level)"));
    r.lost.extend(void_note(voids, (side * side) as u64, VOID));
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1201 × 1201 tile whose height at (row, col) is `f(row, col)`.
    fn tile(f: impl Fn(usize, usize) -> i16) -> Vec<u8> {
        let mut b = Vec::with_capacity(1201 * 1201 * 2);
        for row in 0..1201 {
            for col in 0..1201 {
                b.extend_from_slice(&f(row, col).to_be_bytes());
            }
        }
        b
    }

    #[test]
    fn names_give_the_south_west_corner() {
        assert_eq!(tile_corner("N37W122.hgt"), Some((37, -122)));
        assert_eq!(tile_corner("/d/n37w122.HGT"), Some((37, -122)));
        assert_eq!(tile_corner("S01E006.SRTMGL1.hgt"), Some((-1, 6)));
        assert_eq!(tile_corner("S90W180.hgt"), Some((-90, -180)));
        assert_eq!(tile_corner("N00E000.hgt"), Some((0, 0)));
        for bad in [
            "N90E000.hgt",
            "N00E180.hgt",
            "S91E000.hgt",
            "X37W122.hgt",
            "N37Q122.hgt",
            "N3xW122.hgt",
            "N37W1222.hgt",
            "srtm.hgt",
            "N37W12.hgt",
            "",
            "N37W122x.hgt",
            "N+7W122.hgt",
            "ÑÑÑÑ.hgt",
            "N37W122é.hgt",
            "N37W122é",
        ] {
            assert_eq!(tile_corner(bad), None, "{bad}");
        }
    }

    #[test]
    fn reads_a_tile_in_place() {
        let bytes = tile(|row, col| match (row, col) {
            (0, 0) => 100,
            (1200, 0) => -5,
            (0, 1200) => 7,
            (600, 600) => VOID,
            (600, 601) => VOID,
            _ => (row % 100) as i16,
        });
        let d = 1.0 / 1200.0;
        for (name, west, south) in
            [("N37W122.hgt", -122.0, 37.0), ("S12E045.hgt", 45.0, -12.0), ("n00e000.hgt", 0.0, 0.0), ("S01W001.hgt", -1.0, -1.0)]
        {
            let r = read(name, &bytes).unwrap();
            assert_eq!((r.width, r.height, r.crs, r.nodata), (1201, 1201, Crs::LonLat, Some(-32768.0)), "{name}");
            let t = r.transform;
            // The cells reach half a cell past the whole-degree lines.
            assert!((t.origin_x - (west - d / 2.0)).abs() < 1e-12, "{name}: {t:?}");
            assert!((t.origin_y - (south + 1.0 + d / 2.0)).abs() < 1e-12, "{name}: {t:?}");
            assert!((t.pixel_width - d).abs() < 1e-15 && (t.pixel_height + d).abs() < 1e-15);
            // The north-west sample is at the corner's whole degrees, rows run north to south.
            let nw = t.to_world(0.5, 0.5);
            assert!((nw.x - west).abs() < 1e-9 && (nw.y - (south + 1.0)).abs() < 1e-9);
            assert_eq!(r.value(0, 0, 0), Some(100.0));
            assert_eq!(r.value(0, 0, 1200), Some(-5.0), "the south-west sample");
            assert_eq!(r.value(0, 1200, 0), Some(7.0), "the north-east sample");
            assert_eq!(r.value(0, 600, 600), None, "a void is no data");
            assert_eq!(r.overview, None);
            assert_eq!(r.lost.len(), 2, "{:?}", r.lost);
            assert!(r.lost[0].contains("EGM96 geoid"), "{:?}", r.lost);
            assert_eq!(r.lost[1], "2 of its 1442401 heights are voids (-32768), read as no data");
        }
        // Tiles at the poles and the antimeridian open, their edges half a cell past the lines.
        for name in ["N89E179.hgt", "S90W180.hgt"] {
            let r = read(name, &bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!r.extent().is_empty(), "{name}");
        }
        // With no voids, nothing is said of them; the vertical reference still is.
        let r = read("N00E000.hgt", &tile(|_, _| 1)).unwrap();
        assert_eq!(r.lost.len(), 1, "{:?}", r.lost);
        assert!(r.lost[0].contains("EGM96"));
    }

    #[test]
    fn wrong_sizes_and_names_are_refused() {
        let bytes = tile(|_, _| 3);
        let e = read("srtm.hgt", &bytes).unwrap_err().to_string();
        assert!(e.contains("doesn't say where the tile is"), "{e}");
        let e = read("N37W122.hgt", &bytes[..bytes.len() - 2]).unwrap_err().to_string();
        assert!(e.contains("isn't a tile's size") && e.contains("cut short"), "{e}");
        let mut longer = bytes.clone();
        longer.extend_from_slice(&[0, 0]);
        let e = read("N37W122.hgt", &longer).unwrap_err().to_string();
        assert!(e.contains("isn't a tile's size") && e.contains("7201 × 7201 (103,708,802 bytes)"), "{e}");
        // Any size short of the largest tile may be one cut short; past it, it can't be.
        let e = check("N37W122.hgt", 2 * 3601 * 3601 + 2).unwrap_err().to_string();
        assert!(e.contains("cut short"), "{e}");
        let e = check("N37W122.hgt", 2 * 7201 * 7201 - 2).unwrap_err().to_string();
        assert!(e.contains("cut short"), "{e}");
        let e = check("N37W122.hgt", 2 * 7201 * 7201 + 2).unwrap_err().to_string();
        assert!(e.contains("isn't a tile's size") && !e.contains("cut short"), "{e}");
        assert_eq!(check("N37W122.hgt", 2 * 3601 * 3601).unwrap(), (37, -122, 3601));
        // 1200 × 1200 or 1201 × 1200 would be guesses.
        assert!(read("N37W122.hgt", &vec![0; 2 * 1200 * 1200]).is_err());
        assert!(read("N37W122.hgt", &vec![0; 2 * 1201 * 1200]).is_err());
    }

    #[test]
    fn a_tile_past_the_budget_is_read_at_every_other_sample() {
        let bytes = tile(|row, col| (row * 2 + col % 7) as i16);
        let r = read_limited("N10E020.hgt", &bytes, 1201 * 1201 - 1).unwrap();
        assert_eq!((r.width, r.height, r.overview, r.step), (601, 601, Some((1201, 1201)), 2));
        assert_eq!(r.value(0, 3, 5), Some(26.0), "file row 10 × 2 + file column 6 % 7");
        // Cells are two samples wide, each centered on the sample kept: output (3, 5) is file
        // column 6, row 10, at 20° + 6d east and 11° − 10d north.
        let d = 1.0 / 1200.0;
        assert!((r.transform.origin_x - (20.0 - d)).abs() < 1e-12);
        assert!((r.transform.origin_y - (11.0 + d)).abs() < 1e-12);
        assert!((r.transform.pixel_width - 2.0 * d).abs() < 1e-15);
        let kept = r.transform.to_world(3.5, 5.5);
        assert!((kept.x - (20.0 + 6.0 * d)).abs() < 1e-9 && (kept.y - (11.0 - 10.0 * d)).abs() < 1e-9, "{kept:?}");
    }

    #[test]
    fn a_half_arc_second_tile_is_read_within_the_budget() {
        // 7201 × 7201 at 0.5″: 51,854,401 samples, more than the browser holds, read at a step.
        let side = 7201;
        let mut bytes = vec![0u8; 2 * side * side];
        let mut put = |row: usize, col: usize, v: i16| {
            let at = 2 * (row * side + col);
            bytes[at..at + 2].copy_from_slice(&v.to_be_bytes());
        };
        put(0, 0, 11);
        put(8 * 3, 8 * 5, 853);
        put(7200, 7200, -4);
        let r = read_limited("N45E006.hgt", &bytes, 1_000_000).unwrap();
        assert_eq!((r.width, r.height, r.overview, r.step), (901, 901, Some((7201, 7201)), 8));
        assert_eq!(r.value(0, 0, 0), Some(11.0));
        assert_eq!(r.value(0, 5, 3), Some(853.0));
        assert_eq!(r.value(0, 900, 900), Some(-4.0), "the south-east sample is kept");
        let d = 1.0 / 7200.0;
        let t = r.transform;
        assert!((t.pixel_width - 8.0 * d).abs() < 1e-15 && (t.pixel_height + 8.0 * d).abs() < 1e-15);
        let kept = t.to_world(5.5, 3.5);
        assert!((kept.x - (6.0 + 40.0 * d)).abs() < 1e-9 && (kept.y - (46.0 - 24.0 * d)).abs() < 1e-9, "{kept:?}");
        let se = t.to_world(900.5, 900.5);
        assert!((se.x - 7.0).abs() < 1e-9 && (se.y - 45.0).abs() < 1e-9, "{se:?}");
        // Within the desktop's least budget it would be read whole.
        assert_eq!(step_within(side, side, crate::raster::SAMPLES_FLOOR), Some(1));
        assert_eq!(check("N45E006.hgt", bytes.len() as u64).unwrap(), (45, 6, 7201));
    }

    #[test]
    fn junk_is_refused() {
        let bytes = tile(|r, c| (r ^ c) as i16);
        // Prefixes, random bytes of a tile's size, and nothing never panic.
        let mut seed = 0x2076u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..64 {
            let cut = (next() % bytes.len() as u64) as usize;
            assert!(read("N37W122.hgt", &bytes[..cut]).is_err());
        }
        let mut random = vec![0u8; bytes.len()];
        for _ in 0..4 {
            random.iter_mut().for_each(|b| *b = next() as u8);
            let r = read("N37W122.hgt", &random).unwrap();
            assert_eq!((r.width, r.height), (1201, 1201));
        }
        assert!(read("N37W122.hgt", &[]).is_err());
        assert!(read("", &[]).is_err());
    }
}
