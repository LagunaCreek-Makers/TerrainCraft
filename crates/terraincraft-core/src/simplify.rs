//! Error-bounded simplification of terrain tile meshes.
//!
//! Grid meshes spend two triangles per elevation sample on the top surface and two more on
//! the flat underside. Most of them add nothing a printer can show. This removes vertices by
//! half-edge collapse (a vertex merges into a neighbor, so no new positions are invented):
//!
//! - Only two kinds of vertex may move. *Surface* vertices have every incident face on the
//!   terrain surface: facing up, with all corners at or above `protect_below_z` (the base
//!   top), so connector cavities in the base are never touched. *Bottom* vertices have every
//!   incident face in the lowest horizontal plane, facing down.
//! - Everything else is locked: tile edges and walls, connectors, labels. Adjacent tiles keep
//!   identical boundary vertices, so they still meet exactly.
//! - Each collapse keeps every affected triangle's orientation in the XY plane, so the
//!   surface stays a valid heightfield with no folds.
//! - Every removed surface vertex is remembered. A collapse is rejected if any of them would
//!   end up more than `tolerance_mm` above or below the simplified surface, so the result is
//!   never further than that from the original samples.
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

type Tri = [[f64; 3]; 3];
/// Removed surface vertices and the face each one lands in after a collapse.
type Placed = Vec<(usize, [f64; 3])>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Surface,
    Bottom,
    Other,
}

struct Mesh {
    verts: Vec<[f64; 3]>,
    faces: Vec<[usize; 3]>,
    class: Vec<Class>,
    alive: Vec<bool>,
    vert_faces: Vec<Vec<usize>>,
    /// Removed surface vertices lying inside each face (by XY), for the error bound.
    points: Vec<Vec<[f64; 3]>>,
    stamp: Vec<u32>,
}

/// XY signed area times two.
fn xy_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Height of the plane through `t` at `p`'s XY, if `p` lies inside `t` in XY.
fn height_in(t: [[f64; 3]; 3], p: [f64; 3]) -> Option<f64> {
    let area = xy_area(t[0], t[1], t[2]);
    let w0 = xy_area(p, t[1], t[2]) / area;
    let w1 = xy_area(t[0], p, t[2]) / area;
    let w2 = 1. - w0 - w1;
    const EPS: f64 = -1e-9;
    (w0 >= EPS && w1 >= EPS && w2 >= EPS).then(|| w0 * t[0][2] + w1 * t[1][2] + w2 * t[2][2])
}

impl Mesh {
    fn build(faces: &[Tri], protect_below_z: f64) -> Self {
        let mut index = HashMap::new();
        let mut verts = Vec::new();
        let mut key = |p: [f64; 3]| {
            let k = p.map(|v| if v == 0. { 0f64.to_bits() } else { v.to_bits() });
            *index.entry(k).or_insert_with(|| {
                verts.push(p);
                verts.len() - 1
            })
        };
        let ids: Vec<[usize; 3]> = faces.iter().map(|t| t.map(&mut key)).collect();
        let z_min = verts.iter().map(|v| v[2]).fold(f64::INFINITY, f64::min);
        let class = faces
            .iter()
            .map(|t| {
                let area = xy_area(t[0], t[1], t[2]);
                if area > 0. && t.iter().all(|p| p[2] >= protect_below_z) {
                    Class::Surface
                } else if area < 0. && t.iter().all(|p| p[2] == z_min) {
                    Class::Bottom
                } else {
                    Class::Other
                }
            })
            .collect();
        let mut vert_faces = vec![Vec::new(); verts.len()];
        for (f, t) in ids.iter().enumerate() {
            for &v in t {
                vert_faces[v].push(f);
            }
        }
        let n_faces = ids.len();
        let n_verts = verts.len();
        Self {
            verts,
            faces: ids,
            class,
            alive: vec![true; n_faces],
            vert_faces,
            points: vec![Vec::new(); n_faces],
            stamp: vec![0; n_verts],
        }
    }

    /// The class shared by all faces around `u`, if `u` may move.
    fn movable(&self, u: usize) -> Option<Class> {
        let mut faces = self.vert_faces[u].iter().map(|&f| self.class[f]);
        let first = faces.next()?;
        (first != Class::Other && faces.all(|c| c == first)).then_some(first)
    }

    /// Vertices sharing a face with `u` (fans are small, so a linear dedup is fastest).
    fn neighbors(&self, u: usize) -> Vec<usize> {
        let mut n = Vec::with_capacity(12);
        for &f in &self.vert_faces[u] {
            for w in self.faces[f] {
                if w != u && !n.contains(&w) {
                    n.push(w);
                }
            }
        }
        n
    }

