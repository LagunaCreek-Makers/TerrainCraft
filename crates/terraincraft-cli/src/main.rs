use clap::Parser;
use std::{
    fs,
    path::{Path, PathBuf},
};
use terraincraft_core::engrave::{self, MapOptions, MapStyle, PanoramaOptions, PanoramaView};
use terraincraft_core::export_settings::{ConnectorMode, PrintExportSettings};
use terraincraft_core::georef::CrsUnits;
use terraincraft_core::gis::read_asc;
use terraincraft_core::landmarks::{read_landmarks, to_csv, Landmark};
use terraincraft_core::stack::{self, StackSettings};
use terraincraft_core::stroke_font::Font;
use terraincraft_core::terrain_ops::{prepare, TerrainPrep};
use terraincraft_core::usgs::Bounds;
use terraincraft_core::usgs::LANDFORM_CLASSES;
use terraincraft_core::{mesh, tiles, Dem, TerrainSettings};

/// Generate 3D-print terrain tiles, laser sheets and contour engravings from an elevation grid.
#[derive(Parser)]
#[command(
    version,
    after_help = concat!(
        "CSV rows must contain numeric elevations; no header. Demo input: examples/demo.csv\n",
        "Download from USGS 3DEP with an input of usgs:WEST,SOUTH,EAST,NORTH in degrees,\n",
        "e.g. usgs:-121.85,46.8,-121.7,46.9 (Mount Rainier)."
    )
)]
struct Args {
    /// Elevation input: .csv heightmap, .asc ESRI ASCII grid, GeoTIFF, or usgs:W,S,E,N
    input: PathBuf,
    /// Directory to write outputs into (created if missing)
    output: PathBuf,
    /// Square print bed size in millimeters
    bed_mm: f64,
    /// JSON file of print export settings; command-line flags override it
    #[arg(long)]
    settings: Option<PathBuf>,
    /// Base thickness under the terrain, in millimeters [default: 3]
    #[arg(long)]
    base_mm: Option<f64>,
    /// Alignment features cut into each tile: none, underside or seam-sockets
    #[arg(long)]
    connectors: Option<ConnectorMode>,
    /// Also export seam dowel drilling guides, manifest and printable dowel
    #[arg(long)]
    seam_dowel_guides: bool,
    /// Attempt to fix inconsistent triangle winding before export
    #[arg(long)]
    repair_winding: bool,
    /// Write no tile STLs if any tile fails manufacturing preflight
    #[arg(long)]
    strict_mesh: bool,
    /// Run the triangle self-intersection audit on every tile
    #[arg(long)]
    check_intersections: bool,
    /// Don't engrave tile numbers into the underside
    #[arg(long)]
    no_labels: bool,
    /// Simplify meshes so no surface sample moves more than this many mm [default: 0.05]
    #[arg(long)]
    simplify: Option<f64>,
    /// Keep the full grid mesh
    #[arg(long, conflicts_with = "simplify")]
    no_simplify: bool,
    /// Resample elevation finer than this many mm per sample on the model before making
    /// print and laser files (0 keeps the full grid) [default: 0.3]
    #[arg(long)]
    sample_mm: Option<f64>,
    /// Model width in millimeters
    #[arg(long, default_value_t = 400.)]
    width_mm: f64,
    /// Vertical exaggeration [default: 1 for georeferenced input, else 2]
    #[arg(long)]
    exaggeration: Option<f64>,
    /// Pixels along the longer side for usgs: downloads [default: about 0.5 mm per
    /// sample on the model, capped at the ~10 m native resolution]
    #[arg(long)]
    resolution: Option<u32>,
    /// Resample latitude/longitude data onto a UTM grid (recommended above ~1 degree)
    #[arg(long)]
    utm: bool,
    /// Gaussian smoothing radius in samples
    #[arg(long, default_value_t = 0.)]
    smooth: f64,
    /// Flatten elevations at or below this level in meters (0 = sea level)
    #[arg(long)]
    sea_level: Option<f64>,
    /// Recess flattened water this many meters below the sea level
    #[arg(long, default_value_t = 0.)]
    water_depth: f64,
    /// Laser material thickness in mm; one layer per thickness at the model's vertical scale
    #[arg(long, default_value_t = 3.)]
    material_mm: f64,
    /// Laser kerf width in mm
    #[arg(long, default_value_t = 0.15)]
    kerf_mm: f64,
    /// Laser sheet width in mm
    #[arg(long, default_value_t = 600.)]
    sheet_width_mm: f64,
    /// Laser sheet height in mm
    #[arg(long, default_value_t = 400.)]
    sheet_height_mm: f64,
    /// Write a single-sheet topographic map engraving (engraving_map.svg)
    #[arg(long)]
    engrave_map: bool,
    /// Write a first-person panorama engraving from LAT,LON looking toward BEARING
    /// degrees, with an optional field of view (default 90): LAT,LON,BEARING[,FOV]
    #[arg(long, value_name = "LAT,LON,BEARING[,FOV]")]
    panorama: Option<String>,
    /// Download named summits, passes and ridges for the area from USGS (GNIS) and label
    /// them on engravings; saved as place_names.csv
    #[arg(long)]
    place_names: bool,
    /// Label engravings with names from a CSV or GeoJSON file instead
    #[arg(long, conflicts_with = "place_names")]
    names_file: Option<PathBuf>,
    /// Title engraved on the map or panorama
    #[arg(long, default_value = "")]
    title: String,
    /// Show elevations and contours on engravings in feet
    #[arg(long)]
    feet: bool,
    /// Map engraving style: exact (survey) or stylized (illustrated, with icons)
    #[arg(long, default_value = "exact", value_parser = ["exact", "stylized"])]
    map_style: String,
    /// Engraving typeface
    #[arg(long, default_value = "sans", value_parser = ["sans", "sans-bold", "serif", "script"])]
    font: String,
    /// Only write the processed elevation grid (terrain.tif) and skip manufacturing files
    #[arg(long)]
    dem_only: bool,
}

