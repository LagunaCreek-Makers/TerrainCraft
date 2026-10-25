//! Options controlling 3D-print tile export. Shared by the CLI and desktop app and
//! serializable so it can be stored in settings or project files.
use serde::{Deserialize, Serialize};

/// Alignment features cut into each exported terrain tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorMode {
    #[default]
    None,
    /// Blind registration sockets in the tile underside.
    Underside,
    /// Horizontal dowel sockets in the shared sidewalls between tiles.
    SeamSockets,
}

impl std::str::FromStr for ConnectorMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "underside" => Ok(Self::Underside),
            "seam-sockets" => Ok(Self::SeamSockets),
            _ => Err(format!(
                "Unknown connector mode '{s}'; expected none, underside or seam-sockets"
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrintExportSettings {
    /// Solid base thickness under the terrain surface, in millimeters.
    pub base_mm: f64,
    pub connectors: ConnectorMode,
    /// Also export seam dowel drilling guides, manifest and a printable dowel.
    pub seam_dowel_guides: bool,
    /// Attempt to correct inconsistent triangle winding before export.
    pub repair_winding: bool,
    /// Refuse to write any tile STL if a tile fails manufacturing preflight.
    pub strict_mesh: bool,
    /// Run the triangle self-intersection audit on every tile.
    pub check_intersections: bool,
}

impl Default for PrintExportSettings {
    fn default() -> Self {
        Self {
            base_mm: 3.,
            connectors: ConnectorMode::None,
            seam_dowel_guides: false,
            repair_winding: false,
            strict_mesh: false,
            check_intersections: false,
        }
    }
}

impl PrintExportSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !self.base_mm.is_finite() || self.base_mm <= 0. {
            return Err("Base thickness must be a positive number of millimeters".into());
        }
        Ok(())
    }
    pub fn from_json(text: &str) -> Result<Self, String> {
        let s: Self = serde_json::from_str(text).map_err(|e| format!("Invalid settings: {e}"))?;
        s.validate()?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_json_uses_defaults() {
        let s = PrintExportSettings::from_json(r#"{"base_mm":10,"connectors":"seam-sockets"}"#)
            .unwrap();
        assert_eq!(s.base_mm, 10.);
        assert_eq!(s.connectors, ConnectorMode::SeamSockets);
        assert!(!s.strict_mesh);
    }
    #[test]
    fn rejects_unknown_fields_and_bad_base() {
        assert!(PrintExportSettings::from_json(r#"{"strict":true}"#).is_err());
        assert!(PrintExportSettings::from_json(r#"{"base_mm":0}"#).is_err());
    }
    #[test]
    fn parses_connector_names() {
        assert_eq!("underside".parse(), Ok(ConnectorMode::Underside));
        assert!("both".parse::<ConnectorMode>().is_err());
    }
}
