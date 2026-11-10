//! CD shelf (Settings → Appearance → CD shelf): a playlist's songs stand as
//! CD cases on a bookshelf, spine out. Hover one and it's pulled out: it
//! turns to face you and glides to the middle of the page, big, while the
//! page behind blurs. The disc slides out of the case, a preview of the song
//! plays (see `preview.rs`) with an equalizer beside it, and the case tilts
//! with the mouse. Click a case (or a spine) to play the song.
//!
//! The case is a real little 3D model (box faces, the disc) projected with
//! perspective and drawn as egui meshes; every spine on the shelf is that
//! same model turned sideways, so pulling one out is one continuous turn.

use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use egui::epaint::{Mesh, Shadow, TextShape, Vertex, WHITE_UV};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, CursorIcon, Id, Painter, Pos2, Rect, Sense, TextureId, Ui, Vec2};
use serde::{Deserialize, Serialize};

use crate::app::Starlite;
use crate::blur::BlurRenderer;
use crate::library::{Source, Track};
use crate::nowplaying::smooth_damp;
use crate::preview::{band_db, bar_heights, Preview, BANDS};
use crate::widgets::{cover_texture, font, text_trunc};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct ShelfPrefs {
    pub enabled: bool,
}

impl ShelfPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("shelf.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let f = Self::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, s);
        }
    }
}

const SPINE_W: f32 = 24.0;
const SPINE_H: f32 = 196.0;
const SPINE_GAP: f32 = 3.0;
/// Room under a row for the shelf board and some air.
const ROW_GAP: f32 = 44.0;
/// Rows of spines per page.
const ROWS: usize = 2;
/// Width of the page numbers column.
const SELECTOR_W: f32 = 64.0;
/// The case in units of its height. A jewel case is 142 × 125 × 10 mm;
/// this one is a little thicker so its spine is readable.
const HW: f32 = 0.568;
const HH: f32 = 0.5;
const HD: f32 = SPINE_W / SPINE_H * 0.5;
const DISC_R: f32 = 0.47;
/// How far the disc's centre slides out (units).
const DISC_SLIDE: f32 = 0.66;
/// Distance from the eye to the screen, px (smaller = stronger perspective).
const CAM: f32 = 2300.0;
/// Pull-out / put-back times, s.
const PULL: f32 = 0.55;
const PUT_BACK: f32 = 0.42;

struct Pulled {
    track: Track,
    idx: usize,
    /// 0 on the shelf … 1 out.
    t: f32,
    leaving: bool,
    disc: f32,
    disc_v: f32,
    spin: f32,
}

pub struct Shelf {
    pub prefs: ShelfPrefs,
    key: String,
    page: usize,
    /// Shown page (eased towards `page`), and its speed.
    page_pos: f32,
    page_v: f32,
    wheel: f32,
    wheel_cool: f32,
    /// First page number shown when there are more than fit.
    sel_off: f32,
    debug_turned: bool,
    out: Vec<Pulled>,
    intent: Option<(usize, f32)>,
    away: f32,
    tilt: [f32; 2],
    tilt_v: [f32; 2],
    case_rect: Option<Rect>,
    colors: HashMap<String, Color32>,
    pending: HashSet<String>,
    tx: Sender<(String, Option<Color32>)>,
    rx: Receiver<(String, Option<Color32>)>,
    inflight: Arc<AtomicUsize>,
    eq: [f32; BANDS],
    eq_a: f32,
    eq_peak: f32,
    /// After a click: no hovering until the pointer moves away from here.
    hold: Option<Pos2>,
    sent_volume: f32,
    blur: f32,
    pub blur_gl: Arc<Mutex<Option<BlurRenderer>>>,
    pub preview: Preview,
    /// How loud the main player is while a preview plays (eased).
    pub duck: f32,
    pub duck_sent: f32,
    /// Set while the shelf was drawn this frame.
    pub drawn: bool,
}

impl Shelf {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Shelf {
            prefs: ShelfPrefs::load(),
            key: String::new(),
            page: 0,
            page_pos: 0.0,
            page_v: 0.0,
            wheel: 0.0,
            wheel_cool: 0.0,
            sel_off: 0.0,
            debug_turned: false,
            out: Vec::new(),
            intent: None,
            away: 0.0,
            tilt: [0.0; 2],
            tilt_v: [0.0; 2],
            case_rect: None,
            colors: HashMap::new(),
            pending: HashSet::new(),
            tx,
            rx,
            inflight: Arc::new(AtomicUsize::new(0)),
            eq: [0.0; BANDS],
            eq_a: 0.0,
            eq_peak: -60.0,
            hold: None,
            sent_volume: -1.0,
            blur: 0.0,
            blur_gl: Arc::new(Mutex::new(None)),
            preview: Preview::new(),
            duck: 1.0,
            duck_sent: 1.0,
            drawn: false,
        }
    }

    /// Not on screen: put everything back, stop the preview.
    pub fn reset(&mut self) {
        self.out.clear();
        self.intent = None;
        self.case_rect = None;
        self.blur = 0.0;
        self.eq_a = 0.0;
        self.preview.stop();
    }

    /// A song was picked to play: its case goes back to the shelf.
    pub fn played(&mut self, pointer: Option<Pos2>) {
        for p in &mut self.out {
            p.leaving = true;
        }
        self.hold = pointer;
        self.preview.stop();
    }

    /// Spine colours from the covers, worked out a few at a time.
    fn want_color(&mut self, ctx: &egui::Context, uri: &str) {
        if self.colors.contains_key(uri) || self.pending.contains(uri) || self.inflight.load(Ordering::Relaxed) >= 4 {
            return;
        }
        self.pending.insert(uri.to_string());
        self.inflight.fetch_add(1, Ordering::Relaxed);
        let (u, tx, n, c) = (uri.to_string(), self.tx.clone(), self.inflight.clone(), ctx.clone());
        std::thread::spawn(move || {
            let col = crate::nowplaying::cover_bytes(&u).and_then(|b| crate::nowplaying::palette_from(&b)).map(|p| {
                let avg = |k: usize| (p.iter().map(|q| q[k]).sum::<f32>() / 4.0 * 0.85 * 255.0) as u8;
                Color32::from_rgb(avg(0), avg(1), avg(2))
            });
            let _ = tx.send((u, col));
            n.fetch_sub(1, Ordering::Relaxed);
            c.request_repaint();
        });
    }

    fn collect_colors(&mut self) {
        while let Ok((u, c)) = self.rx.try_recv() {
            self.pending.remove(&u);
            self.colors.insert(u, c.unwrap_or(Color32::from_rgb(70, 72, 82)));
        }
    }
}

