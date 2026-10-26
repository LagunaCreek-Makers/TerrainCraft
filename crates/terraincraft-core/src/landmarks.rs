//! User-supplied named landmarks and safe SVG vector symbol overlays.
//! No claims of authoritative GNIS names: CSV entries are user-provided.
use crate::geoview::GeographicExtent;
use std::fmt::Write;
#[derive(Debug, Clone)]
pub struct Landmark {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation_m: Option<f64>,
    pub symbol: String,
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
/// Write landmarks in the CSV format `read_csv` reads. Commas in names become spaces.
pub fn to_csv(items: &[Landmark]) -> String {
    let mut out = String::from("name,latitude,longitude,elevation_m,symbol\n");
    for l in items {
        let _ = writeln!(
            out,
            "{},{},{},{},{}",
            l.name.replace(',', " "),
            l.lat,
            l.lon,
            l.elevation_m.map(|e| e.to_string()).unwrap_or_default(),
            l.symbol.replace(',', " ")
        );
    }
    out
}
/// Simple comma-delimited CSV. Names containing commas should be avoided in this initial format.
pub fn read_csv(text: &str) -> Result<Vec<Landmark>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty()
            || line.starts_with('#')
            || n == 0 && line.to_lowercase().starts_with("name,")
        {
            continue;
        }
        let cols: Vec<_> = line.split(',').map(str::trim).collect();
        if cols.len() < 3 || cols.len() > 5 {
            return Err(format!("Landmark line {}: expected 3–5 columns", n + 1));
        }
        let lat: f64 = cols[1]
            .parse()
            .map_err(|_| format!("Invalid latitude on line {}", n + 1))?;
        let lon: f64 = cols[2]
            .parse()
            .map_err(|_| format!("Invalid longitude on line {}", n + 1))?;
        if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90. || lon.abs() > 180. {
            return Err(format!("Invalid coordinates on line {}", n + 1));
        }
        let elevation_m = cols
            .get(3)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|_| format!("Invalid elevation on line {}", n + 1))
            })
            .transpose()?;
        if elevation_m.is_some_and(|v| !v.is_finite()) {
            return Err("Nonfinite elevation".into());
        }
        if cols[0].is_empty() || cols[0].len() > 160 {
            return Err(format!("Invalid name on line {}", n + 1));
        }
        out.push(Landmark {
            name: cols[0].to_owned(),
            lat,
            lon,
            elevation_m,
            symbol: cols.get(4).unwrap_or(&"peak").to_string(),
        });
    }
    Ok(out)
}
fn symbol_svg(x: f64, y: f64, symbol: &str) -> String {
    let mut s = String::new();
    match symbol {
        "tree" => {
            let _=write!(s,"<path d=\"M{x:.2} {:.2} l-3 5 h2 l-3 4 h8 l-3 -4 h2 z\" fill=\"none\" stroke=\"black\" stroke-width=\"0.35\"/>",y-8.);
        }
        "cabin" => {
            let _=write!(s,"<path d=\"M{:.2} {:.2} l5 -4 l5 4 v7 h-10 z\" fill=\"none\" stroke=\"black\" stroke-width=\"0.35\"/>",x-5.,y-3.);
        }
        "camp" => {
            let _=write!(s,"<path d=\"M{:.2} {:.2} l5 -8 l5 8 z M{x:.2} {:.2} v-8\" fill=\"none\" stroke=\"black\" stroke-width=\"0.35\"/>",x-5.,y,y);
        }
        _ => {
            let _=write!(s,"<path d=\"M{x:.2} {:.2} l-3 5 h6 z\" fill=\"none\" stroke=\"black\" stroke-width=\"0.35\"/>",y-5.);
        }
    }
    s
}
fn label(x: f64, y: f64, l: &Landmark) -> String {
    let mut s=format!("<g class=\"landmark\">{}<text x=\"{:.2}\" y=\"{:.2}\" font-family=\"sans-serif\" font-size=\"3.2\" fill=\"black\">{}</text>",symbol_svg(x,y,&l.symbol),x+6.,y,escape(&l.name));
    if let Some(e) = l.elevation_m {
        let _=write!(s,"<text x=\"{:.2}\" y=\"{:.2}\" font-family=\"sans-serif\" font-size=\"2.4\">{:.0} m</text>",x+6.,y+3.2,e);
    }
    s.push_str("</g>");
    s
}
fn insert_svg(mut svg: String, overlay: &str) -> Result<String, String> {
    let pos = svg.rfind("</svg>").ok_or("Invalid SVG document")?;
    svg.insert_str(pos, overlay);
    Ok(svg)
}
pub fn overlay_topdown(
    svg: String,
    features: &[Landmark],
    e: GeographicExtent,
    w: f64,
    h: f64,
) -> Result<String, String> {
    e.validate()?;
    let mut layer = String::from("<g id=\"landmark-overlays\">");
    for l in features {
        if l.lon < e.west || l.lon > e.east || l.lat < e.south || l.lat > e.north {
            continue;
        }
        let x = (l.lon - e.west) / (e.east - e.west) * w;
        let y = (e.north - l.lat) / (e.north - e.south) * h;
        layer.push_str(&label(x, y, l));
    }
    layer.push_str("</g>");
    insert_svg(svg, &layer)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_escapes() {
        let f = read_csv("name,latitude,longitude,elevation_m,symbol\nA&B,38.5,-121.5,1234,tree\n")
            .unwrap();
        assert_eq!(f.len(), 1);
        let svg = overlay_topdown(
            "<svg></svg>".into(),
            &f,
            GeographicExtent {
                west: -122.,
                east: -121.,
                south: 38.,
                north: 39.,
            },
            200.,
            100.,
        )
        .unwrap();
        assert!(svg.contains("A&amp;B"));
        assert!(svg.contains("landmark-overlays"));
    }
    #[test]
    fn rejects_bad_coordinates() {
        assert!(read_csv("peak,95,0").is_err());
    }
}

