//! AIXM 5.1, 5.1.1 and 5.2: aeronautical information (airports, navaids, designated points, routes,
//! airspace, obstacles…) in an `AIXMBasicMessage`, read into a layer per feature kind.
//!
//! An AIXM feature is a series of time slices, each valid for a period. The reader builds each
//! feature's state at one effective date (today unless one is given), following the AIXM
//! Temporality Concept (EUROCONTROL/FAA, version 1.1, on the AIXM wiki): of the slices with the same
//! interpretation and sequence number, the highest correction number holds; a slice whose
//! `validTime` is empty (`nilReason="inapplicable"`) is canceled; the BASELINE in force at the date
//! is the feature's state, a PERMDELTA in force since it changes it, and the TEMPDELTAs in force at
//! the date overlay it, each property they hold replacing all of that property's values (`nil`
//! removing them). A validity period includes its start and excludes its end. Features not in force
//! at the date, withdrawn before it (`featureLifetime`) or with only changes and no state are left
//! out and counted.
//!
//! Geometry is the GML 3.2 the AIXM profile allows (OGC 12-028r1): points, curves of
//! `GeodesicString`, `Geodesic`, `LineStringSegment`, `Arc`, `ArcString`, `Circle`,
//! `ArcByCenterPoint` and `CircleByCenterPoint` segments, and surfaces of polygon patches. Arcs are
//! drawn as points along them on the WGS 84 ellipsoid, within a meter of the curve; a geodesic edge
//! longer than 10 km is drawn as points along the geodesic. `ArcByCenterPoint` angles follow the
//! AIXM coding guidance: in a latitude-first system (`urn:ogc:def:crs:EPSG::4326`) bearings
//! clockwise from true north, the arc running from the start angle to the end angle (clockwise when
//! the start is the smaller); in a longitude-first one (CRS84), counter-clockwise from east.
//!
//! Every value a feature holds is kept on its rows. A property's simple value is a column, with its
//! unit (`_uom`), why it is missing (`_nilReason`) and its other attributes, and for a reference to
//! another feature the identifier it names (`urn:uuid:` left off) and its title (`_title`). A
//! value nested in records is a column named by its path (`authority_1_type`,
//! `availability_usage_selection_timeInterval_day`), each record numbered when it repeats anywhere
//! in the layer, so the values of one record share their columns on every row. The values written
//! on a shape (elevation, accuracy, datum) are kept wherever the shape is. An airspace is drawn from its single BASE volume, or from
//! BASE and UNION volumes as the parts of one shape; one built by subtraction, intersection or from
//! other airspaces is kept without a shape and counted, not drawn wrong. A vertical structure gives a
//! row per part. Runways, runway directions and routes, which have no geometry of their own, are
//! tables; other kinds with nothing to draw are counted.
//!
//! The file is hostile: no DTD is read, members are parsed one at a time within the node and
//! namespace limits every XML reader has, and the points arcs and geodesics add are bounded by the
//! file's size.

use std::collections::{BTreeMap, HashMap, HashSet};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use roxmltree::{Document, Node, ParsingOptions};
use rustgis_geom::Coord;

use crate::{DataError, Dataset, Field, Geometry, GeometryType, ReadReport, Result, Value};

/// How much of a file is looked at to tell whether it is AIXM.
pub const HEAD_BYTES: usize = 64 * 1024;

/// AIXM namespaces start with this (`http://www.aixm.aero/schema/5.1`, `…/5.1.1`, `…/5.2`, and
/// their `/message` namespaces).
const AIXM: &str = "http://www.aixm.aero/schema/5.";
const GML: &str = "http://www.opengis.net/gml/3.2";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// Most layers read from one file. Features are read GeoBorders first and route segments last
/// (borders are followed and points named by others), and tables of features with nothing to draw
/// are made after all of them, so in a file of more kinds than this those are the first left out;
/// the layers that are kept come in the file's order.
const MAX_LAYERS: usize = 64;
/// Most attribute columns of a layer.
const MAX_COLUMNS: usize = 2000;
/// Deepest a nested value is read: past it a record's values are counted, not kept.
const MAX_PATH: usize = 64;
/// The farthest a drawn arc strays from the curve, in meters.
const ARC_TOLERANCE_M: f64 = 1.0;
/// Most points one arc or circle is drawn with.
const MAX_ARC_POINTS: usize = 3600;
/// The longest straight piece of a drawn geodesic, in meters.
const GEODESIC_STEP_M: f64 = 10_000.0;
/// Most points one geodesic edge is drawn with.
const MAX_GEODESIC_POINTS: usize = 1000;
/// Points arcs and geodesics may add whatever the file's size.
const MIN_ADDED_POINTS: usize = 1_000_000;
/// The longest radius read, in meters: past a quarter of the Earth's circumference no airspace
/// is drawn by its center.
const MAX_RADIUS_M: f64 = 10_000_000.0;

/// Kinds with no geometry of their own read as tables.
const TABLE_KINDS: &[&str] = &["Runway", "RunwayDirection", "Route", "RouteSegment"];

/// Time-slice properties that describe the slice, not the feature.
const SLICE_PROPERTIES: &[&str] =
    &["validTime", "interpretation", "sequenceNumber", "correctionNumber", "featureLifetime", "timeSliceMetadata", "featureMetadata"];

/// Geometry elements a property can hold (AIXM's and GML's).
const SHAPES: &[&str] = &["Point", "ElevatedPoint", "Curve", "ElevatedCurve", "Surface", "ElevatedSurface", "LineString", "Polygon"];

/// What a read is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The effective date: milliseconds since 1970-01-01 UTC. Today when `None`.
    pub at: Option<f64>,
    /// Only the layers' names are wanted (`info.fileLayers`): no shape is drawn and no value kept.
    pub names_only: bool,
}

impl Options {
    /// Reads `"at"` as a date or date and time (`2026-01-15`, `2026-01-15T12:00Z`): UTC unless it
    /// says its zone.
    pub fn at_text(text: &str) -> std::result::Result<Self, String> {
        crate::time::parse_text(text)
            .map(|t| Options { at: Some(t), ..Options::default() })
            .ok_or_else(|| format!("'{text}' isn't a date (an AIXM effective date is written like 2026-01-15 or 2026-01-15T12:00Z)"))
    }
}

/// The names of the layers [`read`] gives a message at `opts.at`, worked out without drawing
/// their shapes or keeping their values: what listing a file's layers needs.
pub fn layer_names(file_name: &str, bytes: &[u8], opts: &Options) -> Result<Vec<String>> {
    let opts = Options { names_only: true, ..*opts };
    Ok(read(file_name, bytes, &opts)?.into_iter().map(|(d, _)| d.name).collect())
}

/// Whether a file is an AIXM message, by its root: an `AIXMBasicMessage` in an AIXM 5 namespace,
/// or an FAA NASR subscriber file of AIXM 5 features ([`is_root`]).
pub fn looks_like_aixm(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(bytes.get(..HEAD_BYTES).unwrap_or(bytes));
    let mut reader = Reader::from_str(head.trim_start_matches('\u{feff}'));
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => return is_root(&e),
            Ok(Event::Eof) | Err(_) => return false,
            Ok(_) => {}
        }
    }
}

/// The FAA's namespaces for its NASR subscriber files.
const FAA: &str = "http://www.faa.gov/aixm5";

/// Whether an element is the root of a message RustGIS reads: an `AIXMBasicMessage` in an AIXM 5
/// namespace, or the FAA's `SubscriberFile` (its 28-day NASR data) saying it holds AIXM 5
/// (`AIXMVersion="AIXM5.1"`), whose `Member`s hold AIXM features as `hasMember`s do.
fn is_root(e: &BytesStart) -> bool {
    let ns = namespace_of(e);
    match e.local_name().into_inner() {
        "AIXMBasicMessage" => ns.is_some_and(|ns| ns.starts_with(AIXM)),
        "SubscriberFile" => {
            ns.is_some_and(|ns| ns.starts_with(FAA))
                && e.attributes().flatten().any(|a| a.key.local_name().into_inner() == "AIXMVersion" && a.value.trim().starts_with("AIXM5"))
        }
        _ => false,
    }
}

/// The namespace an element's own prefix (or the default) is declared with on it.
fn namespace_of(e: &BytesStart) -> Option<String> {
    let name = e.name();
    let want = match name.prefix() {
        Some(p) => format!("xmlns:{}", p.into_inner()),
        None => "xmlns".to_string(),
    };
    e.attributes().flatten().find(|a| a.key.into_inner() == want).map(|a| a.value.trim().to_string())
}

/// The milliseconds of now, UTC.
fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// A date as notices write it.
fn date_text(t: f64) -> String {
    crate::time::format(t, "YYYY-MM-DD HH:mm") + " UTC"
}

// ---------------------------------------------------------------------------------------------
// Index: the members, found in one pass with no tree.
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Index {
    /// Each `hasMember`: where it starts and ends in the text.
    members: Vec<(usize, usize)>,
    /// The root's namespace declarations, written as attributes (` xmlns:x="…"`).
    namespaces: String,
    /// `hasMember`s that only point elsewhere (`xlink:href`), not followed.
    links: usize,
    /// `hasMember`s holding nothing.
    empty: usize,
    /// Where the text stops being XML, when it does before the root ends.
    broken_at: Option<usize>,
}

impl Index {
    fn of(file_name: &str, text: &str) -> Result<Index> {
        let mut reader = Reader::from_str(text);
        let mut index = Index::default();
        let (mut depth, mut rooted) = (0usize, false);
        // The member being read: where it starts, whether it points elsewhere, and whether it
        // holds an element.
        let mut member: Option<(usize, bool, bool)> = None;
        loop {
            let start = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
            let event = match reader.read_event() {
                Ok(e) => e,
                Err(_) => {
                    index.broken_at = Some(usize::try_from(reader.error_position()).unwrap_or(start));
                    break;
                }
            };
            let end = usize::try_from(reader.buffer_position()).unwrap_or(usize::MAX);
            match event {
                Event::Start(ref e) | Event::Empty(ref e) if depth == 0 => {
                    rooted = true;
                    if !is_root(e) {
                        return Err(DataError::Format(format!(
                            "{file_name}: not an AIXM message (an AIXMBasicMessage in an AIXM 5 namespace, or an FAA NASR subscriber file of AIXM 5)"
                        )));
                    }
                    index.namespaces = declarations(e);
                    // An empty message (`<AIXMBasicMessage/>`) has no members.
                    if matches!(event, Event::Empty(_)) {
                        break;
                    }
                    depth = 1;
                }
                Event::Start(e) => {
                    if depth == 1 && is_member(&e) {
                        member = Some((start, has_href(&e), false));
                    } else if depth == 2
                        && let Some(m) = member.as_mut()
                    {
                        m.2 = true;
                    }
                    depth += 1;
                }
                Event::Empty(e) => {
                    if depth == 1 && is_member(&e) {
                        if has_href(&e) {
                            index.links += 1;
                        } else {
                            index.empty += 1;
                        }
                    } else if depth == 2
                        && let Some(m) = member.as_mut()
                    {
                        m.2 = true;
                    }
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 1
                        && let Some((s, href, held)) = member.take()
                    {
                        match (held, href) {
                            (true, _) => index.members.push((s, end)),
                            (false, true) => index.links += 1,
                            (false, false) => index.empty += 1,
                        }
                    }
                    if depth == 0 {
                        break;
                    }
                }
                Event::Eof => {
                    if rooted && depth > 0 {
                        index.broken_at = Some(text.len());
                    }
                    break;
                }
                _ => {}
            }
        }
        if !rooted {
            return Err(DataError::Format(format!("{file_name}: not an AIXM message (no root element)")));
        }
        Ok(index)
    }
}

/// Whether an element under the root holds a feature: a message's `hasMember`, a subscriber
/// file's `Member`.
fn is_member(e: &BytesStart) -> bool {
    matches!(e.local_name().into_inner(), "hasMember" | "Member")
}

/// Whether an element points elsewhere (`xlink:href`).
fn has_href(e: &BytesStart) -> bool {
    e.attributes().flatten().any(|a| a.key.local_name().into_inner() == "href")
}

