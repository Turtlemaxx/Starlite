//! Settings: the categories in a list on the left, the picked one on the
//! right. Settings sit in groups of rows: what it is on the left, its
//! control on the right, a thin line between rows.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Response, Sense, Stroke, Ui};

use crate::app::Starlite;
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::services::SERVICES;
use crate::soundpage::segmented;
use crate::theme::Theme;
use crate::widgets::{font, line_slider, text_trunc, Slide};

pub const APPEARANCE: usize = 0;
pub const LAYOUT: usize = 1;
pub const MINI: usize = 2;
pub const PLAYBACK: usize = 3;
pub const EQUALIZER: usize = 4;
pub const EFFECTS: usize = 5;
/// Navidrome, Jellyfin, YouTube, Discord and Last.fm follow, in `SERVICES` order.
pub const SERVICE0: usize = 6;
pub const GENERAL: usize = SERVICE0 + SERVICES.len();
pub const COUNT: usize = GENERAL + 1;

/// Name and icon of a category.
fn entry(i: usize) -> (&'static str, &'static str) {
    match i {
        APPEARANCE => ("Appearance", "palette"),
        LAYOUT => ("Layout", "layout-panel-left"),
        MINI => ("Mini player", "picture-in-picture-2"),
        PLAYBACK => ("Playback", "headphones"),
        EQUALIZER => ("Equalizer", "sliders-vertical"),
        EFFECTS => ("Effects", "audio-lines"),
        GENERAL => ("General", "settings"),
        _ => SERVICES[i - SERVICE0],
    }
}

/// A small heading above some of the categories ("" is only a gap).
fn heading(i: usize) -> Option<&'static str> {
    match i {
        PLAYBACK => Some("Sound"),
        SERVICE0 => Some("Services"),
        GENERAL => Some(""),
        _ => None,
    }
}

/// Space inside a group, left and right.
pub const PAD: f32 = 16.0;

/// A row's height: with a second line or without.
pub fn row_h(sub: bool) -> f32 {
    if sub {
        56.0
    } else {
        46.0
    }
}

/// A group of rows on one panel.
pub struct Group {
    x: f32,
    pub w: f32,
    top: f32,
    pub y: f32,
    rows: usize,
    slot: usize,
}

impl Group {
    /// Starts a group at `y`; `title` is the small heading above it.
    pub fn new(ui: &Ui, els: &mut Vec<El>, theme: Theme, x: f32, y: f32, w: f32, title: Option<&str>) -> Group {
        let mut y = y;
        if let Some(t) = title {
            ui.painter().text(pos2(x + 2.0, y + 7.0), Align2::LEFT_CENTER, t, font(12.5, "semibold"), theme.text_dim());
            y += 24.0;
        }
        // The panel goes under everything in the group: its place is kept now.
        let slot = els.len();
        els.push(El::rect(Rect::from_min_size(pos2(x, y), vec2(w, 1.0)), 1.0, 0.0, 0.0, 0.0, Kind::Glass, 0.0, 0.0));
        Group { x, w, top: y, y, rows: 0, slot }
    }

    /// The next row, `h` tall, with a line between it and the one above.
    /// Returns the row without the padding.
    pub fn row(&mut self, ui: &Ui, theme: Theme, h: f32) -> Rect {
        if self.rows > 0 {
            ui.painter().hline(self.x + PAD..=self.x + self.w - PAD, self.y, Stroke::new(1.0_f32, line(theme)));
        }
        self.rows += 1;
        let r = Rect::from_min_size(pos2(self.x + PAD, self.y), vec2(self.w - 2.0 * PAD, h));
        self.y += h;
        r
    }

    /// A row that belongs to the one above (no line between them).
    pub fn sub_row(&mut self, h: f32) -> Rect {
        let r = Rect::from_min_size(pos2(self.x + PAD, self.y), vec2(self.w - 2.0 * PAD, h));
        self.y += h;
        r
    }

