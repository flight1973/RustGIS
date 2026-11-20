//! PDF pages read as rasters (#2336), on the desktop: a page drawn as a picture by `hayro`, placed
//! where its GeoPDF viewport says (ISO 32000-2 § 12.10: a `/VP` entry whose `/Measure` is of
//! subtype `/GEO`), or else a scan with no coordinates, to place with Georeference.
//!
//! The whole page is drawn, its title and legend too, and the map frame's coordinates run across
//! it, as readers of GeoPDFs do.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use hayro::hayro_interpret::font::FontQuery;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{InterpreterSettings, InterpreterWarning};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::content::TypedIter;
use hayro::hayro_syntax::content::ops::TypedInstruction;
use hayro::hayro_syntax::object::{Array, Dict, Name, Rect, Stream, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::kurbo::{Affine, Point};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use rustgis_geom::Coord;

use super::{ColorHint, Crs, Raster, crs, grid, max_samples, unreferenced, wkt, world};
use crate::{DataError, Result};

/// The resolution a page is drawn at, in dots an inch, unless that wouldn't fit in memory.
pub const DPI: f64 = 150.0;
/// The longest side a page is drawn at, in pixels (the renderer's limit).
const MAX_SIDE: f64 = 65_535.0;
/// The largest PDF file read: it is held in memory whole while its page is drawn. The file's size
/// doesn't bound the memory drawing it takes: a small compressed stream or image can decode to far
/// more ([`read_bytes`]).
pub const MAX_BYTES: u64 = 1 << 30;
/// The most other pages a notice names.
const NAMED_PAGES: usize = 20;

/// Reads page `page` of the PDF at `path` (its number, from 1; the first when `None`), placed by
/// its coordinates when `place` (else a scan, as `"as": "scan"` opens it).
pub fn read_file(path: &Path, page: Option<&str>, place: bool) -> Result<Raster> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(DataError::Format(format!("{} is not a regular file", path.display())));
    }
    if meta.len() > MAX_BYTES {
        return Err(DataError::Format(format!("the PDF is larger than {} MB, too large to open", MAX_BYTES >> 20)));
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("raster");
    let mut r = read_bytes(name, crate::provenance::read(path)?, page, place)?;
    r.source = Some(path.display().to_string());
    Ok(r)
}

/// Reads page `page` (its number, from 1; the first when `None`) of a PDF's contents, named
/// `name`. When `place`, a page with a GeoPDF viewport is placed by it; otherwise, or without
/// one, it is a scan. Other pages, other viewports, and a page drawn at less than [`DPI`] are said
/// in `lost`.
///
/// Reading the file and drawing the page are each one step that can't be stopped: Cancel is looked
/// at before, between and after them where the run asks for it ([`crate::progress`], as the read
/// queue's does). A download's page the app draws on a thread of its own is given up by the app
/// instead, before its turn, or dropped once drawn.
///
/// The memory and time drawing takes aren't bounded by the file's size, and can't be stopped: a
/// compressed stream can inflate to many times its size, an image can say it is far larger than
/// its data, and forms drawn within forms multiply the work. `hayro` gives no way to bound or stop
/// any of them (#2482: decoded-size and work limits need hayro support).
pub fn read_bytes(name: &str, bytes: Vec<u8>, page: Option<&str>, place: bool) -> Result<Raster> {
    read_limited(name, bytes, page, place, max_samples())
}

/// [`read_bytes`], drawing the page in at most `max` samples (the tests' small limit exercises the
/// reduction).
fn read_limited(name: &str, bytes: Vec<u8>, page: Option<&str>, place: bool, max: usize) -> Result<Raster> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(DataError::Format(format!("the PDF is larger than {} MB, too large to open", MAX_BYTES >> 20)));
    }
    crate::progress::check()?;
    let pdf = Pdf::new(bytes).map_err(|e| {
        DataError::Format(match e {
            hayro::hayro_syntax::LoadPdfError::Decryption(_) => "the PDF is encrypted, and isn't read".to_string(),
            hayro::hayro_syntax::LoadPdfError::Invalid => "not a PDF RustGIS can read".to_string(),
        })
    })?;
    let pages = pdf.pages();
    let count = pages.len();
    if count == 0 {
        return Err(DataError::Format("the PDF has no pages".into()));
    }
    let number = match page.map(str::trim).filter(|p| !p.is_empty()) {
        None => 1,
        Some(p) => p
            .strip_prefix("page ")
            .unwrap_or(p)
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=count).contains(n))
            .ok_or_else(|| DataError::Format(format!("no page '{p}': give a number from 1 to {count}")))?,
    };
    let Some(page) = pages.get(number - 1) else {
        return Err(DataError::Format(format!("no page {number}")));
    };
    let title = if count > 1 { format!("{name} page {number}") } else { name.to_string() };
    crate::progress::check()?;
    let (mut r, scale) = draw(page, &title, max)?;
    crate::progress::check()?;
    if count > 1 {
        // Only the pages named are written out, however many the file has.
        let others = count - 1;
        let shown: Vec<String> = (1..=count).filter(|n| *n != number).take(NAMED_PAGES).map(|n| n.to_string()).collect();
        let more = others.saturating_sub(NAMED_PAGES);
        let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
        r.lost.push(format!("it has {others} more pages ({}{tail}): add one with \"layer\"", shown.join(", ")));
    }
    Ok(if place { place_by_viewport(r, page, scale) } else { r })
}

/// The page drawn on white, as red, green and blue bands, and the pixels a point spans: at
/// [`DPI`], or less where its bands wouldn't fit in `max` samples or within the renderer's limit
/// (said in `lost`).
fn draw(page: &Page<'_>, name: &str, max: usize) -> Result<(Raster, f64)> {
    let (w, h) = page.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w >= 1.0 && h >= 1.0 && w.is_finite() && h.is_finite()) {
        return Err(DataError::Format("the page has no area".into()));
    }
    let wanted = DPI / 72.0;
    // Three samples a pixel kept (red, green, blue); the drawing's four bytes a pixel are a
    // fraction of what they take as `f32`.
    let fits = (max as f64 / 3.0 / (w * h)).sqrt();
    let largest = MAX_SIDE / w.max(h);
    let scale = wanted.min(largest).min(fits);
    if w * scale < 1.0 || h * scale < 1.0 {
        return Err(DataError::Format("the page is too large to draw".into()));
    }
    let settings = PixmapSettings { x_scale: scale as f32, y_scale: scale as f32, bg_color: WHITE };
    // What couldn't be drawn, counted by kind: images, fonts, annotations, and anything else; and
    // whether `hayro` asked for a font program it carries: a standard font not in the file, or a
    // stand-in for another font not in the file.
    let warned: Arc<[AtomicUsize; 4]> = Arc::new(Default::default());
    let asked = Arc::new(AtomicBool::new(false));
    let defaults = InterpreterSettings::default();
    let resolve = Arc::clone(&defaults.font_resolver);
    let (sink, asked_by) = (Arc::clone(&warned), Arc::clone(&asked));
    let interpret = InterpreterSettings {
        warning_sink: Arc::new(move |w| {
            let kind = match w {
                InterpreterWarning::ImageDecodeFailure => 0,
                InterpreterWarning::UnsupportedFont => 1,
                InterpreterWarning::UnresolvedAnnotationAppearance => 2,
                _ => 3,
            };
            if let Some(n) = sink.get(kind) {
                n.fetch_add(1, Ordering::Relaxed);
            }
        }),
        font_resolver: Arc::new(move |query: &FontQuery| {
            // Every query, a standard one too: `hayro` asks for a simple font not in the file (an
            // Arial) as the standard font it picks for it, so the query alone can't tell a
            // stand-in from one of the standard 14 drawn in its own.
            asked_by.store(true, Ordering::Relaxed);
            resolve(query)
        }),
        ..defaults
    };
    let pixmap = hayro::render(page, &RenderCache::new(), &interpret, &RenderSettings::default(), &settings);
    let (pw, ph) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    // On an opaque background every pixel is opaque, so its premultiplied colour is its colour:
    // red, green and blue, the alpha left out.
    let n = pw as usize * ph as usize;
    let mut bands: Vec<Vec<f32>> = (0..3).map(|_| grid(pw as usize, ph as usize)).collect::<Result<_>>()?;
    for px in pixmap.data_as_u8_slice().as_chunks::<4>().0.iter().take(n) {
        for (band, v) in bands.iter_mut().zip(px) {
            band.push(f32::from(*v));
        }
    }
    drop(pixmap);
    let mut r = Raster::new(name, pw, ph, bands, unreferenced(pw, ph), Crs::LonLat, None, ColorHint::Rgb { alpha: false }, "UInt8")?;
    r.georeferenced = false;
    if scale < wanted * 0.999 {
        let why = if fits <= largest { "to fit in memory" } else { "the largest size it can be drawn at" };
        r.lost.push(format!("its page is drawn at {:.0} dots an inch, not {DPI:.0}, {why}", scale * 72.0));
    }
    // `hayro` warns each time it meets one, not which one it is: an image (or its soft mask, which
    // it can't be told from) each time it is drawn, a font each time it is chosen (`Tf`), whether
    // or not text is then shown in it. The counts are of times, not of images or fonts. The last
    // is for a kind of warning a later `hayro` may add.
    let warnings = [
        "an image or its mask can't be decoded: the image is left blank, or drawn without its mask",
        "a font that can't be read is chosen: its text is drawn in a stand-in font",
        "an annotation whose appearance can't be found is left out",
        "something on the page can't be drawn and is left out",
    ];
    for (count, what) in warned.iter().map(|n| n.load(Ordering::Relaxed)).zip(warnings) {
        match count {
            0 => {}
            1 => r.lost.push(format!("{what} (once)")),
            n => r.lost.push(format!("{what} ({n} times)")),
        }
    }
    // Only when `hayro` asked for a font it carries is the page looked through for which fonts are
    // stand-ins. A search that stopped before naming one, or that met a Type 3 font whose glyphs it
    // doesn't read, says one may be; one that looked at everything else and named none found only
    // standard fonts.
    if asked.load(Ordering::Relaxed) {
        let found = substituted_fonts(page, FONT_SEARCH_BYTES, FONT_SEARCH_WORK)?;
        let shown: Vec<&str> = found.names.iter().take(MISSING_FONTS_NAMED).map(String::as_str).collect();
        let left = found.keys.len().saturating_sub(shown.len());
        let tail = match (found.stopped, left) {
            (false, 0) => String::new(),
            (false, n) => format!(", and {n} more"),
            (true, 0) => ", and perhaps others".to_string(),
            (true, n) => format!(", and {n} more (perhaps others)"),
        };
        if !shown.is_empty() {
            r.lost.push(format!("fonts not in the file are drawn in a stand-in font: {}{tail}", shown.join(", ")));
        } else if found.stopped {
            r.lost.push("fonts not in the file may be drawn in a stand-in font (the search for them stopped early)".into());
        } else if found.type3 {
            r.lost.push(
                "fonts not in the file may be drawn in a stand-in font (inside Type 3 fonts, whose glyphs aren't looked through)".into(),
            );
        }
    }
    Ok((r, scale))
}