/// An element's namespace declarations, as attributes to write on a wrapper.
fn declarations(e: &BytesStart) -> String {
    let mut out = String::new();
    for a in e.attributes().flatten() {
        let key = a.key.into_inner();
        if key == "xmlns" || key.starts_with("xmlns:") {
            let value = &a.value;
            let quote = if value.contains('"') { '\'' } else { '"' };
            out.push(' ');
            out.push_str(key);
            out.push('=');
            out.push(quote);
            out.push_str(value);
            out.push(quote);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Small tree helpers.
// ---------------------------------------------------------------------------------------------

fn local<'a>(n: &Node<'a, '_>) -> &'a str {
    n.tag_name().name()
}

fn in_ns(n: &Node, ns: &str) -> bool {
    n.tag_name().namespace().is_some_and(|x| x.starts_with(ns))
}

fn is_aixm(n: &Node) -> bool {
    in_ns(n, AIXM)
}

fn is_gml(n: &Node) -> bool {
    n.tag_name().namespace() == Some(GML)
}

fn elements<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(Node::is_element)
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    elements(n).find(|c| local(c) == name)
}

fn text_of(n: &Node) -> String {
    n.children().filter(|t| t.is_text()).filter_map(|t| t.text()).collect::<String>().trim().to_string()
}

fn is_nil(n: &Node) -> bool {
    n.attribute((XSI, "nil")).is_some_and(|v| matches!(v.trim(), "true" | "1"))
}

/// A date of a `gml:beginPosition`/`endPosition`/`timePosition`: `Ok(None)` for an open end
/// (`indeterminatePosition`, or empty), `Err` for one that isn't a date.
fn position(n: Option<Node>, empty: &std::cell::Cell<usize>) -> std::result::Result<Option<f64>, ()> {
    let Some(n) = n else { return Ok(None) };
    let t = text_of(&n);
    if t.is_empty() {
        // An empty position that doesn't say it is unknown (the FAA's `<gml:endPosition/>`) is
        // read as open too, and counted.
        if n.attribute("indeterminatePosition").is_none() {
            empty.set(empty.get() + 1);
        }
        return Ok(None);
    }
    crate::time::parse_text(&t).map(Some).ok_or(())
}

// ---------------------------------------------------------------------------------------------
// Time slices.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Interpretation {
    Baseline,
    PermDelta,
    TempDelta,
    Snapshot,
}

/// What a slice says of itself, and where it is.
#[derive(Clone, Debug)]
struct Slice {
    /// Where it is written in the text.
    range: (usize, usize),
    /// The namespaces in scope there: an index into the read's scopes (0, the root's).
    scope: usize,
    kind: Interpretation,
    sequence: u64,
    correction: u64,
    /// Valid from (included) and until (excluded); an instant for a PERMDELTA or SNAPSHOT.
    begin: Option<f64>,
    end: Option<f64>,
    canceled: bool,
    /// The feature's lifetime, when the slice says it.
    lifetime: Option<(Option<f64>, Option<f64>)>,
}

/// A feature's slices, read from any number of members.
struct Feature {
    /// Its identifier (`gml:identifier`, else its `gml:id`).
    id: String,
    kind: String,
    /// Its own `gml:id`: what a local link (`#id`) or the FAA's XPath names it by.
    gml_id: Option<String>,
    slices: Vec<Slice>,
}

/// What a feature is at the date.
enum State {
    /// Built from these slices: the state, then the changes over it in order.
    Use { base: Slice, changes: Vec<Slice>, snapshot: bool, temporary: usize },
    /// Not in force at the date: its validity is all before or after it.
    NotInForce,
    /// Withdrawn by the date, or not yet created (`featureLifetime`).
    OutsideLifetime,
    /// Only changes (PERMDELTA, TEMPDELTA) and no state to apply them to.
    OnlyChanges,
    /// Every slice canceled.
    Canceled,
}

fn in_force(begin: Option<f64>, end: Option<f64>, t: f64) -> bool {
    begin.is_none_or(|b| b <= t) && end.is_none_or(|e| t < e)
}

/// Each feature's state at `t`.
fn state_at(slices: &[Slice], t: f64) -> State {
    // Of slices alike in interpretation and sequence number, the highest correction holds (the
    // later in the file on a tie).
    let mut held: BTreeMap<(u8, u64), &Slice> = BTreeMap::new();
    // SNAPSHOTs have no sequence or correction number: each is its own instant, all kept.
    let mut snapshots: Vec<&Slice> = Vec::new();
    for s in slices {
        if s.kind == Interpretation::Snapshot {
            snapshots.push(s);
            continue;
        }
        let key = (s.kind as u8, s.sequence);
        match held.get(&key) {
            Some(h) if h.correction > s.correction => {}
            _ => {
                held.insert(key, s);
            }
        }
    }
    let live: Vec<&Slice> = held.values().copied().chain(snapshots).filter(|s| !s.canceled).collect();
    if live.is_empty() {
        return if slices.is_empty() { State::OnlyChanges } else { State::Canceled };
    }
    let of = |k: Interpretation| live.iter().copied().filter(move |s| s.kind == k);
    // The BASELINE in force; of several (which the concept doesn't allow), the latest started.
    let base = of(Interpretation::Baseline)
        .filter(|s| in_force(s.begin, s.end, t))
        .max_by(|a, b| a.begin.unwrap_or(f64::MIN).total_cmp(&b.begin.unwrap_or(f64::MIN)).then(a.sequence.cmp(&b.sequence)));
    let (base, snapshot) = match base {
        Some(b) => (b, false),
        None => {
            let baselines = of(Interpretation::Baseline).count();
            // A SNAPSHOT is a state at its own instant: the one nearest before the date, else the
            // earliest after it.
            let snap = of(Interpretation::Snapshot)
                .filter(|s| s.begin.is_none_or(|b| b <= t))
                .max_by(|a, b| a.begin.unwrap_or(f64::MIN).total_cmp(&b.begin.unwrap_or(f64::MIN)))
                .or_else(|| {
                    of(Interpretation::Snapshot).min_by(|a, b| a.begin.unwrap_or(f64::MAX).total_cmp(&b.begin.unwrap_or(f64::MAX)))
                });
            match snap {
                Some(s) if baselines == 0 => (s, true),
                _ if baselines > 0 => return State::NotInForce,
                _ => return State::OnlyChanges,
            }
        }
    };
    // PERMDELTAs since the state began, up to the date, in order.
    let mut changes: Vec<Slice> = of(Interpretation::PermDelta)
        .filter(|s| s.begin.is_none_or(|b| b <= t) && s.begin.is_some_and(|b| base.begin.is_none_or(|bb| b > bb)))
        .cloned()
        .collect();
    changes.sort_by(|a, b| a.begin.unwrap_or(f64::MIN).total_cmp(&b.begin.unwrap_or(f64::MIN)).then(a.sequence.cmp(&b.sequence)));
    // The feature's lifetime: the latest a slice in force says.
    let lifetime = changes.iter().rev().find_map(|s| s.lifetime).or(base.lifetime);
    if let Some((from, until)) = lifetime
        && !in_force(from, until, t)
    {
        return State::OutsideLifetime;
    }
    let mut temps: Vec<Slice> = of(Interpretation::TempDelta).filter(|s| in_force(s.begin, s.end, t)).cloned().collect();
    temps.sort_by(|a, b| a.begin.unwrap_or(f64::MIN).total_cmp(&b.begin.unwrap_or(f64::MIN)).then(a.sequence.cmp(&b.sequence)));
    let temporary = temps.len();
    changes.extend(temps);
    State::Use { base: base.clone(), changes, snapshot, temporary }
}

/// What the first pass found wrong with a slice.
#[derive(Default)]
struct SliceTrouble {
    /// Slices with no interpretation RustGIS knows.
    unknown: usize,
    /// Slices with dates that aren't dates.
    dates: usize,
    /// Validity or lifetime ends left empty without saying they are unknown, read as open.
    empty: usize,
    /// Slices with a sequence or correction number that isn't a number, read as 0.
    numbers: usize,
}

/// A slice's own properties.
fn slice_of(s: Node, range: (usize, usize), scope: usize, trouble: &mut SliceTrouble) -> Option<Slice> {
    let kind = match child(s, "interpretation").map(|n| text_of(&n)).as_deref() {
        Some("BASELINE") => Interpretation::Baseline,
        Some("PERMDELTA") => Interpretation::PermDelta,
        Some("TEMPDELTA") => Interpretation::TempDelta,
        Some("SNAPSHOT") => Interpretation::Snapshot,
        _ => {
            trouble.unknown += 1;
            return None;
        }
    };
    // A number that isn't one is read as 0 and counted.
    let unread_number = std::cell::Cell::new(false);
    let number = |name: &str| match child(s, name).map(|n| text_of(&n)) {
        Some(t) if !t.is_empty() => t.parse::<u64>().unwrap_or_else(|_| {
            unread_number.set(true);
            0
        }),
        _ => 0,
    };
    let empty = std::cell::Cell::new(0);
    let (mut begin, mut end, mut canceled) = (None, None, false);
    let mut bad = false;
    if let Some(v) = child(s, "validTime") {
        match elements(v).next() {
            Some(p) if local(&p) == "TimePeriod" => {
                begin = position(child(p, "beginPosition"), &empty).unwrap_or_else(|()| {
                    bad = true;
                    None
                });
                end = position(child(p, "endPosition"), &empty).unwrap_or_else(|()| {
                    bad = true;
                    None
                });
            }
            Some(i) if local(&i) == "TimeInstant" => {
                begin = position(child(i, "timePosition"), &empty).unwrap_or_else(|()| {
                    bad = true;
                    None
                });
            }
            Some(_) => bad = true,
            // An empty validTime cancels the slice only when it says so
            // (`nilReason="inapplicable"`); any other is a validity RustGIS can't read.
            None => match v.attribute("nilReason").map(str::trim) {
                Some("inapplicable") => canceled = true,
                _ => bad = true,
            },
        }
    }
    let lifetime = child(s, "featureLifetime").and_then(|l| elements(l).find(|p| local(p) == "TimePeriod")).map(|p| {
        let from = position(child(p, "beginPosition"), &empty).unwrap_or_else(|()| {
            bad = true;
            None
        });
        let until = position(child(p, "endPosition"), &empty).unwrap_or_else(|()| {
            bad = true;
            None
        });
        (from, until)
    });
    let (sequence, correction) = (number("sequenceNumber"), number("correctionNumber"));
    if bad {
        trouble.dates += 1;
        return None;
    }
    trouble.numbers += usize::from(unread_number.get());
    trouble.empty += empty.get();
    Some(Slice { range, scope, kind, sequence, correction, begin, end, canceled, lifetime })
}

/// The features of a member (an `AIXMBasicMessage`'s `hasMember` holds one).
fn features_of<'a, 'i>(wrapper: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    elements(wrapper).next().map(|m| elements(m).filter(is_aixm).collect()).unwrap_or_default()
}

/// The elements of a member that aren't AIXM features (another schema's), not read.
fn foreign_in(wrapper: Node) -> usize {
    elements(wrapper).next().map_or(0, |m| elements(m).filter(|c| !is_aixm(c)).count())
}

/// A feature's time slices, in order.
fn slices_of<'a, 'i>(feature: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    elements(feature).filter(|c| local(c) == "timeSlice").filter_map(|t| elements(t).next()).collect()
}

/// A feature's identifier: its `gml:identifier`, else its `gml:id`.
fn identifier(f: Node) -> Option<String> {
    let id = elements(f).find(|c| is_gml(c) && local(c) == "identifier").map(|n| text_of(&n)).filter(|t| !t.is_empty());
    id.or_else(|| f.attribute((GML, "id")).map(|s| s.trim().to_string()))
}

// ---------------------------------------------------------------------------------------------
// Geometry.
// ---------------------------------------------------------------------------------------------

/// How a geometry's coordinates are read.
#[derive(Clone, Copy)]
struct Axes {
    /// Latitude first.
    swap: bool,
}

/// What reading shapes found, for a layer's notices.
#[derive(Default)]
struct ShapeNotes {
    /// Features drawn with arcs or circles as points along them.
    arcs: bool,
    /// Features whose curves were drawn with fewer points than the tolerance needs: the file's
    /// point budget ran out.
    coarse: bool,
    /// Features with Z or M values dropped.
    zm: bool,
    /// Curve segments of kinds the profile doesn't allow, drawn straight between their positions.
    unread_segments: Vec<String>,
    /// Rings or patches left out (too few points, unreadable, a border given by reference).
    rings_lost: usize,
    /// Of those, borders given by a reference to a curve not in the file (or not in force).
    referenced: usize,
    /// Borders given by reference, followed along the GeoBorder they name; of those, ones whose
    /// ends lie more than [`FAR_FROM_BORDER_M`] from it.
    followed: usize,
    far: usize,
    /// Borders by reference followed whole (a ring of two borders: no points to cut them at).
    followed_whole: usize,
    /// A route segment drawn between the points it names, having no curve.
    between: bool,
    /// An arc or circle not given the three positions it needs (fewer, or a circle given more):
    /// drawn through the positions it has (a circle through its first three).
    short_arcs: bool,
    /// An arc or circle whose points are in a line, or past the largest radius: drawn straight.
    straight_arcs: bool,
    /// An arc or circle with no positions: left out, the line going on without it.
    empty_arcs: bool,
    /// A route segment with no curve and a `pathType` RustGIS doesn't draw.
    path_unread: bool,
    /// Borders by reference not followed: past the work allowed, or too short to cut.
    border_allowance: usize,
    border_unusable: usize,
}

/// Reads shapes, adding at most `budget` points to the file's positions.
#[derive(Default)]
struct Shapes {
    budget: usize,
    /// A system other than longitude/latitude a geometry was in (not read), and how many were.
    other_system: Option<String>,
    other_count: usize,
    other_now: bool,
    /// Geometries with no `srsName`, read as `urn:ogc:def:crs:EPSG::4326`.
    assumed_system: usize,
    /// Whether the shape being read had one.
    assumed_now: bool,
    /// A geographic system other than WGS 84 a geometry was read in as if it were, and how many.
    as_wgs84: Option<String>,
    as_wgs84_count: usize,
    as_wgs84_now: bool,
    /// The borders references may follow, by [`border_key`]: each GeoBorder's at the date.
    borders: HashMap<String, Vec<Coord>>,
    /// Steps of looking along borders still allowed ([`Shapes::scan`]).
    scan_left: usize,
    /// Only the layers' names are wanted: a shape is its family's placeholder, not drawn.
    names_only: bool,
    /// Where each point feature is (a navaid, a designated point), by its identifier: the ends of
    /// route segments that name them.
    points: HashMap<String, Coord>,
}

fn wrap180(d: f64) -> f64 {
    let r = (d + 180.0).rem_euclid(360.0) - 180.0;
    if r == -180.0 && d > 0.0 { 180.0 } else { r }
}

/// `c` moved by whole turns of longitude to lie next to `near`.
fn next_to(c: Coord, near: f64) -> Coord {
    Coord::new(near + wrap180(c.x - near), c.y)
}

impl ShapeNotes {
    /// What a shape that wasn't drawn found: why, not how it was drawn.
    fn losses_only(self) -> ShapeNotes {
        ShapeNotes {
            rings_lost: self.rings_lost,
            referenced: self.referenced,
            border_allowance: self.border_allowance,
            border_unusable: self.border_unusable,
            path_unread: self.path_unread,
            ..ShapeNotes::default()
        }
    }

    /// Adds what another shape's reading found.
    fn absorb(&mut self, o: ShapeNotes) {
        self.arcs |= o.arcs;
        self.coarse |= o.coarse;
        self.zm |= o.zm;
        for n in o.unread_segments {
            if !self.unread_segments.contains(&n) {
                self.unread_segments.push(n);
            }
        }
        self.rings_lost += o.rings_lost;
        self.referenced += o.referenced;
        self.followed += o.followed;
        self.followed_whole += o.followed_whole;
        self.far += o.far;
        self.border_allowance += o.border_allowance;
        self.border_unusable += o.border_unusable;
        self.between |= o.between;
        self.short_arcs |= o.short_arcs;
        self.straight_arcs |= o.straight_arcs;
        self.empty_arcs |= o.empty_arcs;
        self.path_unread |= o.path_unread;
    }
}

impl Shapes {
    /// Takes up to `n` added points from the budget: how many may be added.
    fn take(&mut self, n: usize, notes: &mut ShapeNotes) -> usize {
        let k = n.min(self.budget);
        if k < n {
            notes.coarse = true;
        }
        self.budget -= k;
        k
    }

    /// The system a geometry is in: its own `srsName` or its nearest ancestor's.
    fn axes(&mut self, g: Node) -> Option<Axes> {
        let Some(name) = g.ancestors().find_map(|a| a.attribute("srsName")).map(str::trim) else {
            self.assumed_now = true;
            return Some(Axes { swap: true });
        };
        if crate::ogc::warp::is_crs84(name) {
            return Some(Axes { swap: false });
        }
        // OGC's CRS83: NAD83 longitude/latitude, as the FAA writes it; within a couple of meters
        // of WGS 84 in North America, read as it and said.
        if name.trim().to_ascii_uppercase().ends_with(":CRS83") || name.trim().to_ascii_uppercase().ends_with("/CRS83") {
            self.as_wgs84.get_or_insert_with(|| name.to_string());
            self.as_wgs84_now = true;
            return Some(Axes { swap: false });
        }
        match crate::gml::srs(name).map(|(c, _)| c) {
            Some(c) if c.is_geographic() => {
                if c.epsg() != 4326 {
                    self.as_wgs84.get_or_insert_with(|| name.to_string());
                    self.as_wgs84_now = true;
                }
                Some(Axes { swap: crate::gml::lat_first(name, c) })
            }
            _ => {
                self.other_system.get_or_insert_with(|| name.to_string());
                self.other_now = true;
                None
            }
        }
    }

    /// The positions under a node (`pos`, `posList`, `pointProperty`'s point), in order, as lon/lat.
    fn positions(n: Node, axes: Axes, notes: &mut ShapeNotes) -> Option<Vec<Coord>> {
        let mut out = Vec::new();
        for c in n.descendants().filter(|d| d.is_element() && is_gml(d) && matches!(local(d), "pos" | "posList")) {
            let v: Vec<f64> = text_of(&c).split_whitespace().map(|t| t.parse().ok()).collect::<Option<_>>()?;
            let one = local(&c) == "pos" && (2..=4).contains(&v.len());
            let dim: usize = c
                .ancestors()
                .find_map(|a| a.attribute("srsDimension"))
                .and_then(|d| d.trim().parse().ok())
                .filter(|d| (2..=4).contains(d))
                .unwrap_or(if one { v.len() } else { 2 });
            if !v.len().is_multiple_of(dim) {
                return None;
            }
            if dim > 2 && !v.is_empty() {
                notes.zm = true;
            }
            for p in v.chunks(dim) {
                let (a, b) = (*p.first()?, *p.get(1)?);
                let c = if axes.swap { Coord::new(b, a) } else { Coord::new(a, b) };
                if !c.is_finite() || c.y.abs() > 90.0 || c.x.abs() > 540.0 {
                    return None;
                }
                // Next to the position before, so a line across the antimeridian stays continuous.
                out.push(out.last().map_or(c, |l: &Coord| next_to(c, l.x)));
            }
        }
        Some(out)
    }

    /// A point geometry.
    fn point(&mut self, g: Node, notes: &mut ShapeNotes) -> Option<Geometry> {
        let axes = self.axes(g)?;
        Self::positions(g, axes, notes)?.first().map(|c| Geometry::Point(*c))
    }

    /// A curve (or line string) as one run of points.
    fn curve(&mut self, g: Node, notes: &mut ShapeNotes) -> Option<Vec<Coord>> {
        let axes = self.axes(g)?;
        let Some(segments) = elements(g).find(|c| is_gml(c) && local(c) == "segments") else {
            // A LineString or LinearRing: its positions, straight.
            let c = Self::positions(g, axes, notes)?;
            return (c.len() >= 2).then_some(c);
        };
        let mut out: Vec<Coord> = Vec::new();
        for s in elements(segments) {
            let mut part = self.segment(s, axes, notes)?;
            // Each part's longitudes next to where the line is, across the antimeridian too.
            if let Some(last) = out.last().copied() {
                part = part.into_iter().map(|c| next_to(c, last.x)).collect();
                if part.first().is_some_and(|f| (f.x - last.x).abs() < 1e-12 && (f.y - last.y).abs() < 1e-12) {
                    part.remove(0);
                }
            }
            out.extend(part);
        }
        (out.len() >= 2).then_some(out)
    }

    /// One curve segment's points.
    fn segment(&mut self, s: Node, axes: Axes, notes: &mut ShapeNotes) -> Option<Vec<Coord>> {
        match local(&s) {
            "LineStringSegment" => Self::positions(s, axes, notes),
            "GeodesicString" | "Geodesic" => {
                let c = Self::positions(s, axes, notes)?;
                Some(self.geodesic(&c, notes))
            }
            "ArcByCenterPoint" | "CircleByCenterPoint" => {
                notes.arcs = true;
                self.arc_by_center(s, axes, notes)
            }
            "Arc" | "ArcString" | "Circle" => {
                let c = Self::positions(s, axes, notes)?;
                if c.is_empty() {
                    notes.empty_arcs = true;
                    return Some(Vec::new());
                }
                Some(self.arcs_through(&c, local(&s), notes))
            }
            other => {
                if !notes.unread_segments.iter().any(|n| n == other) {
                    notes.unread_segments.push(other.to_string());
                }
                Self::positions(s, axes, notes)
            }
        }
    }

    /// A run of positions joined by geodesics on WGS 84, a point every [`GEODESIC_STEP_M`] at most.
    fn geodesic(&mut self, c: &[Coord], notes: &mut ShapeNotes) -> Vec<Coord> {
        let mut out: Vec<Coord> = Vec::with_capacity(c.len());
        for pair in c.windows(2) {
            let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
            let a = out.last().map_or(a, |l| next_to(a, l.x));
            if out.is_empty() {
                out.push(a);
            }
            let (d, az, _) = rustgis_geom::wgs84_inverse(a, b);
            if d.is_finite() && d > GEODESIC_STEP_M {
                let need = (d / GEODESIC_STEP_M).ceil() as usize;
                notes.coarse |= need > MAX_GEODESIC_POINTS;
                let want = need.min(MAX_GEODESIC_POINTS);
                let n = self.take(want.saturating_sub(1), notes) + 1;
                for k in 1..n {
                    let (p, _) = rustgis_geom::wgs84_direct(a, az, d * k as f64 / n as f64);
                    if p.is_finite() {
                        out.push(next_to(p, a.x));
                    }
                }
            }
            out.push(next_to(b, a.x));
        }
        if c.len() == 1
            && let Some(&a) = c.first()
        {
            out.push(a);
        }
        out
    }

    /// A rhumb line (loxodrome) from `a` to `b`: straight on a Mercator map, a point every
    /// [`GEODESIC_STEP_M`] at most, from the same allowance as geodesics.
    fn rhumb(&mut self, a: Coord, b: Coord, notes: &mut ShapeNotes) -> Vec<Coord> {
        let b = next_to(b, a.x);
        let pole = std::f64::consts::FRAC_PI_2 - 1e-9;
        let merc = |lat: f64| (std::f64::consts::FRAC_PI_4 + lat.to_radians().clamp(-pole, pole) / 2.0).tan().ln();
        let (ya, yb) = (merc(a.y), merc(b.y));
        let d = rustgis_geom::wgs84_distance_m(a, b);
        let need = if d.is_finite() { (d / GEODESIC_STEP_M).ceil() as usize } else { 1 };
        notes.coarse |= need > MAX_GEODESIC_POINTS;
        let n = self.take(need.clamp(1, MAX_GEODESIC_POINTS).saturating_sub(1), notes) + 1;
        // Its ends are the points named, exactly; the points between are on the rhumb line.
        (0..=n)
            .map(|k| match k {
                0 => a,
                _ if k == n => b,
                _ => {
                    let t = k as f64 / n as f64;
                    let y = ya + (yb - ya) * t;
                    Coord::new(a.x + (b.x - a.x) * t, (2.0 * y.exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees())
                }
            })
            .collect()
    }

    /// How many pieces an arc of `radius_m` turning `sweep_deg` is drawn with.
    fn pieces(&mut self, radius_m: f64, sweep_deg: f64, notes: &mut ShapeNotes) -> usize {
        let step = if radius_m <= ARC_TOLERANCE_M { 90.0 } else { (2.0 * (1.0 - ARC_TOLERANCE_M / radius_m).acos()).to_degrees() };
        let need = (sweep_deg.abs() / step.max(1e-6)).ceil() as usize;
        notes.coarse |= need > MAX_ARC_POINTS;
        let want = need.clamp(4, MAX_ARC_POINTS);
        // A full circle keeps at least 8 pieces (or what it needs, if fewer) when the allowance has
        // run out, said as coarse: each circle in the text is far longer than 8 points, so the
        // extra stays bounded by the file's size.
        let n = self.take(want - 1, notes) + 1;
        let floor = want.min(8);
        if sweep_deg.abs() >= 360.0 && n < floor {
            return floor;
        }
        n
    }

    /// An `ArcByCenterPoint` or `CircleByCenterPoint`.
    fn arc_by_center(&mut self, s: Node, axes: Axes, notes: &mut ShapeNotes) -> Option<Vec<Coord>> {
        let center = *Self::positions(s, axes, notes)?.first()?;
        let r = child(s, "radius")?;
        let radius = text_of(&r).parse::<f64>().ok()? * meters_per(r.attribute("uom").unwrap_or("m"))?;
        if !(radius.is_finite() && radius > 0.0 && radius <= MAX_RADIUS_M) {
            return None;
        }
        let angle = |name: &str| -> Option<Option<f64>> {
            match child(s, name) {
                None => Some(None),
                Some(a) => {
                    let v = text_of(&a).parse::<f64>().ok()?;
                    let v = match a.attribute("uom").map(str::trim) {
                        None | Some("deg") => v,
                        Some("rad") => v.to_degrees(),
                        Some(_) => return None,
                    };
                    v.is_finite().then_some(Some(v))
                }
            }
        };
        let (start, end) = (angle("startAngle")?.unwrap_or(0.0), angle("endAngle")?);
        // Angles from the first axis: in a latitude-first system bearings from north, clockwise as
        // they grow; in a longitude-first one from east, counter-clockwise.
        let (bearing0, sweep) = match (local(&s), end) {
            ("CircleByCenterPoint", _) => (if axes.swap { start } else { 90.0 - start }, 360.0),
            (_, Some(end)) if (end - start).abs() <= 360.0 => {
                let d = end - start;
                if axes.swap { (start, d) } else { (90.0 - start, -d) }
            }
            _ => return None,
        };
        let n = self.pieces(radius, sweep, notes);
        let mut out = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let (p, _) = rustgis_geom::wgs84_direct(center, bearing0 + sweep * k as f64 / n as f64, radius);
            if !p.is_finite() {
                return None;
            }
            out.push(next_to(p, center.x));
        }
        Some(out)
    }

    /// Arcs through each three points (`Arc`, `ArcString`: start, a point on it, end; the next arc
    /// starting at the last's end), or a full circle through three (`Circle`).
    fn arcs_through(&mut self, c: &[Coord], name: &str, notes: &mut ShapeNotes) -> Vec<Coord> {
        let circle = name == "Circle";
        if c.len() < 3 {
            notes.short_arcs = true;
            return c.to_vec();
        }
        let mut out: Vec<Coord> = Vec::new();
        // Each point next to the one before it, so a chain of arcs across 180 degrees stays whole.
        let put = |out: &mut Vec<Coord>, p: Coord| {
            let p = out.last().map_or(p, |l| next_to(p, l.x));
            out.push(p);
        };
        let mut i = 0;
        while let (Some(&a), Some(&m), Some(&b)) = (c.get(i), c.get(i + 1), c.get(i + 2)) {
            let arc = self.arc_through(a, m, b, circle, notes);
            let skip = usize::from(!out.is_empty());
            for p in arc.into_iter().skip(skip) {
                put(&mut out, p);
            }
            if circle {
                break;
            }
            i += 2;
        }
        // A circle is through its first three positions: any more are said.
        if circle && c.len() > 3 {
            notes.short_arcs = true;
        }
        // An odd position past the last arc: joined straight, and said.
        if !circle && i + 1 < c.len() {
            for &p in c.iter().skip(i + 1) {
                put(&mut out, p);
            }
            if !notes.unread_segments.iter().any(|n| n == name) {
                notes.unread_segments.push(name.to_string());
            }
        }
        out
    }

    /// The arc from `a` through `m` to `b`, worked out in an azimuthal equidistant plane around `m`
    /// (distances true from it); straight when the three are in a line.
    fn arc_through(&mut self, a: Coord, m: Coord, b: Coord, circle: bool, notes: &mut ShapeNotes) -> Vec<Coord> {
        let plane = rustgis_geom::Aeqd::new(m);
        let (pa, pm, pb) = (plane.forward(next_to(a, m.x)), plane.forward(m), plane.forward(next_to(b, m.x)));
        let d = 2.0 * (pa.x * (pm.y - pb.y) + pm.x * (pb.y - pa.y) + pb.x * (pa.y - pm.y));
        let straight = vec![a, m, b];
        if !d.is_finite() || d.abs() < 1e-9 {
            notes.straight_arcs = true;
            return straight;
        }
        let sq = |p: Coord| p.x * p.x + p.y * p.y;
        let ux = (sq(pa) * (pm.y - pb.y) + sq(pm) * (pb.y - pa.y) + sq(pb) * (pa.y - pm.y)) / d;
        let uy = (sq(pa) * (pb.x - pm.x) + sq(pm) * (pa.x - pb.x) + sq(pb) * (pm.x - pa.x)) / d;
        let r = (pa.x - ux).hypot(pa.y - uy);
        if !r.is_finite() || r > MAX_RADIUS_M {
            notes.straight_arcs = true;
            return straight;
        }
        notes.arcs = true;
        let ang = |p: Coord| (p.y - uy).atan2(p.x - ux);
        // Counter-clockwise when a → m → b turns left.
        let ccw = (pm.x - pa.x) * (pb.y - pm.y) - (pm.y - pa.y) * (pb.x - pm.x) > 0.0;
        let a0 = ang(pa);
        let tau = std::f64::consts::TAU;
        let sweep = if circle {
            if ccw { tau } else { -tau }
        } else {
            let s = (ang(pb) - a0).rem_euclid(tau);
            if ccw { s } else { s - tau }
        };
        let n = self.pieces(r, sweep.to_degrees(), notes);
        (0..=n)
            .map(|k| {
                let t = a0 + sweep * k as f64 / n as f64;
                next_to(plane.inverse(Coord::new(ux + r * t.cos(), uy + r * t.sin())), m.x)
            })
            .collect()
    }

    /// A ring: a `gml:Ring` of curve members, or a `gml:LinearRing`; closed.
    fn ring(&mut self, boundary: Node, notes: &mut ShapeNotes) -> Option<Vec<Coord>> {
        let r = elements(boundary).next()?;
        // Borders followed (cut, or whole) and those far from where they meet, counted once the
        // ring is drawn.
        let (mut followed, mut whole, mut far) = (0, 0, 0);
        let mut pts: Vec<Coord> = match local(&r) {
            "LinearRing" => {
                let axes = self.axes(r)?;
                Self::positions(r, axes, notes)?
            }
            "Ring" => {
                // Its members: curves written out, and borders given by reference, which are cut
                // once the points either side of them are known.
                let mut members: Vec<std::result::Result<Vec<Coord>, String>> = Vec::new();
                for m in elements(r).filter(|c| local(c) == "curveMember") {
                    match elements(m).next() {
                        Some(c) => members.push(Ok(self.curve(c, notes)?)),
                        None => {
                            // The same border named again straight after is the same stretch of
                            // it (DFS writes a border's members so), followed once.
                            let key = border_key(m.attribute((XLINK, "href"))?);
                            if !matches!(members.last(), Some(Err(k)) if *k == key) {
                                members.push(Err(key));
                            }
                        }
                    }
                }
                if members.len() > 1 && matches!((members.first(), members.last()), (Some(Err(a)), Some(Err(b))) if a == b) {
                    members.pop();
                }
                let mut out: Vec<Coord> = Vec::new();
                let n = members.len();
                // Each member's nearest written points before and after it, round the ring: two
                // passes, however many members are references.
                let end_of = |m: Option<&std::result::Result<Vec<Coord>, String>>| match m {
                    Some(Ok(c)) => c.last().copied(),
                    _ => None,
                };
                let start_of = |m: Option<&std::result::Result<Vec<Coord>, String>>| match m {
                    Some(Ok(c)) => c.first().copied(),
                    _ => None,
                };
                let mut befores: Vec<Option<Coord>> = vec![None; n];
                let mut carry = (0..n).rev().find_map(|k| end_of(members.get(k)));
                for i in 0..n {
                    if let Some(b) = befores.get_mut(i) {
                        *b = carry;
                    }
                    carry = end_of(members.get(i)).or(carry);
                }
                let mut afters: Vec<Option<Coord>> = vec![None; n];
                let mut carry = (0..n).find_map(|k| start_of(members.get(k)));
                for i in (0..n).rev() {
                    if let Some(a) = afters.get_mut(i) {
                        *a = carry;
                    }
                    carry = start_of(members.get(i)).or(carry);
                }
                for i in 0..n {
                    let mut part = match members.get(i) {
                        Some(Ok(c)) => c.clone(),
                        Some(Err(key)) => {
                            // Next to another border (a state's outline is a chain of them), the
                            // stretch starts or ends where the two meet: this border's end nearest
                            // that one. Next to written points, at the nearest of those.
                            let neighbor = |k: usize| match members.get(k % n) {
                                Some(Err(other)) if other != key && n > 1 => Some(other.clone()),
                                _ => None,
                            };
                            let (prev, next) = (neighbor(i + n - 1), neighbor(i + 1));
                            // The same border either side (a ring of two borders): they meet at
                            // both its ends, so it is followed whole, turned to meet the line.
                            let (before, after) = if prev.is_some() && prev == next {
                                (None, None)
                            } else {
                                // Where it meets the borders either side; a gap between two is
                                // counted once, from the first of them.
                                // One that can't be worked out (past the work allowed, or a
                                // border not there to meet) leaves the ring undrawn, and why is said.
                                let mut meet = |s: &mut Self, other: Option<String>, count_gap: bool| match other {
                                    Some(other) => s.junction(key, &other).map(|(c, gap)| {
                                        far += usize::from(gap && count_gap);
                                        Some(c)
                                    }),
                                    None => Ok(None),
                                };
                                let (before, after) = match (meet(self, prev, false), meet(self, next, true)) {
                                    (Ok(b), Ok(a)) => (b, a),
                                    (Err(why), _) | (_, Err(why)) => {
                                        say_unfollowed(why, notes);
                                        return None;
                                    }
                                };
                                (before.or_else(|| befores.get(i).copied().flatten()), after.or_else(|| afters.get(i).copied().flatten()))
                            };
                            // Said as a border by reference by the ring's patch, not also as a
                            // ring left out; each why in its own notice.
                            match self.follow(key, before, after, notes) {
                                Ok((part, off)) => {
                                    far += usize::from(off);
                                    if before.is_none() && after.is_none() {
                                        whole += 1;
                                    } else {
                                        followed += 1;
                                    }
                                    match out.last() {
                                        // A whole border turned to start where the line is.
                                        Some(&end) if before.is_none() && after.is_none() => {
                                            let gap =
                                                |c: Option<&Coord>| c.map_or(f64::INFINITY, |c| rustgis_geom::wgs84_distance_m(*c, end));
                                            if gap(part.last()) < gap(part.first()) { part.into_iter().rev().collect() } else { part }
                                        }
                                        _ => part,
                                    }
                                }
                                Err(why) => {
                                    say_unfollowed(why, notes);
                                    return None;
                                }
                            }
                        }
                        None => return None,
                    };
                    if let Some(last) = out.last().copied() {
                        part = part.into_iter().map(|p| next_to(p, last.x)).collect();
                        if part.first().is_some_and(|f| (f.x - last.x).abs() < 1e-12 && (f.y - last.y).abs() < 1e-12) {
                            part.remove(0);
                        }
                    }
                    out.extend(part);
                }
                out
            }
            _ => return None,
        };
        let first = *pts.first()?;
        if pts.last().is_none_or(|l| (l.x - first.x).abs() > 1e-9 || (l.y - first.y).abs() > 1e-9) {
            pts.push(first);
        } else if let Some(l) = pts.last_mut() {
            *l = first;
        }
        if pts.len() < 4 {
            return None;
        }
        notes.followed += followed;
        notes.followed_whole += whole;
        notes.far += far;
        Some(pts)
    }

    /// The part of a border given by reference (`key`, [`border_key`]) a ring follows: from the
    /// point on it nearest `before` (the end of the member before) to the one nearest `after` (the
    /// start of the next), the shorter way round a border that closes; the whole border when the
    /// ring has no other points. AIXM leaves this cut to the reader (its coding guidance, "Geoborder
    /// for Airspace Border"), so each is counted, and those whose ends lie far from the border.
    fn follow(
        &mut self,
        key: &str,
        before: Option<Coord>,
        after: Option<Coord>,
        notes: &mut ShapeNotes,
    ) -> std::result::Result<(Vec<Coord>, bool), Unfollowed> {
        let scan = self.borders.get(key).ok_or(Unfollowed::Missing)?.len();
        if scan < 2 {
            return Err(Unfollowed::Unusable);
        }
        // Looking along borders has an allowance of its own, set by the file's size, so
        // references can't make the looking outgrow it, nor take the points arcs are drawn with.
        if !self.scan(scan) {
            return Err(Unfollowed::Allowance);
        }
        let border = self.borders.get(key).ok_or(Unfollowed::Missing)?;
        let (part, far) = match (before, after) {
            (Some(a), Some(b)) => {
                let (part, off) = cut(border, a, b).ok_or(Unfollowed::Unusable)?;
                (part, off > FAR_FROM_BORDER_M)
            }
            _ => (border.clone(), false),
        };
        // Its points are paid for as a curve's added points are; one that can't be takes none.
        let n = part.len();
        if self.budget < n {
            return Err(Unfollowed::Allowance);
        }
        self.take(n, notes);
        Ok((part, far))
    }

    /// Takes `n` steps of looking along borders from their allowance: false when it is spent.
    fn scan(&mut self, n: usize) -> bool {
        match self.scan_left.checked_sub(n) {
            Some(left) => {
                self.scan_left = left;
                true
            }
            None => false,
        }
    }

    /// Where the border `key` meets the border `other` next to it in a ring: the point of `key`
    /// nearest `other`, of those an end of either gives (its own ends, or the point on it nearest
    /// one of `other`'s, where `other` runs on past where they meet or stops short). Looking along
    /// both is paid for as points are.
    fn junction(&mut self, key: &str, other: &str) -> std::result::Result<(Coord, bool), Unfollowed> {
        let scan = self.borders.get(other).ok_or(Unfollowed::Missing)?.len() + self.borders.get(key).ok_or(Unfollowed::Missing)?.len();
        if !self.scan(scan) {
            return Err(Unfollowed::Allowance);
        }
        let (line, them) = (self.borders.get(key).ok_or(Unfollowed::Missing)?, self.borders.get(other).ok_or(Unfollowed::Missing)?);
        let mut best: Option<(f64, Coord)> = None;
        let mut consider = |d: f64, c: Coord| {
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        };
        for end in [line.first(), line.last()].into_iter().flatten() {
            if let Some((.., d)) = nearest_on(them, *end) {
                consider(d, *end);
            }
        }
        for end in [them.first(), them.last()].into_iter().flatten() {
            if let Some((_, _, on, d)) = nearest_on(line, *end) {
                consider(d, on);
            }
        }
        // Borders that don't meet (a gap between them) are joined across it, and that is said.
        let (d, c) = best.ok_or(Unfollowed::Unusable)?;
        Ok((c, d > FAR_FROM_BORDER_M))
    }

    /// A surface's polygon patches (or a polygon): each its exterior and interiors.
    fn surface(&mut self, g: Node, notes: &mut ShapeNotes) -> Option<Geometry> {
        let patches: Vec<Node> = if local(&g) == "Polygon" {
            vec![g]
        } else {
            g.descendants().filter(|d| d.is_element() && is_gml(d) && matches!(local(d), "PolygonPatch" | "Polygon")).collect()
        };
        let mut polys = Vec::new();
        for p in patches {
            // A ring lost to a border by reference is said as that, once.
            let said = |n: &ShapeNotes| n.referenced + n.border_allowance + n.border_unusable;
            let before = said(notes);
            let outer = elements(p).find(|c| local(c) == "exterior").and_then(|b| self.ring(b, notes));
            let Some(outer) = outer else {
                notes.rings_lost += usize::from(said(notes) == before);
                continue;
            };
            // A hole that can't be read leaves the patch out: drawn without it, the patch would
            // cover what the hole excludes.
            let mut rings = vec![outer];
            let mut hole_lost = false;
            for b in elements(p).filter(|c| local(c) == "interior") {
                match self.ring(b, notes) {
                    Some(r) => rings.push(r),
                    None => hole_lost = true,
                }
            }
            // The patch left out for its hole is counted, whatever lost the hole (said too).
            if hole_lost {
                notes.rings_lost += 1;
            } else {
                polys.push(rings);
            }
        }
        (!polys.is_empty()).then_some(Geometry::Polygon(polys))
    }

    /// A shape read without saying what it finds: one read again elsewhere (a border or point kept
    /// for the features that name it), or before its feature is known to be drawn from it (a
    /// route segment's ends). The points it adds are paid for all the same, so reading again can't
    /// add points past the allowance.
    fn quietly(&mut self, g: Node) -> Option<Geometry> {
        let kept = (self.assumed_system, self.other_count, self.as_wgs84_count, self.other_system.clone(), self.as_wgs84.clone());
        let shape = self.shape(g, &mut ShapeNotes::default());
        (self.assumed_system, self.other_count, self.as_wgs84_count, self.other_system, self.as_wgs84) = kept;
        shape
    }

    /// A geometry element (AIXM's or GML's) as a geometry.
    fn shape(&mut self, g: Node, notes: &mut ShapeNotes) -> Option<Geometry> {
        (self.assumed_now, self.other_now, self.as_wgs84_now) = (false, false, false);
        let shape = self.read_shape(g, notes);
        self.assumed_system += usize::from(self.assumed_now);
        self.other_count += usize::from(self.other_now);
        self.as_wgs84_count += usize::from(self.as_wgs84_now);
        shape
    }

    fn read_shape(&mut self, g: Node, notes: &mut ShapeNotes) -> Option<Geometry> {
        if self.names_only {
            let o = Coord::new(0.0, 0.0);
            return Some(match element_family(local(&g)) {
                "points" => Geometry::Point(o),
                "lines" => Geometry::Line(vec![vec![o, o]]),
                _ => Geometry::Polygon(vec![vec![vec![o, o, o, o]]]),
            });
        }
        match local(&g) {
            "Point" | "ElevatedPoint" => self.point(g, notes),
            "Curve" | "ElevatedCurve" | "LineString" => self.curve(g, notes).map(|c| Geometry::Line(vec![c])),
            "Surface" | "ElevatedSurface" | "Polygon" => self.surface(g, notes),
            _ => None,
        }
    }
}

/// Why a border given by reference wasn't followed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unfollowed {
    /// No GeoBorder by that name in the file, in force at the date.
    Missing,
    /// Following it would pass the work the file's size allows.
    Allowance,
    /// A border too short to cut (fewer than two points).
    Unusable,
}

