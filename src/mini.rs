//! Mini player: while you're in another app (or Starlite is minimized) and a
//! song is loaded, a small
//! always-on-top card is docked to an edge of the screen. At rest it shows
//! only the album cover with the song name under it; hovering slides it
//! further out to reveal compact controls next to the cover. Its background is
//! the album palette, like fullscreen Now Playing.
//!
//! The window never resizes while animating (that stutters on Windows): it is
//! as wide as the expanded card and just slides, with the hidden part past the
//! screen edge. Where that isn't possible (another monitor or the taskbar on
//! that side) it falls back to resizing without animation.
//!
//! Settings → Mini player → Customize opens an edit mode: drag the card to
//! another spot (it snaps to the nearest left/right edge), change its size and
//! pick which controls it shows.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};
use serde::{Deserialize, Serialize};

use crate::app::{smooth, Starlite};
use crate::audio::Status;
use crate::glass::{El, Kind, MAX_EL};
use crate::icons::Icons;
use crate::player::{fmt_time, round_button, Action, Loop};
use crate::widgets::{cover, font, text_trunc};

pub const TITLE: &str = "Starlite Mini";
/// How far the window reaches past the screen edge (hides those corners).
const OVER: f32 = 14.0;
/// Expand / collapse and slide-in times (seconds).
const EXPAND_S: f32 = 0.16;
const SLIDE_S: f32 = 0.28;
const SIZE_MIN: f32 = 0.8;
const SIZE_MAX: f32 = 1.4;
/// Edit toolbar (not scaled).
const TOOL_W: f32 = 284.0;
const TOOL_H: f32 = 112.0;

/// The controls the card can show, in order.
pub const CONTROLS: [(&str, &str); 7] = [
    ("seek", "Seek bar"),
    ("loop", "Loop"),
    ("prev", "Previous"),
    ("play", "Play / pause"),
    ("next", "Next"),
    ("like", "Like"),
    ("volume", "Volume"),
];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct MiniPrefs {
    /// Show the card at all (Settings → Mini player).
    pub enabled: bool,
    /// Only if music was playing when you left (a paused song: no card).
    pub only_when_playing: bool,
    /// Docked to the right edge (else the left one).
    pub right: bool,
    /// Top of the card as a fraction of the work area's height.
    pub y_frac: f32,
    /// A point (physical px) on the monitor it lives on; None = primary.
    pub monitor: Option<[i32; 2]>,
    pub scale: f32,
    pub hidden: Vec<String>,
}

impl Default for MiniPrefs {
    fn default() -> Self {
        Self { enabled: true, only_when_playing: false, right: true, y_frac: 0.2, monitor: None, scale: 1.0, hidden: Vec::new() }
    }
}

impl MiniPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("mini.json")
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
    pub fn shows(&self, key: &str) -> bool {
        !self.hidden.iter().any(|h| h == key)
    }
    fn toggle(&mut self, key: &str) {
        if self.shows(key) {
            self.hidden.push(key.to_string());
        } else {
            self.hidden.retain(|h| h != key);
        }
    }
}

#[derive(Default)]
struct Runtime {
    /// 0 = compact, 1 = expanded (linear; eased when used).
    t: f32,
    /// Slide-in out of the edge, 0..1.
    slide: f32,
    scrub: Option<f32>,
    hwnd: Option<isize>,
    /// Edit-mode drag: cursor minus window position (px), and where the
    /// window is while dragging (points).
    grab: Option<Vec2>,
    free_pos: Option<Pos2>,
    /// After a drop: eases from here to the docked spot.
    snap_from: Option<Pos2>,
    snap_t: f32,
}

#[derive(Default)]
pub struct Mini {
    pub prefs: MiniPrefs,
    /// Customize mode (from Settings).
    pub editing: bool,
    /// Closed with its X: stays away until you leave Starlite again.
    pub dismissed: bool,
    /// Music was playing while you were away, so the card may show. Paused
    /// when you left: no card (pausing from the card keeps it, though).
    pub armed: bool,
    rt: Runtime,
}

