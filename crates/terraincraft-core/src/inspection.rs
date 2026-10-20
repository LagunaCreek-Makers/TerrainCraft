//! Manufacturing preflight and human-readable laser layout report.
//! This validates bounding-box layout only; it does not certify cut topology.
use crate::laser::{CutPart,Placement};
use std::collections::{HashMap,HashSet};

pub fn preflight(parts:&[CutPart],placements:&[Placement],sheet_w:f64,sheet_h:f64,gap:f64)->Result<(),String>{
    if !sheet_w.is_finite()||!sheet_h.is_finite()||!gap.is_finite()||sheet_w<=0.||sheet_h<=0.||gap<0.{return Err("Invalid sheet dimensions or spacing".into())}
    let mut ids=HashSet::new();
    for p in parts {
        if !ids.insert(p.id.as_str()){return Err(format!("Duplicate part ID: {}",p.id))}
        if p.outline.len()<4||p.outline.first()!=p.outline.last() {return Err(format!("Unclosed outline: {}",p.id))}
        if p.outline.iter().any(|&(x,y)|!x.is_finite()||!y.is_finite()){return Err(format!("Nonfinite coordinates: {}",p.id))}
        let area:f64=p.outline.windows(2).map(|e|e[0].0*e[1].1-e[1].0*e[0].1).sum::<f64>().abs()/2.;
        if area<1e-8{return Err(format!("Zero-area outline: {}",p.id))}
    }
    let mut seen=HashSet::new();
    for p in placements {
        if !ids.contains(p.part_id.as_str()){return Err(format!("Unknown placement: {}",p.part_id))}
        if !seen.insert(p.part_id.as_str()){return Err(format!("Duplicate placement: {}",p.part_id))}
        if p.sheet==0||[p.x_mm,p.y_mm,p.width_mm,p.height_mm].iter().any(|v|!v.is_finite())||p.width_mm<=0.||p.height_mm<=0. {return Err(format!("Invalid placement: {}",p.part_id))}
        if p.x_mm<gap-1e-8||p.y_mm<gap-1e-8||p.x_mm+p.width_mm+gap>sheet_w+1e-8||p.y_mm+p.height_mm+gap>sheet_h+1e-8{return Err(format!("Part outside sheet: {}",p.part_id))}
    }
    if seen.len()!=parts.len(){return Err("Not all parts were placed".into())}
    for (i,a) in placements.iter().enumerate(){for b in &placements[i+1..]{if a.sheet==b.sheet && a.x_mm<b.x_mm+b.width_mm+gap-1e-8 && b.x_mm<a.x_mm+a.width_mm+gap-1e-8 && a.y_mm<b.y_mm+b.height_mm+gap-1e-8 && b.y_mm<a.y_mm+a.height_mm+gap-1e-8{return Err(format!("Placement collision: {} / {}",a.part_id,b.part_id))}}}
    Ok(())
}
fn html_escape(s:&str)->String{s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&#39;")}
/// Self-contained overview with scaled SVG previews. No scripts or network required.
pub fn report_html(parts:&[CutPart],placements:&[Placement],sheet_w:f64,sheet_h:f64)->String{
    let by_id:HashMap<&str,&CutPart>=parts.iter().map(|p|(p.id.as_str(),p)).collect();
    let mut sheets:Vec<usize>=placements.iter().map(|p|p.sheet).collect();sheets.sort_unstable();sheets.dedup();
    let mut result=format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>TerrainCraft layout report</title><style>body{{font:15px system-ui,sans-serif;max-width:1100px;margin:2rem auto;padding:0 1rem}}svg{{max-width:100%;height:auto;background:#faf9f5;border:1px solid #aaa}}section{{margin:2rem 0}}table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:.4rem .7rem}}</style><h1>TerrainCraft laser layout</h1><p>{} parts · {} sheets · sheet size {} × {} mm</p><p><strong>Development preview only:</strong> polygon holes, contour topology, kerf, and physical cut readiness have not been certified. Verify paths in laser CAM before cutting.</p>",parts.len(),sheets.len(),sheet_w,sheet_h);
    for sheet in sheets {result.push_str(&format!("<section><h2>Sheet {sheet}</h2><svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {sheet_w} {sheet_h}\" width=\"{sheet_w}\" height=\"{sheet_h}\">"));
        for pl in placements.iter().filter(|p|p.sheet==sheet){if let Some(part)=by_id.get(pl.part_id.as_str()){
            let min_x=part.outline.iter().map(|p|p.0).fold(f64::INFINITY,f64::min);let min_y=part.outline.iter().map(|p|p.1).fold(f64::INFINITY,f64::min);
            let path=part.outline.iter().enumerate().map(|(i,p)|format!("{} {:.4} {:.4}",if i==0{"M"}else{"L"},p.0-min_x+pl.x_mm,p.1-min_y+pl.y_mm)).collect::<Vec<_>>().join(" ");
            result.push_str(&format!("<path d=\"{path} Z\" fill=\"#d9e8d5\" stroke=\"#ad2222\" stroke-width=\"0.4\"/><text x=\"{}\" y=\"{}\" font-size=\"4\">{}</text>",pl.x_mm+2.,pl.y_mm+5.,html_escape(&pl.part_id)));
        }}result.push_str("</svg></section>");}
    result.push_str("<h2>Assembly manifest</h2><table><tr><th>Part</th><th>Elevation</th><th>Sheet</th><th>X (mm)</th><th>Y (mm)</th></tr>");
    for p in placements {let level=by_id.get(p.part_id.as_str()).map(|v|v.level).unwrap_or(0.);result.push_str(&format!("<tr><td>{}</td><td>{:.2}</td><td>{}</td><td>{:.2}</td><td>{:.2}</td></tr>",html_escape(&p.part_id),level,p.sheet,p.x_mm,p.y_mm));}
    result.push_str("</table></html>");result
}
#[cfg(test)] mod tests {use super::*;use crate::laser::layout;
fn part(id:&str)->CutPart{CutPart{id:id.into(),level:1.,outline:vec![(0.,0.),(10.,0.),(10.,10.),(0.,10.),(0.,0.)]}}
#[test]fn valid_layout(){let parts=vec![part("A"),part("B")];let pl=layout(&parts,50.,50.,2.).unwrap();assert!(preflight(&parts,&pl,50.,50.,2.).is_ok());assert!(report_html(&parts,&pl,50.,50.).contains("Sheet 1"));}
#[test]fn detects_overlap(){let parts=vec![part("A"),part("B")];let mut pl=layout(&parts,50.,50.,2.).unwrap();pl[1].x_mm=pl[0].x_mm;assert!(preflight(&parts,&pl,50.,50.,2.).is_err());}
#[test]fn detects_unclosed(){let mut p=part("A");p.outline.pop();assert!(preflight(&[p],&[],50.,50.,2.).is_err());}
}
