//! Rust-native terrain mesh and laser contour geometry. No Python dependency.

/// Re-exported so callers can use the polygon types in laser parts.
pub use geo;

pub mod cartography;
pub mod contours;
pub mod engrave;
pub mod engraving_editor;
pub mod engraving_preview;
pub mod export_settings;
pub mod geocode;
pub mod georef;
pub mod geotiff;
pub mod geoview;
pub mod gis;
pub mod icon_variants;
pub mod intersections;
pub mod label;
pub mod landmarks;
pub mod manufacturing;
pub mod mesh_audit;
pub mod mesh_repair;
pub mod nodata;
pub mod peaks;
pub mod print_export;
pub mod projection;
pub mod registration;
pub mod seam_fabrication;
pub mod seam_joints;
pub mod seam_preflight;
pub mod seam_socket_mesh;
pub mod simplify;
pub mod socket_mesh;
pub mod stack;
pub mod stroke_font;
pub mod terrain_ops;
pub mod threemf;
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
    /// Where the grid sits on the ground, when the source was georeferenced.
    pub georef: Option<georef::GeoReference>,
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
            georef: None,
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
/// Settings that keep real-world proportions for a georeferenced DEM: the model
/// depth follows the ground aspect and the relief is the true vertical scale times
/// `vertical_exaggeration`. Elevations must be in meters.
pub fn true_scale_settings(
    dem: &Dem,
    width_mm: f64,
    base_mm: f64,
    vertical_exaggeration: f64,
) -> Result<TerrainSettings, String> {
    let g = dem
        .georef
        .ok_or("True scale needs a georeferenced elevation source")?;
    let (ground_x, ground_y) = g.sample_span_m(dem.width, dem.height);
    if !(ground_x > 0. && ground_y > 0.) {
        return Err("Georeferenced extent has no ground size".into());
    }
    let mm_per_m = width_mm / ground_x;
    let (lo, hi) = dem.min_max();
    let s = TerrainSettings {
        width_mm,
        height_mm: ground_y * mm_per_m,
        base_mm,
        vertical_exaggeration,
        // `mesh` normalizes by the elevation range, so this gives exactly true scale.
        // Flat terrain still needs a positive value to pass validation.
        relief_mm: ((hi - lo) * mm_per_m).max(1e-6),
    };
    s.validate()?;
    Ok(s)
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
/// `mesh` lays raster rows out along +y, so north (row 0) lands at y = 0 and the tile is a
/// mirror image of the map when seen from above. Convert a tile mesh to north-up
/// coordinates (+x east, +y north, +z up) with the local origin at the tile's south-west
/// corner, restoring outward-facing winding. Apply this after connectors are cut and
/// before anything is exported, labeled or displayed.
pub fn north_up(faces: &mut [[Vec3; 3]], dem: &Dem, s: TerrainSettings, t: Tile) {
    let height = (t.y1 - t.y0) as f64 * s.height_mm / (dem.height - 1) as f64;
    for f in faces.iter_mut() {
        for p in f.iter_mut() {
            p[1] = height - p[1];
        }
        f.swap(1, 2);
    }
}
/// Name used for a tile everywhere: engraved underside, files, 3MF objects, assembly sheet.
pub fn tile_label(t: Tile) -> String {
    format!("{}-{}", t.row + 1, t.col + 1)
}
/// Final steps for one north-up tile mesh (see `north_up`), after connectors are cut and
/// winding repaired: engrave its label and simplify it, per `opts`. A step that cannot be
/// applied, or that would leave the mesh open, is skipped and explained in the warnings.
pub fn finish_tile(
    mut faces: Vec<[Vec3; 3]>,
    s: TerrainSettings,
    t: Tile,
    opts: &export_settings::PrintExportSettings,
) -> Result<(Vec<[Vec3; 3]>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let closed = |f: &[[Vec3; 3]]| mesh_audit::audit(f).is_closed_oriented_manifold();
    let name = tile_label(t);
    if opts.labels {
        // Scale the text with the tile: 1 mm strokes on small tiles, up to 3 mm on large.
        let extent = |k: usize| {
            let (lo, hi) = faces
                .iter()
                .flatten()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    (a.min(p[k]), b.max(p[k]))
                });
            hi - lo
        };
        let label = label::LabelSettings {
            cell_mm: (extent(0).min(extent(1)) / 50.).clamp(1., 3.),
            ..label::LabelSettings::default()
        };
        if label.depth_mm >= s.base_mm {
            warnings.push(format!(
                "Tile {name}: base is too thin for a {} mm label",
                label.depth_mm
            ));
        } else {
            match label::engrave(&faces, &name, label) {
                Ok(f) if closed(&f) => faces = f,
                Ok(_) => warnings.push(format!("Tile {name}: label skipped (mesh would open)")),
                Err(e) => warnings.push(format!("Tile {name}: label skipped ({e})")),
            }
        }
    }
    if let Some(tolerance) = opts.simplify_mm {
        let f = simplify::simplify(&faces, tolerance, s.base_mm)?;
        if closed(&f) {
            faces = f;
        } else {
            warnings.push(format!(
                "Tile {name}: simplification skipped (mesh would open)"
            ));
        }
    }
    Ok((faces, warnings))
}
/// Where a north-up tile's local origin (its south-west corner) sits in the assembled model.
pub fn north_up_origin(dem: &Dem, s: TerrainSettings, t: Tile) -> [f64; 2] {
    [
        t.x0 as f64 * s.width_mm / (dem.width - 1) as f64,
        (dem.height - 1 - t.y1) as f64 * s.height_mm / (dem.height - 1) as f64,
    ]
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
            for pair in pts.as_chunks::<2>().0 {
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
    fn true_scale_preserves_proportions() {
        let mut d = Dem::new(11, 6, (0..66).map(|i| (i % 11) as f64 * 100.).collect()).unwrap();
        // 10 m pixels: samples span 100 m east-west and 50 m north-south.
        d.georef = Some(georef::GeoReference {
            epsg: Some(32610),
            units: georef::CrsUnits::Meters,
            origin_x: 0.,
            origin_y: 0.,
            pixel_width: 10.,
            pixel_height: 10.,
        });
        let s = true_scale_settings(&d, 200., 3., 1.).unwrap();
        assert_eq!(s.height_mm, 100.);
        // 1000 m of relief at 2 mm per meter.
        assert_eq!(s.relief_mm, 2000.);
        assert!(true_scale_settings(&sample(), 200., 3., 1.).is_err());
    }
    #[test]
    fn north_up_puts_north_east_peak_at_back_right() {
        // Raster row 0 is north; only the north-east sample is high.
        let mut v = vec![0.; 9];
        v[2] = 10.;
        let d = Dem::new(3, 3, v).unwrap();
        let s = settings();
        let t = Tile {
            row: 0,
            col: 0,
            x0: 0,
            x1: 2,
            y0: 0,
            y1: 2,
        };
        let mut f = mesh(&d, s, t).unwrap();
        let before = mesh_audit::audit(&f);
        north_up(&mut f, &d, s, t);
        let peak = f
            .iter()
            .flatten()
            .fold([0.; 3], |a: [f64; 3], p| if p[2] > a[2] { *p } else { a });
        assert_eq!((peak[0], peak[1]), (200., 200.));
        assert!(before.is_closed_oriented_manifold());
        assert!(mesh_audit::audit(&f).is_closed_oriented_manifold());
        assert!(manufacturing::inspect(&f).signed_volume_mm3 > 0.);
        // The south-most tile row starts at y = 0 in the assembly.
        let ts = tiles(&d, s, 110., 110., 5.).unwrap();
        let south = ts.iter().find(|t| t.row == 1 && t.col == 0).unwrap();
        assert_eq!(north_up_origin(&d, s, *south), [0., 0.]);
        let north = ts.iter().find(|t| t.row == 0 && t.col == 0).unwrap();
        assert_eq!(north_up_origin(&d, s, *north), [0., 100.]);
    }
    #[test]
    fn rejects_nan() {
        assert!(Dem::new(2, 2, vec![0., 1., f64::NAN, 2.]).is_err());
    }
}

pub mod usgs;

pub mod assembly_obj;
pub mod assembly_sheet;

pub mod icon_library;
pub mod map_providers;

pub mod landmark_catalog;

pub mod state_culture;

pub mod institution_logos;

pub mod regional_icons;
