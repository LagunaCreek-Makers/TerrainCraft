# TerrainCraft Rust — v0.3 migration foundation

Pure Rust workspace (edition 2021), no Python runtime or dependencies. This is the first port of core geometry, **not** a completed port of v0.2.

## Build and test

```bash
cargo test --workspace
cargo run -p terraincraft-cli -- examples/demo.csv output 220
```

### CLI options

```text
terraincraft-cli <input> <output_dir> <bed_mm> [options]

  --settings <file.json>      Load print export settings; flags below override it
  --base-mm <mm>              Base thickness under the terrain (default 3)
  --connectors <mode>         none | underside | seam-sockets (default none)
  --seam-dowel-guides         Export seam dowel drilling guides and a printable dowel
  --repair-winding            Fix inconsistent triangle winding before export
  --strict-mesh               Write no tile STLs if any tile fails preflight
  --check-intersections       Run the triangle self-intersection audit
```

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
- All text is single-stroke vector lines (Hershey Roman Simplex, see `assets/fonts/hershey/NOTICE`), so nothing needs fonts or text-to-path conversion. Layers use LightBurn palette colors (black contours, blue index contours, green labels, orange frame) so each can get its own power and speed.

Example: `cargo run -p terraincraft-cli -- usgs:-121.85,46.8,-121.7,46.9 out 220 --engrave-map --place-names --title "Mount Rainier" --dem-only`

A settings file uses the same names, e.g. `{"base_mm": 10, "connectors": "seam-sockets", "strict_mesh": true}`.
Earlier releases read these from `TERRAINCRAFT_*` environment variables; those are no longer read, and the CLI prints a warning if one is set.


This writes watertight terrain tile STL files and eight **open engraving contour** SVGs. SVGs are **not** closed laser cutting profiles. Do not use them as cut paths without polygonization and validation.

## Implemented
- Validated rectangular elevation grids (CSV CLI)
- Configurable terrain dimensions, base and relief in Rust core API
- Bed-aware, grid-aligned tile segmentation with shared edges
- Closed top/bottom/sidewall triangle meshes and ASCII STL
- Marching-squares contour **line segments** for SVG engraving
- Unit tests (not executed in this environment: cargo/rustc unavailable)

## Still required to replace v0.2
- GeoTIFF/CRS support, reprojection and live USGS 3DEP acquisition
- Closed contour polygons with holes, offset/kerf compensation, material nesting and oversized layer segmentation
- Desktop UI and terrain viewer (recommend Tauri 2 and existing ArtCraft frontend)
- STL import, repair, 3MF and FDM slicer backend
- Assembly registration, joints, labeling, and persistent projects

## Engineering notes
- `relief_mm` maps the DEM's entire min-to-max elevation range to that height before vertical exaggeration; this is a display transformation, not a true physical vertical scale.
- Tile sidewalls are independently closed, not mechanically joined.
- Geographic X/Y scale is not inferred from the CSV: the caller sets physical dimensions.
- Marching squares currently emits segments and does not resolve ambiguous saddle cases into polygons.
- This project has not been compiled here; Rust tooling is unavailable in the current execution environment.

## v0.4: Closed laser-cut outlines and sheet layout

The Rust core now exposes `laser::cut_parts`, `laser::layout`, and `laser::sheet_svg`.
The CLI writes `laser_sheet_NN.svg` and `laser_manifest.csv` alongside STL tiles and contour engravings.
Cut profiles use a DEM-cell staircase approximation; holes are represented as independent closed paths and must be interpreted using even-odd fill in downstream CAM. Sheet packing is a deterministic rectangular shelf algorithm, **not** optimized irregular nesting. Oversized parts are rejected rather than silently clipped. No machine control or G-code is provided.

**Remaining:** native GeoTIFF/USGS, GIS projections, accurate interpolated contour polygons with holes, large contour segmentation, interactive desktop UI, and FDM slicing.

## v0.5 — GIS ASCII grid and bounded laser sections

- New `terraincraft-core::gis::read_asc` parses ESRI ASCII grids (`.asc`), including cell size and lower-left coordinates. It rejects NODATA rather than interpolating silently. The CLI accepts `.asc` as well as CSV.
- `laser::split_oversized` subdivides simple closed outlines into rectangular sections using polygon clipping; the CLI applies this before sheet layout.
- New tests cover ASCII parsing, missing elevations, rectangular segmentation and empty clipping.
- **Not yet implemented:** GeoTIFF decoding, USGS live download, CRS reprojection, accurate interpolated *cut* contours, polygon holes, interlocking registration joints, irregular nesting, desktop UI, and a G-code slicer.
- **Important:** The existing v0.4 cell-derived cut rings can include hole boundaries and are not topology-grouped. Splitting those rings independently does not preserve hole semantics. Treat the laser output as a prototype requiring inspection, not production cutting files.
- Rust compilation remains unverified where `cargo` is unavailable.

## v0.6 — Interpolated contour paths and USGS request foundation

- Added `contours.rs`: interpolated marching-squares contours, saddle-case resolution, stitched open polylines and closed interior rings, plus SVG engraving export. The CLI now uses this output for engraving contours.
- Fixed `.asc` CLI import bug: CSV validation no longer rejects ASCII grids before parsing them.
- Added `gis::usgs_export_url` to construct USGS 3DEP ImageServer GeoTIFF export requests from bounding boxes, EPSG spatial reference and raster dimensions. **Network retrieval and GeoTIFF decoding are not implemented**. Live service URL has not been tested and may require adjustment.
- Existing cell-derived laser cutting profiles are unchanged: **do not use them as production cut paths**. Contour lines are not equivalent to watertight nested plywood-layer polygons with holes. The interpolated path module is a prerequisite to robust topology-aware polygonization.
- Still needed: GeoTIFF decoder/reprojection, polygon boolean operations with hole topology, irregular nesting, registration features, desktop UI, STL import and G-code slicing.
- Cargo/rustc unavailable here, so compilation/tests remain unverified. Run `cargo test --workspace` locally.

## v0.7 — Baseline GeoTIFF elevation import

- Added `terraincraft-core::geotiff::decode(&[u8])` with a dependency-free classic TIFF decoder for **uncompressed, strip-organized, single-band** elevation rasters (8/16/32-bit integers and 32/64-bit floating point). Both byte orders are supported.
- CLI now accepts `.tif` / `.tiff` alongside `.asc` and `.csv`.
- Added a synthetic float32 TIFF decoder unit test and a malformed-header test.
- Unsupported: compressed/tiled/BigTIFF, GeoTIFF georeferencing and coordinate reprojection, nodata tag handling, remote USGS downloading, production-ready laser polygon topology and nesting. The imported raster is treated as an unreferenced elevation grid. Many USGS files use compression and will need a full GeoTIFF backend.
- No Cargo toolchain was available in the build environment: **not compiled or tested**. Run `cargo test --workspace` before use. Laser cutting SVGs remain prototypes, not production-ready cut paths.

## v0.8 — Laser manufacturing preflight and layout report

