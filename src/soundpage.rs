//! The sound tabs in Settings: Playback (crossfade and how server songs are
//! streamed), Equalizer (preamp, ten bands, fine tuning of the picked band's
//! gain and width) and Effects (each with a switch and its own sliders).

use egui::{pos2, vec2, Align2, CursorIcon, Id, Rect, Sense, Stroke, Ui};

use crate::app::Starlite;
use crate::fx::{show, FxPrefs, Unit, CROSSFADE_MAX, EFFECTS, EQ_FREQS, EQ_GAIN, P, Q_DEFAULT, Q_MAX, Q_MIN};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::pages::toggle_row;
use crate::stream::{Format, BITRATES};
use crate::theme::Theme;
use crate::widgets::{font, line_slider, pill, text_trunc, Slide};

/// What the sound tabs remember while the app runs.
pub struct SoundUi {
    /// The equalizer slider being fine-tuned: 0 preamp, 1..=10 the bands.
    pub eq_sel: usize,
    tick_at: f64,
}

impl Default for SoundUi {
    fn default() -> Self {
        // Debug: STARLITE_EQ_SEL=0..10 starts with that slider picked.
        SoundUi { eq_sel: std::env::var("STARLITE_EQ_SEL").ok().and_then(|v| v.parse().ok()).filter(|&i| i <= 10).unwrap_or(6), tick_at: 0.0 }
    }
}

/// A row of choices in one glass strip, the picked one highlighted.
/// Returns the one that was clicked.
#[allow(clippy::too_many_arguments)]
fn segmented(ui: &Ui, els: &mut Vec<El>, ppp: f32, id: Id, at: egui::Pos2, labels: &[String], picked: usize, cell_w: f32, theme: Theme) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let strip = Rect::from_min_size(at, vec2(cell_w * labels.len() as f32 + 8.0, 38.0));
    els.push(El::rect(strip, ppp, 19.0, 1.0, 5.0, Kind::Glass, 0.0, 0.0));
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(strip.left() + 4.0 + i as f32 * cell_w, strip.top() + 4.0), vec2(cell_w, 30.0));
        let resp = ui.interact(r, id.with(i), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let c = if i == picked || resp.hovered() { theme.text() } else { theme.text_dim() };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font(13.0, "semibold"), c);
        if resp.clicked() && i != picked {
            clicked = Some(i);
        }
    }
    let l = ctx.animate_value_with_time(id.with("l"), strip.left() + 4.0 + picked as f32 * cell_w, 0.16);
    els.push(El::rect(Rect::from_min_size(pos2(l, strip.top() + 4.0), vec2(cell_w, 30.0)), ppp, 15.0, 1.0, 2.0, Kind::Layered, 0.25, 0.0));
    clicked
}

fn crossfade_text(v: f32) -> String {
    if v < 0.05 {
        "Off (gapless)".to_string()
    } else if (v - v.round()).abs() < 0.05 {
        format!("{v:.0} s")
    } else {
        format!("{v:.1} s")
    }
}

/// Short band names under the sliders.
fn band_label(i: usize) -> String {
    let f = EQ_FREQS[i];
    if f >= 1000.0 {
        format!("{:.0}k", f / 1000.0)
    } else {
        format!("{f:.0}")
    }
}

impl Starlite {
    /// The sound settings changed: the engine takes them at once.
    pub fn fx_changed(&mut self) {
        self.audio.set_fx(&self.fx);
    }

    /// A soft tick while a slider moves (not more than about 45 a second).
    fn slider_tick(&mut self, at: f32) {
        if self.time - self.sound_ui.tick_at > 0.022 {
            self.sound_ui.tick_at = self.time;
            self.sound.tick(at.clamp(0.0, 1.0));
        }
    }

