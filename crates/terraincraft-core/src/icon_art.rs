//! Landmark icons drawn from real terrain: a mountain's skyline and its larger inner
//! ridges as seen from a chosen viewpoint, in the 24 x 24 line style of the icon library.
use crate::engrave::{PanoramaView, Sight};
use crate::Dem;

type Line = Vec<(f64, f64)>;

/// Icon artwork, in 24 x 24 units like the bundled line icons.
const SIZE: f64 = 24.;
const LEFT: f64 = 2.;
const RIGHT: f64 = 22.;
const TOP: f64 = 4.;
const BASE: f64 = 20.;

/// A `size`-unit square SVG of open polylines, stroked like Tabler icons.
pub fn line_icon_svg(lines: &[Line]) -> String {
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{SIZE}\" height=\"{SIZE}\" \
         viewBox=\"0 0 {SIZE} {SIZE}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\">\n"
    );
    for l in lines.iter().filter(|l| l.len() >= 2) {
        let d: Vec<String> = l
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}{:.2} {:.2}", if i == 0 { 'M' } else { 'L' }, p.0, p.1))
            .collect();
        s.push_str(&format!("  <path d=\"{}\" />\n", d.join(" ")));
    }
    s.push_str("</svg>\n");
    s
}

/// Douglas-Peucker simplification to within `tol`.
fn simplify(line: &[(f64, f64)], tol: f64) -> Line {
    if line.len() < 3 {
        return line.to_vec();
    }
    let (a, b) = (line[0], line[line.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1e-12);
    let (i, far) = line[1..line.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, p)| (i + 1, ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len))
        .fold((0, 0.), |m, v| if v.1 > m.1 { v } else { m });
    if far <= tol {
        return vec![a, b];
    }
    let mut left = simplify(&line[..=i], tol);
    left.pop();
    left.extend(simplify(&line[i..], tol));
    left
}

/// Which terrain a ridgeline icon shows, by distance from the viewpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RidgelineRange {
    /// Terrain nearer than this is ignored, so canyon walls and foreground hills around
    /// the viewpoint don't hide the subject.
    pub near_km: f64,
    /// When set, a second line traces the nearer ridges up to this distance, in front of
    /// the skyline.
    pub front_km: Option<f64>,
}

impl Default for RidgelineRange {
    fn default() -> Self {
        Self {
            near_km: 0.5,
            front_km: None,
        }
    }
}

/// Moving average over `2 * half + 1` values, shrinking at the ends.
fn smooth(v: &[f64], half: usize) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            let w = &v[i.saturating_sub(half)..(i + half + 1).min(v.len())];
            w.iter().sum::<f64>() / w.len() as f64
        })
        .collect()
}

