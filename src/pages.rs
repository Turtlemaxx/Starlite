//! Page contents: Explore, Navidrome, playlists (yours, Liked, Downloaded,
//! YouTube, Navidrome) and Settings.

use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, Id, Rect, Sense, Ui, Vec2};

use crate::app::{Starlite, View};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::library::{downloads_dir, Cover, Track, DOWNLOADED_ID, LIKED_ID};
use crate::theme::Theme;
use crate::widgets::{cover, field, filter_tracks, fmt_duration, font, icon_button, pill, playlist_cover, search_field, text_trunc};

struct Card {
    art: Option<String>,
    title: String,
    sub: String,
}

enum Header {
    Local(Cover),
    Url(Option<String>),
}

enum PlKind {
    User(String),
    Liked,
    Downloaded,
    Remote,
}

enum RowAct {
    Play(usize),
    Like(Track),
    Remove(String),
}

impl Starlite {
    #[allow(clippy::too_many_arguments)]
    pub fn draw_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, dt: f32, content: Rect, animating: &mut bool) {
        match self.view.clone() {
            View::Explore => self.explore_page(ui, icons, els, ppp, dt, content, animating),
            View::Settings => self.settings_page(ui, icons, els, ppp, content),
            View::Playlist(id) => self.local_playlist_page(ui, icons, els, ppp, content, &id),
            View::YtPlaylist => self.yt_playlist_page(ui, icons, els, ppp, content, animating),
            View::Navidrome => self.navidrome_page(ui, icons, els, ppp, content, animating),
        }
    }

    // ── Navidrome ──────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn navidrome_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, content: Rect, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let title_r = p.text(pos2(content.left(), 30.0), Align2::LEFT_TOP, "Navidrome", font(30.0, "bold"), theme.text());

        let Some((url, user)) = self.nd.connected() else {
            // Connect form.
            let b = Rect::from_min_size(pos2(content.left(), 96.0), vec2(content.width().min(560.0), 352.0));
            els.push(El::rect(b, ppp, 26.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
            icons.paint(ui, "cloudy", pos2(b.left() + 40.0, b.top() + 42.0), 24.0, theme.accent());
            p.text(pos2(b.left() + 64.0, b.top() + 42.0), Align2::LEFT_CENTER, "Connect your Navidrome server", font(18.0, "bold"), theme.text());
            p.text(pos2(b.left() + 26.0, b.top() + 76.0), Align2::LEFT_CENTER, "Sign in to play your server's playlists here.", font(13.5, "regular"), theme.text_dim());
            let fw = b.width() - 52.0;
            let mut y = b.top() + 100.0;
            let mut submit = false;
            for (i, (hint, password)) in [("Server address, e.g. http://192.168.1.10:4533", false), ("Username", false), ("Password", true)].into_iter().enumerate() {
                let r = Rect::from_min_size(pos2(b.left() + 26.0, y), vec2(fw, 40.0));
                let mut v = match i {
                    0 => std::mem::take(&mut self.ndst.url),
                    1 => std::mem::take(&mut self.ndst.user),
                    _ => std::mem::take(&mut self.ndst.pass),
                };
                let resp = field(ui, els, ppp, r, &mut v, hint, password, true, theme);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                match i {
                    0 => self.ndst.url = v,
                    1 => self.ndst.user = v,
                    _ => self.ndst.pass = v,
                }
                y += 50.0;
            }
            let busy = self.ndst.busy;
            let r = Rect::from_min_size(pos2(b.left() + 26.0, y + 6.0), vec2(150.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("nd_connect"), r, if busy { "Connecting…" } else { "Connect" }, Some("cloudy"), true, true, theme).clicked() {
                submit = true;
            }
            if busy {
                *animating = true;
            }
            if submit && !busy {
                self.sound.click();
                self.connect_navidrome(&ctx);
            }
            if let Some(e) = self.ndst.error.clone() {
                text_trunc(&p, pos2(r.right() + 16.0, r.center().y), Align2::LEFT_CENTER, &e, font(13.0, "semibold"), Color32::from_rgb(235, 87, 87), b.right() - r.right() - 32.0);
            }
            return;
        };

        // Connected: refresh, who / where, disconnect.
        let loading = self.ndst.loading;
        let rc = pos2(title_r.right() + 30.0, title_r.center().y + 1.0);
        let r = icon_button(ui, icons, els, ppp, Id::new("nd_refresh"), rc, 38.0, "rotate-cw", false, false, theme).on_hover_text("Reload playlists");
        if loading {
            let t = ctx.input(|i| i.time) as f32;
            ui.painter().circle_filled(rc, 13.0, theme_bg(theme));
            icons.paint_rotated(ui, "rotate-cw", rc, 17.0, theme.text(), t * 5.0);
            *animating = true;
        }
        if r.clicked() && !loading {
            self.sound.click();
            self.load_navidrome(&ctx);
        }
        let host = url.split("://").nth(1).unwrap_or(&url).to_string();
        let who = text_trunc(&p, pos2(rc.x + 34.0, rc.y), Align2::LEFT_CENTER, &format!("{user} · {host}"), font(13.0, "regular"), theme.text_dim(), (content.right() - rc.x - 300.0).max(60.0));
        let dr = Rect::from_min_size(pos2(who.right() + 16.0, rc.y - 16.0), vec2(112.0, 32.0));
        if pill(ui, icons, els, ppp, Id::new("nd_disconnect"), dr, "Disconnect", None, false, false, theme).clicked() {
            self.sound.click();
            self.nd.disconnect();
            self.ndst = Default::default();
            return;
        }

        // The server's playlists as a grid of cards.
        let area = Rect::from_min_max(pos2(content.left(), 96.0), pos2(content.right(), content.bottom()));
        els.push(El::rect(area, ppp, 26.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
        let inner = area.shrink2(vec2(24.0, 22.0));
        let pls = self.ndst.playlists.clone();
        if pls.is_empty() {
            let msg = if loading || !self.ndst.loaded {
                if !loading {
                    self.load_navidrome(&ctx);
                }
                "Loading playlists…".to_string()
            } else if let Some(e) = &self.ndst.error {
                e.clone()
            } else {
                "No playlists on your server yet".to_string()
            };
            text_trunc(&p, pos2(inner.center().x, inner.top() + 50.0), Align2::CENTER_CENTER, &msg, font(15.0, "semibold"), theme.text(), inner.width() - 40.0);
            return;
        }
        let gap = 18.0;
        let cols = ((inner.width() + gap) / (170.0 + gap)).floor().max(1.0);
        let cw = (inner.width() - gap * (cols - 1.0)) / cols;
        let ch = cw + 48.0;
        let mut open = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner.expand(10.0).intersect(area.shrink(2.0)));
        egui::ScrollArea::vertical().id_salt("nd_grid").auto_shrink([false, false]).show(&mut child, |ui| {
            let rows = (pls.len() as f32 / cols).ceil();
            let (all, _) = ui.allocate_exact_size(vec2(inner.width(), rows * (ch + gap) - gap), Sense::hover());
            for (i, pl) in pls.iter().enumerate() {
                let (c, r) = ((i as f32 % cols), (i as f32 / cols).floor());
                let card = Rect::from_min_size(all.min + vec2(c * (cw + gap), r * (ch + gap)), vec2(cw, ch));
                if !ui.is_rect_visible(card) {
                    continue;
                }
                let resp = ui.interact(card, Id::new(("nd_pl", &pl.id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                let h = ui.ctx().animate_bool_with_time(Id::new(("nd_pl_h", &pl.id)), resp.hovered(), 0.15);
                if h > 0.0 {
                    let c = if theme.is_light() { Color32::from_black_alpha((14.0 * h) as u8) } else { Color32::from_white_alpha((14.0 * h) as u8) };
                    ui.painter().rect_filled(card.expand(6.0), CornerRadius::same(16), c);
                }
                let art = Rect::from_min_size(card.min - vec2(0.0, 3.0 * h), Vec2::splat(cw));
                cover(ui, icons, art, pl.artwork.as_deref(), 14.0, "cloudy", theme);
                text_trunc(ui.painter(), pos2(card.left() + 2.0, art.bottom() + 16.0), Align2::LEFT_CENTER, &pl.title, font(14.0, "semibold"), theme.text(), cw - 4.0);
                let sub = format!("{} song{}", pl.track_count, if pl.track_count == 1 { "" } else { "s" });
                text_trunc(ui.painter(), pos2(card.left() + 2.0, art.bottom() + 35.0), Align2::LEFT_CENTER, &sub, font(12.5, "regular"), theme.text_dim(), cw - 4.0);
                if resp.clicked() {
                    open = Some(pl.clone());
                }
            }
        });
        if let Some(pl) = open {
            self.sound.click();
            self.open_nd_playlist(&ctx, pl);
        }
    }

    // ── Explore ────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn explore_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, dt: f32, content: Rect, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let title_r = p.text(pos2(content.left(), 30.0), Align2::LEFT_TOP, "Explore", font(30.0, "bold"), theme.text());

        if !self.yt.configured() {
            let b = Rect::from_min_size(pos2(content.left(), 96.0), vec2(content.width().min(620.0), 186.0));
            els.push(El::rect(b, ppp, 26.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
            icons.paint(ui, "play", pos2(b.left() + 38.0, b.top() + 42.0), 22.0, theme.accent());
            p.text(pos2(b.left() + 62.0, b.top() + 42.0), Align2::LEFT_CENTER, "Connect YouTube", font(18.0, "bold"), theme.text());
            p.text(pos2(b.left() + 26.0, b.top() + 78.0), Align2::LEFT_CENTER, "Sign in with Google (or add an API key in Settings)", font(13.5, "regular"), theme.text_dim());
            p.text(pos2(b.left() + 26.0, b.top() + 98.0), Align2::LEFT_CENTER, "to see YouTube Music's charts and chart playlists here.", font(13.5, "regular"), theme.text_dim());
            let r = Rect::from_min_size(pos2(b.left() + 26.0, b.bottom() - 60.0), vec2(200.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("explore_signin"), r, "Sign in with Google", Some("play"), true, true, theme).clicked() {
                self.sound.click();
                if self.yt.can_sign_in() {
                    self.sign_in_youtube(&ctx);
                }
                self.view = View::Settings;
            }
            let r2 = Rect::from_min_size(pos2(r.right() + 12.0, r.top()), vec2(150.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("open_settings"), r2, "Use an API key", Some("settings"), false, true, theme).clicked() {
                self.sound.click();
                self.view = View::Settings;
            }
            return;
        }

        // Refresh: new random mix every time.
        let loading = self.explore.loading;
        let rc = pos2(title_r.right() + 30.0, title_r.center().y + 1.0);
        let r = icon_button(ui, icons, els, ppp, Id::new("refresh"), rc, 38.0, "rotate-cw", false, false, theme).on_hover_text("Refresh");
        if loading {
            // Spin the refresh icon while loading.
            let t = ctx.input(|i| i.time) as f32;
            ui.painter().circle_filled(rc, 13.0, theme_bg(theme));
            icons.paint_rotated(ui, "rotate-cw", rc, 17.0, theme.text(), t * 5.0);
            *animating = true;
        }
        if r.clicked() && !loading {
            self.sound.click();
            self.load_explore(&ctx);
        }

        let avail = content.bottom() - 96.0;
        let bh = ((avail - 20.0) / 2.0).clamp(190.0, 250.0);
        let b1 = Rect::from_min_size(pos2(content.left(), 96.0), vec2(content.width(), bh));
        let b2 = b1.translate(vec2(0.0, bh + 20.0));
        let error = self.explore.error.clone();

        let charts: Vec<Card> = self
            .explore
            .charts
            .iter()
            .map(|t| Card { art: t.artwork.clone(), title: t.title.clone(), sub: t.artist.clone() })
            .collect();
        let (mut s, mut st) = (self.explore.chart_scroll, self.explore.chart_scroll_to);
        let c1 = card_row(ui, icons, els, ppp, dt, b1, "Top Charts", &charts, &mut s, &mut st, loading, error.as_deref(), theme, animating, "charts", false);
        self.explore.chart_scroll = s;
        self.explore.chart_scroll_to = st;

        let pls: Vec<Card> = self
            .explore
            .playlists
            .iter()
            .map(|p| Card { art: p.artwork.clone(), title: p.title.clone(), sub: if p.track_count > 0 { format!("{} · {} songs", p.artist, p.track_count) } else { p.artist.clone() } })
            .collect();
        let (mut s, mut st) = (self.explore.pl_scroll, self.explore.pl_scroll_to);
        let c2 = if b2.bottom() <= content.bottom() + 8.0 {
            card_row(ui, icons, els, ppp, dt, b2, "Chart Playlists", &pls, &mut s, &mut st, loading, error.as_deref(), theme, animating, "pls", true)
        } else {
            None
        };
        self.explore.pl_scroll = s;
        self.explore.pl_scroll_to = st;

        if let Some(i) = c1 {
            self.sound.click();
            let list = self.explore.charts.clone();
            self.play_list(list, i);
        }
        if let Some(i) = c2 {
            self.sound.click();
            let pl = self.explore.playlists[i].clone();
            self.open_yt_playlist(&ctx, pl);
        }
    }

    // ── Playlists ──────────────────────────────────────────────────

    fn local_playlist_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, content: Rect, id: &str) {
        let (kind, title, header, tracks) = if id == DOWNLOADED_ID {
            (PlKind::Downloaded, "Downloaded Music".to_string(), Header::Local(Cover::Preset("download".into())), self.lib.downloaded.clone())
        } else if let Some(p) = self.lib.playlists.iter().find(|p| p.id == id) {
            let k = if id == LIKED_ID { PlKind::Liked } else { PlKind::User(id.to_string()) };
            (k, p.name.clone(), Header::Local(p.cover.clone()), p.tracks.clone())
        } else {
            self.view = View::Explore;
            return;
        };
        let sub = format!("{} song{}", tracks.len(), if tracks.len() == 1 { "" } else { "s" });
        self.playlist_page(ui, icons, els, ppp, content, kind, &title, &sub, header, &tracks, false, None, None);
    }

    fn yt_playlist_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, content: Rect, animating: &mut bool) {
        let Some(v) = self.remote.as_ref() else {
            self.view = View::Explore;
            return;
        };
        let pl = v.pl.clone();
        let from_nd = v.navidrome;
        let (tracks, loading, error) = match &v.tracks {
            None => (Vec::new(), true, None),
            Some(Ok(t)) => (t.clone(), false, None),
            Some(Err(e)) => (Vec::new(), false, Some(e.clone())),
        };
        if loading {
            *animating = true;
        }
        let src = if from_nd { "Navidrome" } else { "YouTube" };
        let sub = if pl.track_count > 0 { format!("by {} · {} songs · {src}", pl.artist, pl.track_count) } else { format!("by {} · {src}", pl.artist) };
        self.playlist_page(ui, icons, els, ppp, content, PlKind::Remote, &pl.title, &sub, Header::Url(pl.artwork.clone()), &tracks, loading, error, pl.permalink.clone());
    }

    #[allow(clippy::too_many_arguments)]
    fn playlist_page(
        &mut self,
        ui: &mut Ui,
        icons: &mut Icons,
        els: &mut Vec<El>,
        ppp: f32,
        content: Rect,
        kind: PlKind,
        title: &str,
        sub: &str,
        header: Header,
        tracks: &[Track],
        loading: bool,
        error: Option<String>,
        link: Option<String>,
    ) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();

        // Header: cover, name, count, actions.
        let art = Rect::from_min_size(pos2(content.left(), 28.0), Vec2::splat(120.0));
        match &header {
            Header::Local(c) => playlist_cover(ui, icons, art, c, 18.0, theme),
            Header::Url(u) => cover(ui, icons, art, u.as_deref(), 18.0, "disc-3", theme),
        }
        let tx = art.right() + 24.0;
        text_trunc(&p, pos2(tx, art.top() + 30.0), Align2::LEFT_CENTER, title, font(28.0, "bold"), theme.text(), content.right() - tx);
        text_trunc(&p, pos2(tx, art.top() + 62.0), Align2::LEFT_CENTER, sub, font(13.5, "regular"), theme.text_dim(), content.right() - tx);

        let mut bx = tx;
        let by = art.bottom() - 38.0;
        let play_r = Rect::from_min_size(pos2(bx, by), vec2(104.0, 38.0));
        if pill(ui, icons, els, ppp, Id::new("pl_play"), play_r, "Play", Some("play"), true, false, theme).clicked() && !tracks.is_empty() {
            self.sound.click();
            self.play_list(tracks.to_vec(), 0);
        }
        bx = play_r.right() + 12.0;
        let shuf_r = Rect::from_min_size(pos2(bx, by), vec2(122.0, 38.0));
        if pill(ui, icons, els, ppp, Id::new("pl_shuffle"), shuf_r, "Shuffle", Some("shuffle"), false, false, theme).on_hover_text("Play in random order").clicked() && !tracks.is_empty() {
            self.sound.click();
            self.shuffle_play(tracks.to_vec());
        }
        bx = shuf_r.right() + 12.0;
        let mut end = bx;
        match &kind {
            PlKind::User(id) => {
                let c = pos2(bx + 19.0, by + 19.0);
                end = bx + 38.0;
                if icon_button(ui, icons, els, ppp, Id::new("pl_delete"), c, 38.0, "trash", true, false, theme).on_hover_text("Delete playlist").clicked() {
                    self.sound.click();
                    let name = title.to_string();
                    self.lib.delete(id);
                    self.view = View::Explore;
                    self.toast(format!("Deleted {name}"));
                    return;
                }
            }
            PlKind::Downloaded => {
                let r = Rect::from_min_size(pos2(bx, by), vec2(140.0, 38.0));
                end = r.right();
                if pill(ui, icons, els, ppp, Id::new("pl_folder"), r, "Open folder", None, false, false, theme).clicked() {
                    self.sound.click();
                    let _ = std::process::Command::new("explorer").arg(downloads_dir()).spawn();
                }
            }
            PlKind::Remote => {
                if let Some(url) = &link {
                    let r = Rect::from_min_size(pos2(bx, by), vec2(176.0, 38.0));
                    end = r.right();
                    if pill(ui, icons, els, ppp, Id::new("pl_link"), r, "Open on YouTube", Some("external-link"), false, false, theme).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
            }
            PlKind::Liked => {}
        }

        // Search (finds songs by title, artist or album).
        if self.pl_search.0 != title {
            // Debug: STARLITE_SEARCH=<text> starts with that typed in.
            self.pl_search = (title.to_string(), std::env::var("STARLITE_SEARCH").unwrap_or_default());
        }
        if !tracks.is_empty() {
            let sw = (content.right() - end - 24.0).min(280.0);
            if sw >= 150.0 {
                let r = Rect::from_min_size(pos2(content.right() - sw, by), vec2(sw, 38.0));
                let mut q = std::mem::take(&mut self.pl_search.1);
                search_field(ui, icons, els, ppp, Id::new("pl_search"), r, &mut q, "Search songs", theme);
                self.pl_search.1 = q;
            }
        }
        let query = self.pl_search.1.trim().to_string();
        let shown = filter_tracks(tracks, &query);
        let no_match = !tracks.is_empty() && shown.is_empty();

        // CD shelf instead of the list (Settings → Appearance).
        if self.shelf.prefs.enabled && !tracks.is_empty() {
            let area = Rect::from_min_max(pos2(content.left(), art.bottom() + 16.0), content.max);
            if no_match {
                p.text(pos2(area.center().x, area.top() + 80.0), Align2::CENTER_CENTER, format!("No songs match \u{201c}{query}\u{201d}"), font(15.0, "semibold"), theme.text());
                return;
            }
            let dt = ctx.input(|i| i.stable_dt);
            let mut anim = false;
            let key = format!("{title}|{}|{query}", tracks.len());
            let found: Vec<Track> = shown.iter().map(|&i| tracks[i].clone()).collect();
            if let Some(j) = self.shelf_view(ui, area, content, &found, &key, dt, &mut anim) {
                self.sound.click();
                self.shelf.played(ctx.input(|i| i.pointer.hover_pos()));
                // Play the whole playlist from that song.
                self.play_list(tracks.to_vec(), shown[j]);
            }
            if anim {
                ctx.request_repaint();
            }
            return;
        }

        // Track list in a glass bubble.
        let list = Rect::from_min_max(pos2(content.left(), art.bottom() + 24.0), pos2(content.right(), content.bottom().max(art.bottom() + 150.0)));
        els.push(El::rect(list, ppp, 24.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
        let inner = list.shrink2(vec2(10.0, 12.0));

        if tracks.is_empty() {
            let (msg, sub2) = if loading {
                ("Loading…", "")
            } else if let Some(e) = &error {
                (e.as_str(), "")
            } else {
                match kind {
                    PlKind::Liked => ("No liked songs yet", "Tap the heart in the player to add songs here."),
                    PlKind::Downloaded => ("No downloaded songs", "Put MP3, FLAC, WAV, OGG or M4A files in the Downloaded songs folder."),
                    PlKind::User(_) => ("This playlist is empty", "Add songs with the + button in the player."),
                    PlKind::Remote => ("No playable songs in this playlist", ""),
                }
            };
            p.text(pos2(list.center().x, list.top() + 60.0), Align2::CENTER_CENTER, msg, font(15.0, "semibold"), theme.text());
            if !sub2.is_empty() {
                p.text(pos2(list.center().x, list.top() + 84.0), Align2::CENTER_CENTER, sub2, font(13.0, "regular"), theme.text_dim());
            }
            return;
        }
        if no_match {
            p.text(pos2(list.center().x, list.top() + 60.0), Align2::CENTER_CENTER, format!("No songs match \u{201c}{query}\u{201d}"), font(15.0, "semibold"), theme.text());
            p.text(pos2(list.center().x, list.top() + 84.0), Align2::CENTER_CENTER, "Search looks at song names, artists and albums.", font(13.0, "regular"), theme.text_dim());
            return;
        }

        let current = self.current_track().map(|t| t.id);
        let can_remove = matches!(kind, PlKind::User(_) | PlKind::Liked);
        let pl_id = match &kind {
            PlKind::User(id) => Some(id.clone()),
            PlKind::Liked => Some(LIKED_ID.to_string()),
            _ => None,
        };
        let liked_ids: Vec<String> = self.lib.liked().tracks.iter().map(|t| t.id.clone()).collect();
        let mut acts: Vec<RowAct> = Vec::new();
        let blocked = self.overlay_open();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical()
            .id_salt(("pl_scroll", title))
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let w = ui.available_width();
                for &i in &shown {
                    let t = &tracks[i];
                    let (row, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
                    let hovered = resp.contains_pointer() && !blocked;
                    let rp = ui.painter();
                    let ha = ui.ctx().animate_bool_with_time(Id::new(("row_h", title, i)), hovered, 0.12);
                    if ha > 0.0 {
                        let c = if theme.is_light() { Color32::from_black_alpha((10.0 * ha) as u8) } else { Color32::from_white_alpha((12.0 * ha) as u8) };
                        rp.rect_filled(row, CornerRadius::same(12), c);
                    }
                    let is_cur = current.as_deref() == Some(t.id.as_str());
                    let cy = row.center().y;
                    rp.text(pos2(row.left() + 24.0, cy), Align2::CENTER_CENTER, format!("{}", i + 1), font(12.5, "regular"), if is_cur { theme.accent() } else { theme.text_dim() });
                    let art = Rect::from_center_size(pos2(row.left() + 66.0, cy), Vec2::splat(40.0));
                    cover(ui, icons, art, t.artwork.as_deref(), 8.0, "disc-3", theme);
                    let tx = art.right() + 14.0;
                    let right_pad = 150.0;
                    let tw = (row.right() - right_pad - tx).max(40.0);
                    let rp = ui.painter();
                    text_trunc(rp, pos2(tx, cy - 9.0), Align2::LEFT_CENTER, &t.title, font(14.0, "semibold"), if is_cur { theme.accent() } else { theme.text() }, tw);
                    let artist = match t.source_name() { Some(src) => format!("{} · {src}", t.artist), None => t.artist.clone() };
                    text_trunc(rp, pos2(tx, cy + 10.0), Align2::LEFT_CENTER, &artist, font(12.5, "regular"), theme.text_dim(), tw);
                    rp.text(pos2(row.right() - 16.0, cy), Align2::RIGHT_CENTER, fmt_duration(t.duration_ms), font(12.5, "regular"), theme.text_dim());

                    // Hover actions: like, remove.
                    let liked = liked_ids.contains(&t.id);
                    let mut over_action = false;
                    if hovered || liked {
                        let hc = pos2(row.right() - 74.0, cy);
                        let hr = ui.interact(Rect::from_center_size(hc, Vec2::splat(30.0)), Id::new(("row_like", i, &t.id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                        over_action |= hr.hovered();
                        icons.paint(ui, if liked { "heart-fill" } else { "heart" }, hc, 16.0, if liked { theme.accent() } else { theme.text_dim() });
                        if hr.on_hover_text(if liked { "Remove from Liked Music" } else { "Add to Liked Music" }).clicked() {
                            acts.push(RowAct::Like(t.clone()));
                        }
                    }
                    if hovered && can_remove {
                        let xc = pos2(row.right() - 106.0, cy);
                        let xr = ui.interact(Rect::from_center_size(xc, Vec2::splat(30.0)), Id::new(("row_rm", i, &t.id)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                        over_action |= xr.hovered();
                        icons.paint(ui, "x", xc, 16.0, theme.text_dim());
                        if xr.on_hover_text("Remove from this playlist").clicked() {
                            acts.push(RowAct::Remove(t.id.clone()));
                        }
                    }
                    if resp.clicked() && !over_action {
                        acts.push(RowAct::Play(i));
                    }
                    if hovered {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                }
            });

        if blocked {
            acts.clear();
        }
        for a in acts {
            self.sound.click();
            match a {
                RowAct::Play(i) => self.play_list(tracks.to_vec(), i),
                RowAct::Like(t) => {
                    let was = self.lib.is_liked(&t);
                    self.lib.toggle_like(&t);
                    self.toast(if was { "Removed from Liked Music" } else { "Added to Liked Music" });
                }
                RowAct::Remove(tid) => {
                    if let Some(id) = &pl_id {
                        self.lib.remove_from(id, &tid);
                    }
                }
            }
        }
    }

    // ── Settings ───────────────────────────────────────────────────

    fn settings_page(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, content: Rect) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let x0 = content.left();
        p.text(pos2(x0, 30.0), Align2::LEFT_TOP, "Settings", font(30.0, "bold"), theme.text());

        // Tabs: one glass strip with a glass highlight that slides between them.
        const TABS: [&str; 5] = ["Appearance", "YouTube", "Mini player", "Lyrics", "Discord"];
        let f = font(13.5, "semibold");
        let widths: Vec<f32> = TABS.iter().map(|t| p.layout_no_wrap(t.to_string(), f.clone(), theme.text()).size().x + 34.0).collect();
        let strip = Rect::from_min_size(pos2(x0, 88.0), vec2(widths.iter().sum::<f32>() + 8.0, 40.0));
        els.push(El::rect(strip, ppp, 20.0, 1.0, 6.0, Kind::Glass, 0.0, 0.0));
        let mut x = strip.left() + 4.0;
        let mut sel_rect = None;
        for (i, (name, w)) in TABS.iter().zip(&widths).enumerate() {
            let r = Rect::from_min_size(pos2(x, strip.top() + 4.0), vec2(*w, 32.0));
            x += w;
            let resp = ui.interact(r, Id::new(("settings_tab", i)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            if resp.clicked() && self.settings_tab != i {
                self.sound.click();
                self.settings_tab = i;
            }
            let selected = self.settings_tab == i;
            if selected {
                sel_rect = Some(r);
            }
            let c = if selected || resp.hovered() { theme.text() } else { theme.text_dim() };
            p.text(r.center(), Align2::CENTER_CENTER, *name, f.clone(), c);
        }
        if let Some(r) = sel_rect {
            let l = ctx.animate_value_with_time(Id::new("settings_tab_l"), r.left(), 0.18);
            let w = ctx.animate_value_with_time(Id::new("settings_tab_w"), r.width(), 0.18);
            els.push(El::rect(Rect::from_min_size(pos2(l, r.top()), vec2(w, r.height())), ppp, 16.0, 1.0, 2.0, Kind::Layered, 0.25, 0.0));
        }
        let top = 154.0;

        match self.settings_tab {
            0 => {
                // Appearance.
                let gap = 14.0;
                let n = Theme::ALL.len() as f32;
                let bw = ((content.width() - gap * (n - 1.0)) / n).clamp(120.0, 176.0);
                let bh = 100.0;
                let mut clicked_theme = None;
                for (i, t) in Theme::ALL.into_iter().enumerate() {
                    let base = Rect::from_min_size(pos2(x0 + i as f32 * (bw + gap), top), vec2(bw, bh));
                    let id = Id::new(("theme", t.label()));
                    let resp = ui.interact(base, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
                    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.16);
                    let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.10);
                    let selected = theme == t;
                    let r = Rect::from_center_size(base.center() - vec2(0.0, 2.0 * h), base.size() + Vec2::splat(6.0 * pr));
                    els.push(El::rect(r, ppp, 20.0, 1.0, 8.0 + 4.0 * h, Kind::Glass, if selected { 0.35 } else { 0.0 } + 0.5 * h, pr));
                    icons.paint(ui, t.icon(), r.min + vec2(28.0, 28.0), 22.0, if selected { theme.accent() } else { theme.text() });
                    p.text(r.left_bottom() + vec2(16.0, -24.0), Align2::LEFT_CENTER, t.label(), font(14.5, "semibold"), theme.text());
                    if selected {
                        icons.paint(ui, "check", r.right_top() + vec2(-24.0, 24.0), 16.0, theme.accent());
                    }
                    if resp.clicked() && !selected {
                        clicked_theme = Some(t);
                    }
                }
                if let Some(t) = clicked_theme {
                    self.sound.click();
                    self.theme = t;
                    t.save();
                    self.apply_theme(&ctx);
                }
                let mut on = self.shelf.prefs.enabled;
                let r = Rect::from_min_size(pos2(x0, top + bh + 28.0), vec2(content.width().min(520.0), 52.0));
                if toggle_row(ui, r, Id::new("shelf_on"), "CD shelf", Some("Shows playlists as CD cases on a bookshelf"), &mut on, theme) {
                    self.sound.click();
                    self.shelf.prefs.enabled = on;
                    self.shelf.prefs.save();
                }
            }
            1 => {
                self.youtube_settings(ui, icons, els, ppp, x0, top, content.width().min(520.0));
            }
            2 => {
                // Mini player: on / off and the customize mode.
                let w = content.width().min(520.0);
                let mut on = self.mini.prefs.enabled;
                if toggle_row(ui, Rect::from_min_size(pos2(x0, top), vec2(w, 52.0)), Id::new("mini_on"), "Mini player", Some("Shows up at the edge of the screen while you're in another app"), &mut on, theme) {
                    self.sound.click();
                    self.mini.prefs.enabled = on;
                    if !on {
                        self.mini.editing = false;
                    }
                    self.mini.prefs.save();
                }
                if on {
                    let mut only = self.mini.prefs.only_when_playing;
                    if toggle_row(ui, Rect::from_min_size(pos2(x0, top + 60.0), vec2(w, 52.0)), Id::new("mini_only_playing"), "Only while music is playing", Some("Off: also shows when the song is paused"), &mut only, theme) {
                        self.sound.click();
                        self.mini.prefs.only_when_playing = only;
                        self.mini.prefs.save();
                    }
                    let editing = self.mini.editing;
                    let r = Rect::from_min_size(pos2(x0, top + 130.0), vec2(if editing { 132.0 } else { 236.0 }, 38.0));
                    let label = if editing { "Done" } else { "Change position & size" };
                    if pill(ui, icons, els, ppp, Id::new("mini_customize"), r, label, Some(if editing { "check" } else { "maximize-2" }), true, false, theme).clicked() {
                        self.sound.click();
                        self.mini.editing = !editing;
                        if editing {
                            self.mini.prefs.save();
                        }
                    }
                }
            }
            3 => {
                // Lyrics: look them up online or only use the song's own.
                let mut on = self.lprefs.online;
                if toggle_row(ui, Rect::from_min_size(pos2(x0, top), vec2(content.width().min(520.0), 52.0)), Id::new("lyrics_online"), "Find lyrics online", Some("Gets timed lyrics for the song you're playing from lrclib.net"), &mut on, theme) {
                    self.sound.click();
                    self.lprefs.online = on;
                    self.lprefs.save();
                    self.load_lyrics(&ctx);
                }
            }
            _ => self.discord_settings(ui, icons, els, ppp, x0, top, content),
        }
    }

    /// Discord Rich Presence: on / off, the Application ID, what to show.
    #[allow(clippy::too_many_arguments)]
    fn discord_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, content: Rect) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        let w = content.width().min(700.0);
        let mut changed = false;

        let status = self.discord.status();
        let mut on = self.dprefs.enabled;
        let sub = if status.is_empty() { "Shows the song you're playing on your Discord profile".to_string() } else { status };
        if toggle_row(ui, Rect::from_min_size(pos2(x0, top), vec2(w.min(520.0), 52.0)), Id::new("dc_on"), "Discord status", Some(&sub), &mut on, theme) {
            self.dprefs.enabled = on;
            changed = true;
        }

        // Application ID + help.
        let y = top + 66.0;
        p.text(pos2(x0, y), Align2::LEFT_TOP, "Discord Application ID", font(13.0, "semibold"), theme.text_dim());
        let fr = Rect::from_min_size(pos2(x0, y + 22.0), vec2(250.0, 38.0));
        let resp = field(ui, els, ppp, fr, &mut self.dc_id_input, "e.g. 1234567890123456789", false, false, theme);
        if resp.changed() {
            let id: String = self.dc_id_input.chars().filter(|c| c.is_ascii_digit()).collect();
            if id != self.dprefs.app_id {
                self.dprefs.app_id = id;
                changed = true;
            }
        }
        let r1 = Rect::from_min_size(pos2(fr.right() + 12.0, fr.top()), vec2(176.0, 38.0));
        if pill(ui, icons, els, ppp, Id::new("dc_portal"), r1, "Developer Portal", Some("external-link"), false, false, theme).clicked() {
            ctx.open_url(egui::OpenUrl::new_tab("https://discord.com/developers/applications"));
        }
        let r2 = Rect::from_min_size(pos2(r1.right() + 10.0, fr.top()), vec2(150.0, 38.0));
        if r2.right() <= content.right() && pill(ui, icons, els, ppp, Id::new("dc_icon"), r2, "Save icon…", Some("download"), false, false, theme).on_hover_text("The white Starlite icon to upload as the art asset").clicked() {
            self.sound.click();
            if let Some(path) = rfd::FileDialog::new().set_file_name("starlite.png").add_filter("PNG", &["png"]).save_file() {
                match std::fs::write(&path, crate::logo::WHITE_PNG) {
                    Ok(()) => self.toast("Saved the Starlite icon"),
                    Err(e) => self.toast(format!("Couldn't save: {e}")),
                }
            }
        }
        let help = format!(
            "Create an application named \"Starlite\" in the Developer Portal and paste its Application ID here. Under Rich Presence → Art Assets, upload the Starlite icon with the name \"{}\".",
            crate::discord::ICON_ASSET
        );
        let g = p.layout(help, font(12.5, "regular"), theme.text_dim(), w);
        let gh = g.size().y;
        p.galley(pos2(x0, fr.bottom() + 10.0), g, theme.text_dim());

        // What to show: two columns of switches.
        let y = fr.bottom() + 10.0 + gh + 18.0;
        p.text(pos2(x0, y), Align2::LEFT_TOP, "What to show", font(13.0, "semibold"), theme.text_dim());
        let col_w = ((w - 20.0) / 2.0).min(340.0);
        let rows: [(&str, Option<&str>, &mut bool); 7] = [
            ("Album cover", Some("Off: always the Starlite icon"), &mut self.dprefs.show_cover),
            ("Song name", None, &mut self.dprefs.show_title),
            ("Artist", None, &mut self.dprefs.show_artist),
            ("Album name", Some("When hovering the picture"), &mut self.dprefs.show_album),
            ("Time bar", None, &mut self.dprefs.show_time),
            ("While paused", None, &mut self.dprefs.show_paused),
            ("Find covers online", Some("Local & Navidrome songs, via iTunes"), &mut self.dprefs.lookup_covers),
        ];
        let mut clicked = false;
        for (i, (label, sub, v)) in rows.into_iter().enumerate() {
            let (c, r) = (i % 2, i / 2);
            let rect = Rect::from_min_size(pos2(x0 + c as f32 * (col_w + 20.0), y + 24.0 + r as f32 * 44.0), vec2(col_w, 40.0));
            if toggle_row(ui, rect, Id::new(("dc_opt", i)), label, sub, v, theme) {
                clicked = true;
            }
        }
        if clicked || changed {
            if clicked || resp.lost_focus() || !resp.has_focus() {
                self.sound.click();
            }
            self.dprefs.save();
        }
    }

    /// Returns the bottom y of the section.
    #[allow(clippy::too_many_arguments)]
    fn youtube_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, y: f32, w: f32) -> f32 {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        p.text(pos2(x0, y), Align2::LEFT_TOP, "YouTube", font(13.0, "semibold"), theme.text_dim());
        let mut yy = y + 26.0;

        if let Some(name) = self.yt.account() {
            text_trunc(&p, pos2(x0, yy + 19.0), Align2::LEFT_CENTER, &format!("Signed in as {name}"), font(14.0, "semibold"), theme.text(), w - 130.0);
            let r = Rect::from_min_size(pos2(x0 + w - 116.0, yy), vec2(116.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("yt_signout"), r, "Sign out", None, false, false, theme).clicked() {
                self.sound.click();
                self.yt.sign_out();
                self.yt_status = None;
            }
        } else {
            let r = Rect::from_min_size(pos2(x0, yy), vec2(200.0, 38.0));
            let label = if self.yt_busy { "Waiting for browser…" } else { "Sign in with Google" };
            let resp = pill(ui, icons, els, ppp, Id::new("yt_signin"), r, label, Some("play"), true, false, theme);
            if resp.clicked() {
                self.sound.click();
                if self.yt.can_sign_in() {
                    self.sign_in_youtube(&ctx);
                } else {
                    self.yt_status = Some((false, "Sign in with Google isn't set up in this build yet. Use an API key for now.".into()));
                }
            }
            if self.yt_busy {
                let c = Rect::from_min_size(pos2(r.right() + 12.0, yy), vec2(90.0, 38.0));
                if pill(ui, icons, els, ppp, Id::new("yt_cancel"), c, "Cancel", None, false, false, theme).clicked() {
                    self.yt.cancel_sign_in.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        yy += 52.0;

        p.text(pos2(x0, yy + 6.0), Align2::LEFT_CENTER, "or use an API key instead", font(12.0, "regular"), theme.text_dim());
        yy += 20.0;
        let key_r = Rect::from_min_size(pos2(x0, yy), vec2(w - 96.0, 40.0));
        let mut key = std::mem::take(&mut self.yt_key_input);
        field(ui, els, ppp, key_r, &mut key, "YouTube Data API key", true, false, theme);
        self.yt_key_input = key;
        let save_r = Rect::from_min_size(pos2(key_r.right() + 10.0, yy + 1.0), vec2(86.0, 38.0));
        if pill(ui, icons, els, ppp, Id::new("yt_key_save"), save_r, "Save", None, false, false, theme).clicked() && !self.yt_busy {
            self.sound.click();
            self.save_youtube_key(&ctx);
        }
        yy += 52.0;

        if let Some((ok, msg)) = &self.yt_status {
            let c = if *ok { Color32::from_rgb(70, 190, 120) } else { Color32::from_rgb(235, 90, 100) };
            text_trunc(&p, pos2(x0, yy + 6.0), Align2::LEFT_CENTER, msg, font(12.5, "semibold"), c, w);
            yy += 24.0;
        }
        text_trunc(&p, pos2(x0, yy + 6.0), Align2::LEFT_CENTER, "Free key: Google Cloud → enable YouTube Data API v3 → Credentials.", font(12.0, "regular"), theme.text_dim(), w);
        let link = text_trunc(&p, pos2(x0, yy + 28.0), Align2::LEFT_CENTER, "Open Google Cloud console", font(12.0, "semibold"), theme.accent(), w);
        if ui.interact(link, Id::new("yt_console"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).clicked() {
            ctx.open_url(egui::OpenUrl::new_tab("https://console.cloud.google.com/apis/library/youtube.googleapis.com"));
        }
        yy + 40.0
    }
}

fn theme_bg(theme: Theme) -> Color32 {
    match theme {
        Theme::Light => Color32::from_rgb(240, 240, 242),
        Theme::Dark => Color32::from_rgb(26, 26, 28),
        _ => Color32::from_rgb(14, 12, 14),
    }
}

/// A glass bubble holding a horizontally scrolling row of cards.
#[allow(clippy::too_many_arguments)]
fn card_row(
    ui: &mut Ui,
    icons: &mut Icons,
    els: &mut Vec<El>,
    ppp: f32,
    dt: f32,
    bubble: Rect,
    title: &str,
    items: &[Card],
    scroll: &mut f32,
    scroll_to: &mut f32,
    loading: bool,
    error: Option<&str>,
    theme: Theme,
    animating: &mut bool,
    key: &str,
    square_titles: bool,
) -> Option<usize> {
    let ctx = ui.ctx().clone();
    els.push(El::rect(bubble, ppp, 26.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
    let p = ui.painter().clone();
    let tr = text_trunc(&p, pos2(bubble.left() + 22.0, bubble.top() + 28.0), Align2::LEFT_CENTER, title, font(18.0, "bold"), theme.text(), 300.0);
    p.text(pos2(tr.right() + 10.0, bubble.top() + 29.0), Align2::LEFT_CENTER, "from YouTube", font(12.0, "regular"), theme.text_dim());

    let inner = Rect::from_min_max(pos2(bubble.left() + 18.0, bubble.top() + 52.0), pos2(bubble.right() - 18.0, bubble.bottom() - 12.0));
    let cw = (inner.height() - 44.0).max(60.0);
    let gap = 16.0;
    let n = items.len();
    let total = n as f32 * (cw + gap) - gap;
    let max = (total - inner.width()).max(0.0);

    if ui.rect_contains_pointer(bubble) {
        let d = ctx.input(|i| i.smooth_scroll_delta);
        let delta = if d.x.abs() > d.y.abs() { d.x } else { d.y };
        if delta != 0.0 {
            *scroll_to = (*scroll_to - delta).clamp(0.0, max);
        }
    }
    *scroll_to = scroll_to.clamp(0.0, max);
    *scroll += (*scroll_to - *scroll) * (dt * 14.0).min(1.0);
    if (*scroll - *scroll_to).abs() < 0.3 {
        *scroll = *scroll_to;
    } else {
        *animating = true;
    }

    let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    cu.set_clip_rect(inner);

    if n == 0 {
        if loading {
            // Skeleton cards while the mix loads.
            let pulse = 0.5 + 0.5 * (ctx.input(|i| i.time) as f32 * 3.0).sin();
            *animating = true;
            let c = if theme.is_light() { Color32::from_black_alpha((10.0 + 8.0 * pulse) as u8) } else { Color32::from_white_alpha((10.0 + 8.0 * pulse) as u8) };
            let mut x = inner.left();
            while x < inner.right() {
                cu.painter().rect_filled(Rect::from_min_size(pos2(x, inner.top()), Vec2::splat(cw)), CornerRadius::same(14), c);
                cu.painter().rect_filled(Rect::from_min_size(pos2(x, inner.top() + cw + 10.0), vec2(cw * 0.7, 10.0)), CornerRadius::same(5), c);
                x += cw + gap;
            }
        } else if let Some(e) = error {
            text_trunc(cu.painter(), inner.center(), Align2::CENTER_CENTER, e, font(13.5, "semibold"), theme.text_dim(), inner.width() - 20.0);
        } else {
            cu.painter().text(inner.center(), Align2::CENTER_CENTER, "Nothing here right now. Try refreshing.", font(13.5, "semibold"), theme.text_dim());
        }
        return None;
    }

    let mut clicked = None;
    for (i, item) in items.iter().enumerate() {
        let x = inner.left() + i as f32 * (cw + gap) - *scroll;
        if x > inner.right() || x + cw < inner.left() {
            continue;
        }
        let card = Rect::from_min_size(pos2(x, inner.top()), vec2(cw, cw + 44.0));
        let id = Id::new((key, i));
        let resp = cu.interact(card, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.16);
        let art = Rect::from_min_size(pos2(x, inner.top() + 3.0 - 3.0 * h), Vec2::splat(cw));
        cover(&cu, icons, art, item.art.as_deref(), 14.0, "disc-3", theme);
        if h > 0.01 {
            let c = art.right_bottom() - vec2(24.0, 24.0);
            cu.painter().circle_filled(c, 16.0 * (0.8 + 0.2 * h), Color32::from_black_alpha((150.0 * h) as u8));
            let _ = square_titles;
            icons.paint(&cu, "play", c + vec2(1.0, 0.0), 14.0, Color32::WHITE.gamma_multiply(h));
        }
        let cp = cu.painter();
        text_trunc(cp, pos2(x + 2.0, art.bottom() + 14.0), Align2::LEFT_CENTER, &item.title, font(13.5, "semibold"), theme.text(), cw - 4.0);
        text_trunc(cp, pos2(x + 2.0, art.bottom() + 32.0), Align2::LEFT_CENTER, &item.sub, font(12.0, "regular"), theme.text_dim(), cw - 4.0);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// A settings row: label (+ optional line under it) and an animated switch
/// on the right. The whole row toggles. Returns true when it was flipped.
#[allow(clippy::too_many_arguments)]
fn toggle_row(ui: &mut Ui, r: Rect, id: Id, label: &str, sub: Option<&str>, on: &mut bool, theme: Theme) -> bool {
    let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let ctx = ui.ctx().clone();
    let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.14);
    let p = ui.painter();
    if h > 0.0 {
        let c = if theme.is_light() { Color32::from_black_alpha((9.0 * h) as u8) } else { Color32::from_white_alpha((10.0 * h) as u8) };
        p.rect_filled(r.expand2(vec2(8.0, 2.0)), CornerRadius::same(12), c);
    }
    let tw = r.width() - 70.0;
    match sub {
        Some(s) => {
            text_trunc(p, pos2(r.left(), r.center().y - 9.0), Align2::LEFT_CENTER, label, font(14.0, "semibold"), theme.text(), tw);
            text_trunc(p, pos2(r.left(), r.center().y + 10.0), Align2::LEFT_CENTER, s, font(12.0, "regular"), theme.text_dim(), tw);
        }
        None => {
            text_trunc(p, pos2(r.left(), r.center().y), Align2::LEFT_CENTER, label, font(14.0, "semibold"), theme.text(), tw);
        }
    }
    let flipped = resp.clicked();
    if flipped {
        *on = !*on;
    }
    let t = ctx.animate_bool_with_time(id.with("t"), *on, 0.16);
    let track = Rect::from_center_size(pos2(r.right() - 24.0, r.center().y), vec2(42.0, 24.0));
    let off_c = if theme.is_light() { Color32::from_black_alpha(36) } else { Color32::from_white_alpha(34) };
    let on_c = if theme.is_light() { Color32::from_rgb(30, 30, 34) } else { Color32::from_rgb(235, 235, 240) };
    p.rect_filled(track, CornerRadius::same(12), lerp_color(off_c, on_c, t));
    let knob_c = if theme.is_light() || t < 0.5 { Color32::WHITE } else { Color32::from_rgb(28, 28, 32) };
    let kc = pos2(track.left() + 12.0 + 18.0 * t, track.center().y);
    p.circle_filled(kc + vec2(0.0, 1.0), 10.0, Color32::from_black_alpha(40));
    p.circle_filled(kc, 9.5, knob_c);
    flipped
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}
