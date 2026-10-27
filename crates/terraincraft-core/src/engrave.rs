//! Single-sheet engravings as vector line art: a topographic map and a first-person
//! mountain panorama. Everything, text included, is drawn as strokes (see `stroke_font`),
//! so a laser can engrave the result without fonts or conversion.
//!
//! Results are `Drawing`s: named layers of polylines in sheet millimeters (y down), each
//! with its own color so laser software can give it its own power and speed. The desktop
//! preview draws the same layers that `Drawing::to_svg` writes.
use crate::landmarks::Landmark;
use crate::stroke_font::{self, Align, Font};
use crate::{contours, Dem};
use std::fmt::Write as _;

pub type Line = Vec<(f64, f64)>;

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub color: [u8; 3],
    pub width_mm: f64,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug)]
pub struct Drawing {
    pub width_mm: f64,
    pub height_mm: f64,
    pub layers: Vec<Layer>,
    /// Typeface for all text in the drawing.
    pub font: Font,
}

// Layer colors follow LightBurn's palette so each imports as its own layer.
const BLACK: [u8; 3] = [0, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 224, 0];
const ORANGE: [u8; 3] = [255, 128, 0];
const CUT_RED: [u8; 3] = [255, 0, 0];

impl Drawing {
    fn new(width_mm: f64, height_mm: f64) -> Self {
        Self {
            width_mm,
            height_mm,
            layers: Vec::new(),
            font: Font::Sans,
        }
    }
    fn width(&self, s: &str, size: f64) -> f64 {
        stroke_font::width_in(self.font, s, size)
    }
    /// Add engraved lines (such as stroke text) to a green layer named `layer`.
    pub fn add_lines(&mut self, layer: &str, lines: Vec<Line>) {
        self.layer(layer, GREEN, 0.2).extend(lines);
    }
    /// Add icon artwork (see `svg_lines`) scaled into a `size` mm square centered at (x, y).
    pub fn add_icon(&mut self, icon: &crate::svg_lines::Strokes, x: f64, y: f64, size: f64) {
        let lines = icon.placed(x, y, size);
        self.layer("icons", GREEN, 0.2).extend(lines);
    }
    fn layer(&mut self, name: &str, color: [u8; 3], width_mm: f64) -> &mut Vec<Line> {
        if let Some(i) = self.layers.iter().position(|l| l.name == name) {
            return &mut self.layers[i].lines;
        }
        self.layers.push(Layer {
            name: name.into(),
            color,
            width_mm,
            lines: Vec::new(),
        });
        &mut self.layers.last_mut().expect("just pushed").lines
    }
    fn text(&mut self, layer: &str, s: &str, x: f64, y: f64, size: f64, align: Align) {
        let strokes = stroke_font::text_in(self.font, s, x, y, size, align, 0.);
        self.layer(layer, GREEN, 0.2).extend(strokes);
    }
    fn rect(&mut self, layer: &str, color: [u8; 3], width: f64, r: [f64; 4]) {
        let [x0, y0, x1, y1] = r;
        self.layer(layer, color, width).push(vec![
            (x0, y0),
            (x1, y0),
            (x1, y1),
            (x0, y1),
            (x0, y0),
        ]);
    }
    /// SVG with one group per layer.
    pub fn to_svg(&self) -> String {
        let (w, h) = (self.width_mm, self.height_mm);
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n"
        );
        for l in &self.layers {
            let [r, g, b] = l.color;
            writeln!(
                out,
                "<g id=\"{}\" fill=\"none\" stroke=\"#{r:02x}{g:02x}{b:02x}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
                l.name, l.width_mm
            )
            .unwrap();
            for line in l.lines.iter().filter(|l| l.len() >= 2) {
                out.push_str("<path d=\"");
                for (i, (x, y)) in line.iter().enumerate() {
                    write!(out, "{}{x:.3} {y:.3} ", if i == 0 { "M" } else { "L" }).unwrap();
                }
                out.push_str("\"/>\n");
            }
            out.push_str("</g>\n");
        }
        out.push_str("</svg>\n");
        out
    }
}

/// Bilinear elevation at a fractional sample position, clamped to the grid.
fn sample(d: &Dem, col: f64, row: f64) -> f64 {
    let x = col.clamp(0., (d.width - 1) as f64);
    let y = row.clamp(0., (d.height - 1) as f64);
    let (ix, iy) = (
        (x as usize).min(d.width - 2),
        (y as usize).min(d.height - 2),
    );
    let (u, v) = (x - ix as f64, y - iy as f64);
    let top = d.at(ix, iy) * (1. - u) + d.at(ix + 1, iy) * u;
    let bottom = d.at(ix, iy + 1) * (1. - u) + d.at(ix + 1, iy + 1) * u;
    top * (1. - v) + bottom * v
}

/// Highest sample within `radius` samples: snaps a named summit to the actual high point.
fn local_max(d: &Dem, col: f64, row: f64, radius: i64) -> f64 {
    let (c, r) = (col.round() as i64, row.round() as i64);
    let mut best = f64::NEG_INFINITY;
    for y in r - radius..=r + radius {
        for x in c - radius..=c + radius {
            if x >= 0 && y >= 0 && (x as usize) < d.width && (y as usize) < d.height {
                best = best.max(d.at(x as usize, y as usize));
            }
        }
    }
    best
}

/// The largest of 1, 2, 2.5 or 5 times a power of ten that is at most `x`.
fn nice_below(x: f64) -> f64 {
    let p = 10f64.powf(x.log10().floor());
    [5., 2.5, 2., 1.]
        .into_iter()
        .map(|m| m * p)
        .find(|v| *v <= x)
        .unwrap_or(p)
}

fn thousands(v: f64) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

const FEET_PER_M: f64 = 1. / 0.3048;

fn elevation_text(m: f64, feet: bool) -> String {
    if feet {
        format!("{} ft", thousands(m * FEET_PER_M))
    } else {
        format!("{} m", thousands(m))
    }
}

/// Axis-aligned boxes already taken by labels and markers.
#[derive(Default)]
struct Occupied(Vec<[f64; 4]>);

impl Occupied {
    fn free(&self, r: [f64; 4]) -> bool {
        !self
            .0
            .iter()
            .any(|o| r[0] < o[2] && o[0] < r[2] && r[1] < o[3] && o[1] < r[3])
    }
}

