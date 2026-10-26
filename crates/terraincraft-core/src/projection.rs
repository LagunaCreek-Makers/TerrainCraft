//! WGS84 Universal Transverse Mercator, and resampling geographic DEMs onto a UTM grid.
//!
//! Uses Krüger's series to sixth order in n (Karney 2011, "Transverse Mercator with an
//! accuracy of a few nanometers"), which is far more accurate than needed anywhere in a zone.
use crate::georef::{CrsUnits, GeoReference};
use crate::Dem;

const A: f64 = 6_378_137.;
const F: f64 = 1. / 298.257_223_563;
const K0: f64 = 0.9996;
const FALSE_EASTING: f64 = 500_000.;
const FALSE_NORTHING_SOUTH: f64 = 10_000_000.;

/// A UTM zone on the WGS84 ellipsoid (EPSG 326xx north, 327xx south).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtmZone {
    pub zone: u8,
    pub north: bool,
}

impl UtmZone {
    /// Standard zone containing a point (Norway/Svalbard exceptions are not applied).
    pub fn containing(lat: f64, lon: f64) -> Self {
        let zone = (((lon + 180.) / 6.).floor() as i32).rem_euclid(60) + 1;
        Self {
            zone: zone as u8,
            north: lat >= 0.,
        }
    }
    pub fn epsg(&self) -> u16 {
        (if self.north { 32600 } else { 32700 }) + self.zone as u16
    }
    fn central_meridian(&self) -> f64 {
        -183. + 6. * self.zone as f64
    }

    /// Latitude/longitude in degrees to (easting, northing) in meters.
    pub fn forward(&self, lat: f64, lon: f64) -> (f64, f64) {
        let s = series();
        let e = (F * (2. - F)).sqrt();
        let phi = lat.to_radians();
        let lam = (lon - self.central_meridian()).to_radians();
        let tau = phi.tan();
        let sigma = (e * (e * tau / (1. + tau * tau).sqrt()).atanh()).sinh();
        let tau_p = tau * (1. + sigma * sigma).sqrt() - sigma * (1. + tau * tau).sqrt();
        let xi_p = tau_p.atan2(lam.cos());
        let eta_p = (lam.sin() / (tau_p * tau_p + lam.cos().powi(2)).sqrt()).asinh();
        let (mut xi, mut eta) = (xi_p, eta_p);
        for (j, a) in s.alpha.iter().enumerate() {
            let k = 2. * (j + 1) as f64;
            xi += a * (k * xi_p).sin() * (k * eta_p).cosh();
            eta += a * (k * xi_p).cos() * (k * eta_p).sinh();
        }
        let northing = K0 * s.a * xi;
        (
            FALSE_EASTING + K0 * s.a * eta,
            if self.north {
                northing
            } else {
                northing + FALSE_NORTHING_SOUTH
            },
        )
    }

    /// (easting, northing) in meters to latitude/longitude in degrees.
    pub fn inverse(&self, easting: f64, northing: f64) -> (f64, f64) {
        let s = series();
        let e2 = F * (2. - F);
        let e = e2.sqrt();
        let y = if self.north {
            northing
        } else {
            northing - FALSE_NORTHING_SOUTH
        };
        let xi = y / (K0 * s.a);
        let eta = (easting - FALSE_EASTING) / (K0 * s.a);
        let (mut xi_p, mut eta_p) = (xi, eta);
        for (j, b) in s.beta.iter().enumerate() {
            let k = 2. * (j + 1) as f64;
            xi_p -= b * (k * xi).sin() * (k * eta).cosh();
            eta_p -= b * (k * xi).cos() * (k * eta).sinh();
        }
        let tau_p = xi_p.sin() / (eta_p.sinh().powi(2) + xi_p.cos().powi(2)).sqrt();
        // Newton iteration for tau = tan(latitude).
        let mut tau = tau_p;
        for _ in 0..8 {
            let sigma = (e * (e * tau / (1. + tau * tau).sqrt()).atanh()).sinh();
            let tau_i = tau * (1. + sigma * sigma).sqrt() - sigma * (1. + tau * tau).sqrt();
            let step = (tau_p - tau_i) / (1. + tau_i * tau_i).sqrt() * (1. + (1. - e2) * tau * tau)
                / ((1. - e2) * (1. + tau * tau).sqrt());
            tau += step;
            if step.abs() < 1e-14 {
                break;
            }
        }
        (
            tau.atan().to_degrees(),
            self.central_meridian() + eta_p.sinh().atan2(xi_p.cos()).to_degrees(),
        )
    }
}

struct Series {
    a: f64,
    alpha: [f64; 6],
    beta: [f64; 6],
}

