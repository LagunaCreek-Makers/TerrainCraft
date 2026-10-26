//! Printable A4 assembly sheet: the tile layout north-up with each tile's number, a north
//! arrow, a scale bar, and the model and map details.
use crate::georef::CrsUnits;
use crate::{north_up_origin, tile_label, Dem, TerrainSettings, Tile};
use std::fmt::Write as _;

const PAGE_W: f64 = 210.;
const PAGE_H: f64 = 297.;
const MARGIN: f64 = 15.;
const DRAW: f64 = 180.;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A round length near `target` (1, 2 or 5 times a power of ten).
fn nice(target: f64) -> f64 {
    let p = 10f64.powf(target.log10().floor());
    [1., 2., 5., 10.]
        .into_iter()
        .map(|m| m * p)
        .rev()
        .find(|v| *v <= target)
        .unwrap_or(p)
}

pub fn svg(dem: &Dem, s: TerrainSettings, tiles: &[Tile], title: &str) -> Result<String, String> {
    s.validate()?;
    if tiles.is_empty() {
        return Err("No tiles for the assembly sheet".into());
    }
    let (w, h) = (s.width_mm, s.height_mm);
    let k = (DRAW / w).min(DRAW / h);
    let left = MARGIN + (DRAW - w * k) * 0.5;
    let top = 32.;
    let dx = s.width_mm / (dem.width - 1) as f64;
    let dy = s.height_mm / (dem.height - 1) as f64;
    let mut out = String::new();
    writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{PAGE_W}mm" height="{PAGE_H}mm" viewBox="0 0 {PAGE_W} {PAGE_H}" font-family="sans-serif">"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<rect width="{PAGE_W}" height="{PAGE_H}" fill="white"/>"#
    )
    .unwrap();
    writeln!(
        out,
        r#"<text x="{MARGIN}" y="20" font-size="7" font-weight="bold">{}</text>"#,
        escape(title)
    )
    .unwrap();
    writeln!(
        out,
        r#"<text x="{MARGIN}" y="26" font-size="3.5">Assembly layout, seen from above with north up</text>"#
    )
    .unwrap();
    let font = (DRAW / 12.).min(
        tiles
            .iter()
            .map(|t| ((t.x1 - t.x0) as f64 * dx).min((t.y1 - t.y0) as f64 * dy) * k * 0.3)
            .fold(f64::INFINITY, f64::min),
    );
    for t in tiles {
        let [ox, oy] = north_up_origin(dem, s, *t);
        let (tw, th) = ((t.x1 - t.x0) as f64 * dx, (t.y1 - t.y0) as f64 * dy);
        let (x, y) = (left + ox * k, top + (h - oy - th) * k);
        let shade = if (t.row + t.col) % 2 == 0 {
            "#e8efe6"
        } else {
            "#d5e2e8"
        };
        writeln!(
            out,
            r##"<rect x="{x:.3}" y="{y:.3}" width="{:.3}" height="{:.3}" fill="{shade}" stroke="#333" stroke-width="0.4"/>"##,
            tw * k,
            th * k
        )
        .unwrap();
        let (cx, cy) = (x + tw * k * 0.5, y + th * k * 0.5);
        writeln!(
            out,
            r#"<text x="{cx:.3}" y="{cy:.3}" font-size="{font:.2}" text-anchor="middle" dominant-baseline="middle" font-weight="bold">{}</text>"#,
            tile_label(*t)
        )
        .unwrap();
        writeln!(
            out,
            r##"<text x="{cx:.3}" y="{:.3}" font-size="{:.2}" text-anchor="middle" fill="#555">{tw:.0} x {th:.0} mm</text>"##,
            cy + font * 0.9,
            (font * 0.35).max(2.)
        )
        .unwrap();
    }
    // North arrow beside the layout.
    let (ax, ay) = (PAGE_W - MARGIN - 6., top + 4.);
    writeln!(
        out,
        r#"<path d="M {ax} {ay} l 4 10 l -4 -3 l -4 3 z" fill="black"/><text x="{ax}" y="{}" font-size="5" text-anchor="middle" font-weight="bold">N</text>"#,
        ay + 16.
    )
    .unwrap();
    // Scale bar in model millimeters.
    let bar = nice(w / 4.);
    let by = top + h * k + 10.;
    writeln!(
        out,
        r#"<path d="M {left:.3} {by} h {:.3}" stroke="black" stroke-width="1"/><text x="{left:.3}" y="{}" font-size="3.5">{bar} mm on the model</text>"#,
        bar * k,
        by + 6.
    )
    .unwrap();
    let mut info = vec![
        format!("Model {w:.1} x {h:.1} mm, {} tiles", tiles.len()),
        format!(
            "Base {:.1} mm, relief {:.1} mm (exaggeration {:.2})",
            s.base_mm,
            s.relief_mm * s.vertical_exaggeration,
            s.vertical_exaggeration
        ),
    ];
    if let Some(g) = dem.georef {
        let (gx, _) = g.sample_span_m(dem.width, dem.height);
        let [w0, s0, e0, n0] = g.bounds(dem.width, dem.height);
        info.push(format!("Map scale about 1:{:.0}", gx * 1000. / w));
        info.push(match g.units {
            CrsUnits::Degrees => format!("Area {s0:.4} to {n0:.4} N, {w0:.4} to {e0:.4} E"),
            CrsUnits::Meters => format!(
                "Area EPSG:{} x {w0:.0} to {e0:.0} m, y {s0:.0} to {n0:.0} m",
                g.epsg.unwrap_or(0)
            ),
        });
    }
    info.push("Each tile's number is engraved underneath, mirrored so it reads".into());
    info.push("correctly when the tile is turned over.".into());
    for (i, line) in info.iter().enumerate() {
        writeln!(
            out,
            r#"<text x="{MARGIN}" y="{:.1}" font-size="3.8">{}</text>"#,
            by + 16. + i as f64 * 6.,
            escape(line)
        )
        .unwrap();
    }
    out.push_str("</svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles;

    #[test]
    fn lays_out_every_tile_north_up() {
        let d = Dem::new(41, 21, vec![1.; 41 * 21]).unwrap();
        let s = TerrainSettings {
            width_mm: 400.,
            height_mm: 200.,
            base_mm: 3.,
            vertical_exaggeration: 1.,
            relief_mm: 10.,
        };
        let ts = tiles(&d, s, 220., 220., 5.).unwrap();
        let svg = svg(&d, s, &ts, "Test & sheet").unwrap();
        assert_eq!(svg.matches("<rect x=").count(), ts.len());
        for t in &ts {
            assert!(svg.contains(&format!(">{}</text>", tile_label(*t))));
        }
        assert!(svg.contains("Test &amp; sheet"));
        // Row 1 (north) is drawn above row 2.
        let y_of = |label: &str| {
            let i = svg.find(&format!(">{label}</text>")).unwrap();
            let head = &svg[..i];
            let y = head.rfind(" y=\"").unwrap() + 4;
            head[y..].split('"').next().unwrap().parse::<f64>().unwrap()
        };
        if ts.iter().any(|t| t.row == 1) {
            assert!(y_of("1-1") < y_of("2-1"));
        }
        assert_eq!(nice(37.), 20.);
        assert_eq!(nice(100.), 100.);
    }
}
