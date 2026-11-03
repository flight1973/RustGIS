//! Geometry families present in loaded rows, memoized without retaining the rows. A mixed
//! GeoJSON's declared family is only its dominant one; styling must also see its other shapes.

use std::sync::{Arc, Mutex, Weak};

use rustgis_data::{Feature, Features, GeometryType};

#[derive(Debug, Default)]
struct State {
    rows: Weak<Vec<Feature>>,
    kinds: u8,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Cache(Arc<Mutex<State>>);

fn bit(kind: GeometryType) -> u8 {
    match kind {
        GeometryType::Point => 1,
        GeometryType::Polyline => 2,
        GeometryType::Polygon => 4,
        GeometryType::None => 0,
    }
}

impl Cache {
    pub(super) fn contains(&self, rows: &Features, kind: GeometryType) -> bool {
        // The memo is disposable; a poisoned lock does not invalidate the features themselves.
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let key = rows.allocation_key();
        if !state.rows.ptr_eq(&key) {
            state.kinds = rows.iter().filter_map(|f| f.geometry.as_ref()).fold(0, |kinds, g| kinds | bit(g.kind()));
            state.rows = key;
        }
        state.kinds & bit(kind) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustgis_data::{Dataset, Geometry};
    use rustgis_geom::Coord;

    #[test]
    fn geometry_kinds_follow_edits_replacements_and_snapshots() {
        let mut ds = Dataset::new("mixed", GeometryType::Polyline, vec![]);
        ds.push(Some(Geometry::Line(vec![vec![Coord::new(0.0, 0.0), Coord::new(1.0, 1.0)]])), vec![]);
        let cache = Cache::default();
        assert!(!cache.contains(&ds.features, GeometryType::Polygon));
        let before = ds.features.clone();
        ds.features[0].geometry = Some(Geometry::Polygon(vec![vec![vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 0.0),
        ]]]));
        assert!(cache.contains(&ds.features, GeometryType::Polygon));
        assert!(!cache.contains(&before, GeometryType::Polygon));
        assert!(cache.contains(&ds.features, GeometryType::Polygon));
        // A weak identity also invalidates when the rows have no other strong owner.
        ds.features[0].geometry = None;
        assert!(!cache.contains(&ds.features, GeometryType::Polygon));
        assert!(!cache.contains(&ds.features, GeometryType::None));
    }
}
