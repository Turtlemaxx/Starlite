//! Floating Liquid Glass player bar.
//!
//! Left: cover, title, artist (title links back to the song on YouTube).
//! Center: previous · play/pause · next.
//! Right: loop · like · add to playlist · download · volume.
//!
//! Motion: play ⇄ pause morph + pop, spinner while loading, skip kicks,
//! loop bounce, hover/press glass discs, and a volume knob that turns into a
//! clear lens magnifying the track while you use it.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, FontFamily, FontId, Id, Pos2, Rect, Response, Sense, Ui, Vec2};

use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::library::Track;
use crate::theme::Theme;
use crate::widgets::{cover, text_trunc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    Off,
    All,
    One,
}

impl Loop {
    pub fn next(self) -> Loop {
        match self {
            Loop::Off => Loop::All,
            Loop::All => Loop::One,
            Loop::One => Loop::Off,
        }
    }
    pub fn tooltip(self) -> &'static str {
        match self {
            Loop::Off => "Loop: off",
            Loop::All => "Loop: playlist",
            Loop::One => "Loop: this song",
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    fn step(&mut self, dt: f32, k: f32, c: f32) -> bool {
        let a = -self.x * k - self.v * c;
        self.v += a * dt;
        self.x += self.v * dt;
        if self.x.abs() < 0.001 && self.v.abs() < 0.01 {
            *self = Spring::default();
            false
        } else {
            true
        }
    }
}

/// What the bar shows this frame.
pub struct NowPlaying<'a> {
    pub track: Option<&'a Track>,
    pub playing: bool,
    pub loading: bool,
    pub liked: bool,
    /// Position and length in seconds (length 0 = unknown).
    pub pos: f64,
    pub dur: f64,
    pub fullscreen: bool,
    /// Speed or bass changed from normal (the sliders button lights up).
    pub fx_on: bool,
}

pub enum Action {
    TogglePlay,
    Prev,
    Next,
    Like,
    /// Open the add-to-playlist popover above this point.
    AddTo(Pos2),
    OpenLink(String),
    Volume(f32),
    /// Jump to this many seconds into the song.
    Seek(f64),
    ToggleFullscreen,
    /// Open the speed / bass dropdown above this point.
    Sound(Pos2),
}

pub struct Player {
    demo: bool,
    pub looping: Loop,
    pub volume: f32,
    pub muted: bool,
    morph: f32,
    vol_shown: f32,
    knob: f32,
    play_pop: Spring,
    prev_kick: Spring,
    next_kick: Spring,
    loop_pop: Spring,
    like_pop: Spring,
    /// Where the user is dragging the bar to (0..1) while scrubbing.
    scrub: Option<f32>,
    /// The bar's buttons last frame: presses that start on them never seek.
    controls: Vec<Rect>,
    /// The current press started on empty bar space (so it may seek).
    /// Remembered because egui forgets where a press began on release.
    seek_armed: bool,
    /// Time bubble fade (0..1) and where it was last shown.
    bubble_a: f32,
    bubble_x: f32,
    bubble_label: String,
}

pub struct Out {
    pub animating: bool,
    pub clicked: bool,
    pub actions: Vec<Action>,
}

fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A round button that grows a glass disc on hover (or always, if `base` > 0).
#[allow(clippy::too_many_arguments)]
pub fn round_button(ui: &Ui, id: Id, center: Pos2, d: f32, base: f32, glow: f32, ppp: f32, els: &mut Vec<El>, enabled: bool) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(d));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let mut resp = ui.interact(rect, id, sense);
    if enabled {
        resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    let ctx = ui.ctx();
    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered() && enabled, 0.16);
    let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on() && enabled, 0.09);
    let s = base.max(h);
    if s > 0.001 {
        let size = d * (0.82 + 0.18 * s) + 6.0 * pr;
        let r = Rect::from_center_size(center, Vec2::splat(size));
        els.push(El::rect(r, ppp, size * 0.5, s, 3.0, Kind::Layered, glow + 0.45 * h, pr));
    }
    resp
}

