//! Triangle-soup topology audit before STL export. This is not a Boolean repair engine.
use std::collections::HashMap;

type Vertex = [u64; 3];
type Edge = (Vertex, Vertex);
fn key(v: [f64; 3]) -> Vertex {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}
fn edge(a: Vertex, b: Vertex) -> (Edge, bool) {
    if a <= b {
        ((a, b), true)
    } else {
        ((b, a), false)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeshAudit {
    pub triangles: usize,
    pub degenerate_triangles: usize,
    pub boundary_edges: usize,
    pub nonmanifold_edges: usize,
    pub inconsistent_winding_edges: usize,
}
impl MeshAudit {
    pub fn is_closed_oriented_manifold(&self) -> bool {
        self.triangles > 0
            && self.degenerate_triangles == 0
            && self.boundary_edges == 0
            && self.nonmanifold_edges == 0
            && self.inconsistent_winding_edges == 0
    }
    pub fn csv_row(&self, name: &str) -> String {
        format!(
            "{},{},{},{},{},{},{}\n",
            name,
            self.triangles,
            self.degenerate_triangles,
            self.boundary_edges,
            self.nonmanifold_edges,
            self.inconsistent_winding_edges,
            self.is_closed_oriented_manifold()
        )
    }
}
pub const CSV_HEADER:&str="filename,triangles,degenerate_triangles,boundary_edges,nonmanifold_edges,inconsistent_winding_edges,closed_oriented_manifold\n";
/// Audit exact shared vertices and edge orientation; does not detect self-intersections.
/// Vertices must have identical floating-point coordinates to be treated as shared.
pub fn audit(faces: &[[[f64; 3]; 3]]) -> MeshAudit {
    let mut result = MeshAudit {
        triangles: faces.len(),
        ..MeshAudit::default()
    };
    let mut edges: HashMap<Edge, (usize, usize)> = HashMap::new();
    for tri in faces {
        if tri.iter().flatten().any(|v| !v.is_finite()) {
            result.degenerate_triangles += 1;
            continue;
        }
        let a = [
            tri[1][0] - tri[0][0],
            tri[1][1] - tri[0][1],
            tri[1][2] - tri[0][2],
        ];
        let b = [
            tri[2][0] - tri[0][0],
            tri[2][1] - tri[0][1],
            tri[2][2] - tri[0][2],
        ];
        let cross = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        if cross.iter().map(|v| v * v).sum::<f64>() <= 1e-20 {
            result.degenerate_triangles += 1;
        }
        let v = tri.map(key);
        for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
            let (k, forward) = edge(a, b);
            let entry = edges.entry(k).or_default();
            entry.0 += 1;
            if forward {
                entry.1 += 1;
            }
        }
    }
    for (_, (count, forward)) in edges {
        if count == 1 {
            result.boundary_edges += 1;
        } else if count != 2 {
            result.nonmanifold_edges += 1;
        } else if forward != 1 {
            result.inconsistent_winding_edges += 1;
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tetrahedron_is_closed() {
        let a = [0., 0., 0.];
        let b = [1., 0., 0.];
        let c = [0., 1., 0.];
        let d = [0., 0., 1.];
        assert!(audit(&[[a, c, b], [a, b, d], [b, c, d], [c, a, d]]).is_closed_oriented_manifold());
    }
    #[test]
    fn missing_face_has_boundary() {
        let a = [0., 0., 0.];
        let b = [1., 0., 0.];
        let c = [0., 1., 0.];
        let d = [0., 0., 1.];
        assert_eq!(audit(&[[a, c, b], [a, b, d], [b, c, d]]).boundary_edges, 3);
    }
    #[test]
    fn reversed_face_detected() {
        let a = [0., 0., 0.];
        let b = [1., 0., 0.];
        let c = [0., 1., 0.];
        let d = [0., 0., 1.];
        assert!(
            audit(&[[a, b, c], [a, b, d], [b, c, d], [c, a, d]]).inconsistent_winding_edges > 0
        );
    }
}
