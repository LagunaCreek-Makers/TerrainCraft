//! Minimal dependency-free baseline TIFF elevation decoder.
//! Supports single-band, uncompressed, chunky, strip-organized TIFFs.
//! Does not reproject or interpret GeoTIFF coordinate keys.
use crate::Dem;

pub fn decode(bytes: &[u8]) -> Result<Dem, String> {
    if bytes.len() < 8 {
        return Err("TIFF header truncated".into());
    }
    let le = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("Invalid TIFF byte order".into()),
    };
    let u16at = |p: usize| -> Result<u16, String> {
        let b = bytes.get(p..p + 2).ok_or("TIFF read out of bounds")?;
        Ok(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32at = |p: usize| -> Result<u32, String> {
        let b = bytes.get(p..p + 4).ok_or("TIFF read out of bounds")?;
        Ok(if le {
            u32::from_le_bytes(b.try_into().unwrap())
        } else {
            u32::from_be_bytes(b.try_into().unwrap())
        })
    };
    if u16at(2)? != 42 {
        return Err("Only classic TIFF is supported (not BigTIFF)".into());
    }
    let ifd = u32at(4)? as usize;
    let count = u16at(ifd)? as usize;
    if count > 4096 {
        return Err("Excessive TIFF tags".into());
    }
    let mut tags = std::collections::BTreeMap::new();
    for i in 0..count {
        let p = ifd
            .checked_add(2)
            .and_then(|v| v.checked_add(i * 12))
            .ok_or("IFD overflow")?;
        let tag = u16at(p)?;
        let typ = u16at(p + 2)?;
        let n = u32at(p + 4)? as usize;
        let unit = match typ {
            3 => 2,
            4 => 4,
            _ => continue,
        };
        let len = n.checked_mul(unit).ok_or("TIFF tag size overflow")?;
        if len > 64 * 1024 * 1024 {
            return Err("TIFF tag too large".into());
        }
        let offset = if len <= 4 {
            p + 8
        } else {
            u32at(p + 8)? as usize
        };
        if bytes
            .get(offset..offset.checked_add(len).ok_or("TIFF offset overflow")?)
            .is_none()
        {
            return Err("TIFF tag outside file".into());
        }
        let mut values = Vec::with_capacity(n);
        for j in 0..n {
            values.push(if typ == 3 {
                u16at(offset + j * 2)? as u32
            } else {
                u32at(offset + j * 4)?
            });
        }
        tags.insert(tag, values);
    }
    let get = |tag: u16, default: u32| -> u32 {
        tags.get(&tag)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(default)
    };
    let width = get(256, 0) as usize;
    let height = get(257, 0) as usize;
    if width < 2
        || height < 2
        || width
            .checked_mul(height)
            .ok_or("Raster dimensions overflow")?
            > 20_000_000
    {
        return Err("Unsupported raster dimensions".into());
    }
    let bits = get(258, 0);
    let sample_format = get(339, 1);
    if get(259, 1) != 1 || get(277, 1) != 1 || get(284, 1) != 1 {
        return Err("Only uncompressed single-band chunky TIFF supported".into());
    }
    let bps = match (sample_format, bits) {
        (3, 32) | (1, 32) | (2, 32) => 4,
        (1, 16) | (2, 16) => 2,
        (1, 8) => 1,
        (3, 64) => 8,
        _ => {
            return Err(format!(
                "Unsupported TIFF sample format {sample_format}, bits {bits}"
            ))
        }
    };
    let offsets = tags.get(&273).ok_or("Missing StripOffsets")?;
    let counts = tags.get(&279).ok_or("Missing StripByteCounts")?;
    let rows_per_strip = get(278, height as u32) as usize;
    if rows_per_strip == 0 || offsets.len() != counts.len() {
        return Err("Invalid TIFF strips".into());
    }
    let needed = height.div_ceil(rows_per_strip);
    if offsets.len() != needed {
        return Err("Strip count does not match raster height".into());
    }
    let mut z = Vec::with_capacity(width * height);
    for (i, (&offset, &count)) in offsets.iter().zip(counts).enumerate() {
        let rows = rows_per_strip.min(height - i * rows_per_strip);
        let size = rows
            .checked_mul(width)
            .and_then(|v| v.checked_mul(bps))
            .ok_or("Strip size overflow")?;
        if (count as usize) < size {
            return Err("Truncated TIFF strip".into());
        }
        let data = bytes
            .get(
                offset as usize
                    ..(offset as usize)
                        .checked_add(size)
                        .ok_or("Strip offset overflow")?,
            )
            .ok_or("Strip outside TIFF")?;
        for chunk in data.chunks_exact(bps) {
            let v = match (sample_format, bits) {
                (3, 32) => {
                    let b: [u8; 4] = chunk.try_into().unwrap();
                    (if le {
                        f32::from_le_bytes(b)
                    } else {
                        f32::from_be_bytes(b)
                    }) as f64
                }
                (3, 64) => {
                    let b: [u8; 8] = chunk.try_into().unwrap();
                    if le {
                        f64::from_le_bytes(b)
                    } else {
                        f64::from_be_bytes(b)
                    }
                }
                (1, 8) => chunk[0] as f64,
                (1, 16) => {
                    let b: [u8; 2] = chunk.try_into().unwrap();
                    (if le {
                        u16::from_le_bytes(b)
                    } else {
                        u16::from_be_bytes(b)
                    }) as f64
                }
                (2, 16) => {
                    let b: [u8; 2] = chunk.try_into().unwrap();
                    (if le {
                        i16::from_le_bytes(b)
                    } else {
                        i16::from_be_bytes(b)
                    }) as f64
                }
                (1, 32) => {
                    let b: [u8; 4] = chunk.try_into().unwrap();
                    (if le {
                        u32::from_le_bytes(b)
                    } else {
                        u32::from_be_bytes(b)
                    }) as f64
                }
                (2, 32) => {
                    let b: [u8; 4] = chunk.try_into().unwrap();
                    (if le {
                        i32::from_le_bytes(b)
                    } else {
                        i32::from_be_bytes(b)
                    }) as f64
                }
                _ => unreachable!(),
            };
            z.push(v)
        }
    }
    Dem::new(width, height, z)
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
        let mut b = vec![0u8; 8 + 2 + 9 * 12 + 4];
        b[0..4].copy_from_slice(b"II*\0");
        b[4..8].copy_from_slice(&8u32.to_le_bytes());
        b[8..10].copy_from_slice(&9u16.to_le_bytes());
        let tags = [
            (256, 4, 2),
            (257, 4, 2),
            (258, 3, 32),
            (259, 3, 1),
            (273, 4, 122),
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
        let d = decode(&b).unwrap();
        assert_eq!(d.at(1, 1), 4.);
    }
}
