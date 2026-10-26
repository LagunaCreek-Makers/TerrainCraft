//! Single-sheet engraving SVG generation and icon selection.
use crate::studio::Studio;
use std::fs;
use std::sync::Arc;
use terraincraft_core::cartography::{self, EngravingOptions, EngravingStyle, Viewpoint};
use terraincraft_core::engrave::{self, Drawing, MapOptions, PanoramaOptions, PanoramaView};
use terraincraft_core::engraving_editor::Preset;
use terraincraft_core::geoview::GeographicExtent;
use terraincraft_core::landmarks::{read_landmarks, Landmark};
use terraincraft_core::Dem;

/// The engraving's base artwork, shared by the preview and export.
pub(crate) type DrawingResult = Result<Arc<Drawing>, String>;

impl Studio {
    /// Downloaded USGS names plus the landmark file, when shown.
    fn engraving_names(&self) -> Result<Vec<Landmark>, String> {
        let mut names = self.place_names.clone();
        if self.show_landmarks && !self.landmark_csv.trim().is_empty() {
            names.extend(read_landmarks(
                &fs::read_to_string(&self.landmark_csv).map_err(|e| e.to_string())?,
            )?);
        }
        Ok(names)
    }

    /// Whether the new map or panorama renderer applies: maps always, panoramas only for
    /// georeferenced data (plain grids keep the raster-coordinate panorama).
    pub(crate) fn uses_drawing(&self, dem: &Dem) -> bool {
        !self.engraving_perspective || dem.georef.is_some()
    }

    /// Map or panorama for `dem`, rebuilt only when the inputs change.
    pub(crate) fn engraving_drawing(&self, dem: &Dem) -> DrawingResult {
        let key = format!(
            "{}|{}x{}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.path,
            dem.width,
            dem.height,
            dem.min_max(),
            self.applied_prep,
            self.engraving_perspective,
            self.width,
            self.height,
            self.contour_interval,
            self.engraving_title,
            self.engraving_subtitle,
            self.engraving_feet,
            self.observer_lat,
            self.observer_lon,
            self.bearing,
            self.fov,
            self.eye_height,
            self.max_distance,
        ) + &format!(
            "|{}|{}|{}",
            self.place_names.len(),
            self.show_landmarks,
            self.landmark_csv
        );
        if let Some((k, r)) = &*self.engraving_cache.borrow() {
            if *k == key {
                return r.clone();
            }
        }
        let result = (|| {
            let names = self.engraving_names()?;
            let drawing = if self.engraving_perspective {
                let view = PanoramaView {
                    lat: self.observer_lat,
                    lon: self.observer_lon,
                    bearing_deg: self.bearing,
                    fov_deg: self.fov,
                    eye_height_m: self.eye_height,
                    max_distance_km: self.max_distance / 1000.,
                };
                let o = PanoramaOptions {
                    sheet_w_mm: self.width,
                    sheet_h_mm: self.height,
                    title: self.engraving_title.clone(),
                    subtitle: self.engraving_subtitle.clone(),
                    feet: self.engraving_feet,
                    ..PanoramaOptions::default()
                };
                engrave::panorama(dem, &names, &view, &o)?
            } else {
                let o = MapOptions {
                    sheet_w_mm: self.width,
                    sheet_h_mm: self.height,
                    contour_interval: self.contour_interval,
                    title: self.engraving_title.clone(),
                    subtitle: self.engraving_subtitle.clone(),
                    feet: self.engraving_feet,
                    ..MapOptions::default()
                };
                engrave::topo_map(dem, &names, &o)?
            };
            Ok(Arc::new(drawing))
        })();
        *self.engraving_cache.borrow_mut() = Some((key, result.clone()));
        result
    }

