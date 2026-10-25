//! Original thematic regional artwork. NOT faithful elevations or landmark-specific views.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionalIcon {
    pub id: String,
    pub jurisdiction: String,
    pub name: String,
    pub motif: String,
    pub category: String,
    pub verified_landmark: bool,
    pub variants: std::collections::BTreeMap<String, String>,
}
pub fn all() -> Vec<RegionalIcon> {
    serde_json::from_str(include_str!("../../../assets/regional_icon_catalog.json"))
        .expect("valid bundled regional catalog")
}
pub fn for_jurisdiction(code: &str) -> Vec<RegionalIcon> {
    let code = code.trim().to_ascii_uppercase();
    all()
        .into_iter()
        .filter(|r| r.jurisdiction == code)
        .collect()
}
pub fn resolve_path(root: &Path, id: &str, style: &str) -> Result<PathBuf, String> {
    let icon = all()
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("Unknown regional icon")?;
    let file = icon.variants.get(style).ok_or("Unknown engraving style")?;
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return Err("Unsafe asset filename".into());
    }
    Ok(root.join("assets/regional_icons").join(file))
}
pub fn read_svg(root: &Path, id: &str, style: &str) -> Result<String, String> {
    let path = resolve_path(root, id, style)?;
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if !s.contains("<svg") || s.contains("<script") || s.contains("<image") {
        return Err("Invalid vector artwork".into());
    }
    Ok(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_coverage() {
        let all = all();
        assert_eq!(
            all.iter()
                .filter(|r| r.jurisdiction.starts_with("US-"))
                .count(),
            50
        );
        assert!(
            all.iter()
                .filter(|r| !r.jurisdiction.starts_with("US-"))
                .count()
                >= 60
        );
        assert!(all.iter().all(|r| r.variants.len() == 4));
    }
    #[test]
    fn utah_variants() {
        let r = for_jurisdiction("US-UT");
        assert_eq!(r.len(), 1);
        assert!(r[0].variants.contains_key("simplified"));
    }
}
