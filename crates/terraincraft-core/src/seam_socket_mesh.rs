//! Blind seam sockets reconstructed across arbitrary DEM wall segment boundaries.
//! The entire affected sidewall is triangulated as a polygon with circular holes;
//! the original DEM top-edge vertices are preserved exactly.
use crate::seam_joints::SeamHole;
use crate::{mesh, Dem, TerrainSettings, Tile};
type Tri = [[f64; 3]; 3];
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn orient(mut t: Tri, normal: [f64; 3]) -> Tri {
    let a = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let b = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    let n = cross(a, b);
    if n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2] < 0. {
        t.swap(1, 2)
    }
    t
}
fn position(side: &str, u: f64, z: f64, depth: f64, w: f64, h: f64) -> [f64; 3] {
    match side {
        "west" => [depth, u, z],
        "east" => [w - depth, u, z],
        "north" => [u, depth, z],
        _ => [u, h - depth, z],
    }
}
fn outward(side: &str) -> [f64; 3] {
    match side {
        "west" => [-1., 0., 0.],
        "east" => [1., 0., 0.],
        "north" => [0., -1., 0.],
        _ => [0., 1., 0.],
    }
}
fn emit(f: &mut Vec<Tri>, a: [f64; 3], b: [f64; 3], c: [f64; 3], normal: [f64; 3]) {
    f.push(orient([a, b, c], normal));
}

