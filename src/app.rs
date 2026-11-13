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
use crate::stream::Server;
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
    /// A music server (Navidrome, Jellyfin): connect, then its playlists.
    Server(Server),
}

#[derive(Clone, PartialEq, Debug)]
enum NavKey {
    Explore,
    Server(Server),
    Playlist(String),
    /// A server playlist added to your playlists.
    NdLink(Server, String),
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
    SrvConnect(Server, Result<(), String>),
    SrvPlaylists(Server, Result<Vec<RemotePlaylist>, String>),
    /// The custom theme's picture, read (for that path).
    BgImage(Option<PathBuf>, Option<crate::theme::BgImage>),
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
    /// The server it's from (None: YouTube).
    pub server: Option<Server>,
    /// Opened from your playlists in the sidebar (no back button).
    pub linked: bool,
    pub tracks: Option<Result<Vec<Track>, String>>,
}

pub struct Dialog {
    pub name: String,
    pub cover: Cover,
    pub error: Option<String>,
    pub open_t: f32,
    /// Add this song once the playlist exists (from the player's "+").
    pub add_after: Option<Track>,
    /// Songs loaded from an .m3u file (and how many weren't found).
    pub import: Option<(Vec<Track>, usize)>,
}

/// The right-click menu on a Navidrome playlist.
pub struct NdMenu {
    pub server: Server,
    pub at: Pos2,
    pub pl: RemotePlaylist,
    pub open_t: f32,
}

pub struct Popover {
    pub anchor: Pos2,
    pub track: Track,
    pub open_t: f32,
}

/// The speed / bass dropdown above the player bar.
pub struct FxPop {
    pub anchor: Pos2,
    pub open_t: f32,
    /// The values as drawn (they glide to the real ones).
    pub shown: [f32; 2],
    /// Last snapped values, for the slider ticks.
    pub ticked: [f32; 2],
    pub tick_at: f64,
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
    pub jf: Arc<crate::jellyfin::Jellyfin>,
    pub jfst: NdState,
    /// How server songs are asked for (as they are, or converted).
    pub stream: crate::stream::StreamPrefs,
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
    pub lyric_scroll: crate::nowplaying::LyricScroll,
    pub lprefs: crate::lyrics::LyricsPrefs,
    /// CD shelf view of playlists (and its song previews).
    pub shelf: crate::shelf::Shelf,
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
    pub fx_pop: Option<FxPop>,
    pub nd_menu: Option<NdMenu>,
    /// Goes up every time a song starts playing (also the same one again).
    pub play_serial: u64,
    /// The song lined up to follow the current one: (audio serial it follows, its place in the queue, its id).
    pub lined_up: Option<(u64, usize, String)>,
    pub sound_ui: crate::soundpage::SoundUi,
    pub lastfm: crate::lastfm::LastFm,
    pub lfm_ui: crate::lastfm::LfmUi,
    last_advanced: u64,
    /// The custom theme's colours and picture.
    pub custom: crate::theme::CustomTheme,
    pub color_pick: Option<crate::customize::ColorPick>,
    /// Which picture the glass renderer has (None = not asked yet).
    bg_for: Option<Option<PathBuf>>,
    /// Liquid glass or the simple look.
    pub look: crate::theme::LookPrefs,
    /// The Custom theme's CSS (theme.css), read again when the file changes.
    pub css: crate::css::Css,
    css_stamp: Option<std::time::SystemTime>,
    css_checked: f64,
    /// The font family in use from the CSS, and whether it was found.
    pub css_font: Option<(String, bool)>,
    /// Speed and bass (sound.json).
    pub fx: crate::fx::FxPrefs,
    /// Playlist search: (which playlist, what's typed).
    pub pl_search: (String, String),
    pub toast: Option<(String, f64)>,
    pub time: f64,
}