/// Counts why a border given by reference wasn't followed.
fn say_unfollowed(why: Unfollowed, notes: &mut ShapeNotes) {
    match why {
        Unfollowed::Missing => notes.referenced += 1,
        Unfollowed::Allowance => notes.border_allowance += 1,
        Unfollowed::Unusable => notes.border_unusable += 1,
    }
}

/// How far from a border the ends of a part cut from it may lie before that is said, in meters.
const FAR_FROM_BORDER_M: f64 = 50.0;

/// What a reference names a border by: `#` and a curve's `gml:id`, or its feature's identifier.
fn border_key(href: &str) -> String {
    let h = href.trim();
    match h.strip_prefix('#') {
        Some(id) => format!("#{id}"),
        None => format!("uuid:{}", h.strip_prefix("urn:uuid:").unwrap_or(h)),
    }
}

/// Where on `line` a point is nearest `p`: the segment, how far along it (0 to 1), the point, and
/// its distance from `p` in meters. Worked out in a plane around `p` (longitude scaled by the
/// cosine of its latitude), fine at the scale of a border's segments.
fn nearest_on(line: &[Coord], p: Coord) -> Option<(usize, f64, Coord, f64)> {
    let k = p.y.to_radians().cos().max(1e-6);
    let mut best: Option<(usize, f64, Coord, f64)> = None;
    for (i, w) in line.windows(2).enumerate() {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        // The segment moved as one next to the point: its ends keep their short span across 180°.
        let a = next_to(a, p.x);
        let b = next_to(b, a.x);
        let (dx, dy) = ((b.x - a.x) * k, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.x - a.x) * k * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = Coord::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        let d2 = ((q.x - p.x) * k).powi(2) + (q.y - p.y).powi(2);
        if best.is_none_or(|(_, _, _, bd)| d2 < bd) {
            best = Some((i, t, q, d2));
        }
    }
    best.map(|(i, t, q, _)| (i, t, q, rustgis_geom::wgs84_distance_m(q, p)))
}

