//! TerrainCraft v0.9: native Rust egui desktop inspection and manufacturing export.
//! Deliberately no machine control or G-code generation.
mod gpu_view;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::{fs, path::Path, io::Cursor, sync::mpsc, thread, collections::{HashMap,HashSet}};
use terraincraft_core::usgs::Bounds;
use terraincraft_core::engraving_preview;
use terraincraft_core::engraving_editor::{Annotation, Preset};
use terraincraft_core::cartography::{self, Viewpoint, EngravingOptions, EngravingStyle};
use terraincraft_core::geoview::GeographicExtent;
use tiff::decoder::{Decoder, DecodingResult};
use serde::{Serialize, Deserialize};
use terraincraft_core::{Dem, TerrainSettings, tiles, mesh};
use terraincraft_core::laser::{cut_parts, layout, sheet_svg, split_oversized, CutPart, Placement};

#[derive(PartialEq, Clone, Copy, Serialize, Deserialize)]
enum View { Terrain, Print, Laser, Engraving }
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Studio {
    path: String, output: String, #[serde(skip)] dem: Option<Dem>, message: String,
    width: f64, height: f64, base: f64, relief: f64, exaggeration: f64,
    bed_x: f64, bed_y: f64, margin: f64,
    sheet_x: f64, sheet_y: f64, gap: f64, layer_count: usize, material: f64,
    view: View, sheet: usize,
    engraving_perspective: bool, engraving_style: usize, engraving_title: String, engraving_subtitle: String, engraving_font: String, contour_interval: f64, observer_x: f64, observer_y: f64, bearing: f64, pitch: f64, fov: f64, eye_height: f64, max_distance: f64, pixel_size_m: f64, label_candidate_peaks: bool, peak_prominence_m: f64, peak_label_limit: usize, geo_view_enabled: bool, observer_lat: f64, observer_lon: f64, depth_bands: usize, show_geo_map: bool, landmark_csv: String, show_landmarks: bool,
    assembly_exploded: bool, assembly_gap_mm: f64, show_assembly_labels: bool, assembly_mesh_mode: bool, assembly_shaded: bool, assembly_gpu: bool, assembly_connector_mode: usize,
    #[serde(skip)] orbit_yaw: f32,
    #[serde(skip)] orbit_pitch: f32,
    #[serde(skip)] orbit_zoom: f32,
    annotations: Vec<Annotation>, engraving_preset: usize, icon_id:usize, cultural_icon_id:String, regional_icon_id:String, regional_style:usize, icon_variant:usize, logo_catalog_path:String, logo_search:String, selected_logo_id:String, unesco_catalog_path:String, unesco_search:String, selected_unesco_svg:String, unesco_simplified:bool, icon_x:f64, icon_y:f64, icon_size:f64, map_provider:usize, map_url:String, catalog_path:String, catalog_jurisdiction:String, catalog_verified_only:bool, catalog_auto_icons:bool,
    project_path: String,
    west:f64, south:f64, east:f64, north:f64, raster_pixels:u32,
    #[serde(skip)] download_rx:Option<mpsc::Receiver<Result<Vec<u8>,String>>>,
    map_zoom:u8, map_markers:bool, map_marker_limit:usize,
    #[serde(skip)] map_results:Option<mpsc::Receiver<(String,Result<Vec<u8>,String>)>>,
    #[serde(skip)] map_textures:HashMap<String,egui::TextureHandle>,
    #[serde(skip)] map_pending:HashSet<String>,
    #[serde(skip)] map_requested:HashSet<String>,
    #[serde(skip)] map_tx:Option<mpsc::Sender<(String,Result<Vec<u8>,String>)>>,
    #[serde(skip)] map_drag_origin:Option<(f64,f64)>,
    #[serde(skip)] map_extent_first:Option<(f64,f64)>,
}
impl Default for Studio {
 fn default()->Self {Self{path:"examples/demo.csv".into(),output:"output".into(),dem:None,message:"Load an elevation grid to begin".into(),width:400.,height:300.,base:3.,relief:25.,exaggeration:2.,bed_x:220.,bed_y:220.,margin:5.,sheet_x:600.,sheet_y:400.,gap:3.,layer_count:8,material:3.,view:View::Terrain,sheet:1,engraving_perspective:true,engraving_style:0,engraving_title:"Mountain panorama".into(),engraving_subtitle:String::new(),engraving_font:"serif".into(),contour_interval:20.,observer_x:0.5,observer_y:0.5,bearing:90.,pitch:0.,fov:90.,eye_height:1.7,max_distance:40000.,pixel_size_m:30.,label_candidate_peaks:true,peak_prominence_m:5.,peak_label_limit:12,geo_view_enabled:false,observer_lat:38.58,observer_lon:-121.50,depth_bands:1,show_geo_map:true,landmark_csv:String::new(),show_landmarks:true,assembly_exploded:true,assembly_gap_mm:12.,show_assembly_labels:true,assembly_mesh_mode:false,assembly_shaded:true,assembly_gpu:false,assembly_connector_mode:0,orbit_yaw:0.65,orbit_pitch:0.65,orbit_zoom:1.,annotations:Vec::new(),engraving_preset:0,icon_id:0,cultural_icon_id:String::new(),regional_icon_id:String::new(),regional_style:0,icon_variant:0,logo_catalog_path:"assets/institution_logo_catalog.json".into(),logo_search:String::new(),selected_logo_id:String::new(),unesco_catalog_path:"assets/unesco_whc/catalog.json".into(),unesco_search:String::new(),selected_unesco_svg:String::new(),unesco_simplified:false,icon_x:0.5,icon_y:0.5,icon_size:12.,map_provider:0,map_url:String::new(),catalog_path:"assets/landmark_catalog_template.csv".into(),catalog_jurisdiction:"US-CA".into(),catalog_verified_only:true,catalog_auto_icons:false,project_path:"terraincraft-project.json".into(),west:-121.65,south:38.48,east:-121.35,north:38.68,raster_pixels:512,download_rx:None,map_zoom:10,map_markers:true,map_marker_limit:100,map_results:None,map_textures:HashMap::new(),map_pending:HashSet::new(),map_requested:HashSet::new(),map_tx:None,map_drag_origin:None,map_extent_first:None}}
}
fn decode_tiff(bytes:&[u8])->Result<Dem,String>{
 let mut decoder=Decoder::new(Cursor::new(bytes)).map_err(|e|format!("TIFF decoder: {e}"))?;
 let (w,h)=decoder.dimensions().map_err(|e|e.to_string())?;
 if w<2||h<2||(w as u64)*(h as u64)>2_000_000{return Err("Invalid raster dimensions".into())}
 let values:Vec<f64>=match decoder.read_image().map_err(|e|e.to_string())?{
  DecodingResult::F32(v)=>v.into_iter().map(f64::from).collect(),
  DecodingResult::F64(v)=>v,
  DecodingResult::U8(v)=>v.into_iter().map(f64::from).collect(),
  DecodingResult::U16(v)=>v.into_iter().map(f64::from).collect(),
  DecodingResult::I16(v)=>v.into_iter().map(f64::from).collect(),
  DecodingResult::U32(v)=>v.into_iter().map(f64::from).collect(),
  DecodingResult::I32(v)=>v.into_iter().map(f64::from).collect(),
  _=>return Err("Unsupported elevation pixel type".into())
 };
 Dem::new(w as usize,h as usize,values)
}
impl Studio {
 fn request_map_tile(&mut self)->Result<(),String>{
  self.queue_visible_tiles(540.,320.)
 }
 fn queue_visible_tiles(&mut self,width:f32,height:f32)->Result<(),String>{
  let providers=terraincraft_core::map_providers::builtins();
  let mut provider=providers[self.map_provider.min(providers.len()-1)].clone();
  if self.map_provider==2 {provider.url_template=self.map_url.clone();}
  let (wx,wy)=terraincraft_core::map_providers::latlon_to_world(self.observer_lat,self.observer_lon,self.map_zoom);
  let count=(1_u64<<self.map_zoom) as f64;
  let cx=wx*count;let cy=wy*count;
  let left=(cx-width as f64/512.).floor() as i64;
  let right=(cx+width as f64/512.).floor() as i64;
  let top=(cy-height as f64/512.).floor() as i64;
  let bottom=(cy+height as f64/512.).floor() as i64;
  if self.map_results.is_none(){let(tx,rx)=mpsc::channel();self.map_tx=Some(tx);self.map_results=Some(rx);}
  let cache=std::env::temp_dir().join("terraincraft-map-cache-v4.9");
  let provider_identity=format!("{}:{}",provider.name,provider.url_template);
  let provider_hash=provider_identity.bytes().fold(14695981039346656037_u64,|h,b|(h^b as u64).wrapping_mul(1099511628211));
  let mut scheduled=0;
  for y in top..=bottom {if y<0||y>=count as i64 {continue}
   for x in left..=right {if scheduled>=12 {break}
    let wrapped=x.rem_euclid(count as i64) as u32;
    let key=format!("{:016x}-{}-{}-{}",provider_hash,self.map_zoom,wrapped,y);
    if self.map_textures.contains_key(&key)||self.map_pending.contains(&key)||self.map_requested.contains(&key){continue}
    let url=provider.xyz_url(self.map_zoom,wrapped,y as u32)?;
    let path=cache.join(format!("{key}.png"));
    let tx=self.map_tx.as_ref().unwrap().clone();
    self.map_pending.insert(key.clone());self.map_requested.insert(key.clone());scheduled+=1;
    let cache=cache.clone();
    thread::spawn(move||{
     let result=(||->Result<Vec<u8>,String>{
      if let Ok(meta)=fs::metadata(&path){if meta.modified().ok().and_then(|t|t.elapsed().ok()).map(|age|age.as_secs()<7*86400).unwrap_or(false){return fs::read(&path).map_err(|e|e.to_string());}}
      let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20))
       .user_agent("TerrainCraft/4.9 (interactive terrain mapping; contact: local desktop user)")
       .build().map_err(|e|e.to_string())?;
      let response=client.get(url).send().map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
      if response.content_length().unwrap_or(0)>2_000_000{return Err("Tile too large".into())}
      let bytes=response.bytes().map_err(|e|e.to_string())?;
      if bytes.len()>2_000_000{return Err("Tile too large".into())}
      fs::create_dir_all(&cache).map_err(|e|e.to_string())?;
      fs::write(path,&bytes).map_err(|e|e.to_string())?;
      Ok(bytes.to_vec())
     })();let _=tx.send((key,result));
    });
   }
  }
  Ok(())
 }
 fn map_canvas(&mut self,ui:&mut egui::Ui){
  ui.label("Map navigation: drag to pan · wheel to zoom · Shift-click twice to define the terrain extent");
  let size=egui::vec2(ui.available_width().min(540.).max(180.),320.);
  let (rect,response)=ui.allocate_exact_size(size,egui::Sense::click_and_drag());
  let painter=ui.painter_at(rect);
  painter.rect_filled(rect,0.,Color32::from_rgb(33,43,52));
  let (wx,wy)=terraincraft_core::map_providers::latlon_to_world(self.observer_lat,self.observer_lon,self.map_zoom);
  let count=(1_u64<<self.map_zoom) as f64;
  let center_x=wx*count;let center_y=wy*count;
  let left=(center_x-size.x as f64/512.).floor() as i64;
  let right=(center_x+size.x as f64/512.).floor() as i64;
  let top=(center_y-size.y as f64/512.).floor() as i64;
  let bottom=(center_y+size.y as f64/512.).floor() as i64;
  let providers=terraincraft_core::map_providers::builtins();
  let mut provider=providers[self.map_provider.min(providers.len()-1)].clone();
  if self.map_provider==2 {provider.url_template=self.map_url.clone();}
  let identity=format!("{}:{}",provider.name,provider.url_template);
  let hash=identity.bytes().fold(14695981039346656037_u64,|h,b|(h^b as u64).wrapping_mul(1099511628211));
  for y in top..=bottom {if y<0||y>=count as i64 {continue}
   for x in left..=right {
    let key=format!("{:016x}-{}-{}-{}",hash,self.map_zoom,x.rem_euclid(count as i64),y);
    if let Some(tex)=self.map_textures.get(&key){
     let px=rect.center().x+(x as f64-center_x) as f32*256.;
     let py=rect.center().y+(y as f64-center_y) as f32*256.;
     let target=Rect::from_min_size(egui::pos2(px,py),egui::vec2(256.,256.));
     if let Some(clipped)=target.intersect(rect).is_positive().then(||target.intersect(rect)){
      let uv=Rect::from_min_max(egui::pos2((clipped.left()-target.left())/256.,(clipped.top()-target.top())/256.),egui::pos2((clipped.right()-target.left())/256.,(clipped.bottom()-target.top())/256.));
      painter.image(tex.id(),clipped,uv,Color32::WHITE);
     }
    }
   }
  }
  if let Err(e)=self.queue_visible_tiles(size.x,size.y){self.message=format!("Map: {e}");}
  // Overlay catalog points in the same Web Mercator coordinate system as tiles.
  // Marker labels are intentionally capped to keep large imported catalogs responsive.
  let mut visible_markers:Vec<(Pos2,String,f64,f64,usize)>=Vec::new();
  if self.map_markers {
   if let Ok(csv)=fs::read_to_string(&self.catalog_path){
    if let Ok(entries)=terraincraft_core::landmark_catalog::parse_catalog(&csv){
     let matching=terraincraft_core::landmark_catalog::filter_jurisdiction(&entries,&self.catalog_jurisdiction,self.catalog_verified_only);
     for entry in matching.into_iter().take(self.map_marker_limit.min(500)) {
      let (mx,my)=terraincraft_core::map_providers::latlon_to_world(entry.latitude,entry.longitude,self.map_zoom);
      let dx=(mx-wx+0.5).rem_euclid(1.)-0.5;
      let position=egui::pos2(rect.center().x+(dx*count*256.) as f32,rect.center().y+((my-wy)*count*256.) as f32);
      if rect.contains(position){
       painter.circle_filled(position,5.,Color32::from_rgb(255,197,50));
       painter.circle_stroke(position,5.,Stroke::new(1.,Color32::BLACK));
       visible_markers.push((position,entry.name.clone(),entry.latitude,entry.longitude,terraincraft_core::landmark_catalog::bundled_icon_index(&entry.icon_id).unwrap_or(self.icon_id)));
      }
     }
    }
   }
  }
  painter.rect_stroke(rect,0.,Stroke::new(1.,Color32::GRAY));
  let cx=rect.center().x;let cy=rect.center().y;
  painter.line_segment([egui::pos2(cx-8.,cy),egui::pos2(cx+8.,cy)],Stroke::new(1.5,Color32::YELLOW));
  painter.line_segment([egui::pos2(cx,cy-8.),egui::pos2(cx,cy+8.)],Stroke::new(1.5,Color32::YELLOW));
  if response.drag_started(){self.map_drag_origin=Some((self.observer_lat,self.observer_lon));}
  if response.drag_stopped(){
   if let Some((lat,lon))=self.map_drag_origin.take(){
    let d=response.drag_delta();
    let scale=256.*(2_f64.powi(self.map_zoom as i32));
    let (x,y)=terraincraft_core::map_providers::latlon_to_world(lat,lon,self.map_zoom);
    let (newlat,newlon)=terraincraft_core::map_providers::world_to_latlon(x-d.x as f64/scale,y-d.y as f64/scale);
    self.observer_lat=newlat;self.observer_lon=newlon;
    let _=self.request_map_tile();
   }
  }
  if response.hovered(){let scroll=ui.input(|i|i.raw_scroll_delta.y);
   if scroll.abs()>2. {let next=(self.map_zoom as i32+if scroll>0.{1}else{-1}).clamp(0,17) as u8;
    if next!=self.map_zoom {self.map_zoom=next;let _=self.request_map_tile();}
   }
  }
  if response.clicked() {
   if let Some(pos)=response.interact_pointer_pos(){
    let (x,y)=terraincraft_core::map_providers::latlon_to_world(self.observer_lat,self.observer_lon,self.map_zoom);
    let scale=256.*(2_f64.powi(self.map_zoom as i32));
    let (lat,lon)=terraincraft_core::map_providers::world_to_latlon(x+(pos.x-cx) as f64/scale,y+(pos.y-cy) as f64/scale);
    // Clicking a marker selects its icon and observer coordinates for engraving.
    if !ui.input(|i|i.modifiers.shift) {
     if let Some((_,name,mlat,mlon,icon))=visible_markers.iter().filter(|(p,_,_,_,_)|p.distance(pos)<12.).min_by(|a,b|a.0.distance(pos).total_cmp(&b.0.distance(pos))) {
      self.observer_lat=*mlat;self.observer_lon=*mlon;self.icon_id=*icon;
      self.message=format!("Selected landmark: {name}");self.geo_view_enabled=true;
      return;
     }
    }
    let shift=ui.input(|i|i.modifiers.shift);
    if shift {if let Some((a,b))=self.map_extent_first.take(){self.south=a.min(lat);self.north=a.max(lat);self.west=b.min(lon);self.east=b.max(lon);self.message="Terrain extent updated from map corners".into();}else{self.map_extent_first=Some((lat,lon));self.message="Select the opposite extent corner with Shift-click".into();}}
    else{self.observer_lat=lat;self.observer_lon=lon;self.geo_view_enabled=true;let _=self.request_map_tile();}
   }
  }
  ui.small(format!("Center: {:.5}°, {:.5}° | Zoom {} | Extent: {:.4},{:.4} to {:.4},{:.4}",self.observer_lat,self.observer_lon,self.map_zoom,self.west,self.south,self.east,self.north));
 }
 fn save_project(&self)->Result<String,String>{
  let json=serde_json::to_string_pretty(self).map_err(|e|e.to_string())?;
  fs::write(&self.project_path,json).map_err(|e|e.to_string())?;
  Ok(format!("Saved project to {}",self.project_path))
 }
 fn open_project(&mut self)->Result<String,String>{
  let json=fs::read_to_string(&self.project_path).map_err(|e|e.to_string())?;
  let mut project:Studio=serde_json::from_str(&json).map_err(|e|e.to_string())?;
  project.project_path=self.project_path.clone();
  project.load()?;
  *self=project;
  Ok("Project loaded (including elevation source)".into())
 }
 fn begin_download(&mut self)->Result<(),String>{
  if self.download_rx.is_some(){return Err("A download is already running".into())}
  let bounds=Bounds{west:self.west,south:self.south,east:self.east,north:self.north};
  let url=bounds.url(self.raster_pixels,self.raster_pixels)?;
  let (tx,rx)=mpsc::channel();self.download_rx=Some(rx);
  self.message="Downloading USGS 3DEP elevation data…".into();
  thread::spawn(move||{
   let result=(||->Result<Vec<u8>,String>{
    let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(90)).build().map_err(|e|e.to_string())?;
    let response=client.get(url).send().map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
    if let Some(len)=response.content_length(){if len>60_000_000{return Err("USGS response exceeds 60 MB".into())}}
    let bytes=response.bytes().map_err(|e|e.to_string())?;
    if bytes.len()>60_000_000{return Err("USGS response exceeds 60 MB".into())}
    if bytes.len()<4 || !matches!(&bytes[..4],b"II*\0"|b"MM\0*"){return Err("USGS did not return a classic TIFF; check service availability".into())}
    Ok(bytes.to_vec())
   })();let _=tx.send(result);
  });Ok(())
 }
 fn finish_download(&mut self,bytes:Vec<u8>)->Result<(),String>{
  let dem=decode_tiff(&bytes)?;let (w,h)=(dem.width,dem.height);
  fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
  let file=Path::new(&self.output).join("usgs_elevation.tif");
  fs::write(&file,bytes).map_err(|e|e.to_string())?;
  self.path=file.to_string_lossy().into_owned();self.dem=Some(dem);
  self.message=format!("Downloaded USGS elevation grid {} × {}. Source saved to {}",w,h,self.path);
  Ok(())
 }
 fn settings(&self)->TerrainSettings {TerrainSettings{width_mm:self.width,height_mm:self.height,base_mm:self.base,relief_mm:self.relief,vertical_exaggeration:self.exaggeration}}
 fn load(&mut self)->Result<(),String>{
  let bytes=fs::read(&self.path).map_err(|e|e.to_string())?;
  let suffix=Path::new(&self.path).extension().and_then(|v|v.to_str()).unwrap_or("").to_ascii_lowercase();
  let dem=match suffix.as_str(){
   "tif"|"tiff"=>decode_tiff(&bytes)?,
   "asc"=>terraincraft_core::gis::read_asc(&String::from_utf8(bytes).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?.dem,
   "csv"=>{let text=String::from_utf8(bytes).map_err(|e|e.to_string())?;let rows=text.lines().filter(|v|!v.trim().is_empty()).map(|line|line.split(',').map(|v|v.trim().parse::<f64>().map_err(|e|e.to_string())).collect::<Result<Vec<_>,_>>()).collect::<Result<Vec<_>,_>>()?;
    let w=rows.first().ok_or("Empty CSV")?.len();let h=rows.len();Dem::new(w,h,rows.into_iter().flatten().collect())?},
   _=>return Err("Supported input formats: CSV, ESRI ASCII (.asc), uncompressed TIFF".into()),
  };
  self.message=format!("Loaded {} × {} elevation samples",dem.width,dem.height);self.dem=Some(dem);Ok(())
 }
 fn levels(&self,dem:&Dem)->Vec<f64>{let(lo,hi)=dem.min_max();(1..=self.layer_count).map(|i|lo+(hi-lo)*i as f64/(self.layer_count+1) as f64).collect()}
 fn laser_data(&self,dem:&Dem)->Result<(Vec<CutPart>,Vec<Placement>),String>{
  let parts=cut_parts(dem,self.width,self.height,&self.levels(dem))?;
  let parts=split_oversized(&parts,self.sheet_x-2.*self.gap,self.sheet_y-2.*self.gap)?;
  let placements=layout(&parts,self.sheet_x,self.sheet_y,self.gap)?;
  terraincraft_core::inspection::preflight(&parts,&placements,self.sheet_x,self.sheet_y,self.gap)?;
  Ok((parts,placements))
 }
 fn engraving_svg(&self,dem:&Dem)->Result<String,String>{
  let style=match self.engraving_style {1=>EngravingStyle::Contour,2=>EngravingStyle::Illustrated,_=>EngravingStyle::Ridgeline};
  let opts=EngravingOptions{width_mm:self.width,height_mm:self.height,contour_interval_m:self.contour_interval,style,title:self.engraving_title.clone(),subtitle:self.engraving_subtitle.clone(),font_family:self.engraving_font.clone(),show_coordinates:false};
  if self.engraving_perspective {
   let mut vp=Viewpoint{x:self.observer_x*(dem.width-1) as f64,y:self.observer_y*(dem.height-1) as f64,bearing_deg:self.bearing,pitch_deg:self.pitch,fov_deg:self.fov,eye_height_m:self.eye_height,max_distance_m:self.max_distance,pixel_size_x_m:self.pixel_size_m,pixel_size_y_m:self.pixel_size_m};
   if self.geo_view_enabled {
    // The selected USGS bounding box is known to use EPSG:4326 and north-up pixels.
    // Imported files must NOT be assumed to share this metadata.
    if !self.path.ends_with("usgs_elevation.tif") {return Err("Geographic viewpoint requires a USGS raster downloaded in this project".into())}
    let extent=GeographicExtent{west:self.west,south:self.south,east:self.east,north:self.north};
    vp=extent.viewpoint(self.observer_lat,self.observer_lon,dem.width,dem.height,vp)?;
   }
   let mut svg=if self.depth_bands>=2 {cartography::panorama_depth_svg(dem,vp,&opts,self.depth_bands)?} else {cartography::panorama_svg(dem,vp,&opts)?};
   if self.label_candidate_peaks && self.depth_bands==1 {svg=cartography::panorama_with_peaks_svg(dem,vp,&opts,self.peak_prominence_m,self.peak_label_limit)?;}
   if self.show_landmarks && !self.landmark_csv.trim().is_empty() {
    let features=terraincraft_core::landmarks::read_landmarks(&fs::read_to_string(&self.landmark_csv).map_err(|e|e.to_string())?)?;
    if !self.geo_view_enabled {return Err("Perspective geographic landmarks require a georeferenced USGS viewpoint".into());}
    svg=terraincraft_core::landmarks::overlay_perspective(svg,&features,GeographicExtent{west:self.west,south:self.south,east:self.east,north:self.north},dem,vp,self.width,self.height)?;
   }
   {let svg=terraincraft_core::engraving_editor::overlay(svg,&self.annotations,self.width,self.height,self.preset())?;terraincraft_core::icon_library::overlay_icon(svg,&self.selected_icon_svg().as_str(),self.icon_x*self.width,self.icon_y*self.height,self.icon_size)}
  }else{
    let mut svg=cartography::topdown_svg(dem,&opts)?;
    if self.show_landmarks && !self.landmark_csv.trim().is_empty() {
     let features=terraincraft_core::landmarks::read_landmarks(&fs::read_to_string(&self.landmark_csv).map_err(|e|e.to_string())?)?;
     svg=terraincraft_core::landmarks::overlay_topdown(svg,&features,GeographicExtent{west:self.west,south:self.south,east:self.east,north:self.north},self.width,self.height)?;
    }
    if self.catalog_auto_icons {
    let entries=terraincraft_core::landmark_catalog::parse_catalog(&fs::read_to_string(&self.catalog_path).map_err(|e|format!("Catalog: {e}"))?)?;
    let selected=terraincraft_core::landmark_catalog::filter_jurisdiction(&entries,&self.catalog_jurisdiction,true);
    let selected:Vec<_>=selected.into_iter().cloned().collect();
    for (entry,x,y) in terraincraft_core::landmark_catalog::positions_in_extent(&selected,self.west,self.south,self.east,self.north)? {
     if let Some(index)=terraincraft_core::landmark_catalog::bundled_icon_index(&entry.icon_id) {
      svg=terraincraft_core::icon_library::overlay_icon(svg,&terraincraft_core::icon_library::procedural_icon(index),x*self.width,y*self.height,self.icon_size)?;
     }
    }
   }
   {let svg=terraincraft_core::engraving_editor::overlay(svg,&self.annotations,self.width,self.height,self.preset())?;terraincraft_core::icon_library::overlay_icon(svg,&self.selected_icon_svg().as_str(),self.icon_x*self.width,self.icon_y*self.height,self.icon_size)}
  }
 }
 fn selected_icon_svg(&self)->String {
  if !self.selected_unesco_svg.is_empty() {
   let base=std::path::Path::new(&self.unesco_catalog_path).parent().unwrap_or(std::path::Path::new("."));
   let selected=if self.unesco_simplified && !self.selected_unesco_svg.ends_with("--simplified.svg") {
    self.selected_unesco_svg.replace(".svg","--simplified.svg")
   } else if !self.unesco_simplified {self.selected_unesco_svg.replace("--simplified.svg",".svg")}
   else {self.selected_unesco_svg.clone()};
   let path=base.join(selected);
   if let Ok(svg)=fs::read_to_string(path) {return svg;}
  }
  if !self.regional_icon_id.is_empty() {
   let styles=["original","simplified","profile","three-quarter"];
   let style=styles[self.regional_style.min(styles.len()-1)];
   if let Ok(svg)=terraincraft_core::regional_icons::read_svg(std::path::Path::new("."),&self.regional_icon_id,style){return svg;}
  }
  let style=terraincraft_core::icon_variants::IconStyle::from_index(self.icon_variant);
  let is_temple=self.cultural_icon_id=="us-ut-salt-lake-temple";
  if !self.selected_logo_id.is_empty() {
   let catalog=std::path::Path::new(&self.logo_catalog_path);
   if let Ok(records)=terraincraft_core::institution_logos::read_catalog(catalog) {
    if let Some(record)=records.iter().find(|r|r.id==self.selected_logo_id) {
     if let Ok(svg)=terraincraft_core::institution_logos::load_svg(record,catalog.parent().unwrap_or(std::path::Path::new("."))) {return terraincraft_core::icon_variants::apply(&svg,style,false);}
    }
   }
  }
  let svg=terraincraft_core::state_culture::svg(&self.cultural_icon_id)
   .unwrap_or_else(||terraincraft_core::icon_library::procedural_icon(self.icon_id));
  terraincraft_core::icon_variants::apply(&svg,style,is_temple)
 }
 fn preset(&self)->Preset{match self.engraving_preset {1=>Preset::Minimal,2=>Preset::Vintage,3=>Preset::Cartoon,_=>Preset::Survey}}
 fn export(&self)->Result<String,String>{
  let dem=self.dem.as_ref().ok_or("Load a terrain first")?;
  self.settings().validate()?;
  fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
  match self.view {
   View::Engraving=>{let svg=self.engraving_svg(dem)?;let filename=if self.engraving_perspective{"perspective_engraving.svg"}else{"topdown_engraving.svg"};fs::write(Path::new(&self.output).join(filename),svg).map_err(|e|e.to_string())?;Ok(format!("Exported {}",filename))},
   View::Print=>{let ts=tiles(dem,self.settings(),self.bed_x,self.bed_y,self.margin)?;

    fs::write(Path::new(&self.output).join("print_tile_manifest.csv"),terraincraft_core::print_export::tile_manifest(dem,self.settings(),&ts)?).map_err(|e|e.to_string())?;
    let reg=terraincraft_core::registration::RegistrationSettings::default();
    let guides=terraincraft_core::registration::guides(dem,self.settings(),&ts,reg)?;
    let holes=if self.assembly_connector_mode==2 {terraincraft_core::seam_joints::plan(dem,&ts,self.settings(),terraincraft_core::seam_joints::SeamSettings::default())?}else{Vec::new()};
    if self.assembly_connector_mode==2 {
     let checks=terraincraft_core::seam_preflight::inspect(dem,self.settings(),&ts,&holes)?;
     fs::write(Path::new(&self.output).join("seam_socket_preflight.csv"),terraincraft_core::seam_preflight::csv(&checks)).map_err(|e|e.to_string())?;
     if checks.iter().any(|c|!c.compatible){return Err("Sidewall socket preflight failed; see seam_socket_preflight.csv".into());}
    }
    let mut audit_csv=String::from(terraincraft_core::mesh_audit::CSV_HEADER);
    for t in &ts {let name=format!("tile_r{:02}_c{:02}.stl",t.row+1,t.col+1);
      let faces=self.tile_print_mesh(dem,*t,&holes)?;
      let audit=terraincraft_core::mesh_audit::audit(&faces);
      audit_csv.push_str(&audit.csv_row(&name));
      terraincraft_core::print_export::write_binary_stl(&Path::new(&self.output).join(name),&faces).map_err(|e|e.to_string())?;
    }
    fs::write(Path::new(&self.output).join("mesh_audit.csv"),audit_csv).map_err(|e|e.to_string())?;
    for g in &guides {fs::write(Path::new(&self.output).join(format!("registration_r{:02}_c{:02}.svg",g.row+1,g.col+1)),terraincraft_core::registration::guide_svg(g,reg)?).map_err(|e|e.to_string())?;}
    fs::write(Path::new(&self.output).join("registration_manifest.csv"),terraincraft_core::registration::guide_manifest(&guides,reg)).map_err(|e|e.to_string())?;
    terraincraft_core::print_export::write_binary_stl(&Path::new(&self.output).join("registration_pin.stl"),&terraincraft_core::registration::pin_mesh(reg,32)?).map_err(|e|e.to_string())?;

    Ok(format!("Exported {} STL tiles and mesh audit to {} (verify audit before printing)",ts.len(),self.output))},
   View::Laser=>{let(parts,placements)=self.laser_data(&dem)?;
    let count=placements.iter().map(|p|p.sheet).max().unwrap_or(0);
    for n in 1..=count {fs::write(Path::new(&self.output).join(format!("laser_sheet_{n:02}.svg")),sheet_svg(&parts,&placements,n,self.sheet_x,self.sheet_y)).map_err(|e|e.to_string())?;}
    let mut manifest=String::from("part_id,level,sheet,x_mm,y_mm,width_mm,height_mm\n");
    for p in &placements {let level=parts.iter().find(|v|v.id==p.part_id).map(|v|v.level).unwrap_or(0.);manifest.push_str(&format!("{},{},{},{},{},{},{}\n",p.part_id,level,p.sheet,p.x_mm,p.y_mm,p.width_mm,p.height_mm));}
    fs::write(Path::new(&self.output).join("laser_manifest.csv"),manifest).map_err(|e|e.to_string())?;
    fs::write(Path::new(&self.output).join("laser_layout_report.html"),terraincraft_core::inspection::report_html(&parts,&placements,self.sheet_x,self.sheet_y)).map_err(|e|e.to_string())?;
    Ok(format!("Exported {} parts on {} laser sheets",parts.len(),count))},
   View::Terrain=>{let(lo,hi)=dem.min_max();for (i,level) in self.levels(dem).iter().enumerate(){fs::write(Path::new(&self.output).join(format!("contour_{:02}.svg",i+1)),terraincraft_core::contours::svg(dem,self.width,self.height,*level)?).map_err(|e|e.to_string())?;}
    Ok(format!("Exported {} contour SVGs (elevation {:.2}–{:.2})",self.layer_count,lo,hi))}
  }
 }
 fn draw_geo_map(&mut self,ui:&mut egui::Ui){
  ui.label("Geographic selection canvas (coordinate grid, not a street basemap). Click to position observer; drag from observer to set bearing.");
  let available=ui.available_size();
  let side=available.x.min(available.y*1.6).max(100.);
  let (rect,response)=ui.allocate_exact_size(Vec2::new(side,side*0.65),Sense::click_and_drag());
  let painter=ui.painter_at(rect);
  painter.rect_filled(rect,4.,Color32::from_rgb(28,43,49));
  for i in 0..=10 {let f=i as f32/10.;let x=rect.left()+f*rect.width();let y=rect.top()+f*rect.height();
   painter.line_segment([Pos2::new(x,rect.top()),Pos2::new(x,rect.bottom())],Stroke::new(0.5,Color32::DARK_GRAY));
   painter.line_segment([Pos2::new(rect.left(),y),Pos2::new(rect.right(),y)],Stroke::new(0.5,Color32::DARK_GRAY));
  }
  let extent=GeographicExtent{west:self.west,south:self.south,east:self.east,north:self.north};
  if extent.validate().is_err(){ui.colored_label(Color32::LIGHT_RED,"Invalid geographic bounds");return;}
  let to_pos=|lat:f64,lon:f64| Pos2::new(rect.left()+((lon-extent.west)/(extent.east-extent.west)) as f32*rect.width(),rect.top()+((extent.north-lat)/(extent.north-extent.south)) as f32*rect.height());
  if response.clicked(){if let Some(pos)=response.interact_pointer_pos(){self.observer_lon=extent.west+((pos.x-rect.left())/rect.width()) as f64*(extent.east-extent.west);self.observer_lat=extent.north-((pos.y-rect.top())/rect.height()) as f64*(extent.north-extent.south);self.geo_view_enabled=true;}}
  if response.dragged(){if let Some(pos)=response.interact_pointer_pos(){let origin=to_pos(self.observer_lat,self.observer_lon);let dx=(pos.x-origin.x) as f64;let dy=(origin.y-pos.y) as f64;if dx.abs()+dy.abs()>4. {self.bearing=dx.atan2(dy).to_degrees().rem_euclid(360.);}}}
  let origin=to_pos(self.observer_lat,self.observer_lon);
  if rect.contains(origin){
   let length=rect.width().min(rect.height())*0.32;
   let left=(self.bearing-self.fov*0.5).to_radians();let right=(self.bearing+self.fov*0.5).to_radians();
   for a in [left,right]{let end=Pos2::new(origin.x+length*a.sin() as f32,origin.y-length*a.cos() as f32);painter.line_segment([origin,end],Stroke::new(1.4,Color32::YELLOW));}
   painter.circle_filled(origin,5.,Color32::LIGHT_GREEN);
  }
  painter.text(rect.left_top()+Vec2::new(7.,7.),egui::Align2::LEFT_TOP,format!("N ↑  {:.5}°, {:.5}°",self.observer_lat,self.observer_lon),egui::FontId::monospace(12.),Color32::WHITE);
 }
 fn draw_3d(&mut self,ui:&mut egui::Ui,dem:&Dem){
  ui.label("Drag to orbit · Scroll to zoom · Terrain mesh preview (not G-code)");
  let available=ui.available_size();
  let (rect,response)=ui.allocate_exact_size(available,Sense::drag());
  if response.dragged(){let delta=response.drag_delta();self.orbit_yaw+=delta.x*0.008;self.orbit_pitch=(self.orbit_pitch+delta.y*0.008).clamp(0.1,1.45);}
  if response.hovered(){let scroll=ui.input(|i|i.raw_scroll_delta.y);self.orbit_zoom=(self.orbit_zoom*(scroll*0.001).exp()).clamp(0.2,8.);}
  if self.assembly_gpu {
   let parts=match tiles(dem,self.settings(),self.bed_x,self.bed_y,self.margin){Ok(p)=>p,Err(e)=>{ui.painter_at(rect).text(rect.center(),egui::Align2::CENTER_CENTER,e,egui::FontId::proportional(14.),Color32::LIGHT_RED);return;}};
   let holes=if self.assembly_connector_mode==2 {terraincraft_core::seam_joints::plan(dem,&parts,self.settings(),terraincraft_core::seam_joints::SeamSettings::default()).unwrap_or_default()}else{Vec::new()};
   let gap=if self.assembly_exploded {self.assembly_gap_mm.max(0.)}else{0.};
   let mut batches=Vec::new();let mut errors=0usize;
   for tile in &parts {
    match self.tile_print_mesh(dem,*tile,&holes) {
     Ok(faces)=>{
      let ox=tile.x0 as f64*self.width/(dem.width-1) as f64+tile.col as f64*gap;
      let oy=tile.y0 as f64*self.height/(dem.height-1) as f64+tile.row as f64*gap;
      let color=if (tile.row+tile.col)%2==0 {[0.48,0.79,0.70]}else{[0.94,0.71,0.47]};
      batches.push((faces,[ox,oy],color));
     },Err(_)=>errors+=1
    }
   }
   let mesh=gpu_view::GpuMesh::from_tiles(&batches);
   let triangles=mesh.triangle_count;
   // Callback paints into the previously allocated rect.
   gpu_view::paint_rect(ui,rect,mesh,self.orbit_yaw,self.orbit_pitch,self.orbit_zoom,
    [((self.width+gap*parts.iter().map(|t|t.col).max().unwrap_or(0) as f64)*0.5) as f32,
     ((self.height+gap*parts.iter().map(|t|t.row).max().unwrap_or(0) as f64)*0.5) as f32,
     (self.base+self.relief*self.exaggeration) as f32*0.5],
    (self.width.max(self.height)+gap*parts.len() as f64) as f32);
   ui.painter_at(rect).text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,
     format!("GPU OpenGL depth buffer · {} triangles · {} mesh errors",triangles,errors),egui::FontId::monospace(12.),Color32::WHITE);
   return;
  }
  let painter=ui.painter_at(rect);painter.rect_filled(rect,0.,Color32::from_rgb(23,29,37));
  let (lo,hi)=dem.min_max();let span=(hi-lo).max(1e-9);
  let stride=((dem.width.max(dem.height)+85)/86).max(1);
  let xs:Vec<usize>=(0..dem.width).step_by(stride).chain(std::iter::once(dem.width-1)).collect();
  let ys:Vec<usize>=(0..dem.height).step_by(stride).chain(std::iter::once(dem.height-1)).collect();
  let point=|x:usize,y:usize|->Pos2{
   let xx=(x as f32/(dem.width-1) as f32-0.5)*2.;
   let yy=(y as f32/(dem.height-1) as f32-0.5)*2.*(self.height/self.width) as f32;
   let zz=(((dem.at(x,y)-lo)/span) as f32*self.relief as f32*self.exaggeration as f32/self.width as f32)*2.;
   let cy=self.orbit_yaw.cos();let sy=self.orbit_yaw.sin();
   let rx=xx*cy-yy*sy;let ry=xx*sy+yy*cy;
   let py=ry*self.orbit_pitch.cos()-zz*self.orbit_pitch.sin();
   let scale=rect.width().min(rect.height())*0.33*self.orbit_zoom;
   Pos2::new(rect.center().x+rx*scale,rect.center().y+py*scale)
  };
  for (j,&y) in ys.iter().enumerate(){
   let mut points=Vec::with_capacity(xs.len());
   for &x in &xs{points.push(point(x,y));}
   let shade=(110.+110.*j as f32/ys.len() as f32) as u8;
   painter.add(egui::Shape::line(points,Stroke::new(0.9,Color32::from_rgb(70,shade,145))));
  }
  for &x in &xs{let points:Vec<Pos2>=ys.iter().map(|&y|point(x,y)).collect();painter.add(egui::Shape::line(points,Stroke::new(0.8,Color32::from_rgb(85,170,140))));}
 }
 // Generate the same connector-modified triangles for both the desktop STL exporter
 // and the wireframe inspector. An unsupported cut is an error, not a fallback.
 fn tile_print_mesh(&self,dem:&Dem,t:terraincraft_core::Tile,holes:&[terraincraft_core::seam_joints::SeamHole])->Result<Vec<[[f64;3];3]>,String>{
  match self.assembly_connector_mode {
   0=>mesh(dem,self.settings(),t),
   1=>{
    let ts=tiles(dem,self.settings(),self.bed_x,self.bed_y,self.margin)?;
    let reg=terraincraft_core::registration::RegistrationSettings::default();
    let guides=terraincraft_core::registration::guides(dem,self.settings(),&ts,reg)?;
    let idx=ts.iter().position(|v|v.row==t.row&&v.col==t.col).ok_or("Unknown tile")?;
    terraincraft_core::socket_mesh::mesh_with_sockets(dem,self.settings(),t,&guides[idx],reg,32)
   },
   2=>{
    let selected:Vec<_>=holes.iter().filter(|h|h.tile_row==t.row&&h.tile_col==t.col).cloned().collect();
    terraincraft_core::seam_socket_mesh::mesh_with_seam_sockets(dem,self.settings(),t,&selected,32)
   },
   _=>Err("Unknown connector mode".into())
  }
 }
 fn draw_assembly_3d(&mut self,ui:&mut egui::Ui,dem:&Dem){
  ui.label("3D assembly inspection · drag to orbit · scroll to zoom · optional exploded tile spacing");
  let available=ui.available_size();
  let (rect,response)=ui.allocate_exact_size(available,Sense::drag());
  if response.dragged(){let delta=response.drag_delta();self.orbit_yaw+=delta.x*0.008;self.orbit_pitch=(self.orbit_pitch+delta.y*0.008).clamp(0.1,1.45);}
  if response.hovered(){let scroll=ui.input(|i|i.raw_scroll_delta.y);self.orbit_zoom=(self.orbit_zoom*(scroll*0.001).exp()).clamp(0.2,8.);}
  let painter=ui.painter_at(rect);painter.rect_filled(rect,0.,Color32::from_rgb(23,29,37));
  let parts=match tiles(dem,self.settings(),self.bed_x,self.bed_y,self.margin){Ok(v)=>v,Err(e)=>{painter.text(rect.center(),egui::Align2::CENTER_CENTER,e,egui::FontId::proportional(14.),Color32::LIGHT_RED);return;}};
  let (lo,hi)=dem.min_max();let span=(hi-lo).max(1e-9);
  let max_col=parts.iter().map(|t|t.col).max().unwrap_or(0) as f64;
  let max_row=parts.iter().map(|t|t.row).max().unwrap_or(0) as f64;
  let gap=if self.assembly_exploded {self.assembly_gap_mm.max(0.)}else{0.};
  let full_x=self.width+gap*max_col;let full_y=self.height+gap*max_row;
  let center_x=full_x*0.5;let center_y=full_y*0.5;
  let scale=rect.width().min(rect.height())*0.65*self.orbit_zoom/(full_x.max(full_y) as f32).max(1.);
  let yaw=self.orbit_yaw;let pitch=self.orbit_pitch;
  let project=|x:f64,y:f64,z:f64|->Pos2{
   let xx=(x-center_x) as f32;let yy=(y-center_y) as f32;
   let cy=yaw.cos();let sy=yaw.sin();let rx=xx*cy-yy*sy;let ry=xx*sy+yy*cy;
   let py=ry*pitch.cos()-(z as f32)*pitch.sin();
   Pos2::new(rect.center().x+rx*scale,rect.center().y+py*scale)
  };
  // The exact-mesh option uses the same terrain triangle generator as STL export.
  // It is deliberately capped to keep the immediate-mode egui painter responsive.
  if self.assembly_mesh_mode {
   let total_tiles=parts.len();
   let mut drawn=0usize;
   let mut skipped=0usize;
   let mut failures=0usize;
   let mut invalid=0usize;
   let holes=if self.assembly_connector_mode==2 {
    match terraincraft_core::seam_joints::plan(dem,&parts,self.settings(),terraincraft_core::seam_joints::SeamSettings::default()) {
     Ok(h)=>h,Err(e)=>{painter.text(rect.center(),egui::Align2::CENTER_CENTER,e,egui::FontId::proportional(14.),Color32::LIGHT_RED);return;}
    }
   }else{Vec::new()};
   // Painter-order software shading: sort all visible triangles by camera-space
   // depth before drawing. This requires no GPU backend or external renderer.
   let mut shaded_faces:Vec<(f32,[Pos2;3],Color32)>=Vec::new();
   const MAX_SHADED_FACES:usize=32_000;
   // Painter's algorithm: sort all faces across all tiles by camera-space depth.
   // This avoids wireframe see-through without requiring a separate GPU renderer.
   if self.assembly_shaded {
    let mut faces_to_draw:Vec<(f32,[Pos2;3],Color32)>=Vec::new();
    let mut triangle_count=0usize;
    let mut skipped_faces=0usize;
    const MAX_SHADED_FACES:usize=60_000;
    for tile in &parts {
     let faces=match self.tile_print_mesh(dem,*tile,&holes){Ok(f)=>f,Err(_)=>{failures+=1;continue}};
     let audit=terraincraft_core::mesh_audit::audit(&faces);
     if audit.boundary_edges>0 || audit.nonmanifold_edges>0 || audit.degenerate_triangles>0 {invalid+=1;}
     let stride=(faces.len().saturating_mul(parts.len())/MAX_SHADED_FACES).max(1);
     let offset_x=tile.x0 as f64*self.width/(dem.width-1) as f64+tile.col as f64*gap;
     let offset_y=tile.y0 as f64*self.height/(dem.height-1) as f64+tile.row as f64*gap;
     let base_color=if (tile.row+tile.col)%2==0 {[122.,202.,178.]}else{[240.,180.,120.]};
     for (i,tri) in faces.iter().enumerate(){
      triangle_count+=1;
      if i%stride!=0 {skipped_faces+=1;continue;}
      let a=tri[0];let b=tri[1];let c=tri[2];
      let u=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];
      let v=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
      let n=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
      let len=(n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt();
      if len<1e-12 {skipped_faces+=1;continue;}
      let light=[0.35_f64,-0.45,0.82];
      let diffuse=((n[0]*light[0]+n[1]*light[1]+n[2]*light[2])/len).abs();
      let brightness=(0.35+0.65*diffuse).clamp(0.,1.);
      let color=Color32::from_rgb((base_color[0]*brightness) as u8,(base_color[1]*brightness) as u8,(base_color[2]*brightness) as u8);
      let points=tri.map(|p|project(p[0]+offset_x,p[1]+offset_y,p[2]));
      // Rotated y coordinate is camera depth after the pitch transform.
      let depth=tri.iter().map(|p|{
       let x=(p[0]+offset_x-center_x) as f32;
       let y=(p[1]+offset_y-center_y) as f32;
       let ry=x*yaw.sin()+y*yaw.cos();
       ry*pitch.sin()+(p[2] as f32)*pitch.cos()
      }).sum::<f32>()/3.;
      faces_to_draw.push((depth,points,color));
     }
    }
    // Farther surfaces are painted first; nearest surfaces cover them.
    faces_to_draw.sort_by(|a,b|b.0.total_cmp(&a.0));
    for (_,points,color) in faces_to_draw {
     painter.add(egui::Shape::convex_polygon(points.to_vec(),color,Stroke::NONE));
    }
    if self.show_assembly_labels {
     for tile in &parts {
      let cx=(tile.x0+tile.x1) as f64*0.5*self.width/(dem.width-1) as f64+tile.col as f64*gap;
      let cy=(tile.y0+tile.y1) as f64*0.5*self.height/(dem.height-1) as f64+tile.row as f64*gap;
      painter.text(project(cx,cy,self.base+self.relief*self.exaggeration),egui::Align2::CENTER_CENTER,format!("R{} C{}",tile.row+1,tile.col+1),egui::FontId::monospace(12.),Color32::WHITE);
     }
    }
    painter.text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,format!("Shaded mesh · {} tiles · {} triangles · {} skipped · {} generation errors · {} topology warnings",total_tiles,triangle_count,skipped_faces,failures,invalid),egui::FontId::monospace(12.),Color32::WHITE);
    return;
   }
   const MAX_EDGES:usize=55_000;
   for tile in &parts {
    let faces=match self.tile_print_mesh(dem,*tile,&holes){Ok(f)=>f,Err(_)=>{failures+=1;continue}};
    let audit=terraincraft_core::mesh_audit::audit(&faces);
    if audit.boundary_edges>0 || audit.nonmanifold_edges>0 || audit.degenerate_triangles>0 {invalid+=1;}
    let stride=if self.assembly_shaded {(faces.len().saturating_mul(parts.len())/MAX_SHADED_FACES).max(1)}else{(faces.len().saturating_mul(3)/MAX_EDGES).max(1)};
    let color=if (tile.row+tile.col)%2==0 {Color32::from_rgb(122,202,178)}else{Color32::from_rgb(240,180,120)};
    for (i,tri) in faces.iter().enumerate() {
     if i%stride!=0 {skipped+=1;continue;}
     let offset_x=tile.x0 as f64*self.width/(dem.width-1) as f64+tile.col as f64*gap;
     let offset_y=tile.y0 as f64*self.height/(dem.height-1) as f64+tile.row as f64*gap;
     let points=tri.map(|v|project(v[0]+offset_x,v[1]+offset_y,v[2]));
     if self.assembly_shaded {
      let a=tri[0];let b=tri[1];let c=tri[2];
      let u=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];
      let v=[c[0]-a[0],c[1]-a[1],c[2]-a[2]];
      let n=[u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]];
      let norm=(n[0]*n[0]+n[1]*n[1]+n[2]*n[2]).sqrt();
      if norm<=1e-12 {continue;}
      let light=[0.4_f64,0.35,0.85];
      let brightness=(0.28+0.72*((n[0]*light[0]+n[1]*light[1]+n[2]*light[2])/norm).abs()).clamp(0.,1.) as f32;
      let tint=Color32::from_rgb((color.r() as f32*brightness) as u8,(color.g() as f32*brightness) as u8,(color.b() as f32*brightness) as u8);
      // Camera-space depth (the axis orthogonal to the screen).
      let cx=(a[0]+b[0]+c[0])/3.+offset_x;
      let cy=(a[1]+b[1]+c[1])/3.+offset_y;
      let cz=(a[2]+b[2]+c[2])/3.;
      let ry=-(cx-center_x)*yaw.sin() as f64+(cy-center_y)*yaw.cos() as f64;
      let depth=(ry*pitch.sin() as f64+cz*pitch.cos() as f64) as f32;
      shaded_faces.push((depth,points,tint));
     }else{
      for e in 0..3 {painter.line_segment([points[e],points[(e+1)%3]],Stroke::new(0.6,color));drawn+=1;}
     }
    }
    if self.show_assembly_labels {
     let cx=(tile.x0+tile.x1) as f64*0.5*self.width/(dem.width-1) as f64+tile.col as f64*gap;
     let cy=(tile.y0+tile.y1) as f64*0.5*self.height/(dem.height-1) as f64+tile.row as f64*gap;
     painter.text(project(cx,cy,self.base+self.relief*self.exaggeration),egui::Align2::CENTER_CENTER,format!("R{} C{}",tile.row+1,tile.col+1),egui::FontId::monospace(12.),Color32::WHITE);
    }
   }
   if self.assembly_shaded {
    shaded_faces.sort_by(|a,b|a.0.total_cmp(&b.0));
    for (_,pts,tint) in shaded_faces {
     painter.add(egui::Shape::convex_polygon(pts.to_vec(),tint,Stroke::NONE));
     drawn+=1;
    }
   }
   painter.text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,format!("{} tiles · {} drawn edges · {} skipped faces · {} generation errors · {} topology warnings · connector mode {}",total_tiles,drawn,skipped,failures,invalid,self.assembly_connector_mode),egui::FontId::monospace(12.),Color32::LIGHT_GRAY);
   return;
  }
  for tile in &parts {
   let stride_x=((tile.x1-tile.x0+35)/36).max(1);
   let stride_y=((tile.y1-tile.y0+35)/36).max(1);
   let xs:Vec<usize>=(tile.x0..tile.x1).step_by(stride_x).chain(std::iter::once(tile.x1)).collect();
   let ys:Vec<usize>=(tile.y0..tile.y1).step_by(stride_y).chain(std::iter::once(tile.y1)).collect();
   let sample=|x:usize,y:usize|{
    let px=x as f64/(dem.width-1) as f64*self.width+tile.col as f64*gap;
    let py=y as f64/(dem.height-1) as f64*self.height+tile.row as f64*gap;
    let z=self.base+(dem.at(x,y)-lo)/span*self.relief*self.exaggeration;
    project(px,py,z)
   };
   let color=if (tile.row+tile.col)%2==0 {Color32::from_rgb(122,202,178)}else{Color32::from_rgb(240,180,120)};
   for &y in &ys {let line:Vec<Pos2>=xs.iter().map(|&x|sample(x,y)).collect();if line.len()>1{painter.add(egui::Shape::line(line,Stroke::new(1.,color)));}}
   for &x in &xs {let line:Vec<Pos2>=ys.iter().map(|&y|sample(x,y)).collect();if line.len()>1{painter.add(egui::Shape::line(line,Stroke::new(0.8,color)));}}
   let corners=[(tile.x0,tile.y0),(tile.x1,tile.y0),(tile.x1,tile.y1),(tile.x0,tile.y1)];
   for i in 0..4 {let (x,y)=corners[i];let (nx,ny)=corners[(i+1)%4];painter.line_segment([sample(x,y),sample(nx,ny)],Stroke::new(2.,Color32::WHITE));}
   if self.show_assembly_labels {
    let cx=(tile.x0+tile.x1)/2;let cy=(tile.y0+tile.y1)/2;
    painter.text(sample(cx,cy),egui::Align2::CENTER_CENTER,format!("R{} C{}",tile.row+1,tile.col+1),egui::FontId::monospace(12.),Color32::WHITE);
   }
  }
  painter.text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,format!("{} tiles · gap {:.1} mm · preview only",parts.len(),gap),egui::FontId::monospace(12.),Color32::LIGHT_GRAY);
 }
 fn draw(&mut self,ui:&mut egui::Ui){
  let Some(dem)=self.dem.clone() else {ui.centered_and_justified(|ui|{ui.label("Load a DEM (.csv, .asc or .tif) to preview terrain");});return};
  if self.view==View::Engraving {
   ui.heading(if self.engraving_perspective{"Perspective engraving preview"}else{"Topographic engraving preview"});
   ui.label("Live vector geometry preview · drag labels to reposition · export produces separate SVG groups.");
   let size=ui.available_width().min(560.);let h=size*(self.height/self.width).clamp(0.2,2.0) as f32;let (r,response)=ui.allocate_exact_size(Vec2::new(size,h),Sense::click_and_drag());let painter=ui.painter_at(r);painter.rect_filled(r,2.,Color32::from_rgb(229,207,171));painter.rect_stroke(r,2.,Stroke::new(1.,Color32::DARK_GRAY));
   let geometry=if self.engraving_perspective {
      let mut vp=Viewpoint{x:self.observer_x*(dem.width-1) as f64,y:self.observer_y*(dem.height-1) as f64,bearing_deg:self.bearing,pitch_deg:self.pitch,fov_deg:self.fov,eye_height_m:self.eye_height,max_distance_m:self.max_distance,pixel_size_x_m:self.pixel_size_m,pixel_size_y_m:self.pixel_size_m};
      if self.geo_view_enabled {
       let extent=GeographicExtent{west:self.west,south:self.south,east:self.east,north:self.north};
       match extent.viewpoint(self.observer_lat,self.observer_lon,dem.width,dem.height,vp) {Ok(v)=>vp=v,Err(e)=>{ui.colored_label(Color32::DARK_RED,e);return;}}
      }
      engraving_preview::perspective(&dem,vp,self.width,self.height,self.depth_bands,240)
   } else {engraving_preview::topdown(&dem,self.width,self.height,self.contour_interval,120)};
   match geometry {
    Ok(paths)=>{
     for path in paths {
      let points:Vec<Pos2>=path.points.iter().map(|(x,y)|Pos2::new(r.left()+(*x/self.width) as f32*r.width(),r.top()+(*y/self.height) as f32*r.height())).collect();
      if points.len()>1 {painter.add(egui::Shape::line(points,Stroke::new(if path.layer%5==0{1.35}else{0.8},Color32::from_rgb(94,67,43))));}
     }
    },Err(e)=>{ui.colored_label(Color32::DARK_RED,format!("Preview: {e}"));}
   }
   painter.text(Pos2::new(r.center().x,r.top()+10.),egui::Align2::CENTER_TOP,&self.engraving_title,egui::FontId::proportional(16.),Color32::BLACK);
   for (i,a) in self.annotations.iter_mut().enumerate(){let p=Pos2::new(r.left()+a.x as f32*r.width(),r.top()+a.y as f32*r.height());painter.text(p,egui::Align2::CENTER_CENTER,&a.text,egui::FontId::proportional(14.),Color32::BLACK);let hit=Rect::from_center_size(p,Vec2::new(120.,22.));let drag=ui.interact(hit,ui.id().with(("annotation",i)),Sense::drag());if drag.dragged(){let d=drag.drag_delta();a.x=(a.x+d.x as f64/r.width() as f64).clamp(0.,1.);a.y=(a.y+d.y as f64/r.height() as f64).clamp(0.,1.);}}
   let _=response;
   if self.show_geo_map && self.engraving_perspective {self.draw_geo_map(ui);}else{self.draw_3d(ui,&dem);}return;
  }
  if self.view==View::Print {self.draw_assembly_3d(ui,&dem);return;}
  if self.view==View::Terrain {
   self.draw_3d(ui,&dem);
   return;
  }
  let available=ui.available_size();let aspect=match self.view {View::Laser=>self.sheet_x/self.sheet_y,_=>self.width/self.height}.max(0.05) as f32;
  let w=available.x.min(available.y*aspect).max(10.);let h=w/aspect;
  let (rect,_)=ui.allocate_exact_size(Vec2::new(w,h),Sense::hover());let painter=ui.painter_at(rect);
  painter.rect_filled(rect,2.,Color32::from_rgb(28,36,42));
  if self.view==View::Laser {
   match self.laser_data(&dem){Ok((parts,placements))=>{
    for p in placements.iter().filter(|p|p.sheet==self.sheet){if let Some(part)=parts.iter().find(|v|v.id==p.part_id){
     let minx=part.outline.iter().map(|v|v.0).fold(f64::INFINITY,f64::min);
     let miny=part.outline.iter().map(|v|v.1).fold(f64::INFINITY,f64::min);
     let pts:Vec<Pos2>=part.outline.iter().map(|(x,y)|Pos2::new(rect.left()+((p.x_mm+x-minx)/self.sheet_x) as f32*rect.width(),rect.top()+((p.y_mm+y-miny)/self.sheet_y) as f32*rect.height())).collect();
     painter.add(egui::Shape::line(pts,Stroke::new(1.3,Color32::LIGHT_RED)));
    }}
   },Err(e)=>{painter.text(rect.center(),egui::Align2::CENTER_CENTER,e,egui::FontId::proportional(13.),Color32::LIGHT_RED);}}
  }else{
   // Downsampled shaded relief preview; native Rust painting, no browser or GPU shader dependency.
   let sx=(dem.width/180).max(1);let sy=(dem.height/180).max(1);let(lo,hi)=dem.min_max();let span=(hi-lo).max(1e-12);
   for y in (0..dem.height-1).step_by(sy){for x in (0..dem.width-1).step_by(sx){
    let z=(dem.at(x,y)-lo)/span;
    let shade=(65.+z*165.).clamp(0.,255.) as u8;
    let col=Color32::from_rgb((shade as f64*0.75) as u8,shade,(shade as f64*0.72) as u8);
    let r=Rect::from_min_max(Pos2::new(rect.left()+x as f32/(dem.width-1) as f32*rect.width(),rect.top()+y as f32/(dem.height-1) as f32*rect.height()),Pos2::new(rect.left()+((x+sx).min(dem.width-1)) as f32/(dem.width-1) as f32*rect.width(),rect.top()+((y+sy).min(dem.height-1)) as f32/(dem.height-1) as f32*rect.height()));
    painter.rect_filled(r,0.,col);
   }}
   if self.view==View::Print {if let Ok(ts)=tiles(&dem,self.settings(),self.bed_x,self.bed_y,self.margin){for t in ts {let r=Rect::from_min_max(Pos2::new(rect.left()+t.x0 as f32/(dem.width-1) as f32*rect.width(),rect.top()+t.y0 as f32/(dem.height-1) as f32*rect.height()),Pos2::new(rect.left()+t.x1 as f32/(dem.width-1) as f32*rect.width(),rect.top()+t.y1 as f32/(dem.height-1) as f32*rect.height()));painter.rect_stroke(r,0.,Stroke::new(1.5,Color32::YELLOW));}}}
  }
 }
}
impl eframe::App for Studio {
 fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
  if let Some(rx)=&self.map_results {
   let mut completed=Vec::new();
   for _ in 0..32 {match rx.try_recv(){Ok(item)=>completed.push(item),Err(_)=>break}}
   for (key,result) in completed {
    self.map_pending.remove(&key);
    match result.and_then(|bytes|{
     let decoded=image::load_from_memory(&bytes).map_err(|e|e.to_string())?.to_rgba8();
     let size=[decoded.width() as usize,decoded.height() as usize];
     if size[0]>1024||size[1]>1024{return Err("Tile dimensions exceed limit".into())}
     let image=egui::ColorImage::from_rgba_unmultiplied(size,decoded.as_raw());
     self.map_textures.insert(key.clone(),ctx.load_texture(key,image,egui::TextureOptions::LINEAR));Ok(())
    }){Ok(())=>{},Err(e)=>self.message=format!("Map tile: {e}")}
   }
   if !self.map_pending.is_empty(){ctx.request_repaint_after(std::time::Duration::from_millis(150));}
  }
  // Bound GPU texture residency; disk-cached tiles can be reloaded as needed.
  if self.map_textures.len()>192 {
   self.map_textures.clear();
   self.map_requested.clear();
  }
  if let Some(rx)=&self.download_rx {match rx.try_recv(){Ok(result)=>{self.download_rx=None;self.message=match result.and_then(|b|self.finish_download(b)){Ok(())=>self.message.clone(),Err(e)=>format!("USGS error: {e}")};},Err(mpsc::TryRecvError::Disconnected)=>{self.download_rx=None;self.message="USGS download interrupted".into();},Err(mpsc::TryRecvError::Empty)=>{ctx.request_repaint_after(std::time::Duration::from_millis(100));}}}

