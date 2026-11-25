//! Settings → Appearance: theme (with a small picture of each), your saved
//! looks, the panels (style, corners, opacity, blur, edges, shadows),
//! colour (accent, background), text and icons (font, text size, icon
//! lines) and, for the themes that move, their motion. Also the font list
//! and saving / switching looks.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::app::{smooth, Starlite};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::settings::{button, menu_button, row_h, row_hover, row_label, Group, PAD};
use crate::theme::{AccentFrom, Backdrop, CustomTheme, LookPrefs, SavedLook, Style, Theme};
use crate::widgets::{field, font, text_trunc, Slide};

/// The font list menu.
pub struct FontMenu {
    pub at: Pos2,
    pub open_t: f32,
}

/// Glass elements drawn inside the scrolling page, cut to what's visible.
pub fn clip_els(els: &mut Vec<El>, from: usize, clip: Rect, ppp: f32) {
    let tail: Vec<El> = els.drain(from..).collect();
    for mut e in tail {
        let r = Rect::from_center_size(pos2(e.center[0], e.center[1]) / ppp, vec2(e.half[0], e.half[1]) * 2.0 / ppp);
        let c = r.intersect(clip);
        if c.width() < 1.0 || c.height() < 1.0 {
            continue;
        }
        if c != r {
            e.center = [c.center().x * ppp, c.center().y * ppp];
            e.half = [c.width() * 0.5 * ppp, c.height() * 0.5 * ppp];
            e.radius = e.radius.min(c.height() * 0.5 * ppp);
        }
        els.push(e);
    }
}

/// The font families installed on this PC that have a regular face.
pub fn font_families() -> Vec<String> {
    let table = crate::win::font_table();
    let weights = [" bold", " italic", " light", " semibold", " semilight", " black", " thin", " medium", " oblique", " heavy", " extrabold", " extralight", " demibold", " condensed"];
    let mut out: Vec<String> = table
        .iter()
        .map(|(name, _)| {
            let n = name.split(" (").next().unwrap_or(name).trim();
            n.strip_suffix(" Regular").unwrap_or(n).to_string()
        })
        .filter(|n| !n.is_empty() && !n.contains(" & ") && !weights.iter().any(|w| n.to_lowercase().contains(w)))
        // Symbol and icon fonts have no letters to read.
        .filter(|n| {
            let l = n.to_lowercase();
            !["icon", "symbol", "wingdings", "webdings", "emoji", "mdl2", "marlett", "dings"].iter().any(|x| l.contains(x)) && n.chars().next().is_some_and(|c| c.is_uppercase())
        })
        .collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out.retain(|n| crate::css::pick_fonts(&table, n).is_some());
    out
}

/// A saved look equals what's on screen (`advanced`, from before, doesn't count).
fn same_look(l: &SavedLook, theme: Theme, custom: &CustomTheme, look: &LookPrefs) -> bool {
    let colours = |c: &CustomTheme| (c.background, c.accent, c.text);
    let open = |p: &LookPrefs| LookPrefs { advanced: false, ..p.clone() };
    l.theme == theme.key() && (theme != Theme::Custom || colours(&l.custom) == colours(custom)) && open(&l.look) == open(look)
}

