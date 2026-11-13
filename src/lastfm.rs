//! Last.fm scrobbling. You bring your own API key and shared secret (from
//! last.fm/api/account/create), then allow Starlite in your browser: your
//! Last.fm password never comes near the app. What's saved
//! (%APPDATA%\Starlite\lastfm.json): the key, the secret and the session
//! key Last.fm hands out.
//!
//! A song is sent as "now playing" when it starts and scrobbled once you've
//! heard half of it or four minutes, whichever comes first (songs under 30
//! seconds don't count): Last.fm's own rules. Scrobbles that couldn't be
//! sent wait in lastfm-queue.json and go out with the next one.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{pos2, vec2, Align2, Color32, Id, Rect, Ui};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::Starlite;
use crate::glass::El;
use crate::icons::Icons;
use crate::library::Track;
use crate::pages::toggle_row;
use crate::widgets::{field, font, pill, text_trunc};

const API: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct LfmPrefs {
    pub enabled: bool,
    pub api_key: String,
    pub secret: String,
    /// The session key Last.fm gave after you allowed Starlite.
    pub session: String,
    pub user: String,
}

impl LfmPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("lastfm.json")
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
    pub fn connected(&self) -> bool {
        !self.session.is_empty()
    }
}

/// A play to tell Last.fm about.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Play {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    /// Seconds (0: unknown).
    pub secs: u64,
    /// When it started (Unix time).
    pub at: u64,
}

impl Play {
    fn of(t: &Track, at: u64) -> Play {
        Play { artist: t.artist.clone(), title: t.title.clone(), album: t.album.clone().filter(|a| !a.is_empty()), secs: t.duration_ms / 1000, at }
    }
}

enum Msg {
    /// Start connecting with this key and secret.
    Connect(String, String),
    Cancel,
    NowPlaying(LfmPrefs, Play),
    Scrobble(LfmPrefs, Play),
}

/// What the worker reports back.
#[derive(Default)]
pub struct Report {
    /// A line for the settings tab (and whether it's good news).
    pub status: Option<(bool, String)>,
    /// Waiting for you to allow Starlite in the browser.
    pub connecting: bool,
    /// A page to open in the browser.
    pub open: Option<String>,
    /// A finished sign-in: (session key, user name).
    pub session: Option<(String, String)>,
}

pub struct LastFm {
    tx: Sender<Msg>,
    pub prefs: LfmPrefs,
    pub report: Arc<Mutex<Report>>,
    /// The play being listened to: (song id, play serial), what to send, seconds heard, scrobbled.
    current: Option<((String, u64), Play, f64, bool)>,
    last_frame: Instant,
}

