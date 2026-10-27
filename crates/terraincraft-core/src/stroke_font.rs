//! Single-stroke vector text for engraving, using Hershey fonts (assets/fonts/hershey;
//! see its NOTICE). Lasers and CNC routers engrave these strokes directly, so outputs
//! need no installed fonts and no text-to-path conversion.
use std::sync::OnceLock;

/// Strokes per printable ASCII character (32..=127), in font units with y down, already
/// shifted so the glyph starts at x = 0, plus its advance width.
struct Glyph {
    strokes: Vec<Vec<(f64, f64)>>,
    advance: f64,
}

/// Cap height in font units (capitals run from -12 to the baseline at 9).
const CAP: f64 = 21.;
const BASELINE: f64 = 9.;

/// Engraving typefaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Font {
    /// Hershey Roman Simplex: clean single-line sans.
    #[default]
    Sans,
    /// Hershey Roman Duplex: sans with doubled strokes, reads heavier.
    SansBold,
    /// Hershey Times Roman: serif, for a survey or vintage look.
    Serif,
    /// Hershey Script Simplex: handwritten.
    Script,
}

impl Font {
    pub const ALL: [Font; 4] = [Font::Sans, Font::SansBold, Font::Serif, Font::Script];
    pub fn name(self) -> &'static str {
        match self {
            Font::Sans => "Sans",
            Font::SansBold => "Sans bold",
            Font::Serif => "Serif",
            Font::Script => "Script",
        }
    }
}

fn glyphs(font: Font) -> &'static Vec<Glyph> {
    static FONTS: [OnceLock<Vec<Glyph>>; 4] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let (slot, data) = match font {
        Font::Sans => (0, include_str!("../../../assets/fonts/hershey/futural.jhf")),
        Font::SansBold => (1, include_str!("../../../assets/fonts/hershey/futuram.jhf")),
        Font::Serif => (2, include_str!("../../../assets/fonts/hershey/timesr.jhf")),
        Font::Script => (3, include_str!("../../../assets/fonts/hershey/scripts.jhf")),
    };
    FONTS[slot].get_or_init(|| {
        data.lines()
            .filter(|l| l.len() >= 10)
            .map(|line| {
                let b = line.as_bytes();
                let coord = |i: usize| b[i] as f64 - b'R' as f64;
                let (left, right) = (coord(8), coord(9));
                let mut strokes = vec![Vec::new()];
                let mut i = 10;
                while i + 1 < b.len() {
                    if b[i] == b' ' && b[i + 1] == b'R' {
                        strokes.push(Vec::new());
                    } else {
                        let p = (coord(i) - left, coord(i + 1));
                        strokes.last_mut().expect("starts non-empty").push(p);
                    }
                    i += 2;
                }
                strokes.retain(|s| s.len() >= 2);
                Glyph {
                    strokes,
                    advance: right - left,
                }
            })
            .collect()
    })
}

/// The degree sign, which the ASCII font lacks: a small circle at cap height.
fn degree() -> &'static Glyph {
    static DEGREE: OnceLock<Glyph> = OnceLock::new();
    DEGREE.get_or_init(|| Glyph {
        strokes: vec![(0..=12)
            .map(|i| {
                let a = i as f64 / 12. * std::f64::consts::TAU;
                (3. + 2. * a.cos(), -10. + 2. * a.sin())
            })
            .collect()],
        advance: 7.,
    })
}

fn glyph(font: Font, c: char) -> &'static Glyph {
    if c == '°' {
        return degree();
    }
    let g = glyphs(font);
    let i = (c as u32).wrapping_sub(32) as usize;
    // Unsupported characters render as '?'.
    g.get(i).unwrap_or(&g[('?' as usize) - 32])
}

/// Horizontal alignment of text relative to its anchor point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Width of `text` in millimeters at cap height `size_mm`, in the default font.
pub fn width(text: &str, size_mm: f64) -> f64 {
    width_in(Font::Sans, text, size_mm)
}

