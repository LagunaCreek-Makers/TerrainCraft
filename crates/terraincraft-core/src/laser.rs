//! Closed laser-cut outlines and deterministic sheet layouts (no machine control).
use crate::Dem;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct CutPart {
    pub level: f64,
    pub outline: Vec<(f64, f64)>,
    pub id: String,
}
#[derive(Clone, Debug)]
pub struct Placement {
    pub part_id: String,
    pub sheet: usize,
    pub x_mm: f64,
    pub y_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
}
/// Threshold DEM cells at their four-corner mean, trace exposed edges into closed polygons.
/// This is a staircase approximation; resolution is controlled by the DEM sample spacing.
pub fn cut_parts(dem: &Dem, w_mm: f64, h_mm: f64, levels: &[f64]) -> Result<Vec<CutPart>, String> {
    if !w_mm.is_finite()
        || !h_mm.is_finite()
        || w_mm <= 0.
        || h_mm <= 0.
        || levels.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid cut geometry".into());
    }
    let mut parts = vec![];
    for (li, &level) in levels.iter().enumerate() {
        type P = (usize, usize);
        let mut edges: BTreeMap<P, Vec<P>> = BTreeMap::new();
        let active = |x: usize, y: usize| -> bool {
            if x >= dem.width - 1 || y >= dem.height - 1 {
                return false;
            }
            (dem.at(x, y) + dem.at(x + 1, y) + dem.at(x, y + 1) + dem.at(x + 1, y + 1)) / 4.
                >= level
        };
        for y in 0..dem.height - 1 {
            for x in 0..dem.width - 1 {
                if !active(x, y) {
                    continue;
                }
                let mut add = |a: P, b: P| edges.entry(a).or_default().push(b);
                if y == 0 || !active(x, y - 1) {
                    add((x, y), (x + 1, y));
                }
                if x == dem.width - 2 || !active(x + 1, y) {
                    add((x + 1, y), (x + 1, y + 1));
                }
                if y == dem.height - 2 || !active(x, y + 1) {
                    add((x + 1, y + 1), (x, y + 1));
                }
                if x == 0 || !active(x - 1, y) {
                    add((x, y + 1), (x, y));
                }
            }
        }
        let mut used = BTreeSet::new();
        let mut ring_num = 0;
        for (&start, ends) in &edges {
            for &first in ends {
                if used.contains(&(start, first)) {
                    continue;
                }
                let mut ring = vec![start];
                let mut a = start;
                let mut b = first;
                let mut valid = true;
                loop {
                    if !used.insert((a, b)) {
                        valid = false;
                        break;
                    }
                    ring.push(b);
                    if b == start {
                        break;
                    }
                    let options = match edges.get(&b) {
                        Some(v) => v,
                        None => {
                            valid = false;
                            break;
                        }
                    };
                    let next = options.iter().copied().find(|&v| !used.contains(&(b, v)));
                    match next {
                        Some(v) => {
                            a = b;
                            b = v
                        }
                        None => {
                            valid = false;
                            break;
                        }
                    }
                    if ring.len() > edges.len() + 1 {
                        valid = false;
                        break;
                    }
                }
                if valid && ring.len() >= 5 {
                    ring_num += 1;
                    parts.push(CutPart {
                        level,
                        outline: ring
                            .into_iter()
                            .map(|(x, y)| {
                                (
                                    x as f64 * w_mm / (dem.width - 1) as f64,
                                    y as f64 * h_mm / (dem.height - 1) as f64,
                                )
                            })
                            .collect(),
                        id: format!("L{:02}_P{:03}", li + 1, ring_num),
                    });
                }
            }
        }
    }
    Ok(parts)
}
fn bounds(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)),
    )
}
/// Deterministic bounding-box shelf packing. This is NOT irregular polygon nesting.
pub fn layout(
    parts: &[CutPart],
    sheet_w: f64,
    sheet_h: f64,
    gap: f64,
) -> Result<Vec<Placement>, String> {
    if [sheet_w, sheet_h]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
        || !gap.is_finite()
        || gap < 0.
    {
        return Err("Invalid sheet settings".into());
    }
    let mut out = vec![];
    let (mut sheet, mut x, mut y, mut row_h) = (1, gap, gap, 0f64);
    for p in parts {
        let (a, b, c, d) = bounds(&p.outline);
        let (w, h) = (c - a, d - b);
        if w + 2. * gap > sheet_w || h + 2. * gap > sheet_h {
            return Err(format!(
                "Part {} exceeds sheet dimensions; split or scale the part",
                p.id
            ));
        }
        if x + w + gap > sheet_w {
            x = gap;
            y += row_h + gap;
            row_h = 0.;
        }
        if y + h + gap > sheet_h {
            sheet += 1;
            x = gap;
            y = gap;
            row_h = 0.;
        }
        out.push(Placement {
            part_id: p.id.clone(),
            sheet,
            x_mm: x,
            y_mm: y,
            width_mm: w,
            height_mm: h,
        });
        x += w + gap;
        row_h = row_h.max(h);
    }
    Ok(out)
}
pub fn sheet_svg(
    parts: &[CutPart],
    placements: &[Placement],
    sheet: usize,
    w: f64,
    h: f64,
) -> String {
    let mut paths = String::new();
    for place in placements.iter().filter(|p| p.sheet == sheet) {
        if let Some(part) = parts.iter().find(|p| p.id == place.part_id) {
            let (a, b, _, _) = bounds(&part.outline);
            let d = part
                .outline
                .iter()
                .enumerate()
                .map(|(i, (x, y))| {
                    format!(
                        "{} {:.4} {:.4}",
                        if i == 0 { "M" } else { "L" },
                        x - a + place.x_mm,
                        y - b + place.y_mm
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            paths.push_str(&format!("<path id=\"{}\" d=\"{} Z\"/>\n", part.id, d));
        }
    }
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><g fill=\"none\" stroke=\"#ff0000\" stroke-width=\"0.1\">{paths}</g></svg>")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_contours() {
        let d = Dem::new(
            4,
            4,
            vec![
                0., 0., 0., 0., 0., 5., 5., 0., 0., 5., 5., 0., 0., 0., 0., 0.,
            ],
        )
        .unwrap();
        let p = cut_parts(&d, 30., 30., &[2.]).unwrap();
        assert!(!p.is_empty());
        assert_eq!(p[0].outline.first(), p[0].outline.last());
    }
    #[test]
    fn layout_multiple_sheets() {
        let p = vec![
            CutPart {
                id: "a".into(),
                level: 1.,
                outline: vec![(0., 0.), (40., 0.), (40., 40.), (0., 40.), (0., 0.)]
            };
            3
        ];
        let mut p = p;
        for (i, v) in p.iter_mut().enumerate() {
            v.id = format!("p{i}")
        }
        let result = layout(&p, 50., 50., 2.).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[2].sheet, 3);
    }
}

/// Clip a closed ring to an axis-aligned rectangle using Sutherland-Hodgman.
/// Each output ring is closed. Intended for simple outlines; holes and
/// disconnected components need a topology-aware boolean engine.
pub fn clip_ring_rect(
    ring: &[(f64, f64)],
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
) -> Vec<(f64, f64)> {
    let mut poly = ring.to_vec();
    if poly.first() == poly.last() {
        poly.pop();
    }
    for side in 0..4 {
        if poly.is_empty() {
            break;
        }
        let inside = |p: (f64, f64)| match side {
            0 => p.0 >= xmin,
            1 => p.0 <= xmax,
            2 => p.1 >= ymin,
            _ => p.1 <= ymax,
        };
        let intersect = |a: (f64, f64), b: (f64, f64)| -> (f64, f64) {
            let v = match side {
                0 => xmin,
                1 => xmax,
                2 => ymin,
                _ => ymax,
            };
            if side < 2 {
                let t = if (b.0 - a.0).abs() < 1e-12 {
                    0.
                } else {
                    (v - a.0) / (b.0 - a.0)
                };
                (v, a.1 + t * (b.1 - a.1))
            } else {
                let t = if (b.1 - a.1).abs() < 1e-12 {
                    0.
                } else {
                    (v - a.1) / (b.1 - a.1)
                };
                (a.0 + t * (b.0 - a.0), v)
            }
        };
        let old = poly;
        poly = Vec::new();
        let mut prev = *old.last().unwrap();
        for &cur in &old {
            match (inside(prev), inside(cur)) {
                (true, true) => poly.push(cur),
                (true, false) => poly.push(intersect(prev, cur)),
                (false, true) => {
                    poly.push(intersect(prev, cur));
                    poly.push(cur)
                }
                _ => {}
            }
            prev = cur;
        }
    }
    if poly.len() >= 3 {
        poly.push(poly[0]);
        poly
    } else {
        Vec::new()
    }
}
/// Split simple closed cut outlines into bounded rectangular sections.
/// Rectangular segmentation is conservative and does not add joints.
pub fn split_oversized(parts: &[CutPart], max_w: f64, max_h: f64) -> Result<Vec<CutPart>, String> {
    if !max_w.is_finite() || !max_h.is_finite() || max_w <= 0. || max_h <= 0. {
        return Err("Invalid segment dimensions".into());
    }
    let mut result = Vec::new();
    for part in parts {
        if part.outline.len() < 4 {
            return Err(format!("Invalid outline: {}", part.id));
        }
        let (a, b, c, d) = bounds(&part.outline);
        let nx = ((c - a) / max_w).ceil().max(1.) as usize;
        let ny = ((d - b) / max_h).ceil().max(1.) as usize;
        if nx.saturating_mul(ny) > 10000 {
            return Err("Excessive segmentation requested".into());
        }
        for iy in 0..ny {
            for ix in 0..nx {
                let x0 = a + ix as f64 * (c - a) / nx as f64;
                let x1 = a + (ix + 1) as f64 * (c - a) / nx as f64;
                let y0 = b + iy as f64 * (d - b) / ny as f64;
                let y1 = b + (iy + 1) as f64 * (d - b) / ny as f64;
                let outline = clip_ring_rect(&part.outline, x0, y0, x1, y1);
                if outline.len() >= 4 {
                    result.push(CutPart {
                        level: part.level,
                        id: format!("{}_R{:02}C{:02}", part.id, iy + 1, ix + 1),
                        outline,
                    });
                }
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod extra_tests {
    use super::*;
    #[test]
    fn split_rectangle() {
        let p = CutPart {
            id: "A".into(),
            level: 10.,
            outline: vec![(0., 0.), (100., 0.), (100., 60.), (0., 60.), (0., 0.)],
        };
        let out = split_oversized(&[p], 45., 35.).unwrap();
        assert_eq!(out.len(), 6);
        for p in out {
            let (a, b, c, d) = bounds(&p.outline);
            assert!(c - a <= 45.001 && d - b <= 35.001);
        }
    }
    #[test]
    fn clips_outside() {
        let r = vec![(0., 0.), (10., 0.), (10., 10.), (0., 10.), (0., 0.)];
        assert!(clip_ring_rect(&r, 20., 20., 30., 30.).is_empty());
    }
}