/// Load the elevation input, returning the DEM and the number of nodata samples filled.
fn load_input(args: &Args) -> Result<(Dem, usize), Box<dyn std::error::Error>> {
    let text = args.input.to_string_lossy();
    if let Some(spec) = text.strip_prefix("usgs:") {
        let v: Vec<f64> = spec
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| "usgs: input must be four numbers: WEST,SOUTH,EAST,NORTH")?;
        let [west, south, east, north] = v[..] else {
            return Err("usgs: input must be four numbers: WEST,SOUTH,EAST,NORTH".into());
        };
        let bounds = Bounds {
            west,
            south,
            east,
            north,
        };
        bounds.validate()?;
        let long_side = args
            .resolution
            .unwrap_or_else(|| bounds.suggested_long_side(args.width_mm, 0.5));
        let plan = bounds.plan_download(long_side)?;
        eprintln!(
            "Downloading {} x {} samples from USGS 3DEP in {} request(s)",
            plan.width,
            plan.height,
            plan.tiles.len()
        );
        let r = terraincraft_core::usgs::download(&bounds, long_side, &mut |done, total| {
            eprintln!("  {done}/{total}")
        })?;
        fs::create_dir_all(&args.output)?;
        fs::write(
            args.output.join("usgs_elevation.tif"),
            terraincraft_core::geotiff::encode(&r.dem)?,
        )?;
        return Ok((r.dem, r.filled_cells));
    }
    let lower = text.to_ascii_lowercase();
    if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        let r = terraincraft_core::geotiff::decode(&fs::read(&args.input)?)?;
        return Ok((r.dem, r.filled_cells));
    }
    let input = fs::read_to_string(&args.input)?;
    if lower.ends_with(".asc") {
        let g = read_asc(&input)?;
        return Ok((g.dem, g.filled_cells));
    }
    let rows: Vec<Vec<f64>> = input
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|line| {
            line.split(',')
                .map(|v| v.trim().parse::<f64>())
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let w = rows.first().ok_or("Empty CSV")?.len();
    if rows.iter().any(|r| r.len() != w) {
        return Err("CSV must be rectangular".into());
    }
    let h = rows.len();
    Ok((Dem::new(w, h, rows.into_iter().flatten().collect())?, 0))
}