/// The part of `line` from the point nearest `a` to the one nearest `b`, and the farther of the
/// two ends from its point. Along the line either way; on a line that closes (an island's
/// coast), the shorter way round.
fn cut(line: &[Coord], a: Coord, b: Coord) -> Option<(Vec<Coord>, f64)> {
    let (ia, ta, pa, da) = nearest_on(line, a)?;
    let (ib, tb, pb, db) = nearest_on(line, b)?;
    let at = |i: usize| line.get(i).copied();
    let forward = (ia as f64 + ta) <= (ib as f64 + tb);
    // Along the line: the vertices between the two points, in the order traveled.
    let mut direct = vec![pa];
    if forward {
        direct.extend((ia + 1..=ib).filter_map(at));
    } else {
        direct.extend((ib + 1..=ia).rev().filter_map(at));
    }
    direct.push(pb);
    let closes = match (line.first(), line.last()) {
        (Some(f), Some(l)) => line.len() > 3 && (f.x - l.x).abs() < 1e-9 && (f.y - l.y).abs() < 1e-9,
        _ => false,
    };
    let part = if closes {
        // The other way round, through where the line closes (its last point is its first).
        let last = line.len() - 1;
        let mut round = vec![pa];
        if forward {
            round.extend((0..=ia).rev().filter_map(at));
            round.extend((ib + 1..last).rev().filter_map(at));
        } else {
            round.extend((ia + 1..last).filter_map(at));
            round.extend((0..=ib).filter_map(at));
        }
        round.push(pb);
        let length = |l: &[Coord]| l.windows(2).filter_map(|w| Some(rustgis_geom::wgs84_distance_m(*w.first()?, *w.get(1)?))).sum::<f64>();
        if length(&round) < length(&direct) { round } else { direct }
    } else {
        direct
    };
    Some((part, da.max(db)))
}

/// Meters in one of a length's units: UCUM's (`m`, `km`, `[nmi_i]`, `[ft_i]`, `[mi_i]`) and AIXM's
/// (`M`, `KM`, `NM`, `FT`, `MI`).
fn meters_per(uom: &str) -> Option<f64> {
    Some(match uom.trim() {
        "m" | "M" => 1.0,
        "cm" | "CM" => 0.01,
        "km" | "KM" => 1000.0,
        "[nmi_i]" | "NM" | "nmi" => 1852.0,
        "[ft_i]" | "FT" | "ft" => 0.3048,
        "[mi_i]" | "MI" | "mi" => 1609.344,
        _ => return None,
    })
}

/// A property's geometry element, when it holds one.
fn shape_in<'a, 'i>(p: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    elements(p).find(|c| (is_aixm(c) || is_gml(c)) && SHAPES.contains(&local(c)))
}

// ---------------------------------------------------------------------------------------------
// Rows and layers.
// ---------------------------------------------------------------------------------------------

/// The first few distinct names a notice counts; more are said as `…`.
#[derive(Default)]
struct Names(Vec<String>, bool);

impl Names {
    fn add(&mut self, p: &str) {
        if !self.0.iter().any(|n| n == p) {
            if self.0.len() < 3 {
                self.0.push(p.to_string());
            } else {
                self.1 = true;
            }
        }
    }
    fn said(&self) -> String {
        self.0.join(", ") + if self.1 { ", …" } else { "" }
    }
}

/// A row's values by column; a column with no value is null.
type Attrs = BTreeMap<String, String>;

/// A layer being read.
#[derive(Default)]
struct Layer {
    kind: String,
    label: String,
    columns: Vec<String>,
    /// Each column's number.
    named: HashMap<String, usize>,
    /// Its rows: their shape, their simple values by column number, their nested ones by path.
    rows: Vec<(Option<Geometry>, Vec<(usize, String)>, Nested)>,
    /// Values not kept: past the memory the file's size allows.
    over_bytes: usize,
    /// Values of properties past the column cap, or nested past [`MAX_PATH`].
    capped: usize,
    /// Features kept without a shape: no geometry, or one that couldn't be read.
    shapeless: usize,
    unreadable: usize,
    arcs: usize,
    coarse: usize,
    zm: usize,
    unread_segments: Names,
    approximated: usize,
    rings_lost: usize,
    referenced: usize,
    /// Airspaces kept without a shape: built from other airspace or by subtraction/intersection.
    derived: usize,
    derived_names: Names,
    /// Airspaces drawn as the parts of BASE and UNION volumes.
    unions: usize,
    /// Borders given by reference followed along a GeoBorder, and those with ends far from it.
    followed: usize,
    far: usize,
    /// Route segments drawn between the points they name.
    between: usize,
    /// Features with an arc or circle not given the three positions it needs.
    short_arcs: usize,
    /// Features with an arc or circle drawn straight: its points in a line, or too far apart.
    straight_arcs: usize,
    /// Features with an arc or circle with no positions, left out.
    empty_arcs: usize,
    /// Route segments with no curve and a `pathType` RustGIS doesn't draw.
    path_unread: usize,
    /// Rows left out: the table they'd make passes the memory the file's size allows.
    dense_over: usize,
    /// Records holding no shape beside records of the same property that do: rows of their own.
    unlocated: usize,
    /// Borders by reference followed whole.
    followed_whole: usize,
    /// Borders by reference not followed: past the work allowed, or too short to cut.
    border_allowance: usize,
    border_unusable: usize,
    /// Shapes in the records of features drawn from their own shapes, not drawn.
    shapes_in_records: usize,
    /// Features as a TEMPDELTA changed them.
    temporary: usize,
    snapshots: usize,
}

impl Layer {
    /// A column's number, made when there is room for another; `None` (counted) past
    /// [`MAX_COLUMNS`].
    fn column(&mut self, name: &str, value: bool) -> Option<usize> {
        if let Some(&i) = self.named.get(name) {
            return Some(i);
        }
        // Only a value past the cap is one not kept.
        if self.columns.len() >= MAX_COLUMNS {
            self.capped += usize::from(value);
            return None;
        }
        self.named.insert(name.to_string(), self.columns.len());
        self.columns.push(name.to_string());
        Some(self.columns.len() - 1)
    }

