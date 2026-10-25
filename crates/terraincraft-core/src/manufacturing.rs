//! Conservative manufacturing preflight: topology, dimensions, and signed volume.
//! Does not claim to detect self intersections or minimum local wall thickness.
use crate::mesh_audit::{audit, MeshAudit};

#[derive(Debug, Clone)]
pub struct ManufacturingReport {
    pub topology: MeshAudit,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub signed_volume_mm3: f64,
    pub surface_area_mm2: f64,
    pub finite_geometry: bool,
}
impl ManufacturingReport {
    pub fn is_ready_for_review(&self) -> bool {
        self.finite_geometry
            && self.topology.is_closed_oriented_manifold()
            && self.signed_volume_mm3 > 1e-8
            && self.surface_area_mm2 > 1e-8
            && (0..3).all(|i| self.max[i] > self.min[i])
    }
    pub fn csv_row(&self, filename: &str) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            filename,
            self.topology.triangles,
            self.min[0],
            self.min[1],
            self.min[2],
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
            self.signed_volume_mm3,
            self.surface_area_mm2,
            self.finite_geometry,
            self.is_ready_for_review()
        )
    }
}
pub const CSV_HEADER: &str = "filename,triangles,min_x_mm,min_y_mm,min_z_mm,size_x_mm,size_y_mm,size_z_mm,signed_volume_mm3,surface_area_mm2,finite_geometry,ready_for_review\n";
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// Signed volume is meaningful only for consistently wound closed surfaces.
pub fn inspect(faces: &[[[f64; 3]; 3]]) -> ManufacturingReport {
    let topology = audit(faces);
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut finite_geometry = true;
    let mut volume = 0.;
    let mut area = 0.;
    for tri in faces {
        for p in tri {
            for i in 0..3 {
                if !p[i].is_finite() {
                    finite_geometry = false;
                } else {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
        }
        if tri.iter().flatten().all(|v| v.is_finite()) {
            let ab = sub(tri[1], tri[0]);
            let ac = sub(tri[2], tri[0]);
            let n = cross(ab, ac);
            area += dot(n, n).sqrt() / 2.;
            volume += dot(tri[0], cross(tri[1], tri[2])) / 6.;
        }
    }
    ManufacturingReport {
        topology,
        min,
        max,
        signed_volume_mm3: volume,
        surface_area_mm2: area,
        finite_geometry,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tetra() -> Vec<[[f64; 3]; 3]> {
        let a = [0., 0., 0.];
        let b = [1., 0., 0.];
        let c = [0., 1., 0.];
        let d = [0., 0., 1.];
        vec![[a, c, b], [a, b, d], [b, c, d], [c, a, d]]
    }
    #[test]
    fn valid_tetrahedron() {
        let r = inspect(&tetra());
        assert!(r.is_ready_for_review());
        assert!((r.signed_volume_mm3 - 1. / 6.).abs() < 1e-10);
    }
    #[test]
    fn inward_facing_shell_is_rejected() {
        let mut t = tetra();
        for face in &mut t {
            face.swap(1, 2);
        }
        let r = inspect(&t);
        assert!(r.topology.is_closed_oriented_manifold());
        assert!(!r.is_ready_for_review());
        assert!(r.signed_volume_mm3 < 0.);
    }
    #[test]
    fn open_mesh_is_rejected() {
        let mut t = tetra();
        t.pop();
        assert!(!inspect(&t).is_ready_for_review());
    }
}
