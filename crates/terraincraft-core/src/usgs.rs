//! USGS 3DEP geographic selection and safe raster request construction.
//! Geographic coordinates are WGS84 degrees; longitude is X, latitude is Y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds { pub west:f64, pub south:f64, pub east:f64, pub north:f64 }
impl Bounds {
 pub fn validate(&self)->Result<(),String>{
  if [self.west,self.south,self.east,self.north].iter().any(|v|!v.is_finite()) || self.west < -180. || self.east > 180. || self.south < -90. || self.north > 90. || self.west >= self.east || self.south >= self.north {return Err("Invalid WGS84 bounds; antimeridian-spanning selections are not yet supported".into());}
  if self.east-self.west>5. || self.north-self.south>5. {return Err("Selection exceeds 5 degrees per axis; select a smaller region".into());}
  Ok(())
 }
 pub fn approx_aspect(&self)->f64 {let lat=(self.south+self.north)*0.5;((self.east-self.west)*lat.to_radians().cos().abs())/(self.north-self.south)}
 pub fn url(&self,width:u32,height:u32)->Result<String,String>{self.validate()?;if !(16..=2048).contains(&width)||!(16..=2048).contains(&height)|| (width as u64)*(height as u64)>2_000_000{return Err("Requested raster size outside supported limits".into())}
  Ok(format!("https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer/exportImage?bbox={},{},{},{}&bboxSR=4326&imageSR=4326&size={},{}&format=tiff&pixelType=F32&interpolation=RSP_Bilinear&f=image",self.west,self.south,self.east,self.north,width,height))
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn rejects_invalid(){assert!(Bounds{west:-120.,south:38.,east:-121.,north:39.}.validate().is_err());assert!(Bounds{west:170.,south:38.,east:190.,north:39.}.validate().is_err());}
 #[test]fn request(){let b=Bounds{west:-121.6,south:38.4,east:-121.4,north:38.6};assert!(b.url(512,512).unwrap().contains("bboxSR=4326"));assert!(b.url(4096,4096).is_err());assert!(b.approx_aspect()>0.7);}
}
