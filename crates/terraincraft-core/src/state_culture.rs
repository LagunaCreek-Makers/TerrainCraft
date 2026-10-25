//! US state cultural icon catalog. Motifs are original, illustrative, and unofficial.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CulturalIcon {
    pub id: String,
    pub title: String,
    pub category: String,
    pub jurisdiction: String,
    pub icon_id: String,
    pub svg_file: String,
    pub license: String,
    pub source: String,
    pub official: bool,
}
pub fn all() -> Vec<CulturalIcon> {
    serde_json::from_str(include_str!("../../../assets/state_cultural_icons.json"))
        .expect("bundled catalog JSON")
}
pub fn for_state(state: &str) -> Vec<CulturalIcon> {
    let code = if state.len() == 2 {
        format!("US-{}", state.to_ascii_uppercase())
    } else {
        state.to_ascii_uppercase()
    };
    all()
        .into_iter()
        .filter(|i| i.jurisdiction == code)
        .collect()
}
pub fn svg(id: &str) -> Option<String> {
    let icon = all().into_iter().find(|i| i.id == id)?;
    let custom = match id {
        "us-ut-seagull" => Some(include_str!("../../../assets/cultural_icons/seagull.svg")),
        "us-ut-covered-wagon" => Some(include_str!(
            "../../../assets/cultural_icons/covered-wagon.svg"
        )),
        "us-ut-salt-shaker" => Some(include_str!(
            "../../../assets/cultural_icons/salt-shaker.svg"
        )),
        "us-ut-salt-lake-temple" => Some(include_str!(
            "../../../assets/cultural_icons/salt-lake-temple.svg"
        )),
        "us-ut-utah-u-inspired" => Some(include_str!(
            "../../../assets/cultural_icons/utah-u-inspired.svg"
        )),
        "us-ut-byu-inspired" => Some(include_str!(
            "../../../assets/cultural_icons/byu-inspired.svg"
        )),
        _ => None,
    };
    if let Some(s) = custom {
        return Some(s.to_owned());
    }
    if let Some(s) = match icon.icon_id.as_str() {
        "sport-football" => Some(include_str!("../../../assets/sports_icons/football.svg")),
        "sport-skiing" => Some(include_str!("../../../assets/sports_icons/skiing.svg")),
        "sport-whitewater-rafting" => Some(include_str!(
            "../../../assets/sports_icons/whitewater-rafting.svg"
        )),
        "sport-fishing" => Some(include_str!("../../../assets/sports_icons/fishing.svg")),
        "sport-duck-hunting" => Some(include_str!(
            "../../../assets/sports_icons/duck-hunting.svg"
        )),
        "sport-hunting" => Some(include_str!("../../../assets/sports_icons/hunting.svg")),
        "featured-powder-skiing" => Some(include_str!(
            "../../../assets/sports_icons/powder-skiing.svg"
        )),
        "featured-redrock-mtb" => {
            Some(include_str!("../../../assets/sports_icons/redrock-mtb.svg"))
        }
        "featured-fly-fishing" => {
            Some(include_str!("../../../assets/sports_icons/fly-fishing.svg"))
        }
        "featured-whitewater-kayak" => Some(include_str!(
            "../../../assets/sports_icons/whitewater-kayak.svg"
        )),
        "featured-surfing" => Some(include_str!("../../../assets/sports_icons/surfing.svg")),
        "featured-salmon-fishing" => Some(include_str!(
            "../../../assets/sports_icons/salmon-fishing.svg"
        )),
        "featured-ice-hockey" => Some(include_str!("../../../assets/sports_icons/ice-hockey.svg")),
        "featured-rodeo" => Some(include_str!("../../../assets/sports_icons/rodeo.svg")),
        "featured-horse-racing" => Some(include_str!(
            "../../../assets/sports_icons/horse-racing.svg"
        )),
        "featured-basketball" => Some(include_str!("../../../assets/sports_icons/basketball.svg")),
        "featured-baseball" => Some(include_str!("../../../assets/sports_icons/baseball.svg")),
        "featured-sailing" => Some(include_str!("../../../assets/sports_icons/sailing.svg")),
        "featured-rock-climbing" => Some(include_str!(
            "../../../assets/sports_icons/rock-climbing.svg"
        )),
        "featured-golf" => Some(include_str!("../../../assets/sports_icons/golf.svg")),
        "featured-snowboarding" => Some(include_str!(
            "../../../assets/sports_icons/snowboarding.svg"
        )),
        _ => None,
    } {
        return Some(s.to_owned());
    }
    let index = icon
        .icon_id
        .strip_prefix("generic-")?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    Some(crate::icon_library::procedural_icon(index))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_state_has_six_motifs() {
        assert_eq!(
            all()
                .iter()
                .filter(|i| i.jurisdiction.starts_with("US-"))
                .map(|i| i.jurisdiction.clone())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            50
        );
        for s in ["UT", "CA", "NY", "AK"] {
            assert!(for_state(s).len() >= 6)
        }
    }
    #[test]
    fn sports_cover_all_states() {
        for st in all()
            .iter()
            .map(|i| i.jurisdiction.clone())
            .collect::<std::collections::HashSet<_>>()
        {
            assert_eq!(
                for_state(&st)
                    .iter()
                    .filter(|i| i.category == "sports")
                    .count(),
                6
            );
        }
    }
    #[test]
    fn every_state_has_featured_sport() {
        for st in all()
            .iter()
            .map(|i| i.jurisdiction.clone())
            .collect::<std::collections::HashSet<_>>()
        {
            assert_eq!(
                for_state(&st)
                    .iter()
                    .filter(|i| i.category == "sports-featured")
                    .count(),
                1
            );
        }
    }
    #[test]
    fn all_assets_resolve() {
        for item in all() {
            let s = svg(&item.id).unwrap();
            assert!(s.contains("<svg"));
            assert!(!s.contains("<image"));
        }
    }
}