impl Args {
    fn export_settings(&self) -> Result<PrintExportSettings, String> {
        let mut s = match &self.settings {
            Some(path) => PrintExportSettings::from_json(
                &fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
            )?,
            None => PrintExportSettings::default(),
        };
        if let Some(base) = self.base_mm {
            s.base_mm = base;
        }
        if let Some(mode) = self.connectors {
            s.connectors = mode;
        }
        s.seam_dowel_guides |= self.seam_dowel_guides;
        s.repair_winding |= self.repair_winding;
        s.strict_mesh |= self.strict_mesh;
        s.check_intersections |= self.check_intersections;
        if self.no_labels {
            s.labels = false;
        }
        if let Some(t) = self.simplify {
            s.simplify_mm = Some(t);
        }
        if self.no_simplify {
            s.simplify_mm = None;
        }
        if let Some(v) = self.sample_mm {
            s.sample_mm = v;
        }
        s.validate()?;
        Ok(s)
    }
}

/// Earlier releases read options from TERRAINCRAFT_* environment variables. Warn rather
/// than silently ignoring them so existing scripts don't quietly change behavior.
fn warn_legacy_env() {
    for (var, flag) in [
        ("TERRAINCRAFT_BASE_MM", "--base-mm <mm>"),
        ("TERRAINCRAFT_INTEGRATED_SOCKETS", "--connectors underside"),
        ("TERRAINCRAFT_SEAM_SOCKETS", "--connectors seam-sockets"),
        ("TERRAINCRAFT_SEAM_GUIDES", "--seam-dowel-guides"),
        ("TERRAINCRAFT_SEAM_DOWELS", "--seam-dowel-guides"),
        ("TERRAINCRAFT_REPAIR_WINDING", "--repair-winding"),
        ("TERRAINCRAFT_STRICT_MESH", "--strict-mesh"),
        ("TERRAINCRAFT_CHECK_INTERSECTIONS", "--check-intersections"),
    ] {
        if std::env::var_os(var).is_some() {
            eprintln!("Warning: {var} is no longer read; use {flag} instead");
        }
    }
}

/// Named features for engravings: downloaded from USGS, read from a file, or none.
fn place_names(
    args: &Args,
    dem: &Dem,
    output: &Path,
) -> Result<Vec<Landmark>, Box<dyn std::error::Error>> {
    if let Some(path) = &args.names_file {
        return Ok(read_landmarks(&fs::read_to_string(path)?)?);
    }
    if !args.place_names {
        return Ok(Vec::new());
    }
    let bounds = terraincraft_core::usgs::dem_bounds(dem)
        .ok_or("--place-names needs a georeferenced elevation source")?;
    let names = terraincraft_core::usgs::download_place_names(&bounds, &LANDFORM_CLASSES)?;
    fs::create_dir_all(output)?;
    fs::write(output.join("place_names.csv"), to_csv(&names))?;
    println!("Downloaded {} place names from USGS GNIS", names.len());
    Ok(names)
}

fn font(name: &str) -> Font {
    match name {
        "sans-bold" => Font::SansBold,
        "serif" => Font::Serif,
        "script" => Font::Script,
        _ => Font::Sans,
    }
}

