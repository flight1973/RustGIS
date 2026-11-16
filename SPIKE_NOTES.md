# Z layers spike (option E, issue #726)

Branch `z-layers-spike`, from `origin/main` 414dd199. A measurement spike, not a feature to merge.
Nothing pushed.

## Design

- `rustgis_geom::CoordZ { x, y, z }` beside the unchanged 16-byte `Coord`.
- `rustgis_data::Geom<C>` is the old `Geometry` enum made generic over its position;
  `Geometry = Geom<Coord>` and `GeometryZ = Geom<CoordZ>` are type aliases, so every existing
  `Geometry::Point(..)` pattern and constructor compiles unchanged. Code written once over
  `C: Position` (vertex edits, ring walks) serves both.
- A 3D layer's feature holds a private `geometry_z: Option<Box<GeometryZ>>` (canonical) beside
  the public 2D `geometry` (its x and y, what the map and 2D tools read). Setters keep them
  together (`Feature::new_z`, `set_geometry_z`, `set_geometry_2d`, `drop_z`).
- Invariant: `Feature::geometry_z()` returns the heights only when their x and y still equal
  `geometry` exactly. Code that writes `geometry` alone (2D edits, tools) leaves the heights
  behind, and they then read as lost, never as heights of other points. Exports and saves say so.
- A layer is 3D when its features carry heights (`Dataset::has_z`), decided by the reader.

## Checklist

(updated per step below)

## Measurements

(updated per step below)

## Hard spots

(updated per step below)

## Next step

(updated per step below)