/// How a map is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MapStyle {
    /// Survey style: every contour, labeled index contours, coordinate frame, triangles
    /// for summits.
    #[default]
    Exact,
    /// Illustrated: features drawn as icons, fewer and unlabeled contours, a decorative
    /// double frame and a compass rose.
    Stylized,
}

/// Top-down topographic map settings.
#[derive(Clone, Debug, PartialEq)]
pub struct MapOptions {
    pub style: MapStyle,
    pub font: Font,
    pub sheet_w_mm: f64,
    pub sheet_h_mm: f64,
    pub margin_mm: f64,
    /// Contour spacing in the display unit (meters or feet); 0 picks one automatically.
    pub contour_interval: f64,
    /// Every n-th contour is a heavier, labeled index contour.
    pub index_every: usize,
    pub title: String,
    pub subtitle: String,
    /// Label elevations and contours in feet instead of meters.
    pub feet: bool,
    pub label_contours: bool,
    pub label_features: bool,
    pub frame: bool,
    pub coordinates: bool,
    pub scale_bar: bool,
    pub north_arrow: bool,
    /// Add a red outline of the sheet to cut the plaque out.
    pub cut_outline: bool,
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            style: MapStyle::Exact,
            font: Font::Sans,
            sheet_w_mm: 400.,
            sheet_h_mm: 300.,
            margin_mm: 10.,
            contour_interval: 0.,
            index_every: 5,
            title: String::new(),
            subtitle: String::new(),
            feet: false,
            label_contours: true,
            label_features: true,
            frame: true,
            coordinates: true,
            scale_bar: true,
            north_arrow: true,
            cut_outline: false,
        }
    }
}

/// Interval giving roughly 25 contours over `range`, from 1-2-2.5-5 steps.
fn auto_interval(range: f64) -> f64 {
    nice_below((range / 25.).max(1.))
}

/// Remove the part of `line` within `r` of `c`, splitting it where the label sits.
fn cut_around(line: &[(f64, f64)], c: (f64, f64), r: f64) -> Vec<Line> {
    let mut out = vec![Vec::new()];
    for &p in line {
        if (p.0 - c.0).hypot(p.1 - c.1) < r {
            if !out.last().expect("non-empty").is_empty() {
                out.push(Vec::new());
            }
        } else {
            out.last_mut().expect("non-empty").push(p);
        }
    }
    out.retain(|l| l.len() >= 2);
    out
}

/// Insert points so no segment is longer than `step`.
fn densify(line: &[(f64, f64)], step: f64) -> Line {
    let mut out = Vec::with_capacity(line.len());
    for w in line.windows(2) {
        let n = ((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) / step)
            .ceil()
            .max(1.) as usize;
        for k in 0..n {
            let t = k as f64 / n as f64;
            out.push((
                w[0].0 + (w[1].0 - w[0].0) * t,
                w[0].1 + (w[1].1 - w[0].1) * t,
            ));
        }
    }
    out.extend(line.last());
    out
}

