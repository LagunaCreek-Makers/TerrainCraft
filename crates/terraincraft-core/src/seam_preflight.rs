//! Socket preflight for resolution-independent sidewall triangulation.
use crate::{Dem,TerrainSettings,Tile};
use crate::seam_joints::SeamHole;
use std::fmt::Write;
#[derive(Debug,Clone)]
pub struct SocketCheck {pub tile_row:usize,pub tile_col:usize,pub side:String,pub along_mm:f64,pub compatible:bool,pub reason:String}
/// Validate footprint, wall thickness, depth and opening overlap.
pub fn inspect(d:&Dem,s:TerrainSettings,tiles:&[Tile],holes:&[SeamHole])->Result<Vec<SocketCheck>,String>{
 s.validate()?;
 let sx=s.width_mm/(d.width-1) as f64;let sy=s.height_mm/(d.height-1) as f64;
 let mut out:Vec<SocketCheck>=Vec::with_capacity(holes.len());
 let mut accepted:Vec<(usize,usize,String,f64,f64,f64,usize)>=Vec::new();
 for hole in holes {
  let tile=tiles.iter().find(|t|t.row==hole.tile_row&&t.col==hole.tile_col).ok_or("Seam hole references unknown tile")?;
  let (step,cells,depth_limit)=match hole.side {
   "north"|"south"=>(sx,tile.x1-tile.x0,(tile.y1-tile.y0) as f64*sy/2.),
   "east"|"west"=>(sy,tile.y1-tile.y0,(tile.x1-tile.x0) as f64*sx/2.),
   _=>return Err("Invalid seam wall".into())
  };
  let r=hole.diameter_mm/2.;let u=hole.along_mm;
  let mut reason=if ![u,r,hole.height_mm,hole.depth_mm].iter().all(|v|v.is_finite())||r<=0.||hole.depth_mm<=0.{"invalid dimensions"}
   else if u-r<=0.||u+r>=step*cells as f64 {"outside wall bounds"}
   else if hole.height_mm-r<=0.||hole.height_mm+r>=s.base_mm {"insufficient base thickness"}
   else if hole.depth_mm>=depth_limit {"socket exceeds tile half-width"}
   else {"ok"};
  // Cylinders can overlap even when their centers are in different DEM cells.
  // Compare circular openings on each tile wall, with a small numerical tolerance.
  if reason=="ok" {
   for &(row,col,ref wall,other_u,other_z,other_r,prior) in &accepted {
    if row==hole.tile_row && col==hole.tile_col && wall==hole.side {
     let du=u-other_u;let dz=hole.height_mm-other_z;
     if du*du+dz*dz < (r+other_r+0.01).powi(2) {
      out[prior].compatible=false;
      out[prior].reason="overlapping socket openings".into();
      reason="overlapping socket openings";
     }
    }
   }
  }
  if reason=="ok" {accepted.push((hole.tile_row,hole.tile_col,hole.side.to_string(),u,hole.height_mm,r,out.len()));}
  out.push(SocketCheck{tile_row:hole.tile_row,tile_col:hole.tile_col,side:hole.side.into(),along_mm:u,compatible:reason=="ok",reason:reason.into()});
 }
 Ok(out)
}
pub fn csv(checks:&[SocketCheck])->String{
 let mut s=String::from("tile_row,tile_col,wall,along_mm,compatible,reason\n");
 for c in checks {writeln!(s,"{},{},{},{:.4},{},{}",c.tile_row+1,c.tile_col+1,c.side,c.along_mm,c.compatible,c.reason).unwrap();}s
}
/// Plan-view SVG with numbered tile outlines and socket locations; schematic, not a drill template.
pub fn assembly_preview(d:&Dem,s:TerrainSettings,tiles:&[Tile],checks:&[SocketCheck])->Result<String,String>{
 s.validate()?;let sx=s.width_mm/(d.width-1) as f64;let sy=s.height_mm/(d.height-1) as f64;
 let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">",s.width_mm,s.height_mm,s.width_mm,s.height_mm);
 svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"white\"/>");
 for t in tiles {
  let x=t.x0 as f64*sx;let y=t.y0 as f64*sy;let w=(t.x1-t.x0) as f64*sx;let h=(t.y1-t.y0) as f64*sy;
  write!(svg,"<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"#444\" stroke-width=\"0.6\"/>").unwrap();
  write!(svg,"<text x=\"{}\" y=\"{}\" font-size=\"4\" text-anchor=\"middle\" fill=\"#333\">R{} C{}</text>",x+w/2.,y+h/2.,t.row+1,t.col+1).unwrap();
 }
 for c in checks {
  let t=tiles.iter().find(|t|t.row==c.tile_row&&t.col==c.tile_col).ok_or("Unknown tile")?;
  let x=t.x0 as f64*sx;let y=t.y0 as f64*sy;let w=(t.x1-t.x0) as f64*sx;let h=(t.y1-t.y0) as f64*sy;
  let (cx,cy)=match c.side.as_str(){"north"=>(x+c.along_mm,y),"south"=>(x+c.along_mm,y+h),"west"=>(x,y+c.along_mm),"east"=>(x+w,y+c.along_mm),_=>return Err("Unknown wall".into())};
  let color=if c.compatible{"#16803c"}else{"#d12b2b"};
  write!(svg,"<circle cx=\"{cx}\" cy=\"{cy}\" r=\"1.5\" fill=\"{color}\"/>").unwrap();
 }
 svg.push_str("</svg>");Ok(svg)
}
#[cfg(test)]mod tests {use super::*;
 #[test]fn allows_cross_cell(){let d=Dem::new(3,3,vec![0.;9]).unwrap();let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:10.,relief_mm:5.,vertical_exaggeration:1.};let t=Tile{row:0,col:0,x0:0,x1:2,y0:0,y1:2};let h=SeamHole{seam_id:"x".into(),tile_row:0,tile_col:0,side:"east",along_mm:50.,height_mm:5.,depth_mm:6.,diameter_mm:3.3};let checks=inspect(&d,s,&[t],&[h]).unwrap();assert!(checks[0].compatible);assert!(csv(&checks).contains("true,ok"));assert!(assembly_preview(&d,s,&[t],&checks).unwrap().contains("<circle"));}
}

