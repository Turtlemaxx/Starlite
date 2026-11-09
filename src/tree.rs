//! Procedurally painted sakura tree for the Cherry Blossom theme.
//! Rendered once on the CPU (tiny-skia) into a premultiplied RGBA image that
//! the glass shader uses as part of the background, plus a pre-blurred copy
//! the glass samples when it frosts the tree.

use tiny_skia::{
    Color, FillRule, GradientStop, Paint, PathBuilder, Pixmap, Point, RadialGradient, Rect,
    SpreadMode, Transform,
};

pub struct TreeImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Same image, Gaussian-blurred (what frosted glass sees).
    pub blurred: Vec<u8>,
}

struct Rng(u64);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

const MAX_DEPTH: u32 = 6;
const BARK: [u8; 3] = [60, 34, 41];
const PETALS: [[u8; 3]; 4] = [[255, 183, 197], [255, 205, 215], [246, 160, 184], [255, 226, 232]];

type P = (f32, f32);

struct Anchor {
    p: P,
    tip: bool,
}

/// Tree geometry in "tree space": trunk base at the origin, growing up (-y).
#[derive(Default)]
struct Geo {
    limbs: Vec<Vec<P>>,
    joints: Vec<(P, f32)>,
    anchors: Vec<Anchor>,
}

fn quad(a: P, c: P, b: P, t: f32) -> P {
    let u = 1.0 - t;
    (u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1)
}

fn limb(geo: &mut Geo, rng: &mut Rng, p0: P, ang: f32, len: f32, w0: f32, depth: u32, min_len: f32) {
    let w1 = w0 * 0.62;
    let dir = (ang.cos(), ang.sin());
    let p2 = (p0.0 + dir.0 * len, p0.1 + dir.1 * len);
    let bend = rng.range(-0.16, 0.16) * len;
    let c = (p0.0 + dir.0 * len * 0.5 - dir.1 * bend, p0.1 + dir.1 * len * 0.5 + dir.0 * bend);

    let n = 12;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let pt = quad(p0, c, p2, t);
        let d = (
            2.0 * (1.0 - t) * (c.0 - p0.0) + 2.0 * t * (p2.0 - c.0),
            2.0 * (1.0 - t) * (c.1 - p0.1) + 2.0 * t * (p2.1 - c.1),
        );
        let l = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-3);
        let nrm = (-d.1 / l, d.0 / l);
        let hw = (w0 + (w1 - w0) * t) * 0.5;
        left.push((pt.0 + nrm.0 * hw, pt.1 + nrm.1 * hw));
        right.push((pt.0 - nrm.0 * hw, pt.1 - nrm.1 * hw));
    }
    right.reverse();
    left.extend(right);
    geo.limbs.push(left);
    geo.joints.push((p0, w0 * 0.5));

    if depth >= 3 {
        geo.anchors.push(Anchor { p: c, tip: false });
    }
    if depth >= MAX_DEPTH || len < min_len {
        geo.anchors.push(Anchor { p: p2, tip: true });
        return;
    }
    let kids = if depth == 0 || rng.f() > 0.35 { 2 } else { 3 };
    for k in 0..kids {
        let spread = if depth < 2 { 0.44 } else { 0.58 };
        let off = (k as f32 - (kids as f32 - 1.0) * 0.5) * spread + rng.range(-0.16, 0.16);
        let a = (ang + off + 0.07).clamp(-2.95, -0.08);
        let l = len * rng.range(0.68, 0.80);
        limb(geo, rng, p2, a, l, w1, depth + 1, min_len);
    }
}

fn fill(pm: &mut Pixmap, path: &tiny_skia::Path, paint: &Paint) {
    pm.fill_path(path, paint, FillRule::Winding, Transform::identity(), None);
}

fn soft_disc(pm: &mut Pixmap, x: f32, y: f32, r: f32, rgb: [u8; 3], a: u8) {
    let stops = vec![
        GradientStop::new(0.0, Color::from_rgba8(rgb[0], rgb[1], rgb[2], a)),
        GradientStop::new(1.0, Color::from_rgba8(rgb[0], rgb[1], rgb[2], 0)),
    ];
    let Some(shader) =
        RadialGradient::new(Point::from_xy(x, y), Point::from_xy(x, y), r, stops, SpreadMode::Pad, Transform::identity())
    else {
        return;
    };
    let mut paint = Paint::default();
    paint.shader = shader;
    paint.anti_alias = true;
    if let Some(c) = PathBuilder::from_circle(x, y, r) {
        fill(pm, &c, &paint);
    }
}

fn flower(pm: &mut Pixmap, rng: &mut Rng, x: f32, y: f32, size: f32) {
    let col = PETALS[(rng.f() * PETALS.len() as f32) as usize % PETALS.len()];
    let alpha = rng.range(170.0, 255.0) as u8;
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(col[0], col[1], col[2], alpha);
    let rot = rng.range(0.0, 72.0);
    for k in 0..5 {
        let deg = rot + k as f32 * 72.0;
        let a = deg.to_radians();
        let (cx, cy) = (x + a.cos() * size * 0.52, y + a.sin() * size * 0.52);
        let Some(r) = Rect::from_xywh(cx - size * 0.5, cy - size * 0.32, size, size * 0.64) else { continue };
        if let Some(o) = PathBuilder::from_oval(r) {
            pm.fill_path(&o, &paint, FillRule::Winding, Transform::from_rotate_at(deg, cx, cy), None);
        }
    }
    let mut c = Paint::default();
    c.anti_alias = true;
    c.set_color_rgba8(214, 96, 128, alpha);
    if let Some(dot) = PathBuilder::from_circle(x, y, size * 0.22) {
        fill(pm, &dot, &c);
    }
}

fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let at = |o: usize, i: usize| if horizontal { (o * w + i) * 4 } else { (i * w + o) * 4 };
    let div = (2 * r + 1) as u32;
    let last = inner as isize - 1;
    for o in 0..outer {
        let mut acc = [0u32; 4];
        for k in -(r as isize)..=(r as isize) {
            let p = at(o, k.clamp(0, last) as usize);
            for c in 0..4 {
                acc[c] += src[p + c] as u32;
            }
        }
        for i in 0..inner {
            let p = at(o, i);
            for c in 0..4 {
                dst[p + c] = (acc[c] / div) as u8;
            }
            let add = at(o, (i as isize + r as isize + 1).min(last) as usize);
            let sub = at(o, (i as isize - r as isize).max(0) as usize);
            for c in 0..4 {
                acc[c] += src[add + c] as u32;
                acc[c] -= src[sub + c] as u32;
            }
        }
    }
}

fn gaussian(src: &[u8], w: usize, h: usize, sigma: f32) -> Vec<u8> {
    let r = ((((4.0 * sigma * sigma) + 1.0).sqrt() - 1.0) * 0.5).round().max(1.0) as usize;
    let mut a = src.to_vec();
    let mut b = vec![0u8; src.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, w, h, r, true);
        box_pass(&b, &mut a, w, h, r, false);
    }
    a
}

/// Paint the tree into a `w_pt` × `h_pt` (points) image at `ppp` scale. The
/// trunk rises from the bottom edge; the whole tree is scaled to fit.
pub fn render(w_pt: f32, h_pt: f32, ppp: f32) -> TreeImage {
    let (w, h) = ((w_pt * ppp).round().max(1.0) as u32, (h_pt * ppp).round().max(1.0) as u32);
    let (wf, hf) = (w as f32, h as f32);
    let s = ppp;
    let mut rng = Rng(0x5EED_CAFE_F00D);

    let mut geo = Geo::default();
    limb(&mut geo, &mut rng, (0.0, 0.0), -1.30, hf * 0.30, 30.0 * s, 0, 9.0 * s);

    // Fit: keep the trunk base on the bottom edge, scale the crown inside.
    let pad = 34.0 * s;
    let (mut min_x, mut max_x, mut min_y) = (0.0f32, 0.0f32, 0.0f32);
    for p in geo.limbs.iter().flatten().chain(geo.anchors.iter().map(|a| &a.p)) {
        min_x = min_x.min(p.0);
        max_x = max_x.max(p.0);
        min_y = min_y.min(p.1);
    }
    let base_x = wf * 0.20;
    let scale = 1.0f32
        .min((hf - pad) / -min_y.min(-1.0))
        .min((wf - base_x - pad) / max_x.max(1.0))
        .min((base_x - pad * 0.3) / (-min_x).max(1.0));
    let map = |p: P| (base_x + p.0 * scale, hf + 6.0 * s + p.1 * scale);
    let k = scale; // size multiplier for blossoms

    let mut pm = Pixmap::new(w, h).expect("pixmap");

    // Soft blossom haze and out-of-focus blossoms behind the branches.
    for (i, a) in geo.anchors.iter().enumerate() {
        let (x, y) = map(a.p);
        if i % 3 == 0 {
            soft_disc(&mut pm, x, y, rng.range(40.0, 70.0) * s * k, [255, 170, 195], 22);
        }
        if a.tip && i % 2 == 0 {
            let (dx, dy) = (rng.range(-30.0, 30.0) * s * k, rng.range(-30.0, 30.0) * s * k);
            soft_disc(&mut pm, x + dx, y + dy, rng.range(7.0, 12.0) * s * k, [255, 196, 210], 70);
        }
    }

    // Branches.
    let mut bark = Paint::default();
    bark.anti_alias = true;
    bark.set_color_rgba8(BARK[0], BARK[1], BARK[2], 255);
    for poly in &geo.limbs {
        let mut pb = PathBuilder::new();
        let first = map(poly[0]);
        pb.move_to(first.0, first.1);
        for p in poly.iter().skip(1) {
            let q = map(*p);
            pb.line_to(q.0, q.1);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            fill(&mut pm, &path, &bark);
        }
    }
    for (c, r) in &geo.joints {
        let q = map(*c);
        if let Some(j) = PathBuilder::from_circle(q.0, q.1, r * scale) {
            fill(&mut pm, &j, &bark);
        }
    }

    // Blossom clusters.
    for a in &geo.anchors {
        let (x, y) = map(a.p);
        let count = if a.tip { rng.range(10.0, 18.0) } else { rng.range(3.0, 7.0) } as usize;
        let spread = if a.tip { 24.0 } else { 14.0 } * s * k;
        for _ in 0..count {
            let r = rng.f().sqrt() * spread;
            let t = rng.range(0.0, std::f32::consts::TAU);
            let size = rng.range(2.6, 5.4) * s * k.max(0.75);
            flower(&mut pm, &mut rng, x + t.cos() * r, y + t.sin() * r * 0.8, size);
        }
    }

    let rgba = pm.take();
    let blurred = gaussian(&rgba, w as usize, h as usize, 9.0 * s);
    TreeImage { width: w, height: h, rgba, blurred }
}
