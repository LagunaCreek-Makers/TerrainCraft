//! OpenGL hardware-accelerated terrain preview using egui_glow paint callbacks.
//! Unlike the legacy painter's algorithm, the GPU depth buffer resolves occlusion.
use eframe::{egui, glow};
use glow::HasContext;
use std::sync::Arc;

const VERT: &str = r#"#version 330 core
layout(location=0) in vec3 a_pos;
layout(location=1) in vec3 a_normal;
layout(location=2) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_normal;
out vec3 v_color;
void main(){gl_Position=u_mvp*vec4(a_pos,1.0);v_normal=a_normal;v_color=a_color;}
"#;
const FRAG: &str = r#"#version 330 core
in vec3 v_normal;
in vec3 v_color;
out vec4 out_color;
void main(){vec3 light=normalize(vec3(0.4,-0.5,0.8));float d=max(dot(normalize(v_normal),light),0.0);out_color=vec4(v_color*(0.28+0.72*d),1.0);}
"#;

pub struct GpuMesh {
    pub vertices: Vec<f32>,
    pub triangle_count: usize,
}
impl GpuMesh {
    /// Each triangle is [x,y,z] in millimeters; per-tile translation is applied before upload.
    pub fn from_tiles(tiles: &[(Vec<[[f64; 3]; 3]>, [f64; 2], [f32; 3])]) -> Self {
        let mut vertices = Vec::new();
        for (faces, offset, color) in tiles {
            for face in faces {
                let a = face[0];
                let b = face[1];
                let c = face[2];
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if l <= 1e-12 {
                    continue;
                }
                for p in face {
                    vertices.extend_from_slice(&[
                        (p[0] + offset[0]) as f32,
                        (p[1] + offset[1]) as f32,
                        p[2] as f32,
                        (n[0] / l) as f32,
                        (n[1] / l) as f32,
                        (n[2] / l) as f32,
                        color[0],
                        color[1],
                        color[2],
                    ]);
                }
            }
        }
        let triangle_count = vertices.len() / 27;
        Self {
            vertices,
            triangle_count,
        }
    }
}

/// GPU view draws into an isolated color+depth framebuffer, then composites the result
/// into the egui callback's viewport. No CPU depth sorting or triangle decimation.
pub fn paint_rect(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    mesh: GpuMesh,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    center: [f32; 3],
    extent: f32,
) {
    let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
        let gl = painter.gl();
        // Render in a temporary offscreen framebuffer, ensuring a real depth attachment.
        // This keeps egui's own framebuffer and depth/stencil state independent.
        unsafe {
            if let Err(err) = render(
                gl,
                &mesh,
                info.viewport_in_pixels().width_px as i32,
                info.viewport_in_pixels().height_px as i32,
                info.viewport_in_pixels().left_px as i32,
                info.viewport_in_pixels().from_bottom_px as i32,
                yaw,
                pitch,
                zoom,
                center,
                extent,
            ) {
                eprintln!("TerrainCraft GPU preview: {err}");
            }
        }
    });
    ui.painter().add(egui::PaintCallback {
        rect,
        callback: Arc::new(callback),
    });
}

unsafe fn compile(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> Result<glow::NativeShader, String> {
    let shader = gl.create_shader(kind)?;
    gl.shader_source(shader, source);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let err = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        return Err(err);
    }
    Ok(shader)
}
unsafe fn render(
    gl: &glow::Context,
    mesh: &GpuMesh,
    w: i32,
    h: i32,
    left: i32,
    bottom: i32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    center: [f32; 3],
    extent: f32,
) -> Result<(), String> {
    if w <= 0 || h <= 0 || mesh.vertices.is_empty() {
        return Ok(());
    }
    let vs = compile(gl, glow::VERTEX_SHADER, VERT)?;
    let fs = compile(gl, glow::FRAGMENT_SHADER, FRAG)?;
    let program = gl.create_program()?;
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    gl.delete_shader(vs);
    gl.delete_shader(fs);
    if !gl.get_program_link_status(program) {
        let e = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(e);
    }
    let vao = gl.create_vertex_array()?;
    let vbo = gl.create_buffer()?;
    gl.bind_vertex_array(Some(vao));
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
    let bytes =
        std::slice::from_raw_parts(mesh.vertices.as_ptr() as *const u8, mesh.vertices.len() * 4);
    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
    for (idx, offset) in [(0, 0), (1, 12), (2, 24)] {
        gl.enable_vertex_attrib_array(idx);
        gl.vertex_attrib_pointer_f32(idx, 3, glow::FLOAT, false, 36, offset);
    }
    let color = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(color));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w,
        h,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        None,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::LINEAR as i32,
    );
    let depth = gl.create_renderbuffer()?;
    gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
    gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, w, h);
    let fbo = gl.create_framebuffer()?;
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(color),
        0,
    );
    gl.framebuffer_renderbuffer(
        glow::FRAMEBUFFER,
        glow::DEPTH_ATTACHMENT,
        glow::RENDERBUFFER,
        Some(depth),
    );
    if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
        return Err("GPU framebuffer incomplete".into());
    }
    gl.viewport(0, 0, w, h);
    gl.enable(glow::DEPTH_TEST);
    gl.depth_func(glow::LESS);
    gl.clear_color(0.09, 0.12, 0.16, 1.0);
    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    gl.use_program(Some(program));
    let cy = yaw.cos();
    let sy = yaw.sin();
    let cp = pitch.cos();
    let sp = pitch.sin();
    let scale = 2.0 * zoom / (extent.max(1.0) * 1.7);
    let aspect = w as f32 / h as f32;
    // Column-major orthographic model-view-projection matrix.
    let m = [
        cy * scale / aspect,
        sy * cp * scale,
        sy * sp * scale,
        0.,
        -sy * scale / aspect,
        cy * cp * scale,
        cy * sp * scale,
        0.,
        0.,
        -sp * scale,
        cp * scale,
        0.,
        0.,
        0.,
        0.,
        1.,
    ];
    let translation = [
        -(m[0] * center[0] + m[4] * center[1] + m[8] * center[2]),
        -(m[1] * center[0] + m[5] * center[1] + m[9] * center[2]),
        -(m[2] * center[0] + m[6] * center[1] + m[10] * center[2]),
    ];
    let mut m = m;
    m[12] = translation[0];
    m[13] = translation[1];
    m[14] = translation[2];
    gl.uniform_matrix_4_f32_slice(
        gl.get_uniform_location(program, "u_mvp").as_ref(),
        false,
        &m,
    );
    gl.draw_arrays(glow::TRIANGLES, 0, (mesh.vertices.len() / 9) as i32);
    // Blit the shaded color into the callback's active framebuffer, preserving egui composition.
    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));
    gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
    gl.blit_framebuffer(
        0,
        0,
        w,
        h,
        left,
        bottom,
        left + w,
        bottom + h,
        glow::COLOR_BUFFER_BIT,
        glow::LINEAR,
    );
    gl.disable(glow::DEPTH_TEST);
    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    gl.bind_vertex_array(None);
    gl.bind_buffer(glow::ARRAY_BUFFER, None);
    gl.use_program(None);
    gl.delete_framebuffer(fbo);
    gl.delete_renderbuffer(depth);
    gl.delete_texture(color);
    gl.delete_buffer(vbo);
    gl.delete_vertex_array(vao);
    gl.delete_program(program);
    Ok(())
}
