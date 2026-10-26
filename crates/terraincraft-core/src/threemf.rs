//! 3MF export: every tile as a separately named object in one file, which slicers such as
//! PrusaSlicer, OrcaSlicer, Bambu Studio and Cura import as individual parts.
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

type Tri = [[f64; 3]; 3];

/// One printable part: triangles in millimeters and where it sits on the build plate.
pub struct Part<'a> {
    pub name: String,
    pub triangles: &'a [Tri],
    pub offset_mm: [f64; 3],
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The 3D model XML: shared vertices per object, so each closed tile stays manifold.
fn model_xml(parts: &[Part]) -> Result<String, String> {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<metadata name="Application">TerrainCraft</metadata>
<resources>
"#,
    );
    for (id, part) in parts.iter().enumerate() {
        let mut index = HashMap::new();
        let mut verts: Vec<[f32; 3]> = Vec::new();
        let mut tris = Vec::with_capacity(part.triangles.len());
        for t in part.triangles {
            let mut ids = [0usize; 3];
            for (k, p) in t.iter().enumerate() {
                if p.iter().any(|v| !v.is_finite()) {
                    return Err(format!("Non-finite vertex in {}", part.name));
                }
                // Merge at the precision written to the file so shared edges stay shared.
                let q = p.map(|v| v as f32);
                ids[k] = *index.entry(q.map(f32::to_bits)).or_insert_with(|| {
                    verts.push(q);
                    verts.len() - 1
                });
            }
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                tris.push(ids);
            }
        }
        writeln!(
            xml,
            r#"<object id="{}" name="{}" type="model"><mesh><vertices>"#,
            id + 1,
            escape(&part.name)
        )
        .unwrap();
        for v in &verts {
            writeln!(xml, r#"<vertex x="{}" y="{}" z="{}"/>"#, v[0], v[1], v[2]).unwrap();
        }
        xml.push_str("</vertices><triangles>\n");
        for t in &tris {
            writeln!(
                xml,
                r#"<triangle v1="{}" v2="{}" v3="{}"/>"#,
                t[0], t[1], t[2]
            )
            .unwrap();
        }
        xml.push_str("</triangles></mesh></object>\n");
    }
    xml.push_str("</resources>\n<build>\n");
    for (id, part) in parts.iter().enumerate() {
        let [x, y, z] = part.offset_mm;
        writeln!(
            xml,
            r#"<item objectid="{}" transform="1 0 0 0 1 0 0 0 1 {x} {y} {z}"/>"#,
            id + 1
        )
        .unwrap();
    }
    xml.push_str("</build>\n</model>\n");
    Ok(xml)
}

/// Write a 3MF package containing `parts`.
pub fn write(parts: &[Part]) -> Result<Vec<u8>, String> {
    if parts.is_empty() {
        return Err("Nothing to write to 3MF".into());
    }
    let model = model_xml(parts)?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", RELS),
        ("3D/3dmodel.model", model.as_str()),
    ] {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn tetra() -> Vec<Tri> {
        let (a, b, c, d) = ([0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]);
        vec![[a, c, b], [a, b, d], [b, c, d], [c, a, d]]
    }

    #[test]
    fn writes_shared_vertices_and_named_objects() {
        let t = tetra();
        let bytes = write(&[
            Part {
                name: "Tile 1-1".into(),
                triangles: &t,
                offset_mm: [0., 0., 0.],
            },
            Part {
                name: "Tile <1-2>".into(),
                triangles: &t,
                offset_mm: [10., 0., 0.],
            },
        ])
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        for name in ["[Content_Types].xml", "_rels/.rels"] {
            assert!(archive.by_name(name).is_ok(), "{name}");
        }
        let mut model = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut model)
            .unwrap();
        // Four shared vertices and four triangles per tetrahedron.
        assert_eq!(model.matches("<vertex ").count(), 8);
        assert_eq!(model.matches("<triangle ").count(), 8);
        assert!(model.contains(r#"name="Tile &lt;1-2&gt;""#));
        assert!(model.contains("transform=\"1 0 0 0 1 0 0 0 1 10 0 0\""));
        assert!(write(&[]).is_err());
    }
}