// ── The 3D case ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub center: Pos2,
    /// Pixels per unit (the case is one unit tall).
    pub scale: f32,
    pub yaw: f32,
    pub pitch: f32,
}

type V3 = [f32; 3];

struct Proj {
    pose: Pose,
    sy: f32,
    cy: f32,
    sp: f32,
    cp: f32,
}

impl Proj {
    fn new(pose: Pose) -> Self {
        let (sy, cy) = pose.yaw.sin_cos();
        let (sp, cp) = pose.pitch.sin_cos();
        Proj { pose, sy, cy, sp, cp }
    }
    /// Model → view space (x right, y up, z away from the eye).
    fn rot(&self, v: V3) -> V3 {
        let x = v[0] * self.cy + v[2] * self.sy;
        let z = -v[0] * self.sy + v[2] * self.cy;
        let y = v[1];
        [x, y * self.cp - z * self.sp, y * self.sp + z * self.cp]
    }
    fn pt(&self, v: V3) -> Pos2 {
        let r = self.rot(v);
        let s = self.pose.scale;
        let k = CAM / (CAM + r[2] * s).max(50.0);
        self.pose.center + vec2(r[0] * s * k, -r[1] * s * k)
    }
    /// How squarely a face looks at the eye (cosine; ≤ 0: facing away).
    fn facing(&self, n: V3, c: V3) -> f32 {
        let n = self.rot(n);
        let c = self.rot(c);
        let s = self.pose.scale;
        let v = [c[0] * s, c[1] * s, c[2] * s + CAM];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
        -(n[0] * v[0] + n[1] * v[1] + n[2] * v[2]) / len
    }
    /// Light from the top left, a little in front: 0.62 … 1.
    fn shade(&self, n: V3) -> f32 {
        let n = self.rot(n);
        let l = [-0.35, 0.55, -0.76];
        0.62 + 0.38 * (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0)
    }
}

fn lerp3(a: V3, b: V3, t: f32) -> V3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Corners top-left, top-right, bottom-right, bottom-left.
type Quad = [V3; 4];

fn at(q: &Quad, u: f32, v: f32) -> V3 {
    lerp3(lerp3(q[0], q[1], u), lerp3(q[3], q[2], u), v)
}

fn sub(q: &Quad, u0: f32, v0: f32, u1: f32, v1: f32) -> Quad {
    [at(q, u0, v0), at(q, u1, v0), at(q, u1, v1), at(q, u0, v1)]
}

/// A face as an n × m grid (fine enough that textures don't warp).
fn quad(mesh: &mut Mesh, pr: &Proj, q: &Quad, uv: Rect, n: usize, m: usize, color: impl Fn(f32, f32) -> Color32) {
    let base = mesh.vertices.len() as u32;
    for j in 0..=m {
        for i in 0..=n {
            let (u, v) = (i as f32 / n as f32, j as f32 / m as f32);
            mesh.vertices.push(Vertex { pos: pr.pt(at(q, u, v)), uv: pos2(uv.min.x + uv.width() * u, uv.min.y + uv.height() * v), color: color(u, v) });
        }
    }
    let w = n as u32 + 1;
    for j in 0..m as u32 {
        for i in 0..n as u32 {
            let a = base + j * w + i;
            mesh.indices.extend_from_slice(&[a, a + 1, a + w, a + 1, a + w + 1, a + w]);
        }
    }
}

fn flat() -> Rect {
    Rect::from_min_max(WHITE_UV, WHITE_UV)
}

fn shaded(c: Color32, k: f32, alpha: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied((r as f32 * k).min(255.0) as u8, (g as f32 * k).min(255.0) as u8, (b as f32 * k).min(255.0) as u8, (a as f32 * alpha) as u8)
}

/// What a case looks like.
pub struct Look<'a> {
    pub cover: Option<(TextureId, Rect)>,
    pub spine: Color32,
    pub label: &'a str,
}

/// Is the spine's colour light (dark text on it)?
fn light_color(c: Color32) -> bool {
    0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32 > 150.0
}

