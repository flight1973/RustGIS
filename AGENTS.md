# RustGIS — instructions for agents

RustGIS is a fast, free and open-source GIS in pure Rust for mapping, analyzing and sharing
geospatial data, on the desktop and in the browser. Its architecture: layered crates, everything
is a command, a thin egui front end, headless screenshots for UI work.

Before picking work, read [ROADMAP.md](ROADMAP.md) (check Underway) and
[CONTRIBUTING.md](CONTRIBUTING.md) (what to coordinate first).

## Never crash
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` in non-test code; no `unsafe`
  (`unsafe_code = "forbid"`). Crate roots carry the `#![deny(clippy::unwrap_used, …)]` block.
- Input from files and command parameters is hostile: `get()` over indexing, clamp sizes,
  bound recursion. `Session::run` and `analysis::run` catch escaped panics as a last resort.
- `crates/engine/src/tests.rs::hostile_params_never_panic` and the analysis equivalent run every
  command/tool with junk parameters. Keep them green; every crash fix gets a regression test.

## Architecture
| Layer | Crate | Job |
|---|---|---|
| L0 | `geom` | coordinates, extents, Web Mercator, geodesic measurement, scale |
| L1 | `data` | features, typed attributes, GeoJSON/CSV/Shapefile/KML/GPX I/O, GeoPackage/GeoParquet/FlatGeobuf/LiDAR (LAS/LAZ)/PostGIS/MySQL providers (`source`; PostGIS through the pure-Rust `postgres` client, MySQL and MariaDB through the pure-Rust `mysql` client, desktop only; GeoPackages in the browser through the pure-Rust `sqlite` reader; GDAL formats and DuckDB queries through their programs), SQL where clauses, rasters (GeoTIFF/ASCII grid, Zarr stores, EPSG reprojection) |
| L2 | `map`, `analysis` | layers, symbology, labels, camera, basemaps, web tile layers; geoprocessing tools |
| L2 | `tiles` | web map tiles: XYZ/TMS/WMS rasters, Mapbox Vector Tiles, PMTiles and MBTiles archives, a MapLibre style subset; the tile cache and background fetching (see `docs/user-guide/maps.md` › Basemaps) |
| L3 | `render` | tiny-skia map renderer, label placement, PNG export, font discovery |
| L3 | `plugins` | plugins: sandboxed WebAssembly modules (`wasmi`) adding commands and tools; `.rgplugin` packages, the plugin registry (see `docs/plugins.md`) |
| L3 | `geolibre` | the GeoLibre/Whitebox tool suite as geoprocessing tools (engine feature `geolibre`): run by the optional `geolibre` program natively, by geolibre-rust's WASI runner in the browser |
| L4 | `engine` | `Session`, undo, command registry, sample project |
| L5 | `assistant` | the AI Assistant: a chat agent (sans I/O: `provider` builds and reads OpenAI Chat Completions/Responses, Anthropic and Gemini requests, `chatgpt` signs in with ChatGPT to use the user's plan, `agent` loops, `tools` run commands; file, network and plugin commands wait for the user's approval) |
| L6 | `ui-egui` | ribbon, panes, map view, attribute table, dialogs (swappable) |
| apps | `rustgis`, `rustgis-cli`, `rustgis-web`, `rustgis-python` | desktop app, command line, browser (WebAssembly), the `rustgis` Python package (PyO3, built with maturin) |

- **Everything is a command**: a `CommandSpec` in `crates/engine/src/cmd/*.rs`. UI-only
  commands (`ui.*`) live in `crates/ui-egui/src/lib.rs`. The ribbon, shortcuts, command search
  and CLI dispatch by id. Commands never open dialogs.
- **Ribbon placement** follows the
  [ribbon guidelines](docs/contributing.md#ribbon-guidelines): the tab's purpose, one button per
  destination, labels that say what happens, and what a ribbon pull request states.
- **Never break wasm**: everything below `apps/` must build for `wasm32-unknown-unknown`
  (CI runs `cargo build --target wasm32-unknown-unknown -p rustgis-web`; the Zstandard decoder in
  `rustgis-data` is C, so this needs clang). Use `web_time`, not
  `std::time`; file access goes through `Services` on the web. `rustgis-geolibre` builds for
  both: natively it runs the `geolibre` program; in the browser the web app runs geolibre-rust's
  WASI runner asynchronously (`external_start` → `Services::geolibre_run` → `gp.finish`).
- **Interface text is translatable**: wrap user-visible literals in `tl!("…")`, use
  `i18n::fmt("… {name} …", &[("name", …)])` instead of `format!` for messages, and pass labels to
  the ribbon widgets (they translate). Pull requests don't need to add the new strings to the
  catalogs in `crates/ui-egui/src/i18n/*.tsv`: a missing entry shows in English, and the
  maintainer translates them before each release (format in `docs/i18n.md`). Command ids, data
  and the CLI are never translated.
- **Layering** is enforced by `cargo xtask layers`: nothing below L6 depends on egui/eframe/rfd.

## Assets
- No third-party vendor icons, images, styles, fonts, help text or code. Icons are drawn in
  code (`icons.rs`). Every asset file has a row in `ATTRIBUTION.md`.
- Screenshots and other images are not committed here: they live in
  [opengeos/rustgis-assets](https://github.com/opengeos/rustgis-assets) (WebP, served at
  `https://assets.rustgis.app/images/…`) and docs link to them by URL.

## Quality gates
`cargo xtask ci` (fmt, clippy -D warnings, tests, layers) before every commit.
A new dependency must have a licence `deny.toml` allows (CI runs `cargo deny check licenses`); after
changing dependencies, run `cargo xtask notices` to refresh the `THIRD-PARTY-LICENSES.txt` files the
packages ship (the release build fails on stale ones).
Pull requests run CI on Linux; pushes to `main` add macOS and Windows. Label a pull request
`ci:windows`, `ci:macos` or `ci:full` to test there before merging (platform-specific code, paths,
file dialogs). The release build of the apps runs nightly (`.github/workflows/nightly.yml`), with every
platform's tests; a failure opens a `Nightly build failed` issue. `ci:full` also runs the release
build on the pull request.

## Looking at the app
- `cargo run --release -p rustgis` (opens the World Atlas sample).
- Headless screenshots: `cargo run --release -p rustgis-ui-egui --example ui_shot -- script.jsonl`
  (see the example header). For UI work, render the actual changed UI and look at the PNG.
  For substantial visible changes, include screenshots in the PR (before/after where useful);
  see [attachment guidance](docs/contributing.md#documentation).
- CLI: `rustgis-cli run --sample --cmd 'gp.run={"tool":"buffer","params":{"input":"Cities","distance":100}}' --cmd 'share.exportMap={"path":"out.png"}'`.