- Added `inspection.rs`: validates that every laser part has a closed, finite, nonzero-area ring and a unique ID, every part is placed once, sheet margins are respected, and rectangular placement envelopes do not overlap.
- CLI now runs preflight before exporting laser sheets. Invalid placements fail with an explicit error.
- CLI creates `laser_layout_report.html`, a self-contained, offline visual overview of sheets and an assembly placement table. The report contains no external scripts.
- Added three Rust unit tests for valid layouts, collisions, and open rings.
- **Safety:** This is a bounding-box preflight, not polygon-topology validation. The existing staircase outlines, hole handling and clipping remain prototypes; the exported SVGs are **not certified cut-ready**. GeoTIFF compression, USGS downloads, accurate interpolated plywood polygons, and native desktop UI are still pending.
- No Cargo/rustc executable was present in the build environment; compilation and test execution remain unverified.

## v0.9 — Native Rust desktop studio

New `terraincraft-desktop` crate uses **eframe/egui**, a Rust-native UI. No Python, JavaScript, browser, or machine control. It provides CSV/ASC/baseline TIFF path loading, shaded DEM overview, print-bed tile overlays, laser sheet outline previews, controls for elevation exaggeration, dimensions and material settings, and per-workspace STL/SVG export.

```bash
cargo run -p terraincraft-desktop
```

Run from the repository root so the default `examples/demo.csv` resolves. The desktop has a path field rather than a native file picker. The elevation view is **2D shaded relief, not a rotatable 3D mesh**; a full 3D viewer, map region selection, online USGS downloads, and STL import/slicing remain pending. Laser cut polygons and layout are experimental and must be validated in external CAM before production. The UI uses eframe 0.29 and needs platform windowing/OpenGL dependencies. The source has not been compiled in this environment because Cargo/rustc are not installed.

## v1.0 desktop milestone (source-only; build unverified)

The Rust egui desktop now includes a rotatable/zoomable 3D terrain **wireframe** preview in the Terrain tab. Drag to orbit and scroll to zoom. The 3D preview is a sampled heightfield, not a shaded triangle renderer or printable G-code preview.

A JSON project Save/Open workflow preserves the input elevation path, output directory, manufacturing parameters, selected workspace and sheet. On reopening, the original elevation file is loaded again; moving or deleting that file will prevent reopening the project. Projects do **not** embed raster data. Save and Open operate on the `Project JSON path` field.

Run `cargo run -p terraincraft-desktop` from the workspace root. Run `cargo test --workspace` to check all crates. Rust/Cargo are not installed in the build environment, so compilation and GUI execution have not been verified. USGS live download, true map selection, polygon-hole-aware laser geometry, and drag/drop laser nesting remain future work. Machine control remains external.

## v1.1 — USGS geographic bounding-box download (source-only)

- New `terraincraft-core::usgs::Bounds` validates WGS84 coordinates and creates 3DEP ImageServer TIFF requests, with tests for invalid bounds and excessive raster sizes.
- Desktop Terrain workspace adds editable west/south/east/north selection and raster resolution, and a background HTTP download (reqwest/rustls). Downloads are capped at 60 MB and written to `output/usgs_elevation.tif` for project reopening.
- Desktop TIFF loading now uses the Rust `tiff` decoder, including common compressed TIFF variants. Core's older minimal TIFF decoder remains for the CLI.
- Geographic selection is **numeric bounds**, not yet an interactive basemap. No CRS reprojection or precise geodesic sizing: the UI reports approximate longitude/latitude aspect ratio; user must set physical dimensions. NoData rasters are rejected, not silently repaired.
- Remote USGS service compatibility is **not verified**, and no live service call or Rust compilation was performed in this environment. Run `cargo test --workspace` and `cargo run -p terraincraft-desktop` on a Rust-equipped machine.
- Laser polygons remain experimental; not certified for direct cutting. No machine control is included.

## v1.2 — Single-sheet topographic and perspective engraving (experimental)

The new **Engraving Studio** desktop tab exports `topdown_engraving.svg` or
`perspective_engraving.svg`. All terrain computations and SVG generation are Rust.

- Top-down mode generates interpolated vector contour paths at a user-defined
  elevation interval, with editable title text.
- Perspective mode ray-marches the elevation grid from an observer location,
  bearing, pitch, eye height, field of view, and range, and exports the visible
  horizon silhouette as a vector path. This is a **horizon-only** panorama,
  not a full hidden-surface/peak-label renderer.
- User-selected SVG font family, title, subtitle, and basic decorative frame.
- Project JSON saves engraving settings along with the original settings.
- No laser machine control is included.

**Important limitations:** The viewer still displays the generic 3D wireframe,
not the generated SVG. Open exported SVG in Inkscape/LightBurn/LaserCraft to
inspect the actual geometry. Perspective position uses fractional DEM grid
coordinates (0–1), not map-click latitude/longitude. The user must supply
accurate projected raster cell size in meters; GeoTIFF CRS metadata is not
fully processed. Bearing assumes raster rows run geographic north-to-south.
The current perspective export includes only the maximum-elevation-angle
horizon per bearing, not individual overlapping ridges, visible landmark
identification, or automatic cartoon icons. Style presets are preliminary.

Run `cargo test --workspace` and `cargo run -p terraincraft-desktop` on a
machine with Rust installed. Source is not yet compile-verified in this environment.

## v1.3 — DEM summit visibility and automatic candidate labels

- Added `terraincraft-core::peaks` for detecting strict eight-neighbor local maxima and checking their line of sight from the raster-coordinate viewpoint with bilinear DEM sampling and curvature approximation.
- Added `cartography::panorama_with_peaks_svg` to annotate visible candidate peaks with DEM elevation in meters, basic horizontal label spacing, and SVG vector marks.
- The Engraving GUI now offers candidate labels, minimum **local-neighbor height difference** (not true topographic prominence), and maximum label count; settings persist in project JSON.
- **Important:** Labels say *Candidate peak* rather than inventing summit names. The current raster-only coordinate system cannot identify real landmarks or guarantee true compass north. GNIS integration, CRS-aware geographic viewpoint selection, multiple depth-layer silhouettes, cartoon symbols, and production-grade occlusion/label layout remain pending.
- Raster coverage, sampling resolution, nodata handling, and coordinate scale affect visibility accuracy. This feature is an experimental line-of-sight approximation, not a survey-grade viewshed.
- No machine control. Compile/test with `cargo test --workspace` and `cargo run -p terraincraft-desktop` on a machine with Rust installed.

## v1.4 — Geographic viewpoint and multi-depth ridgelines (source-only)

- Added `geoview.rs` for EPSG:4326 north-up geographic bounds, lat/lon-to-DEM pixel selection and latitude-aware pixel scale estimates. Geographic viewpoint mode is restricted to rasters downloaded by the USGS workflow; generic imported TIFF/ASC/CSV files do not establish their CRS or geographic bounds.
- Added `cartography::depth_horizons` and `panorama_depth_svg` to produce 2–8 separate SVG ridgeline groups at increasing distances. The final group is the full-distance horizon; the near groups expose intermediate ridges. This is an **approximate layered horizon visualization**, not a full occlusion-correct perspective surface renderer.
- Desktop engraving controls now include observer latitude/longitude and depth-band count. Existing candidate summit labels are supported in single-band mode only.
- Added unit tests for coordinate mapping, bounds rejection and SVG ridge-band output.
- Still pending: interactive basemap click selection, GNIS official summit names, georeferencing extraction from arbitrary GeoTIFFs, proper multi-depth peak labeling, full surface occlusion, and stylized vector landmark icons.
- Build remains unverified in this environment (Rust/Cargo unavailable). Run `cargo test --workspace` and `cargo run -p terraincraft-desktop` locally.

