//! Editable, persistent engraving annotations; independent of laser control.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub text: String,
    /// Position in normalized page coordinates, 0..1.
    pub x: f64,
    pub y: f64,
    pub size_mm: f64,
    pub rotation_deg: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Preset {
    Survey,
    Minimal,
    Vintage,
    Cartoon,
}
impl Preset {
    pub fn font(self) -> &'static str {
        match self {
            Self::Survey => "sans-serif",
            Self::Minimal => "sans-serif",
            Self::Vintage => "serif",
            Self::Cartoon => "cursive",
        }
    }
    pub fn stroke(self) -> &'static str {
        match self {
            Self::Survey => "#333333",
            Self::Minimal => "#111111",
            Self::Vintage => "#624a32",
            Self::Cartoon => "#333333",
        }
    }
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
/// Adds a separately editable SVG group immediately before the closing svg tag.
pub fn overlay(
    svg: String,
    labels: &[Annotation],
    width_mm: f64,
    height_mm: f64,
    preset: Preset,
) -> Result<String, String> {
    if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0. || height_mm <= 0. {
        return Err("Invalid page dimensions".into());
    }
    let mut output = String::from("<g id=\"custom-annotations\" data-editor=\"terraincraft\">\n");
    for (index, a) in labels.iter().enumerate() {
        if !a.x.is_finite()
            || !a.y.is_finite()
            || !(0.0..=1.0).contains(&a.x)
            || !(0.0..=1.0).contains(&a.y)
            || !a.size_mm.is_finite()
            || a.size_mm <= 0.
            || a.size_mm > 100.
            || !a.rotation_deg.is_finite()
        {
            return Err(format!("Invalid annotation {index}"));
        }
        let x = a.x * width_mm;
        let y = a.y * height_mm;
        output.push_str(&format!("<text id=\"annotation-{index}\" x=\"{x:.3}\" y=\"{y:.3}\" font-family=\"{}\" font-size=\"{:.3}\" fill=\"{}\" transform=\"rotate({:.3} {x:.3} {y:.3})\">{}</text>\n",preset.font(),a.size_mm,preset.stroke(),a.rotation_deg,escape(&a.text)));
    }
    output.push_str("</g>\n");
    let mut svg = svg;
    let pos = svg.rfind("</svg>").ok_or("SVG closing tag missing")?;
    svg.insert_str(pos, &output);
    Ok(svg)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_and_positions() {
        let a = Annotation {
            text: "A&B <peak>".into(),
            x: 0.5,
            y: 0.25,
            size_mm: 4.,
            rotation_deg: 0.,
        };
        let s = overlay("<svg></svg>".into(), &[a], 200., 100., Preset::Survey).unwrap();
        assert!(s.contains("x=\"100.000\""));
        assert!(s.contains("A&amp;B &lt;peak&gt;"));
    }
    #[test]
    fn rejects_bad_positions() {
        let a = Annotation {
            text: "bad".into(),
            x: 2.,
            y: 0.,
            size_mm: 4.,
            rotation_deg: 0.,
        };
        assert!(overlay("<svg></svg>".into(), &[a], 100., 100., Preset::Minimal).is_err());
    }
}
