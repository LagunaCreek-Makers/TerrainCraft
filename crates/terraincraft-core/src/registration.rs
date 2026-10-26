//! Optional post-print tile registration: printable dowels and accurately dimensioned
//! underside drilling templates. Terrain STLs are intentionally unchanged.
use crate::{Dem, TerrainSettings, Tile};
use std::fmt::Write;

#[derive(Clone, Copy, Debug)]
pub struct RegistrationSettings {
    pub pin_diameter_mm: f64,
    pub hole_clearance_mm: f64,
    pub pin_length_mm: f64,
    pub inset_mm: f64,
    pub spacing_mm: f64,
}
impl Default for RegistrationSettings {
    fn default() -> Self {
        Self {
            pin_diameter_mm: 4.,
            hole_clearance_mm: 0.3,
            pin_length_mm: 8.,
            inset_mm: 8.,
            spacing_mm: 35.,
        }
    }
}
impl RegistrationSettings {
    pub fn validate(self) -> Result<(), String> {
        if !self.pin_diameter_mm.is_finite()
            || self.pin_diameter_mm <= 0.
            || !self.hole_clearance_mm.is_finite()
            || self.hole_clearance_mm < 0.
            || !self.pin_length_mm.is_finite()
            || self.pin_length_mm <= 0.
            || !self.inset_mm.is_finite()
            || self.inset_mm <= 0.
            || !self.spacing_mm.is_finite()
            || self.spacing_mm <= 0.
        {
            return Err("Invalid registration dimensions".into());
        }
        if self.inset_mm < (self.pin_diameter_mm + self.hole_clearance_mm) / 2. {
            return Err("Inset must accommodate hole radius".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Hole {
    pub x_mm: f64,
    pub y_mm: f64,
    pub seam_id: String,
}
#[derive(Clone, Debug)]
pub struct TileGuide {
    pub row: usize,
    pub col: usize,
    pub width_mm: f64,
    pub height_mm: f64,
    pub holes: Vec<Hole>,
}

/// Places paired holes on the UNDERSIDE of adjoining tiles. The guides are
/// intended for post-print drilling, NOT subtraction from the exported mesh.
pub fn guides(
    dem: &Dem,
    s: TerrainSettings,
    tiles: &[Tile],
    reg: RegistrationSettings,
) -> Result<Vec<TileGuide>, String> {
    s.validate()?;
    reg.validate()?;
    let sx = s.width_mm / (dem.width - 1) as f64;
    let sy = s.height_mm / (dem.height - 1) as f64;
    let mut result = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if tile.x0 >= tile.x1 || tile.y0 >= tile.y1 || tile.x1 >= dem.width || tile.y1 >= dem.height
        {
            return Err("Invalid tile bounds".into());
        }
        result.push(TileGuide {
            row: tile.row,
            col: tile.col,
            width_mm: (tile.x1 - tile.x0) as f64 * sx,
            height_mm: (tile.y1 - tile.y0) as f64 * sy,
            holes: vec![],
        });
    }
    for i in 0..tiles.len() {
        for j in i + 1..tiles.len() {
            let a = tiles[i];
            let b = tiles[j];
            if a.x1 == b.x0 || b.x1 == a.x0 {
                let lo = a.y0.max(b.y0);
                let hi = a.y1.min(b.y1);
                if lo >= hi {
                    continue;
                }
                let len = (hi - lo) as f64 * sy;
                let count = if len >= 2. * reg.inset_mm + reg.spacing_mm {
                    2
                } else if len >= 2. * reg.inset_mm {
                    1
                } else {
                    0
                };
                let seam = format!(
                    "V-r{}c{}-r{}c{}",
                    a.row + 1,
                    a.col + 1,
                    b.row + 1,
                    b.col + 1
                );
                for k in 0..count {
                    let y = if count == 1 {
                        (lo as f64 + hi as f64) * sy / 2.
                    } else {
                        lo as f64 * sy + reg.inset_mm + (len - 2. * reg.inset_mm) * k as f64
                    };
                    let xa = if a.x1 == b.x0 {
                        result[i].width_mm - reg.inset_mm
                    } else {
                        reg.inset_mm
                    };
                    let xb = if a.x1 == b.x0 {
                        reg.inset_mm
                    } else {
                        result[j].width_mm - reg.inset_mm
                    };
                    result[i].holes.push(Hole {
                        x_mm: xa,
                        y_mm: y - a.y0 as f64 * sy,
                        seam_id: seam.clone(),
                    });
                    result[j].holes.push(Hole {
                        x_mm: xb,
                        y_mm: y - b.y0 as f64 * sy,
                        seam_id: seam.clone(),
                    });
                }
            } else if a.y1 == b.y0 || b.y1 == a.y0 {
                let lo = a.x0.max(b.x0);
                let hi = a.x1.min(b.x1);
                if lo >= hi {
                    continue;
                }
                let len = (hi - lo) as f64 * sx;
                let count = if len >= 2. * reg.inset_mm + reg.spacing_mm {
                    2
                } else if len >= 2. * reg.inset_mm {
                    1
                } else {
                    0
                };
                let seam = format!(
                    "H-r{}c{}-r{}c{}",
                    a.row + 1,
                    a.col + 1,
                    b.row + 1,
                    b.col + 1
                );
                for k in 0..count {
                    let x = if count == 1 {
                        (lo as f64 + hi as f64) * sx / 2.
                    } else {
                        lo as f64 * sx + reg.inset_mm + (len - 2. * reg.inset_mm) * k as f64
                    };
                    let ya = if a.y1 == b.y0 {
                        result[i].height_mm - reg.inset_mm
                    } else {
                        reg.inset_mm
                    };
                    let yb = if a.y1 == b.y0 {
                        reg.inset_mm
                    } else {
                        result[j].height_mm - reg.inset_mm
                    };
                    result[i].holes.push(Hole {
                        x_mm: x - a.x0 as f64 * sx,
                        y_mm: ya,
                        seam_id: seam.clone(),
                    });
                    result[j].holes.push(Hole {
                        x_mm: x - b.x0 as f64 * sx,
                        y_mm: yb,
                        seam_id: seam.clone(),
                    });
                }
            }
        }
    }
    Ok(result)
}

pub fn guide_svg(g: &TileGuide, reg: RegistrationSettings) -> Result<String, String> {
    reg.validate()?;
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.4}mm\" height=\"{:.4}mm\" viewBox=\"0 0 {:.4} {:.4}\"><g fill=\"none\" stroke=\"black\" stroke-width=\"0.15\"><rect x=\"0.075\" y=\"0.075\" width=\"{:.4}\" height=\"{:.4}\"/>",g.width_mm,g.height_mm,g.width_mm,g.height_mm,(g.width_mm-0.15).max(0.),(g.height_mm-0.15).max(0.));
    for h in &g.holes {
        write!(svg,"<circle cx=\"{:.4}\" cy=\"{:.4}\" r=\"{:.4}\"/><path d=\"M {:.4} {:.4} h 2 M {:.4} {:.4} v 2\"/>",h.x_mm,h.y_mm,(reg.pin_diameter_mm+reg.hole_clearance_mm)/2.,h.x_mm-1.,h.y_mm,h.x_mm,h.y_mm-1.).unwrap();
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}

pub fn guide_manifest(guides: &[TileGuide], reg: RegistrationSettings) -> String {
    let mut out =
        String::from("tile,row,col,seam_id,x_mm,y_mm,hole_diameter_mm,recommended_depth_mm\n");
    for g in guides {
        for h in &g.holes {
            writeln!(
                out,
                "tile_r{:02}_c{:02}.stl,{},{},{},{:.4},{:.4},{:.4},{:.4}",
                g.row + 1,
                g.col + 1,
                g.row + 1,
                g.col + 1,
                h.seam_id,
                h.x_mm,
                h.y_mm,
                reg.pin_diameter_mm + reg.hole_clearance_mm,
                reg.pin_length_mm / 2.
            )
            .unwrap();
        }
    }
    out
}

/// Cylindrical pin STL triangles, centered in XY, with its base at Z=0.
pub fn pin_mesh(reg: RegistrationSettings, segments: usize) -> Result<Vec<[[f64; 3]; 3]>, String> {
    reg.validate()?;
    if !(12..=256).contains(&segments) {
        return Err("Pin segment count must be 12..256".into());
    }
    let r = reg.pin_diameter_mm / 2.;
    let mut triangles = Vec::with_capacity(segments * 4);
    for i in 0..segments {
        let a = std::f64::consts::TAU * i as f64 / segments as f64;
        let b = std::f64::consts::TAU * (i + 1) as f64 / segments as f64;
        let (p, q) = (
            [r * a.cos(), r * a.sin(), 0.],
            [r * b.cos(), r * b.sin(), 0.],
        );
        let (u, v) = (
            [p[0], p[1], reg.pin_length_mm],
            [q[0], q[1], reg.pin_length_mm],
        );
        triangles.push([p, q, v]);
        triangles.push([p, v, u]);
        triangles.push([[0., 0., 0.], q, p]);
        triangles.push([[0., 0., reg.pin_length_mm], u, v]);
    }
    Ok(triangles)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Dem, TerrainSettings, Vec<Tile>) {
        let d = Dem::new(5, 3, vec![0.; 15]).unwrap();
        let s = TerrainSettings {
            width_mm: 100.,
            height_mm: 60.,
            base_mm: 3.,
            relief_mm: 5.,
            vertical_exaggeration: 1.,
        };
        let t = vec![
            Tile {
                row: 0,
                col: 0,
                x0: 0,
                x1: 2,
                y0: 0,
                y1: 2,
            },
            Tile {
                row: 0,
                col: 1,
                x0: 2,
                x1: 4,
                y0: 0,
                y1: 2,
            },
        ];
        (d, s, t)
    }
    #[test]
    fn paired_holes() {
        let (d, s, t) = fixture();
        let g = guides(&d, s, &t, RegistrationSettings::default()).unwrap();
        assert_eq!(g[0].holes.len(), 2);
        assert_eq!(g[1].holes.len(), 2);
        assert!((g[0].holes[0].y_mm - g[1].holes[0].y_mm).abs() < 1e-9);
        assert_eq!(g[0].holes[0].seam_id, g[1].holes[0].seam_id);
    }
    #[test]
    fn printable_pin() {
        assert_eq!(
            pin_mesh(RegistrationSettings::default(), 32).unwrap().len(),
            128
        );
    }
    #[test]
    fn invalid_clearance() {
        let r = RegistrationSettings {
            hole_clearance_mm: -1.,
            ..RegistrationSettings::default()
        };
        assert!(r.validate().is_err());
    }
    #[test]
    fn svg_dimensions() {
        let (d, s, t) = fixture();
        let r = RegistrationSettings::default();
        let g = guides(&d, s, &t, r).unwrap();
        assert!(guide_svg(&g[0], r).unwrap().contains("circle"));
    }
}
