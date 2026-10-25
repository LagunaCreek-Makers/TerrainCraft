//! USGS 3DEP geographic selection and safe raster request construction.
//! Geographic coordinates are WGS84 degrees; longitude is X, latitude is Y.
/// Request limits kept well inside what the 3DEP ImageServer will return.
const MAX_SIDE: u32 = 2048;
const MAX_SAMPLES: u64 = 2_000_000;

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
        for_print.min(native).clamp(16., MAX_SIDE as f64).round() as u32
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
