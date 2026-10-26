//! USGS 3DEP geographic selection and safe raster request construction.
//! Geographic coordinates are WGS84 degrees; longitude is X, latitude is Y.
use crate::georef::{CrsUnits, GeoReference};
use crate::Dem;

/// Per-request limits kept well inside what the 3DEP ImageServer will return.
const MAX_SIDE: u32 = 2048;
const MAX_SAMPLES: u64 = 2_000_000;
/// Side of each request in a tiled download. A merged remainder can add up to
/// MIN_TILE, and (1380 + 16)² still stays under MAX_SAMPLES.
const TILE_SIDE: u32 = 1380;
const MIN_TILE: u32 = 16;
/// Limits for a whole tiled download.
pub const MAX_DOWNLOAD_SIDE: u32 = 8192;
const MAX_DOWNLOAD_SAMPLES: u64 = 16_000_000;

/// One request of a tiled download, and where its pixels go in the full raster.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadTile {
    pub bounds: Bounds,
    pub col0: u32,
    pub row0: u32,
    pub width: u32,
    pub height: u32,
}

/// A download split into service-sized requests on one shared pixel grid.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadPlan {
    pub width: u32,
    pub height: u32,
    /// Square pixel size in degrees, shared by every tile so they abut exactly.
    pub pixel_deg: f64,
    pub west: f64,
    pub north: f64,
    pub tiles: Vec<DownloadTile>,
}

