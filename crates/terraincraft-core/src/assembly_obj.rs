//! Export assembled terrain tiles to a standard Wavefront OBJ for external 3D inspection.
//! Each tile is positioned in the common landscape coordinate system.
use crate::{Dem, TerrainSettings, Tile};
use std::fmt::Write;

pub type Triangle = [[f64; 3]; 3];

/// One tile's final STL triangles. Coordinates are tile-local, in millimeters.
pub struct AssemblyPart<'a> {
    pub tile: Tile,
    pub triangles: &'a [Triangle],
}

/// Export triangle soup as grouped OBJ. Positions match the actual tile assembly.
/// Materials distinguish adjacent tiles; OBJ is for inspection, not slicing.
pub fn export(
    dem: &Dem,
    settings: TerrainSettings,
    parts: &[AssemblyPart<'_>],
) -> Result<(String, String), String> {
    settings.validate()?;
    if parts.is_empty() {
        return Err("No assembly tiles".into());
    }
    let dx = settings.width_mm / (dem.width - 1) as f64;
    let dy = settings.height_mm / (dem.height - 1) as f64;
    let mut obj = String::from("# TerrainCraft assembly preview; coordinates in millimeters\nmtllib terrain_assembly.mtl\n");
    let mut mtl = String::from("# Alternating materials for identifying tile seams\n");
    for (name, rgb) in [
        ("tile_light", [0.77, 0.82, 0.76]),
        ("tile_dark", [0.42, 0.57, 0.63]),
    ] {
        writeln!(
            mtl,
            "newmtl {name}\nKd {} {} {}\nKa 0.1 0.1 0.1\n",
            rgb[0], rgb[1], rgb[2]
        )
        .unwrap();
    }
    let mut next_index = 1usize;
    for part in parts {
        let t = part.tile;
        if t.x0 >= t.x1 || t.y0 >= t.y1 || t.x1 >= dem.width || t.y1 >= dem.height {
            return Err("Invalid assembly tile bounds".into());
        }
        let ox = t.x0 as f64 * dx;
        let oy = t.y0 as f64 * dy;
        writeln!(obj, "o tile_r{:02}_c{:02}", t.row + 1, t.col + 1).unwrap();
        writeln!(
            obj,
            "usemtl {}",
            if (t.row + t.col) % 2 == 0 {
                "tile_light"
            } else {
                "tile_dark"
            }
        )
        .unwrap();
        for tri in part.triangles {
            for v in tri {
                if !v.iter().all(|c| c.is_finite()) {
                    return Err("Non-finite assembly vertex".into());
                }
                writeln!(obj, "v {:.7} {:.7} {:.7}", v[0] + ox, v[1] + oy, v[2]).unwrap();
            }
            writeln!(
                obj,
                "f {} {} {}",
                next_index,
                next_index + 1,
                next_index + 2
            )
            .unwrap();
            next_index += 3;
        }
    }
    Ok((obj, mtl))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offsets_tile_geometry() {
        let d = Dem::new(3, 2, vec![0.; 6]).unwrap();
        let s = TerrainSettings {
            width_mm: 100.,
            height_mm: 50.,
            base_mm: 5.,
            relief_mm: 10.,
            vertical_exaggeration: 1.,
        };
        let t = Tile {
            row: 0,
            col: 1,
            x0: 1,
            x1: 2,
            y0: 0,
            y1: 1,
        };
        let face = [[[0., 0., 0.], [10., 0., 0.], [0., 10., 0.]]];
        let (obj, mtl) = export(
            &d,
            s,
            &[AssemblyPart {
                tile: t,
                triangles: &face,
            }],
        )
        .unwrap();
        assert!(obj.contains("v 50.0000000 0.0000000 0.0000000"));
        assert!(obj.contains("o tile_r01_c02"));
        assert!(mtl.contains("newmtl tile_dark"));
    }
    #[test]
    fn rejects_invalid_vertices() {
        let d = Dem::new(2, 2, vec![0.; 4]).unwrap();
        let s = TerrainSettings {
            width_mm: 100.,
            height_mm: 100.,
            base_mm: 5.,
            relief_mm: 10.,
            vertical_exaggeration: 1.,
        };
        let t = Tile {
            row: 0,
            col: 0,
            x0: 0,
            x1: 1,
            y0: 0,
            y1: 1,
        };
        let face = [[[f64::NAN, 0., 0.]; 3]];
        assert!(export(
            &d,
            s,
            &[AssemblyPart {
                tile: t,
                triangles: &face
            }]
        )
        .is_err());
    }
}
