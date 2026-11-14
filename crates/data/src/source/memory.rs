//! Allocation accounting and bounded sampling for disposable view windows. Full reads are unchanged.
use crate::{Feature, Geometry, Value};
use std::mem::size_of;

/// Allocated vector storage owned by a geometry, excluding the enum in its containing array.
pub fn geometry_bytes(g: &Geometry) -> usize {
    let ring = |r: &Vec<crate::Coord>| r.capacity().saturating_mul(size_of::<crate::Coord>());
    let parts = |p: &Vec<Vec<crate::Coord>>| {
        p.iter().fold(p.capacity().saturating_mul(size_of::<Vec<crate::Coord>>()), |n, r| n.saturating_add(ring(r)))
    };
    match g {
        Geometry::Point(_) => 0,
        Geometry::MultiPoint(p) => ring(p),
        Geometry::Line(p) => parts(p),
        Geometry::Polygon(p) => {
            p.iter().fold(p.capacity().saturating_mul(size_of::<Vec<Vec<crate::Coord>>>()), |n, r| n.saturating_add(parts(r)))
        }
    }
}

/// Heap storage owned by a row, including unused capacity and attribute strings.
pub fn feature_bytes(f: &Feature) -> usize {
    let attrs = f.attrs.iter().fold(f.attrs.capacity().saturating_mul(size_of::<Value>()), |n, v| {
        n.saturating_add(match v {
            Value::Text(s) => s.capacity(),
            _ => 0,
        })
    });
    attrs.saturating_add(f.geometry.as_ref().map_or(0, geometry_bytes))
}

/// Actual allocated window storage; shared owners count the allocation once.
pub fn features_bytes(features: &Vec<Feature>) -> usize {
    features.iter().fold(features.capacity().saturating_mul(size_of::<Feature>()), |n, f| n.saturating_add(feature_bytes(f)))
}

/// Keep every other candidate, in file order. The source remains authoritative: this changes
/// only a disposable display sample, never coordinates, attributes, or full/edit/export reads.
pub(crate) fn halve(features: &mut Vec<Feature>) {
    let mut i = 0usize;
    features.retain(|_| {
        let keep = i.is_multiple_of(2);
        i += 1;
        keep
    });
    features.shrink_to_fit();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_accounting_includes_spare_capacity_nested_parts_and_text() {
        let mut ring = Vec::with_capacity(100);
        ring.push(crate::Coord::new(0.0, 0.0));
        let g = Geometry::Polygon(vec![vec![ring]]);
        let mut text = String::with_capacity(1024);
        text.push('x');
        let mut rows = Vec::with_capacity(8);
        rows.push(Feature { id: 1, geometry: Some(g), attrs: vec![Value::Text(text)], zm: None });
        let expected = rows.capacity() * size_of::<Feature>()
            + 100 * size_of::<crate::Coord>()
            + size_of::<Vec<Vec<crate::Coord>>>()
            + size_of::<Vec<crate::Coord>>()
            + size_of::<Value>()
            + 1024;
        assert_eq!(features_bytes(&rows), expected);
    }
    fn source() -> Box<dyn crate::source::FeatureSource> {
        let mut ds = crate::Dataset::new("budget", crate::GeometryType::Polyline, vec![crate::Field::new("text", crate::FieldType::Text)]);
        for i in 0..300 {
            ds.push(
                Some(Geometry::Line(vec![(0..50).map(|n| crate::Coord::new(i as f64 * 0.001, n as f64 * 0.001)).collect()])),
                vec![Value::Text("x".repeat(400))],
            );
        }
        let (bytes, _) = crate::io::write_bytes(&ds, "fgb").unwrap();
        crate::source::open_bytes("budget.fgb", bytes, None).unwrap()
    }

    #[test]
    fn budgeted_windows_preserve_ids_attributes_order_and_full_materialization() {
        let src = source();
        let request = crate::source::Request::all();
        let full = src.read(&request).unwrap();
        let bytes = 32 * 1024;
        let sample = src.read_budgeted(&request, bytes).unwrap();
        assert!(features_bytes(&sample.features) <= bytes);
        assert!(!sample.complete);
        assert_eq!(sample.matched, full.matched);
        assert_eq!(sample.skipped, full.skipped);
        let mut previous = None;
        for feature in &sample.features {
            let position = full.features.iter().position(|f| f == feature).unwrap();
            assert!(previous.is_none_or(|old| old < position));
            previous = Some(position);
        }
        assert_eq!(src.read(&request).unwrap().features, full.features);
        assert_eq!(crate::source::materialize(src.as_ref()).unwrap().0.features.len(), 300);
        assert!(src.read_budgeted(&request, 0).is_err());
        let empty = src.read_budgeted(&crate::source::Request { limit: 0, ..request }, bytes).unwrap();
        assert!(empty.features.is_empty());
    }
}
