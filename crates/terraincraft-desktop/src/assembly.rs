//! 3D print tile assembly preview and connector mesh generation.
use crate::studio::Studio;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};
use terraincraft_core::{mesh, tiles, Dem};

impl Studio {
    // Generate the same connector-modified triangles for both the desktop STL exporter
    // and the wireframe inspector. An unsupported cut is an error, not a fallback.
    pub(crate) fn tile_print_mesh(
        &self,
        dem: &Dem,
        t: terraincraft_core::Tile,
        holes: &[terraincraft_core::seam_joints::SeamHole],
    ) -> Result<Vec<[[f64; 3]; 3]>, String> {
        match self.assembly_connector_mode {
            0 => mesh(dem, self.settings(), t),
            1 => {
                let ts = tiles(dem, self.settings(), self.bed_x, self.bed_y, self.margin)?;
                let reg = terraincraft_core::registration::RegistrationSettings::default();
                let guides =
                    terraincraft_core::registration::guides(dem, self.settings(), &ts, reg)?;
                let idx = ts
                    .iter()
                    .position(|v| v.row == t.row && v.col == t.col)
                    .ok_or("Unknown tile")?;
                terraincraft_core::socket_mesh::mesh_with_sockets(
                    dem,
                    self.settings(),
                    t,
                    &guides[idx],
                    reg,
                    32,
                )
            }
            2 => {
                let selected: Vec<_> = holes
                    .iter()
                    .filter(|h| h.tile_row == t.row && h.tile_col == t.col)
                    .cloned()
                    .collect();
                terraincraft_core::seam_socket_mesh::mesh_with_seam_sockets(
                    dem,
                    self.settings(),
                    t,
                    &selected,
                    32,
                )
            }
            _ => Err("Unknown connector mode".into()),
        }
    }
    pub(crate) fn draw_assembly_3d(&mut self, ui: &mut egui::Ui, dem: &Dem) {
        ui.label("3D assembly inspection · drag to orbit · scroll to zoom · optional exploded tile spacing");
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
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_rgb(23, 29, 37));
        let parts = match tiles(dem, self.settings(), self.bed_x, self.bed_y, self.margin) {
            Ok(v) => v,
            Err(e) => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    e,
                    egui::FontId::proportional(14.),
                    Color32::LIGHT_RED,
                );
                return;
            }
        };
        let (lo, hi) = dem.min_max();
        let span = (hi - lo).max(1e-9);
        let max_col = parts.iter().map(|t| t.col).max().unwrap_or(0) as f64;
        let max_row = parts.iter().map(|t| t.row).max().unwrap_or(0) as f64;
        let gap = if self.assembly_exploded {
            self.assembly_gap_mm.max(0.)
        } else {
            0.
        };
        let full_x = self.width + gap * max_col;
        let full_y = self.height + gap * max_row;
        let center_x = full_x * 0.5;
        let center_y = full_y * 0.5;
        let scale = rect.width().min(rect.height()) * 0.65 * self.orbit_zoom
            / (full_x.max(full_y) as f32).max(1.);
        let yaw = self.orbit_yaw;
        let pitch = self.orbit_pitch;
        let project = |x: f64, y: f64, z: f64| -> Pos2 {
            let xx = (x - center_x) as f32;
            let yy = (y - center_y) as f32;
            let cy = yaw.cos();
            let sy = yaw.sin();
            let rx = xx * cy - yy * sy;
            let ry = xx * sy + yy * cy;
            let py = ry * pitch.cos() - (z as f32) * pitch.sin();
            Pos2::new(rect.center().x + rx * scale, rect.center().y + py * scale)
        };
        // The exact-mesh option uses the same terrain triangle generator as STL export.
        // It is deliberately capped to keep the immediate-mode egui painter responsive.
        if self.assembly_mesh_mode {
            let total_tiles = parts.len();
            let mut drawn = 0usize;
            let mut skipped = 0usize;
            let mut failures = 0usize;
            let mut invalid = 0usize;
            let holes = if self.assembly_connector_mode == 2 {
                match terraincraft_core::seam_joints::plan(
                    dem,
                    &parts,
                    self.settings(),
                    terraincraft_core::seam_joints::SeamSettings::default(),
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            e,
                            egui::FontId::proportional(14.),
                            Color32::LIGHT_RED,
                        );
                        return;
                    }
                }
            } else {
                Vec::new()
            };
            // Painter-order software shading: sort all visible triangles by camera-space
            // depth before drawing. This requires no GPU backend or external renderer.
            let mut shaded_faces: Vec<(f32, [Pos2; 3], Color32)> = Vec::new();
            const MAX_SHADED_FACES: usize = 32_000;
            // Painter's algorithm: sort all faces across all tiles by camera-space depth.
            // This avoids wireframe see-through without requiring a separate GPU renderer.
            if self.assembly_shaded {
                let mut faces_to_draw: Vec<(f32, [Pos2; 3], Color32)> = Vec::new();
                let mut triangle_count = 0usize;
                let mut skipped_faces = 0usize;
                const MAX_SHADED_FACES: usize = 60_000;
                for tile in &parts {
                    let faces = match self.tile_print_mesh(dem, *tile, &holes) {
                        Ok(f) => f,
                        Err(_) => {
                            failures += 1;
                            continue;
                        }
                    };
                    let audit = terraincraft_core::mesh_audit::audit(&faces);
                    if audit.boundary_edges > 0
                        || audit.nonmanifold_edges > 0
                        || audit.degenerate_triangles > 0
                    {
                        invalid += 1;
                    }
                    let stride =
                        (faces.len().saturating_mul(parts.len()) / MAX_SHADED_FACES).max(1);
                    let offset_x = tile.x0 as f64 * self.width / (dem.width - 1) as f64
                        + tile.col as f64 * gap;
                    let offset_y = tile.y0 as f64 * self.height / (dem.height - 1) as f64
                        + tile.row as f64 * gap;
                    let base_color = if (tile.row + tile.col) % 2 == 0 {
                        [122., 202., 178.]
                    } else {
                        [240., 180., 120.]
                    };
                    for (i, tri) in faces.iter().enumerate() {
                        triangle_count += 1;
                        if i % stride != 0 {
                            skipped_faces += 1;
                            continue;
                        }
                        let a = tri[0];
                        let b = tri[1];
                        let c = tri[2];
                        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        let n = [
                            u[1] * v[2] - u[2] * v[1],
                            u[2] * v[0] - u[0] * v[2],
                            u[0] * v[1] - u[1] * v[0],
                        ];
                        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                        if len < 1e-12 {
                            skipped_faces += 1;
                            continue;
                        }
                        let light = [0.35_f64, -0.45, 0.82];
                        let diffuse =
                            ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len).abs();
                        let brightness = (0.35 + 0.65 * diffuse).clamp(0., 1.);
                        let color = Color32::from_rgb(
                            (base_color[0] * brightness) as u8,
                            (base_color[1] * brightness) as u8,
                            (base_color[2] * brightness) as u8,
                        );
                        let points = tri.map(|p| project(p[0] + offset_x, p[1] + offset_y, p[2]));
                        // Rotated y coordinate is camera depth after the pitch transform.
                        let depth = tri
                            .iter()
                            .map(|p| {
                                let x = (p[0] + offset_x - center_x) as f32;
                                let y = (p[1] + offset_y - center_y) as f32;
                                let ry = x * yaw.sin() + y * yaw.cos();
                                ry * pitch.sin() + (p[2] as f32) * pitch.cos()
                            })
                            .sum::<f32>()
                            / 3.;
                        faces_to_draw.push((depth, points, color));
                    }
                }
                // Farther surfaces are painted first; nearest surfaces cover them.
                faces_to_draw.sort_by(|a, b| b.0.total_cmp(&a.0));
                for (_, points, color) in faces_to_draw {
                    painter.add(egui::Shape::convex_polygon(
                        points.to_vec(),
                        color,
                        Stroke::NONE,
                    ));
                }
                if self.show_assembly_labels {
                    for tile in &parts {
                        let cx = (tile.x0 + tile.x1) as f64 * 0.5 * self.width
                            / (dem.width - 1) as f64
                            + tile.col as f64 * gap;
                        let cy = (tile.y0 + tile.y1) as f64 * 0.5 * self.height
                            / (dem.height - 1) as f64
                            + tile.row as f64 * gap;
                        painter.text(
                            project(cx, cy, self.base + self.relief * self.exaggeration),
                            egui::Align2::CENTER_CENTER,
                            format!("R{} C{}", tile.row + 1, tile.col + 1),
                            egui::FontId::monospace(12.),
                            Color32::WHITE,
                        );
                    }
                }
                painter.text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,format!("Shaded mesh · {} tiles · {} triangles · {} skipped · {} generation errors · {} topology warnings",total_tiles,triangle_count,skipped_faces,failures,invalid),egui::FontId::monospace(12.),Color32::WHITE);
                return;
            }
            const MAX_EDGES: usize = 55_000;
            for tile in &parts {
                let faces = match self.tile_print_mesh(dem, *tile, &holes) {
                    Ok(f) => f,
                    Err(_) => {
                        failures += 1;
                        continue;
                    }
                };
                let audit = terraincraft_core::mesh_audit::audit(&faces);
                if audit.boundary_edges > 0
                    || audit.nonmanifold_edges > 0
                    || audit.degenerate_triangles > 0
                {
                    invalid += 1;
                }
                let stride = if self.assembly_shaded {
                    (faces.len().saturating_mul(parts.len()) / MAX_SHADED_FACES).max(1)
                } else {
                    (faces.len().saturating_mul(3) / MAX_EDGES).max(1)
                };
                let color = if (tile.row + tile.col) % 2 == 0 {
                    Color32::from_rgb(122, 202, 178)
                } else {
                    Color32::from_rgb(240, 180, 120)
                };
                for (i, tri) in faces.iter().enumerate() {
                    if i % stride != 0 {
                        skipped += 1;
                        continue;
                    }
                    let offset_x = tile.x0 as f64 * self.width / (dem.width - 1) as f64
                        + tile.col as f64 * gap;
                    let offset_y = tile.y0 as f64 * self.height / (dem.height - 1) as f64
                        + tile.row as f64 * gap;
                    let points = tri.map(|v| project(v[0] + offset_x, v[1] + offset_y, v[2]));
                    if self.assembly_shaded {
                        let a = tri[0];
                        let b = tri[1];
                        let c = tri[2];
                        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        let n = [
                            u[1] * v[2] - u[2] * v[1],
                            u[2] * v[0] - u[0] * v[2],
                            u[0] * v[1] - u[1] * v[0],
                        ];
                        let norm = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                        if norm <= 1e-12 {
                            continue;
                        }
                        let light = [0.4_f64, 0.35, 0.85];
                        let brightness = (0.28
                            + 0.72
                                * ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / norm)
                                    .abs())
                        .clamp(0., 1.) as f32;
                        let tint = Color32::from_rgb(
                            (color.r() as f32 * brightness) as u8,
                            (color.g() as f32 * brightness) as u8,
                            (color.b() as f32 * brightness) as u8,
                        );
                        // Camera-space depth (the axis orthogonal to the screen).
                        let cx = (a[0] + b[0] + c[0]) / 3. + offset_x;
                        let cy = (a[1] + b[1] + c[1]) / 3. + offset_y;
                        let cz = (a[2] + b[2] + c[2]) / 3.;
                        let ry = -(cx - center_x) * yaw.sin() as f64
                            + (cy - center_y) * yaw.cos() as f64;
                        let depth = (ry * pitch.sin() as f64 + cz * pitch.cos() as f64) as f32;
                        shaded_faces.push((depth, points, tint));
                    } else {
                        for e in 0..3 {
                            painter.line_segment(
                                [points[e], points[(e + 1) % 3]],
                                Stroke::new(0.6_f32, color),
                            );
                            drawn += 1;
                        }
                    }
                }
                if self.show_assembly_labels {
                    let cx = (tile.x0 + tile.x1) as f64 * 0.5 * self.width / (dem.width - 1) as f64
                        + tile.col as f64 * gap;
                    let cy = (tile.y0 + tile.y1) as f64 * 0.5 * self.height
                        / (dem.height - 1) as f64
                        + tile.row as f64 * gap;
                    painter.text(
                        project(cx, cy, self.base + self.relief * self.exaggeration),
                        egui::Align2::CENTER_CENTER,
                        format!("R{} C{}", tile.row + 1, tile.col + 1),
                        egui::FontId::monospace(12.),
                        Color32::WHITE,
                    );
                }
            }
            if self.assembly_shaded {
                shaded_faces.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (_, pts, tint) in shaded_faces {
                    painter.add(egui::Shape::convex_polygon(
                        pts.to_vec(),
                        tint,
                        Stroke::NONE,
                    ));
                    drawn += 1;
                }
            }
            painter.text(rect.left_top()+Vec2::new(8.,8.),egui::Align2::LEFT_TOP,format!("{} tiles · {} drawn edges · {} skipped faces · {} generation errors · {} topology warnings · connector mode {}",total_tiles,drawn,skipped,failures,invalid,self.assembly_connector_mode),egui::FontId::monospace(12.),Color32::LIGHT_GRAY);
            return;
        }
        for tile in &parts {
            let stride_x = ((tile.x1 - tile.x0 + 35) / 36).max(1);
            let stride_y = ((tile.y1 - tile.y0 + 35) / 36).max(1);
            let xs: Vec<usize> = (tile.x0..tile.x1)
                .step_by(stride_x)
                .chain(std::iter::once(tile.x1))
                .collect();
            let ys: Vec<usize> = (tile.y0..tile.y1)
                .step_by(stride_y)
                .chain(std::iter::once(tile.y1))
                .collect();
            let sample = |x: usize, y: usize| {
                let px = x as f64 / (dem.width - 1) as f64 * self.width + tile.col as f64 * gap;
                let py = y as f64 / (dem.height - 1) as f64 * self.height + tile.row as f64 * gap;
                let z = self.base + (dem.at(x, y) - lo) / span * self.relief * self.exaggeration;
                project(px, py, z)
            };
            let color = if (tile.row + tile.col) % 2 == 0 {
                Color32::from_rgb(122, 202, 178)
            } else {
                Color32::from_rgb(240, 180, 120)
            };
            for &y in &ys {
                let line: Vec<Pos2> = xs.iter().map(|&x| sample(x, y)).collect();
                if line.len() > 1 {
                    painter.add(egui::Shape::line(line, Stroke::new(1_f32, color)));
                }
            }
            for &x in &xs {
                let line: Vec<Pos2> = ys.iter().map(|&y| sample(x, y)).collect();
                if line.len() > 1 {
                    painter.add(egui::Shape::line(line, Stroke::new(0.8_f32, color)));
                }
            }
            let corners = [
                (tile.x0, tile.y0),
                (tile.x1, tile.y0),
                (tile.x1, tile.y1),
                (tile.x0, tile.y1),
            ];
            for i in 0..4 {
                let (x, y) = corners[i];
                let (nx, ny) = corners[(i + 1) % 4];
                painter.line_segment(
                    [sample(x, y), sample(nx, ny)],
                    Stroke::new(2_f32, Color32::WHITE),
                );
            }
            if self.show_assembly_labels {
                let cx = (tile.x0 + tile.x1) / 2;
                let cy = (tile.y0 + tile.y1) / 2;
                painter.text(
                    sample(cx, cy),
                    egui::Align2::CENTER_CENTER,
                    format!("R{} C{}", tile.row + 1, tile.col + 1),
                    egui::FontId::monospace(12.),
                    Color32::WHITE,
                );
            }
        }
        painter.text(
            rect.left_top() + Vec2::new(8., 8.),
            egui::Align2::LEFT_TOP,
            format!("{} tiles · gap {:.1} mm · preview only", parts.len(), gap),
            egui::FontId::monospace(12.),
            Color32::LIGHT_GRAY,
        );
    }
}