pub fn mesh_with_seam_sockets(
    d: &Dem,
    s: TerrainSettings,
    t: Tile,
    holes: &[SeamHole],
    segments: usize,
) -> Result<Vec<Tri>, String> {
    s.validate()?;
    if !(16..=128).contains(&segments) || !segments.is_multiple_of(4) {
        return Err("Socket segments must be a multiple of four in [16,128]".into());
    }
    let sx = s.width_mm / (d.width - 1) as f64;
    let sy = s.height_mm / (d.height - 1) as f64;
    let w = (t.x1 - t.x0) as f64 * sx;
    let h = (t.y1 - t.y0) as f64 * sy;
    let mut faces = mesh(d, s, t)?;
    if holes.is_empty() {
        return Ok(faces);
    }
    let xcells = t.x1 - t.x0;
    let ycells = t.y1 - t.y0;
    let wall_start = 4 * xcells * ycells;
    let (min, max) = d.min_max();
    let range = (max - min).max(1e-12);
    let mut patches: Vec<(usize, Vec<Tri>)> = Vec::new();
    for (side, count, step, side_index) in [
        ("north", xcells, sx, 0usize),
        ("south", xcells, sx, 1),
        ("west", ycells, sy, 2),
        ("east", ycells, sy, 3),
    ] {
        let selected: Vec<&SeamHole> = holes.iter().filter(|hole| hole.side == side).collect();
        if selected.is_empty() {
            continue;
        }
        let wall_len = count as f64 * step;
        let depth_limit = if side == "north" || side == "south" {
            h / 2.
        } else {
            w / 2.
        };
        for hole in &selected {
            if hole.tile_row != t.row || hole.tile_col != t.col {
                return Err("Socket assigned to another tile".into());
            }
            if ![
                hole.along_mm,
                hole.height_mm,
                hole.depth_mm,
                hole.diameter_mm,
            ]
            .iter()
            .all(|v| v.is_finite())
                || hole.diameter_mm <= 0.
                || hole.depth_mm <= 0.
            {
                return Err("Invalid socket geometry".into());
            }
            let r = hole.diameter_mm / 2.;
            if hole.along_mm - r <= 0.
                || hole.along_mm + r >= wall_len
                || hole.height_mm - r <= 0.
                || hole.height_mm + r >= s.base_mm
            {
                return Err("Socket outside sidewall base".into());
            }
            if hole.depth_mm >= depth_limit {
                return Err("Socket depth exceeds half tile width".into());
            }
        }
        for i in 0..selected.len() {
            for j in i + 1..selected.len() {
                let a = selected[i];
                let b = selected[j];
                let du = a.along_mm - b.along_mm;
                let dz = a.height_mm - b.height_mm;
                let clearance = (a.diameter_mm + b.diameter_mm) / 2. + 0.01;
                if du * du + dz * dz <= clearance * clearance {
                    return Err("Overlapping socket openings".into());
                }
            }
        }
        // Exterior perimeter: all original bottom vertices, then all original top
        // vertices in reverse order. This keeps edge connectivity with the original
        // bottom and top surfaces even when a hole crosses many DEM wall cells.
        let mut coords: Vec<f64> = Vec::new();
        macro_rules! push_coord {
            ($u:expr,$z:expr) => {{
                coords.push($u);
                coords.push($z);
            }};
        }
        for i in 0..=count {
            push_coord!(i as f64 * step, 0.);
        }
        for i in (0..=count).rev() {
            let v = match side {
                "north" => d.at(t.x0 + i, t.y0),
                "south" => d.at(t.x0 + i, t.y1),
                "west" => d.at(t.x0, t.y0 + i),
                _ => d.at(t.x1, t.y0 + i),
            };
            push_coord!(
                i as f64 * step,
                s.base_mm + (v - min) / range * s.relief_mm * s.vertical_exaggeration
            );
        }
        let mut hole_indices = Vec::new();
        for hole in &selected {
            hole_indices.push(coords.len() / 2);
            let r = hole.diameter_mm / 2.;
            for k in 0..segments {
                let angle = std::f64::consts::TAU * k as f64 / segments as f64;
                push_coord!(
                    hole.along_mm + r * angle.cos(),
                    hole.height_mm + r * angle.sin()
                );
            }
        }
        let indices = earcutr::earcut(&coords, &hole_indices, 2)
            .map_err(|e| format!("Sidewall triangulation failed: {e}"))?;
        let normal = outward(side);
        let mut patch = Vec::new();
        for tri in indices.as_chunks::<3>().0 {
            let point = |i: usize| position(side, coords[2 * i], coords[2 * i + 1], 0., w, h);
            emit(
                &mut patch,
                point(tri[0]),
                point(tri[1]),
                point(tri[2]),
                normal,
            );
        }
        for hole in selected {
            let r = hole.diameter_mm / 2.;
            let ring = |k: usize, depth: f64| {
                let angle = std::f64::consts::TAU * k as f64 / segments as f64;
                position(
                    side,
                    hole.along_mm + r * angle.cos(),
                    hole.height_mm + r * angle.sin(),
                    depth,
                    w,
                    h,
                )
            };
            for k in 0..segments {
                let a = ring(k, 0.);
                let b = ring((k + 1) % segments, 0.);
                let c = ring(k, hole.depth_mm);
                let e = ring((k + 1) % segments, hole.depth_mm);
                // The cavity wall normal points towards the cylinder axis.
                let angle = std::f64::consts::TAU * (k as f64 + 0.5) / segments as f64;
                let radial_u = -angle.cos();
                let radial_z = -angle.sin();
                let radial = if side == "north" || side == "south" {
                    [radial_u, 0., radial_z]
                } else {
                    [0., radial_u, radial_z]
                };
                emit(&mut patch, a, b, e, radial);
                emit(&mut patch, a, e, c, radial);
                let center = position(side, hole.along_mm, hole.height_mm, hole.depth_mm, w, h);
                emit(&mut patch, center, c, e, normal);
            }
        }
        patches.push((side_index, patch));
    }
    // Reject unknown wall names, including those otherwise omitted by filtering.
    if holes
        .iter()
        .any(|h| !["north", "south", "west", "east"].contains(&h.side))
    {
        return Err("Unknown tile wall".into());
    }
    // The original mesh interleaves north/south faces for each X cell and
    // west/east faces for each Y cell. Replacing a contiguous slice corrupts
    // neighboring walls. Rebuild the wall section side by side instead.
    let mut rebuilt = faces[..wall_start].to_vec();
    for side_index in 0..4 {
        if let Some((_, patch)) = patches.iter().find(|(idx, _)| *idx == side_index) {
            rebuilt.extend_from_slice(patch);
        } else {
            let count = if side_index < 2 { xcells } else { ycells };
            let base = if side_index < 2 {
                wall_start
            } else {
                wall_start + 4 * xcells
            };
            let local = side_index % 2;
            for i in 0..count {
                let first = base + 4 * i + 2 * local;
                rebuilt.extend_from_slice(&faces[first..first + 2]);
            }
        }
    }
    faces = rebuilt;
    // Validate reconstructed topology at the geometry boundary, before STL export.
    // A bad socket mesh is never silently substituted with a plain terrain tile.
    let audit = crate::mesh_audit::audit(&faces);
    if !audit.is_closed_oriented_manifold() {
        return Err(format!(
            "Reconstructed seam socket mesh is not a closed oriented manifold: {audit:?}"
        ));
    }
    Ok(faces)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(n: usize) -> (Dem, TerrainSettings, Tile) {
        (
            Dem::new(n, n, vec![0.; n * n]).unwrap(),
            TerrainSettings {
                width_mm: 100.,
                height_mm: 100.,
                base_mm: 10.,
                relief_mm: 5.,
                vertical_exaggeration: 1.,
            },
            Tile {
                row: 0,
                col: 0,
                x0: 0,
                x1: n - 1,
                y0: 0,
                y1: n - 1,
            },
        )
    }
    fn hole(u: f64) -> SeamHole {
        SeamHole {
            seam_id: "s".into(),
            tile_row: 0,
            tile_col: 0,
            side: "east",
            along_mm: u,
            height_mm: 5.,
            depth_mm: 6.,
            diameter_mm: 3.3,
        }
    }
    #[test]
    fn crosses_multiple_dem_segments() {
        let (d, s, t) = sample(101);
        let m = mesh_with_seam_sockets(&d, s, t, &[hole(50.)], 32).unwrap();
        assert!(m.len() > mesh(&d, s, t).unwrap().len());
        let audit = crate::mesh_audit::audit(&m);
        assert!(audit.is_closed_oriented_manifold(), "{audit:?}");
    }
    #[test]
    fn supports_two_separated_sockets() {
        let (d, s, t) = sample(101);
        let m = mesh_with_seam_sockets(&d, s, t, &[hole(25.), hole(75.)], 32).unwrap();
        assert!(crate::mesh_audit::audit(&m).is_closed_oriented_manifold());
    }
    #[test]
    fn sockets_on_all_four_walls_are_manifold() {
        let (d, s, t) = sample(41);
        let holes = ["north", "south", "west", "east"]
            .iter()
            .map(|side| SeamHole {
                seam_id: "s".into(),
                tile_row: 0,
                tile_col: 0,
                side,
                along_mm: 50.,
                height_mm: 5.,
                depth_mm: 6.,
                diameter_mm: 3.3,
            })
            .collect::<Vec<_>>();
        let m = mesh_with_seam_sockets(&d, s, t, &holes, 32).unwrap();
        let a = crate::mesh_audit::audit(&m);
        assert!(a.is_closed_oriented_manifold(), "{a:?}");
    }
    #[test]
    fn rejects_overlap() {
        let (d, s, t) = sample(101);
        assert!(mesh_with_seam_sockets(&d, s, t, &[hole(25.), hole(26.)], 32).is_err());
    }
}
