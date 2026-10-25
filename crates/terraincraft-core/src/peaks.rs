//! DEM-local summit candidates and line-of-sight checks for perspective engraving.
//! These are *unnamed candidates*, not verified GNIS summits.
use crate::{cartography::Viewpoint, Dem};
#[derive(Clone, Debug)]
pub struct VisiblePeak {
    pub x: usize,
    pub y: usize,
    pub elevation_m: f64,
    pub distance_m: f64,
    pub bearing_deg: f64,
    pub elevation_angle_deg: f64,
    pub horizontal_fraction: f64,
}
fn bilinear(d: &Dem, x: f64, y: f64) -> f64 {
    let x = x.clamp(0., (d.width - 1) as f64);
    let y = y.clamp(0., (d.height - 1) as f64);
    let i = x.floor() as usize;
    let j = y.floor() as usize;
    let a = (i + 1).min(d.width - 1);
    let b = (j + 1).min(d.height - 1);
    let u = x - i as f64;
    let v = y - j as f64;
    d.at(i, j) * (1. - u) * (1. - v)
        + d.at(a, j) * u * (1. - v)
        + d.at(i, b) * (1. - u) * v
        + d.at(a, b) * u * v
}
fn wrap(a: f64) -> f64 {
    (a + 180.).rem_euclid(360.) - 180.
}
/// Find local maxima in an 8-neighbor window and retain those with unobstructed
/// DEM line of sight. Bearings are clockwise from raster north, not true north
/// unless the source raster has been north-up georeferenced.
pub fn visible_peaks(
    d: &Dem,
    v: Viewpoint,
    min_prominence_m: f64,
    max_results: usize,
) -> Result<Vec<VisiblePeak>, String> {
    v.validate(d)?;
    if !min_prominence_m.is_finite() || min_prominence_m < 0. || max_results > 1000 {
        return Err("Invalid summit filter".into());
    }
    let eye = bilinear(d, v.x, v.y) + v.eye_height_m;
    let mut peaks = Vec::new();
    for y in 1..d.height - 1 {
        for x in 1..d.width - 1 {
            let z = d.at(x, y);
            let mut highest_neighbor = f64::NEG_INFINITY;
            let mut strict = true;
            for yy in y - 1..=y + 1 {
                for xx in x - 1..=x + 1 {
                    if xx != x || yy != y {
                        let n = d.at(xx, yy);
                        highest_neighbor = highest_neighbor.max(n);
                        if n >= z {
                            strict = false;
                        }
                    }
                }
            }
            if !strict || z - highest_neighbor < min_prominence_m {
                continue;
            }
            let dx = (x as f64 - v.x) * v.pixel_size_x_m;
            let dy = (v.y - y as f64) * v.pixel_size_y_m;
            let dist = dx.hypot(dy);
            if dist < 1. || dist > v.max_distance_m {
                continue;
            }
            let bearing = dx.atan2(dy).to_degrees().rem_euclid(360.);
            let delta = wrap(bearing - v.bearing_deg);
            if delta.abs() > v.fov_deg * 0.5 {
                continue;
            }
            let angle = ((z - eye - dist * dist / (2. * 6_371_000.)) / dist).atan();
            let step = v.pixel_size_x_m.min(v.pixel_size_y_m) * 0.5;
            let n = ((dist / step).ceil() as usize).clamp(2, 20000);
            let mut visible = true;
            for i in 1..n {
                let t = i as f64 / n as f64;
                let distance = dist * t;
                let px = v.x + (x as f64 - v.x) * t;
                let py = v.y + (y as f64 - v.y) * t;
                let z0 = bilinear(d, px, py);
                let a = ((z0 - eye - distance * distance / (2. * 6_371_000.)) / distance).atan();
                if a >= angle - 1e-7 {
                    visible = false;
                    break;
                }
            }
            if visible {
                peaks.push(VisiblePeak {
                    x,
                    y,
                    elevation_m: z,
                    distance_m: dist,
                    bearing_deg: bearing,
                    elevation_angle_deg: angle.to_degrees(),
                    horizontal_fraction: 0.5 + delta / v.fov_deg,
                });
            }
        }
    }
    peaks.sort_by(|a, b| b.elevation_m.total_cmp(&a.elevation_m));
    peaks.truncate(max_results);
    Ok(peaks)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_visible_summit() {
        let mut h = vec![0.; 81];
        h[2 * 9 + 4] = 100.;
        let d = Dem::new(9, 9, h).unwrap();
        let v = Viewpoint {
            x: 4.,
            y: 7.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 1.7,
            max_distance_m: 200.,
            pixel_size_x_m: 10.,
            pixel_size_y_m: 10.,
        };
        let p = visible_peaks(&d, v, 1., 10).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].x, p[0].y), (4, 2));
    }
    #[test]
    fn rejects_bad_filter() {
        let d = Dem::new(3, 3, vec![0.; 9]).unwrap();
        let v = Viewpoint {
            x: 1.,
            y: 1.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 1.7,
            max_distance_m: 100.,
            pixel_size_x_m: 10.,
            pixel_size_y_m: 10.,
        };
        assert!(visible_peaks(&d, v, -1., 5).is_err());
    }
}