/// The most names of fonts not in the file kept, the most named in a notice, and the most told
/// apart.
const MISSING_FONTS_KEPT: usize = 100;
const MISSING_FONTS_NAMED: usize = 10;
const MISSING_FONTS_COUNTED: usize = 10_000;
/// The most forms within forms, and content streams, looked through for the fonts chosen; the
/// most bytes their content decodes to in all; and the most fonts chosen and annotations looked at.
const FONT_SEARCH_DEPTH: usize = 32;
const FONT_SEARCH_STREAMS: usize = 1_000;
const FONT_SEARCH_BYTES: usize = 32 << 20;
const FONT_SEARCH_WORK: usize = 1_000_000;

/// The standard 14 fonts (ISO 32000-2 § 9.6.2.2), drawn in `hayro`'s own when not in the file, as
/// every reader draws them in its own: not a stand-in for something else.
const STANDARD_FONTS: [&str; 14] = [
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Symbol",
    "ZapfDingbats",
];

/// The resources a content stream's names are looked up in.
#[derive(Clone, Default)]
struct Res<'a> {
    fonts: Dict<'a>,
    xobjects: Dict<'a>,
    states: Dict<'a>,
    patterns: Dict<'a>,
}

impl<'a> Res<'a> {
    fn of(d: &Dict<'a>) -> Self {
        let get = |k: &[u8]| d.get::<Dict<'a>>(k).unwrap_or_default();
        Res { fonts: get(b"Font"), xobjects: get(b"XObject"), states: get(b"ExtGState"), patterns: get(b"Pattern") }
    }

    /// Stream `s`'s own resources, else these, which it is then drawn with.
    fn within(&self, s: &Stream<'a>) -> Self {
        s.dict().get::<Dict<'a>>(b"Resources".as_slice()).map_or_else(|| self.clone(), |d| Res::of(&d))
    }

    /// Which resources these are, by where their dictionaries lie ([`span`]).
    fn key(&self) -> [(usize, usize); 4] {
        [span(&self.fonts), span(&self.xobjects), span(&self.states), span(&self.patterns)]
    }

    /// Stream `s`'s own resources, else none: a tiling pattern is drawn with its own only, as
    /// `hayro` draws it.
    fn own(s: &Stream<'a>) -> Self {
        s.dict().get::<Dict<'a>>(b"Resources".as_slice()).map(|d| Res::of(&d)).unwrap_or_default()
    }
}

/// A content stream queued for the font search: where it lies ([`span`]), and the resources it is
/// drawn with when they aren't its own ([`Res::key`]).
type Seen = ((usize, usize), Option<[(usize, usize); 4]>);

/// Where an object's dictionary lies in the bytes it was read from: one key for an object
/// however it is reached, with an object number or without (a dictionary written in place).
fn span(d: &Dict<'_>) -> (usize, usize) {
    let bytes = d.data();
    (bytes.as_ptr() as usize, bytes.len())
}

/// The fonts not in the file that the page chooses, found by [`substituted_fonts`]: their names
/// (at most [`MISSING_FONTS_KEPT`]), the fonts told apart (at most [`MISSING_FONTS_COUNTED`]), and
/// whether the search stopped before it had looked at everything; and what it has left to spend.
#[derive(Default)]
struct FontSearch<'a> {
    names: Vec<String>,
    keys: HashSet<String>,
    stopped: bool,
    /// Whether the page chooses a Type 3 font, whose glyphs (which may choose fonts) aren't read.
    type3: bool,
    /// The streams queued, and the fonts looked at, by [`span`].
    seen: HashSet<Seen>,
    fonts: HashSet<(usize, usize)>,
    streams: usize,
    /// The bytes content may still decode to, and the fonts chosen and annotations that may
    /// still be looked at.
    bytes_left: usize,
    work_left: usize,
    /// The bytes decoded so far.
    decoded: usize,
    todo: Vec<(Stream<'a>, Res<'a>, usize)>,
}

/// The fonts the page chooses (`Tf`) whose programs aren't in the file and which aren't one of the
/// [`STANDARD_FONTS`] ([`missing_font`]): `hayro` draws their text in a stand-in. The page's
/// content is followed as `hayro` draws it, into the forms it draws, soft-mask groups, tiling
/// patterns and its annotations' appearances, to [`FONT_SEARCH_DEPTH`] deep and
/// [`FONT_SEARCH_STREAMS`] streams. Annotations are passed over as `hayro` passes them over (hidden,
/// or not for the screen), and of an appearance with states only the one `/AS` names (else `Off`)
/// is followed. Forms' content is decoded again for it (`hayro` doesn't keep it), at most `bytes`
/// in all: a stream whose stored length is more than is left isn't decoded, and one that decodes
/// to more stops the search once it is decoded (`hayro` can't decode to a limit, so one decode
/// isn't bounded by `bytes`). It looks at most `work` chosen fonts and annotations, and Cancel
/// between streams ([`crate::progress::check`]). A Type 3 font's glyphs aren't decoded: a font
/// chosen inside one isn't named. Optional content isn't looked at: a font chosen only in content
/// turned off is named, though `hayro` (which asks for its stand-in all the same) draws none of its
/// text. Done only once `hayro` has asked for a font it carries.
fn substituted_fonts<'a>(page: &Page<'a>, bytes: usize, work: usize) -> Result<FontSearch<'a>> {
    let mut search = FontSearch { bytes_left: bytes, work_left: work, ..FontSearch::default() };
    let res = page.resources();
    let page_res =
        Res { fonts: res.fonts.clone(), xobjects: res.x_objects.clone(), states: res.ext_g_states.clone(), patterns: res.patterns.clone() };
    search.ops(page.typed_operations(), &page_res, 0);
    if let Some(annots) = page.raw().get::<Array<'a>>(b"Annots".as_slice()) {
        for annot in annots.iter::<Dict<'a>>() {
            if !search.spend() {
                break;
            }
            // Hidden (bit 2) or not for the screen (NoView, bit 6): not drawn (ISO 32000-2 § 12.5.3).
            if annot.get::<u32>(b"F".as_slice()).unwrap_or(0) & (1 << 1 | 1 << 5) != 0 {
                continue;
            }
            // Without a rectangle (four numbers) to draw it in, `hayro` draws none.
            if annot.get::<Rect>(b"Rect".as_slice()).is_none() {
                continue;
            }
            // The normal appearance is the one drawn; with states, the one `/AS` names, else `Off`.
            let Some(ap) = annot.get::<Dict<'a>>(b"AP".as_slice()) else { continue };
            let drawn = ap.get::<Stream<'a>>(b"N".as_slice()).or_else(|| {
                let states = ap.get::<Dict<'a>>(b"N".as_slice())?;
                annot
                    .get::<Name<'_>>(b"AS".as_slice())
                    .and_then(|s| states.get::<Stream<'a>>(s))
                    .or_else(|| states.get::<Stream<'a>>(b"Off".as_slice()))
            });
            // `hayro` draws an appearance only with a box (four numbers) to fit it in.
            if let Some(s) = drawn.filter(|s| s.dict().get::<Rect>(b"BBox".as_slice()).is_some()) {
                search.queue(page_res.within(&s), s, 1);
            }
        }
    }
    while let Some((stream, res, depth)) = search.todo.pop() {
        crate::progress::check()?;
        // No fonts left to look at: what is left can't name one.
        if search.work_left == 0 {
            search.stopped = true;
            break;
        }
        // Its stored length first, as it decodes to at least about that (an unknown one, as much
        // as it holds).
        let stored = stream.dict().get::<usize>(b"Length".as_slice()).unwrap_or_else(|| stream.raw_data().len());
        if search.streams >= FONT_SEARCH_STREAMS || stored > search.bytes_left {
            search.stopped = true;
            break;
        }
        search.streams += 1;
        let Ok(data) = stream.decoded_data() else { continue };
        if data.len() > search.bytes_left {
            search.stopped = true;
            break;
        }
        search.bytes_left -= data.len();
        search.decoded += data.len();
        search.ops(TypedIter::new(&data), &res, depth);
    }
    Ok(search)
}

impl<'a> FontSearch<'a> {
    /// Spends one of the fonts chosen and annotations that may be looked at: false, and the
    /// search stopped, when none are left.
    fn spend(&mut self) -> bool {
        if self.work_left == 0 {
            self.stopped = true;
            return false;
        }
        self.work_left -= 1;
        true
    }

    /// Queues content stream `s`, drawn with `res`, `depth` forms deep: once, and not past
    /// [`FONT_SEARCH_DEPTH`].
    fn queue(&mut self, res: Res<'a>, s: Stream<'a>, depth: usize) {
        // A stream without resources of its own is drawn with its drawer's, which may name other
        // fonts: it is looked at once for each set it is drawn with.
        let inherited = s.dict().get::<Dict<'a>>(b"Resources".as_slice()).is_none().then(|| res.key());
        if depth > FONT_SEARCH_DEPTH {
            self.stopped = true;
        } else if self.seen.insert((span(s.dict()), inherited)) {
            self.todo.push((s, res, depth));
        }
    }