    pub(crate) fn engraving_svg(&self, dem: &Dem) -> Result<String, String> {
        if self.uses_drawing(dem) {
            // Named features are drawn by the map or panorama itself; user annotations,
            // catalog icons and the selected icon still go on top.
            let mut svg = self.engraving_drawing(dem)?.to_svg();
            if !self.engraving_perspective && self.catalog_auto_icons {
                svg = self.catalog_icons(svg)?;
            }
            let svg = terraincraft_core::engraving_editor::overlay(
                svg,
                &self.annotations,
                self.width,
                self.height,
                self.preset(),
            )?;
            return terraincraft_core::icon_library::overlay_icon(
                svg,
                self.selected_icon_svg().as_str(),
                self.icon_x * self.width,
                self.icon_y * self.height,
                self.icon_size,
            );
        }
        let style = match self.engraving_style {
            1 => EngravingStyle::Contour,
            2 => EngravingStyle::Illustrated,
            _ => EngravingStyle::Ridgeline,
        };
        let opts = EngravingOptions {
            width_mm: self.width,
            height_mm: self.height,
            contour_interval_m: self.contour_interval,
            style,
            title: self.engraving_title.clone(),
            subtitle: self.engraving_subtitle.clone(),
            font_family: self.engraving_font.clone(),
            show_coordinates: false,
        };
        // Panorama of a grid without georeferencing, in raster coordinates.
        let mut vp = Viewpoint {
            x: self.observer_x * (dem.width - 1) as f64,
            y: self.observer_y * (dem.height - 1) as f64,
            bearing_deg: self.bearing,
            pitch_deg: self.pitch,
            fov_deg: self.fov,
            eye_height_m: self.eye_height,
            max_distance_m: self.max_distance,
            pixel_size_x_m: self.pixel_size_m,
            pixel_size_y_m: self.pixel_size_m,
        };
        if self.geo_view_enabled {
            // The selected USGS bounding box is known to use EPSG:4326 and north-up pixels.
            // Imported files must NOT be assumed to share this metadata.
            if !self.path.ends_with("usgs_elevation.tif") {
                return Err(
                    "Geographic viewpoint requires a USGS raster downloaded in this project".into(),
                );
            }
            let extent = GeographicExtent {
                west: self.west,
                south: self.south,
                east: self.east,
                north: self.north,
            };
            vp = extent.viewpoint(
                self.observer_lat,
                self.observer_lon,
                dem.width,
                dem.height,
                vp,
            )?;
        }
        let mut svg = if self.depth_bands >= 2 {
            cartography::panorama_depth_svg(dem, vp, &opts, self.depth_bands)?
        } else {
            cartography::panorama_svg(dem, vp, &opts)?
        };
        if self.label_candidate_peaks && self.depth_bands == 1 {
            svg = cartography::panorama_with_peaks_svg(
                dem,
                vp,
                &opts,
                self.peak_prominence_m,
                self.peak_label_limit,
            )?;
        }
        if self.show_landmarks && !self.landmark_csv.trim().is_empty() {
            let features = terraincraft_core::landmarks::read_landmarks(
                &fs::read_to_string(&self.landmark_csv).map_err(|e| e.to_string())?,
            )?;
            if !self.geo_view_enabled {
                return Err(
                    "Perspective geographic landmarks require a georeferenced USGS viewpoint"
                        .into(),
                );
            }
            svg = terraincraft_core::landmarks::overlay_perspective(
                svg,
                &features,
                GeographicExtent {
                    west: self.west,
                    south: self.south,
                    east: self.east,
                    north: self.north,
                },
                dem,
                vp,
                self.width,
                self.height,
            )?;
        }
        {
            let svg = terraincraft_core::engraving_editor::overlay(
                svg,
                &self.annotations,
                self.width,
                self.height,
                self.preset(),
            )?;
            terraincraft_core::icon_library::overlay_icon(
                svg,
                self.selected_icon_svg().as_str(),
                self.icon_x * self.width,
                self.icon_y * self.height,
                self.icon_size,
            )
        }
    }
    /// Bundled icons for verified catalog landmarks inside the selection.
    fn catalog_icons(&self, mut svg: String) -> Result<String, String> {
        let entries = terraincraft_core::landmark_catalog::parse_catalog(
            &fs::read_to_string(&self.catalog_path).map_err(|e| format!("Catalog: {e}"))?,
        )?;
        let selected = terraincraft_core::landmark_catalog::filter_jurisdiction(
            &entries,
            &self.catalog_jurisdiction,
            true,
        );
        let selected: Vec<_> = selected.into_iter().cloned().collect();
        for (entry, x, y) in terraincraft_core::landmark_catalog::positions_in_extent(
            &selected, self.west, self.south, self.east, self.north,
        )? {
            if let Some(index) =
                terraincraft_core::landmark_catalog::bundled_icon_index(&entry.icon_id)
            {
                svg = terraincraft_core::icon_library::overlay_icon(
                    svg,
                    &terraincraft_core::icon_library::procedural_icon(index),
                    x * self.width,
                    y * self.height,
                    self.icon_size,
                )?;
            }
        }
        Ok(svg)
    }
    pub(crate) fn selected_icon_svg(&self) -> String {
        if !self.selected_unesco_svg.is_empty() {
            let base = std::path::Path::new(&self.unesco_catalog_path)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let selected = if self.unesco_simplified
                && !self.selected_unesco_svg.ends_with("--simplified.svg")
            {
                self.selected_unesco_svg.replace(".svg", "--simplified.svg")
            } else if !self.unesco_simplified {
                self.selected_unesco_svg.replace("--simplified.svg", ".svg")
            } else {
                self.selected_unesco_svg.clone()
            };
            let path = base.join(selected);
            if let Ok(svg) = fs::read_to_string(path) {
                return svg;
            }
        }
        if !self.regional_icon_id.is_empty() {
            let styles = ["original", "simplified", "profile", "three-quarter"];
            let style = styles[self.regional_style.min(styles.len() - 1)];
            if let Ok(svg) = terraincraft_core::regional_icons::read_svg(
                std::path::Path::new("."),
                &self.regional_icon_id,
                style,
            ) {
                return svg;
            }
        }
        let style = terraincraft_core::icon_variants::IconStyle::from_index(self.icon_variant);
        let is_temple = self.cultural_icon_id == "us-ut-salt-lake-temple";
        if !self.selected_logo_id.is_empty() {
            let catalog = std::path::Path::new(&self.logo_catalog_path);
            if let Ok(records) = terraincraft_core::institution_logos::read_catalog(catalog) {
                if let Some(record) = records.iter().find(|r| r.id == self.selected_logo_id) {
                    if let Ok(svg) = terraincraft_core::institution_logos::load_svg(
                        record,
                        catalog.parent().unwrap_or(std::path::Path::new(".")),
                    ) {
                        return terraincraft_core::icon_variants::apply(&svg, style, false);
                    }
                }
            }
        }
        let svg = terraincraft_core::state_culture::svg(&self.cultural_icon_id)
            .unwrap_or_else(|| terraincraft_core::icon_library::procedural_icon(self.icon_id));
        terraincraft_core::icon_variants::apply(&svg, style, is_temple)
    }
    pub(crate) fn preset(&self) -> Preset {
        match self.engraving_preset {
            1 => Preset::Minimal,
            2 => Preset::Vintage,
            3 => Preset::Cartoon,
            _ => Preset::Survey,
        }
    }
}