/// Project user-supplied geographic landmarks onto the first-person engraving.
/// Requires an explicitly georeferenced north-up EPSG:4326 raster. Occlusion is
/// evaluated against the DEM along the observer-to-feature segment.
pub fn overlay_perspective(
    svg: String,
    features: &[Landmark],
    extent: GeographicExtent,
    dem: &crate::Dem,
    v: crate::cartography::Viewpoint,
    width_mm: f64,
    height_mm: f64,
) -> Result<String, String> {
    v.validate(dem)?;
    extent.validate()?;
    if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0. || height_mm <= 50. {
        return Err("Invalid engraving dimensions".into());
    }
    let eye = bilinear(dem, v.x, v.y) + v.eye_height_m;
    let top = 25.;
    let bottom = height_mm - 25.;
    let center = (top + bottom) * 0.5;
    let vfov = v.fov_deg.to_radians() * (height_mm / width_mm);
    let scale = (bottom - top) / vfov.max(0.05);
    let mut layer=String::from("<g id=\"perspective-landmarks\" fill=\"black\" font-family=\"sans-serif\" font-size=\"3.2\">\n");
    let mut occupied: Vec<(f64, f64)> = Vec::new();
    for l in features {
        let Ok((px, py)) = extent.point_to_pixel(l.lat, l.lon, dem.width, dem.height) else {
            continue;
        };
        let dx = (px - v.x) * v.pixel_size_x_m;
        let dy = -(py - v.y) * v.pixel_size_y_m;
        let distance = dx.hypot(dy);
        if distance < 1. || distance > v.max_distance_m {
            continue;
        }
        let bearing = dx.atan2(dy).to_degrees().rem_euclid(360.);
        let offset = (bearing - v.bearing_deg + 540.).rem_euclid(360.) - 180.;
        if offset.abs() > v.fov_deg / 2. {
            continue;
        }
        let terrain = bilinear(dem, px, py);
        // Use DEM height for occlusion; CSV elevations are descriptive metadata,
        // not a trusted replacement for the sampled surface.
        let target_height = terrain;
        let target_angle =
            ((target_height - eye - distance * distance / (2. * 6_371_000.)) / distance).atan();
        let step = (v.pixel_size_x_m.min(v.pixel_size_y_m) * 0.5).max(0.25);
        let count = (distance / step).ceil().min(20000.) as usize;
        let mut blocked = false;
        for i in 1..count {
            let t = i as f64 / count as f64;
            let d = distance * t;
            let sx = v.x + (px - v.x) * t;
            let sy = v.y + (py - v.y) * t;
            let angle = ((bilinear(dem, sx, sy) - eye - d * d / (2. * 6_371_000.)) / d).atan();
            if angle > target_angle + 1e-5 {
                blocked = true;
                break;
            }
        }
        if blocked {
            continue;
        }
        let x = (offset / v.fov_deg + 0.5) * width_mm;
        let y = (center - (target_angle - v.pitch_deg.to_radians()) * scale).clamp(top, bottom);
        if occupied
            .iter()
            .any(|(ox, oy)| (x - ox).abs() < 24. && (y - oy).abs() < 10.)
        {
            continue;
        }
        occupied.push((x, y));
        let label_y = (y - 7.).max(17.);
        let _=write!(layer,"<g class=\"visible-landmark\"><circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"0.8\"/><path d=\"M {x:.2} {y:.2} L {x:.2} {label_y:.2}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.12\"/><text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>",label_y-1.,escape(&l.name));
        if let Some(e) = l.elevation_m {
            let _=write!(layer,"<text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"2.5\">{e:.0} m (supplied)</text>",label_y+3.);
        }
        layer.push_str("</g>\n");
    }
    layer.push_str("</g>\n");
    insert_svg(svg, &layer)
}
fn bilinear(d: &crate::Dem, x: f64, y: f64) -> f64 {
    let x = x.clamp(0., (d.width - 1) as f64);
    let y = y.clamp(0., (d.height - 1) as f64);
    let ix = x.floor() as usize;
    let iy = y.floor() as usize;
    let jx = (ix + 1).min(d.width - 1);
    let jy = (iy + 1).min(d.height - 1);
    let a = x - ix as f64;
    let b = y - iy as f64;
    d.at(ix, iy) * (1. - a) * (1. - b)
        + d.at(jx, iy) * a * (1. - b)
        + d.at(ix, jy) * (1. - a) * b
        + d.at(jx, jy) * a * b
}
#[cfg(test)]
mod perspective_tests {
    use super::*;
    #[test]
    fn front_landmark_visible_and_behind_hidden() {
        let d = crate::Dem::new(11, 11, vec![100.; 121]).unwrap();
        let e = GeographicExtent {
            west: 0.,
            east: 1.,
            south: 0.,
            north: 1.,
        };
        let v = crate::cartography::Viewpoint {
            x: 5.,
            y: 8.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 2.,
            max_distance_m: 10000.,
            pixel_size_x_m: 100.,
            pixel_size_y_m: 100.,
        };
        let f = vec![
            Landmark {
                name: "Front & peak".into(),
                lat: 0.6,
                lon: 0.5,
                elevation_m: Some(100.),
                symbol: "peak".into(),
            },
            Landmark {
                name: "Behind".into(),
                lat: 0.1,
                lon: 0.5,
                elevation_m: None,
                symbol: "peak".into(),
            },
        ];
        let svg = overlay_perspective("<svg></svg>".into(), &f, e, &d, v, 400., 200.).unwrap();
        assert!(svg.contains("Front &amp; peak"));
        assert!(!svg.contains("Behind"));
    }
    #[test]
    fn ridge_occludes_feature() {
        let mut z = vec![100.; 121];
        for x in 0..11 {
            z[5 * 11 + x] = 1000.;
        }
        let d = crate::Dem::new(11, 11, z).unwrap();
        let e = GeographicExtent {
            west: 0.,
            east: 1.,
            south: 0.,
            north: 1.,
        };
        let v = crate::cartography::Viewpoint {
            x: 5.,
            y: 9.,
            bearing_deg: 0.,
            pitch_deg: 0.,
            fov_deg: 90.,
            eye_height_m: 2.,
            max_distance_m: 10000.,
            pixel_size_x_m: 100.,
            pixel_size_y_m: 100.,
        };
        let f = vec![Landmark {
            name: "Hidden".into(),
            lat: 0.9,
            lon: 0.5,
            elevation_m: None,
            symbol: "peak".into(),
        }];
        let svg = overlay_perspective("<svg></svg>".into(), &f, e, &d, v, 400., 200.).unwrap();
        assert!(!svg.contains("Hidden"));
    }
}