    /// The panel; returns where the next thing goes.
    pub fn end(self, els: &mut [El], ppp: f32) -> f32 {
        let r = Rect::from_min_max(pos2(self.x, self.top), pos2(self.x + self.w, self.y.max(self.top + 1.0)));
        els[self.slot] = El::rect(r, ppp, 12.0, 1.0, 3.0, Kind::Glass, 0.0, 0.0);
        self.y + 28.0
    }
}

/// The line between rows.
pub fn line(theme: Theme) -> Color32 {
    theme.hairline().gamma_multiply(if theme.is_light() { 0.7 } else { 0.75 })
}

/// A small note under a group (`y`: where the group ended). Returns where
/// the next thing goes.
pub fn note(ui: &Ui, theme: Theme, x: f32, y: f32, w: f32, text: &str) -> f32 {
    let g = ui.painter().layout(text.to_string(), font(12.0, "regular"), theme.text_dim(), w - 4.0);
    let h = g.size().y;
    ui.painter().galley(pos2(x + 2.0, y - 20.0), g, theme.text_dim());
    y - 20.0 + h + 26.0
}

/// A row's name, and the smaller line under it.
pub fn row_label(ui: &Ui, theme: Theme, r: Rect, label: &str, sub: Option<&str>, max_w: f32) {
    let p = ui.painter();
    match sub {
        Some(s) => {
            text_trunc(p, pos2(r.left(), r.center().y - 9.0), Align2::LEFT_CENTER, label, font(13.5, "regular"), theme.text(), max_w);
            text_trunc(p, pos2(r.left(), r.center().y + 10.0), Align2::LEFT_CENTER, s, font(12.0, "regular"), theme.text_dim(), max_w);
        }
        None => {
            text_trunc(p, pos2(r.left(), r.center().y), Align2::LEFT_CENTER, label, font(13.5, "regular"), theme.text(), max_w);
        }
    }
}

/// A faint highlight under a row the pointer is on.
pub fn row_hover(ui: &Ui, theme: Theme, r: Rect, h: f32) {
    if h > 0.001 {
        let c = if theme.is_light() { Color32::from_black_alpha((8.0 * h) as u8) } else { Color32::from_white_alpha((9.0 * h) as u8) };
        ui.painter().rect_filled(r.expand2(vec2(PAD - 6.0, -3.0)), CornerRadius::same(8), c);
    }
}

/// A row with a switch; the whole row flips it. Returns whether it flipped.
#[allow(clippy::too_many_arguments)]
pub fn switch_row(ui: &Ui, theme: Theme, g: &mut Group, id: Id, label: &str, sub: Option<&str>, on: &mut bool) -> bool {
    let r = g.row(ui, theme, row_h(sub.is_some()));
    let resp = ui.interact(r.expand2(vec2(PAD - 6.0, 0.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    row_hover(ui, theme, r, ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.12));
    row_label(ui, theme, r, label, sub, r.width() - 64.0);
    let flipped = resp.clicked();
    if flipped {
        *on = !*on;
    }
    crate::pages::switch(ui, id, pos2(r.right() - 20.0, r.center().y), *on, theme);
    flipped
}

/// A small button inside a group row (or anywhere on a panel).
#[allow(clippy::too_many_arguments)]
pub fn button(ui: &Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, id: Id, r: Rect, label: &str, icon: Option<&'static str>, theme: Theme) -> Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let ctx = ui.ctx();
    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
    let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.08);
    let rr = Rect::from_center_size(r.center(), r.size() + egui::Vec2::splat(3.0 * pr));
    els.push(El::rect(rr, ppp, 9.0, 1.0, 1.0 + h, Kind::Layered, 0.22 + 0.3 * h, pr));
    let p = ui.painter();
    let g = p.layout_no_wrap(label.to_owned(), font(13.0, "regular"), theme.text());
    let total = g.size().x + if icon.is_some() { 22.0 } else { 0.0 };
    let mut x = r.center().x - total * 0.5;
    if let Some(ic) = icon {
        icons.paint(ui, ic, pos2(x + 7.0, r.center().y), 14.0, theme.text());
        x += 22.0;
    }
    p.galley(pos2(x, r.center().y - g.size().y * 0.5), g, theme.text());
    resp
}

/// A button's width for its label.
pub fn button_w(ui: &Ui, label: &str, icon: bool) -> f32 {
    ui.painter().layout_no_wrap(label.to_owned(), font(13.0, "regular"), Color32::WHITE).size().x + 28.0 + if icon { 22.0 } else { 0.0 }
}

/// A button that opens a list: the current choice and a chevron.
#[allow(clippy::too_many_arguments)]
pub fn menu_button(ui: &Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, id: Id, r: Rect, label: &str, theme: Theme) -> Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let h = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
    els.push(El::rect(r, ppp, 9.0, 1.0, 1.0 + h, Kind::Layered, 0.22 + 0.3 * h, 0.0));
    text_trunc(ui.painter(), pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, label, font(13.0, "regular"), theme.text(), r.width() - 44.0);
    icons.paint(ui, "chevron-down", pos2(r.right() - 16.0, r.center().y), 14.0, theme.text_dim());
    resp
}