impl DownloadPlan {
    /// Assemble decoded tiles (in `tiles` order) into one georeferenced DEM. Each
    /// tile must have its planned size and, if georeferenced, sit where it was planned.
    pub fn stitch(&self, parts: &[Dem]) -> Result<Dem, String> {
        if parts.len() != self.tiles.len() {
            return Err("Tile count does not match the download plan".into());
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let mut heights = vec![0.; w * h];
        for (t, part) in self.tiles.iter().zip(parts) {
            if (part.width, part.height) != (t.width as usize, t.height as usize) {
                return Err(format!(
                    "Tile at column {} row {} is {} x {}, expected {} x {}",
                    t.col0, t.row0, part.width, part.height, t.width, t.height
                ));
            }
            if let Some(g) = part.georef {
                let off_x = (g.origin_x - t.bounds.west) / self.pixel_deg;
                let off_y = (g.origin_y - t.bounds.north) / self.pixel_deg;
                if off_x.abs() > 0.01 || off_y.abs() > 0.01 {
                    return Err(format!(
                        "Tile at column {} row {} is offset by {off_x:.3}, {off_y:.3} pixels",
                        t.col0, t.row0
                    ));
                }
            }
            for row in 0..part.height {
                let dst = (t.row0 as usize + row) * w + t.col0 as usize;
                heights[dst..dst + part.width]
                    .copy_from_slice(&part.heights[row * part.width..(row + 1) * part.width]);
            }
        }
        let mut dem = Dem::new(w, h, heights)?;
        dem.georef = Some(GeoReference {
            epsg: Some(4326),
            units: CrsUnits::Degrees,
            origin_x: self.west,
            origin_y: self.north,
            pixel_width: self.pixel_deg,
            pixel_height: self.pixel_deg,
        });
        Ok(dem)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}
impl Bounds {
    pub fn validate(&self) -> Result<(), String> {
        if [self.west, self.south, self.east, self.north]
            .iter()
            .any(|v| !v.is_finite())
            || self.west < -180.
            || self.east > 180.
            || self.south < -90.
            || self.north > 90.
            || self.west >= self.east
            || self.south >= self.north
        {
            return Err(
                "Invalid WGS84 bounds; antimeridian-spanning selections are not yet supported"
                    .into(),
            );
        }
        if self.east - self.west > 5. || self.north - self.south > 5. {
            return Err("Selection exceeds 5 degrees per axis; select a smaller region".into());
        }
        Ok(())
    }
    pub fn approx_aspect(&self) -> f64 {
        let lat = (self.south + self.north) * 0.5;
        ((self.east - self.west) * lat.to_radians().cos().abs()) / (self.north - self.south)
    }
    /// Raster size whose aspect matches this selection in degrees, so the service
    /// returns exactly these bounds. (It keeps pixels square in the output coordinate
    /// system and widens the extent to fit any other aspect.) `long_side` is the pixel
    /// count along the longer axis; the result is scaled down to the service limits.
    pub fn request_size(&self, long_side: u32) -> Result<(u32, u32), String> {
        self.validate()?;
        let aspect = (self.east - self.west) / (self.north - self.south);
        let mut long = long_side.clamp(16, MAX_SIDE) as f64;
        let fit = (MAX_SAMPLES as f64 / (long * long / aspect.max(1. / aspect))).sqrt();
        if fit < 1. {
            long = (long * fit).floor();
        }
        let short = (long / aspect.max(1. / aspect)).round();
        if short < 16. {
            return Err("Selection is too narrow; widen the shorter side".into());
        }
        let (w, h) = if aspect >= 1. {
            (long, short)
        } else {
            (short, long)
        };
        Ok((w as u32, h as u32))
    }
    /// Long-side pixel count that gives about `mm_per_sample` printed spacing for a model
    /// whose longer side is `model_long_side_mm`, without going finer than the ~10 m
    /// native 3DEP coverage available nationwide.
    pub fn suggested_long_side(&self, model_long_side_mm: f64, mm_per_sample: f64) -> u32 {
        let lat = (self.south + self.north) * 0.5;
        let (m_lon, m_lat) = crate::georef::meters_per_degree(lat);
        let ground = ((self.east - self.west) * m_lon).max((self.north - self.south) * m_lat);
        let for_print = model_long_side_mm / mm_per_sample.max(0.05);
        let native = ground / 10.;
        for_print
            .min(native)
            .clamp(16., MAX_DOWNLOAD_SIDE as f64)
            .round() as u32
    }
    /// Split a download with `long_side` pixels on the longer axis into requests the
    /// service accepts. All tiles share one square pixel size and their bounds are exact
    /// multiples of it, so the pieces line up with no resampling. The covered area may
    /// differ from the selection by under one pixel on the east and south edges.
    pub fn plan_download(&self, long_side: u32) -> Result<DownloadPlan, String> {
        self.validate()?;
        let (dx, dy) = (self.east - self.west, self.north - self.south);
        let mut long = long_side.clamp(16, MAX_DOWNLOAD_SIDE) as f64;
        let ratio = dx.max(dy) / dx.min(dy);
        let fit = (MAX_DOWNLOAD_SAMPLES as f64 * ratio).sqrt();
        long = long.min(fit.floor());
        let pixel_deg = dx.max(dy) / long;
        let (width, height) = (
            (dx / pixel_deg).round() as u32,
            (dy / pixel_deg).round() as u32,
        );
        if width.min(height) < 16 {
            return Err("Selection is too narrow; widen the shorter side".into());
        }
        let mut tiles = Vec::new();
        for (row0, h) in splits(height) {
            for (col0, w) in splits(width) {
                tiles.push(DownloadTile {
                    bounds: Bounds {
                        west: self.west + col0 as f64 * pixel_deg,
                        east: self.west + (col0 + w) as f64 * pixel_deg,
                        north: self.north - row0 as f64 * pixel_deg,
                        south: self.north - (row0 + h) as f64 * pixel_deg,
                    },
                    col0,
                    row0,
                    width: w,
                    height: h,
                });
            }
        }
        Ok(DownloadPlan {
            width,
            height,
            pixel_deg,
            west: self.west,
            north: self.north,
            tiles,
        })
    }
    pub fn url(&self, width: u32, height: u32) -> Result<String, String> {
        self.validate()?;
        if !(16..=MAX_SIDE).contains(&width)
            || !(16..=MAX_SIDE).contains(&height)
            || (width as u64) * (height as u64) > MAX_SAMPLES
        {
            return Err("Requested raster size outside supported limits".into());
        }
        Ok(format!("https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage?bbox={},{},{},{}&bboxSR=4326&imageSR=4326&size={},{}&format=tiff&pixelType=F32&interpolation=RSP_BilinearInterpolation&f=image",self.west,self.south,self.east,self.north,width,height))
    }
}
/// Download `bounds` from the 3DEP ImageServer at `long_side` pixels on the longer axis,
/// in as many requests as needed, and stitch them into one georeferenced DEM.
/// `progress(done, total)` is called after each request.
#[cfg(feature = "download")]
pub fn download(
    bounds: &Bounds,
    long_side: u32,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<crate::geotiff::DecodedRaster, String> {
    let plan = bounds.plan_download(long_side)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("TerrainCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut parts = Vec::with_capacity(plan.tiles.len());
    let mut filled_cells = 0;
    for (i, t) in plan.tiles.iter().enumerate() {
        let url = t.bounds.url(t.width, t.height)?;
        let response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("USGS request {} of {}: {e}", i + 1, plan.tiles.len()))?;
        if response.content_length().unwrap_or(0) > 60_000_000 {
            return Err("USGS response exceeds 60 MB".into());
        }
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        if bytes.len() < 4 || !matches!(&bytes[..4], b"II*\0" | b"MM\0*") {
            return Err("USGS did not return a TIFF; check service availability".into());
        }
        let r = crate::geotiff::decode(&bytes)?;
        filled_cells += r.filled_cells;
        parts.push(r.dem);
        progress(i + 1, plan.tiles.len());
    }
    Ok(crate::geotiff::DecodedRaster {
        dem: plan.stitch(&parts)?,
        filled_cells,
    })
}

/// Latitude/longitude box around a georeferenced DEM (from its corner samples).
pub fn dem_bounds(dem: &Dem) -> Option<Bounds> {
    let g = dem.georef?;
    let (w, h) = ((dem.width - 1) as f64, (dem.height - 1) as f64);
    let corners: Vec<(f64, f64)> = [(0., 0.), (w, 0.), (0., h), (w, h)]
        .iter()
        .filter_map(|&(c, r)| g.sample_to_latlon(c, r))
        .collect();
    (corners.len() == 4).then(|| Bounds {
        west: corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
        east: corners
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max),
        south: corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        north: corners
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max),
    })
}

/// USGS Geographic Names (GNIS) landform classes worth labeling on engravings.
pub const LANDFORM_CLASSES: [&str; 4] = ["Summit", "Pillar", "Gap", "Ridge"];

/// Query URL for named landforms of `classes` in `bounds` from The National Map's
/// geographic names service (USGS GNIS, public domain), one page of up to 2000 results.
pub fn place_names_url(bounds: &Bounds, classes: &[&str], offset: usize) -> Result<String, String> {
    bounds.validate()?;
    if classes.is_empty()
        || classes
            .iter()
            .any(|c| !c.chars().all(|ch| ch.is_ascii_alphabetic()))
    {
        return Err("Feature classes must be plain words".into());
    }
    let list = classes
        .iter()
        .map(|c| format!("%27{c}%27"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "https://carto.nationalmap.gov/arcgis/rest/services/geonames/MapServer/5/query?where=gaz_featureclass+IN+({list})&geometry={},{},{},{}&geometryType=esriGeometryEnvelope&inSR=4326&spatialRel=esriSpatialRelIntersects&outFields=gaz_name,gaz_featureclass&returnGeometry=true&outSR=4326&resultOffset={offset}&resultRecordCount=2000&f=json",
        bounds.west, bounds.south, bounds.east, bounds.north
    ))
}

/// Named features from one page of the names service: name, class (as the landmark
/// symbol, lowercase) and the first point. Elevations come from the DEM when drawn.
pub fn parse_place_names(json: &str) -> Result<(Vec<crate::landmarks::Landmark>, bool), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Names service: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(format!("Names service error: {e}"));
    }
    let mut out = Vec::new();
    for f in v["features"]
        .as_array()
        .ok_or("Names service returned no features list")?
    {
        let name = f["attributes"]["gaz_name"].as_str().unwrap_or("").trim();
        let class = f["attributes"]["gaz_featureclass"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let p = &f["geometry"]["points"][0];
        let (Some(lon), Some(lat)) = (p[0].as_f64(), p[1].as_f64()) else {
            continue;
        };
        if !name.is_empty() {
            out.push(crate::landmarks::Landmark {
                name: name.to_string(),
                lat,
                lon,
                elevation_m: None,
                symbol: class,
            });
        }
    }
    let more = v["exceededTransferLimit"].as_bool().unwrap_or(false);
    Ok((out, more))
}

