//! Export a styled raster as Web Mercator PNG tiles in a PMTiles archive.

use rustgis_geom::{Coord, MERCATOR_R, extent_to_merc};
use rustgis_map::{camera::Camera, projection::Flat, raster::RasterLayer};
use serde_json::json;
use tiny_skia::Pixmap;

use crate::rustgis_tiles::pmtiles::{self, NewArchive, TileType};
use crate::{View, raster};

const MAX_TILES: u64 = 512;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const TILE_SIZE: u32 = 256;
const MAX_LAT: f64 = 85.051_128_779_806_6;

/// Half-open tile interval, retaining a tile for a zero-width extent.
fn tile_range(start: f64, end: f64, n: u32) -> (u32, u32) {
    let first = ((start * f64::from(n)).floor() as i64).clamp(0, i64::from(n) - 1) as u32;
    // One representable step absorbs projection rounding at an exact tile boundary.
    let last = ((end * f64::from(n)).next_down().ceil() as i64 - 1).clamp(i64::from(first), i64::from(n) - 1) as u32;
    (first, last)
}

/// Raster pixels are rendered with the same style and resampling as the map.
pub fn export(layer: &RasterLayer, min_zoom: u8, max_zoom: u8, opacity: f32) -> Result<Vec<u8>, String> {
    if min_zoom > max_zoom || max_zoom > 16 {
        return Err("zoom range must satisfy 0 <= minZoom <= maxZoom <= 16".into());
    }
    let r = layer.data.as_deref().ok_or("the raster's pixels are not loaded")?;
    r.require_coordinates()?;
    if layer.style.dra || layer.style.rotation.rem_euclid(360.0) != 0.0 {
        return Err("turn off dynamic range adjustment and raster rotation before exporting tiles".into());
    }
    let e = r.extent();
    if e.is_empty()
        || ![e.min_x, e.min_y, e.max_x, e.max_y].iter().all(|x| x.is_finite())
        || e.min_x < -180.0
        || e.max_x > 180.0
        || e.max_y < -MAX_LAT
        || e.min_y > MAX_LAT
    {
        return Err("the raster must have bounds within one Web Mercator world".into());
    }
    let e = rustgis_geom::Extent::new(e.min_x, e.min_y.max(-MAX_LAT), e.max_x, e.max_y.min(MAX_LAT));
    let m = extent_to_merc(&e);
    let half = std::f64::consts::PI * MERCATOR_R;
    let mut ranges = Vec::new();
    let mut count = 0u64;
    for z in min_zoom..=max_zoom {
        let n = 1u32 << z;
        let (x0, x1) = tile_range((m.min_x + half) / (2.0 * half), (m.max_x + half) / (2.0 * half), n);
        let (y0, y1) = tile_range((half - m.max_y) / (2.0 * half), (half - m.min_y) / (2.0 * half), n);
        count = count.saturating_add(u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1));
        if count > MAX_TILES {
            return Err(format!("raster export exceeds {MAX_TILES} tiles; reduce the zoom range or raster extent"));
        }
        ranges.push((z, x0, x1, y0, y1));
    }
    let stats = layer.masked_stats();
    let mut tiles = Vec::new();
    let mut bytes = 0usize;
    for (z, x0, x1, y0, y1) in ranges {
        let size = 2.0 * half / f64::from(1u32 << z);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let center = Coord::new(-half + (f64::from(x) + 0.5) * size, half - (f64::from(y) + 0.5) * size);
                let view =
                    View { width: 256.0, height: 256.0, pixels_per_point: 1.0, camera: Camera::new(center, size / 256.0), sky: false };
                let mut image = Pixmap::new(TILE_SIZE, TILE_SIZE).ok_or("couldn't allocate a raster tile")?;
                raster::draw(
                    &mut image,
                    r,
                    &layer.style,
                    stats.as_deref().map(Vec::as_slice),
                    &view,
                    opacity,
                    true,
                    raster::Place::Flat(&Flat::Mercator, "EPSG:3857"),
                );
                if image.data().as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
                    continue;
                }
                let png = image.encode_png().map_err(|e| format!("couldn't encode a raster tile: {e}"))?;
                bytes = bytes.checked_add(png.len()).ok_or("raster tile data is too large")?;
                if bytes > MAX_BYTES {
                    return Err("raster export exceeds 64 MiB of tile data; reduce the zoom range or raster extent".into());
                }
                tiles.push((z, x, y, png));
            }
        }
    }
    if tiles.is_empty() {
        return Err("the raster has no visible pixels in the requested tiles".into());
    }
    pmtiles::write(
        tiles,
        &NewArchive {
            tile_type: TileType::Png,
            gzipped_tiles: false,
            bounds: [e.min_x, e.min_y, e.max_x, e.max_y],
            metadata: json!({"name": r.name}),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rustgis_tiles::cache::{self, Payload, State, Want};
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster};

    #[test]
    fn tile_ranges_respect_exact_and_near_boundaries() {
        let n = 65_536;
        let start = 1000.0 / f64::from(n);
        let end = 1022.0 / f64::from(n);
        assert_eq!(tile_range(start, end, n), (1000, 1021));
        assert_eq!(tile_range(start, 1022.0_f64.next_up() / f64::from(n), n), (1000, 1021));
        assert_eq!(tile_range(start, end + 1e-12, n), (1000, 1022));
        assert_eq!(tile_range(start, end - 1e-12, n), (1000, 1021));
        assert_eq!(tile_range(start, start, n), (1000, 1000));
        let (first, last) = tile_range(start, end, n);
        assert_eq!((last - first + 1).pow(2), 484);
    }

    #[test]
    #[allow(clippy::panic)]
    fn exports_png_tiles_and_rejects_unbounded_or_unsupported_requests() {
        let raster = Raster::new(
            "test",
            4,
            4,
            vec![(0..16).map(|v| v as f32).collect()],
            GeoTransform { origin_x: -180.0, origin_y: 80.0, pixel_width: 90.0, pixel_height: -40.0 },
            Crs::LonLat,
            None,
            ColorHint::Gray,
            "Float32",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let mut layer = RasterLayer::from_path(raster, "test.tif");
        let data = export(&layer, 0, 1, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let path = std::env::temp_dir().join(format!("rustgis-raster-tiles-{}.pmtiles", std::process::id()));
        std::fs::write(&path, data).unwrap_or_else(|e| panic!("{e}"));
        let archive = path.to_string_lossy();
        let info = pmtiles::info_blocking(&archive, std::time::Duration::from_secs(10)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(info.tile_type, TileType::Png);
        assert_eq!((info.min_zoom, info.max_zoom), (0, 1));
        let url = format!("pmtiles:0/0/0@{archive}");
        let _ = cache::get(&url, Want::Image);
        cache::wait(web_time::Duration::from_secs(10));
        let State::Ready(payload) = cache::get(&url, Want::Image) else { panic!("tile did not load") };
        let Payload::Image(image) = payload.as_ref() else { panic!("tile was not an image") };
        assert_eq!((image.width(), image.height()), (256, 256));
        assert!(image.data().as_chunks::<4>().0.iter().any(|p| p[3] != 0));
        let faded_path = std::env::temp_dir().join(format!("rustgis-raster-tiles-faded-{}.pmtiles", std::process::id()));
        std::fs::write(&faded_path, export(&layer, 0, 0, 0.25).unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));
        let faded_url = format!("pmtiles:0/0/0@{}", faded_path.display());
        let _ = cache::get(&faded_url, Want::Image);
        cache::wait(web_time::Duration::from_secs(10));
        let State::Ready(faded) = cache::get(&faded_url, Want::Image) else { panic!("faded tile did not load") };
        let Payload::Image(faded) = faded.as_ref() else { panic!("faded tile was not an image") };
        let original_alpha = image.data().as_chunks::<4>().0.iter().map(|p| p[3]).max().unwrap_or_default();
        let faded_alpha = faded.data().as_chunks::<4>().0.iter().map(|p| p[3]).max().unwrap_or_default();
        assert_eq!(original_alpha, 255);
        assert!((63..=64).contains(&faded_alpha), "faded alpha: {faded_alpha}");
        assert!(export(&layer, 3, 2, 1.0).is_err());
        assert!(export(&layer, 0, 16, 1.0).is_err());
        layer.style.mask.min = Some(100.0);
        layer.style.mask.max = Some(200.0);
        assert!(export(&layer, 0, 0, 1.0).is_err());
        layer.style.mask = Default::default();
        layer.style.dra = true;
        assert!(export(&layer, 0, 0, 1.0).is_err());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(faded_path);
    }
}