/// Width of `text` in `font` at cap height `size_mm`.
pub fn width_in(font: Font, text: &str, size_mm: f64) -> f64 {
    text.chars().map(|c| glyph(font, c).advance).sum::<f64>() * size_mm / CAP
}

/// Strokes for `text` with its baseline through (x, y), y down, rotated `angle_deg`
/// clockwise about that anchor. Capitals are `size_mm` tall.
pub fn text(
    text: &str,
    x: f64,
    y: f64,
    size_mm: f64,
    align: Align,
    angle_deg: f64,
) -> Vec<Vec<(f64, f64)>> {
    text_in(Font::Sans, text, x, y, size_mm, align, angle_deg)
}

/// `text` in a chosen font.
pub fn text_in(
    font: Font,
    text: &str,
    x: f64,
    y: f64,
    size_mm: f64,
    align: Align,
    angle_deg: f64,
) -> Vec<Vec<(f64, f64)>> {
    let k = size_mm / CAP;
    let w = width_in(font, text, size_mm);
    let start = match align {
        Align::Left => 0.,
        Align::Center => -w * 0.5,
        Align::Right => -w,
    };
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let mut out = Vec::new();
    let mut pen = start;
    for c in text.chars() {
        let g = glyph(font, c);
        for s in &g.strokes {
            out.push(
                s.iter()
                    .map(|&(gx, gy)| {
                        let (u, v) = (pen + gx * k, (gy - BASELINE) * k);
                        (x + u * cos - v * sin, y + u * sin + v * cos)
                    })
                    .collect(),
            );
        }
        pen += g.advance * k;
    }
    out
}

/// SVG path data for strokes.
pub fn path_data(strokes: &[Vec<(f64, f64)>]) -> String {
    use std::fmt::Write;
    let mut d = String::new();
    for s in strokes {
        for (i, (x, y)) in s.iter().enumerate() {
            write!(d, "{}{x:.3} {y:.3} ", if i == 0 { "M" } else { "L" }).unwrap();
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitals_have_the_requested_height_on_the_baseline() {
        let s = text("H", 10., 50., 7., Align::Left, 0.);
        let ys: Vec<f64> = s.iter().flatten().map(|p| p.1).collect();
        let (top, bottom) = ys
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| {
                (a.min(*y), b.max(*y))
            });
        assert!((bottom - 50.).abs() < 1e-9, "baseline {bottom}");
        assert!((bottom - top - 7.).abs() < 1e-9, "height {}", bottom - top);
        // "H" is two verticals and a bar.
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn every_font_has_capitals_on_the_same_baseline() {
        for font in Font::ALL {
            let s = text_in(font, "HMW", 0., 40., 10., Align::Left, 0.);
            let ys: Vec<f64> = s.iter().flatten().map(|p| p.1).collect();
            let bottom = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            assert!((bottom - 40.).abs() < 0.6, "{font:?} baseline {bottom}");
            assert!(width_in(font, "HMW", 10.) > 15., "{font:?}");
        }
    }
    #[test]
    fn alignment_rotation_and_fallback() {
        let w = width("Rainier", 5.);
        assert!(w > 15. && w < 30., "{w}");
        let centered = text("Rainier", 100., 0., 5., Align::Center, 0.);
        let xs = centered.iter().flatten().map(|p| p.0);
        let (lo, hi) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
            (a.min(x), b.max(x))
        });
        assert!((lo + hi) * 0.5 - 100. < 1.5);
        // A quarter turn clockwise makes text run downward.
        let down = text("HHHH", 0., 0., 5., Align::Left, 90.);
        let ys: Vec<f64> = down.iter().flatten().map(|p| p.1).collect();
        let reach = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!((reach - width("HHHH", 5.)).abs() < 1.5, "{reach}");
        assert_eq!(
            text("é", 0., 0., 5., Align::Left, 0.).len(),
            text("?", 0., 0., 5., Align::Left, 0.).len()
        );
        assert!(path_data(&centered).starts_with('M'));
        assert!(width("46°", 5.) > width("46", 5.));
        assert_eq!(text("°", 0., 0., 5., Align::Left, 0.).len(), 1);
    }
}