    /// Check collapsing `u` into `v`. Returns the worst vertical error and, for each
    /// carried point, the face it lands in. With `quick`, only `u` itself is checked
    /// (no points are placed): a lower bound on the full error, cheap enough to rank
    /// every candidate.
    fn evaluate(&self, u: usize, v: usize, tolerance: f64, quick: bool) -> Option<(f64, Placed)> {
        let class = self.movable(u)?;
        let mut shared = [usize::MAX; 2];
        let mut n_shared = 0;
        for &f in &self.vert_faces[u] {
            if self.faces[f].contains(&v) {
                if n_shared == 2 {
                    return None;
                }
                shared[n_shared] = f;
                n_shared += 1;
            }
        }
        if n_shared != 2 {
            return None;
        }
        // Link condition: u and v may share only the two vertices opposite their edge.
        // Only needed for the collapse actually made, so the quick ranking skips it.
        if !quick {
            let opposite: Vec<usize> = shared
                .iter()
                .flat_map(|&f| self.faces[f])
                .filter(|&w| w != u && w != v)
                .collect();
            let nv = self.neighbors(v);
            let common = self
                .neighbors(u)
                .into_iter()
                .filter(|w| *w != v && nv.contains(w))
                .count();
            if common != 2 || opposite.len() != 2 || opposite[0] == opposite[1] {
                return None;
            }
        }
        let mut new_faces = Vec::with_capacity(self.vert_faces[u].len());
        for &f in &self.vert_faces[u] {
            if shared.contains(&f) {
                continue;
            }
            let old = self.faces[f].map(|i| self.verts[i]);
            let new = self.faces[f].map(|i| self.verts[if i == u { v } else { i }]);
            let (a0, a1) = (
                xy_area(old[0], old[1], old[2]),
                xy_area(new[0], new[1], new[2]),
            );
            if a0.signum() != a1.signum() || a1.abs() < 1e-9 * a0.abs().max(1e-12) {
                return None;
            }
            new_faces.push((f, new));
        }
        if class == Class::Bottom {
            return Some((0., Vec::new()));
        }
        if quick {
            let p = self.verts[u];
            let z = new_faces.iter().find_map(|(_, t)| height_in(*t, p))?;
            let e = (p[2] - z).abs();
            return (e <= tolerance).then(|| (e, Vec::new()));
        }
        let carried = self.vert_faces[u]
            .iter()
            .flat_map(|&f| self.points[f].iter().copied())
            .chain(std::iter::once(self.verts[u]));
        let mut worst = 0f64;
        let mut placed = Vec::new();
        for p in carried {
            let (f, z) = new_faces
                .iter()
                .find_map(|(f, t)| height_in(*t, p).map(|z| (*f, z)))?;
            worst = worst.max((p[2] - z).abs());
            if worst > tolerance {
                return None;
            }
            placed.push((f, p));
        }
        Some((worst, placed))
    }