#[cfg(test)] mod collision_tests {
 use super::*;
 #[test] fn overlapping_same_cell_fails_both_sockets() {
  let d=Dem::new(3,3,vec![0.;9]).unwrap();
  let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:10.,relief_mm:5.,vertical_exaggeration:1.};
  let t=Tile{row:0,col:0,x0:0,x1:2,y0:0,y1:2};
  let make=|u|SeamHole{seam_id:"a".into(),tile_row:0,tile_col:0,side:"east",along_mm:u,height_mm:5.,depth_mm:6.,diameter_mm:3.};
  let checks=inspect(&d,s,&[t],&[make(20.),make(21.)]).unwrap();
  assert_eq!(checks.len(),2);
  assert!(checks.iter().all(|c|!c.compatible));
  assert!(checks.iter().all(|c|c.reason.contains("overlapping socket openings")));
 }
 #[test] fn different_cells_are_compatible() {
  let d=Dem::new(3,3,vec![0.;9]).unwrap();
  let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:10.,relief_mm:5.,vertical_exaggeration:1.};
  let t=Tile{row:0,col:0,x0:0,x1:2,y0:0,y1:2};
  let make=|u|SeamHole{seam_id:"a".into(),tile_row:0,tile_col:0,side:"east",along_mm:u,height_mm:5.,depth_mm:6.,diameter_mm:3.};
  let checks=inspect(&d,s,&[t],&[make(20.),make(70.)]).unwrap();
  assert!(checks.iter().all(|c|c.compatible));
 }
}

#[cfg(test)] mod proximity_tests {
 use super::*;
 #[test] fn neighboring_cell_openings_must_not_overlap() {
  let d=Dem::new(11,11,vec![0.;121]).unwrap();
  let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:12.,relief_mm:5.,vertical_exaggeration:1.};
  let t=Tile{row:0,col:0,x0:0,x1:10,y0:0,y1:10};
  let mk=|u|SeamHole{seam_id:"s".into(),tile_row:0,tile_col:0,side:"east",along_mm:u,height_mm:6.,depth_mm:4.,diameter_mm:7.};
  let checks=inspect(&d,s,&[t],&[mk(14.),mk(16.)]).unwrap();
  assert!(checks.iter().all(|c|!c.compatible));
 }
 #[test] fn separated_openings_are_valid() {
  let d=Dem::new(11,11,vec![0.;121]).unwrap();
  let s=TerrainSettings{width_mm:100.,height_mm:100.,base_mm:12.,relief_mm:5.,vertical_exaggeration:1.};
  let t=Tile{row:0,col:0,x0:0,x1:10,y0:0,y1:10};
  let mk=|u|SeamHole{seam_id:"s".into(),tile_row:0,tile_col:0,side:"east",along_mm:u,height_mm:6.,depth_mm:4.,diameter_mm:3.};
  let checks=inspect(&d,s,&[t],&[mk(15.),mk(35.)]).unwrap();
  assert!(checks.iter().all(|c|c.compatible));
 }
}