/// A plain text link (like "Reset"). Returns whether it was clicked.
pub fn link(ui: &Ui, id: Id, at: Pos2, align: Align2, label: &str, theme: Theme) -> bool {
    let g = ui.painter().layout_no_wrap(label.to_owned(), font(13.0, "regular"), theme.text());
    let r = align.anchor_size(at, g.size()).expand(4.0);
    let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let c = if resp.hovered() { theme.text() } else { theme.text_dim() };
    ui.painter().text(at, align, label, font(13.0, "regular"), c);
    resp.clicked()
}

impl Starlite {
    /// A row with a slider and its value on the right. Returns the value,
    /// whether it moved and whether the mouse let go.
    #[allow(clippy::too_many_arguments)]
    pub fn slider_row(&mut self, ui: &Ui, g: &mut Group, id: &str, label: &str, v: f32, s: &Slide, text: &dyn Fn(f32) -> String) -> (f32, bool, bool) {
        let theme = self.theme;
        let r = g.row(ui, theme, 46.0);
        let lw = (r.width() * 0.32).clamp(110.0, 170.0);
        row_label(ui, theme, r, label, None, lw - 10.0);
        let o = line_slider(ui, Id::new(("set_slider", id)), pos2(r.left() + lw, r.center().y), pos2(r.right() - 100.0, r.center().y), v, s, theme.text(), theme.text_dim(), true);
        let c = if o.hot > 0.5 { theme.text() } else { theme.text_dim() };
        ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, text(o.value), font(13.0, "regular"), c);
        if o.changed {
            self.slider_tick((o.value - s.min) / (s.max - s.min));
        }
        (o.value, o.changed, o.released)
    }

    /// A row of choices on the right. Returns the one that was picked.
    #[allow(clippy::too_many_arguments)]
    pub fn choice_row(&mut self, ui: &Ui, els: &mut Vec<El>, ppp: f32, g: &mut Group, id: &str, label: &str, sub: Option<&str>, choices: &[&str], picked: usize, cell: f32) -> Option<usize> {
        let theme = self.theme;
        let r = g.row(ui, theme, row_h(sub.is_some()).max(54.0));
        let sw = cell * choices.len() as f32 + 8.0;
        row_label(ui, theme, r, label, sub, r.width() - sw - 16.0);
        let labels: Vec<String> = choices.iter().map(|s| s.to_string()).collect();
        let c = segmented(ui, els, ppp, Id::new(("set_choice", id)), pos2(r.right() - sw, r.center().y - 19.0), &labels, picked, cell, theme);
        if c.is_some() {
            self.sound.click();
        }
        c
    }

    /// The Settings page.
    pub fn settings_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, content: Rect) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let x0 = content.left();
        if self.settings_tab >= COUNT {
            self.settings_tab = APPEARANCE;
        }
        if (SERVICE0..GENERAL).contains(&self.settings_tab) {
            self.service_sel = self.settings_tab - SERVICE0;
        }

        // â”€â”€ The categories â”€â”€
        let lw = 188.0;
        p.text(pos2(x0 + 10.0, 46.0), Align2::LEFT_CENTER, "Settings", font(22.0, "bold"), theme.text());
        let top = 84.0;
        let gaps = (0..COUNT).filter_map(heading).map(|h| if h.is_empty() { 14.0 } else { 30.0 }).sum::<f32>();
        let rh = ((content.bottom() - top - gaps - 8.0) / COUNT as f32).clamp(26.0, 34.0);
        let mut y = top;
        let mut picked = None;
        let mut hover: Vec<El> = Vec::new();
        let mut sel_y = top;
        for i in 0..COUNT {
            if let Some(h) = heading(i) {
                if h.is_empty() {
                    y += 14.0;
                } else {
                    y += 30.0;
                    p.text(pos2(x0 + 10.0, y - 9.0), Align2::LEFT_CENTER, h, font(12.0, "semibold"), theme.text_dim());
                }
            }
            let (name, icon) = entry(i);
            let r = Rect::from_min_size(pos2(x0, y), vec2(lw, rh));
            let id = Id::new(("settings_cat", i));
            let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let sel = self.settings_tab == i;
            if sel {
                sel_y = y;
            }
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered() && !sel, 0.12);
            if h > 0.001 {
                hover.push(El::rect(r, ppp, 8.0, h, 0.0, Kind::Layered, 0.2 * h, 0.0));
            }
            let logo = (SERVICE0..GENERAL).contains(&i);
            let tint = if logo || sel { theme.text() } else { theme.text_dim() };
            icons.paint(ui, icon, pos2(r.left() + 19.0, r.center().y), if logo { 16.0 } else { 15.0 }, tint);
            let tc = if sel { theme.text() } else { theme.text().gamma_multiply(0.82) };
            text_trunc(&p, pos2(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, name, font(13.5, if sel { "semibold" } else { "regular" }), tc, lw - 60.0);
            // Services that are set up get a small dot.
            if logo && self.service_status(i - SERVICE0).1 {
                p.circle_filled(pos2(r.right() - 14.0, r.center().y), 3.0, Color32::from_rgb(70, 190, 120));
            }
            if resp.clicked() && !sel {
                picked = Some(i);
            }
            y += rh;
        }
        els.extend(hover);
        let sy = ctx.animate_value_with_time(Id::new("settings_cat_y"), sel_y, 0.16);
        els.push(El::rect(Rect::from_min_size(pos2(x0, sy), vec2(lw, rh)), ppp, 8.0, 1.0, 1.0, Kind::Layered, 0.3, 0.0));
        if let Some(i) = picked {
            self.sound.click();
            self.settings_tab = i;
            if (SERVICE0..GENERAL).contains(&i) {
                self.service_sel = i - SERVICE0;
            }
        }

        // â”€â”€ The picked one â”€â”€
        let px = x0 + lw + 36.0;
        let pane = Rect::from_min_max(pos2(px, content.top()), content.max);
        let w = pane.width().min(680.0);
        let tab = self.settings_tab;
        p.text(pos2(px, 46.0), Align2::LEFT_CENTER, entry(tab).0, font(22.0, "bold"), theme.text());
        let body = 86.0;
        match tab {
            EQUALIZER => self.equalizer_settings(ui, icons, els, ppp, px, body, Rect::from_min_max(pos2(px, content.top()), pos2(px + w.max(560.0).min(pane.width()), content.bottom()))),
            t if (SERVICE0..GENERAL).contains(&t) => {
                let sw = w.min(580.0);
                match t - SERVICE0 {
                    0 => self.server_settings(ui, icons, els, ppp, px, body, sw, crate::stream::Server::Navidrome),
                    1 => self.server_settings(ui, icons, els, ppp, px, body, sw, crate::stream::Server::Jellyfin),
                    2 => {
                        self.youtube_settings(ui, icons, els, ppp, px, body, sw.min(520.0));
                    }
                    3 => self.discord_settings(ui, icons, els, ppp, px, body - 6.0, Rect::from_min_max(pos2(px, content.top()), pos2(px + sw, content.bottom()))),
                    _ => self.lastfm_settings(ui, icons, els, ppp, px, body - 6.0, sw),
                }
            }
            _ => {
                // Reset, for the pages that have one.
                let reset = match tab {
                    APPEARANCE => Some("Reset to defaults"),
                    LAYOUT => Some("Reset layout"),
                    _ => None,
                };
                if let Some(label) = reset {
                    if link(ui, Id::new(("settings_reset", tab)), pos2(px + w, 48.0), Align2::RIGHT_CENTER, label, theme) {
                        self.sound.click();
                        let mut look = self.look.clone();
                        if tab == APPEARANCE {
                            look.reset_look();
                            look.style = crate::theme::Style::default();
                        } else {
                            look.reset_layout();
                        }
                        self.commit_look(&ctx, look, true);
                    }
                }
                let area = Rect::from_min_max(pos2(px - 14.0, body - 14.0), pos2(content.right() + 8.0, content.bottom()));
                self.scroll_page(ui, icons, els, ppp, area, w, tab);
            }
        }
    }

    /// A settings page that scrolls when it's taller than the window.
    #[allow(clippy::too_many_arguments)]
    fn scroll_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, area: Rect, w: f32, tab: usize) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        child.set_clip_rect(area);
        let from = els.len();
        let height = self.settings_h[tab];
        let mut used = 0.0;
        let mut sa = egui::ScrollArea::vertical().id_salt(("settings_scroll", tab)).auto_shrink([false, false]);
        // Debug: STARLITE_SETTINGS_SCROLL=<points> starts the page scrolled down that far.
        static SCROLLED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if let Some(off) = std::env::var("STARLITE_SETTINGS_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
            if SCROLLED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
                sa = sa.vertical_scroll_offset(off);
            }
        }
        sa.show(&mut child, |ui| {
            let (all, _) = ui.allocate_exact_size(vec2(area.width() - 14.0, height.max(area.height() - 8.0)), Sense::hover());
            let (x, y) = (all.left() + 14.0, all.top() + 14.0);
            let end = match tab {
                APPEARANCE => self.appearance_page(ui, icons, els, ppp, x, y, w),
                LAYOUT => self.layout_page(ui, els, ppp, x, y, w),
                MINI => self.mini_page(ui, icons, els, ppp, x, y, w),
                PLAYBACK => self.playback_page(ui, els, ppp, x, y, w),
                EFFECTS => self.effects_page(ui, els, ppp, x, y, w),
                _ => self.general_page(ui, icons, els, ppp, x, y, w),
            };
            used = end - all.top();
        });
        crate::appearance::clip_els(els, from, area, ppp);
        self.settings_h[tab] = used + 24.0;
    }

    /// Takes a changed look: applies it, and saves it if asked.
    pub fn commit_look(&mut self, ctx: &egui::Context, look: crate::theme::LookPrefs, save: bool) {
        if look != self.look {
            let font_changed = look.font != self.look.font;
            self.look = look;
            self.look.apply();
            if font_changed {
                self.refresh_fonts(ctx);
            }
        }
        if save {
            self.look.save();
        }
    }

    /// Layout: song lists, the sidebar, the player bar, the CD shelf.
    #[allow(clippy::too_many_arguments)]
    fn layout_page(&mut self, ui: &mut Ui, els: &mut Vec<El>, ppp: f32, x: f32, y: f32, w: f32) -> f32 {
        use crate::theme::{RowSize, BAR_BUTTONS};
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let mut look = self.look.clone();
        let mut save = false;

        let mut g = Group::new(ui, els, theme, x, y, w, Some("Song lists"));
        let labels: Vec<&str> = RowSize::ALL.iter().map(|r| r.label()).collect();
        let picked = RowSize::ALL.iter().position(|r| *r == look.rows).unwrap_or(1);
        if let Some(i) = self.choice_row(ui, els, ppp, &mut g, "rows", "Row size", None, &labels, picked, 86.0) {
            look.rows = RowSize::ALL[i];
            save = true;
        }
        save |= switch_row(ui, theme, &mut g, Id::new("show_numbers"), "Song numbers", None, &mut look.show_numbers);
        save |= switch_row(ui, theme, &mut g, Id::new("show_album"), "Album name", Some("Next to the artist"), &mut look.show_album);
        let y = g.end(els, ppp);

        let mut g = Group::new(ui, els, theme, x, y, w, Some("Sidebar"));
        let (v, c, r) = self.slider_row(ui, &mut g, "nav_w", "Width", look.nav_width, &Slide { min: 196.0, max: 300.0, neutral: 232.0, step: 4.0, marks: 52.0 }, &|v| format!("{v:.0} pt"));
        if c {
            look.nav_width = v;
        }
        save |= r;
        save |= switch_row(ui, theme, &mut g, Id::new("nav_covers"), "Playlist covers", Some("Off: a small icon instead"), &mut look.nav_covers);
        let y = g.end(els, ppp);

        let mut g = Group::new(ui, els, theme, x, y, w, Some("Player bar"));
        for (key, label) in BAR_BUTTONS.iter().rev() {
            let mut shown = look.shows(key);
            if switch_row(ui, theme, &mut g, Id::new(("bar_btn", *key)), label, None, &mut shown) {
                if shown {
                    look.bar_hidden.retain(|b| b != key);
                } else {
                    look.bar_hidden.push(key.to_string());
                }
                save = true;
            }
        }
        let y = g.end(els, ppp);

        let mut g = Group::new(ui, els, theme, x, y, w, Some("Playlists"));
        let mut on = self.shelf.prefs.enabled;
        if switch_row(ui, theme, &mut g, Id::new("shelf_on"), "CD shelf", Some("Playlists as CD cases on a bookshelf"), &mut on) {
            self.shelf.prefs.enabled = on;
            self.shelf.prefs.save();
            self.sound.click();
        }
        let y = g.end(els, ppp);

        if save {
            self.sound.click();
        }
        self.commit_look(&ctx, look, save);
        y
    }

    /// General: the click sound, updates, files.
    #[allow(clippy::too_many_arguments)]
    fn general_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x: f32, y: f32, w: f32) -> f32 {
        use crate::sound::ClickSound;
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let mut look = self.look.clone();
        let mut save = false;
        let mut g = Group::new(ui, els, theme, x, y, w, Some("Sounds"));
        let r = g.row(ui, theme, 54.0);
        let cell = 74.0;
        let sw = cell * ClickSound::ALL.len() as f32 + 8.0;
        row_label(ui, theme, r, "Click sound", None, r.width() - sw - 16.0);
        let labels: Vec<String> = ClickSound::ALL.iter().map(|c| c.label().to_string()).collect();
        let picked = ClickSound::ALL.iter().position(|c| *c == look.click).unwrap_or(0);
        let mut choose_file = false;
        if let Some(i) = segmented(ui, els, ppp, Id::new("click_sound"), pos2(r.right() - sw, r.center().y - 19.0), &labels, picked, cell, theme) {
            let kind = ClickSound::ALL[i];
            if kind == ClickSound::Custom && look.click_file.as_deref().is_none_or(|f| !f.exists()) {
                choose_file = true;
            } else {
                match self.sound.set_click(kind, look.click_file.as_deref()) {
                    Ok(()) => {
                        look.click = kind;
                        save = true;
                        self.click_error = None;
                        // Let it be heard.
                        self.sound.click();
                    }
                    Err(e) => self.click_error = Some(e),
                }
            }
        }
        if look.click == ClickSound::Custom || choose_file {
            let r = g.row(ui, theme, row_h(true));
            let name = look.click_file.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "None yet".to_string());
            let bw = button_w(ui, "Choose\u{2026}", false);
            row_label(ui, theme, r, "Your sound", Some(&name), r.width() - bw - 16.0);
            let br = Rect::from_min_size(pos2(r.right() - bw, r.center().y - 15.0), vec2(bw, 30.0));
            if button(ui, icons, els, ppp, Id::new("click_file"), br, "Choose\u{2026}", None, theme).on_hover_text("A .wav (or .mp3 / .ogg); the first two seconds are used").clicked() {
                choose_file = true;
            }
        }
        let mut y = g.end(els, ppp);
        if let Some(e) = &self.click_error {
            text_trunc(ui.painter(), pos2(x + 2.0, y - 14.0), Align2::LEFT_CENTER, e, font(12.5, "regular"), Color32::from_rgb(235, 90, 100), w);
            y += 18.0;
        }
        if choose_file {
            self.sound.click();
            if let Some(src) = rfd::FileDialog::new().add_filter("Sound", &["wav", "mp3", "ogg"]).pick_file() {
                // A copy in Starlite's folder, so moving the original doesn't break it.
                let dir = crate::library::data_dir().join("sounds");
                let _ = std::fs::create_dir_all(&dir);
                let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("wav").to_ascii_lowercase();
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let dst = dir.join(format!("click-{t}.{ext}"));
                match crate::sound::load_click(&src).and_then(|_| std::fs::copy(&src, &dst).map_err(|e| format!("Couldn't copy it ({e})"))) {
                    Ok(_) => {
                        look.click_file = Some(dst.clone());
                        look.click = ClickSound::Custom;
                        let _ = self.sound.set_click(ClickSound::Custom, Some(&dst));
                        self.click_error = None;
                        save = true;
                        self.sound.click();
                    }
                    Err(e) => self.click_error = Some(e),
                }
            }
        }
        self.commit_look(&ctx, look, save);
        self.updates_group(ui, icons, els, ppp, x, y, w)
    }

    /// Mini player: on / off and how it behaves.
    #[allow(clippy::too_many_arguments)]
    fn mini_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x: f32, y: f32, w: f32) -> f32 {
        let theme = self.theme;
        let mut g = Group::new(ui, els, theme, x, y, w, None);
        let mut prefs = self.mini.prefs.clone();
        switch_row(ui, theme, &mut g, Id::new("mini_on"), "Mini player", Some("Shows at the edge of the screen while you're in another app"), &mut prefs.enabled);
        if prefs.enabled {
            switch_row(ui, theme, &mut g, Id::new("mini_only_playing"), "Only while music plays", Some("Off: also when the song is paused"), &mut prefs.only_when_playing);
            switch_row(ui, theme, &mut g, Id::new("mini_always"), "Always show the controls", Some("Off: they slide out when you point at it"), &mut prefs.always_open);
            switch_row(ui, theme, &mut g, Id::new("mini_cover_bg"), "Colours from the cover", Some("Off: dark glass"), &mut prefs.cover_bg);
            let r = g.row(ui, theme, row_h(true));
            let editing = self.mini.editing;
            row_label(ui, theme, r, "Position, size and buttons", Some(if editing { "Drag it where you want it, then press Done" } else { "Move it, resize it, pick its buttons" }), r.width() - 110.0);
            let label = if editing { "Done" } else { "Edit" };
            let bw = button_w(ui, label, false).max(72.0);
            if button(ui, icons, els, ppp, Id::new("mini_customize"), Rect::from_min_size(pos2(r.right() - bw, r.center().y - 15.0), vec2(bw, 30.0)), label, None, theme).clicked() {
                self.sound.click();
                self.mini.editing = !editing;
                if editing {
                    self.mini.prefs.save();
                }
            }
        }
        let end = g.end(els, ppp);
        if prefs != self.mini.prefs {
            self.sound.click();
            if !prefs.enabled {
                self.mini.editing = false;
            }
            self.mini.prefs = prefs;
            self.mini.prefs.save();
        }
        end
    }
}
