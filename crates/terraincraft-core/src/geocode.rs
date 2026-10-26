//! Place search: US names from the USGS Geographic Names Information System (public
//! domain, via The National Map), and worldwide names from OpenStreetMap's Nominatim
//! (used within its policy: one request per user search, identified, no autocomplete;
//! results are © OpenStreetMap contributors, ODbL).
use serde_json::Value;

/// One search hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// Kind and region, e.g. "Summit, WA" or an OpenStreetMap address.
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
    pub source: &'static str,
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn check(query: &str) -> Result<&str, String> {
    let q = query.trim();
    if q.chars().count() < 2 || q.chars().count() > 120 {
        return Err("Search for 2 to 120 characters".into());
    }
    Ok(q)
}

/// USGS names service: populated places, landforms and water features containing `query`.
pub fn gnis_url(query: &str) -> Result<String, String> {
    Ok(format!(
        "https://carto.nationalmap.gov/arcgis/rest/services/geonames/MapServer/find?searchText={}&contains=true&layers=3,5,7&searchFields=gaz_name&returnGeometry=true&sr=4326&f=json",
        encode(check(query)?)
    ))
}

/// OpenStreetMap Nominatim search for `query` anywhere.
pub fn nominatim_url(query: &str) -> Result<String, String> {
    Ok(format!(
        "https://nominatim.openstreetmap.org/search?q={}&format=jsonv2&limit=8",
        encode(check(query)?)
    ))
}

pub fn parse_gnis(json: &str) -> Result<Vec<Place>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("USGS names: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(format!("USGS names error: {e}"));
    }
    let mut out = Vec::new();
    for r in v["results"]
        .as_array()
        .ok_or("USGS names: no results list")?
    {
        let a = &r["attributes"];
        let p = &r["geometry"]["points"][0];
        let (Some(lon), Some(lat)) = (p[0].as_f64(), p[1].as_f64()) else {
            continue;
        };
        let name = a["gaz_name"].as_str().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        let class = a["gaz_featureclass"].as_str().unwrap_or("");
        let state = a["state_alpha"].as_str().unwrap_or("");
        out.push(Place {
            name: name.into(),
            detail: [class, state]
                .iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            lat,
            lon,
            source: "USGS",
        });
    }
    Ok(out)
}

pub fn parse_nominatim(json: &str) -> Result<Vec<Place>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("OpenStreetMap search: {e}"))?;
    let mut out = Vec::new();
    for r in v
        .as_array()
        .ok_or("OpenStreetMap search: unexpected response")?
    {
        let (Some(lat), Some(lon)) = (
            r["lat"].as_str().and_then(|s| s.parse::<f64>().ok()),
            r["lon"].as_str().and_then(|s| s.parse::<f64>().ok()),
        ) else {
            continue;
        };
        let display = r["display_name"].as_str().unwrap_or("");
        let name = r["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| display.split(',').next().unwrap_or(""))
            .trim();
        if name.is_empty() {
            continue;
        }
        // Drop the leading name from the address so the detail adds information.
        let detail = display
            .strip_prefix(name)
            .map(|s| s.trim_start_matches([',', ' ']))
            .unwrap_or(display);
        out.push(Place {
            name: name.into(),
            detail: detail.into(),
            lat,
            lon,
            source: "OpenStreetMap",
        });
    }
    Ok(out)
}

/// Order results: exact name matches first, then names starting with the query, then the
/// rest; within each, mountains and other landforms before towns. Duplicates (same name
/// within about 1 km) are dropped.
pub fn rank(query: &str, mut places: Vec<Place>) -> Vec<Place> {
    let q = query.trim().to_lowercase();
    let score = |p: &Place| {
        let n = p.name.to_lowercase();
        let name = if n == q {
            0
        } else if n.starts_with(&q) {
            1
        } else {
            2
        };
        let d = p.detail.to_lowercase();
        let kind = if ["summit", "peak", "mountain", "ridge", "range", "volcano"]
            .iter()
            .any(|k| d.contains(k))
        {
            0
        } else {
            1
        };
        (name, kind)
    };
    places.sort_by_key(score);
    let mut out: Vec<Place> = Vec::new();
    for p in places {
        if !out.iter().any(|o| {
            o.name.eq_ignore_ascii_case(&p.name)
                && (o.lat - p.lat).abs() < 0.01
                && (o.lon - p.lon).abs() < 0.01
        }) {
            out.push(p);
        }
    }
    out
}

