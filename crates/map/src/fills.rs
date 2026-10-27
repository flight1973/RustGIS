//! A polygon layer's stacked fills (#882 piece 5): drawn bottom first inside each polygon in place
//! of its symbol's single fill, as QGIS's symbol layers stack. Each fills the polygon with its
//! colour, hatches it with parallel lines (QGIS's line pattern fill, and a simple fill's hatched
//! brush styles), or dots it with markers on a grid (QGIS's point pattern fill). Sizes are in
//! points, as a symbol's, so a pattern keeps its size on screen at every scale.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::symbology::Marker;

/// The most fills one polygon symbol stacks.
pub const MAX_FILLS: usize = 8;
/// How far apart a pattern's lines or markers may be (points).
pub const PATTERN_SPACING: std::ops::RangeInclusive<f32> = 1.0..=500.0;
/// The widest hatch line (points).
pub const MAX_HATCH_WIDTH: f32 = 50.0;
/// The largest pattern marker (points).
pub const MAX_DOT_SIZE: f32 = 200.0;
/// How far a pattern may be shifted each way (points).
pub const MAX_PATTERN_OFFSET: f32 = 500.0;

/// One fill of a polygon layer's stacked symbol (`Layer::fill_layers`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillLayer {
    /// Its colour (a pattern's lines' or markers'); `None` takes the feature's fill colour from
    /// the renderer, so the fills follow Unique Values and Graduated Colors classes.
    #[serde(default)]
    pub color: Option<Rgba>,
    #[serde(default)]
    pub pattern: FillPattern,
}

/// What a fill draws inside its polygon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FillPattern {
    /// The whole polygon in its colour.
    #[default]
    Solid,
    /// Parallel lines `spacing` points apart and `width` points wide, at `angle` degrees
    /// anticlockwise from east, shifted `offset` points across them.
    Hatch {
        #[serde(default)]
        angle: f32,
        #[serde(default = "hatch_spacing")]
        spacing: f32,
        #[serde(default = "hatch_width")]
        width: f32,
        #[serde(default)]
        offset: f32,
    },
    /// Markers `size` points across on a grid `spacing` points apart (across, down), every other
    /// row shifted `displacement[0]` across and every other column `displacement[1]` down (QGIS's
    /// displacement), the grid shifted `offset`; outlined in `outline`, `outline_width` wide.
    Dots {
        #[serde(default)]
        marker: Marker,
        #[serde(default = "dot_size")]
        size: f32,
        #[serde(default)]
        outline: Option<Rgba>,
        #[serde(default)]
        outline_width: f32,
        #[serde(default = "dot_spacing")]
        spacing: [f32; 2],
        #[serde(default)]
        displacement: [f32; 2],
        #[serde(default)]
        offset: [f32; 2],
    },
}

fn hatch_spacing() -> f32 {
    8.0
}

fn hatch_width() -> f32 {
    1.0
}

fn dot_size() -> f32 {
    3.0
}

fn dot_spacing() -> [f32; 2] {
    [8.0, 8.0]
}

/// `v` within `lo..=hi`; one of no number is `none`.
fn within(v: f32, lo: f32, hi: f32, none: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { none }
}

impl FillLayer {
    /// A fill in the feature's colour, hatched `angle` degrees `spacing` points apart.
    pub fn hatch(angle: f32, spacing: f32, width: f32) -> FillLayer {
        FillLayer { color: None, pattern: FillPattern::Hatch { angle, spacing, width, offset: 0.0 } }.clamped()
    }

    /// Kept as the command would leave it: sizes within their ranges, an angle within a turn,
    /// a displacement within a cell. A project file is hostile input, and every fill is drawn per
    /// feature per frame.
    pub fn clamped(self) -> FillLayer {
        let pattern = match self.pattern {
            FillPattern::Solid => FillPattern::Solid,
            FillPattern::Hatch { angle, spacing, width, offset } => FillPattern::Hatch {
                angle: if angle.is_finite() { angle.rem_euclid(360.0) } else { 0.0 },
                spacing: within(spacing, *PATTERN_SPACING.start(), *PATTERN_SPACING.end(), hatch_spacing()),
                width: within(width, 0.0, MAX_HATCH_WIDTH, hatch_width()),
                offset: within(offset, -MAX_PATTERN_OFFSET, MAX_PATTERN_OFFSET, 0.0),
            },
            FillPattern::Dots { marker, size, outline, outline_width, spacing, displacement, offset } => {
                let spacing = spacing.map(|s| within(s, *PATTERN_SPACING.start(), *PATTERN_SPACING.end(), dot_spacing()[0]));
                FillPattern::Dots {
                    marker,
                    size: within(size, 0.0, MAX_DOT_SIZE, dot_size()),
                    outline,
                    outline_width: within(outline_width, 0.0, MAX_HATCH_WIDTH, 0.0),
                    displacement: [0, 1].map(|i| {
                        let cell = spacing.get(i).copied().unwrap_or(1.0);
                        displacement.get(i).map_or(0.0, |d| if d.is_finite() { d.rem_euclid(cell) } else { 0.0 })
                    }),
                    offset: offset.map(|o| within(o, -MAX_PATTERN_OFFSET, MAX_PATTERN_OFFSET, 0.0)),
                    spacing,
                }
            }
        };
        FillLayer { pattern, ..self }
    }

