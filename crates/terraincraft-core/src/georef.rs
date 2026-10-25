//! Raster georeferencing and ground-distance estimates for north-up grids.

/// Units of the raster's coordinate reference system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrsUnits {
    /// Geographic latitude/longitude, e.g. EPSG:4326.
    Degrees,
    /// Projected coordinates in meters, e.g. UTM.
    Meters,
}

/// Placement of a north-up raster in its coordinate reference system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoReference {
    pub epsg: Option<u16>,
    pub units: CrsUnits,
    /// CRS coordinates of the outer top-left corner of the top-left pixel.
    pub origin_x: f64,
    pub origin_y: f64,
    /// Pixel width and height in CRS units. Both positive; rows run southward.
    pub pixel_width: f64,
    pub pixel_height: f64,
}

impl GeoReference {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.origin_x,
            self.origin_y,
            self.pixel_width,
            self.pixel_height,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.pixel_width <= 0.
            || self.pixel_height <= 0.
        {
            return Err("Invalid raster georeference".into());
        }
        Ok(())
    }

    /// Outer `[west, south, east, north]` bounds of a `width` × `height` raster.
    pub fn bounds(&self, width: usize, height: usize) -> [f64; 4] {
        [
            self.origin_x,
            self.origin_y - height as f64 * self.pixel_height,
            self.origin_x + width as f64 * self.pixel_width,
            self.origin_y,
        ]
    }

    /// Ground distance in meters between the first and last sample centers along
    /// x and y. This is the span a mesh covers, since it places one vertex per sample.
    /// Geographic rasters use the WGS84 scale at the center latitude, which is accurate
    /// for regions a few degrees across and approximate beyond that.
    pub fn sample_span_m(&self, width: usize, height: usize) -> (f64, f64) {
        let dx = (width.saturating_sub(1)) as f64 * self.pixel_width;
        let dy = (height.saturating_sub(1)) as f64 * self.pixel_height;
        match self.units {
            CrsUnits::Meters => (dx, dy),
            CrsUnits::Degrees => {
                let [_, south, _, north] = self.bounds(width, height);
                let (m_lon, m_lat) = meters_per_degree((south + north) * 0.5);
                (dx * m_lon, dy * m_lat)
            }
        }
    }
}

/// Meters per degree of longitude and of latitude on the WGS84 ellipsoid.
pub fn meters_per_degree(lat_deg: f64) -> (f64, f64) {
    const A: f64 = 6_378_137.;
    const E2: f64 = 0.006_694_379_990_14;
    let phi = lat_deg.to_radians();
    let w = (1. - E2 * phi.sin().powi(2)).sqrt();
    let prime_vertical = A / w;
    let meridional = A * (1. - E2) / (w * w * w);
    let per_rad = std::f64::consts::PI / 180.;
    (prime_vertical * phi.cos() * per_rad, meridional * per_rad)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meters_per_degree_matches_reference_values() {
        let (lon, lat) = meters_per_degree(0.);
        assert!((lon - 111_319.5).abs() < 1.);
        assert!((lat - 110_574.3).abs() < 1.);
        let (lon, lat) = meters_per_degree(45.);
        assert!((lon - 78_846.8).abs() < 1.);
        assert!((lat - 111_131.8).abs() < 1.);
    }
    #[test]
    fn geographic_span_and_bounds() {
        let g = GeoReference {
            epsg: Some(4326),
            units: CrsUnits::Degrees,
            origin_x: -121.85,
            origin_y: 46.9,
            pixel_width: 0.15 / 450.,
            pixel_height: 0.1 / 300.,
        };
        let b = g.bounds(450, 300);
        assert!((b[2] + 121.7).abs() < 1e-9 && (b[1] - 46.8).abs() < 1e-9);
        let (x, y) = g.sample_span_m(450, 300);
        // ~0.15° of longitude and ~0.1° of latitude at 46.85°N, less one pixel.
        assert!((x - 11_406.).abs() < 30., "{x}");
        assert!((y - 11_081.).abs() < 30., "{y}");
    }
    #[test]
    fn projected_span() {
        let g = GeoReference {
            epsg: Some(32610),
            units: CrsUnits::Meters,
            origin_x: 0.,
            origin_y: 0.,
            pixel_width: 10.,
            pixel_height: 10.,
        };
        assert_eq!(g.sample_span_m(101, 51), (1000., 500.));
    }
}
