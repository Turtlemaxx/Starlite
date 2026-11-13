//! The speed / bass dropdown that opens upwards from the player bar. Two
//! line sliders: a thin line, the part between "normal" and the value drawn
//! bolder, and a short upright line as the handle. Values glide when they
//! change, and dragging ticks softly (higher notes for higher values).
//! Double-click a slider to put it back to normal.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Stroke, Ui};

use crate::app::{smooth, Starlite};
use crate::fx::{BASS_MAX, BASS_MIN, SPEED_MAX, SPEED_MIN};
use crate::glass::{El, Kind};
use crate::widgets::font;

struct Spec {
    label: &'static str,
    min: f32,
    max: f32,
    neutral: f32,
    step: f32,
    /// Faint marks every this much.
    marks: f32,
}

const SPECS: [Spec; 2] = [
    Spec { label: "Speed", min: SPEED_MIN, max: SPEED_MAX, neutral: 1.0, step: 0.05, marks: 0.25 },
    Spec { label: "Bass", min: BASS_MIN, max: BASS_MAX, neutral: 0.0, step: 1.0, marks: 4.0 },
];

fn label_for(k: usize, v: f32) -> String {
    if k == 0 {
        format!("{v:.2}×")
    } else if v.abs() < 0.5 {
        "0 dB".to_string()
    } else {
        format!("{v:+.0} dB")
    }
}

/// Snap to the slider's step.
pub fn snap(k: usize, v: f32) -> f32 {
    let s = &SPECS[k];
    ((v / s.step).round() * s.step).clamp(s.min, s.max)
}