    /// Whether it draws anything in a feature of fill `feature`: a colour that shows, and for a
    /// pattern, lines or markers of some size.
    pub fn draws(&self, feature: Rgba) -> bool {
        let shown = self.color.unwrap_or(feature).a() > 0;
        match self.pattern {
            FillPattern::Solid => shown,
            FillPattern::Hatch { width, .. } => shown && width > 0.0,
            FillPattern::Dots { size, outline, outline_width, .. } => {
                size > 0.0 && (shown || (outline.is_some_and(|o| o.a() > 0) && outline_width > 0.0))
            }
        }
    }
}

/// A layer's fills read leniently: at most [`MAX_FILLS`], each [`FillLayer::clamped`]; one that
/// isn't a fill is left out, and a value that isn't a list is none, so a damaged entry doesn't
/// make the project unreadable.
pub fn de_fills<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<FillLayer>, D::Error> {
    Ok(fills_of(serde_json::Value::deserialize(d)?))
}

fn fills_of(v: serde_json::Value) -> Vec<FillLayer> {
    let serde_json::Value::Array(list) = v else { return Vec::new() };
    list.into_iter().filter_map(|f| serde_json::from_value::<FillLayer>(f).ok()).take(MAX_FILLS).map(FillLayer::clamped).collect()
}

/// A renderer's per-class fills read as [`de_fills`] reads a layer's; a class left with none is
/// dropped, and a class that doesn't read is left out.
pub fn de_class_fills<'de, D, K>(d: D) -> Result<BTreeMap<K, Vec<FillLayer>>, D::Error>
where
    D: serde::Deserializer<'de>,
    K: serde::de::DeserializeOwned + Ord,
{
    let serde_json::Value::Object(m) = serde_json::Value::deserialize(d)? else { return Ok(BTreeMap::new()) };
    Ok(m.into_iter()
        .filter_map(|(k, v)| {
            let key: K = serde_json::from_value(serde_json::Value::String(k.clone())).or_else(|_| serde_json::from_str(&k)).ok()?;
            let fills = fills_of(v);
            (!fills.is_empty()).then_some((key, fills))
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Fills are kept; crafted ones are held within their ranges, and ones that aren't fills are
    /// left out without the rest failing to read.
    #[test]
    fn fills_read_leniently_and_within_their_ranges() {
        let hatch = FillLayer::hatch(45.0, 4.0, 0.5);
        let dots = FillLayer {
            color: Some(Rgba::rgb(0, 0, 255)),
            pattern: FillPattern::Dots {
                marker: Marker::Circle,
                size: 2.0,
                outline: None,
                outline_width: 0.0,
                spacing: [6.0, 6.0],
                displacement: [3.0, 0.0],
                offset: [1.0, -1.0],
            },
        };
        let v = serde_json::to_value([FillLayer::default(), hatch, dots]).unwrap();
        assert_eq!(
            v[1],
            serde_json::json!({"color": null, "pattern": {"kind": "hatch", "angle": 45.0, "spacing": 4.0, "width": 0.5, "offset": 0.0}})
        );
        assert_eq!(fills_of(v), vec![FillLayer::default(), hatch, dots]);
        // Damaged: not fills, too many, sizes of no number or far too large.
        let crafted = serde_json::json!([
            7,
            {"pattern": {"kind": "stripes"}},
            {"pattern": {"kind": "hatch", "angle": -90, "spacing": 1e30, "width": -1, "offset": -1e30}},
            {"pattern": {"kind": "dots", "size": 1e9, "spacing": [0, 1e9], "displacement": [99, -1], "offset": [1e9, 0]}}
        ]);
        let read = fills_of(crafted);
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].pattern, FillPattern::Hatch { angle: 270.0, spacing: 500.0, width: 0.0, offset: -MAX_PATTERN_OFFSET });
        let FillPattern::Dots { size, spacing, displacement, offset, .. } = read[1].pattern else { panic!("{read:?}") };
        assert_eq!((size, spacing, offset), (MAX_DOT_SIZE, [1.0, 500.0], [MAX_PATTERN_OFFSET, 0.0]));
        assert_eq!(displacement, [0.0, 499.0], "within a cell");
        assert_eq!(fills_of(serde_json::json!((0..20).map(|_| serde_json::json!({})).collect::<Vec<_>>())).len(), MAX_FILLS);
        assert!(fills_of(serde_json::json!("solid")).is_empty());
        // What draws: a colour that shows, and a pattern of some size.
        let feature = Rgba::rgb(200, 100, 0);
        assert!(FillLayer::default().draws(feature));
        assert!(!FillLayer::default().draws(Rgba::new(0, 0, 0, 0)));
        assert!(!FillLayer::hatch(0.0, 4.0, 0.0).draws(feature));
    }
}
