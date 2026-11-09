//! Starlite: window shell, navigation, playback queue, overlays.
//! Page contents live in `pages.rs`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::{egui_glow, glow};
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, Pos2, Rect, ResizeDirection,
    Sense, Stroke, Vec2, ViewportCommand,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::audio::{Audio, Status};
use crate::glass::{El, Frame, GlassRenderer, Kind, MAX_EL};
use crate::icons::{Icons, PRESETS};
use crate::library::{import_cover, Cover, Library, Source, Track, DOWNLOADED_ID, LIKED_ID};
use crate::meta::Lyrics;
use crate::player::{Action, Loop, NowPlaying, Player};
use crate::sound::Sound;
use crate::theme::Theme;
use crate::widgets::{field, font, icon_button, pill, playlist_cover, text_trunc};
use crate::youtube::{RemotePlaylist, YouTube};
use crate::ytplayer::{YtEvent, YtPlayer};

pub const NAV_FULL: f32 = 232.0;
pub const NAV_MINI: f32 = 80.0;
pub const PLAYER_H: f32 = 66.0;
const TREE_SIZE: [f32; 2] = [540.0, 450.0];
/// The YouTube player card (16:9, at least 200 px tall per YouTube's embed rules).
pub const VIDEO: [f32; 2] = [356.0, 200.0];

#[derive(Clone, PartialEq, Debug)]
pub enum View {
    Explore,
    Settings,
    /// A local playlist (incl. Liked / Downloaded) by id.
    Playlist(String),
    /// A YouTube playlist opened from Explore, or a Navidrome playlist.
    YtPlaylist,
    /// Navidrome: connect, then the server's playlists.
    Navidrome,
}

#[derive(Clone, PartialEq, Debug)]
enum NavKey {
    Explore,
    Navidrome,
    Playlist(String),
    Liked,
    Downloaded,
    Settings,
}

pub enum Job {
    Explore(Result<(Vec<Track>, Vec<RemotePlaylist>), String>),
    YtPlaylist(String, Result<Vec<Track>, String>),
    YtSignIn(Result<String, String>),
    Palette(String, Option<[[f32; 3]; 4]>),
    Lyrics(String, Option<Lyrics>),
    YtKeyTest(Result<(), String>),
    NdConnect(Result<(), String>),
    NdPlaylists(Result<Vec<RemotePlaylist>, String>),
}

/// The Navidrome page: the connect form and the loaded playlists.
#[derive(Default)]
pub struct NdState {
    pub url: String,
    pub user: String,
    pub pass: String,
    pub busy: bool,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded: bool,
    pub playlists: Vec<RemotePlaylist>,
}

#[derive(Default)]
pub struct ExploreState {
    pub loading: bool,
    pub error: Option<String>,
    pub charts: Vec<Track>,
    pub playlists: Vec<RemotePlaylist>,
    pub loaded: bool,
    pub chart_scroll: f32,
    pub chart_scroll_to: f32,
    pub pl_scroll: f32,
    pub pl_scroll_to: f32,
}

pub struct RemoteView {
    pub pl: RemotePlaylist,
    /// From Navidrome (else YouTube).
    pub navidrome: bool,
    pub tracks: Option<Result<Vec<Track>, String>>,
}

pub struct Dialog {
    pub name: String,
    pub cover: Cover,
    pub error: Option<String>,
    pub open_t: f32,
    /// Add this song once the playlist exists (from the player's "+").
    pub add_after: Option<Track>,
}

pub struct Popover {
    pub anchor: Pos2,
    pub track: Track,
    pub open_t: f32,
}

pub struct Starlite {
    pub renderer: Arc<Mutex<GlassRenderer>>,
    pub sound: Sound,
    pub hwnd: Option<isize>,
    pub theme: Theme,
    pub tree_ppp: Option<f32>,
    /// Frame interval (ms) for the background ticker; 0 = off.
    pub ticker_ms: Arc<AtomicU64>,
    pub icons: Icons,
    logo: crate::logo::Logo,
    pub lib: Library,
    pub audio: Audio,
    pub yt: Arc<YouTube>,
    pub nd: Arc<crate::navidrome::Navidrome>,
    pub ndst: NdState,
    pub settings_tab: usize,
    pub discord: crate::discord::Discord,
    pub dprefs: crate::discord::DiscordPrefs,
    pub dc_id_input: String,
    pub ytp: Option<YtPlayer>,
    /// The song that's playing (or loading / paused), from any source.
    pub now: Option<Track>,
    pub yt_busy: bool,
    pub yt_status: Option<(bool, String)>,
    pub yt_key_input: String,
    /// Where the YouTube video card sits this frame (overlays avoid it).
    pub card: Option<Rect>,
    /// Fullscreen Now Playing.
    pub fullscreen: bool,
    pub fs_t: f32,
    /// Lyrics for the current song (track id, lyrics if it has any).
    pub lyrics: Option<(String, Option<Lyrics>)>,
    pub lyric_scroll: f32,
    /// Album palette for the fullscreen background (eased towards the target).
    pub palette: [[f32; 3]; 4],
    palette_target: [[f32; 3]; 4],
    palette_for: Option<String>,
    pub jobs_tx: Sender<Job>,
    pub jobs_rx: Receiver<Job>,
    pub view: View,
    sel: Option<Rect>,
    sel_v: f32,
    sel_vis: f32,
    nav_open: bool,
    nav_t: f32,
    ptr_glow: f32,
    nav_scroll: f32,
    pub explore: ExploreState,
    pub remote: Option<RemoteView>,
    pub queue: Vec<Track>,
    pub qi: usize,
    last_finished: u64,
    last_status: Status,
    pub player: Player,
    pub mini: crate::mini::Mini,
    pub dialog: Option<Dialog>,
    pub popover: Option<Popover>,
    pub toast: Option<(String, f64)>,
    pub time: f64,
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var("WINDIR")
        .map(|w| PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\Fonts"));
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, file) in [("regular", "segoeui.ttf"), ("semibold", "seguisb.ttf"), ("bold", "segoeuib.ttf")] {
        let mut fam = base.clone();
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            fam.insert(0, name.into());
        }
        let key = if name == "regular" { FontFamily::Proportional } else { FontFamily::Name(name.into()) };
        fonts.families.insert(key, fam);
    }
    ctx.set_fonts(fonts);
}

const DEFAULT_PALETTE: [[f32; 3]; 4] = [[0.36, 0.30, 0.62], [0.20, 0.32, 0.58], [0.55, 0.25, 0.42], [0.25, 0.22, 0.40]];

