//! Interactive basemap: tile download and cache, pan and zoom, selecting the area to
//! download, and placing the panorama observer. Shared by every workspace.
use crate::studio::Studio;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::{fs, sync::mpsc, thread};
use terraincraft_core::map_providers::{latlon_to_world, world_to_latlon, MapProvider};
use terraincraft_core::usgs::Bounds;

const TILE: f64 = 256.;
const MAX_ZOOM: u8 = 17;

/// Screen geometry of the map for one frame.
struct View {
    rect: Rect,
    /// Center in normalized Web Mercator coordinates (0..1).
    cx: f64,
    cy: f64,
    /// Screen pixels per normalized unit.
    scale: f64,
}

impl View {
    fn to_screen(&self, lat: f64, lon: f64) -> Pos2 {
        let (x, y) = latlon_to_world(lat, lon, 0);
        // Take the shorter way around the antimeridian.
        let dx = (x - self.cx + 0.5).rem_euclid(1.) - 0.5;
        Pos2::new(
            self.rect.center().x + (dx * self.scale) as f32,
            self.rect.center().y + ((y - self.cy) * self.scale) as f32,
        )
    }
    fn to_latlon(&self, p: Pos2) -> (f64, f64) {
        world_to_latlon(
            (self.cx + (p.x - self.rect.center().x) as f64 / self.scale).rem_euclid(1.),
            (self.cy + (p.y - self.rect.center().y) as f64 / self.scale).clamp(0., 1.),
        )
    }
}

impl Studio {
    pub(crate) fn map_provider(&self) -> MapProvider {
        let providers = terraincraft_core::map_providers::builtins();
        let mut p = providers[self.map_provider.min(providers.len() - 1)].clone();
        if self.map_provider == 2 && !self.map_url.is_empty() {
            p.url_template = self.map_url.clone();
        }
        p
    }

