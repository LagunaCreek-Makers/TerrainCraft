//! Alternate views of icons that have separately drawn artwork. Only the Salt Lake Temple
//! has them: front, side and corner schematics, not camera transformations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconStyle {
    Original,
    TempleFront,
    TempleSide,
    TempleCorner,
}
impl IconStyle {
    /// Indices match the desktop's saved projects, where 1-3 were retired line weights.
    pub fn from_index(i: usize) -> Self {
        match i {
            4 => Self::TempleFront,
            5 => Self::TempleSide,
            6 => Self::TempleCorner,
            _ => Self::Original,
        }
    }
}
/// The artwork for `style`: a drawn temple view when one is selected for the Salt Lake
/// Temple, otherwise the original.
pub fn apply(original: &str, style: IconStyle, is_salt_lake_temple: bool) -> String {
    if !is_salt_lake_temple {
        return original.to_owned();
    }
    match style {
        IconStyle::TempleFront => {
            include_str!("../../../assets/cultural_icons/salt-lake-temple__front.svg")
        }
        IconStyle::TempleSide => {
            include_str!("../../../assets/cultural_icons/salt-lake-temple__side.svg")
        }
        IconStyle::TempleCorner => {
            include_str!("../../../assets/cultural_icons/salt-lake-temple__corner.svg")
        }
        IconStyle::Original => original,
    }
    .to_owned()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_temple_views() {
        let original = crate::state_culture::svg("us-ut-salt-lake-temple").unwrap();
        let a = apply(&original, IconStyle::TempleFront, true);
        let b = apply(&original, IconStyle::TempleCorner, true);
        assert_ne!(a, b);
        assert!(a.contains("<g "));
    }
    #[test]
    fn other_icons_are_unchanged() {
        let a = crate::icon_library::svg("tabler:mountain").unwrap();
        assert_eq!(apply(a, IconStyle::TempleFront, false), a);
    }
}