impl Mini {
    /// Called every frame: should the card be up? `away` = you're in another
    /// app or Starlite is minimized. With "only while music is playing", the
    /// card needs music playing at some point while you're away, so leaving
    /// with a paused song shows nothing, but pausing from the card keeps it.
    pub fn wants(&mut self, away: bool, playing: bool, has_track: bool) -> bool {
        if !away {
            self.armed = false;
            self.dismissed = false;
        } else if playing {
            self.armed = true;
        }
        self.editing || (away && has_track && !self.dismissed && self.prefs.enabled && (self.armed || !self.prefs.only_when_playing))
    }

    pub fn new() -> Self {
        // Debug: STARLITE_MINI_EDIT=1 starts in customize mode.
        Self { prefs: MiniPrefs::load(), editing: std::env::var("STARLITE_MINI_EDIT").is_ok(), dismissed: false, armed: false, rt: Runtime::default() }
    }
}

/// Card geometry in card coordinates (points, already scaled).
struct Layout {
    k: f32,
    h: f32,
    small_w: f32,
    big_w: f32,
    art: Rect,
    text_x: f32,
    panel: Option<Rect>,
}

fn layout(p: &MiniPrefs) -> Layout {
    let k = p.scale.clamp(SIZE_MIN, SIZE_MAX);
    let pad = 10.0 * k;
    let art_s = 92.0 * k;
    let h = 134.0 * k;
    let small_w = art_s + pad * 2.0;
    let buttons = ["loop", "prev", "play", "next", "like"].iter().filter(|b| p.shows(b)).count() as f32;
    let volume = p.shows("volume");
    let seek = p.shows("seek");
    // Width the controls row needs inside the panel.
    let mut row = 0.0;
    if buttons > 0.0 {
        row += 17.0 * k + (buttons - 1.0) * 30.0 * k + 14.0 * k;
    }
    if volume {
        row += if buttons > 0.0 { 18.0 * k } else { 17.0 * k } + 16.0 * k + 44.0 * k;
    }
    if row > 0.0 {
        row += 12.0 * k;
    }
    let has_panel = seek || row > 0.0;
    let panel_w = row.max(150.0 * k);
    let big_w = small_w - 4.0 * k + panel_w + 8.0 * k;
    let (art, panel_x0, text_x) = if p.right {
        (Rect::from_min_size(pos2(pad, pad), Vec2::splat(art_s)), small_w - 4.0 * k, small_w + 2.0 * k)
    } else {
        (Rect::from_min_size(pos2(big_w - pad - art_s, pad), Vec2::splat(art_s)), 8.0 * k, 14.0 * k)
    };
    let panel = has_panel.then(|| Rect::from_min_size(pos2(panel_x0, 52.0 * k), vec2(panel_w, h - 62.0 * k)));
    Layout { k, h, small_w, big_w, art, text_x, panel }
}