    /// Start downloading tiles visible in `rect` that are not loaded or loading. Tiles are
    /// cached on disk for a week and at most 12 requests start per frame, in line with the
    /// OpenStreetMap tile usage policy.
    fn queue_tiles(&mut self, size: Vec2) -> Result<(), String> {
        let provider = self.map_provider();
        let zoom = self.map_zoom.min(provider.max_zoom);
        let (wx, wy) = latlon_to_world(self.map_center_lat, self.map_center_lon, zoom);
        let count = (1_u64 << zoom) as f64;
        let (cx, cy) = (wx * count, wy * count);
        let (hx, hy) = (size.x as f64 / 2. / TILE, size.y as f64 / 2. / TILE);
        if self.map_results.is_none() {
            let (tx, rx) = mpsc::channel();
            self.map_tx = Some(tx);
            self.map_results = Some(rx);
        }
        let cache = std::env::temp_dir().join("terraincraft-map-cache-v4.9");
        let mut scheduled = 0;
        for y in (cy - hy).floor() as i64..=(cy + hy).floor() as i64 {
            if y < 0 || y >= count as i64 {
                continue;
            }
            for x in (cx - hx).floor() as i64..=(cx + hx).floor() as i64 {
                if scheduled >= 12 {
                    return Ok(());
                }
                let wrapped = x.rem_euclid(count as i64) as u32;
                let key = tile_key(&provider, zoom, wrapped, y as u32);
                if self.map_textures.contains_key(&key) || self.map_requested.contains(&key) {
                    continue;
                }
                let url = provider.xyz_url(zoom, wrapped, y as u32)?;
                let path = cache.join(format!("{key}.png"));
                let tx = self.map_tx.as_ref().expect("created above").clone();
                self.map_pending.insert(key.clone());
                self.map_requested.insert(key.clone());
                scheduled += 1;
                let cache = cache.clone();
                thread::spawn(move || {
                    let result = (|| -> Result<Vec<u8>, String> {
                        let fresh = fs::metadata(&path)
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age.as_secs() < 7 * 86400);
                        if fresh {
                            return fs::read(&path).map_err(|e| e.to_string());
                        }
                        let client = reqwest::blocking::Client::builder()
                            .timeout(std::time::Duration::from_secs(20))
                            .user_agent(concat!(
                                "TerrainCraft/",
                                env!("CARGO_PKG_VERSION"),
                                " (https://github.com/LagunaCreek-Makers/TerrainCraft)"
                            ))
                            .build()
                            .map_err(|e| e.to_string())?;
                        let bytes = client
                            .get(url)
                            .send()
                            .and_then(|r| r.error_for_status())
                            .and_then(|r| r.bytes())
                            .map_err(|e| e.to_string())?;
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

    /// Center the map on a point at a useful zoom.
    pub(crate) fn map_go_to(&mut self, lat: f64, lon: f64, zoom: u8) {
        self.map_center_lat = lat.clamp(-85., 85.);
        self.map_center_lon = lon;
        self.map_zoom = zoom.min(MAX_ZOOM);
    }

    /// The interactive map, filling `size`.
    ///
    /// - Drag to pan, scroll to zoom around the pointer.
    /// - Shift-drag to select the area to download.
    /// - With `place_observer`, click to set the panorama viewpoint and drag from it to
    ///   set the viewing direction.
    pub(crate) fn map_view(&mut self, ui: &mut egui::Ui, size: Vec2, place_observer: bool) {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_rgb(33, 43, 52));
        let provider = self.map_provider();
        self.map_zoom = self.map_zoom.min(provider.max_zoom).min(MAX_ZOOM);
        let (cx, cy) = latlon_to_world(self.map_center_lat, self.map_center_lon, 0);
        let mut view = View {
            rect,
            cx,
            cy,
            scale: TILE * (1_u64 << self.map_zoom) as f64,
        };

        // Input first, so this frame draws the result.
        let shift = ui.input(|i| i.modifiers.shift);
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        if response.drag_started() {
            if let Some(p) = pointer {
                if shift {
                    self.map_select_start = Some(view.to_latlon(p));
                } else if place_observer {
                    let obs = view.to_screen(self.observer_lat, self.observer_lon);
                    if obs.distance(p) > 10. {
                        (self.observer_lat, self.observer_lon) = view.to_latlon(p);
                    }
                    self.map_aiming = true;
                }
            }
        }
        if response.dragged() {
            if let Some(p) = pointer {
                if self.map_select_start.is_none() && !self.map_aiming {
                    let d = response.drag_delta();
                    view.cx = (view.cx - d.x as f64 / view.scale).rem_euclid(1.);
                    view.cy = (view.cy - d.y as f64 / view.scale).clamp(0., 1.);
                    (self.map_center_lat, self.map_center_lon) = world_to_latlon(view.cx, view.cy);
                } else if self.map_aiming {
                    let o = view.to_screen(self.observer_lat, self.observer_lon);
                    let (dx, dy) = ((p.x - o.x) as f64, (o.y - p.y) as f64);
                    if dx.hypot(dy) > 6. {
                        self.bearing = dx.atan2(dy).to_degrees().rem_euclid(360.);
                    }
                }
            }
        }
        if response.drag_stopped() {
            if let (Some(a), Some(p)) = (self.map_select_start.take(), pointer) {
                let b = view.to_latlon(p);
                let sel = Bounds {
                    west: a.1.min(b.1),
                    east: a.1.max(b.1),
                    south: a.0.min(b.0),
                    north: a.0.max(b.0),
                };
                self.message = match sel.validate() {
                    Ok(()) if sel.east - sel.west > 1e-4 && sel.north - sel.south > 1e-4 => {
                        [self.west, self.south, self.east, self.north] =
                            [sel.west, sel.south, sel.east, sel.north];
                        "Download area selected; adjust the resolution and download".into()
                    }
                    Ok(()) => "Selection too small".into(),
                    Err(e) => e,
                };
            }
            self.map_aiming = false;
        }
        if response.clicked() && place_observer {
            if let Some(p) = pointer {
                (self.observer_lat, self.observer_lon) = view.to_latlon(p);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 2. {
                let next = (self.map_zoom as i32 + if scroll > 0. { 1 } else { -1 })
                    .clamp(0, provider.max_zoom.min(MAX_ZOOM) as i32)
                    as u8;
                if next != self.map_zoom {
                    // Keep the point under the pointer still.
                    let anchor = pointer.unwrap_or(rect.center());
                    let (lat, lon) = view.to_latlon(anchor);
                    self.map_zoom = next;
                    view.scale = TILE * (1_u64 << next) as f64;
                    let (ax, ay) = latlon_to_world(lat, lon, 0);
                    view.cx =
                        (ax - (anchor.x - rect.center().x) as f64 / view.scale).rem_euclid(1.);
                    view.cy = (ay - (anchor.y - rect.center().y) as f64 / view.scale).clamp(0., 1.);
                    (self.map_center_lat, self.map_center_lon) = world_to_latlon(view.cx, view.cy);
                }
            }
        }

        // Tiles.
        let zoom = self.map_zoom;
        let count = (1_u64 << zoom) as f64;
        let (tcx, tcy) = (view.cx * count, view.cy * count);
        let (hx, hy) = (size.x as f64 / 2. / TILE, size.y as f64 / 2. / TILE);
        for y in (tcy - hy).floor() as i64..=(tcy + hy).floor() as i64 {
            if y < 0 || y >= count as i64 {
                continue;
            }
            for x in (tcx - hx).floor() as i64..=(tcx + hx).floor() as i64 {
                let key = tile_key(&provider, zoom, x.rem_euclid(count as i64) as u32, y as u32);
                let Some(tex) = self.map_textures.get(&key) else {
                    continue;
                };
                let min = Pos2::new(
                    rect.center().x + ((x as f64 - tcx) * TILE) as f32,
                    rect.center().y + ((y as f64 - tcy) * TILE) as f32,
                );
                let target = Rect::from_min_size(min, Vec2::splat(TILE as f32));
                let clipped = target.intersect(rect);
                if clipped.is_positive() {
                    let uv = Rect::from_min_max(
                        ((clipped.min - target.min) / TILE as f32).to_pos2(),
                        ((clipped.max - target.min) / TILE as f32).to_pos2(),
                    );
                    painter.image(tex.id(), clipped, uv, Color32::WHITE);
                }
            }
        }
        if let Err(e) = self.queue_tiles(size) {
            self.message = format!("Map: {e}");
        }

        // Overlays: loaded data, download selection, place names, search results, observer.
        let outline = |b: [f64; 4]| {
            let [w, s, e, n] = b;
            let (a, c) = (view.to_screen(n, w), view.to_screen(s, e));
            Rect::from_two_pos(a, c)
        };
        if let Some(b) = self
            .dem
            .as_ref()
            .and_then(terraincraft_core::usgs::dem_bounds)
        {
            painter.rect_stroke(
                outline([b.west, b.south, b.east, b.north]),
                0.,
                Stroke::new(1.5_f32, Color32::from_rgb(80, 200, 255)),
            );
        }
        painter.rect_stroke(
            outline([self.west, self.south, self.east, self.north]),
            0.,
            Stroke::new(2_f32, Color32::from_rgb(255, 150, 30)),
        );
        if let (Some(a), Some(p)) = (self.map_select_start, pointer) {
            painter.rect_stroke(
                Rect::from_two_pos(view.to_screen(a.0, a.1), p),
                0.,
                Stroke::new(2_f32, Color32::YELLOW),
            );
        }
        if self.place_names.len() <= 600 && zoom >= 10 {
            for n in &self.place_names {
                let p = view.to_screen(n.lat, n.lon);
                if rect.contains(p) {
                    let tri = vec![
                        p + Vec2::new(0., -4.),
                        p + Vec2::new(3.5, 2.5),
                        p + Vec2::new(-3.5, 2.5),
                    ];
                    painter.add(egui::Shape::convex_polygon(
                        tri,
                        Color32::from_rgb(120, 60, 20),
                        Stroke::NONE,
                    ));
                }
            }
        }
        let mut hovered_place = None;
        for (i, r) in self.search_results.iter().enumerate() {
            let p = view.to_screen(r.lat, r.lon);
            if !rect.contains(p) {
                continue;
            }
            painter.circle_filled(p, 7., Color32::from_rgb(220, 40, 40));
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                format!("{}", i + 1),
                egui::FontId::proportional(10.),
                Color32::WHITE,
            );
            if pointer.is_some_and(|q| q.distance(p) < 9.) {
                hovered_place = Some((p, format!("{}  ({})", r.name, r.detail)));
            }
        }
        if place_observer {
            let o = view.to_screen(self.observer_lat, self.observer_lon);
            let len = 90_f32;
            for a in [self.bearing - self.fov * 0.5, self.bearing + self.fov * 0.5] {
                let a = a.to_radians();
                let end = o + Vec2::new(len * a.sin() as f32, -len * a.cos() as f32);
                painter.line_segment([o, end], Stroke::new(1.5_f32, Color32::YELLOW));
            }
            painter.circle_filled(o, 5., Color32::LIGHT_GREEN);
            painter.circle_stroke(o, 5., Stroke::new(1_f32, Color32::BLACK));
        }
        if let Some((p, text)) = hovered_place {
            painter.text(
                p + Vec2::new(10., -10.),
                egui::Align2::LEFT_BOTTOM,
                text,
                egui::FontId::proportional(13.),
                Color32::BLACK,
            );
        }
        // Attribution is required by most tile providers.
        let attribution = painter.text(
            rect.right_bottom() + Vec2::new(-4., -3.),
            egui::Align2::RIGHT_BOTTOM,
            &provider.attribution,
            egui::FontId::proportional(11.),
            Color32::BLACK,
        );
        painter.rect_filled(attribution.expand(2.), 2., Color32::from_white_alpha(190));
        painter.text(
            rect.right_bottom() + Vec2::new(-4., -3.),
            egui::Align2::RIGHT_BOTTOM,
            &provider.attribution,
            egui::FontId::proportional(11.),
            Color32::BLACK,
        );
        painter.rect_stroke(rect, 0., Stroke::new(1_f32, Color32::GRAY));
        if let Some(p) = response.hover_pos() {
            let (lat, lon) = view.to_latlon(p);
            ui.small(format!(
                "{lat:.5}°, {lon:.5}°  zoom {zoom}  ·  drag to pan, scroll to zoom, Shift-drag to select the download area{}",
                if place_observer { ", click to place the viewpoint, drag from it to aim" } else { "" }
            ));
        } else {
            ui.small(" ");
        }
    }
}

fn tile_key(provider: &MapProvider, zoom: u8, x: u32, y: u32) -> String {
    let identity = format!("{}:{}", provider.name, provider.url_template);
    let hash = identity.bytes().fold(14695981039346656037_u64, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    });
    format!("{hash:016x}-{zoom}-{x}-{y}")
}