/// Loads the three weights the app draws with: Segoe UI, or the font
/// family a theme's CSS names (when it's installed).
fn install_fonts(ctx: &egui::Context, family: Option<&str>) -> bool {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var("WINDIR")
        .map(|w| PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\Fonts"));
    let custom = family.and_then(|f| crate::css::pick_fonts(&crate::win::font_table(), f));
    let found = custom.is_some();
    let files = custom.unwrap_or_else(|| ["segoeui.ttf".into(), "seguisb.ttf".into(), "segoeuib.ttf".into()]);
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, file) in ["regular", "semibold", "bold"].into_iter().zip(files) {
        let mut fam = base.clone();
        // A bare file name is in Windows' Fonts folder; per-user fonts come with their path.
        if let Ok(bytes) = std::fs::read(dir.join(&file)) {
            fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            fam.insert(0, name.into());
        }
        let key = if name == "regular" { FontFamily::Proportional } else { FontFamily::Name(name.into()) };
        fonts.families.insert(key, fam);
    }
    ctx.set_fonts(fonts);
    found
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

/// Debug: STARLITE_SHOTS=<dir>,<count>,<ms>,<delay ms> saves `count` frames
/// of the window as dir/fNN.png, `ms` apart, starting after `delay`.
fn debug_shots(ctx: &egui::Context) {
    static STATE: Mutex<(usize, f64)> = Mutex::new((0, -1.0));
    let Ok(spec) = std::env::var("STARLITE_SHOTS") else { return };
    let parts: Vec<&str> = spec.split(',').collect();
    let (Some(dir), Some(n), Some(ms), Some(delay)) = (parts.first(), parts.get(1).and_then(|v| v.parse::<usize>().ok()), parts.get(2).and_then(|v| v.parse::<f64>().ok()), parts.get(3).and_then(|v| v.parse::<f64>().ok())) else { return };
    let now = ctx.input(|i| i.time);
    let mut st = STATE.lock().unwrap();
    for e in ctx.input(|i| i.raw.events.clone()) {
        if let egui::Event::Screenshot { image, .. } = e {
            let [w, h] = image.size;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = std::fs::create_dir_all(dir);
                let _ = img.save(std::path::Path::new(dir).join(format!("f{:02}.png", st.0)));
            }
            st.0 += 1;
            if st.0 >= n {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }
    let due = delay / 1000.0 + st.0 as f64 * ms / 1000.0;
    if st.0 < n && now >= due && st.1 < due {
        st.1 = due;
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
    }
    ctx.request_repaint_after(Duration::from_millis(15));
}

fn rgb_f(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
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
        install_fonts(&cc.egui_ctx, None);
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
            lyric_scroll: Default::default(),
            lprefs: crate::lyrics::LyricsPrefs::load(),
            shelf: crate::shelf::Shelf::new(),
            palette: DEFAULT_PALETTE,
            palette_target: DEFAULT_PALETTE,
            palette_for: None,
            yt: Arc::new(YouTube::new()),
            nd: Arc::new(crate::navidrome::Navidrome::load()),
            ndst: NdState::default(),
            jf: Arc::new(crate::jellyfin::Jellyfin::load()),
            jfst: NdState::default(),
            stream: crate::stream::StreamPrefs::load(),
            // Debug: STARLITE_SETTINGS_TAB=0..8 opens that Settings tab.
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
                Ok("navidrome") => View::Server(Server::Navidrome),
                Ok("jellyfin") => View::Server(Server::Jellyfin),
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
            fx_pop: None,
            nd_menu: None,
            play_serial: 0,
            lined_up: None,
            sound_ui: Default::default(),
            lastfm: crate::lastfm::LastFm::start(),
            lfm_ui: Default::default(),
            last_advanced: 0,
            custom: crate::theme::CustomTheme::get(),
            color_pick: None,
            bg_for: None,
            look: crate::theme::LookPrefs::load(),
            css: Default::default(),
            css_stamp: None,
            css_checked: -10.0,
            css_font: None,
            fx: crate::fx::FxPrefs::load(),
            pl_search: (String::new(), String::new()),
            toast: None,
            time: 0.0,
            mini: crate::mini::Mini::new(),
        };
        app.yt_key_input = app.yt.api_key();
        if let Some(h) = hwnd {
            match YtPlayer::new(h, cc.egui_ctx.clone()) {
                Ok(mut p) => {
                    p.set_speed(app.fx.speed);
                    app.ytp = Some(p);
                }
                Err(e) => app.toast(e),
            }
        }
        // Debug: STARLITE_DIALOG=1 opens the new-playlist dialog.
        if std::env::var("STARLITE_DIALOG").is_ok() {
            app.open_dialog(None);
            // Debug: STARLITE_M3U=<file> loads that .m3u into it.
            if let Ok(f) = std::env::var("STARLITE_M3U") {
                let known = app.lib.downloaded.clone();
                if let (Some(d), Ok(r)) = (app.dialog.as_mut(), crate::m3u::import(std::path::Path::new(&f), &known)) {
                    d.name = r.name;
                    d.import = Some((r.tracks, r.missing));
                }
            }
        }
        // Debug: STARLITE_FULLSCREEN=1 opens fullscreen Now Playing (with autoplay).
        app.fullscreen = std::env::var("STARLITE_FULLSCREEN").is_ok();
        // Debug: STARLITE_AUTOPLAY=1 plays the first downloaded song.
        if std::env::var("STARLITE_AUTOPLAY").is_ok() && !app.lib.downloaded.is_empty() {
            let list = app.lib.downloaded.clone();
            app.play_list(list, 0);
        }
        app.audio.set_speed(app.fx.speed);
        app.audio.set_fx(&app.fx);
        app.apply_theme(&cc.egui_ctx);
        if app.yt.configured() {
            app.load_explore(&cc.egui_ctx);
        }
        // Debug: STARLITE_ND_CONNECT / STARLITE_JF_CONNECT=url|user|password connect to a
        // (test) Navidrome / Jellyfin server at start.
        for (var, srv) in [("STARLITE_ND_CONNECT", Server::Navidrome), ("STARLITE_JF_CONNECT", Server::Jellyfin)] {
            if let Ok(v) = std::env::var(var) {
                let mut it = v.splitn(3, '|');
                let st = app.st(srv);
                st.url = it.next().unwrap_or_default().to_string();
                st.user = it.next().unwrap_or_default().to_string();
                st.pass = it.next().unwrap_or_default().to_string();
                app.connect_server(&cc.egui_ctx, srv);
            }
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
        self.remote = Some(RemoteView { pl, server: None, linked: false, tracks: None });
        self.view = View::YtPlaylist;
        let (yt, tx, ctx) = (self.yt.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = yt.playlist_tracks(&id);
            let _ = tx.send(Job::YtPlaylist(id, r));
            ctx.request_repaint();
        });
    }

    /// Where the Custom theme's CSS lives.
    pub fn css_file() -> PathBuf {
        crate::library::data_dir().join("theme.css")
    }

    /// The Custom theme's colours with its CSS on top (what's drawn).
    pub fn effective_custom(&self) -> crate::theme::CustomTheme {
        let mut c = self.custom.clone();
        c.background = self.css.background.unwrap_or(c.background);
        c.accent = self.css.accent.unwrap_or(c.accent);
        c.text = self.css.text.unwrap_or(c.text);
        c.text_dim = self.css.text_dim;
        c
    }

    /// The Custom theme's picture: the CSS's if it names one, else the picked one.
    pub fn custom_picture(&self) -> Option<PathBuf> {
        match &self.css.image {
            Some(p) => p.clone(),
            None => self.custom.image.clone(),
        }
    }

    /// Reads theme.css again when it was saved (looked at once a second).
    fn watch_css(&mut self, ctx: &egui::Context) {
        if self.time - self.css_checked < 1.0 {
            return;
        }
        self.css_checked = self.time;
        let stamp = std::fs::metadata(Self::css_file()).and_then(|m| m.modified()).ok();
        if stamp == self.css_stamp {
            return;
        }
        self.css_stamp = stamp;
        self.css = std::fs::read_to_string(Self::css_file()).map(|t| crate::css::parse(&t)).unwrap_or_default();
        // A font it names (back to Segoe UI when it names none).
        let family = self.css.font.clone();
        if family != self.css_font.as_ref().map(|f| f.0.clone()) {
            let found = install_fonts(ctx, family.as_deref());
            self.css_font = family.map(|f| (f, found));
        }
        self.custom_changed(ctx, false);
        ctx.request_repaint();
    }

    /// The page state of a server.
    pub fn st(&mut self, s: Server) -> &mut NdState {
        match s {
            Server::Navidrome => &mut self.ndst,
            Server::Jellyfin => &mut self.jfst,
        }
    }

    /// (server URL, user name) when connected to it.
    pub fn server_connected(&self, s: Server) -> Option<(String, String)> {
        match s {
            Server::Navidrome => self.nd.connected(),
            Server::Jellyfin => self.jf.connected(),
        }
    }

    /// Checks a server login on a worker thread.
    pub fn connect_server(&mut self, ctx: &egui::Context, s: Server) {
        if self.st(s).busy {
            return;
        }
        let st = self.st(s);
        st.busy = true;
        st.error = None;
        let (url, user, pass) = (st.url.clone(), st.user.clone(), st.pass.clone());
        let (nd, jf, tx, ctx) = (self.nd.clone(), self.jf.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = match s {
                Server::Navidrome => nd.connect(&url, &user, &pass),
                Server::Jellyfin => jf.connect(&url, &user, &pass),
            };
            let _ = tx.send(Job::SrvConnect(s, r));
            ctx.request_repaint();
        });
    }

    pub fn disconnect_server(&mut self, s: Server) {
        match s {
            Server::Navidrome => self.nd.disconnect(),
            Server::Jellyfin => {
                // Signing out tells the server; don't hold the window up for it.
                let jf = self.jf.clone();
                std::thread::spawn(move || jf.disconnect());
            }
        }
        *self.st(s) = Default::default();
    }

    pub fn load_server(&mut self, ctx: &egui::Context, s: Server) {
        if self.st(s).loading {
            return;
        }
        self.st(s).loading = true;
        let (nd, jf, tx, ctx) = (self.nd.clone(), self.jf.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = match s {
                Server::Navidrome => nd.playlists(),
                Server::Jellyfin => jf.playlists(),
            };
            let _ = tx.send(Job::SrvPlaylists(s, r));
            ctx.request_repaint();
        });
    }

    pub fn open_server_playlist(&mut self, ctx: &egui::Context, s: Server, pl: RemotePlaylist, linked: bool) {
        let id = pl.id.clone();
        self.remote = Some(RemoteView { pl, server: Some(s), linked, tracks: None });
        self.view = View::YtPlaylist;
        let (nd, jf, tx, ctx) = (self.nd.clone(), self.jf.clone(), self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = match s {
                Server::Navidrome => nd.playlist_tracks(&id),
                Server::Jellyfin => jf.playlist_tracks(&id),
            };
            let _ = tx.send(Job::YtPlaylist(id, r));
            ctx.request_repaint();
        });
    }

    /// The link a server song streams from (None: not a server song, or not connected).
    pub fn stream_for(&self, t: &Track) -> Option<String> {
        match &t.source {
            Source::Navidrome { song_id, suffix } => self.nd.stream_url(song_id, suffix, &self.stream),
            Source::Jellyfin { item_id } => self.jf.stream_url(item_id, &self.stream),
            _ => None,
        }
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
                            if v.server.is_some() {
                                let list = list.clone();
                                v.tracks = Some(r);
                                self.play_list(list, n.parse().unwrap_or(0));
                                continue;
                            }
                        }
                        v.tracks = Some(r);
                    }
                }
                Job::BgImage(path, img) => {
                    if self.bg_for.as_ref() == Some(&path) {
                        if path.is_some() && img.is_none() {
                            self.toast("Couldn't read the background picture");
                        }
                        self.renderer.lock().unwrap().pending_bg = Some(img);
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
                        self.lyric_scroll = Default::default();
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
                Job::SrvConnect(s, r) => {
                    self.st(s).busy = false;
                    match r {
                        Ok(()) => {
                            let st = self.st(s);
                            st.pass.clear();
                            st.error = None;
                            st.loaded = false;
                            self.load_server(ctx, s);
                        }
                        Err(e) => self.st(s).error = Some(e),
                    }
                }
                Job::SrvPlaylists(s, r) => {
                    self.st(s).loading = false;
                    self.st(s).loaded = true;
                    match r {
                        Ok(p) => {
                            self.lib.refresh_nd_links(s, &p);
                            // Debug: STARLITE_ND_MENU=x,y opens the first playlist's right-click menu there.
                            if let Some((x, y)) = std::env::var("STARLITE_ND_MENU").ok().and_then(|v| {
                                let (x, y) = v.split_once(',')?;
                                Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
                            }) {
                                if let Some(first) = p.first().cloned() {
                                    self.nd_menu = Some(NdMenu { server: s, at: pos2(x, y), pl: first, open_t: 0.0 });
                                }
                            }
                            // Debug: STARLITE_ND_PLAY=<n> opens the first playlist and plays song n
                            // (STARLITE_ND_OPEN=1 only opens it).
                            if std::env::var("STARLITE_ND_PLAY").is_ok() || std::env::var("STARLITE_ND_OPEN").is_ok() {
                                if let Some(first) = p.first().cloned() {
                                    self.open_server_playlist(ctx, s, first, false);
                                }
                            }
                            let st = self.st(s);
                            st.playlists = p;
                            st.error = None;
                        }
                        Err(e) => self.st(s).error = Some(e),
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
        self.play_serial += 1;
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
            let stream = self.stream_for(&t);
            if stream.is_none() {
                match t.source {
                    Source::Navidrome { .. } => self.toast("Connect to Navidrome to play this song"),
                    Source::Jellyfin { .. } => self.toast("Connect to Jellyfin to play this song"),
                    _ => {}
                }
            }
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

    /// New song: fetch its cover colours and its lyrics.
    fn on_track_changed(&mut self, ctx: &egui::Context) {
        let Some(t) = self.now.clone() else { return };
        self.palette_for = Some(t.id.clone());
        let (tx, c) = (self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let pal = t.artwork.as_deref().and_then(crate::nowplaying::cover_bytes).and_then(|b| crate::nowplaying::palette_from(&b));
            let _ = tx.send(Job::Palette(t.id.clone(), pal));
            c.request_repaint();
        });
        self.load_lyrics(ctx);
    }

    /// Lyrics for the current song: a `.lrc` file or timed tags win; then
    /// lrclib.net (when on), whose timed lyrics beat untimed tags.
    pub fn load_lyrics(&mut self, ctx: &egui::Context) {
        let Some(t) = self.now.clone() else { return };
        self.lyrics = None;
        self.lyric_scroll = Default::default();
        let online = self.lprefs.online;
        let (tx, c) = (self.jobs_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |l: Option<Lyrics>| {
                let _ = tx.send(Job::Lyrics(t.id.clone(), l));
                c.request_repaint();
            };
            let local = match &t.source {
                Source::File { path } => crate::meta::lyrics(path),
                _ => None,
            };
            if local.as_ref().is_some_and(|l| l.synced) || !online {
                return send(local);
            }
            if local.is_some() {
                send(local.clone());
            }
            match crate::lyrics::online(&t) {
                Some(l) if l.synced || local.is_none() => send(Some(l)),
                _ if local.is_none() => send(None),
                _ => {}
            }
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

    /// A dropdown or dialog is open: the page under it must not react.
    pub fn overlay_open(&self) -> bool {
        self.fx_pop.is_some() || self.popover.is_some() || self.dialog.is_some() || self.nd_menu.is_some() || self.color_pick.is_some()
    }

    pub fn now_is_youtube(&self) -> bool {
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

    /// Which song of the queue comes after the current one (None: the end).
    /// `auto`: the song ended by itself (loop-one repeats it).
    fn following(&self, forward: bool, auto: bool) -> Option<usize> {
        let n = self.queue.len();
        if n == 0 {
            return None;
        }
        if auto && self.player.looping == Loop::One {
            Some(self.qi.min(n - 1))
        } else if forward {
            if self.qi + 1 < n {
                Some(self.qi + 1)
            } else if !auto || self.player.looping == Loop::All {
                Some(0)
            } else {
                None
            }
        } else {
            Some(self.qi.saturating_sub(1))
        }
    }

    fn step(&mut self, forward: bool, auto: bool) {
        if let Some(i) = self.following(forward, auto) {
            self.qi = i;
            self.play_track(self.queue[i].clone());
        }
    }

    /// Keeps the song that follows lined up in the audio engine, so it
    /// starts without a gap (or with the crossfade). YouTube songs can't be.
    fn line_up_next(&mut self) {
        let playing = !self.now_is_youtube() && matches!(self.audio.status(), Status::Playing | Status::Paused);
        if !playing {
            self.lined_up = None;
            return;
        }
        let serial = self.audio.serial();
        let want = self.following(true, true).filter(|&i| self.queue[i].youtube_id().is_none()).map(|i| (serial, i, self.queue[i].id.clone()));
        if want == self.lined_up {
            return;
        }
        match &want {
            Some((_, i, _)) => {
                let t = self.queue[*i].clone();
                match (self.stream_for(&t), &t.source) {
                    // A server song without its server can't be loaded.
                    (None, Source::Navidrome { .. } | Source::Jellyfin { .. }) => self.audio.clear_next(),
                    (stream, _) => self.audio.preload(t, stream),
                }
            }
            None => self.audio.clear_next(),
        }
        self.lined_up = want;
    }

    pub fn current_track(&self) -> Option<Track> {
        self.now.clone()
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.time));
    }

    pub fn open_dialog(&mut self, add_after: Option<Track>) {
        self.popover = None;
        self.dialog = Some(Dialog { name: String::new(), cover: Cover::Preset("disc-3".into()), error: None, open_t: 0.0, add_after, import: None });
    }

    // ── Navigation ─────────────────────────────────────────────────

    fn nav_layout(&self, w: f32, h: f32, t: f32) -> NavLayout {
        let full = |y: f32| Rect::from_min_size(pos2(12.0, y), vec2(w - 24.0, 38.0));
        let mini = |y: f32| Rect::from_min_size(pos2(w * 0.5 - 23.0, y), Vec2::splat(46.0));
        let row = |ye: f32, ym: f32| lerp_rect(mini(ym), full(ye), t);

        let logo = lerp_rect(Rect::from_center_size(pos2(w * 0.5, 46.0), Vec2::splat(56.0)), Rect::from_min_size(pos2(18.0, 14.0), Vec2::splat(74.0)), t);
        let mut rows = vec![(NavKey::Explore, row(98.0, 92.0))];
        let nd_y = lerp(144.0, 140.0, t);
        rows.push((NavKey::Server(Server::Navidrome), row(nd_y, nd_y)));
        let jf_y = lerp(196.0, 182.0, t);
        rows.push((NavKey::Server(Server::Jellyfin), row(jf_y, jf_y)));
        let shift = lerp(104.0, 88.0, t);
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
        let keys = self.lib.user_playlists().map(|p| NavKey::Playlist(p.id.clone())).chain(self.lib.nd_links.iter().map(|l| NavKey::NdLink(l.server, l.id.clone())));
        for (i, k) in keys.enumerate() {
            let y = list_top + i as f32 * pitch - self.nav_scroll;
            rows.push((k, row(y, y)));
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
            View::Server(s) => Some(NavKey::Server(*s)),
            View::YtPlaylist => match self.remote.as_ref() {
                Some(RemoteView { server: Some(s), linked: true, pl, .. }) => Some(NavKey::NdLink(*s, pl.id.clone())),
                Some(RemoteView { server: Some(s), .. }) => Some(NavKey::Server(*s)),
                _ => None,
            },
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
        let n = (self.lib.user_playlists().count() + self.lib.nd_links.len()) as f32;
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
        let mut right_clicked: Option<(Server, RemotePlaylist, Pos2)> = None;
        for (key, rect) in &lay.rows {
            let in_list = matches!(key, NavKey::Playlist(_) | NavKey::NdLink(..));
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
            if let NavKey::NdLink(srv, lid) = key {
                if resp.secondary_clicked() {
                    if let (Some(l), Some(at)) = (self.lib.nd_link(*srv, lid), resp.interact_pointer_pos()) {
                        right_clicked = Some((*srv, l.to_remote(), at));
                    }
                }
            }

            let name: String = match key {
                NavKey::Explore => "Explore".into(),
                NavKey::Server(s) => s.name().into(),
                NavKey::Settings => "Settings".into(),
                NavKey::Liked => "Liked Music".into(),
                NavKey::Downloaded => "Downloaded Music".into(),
                NavKey::Playlist(pid) => self.lib.playlists.iter().find(|p| &p.id == pid).map(|p| p.name.clone()).unwrap_or_default(),
                NavKey::NdLink(srv, lid) => self.lib.nd_link(*srv, lid).map(|l| l.title.clone()).unwrap_or_default(),
            };
            if label_a < 0.5 {
                resp.on_hover_text(name.clone());
            }
            let tint = if is_sel { theme.accent() } else { theme.text_dim() };
            let tc = if is_sel { theme.text() } else { theme.text_dim() };
            let lead_c = pos2(lerp(rect.center().x, rect.left() + 22.0, t), rect.center().y);
            let icon: Option<&'static str> = match key {
                NavKey::Explore => Some("compass"),
                NavKey::Server(s) => Some(s.icon()),
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
                NavKey::NdLink(srv, lid) => {
                    if let Some(l) = self.lib.nd_link(*srv, lid) {
                        let c = Rect::from_center_size(lead_c, Vec2::splat(lerp(34.0, 26.0, t)));
                        let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(lay.list_clip));
                        cu.set_clip_rect(lay.list_clip);
                        let art = l.artwork.clone();
                        crate::widgets::cover(&cu, icons, c, art.as_deref(), 7.0, srv.icon(), theme);
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
            if matches!(selected, Some(NavKey::Playlist(_) | NavKey::NdLink(..))) {
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

        if let Some((server, pl, at)) = right_clicked {
            self.sound.click();
            self.nd_menu = Some(NdMenu { server, at, pl, open_t: 0.0 });
        }
        if let Some(NavKey::NdLink(srv, lid)) = &clicked {
            self.sound.click();
            if let Some(pl) = self.lib.nd_link(*srv, lid).map(|l| l.to_remote()) {
                self.open_server_playlist(&ctx, *srv, pl, true);
            }
            return;
        }
        if let Some(k) = clicked {
            self.sound.click();
            self.view = match k {
                NavKey::Explore => View::Explore,
                NavKey::Server(s) => {
                    if self.server_connected(s).is_some() && !self.st(s).loaded {
                        self.load_server(&ctx, s);
                    }
                    View::Server(s)
                }
                NavKey::Settings => View::Settings,
                NavKey::Liked => View::Playlist(LIKED_ID.into()),
                NavKey::Downloaded => {
                    self.lib.scan_downloads();
                    View::Playlist(DOWNLOADED_ID.into())
                }
                NavKey::Playlist(id) => View::Playlist(id),
                NavKey::NdLink(s, _) => View::Server(s),
            };
        }
    }

    // ── Overlays ───────────────────────────────────────────────────

    pub fn base_frame(&self, full: Rect, ppp: f32, time: f32, nav_w: f32) -> Frame {
        // The Custom theme as drawn: its colours with the CSS on top.
        let custom = crate::theme::CustomTheme::get();
        let css = (self.theme == Theme::Custom).then_some(&self.css);
        let flat = !self.look.liquid_glass;
        let unset = [0.0, 0.0, 0.0, -1.0];
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
            custom_bg: rgb_f(custom.background),
            accent: rgb_f(custom.accent),
            light_ui: custom.is_light(),
            flat,
            fills: [css.and_then(|c| c.panel).unwrap_or(unset), css.and_then(|c| c.sidebar).unwrap_or(unset), css.and_then(|c| c.player).unwrap_or(unset)],
            edge: css.and_then(|c| c.border).unwrap_or(unset),
            // The simple look has calmer corners; liquid glass keeps its own.
            round: css.and_then(|c| c.radius).unwrap_or(if flat { 14.0 } else { 1000.0 }),
            frost: css.and_then(|c| c.blur).unwrap_or(16.0),
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
        let import = d.import.as_ref().map(|(t, m)| (t.len(), *m));
        let fresp = field(ui, &mut els, ppp, name_rect, &mut name, "My playlist", false, true, theme);
        if open_t < 0.15 {
            fresp.request_focus();
        }
        let submit = fresp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(e) = &error {
            p.text(pos2(name_rect.left(), name_rect.bottom() + 14.0), Align2::LEFT_CENTER, e, font(12.0, "regular"), Color32::from_rgb(235, 90, 100).gamma_multiply(ot));
        } else if let Some((n, missing)) = import {
            let mut s = format!("{n} song{} from the .m3u", if n == 1 { "" } else { "s" });
            if missing > 0 {
                s += &format!(" · {missing} not found");
            }
            text_trunc(&p, pos2(name_rect.left(), name_rect.bottom() + 14.0), Align2::LEFT_CENTER, &s, font(12.0, "semibold"), dim, name_rect.width());
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
        let m3u_r = Rect::from_min_size(pos2(x0, by), vec2(142.0, 40.0));
        let m3u = pill(ui, icons, &mut els, ppp, ui.id().with("dlg_m3u"), m3u_r, "Load .m3u", Some("file-music"), false, true, theme)
            .on_hover_text("Fill the playlist from an .m3u or .m3u8 file");
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
        if m3u.clicked() {
            self.sound.click();
            if let Some(path) = rfd::FileDialog::new().add_filter("Playlist", &["m3u", "m3u8"]).pick_file() {
                let mut known = self.lib.downloaded.clone();
                known.extend(self.lib.playlists.iter().flat_map(|p| p.tracks.iter().cloned()));
                match crate::m3u::import(&path, &known) {
                    Ok(r) if r.tracks.is_empty() => d.error = Some(format!("None of its {} songs were found on this PC", r.missing)),
                    Ok(r) => {
                        if d.name.trim().is_empty() {
                            d.name = r.name;
                        }
                        d.error = None;
                        d.import = Some((r.tracks, r.missing));
                    }
                    Err(e) => d.error = Some(e),
                }
            }
            return;
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
            if let Some((tracks, _)) = &d.import {
                let n = self.lib.add_many(&id, tracks);
                self.toast(format!("Loaded {n} song{}", if n == 1 { "" } else { "s" }));
            }
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
    /// Right-click menu on a server playlist: add it to your playlists
    /// (or take it away again).
    #[allow(clippy::too_many_arguments)]
    fn draw_nd_menu(&mut self, ui: &mut egui::Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let Some(m) = self.nd_menu.as_mut() else { return };
        m.open_t = (m.open_t + dt * 9.0).min(1.0);
        if m.open_t < 1.0 {
            *animating = true;
        }
        let ot = smooth(m.open_t);
        let (server, at, pl) = (m.server, m.at, m.pl.clone());
        let linked = self.lib.nd_link(server, &pl.id).is_some();
        let back = ui.interact(full, ui.id().with("ndm_back"), Sense::click());
        let label = if linked { "Remove from your playlists" } else { "Add to your playlists" };
        let w = ui.painter().layout_no_wrap(label.to_string(), font(13.5, "semibold"), theme.text()).size().x + 70.0;
        let h = 52.0;
        let x = at.x.min(full.right() - w - 10.0);
        let y = if at.y + h + 10.0 > full.bottom() { at.y - h } else { at.y };
        let panel = Rect::from_min_size(pos2(x, y - 6.0 * (1.0 - ot)), vec2(w, h));
        let mut els = vec![El::rect(panel, ppp, 18.0, 1.0, 10.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let r = panel.shrink(6.0);
        let id = ui.id().with("ndm_row");
        let resp = ui.interact(r, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let hv = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
        if hv > 0.001 {
            els.push(El::rect(r, ppp, 13.0, hv, 2.0, Kind::Layered, 0.3, 0.0));
        }
        let fg = theme.text().gamma_multiply(ot);
        icons.paint(ui, if linked { "list-x" } else { "list-plus" }, pos2(r.left() + 20.0, r.center().y), 17.0, fg);
        ui.painter().text(pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, label, font(13.5, "semibold"), fg);
        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        self.set_frame(ui, slot, full, frame, &els);

        if resp.clicked() {
            self.sound.click();
            let added = self.lib.toggle_nd_link(server, &pl);
            self.toast(if added { format!("Added {} to your playlists", pl.title) } else { format!("Removed {} from your playlists", pl.title) });
            // Taken away while open from the sidebar: it's now a plain Navidrome playlist.
            if !added {
                if let Some(rv) = self.remote.as_mut().filter(|rv| rv.server == Some(server) && rv.pl.id == pl.id) {
                    rv.linked = false;
                }
            }
            self.nd_menu = None;
        } else if ((back.clicked() || back.secondary_clicked()) && !back.interact_pointer_pos().is_some_and(|p| panel.contains(p))) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.nd_menu = None;
        }
    }

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
                    self.fx_pop = None;
                }
                Action::Sound(anchor) => {
                    self.popover = None;
                    self.fx_pop = Some(FxPop { anchor, open_t: 0.0, shown: [self.fx.speed, self.fx.bass], ticked: [self.fx.speed, self.fx.bass], tick_at: 0.0 });
                }
                Action::Volume(v) => {
                    // Still turned down while a shelf preview plays.
                    let v = v * self.shelf.duck;
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
        debug_shots(ctx);
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

        // The engine moved on to the lined-up song by itself (gapless / crossfade).
        let (advanced, on) = self.audio.advanced();
        if advanced != self.last_advanced {
            self.last_advanced = advanced;
            if let Some(t) = on {
                if let Some(i) = self.lined_up.as_ref().map(|l| l.1).filter(|&i| self.queue.get(i).is_some_and(|q| q.id == t.id)).or_else(|| self.queue.iter().position(|q| q.id == t.id)) {
                    self.qi = i;
                }
                self.now = Some(t);
                self.play_serial += 1;
                if self.palette_for.as_ref() == self.now.as_ref().map(|t| &t.id) {
                    // The same song again (loop one): its lyrics start over.
                    self.lyric_scroll = Default::default();
                }
            }
        }
        self.line_up_next();
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
        if self.fullscreen && !self.overlay_open() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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

        // The custom theme's picture, read off the UI thread.
        if theme == Theme::Custom {
            self.watch_css(ctx);
        }
        let picture = self.custom_picture();
        if theme == Theme::Custom && self.bg_for.as_ref() != Some(&picture) {
            let path = picture;
            self.bg_for = Some(path.clone());
            let (tx, c) = (self.jobs_tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let img = path.as_deref().and_then(crate::theme::load_bg_image);
                let _ = tx.send(Job::BgImage(path, img));
                c.request_repaint();
            });
        }
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
                els.push(El::rect(Rect::from_min_max(pos2(-80.0, -80.0), pos2(nav_w, full.bottom() + 80.0)), ppp, 26.0, 1.0, 6.0, Kind::Glass, 0.0, 0.0).with_value(1.0).with_class(crate::glass::Class::Sidebar));
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
            self.shelf_after_frame(dt);
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
                fx_on: !self.fx.is_default(),
            };
            let po = self.player.ui(ui, &mut icons, bar, ppp, dt, theme, &mut els, now);
            // Debug: STARLITE_FX_OPEN=1 opens the speed / bass dropdown once.
            static FX_OPENED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if std::env::var("STARLITE_FX_OPEN").is_ok() && !FX_OPENED.swap(true, Ordering::Relaxed) {
                let anchor = pos2(bar.right() - 392.0, bar.center().y);
                self.fx_pop = Some(FxPop { anchor, open_t: 0.0, shown: [self.fx.speed, self.fx.bass], ticked: [self.fx.speed, self.fx.bass], tick_at: 0.0 });
            }
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
            if self.fx_pop.is_some() {
                self.draw_fx_popover(ui, ppp, dt, full, time, nav_w, &mut animating);
            }
            if self.popover.is_some() {
                self.draw_popover(ui, &mut icons, ppp, dt, full, time, nav_w, &mut animating);
            }
            if self.dialog.is_some() {
                self.draw_dialog(ui, &mut icons, ppp, dt, full, time, nav_w, &mut animating);
            }
            if self.nd_menu.is_some() {
                self.draw_nd_menu(ui, &mut icons, ppp, dt, full, time, nav_w, &mut animating);
            }
            if self.color_pick.is_some() {
                self.draw_color_pick(ui, ppp, dt, full, time, nav_w, &mut animating);
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
            // Last.fm: now playing, and a scrobble once enough of the song was heard.
            let heard = self.playback_status() == Status::Playing;
            self.lastfm.update(now.as_ref(), self.play_serial, heard);
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
        } else if self.theme == Theme::Custom && self.view == View::Settings {
            // Editing the theme's CSS: notice when the file is saved.
            1000
        } else {
            0
        };
        self.ticker_ms.store(interval, Ordering::Relaxed);
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        self.audio.stop();
        if let Some(gl) = gl {
            self.renderer.lock().unwrap().destroy(gl);
            if let Some(b) = self.shelf.blur_gl.lock().unwrap().as_ref() {
                b.destroy(gl);
            }
        }
    }
}
