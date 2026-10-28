//! Interactive basemap: tile download and cache, pan and zoom, and placing the panorama
//! observer. Shared by every workspace. In the Terrain workspace the map window is the
//! download area.
use crate::studio::Studio;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::{fs, sync::mpsc, thread};
use terraincraft_core::map_providers::{latlon_to_world, world_to_latlon, MapProvider};

const TILE: f64 = 256.;
const MAX_ZOOM: f64 = 17.;

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
    /// The area shown, as [west, south, east, north] in degrees. Longitudes are not
    /// wrapped, so a view across the antimeridian has east beyond 180.
    fn bounds(&self) -> [f64; 4] {
        let (hw, hh) = (
            self.rect.width() as f64 / 2. / self.scale,
            self.rect.height() as f64 / 2. / self.scale,
        );
        let lon = |x: f64| x * 360. - 180.;
        let (north, _) = world_to_latlon(0., (self.cy - hh).clamp(0., 1.));
        let (south, _) = world_to_latlon(0., (self.cy + hh).clamp(0., 1.));
        [lon(self.cx - hw), south, lon(self.cx + hw), north]
    }
    /// The view of `rect` that shows all of `b` ([west, south, east, north]) as large as
    /// possible.
    fn fitting(rect: Rect, b: [f64; 4]) -> Self {
        let [w, s, e, n] = b;
        let (x0, y0) = ((w + 180.) / 360., latlon_to_world(n, 0., 0).1);
        let (x1, y1) = ((e + 180.) / 360., latlon_to_world(s, 0., 0).1);
        let scale = (rect.width() as f64 / (x1 - x0).max(1e-12))
            .min(rect.height() as f64 / (y1 - y0).max(1e-12))
            .clamp(TILE, TILE * MAX_ZOOM.exp2());
        View {
            rect,
            cx: ((x0 + x1) / 2.).rem_euclid(1.),
            cy: (y0 + y1) / 2.,
            scale,
        }
    }
    fn zoom(&self) -> f64 {
        (self.scale / TILE).log2()
    }
}

