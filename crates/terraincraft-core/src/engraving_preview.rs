//! Geometry for an interactive, machine-independent engraving preview.
//! Uses the same contour and horizon algorithms as SVG export.
use crate::{cartography::{self,Viewpoint}, contours, Dem};

#[derive(Clone, Debug)]
pub struct PreviewPath { pub points: Vec<(f64,f64)>, pub layer: usize }

/// Physical coordinates in millimeters, matching top-down SVG output.
pub fn topdown(dem:&Dem,width:f64,height:f64,interval:f64,max_levels:usize)->Result<Vec<PreviewPath>,String>{
    if !interval.is_finite() || interval<=0. || max_levels==0 {return Err("Invalid contour interval or level limit".into())}
    let (min,max)=dem.min_max();
    if !min.is_finite() || !max.is_finite() {return Err("Elevation raster contains invalid values".into())}
    let start=(min/interval).floor()*interval+interval;
    let n=(((max-start)/interval).floor()+1.).max(0.) as usize;
    if n>max_levels {return Err(format!("Too many contours ({n}); increase contour interval"))}
    let mut out=Vec::new();
    for i in 0..n {
        let level=start+i as f64*interval;
        for points in contours::paths(dem,width,height,level)? {
            if points.len()>1 {out.push(PreviewPath{points,layer:i});}
        }
    }
    Ok(out)
}

/// Normalized page coordinates, using the same camera mapping as SVG export.
pub fn perspective(dem:&Dem,v:Viewpoint,width:f64,height:f64,bands:usize,columns:usize)->Result<Vec<PreviewPath>,String>{
    if !width.is_finite()||!height.is_finite()||width<=0.||height<=50. {return Err("Invalid page size".into())}
    let curves=if bands<=1 {vec![cartography::horizon(dem,v,columns)?]} else {cartography::depth_horizons(dem,v,columns,bands)?};
    let top=25.;let bottom=height-25.;let center=(top+bottom)*0.5;
    let vertical_fov=v.fov_deg.to_radians()*(height/width);
    let scale=(bottom-top)/vertical_fov.max(0.05);
    let mut result=Vec::new();
    for (band,curve) in curves.iter().enumerate() {
        let mut segment=Vec::new();
        for (i,a) in curve.iter().enumerate() {
            let hidden=band>0 && curves[band-1][i]>=*a-1e-8;
            if !a.is_finite() || *a<=-std::f64::consts::FRAC_PI_2+1e-5 || hidden {
                if segment.len()>1 {result.push(PreviewPath{points:std::mem::take(&mut segment),layer:band});} else {segment.clear();}
                continue;
            }
            segment.push((i as f64/(columns-1) as f64*width,(center-(*a-v.pitch_deg.to_radians())*scale).clamp(top,bottom)));
        }
        if segment.len()>1 {result.push(PreviewPath{points:segment,layer:band});}
    }
    Ok(result)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn topdown_has_real_contours(){let mut v=vec![0.;25];v[12]=100.;let d=Dem::new(5,5,v).unwrap();let p=topdown(&d,40.,40.,20.,10).unwrap();assert!(!p.is_empty());assert!(p.iter().all(|x|x.points.iter().all(|(a,b)|*a>=0.&&*a<=40.&&*b>=0.&&*b<=40.)));}
    #[test] fn rejects_excessive_levels(){let d=Dem::new(2,2,vec![0.,1000.,0.,1000.]).unwrap();assert!(topdown(&d,10.,10.,0.01,20).is_err());}
    #[test] fn perspective_is_in_page(){let d=Dem::new(5,5,vec![100.;25]).unwrap();let v=Viewpoint{x:2.,y:2.,bearing_deg:90.,pitch_deg:0.,fov_deg:80.,eye_height_m:2.,max_distance_m:100.,pixel_size_x_m:10.,pixel_size_y_m:10.};let p=perspective(&d,v,200.,100.,2,48).unwrap();assert!(p.iter().all(|line|line.points.iter().all(|(x,y)|*x>=0.&&*x<=200.&&*y>=25.&&*y<=75.)));}
}
