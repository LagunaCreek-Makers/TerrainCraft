//! Vector cartography: orthographic contours and DEM-based first-person panoramas.
//! Coordinates for viewpoints are fractional raster coordinates; a georeferenced UI
//! should transform WGS84 positions to these coordinates before calling render.
use crate::{contours, Dem};
use std::fmt::Write;

#[derive(Clone, Copy, Debug)]
pub struct Viewpoint {
    pub x: f64,
    pub y: f64,
    pub bearing_deg: f64,
    pub pitch_deg: f64,
    pub fov_deg: f64,
    pub eye_height_m: f64,
    pub max_distance_m: f64,
    pub pixel_size_x_m: f64,
    pub pixel_size_y_m: f64,
}
impl Viewpoint {
    pub fn validate(&self, dem: &Dem) -> Result<(), String> {
        let vals = [
            self.x,
            self.y,
            self.bearing_deg,
            self.pitch_deg,
            self.fov_deg,
            self.eye_height_m,
            self.max_distance_m,
            self.pixel_size_x_m,
            self.pixel_size_y_m,
        ];
        if vals.iter().any(|x| !x.is_finite())
            || self.x < 0.
            || self.y < 0.
            || self.x > (dem.width - 1) as f64
            || self.y > (dem.height - 1) as f64
            || !(1. ..=180.).contains(&self.fov_deg)
            || self.pitch_deg.abs() > 89.
            || self.eye_height_m < 0.
            || self.max_distance_m <= 0.
            || self.pixel_size_x_m <= 0.
            || self.pixel_size_y_m <= 0.
        {
            return Err("Invalid viewpoint, field of view, or geographic pixel scale".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngravingStyle {
    Ridgeline,
    Contour,
    Illustrated,
}
#[derive(Clone, Debug)]
pub struct EngravingOptions {
    pub width_mm: f64,
    pub height_mm: f64,
    pub contour_interval_m: f64,
    pub style: EngravingStyle,
    pub title: String,
    pub subtitle: String,
    pub font_family: String,
    pub show_coordinates: bool,
}
impl Default for EngravingOptions {
    fn default() -> Self {
        Self {
            width_mm: 400.,
            height_mm: 250.,
            contour_interval_m: 20.,
            style: EngravingStyle::Ridgeline,
            title: "Mountain panorama".into(),
            subtitle: String::new(),
            font_family: "serif".into(),
            show_coordinates: false,
        }
    }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn sample(d: &Dem, x: f64, y: f64) -> f64 {
    let x = x.clamp(0., (d.width - 1) as f64);
    let y = y.clamp(0., (d.height - 1) as f64);
    let ix = x.floor() as usize;
    let iy = y.floor() as usize;
    let jx = (ix + 1).min(d.width - 1);
    let jy = (iy + 1).min(d.height - 1);
    let u = x - ix as f64;
    let v = y - iy as f64;
    d.at(ix, iy) * (1. - u) * (1. - v)
        + d.at(jx, iy) * u * (1. - v)
        + d.at(ix, jy) * (1. - u) * v
        + d.at(jx, jy) * u * v
}
/// For each horizontal camera column, ray-march the DEM and return the
/// highest visible elevation angle. A zero-angle horizon is y=0.5.
/// Bearing is clockwise from raster north (negative row direction).
/// This is a terrain horizon, not a full photorealistic renderer.
pub fn horizon(d: &Dem, v: Viewpoint, columns: usize) -> Result<Vec<f64>, String> {
    v.validate(d)?;
    if !(2..=8192).contains(&columns) {
        return Err("Horizon columns must be 2..8192".into());
    }
    let eye = sample(d, v.x, v.y) + v.eye_height_m;
    let step = (v.pixel_size_x_m.min(v.pixel_size_y_m) * 0.65).max(0.25);
    let mut result = Vec::with_capacity(columns);
    for col in 0..columns {
        let bearing =
            (v.bearing_deg + (col as f64 / (columns - 1) as f64 - 0.5) * v.fov_deg).to_radians();
        let dx = bearing.sin() / v.pixel_size_x_m;
        let dy = -bearing.cos() / v.pixel_size_y_m;
        let mut distance = step;
        let mut max_angle = -std::f64::consts::FRAC_PI_2;
        while distance <= v.max_distance_m {
            let x = v.x + dx * distance;
            let y = v.y + dy * distance;
            if x < 0. || y < 0. || x > (d.width - 1) as f64 || y > (d.height - 1) as f64 {
                break;
            }
            let terrain = sample(d, x, y);
            // Standard curvature approximation, without atmospheric refraction.
            let curvature = distance * distance / (2. * 6_371_000.);
            let angle = ((terrain - eye - curvature) / distance).atan();
            max_angle = max_angle.max(angle);
            distance += step.max(distance * 0.003);
        }
        result.push(max_angle);
    }
    Ok(result)
}
/// Sample visible terrain horizons in near-to-far distance bands. The final
/// band is the full horizon; nearer bands reveal intermediate ridges.
pub fn depth_horizons(
    d: &Dem,
    v: Viewpoint,
    columns: usize,
    bands: usize,
) -> Result<Vec<Vec<f64>>, String> {
    if !(2..=12).contains(&bands) {
        return Err("Depth bands must be 2..12".into());
    }
    let mut out = Vec::with_capacity(bands);
    for b in 1..=bands {
        let mut limited = v;
        limited.max_distance_m = v.max_distance_m * b as f64 / bands as f64;
        out.push(horizon(d, limited, columns)?);
    }
    Ok(out)
}
/// Multi-distance horizon engraving with individually editable SVG groups.
/// Occluded segments are not drawn if a nearer band has a higher horizon.
pub fn panorama_depth_svg(
    d: &Dem,
    v: Viewpoint,
    o: &EngravingOptions,
    bands: usize,
) -> Result<String, String> {
    if !o.width_mm.is_finite() || !o.height_mm.is_finite() || o.width_mm <= 0. || o.height_mm <= 50.
    {
        return Err("Invalid engraving dimensions".into());
    }
    let columns = 600;
    let curves = depth_horizons(d, v, columns, bands)?;
    let top = 25.;
    let bottom = o.height_mm - 25.;
    let center = (top + bottom) * 0.5;
    let vertical_fov = v.fov_deg.to_radians() * (o.height_mm / o.width_mm);
    let scale = (bottom - top) / vertical_fov.max(0.05);
    let mut svg = svg_header(o.width_mm, o.height_mm);
    for (b, curve) in curves.iter().enumerate().rev() {
        let _=write!(svg,"<g id=\"ridge-band-{}\" fill=\"none\" stroke=\"#000\" stroke-width=\"{:.2}\"><path d=\"",b+1,0.12+0.03*(bands-b) as f64);
        let mut drawing = false;
        for (i, a) in curve.iter().enumerate() {
            let obscured = b > 0 && curves[b - 1][i] >= *a - 1e-8;
            if !a.is_finite() || *a <= -std::f64::consts::FRAC_PI_2 + 1e-5 || obscured {
                drawing = false;
                continue;
            }
            let x = i as f64 / (columns - 1) as f64 * o.width_mm;
            let y = (center - (*a - v.pitch_deg.to_radians()) * scale).clamp(top, bottom);
            let _ = write!(
                svg,
                "{} {:.3} {:.3} ",
                if drawing { "L" } else { "M" },
                x,
                y
            );
            drawing = true;
        }
        svg.push_str("\"/></g>\n");
    }
    let _=writeln!(svg,"<g id=\"annotations\" font-family=\"{}\" text-anchor=\"middle\"><text x=\"{:.2}\" y=\"12\" font-size=\"7\">{}</text></g>",xml(&o.font_family),o.width_mm/2.,xml(&o.title));
    svg.push_str("</svg>\n");
    Ok(svg)
}
fn svg_header(w: f64, h: f64) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n")
}
/// Perspective ridgeline SVG with labels supplied by the user. Labels are
/// editable SVG text; downstream laser CAM may require converting to paths.
pub fn panorama_svg(d: &Dem, v: Viewpoint, o: &EngravingOptions) -> Result<String, String> {
    if !o.width_mm.is_finite() || !o.height_mm.is_finite() || o.width_mm <= 0. || o.height_mm <= 0.
    {
        return Err("Invalid engraving size".into());
    }
    let cols = 600;
    let angles = horizon(d, v, cols)?;
    let mut s = svg_header(o.width_mm, o.height_mm);
    let top = 25.;
    let bottom = o.height_mm - 25.;
    let center = (top + bottom) * 0.5;
    let vertical_fov = v.fov_deg.to_radians() * (o.height_mm / o.width_mm);
    let scale = (bottom - top) / vertical_fov.max(0.05);
    s.push_str("<g id=\"terrain-ridgeline\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.18\"><path d=\"");
    for (i, a) in angles.iter().enumerate() {
        let x = i as f64 / (cols - 1) as f64 * o.width_mm;
        let y = (center - (*a - v.pitch_deg.to_radians()) * scale).clamp(top, bottom);
        let _ = write!(s, "{} {:.3} {:.3} ", if i == 0 { "M" } else { "L" }, x, y);
    }
    s.push_str("\"/></g>\n");
    if o.style == EngravingStyle::Illustrated {
        s.push_str("<g id=\"decorative-frame\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.25\"><rect x=\"2\" y=\"2\" width=\"");
        let _ = writeln!(
            s,
            "{}\" height=\"{}\"/></g>",
            o.width_mm - 4.,
            o.height_mm - 4.
        );
    }
    let font = xml(&o.font_family);
    let _ = write!(
        s,
        "<g id=\"annotations\" fill=\"#000\" font-family=\"{}\" text-anchor=\"middle\">",
        font
    );
    let _ = write!(
        s,
        "<text x=\"{}\" y=\"12\" font-size=\"7\">{}</text>",
        o.width_mm / 2.,
        xml(&o.title)
    );
    let sub = if o.subtitle.is_empty() {
        format!("View bearing {:.1}° · FOV {:.1}°", v.bearing_deg, v.fov_deg)
    } else {
        o.subtitle.clone()
    };
    let _ = write!(
        s,
        "<text x=\"{}\" y=\"{}\" font-size=\"4\">{}</text></g>",
        o.width_mm / 2.,
        o.height_mm - 7.,
        xml(&sub)
    );
    s.push_str("</svg>\n");
    Ok(s)
}
/// Add automatically detected, *unnamed* visible summit candidates to a panorama.
/// A separate geographic gazetteer is needed before assigning real peak names.
pub fn panorama_with_peaks_svg(
    d: &Dem,
    v: Viewpoint,
    o: &EngravingOptions,
    min_prominence_m: f64,
    max_labels: usize,
) -> Result<String, String> {
    let mut svg = panorama_svg(d, v, o)?;
    let peaks = crate::peaks::visible_peaks(d, v, min_prominence_m, max_labels)?;
    let top = 25.;
    let bottom = o.height_mm - 25.;
    let center = (top + bottom) * 0.5;
    let vertical_fov = v.fov_deg.to_radians() * (o.height_mm / o.width_mm);
    let scale = (bottom - top) / vertical_fov.max(0.05);
    let mut labels=String::from("<g id=\"candidate-summits\" fill=\"#000\" stroke=\"none\" font-family=\"sans-serif\" font-size=\"3.2\">\n");
    let mut occupied: Vec<f64> = Vec::new();
    for p in peaks {
        let x = p.horizontal_fraction * o.width_mm;
        if occupied.iter().any(|old| (x - old).abs() < 20.) {
            continue;
        }
        let y = (center - (p.elevation_angle_deg - v.pitch_deg).to_radians() * scale)
            .clamp(top, bottom);
        let label_y = (y - 6.).max(18.);
        let _=writeln!(labels,"<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"0.9\"/><path d=\"M {x:.2} {y:.2} L {x:.2} {label_y:.2}\" stroke=\"#000\" stroke-width=\"0.12\"/><text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">Candidate peak · {:.0} m</text>",label_y-1.,p.elevation_m);
        occupied.push(x);
    }
    labels.push_str("</g>\n");
    if let Some(i) = svg.rfind("</svg>") {
        svg.insert_str(i, &labels);
    }
    Ok(svg)
}
/// Contour line engraving on a single sheet. The source DEM is in meters.
pub fn topdown_svg(d: &Dem, o: &EngravingOptions) -> Result<String, String> {
    if !o.contour_interval_m.is_finite()
        || o.contour_interval_m <= 0.
        || o.width_mm <= 0.
        || o.height_mm <= 0.
    {
        return Err("Invalid engraving settings".into());
    }
    let (lo, hi) = d.min_max();
    let first = (lo / o.contour_interval_m).ceil() as i64;
    let last = (hi / o.contour_interval_m).floor() as i64;
    if last - first > 1000 {
        return Err("Too many contour levels; increase interval".into());
    }
    let mut s = svg_header(o.width_mm, o.height_mm);
    s.push_str(
        "<g id=\"contour-engraving\" stroke=\"#000\" fill=\"none\" stroke-width=\"0.15\">\n",
    );
    for i in first..=last {
        let level = i as f64 * o.contour_interval_m;
        for path in contours::paths(d, o.width_mm, o.height_mm, level).map_err(|e| e.to_string())? {
            if path.len() < 2 {
                continue;
            }
            s.push_str("<path d=\"");
            for (j, (x, y)) in path.iter().enumerate() {
                let _ = write!(s, "{} {:.3} {:.3} ", if j == 0 { "M" } else { "L" }, x, y);
            }
            if path.first() == path.last() {
                s.push('Z');
            }
            s.push_str("\"/>\n");
        }
    }
    s.push_str("</g>\n");
    let _=write!(s,"<g id=\"annotations\" fill=\"#000\" font-family=\"{}\" text-anchor=\"middle\"><text x=\"{}\" y=\"9\" font-size=\"6\">{}</text></g></svg>",xml(&o.font_family),o.width_mm/2.,xml(&o.title));
    Ok(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn dem() -> Dem {
        Dem::new(5, 5, (0..25).map(|i| i as f64).collect()).unwrap()
    }
    #[test]
    fn panorama_has_vector_path() {
        let v = Viewpoint {
            x: 2.,
            y: 2.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 1.7,
            max_distance_m: 100.,
            pixel_size_x_m: 10.,
            pixel_size_y_m: 10.,
        };
        let svg = panorama_svg(&dem(), v, &EngravingOptions::default()).unwrap();
        assert!(svg.contains("terrain-ridgeline"));
        assert!(svg.contains("<path"));
    }
    #[test]
    fn rejects_bad_viewpoint() {
        let mut v = Viewpoint {
            x: 2.,
            y: 2.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 1.7,
            max_distance_m: 100.,
            pixel_size_x_m: 10.,
            pixel_size_y_m: 10.,
        };
        v.fov_deg = 0.;
        assert!(horizon(&dem(), v, 50).is_err());
    }
    #[test]
    fn escapes_labels() {
        let o = EngravingOptions {
            title: "A&B <C>".into(),
            ..EngravingOptions::default()
        };
        let svg = topdown_svg(&dem(), &o).unwrap();
        assert!(svg.contains("A&amp;B &lt;C&gt;"));
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;
    #[test]
    fn depth_groups_export() {
        let d = Dem::new(12, 12, (0..144).map(|i| ((i * 17) % 31) as f64).collect()).unwrap();
        let v = Viewpoint {
            x: 5.,
            y: 10.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 60.,
            eye_height_m: 2.,
            max_distance_m: 90.,
            pixel_size_x_m: 10.,
            pixel_size_y_m: 10.,
        };
        let svg = panorama_depth_svg(&d, v, &EngravingOptions::default(), 3).unwrap();
        assert!(svg.contains("ridge-band-1") && svg.contains("ridge-band-3"));
    }
}