    /// The notes of a row's shapes. How a shape was drawn is said only for a row that is drawn;
    /// why something wasn't is said either way.
    fn take_notes(&mut self, n: &ShapeNotes, drawn: bool) {
        if drawn {
            self.arcs += usize::from(n.arcs);
            self.coarse += usize::from(n.coarse);
            self.zm += usize::from(n.zm);
            if !n.unread_segments.is_empty() {
                self.approximated += 1;
                for s in &n.unread_segments {
                    self.unread_segments.add(s);
                }
            }
            self.followed += n.followed;
            self.followed_whole += n.followed_whole;
            self.far += n.far;
            self.between += usize::from(n.between);
            self.short_arcs += usize::from(n.short_arcs);
            self.straight_arcs += usize::from(n.straight_arcs);
            self.empty_arcs += usize::from(n.empty_arcs);
        }
        self.rings_lost += n.rings_lost;
        self.referenced += n.referenced;
        self.path_unread += usize::from(n.path_unread);
        self.border_allowance += n.border_allowance;
        self.border_unusable += n.border_unusable;
    }
}

/// A path to a nested value: every step a property's name and which of its kind among its
/// siblings it is (from 1).
type Path = Vec<(String, usize)>;

/// Every path a read meets, each kept once: a value holds its path's number, not a copy of it.
#[derive(Default)]
struct Paths {
    ids: HashMap<Path, usize>,
    list: Vec<Path>,
    /// The bytes the paths hold, and the most they may: one allowance for the whole read.
    bytes: usize,
    limit: usize,
}

/// The paths of a read, shared by its rows.
type SharedPaths = std::rc::Rc<std::cell::RefCell<Paths>>;

/// A row's values nested in records, each by its path's number ([`Paths`]). `None` is a value
/// given as missing (`nil`). They are named as columns once the layer's rows are all read
/// ([`path_columns`]).
type Nested = Vec<(usize, Option<String>)>;

/// A row being built: its values, simple ones by column and nested ones by path.
#[derive(Default, Clone)]
struct Row {
    attrs: Attrs,
    /// Every column named, null or not, in order, and the same as a set.
    columns: Vec<String>,
    named: HashSet<String>,
    /// Values in records, by path, and the read's paths.
    paths: Nested,
    interned: SharedPaths,
    /// How many records of each property the feature has had so far.
    records: HashMap<String, usize>,
    /// Values of names past [`MAX_COLUMNS`], or nested past [`MAX_PATH`], not kept.
    capped: usize,
    /// The bytes its values and their names and paths hold, and the most they may (none until
    /// set: a row being read stops keeping values past what the file's size allows).
    bytes: usize,
    limit: Option<usize>,
    /// Values not kept: past the memory the file's size allows.
    over_bytes: usize,
}

impl Row {
    /// Names a column; false past [`MAX_COLUMNS`] names, when its value isn't kept (counted).
    fn name(&mut self, key: &str, value: bool) -> bool {
        if self.named.contains(key) {
            return true;
        }
        if self.columns.len() >= MAX_COLUMNS {
            self.capped += usize::from(value);
            return false;
        }
        self.named.insert(key.to_string());
        self.columns.push(key.to_string());
        true
    }

    /// Takes `cost` bytes for a value, or counts it as not kept past the row's limit.
    fn afford(&mut self, cost: usize) -> bool {
        let cost = cost + VALUE_OVERHEAD;
        if self.limit.is_some_and(|l| self.bytes + cost > l) {
            self.over_bytes += 1;
            return false;
        }
        self.bytes += cost;
        true
    }

    /// The number of a path, kept once for the read: a path met for the first time is paid for in
    /// bytes, so the paths together can't outgrow what the file's size allows.
    fn path_id(&mut self, at: Path) -> Option<usize> {
        let shared = std::rc::Rc::clone(&self.interned);
        let Ok(mut paths) = shared.try_borrow_mut() else {
            self.capped += 1;
            return None;
        };
        if let Some(&id) = paths.ids.get(&at) {
            return Some(id);
        }
        // A new path is kept twice (in the list and as its key): charged so to the read's
        // allowance for paths, whichever row meets it first, kept or not.
        let cost = 2 * at.iter().map(|(n, _)| n.len() + VALUE_OVERHEAD).sum::<usize>() + 4 * VALUE_OVERHEAD;
        if paths.bytes + cost > paths.limit {
            self.over_bytes += 1;
            return None;
        }
        paths.bytes += cost;
        let id = paths.list.len();
        paths.list.push(at.clone());
        paths.ids.insert(at, id);
        Some(id)
    }

    /// What the row holds, in bytes: what keeping it, or a copy of it, costs.
    fn weight(&self) -> usize {
        self.bytes.max(1)
    }

    fn set(&mut self, key: &str, value: String) {
        if !self.name(key, true) || !self.afford(key.len() + value.len()) {
            return;
        }
        match self.attrs.get_mut(key) {
            // A property given more than once keeps every value.
            Some(v) if !value.is_empty() => {
                v.push_str("; ");
                v.push_str(&value);
            }
            Some(_) => {}
            None => {
                self.attrs.insert(key.to_string(), value);
            }
        }
    }

    fn null(&mut self, key: &str) {
        let _ = self.name(key, false);
    }

    /// A property's simple value as attributes, under `prefix`.
    fn property(&mut self, p: Node, prefix: &str) {
        let name = format!("{prefix}{}", local(&p));
        if !is_nil(&p) && elements(p).next().is_some() && p.attribute((XLINK, "href")).is_none() {
            // A record: each of its values under its path, numbered by which record it is.
            let n = self.records.entry(name.clone()).or_default();
            *n += 1;
            let mut path = vec![(name, *n)];
            self.record(p, &mut path, false);
            return;
        }
        for (key, value) in leaf_values(p, &name) {
            match value {
                Some(v) => self.set(&key, v),
                None => self.null(&key),
            }
        }
    }

    /// The values in a record (a property holding an object, or the object), each under `path`
    /// and its own name. An object (`AuthorityForNavaidEquipment`, `gml:TimePeriod`) adds no step:
    /// its properties are the record's. A shape's GML isn't a value (it is the feature's shape, or
    /// one drawn with it); the values AIXM writes on it (elevation, accuracy) are. With
    /// `aixm_only`, only AIXM's properties are read.
    fn record(&mut self, node: Node, path: &mut Vec<(String, usize)>, aixm_only: bool) {
        self.record_at(node, path, aixm_only, 0);
    }

    /// As [`Row::record`], `depth` records and objects down: bounded whether the depth is in
    /// properties (the path) or in objects (which add no step to it).
    fn record_at(&mut self, node: Node, path: &mut Vec<(String, usize)>, aixm_only: bool, depth: usize) {
        if path.len() > MAX_PATH || depth > 2 * MAX_PATH {
            self.capped += 1;
            return;
        }
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for c in elements(node) {
            if aixm_only && !is_aixm(&c) {
                continue;
            }
            let name = local(&c);
            // AIXM and GML name objects with a capital, properties without.
            if name.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                let shape = SHAPES.contains(&name) && (is_aixm(&c) || is_gml(&c));
                let links = c.attribute((XLINK, "href")).is_some_and(|h| !h.trim().is_empty());
                if elements(c).next().is_some() {
                    // An object that links elsewhere and holds values too keeps both.
                    if links && !shape {
                        path.push((name.to_string(), 1));
                        for (key, value) in leaf_values(c, name).into_iter().filter(|(k, _)| k == name || k.ends_with("_title")) {
                            let mut at = path.clone();
                            if let Some(step) = at.last_mut() {
                                step.0 = key;
                            }
                            let cost = value.as_ref().map_or(0, String::len);
                            if self.afford(cost) {
                                match self.path_id(at) {
                                    Some(id) => self.paths.push((id, value)),
                                    None => self.bytes = self.bytes.saturating_sub(cost + VALUE_OVERHEAD),
                                }
                            }
                        }
                        path.pop();
                    }
                    self.record_at(c, path, shape, depth + 1);
                    continue;
                }
                // An object with nothing in it holds no value; one with text, or what an attribute
                // says (a link, why it is missing, a unit), is a value.
                let says = !shape && (is_nil(&c) || links || c.attributes().any(|a| value_attribute(&a).is_some()));
                if text_of(&c).is_empty() && !says {
                    continue;
                }
            }
            let n = seen.entry(name).or_default();
            *n += 1;
            path.push((name.to_string(), *n));
            if elements(c).next().is_some() && c.attribute((XLINK, "href")).is_none() {
                self.record_at(c, path, false, depth + 1);
            } else if let Some((last, _)) = path.last().cloned() {
                for (key, value) in leaf_values(c, &last) {
                    let mut at = path.clone();
                    if let Some(step) = at.last_mut() {
                        step.0 = key;
                    }
                    // Paid for first; given back when its path can't be kept.
                    let cost = value.as_ref().map_or(0, String::len);
                    if !self.afford(cost) {
                        continue;
                    }
                    match self.path_id(at) {
                        Some(id) => self.paths.push((id, value)),
                        None => self.bytes = self.bytes.saturating_sub(cost + VALUE_OVERHEAD),
                    }
                }
            }
            path.pop();
        }
    }

    /// The simple values written on a geometry element (an `ElevatedPoint`'s elevation and datum,
    /// a point's accuracy), each under its own name or, where the row has that name, the
    /// property's.
    fn shape_values(&mut self, g: Node, property: &str) {
        for c in elements(g).filter(is_aixm) {
            let prefix = if self.named.contains(local(&c)) { format!("{property}_") } else { String::new() };
            self.property(c, &prefix);
        }
    }
}

/// The identifier a reference names: a UUID (`urn:uuid:` left off), a local `#id`, or the
/// `gml:id` an XPath picks (the FAA's `#/faa:SubscriberFile/faa:Member/aixm:Navaid[@gml:id='N1']`,
/// brackets written as `%5B`/`%5D` or not): what the feature it names is known by in its columns.
fn reference(href: &str) -> String {
    let h = href.trim();
    if let Some(at) = h.find("@gml:id") {
        let rest = h.get(at..).unwrap_or_default();
        let mut quoted = rest.split(['\'', '"']);
        if let (Some(_), Some(id)) = (quoted.next(), quoted.next()) {
            return id.trim().to_string();
        }
    }
    h.strip_prefix("urn:uuid:").or_else(|| h.strip_prefix('#')).unwrap_or(h).to_string()
}

/// A property's value and those written on it, each under its own name from `name`: its text (or
/// `None` when given as missing), the identifier a reference names (`urn:uuid:` left off) with its
/// title (`_title`), its unit (`_uom`), why it is missing (`_nilReason`), its language (`_lang`)
/// and any other attribute (`_` and its name). An element's id and the XML plumbing aren't values.
fn leaf_values(p: Node, name: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    if is_nil(&p) {
        out.push((name.to_string(), None));
    } else if let Some(href) = p.attribute((XLINK, "href")) {
        out.push((name.to_string(), Some(reference(href))));
    } else {
        out.push((name.to_string(), Some(text_of(&p))));
    }
    for a in p.attributes() {
        if let Some(key) = value_attribute(&a) {
            out.push((format!("{name}_{key}"), Some(a.value().trim().to_string())));
        }
    }
    out
}

/// The name an attribute is kept under, when it holds a value: a reference's title, a unit, why
/// a value is missing, a language, any other; not an element's id or the XML plumbing (other
/// XLink and XML Schema attributes).
fn value_attribute<'a>(a: &roxmltree::Attribute<'a, '_>) -> Option<&'a str> {
    match (a.namespace(), a.name()) {
        (Some(XLINK), "title") => Some("title"),
        (Some(XLINK) | Some(XSI), _) | (Some(GML), "id") | (None, "id") => None,
        (_, other) => Some(other),
    }
}

/// The column of each nested value of a layer's rows: its path's names joined by `_`, each step
/// numbered (`authority_1_type`) when its record repeats anywhere in the layer, so every row's
/// values of one record share their columns.
fn path_columns(rows: &[(Option<Geometry>, Vec<(usize, String)>, Nested)], paths: &Paths) -> HashSet<String> {
    let used: HashSet<usize> = rows.iter().flat_map(|(_, _, nested)| nested.iter().map(|(id, _)| *id)).collect();
    let mut repeating = HashSet::new();
    for id in used {
        let Some(path) = paths.list.get(id) else { continue };
        {
            let mut key = String::new();
            for (name, n) in path {
                key.push('/');
                key.push_str(name);
                if *n > 1 {
                    repeating.insert(key.clone());
                }
            }
        }
    }
    repeating
}

/// A nested value's column, as [`path_columns`] numbers it.
fn path_column(path: &[(String, usize)], repeating: &HashSet<String>) -> String {
    let (mut key, mut column) = (String::new(), String::new());
    for (name, n) in path {
        key.push('/');
        key.push_str(name);
        if !column.is_empty() {
            column.push('_');
        }
        column.push_str(name);
        if repeating.contains(&key) {
            column.push_str(&format!("_{n}"));
        }
    }
    column
}

/// The layers of a file in the order their kinds were first met.
#[derive(Default)]
struct Layers {
    by_key: HashMap<(String, String), usize>,
    /// Bytes rows may still hold: rows copied for a feature's parts or records are bounded by the
    /// file's size, not by how many times a feature repeats them or how long its values are.
    bytes_left: usize,
    /// Rows left out past that.
    rows_over: usize,
    list: Vec<Layer>,
    /// Features of kinds past [`MAX_LAYERS`], by kind and identifier.
    over: HashSet<String>,
    /// The feature being read (its kind and identifier): what `over` counts.
    feature: String,
    /// Only the layers' names are wanted: rows aren't kept.
    names_only: bool,
}

impl Layers {
    /// The layer of a kind and label, made when there is room for another.
    fn index_of(&mut self, kind: &str, label: &str) -> Option<usize> {
        let key = (kind.to_string(), label.to_string());
        match self.by_key.get(&key) {
            Some(&i) => Some(i),
            None if self.list.len() >= MAX_LAYERS => None,
            None => {
                self.list.push(Layer { kind: kind.to_string(), label: label.to_string(), ..Layer::default() });
                self.by_key.insert(key, self.list.len() - 1);
                Some(self.list.len() - 1)
            }
        }
    }

    fn at(&mut self, i: Option<usize>) -> Option<&mut Layer> {
        i.and_then(|i| self.list.get_mut(i))
    }

    fn has_kind(&self, kind: &str) -> bool {
        self.list.iter().any(|l| l.kind == kind)
    }

    /// Adds a row to the layer of `kind` and `label`, which it returns; `None` when it is left out
    /// (counted): past the values the file's size allows, or a kind past [`MAX_LAYERS`].
    /// `change` says whether the feature is as a TEMPDELTA has it, or a SNAPSHOT.
    fn push(
        &mut self,
        kind: &str,
        label: &str,
        geometry: Option<Geometry>,
        row: Row,
        notes: Option<&ShapeNotes>,
        change: (bool, bool),
    ) -> Option<usize> {
        let cells = row.weight();
        if cells > self.bytes_left {
            self.rows_over += 1;
            return None;
        }
        let Some(i) = self.index_of(kind, label) else {
            self.over.insert(self.feature.clone());
            return None;
        };
        self.bytes_left -= cells;
        let layer = self.list.get_mut(i)?;
        layer.capped += row.capped;
        layer.over_bytes += row.over_bytes;
        layer.temporary += usize::from(change.0);
        layer.snapshots += usize::from(change.1);
        let mut attrs = Vec::with_capacity(row.attrs.len());
        for c in &row.columns {
            if let Some(k) = layer.column(c, row.attrs.contains_key(c))
                && let Some(v) = row.attrs.get(c)
            {
                attrs.push((k, v.clone()));
            }
        }
        if let Some(n) = notes {
            layer.take_notes(n, geometry.is_some());
        }
        if self.names_only {
            return Some(i);
        }
        layer.rows.push((geometry, attrs, row.paths));
        Some(i)
    }
}

/// A family label for a geometry.
fn family(g: &Geometry) -> &'static str {
    match g {
        Geometry::Point(_) | Geometry::MultiPoint(_) => "points",
        Geometry::Line(_) => "lines",
        Geometry::Polygon(_) => "polygons",
    }
}

