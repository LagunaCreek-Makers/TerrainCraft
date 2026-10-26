//! Tile identification text engraved into the flat underside of a tile mesh.
//!
//! Text uses a seven-segment style font on a 4 x 7 cell grid. Segments are full-length
//! bars that meet edge to edge, so filled cells never touch only at a corner (which would
//! make a non-manifold "pinch"), and every bar and wall between pockets is at least one
//! cell wide. The text is mirrored so it reads correctly when the tile is turned over.
use std::collections::HashSet;

type Tri = [[f64; 3]; 3];
/// A font segment: whether it is lit, and its first and last cell.
type Bar = (bool, (i32, i32), (i32, i32));
/// One side of the frame around the text: outline vertices, then box edge vertices.
type FrameSide = (Vec<[f64; 3]>, Vec<[f64; 3]>);

/// Engraving size and placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelSettings {
    /// Side of one font cell in millimeters; characters are 4 x 7 cells.
    pub cell_mm: f64,
    /// Engraving depth into the underside in millimeters.
    pub depth_mm: f64,
}

impl Default for LabelSettings {
    fn default() -> Self {
        // 1 mm strokes are clean at a 0.4 mm nozzle; 0.6 mm is three 0.2 mm layers.
        Self {
            cell_mm: 1.,
            depth_mm: 0.6,
        }
    }
}

const GLYPH_W: i32 = 4;
const GLYPH_H: i32 = 7;
const GAP: i32 = 2;

/// Seven-segment pattern (bits a..g = top, upper right, lower right, bottom, lower left,
/// upper left, middle) for the characters labels use.
pub(crate) fn segments(c: char) -> Option<u8> {
    Some(match c {
        '0' => 0b0111111,
        '1' => 0b0000110,
        '2' => 0b1011011,
        '3' => 0b1001111,
        '4' => 0b1100110,
        '5' => 0b1101101,
        '6' => 0b1111101,
        '7' => 0b0000111,
        '8' => 0b1111111,
        '9' => 0b1101111,
        '-' => 0b1000000,
        'L' => 0b0111000,
        ' ' => 0,
        _ => return None,
    })
}

