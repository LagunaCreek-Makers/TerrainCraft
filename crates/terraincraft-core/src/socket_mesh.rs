//! Underside blind cylindrical registration pockets. The hole is cut into one
//! DEM bottom quad; this requires enough grid resolution around every hole.
use crate::{Dem,TerrainSettings,Tile,mesh};
use crate::registration::{TileGuide,RegistrationSettings};
use std::collections::HashMap;
type Tri=[[f64;3];3];
fn push(out:&mut Vec<Tri>,a:[f64;3],b:[f64;3],c:[f64;3]) {out.push([a,b,c]);}
/// Returns a closed tile with blind holes. No boolean/CAD dependency.
/// Fails explicitly if a hole crosses a DEM cell edge or pockets overlap.
pub fn mesh_with_sockets(d:&Dem,s:TerrainSettings,t:Tile,g:&TileGuide,r:RegistrationSettings,segments:usize)->Result<Vec<Tri>,String>{
 s.validate()?;r.validate()?;
 if segments<12 || segments>128 || segments%4!=0 {return Err("Socket segments must be divisible by 4, between 12 and 128".into())}
 if g.row!=t.row||g.col!=t.col{return Err("Guide does not match tile".into())}
 let depth=r.pin_length_mm/2.;if depth>=s.base_mm {return Err(format!("Socket depth {depth:.2} mm must be less than base thickness {:.2} mm",s.base_mm))}
 let sx=s.width_mm/(d.width-1) as f64;let sy=s.height_mm/(d.height-1) as f64;
 let radius=(r.pin_diameter_mm+r.hole_clearance_mm)/2.;
 let mut pockets=HashMap::<(usize,usize),[f64;2]>::new();
 for h in &g.holes {
  if !h.x_mm.is_finite()||!h.y_mm.is_finite()||h.x_mm<0.||h.y_mm<0. {return Err("Invalid socket position".into())}
  let cx=(h.x_mm/sx).floor() as usize;let cy=(h.y_mm/sy).floor() as usize;
  if cx>=t.x1-t.x0 || cy>=t.y1-t.y0 {return Err("Socket outside tile".into())}
  let x0=cx as f64*sx;let y0=cy as f64*sy;
  if h.x_mm-x0<=radius+1e-7 || x0+sx-h.x_mm<=radius+1e-7 || h.y_mm-y0<=radius+1e-7 || y0+sy-h.y_mm<=radius+1e-7 {return Err("Socket overlaps DEM grid edge; downsample DEM or move registration point".into())}
  if pockets.insert((cx,cy),[h.x_mm,h.y_mm]).is_some(){return Err("Two sockets occupy the same DEM cell".into())}
 }
 let mut faces=mesh(d,s,t)?;
 // The standard mesh emits two top and then two underside triangles per cell.
 let cells_x=t.x1-t.x0;
 let mut discard=Vec::new();
 for &(cx,cy) in pockets.keys(){let index=(cy*cells_x+cx)*4;discard.push(index+2);discard.push(index+3);}
 discard.sort_unstable();for i in discard.into_iter().rev(){faces.remove(i);}
 let quarter=segments/4;
 for ((cx,cy),center) in pockets {
  let x0=cx as f64*sx;let x1=x0+sx;let y0=cy as f64*sy;let y1=y0+sy;
  // Four corners in CCW order. Match each quarter-circle to a square edge.
  let corners=[[x1,y0],[x1,y1],[x0,y1],[x0,y0]];
  let ring=|i:usize,z:f64|->[f64;3]{let a=-std::f64::consts::FRAC_PI_4+std::f64::consts::TAU*i as f64/segments as f64;[center[0]+radius*a.cos(),center[1]+radius*a.sin(),z]};
  for side in 0..4 {
   let start=corners[side];let end=corners[(side+1)%4];
   for k in 0..quarter {
    let a=side*quarter+k;let b=a+1;
    let t0=k as f64/quarter as f64;let t1=(k+1) as f64/quarter as f64;
    let outer0=[start[0]+(end[0]-start[0])*t0,start[1]+(end[1]-start[1])*t0,0.];
    let outer1=[start[0]+(end[0]-start[0])*t1,start[1]+(end[1]-start[1])*t1,0.];
    let inner0=ring(a,0.);let inner1=ring(b,0.);
    // Bottom face points down; circular wall points into the pocket.
    push(&mut faces,outer0,inner1,outer1);push(&mut faces,outer0,inner0,inner1);
    let upper0=ring(a,depth);let upper1=ring(b,depth);
    push(&mut faces,inner0,upper1,upper0);push(&mut faces,inner0,inner1,upper1);
    push(&mut faces,[center[0],center[1],depth],upper0,upper1);
   }
  }
 }
 Ok(faces)
}
#[cfg(test)]mod tests{
 use super::*;use crate::registration::Hole;
 #[test]fn pocket_adds_geometry(){let d=Dem::new(3,3,vec![0.;9]).unwrap();let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:6.,relief_mm:5.,vertical_exaggeration:1.};let t=Tile{row:0,col:0,x0:0,x1:2,y0:0,y1:2};let g=TileGuide{row:0,col:0,width_mm:100.,height_mm:100.,holes:vec![Hole{x_mm:25.,y_mm:25.,seam_id:"a".into()}]};let f=mesh_with_sockets(&d,s,t,&g,RegistrationSettings::default(),32).unwrap();assert!(f.len()>mesh(&d,s,t).unwrap().len());}
 #[test]fn insufficient_base_rejected(){let d=Dem::new(2,2,vec![0.;4]).unwrap();let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:3.,relief_mm:5.,vertical_exaggeration:1.};let t=Tile{row:0,col:0,x0:0,x1:1,y0:0,y1:1};let g=TileGuide{row:0,col:0,width_mm:100.,height_mm:100.,holes:vec![]};assert!(mesh_with_sockets(&d,s,t,&g,RegistrationSettings::default(),32).is_err());}
}
