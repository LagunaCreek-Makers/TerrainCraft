//! Interpolated marching-squares contour paths for engraving and inspection.
//! This is not yet a polygon boolean engine for production cut paths.
use crate::Dem;
use std::collections::BTreeMap;
pub type Point=(f64,f64);
fn key(p:Point)->(i64,i64){((p.0*1e7).round() as i64,(p.1*1e7).round() as i64)}
fn edge(a:Point,b:Point,za:f64,zb:f64,t:f64)->Point {let f=if (zb-za).abs()<1e-14{0.5}else{((t-za)/(zb-za)).clamp(0.,1.)};(a.0+(b.0-a.0)*f,a.1+(b.1-a.1)*f)}
/// Produces stitched contour polylines in physical millimeters. Interior closed
/// loops repeat their starting point. Open paths may end on raster boundaries.
pub fn paths(d:&Dem,w:f64,h:f64,level:f64)->Result<Vec<Vec<Point>>,String>{
 if !w.is_finite()||!h.is_finite()||!level.is_finite()||w<=0.||h<=0.{return Err("Invalid contour parameters".into())}
 let mut segments:Vec<(Point,Point)>=Vec::new();
 for y in 0..d.height-1 {for x in 0..d.width-1 {
  let z=[d.at(x,y),d.at(x+1,y),d.at(x+1,y+1),d.at(x,y+1)];
  let p=[(x as f64*w/(d.width-1) as f64,y as f64*h/(d.height-1) as f64),((x+1) as f64*w/(d.width-1) as f64,y as f64*h/(d.height-1) as f64),((x+1) as f64*w/(d.width-1) as f64,(y+1) as f64*h/(d.height-1) as f64),(x as f64*w/(d.width-1) as f64,(y+1) as f64*h/(d.height-1) as f64)];
  let mut hits=Vec::new();for e in 0..4 {let j=(e+1)%4;if (z[e]>level)!=(z[j]>level){hits.push((e,edge(p[e],p[j],z[e],z[j],level)));}}
  match hits.len(){2=>segments.push((hits[0].1,hits[1].1)),4=>{
    // Resolve saddle consistently by connecting around corners whose sign
    // differs from the bilinearly interpolated cell center.
    let center=(z[0]+z[1]+z[2]+z[3])/4.;
    let pairs=if (z[0]>level)==(center>level){[(0,1),(2,3)]}else{[(0,3),(1,2)]};
    for (a,b) in pairs {segments.push((hits[a].1,hits[b].1));}
  },_=>{}}
 }}
 let mut adjacency:BTreeMap<(i64,i64),Vec<(usize,usize)>>=BTreeMap::new();
 for (i,(a,b)) in segments.iter().enumerate(){adjacency.entry(key(*a)).or_default().push((i,0));adjacency.entry(key(*b)).or_default().push((i,1));}
 let mut used=vec![false;segments.len()];let mut result=Vec::new();
 // Start at degree-one vertices to preserve open contours; then closed loops.
 let mut order:Vec<usize>=(0..segments.len()).collect();order.sort_by_key(|&i|{let a=key(segments[i].0);let b=key(segments[i].1);if adjacency[&a].len()==1||adjacency[&b].len()==1{0}else{1}});
 for i in order {if used[i]{continue}let (a,b)=segments[i];let start=if adjacency[&key(b)].len()==1{b}else{a};let mut line=vec![start];let mut current=key(start);
  loop {let next=adjacency.get(&current).and_then(|v|v.iter().find(|(j,_)|!used[*j]).copied());let Some((j,endpoint))=next else {break};used[j]=true;let p=if endpoint==0{segments[j].1}else{segments[j].0};line.push(p);current=key(p);if current==key(start){break}}
  if line.len()>1{result.push(line)}
 }
 Ok(result)
}
pub fn svg(d:&Dem,w:f64,h:f64,level:f64)->Result<String,String>{let polylines=paths(d,w,h,level)?;let mut out=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><g fill=\"none\" stroke=\"black\" stroke-width=\"0.1\">\n");for line in polylines{let mut s=String::new();for (i,(x,y)) in line.iter().enumerate(){s.push_str(&format!("{} {:.5} {:.5} ",if i==0{"M"}else{"L"},x,y));}out.push_str(&format!("<path d=\"{}\"/>\n",s));}out.push_str("</g></svg>\n");Ok(out)}
#[cfg(test)]mod tests{use super::*;
#[test]fn closed_peak(){let mut z=vec![0.;25];z[12]=10.;let d=Dem::new(5,5,z).unwrap();let p=paths(&d,40.,40.,5.).unwrap();assert_eq!(p.len(),1);assert_eq!(key(p[0][0]),key(*p[0].last().unwrap()));}
#[test]fn open_slope(){let d=Dem::new(3,3,vec![0.,1.,2.,0.,1.,2.,0.,1.,2.]).unwrap();let p=paths(&d,20.,20.,0.5).unwrap();assert_eq!(p.len(),1);assert_ne!(key(p[0][0]),key(*p[0].last().unwrap()));}
}