/// The fields of the settings tab.
#[derive(Default)]
pub struct LfmUi {
    pub key: String,
    pub secret: String,
    loaded: bool,
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How long (s) a song must be heard to count: half of it or four minutes.
/// None: too short to count at all.
pub fn scrobble_after(secs: u64) -> Option<f64> {
    // Debug: STARLITE_LASTFM_FAST=1 counts any song after five seconds.
    if std::env::var("STARLITE_LASTFM_FAST").is_ok() {
        return Some(5.0);
    }
    match secs {
        // Unknown length: four minutes it is.
        0 => Some(240.0),
        1..=30 => None,
        s => Some((s as f64 / 2.0).min(240.0)),
    }
}

/// Last.fm's request signature: every parameter (sorted by name) as
/// name + value, then the shared secret, MD5'd.
fn sign(params: &[(&str, String)], secret: &str) -> String {
    let mut p: Vec<&(&str, String)> = params.iter().collect();
    p.sort_by(|a, b| a.0.cmp(b.0));
    let mut s: String = p.iter().map(|(k, v)| format!("{k}{v}")).collect();
    s.push_str(secret);
    crate::navidrome::md5_hex(&s)
}

/// Calls a Last.fm method. Err: (Last.fm's error code or 0, message).
fn call(method: &str, mut params: Vec<(&str, String)>, secret: &str) -> Result<Value, (i64, String)> {
    // Debug: STARLITE_LASTFM_API=<url> talks to a test server instead.
    let api = std::env::var("STARLITE_LASTFM_API").unwrap_or_else(|_| API.to_string());
    params.push(("method", method.to_string()));
    let sig = sign(&params, secret);
    params.push(("api_sig", sig));
    params.push(("format", "json".to_string()));
    let form: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let ua = concat!("Starlite v", env!("CARGO_PKG_VERSION"));
    let v: Value = match ureq::post(&api).timeout(Duration::from_secs(12)).set("User-Agent", ua).send_form(&form) {
        Ok(r) => r.into_json().map_err(|_| (0, "Last.fm sent something unexpected".to_string()))?,
        Err(ureq::Error::Status(code, r)) => r.into_json().map_err(|_| (0, format!("Last.fm answered with error {code}")))?,
        Err(ureq::Error::Transport(t)) => return Err((0, format!("Can't reach Last.fm ({})", t.kind()))),
    };
    match v["error"].as_i64() {
        Some(code) => Err((code, v["message"].as_str().unwrap_or("Last.fm refused the request").to_string())),
        None => Ok(v),
    }
}

fn play_params(p: &Play, prefs: &LfmPrefs) -> Vec<(&'static str, String)> {
    let mut v = vec![("api_key", prefs.api_key.clone()), ("sk", prefs.session.clone()), ("artist", p.artist.clone()), ("track", p.title.clone())];
    if let Some(a) = &p.album {
        v.push(("album", a.clone()));
    }
    if p.secs > 0 {
        v.push(("duration", p.secs.to_string()));
    }
    v
}

fn queue_file() -> std::path::PathBuf {
    crate::library::data_dir().join("lastfm-queue.json")
}

fn worker(rx: Receiver<Msg>, report: Arc<Mutex<Report>>) {
    let say = |ok: bool, s: String| report.lock().unwrap().status = Some((ok, s));
    // Scrobbles that still have to go out (oldest first).
    let mut queue: Vec<Play> = std::fs::read_to_string(queue_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Cancel => {}
            Msg::Connect(key, secret) => {
                report.lock().unwrap().connecting = true;
                let done = (|| -> Result<(String, String), String> {
                    let v = call("auth.getToken", vec![("api_key", key.clone())], &secret).map_err(|(code, m)| match code {
                        10 => "That API key isn't valid".to_string(),
                        26 => "That API key has been suspended".to_string(),
                        _ => m,
                    })?;
                    let token = v["token"].as_str().ok_or("Last.fm sent something unexpected")?.to_string();
                    {
                        let mut r = report.lock().unwrap();
                        r.open = Some(format!("https://www.last.fm/api/auth/?api_key={}&token={}", crate::youtube::enc(&key), crate::youtube::enc(&token)));
                        r.status = Some((true, "Allow Starlite in your browser, then come back".to_string()));
                    }
                    // Ask every couple of seconds whether you've allowed it (two minutes at most).
                    let started = Instant::now();
                    loop {
                        match rx.recv_timeout(Duration::from_millis(2500)) {
                            Ok(Msg::Cancel) => return Err("Cancelled".to_string()),
                            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                            Err(_) => return Err(String::new()),
                        }
                        match call("auth.getSession", vec![("api_key", key.clone()), ("token", token.clone())], &secret) {
                            Ok(v) => {
                                let s = &v["session"];
                                return match (s["key"].as_str(), s["name"].as_str()) {
                                    (Some(k), Some(n)) => Ok((k.to_string(), n.to_string())),
                                    _ => Err("Last.fm sent something unexpected".to_string()),
                                };
                            }
                            // 14: not allowed yet. Keep waiting.
                            Err((14, _)) if started.elapsed() < Duration::from_secs(120) => {}
                            Err((14, _)) => return Err("Timed out waiting for you to allow Starlite".to_string()),
                            Err((13, _)) => return Err("That shared secret doesn't match the API key".to_string()),
                            Err((_, m)) => return Err(m),
                        }
                    }
                })();
                let mut r = report.lock().unwrap();
                r.connecting = false;
                match done {
                    Ok((k, n)) => {
                        r.status = Some((true, format!("Connected as {n}")));
                        r.session = Some((k, n));
                    }
                    Err(e) => r.status = Some((false, e)),
                }
            }
            Msg::NowPlaying(prefs, play) => {
                if let Err((code, m)) = call("track.updateNowPlaying", play_params(&play, &prefs), &prefs.secret) {
                    // 9: the session was revoked on last.fm.
                    if code == 9 {
                        say(false, "Last.fm signed you out. Connect again.".to_string());
                    } else if code != 0 {
                        say(false, format!("Last.fm: {m}"));
                    }
                }
            }
            Msg::Scrobble(prefs, play) => {
                queue.push(play);
                // Oldest first; stop at the first one that can't go out now.
                while let Some(p) = queue.first().cloned() {
                    let mut params = play_params(&p, &prefs);
                    params.push(("timestamp", p.at.to_string()));
                    match call("track.scrobble", params, &prefs.secret) {
                        Ok(_) => {
                            queue.remove(0);
                        }
                        // Offline or Last.fm is down (11, 16): try again with the next song.
                        Err((0 | 11 | 16, _)) => break,
                        Err((9, _)) => {
                            say(false, "Last.fm signed you out. Connect again.".to_string());
                            break;
                        }
                        // Anything else is about this one scrobble: drop it.
                        Err((_, m)) => {
                            say(false, format!("Last.fm: {m}"));
                            queue.remove(0);
                        }
                    }
                }
                // Keep at most the last 500 waiting.
                let over = queue.len().saturating_sub(500);
                queue.drain(..over);
                if queue.is_empty() {
                    let _ = std::fs::remove_file(queue_file());
                } else if let Ok(s) = serde_json::to_string(&queue) {
                    let _ = std::fs::write(queue_file(), s);
                }
            }
        }
    }
}