/// Geometries of one family as one.
fn merged(gs: Vec<Geometry>) -> Option<Geometry> {
    let mut it = gs.into_iter();
    let first = it.next()?;
    Some(it.fold(first, |acc, g| match (acc, g) {
        (Geometry::Point(a), Geometry::Point(b)) => Geometry::MultiPoint(vec![a, b]),
        (Geometry::Point(a), Geometry::MultiPoint(mut b)) => {
            b.insert(0, a);
            Geometry::MultiPoint(b)
        }
        (Geometry::MultiPoint(mut a), Geometry::Point(b)) => {
            a.push(b);
            Geometry::MultiPoint(a)
        }
        (Geometry::MultiPoint(mut a), Geometry::MultiPoint(b)) => {
            a.extend(b);
            Geometry::MultiPoint(a)
        }
        (Geometry::Line(mut a), Geometry::Line(b)) => {
            a.extend(b);
            Geometry::Line(a)
        }
        (Geometry::Polygon(mut a), Geometry::Polygon(b)) => {
            a.extend(b);
            Geometry::Polygon(a)
        }
        (acc, _) => acc,
    }))
}

/// Bytes the layers' values may hold whatever the file's size.
const MIN_ROW_BYTES: usize = 64 << 20;
/// What keeping one value costs besides its text: its slot, its column's or path step's slot.
const VALUE_OVERHEAD: usize = 24;

/// Bytes wrapping slices may add whatever the file's size.
const MIN_WRAP_BYTES: usize = 16 << 20;

/// The scope (an index into `scopes`) of the namespaces in force where slice `sl` is written: the
/// root's (0) unless its member, feature, time slice or itself declares more, when they are
/// written out once for every slice like it. `None` when writing them would pass `wrap_left`.
fn scope_of(
    sl: Node,
    text: &str,
    start: usize,
    prefix: usize,
    scopes: &mut Vec<String>,
    ids: &mut HashMap<String, usize>,
    wrap_left: &mut usize,
) -> Option<usize> {
    // The start tags between the member and the slice's first child, looked at for declarations.
    let declares = sl.ancestors().take_while(|a| a.parent_element().is_some()).any(|a| {
        let r = a.range();
        let tag_end = a.first_child().map_or(r.end, |c| c.range().start);
        r.start
            .checked_sub(prefix)
            .zip(tag_end.checked_sub(prefix))
            .and_then(|(x, y)| text.get(start + x..start + y))
            .is_some_and(|t| t.contains("xmlns"))
    });
    if !declares {
        return Some(0);
    }
    // Each in-scope namespace costs its writing; paid before it is written.
    // Charged as written: escaping can make a URI six times longer (`"` as `&quot;`).
    let escaped = |u: &str| {
        u.len()
            + u.bytes()
                .map(|b| match b {
                    b'&' => 4,
                    b'<' => 3,
                    b'"' => 5,
                    _ => 0,
                })
                .sum::<usize>()
    };
    let cost =
        sl.namespaces().filter(|n| n.name() != Some("xml")).map(|n| escaped(n.uri()) + n.name().map_or(0, str::len) + 10).sum::<usize>();
    *wrap_left = wrap_left.checked_sub(cost)?;
    let mut decls = String::with_capacity(cost);
    for n in sl.namespaces().filter(|n| n.name() != Some("xml")) {
        let uri = n.uri().replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
        match n.name() {
            Some(name) => decls.push_str(&format!(" xmlns:{name}=\"{uri}\"")),
            None => decls.push_str(&format!(" xmlns=\"{uri}\"")),
        }
    }
    if let Some(&i) = ids.get(&decls) {
        return Some(i);
    }
    scopes.push(decls.clone());
    ids.insert(decls, scopes.len() - 1);
    Some(scopes.len() - 1)
}

/// What a change replaces a property by: its name, or for an `extension`, its name and the
/// namespace of the extension it holds (AIXM replaces extensions within their namespace).
fn property_key(p: Node) -> String {
    match (local(&p), elements(p).next().and_then(|e| e.tag_name().namespace())) {
        ("extension", Some(ns)) => format!("extension {ns}"),
        (name, _) => name.to_string(),
    }
}

/// How each member is parsed: no DTD, and the node limit every XML reader has.
fn options<'a>() -> ParsingOptions<'a> {
    ParsingOptions { allow_dtd: false, nodes_limit: crate::io::XML_NODES, ..ParsingOptions::default() }
}

/// A slice's properties of the feature (not those describing the slice).
fn own<'a, 'i>(n: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    elements(n).filter(|p| !SLICE_PROPERTIES.contains(&local(p))).collect()
}

/// The shape of an airspace volume's horizontal projection.
fn projection<'a, 'i>(v: Option<Node<'a, 'i>>) -> Option<Node<'a, 'i>> {
    v.and_then(|v| child(v, "horizontalProjection")).and_then(shape_in)
}

/// What the read says of the file as a whole.
#[derive(Default)]
struct FileNotes {
    /// Slices with a sequence or correction number that isn't a number, read as 0.
    bad_numbers: usize,
    /// Features left out because a change in force at the date couldn't be read.
    incomplete: usize,
    /// Time positions left empty without saying they are unknown, read as open.
    empty_dates: usize,
    /// Elements of members that aren't AIXM features.
    foreign: usize,
    /// Slices not read: wrapping or parsing them would pass the file's limits.
    past_limits: usize,
    not_in_force: usize,
    outside_lifetime: usize,
    no_state: usize,
    canceled: usize,
    unreadable_members: usize,
    unknown_interpretation: usize,
    bad_dates: usize,
    no_slices: usize,
    /// Features of kinds with nothing to draw, left out.
    nothing_to_draw: usize,
    nothing_names: Names,
    other_system: Option<(String, usize)>,
    assumed_system: usize,
    as_wgs84: Option<(String, usize)>,
}

/// Reads an AIXM message into a layer per feature kind (and per geometry property, for kinds with
/// several), at the effective date `opts.at` (today when not given). Cancel is looked at between
/// members; finding them first is one pass over the text without a look.
pub fn read(file_name: &str, bytes: &[u8], opts: &Options) -> Result<Vec<(Dataset, ReadReport)>> {
    let max = crate::io::max_bytes();
    if bytes.len() as u64 > max {
        return Err(DataError::Format(crate::io::file_size_error(file_name, max)));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        // A file cut inside a character is read up to it.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(bytes.get(..e.valid_up_to()).unwrap_or_default()).unwrap_or_default(),
        Err(_) => return Err(DataError::Format(format!("{file_name} is not UTF-8 text"))),
    };
    let text = text.trim_start_matches('\u{feff}');
    let stem = std::path::Path::new(file_name).file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("AIXM").to_string();
    let at = opts.at.filter(|t| t.is_finite()).unwrap_or_else(now_ms);
    let index = Index::of(file_name, text)?;
    if index.members.is_empty() {
        let mut why = Vec::new();
        if index.links > 0 {
            why.push(format!("{} only point elsewhere (xlink:href)", index.links));
        }
        if let Some(at) = index.broken_at {
            why.push(format!("the file stops being well-formed XML at byte {at}"));
        }
        let why = if why.is_empty() { String::new() } else { format!(": {}", why.join("; ")) };
        return Err(DataError::Format(format!("{file_name}: no features (hasMember or Member elements) in the AIXM message{why}")));
    }
    let mut work_left = crate::xml::namespace_budget(text.len());
    let wrap = |member: &str, out: &mut String| {
        out.clear();
        out.push_str("<m");
        out.push_str(&index.namespaces);
        out.push('>');
        out.push_str(member);
        out.push_str("</m>");
    };
    let mut notes = FileNotes::default();
    // First pass: each feature's slices, a member at a time.
    let mut features: HashMap<String, Feature> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut trouble = SliceTrouble::default();
    // The namespace declarations slices are parsed with in the second pass: the root's, and any a
    // slice has in scope besides (declared on its member, feature or itself), each written once.
    let mut scopes: Vec<String> = vec![index.namespaces.clone()];
    let mut scope_ids: HashMap<String, usize> = HashMap::new();
    // What wrapping slices may add to the text parsed, whatever namespaces they declare.
    let mut wrap_left = text.len().saturating_mul(4).max(MIN_WRAP_BYTES);
    let prefix = "<m".len() + index.namespaces.len() + ">".len();
    crate::progress::begin(index.members.len() * 2);
    for (m, &(start, end)) in index.members.iter().enumerate() {
        crate::progress::step(1)?;
        let Some(member) = text.get(start..end) else {
            notes.unreadable_members += 1;
            continue;
        };
        // Each member is parsed in a copy of the root's declarations: paid before it is written, so
        // a root declaring thousands can't make the copies outgrow the file.
        let extra = index.namespaces.len() + "<m></m>".len();
        if extra > wrap_left {
            notes.past_limits += 1;
            continue;
        }
        wrap_left -= extra;
        let mut wrapped = String::new();
        wrap(member, &mut wrapped);
        let Ok(doc) = crate::xml::parse_within(&wrapped, options(), &mut work_left) else {
            notes.unreadable_members += 1;
            continue;
        };
        notes.foreign += foreign_in(doc.root_element());
        for (fi, f) in features_of(doc.root_element()).into_iter().enumerate() {
            let id = identifier(f).unwrap_or_else(|| format!("#{m}.{fi}"));
            let mut slices = Vec::new();
            for sl in slices_of(f) {
                let r = sl.range();
                let Some(range) = r.start.checked_sub(prefix).zip(r.end.checked_sub(prefix)).map(|(a, b)| (start + a, start + b)) else {
                    notes.unreadable_members += 1;
                    continue;
                };
                let Some(scope) = scope_of(sl, text, start, prefix, &mut scopes, &mut scope_ids, &mut wrap_left) else {
                    notes.past_limits += 1;
                    continue;
                };
                slices.extend(slice_of(sl, range, scope, &mut trouble));
            }
            // A feature is its kind and identifier: two kinds sharing one aren't put together.
            let key = format!("{}\u{1}{id}", local(&f));
            let entry = features.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                Feature {
                    id: id.clone(),
                    kind: local(&f).to_string(),
                    gml_id: f.attribute((GML, "id")).map(|g| g.trim().to_string()),
                    slices: Vec::new(),
                }
            });
            entry.slices.extend(slices);
        }
    }
    notes.unknown_interpretation = trouble.unknown;
    notes.bad_dates = trouble.dates;
    notes.empty_dates = trouble.empty;
    notes.bad_numbers = trouble.numbers;
    // Second pass: each feature in force at the date, from its slices.
    // The nested paths of every row, each kept once.
    let paths: SharedPaths =
        std::rc::Rc::new(std::cell::RefCell::new(Paths { limit: text.len().saturating_mul(2).max(MIN_ROW_BYTES), ..Paths::default() }));
    let mut layers =
        Layers { names_only: opts.names_only, bytes_left: text.len().saturating_mul(4).max(MIN_ROW_BYTES), ..Layers::default() };
    let mut shapes = Shapes {
        names_only: opts.names_only,
        budget: text.len().max(MIN_ADDED_POINTS),
        scan_left: text.len().saturating_mul(8).max(MIN_ADDED_POINTS * 8),
        ..Shapes::default()
    };
    // Rows of features with nothing to draw, by kind, kept until it is known whether the kind has
    // a layer to hold them.
    let mut shapeless: Vec<(String, Row, usize, bool, String)> = Vec::new();
    let mut held_left = text.len().saturating_mul(2).max(MIN_ROW_BYTES);
    // GeoBorders first: airspace borders follow them by reference; route segments last: one with
    // no curve is drawn between the points it names.
    let rank = |id: &String| match features.get(id).map(|f| f.kind.as_str()) {
        Some("GeoBorder") => 0,
        Some("RouteSegment") => 2,
        _ => 1,
    };
    // The layers still come in the order their kinds are first met in the file.
    let mut kind_rank: HashMap<String, usize> = HashMap::new();
    for id in &order {
        if let Some(f) = features.get(id) {
            let next = kind_rank.len();
            kind_rank.entry(f.kind.clone()).or_insert(next);
        }
    }
    order.sort_by_key(rank);
    crate::progress::begin(index.members.len() + order.len());
    crate::progress::step(index.members.len())?;
    for key in &order {
        crate::progress::step(1)?;
        let Some(feature) = features.get(key) else { continue };
        layers.feature.clone_from(key);
        let id = &feature.id;
        if feature.slices.is_empty() {
            notes.no_slices += 1;
            continue;
        }
        let (base, changes, snapshot, temporary) = match state_at(&feature.slices, at) {
            State::Use { base, changes, snapshot, temporary } => (base, changes, snapshot, temporary),
            State::NotInForce => {
                notes.not_in_force += 1;
                continue;
            }
            State::OutsideLifetime => {
                notes.outside_lifetime += 1;
                continue;
            }
            State::OnlyChanges => {
                notes.no_state += 1;
                continue;
            }
            State::Canceled => {
                notes.canceled += 1;
                continue;
            }
        };
        // Its slices in force, each parsed on its own with the namespaces in scope where it is
        // written: the text parsed again is the slices', not their members'.
        let wanted: Vec<&Slice> = std::iter::once(&base).chain(changes.iter()).collect();
        let mut texts: Vec<String> = Vec::with_capacity(wanted.len());
        for sl in &wanted {
            let decls = scopes.get(sl.scope).map_or("", String::as_str);
            let body = text.get(sl.range.0..sl.range.1).unwrap_or_default();
            let extra = decls.len() + "<m></m>".len();
            if extra > wrap_left {
                texts.push(String::new());
                continue;
            }
            wrap_left -= extra;
            texts.push(format!("<m{decls}>{body}</m>"));
        }
        let docs: Vec<Option<Document>> =
            texts.iter().map(|t| if t.is_empty() { None } else { crate::xml::parse_within(t, options(), &mut work_left).ok() }).collect();
        let node = |i: usize| docs.get(i).and_then(Option::as_ref).and_then(|d| elements(d.root_element()).next());
        let Some(base_node) = node(0) else {
            notes.past_limits += 1;
            continue;
        };
        // The state's properties, each change's replacing all values of the properties it holds:
        // the last change holding a name gives that name's values (worked out in one pass, however
        // many changes there are).
        // A change in force that can't be read leaves the feature out (counted): its state
        // without the change isn't its state then.
        let mut layers_of: Vec<Vec<Node>> = vec![own(base_node)];
        let mut incomplete = false;
        for i in 1..wanted.len() {
            match node(i) {
                Some(n) => layers_of.push(own(n)),
                None => incomplete = true,
            }
        }
        if incomplete {
            notes.incomplete += 1;
            continue;
        }
        // Extensions are replaced within their own namespace (each its own key).
        let mut last: HashMap<String, usize> = HashMap::new();
        for (i, ps) in layers_of.iter().enumerate() {
            for p in ps {
                last.insert(property_key(*p), i);
            }
        }
        let last = &last;
        let props: Vec<Node> = layers_of
            .iter()
            .enumerate()
            .flat_map(|(i, ps)| ps.iter().copied().filter(move |p| last.get(&property_key(*p)) == Some(&i)))
            .collect();
        // A row being read keeps no more than the layers still may.
        let mut row = Row { limit: Some(layers.bytes_left), interned: std::rc::Rc::clone(&paths), ..Row::default() };
        row.set("identifier", id.trim_start_matches("urn:uuid:").to_string());
        let valid = |t: Option<f64>| t.map(|t| crate::time::format(t, "YYYY-MM-DDTHH:mm:ss") + "Z");
        match valid(base.begin) {
            Some(v) => row.set("validFrom", v),
            None => row.null("validFrom"),
        }
        match valid(base.end).filter(|_| !snapshot) {
            Some(v) => row.set("validTo", v),
            None => row.null("validTo"),
        }
        let kind = feature.kind.as_str();
        // A GeoBorder's line, at the date, for the borders that follow it by reference: by its
        // identifier and its curve's id.
        if kind == "GeoBorder"
            && let Some(g) = props.iter().filter(|p| local(p) == "border").find_map(|p| shape_in(*p))
            && let Some(Geometry::Line(parts)) = shapes.quietly(g)
        {
            let line: Vec<Coord> = parts.concat();
            if let Some(gid) = g.attribute((GML, "id")) {
                shapes.borders.insert(format!("#{}", gid.trim()), line.clone());
            }
            if let Some(gid) = &feature.gml_id {
                shapes.borders.insert(format!("#{gid}"), line.clone());
            }
            shapes.borders.insert(border_key(id), line);
        }
        // Where a point feature is, for route segments that name it: by its identifier and its
        // own gml:id (a local link's or an XPath's).
        if let Some(g) = props.iter().find_map(|p| shape_in(*p).filter(|g| matches!(local(g), "Point" | "ElevatedPoint")))
            && let Some(Geometry::Point(c)) = shapes.quietly(g)
        {
            if let Some(gid) = &feature.gml_id {
                shapes.points.insert(gid.clone(), c);
            }
            shapes.points.insert(id.trim_start_matches("urn:uuid:").to_string(), c);
        }
        let mut fnotes = ShapeNotes::default();
        let placed = match kind {
            "Airspace" => airspace(&props, &mut row, &mut shapes, &mut fnotes, &mut layers, kind, temporary, snapshot),
            "VerticalStructure" => vertical_structure(&props, &mut row, &mut shapes, &mut layers, kind, temporary, snapshot),
            _ => feature_rows(&props, &mut row, &mut shapes, &mut layers, kind, temporary, snapshot),
        };
        if !placed {
            // Held until it is known whether its kind has a layer, within an allowance of their
            // own: rows held and then let go can't crowd out rows that are drawn.
            let w = row.weight() + layers.feature.len() + kind.len();
            if w > held_left {
                layers.rows_over += 1;
            } else {
                held_left -= w;
                shapeless.push((kind.to_string(), row, temporary, snapshot, layers.feature.clone()));
            }
        }
    }
    // Features with nothing to draw: in their kind's layer when it has one, as a table for the
    // kinds that are tables, else counted.
    for (kind, row, temporary, snapshot, feature) in shapeless {
        layers.feature = feature;
        let change = (temporary > 0, snapshot);
        if layers.has_kind(&kind) {
            let label = layers.list.iter().find(|l| l.kind == kind).map(|l| l.label.clone()).unwrap_or_default();
            let i = layers.push(&kind, &label, None, row, None, change);
            if let Some(l) = layers.at(i) {
                l.shapeless += 1;
            }
        } else if TABLE_KINDS.contains(&kind.as_str()) {
            layers.push(&kind, "", None, row, None, change);
        } else {
            notes.nothing_to_draw += 1;
            notes.nothing_names.add(&kind);
        }
    }
    notes.other_system = shapes.other_system.take().map(|s| (s, shapes.other_count));
    notes.as_wgs84 = shapes.as_wgs84.take().map(|s| (s, shapes.as_wgs84_count));
    notes.assumed_system = shapes.assumed_system;
    let paths = paths.try_borrow_mut().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default();
    layers.list.sort_by_key(|l| kind_rank.get(&l.kind).copied().unwrap_or(usize::MAX));
    let mut dense_left = text.len().saturating_mul(8).max(MIN_ROW_BYTES * 4);
    finish(&stem, at, layers, notes, &index, shapes.budget == 0, &paths, &mut dense_left)
}

