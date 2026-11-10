//! Blurs part of Starlite's own frame while it's being drawn: everything
//! painted so far inside a rect is copied to a texture and drawn back
//! blurred, with soft edges. Used behind the CD shelf's pulled-out case.
//! Only Starlite's own window contents are read, never the screen.

use std::sync::{Arc, Mutex};

use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use egui::Rect;

const VS: &str = r#"#version 330 core
const vec2 V[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
void main() { gl_Position = vec4(V[gl_VertexID], 0.0, 1.0); }
"#;

const FS: &str = r#"#version 330 core
out vec4 o;
uniform sampler2D u_src;
uniform vec2  u_origin;   // rect's bottom-left in framebuffer px
uniform vec2  u_size;     // rect size px
uniform float u_amount;   // 0..1
uniform float u_feather;  // soft edge, px
uniform vec4  u_tint;     // rgb + strength at full blur

void main() {
    vec2 q = gl_FragCoord.xy - u_origin;
    vec2 uv = q / u_size;
    float lod = u_amount * 3.4;
    vec2 px = exp2(lod) / u_size;
    vec2 lo = 0.5 / u_size, hi = 1.0 - lo;
    // A disc of samples on a golden-angle spiral, gaussian weighted.
    vec3 acc = textureLod(u_src, uv, lod).rgb;
    float wsum = 1.0;
    for (int i = 0; i < 24; i++) {
        float r = sqrt((float(i) + 0.5) / 24.0) * 2.6;
        float a = float(i) * 2.39996;
        float w = exp(-r * r * 0.35);
        acc += textureLod(u_src, clamp(uv + vec2(cos(a), sin(a)) * r * px, lo, hi), lod).rgb * w;
        wsum += w;
    }
    vec3 blurred = mix(acc / wsum, u_tint.rgb, u_tint.a * u_amount);
    vec3 sharp = textureLod(u_src, uv, 0.0).rgb;
    vec2 e = min(q, u_size - q);
    float edge = clamp(min(e.x, e.y) / max(u_feather, 1.0), 0.0, 1.0);
    edge = edge * edge * (3.0 - 2.0 * edge);
    o = vec4(mix(sharp, blurred, clamp(u_amount * 1.6, 0.0, 1.0) * edge), 1.0);
}
"#;

pub struct BlurRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    tex: glow::Texture,
    size: (i32, i32),
}

impl BlurRenderer {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let compile = |kind, src: &str| {
                let s = gl.create_shader(kind).expect("create shader");
                gl.shader_source(s, src);
                gl.compile_shader(s);
                assert!(gl.get_shader_compile_status(s), "blur shader: {}", gl.get_shader_info_log(s));
                s
            };
            let program = gl.create_program().expect("create program");
            let (vs, fs) = (compile(glow::VERTEX_SHADER, VS), compile(glow::FRAGMENT_SHADER, FS));
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            assert!(gl.get_program_link_status(program), "blur link: {}", gl.get_program_info_log(program));
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            let vao = gl.create_vertex_array().expect("vao");
            let tex = gl.create_texture().expect("texture");
            Self { program, vao, tex, size: (0, 0) }
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_texture(self.tex);
        }
    }

    /// `x, y` (bottom-left), `w, h`: the part of the framebuffer to blur, px.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, gl: &glow::Context, x: i32, y: i32, w: i32, h: i32, amount: f32, feather: f32, tint: [f32; 4]) {
        if w <= 0 || h <= 0 || amount <= 0.003 {
            return;
        }
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            if self.size != (w, h) {
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                self.size = (w, h);
            }
            gl.copy_tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, x, y, w, h);
            gl.generate_mipmap(glow::TEXTURE_2D);

            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.viewport(x, y, w, h);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            let p = self.program;
            let loc = |n: &str| gl.get_uniform_location(p, n);
            gl.uniform_1_i32(loc("u_src").as_ref(), 0);
            gl.uniform_2_f32(loc("u_origin").as_ref(), x as f32, y as f32);
            gl.uniform_2_f32(loc("u_size").as_ref(), w as f32, h as f32);
            gl.uniform_1_f32(loc("u_amount").as_ref(), amount);
            gl.uniform_1_f32(loc("u_feather").as_ref(), feather);
            gl.uniform_4_f32(loc("u_tint").as_ref(), tint[0], tint[1], tint[2], tint[3]);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
            gl.enable(glow::BLEND);
        }
    }
}

/// A paint callback that blurs what's already drawn in `rect` (points).
pub fn callback(r: Arc<Mutex<Option<BlurRenderer>>>, rect: Rect, amount: f32, feather_pt: f32, tint: [f32; 4]) -> egui::PaintCallback {
    egui::PaintCallback {
        rect,
        callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
            let gl = painter.gl();
            let v = info.viewport_in_pixels();
            let [sw, sh] = info.screen_size_px.map(|s| s as i32);
            // Keep to the framebuffer.
            let (x0, y0) = (v.left_px.max(0), v.from_bottom_px.max(0));
            let (x1, y1) = ((v.left_px + v.width_px).min(sw), (v.from_bottom_px + v.height_px).min(sh));
            let mut lock = r.lock().unwrap();
            let b = lock.get_or_insert_with(|| BlurRenderer::new(gl));
            b.paint(gl, x0, y0, x1 - x0, y1 - y0, amount, feather_pt * info.pixels_per_point, tint);
        })),
    }
}