    /// Follows a content stream drawn with `res`: the fonts it chooses, and the forms, soft-mask
    /// groups and tiling patterns it draws.
    fn ops(&mut self, mut ops: TypedIter<'_>, res: &Res<'a>, depth: usize) {
        while let Some(op) = ops.next() {
            // A form or soft-mask group is drawn with its own resources, else these; a pattern
            // with its own only.
            let inherits = |s: Stream<'a>| (res.within(&s), s);
            let pattern = |s: Stream<'a>| (Res::own(&s), s);
            let drawn = match op {
                TypedInstruction::TextFont(t) => {
                    if !self.spend() {
                        return;
                    }
                    self.font(t.0, res);
                    None
                }
                TypedInstruction::XObject(x) => res
                    .xobjects
                    .get::<Stream<'a>>(x.0)
                    .filter(|s| s.dict().get::<Name<'_>>(b"Subtype".as_slice()).is_some_and(|n| n.as_str() == "Form"))
                    .map(inherits),
                TypedInstruction::SetGraphicsState(g) => {
                    let state = res.states.get::<Dict<'a>>(g.0);
                    // A graphics state may choose the font itself (`/Font [font size]`), as `Tf` does.
                    if let Some(font) =
                        state.as_ref().and_then(|s| s.get::<Array<'a>>(b"Font".as_slice())).and_then(|a| a.iter::<Dict<'a>>().next())
                    {
                        if !self.spend() {
                            return;
                        }
                        self.font_dict(&font);
                    }
                    state
                        .and_then(|s| s.get::<Dict<'a>>(b"SMask".as_slice()))
                        .and_then(|m| m.get::<Stream<'a>>(b"G".as_slice()))
                        .map(inherits)
                }
                TypedInstruction::NonStrokeColorNamed(c) => c.1.and_then(|p| res.patterns.get::<Stream<'a>>(p)).map(pattern),
                TypedInstruction::StrokeColorNamed(c) => c.1.and_then(|p| res.patterns.get::<Stream<'a>>(p)).map(pattern),
                _ => None,
            };
            if let Some((r, s)) = drawn {
                self.queue(r, s, depth + 1);
            }
        }
    }

    /// A font chosen by name in `res`: named when not in the file. Each font is looked at once,
    /// however often it is chosen; a Type 3 font is passed over ([`missing_font`]).
    fn font(&mut self, name: &Name<'_>, res: &Res<'a>) {
        match res.fonts.get::<Dict<'a>>(name) {
            Some(font) => self.font_dict(&font),
            // `hayro` draws its text in Helvetica.
            None => self.named(format!("/{} (not in its resources)", name.as_str())),
        }
    }

    /// A font chosen, by `Tf` or a graphics state: named when not in the file, once.
    fn font_dict(&mut self, font: &Dict<'a>) {
        if !self.fonts.insert(span(font)) {
            return;
        }
        if font.get::<Name<'_>>(b"Subtype".as_slice()).is_some_and(|n| n.as_str() == "Type3") {
            self.type3 = true;
        }
        if let Some(name) = missing_font(font) {
            self.named(name);
        }
    }

    /// Keeps `name`, of a font drawn in a stand-in: once for each spelling of it ([`font_key`]),
    /// and past [`MISSING_FONTS_COUNTED`] fonts told apart, the search stopped.
    fn named(&mut self, name: String) {
        let key = font_key(&name);
        if self.keys.contains(&key) {
            return;
        }
        if self.keys.len() >= MISSING_FONTS_COUNTED {
            self.stopped = true;
        } else {
            self.keys.insert(key);
            if self.names.len() < MISSING_FONTS_KEPT {
                self.names.push(name);
            }
        }
    }
}

/// A font's name as a key that two spellings of one font share: its letters and digits in lower
/// case, without an `MT` or `PS` maker's suffix, or both ("Arial,Bold" and "Arial-BoldMT" alike,
/// "TimesNewRoman" and "TimesNewRomanPSMT").
fn font_key(name: &str) -> String {
    let key: String = name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
    // `MT`, then `PS` (TimesNewRomanPSMT is TimesNewRoman).
    let key = key.strip_suffix("mt").filter(|k| !k.is_empty()).map_or(key.clone(), str::to_string);
    key.strip_suffix("ps").filter(|k| !k.is_empty()).map_or(key.clone(), str::to_string)
}

/// A font's name when `hayro` draws it in a stand-in: its program isn't in the file (no
/// `/FontFile`, `/FontFile2` or `/FontFile3` in its descriptor, or its descendant's for a
/// composite font) and it isn't one of the [`STANDARD_FONTS`], or its program is there but
/// `hayro` can't load it ([`program_rejected`]). One without a `/BaseFont` is "a font without a
/// name". A Type 3 font draws its glyphs itself; one without a `/Subtype`, of a kind `hayro`
/// can't read, or a composite font without a descendant `hayro` doesn't draw with at all, which
/// its warnings say as a font that can't be read.
fn missing_font(font: &Dict<'_>) -> Option<String> {
    let subtype = font.get::<Name<'_>>(b"Subtype".as_slice())?;
    if !matches!(subtype.as_str(), "Type1" | "MMType1" | "TrueType" | "Type0") {
        return None;
    }
    let described = if subtype.as_str() == "Type0" {
        font.get::<Array<'_>>(b"DescendantFonts".as_slice()).and_then(|a| a.iter::<Dict<'_>>().next())?
    } else {
        font.clone()
    };
    let embedded = described
        .get::<Dict<'_>>(b"FontDescriptor".as_slice())
        .is_some_and(|d| [b"FontFile".as_slice(), b"FontFile2".as_slice(), b"FontFile3".as_slice()].iter().any(|k| d.contains_key(k)));
    let base = font.get::<Name<'_>>(b"BaseFont".as_slice());
    // A subset's name starts with six capitals and a plus (ISO 32000-2 § 9.6.4).
    let name = base.as_ref().map(|n| match n.as_str().split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => n.as_str(),
    });
    let named = || name.unwrap_or("a font without a name").to_string();
    if embedded {
        return program_rejected(font).then(|| format!("{} (its program in the file can't be read)", named()));
    }
    (!name.is_some_and(|n| STANDARD_FONTS.contains(&n))).then(named)
}

/// Whether `hayro` loads `font`, whose program is in the file, in a stand-in: it can't read the
/// program, and asks for a font it carries instead. Loaded as `hayro` loads it, again (the
/// search runs only when `hayro` asked for a font it carries); a font it can't load at all is
/// left to its warnings.
fn program_rejected(font: &Dict<'_>) -> bool {
    let defaults = InterpreterSettings::default();
    let asked = Arc::new(AtomicBool::new(false));
    let (flag, resolve) = (Arc::clone(&asked), Arc::clone(&defaults.font_resolver));
    let resolver: hayro_font::FontResolverFn = Arc::new(move |query: &FontQuery| {
        flag.store(true, Ordering::Relaxed);
        resolve(query)
    });
    let loaded = hayro_font::FontCache::new().font(font, &resolver, &defaults.cmap_resolver);
    loaded.is_some() && asked.load(Ordering::Relaxed)
}

/// A GeoPDF viewport: its rectangle on the page, its control points (latitude, longitude) and
/// where they lie in that rectangle (fractions of it), and its coordinate system with the PROJ
/// definition whose datum the points are given on.
struct Viewport {
    bbox: [f64; 4],
    gpts: Vec<(f64, f64)>,
    lpts: Vec<(f64, f64)>,
    crs: Crs,
    def: Option<String>,
    note: Option<String>,
    /// What of its coordinate system's text couldn't be read.
    text_note: Option<String>,
}

impl Viewport {
    fn area(&self) -> f64 {
        ((self.bbox[2] - self.bbox[0]) * (self.bbox[3] - self.bbox[1])).abs()
    }
}

/// The page's GeoPDF viewports, and why any that would place it can't be read.
fn viewports(page: &Page<'_>) -> (Vec<Viewport>, Unread) {
    let (mut found, mut unread) = (Vec::new(), Unread::default());
    let Some(vps) = page.raw().get::<Array>(b"VP".as_slice()) else { return (found, unread) };
    // An entry that isn't a viewport's dictionary is counted, not passed over.
    let dicts: Vec<Dict> = vps.iter::<Dict>().collect();
    let strays = vps.raw_iter().count().saturating_sub(dicts.len());
    if strays > 0 {
        unread.add("an entry of its viewports isn't one".into(), strays);
    }
    for vp in dicts {
        let Some(measure) = vp.get::<Dict>(b"Measure".as_slice()) else { continue };
        // A measure in plain units (a drawing's scale) says nothing of the ground.
        if measure.get::<Name>(b"Subtype".as_slice()).is_none_or(|n| n.as_str() != "GEO") {
            continue;
        }
        match viewport(&vp, &measure) {
            Ok(v) => found.push(v),
            Err(why) => unread.add(why, 1),
        }
    }
    (found, unread)
}

/// A page's map frames that can't be used: how many, and why the first can't.
#[derive(Default)]
struct Unread {
    count: usize,
    first: Option<String>,
}

impl Unread {
    /// Counts `n` more frames that can't be used for `why`.
    fn add(&mut self, why: String, n: usize) {
        self.count = self.count.saturating_add(n);
        self.first.get_or_insert(why);
    }
}

/// Pairs of numbers from an array, or why it isn't one: an entry that isn't a number, or an odd
/// count, would pair every later point with the wrong partner.
fn pairs(a: &Array, what: &str) -> std::result::Result<Vec<(f64, f64)>, String> {
    let n: Vec<f64> = a.iter::<f64>().collect();
    let (paired, rest) = n.as_chunks::<2>();
    if n.len() != a.raw_iter().count() || !rest.is_empty() {
        return Err(format!("its {what} aren't pairs of numbers"));
    }
    Ok(paired.iter().map(|&[x, y]| (x, y)).collect())
}

/// One GeoPDF viewport, or why it can't be used.
fn viewport(vp: &Dict<'_>, measure: &Dict<'_>) -> std::result::Result<Viewport, String> {
    // Every entry a number, so one that isn't can't shift the corners.
    let bbox = vp.get::<Array>(b"BBox".as_slice());
    let numbers: Vec<f64> = bbox.as_ref().map(|a| a.iter::<f64>().collect()).unwrap_or_default();
    let entries = bbox.as_ref().map_or(0, |a| a.raw_iter().count());
    let bbox: [f64; 4] = numbers.try_into().ok().filter(|_| entries == 4).ok_or_else(|| "its rectangle isn't four numbers".to_string())?;
    let gpts = measure.get::<Array>(b"GPTS".as_slice()).map(|a| pairs(&a, "control points (/GPTS)")).transpose()?.unwrap_or_default();
    // Where the points lie in the rectangle.
    let lpts = measure
        .get::<Array>(b"LPTS".as_slice())
        .ok_or_else(|| "it doesn't say where its control points lie in it (/LPTS)".to_string())
        .and_then(|a| pairs(&a, "control points' places (/LPTS)"))?;
    if gpts.len() < 3 || gpts.len() != lpts.len() {
        return Err("its control points are missing or don't pair up".into());
    }
    let all = bbox.iter().chain(gpts.iter().flat_map(|(a, b)| [a, b])).chain(lpts.iter().flat_map(|(a, b)| [a, b]));
    if !all.into_iter().all(|v| v.is_finite()) {
        return Err("a number in it isn't one".into());
    }
    let gcs = measure.get::<Dict>(b"GCS".as_slice()).ok_or("it doesn't say its coordinate system")?;
    let epsg = gcs.get::<i64>(b"EPSG".as_slice()).and_then(|c| u32::try_from(c).ok());
    let (text, unread) =
        gcs.get::<PdfString>(b"WKT".as_slice()).map(|w| text_string(w.as_bytes())).map_or((None, 0), |(t, n)| (Some(t), n));
    // As a `.prj` and a code are read together elsewhere (a GeoPackage's): the code, unless the
    // text gives another shift or body; the text alone when the code isn't one RustGIS knows.
    let unknown = |code: u32| format!("its coordinate system, EPSG:{code}, isn't one RustGIS knows");
    let unreadable = |e: String| format!("its coordinate system can't be read ({e})");
    let ((crs, note), text_used) = match (text.as_deref(), epsg) {
        (Some(t), Some(code)) => match (wkt::read_wkt_over_code(t, code), Crs::from_epsg(code)) {
            (Some(read), _) => (read.map_err(unreadable)?, true),
            (None, Some(crs)) => ((crs, None), false),
            (None, None) => (wkt::read_wkt(t).map_err(|_| unknown(code))?, true),
        },
        (Some(t), None) => (wkt::read_wkt(t).map_err(unreadable)?, true),
        (None, Some(code)) => ((Crs::from_epsg(code).ok_or_else(|| unknown(code))?, None), false),
        (None, None) => return Err("it doesn't say its coordinate system".into()),
    };
    // What couldn't be read of the text is said only where the text placed the page.
    let text_note = (text_used && unread > 0).then(|| {
        let chars = if unread == 1 { "1 character".to_string() } else { format!("{unread} characters") };
        format!("{chars} of its coordinate system's text (/WKT) couldn't be read: replaced or left out")
    });
    let def = match crs {
        Crs::Epsg(c) => crs::epsg_proj4(c).map(str::to_string),
        Crs::Custom(_) => crs.custom_def().map(|d| d.proj4.clone()),
        _ => None,
    };
    Ok(Viewport { bbox, gpts, lpts, crs, def, note, text_note })
}

/// PDFDocEncoding's characters for bytes 18–1F and 80–A0, where they differ from Latin-1's;
/// `None` where it defines none (ISO 32000-1:2008, Annex D, Table D.2).
const PDF_DOC_18: [char; 8] = ['\u{02D8}', '\u{02C7}', '\u{02C6}', '\u{02D9}', '\u{02DD}', '\u{02DB}', '\u{02DA}', '\u{02DC}'];
const PDF_DOC_80: [Option<char>; 33] = [
    Some('\u{2022}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{2026}'),
    Some('\u{2014}'),
    Some('\u{2013}'),
    Some('\u{0192}'),
    Some('\u{2044}'),
    Some('\u{2039}'),
    Some('\u{203A}'),
    Some('\u{2212}'),
    Some('\u{2030}'),
    Some('\u{201E}'),
    Some('\u{201C}'),
    Some('\u{201D}'),
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201A}'),
    Some('\u{2122}'),
    Some('\u{FB01}'),
    Some('\u{FB02}'),
    Some('\u{0141}'),
    Some('\u{0152}'),
    Some('\u{0160}'),
    Some('\u{0178}'),
    Some('\u{017D}'),
    Some('\u{0131}'),
    Some('\u{0142}'),
    Some('\u{0153}'),
    Some('\u{0161}'),
    Some('\u{017E}'),
    None,
    Some('\u{20AC}'),
];