impl LastFm {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let report = Arc::new(Mutex::new(Report::default()));
        let r = report.clone();
        std::thread::Builder::new().name("lastfm".into()).spawn(move || worker(rx, r)).expect("spawn lastfm thread");
        let mut me = Self { tx, prefs: LfmPrefs::load(), report, current: None, last_frame: Instant::now() };
        // Debug: STARLITE_LASTFM_CONNECT=key|secret starts connecting at once.
        if let Some((k, s)) = std::env::var("STARLITE_LASTFM_CONNECT").ok().as_deref().and_then(|v| v.split_once('|')) {
            me.connect(k, s);
        }
        me
    }

    fn active(&self) -> bool {
        self.prefs.enabled && self.prefs.connected()
    }

    /// Every frame: follows what's playing and sends what's due.
    /// `serial` goes up with every play (also of the same song again).
    pub fn update(&mut self, track: Option<&Track>, serial: u64, playing: bool) {
        let dt = self.last_frame.elapsed().as_secs_f64().min(10.0);
        self.last_frame = Instant::now();
        // A finished sign-in.
        let session = self.report.lock().unwrap().session.take();
        if let Some((key, user)) = session {
            self.prefs.session = key;
            self.prefs.user = user;
            self.prefs.enabled = true;
            self.prefs.save();
        }
        let Some(t) = track else {
            self.current = None;
            return;
        };
        let key = (t.id.clone(), serial);
        if self.current.as_ref().map(|c| &c.0) != Some(&key) {
            let play = Play::of(t, unix_now());
            self.current = Some((key, play.clone(), 0.0, false));
            if self.active() {
                let _ = self.tx.send(Msg::NowPlaying(self.prefs.clone(), play));
            }
        }
        let active = self.active();
        let Some((_, play, heard, done)) = self.current.as_mut() else { return };
        if playing {
            *heard += dt;
        }
        if !*done && active && scrobble_after(play.secs).is_some_and(|need| *heard >= need) {
            *done = true;
            let _ = self.tx.send(Msg::Scrobble(self.prefs.clone(), play.clone()));
        }
    }

    pub fn connect(&mut self, key: &str, secret: &str) {
        self.prefs.api_key = key.trim().to_string();
        self.prefs.secret = secret.trim().to_string();
        self.prefs.save();
        let _ = self.tx.send(Msg::Connect(self.prefs.api_key.clone(), self.prefs.secret.clone()));
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(Msg::Cancel);
    }

    /// Forgets the session (the key and secret stay for next time).
    pub fn disconnect(&mut self) {
        self.prefs.session.clear();
        self.prefs.user.clear();
        self.prefs.save();
        self.report.lock().unwrap().status = None;
    }
}

