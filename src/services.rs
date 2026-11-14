//! Settings → Services: one menu for everything Starlite connects to
//! (Navidrome, Jellyfin, YouTube, Discord, Last.fm), each with its
//! configuration beside it. Also the welcome window of the very first start,
//! where Navidrome, Jellyfin or both can be connected straight away.

use egui::{pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Ui};

use crate::app::{smooth, Starlite, View};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::stream::Server;
use crate::widgets::{field, font, pill, text_trunc};

/// The Settings tab the services live in.
pub const TAB: usize = 6;

/// The menu: name and icon, in this order.
pub const SERVICES: [(&str, &str); 5] = [("Navidrome", "navidrome"), ("Jellyfin", "jellyfin"), ("YouTube", "youtube"), ("Discord", "discord"), ("Last.fm", "lastfm")];

const GOOD: Color32 = Color32::from_rgb(70, 190, 120);
const BAD: Color32 = Color32::from_rgb(235, 90, 100);

/// The first-start window.
pub struct Welcome {
    open_t: f32,
    /// Connect was pressed: close by itself once everything asked for is connected.
    tried: bool,
    /// Nothing was filled in when Connect was pressed.
    empty: bool,
}

fn marker() -> std::path::PathBuf {
    crate::library::data_dir().join("welcomed")
}

/// Should the welcome window show? Only on a first start: nothing saved
/// yet and no server connected. (Remembers that it was asked.)
pub fn first_start(any_server: bool) -> Option<Welcome> {
    // Debug: STARLITE_WELCOME=1 always shows it.
    let forced = std::env::var("STARLITE_WELCOME").is_ok();
    let seen = marker().exists();
    let used = crate::library::data_dir().join("library.json").exists() || any_server;
    if !forced && (seen || used) {
        if !seen {
            done();
        }
        return None;
    }
    Some(Welcome { open_t: 0.0, tried: false, empty: false })
}

fn done() {
    let f = marker();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, "1");
}

impl Starlite {
    /// Opens Settings → Services on one of them.
    pub fn open_service(&mut self, i: usize) {
        self.fullscreen = false;
        self.view = View::Settings;
        self.settings_tab = TAB;
        self.service_sel = i.min(SERVICES.len() - 1);
    }

    /// The servers that are connected (the sidebar shows these).
    pub fn connected_servers(&self) -> Vec<Server> {
        [Server::Navidrome, Server::Jellyfin].into_iter().filter(|s| self.server_connected(*s).is_some()).collect()
    }

    /// A service's line in the menu, and whether it's set up.
    fn service_status(&self, i: usize) -> (String, bool) {
        let server = |s: Server| match self.server_connected(s) {
            Some((_, user)) => (format!("Connected as {user}"), true),
            None => ("Not set up".to_string(), false),
        };
        match i {
            0 => server(Server::Navidrome),
            1 => server(Server::Jellyfin),
            2 => match self.yt.account() {
                Some(name) => (format!("Signed in as {name}"), true),
                None if self.yt.configured() => ("API key set".to_string(), true),
                None => ("Not set up".to_string(), false),
            },
            3 if self.dprefs.enabled && !self.dprefs.app_id.is_empty() => ("Status on".to_string(), true),
            3 if self.dprefs.enabled => ("Needs an Application ID".to_string(), false),
            3 => ("Off".to_string(), false),
            _ if self.lastfm.prefs.connected() && self.lastfm.prefs.enabled => (format!("Scrobbling as {}", self.lastfm.prefs.user), true),
            _ if self.lastfm.prefs.connected() => ("Connected, scrobbling off".to_string(), false),
            _ => ("Not set up".to_string(), false),
        }
    }