impl Starlite {
    /// The theme cards: a small picture of each theme, its name under it.
    #[allow(clippy::too_many_arguments)]
    fn theme_cards(&mut self, ui: &mut Ui, x0: f32, top: f32, w: f32) -> f32 {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let gap = 12.0;
        let n = Theme::ALL.len();
        let cols = if w >= n as f32 * 84.0 + (n - 1) as f32 * gap { n } else { n.div_ceil(2) };
        let bw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
        let ph = (bw * 0.62).clamp(52.0, 74.0);
        let bh = ph + 30.0;
        let mut clicked = None;
        for (i, t) in Theme::ALL.into_iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            let card = Rect::from_min_size(pos2(x0 + c as f32 * (bw + gap), top + r as f32 * (bh + gap)), vec2(bw, bh));
            let id = Id::new(("theme", t.label()));
            let resp = ui.interact(card, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
            let selected = theme == t;
            let pr = Rect::from_min_size(card.min, vec2(bw, ph));
            theme_preview(&p, pr, t);
            let ring = if selected {
                Stroke::new(2.0_f32, theme.accent())
            } else {
                Stroke::new(1.0_f32, theme.hairline().lerp_to_gamma(theme.text_dim(), 0.6 * h))
            };
            p.rect_stroke(pr, CornerRadius::same(10), ring, egui::StrokeKind::Outside);
            text_trunc(&p, pos2(card.left() + 2.0, pr.bottom() + 15.0), Align2::LEFT_CENTER, t.label(), font(13.0, if selected { "semibold" } else { "regular" }), if selected || h > 0.5 { theme.text() } else { theme.text().gamma_multiply(0.82) }, bw - 4.0);
            if resp.clicked() && !selected {
                clicked = Some(t);
            }
        }
        if let Some(t) = clicked {
            self.sound.click();
            self.theme = t;
            t.save();
            self.apply_theme(&ctx);
        }
        let rows = n.div_ceil(cols) as f32;
        top + rows * bh + (rows - 1.0) * gap
    }

