# Z layers spike (option E, issue #726)

Branch `z-layers-spike`, from `origin/main` 414dd199. A measurement spike, not a feature to merge.
Nothing pushed. Compare with branch `z-arrays-prototype` (`git show z-arrays-prototype:PROTOTYPE_NOTES.md`).

## Design

- `rustgis_geom::CoordZ { x, y, z }` (24 bytes) beside the unchanged 16-byte `Coord`.
- `rustgis_data::Geom<C>` is the old `Geometry` enum made generic over its position. Two type
  aliases: `Geometry = Geom<Coord>` (unchanged for every 2D caller: `Geometry::Point(..)` patterns
  and constructors compile through the alias) and `GeometryZ = Geom<CoordZ>`. Trait
  `Position { fn xy(self) -> Coord }` lets code be written once for both (vertex edits, ring
  closing/turning in `publish.rs::closed_parts`, GeoJSON read/write).
- A 3D layer's feature holds a private `geometry_z: Option<Box<GeometryZ>>` (canonical) beside the
  public 2D `geometry` (its x and y: what the map, selection, identify and 2D tools read). Setters
  keep them together: `Feature::new` (2D), `Feature::new_z`, `set_geometry_z`, `set_geometry_2d`
  (explicit drop, returns whether heights were dropped, for the notice), `drop_z`.
- Private invariant, checked on read: `Feature::geometry_z()` returns the heights only when their
  x and y are still exactly `geometry` (`Geom::same_xy`, NaN by bits). Code that writes `geometry`
  alone (a 2D edit, a tool writing in place, a reprojection) leaves the heights behind, and they
  then read as lost (`Feature::lost_z`), never as heights of other points. Export says how many.
- A layer is 3D when its features carry heights (`Dataset::has_z`); readers decide. No new
  `Dataset` field.
- Why this form: the generic enum gives the "mirror of `Geometry`" at zero cost to 2D call sites;
  the derived 2D copy beside the canonical 3D one keeps every 2D reader (about 1,100
  `.geometry` uses) unchanged. Making `geometry` itself 3D-capable would touch them all.

## Checklist

1. GeoJSON with Z reads into a 3D layer, draws, exports every Z exact; 2D GeoJSON unchanged and
   quiet: done (`crates/engine/tests/z_layers.rs`).
2. Edits keep heights: done. `edit.flip` (generic over position), `edit.vertex` move (keeps z,
   or takes `"z"`), insert (linear interpolation along the edge), delete. Other edits made in 2D
   drop heights with a notice (`heights_dropped_notice`, design.rs).
3. 2D tool on a 3D layer says heights were dropped: done (`heights_note` in
   `crates/analysis/src/lib.rs`, both `run` and `run_resolved`).
4. Project save/reopen with Z: done; saving requires capability `feature-heights`.
5. Ring closing/turning keeps z: done (`publish.rs::closed_parts`, test
   `heights_tests::closing_and_turning_a_ring_keeps_each_height_with_its_point`).
6. hostile_params_never_panic and never_silent_roundtrip: in progress (running in `cargo xtask ci`).
7. `cargo xtask ci` and wasm build: in progress.

## Measurements

- `size_of` (64-bit, test `data::heights_tests::sizes`): `Coord` 16, `CoordZ` 24,
  `Geometry` == `GeometryZ` (same enum layout), `Feature` 64 -> 72 (the boxed pointer),
  `Dataset` 112 -> 112.
- Bytes per vertex: 2D layer 16 (unchanged). 3D layer 16 (2D view) + 24 (canonical) = 40, plus
  one boxed `GeometryZ` per feature. Arrays-on-feature: 24.
- Churn so far: see the final report; ~123 `Feature { .. }` literals became `Feature::new(..)`
  (mechanical, by a script driven by compiler errors), 3 `..f.clone()` struct updates rewritten.
  No `match` on `Geometry` changed.

## Hard spots

- Holes the compiler can't close in this form: `f.geometry = ..` (about 40 non-test sites) and
  in-place `&mut` edits of the 2D view are runtime-detected, not compile-time. A 2D reorder whose
  result has the same x/y sequence (a vertical line, a line that retraces itself) is not
  detectable, so its heights would stay in the old order.
- Mixed file rule (GeoJSON): 3D only when every feature with a shape has exactly three numbers at
  every position; otherwise the whole layer is 2D and the existing Z/M notice counts the features
  that had heights. TopoJSON (read through GeoJSON) stays 2D for now (`Dataset::flatten_z`).
- Other formats still drop Z as before (Shapefile, GeoPackage, FlatGeobuf, KML, GPX ...). Export
  of a 3D layer to them says so (`z_export_note`).

## Next step

Wait for `cargo xtask ci` and the wasm build; fix what they find; then the final report.