/// Draws the case (and its disc, `disc` 0 = inside … 1 = out). Returns the
/// screen rect it covers.
pub fn draw_case(p: &Painter, pose: Pose, look: &Look, disc: f32, spin: f32, alpha: f32) -> Rect {
    let pr = Proj::new(pose);
    let s = pose.scale;
    let corners: [V3; 8] = [[-HW, HH, -HD], [HW, HH, -HD], [HW, -HH, -HD], [-HW, -HH, -HD], [-HW, HH, HD], [HW, HH, HD], [HW, -HH, HD], [-HW, -HH, HD]];
    let mut bounds = Rect::NOTHING;
    for c in corners {
        bounds.extend_with(pr.pt(c));
    }
    if alpha <= 0.0 {
        return bounds;
    }

    let front: Quad = [corners[0], corners[1], corners[2], corners[3]];
    let spine: Quad = [corners[4], corners[0], corners[3], corners[7]];
    let right: Quad = [corners[1], corners[5], corners[6], corners[2]];
    let top: Quad = [corners[4], corners[5], corners[1], corners[0]];
    let bottom: Quad = [corners[3], corners[2], corners[6], corners[7]];

    // Shadow under the case, softer the further out it is.
    if s > SPINE_H * 1.15 {
        let k = ((s - SPINE_H * 1.15) / (s * 0.5)).clamp(0.0, 1.0);
        let r = bounds.shrink2(vec2(bounds.width() * 0.06, bounds.height() * 0.04)).translate(vec2(0.0, s * 0.05));
        p.add(Shadow { offset: [0, 22], blur: 60, spread: 0, color: Color32::from_black_alpha((120.0 * k * alpha) as u8) }.as_shape(r, egui::CornerRadius::same(10)));
    }

    let mut mesh = Mesh::default();
    let edge_col = Color32::from_rgb(214, 218, 226);
    let face = |n: V3, q: &Quad| pr.facing(n, at(q, 0.5, 0.5));

    // Thin plastic edges.
    for (n, q, a) in [([1.0, 0.0, 0.0], &right, 0.42), ([0.0, 1.0, 0.0], &top, 0.55), ([0.0, -1.0, 0.0], &bottom, 0.48)] {
        if face(n, q) > 0.0 {
            let k = pr.shade(n);
            quad(&mut mesh, &pr, q, flat(), 1, 1, |_, _| shaded(edge_col, k, a * alpha));
        }
    }
    // The spine: the case's colour, caps, a lighter hinge line at the front.
    let spine_face = face([-1.0, 0.0, 0.0], &spine);
    if spine_face > 0.0 {
        let k = pr.shade([-1.0, 0.0, 0.0]);
        quad(&mut mesh, &pr, &spine, flat(), 1, 6, |u, _| shaded(look.spine, k * (0.86 + 0.24 * (1.0 - (u * 2.0 - 1.0).abs())), alpha));
        for (v0, v1) in [(0.0, 0.012), (0.988, 1.0)] {
            quad(&mut mesh, &pr, &sub(&spine, 0.0, v0, 1.0, v1), flat(), 1, 1, |_, _| shaded(Color32::from_rgb(20, 20, 24), 1.0, 0.9 * alpha));
        }
        quad(&mut mesh, &pr, &sub(&spine, 0.9, 0.0, 0.95, 1.0), flat(), 1, 1, |_, _| Color32::from_white_alpha((70.0 * alpha) as u8));
    }
    p.add(egui::Shape::mesh(std::mem::take(&mut mesh)));

    // Small cover at the top of the spine.
    if spine_face > 0.0 {
        if let Some((tex, uv)) = look.cover {
            let side = 0.76 * 2.0 * HD / (2.0 * HH);
            let mut m = Mesh::with_texture(tex);
            quad(&mut m, &pr, &sub(&spine, 0.12, 0.03, 0.88, 0.03 + side), uv, 2, 2, |_, _| Color32::from_white_alpha((255.0 * alpha) as u8));
            p.add(egui::Shape::mesh(m));
        }
    }

    let front_face = face([0.0, 0.0, -1.0], &front);
    // The disc, sliding out of the open side.
    if disc > 0.005 && front_face > 0.0 {
        draw_disc(p, &pr, look, disc, spin, alpha);
    }

    if front_face > 0.0 {
        let k = pr.shade([0.0, 0.0, -1.0]);
        // Black tray seen through the lid, hinge ridges on the left.
        quad(&mut mesh, &pr, &front, flat(), 1, 1, |_, _| shaded(Color32::from_rgb(22, 22, 26), k, alpha));
        for u in [0.022, 0.058, 0.094] {
            quad(&mut mesh, &pr, &sub(&front, u, 0.0, u + 0.012, 1.0), flat(), 1, 1, |_, _| shaded(Color32::from_rgb(88, 90, 98), k, alpha));
        }
        p.add(egui::Shape::mesh(std::mem::take(&mut mesh)));
        // The booklet: the album cover.
        let art = sub(&front, 0.142, 0.02, 0.985, 0.98);
        match look.cover {
            Some((tex, uv)) => {
                let mut m = Mesh::with_texture(tex);
                let c = (255.0 * k.min(1.0)) as u8;
                quad(&mut m, &pr, &art, uv, 12, 12, |_, _| Color32::from_rgba_premultiplied((c as f32 * alpha) as u8, (c as f32 * alpha) as u8, (c as f32 * alpha) as u8, (255.0 * alpha) as u8));
                p.add(egui::Shape::mesh(m));
            }
            None => {
                quad(&mut mesh, &pr, &art, flat(), 1, 8, |_, v| shaded(look.spine, k * (1.15 - 0.35 * v), alpha));
                p.add(egui::Shape::mesh(std::mem::take(&mut mesh)));
            }
        }
        // The clear lid: a soft glare band that moves as the case tilts, and bright edges.
        let g = 0.32 - pose.yaw * 1.1 + pose.pitch * 0.9;
        quad(&mut mesh, &pr, &front, flat(), 14, 14, |u, v| {
            let d = u * 0.58 + v * 0.42 - g;
            let a = 0.035 + 0.19 * (-d * d / 0.012).exp() + 0.06 * (1.0 - v).powi(6);
            Color32::from_white_alpha((255.0 * a * alpha) as u8)
        });
        for (u0, v0, u1, v1) in [(0.0, 0.0, 1.0, 0.007), (0.993, 0.0, 1.0, 1.0), (0.0, 0.993, 1.0, 1.0), (0.13, 0.0, 0.138, 1.0)] {
            quad(&mut mesh, &pr, &sub(&front, u0, v0, u1, v1), flat(), 1, 1, |_, _| Color32::from_white_alpha((60.0 * alpha) as u8));
        }
        p.add(egui::Shape::mesh(mesh));
    }

    // Spine text, while the spine faces you (it fades as the case turns).
    let fade = ((spine_face - 0.55) / 0.35).clamp(0.0, 1.0);
    if fade > 0.0 && !look.label.is_empty() {
        let size = (0.054 * s).clamp(8.0, 40.0);
        let room = 0.80 * s;
        let col = if light_color(look.spine) { Color32::from_rgb(24, 24, 28) } else { Color32::from_rgb(245, 245, 248) };
        let mut job = LayoutJob::single_section(look.label.to_owned(), TextFormat::simple(font(size, "semibold"), col.gamma_multiply(fade * alpha)));
        job.wrap = TextWrapping::truncate_at_width(room);
        let g = p.layout_job(job);
        let c = pr.pt(at(&spine, 0.5, 0.95));
        let pos = pos2(c.x - g.size().y * 0.5, c.y);
        p.add(TextShape::new(pos, g, col).with_angle(-FRAC_PI_2));
    }
    bounds
}