fn length(line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

/// Draw a topographic map of `dem` with `names` labeled. Exact style: summits and pillars
/// get a triangle, gaps a pass mark, other classes just their name. Stylized style:
/// features are drawn as library icons. A symbol `icon:ID` (a library icon id such as
/// `icon:maki:volcano`) always draws that icon.
pub fn topo_map(dem: &Dem, names: &[Landmark], o: &MapOptions) -> Result<Drawing, String> {
    if !(o.sheet_w_mm > 4. * o.margin_mm && o.sheet_h_mm > 4. * o.margin_mm && o.margin_mm >= 0.) {
        return Err("Sheet is too small for its margins".into());
    }
    let mut d = Drawing::new(o.sheet_w_mm, o.sheet_h_mm);
    d.font = o.font;
    let stylized = o.style == MapStyle::Stylized;
    let unit = if o.feet { 0.3048 } else { 1. };
    let (lo, hi) = dem.min_max();
    let interval = if o.contour_interval > 0. {
        o.contour_interval
    } else if stylized {
        // Fewer lines leave room for the illustrations.
        auto_interval((hi - lo) / unit) * 2.
    } else {
        auto_interval((hi - lo) / unit)
    };
    let coordinates = o.coordinates && !stylized;
    if (hi - lo) / unit / interval > 2000. {
        return Err("Too many contours; increase the contour interval".into());
    }
    // Layout: title band, map, then a band for the scale bar and notes.
    let label_band = if coordinates && dem.georef.is_some() {
        6.
    } else {
        0.
    };
    let top = o.margin_mm + if o.title.is_empty() { 0. } else { 16. } + label_band;
    let bottom_band = if o.scale_bar || o.north_arrow {
        18.
    } else {
        8.
    };
    let avail = (
        o.sheet_w_mm - 2. * (o.margin_mm + label_band),
        o.sheet_h_mm - top - o.margin_mm - bottom_band - label_band,
    );
    let (gx, gy) = match dem.georef {
        Some(g) => g.sample_span_m(dem.width, dem.height),
        None => ((dem.width - 1) as f64, (dem.height - 1) as f64),
    };
    let k = (avail.0 / gx).min(avail.1 / gy);
    let (mw, mh) = (gx * k, gy * k);
    let (ox, oy) = ((o.sheet_w_mm - mw) * 0.5, top + (avail.1 - mh) * 0.5);
    let to_map = |col: f64, row: f64| {
        (
            ox + col / (dem.width - 1) as f64 * mw,
            oy + row / (dem.height - 1) as f64 * mh,
        )
    };
    let mut taken = Occupied::default();

    // Named features first, highest first, so the important ones get room.
    let mut features: Vec<(f64, f64, f64, &Landmark)> = Vec::new();
    if let (true, Some(g)) = (o.label_features, dem.georef) {
        for n in names {
            let Some((c, r)) = g.latlon_to_sample(n.lat, n.lon) else {
                continue;
            };
            if c < 0. || r < 0. || c > (dem.width - 1) as f64 || r > (dem.height - 1) as f64 {
                continue;
            }
            let elev = n.elevation_m.unwrap_or_else(|| local_max(dem, c, r, 2));
            let (x, y) = to_map(c, r);
            features.push((x, y, elev, n));
        }
    }
    features.sort_by(|a, b| b.2.total_cmp(&a.2));
    for (x, y, elev, n) in features {
        let peak = matches!(n.symbol.as_str(), "summit" | "pillar" | "peak");
        let (size, small) = (2.8, 2.2);
        let name_w = d.width(&n.name, size);
        let elev_s = elevation_text(elev, o.feet);
        let w = name_w.max(if peak { d.width(&elev_s, small) } else { 0. });
        let h = if peak { size + small + 1.2 } else { size };
        // Stylized maps draw the feature as an icon with its name underneath; features
        // with an explicit `icon:ID` symbol always do.
        let library_icon = n
            .symbol
            .strip_prefix("icon:")
            .and_then(crate::icon_library::resolve)
            .and_then(crate::icon_library::svg)
            .and_then(|s| crate::svg_lines::parse(s).ok());
        let icon = library_icon
            .as_ref()
            .or_else(|| stylized.then(|| feature_icon(&n.symbol)).flatten());
        if let Some(icon) = icon {
            let s = 6.;
            let (ty, label_w) = (y + s * 0.55, w);
            let r = [x - s * 0.5, y - s * 0.5, x + s * 0.5, y + s * 0.5];
            let label = [
                x - label_w * 0.5 - 0.5,
                ty,
                x + label_w * 0.5 + 0.5,
                ty + h + 0.5,
            ];
            let inside = |b: [f64; 4]| b[0] > ox && b[2] < ox + mw && b[1] > oy && b[3] < oy + mh;
            if inside(r) && inside(label) && taken.free(r) && taken.free(label) {
                taken.0.push(r);
                taken.0.push(label);
                d.add_icon(icon, x, y, s);
                d.text("labels", &n.name, x, ty + size, size, Align::Center);
                if peak {
                    d.text(
                        "labels",
                        &elev_s,
                        x,
                        ty + size + 1.2 + small,
                        small,
                        Align::Center,
                    );
                }
            }
            continue;
        }
        // Peaks: right, left, above or below the marker. Ridges and other unmarked
        // features: centered on the point first.
        let mut spots = vec![
            (x + 2.5, y - h * 0.5, Align::Left),
            (x - 2.5, y - h * 0.5, Align::Right),
            (x, y - 2.5 - h, Align::Center),
            (x, y + 2.5, Align::Center),
        ];
        if !peak && n.symbol != "gap" {
            spots.insert(0, (x, y - h * 0.5, Align::Center));
        }
        let Some(&(tx, ty, align)) = spots.iter().find(|(tx, ty, a)| {
            let x0 = match a {
                Align::Left => *tx,
                Align::Right => tx - w,
                Align::Center => tx - w * 0.5,
            };
            let r = [x0 - 0.5, ty - 0.5, x0 + w + 0.5, ty + h + 0.5];
            r[0] > ox && r[2] < ox + mw && r[1] > oy && r[3] < oy + mh && taken.free(r)
        }) else {
            continue;
        };
        let x0 = match align {
            Align::Left => tx,
            Align::Right => tx - w,
            Align::Center => tx - w * 0.5,
        };
        taken
            .0
            .push([x0 - 0.5, ty - 0.5, x0 + w + 0.5, ty + h + 0.5]);
        taken.0.push([x - 1.6, y - 1.6, x + 1.6, y + 1.6]);
        let marks = d.layer("labels", GREEN, 0.2);
        if peak {
            marks.push(vec![
                (x, y - 1.3),
                (x + 1.2, y + 0.8),
                (x - 1.2, y + 0.8),
                (x, y - 1.3),
            ]);
        } else if n.symbol == "gap" {
            marks.push(vec![(x - 1.2, y - 1.), (x - 0.5, y), (x - 1.2, y + 1.)]);
            marks.push(vec![(x + 1.2, y - 1.), (x + 0.5, y), (x + 1.2, y + 1.)]);
        }
        d.text("labels", &n.name, tx, ty + size, size, align);
        if peak {
            d.text("labels", &elev_s, tx, ty + size + 1.2 + small, small, align);
        }
    }

    // Contours, index contours labeled along the line with the line broken under the label.
    let first = (lo / unit / interval).ceil() as i64;
    let last = (hi / unit / interval).floor() as i64;
    let index_every = o.index_every.max(1) as i64;
    for i in first..=last {
        let value = i as f64 * interval;
        let index = i.rem_euclid(index_every) == 0;
        let paths = contours::paths(dem, mw, mh, value * unit)?;
        for p in paths {
            let mut line: Line = p.iter().map(|(x, y)| (x + ox, y + oy)).collect();
            if line.len() < 2 {
                continue;
            }
            let mut pieces = vec![line.clone()];
            if index && o.label_contours && !stylized && length(&line) > 60. {
                let label = thousands(value);
                let size = 2.2;
                let w = d.width(&label, size);
                // Walk to the middle of the line and read its direction there.
                let half = length(&line) * 0.5;
                let mut acc = 0.;
                let mut at = None;
                for win in line.windows(2) {
                    let seg = (win[1].0 - win[0].0).hypot(win[1].1 - win[0].1);
                    if acc + seg >= half && seg > 0. {
                        let t = (half - acc) / seg;
                        let c = (
                            win[0].0 + (win[1].0 - win[0].0) * t,
                            win[0].1 + (win[1].1 - win[0].1) * t,
                        );
                        let mut angle = (win[1].1 - win[0].1)
                            .atan2(win[1].0 - win[0].0)
                            .to_degrees();
                        // Keep text upright.
                        if angle > 90. {
                            angle -= 180.;
                        } else if angle < -90. {
                            angle += 180.;
                        }
                        at = Some((c, angle));
                        break;
                    }
                    acc += seg;
                }
                if let Some((c, angle)) = at {
                    let r = w * 0.5 + 0.8;
                    let bbox = [c.0 - r, c.1 - r, c.0 + r, c.1 + r];
                    if taken.free(bbox) {
                        taken.0.push(bbox);
                        let (sin, cos) = angle.to_radians().sin_cos();
                        // Baseline offset so the text is centered on the line.
                        let (bx, by) = (c.0 - sin * size * 0.5, c.1 + cos * size * 0.5);
                        let strokes = stroke_font::text_in(
                            d.font,
                            &label,
                            bx,
                            by,
                            size,
                            Align::Center,
                            angle,
                        );
                        d.layer("labels", GREEN, 0.2).extend(strokes);
                        pieces = cut_around(&line, c, r);
                    }
                }
            }
            // Contours also stop short of feature labels and markers. Densify first so a
            // segment crossing a label is cut at the label, not at its far vertex.
            for piece in pieces {
                line = densify(&piece, 0.3);
                let mut run: Line = Vec::new();
                let mut kept: Vec<Line> = Vec::new();
                for &p in &line {
                    if taken.free([p.0, p.1, p.0, p.1]) || !o.label_features {
                        run.push(p);
                    } else if run.len() >= 2 {
                        kept.push(std::mem::take(&mut run));
                    } else {
                        run.clear();
                    }
                }
                if run.len() >= 2 {
                    kept.push(run);
                }
                let (name, color, w) = if index {
                    ("index-contours", BLUE, 0.3)
                } else {
                    ("contours", BLACK, 0.15)
                };
                d.layer(name, color, w).extend(kept);
            }
        }
    }

    if o.frame {
        d.rect("frame", ORANGE, 0.3, [ox, oy, ox + mw, oy + mh]);
        if stylized {
            let g = 2.5;
            d.rect(
                "frame",
                ORANGE,
                0.5,
                [ox - g, oy - g, ox + mw + g, oy + mh + g],
            );
        }
    }
    if let (true, Some(g)) = (coordinates, dem.georef) {
        coordinate_ticks(&mut d, dem, &g, (ox, oy, mw, mh));
    }
    if !o.title.is_empty() {
        d.text(
            "labels",
            &o.title,
            o.sheet_w_mm * 0.5,
            o.margin_mm + 8.,
            7.,
            Align::Center,
        );
        if !o.subtitle.is_empty() {
            d.text(
                "labels",
                &o.subtitle,
                o.sheet_w_mm * 0.5,
                o.margin_mm + 14.,
                3.5,
                Align::Center,
            );
        }
    }
    let base_y = oy + mh + label_band + 6.;
    let unit_name = if o.feet { "ft" } else { "m" };
    let mut note = format!("Contour interval {} {unit_name}", thousands(interval));
    if dem.georef.is_some() {
        note.push_str("   Elevation: USGS 3DEP");
    }
    d.text("labels", &note, ox, base_y + 10., 2.5, Align::Left);
    if let (true, Some(_)) = (o.scale_bar, dem.georef) {
        scale_bar(&mut d, mw / gx, ox + mw * 0.5, base_y, mw * 0.3, o.feet);
    }
    if o.north_arrow && stylized {
        compass_rose(&mut d, ox + mw - 8., base_y + 4., 7.);
    } else if o.north_arrow {
        let (x, y) = (ox + mw - 4., base_y - 2.);
        d.layer("labels", GREEN, 0.2).push(vec![
            (x, y),
            (x + 2.5, y + 8.),
            (x, y + 6.),
            (x - 2.5, y + 8.),
            (x, y),
        ]);
        d.text("labels", "N", x, y - 1., 3.5, Align::Center);
    }
    if o.cut_outline {
        d.rect("cut", CUT_RED, 0.1, [0., 0., o.sheet_w_mm, o.sheet_h_mm]);
    }
    Ok(d)
}

/// Icon for a named feature class on stylized maps, from the built-in icon library.
fn feature_icon(symbol: &str) -> Option<&'static crate::svg_lines::Strokes> {
    use std::sync::OnceLock;
    static ICONS: OnceLock<Vec<crate::svg_lines::Strokes>> = OnceLock::new();
    // Summit, rock pillar, pass, ridge.
    let icons = ICONS.get_or_init(|| {
        [
            "maki:mountain",
            "tabler:building-monument",
            "tabler:route",
            "tabler:mountain",
        ]
        .iter()
        .map(|id| {
            let svg = crate::icon_library::svg(id).expect("bundled icon");
            crate::svg_lines::parse(svg).expect("built-in icons parse")
        })
        .collect()
    });
    match symbol {
        "summit" | "peak" => Some(&icons[0]),
        "pillar" => Some(&icons[1]),
        "gap" => Some(&icons[2]),
        "ridge" => Some(&icons[3]),
        _ => None,
    }
}