impl Starlite {
    /// The Last.fm tab: the switch, your API key and secret, connect.
    #[allow(clippy::too_many_arguments)]
    pub fn lastfm_settings(&mut self, ui: &mut Ui, icons: &mut Icons, els: &mut Vec<El>, ppp: f32, x0: f32, top: f32, w: f32) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        if !self.lfm_ui.loaded {
            self.lfm_ui = LfmUi { key: self.lastfm.prefs.api_key.clone(), secret: self.lastfm.prefs.secret.clone(), loaded: true };
        }
        // A page the worker wants opened (the "allow Starlite" page).
        let (status, connecting, open) = {
            let mut r = self.lastfm.report.lock().unwrap();
            (r.status.clone(), r.connecting, r.open.take())
        };
        // (Not when talking to a test server.)
        if let Some(url) = open.filter(|_| std::env::var("STARLITE_LASTFM_API").is_err()) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if connecting {
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
        }
        let connected = self.lastfm.prefs.connected();

        let mut on = self.lastfm.prefs.enabled;
        let sub = if connected { format!("Connected as {}", self.lastfm.prefs.user) } else { "Sends the songs you play to your Last.fm profile".to_string() };
        if toggle_row(ui, Rect::from_min_size(pos2(x0, top), vec2(w.min(520.0), 52.0)), Id::new("lfm_on"), "Scrobble to Last.fm", Some(&sub), &mut on, theme) {
            self.sound.click();
            self.lastfm.prefs.enabled = on;
            self.lastfm.prefs.save();
        }