    /// Playback: crossfade, and how songs come from Navidrome / Jellyfin.
    #[allow(clippy::too_many_arguments)]
    pub fn playback_settings(&mut self, ui: &mut Ui, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, w: f32) {
        let theme = self.theme;
        let p = ui.painter().clone();
        let (fg, dim) = (theme.text(), theme.text_dim());

        // Crossfade.
        p.text(pos2(x0, top + 10.0), Align2::LEFT_CENTER, "Crossfade", font(14.0, "semibold"), fg);
        let s = Slide { min: 0.0, max: CROSSFADE_MAX, neutral: 0.0, step: 0.5, marks: 2.0 };
        let o = line_slider(ui, Id::new("crossfade"), pos2(x0, top + 42.0), pos2(x0 + w, top + 42.0), self.fx.crossfade, &s, fg, dim, true);
        p.text(pos2(x0 + w, top + 10.0), Align2::RIGHT_CENTER, crossfade_text(o.value), font(13.0, "semibold"), dim.lerp_to_gamma(fg, o.hot));
        if o.changed {
            self.fx.crossfade = o.value;
            self.fx_changed();
            self.slider_tick(o.value / CROSSFADE_MAX);
        }
        if o.released {
            self.fx.save();
        }
        let g = p.layout("Songs fade into each other for this long. Off: the next song starts the instant the last one ends, with no gap. YouTube songs always start on their own.".to_string(), font(12.5, "regular"), dim, w);
        let gh = g.size().y;
        p.galley(pos2(x0, top + 62.0), g, dim);

        // Server songs.
        let y = top + 62.0 + gh + 30.0;
        p.text(pos2(x0, y), Align2::LEFT_TOP, "Songs from Navidrome and Jellyfin", font(13.0, "semibold"), dim);
        let mut st = self.stream;
        if toggle_row(ui, Rect::from_min_size(pos2(x0, y + 26.0), vec2(w, 52.0)), Id::new("transcode_on"), "Convert on the server", Some("Off: songs arrive as they are stored, in full quality"), &mut st.transcode, theme) {
            self.sound.click();
        }
        if st.transcode {
            let y = y + 96.0;
            p.text(pos2(x0, y + 19.0), Align2::LEFT_CENTER, "Format", font(14.0, "semibold"), fg);
            let labels: Vec<String> = Format::ALL.iter().map(|f| f.label().to_string()).collect();
            let picked = Format::ALL.iter().position(|f| *f == st.format).unwrap_or(0);
            if let Some(i) = segmented(ui, els, ppp, Id::new("tc_format"), pos2(x0 + 90.0, y), &labels, picked, 70.0, theme) {
                self.sound.click();
                st.format = Format::ALL[i];
            }
            let y = y + 52.0;
            if st.format.lossless() {
                p.text(pos2(x0, y + 19.0), Align2::LEFT_CENTER, "Bitrate", font(14.0, "semibold"), dim.gamma_multiply(0.6));
                p.text(pos2(x0 + 90.0, y + 19.0), Align2::LEFT_CENTER, "FLAC keeps everything, so there's none to pick", font(12.5, "regular"), dim);
            } else {
                p.text(pos2(x0, y + 19.0), Align2::LEFT_CENTER, "Bitrate", font(14.0, "semibold"), fg);
                let labels: Vec<String> = BITRATES.iter().map(|b| b.to_string()).collect();
                let picked = BITRATES.iter().position(|b| *b == st.bitrate).unwrap_or(3);
                let cell = ((w - 90.0 - 8.0 - 62.0) / BITRATES.len() as f32).clamp(44.0, 58.0);
                if let Some(i) = segmented(ui, els, ppp, Id::new("tc_bitrate"), pos2(x0 + 90.0, y), &labels, picked, cell, theme) {
                    self.sound.click();
                    st.bitrate = BITRATES[i];
                }
                p.text(pos2(x0 + 90.0 + cell * BITRATES.len() as f32 + 20.0, y + 19.0), Align2::LEFT_CENTER, "kbit/s", font(12.5, "regular"), dim);
            }
            let note = "The server does the converting, so it needs this format set up (Navidrome: Settings \u{2192} Transcoding). Smaller streams load faster on slow connections.";
            let g = p.layout(note.to_string(), font(12.5, "regular"), dim, w);
            p.galley(pos2(x0, y + 54.0), g, dim);
        }
        if st != self.stream {
            self.stream = st;
            self.stream.save();
            // The song lined up next was asked for the old way.
            self.lined_up = None;
        }
    }

