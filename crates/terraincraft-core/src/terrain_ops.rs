//! Elevation edits applied before meshing: UTM reprojection, smoothing and sea-level
//! clipping, combined in `prepare`.
use crate::georef::CrsUnits;
use crate::Dem;
use serde::{Deserialize, Serialize};

/// How a loaded DEM is processed before it is meshed or contoured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerrainPrep {
    /// Resample latitude/longitude data onto a UTM grid. Recommended for regions more
    /// than about a degree across, where a single longitude scale distorts the shape.
    pub utm: bool,
    /// Gaussian smoothing radius (standard deviation) in samples; 0 disables it.
    pub smooth_sigma: f64,
    /// Flatten everything at or below this elevation in meters (0 is sea level).
    pub sea_level_m: Option<f64>,
    /// How far below `sea_level_m` to place the flattened water surface, in meters.
    pub water_depth_m: f64,
}

impl Default for TerrainPrep {
    fn default() -> Self {
        Self {
            utm: false,
            smooth_sigma: 0.,
            sea_level_m: None,
            water_depth_m: 0.,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PrepReport {
    pub reprojected: bool,
    pub water_samples: usize,
}

/// Apply `prep` to a copy of `dem`: reproject first so smoothing is measured in output
/// samples, then smooth, then clip so the water surface stays exactly flat.
pub fn prepare(dem: &Dem, prep: &TerrainPrep) -> Result<(Dem, PrepReport), String> {
    let mut report = PrepReport::default();
    let mut out = if prep.utm && dem.georef.is_some_and(|g| g.units == CrsUnits::Degrees) {
        report.reprojected = true;
        crate::projection::reproject_to_utm(dem)?
    } else {
        dem.clone()
    };
    out.smooth(prep.smooth_sigma)?;
    if let Some(level) = prep.sea_level_m {
        report.water_samples = out.clip_sea_level(level, prep.water_depth_m)?;
    }
    Ok((out, report))
}

impl Dem {
    /// A coarser copy whose samples are at least `min_spacing_mm` apart on a model
    /// `model_width_mm` wide, or `None` if the grid is already that coarse. Detail finer
    /// than a printer or laser can reproduce only costs time: a 3000-sample grid across a
    /// 400 mm model is 24 million triangles at 0.13 mm spacing. The grid is smoothed in
    /// proportion first so detail averages out rather than aliasing, the outer samples
    /// stay on the same ground positions, and the georeference is updated to match.
    pub fn resample_for_model(&self, model_width_mm: f64, min_spacing_mm: f64) -> Option<Dem> {
        if !(min_spacing_mm > 0. && model_width_mm > 0.) {
            return None;
        }
        let spacing = model_width_mm / (self.width - 1) as f64;
        if spacing >= min_spacing_mm {
            return None;
        }
        let k = min_spacing_mm / spacing;
        let w = ((self.width - 1) as f64 / k).floor() as usize + 1;
        let h = ((self.height - 1) as f64 / k).floor() as usize + 1;
        if w < 2 || h < 2 {
            return None;
        }
        let mut smooth = self.clone();
        smooth.smooth(k * 0.5).ok()?;
        let (fx, fy) = (
            (self.width - 1) as f64 / (w - 1) as f64,
            (self.height - 1) as f64 / (h - 1) as f64,
        );
        let mut heights = Vec::with_capacity(w * h);
        for j in 0..h {
            for i in 0..w {
                let (x, y) = (i as f64 * fx, j as f64 * fy);
                let (x0, y0) = (
                    (x as usize).min(self.width - 2),
                    (y as usize).min(self.height - 2),
                );
                let (u, v) = (x - x0 as f64, y - y0 as f64);
                let top = smooth.at(x0, y0) * (1. - u) + smooth.at(x0 + 1, y0) * u;
                let bottom = smooth.at(x0, y0 + 1) * (1. - u) + smooth.at(x0 + 1, y0 + 1) * u;
                heights.push(top * (1. - v) + bottom * v);
            }
        }
        let mut out = Dem::new(w, h, heights).ok()?;
        out.georef = self.georef.map(|g| {
            let (pw, ph) = (g.pixel_width * fx, g.pixel_height * fy);
            crate::georef::GeoReference {
                // Keep the first sample center where it was.
                origin_x: g.origin_x + (g.pixel_width - pw) * 0.5,
                origin_y: g.origin_y - (g.pixel_height - ph) * 0.5,
                pixel_width: pw,
                pixel_height: ph,
                ..g
            }
        });
        Some(out)
    }

    /// Gaussian smoothing with standard deviation `sigma` in samples. Edges are clamped,
    /// so the outline of the grid is unchanged. `sigma` of 0 leaves the grid as is.
    pub fn smooth(&mut self, sigma: f64) -> Result<(), String> {
        if !sigma.is_finite() || sigma < 0. {
            return Err("Smoothing radius must be zero or a positive number".into());
        }
        if sigma == 0. {
            return Ok(());
        }
        let radius = (sigma * 3.).ceil() as isize;
        let mut kernel: Vec<f64> = (-radius..=radius)
            .map(|i| (-(i * i) as f64 / (2. * sigma * sigma)).exp())
            .collect();
        let total: f64 = kernel.iter().sum();
        kernel.iter_mut().for_each(|k| *k /= total);
        let (w, h) = (self.width as isize, self.height as isize);
        let pass = |src: &[f64], horizontal: bool| -> Vec<f64> {
            let mut out = vec![0.; src.len()];
            for y in 0..h {
                for x in 0..w {
                    out[(y * w + x) as usize] = kernel
                        .iter()
                        .zip(-radius..=radius)
                        .map(|(k, d)| {
                            let (sx, sy) = if horizontal {
                                ((x + d).clamp(0, w - 1), y)
                            } else {
                                (x, (y + d).clamp(0, h - 1))
                            };
                            k * src[(sy * w + sx) as usize]
                        })
                        .sum();
                }
            }
            out
        };
        let horizontal = pass(&self.heights, true);
        self.heights = pass(&horizontal, false);
        Ok(())
    }

    /// Flatten everything at or below `level_m` (sea level is 0) to a water surface
    /// `water_depth_m` below that level. A positive depth recesses the water so the
    /// coastline stays visible at true scale. Returns the number of water samples.
    pub fn clip_sea_level(&mut self, level_m: f64, water_depth_m: f64) -> Result<usize, String> {
        if !level_m.is_finite() || !water_depth_m.is_finite() || water_depth_m < 0. {
            return Err("Sea level must be finite and water depth zero or positive".into());
        }
        let surface = level_m - water_depth_m;
        let mut count = 0;
        for v in &mut self.heights {
            if *v <= level_m {
                *v = surface;
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use crate::Dem;
    #[test]
    fn smoothing_preserves_flat_and_mean_and_reduces_spike() {
        let mut flat = Dem::new(5, 5, vec![7.; 25]).unwrap();
        flat.smooth(1.5).unwrap();
        assert!(flat.heights.iter().all(|v| (v - 7.).abs() < 1e-12));
        let mut spike = vec![0.; 81];
        spike[40] = 81.;
        let mut d = Dem::new(9, 9, spike).unwrap();
        d.smooth(1.).unwrap();
        assert!(d.heights[40] < 20. && d.heights[40] > 10.);
        // Mass is conserved when the kernel stays inside the grid.
        assert!((d.heights.iter().sum::<f64>() - 81.).abs() < 1e-6);
        assert!(d.smooth(-1.).is_err());
    }
    #[test]
    fn prepare_runs_steps_in_order() {
        use super::{prepare, TerrainPrep};
        let d = Dem::new(2, 2, vec![-1., 0., 10., 20.]).unwrap();
        let prep = TerrainPrep {
            sea_level_m: Some(0.),
            water_depth_m: 2.,
            ..TerrainPrep::default()
        };
        let (out, report) = prepare(&d, &prep).unwrap();
        assert_eq!(out.heights, vec![-2., -2., 10., 20.]);
        assert_eq!((report.reprojected, report.water_samples), (false, 2));
        // UTM is skipped, not an error, for data that is not latitude/longitude.
        let utm = TerrainPrep {
            utm: true,
            ..TerrainPrep::default()
        };
        assert!(!prepare(&d, &utm).unwrap().1.reprojected);
    }
    #[test]
    fn resampling_keeps_extent_shape_and_georeference() {
        use crate::georef::{CrsUnits, GeoReference};
        // A smooth slope: resampling should reproduce it closely.
        let (w, h) = (301, 201);
        let v = (0..w * h)
            .map(|i| (i % w) as f64 * 2. + (i / w) as f64)
            .collect();
        let mut d = Dem::new(w, h, v).unwrap();
        d.georef = Some(GeoReference {
            epsg: Some(32610),
            units: CrsUnits::Meters,
            origin_x: 1000.,
            origin_y: 5000.,
            pixel_width: 10.,
            pixel_height: 10.,
        });
        // 300 intervals over 60 mm is 0.2 mm; ask for 0.5 mm.
        let r = d.resample_for_model(60., 0.5).unwrap();
        assert_eq!((r.width, r.height), (121, 81));
        let (g0, g1) = (d.georef.unwrap(), r.georef.unwrap());
        assert_eq!(g0.sample_span_m(w, h), g1.sample_span_m(r.width, r.height));
        assert_eq!(g0.sample_to_latlon(0., 0.), g1.sample_to_latlon(0., 0.));
        // Interior samples match the plane (edges are pulled slightly by smoothing).
        assert!((r.at(60, 40) - (150. * 2. + 100.)).abs() < 1e-6);
        assert!(d.resample_for_model(600., 0.5).is_none());
    }
    #[test]
    fn clips_water_to_recessed_surface() {
        let mut d = Dem::new(2, 2, vec![-2., 0., 0.5, 30.]).unwrap();
        assert_eq!(d.clip_sea_level(0., 10.).unwrap(), 2);
        assert_eq!(d.heights, vec![-10., -10., 0.5, 30.]);
        assert!(d.clip_sea_level(0., -1.).is_err());
    }
}
