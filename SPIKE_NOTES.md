# Z layers spike (option E, issue #726)

Branch `z-layers-spike`, from `origin/main` 414dd199. This is a measurement spike, not a
feature to merge. Nothing is pushed. Compare it with the branch `z-arrays-prototype`
(`git show z-arrays-prototype:PROTOTYPE_NOTES.md`). Diff against 414dd199, not `origin/main`,
because `origin/main` has moved since.

## Design

- `rustgis_geom::CoordZ { x, y, z }` (24 bytes) sits beside the unchanged 16-byte `Coord`
  (crates/geom/src/lib.rs:89).
- `rustgis_data::Geom<C>` is the old `Geometry` enum made generic over its position
  (crates/data/src/geometry.rs:145). There are two aliases: `Geometry = Geom<Coord>` and
  `GeometryZ = Geom<CoordZ>`. Every existing `Geometry::Point(..)` pattern and constructor
  compiles through the alias, so no `match` on `Geometry` changed. `trait Position { fn xy }`
  lets code be written once for both kinds: the vertex edits (geometry.rs:550), ring closing and
  turning (`publish.rs::closed_parts`), and GeoJSON reading and writing (`io.rs::geom_from_geojson`).
- A 3D feature holds a private `geometry_z: Option<Box<GeometryZ>>` (lib.rs:214). This is the
  canonical geometry. The public 2D `geometry` beside it holds its x and y, which the map,
  selection, identify, the table and every 2D tool read. Setters keep the two together:
  `Feature::new` (2D), `new_z`, `set_geometry_z`, `set_geometry_2d` (an explicit drop that reports
  whether heights were dropped, for the notice), and `drop_z`.
- A private invariant is checked on read. `Feature::geometry_z()` (lib.rs:231) returns the
  heights only when their x and y still exactly equal `geometry` (`Geom::same_xy`, NaN compared by
  bits). Code that writes `geometry` alone leaves the heights behind. They then read as lost
  (`lost_z`), never as the heights of other points, and the export counts them (`z_export_note`).
- A layer is 3D when its features carry heights (`Dataset::has_z`). The reader decides this. No
  `Dataset` field was added.
- Why this form: the generic enum is the "mirror of `Geometry`" at no cost to 2D call sites. The
  derived 2D copy keeps about 1,100 `.geometry` readers unchanged. Making `Feature::geometry`
  itself 3D-capable would touch all of them.

## Checklist

1. A GeoJSON with Z reads into a 3D layer, draws, and exports every Z exactly. A 2D GeoJSON is
   unchanged and gives no notice. **Done** (crates/engine/tests/z_layers.rs).
2. Edits keep heights. **Done**:
   - `edit.flip` is generic over position.
   - `edit.vertex` move keeps the vertex's z, or takes `"z"`.
   - Insert interpolates z linearly along the edge (`GeometryZ::height_on_edge`).
   - Delete takes the height with its vertex.
   - Other edits through `modify` (move, rotate, scale) and `edit.setGeometry` work in 2D and
     drop the heights with a notice (design.rs `heights_dropped_notice`).
3. A 2D tool on a 3D layer says the heights were dropped. **Done** (`heights_note`,
   crates/analysis/src/lib.rs:606, called from both `run` and `run_resolved`).
4. A project saves and reopens with Z. **Done**. Saving one requires the capability
   `feature-heights` (project.rs:80, project_capabilities.rs:109).
5. Ring closing and turning keeps z. **Done** (`publish.rs::closed_parts`, with a test in
   `publish.rs` `heights_tests`).
6. `hostile_params_never_panic` (engine and analysis) and `never_silent_roundtrip` are green.
   **Done**. Older tests that used `[x,y,z]` to get a "Z dropped" notice now use a measure
   (`[1,2,null,30]`, `[x,y,z,m]`). `never_silent_reading`'s GeoJSON case is now "a measure".
7. `cargo xtask ci` passes. `cargo build --target wasm32-unknown-unknown -p rustgis-web` passes.
   **Done**.

## Measurements

- `size_of` on 64-bit (test `data::heights_tests::sizes`):

  | Type | Before | After |
  |---|---|---|
  | `Coord` | 16 | 16 |
  | `CoordZ` | – | 24 |
  | `Geometry` | 32 | 32, same as `GeometryZ` |
  | `Feature` | 64 | 72 (the boxed pointer) |
  | `Dataset` | 112 | 112 |

- Bytes per vertex: a 2D layer still uses 16. A 3D layer uses 16 for the 2D view plus 24 for the
  canonical geometry, 40 in all, plus one boxed `GeometryZ` per feature. Arrays on the feature
  used 24.
- Churn against 414dd199: 61 .rs files, +1102/−409, including the new 229-line test file.
  - About 123 `Feature { .. }` literals became `Feature::new(..)`. A script driven by compiler
    errors rewrote them.
  - 3 `..f.clone()` struct updates were rewritten.
  - The 2D logic sites that changed are about 14 functions: 4 in design.rs, the GeoJSON parse,
    write and collect code in io.rs, `publish.rs::geometry_parts`, analysis `run` and
    `run_resolved`, one line in `share.rs::write_layer`, topojson, and the capabilities.

## Hard spots

- **What the compiler can't catch in this form.** Writes to `geometry` alone
  (`f.geometry = ..`, about 40 sites outside tests) and in-place `&mut` edits of the 2D view are
  caught at runtime, not at compile time. A loss shows up at export, not at the edit (for example
  `edit.moveSharedVertex`).
- **Reorders that leave x and y the same can't be seen.** If a 2D reorder gives the same x/y
  sequence (a vertical line such as a borehole, or a line that retraces itself), the heights stay
  in the old order. The test `a_2d_reorder_that_leaves_the_same_xy_is_not_seen` pins this.
- **Mixed files (GeoJSON).** A layer is 3D only when every feature with a shape has exactly three
  numbers at every position. Otherwise the whole layer is 2D, and the existing Z/M notice counts
  the features that had heights. A fourth number (M) makes the layer 2D. TopoJSON is read
  through GeoJSON but stays 2D for now (`Dataset::flatten_z`).
- **Other formats.** They still drop Z on read. A 3D layer exported to them says it is written
  with x and y only (`z_export_note`). The in-place GeoJSON source (>32 MB) stays 2D.
- **Not 3D-aware yet:** plugins (`host.features`, `host.addLayer`), identify, the attribute table,
  move/rotate/scale, merge, split and reshape. Each of these works in 2D and drops the heights,
  either with a notice or, for in-place writers, at export.
- **Project schema.** The `$defs` names became `GeometryCoord` and `GeometryCoordZ`. Saved files
  are unchanged for 2D layers.

## Next step

None for the spike. The final report goes to the coordinator. To continue toward Phase 1:
1. Make `Feature::geometry` private behind an accessor, so that 2D writes need an explicit step.
   This touches about 1,100 sites.
2. Write the move, rotate and scale transforms generically over `Position`.
3. Then read and write Z in Shapefile, GeoPackage and FlatGeobuf.
