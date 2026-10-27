//! Turn simple SVG line art (the icon library) into engravable polylines.
//!
//! Supports what the icons use: `path` (all commands, curves and arcs flattened),
//! `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon` and `text` (drawn with the
//! stroke font), nested `g`/element `transform`s (translate, scale, rotate, skew, matrix),
//! and
//! the `viewBox`. Fills are not engraved; every shape becomes its outline.
use crate::stroke_font::{self, Align};

pub type Line = Vec<(f64, f64)>;

/// An icon's outlines in its own viewBox coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Strokes {
    pub lines: Vec<Line>,
    /// viewBox: min x, min y, width, height.
    pub view: [f64; 4],
}

/// Affine transform [a b c d e f]: x' = a x + c y + e, y' = b x + d y + f.
type Matrix = [f64; 6];
const IDENTITY: Matrix = [1., 0., 0., 1., 0., 0.];

fn mul(m: Matrix, n: Matrix) -> Matrix {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

fn apply(m: Matrix, (x, y): (f64, f64)) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        if b[i] == b'-' || b[i] == b'+' {
            i += 1;
        }
        let mut dot = false;
        while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b'.' && !dot)) {
            dot |= b[i] == b'.';
            i += 1;
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            i += 1;
            if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                i += 1;
            }
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i > start {
            if let Ok(v) = s[start..i].parse::<f64>() {
                out.push(v);
                continue;
            }
        }
        i = start + 1;
    }
    out
}