## v1.5: Geographic selection canvas and user-supplied landmarks

The Engraving Studio has a native Rust geographic coordinate canvas (not a tiled
street basemap). Click to set observer latitude/longitude and drag from the
observer to set bearing; yellow rays show the field of view. This uses the
configured WGS84 bounding box, and geographic viewpoint rendering still requires
a raster downloaded through the USGS workflow.

Top-down engraving accepts a user-supplied landmark CSV with columns
`name,latitude,longitude,elevation_m,symbol`. The symbol field accepts `peak`,
`tree`, `cabin`, and `camp`; see `examples/landmarks.csv`. The program adds SVG
vector symbols and escaped editable text as a separate `landmark-overlays` group.
The imported data is **not** official GNIS data. CSV parsing is deliberately
minimal and does not support quoted commas. A geographically accurate perspective
projection of named landmarks, terrain occlusion and live GNIS queries are **not
implemented**; perspective export with landmark overlay enabled and a CSV set
returns an explicit error rather than placing inaccurate annotations.

The coordinate canvas is not yet a navigable online basemap; USGS live download,
Rust compilation and GUI execution still require verification on a Rust-equipped
machine. Laser cut-path production checks remain necessary.

## v1.6: perspective geographic landmark overlays

The engraving workspace now projects imported CSV landmarks into first-person panoramas when **Use geographic observer coordinates** is enabled and the raster was downloaded using the USGS workflow. Landmark locations are projected by bearing and angular elevation; DEM line-of-sight checks hide occluded points. A basic screen-space collision filter suppresses overlapping labels. User-supplied elevations are identified as supplied values, not certified summit heights. This feature does not yet include GNIS retrieval, online map tiles, or drag-and-drop annotation editing. The current geographic transform assumes a north-up EPSG:4326 raster and is not suitable for arbitrary GeoTIFF projections.

## v1.7 — GeoJSON geographic landmark import (source-only)

- Added a GeoJSON FeatureCollection point importer in `terraincraft-core::landmarks`.
- Reads WGS84 longitude/latitude points and common GNIS-style name/class/elevation property names; non-point features are skipped.
- The engraving studio now accepts either landmark CSV or GeoJSON via the same file path field, detecting the format from content.
- Existing top-down and perspective landmark projection and terrain visibility filtering are reused.
- Added Rust unit tests for GeoJSON parsing and invalid document roots.
- **Important:** This is an *importer*, not a live GNIS API connection or an authoritative verification of imported names. It does not provide a navigable basemap or drag-and-drop label editing.
- **Build status:** Source changed; Cargo and rustc were not available in the current environment, so compilation and tests were not run. Run `cargo test --workspace` locally.

## v1.8: Engraving label editor (incremental)

- New Rust `engraving_editor` module supports positioned, rotated SVG text annotations, with XML escaping and input validation.
- Engraving Studio now includes a simple draggable text placement canvas, label editing, font size and rotation controls, and four typography/color presets (Survey, Minimal, Vintage, Cartoon).
- Custom annotations are serialized with the project JSON and exported into a separate `custom-annotations` SVG group.
- This phase does **not** yet implement a real navigable basemap, direct GNIS downloads, automatic artistic illustration, or live SVG terrain rendering in the editor. Presets currently affect custom annotation typography and stroke color only.
- Text is SVG text, not converted to vector glyph outlines; install matching fonts or convert text to paths in your vector software before laser engraving.
- No Rust compiler was available for this package; run `cargo test --workspace` and `cargo run -p terraincraft-desktop` locally. Compilation and GUI behavior are unverified.

## v1.9 — Live engraving geometry canvas

The engraving canvas now draws actual interpolated DEM contour polylines in top-down mode, or DEM-derived first-person perspective horizon/depth bands in perspective mode. These paths are produced by the same core algorithms used by the SVG exporters. Custom annotations remain draggable over the terrain and persist in the project JSON. The canvas is a vector geometry preview, not a rasterized SVG browser, and automatic named summit labels and imported symbol layers are not yet rendered in the interactive canvas. Exports retain their existing SVG behavior.

The new `terraincraft-core::engraving_preview` module provides reusable preview paths and unit tests. Excessively dense contour settings return an explicit error. Perspective rendering depends on correct DEM pixel spacing and coordinate metadata. This phase does not add machine control, GNIS retrieval, or an online basemap.

Run `cargo test --workspace` and `cargo run -p terraincraft-desktop` on a system with Rust installed. Compilation and runtime validation have not been performed in the authoring environment.

## v2.0 — Binary STL and print assembly manifest

- Print exports from both CLI and desktop now use compact standard **binary STL** rather than ASCII STL. The new Rust-only `print_export` module validates triangle coordinates and rejects zero-area triangles before writing.
- Both print workflows export `print_tile_manifest.csv` containing tile filenames, row/column, assembly origin in millimeters, physical footprint, and DEM cell boundaries. Adjacent tiles intentionally share their boundary sample and each STL uses a local XY origin; place tiles using the manifest to reconstruct the full map.
- Added unit tests for binary STL byte layout, degenerate triangle rejection, and manifest footprint dimensions.
- **Not a slicer:** the generated STL must be imported into a separate FDM slicer. No G-code, toolpaths, supports, print profiles, interlocking joints or registration features are generated. This phase does not add a live basemap or GNIS API.
- Compilation and test execution remain unverified in this environment; run `cargo test --workspace` and `cargo run -p terraincraft-desktop` locally.

## v2.1 — Post-print registration kit

Print exports now also generate `registration_pin.stl`, `registration_manifest.csv`, and
`registration_rXX_cYY.svg` templates for each tile. Each template is **an underside
hole-drilling guide**, at actual millimeter scale, with paired locations on opposite
sides of neighboring seams. Print multiple copies of the cylindrical dowel STL.
Default pin diameter: 4 mm; nominal hole diameter: 4.3 mm; pin length: 8 mm;
recommended drilling depth: 4 mm **into each adjoining tile**. The base should
be thick enough for that drilling depth; increase `base_mm` accordingly.

**Important:** The tile STL geometry does **not** yet contain sockets or tabs.
The SVGs are drill guides for manual post-processing, not laser cut paths for the
terrain. Verify actual printer tolerances with a small fit test and align the
underside orientation before drilling. This is not an integrated snap-fit system.
The API in `registration.rs` exposes configurable dimensions, but CLI and GUI
currently use defaults. Automated printable interlocks remain future work.

## v2.2 — Experimental integrated blind sockets

New `socket_mesh::mesh_with_sockets` replaces individual bottom-face grid cells with
annular, closed blind cylindrical pockets and a pocket roof. The original
post-print drilling templates remain the default. Opt in to integrated sockets:

```bash
cargo run -p terraincraft-cli -- examples/demo.csv out 200 --connectors underside
```

The desktop process recognizes the same environment variable. **Limitations:**
- Default 8 mm pin length needs 4 mm socket depth, so the default 3 mm base
  thickness is insufficient. Increase the base thickness above 4 mm first.
