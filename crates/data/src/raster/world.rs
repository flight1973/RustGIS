//! World files: the six numbers, in a small text file beside a plain picture (a TIFF without
//! GeoTIFF tags, a PNG, a JPEG), that place it on the ground (#2075). As Esri documents them
//! ("World files for raster datasets"), the lines are, in order:
//!
//! 1. A: the pixel's size in x;
//! 2. D: rotation, how far y moves per column;
//! 3. B: rotation, how far x moves per row;
//! 4. E: the pixel's size in y, negative for a picture with north up;
//! 5. C: x of the center of the top-left pixel;
//! 6. F: y of the center of the top-left pixel.
//!
//! So a pixel center (column, row) is at `x = A·col + B·row + C`, `y = D·col + E·row + F`. A world
//! file names no coordinate system: that comes from the `.prj` beside the picture.
//!
//! Its name is the picture's with the extension's first and last letters and a `w` (`.tfw` for
//! `.tif`, `.jgw` for `.jpg`, `.pgw` for `.png`), the extension and a `w` (`.tifw`, `.jpgw`), or
//! `.wld`.

use std::path::{Path, PathBuf};

/// A world file is six numbers: a larger file isn't one.
const MAX_BYTES: u64 = 4096;

/// The six numbers of a world file, as written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldFile {
    pub a: f64,
    pub d: f64,
    pub b: f64,
    pub e: f64,
    pub c: f64,
    pub f: f64,
}

impl WorldFile {
    /// Reads the six numbers: one a line (blank lines and spaces around them are passed over).
    /// Anything else, a number that isn't finite, or pixels of no area, is refused, saying why.
    pub fn parse(text: &str) -> Result<WorldFile, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() != 6 {
            return Err(format!("a world file holds six numbers, this one {}", words.len()));
        }
        let mut n = [0.0f64; 6];
        for (slot, word) in n.iter_mut().zip(&words) {
            *slot = word.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("{word:?} isn't a number"))?;
        }
        let [a, d, b, e, c, f] = n;
        let w = WorldFile { a, d, b, e, c, f };
        if !(a * e - b * d).is_normal() {
            return Err("its pixels have no area (the pixel sizes and rotation cancel out)".into());
        }
        Ok(w)
    }

    /// The transform of pixel corners, in GDAL's order: x of the top-left corner, x per column,
    /// x per row, y of the top-left corner, y per column, y per row. The corner is half a pixel
    /// up and left of the center the file gives.
    pub fn affine(&self) -> [f64; 6] {
        let WorldFile { a, d, b, e, c, f } = *self;
        [c - (a + b) / 2.0, a, b, f - (d + e) / 2.0, d, e]
    }

    /// Whether it places the picture north up, unrotated and unmirrored: rows run south, columns
    /// east.
    pub fn is_north_up(&self) -> bool {
        self.b == 0.0 && self.d == 0.0 && self.a > 0.0 && self.e < 0.0
    }
}

/// The names a world file for `path` may have, most specific first: `.tfw`, `.tifw`, `.wld` for
/// a `.tif`, each in lower and upper case (`.TFW`), for file systems that tell them apart.
pub fn names(path: &Path) -> Vec<PathBuf> {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return Vec::new() };
    let lower = ext.to_ascii_lowercase();
    let mut chars = lower.chars();
    let mut out = Vec::new();
    if let (Some(first), Some(last)) = (chars.next(), lower.chars().last()) {
        out.push(format!("{first}{last}w"));
    }
    out.push(format!("{lower}w"));
    out.push("wld".to_string());
    let upper: Vec<String> = out.iter().map(|e| e.to_ascii_uppercase()).collect();
    out.extend(upper);
    out.into_iter().map(|e| path.with_extension(e)).collect()
}

/// The world file beside `path`, if there is one: its path and what reading it gave.
pub fn beside(path: &Path) -> Option<(PathBuf, Result<WorldFile, String>)> {
    let found = names(path).into_iter().find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))?;
    let read = match std::fs::metadata(&found) {
        Ok(m) if m.len() > MAX_BYTES => Err("it is too large to be a world file".to_string()),
        _ => std::fs::read_to_string(&found).map_err(|e| e.to_string()).and_then(|t| WorldFile::parse(&t)),
    };
    Some((found, read))
}

