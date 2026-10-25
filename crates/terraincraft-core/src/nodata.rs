//! Filling gaps (nodata samples) in elevation rasters.

/// Replace missing samples with the mean of their already-known 8-neighbors, growing
/// inward from known data one ring at a time. Small holes take on the surrounding
/// elevation; large ones are smoothly flattened. Returns the number of filled samples.
pub fn fill_missing(
    width: usize,
    height: usize,
    values: &mut [f64],
    missing: &[bool],
) -> Result<usize, String> {
    if values.len() != width * height || missing.len() != values.len() {
        return Err("Nodata mask does not match raster size".into());
    }
    let mut known: Vec<bool> = missing.iter().map(|m| !m).collect();
    if !known.iter().any(|k| *k) {
        return Err("Raster contains no elevation data".into());
    }
    let neighbors = |i: usize| {
        let (x, y) = ((i % width) as isize, (i / width) as isize);
        (-1..=1)
            .flat_map(move |dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
            .filter(move |&(nx, ny)| {
                (nx, ny) != (x, y)
                    && nx >= 0
                    && ny >= 0
                    && nx < width as isize
                    && ny < height as isize
            })
            .map(move |(nx, ny)| ny as usize * width + nx as usize)
    };
    let mut queued = known.clone();
    let mut frontier: Vec<usize> = (0..values.len())
        .filter(|&i| !known[i] && neighbors(i).any(|n| known[n]))
        .collect();
    for &i in &frontier {
        queued[i] = true;
    }
    let mut filled = 0;
    while !frontier.is_empty() {
        // Compute the whole ring from previously known samples before marking any of it.
        let ring: Vec<f64> = frontier
            .iter()
            .map(|&i| {
                let (sum, n) = neighbors(i)
                    .filter(|&n| known[n])
                    .fold((0., 0), |(s, c), n| (s + values[n], c + 1));
                sum / n as f64
            })
            .collect();
        for (&i, v) in frontier.iter().zip(ring) {
            values[i] = v;
            known[i] = true;
        }
        filled += frontier.len();
        let mut next = Vec::new();
        for &i in &frontier {
            for n in neighbors(i) {
                if !queued[n] {
                    queued[n] = true;
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fills_single_hole_with_neighbor_mean() {
        let mut v = vec![1., 1., 1., 1., 0., 1., 1., 1., 9.];
        let mut m = vec![false; 9];
        m[4] = true;
        assert_eq!(fill_missing(3, 3, &mut v, &m).unwrap(), 1);
        assert_eq!(v[4], 2.);
    }
    #[test]
    fn fills_large_region_from_edges() {
        let mut v = vec![5.; 25];
        let mut m = vec![true; 25];
        m[0] = false;
        assert_eq!(fill_missing(5, 5, &mut v, &m).unwrap(), 24);
        assert!(v.iter().all(|x| *x == 5.));
    }
    #[test]
    fn rejects_all_missing() {
        let mut v = vec![0.; 4];
        assert!(fill_missing(2, 2, &mut v, &[true; 4]).is_err());
    }
}