/// Whether two areas match to well under a pixel at any zoom.
fn same_area(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-7)
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
    fn queue_tiles(&mut self, view: &View, zoom: u8, tile_px: f64) -> Result<(), String> {
        let provider = self.map_provider();
        let count = (1_u64 << zoom) as f64;
        let (cx, cy) = (view.cx * count, view.cy * count);
        let size = view.rect.size();
        let (hx, hy) = (size.x as f64 / 2. / tile_px, size.y as f64 / 2. / tile_px);
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
        self.map_zoom = (zoom as f64).min(MAX_ZOOM);
    }

    /// The interactive map, filling `size`.
    ///
    /// - Drag to pan, scroll to zoom around the pointer.
    /// - Without `place_observer` (the Terrain workspace) the download area is the map
    ///   window, so moving the map moves it, until a box is selected: double-click to
    ///   start the box, then click (or drag and release) at the opposite corner. A
    ///   double-click removes the box, and the window is the download area again.
    /// - With `place_observer`, the download area is drawn as a box; click to set the
    ///   panorama viewpoint and drag from it to set the viewing direction.
    pub(crate) fn map_view(&mut self, ui: &mut egui::Ui, size: Vec2, place_observer: bool) {
        let size = Vec2::new(size.x, size.y - if place_observer { 0. } else { 40. });
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_rgb(33, 43, 52));
        let provider = self.map_provider();
        let selects = !place_observer;
        let area = [self.west, self.south, self.east, self.north];
        if selects && rect.is_positive() {
            // The area was changed somewhere else (typed, undone, loaded): the window
            // becomes that area. After another map moved the view, show the area again.
            let changed = !self.map_synced.is_some_and(|s| same_area(s, area));
            if changed {
                self.map_selected = false;
                self.map_select_start = None;
            }
            if changed || self.map_refit {
                let shown = if self.map_selected {
                    let (dx, dy) = ((area[2] - area[0]) * 0.15, (area[3] - area[1]) * 0.15);
                    [area[0] - dx, area[1] - dy, area[2] + dx, area[3] + dy]
                } else {
                    area
                };
                let fit = View::fitting(rect, shown);
                self.map_zoom = fit.zoom();
                (self.map_center_lat, self.map_center_lon) = world_to_latlon(fit.cx, fit.cy);
                self.map_refit = false;
            }
        }
        self.map_zoom = self.map_zoom.clamp(0., MAX_ZOOM);
        let (cx, cy) = latlon_to_world(self.map_center_lat, self.map_center_lon, 0);
        let mut view = View {
            rect,
            cx,
            cy,
            scale: TILE * self.map_zoom.exp2(),
        };
        let start_view = (view.cx, view.cy, view.scale);

        // Input first, so this frame draws the result.
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let mut finish_box = None;
        if selects {
            if response.double_clicked() {
                if self.map_selected || self.map_select_start.is_some() {
                    // Back to the window as the download area.
                    self.map_selected = false;
                    self.map_select_start = None;
                } else if let Some(p) = pointer {
                    self.map_select_start = Some(view.to_latlon(p));
                }
            } else if self.map_select_start.is_some()
                && (response.clicked() || response.drag_stopped())
            {
                finish_box = pointer;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.map_select_start = None;
            }
        }
        if response.drag_started() {
            if let Some(p) = pointer {
                if place_observer {
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
            self.map_aiming = false;
        }
        if let (Some(a), Some(p)) = (self.map_select_start, finish_box) {
            // A box at least a few pixels across becomes the download area.
            let start = view.to_screen(a.0, a.1);
            if (start.x - p.x).abs() >= 4. && (start.y - p.y).abs() >= 4. {
                let b = view.to_latlon(p);
                [self.west, self.south, self.east, self.north] =
                    [a.1.min(b.1), a.0.min(b.0), a.1.max(b.1), a.0.max(b.0)];
                self.map_selected = true;
                self.map_select_start = None;
            }
        }
        if response.clicked() && place_observer {
            if let Some(p) = pointer {
                (self.observer_lat, self.observer_lon) = view.to_latlon(p);
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y) as f64;
            if scroll.abs() > 0.1 {
                // Zoom smoothly, keeping the point under the pointer still.
                let anchor = pointer.unwrap_or(rect.center());
                let (lat, lon) = view.to_latlon(anchor);
                let zoom = (view.zoom() + scroll / 150.).clamp(0., MAX_ZOOM);
                view.scale = TILE * zoom.exp2();
                let (ax, ay) = latlon_to_world(lat, lon, 0);
                view.cx = (ax - (anchor.x - rect.center().x) as f64 / view.scale).rem_euclid(1.);
                view.cy = (ay - (anchor.y - rect.center().y) as f64 / view.scale).clamp(0., 1.);
            }
        }
        if (view.cx, view.cy, view.scale) != start_view {
            self.map_zoom = view.zoom();
            (self.map_center_lat, self.map_center_lon) = world_to_latlon(view.cx, view.cy);
            if place_observer {
                // The Terrain map shows the download area again when it is next shown.
                self.map_refit = true;
            }
        }
        if selects && rect.is_positive() {
            if !self.map_selected {
                [self.west, self.south, self.east, self.north] = view.bounds();
            }
            self.map_synced = Some([self.west, self.south, self.east, self.north]);
        }

        // Tiles: the nearest whole zoom level, scaled to the fractional zoom.
        let zoom = view.zoom();
        let level = (zoom.round() as u8).min(provider.max_zoom);
        let tile_px = view.scale / (1_u64 << level) as f64;
        let count = (1_u64 << level) as f64;
        let (tcx, tcy) = (view.cx * count, view.cy * count);
        let (hx, hy) = (size.x as f64 / 2. / tile_px, size.y as f64 / 2. / tile_px);
        for y in (tcy - hy).floor() as i64..=(tcy + hy).floor() as i64 {
            if y < 0 || y >= count as i64 {
                continue;
            }
            for x in (tcx - hx).floor() as i64..=(tcx + hx).floor() as i64 {
                let key = tile_key(
                    &provider,
                    level,
                    x.rem_euclid(count as i64) as u32,
                    y as u32,
                );
                let Some(tex) = self.map_textures.get(&key) else {
                    continue;
                };
                let min = Pos2::new(
                    rect.center().x + ((x as f64 - tcx) * tile_px) as f32,
                    rect.center().y + ((y as f64 - tcy) * tile_px) as f32,
                );
                let target = Rect::from_min_size(min, Vec2::splat(tile_px as f32));
                let clipped = target.intersect(rect);
                if clipped.is_positive() {
                    let uv = Rect::from_min_max(
                        ((clipped.min - target.min) / tile_px as f32).to_pos2(),
                        ((clipped.max - target.min) / tile_px as f32).to_pos2(),
                    );
                    painter.image(tex.id(), clipped, uv, Color32::WHITE);
                }
            }
        }
        if let Err(e) = self.queue_tiles(&view, level, tile_px) {
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
        let orange = Color32::from_rgb(255, 150, 30);
        if place_observer || self.map_selected {
            let boxed = outline([self.west, self.south, self.east, self.north]);
            if self.map_selected {
                painter.rect_filled(boxed, 0., Color32::from_rgba_unmultiplied(255, 150, 30, 40));
            }
            painter.rect_stroke(boxed, 0., Stroke::new(2_f32, orange));
        }
        if let Some(a) = self.map_select_start {
            let start = view.to_screen(a.0, a.1);
            painter.circle_filled(start, 4., Color32::YELLOW);
            if let Some(p) = response.hover_pos().or(pointer) {
                painter.rect_stroke(
                    Rect::from_two_pos(start, p),
                    0.,
                    Stroke::new(2_f32, Color32::YELLOW),
                );
            }
        }
        if self.place_names.len() <= 600 && zoom >= 10. {
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
        if selects && !self.map_selected {
            // The whole window is the download area.
            painter.rect_stroke(rect.shrink(1.), 0., Stroke::new(2_f32, orange));
        } else {
            painter.rect_stroke(rect, 0., Stroke::new(1_f32, Color32::GRAY));
        }
        if selects {
            let [w, s, e, n] = [self.west, self.south, self.east, self.north];
            let (mx, my) = terraincraft_core::georef::meters_per_degree((s + n) / 2.);
            let mono = |ui: &mut egui::Ui, text: String| {
                ui.label(egui::RichText::new(text).monospace().small());
            };
            mono(
                ui,
                format!(
                    "{}  center {:.5}, {:.5}  ·  {:.2} x {:.2} km",
                    if self.map_selected {
                        "Selected box"
                    } else {
                        "Map window"
                    },
                    (s + n) / 2.,
                    (w + e) / 2.,
                    (e - w) * mx / 1000.,
                    (n - s) * my / 1000.
                ),
            );
            mono(
                ui,
                format!(
                    "NW {n:.5}, {w:.5}   NE {n:.5}, {e:.5}   SW {s:.5}, {w:.5}   SE {s:.5}, {e:.5}"
                ),
            );
        }
        let help = if place_observer {
            "drag to pan, scroll to zoom, click to place the viewpoint, drag from it to aim"
        } else if self.map_select_start.is_some() {
            "click the opposite corner to finish the box  ·  Esc or double-click to cancel"
        } else if self.map_selected {
            "drag to pan, scroll to zoom  ·  double-click to remove the box"
        } else {
            "drag to pan, scroll to zoom  ·  double-click to start a selection box"
        };
        if let Some(p) = response.hover_pos() {
            let (lat, lon) = view.to_latlon(p);
            ui.small(format!(
                "Pointer {lat:.5}, {lon:.5}  zoom {zoom:.1}  ·  {help}"
            ));
        } else {
            ui.small(help);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_an_area_shows_it_and_reads_it_back() {
        let rect = Rect::from_min_size(Pos2::new(10., 20.), Vec2::new(800., 500.));
        // Mount Rainier, wider than tall like the window.
        let area = [-121.85, 46.8, -121.7, 46.9];
        let view = View::fitting(rect, area);
        let shown = view.bounds();
        // The window contains the area and matches it along one axis.
        assert!(shown[0] <= area[0] + 1e-9 && shown[2] >= area[2] - 1e-9);
        assert!(shown[1] <= area[1] + 1e-9 && shown[3] >= area[3] - 1e-9);
        let fits_x = (shown[2] - shown[0] - (area[2] - area[0])).abs() < 1e-6;
        let fits_y = (shown[3] - shown[1] - (area[3] - area[1])).abs() < 1e-6;
        assert!(fits_x || fits_y, "{shown:?}");
        // Framing the shown area again changes nothing, so the map and the area agree.
        assert!(same_area(View::fitting(rect, shown).bounds(), shown));
    }

    #[test]
    fn zoom_and_scale_agree() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400., 300.));
        let v = View {
            rect,
            cx: 0.5,
            cy: 0.5,
            scale: TILE * 10.5_f64.exp2(),
        };
        assert!((v.zoom() - 10.5).abs() < 1e-12);
    }
}
