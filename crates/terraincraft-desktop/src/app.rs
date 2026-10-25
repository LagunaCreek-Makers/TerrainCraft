//! Side panel controls and per-frame update loop.
use crate::studio::{Studio, View};
use eframe::egui::{self};
use std::{fs, sync::mpsc};
use terraincraft_core::engraving_editor::Annotation;
use terraincraft_core::usgs::Bounds;

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.map_results {
            let mut completed = Vec::new();
            for _ in 0..32 {
                match rx.try_recv() {
                    Ok(item) => completed.push(item),
                    Err(_) => break,
                }
            }
            for (key, result) in completed {
                self.map_pending.remove(&key);
                match result.and_then(|bytes| {
                    let decoded = image::load_from_memory(&bytes)
                        .map_err(|e| e.to_string())?
                        .to_rgba8();
                    let size = [decoded.width() as usize, decoded.height() as usize];
                    if size[0] > 1024 || size[1] > 1024 {
                        return Err("Tile dimensions exceed limit".into());
                    }
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw());
                    self.map_textures.insert(
                        key.clone(),
                        ctx.load_texture(key, image, egui::TextureOptions::LINEAR),
                    );
                    Ok(())
                }) {
                    Ok(()) => {}
                    Err(e) => self.message = format!("Map tile: {e}"),
                }
            }
            if !self.map_pending.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
        }
        // Bound GPU texture residency; disk-cached tiles can be reloaded as needed.
        if self.map_textures.len() > 192 {
            self.map_textures.clear();
            self.map_requested.clear();
        }
        if let Some(rx) = &self.download_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.download_rx = None;
                    self.message = match result.and_then(|b| self.finish_download(b)) {
                        Ok(()) => self.message.clone(),
                        Err(e) => format!("USGS error: {e}"),
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.download_rx = None;
                    self.message = "USGS download interrupted".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("TerrainCraft · Rust Studio v5.0");
                ui.separator();
                ui.selectable_value(&mut self.view, View::Terrain, "Terrain");
                ui.selectable_value(&mut self.view, View::Print, "3D Print");
                ui.selectable_value(&mut self.view, View::Laser, "Laser Layout");
                ui.selectable_value(&mut self.view, View::Engraving, "Engraving Studio");
            });
        });
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(match self.view {
                View::Terrain => "Elevation preview",
                View::Print => "Terrain and print-bed tiles",
                View::Laser => "Laser sheet outlines",
                View::Engraving => "Single-sheet engraving",
            });
            self.draw(ui);
        });
    }
}
