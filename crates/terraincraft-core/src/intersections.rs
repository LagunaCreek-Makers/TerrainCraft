//! Conservative triangle intersection diagnostic, including coplanar face overlaps.
//! A sweep-and-prune broad phase avoids quadratic comparisons for spatially separated faces.
//! Exact shared vertices are excluded; tangential contact and shared-vertex overlaps are not detected.
#[derive(Debug, Clone, Copy)]
pub struct IntersectionReport {
    pub tested_pairs: usize,
    pub crossings: usize,
    pub coplanar_overlaps: usize,
    pub skipped_adjacent_pairs: usize,
    pub broadphase_candidates: usize,
    pub complete: bool,
}
type Tri = [[f64; 3]; 3];
type Aabb = ([f64; 3], [f64; 3]);
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn bounds(t: &Tri) -> Aabb {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in t {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}
fn overlap(a: &Aabb, b: &Aabb) -> bool {
    (0..3).all(|k| a.0[k] <= b.1[k] && b.0[k] <= a.1[k])
}
fn shared(a: &Tri, b: &Tri) -> bool {
    a.iter().any(|p| b.iter().any(|q| p == q))
}
fn segment_hits_triangle(p: [f64; 3], q: [f64; 3], t: &Tri) -> bool {
    let dir = sub(q, p);
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let h = cross(dir, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv = 1. / det;
    let s = sub(p, t[0]);
    let u = dot(s, h) * inv;
    if u <= 1e-9 || u >= 1. - 1e-9 {
        return false;
    }
    let v = dot(dir, cross(s, e1)) * inv;
    if v <= 1e-9 || u + v >= 1. - 1e-9 {
        return false;
    }
    let dist = dot(e2, cross(s, e1)) * inv;
    dist > 1e-9 && dist < 1. - 1e-9
}
fn crosses(a: &Tri, b: &Tri) -> bool {
    (0..3).any(|i| {
        segment_hits_triangle(a[i], a[(i + 1) % 3], b)
            || segment_hits_triangle(b[i], b[(i + 1) % 3], a)
    })
}
// Detect positive-area overlap between triangles in the same plane. Mere edge
// or point contact does not count. Project onto the dominant plane axis.
fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn proper_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let x = orient(a, b, c);
    let y = orient(a, b, d);
    let z = orient(c, d, a);
    let w = orient(c, d, b);
    (x > 1e-10 && y < -1e-10 || x < -1e-10 && y > 1e-10)
        && (z > 1e-10 && w < -1e-10 || z < -1e-10 && w > 1e-10)
}
fn inside(p: [f64; 2], t: &[[f64; 2]; 3]) -> bool {
    let a = orient(t[0], t[1], p);
    let b = orient(t[1], t[2], p);
    let c = orient(t[2], t[0], p);
    (a > 1e-10 && b > 1e-10 && c > 1e-10) || (a < -1e-10 && b < -1e-10 && c < -1e-10)
}
fn coplanar_overlap(a: &Tri, b: &Tri) -> bool {
    let n = cross(sub(a[1], a[0]), sub(a[2], a[0]));
    let norm = dot(n, n).sqrt();
    if norm <= 1e-12 {
        return false;
    }
    let m = cross(sub(b[1], b[0]), sub(b[2], b[0]));
    let mnorm = dot(m, m).sqrt();
    if mnorm <= 1e-12 || dot(cross(n, m), cross(n, m)).sqrt() > 1e-9 * norm * mnorm {
        return false;
    }
    if b.iter().any(|p| dot(n, sub(*p, a[0])).abs() > 1e-9 * norm) {
        return false;
    }
    let drop = (0..3)
        .max_by(|&i, &j| n[i].abs().total_cmp(&n[j].abs()))
        .unwrap_or(2);
    let project = |p: [f64; 3]| -> [f64; 2] {
        match drop {
            0 => [p[1], p[2]],
            1 => [p[0], p[2]],
            _ => [p[0], p[1]],
        }
    };
    let aa = [project(a[0]), project(a[1]), project(a[2])];
    let bb = [project(b[0]), project(b[1]), project(b[2])];
    if aa.iter().any(|&p| inside(p, &bb)) || bb.iter().any(|&p| inside(p, &aa)) {
        return true;
    }
    for i in 0..3 {
        for j in 0..3 {
            if proper_cross(aa[i], aa[(i + 1) % 3], bb[j], bb[(j + 1) % 3]) {
                return true;
            }
        }
    }
    // Coincident triangles (all edges equal) have positive-area overlap.
    aa.iter().all(|p| bb.iter().any(|q| p == q))
}
/// Sweep along the axis with the largest center spread; test only overlapping AABBs.
/// `max_pairs` limits narrow-phase tests, not total face count. Incomplete scans are inconclusive.
pub fn inspect(faces: &[Tri], max_pairs: usize) -> IntersectionReport {
    let mut r = IntersectionReport {
        tested_pairs: 0,
        crossings: 0,
        coplanar_overlaps: 0,
        skipped_adjacent_pairs: 0,
        broadphase_candidates: 0,
        complete: true,
    };
    if faces.len() < 2 {
        return r;
    }
    // Check vertices directly: f64::min/max discard NaN, so bounds alone would hide it.
    if faces
        .iter()
        .any(|t| t.iter().any(|p| p.iter().any(|v| !v.is_finite())))
    {
        r.complete = false;
        return r;
    }
    let boxes: Vec<Aabb> = faces.iter().map(bounds).collect();
    let mut span = [0.0_f64; 3];
    for k in 0..3 {
        let low = boxes.iter().map(|b| b.0[k]).fold(f64::INFINITY, f64::min);
        let high = boxes
            .iter()
            .map(|b| b.1[k])
            .fold(f64::NEG_INFINITY, f64::max);
        span[k] = high - low;
    }
    let axis = (0..3)
        .max_by(|&a, &b| span[a].total_cmp(&span[b]))
        .unwrap_or(0);
    let mut order: Vec<usize> = (0..faces.len()).collect();
    order.sort_unstable_by(|&a, &b| boxes[a].0[axis].total_cmp(&boxes[b].0[axis]));
    let mut active: Vec<usize> = Vec::new();
    for &i in &order {
        active.retain(|&j| boxes[j].1[axis] >= boxes[i].0[axis]);
        for &j in &active {
            if !overlap(&boxes[i], &boxes[j]) {
                continue;
            }
            r.broadphase_candidates += 1;
            if shared(&faces[i], &faces[j]) {
                r.skipped_adjacent_pairs += 1;
                continue;
            }
            if r.tested_pairs >= max_pairs {
                r.complete = false;
                return r;
            }
            r.tested_pairs += 1;
            if crosses(&faces[i], &faces[j]) {
                r.crossings += 1;
            } else if coplanar_overlap(&faces[i], &faces[j]) {
                r.crossings += 1;
                r.coplanar_overlaps += 1;
            }
        }
        active.push(i);
    }
    r
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disjoint() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let b = [[2., 0., 0.], [3., 0., 0.], [2., 1., 0.]];
        let r = inspect(&[a, b], 100);
        assert_eq!(r.crossings, 0);
        assert_eq!(r.broadphase_candidates, 0);
        assert!(r.complete);
    }
    #[test]
    fn crossing() {
        let a = [[0., 0., 0.], [4., 0., 0.], [0., 4., 0.]];
        let b = [[1., 1., -1.], [1., 1., 1.], [2., 2., 1.]];
        assert_eq!(inspect(&[a, b], 100).crossings, 1);
    }
    #[test]
    fn cap_is_reported() {
        let a = [[0., 0., 0.], [4., 0., 0.], [0., 4., 0.]];
        let b = [[1., 1., -1.], [1., 1., 1.], [2., 2., 1.]];
        assert!(!inspect(&[a, b], 0).complete);
    }
    #[test]
    fn sparse_large_mesh() {
        let faces: Vec<Tri> = (0..10000)
            .map(|i| {
                let x = i as f64 * 3.;
                [[x, 0., 0.], [x + 1., 0., 0.], [x, 1., 0.]]
            })
            .collect();
        let r = inspect(&faces, 100);
        assert!(r.complete);
        assert_eq!(r.broadphase_candidates, 0);
    }
    #[test]
    fn invalid_coordinates_inconclusive() {
        let t = [[f64::NAN, 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        assert!(!inspect(&[t, t], 100).complete);
    }
}

#[cfg(test)]
mod coplanar_tests {
    use super::*;
    #[test]
    fn overlapping_coplanar() {
        let a = [[0., 0., 0.], [4., 0., 0.], [0., 4., 0.]];
        let b = [[1., 1., 0.], [2., 1., 0.], [1., 2., 0.]];
        let r = inspect(&[a, b], 100);
        assert_eq!(r.coplanar_overlaps, 1);
        assert_eq!(r.crossings, 1);
    }
    #[test]
    fn edge_touch_only() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let b = [[1., 0., 0.], [2., 0., 0.], [1., 1., 0.]];
        assert_eq!(inspect(&[a, b], 100).coplanar_overlaps, 0);
    }
    #[test]
    fn parallel_separate_planes() {
        let a = [[0., 0., 0.], [4., 0., 0.], [0., 4., 0.]];
        let b = [[1., 1., 1.], [2., 1., 1.], [1., 2., 1.]];
        assert_eq!(inspect(&[a, b], 100).coplanar_overlaps, 0);
    }
}
