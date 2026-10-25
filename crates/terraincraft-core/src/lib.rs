//! Rust-native terrain mesh and laser contour geometry. No Python dependency.

pub mod cartography;
pub mod contours;
pub mod engraving_editor;
pub mod engraving_preview;
pub mod geotiff;
pub mod geoview;
pub mod gis;
pub mod icon_variants;
pub mod inspection;
pub mod intersections;
pub mod landmarks;
pub mod laser;
pub mod manufacturing;
pub mod mesh_audit;
pub mod mesh_repair;
pub mod peaks;
pub mod print_export;
pub mod registration;
pub mod seam_fabrication;
pub mod seam_joints;
pub mod seam_preflight;
pub mod seam_socket_mesh;
pub mod socket_mesh;
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Dem {
    pub width: usize,
    pub height: usize,
    pub heights: Vec<f64>,
}
impl Dem {
    pub fn new(width: usize, height: usize, heights: Vec<f64>) -> Result<Self, String> {
        if width < 2
            || height < 2
            || heights.len() != width * height
            || heights.iter().any(|v| !v.is_finite())
        {
            return Err("DEM requires a finite rectangular grid of at least 2 x 2".into());
        }
        Ok(Self {
            width,
            height,
            heights,
        })
    }
    pub fn at(&self, x: usize, y: usize) -> f64 {
        self.heights[y * self.width + x]
    }
    pub fn min_max(&self) -> (f64, f64) {
        self.heights
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(*v), b.max(*v))
            })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct TerrainSettings {
    pub width_mm: f64,
    pub height_mm: f64,
    pub base_mm: f64,
    pub vertical_exaggeration: f64,
    pub relief_mm: f64,
}
impl TerrainSettings {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.width_mm,
            self.height_mm,
            self.base_mm,
            self.vertical_exaggeration,
            self.relief_mm,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
        {
            Err("Dimensions, base, exaggeration and relief must be positive finite values".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub row: usize,
    pub col: usize,
    pub x0: usize,
    pub x1: usize,
    pub y0: usize,
    pub y1: usize,
}
pub fn tiles(
    dem: &Dem,
    settings: TerrainSettings,
    bed_x: f64,
    bed_y: f64,
    margin: f64,
) -> Result<Vec<Tile>, String> {
    settings.validate()?;
    if !bed_x.is_finite()
        || !bed_y.is_finite()
        || !margin.is_finite()
        || margin < 0.
        || bed_x <= 2. * margin
        || bed_y <= 2. * margin
    {
        return Err("Invalid print bed or margin".into());
    }
    let sx = settings.width_mm / (dem.width - 1) as f64;
    let sy = settings.height_mm / (dem.height - 1) as f64;
    let nx = ((bed_x - 2. * margin) / sx).floor() as usize;
    let ny = ((bed_y - 2. * margin) / sy).floor() as usize;
    if nx == 0 || ny == 0 {
        return Err("Bed cannot accommodate a single DEM cell; downsample DEM".into());
    }
    let mut out = vec![];
    let mut y = 0;
    let mut row = 0;
    while y < dem.height - 1 {
        let y1 = (y + ny).min(dem.height - 1);
        let mut x = 0;
        let mut col = 0;
        while x < dem.width - 1 {
            let x1 = (x + nx).min(dem.width - 1);
            out.push(Tile {
                row,
                col,
                x0: x,
                x1,
                y0: y,
                y1,
            });
            x = x1;
            col += 1;
        }
        y = y1;
        row += 1;
    }
    Ok(out)
}
type Vec3 = [f64; 3];
fn tri(buf: &mut Vec<[Vec3; 3]>, a: Vec3, b: Vec3, c: Vec3) {
    buf.push([a, b, c]);
}
/// Generate a closed solid terrain tile, with a planar underside and vertical side walls.
pub fn mesh(dem: &Dem, s: TerrainSettings, t: Tile) -> Result<Vec<[Vec3; 3]>, String> {
    s.validate()?;
    if t.x0 >= t.x1 || t.y0 >= t.y1 || t.x1 >= dem.width || t.y1 >= dem.height {
        return Err("Invalid tile bounds".into());
    }
    let (min, max) = dem.min_max();
    let range = (max - min).max(1e-12);
    let point = |x: usize, y: usize| -> Vec3 {
        [
            (x - t.x0) as f64 * s.width_mm / (dem.width - 1) as f64,
            (y - t.y0) as f64 * s.height_mm / (dem.height - 1) as f64,
            s.base_mm + (dem.at(x, y) - min) / range * s.relief_mm * s.vertical_exaggeration,
        ]
    };
    let ground = |x: usize, y: usize| -> Vec3 {
        let mut p = point(x, y);
        p[2] = 0.;
        p
    };
    let mut f = Vec::new();
    for y in t.y0..t.y1 {
        for x in t.x0..t.x1 {
            let (a, b, c, d) = (
                point(x, y),
                point(x + 1, y),
                point(x + 1, y + 1),
                point(x, y + 1),
            );
            tri(&mut f, a, b, c);
            tri(&mut f, a, c, d);
            let (a, b, c, d) = (
                ground(x, y),
                ground(x + 1, y),
                ground(x + 1, y + 1),
                ground(x, y + 1),
            );
            tri(&mut f, a, c, b);
            tri(&mut f, a, d, c);
        }
    }
    for x in t.x0..t.x1 {
        let (a, b) = (point(x, t.y0), point(x + 1, t.y0));
        let (c, d) = (ground(x, t.y0), ground(x + 1, t.y0));
        tri(&mut f, a, c, b);
        tri(&mut f, b, c, d);
        let (a, b) = (point(x, t.y1), point(x + 1, t.y1));
        let (c, d) = (ground(x, t.y1), ground(x + 1, t.y1));
        tri(&mut f, a, b, c);
        tri(&mut f, b, d, c);
    }
    for y in t.y0..t.y1 {
        let (a, b) = (point(t.x0, y), point(t.x0, y + 1));
        let (c, d) = (ground(t.x0, y), ground(t.x0, y + 1));
        tri(&mut f, a, b, c);
        tri(&mut f, b, d, c);
        let (a, b) = (point(t.x1, y), point(t.x1, y + 1));
        let (c, d) = (ground(t.x1, y), ground(t.x1, y + 1));
        tri(&mut f, a, c, b);
        tri(&mut f, b, c, d);
    }
    Ok(f)
}
fn normal(t: &[Vec3; 3]) -> Vec3 {
    let a = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let b = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l == 0. {
        [0., 0., 0.]
    } else {
        [n[0] / l, n[1] / l, n[2] / l]
    }
}
pub fn write_stl(path: &Path, faces: &[[Vec3; 3]]) -> io::Result<()> {
    let mut f = File::create(path)?;
    writeln!(f, "solid terraincraft")?;
    for face in faces {
        let n = normal(face);
        writeln!(f, " facet normal {} {} {}\n  outer loop", n[0], n[1], n[2])?;
        for v in face {
            writeln!(f, "   vertex {} {} {}", v[0], v[1], v[2])?;
        }
        writeln!(f, "  endloop\n endfacet")?;
    }
    writeln!(f, "endsolid terraincraft")
}
/// Marching-squares contour segments, exported as an engraving-path SVG.
/// Segments are not closed cut polygons: contour-to-polygon conversion is a later phase.
pub fn contour_svg(
    dem: &Dem,
    width_mm: f64,
    height_mm: f64,
    threshold: f64,
) -> Result<String, String> {
    if !width_mm.is_finite()
        || !height_mm.is_finite()
        || width_mm <= 0.
        || height_mm <= 0.
        || !threshold.is_finite()
    {
        return Err("Invalid contour parameters".into());
    }
    let mut lines = String::new();
    for y in 0..dem.height - 1 {
        for x in 0..dem.width - 1 {
            let vals = [
                dem.at(x, y),
                dem.at(x + 1, y),
                dem.at(x + 1, y + 1),
                dem.at(x, y + 1),
            ];
            let corners = [
                (x as f64, y as f64),
                ((x + 1) as f64, y as f64),
                ((x + 1) as f64, (y + 1) as f64),
                (x as f64, (y + 1) as f64),
            ];
            let mut pts = vec![];
            for e in 0..4 {
                let j = (e + 1) % 4;
                if (vals[e] >= threshold) != (vals[j] >= threshold) {
                    let t = (threshold - vals[e]) / (vals[j] - vals[e]);
                    let xx = (corners[e].0 + t * (corners[j].0 - corners[e].0)) * width_mm
                        / (dem.width - 1) as f64;
                    let yy = (corners[e].1 + t * (corners[j].1 - corners[e].1)) * height_mm
                        / (dem.height - 1) as f64;
                    pts.push((xx, yy));
                }
            }
            for pair in pts.chunks_exact(2) {
                lines.push_str(&format!(
                    "<path d=\"M {:.4} {:.4} L {:.4} {:.4}\"/>\n",
                    pair[0].0, pair[0].1, pair[1].0, pair[1].1
                ));
            }
        }
    }
    Ok(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width_mm}mm\" height=\"{height_mm}mm\" viewBox=\"0 0 {width_mm} {height_mm}\"><g fill=\"none\" stroke=\"black\" stroke-width=\"0.1\">{lines}</g></svg>"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Dem {
        Dem::new(3, 3, vec![0., 1., 2., 1., 2., 3., 2., 3., 4.]).unwrap()
    }
    fn settings() -> TerrainSettings {
        TerrainSettings {
            width_mm: 200.,
            height_mm: 200.,
            base_mm: 3.,
            vertical_exaggeration: 2.,
            relief_mm: 20.,
        }
    }
    #[test]
    fn tiles_fit() {
        let t = tiles(&sample(), settings(), 110., 110., 5.).unwrap();
        assert_eq!(t.len(), 4);
    }
    #[test]
    fn closed_mesh_count() {
        let d = sample();
        let t = tiles(&d, settings(), 220., 220., 5.).unwrap();
        assert_eq!(mesh(&d, settings(), t[0]).unwrap().len(), 32);
    }
    #[test]
    fn contours() {
        assert!(contour_svg(&sample(), 200., 200., 2.)
            .unwrap()
            .contains("<path"));
    }
    #[test]
    fn rejects_nan() {
        assert!(Dem::new(2, 2, vec![0., 1., f64::NAN, 2.]).is_err());
    }
}

pub mod usgs;

pub mod assembly_obj;

pub mod icon_library;
pub mod map_providers;

pub mod landmark_catalog;

pub mod state_culture;

pub mod institution_logos;

pub mod regional_icons;
