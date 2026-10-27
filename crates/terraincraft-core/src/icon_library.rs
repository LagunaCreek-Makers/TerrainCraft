//! Bundled black-and-white engraving icons: a curated subset of Tabler Icons (MIT) and
//! Maki (CC0), with per-set provenance. `tools/build_icon_library.py` regenerates the
//! assets, `assets/icon_library/catalog.json` and the embedded SVG table.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// An upstream icon set and the license it is bundled under.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IconSet {
    pub name: String,
    pub title: String,
    pub version: String,
    pub license: String,
    /// Path of the license text, relative to `assets/icon_library`.
    pub license_file: String,
    pub source: String,
    pub package: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryIcon {
    /// `set:name`, e.g. `tabler:mountain`.
    pub id: String,
    pub title: String,
    pub set: String,
    pub category: String,
    pub tags: Vec<String>,
    /// How a landmark icon was made: `dem-ridgeline`, `osm-footprint` or `hand-drawn`.
    #[serde(default)]
    pub method: String,
    /// Per-icon license, when it differs from its set's.
    #[serde(default)]
    pub license: String,
    /// Credit line the license requires, if any.
    #[serde(default)]
    pub attribution: String,
    /// Where the subject's facts and geometry came from.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Review state of TerrainCraft's own drawings: `pending` or `approved`.
    #[serde(default)]
    pub review: String,
}

#[derive(Deserialize)]
struct Catalog {
    sets: Vec<IconSet>,
    icons: Vec<LibraryIcon>,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/icon_library/catalog.json"))
            .expect("bundled icon catalog")
    })
}

/// The icon used when nothing else is selected.
pub const DEFAULT_ICON: &str = "tabler:mountain";

pub fn sets() -> &'static [IconSet] {
    &catalog().sets
}

pub fn all() -> &'static [LibraryIcon] {
    &catalog().icons
}

pub fn get(id: &str) -> Option<&'static LibraryIcon> {
    all().iter().find(|i| i.id == id)
}

/// The set an icon comes from, for attribution.
pub fn set_of(icon: &LibraryIcon) -> Option<&'static IconSet> {
    sets().iter().find(|s| s.name == icon.set)
}

/// Categories in catalog order, without repeats.
pub fn categories() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for i in all() {
        if !out.contains(&i.category.as_str()) {
            out.push(&i.category);
        }
    }
    out
}

/// Icons whose name or tags contain every word of `query`, optionally in one category.
pub fn search(query: &str, category: Option<&str>) -> Vec<&'static LibraryIcon> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    all()
        .iter()
        .filter(|i| category.is_none_or(|c| i.category == c))
        .filter(|i| {
            words.iter().all(|w| {
                i.title.to_lowercase().contains(w.as_str())
                    || i.tags.iter().any(|t| t.to_lowercase().contains(w.as_str()))
            })
        })
        .collect()
}

/// The SVG source of a library icon.
pub fn svg(id: &str) -> Option<&'static str> {
    crate::icon_library_data::SVGS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, s)| *s)
}

/// Library icon for an icon reference: a library id, or one of the retired
/// `generic-NNN` procedural ids, which map to the closest library icon by subject.
pub fn resolve(reference: &str) -> Option<&'static str> {
    if let Some(i) = get(reference.trim()) {
        return Some(&i.id);
    }
    let n = reference
        .trim()
        .strip_prefix("generic-")?
        .parse::<usize>()
        .ok()?;
    if !(1..=240).contains(&n) {
        return None;
    }
    // The old generator cycled through these 20 subjects.
    const SUBJECTS: [&str; 20] = [
        "tabler:mountain",
        "tabler:building-cottage",
        "tabler:building-monument",
        "tabler:building-bridge",
        "tabler:compass",
        "tabler:building-church",
        "maki:museum",
        "tabler:mountain",
        "tabler:tree",
        "tabler:globe",
        "maki:town-hall",
        "maki:observation-tower",
        "maki:water",
        "tabler:building-lighthouse",
        "tabler:trekking",
        "tabler:buildings",
        "tabler:ripple",
        "tabler:building-castle",
        "maki:mountain",
        "tabler:building",
    ];
    Some(SUBJECTS[(n - 1) % 20])
}

