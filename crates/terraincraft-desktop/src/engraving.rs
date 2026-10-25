//! Single-sheet engraving SVG generation and icon selection.
use crate::studio::Studio;
use std::fs;
use terraincraft_core::cartography::{self, EngravingOptions, EngravingStyle, Viewpoint};
use terraincraft_core::engraving_editor::Preset;
use terraincraft_core::geoview::GeographicExtent;
use terraincraft_core::Dem;

impl Studio {
    pub(crate) fn engraving_svg(&self, dem: &Dem) -> Result<String, String> {
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
        if self.engraving_perspective {
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
                        "Geographic viewpoint requires a USGS raster downloaded in this project"
                            .into(),
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
                    &self.selected_icon_svg().as_str(),
                    self.icon_x * self.width,
                    self.icon_y * self.height,
                    self.icon_size,
                )
            }
        } else {
            let mut svg = cartography::topdown_svg(dem, &opts)?;
            if self.show_landmarks && !self.landmark_csv.trim().is_empty() {
                let features = terraincraft_core::landmarks::read_landmarks(
                    &fs::read_to_string(&self.landmark_csv).map_err(|e| e.to_string())?,
                )?;
                svg = terraincraft_core::landmarks::overlay_topdown(
                    svg,
                    &features,
                    GeographicExtent {
                        west: self.west,
                        south: self.south,
                        east: self.east,
                        north: self.north,
                    },
                    self.width,
                    self.height,
                )?;
            }
            if self.catalog_auto_icons {
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
                    &self.selected_icon_svg().as_str(),
                    self.icon_x * self.width,
                    self.icon_y * self.height,
                    self.icon_size,
                )
            }
        }
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