fn series() -> Series {
    let n = F / (2. - F);
    let (n2, n3, n4, n5, n6) = (n * n, n.powi(3), n.powi(4), n.powi(5), n.powi(6));
    Series {
        a: A / (1. + n) * (1. + n2 / 4. + n4 / 64. + n6 / 256.),
        alpha: [
            n / 2. - 2. * n2 / 3. + 5. * n3 / 16. + 41. * n4 / 180. - 127. * n5 / 288.
                + 7891. * n6 / 37800.,
            13. * n2 / 48. - 3. * n3 / 5. + 557. * n4 / 1440. + 281. * n5 / 630.
                - 1983433. * n6 / 1935360.,
            61. * n3 / 240. - 103. * n4 / 140. + 15061. * n5 / 26880. + 167603. * n6 / 181440.,
            49561. * n4 / 161280. - 179. * n5 / 168. + 6601661. * n6 / 7257600.,
            34729. * n5 / 80640. - 3418889. * n6 / 1995840.,
            212378941. * n6 / 319334400.,
        ],
        beta: [
            n / 2. - 2. * n2 / 3. + 37. * n3 / 96. - n4 / 360. - 81. * n5 / 512.
                + 96199. * n6 / 604800.,
            n2 / 48. + n3 / 15. - 437. * n4 / 1440. + 46. * n5 / 105. - 1118711. * n6 / 3870720.,
            17. * n3 / 480. - 37. * n4 / 840. - 209. * n5 / 4480. + 5569. * n6 / 90720.,
            4397. * n4 / 161280. - 11. * n5 / 504. - 830251. * n6 / 7257600.,
            4583. * n5 / 161280. - 108847. * n6 / 3991680.,
            20648693. * n6 / 638668800.,
        ],
    }
}

