//! Project save/open, elevation loading, and USGS 3DEP download.
use crate::studio::Studio;
use std::{fs, io::Cursor, path::Path, sync::mpsc, thread};
use terraincraft_core::usgs::Bounds;
use terraincraft_core::{Dem, TerrainSettings};
use tiff::decoder::{Decoder, DecodingResult};

fn decode_tiff(bytes: &[u8]) -> Result<Dem, String> {
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| format!("TIFF decoder: {e}"))?;
    let (w, h) = decoder.dimensions().map_err(|e| e.to_string())?;
    if w < 2 || h < 2 || (w as u64) * (h as u64) > 2_000_000 {
        return Err("Invalid raster dimensions".into());
    }
    let values: Vec<f64> = match decoder.read_image().map_err(|e| e.to_string())? {
        DecodingResult::F32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::F64(v) => v,
        DecodingResult::U8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I32(v) => v.into_iter().map(f64::from).collect(),
        _ => return Err("Unsupported elevation pixel type".into()),
    };
    Dem::new(w as usize, h as usize, values)
}

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
        let url = bounds.url(self.raster_pixels, self.raster_pixels)?;
        let (tx, rx) = mpsc::channel();
        self.download_rx = Some(rx);
        self.message = "Downloading USGS 3DEP elevation data…".into();
        thread::spawn(move || {
            let result = (|| -> Result<Vec<u8>, String> {
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(90))
                    .build()
                    .map_err(|e| e.to_string())?;
                let response = client
                    .get(url)
                    .send()
                    .map_err(|e| e.to_string())?
                    .error_for_status()
                    .map_err(|e| e.to_string())?;
                if let Some(len) = response.content_length() {
                    if len > 60_000_000 {
                        return Err("USGS response exceeds 60 MB".into());
                    }
                }
                let bytes = response.bytes().map_err(|e| e.to_string())?;
                if bytes.len() > 60_000_000 {
                    return Err("USGS response exceeds 60 MB".into());
                }
                if bytes.len() < 4 || !matches!(&bytes[..4], b"II*\0" | b"MM\0*") {
                    return Err(
                        "USGS did not return a classic TIFF; check service availability".into(),
                    );
                }
                Ok(bytes.to_vec())
            })();
            let _ = tx.send(result);
        });
        Ok(())
    }
    pub(crate) fn finish_download(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let dem = decode_tiff(&bytes)?;
        let (w, h) = (dem.width, dem.height);
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let file = Path::new(&self.output).join("usgs_elevation.tif");
        fs::write(&file, bytes).map_err(|e| e.to_string())?;
        self.path = file.to_string_lossy().into_owned();
        self.dem = Some(dem);
        self.message = format!(
            "Downloaded USGS elevation grid {} × {}. Source saved to {}",
            w, h, self.path
        );
        Ok(())
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
        let dem = match suffix.as_str() {
            "tif" | "tiff" => decode_tiff(&bytes)?,
            "asc" => {
                terraincraft_core::gis::read_asc(
                    &String::from_utf8(bytes).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
                .dem
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
                Dem::new(w, h, rows.into_iter().flatten().collect())?
            }
            _ => {
                return Err(
                    "Supported input formats: CSV, ESRI ASCII (.asc), uncompressed TIFF".into(),
                )
            }
        };
        self.message = format!("Loaded {} × {} elevation samples", dem.width, dem.height);
        self.dem = Some(dem);
        Ok(())
    }
    pub(crate) fn levels(&self, dem: &Dem) -> Vec<f64> {
        let (lo, hi) = dem.min_max();
        (1..=self.layer_count)
            .map(|i| lo + (hi - lo) * i as f64 / (self.layer_count + 1) as f64)
            .collect()
    }
}