/// Download named landforms of `classes` inside `bounds`, following result pages.
#[cfg(feature = "download")]
pub fn download_place_names(
    bounds: &Bounds,
    classes: &[&str],
) -> Result<Vec<crate::landmarks::Landmark>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(concat!("TerrainCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    for page in 0..20 {
        let url = place_names_url(bounds, classes, page * 2000)?;
        let text = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("Names service: {e}"))?;
        let (features, more) = parse_place_names(&text)?;
        all.extend(features);
        if !more {
            break;
        }
    }
    Ok(all)
}

/// Split `total` pixels into (start, length) runs of TILE_SIDE, folding a remainder
/// shorter than MIN_TILE into the last run so no request is degenerate.
fn splits(total: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < total {
        let rest = total - start;
        let len = if rest <= TILE_SIDE + MIN_TILE {
            rest
        } else {
            TILE_SIDE
        };
        out.push((start, len));
        start += len;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid() {
        assert!(Bounds {
            west: -120.,
            south: 38.,
            east: -121.,
            north: 39.
        }
        .validate()
        .is_err());
        assert!(Bounds {
            west: 170.,
            south: 38.,
            east: 190.,
            north: 39.
        }
        .validate()
        .is_err());
    }
    #[test]
    fn request() {
        let b = Bounds {
            west: -121.6,
            south: 38.4,
            east: -121.4,
            north: 38.6,
        };
        assert!(b.url(512, 512).unwrap().contains("bboxSR=4326"));
        assert!(b.url(4096, 4096).is_err());
        assert!(b.approx_aspect() > 0.7);
    }
    #[test]
    fn request_size_matches_degree_aspect() {
        let b = Bounds {
            west: -121.85,
            south: 46.8,
            east: -121.7,
            north: 46.9,
        };
        assert_eq!(b.request_size(450).unwrap(), (450, 300));
        let tall = Bounds {
            west: 0.,
            south: 0.,
            east: 0.1,
            north: 0.2,
        };
        assert_eq!(tall.request_size(400).unwrap(), (200, 400));
        let (w, h) = Bounds {
            west: 0.,
            south: 0.,
            east: 1.,
            north: 1.,
        }
        .request_size(4000)
        .unwrap();
        assert!(w == h && (w as u64 * h as u64) <= MAX_SAMPLES);
        let sliver = Bounds {
            west: 0.,
            south: 0.,
            east: 1.,
            north: 0.001,
        };
        assert!(sliver.request_size(2048).is_err());
    }
    #[test]
    fn plans_tiles_on_one_grid() {
        let b = Bounds {
            west: -122.,
            south: 46.,
            east: -121.,
            north: 46.75,
        };
        let p = b.plan_download(4000).unwrap();
        assert_eq!((p.width, p.height), (4000, 3000));
        assert_eq!(p.tiles.len(), 9);
        for t in &p.tiles {
            assert!(t.width >= MIN_TILE && t.height >= MIN_TILE);
            assert!(t.width as u64 * t.height as u64 <= MAX_SAMPLES);
            // Each request's aspect matches its pixel shape, so pixels stay square.
            let px = (t.bounds.east - t.bounds.west) / t.width as f64;
            let py = (t.bounds.north - t.bounds.south) / t.height as f64;
            assert!((px - p.pixel_deg).abs() < 1e-12 && (py - p.pixel_deg).abs() < 1e-12);
        }
        let covered: u64 = p
            .tiles
            .iter()
            .map(|t| t.width as u64 * t.height as u64)
            .sum();
        assert_eq!(covered, 4000 * 3000);
        // Small selections are a single request.
        assert_eq!(b.plan_download(500).unwrap().tiles.len(), 1);
        // Totals are capped.
        let big = Bounds {
            west: 0.,
            south: 0.,
            east: 5.,
            north: 5.,
        }
        .plan_download(8192)
        .unwrap();
        assert!(big.width as u64 * big.height as u64 <= MAX_DOWNLOAD_SAMPLES);
        assert_eq!(splits(1390), vec![(0, 1390)]);
        assert_eq!(splits(2770), vec![(0, 1380), (1380, 1390)]);
    }
    #[test]
    fn stitches_tiles_and_rejects_misalignment() {
        let b = Bounds {
            west: 0.,
            south: 0.,
            east: 3.,
            north: 0.1,
        };
        let p = b.plan_download(3000).unwrap();
        assert_eq!(p.tiles.len(), 3);
        let parts: Vec<Dem> = p
            .tiles
            .iter()
            .map(|t| {
                let v = (0..t.width * t.height)
                    .map(|i| (t.col0 + i % t.width) as f64)
                    .collect();
                let mut d = Dem::new(t.width as usize, t.height as usize, v).unwrap();
                d.georef = Some(GeoReference {
                    epsg: Some(4326),
                    units: CrsUnits::Degrees,
                    origin_x: t.bounds.west,
                    origin_y: t.bounds.north,
                    pixel_width: p.pixel_deg,
                    pixel_height: p.pixel_deg,
                });
                d
            })
            .collect();
        let d = p.stitch(&parts).unwrap();
        assert_eq!((d.width, d.height), (3000, 100));
        assert!((0..3000).all(|x| d.at(x, 57) == x as f64));
        let mut shifted = parts.clone();
        shifted[1].georef.as_mut().unwrap().origin_x += p.pixel_deg * 0.5;
        assert!(p.stitch(&shifted).is_err());
    }
    #[test]
    fn place_names_query_and_parse() {
        let b = Bounds {
            west: -121.85,
            south: 46.8,
            east: -121.7,
            north: 46.9,
        };
        let url = place_names_url(&b, &["Summit", "Gap"], 0).unwrap();
        assert!(
            url.contains("IN+(%27Summit%27,%27Gap%27)") && url.contains("-121.85,46.8,-121.7,46.9")
        );
        assert!(place_names_url(&b, &["Summit'; drop"], 0).is_err());
        let json = r#"{"features":[
            {"attributes":{"gaz_name":"Little Tahoma Peak","gaz_featureclass":"Summit"},
             "geometry":{"points":[[-121.7114,46.8494]]}},
            {"attributes":{"gaz_name":"","gaz_featureclass":"Summit"},
             "geometry":{"points":[[-121.7,46.8]]}}],
            "exceededTransferLimit":true}"#;
        let (names, more) = parse_place_names(json).unwrap();
        assert!(more);
        assert_eq!(names.len(), 1);
        assert_eq!(
            (names[0].name.as_str(), names[0].symbol.as_str()),
            ("Little Tahoma Peak", "summit")
        );
        assert!(parse_place_names(r#"{"error":{"code":400}}"#).is_err());
    }
    #[test]
    fn suggested_resolution() {
        let b = Bounds {
            west: -121.85,
            south: 46.8,
            east: -121.7,
            north: 46.9,
        };
        // 300 mm at 0.5 mm per sample; the ~11.4 km area supports ~1144 at 10 m.
        assert_eq!(b.suggested_long_side(300., 0.5), 600);
        assert_eq!(b.suggested_long_side(1000., 0.5), 1144);
    }
}