/// A route segment's line from the points its `start` and `end` name (their `pointChoice_…`
/// references): a geodesic between them, as AIXM's default path, when both are in the file.
fn between(props: &[Node], shapes: &mut Shapes) -> Option<(Option<Vec<Coord>>, ShapeNotes)> {
    // Both ends found first: a segment drawn neither way reads its ends' shapes once, as records.
    let (a, b) = (segment_end(props, "start", shapes)?, segment_end(props, "end", shapes)?);
    let quiet = |end: &End, shapes: &mut Shapes| match end {
        End::Named(c) => Some(*c),
        End::Written(g) => match shapes.quietly(*g) {
            Some(Geometry::Point(c)) => Some(c),
            _ => None,
        },
    };
    let (ca, cb) = (quiet(&a, shapes)?, quiet(&b, shapes)?);
    let mut notes = ShapeNotes { between: true, ..ShapeNotes::default() };
    // Drawn from them: what the ends written in place find is said with the segment.
    for end in [&a, &b] {
        if let End::Written(g) = end {
            let _ = shapes.shape(*g, &mut notes);
        }
    }
    // The path its pathType says: a geodesic (GDS, or a great circle, GRC, which the ellipsoid's
    // geodesic is) by default, a rhumb line (RHL); another isn't drawn, and that is said.
    let path = props.iter().find(|p| local(p) == "pathType").map(|p| text_of(p)).unwrap_or_default();
    let line = match path.as_str() {
        "" | "GDS" | "GRC" => Some(shapes.geodesic(&[ca, cb], &mut notes)),
        "RHL" => Some(shapes.rhumb(ca, cb, &mut notes)),
        _ => {
            notes.between = false;
            notes.path_unread = true;
            None
        }
    };
    Some((line, notes))
}

/// A route segment's `start` or `end`: a point it names, or one written in place.
enum End<'a, 'i> {
    Named(Coord),
    Written(Node<'a, 'i>),
}

/// Where a route segment's `start` or `end` is: the point it names (when that is in the file), or
/// one written in place (`pointChoice_position`), read as any shape is, with what its reading finds.
fn segment_end<'a, 'i>(props: &[Node<'a, 'i>], name: &str, shapes: &Shapes) -> Option<End<'a, 'i>> {
    let p = props.iter().find(|p| local(p) == name)?;
    let choice = p.descendants().find(|d| d.is_element() && local(d).starts_with("pointChoice_"))?;
    match choice.attribute((XLINK, "href")) {
        Some(h) => shapes.points.get(&reference(h)).copied().map(End::Named),
        None => shape_in(choice).filter(|g| matches!(local(g), "Point" | "ElevatedPoint")).map(End::Written),
    }
}

/// The family a shape element draws as, by its name (what a layer of its shapes is labeled).
fn element_family(name: &str) -> &'static str {
    match name {
        "Point" | "ElevatedPoint" => "points",
        "Curve" | "ElevatedCurve" | "LineString" => "lines",
        _ => "polygons",
    }
}

/// The shapes nested in a record (not those inside another shape, such as a surface's curves).
fn nested_shapes<'a, 'i>(p: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    p.descendants()
        .skip(1)
        .filter(|d| d.is_element() && (is_aixm(d) || is_gml(d)) && SHAPES.contains(&local(d)))
        .filter(|g| !g.ancestors().skip(1).take_while(|a| *a != p).any(|a| SHAPES.contains(&local(&a))))
        .collect()
}

/// Adds a feature's rows: a row in a layer for each property holding a shape; or, when none does,
/// a row for each record holding shapes (a light system's lights, a marking's elements), with the
/// feature's other values and the record's own (`element_style`), its shapes its geometry. False
/// when it has nothing to draw.
fn feature_rows(
    props: &[Node],
    row: &mut Row,
    shapes: &mut Shapes,
    layers: &mut Layers,
    kind: &str,
    temporary: usize,
    snapshot: bool,
) -> bool {
    let mut shaped: Vec<(String, Node)> = Vec::new();
    let mut parts: Vec<Node> = Vec::new();
    let mut others: Vec<Node> = Vec::new();
    for p in props {
        match shape_in(*p) {
            Some(g) if !is_nil(p) => shaped.push((local(p).to_string(), g)),
            _ if !is_nil(p) && p.attribute((XLINK, "href")).is_none() && !nested_shapes(*p).is_empty() => parts.push(*p),
            _ => others.push(*p),
        }
    }
    // A record with no shape of a property whose other records hold one (an unlocated light
    // beside located ones) is a row of its own, not values on every located one's row.
    let record_names: HashSet<&str> = if shaped.is_empty() { parts.iter().map(|p| local(p)).collect() } else { HashSet::new() };
    let mut unlocated: Vec<Node> = Vec::new();
    for p in others {
        if record_names.contains(local(&p)) && !is_nil(&p) && elements(p).next().is_some() {
            unlocated.push(p);
        } else {
            row.property(p, "");
        }
    }
    let change = (temporary > 0, snapshot);
    if !shaped.is_empty() {
        // A feature drawn from its own shapes: its records' values are kept as columns, and the
        // shapes in them (not drawn) are counted.
        let mut in_records = 0;
        for p in &parts {
            in_records += nested_shapes(*p).len();
            row.property(*p, "");
        }
        let mut first = None;
        for (name, g) in shaped {
            let mut notes = ShapeNotes::default();
            let geom = shapes.shape(g, &mut notes);
            let mut r = row.clone();
            r.shape_values(g, &name);
            // A kind's shapes under different properties are different layers (an airport's
            // reference point and its boundary); a kind with one is one layer, titled by its kind.
            // A shape whose positions are all empty (`<gml:pos></gml:pos>`, as the FAA writes a
            // position it doesn't have) gives none: said as no geometry, not as unreadable.
            let empty =
                !g.descendants().any(|d| d.is_element() && is_gml(&d) && matches!(local(&d), "pos" | "posList") && !text_of(&d).is_empty());
            let empty = empty && geom.is_none();
            let bad = geom.is_none() && !empty;
            let i = layers.push(kind, &name, geom, r, Some(&notes), change);
            first = first.or(i);
            if let Some(l) = layers.at(i) {
                l.unreadable += usize::from(bad);
                l.shapeless += usize::from(empty);
            }
        }
        if let Some(l) = layers.at(first) {
            l.shapes_in_records += in_records;
        }
        return true;
    }
    // A route segment with no curve: a geodesic between the points its start and end name (or
    // hold), its start and end kept as records.
    if kind == "RouteSegment"
        && let Some((line, notes)) = between(props, shapes)
    {
        for p in parts.iter().chain(&unlocated) {
            row.property(*p, "");
        }
        let line = line.map(|l| vec![l]);
        layers.push(kind, "curveExtent", line.map(Geometry::Line), row.clone(), Some(&notes), change);
        return true;
    }
    if parts.is_empty() {
        return false;
    }
    // Rows copied for the records stay within what the file's size allows: checked before each
    // copy, as a vertical structure's parts are.
    let mut room = layers.bytes_left;
    let first_label = parts.iter().flat_map(|p| nested_shapes(*p)).next().map_or("points", |g| element_family(local(&g)));
    for p in unlocated {
        if row.weight() > room {
            layers.rows_over += 1;
            continue;
        }
        let mut r = row.clone();
        let mut path = vec![(local(&p).to_string(), 1)];
        r.record(p, &mut path, false);
        room = room.saturating_sub(r.weight());
        let i = layers.push(kind, first_label, None, r, None, (false, false));
        if let Some(l) = layers.at(i) {
            l.unlocated += 1;
        }
    }
    let count = parts.len();
    for (k, p) in parts.into_iter().enumerate() {
        if row.weight() > room {
            layers.rows_over += count - k;
            break;
        }
        let mut r = row.clone();
        let mut path = vec![(local(&p).to_string(), 1)];
        r.record(p, &mut path, false);
        room = room.saturating_sub(r.weight());
        // The record's shapes by family, each family a row of its own (most records have one).
        let mut by_family: BTreeMap<&'static str, (Vec<Geometry>, ShapeNotes)> = BTreeMap::new();
        for g in nested_shapes(p) {
            let mut notes = ShapeNotes::default();
            let geom = shapes.shape(g, &mut notes);
            let (gs, n) = by_family.entry(element_family(local(&g))).or_default();
            n.absorb(if geom.is_some() { notes } else { notes.losses_only() });
            gs.extend(geom);
        }
        // What the feature was is said once for it, with its first row.
        let change = if k == 0 { change } else { (false, false) };
        for (label, (gs, notes)) in by_family {
            let bad = gs.is_empty();
            let i = layers.push(kind, label, merged(gs), r.clone(), Some(&notes), change);
            if let Some(l) = layers.at(i) {
                l.unreadable += usize::from(bad);
            }
        }
    }
    true
}

/// An airspace: its volumes' operations decide what is drawn. Its limits are its BASE volume's.
#[expect(clippy::too_many_arguments, reason = "an airspace reads into the row, the shapes, the layers and its notes at once")]
fn airspace(
    props: &[Node],
    row: &mut Row,
    shapes: &mut Shapes,
    notes: &mut ShapeNotes,
    layers: &mut Layers,
    kind: &str,
    temporary: usize,
    snapshot: bool,
) -> bool {
    let mut components: Vec<(u64, String, Option<Node>)> = Vec::new();
    for p in props {
        if local(p) == "geometryComponent" {
            if is_nil(p) {
                row.property(*p, "");
                continue;
            }
            // A volume given by reference to another feature's isn't followed: the airspace isn't
            // drawn from the rest as if it were whole.
            let Some(c) = elements(*p).find(|c| local(c) == "AirspaceGeometryComponent") else {
                if p.attribute((XLINK, "href")).is_some() {
                    components.push((u64::MAX, "REFERENCE".to_string(), None));
                }
                row.property(*p, "");
                continue;
            };
            // Every component's values as its record (its operation, its volume's limits, the
            // airspaces it is built from), besides the BASE volume's limits below.
            row.property(*p, "");
            let op = child(c, "operation").map(|n| text_of(&n)).unwrap_or_default();
            let seq = child(c, "operationSequence").and_then(|n| text_of(&n).parse().ok()).unwrap_or(u64::MAX);
            let volume = child(c, "theAirspaceVolume").and_then(|v| child(v, "AirspaceVolume"));
            components.push((seq, op, volume));
        } else {
            row.property(*p, "");
        }
    }
    if components.is_empty() {
        return false;
    }
    components.sort_by_key(|(s, ..)| *s);
    // One volume with no operation written is the airspace's own (the element is optional).
    if let [(_, op, _)] = components.as_mut_slice()
        && op.is_empty()
    {
        *op = "BASE".to_string();
    }
    // The BASE volume's limits and width, as written.
    if let Some((_, _, Some(v))) = components.iter().find(|(_, op, _)| op == "BASE").or(components.first()) {
        for p in elements(*v).filter(|p| !matches!(local(p), "horizontalProjection" | "centreline" | "contributorAirspace")) {
            let prefix = if row.named.contains(local(&p)) { "volume_" } else { "" };
            row.property(p, prefix);
        }
    }
    let drawable = components.iter().all(|(_, op, v)| matches!(op.as_str(), "BASE" | "UNION") && projection(*v).is_some());
    let mut geom = None;
    let mut unreadable = false;
    if drawable {
        let mut parts = Vec::new();
        for (_, _, v) in &components {
            match projection(*v).and_then(|g| shapes.shape(g, notes)) {
                Some(g) => parts.push(g),
                None => unreadable = true,
            }
        }
        // Parts of different kinds (a line where a surface is meant) aren't one shape.
        if parts.first().is_some_and(|f| parts.iter().any(|g| family(g) != family(f))) {
            unreadable = true;
        }
        if !unreadable {
            geom = merged(parts);
        }
    }
    let i = layers.push(kind, "", geom, row.clone(), Some(notes), (temporary > 0, snapshot));
    if let Some(l) = layers.at(i) {
        if !drawable {
            l.derived += 1;
            for (_, op, v) in &components {
                if op == "REFERENCE" {
                    l.derived_names.add("a volume given by reference");
                } else if v.is_some_and(|v| child(v, "contributorAirspace").is_some()) {
                    l.derived_names.add("contributorAirspace");
                } else if op.is_empty() {
                    l.derived_names.add("a volume with no operation");
                } else if projection(*v).is_none() {
                    l.derived_names.add("a volume without horizontalProjection");
                } else if !matches!(op.as_str(), "BASE" | "UNION") {
                    l.derived_names.add(op);
                }
            }
        } else if unreadable {
            l.unreadable += 1;
        } else if components.len() > 1 {
            l.unions += 1;
        }
    }
    true
}

