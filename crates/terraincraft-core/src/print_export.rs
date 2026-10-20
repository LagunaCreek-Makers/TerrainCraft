//! Binary STL export and printable tile placement manifest.
use crate::{Dem, TerrainSettings, Tile};
use std::{fs::File, io::{self, Write}, path::Path};

/// Writes standard little-endian binary STL. Rejects degenerate and nonfinite triangles.
pub fn write_binary_stl(path: &Path, faces: &[[[f64; 3]; 3]]) -> io::Result<()> {
    if faces.len() > u32::MAX as usize { return Err(io::Error::new(io::ErrorKind::InvalidInput,"STL triangle count exceeds u32")); }
    let mut records = Vec::with_capacity(faces.len().saturating_mul(50));
    for face in faces {
        if face.iter().flatten().any(|v| !v.is_finite() || v.abs() > f32::MAX as f64) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,"Nonfinite or out-of-range STL vertex"));
        }
        let a = [face[1][0]-face[0][0],face[1][1]-face[0][1],face[1][2]-face[0][2]];
        let b = [face[2][0]-face[0][0],face[2][1]-face[0][1],face[2][2]-face[0][2]];
        let n = [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
        let length = n.iter().map(|v|v*v).sum::<f64>().sqrt();
        if !length.is_finite() || length <= 1e-15 { return Err(io::Error::new(io::ErrorKind::InvalidInput,"Degenerate STL triangle")); }
        for component in n { records.extend_from_slice(&((component/length) as f32).to_le_bytes()); }
        for vertex in face { for coordinate in vertex { records.extend_from_slice(&(*coordinate as f32).to_le_bytes()); } }
        records.extend_from_slice(&0u16.to_le_bytes());
    }
    let mut file = File::create(path)?;
    let mut header = [0u8;80];
    header[..23].copy_from_slice(b"TerrainCraft binary STL");
    file.write_all(&header)?;
    file.write_all(&(faces.len() as u32).to_le_bytes())?;
    file.write_all(&records)?;
    Ok(())
}

/// Records tile footprints and the origin of each tile in the assembled terrain.
pub fn tile_manifest(dem:&Dem, settings:TerrainSettings, tiles:&[Tile]) -> Result<String,String> {
    settings.validate()?;
    let mut csv = String::from("filename,row,col,origin_x_mm,origin_y_mm,width_mm,height_mm,x0_cell,x1_cell,y0_cell,y1_cell\n");
    let sx=settings.width_mm/(dem.width-1) as f64;
    let sy=settings.height_mm/(dem.height-1) as f64;
    for t in tiles {
        if t.x0>=t.x1 || t.y0>=t.y1 || t.x1>=dem.width || t.y1>=dem.height {return Err("Invalid tile in manifest".into());}
        csv.push_str(&format!("tile_r{:02}_c{:02}.stl,{},{},{:.4},{:.4},{:.4},{:.4},{},{},{},{}\n",
            t.row+1,t.col+1,t.row+1,t.col+1,t.x0 as f64*sx,t.y0 as f64*sy,
            (t.x1-t.x0) as f64*sx,(t.y1-t.y0) as f64*sy,t.x0,t.x1,t.y0,t.y1));
    }
    Ok(csv)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn binary_stl_layout() {
        let p=std::env::temp_dir().join(format!("terraincraft-stl-{}.stl",std::process::id()));
        let tri=[[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]]];
        write_binary_stl(&p,&tri).unwrap();
        let bytes=std::fs::read(&p).unwrap();std::fs::remove_file(p).unwrap();
        assert_eq!(bytes.len(),134);assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()),1);
    }
    #[test] fn rejects_degenerate() {
        let p=std::env::temp_dir().join("terraincraft-degenerate.stl");
        assert!(write_binary_stl(&p,&[[[0.,0.,0.];3]]).is_err());
    }
    #[test] fn manifest_footprints() {
        let d=Dem::new(3,3,vec![0.;9]).unwrap();
        let s=TerrainSettings{width_mm:200.,height_mm:100.,base_mm:2.,vertical_exaggeration:1.,relief_mm:10.};
        let t=Tile{row:0,col:0,x0:0,x1:1,y0:0,y1:2};
        let m=tile_manifest(&d,s,&[t]).unwrap();assert!(m.contains("100.0000,100.0000"));
    }
}
