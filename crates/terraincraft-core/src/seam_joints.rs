//! Matched horizontal dowel locations on vertical tile seams.
//! These are fabrication guides, not yet boolean-subtracted STL sockets.
use crate::{Dem, TerrainSettings, Tile};
use std::fmt::Write;
#[derive(Clone, Copy, Debug)]
pub struct SeamSettings {
    pub diameter_mm: f64,
    pub clearance_mm: f64,
    pub depth_mm: f64,
    pub center_height_mm: f64,
    pub end_inset_mm: f64,
    pub spacing_mm: f64,
}
impl Default for SeamSettings {
    fn default() -> Self {
        Self {
            diameter_mm: 3.,
            clearance_mm: 0.3,
            depth_mm: 6.,
            center_height_mm: 3.,
            end_inset_mm: 10.,
            spacing_mm: 45.,
        }
    }
}
impl SeamSettings {
    pub fn validate(self, base_mm: f64) -> Result<(), String> {
        let a = [
            self.diameter_mm,
            self.depth_mm,
            self.center_height_mm,
            self.end_inset_mm,
            self.spacing_mm,
        ];
        if a.iter().any(|v| !v.is_finite() || *v <= 0.)
            || !self.clearance_mm.is_finite()
            || self.clearance_mm < 0.
        {
            return Err("Invalid seam joint settings".into());
        }
        let r = (self.diameter_mm + self.clearance_mm) / 2.;
        if self.center_height_mm - r <= 0. || self.center_height_mm + r >= base_mm {
            return Err("Seam dowels must fit entirely inside the terrain base".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct SeamHole {
    pub seam_id: String,
    pub tile_row: usize,
    pub tile_col: usize,
    pub side: &'static str,
    pub along_mm: f64,
    pub height_mm: f64,
    pub depth_mm: f64,
    pub diameter_mm: f64,
}
fn positions(length: f64, s: SeamSettings) -> Vec<f64> {
    if length < 2. * s.end_inset_mm {
        return vec![];
    }
    let count = 1 + ((length - 2. * s.end_inset_mm) / s.spacing_mm).floor() as usize;
    (0..count)
        .map(|i| {
            if count == 1 {
                length / 2.
            } else {
                s.end_inset_mm + (length - 2. * s.end_inset_mm) * i as f64 / (count - 1) as f64
            }
        })
        .collect()
}
/// Both tiles receive holes at the same world-space seam locations, measured
/// from the low-coordinate end of their respective vertical wall.
pub fn plan(
    d: &Dem,
    t: &[Tile],
    terrain: TerrainSettings,
    s: SeamSettings,
) -> Result<Vec<SeamHole>, String> {
    terrain.validate()?;
    s.validate(terrain.base_mm)?;
    let sx = terrain.width_mm / (d.width - 1) as f64;
    let sy = terrain.height_mm / (d.height - 1) as f64;
    let mut out = vec![];
    for (i, a) in t.iter().enumerate() {
        for b in t.iter().skip(i + 1) {
            let (vertical, lo, hi, scale, side_a, side_b) = if a.x1 == b.x0 || b.x1 == a.x0 {
                (
                    true,
                    a.y0.max(b.y0),
                    a.y1.min(b.y1),
                    sy,
                    if a.x1 == b.x0 { "east" } else { "west" },
                    if a.x1 == b.x0 { "west" } else { "east" },
                )
            } else if a.y1 == b.y0 || b.y1 == a.y0 {
                (
                    false,
                    a.x0.max(b.x0),
                    a.x1.min(b.x1),
                    sx,
                    if a.y1 == b.y0 { "south" } else { "north" },
                    if a.y1 == b.y0 { "north" } else { "south" },
                )
            } else {
                continue;
            };
            if hi <= lo {
                continue;
            }
            let id = format!(
                "{}-r{}c{}-r{}c{}",
                if vertical { "V" } else { "H" },
                a.row + 1,
                a.col + 1,
                b.row + 1,
                b.col + 1
            );
            for offset in positions((hi - lo) as f64 * scale, s) {
                let world = lo as f64 * scale + offset;
                for (tile, side) in [(a, side_a), (b, side_b)] {
                    let start = if vertical { tile.y0 } else { tile.x0 };
                    out.push(SeamHole {
                        seam_id: id.clone(),
                        tile_row: tile.row,
                        tile_col: tile.col,
                        side,
                        along_mm: world - start as f64 * scale,
                        height_mm: s.center_height_mm,
                        depth_mm: s.depth_mm,
                        diameter_mm: s.diameter_mm + s.clearance_mm,
                    });
                }
            }
        }
    }
    Ok(out)
}
pub fn manifest(holes: &[SeamHole]) -> String {
    let mut out=String::from("seam_id,tile_row,tile_col,wall,along_mm,center_height_mm,hole_diameter_mm,drill_depth_mm\n");
    for h in holes {
        writeln!(
            out,
            "{},{},{},{},{:.4},{:.4},{:.4},{:.4}",
            h.seam_id,
            h.tile_row + 1,
            h.tile_col + 1,
            h.side,
            h.along_mm,
            h.height_mm,
            h.diameter_mm,
            h.depth_mm
        )
        .unwrap();
    }
    out
}
/// SVG wall drilling template. Horizontal position is measured from the
/// minimum world-coordinate end of the wall, not from the observer's left.
pub fn wall_svg(length_mm: f64, base_mm: f64, holes: &[&SeamHole]) -> Result<String, String> {
    if !length_mm.is_finite() || !base_mm.is_finite() || length_mm <= 0. || base_mm <= 0. {
        return Err("Invalid wall dimensions".into());
    }
    let mut out=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{length_mm}mm\" height=\"{base_mm}mm\" viewBox=\"0 0 {length_mm} {base_mm}\"><g stroke=\"black\" fill=\"none\" stroke-width=\"0.15\"><rect x=\"0\" y=\"0\" width=\"{length_mm}\" height=\"{base_mm}\"/>");
    for h in holes {
        if h.along_mm < 0. || h.along_mm > length_mm {
            return Err("Hole outside wall".into());
        }
        write!(
            out,
            "<circle cx=\"{:.4}\" cy=\"{:.4}\" r=\"{:.4}\"/>",
            h.along_mm,
            base_mm - h.height_mm,
            h.diameter_mm / 2.
        )
        .unwrap();
    }
    out.push_str("</g></svg>");
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_world_coordinates() {
        let d = Dem::new(5, 3, vec![0.; 15]).unwrap();
        let s = TerrainSettings {
            width_mm: 200.,
            height_mm: 100.,
            base_mm: 8.,
            relief_mm: 10.,
            vertical_exaggeration: 1.,
        };
        let ts = vec![
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
        let h = plan(&d, &ts, s, SeamSettings::default()).unwrap();
        assert!(!h.is_empty());
        for pair in h.chunks_exact(2) {
            assert_eq!(pair[0].seam_id, pair[1].seam_id);
            assert!((pair[0].along_mm - pair[1].along_mm).abs() < 1e-8);
        }
    }
    #[test]
    fn thin_base_rejected() {
        let d = Dem::new(2, 2, vec![0.; 4]).unwrap();
        let s = TerrainSettings {
            width_mm: 100.,
            height_mm: 100.,
            base_mm: 3.,
            relief_mm: 10.,
            vertical_exaggeration: 1.,
        };
        assert!(plan(&d, &[], s, SeamSettings::default()).is_err());
    }
}
