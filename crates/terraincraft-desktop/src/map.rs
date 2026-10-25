//! Interactive XYZ tile map, tile download/cache, and geographic selection canvas.
use crate::studio::Studio;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::{fs, sync::mpsc, thread};
use terraincraft_core::geoview::GeographicExtent;

impl Studio {
    pub(crate) fn request_map_tile(&mut self) -> Result<(), String> {
        self.queue_visible_tiles(540., 320.)
    }
    pub(crate) fn queue_visible_tiles(&mut self, width: f32, height: f32) -> Result<(), String> {
        let providers = terraincraft_core::map_providers::builtins();
        let mut provider = providers[self.map_provider.min(providers.len() - 1)].clone();
        if self.map_provider == 2 {
            provider.url_template = self.map_url.clone();
        }
        let (wx, wy) = terraincraft_core::map_providers::latlon_to_world(
            self.observer_lat,
            self.observer_lon,
            self.map_zoom,
        );
        let count = (1_u64 << self.map_zoom) as f64;
        let cx = wx * count;
        let cy = wy * count;
        let left = (cx - width as f64 / 512.).floor() as i64;
        let right = (cx + width as f64 / 512.).floor() as i64;
        let top = (cy - height as f64 / 512.).floor() as i64;
        let bottom = (cy + height as f64 / 512.).floor() as i64;
        if self.map_results.is_none() {
            let (tx, rx) = mpsc::channel();
            self.map_tx = Some(tx);
            self.map_results = Some(rx);
        }
        let cache = std::env::temp_dir().join("terraincraft-map-cache-v4.9");
        let provider_identity = format!("{}:{}", provider.name, provider.url_template);
        let provider_hash = provider_identity
            .bytes()
            .fold(14695981039346656037_u64, |h, b| {
                (h ^ b as u64).wrapping_mul(1099511628211)
            });
        let mut scheduled = 0;
        for y in top..=bottom {
            if y < 0 || y >= count as i64 {
                continue;
            }
            for x in left..=right {
                if scheduled >= 12 {
                    break;
                }
                let wrapped = x.rem_euclid(count as i64) as u32;
                let key = format!("{:016x}-{}-{}-{}", provider_hash, self.map_zoom, wrapped, y);
                if self.map_textures.contains_key(&key)
                    || self.map_pending.contains(&key)
                    || self.map_requested.contains(&key)
                {
                    continue;
                }
                let url = provider.xyz_url(self.map_zoom, wrapped, y as u32)?;
                let path = cache.join(format!("{key}.png"));
                let tx = self.map_tx.as_ref().unwrap().clone();
                self.map_pending.insert(key.clone());
                self.map_requested.insert(key.clone());
                scheduled += 1;
                let cache = cache.clone();
                thread::spawn(move || {
                    let result = (|| -> Result<Vec<u8>, String> {
                        if let Ok(meta) = fs::metadata(&path) {
                            if meta
                                .modified()
                                .ok()
                                .and_then(|t| t.elapsed().ok())
                                .map(|age| age.as_secs() < 7 * 86400)
                                .unwrap_or(false)
                            {
                                return fs::read(&path).map_err(|e| e.to_string());
                            }
                        }
                        let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20))
       .user_agent("TerrainCraft/4.9 (interactive terrain mapping; contact: local desktop user)")
       .build().map_err(|e|e.to_string())?;
                        let response = client
                            .get(url)
                            .send()
                            .map_err(|e| e.to_string())?
                            .error_for_status()
                            .map_err(|e| e.to_string())?;
                        if response.content_length().unwrap_or(0) > 2_000_000 {
                            return Err("Tile too large".into());
                        }
                        let bytes = response.bytes().map_err(|e| e.to_string())?;
                        if bytes.len() > 2_000_000 {
                            return Err("Tile too large".into());
                        }
                        fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
                        fs::write(path, &bytes).map_err(|e| e.to_string())?;
                        Ok(bytes.to_vec())
                    })();
                    let _ = tx.send((key, result));
                });
            }
        }
        Ok(())
    }
    pub(crate) fn map_canvas(&mut self, ui: &mut egui::Ui) {
        ui.label("Map navigation: drag to pan · wheel to zoom · Shift-click twice to define the terrain extent");
        let size = egui::vec2(ui.available_width().min(540.).max(180.), 320.);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_rgb(33, 43, 52));
        let (wx, wy) = terraincraft_core::map_providers::latlon_to_world(
            self.observer_lat,
            self.observer_lon,
            self.map_zoom,
        );
        let count = (1_u64 << self.map_zoom) as f64;
        let center_x = wx * count;
        let center_y = wy * count;
        let left = (center_x - size.x as f64 / 512.).floor() as i64;
        let right = (center_x + size.x as f64 / 512.).floor() as i64;
        let top = (center_y - size.y as f64 / 512.).floor() as i64;
        let bottom = (center_y + size.y as f64 / 512.).floor() as i64;
        let providers = terraincraft_core::map_providers::builtins();
        let mut provider = providers[self.map_provider.min(providers.len() - 1)].clone();
        if self.map_provider == 2 {
            provider.url_template = self.map_url.clone();
        }
        let identity = format!("{}:{}", provider.name, provider.url_template);
        let hash = identity.bytes().fold(14695981039346656037_u64, |h, b| {
            (h ^ b as u64).wrapping_mul(1099511628211)
        });
        for y in top..=bottom {
            if y < 0 || y >= count as i64 {
                continue;
            }
            for x in left..=right {
                let key = format!(
                    "{:016x}-{}-{}-{}",
                    hash,
                    self.map_zoom,
                    x.rem_euclid(count as i64),
                    y
                );
                if let Some(tex) = self.map_textures.get(&key) {
                    let px = rect.center().x + (x as f64 - center_x) as f32 * 256.;
                    let py = rect.center().y + (y as f64 - center_y) as f32 * 256.;
                    let target = Rect::from_min_size(egui::pos2(px, py), egui::vec2(256., 256.));
                    if let Some(clipped) = target
                        .intersect(rect)
                        .is_positive()
                        .then(|| target.intersect(rect))
                    {
                        let uv = Rect::from_min_max(
                            egui::pos2(
                                (clipped.left() - target.left()) / 256.,
                                (clipped.top() - target.top()) / 256.,
                            ),
                            egui::pos2(
                                (clipped.right() - target.left()) / 256.,
                                (clipped.bottom() - target.top()) / 256.,
                            ),
                        );
                        painter.image(tex.id(), clipped, uv, Color32::WHITE);
                    }
                }
            }
        }
        if let Err(e) = self.queue_visible_tiles(size.x, size.y) {
            self.message = format!("Map: {e}");
        }
        // Overlay catalog points in the same Web Mercator coordinate system as tiles.
        // Marker labels are intentionally capped to keep large imported catalogs responsive.
        let mut visible_markers: Vec<(Pos2, String, f64, f64, usize)> = Vec::new();
        if self.map_markers {
            if let Ok(csv) = fs::read_to_string(&self.catalog_path) {
                if let Ok(entries) = terraincraft_core::landmark_catalog::parse_catalog(&csv) {
                    let matching = terraincraft_core::landmark_catalog::filter_jurisdiction(
                        &entries,
                        &self.catalog_jurisdiction,
                        self.catalog_verified_only,
                    );
                    for entry in matching.into_iter().take(self.map_marker_limit.min(500)) {
                        let (mx, my) = terraincraft_core::map_providers::latlon_to_world(
                            entry.latitude,
                            entry.longitude,
                            self.map_zoom,
                        );
                        let dx = (mx - wx + 0.5).rem_euclid(1.) - 0.5;
                        let position = egui::pos2(
                            rect.center().x + (dx * count * 256.) as f32,
                            rect.center().y + ((my - wy) * count * 256.) as f32,
                        );
                        if rect.contains(position) {
                            painter.circle_filled(position, 5., Color32::from_rgb(255, 197, 50));
                            painter.circle_stroke(position, 5., Stroke::new(1_f32, Color32::BLACK));
                            visible_markers.push((
                                position,
                                entry.name.clone(),
                                entry.latitude,
                                entry.longitude,
                                terraincraft_core::landmark_catalog::bundled_icon_index(
                                    &entry.icon_id,
                                )
                                .unwrap_or(self.icon_id),
                            ));
                        }
                    }
                }
            }
        }
        painter.rect_stroke(rect, 0., Stroke::new(1_f32, Color32::GRAY));
        let cx = rect.center().x;
        let cy = rect.center().y;
        painter.line_segment(
            [egui::pos2(cx - 8., cy), egui::pos2(cx + 8., cy)],
            Stroke::new(1.5_f32, Color32::YELLOW),
        );
        painter.line_segment(
            [egui::pos2(cx, cy - 8.), egui::pos2(cx, cy + 8.)],
            Stroke::new(1.5_f32, Color32::YELLOW),
        );
        if response.drag_started() {
            self.map_drag_origin = Some((self.observer_lat, self.observer_lon));
        }
        if response.drag_stopped() {
            if let Some((lat, lon)) = self.map_drag_origin.take() {
                let d = response.drag_delta();
                let scale = 256. * (2_f64.powi(self.map_zoom as i32));
                let (x, y) =
                    terraincraft_core::map_providers::latlon_to_world(lat, lon, self.map_zoom);
                let (newlat, newlon) = terraincraft_core::map_providers::world_to_latlon(
                    x - d.x as f64 / scale,
                    y - d.y as f64 / scale,
                );
                self.observer_lat = newlat;
                self.observer_lon = newlon;
                let _ = self.request_map_tile();
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 2. {
                let next =
                    (self.map_zoom as i32 + if scroll > 0. { 1 } else { -1 }).clamp(0, 17) as u8;
                if next != self.map_zoom {
                    self.map_zoom = next;
                    let _ = self.request_map_tile();
                }
            }
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = terraincraft_core::map_providers::latlon_to_world(
                    self.observer_lat,
                    self.observer_lon,
                    self.map_zoom,
                );
                let scale = 256. * (2_f64.powi(self.map_zoom as i32));
                let (lat, lon) = terraincraft_core::map_providers::world_to_latlon(
                    x + (pos.x - cx) as f64 / scale,
                    y + (pos.y - cy) as f64 / scale,
                );
                // Clicking a marker selects its icon and observer coordinates for engraving.
                if !ui.input(|i| i.modifiers.shift) {
                    if let Some((_, name, mlat, mlon, icon)) = visible_markers
                        .iter()
                        .filter(|(p, _, _, _, _)| p.distance(pos) < 12.)
                        .min_by(|a, b| a.0.distance(pos).total_cmp(&b.0.distance(pos)))
                    {
                        self.observer_lat = *mlat;
                        self.observer_lon = *mlon;
                        self.icon_id = *icon;
                        self.message = format!("Selected landmark: {name}");
                        self.geo_view_enabled = true;
                        return;
                    }
                }
                let shift = ui.input(|i| i.modifiers.shift);
                if shift {
                    if let Some((a, b)) = self.map_extent_first.take() {
                        self.south = a.min(lat);
                        self.north = a.max(lat);
                        self.west = b.min(lon);
                        self.east = b.max(lon);
                        self.message = "Terrain extent updated from map corners".into();
                    } else {
                        self.map_extent_first = Some((lat, lon));
                        self.message = "Select the opposite extent corner with Shift-click".into();
                    }
                } else {
                    self.observer_lat = lat;
                    self.observer_lon = lon;
                    self.geo_view_enabled = true;
                    let _ = self.request_map_tile();
                }
            }
        }
        ui.small(format!(
            "Center: {:.5}°, {:.5}° | Zoom {} | Extent: {:.4},{:.4} to {:.4},{:.4}",
            self.observer_lat,
            self.observer_lon,
            self.map_zoom,
            self.west,
            self.south,
            self.east,
            self.north
        ));
    }
    pub(crate) fn draw_geo_map(&mut self, ui: &mut egui::Ui) {
        ui.label("Geographic selection canvas (coordinate grid, not a street basemap). Click to position observer; drag from observer to set bearing.");
        let available = ui.available_size();
        let side = available.x.min(available.y * 1.6).max(100.);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(side, side * 0.65), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4., Color32::from_rgb(28, 43, 49));
        for i in 0..=10 {
            let f = i as f32 / 10.;
            let x = rect.left() + f * rect.width();
            let y = rect.top() + f * rect.height();
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(0.5_f32, Color32::DARK_GRAY),
            );
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(0.5_f32, Color32::DARK_GRAY),
            );
        }
        let extent = GeographicExtent {
            west: self.west,
            south: self.south,
            east: self.east,
            north: self.north,
        };
        if extent.validate().is_err() {
            ui.colored_label(Color32::LIGHT_RED, "Invalid geographic bounds");
            return;
        }
        let to_pos = |lat: f64, lon: f64| {
            Pos2::new(
                rect.left()
                    + ((lon - extent.west) / (extent.east - extent.west)) as f32 * rect.width(),
                rect.top()
                    + ((extent.north - lat) / (extent.north - extent.south)) as f32 * rect.height(),
            )
        };
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.observer_lon = extent.west
                    + ((pos.x - rect.left()) / rect.width()) as f64 * (extent.east - extent.west);
                self.observer_lat = extent.north
                    - ((pos.y - rect.top()) / rect.height()) as f64 * (extent.north - extent.south);
                self.geo_view_enabled = true;
            }
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let origin = to_pos(self.observer_lat, self.observer_lon);
                let dx = (pos.x - origin.x) as f64;
                let dy = (origin.y - pos.y) as f64;
                if dx.abs() + dy.abs() > 4. {
                    self.bearing = dx.atan2(dy).to_degrees().rem_euclid(360.);
                }
            }
        }
        let origin = to_pos(self.observer_lat, self.observer_lon);
        if rect.contains(origin) {
            let length = rect.width().min(rect.height()) * 0.32;
            let left = (self.bearing - self.fov * 0.5).to_radians();
            let right = (self.bearing + self.fov * 0.5).to_radians();
            for a in [left, right] {
                let end = Pos2::new(
                    origin.x + length * a.sin() as f32,
                    origin.y - length * a.cos() as f32,
                );
                painter.line_segment([origin, end], Stroke::new(1.4_f32, Color32::YELLOW));
            }
            painter.circle_filled(origin, 5., Color32::LIGHT_GREEN);
        }
        painter.text(
            rect.left_top() + Vec2::new(7., 7.),
            egui::Align2::LEFT_TOP,
            format!("N ↑  {:.5}°, {:.5}°", self.observer_lat, self.observer_lon),
            egui::FontId::monospace(12.),
            Color32::WHITE,
        );
    }
}