/// An eight-point compass rose of radius `r` centered at (x, y), with N above.
fn compass_rose(d: &mut Drawing, x: f64, y: f64, r: f64) {
    let star: Line = (0..=16)
        .map(|i| {
            let a = (i as f64 * 22.5).to_radians();
            let rr = if i % 4 == 0 {
                r
            } else if i % 2 == 0 {
                r * 0.55
            } else {
                r * 0.22
            };
            (x + rr * a.sin(), y - rr * a.cos())
        })
        .collect();
    let lines = d.layer("labels", GREEN, 0.2);
    lines.push(star);
    lines.push(vec![(x, y - r), (x, y + r)]);
    lines.push(vec![(x - r, y), (x + r, y)]);
    d.text("labels", "N", x, y - r - 1., 3., Align::Center);
}

/// Tick marks and coordinate labels around the map frame: degrees and minutes for
/// latitude/longitude data, kilometers for UTM.
fn coordinate_ticks(
    d: &mut Drawing,
    dem: &Dem,
    g: &crate::georef::GeoReference,
    map: (f64, f64, f64, f64),
) {
    use crate::georef::CrsUnits;
    let (ox, oy, mw, mh) = map;
    let (w, h) = ((dem.width - 1) as f64, (dem.height - 1) as f64);
    // Coordinates of the map edges (sample centers at the corners).
    let (x0, y0) = (
        g.origin_x + g.pixel_width * 0.5,
        g.origin_y - g.pixel_height * 0.5,
    );
    let (x1, y1) = (x0 + w * g.pixel_width, y0 - h * g.pixel_height);
    let degrees = g.units == CrsUnits::Degrees;
    let fmt = |v: f64, pos: &str, neg: &str| -> String {
        if degrees {
            let hemi = if v < 0. { neg } else { pos };
            let total = (v.abs() * 60.).round() as i64;
            format!("{}°{:02}'{hemi}", total / 60, total % 60)
        } else {
            format!("{} km", thousands(v / 1000.))
        }
    };
    // Interval: 1, 2, 5, 10, 15, 30 minutes or whole degrees; or nice kilometers.
    let pick = |span: f64| -> f64 {
        if degrees {
            [1., 2., 5., 10., 15., 30., 60., 120.]
                .into_iter()
                .map(|m| m / 60.)
                .find(|s| span / s <= 6.)
                .unwrap_or(5.)
        } else {
            nice_below(span / 4.).max(1000.)
        }
    };
    let (sx, sy) = (pick(x1 - x0), pick(y0 - y1));
    let layer_lines = |d: &mut Drawing, l: Line| d.layer("frame", ORANGE, 0.3).push(l);
    let mut v = (x0 / sx).ceil() * sx;
    while v <= x1 + 1e-9 {
        let x = ox + (v - x0) / (x1 - x0) * mw;
        layer_lines(d, vec![(x, oy), (x, oy + 2.)]);
        layer_lines(d, vec![(x, oy + mh), (x, oy + mh - 2.)]);
        d.text("labels", &fmt(v, "E", "W"), x, oy - 1.5, 2.3, Align::Center);
        v += sx;
    }
    let mut v = (y1 / sy).ceil() * sy;
    while v <= y0 + 1e-9 {
        let y = oy + (y0 - v) / (y0 - y1) * mh;
        layer_lines(d, vec![(ox, y), (ox + 2., y)]);
        layer_lines(d, vec![(ox + mw, y), (ox + mw - 2., y)]);
        d.text(
            "labels",
            &fmt(v, "N", "S"),
            ox - 1.5,
            y + 1.1,
            2.3,
            Align::Right,
        );
        v += sy;
    }
}

