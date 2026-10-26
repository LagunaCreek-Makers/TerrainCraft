//! Stacked laser-cut terrain: one sheet of material per elevation band.
//!
//! Layer 1 is the whole map; layer k+1 is the ground at or above the next elevation,
//! glued on top. Outlines come from interpolated marching squares with islands and lakes
//! as separate parts and holes, so edges follow the terrain rather than the raster grid.
//! Each part carries engraved marks: the outline of the layer that goes on top of it (to
//! place it while gluing) and its part number, hidden where that next layer covers it.
//!
//! 2D coordinates are model millimeters with y pointing down (north at the top), matching
//! the SVG outputs.
use crate::{Dem, TerrainSettings};
use geo::{
    Area, BooleanOps, BoundingRect, Buffer, Contains, InteriorPoint, LineString, MapCoords,
    MultiLineString, MultiPolygon, Polygon, Rect, Simplify,
};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Material, cutting and sheet settings for a stacked model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackSettings {
    /// Sheet material thickness; each layer stands this tall.
    pub material_mm: f64,
    /// Width of material the laser removes; cut paths are offset by half of it.
    pub kerf_mm: f64,
    /// Narrower necks and smaller islands or holes are removed as uncuttable.
    pub min_feature_mm: f64,
    /// Outline simplification tolerance.
    pub smooth_mm: f64,
    pub sheet_w_mm: f64,
    pub sheet_h_mm: f64,
    /// Gap between parts and from the sheet edge.
    pub spacing_mm: f64,
    /// Engrave the next layer's outline on each part as a placement guide.
    pub guides: bool,
    /// Engrave each part's number where the next layer will hide it.
    pub labels: bool,
}

impl Default for StackSettings {
    fn default() -> Self {
        Self {
            material_mm: 3.,
            kerf_mm: 0.15,
            min_feature_mm: 1.5,
            smooth_mm: 0.1,
            sheet_w_mm: 600.,
            sheet_h_mm: 400.,
            spacing_mm: 3.,
            guides: true,
            labels: true,
        }
    }
}

impl StackSettings {
    pub fn validate(&self) -> Result<(), String> {
        let positive = [self.material_mm, self.sheet_w_mm, self.sheet_h_mm];
        let non_negative = [
            self.kerf_mm,
            self.min_feature_mm,
            self.smooth_mm,
            self.spacing_mm,
        ];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.)
            || non_negative.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(
                "Material, sheet size, kerf, feature size and spacing must be valid".into(),
            );
        }
        if self.spacing_mm < self.kerf_mm {
            return Err("Part spacing must be at least the kerf width".into());
        }
        if self.sheet_w_mm <= 2. * self.spacing_mm || self.sheet_h_mm <= 2. * self.spacing_mm {
            return Err("Sheet is too small for the part spacing".into());
        }
        Ok(())
    }
}

/// Elevations where each layer above the base starts, so every layer is one sheet thick at
/// the model's vertical scale (relief and exaggeration from `s`, as in the 3D print).
pub fn levels_for_material(dem: &Dem, s: TerrainSettings, material_mm: f64) -> Vec<f64> {
    let (lo, hi) = dem.min_max();
    let model_relief = s.relief_mm * s.vertical_exaggeration;
    if !(material_mm > 0. && model_relief > 0. && hi > lo) {
        return Vec::new();
    }
    let step = material_mm * (hi - lo) / model_relief;
    (1..)
        .map(|k| lo + k as f64 * step)
        .take_while(|l| *l < hi)
        .take(1000)
        .collect()
}

