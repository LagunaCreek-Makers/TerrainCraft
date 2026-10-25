//! Optional institution/team artwork. Names are discovery metadata, not a grant of trademark rights.
//! Actual logos are supplied by the user as locally licensed monochrome SVG files.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogoRecord {
    pub id: String,
    pub name: String,
    pub jurisdiction: String,
    pub category: String, // college, university, professional-team
    pub sport: String,
    pub svg_file: String,
    pub source_url: String,
    pub license: String,
    pub rights_holder: String,
    pub permission_notes: String,
}

pub fn bundled_examples() -> Vec<LogoRecord> {
    serde_json::from_str(include_str!(
        "../../../assets/institution_logo_catalog.json"
    ))
    .expect("bundled logo discovery catalog")
}

pub fn read_catalog(path: &Path) -> Result<Vec<LogoRecord>, String> {
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let records: Vec<LogoRecord> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    for r in &records {
        if r.id.trim().is_empty() || r.name.trim().is_empty() || r.jurisdiction.trim().is_empty() {
            return Err("Logo entries require id, name, and jurisdiction".into());
        }
        if !seen.insert(&r.id) {
            return Err(format!("Duplicate logo id: {}", r.id));
        }
    }
    Ok(records)
}

pub fn filter<'a>(
    records: &'a [LogoRecord],
    jurisdiction: &str,
    search: &str,
) -> Vec<&'a LogoRecord> {
    let q = search.trim().to_lowercase();
    let j = jurisdiction.trim().to_ascii_uppercase();
    records
        .iter()
        .filter(|r| {
            (j.is_empty() || r.jurisdiction.to_ascii_uppercase() == j)
                && (q.is_empty()
                    || format!("{} {} {}", r.name, r.sport, r.category)
                        .to_lowercase()
                        .contains(&q))
        })
        .collect()
}

/// Only local files underneath the catalog directory can be used. No remote SVG fetching.
/// SVG active content and external references are rejected; rendering is still experimental.
pub fn load_svg(record: &LogoRecord, catalog_dir: &Path) -> Result<String, String> {
    if record.svg_file.trim().is_empty() {
        return Err("No artwork supplied for this institution/team".into());
    }
    if record.license.trim().is_empty() || record.permission_notes.trim().is_empty() {
        return Err("Logo requires recorded license and permission notes".into());
    }
    let rel = Path::new(&record.svg_file);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Logo path must be a relative filename within the catalog directory".into());
    }
    let base = fs::canonicalize(catalog_dir).map_err(|e| e.to_string())?;
    let file: PathBuf = fs::canonicalize(base.join(rel)).map_err(|e| e.to_string())?;
    if !file.starts_with(&base) {
        return Err("Logo path escapes the catalog directory".into());
    }
    let metadata = fs::metadata(&file).map_err(|e| e.to_string())?;
    if metadata.len() > 512_000 {
        return Err("SVG exceeds 500 KiB size limit".into());
    }
    let svg = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let lower = svg.to_ascii_lowercase();
    if !lower.contains("<svg") || !lower.contains("</svg>") {
        return Err("Not an SVG document".into());
    }
    for prohibited in [
        "<script",
        "<foreignobject",
        "<image",
        "<use",
        "<style",
        "<iframe",
        "<object",
        "<!doctype",
        "<!entity",
        "<?xml-stylesheet",
        "onload=",
        "onclick=",
        "onerror=",
        "href=",
        "url(",
        "@import",
        "<animate",
        "<set",
        "<filter",
    ] {
        if lower.contains(prohibited) {
            return Err(format!("Unsupported or unsafe SVG content: {prohibited}"));
        }
    }
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn examples_have_unique_ids() {
        let records = bundled_examples();
        let mut ids = std::collections::HashSet::new();
        for r in records {
            assert!(ids.insert(r.id));
        }
    }
    #[test]
    fn metadata_only_is_not_a_logo() {
        let r = &bundled_examples()[0];
        assert!(load_svg(r, Path::new("assets")).is_err());
    }
    #[test]
    fn jurisdiction_filter() {
        let records = bundled_examples();
        assert!(filter(&records, "US-UT", "Utah")
            .iter()
            .all(|r| r.jurisdiction == "US-UT"));
    }
}