/// Search both services and merge the results. A failing service is reported only if
/// both fail.
#[cfg(feature = "download")]
pub fn search(query: &str) -> Result<Vec<Place>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!(
            "TerrainCraft/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/LagunaCreek-Makers/TerrainCraft)"
        ))
        .build()
        .map_err(|e| e.to_string())?;
    let get = |url: String| -> Result<String, String> {
        client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| e.to_string())
    };
    let usgs = get(gnis_url(query)?).and_then(|t| parse_gnis(&t));
    let osm = get(nominatim_url(query)?).and_then(|t| parse_nominatim(&t));
    match (usgs, osm) {
        (Err(a), Err(b)) => Err(format!("Search failed: USGS: {a}; OpenStreetMap: {b}")),
        (a, b) => Ok(rank(
            query,
            a.unwrap_or_default()
                .into_iter()
                .chain(b.unwrap_or_default())
                .collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_encoded_and_bounded() {
        let u = gnis_url("Mount St. Helens").unwrap();
        assert!(u.contains("searchText=Mount+St.+Helens&contains=true"));
        assert!(nominatim_url("Zürich & co")
            .unwrap()
            .contains("q=Z%C3%BCrich+%26+co&"));
        assert!(gnis_url(" ").is_err());
    }

    #[test]
    fn parses_both_services_and_ranks_exact_landforms_first() {
        let gnis = r#"{"results":[
            {"layerName":"Populated Places","attributes":{"gaz_name":"Rainier","gaz_featureclass":"Populated Place","state_alpha":"OR"},
             "geometry":{"points":[[-122.9359,46.0890]]}},
            {"layerName":"Landforms","attributes":{"gaz_name":"Mount Rainier","gaz_featureclass":"Summit","state_alpha":"WA"},
             "geometry":{"points":[[-121.7603,46.8523]]}}]}"#;
        let osm = r#"[{"name":"Mount Rainier","display_name":"Mount Rainier, Pierce County, Washington, United States",
             "lat":"46.8529","lon":"-121.7604","category":"natural","type":"volcano"},
            {"name":"","display_name":"Rainier Avenue, Seattle","lat":"47.5","lon":"-122.3"}]"#;
        let g = parse_gnis(gnis).unwrap();
        let o = parse_nominatim(osm).unwrap();
        assert_eq!(g[1].detail, "Summit, WA");
        assert_eq!(o[0].detail, "Pierce County, Washington, United States");
        assert_eq!(o[1].name, "Rainier Avenue");
        let ranked = rank("mount rainier", g.into_iter().chain(o).collect());
        // The USGS summit and the OSM volcano are the same place; one is kept, first.
        assert_eq!(ranked[0].name, "Mount Rainier");
        assert_eq!(
            ranked.iter().filter(|p| p.name == "Mount Rainier").count(),
            1
        );
        assert!(parse_gnis(r#"{"error":{"code":400}}"#).is_err());
        assert!(parse_nominatim("{}").is_err());
    }

    /// Live check against both services: `cargo test -p terraincraft-core --features
    /// download -- --ignored geocode`.
    #[cfg(feature = "download")]
    #[test]
    #[ignore = "needs network"]
    fn live_search() {
        let r = search("Mount Rainier").unwrap();
        assert!(r[0].name == "Mount Rainier", "{r:?}");
        assert!((r[0].lat - 46.85).abs() < 0.05 && (r[0].lon + 121.76).abs() < 0.05);
        let m = search("Matterhorn").unwrap();
        assert!(m
            .iter()
            .any(|p| (p.lat - 45.976).abs() < 0.01 && (p.lon - 7.658).abs() < 0.01));
    }
}