fn parse_transform(t: &str) -> Result<Matrix, String> {
    let mut m = IDENTITY;
    for part in t.split(')') {
        let Some((name, args)) = part.split_once('(') else {
            continue;
        };
        let a = numbers(args);
        let get = |i: usize, d: f64| a.get(i).copied().unwrap_or(d);
        let n = match name.trim().trim_start_matches(',').trim() {
            "translate" => [1., 0., 0., 1., get(0, 0.), get(1, 0.)],
            "scale" => [get(0, 1.), 0., 0., get(1, get(0, 1.)), 0., 0.],
            "rotate" => {
                let (s, c) = get(0, 0.).to_radians().sin_cos();
                let (cx, cy) = (get(1, 0.), get(2, 0.));
                mul(
                    mul([1., 0., 0., 1., cx, cy], [c, s, -s, c, 0., 0.]),
                    [1., 0., 0., 1., -cx, -cy],
                )
            }
            "matrix" if a.len() == 6 => [a[0], a[1], a[2], a[3], a[4], a[5]],
            "skewX" => [1., 0., get(0, 0.).to_radians().tan(), 1., 0., 0.],
            "skewY" => [1., get(0, 0.).to_radians().tan(), 0., 1., 0., 0.],
            other => return Err(format!("Unsupported transform {other}")),
        };
        m = mul(m, n);
    }
    Ok(m)
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = &rest[i + name.len()..];
        if before.is_some_and(|c| c.is_whitespace()) && after.trim_start().starts_with('=') {
            let after = after.trim_start()[1..].trim_start();
            let q = after.chars().next()?;
            if q == '"' || q == '\'' {
                let body = &after[1..];
                return body.find(q).map(|end| &body[..end]);
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

fn num(tag: &str, name: &str) -> f64 {
    attr(tag, name)
        .and_then(|v| v.trim().trim_end_matches("px").parse().ok())
        .unwrap_or(0.)
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Line {
    (0..=48)
        .map(|i| {
            let a = i as f64 / 48. * std::f64::consts::TAU;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Flatten SVG path data into polylines.
fn path(d: &str) -> Result<Vec<Line>, String> {
    // Split into (command, numbers) groups.
    let mut groups: Vec<(char, Vec<f64>)> = Vec::new();
    let mut last = 0;
    let idx: Vec<(usize, char)> = d
        .char_indices()
        .filter(|(_, c)| c.is_ascii_alphabetic() && *c != 'e' && *c != 'E')
        .collect();
    for (k, &(i, c)) in idx.iter().enumerate() {
        let end = idx.get(k + 1).map_or(d.len(), |n| n.0);
        groups.push((c, numbers(&d[i + 1..end])));
        last = end;
    }
    if last == 0 && !d.trim().is_empty() {
        return Err("Path data has no commands".into());
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut cur = (0., 0.);
    let mut start = (0., 0.);
    let mut prev_ctrl: Option<(char, (f64, f64))> = None;
    const STEPS: usize = 16;
    for (cmd, args) in groups {
        let rel = cmd.is_ascii_lowercase();
        let up = cmd.to_ascii_uppercase();
        let base = |c: (f64, f64)| if rel { c } else { (0., 0.) };
        let arity = match up {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            'Z' => 0,
            other => return Err(format!("Unsupported path command {other}")),
        };
        if up == 'Z' {
            if let Some(l) = lines.last_mut() {
                l.push(start);
            }
            cur = start;
            prev_ctrl = None;
            continue;
        }
        if args.is_empty() || args.len() % arity != 0 {
            return Err(format!("Path command {cmd} has {} numbers", args.len()));
        }
        for (n, a) in args.chunks(arity).enumerate() {
            let b = base(cur);
            match up {
                'M' if n == 0 => {
                    cur = (b.0 + a[0], b.1 + a[1]);
                    start = cur;
                    lines.push(vec![cur]);
                    prev_ctrl = None;
                    continue;
                }
                // Further coordinate pairs after M are implicit line-tos.
                'M' | 'L' => cur = (b.0 + a[0], b.1 + a[1]),
                'H' => cur = (if rel { cur.0 + a[0] } else { a[0] }, cur.1),
                'V' => cur = (cur.0, if rel { cur.1 + a[0] } else { a[0] }),
                'C' | 'S' | 'Q' | 'T' => {
                    let p0 = cur;
                    let reflect = |kinds: &[char]| match prev_ctrl {
                        Some((k, c)) if kinds.contains(&k) => (2. * p0.0 - c.0, 2. * p0.1 - c.1),
                        _ => p0,
                    };
                    let pts: Vec<(f64, f64)> = match up {
                        'C' => vec![
                            (b.0 + a[0], b.1 + a[1]),
                            (b.0 + a[2], b.1 + a[3]),
                            (b.0 + a[4], b.1 + a[5]),
                        ],
                        'S' => vec![
                            reflect(&['C', 'S']),
                            (b.0 + a[0], b.1 + a[1]),
                            (b.0 + a[2], b.1 + a[3]),
                        ],
                        'Q' => vec![(b.0 + a[0], b.1 + a[1]), (b.0 + a[2], b.1 + a[3])],
                        _ => vec![reflect(&['Q', 'T']), (b.0 + a[0], b.1 + a[1])],
                    };
                    let line = lines.last_mut().ok_or("Path draws before moving")?;
                    for s in 1..=STEPS {
                        let t = s as f64 / STEPS as f64;
                        let u = 1. - t;
                        line.push(if pts.len() == 3 {
                            let w = [u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t];
                            (
                                w[0] * p0.0 + w[1] * pts[0].0 + w[2] * pts[1].0 + w[3] * pts[2].0,
                                w[0] * p0.1 + w[1] * pts[0].1 + w[2] * pts[1].1 + w[3] * pts[2].1,
                            )
                        } else {
                            let w = [u * u, 2. * u * t, t * t];
                            (
                                w[0] * p0.0 + w[1] * pts[0].0 + w[2] * pts[1].0,
                                w[0] * p0.1 + w[1] * pts[0].1 + w[2] * pts[1].1,
                            )
                        });
                    }
                    prev_ctrl = Some((up, pts[pts.len() - 2]));
                    cur = *pts.last().expect("non-empty");
                    continue;
                }
                'A' => {
                    let end = (b.0 + a[5], b.1 + a[6]);
                    let line = lines.last_mut().ok_or("Path draws before moving")?;
                    line.extend(arc(cur, a[0], a[1], a[2], a[3] != 0., a[4] != 0., end));
                    cur = end;
                    prev_ctrl = None;
                    continue;
                }
                _ => unreachable!(),
            }
            lines
                .last_mut()
                .ok_or("Path draws before moving")?
                .push(cur);
            prev_ctrl = None;
        }
    }
    Ok(lines)
}

/// Points along an SVG elliptical arc (endpoint form, SVG spec F.6.5), excluding `p0`.
fn arc(
    p0: (f64, f64),
    rx: f64,
    ry: f64,
    rot_deg: f64,
    large: bool,
    sweep: bool,
    p1: (f64, f64),
) -> Line {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0. || ry == 0. || p0 == p1 {
        return vec![p1];
    }
    let (s, c) = rot_deg.to_radians().sin_cos();
    let (dx, dy) = ((p0.0 - p1.0) / 2., (p0.1 - p1.1) / 2.);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let lambda = (x1 / rx).powi(2) + (y1 / ry).powi(2);
    if lambda > 1. {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (
        c * cx1 - s * cy1 + (p0.0 + p1.0) / 2.,
        s * cx1 + c * cy1 + (p0.1 + p1.1) / 2.,
    );
    let angle = |ux: f64, uy: f64| uy.atan2(ux);
    let t1 = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - t1;
    if sweep && dt < 0. {
        dt += std::f64::consts::TAU;
    } else if !sweep && dt > 0. {
        dt -= std::f64::consts::TAU;
    }
    let steps = ((dt.abs() / std::f64::consts::TAU * 48.).ceil() as usize).max(4);
    (1..=steps)
        .map(|i| {
            let t = t1 + dt * i as f64 / steps as f64;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            (c * x - s * y + cx, s * x + c * y + cy)
        })
        .collect()
}

/// Parse SVG line art into polylines in viewBox coordinates.
pub fn parse(svg: &str) -> Result<Strokes, String> {
    let open = svg.find("<svg").ok_or("Not an SVG")?;
    let head_end = svg[open..].find('>').ok_or("Unterminated svg tag")? + open;
    let head = &svg[open..head_end];
    let view = match attr(head, "viewBox").map(numbers) {
        Some(v) if v.len() == 4 && v[2] > 0. && v[3] > 0. => [v[0], v[1], v[2], v[3]],
        _ => [
            0.,
            0.,
            num(head, "width").max(1.),
            num(head, "height").max(1.),
        ],
    };
    let mut lines = Vec::new();
    let mut stack = vec![IDENTITY];
    let mut rest = &svg[head_end + 1..];
    // Skip content that is not drawn.
    let skip_to = |rest: &str, name: &str| -> usize {
        let close = format!("</{name}>");
        rest.find(&close).map_or(rest.len(), |i| i + close.len())
    };
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt..];
        let gt = rest.find('>').ok_or("Unterminated tag")?;
        let tag = &rest[1..gt];
        let after = &rest[gt + 1..];
        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if tag.starts_with('/') {
            if name == "g" && stack.len() > 1 {
                stack.pop();
            }
            rest = after;
            continue;
        }
        if matches!(
            name,
            "metadata" | "title" | "desc" | "defs" | "style" | "clipPath" | "mask"
        ) && !self_closing
        {
            rest = &after[skip_to(after, name)..];
            continue;
        }
        let own = match attr(tag, "transform") {
            Some(t) => parse_transform(t)?,
            None => IDENTITY,
        };
        let m = mul(*stack.last().expect("non-empty"), own);
        let mut shapes: Vec<Line> = Vec::new();
        match name {
            "g" => {
                if !self_closing {
                    stack.push(m);
                }
            }
            "path" => shapes = path(attr(tag, "d").unwrap_or(""))?,
            "rect" => {
                let (x, y, w, h) = (
                    num(tag, "x"),
                    num(tag, "y"),
                    num(tag, "width"),
                    num(tag, "height"),
                );
                shapes.push(vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)]);
            }
            "circle" => {
                let r = num(tag, "r");
                shapes.push(ellipse(num(tag, "cx"), num(tag, "cy"), r, r));
            }
            "ellipse" => shapes.push(ellipse(
                num(tag, "cx"),
                num(tag, "cy"),
                num(tag, "rx"),
                num(tag, "ry"),
            )),
            "line" => shapes.push(vec![
                (num(tag, "x1"), num(tag, "y1")),
                (num(tag, "x2"), num(tag, "y2")),
            ]),
            "polyline" | "polygon" => {
                let v = numbers(attr(tag, "points").unwrap_or(""));
                let mut l: Line = v.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect();
                if name == "polygon" && !l.is_empty() {
                    l.push(l[0]);
                }
                shapes.push(l);
            }
            "text" if !self_closing => {
                let end = after.find("</text>").ok_or("Unterminated text")?;
                let content: String = after[..end]
                    .split('<')
                    .map(|s| s.split_once('>').map_or(s, |(_, t)| t))
                    .collect();
                let size = attr(tag, "font-size")
                    .and_then(|v| v.trim_end_matches("px").parse().ok())
                    .unwrap_or(10.)
                    * 0.72; // font size to cap height
                let align = match attr(tag, "text-anchor") {
                    Some("middle") => Align::Center,
                    Some("end") => Align::Right,
                    _ => Align::Left,
                };
                let text = content
                    .replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">");
                shapes =
                    stroke_font::text(text.trim(), num(tag, "x"), num(tag, "y"), size, align, 0.);
                rest = &after[end + "</text>".len()..];
                lines.extend(
                    shapes
                        .into_iter()
                        .map(|l| l.into_iter().map(|p| apply(m, p)).collect::<Line>()),
                );
                continue;
            }
            _ => {}
        }
        lines.extend(
            shapes
                .into_iter()
                .filter(|l| l.len() >= 2)
                .map(|l| l.into_iter().map(|p| apply(m, p)).collect::<Line>()),
        );
        rest = after;
    }
    Ok(Strokes { lines, view })
}

impl Strokes {
    /// The icon scaled to fit a `size` x `size` square centered at (x, y).
    pub fn placed(&self, x: f64, y: f64, size: f64) -> Vec<Line> {
        let [vx, vy, vw, vh] = self.view;
        let k = size / vw.max(vh);
        let (ox, oy) = (x - vw * k * 0.5, y - vh * k * 0.5);
        self.lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|&(px, py)| (ox + (px - vx) * k, oy + (py - vy) * k))
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shapes_paths_and_transforms() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">
            <title>t</title><metadata><x>{"not": "drawn"}</x></metadata>
            <g transform="translate(10,5) scale(2)">
              <path d="M0 0 h10 v10 H0 Z m20 0 l5 5"/>
              <rect x="1" y="1" width="2" height="3"/>
            </g>
            <circle cx="50" cy="25" r="10"/>
            <path d="M0 40 Q 10 30 20 40 T 40 40 C 45 35 50 35 55 40 A 5 5 0 0 1 65 40"/>
        </svg>"#;
        let s = parse(svg).unwrap();
        assert_eq!(s.view, [0., 0., 100., 50.]);
        // The square, transformed: (10,5) .. (30,25).
        assert_eq!(
            s.lines[0],
            vec![(10., 5.), (30., 5.), (30., 25.), (10., 25.), (10., 5.)]
        );
        // Relative move after Z starts from the subpath start.
        assert_eq!(s.lines[1], vec![(50., 5.), (60., 15.)]);
        // Circle points sit on the radius.
        assert!(s.lines[3]
            .iter()
            .all(|p| ((p.0 - 50.).hypot(p.1 - 25.) - 10.).abs() < 1e-9));
        // Curves and the arc end where the path says.
        assert_eq!(*s.lines[4].last().unwrap(), (65., 40.));
        let placed = s.placed(0., 0., 10.);
        assert!(placed.iter().flatten().all(|p| p.0.abs() <= 5. + 1e-9));
        assert!(parse("<svg><path d='M0 0 X 1'/></svg>").is_err());
    }

    #[test]
    fn every_library_icon_converts() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let mut stack = vec![root];
        let (mut files, mut empty) = (0, Vec::new());
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "svg") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    let s = parse(&text).unwrap_or_else(|err| panic!("{}: {err}", p.display()));
                    let [vx, vy, vw, vh] = s.view;
                    let slack = vw.max(vh);
                    assert!(
                        s.lines.iter().flatten().all(|q| q.0 > vx - slack
                            && q.0 < vx + vw + slack
                            && q.1 > vy - slack
                            && q.1 < vy + vh + slack),
                        "{} draws far outside its viewBox",
                        p.display()
                    );
                    if s.lines.is_empty() {
                        empty.push(p.display().to_string());
                    }
                    files += 1;
                }
            }
        }
        assert!(files >= 1686, "{files}");
        assert!(empty.is_empty(), "no strokes from {empty:?}");
    }
}
