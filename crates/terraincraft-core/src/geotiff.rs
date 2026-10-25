//! GeoTIFF elevation decoding: any layout the `tiff` crate reads (strips or tiles,
//! uncompressed or compressed), north-up georeferencing from the GeoTIFF tags, and
//! nodata gaps filled from surrounding samples. Elevations are assumed to be meters.
use crate::georef::{CrsUnits, GeoReference};
use crate::Dem;
use std::io::Cursor;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

/// Largest raster accepted, in samples.
const MAX_SAMPLES: u64 = 25_000_000;

#[derive(Clone, Debug)]
pub struct DecodedRaster {
    /// Elevation grid; `dem.georef` is set when the file carries GeoTIFF tags.
    pub dem: Dem,
    /// Number of nodata samples replaced by `nodata::fill_missing`.
    pub filled_cells: usize,
}

pub fn decode(bytes: &[u8]) -> Result<DecodedRaster, String> {
    let err = |e: tiff::TiffError| format!("TIFF: {e}");
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(err)?;
    let (w, h) = decoder.dimensions().map_err(err)?;
    if w < 2 || h < 2 || w as u64 * h as u64 > MAX_SAMPLES {
        return Err(format!("Unsupported raster size {w} x {h}"));
    }
    let georef = read_georef(&mut decoder)?;
    let nodata = decoder
        .find_tag(Tag::GdalNodata)
        .map_err(err)?
        .map(|v| v.into_string().map_err(err))
        .transpose()?
        .and_then(|s| s.trim().trim_end_matches(char::from(0)).parse::<f64>().ok());
    let mut values: Vec<f64> = match decoder.read_image().map_err(err)? {
        DecodingResult::F32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::F64(v) => v,
        DecodingResult::U8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I32(v) => v.into_iter().map(f64::from).collect(),
        _ => return Err("Unsupported elevation sample type".into()),
    };
    let (w, h) = (w as usize, h as usize);
    if values.len() != w * h {
        return Err("Expected a single-band elevation raster".into());
    }
    // ESRI services mark missing data with -3.4e38 even without a nodata tag.
    let missing: Vec<bool> = values
        .iter()
        .map(|&v| !v.is_finite() || v < -1e30 || nodata.is_some_and(|n| v as f32 == n as f32))
        .collect();
    let filled_cells = crate::nodata::fill_missing(w, h, &mut values, &missing)?;
    let mut dem = Dem::new(w, h, values)?;
    dem.georef = georef;
    Ok(DecodedRaster { dem, filled_cells })
}

/// GeoKey IDs from the GeoTIFF 1.0 specification.
const GT_MODEL_TYPE: u16 = 1024;
const GT_RASTER_TYPE: u16 = 1025;
const GEOGRAPHIC_TYPE: u16 = 2048;
const PROJECTED_CS_TYPE: u16 = 3072;
const PROJ_LINEAR_UNITS: u16 = 3076;
const LINEAR_METER: u16 = 9001;