- Each socket must fit *entirely within one DEM grid cell* and no two sockets
  may share a cell. High-resolution DEMs frequently violate this requirement.
  The operation then returns a descriptive error; it does **not** produce a
  misleading or invalid STL. Downsampling or a future adaptive mesher is needed.
- Pockets are blind recesses on the underside, intended for a fixture/assembly
  approach; they are **not** horizontal seam-edge sockets for bridging adjoining
  tiles. Do not assume the included dowels join adjacent tiles without an
  appropriate fixture or redesign.
- This is experimental mesh geometry and has not been verified by a slicer or
  Rust compilation in this environment. Inspect watertightness and fit before use.

## v2.3 — Seam dowel fabrication planning (experimental)

The new `terraincraft-core::seam_joints` module identifies shared **vertical tile walls**, calculates matching horizontal dowel locations, checks that the holes fit within the terrain base, and produces a drilling manifest and 1:1 SVG wall templates. Unlike the earlier underside guides, these guides specify holes **normal to the joining walls**. The hole positions are measured from the minimum-coordinate end of each wall; flip/mirror templates as necessary when physically viewing opposite faces. Use a depth stop and test fit before drilling.

Enable the optional CLI export with a base thick enough for the 3 mm dowels (3.3 mm holes, centered at 3 mm):

```sh
cargo run -p terraincraft-cli -- examples/demo.csv output 220 --base-mm 8 --seam-dowel-guides
```

Files: `seam_dowel_manifest.csv`, `seam_rXX_cYY_<side>.svg`. The CLI still exports ordinary terrain STLs and the existing underside registration system. **This phase does not yet modify the STL seam walls, provide interlocking dovetails, or produce a verified watertight seam-boolean mesh.** The new module includes pairing and base-thickness tests. Build and test with `cargo test --workspace` on a Rust-equipped machine.

## v2.4 — Printable horizontal seam dowels and assembly preflight

- Added `seam_fabrication.rs`: closed, chamfered cylindrical dowel mesh generator; matched-hole geometric preflight; and a simple offline assembly SVG.
- The CLI now exports `seam_dowel.stl` and `seam_assembly.svg` alongside the existing seam drilling guides and manifest when `--seam-dowel-guides` or `--seam-dowel-guides`.
- Each printed dowel spans two adjacent holes of the configured depth. Dowel nominal diameter is smaller than the drill-hole diameter by the configured clearance.
- Preflight detects unpaired holes, mismatched positions, and holes that are too close to each other.
- **Important limitation:** This release does NOT boolean-subtract mating sockets into the vertical terrain tile walls. Drill the sidewalls using the SVG guides before installing the printed dowels. The v2.2 underside experimental sockets are unrelated to these sidewall connectors.
- The assembly SVG is a schematic, not a dimensioned replacement for the wall templates. Test fit on scrap before drilling finished prints.
- Rust compilation and mesh manifold tests must still be run in a Rust-equipped environment.

## v2.5 — Experimental sidewall sockets