/// Repaints on a timer: ~30 fps for animated backgrounds (petals, stars,
/// fullscreen colours), a few times a second while music plays (progress,
/// lyrics), and not at all otherwise.
fn start_ticker(ctx: &egui::Context) -> Arc<AtomicU64> {
    let ms = Arc::new(AtomicU64::new(0));
    let (iv, ctx) = (ms.clone(), ctx.clone());
    std::thread::Builder::new()
        .name("frame-ticker".into())
        .spawn(move || loop {
            let m = iv.load(Ordering::Relaxed);
            if m == 0 {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            let want = Duration::from_millis(m);
            let start = std::time::Instant::now();
            if m <= 100 {
                // Animations: wait a whole number of screen refreshes, so every
                // frame stays up equally long (a free-running timer drifts
                // against the refresh and the motion shimmers).
                let n = crate::win::refreshes_for(want);
                for _ in 0..n {
                    if !crate::win::wait_for_refresh() {
                        break;
                    }
                }
            }
            if let Some(rest) = want.checked_sub(start.elapsed()) {
                // No refresh signal (or slow ticks): plain sleep.
                if start.elapsed() < want / 2 {
                    std::thread::sleep(rest);
                }
            }
            // Not request_repaint(): egui paints a zero-delay request twice,
            // which put two frames 3 ms apart on every tick.
            ctx.request_repaint_after(Duration::from_micros(1));
        })
        .expect("spawn ticker");
    ms
}

fn resize_handles(ctx: &egui::Context, rect: Rect) {
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let e = 5.0;
    let (l, r, t, b) = (p.x < rect.left() + e, p.x > rect.right() - e, p.y < rect.top() + e, p.y > rect.bottom() - e);
    use ResizeDirection::*;
    let dir = match (l, r, t, b) {
        (true, _, true, _) => NorthWest,
        (_, true, true, _) => NorthEast,
        (true, _, _, true) => SouthWest,
        (_, true, _, true) => SouthEast,
        (true, ..) => West,
        (_, true, ..) => East,
        (_, _, true, _) => North,
        (_, _, _, true) => South,
        _ => return,
    };
    ctx.set_cursor_icon(match dir {
        NorthWest | SouthEast => CursorIcon::ResizeNwSe,
        NorthEast | SouthWest => CursorIcon::ResizeNeSw,
        West | East => CursorIcon::ResizeHorizontal,
        North | South => CursorIcon::ResizeVertical,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

fn drag_window(ctx: &egui::Context, resp: &egui::Response) {
    if resp.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
    }
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, t), a.max.lerp(b.max, t))
}

pub fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

struct NavLayout {
    rows: Vec<(NavKey, Rect)>,
    logo: Rect,
    header: Rect,
    plus: Pos2,
    list_clip: Rect,
    divider_y: f32,
    collapse: Pos2,
}

impl Starlite {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let gl = cc.gl.as_ref().expect("Starlite needs the glow renderer");
        let hwnd = cc.window_handle().ok().and_then(|h| match h.as_raw() {
            RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
            _ => None,
        });
        egui_extras::install_image_loaders(&cc.egui_ctx);
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.repaint_on_widget_change = false);
        let audio = Audio::new(cc.egui_ctx.clone());
        let player = Player::new();
        audio.set_volume(player.effective_volume());
        let (jobs_tx, jobs_rx) = channel();
        let nav_open = std::env::var("STARLITE_NAV").as_deref() != Ok("mini");
        let mut app = Self {
            renderer: Arc::new(Mutex::new(GlassRenderer::new(gl))),
            sound: Sound::new(),
            hwnd,
            theme: Theme::load(),
            tree_ppp: None,
            ticker_ms: start_ticker(&cc.egui_ctx),
            icons: Icons::default(),
            logo: Default::default(),
            lib: Library::load(),
            audio,
            yt_key_input: String::new(),
            card: None,
            fullscreen: false,
            fs_t: 0.0,
            lyrics: None,
            lyric_scroll: 0.0,
            palette: DEFAULT_PALETTE,
            palette_target: DEFAULT_PALETTE,
            palette_for: None,
            yt: Arc::new(YouTube::new()),
            nd: Arc::new(crate::navidrome::Navidrome::load()),
            ndst: NdState::default(),
            // Debug: STARLITE_SETTINGS_TAB=0..3 opens that Settings tab.
            settings_tab: std::env::var("STARLITE_SETTINGS_TAB").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
            discord: crate::discord::Discord::start(),
            dc_id_input: crate::discord::DiscordPrefs::load().app_id,
            dprefs: crate::discord::DiscordPrefs::load(),
            ytp: None,
            now: None,
            yt_busy: false,
            yt_status: None,
            jobs_tx,
            jobs_rx,
            // Debug: STARLITE_PAGE=settings|liked|downloaded opens that page.
            view: match std::env::var("STARLITE_PAGE").as_deref() {
                Ok("settings") => View::Settings,
                Ok("liked") => View::Playlist(LIKED_ID.into()),
                Ok("downloaded") => View::Playlist(DOWNLOADED_ID.into()),
                Ok("navidrome") => View::Navidrome,
                _ => View::Explore,
            },
            sel: None,
            sel_v: 0.0,
            sel_vis: 0.0,
            nav_open,
            nav_t: if nav_open { 1.0 } else { 0.0 },
            ptr_glow: 0.0,
            nav_scroll: 0.0,
            explore: ExploreState::default(),
            remote: None,
            queue: Vec::new(),
            qi: 0,
            last_finished: 0,
            last_status: Status::Idle,
            player,
            dialog: None,
            popover: None,
            toast: None,
            time: 0.0,
            mini: crate::mini::Mini::new(),
        };
        app.yt_key_input = app.yt.api_key();
        if let Some(h) = hwnd {
            match YtPlayer::new(h, cc.egui_ctx.clone()) {
                Ok(p) => app.ytp = Some(p),
                Err(e) => app.toast(e),
            }
        }
        // Debug: STARLITE_DIALOG=1 opens the new-playlist dialog.
        if std::env::var("STARLITE_DIALOG").is_ok() {
            app.open_dialog(None);
        }
        // Debug: STARLITE_FULLSCREEN=1 opens fullscreen Now Playing (with autoplay).
        app.fullscreen = std::env::var("STARLITE_FULLSCREEN").is_ok();
        // Debug: STARLITE_AUTOPLAY=1 plays the first downloaded song.
        if std::env::var("STARLITE_AUTOPLAY").is_ok() && !app.lib.downloaded.is_empty() {
            let list = app.lib.downloaded.clone();
            app.play_list(list, 0);
        }
        app.apply_theme(&cc.egui_ctx);
        if app.yt.configured() {
            app.load_explore(&cc.egui_ctx);
        }
        // Debug: STARLITE_ND_CONNECT=url|user|password connects to Navidrome at start.
        if let Ok(v) = std::env::var("STARLITE_ND_CONNECT") {
            let mut it = v.splitn(3, '|');
            app.ndst.url = it.next().unwrap_or_default().to_string();
            app.ndst.user = it.next().unwrap_or_default().to_string();
            app.ndst.pass = it.next().unwrap_or_default().to_string();
            app.connect_navidrome(&cc.egui_ctx);
        }
        // Debug: STARLITE_YT_VIDEO=<id> starts that YouTube video.
        if let Ok(id) = std::env::var("STARLITE_YT_VIDEO") {
            let t = Track { id: format!("youtube:{id}"), title: "YouTube test".into(), artist: "YouTube".into(), album: None, artwork: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")), permalink: Some(format!("https://www.youtube.com/watch?v={id}")), duration_ms: 0, source: crate::library::Source::YouTube { video_id: id } };
            app.play_list(vec![t], 0);
        }
        app
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(if self.theme.is_light() { egui::Theme::Light } else { egui::Theme::Dark });
        if let Some(h) = self.hwnd {
            crate::win::style_window(h, self.theme.is_light());
        }
    }

    // ── Background jobs ────────────────────────────────────────────

    pub fn load_explore(&mut self, ctx: &egui::Context) {
        if self.explore.loading {
            return;
        }
        self.explore.loading = true;
        self.explore.error = None;
        let (yt, tx, ctx) = (self.yt.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Job::Explore(yt.explore()));
            ctx.request_repaint();
        });
    }

    pub fn open_yt_playlist(&mut self, ctx: &egui::Context, pl: RemotePlaylist) {
        let id = pl.id.clone();
        self.remote = Some(RemoteView { pl, navidrome: false, tracks: None });
        self.view = View::YtPlaylist;
        let (yt, tx, ctx) = (self.yt.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = yt.playlist_tracks(&id);
            let _ = tx.send(Job::YtPlaylist(id, r));
            ctx.request_repaint();
        });
    }

    /// Checks the Navidrome login on a worker thread.
    pub fn connect_navidrome(&mut self, ctx: &egui::Context) {
        if self.ndst.busy {
            return;
        }
        self.ndst.busy = true;
        self.ndst.error = None;
        let (nd, tx, ctx) = (self.nd.clone(), self.jobs_tx.clone(), ctx.clone());
        let (url, user, pass) = (self.ndst.url.clone(), self.ndst.user.clone(), self.ndst.pass.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Job::NdConnect(nd.connect(&url, &user, &pass)));
            ctx.request_repaint();
        });
    }

    pub fn load_navidrome(&mut self, ctx: &egui::Context) {
        if self.ndst.loading {
            return;
        }
        self.ndst.loading = true;
        let (nd, tx, ctx) = (self.nd.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Job::NdPlaylists(nd.playlists()));
            ctx.request_repaint();
        });
    }

    pub fn open_nd_playlist(&mut self, ctx: &egui::Context, pl: RemotePlaylist) {
        let id = pl.id.clone();
        self.remote = Some(RemoteView { pl, navidrome: true, tracks: None });
        self.view = View::YtPlaylist;
        let (nd, tx, ctx) = (self.nd.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = nd.playlist_tracks(&id);
            let _ = tx.send(Job::YtPlaylist(id, r));
            ctx.request_repaint();
        });
    }

    /// Plays a list in random order.
    pub fn shuffle_play(&mut self, mut list: Vec<Track>) {
        if list.is_empty() {
            return;
        }
        crate::youtube::Rng::seeded().shuffle(&mut list);
        self.play_list(list, 0);
        self.toast("Shuffling");
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(job) = self.jobs_rx.try_recv() {
            match job {
                Job::Explore(r) => {
                    self.explore.loading = false;
                    self.explore.loaded = true;
                    match r {
                        Ok((charts, pls)) => {
                            self.explore.charts = charts;
                            self.explore.playlists = pls;
                            self.explore.error = None;
                            self.explore.chart_scroll_to = 0.0;
                            self.explore.pl_scroll_to = 0.0;
                        }
                        Err(e) => self.explore.error = Some(e),
                    }
                }
                Job::YtPlaylist(id, r) => {
                    if let Some(v) = self.remote.as_mut().filter(|v| v.pl.id == id) {
                        if let (Ok(list), Ok(n)) = (&r, std::env::var("STARLITE_ND_PLAY")) {
                            if v.navidrome {
                                let list = list.clone();
                                v.tracks = Some(r);
                                self.play_list(list, n.parse().unwrap_or(0));
                                continue;
                            }
                        }
                        v.tracks = Some(r);
                    }
                }
                Job::Palette(id, pal) => {
                    if self.palette_for.as_deref() == Some(id.as_str()) {
                        self.palette_target = pal.unwrap_or(DEFAULT_PALETTE);
                    }
                }
                Job::Lyrics(id, ly) => {
                    if self.now.as_ref().is_some_and(|t| t.id == id) {
                        self.lyrics = Some((id, ly));
                        self.lyric_scroll = 0.0;
                    }
                }
                Job::YtSignIn(r) => {
                    self.yt_busy = false;
                    match r {
                        Ok(name) => {
                            self.yt_status = Some((true, format!("Signed in as {name}")));
                            self.explore = ExploreState::default();
                            self.load_explore(ctx);
                        }
                        Err(e) => self.yt_status = Some((false, e)),
                    }
                }
                Job::NdConnect(r) => {
                    self.ndst.busy = false;
                    match r {
                        Ok(()) => {
                            self.ndst.pass.clear();
                            self.ndst.error = None;
                            self.ndst.loaded = false;
                            self.load_navidrome(ctx);
                        }
                        Err(e) => self.ndst.error = Some(e),
                    }
                }
                Job::NdPlaylists(r) => {
                    self.ndst.loading = false;
                    self.ndst.loaded = true;
                    match r {
                        Ok(p) => {
                            // Debug: STARLITE_ND_PLAY=<n> opens the first playlist and plays song n.
                            if std::env::var("STARLITE_ND_PLAY").is_ok() {
                                if let Some(first) = p.first().cloned() {
                                    self.open_nd_playlist(ctx, first);
                                }
                            }
                            self.ndst.playlists = p;
                            self.ndst.error = None;
                        }
                        Err(e) => self.ndst.error = Some(e),
                    }
                }
                Job::YtKeyTest(r) => {
                    self.yt_busy = false;
                    match r {
                        Ok(()) => {
                            self.yt_status = Some((true, "API key works".into()));
                            self.explore = ExploreState::default();
                            self.load_explore(ctx);
                        }
                        Err(e) => self.yt_status = Some((false, e)),
                    }
                }
            }
        }
    }

    // ── Playback ───────────────────────────────────────────────────

    pub fn play_list(&mut self, list: Vec<Track>, i: usize) {
        if i >= list.len() {
            return;
        }
        self.queue = list;
        self.qi = i;
        self.play_track(self.queue[i].clone());
    }

    /// Start a song on the right engine: YouTube's player or the audio engine.
    pub fn play_track(&mut self, t: Track) {
        self.now = Some(t.clone());
        if let Some(id) = t.youtube_id() {
            self.audio.stop();
            let vol = self.player.effective_volume();
            match self.ytp.as_mut() {
                Some(p) => p.load(id, vol),
                None => self.toast("The YouTube player isn't available (WebView2 missing?)"),
            }
        } else {
            if let Some(p) = self.ytp.as_mut() {
                p.stop();
            }
            let stream = match &t.source {
                Source::Navidrome { song_id, suffix } => {
                    let u = self.nd.stream_url(song_id, suffix);
                    if u.is_none() {
                        self.toast("Connect to Navidrome to play this song");
                    }
                    u
                }
                _ => None,
            };
            self.audio.play(t, stream);
        }
    }

    /// (position, length) in seconds of whatever is playing.
    pub fn playback_position(&self) -> (f64, f64) {
        if self.now_is_youtube() {
            self.ytp.as_ref().map(|p| p.position()).unwrap_or((0.0, 0.0))
        } else if self.now.is_some() {
            self.audio.position()
        } else {
            (0.0, 0.0)
        }
    }

    pub fn seek_to(&mut self, secs: f64) {
        if self.now_is_youtube() {
            if let Some(p) = self.ytp.as_mut() {
                p.seek(secs);
            }
        } else {
            self.audio.seek((secs.max(0.0) * 1000.0) as u64);
        }
    }

    /// New song: fetch its cover colours and (for files) its lyrics.
    fn on_track_changed(&mut self, ctx: &egui::Context) {
        let Some(t) = self.now.clone() else { return };
        self.palette_for = Some(t.id.clone());
        self.lyrics = None;
        self.lyric_scroll = 0.0;
        let (tx, c) = (self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let pal = t.artwork.as_deref().and_then(crate::nowplaying::cover_bytes).and_then(|b| crate::nowplaying::palette_from(&b));
            let _ = tx.send(Job::Palette(t.id.clone(), pal));
            if let Source::File { path } = &t.source {
                let _ = tx.send(Job::Lyrics(t.id.clone(), crate::meta::lyrics(path)));
            } else {
                let _ = tx.send(Job::Lyrics(t.id.clone(), None));
            }
            c.request_repaint();
        });
    }

    /// Minimize / maximize / close, top-right.
    #[allow(clippy::too_many_arguments)]
    fn window_buttons(&mut self, ui: &egui::Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, full: Rect) {
        let ctx = ui.ctx().clone();
        let theme = self.theme;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let y = 24.0;
        let items: [(&'static str, &str); 3] = [("x", "Close"), (if maximized { "copy" } else { "square" }, if maximized { "Restore" } else { "Maximize" }), ("minus", "Minimize")];
        for (i, (icon, tip)) in items.iter().enumerate() {
            let c = pos2(full.right() - 26.0 - i as f32 * 38.0, y);
            let id = ui.id().with(("winbtn", i));
            let resp = ui.interact(Rect::from_center_size(c, Vec2::splat(30.0)), id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text(*tip);
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.14);
            let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.09);
            if h > 0.001 {
                let size = 26.0 + 4.0 * h + 3.0 * pr;
                els.push(El::rect(Rect::from_center_size(c, Vec2::splat(size)), ppp, size * 0.5, h, 3.0, Kind::Glass, 0.45 * h, pr));
            }
            let tint = if i == 0 && h > 0.01 { Color32::from_rgb(232, 72, 85) } else { theme.text_dim() };
            icons.paint(ui, icon, c, if *icon == "square" || *icon == "copy" { 13.0 } else { 15.0 }, tint);
            if resp.clicked() {
                match i {
                    0 => ctx.send_viewport_cmd(ViewportCommand::Close),
                    1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
                    _ => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                }
            }
        }
    }

    fn now_is_youtube(&self) -> bool {
        self.now.as_ref().is_some_and(|t| t.youtube_id().is_some())
    }

    pub fn playback_status(&self) -> Status {
        if self.now_is_youtube() {
            self.ytp.as_ref().map(|p| p.status.clone()).unwrap_or(Status::Idle)
        } else {
            self.audio.status()
        }
    }

    pub fn sign_in_youtube(&mut self, ctx: &egui::Context) {
        if self.yt_busy {
            return;
        }
        self.yt_busy = true;
        self.yt_status = Some((true, "Finish signing in in your browser…".into()));
        let (yt, tx, ctx) = (self.yt.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let c2 = ctx.clone();
            let r = yt.sign_in(move |url| {
                c2.open_url(egui::OpenUrl::new_tab(url));
                c2.request_repaint();
            });
            let _ = tx.send(Job::YtSignIn(r));
            ctx.request_repaint();
        });
    }

    pub fn save_youtube_key(&mut self, ctx: &egui::Context) {
        self.yt.set_api_key(&self.yt_key_input);
        if self.yt_key_input.trim().is_empty() {
            self.yt_status = None;
            return;
        }
        self.yt_busy = true;
        self.yt_status = None;
        let (yt, tx, ctx) = (self.yt.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Job::YtKeyTest(yt.test()));
            ctx.request_repaint();
        });
    }

    fn step(&mut self, forward: bool, auto: bool) {
        if self.queue.is_empty() {
            return;
        }
        let n = self.queue.len();
        if auto && self.player.looping == Loop::One {
            // replay the same song
        } else if forward {
            if self.qi + 1 < n {
                self.qi += 1;
            } else if !auto || self.player.looping == Loop::All {
                self.qi = 0;
            } else {
                return; // end of the queue
            }
        } else {
            self.qi = self.qi.saturating_sub(1);
        }
        self.play_track(self.queue[self.qi].clone());
    }

    pub fn current_track(&self) -> Option<Track> {
        self.now.clone()
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.time));
    }

    pub fn open_dialog(&mut self, add_after: Option<Track>) {
        self.popover = None;
        self.dialog = Some(Dialog { name: String::new(), cover: Cover::Preset("disc-3".into()), error: None, open_t: 0.0, add_after });
    }

    // ── Navigation ─────────────────────────────────────────────────

    fn nav_layout(&self, w: f32, h: f32, t: f32) -> NavLayout {
        let full = |y: f32| Rect::from_min_size(pos2(12.0, y), vec2(w - 24.0, 38.0));
        let mini = |y: f32| Rect::from_min_size(pos2(w * 0.5 - 23.0, y), Vec2::splat(46.0));
        let row = |ye: f32, ym: f32| lerp_rect(mini(ym), full(ye), t);

        let logo = lerp_rect(Rect::from_center_size(pos2(w * 0.5, 46.0), Vec2::splat(56.0)), Rect::from_min_size(pos2(18.0, 14.0), Vec2::splat(74.0)), t);
        let mut rows = vec![(NavKey::Explore, row(98.0, 92.0))];
        let nd_y = lerp(144.0, 140.0, t);
        rows.push((NavKey::Navidrome, row(nd_y, nd_y)));
        let shift = lerp(52.0, 46.0, t);
        let header = Rect::from_min_size(pos2(24.0, lerp(146.0, 150.0, t) + shift), vec2((w - 48.0).max(1.0), 22.0));
        let plus = pos2(lerp(w * 0.5, w - 30.0, t), lerp(158.0, 161.0, t) + shift);

        let collapse = pos2(lerp(w * 0.5, 36.0, t), h - 30.0);
        let settings_y = h - 52.0 - lerp(46.0, 38.0, t);
        let settings = row(settings_y, settings_y);
        let divider_y = settings.top() - 9.0;
        let dl_y = divider_y - 9.0 - lerp(46.0, 38.0, t);
        let dl = row(dl_y, dl_y);
        let liked_y = dl.top() - lerp(52.0, 42.0, t);
        let liked = row(liked_y, liked_y);

        let list_top = lerp(184.0, 178.0, t) + shift;
        let list_clip = Rect::from_min_max(pos2(0.0, list_top - 2.0), pos2(w, (liked.top() - 10.0).max(list_top)));
        let pitch = lerp(52.0, 42.0, t);
        for (i, p) in self.lib.user_playlists().enumerate() {
            let y = list_top + i as f32 * pitch - self.nav_scroll;
            rows.push((NavKey::Playlist(p.id.clone()), row(y, y)));
        }
        rows.push((NavKey::Liked, liked));
        rows.push((NavKey::Downloaded, dl));
        rows.push((NavKey::Settings, settings));
        NavLayout { rows, logo, header, plus, list_clip, divider_y, collapse }
    }

    fn key_for_view(&self) -> Option<NavKey> {
        match &self.view {
            View::Explore => Some(NavKey::Explore),
            View::Settings => Some(NavKey::Settings),
            View::Playlist(id) if id == LIKED_ID => Some(NavKey::Liked),
            View::Playlist(id) if id == DOWNLOADED_ID => Some(NavKey::Downloaded),
            View::Playlist(id) => Some(NavKey::Playlist(id.clone())),
            View::Navidrome => Some(NavKey::Navidrome),
            View::YtPlaylist if self.remote.as_ref().is_some_and(|r| r.navidrome) => Some(NavKey::Navidrome),
            View::YtPlaylist => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_nav(&mut self, ui: &mut egui::Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, dt: f32, full: Rect, nav_w: f32, animating: &mut bool) {
        let ctx = ui.ctx().clone();
        let theme = self.theme;
        let t = smooth(self.nav_t);
        let lay = self.nav_layout(nav_w, full.height(), t);
        let label_a = ((self.nav_t - 0.55) / 0.45).clamp(0.0, 1.0);
        let p = ui.painter().clone();

        // Scroll the playlist list if it overflows.
        let n = self.lib.user_playlists().count() as f32;
        let content_h = n * lerp(52.0, 42.0, t);
        let max_scroll = (content_h - lay.list_clip.height() + 6.0).max(0.0);
        if ui.rect_contains_pointer(lay.list_clip) {
            let d = ctx.input(|i| i.smooth_scroll_delta.y);
            self.nav_scroll = (self.nav_scroll - d).clamp(0.0, max_scroll);
        }
        self.nav_scroll = self.nav_scroll.min(max_scroll);

        // Logo (top-left): the light-theme version on Light, else the dark one.
        self.logo.paint(ui, lay.logo, theme.is_light(), 1.0);

        // "Playlists" section header with the + (new playlist) button.
        if label_a > 0.0 {
            p.text(lay.header.left_center(), Align2::LEFT_CENTER, "PLAYLISTS", font(11.0, "semibold"), theme.text_dim().gamma_multiply(label_a));
        }
        if icon_button(ui, icons, els, ppp, ui.id().with("nav_plus"), lay.plus, 28.0, "plus", false, true, theme)
            .on_hover_text("New playlist")
            .clicked()
        {
            self.sound.click();
            self.open_dialog(None);
        }

        // Divider between Liked / Downloaded and Settings.
        let inset = lerp(18.0, 24.0, t);
        p.line_segment([pos2(inset, lay.divider_y), pos2(nav_w - inset, lay.divider_y)], Stroke::new(1.0_f32, theme.hairline().gamma_multiply(0.8)));

        let selected = self.key_for_view();
        let mut hover_el = None;
        let mut sel_press = 0.0;
        let mut clicked: Option<NavKey> = None;
        for (key, rect) in &lay.rows {
            let in_list = matches!(key, NavKey::Playlist(_));
            if in_list && (rect.bottom() < lay.list_clip.top() || rect.top() > lay.list_clip.bottom()) {
                continue;
            }
            let id = ui.id().with(("nav", format!("{key:?}")));
            let hit = if in_list { rect.intersect(lay.list_clip) } else { *rect };
            let resp = ui.interact(hit, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let is_sel = selected.as_ref() == Some(key);
            let h = ctx.animate_bool_with_time(id.with("h"), resp.hovered() && !is_sel, 0.16);
            let pr = ctx.animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.10);
            if is_sel {
                sel_press = pr;
            } else if h > 0.001 {
                let mut r = Rect::from_center_size(rect.center(), rect.size() * (0.92 + 0.08 * h) + Vec2::splat(4.0 * pr));
                if in_list {
                    r = r.intersect(lay.list_clip);
                }
                hover_el = Some(El::rect(r, ppp, 12.0, h, 3.0, Kind::Layered, 0.4 * h, pr));
            }
            if resp.clicked() && !is_sel {
                clicked = Some(key.clone());
            }

            let name: String = match key {
                NavKey::Explore => "Explore".into(),
                NavKey::Navidrome => "Navidrome".into(),
                NavKey::Settings => "Settings".into(),
                NavKey::Liked => "Liked Music".into(),
                NavKey::Downloaded => "Downloaded Music".into(),
                NavKey::Playlist(pid) => self.lib.playlists.iter().find(|p| &p.id == pid).map(|p| p.name.clone()).unwrap_or_default(),
            };
            if label_a < 0.5 {
                resp.on_hover_text(name.clone());
            }
            let tint = if is_sel { theme.accent() } else { theme.text_dim() };
            let tc = if is_sel { theme.text() } else { theme.text_dim() };
            let lead_c = pos2(lerp(rect.center().x, rect.left() + 22.0, t), rect.center().y);
            let icon: Option<&'static str> = match key {
                NavKey::Explore => Some("compass"),
                NavKey::Navidrome => Some("cloudy"),
                NavKey::Settings => Some("settings"),
                NavKey::Liked => Some(if is_sel { "heart-fill" } else { "heart" }),
                NavKey::Downloaded => Some("download"),
                NavKey::Playlist(pid) => {
                    if let Some(pl) = self.lib.playlists.iter().find(|p| &p.id == pid) {
                        let c = Rect::from_center_size(lead_c, Vec2::splat(lerp(34.0, 26.0, t)));
                        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(lay.list_clip));
                        cu.set_clip_rect(lay.list_clip);
                        let cover = pl.cover.clone();
                        playlist_cover(&cu, icons, c, &cover, 7.0, theme);
                    }
                    None
                }
            };
            if let Some(ic) = icon {
                icons.paint(ui, ic, lead_c, lerp(22.0, 18.0, t), tint);
            }
            if label_a > 0.0 {
                let pp = if in_list { p.with_clip_rect(lay.list_clip) } else { p.clone() };
                text_trunc(&pp, pos2(rect.left() + 42.0, rect.center().y), Align2::LEFT_CENTER, &name, font(14.5, "semibold"), tc.gamma_multiply(label_a), rect.width() - 50.0);
            }
        }
        if let Some(h) = hover_el {
            els.push(h);
        }

        // Selection pill glides between rows on a spring, stretching like gel.
        let target = selected.as_ref().and_then(|k| lay.rows.iter().find(|(kk, _)| kk == k)).map(|(_, r)| *r);
        let vis_target = if target.is_some() { 1.0 } else { 0.0 };
        self.sel_vis += (vis_target - self.sel_vis) * (dt * 12.0).min(1.0);
        if (self.sel_vis - vis_target).abs() > 0.002 {
            *animating = true;
        }
        if let Some(tr) = target {
            let cur = self.sel.get_or_insert(tr);
            let dy = tr.center().y - cur.center().y;
            let acc = dy * 340.0 - self.sel_v * 26.0;
            self.sel_v += acc * dt;
            let ny = cur.center().y + self.sel_v * dt;
            let k = (dt * 16.0).min(1.0);
            let size = cur.size() + (tr.size() - cur.size()) * k;
            let cx = cur.center().x + (tr.center().x - cur.center().x) * k;
            *cur = Rect::from_center_size(pos2(cx, ny), size);
            if dy.abs() < 0.05 && self.sel_v.abs() < 0.5 && (tr.size() - cur.size()).length() < 0.05 && (tr.center().x - cx).abs() < 0.05 {
                *cur = tr;
                self.sel_v = 0.0;
            } else {
                *animating = true;
            }
        }
        if let Some(cur) = self.sel {
            let stretch = (self.sel_v.abs() * 0.03).min(16.0);
            let squeeze = (self.sel_v.abs() * 0.008).min(6.0);
            let mut pill_r = Rect::from_center_size(cur.center(), vec2(cur.width() - squeeze + 4.0 * sel_press, cur.height() + stretch + 4.0 * sel_press));
            if matches!(selected, Some(NavKey::Playlist(_))) {
                pill_r = pill_r.intersect(lay.list_clip);
            }
            if self.sel_vis > 0.002 && pill_r.width() > 1.0 && pill_r.height() > 1.0 {
                els.push(El::rect(pill_r, ppp, 12.0, self.sel_vis, 3.0, Kind::Layered, 0.2, sel_press));
            }
        }

        // Collapse / expand arrow, under Settings. Points the way it'll move.
        let cr = ui.interact(Rect::from_center_size(lay.collapse, Vec2::splat(30.0)), ui.id().with("nav_collapse"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let ch = ctx.animate_bool_with_time(ui.id().with("nav_collapse_h"), cr.hovered(), 0.16);
        if ch > 0.001 {
            els.push(El::rect(Rect::from_center_size(lay.collapse, Vec2::splat(26.0 + 4.0 * ch)), ppp, 15.0, ch, 3.0, Kind::Layered, 0.4 * ch, 0.0));
        }
        icons.paint_rotated(ui, "chevron-left", lay.collapse, 16.0, theme.text_dim(), std::f32::consts::PI * (1.0 - t));
        if cr.on_hover_text(if self.nav_open { "Collapse sidebar" } else { "Expand sidebar" }).clicked() {
            self.sound.click();
            self.nav_open = !self.nav_open;
        }

        if let Some(k) = clicked {
            self.sound.click();
            self.view = match k {
                NavKey::Explore => View::Explore,
                NavKey::Navidrome => {
                    if self.nd.connected().is_some() && !self.ndst.loaded {
                        self.load_navidrome(&ctx);
                    }
                    View::Navidrome
                }
                NavKey::Settings => View::Settings,
                NavKey::Liked => View::Playlist(LIKED_ID.into()),
                NavKey::Downloaded => {
                    self.lib.scan_downloads();
                    View::Playlist(DOWNLOADED_ID.into())
                }
                NavKey::Playlist(id) => View::Playlist(id),
            };
        }
    }

    // ── Overlays ───────────────────────────────────────────────────

    pub fn base_frame(&self, full: Rect, ppp: f32, time: f32, nav_w: f32) -> Frame {
        Frame {
            res: [full.width() * ppp, full.height() * ppp],
            ppp,
            time,
            theme: self.theme.shader_id(),
            els: [El::default(); MAX_EL],
            count: 0,
            light: [-0.70, -0.71],
            pointer: [-1e4, -1e4, 0.0],
            tree_rect: [(nav_w - 40.0 - TREE_SIZE[0] * 0.20) * ppp, (full.bottom() - TREE_SIZE[1]) * ppp, TREE_SIZE[0] * ppp, TREE_SIZE[1] * ppp],
            overlay: false,
            dim: 0.0,
            fs: smooth(self.fs_t),
            palette: self.palette,
        }
    }

    pub fn set_frame(&self, ui: &egui::Ui, slot: egui::layers::ShapeIdx, full: Rect, mut frame: Frame, els: &[El]) {
        frame.count = els.len().min(MAX_EL);
        if std::env::var("STARLITE_TIMING").is_ok() {
            eprintln!("glass els={}", els.len());
        }
        for (d, s) in frame.els.iter_mut().zip(els) {
            *d = *s;
        }
        let r = self.renderer.clone();
        ui.painter().set(
            slot,
            egui::PaintCallback {
                rect: full,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_info, painter| {
                    r.lock().unwrap().paint(painter.gl(), &frame);
                })),
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_dialog(&mut self, ui: &mut egui::Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let Some(d) = self.dialog.as_mut() else { return };
        d.open_t = (d.open_t + dt * 6.0).min(1.0);
        if d.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(d.open_t);
        let open_t = d.open_t;
        let cover = d.cover.clone();
        let mut name = d.name.clone();
        let error = d.error.clone();

        // Block everything behind; clicking outside cancels.
        let back = ui.interact(full, ui.id().with("dialog_back"), Sense::click());
        let size = vec2(476.0, 420.0);
        // Centre in the space left of the video card (if any).
        let area_right = self.card.map(|c| c.left()).unwrap_or(full.right());
        let center = pos2(((nav_w + area_right) * 0.5).max(full.center().x.min(area_right - size.x * 0.5 - 10.0)), full.center().y);
        let panel = Rect::from_center_size(center + vec2(0.0, 18.0 * (1.0 - ot)), size * (0.94 + 0.06 * ot));
        let mut els: Vec<El> = vec![El::rect(panel, ppp, 28.0, 1.0, 14.0, Kind::Glass, 0.0, 0.0)];
        let mut close = back.clicked() && !back.interact_pointer_pos().is_some_and(|p| panel.contains(p));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }

        let slot = ui.painter().add(egui::Shape::Noop);
        let p = ui.painter().clone();
        let fg = theme.text().gamma_multiply(ot);
        let dim = theme.text_dim().gamma_multiply(ot);
        let x0 = panel.left() + 28.0;

        p.text(pos2(x0, panel.top() + 32.0), Align2::LEFT_CENTER, "New Playlist", font(20.0, "bold"), fg);

        // Cover preview + name.
        let preview = Rect::from_min_size(pos2(x0, panel.top() + 60.0), Vec2::splat(92.0));
        playlist_cover(ui, icons, preview, &cover, 16.0, theme);
        p.text(pos2(preview.right() + 18.0, preview.top() + 12.0), Align2::LEFT_CENTER, "Name", font(12.0, "semibold"), dim);
        let name_rect = Rect::from_min_size(pos2(preview.right() + 18.0, preview.top() + 28.0), vec2(panel.right() - 28.0 - preview.right() - 18.0, 40.0));
        let fresp = field(ui, &mut els, ppp, name_rect, &mut name, "My playlist", false, true, theme);
        if open_t < 0.15 {
            fresp.request_focus();
        }
        let submit = fresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(e) = &error {
            p.text(pos2(name_rect.left(), name_rect.bottom() + 14.0), Align2::LEFT_CENTER, e, font(12.0, "regular"), Color32::from_rgb(235, 90, 100).gamma_multiply(ot));
        }

        // Cover choices: Lucide presets + "choose image".
        let grid_top = preview.bottom() + 26.0;
        p.text(pos2(x0, grid_top), Align2::LEFT_CENTER, "Cover", font(12.0, "semibold"), dim);
        let (tile, gap, cols) = (44.0, 8.0, 8);
        let mut picked: Option<Cover> = None;
        let mut choose_file = false;
        let mut hover_tile = None;
        for i in 0..=PRESETS.len() {
            let (cxi, cyi) = (i % cols, i / cols);
            let r = Rect::from_min_size(pos2(x0 + cxi as f32 * (tile + gap), grid_top + 16.0 + cyi as f32 * (tile + gap)), Vec2::splat(tile));
            let id = ui.id().with(("tile", i));
            let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let h = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.14);
            let (icon, sel) = if i < PRESETS.len() {
                (PRESETS[i], matches!(&cover, Cover::Preset(n) if n == PRESETS[i]))
            } else {
                ("image-plus", matches!(&cover, Cover::Image(_)))
            };
            if sel {
                els.push(El::rect(r, ppp, 13.0, 1.0, 3.0, Kind::Layered, 0.5, 0.0));
            } else if h > 0.001 {
                hover_tile = Some(El::rect(Rect::from_center_size(r.center(), r.size() * (0.9 + 0.1 * h)), ppp, 13.0, h, 2.0, Kind::Layered, 0.3, 0.0));
            }
            icons.paint(ui, icon, r.center(), 20.0, if sel { theme.accent().gamma_multiply(ot) } else { fg });
            let resp = if i == PRESETS.len() { resp.on_hover_text("Choose your own image (PNG or JPG)") } else { resp };
            if resp.clicked() {
                if i < PRESETS.len() {
                    picked = Some(Cover::Preset(PRESETS[i].into()));
                } else {
                    choose_file = true;
                }
            }
        }
        if let Some(h) = hover_tile {
            els.push(h);
        }

        // Buttons.
        let by = panel.bottom() - 28.0 - 40.0;
        let create_r = Rect::from_min_size(pos2(panel.right() - 28.0 - 124.0, by), vec2(124.0, 40.0));
        let cancel_r = Rect::from_min_size(pos2(create_r.left() - 12.0 - 110.0, by), vec2(110.0, 40.0));
        let cancel = pill(ui, icons, &mut els, ppp, ui.id().with("dlg_cancel"), cancel_r, "Cancel", None, false, true, theme);
        let create = pill(ui, icons, &mut els, ppp, ui.id().with("dlg_create"), create_r, "Create", Some("plus"), true, true, theme);

        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        frame.dim = 0.45 * ot;
        self.set_frame(ui, slot, full, frame, &els);

        // Apply input.
        let Some(d) = self.dialog.as_mut() else { return };
        d.name = name;
        if let Some(c) = picked {
            self.sound.click();
            d.cover = c;
        }
        if choose_file {
            self.sound.click();
            if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                match import_cover(&path) {
                    Some(dst) => d.cover = Cover::Image(dst),
                    None => d.error = Some("Couldn't use that image (PNG or JPG only)".into()),
                }
            }
        }
        if cancel.clicked() || close {
            self.sound.click();
            self.dialog = None;
            return;
        }
        if create.clicked() || submit {
            let name = d.name.trim().to_string();
            if name.is_empty() {
                d.error = Some("Give your playlist a name".into());
                return;
            }
            self.sound.click();
            let d = self.dialog.take().unwrap();
            let id = self.lib.create(name.clone(), d.cover);
            if let Some(t) = d.add_after {
                self.lib.add_to(&id, &t);
                self.toast(format!("Added to {name}"));
            }
            self.view = View::Playlist(id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_popover(&mut self, ui: &mut egui::Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let Some(pop) = self.popover.as_mut() else { return };
        pop.open_t = (pop.open_t + dt * 8.0).min(1.0);
        if pop.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(pop.open_t);
        let anchor = pop.anchor;
        let track = pop.track.clone();

        let back = ui.interact(full, ui.id().with("pop_back"), Sense::click());
        let lists: Vec<(String, String, Cover)> = self.lib.user_playlists().take(6).map(|p| (p.id.clone(), p.name.clone(), p.cover.clone())).collect();
        let rows = lists.len() + 1;
        let row_h = 40.0;
        let w = 260.0;
        let h = 44.0 + rows as f32 * row_h + 8.0;
        let max_x = self.card.map(|c| c.left() - w - 10.0).unwrap_or(full.right() - w - 10.0).max(nav_w + 10.0);
        let panel = Rect::from_min_size(pos2((anchor.x - w * 0.5).clamp(nav_w + 10.0, max_x), anchor.y - 30.0 - h + 10.0 * (1.0 - ot)), vec2(w, h));
        let mut els = vec![El::rect(panel, ppp, 20.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let p = ui.painter().clone();
        let fg = theme.text().gamma_multiply(ot);
        p.text(pos2(panel.left() + 18.0, panel.top() + 22.0), Align2::LEFT_CENTER, "Add to playlist", font(13.0, "semibold"), theme.text_dim().gamma_multiply(ot));

        let mut chosen: Option<(String, String)> = None;
        let mut new_pl = false;
        let mut hover_row = None;
        for i in 0..rows {
            let r = Rect::from_min_size(pos2(panel.left() + 8.0, panel.top() + 40.0 + i as f32 * row_h), vec2(w - 16.0, row_h - 4.0));
            let id = ui.id().with(("pop", i));
            let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let hv = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
            if hv > 0.001 {
                hover_row = Some(El::rect(r, ppp, 11.0, hv, 2.0, Kind::Layered, 0.3, 0.0));
            }
            let c = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), Vec2::splat(26.0));
            if let Some((id_s, name, cover)) = lists.get(i) {
                playlist_cover(ui, icons, c, cover, 6.0, theme);
                text_trunc(&p, pos2(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, name, font(13.5, "semibold"), fg, r.width() - 50.0);
                if resp.clicked() {
                    chosen = Some((id_s.clone(), name.clone()));
                }
            } else {
                icons.paint(ui, "plus", c.center(), 17.0, fg);
                p.text(pos2(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, "New playlist…", font(13.5, "semibold"), fg);
                if resp.clicked() {
                    new_pl = true;
                }
            }
        }
        if let Some(hr) = hover_row {
            els.push(hr);
        }
        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        self.set_frame(ui, slot, full, frame, &els);

        if let Some((id, name)) = chosen {
            self.sound.click();
            if self.lib.add_to(&id, &track) {
                self.toast(format!("Added to {name}"));
            } else {
                self.toast(format!("Already in {name}"));
            }
            self.popover = None;
        } else if new_pl {
            self.sound.click();
            self.open_dialog(Some(track));
        } else if (back.clicked() && !back.interact_pointer_pos().is_some_and(|p| panel.contains(p))) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.popover = None;
        }
    }
}

impl Starlite {
    /// Runs player actions (from the bar or the mini player).
    pub fn run_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>, track: Option<Track>) {
        for a in actions {
            match a {
                Action::TogglePlay => {
                    let yt = self.now_is_youtube();
                    match self.playback_status() {
                        Status::Playing if yt => self.ytp.as_ref().map(|p| p.pause()).unwrap_or(()),
                        Status::Paused if yt => self.ytp.as_ref().map(|p| p.play()).unwrap_or(()),
                        Status::Playing => self.audio.pause(),
                        Status::Paused => self.audio.resume(),
                        Status::Loading => {}
                        _ => {
                            if let Some(t) = track.clone() {
                                self.play_track(t);
                            }
                        }
                    }
                }
                Action::Next => self.step(true, false),
                Action::Prev => self.step(false, false),
                Action::Like => {
                    if let Some(t) = &track {
                        let was = self.lib.is_liked(t);
                        self.lib.toggle_like(t);
                        self.toast(if was { "Removed from Liked Music" } else { "Added to Liked Music" });
                    }
                }
                Action::AddTo(anchor) => {
                    if let Some(t) = track.clone() {
                        if self.lib.user_playlists().next().is_none() {
                            self.open_dialog(Some(t));
                        } else {
                            self.popover = Some(Popover { anchor, track: t, open_t: 0.0 });
                        }
                    }
                }
                Action::OpenLink(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                Action::Seek(s) => self.seek_to(s),
                Action::ToggleFullscreen => {
                    self.fullscreen = !self.fullscreen && self.now.is_some();
                    self.popover = None;
                }
                Action::Volume(v) => {
                    self.audio.set_volume(v);
                    if let Some(p) = &self.ytp {
                        p.set_volume(v);
                    }
                }
            }
        }
    }

}

impl eframe::App for Starlite {
    fn update(&mut self, ctx: &egui::Context, frame_info: &mut eframe::Frame) {
        if std::env::var("STARLITE_TIMING").is_ok() {
            let dt_ms = ctx.input(|i| i.unstable_dt) * 1000.0;
            let cpu_ms = frame_info.info().cpu_usage.unwrap_or(0.0) * 1000.0;
            eprintln!("frame dt={dt_ms:.1}ms app+paint={cpu_ms:.2}ms causes={:?}", ctx.repaint_causes());
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(1.0 / 240.0, 1.0 / 20.0);
        self.time = ctx.input(|i| i.time);
        let time = self.time as f32;
        let ppp = ctx.pixels_per_point();
        let full = ctx.screen_rect();
        let theme = self.theme;
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let mut animating = false;
        let mut icons = std::mem::take(&mut self.icons);

        self.poll_jobs(ctx);

        // Playback events: song ended → next; errors → toast.
        let finished = self.audio.finished();
        if finished != self.last_finished {
            self.last_finished = finished;
            if !self.now_is_youtube() {
                self.step(true, true);
            }
        }
        // Any click that reaches Starlite (clicks on the video go to the
        // player itself) takes keyboard focus back for text fields.
        if ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(p) = &self.ytp {
                p.focus_app();
            }
        }
        let yt_events = self.ytp.as_mut().map(|p| p.poll()).unwrap_or_default();
        for e in yt_events {
            match e {
                YtEvent::Ended => self.step(true, true),
                // Unplayable video (e.g. embedding disabled): skip ahead.
                YtEvent::Error if self.queue.len() > 1 => {
                    self.qi = (self.qi + 1) % self.queue.len();
                    let t = self.queue[self.qi].clone();
                    self.play_track(t);
                }
                YtEvent::Error => {}
            }
        }
        if self.now.as_ref().map(|t| &t.id) != self.palette_for.as_ref() {
            self.on_track_changed(ctx);
        }
        // Ease the background palette towards the new cover's colours.
        for (c, t) in self.palette.iter_mut().zip(self.palette_target) {
            for k in 0..3 {
                c[k] += (t[k] - c[k]) * (dt * 2.5).min(1.0);
            }
        }
        // Fullscreen Now Playing (Esc leaves it).
        if self.now.is_none() {
            self.fullscreen = false;
        }
        if self.fullscreen && self.dialog.is_none() && self.popover.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.fullscreen = false;
        }
        let fs_target = if self.fullscreen { 1.0 } else { 0.0 };
        self.fs_t += (fs_target - self.fs_t) * (dt * 7.0).min(1.0);
        if (self.fs_t - fs_target).abs() < 0.003 {
            self.fs_t = fs_target;
        } else {
            animating = true;
        }
        let fs = smooth(self.fs_t);
        // Debug: STARLITE_SEEK=<secs> jumps there once playback starts.
        static SEEK_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if let Ok(v) = std::env::var("STARLITE_SEEK") {
            if self.playback_status() == Status::Playing && !SEEK_DONE.swap(true, Ordering::Relaxed) {
                self.seek_to(v.parse().unwrap_or(0.0));
            }
        }
        // Debug: STARLITE_PAUSE=1 pauses once playback has started.
        static PAUSE_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if std::env::var("STARLITE_PAUSE").is_ok() && self.playback_status() == Status::Playing && !PAUSE_DONE.swap(true, Ordering::Relaxed) {
            self.audio.pause();
        }
        let status = self.playback_status();
        if status != self.last_status {
            if let Status::Error(e) = &status {
                self.toast(e.clone());
            }
            self.last_status = status.clone();
        }

        // Sidebar width animates between full and compact.
        let tgt = if self.nav_open { 1.0 } else { 0.0 };
        self.nav_t += (tgt - self.nav_t) * (dt * 10.0).min(1.0);
        if (self.nav_t - tgt).abs() < 0.002 {
            self.nav_t = tgt;
        } else {
            animating = true;
        }
        let nav_w = lerp(NAV_MINI, NAV_FULL, smooth(self.nav_t));

        if theme == Theme::Cherry && self.tree_ppp != Some(ppp) {
            let img = crate::tree::render(TREE_SIZE[0], TREE_SIZE[1], ppp);
            self.renderer.lock().unwrap().pending_tree = Some(img);
            self.tree_ppp = Some(ppp);
        }

        let in_fs = self.fullscreen || fs > 0.5;
        let bar_left = lerp(nav_w + 18.0, 18.0, fs);
        let bar = Rect::from_min_max(pos2(bar_left, full.bottom() - 18.0 - PLAYER_H), pos2(full.right() - 18.0, full.bottom() - 18.0));
        let nav = if in_fs { Rect::NOTHING } else { Rect::from_min_max(pos2(0.0, 0.0), pos2(nav_w, full.bottom())) };
        let glow_target = if self.dialog.is_none() && pointer.is_some_and(|p| nav.contains(p) || bar.contains(p)) { 0.8 } else { 0.0 };
        self.ptr_glow += (glow_target - self.ptr_glow) * (dt * 8.0).min(1.0);
        animating |= (self.ptr_glow - glow_target).abs() > 0.003;

        let mut els: Vec<El> = Vec::with_capacity(MAX_EL);
        let mut player_out = None;

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
            let slot = ui.painter().add(egui::Shape::Noop);

            if !in_fs {
                let nav_bg = ui.interact(nav, ui.id().with("nav_drag"), Sense::click_and_drag());
                drag_window(ctx, &nav_bg);
            }
            let top = Rect::from_min_max(pos2(if in_fs { 0.0 } else { nav.right() }, 0.0), pos2(full.right(), if in_fs { 48.0 } else { 24.0 }));
            let top_bg = ui.interact(top, ui.id().with("top_drag"), Sense::click_and_drag());
            drag_window(ctx, &top_bg);

            if !in_fs {
                // The nav bar: clear glass over the one window-wide background.
                els.push(El::rect(Rect::from_min_max(pos2(-80.0, -80.0), pos2(nav_w, full.bottom() + 80.0)), ppp, 26.0, 1.0, 6.0, Kind::Glass, 0.0, 0.0).with_value(1.0));
                self.draw_nav(ui, &mut icons, &mut els, ppp, dt, full, nav_w, &mut animating);
            }

            // Page content.
            // YouTube video card, bottom-right above the player bar. YouTube's
            // embed rules want the player visible while it plays.
            self.card = (self.now_is_youtube() && !in_fs).then(|| {
                Rect::from_min_size(
                    pos2(full.right() - 18.0 - VIDEO[0] - 16.0, bar.top() - 14.0 - VIDEO[1] - 16.0),
                    vec2(VIDEO[0] + 16.0, VIDEO[1] + 16.0),
                )
            });
            if let Some(c) = self.card {
                els.push(El::rect(c, ppp, 20.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0));
            }
            let mut video = self.card.map(|c| c.shrink(8.0));
            if in_fs {
                // Fullscreen Now Playing takes over the whole window.
                let area = Rect::from_min_max(pos2(40.0, 56.0), pos2(full.right() - 40.0, bar.top() - 24.0));
                let v = self.now_playing_view(ui, &mut icons, &mut els, ppp, dt, area, fs, &mut animating);
                if let Some(r) = v {
                    els.push(El::rect(r.expand(8.0), ppp, 20.0, 1.0, 12.0, Kind::Glass, 0.0, 0.0));
                }
                video = v;
            } else {
                let right = self.card.map(|c| c.left() - 16.0).unwrap_or(full.right() - 32.0);
                let content = Rect::from_min_max(pos2(nav_w + 32.0, 0.0), pos2(right, bar.top() - 16.0));
                self.draw_page(ui, &mut icons, &mut els, ppp, dt, content, &mut animating);
                // Window buttons (hidden in fullscreen).
                self.window_buttons(ui, &mut icons, &mut els, ppp, full);
            }
            if let Some(p) = self.ytp.as_mut() {
                p.place(video);
            }

            // Player bar.
            let track = self.current_track();
            let liked = track.as_ref().is_some_and(|t| self.lib.is_liked(t));
            let (pos, dur) = self.playback_position();
            let now = NowPlaying {
                track: track.as_ref(),
                playing: matches!(status, Status::Playing | Status::Loading),
                loading: status == Status::Loading,
                liked,
                pos,
                dur,
                fullscreen: self.fullscreen,
            };
            let po = self.player.ui(ui, &mut icons, bar, ppp, dt, theme, &mut els, now);
            animating |= po.animating;
            player_out = Some((po, track));

            // Toast (top centre).
            if let Some((msg, at)) = self.toast.clone() {
                let age = (self.time - at) as f32;
                if age > 2.6 {
                    self.toast = None;
                } else {
                    animating = true;
                    let a = smooth((age / 0.2).min(1.0)) * smooth(((2.6 - age) / 0.3).clamp(0.0, 1.0));
                    let g = ui.painter().layout_no_wrap(msg.clone(), font(13.5, "semibold"), theme.text());
                    let w = (g.size().x + 36.0).min(full.width() - nav_w - 80.0);
                    let cx = (nav_w + full.right()) * 0.5;
                    let r = Rect::from_center_size(pos2(cx, 34.0 - 8.0 * (1.0 - a)), vec2(w, 38.0));
                    els.push(El::rect(r, ppp, 19.0, a, 8.0, Kind::Glass, 0.3, 0.0));
                    text_trunc(ui.painter(), r.center(), Align2::CENTER_CENTER, &msg, font(13.5, "semibold"), theme.text().gamma_multiply(a), w - 30.0);
                }
            }

            let ptr = pointer.unwrap_or(pos2(-1e4, -1e4));
            let mut frame = self.base_frame(full, ppp, time, nav_w);
            frame.pointer = [ptr.x * ppp, ptr.y * ppp, self.ptr_glow];
            self.set_frame(ui, slot, full, frame, &els);

            // Overlays draw last, over the finished UI.
            if self.popover.is_some() {
                self.draw_popover(ui, &mut icons, ppp, dt, full, time, nav_w, &mut animating);
            }
            if self.dialog.is_some() {
                self.draw_dialog(ui, &mut icons, ppp, dt, full, time, nav_w, &mut animating);
            }
        });

        // Player actions.
        if let Some((po, track)) = player_out {
            if po.clicked {
                self.sound.click();
            }
            self.run_actions(ctx, po.actions, track);
        }

        // Mini player while you're in another app (or Starlite is minimized)
        // and a song is loaded. Starlite's own windows (its YouTube player,
        // file dialogs) still count as "in"; the card itself doesn't.
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let away = minimized || !crate::win::app_in_front(self.mini_hwnd());
        let playing = matches!(self.playback_status(), Status::Playing | Status::Loading);
        let has_track = self.now.is_some();
        if self.mini.wants(away, playing, has_track) {
            animating |= self.mini_player(ctx, &mut icons);
        } else {
            self.mini_hidden();
        }

        // Discord status (only real changes reach Discord).
        {
            let (pos, dur) = self.playback_position();
            let playing = matches!(self.playback_status(), Status::Playing | Status::Loading);
            let now = self.now.clone();
            self.discord.update(&self.dprefs, now.as_ref(), playing, pos, dur);
        }

        if !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            resize_handles(ctx, full);
        }
        self.icons = icons;
        if animating {
            ctx.request_repaint();
        }
        let interval = if self.theme.animated() || self.fs_t > 0.0 {
            33
        } else if self.playback_status() == Status::Playing {
            250
        } else {
            0
        };
        self.ticker_ms.store(interval, Ordering::Relaxed);
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        self.audio.stop();
        if let Some(gl) = gl {
            self.renderer.lock().unwrap().destroy(gl);
        }
    }
}