/// Resample a geographic (latitude/longitude) DEM onto a north-up UTM grid in the zone
/// containing its center, using bilinear interpolation. Square output pixels match the
/// finer source spacing. In UTM the source footprint is rotated by the grid convergence
/// (about Δlongitude × sin(latitude) from the zone's central meridian) and slightly curved,
/// so the output is the largest north-up rectangle inside it: edges are trimmed by about
/// the footprint size × tan(convergence) rather than filled with invented data.
pub fn reproject_to_utm(dem: &Dem) -> Result<Dem, String> {
    let g = dem
        .georef
        .filter(|g| g.units == CrsUnits::Degrees)
        .ok_or("UTM reprojection needs a latitude/longitude georeferenced DEM")?;
    let (w, h) = (dem.width, dem.height);
    // Sample centers, which is where the elevations are defined.
    let lon0 = g.origin_x + g.pixel_width * 0.5;
    let lat0 = g.origin_y - g.pixel_height * 0.5;
    let lon1 = lon0 + (w - 1) as f64 * g.pixel_width;
    let lat1 = lat0 - (h - 1) as f64 * g.pixel_height;
    let zone = UtmZone::containing((lat0 + lat1) * 0.5, (lon0 + lon1) * 0.5);
    let edge = |from: (f64, f64), to: (f64, f64)| -> Vec<(f64, f64)> {
        (0..=64)
            .map(|i| {
                let t = i as f64 / 64.;
                zone.forward(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
            })
            .collect()
    };
    let west = edge((lat0, lon0), (lat1, lon0));
    let east = edge((lat0, lon1), (lat1, lon1));
    let north = edge((lat0, lon0), (lat0, lon1));
    let south = edge((lat1, lon0), (lat1, lon1));
    let fold = |pts: &[(f64, f64)], pick: fn(&(f64, f64)) -> f64, max: bool| {
        pts.iter().map(pick).fold(
            if max {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            },
            if max { f64::max } else { f64::min },
        )
    };
    let x_min = fold(&west, |p| p.0, true);
    let x_max = fold(&east, |p| p.0, false);
    let y_max = fold(&north, |p| p.1, false);
    let y_min = fold(&south, |p| p.1, true);
    let (m_lon, m_lat) = crate::georef::meters_per_degree((lat0 + lat1) * 0.5);
    let step = (g.pixel_width * m_lon).min(g.pixel_height * m_lat);
    if !(x_max > x_min && y_max > y_min && step > 0.) {
        return Err("DEM footprint is too small to reproject".into());
    }
    let out_w = ((x_max - x_min) / step).floor() as usize + 1;
    let out_h = ((y_max - y_min) / step).floor() as usize + 1;
    if out_w < 2 || out_h < 2 || out_w as u64 * out_h as u64 > 50_000_000 {
        return Err(format!("Unsupported UTM grid size {out_w} x {out_h}"));
    }
    let mut heights = Vec::with_capacity(out_w * out_h);
    for j in 0..out_h {
        for i in 0..out_w {
            let (lat, lon) = zone.inverse(x_min + i as f64 * step, y_max - j as f64 * step);
            let fx = ((lon - lon0) / g.pixel_width).clamp(0., (w - 1) as f64);
            let fy = ((lat0 - lat) / g.pixel_height).clamp(0., (h - 1) as f64);
            let (x, y) = ((fx as usize).min(w - 2), (fy as usize).min(h - 2));
            let (tx, ty) = (fx - x as f64, fy - y as f64);
            let top = dem.at(x, y) * (1. - tx) + dem.at(x + 1, y) * tx;
            let bottom = dem.at(x, y + 1) * (1. - tx) + dem.at(x + 1, y + 1) * tx;
            heights.push(top * (1. - ty) + bottom * ty);
        }
    }
    let mut out = Dem::new(out_w, out_h, heights)?;
    out.georef = Some(GeoReference {
        epsg: Some(zone.epsg()),
        units: CrsUnits::Meters,
        origin_x: x_min - step * 0.5,
        origin_y: y_max + step * 0.5,
        pixel_width: step,
        pixel_height: step,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// (lat, lon, EPSG, easting, northing) from pyproj / PROJ 9.
    const REFERENCE: [(f64, f64, u16, f64, f64); 7] = [
        (46.8523, -121.7603, 32610, 594507.8527, 5189496.9692),
        (37.8199, -122.4783, 32610, 545915.8162, 4185961.0391),
        (0.0, 3.0, 32631, 500000.0000, 0.0000),
        (-33.8568, 151.2153, 32756, 334900.5697, 6252288.7529),
        (64.1, -21.9, 32627, 456137.5541, 7108467.4170),
        (45.0, -120.0, 32611, 263553.9739, 4987329.5047),
        (45.0, -114.0001, 32611, 736438.1446, 4987329.2126),
    ];
    #[test]
    fn matches_proj_and_round_trips() {
        for (lat, lon, epsg, e, n) in REFERENCE {
            let z = UtmZone::containing(lat, lon);
            assert_eq!(z.epsg(), epsg);
            let (x, y) = z.forward(lat, lon);
            assert!(
                (x - e).abs() < 1e-3 && (y - n).abs() < 1e-3,
                "{lat},{lon}: {x},{y}"
            );
            let (la, lo) = z.inverse(x, y);
            assert!((la - lat).abs() < 1e-9 && (lo - lon).abs() < 1e-9);
        }
    }
    #[test]
    fn reprojects_plane_without_distorting_it() {
        // A tilted plane in latitude stays a plane in northing after reprojection.
        let (w, h) = (60, 40);
        let heights = (0..w * h).map(|i| (i / w) as f64).collect();
        let mut d = Dem::new(w, h, heights).unwrap();
        d.georef = Some(GeoReference {
            epsg: Some(4326),
            units: CrsUnits::Degrees,
            origin_x: -121.9,
            origin_y: 46.9,
            pixel_width: 0.15 / w as f64,
            pixel_height: 0.1 / h as f64,
        });
        let u = reproject_to_utm(&d).unwrap();
        let g = u.georef.unwrap();
        assert_eq!((g.epsg, g.units), (Some(32610), CrsUnits::Meters));
        assert_eq!(g.pixel_width, g.pixel_height);
        // Source samples span ~11.25 x 10.84 km. About 1.2 degrees from the central
        // meridian, grid convergence (~0.85 degrees) rotates that rectangle in UTM, and
        // the upright inner rectangle loses roughly 170 m at each edge.
        let (sx, sy) = g.sample_span_m(u.width, u.height);
        assert!(
            sx > 10_900. && sx < 11_250. && sy > 10_350. && sy < 10_650.,
            "{sx} {sy}"
        );
        // Row index of the source plane is recovered from each output sample's latitude.
        let z = UtmZone {
            zone: 10,
            north: true,
        };
        for (i, j) in [
            (0, 0),
            (u.width / 2, u.height / 3),
            (u.width - 1, u.height - 1),
        ] {
            let (lat, _) = z.inverse(
                g.origin_x + (i as f64 + 0.5) * g.pixel_width,
                g.origin_y - (j as f64 + 0.5) * g.pixel_height,
            );
            let expected = (46.9 - 0.05 / h as f64 - lat) * h as f64 / 0.1;
            assert!((u.at(i, j) - expected).abs() < 1e-6, "{i},{j}");
        }
        assert!(reproject_to_utm(&Dem::new(2, 2, vec![0.; 4]).unwrap()).is_err());
    }
}
