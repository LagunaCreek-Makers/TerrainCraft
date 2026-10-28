# TerrainCraft

TerrainCraft turns USGS elevation data into things you can make: 3D-printable terrain tiles, laser-cut stacked layers, and single-sheet engravings (a topographic map or the panorama seen from a chosen spot). It has a command-line tool and a desktop app, both written in Rust.

## Download

Prebuilt Windows, macOS and Linux versions are on the [Releases page](https://github.com/LagunaCreek-Makers/TerrainCraft/releases). Each archive holds:

- `terraincraft-desktop`: the desktop app;
- `terraincraft-cli`: the command-line tool;
- the `assets` and `examples` folders.

Keep `assets` next to the programs, since the app reads its catalogs from it. The builds are not code-signed. On macOS, right-click the app and choose **Open** the first time; on Windows, choose **More info → Run anyway** if SmartScreen warns.

## Build from source

Install Rust from <https://rustup.rs>, then from this folder:

```bash
cargo run --release -p terraincraft-desktop                         # desktop app
cargo run --release -p terraincraft-cli -- examples/demo.csv out 220   # CLI on the demo grid
```

On Linux the desktop app also needs the X11/Wayland development packages listed in `.github/workflows/ci.yml`.

## Command-line tool

```text
terraincraft-cli <input> <output_dir> <bed_mm> [options]
```

`<input>` is a GeoTIFF, an ESRI ASCII grid, a CSV height grid, or `usgs:WEST,SOUTH,EAST,NORTH` to download USGS 3DEP elevation. `<bed_mm>` is your printer bed size; larger models are split into tiles that fit it. Run `terraincraft-cli --help` for every option.

### Common options

```text
  --settings <file.json>      Load print export settings; flags below override it
  --base-mm <mm>              Base thickness under the terrain (default 3)
  --connectors <mode>         none | underside | seam-sockets (default none)
  --seam-dowel-guides         Export seam dowel drilling guides and a printable dowel
  --repair-winding            Fix inconsistent triangle winding before export
  --strict-mesh               Write no tile STLs if any tile fails preflight
  --check-intersections       Run the triangle self-intersection audit
```

A settings file uses the same names, e.g. `{"base_mm": 10, "connectors": "seam-sockets", "strict_mesh": true}`.
Earlier releases read these from `TERRAINCRAFT_*` environment variables; those are no longer read, and the CLI prints a warning if one is set.

Model size: `--width-mm <mm>` (default 400) and `--exaggeration <x>`.
GeoTIFF input (e.g. a USGS 3DEP download) is read with its georeference and printed at **true scale**: depth follows the ground extent and relief is the real vertical scale times the exaggeration (default 1). CSV grids have no ground size, so they use a 4:3 footprint with 25 mm of relief (exaggeration default 2). Nodata samples in GeoTIFF and ASCII grids are filled from their surroundings.

**Download and terrain processing**

- Input `usgs:WEST,SOUTH,EAST,NORTH` downloads USGS 3DEP elevation directly, e.g. `usgs:-121.85,46.8,-121.7,46.9` for Mount Rainier. Large areas are split into several requests on one shared pixel grid and stitched (up to 8192 px per side). `--resolution <px>` sets the long side; by default it targets ~0.5 mm per sample on the model, capped at the ~10 m native resolution. The raw download is saved as `usgs_elevation.tif`.
- `--utm` resamples latitude/longitude data onto a UTM grid; recommended for regions more than about a degree across. Edges are trimmed slightly because the area is rotated by grid convergence in UTM.
- `--smooth <samples>` applies Gaussian smoothing.
- `--sea-level <m>` flattens everything at or below that elevation; `--water-depth <m>` recesses the flattened water so the coastline stays visible at true scale.
- `--dem-only` writes the processed grid as `terrain.tif` and skips the manufacturing files.

Example: `cargo run -p terraincraft-cli -- usgs:-122.6,37.7,-122.4,37.85 out 220 --width-mm 300 --sea-level 0 --water-depth 30 --exaggeration 2`

**3D print output**

- Tiles are exported **north-up** (+x east, +y north). Earlier releases exported a north-south mirror image of the terrain.
- Each tile's row-column number (e.g. `1-2`) is engraved 0.6 mm into its underside, mirrored so it reads correctly when the tile is turned over. `--no-labels` turns this off.
- Meshes are simplified so no elevation sample moves more than 0.05 mm vertically (`--simplify <mm>`, or `--no-simplify` for the full grid). Tile edges are left untouched, so neighboring tiles still meet exactly. A Mount Rainier export shrinks from 26 MB to 6 MB of STL.
- Elevation grids finer than 0.3 mm per sample on the model are resampled before print and laser files are made (`--sample-mm`, 0 keeps the full grid); engravings keep full detail. Tiles are finished in parallel. A 3000 x 2000 grid now exports in about 15 s instead of over 10 minutes, within 0.12 mm (95th percentile) of the full-resolution surface.
- `terrain_tiles.3mf` holds every tile as a named part for PrusaSlicer, OrcaSlicer, Bambu Studio or Cura; `assembly_sheet.svg` is an A4 layout with tile numbers, north arrow, scale bar and map details.

**Laser-cut stacked layers**

- One layer per sheet of material: `--material-mm` (default 3) at the model's vertical scale, the same relief and exaggeration as the 3D print. Layer 1 is the whole map; each layer above is the ground at or above its elevation, with islands as separate parts and lakes as holes.
- Outlines follow interpolated contours (not the raster grid), are cut along the map edge exactly so layers line up at the rim, and never overhang the layer below. Necks narrower than 1.5 mm, islands under 6 x 6 mm and holes under 3 x 3 mm are removed as uncuttable.
- Each part is engraved with the outline of the next layer (where to glue it) and its part number (`L12`, `L12-3` for islands and pieces), hidden where the next layer covers it.
- Cut paths are offset by half the kerf (`--kerf-mm`, default 0.15). Parts larger than the sheet (`--sheet-width-mm`, `--sheet-height-mm`, default 600 x 400) are split into equal pieces, then packed onto sheets.
- Output: `laser_sheet_NN.svg` and `.dxf` (red `CUT`, blue `ENGRAVE` layers), `laser_manifest.csv`, and `laser_layers.svg`, a preview of all layers in place.

**Single-sheet engravings**

- `--engrave-map` writes `engraving_map.svg`: contour lines with heavier, labeled index contours, named features, a coordinate frame (degrees and minutes, or UTM kilometers), scale bar, north arrow and title. Contours stop short of every label.
- `--panorama LAT,LON,BEARING[,FOV]` writes `panorama.svg`: the landscape seen from that spot, with every visible ridgeline (heavier when nearer, with true terrain occlusion, earth curvature and refraction), named summits that are actually in sight, and a compass scale.
- `--place-names` downloads named summits, passes and ridges for the area from the USGS Geographic Names Information System (public domain) and saves them as `place_names.csv`; `--names-file` uses your own CSV or GeoJSON instead. `--title` and `--feet` adjust the text.
- `--map-style stylized` draws an illustrated map: named features as icons from the library (mountains, pinnacles, passes, ridges), fewer unlabeled contours, a double frame and a compass rose. `--font sans|sans-bold|serif|script` picks the typeface.
- Icons come from a bundled library of 1,051 line icons: curated subsets of [Tabler Icons](https://tabler.io/icons) (MIT), [Maki](https://labs.mapbox.com/maki-icons/) and [Temaki](https://github.com/rapideditor/temaki) (CC0), the [National Park Service map symbols](https://github.com/nationalparkservice/symbol-library) (BSD), and TerrainCraft's own landmark icons. The desktop's **Icon library** panel searches them by name or tag, filters by category and shows thumbnails, the license, any required credit, and the sources of landmark icons. A named feature with symbol `icon:ID` (e.g. `icon:maki:volcano`) is drawn with that icon. Every icon's set, version and license is recorded in `assets/icon_library/catalog.json`; license texts are in `assets/icon_library/` (see its `NOTICE`). `tools/build_icon_library.py` rebuilds the library from the upstream packages.
- **Utah landmark list.** `assets/landmark_catalog_utah.csv` lists 26 Utah sites with coordinates and a source link each (USGS GNIS or Wikidata). For now each uses the closest library icon; site-specific artwork is still to come.
- **Tools for landmark artwork**, for making site-specific icons:
  - *Ridgelines from elevation data*: `--ridgeline-icon LAT,LON,BEARING,FOV[,NEAR_KM[,FRONT_KM]]` traces the skyline seen from a viewpoint into a 24 x 24 icon (`ridgeline_icon.svg`), ignoring terrain nearer than NEAR_KM and optionally adding the nearer ridges up to FRONT_KM. Example: `cargo run -p terraincraft-cli -- usgs:-111.7285,40.2857,-111.528,40.4738 out 220 --resolution 1740 --dem-only --ridgeline-icon 40.28957,-111.72257,30,30,4,9` (Mount Timpanogos from Orem).
  - *Building footprints*: `python tools/osm_footprint_icon.py way 32920861 --parts -o capitol.svg` draws an OpenStreetMap building outline and its parts (domes, towers) north-up. Icons made this way are ODbL produced works and need the credit "© OpenStreetMap contributors".
  - Icons added under `assets/icon_library/landmarks/` with a `manifest.json` (license, credit, sources, review state) are picked up by `tools/build_icon_library.py`; the desktop shows their credit and sources and flags unreviewed ones.
- All text is single-stroke vector lines (Hershey fonts, see `assets/fonts/hershey/NOTICE`), so nothing needs fonts or text-to-path conversion. Layers use LightBurn palette colors (black contours, blue index contours, green labels, orange frame) so each can get its own power and speed.

Example: `cargo run -p terraincraft-cli -- usgs:-121.85,46.8,-121.7,46.9 out 220 --engrave-map --place-names --title "Mount Rainier" --dem-only`

## Desktop app

- *Find a place*: search US names (USGS GNIS) and worldwide names (OpenStreetMap Nominatim); results are pinned on the map.
- The Terrain workspace opens on a basemap (OpenStreetMap, OpenTopoMap, USGS Topo or USGS Imagery): drag to pan and scroll to zoom. The download area is the map window, or **double-click to start a selection box and click its opposite corner** (double-click again to go back to the window). Below the map are the area's center and four corners. Orange marks the download area, blue the loaded data. In the Engraving Studio the same map places the panorama viewpoint (click) and direction (drag).
- Projects save every setting, including downloaded place names; older project files still open. **Undo/Redo** (Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z) covers settings but not map navigation; Ctrl+S saves.


## Data sources and licenses

- **Elevation:** USGS 3D Elevation Program (public domain).
- **Place names:** USGS GNIS (public domain).
- **Place search:** OpenStreetMap Nominatim, used within its usage policy.
- **Basemaps:** © OpenStreetMap contributors, OpenTopoMap and USGS The National Map. Each map shows its attribution.
- **Fonts:** Hershey fonts (see `assets/fonts/hershey/NOTICE`).
- **Icons:** Tabler Icons, Maki, Temaki and the NPS map symbols (see `assets/icon_library/NOTICE`).
- **TerrainCraft itself:** MIT licensed.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
python -m unittest discover -s tools -p "test_*.py"
```

CI runs these on every pull request. Pushing a tag such as `v1.1.0` builds release archives for Windows, macOS (Apple silicon and Intel) and Linux and publishes them on the Releases page (`.github/workflows/release.yml`).

Earlier version notes are in [CHANGELOG.md](CHANGELOG.md).