/// A vertical structure: a row per part, the structure's values with the part's (`part_…`).
fn vertical_structure(
    props: &[Node],
    row: &mut Row,
    shapes: &mut Shapes,
    layers: &mut Layers,
    kind: &str,
    temporary: usize,
    snapshot: bool,
) -> bool {
    let mut parts = Vec::new();
    for p in props {
        if local(p) == "part" && !is_nil(p) {
            match elements(*p).find(|c| local(c) == "VerticalStructurePart") {
                Some(vp) => parts.push(vp),
                // A part given by reference: its link kept.
                None => row.property(*p, ""),
            }
        } else {
            row.property(*p, "");
        }
    }
    if parts.is_empty() {
        // Kept with its kind (or counted) as any other feature with nothing to draw.
        return false;
    }
    let mut rows = Vec::new();
    // Rows copied for the parts stay within what the file's size allows: checked before each copy,
    // so a structure of many parts and many values can't outgrow it.
    let mut room = layers.bytes_left;
    let count = parts.len();
    for (k, vp) in parts.into_iter().enumerate() {
        if row.weight() > room {
            layers.rows_over += count - k;
            break;
        }
        let mut r = row.clone();
        let mut notes = ShapeNotes::default();
        let mut geom = None;
        let mut bad = false;
        for p in elements(vp) {
            match shape_in(p) {
                Some(g) if !is_nil(&p) => {
                    geom = shapes.shape(g, &mut notes);
                    bad = geom.is_none();
                    r.shape_values(g, &format!("part_{}", local(&p)));
                }
                _ => r.property(p, "part_"),
            }
        }
        room = room.saturating_sub(r.weight());
        rows.push((geom, r, notes, bad));
    }
    // A part with no shape goes with the structure's first family of parts.
    let first = rows.iter().find_map(|(g, ..)| g.as_ref().map(family)).unwrap_or("points");
    for (k, (geom, r, notes, bad)) in rows.into_iter().enumerate() {
        let label = geom.as_ref().map_or(first, family);
        let change = if k == 0 { (temporary > 0, snapshot) } else { (false, false) };
        let shapeless = geom.is_none() && !bad;
        let i = layers.push(kind, label, geom, r, Some(&notes), change);
        if let Some(l) = layers.at(i) {
            l.unreadable += usize::from(bad);
            l.shapeless += usize::from(shapeless);
        }
    }
    true
}

/// The layers as datasets, with what the read says.
/// What a row of a table holds besides its values (its shape's slot, its bookkeeping), in bytes.
const DENSE_ROW_OVERHEAD: usize = 64;

fn finish(
    stem: &str,
    at: f64,
    layers: Layers,
    notes: FileNotes,
    index: &Index,
    budget_spent: bool,
    paths: &Paths,
    dense_left: &mut usize,
) -> Result<Vec<(Dataset, ReadReport)>> {
    let mut file = ReadReport::default();
    let say = |r: &mut ReadReport, n: usize, one: &str, many: &str, rest: &str| {
        if n > 0 {
            let what = if n == 1 { one } else { many };
            r.messages.push(format!("{n} {what} {rest}"));
        }
    };
    let when = date_text(at);
    say(&mut file, notes.not_in_force, "feature", "features", &format!("not in force on {when} (valid only at other times) left out"));
    say(
        &mut file,
        notes.outside_lifetime,
        "feature",
        "features",
        &format!("withdrawn by {when} or created after it (featureLifetime) left out"),
    );
    say(
        &mut file,
        notes.no_state,
        "feature",
        "features",
        "given only as changes (PERMDELTA or TEMPDELTA), with no BASELINE or SNAPSHOT to apply them to, left out",
    );
    say(&mut file, notes.canceled, "feature", "features", "whose time slices were all canceled left out");
    say(&mut file, notes.no_slices, "feature", "features", "with no time slice RustGIS reads left out");
    say(
        &mut file,
        notes.unknown_interpretation,
        "time slice",
        "time slices",
        "with no interpretation RustGIS knows (BASELINE, PERMDELTA, TEMPDELTA, SNAPSHOT) not read",
    );
    say(
        &mut file,
        notes.empty_dates,
        "time position",
        "time positions",
        "left empty (not said to be unknown) read as open: no start or no end",
    );
    say(&mut file, notes.bad_numbers, "time slice", "time slices", "with a sequence or correction number that isn't a number read as 0");
    say(
        &mut file,
        notes.incomplete,
        "feature",
        "features",
        "left out: a change to it in force then couldn't be read within the limits the file's size sets",
    );
    say(&mut file, notes.bad_dates, "time slice", "time slices", "with a validTime or featureLifetime that isn't a date not read");
    say(
        &mut file,
        notes.unreadable_members,
        "member",
        "members",
        "of the message couldn't be read (not well-formed XML, or past the reading limits)",
    );
    say(
        &mut file,
        notes.past_limits,
        "member or time slice",
        "members or time slices",
        "not read: reading them would pass the limits the file's size sets",
    );
    say(&mut file, layers.rows_over, "row", "rows", "past the memory the file's size allows left out");
    say(&mut file, index.links, "member", "members", "of the message only point elsewhere (xlink:href) and weren't followed");
    let nothing = format!("of kinds with nothing to draw left out ({})", notes.nothing_names.said());
    say(&mut file, notes.nothing_to_draw, "feature", "features", &nothing);
    say(&mut file, notes.foreign, "element", "elements", "of members that aren't AIXM features (another schema's) not read");
    say(&mut file, index.empty, "member", "members", "of the message hold nothing");
    if let Some(at) = index.broken_at {
        file.messages.push(format!("the file stops being well-formed XML at byte {at}: features after it weren't read"));
    }
    say(&mut file, layers.over.len(), "feature", "features", &format!("of kinds past the first {MAX_LAYERS} left out"));
    if let Some((s, n)) = &notes.other_system {
        say(&mut file, *n, "geometry", "geometries", &format!("in a system other than longitude/latitude ('{s}', or another) not read"));
    }
    if let Some((s, n)) = &notes.as_wgs84 {
        say(
            &mut file,
            *n,
            "geometry",
            "geometries",
            &format!("in a geographic system other than WGS 84 ('{s}', or another) read as WGS 84 longitude/latitude"),
        );
    }
    say(
        &mut file,
        notes.assumed_system,
        "geometry",
        "geometries",
        "without an srsName read as urn:ogc:def:crs:EPSG::4326 (latitude first)",
    );
    if budget_spent {
        file.messages.push("arcs and geodesics added more points than the file's size allows: the rest were drawn with fewer points");
    }
    let mut kinds_with_labels: HashMap<String, usize> = HashMap::new();
    for l in &layers.list {
        *kinds_with_labels.entry(l.kind.clone()).or_default() += 1;
    }
    let mut out = Vec::new();
    for layer in layers.list {
        let title = if layer.label.is_empty() || kinds_with_labels.get(&layer.kind).copied().unwrap_or(0) < 2 {
            format!("{stem} {}", layer.kind)
        } else {
            format!("{stem} {} {}", layer.kind, layer.label)
        };
        // Nested values as columns, named by their paths across the whole layer; each path's
        // column worked out once.
        let mut layer = layer;
        let repeating = path_columns(&layer.rows, paths);
        let mut column_of: HashMap<usize, Option<usize>> = HashMap::new();
        let mut rows: Vec<(Option<Geometry>, Vec<(usize, String)>)> = Vec::with_capacity(layer.rows.len());
        for (g, mut attrs, nested) in std::mem::take(&mut layer.rows) {
            for (id, value) in nested {
                let k = match column_of.get(&id) {
                    Some(k) => *k,
                    None => {
                        let k = paths.list.get(id).and_then(|p| layer.column(&path_column(p, &repeating), false));
                        column_of.insert(id, k);
                        k
                    }
                };
                // A value past the cap is counted, each one; a missing one isn't a loss.
                if k.is_none() && value.is_some() {
                    layer.capped += 1;
                }
                if let (Some(k), Some(v)) = (k, value) {
                    attrs.push((k, v));
                }
            }
            // A column given more than once keeps every value, in order.
            attrs.sort_by_key(|(k, _)| *k);
            let mut merged: Vec<(usize, String)> = Vec::with_capacity(attrs.len());
            for (k, v) in attrs {
                match merged.last_mut() {
                    Some((last, old)) if *last == k => {
                        if old.is_empty() {
                            *old = v;
                        } else if !v.is_empty() {
                            old.push_str("; ");
                            old.push_str(&v);
                        }
                    }
                    _ => merged.push((k, v)),
                }
            }
            rows.push((g, merged));
        }
        // Each column typed from its values, looked at where they are.
        let mut values: Vec<Vec<&str>> = vec![Vec::new(); layer.columns.len()];
        for (_, attrs) in &rows {
            for (k, v) in attrs {
                if let Some(col) = values.get_mut(*k)
                    && !v.is_empty()
                {
                    col.push(v);
                }
            }
        }
        let fields: Vec<Field> =
            layer.columns.iter().zip(&values).map(|(c, vs)| Field::new(c.clone(), crate::io::text_type(vs.iter().copied()))).collect();
        drop(values);
        let mut ds = Dataset::new(title, GeometryType::None, fields);
        // The table holds a value for every column of every row: the rows that fit the memory the
        // file's size allows are kept, and the rest counted.
        let per_row = (ds.fields.len() * std::mem::size_of::<Value>()).max(1) + DENSE_ROW_OVERHEAD;
        let fit = *dense_left / per_row;
        if rows.len() > fit {
            layer.dense_over += rows.len() - fit;
            rows.truncate(fit);
        }
        *dense_left -= rows.len() * per_row;
        for (g, attrs) in rows {
            let mut row = vec![Value::Null; ds.fields.len()];
            for (k, v) in attrs {
                if let (Some(slot), Some(f)) = (row.get_mut(k), ds.fields.get(k)) {
                    *slot = Value::parse_file(&v, f.ty);
                }
            }
            ds.push_next(g, row);
        }
        ds.geometry_type = crate::io::dominant_type(&ds);
        let mut rep = if out.is_empty() { std::mem::take(&mut file) } else { ReadReport::default() };
        let name = ds.name.clone();
        // Each notice as one feature and as several say it.
        let mut say = |n: usize, one: &str, many: &str| {
            if n > 0 {
                rep.messages.push(format!("{name}: {n} {}", if n == 1 { one } else { many }));
            }
        };
        say(
            layer.unreadable,
            "feature has geometry that couldn't be read and is kept without a shape",
            "features have geometry that couldn't be read and are kept without a shape",
        );
        say(
            layer.shapeless,
            "feature has no geometry and is kept without a shape",
            "features have no geometry and are kept without a shape",
        );
        let derived = layer.derived_names.said();
        say(
            layer.derived,
            &format!("airspace built from other airspace or by subtraction or intersection is kept without a shape, not drawn ({derived})"),
            &format!(
                "airspaces built from other airspace or by subtraction or intersection are kept without a shape, not drawn ({derived})"
            ),
        );
        say(
            layer.unions,
            "airspace is drawn as the parts of its BASE and UNION volumes, not merged into one outline; its limits are its BASE volume's",
            "airspaces are drawn as the parts of their BASE and UNION volumes, not merged into one outline; their limits are their BASE volume's",
        );
        say(
            layer.shapes_in_records,
            "shape in the records of features drawn from their own shape isn't drawn (its record's values are kept)",
            "shapes in the records of features drawn from their own shapes aren't drawn (their records' values are kept)",
        );
        say(
            layer.arcs,
            "feature has arcs or circles, drawn as points along them within a meter",
            "features have arcs or circles, drawn as points along them within a meter",
        );
        say(
            layer.coarse,
            "feature has arcs or geodesics drawn with fewer points than that needs (a curve's most points, or the file's point allowance, ran out)",
            "features have arcs or geodesics drawn with fewer points than that needs (a curve's most points, or the file's point allowance, ran out)",
        );
        let segments = layer.unread_segments.said();
        say(
            layer.approximated,
            &format!("feature has curve segments of kinds AIXM doesn't use, drawn straight between their positions ({segments})"),
            &format!("features have curve segments of kinds AIXM doesn't use, drawn straight between their positions ({segments})"),
        );
        say(
            layer.rings_lost,
            "ring or surface patch was left out (with the patch it is a hole of): too few points, or unreadable",
            "rings or surface patches were left out (with the patches they are holes of): too few points, or unreadable",
        );
        say(
            layer.referenced,
            "border is given by reference to a curve that isn't in the file (or not in force then), so it can't be followed",
            "borders are given by reference to curves that aren't in the file (or not in force then), so they can't be followed",
        );
        say(
            layer.followed_whole,
            "border given by reference is drawn whole along the GeoBorder it names: the ring names no other points to cut it at",
            "borders given by reference are drawn whole along the GeoBorders they name: their rings name no other points to cut them at",
        );
        say(
            layer.border_allowance,
            "border given by reference isn't followed: following it would pass the work the file's size allows",
            "borders given by reference aren't followed: following them would pass the work the file's size allows",
        );
        say(
            layer.border_unusable,
            "border given by reference isn't followed: the GeoBorder it names has too few points to cut",
            "borders given by reference aren't followed: the GeoBorders they name have too few points to cut",
        );
        say(
            layer.short_arcs,
            "feature has an arc or circle not given the three positions it needs, drawn through the positions it has (a circle through its first three)",
            "features have an arc or circle not given the three positions it needs, drawn through the positions they have (circles through their first three)",
        );
        say(
            layer.straight_arcs,
            "feature has an arc or circle drawn straight: its points are in a line, or too far apart to be one",
            "features have an arc or circle drawn straight: their points are in a line, or too far apart to be one",
        );
        say(
            layer.empty_arcs,
            "feature has an arc or circle with no positions, left out: its line is joined straight from the positions either side",
            "features have an arc or circle with no positions, left out: their lines are joined straight from the positions either side",
        );
        say(
            layer.path_unread,
            "route segment with no curve isn't drawn: its pathType is one RustGIS doesn't draw",
            "route segments with no curve aren't drawn: their pathType is one RustGIS doesn't draw",
        );
        say(
            layer.dense_over,
            "row is left out: the table would pass the memory the file's size allows",
            "rows are left out: the table would pass the memory the file's size allows",
        );
        say(
            layer.unlocated,
            "record with no shape (beside ones of the same property that have one) is a row without a shape",
            "records with no shape (beside ones of the same property that have one) are rows without a shape",
        );
        say(
            layer.between,
            "route segment with no curve is drawn between the points its start and end name, as its pathType says (a geodesic, or a rhumb line)",
            "route segments with no curve are drawn between the points their start and end name, as their pathType says (geodesics, or rhumb lines)",
        );
        say(
            layer.followed,
            "border given by reference is drawn along the GeoBorder it names, between the points on it nearest those either side (AIXM leaves this cut to the reader)",
            "borders given by reference are drawn along the GeoBorders they name, between the points on them nearest those either side (AIXM leaves this cut to the reader)",
        );
        say(
            layer.far,
            &format!("border followed by reference starts or ends more than {FAR_FROM_BORDER_M} m from the GeoBorder"),
            &format!("borders followed by reference start or end more than {FAR_FROM_BORDER_M} m from the GeoBorder"),
        );
        say(
            layer.temporary,
            &format!("feature is shown with a temporary change (TEMPDELTA) in force on {when}"),
            &format!("features are shown with temporary changes (TEMPDELTA) in force on {when}"),
        );
        say(
            layer.snapshots,
            "feature is a SNAPSHOT: its state at its own instant, which may not be its state on the date asked",
            "features are SNAPSHOTs: their state at their own instant, which may not be their state on the date asked",
        );
        say(
            layer.over_bytes,
            "value wasn't kept: past the memory the file's size allows",
            "values weren't kept: past the memory the file's size allows",
        );
        say(
            layer.capped,
            &format!("value past the first {MAX_COLUMNS} columns, or nested deeper than {MAX_PATH} records, wasn't read"),
            &format!("values past the first {MAX_COLUMNS} columns, or nested deeper than {MAX_PATH} records, weren't read"),
        );
        if layer.zm > 0 {
            ds.zm_dropped = true;
            // First, as every reader's: a layer picked from the file says it, the others don't.
            rep.messages.insert(0, format!("{name}: {}", crate::zm_note(Some(layer.zm as u64))));
        }
        out.push((ds, rep));
    }
    if out.is_empty() {
        let mut why: Vec<String> = file.messages.iter().map(|m| m.text.clone()).collect();
        if why.is_empty() {
            why.push("no features in force".into());
        }
        return Err(DataError::Format(format!("nothing to add at {when}: {}", why.join("; "))));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