/// Import point features from GeoJSON FeatureCollection. Recognizes common
/// GNIS-style property names without asserting the dataset is authoritative.
/// Only WGS84 lon/lat Point geometry is accepted; invalid points are skipped.
pub fn read_geojson(input: &str) -> Result<Vec<Landmark>, String> {
    let root: serde_json::Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    if root.get("type").and_then(|v| v.as_str()) != Some("FeatureCollection") {
        return Err("Expected a GeoJSON FeatureCollection".into());
    }
    let features = root
        .get("features")
        .and_then(|v| v.as_array())
        .ok_or("Missing features array")?;
    let mut result = Vec::new();
    for feature in features {
        let Some(geom) = feature.get("geometry") else {
            continue;
        };
        if geom.get("type").and_then(|v| v.as_str()) != Some("Point") {
            continue;
        }
        let Some(coords) = geom.get("coordinates").and_then(|v| v.as_array()) else {
            continue;
        };
        if coords.len() < 2 {
            continue;
        }
        let (Some(lon), Some(lat)) = (coords[0].as_f64(), coords[1].as_f64()) else {
            continue;
        };
        if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90. || lon.abs() > 180. {
            continue;
        }
        let props = &feature["properties"];
        let name = [
            "feature_name",
            "FEATURE_NAME",
            "name",
            "Name",
            "official_name",
        ]
        .iter()
        .find_map(|key| props.get(*key).and_then(|v| v.as_str()))
        .unwrap_or("Unnamed feature");
        if name.len() > 160 {
            continue;
        }
        let elevation_m = ["elevation_m", "elev_m", "ELEV_IN_M"]
            .iter()
            .find_map(|key| props.get(*key).and_then(|v| v.as_f64()))
            .filter(|v| v.is_finite());
        let feature_class = ["feature_class", "FEATURE_CLASS", "class"]
            .iter()
            .find_map(|key| props.get(*key).and_then(|v| v.as_str()))
            .unwrap_or("Summit");
        let symbol = match feature_class.to_ascii_lowercase().as_str() {
            "summit" | "peak" => "peak",
            "populated place" | "building" => "cabin",
            "locale" | "campground" => "camp",
            "woods" | "forest" => "tree",
            _ => "peak",
        };
        result.push(Landmark {
            name: name.to_owned(),
            lat,
            lon,
            elevation_m,
            symbol: symbol.into(),
        });
    }
    Ok(result)
}

/// Detect CSV or GeoJSON from the file content rather than relying on extensions.
pub fn read_landmarks(input: &str) -> Result<Vec<Landmark>, String> {
    if input.trim_start().starts_with('{') {
        read_geojson(input)
    } else {
        read_csv(input)
    }
}

#[cfg(test)]
mod geojson_tests {
    use super::*;
    #[test]
    fn reads_gnis_like_geojson() {
        let src = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[-120.2,38.5]},"properties":{"feature_name":"Test Summit","feature_class":"Summit","elevation_m":2500}},{"type":"Feature","geometry":{"type":"LineString","coordinates":[]},"properties":{}}]}"#;
        let v = read_landmarks(src).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "Test Summit");
        assert_eq!(v[0].elevation_m, Some(2500.));
    }
    #[test]
    fn rejects_wrong_geojson_root() {
        assert!(read_geojson("{\"type\":\"Point\"}").is_err());
    }
}