fn read_georef(decoder: &mut Decoder<Cursor<&[u8]>>) -> Result<Option<GeoReference>, String> {
    let err = |e: tiff::TiffError| format!("GeoTIFF: {e}");
    let mut f64_tag = |tag| -> Result<Option<Vec<f64>>, String> {
        decoder
            .find_tag(tag)
            .map_err(err)?
            .map(|v| v.into_f64_vec().map_err(err))
            .transpose()
    };
    let (Some(scale), Some(tie)) = (
        f64_tag(Tag::ModelPixelScaleTag)?,
        f64_tag(Tag::ModelTiepointTag)?,
    ) else {
        return Ok(None);
    };
    if scale.len() < 2 || tie.len() < 6 {
        return Err("Malformed GeoTIFF pixel scale or tie point".into());
    }
    let keys = decoder
        .find_tag(Tag::GeoKeyDirectoryTag)
        .map_err(err)?
        .map(|v| v.into_u16_vec().map_err(err))
        .transpose()?
        .unwrap_or_default();
    // Directory header is 4 shorts, then (key, location, count, value) entries.
    // Only keys stored inline (location 0) are needed here.
    let key = |id: u16| {
        keys.get(4..)
            .unwrap_or_default()
            .chunks_exact(4)
            .find(|k| k[0] == id && k[1] == 0)
            .map(|k| k[3])
    };
    let projected = key(GT_MODEL_TYPE) == Some(1);
    if projected && key(PROJ_LINEAR_UNITS).is_some_and(|u| u != LINEAR_METER) {
        return Err("Projected GeoTIFFs must use meters".into());
    }
    let (px, py) = (scale[0], scale[1]);
    let (mut x, mut y) = (tie[3] - tie[0] * px, tie[4] + tie[1] * py);
    // PixelIsPoint tie points refer to pixel centers; convert to the outer corner.
    if key(GT_RASTER_TYPE) == Some(2) {
        x -= px * 0.5;
        y += py * 0.5;
    }
    let g = GeoReference {
        epsg: key(PROJECTED_CS_TYPE).or(key(GEOGRAPHIC_TYPE)),
        units: if projected {
            CrsUnits::Meters
        } else {
            CrsUnits::Degrees
        },
        origin_x: x,
        origin_y: y,
        pixel_width: px,
        pixel_height: py,
    };
    g.validate()?;
    Ok(Some(g))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_header() {
        assert!(decode(b"not a tiff").is_err());
    }
    #[test]
    fn basic_float32() {
        // Minimal valid strip TIFF: 10 IFD entries, pixel data right after the IFD.
        let mut b = vec![0u8; 8 + 2 + 10 * 12 + 4];
        b[0..4].copy_from_slice(b"II*\0");
        b[4..8].copy_from_slice(&8u32.to_le_bytes());
        b[8..10].copy_from_slice(&10u16.to_le_bytes());
        let tags = [
            (256, 4, 2),
            (257, 4, 2),
            (258, 3, 32),
            (259, 3, 1),
            (262, 3, 1),
            (273, 4, 134),
            (277, 3, 1),
            (278, 4, 2),
            (279, 4, 16),
            (339, 3, 3),
        ];
        for (i, (tag, typ, val)) in tags.iter().enumerate() {
            let p = 10 + i * 12;
            b[p..p + 2].copy_from_slice(&(*tag as u16).to_le_bytes());
            b[p + 2..p + 4].copy_from_slice(&(*typ as u16).to_le_bytes());
            b[p + 4..p + 8].copy_from_slice(&1u32.to_le_bytes());
            b[p + 8..p + 12].copy_from_slice(&(*val as u32).to_le_bytes());
        }
        for v in [1f32, 2., 3., 4.] {
            b.extend_from_slice(&v.to_le_bytes())
        }
        let d = decode(&b).unwrap().dem;
        assert_eq!(d.at(1, 1), 4.);
        assert!(d.georef.is_none());
    }

    fn encode(values: &[f32], w: u32, h: u32, nodata: Option<&str>) -> Vec<u8> {
        use tiff::encoder::{colortype::Gray32Float, TiffEncoder};
        let mut out = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut out).unwrap();
        let mut img = enc.new_image::<Gray32Float>(w, h).unwrap();
        let e = img.encoder();
        e.write_tag(Tag::ModelPixelScaleTag, &[0.001f64, 0.002, 0.][..])
            .unwrap();
        e.write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., -121.5, 46.5, 0.][..])
            .unwrap();
        // Geographic model, PixelIsArea, EPSG:4326.
        let keys: [u16; 16] = [1, 1, 0, 3, 1024, 0, 1, 2, 1025, 0, 1, 1, 2048, 0, 1, 4326];
        e.write_tag(Tag::GeoKeyDirectoryTag, &keys[..]).unwrap();
        if let Some(n) = nodata {
            e.write_tag(Tag::GdalNodata, n).unwrap();
        }
        img.write_data(values).unwrap();
        out.into_inner()
    }
    #[test]
    fn reads_georeference() {
        let d = decode(&encode(&[1., 2., 3., 4., 5., 6.], 3, 2, None))
            .unwrap()
            .dem;
        let g = d.georef.unwrap();
        assert_eq!((g.epsg, g.units), (Some(4326), CrsUnits::Degrees));
        let b = g.bounds(3, 2);
        assert!(b
            .iter()
            .zip([-121.5, 46.496, -121.497, 46.5])
            .all(|(a, w)| (a - w).abs() < 1e-9));
    }
    /// A real 3DEP ImageServer response (tiled float GeoTIFF) for Mount Rainier,
    /// requested as bbox -121.85,46.8,-121.7,46.9 at 48 x 32 pixels.
    #[test]
    fn decodes_real_usgs_response() {
        let r = decode(include_bytes!("../../../examples/rainier_48x32.tif")).unwrap();
        let (d, g) = (&r.dem, r.dem.georef.unwrap());
        assert_eq!((d.width, d.height, r.filled_cells), (48, 32, 0));
        assert_eq!((g.epsg, g.units), (Some(4326), CrsUnits::Degrees));
        let b = g.bounds(d.width, d.height);
        let want = [-121.85, 46.8, -121.7, 46.9];
        assert!(
            b.iter().zip(want).all(|(a, w)| (a - w).abs() < 1e-9),
            "{b:?}"
        );
        // Coarse samples sit below Rainier's 4,392 m summit but well above the valleys.
        let (lo, hi) = d.min_max();
        assert!(
            lo > 1000. && lo < 1600. && hi > 4000. && hi < 4400.,
            "{lo} {hi}"
        );
        let s = crate::true_scale_settings(d, 400., 3., 1.).unwrap();
        // 31 x 111,168 m vs 47 x 76,242 m per degree, at 0.003125 degree pixels.
        assert!(
            (s.height_mm / s.width_mm - 0.9617).abs() < 0.002,
            "{}",
            s.height_mm
        );
    }
    #[test]
    fn fills_tagged_and_sentinel_nodata() {
        let r = decode(&encode(
            &[10., -9999., 10., 10., -3.4e38, 10.],
            3,
            2,
            Some("-9999"),
        ))
        .unwrap();
        assert_eq!(r.filled_cells, 2);
        assert!(r.dem.heights.iter().all(|v| *v == 10.));
    }
}