    /// Settings → Appearance.
    #[allow(clippy::too_many_arguments)]
    pub fn appearance_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, w: f32) -> f32 {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let mut look = self.look.clone();
        let mut save = false;

        // ── Theme ──
        p.text(pos2(x0 + 2.0, top + 7.0), Align2::LEFT_CENTER, "Theme", font(12.5, "semibold"), theme.text_dim());
        let mut y = self.theme_cards(ui, x0, top + 24.0, w) + 26.0;
        if self.theme == Theme::Custom {
            y = self.custom_theme_settings(ui, icons, els, ppp, x0, y, w) + 28.0;
        }

        // ── Saved looks ──
        y = self.looks_group(ui, icons, els, ppp, x0, y, w);
        if self.look != look {
            // A look was switched to: everything below is drawn again next frame.
            return y;
        }

        // ── Panels ──
        let mut g = Group::new(ui, els, theme, x0, y, w, Some("Panels"));
        let labels: Vec<&str> = Style::ALL.iter().map(|s| s.label()).collect();
        let picked = Style::ALL.iter().position(|s| *s == look.style).unwrap_or(0);
        let about = match look.style {
            Style::Simple => "Solid and flat",
            Style::Glass => "See-through, blurred behind",
            Style::Liquid => "Bends the light at the edges",
        };
        if let Some(i) = self.choice_row(ui, els, ppp, &mut g, "style", "Style", Some(about), &labels, picked, 96.0) {
            look.style = Style::ALL[i];
            save = true;
        }
        // A value that was never touched shows the style's own.
        let flat = look.style != Style::Liquid;
        let pct = |v: f32| format!("{:.0} %", v * 100.0);
        let rows: [(&str, &str, f32, Slide, &dyn Fn(f32) -> String); 5] = [
            ("round", "Corners", look.round.unwrap_or(if flat { 14.0 } else { 40.0 }), Slide { min: 0.0, max: 40.0, neutral: 0.0, step: 1.0, marks: 10.0 }, &|v: f32| if v >= 40.0 { "Round".to_string() } else { format!("{v:.0} pt") }),
            ("solid", "Opacity", look.solid.unwrap_or(1.0), Slide { min: 0.0, max: 3.0, neutral: 1.0, step: 0.05, marks: 1.0 }, &pct),
            ("blur", "Blur", look.blur.unwrap_or(16.0), Slide { min: 0.0, max: 48.0, neutral: 0.0, step: 1.0, marks: 16.0 }, &|v: f32| if v < 0.5 { "None".to_string() } else { format!("{v:.0} pt") }),
            ("outline", "Edges", look.outline.unwrap_or(1.0), Slide { min: 0.0, max: 3.0, neutral: 1.0, step: 0.05, marks: 1.0 }, &pct),
            ("shadow", "Shadows", look.shadow.unwrap_or(1.0), Slide { min: 0.0, max: 3.0, neutral: 1.0, step: 0.05, marks: 1.0 }, &pct),
        ];
        for (key, label, v, s, show) in rows {
            if key == "blur" && look.style == Style::Simple {
                // Nothing shows through solid panels.
                continue;
            }
            let (nv, changed, released) = self.slider_row(ui, &mut g, key, label, v, &s, show);
            if changed {
                let slot = match key {
                    "round" => &mut look.round,
                    "solid" => &mut look.solid,
                    "blur" => &mut look.blur,
                    "outline" => &mut look.outline,
                    _ => &mut look.shadow,
                };
                *slot = Some(nv);
            }
            save |= released;
        }
        y = g.end(els, ppp);

        // ── Colour ──
        let mut g = Group::new(ui, els, theme, x0, y, w, Some("Colour"));
        let opts = [AccentFrom::Theme, AccentFrom::Album, AccentFrom::Picked];
        let picked = opts.iter().position(|o| *o == look.accent_from).unwrap_or(0);
        let row_top = g.y;
        if let Some(i) = self.choice_row(ui, els, ppp, &mut g, "accent_from", "Accent", None, &["Theme", "Cover", "Custom"], picked, 78.0) {
            look.accent_from = opts[i];
            save = true;
        }
        if look.accent_from == AccentFrom::Picked {
            // The colour, left of the choices.
            let c = pos2(x0 + w - PAD - 3.0 * 78.0 - 8.0 - 26.0, row_top + 27.0);
            let r = Rect::from_center_size(c, Vec2::splat(28.0));
            let resp = ui.interact(r, Id::new("accent_swatch"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Pick the colour");
            p.circle_filled(c, 11.0, Color32::from_rgb(look.accent[0], look.accent[1], look.accent[2]));
            p.circle_stroke(c, 11.0, Stroke::new(1.0_f32, theme.text().gamma_multiply(0.35)));
            if resp.clicked() {
                self.sound.click();
                self.color_pick = Some(crate::customize::ColorPick::new(3, r, look.accent));
            }
        }
        let picked = if look.backdrop == Backdrop::Album { 1 } else { 0 };
        if let Some(i) = self.choice_row(ui, els, ppp, &mut g, "backdrop", "Background", Some("Cover: the playing song's colours"), &["Theme", "Cover"], picked, 78.0) {
            look.backdrop = if i == 1 { Backdrop::Album } else { Backdrop::Theme };
            save = true;
        }
        y = g.end(els, ppp);

        // ── Text and icons ──
        let mut g = Group::new(ui, els, theme, x0, y, w, Some("Text and icons"));
        let r = g.row(ui, theme, row_h(false));
        row_label(ui, theme, r, "Font", None, 120.0);
        let name = look.font.clone().unwrap_or_else(|| "Segoe UI".to_string());
        let fr = Rect::from_min_size(pos2(r.right() - 220.0, r.center().y - 15.0), vec2(220.0, 30.0));
        // Debug: STARLITE_FONT_MENU=1 opens the font list once.
        static FONT_MENU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let debug = std::env::var("STARLITE_FONT_MENU").is_ok() && !FONT_MENU.swap(true, std::sync::atomic::Ordering::Relaxed);
        if menu_button(ui, icons, els, ppp, Id::new("font_pick"), fr, &name, theme).on_hover_text("Any font installed on this PC").clicked() || debug {
            self.sound.click();
            if self.font_list.is_none() {
                self.font_list = Some(font_families());
            }
            self.font_menu = Some(FontMenu { at: fr.left_bottom() + vec2(0.0, 6.0), open_t: 0.0 });
        }
        let note = match &self.css_font {
            Some((f, false)) if look.font.as_ref() == Some(f) => Some(("Not installed", Color32::from_rgb(235, 90, 100))),
            _ if self.theme == Theme::Custom && self.css.font.is_some() => Some(("Set by your theme.css", theme.text_dim())),
            _ => None,
        };
        if let Some((t, c)) = note {
            text_trunc(&p, pos2(fr.left() - 12.0, r.center().y), Align2::RIGHT_CENTER, t, font(12.0, "regular"), c, (fr.left() - r.left() - 60.0).max(40.0));
        }
        let (v, c, r) = self.slider_row(ui, &mut g, "text", "Text size", look.text_size, &Slide { min: 0.85, max: 1.2, neutral: 1.0, step: 0.05, marks: 0.05 }, &pct);
        if c {
            look.text_size = v;
        }
        save |= r;
        let weight_name = |v: f32| if v < 1.6 { "Thin".to_string() } else if v > 2.4 { "Bold".to_string() } else if (v - 2.0).abs() < 0.05 { "Regular".to_string() } else { format!("{v:.2}") };
        let (v, c, r) = self.slider_row(ui, &mut g, "icons", "Icon lines", look.icon_weight, &Slide { min: 1.25, max: 2.75, neutral: 2.0, step: 0.25, marks: 0.25 }, &weight_name);
        if c {
            look.icon_weight = v;
        }
        save |= r;
        y = g.end(els, ppp);

        // ── Motion (the themes that move) ──
        if theme.animated() {
            let mut g = Group::new(ui, els, theme, x0, y, w, Some("Motion"));
            let speed = |v: f32| if v < 0.01 { "Still".to_string() } else { format!("{:.0} %", v * 100.0) };
            let (v, c, r) = self.slider_row(ui, &mut g, "speed", "Speed", look.motion_speed, &Slide { min: 0.0, max: 3.0, neutral: 1.0, step: 0.05, marks: 1.0 }, &speed);
            if c {
                look.motion_speed = v;
            }
            save |= r;
            let (v, c, r) = self.slider_row(ui, &mut g, "amount", "Amount", look.motion_amount, &Slide { min: 0.2, max: 2.0, neutral: 1.0, step: 0.05, marks: 0.2 }, &pct);
            if c {
                look.motion_amount = v;
            }
            save |= r;
            y = g.end(els, ppp);
        }

        self.commit_look(&ctx, look, save);
        y
    }

    /// Your saved looks: one row each, and a row to save the current one.
    #[allow(clippy::too_many_arguments)]
    fn looks_group(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, y: f32, w: f32) -> f32 {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let mut g = Group::new(ui, els, theme, x0, y, w, Some("Saved looks"));
        let mut apply: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let looks = self.saved_looks.clone();
        for (i, l) in looks.iter().enumerate() {
            let r = g.row(ui, theme, row_h(false));
            let id = Id::new(("look", i));
            let resp = ui.interact(r.expand2(vec2(PAD - 6.0, 0.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
            row_hover(ui, theme, r, h);
            let on = same_look(l, self.theme, &self.custom, &self.look);
            let dot = pos2(r.left() + 8.0, r.center().y);
            if l.theme == "custom" {
                p.circle_filled(dot, 7.0, Color32::from_rgb(l.custom.accent[0], l.custom.accent[1], l.custom.accent[2]));
            } else {
                icons.paint(ui, Theme::from_key(&l.theme).icon(), dot, 15.0, theme.text_dim());
            }
            text_trunc(&p, pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, &l.name, font(13.5, "regular"), theme.text(), r.width() - 150.0);
            let xc = pos2(r.right() - 8.0, r.center().y);
            let xr = ui.interact(Rect::from_center_size(xc, Vec2::splat(24.0)), id.with("x"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            if h > 0.01 || xr.hovered() {
                icons.paint(ui, "trash", xc, 14.0, if xr.hovered() { Color32::from_rgb(235, 90, 100) } else { theme.text_dim().gamma_multiply(h) });
            }
            if on {
                p.text(pos2(r.right() - 30.0, r.center().y), Align2::RIGHT_CENTER, "In use", font(12.5, "regular"), theme.text_dim());
            }
            if xr.on_hover_text("Delete this look").clicked() {
                remove = Some(i);
            } else if resp.clicked() && !on {
                apply = Some(i);
            }
        }
        // Save what's on screen as a new look.
        let r = g.row(ui, theme, row_h(false));
        if let Some(mut name) = self.look_name.clone() {
            let fr = Rect::from_min_size(pos2(r.left() - 6.0, r.center().y - 16.0), vec2((r.width() - 180.0).min(260.0), 32.0));
            let resp = field(ui, els, ppp, fr, &mut name, "Name this look", false, true, theme);
            if !resp.has_focus() && !resp.lost_focus() && name.is_empty() {
                resp.request_focus();
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cr = Rect::from_min_size(pos2(r.right() - 76.0, r.center().y - 15.0), vec2(76.0, 30.0));
            let sr = Rect::from_min_size(pos2(cr.left() - 82.0, r.center().y - 15.0), vec2(76.0, 30.0));
            let ok = button(ui, icons, els, ppp, Id::new("look_save"), sr, "Save", None, theme).clicked() || enter;
            let cancel = button(ui, icons, els, ppp, Id::new("look_cancel"), cr, "Cancel", None, theme).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            if ok && !name.trim().is_empty() {
                self.sound.click();
                self.save_current_look(name.trim());
                self.look_name = None;
            } else if cancel {
                self.sound.click();
                self.look_name = None;
            } else {
                self.look_name = Some(name);
            }
        } else {
            let id = Id::new("look_new");
            let resp = ui.interact(r.expand2(vec2(PAD - 6.0, 0.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            row_hover(ui, theme, r, ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.12));
            icons.paint(ui, "plus", pos2(r.left() + 8.0, r.center().y), 15.0, theme.accent());
            p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, "Save the current look\u{2026}", font(13.5, "regular"), theme.text());
            p.text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, "Theme, colours, picture and everything below", font(12.0, "regular"), theme.text_dim());
            if resp.clicked() {
                self.sound.click();
                self.look_name = Some(String::new());
            }
        }
        let end = g.end(els, ppp);
        if let Some(i) = remove {
            self.sound.click();
            let l = self.saved_looks.remove(i);
            if let Some(img) = &l.custom.image {
                let _ = std::fs::remove_file(img);
            }
            crate::theme::save_looks(&self.saved_looks);
            self.toast(format!("Deleted the look \u{201c}{}\u{201d}", l.name));
        } else if let Some(i) = apply {
            self.sound.click();
            let l = self.saved_looks[i].clone();
            self.apply_saved_look(&ctx, &l);
        }
        end
    }
}

/// The accent a theme has on its own.
fn own_accent(t: Theme) -> Color32 {
    match t {
        Theme::Cherry => Color32::from_rgb(255, 183, 197),
        Theme::Stars => Color32::from_rgb(220, 228, 255),
        Theme::Bubbles => Color32::from_rgb(168, 222, 255),
        Theme::Custom => {
            let c = CustomTheme::get().accent;
            Color32::from_rgb(c[0], c[1], c[2])
        }
        _ => t.text(),
    }
}

/// A tiny window in a theme's colours: sidebar, a few lines of text, the
/// accent, and what moves in it.
fn theme_preview(p: &egui::Painter, r: Rect, t: Theme) {
    let bg = t.background();
    let fg = t.text();
    let cr = CornerRadius::same(10);
    p.rect_filled(r, cr, bg);
    let p = p.with_clip_rect(r.shrink(1.0).intersect(p.clip_rect()));
    // What the theme is about.
    let s = r.width() / 100.0;
    match t {
        Theme::Cherry => {
            for (x, y, k) in [(0.62, 0.30, 1.0), (0.80, 0.62, 0.8), (0.48, 0.78, 0.7), (0.90, 0.22, 0.6)] {
                p.circle_filled(pos2(r.left() + r.width() * x, r.top() + r.height() * y), 3.2 * k * s, Color32::from_rgba_unmultiplied(255, 183, 197, 200));
            }
        }
        Theme::Stars => {
            for (x, y, k) in [(0.55, 0.25, 1.0), (0.72, 0.55, 0.7), (0.88, 0.30, 0.8), (0.64, 0.82, 0.6), (0.93, 0.75, 0.5), (0.45, 0.6, 0.5)] {
                p.circle_filled(pos2(r.left() + r.width() * x, r.top() + r.height() * y), 1.3 * k * s.max(0.9), Color32::from_rgba_unmultiplied(230, 235, 255, 220));
            }
        }
        Theme::Bubbles => {
            for (x, y, k) in [(0.62, 0.70, 1.0), (0.84, 0.36, 0.7), (0.50, 0.30, 0.5)] {
                p.circle_stroke(pos2(r.left() + r.width() * x, r.top() + r.height() * y), 7.0 * k * s, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(168, 222, 255, 170)));
            }
        }
        _ => {}
    }
    // The sidebar and a panel.
    let side = Rect::from_min_max(r.min, pos2(r.left() + r.width() * 0.28, r.bottom()));
    p.rect_filled(side, CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, if t.is_light() { Color32::from_black_alpha(10) } else { Color32::from_white_alpha(8) });
    for k in 0..3 {
        let yy = r.top() + 12.0 + k as f32 * 9.0;
        p.rect_filled(Rect::from_min_size(pos2(side.left() + 7.0, yy), vec2(side.width() * (0.62 - 0.1 * k as f32), 3.0)), CornerRadius::same(2), fg.gamma_multiply(if k == 0 { 0.55 } else { 0.25 }));
    }
    let x = side.right() + 9.0;
    p.rect_filled(Rect::from_min_size(pos2(x, r.top() + 12.0), vec2(r.width() * 0.34, 5.0)), CornerRadius::same(2), fg.gamma_multiply(0.8));
    for k in 0..2 {
        p.rect_filled(Rect::from_min_size(pos2(x, r.top() + 24.0 + k as f32 * 9.0), vec2(r.width() * (0.42 - 0.12 * k as f32), 3.0)), CornerRadius::same(2), fg.gamma_multiply(0.3));
    }
    p.circle_filled(pos2(x + 4.0, r.bottom() - 11.0), 4.0, own_accent(t));
    p.rect_filled(Rect::from_min_size(pos2(x + 13.0, r.bottom() - 12.5), vec2(r.width() * 0.24, 3.0)), CornerRadius::same(2), fg.gamma_multiply(0.3));
}

impl Starlite {
    /// The font menu: Segoe UI and every family installed.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_font_menu(&mut self, ui: &mut Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let Some(m) = self.font_menu.as_mut() else { return };
        m.open_t = (m.open_t + dt * 9.0).min(1.0);
        if m.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(m.open_t);
        let back = ui.interact(full, ui.id().with("font_back"), Sense::click());
        let (w, h) = (280.0, 360.0f32.min(full.height() - 40.0));
        let x = m.at.x.min(full.right() - w - 10.0);
        let y = if m.at.y + h + 10.0 > full.bottom() { (m.at.y - h - 52.0).max(10.0) } else { m.at.y };
        let panel = Rect::from_min_size(pos2(x, y - 6.0 * (1.0 - ot)), vec2(w, h));
        let els = vec![El::rect(panel, ppp, 18.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let list = self.font_list.clone().unwrap_or_default();
        let current = self.look.font.clone();
        let mut picked: Option<Option<String>> = None;
        let inner = panel.shrink(8.0);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical().id_salt("font_list").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let rw = ui.available_width();
            let mut ticks = Vec::new();
            for (i, name) in std::iter::once(None).chain(list.iter().map(Some)).enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(rw, 34.0), Sense::click());
                let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                let on = name == current.as_ref();
                if resp.hovered() || on {
                    let c = if theme.is_light() { Color32::from_black_alpha(if on { 18 } else { 10 }) } else { Color32::from_white_alpha(if on { 20 } else { 12 }) };
                    ui.painter().rect_filled(r, egui::CornerRadius::same(9), c);
                }
                let label = name.cloned().unwrap_or_else(|| "Segoe UI".to_string());
                text_trunc(ui.painter(), pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, &label, font(13.5, if on { "semibold" } else { "regular" }), theme.text().gamma_multiply(ot), rw - 40.0);
                if on {
                    ticks.push(pos2(r.right() - 16.0, r.center().y));
                }
                if resp.clicked() {
                    picked = Some(name.cloned());
                }
                let _ = i;
            }
            for t in ticks {
                icons.paint(ui, "check", t, 14.0, theme.accent());
            }
        });
        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        self.set_frame(ui, slot, full, frame, &els);
        if let Some(f) = picked {
            self.sound.click();
            self.look.font = f;
            self.look.save();
            self.refresh_fonts(&ctx);
            self.font_menu = None;
        } else if ((back.clicked() || back.secondary_clicked()) && !back.interact_pointer_pos().is_some_and(|q| panel.contains(q))) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.font_menu = None;
        }
    }

    /// Saves what's on screen as a look (a look with that name is replaced).
    pub fn save_current_look(&mut self, name: &str) {
        let mut custom = self.custom.clone();
        // Its own copy of the picture, so changing the picture later doesn't take it away.
        custom.image = self.custom.image.as_deref().filter(|p| p.exists()).and_then(crate::theme::import_bg_image);
        let css = std::fs::read_to_string(Self::css_file()).ok().filter(|t| !t.trim().is_empty());
        let look = SavedLook { name: name.to_string(), theme: self.theme.key().to_string(), custom, look: LookPrefs { advanced: false, ..self.look.clone() }, css };
        if let Some(i) = self.saved_looks.iter().position(|l| l.name.eq_ignore_ascii_case(name)) {
            let old = std::mem::replace(&mut self.saved_looks[i], look);
            if let Some(img) = old.custom.image {
                let _ = std::fs::remove_file(img);
            }
        } else {
            self.saved_looks.push(look);
        }
        crate::theme::save_looks(&self.saved_looks);
        self.toast(format!("Saved the look \u{201c}{name}\u{201d}"));
    }

    /// Switches to a saved look.
    pub fn apply_saved_look(&mut self, ctx: &egui::Context, l: &SavedLook) {
        self.theme = Theme::from_key(&l.theme);
        self.theme.save();
        // The picture: a fresh copy (the current one is only the current one's).
        if let Some(old) = self.custom.image.take() {
            let _ = std::fs::remove_file(old);
        }
        let mut custom = l.custom.clone();
        custom.image = l.custom.image.as_deref().filter(|p| p.exists()).and_then(crate::theme::import_bg_image);
        self.custom = custom;
        self.custom.save();
        let advanced = self.look.advanced;
        self.look = LookPrefs { advanced, ..l.look.clone() }.clamped();
        self.look.save();
        // The theme's CSS goes with the look; one it didn't have is kept aside, not lost.
        let f = Self::css_file();
        match &l.css {
            Some(t) => {
                let _ = std::fs::write(&f, t);
            }
            None if f.exists() => {
                let _ = std::fs::rename(&f, f.with_file_name("theme.backup.css"));
            }
            None => {}
        }
        self.reload_css();
        self.custom_changed(ctx, false);
        self.refresh_fonts(ctx);
        self.apply_theme(ctx);
        self.toast(format!("Look \u{201c}{}\u{201d}", l.name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_look_matches_until_something_changes() {
        let custom = CustomTheme::default();
        let look = LookPrefs::default();
        let saved = SavedLook { name: "Mine".into(), theme: "dark".into(), custom: custom.clone(), look: LookPrefs { advanced: true, ..look.clone() }, css: None };
        // The Advanced switch being open or shut doesn't make it different.
        assert!(same_look(&saved, Theme::Dark, &custom, &look));
        assert!(!same_look(&saved, Theme::Light, &custom, &look));
        let rounder = LookPrefs { round: Some(30.0), ..look.clone() };
        assert!(!same_look(&saved, Theme::Dark, &custom, &rounder));
        // Custom colours only count for the Custom theme.
        let other = CustomTheme { accent: [1, 2, 3], ..custom.clone() };
        assert!(same_look(&saved, Theme::Dark, &other, &look));
    }

    #[test]
    fn cut_elements_stay_inside_the_page() {
        let ppp = 1.0;
        let mut els = vec![El::rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0)), ppp, 4.0, 1.0, 0.0, Kind::Glass, 0.0, 0.0)];
        els.push(El::rect(Rect::from_min_size(pos2(0.0, 90.0), vec2(100.0, 40.0)), ppp, 20.0, 1.0, 0.0, Kind::Glass, 0.0, 0.0));
        els.push(El::rect(Rect::from_min_size(pos2(0.0, 300.0), vec2(100.0, 40.0)), ppp, 20.0, 1.0, 0.0, Kind::Glass, 0.0, 0.0));
        clip_els(&mut els, 1, Rect::from_min_max(pos2(0.0, 100.0), pos2(200.0, 200.0)), ppp);
        assert_eq!(els.len(), 2);
        assert_eq!((els[1].center[1], els[1].half[1]), (115.0, 15.0));
    }
}