/// A byte's character in PDFDocEncoding (ISO 32000-1:2008, Annex D, Table D.2), or `None` where
/// it defines none: 00–08, 0B, 0C, 0E–17, 7F, 9F and AD. Elsewhere it is Latin-1's but for
/// 18–1F and 80–A0.
fn pdf_doc_char(c: u8) -> Option<char> {
    match c {
        0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x17 | 0x7F | 0xAD => None,
        0x18..=0x1F => PDF_DOC_18.get(usize::from(c - 0x18)).copied(),
        0x80..=0xA0 => PDF_DOC_80.get(usize::from(c - 0x80)).copied().flatten(),
        _ => Some(char::from(c)),
    }
}

/// A PDF text string's text (ISO 32000-2 § 7.9.2.2), and how many characters couldn't be read
/// (each the replacement character, U+FFFD, or left out). UTF-16 after a byte-order mark (FE FF,
/// big-endian, as RustGIS writes one; or FF FE), UTF-8 after its own (EF BB BF), either with its
/// language escapes taken out ([`without_language_escapes`]). Without a mark, UTF-8 where the
/// bytes are that (as some writers use, though the standard says PDFDocEncoding), its characters
/// below U+0080 read as PDFDocEncoding; else PDFDocEncoding ([`pdf_doc_char`]). Either way a byte
/// PDFDocEncoding defines no character for is replaced.
fn text_string(b: &[u8]) -> (String, usize) {
    let (text, bad) = if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        crate::io::utf16_text(rest, true)
    } else if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        crate::io::utf16_text(rest, false)
    } else if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (mut text, mut bad) = (String::new(), 0);
        for chunk in rest.utf8_chunks() {
            text.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                text.push(char::REPLACEMENT_CHARACTER);
                bad += 1;
            }
        }
        (text, bad)
    } else if let Ok(s) = std::str::from_utf8(b) {
        // A character below U+0080 is its one byte in UTF-8, which PDFDocEncoding reads its own
        // way there: 18–1F are accents, and 00–08, 0B, 0C, 0E–17 and 7F are no character.
        let mut bad = 0;
        let text = s
            .chars()
            .map(|c| match u8::try_from(c) {
                Ok(byte) if byte < 0x80 => pdf_doc_char(byte).unwrap_or_else(|| {
                    bad += 1;
                    char::REPLACEMENT_CHARACTER
                }),
                _ => c,
            })
            .collect();
        return (text, bad);
    } else {
        let chars: Vec<Option<char>> = b.iter().map(|&c| pdf_doc_char(c)).collect();
        let bad = chars.iter().filter(|c| c.is_none()).count();
        return (chars.into_iter().map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect(), bad);
    };
    let (text, stray) = without_language_escapes(&text);
    (text, bad + stray)
}

/// `text` without its language escapes (ISO 32000-2 § 7.9.2.2): U+001B, a two-letter language
/// code, an optional two-letter country code, and U+001B. An escape mark that doesn't begin one
/// is taken out and counted, and the text after it kept.
fn without_language_escapes(text: &str) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (mut kept, mut stray, mut i) = (String::with_capacity(text.len()), 0, 0);
    while let Some(&c) = chars.get(i) {
        if c != '\u{1B}' {
            kept.push(c);
            i += 1;
            continue;
        }
        let letters = |from: usize, n: usize| chars.get(from..from + n).is_some_and(|s| s.iter().all(char::is_ascii_alphabetic));
        let closes = |at: usize| chars.get(at) == Some(&'\u{1B}');
        if letters(i + 1, 2) && closes(i + 3) {
            i += 4;
        } else if letters(i + 1, 4) && closes(i + 5) {
            i += 6;
        } else {
            stray += 1;
            i += 1;
        }
    }
    (kept, stray)
}

/// `r`, the drawn page at `scale` pixels a point, placed by the largest of its GeoPDF viewports:
/// left a scan when it has none, or none that can be used (said in `lost`).
fn place_by_viewport(mut r: Raster, page: &Page<'_>, scale: f64) -> Raster {
    let (mut found, unread) = viewports(page);
    let unplaced = "it opens without coordinates";
    // The OGC's GeoPDF encoding, older than ISO 32000's viewports, isn't read: said whenever the
    // page is left without coordinates. Its map frames are an entry of the page object only
    // (OGC 08-139r3, PDF Geo-registration Encoding Best Practice 2.2, § 6, Table 1), not of the
    // document catalog.
    let lgidict = page
        .raw()
        .contains_key(b"LGIDict".as_slice())
        .then(|| format!("its coordinates are in a form RustGIS doesn't read (OGC /LGIDict): {unplaced}"));
    if found.is_empty() {
        if let Some(why) = &unread.first {
            r.lost.push(format!("{} ({why}, the first): {unplaced}", unusable_frames(unread.count)));
        }
        r.lost.extend(lgidict);
        return r;
    }
    // The largest first; one that can't be fitted or placed gives way to the next.
    found.sort_by(|a, b| b.area().total_cmp(&a.area()));
    let (count, mut failed) = (found.len(), Vec::new());
    let to_pixels = Affine::scale(scale) * page.initial_transform(true).to_kurbo();
    for v in found {
        let (wf, off) = match fit(&v, to_pixels, r.width, r.height) {
            Ok(fitted) => fitted,
            Err(why) => {
                failed.push(why);
                continue;
            }
        };
        r = match r.placed_by_world_file(&wf, v.crs) {
            Ok(mut placed) => {
                let others = count - 1;
                if others > 0 {
                    let by = if failed.is_empty() { "the largest" } else { "the largest that can be used" };
                    placed.lost.push(format!("it has {others} more map frames with coordinates (insets, say): it is placed by {by}"));
                }
                let unusable = failed.len() + unread.count;
                if let Some(why) = failed.first().or(unread.first.as_ref()) {
                    placed.lost.push(format!("{unusable} of its map frames have coordinates that can't be used ({why}, the first)"));
                }
                placed.lost.extend(v.text_note);
                placed.lost.extend(v.note);
                if off > 1.0 {
                    placed.lost.push(format!("its map frame's corners don't lie on one flat grid: placed to within {off:.0} pixels"));
                }
                return placed;
            }
            Err((back, why)) => {
                failed.push(why);
                *back
            }
        };
    }
    if let Some(why) = failed.first().or(unread.first.as_ref()) {
        r.lost.push(format!("{} ({why}, the first): {unplaced}", unusable_frames(failed.len() + unread.count)));
    }
    r.lost.extend(lgidict);
    r
}

/// That `n` of a page's map frames have coordinates that can't be used, said with the count.
fn unusable_frames(n: usize) -> String {
    if n == 1 {
        "1 map frame has coordinates that can't be used".into()
    } else {
        format!("{n} map frames have coordinates that can't be used")
    }
}

/// The world file placing a `w` × `h` drawing of the page by viewport `v` (`to_pixels` takes a
/// point on the page to the drawing's pixels): the least-squares affine through its control
/// points in its coordinate system, and how far, in pixels, the farthest lies from it.
fn fit(v: &Viewport, to_pixels: Affine, w: u32, h: u32) -> std::result::Result<(world::WorldFile, f64), String> {
    let t = v.crs.transformer()?;
    let [x0, y0, x1, y1] = v.bbox;
    // Each control point's pixel and its WGS 84 longitude/latitude.
    let mut at = Vec::with_capacity(v.gpts.len());
    for (&(lat, lon), &(lx, ly)) in v.gpts.iter().zip(&v.lpts) {
        let on = Coord::new(lon, lat);
        let wgs84 = match &v.def {
            Some(def) => crs::lonlat_from_datum_of(def, on).ok_or("a control point is off the Earth")?,
            None => on,
        };
        let p = to_pixels * Point::new(x0 + lx * (x1 - x0), y0 + ly * (y1 - y0));
        at.push(((p.x, p.y), wgs84));
    }
    // The frame's longitudes made continuous across it, taking the points left to right on the
    // page, whatever order the file gives them in: across 180°, 179° and -179° are 2° apart, not
    // 358°, and a frame with points at -170°, 0° and 170° stays 340° wide.
    at.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.total_cmp(&b.0.1)));
    let mut previous: Option<f64> = None;
    for (_, c) in &mut at {
        if let Some(reference) = previous {
            c.x = reference + (c.x - reference + 180.0).rem_euclid(360.0) - 180.0;
        }
        previous = Some(c.x);
    }
    let mut points = Vec::with_capacity(at.len());
    for (p, wgs84) in at {
        let ground = t.from_lonlat(wgs84).ok_or("a control point is outside its coordinate system")?;
        points.push((p, (ground.x, ground.y)));
    }
    let [a, b, c] = solve(&points, |g| g.0).ok_or("its control points lie in a line")?;
    let [d, e, f] = solve(&points, |g| g.1).ok_or("its control points lie in a line")?;
    let size = (a * e - b * d).abs().sqrt();
    if !(size.is_normal() && [a, b, c, d, e, f].iter().all(|v| v.is_finite())) {
        return Err("its control points place the page nowhere".into());
    }
    let off =
        points.iter().map(|&((px, py), (gx, gy))| (a * px + b * py + c - gx).hypot(d * px + e * py + f - gy) / size).fold(0.0, f64::max);
    // A turn of less than a thousandth of a pixel across the whole drawing is none: the picture is
    // then placed north up as drawn, not resampled.
    let (w, h) = (f64::from(w), f64::from(h));
    let (b, d) = if (b * h).abs() < 1e-3 * a.abs() && (d * w).abs() < 1e-3 * e.abs() { (0.0, 0.0) } else { (b, d) };
    // The world file gives the centre of the top-left pixel.
    Ok((world::WorldFile { a, d, b, e, c: c + (a + b) / 2.0, f: f + (d + e) / 2.0 }, off))
}

