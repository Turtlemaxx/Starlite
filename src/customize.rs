//! The custom theme (Settings → Appearance → Custom): its three colours and
//! a background picture, and the colour picker that opens from a colour.
//! Changes show at once; they're saved when the picker closes.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Id, Mesh, Rect, Sense, Shape, Stroke, Ui, Vec2};

use crate::app::{smooth, Starlite};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::theme::{CustomTheme, Theme};
use crate::widgets::{field, font, pill, text_trunc};

const NAMES: [&str; 3] = ["Background", "Accent", "Text"];

/// A few ready colours for each of the three.
const SWATCHES: [[[u8; 3]; 8]; 3] = [
    [[22, 20, 34], [12, 12, 14], [34, 18, 40], [14, 30, 26], [30, 22, 18], [18, 28, 46], [236, 228, 214], [228, 233, 242]],
    [[150, 130, 255], [110, 190, 255], [110, 225, 180], [255, 170, 120], [255, 120, 160], [240, 200, 90], [240, 92, 92], [240, 240, 245]],
    [[240, 238, 248], [250, 240, 228], [200, 202, 214], [255, 248, 231], [22, 22, 26], [26, 32, 58], [56, 40, 30], [60, 60, 68]],
];

/// The open colour picker.
pub struct ColorPick {
    /// 0 background, 1 accent, 2 text.
    pub which: usize,
    /// The colour it opened from.
    pub from: Rect,
    pub open_t: f32,
    pub hsv: [f32; 3],
    pub hex: String,
}