fn draw_disc(p: &Painter, pr: &Proj, look: &Look, disc: f32, spin: f32, alpha: f32) {
    let cx = disc * DISC_SLIDE;
    let seg = 96;
    let ring = |m: &mut Mesh, r0: f32, r1: f32, color: &dyn Fn(f32, f32) -> Color32, uv: &dyn Fn(f32, f32) -> Pos2| {
        let base = m.vertices.len() as u32;
        for k in 0..=seg {
            let th = k as f32 / seg as f32 * TAU;
            for r in [r0, r1] {
                m.vertices.push(Vertex { pos: pr.pt([cx + r * th.cos(), r * th.sin(), 0.0]), uv: uv(r, th), color: color(r, th) });
            }
        }
        for k in 0..seg as u32 {
            let a = base + k * 2;
            m.indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
    };
    let white = |a: f32| Color32::from_white_alpha((255.0 * a * alpha) as u8);
    let mut m = Mesh::default();
    // Clear centre ring and the silver stacking ring.
    ring(&mut m, 0.065, 0.15, &|_, _| white(0.22), &|_, _| WHITE_UV);
    ring(&mut m, 0.15, 0.168, &|_, _| shaded(Color32::from_rgb(200, 204, 212), 1.0, 0.85 * alpha), &|_, _| WHITE_UV);
    p.add(egui::Shape::mesh(std::mem::take(&mut m)));
    // The printed label: the album art, turning with the disc.
    match look.cover {
        Some((tex, uv)) => {
            let mut t = Mesh::with_texture(tex);
            let map = |r: f32, th: f32| {
                let k = r / DISC_R * 0.5;
                pos2(uv.center().x + uv.width() * k * (th + spin).cos(), uv.center().y - uv.height() * k * (th + spin).sin())
            };
            ring(&mut t, 0.168, DISC_R, &|_, _| white(1.0), &map);
            p.add(egui::Shape::mesh(t));
        }
        None => {
            ring(&mut m, 0.168, DISC_R, &|r, _| shaded(Color32::from_rgb(196, 200, 210), 0.8 + 0.3 * (r / DISC_R), alpha), &|_, _| WHITE_UV);
            p.add(egui::Shape::mesh(std::mem::take(&mut m)));
        }
    }
    // Sheen that sweeps round as the case tilts, and the rim.
    let sweep = pr.pose.yaw * 4.0 - pr.pose.pitch * 3.0;
    ring(&mut m, 0.168, DISC_R, &|_, th| white(0.05 + 0.20 * (0.5 + 0.5 * (2.0 * th + sweep).cos()).powi(8)), &|_, _| WHITE_UV);
    ring(&mut m, DISC_R - 0.006, DISC_R, &|_, _| shaded(Color32::from_rgb(225, 228, 235), 1.0, 0.9 * alpha), &|_, _| WHITE_UV);
    p.add(egui::Shape::mesh(m));
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Overshoots a little before settling (a soft pop).
fn back_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + t * t * (2.7 * t + 1.7)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scale for a case standing on the shelf: its spine is nearer the eye than
/// its middle, so a little smaller makes the spine exactly fill its slot.
fn shelf_scale(h: f32) -> f32 {
    h * (CAM - HW * h) / CAM
}

/// The case on its way between its slot (`t` = 0, spine out) and the middle
/// of the page (`t` = 1, facing you, tilted by `tilt`).
pub fn pose_at(t: f32, slot: Rect, target: Pos2, size: f32, tilt: [f32; 2]) -> Pose {
    let e = ease(t);
    let lift = (PI * t).sin();
    Pose {
        center: slot.center() + (target - slot.center()) * e + vec2(0.0, -50.0 * lift),
        scale: (shelf_scale(slot.height()) + (size - shelf_scale(slot.height())) * e) * (1.0 + 0.05 * lift),
        yaw: -FRAC_PI_2 * (1.0 - smoothstep(0.08, 0.85, t)) + tilt[0] * e,
        pitch: tilt[1] * e,
    }
}

impl Starlite {
    /// The shelf for a playlist's songs inside `area`; returns a song to play.
    #[allow(clippy::too_many_arguments)]
    pub fn shelf_view(&mut self, ui: &mut Ui, area: Rect, content: Rect, tracks: &[Track], key: &str, dt: f32, animating: &mut bool) -> Option<usize> {
        let ctx = ui.ctx().clone();
        let theme = self.theme;
        let dt = dt.clamp(0.0, 0.05);
        // Under an open dropdown or dialog the shelf doesn't see the mouse.
        let blocked = self.overlay_open();
        let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|_| !blocked);
        let online = self.lprefs.online;
        let volume = self.player.effective_volume();
        let mut play = None;

        let sh = &mut self.shelf;
        sh.drawn = true;
        if sh.key != key {
            sh.reset();
            sh.key = key.to_string();
            sh.page = 0;
            sh.page_pos = 0.0;
            sh.page_v = 0.0;
            sh.sel_off = 0.0;
        }
        sh.collect_colors();

        // Layout: pages of two rows of spines on boards, page numbers on the
        // right when there's more than one page. Spines shrink to fit two rows.
        let full = area.shrink2(vec2(16.0, 12.0));
        let sp_h = ((full.height() - ROW_GAP - 12.0) / ROWS as f32).clamp(100.0, SPINE_H);
        let sp_w = sp_h * SPINE_W / SPINE_H;
        let gap = SPINE_GAP * sp_h / SPINE_H;
        let row_h = sp_h + ROW_GAP;
        let fit = |w: f32| (((w + gap) / (sp_w + gap)).floor() as usize).max(1);
        let mut inner = full;
        let mut per_row = fit(inner.width());
        if tracks.len() > per_row * ROWS {
            inner.max.x -= SELECTOR_W;
            per_row = fit(inner.width());
        }
        let per_page = per_row * ROWS;
        let pages = tracks.len().div_ceil(per_page).max(1);
        sh.page = sh.page.min(pages - 1);
        let held = sh.out.iter().any(|p| !p.leaving);
        // The mouse wheel turns pages too, one at a time.
        let mut turn: Option<usize> = None;
        sh.wheel_cool = (sh.wheel_cool - dt).max(0.0);
        sh.wheel *= 1.0 - (dt * 3.0).min(1.0);
        if !held && pages > 1 && pointer.is_some_and(|p| area.contains(p)) {
            sh.wheel += ctx.input(|i| i.raw_scroll_delta.y);
            if sh.wheel.abs() >= 40.0 && sh.wheel_cool <= 0.0 {
                let next = if sh.wheel < 0.0 { (sh.page + 1).min(pages - 1) } else { sh.page.saturating_sub(1) };
                if next != sh.page {
                    turn = Some(next);
                }
                sh.wheel = 0.0;
                sh.wheel_cool = 0.3;
            }
        }
        sh.page_pos = smooth_damp(sh.page_pos, sh.page as f32, &mut sh.page_v, 0.3, dt);
        if (sh.page_pos - sh.page as f32).abs() < 0.0005 && sh.page_v.abs() < 0.01 {
            sh.page_pos = sh.page as f32;
            sh.page_v = 0.0;
        } else {
            *animating = true;
        }
        let page_h = area.height();
        let page_pos = sh.page_pos;
        let cur_page = sh.page;
        // Where song i stands. While pages slide, the lower row follows a beat later.
        let row_d = move |pg: usize, r: usize| {
            let d = pg as f32 - page_pos;
            let lag = r as f32 / ROWS as f32;
            if d.abs() < 1.0 {
                d.signum() * (d.abs() * (1.0 + 0.45 * lag)).min(1.0)
            } else {
                d
            }
        };
        let slot = move |i: usize| {
            let (pg, local) = (i / per_page, i % per_page);
            let (r, c) = (local / per_row, local % per_row);
            Rect::from_min_size(pos2(inner.left() + c as f32 * (sp_w + gap), inner.top() + r as f32 * row_h + row_d(pg, r) * page_h), vec2(sp_w, sp_h))
        };

        // Where a pulled-out case goes, and how big.
        let size = (content.height() * 0.5).min((content.width() - 200.0) / 1.75).clamp(160.0, 380.0);
        let group_w = (HW + DISC_SLIDE + DISC_R) * size + 110.0;
        let target = pos2(content.center().x - group_w * 0.5 + HW * size, content.center().y - content.height() * 0.05);

        // Debug: STARLITE_SHELF_PULL=<n> pulls song n out (as if hovered) and
        // STARLITE_SHELF_TILT=<x>,<y> (-1..1) points at the case there.
        let debug_pull: Option<usize> = std::env::var("STARLITE_SHELF_PULL").ok().and_then(|v| v.parse().ok()).filter(|&i| i < tracks.len());
        let debug_tilt: Option<Vec2> = std::env::var("STARLITE_SHELF_TILT").ok().and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some(vec2(x.trim().parse().ok()?, y.trim().parse().ok()?))
        });
        let pointer = match (debug_tilt, sh.case_rect) {
            (Some(n), Some(r)) => Some(r.center() + n * r.size() * 0.5),
            _ => pointer,
        };
        // Just clicked: the pointer counts as away until it moves.
        if let (Some(h), Some(p)) = (sh.hold, pointer) {
            if (p - h).length() > 8.0 {
                sh.hold = None;
            }
        }
        let pointer = if sh.hold.is_some() { None } else { pointer };

        // Which spine is under the pointer (not when it's on the big case).
        let over_case = pointer.is_some_and(|p| sh.case_rect.is_some_and(|r| r.expand(24.0).contains(p))) || (debug_pull.is_some() && sh.case_rect.is_some());
        let hovered = if debug_pull.is_some() && sh.out.is_empty() { debug_pull } else { None };
        let hovered = hovered.or(pointer.filter(|p| area.contains(*p) && !over_case).and_then(|p| {
            if (page_pos - cur_page as f32).abs() > 0.05 {
                return None;
            }
            let rel = p - inner.min;
            let (c, r) = ((rel.x / (sp_w + gap)).floor(), (rel.y / row_h).floor());
            if c < 0.0 || r < 0.0 || c as usize >= per_row || r as usize >= ROWS || rel.x - c * (sp_w + gap) > sp_w || rel.y - r * row_h > sp_h {
                return None;
            }
            let i = cur_page * per_page + r as usize * per_row + c as usize;
            (i < tracks.len()).then_some(i)
        }));

        // Hover intent: a short pause on a spine pulls it out; leaving puts it back.
        let active = sh.out.iter().position(|p| !p.leaving);
        let active_idx = active.map(|a| sh.out[a].idx);
        match hovered {
            Some(i) if Some(i) != active_idx => {
                let t = match sh.intent {
                    Some((j, t)) if j == i => t + dt,
                    _ => 0.0,
                };
                sh.intent = Some((i, t));
                *animating = true;
                if t >= if active.is_some() { 0.08 } else { 0.14 } {
                    if let Some(a) = active {
                        sh.out[a].leaving = true;
                    }
                    sh.out.retain(|p| p.idx != i);
                    sh.out.push(Pulled { track: tracks[i].clone(), idx: i, t: 0.0, leaving: false, disc: 0.0, disc_v: 0.0, spin: 0.0 });
                    sh.intent = None;
                    sh.away = 0.0;
                }
            }
            Some(_) => {
                sh.intent = None;
                sh.away = 0.0;
            }
            None => {
                sh.intent = None;
                if over_case {
                    sh.away = 0.0;
                } else if let Some(a) = active {
                    sh.away += dt;
                    *animating = true;
                    if sh.away > 0.28 {
                        sh.out[a].leaving = true;
                    }
                }
            }
        }

        // Animate the pulled cases.
        let audible = sh.preview.audible();
        for p in sh.out.iter_mut() {
            if p.leaving {
                p.t -= dt / PUT_BACK;
            } else {
                p.t = (p.t + dt / PULL).min(1.0);
            }
            let want = if !p.leaving && p.t >= 0.97 { 1.0 } else { 0.0 };
            p.disc = smooth_damp(p.disc, want, &mut p.disc_v, if want > 0.5 { 0.38 } else { 0.12 }, dt).clamp(0.0, 1.0);
            if audible && !p.leaving {
                p.spin += dt * 1.4;
            }
            *animating |= p.t < 1.0 || (p.disc - want).abs() > 0.002 || audible;
        }
        sh.out.retain(|p| !(p.leaving && p.t <= 0.0));
        let active = sh.out.iter().position(|p| !p.leaving);

        // Tilt towards the pointer while it's on the case.
        let tilt_to = match (pointer, sh.case_rect, active) {
            (Some(p), Some(r), Some(_)) if r.expand(24.0).contains(p) => {
                let n = (p - r.center()) / (r.size() * 0.5);
                [(-n.x).clamp(-1.0, 1.0) * 0.30, (-n.y).clamp(-1.0, 1.0) * 0.22]
            }
            _ => [0.0, 0.0],
        };
        for k in 0..2 {
            sh.tilt[k] = smooth_damp(sh.tilt[k], tilt_to[k], &mut sh.tilt_v[k], 0.16, dt);
            *animating |= (sh.tilt[k] - tilt_to[k]).abs() > 0.001;
        }

        // Preview: the case that's out plays a bit of its song.
        // Loading starts early in the pull, so the sound is ready about when the disc is out.
        let preview_for = active.filter(|&a| sh.out[a].t > 0.25).map(|a| sh.out[a].track.clone());
        let stream = preview_for.as_ref().and_then(|t| match &t.source {
            Source::Navidrome { song_id, .. } => self.nd.preview_url(song_id),
            _ => None,
        });
        let sh = &mut self.shelf;
        match &preview_for {
            Some(t) => {
                if sh.sent_volume != volume {
                    sh.sent_volume = volume;
                    sh.preview.set_volume(volume);
                }
                sh.preview.play(t, stream, online);
            }
            None => sh.preview.stop(),
        }

        // ── Drawing ──
        let p = ui.painter().with_clip_rect(area);
        let dark = !theme.is_light();
        // Boards under each row of the pages on screen (empty ones too: it's a shelf).
        let vis: Vec<usize> = (0..pages).filter(|&pg| (pg as f32 - page_pos).abs() < 1.5).collect();
        for &pg in &vis {
            for r in 0..ROWS {
                let y = inner.top() + r as f32 * row_h + row_d(pg, r) * page_h + sp_h;
                if y < area.top() - 20.0 || y > area.bottom() + 20.0 {
                    continue;
                }
                let board = Rect::from_min_max(pos2(inner.left() - 10.0, y), pos2(inner.right() + 10.0, y + 9.0));
                p.add(Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 40 }) }.as_shape(board, egui::CornerRadius::same(4)));
                let (top, body) = if dark { (Color32::from_white_alpha(46), Color32::from_white_alpha(18)) } else { (Color32::from_white_alpha(230), Color32::from_black_alpha(22)) };
                p.rect_filled(board, 4.0, body);
                p.rect_filled(Rect::from_min_max(board.min, pos2(board.right(), board.top() + 1.5)), 2.0, top);
            }
        }
        // The spines.
        let pulled: Vec<usize> = sh.out.iter().map(|p| p.idx).collect();
        for &pg in &vis {
            for i in pg * per_page..((pg + 1) * per_page).min(tracks.len()) {
                let r = slot(i);
                if pulled.contains(&i) || r.bottom() < area.top() || r.top() > area.bottom() {
                    continue;
                }
                let t = &tracks[i];
                if let Some(u) = &t.artwork {
                    sh.want_color(&ctx, u);
                }
                let spine = t.artwork.as_ref().and_then(|u| sh.colors.get(u).copied()).unwrap_or(if dark { Color32::from_rgb(58, 60, 68) } else { Color32::from_rgb(210, 212, 218) });
                let lift = ctx.animate_bool_with_time(Id::new(("spine_h", key, i)), hovered == Some(i), 0.12);
                let label = format!("{}  ·  {}", t.title, t.artist);
                let cover = cover_texture(&ctx, t.artwork.as_deref(), Vec2::splat(1.0));
                let pose = Pose { center: r.center() - vec2(0.0, 7.0 * lift), scale: shelf_scale(r.height()), yaw: -FRAC_PI_2, pitch: 0.0 };
                draw_case(&p, pose, &Look { cover, spine, label: &label }, 0.0, 0.0, 1.0);
            }
        }

        // Page numbers: hover one and it grows while a ring draws itself
        // round it; the current page sits in a filled circle.
        if pages > 1 {
            let step = 40.0;
            let max_vis = (((inner.height() - 8.0) / step).floor() as usize).max(3);
            let shown = pages.min(max_vis);
            let want_off = (cur_page as f32 - (shown as f32 - 1.0) * 0.5).clamp(0.0, (pages - shown) as f32);
            sh.sel_off += (want_off - sh.sel_off) * (dt * 10.0).min(1.0);
            if (sh.sel_off - want_off).abs() > 0.001 {
                *animating = true;
            }
            let x = area.right() - SELECTOR_W * 0.5 - 2.0;
            let top = inner.center().y - (shown as f32 - 1.0) * step * 0.5;
            let (fg, dim) = (theme.text(), theme.text_dim());
            let (fill, on_fill) = if dark { (Color32::from_white_alpha(235), Color32::from_rgb(20, 20, 24)) } else { (Color32::from_rgb(28, 28, 32), Color32::WHITE) };
            for k in 0..pages {
                let slot_f = k as f32 - sh.sel_off;
                if slot_f < -0.6 || slot_f > shown as f32 - 0.4 {
                    continue;
                }
                // Fade the numbers at the ends of a long list.
                let edge = if pages > shown { (slot_f + 0.6).min(shown as f32 - 0.4 - slot_f).clamp(0.0, 1.0) } else { 1.0 };
                let c = pos2(x, top + slot_f * step);
                let id = Id::new(("shelf_page", key, k));
                let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(36.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                // Debug: STARLITE_SHELF_PAGE_HOVER=<n> shows number n as hovered.
                let debug_hover = std::env::var("STARLITE_SHELF_PAGE_HOVER").ok().and_then(|v| v.parse::<usize>().ok()) == Some(k + 1);
                let h_lin = ctx.animate_bool_with_time(id.with("h"), resp.hovered() || debug_hover, 0.24);
                let sel = ctx.animate_bool_with_time(id.with("s"), k == cur_page, 0.28);
                *animating |= (h_lin > 0.0 && h_lin < 1.0) || (sel > 0.0 && sel < 1.0);
                let h = ease_out(h_lin);
                let radius = 15.0 + 2.0 * h;
                if sel > 0.0 {
                    p.circle_filled(c, radius * back_out(sel), fill.gamma_multiply(edge));
                }
                if h > 0.0 && sel < 1.0 {
                    // The ring sweeps round from the top as it appears.
                    let sweep = TAU * h;
                    let n = 48;
                    let pts: Vec<Pos2> = (0..=n)
                        .map(|j| {
                            let a = -FRAC_PI_2 + sweep * j as f32 / n as f32;
                            c + vec2(a.cos(), a.sin()) * radius
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, egui::Stroke::new(1.6_f32, fg.gamma_multiply(0.85 * edge * (1.0 - sel)))));
                }
                let base = lerp_color(dim, fg, h);
                let col = lerp_color(base, on_fill, sel).gamma_multiply(edge);
                p.text(c, Align2::CENTER_CENTER, format!("{}", k + 1), font(14.0 * (1.0 + 0.3 * h), "semibold"), col);
                if resp.clicked() && k != cur_page {
                    turn = Some(k);
                }
            }
        }
        // Debug: STARLITE_SHELF_TURN=<n> turns to page n once, 2 s in.
        if let Some(n) = std::env::var("STARLITE_SHELF_TURN").ok().and_then(|v| v.parse::<usize>().ok()) {
            if ctx.input(|i| i.time) > 2.0 && !sh.debug_turned && n >= 1 && n <= pages {
                sh.debug_turned = true;
                turn = Some(n - 1);
            }
        }
        if let Some(n) = turn {
            sh.page = n;
            for pl in &mut sh.out {
                pl.leaving = true;
            }
            self.sound.page();
        }
        let sh = &mut self.shelf;
        if let Some(i) = hovered {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            if !blocked && ctx.input(|inp| inp.pointer.primary_clicked()) {
                play = Some(i);
            }
        }

        // The page blurs behind a pulled-out case.
        let want_blur = sh.out.iter().map(|p| ease(p.t)).fold(0.0, f32::max);
        sh.blur = want_blur;
        if sh.blur > 0.003 {
            let r = Rect::from_min_max(pos2(content.left() - 32.0, 0.0), pos2(content.right() + 32.0, content.bottom() + 16.0));
            let tint = if dark { [0.0, 0.0, 0.0, 0.22] } else { [1.0, 1.0, 1.0, 0.28] };
            ui.painter().add(crate::blur::callback(sh.blur_gl.clone(), r, sh.blur, 26.0, tint));
        }

        // The pulled cases (the one leaving first, under the new one).
        let fp = ui.painter().with_clip_rect(content.expand2(vec2(32.0, 16.0)));
        sh.case_rect = None;
        // At rest the case is turned a little (a hint of its spine) and floats.
        let time = ctx.input(|i| i.time) as f32;
        let tilt = [sh.tilt[0] - 0.16 + 0.035 * (time * 0.7).sin(), sh.tilt[1] + 0.025 * (time * 0.55).sin()];
        *animating |= active.is_some();
        let mut order: Vec<usize> = (0..sh.out.len()).collect();
        order.sort_by_key(|&k| !sh.out[k].leaving);
        for k in order {
            let pl = &sh.out[k];
            let pose = pose_at(pl.t, slot(pl.idx), target, size, if pl.leaving { [-0.16, 0.0] } else { tilt });
            let t = &pl.track;
            let spine = t.artwork.as_ref().and_then(|u| sh.colors.get(u).copied()).unwrap_or(if dark { Color32::from_rgb(58, 60, 68) } else { Color32::from_rgb(210, 212, 218) });
            let label = format!("{}  ·  {}", t.title, t.artist);
            let cover = cover_texture(&ctx, t.artwork.as_deref(), Vec2::splat(1.0));
            let r = draw_case(&fp, pose, &Look { cover, spine, label: &label }, pl.disc, pl.spin, 1.0);
            if !pl.leaving && pl.t > 0.5 {
                sh.case_rect = Some(r.union(Rect::from_center_size(pos2(r.right() + pl.disc * DISC_SLIDE * size * 0.5, r.center().y), vec2(pl.disc * DISC_SLIDE * size, r.height()))));
            }
        }

        // Song info (bottom left of the page) and the equalizer.
        let shown = sh.out.iter().filter(|p| !p.leaving).chain(sh.out.iter().filter(|p| p.leaving)).next();
        if let Some(pl) = shown {
            let a = ease(pl.t);
            let col = |c: Color32, k: f32| c.gamma_multiply(a * k);
            let (text, dim) = (theme.text(), theme.text_dim());
            let x = content.left() + 6.0;
            let w = content.width() * 0.6;
            let y = content.bottom() - 12.0 + 18.0 * (1.0 - a);
            let t = &pl.track;
            let album_line = match (&t.album, t.source_name()) {
                (Some(al), Some(src)) => format!("{al} · {src}"),
                (Some(al), None) => al.clone(),
                (None, Some(src)) => src.to_string(),
                (None, None) => String::new(),
            };
            if !album_line.is_empty() {
                text_trunc(&fp, pos2(x, y), Align2::LEFT_BOTTOM, &album_line, font(13.5, "regular"), col(dim, 1.0), w);
            }
            text_trunc(&fp, pos2(x, y - 22.0), Align2::LEFT_BOTTOM, &t.artist, font(17.0, "semibold"), col(dim, 1.0), w);
            text_trunc(&fp, pos2(x, y - 46.0), Align2::LEFT_BOTTOM, &t.title, font(32.0, "bold"), col(text, 1.0), w);
            if t.youtube_id().is_some() && !pl.leaving {
                text_trunc(&fp, pos2(x, y - 92.0), Align2::LEFT_BOTTOM, "No preview for YouTube songs", font(12.5, "regular"), col(dim, 0.8), w);
            }
        }
        // Equalizer: the preview's sound in seven bands, right of the disc.
        let db = band_db(sh.preview.levels.get());
        let loudest = db.iter().copied().fold(-120.0, f32::max);
        // The peak follows loud moments at once and lets go slowly.
        sh.eq_peak = if loudest > sh.eq_peak { loudest } else { (sh.eq_peak - dt * 5.0).max(loudest).max(-60.0) };
        let lv = bar_heights(db, sh.eq_peak.max(-40.0));
        for (e, h) in sh.eq.iter_mut().zip(lv) {
            let k = if h > *e { 28.0 } else { 7.0 };
            *e += (h - *e) * (dt * k).min(1.0);
        }
        let eq_on = audible && active.is_some();
        sh.eq_a += ((if eq_on { 1.0 } else { 0.0 }) - sh.eq_a) * (dt * 6.0).min(1.0);
        if sh.eq_a > 0.01 {
            *animating = true;
            let a = sh.eq_a;
            let (bw, gap) = (7.0, 6.0);
            let x0 = target.x + (DISC_SLIDE + DISC_R) * size + 40.0;
            let bottom = target.y + HH * size;
            let max_h = size * 0.42;
            let c = if dark { Color32::WHITE } else { theme.text() };
            for (i, e) in sh.eq.iter().enumerate() {
                let h = 6.0 + (max_h - 6.0) * e * a;
                let r = Rect::from_min_max(pos2(x0 + i as f32 * (bw + gap), bottom - h), pos2(x0 + i as f32 * (bw + gap) + bw, bottom));
                fp.rect_filled(r, bw * 0.5, c.gamma_multiply(0.85 * a));
            }
        }

        // Clicking the big case plays its song.
        if let (Some(r), Some(a)) = (sh.case_rect, active) {
            let resp = ui.interact(r, Id::new(("shelf_case", key)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            if resp.clicked() {
                play = Some(sh.out[a].idx);
            }
        }
        play
    }

    /// After the page is drawn: tidy up when the shelf isn't on screen and
    /// turn the main player down while a preview plays.
    pub fn shelf_after_frame(&mut self, dt: f32) {
        if !self.shelf.drawn {
            if !self.shelf.out.is_empty() || self.shelf.preview.current.is_some() {
                self.shelf.reset();
            }
        }
        self.shelf.drawn = false;
        let target = if self.shelf.preview.audible() { 0.22 } else { 1.0 };
        let sh = &mut self.shelf;
        sh.duck += (target - sh.duck) * (dt.clamp(0.0, 0.05) * 5.0).min(1.0);
        if (sh.duck - target).abs() < 0.005 {
            sh.duck = target;
        }
        if (sh.duck - sh.duck_sent).abs() > 0.01 || (sh.duck == target && sh.duck_sent != target) {
            sh.duck_sent = sh.duck;
            let v = self.player.effective_volume() * self.shelf.duck;
            self.audio.set_volume(v);
            if let Some(p) = &self.ytp {
                p.set_volume(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_the_shelf_only_the_spine_shows() {
        let slot = Rect::from_min_size(pos2(100.0, 100.0), vec2(SPINE_W, SPINE_H));
        let pose = pose_at(0.0, slot, pos2(600.0, 400.0), 340.0, [0.0, 0.0]);
        let pr = Proj::new(pose);
        let c = |q: V3| q;
        assert!(pr.facing([-1.0, 0.0, 0.0], c([-HW, 0.0, 0.0])) > 0.99, "spine faces you");
        assert!(pr.facing([0.0, 0.0, -1.0], c([0.0, 0.0, -HD])) <= 0.0, "cover hidden");
        // Its spine covers the slot.
        let a = pr.pt([-HW, HH, HD]);
        let b = pr.pt([-HW, -HH, -HD]);
        assert!((a.x - slot.left()).abs() < 1.5 && (b.x - slot.right()).abs() < 1.5, "{a:?} {b:?}");
        assert!((a.y - slot.top()).abs() < 2.0 && (b.y - slot.bottom()).abs() < 2.0, "{a:?} {b:?}");
    }

    #[test]
    fn pulled_out_it_faces_you_in_the_middle() {
        let slot = Rect::from_min_size(pos2(100.0, 100.0), vec2(SPINE_W, SPINE_H));
        let pose = pose_at(1.0, slot, pos2(600.0, 400.0), 340.0, [0.0, 0.0]);
        let pr = Proj::new(pose);
        assert!(pr.facing([0.0, 0.0, -1.0], [0.0, 0.0, -HD]) > 0.99);
        assert!(pr.facing([-1.0, 0.0, 0.0], [-HW, 0.0, 0.0]) <= 0.0);
        let tl = pr.pt([-HW, HH, -HD]);
        let br = pr.pt([HW, -HH, -HD]);
        assert!(((tl.x + br.x) * 0.5 - 600.0).abs() < 0.5 && ((br.y - tl.y) - 340.0).abs() < 8.0, "{tl:?} {br:?}");
    }

    #[test]
    fn the_case_leans_towards_the_pointer_side_going_away() {
        // Pointer on the right: tilt yaw negative; the right edge recedes (smaller).
        let pose = Pose { center: pos2(0.0, 0.0), scale: 340.0, yaw: -0.3, pitch: 0.0 };
        let pr = Proj::new(pose);
        let left_h = pr.pt([-HW, -HH, -HD]).y - pr.pt([-HW, HH, -HD]).y;
        let right_h = pr.pt([HW, -HH, -HD]).y - pr.pt([HW, HH, -HD]).y;
        assert!(right_h < left_h);
    }
}
