# Z/M arrays prototype (#726, open question 1)

Branch `z-arrays-prototype`, based on `origin/main` (f0dc6b68). A design prototype, not meant to
merge as is. Nothing pushed.

## Design

- `Feature` gains `zm: Option<Box<Zm>>` (`crates/data/src/lib.rs`), serde default + skipped when
  `None`. `Zm { z: Option<Vec<f64>>, m: Option<Vec<f64>> }`, one value per vertex in
  `Geometry::for_each_coord` order (parts, rings, points, closing points included).
- `Geometry::coord_count()` (already there) is the vertex count. `Feature::zm_fits()` checks the
  arrays against it; `Feature::z()` gives heights only when they fit.
- `Feature::zm_after(same)`: for code that replaces a geometry. Carries the arrays when `same`
  (same vertices, same order); otherwise *empties* them, so they never fit again and an export says
  the heights were lost (instead of a silent drop or heights on the wrong vertices).
- GeoJSON read whole (`Collector` in `crates/data/src/io.rs`, `geojson_z`) keeps Z when every
  position has a numeric third value; partial Z or a 4th number → notice (`geojson_zm_notes`),
  `zm_dropped` true. In-place GeoJSON source (>32 MB) stays 2D and says so (unchanged).
- GeoJSON write (`feature_to_geojson` → `geometry_to_geojson_z`) writes `[x, y, z]` when Z fits.
- `zm_written_note(ds, ext, target)` (lib.rs): export notice for features whose Z isn't written
  (unfit in GeoJSON; any Z in other formats; M in GeoJSON). Called from `share.exportLayer`
  (share.rs `write_layer`), table export (share.rs), workflows convert/export, web sharing
  (sharing.rs `note_export`), the desktop download (ui-egui lib.rs).
- `zm_export_note` wording changed to "lacks Z or M values that were dropped" (was "has no Z or M
  values"), since a GeoJSON layer may keep Z but lose M or some features' Z.
- `chunk_memory::feature_bytes` counts the arrays.

## Checklist (brief items)

1. Zm type + helpers: done.
2. Feature literal sites: done (compiles, `cargo check --workspace --all-targets` clean).
3. GeoJSON read: done.
4. GeoJSON write + export notice: done.
5. `zm_dropped` honest: done for GeoJSON read.
6. Projects: serde round trip — test written (z_arrays.rs `a_saved_project_keeps_the_heights`),
   being run. Older release finding below.
7. Tests: written (`crates/engine/tests/z_arrays.rs`, data lib tests); data lib tests pass; engine
   tests being run.
8. `cargo xtask ci` + wasm build: not started.

## Where Z is dropped (tools/edits that change geometry)

Tool outputs built as new features (zm: None, silent — no notice anywhere):
- `crates/analysis/src/tools.rs:1495` buffer: new polygons.
- `crates/analysis/src/tools.rs:3173` point outputs from vertices/geometry: new points (could carry
  one Z each, but tools aren't Z-aware).
- `crates/analysis/src/tools/geometry.rs:144` lines split into segments.
- Every tool that builds output through `Dataset::push` (most of analysis): new geometry, None.

Emptied with `zm_after(false)` (export says so):
- `crates/engine/src/cmd/design.rs` `modify`: `edit.flip` (reversed order); moves/turns/scales keep
  Z when the vertex count is unchanged.
- design.rs `replace_geometry`: `edit.setGeometry`, `edit.vertex` insert/delete (move keeps Z).
- design.rs merge, explode (all parts), split (both pieces).
- `crates/engine/src/cmd/design/sketch.rs` cut polygon (all pieces), reshape.
- `crates/engine/src/cmd/items.rs` simplify for an offline/package copy (kept if unchanged).
- `crates/engine/src/cmd/graphics.rs` graphic placed again.
- `crates/analysis/src/tools/repair_geometry.rs:196` repaired or removed geometry.
- `crates/analysis/src/tools/check_geometry.rs:155` geometry left out (non-finite).

Carried: attribute join, zonal statistics (tools.rs:7412), check geometry rows, clipboard paste,
offline sync struct update (workspace.rs:751), shared-vertex move (in-place, same count),
reprojection via `map_coords` (io.rs project, geolibre convert).

## Measurements so far

- `size_of::<Feature>()`: 64 → 72 bytes (64-bit); `Zm` 48 bytes boxed; 8 bytes/vertex per array.
- Older release + project with `zm`: `Feature` derives `Deserialize` without
  `deny_unknown_fields` (lib.rs), so an older RustGIS opens it, silently ignores `zm`, and a resave
  loses the heights for good. `FORMAT` is 1 (`crates/engine/src/project.rs:57`, doc: "Advance it
  when an older reader would lose meaningful saved information on resaving") — by that rule this
  change should bump it, which would make older releases refuse the file. Not changed here.
  Test: lib.rs `a_reader_without_heights_skips_them_silently`.

## Open problems

- Tools drop Z silently (output is new features, no notice).
- Plugins exchange GeoJSON through `geometry_to_geojson` (2D): Z silently not passed.
- In-place mutation sites aren't found by the compiler (only literals are): any `f.geometry = …`
  that forgets `zm_after` keeps stale arrays; caught on export only if the vertex count changed.
- A legacy GeoJSON `crs` converts x/y but leaves Z as given (no vertical datum on the layer yet).
- Web map notices count features but don't name the layer.

## Next step

Run `cargo test -p rustgis-engine --test z_arrays`, fix failures, then the analysis/engine test
suites, `cargo xtask ci`, and `cargo build --target wasm32-unknown-unknown -p rustgis-web`.
