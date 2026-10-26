//! Main canvas dispatch and terrain 3D preview.
use crate::gpu_view;
use crate::studio::{Studio, View};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use terraincraft_core::cartography::Viewpoint;
use terraincraft_core::engraving_preview;
use terraincraft_core::geoview::GeographicExtent;
use terraincraft_core::{tiles, Dem};

impl Studio {
    pub(crate) fn draw_3d(&mut self, ui: &mut egui::Ui, dem: &Dem) {
        ui.label("Drag to orbit · Scroll to zoom · Terrain mesh preview (not G-code)");
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, Sense::drag());
        if response.dragged() {
            let delta = response.drag_delta();
            self.orbit_yaw += delta.x * 0.008;
            self.orbit_pitch = (self.orbit_pitch + delta.y * 0.008).clamp(0.1, 1.45);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            self.orbit_zoom = (self.orbit_zoom * (scroll * 0.001).exp()).clamp(0.2, 8.);
        }
        if self.assembly_gpu {
            let parts = match tiles(dem, self.settings(), self.bed_x, self.bed_y, self.margin) {
                Ok(p) => p,
                Err(e) => {
                    ui.painter_at(rect).text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        e,
                        egui::FontId::proportional(14.),
                        Color32::LIGHT_RED,
                    );
                    return;
                }
            };
            let holes = if self.assembly_connector_mode == 2 {
                terraincraft_core::seam_joints::plan(
                    dem,
                    &parts,
                    self.settings(),
                    terraincraft_core::seam_joints::SeamSettings::default(),
                )
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            let gap = if self.assembly_exploded {
                self.assembly_gap_mm.max(0.)
            } else {
                0.
            };
            let mut batches = Vec::new();
            let mut errors = 0usize;
            for tile in &parts {
                match self.tile_print_mesh(dem, *tile, &holes) {
                    Ok(faces) => {
                        let ox = tile.x0 as f64 * self.width / (dem.width - 1) as f64
                            + tile.col as f64 * gap;
                        let oy = tile.y0 as f64 * self.height / (dem.height - 1) as f64
                            + tile.row as f64 * gap;
                        let color = if (tile.row + tile.col) % 2 == 0 {
                            [0.48, 0.79, 0.70]
                        } else {
                            [0.94, 0.71, 0.47]
                        };
                        batches.push((faces, [ox, oy], color));
                    }
                    Err(_) => errors += 1,
                }
            }
            let mesh = gpu_view::GpuMesh::from_tiles(&batches);
            let triangles = mesh.triangle_count;
            // Callback paints into the previously allocated rect.
            let camera = gpu_view::Camera {
                yaw: self.orbit_yaw,
                pitch: self.orbit_pitch,
                zoom: self.orbit_zoom,
                center: [
                    ((self.width + gap * parts.iter().map(|t| t.col).max().unwrap_or(0) as f64)
                        * 0.5) as f32,
                    ((self.height + gap * parts.iter().map(|t| t.row).max().unwrap_or(0) as f64)
                        * 0.5) as f32,
                    (self.base + self.relief * self.exaggeration) as f32 * 0.5,
                ],
                extent: (self.width.max(self.height) + gap * parts.len() as f64) as f32,
            };
            gpu_view::paint_rect(ui, rect, mesh, camera);
            ui.painter_at(rect).text(
                rect.left_top() + Vec2::new(8., 8.),
                egui::Align2::LEFT_TOP,
                format!(
                    "GPU OpenGL depth buffer · {} triangles · {} mesh errors",
                    triangles, errors
                ),
                egui::FontId::monospace(12.),
                Color32::WHITE,
            );
            return;
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_rgb(23, 29, 37));
        let (lo, hi) = dem.min_max();
        let span = (hi - lo).max(1e-9);
        let stride = dem.width.max(dem.height).div_ceil(86).max(1);
        let xs: Vec<usize> = (0..dem.width)
            .step_by(stride)
            .chain(std::iter::once(dem.width - 1))
            .collect();
        let ys: Vec<usize> = (0..dem.height)
            .step_by(stride)
            .chain(std::iter::once(dem.height - 1))
            .collect();
        let point = |x: usize, y: usize| -> Pos2 {
            let xx = (x as f32 / (dem.width - 1) as f32 - 0.5) * 2.;
            let yy =
                (y as f32 / (dem.height - 1) as f32 - 0.5) * 2. * (self.height / self.width) as f32;
            let zz = (((dem.at(x, y) - lo) / span) as f32
                * self.relief as f32
                * self.exaggeration as f32
                / self.width as f32)
                * 2.;
            let cy = self.orbit_yaw.cos();
            let sy = self.orbit_yaw.sin();
            let rx = xx * cy - yy * sy;
            let ry = xx * sy + yy * cy;
            let py = ry * self.orbit_pitch.cos() - zz * self.orbit_pitch.sin();
            let scale = rect.width().min(rect.height()) * 0.33 * self.orbit_zoom;
            Pos2::new(rect.center().x + rx * scale, rect.center().y + py * scale)
        };
        for (j, &y) in ys.iter().enumerate() {
            let mut points = Vec::with_capacity(xs.len());
            for &x in &xs {
                points.push(point(x, y));
            }
            let shade = (110. + 110. * j as f32 / ys.len() as f32) as u8;
            painter.add(egui::Shape::line(
                points,
                Stroke::new(0.9_f32, Color32::from_rgb(70, shade, 145)),
            ));
        }
        for &x in &xs {
            let points: Vec<Pos2> = ys.iter().map(|&y| point(x, y)).collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(0.8_f32, Color32::from_rgb(85, 170, 140)),
            ));
        }
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui) {
        let Some(dem) = self.dem.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label("Load a DEM (.csv, .asc or .tif) to preview terrain");
            });
            return;
        };
        if self.view == View::Engraving {
            ui.heading(if self.engraving_perspective {
                "Perspective engraving preview"
            } else {
                "Topographic engraving preview"
            });
            ui.label("Live vector geometry preview · drag labels to reposition · export produces separate SVG groups.");
            let size = ui.available_width().min(560.);
            let h = size * (self.height / self.width).clamp(0.2, 2.0) as f32;
            let (r, response) = ui.allocate_exact_size(Vec2::new(size, h), Sense::click_and_drag());
            let painter = ui.painter_at(r);
            painter.rect_filled(r, 2., Color32::from_rgb(229, 207, 171));
            painter.rect_stroke(r, 2., Stroke::new(1_f32, Color32::DARK_GRAY));
            let geometry = if self.engraving_perspective {
                let mut vp = Viewpoint {
                    x: self.observer_x * (dem.width - 1) as f64,
                    y: self.observer_y * (dem.height - 1) as f64,
                    bearing_deg: self.bearing,
                    pitch_deg: self.pitch,
                    fov_deg: self.fov,
                    eye_height_m: self.eye_height,
                    max_distance_m: self.max_distance,
                    pixel_size_x_m: self.pixel_size_m,
                    pixel_size_y_m: self.pixel_size_m,
                };
                if self.geo_view_enabled {
                    let extent = GeographicExtent {
                        west: self.west,
                        south: self.south,
                        east: self.east,
                        north: self.north,
                    };
                    match extent.viewpoint(
                        self.observer_lat,
                        self.observer_lon,
                        dem.width,
                        dem.height,
                        vp,
                    ) {
                        Ok(v) => vp = v,
                        Err(e) => {
                            ui.colored_label(Color32::DARK_RED, e);
                            return;
                        }
                    }
                }
                engraving_preview::perspective(
                    &dem,
                    vp,
                    self.width,
                    self.height,
                    self.depth_bands,
                    240,
                )
            } else {
                engraving_preview::topdown(
                    &dem,
                    self.width,
                    self.height,
                    self.contour_interval,
                    120,
                )
            };
            match geometry {
                Ok(paths) => {
                    for path in paths {
                        let points: Vec<Pos2> = path
                            .points
                            .iter()
                            .map(|(x, y)| {
                                Pos2::new(
                                    r.left() + (*x / self.width) as f32 * r.width(),
                                    r.top() + (*y / self.height) as f32 * r.height(),
                                )
                            })
                            .collect();
                        if points.len() > 1 {
                            painter.add(egui::Shape::line(
                                points,
                                Stroke::new(
                                    if path.layer % 5 == 0 { 1.35 } else { 0.8_f32 },
                                    Color32::from_rgb(94, 67, 43),
                                ),
                            ));
                        }
                    }
                }
                Err(e) => {
                    ui.colored_label(Color32::DARK_RED, format!("Preview: {e}"));
                }
            }
            painter.text(
                Pos2::new(r.center().x, r.top() + 10.),
                egui::Align2::CENTER_TOP,
                &self.engraving_title,
                egui::FontId::proportional(16.),
                Color32::BLACK,
            );
            for (i, a) in self.annotations.iter_mut().enumerate() {
                let p = Pos2::new(
                    r.left() + a.x as f32 * r.width(),
                    r.top() + a.y as f32 * r.height(),
                );
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    &a.text,
                    egui::FontId::proportional(14.),
                    Color32::BLACK,
                );
                let hit = Rect::from_center_size(p, Vec2::new(120., 22.));
                let drag = ui.interact(hit, ui.id().with(("annotation", i)), Sense::drag());
                if drag.dragged() {
                    let d = drag.drag_delta();
                    a.x = (a.x + d.x as f64 / r.width() as f64).clamp(0., 1.);
                    a.y = (a.y + d.y as f64 / r.height() as f64).clamp(0., 1.);
                }
            }
            let _ = response;
            if self.show_geo_map && self.engraving_perspective {
                self.draw_geo_map(ui);
            } else {
                self.draw_3d(ui, &dem);
            }
            return;
        }
        if self.view == View::Print {
            self.draw_assembly_3d(ui, &dem);
            return;
        }
        if self.view == View::Terrain {
            self.draw_3d(ui, &dem);
            return;
        }
        let available = ui.available_size();
        let aspect = match self.view {
            View::Laser => self.sheet_x / self.sheet_y,
            _ => self.width / self.height,
        }
        .max(0.05) as f32;
        let w = available.x.min(available.y * aspect).max(10.);
        let h = w / aspect;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2., Color32::from_rgb(28, 36, 42));
        if self.view == View::Laser {
            match self.laser_data(&dem) {
                Ok((parts, placements)) => {
                    for p in placements.iter().filter(|p| p.sheet == self.sheet) {
                        if let Some(part) = parts.iter().find(|v| v.id == p.part_id) {
                            let minx = part
                                .outline
                                .iter()
                                .map(|v| v.0)
                                .fold(f64::INFINITY, f64::min);
                            let miny = part
                                .outline
                                .iter()
                                .map(|v| v.1)
                                .fold(f64::INFINITY, f64::min);
                            let pts: Vec<Pos2> = part
                                .outline
                                .iter()
                                .map(|(x, y)| {
                                    Pos2::new(
                                        rect.left()
                                            + ((p.x_mm + x - minx) / self.sheet_x) as f32
                                                * rect.width(),
                                        rect.top()
                                            + ((p.y_mm + y - miny) / self.sheet_y) as f32
                                                * rect.height(),
                                    )
                                })
                                .collect();
                            painter.add(egui::Shape::line(
                                pts,
                                Stroke::new(1.3_f32, Color32::LIGHT_RED),
                            ));
                        }
                    }
                }
                Err(e) => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        e,
                        egui::FontId::proportional(13.),
                        Color32::LIGHT_RED,
                    );
                }
            }
        } else {
            // Downsampled shaded relief preview; native Rust painting, no browser or GPU shader dependency.
            let sx = (dem.width / 180).max(1);
            let sy = (dem.height / 180).max(1);
            let (lo, hi) = dem.min_max();
            let span = (hi - lo).max(1e-12);
            for y in (0..dem.height - 1).step_by(sy) {
                for x in (0..dem.width - 1).step_by(sx) {
                    let z = (dem.at(x, y) - lo) / span;
                    let shade = (65. + z * 165.).clamp(0., 255.) as u8;
                    let col = Color32::from_rgb(
                        (shade as f64 * 0.75) as u8,
                        shade,
                        (shade as f64 * 0.72) as u8,
                    );
                    let r = Rect::from_min_max(
                        Pos2::new(
                            rect.left() + x as f32 / (dem.width - 1) as f32 * rect.width(),
                            rect.top() + y as f32 / (dem.height - 1) as f32 * rect.height(),
                        ),
                        Pos2::new(
                            rect.left()
                                + ((x + sx).min(dem.width - 1)) as f32 / (dem.width - 1) as f32
                                    * rect.width(),
                            rect.top()
                                + ((y + sy).min(dem.height - 1)) as f32 / (dem.height - 1) as f32
                                    * rect.height(),
                        ),
                    );
                    painter.rect_filled(r, 0., col);
                }
            }
            if self.view == View::Print {
                if let Ok(ts) = tiles(&dem, self.settings(), self.bed_x, self.bed_y, self.margin) {
                    for t in ts {
                        let r = Rect::from_min_max(
                            Pos2::new(
                                rect.left() + t.x0 as f32 / (dem.width - 1) as f32 * rect.width(),
                                rect.top() + t.y0 as f32 / (dem.height - 1) as f32 * rect.height(),
                            ),
                            Pos2::new(
                                rect.left() + t.x1 as f32 / (dem.width - 1) as f32 * rect.width(),
                                rect.top() + t.y1 as f32 / (dem.height - 1) as f32 * rect.height(),
                            ),
                        );
                        painter.rect_stroke(r, 0., Stroke::new(1.5_f32, Color32::YELLOW));
                    }
                }
            }
        }
    }
}
