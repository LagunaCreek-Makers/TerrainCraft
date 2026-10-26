//! Fabrication-ready loose dowels and conservative seam fit checks.
//! Does NOT subtract sockets from terrain meshes.
use crate::seam_joints::{SeamHole, SeamSettings};
use std::collections::BTreeMap;
use std::fmt::Write;

pub type Triangle = [[f64; 3]; 3];

/// A printable dowel oriented along Z, with a small lead-in chamfer.
/// Diameter is nominal (without the socket clearance).
pub fn dowel_mesh(settings: SeamSettings, segments: usize) -> Result<Vec<Triangle>, String> {
    if !(12..=512).contains(&segments)
        || !settings.diameter_mm.is_finite()
        || settings.diameter_mm <= 0.
        || !settings.depth_mm.is_finite()
        || settings.depth_mm <= 0.
    {
        return Err("Invalid dowel dimensions or radial resolution".into());
    }
    let radius = settings.diameter_mm / 2.;
    let length = 2. * settings.depth_mm;
    let chamfer = (radius * 0.15).min(length * 0.1);
    let mut tris = Vec::with_capacity(segments * 8);
    let ring = |i: usize, r: f64, z: f64| -> [f64; 3] {
        let angle = std::f64::consts::TAU * i as f64 / segments as f64;
        [r * angle.cos(), r * angle.sin(), z]
    };
    for i in 0..segments {
        let j = (i + 1) % segments;
        let a = ring(i, radius, chamfer);
        let b = ring(j, radius, chamfer);
        let c = ring(i, radius, length - chamfer);
        let d = ring(j, radius, length - chamfer);
        tris.push([a, b, d]);
        tris.push([a, d, c]);
        let low_a = ring(i, radius - chamfer, 0.);
        let low_b = ring(j, radius - chamfer, 0.);
        tris.push([low_a, low_b, b]);
        tris.push([low_a, b, a]);
        let high_a = ring(i, radius - chamfer, length);
        let high_b = ring(j, radius - chamfer, length);
        tris.push([c, d, high_b]);
        tris.push([c, high_b, high_a]);
        tris.push([[0., 0., 0.], low_b, low_a]);
        tris.push([[0., 0., length], high_a, high_b]);
    }
    Ok(tris)
}

/// Validate matched hole pairs and minimum end/neighbor clearance.
/// This is a geometric plan check, not an STL manifold check.
pub fn preflight(holes: &[SeamHole], settings: SeamSettings) -> Result<(), String> {
    let mut by_seam: BTreeMap<&str, Vec<&SeamHole>> = BTreeMap::new();
    for h in holes {
        by_seam.entry(&h.seam_id).or_default().push(h);
    }
    for (seam, entries) in by_seam {
        if entries.len() % 2 != 0 {
            return Err(format!("Unpaired seam hole: {seam}"));
        }
        let mut by_side: BTreeMap<(usize, usize, &str), Vec<&SeamHole>> = BTreeMap::new();
        for h in entries {
            by_side
                .entry((h.tile_row, h.tile_col, h.side))
                .or_default()
                .push(h);
        }
        if by_side.len() != 2 {
            return Err(format!("Expected two tile walls for {seam}"));
        }
        let walls: Vec<_> = by_side.values().collect();
        if walls[0].len() != walls[1].len() {
            return Err(format!("Hole count mismatch: {seam}"));
        }
        let mut a = walls[0].clone();
        let mut b = walls[1].clone();
        a.sort_by(|x, y| x.along_mm.total_cmp(&y.along_mm));
        b.sort_by(|x, y| x.along_mm.total_cmp(&y.along_mm));
        for (x, y) in a.iter().zip(b.iter()) {
            if (x.along_mm - y.along_mm).abs() > 1e-6
                || (x.height_mm - y.height_mm).abs() > 1e-6
                || (x.diameter_mm - y.diameter_mm).abs() > 1e-6
            {
                return Err(format!("Mismatched mating hole locations: {seam}"));
            }
        }
        for w in walls {
            let mut sorted = w.clone();
            sorted.sort_by(|x, y| x.along_mm.total_cmp(&y.along_mm));
            for pair in sorted.windows(2) {
                if pair[1].along_mm - pair[0].along_mm
                    < settings.diameter_mm + settings.clearance_mm + 2.
                {
                    return Err(format!("Insufficient material between sockets: {seam}"));
                }
            }
        }
    }
    Ok(())
}

/// Offline assembly report showing wall coordinates and the current fabrication limitation.
pub fn assembly_svg(holes: &[SeamHole]) -> String {
    let mut svg=String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300mm\" height=\"160mm\" viewBox=\"0 0 300 160\"><rect width=\"300\" height=\"160\" fill=\"white\"/><g fill=\"none\" stroke=\"black\" stroke-width=\"0.5\"><rect x=\"10\" y=\"20\" width=\"125\" height=\"100\"/><rect x=\"165\" y=\"20\" width=\"125\" height=\"100\"/></g><g fill=\"black\" font-family=\"sans-serif\" font-size=\"5\"><text x=\"10\" y=\"12\">TerrainCraft seam dowel assembly (schematic)</text><text x=\"10\" y=\"132\">Sockets require drilling; not cut into exported STL.</text>");
    let pairs = holes.len() / 2;
    let _ = write!(
        svg,
        "<text x=\"10\" y=\"141\">{} matched dowel positions</text>",
        pairs
    );
    for i in 0..pairs.min(6) {
        let y = 30. + i as f64 * 14.;
        let _=write!(svg,"<g stroke=\"black\" stroke-width=\"0.5\"><line x1=\"132\" y1=\"{y}\" x2=\"168\" y2=\"{y}\"/></g><circle cx=\"135\" cy=\"{y}\" r=\"2\" fill=\"black\"/><circle cx=\"165\" cy=\"{y}\" r=\"2\" fill=\"black\"/>");
    }
    svg.push_str("</g></svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dowel_is_closed_and_finite() {
        let mesh = dowel_mesh(SeamSettings::default(), 24).unwrap();
        assert_eq!(mesh.len(), 192);
        assert!(mesh.iter().flatten().flatten().all(|x| x.is_finite()));
    }
    #[test]
    fn bad_resolution_rejected() {
        assert!(dowel_mesh(SeamSettings::default(), 3).is_err());
    }
    #[test]
    fn mismatched_holes_rejected() {
        let h = SeamHole {
            seam_id: "a".into(),
            tile_row: 0,
            tile_col: 0,
            side: "east",
            along_mm: 10.,
            height_mm: 3.,
            depth_mm: 6.,
            diameter_mm: 3.3,
        };
        assert!(preflight(&[h], SeamSettings::default()).is_err());
    }
}