    /// The neighbor to collapse `u` into with the smallest error, by the quick or the
    /// full check.
    fn best(&self, u: usize, tolerance: f64, quick: bool) -> Option<(f64, usize)> {
        self.movable(u)?;
        self.neighbors(u)
            .into_iter()
            .filter_map(|v| self.evaluate(u, v, tolerance, quick).map(|(e, _)| (e, v)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    fn collapse(&mut self, u: usize, v: usize, placed: Placed) {
        let faces_u = std::mem::take(&mut self.vert_faces[u]);
        for f in faces_u {
            if self.faces[f].contains(&v) {
                self.alive[f] = false;
                for w in self.faces[f] {
                    if w != u {
                        self.vert_faces[w].retain(|&g| g != f);
                    }
                }
            } else {
                for i in self.faces[f].iter_mut() {
                    if *i == u {
                        *i = v;
                    }
                }
                self.vert_faces[v].push(f);
            }
            self.points[f].clear();
        }
        for (f, p) in placed {
            self.points[f].push(p);
        }
    }
}

/// Simplify a closed tile mesh. `tolerance_mm` bounds how far any original surface sample
/// may be from the result, vertically; `protect_below_z` is the base thickness, below which
/// nothing on the surface is modified. A tolerance of 0 removes only exactly redundant
/// vertices (flat areas).
pub fn simplify(
    faces: &[Tri],
    tolerance_mm: f64,
    protect_below_z: f64,
) -> Result<Vec<Tri>, String> {
    if !tolerance_mm.is_finite() || tolerance_mm < 0. || !protect_below_z.is_finite() {
        return Err("Simplification tolerance must be zero or positive".into());
    }
    let mut m = Mesh::build(faces, protect_below_z);
    let mut heap = BinaryHeap::new();
    let key = |e: f64| Reverse((e * 1e9) as u64);
    // Candidates are ranked by the quick check; the full check runs once, when one
    // reaches the front. If it fails, the vertex is re-ranked by full checks, which then
    // hold when it comes up again unless its neighborhood changes (which restamps it).
    for u in 0..m.verts.len() {
        if let Some((e, v)) = m.best(u, tolerance_mm, true) {
            heap.push((key(e), u, v, 0u32, true));
        }
    }
    while let Some((_, u, v, stamp, quick)) = heap.pop() {
        if stamp != m.stamp[u] || m.vert_faces[u].is_empty() {
            continue;
        }
        let Some((_, placed)) = m.evaluate(u, v, tolerance_mm, false) else {
            m.stamp[u] += 1;
            // After a failed quick candidate, rank this vertex by full checks; after a
            // failed full one (its neighborhood moved), start over with the quick check.
            if let Some((e, v)) = m.best(u, tolerance_mm, !quick) {
                heap.push((key(e), u, v, m.stamp[u], !quick));
            }
            continue;
        };
        m.collapse(u, v, placed);
        let mut affected = m.neighbors(v);
        affected.push(v);
        for w in affected {
            m.stamp[w] += 1;
            if let Some((e, t)) = m.best(w, tolerance_mm, true) {
                heap.push((key(e), w, t, m.stamp[w], true));
            }
        }
    }
    Ok(m.faces
        .iter()
        .zip(&m.alive)
        .filter(|(_, a)| **a)
        .map(|(t, _)| t.map(|i| m.verts[i]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mesh, mesh_audit::audit, tiles, Dem, TerrainSettings, Tile};

    fn settings() -> TerrainSettings {
        TerrainSettings {
            width_mm: 100.,
            height_mm: 80.,
            base_mm: 3.,
            vertical_exaggeration: 1.,
            relief_mm: 10.,
        }
    }
    fn bumpy(w: usize, h: usize) -> Dem {
        let v = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64 / w as f64, (i / w) as f64 / h as f64);
                (x * 7.).sin() * (y * 5.).cos() * 40. + ((x - 0.3).powi(2) + y * y) * 90.
            })
            .collect();
        Dem::new(w, h, v).unwrap()
    }
    /// Largest vertical distance from each original top-surface vertex to the simplified
    /// surface, found by brute force over all simplified surface faces.
    fn max_vertical_error(original: &[Tri], simplified: &[Tri], base: f64) -> f64 {
        let top: Vec<&Tri> = simplified
            .iter()
            .filter(|t| xy_area(t[0], t[1], t[2]) > 0. && t.iter().all(|p| p[2] >= base))
            .collect();
        let mut worst = 0f64;
        for p in original.iter().flatten().filter(|p| p[2] >= base) {
            // Wall corners on the tile edge are kept, so only interior samples are checked.
            let z = top
                .iter()
                .filter_map(|t| height_in(**t, *p))
                .fold(f64::NAN, |a, b| if a.is_nan() { b } else { a.max(b) });
            if !z.is_nan() {
                worst = worst.max((p[2] - z).abs());
            }
        }
        worst
    }

    #[test]
    fn flat_tile_collapses_to_its_outline() {
        let d = Dem::new(21, 21, vec![5.; 441]).unwrap();
        let t = Tile {
            row: 0,
            col: 0,
            x0: 0,
            x1: 20,
            y0: 0,
            y1: 20,
        };
        let full = mesh(&d, settings(), t).unwrap();
        let s = simplify(&full, 0., 3.).unwrap();
        assert!(audit(&s).is_closed_oriented_manifold());
        // 1600 grid faces on top and bottom collapse to fans over the 80 edge samples.
        assert!(s.len() < full.len() / 4, "{} of {}", s.len(), full.len());
    }

    #[test]
    fn error_stays_within_tolerance_and_mesh_stays_closed() {
        let d = bumpy(61, 49);
        let t = Tile {
            row: 0,
            col: 0,
            x0: 0,
            x1: 60,
            y0: 0,
            y1: 48,
        };
        let full = mesh(&d, settings(), t).unwrap();
        for tol in [0.05, 0.2] {
            let s = simplify(&full, tol, 3.).unwrap();
            assert!(audit(&s).is_closed_oriented_manifold(), "tol {tol}");
            assert!(
                s.len() < full.len() / 2,
                "tol {tol}: {} of {}",
                s.len(),
                full.len()
            );
            let err = max_vertical_error(&full, &s, 3.);
            assert!(err <= tol + 1e-9, "tol {tol}: error {err}");
        }
    }

    #[test]
    fn adjacent_tiles_keep_matching_edges() {
        let d = bumpy(41, 21);
        let ts = tiles(&d, settings(), 60., 100., 2.).unwrap();
        assert_eq!(ts.len(), 2);
        let edge = |faces: &[Tri], x: f64| {
            let mut pts: Vec<[u64; 3]> = faces
                .iter()
                .flatten()
                .filter(|p| (p[0] - x).abs() < 1e-9)
                .map(|p| [p[0], p[1], p[2]].map(f64::to_bits))
                .collect();
            pts.sort_unstable();
            pts.dedup();
            pts
        };
        let a = simplify(&mesh(&d, settings(), ts[0]).unwrap(), 0.1, 3.).unwrap();
        let b = simplify(&mesh(&d, settings(), ts[1]).unwrap(), 0.1, 3.).unwrap();
        // Tile meshes are local to each tile; tile 0's east edge is tile 1's x = 0.
        let width0 = (ts[0].x1 - ts[0].x0) as f64 * 100. / 40.;
        let shifted: Vec<Tri> = b
            .iter()
            .map(|t| t.map(|p| [p[0] + width0, p[1], p[2]]))
            .collect();
        assert_eq!(edge(&a, width0), edge(&shifted, width0));
    }

    #[test]
    fn rejects_bad_tolerance() {
        assert!(simplify(&[], -1., 0.).is_err());
    }
}
