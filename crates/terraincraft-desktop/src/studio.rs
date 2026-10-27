//! Application state persisted in project files.
use eframe::egui::{self};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
};
use terraincraft_core::engraving_editor::Annotation;
use terraincraft_core::geotiff::DecodedRaster;
use terraincraft_core::terrain_ops::TerrainPrep;
use terraincraft_core::Dem;

/// A downloaded map tile: its cache key and the image bytes or an error.
pub(crate) type TileFetch = (String, Result<Vec<u8>, String>);

/// Messages from the background USGS download thread.
pub(crate) enum DownloadMsg {
    Progress(usize, usize),
    Done(Result<DecodedRaster, String>),
}

#[derive(PartialEq, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum View {
    Terrain,
    Print,
    Laser,
    Engraving,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Studio {
    pub(crate) path: String,
    pub(crate) output: String,
    /// Processed elevation grid that previews and exports use.
    #[serde(skip)]
    pub(crate) dem: Option<Dem>,
    /// Elevation grid as loaded, before `prep` is applied.
    #[serde(skip)]
    pub(crate) source_dem: Option<Dem>,
    /// The `prep` that produced `dem`, so changes trigger reprocessing.
    #[serde(skip)]
    pub(crate) applied_prep: Option<TerrainPrep>,
    pub(crate) prep: TerrainPrep,
    /// Engrave tile numbers under exported tiles.
    pub(crate) print_labels: bool,
    /// Simplify exported tile meshes to within `simplify_mm`.
    pub(crate) print_simplify: bool,
    pub(crate) simplify_mm: f64,
    /// Slicer program to open exported 3MF files with.
    pub(crate) slicer_path: String,
    #[serde(skip)]
    pub(crate) last_3mf: Option<String>,
    pub(crate) message: String,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) base: f64,
    pub(crate) relief: f64,
    pub(crate) exaggeration: f64,
    /// Derive model depth and relief from the georeferenced ground size.
    pub(crate) true_scale: bool,
    pub(crate) bed_x: f64,
    pub(crate) bed_y: f64,
    pub(crate) margin: f64,
    pub(crate) sheet_x: f64,
    pub(crate) sheet_y: f64,
    pub(crate) gap: f64,
    pub(crate) layer_count: usize,
    pub(crate) material: f64,
    /// Laser kerf width in millimeters.
    pub(crate) kerf: f64,
    /// Last laser stack and the inputs it was built from; rebuilt only when they change.
    #[serde(skip)]
    pub(crate) laser_cache: std::cell::RefCell<Option<(String, crate::export::LaserResult)>>,
    pub(crate) view: View,
    pub(crate) sheet: usize,
    pub(crate) engraving_perspective: bool,
    pub(crate) engraving_style: usize,
    pub(crate) engraving_title: String,
    pub(crate) engraving_subtitle: String,
    pub(crate) engraving_font: String,
    pub(crate) contour_interval: f64,
    pub(crate) observer_x: f64,
    pub(crate) observer_y: f64,
    pub(crate) bearing: f64,
    pub(crate) pitch: f64,
    pub(crate) fov: f64,
    pub(crate) eye_height: f64,
    pub(crate) max_distance: f64,
    pub(crate) pixel_size_m: f64,
    pub(crate) label_candidate_peaks: bool,
    pub(crate) peak_prominence_m: f64,
    pub(crate) peak_label_limit: usize,
    pub(crate) geo_view_enabled: bool,
    pub(crate) observer_lat: f64,
    pub(crate) observer_lon: f64,
    pub(crate) depth_bands: usize,
    pub(crate) show_geo_map: bool,
    pub(crate) landmark_csv: String,
    pub(crate) show_landmarks: bool,
    /// Basemap center and whether the Terrain workspace shows the map (or the 3D view).
    pub(crate) map_center_lat: f64,
    #[serde(skip)]
    pub(crate) history: crate::history::History,
    pub(crate) map_center_lon: f64,
    pub(crate) terrain_show_map: bool,
    #[serde(skip)]
    pub(crate) map_select_start: Option<(f64, f64)>,
    #[serde(skip)]
    pub(crate) map_aiming: bool,
    pub(crate) search_query: String,
    #[serde(skip)]
    pub(crate) search_results: Vec<terraincraft_core::geocode::Place>,
    #[serde(skip)]
    pub(crate) search_rx:
        Option<mpsc::Receiver<Result<Vec<terraincraft_core::geocode::Place>, String>>>,
    /// Show engraving elevations and contours in feet.
    pub(crate) engraving_feet: bool,
    /// Exact (survey) or stylized (illustrated) top-down map.
    pub(crate) map_style: terraincraft_core::engrave::MapStyle,
    /// Typeface for every engraved text, annotations included.
    pub(crate) stroke_font: terraincraft_core::stroke_font::Font,
    /// Draw the selected library icon on the engraving.
    pub(crate) place_icon: bool,
    /// The selected icon's SVG and its strokes, so the preview converts it only once.
    #[serde(skip)]
    pub(crate) icon_cache:
        std::cell::RefCell<Option<(String, Option<terraincraft_core::svg_lines::Strokes>)>>,
    /// Named features downloaded from USGS for the current area.
    pub(crate) place_names: Vec<terraincraft_core::landmarks::Landmark>,
    #[serde(skip)]
    pub(crate) names_rx:
        Option<mpsc::Receiver<Result<Vec<terraincraft_core::landmarks::Landmark>, String>>>,
    /// Last engraving drawing and the inputs it was built from.
    #[serde(skip)]
    pub(crate) engraving_cache:
        std::cell::RefCell<Option<(String, crate::engraving::DrawingResult)>>,
    pub(crate) assembly_exploded: bool,
    pub(crate) assembly_gap_mm: f64,
    pub(crate) show_assembly_labels: bool,
    pub(crate) assembly_mesh_mode: bool,
    pub(crate) assembly_shaded: bool,
    pub(crate) assembly_gpu: bool,
    pub(crate) assembly_connector_mode: usize,
    #[serde(skip)]
    pub(crate) orbit_yaw: f32,
    #[serde(skip)]
    pub(crate) orbit_pitch: f32,
    #[serde(skip)]
    pub(crate) orbit_zoom: f32,
    pub(crate) annotations: Vec<Annotation>,
    pub(crate) engraving_preset: usize,
    pub(crate) icon_id: usize,
    pub(crate) cultural_icon_id: String,
    pub(crate) regional_icon_id: String,
    pub(crate) regional_style: usize,
    pub(crate) icon_variant: usize,
    pub(crate) logo_catalog_path: String,
    pub(crate) logo_search: String,
    pub(crate) selected_logo_id: String,
    pub(crate) unesco_catalog_path: String,
    pub(crate) unesco_search: String,
    pub(crate) selected_unesco_svg: String,
    pub(crate) unesco_simplified: bool,
    pub(crate) icon_x: f64,
    pub(crate) icon_y: f64,
    pub(crate) icon_size: f64,
    pub(crate) map_provider: usize,
    pub(crate) map_url: String,
    pub(crate) catalog_path: String,
    pub(crate) catalog_jurisdiction: String,
    pub(crate) catalog_verified_only: bool,
    pub(crate) catalog_auto_icons: bool,
    pub(crate) project_path: String,
    pub(crate) west: f64,
    pub(crate) south: f64,
    pub(crate) east: f64,
    pub(crate) north: f64,
    pub(crate) raster_pixels: u32,
    #[serde(skip)]
    pub(crate) download_rx: Option<mpsc::Receiver<DownloadMsg>>,
    pub(crate) map_zoom: u8,
    pub(crate) map_markers: bool,
    pub(crate) map_marker_limit: usize,
    #[serde(skip)]
    pub(crate) map_results: Option<mpsc::Receiver<TileFetch>>,
    #[serde(skip)]
    pub(crate) map_textures: HashMap<String, egui::TextureHandle>,
    #[serde(skip)]
    pub(crate) map_pending: HashSet<String>,
    #[serde(skip)]
    pub(crate) map_requested: HashSet<String>,
    #[serde(skip)]
    pub(crate) map_tx: Option<mpsc::Sender<TileFetch>>,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            path: "examples/demo.csv".into(),
            output: "output".into(),
            dem: None,
            source_dem: None,
            applied_prep: None,
            prep: TerrainPrep::default(),
            print_labels: true,
            print_simplify: true,
            simplify_mm: 0.05,
            slicer_path: String::new(),
            last_3mf: None,
            message: "Load an elevation grid to begin".into(),
            width: 400.,
            height: 300.,
            base: 3.,
            relief: 25.,
            exaggeration: 2.,
            true_scale: true,
            bed_x: 220.,
            bed_y: 220.,
            margin: 5.,
            sheet_x: 600.,
            sheet_y: 400.,
            gap: 3.,
            layer_count: 8,
            material: 3.,
            kerf: 0.15,
            laser_cache: Default::default(),
            view: View::Terrain,
            sheet: 1,
            engraving_perspective: true,
            engraving_style: 0,
            engraving_title: "Mountain panorama".into(),
            engraving_subtitle: String::new(),
            engraving_font: "serif".into(),
            contour_interval: 20.,
            observer_x: 0.5,
            observer_y: 0.5,
            bearing: 90.,
            pitch: 0.,
            fov: 90.,
            eye_height: 1.7,
            max_distance: 40000.,
            pixel_size_m: 30.,
            label_candidate_peaks: true,
            peak_prominence_m: 5.,
            peak_label_limit: 12,
            geo_view_enabled: false,
            observer_lat: 38.58,
            observer_lon: -121.50,
            depth_bands: 1,
            show_geo_map: true,
            landmark_csv: String::new(),
            show_landmarks: true,
            map_center_lat: 46.85,
            history: Default::default(),
            map_center_lon: -121.76,
            terrain_show_map: true,
            map_select_start: None,
            map_aiming: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_rx: None,
            engraving_feet: false,
            map_style: Default::default(),
            stroke_font: Default::default(),
            place_icon: true,
            icon_cache: Default::default(),
            place_names: Vec::new(),
            names_rx: None,
            engraving_cache: Default::default(),
            assembly_exploded: true,
            assembly_gap_mm: 12.,
            show_assembly_labels: true,
            assembly_mesh_mode: false,
            assembly_shaded: true,
            assembly_gpu: false,
            assembly_connector_mode: 0,
            orbit_yaw: 0.65,
            orbit_pitch: 0.65,
            orbit_zoom: 1.,
            annotations: Vec::new(),
            engraving_preset: 0,
            icon_id: 0,
            cultural_icon_id: String::new(),
            regional_icon_id: String::new(),
            regional_style: 0,
            icon_variant: 0,
            logo_catalog_path: "assets/institution_logo_catalog.json".into(),
            logo_search: String::new(),
            selected_logo_id: String::new(),
            unesco_catalog_path: "assets/unesco_whc/catalog.json".into(),
            unesco_search: String::new(),
            selected_unesco_svg: String::new(),
            unesco_simplified: false,
            icon_x: 0.5,
            icon_y: 0.5,
            icon_size: 12.,
            map_provider: 0,
            map_url: String::new(),
            catalog_path: "assets/landmark_catalog_template.csv".into(),
            catalog_jurisdiction: "US-CA".into(),
            catalog_verified_only: true,
            catalog_auto_icons: false,
            project_path: "terraincraft-project.json".into(),
            west: -121.65,
            south: 38.48,
            east: -121.35,
            north: 38.68,
            raster_pixels: 512,
            download_rx: None,
            map_zoom: 10,
            map_markers: true,
            map_marker_limit: 100,
            map_results: None,
            map_textures: HashMap::new(),
            map_pending: HashSet::new(),
            map_requested: HashSet::new(),
            map_tx: None,
        }
    }
}