/// The skyline seen from `view` (and optionally a nearer front ridge) as a line icon.
/// Heights are exaggerated up to 5x so the subject fills the icon; choose a field of
/// view that frames it closely.
pub fn ridgeline_icon(
    dem: &Dem,
    view: &PanoramaView,
    range: RidgelineRange,
) -> Result<String, String> {
    let sight = Sight::new(dem, view)?;
    let (near, far) = (range.near_km * 1000., view.max_distance_km * 1000.);
    let front = range.front_km.map(|f| f * 1000.);
    if !(near >= 0. && near < far) || front.is_some_and(|f| f <= near) {
        return Err("Invalid ridgeline distances".into());
    }
    const COLUMNS: usize = 400;
    let fov = view.fov_deg.to_radians();
    let mut sky = Vec::with_capacity(COLUMNS);
    let mut fore = Vec::with_capacity(COLUMNS);
    for i in 0..COLUMNS {
        let b = view.bearing_deg.to_radians() + (i as f64 / (COLUMNS - 1) as f64 - 0.5) * fov;
        let (mut top, mut front_top) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut d = near.max(sight.step(0.));
        while d <= far {
            let Some(a) = sight.angle_at(b, d) else {
                break;
            };
            top = top.max(a);
            if front.is_some_and(|f| d <= f) {
                front_top = front_top.max(a);
            }
            d += sight.step(d);
        }
        if !top.is_finite() {
            return Err("The view leaves the elevation data; download a larger area".into());
        }
        sky.push(top);
        fore.push(front_top);
    }
    let half = COLUMNS / 80;
    let sky = smooth(&sky, half);
    let fore = smooth(&fore, half);
    let (hi, lo) = sky
        .iter()
        .fold((f64::NEG_INFINITY, f64::INFINITY), |(h, l), &a| {
            (h.max(a), l.min(a))
        });
    // One icon unit per `per_rad` radians at true proportions, then exaggerated.
    let per_rad = (RIGHT - LEFT) / fov;
    let room = BASE - 1.5 - TOP;
    let k = (room / ((hi - lo) * per_rad).max(1e-9)).clamp(1., 5.);
    let height = (hi - lo) * per_rad * k;
    let top = TOP + (room - height) * 0.5;
    let base = top + height + 1.5;
    let x_of = |i: usize| LEFT + i as f64 / (COLUMNS - 1) as f64 * (RIGHT - LEFT);
    let y_of = |a: f64| top + (hi - a) * per_rad * k;
    let mut lines = vec![simplify(
        &(0..COLUMNS)
            .map(|i| (x_of(i), y_of(sky[i])))
            .collect::<Line>(),
        0.3,
    )];
    if front.is_some() {
        // The front ridge only where it stands clearly below the skyline.
        let gap = (hi - lo).max(1e-9) * 0.06;
        let mut run: Line = Vec::new();
        for i in 0..COLUMNS {
            let y = y_of(fore[i]);
            if fore[i].is_finite() && sky[i] - fore[i] > gap && y < base - 0.5 {
                run.push((x_of(i), y));
            } else if run.len() > 1 {
                lines.push(simplify(&std::mem::take(&mut run), 0.3));
            } else {
                run.clear();
            }
        }
        if run.len() > 1 {
            lines.push(simplify(&run, 0.3));
        }
        lines.retain(|l| l.len() > 1 && l[l.len() - 1].0 - l[0].0 > 1.5);
    }
    lines.push(vec![(LEFT, base), (RIGHT, base)]);
    Ok(line_icon_svg(&lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::georef::{CrsUnits, GeoReference};

    /// A cone-shaped mountain with a lower shoulder to the east, on a 30 m UTM grid.
    fn mountain() -> Dem {
        let n = 201;
        let v = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64, (i / n) as f64);
                let peak = 3000. - 30. * ((x - 100.).powi(2) + (y - 100.).powi(2)).sqrt();
                let shoulder = 2200. - 30. * ((x - 130.).powi(2) + (y - 105.).powi(2)).sqrt();
                peak.max(shoulder).max(500.)
            })
            .collect();
        let mut d = Dem::new(n, n, v).unwrap();
        d.georef = Some(GeoReference {
            epsg: Some(32610),
            units: CrsUnits::Meters,
            origin_x: 591_000.,
            origin_y: 5_192_500.,
            pixel_width: 30.,
            pixel_height: 30.,
        });
        d
    }

    #[test]
    fn skyline_icon_fits_the_box() {
        let dem = mountain();
        // Stand south of the peak, looking north.
        let (lat, lon) = dem.georef.unwrap().sample_to_latlon(100., 190.).unwrap();
        let view = PanoramaView {
            lat,
            lon,
            bearing_deg: 0.,
            fov_deg: 60.,
            eye_height_m: 1.7,
            max_distance_km: 20.,
        };
        let range = RidgelineRange {
            near_km: 0.3,
            front_km: Some(2.),
        };
        let svg = ridgeline_icon(&dem, &view, range).unwrap();
        let s = crate::svg_lines::parse(&svg).unwrap();
        assert!(s.lines.len() >= 2);
        assert!(ridgeline_icon(
            &dem,
            &view,
            RidgelineRange {
                near_km: 30.,
                front_km: None
            }
        )
        .is_err());
        for p in s.lines.iter().flatten() {
            assert!(
                (0. ..=24.).contains(&p.0) && (0. ..=24.).contains(&p.1),
                "{p:?}"
            );
        }
        // The skyline is centered in the box, with the summit in the middle of the view.
        let sky = &s.lines[0];
        let summit = sky
            .iter()
            .copied()
            .fold((0., f64::INFINITY), |m, p| if p.1 < m.1 { p } else { m });
        assert!((TOP..12.).contains(&summit.1), "{summit:?}");
        assert!((summit.0 - 12.).abs() < 2., "{summit:?}");
    }

    #[test]
    fn simplify_keeps_corners() {
        let l = vec![(0., 0.), (1., 0.01), (2., 0.), (2., 2.)];
        assert_eq!(simplify(&l, 0.1), vec![(0., 0.), (2., 0.), (2., 2.)]);
    }
}
