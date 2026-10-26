//! Project save/open, elevation loading, and USGS 3DEP download.
use crate::studio::DownloadMsg;
use crate::studio::Studio;
use std::{fs, path::Path, sync::mpsc, thread};
use terraincraft_core::georef::CrsUnits;
use terraincraft_core::geotiff::{self, DecodedRaster};
use terraincraft_core::terrain_ops::prepare;
use terraincraft_core::usgs::{self, Bounds};
use terraincraft_core::{Dem, TerrainSettings};

impl Studio {
    pub(crate) fn save_project(&self) -> Result<String, String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&self.project_path, json).map_err(|e| e.to_string())?;
        Ok(format!("Saved project to {}", self.project_path))
    }
    pub(crate) fn open_project(&mut self) -> Result<String, String> {
        let json = fs::read_to_string(&self.project_path).map_err(|e| e.to_string())?;
        let mut project: Studio = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        project.project_path = self.project_path.clone();
        project.load()?;
        *self = project;
        Ok("Project loaded (including elevation source)".into())
    }
    pub(crate) fn begin_download(&mut self) -> Result<(), String> {
        if self.download_rx.is_some() {
            return Err("A download is already running".into());
        }
        let bounds = Bounds {
            west: self.west,
            south: self.south,
            east: self.east,
            north: self.north,
        };
        let plan = bounds.plan_download(self.raster_pixels)?;
        let long_side = self.raster_pixels;
        let (tx, rx) = mpsc::channel();
        self.download_rx = Some(rx);
        self.message = format!(
            "Downloading {} × {} USGS 3DEP samples in {} request(s)…",
            plan.width,
            plan.height,
            plan.tiles.len()
        );
        thread::spawn(move || {
            let result = usgs::download(&bounds, long_side, &mut |done, total| {
                let _ = tx.send(DownloadMsg::Progress(done, total));
            });
            let _ = tx.send(DownloadMsg::Done(result));
        });
        Ok(())
    }
    pub(crate) fn finish_download(&mut self, decoded: DecodedRaster) -> Result<(), String> {
        let (w, h) = (decoded.dem.width, decoded.dem.height);
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let file = Path::new(&self.output).join("usgs_elevation.tif");
        fs::write(&file, geotiff::encode(&decoded.dem)?).map_err(|e| e.to_string())?;
        self.path = file.to_string_lossy().into_owned();
        self.set_dem(decoded.dem);
        self.message = format!(
            "Downloaded USGS elevation grid {} × {}{}. Source saved to {}",
            w,
            h,
            filled_note(decoded.filled_cells),
            self.path
        );
        Ok(())
    }
    /// Install a newly loaded DEM. A geographic georeference replaces the selection
    /// bounds with the raster's actual extent, so landmark and observer placement
    /// use the coordinates the samples really cover.
    fn set_dem(&mut self, dem: Dem) {
        if let Some(g) = dem.georef.filter(|g| g.units == CrsUnits::Degrees) {
            [self.west, self.south, self.east, self.north] = g.bounds(dem.width, dem.height);
        }
        self.source_dem = Some(dem);
        self.applied_prep = None;
        self.reprocess();
    }
    /// Fetch USGS place names for the loaded area in the background.
    pub(crate) fn begin_place_names(&mut self) {
        let Some(bounds) = self.dem.as_ref().and_then(usgs::dem_bounds) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.names_rx = Some(rx);
        self.message = "Downloading USGS place names…".into();
        thread::spawn(move || {
            let _ = tx.send(usgs::download_place_names(&bounds, &usgs::LANDFORM_CLASSES));
        });
    }
    /// Rebuild `dem` from `source_dem` when the processing settings changed.
    pub(crate) fn reprocess(&mut self) {
        let Some(source) = &self.source_dem else {
            return;
        };
        if self.applied_prep.as_ref() == Some(&self.prep) {
            return;
        }
        match prepare(source, &self.prep) {
            Ok((dem, _)) => self.dem = Some(dem),
            Err(e) => self.message = format!("Terrain processing: {e}"),
        }
        self.applied_prep = Some(self.prep.clone());
        self.sync_true_scale();
    }
    /// With true scale on and a georeferenced DEM, derive model height and relief from
    /// the ground extent. Writing them back to the fields keeps every preview and export
    /// that reads `height`/`relief` consistent, and saves them with the project.
    pub(crate) fn sync_true_scale(&mut self) {
        if !self.true_scale {
            return;
        }
        if let Some(s) = self.dem.as_ref().and_then(|d| {
            terraincraft_core::true_scale_settings(d, self.width, self.base, self.exaggeration).ok()
        }) {
            self.height = s.height_mm;
            self.relief = s.relief_mm;
        }
    }
    pub(crate) fn settings(&self) -> TerrainSettings {
        TerrainSettings {
            width_mm: self.width,
            height_mm: self.height,
            base_mm: self.base,
            relief_mm: self.relief,
            vertical_exaggeration: self.exaggeration,
        }
    }
    pub(crate) fn load(&mut self) -> Result<(), String> {
        let bytes = fs::read(&self.path).map_err(|e| e.to_string())?;
        let suffix = Path::new(&self.path)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (dem, filled) = match suffix.as_str() {
            "tif" | "tiff" => {
                let r = geotiff::decode(&bytes)?;
                (r.dem, r.filled_cells)
            }
            "asc" => {
                let g = terraincraft_core::gis::read_asc(
                    &String::from_utf8(bytes).map_err(|e| e.to_string())?,
                )?;
                (g.dem, g.filled_cells)
            }
            "csv" => {
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                let rows = text
                    .lines()
                    .filter(|v| !v.trim().is_empty())
                    .map(|line| {
                        line.split(',')
                            .map(|v| v.trim().parse::<f64>().map_err(|e| e.to_string()))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let w = rows.first().ok_or("Empty CSV")?.len();
                let h = rows.len();
                (Dem::new(w, h, rows.into_iter().flatten().collect())?, 0)
            }
            _ => return Err("Supported input formats: CSV, ESRI ASCII (.asc), GeoTIFF".into()),
        };
        self.message = format!(
            "Loaded {} × {} elevation samples{}",
            dem.width,
            dem.height,
            filled_note(filled)
        );
        self.set_dem(dem);
        Ok(())
    }
    pub(crate) fn levels(&self, dem: &Dem) -> Vec<f64> {
        let (lo, hi) = dem.min_max();
        (1..=self.layer_count)
            .map(|i| lo + (hi - lo) * i as f64 / (self.layer_count + 1) as f64)
            .collect()
    }
}

fn filled_note(filled: usize) -> String {
    if filled == 0 {
        String::new()
    } else {
        format!(" ({filled} missing samples filled)")
    }
}
