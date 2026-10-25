//! Configurable XYZ and WMS map provider URL construction. No automatic tile scraping.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapProvider {
    pub name: String,
    pub url_template: String,
    pub attribution: String,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub api_key: Option<String>,
}
impl MapProvider {
    pub fn xyz_url(&self, z: u8, x: u32, y: u32) -> Result<String, String> {
        if z < self.min_zoom || z > self.max_zoom || z > 22 {
            return Err("Zoom outside provider range".into());
        }
        let n = 1_u64 << z;
        if x as u64 >= n || y as u64 >= n {
            return Err("Invalid XYZ tile coordinates".into());
        }
        if !self.url_template.starts_with("https://")
            && !self.url_template.starts_with("http://127.0.0.1:")
        {
            return Err("Map source must use HTTPS (or loopback HTTP)".into());
        }
        for required in ["{z}", "{x}", "{y}"] {
            if !self.url_template.contains(required) {
                return Err(format!("Missing tile placeholder {required}"));
            }
        }
        let mut url = self
            .url_template
            .replace("{z}", &z.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string());
        if url.contains("{api_key}") {
            let key = self
                .api_key
                .as_ref()
                .ok_or("Map provider API key is required")?;
            url = url.replace("{api_key}", key);
        }
        Ok(url)
    }
}
pub fn builtins() -> Vec<MapProvider> {
    vec![
        MapProvider {
            name: "OpenStreetMap Standard (interactive only)".into(),
            url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),
            attribution: "© OpenStreetMap contributors".into(),
            min_zoom: 0,
            max_zoom: 19,
            api_key: None,
        },
        MapProvider {
            name: "OpenTopoMap".into(),
            url_template: "https://a.tile.opentopomap.org/{z}/{x}/{y}.png".into(),
            attribution: "© OpenTopoMap contributors; © OpenStreetMap contributors".into(),
            min_zoom: 0,
            max_zoom: 17,
            api_key: None,
        },
        MapProvider {
            name: "Custom XYZ (configure URL and terms)".into(),
            url_template: "https://example.org/tiles/{z}/{x}/{y}.png".into(),
            attribution: "Provider attribution required".into(),
            min_zoom: 0,
            max_zoom: 19,
            api_key: None,
        },
    ]
}
pub fn latlon_to_xyz(lat: f64, lon: f64, z: u8) -> Result<(u32, u32), String> {
    if !lat.is_finite()
        || !lon.is_finite()
        || lat.abs() > 85.05112878
        || lon < -180.
        || lon > 180.
        || z > 22
    {
        return Err("Invalid Web Mercator coordinates".into());
    }
    let n = (1_u64 << z) as f64;
    let latr = lat.to_radians();
    let x = ((lon + 180.) / 360. * n).floor().clamp(0., n - 1.) as u32;
    let y = ((1. - (latr.tan() + 1. / latr.cos()).ln() / std::f64::consts::PI) / 2. * n)
        .floor()
        .clamp(0., n - 1.) as u32;
    Ok((x, y))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tile_url() {
        let p = &builtins()[0];
        assert_eq!(
            p.xyz_url(0, 0, 0).unwrap(),
            "https://tile.openstreetmap.org/0/0/0.png"
        );
        assert!(p.xyz_url(1, 2, 0).is_err());
    }
    #[test]
    fn coordinates() {
        assert_eq!(latlon_to_xyz(0., 0., 1).unwrap(), (1, 1));
    }
}

/// TMS services number rows from the south; XYZ numbers rows from the north.
pub fn xyz_to_tms_y(z: u8, y: u32) -> Result<u32, String> {
    if z > 22 || (y as u64) >= (1_u64 << z) {
        return Err("Invalid tile row".into());
    }
    Ok(((1_u64 << z) - 1 - y as u64) as u32)
}
/// Bing-compatible quadkey addressing (address calculation only; not an API integration).
pub fn quadkey(z: u8, x: u32, y: u32) -> Result<String, String> {
    if z > 22 || (x as u64) >= (1_u64 << z) || (y as u64) >= (1_u64 << z) {
        return Err("Invalid quadkey coordinates".into());
    }
    let mut result = String::new();
    for level in (1..=z).rev() {
        let bit = 1_u32 << (level - 1);
        let digit = ((x & bit != 0) as u8) + 2 * ((y & bit != 0) as u8);
        result.push(char::from(b'0' + digit));
    }
    Ok(result)
}
/// URL generation for TMS tile services; requests are not made by this function.
pub fn tms_url(provider: &MapProvider, z: u8, x: u32, y: u32) -> Result<String, String> {
    provider.xyz_url(z, x, xyz_to_tms_y(z, y)?)
}
#[cfg(test)]
mod extra_tests {
    use super::*;
    #[test]
    fn tms_rows() {
        assert_eq!(xyz_to_tms_y(2, 0).unwrap(), 3);
        assert_eq!(xyz_to_tms_y(2, 3).unwrap(), 0);
    }
    #[test]
    fn quadkeys() {
        assert_eq!(quadkey(3, 3, 5).unwrap(), "213");
        assert!(quadkey(2, 4, 0).is_err());
    }
}

/// Normalized Web Mercator coordinates, with x/y in [0,1].
pub fn latlon_to_world(lat: f64, lon: f64, _z: u8) -> (f64, f64) {
    let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
    (
        ((lon + 180.) / 360.).clamp(0., 1.),
        (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2.,
    )
}
pub fn world_to_latlon(x: f64, y: f64) -> (f64, f64) {
    let y = y.clamp(0., 1.);
    let lat = (std::f64::consts::PI * (1. - 2. * y))
        .sinh()
        .atan()
        .to_degrees();
    (lat, x.clamp(0., 1.) * 360. - 180.)
}
#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for (lat, lon) in [(0., 0.), (38.58, -121.5), (-33.9, 151.2)] {
            let (x, y) = latlon_to_world(lat, lon, 10);
            let (a, b) = world_to_latlon(x, y);
            assert!((a - lat).abs() < 1e-8);
            assert!((b - lon).abs() < 1e-8);
        }
    }
}
