//! Jurisdictional landmark catalog. Empty template rows are not landmarks.
use serde::{Deserialize,Serialize};
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct CatalogEntry {pub jurisdiction:String,pub name:String,pub latitude:f64,pub longitude:f64,pub category:String,pub icon_id:String,pub rank:u32,pub source_url:String,pub license:String,pub verified:bool}
/// Import the bundled simple CSV schema. Quoted commas in names are not supported;
/// malformed lines are rejected rather than silently misinterpreted.
pub fn parse_catalog(input:&str)->Result<Vec<CatalogEntry>,String>{
 let mut result=Vec::new();
 for (line_no,line) in input.lines().enumerate().skip(1){
  if line.trim().is_empty(){continue}
  let fields:Vec<&str>=line.trim_end_matches('\r').split(',').collect();
  if fields.len()!=11{return Err(format!("Catalog row {}: expected 11 columns",line_no+1))}
  if fields[2].trim().is_empty(){continue} // Unfilled template slot.
  let latitude:f64=fields[3].parse().map_err(|_|format!("Row {} invalid latitude",line_no+1))?;
  let longitude:f64=fields[4].parse().map_err(|_|format!("Row {} invalid longitude",line_no+1))?;
  if !latitude.is_finite()||!longitude.is_finite()||latitude.abs()>90.||longitude.abs()>180.{return Err(format!("Row {} coordinates out of range",line_no+1))}
  let rank=fields[7].parse().map_err(|_|format!("Row {} invalid rank",line_no+1))?;
  result.push(CatalogEntry{jurisdiction:fields[1].into(),name:fields[2].into(),latitude,longitude,category:fields[5].into(),icon_id:fields[6].into(),rank,source_url:fields[8].into(),license:fields[9].into(),verified:fields[10].trim().eq_ignore_ascii_case("true")});
 }
 Ok(result)
}
pub fn filter_jurisdiction<'a>(items:&'a [CatalogEntry],jurisdiction:&str,verified_only:bool)->Vec<&'a CatalogEntry>{
 let mut out:Vec<_>=items.iter().filter(|e|e.jurisdiction.eq_ignore_ascii_case(jurisdiction)&&(!verified_only||e.verified)).collect();
 out.sort_by_key(|e|e.rank);out
}
#[cfg(test)]mod tests {use super::*;
 #[test]fn template_rows_are_not_landmarks(){let csv="jurisdiction_type,jurisdiction,name,latitude,longitude,category,icon_id,rank,source_url,license,verified\nstate,US-CA,,,,,generic-001,1,,,false\n";assert!(parse_catalog(csv).unwrap().is_empty());}
 #[test]fn imported_landmark(){let csv="header\nstate,US-CA,Example,38.5,-121.5,monument,generic-001,1,https://example.org,original,true\n";assert_eq!(filter_jurisdiction(&parse_catalog(csv).unwrap(),"US-CA",true).len(),1);}
}

/// Convert verified catalog coordinates to normalized coordinates inside a north-up
/// geographic engraving extent. Points outside the engraving are omitted.
pub fn positions_in_extent<'a>(items:&'a [CatalogEntry],west:f64,south:f64,east:f64,north:f64)->Result<Vec<(&'a CatalogEntry,f64,f64)>,String>{
 if ![west,south,east,north].iter().all(|v|v.is_finite()) || east<=west || north<=south {return Err("Invalid engraving extent".into())}
 Ok(items.iter().filter(|e|e.verified && e.longitude>=west && e.longitude<=east && e.latitude>=south && e.latitude<=north)
 .map(|e|(e,(e.longitude-west)/(east-west),(north-e.latitude)/(north-south))).collect())
}
/// Catalog icon identifiers are explicitly mapped to the bundled generic artwork.
/// Unknown or externally sourced artwork is never silently treated as a bundled icon.
pub fn bundled_icon_index(id:&str)->Option<usize>{
 let number=id.strip_prefix("generic-")?.parse::<usize>().ok()?;
 (1..=240).contains(&number).then_some(number-1)
}
#[cfg(test)]mod geographic_tests {use super::*;
 #[test]fn extent_and_verified_filter(){
 let csv="header\nstate,US-CA,Sample,38.5,-121.5,monument,generic-001,1,source,original,true\nstate,US-CA,Unverified,38.5,-121.5,monument,generic-002,2,source,original,false\n";
 let entries=parse_catalog(csv).unwrap();let points=positions_in_extent(&entries,-122.,38.,-121.,39.).unwrap();
 assert_eq!(points.len(),1);assert!((points[0].1-0.5).abs()<1e-9);assert_eq!(bundled_icon_index("generic-001"),Some(0));assert_eq!(bundled_icon_index("generic-999"),None);
 }
}