Set `--base-mm 10` --connectors seam-sockets` when invoking the CLI to attempt integrated blind horizontal sockets. This mode replaces selected sidewall triangles with circular openings, pocket walls and end caps. It rejects holes crossing individual DEM wall segments, so **dense DEM grids may fail**; resample the DEM or use the existing drilling guides. Sidewall and underside socket modes cannot currently be combined. Mesh manifoldness and slicer compatibility have **not been verified**. This is an experimental geometry implementation, not production-ready printable joinery.

## v2.6 — Mesh topology audit and strict export mode

Every terrain tile exported by the CLI now receives an entry in `mesh_audit.csv` with triangle count, degenerate triangles, unmatched boundary edges, edges shared by more than two faces, and inconsistently oriented shared edges. The default behavior is **report-only**, preserving experimental exports. Set `--strict-mesh` to reject a batch if any tile fails the closed-oriented-manifold audit. The CSV is written before strict-mode failure so you can inspect the problem.

```sh
cargo run -p terraincraft-cli -- examples/demo.csv output 220 --base-mm 10 --connectors seam-sockets --strict-mesh
```

The audit uses exact vertex coordinates and edge incidence. It **does not** prove that the mesh is printable: self-intersections, overlapping coplanar triangles, inside-out disconnected shells, minimum wall thickness and physical connector tolerances still require additional validation. This phase does not remove the DEM-segment socket restriction or implement automatic mesh repair. Tests are included but have not been run in this environment.

## v2.7 — Conservative mesh orientation repair

Set `--repair-winding` to attempt to correct inconsistent triangle winding before binary STL export. The new `mesh_repair.rs` module constructs shared-edge adjacency, traverses connected components, and reverses triangles when necessary to make neighboring faces consistently oriented. It refuses to modify meshes with open boundaries, non-manifold edges, degenerate triangles, or contradictory orientation constraints. The CLI writes `mesh_repair.csv` (repair/refusal and flip count), and continues to write the post-repair `mesh_audit.csv`. Combine with `--strict-mesh` to reject any remaining invalid topology. This repair does **not** fill holes, remove intersections, reconstruct high-resolution seam sockets, guarantee outward-facing normals, or validate printability.

```sh
cargo run -p terraincraft-cli -- examples/demo.csv output 220 --repair-winding --strict-mesh
```

The module has unit tests for reversed faces, already-correct faces and open mesh refusal. Run `cargo test --workspace` in an environment with Cargo installed. Compilation and tests have not been verified in this environment.


## v2.8 — Manufacturing preflight (experimental)

New `manufacturing.rs` computes axis-aligned bounds, signed enclosed volume (mm³),
triangle surface area (mm²), and finite-coordinate checks. The CLI now writes
`manufacturing_report.csv` for every tile alongside `mesh_audit.csv`.
`--strict-mesh` rejects tiles with open/nonmanifold or inconsistent
mesh topology, inward-facing shells (negative signed volume), or invalid extents.

**Limitations:** This is a conservative *review gate*, not printability certification.
It does **not** detect triangle self-intersections, local minimum wall thickness,
trapped voids, or automatically repair high-resolution socket geometry. Signed
volume assumes a closed, consistently oriented surface. Physical fit testing and
Rust compilation are still required.

## v2.9 — Optional triangle crossing preflight

Set `--check-intersections` to produce `intersection_report.csv` for exported terrain tiles. The Rust-only detector tests strict, non-coplanar triangle/segment crossings after bounding-box filtering. It excludes pairs sharing an exact vertex, does **not** resolve coplanar overlaps, and caps expensive pair tests at 250,000 per tile. A report with `complete=false` is **inconclusive**, not a clean bill of health. With `--strict-mesh`, detected crossings and incomplete scans reject the export. This is a diagnostic step, **not** automatic repair or proof of watertightness. For dense meshes a spatial acceleration structure and robust coplanar predicates remain necessary.

Example: `cargo run -p terraincraft-cli -- examples/demo.csv output 220 --check-intersections --strict-mesh`.


## v3.0 — Sweep-and-prune intersection acceleration (source-only)

- Replaced the quadratic all-triangle-pairs broad phase with a sweep-and-prune active set along the largest mesh extent. Spatially separated triangles are discarded without pairwise AABB tests.
- Added `broadphase_candidates` to `intersection_report.csv`, providing visibility into how many potentially overlapping triangle pairs were encountered.
- Retained the narrow-phase limit of 250,000 tested nonadjacent pairs; incomplete scans still fail strict mode. Non-finite coordinates produce an inconclusive result.
- Added tests for 10,000 spatially separated triangles and invalid coordinates, alongside existing crossing and cap tests.
- **Limits:** The active-set implementation may still approach quadratic behavior for heavily overlapping triangle bounds. It is not a BVH, does not handle coplanar overlaps or shared-vertex intersections, and does not reconstruct sockets or repair geometry. Compilation and test execution remain unverified until Cargo is available.

## v3.1 — Sidewall socket preflight and assembly preview

With `--connectors seam-sockets`, the CLI now writes `seam_socket_preflight.csv`
and `seam_socket_preview.svg` before generating STLs. The preview is a plan-view
schematic of tile outlines and connector locations: green markers meet the
current socket-mesher's geometric restrictions; red markers do not. Invalid
sockets stop export with a useful diagnostic instead of failing partway through
an STL export. This **does not** remove the existing single-DEM-wall-segment
limitation or certify watertightness. It is a diagnostic improvement, not a
new Boolean geometry engine. The preview is not a drilling template.


## v3.2 — Coplanar mesh intersection checks

The optional `--check-intersections` audit now detects positive-area coplanar triangle overlaps in addition to non-coplanar crossings. `intersection_report.csv` includes a `coplanar_overlaps` column; overlaps also contribute to the `crossings` total, so existing strict validation rejects them. Three new tests cover contained coplanar triangles, edge contact, and separate parallel planes.

**Limitations:** Faces sharing exact vertices remain excluded from intersection testing, and tangent contacts or numerical edge cases can be missed. Integrated sidewall sockets are still limited by DEM segment width; v3.2 does not implement high-resolution socket reconstruction. Compilation and runtime validation are not claimed.

## v3.3 — assembled 3D inspection export

The CLI now writes `terrain_assembly.obj` and `terrain_assembly.mtl` alongside the tile STLs.
The OBJ positions each tile in the original landscape coordinate system and uses alternating
materials so seams are easy to identify in Blender, MeshLab, or other OBJ viewers. The OBJ
contains the actual post-repair tile meshes exported as STLs, including any enabled experimental
connectors. It is a **visual inspection artifact**, not an interactive in-app 3D viewer, not
a Boolean union, and not a substitute for mesh validation. The existing DEM-resolution
restriction on integrated sidewall sockets is unchanged.

The new `assembly_obj` module has unit tests for offsets and non-finite vertices.
Run `cargo test --workspace` to validate locally; the build has not been verified here.


## v3.4: Native interactive assembly wireframe preview

The **3D Print** workspace now draws a native egui orbitable assembly of the tiled DEM instead of the previous flat tile layout. Adjacent tiles alternate colors; an optional exploded view separates tiles by a configurable millimeter gap. Each tile is labeled by row and column. Drag to orbit, scroll to zoom, and toggle labels or the exploded view. The preview samples terrain heights and tile boundaries; it does **not** display exact exported triangle geometry, integrated socket holes, dowels, or a slicer-ready result. STL/OBJ exports and mesh reports remain unchanged. The new project fields use serde defaults for backwards compatibility.


## v3.5 — Native triangle-mesh assembly preview (experimental)

The Print workspace now offers **Render generated STL triangle mesh**. Unlike the original DEM grid wireframe, this mode calls the same `terraincraft_core::mesh` function used for generating base terrain STL triangles, and draws triangle edges including the underside and sidewalls. Tile-local coordinates are translated into the assembly coordinate system, and exploded spacing, orbit, zoom and tile labels are preserved.

To keep the egui immediate-mode painter usable, the viewer skips faces when a tile exceeds its per-tile edge budget. This is a **wireframe inspection preview**, not a shaded rasterizer. It shows the base terrain meshes and **does not yet apply optional seam socket / dowel modifications**; inspect `terrain_assembly.obj` to see final connector geometry. This version does not implement resolution-independent sockets or automatic mesh repair.

Build/test verification is pending in the current environment because the Rust toolchain is not installed.


## v3.6 — Desktop connector mesh inspection

The 3D Print workspace now offers connector modes **None**, **Underside**, and **Seam sockets**. Enable triangle mesh rendering to inspect the corresponding generated triangles in the native wireframe viewport. The desktop STL export uses the same mesh generation selection and writes `mesh_audit.csv`. Unsupported sidewall socket placements fail preflight instead of silently reverting to a plain tile. The preview counts topology warnings (open boundaries, non-manifold edges, degenerate triangles); it does not repair meshes, provide depth-buffered shading, or guarantee printability. The old CLI environment-variable workflow remains unchanged. Sidewall sockets still cannot cross DEM wall segments.

Build/test (requires Rust toolchain): `cargo test --workspace` and `cargo run -p terraincraft-desktop`.


## v3.7 — Socket placement collision preflight

The seam-socket preflight now detects multiple planned sockets occupying the same DEM wall segment. Previously, this conflict could pass preflight and fail only while building a tile's mesh, potentially after other files were written. Both conflicting placements are now marked incompatible in `seam_socket_preflight.csv` and red in `seam_socket_preview.svg`. Two regression tests check colliding and independent sockets; a mesher test confirms that the same collision is rejected during geometry generation.

**Scope:** This is a validation improvement, not the promised resolution-independent sidewall reconstruction. A socket crossing a DEM segment boundary is still rejected. The code has not been compiled or run in this environment; run `cargo test --workspace` before use.


## v3.8 — Additional socket opening clearance checks

The seam preflight now explicitly checks the distance between socket-opening circles on each tile wall, including their vertical offsets. It flags conflicting openings before export and includes the reason in the preflight CSV. The existing single-DEM-segment limitation remains; this is a safety validation increment, **not** a resolution-independent Boolean socket engine. Run `cargo test --workspace` to validate on a machine with Rust installed.

## v3.9 — Sidewall socket reconstruction across DEM cells (experimental)

The sidewall socket generator now replaces a **complete sidewall** with a triangulated
polygon containing circular holes, rather than requiring every socket to fit in one
DEM cell. Original terrain-edge and base-edge vertices are preserved to maintain
connections to adjacent terrain surfaces. Each socket has a cylindrical inner wall
and a blind end cap. Multiple non-overlapping sockets on a wall are supported.

This change uses the `earcutr` Rust triangulation crate. Preflight now accepts
sockets that span multiple raster cells, while still rejecting sockets outside the
base, too deep for the tile, or overlapping other sockets. Run with:

```bash
cargo run -p terraincraft-cli -- examples/demo.csv output 220 --base-mm 10 --connectors seam-sockets --strict-mesh --check-intersections
```

**Unverified:** Rust compilation and mesh topology checks have not been run in the
packaging environment. The new algorithm is experimental. Strict topology checks
should be used and exported models should be inspected in a slicer before printing.
Automatic repair of missing faces, comprehensive self-intersection detection,
minimum wall thickness checks, and a shaded 3D viewer are still outstanding.

## v4.0 — Correct sidewall reconstruction indexing and topology regressions

Corrected a critical v3.9 sidewall replacement bug: the base mesh stores north/south
and west/east wall triangles **interleaved by DEM cell**. Replacing contiguous
triangle ranges removed portions of unrelated walls. The generator now retains
all terrain/underside triangles and rebuilds the four sidewall sections independently,
using triangulated socket patches only on selected walls.

Added regression assertions for closed, consistently oriented manifold geometry
with sockets crossing many DEM cells, two separated sockets, and sockets on all
four sides. Fixed an outdated preflight test assertion to match its actual
"overlapping socket openings" diagnostic.

These tests are **included but not executed** in this environment (no Rust
compiler installed). This is a targeted correctness fix, not a full Boolean
mesh repair engine. Real printing requires successful Cargo tests, mesh audit,
intersection checking, slicer inspection, and fit calibration.


## v4.1 — Fail-closed connector exports and strict preflight

- Integrated seam-socket mesh generation now checks for a closed, consistently wound manifold **before returning geometry**. Invalid connector meshes produce an error instead of a printable STL.
- The CLI now generates and audits **all** terrain tiles before writing any tile STL. With `--strict-mesh`, a failed preflight writes diagnostic CSVs and exits without writing terrain STLs.
- Existing STLs in a reused output directory are **not deleted**. Use a new/empty output directory for a strict manufacturing run.
- This is not a mesh Boolean repair engine. Socket triangulation, self-intersection detection, printer fit, and GUI rendering still need compiled and physical verification.

Example:

```sh
cargo run -p terraincraft-cli -- examples/demo.csv output_v4_1 220 --base-mm 10 --connectors seam-sockets --strict-mesh --check-intersections
```

## v4.2 — shaded desktop assembly view (experimental)

In **3D Print**, enable **Render generated STL triangle mesh**, then enable **Shaded solid triangles (depth sorted)**. The viewer renders triangle-filled faces from the same connector-aware mesh generator used for STL export, uses simple directional diffuse lighting and painter-order depth sorting, and retains orbit, zoom, exploded spacing, and alternating tile colors. For responsiveness it samples a maximum of roughly 32,000 faces (this is not a precise per-pixel Z-buffer). Dense or intersecting meshes can show sorting artifacts, and the view is not a printability guarantee. No GPU backend or machine control was added. Rust build and GUI behavior have not been tested in this environment.


## v4.2 — shaded assembly viewer

The desktop **3D Print** panel now supports **Shaded solid mesh (depth-sorted)** when **Render generated STL triangle mesh** is enabled. The preview renders the generated tile triangle geometry, including the selected experimental connector cavities, with per-face directional diffuse lighting and alternating tile colors. Mouse orbit, zoom, exploded spacing and tile labels remain available.

The renderer uses egui's CPU-side polygon painter and a painter's-algorithm depth sort, rather than a hardware z-buffer. Overlapping/intersecting triangles may sort incorrectly; faces are sampled on large models to maintain responsiveness. This is a visualization aid, not a substitute for slicer inspection or geometry validation. Build and GUI operation have not been verified in this environment.

## v4.3 experimental GPU-accelerated assembly preview

The 3D Print panel includes **GPU accelerated OpenGL solid preview (experimental)**. It uploads the actual generated tile triangles (including selected connector geometry) to an OpenGL vertex buffer and shades them in a vertex/fragment shader. A 24-bit hardware depth attachment replaces CPU painter-order triangle sorting and the 60,000-face preview cap. Mouse orbit, zoom, tile colors and exploded assembly positions remain available. Requires an OpenGL 3.3-compatible driver and the eframe `glow` backend. The legacy egui wireframe/shaded renderer remains as a fallback.

This path is an initial GPU integration and has **not been compiled or run** in this environment. GPU resources are currently recreated each frame, and the callback's framebuffer compositing needs platform validation. Do not assume performance gains or cross-platform compatibility until profiled and tested.


## v4.4: Engraving icons and international map provider groundwork

- `assets/icons/`: 240 **original monochrome vector pictograms**, with CC0 metadata in `assets/icon_manifest.json`. These are generic templates, **not** 240 verified landmark-specific images for each jurisdiction.
- `assets/landmark_catalog_template.csv`: 50 unpopulated ranked slots each for six example jurisdictions. Empty rows are deliberately not represented as verified landmarks.
- `terraincraft-core::icon_library`: procedural icons and an SVG overlay API, with desktop size and placement controls.
- `terraincraft-core::map_providers`: configurable XYZ URL generation for international providers, plus desktop source selection and copy-URL controls. This is **not yet** an embedded raster tile map or WMS/WMTS client.
- Attribution, caching, rate limits and API keys must be honored for any tile fetching implementation. OSMF prohibits bulk tile downloading on its public servers.

### Still required
Curate and verify top-50 landmark lists for each jurisdiction, source/create 200+ distinctive landmark icons *per* jurisdiction, review icon provenance and licensing, implement provider-specific tile retrieval/cache and actual viewport rendering, WMS/WMTS, and run Cargo and UI tests.


## v4.5: Catalog import and additional tile addressing

- Fixed malformed Rust syntax in the procedural icon generator (previous v4.4 release).
- Added `landmark_catalog::parse_catalog` and `filter_jurisdiction` for jurisdiction-specific, ranked landmark metadata. Blank template rows are ignored and do not count as landmarks. CSV currently requires simple comma-separated fields without quoted commas.
- Added `map_providers::tms_url`, `xyz_to_tms_y`, and `quadkey` for TMS and Bing-style addressing. These generate addresses only; no provider account, fetching, caching, or map display is implied.
- Unit tests added; Cargo compilation and GUI tests have not been executed in this environment.
- OpenStreetMap public tile infrastructure must not be bulk downloaded; honor attribution and caching policies.


## v4.6 — Jurisdiction landmark browser and geographic engraving placement

The Engraving Studio can load a populated landmark CSV, filter by jurisdiction and verification status, browse ranked landmarks, select an observer location, and place **verified** catalog pictograms on a top-down engraving using the selected WGS84 USGS bounding box. Bundled `generic-001` through `generic-240` identifiers map to the 240 original pictograms; unknown IDs are skipped rather than misrepresented. Geographic placement requires accurate `west/south/east/north` values and a north-up georeferenced raster. Perspective automatic icon placement is not yet supported. The bundled catalog is still an unpopulated template, not a verified top-50 collection.

OpenStreetMap Standard tiles must be used only for interactive viewing, with visible attribution, identifiable requests, compliant caching, and no bulk prefetch. This release does not add network tile downloading.


## v4.7: On-demand international raster tile preview

Engraving Studio now has a map tile zoom control and a **Load map tile at observer** button.
The selected XYZ source is fetched asynchronously, decoded and displayed as an egui texture.
Only one explicitly requested tile is downloaded, with a seven-day local cache and an identifying
HTTP User-Agent. The map attribution remains visible alongside the preview. The cache is stored
in the operating system's temporary directory. This is a single-tile preview, **not** a full
pannable map, WMS/WMTS client, or offline map downloader. Provider licensing and access terms apply.

Build: `cargo run -p terraincraft-desktop`. Cargo/runtime verification has not been performed.

### v4.8 interactive map navigation (experimental)
The Engraving Studio map canvas now supports drag-to-pan, scroll zoom, click-to-position observer, and two Shift-clicks to set the elevation/terrain bounding box. Web Mercator conversion functions have round-trip tests. The map remains a **single tile stretched to the canvas**, not yet a seamless multi-tile map; the center tile is requested on navigation, with the existing 7-day disk cache. Compilation and graphical runtime testing have not been completed.


## v4.9: Geographic multi-tile map viewport (experimental)

The Engraving Studio map now composes adjacent 256-pixel XYZ tiles in their correct Web Mercator positions, wraps tile columns across the antimeridian, and clips image drawing to the map canvas. Only tiles intersecting the viewport are requested. Asynchronous HTTPS requests use a seven-day disk cache keyed by provider URL and tile coordinates. Up to 12 previously unseen tiles are scheduled per viewport update; already requested tiles are not retried until application restart. The previous single tile stretched across the entire viewport is no longer used.

**Limitations:** Rust compilation and UI tests are unverified; network requests are currently spawned independently per tile and have no global rate limiter. Providers may have stricter rate limits or licensing terms. Texture storage grows during the session; eviction and request cancellation are still required. Landmark marker overlays are not implemented.

## v5.0: interactive landmark markers

The multi-tile map can now display catalog landmarks as high-contrast point markers.
Choose a catalog path and jurisdiction, enable markers, and click a marker to select
its observer coordinates and associated bundled engraving icon. Verified-only filtering
and a configurable marker cap are available. The GPU texture cache is capped by
clearing it when it exceeds 192 tiles; cached tiles are loaded again on demand.
This is a simple safeguard, not an LRU cache or a request scheduler.

**Limitations:** Catalog CSV is re-read when the map is drawn; it does not support
quoted commas. Marker selection does not automatically add an annotation to the SVG.
The new code has not been compiled or runtime tested in this environment.

## v5.1: US state cultural engraving icons

`assets/state_cultural_icons.json` catalogs six broad cultural themes for each of the 50 US states (300 state-tagged entries). The six baseline motifs per state are generic placeholders, **not 300 distinct state-specific drawings**. Six additional Utah-specific original single-color SVGs depict a seagull, covered wagon, salt shaker, simplified Salt Lake Temple, and unofficial U- and B-letter academic motifs. The academic motifs are **not** reproductions of official University of Utah or BYU logos.

In Engraving Studio, set the jurisdiction (e.g. `US-UT`) and expand **US state cultural icons** to select an SVG. The selection is used in exported single-layer engraving. Icon positions and dimensions use the existing controls. Source/license metadata are in the JSON manifest. The icons are illustrative and do not assert that a theme is the official state symbol.


### v5.2: Sports engraving category (all 50 US states)
Six original single-color SVG sports pictograms are bundled: football, skiing, whitewater rafting, fishing, duck hunting, and hunting. Each state has six sports entries in `assets/state_cultural_icons.json` (300 entries total). The images are reusable activity icons, **not state-specific teams, official logos, or assertions about local sports popularity**. The desktop's existing state cultural icon selector includes the sports category and uses the same single-layer engraving placement controls. SVG artwork is in `assets/sports_icons/`. Run `cargo test --workspace` to verify on a Rust-equipped machine.

### v5.3 — Regional sports engraving expansion

Added 15 original monochrome SVG sports illustrations (powder skiing, red-rock biking, fly fishing, whitewater kayaking, surfing, salmon fishing, hockey, rodeo, horse racing, basketball, baseball, sailing, rock climbing, golf, snowboarding). Added one curated regional sports entry for each of the 50 states, while retaining six shared sports symbols per state. These are editorial illustrations, not official state sports, university logos, or licensed team marks. Selection works through the existing state cultural icon browser and SVG export. Unit tests cover state coverage and asset resolution. Cargo tests and desktop runtime have not been run in this environment.


## Institution and professional sports logos (v5.4)

The Engraving Studio now has a jurisdiction-filtered college, university, and professional-team logo browser. `assets/institution_logo_catalog.json` includes sample named organizations and empty catalog slots for all 50 US states, plus several international examples. **No official logos are bundled.** These names are discovery metadata, not claims of trademark licenses.

To add artwork, supply a local, monochrome, self-contained SVG file inside the catalog directory (or its subdirectories), set `svg_file` to its relative path, and record `source_url`, `license`, `rights_holder`, and `permission_notes`. Select the institution in the browser to use the SVG in the single-layer engraving export. Records without artwork fall back to the existing icon.

The SVG loader rejects external references, scripts, embedded images, animation, traversal, and files over 500 KiB. This is a conservative check, **not** a complete SVG sanitizer; review imported artwork before use. Trademarks and official team/college logos may require permission, especially for commercial products. The software does not download or imply authorization to use logos.

## v5.5 — Named temples, ski resorts and cathedrals

The shared `assets/institution_logo_catalog.json` now contains curated **named entries** for LDS temples, ski resorts and cathedrals in several countries and US states. In **Engraving Studio → Temples, ski resorts and cathedrals**, filter by jurisdiction (such as `US-UT`, `FR`, `GB-ENG`) and search by name. The selected icon is exported using the existing single-layer SVG overlay mechanism.

**Scope and accuracy:** This is **not** a complete inventory of every LDS temple, ski resort or cathedral worldwide. Entries have not been live-verified for operational status or naming. The three bundled SVGs under `assets/site_icons/` are **original, generic category symbols**, reused for each matching place; they are **not** distinctive architectural portraits or official emblems. Additional records can be added to the JSON catalog. For a true likeness, replace an entry's `svg_file` with authorized original or licensed monochrome artwork and record its rights information.

The Rust workspace has not been compiled in this environment; test locally with `cargo test --workspace` and `cargo run -p terraincraft-desktop`.


## v5.6 — Engraving styles and architectural view assets

The `assets/icon_variants/` directory contains 801 algorithmically derived SVG files (simplified, fine-outline, bold) from the 267 base SVGs with a vector group, plus three original schematic Salt Lake Temple perspectives (front, side, corner). The original icon collection remains intact. The generated variants are *stylistic treatments*, not new viewpoints or verified building elevations. The simplified variants remove some secondary shapes; check legibility before engraving.

Engraving Studio offers an icon-style selector that applies vector stroke variants during SVG export, and selects the three distinct Salt Lake Temple drawings when the Salt Lake Temple cultural icon is active. A manifest records the source icon, style, generated file and derivation. Official sports-team logos are not bundled.

Compilation and UI testing remain unverified in this environment.

## National Park Service National Historic Landmarks (NHL) synchronization (development)

The official NPS state list is at https://www.nps.gov/subjects/nationalhistoriclandmarks/list-of-nhls-by-state.htm .

To create a one-to-one *catalog entry and pair of generic monochrome SVG placeholders* for every successfully parsed NPS entry:

```bash
python -m pip install beautifulsoup4
python tools/sync_nps_nhl.py --output assets/nps_nhl
# Offline alternative: download the NPS page yourself, then:
python tools/sync_nps_nhl.py --html /path/to/nps-nhls.html --output assets/nps_nhl
```

The sync fails closed if fewer than 1,000 records are parsed (adjust `--minimum` only after inspecting the page and parser). Review `coverage.json` state counts against the NPS page before asserting complete coverage. A generated SVG is a **category-level placeholder**, not an architectural depiction of the named site; `artwork_status=generic-placeholder` makes that explicit. No sensitive/restricted archaeological coordinates are exported. Only names and state jurisdictions are included. Do not use this catalog to infer public access or permission to visit a property. The script does not redistribute NPS images or third-party photographs.

Run the parser's offline tests with `cd tools && python -m unittest -v test_sync_nps_nhl`.

**Current release limitation:** The official page was accessible for review, but outbound HTTP downloads from this build container were blocked, so the full NPS list has not yet been fetched, populated, or audited here. Run the sync on a machine with internet access and review its coverage report. Existing TerrainCraft SVG artwork is unchanged.

## UNESCO World Heritage List (v5.8)

TerrainCraft includes an official-source UNESCO World Heritage synchronization tool:

```bash
python tools/sync_unesco_whc.py --output assets/unesco_whc
# Or from a downloaded official UNESCO XML file:
python tools/sync_unesco_whc.py --xml world_heritage.xml --output assets/unesco_whc
python -m unittest discover -s tools -p 'test_sync_unesco_whc.py'
```

The tool reads `https://whc.unesco.org/en/list/xml/`, uses the UNESCO property ID to deduplicate transboundary sites, and creates `catalog.csv`, `catalog.json`, `coverage.json` and two single-color SVGs per property. The desktop Engraving Studio has a **UNESCO World Heritage icons** browser using the JSON catalog; search by site or country and select an engraving icon. A failed or incomplete fetch does not generate a misleading catalog. This source archive does **not** include the live UNESCO catalog because the build environment cannot fetch the XML; run the sync to populate it.

