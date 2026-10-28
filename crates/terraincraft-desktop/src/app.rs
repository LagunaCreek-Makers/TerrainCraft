//! Side panel controls and per-frame update loop.
use crate::studio::{DownloadMsg, Studio, View};
use eframe::egui::{self};
use std::{fs, sync::mpsc};
use terraincraft_core::engraving_editor::Annotation;
use terraincraft_core::usgs::Bounds;

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.reprocess();
        self.sync_true_scale();
        // Undo history: record settled changes; Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) unless a
        // text field is focused (it has its own undo); Ctrl+S saves the project.
        let interacting = ctx.input(|i| i.pointer.any_down());
        self.history.observe(
            crate::history::snapshot(self),
            std::time::Instant::now(),
            interacting,
        );
        if self.history.settling() {
            ctx.request_repaint_after(std::time::Duration::from_millis(450));
        }
        if !ctx.wants_keyboard_input() {
            let (undo, redo, save) = ctx.input(|i| {
                let cmd = i.modifiers.command;
                (
                    cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    cmd && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                    cmd && i.key_pressed(egui::Key::S),
                )
            });
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
            if save {
                self.message = self
                    .save_project()
                    .unwrap_or_else(|e| format!("Save error: {e}"));
            }
        }
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
        if let Some(rx) = &self.search_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.search_rx = None;
                    match result {
                        Ok(places) if places.is_empty() => self.message = "No places found".into(),
                        Ok(places) => {
                            self.message = format!("{} places found", places.len());
                            let first = (places[0].lat, places[0].lon);
                            self.search_results = places;
                            self.map_go_to(first.0, first.1, 11);
                            self.view = View::Terrain;
                            self.terrain_show_map = true;
                        }
                        Err(e) => self.message = e,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.search_rx = None,
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if let Some(rx) = &self.names_rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.names_rx = None;
                    self.message = match result {
                        Ok(names) => {
                            let n = names.len();
                            self.place_names = names;
                            format!("Loaded {n} USGS place names")
                        }
                        Err(e) => format!("Place names: {e}"),
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => self.names_rx = None,
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if let Some(rx) = &self.download_rx {
            match rx.try_recv() {
                Ok(DownloadMsg::Progress(done, total)) => {
                    self.message = format!("Downloading USGS 3DEP elevation: {done} of {total}…");
                    ctx.request_repaint();
                }
                Ok(DownloadMsg::Done(result)) => {
                    self.download_rx = None;
                    self.message = match result.and_then(|r| self.finish_download(r)) {
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
                ui.heading("TerrainCraft");
                ui.separator();
                ui.selectable_value(&mut self.view, View::Terrain, "Terrain");
                ui.selectable_value(&mut self.view, View::Print, "3D Print");
                ui.selectable_value(&mut self.view, View::Laser, "Laser Layout");
                ui.selectable_value(&mut self.view, View::Engraving, "Engraving Studio");
                ui.separator();
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                    .on_hover_text("Ctrl+Z")
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                    .on_hover_text("Ctrl+Y or Ctrl+Shift+Z")
                    .clicked()
                {
                    self.redo();
                }
            });
        });
        egui::SidePanel::left("settings").resizable(true).default_width(290.).show(ctx,|ui|{
   ui.heading("Find a place");
   ui.horizontal(|ui|{
    let edit=ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Mount Rainier, Matterhorn…").desired_width(170.));
    let go=edit.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter));
    if (ui.add_enabled(self.search_rx.is_none(),egui::Button::new("Search")).clicked()||go)&&self.search_rx.is_none(){self.begin_search();}
   });
   let mut pick=None;
   for (i,r) in self.search_results.iter().enumerate().take(10){
    if ui.add(egui::Button::new(format!("{}. {}  ·  {}",i+1,r.name,r.detail)).wrap()).on_hover_text(format!("{:.4}, {:.4} ({})",r.lat,r.lon,r.source)).clicked(){pick=Some((r.lat,r.lon));}
   }
   if !self.search_results.is_empty(){ui.small("USGS GNIS (public domain) and OpenStreetMap Nominatim (© OpenStreetMap contributors)");}
   if let Some((lat,lon))=pick{
    self.map_go_to(lat,lon,12);
    self.observer_lat=lat;self.observer_lon=lon;
    self.view=View::Terrain;self.terrain_show_map=true;
   }
   ui.small("On the Terrain map the download area is the map window: pan and zoom to frame it. Double-click to start a selection box, then click its opposite corner; double-click again to go back to the window.");
   ui.add_space(4.);
   ui.strong(if self.map_selected{"Download area: selected box"}else{"Download area: map window"});
   let (w,s,e,n)=(self.west,self.south,self.east,self.north);
   egui::Grid::new("area_corners").num_columns(3).spacing([8.,2.]).show(ui,|ui|{
    for (name,lat,lon) in [("Center",(s+n)/2.,(w+e)/2.),("NW",n,w),("NE",n,e),("SW",s,w),("SE",s,e)]{
     ui.label(name);
     ui.monospace(format!("{lat:.5}"));
     ui.monospace(format!("{lon:.5}"));
     ui.end_row();
    }
   });
   let (mx,my)=terraincraft_core::georef::meters_per_degree((s+n)/2.);
   ui.small(format!("{:.2} x {:.2} km  ·  latitude, longitude in degrees",(e-w)*mx/1000.,(n-s)*my/1000.));
   ui.separator();
   ui.heading("Project");
   ui.collapsing("USGS 3DEP geographic selection",|ui|{
    ui.label("WGS84 geographic bounding box (degrees)");
    ui.horizontal(|ui|{ui.label("West");ui.add(egui::DragValue::new(&mut self.west).speed(0.01).max_decimals(5));ui.label("East");ui.add(egui::DragValue::new(&mut self.east).speed(0.01).max_decimals(5));});
    ui.horizontal(|ui|{ui.label("South");ui.add(egui::DragValue::new(&mut self.south).speed(0.01).max_decimals(5));ui.label("North");ui.add(egui::DragValue::new(&mut self.north).speed(0.01).max_decimals(5));});
    ui.horizontal(|ui|{
     ui.add(egui::Slider::new(&mut self.raster_pixels,128..=terraincraft_core::usgs::MAX_DOWNLOAD_SIDE).logarithmic(true).text("Raster long side (px)"));
     let bounds=Bounds{west:self.west,south:self.south,east:self.east,north:self.north};
     if ui.button("Match print size").on_hover_text("About 0.5 mm per sample on the model, capped at the ~10 m native 3DEP resolution").clicked()&&bounds.validate().is_ok(){
      self.raster_pixels=bounds.suggested_long_side(self.width.max(self.height),0.5);
     }
    });
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
   let georeferenced=self.dem.as_ref().is_some_and(|d|d.georef.is_some());
   ui.checkbox(&mut self.true_scale,"True scale (keep real-world proportions)").on_hover_text("For georeferenced sources: depth follows the ground and relief is true vertical scale × exaggeration");
   let derived=self.true_scale&&georeferenced;
   ui.add(egui::DragValue::new(&mut self.width).range(1.0..=10000.).prefix("Width ").speed(1.));
   if derived {
    ui.label(format!("Height {:.1} (from ground extent)",self.height));
    ui.add(egui::DragValue::new(&mut self.base).range(0.1..=100.).prefix("Base "));
    ui.label(format!("Relief {:.1} mm at this exaggeration",self.relief*self.exaggeration));
   } else {
    if self.true_scale&&self.dem.is_some(){ui.small("Source has no georeference; using manual height and relief.");}
    ui.add(egui::DragValue::new(&mut self.height).range(1.0..=10000.).prefix("Height ").speed(1.));
    ui.add(egui::DragValue::new(&mut self.base).range(0.1..=100.).prefix("Base "));
    ui.add(egui::Slider::new(&mut self.relief,0.1..=300.).text("Relief mm"));
   }
   ui.collapsing("Terrain processing",|ui|{
    ui.checkbox(&mut self.prep.utm,"Reproject to UTM").on_hover_text("Recommended for regions more than about a degree across. Landmark and viewpoint placement assume latitude/longitude and become approximate.");
    ui.add(egui::Slider::new(&mut self.prep.smooth_sigma,0.0..=10.0).text("Smoothing (samples)"));
    let mut clip=self.prep.sea_level_m.is_some();
    if ui.checkbox(&mut clip,"Flatten water at sea level").changed(){self.prep.sea_level_m=clip.then_some(0.);}
    if let Some(level)=self.prep.sea_level_m.as_mut(){
     ui.add(egui::DragValue::new(level).range(-500.0..=5000.).suffix(" m").prefix("Level "));
     ui.add(egui::DragValue::new(&mut self.prep.water_depth_m).range(0.0..=1000.).suffix(" m").prefix("Recess water ")).on_hover_text("Lowers the water surface so the coastline stays visible at true scale");
    }
   });
   ui.add(egui::Slider::new(&mut self.exaggeration,0.1..=10.).text("Vertical exaggeration"));
   ui.separator();
   match self.view {
    View::Terrain=>{ui.label("Elevation contours (engraving SVGs)");ui.add(egui::Slider::new(&mut self.layer_count,1..=60).text("Contour levels"));},
    View::Print=>{ui.heading("Print bed");ui.add(egui::DragValue::new(&mut self.bed_x).range(10.0..=2000.).prefix("Bed X "));ui.add(egui::DragValue::new(&mut self.bed_y).range(10.0..=2000.).prefix("Bed Y "));ui.add(egui::DragValue::new(&mut self.margin).range(0.0..=50.).prefix("Margin "));ui.separator();ui.checkbox(&mut self.assembly_exploded,"Exploded 3D assembly");ui.add(egui::Slider::new(&mut self.assembly_gap_mm,0.0..=80.0).text("Gap (mm)"));ui.checkbox(&mut self.show_assembly_labels,"Show tile row/column labels");ui.checkbox(&mut self.assembly_mesh_mode,"Render generated STL triangle mesh (slower)");ui.checkbox(&mut self.assembly_gpu,"GPU accelerated OpenGL solid preview (experimental)");ui.checkbox(&mut self.assembly_shaded,"Shaded solid mesh (depth-sorted)");ui.add_enabled_ui(self.assembly_mesh_mode,|ui|{ui.checkbox(&mut self.assembly_shaded,"Shaded solid triangles (depth sorted)");});
     ui.horizontal(|ui|{ui.label("Preview/export connectors:");ui.selectable_value(&mut self.assembly_connector_mode,0,"None");ui.selectable_value(&mut self.assembly_connector_mode,1,"Underside");ui.selectable_value(&mut self.assembly_connector_mode,2,"Seam sockets");});
     ui.small("Mesh preview uses the same generated triangles as desktop STL export. Experimental connector geometry may fail topology validation. Seam sockets still require coarse enough wall segments.");
     ui.separator();ui.heading("Export");
     ui.checkbox(&mut self.print_labels,"Engrave tile numbers underneath").on_hover_text("Mirrored so they read correctly when the tile is turned over");
     ui.horizontal(|ui|{
      ui.checkbox(&mut self.print_simplify,"Simplify meshes to within");
      ui.add_enabled(self.print_simplify,egui::DragValue::new(&mut self.simplify_mm).range(0.0..=1.0).speed(0.005).suffix(" mm"));
     });
     ui.horizontal(|ui|{
      ui.label("Slicer");ui.text_edit_singleline(&mut self.slicer_path).on_hover_text("Path to PrusaSlicer, OrcaSlicer, Bambu Studio, Cura...");
     });
     if ui.add_enabled(self.last_3mf.is_some()&&!self.slicer_path.trim().is_empty(),egui::Button::new("Open exported 3MF in slicer")).clicked(){
      if let Some(file)=&self.last_3mf{
       self.message=match std::process::Command::new(self.slicer_path.trim()).arg(file).spawn(){
        Ok(_)=>format!("Opened {file} in the slicer"),
        Err(e)=>format!("Could not start the slicer: {e}"),
       };
      }
     }},
    View::Engraving=>{ui.heading("Single-sheet engraving");ui.horizontal(|ui|{ui.label("Map preset");ui.selectable_value(&mut self.engraving_preset,0,"Survey");ui.selectable_value(&mut self.engraving_preset,1,"Minimal");ui.selectable_value(&mut self.engraving_preset,2,"Vintage");ui.selectable_value(&mut self.engraving_preset,3,"Cartoon");});
    ui.horizontal(|ui|{
     ui.label("Typeface");
     egui::ComboBox::from_id_salt("stroke_font").selected_text(self.stroke_font.name()).show_ui(ui,|ui|{
      for f in terraincraft_core::stroke_font::Font::ALL{ui.selectable_value(&mut self.stroke_font,f,f.name());}
     });
    });
    if !self.engraving_perspective{
     ui.horizontal(|ui|{
      ui.label("Map style");
      ui.selectable_value(&mut self.map_style,terraincraft_core::engrave::MapStyle::Exact,"Exact").on_hover_text("Every contour, labeled index contours, coordinate frame");
      ui.selectable_value(&mut self.map_style,terraincraft_core::engrave::MapStyle::Stylized,"Stylized").on_hover_text("Features as icons, fewer contours, decorative frame and compass rose");
     });
    }
    ui.checkbox(&mut self.place_icon,"Place the selected icon on the engraving");
    ui.collapsing(format!("Icon library ({} line icons)",terraincraft_core::icon_library::all().len()),|ui|{
      use terraincraft_core::icon_library as lib;
      ui.horizontal(|ui|{ui.label("Search");ui.text_edit_singleline(&mut self.icon_search);});
      egui::ComboBox::from_label("Category").selected_text(if self.icon_category.is_empty(){"All"}else{&self.icon_category}).show_ui(ui,|ui|{
       ui.selectable_value(&mut self.icon_category,String::new(),"All");
       for c in lib::categories(){ui.selectable_value(&mut self.icon_category,c.to_owned(),c);}
      });
      let matches=lib::search(&self.icon_search,(!self.icon_category.is_empty()).then_some(self.icon_category.as_str()));
      ui.label(format!("{} matching icons",matches.len()));
      egui::ScrollArea::vertical().id_salt("icon_grid").max_height(220.).show(ui,|ui|{
       ui.horizontal_wrapped(|ui|{
        for icon in matches {
         let selected=self.cultural_icon_id.is_empty()&&self.selected_logo_id.is_empty()&&self.selected_unesco_svg.is_empty()&&self.library_icon==icon.id;
         if icon_tile(ui,&icon.id,selected).on_hover_text(&icon.title).clicked(){
          self.library_icon=icon.id.clone();
          self.cultural_icon_id.clear();self.selected_logo_id.clear();self.selected_unesco_svg.clear();
          self.message=format!("Selected icon: {}",icon.title);
         }
        }
       });
      });
      if let Some(icon)=lib::get(&self.library_icon){
       let set=lib::set_of(icon);
       let license=if icon.license.is_empty(){set.map_or("",|s|s.license.as_str())}else{icon.license.as_str()};
       ui.small(format!("Selected: {} — {} {}, {}",icon.title,set.map_or("",|s|s.title.as_str()),set.map_or("",|s|s.version.as_str()),license));
       if !icon.attribution.is_empty(){ui.small(format!("Credit required: {}",icon.attribution));}
       if icon.review=="pending"{ui.colored_label(egui::Color32::YELLOW,"Draft artwork, not yet reviewed");}
       for s in &icon.sources{ui.hyperlink_to(egui::RichText::new(s).small(),s);}
      }
      ui.add(egui::Slider::new(&mut self.icon_x,0.0..=1.0).text("X position"));
      ui.add(egui::Slider::new(&mut self.icon_y,0.0..=1.0).text("Y position"));
      ui.add(egui::Slider::new(&mut self.icon_size,2.0..=60.0).text("Size mm"));
      ui.small("Tabler Icons (MIT), Maki and Temaki (CC0), and NPS map symbols (BSD). License texts are in assets/icon_library.");
    });
    if self.cultural_icon_id=="us-ut-salt-lake-temple"{
     ui.horizontal(|ui|{
      ui.label("Temple view");
      for (i,label) in [(0,"Original"),(4,"Front"),(5,"Side"),(6,"Corner")]{ui.selectable_value(&mut self.icon_variant,i,label);}
     });
    }
    ui.collapsing("US state cultural icons (engraving)",|ui|{
      ui.label("Select state using the jurisdiction field below (e.g. US-UT)");
      let icons=terraincraft_core::state_culture::for_state(&self.catalog_jurisdiction);
      egui::ScrollArea::vertical().max_height(190.).show(ui,|ui|{
       for icon in &icons {
        if ui.selectable_label(self.cultural_icon_id==icon.id,&icon.title).clicked(){
          self.cultural_icon_id=icon.id.clone();
          self.selected_logo_id.clear();
          self.message=format!("Selected cultural icon: {}",icon.title);
        }
       }
      });
      if ui.button("Use the icon library instead").clicked(){self.cultural_icon_id.clear();}
      ui.small("Sports: six shared pictograms plus a curated regional sports illustration for each of the 50 states. All designs are unofficial.");
      ui.small("Illustrative symbols, not official state seals or licensed university logos.");
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
           if let Some(id)=terraincraft_core::landmark_catalog::bundled_icon(&entry.icon_id){self.library_icon=id.into();self.cultural_icon_id.clear();}
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
      ui.add(egui::Slider::new(&mut self.map_zoom,0.0..=17.0).text("Map zoom"));
      ui.checkbox(&mut self.map_markers,"Show jurisdiction landmark markers");
      ui.add(egui::Slider::new(&mut self.map_marker_limit,10..=500).text("Maximum markers"));
      ui.small("The map in the Terrain workspace and the viewpoint map use this provider. Tiles are cached for 7 days.");
    });
    ui.collapsing("Editable engraving annotations",|ui|{
      if ui.button("Add text label").clicked(){self.annotations.push(Annotation{text:"New label".into(),x:0.5,y:0.5,size_mm:4.,rotation_deg:0.});}
      let mut remove=None;
      for (i,a) in self.annotations.iter_mut().enumerate(){ui.group(|ui|{ui.horizontal(|ui|{ui.label(format!("Label {}",i+1));if ui.button("Remove").clicked(){remove=Some(i);}});ui.text_edit_singleline(&mut a.text);ui.add(egui::Slider::new(&mut a.x,0.0..=1.0).text("Horizontal"));ui.add(egui::Slider::new(&mut a.y,0.0..=1.0).text("Vertical"));ui.add(egui::Slider::new(&mut a.size_mm,1.0..=20.0).text("Text size mm"));ui.add(egui::Slider::new(&mut a.rotation_deg,-180.0..=180.0).text("Rotation"));});}
      if let Some(i)=remove{self.annotations.remove(i);}
    });ui.checkbox(&mut self.engraving_perspective,"First-person perspective (otherwise top-down)");ui.checkbox(&mut self.show_geo_map,"Show geographic selection canvas");ui.checkbox(&mut self.engraving_feet,"Elevations in feet");
    ui.horizontal(|ui|{
     let can=self.names_rx.is_none()&&self.dem.as_ref().is_some_and(|d|d.georef.is_some());
     if ui.add_enabled(can,egui::Button::new("Download USGS place names")).on_hover_text("Named summits, passes and ridges for the loaded area from USGS GNIS (public domain)").clicked(){
      self.begin_place_names();
     }
     ui.label(format!("{} names",self.place_names.len()));
    });
    ui.checkbox(&mut self.show_landmarks,"Overlay imported landmark labels");ui.label("Landmark CSV or GeoJSON FeatureCollection (WGS84 points)");ui.text_edit_singleline(&mut self.landmark_csv);ui.label("Title");ui.text_edit_singleline(&mut self.engraving_title);ui.label("Subtitle");ui.text_edit_singleline(&mut self.engraving_subtitle);ui.label("SVG font family");ui.text_edit_singleline(&mut self.engraving_font);ui.add(egui::Slider::new(&mut self.engraving_style,0..=2).text("Style: 0=ridgeline 1=contour 2=illustrated"));if self.engraving_perspective {ui.checkbox(&mut self.geo_view_enabled,"Use geographic observer coordinates (USGS download)");
    if self.geo_view_enabled {ui.add(egui::DragValue::new(&mut self.observer_lat).speed(0.001).prefix("Latitude "));ui.add(egui::DragValue::new(&mut self.observer_lon).speed(0.001).prefix("Longitude "));}
    ui.add(egui::Slider::new(&mut self.depth_bands,1..=8).text("Depth bands (1=original silhouette)"));
    ui.small("Multi-depth ridges currently omit summit labels; use one band for annotated peaks.");
    ui.label("Observer position as fraction of raster width and height (0–1)");ui.add(egui::Slider::new(&mut self.observer_x,0.0..=1.0).text("Observer X"));ui.add(egui::Slider::new(&mut self.observer_y,0.0..=1.0).text("Observer Y"));ui.add(egui::Slider::new(&mut self.bearing,0.0..=360.0).text("Bearing °"));ui.add(egui::Slider::new(&mut self.pitch,-45.0..=45.0).text("Pitch °"));ui.add(egui::Slider::new(&mut self.fov,10.0..=180.0).text("FOV °"));ui.add(egui::DragValue::new(&mut self.eye_height).range(0.0..=100.).prefix("Eye height m "));ui.add(egui::DragValue::new(&mut self.max_distance).range(10.0..=200000.).prefix("Range m "));ui.add(egui::DragValue::new(&mut self.pixel_size_m).range(0.01..=10000.).prefix("DEM cell m "));ui.checkbox(&mut self.label_candidate_peaks,"Annotate visible candidate summits (unnamed)");ui.add(egui::DragValue::new(&mut self.peak_prominence_m).range(0.0..=10000.).prefix("Local prominence m "));ui.add(egui::Slider::new(&mut self.peak_label_limit,0..=40).text("Max summit labels"));ui.small("IMPORTANT: enter correct DEM cell spacing in meters; raster-only import cannot infer projected pixel scale. Bearing is relative to raster north.");}else{ui.add(egui::DragValue::new(&mut self.contour_interval).range(0.1..=10000.).prefix("Contour interval m "));}},
    View::Laser=>{ui.heading("Laser sheet");ui.add(egui::DragValue::new(&mut self.sheet_x).range(20.0..=3000.).prefix("Sheet X "));ui.add(egui::DragValue::new(&mut self.sheet_y).range(20.0..=3000.).prefix("Sheet Y "));ui.add(egui::DragValue::new(&mut self.gap).range(0.0..=30.).prefix("Gap "));ui.add(egui::DragValue::new(&mut self.material).range(0.5..=30.).prefix("Material mm "));ui.add(egui::DragValue::new(&mut self.kerf).range(0.0..=1.0).speed(0.01).prefix("Kerf mm "));
     if let Some(dem)=self.dem.as_ref(){let n=terraincraft_core::stack::levels_for_material(dem,self.settings(),self.material).len()+1;ui.label(format!("{n} layers, {:.1} mm stack (relief and exaggeration as in the 3D print)",n as f64*self.material));}
     ui.small("Red is cut, blue is engraved: each part shows where the next layer goes and its part number.");ui.add(egui::DragValue::new(&mut self.sheet).range(1..=100).prefix("Preview sheet "));}
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

/// A clickable thumbnail of a library icon, drawn from its engraving strokes.
fn icon_tile(ui: &mut egui::Ui, id: &str, selected: bool) -> egui::Response {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use terraincraft_core::svg_lines::Strokes;
    static STROKES: OnceLock<HashMap<&'static str, Strokes>> = OnceLock::new();
    let strokes = STROKES.get_or_init(|| {
        terraincraft_core::icon_library::all()
            .iter()
            .filter_map(|i| {
                let svg = terraincraft_core::icon_library::svg(&i.id)?;
                Some((
                    i.id.as_str(),
                    terraincraft_core::svg_lines::parse(svg).ok()?,
                ))
            })
            .collect()
    });
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36., 36.), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter_at(rect);
    painter.rect(rect, 3., visuals.weak_bg_fill, visuals.bg_stroke);
    if let Some(s) = strokes.get(id) {
        let stroke = egui::Stroke::new(1.2_f32, visuals.fg_stroke.color);
        let c = rect.center();
        for line in s.placed(0., 0., 26.) {
            let pts = line
                .iter()
                .map(|p| c + egui::vec2(p.0 as f32, p.1 as f32))
                .collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
    }
    response
}
