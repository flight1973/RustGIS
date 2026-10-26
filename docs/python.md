# Python

[![PyPI](https://img.shields.io/pypi/v/rustgis.svg)](https://pypi.org/project/rustgis/)
[![Open in Colab](https://colab.research.google.com/assets/colab-badge.svg)](https://colab.research.google.com/github/opengeos/RustGIS/blob/main/apps/rustgis-python/examples/getting-started.ipynb)

The `rustgis` package runs the RustGIS engine inside Python, headless. It opens data and
projects, runs engine commands and geoprocessing tools, styles layers, renders maps and saves
projects that the desktop app opens as they are. There is no app to install and no server to run:
the engine is a native module for Python 3.12 and later. Release wheels cover Windows x64,
macOS Intel and Apple Silicon, and Linux x64 and arm64 (glibc 2.28 or later).

```sh
pip install rustgis               # pip install "rustgis[geopandas]" for GeoDataFrames
```

A [getting-started notebook](https://github.com/opengeos/RustGIS/blob/main/apps/rustgis-python/examples/getting-started.ipynb) walks
through what follows; [open it in Colab](https://colab.research.google.com/github/opengeos/RustGIS/blob/main/apps/rustgis-python/examples/getting-started.ipynb) to try it without installing anything.

## A first session

```python
import rustgis

s = rustgis.Session(sample=True)                    # the World Atlas sample
s.layer_names()                                     # ['Cities', 'Rivers', 'Lakes', 'Countries']
buffers = s.run_tool("buffer", input=s["Cities"], distance=100)
s["Countries"].style("graduated", field="pop_est")
s.zoom_to(buffers)
s.to_png("map.png", width=1600, height=900)
s.save("atlas.rgis")
```

In Jupyter, a session on the last line of a cell shows its map as a picture. For the app itself,
use a `Map`.

## Layers

`s["Cities"]` (or `s.layer("Cities")`) is a `Layer`: one layer of the active map. It is kept by
its id, so it is still the same layer after a rename, and its properties read the session each
time.

```python
cities = s["Cities"]                                # by name (any case) or id
cities.count, cities.type, cities.fields, cities.bounds
"Cities" in s                                       # True
[layer.name for layer in s]                         # every layer, top first

cities.style("proportional", field="pop_max")       # layer.symbology; returns the layer
cities.visible = False
cities.set_transparency(30)
cities.filter("pop_max > 1000000")                  # a definition query; None clears it
cities.rename("Towns")
s.undo()                                            # restore the name "Cities"
cities.zoom_to()
cities.remove()
s.undo()                                            # restore the removed layer
```

Selections work by SQL where clause:

```python
n = cities.select("pop_max > 10000000")            # mode="add", "remove" or "subset"
cities.to_geojson(selected=True)                    # only the selected features
cities.clear_selection()                            # clears selections in every layer
```

## PMTiles archives

`Session.add_pmtiles` and `Map.add_pmtiles` add a raster or vector PMTiles archive by web
address or local file path. `source_layers` chooses the vector source layers; `style` is an
array of MapLibre style layers (or its JSON text), using the same
[supported style properties](user-guide/maps.md#basemaps) as other vector tile layers. A custom
style chooses what it draws through its own `source-layer` properties, overriding `source_layers`.
Omit both for the archive's default look. Other `map.addTiles` parameters, such as `opacity`,
can be passed as keyword arguments.

```python
s.add_pmtiles("https://example.org/atlas.pmtiles", name="Atlas")
s.add_pmtiles(
    "atlas.pmtiles", name="Water", source_layers=["water"],
    style=[{
        "id": "water", "type": "fill", "source-layer": "water",
        "paint": {"fill-color": "#336699"},
    }],
)
```

The result gives the tile layer's name, kind and position; it is not a feature `Layer`.
Web archives need a server supporting byte-range requests (and CORS for the notebook app).
Local archives can be read by Python, but are not embedded in the notebook app: use a web
address accessible to the browser to show one there.

## The app in a notebook

`rustgis.Map` shows the RustGIS app (ribbon, panes, map) in the cell, kept in step with its
session: what Python adds or styles appears in the app, and what you do in the app (an edit, a new
layer, a pan) is back in the session for Python to read. It needs
[anywidget](https://anywidget.dev), and works in JupyterLab, Notebook, VS Code and Colab.

```sh
pip install "rustgis[jupyter]"
```

```python
m = rustgis.Map(sample=True)                         # height="800px"; lang="de" sets the language
m                                                   # the app, in the cell

m.add_geojson("sites.geojson", name="Sites")        # appears in the app
m["Sites"].style("unique", field="name")
m.set_center(10, 50, scale=5_000_000)
m.add_tiles("https://tile.openstreetmap.org/{z}/{x}/{y}.png", name="OpenStreetMap")

m.info()["center"]                                  # where the app's map is now
m.save("sites.rgis")                                # with what was done in the app
m.run_in_app("ui.language", code="de")              # a command only the app has
```

`Map` shares the command, data and view methods of `Session` (`run`, `run_tool`, `add_data`,
`m["Sites"]`, `to_png`, ...); its layers' changes also appear in the app. `m.session` is the
underlying session; `Map(session=s)` displays an existing one. Make changes through `m`, rather
than directly through `m.session`, to send them to the app.

Supported files up to 2 MiB added through `Map` travel with the project. Larger files and formats
that cannot be embedded are read from a path only Python can see: they draw in `to_png` but not
in the app. Use a zip for a Shapefile. The whole project goes across after each change, so
simultaneous changes do not merge: the later one wins. A change made in the app replaces Python's
project, so `m.undo()` undoes only what Python did since.

The app is the [web app](installation.md), loaded from https://web.rustgis.app, so the
notebook needs to be online. `Map(url=...)` or the `RUSTGIS_WEB_URL` environment variable points
it at another copy (a self-hosted one, such as one built with the
[Dockerfile](installation.md#self-host-the-web-app-with-docker), or `trunk serve` in
`apps/rustgis-web`). Settings you change in the widget last until the cell is run again: an
embedded app keeps nothing in the browser.

The project, with the data that travels with it, goes to that app through the browser
(`postMessage`); nothing goes to a server. The app runs in your browser, so with the default
address your data stays on your machine. Point `url=` only at a copy of the app you trust.

## The app in a Qt application

`rustgis.qt.MapWidget` is the same app in a Qt widget, for a desktop tool or a script that opens a
window: it shows the web app in Qt WebEngine and keeps it in step with its session, as `Map` does
in a notebook. It needs PySide6 (or PyQt6 with PyQt6-WebEngine).

```sh
pip install "rustgis[qt]"
```

```python
from rustgis.qt import MapWidget                     # import it before the QApplication
from PySide6.QtWidgets import QApplication

app = QApplication([])
w = MapWidget(sample=True)                           # lang="de", url=... as for Map
w["Countries"].style("graduated", field="pop_est")   # appears in the app
w.show()                                             # or add it to a layout
app.exec()

w.info()["center"]                                   # where the app's map was left
```

`MapWidget` has the methods of `Map` (`run`, `run_in_app`, `add_data`, `w["Sites"]`, `to_png`,
`notices`, ...) and is a `QWidget`, so it goes in a layout or a dock like any other; `w.view` is
the `QWebEngineView` the app runs in. Call its methods from the Qt main thread. What the
[notebook section](#the-app-in-a-notebook) says about files, merging and the app's address holds
here too.

In a conda environment that also has conda's own Qt, PySide6 from pip may find conda's Qt plugins
instead of its own and fail to start ("Could not find the Qt platform plugin"). Use a separate
environment for it, or point `QT_PLUGIN_PATH` at the `Qt/plugins` folder of the PySide6 package
(and `QTWEBENGINEPROCESS_PATH` at its `Qt/libexec/QtWebEngineProcess`).

## Commands

Engine actions are commands with an id and JSON parameters: the same ids the ribbon, the
[command line](cli.md) and the [AI Assistant](user-guide/assistant.md) use.
`Session.run(id, ...)` runs them, with parameters as keyword arguments, a positional dict, or
both (keyword arguments win). It returns the decoded command result and raises
`rustgis.RustGISError` when the command fails. UI-only commands (`ui.*`) need
`Map.run_in_app`, as in the notebook example above.

A command runs to its end before `run` returns, holding Python's interpreter lock: other Python
threads, and calls to the session from them, wait their turn, and Ctrl+C (`KeyboardInterrupt`) is
raised only once the command has ended. So a trend over a data cube (`cube.trend`) keeps the Script
pane's bounds here (2 billion pairs compared, a STAC search of 2,000 items); run a larger one, such
as a trend over a state, with `rustgis-cli run` (see [the command line](cli.md)).

```python
s.commands()                                        # every command, with its parameters
s.tools()                                           # the geoprocessing tools
s.describe("layer.symbology")                       # one command's (or tool's) parameters
s.run("labels.field", layer="Cities", field="name")
s.run("map.goToXY", {"lon": 114.1, "lat": 22.4, "scale": 2_000_000})
s.notices()                                         # drains (text, is_error) pairs since last call
s.undo()
s.redo()
```

`run_tool` runs a geoprocessing tool and returns the layer it made (a list when it made several),
or the tool's own result for one that selects or makes a table. Layers can be given as `Layer`
objects or by name; tools take layers by name, so of two layers with the same name the upper one
is used:

```python
s.describe("buffer")["params"]                      # input, distance, units, dissolve, output
rings = s.run_tool("buffer", input=s["Cities"], distance=50, units="Kilometers")
rings.style("single").set_transparency(50)
```

The common view commands have shortcuts too:

```python
s.set_center(114.1, 22.4, scale=2_000_000)          # bearing= and pitch= tilt and turn it
s.zoom_to(s["Cities"])                              # or a box: (west, south, east, north)
s.bounds                                            # what the map shows now
s.set_basemap("paper")                              # paper, midnight, ocean, atlas, blank
s.add_tiles("https://tile.openstreetmap.org/{z}/{x}/{y}.png", name="OpenStreetMap")
```

The GeoLibre and Whitebox tools run as they do in the app, through the `geolibre` program
(see [Analysis](user-guide/analysis.md)). `rustgis.load_plugins()`, or
`Session(plugins=True)`, loads the [plugins](plugins.md) installed for the desktop app.

## Data in and out

```python
s = rustgis.Session(sample=True, data=["parcels.gpkg"])
roads = s.add_data("roads.fgb", name="Roads")
sites = s.add_geojson({
    "type": "FeatureCollection",
    "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10, 50]},
                  "properties": {"name": "Site A"}}],
}, name="Sites")

gdf = s["Cities"].to_geopandas()                    # a GeoDataFrame in EPSG:4326
big = s.add_geopandas(gdf[gdf.pop_max > 5_000_000], name="Big cities")
s["Cities"].to_geojson()                            # a FeatureCollection dict
big.to_file("big-cities.gpkg", crs="EPSG:3857")     # GeoJSON, CSV, FlatGeobuf, GeoParquet, …
```

GeoPackage exports through `Layer.to_file` or `share.exportLayer` require a new filename.
An existing `.gpkg` is refused to preserve its tables; export does not append or replace tables.

`add_geojson` also takes any object with `__geo_interface__` (a Shapely geometry, a GeoSeries).
`add_data`, `add_geojson` and `add_geopandas` return a `Layer` for a single layer, a list for
multiple layers, or the command result for standalone tables. For example, a CSV without
geometry is a table. Extra `add_data` keyword arguments go to `layer.addData`, such as `layer=`
to choose a GeoPackage table, or `x=` and `y=` to choose coordinate columns.

Every format the app reads works here: GeoJSON, CSV, Shapefile, GeoPackage, GeoParquet,
FlatGeobuf, KML/KMZ, GPX, GeoTIFF and the rest of [Supported Data
Formats](user-guide/data-formats.md). A large file read in place keeps only the features of the
last view in memory; run `layer.loadAll` on it before `to_geojson` or `to_geopandas` to get all
of them.

Open an existing project with `Session(project="atlas.rgis")` or `s.open("atlas.rgis")`.
`s.project_json()` returns its JSON text; `s.run("project.openText", text=...)` opens project
JSON without an intermediate file. `to_png()` returns PNG bytes even when it also writes a
file; omit the path to keep the image in memory.

## From source

The package lives in `apps/rustgis-python`, built with [maturin](https://www.maturin.rs).
Install the [source-build prerequisites](installation.md#from-source) and use a Python 3.12+
virtual environment. From the repository root:

```sh
python -m pip install "./apps/rustgis-python[test]"
python -m pytest apps/rustgis-python/tests
```

For an editable development install, activate the virtual environment, then:

```sh
python -m pip install "maturin>=1.9.3,<2"
cd apps/rustgis-python
maturin develop --release
```

Its version is the RustGIS version, and each release publishes wheels to PyPI.