**Artwork accuracy:** generated SVGs are explicitly category-level placeholders (cultural, natural, mixed), **not** site-specific likenesses. The library does not yet contain verified perspectives and simplified depictions of every UNESCO property. The data is attributed to UNESCO; review the official data's licensing and attribution requirements before redistribution. The source metadata remains separate from user-supplied or original artwork.


## v5.9 UNESCO curated artwork workflow

The UNESCO sync can now merge curator-created site-specific SVG illustrations with
category placeholders. Put reviewed files in `assets/unesco_curated/` using
`unesco-<UNESCO ID>.svg` and optionally `unesco-<UNESCO ID>--simplified.svg`.
Then run `python tools/sync_unesco_whc.py --curated assets/unesco_curated --output assets/unesco_whc`.
A coverage report distinguishes curated artwork from placeholders. The desktop
UNESCO browser shows the curated count and offers a simplified icon toggle.
The script rejects scripting, external references and other unsafe SVG constructs
in curated artwork. Curators must independently verify visual accuracy and rights.
**No site-specific artwork is generated automatically by this phase.**

## v6.0 regional cultural artwork expansion

The `assets/regional_icon_catalog.json` catalog includes **50 US states and 67 countries**,
with four **original, monochrome SVG compositions** per jurisdiction (468 new SVGs):
original, simplified, profile composition, and three-quarter composition. These are
**thematic graphic variations**, not accurate architectural elevations or actual
camera viewpoints. Regional associations are editorial suggestions, not official
symbols, endorsements, or comprehensive national cultural inventories.