/// The least-squares `[p, q, r]` with `p·x + q·y + r` nearest `value(ground)` at each `(x, y)`.
fn solve(points: &[((f64, f64), (f64, f64))], value: impl Fn((f64, f64)) -> f64) -> Option<[f64; 3]> {
    // The normal equations, solved by Cramer's rule. Pixel positions are centred first, so the
    // sums stay well conditioned on a large page.
    let n = points.len() as f64;
    let (mx, my) = points.iter().fold((0.0, 0.0), |(sx, sy), ((x, y), _)| (sx + x / n, sy + y / n));
    let (mut sxx, mut sxy, mut syy, mut sxv, mut syv, mut sv) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for &((x, y), g) in points {
        let (x, y, v) = (x - mx, y - my, value(g));
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sxv += x * v;
        syv += y * v;
        sv += v;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() <= 1e-12 * (sxx * syy).max(f64::MIN_POSITIVE) {
        return None;
    }
    let p = (sxv * syy - syv * sxy) / det;
    let q = (syv * sxx - sxv * sxy) / det;
    let r = sv / n - p * mx - q * my;
    Some([p, q, r])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fit_through_exact_points_is_exact_and_points_in_a_line_have_none() {
        // 2 units a pixel east, 3 south, from (100, 500).
        let at = |x: f64, y: f64| ((x, y), (100.0 + 2.0 * x, 500.0 - 3.0 * y));
        let pts = [at(0.0, 0.0), at(10.0, 0.0), at(10.0, 20.0), at(0.0, 20.0)];
        let [a, b, c] = solve(&pts, |g| g.0).unwrap();
        let [d, e, f] = solve(&pts, |g| g.1).unwrap();
        for (got, want) in [a, b, c, d, e, f].iter().zip([2.0, 0.0, 100.0, 0.0, -3.0, 500.0]) {
            assert!((got - want).abs() < 1e-9, "{got} for {want}");
        }
        assert!(solve(&[at(0.0, 0.0), at(1.0, 1.0), at(2.0, 2.0)], |g| g.0).is_none());
        assert!(super::super::is_pdf_name("/d/Map.PDF") && !super::super::is_pdf_name("map.pdf.tif"));
    }

    /// A one-page PDF of `media` points with a blue square drawn, its page dictionary given
    /// `extra` (a `/VP`, say), with a correct cross-reference table.
    fn tiny_pdf(media: &str, extra: &str) -> Vec<u8> {
        pdf_of(media, extra, b"0 0 1 rg 0 0 10 10 re f", &[])
    }

    /// A one-page PDF of `media` points drawing `content`, its page dictionary given `extra`, and
    /// objects `more` numbered from 5, with a correct cross-reference table.
    fn pdf_of(media: &str, extra: &str, content: &[u8], more: &[Vec<u8>]) -> Vec<u8> {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [{media}] /Contents 4 0 R {extra} >>").into_bytes(),
            stream("", content),
        ];
        objects.extend_from_slice(more);
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut at = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            at.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for a in at {
            out.extend_from_slice(format!("{a:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }

    /// A stream object of dictionary entries `dict` holding `data`.
    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut o = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        o.extend_from_slice(data);
        o.extend_from_slice(b"\nendstream");
        o
    }

    /// A 72 × 36 point page drawing image object 5, of dictionary entries `dict` and `data`, by
    /// `content`.
    fn image_page(dict: &str, data: &[u8], content: &[u8]) -> Vec<u8> {
        let image = stream(&format!("/Type /XObject /Subtype /Image {dict}"), data);
        pdf_of("0 0 72 36", "/Resources << /XObject << /Im0 5 0 R >> >>", content, &[image])
    }

    /// A 72 × 36 point page showing text in font object 5, of dictionary `font`, by `content`.
    fn text_page(font: &str, content: &[u8]) -> Vec<u8> {
        pdf_of("0 0 72 36", "/Resources << /Font << /F1 5 0 R >> >>", content, &[font.as_bytes().to_vec()])
    }

    #[test]
    fn what_cant_be_drawn_is_said_with_how_often() {
        // An image whose JPEG data is junk, drawn once and then twice: left blank, and said with
        // the times it is drawn.
        let junk = "/Width 8 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode";
        let once = b"q 72 0 0 36 0 0 cm /Im0 Do Q";
        let image = "an image or its mask can't be decoded: the image is left blank, or drawn without its mask";
        let r = read_bytes("x", image_page(junk, b"not a jpeg at all", once), None, true).unwrap();
        assert_eq!(r.lost, [format!("{image} (once)")]);
        let twice = b"q 36 0 0 36 0 0 cm /Im0 Do Q q 36 0 0 36 36 0 cm /Im0 Do Q";
        let r = read_bytes("x", image_page(junk, b"not a jpeg at all", twice), None, true).unwrap();
        assert_eq!(r.lost, [format!("{image} (2 times)")]);
        // A sound image whose soft mask is junk: drawn without the mask, and said the same way.
        let red = [255u8, 0, 0].repeat(4);
        let masked =
            stream("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 6 0 R", &red);
        let mask = stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode",
            b"junk",
        );
        let r =
            read_bytes("x", pdf_of("0 0 72 36", "/Resources << /XObject << /Im0 5 0 R >> >>", once, &[masked, mask]), None, true).unwrap();
        assert_eq!(r.lost, [format!("{image} (once)")]);
        // A font of a kind that doesn't exist, chosen once and then twice: its text drawn in a
        // stand-in, and said with the times it is chosen, shown or not.
        let nonsense = "<< /Type /Font /Subtype /Nonsense /BaseFont /Bogus >>";
        let font = "a font that can't be read is chosen: its text is drawn in a stand-in font";
        let r = read_bytes("x", text_page(nonsense, b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET"), None, true).unwrap();
        assert_eq!(r.lost, [format!("{font} (once)")]);
        let r = read_bytes("x", text_page(nonsense, b"BT /F1 12 Tf 5 10 Td (Hi) Tj /F1 9 Tf ET"), None, true).unwrap();
        assert_eq!(r.lost, [format!("{font} (2 times)")]);
        // A font not in the file (no program, not one of the standard 14): drawn in a stand-in,
        // and named.
        let arial = "<< /Type /Font /Subtype /TrueType /BaseFont /Arial,Bold /Encoding /WinAnsiEncoding >>";
        let r = read_bytes("x", text_page(arial, b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET"), None, true).unwrap();
        assert_eq!(r.lost, ["fonts not in the file are drawn in a stand-in font: Arial,Bold"]);
        // An annotation whose appearance can't be found: left out, and said.
        let annot = "/Annots [<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /D 9 >> >>]";
        let r = read_bytes("x", tiny_pdf("0 0 72 36", annot), None, true).unwrap();
        assert_eq!(r.lost, ["an annotation whose appearance can't be found is left out (once)"]);
        // The same kinds, drawable: nothing said. Helvetica, one of the standard 14, isn't a
        // stand-in for anything.
        let r =
            read_bytes("x", image_page("/Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8", &red, once), None, true).unwrap();
        assert!(r.lost.is_empty(), "{:?}", r.lost);
        assert_eq!((r.bands[0].data[150 * 37 + 75], r.bands[1].data[150 * 37 + 75]), (255.0, 0.0));
        let helvetica = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
        let r = read_bytes("x", text_page(helvetica, b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET"), None, true).unwrap();
        assert!(r.lost.is_empty(), "{:?}", r.lost);
    }

    #[test]
    fn an_ogc_lgidict_page_says_its_coordinates_arent_read() {
        let lgi = "<< /Type /LGIDict /Version (2.1) /CTM [(0.0277777778) (0) (0) (0.0277777778) (10) (40)] \
                   /Projection << /Type /Projection /ProjectionType (GEOGRAPHIC) /Datum (WGE) >> >>";
        for extra in [format!("/LGIDict [{lgi}]"), format!("/LGIDict {lgi}")] {
            let r = read_bytes("lgi", tiny_pdf("0 0 72 36", &extra), None, true).unwrap();
            assert!(!r.georeferenced, "{extra}");
            assert_eq!(r.lost, ["its coordinates are in a form RustGIS doesn't read (OGC /LGIDict): it opens without coordinates"]);
            // Opened as a scan, nothing is said.
            let r = read_bytes("lgi", tiny_pdf("0 0 72 36", &extra), None, false).unwrap();
            assert!(r.lost.is_empty(), "{:?}", r.lost);
        }
        // An ISO viewport beside it places the page, and nothing is said of the other.
        let both = format!("/LGIDict {lgi} {}", geo("/Type /GEOGCS /EPSG 4326"));
        let r = read_bytes("lgi", tiny_pdf("0 0 72 36", &both), None, true).unwrap();
        assert!(r.georeferenced && r.lost.is_empty(), "{:?}", r.lost);
        // One that can't be used (it gives no system): both are said.
        let both = format!("/LGIDict {lgi} {}", geo("/Type /GEOGCS"));
        let r = read_bytes("lgi", tiny_pdf("0 0 72 36", &both), None, true).unwrap();
        assert!(!r.georeferenced && r.lost.len() == 2, "{:?}", r.lost);
        assert!(r.lost[0].starts_with("1 map frame has coordinates that can't be used"), "{:?}", r.lost);
        assert!(r.lost[1].contains("(OGC /LGIDict)"), "{:?}", r.lost);
    }

    #[test]
    fn a_utf16_coordinate_system_is_read() {
        let hex16 = |s: &str| format!("<FEFF{}>", s.encode_utf16().map(|u| format!("{u:04X}")).collect::<String>());
        // As RustGIS writes a GeoPDF of a `local:10,45` map: its label has degree signs, so its
        // text is UTF-16, and it has no EPSG code to fall back on.
        let def = "+proj=aeqd +lat_0=45 +lon_0=10 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs";
        let own = wkt::proj4_to_wkt(def, "Local (10.00°, 45.00°)").unwrap();
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(&format!("/Type /PROJCS /WKT {}", hex16(&own)))), None, true).unwrap();
        assert!(r.georeferenced, "{:?}", r.lost);
        let e = r.extent();
        assert!(e.min_x < e.max_x && e.min_y < e.max_y, "{e:?}");
        assert!(r.lost.is_empty(), "{:?}", r.lost);
        // The same with an odd last byte: placed, and what couldn't be read said.
        let odd = hex16(&own).replace('>', "00>");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(&format!("/Type /PROJCS /WKT {odd}"))), None, true).unwrap();
        assert!(r.georeferenced, "{:?}", r.lost);
        assert_eq!(r.lost, ["1 character of its coordinate system's text (/WKT) couldn't be read: replaced or left out"]);
        // Text strings in their other forms, and what each couldn't read.
        let read = |b: &[u8]| text_string(b);
        assert_eq!(read(&[0xFF, 0xFE, b'G', 0, 0xB0, 0]), ("G°".into(), 0));
        assert_eq!(read(&[0xEF, 0xBB, 0xBF, b'G', 0xC2, 0xB0]), ("G°".into(), 0));
        assert_eq!(read(&[b'G', 0xC2, 0xB0]), ("G°".into(), 0));
        assert_eq!(read(&[b'G', 0xB0]), ("G°".into(), 0));
        // An odd last byte, a lone surrogate, and bytes that aren't UTF-8 after its mark.
        assert_eq!(read(&[0xFE, 0xFF, 0, b'G', 0]), ("G".into(), 1));
        assert_eq!(read(&[0xFE, 0xFF, 0xD8, 0x00, 0, b'G']), ("\u{FFFD}G".into(), 1));
        assert_eq!(read(&[0xEF, 0xBB, 0xBF, b'G', 0xFF]), ("G\u{FFFD}".into(), 1));
        // Language escapes (a language, or a language and a country) are taken out, and what is
        // in them isn't counted. A mark that begins none is taken out and counted, and the text
        // after it kept.
        let utf16 = |s: &str| -> Vec<u8> { [0xFE, 0xFF].into_iter().chain(s.encode_utf16().flat_map(u16::to_be_bytes)).collect() };
        assert_eq!(read(&utf16("a\u{1B}en\u{1B}b\u{1B}deCH\u{1B}c")), ("abc".into(), 0));
        assert_eq!(read(&utf16("a\u{1B}en")), ("aen".into(), 1));
        assert_eq!(read(&utf16("a\u{1B}e1\u{1B}b")), ("ae1b".into(), 2));
        // Without a mark and not UTF-8: PDFDocEncoding, as ISO 32000-1 Table D.2 gives it (0x80 a
        // bullet, 0xA0 the euro sign, 0x18 a breve), and a byte it defines no character for
        // (0xAD) replaced.
        assert_eq!(read(&[b'G', 0xFF, 0xE9, 0x80, 0xA0, 0x18, 0xAD]), ("Gÿé•€˘\u{FFFD}".into(), 1));
        // Without a mark and UTF-8: the bytes below 0x80 are still PDFDocEncoding's, so 0x18 is a
        // breve and 0x7F, which it defines no character for, is replaced and counted, beside
        // UTF-8's own characters; tab, line feed and carriage return are kept.
        assert_eq!(read(b"G\x18"), ("G\u{02D8}".into(), 0));
        assert_eq!(read(b"G\x7F"), ("G\u{FFFD}".into(), 1));
        assert_eq!(read(&[b'G', 0x18, 0xC2, 0xB0]), ("G\u{02D8}°".into(), 0));
        assert_eq!(read(b"a\tb\nc\r"), ("a\tb\nc\r".into(), 0));
    }

    #[test]
    fn a_coordinate_systems_unreadable_text_is_said_only_where_it_is_used() {
        let ascii = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;
        // UTF-16 with an odd last byte.
        let odd = format!("<FEFF{}00>", ascii.encode_utf16().map(|u| format!("{u:04X}")).collect::<String>());
        let note = "1 character of its coordinate system's text (/WKT) couldn't be read: replaced or left out";
        // The text alone places the page: said.
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(&format!("/Type /GEOGCS /WKT {odd}"))), None, true).unwrap();
        assert!(r.georeferenced && r.lost == [note], "{:?}", r.lost);
        // A code it gives no other shift or body than places it: the text isn't used, and
        // nothing is said of it.
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(&format!("/Type /GEOGCS /EPSG 4326 /WKT {odd}"))), None, true).unwrap();
        assert!(r.georeferenced && r.lost.is_empty(), "{:?}", r.lost);
    }

    #[test]
    fn fonts_not_in_the_file_are_named_where_they_are_chosen() {
        let notice = |names: &str| format!("fonts not in the file are drawn in a stand-in font: {names}");
        let arial = "<< /Type /Font /Subtype /TrueType /BaseFont /Arial,Bold /Encoding /WinAnsiEncoding >>";
        let helvetica = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
        // Text in Helvetica (a standard font) with Arial in the resources but never chosen:
        // nothing said of Arial.
        let unused = pdf_of(
            "0 0 72 36",
            "/Resources << /Font << /F1 5 0 R /F2 6 0 R >> >>",
            b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET",
            &[helvetica.as_bytes().to_vec(), arial.as_bytes().to_vec()],
        );
        let r = read_bytes("x", unused, None, true).unwrap();
        assert!(r.lost.is_empty(), "{:?}", r.lost);
        // Two spellings of one font, both chosen: named once.
        let mt = "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Arial-BoldMT /Encoding /WinAnsiEncoding >>";
        let both = pdf_of(
            "0 0 72 36",
            "/Resources << /Font << /F1 5 0 R /F2 6 0 R >> >>",
            b"BT /F1 12 Tf 5 10 Td (Hi) Tj /F2 12 Tf (there) Tj ET",
            &[arial.as_bytes().to_vec(), mt.as_bytes().to_vec()],
        );
        let r = read_bytes("x", both, None, true).unwrap();
        assert_eq!(r.lost, [notice("Arial,Bold")]);
        // Chosen only inside an annotation's appearance, a soft-mask group or a tiling pattern:
        // named.
        let text = b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET";
        let with_font =
            |kind: &str| format!("/Type /XObject /Subtype /Form /BBox [0 0 72 36] /Resources << /Font << /F1 6 0 R >> >> {kind}");
        let font = arial.as_bytes().to_vec();
        let appearance = stream(&with_font(""), text);
        let annotated = pdf_of(
            "0 0 72 36",
            "/Annots [<< /Type /Annot /Subtype /Square /Rect [0 0 72 36] /AP << /N 5 0 R >> >>]",
            b"",
            &[appearance, font.clone()],
        );
        assert_eq!(read_bytes("x", annotated, None, true).unwrap().lost, [notice("Arial,Bold")]);
        let group = stream(&with_font("/Group << /S /Transparency /CS /DeviceGray >>"), text);
        let masked = pdf_of(
            "0 0 72 36",
            "/Resources << /ExtGState << /G0 << /SMask << /S /Luminosity /G 5 0 R >> >> >> >>",
            b"/G0 gs 0 0 1 rg 0 0 10 10 re f",
            &[group, font.clone()],
        );
        assert_eq!(read_bytes("x", masked, None, true).unwrap().lost, [notice("Arial,Bold")]);
        let tile = stream(
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 72 36] /XStep 72 /YStep 36 \
             /Resources << /Font << /F1 6 0 R >> >>",
            text,
        );
        let patterned =
            pdf_of("0 0 72 36", "/Resources << /Pattern << /P0 5 0 R >> >>", b"/Pattern cs /P0 scn 0 0 72 36 re f", &[tile, font.clone()]);
        assert_eq!(read_bytes("x", patterned, None, true).unwrap().lost, [notice("Arial,Bold")]);
        let type3 = "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] /FontMatrix [0.001 0 0 0.001 0 0] \
                     /CharProcs << /H 6 0 R >> /Encoding << /Differences [72 /H] >> /FirstChar 72 /LastChar 72 /Widths [1000] \
                     /Resources << /Font << /F1 7 0 R >> >> >>";
        let glyph = stream("", b"1000 0 d0 BT /F1 500 Tf 0 0 Td (x) Tj ET");
        let glyphs = pdf_of(
            "0 0 72 36",
            "/Resources << /Font << /T3 5 0 R >> >>",
            b"BT /T3 12 Tf 5 10 Td (H) Tj ET",
            &[type3.as_bytes().to_vec(), glyph, font],
        );
        // Chosen inside a Type 3 font's glyph: not named, as its glyphs aren't decoded for the
        // search (a bound on its work), but said as maybe.
        assert_eq!(
            read_bytes("x", glyphs, None, true).unwrap().lost,
            ["fonts not in the file may be drawn in a stand-in font (inside Type 3 fonts, whose glyphs aren't looked through)"]
        );
        assert_eq!(font_key("Arial,Bold"), font_key("Arial-BoldMT"));
        assert_eq!(font_key("TimesNewRomanPSMT"), font_key("TimesNewRoman"));
        assert_eq!(font_key("TimesNewRomanPS"), font_key("TimesNewRoman"));
        assert_ne!(font_key("Arial"), font_key("TimesNewRoman"));
    }

    /// A font `hayro` draws in a stand-in is named however it comes to: its program in the file
    /// can't be loaded, it has no name, or `Tf` names one the resources don't have. A program
    /// `hayro` loads, a named standard font, and a Type 3 font without twins say nothing (#2352).
    #[test]
    fn stand_ins_without_a_missing_program_are_named() {
        let stand_in = |names: &str| format!("fonts not in the file are drawn in a stand-in font: {names}");
        let descriptor = |file: &str| {
            format!(
                "<< /Type /FontDescriptor /FontName /Arial /Flags 32 /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 \
                 /Descent -200 /CapHeight 700 /StemV 80 {file} >>"
            )
        };
        let text = b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET";
        let page_with = |font: &str, more: &[Vec<u8>]| {
            let mut objects = vec![font.as_bytes().to_vec()];
            objects.extend_from_slice(more);
            pdf_of("0 0 72 36", "/Resources << /Font << /F1 5 0 R >> >>", text, &objects)
        };
        // A TrueType program that is junk: hayro draws a stand-in, said.
        let junk = stream("", b"not a font program at all");
        let font = format!("<< /Type /Font /Subtype /TrueType /BaseFont /Arial /FontDescriptor {} >>", descriptor("/FontFile2 6 0 R"));
        let r = read_bytes("x", page_with(&font, &[junk]), None, true).unwrap();
        assert_eq!(r.lost, [stand_in("Arial (its program in the file can't be read)")]);
        // A Type 1 program hayro loads (its own Helvetica's): nothing said.
        let (program, _) = hayro::hayro_interpret::font::StandardFont::Helvetica.get_font_data();
        let program = stream("", (*program).as_ref());
        let font = format!("<< /Type /Font /Subtype /Type1 /BaseFont /Arial /FontDescriptor {} >>", descriptor("/FontFile 6 0 R"));
        let pdf = page_with(&font, &[program]);
        let loaded = Pdf::new(pdf.clone()).unwrap();
        let pages = loaded.pages();
        let dict = pages.first().unwrap().resources().fonts.get::<Dict<'_>>(b"F1".as_slice()).unwrap();
        assert!(!program_rejected(&dict));
        assert!(read_bytes("x", pdf, None, true).unwrap().lost.is_empty());
        // A TrueType font without a /BaseFont, not in the file: named as such.
        let r = read_bytes("x", page_with("<< /Type /Font /Subtype /TrueType /Encoding /WinAnsiEncoding >>", &[]), None, true).unwrap();
        assert_eq!(r.lost, [stand_in("a font without a name")]);
        // Tf naming a font the resources don't have: drawn in Helvetica, said.
        let missing = pdf_of("0 0 72 36", "/Resources << /Font << >> >>", b"BT /F9 12 Tf 5 10 Td (Hi) Tj ET", &[]);
        assert_eq!(read_bytes("x", missing, None, true).unwrap().lost, [stand_in("/F9 (not in its resources)")]);
        // Helvetica named: nothing.
        let r = read_bytes("x", page_with("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", &[]), None, true).unwrap();
        assert!(r.lost.is_empty(), "{:?}", r.lost);
    }

    /// An annotation appearance without a box isn't drawn, so isn't looked at; a font already
    /// counted at the limit of fonts told apart doesn't stop the search (#2352).
    #[test]
    fn the_font_search_skips_boxless_appearances_and_recounts_nothing() {
        let arial = b"<< /Type /Font /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding >>".to_vec();
        let text = b"BT /F1 12 Tf (Hi) Tj ET";
        for (bbox, named) in [("/BBox [0 0 72 36]", 1), ("", 0), ("/BBox [0 0 72]", 0)] {
            let appearance = stream(&format!("/Type /XObject /Subtype /Form {bbox} /Resources << /Font << /F1 6 0 R >> >>"), text);
            let pdf = pdf_of(
                "0 0 72 36",
                "/Annots [<< /Type /Annot /Subtype /Square /Rect [0 0 72 36] /AP << /N 5 0 R >> >>]",
                b"",
                &[appearance, arial.clone()],
            );
            assert_eq!(search(pdf, 1 << 20, 1000).0.len(), named, "{bbox}");
        }
        let mut found = FontSearch::default();
        for i in 0..MISSING_FONTS_COUNTED {
            found.named(format!("Font{i}"));
        }
        found.named("Font0".into());
        assert!(!found.stopped);
        found.named("Another".into());
        assert!(found.stopped);
    }

    /// A font a graphics state chooses (`/Font [font size]`) is named as one `Tf` chooses; a form
    /// without resources of its own is looked at with each drawer's; an annotation without a
    /// rectangle isn't drawn, so isn't looked at (#2352).
    #[test]
    fn the_font_search_follows_graphics_states_inherited_resources_and_rectangles() {
        let notice = "fonts not in the file are drawn in a stand-in font: Arial";
        let arial = b"<< /Type /Font /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding >>".to_vec();
        let helvetica = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec();
        // Chosen by a graphics state: named; the same text by Helvetica's: nothing said.
        let by_state = |font: Vec<u8>| {
            pdf_of("0 0 72 36", "/Resources << /ExtGState << /GS0 << /Font [5 0 R 12] >> >> >>", b"/GS0 gs BT 5 10 Td (Hi) Tj ET", &[font])
        };
        assert_eq!(read_bytes("x", by_state(arial.clone()), None, true).unwrap().lost, [notice]);
        assert!(read_bytes("x", by_state(helvetica.clone()), None, true).unwrap().lost.is_empty());
        // Form X has no resources and chooses F1: the page's F1 is Helvetica, form Y's is Arial.
        let x = stream("/Type /XObject /Subtype /Form /BBox [0 0 72 36]", b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET");
        let y = stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 72 36] /Resources << /Font << /F1 8 0 R >> /XObject << /X 6 0 R >> >>",
            b"/X Do",
        );
        let drawn_by_both = pdf_of(
            "0 0 72 36",
            "/Resources << /Font << /F1 5 0 R >> /XObject << /X 6 0 R /Y 7 0 R >> >>",
            b"/X Do /Y Do",
            &[helvetica.clone(), x.clone(), y, arial.clone()],
        );
        assert_eq!(search(drawn_by_both, 1 << 20, 1000).0, ["Arial"]);
        // Drawn by the page alone: Helvetica only, nothing named.
        let by_page = pdf_of("0 0 72 36", "/Resources << /Font << /F1 5 0 R >> /XObject << /X 6 0 R >> >>", b"/X Do", &[helvetica, x]);
        assert!(search(by_page, 1 << 20, 1000).0.is_empty());
        // An annotation's appearance choosing Arial: looked at with a rectangle, not without one.
        let with_font =
            stream("/Type /XObject /Subtype /Form /BBox [0 0 72 36] /Resources << /Font << /F1 6 0 R >> >>", b"BT /F1 12 Tf (Hi) Tj ET");
        for (rect, named) in [("/Rect [0 0 72 36]", 1), ("", 0), ("/Rect [0 0 72]", 0)] {
            let pdf = pdf_of(
                "0 0 72 36",
                &format!("/Annots [<< /Type /Annot /Subtype /Square {rect} /AP << /N 5 0 R >> >>]"),
                b"",
                &[with_font.clone(), arial.clone()],
            );
            assert_eq!(search(pdf, 1 << 20, 1000).0.len(), named, "{rect}");
        }
    }

    /// The font search follows what `hayro` draws: not a hidden annotation or one not for the
    /// screen, of an appearance with states only the one `/AS` names (else `Off`), and a tiling
    /// pattern without resources of its own with none (#2352).
    #[test]
    fn the_font_search_looks_only_at_what_is_drawn() {
        let arial = b"<< /Type /Font /Subtype /TrueType /BaseFont /Arial,Bold /Encoding /WinAnsiEncoding >>".to_vec();
        let text = b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET";
        let form = |resources: &str| format!("/Type /XObject /Subtype /Form /BBox [0 0 72 36] {resources}");
        let with_font = form("/Resources << /Font << /F1 7 0 R >> >>");
        let names = |pdf: Vec<u8>| search(pdf, 1 << 20, 1000).0;
        // Annotations: shown, hidden (F 2), not for the screen (F 32).
        for (flags, named) in [(0, true), (2, false), (32, false)] {
            let pdf = pdf_of(
                "0 0 72 36",
                &format!("/Annots [<< /Type /Annot /Subtype /Square /F {flags} /Rect [0 0 72 36] /AP << /N 5 0 R >> >>]"),
                b"",
                &[stream(&with_font, text), stream(&with_font, b""), arial.clone()],
            );
            assert_eq!(names(pdf).len(), usize::from(named), "flags {flags}");
        }
        // Appearance states: /On chooses Arial, /Off nothing; the one /AS names is looked at, else Off.
        for (state, named) in [("/AS /On", true), ("/AS /Off", false), ("", false)] {
            let pdf = pdf_of(
                "0 0 72 36",
                &format!("/Annots [<< /Type /Annot /Subtype /Widget /Rect [0 0 72 36] {state} /AP << /N << /On 5 0 R /Off 6 0 R >> >> >>]"),
                b"",
                &[stream(&with_font, text), stream(&with_font, b""), arial.clone()],
            );
            assert_eq!(names(pdf).len(), usize::from(named), "{state}");
        }
        // A tiling pattern choosing F1 without resources of its own doesn't find the page's F1 (Arial):
        // hayro draws it in Helvetica, as a font not in its resources.
        let tile = |resources: &str| {
            stream(
                &format!("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 72 36] /XStep 72 /YStep 36 {resources}"),
                text,
            )
        };
        for (resources, named) in [("/Resources << /Font << /F1 7 0 R >> >>", "Arial,Bold"), ("", "/F1 (not in its resources)")] {
            let pdf = pdf_of(
                "0 0 72 36",
                "/Resources << /Pattern << /P0 5 0 R >> /Font << /F1 7 0 R >> >>",
                b"/Pattern cs /P0 scn 0 0 72 36 re f",
                &[tile(resources), stream("", b""), arial.clone()],
            );
            assert_eq!(names(pdf), [named], "{resources}");
        }
    }

    /// A stand-in asked for where the search stops before naming it (a font chosen deeper in forms
    /// than it looks) is still said (#2352).
    #[test]
    fn a_stand_in_is_said_when_the_search_stops_before_naming_it() {
        let deep = FONT_SEARCH_DEPTH + 2;
        let font = 5 + deep;
        // Forms 5, 6, … each drawing the next; the last chooses Arial.
        let mut objects: Vec<Vec<u8>> = (0..deep)
            .map(|i| {
                let obj = 5 + i;
                let (resources, content) = if i + 1 == deep {
                    (format!("/Font << /F1 {font} 0 R >>"), b"BT /F1 12 Tf 5 10 Td (Hi) Tj ET".to_vec())
                } else {
                    (format!("/XObject << /X {} 0 R >>", obj + 1), b"/X Do".to_vec())
                };
                stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 72 36] /Resources << {resources} >>"), &content)
            })
            .collect();
        objects.push(b"<< /Type /Font /Subtype /TrueType /BaseFont /Arial,Bold /Encoding /WinAnsiEncoding >>".to_vec());
        let pdf = pdf_of("0 0 72 36", "/Resources << /XObject << /X 5 0 R >> >>", b"/X Do", &objects);
        let (names, stopped, ..) = search(pdf.clone(), 1 << 20, 1000);
        assert!(names.is_empty() && stopped, "{names:?} {stopped}");
        let lost = read_bytes("x", pdf, None, true).unwrap().lost;
        assert_eq!(lost, ["fonts not in the file may be drawn in a stand-in font (the search for them stopped early)"]);
    }

    /// The first page of `pdf`, searched for the fonts not in the file it chooses with `bytes` to
    /// decode and `work` to spend.
    fn search(pdf: Vec<u8>, bytes: usize, work: usize) -> (Vec<String>, bool, usize, usize) {
        let pdf = Pdf::new(pdf).unwrap();
        let pages = pdf.pages();
        let found = substituted_fonts(pages.first().unwrap(), bytes, work).unwrap();
        (found.names, found.stopped, found.decoded, found.streams)
    }

    #[test]
    fn the_font_search_stops_at_its_bytes_and_its_work() {
        let arial = |name: &str| format!("<< /Type /Font /Subtype /TrueType /BaseFont /{name} /Encoding /WinAnsiEncoding >>").into_bytes();
        // Two forms, each choosing a font not in the file, whose content is 1,000 bytes.
        let form = |font: u32| {
            let mut content = b"BT /A 12 Tf 5 10 Td (x) Tj ET".to_vec();
            content.resize(1000, b' ');
            stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 72 36] /Resources << /Font << /A {font} 0 R >> >>"), &content)
        };
        let pdf = pdf_of(
            "0 0 72 36",
            "/Resources << /XObject << /F1 5 0 R /F2 6 0 R >> >>",
            b"/F1 Do /F2 Do",
            &[form(7), form(8), arial("Arial"), arial("Verdana")],
        );
        // Room for both: both named.
        let (names, stopped, decoded, _) = search(pdf.clone(), 10_000, 1000);
        assert!(names.len() == 2 && !stopped && decoded == 2000, "{names:?} {stopped} {decoded}");
        // Room for one: one named, the search stopped, and nothing past the bytes decoded.
        let (names, stopped, decoded, _) = search(pdf.clone(), 1500, 1000);
        assert!(names.len() == 1 && stopped && decoded == 1000, "{names:?} {stopped} {decoded}");
        // Work for one font: once it is spent, the other form isn't decoded.
        let (names, stopped, decoded, _) = search(pdf, 10_000, 1);
        assert!(names.len() == 1 && stopped && decoded == 1000, "{names:?} {stopped} {decoded}");
        // A Type 3 font written in place (no object number) chosen 1,000 times: looked at once,
        // its glyphs not decoded; each choice spends from the work.
        let type3 = "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] /FontMatrix [0.001 0 0 0.001 0 0] \
                     /CharProcs << /H 5 0 R >> /Encoding << /Differences [72 /H] >> /FirstChar 72 /LastChar 72 /Widths [1000] >>";
        let content = format!("BT {} ET", "/T3 12 Tf (H) Tj ".repeat(1000));
        let pdf =
            pdf_of("0 0 72 36", &format!("/Resources << /Font << /T3 {type3} >> >>"), content.as_bytes(), &[stream("", b"1000 0 d0")]);
        let (names, stopped, decoded, streams) = search(pdf.clone(), 10_000, 2000);
        assert!(names.is_empty() && !stopped && decoded == 0 && streams == 0, "{names:?} {stopped} {decoded} {streams}");
        let (_, stopped, _, _) = search(pdf, 10_000, 10);
        assert!(stopped);
    }

    /// A viewport over `bbox` on the page, its corners at 10–12° E, 40–41° N.
    fn vp(gcs: &str, bbox: &str) -> String {
        format!(
            "<< /Type /Viewport /BBox [{bbox}] /Measure << /Type /Measure /Subtype /GEO \
             /GPTS [41 10 41 12 40 12 40 10] /LPTS [0 1 1 1 1 0 0 0] /GCS << {gcs} >> >> >>"
        )
    }

    /// A viewport over the whole of a 72 × 36 point page.
    fn geo(gcs: &str) -> String {
        format!("/VP [{}]", vp(gcs, "0 0 72 36"))
    }

    #[test]
    fn a_page_without_coordinates_is_a_scan_drawn_at_150_dpi() {
        let r = read_bytes("plain", tiny_pdf("0 0 72 36", ""), None, true).unwrap();
        assert_eq!((r.width, r.height, r.bands.len(), r.georeferenced), (150, 75, 3, false));
        assert!(r.lost.is_empty(), "{:?}", r.lost);
        // The blue square at the bottom left, white elsewhere.
        let at = |band: usize, col: usize, row: usize| r.bands[band].data[row * 150 + col];
        assert_eq!((at(0, 2, 72), at(1, 2, 72), at(2, 2, 72)), (0.0, 0.0, 255.0));
        assert_eq!((at(0, 100, 10), at(2, 100, 10)), (255.0, 255.0));
        let e = read_bytes("plain", tiny_pdf("0 0 72 36", ""), Some("2"), true).unwrap_err().to_string();
        assert!(e.contains("from 1 to 1"), "{e}");
    }

    #[test]
    fn a_geopdf_page_is_placed_by_its_viewport() {
        let wgs84 = r#"/Type /GEOGCS /EPSG 4326 /WKT (GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]])"#;
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(wgs84)), None, true).unwrap();
        assert!(r.georeferenced && r.lost.is_empty(), "{:?}", r.lost);
        let e = r.extent();
        for (got, want) in [(e.min_x, 10.0), (e.max_x, 12.0), (e.min_y, 40.0), (e.max_y, 41.0)] {
            assert!((got - want).abs() < 1e-9, "{e:?}");
        }
        // A code RustGIS doesn't know: its WKT is read instead.
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo(&wgs84.replace("4326", "999999"))), None, true).unwrap();
        assert!(r.georeferenced, "{:?}", r.lost);
        // A second, smaller frame (an inset) is counted, and the larger places the page; read as
        // a scan, nothing is said of either.
        let both = format!("/VP [{} {}]", vp(wgs84, "0 0 72 36"), vp(wgs84, "0 0 10 10"));
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &both), None, true).unwrap();
        assert!(
            r.georeferenced && r.lost == ["it has 1 more map frames with coordinates (insets, say): it is placed by the largest"],
            "{:?}",
            r.lost
        );
        assert!((r.extent().max_x - 12.0).abs() < 1e-9, "placed by the larger");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &both), None, false).unwrap();
        assert!(!r.georeferenced && r.lost.is_empty(), "{:?}", r.lost);
        // A frame across 180°: two degrees wide, not mirrored over the rest of the world.
        let across = geo(wgs84).replace("/GPTS [41 10 41 12 40 12 40 10]", "/GPTS [41 179 41 -179 40 -179 40 179]");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &across), None, true).unwrap();
        let wide = r.transform.pixel_width * f64::from(r.width);
        assert!(r.georeferenced && (wide - 2.0).abs() < 1e-6, "{wide}° wide, {:?}", r.lost);
        // A frame round most of the world, its points stepping across it: 340° wide.
        let wide = geo(wgs84).replace(
            "/GPTS [41 10 41 12 40 12 40 10] /LPTS [0 1 1 1 1 0 0 0]",
            "/GPTS [41 -170 41 0 41 170 40 170 40 0 40 -170] /LPTS [0 1 0.5 1 1 1 1 0 0.5 0 0 0]",
        );
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &wide), None, true).unwrap();
        let span = r.transform.pixel_width * f64::from(r.width);
        assert!(r.georeferenced && (span - 340.0).abs() < 1e-6, "{span}° wide, {:?}", r.lost);
        // The same frame with its pairs in another order: still 340° wide.
        let shuffled = geo(wgs84).replace(
            "/GPTS [41 10 41 12 40 12 40 10] /LPTS [0 1 1 1 1 0 0 0]",
            "/GPTS [41 -170 40 170 41 0 40 0 41 170 40 -170] /LPTS [0 1 1 0 0.5 1 0.5 0 1 1 0 0]",
        );
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &shuffled), None, true).unwrap();
        let span = r.transform.pixel_width * f64::from(r.width);
        assert!(r.georeferenced && (span - 340.0).abs() < 1e-6, "{span}° wide, {:?}", r.lost);
        // A larger frame whose points lie in a line gives way to a smaller one that places it.
        let flat = vp(wgs84, "0 0 72 36").replace("/LPTS [0 1 1 1 1 0 0 0]", "/LPTS [0 0 0.3 0.3 0.6 0.6 1 1]");
        let both = format!("/VP [{flat} {}]", vp(wgs84, "0 0 10 10"));
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &both), None, true).unwrap();
        assert!(r.georeferenced, "{:?}", r.lost);
        assert!(r.lost.iter().any(|t| t.contains("placed by the largest that can be used")), "{:?}", r.lost);
        assert!(r.lost.iter().any(|t| t.starts_with("1 of its map frames have coordinates that can't be used")), "{:?}", r.lost);
        // Without /LPTS, /Bounds (the frame's outline) isn't taken for the points' places: a scan,
        // and said.
        let outline = geo(wgs84).replace("/LPTS [0 1 1 1 1 0 0 0]", "/Bounds [0 1 1 1 1 0 0 0]");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &outline), None, true).unwrap();
        assert!(!r.georeferenced && r.lost[0].contains("(/LPTS)"), "{:?}", r.lost);
        // Two frames, neither usable, and an entry that isn't a frame: all three counted.
        let none = format!("/VP [{} {} 7]", vp("/Type /GEOGCS", "0 0 72 36"), vp("/Type /GEOGCS", "0 0 10 10"));
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &none), None, true).unwrap();
        assert!(!r.georeferenced && r.lost[0].starts_with("3 map frames have coordinates that can't be used ("), "{:?}", r.lost);
        // A rectangle with a fifth entry: not read as four corners, and said.
        let five = geo(wgs84).replace("/BBox [0 0 72 36]", "/BBox [0 0 72 36 /extra]");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &five), None, true).unwrap();
        assert!(!r.georeferenced && r.lost[0].contains("isn't four numbers"), "{:?}", r.lost);
        // A control point that isn't a number: no placing by pairs out of step, and said.
        let bad = geo(wgs84).replace("/GPTS [41 10 41 12", "/GPTS [41 /Ten 41 12");
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &bad), None, true).unwrap();
        assert!(!r.georeferenced && r.lost[0].contains("aren't pairs of numbers"), "{:?}", r.lost);
        // No system at all: a scan, and said.
        let r = read_bytes("geo", tiny_pdf("0 0 72 36", &geo("/Type /GEOGCS")), None, true).unwrap();
        assert!(!r.georeferenced && r.lost[0].contains("doesn't say its coordinate system"), "{:?}", r.lost);
    }

    #[test]
    fn hostile_pdfs_are_refused_or_drawn_small_never_a_panic() {
        assert!(read_bytes("x", b"not a pdf".to_vec(), None, true).unwrap_err().to_string().contains("not a PDF"));
        // Cut short: whatever it reads, it doesn't panic.
        let whole = tiny_pdf("0 0 72 36", "");
        for cut in [10, whole.len() / 2, whole.len() - 8] {
            let _ = read_bytes("cut", whole[..cut].to_vec(), None, true);
        }
        // A page ten million points on a side is drawn within the limit, and says so.
        let r = read_limited("huge", tiny_pdf("0 0 10000000 10000000", ""), None, true, 30_000).unwrap();
        assert!(r.width * r.height * 3 <= 30_000 && r.width >= 1, "{} × {}", r.width, r.height);
        assert!(r.lost[0].contains("dots an inch, not 150"), "{:?}", r.lost);
        // A page box of no area: drawn at a size the renderer gives it, or refused, never a panic.
        let _ = read_bytes("flat", tiny_pdf("0 0 72 0", ""), None, true);
    }
}
