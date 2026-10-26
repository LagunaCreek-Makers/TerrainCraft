//! Laser layout and manufacturing file export.
use crate::studio::{Studio, View};
use std::{fs, path::Path};
use terraincraft_core::export_settings::PrintExportSettings;
use terraincraft_core::laser::{cut_parts, layout, sheet_svg, split_oversized, CutPart, Placement};
use terraincraft_core::{tiles, Dem};

impl Studio {
    pub(crate) fn laser_data(&self, dem: &Dem) -> Result<(Vec<CutPart>, Vec<Placement>), String> {
        let parts = cut_parts(dem, self.width, self.height, &self.levels(dem))?;
        let parts = split_oversized(
            &parts,
            self.sheet_x - 2. * self.gap,
            self.sheet_y - 2. * self.gap,
        )?;
        let placements = layout(&parts, self.sheet_x, self.sheet_y, self.gap)?;
        terraincraft_core::inspection::preflight(
            &parts,
            &placements,
            self.sheet_x,
            self.sheet_y,
            self.gap,
        )?;
        Ok((parts, placements))
    }
    pub(crate) fn export(&mut self) -> Result<String, String> {
        let dem = self.dem.as_ref().ok_or("Load a terrain first")?;
        self.settings().validate()?;
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        match self.view {
            View::Engraving => {
                let svg = self.engraving_svg(dem)?;
                let filename = if self.engraving_perspective {
                    "perspective_engraving.svg"
                } else {
                    "topdown_engraving.svg"
                };
                fs::write(Path::new(&self.output).join(filename), svg)
                    .map_err(|e| e.to_string())?;
                Ok(format!("Exported {}", filename))
            }
            View::Print => {
                let ts = tiles(dem, self.settings(), self.bed_x, self.bed_y, self.margin)?;

                fs::write(
                    Path::new(&self.output).join("print_tile_manifest.csv"),
                    terraincraft_core::print_export::tile_manifest(dem, self.settings(), &ts)?,
                )
                .map_err(|e| e.to_string())?;
                let reg = terraincraft_core::registration::RegistrationSettings::default();
                let guides =
                    terraincraft_core::registration::guides(dem, self.settings(), &ts, reg)?;
                let holes = if self.assembly_connector_mode == 2 {
                    terraincraft_core::seam_joints::plan(
                        dem,
                        &ts,
                        self.settings(),
                        terraincraft_core::seam_joints::SeamSettings::default(),
                    )?
                } else {
                    Vec::new()
                };
                if self.assembly_connector_mode == 2 {
                    let checks = terraincraft_core::seam_preflight::inspect(
                        dem,
                        self.settings(),
                        &ts,
                        &holes,
                    )?;
                    fs::write(
                        Path::new(&self.output).join("seam_socket_preflight.csv"),
                        terraincraft_core::seam_preflight::csv(&checks),
                    )
                    .map_err(|e| e.to_string())?;
                    if checks.iter().any(|c| !c.compatible) {
                        return Err(
                            "Sidewall socket preflight failed; see seam_socket_preflight.csv"
                                .into(),
                        );
                    }
                }
                let opts = PrintExportSettings {
                    base_mm: self.base,
                    labels: self.print_labels,
                    simplify_mm: self.print_simplify.then_some(self.simplify_mm.max(0.)),
                    ..PrintExportSettings::default()
                };
                let s = self.settings();
                let mut audit_csv = String::from(terraincraft_core::mesh_audit::CSV_HEADER);
                let mut finished = Vec::new();
                let mut warnings = Vec::new();
                for t in &ts {
                    let name = format!("tile_r{:02}_c{:02}.stl", t.row + 1, t.col + 1);
                    // Previews use the raw mesh; exports are turned north-up and finished.
                    let mut faces = self.tile_print_mesh(dem, *t, &holes)?;
                    terraincraft_core::north_up(&mut faces, dem, s, *t);
                    let (faces, w) = terraincraft_core::finish_tile(faces, s, *t, &opts)?;
                    warnings.extend(w);
                    let audit = terraincraft_core::mesh_audit::audit(&faces);
                    audit_csv.push_str(&audit.csv_row(&name));
                    terraincraft_core::print_export::write_binary_stl(
                        &Path::new(&self.output).join(name),
                        &faces,
                    )
                    .map_err(|e| e.to_string())?;
                    finished.push((*t, faces));
                }
                fs::write(Path::new(&self.output).join("mesh_audit.csv"), audit_csv)
                    .map_err(|e| e.to_string())?;
                let max_row = ts.iter().map(|t| t.row).max().unwrap_or(0);
                let parts: Vec<_> = finished
                    .iter()
                    .map(|(t, triangles)| {
                        let [x, y] = terraincraft_core::north_up_origin(dem, s, *t);
                        terraincraft_core::threemf::Part {
                            name: format!("Tile {}", terraincraft_core::tile_label(*t)),
                            triangles,
                            offset_mm: [
                                x + t.col as f64 * 10.,
                                y + (max_row - t.row) as f64 * 10.,
                                0.,
                            ],
                        }
                    })
                    .collect();
                let threemf = Path::new(&self.output).join("terrain_tiles.3mf");
                fs::write(&threemf, terraincraft_core::threemf::write(&parts)?)
                    .map_err(|e| e.to_string())?;
                self.last_3mf = Some(threemf.to_string_lossy().into_owned());
                let title = format!(
                    "TerrainCraft: {}",
                    Path::new(&self.path)
                        .file_stem()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
                fs::write(
                    Path::new(&self.output).join("assembly_sheet.svg"),
                    terraincraft_core::assembly_sheet::svg(dem, s, &ts, &title)?,
                )
                .map_err(|e| e.to_string())?;
                for g in &guides {
                    fs::write(
                        Path::new(&self.output).join(format!(
                            "registration_r{:02}_c{:02}.svg",
                            g.row + 1,
                            g.col + 1
                        )),
                        terraincraft_core::registration::guide_svg(g, reg)?,
                    )
                    .map_err(|e| e.to_string())?;
                }
                fs::write(
                    Path::new(&self.output).join("registration_manifest.csv"),
                    terraincraft_core::registration::guide_manifest(&guides, reg),
                )
                .map_err(|e| e.to_string())?;
                terraincraft_core::print_export::write_binary_stl(
                    &Path::new(&self.output).join("registration_pin.stl"),
                    &terraincraft_core::registration::pin_mesh(reg, 32)?,
                )
                .map_err(|e| e.to_string())?;

                let triangles: usize = finished.iter().map(|(_, f)| f.len()).sum();
                Ok(format!(
                    "Exported {} tiles ({triangles} triangles) as STL and terrain_tiles.3mf, plus assembly_sheet.svg, to {}{}",
                    ts.len(),
                    self.output,
                    if warnings.is_empty() {
                        String::new()
                    } else {
                        format!(". Warnings: {}", warnings.join("; "))
                    }
                ))
            }
            View::Laser => {
                let (parts, placements) = self.laser_data(dem)?;
                let count = placements.iter().map(|p| p.sheet).max().unwrap_or(0);
                for n in 1..=count {
                    fs::write(
                        Path::new(&self.output).join(format!("laser_sheet_{n:02}.svg")),
                        sheet_svg(&parts, &placements, n, self.sheet_x, self.sheet_y),
                    )
                    .map_err(|e| e.to_string())?;
                }
                let mut manifest =
                    String::from("part_id,level,sheet,x_mm,y_mm,width_mm,height_mm\n");
                for p in &placements {
                    let level = parts
                        .iter()
                        .find(|v| v.id == p.part_id)
                        .map(|v| v.level)
                        .unwrap_or(0.);
                    manifest.push_str(&format!(
                        "{},{},{},{},{},{},{}\n",
                        p.part_id, level, p.sheet, p.x_mm, p.y_mm, p.width_mm, p.height_mm
                    ));
                }
                fs::write(Path::new(&self.output).join("laser_manifest.csv"), manifest)
                    .map_err(|e| e.to_string())?;
                fs::write(
                    Path::new(&self.output).join("laser_layout_report.html"),
                    terraincraft_core::inspection::report_html(
                        &parts,
                        &placements,
                        self.sheet_x,
                        self.sheet_y,
                    ),
                )
                .map_err(|e| e.to_string())?;
                Ok(format!(
                    "Exported {} parts on {} laser sheets",
                    parts.len(),
                    count
                ))
            }
            View::Terrain => {
                let (lo, hi) = dem.min_max();
                for (i, level) in self.levels(dem).iter().enumerate() {
                    fs::write(
                        Path::new(&self.output).join(format!("contour_{:02}.svg", i + 1)),
                        terraincraft_core::contours::svg(dem, self.width, self.height, *level)?,
                    )
                    .map_err(|e| e.to_string())?;
                }
                Ok(format!(
                    "Exported {} contour SVGs (elevation {:.2}–{:.2})",
                    self.layer_count, lo, hi
                ))
            }
        }
    }
}