    /// Equalizer: its switch, the curve, preamp and ten band sliders, and
    /// fine tuning (gain and width) of the slider that's picked.
    #[allow(clippy::too_many_arguments)]
    pub fn equalizer_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, content: Rect) {
        let theme = self.theme;
        let p = ui.painter().clone();
        let before = self.fx;
        let mut fx = self.fx;
        let mut save = false;
        let pw = content.width().min(800.0);
        if toggle_row(ui, Rect::from_min_size(pos2(x0, top), vec2(pw.min(430.0), 52.0)), Id::new("eq_on"), "Equalizer", Some("For your own files and server songs (not YouTube)"), &mut fx.eq_on, theme) {
            self.sound.click();
            save = true;
        }
        let reset = Rect::from_min_size(pos2(x0 + pw - 104.0, top + 7.0), vec2(104.0, 38.0));
        if pill(ui, icons, els, ppp, Id::new("eq_reset"), reset, "Reset", Some("rotate-ccw"), false, false, theme).on_hover_text("All sliders back to the middle").clicked() {
            self.sound.click();
            fx.preamp = 0.0;
            fx.eq = FxPrefs::default().eq;
            save = true;
        }
        let (fg, dim) = if fx.eq_on { (theme.text(), theme.text_dim()) } else { (theme.text_dim(), theme.text_dim().gamma_multiply(0.75)) };
        let line = if fx.eq_on { theme.accent() } else { theme.text_dim() };

        let panel_h = (content.bottom() - (top + 68.0) - 78.0).clamp(250.0, 340.0);
        let panel = Rect::from_min_size(pos2(x0, top + 68.0), vec2(pw, panel_h));
        els.push(El::rect(panel, ppp, 24.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));

        // The curve: 20 Hz to 20 kHz, ±15 dB.
        let curve = Rect::from_min_max(pos2(panel.left() + 24.0, panel.top() + 18.0), pos2(panel.right() - 24.0, panel.top() + 18.0 + (panel_h * 0.27).min(86.0)));
        let cy = curve.center().y;
        p.line_segment([pos2(curve.left(), cy), pos2(curve.right(), cy)], Stroke::new(1.0_f32, dim.gamma_multiply(0.4)));
        let n = 120;
        let pts: Vec<egui::Pos2> = (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let f = 20.0 * 1000f32.powf(t);
                let db = crate::dsp::eq_response(&fx, f).clamp(-15.0, 15.0);
                pos2(curve.left() + t * curve.width(), cy - db / 15.0 * curve.height() * 0.5)
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, line)));

        // Preamp and the ten bands.
        let cw = (pw - 48.0) / 11.0;
        let (y_top, y_bot) = (curve.bottom() + 40.0, panel.bottom() - 40.0);
        let s = Slide { min: -EQ_GAIN, max: EQ_GAIN, neutral: 0.0, step: 0.5, marks: 6.0 };
        let mut tick = None;
        for k in 0..11 {
            let x = panel.left() + 24.0 + (k as f32 + 0.5) * cw;
            let v = if k == 0 { fx.preamp } else { fx.eq[k - 1].gain };
            let id = Id::new(("eq_band", k));
            let o = line_slider(ui, id, pos2(x, y_bot), pos2(x, y_top), v, &s, fg, dim, true);
            if o.changed {
                if k == 0 {
                    fx.preamp = o.value;
                } else {
                    fx.eq[k - 1].gain = o.value;
                }
                tick = Some((o.value + EQ_GAIN) / (2.0 * EQ_GAIN));
            }
            if o.changed || ui.ctx().is_being_dragged(id) || ui.rect_contains_pointer(Rect::from_min_max(pos2(x - cw * 0.5, y_top - 30.0), pos2(x + cw * 0.5, y_bot + 30.0))) && ui.input(|i| i.pointer.primary_pressed()) {
                self.sound_ui.eq_sel = k;
            }
            save |= o.released;
            let sel = self.sound_ui.eq_sel == k;
            let val = show(Unit::Db, o.value).replace(" dB", "");
            p.text(pos2(x, y_top - 18.0), Align2::CENTER_CENTER, val, font(11.5, "semibold"), if sel || o.hot > 0.5 { fg } else { dim });
            let name = if k == 0 { "Pre".to_string() } else { band_label(k - 1) };
            p.text(pos2(x, y_bot + 20.0), Align2::CENTER_CENTER, name, font(12.0, if sel { "bold" } else { "regular" }), if sel { line } else { dim });
        }
        // A hairline between the preamp and the bands.
        let sx = panel.left() + 24.0 + cw;
        p.line_segment([pos2(sx, y_top - 6.0), pos2(sx, y_bot + 6.0)], Stroke::new(1.0_f32, dim.gamma_multiply(0.3)));

        // Fine tuning of the picked slider.
        let k = self.sound_ui.eq_sel;
        let fy = panel.bottom() + 34.0;
        let title = if k == 0 { "Preamp".to_string() } else { show(Unit::Hz, EQ_FREQS[k - 1]) };
        p.text(pos2(x0, fy), Align2::LEFT_CENTER, title, font(15.0, "bold"), fg);
        let gx = x0 + 96.0;
        let gw = ((pw - 96.0 - 60.0) * 0.5 - 150.0).clamp(90.0, 230.0);
        p.text(pos2(gx, fy), Align2::LEFT_CENTER, "Gain", font(13.0, "regular"), dim);
        let fine = Slide { min: -EQ_GAIN, max: EQ_GAIN, neutral: 0.0, step: 0.1, marks: 6.0 };
        let v = if k == 0 { fx.preamp } else { fx.eq[k - 1].gain };
        let o = line_slider(ui, Id::new("eq_fine_gain"), pos2(gx + 44.0, fy), pos2(gx + 44.0 + gw, fy), v, &fine, fg, dim, true);
        if o.changed {
            if k == 0 {
                fx.preamp = o.value;
            } else {
                fx.eq[k - 1].gain = o.value;
            }
            tick = Some((o.value + EQ_GAIN) / (2.0 * EQ_GAIN));
        }
        save |= o.released;
        p.text(pos2(gx + 44.0 + gw + 14.0, fy), Align2::LEFT_CENTER, show(Unit::Db, o.value), font(13.0, "semibold"), dim.lerp_to_gamma(fg, o.hot));
        if k > 0 {
            let qx = gx + 44.0 + gw + 96.0;
            p.text(pos2(qx, fy), Align2::LEFT_CENTER, "Width (Q)", font(13.0, "regular"), dim);
            let qs = Slide { min: Q_MIN, max: Q_MAX, neutral: Q_DEFAULT, step: 0.05, marks: 0.0 };
            // Low Q is a wide band: the slider runs from narrow to wide.
            let o = line_slider(ui, Id::new("eq_fine_q"), pos2(qx + 78.0 + gw, fy), pos2(qx + 78.0, fy), fx.eq[k - 1].q, &qs, fg, dim, true);
            if o.changed {
                fx.eq[k - 1].q = o.value;
                tick = Some(1.0 - (o.value - Q_MIN) / (Q_MAX - Q_MIN));
            }
            save |= o.released;
            let kind = if o.value < 0.8 { "wide" } else if o.value > 2.5 { "narrow" } else { "medium" };
            text_trunc(&p, pos2(qx + 78.0 + gw + 14.0, fy), Align2::LEFT_CENTER, &format!("{:.2} \u{00b7} {kind}", o.value), font(13.0, "semibold"), dim.lerp_to_gamma(fg, o.hot), (x0 + pw - (qx + 78.0 + gw + 14.0)).max(40.0));
        }

        if fx != before {
            self.fx = fx;
            self.fx_changed();
        }
        if let Some(t) = tick {
            self.slider_tick(t);
        }
        if save {
            self.fx.save();
        }
    }

    /// Effects: a switch each; the sliders of the ones that are on show under them.
    pub fn effects_settings(&mut self, ui: &mut Ui, x0: f32, top: f32, content: Rect) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let (fg, dim) = (theme.text(), theme.text_dim());
        let w = content.width().min(580.0) - 14.0;
        let before = self.fx;
        let mut fx = self.fx;
        let (mut save, mut clicked, mut tick) = (false, false, None);
        let area = Rect::from_min_max(pos2(x0 - 10.0, top), pos2(content.right(), content.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        child.set_clip_rect(area);
        egui::ScrollArea::vertical().id_salt("effects_scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            // How far each effect's sliders are open (eased).
            let open: Vec<f32> = (0..EFFECTS.len()).map(|i| ctx.animate_bool_with_time(Id::new(("fx_open", i)), fx.on(i), 0.18)).collect();
            let row = 36.0;
            let total: f32 = 34.0 + EFFECTS.iter().zip(&open).map(|(e, t)| 58.0 + (e.params.len() as f32 * row + 10.0) * crate::app::smooth(*t)).sum::<f32>() + 20.0;
            let (all, _) = ui.allocate_exact_size(vec2(w + 10.0, total), Sense::hover());
            let left = all.left() + 10.0;
            ui.painter().text(pos2(left, all.top() + 8.0), Align2::LEFT_CENTER, "For your own files and server songs. YouTube songs play in YouTube's player and stay as they are.", font(12.5, "regular"), dim);
            let mut y = all.top() + 34.0;
            for (i, e) in EFFECTS.iter().enumerate() {
                let mut on = fx.on(i);
                if toggle_row(ui, Rect::from_min_size(pos2(left, y), vec2(w, 52.0)), Id::new(("fx_on", i)), e.name, Some(e.about), &mut on, theme) {
                    fx.set_on(i, on);
                    clicked = true;
                    save = true;
                }
                y += 58.0;
                let t = crate::app::smooth(open[i]);
                let h = (e.params.len() as f32 * row + 10.0) * t;
                if t > 0.01 {
                    let clip = Rect::from_min_max(pos2(left, y), pos2(left + w, y + h)).intersect(ui.clip_rect());
                    let p = ui.painter().with_clip_rect(clip);
                    let (fg, dim) = (fg.gamma_multiply(t), dim.gamma_multiply(t));
                    for (j, par) in e.params.iter().enumerate() {
                        let cy = y + 4.0 + (j as f32 + 0.5) * row;
                        if cy + 14.0 > y + h {
                            break;
                        }
                        p.text(pos2(left + 18.0, cy), Align2::LEFT_CENTER, par.label, font(13.0, "regular"), dim);
                        let neutral = if par.p == P::Widen { 1.0 } else { 0f32.clamp(par.min, par.max) };
                        let marks = match par.unit {
                            Unit::Db => 6.0,
                            Unit::Pan => 1.0,
                            _ => 0.0,
                        };
                        let s = Slide { min: par.min, max: par.max, neutral, step: par.step, marks };
                        let v = fx.get(par.p);
                        let o = line_slider(ui, Id::new(("fx_par", i, j)), pos2(left + 118.0, cy), pos2(left + w - 98.0, cy), v, &s, fg, dim, fx.on(i));
                        if o.changed {
                            fx.set(par.p, o.value);
                            tick = Some((o.value - par.min) / (par.max - par.min));
                        }
                        save |= o.released;
                        p.text(pos2(left + w, cy), Align2::RIGHT_CENTER, show(par.unit, o.value), font(13.0, "semibold"), dim.lerp_to_gamma(fg, o.hot));
                    }
                }
                y += h;
            }
        });
        if clicked {
            self.sound.click();
        }
        if fx != before {
            self.fx = fx;
            self.fx_changed();
        }
        if let Some(t) = tick {
            self.slider_tick(t);
        }
        if save {
            self.fx.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(crossfade_text(0.0), "Off (gapless)");
        assert_eq!(crossfade_text(4.0), "4 s");
        assert_eq!(crossfade_text(2.5), "2.5 s");
        assert_eq!((band_label(0), band_label(5), band_label(9)), ("31".to_string(), "1k".to_string(), "16k".to_string()));
    }
}