  egui::TopBottomPanel::top("header").show(ctx,|ui|{ui.horizontal(|ui|{ui.heading("TerrainCraft · Rust Studio v5.0");ui.separator();ui.selectable_value(&mut self.view,View::Terrain,"Terrain");ui.selectable_value(&mut self.view,View::Print,"3D Print");ui.selectable_value(&mut self.view,View::Laser,"Laser Layout");ui.selectable_value(&mut self.view,View::Engraving,"Engraving Studio");});});
  egui::SidePanel::left("settings").resizable(true).default_width(290.).show(ctx,|ui|{
   ui.heading("Project");
   ui.collapsing("USGS 3DEP geographic selection",|ui|{
    ui.label("WGS84 geographic bounding box (degrees)");
    ui.horizontal(|ui|{ui.label("West");ui.add(egui::DragValue::new(&mut self.west).speed(0.01));ui.label("East");ui.add(egui::DragValue::new(&mut self.east).speed(0.01));});
    ui.horizontal(|ui|{ui.label("South");ui.add(egui::DragValue::new(&mut self.south).speed(0.01));ui.label("North");ui.add(egui::DragValue::new(&mut self.north).speed(0.01));});
    ui.add(egui::Slider::new(&mut self.raster_pixels,128..=1024).text("Raster width / height"));
    if let Ok(())=(Bounds{west:self.west,south:self.south,east:self.east,north:self.north}).validate(){ui.small(format!("Approximate geographic aspect ratio: {:.3}",Bounds{west:self.west,south:self.south,east:self.east,north:self.north}.approx_aspect()));}
    if ui.add_enabled(self.download_rx.is_none(),egui::Button::new("Download USGS elevation")).clicked(){if let Err(e)=self.begin_download(){self.message=e;}}
    ui.small("Downloads real USGS 3DEP raster. Internet required. For geographic accuracy, adjust model dimensions to match the reported aspect ratio.");
   });
   ui.label("Project JSON path");ui.text_edit_singleline(&mut self.project_path);
   ui.horizontal(|ui|{
    if ui.button("Save project").clicked(){self.message=match self.save_project(){Ok(v)=>v,Err(e)=>format!("Save error: {e}")};}
    if ui.button("Open project").clicked(){self.message=match self.open_project(){Ok(v)=>v,Err(e)=>format!("Open error: {e}")};}
   });
   ui.label("Elevation file path");ui.text_edit_singleline(&mut self.path);
   if ui.button("Load elevation grid").clicked(){if let Err(e)=self.load(){self.message=format!("Error: {e}");}}
   ui.label("Output folder");ui.text_edit_singleline(&mut self.output);
   ui.separator();ui.heading("Terrain dimensions (mm)");
   ui.add(egui::DragValue::new(&mut self.width).range(1.0..=10000.).prefix("Width ").speed(1.));
   ui.add(egui::DragValue::new(&mut self.height).range(1.0..=10000.).prefix("Height ").speed(1.));
   ui.add(egui::DragValue::new(&mut self.base).range(0.1..=100.).prefix("Base "));
   ui.add(egui::Slider::new(&mut self.relief,0.1..=300.).text("Relief mm"));
   ui.add(egui::Slider::new(&mut self.exaggeration,0.1..=10.).text("Vertical exaggeration"));
   ui.separator();
   match self.view {
    View::Terrain=>{ui.label("Elevation contours (engraving SVGs)");ui.add(egui::Slider::new(&mut self.layer_count,1..=60).text("Contour levels"));},
    View::Print=>{ui.heading("Print bed");ui.add(egui::DragValue::new(&mut self.bed_x).range(10.0..=2000.).prefix("Bed X "));ui.add(egui::DragValue::new(&mut self.bed_y).range(10.0..=2000.).prefix("Bed Y "));ui.add(egui::DragValue::new(&mut self.margin).range(0.0..=50.).prefix("Margin "));ui.separator();ui.checkbox(&mut self.assembly_exploded,"Exploded 3D assembly");ui.add(egui::Slider::new(&mut self.assembly_gap_mm,0.0..=80.0).text("Gap (mm)"));ui.checkbox(&mut self.show_assembly_labels,"Show tile row/column labels");ui.checkbox(&mut self.assembly_mesh_mode,"Render generated STL triangle mesh (slower)");ui.checkbox(&mut self.assembly_gpu,"GPU accelerated OpenGL solid preview (experimental)");ui.checkbox(&mut self.assembly_shaded,"Shaded solid mesh (depth-sorted)");ui.add_enabled_ui(self.assembly_mesh_mode,|ui|{ui.checkbox(&mut self.assembly_shaded,"Shaded solid triangles (depth sorted)");});
     ui.horizontal(|ui|{ui.label("Preview/export connectors:");ui.selectable_value(&mut self.assembly_connector_mode,0,"None");ui.selectable_value(&mut self.assembly_connector_mode,1,"Underside");ui.selectable_value(&mut self.assembly_connector_mode,2,"Seam sockets");});
     ui.small("Mesh preview uses the same generated triangles as desktop STL export. Experimental connector geometry may fail topology validation. Seam sockets still require coarse enough wall segments.");},
    View::Engraving=>{ui.heading("Single-sheet engraving");ui.horizontal(|ui|{ui.label("Map preset");ui.selectable_value(&mut self.engraving_preset,0,"Survey");ui.selectable_value(&mut self.engraving_preset,1,"Minimal");ui.selectable_value(&mut self.engraving_preset,2,"Vintage");ui.selectable_value(&mut self.engraving_preset,3,"Cartoon");});
    ui.collapsing("B&W engraving icon library (240 procedural SVGs)",|ui|{
      ui.add(egui::Slider::new(&mut self.icon_id,0..=239).text("Icon index"));
      ui.add(egui::Slider::new(&mut self.icon_x,0.0..=1.0).text("X position"));
      ui.add(egui::Slider::new(&mut self.icon_y,0.0..=1.0).text("Y position"));
      ui.add(egui::Slider::new(&mut self.icon_size,2.0..=60.0).text("Size mm"));
      ui.label("Original generic symbols; not yet a verified landmark-specific collection.");
    });
    ui.collapsing("Icon engraving styles and building perspectives",|ui|{
      ui.horizontal_wrapped(|ui|{
       for (i,label) in ["Original","Simplified","Fine outline","Bold","Temple front","Temple side","Temple corner"].iter().enumerate(){
        ui.selectable_value(&mut self.icon_variant,i,*label);
       }
      });
      ui.small("Front, side and corner are distinct schematic Salt Lake Temple illustrations. Other icons support line-weight engraving treatments; selecting a temple view for another icon retains its original view.");
    });
    ui.collapsing("US state cultural icons (engraving)",|ui|{
      ui.label("Select state using the jurisdiction field below (e.g. US-UT)");
      let icons=terraincraft_core::state_culture::for_state(&self.catalog_jurisdiction);
      egui::ScrollArea::vertical().max_height(190.).show(ui,|ui|{
       for icon in &icons {
        if ui.selectable_label(self.cultural_icon_id==icon.id,&icon.title).clicked(){
          self.cultural_icon_id=icon.id.clone();self.regional_icon_id.clear();
          self.selected_logo_id.clear();
          self.message=format!("Selected cultural icon: {}",icon.title);
        }
       }
      });
      if ui.button("Use generic icon instead").clicked(){self.cultural_icon_id.clear();}
      ui.small("Sports: six shared pictograms plus a curated regional sports illustration for each of the 50 states. All designs are unofficial.");
      ui.small("Illustrative symbols, not official state seals or licensed university logos.");
    });
    ui.collapsing("Regional cultural illustrations — states and countries",|ui|{
      ui.small("Original thematic art, not official emblems or verified views of real buildings.");
      ui.small("Choose a state code (US-UT) or country code (JP, FR, CA) in Jurisdiction below.");
      let records=terraincraft_core::regional_icons::for_jurisdiction(&self.catalog_jurisdiction);
      ui.horizontal_wrapped(|ui|{
       for (idx,style) in ["Original","Simplified","Profile composition","Three-quarter composition"].iter().enumerate(){
        ui.selectable_value(&mut self.regional_style,idx,*style);
       }
      });
      for r in &records {
       if ui.selectable_label(self.regional_icon_id==r.id,format!("{} — {}",r.name,r.motif)).clicked(){
        self.regional_icon_id=r.id.clone();self.cultural_icon_id.clear();self.selected_logo_id.clear();
        self.selected_unesco_svg.clear();
       }
      }
      if ui.button("Clear regional selection").clicked(){self.regional_icon_id.clear();}
      ui.small("Assets are loaded from assets/regional_icons relative to the working directory.");
    });
    ui.collapsing("College, university and professional team logos",|ui|{
      ui.small("Browse named organizations; artwork is not bundled. Import only logos you have permission to reproduce or engrave.");
      ui.horizontal(|ui|{ui.label("Logo catalog JSON");ui.text_edit_singleline(&mut self.logo_catalog_path);});
      ui.horizontal(|ui|{ui.label("Search");ui.text_edit_singleline(&mut self.logo_search);});
      match terraincraft_core::institution_logos::read_catalog(std::path::Path::new(&self.logo_catalog_path)) {
       Ok(records)=>{
        let matches=terraincraft_core::institution_logos::filter(&records,&self.catalog_jurisdiction,&self.logo_search);
        ui.label(format!("{} matching institutions / teams",matches.len()));
        egui::ScrollArea::vertical().max_height(190.).show(ui,|ui|{
         for record in matches {
          let ready=!record.svg_file.is_empty() && !record.license.is_empty() && !record.permission_notes.is_empty();
          let label=format!("{} {} ({})",if ready {"[SVG]"} else {"[metadata]"},record.name,record.category);
          if ui.selectable_label(self.selected_logo_id==record.id,label).clicked(){
           self.selected_logo_id=record.id.clone();self.cultural_icon_id.clear();
           self.message=if ready {format!("Selected logo: {}",record.name)} else {format!("{} has no licensed SVG; import artwork into catalog first",record.name)};
          }
         }
        });
       },Err(e)=>{ui.colored_label(egui::Color32::YELLOW,format!("Catalog: {e}"));}
      }
      if ui.button("Clear selected logo").clicked(){self.selected_logo_id.clear();}
      ui.small("Jurisdiction accepts US-UT, US-CA, CA-ON, GB-ENG, JP-13, etc. Edit the JSON to add any country, state, province, college or team.");
    });
    ui.collapsing("Temples, ski resorts and cathedrals",|ui|{
      ui.small("Named places use ORIGINAL CATEGORY PICTOGRAMS, not site-specific architectural likenesses. The catalog is a curated starter list, not a complete global inventory.");
      ui.horizontal(|ui|{ui.label("Catalog JSON");ui.text_edit_singleline(&mut self.logo_catalog_path);});
      ui.horizontal(|ui|{ui.label("Search places");ui.text_edit_singleline(&mut self.logo_search);});
      ui.small("Jurisdiction filter uses the same code as the landmark catalog; clear it to search worldwide.");
      match terraincraft_core::institution_logos::read_catalog(std::path::Path::new(&self.logo_catalog_path)) {
        Ok(records)=>{
          let matches=terraincraft_core::institution_logos::filter(&records,&self.catalog_jurisdiction,&self.logo_search);
          let places:Vec<_>=matches.into_iter().filter(|r| matches!(r.category.as_str(),"lds-temple"|"ski-resort"|"cathedral")).collect();
          ui.label(format!("{} matching places",places.len()));
          egui::ScrollArea::vertical().max_height(200.).show(ui,|ui|{
            for place in places {
              if ui.selectable_label(self.selected_logo_id==place.id,format!("{} — {}",place.name,place.category)).clicked(){
                self.selected_logo_id=place.id.clone();self.cultural_icon_id.clear();
                self.message=format!("Selected {} (generic {} symbol)",place.name,place.category);
              }
            }
          });
        },Err(e)=>{ui.colored_label(egui::Color32::YELLOW,format!("Site catalog: {e}"));}
      }
      if ui.button("Clear selected site icon").clicked(){self.selected_logo_id.clear();}
    });
    ui.collapsing("UNESCO World Heritage icons",|ui|{
      ui.small("Official UNESCO property metadata; generated SVGs are generic category placeholders, not property-specific illustrations.");
      ui.horizontal(|ui|{ui.label("UNESCO catalog JSON");ui.text_edit_singleline(&mut self.unesco_catalog_path);});
      ui.horizontal(|ui|{ui.label("Search name/country");ui.text_edit_singleline(&mut self.unesco_search);});
      ui.checkbox(&mut self.unesco_simplified,"Simplified engraving icon");
      if let Ok(json)=fs::read_to_string(&self.unesco_catalog_path) {
       if let Ok(entries)=serde_json::from_str::<Vec<serde_json::Value>>(&json) {
        let search=self.unesco_search.to_lowercase();
        let matches:Vec<_>=entries.iter().filter(|v|{
          let name=v["name"].as_str().unwrap_or("").to_lowercase();
          let countries=v["countries"].as_str().unwrap_or("").to_lowercase();
          search.is_empty() || name.contains(&search) || countries.contains(&search)
        }).collect();
        let curated=entries.iter().filter(|e|e["artwork_status"].as_str().unwrap_or("").starts_with("curated-")).count();
        ui.label(format!("{} matches / {} UNESCO properties ({} curated)",matches.len(),entries.len(),curated));
        egui::ScrollArea::vertical().max_height(180.).show(ui,|ui|{
         for entry in matches.iter().take(250) {
          let name=entry["name"].as_str().unwrap_or("Unnamed");
          let country=entry["countries"].as_str().unwrap_or("");
          let icon=entry[if self.unesco_simplified {"simple_svg_file"} else {"svg_file"}].as_str().unwrap_or("");
          if ui.selectable_label(self.selected_unesco_svg==icon,format!("{} — {}",name,country)).clicked(){
           self.selected_unesco_svg=icon.to_owned();
           self.cultural_icon_id.clear();self.selected_logo_id.clear();
           self.message=format!("Selected UNESCO icon for {name}");
          }
         }
        });
        if matches.len()>250 {ui.small("Showing first 250 matches; refine search to find additional sites.");}
       }
      } else {ui.small("Run: python tools/sync_unesco_whc.py --output assets/unesco_whc");}
      if ui.button("Clear UNESCO icon").clicked(){self.selected_unesco_svg.clear();}
    });
    ui.collapsing("Jurisdiction landmark catalog",|ui|{
      ui.label("CSV catalog file");ui.text_edit_singleline(&mut self.catalog_path);
      ui.label("Jurisdiction code (e.g. US-CA, FR)");ui.text_edit_singleline(&mut self.catalog_jurisdiction);
      ui.checkbox(&mut self.catalog_verified_only,"Show verified records only");
      ui.checkbox(&mut self.catalog_auto_icons,"Place verified catalog icons on top-down engraving");
      ui.small("Auto placement uses the USGS extent fields (WGS84); verify alignment before engraving. Perspective placement is not supported.");
      if ui.button("Load catalog and show landmarks").clicked(){
        self.message=match fs::read_to_string(&self.catalog_path).and_then(|csv|terraincraft_core::landmark_catalog::parse_catalog(&csv).map_err(|e|std::io::Error::new(std::io::ErrorKind::InvalidData,e))){
          Ok(items)=>{let matches=terraincraft_core::landmark_catalog::filter_jurisdiction(&items,&self.catalog_jurisdiction,self.catalog_verified_only);format!("{} matching landmarks; {} total populated records",matches.len(),items.len())},Err(e)=>format!("Catalog error: {e}")};
      }
      if let Ok(csv)=fs::read_to_string(&self.catalog_path){
       if let Ok(items)=terraincraft_core::landmark_catalog::parse_catalog(&csv){
        let matches=terraincraft_core::landmark_catalog::filter_jurisdiction(&items,&self.catalog_jurisdiction,self.catalog_verified_only);
        egui::ScrollArea::vertical().max_height(180.).show(ui,|ui|{
         for entry in matches.iter().take(100){
          if ui.button(format!("#{:02} {} · {}",entry.rank,entry.name,entry.category)).clicked(){
           self.observer_lat=entry.latitude;self.observer_lon=entry.longitude;
           if let Some(index)=terraincraft_core::landmark_catalog::bundled_icon_index(&entry.icon_id){self.icon_id=index;}
           self.message=format!("Selected {} ({:.5}, {:.5})",entry.name,entry.latitude,entry.longitude);
          }
         }
        });
       }
      }
    });
    ui.collapsing("International map sources (XYZ)",|ui|{
      let providers=terraincraft_core::map_providers::builtins();
      egui::ComboBox::from_label("Provider").selected_text(&providers[self.map_provider.min(providers.len()-1)].name).show_ui(ui,|ui|{for (i,p) in providers.iter().enumerate(){ui.selectable_value(&mut self.map_provider,i,&p.name);}});
      ui.label(&providers[self.map_provider.min(providers.len()-1)].attribution);
      ui.label("Custom XYZ template (z/x/y placeholders)");ui.text_edit_singleline(&mut self.map_url);
      if ui.button("Copy observer tile URL (zoom 10)").clicked(){
        let mut provider=providers[self.map_provider.min(providers.len()-1)].clone();
        if self.map_provider==2 && !self.map_url.is_empty(){provider.url_template=self.map_url.clone();}
        self.message=match terraincraft_core::map_providers::latlon_to_xyz(self.observer_lat,self.observer_lon,10).and_then(|(x,y)|provider.xyz_url(10,x,y)){Ok(url)=>{ui.ctx().copy_text(url);"Tile URL copied (not downloaded)".into()},Err(e)=>e};
      }
      ui.add(egui::Slider::new(&mut self.map_zoom,0..=17).text("Tile zoom"));
      ui.checkbox(&mut self.map_markers,"Show jurisdiction landmark markers");
      ui.add(egui::Slider::new(&mut self.map_marker_limit,10..=500).text("Maximum markers"));
      if ui.button("Load map tile at observer").clicked(){
       if let Err(e)=self.request_map_tile(){self.message=format!("Map tile: {e}");}
      }
      self.map_canvas(ui);
      ui.small("Multi-tile map viewport. Visible tiles only; cached for at least 7 days. Provider attribution above.");
    });
    ui.collapsing("Editable engraving annotations",|ui|{
      if ui.button("Add text label").clicked(){self.annotations.push(Annotation{text:"New label".into(),x:0.5,y:0.5,size_mm:4.,rotation_deg:0.});}
      let mut remove=None;
      for (i,a) in self.annotations.iter_mut().enumerate(){ui.group(|ui|{ui.horizontal(|ui|{ui.label(format!("Label {}",i+1));if ui.button("Remove").clicked(){remove=Some(i);}});ui.text_edit_singleline(&mut a.text);ui.add(egui::Slider::new(&mut a.x,0.0..=1.0).text("Horizontal"));ui.add(egui::Slider::new(&mut a.y,0.0..=1.0).text("Vertical"));ui.add(egui::Slider::new(&mut a.size_mm,1.0..=20.0).text("Text size mm"));ui.add(egui::Slider::new(&mut a.rotation_deg,-180.0..=180.0).text("Rotation"));});}
      if let Some(i)=remove{self.annotations.remove(i);}
    });ui.checkbox(&mut self.engraving_perspective,"First-person perspective (otherwise top-down)");ui.checkbox(&mut self.show_geo_map,"Show geographic selection canvas");ui.checkbox(&mut self.show_landmarks,"Overlay imported landmark labels");ui.label("Landmark CSV or GeoJSON FeatureCollection (WGS84 points)");ui.text_edit_singleline(&mut self.landmark_csv);ui.label("Title");ui.text_edit_singleline(&mut self.engraving_title);ui.label("Subtitle");ui.text_edit_singleline(&mut self.engraving_subtitle);ui.label("SVG font family");ui.text_edit_singleline(&mut self.engraving_font);ui.add(egui::Slider::new(&mut self.engraving_style,0..=2).text("Style: 0=ridgeline 1=contour 2=illustrated"));if self.engraving_perspective {ui.checkbox(&mut self.geo_view_enabled,"Use geographic observer coordinates (USGS download)");
    if self.geo_view_enabled {ui.add(egui::DragValue::new(&mut self.observer_lat).speed(0.001).prefix("Latitude "));ui.add(egui::DragValue::new(&mut self.observer_lon).speed(0.001).prefix("Longitude "));}
    ui.add(egui::Slider::new(&mut self.depth_bands,1..=8).text("Depth bands (1=original silhouette)"));
    ui.small("Multi-depth ridges currently omit summit labels; use one band for annotated peaks.");
    ui.label("Observer position as fraction of raster width and height (0–1)");ui.add(egui::Slider::new(&mut self.observer_x,0.0..=1.0).text("Observer X"));ui.add(egui::Slider::new(&mut self.observer_y,0.0..=1.0).text("Observer Y"));ui.add(egui::Slider::new(&mut self.bearing,0.0..=360.0).text("Bearing °"));ui.add(egui::Slider::new(&mut self.pitch,-45.0..=45.0).text("Pitch °"));ui.add(egui::Slider::new(&mut self.fov,10.0..=180.0).text("FOV °"));ui.add(egui::DragValue::new(&mut self.eye_height).range(0.0..=100.).prefix("Eye height m "));ui.add(egui::DragValue::new(&mut self.max_distance).range(10.0..=200000.).prefix("Range m "));ui.add(egui::DragValue::new(&mut self.pixel_size_m).range(0.01..=10000.).prefix("DEM cell m "));ui.checkbox(&mut self.label_candidate_peaks,"Annotate visible candidate summits (unnamed)");ui.add(egui::DragValue::new(&mut self.peak_prominence_m).range(0.0..=10000.).prefix("Local prominence m "));ui.add(egui::Slider::new(&mut self.peak_label_limit,0..=40).text("Max summit labels"));ui.small("IMPORTANT: enter correct DEM cell spacing in meters; raster-only import cannot infer projected pixel scale. Bearing is relative to raster north.");}else{ui.add(egui::DragValue::new(&mut self.contour_interval).range(0.1..=10000.).prefix("Contour interval m "));}},
    View::Laser=>{ui.heading("Laser sheet");ui.add(egui::DragValue::new(&mut self.sheet_x).range(20.0..=3000.).prefix("Sheet X "));ui.add(egui::DragValue::new(&mut self.sheet_y).range(20.0..=3000.).prefix("Sheet Y "));ui.add(egui::DragValue::new(&mut self.gap).range(0.0..=30.).prefix("Gap "));ui.add(egui::DragValue::new(&mut self.material).range(0.5..=30.).prefix("Material mm "));ui.add(egui::Slider::new(&mut self.layer_count,1..=60).text("Elevation layers"));ui.label(format!("Nominal maximum stack: {:.1} mm",self.layer_count as f64*self.material));ui.add(egui::DragValue::new(&mut self.sheet).range(1..=100).prefix("Preview sheet "));}
   }
   ui.separator();if ui.button("Export current workspace").clicked(){self.message=match self.export(){Ok(v)=>v,Err(e)=>format!("Export failed: {e}")};}
   ui.separator();ui.label(&self.message);ui.small("Laser cutting polygons remain experimental: verify holes, offsets and toolpaths in external CAM software.");
  });
  egui::CentralPanel::default().show(ctx,|ui|{ui.heading(match self.view{View::Terrain=>"Elevation preview",View::Print=>"Terrain and print-bed tiles",View::Laser=>"Laser sheet outlines",View::Engraving=>"Single-sheet engraving"});self.draw(ui);});
 }
}
fn main()->eframe::Result<()> {let opts=eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size([1200.,800.]).with_title("TerrainCraft"),..Default::default()};eframe::run_native("TerrainCraft",opts,Box::new(|_cc|Ok(Box::new(Studio::default()))))}
