# Contributing

Contributions are welcome. See [AGENTS.md](https://github.com/opengeos/RustGIS/blob/main/AGENTS.md)
for the conventions.

1. Fork the repository and create a branch.
2. Make your change with tests. Engine behaviour is a command in `crates/engine/src/cmd/`;
   UI-only commands (`ui.*`) live in `crates/ui-egui/src/lib.rs`.
3. Run `cargo xtask ci` and `pre-commit run --all-files`.
4. Open a pull request. Rust CI runs on Linux by default. A maintainer can label it `ci:windows`,
   `ci:macos` or `ci:full` to test those platforms before merging; pushes to `main` test all three.
   `ci:full` also builds the release apps. The nightly workflow tests and release-builds all three
   when there are new commits, and opens an issue on failure.
5. The docs workflow builds a site and web-app preview. A first-time fork contributor's preview
   needs a maintainer to add `preview` or re-run the workflow; returning contributors' previews
   publish automatically.

A maintainer's `automerge` label approves and squash-merges a pull request after its required
checks pass. Pushing again removes the label and disables auto-merge; removing the label also
disables it.

A maintainer's `claude-autofix` label has Claude bring a pull request up to date: it merges the base
branch and resolves any conflicts, then fixes the open review comments it agrees with, replies to
each and resolves the ones it fixed or found already fixed, and pushes the commits to the branch (a
fork's too, when edits by maintainers are allowed). The label comes off when it's done; add it again
for another round.

## Developer checks

`cargo xtask ci` runs `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -q` and
`cargo xtask layers`. The pre-commit hooks also check TOML, YAML, JSON, whitespace and large files.
CI separately builds `rustgis-web` for `wasm32-unknown-unknown` and runs
`cargo deny --locked check licenses` against `deny.toml`.

Other workspace tasks:

| Task | Purpose |
| --- | --- |
| `cargo xtask layers` | Check crate dependency direction and keep UI dependencies out of lower layers |
| `cargo xtask notices` | Regenerate the shipped `THIRD-PARTY-LICENSES.txt` files after changing dependencies; requires `cargo-about` |
| `cargo xtask notices --check` | Check the notices without writing them; stale notices warn in CI but block releases |
| `cargo xtask web-pwa <dir>` | Generate the service worker in a Trunk build output directory, such as `dist/web` |

Interface strings must be translatable: use `tl!("…")` for literals and
`i18n::fmt("… {name} …", &[("name", value)])` for messages with values. New strings can fall back
to English until the catalogs are updated; see [Languages](i18n.md).

## Ribbon guidelines

These guidelines decide where a ribbon control goes and what kind of control it is. They guide
new features and small, tab-by-tab improvements; they are not a claim that the current ribbon
already follows every rule. Decisions about particular controls live in their issues.

### Start with the task and the tab's purpose

Use the tab's purpose to decide where a feature belongs, rather than putting it wherever there
is space. Agree on that purpose before reorganising a tab.

| Tab | Purpose |
| --- | --- |
| Home | The basics of working with a map: starting a map or layout, adding data, moving around, and selecting and inspecting features. |
| Data | Working with the content and structure of data: looking at records, changing fields and values, connecting tables, and bringing data in and out. Exports here produce data, such as features and rasters. |
| Style | Deciding how a layer looks on the map: how its features are drawn and shown at different scales. |
| Labeling | How the map is labelled: what gets a label, with which text, where, and at which scales, plus the label settings for the whole map. |
| Edit | Creating and changing features and graphics on the map, and managing those edits. |
| Analyze | Turning existing data into new results, from single tools to repeatable workflows, scripts and automation. |
| View | How the map and workspace are displayed: map appearance and view configuration, and which panes and windows are open. Basic navigation stays on Home. |
| Publish | Getting work out to others: printing, exporting or sharing products such as maps, layouts, reports and packages, including on the web. |
| Plugins | Finding, installing and using plugins that extend RustGIS. |
| Settings | Preferences that apply to every project, such as language, appearance and start-up. |
| Help | Learning RustGIS, finding commands and shortcuts, and getting information about the app. |

Keep Data, Style and Labeling fixed, rather than making the tab strip change with the selected
layer. Their target should be explicit: a layer chooser at the left of each layer-dependent tab
(for example, “Styling: Countries ▾”) is planned, with a useful hint when no suitable layer is
selected. Where a class chooser is needed, the layer comes first, then the class.

If a control seems to fit two tabs, or none, clarify its task and destination before placing
it. Home is not a catch-all for anything described as “frequently used”. Data exports data;
Publish exports products. A control kept on a tab its purpose doesn't cover, until another tab
is reorganised, is an interim exception: record it in the issue that tracks the move.

### Keep useful shortcuts, not duplicate destinations

- **One button per destination, not per pane.** Buttons that open the same pane at different
  sections are useful task shortcuts. Merge controls that land in the same place with the same
  context.
- **Keep View › Windows complete.** Every pane must be reachable there. Working tabs may repeat
  the panes their tasks need; do not replace those shortcuts with a generic Panes dropdown just
  to reduce the button count.
- **Keep modes with their tools.** Measurement units belong with Measure because they are chosen
  while measuring. Application-wide preferences such as Dark Mode belong in Settings.
- **One name and icon per thing.** Use the same label and icon for the same command wherever it
  appears: ribbon, panes and Command Search. Do not give one tool different names, or expose
  one setting as two differently named toggles.
- **Labels say what happens.** Name a control for what it does, not for a similar feature
  elsewhere. A coordinates-only action should say “By Coordinates”; the “Locate” menu covers
  both coordinates and place/address search. End a label with “…” when it opens a dialog or
  file picker that asks for input.
- **Keep a route to every feature.** Removing a ribbon control must not remove access to its
  action. Name and check the remaining route, including for UI-only actions.

### Choose controls for the workflow

- Put everyday actions and quick adjustments on the ribbon; keep deeper, less frequent options
  in a pane. A pane holds the full set of settings; its ribbon controls are shortcuts to the
  same settings and selected layer or class, not separate state.
- Use a split button only when there is a clear everyday default. The main action stays fixed;
  choosing an alternative never changes it to “last used”. The arrow opens related alternatives.
  Without an obvious default, use a plain dropdown. For example, Select's main action is click
  or box selection; Polygon, Lasso and Radius are in its arrow menu.

For split-button implementation and enabled-state handling, see
[Architecture](architecture.md#everything-is-a-command).

### Check workflows before shipping

Improve the ribbon in small pull requests, tab by tab, rather than replacing the whole layout
at once. Clear naming and duplicate-destination fixes can land independently of a larger
reorganisation.

Check everyday workflows such as adding data, selecting by location, buffering, symbolising
and exporting a map. When moving, removing or regrouping controls, record clicks and tab
switches for these before and after; fewer controls is not an improvement if those tasks become
harder. Check the tab at 1366 px wide with the [`ui_shot` example](#documentation) and look at
the image. Keep everyday controls reachable without horizontal scrolling; move deeper options to
their pane rather than packing more controls into the strip.

A pull request that adds a ribbon control should state:

- the task it serves, its tab and group, and how it fits the tab's purpose;
- whether it runs immediately, selects a tool, opens a dialog or opens a pane, including the
  pane section and layer/class context;
- the fixed default and alternatives if it uses a split button;
- a screenshot of the tab at 1366 px wide.

A pull request that moves, removes or regroups controls should also state:

- where each removed control's action remains reachable, and any Command Search location or
  label updates;
- why any repeated destination is useful in its workflow;
- the workflows checked, with their click and tab-switch counts before and after.

Changing ribbon placement does not require changing command ids or parameters: keep CLI,
shortcuts and scripts working.

## Test data

Generate small datasets during tests instead of committing fixture files. The shared generator
in `tests/support/ogc.rs` builds OGC landing pages, collection lists and GeoJSON in memory;
`tests/support/ogc_server.rs` serves them locally for engine and UI tests. The sorting smoke
test similarly generates its KML, KMZ, CSV and GeoJSON inputs. These run automatically with
`cargo test --workspace --locked` and `cargo xtask ci`, without a separate setup step or a
public service. Extend these generators when adding cases.

## Documentation

The site is built with [Zensical](https://zensical.org):

```sh
pip install -r requirements_docs.txt
zensical serve        # http://localhost:8000
```

Images live in [rustgis-assets](https://github.com/opengeos/rustgis-assets) and are linked by
URL; don't commit images to this repository.

For PR screenshots, GitHub web attachments are fine. The optional
[`gh image` extension](https://github.com/drogers0/gh-image) can automate uploads and return
image links; `gh` itself has no native standalone attachment-upload command.

The screenshots are rendered by the app itself. From the repository root, run
`crates/ui-egui/examples/shots/render.sh`; it needs ImageMagick (`magick`) and network access for
tiles, plugins and the sample DEM. It runs the `ui_shot` scripts beside it and writes WebP files
to `target/shots/`; copy them to `images/` in rustgis-assets. The `assistant` shot asks a real
model, so set `RUSTGIS_ASSISTANT_PROVIDER` and the provider's key first, or it shows an empty
pane (see [AI Assistant](user-guide/assistant.md#choosing-a-model)).

## Releases

1. Bump `version` in the workspace `Cargo.toml` and merge it.
2. Refresh third-party notices if dependencies changed (`cargo xtask notices`).
3. Run the **Release dry run** workflow, which checks the version and builds the desktop/CLI
   packages without publishing anything:
   `gh workflow run release-dry-run.yml -R opengeos/RustGIS --ref main -f version=<version>`
4. When it passes, publish a GitHub release tagged `v<version>` (the tag must match `Cargo.toml`).

The **Release** workflow then builds and attaches the Windows installers, zip and MSIX, the
macOS DMGs, the Linux tarballs, `.deb` and `.rpm` packages and AppImages, and `SHA256SUMS`. To try the packaging locally:

```sh
cargo build --release --locked -p rustgis -p rustgis-cli
packaging/package.sh <version>  # writes dist/ for the host platform
```