/// A scale bar centered at (cx, y): ticks at 0, half and full length, labeled in km/m or
/// mi/ft, with the representative fraction.
fn scale_bar(d: &mut Drawing, mm_per_m: f64, cx: f64, y: f64, max_mm: f64, feet: bool) {
    let (unit_m, big, small) = if feet {
        (1609.344, "mi", "ft")
    } else {
        (1000., "km", "m")
    };
    let length_m = {
        let units = nice_below(max_mm / mm_per_m / unit_m);
        if units >= 0.5 {
            units * unit_m
        } else if feet {
            nice_below(max_mm / mm_per_m * FEET_PER_M) / FEET_PER_M
        } else {
            nice_below(max_mm / mm_per_m)
        }
    };
    let len = length_m * mm_per_m;
    let x0 = cx - len * 0.5;
    let label = |m: f64| -> String {
        if m >= unit_m * 0.5 {
            let v = m / unit_m;
            if (v - v.round()).abs() < 1e-9 {
                format!("{v:.0} {big}")
            } else {
                format!("{v} {big}")
            }
        } else if feet {
            format!("{} {small}", thousands(m * FEET_PER_M))
        } else {
            format!("{} {small}", thousands(m))
        }
    };
    let lines = d.layer("labels", GREEN, 0.2);
    lines.push(vec![(x0, y), (x0 + len, y)]);
    for t in [0., 0.5, 1.] {
        let x = x0 + len * t;
        lines.push(vec![(x, y - 1.5), (x, y + 1.5)]);
    }
    d.text("labels", "0", x0, y - 2.2, 2.5, Align::Center);
    d.text(
        "labels",
        &label(length_m * 0.5),
        x0 + len * 0.5,
        y - 2.2,
        2.5,
        Align::Center,
    );
    d.text(
        "labels",
        &label(length_m),
        x0 + len,
        y - 2.2,
        2.5,
        Align::Center,
    );
    let ratio = 1000. / mm_per_m;
    let p = 10f64.powf(ratio.log10().floor() - 1.);
    d.text(
        "labels",
        &format!("Scale 1:{}", thousands((ratio / p).round() * p)),
        cx,
        y + 5.5,
        2.5,
        Align::Center,
    );
}

/// Where the observer stands and looks, for `panorama`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanoramaView {
    pub lat: f64,
    pub lon: f64,
    /// Center of the view, degrees clockwise from true north.
    pub bearing_deg: f64,
    pub fov_deg: f64,
    pub eye_height_m: f64,
    pub max_distance_km: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanoramaOptions {
    pub font: Font,
    pub sheet_w_mm: f64,
    pub sheet_h_mm: f64,
    pub margin_mm: f64,
    pub title: String,
    pub subtitle: String,
    pub label_features: bool,
    pub feet: bool,
    pub compass: bool,
    pub cut_outline: bool,
    /// A crest is drawn only where the terrain behind it drops at least this far below
    /// the line of sight, so small bumps don't become lines.
    pub min_ridge_drop_m: f64,
}

impl Default for PanoramaOptions {
    fn default() -> Self {
        Self {
            font: Font::Sans,
            sheet_w_mm: 500.,
            sheet_h_mm: 200.,
            margin_mm: 10.,
            title: String::new(),
            subtitle: String::new(),
            label_features: true,
            feet: false,
            compass: true,
            cut_outline: false,
            min_ridge_drop_m: 15.,
        }
    }
}

/// Earth radius and the standard refraction coefficient for long sight lines.
const EARTH_R: f64 = 6_371_000.;
const REFRACTION: f64 = 0.13;

/// One visible crest along a column's sight line.
#[derive(Clone, Copy, Debug)]
struct Crest {
    distance: f64,
    angle: f64,
}

/// Ray-marching geometry shared by crest finding and peak visibility.
struct Sight<'a> {
    dem: &'a Dem,
    col: f64,
    row: f64,
    eye: f64,
    /// Ground meters per sample along x and y at the observer.
    mx: f64,
    my: f64,
}