impl Player {
    pub fn new() -> Self {
        // Debug: STARLITE_PLAYER_DEMO=1 shows loop-one + the knob's lens state.
        let demo = std::env::var("STARLITE_PLAYER_DEMO").is_ok();
        Self {
            demo,
            looping: if demo { Loop::One } else { Loop::Off },
            volume: 0.7,
            // Debug: STARLITE_MUTE=1 starts muted (silent test runs).
            muted: std::env::var("STARLITE_MUTE").is_ok(),
            morph: 0.0,
            vol_shown: 0.7,
            knob: 0.0,
            play_pop: Spring::default(),
            prev_kick: Spring::default(),
            next_kick: Spring::default(),
            loop_pop: Spring::default(),
            like_pop: Spring::default(),
            scrub: None,
            controls: Vec::new(),
            seek_armed: false,
            bubble_a: 0.0,
            bubble_x: 0.0,
            bubble_label: String::new(),
        }
    }

    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ui(&mut self, ui: &Ui, icons: &mut Icons, bar: Rect, ppp: f32, dt: f32, theme: Theme, els: &mut Vec<El>, now: NowPlaying) -> Out {
        let mut out = Out { animating: false, clicked: false, actions: Vec::new() };
        let ctx = ui.ctx().clone();
        let id = Id::new("player");
        let cy = bar.center().y;
        let fg = theme.text();
        let dim = theme.text_dim();
        let accent = theme.accent();
        let has_track = now.track.is_some();

        // The whole bar is the seek bar: it fills with progress, and clicking
        // or dragging anywhere that isn't a button jumps in the song.
        let can_seek = has_track && now.dur > 0.0;
        let seek = ui.interact(bar, id.with("seek"), if can_seek { Sense::click_and_drag() } else { Sense::hover() });
        let frac_at = |x: f32| ((x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        // egui hands drags to the bar even when a button on top takes the
        // click, so only presses that start on empty bar space count.
        let on_controls = |p: Pos2, ctl: &[Rect]| ctl.iter().any(|r| r.expand2(vec2(14.0, 0.0)).contains(p));
        if ui.input(|i| i.pointer.any_pressed()) {
            self.seek_armed = ui
                .input(|i| i.pointer.press_origin())
                .is_some_and(|p| bar.contains(p) && !on_controls(p, &self.controls));
        }
        let mut ctl: Vec<Rect> = Vec::new();
        if can_seek && self.seek_armed {
            if let Some(p) = seek.interact_pointer_pos() {
                if seek.dragged() || seek.is_pointer_button_down_on() {
                    self.scrub = Some(frac_at(p.x));
                }
            }
            if seek.drag_stopped() || seek.clicked() {
                if let Some(f) = self.scrub.take().or_else(|| seek.interact_pointer_pos().map(|p| frac_at(p.x))) {
                    out.actions.push(Action::Seek(f as f64 * now.dur));
                }
            }
            if !seek.is_pointer_button_down_on() && !seek.dragged() {
                self.scrub = None;
            }
        } else {
            self.scrub = None;
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.seek_armed = false;
        }
        let seek_hover_ok = seek.hover_pos().is_some_and(|p| !on_controls(p, &self.controls));
        let progress = self.scrub.unwrap_or(if now.dur > 0.0 { (now.pos / now.dur) as f32 } else { 0.0 });
        let mut bar_el = El::rect(bar, ppp, 24.0, 1.0, 12.0, Kind::Glass, 0.0, 0.0).with_class(crate::glass::Class::Player);
        if can_seek {
            bar_el = bar_el.with_fill(progress);
        }
        els.push(bar_el);
        // Time bubble above the bar while hovering / dragging it. It fades in
        // and out (never pops) so moving along the bar doesn't blink.
        let show = can_seek && ((seek.hovered() && seek_hover_ok) || self.scrub.is_some());
        if show {
            let x = seek.hover_pos().or(seek.interact_pointer_pos()).map(|p| p.x).unwrap_or(bar.left() + bar.width() * progress);
            let at = self.scrub.map(|f| f as f64 * now.dur).unwrap_or(frac_at(x) as f64 * now.dur);
            self.bubble_x = x;
            self.bubble_label = format!("{} / {}", fmt_time(at), fmt_time(now.dur));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let ba = if show { 1.0 } else { 0.0 };
        self.bubble_a += (ba - self.bubble_a) * (dt * 16.0).min(1.0);
        if (self.bubble_a - ba).abs() < 0.01 {
            self.bubble_a = ba;
        } else {
            out.animating = true;
        }
        if self.bubble_a > 0.0 && can_seek && !self.bubble_label.is_empty() {
            let a = smoothstep01(self.bubble_a);
            let g = ui.painter().layout_no_wrap(self.bubble_label.clone(), FontId::new(12.5, FontFamily::Name("semibold".into())), fg);
            let w = g.size().x + 24.0;
            let r = Rect::from_center_size(pos2(self.bubble_x.clamp(bar.left() + w * 0.5, bar.right() - w * 0.5), bar.top() - 22.0 + 4.0 * (1.0 - a)), vec2(w, 28.0));
            els.push(El::rect(r, ppp, 14.0, a, 6.0, Kind::Glass, 0.3 * a, 0.0));
            ui.painter().galley(r.center() - g.size() * 0.5, g, fg.gamma_multiply(a));
        }

        // ── Transport (center) ──────────────────────────────────────
        // Centred, but pushed left when the bar is too narrow for the right
        // cluster (its leftmost button, speed & bass, sits 392 pt from the right end).
        let cx = bar.center().x.min(bar.right() - 392.0 - 17.0 - 12.0 - 82.0);
        let play_resp = round_button(ui, id.with("play"), pos2(cx, cy), 46.0, 1.0, 0.25, ppp, els, has_track);
        let prev_resp = round_button(ui, id.with("prev"), pos2(cx - 62.0, cy), 40.0, 0.0, 0.0, ppp, els, has_track);
        let next_resp = round_button(ui, id.with("next"), pos2(cx + 62.0, cy), 40.0, 0.0, 0.0, ppp, els, has_track);
        ctl.push(Rect::from_center_size(pos2(cx, cy), Vec2::splat(46.0)));
        ctl.push(Rect::from_center_size(pos2(cx - 62.0, cy), Vec2::splat(40.0)));
        ctl.push(Rect::from_center_size(pos2(cx + 62.0, cy), Vec2::splat(40.0)));
        if play_resp.clicked() {
            self.play_pop.v = 2.2;
            out.clicked = true;
            out.actions.push(Action::TogglePlay);
        }
        if prev_resp.clicked() {
            self.prev_kick.v = -90.0;
            out.clicked = true;
            out.actions.push(Action::Prev);
        }
        if next_resp.clicked() {
            self.next_kick.v = 90.0;
            out.clicked = true;
            out.actions.push(Action::Next);
        }

        let target = if now.playing { 1.0 } else { 0.0 };
        self.morph += (target - self.morph) * (dt * 14.0).min(1.0);
        if (self.morph - target).abs() < 0.002 {
            self.morph = target;
        } else {
            out.animating = true;
        }
        out.animating |= self.play_pop.step(dt, 380.0, 16.0);
        let pop = 1.0 + 0.08 * self.play_pop.x;
        let tint = if has_track { fg } else { dim };
        if now.loading {
            let t = ctx.input(|i| i.time) as f32;
            icons.paint_rotated(ui, "loader-circle", pos2(cx, cy), 20.0, fg, t * 5.0);
            out.animating = true;
        } else {
            let m = self.morph;
            let q = std::f32::consts::FRAC_PI_2;
            icons.paint_rotated(ui, "play", pos2(cx + 1.5 * (1.0 - m), cy), 20.0 * pop * (0.5 + 0.5 * (1.0 - m)), tint.gamma_multiply(1.0 - m), -q * m);
            icons.paint_rotated(ui, "pause", pos2(cx, cy), 20.0 * pop * (0.5 + 0.5 * m), tint.gamma_multiply(m), q * (1.0 - m));
        }
        out.animating |= self.prev_kick.step(dt, 260.0, 18.0);
        out.animating |= self.next_kick.step(dt, 260.0, 18.0);
        icons.paint(ui, "skip-back", pos2(cx - 62.0 + self.prev_kick.x, cy), 18.0, tint);
        icons.paint(ui, "skip-forward", pos2(cx + 62.0 + self.next_kick.x, cy), 18.0, tint);

        // ── Now playing (left) ──────────────────────────────────────
        let art = Rect::from_center_size(pos2(bar.left() + 12.0 + 23.0, cy), Vec2::splat(46.0));
        let tx = art.right() + 12.0;
        let text_w = (cx - 62.0 - 32.0 - tx).max(40.0);
        let p = ui.painter();
        match now.track {
            Some(t) => {
                cover(ui, icons, art, t.artwork.as_deref(), 10.0, "disc-3", theme);
                let title_r = text_trunc(p, pos2(tx, cy - 9.0), Align2::LEFT_CENTER, &t.title, FontId::new(14.0, FontFamily::Name("semibold".into())), fg, text_w);
                let sub = match t.source_name() {
                    Some(src) => format!("{} · {src}", t.artist),
                    None => t.artist.clone(),
                };
                text_trunc(p, pos2(tx, cy + 10.0), Align2::LEFT_CENTER, &sub, FontId::proportional(12.5), dim, text_w);
                if let Some(link) = &t.permalink {
                    ctl.push(title_r);
                    let r = ui.interact(title_r, id.with("title"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text(format!("Open on {}", t.source_name().unwrap_or("the web")));
                    if r.hovered() {
                        p.line_segment([title_r.left_bottom(), title_r.right_bottom()], egui::Stroke::new(1.0_f32, fg.gamma_multiply(0.6)));
                    }
                    if r.clicked() {
                        out.actions.push(Action::OpenLink(link.clone()));
                    }
                }
            }
            None => {
                els.push(El::rect(art, ppp, 10.0, 1.0, 2.0, Kind::Layered, 0.0, 0.0));
                icons.paint(ui, "disc-3", art.center(), 20.0, dim);
                p.text(pos2(tx, cy), Align2::LEFT_CENTER, "Not playing", FontId::proportional(13.5), dim);
            }
        }

        // ── Volume (right) ──────────────────────────────────────────
        let tw = 112.0;
        // Fullscreen Now Playing toggle (far right).
        let fs_c = pos2(bar.right() - 30.0, cy);
        ctl.push(Rect::from_center_size(fs_c, Vec2::splat(34.0)));
        let fsr = round_button(ui, id.with("fullscreen"), fs_c, 34.0, 0.0, 0.0, ppp, els, has_track || now.fullscreen)
            .on_hover_text(if now.fullscreen { "Exit full screen (Esc)" } else { "Full screen" });
        if fsr.clicked() {
            out.clicked = true;
            out.actions.push(Action::ToggleFullscreen);
        }
        icons.paint(ui, if now.fullscreen { "minimize-2" } else { "maximize-2" }, fs_c, 16.0, if has_track || now.fullscreen { fg } else { dim });

        let track = Rect::from_min_size(pos2(fs_c.x - 30.0 - tw, cy - 2.5), vec2(tw, 5.0));
        let hit = track.expand2(vec2(10.0, 12.0));
        ctl.push(hit);
        let tresp = ui.interact(hit, id.with("vol"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand);
        let before = self.effective_volume();
        if let Some(pt) = tresp.interact_pointer_pos() {
            if tresp.dragged() || tresp.clicked() {
                self.volume = ((pt.x - track.left()) / track.width()).clamp(0.0, 1.0);
                self.muted = false;
            }
        }
        if tresp.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.volume = (self.volume + scroll * 0.002).clamp(0.0, 1.0);
                self.muted = false;
            }
        }
        let vol = self.effective_volume();
        let follow = if tresp.dragged() { 30.0 } else { 14.0 };
        self.vol_shown += (vol - self.vol_shown) * (dt * follow).min(1.0);
        if (self.vol_shown - vol).abs() < 0.001 {
            self.vol_shown = vol;
        } else {
            out.animating = true;
        }
        let engaged = tresp.hovered() || tresp.dragged() || self.demo;
        let kt = if engaged { 1.0 } else { 0.0 };
        self.knob += (kt - self.knob) * (dt * 12.0).min(1.0);
        if (self.knob - kt).abs() < 0.002 {
            self.knob = kt;
        } else {
            out.animating = true;
        }
        els.push(El::rect(track, ppp, 2.5, 1.0, 0.0, Kind::Track, 0.0, 0.0).with_value(self.vol_shown));
        let k = self.knob;
        let knob = Rect::from_center_size(pos2(track.left() + track.width() * self.vol_shown, cy), vec2(16.0 + 20.0 * k, 16.0 + 8.0 * k));
        els.push(El::rect(knob, ppp, knob.height() * 0.5, 1.0, 3.0, Kind::Lens, 0.0, 0.0).with_value(k));

        let mute_c = pos2(track.left() - 24.0, cy);
        ctl.push(Rect::from_center_size(mute_c, Vec2::splat(34.0)));
        if round_button(ui, id.with("mute"), mute_c, 34.0, 0.0, 0.0, ppp, els, true).clicked() {
            self.muted = !self.muted;
            out.clicked = true;
        }
        let vsrc = if vol <= 0.001 { "volume-x" } else if vol < 0.5 { "volume-1" } else { "volume-2" };
        icons.paint(ui, vsrc, mute_c, 17.0, fg);
        if (self.effective_volume() - before).abs() > 1e-4 {
            out.actions.push(Action::Volume(self.effective_volume()));
        }

        // ── Download · add · like · loop ────────────────────────────
        let dl_c = pos2(mute_c.x - 44.0, cy);
        let add_c = pos2(dl_c.x - 38.0, cy);
        let like_c = pos2(add_c.x - 38.0, cy);
        let loop_c = pos2(like_c.x - 38.0, cy);
        let fx_c = pos2(loop_c.x - 38.0, cy);
        for c in [dl_c, add_c, like_c, loop_c, fx_c] {
            ctl.push(Rect::from_center_size(c, Vec2::splat(34.0)));
        }

        // YouTube's terms don't allow downloading its songs; local
        // files are already in Downloaded Music. (Navidrome downloads later.)
        let remote = now.track.and_then(|t| t.source_name());
        let dl = round_button(ui, id.with("dl"), dl_c, 34.0, 0.0, 0.0, ppp, els, false);
        let dl_tip = if remote.is_some() {
            "YouTube doesn't allow downloading songs in other apps"
        } else if has_track {
            "Already in Downloaded Music"
        } else {
            "Download"
        };
        dl.on_hover_text(dl_tip);
        let sc = remote.is_some();
        icons.paint(ui, if has_track && !sc { "check" } else { "download" }, dl_c, 17.0, dim.gamma_multiply(if has_track { 1.0 } else { 0.6 }));

        let add = round_button(ui, id.with("add"), add_c, 34.0, 0.0, 0.0, ppp, els, has_track).on_hover_text("Add to playlist");
        if add.clicked() {
            out.clicked = true;
            out.actions.push(Action::AddTo(add_c));
        }
        icons.paint(ui, "list-plus", add_c, 17.0, if has_track { fg } else { dim });

        let like = round_button(ui, id.with("like"), like_c, 34.0, 0.0, 0.0, ppp, els, has_track).on_hover_text(if now.liked { "Remove from Liked Music" } else { "Add to Liked Music" });
        if like.clicked() {
            self.like_pop.v = 3.5;
            out.clicked = true;
            out.actions.push(Action::Like);
        }
        out.animating |= self.like_pop.step(dt, 320.0, 13.0);
        let ls = 17.0 * (1.0 + 0.14 * self.like_pop.x);
        if now.liked {
            icons.paint(ui, "heart-fill", like_c, ls, if theme == Theme::Light { Color32::from_rgb(232, 64, 96) } else { accent });
        } else {
            icons.paint(ui, "heart", like_c, ls, if has_track { fg } else { dim });
        }

        let active = ctx.animate_bool_with_time(id.with("loop_on"), self.looping != Loop::Off, 0.2);
        let lr = round_button(ui, id.with("loop"), loop_c, 34.0, active, 0.2, ppp, els, true).on_hover_text(self.looping.tooltip());
        if lr.clicked() {
            self.looping = self.looping.next();
            self.loop_pop.v = 3.0;
            out.clicked = true;
        }
        out.animating |= self.loop_pop.step(dt, 320.0, 13.0);
        let lsize = 18.0 * (1.0 + 0.12 * self.loop_pop.x);
        let ltint = if self.looping == Loop::Off { dim } else { accent };
        icons.paint(ui, if self.looping == Loop::One { "repeat-1" } else { "repeat" }, loop_c, lsize, ltint);

        // Speed and bass.
        let fx_on = ctx.animate_bool_with_time(id.with("fx_on"), now.fx_on, 0.2);
        if round_button(ui, id.with("fx"), fx_c, 34.0, fx_on, 0.2, ppp, els, true).on_hover_text("Speed and bass").clicked() {
            out.clicked = true;
            out.actions.push(Action::Sound(fx_c));
        }
        icons.paint(ui, "sliders-horizontal", fx_c, 17.0, if now.fx_on { accent } else { dim });

        self.controls = ctl;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Source;

    const BAR: Rect = Rect { min: pos2(250.0, 676.0), max: pos2(1182.0, 742.0) };

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        let base = fonts.families[&FontFamily::Proportional].clone();
        for f in ["semibold", "bold"] {
            fonts.families.insert(FontFamily::Name(f.into()), base.clone());
        }
        ctx.set_fonts(fonts);
        ctx
    }

    fn frame(ctx: &egui::Context, player: &mut Player, icons: &mut Icons, track: &Track, events: Vec<egui::Event>) -> Out {
        let mut out = None;
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 760.0))), events, ..Default::default() };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let now = NowPlaying { track: Some(track), playing: true, loading: false, liked: false, pos: 10.0, dur: 200.0, fullscreen: false, fx_on: false };
                let mut els = Vec::new();
                out = Some(player.ui(ui, icons, BAR, 1.0, 1.0 / 60.0, Theme::Dark, &mut els, now));
            });
        });
        out.unwrap()
    }

    fn press(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// Click at `p`: hover, press, release (one frame each). Returns all actions.
    fn click(p: Pos2) -> Vec<Action> {
        let (ctx, mut player, mut icons) = (ctx(), Player::new(), Icons::default());
        let track = Track { id: "t".into(), title: "Song".into(), artist: "Artist".into(), album: None, artwork: None, permalink: None, duration_ms: 200_000, source: Source::File { path: "x.mp3".into() } };
        let mut acts = Vec::new();
        for _ in 0..3 {
            acts.extend(frame(&ctx, &mut player, &mut icons, &track, vec![egui::Event::PointerMoved(p)]).actions);
        }
        acts.extend(frame(&ctx, &mut player, &mut icons, &track, vec![press(p, true)]).actions);
        acts.extend(frame(&ctx, &mut player, &mut icons, &track, vec![press(p, false)]).actions);
        acts.extend(frame(&ctx, &mut player, &mut icons, &track, vec![]).actions);
        acts
    }

    #[test]
    fn clicking_empty_bar_seeks() {
        let p = pos2(520.0, BAR.center().y);
        let seeks: Vec<f64> = click(p).into_iter().filter_map(|a| if let Action::Seek(s) = a { Some(s) } else { None }).collect();
        assert_eq!(seeks.len(), 1, "one seek expected");
        let want = ((520.0 - BAR.left()) / BAR.width()) as f64 * 200.0;
        assert!((seeks[0] - want).abs() < 0.5, "seek to {} (want {want})", seeks[0]);
    }

    #[test]
    fn clicking_a_button_does_not_seek() {
        // The next button sits 62 pt right of the bar's centre.
        let p = pos2(BAR.center().x.min(BAR.right() - 503.0) + 62.0, BAR.center().y);
        let acts = click(p);
        assert!(acts.iter().any(|a| matches!(a, Action::Next)), "next pressed");
        assert!(!acts.iter().any(|a| matches!(a, Action::Seek(_))), "no seek");
    }
}