/// Write the requested map and panorama engravings.
fn write_engravings(
    args: &Args,
    dem: &Dem,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let names = place_names(args, dem, output)?;
    fs::create_dir_all(output)?;
    if args.engrave_map {
        let o = MapOptions {
            title: args.title.clone(),
            feet: args.feet,
            font: font(&args.font),
            style: if args.map_style == "stylized" {
                MapStyle::Stylized
            } else {
                MapStyle::Exact
            },
            ..MapOptions::default()
        };
        let map = engrave::topo_map(dem, &names, &o)?;
        fs::write(output.join("engraving_map.svg"), map.to_svg())?;
        println!(
            "Wrote engraving_map.svg ({} x {} mm)",
            o.sheet_w_mm, o.sheet_h_mm
        );
    }
    if let Some(spec) = &args.panorama {
        let v: Vec<f64> = spec
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| "--panorama expects LAT,LON,BEARING[,FOV]")?;
        let (lat, lon, bearing, fov) = match v[..] {
            [a, b, c] => (a, b, c, 90.),
            [a, b, c, d] => (a, b, c, d),
            _ => return Err("--panorama expects LAT,LON,BEARING[,FOV]".into()),
        };
        let view = PanoramaView {
            lat,
            lon,
            bearing_deg: bearing,
            fov_deg: fov,
            eye_height_m: 1.7,
            max_distance_km: 150.,
        };
        let o = PanoramaOptions {
            title: args.title.clone(),
            feet: args.feet,
            font: font(&args.font),
            ..PanoramaOptions::default()
        };
        let pano = engrave::panorama(dem, &names, &view, &o)?;
        fs::write(output.join("panorama.svg"), pano.to_svg())?;
        println!(
            "Wrote panorama.svg ({} x {} mm)",
            o.sheet_w_mm, o.sheet_h_mm
        );
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    warn_legacy_env();
    let args = Args::parse();
    let opts = args.export_settings()?;
    let (source, filled) = load_input(&args)?;
    if filled > 0 {
        eprintln!("Filled {filled} missing elevation samples from their surroundings");
    }
    let prep = TerrainPrep {
        utm: args.utm,
        smooth_sigma: args.smooth,
        sea_level_m: args.sea_level,
        water_depth_m: args.water_depth,
    };
    if args.utm && !source.georef.is_some_and(|g| g.units == CrsUnits::Degrees) {
        eprintln!("Warning: --utm needs latitude/longitude input; using the grid as is");
    }
    let (dem, report) = prepare(&source, &prep)?;
    if report.reprojected {
        let g = dem.georef.expect("reprojected DEMs are georeferenced");
        println!(
            "Reprojected to UTM (EPSG:{}): {} x {} samples at {:.1} m",
            g.epsg.unwrap_or(0),
            dem.width,
            dem.height,
            g.pixel_width
        );
    }
    if args.sea_level.is_some() {
        println!("Flattened {} samples as water", report.water_samples);
    }
    if args.engrave_map || args.panorama.is_some() {
        write_engravings(&args, &dem, &args.output)?;
    }
    if args.dem_only {
        fs::create_dir_all(&args.output)?;
        let path = args.output.join("terrain.tif");
        fs::write(&path, terraincraft_core::geotiff::encode(&dem)?)?;
        println!(
            "Wrote {} x {} grid to {}",
            dem.width,
            dem.height,
            path.display()
        );
        return Ok(());
    }
    // Print and laser files need no more detail than the machines can reproduce.
    let dem = match dem.resample_for_model(args.width_mm, opts.sample_mm) {
        Some(coarse) => {
            println!(
                "Resampled {} x {} to {} x {} samples ({} mm apart on the model)",
                dem.width, dem.height, coarse.width, coarse.height, opts.sample_mm
            );
            coarse
        }
        None => dem,
    };
    let output = args.output.clone();
    fs::create_dir_all(&output)?;
    let bed = args.bed_mm;
    // Georeferenced input keeps real proportions; bare grids fit a fixed 4:3 footprint
    // and relief because their ground size is unknown.
    let s = if dem.georef.is_some() {
        let s = terraincraft_core::true_scale_settings(
            &dem,
            args.width_mm,
            opts.base_mm,
            args.exaggeration.unwrap_or(1.),
        )?;
        println!(
            "True scale: {:.1} x {:.1} mm, {:.1} mm of relief",
            s.width_mm,
            s.height_mm,
            s.relief_mm * s.vertical_exaggeration
        );
        s
    } else {
        TerrainSettings {
            width_mm: args.width_mm,
            height_mm: args.width_mm * 0.75,
            base_mm: opts.base_mm,
            vertical_exaggeration: args.exaggeration.unwrap_or(2.),
            relief_mm: 25.,
        }
    };
    let tile_list = tiles(&dem, s, bed, bed, 5.)?;

    fs::write(
        output.join("print_tile_manifest.csv"),
        terraincraft_core::print_export::tile_manifest(&dem, s, &tile_list)?,
    )?;
    let reg = terraincraft_core::registration::RegistrationSettings::default();
    let guides = terraincraft_core::registration::guides(&dem, s, &tile_list, reg)?;
    let integrated = opts.connectors == ConnectorMode::Underside;
    let seam_integrated = opts.connectors == ConnectorMode::SeamSockets;
    let seam_holes = if seam_integrated {
        Some(terraincraft_core::seam_joints::plan(
            &dem,
            &tile_list,
            s,
            terraincraft_core::seam_joints::SeamSettings::default(),
        )?)
    } else {
        None
    };
    if let Some(ref holes) = seam_holes {
        let checks = terraincraft_core::seam_preflight::inspect(&dem, s, &tile_list, holes)?;
        fs::write(
            output.join("seam_socket_preflight.csv"),
            terraincraft_core::seam_preflight::csv(&checks),
        )?;
        fs::write(
            output.join("seam_socket_preview.svg"),
            terraincraft_core::seam_preflight::assembly_preview(&dem, s, &tile_list, &checks)?,
        )?;
        let failed = checks.iter().filter(|c| !c.compatible).count();
        if failed > 0 {
            return Err(format!("{failed} sidewall sockets are incompatible with the current DEM mesh; see seam_socket_preflight.csv and seam_socket_preview.svg. Use --seam-dowel-guides or a coarser DEM.").into());
        }
    }
    let repair_winding = opts.repair_winding;
    let mut repair_csv = String::from("filename,status,flipped_triangles,details\n");
    let strict_mesh = opts.strict_mesh;
    let mut audit_csv = String::from(terraincraft_core::mesh_audit::CSV_HEADER);
    let mut invalid_tiles = Vec::new();
    let mut manufacturing_csv = String::from(terraincraft_core::manufacturing::CSV_HEADER);
    let check_intersections = opts.check_intersections;
    let mut intersection_csv=String::from("filename,tested_pairs,crossings,coplanar_overlaps,skipped_shared_vertex_pairs,broadphase_candidates,complete\n");
    let mut assembly_meshes: Vec<(terraincraft_core::Tile, Vec<[[f64; 3]; 3]>)> = Vec::new();
    let mut raw = Vec::with_capacity(tile_list.len());
    for (tile, guide) in tile_list.iter().zip(&guides) {
        let path = output.join(format!(
            "tile_r{:02}_c{:02}.stl",
            tile.row + 1,
            tile.col + 1
        ));
        let mut faces = if let Some(ref all) = seam_holes {
            let selected: Vec<_> = all
                .iter()
                .filter(|h| h.tile_row == tile.row && h.tile_col == tile.col)
                .cloned()
                .collect();
            terraincraft_core::seam_socket_mesh::mesh_with_seam_sockets(
                &dem, s, *tile, &selected, 32,
            )?
        } else if integrated {
            terraincraft_core::socket_mesh::mesh_with_sockets(&dem, s, *tile, guide, reg, 32)?
        } else {
            mesh(&dem, s, *tile)?
        };
        if repair_winding {
            let filename = path.file_name().unwrap().to_string_lossy();
            match terraincraft_core::mesh_repair::repair_winding(&mut faces) {
                Ok(report) => repair_csv.push_str(&format!(
                    "{},repaired,{},ok\n",
                    filename, report.flipped_triangles
                )),
                Err(reason) => {
                    repair_csv.push_str(&format!(
                        "{},refused,0,{}\n",
                        filename,
                        reason.replace(',', ";")
                    ));
                    // Preserve original mesh for inspection; strict mode will still reject invalid topology.
                }
            }
        }
        terraincraft_core::north_up(&mut faces, &dem, s, *tile);
        raw.push((*tile, faces));
    }
    // Labels and simplification are the slow part; tiles are finished in parallel.
    let finished = terraincraft_core::finish_tiles(raw, s, &opts)?;
    for (tile, faces, warnings) in finished {
        for w in warnings {
            eprintln!("Warning: {w}");
        }
        let path = output.join(format!(
            "tile_r{:02}_c{:02}.stl",
            tile.row + 1,
            tile.col + 1
        ));
        let audit = terraincraft_core::mesh_audit::audit(&faces);
        let filename = path.file_name().unwrap().to_string_lossy();
        audit_csv.push_str(&audit.csv_row(&filename));
        let manufacturing = terraincraft_core::manufacturing::inspect(&faces);
        manufacturing_csv.push_str(&manufacturing.csv_row(&filename));
        let mut valid = manufacturing.is_ready_for_review();
        if check_intersections {
            let report = terraincraft_core::intersections::inspect(&faces, 250_000);
            intersection_csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                filename,
                report.tested_pairs,
                report.crossings,
                report.coplanar_overlaps,
                report.skipped_adjacent_pairs,
                report.broadphase_candidates,
                report.complete
            ));
            if report.crossings > 0 || !report.complete {
                valid = false;
            }
        }
        if !valid {
            invalid_tiles.push(filename.to_string());
        }
        // Defer ALL STL writes until every tile has passed strict preflight.
        // A rejected job must not leave a partial collection of printable STLs.
        assembly_meshes.push((tile, faces));
    }
    // Diagnostic reports are written even when strict preflight fails.
    fs::write(output.join("mesh_audit.csv"), &audit_csv)?;
    fs::write(output.join("manufacturing_report.csv"), &manufacturing_csv)?;
    if check_intersections {
        fs::write(output.join("intersection_report.csv"), &intersection_csv)?;
    }
    if repair_winding {
        fs::write(output.join("mesh_repair.csv"), &repair_csv)?;
    }
    if strict_mesh && !invalid_tiles.is_empty() {
        return Err(format!("Manufacturing preflight failed for {} tile(s): {}. No terrain STL files were written; inspect mesh_audit.csv and manufacturing_report.csv",invalid_tiles.len(),invalid_tiles.join(", ")).into());
    }
    // All geometry has passed the requested gate; now commit printable meshes.
    for (tile, faces) in &assembly_meshes {
        let path = output.join(format!(
            "tile_r{:02}_c{:02}.stl",
            tile.row + 1,
            tile.col + 1
        ));
        terraincraft_core::print_export::write_binary_stl(&path, faces)?;
    }
    // One 3MF with every tile as a named part, spread out in assembly order with gaps.
    let max_row = tile_list.iter().map(|t| t.row).max().unwrap_or(0);
    let parts: Vec<_> = assembly_meshes
        .iter()
        .map(|(t, triangles)| {
            let [x, y] = terraincraft_core::north_up_origin(&dem, s, *t);
            terraincraft_core::threemf::Part {
                name: format!("Tile {}", terraincraft_core::tile_label(*t)),
                triangles,
                offset_mm: [
                    x + t.col as f64 * 10.,
                    y + (max_row - t.row) as f64 * 10.,
                    0.,
                ],
            }
        })
        .collect();
    fs::write(
        output.join("terrain_tiles.3mf"),
        terraincraft_core::threemf::write(&parts)?,
    )?;
    let title = format!(
        "TerrainCraft: {}",
        args.input
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    fs::write(
        output.join("assembly_sheet.svg"),
        terraincraft_core::assembly_sheet::svg(&dem, s, &tile_list, &title)?,
    )?;
    let triangles: usize = assembly_meshes.iter().map(|(_, f)| f.len()).sum();
    println!(
        "{} tiles, {triangles} triangles{}",
        assembly_meshes.len(),
        opts.simplify_mm
            .map(|t| format!(" (simplified to within {t} mm)"))
            .unwrap_or_default()
    );
    let assembly_refs: Vec<_> = assembly_meshes
        .iter()
        .map(
            |(tile, triangles)| terraincraft_core::assembly_obj::AssemblyPart {
                tile: *tile,
                triangles,
            },
        )
        .collect();
    let (assembly_obj, assembly_mtl) =
        terraincraft_core::assembly_obj::export(&dem, s, &assembly_refs)?;
    fs::write(output.join("terrain_assembly.obj"), assembly_obj)?;
    fs::write(output.join("terrain_assembly.mtl"), assembly_mtl)?;
    for g in &guides {
        fs::write(
            output.join(format!(
                "registration_r{:02}_c{:02}.svg",
                g.row + 1,
                g.col + 1
            )),
            terraincraft_core::registration::guide_svg(g, reg)?,
        )?;
    }
    fs::write(
        output.join("registration_manifest.csv"),
        terraincraft_core::registration::guide_manifest(&guides, reg),
    )?;
    terraincraft_core::print_export::write_binary_stl(
        &output.join("registration_pin.stl"),
        &terraincraft_core::registration::pin_mesh(reg, 32)?,
    )?;

    if opts.seam_dowel_guides {
        let cfg = terraincraft_core::seam_joints::SeamSettings::default();
        let holes = terraincraft_core::seam_joints::plan(&dem, &tile_list, s, cfg)?;
        fs::write(
            output.join("seam_dowel_manifest.csv"),
            terraincraft_core::seam_joints::manifest(&holes),
        )?;
        terraincraft_core::seam_fabrication::preflight(&holes, cfg)?;
        fs::write(
            output.join("seam_assembly.svg"),
            terraincraft_core::seam_fabrication::assembly_svg(&holes),
        )?;
        terraincraft_core::print_export::write_binary_stl(
            &output.join("seam_dowel.stl"),
            &terraincraft_core::seam_fabrication::dowel_mesh(cfg, 48)?,
        )?;
        let sx = s.width_mm / (dem.width - 1) as f64;
        let sy = s.height_mm / (dem.height - 1) as f64;
        for tile in &tile_list {
            for side in ["east", "west", "north", "south"] {
                let selected: Vec<_> = holes
                    .iter()
                    .filter(|h| h.tile_row == tile.row && h.tile_col == tile.col && h.side == side)
                    .collect();
                if selected.is_empty() {
                    continue;
                }
                let length = if side == "east" || side == "west" {
                    (tile.y1 - tile.y0) as f64 * sy
                } else {
                    (tile.x1 - tile.x0) as f64 * sx
                };
                let svg = terraincraft_core::seam_joints::wall_svg(length, s.base_mm, &selected)?;
                fs::write(
                    output.join(format!(
                        "seam_r{:02}_c{:02}_{}.svg",
                        tile.row + 1,
                        tile.col + 1,
                        side
                    )),
                    svg,
                )?;
            }
        }
    }
    let (lo, hi) = dem.min_max();
    for i in 1..=8 {
        let level = lo + (hi - lo) * i as f64 / 9.;
        fs::write(
            output.join(format!("contour_{i:02}.svg")),
            terraincraft_core::contours::svg(&dem, s.width_mm, s.height_mm, level)?,
        )?;
    }
    let st = StackSettings {
        material_mm: args.material_mm,
        kerf_mm: args.kerf_mm,
        sheet_w_mm: args.sheet_width_mm,
        sheet_h_mm: args.sheet_height_mm,
        ..StackSettings::default()
    };
    let levels = stack::levels_for_material(&dem, s, st.material_mm);
    let parts = stack::build_parts(&dem, s, &levels, &st)?;
    let placements = stack::place(&parts, &st)?;
    let sheets = placements.iter().map(|p| p.sheet).max().unwrap_or(0);
    for sheet in 1..=sheets {
        fs::write(
            output.join(format!("laser_sheet_{sheet:02}.svg")),
            stack::sheet_svg(&parts, &placements, sheet, &st),
        )?;
        fs::write(
            output.join(format!("laser_sheet_{sheet:02}.dxf")),
            stack::sheet_dxf(&parts, &placements, sheet, &st),
        )?;
    }
    fs::write(
        output.join("laser_manifest.csv"),
        stack::manifest_csv(&parts, &placements),
    )?;
    fs::write(
        output.join("laser_layers.svg"),
        stack::layers_preview_svg(&parts, s.width_mm, s.height_mm),
    )?;
    println!(
        "Laser stack: {} parts in {} layers of {} mm on {} sheets",
        parts.len(),
        parts.iter().map(|p| p.layer).max().unwrap_or(0),
        st.material_mm,
        sheets
    );
    println!(
        "Generated {} STL tiles and 8 SVG contour engravings in {}",
        tile_list.len(),
        output.display()
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1)
    }
}