impl Sight<'_> {
    fn angle_at(&self, bearing: f64, distance: f64) -> Option<f64> {
        let (s, c) = bearing.sin_cos();
        let (x, y) = (
            self.col + s * distance / self.mx,
            self.row - c * distance / self.my,
        );
        if x < 0. || y < 0. || x > (self.dem.width - 1) as f64 || y > (self.dem.height - 1) as f64 {
            return None;
        }
        let drop = distance * distance / (2. * EARTH_R) * (1. - REFRACTION);
        Some(((sample(self.dem, x, y) - self.eye - drop) / distance).atan())
    }
    fn step(&self, distance: f64) -> f64 {
        (self.mx.min(self.my) * 0.5).max(distance * 0.002)
    }
    /// Visible crests along one bearing, nearest first.
    fn crests(&self, bearing: f64, max_d: f64, min_drop: f64) -> Vec<Crest> {
        let mut out = Vec::new();
        let mut best = f64::NEG_INFINITY;
        let mut rising: Option<Crest> = None;
        // Candidate crest and the lowest angle seen behind it.
        let mut candidate: Option<(Crest, f64)> = None;
        let mut d = 100f64.max(self.step(0.));
        while d <= max_d {
            let Some(a) = self.angle_at(bearing, d) else {
                break;
            };
            if a > best {
                if let Some((c, low)) = candidate.take() {
                    if (c.angle - low) * c.distance >= min_drop {
                        out.push(c);
                    }
                }
                best = a;
                rising = Some(Crest {
                    distance: d,
                    angle: a,
                });
            } else if let Some(c) = rising.take() {
                candidate = Some((c, a));
            } else if let Some((_, low)) = candidate.as_mut() {
                *low = low.min(a);
            }
            d += self.step(d);
        }
        // The last crest is the skyline; keep it even if the data ends close behind it.
        if let Some((c, _)) = candidate {
            out.push(c);
        }
        out
    }
}

