use std::{env,fs,path::PathBuf};
use terraincraft_core::laser::{cut_parts,layout,sheet_svg,split_oversized};
use terraincraft_core::gis::read_asc;
use terraincraft_core::{Dem,TerrainSettings,tiles,mesh};
fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<String>=env::args().collect();
    if args.len()!=4 {return Err("Usage: terraincraft-cli <heightmap.csv|grid.asc|dem.tif> <output_dir> <bed_mm>\nCSV rows must contain numeric elevations; no header. Demo input: examples/demo.csv".into());}
    let is_tif=args[1].to_ascii_lowercase().ends_with(".tif") || args[1].to_ascii_lowercase().ends_with(".tiff");
    let input=if is_tif {String::new()} else {fs::read_to_string(&args[1])?};
    let is_asc=args[1].to_ascii_lowercase().ends_with(".asc");
    let rows:Vec<Vec<f64>>=if is_asc || is_tif {Vec::new()} else {input.lines().filter(|s|!s.trim().is_empty()).map(|line|line.split(',').map(|v|v.trim().parse::<f64>()).collect::<Result<Vec<_>,_>>()).collect::<Result<Vec<_>,_>>()?};
    let dem=if is_tif {terraincraft_core::geotiff::decode(&fs::read(&args[1])?)?} else if is_asc {read_asc(&input)?.dem} else {
        let h=rows.len();let w=rows.first().ok_or("Empty CSV")?.len();
        if rows.iter().any(|r|r.len()!=w){return Err("CSV must be rectangular".into());}
        Dem::new(w,h,rows.into_iter().flatten().collect())?
    };
    let output=PathBuf::from(&args[2]);fs::create_dir_all(&output)?;
    let bed:f64=args[3].parse()?;
    let s=TerrainSettings{width_mm:400.,height_mm:300.,base_mm:std::env::var("TERRAINCRAFT_BASE_MM").ok().and_then(|v|v.parse::<f64>().ok()).unwrap_or(3.),vertical_exaggeration:2.,relief_mm:25.};
    let tile_list=tiles(&dem,s,bed,bed,5.)?;

    fs::write(output.join("print_tile_manifest.csv"),terraincraft_core::print_export::tile_manifest(&dem,s,&tile_list)?)?;
    let reg=terraincraft_core::registration::RegistrationSettings::default();
    let guides=terraincraft_core::registration::guides(&dem,s,&tile_list,reg)?;
    let integrated=std::env::var("TERRAINCRAFT_INTEGRATED_SOCKETS").ok().as_deref()==Some("1");
    let seam_integrated=std::env::var("TERRAINCRAFT_SEAM_SOCKETS").ok().as_deref()==Some("1");
    let seam_holes=if seam_integrated {Some(terraincraft_core::seam_joints::plan(&dem,&tile_list,s,terraincraft_core::seam_joints::SeamSettings::default())?)}else{None};
    if seam_integrated && integrated {return Err("Combining underside and sidewall sockets is not yet supported".into())}
    if let Some(ref holes)=seam_holes {
        let checks=terraincraft_core::seam_preflight::inspect(&dem,s,&tile_list,holes)?;
        fs::write(output.join("seam_socket_preflight.csv"),terraincraft_core::seam_preflight::csv(&checks))?;
        fs::write(output.join("seam_socket_preview.svg"),terraincraft_core::seam_preflight::assembly_preview(&dem,s,&tile_list,&checks)?)?;
        let failed=checks.iter().filter(|c|!c.compatible).count();
        if failed>0 {return Err(format!("{failed} sidewall sockets are incompatible with the current DEM mesh; see seam_socket_preflight.csv and seam_socket_preview.svg. Use seam drilling guides or a coarser DEM.").into());}
    }
    let repair_winding=std::env::var("TERRAINCRAFT_REPAIR_WINDING").ok().as_deref()==Some("1");
    let mut repair_csv=String::from("filename,status,flipped_triangles,details\n");
    let strict_mesh=std::env::var("TERRAINCRAFT_STRICT_MESH").ok().as_deref()==Some("1");
    let mut audit_csv=String::from(terraincraft_core::mesh_audit::CSV_HEADER);
    let mut invalid_tiles=Vec::new();
    let mut manufacturing_csv=String::from(terraincraft_core::manufacturing::CSV_HEADER);
    let check_intersections=std::env::var("TERRAINCRAFT_CHECK_INTERSECTIONS").ok().as_deref()==Some("1");
    let mut intersection_csv=String::from("filename,tested_pairs,crossings,coplanar_overlaps,skipped_shared_vertex_pairs,broadphase_candidates,complete\n");
    let mut assembly_meshes: Vec<(terraincraft_core::Tile, Vec<[[f64;3];3]>)> = Vec::new();
    for (tile,guide) in tile_list.iter().zip(&guides) {let path=output.join(format!("tile_r{:02}_c{:02}.stl",tile.row+1,tile.col+1));
      let mut faces=if let Some(ref all)=seam_holes {let selected:Vec<_>=all.iter().filter(|h|h.tile_row==tile.row&&h.tile_col==tile.col).cloned().collect();terraincraft_core::seam_socket_mesh::mesh_with_seam_sockets(&dem,s,*tile,&selected,32)?}else if integrated {terraincraft_core::socket_mesh::mesh_with_sockets(&dem,s,*tile,guide,reg,32)?}else{mesh(&dem,s,*tile)?};
      if repair_winding {
          let filename=path.file_name().unwrap().to_string_lossy();
          match terraincraft_core::mesh_repair::repair_winding(&mut faces) {
              Ok(report)=>repair_csv.push_str(&format!("{},repaired,{},ok\n",filename,report.flipped_triangles)),
              Err(reason)=>{
                  repair_csv.push_str(&format!("{},refused,0,{}\n",filename,reason.replace(',',";")));
                  // Preserve original mesh for inspection; strict mode will still reject invalid topology.
              }
          }
      }
      let audit=terraincraft_core::mesh_audit::audit(&faces);
      let filename=path.file_name().unwrap().to_string_lossy();
      audit_csv.push_str(&audit.csv_row(&filename));
      let manufacturing=terraincraft_core::manufacturing::inspect(&faces);
      manufacturing_csv.push_str(&manufacturing.csv_row(&filename));
      let mut valid=manufacturing.is_ready_for_review();
      if check_intersections {
          let report=terraincraft_core::intersections::inspect(&faces,250_000);
          intersection_csv.push_str(&format!("{},{},{},{},{},{},{}\n",filename,report.tested_pairs,report.crossings,report.coplanar_overlaps,report.skipped_adjacent_pairs,report.broadphase_candidates,report.complete));
          if report.crossings>0 || !report.complete {valid=false;}
      }
      if !valid {invalid_tiles.push(filename.to_string());}
      // Defer ALL STL writes until every tile has passed strict preflight.
      // A rejected job must not leave a partial collection of printable STLs.
      assembly_meshes.push((*tile,faces));
    }
    // Diagnostic reports are written even when strict preflight fails.
    fs::write(output.join("mesh_audit.csv"),&audit_csv)?;
    fs::write(output.join("manufacturing_report.csv"),&manufacturing_csv)?;
    if check_intersections {fs::write(output.join("intersection_report.csv"),&intersection_csv)?;}
    if repair_winding {fs::write(output.join("mesh_repair.csv"),&repair_csv)?;}
    if strict_mesh && !invalid_tiles.is_empty(){
        return Err(format!("Manufacturing preflight failed for {} tile(s): {}. No terrain STL files were written; inspect mesh_audit.csv and manufacturing_report.csv",invalid_tiles.len(),invalid_tiles.join(", ")).into());
    }
    // All geometry has passed the requested gate; now commit printable meshes.
    for (tile,faces) in &assembly_meshes {
        let path=output.join(format!("tile_r{:02}_c{:02}.stl",tile.row+1,tile.col+1));
        terraincraft_core::print_export::write_binary_stl(&path,faces)?;
    }
    let assembly_refs: Vec<_> = assembly_meshes.iter().map(|(tile, triangles)|
        terraincraft_core::assembly_obj::AssemblyPart { tile: *tile, triangles }).collect();
    let (assembly_obj, assembly_mtl) = terraincraft_core::assembly_obj::export(&dem,s,&assembly_refs)?;
    fs::write(output.join("terrain_assembly.obj"), assembly_obj)?;
    fs::write(output.join("terrain_assembly.mtl"), assembly_mtl)?;
    for g in &guides {fs::write(output.join(format!("registration_r{:02}_c{:02}.svg",g.row+1,g.col+1)),terraincraft_core::registration::guide_svg(g,reg)?)?;}
    fs::write(output.join("registration_manifest.csv"),terraincraft_core::registration::guide_manifest(&guides,reg))?;
    terraincraft_core::print_export::write_binary_stl(&output.join("registration_pin.stl"),&terraincraft_core::registration::pin_mesh(reg,32)?)?;

    if std::env::var("TERRAINCRAFT_SEAM_GUIDES").ok().as_deref()==Some("1") || std::env::var("TERRAINCRAFT_SEAM_DOWELS").ok().as_deref()==Some("1") {
        let cfg=terraincraft_core::seam_joints::SeamSettings::default();
        let holes=terraincraft_core::seam_joints::plan(&dem,&tile_list,s,cfg)?;
        fs::write(output.join("seam_dowel_manifest.csv"),terraincraft_core::seam_joints::manifest(&holes))?;
        terraincraft_core::seam_fabrication::preflight(&holes,cfg)?;
        fs::write(output.join("seam_assembly.svg"),terraincraft_core::seam_fabrication::assembly_svg(&holes))?;
        terraincraft_core::print_export::write_binary_stl(&output.join("seam_dowel.stl"),&terraincraft_core::seam_fabrication::dowel_mesh(cfg,48)?)?;
        let sx=s.width_mm/(dem.width-1) as f64;
        let sy=s.height_mm/(dem.height-1) as f64;
        for tile in &tile_list {for side in ["east","west","north","south"] {
            let selected:Vec<_>=holes.iter().filter(|h|h.tile_row==tile.row&&h.tile_col==tile.col&&h.side==side).collect();
            if selected.is_empty(){continue}
            let length=if side=="east"||side=="west" {(tile.y1-tile.y0) as f64*sy}else{(tile.x1-tile.x0) as f64*sx};
            let svg=terraincraft_core::seam_joints::wall_svg(length,s.base_mm,&selected)?;
            fs::write(output.join(format!("seam_r{:02}_c{:02}_{}.svg",tile.row+1,tile.col+1,side)),svg)?;
        }}
    }
    let (lo,hi)=dem.min_max();
    for i in 1..=8 {let level=lo+(hi-lo)*i as f64/9.;fs::write(output.join(format!("contour_{i:02}.svg")),terraincraft_core::contours::svg(&dem,s.width_mm,s.height_mm,level)?)?;}
    let levels=(1..=8).map(|i|lo+(hi-lo)*i as f64/9.).collect::<Vec<_>>();
    let parts=cut_parts(&dem,s.width_mm,s.height_mm,&levels)?;
    let parts=split_oversized(&parts,594.,394.)?;
    let placements=layout(&parts,600.,400.,3.)?;
    terraincraft_core::inspection::preflight(&parts,&placements,600.,400.,3.)?;
    fs::write(output.join("laser_layout_report.html"),terraincraft_core::inspection::report_html(&parts,&placements,600.,400.))?;
    let sheets=placements.iter().map(|p|p.sheet).max().unwrap_or(0);
    for sheet in 1..=sheets {fs::write(output.join(format!("laser_sheet_{sheet:02}.svg")),sheet_svg(&parts,&placements,sheet,600.,400.))?;}
    let mut manifest=String::from("part_id,level,sheet,x_mm,y_mm,width_mm,height_mm\n");
    for p in &placements {let level=parts.iter().find(|v|v.id==p.part_id).unwrap().level;manifest.push_str(&format!("{},{},{},{},{},{},{}\n",p.part_id,level,p.sheet,p.x_mm,p.y_mm,p.width_mm,p.height_mm));}
    fs::write(output.join("laser_manifest.csv"),manifest)?;
    println!("Generated {} closed contour parts across {} laser sheets",parts.len(),sheets);
    println!("Generated {} STL tiles and 8 SVG contour engravings in {}",tile_list.len(),output.display());Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("Error: {e}");std::process::exit(1)}}
