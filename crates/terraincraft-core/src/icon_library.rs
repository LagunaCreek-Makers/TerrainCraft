//! Single-pass black-and-white SVG icon library with provenance metadata.
//! Icons are original procedural pictograms, not depictions of specific landmarks.
use serde::{Deserialize,Serialize};
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct IconRecord {pub id:String,pub title:String,pub category:String,pub jurisdiction:String,pub landmark:String,pub license:String,pub source:String,pub svg_file:String}
/// 240 distinct geometric pictograms, suitable for SVG engraving and scalable to regions.
pub fn procedural_icon(index:usize)->String {
 let kind=index%20; let variant=index/20;
 let mut paths=String::new();
 let y=6+(variant%4) as i32;
 match kind {
  0=>paths.push_str("<path d='M4 54 L32 8 L60 54 Z M18 54 L32 30 L46 54'/>"),
  1=>paths.push_str("<path d='M8 56 V25 L32 6 L56 25 V56 Z M25 56 V38 H39 V56'/>"),
  2=>paths.push_str("<path d='M8 56 H56 M13 56 V17 H51 V56 M8 17 H56 M20 17 V8 H44 V17 M23 56 V34 H41 V56'/>"),
  3=>paths.push_str("<path d='M4 55 Q32 15 60 55 M4 55 H60 M12 43 V55 M52 43 V55'/>"),
  4=>paths.push_str("<path d='M32 5 L43 28 L32 57 L21 28 Z M7 28 H57 M32 5 V57'/>"),
  5=>paths.push_str("<path d='M6 54 L20 12 L34 54 M13 35 H27 M34 54 L48 12 L59 54 M41 35 H54'/>"),
  6=>paths.push_str("<path d='M5 54 H59 M12 54 V25 H52 V54 M8 25 L32 8 L56 25 M25 54 V35 H39 V54'/>"),
  7=>paths.push_str("<path d='M8 55 L16 25 L26 43 L37 9 L47 34 L56 20 L60 55 Z'/>"),
  8=>paths.push_str("<path d='M8 55 C8 39 23 39 23 25 C23 11 41 11 41 25 C41 39 56 39 56 55 M32 55 V17'/>"),
  9=>paths.push_str("<circle cx='32' cy='31' r='23'/><path d='M9 31 H55 M32 8 V54 M16 16 L48 46'/>"),
  10=>paths.push_str("<path d='M6 54 H58 M13 54 V23 L32 8 L51 23 V54 M20 30 H44 M20 39 H44 M20 48 H44'/>"),
  11=>paths.push_str("<path d='M8 55 L18 17 H46 L56 55 Z M14 39 H50 M22 17 V7 H42 V17'/>"),
  12=>paths.push_str("<path d='M4 49 Q18 30 32 49 T60 49 M4 35 Q18 16 32 35 T60 35 M4 21 Q18 2 32 21 T60 21'/>"),
  13=>paths.push_str("<path d='M5 54 H59 M12 54 L22 17 H42 L52 54 M17 35 H47 M24 17 V8 H40 V17'/>"),
  14=>paths.push_str("<path d='M32 5 L57 55 H7 Z M32 5 V55 M17 35 H47'/>"),
  15=>paths.push_str("<path d='M8 55 H56 M13 55 V12 H51 V55 M20 20 H28 V28 H20 Z M36 20 H44 V28 H36 Z M26 55 V38 H38 V55'/>"),
  16=>paths.push_str("<path d='M5 53 Q17 7 32 32 Q47 57 59 11 M5 11 Q17 57 32 32 Q47 7 59 53'/>"),
  17=>paths.push_str("<path d='M5 54 H59 M9 54 L15 17 H49 L55 54 M22 17 V8 H42 V17 M22 36 H42'/>"),
  18=>paths.push_str("<path d='M6 54 L32 8 L58 54 M12 43 H52 M20 30 H44 M32 8 V54'/>"),
  _=>paths.push_str("<path d='M8 54 H56 M16 54 V14 H48 V54 M10 14 H54 M25 14 V5 H39 V14 M24 54 V33 H40 V54'/>"),
 }
 // 12 distinctive secondary motifs, kept within 64x64 and in one monochrome stroke.
 if variant%3==0 { paths.push_str(&format!("<path d='M{} 58 H{}'/>",8+variant%8,56-variant%8)); }
 else if variant%3==1 { paths.push_str(&format!("<circle cx='{}' cy='{}' r='2'/>",14+variant%30,y+1)); }
 else { paths.push_str(&format!("<path d='M{} 59 l4 -4 l4 4'/>",12+variant%30)); }
 format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64' width='64' height='64'><g fill='none' stroke='#000' stroke-width='2.2' stroke-linecap='round' stroke-linejoin='round'>{paths}</g></svg>")
}
/// Insert an icon into a single-layer engraving without changing the underlying SVG.
pub fn overlay_icon(mut svg:String,icon:&str,x_mm:f64,y_mm:f64,size_mm:f64)->Result<String,String>{
 if ![x_mm,y_mm,size_mm].iter().all(|x|x.is_finite())||size_mm<=0.||size_mm>1000. {return Err("Invalid icon placement".into())}
 let start=icon.find("<g ").ok_or("Icon must contain a vector group")?;
 let end=icon.rfind("</g>").ok_or("Invalid icon group")?+4;
 let inner=&icon[start..end];
 let group=format!("<g id=\"terraincraft-engraving-icon\" transform=\"translate({x_mm:.3} {y_mm:.3}) scale({:.6})\">{inner}</g>\n",size_mm/64.);
 let pos=svg.rfind("</svg>").ok_or("Missing SVG closing tag")?;svg.insert_str(pos,&group);Ok(svg)
}
#[cfg(test)]mod tests {use super::*;
 #[test]fn icons_are_monochrome_vectors(){for i in 0..240{let s=procedural_icon(i);assert!(s.contains("stroke='#000'"));assert!(!s.contains("<image"));}}
 #[test]fn overlays(){let s=overlay_icon("<svg></svg>".into(),&procedural_icon(3),20.,30.,12.).unwrap();assert!(s.contains("translate(20.000 30.000)"));}
}
