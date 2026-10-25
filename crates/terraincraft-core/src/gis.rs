//! Pure Rust ESRI ASCII grid import. Coordinates are retained for GIS workflows.
use crate::Dem;
#[derive(Clone, Debug)]
pub struct GeoGrid {
    pub dem: Dem,
    pub xll: f64,
    pub yll: f64,
    pub cell_size: f64,
    pub nodata: Option<f64>,
    /// Number of nodata cells replaced by `nodata::fill_missing`.
    pub filled_cells: usize,
}
/// Parses ESRI ASCII grids. Nodata cells are filled from surrounding elevations.
pub fn read_asc(input: &str) -> Result<GeoGrid, String> {
    let mut lines = input.lines().filter(|l| !l.trim().is_empty()).peekable();
    let mut headers = std::collections::HashMap::new();
    // Five or six header lines: NODATA_value is optional.
    while let Some(l) = lines.next_if(|l| {
        l.trim_start()
            .starts_with(|c: char| c.is_ascii_alphabetic())
    }) {
        let mut t = l.split_whitespace();
        let k = t.next().ok_or("Empty header")?.to_ascii_lowercase();
        let v = t.next().ok_or("Missing header value")?;
        headers.insert(k, v.to_string());
    }
    let get = |k: &str| -> Result<f64, String> {
        headers
            .get(k)
            .ok_or_else(|| format!("Missing {k}"))?
            .parse::<f64>()
            .map_err(|_| format!("Invalid {k}"))
    };
    let w = get("ncols")?;
    let h = get("nrows")?;
    if !w.is_finite()
        || !h.is_finite()
        || w < 2.
        || h < 2.
        || w.fract() != 0.
        || h.fract() != 0.
        || w > 100000.
        || h > 100000.
    {
        return Err("Invalid raster dimensions".into());
    }
    let (w, h) = (w as usize, h as usize);
    let cell_size = get("cellsize")?;
    if !cell_size.is_finite() || cell_size <= 0. {
        return Err("Invalid cellsize".into());
    }
    let xll = if headers.contains_key("xllcorner") {
        get("xllcorner")?
    } else {
        get("xllcenter")? - cell_size / 2.
    };
    let yll = if headers.contains_key("yllcorner") {
        get("yllcorner")?
    } else {
        get("yllcenter")? - cell_size / 2.
    };
    let nodata = headers
        .get("nodata_value")
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| "Invalid NODATA_value".to_string())
        })
        .transpose()?;
    let mut values = Vec::with_capacity(w * h);
    let mut missing = Vec::with_capacity(w * h);
    for l in lines {
        for token in l.split_whitespace() {
            let z = token.parse::<f64>().map_err(|_| "Non-numeric elevation")?;
            missing.push(!z.is_finite() || nodata == Some(z));
            values.push(z);
            if values.len() > w * h {
                return Err("Too many elevation values".into());
            }
        }
    }
    if values.len() != w * h {
        return Err("Too few elevation values".into());
    }
    let filled_cells = crate::nodata::fill_missing(w, h, &mut values, &missing)?;
    let dem = Dem::new(w, h, values)?;
    Ok(GeoGrid {
        dem,
        xll,
        yll,
        cell_size,
        nodata,
        filled_cells,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_grid() {
        let s="ncols 2\nnrows 2\nxllcorner 10\nyllcorner 20\ncellsize 3\nNODATA_value -9999\n1 2\n3 4\n";
        let g = read_asc(s).unwrap();
        assert_eq!(g.dem.at(1, 1), 4.);
        assert_eq!(g.xll, 10.);
    }
    #[test]
    fn fills_missing() {
        let s="ncols 2\nnrows 2\nxllcorner 10\nyllcorner 20\ncellsize 3\nNODATA_value -9999\n1 -9999\n3 4\n";
        let g = read_asc(s).unwrap();
        assert_eq!(g.filled_cells, 1);
        assert_eq!(g.dem.at(1, 0), 8. / 3.);
    }
    #[test]
    fn nodata_header_is_optional() {
        let g =
            read_asc("ncols 2\nnrows 2\nxllcorner 0\nyllcorner 0\ncellsize 1\n1 2\n3 4\n").unwrap();
        assert_eq!(g.dem.heights, vec![1., 2., 3., 4.]);
    }
}

/// Build a USGS 3DEP ImageServer exportImage request for a projected bounding
/// box. Caller must supply a valid spatial reference and download the URL.
/// Response is requested as GeoTIFF; decoding/reprojection are not yet included.
pub fn usgs_export_url(bbox: [f64; 4], epsg: u32, pixels: (u32, u32)) -> Result<String, String> {
    if bbox.iter().any(|x| !x.is_finite())
        || bbox[0] >= bbox[2]
        || bbox[1] >= bbox[3]
        || epsg == 0
        || pixels.0 < 2
        || pixels.1 < 2
        || pixels.0 > 4096
        || pixels.1 > 4096
    {
        return Err("Invalid USGS export bounds, CRS or image size".into());
    }
    Ok(format!("https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage?bbox={},{},{},{}&bboxSR={}&imageSR={}&size={},{}&format=tiff&pixelType=F32&f=image",bbox[0],bbox[1],bbox[2],bbox[3],epsg,epsg,pixels.0,pixels.1))
}
#[cfg(test)]
mod url_tests {
    use super::*;
    #[test]
    fn valid_url() {
        assert!(usgs_export_url([-121., 38., -120., 39.], 4326, (512, 512))
            .unwrap()
            .contains("format=tiff"));
    }
    #[test]
    fn invalid_bounds() {
        assert!(usgs_export_url([1., 0., 0., 1.], 4326, (512, 512)).is_err());
    }
}