/// Filled font cells of `text`, as (column, row) with row 0 at the top of the glyph.
fn cells(text: &str) -> Result<HashSet<(i32, i32)>, String> {
    let mut out = HashSet::new();
    for (i, c) in text.chars().enumerate() {
        let bits = segments(c).ok_or_else(|| format!("Label character '{c}' is not supported"))?;
        let x0 = i as i32 * (GLYPH_W + GAP);
        let (r, m, b) = (0, GLYPH_H / 2, GLYPH_H - 1);
        let bars: [Bar; 7] = [
            (bits & 1 != 0, (0, r), (GLYPH_W - 1, r)),
            (bits & 2 != 0, (GLYPH_W - 1, r), (GLYPH_W - 1, m)),
            (bits & 4 != 0, (GLYPH_W - 1, m), (GLYPH_W - 1, b)),
            (bits & 8 != 0, (0, b), (GLYPH_W - 1, b)),
            (bits & 16 != 0, (0, m), (0, b)),
            (bits & 32 != 0, (0, r), (0, m)),
            (bits & 64 != 0, (0, m), (GLYPH_W - 1, m)),
        ];
        for (on, (ax, ay), (bx, by)) in bars {
            if on {
                for x in ax..=bx {
                    for y in ay..=by {
                        out.insert((x0 + x, y));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Width and height of `text` in millimeters.
pub fn text_size(text: &str, s: LabelSettings) -> (f64, f64) {
    let n = text.chars().count() as i32;
    (
        (n * (GLYPH_W + GAP) - GAP).max(0) as f64 * s.cell_mm,
        GLYPH_H as f64 * s.cell_mm,
    )
}

/// Engrave `text` centered in the underside of a closed tile mesh whose bottom is one
/// axis-aligned rectangle at the lowest z. The bottom is rebuilt on a coarse grid that
/// reuses every vertex already on the tile outline, so it stays joined to the walls.
pub fn engrave(faces: &[Tri], text: &str, s: LabelSettings) -> Result<Vec<Tri>, String> {
    if !(s.cell_mm > 0. && s.depth_mm > 0.) {
        return Err("Label cell size and depth must be positive".into());
    }
    let z0 = faces
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::INFINITY, f64::min);
    let is_bottom = |t: &Tri| {
        t.iter().all(|p| p[2] == z0)
            && (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0])
                < 0.
    };
    let (bottom, mut out): (Vec<Tri>, Vec<Tri>) = faces.iter().partition(|t| is_bottom(t));
    let pts: Vec<[f64; 3]> = bottom.iter().flatten().copied().collect();
    let (x_min, x_max) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p[0]), b.max(p[0]))
        });
    let (y_min, y_max) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p[1]), b.max(p[1]))
        });
    let area: f64 = bottom
        .iter()
        .map(|t| {
            ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]))
                .abs()
                * 0.5
        })
        .sum();
    let rect = (x_max - x_min) * (y_max - y_min);
    if bottom.is_empty() || (area - rect).abs() > 1e-6 * rect.max(1.) {
        return Err("Tile underside is not a plain rectangle (underside sockets?)".into());
    }
    let (tw, th) = text_size(text, s);
    let margin = 2. * s.cell_mm;
    if tw + 2. * margin > x_max - x_min || th + 2. * margin > y_max - y_min {
        return Err(format!("Tile is too small for label \"{text}\""));
    }
    let filled = cells(text)?;
    // Mirror x so the text reads left to right when the tile is viewed from below. The
    // tile's +y is north on the map; viewed from below with north still away from the
    // reader, rows run with y.
    let right = (x_min + x_max + tw) * 0.5;
    let top = (y_min + y_max + th) * 0.5;
    let n_cols = (tw / s.cell_mm).round() as i32;
    let font_x = |col: i32| right - col as f64 * s.cell_mm;
    let font_y = |row: i32| top - row as f64 * s.cell_mm;
    // The text sits in a box one cell larger than the text on every side. Inside it, a
    // grid on the font lines; outside it, four convex trapezoids out to the tile outline.
    let (bx0, bx1) = (font_x(n_cols + 1), font_x(-1));
    let (by0, by1) = (font_y(GLYPH_H + 1), font_y(-1));
    let mut xs: Vec<f64> = (-1..=n_cols + 1).map(font_x).collect();
    let mut ys: Vec<f64> = (-1..=GLYPH_H + 1).map(font_y).collect();
    xs.reverse();
    ys.reverse();
    let void = |i: usize, j: usize| {
        let (cx, cy) = ((xs[i] + xs[i + 1]) * 0.5, (ys[j] + ys[j + 1]) * 0.5);
        let col = ((right - cx) / s.cell_mm).floor() as i32;
        let row = ((top - cy) / s.cell_mm).floor() as i32;
        filled.contains(&(col, row))
    };
    let (zc, zb) = (z0 + s.depth_mm, z0);
    for i in 0..xs.len() - 1 {
        for j in 0..ys.len() - 1 {
            let (x0, x1, y0, y1) = (xs[i], xs[i + 1], ys[j], ys[j + 1]);
            if !void(i, j) {
                // Solid underside cell, facing down.
                out.push([[x0, y0, zb], [x0, y1, zb], [x1, y1, zb]]);
                out.push([[x0, y0, zb], [x1, y1, zb], [x1, y0, zb]]);
                continue;
            }
            // Pocket ceiling: solid above, so it faces down.
            out.push([[x0, y0, zc], [x0, y1, zc], [x1, y1, zc]]);
            out.push([[x0, y0, zc], [x1, y1, zc], [x1, y0, zc]]);
            // Walls toward each solid neighbor, facing into the pocket. The box margin
            // is always solid, so pockets never sit in the first or last row or column.
            let wall = |a: [f64; 2], b: [f64; 2]| -> [Tri; 2] {
                [
                    [[a[0], a[1], zb], [b[0], b[1], zb], [b[0], b[1], zc]],
                    [[a[0], a[1], zb], [b[0], b[1], zc], [a[0], a[1], zc]],
                ]
            };
            if !void(i - 1, j) {
                out.extend(wall([x0, y0], [x0, y1]));
            }
            if !void(i + 1, j) {
                out.extend(wall([x1, y1], [x1, y0]));
            }
            if !void(i, j - 1) {
                out.extend(wall([x1, y0], [x0, y0]));
            }
            if !void(i, j + 1) {
                out.extend(wall([x0, y1], [x1, y1]));
            }
        }
    }
    // Frame between the tile outline and the box. Each side is a convex polygon made of the
    // outline vertices already on that tile edge (shared with the walls) and the box edge
    // vertices (shared with the grid), fanned from its centroid so collinear runs of
    // vertices never form zero-area triangles.
    let outline = |f: &dyn Fn(&[f64; 3]) -> bool, key: fn(&[f64; 3]) -> f64| {
        let mut v: Vec<[f64; 3]> = pts.iter().copied().filter(|p| f(p)).collect();
        v.sort_by(|a, b| key(a).total_cmp(&key(b)));
        v.dedup();
        v
    };
    let at = |x: f64, y: f64| [x, y, zb];
    let sides: [FrameSide; 4] = [
        (
            outline(&|p| p[1] == y_min, |p| p[0]),
            xs.iter().rev().map(|&x| at(x, by0)).collect(),
        ),
        (
            outline(&|p| p[0] == x_max, |p| p[1]),
            ys.iter().rev().map(|&y| at(bx1, y)).collect(),
        ),
        (
            outline(&|p| p[1] == y_max, |p| -p[0]),
            xs.iter().map(|&x| at(x, by1)).collect(),
        ),
        (
            outline(&|p| p[0] == x_min, |p| -p[1]),
            ys.iter().map(|&y| at(bx0, y)).collect(),
        ),
    ];
    for (outer, inner) in sides {
        let ring: Vec<[f64; 3]> = outer.into_iter().chain(inner).collect();
        let n = ring.len() as f64;
        let c = ring
            .iter()
            .fold([0., 0., zb], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n, zb]);
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            // The ring runs counterclockwise seen from above; reverse to face down.
            out.push([c, b, a]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mesh, mesh_audit::audit, Dem, TerrainSettings, Tile};

    fn tile_mesh() -> Vec<Tri> {
        let d = Dem::new(31, 21, (0..651).map(|i| (i % 31) as f64).collect()).unwrap();
        let s = TerrainSettings {
            width_mm: 60.,
            height_mm: 40.,
            base_mm: 3.,
            vertical_exaggeration: 1.,
            relief_mm: 10.,
        };
        mesh(
            &d,
            s,
            Tile {
                row: 0,
                col: 0,
                x0: 0,
                x1: 30,
                y0: 0,
                y1: 20,
            },
        )
        .unwrap()
    }

    #[test]
    fn engraved_tile_is_closed_and_has_pockets() {
        let full = tile_mesh();
        let out = engrave(&full, "12-3", LabelSettings::default()).unwrap();
        let a = audit(&out);
        assert!(a.is_closed_oriented_manifold(), "{a:?}");
        // Pocket ceilings sit at the engraving depth.
        assert!(out.iter().flatten().any(|p| (p[2] - 0.6).abs() < 1e-12));
        // Volume drops by exactly the engraved cells.
        let filled = cells("12-3").unwrap().len() as f64;
        let vol = |f: &[Tri]| crate::manufacturing::inspect(f).signed_volume_mm3;
        assert!((vol(&full) - vol(&out) - filled * 0.6).abs() < 1e-6);
    }

    #[test]
    fn text_is_mirrored_for_reading_from_below() {
        // "1" fills only the glyph's right column; mirrored, its pocket is on the left
        // of the glyph box in tile coordinates.
        let out = engrave(&tile_mesh(), "1", LabelSettings::default()).unwrap();
        let ceil: Vec<f64> = out
            .iter()
            .flatten()
            .filter(|p| (p[2] - 0.6).abs() < 1e-12)
            .map(|p| p[0])
            .collect();
        let (lo, hi) = ceil
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                (a.min(*x), b.max(*x))
            });
        assert_eq!((lo, hi), (28., 29.));
    }

    #[test]
    fn rejects_unsupported_text_and_small_tiles() {
        assert!(engrave(&tile_mesh(), "R1", LabelSettings::default()).is_err());
        assert!(engrave(&tile_mesh(), "1234567890", LabelSettings::default()).is_err());
    }
}
