//! Memoized allocation accounting. Weak identities invalidate on copy-on-write mutation without
//! retaining old windows or forcing a deep clone of their rows.
use rustgis_data::{Feature, Features, Geometry, source::memory};
use rustgis_geom::Extent;
use std::sync::{Arc, Mutex, Weak};

type Levels = Vec<(i32, Option<Arc<Vec<Option<Geometry>>>>)>;

#[derive(Debug)]
struct Allocation<T> {
    key: Weak<Vec<T>>,
    bytes: usize,
}

impl<T> Default for Allocation<T> {
    fn default() -> Self {
        Self { key: Weak::new(), bytes: 0 }
    }
}

impl<T> Allocation<T> {
    fn get(&mut self, key: Weak<Vec<T>>, measure: impl FnOnce() -> usize) -> usize {
        if !self.key.ptr_eq(&key) {
            self.bytes = measure();
            self.key = key;
        }
        self.bytes
    }
}

#[derive(Debug, Default)]
struct State {
    rows: Allocation<Feature>,
    projected: Allocation<Option<Geometry>>,
    levels: Vec<Allocation<Option<Geometry>>>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Cache(Arc<Mutex<State>>);

impl Cache {
    fn with(&self, f: impl FnOnce(&mut State) -> usize) -> usize {
        // A poisoned memo contains no authoritative data: recover and remeasure as needed.
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut state)
    }

    pub(super) fn seed(&self, rows: &Features, projected: &Arc<Vec<Option<Geometry>>>, row_bytes: usize, projected_bytes: usize) {
        self.with(|s| {
            s.rows = Allocation { key: rows.allocation_key(), bytes: row_bytes };
            s.projected = Allocation { key: Arc::downgrade(projected), bytes: projected_bytes };
            0
        });
    }

    pub(super) fn base(&self, rows: &Features, projected: &Arc<Vec<Option<Geometry>>>, extents: &Vec<Extent>) -> usize {
        self.with(|s| s.base(rows, projected, extents))
    }

    pub(super) fn total(
        &self,
        rows: &Features,
        projected: &Arc<Vec<Option<Geometry>>>,
        extents: &Vec<Extent>,
        levels: &Arc<Levels>,
    ) -> usize {
        self.with(|s| {
            let mut bytes = s.base(rows, projected, extents);
            for c in levels.iter().filter_map(|(_, c)| c.as_ref()) {
                let key = Arc::downgrade(c);
                let extra = if let Some(old) = s.levels.iter().find(|old| old.key.ptr_eq(&key)) {
                    old.bytes
                } else {
                    let bytes = crate::lod::cache_bytes(c);
                    s.levels.push(Allocation { key, bytes });
                    bytes
                };
                bytes = bytes.saturating_add(extra);
            }
            // LRU reordering changes the outer Vec but not the geometry allocations. Measure
            // only new copies, and keep a bounded set of weak entries for the current levels.
            s.levels.retain(|old| levels.iter().filter_map(|(_, c)| c.as_ref()).any(|c| old.key.ptr_eq(&Arc::downgrade(c))));
            bytes
        })
    }
}

impl State {
    fn base(&mut self, rows: &Features, projected: &Arc<Vec<Option<Geometry>>>, extents: &Vec<Extent>) -> usize {
        self.rows
            .get(rows.allocation_key(), || memory::features_bytes(rows))
            .saturating_add(self.projected.get(Arc::downgrade(projected), || crate::lod::cache_bytes(projected)))
            .saturating_add(extents.capacity().saturating_mul(std::mem::size_of::<Extent>()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_allocations_reuse_totals_and_weak_keys_do_not_retain_rows() {
        let mut rows: Features = vec![Feature { id: 1, geometry: None, attrs: vec![], zm: None }].into();
        let key = rows.allocation_key();
        let mut measured = Allocation::default();
        assert_eq!(measured.get(key.clone(), || 123), 123);
        for _ in 0..100 {
            assert_eq!(measured.get(rows.allocation_key(), || panic!("unchanged rows were walked again")), 123);
        }
        // There is no strong snapshot here: even an in-place edit must invalidate the memo.
        rows[0].attrs.push(rustgis_data::Value::Text("changed".into()));
        assert!(!key.ptr_eq(&rows.allocation_key()));
        assert!(key.upgrade().is_none());
        let used = memory::features_bytes(&rows);
        assert_eq!(measured.get(rows.allocation_key(), || used), used);
        let key = rows.allocation_key();
        drop(rows);
        assert!(key.upgrade().is_none(), "accounting must not keep an evicted window alive");
    }
    #[test]
    fn accounting_tracks_attributes_geometry_lod_snapshots_and_eviction() {
        let mut ds = rustgis_data::Dataset::new(
            "memory",
            rustgis_data::GeometryType::Polyline,
            vec![rustgis_data::Field::new("text", rustgis_data::FieldType::Text)],
        );
        ds.push(Some(Geometry::Line(vec![vec![rustgis_geom::Coord::new(0.0, 0.0); 100]])), vec![rustgis_data::Value::Text("x".into())]);
        let mut layer = crate::Layer::new(1, ds);
        let exact = |l: &crate::Layer| {
            memory::features_bytes(&l.dataset.features)
                + crate::lod::cache_bytes(&l.projected)
                + l.projected_extents.capacity() * std::mem::size_of::<Extent>()
                + l.thinned.iter().filter_map(|(_, g)| g.as_ref()).map(|g| crate::lod::cache_bytes(g)).sum::<usize>()
        };
        layer.projected();
        let original = layer.view_cache_bytes();
        assert_eq!(original, exact(&layer));
        let old = layer.clone();
        layer.dataset.features[0].attrs[0] = rustgis_data::Value::Text("x".repeat(4096));
        let edited = layer.view_cache_bytes();
        assert!(edited > original);
        assert_eq!(edited, exact(&layer));
        assert_eq!(old.view_cache_bytes(), original);
        assert_eq!(layer.view_cache_bytes(), edited);
        // Public row mutation is detected without requiring a geometry revision bump.
        layer.dataset.features[0].geometry = Some(Geometry::Line(vec![vec![rustgis_geom::Coord::new(0.0, 0.0); 500]]));
        assert_eq!(layer.view_cache_bytes(), exact(&layer));
        Arc::make_mut(&mut layer.projected).reserve(100);
        Arc::make_mut(&mut layer.projected_extents).reserve(100);
        assert_eq!(layer.view_cache_bytes(), exact(&layer));
        let copy = Arc::new(vec![Some(Geometry::Line(vec![vec![rustgis_geom::Coord::new(0.0, 0.0); 30]]))]);
        Arc::make_mut(&mut layer.thinned).push((5, Some(copy)));
        assert_eq!(layer.view_cache_bytes(), exact(&layer));
        Arc::make_mut(&mut layer.thinned).insert(0, (6, None));
        assert_eq!(layer.view_cache_bytes(), exact(&layer));
        layer.clear_cache();
        assert_eq!(layer.view_cache_bytes(), memory::features_bytes(&layer.dataset.features));
        layer.install(Vec::new());
        assert_eq!(layer.view_cache_bytes(), 0);
        assert_eq!(old.view_cache_bytes(), original);
    }
}