impl Starlite {
    #[allow(clippy::too_many_arguments)]
    pub fn draw_fx_popover(&mut self, ui: &mut Ui, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let yt = self.now_is_youtube();
        let Some(pop) = self.fx_pop.as_mut() else { return };
        pop.open_t = (pop.open_t + dt * 8.0).min(1.0);
        if pop.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(pop.open_t);
        let back = ui.interact(full, ui.id().with("fx_back"), Sense::click());

        let (w, h) = (300.0, 202.0);
        let max_x = self.card.map(|c| c.left() - w - 10.0).unwrap_or(full.right() - w - 10.0).max(nav_w + 10.0);
        let panel = Rect::from_min_size(pos2((pop.anchor.x - w * 0.5).clamp(nav_w + 10.0, max_x), pop.anchor.y - 30.0 - h + 10.0 * (1.0 - ot)), vec2(w, h));
        let els = vec![El::rect(panel, ppp, 20.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let p = ui.painter().clone();
        let (fg, dim) = (theme.text().gamma_multiply(ot), theme.text_dim().gamma_multiply(ot));

        let mut values = [self.fx.speed, self.fx.bass];
        let mut released = false;
        let mut ticks: Vec<f32> = Vec::new();
        for (k, spec) in SPECS.iter().enumerate() {
            let off = k == 1 && yt;
            let top = panel.top() + 22.0 + k as f32 * 72.0;
            let line = Rect::from_min_max(pos2(panel.left() + 22.0, top + 32.0), pos2(panel.right() - 22.0, top + 32.0));
            let x_of = |v: f32| line.left() + (v - spec.min) / (spec.max - spec.min) * line.width();
            let id = Id::new(("fx_slider", k));
            let resp = ui.interact(line.expand2(vec2(8.0, 14.0)), id, if off { Sense::hover() } else { Sense::click_and_drag() });
            let resp = if off { resp } else { resp.on_hover_cursor(CursorIcon::ResizeHorizontal) };
            if !off {
                if resp.double_clicked() {
                    values[k] = spec.neutral;
                    released = true;
                } else if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked() || resp.drag_started()) {
                    let raw = spec.min + ((pt.x - line.left()) / line.width()).clamp(0.0, 1.0) * (spec.max - spec.min);
                    values[k] = snap(k, raw);
                }
                if resp.drag_stopped() || resp.clicked() {
                    released = true;
                }
            }
            // Ticks as the value moves through its steps.
            if (values[k] - pop.ticked[k]).abs() > spec.step * 0.5 {
                pop.ticked[k] = values[k];
                ticks.push((values[k] - spec.min) / (spec.max - spec.min));
            }
            // The drawn value glides (faster while dragging).
            let speed = if resp.dragged() { 30.0 } else { 14.0 };
            pop.shown[k] += (values[k] - pop.shown[k]) * (dt * speed).min(1.0);
            if (pop.shown[k] - values[k]).abs() < spec.step * 0.002 {
                pop.shown[k] = values[k];
            } else {
                *animating = true;
            }
            let shown = pop.shown[k];
            let hv = ctx.animate_bool_with_time(id.with("h"), (resp.hovered() || resp.dragged()) && !off, 0.16);
            let dr = ctx.animate_bool_with_time(id.with("d"), resp.dragged(), 0.12);
            *animating |= (hv > 0.0 && hv < 1.0) || (dr > 0.0 && dr < 1.0);
            let k_off = if off { 0.4 } else { 1.0 };

            p.text(pos2(line.left(), top + 8.0), Align2::LEFT_CENTER, spec.label, font(13.5, "semibold"), fg.gamma_multiply(k_off));
            let value_txt = if off { "Not for YouTube songs".to_string() } else { label_for(k, shown) };
            p.text(pos2(line.right(), top + 8.0), Align2::RIGHT_CENTER, value_txt, font(13.0, "semibold"), if off { dim } else { lerp_c(dim, fg, hv) });

            // The line, its marks, the bold part from normal to the value, the handle.
            p.line_segment([line.left_center(), line.right_center()], Stroke::new(1.5_f32, dim.gamma_multiply(0.45 * k_off)));
            let mut m = (spec.min / spec.marks).ceil() * spec.marks;
            while m <= spec.max + 1e-4 {
                let x = x_of(m);
                let tall = if (m - spec.neutral).abs() < 1e-4 { 5.0 } else { 2.5 };
                p.line_segment([pos2(x, line.center().y - tall), pos2(x, line.center().y + tall)], Stroke::new(1.2_f32, dim.gamma_multiply(if tall > 3.0 { 0.8 } else { 0.4 } * k_off)));
                m += spec.marks;
            }
            if !off {
                let (a, b) = (x_of(spec.neutral), x_of(shown));
                p.line_segment([pos2(a.min(b), line.center().y), pos2(a.max(b), line.center().y)], Stroke::new(2.5 + 1.0 * hv, fg));
                let hh = 7.0 + 3.0 * hv + 2.0 * dr;
                if dr > 0.0 {
                    p.circle_filled(pos2(b, line.center().y), 12.0 * dr, fg.gamma_multiply(0.12 * dr));
                }
                p.line_segment([pos2(b, line.center().y - hh), pos2(b, line.center().y + hh)], Stroke::new(2.5 + 0.5 * hv, fg));
            }
        }

        // On to the equalizer and the effects (in Settings).
        let link = Rect::from_min_max(pos2(panel.left() + 12.0, panel.bottom() - 44.0), pos2(panel.right() - 12.0, panel.bottom() - 10.0));
        let lresp = ui.interact(link, Id::new("fx_more"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let lh = ctx.animate_bool_with_time(Id::new("fx_more_h"), lresp.hovered(), 0.14);
        p.line_segment([pos2(panel.left() + 22.0, link.top() - 2.0), pos2(panel.right() - 22.0, link.top() - 2.0)], Stroke::new(1.0_f32, dim.gamma_multiply(0.3)));
        p.text(pos2(panel.left() + 22.0, link.center().y), Align2::LEFT_CENTER, "Equalizer and effects", font(13.0, "semibold"), lerp_c(dim, fg, lh));
        p.text(pos2(panel.right() - 22.0, link.center().y - 1.0), Align2::RIGHT_CENTER, "›", font(17.0, "semibold"), lerp_c(dim, fg, lh));
        let more = lresp.clicked();

        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        self.set_frame(ui, slot, full, frame, &els);

        // Apply.
        if more {
            self.sound.click();
            self.fx.save();
            self.fx_pop = None;
            self.fullscreen = false;
            self.view = crate::app::View::Settings;
            self.settings_tab = 2;
            return;
        }
        if (values[0] - self.fx.speed).abs() > 1e-4 {
            self.fx.speed = values[0];
            self.audio.set_speed(values[0]);
            if let Some(p) = self.ytp.as_mut() {
                p.set_speed(values[0]);
            }
        }
        if (values[1] - self.fx.bass).abs() > 1e-4 {
            // The quick slider is the Bass effect: away from zero switches it on.
            self.fx.bass = values[1];
            self.fx.bass_on = values[1].abs() > 0.01;
            self.audio.set_fx(&self.fx);
        }
        let now = ctx.input(|i| i.time);
        if let (Some(&t), Some(pop)) = (ticks.last(), self.fx_pop.as_mut()) {
            if now - pop.tick_at > 0.022 {
                pop.tick_at = now;
                self.sound.tick(t);
            }
        }
        if released {
            self.fx.save();
        }
        let over_slider = back.interact_pointer_pos().is_some_and(|p| panel.contains(p));
        if (back.clicked() && !over_slider) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.fx.save();
            self.fx_pop = None;
        }
    }
}

fn lerp_c(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_snap_to_steps() {
        assert_eq!(snap(0, 1.234), 1.25);
        assert_eq!(snap(0, 9.0), SPEED_MAX);
        assert_eq!(snap(1, 3.4), 3.0);
        assert_eq!(snap(1, -40.0), BASS_MIN);
    }

    #[test]
    fn labels() {
        assert_eq!(label_for(0, 1.0), "1.00×");
        assert_eq!(label_for(1, 0.0), "0 dB");
        assert_eq!(label_for(1, 4.0), "+4 dB");
        assert_eq!(label_for(1, -3.0), "-3 dB");
    }
}