In Engraving Studio, set the jurisdiction to `US-UT`, `JP`, `FR`, etc., then
open **Regional cultural illustrations — states and countries**. Select the
motif and composition. Artwork is read from `assets/regional_icons` relative
to the current working directory. Keep that directory with the application.
The original state cultural catalog, UNESCO and NPS importers remain available.

A full jurisdiction-by-jurisdiction, landmark-specific perspective library
requires accurate reference photography/architectural research and artwork
verification; these new compositions do not substitute for that work.

## v6.1 — Cross-release SVG and per-site coverage audit

The complete archived source comparison (v4.6 through v6.0) found **no lost SVGs**:
270 SVGs in v5.5, 1,074 in v5.6, and 1,542 in v6.0. The additional
v5.6 files are mostly automated line-style variants; v6.0 adds thematic
regional compositions, not verified views of individual sites.

Run `python tools/audit_icon_coverage.py` to produce:

- `assets/coverage_audit/coverage_summary.json`: counts by asset directory,
  validity, catalog collection, missing artwork and simplified variants.
- `assets/coverage_audit/site_coverage.csv`: every currently populated catalog
  entry with its artwork status and presence checks.

**Do not treat styles or rotations as architectural perspectives.** Only
verified, site-specific drawings with documented viewpoint/provenance may be
counted as front, side, corner or aerial views. The coverage report currently
credits **zero verified multi-view site sets** because existing schematic
illustrations lack verified provenance. It also reports missing institution
logo assets, and does not claim UNESCO/NPS completeness until their official
catalog sync jobs have actually populated the corresponding CSV files.