pub fn hex_of(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(&h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn hsv_to_rgb(hsv: [f32; 3]) -> [u8; 3] {
    let [h, s, v] = hsv;
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8]
}

pub fn rgb_to_hsv(c: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d < 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn get(c: &CustomTheme, which: usize) -> [u8; 3] {
    match which {
        0 => c.background,
        1 => c.accent,
        _ => c.text,
    }
}

impl Starlite {
    /// The custom theme's colours and picture, under the theme cards.
    /// Returns where the section ends.
    #[allow(clippy::too_many_arguments)]
    pub fn custom_theme_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, y: f32, w: f32) -> f32 {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();

        // Three colours side by side.
        let gap = 12.0;
        let cw = ((w - 2.0 * gap) / 3.0).min(170.0);
        for k in 0..3 {
            let r = Rect::from_min_size(pos2(x0 + k as f32 * (cw + gap), y), vec2(cw, 44.0));
            let id = Id::new(("custom_color", k));
            let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let open = self.color_pick.as_ref().is_some_and(|c| c.which == k);
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered() || open, 0.16);
            let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.09);
            let rr = Rect::from_center_size(r.center(), r.size() + Vec2::splat(4.0 * pr));
            els.push(El::rect(rr, ppp, 22.0, 1.0, 4.0 + 3.0 * h, Kind::Glass, 0.45 * h, pr));
            let c = get(&self.custom, k);
            let sc = pos2(r.left() + 24.0, r.center().y);
            p.circle_filled(sc, 11.0 + 1.0 * h, c32(c));
            p.circle_stroke(sc, 11.0 + 1.0 * h, Stroke::new(1.0_f32, theme.text().gamma_multiply(0.35)));
            text_trunc(&p, pos2(r.left() + 44.0, r.center().y - 7.0), Align2::LEFT_CENTER, NAMES[k], font(13.5, "semibold"), theme.text(), cw - 54.0);
            text_trunc(&p, pos2(r.left() + 44.0, r.center().y + 10.0), Align2::LEFT_CENTER, &hex_of(c), font(11.5, "regular"), theme.text_dim(), cw - 54.0);
            // Debug: STARLITE_COLOR_PICK=0..2 opens that colour's picker once.
            static DEBUG_PICK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let debug = std::env::var("STARLITE_COLOR_PICK").ok().and_then(|v| v.parse::<usize>().ok()) == Some(k) && !DEBUG_PICK.swap(true, std::sync::atomic::Ordering::Relaxed);
            if resp.clicked() || debug {
                self.sound.click();
                self.color_pick = if open { None } else { Some(ColorPick { which: k, from: r, open_t: 0.0, hsv: rgb_to_hsv(c), hex: hex_of(c) }) };
            }
        }

        // Background picture.
        let y2 = y + 58.0;
        let has = self.custom.image.is_some();
        let r = Rect::from_min_size(pos2(x0, y2), vec2(if has { 168.0 } else { 196.0 }, 38.0));
        if pill(ui, icons, els, ppp, Id::new("custom_img"), r, if has { "Change picture" } else { "Background picture…" }, Some("image-plus"), false, false, theme)
            .on_hover_text("A PNG or JPG behind everything")
            .clicked()
        {
            self.sound.click();
            if let Some(path) = rfd::FileDialog::new().add_filter("Picture", &["png", "jpg", "jpeg"]).pick_file() {
                match crate::theme::import_bg_image(&path) {
                    Some(dst) => {
                        if let Some(old) = self.custom.image.replace(dst) {
                            let _ = std::fs::remove_file(old);
                        }
                        self.custom_changed(&ctx, true);
                    }
                    None => self.toast("Couldn't use that picture (PNG or JPG only)"),
                }
            }
        }
        if has {
            let rm = Rect::from_min_size(pos2(r.right() + 10.0, y2), vec2(104.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("custom_img_rm"), rm, "Remove", Some("x"), false, false, theme).clicked() {
                self.sound.click();
                if let Some(old) = self.custom.image.take() {
                    let _ = std::fs::remove_file(old);
                }
                self.custom_changed(&ctx, true);
            }
        }
        y2 + 38.0
    }

    /// The custom colours changed: use them now (and save when `save`).
    pub fn custom_changed(&mut self, ctx: &egui::Context, save: bool) {
        let was_light = self.theme.is_light();
        CustomTheme::set(self.custom.clone());
        if self.theme == Theme::Custom && was_light != self.theme.is_light() {
            self.apply_theme(ctx);
        }
        if save {
            self.custom.save();
        }
    }

    /// The colour picker: a saturation / brightness square, a hue strip, a
    /// few ready colours and the hex code.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_color_pick(&mut self, ui: &mut Ui, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let Some(cp) = self.color_pick.as_mut() else { return };
        cp.open_t = (cp.open_t + dt * 8.0).min(1.0);
        if cp.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(cp.open_t);
        let back = ui.interact(full, ui.id().with("cp_back"), Sense::click());
        let (w, h) = (276.0, 318.0);
        let below = cp.from.bottom() + 10.0 + h < full.bottom() - 110.0;
        let x = (cp.from.left()).clamp(nav_w + 10.0, full.right() - w - 10.0);
        let y = if below { cp.from.bottom() + 10.0 - 8.0 * (1.0 - ot) } else { cp.from.top() - 10.0 - h + 8.0 * (1.0 - ot) };
        let panel = Rect::from_min_size(pos2(x, y), vec2(w, h));
        let mut els = vec![El::rect(panel, ppp, 22.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(Shape::Noop);
        let p = ui.painter().clone();
        let fg = theme.text().gamma_multiply(ot);
        let which = cp.which;
        p.text(pos2(panel.left() + 18.0, panel.top() + 22.0), Align2::LEFT_CENTER, NAMES[which], font(13.5, "semibold"), fg);

        // Saturation (across) and brightness (down) for the current hue.
        let sv = Rect::from_min_size(pos2(panel.left() + 18.0, panel.top() + 42.0), vec2(w - 36.0, 140.0));
        let (nx, ny) = (14, 10);
        let mut mesh = Mesh::default();
        for j in 0..=ny {
            for i in 0..=nx {
                let (s, v) = (i as f32 / nx as f32, 1.0 - j as f32 / ny as f32);
                let c = c32(hsv_to_rgb([cp.hsv[0], s, v])).gamma_multiply(ot);
                mesh.colored_vertex(pos2(sv.left() + s * sv.width(), sv.top() + (1.0 - v) * sv.height()), c);
            }
        }
        for j in 0..ny {
            for i in 0..nx {
                let a = (j * (nx + 1) + i) as u32;
                let b = a + nx as u32 + 1;
                mesh.add_triangle(a, a + 1, b);
                mesh.add_triangle(a + 1, b + 1, b);
            }
        }
        p.add(Shape::mesh(mesh));
        p.rect_stroke(sv, CornerRadius::same(2), Stroke::new(1.0_f32, Color32::from_white_alpha((40.0 * ot) as u8)), egui::StrokeKind::Inside);
        let sv_resp = ui.interact(sv.expand(4.0), Id::new("cp_sv"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::Crosshair);
        let mut changed = false;
        if let Some(pt) = sv_resp.interact_pointer_pos().filter(|_| sv_resp.dragged() || sv_resp.clicked() || sv_resp.drag_started()) {
            cp.hsv[1] = ((pt.x - sv.left()) / sv.width()).clamp(0.0, 1.0);
            cp.hsv[2] = 1.0 - ((pt.y - sv.top()) / sv.height()).clamp(0.0, 1.0);
            changed = true;
        }

        // Hue strip.
        let hue = Rect::from_min_size(pos2(sv.left(), sv.bottom() + 16.0), vec2(sv.width(), 12.0));
        let mut mesh = Mesh::default();
        let n = 24;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let c = c32(hsv_to_rgb([t, 1.0, 1.0])).gamma_multiply(ot);
            let xx = hue.left() + 6.0 + t * (hue.width() - 12.0);
            mesh.colored_vertex(pos2(xx, hue.top()), c);
            mesh.colored_vertex(pos2(xx, hue.bottom()), c);
            if i > 0 {
                let a = (i as u32 - 1) * 2;
                mesh.add_triangle(a, a + 1, a + 2);
                mesh.add_triangle(a + 1, a + 3, a + 2);
            }
        }
        let red = c32([255, 0, 0]).gamma_multiply(ot);
        p.circle_filled(pos2(hue.left() + 6.0, hue.center().y), 6.0, red);
        p.circle_filled(pos2(hue.right() - 6.0, hue.center().y), 6.0, red);
        p.add(Shape::mesh(mesh));
        let hue_resp = ui.interact(hue.expand2(vec2(4.0, 8.0)), Id::new("cp_hue"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
        if let Some(pt) = hue_resp.interact_pointer_pos().filter(|_| hue_resp.dragged() || hue_resp.clicked() || hue_resp.drag_started()) {
            cp.hsv[0] = ((pt.x - hue.left() - 6.0) / (hue.width() - 12.0)).clamp(0.0, 0.9999);
            changed = true;
        }

        // Ready colours.
        let sy = hue.bottom() + 26.0;
        let step = (sv.width() - 22.0) / 7.0;
        let mut picked = None;
        for (i, c) in SWATCHES[which].iter().enumerate() {
            let cc = pos2(sv.left() + 11.0 + i as f32 * step, sy);
            let id = Id::new(("cp_sw", which, i));
            let r = ui.interact(Rect::from_center_size(cc, Vec2::splat(24.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let hv = ctx.animate_bool_with_time(id.with("h"), r.hovered(), 0.12);
            p.circle_filled(cc, 10.0 + 2.0 * hv, c32(*c).gamma_multiply(ot));
            p.circle_stroke(cc, 10.0 + 2.0 * hv, Stroke::new(1.0_f32, theme.text().gamma_multiply(0.3 * ot)));
            if r.clicked() {
                picked = Some(*c);
            }
        }

        // Hex code.
        let hex_r = Rect::from_min_size(pos2(sv.left(), sy + 24.0), vec2(sv.width(), 38.0));
        let mut hex = std::mem::take(&mut cp.hex);
        let hresp = field(ui, &mut els, ppp, hex_r, &mut hex, "#RRGGBB", false, true, theme);
        let typed = if hresp.changed() { parse_hex(&hex) } else { None };
        cp.hex = hex;

        // The knobs.
        let cur = hsv_to_rgb(cp.hsv);
        let kp = pos2(sv.left() + cp.hsv[1] * sv.width(), sv.top() + (1.0 - cp.hsv[2]) * sv.height());
        let kd = ctx.animate_bool_with_time(Id::new("cp_sv_d"), sv_resp.dragged(), 0.1);
        p.circle_filled(kp + vec2(0.0, 1.0), 9.0 + 2.0 * kd, Color32::from_black_alpha((60.0 * ot) as u8));
        p.circle_filled(kp, 8.0 + 2.0 * kd, c32(cur).gamma_multiply(ot));
        p.circle_stroke(kp, 8.0 + 2.0 * kd, Stroke::new(2.0_f32, Color32::WHITE.gamma_multiply(ot)));
        let hp = pos2(hue.left() + 6.0 + cp.hsv[0] * (hue.width() - 12.0), hue.center().y);
        let hd = ctx.animate_bool_with_time(Id::new("cp_hue_d"), hue_resp.dragged(), 0.1);
        p.circle_filled(hp + vec2(0.0, 1.0), 10.0 + 2.0 * hd, Color32::from_black_alpha((60.0 * ot) as u8));
        p.circle_filled(hp, 9.0 + 2.0 * hd, c32(hsv_to_rgb([cp.hsv[0], 1.0, 1.0])).gamma_multiply(ot));
        p.circle_stroke(hp, 9.0 + 2.0 * hd, Stroke::new(2.0_f32, Color32::WHITE.gamma_multiply(ot)));
        *animating |= (kd > 0.0 && kd < 1.0) || (hd > 0.0 && hd < 1.0);

        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        self.set_frame(ui, slot, full, frame, &els);

        // Apply.
        let Some(cp) = self.color_pick.as_mut() else { return };
        let mut new = None;
        if let Some(c) = picked.or(typed) {
            cp.hsv = rgb_to_hsv(c);
            new = Some(c);
            if picked.is_some() {
                cp.hex = hex_of(c);
            }
        } else if changed {
            let c = hsv_to_rgb(cp.hsv);
            cp.hex = hex_of(c);
            new = Some(c);
        }
        if let Some(c) = new {
            if picked.is_some() {
                self.sound.click();
            }
            match which {
                0 => self.custom.background = c,
                1 => self.custom.accent = c,
                _ => self.custom.text = c,
            }
            self.custom_changed(&ctx, false);
        }
        let inside = back.interact_pointer_pos().is_some_and(|p| panel.contains(p));
        if (back.clicked() && !inside) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.color_pick = None;
            self.custom.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(parse_hex("#96f"), Some([0x99, 0x66, 0xff]));
        assert_eq!(parse_hex("1a2B3c"), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(hex_of([0x1a, 0x2b, 0x3c]), "#1A2B3C");
    }

    #[test]
    fn hsv_round_trips() {
        for c in [[150u8, 130, 255], [22, 20, 34], [255, 0, 0], [0, 255, 128], [240, 240, 245], [0, 0, 0]] {
            assert_eq!(hsv_to_rgb(rgb_to_hsv(c)), c);
        }
    }
}