        let mut y = top + 70.0;
        if connected {
            let r = Rect::from_min_size(pos2(x0, y), vec2(126.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("lfm_disconnect"), r, "Disconnect", None, false, false, theme).clicked() {
                self.sound.click();
                self.lastfm.disconnect();
            }
            let r2 = Rect::from_min_size(pos2(r.right() + 10.0, y), vec2(156.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("lfm_profile"), r2, "Your profile", Some("external-link"), false, false, theme).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(format!("https://www.last.fm/user/{}", crate::youtube::enc(&self.lastfm.prefs.user))));
            }
            y += 54.0;
            let note = "A song is scrobbled once you've heard half of it or four minutes. Songs under 30 seconds don't count.";
            let g = p.layout(note.to_string(), font(12.5, "regular"), theme.text_dim(), w);
            p.galley(pos2(x0, y), g, theme.text_dim());
        } else {
            p.text(pos2(x0, y), Align2::LEFT_TOP, "API key", font(13.0, "semibold"), theme.text_dim());
            field(ui, els, ppp, Rect::from_min_size(pos2(x0, y + 22.0), vec2(w.min(360.0), 38.0)), &mut self.lfm_ui.key, "32 letters and numbers", false, false, theme);
            y += 74.0;
            p.text(pos2(x0, y), Align2::LEFT_TOP, "Shared secret", font(13.0, "semibold"), theme.text_dim());
            field(ui, els, ppp, Rect::from_min_size(pos2(x0, y + 22.0), vec2(w.min(360.0), 38.0)), &mut self.lfm_ui.secret, "Comes with the API key", true, false, theme);
            y += 78.0;
            let r = Rect::from_min_size(pos2(x0, y), vec2(if connecting { 196.0 } else { 122.0 }, 38.0));
            if pill(ui, icons, els, ppp, Id::new("lfm_connect"), r, if connecting { "Waiting for browser\u{2026}" } else { "Connect" }, None, true, false, theme).clicked() && !connecting {
                self.sound.click();
                let (k, s) = (self.lfm_ui.key.trim().to_string(), self.lfm_ui.secret.trim().to_string());
                if k.is_empty() || s.is_empty() {
                    self.lastfm.report.lock().unwrap().status = Some((false, "Fill in the API key and the shared secret".to_string()));
                } else {
                    self.lastfm.connect(&k, &s);
                }
            }
            let r2 = if connecting {
                let c = Rect::from_min_size(pos2(r.right() + 10.0, y), vec2(96.0, 38.0));
                if pill(ui, icons, els, ppp, Id::new("lfm_cancel"), c, "Cancel", None, false, false, theme).clicked() {
                    self.lastfm.cancel();
                }
                c
            } else {
                r
            };
            let r3 = Rect::from_min_size(pos2(r2.right() + 10.0, y), vec2(170.0, 38.0));
            if pill(ui, icons, els, ppp, Id::new("lfm_getkey"), r3, "Get an API key", Some("external-link"), false, false, theme).clicked() {
                ctx.open_url(egui::OpenUrl::new_tab("https://www.last.fm/api/account/create"));
            }
            y += 52.0;
            if let Some((ok, msg)) = &status {
                let c = if *ok { Color32::from_rgb(70, 190, 120) } else { Color32::from_rgb(235, 90, 100) };
                text_trunc(&p, pos2(x0, y + 4.0), Align2::LEFT_CENTER, msg, font(12.5, "semibold"), c, w);
                y += 24.0;
            }
            let help = "Create an API account on Last.fm (any name will do, the other boxes can stay empty), then paste its API key and shared secret here and press Connect. Your browser opens so you can allow Starlite. Your Last.fm password never comes near Starlite.";
            let g = p.layout(help.to_string(), font(12.5, "regular"), theme.text_dim(), w);
            p.galley(pos2(x0, y + 2.0), g, theme.text_dim());
        }
        if connected {
            if let Some((false, msg)) = &status {
                text_trunc(&p, pos2(x0, y + 46.0), Align2::LEFT_CENTER, msg, font(12.5, "semibold"), Color32::from_rgb(235, 90, 100), w);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_the_sorted_params_and_secret() {
        // Last.fm's documented recipe: api_keyxxxmethodauth.getSessiontokenyyy + secret.
        let s = sign(&[("token", "yyy".into()), ("method", "auth.getSession".into()), ("api_key", "xxx".into())], "ilovecher");
        assert_eq!(s, crate::navidrome::md5_hex("api_keyxxxmethodauth.getSessiontokenyyyilovecher"));
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn half_the_song_or_four_minutes() {
        assert_eq!(scrobble_after(200), Some(100.0));
        assert_eq!(scrobble_after(900), Some(240.0));
        assert_eq!(scrobble_after(25), None);
        assert_eq!(scrobble_after(0), Some(240.0));
    }

    #[test]
    fn a_play_carries_what_lastfm_wants() {
        let prefs = LfmPrefs { api_key: "k".into(), session: "s".into(), ..Default::default() };
        let p = Play { artist: "Daft Punk".into(), title: "Get Lucky".into(), album: Some("RAM".into()), secs: 248, at: 1 };
        let v = play_params(&p, &prefs);
        assert!(v.contains(&("artist", "Daft Punk".to_string())) && v.contains(&("track", "Get Lucky".to_string())) && v.contains(&("duration", "248".to_string())));
        let bare = play_params(&Play { album: None, secs: 0, ..p }, &prefs);
        assert!(!bare.iter().any(|(k, _)| *k == "album" || *k == "duration"));
    }
}
