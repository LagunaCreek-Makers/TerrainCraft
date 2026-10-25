//! Deterministic engraving variants. Alternate perspective requires original view artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconStyle {
    Original,
    Simplified,
    FineOutline,
    Bold,
    TempleFront,
    TempleSide,
    TempleCorner,
}
impl IconStyle {
    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Self::Simplified,
            2 => Self::FineOutline,
            3 => Self::Bold,
            4 => Self::TempleFront,
            5 => Self::TempleSide,
            6 => Self::TempleCorner,
            _ => Self::Original,
        }
    }
}
/// Apply the selected engraving treatment to the same original artwork.
/// The three temple perspectives are separately drawn schematic views, not camera transformations.
pub fn apply(original: &str, style: IconStyle, is_salt_lake_temple: bool) -> String {
    if is_salt_lake_temple {
        let view = match style {
            IconStyle::TempleFront => Some(include_str!(
                "../../../assets/icon_variants/salt-lake-temple__front.svg"
            )),
            IconStyle::TempleSide => Some(include_str!(
                "../../../assets/icon_variants/salt-lake-temple__side.svg"
            )),
            IconStyle::TempleCorner => Some(include_str!(
                "../../../assets/icon_variants/salt-lake-temple__corner.svg"
            )),
            _ => None,
        };
        if let Some(s) = view {
            return s.to_owned();
        }
    }
    let weight = match style {
        IconStyle::Simplified => "3.8",
        IconStyle::FineOutline => "1.2",
        IconStyle::Bold => "5.0",
        _ => return original.to_owned(),
    };
    // Modify the primary vector group's stroke only; no rasterization.
    if let Some(start) = original.find("<g ") {
        if let Some(close) = original[start..].find('>') {
            let end = start + close + 1;
            let mut tag = original[start..end].to_string();
            if let Some(pos) = tag.find("stroke-width=") {
                let q = tag.as_bytes()[pos + 13] as char;
                if q == '\'' || q == '"' {
                    let value_start = pos + 14;
                    if let Some(offset) = tag[value_start..].find(q) {
                        tag.replace_range(value_start..value_start + offset, weight);
                    }
                }
            } else {
                tag.insert_str(tag.len() - 1, &format!(" stroke-width=\"{}\"", weight));
            }
            let mut output = format!("{}{}{}", &original[..start], tag, &original[end..]);
            if style == IconStyle::Simplified {
                // Keep original recognizable geometry; use heavier line treatment rather than invent a silhouette.
                output = output.replace("stroke-width='2.4'", "stroke-width='3.8'");
            }
            return output;
        }
    }
    original.to_owned()
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
    fn generic_styles_keep_vectors() {
        let a = crate::icon_library::procedural_icon(2);
        let b = apply(&a, IconStyle::Bold, false);
        assert!(b.contains("stroke-width='5.0'"));
    }
}
