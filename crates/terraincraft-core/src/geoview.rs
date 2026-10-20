//! Geographic selection for north-up EPSG:4326 rasters with evenly spaced pixel centers.
//! This metadata is supplied by the acquisition workflow; generic TIFFs are NOT assumed georeferenced.
use crate::cartography::Viewpoint;
#[derive(Clone,Copy,Debug)]
pub struct GeographicExtent {pub west:f64,pub south:f64,pub east:f64,pub north:f64}
impl GeographicExtent {
 pub fn validate(self)->Result<(),String>{
  if [self.west,self.south,self.east,self.north].iter().any(|v|!v.is_finite()) || self.west>=self.east || self.south>=self.north || self.west< -180. || self.east>180. || self.south< -90. || self.north>90. || self.east-self.west>=180. {return Err("Invalid geographic bounds (antimeridian crossing not supported)".into())}Ok(())
 }
 /// Pixel centers are evenly distributed across the specified extent.
 pub fn point_to_pixel(self,latitude:f64,longitude:f64,width:usize,height:usize)->Result<(f64,f64),String>{
  self.validate()?;
  if width<2||height<2||!latitude.is_finite()||!longitude.is_finite()||longitude<self.west||longitude>self.east||latitude<self.south||latitude>self.north{return Err("Viewpoint is outside the geographic raster".into())}
  Ok(((longitude-self.west)/(self.east-self.west)*(width-1) as f64,(self.north-latitude)/(self.north-self.south)*(height-1) as f64))
 }
 pub fn pixel_scale_m(self,width:usize,height:usize,latitude:f64)->Result<(f64,f64),String>{
  self.validate()?;
  if width<2||height<2||latitude<self.south||latitude>self.north{return Err("Invalid raster dimensions or latitude".into())}
  let lat=latitude.to_radians();
  // WGS84 approximations of meters per degree at latitude.
  let meters_lat=111132.92-559.82*(2.*lat).cos()+1.175*(4.*lat).cos();
  let meters_lon=111412.84*lat.cos()-93.5*(3.*lat).cos()+0.118*(5.*lat).cos();
  Ok(((self.east-self.west)*meters_lon/(width-1) as f64,(self.north-self.south)*meters_lat/(height-1) as f64))
 }
 pub fn viewpoint(self,lat:f64,lon:f64,width:usize,height:usize,mut v:Viewpoint)->Result<Viewpoint,String>{
  let (x,y)=self.point_to_pixel(lat,lon,width,height)?;
  let (sx,sy)=self.pixel_scale_m(width,height,lat)?;
  v.x=x;v.y=y;v.pixel_size_x_m=sx;v.pixel_size_y_m=sy;Ok(v)
 }
}
#[cfg(test)]mod tests {use super::*;
 #[test]fn center_maps_to_center(){let e=GeographicExtent{west:-121.,south:38.,east:-120.,north:39.};let (x,y)=e.point_to_pixel(38.5,-120.5,101,101).unwrap();assert!((x-50.).abs()<1e-8&&(y-50.).abs()<1e-8);let (sx,sy)=e.pixel_scale_m(101,101,38.5).unwrap();assert!(sx>800.&&sx<1000.&&sy>1000.&&sy<1200.);}
 #[test]fn rejects_outside(){let e=GeographicExtent{west:-121.,south:38.,east:-120.,north:39.};assert!(e.point_to_pixel(40.,-120.5,20,20).is_err());}
}