impl Starlite {
    /// Shows the card (call every frame while it should be visible).
    pub fn mini_player(&mut self, ctx: &egui::Context, icons: &mut Icons) -> bool {
        let editing = self.mini.editing;
        let track = self.current_track();
        if track.is_none() && !editing {
            return false;
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(1.0 / 240.0, 1.0 / 20.0);
        let mut animating = false;
        let id = ViewportId::from_hash_of("starlite-mini");
        let ppp = ctx
            .input(|i| i.raw.viewports.get(&id).and_then(|v| v.native_pixels_per_point))
            .or_else(|| ctx.input(|i| i.viewport().native_pixels_per_point))
            .unwrap_or(1.0);
        let lay = layout(&self.mini.prefs);
        let right = self.mini.prefs.right;
        let y_frac = self.mini.prefs.y_frac;
        let probe = self.mini.prefs.monitor.unwrap_or([0, 0]);
        let rt = &mut self.mini.rt;

        // Where it docks.
        let (work, mon) = crate::win::monitor_rects(probe[0], probe[1]);
        let edge_px = if right { work[2] } else { work[0] };
        let cy_px = (work[1] + work[3]) * 0.5;
        // Sliding past the edge needs nothing there (no monitor, no taskbar).
        let offscreen_ok = if right {
            work[2] >= mon[2] && !crate::win::monitor_at(mon[2] + 2.0, cy_px)
        } else {
            work[0] <= mon[0] && !crate::win::monitor_at(mon[0] - 3.0, cy_px)
        };
        let edge = edge_px / ppp;

        // Expand while the cursor is over the card; collapse the moment it leaves.
        let over = rt.scrub.is_some() || rt.hwnd.is_some_and(crate::win::cursor_in_window) || std::env::var("STARLITE_MINI_OPEN").is_ok();
        let target = if editing || over { 1.0 } else { 0.0 };
        if !offscreen_ok && !editing {
            rt.t = target; // a resizing window can't animate smoothly: snap
        } else if rt.t < target {
            rt.t = (rt.t + dt / EXPAND_S).min(target);
        } else if rt.t > target {
            rt.t = (rt.t - dt / EXPAND_S).max(target);
        }
        animating |= rt.t != target;
        let e = smooth(rt.t);

        if offscreen_ok && !editing {
            rt.slide = (rt.slide + dt / SLIDE_S).min(1.0);
            animating |= rt.slide < 1.0;
        } else {
            rt.slide = 1.0;
        }
        let s = 1.0 - (1.0 - rt.slide).powi(3);
        let vis = (lay.small_w + (lay.big_w - lay.small_w) * e) * s;

        // Window size, card origin inside it, and position.
        let (win_w, win_h, ox, x) = if editing {
            let w = lay.big_w.max(TOOL_W);
            (w, lay.h + TOOL_H, if right { w - lay.big_w } else { 0.0 }, if right { edge - w } else { edge })
        } else if offscreen_ok {
            let w = lay.big_w + OVER;
            (w, lay.h, if right { 0.0 } else { OVER }, if right { edge - vis } else { edge - (w - vis) })
        } else {
            let w = vis.max(1.0);
            (w, lay.h, if right { 0.0 } else { -(lay.big_w - w) }, if right { edge - w } else { edge })
        };
        let top = work[1] / ppp;
        let bottom = work[3] / ppp;
        let mut pos = pos2(x, (top + (bottom - top) * y_frac).clamp(top, (bottom - win_h).max(top)));
        // Dragging (edit mode), and the glide back to the edge after a drop.
        if let Some(fp) = rt.free_pos {
            pos = fp;
        } else if let Some(from) = rt.snap_from {
            rt.snap_t = (rt.snap_t + dt / 0.22).min(1.0);
            pos = from + (pos - from) * smooth(rt.snap_t);
            if rt.snap_t >= 1.0 {
                rt.snap_from = None;
            } else {
                animating = true;
            }
        }
        let pos = pos2(pos.x.round(), pos.y.round());

        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_active(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_inner_size(vec2(win_w.round(), win_h.round()))
            .with_position(pos);

        let mut actions: Vec<Action> = Vec::new();
        let mut clicked = false;
        let mut restore = false;
        let mut done = false;
        let mut dismiss = false;
        let mut drop_at: Option<Pos2> = None;

        ctx.show_viewport_immediate(id, builder, |mctx, _| {
            let mppp = mctx.pixels_per_point();
            let full = mctx.screen_rect();
            let time = mctx.input(|i| i.time);
            egui::CentralPanel::default().frame(egui::Frame::new()).show(mctx, |ui| {
                let slot = ui.painter().add(egui::Shape::Noop);
                let mut els: Vec<El> = Vec::with_capacity(MAX_EL);
                let k = lay.k;
                let white = Color32::WHITE;
                let dim = Color32::from_white_alpha(175);
                let p = ui.painter().clone();
                let o = vec2(ox, 0.0);
                let card = Rect::from_min_size(pos2(ox, 0.0), vec2(lay.big_w, lay.h));
                let art = lay.art.translate(o);

                if editing {
                    // Drag the card anywhere; it snaps to the nearest edge.
                    let r = ui.interact(card, Id::new("mini-move"), Sense::drag()).on_hover_cursor(CursorIcon::Grab);
                    if r.drag_started() {
                        let (cx, cy) = crate::win::cursor_pos();
                        self.mini.rt.grab = Some(vec2(cx, cy) - pos.to_vec2() * mppp);
                    }
                    if r.dragged() {
                        if let Some(g) = self.mini.rt.grab {
                            let (cx, cy) = crate::win::cursor_pos();
                            self.mini.rt.free_pos = Some(((vec2(cx, cy) - g) / mppp).to_pos2());
                            mctx.set_cursor_icon(CursorIcon::Grabbing);
                        }
                    }
                    if r.drag_stopped() {
                        drop_at = self.mini.rt.free_pos.take();
                        self.mini.rt.grab = None;
                    }
                } else {
                    // Clicking the cover / name brings Starlite back.
                    let head = if right {
                        Rect::from_min_size(pos2(ox, 0.0), vec2(lay.small_w, lay.h))
                    } else {
                        Rect::from_min_max(pos2(ox + lay.big_w - lay.small_w, 0.0), pos2(ox + lay.big_w, lay.h))
                    };
                    let r = ui.interact(head, Id::new("mini-head"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Open Starlite");
                    if r.clicked() {
                        restore = true;
                    }
                }

                let (title, sub, artwork) = match &track {
                    Some(t) => (t.title.clone(), t.album.clone().filter(|a| !a.is_empty()).unwrap_or_else(|| t.artist.clone()), t.artwork.clone()),
                    None => ("Song name".to_string(), "Album".to_string(), None),
                };
                p.add(egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(art, egui::CornerRadius::same((10.0 * k) as u8)));
                cover(ui, icons, art, artwork.as_deref(), 10.0 * k, "disc-3", self.theme);
                let a_small = (1.0 - e * 2.0).clamp(0.0, 1.0);
                if a_small > 0.0 {
                    text_trunc(&p, pos2(art.center().x, art.bottom() + 16.0 * k), Align2::CENTER_CENTER, &title, font(12.0 * k, "semibold"), white.gamma_multiply(a_small), lay.small_w - 16.0 * k);
                }

                if e > 0.01 {
                    let a = ((e - 0.3) / 0.7).clamp(0.0, 1.0);
                    let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(full));
                    cu.set_opacity(a);
                    let cp = cu.painter().clone();
                    let tw = lay.big_w - lay.small_w - 32.0 * k;
                    // X (corner away from the cover): hide until the next minimize.
                    if !editing {
                        let xc = if right { pos2(ox + lay.big_w - 18.0 * k, 18.0 * k) } else { pos2(ox + 18.0 * k, 18.0 * k) };
                        let xr = round_button(&cu, Id::new("mini-close"), xc, 22.0 * k, 0.0, 0.0, mppp, &mut els, true).on_hover_text("Close until next minimize");
                        if xr.clicked() {
                            dismiss = true;
                        }
                        icons.paint(&cu, "x", xc, 12.0 * k, if xr.hovered() { white } else { dim });
                    }
                    text_trunc(&cp, pos2(ox + lay.text_x, 21.0 * k), Align2::LEFT_CENTER, &title, font(13.5 * k, "semibold"), white, tw);
                    text_trunc(&cp, pos2(ox + lay.text_x, 38.0 * k), Align2::LEFT_CENTER, &sub, font(11.5 * k, "regular"), dim, tw);
                    if let Some(panel) = lay.panel.map(|r| r.translate(o)) {
                        els.push(El::rect(panel, mppp, 18.0 * k, a, 4.0, Kind::Glass, 0.0, 0.0));
                        if a > 0.05 {
                            self.mini_controls(&mut cu, icons, &mut els, mppp, k, panel, !editing, &mut actions, &mut clicked, &mut animating);
                        }
                    }
                }

                if editing {
                    ui.painter().rect_stroke(card.shrink(1.5), egui::CornerRadius::same(10), Stroke::new(1.5_f32, Color32::from_white_alpha(150)), StrokeKind::Inside);
                    let bar = Rect::from_min_max(pos2(0.0, lay.h), pos2(win_w, win_h));
                    if self.mini_toolbar(ui, icons, &mut els, mppp, bar, &mut clicked) {
                        done = true;
                    }
                }

                let mut frame = self.base_frame(full, mppp, time as f32, 0.0);
                frame.tree_rect = [-1e5, -1e5, 1.0, 1.0];
                frame.fs = 1.0;
                self.set_frame(ui, slot, full, frame, &els);
            });
        });

        // Dropped after a drag: dock to the nearest side of that monitor.
        if let Some(at) = drop_at {
            let lay = layout(&self.mini.prefs);
            let w = lay.big_w.max(TOOL_W);
            let c = (at + vec2(w * 0.5, lay.h * 0.5)) * ppp;
            let (work, _) = crate::win::monitor_rects(c.x as i32, c.y as i32);
            let prefs = &mut self.mini.prefs;
            prefs.right = c.x > (work[0] + work[2]) * 0.5;
            prefs.y_frac = ((at.y * ppp - work[1]) / (work[3] - work[1]).max(1.0)).clamp(0.0, 0.95);
            prefs.monitor = Some([((work[0] + work[2]) * 0.5) as i32, ((work[1] + work[3]) * 0.5) as i32]);
            prefs.save();
            self.mini.rt.snap_from = Some(at);
            self.mini.rt.snap_t = 0.0;
            animating = true;
        }
        if self.mini.rt.hwnd.is_none() {
            if let Some(h) = crate::win::find_window(TITLE) {
                crate::win::style_window(h, false);
                self.mini.rt.hwnd = Some(h);
            }
        }
        if restore {
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
        }
        if dismiss {
            self.mini.dismissed = true;
            clicked = true;
        }
        if clicked {
            self.sound.click();
        }
        if done {
            self.mini.editing = false;
            self.mini.prefs.save();
        }
        self.run_actions(ctx, actions, track);
        animating
    }

    /// The card's window, while it's shown.
    pub fn mini_hwnd(&self) -> Option<isize> {
        self.mini.rt.hwnd
    }

    /// Call when the card isn't shown, so it slides in and gets restyled next time.
    pub fn mini_hidden(&mut self) {
        self.mini.rt = Runtime::default();
    }

    /// Edit toolbar under the card: size, which controls show, Done.
    /// Returns true when Done was pressed.
    fn mini_toolbar(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, bar: Rect, clicked: &mut bool) -> bool {
        let white = Color32::WHITE;
        let dim = Color32::from_white_alpha(170);
        let p = ui.painter().clone();
        let panel = bar.shrink2(vec2(8.0, 6.0));
        els.push(El::rect(panel, ppp, 18.0, 1.0, 4.0, Kind::Glass, 0.0, 0.0));
        let id = Id::new("mini-tools");
        let x0 = panel.left() + 14.0;

        p.text(pos2(x0, panel.top() + 15.0), Align2::LEFT_CENTER, "Drag the card to move it", font(11.5, "semibold"), dim);

        // Size: − 100% +
        let y1 = panel.top() + 44.0;
        p.text(pos2(x0, y1), Align2::LEFT_CENTER, "Size", font(12.5, "semibold"), white);
        let minus_c = pos2(x0 + 52.0, y1);
        let plus_c = pos2(x0 + 122.0, y1);
        let scale = self.mini.prefs.scale;
        let can_down = scale > SIZE_MIN + 0.01;
        let can_up = scale < SIZE_MAX - 0.01;
        if round_button(ui, id.with("minus"), minus_c, 26.0, 0.5, 0.0, ppp, els, can_down).clicked() {
            self.mini.prefs.scale = ((scale - 0.1) * 10.0).round() / 10.0;
            self.mini.prefs.save();
            *clicked = true;
        }
        icons.paint(ui, "minus", minus_c, 14.0, if can_down { white } else { dim });
        p.text(pos2((minus_c.x + plus_c.x) * 0.5, y1), Align2::CENTER_CENTER, format!("{}%", (scale * 100.0).round()), font(12.0, "semibold"), white);
        if round_button(ui, id.with("plus"), plus_c, 26.0, 0.5, 0.0, ppp, els, can_up).clicked() {
            self.mini.prefs.scale = ((scale + 0.1) * 10.0).round() / 10.0;
            self.mini.prefs.save();
            *clicked = true;
        }
        icons.paint(ui, "plus", plus_c, 14.0, if can_up { white } else { dim });

        // Done.
        let done_r = Rect::from_center_size(pos2(panel.right() - 40.0, y1), vec2(60.0, 26.0));
        let dr = ui.interact(done_r, id.with("done"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let h = ui.ctx().animate_bool_with_time(id.with("done-h"), dr.hovered(), 0.15);
        els.push(El::rect(done_r, ppp, 13.0, 1.0, 3.0, Kind::Layered, 0.3 + 0.4 * h, 0.0));
        p.text(done_r.center(), Align2::CENTER_CENTER, "Done", font(12.5, "semibold"), white);
        let done = dr.clicked();
        if done {
            *clicked = true;
        }

        // Which controls show: tap to toggle.
        let y2 = panel.bottom() - 22.0;
        let n = CONTROLS.len() as f32;
        let gap = ((panel.width() - 28.0) / n).min(38.0);
        let start = panel.center().x - gap * (n - 1.0) * 0.5;
        for (i, (key, label)) in CONTROLS.iter().enumerate() {
            let c = pos2(start + gap * i as f32, y2);
            let on = self.mini.prefs.shows(key);
            let r = round_button(ui, id.with(*key), c, 28.0, if on { 0.8 } else { 0.0 }, if on { 0.25 } else { 0.0 }, ppp, els, true)
                .on_hover_text(format!("{label}: {}", if on { "shown" } else { "hidden" }));
            if r.clicked() {
                self.mini.prefs.toggle(key);
                self.mini.prefs.save();
                *clicked = true;
            }
            let tint = if on { white } else { Color32::from_white_alpha(90) };
            match *key {
                "seek" => {
                    // A tiny seek bar.
                    p.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], Stroke::new(2.0_f32, tint));
                    p.circle_filled(c - vec2(1.5, 0.0), 3.0, tint);
                }
                "loop" => icons.paint(ui, "repeat", c, 14.0, tint),
                "prev" => icons.paint(ui, "skip-back", c, 14.0, tint),
                "play" => icons.paint(ui, "play", c, 14.0, tint),
                "next" => icons.paint(ui, "skip-forward", c, 14.0, tint),
                "like" => icons.paint(ui, "heart", c, 14.0, tint),
                _ => icons.paint(ui, "volume-2", c, 14.0, tint),
            }
            if !on {
                p.line_segment([c + vec2(-8.0, 8.0), c + vec2(8.0, -8.0)], Stroke::new(1.5_f32, Color32::from_white_alpha(120)));
            }
        }
        done
    }

    #[allow(clippy::too_many_arguments)]
    fn mini_controls(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, k: f32, panel: Rect, live: bool, actions: &mut Vec<Action>, clicked: &mut bool, animating: &mut bool) {
        let prefs = self.mini.prefs.clone();
        let white = Color32::WHITE;
        let dim = Color32::from_white_alpha(170);
        let id = Id::new("mini");
        let p = ui.painter().clone();
        // In edit mode (or with nothing playing) show a sample position.
        let (pos, dur) = if live { self.playback_position() } else { (72.0, 200.0) };
        let status = self.playback_status();
        let buttons: Vec<&str> = ["loop", "prev", "play", "next", "like"].into_iter().filter(|b| prefs.shows(b)).collect();
        let has_row = !buttons.is_empty() || prefs.shows("volume");

        // Seek row.
        if prefs.shows("seek") {
            let sy = if has_row { panel.top() + 17.0 * k } else { panel.center().y };
            let track = Rect::from_min_max(pos2(panel.left() + 40.0 * k, sy - 2.0 * k), pos2(panel.right() - 40.0 * k, sy + 2.0 * k));
            let can_seek = live && dur > 0.0;
            let hit = track.expand2(vec2(6.0 * k, 9.0 * k));
            let resp = ui.interact(hit, id.with("seek"), if can_seek { Sense::click_and_drag() } else { Sense::hover() });
            let frac_at = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0);
            if can_seek {
                if let Some(pt) = resp.interact_pointer_pos() {
                    if resp.dragged() || resp.is_pointer_button_down_on() {
                        self.mini.rt.scrub = Some(frac_at(pt.x));
                    }
                }
                if resp.drag_stopped() || resp.clicked() {
                    if let Some(f) = self.mini.rt.scrub.take().or_else(|| resp.interact_pointer_pos().map(|pt| frac_at(pt.x))) {
                        actions.push(Action::Seek(f as f64 * dur));
                    }
                }
                if !resp.is_pointer_button_down_on() && !resp.dragged() {
                    self.mini.rt.scrub = None;
                }
                resp.clone().on_hover_cursor(CursorIcon::PointingHand);
            }
            let prog = self.mini.rt.scrub.unwrap_or(if dur > 0.0 { (pos / dur) as f32 } else { 0.0 });
            els.push(El::rect(track, ppp, 2.0 * k, 1.0, 0.0, Kind::Track, 0.0, 0.0).with_value(prog));
            if can_seek && (resp.hovered() || self.mini.rt.scrub.is_some()) {
                let kn = Rect::from_center_size(pos2(track.left() + track.width() * prog, sy), vec2(22.0 * k, 14.0 * k));
                els.push(El::rect(kn, ppp, 7.0 * k, 1.0, 2.0, Kind::Lens, 0.0, 0.0).with_value(1.0));
                *animating |= self.mini.rt.scrub.is_some();
            }
            let shown = self.mini.rt.scrub.map(|f| f as f64 * dur).unwrap_or(pos);
            p.text(pos2(panel.left() + 12.0 * k, sy), Align2::LEFT_CENTER, fmt_time(shown), font(10.5 * k, "semibold"), dim);
            p.text(pos2(panel.right() - 12.0 * k, sy), Align2::RIGHT_CENTER, if dur > 0.0 { fmt_time(dur) } else { "--:--".into() }, font(10.5 * k, "semibold"), dim);
        }
        if !has_row {
            return;
        }

        // Controls row: the enabled buttons, then mute + volume.
        let cy = if prefs.shows("seek") { panel.bottom() - 20.0 * k } else { panel.center().y };
        let small = 28.0 * k;
        let mut x = panel.left() + 17.0 * k;
        for b in &buttons {
            let c = pos2(x, cy);
            x += 30.0 * k;
            match *b {
                "loop" => {
                    let lr = round_button(ui, id.with("loop"), c, small, 0.0, 0.0, ppp, els, live).on_hover_text(self.player.looping.tooltip());
                    if lr.clicked() {
                        self.player.looping = self.player.looping.next();
                        *clicked = true;
                    }
                    let on = self.player.looping != Loop::Off;
                    icons.paint(ui, if self.player.looping == Loop::One { "repeat-1" } else { "repeat" }, c, 14.0 * k, if on { white } else { dim });
                }
                "prev" => {
                    if round_button(ui, id.with("prev"), c, small, 0.0, 0.0, ppp, els, live).clicked() {
                        actions.push(Action::Prev);
                        *clicked = true;
                    }
                    icons.paint(ui, "skip-back", c, 15.0 * k, white);
                }
                "play" => {
                    if round_button(ui, id.with("play"), c, 32.0 * k, 0.6, 0.15, ppp, els, live).clicked() {
                        actions.push(Action::TogglePlay);
                        *clicked = true;
                    }
                    let playing = live && matches!(status, Status::Playing | Status::Loading);
                    icons.paint(ui, if playing { "pause" } else { "play" }, c, 16.0 * k, white);
                }
                "next" => {
                    if round_button(ui, id.with("next"), c, small, 0.0, 0.0, ppp, els, live).clicked() {
                        actions.push(Action::Next);
                        *clicked = true;
                    }
                    icons.paint(ui, "skip-forward", c, 15.0 * k, white);
                }
                _ => {
                    let liked = self.current_track().is_some_and(|t| self.lib.is_liked(&t));
                    if round_button(ui, id.with("like"), c, small, 0.0, 0.0, ppp, els, live).on_hover_text(if liked { "Unlike" } else { "Like" }).clicked() {
                        actions.push(Action::Like);
                        *clicked = true;
                    }
                    icons.paint(ui, if liked { "heart-fill" } else { "heart" }, c, 14.0 * k, if liked { Color32::from_rgb(255, 92, 120) } else { white });
                }
            }
        }
        if prefs.shows("volume") {
            // Same spacing as the layout: 18 after the last button's slot.
            let mute_c = if buttons.is_empty() { pos2(x, cy) } else { pos2(x - 30.0 * k + 32.0 * k, cy) };
            if round_button(ui, id.with("mute"), mute_c, small, 0.0, 0.0, ppp, els, live).clicked() {
                self.player.muted = !self.player.muted;
                actions.push(Action::Volume(self.player.effective_volume()));
                *clicked = true;
            }
            let vol = self.player.effective_volume();
            icons.paint(ui, if vol <= 0.001 { "volume-x" } else if vol < 0.5 { "volume-1" } else { "volume-2" }, mute_c, 14.0 * k, white);
            let vt = Rect::from_min_max(pos2(mute_c.x + 16.0 * k, cy - 2.0 * k), pos2(panel.right() - 12.0 * k, cy + 2.0 * k));
            let vresp = ui.interact(vt.expand2(vec2(6.0 * k, 10.0 * k)), id.with("vol"), if live { Sense::click_and_drag() } else { Sense::hover() });
            let before = vol;
            if live {
                let vresp = vresp.clone().on_hover_cursor(CursorIcon::PointingHand);
                if let Some(pt) = vresp.interact_pointer_pos() {
                    if vresp.dragged() || vresp.clicked() {
                        self.player.volume = ((pt.x - vt.left()) / vt.width()).clamp(0.0, 1.0);
                        self.player.muted = false;
                    }
                }
                if vresp.hovered() {
                    let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.player.volume = (self.player.volume + scroll * 0.002).clamp(0.0, 1.0);
                        self.player.muted = false;
                    }
                }
            }
            let vol = self.player.effective_volume();
            els.push(El::rect(vt, ppp, 2.0 * k, 1.0, 0.0, Kind::Track, 0.0, 0.0).with_value(vol));
            if live && (vresp.hovered() || vresp.dragged()) {
                let kn = Rect::from_center_size(pos2(vt.left() + vt.width() * vol, cy), vec2(14.0 * k, 9.0 * k));
                els.push(El::rect(kn, ppp, 4.5 * k, 1.0, 2.0, Kind::Lens, 0.0, 0.0).with_value(1.0));
            }
            if (vol - before).abs() > 1e-4 {
                actions.push(Action::Volume(vol));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mini(only_when_playing: bool) -> Mini {
        let mut m = Mini::default();
        m.prefs = MiniPrefs { only_when_playing, ..Default::default() };
        m
    }

    #[test]
    fn hidden_while_in_starlite() {
        let mut m = mini(true);
        assert!(!m.wants(false, true, true));
        let mut m = mini(false);
        assert!(!m.wants(false, true, true));
    }

    #[test]
    fn leaving_while_playing_shows_it() {
        let mut m = mini(true);
        m.wants(false, true, true);
        assert!(m.wants(true, true, true));
    }

    #[test]
    fn leaving_while_paused_shows_nothing() {
        let mut m = mini(true);
        m.wants(false, false, true);
        assert!(!m.wants(true, false, true));
        assert!(!m.wants(true, false, true), "still nothing while away and paused");
        // Music starts while away (e.g. from somewhere else): now it shows.
        assert!(m.wants(true, true, true));
    }

    #[test]
    fn pausing_from_the_card_keeps_it() {
        let mut m = mini(true);
        m.wants(false, true, true);
        assert!(m.wants(true, true, true));
        assert!(m.wants(true, false, true), "paused from the card");
    }

    #[test]
    fn coming_back_resets_it() {
        let mut m = mini(true);
        m.wants(true, true, true);
        m.wants(false, false, true); // back in Starlite, pauses there
        assert!(!m.wants(true, false, true), "left again while paused");
    }

    #[test]
    fn option_off_shows_even_when_paused() {
        let mut m = mini(false);
        m.wants(false, false, true);
        assert!(m.wants(true, false, true));
    }

    #[test]
    fn x_hides_until_next_time() {
        let mut m = mini(false);
        assert!(m.wants(true, true, true));
        m.dismissed = true;
        assert!(!m.wants(true, true, true));
        m.wants(false, true, true);
        assert!(m.wants(true, true, true), "back after leaving again");
    }

    #[test]
    fn needs_a_song_and_the_switch_on() {
        let mut m = mini(false);
        assert!(!m.wants(true, false, false), "no song loaded");
        m.prefs.enabled = false;
        assert!(!m.wants(true, true, true), "mini player off");
    }
}