    /// Address, username and password boxes for a server. Returns whether
    /// Enter was pressed in one, and where they end.
    #[allow(clippy::too_many_arguments)]
    fn server_fields(&mut self, ui: &mut Ui, els: &mut Vec<El>, ppp: f32, x: f32, mut y: f32, w: f32, s: Server, on_glass: bool) -> (bool, f32) {
        let theme = self.theme;
        let address = match s {
            Server::Navidrome => "Server address, e.g. http://192.168.1.10:4533",
            Server::Jellyfin => "Server address, e.g. http://192.168.1.10:8096",
        };
        let mut submit = false;
        for (i, (hint, password)) in [(address, false), ("Username", false), ("Password", true)].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 40.0));
            let mut v = match i {
                0 => std::mem::take(&mut self.st(s).url),
                1 => std::mem::take(&mut self.st(s).user),
                _ => std::mem::take(&mut self.st(s).pass),
            };
            let resp = field(ui, els, ppp, r, &mut v, hint, password, on_glass, theme);
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            match i {
                0 => self.st(s).url = v,
                1 => self.st(s).user = v,
                _ => self.st(s).pass = v,
            }
            y += 50.0;
        }
        (submit, y - 10.0)
    }

    /// A server's configuration: sign in, or who's signed in and disconnect.
    #[allow(clippy::too_many_arguments)]
    fn server_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, w: f32, s: Server) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        p.text(pos2(x0, top), Align2::LEFT_TOP, s.name(), font(13.0, "semibold"), theme.text_dim());
        if let Some((url, user)) = self.server_connected(s) {
            let host = url.split("://").nth(1).unwrap_or(&url).to_string();
            text_trunc(&p, pos2(x0, top + 42.0), Align2::LEFT_CENTER, &format!("Connected as {user}"), font(14.5, "semibold"), theme.text(), w);
            text_trunc(&p, pos2(x0, top + 62.0), Align2::LEFT_CENTER, &host, font(12.5, "regular"), theme.text_dim(), w);
            let r = Rect::from_min_size(pos2(x0, top + 86.0), vec2(166.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new(("srv_open", s.name())), r, "Open playlists", Some(s.icon()), true, false, theme).clicked() {
                self.sound.click();
                if !self.st(s).loaded {
                    self.load_server(&ctx, s);
                }
                self.view = View::Server(s);
            }
            let r2 = Rect::from_min_size(pos2(r.right() + 10.0, r.top()), vec2(122.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new(("srv_off", s.name())), r2, "Disconnect", None, false, false, theme).clicked() {
                self.sound.click();
                self.disconnect_server(s);
            }
            let note = format!("{} is in the sidebar while it's connected. Only a login token is saved, never your password.", s.name());
            let g = p.layout(note, font(12.5, "regular"), theme.text_dim(), w.min(520.0));
            p.galley(pos2(x0, top + 142.0), g, theme.text_dim());
            return;
        }
        p.text(pos2(x0, top + 34.0), Align2::LEFT_CENTER, "Sign in to play your server's playlists in Starlite.", font(13.5, "regular"), theme.text_dim());
        let fw = w.min(420.0);
        let (enter, bottom) = self.server_fields(ui, els, ppp, x0, top + 56.0, fw, s, false);
        let busy = self.st(s).busy;
        let r = Rect::from_min_size(pos2(x0, bottom + 16.0), vec2(if busy { 170.0 } else { 136.0 }, 38.0));
        let click = pill(ui, icons, els, ppp, Id::new(("srv_connect", s.name())), r, if busy { "Connecting\u{2026}" } else { "Connect" }, Some(s.icon()), true, false, theme).clicked();
        if busy {
            ctx.request_repaint();
        }
        if (click || enter) && !busy {
            self.sound.click();
            self.connect_server(&ctx, s);
        }
        if let Some(e) = self.st(s).error.clone() {
            text_trunc(&p, pos2(x0, r.bottom() + 20.0), Align2::LEFT_CENTER, &e, font(13.0, "semibold"), BAD, w);
        }
    }

    /// The Services tab: the menu on the left, the picked one's settings beside it.
    #[allow(clippy::too_many_arguments)]
    pub fn services_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, content: Rect) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let (mw, row) = (236.0, 52.0);
        let menu = Rect::from_min_size(pos2(x0, top), vec2(mw, SERVICES.len() as f32 * row + 12.0));
        els.push(El::rect(menu, ppp, 20.0, 1.0, 8.0, Kind::Glass, 0.0, 0.0));
        let mut picked = None;
        let mut hover = None;
        for (i, (name, icon)) in SERVICES.iter().enumerate() {
            let r = Rect::from_min_size(pos2(menu.left() + 6.0, menu.top() + 6.0 + i as f32 * row), vec2(mw - 12.0, row));
            let id = Id::new(("service", i));
            let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let sel = self.service_sel == i;
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered() && !sel, 0.14);
            if h > 0.001 {
                hover = Some(El::rect(r.shrink(2.0), ppp, 13.0, h, 2.0, Kind::Layered, 0.3 * h, 0.0));
            }
            let (status, set_up) = self.service_status(i);
            icons.paint(ui, icon, pos2(r.left() + 22.0, r.center().y), 18.0, if sel { theme.accent() } else { theme.text_dim() });
            p.text(pos2(r.left() + 46.0, r.center().y - 8.0), Align2::LEFT_CENTER, *name, font(14.0, "semibold"), if sel { theme.text() } else { theme.text().gamma_multiply(0.86) });
            text_trunc(&p, pos2(r.left() + 46.0, r.center().y + 10.0), Align2::LEFT_CENTER, &status, font(11.5, "regular"), theme.text_dim(), mw - 12.0 - 46.0 - 30.0);
            if set_up {
                icons.paint(ui, "check", pos2(r.right() - 18.0, r.center().y), 14.0, theme.accent());
            }
            if resp.clicked() && !sel {
                picked = Some(i);
            }
        }
        if let Some(h) = hover {
            els.push(h);
        }
        let sy = ctx.animate_value_with_time(Id::new("service_sel_y"), menu.top() + 6.0 + self.service_sel as f32 * row, 0.16);
        els.push(El::rect(Rect::from_min_size(pos2(menu.left() + 6.0, sy), vec2(mw - 12.0, row)).shrink(2.0), ppp, 13.0, 1.0, 2.0, Kind::Layered, 0.25, 0.0));
        if let Some(i) = picked {
            self.sound.click();
            self.service_sel = i;
        }

        // The picked one's settings.
        let cx = menu.right() + 32.0;
        let w = (content.right() - cx).max(260.0);
        let top = top + 6.0;
        match self.service_sel {
            0 => self.server_settings(ui, icons, els, ppp, cx, top, w, Server::Navidrome),
            1 => self.server_settings(ui, icons, els, ppp, cx, top, w, Server::Jellyfin),
            2 => {
                self.youtube_settings(ui, icons, els, ppp, cx, top, w.min(520.0));
            }
            3 => self.discord_settings(ui, icons, els, ppp, cx, top - 6.0, Rect::from_min_max(pos2(cx, content.top()), content.max)),
            _ => self.lastfm_settings(ui, icons, els, ppp, cx, top - 6.0, w.min(560.0)),
        }
    }

    /// The welcome window of the first start: connect Navidrome, Jellyfin
    /// or both (whichever is filled in), or skip.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_welcome(&mut self, ui: &mut Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let Some(wl) = self.welcome.as_mut() else { return };
        wl.open_t = (wl.open_t + dt * 6.0).min(1.0);
        if wl.open_t < 1.0 {
            *animating = true;
        }
        let (ot, tried, empty) = (smooth(wl.open_t), wl.tried, wl.empty);

        // Blocks everything behind (no click-away: Skip is the way out).
        ui.interact(full, ui.id().with("welcome_back"), Sense::click());
        let size = vec2((full.width() - 60.0).min(748.0), 452.0);
        let panel = Rect::from_center_size(full.center() + vec2(0.0, 18.0 * (1.0 - ot)), size * (0.94 + 0.06 * ot));
        let mut els: Vec<El> = vec![El::rect(panel, ppp, 28.0, 1.0, 14.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let p = ui.painter().clone();
        let (fg, dim) = (theme.text().gamma_multiply(ot), theme.text_dim().gamma_multiply(ot));
        let x0 = panel.left() + 30.0;
        let w = panel.width() - 60.0;
        p.text(pos2(x0, panel.top() + 38.0), Align2::LEFT_CENTER, "Welcome to Starlite", font(22.0, "bold"), fg);
        let intro = "Have a music server? Connect Navidrome, Jellyfin or both now: fill in the ones you use. You can always do this later in Settings \u{2192} Services.";
        let g = p.layout(intro.to_string(), font(13.0, "regular"), dim, w);
        p.galley(pos2(x0, panel.top() + 60.0), g, dim);

        let gap = 24.0;
        let cw = (w - gap) / 2.0;
        let top = panel.top() + 116.0;
        let mut enter = false;
        let servers = [Server::Navidrome, Server::Jellyfin];
        for (k, s) in servers.into_iter().enumerate() {
            let cx = x0 + k as f32 * (cw + gap);
            icons.paint(ui, s.icon(), pos2(cx + 11.0, top + 10.0), 20.0, theme.accent().gamma_multiply(ot));
            p.text(pos2(cx + 32.0, top + 10.0), Align2::LEFT_CENTER, s.name(), font(16.0, "bold"), fg);
            if let Some((url, user)) = self.server_connected(s) {
                let host = url.split("://").nth(1).unwrap_or(&url).to_string();
                icons.paint(ui, "check", pos2(cx + 10.0, top + 52.0), 16.0, GOOD.gamma_multiply(ot));
                text_trunc(&p, pos2(cx + 28.0, top + 52.0), Align2::LEFT_CENTER, &format!("Connected as {user}"), font(14.0, "semibold"), fg, cw - 28.0);
                text_trunc(&p, pos2(cx + 28.0, top + 72.0), Align2::LEFT_CENTER, &host, font(12.5, "regular"), dim, cw - 28.0);
                continue;
            }
            let (e, bottom) = self.server_fields(ui, &mut els, ppp, cx, top + 34.0, cw, s, true);
            enter |= e;
            let line = match (self.st(s).busy, self.st(s).error.clone()) {
                (true, _) => Some(("Connecting\u{2026}".to_string(), dim)),
                (false, Some(e)) => Some((e, BAD.gamma_multiply(ot))),
                _ => None,
            };
            if let Some((text, c)) = line {
                text_trunc(&p, pos2(cx, bottom + 22.0), Align2::LEFT_CENTER, &text, font(12.5, "semibold"), c, cw);
            }
        }
        let connected = servers.iter().filter(|s| self.server_connected(**s).is_some()).count();
        let busy = servers.iter().any(|s| self.st(*s).busy);
        // Filled in but not connected (yet, or it failed).
        let open_forms = servers.iter().filter(|s| self.server_connected(**s).is_none() && !self.st(**s).url.trim().is_empty()).count();
        if busy {
            *animating = true;
        }
        let by = panel.bottom() - 28.0 - 40.0;
        let go_label = if busy {
            "Connecting\u{2026}"
        } else if connected > 0 && open_forms == 0 {
            "Done"
        } else {
            "Connect"
        };
        let go_r = Rect::from_min_size(pos2(panel.right() - 30.0 - 150.0, by), vec2(150.0, 40.0));
        let skip_r = Rect::from_min_size(pos2(go_r.left() - 12.0 - 130.0, by), vec2(130.0, 40.0));
        // With one server in and the other one failing, this is the way on.
        let skip = (connected == 0 || open_forms > 0) && !busy && pill(ui, icons, &mut els, ppp, ui.id().with("wl_skip"), skip_r, if connected > 0 { "Done" } else { "Skip for now" }, None, false, true, theme).clicked();
        let go = pill(ui, icons, &mut els, ppp, ui.id().with("wl_go"), go_r, go_label, None, true, true, theme).clicked() || enter;
        if empty && open_forms == 0 && connected == 0 {
            p.text(pos2(x0, by + 20.0), Align2::LEFT_CENTER, "Fill in a server first, or skip", font(12.5, "semibold"), dim);
        }

        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        frame.dim = 0.45 * ot;
        self.set_frame(ui, slot, full, frame, &els);

        let mut close = skip;
        if go && !busy {
            self.sound.click();
            if connected > 0 && open_forms == 0 {
                close = true;
            } else {
                for s in servers {
                    if self.server_connected(s).is_none() && !self.st(s).url.trim().is_empty() {
                        self.connect_server(&ctx, s);
                    }
                }
                if let Some(wl) = self.welcome.as_mut() {
                    wl.tried = open_forms > 0;
                    wl.empty = open_forms == 0;
                }
            }
        }
        // Everything that was asked for is connected: on to the music.
        if tried && !busy && connected > 0 && open_forms == 0 {
            close = true;
        }
        if close {
            if skip {
                self.sound.click();
            }
            self.welcome = None;
            done();
            if let Some(s) = self.connected_servers().first().copied() {
                self.view = View::Server(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_the_five_services() {
        let names: Vec<&str> = SERVICES.iter().map(|s| s.0).collect();
        assert_eq!(names, ["Navidrome", "Jellyfin", "YouTube", "Discord", "Last.fm"]);
        assert!(SERVICES.iter().all(|s| crate::icons::known(s.1).is_some()));
    }
}