To populate the public heritage catalogs on an internet-enabled workstation:

```sh
python tools/sync_unesco_whc.py --output assets/unesco_whc
python tools/sync_nps_nhl.py --output assets/nps_nhl
python tools/audit_icon_coverage.py
```

These importers initially produce **generic category placeholders**, not
site-specific artwork. Artwork for each individual building must be drawn or
licensed and reviewed against references for the requested perspectives.

## v6.2 — Embedded SVG provenance and missing-artwork previews

Run `python tools/enrich_svg_metadata.py` to embed a machine-readable TerrainCraft metadata element, SVG title, and description in **every** SVG. The `associated_sites` array records all catalog associations (including shared icons), jurisdiction, license/source when present, and whether the illustration is generic or thematic. The metadata explicitly sets `perspective_verified=false`; it does not claim an algorithmically generated variant is a real architectural view. Re-running the script replaces its own metadata rather than duplicating it.

The script also creates generic monochrome fallback preview SVGs for the 120 institution/team entries that have no licensed logo. These appear under `assets/unlicensed_placeholders/` and are referenced through `preview_svg_file`, while `svg_file` intentionally stays blank: the generic preview is **not** an official team/university logo. Full live UNESCO and NPS inventories, accurate landmark-specific drawings, and licensed official logos are **not** included; their absence cannot be solved by renaming generic icons. The report `assets/svg_metadata_summary.json` distinguishes SVG coverage from those outstanding needs.

Validate with `python -m unittest discover -s tools -p 'test_enrich_svg_metadata.py' -v` and `python tools/audit_icon_coverage.py`. Rust compilation remains unverified.

## v6.3 — Curated UNESCO landmark illustrations (initial batch)

`assets/unesco_curated/` now includes **24 original, site-specific stylized SVGs** for six UNESCO properties: Taj Mahal (252), Great Wall (438), Acropolis (394), Petra (320), Machu Picchu (274), and Stonehenge (373). Each has independently drawn `front`, `side`, `three-quarter`, and `simplified` compositions. These are schematic interpretations, **not verified architectural elevations**, and the perspective flags are intentionally false. Source links and explicit site IDs are embedded in SVG metadata. `curated_catalog.json` provides the variant mapping.

Run `python tools/build_curated_unesco.py --output assets/unesco_curated` to regenerate; `python tools/enrich_svg_metadata.py --assets assets` to refresh the complete library's searchable metadata; `python -m unittest discover -s tools -p 'test_*.py'` to validate. To use curated standard and simplified drawings in the UNESCO XML import, pass `--curated assets/unesco_curated` to `tools/sync_unesco_whc.py`. The side and three-quarter drawings are stored in the curated catalog for later desktop perspective-selection integration.

**Coverage limitation:** Six curated sites are only a first batch. Full UNESCO/NPS syncs have not been performed, and the rest of those sites lack site-specific drawings. No official logos or site photographs are included.