/// Draw the landscape seen from `view`: every visible ridgeline (heavier when nearer),
/// labeled named summits that are actually in sight, and a compass scale.
pub fn panorama(
    dem: &Dem,
    names: &[Landmark],
    view: &PanoramaView,
    o: &PanoramaOptions,
) -> Result<Drawing, String> {
    let g = dem
        .georef
        .ok_or("A panorama needs a georeferenced elevation source")?;
    if !(1. ..=360.).contains(&view.fov_deg)
        || !view.max_distance_km.is_finite()
        || view.max_distance_km <= 0.
        || view.eye_height_m < 0.
    {
        return Err("Invalid field of view, distance or eye height".into());
    }
    let (col, row) = g
        .latlon_to_sample(view.lat, view.lon)
        .ok_or("Unsupported coordinate system")?;
    if col < 0. || row < 0. || col > (dem.width - 1) as f64 || row > (dem.height - 1) as f64 {
        return Err("The viewpoint is outside the elevation data".into());
    }
    let (mx, my) = g.sample_size_m(row);
    let sight = Sight {
        dem,
        col,
        row,
        eye: sample(dem, col, row) + view.eye_height_m,
        mx,
        my,
    };
    let max_d = view.max_distance_km * 1000.;
    let pano_w = o.sheet_w_mm - 2. * o.margin_mm;
    let columns = (pano_w * 4.).clamp(200., 4000.) as usize;
    let fov = view.fov_deg.to_radians();
    let bearing_of =
        |i: f64| view.bearing_deg.to_radians() + (i / (columns - 1) as f64 - 0.5) * fov;
    let per_column: Vec<Vec<Crest>> = (0..columns)
        .map(|i| sight.crests(bearing_of(i as f64), max_d, o.min_ridge_drop_m))
        .collect();

    // Chain crests at similar distance and angle in neighboring columns into ridgelines.
    let mut chains: Vec<Vec<(usize, Crest)>> = Vec::new();
    let mut open: Vec<usize> = Vec::new(); // chains ending in the previous column
    for (i, crests) in per_column.iter().enumerate() {
        let mut next_open = Vec::new();
        let mut used = vec![false; open.len()];
        for c in crests {
            let best = open
                .iter()
                .enumerate()
                .filter(|(k, _)| !used[*k])
                .map(|(k, &ch)| {
                    let last = chains[ch].last().expect("chains are non-empty").1;
                    let dd = (last.distance - c.distance).abs() / c.distance;
                    let da = (last.angle - c.angle).abs();
                    (k, ch, dd + da * 20.)
                })
                .filter(|(_, ch, _)| {
                    let last = chains[*ch].last().expect("chains are non-empty").1;
                    (last.distance - c.distance).abs() / c.distance < 0.08
                        && (last.angle - c.angle).abs() < 0.6f64.to_radians()
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((k, ch, _)) = best {
                used[k] = true;
                chains[ch].push((i, *c));
                next_open.push(ch);
            } else {
                chains.push(vec![(i, *c)]);
                next_open.push(chains.len() - 1);
            }
        }
        open = next_open;
    }
    chains.retain(|c| c.len() >= 6);

    // Vertical layout: one angular scale for both axes, skyline below the label band.
    let ppr = pano_w / fov;
    let label_band = if o.label_features { 32. } else { 6. };
    let bottom_band = if o.compass { 18. } else { 10. };
    let y_bottom = o.sheet_h_mm - o.margin_mm - bottom_band;
    let (top_angle, low_angle) = chains
        .iter()
        .flatten()
        .map(|(_, c)| c.angle)
        .fold((f64::NEG_INFINITY, f64::INFINITY), |(a, b), v| {
            (a.max(v), b.min(v))
        });
    if !top_angle.is_finite() {
        return Err("No visible ridgelines; check the viewpoint and direction".into());
    }
    // Center labels and ridgelines vertically when they don't fill the sheet.
    let room = y_bottom - o.margin_mm - label_band;
    let slack = (room - (top_angle - low_angle) * ppr).max(0.) * 0.5;
    let label_top = o.margin_mm + slack;
    let y_top = label_top + label_band;
    let x_of = |i: f64| o.margin_mm + i / (columns - 1) as f64 * pano_w;
    let y_of = |a: f64| y_top + (top_angle - a) * ppr;
    let mut d = Drawing::new(o.sheet_w_mm, o.sheet_h_mm);
    d.font = o.font;
    for chain in &chains {
        let mean = chain.iter().map(|(_, c)| c.distance).sum::<f64>() / chain.len() as f64;
        let (name, color, w) = if mean < 5_000. {
            ("ridges-near", BLACK, 0.4)
        } else if mean < 25_000. {
            ("ridges-mid", BLUE, 0.28)
        } else {
            ("ridges-far", GREEN, 0.18)
        };
        // Split where the line would leave the drawing area.
        let mut run: Line = Vec::new();
        for (i, c) in chain {
            let p = (x_of(*i as f64), y_of(c.angle));
            if p.1 <= y_bottom {
                run.push(p);
            } else if run.len() >= 2 {
                d.layer(name, color, w).push(std::mem::take(&mut run));
            } else {
                run.clear();
            }
        }
        if run.len() >= 2 {
            d.layer(name, color, w).push(run);
        }
    }

    if o.label_features {
        label_visible_summits(
            &mut d,
            &sight,
            names,
            view,
            o,
            &g,
            (x_of(0.), pano_w, label_top, y_top),
            &y_of,
        )?;
    }
    if o.compass {
        let y = y_bottom + 4.;
        let lines = d.layer("labels", GREEN, 0.2);
        lines.push(vec![(x_of(0.), y), (x_of((columns - 1) as f64), y)]);
        let start = (view.bearing_deg - view.fov_deg * 0.5).ceil() as i64;
        let end = (view.bearing_deg + view.fov_deg * 0.5).floor() as i64;
        for b in start..=end {
            if b.rem_euclid(5) != 0 {
                continue;
            }
            let x = o.margin_mm + ((b as f64 - view.bearing_deg) / view.fov_deg + 0.5) * pano_w;
            let long = b.rem_euclid(15) == 0;
            d.layer("labels", GREEN, 0.2)
                .push(vec![(x, y), (x, y + if long { 2. } else { 1. })]);
            if b.rem_euclid(45) == 0 {
                let name =
                    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][(b.rem_euclid(360) / 45) as usize];
                d.text("labels", name, x, y + 6., 3., Align::Center);
            } else if long {
                d.text(
                    "labels",
                    &format!("{}°", b.rem_euclid(360)),
                    x,
                    y + 5.5,
                    2.2,
                    Align::Center,
                );
            }
        }
    }
    let footer_y = o.sheet_h_mm - o.margin_mm;
    if !o.title.is_empty() {
        d.text(
            "labels",
            &o.title,
            o.sheet_w_mm * 0.5,
            footer_y - 3.5,
            5.,
            Align::Center,
        );
    }
    let sub = if o.subtitle.is_empty() {
        let hemi =
            |v: f64, p: &str, n: &str| format!("{:.4}°{}", v.abs(), if v < 0. { n } else { p });
        format!(
            "From {} {}, {}, looking {:.0}°",
            hemi(view.lat, "N", "S"),
            hemi(view.lon, "E", "W"),
            elevation_text(sample(dem, col, row), o.feet),
            view.bearing_deg.rem_euclid(360.)
        )
    } else {
        o.subtitle.clone()
    };
    d.text(
        "labels",
        &sub,
        o.sheet_w_mm * 0.5,
        footer_y + 0.5,
        2.6,
        Align::Center,
    );
    if o.cut_outline {
        d.rect("cut", CUT_RED, 0.1, [0., 0., o.sheet_w_mm, o.sheet_h_mm]);
    }
    Ok(d)
}

/// Label named summits and pillars that are in the view and not hidden by nearer terrain,
/// in rows above the skyline with leader lines down to each peak.
#[allow(clippy::too_many_arguments)]
fn label_visible_summits(
    d: &mut Drawing,
    sight: &Sight,
    names: &[Landmark],
    view: &PanoramaView,
    o: &PanoramaOptions,
    g: &crate::georef::GeoReference,
    area: (f64, f64, f64, f64),
    y_of: &dyn Fn(f64) -> f64,
) -> Result<(), String> {
    let (x_left, pano_w, label_top, y_top) = area;
    let mut peaks = Vec::new();
    for n in names
        .iter()
        .filter(|n| matches!(n.symbol.as_str(), "summit" | "pillar" | "peak"))
    {
        let Some((c, r)) = g.latlon_to_sample(n.lat, n.lon) else {
            continue;
        };
        if c < 0. || r < 0. || c > (sight.dem.width - 1) as f64 || r > (sight.dem.height - 1) as f64
        {
            continue;
        }
        let (ex, ny) = ((c - sight.col) * sight.mx, (sight.row - r) * sight.my);
        let dist = ex.hypot(ny);
        if !(200. ..=view.max_distance_km * 1000.).contains(&dist) {
            continue;
        }
        let bearing = ex.atan2(ny).to_degrees();
        let off = (bearing - view.bearing_deg + 540.).rem_euclid(360.) - 180.;
        if off.abs() > view.fov_deg * 0.5 {
            continue;
        }
        let elev = n
            .elevation_m
            .unwrap_or_else(|| local_max(sight.dem, c, r, 2));
        let drop = dist * dist / (2. * EARTH_R) * (1. - REFRACTION);
        let angle = ((elev - sight.eye - drop) / dist).atan();
        // Hidden if any terrain on the way rises above the line of sight to the summit.
        let br = bearing.to_radians();
        let mut t = 100f64;
        let mut hidden = false;
        while t < dist * 0.97 {
            if sight.angle_at(br, t).is_some_and(|a| a > angle + 0.0003) {
                hidden = true;
                break;
            }
            t += sight.step(t);
        }
        if !hidden {
            peaks.push((
                x_left + (off / view.fov_deg + 0.5) * pano_w,
                y_of(angle),
                elev,
                &n.name,
            ));
        }
    }
    peaks.sort_by(|a, b| b.2.total_cmp(&a.2));
    let (size, small, row_h) = (3., 2.3, 7.5);
    let rows = ((y_top - label_top - 4.) / row_h).floor().max(1.) as usize;
    let mut taken: Vec<Vec<(f64, f64)>> = vec![Vec::new(); rows];
    for (x, y, elev, name) in peaks {
        let elev_s = elevation_text(elev, o.feet);
        let w = d.width(name, size).max(d.width(&elev_s, small)) + 2.;
        // Lowest free row (nearest the peak), so leaders stay short.
        let Some(row) = (0..rows).rev().find(|&r| {
            taken[r]
                .iter()
                .all(|&(a, b)| x + w * 0.5 < a || x - w * 0.5 > b)
        }) else {
            continue;
        };
        taken[row].push((x - w * 0.5, x + w * 0.5));
        let base = label_top + 4. + row as f64 * row_h + size;
        d.text("labels", name, x, base, size, Align::Center);
        d.text(
            "labels",
            &elev_s,
            x,
            base + small + 0.8,
            small,
            Align::Center,
        );
        d.layer("labels", GREEN, 0.2)
            .push(vec![(x, base + small + 1.6), (x, y - 1.)]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::georef::{CrsUnits, GeoReference};

    /// A cone-shaped mountain on a 30 m UTM grid, peak at the center.
    fn mountain() -> Dem {
        let n = 201;
        let c = 100.;
        let v = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - c, (i / n) as f64 - c);
                (3000. - 30. * (x * x + y * y).sqrt()).max(500.)
            })
            .collect();
        let mut d = Dem::new(n, n, v).unwrap();
        d.georef = Some(GeoReference {
            epsg: Some(32610),
            units: CrsUnits::Meters,
            origin_x: 591_000.,
            origin_y: 5_192_500.,
            pixel_width: 30.,
            pixel_height: 30.,
        });
        d
    }

    fn summit(d: &Dem, name: &str, col: f64, row: f64) -> Landmark {
        let (lat, lon) = d.georef.unwrap().sample_to_latlon(col, row).unwrap();
        Landmark {
            name: name.into(),
            lat,
            lon,
            elevation_m: None,
            symbol: "summit".into(),
        }
    }

    #[test]
    fn topo_map_has_contours_labels_and_furniture() {
        let d = mountain();
        let names = [summit(&d, "Test Peak", 100., 100.)];
        let o = MapOptions {
            title: "Test Map".into(),
            ..MapOptions::default()
        };
        let map = topo_map(&d, &names, &o).unwrap();
        let layer = |n: &str| map.layers.iter().find(|l| l.name == n);
        assert!(layer("contours").is_some_and(|l| !l.lines.is_empty()));
        assert!(layer("index-contours").is_some_and(|l| !l.lines.is_empty()));
        assert!(layer("frame").is_some());
        // Everything stays on the sheet.
        for p in map.layers.iter().flat_map(|l| l.lines.iter().flatten()) {
            assert!(p.0 >= 0. && p.1 >= 0. && p.0 <= o.sheet_w_mm && p.1 <= o.sheet_h_mm);
        }
        // 2500 m of relief gives an automatic interval of 100 m.
        let svg = map.to_svg();
        assert!(svg.contains("id=\"labels\"") && svg.contains("id=\"index-contours\""));
        assert_eq!(auto_interval(2500.), 100.);
        assert_eq!(thousands(4392.4), "4,392");
        assert!(topo_map(
            &d,
            &[],
            &MapOptions {
                sheet_w_mm: 20.,
                ..o
            }
        )
        .is_err());
    }

    #[test]
    fn stylized_map_uses_icons_rose_and_fonts() {
        let d = mountain();
        let names = [summit(&d, "Test Peak", 100., 100.)];
        let o = MapOptions {
            style: MapStyle::Stylized,
            font: Font::Serif,
            title: "Test Map".into(),
            ..MapOptions::default()
        };
        let map = topo_map(&d, &names, &o).unwrap();
        let layer = |n: &str| map.layers.iter().find(|l| l.name == n);
        assert!(layer("icons").is_some_and(|l| !l.lines.is_empty()));
        // Double frame, and no coordinate ticks.
        assert_eq!(layer("frame").unwrap().lines.len(), 2);
        // Fewer contours than the exact style (interval doubled).
        let exact = topo_map(&d, &names, &MapOptions::default()).unwrap();
        let count = |m: &Drawing| {
            m.layers
                .iter()
                .filter(|l| l.name.ends_with("contours"))
                .map(|l| l.lines.len())
                .sum::<usize>()
        };
        assert!(count(&map) < count(&exact));
        assert_eq!(map.font, Font::Serif);
        // An explicit library icon is drawn in the exact style too.
        let mut catalog = summit(&d, "Lookout", 60., 60.);
        catalog.symbol = "icon:maki:volcano".into();
        let exact = topo_map(&d, &[catalog], &MapOptions::default()).unwrap();
        assert!(exact
            .layers
            .iter()
            .any(|l| l.name == "icons" && !l.lines.is_empty()));
        for p in map.layers.iter().flat_map(|l| l.lines.iter().flatten()) {
            assert!(p.0 >= 0. && p.1 >= 0. && p.0 <= o.sheet_w_mm && p.1 <= o.sheet_h_mm);
        }
    }
    #[test]
    fn panorama_sees_the_mountain_and_labels_its_summit() {
        let d = mountain();
        // Stand 2.7 km south of the peak, looking north.
        let (lat, lon) = d.georef.unwrap().sample_to_latlon(100., 190.).unwrap();
        let view = PanoramaView {
            lat,
            lon,
            bearing_deg: 0.,
            fov_deg: 60.,
            eye_height_m: 1.7,
            max_distance_km: 20.,
        };
        let names = [
            summit(&d, "Test Peak", 100., 100.),
            summit(&d, "Behind Me", 100., 199.),
        ];
        let pano = panorama(&d, &names, &view, &PanoramaOptions::default()).unwrap();
        let ridges: Vec<&Layer> = pano
            .layers
            .iter()
            .filter(|l| l.name.starts_with("ridges"))
            .collect();
        assert!(!ridges.is_empty());
        // The skyline peaks in the middle of the view, where the summit is.
        let skyline = ridges.iter().flat_map(|l| l.lines.iter().flatten());
        let top = skyline.fold(
            (0., f64::INFINITY),
            |a: (f64, f64), p| if p.1 < a.1 { *p } else { a },
        );
        assert!((top.0 - 250.).abs() < 5., "skyline top at x = {}", top.0);
        // The summit in view is labeled; the one behind the observer adds nothing.
        let strokes = |n: &[Landmark]| {
            let p = panorama(&d, n, &view, &PanoramaOptions::default()).unwrap();
            p.layers
                .iter()
                .find(|l| l.name == "labels")
                .map_or(0, |l| l.lines.len())
        };
        assert!(strokes(&names[..1]) > strokes(&[]));
        assert_eq!(strokes(&names), strokes(&names[..1]));
        assert!(panorama(
            &d,
            &[],
            &PanoramaView { lat: 0., ..view },
            &PanoramaOptions::default()
        )
        .is_err());
    }
}