/// The ground at or above `level` as polygons in model millimeters (y down), with lakes
/// as holes. Outlines are interpolated between samples; along the map edge they follow
/// the edge exactly, so every layer lines up at the rim.
pub fn region(dem: &Dem, w_mm: f64, h_mm: f64, level: f64) -> MultiPolygon<f64> {
    let (w, h) = (dem.width, dem.height);
    let (sx, sy) = (w_mm / (w - 1) as f64, h_mm / (h - 1) as f64);
    let pad = dem.min_max().0.min(level) - 1.;
    // Padded grid: a ring of below-everything samples sitting exactly on the map edge.
    let val = |i: usize, j: usize| {
        if (1..=w).contains(&i) && (1..=h).contains(&j) {
            dem.at(i - 1, j - 1)
        } else {
            pad
        }
    };
    let pos = |i: usize, j: usize| {
        (
            i.clamp(1, w) as f64 * sx - sx,
            j.clamp(1, h) as f64 * sy - sy,
        )
    };
    let inside = |i, j| val(i, j) >= level;
    // Edge keys: (0, i, j) is the edge from node (i, j) to (i+1, j); (1, i, j) to (i, j+1).
    let crossing = |key: (u8, usize, usize)| {
        let (a, b) = match key {
            (0, i, j) => ((i, j), (i + 1, j)),
            (_, i, j) => ((i, j), (i, j + 1)),
        };
        let (va, vb) = (val(a.0, a.1), val(b.0, b.1));
        // Keep crossings strictly inside the edge: a sample exactly at the level would
        // otherwise put points from several contours on the same node, touching rings.
        let f = ((level - va) / (vb - va)).clamp(1e-6, 1. - 1e-6);
        let (pa, pb) = (pos(a.0, a.1), pos(b.0, b.1));
        (pa.0 + (pb.0 - pa.0) * f, pa.1 + (pb.1 - pa.1) * f)
    };
    let mut segments: Vec<[(u8, usize, usize); 2]> = Vec::new();
    for j in 0..h + 1 {
        for i in 0..w + 1 {
            let c = [
                inside(i, j),
                inside(i + 1, j),
                inside(i + 1, j + 1),
                inside(i, j + 1),
            ];
            let e = [(0, i, j), (1, i + 1, j), (0, i, j + 1), (1, i, j)];
            // Edges around each corner: corner k touches edges k-1 and k.
            let corner_edges = |k: usize| [e[(k + 3) % 4], e[k]];
            let crossed: Vec<usize> = (0..4).filter(|&k| c[k] != c[(k + 1) % 4]).collect();
            match crossed.len() {
                2 => segments.push([e[crossed[0]], e[crossed[1]]]),
                4 => {
                    // Saddle: decide by the cell center which diagonal pair is connected,
                    // and cut off the two corners that are not.
                    let center =
                        (val(i, j) + val(i + 1, j) + val(i + 1, j + 1) + val(i, j + 1)) / 4.;
                    let isolate = center >= level;
                    for (k, &corner_inside) in c.iter().enumerate() {
                        if corner_inside != isolate {
                            segments.push(corner_edges(k));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Link segments into rings through their shared edge crossings.
    let mut at: HashMap<(u8, usize, usize), Vec<usize>> = HashMap::new();
    for (s, seg) in segments.iter().enumerate() {
        for k in seg {
            at.entry(*k).or_default().push(s);
        }
    }
    let mut used = vec![false; segments.len()];
    let mut rings: Vec<LineString<f64>> = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let first = segments[start][0];
        let mut key = segments[start][1];
        let mut pts = vec![crossing(first)];
        while key != first {
            pts.push(crossing(key));
            let Some(&next) = at[&key].iter().find(|&&s| !used[s]) else {
                break;
            };
            used[next] = true;
            let [a, b] = segments[next];
            key = if a == key { b } else { a };
        }
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        if pts.len() >= 3 {
            pts.push(pts[0]);
            rings.push(LineString::from(pts));
        }
    }
    nest(rings)
}

/// Turn non-crossing closed rings into polygons: rings inside an even number of others are
/// shells, the rest are holes of the smallest ring around them.
fn nest(mut rings: Vec<LineString<f64>>) -> MultiPolygon<f64> {
    let area = |r: &LineString<f64>| Polygon::new(r.clone(), vec![]).unsigned_area();
    rings.retain(|r| area(r) > 1e-9);
    rings.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let probes: Vec<geo::Point<f64>> = rings
        .iter()
        .map(|r| {
            let (a, b) = (r.0[0], r.0[1]);
            geo::Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
        })
        .collect();
    let mut parent: Vec<Option<usize>> = vec![None; rings.len()];
    let mut depth = vec![0usize; rings.len()];
    for i in 0..rings.len() {
        // Larger rings come first, so the last one containing ring i is its parent.
        for j in 0..i {
            if Polygon::new(rings[j].clone(), vec![]).contains(&probes[i]) {
                parent[i] = Some(j);
                depth[i] = depth[j] + 1;
            }
        }
    }
    let mut polys: Vec<(LineString<f64>, Vec<LineString<f64>>)> = Vec::new();
    let mut slot = vec![usize::MAX; rings.len()];
    for i in 0..rings.len() {
        if depth[i].is_multiple_of(2) {
            slot[i] = polys.len();
            polys.push((rings[i].clone(), Vec::new()));
        } else if let Some(p) = parent[i] {
            polys[slot[p]].1.push(rings[i].clone());
        }
    }
    MultiPolygon::new(polys.into_iter().map(|(s, h)| Polygon::new(s, h)).collect())
}

/// Remove features narrower than the minimum feature size (open by half of it), smooth
/// outlines, and drop pieces too small to handle.
fn clean(m: &MultiPolygon<f64>, st: &StackSettings) -> MultiPolygon<f64> {
    let r = st.min_feature_mm * 0.5;
    let opened = if r > 0. {
        m.buffer(-r).buffer(r)
    } else {
        m.clone()
    };
    drop_small(&opened.simplify(st.smooth_mm), st)
}

/// Drop islands smaller than a square four minimum features across (too small to label or
/// glue) and holes smaller than one two features across.
fn drop_small(m: &MultiPolygon<f64>, st: &StackSettings) -> MultiPolygon<f64> {
    let feature = st.min_feature_mm.max(0.5);
    let (min_part, min_hole) = ((4. * feature).powi(2), (2. * feature).powi(2));
    MultiPolygon::new(
        m.0.iter()
            .filter(|p| p.unsigned_area() >= min_part)
            .map(|p| {
                let holes = p
                    .interiors()
                    .iter()
                    .filter(|h| Polygon::new((*h).clone(), vec![]).unsigned_area() >= min_hole)
                    .cloned()
                    .collect();
                Polygon::new(p.exterior().clone(), holes)
            })
            .collect(),
    )
}

/// One piece of material to cut.
#[derive(Clone, Debug)]
pub struct Part {
    /// Engraved and listed name: `L<layer>`, plus `-<n>` for islands and split pieces.
    pub id: String,
    /// 1 is the base sheet.
    pub layer: usize,
    /// Lowest elevation this layer represents (the map minimum for the base).
    pub level_m: f64,
    /// Finished shape in model millimeters, before kerf compensation.
    pub shape: Polygon<f64>,
    /// Lines to engrave: the next layer's outline and the part number.
    pub engrave: MultiLineString<f64>,
}

/// Build every part of the stack: layer outlines at `levels` (see `levels_for_material`)
/// above a full-map base, split to fit the sheet, with guides and labels.
pub fn build_parts(
    dem: &Dem,
    s: TerrainSettings,
    levels: &[f64],
    st: &StackSettings,
) -> Result<Vec<Part>, String> {
    s.validate()?;
    st.validate()?;
    let (w, h) = (s.width_mm, s.height_mm);
    let base = MultiPolygon::new(vec![Rect::new((0., 0.), (w, h)).to_polygon()]);
    let mut layers = vec![(dem.min_max().0, base)];
    for &l in levels {
        // Cleaning layers separately can leave slivers overhanging the layer below, and a
        // stacked layer must sit entirely on the one beneath it.
        let below = &layers.last().expect("base layer").1;
        let m = drop_small(&clean(&region(dem, w, h, l), st).intersection(below), st);
        if m.0.is_empty() {
            break;
        }
        layers.push((l, m));
    }
    let usable = (
        st.sheet_w_mm - 2. * st.spacing_mm,
        st.sheet_h_mm - 2. * st.spacing_mm,
    );
    let mut parts = Vec::new();
    for (k, (level, shape)) in layers.iter().enumerate() {
        let next = layers.get(k + 1).map(|(_, m)| m);
        let mut pieces: Vec<Polygon<f64>> = Vec::new();
        let mut polys = shape.0.clone();
        polys.sort_by(|a, b| b.unsigned_area().total_cmp(&a.unsigned_area()));
        for p in polys {
            pieces.extend(split_to_fit(&p, usable));
        }
        let many = pieces.len() > 1;
        for (n, piece) in pieces.into_iter().enumerate() {
            let id = if many {
                format!("L{}-{}", k + 1, n + 1)
            } else {
                format!("L{}", k + 1)
            };
            let mut engrave = MultiLineString::new(vec![]);
            if let (true, Some(next)) = (st.guides, next) {
                let outline: Vec<LineString<f64>> = next
                    .0
                    .iter()
                    .flat_map(|p| std::iter::once(p.exterior()).chain(p.interiors()))
                    .cloned()
                    .collect();
                engrave
                    .0
                    .extend(piece.clip(&MultiLineString::new(outline), false).0);
            }
            if st.labels {
                let hidden = next.map(|m| MultiPolygon::new(vec![piece.clone()]).intersection(m));
                if let Some(text) = place_label(&id, &piece, hidden.as_ref()) {
                    engrave.0.extend(text);
                }
            }
            parts.push(Part {
                id,
                layer: k + 1,
                level_m: *level,
                shape: piece,
                engrave,
            });
        }
    }
    Ok(parts)
}

/// Cut a polygon on a grid of `max` (width, height) cells if it fits the sheet neither
/// way round.
fn split_to_fit(p: &Polygon<f64>, max: (f64, f64)) -> Vec<Polygon<f64>> {
    let Some(b) = p.bounding_rect() else {
        return Vec::new();
    };
    let (w, h) = (b.width(), b.height());
    let fits = |(mw, mh): (f64, f64)| (w <= mw && h <= mh) || (h <= mw && w <= mh);
    if fits(max) {
        return vec![p.clone()];
    }
    // Fewest equal pieces that fit, so no thin slivers are left over.
    let piece_fits = |(nx, ny): (usize, usize)| {
        let (pw, ph) = (w / nx as f64, h / ny as f64);
        (pw <= max.0 && ph <= max.1) || (ph <= max.0 && pw <= max.1)
    };
    let Some((nx, ny)) = (1..=400).find_map(|n: usize| {
        (1..=n)
            .filter(|&nx| n.is_multiple_of(nx))
            .map(|nx| (nx, n / nx))
            .find(|&grid| piece_fits(grid))
    }) else {
        return vec![p.clone()];
    };
    let (cw, ch) = (w / nx as f64, h / ny as f64);
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let x0 = b.min().x + i as f64 * cw;
            let y0 = b.min().y + j as f64 * ch;
            let r = Rect::new((x0, y0), (x0 + cw, y0 + ch)).to_polygon();
            out.extend(
                p.intersection(&r)
                    .0
                    .into_iter()
                    .filter(|q| q.unsigned_area() > 1e-6),
            );
        }
    }
    out
}

/// Seven-segment stroke text for engraving, `height` mm tall, top-left at (x, y), y down.
fn stroke_text(text: &str, x: f64, y: f64, height: f64) -> Vec<LineString<f64>> {
    let (gw, gap) = (height * 0.55, height * 0.35);
    let mut out = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let Some(bits) = crate::label::segments(c) else {
            continue;
        };
        let (l, r) = (x + i as f64 * (gw + gap), x + i as f64 * (gw + gap) + gw);
        let (t, m, b) = (y, y + height * 0.5, y + height);
        let bars = [
            ((l, t), (r, t)),
            ((r, t), (r, m)),
            ((r, m), (r, b)),
            ((l, b), (r, b)),
            ((l, m), (l, b)),
            ((l, t), (l, m)),
            ((l, m), (r, m)),
        ];
        for (k, (a, z)) in bars.into_iter().enumerate() {
            if bits & (1 << k) != 0 {
                out.push(LineString::from(vec![a, z]));
            }
        }
    }
    out
}

/// Place `id` where the next layer will cover it if there is room, otherwise anywhere on
/// the part (the top layer), shrinking the text from 6 mm down to 3 mm as needed.
fn place_label(
    id: &str,
    part: &Polygon<f64>,
    hidden: Option<&MultiPolygon<f64>>,
) -> Option<Vec<LineString<f64>>> {
    let candidates: Vec<Polygon<f64>> = match hidden {
        Some(m) if !m.0.is_empty() => {
            let mut v = m.0.clone();
            v.sort_by(|a, b| b.unsigned_area().total_cmp(&a.unsigned_area()));
            v
        }
        _ => vec![part.clone()],
    };
    let n = id.chars().count() as f64;
    for height in [6., 4.5, 3.] {
        let (tw, th) = (n * height * 0.9 - height * 0.35, height);
        for poly in candidates.iter().take(3) {
            // Shrink first so the text keeps a margin from the region's edge.
            for inner in poly.buffer(-height * 0.4).0 {
                let Some(c) = inner.interior_point() else {
                    continue;
                };
                let (x, y) = (c.x() - tw * 0.5, c.y() - th * 0.5);
                if inner.contains(&Rect::new((x, y), (x + tw, y + th)).to_polygon()) {
                    return Some(stroke_text(id, x, y, height));
                }
                // Turned a quarter turn for narrow regions: (u, v) -> (th - v, u).
                let (x, y) = (c.x() - th * 0.5, c.y() - tw * 0.5);
                if inner.contains(&Rect::new((x, y), (x + th, y + tw)).to_polygon()) {
                    return Some(
                        stroke_text(id, 0., 0., height)
                            .into_iter()
                            .map(|l| l.map_coords(|q| geo::coord! { x: x + th - q.y, y: y + q.x }))
                            .collect(),
                    );
                }
            }
        }
    }
    None
}

/// Where a part goes: its sheet, whether it is turned 90 degrees, and the offset of its
/// (turned) bounding box's top-left corner on the sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub part: usize,
    pub sheet: usize,
    pub rotated: bool,
    pub x_mm: f64,
    pub y_mm: f64,
}

/// Shelf-pack parts onto sheets by bounding box (turning a part 90 degrees when it only
/// fits that way, or when that keeps the shelf lower), largest first.
pub fn place(parts: &[Part], st: &StackSettings) -> Result<Vec<Placement>, String> {
    st.validate()?;
    let (uw, uh) = (
        st.sheet_w_mm - 2. * st.spacing_mm,
        st.sheet_h_mm - 2. * st.spacing_mm,
    );
    let grow = st.kerf_mm; // a kerf-offset outline grows by half the kerf on each side
    let mut order: Vec<usize> = (0..parts.len()).collect();
    let size = |i: usize| {
        let b = parts[i].shape.bounding_rect().expect("parts are non-empty");
        (b.width() + grow, b.height() + grow)
    };
    order.sort_by(|&a, &b| {
        size(b)
            .1
            .max(size(b).0)
            .total_cmp(&size(a).1.max(size(a).0))
    });
    let mut out = Vec::new();
    let (mut sheet, mut x, mut y, mut shelf) = (1, 0., 0., 0f64);
    for i in order {
        let (w, h) = size(i);
        // Prefer the orientation that is wider than tall, if it fits.
        let mut options = [(w, h, false), (h, w, true)];
        options.sort_by(|a, b| a.1.total_cmp(&b.1));
        let Some(&(pw, ph, rotated)) = options.iter().find(|o| o.0 <= uw && o.1 <= uh) else {
            return Err(format!("Part {} does not fit on the sheet", parts[i].id));
        };
        if x + pw > uw {
            (x, y, shelf) = (0., y + shelf + st.spacing_mm, 0.);
        }
        if y + ph > uh {
            (sheet, x, y, shelf) = (sheet + 1, 0., 0., 0.);
        }
        out.push(Placement {
            part: i,
            sheet,
            rotated,
            x_mm: st.spacing_mm + x + grow * 0.5,
            y_mm: st.spacing_mm + y + grow * 0.5,
        });
        x += pw + st.spacing_mm;
        shelf = shelf.max(ph);
    }
    Ok(out)
}

/// A part's geometry moved to its place on the sheet.
fn placed<G: MapCoords<f64, f64, Output = G>>(g: &G, part: &Part, p: &Placement) -> G {
    let b = part.shape.bounding_rect().expect("parts are non-empty");
    let (x0, y0, h) = (b.min().x, b.min().y, b.height());
    g.map_coords(|c| {
        let (u, v) = (c.x - x0, c.y - y0);
        // A quarter turn: (u, v) -> (h - v, u), a rotation rather than a mirror.
        let (u, v) = if p.rotated { (h - v, u) } else { (u, v) };
        geo::coord! { x: u + p.x_mm, y: v + p.y_mm }
    })
}

/// Kerf-compensated cut outlines and engrave lines for one sheet, in sheet millimeters.
pub fn sheet_geometry(
    parts: &[Part],
    placements: &[Placement],
    sheet: usize,
    st: &StackSettings,
) -> (Vec<LineString<f64>>, Vec<LineString<f64>>) {
    let mut cut = Vec::new();
    let mut engrave = Vec::new();
    for p in placements.iter().filter(|p| p.sheet == sheet) {
        let part = &parts[p.part];
        let outline = if st.kerf_mm > 0. {
            part.shape.buffer(st.kerf_mm * 0.5)
        } else {
            MultiPolygon::new(vec![part.shape.clone()])
        };
        for poly in placed(&outline, part, p).0 {
            cut.push(poly.exterior().clone());
            cut.extend(poly.interiors().iter().cloned());
        }
        engrave.extend(placed(&part.engrave, part, p).0);
    }
    (cut, engrave)
}

fn path_data(ls: &LineString<f64>, closed: bool) -> String {
    let mut d = String::new();
    for (i, c) in ls.0.iter().enumerate() {
        write!(
            d,
            "{}{:.3} {:.3} ",
            if i == 0 { "M" } else { "L" },
            c.x,
            c.y
        )
        .unwrap();
    }
    if closed {
        d.push('Z');
    }
    d
}

/// One sheet as SVG: red hairlines to cut, blue to engrave (LightBurn-style colors).
pub fn sheet_svg(
    parts: &[Part],
    placements: &[Placement],
    sheet: usize,
    st: &StackSettings,
) -> String {
    let (cut, engrave) = sheet_geometry(parts, placements, sheet, st);
    let (w, h) = (st.sheet_w_mm, st.sheet_h_mm);
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">
<g id="cut" fill="none" stroke="#ff0000" stroke-width="0.1">
"##
    );
    for ls in &cut {
        writeln!(out, r#"<path d="{}"/>"#, path_data(ls, true)).unwrap();
    }
    out.push_str(
        "</g>\n<g id=\"engrave\" fill=\"none\" stroke=\"#0000ff\" stroke-width=\"0.1\">\n",
    );
    for ls in &engrave {
        writeln!(out, r#"<path d="{}"/>"#, path_data(ls, false)).unwrap();
    }
    out.push_str("</g>\n</svg>\n");
    out
}

/// One sheet as DXF (R12, millimeters, y up) with CUT (red) and ENGRAVE (blue) layers.
pub fn sheet_dxf(
    parts: &[Part],
    placements: &[Placement],
    sheet: usize,
    st: &StackSettings,
) -> String {
    let (cut, engrave) = sheet_geometry(parts, placements, sheet, st);
    let mut out = String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
         0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n2\n\
         0\nLAYER\n2\nCUT\n70\n0\n62\n1\n6\nCONTINUOUS\n\
         0\nLAYER\n2\nENGRAVE\n70\n0\n62\n5\n6\nCONTINUOUS\n\
         0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
    );
    let mut polyline = |ls: &LineString<f64>, layer: &str, closed: bool| {
        let pts: &[geo::Coord<f64>] = if closed && ls.0.len() > 1 {
            &ls.0[..ls.0.len() - 1]
        } else {
            &ls.0
        };
        write!(
            out,
            "0\nPOLYLINE\n8\n{layer}\n66\n1\n70\n{}\n10\n0\n20\n0\n30\n0\n",
            u8::from(closed)
        )
        .unwrap();
        for c in pts {
            write!(
                out,
                "0\nVERTEX\n8\n{layer}\n10\n{:.4}\n20\n{:.4}\n30\n0\n",
                c.x,
                st.sheet_h_mm - c.y
            )
            .unwrap();
        }
        write!(out, "0\nSEQEND\n8\n{layer}\n").unwrap();
    };
    for ls in &cut {
        polyline(ls, "CUT", true);
    }
    for ls in &engrave {
        polyline(ls, "ENGRAVE", false);
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}

/// Every part's outline where it sits in the finished model (not on its sheet), shaded from
/// low to high layers: an overview of the stack and a check against the map.
pub fn layers_preview_svg(parts: &[Part], w_mm: f64, h_mm: f64) -> String {
    let top = parts.iter().map(|p| p.layer).max().unwrap_or(1).max(2);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w_mm}mm" height="{h_mm}mm" viewBox="0 0 {w_mm} {h_mm}">
"#
    );
    for p in parts {
        let t = (p.layer - 1) as f64 / (top - 1) as f64;
        let (r, g, b) = (
            (40. + 200. * t) as u8,
            (120. + 80. * (1. - t)) as u8,
            (90. * (1. - t)) as u8,
        );
        let mut d = path_data(p.shape.exterior(), true);
        for hole in p.shape.interiors() {
            d.push(' ');
            d.push_str(&path_data(hole, true));
        }
        writeln!(
            out,
            r#"<path data-part="{}" data-layer="{}" d="{d}" fill="rgb({r},{g},{b})" fill-opacity="0.35" fill-rule="evenodd" stroke="black" stroke-width="0.15"/>"#,
            p.id, p.layer
        )
        .unwrap();
    }
    out.push_str("</svg>\n");
    out
}