/// Insert an icon into an engraving SVG as stroked polylines, scaled into a `size_mm`
/// square centered at (x_mm, y_mm), as `engrave::Drawing::add_icon` places it.
pub fn overlay_icon(
    mut svg: String,
    icon: &str,
    x_mm: f64,
    y_mm: f64,
    size_mm: f64,
) -> Result<String, String> {
    if ![x_mm, y_mm, size_mm].iter().all(|x| x.is_finite()) || size_mm <= 0. || size_mm > 1000. {
        return Err("Invalid icon placement".into());
    }
    let strokes = crate::svg_lines::parse(icon)?;
    let mut group = String::from(
        "<g id=\"terraincraft-engraving-icon\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.2\">",
    );
    for line in strokes.placed(x_mm, y_mm, size_mm) {
        let points: Vec<String> = line
            .iter()
            .map(|p| format!("{:.3},{:.3}", p.0, p.1))
            .collect();
        group.push_str(&format!("<polyline points=\"{}\"/>", points.join(" ")));
    }
    group.push_str("</g>\n");
    let pos = svg.rfind("</svg>").ok_or("Missing SVG closing tag")?;
    svg.insert_str(pos, &group);
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_and_files_agree() {
        let table = crate::icon_library_data::SVGS;
        assert_eq!(all().len(), table.len());
        assert!(all().len() > 500);
        for i in all() {
            let s = svg(&i.id).unwrap_or_else(|| panic!("{} has no SVG", i.id));
            assert!(s.contains("<svg") && !s.contains("<image") && !s.contains("<script"));
            assert!(set_of(i).is_some(), "{} has no license record", i.id);
        }
        for s in sets() {
            assert!(!s.license.is_empty() && !s.version.is_empty());
            let license = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/icon_library")
                .join(&s.license_file);
            assert!(license.is_file(), "missing {}", license.display());
        }
    }
    #[test]
    fn icons_are_engravable_without_bounding_boxes() {
        for i in all() {
            let s = crate::svg_lines::parse(svg(&i.id).unwrap()).unwrap();
            assert!(!s.lines.is_empty(), "{}", i.id);
            // Tabler's invisible 24x24 frame must not survive as a square around the icon.
            let full_frame = s.lines.iter().any(|l| {
                l.len() == 5
                    && l.iter()
                        .all(|p| (p.0 == 0. || p.0 == 24.) && (p.1 == 0. || p.1 == 24.))
            });
            assert!(!full_frame, "{} engraves its bounding box", i.id);
        }
    }
    #[test]
    fn landmark_icons_record_provenance() {
        let landmarks: Vec<_> = all().iter().filter(|i| i.set == "landmarks").collect();
        assert!(landmarks.len() >= 20);
        for i in landmarks {
            assert!(!i.sources.is_empty(), "{} has no sources", i.id);
            assert!(
                ["dem-ridgeline", "osm-footprint", "hand-drawn"].contains(&i.method.as_str()),
                "{}",
                i.id
            );
            assert!(
                ["pending", "approved"].contains(&i.review.as_str()),
                "{}",
                i.id
            );
            if i.method == "osm-footprint" {
                assert!(i.license.starts_with("ODbL") && i.attribution.contains("OpenStreetMap"));
            }
        }
    }
    #[test]
    fn utah_catalog_uses_library_icons() {
        let csv = include_str!("../../../assets/landmark_catalog_utah.csv");
        let entries = crate::landmark_catalog::parse_catalog(csv).unwrap();
        assert!((20..=30).contains(&entries.len()));
        for e in &entries {
            assert_eq!(e.jurisdiction, "US-UT");
            assert!(e.source_url.starts_with("https://"), "{}", e.name);
            let id = resolve(&e.icon_id).unwrap_or_else(|| panic!("{} has no icon", e.name));
            assert!(svg(id).is_some());
        }
    }
    #[test]
    fn search_and_legacy_ids() {
        assert!(search("mountain", None)
            .iter()
            .any(|i| i.id == DEFAULT_ICON));
        assert!(search("ski", Some("Points of interest"))
            .iter()
            .all(|i| i.set == "maki"));
        assert_eq!(resolve("generic-001"), Some("tabler:mountain"));
        assert_eq!(resolve("generic-999"), None);
        assert_eq!(resolve("maki:volcano"), Some("maki:volcano"));
        for n in 1..=20 {
            assert!(
                get(resolve(&format!("generic-{n:03}")).unwrap()).is_some(),
                "{n}"
            );
        }
    }
    #[test]
    fn overlays_as_strokes() {
        let s = overlay_icon(
            "<svg></svg>".into(),
            svg(DEFAULT_ICON).unwrap(),
            20.,
            30.,
            12.,
        )
        .unwrap();
        assert!(s.contains("<polyline"));
    }
}