/// Whether a world file is beside `path`, whether or not it can be read.
pub fn is_beside(path: &Path) -> bool {
    names(path).iter().any(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_numbers_and_gives_the_corner() {
        let w = WorldFile::parse("2.0\n0.0\n0.0\n-2.0\n101.0\n499.0\n").unwrap();
        assert!(w.is_north_up());
        // The top-left pixel's center is (101, 499): its corner is a pixel's half up and left.
        assert_eq!(w.affine(), [100.0, 2.0, 0.0, 500.0, 0.0, -2.0]);
        // CRLF, spaces and a blank line are passed over.
        assert_eq!(WorldFile::parse(" 2\r\n0\r\n\r\n0\r\n-2\r\n101\r\n499\r\n").unwrap(), w);
        // Rotated and mirrored ones aren't north up.
        assert!(!WorldFile::parse("2 0.5 0.5 -2 0 0").unwrap().is_north_up());
        assert!(!WorldFile::parse("2 0 0 2 0 0").unwrap().is_north_up());
    }

    #[test]
    fn junk_is_refused_saying_why() {
        assert!(WorldFile::parse("1 0 0 -1 0").unwrap_err().contains("six numbers"));
        assert!(WorldFile::parse("1 0 0 -1 0 0 7").unwrap_err().contains("six numbers"));
        assert!(WorldFile::parse("1 0 0 -1 x 0").unwrap_err().contains("\"x\""));
        assert!(WorldFile::parse("1 0 0 -1 inf 0").unwrap_err().contains("isn't a number"));
        assert!(WorldFile::parse("0 0 0 -1 0 0").unwrap_err().contains("no area"));
        assert!(WorldFile::parse("1 1 1 1 0 0").unwrap_err().contains("no area"));
        assert!(WorldFile::parse("").is_err());
    }

    #[test]
    fn names_follow_the_picture() {
        let ext = |p: &str| names(Path::new(p)).iter().map(|n| n.extension().unwrap().to_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(ext("a.tif"), ["tfw", "tifw", "wld", "TFW", "TIFW", "WLD"]);
        assert_eq!(ext("a.JPG"), ["jgw", "jpgw", "wld", "JGW", "JPGW", "WLD"]);
        assert_eq!(ext("a.jpeg"), ["jgw", "jpegw", "wld", "JGW", "JPEGW", "WLD"]);
        assert_eq!(ext("a.png")[0], "pgw");
        assert!(names(Path::new("noext")).is_empty());
    }

    #[test]
    fn found_beside_and_too_large_refused() {
        let dir = std::env::temp_dir().join(format!("rustgis-world-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pic = dir.join("p.png");
        assert!(beside(&pic).is_none() && !is_beside(&pic));
        std::fs::write(dir.join("p.wld"), "1\n0\n0\n-1\n0.5\n9.5\n").unwrap();
        let (found, read) = beside(&pic).unwrap();
        assert_eq!(found.extension().unwrap(), "wld");
        assert!(read.unwrap().is_north_up());
        // The specific name wins over `.wld`.
        std::fs::write(dir.join("p.pgw"), "x").unwrap();
        let (found, read) = beside(&pic).unwrap();
        assert_eq!(found.extension().unwrap(), "pgw");
        assert!(read.is_err());
        std::fs::write(dir.join("p.pgw"), "1 ".repeat(3000)).unwrap();
        assert!(beside(&pic).unwrap().1.unwrap_err().contains("too large"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// WGS 84 longitude/latitude, as an Esri `.prj` writes it.
    const PRJ: &str = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]]"#;

    /// A folder for one test, emptied, with a 4 × 2 gray PNG `p.png` in it.
    fn picture(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustgis-world-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::GrayImage::from_fn(4, 2, |x, y| image::Luma([(x + 4 * y) as u8 * 10])).save(dir.join("p.png")).unwrap();
        dir
    }

    #[test]
    fn a_picture_is_placed_by_its_world_file_and_prj() {
        let dir = picture("placed");
        // Half-degree pixels, the top-left pixel's center at (10.25, 49.75): its corner at (10, 50).
        std::fs::write(dir.join("p.pgw"), "0.5\n0\n0\n-0.5\n10.25\n49.75\n").unwrap();
        std::fs::write(dir.join("p.prj"), PRJ).unwrap();
        let r = crate::raster::read_path(&dir.join("p.png")).unwrap();
        assert!(r.georeferenced && r.lost.is_empty(), "{:?}", r.lost);
        let t = r.transform;
        assert_eq!((t.origin_x, t.origin_y, t.pixel_width, t.pixel_height), (10.0, 50.0, 0.5, -0.5));
        let e = r.extent();
        assert!((e.min_x - 10.0).abs() < 1e-9 && (e.max_x - 12.0).abs() < 1e-9 && (e.min_y - 49.0).abs() < 1e-9, "{e:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_prj_or_with_a_bad_world_file_it_stays_a_scan_and_says_why() {
        let dir = picture("scan");
        std::fs::write(dir.join("p.pgw"), "0.5\n0\n0\n-0.5\n10.25\n49.75\n").unwrap();
        let r = crate::raster::read_path(&dir.join("p.png")).unwrap();
        assert!(!r.georeferenced);
        assert!(r.lost.iter().any(|m| m.contains("p.pgw") && m.contains("no .prj")), "{:?}", r.lost);

        std::fs::write(dir.join("p.pgw"), "0.5 0 0 -0.5 x 49.75").unwrap();
        std::fs::write(dir.join("p.prj"), PRJ).unwrap();
        let r = crate::raster::read_path(&dir.join("p.png")).unwrap();
        assert!(!r.georeferenced);
        assert!(r.lost.iter().any(|m| m.contains("can't be read") && m.contains("without coordinates")), "{:?}", r.lost);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_turned_world_file_is_resampled_north_up() {
        let dir = picture("turned");
        // Turned a quarter: columns run south, rows run west. The 4 × 2 picture lies 2 wide, 4 tall.
        std::fs::write(dir.join("p.wld"), "0\n-1\n-1\n0\n11.5\n49.5\n").unwrap();
        std::fs::write(dir.join("p.prj"), PRJ).unwrap();
        let r = crate::raster::read_path(&dir.join("p.png")).unwrap();
        assert!(r.georeferenced, "{:?}", r.lost);
        assert_eq!((r.width, r.height), (2, 4));
        let e = r.extent();
        assert!((e.min_x - 10.0).abs() < 1e-9 && (e.max_x - 12.0).abs() < 1e-9, "{e:?}");
        assert!((e.min_y - 46.0).abs() < 1e-9 && (e.max_y - 50.0).abs() < 1e-9, "{e:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_geotiff_keeps_its_tags_and_says_when_a_world_file_differs() {
        let dir = picture("tagged");
        std::fs::write(dir.join("p.pgw"), "0.5\n0\n0\n-0.5\n10.25\n49.75\n").unwrap();
        std::fs::write(dir.join("p.prj"), PRJ).unwrap();
        let placed = crate::raster::read_path(&dir.join("p.png")).unwrap();
        let tif = dir.join("t.tif");
        crate::raster::cogwrite::write_file(&placed, 0, &tif).unwrap();
        // A world file that agrees says nothing.
        std::fs::write(dir.join("t.tfw"), "0.5\n0\n0\n-0.5\n10.25\n49.75\n").unwrap();
        let r = crate::raster::read_path(&tif).unwrap();
        assert!(r.georeferenced && r.lost.iter().all(|m| !m.contains("t.tfw")), "{:?}", r.lost);
        // One that doesn't is said, and the tags are used.
        std::fs::write(dir.join("t.tfw"), "0.5\n0\n0\n-0.5\n20.25\n49.75\n").unwrap();
        let r = crate::raster::read_path(&tif).unwrap();
        assert_eq!(r.transform.origin_x, 10.0);
        assert!(r.lost.iter().any(|m| m.contains("t.tfw") && m.contains("tags, which are used")), "{:?}", r.lost);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