/// Parts list with sheet positions, for checking the job and sorting cut parts.
pub fn manifest_csv(parts: &[Part], placements: &[Placement]) -> String {
    let mut out =
        String::from("part,layer,level_m,sheet,x_mm,y_mm,rotated,width_mm,height_mm,area_mm2\n");
    for p in placements {
        let part = &parts[p.part];
        let b = part.shape.bounding_rect().expect("parts are non-empty");
        writeln!(
            out,
            "{},{},{:.2},{},{:.2},{:.2},{},{:.2},{:.2},{:.1}",
            part.id,
            part.layer,
            part.level_m,
            p.sheet,
            p.x_mm,
            p.y_mm,
            p.rotated,
            b.width(),
            b.height(),
            part.shape.unsigned_area()
        )
        .unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Validation;

    fn cone(n: usize) -> Dem {
        let c = (n - 1) as f64 / 2.;
        let v = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - c, (i / n) as f64 - c);
                100. - (x * x + y * y).sqrt()
            })
            .collect();
        Dem::new(n, n, v).unwrap()
    }

    #[test]
    fn cone_level_is_a_circle() {
        // Samples are 1 mm apart; the 80 m contour is a circle of radius 20 mm.
        let d = cone(61);
        let m = region(&d, 60., 60., 80.);
        assert_eq!(m.0.len(), 1);
        assert!(m.is_valid());
        let area = m.unsigned_area();
        let expected = std::f64::consts::PI * 400.;
        assert!(
            (area - expected).abs() / expected < 0.01,
            "{area} vs {expected}"
        );
    }

    #[test]
    fn ridge_ring_has_a_hole_and_edge_region_follows_the_map_edge() {
        // A circular ridge: high between radius 10 and 20 only.
        let n = 61;
        let v = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - 30., (i / n) as f64 - 30.);
                let r = (x * x + y * y).sqrt();
                if (10. ..=20.).contains(&r) {
                    10.
                } else {
                    0.
                }
            })
            .collect();
        let d = Dem::new(n, n, v).unwrap();
        let m = region(&d, 60., 60., 5.);
        assert!(m.is_valid());
        assert_eq!((m.0.len(), m.0[0].interiors().len()), (1, 1));
        // Everything above the minimum is the whole map, edges included.
        let all = region(&cone(31), 30., 30., 0.);
        assert!((all.unsigned_area() - 900.).abs() < 1e-6);
    }

    #[test]
    fn saddles_and_islands_stay_valid() {
        // Checkerboard of peaks: many islands meeting at saddle points.
        let n = 41;
        let v = (0..n * n)
            .map(|i| ((i % n) as f64 * 0.7).sin() * ((i / n) as f64 * 0.7).sin() * 50.)
            .collect();
        let d = Dem::new(n, n, v).unwrap();
        for level in [-20., 0., 20.] {
            let m = region(&d, 80., 80., level);
            assert!(m.is_valid(), "level {level}");
        }
    }

    #[test]
    fn stack_parts_nest_on_sheets() {
        let d = cone(61);
        let s = TerrainSettings {
            width_mm: 300.,
            height_mm: 300.,
            base_mm: 3.,
            vertical_exaggeration: 1.,
            relief_mm: 30.,
        };
        let st = StackSettings {
            sheet_w_mm: 400.,
            sheet_h_mm: 250.,
            ..StackSettings::default()
        };
        let levels = levels_for_material(&d, s, st.material_mm);
        assert_eq!(levels.len(), 9);
        let parts = build_parts(&d, s, &levels, &st).unwrap();
        // The 300 mm base does not fit a 250 mm sheet, so it is split into pieces.
        assert!(parts.iter().filter(|p| p.layer == 1).count() > 1);
        assert!(parts.iter().all(|p| p.shape.is_valid()));
        // Every layer below the top has a placement guide and a label.
        assert!(parts
            .iter()
            .filter(|p| p.layer < 10)
            .all(|p| !p.engrave.0.is_empty()));
        let placements = place(&parts, &st).unwrap();
        assert_eq!(placements.len(), parts.len());
        let sheets = placements.iter().map(|p| p.sheet).max().unwrap();
        for sheet in 1..=sheets {
            let (cut, _) = sheet_geometry(&parts, &placements, sheet, &st);
            let polys: Vec<Polygon<f64>> = cut
                .iter()
                .map(|r| Polygon::new(r.clone(), vec![]))
                .collect();
            for c in polys.iter().flat_map(|p| p.exterior().0.iter()) {
                assert!(c.x >= 0. && c.y >= 0. && c.x <= st.sheet_w_mm && c.y <= st.sheet_h_mm);
            }
            // Kerf-offset outlines of different parts never overlap.
            let shells: Vec<MultiPolygon<f64>> = placements
                .iter()
                .filter(|p| p.sheet == sheet)
                .map(|p| {
                    placed(
                        &parts[p.part].shape.buffer(st.kerf_mm * 0.5),
                        &parts[p.part],
                        p,
                    )
                })
                .collect();
            for i in 0..shells.len() {
                for j in i + 1..shells.len() {
                    assert!(shells[i].intersection(&shells[j]).unsigned_area() < 1e-6);
                }
            }
        }
        let svg = sheet_svg(&parts, &placements, 1, &st);
        assert!(svg.contains(r#"id="cut""#) && svg.contains(r#"id="engrave""#));
        let dxf = sheet_dxf(&parts, &placements, 1, &st);
        assert!(dxf.contains("\nCUT\n") && dxf.ends_with("EOF\n"));
        assert!(manifest_csv(&parts, &placements).lines().count() == parts.len() + 1);
    }

    #[test]
    fn quarter_turn_is_a_rotation() {
        let part = Part {
            id: "L1".into(),
            layer: 1,
            level_m: 0.,
            shape: Rect::new((10., 20.), (40., 30.)).to_polygon(),
            engrave: MultiLineString::new(vec![]),
        };
        let p = Placement {
            part: 0,
            sheet: 1,
            rotated: true,
            x_mm: 0.,
            y_mm: 0.,
        };
        let moved = placed(&part.shape, &part, &p);
        let b = moved.bounding_rect().unwrap();
        assert_eq!((b.width(), b.height()), (10., 30.));
        // Orientation (winding) is preserved by a rotation and reversed by a mirror.
        use geo::orient::{Direction, Orient};
        use geo::Winding;
        let before = part.shape.orient(Direction::Default);
        let after = placed(&before, &part, &p);
        assert_eq!(before.exterior().is_ccw(), after.exterior().is_ccw());
    }
}
