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
        // YouTube videos are named for their channel and video; Last.fm wants artist and song.
        let (artist, title) = crate::lyrics::clean_names(t).unwrap_or_else(|| (t.artist.clone(), t.title.clone()));
        let album = t.album.clone().filter(|a| !a.trim().is_empty() && t.youtube_id().is_none());
        Play { artist, title, album, secs: t.duration_ms / 1000, at }
    }

    fn label(&self) -> String {
        format!("{} by {}", self.title, self.artist)
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

/// The play being listened to.
struct Current {
    /// (song id, play serial)
    key: (String, u64),
    play: Play,
    /// Seconds of it heard so far.
    heard: f64,
    last_pos: f64,
    /// "Now playing" was sent for it.
    announced: bool,
    scrobbled: bool,
}

impl Current {
    /// At the very end of the song there's nothing left to announce.
    fn scrobbled_out(&self, dur: f64, pos: f64) -> bool {
        dur > 0.0 && pos >= dur - 1.0
    }
}

pub struct LastFm {
    tx: Sender<Msg>,
    pub prefs: LfmPrefs,
    pub report: Arc<Mutex<Report>>,
    current: Option<Current>,
    last_frame: Instant,
    /// When playback was last seen running (a long pause announces the song again).
    last_playing: Instant,
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

/// Why Last.fm ignored a song (its `ignoredMessage`), if it did.
fn ignored(m: &Value) -> Option<&'static str> {
    let code = m["code"].as_str().map(str::to_string).or_else(|| m["code"].as_i64().map(|c| c.to_string()))?;
    match code.as_str() {
        "" | "0" => None,
        "1" => Some("it doesn't accept that artist name"),
        "2" => Some("it doesn't accept that song name"),
        "3" => Some("the time of the play is too far back"),
        "4" => Some("this PC's clock is ahead"),
        "5" => Some("today's scrobble limit is reached"),
        _ => Some("it gave no reason"),
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
            Msg::NowPlaying(prefs, play) => match call("track.updateNowPlaying", play_params(&play, &prefs), &prefs.secret) {
                Ok(v) => match ignored(&v["nowplaying"]["ignoredMessage"]) {
                    Some(why) => say(false, format!("Last.fm won't show \u{201c}{}\u{201d}: {why}", play.title)),
                    None => say(true, format!("Now playing sent: {}", play.label())),
                },
                // 9: the session was revoked on last.fm.
                Err((9, _)) => say(false, "Last.fm signed you out. Connect again.".to_string()),
                Err((0, m)) => say(false, m),
                Err((_, m)) => say(false, format!("Last.fm: {m}")),
            },
            Msg::Scrobble(prefs, play) => {
                queue.push(play);
                // Oldest first; stop at the first one that can't go out now.
                while let Some(p) = queue.first().cloned() {
                    let mut params = play_params(&p, &prefs);
                    params.push(("timestamp", p.at.to_string()));
                    match call("track.scrobble", params, &prefs.secret) {
                        Ok(v) => {
                            queue.remove(0);
                            let sc = &v["scrobbles"]["scrobble"];
                            // (One scrobble comes back as an object, several as a list.)
                            match ignored(&sc["ignoredMessage"]).or_else(|| ignored(&sc[0]["ignoredMessage"])) {
                                Some(why) => say(false, format!("Last.fm didn't take \u{201c}{}\u{201d}: {why}", p.title)),
                                None => say(true, format!("Scrobbled: {}", p.label())),
                            }
                        }
                        // Offline or Last.fm is down (11, 16): try again with the next song.
                        Err((0 | 11 | 16, m)) => {
                            say(false, format!("{m}. {} waiting to be sent.", if queue.len() == 1 { "1 scrobble is".to_string() } else { format!("{} scrobbles are", queue.len()) }));
                            break;
                        }
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
        let mut me = Self { tx, prefs: LfmPrefs::load(), report, current: None, last_frame: Instant::now(), last_playing: Instant::now() };
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
    /// `serial` goes up with every play (also of the same song again);
    /// `pos` / `dur`: where playback is and the song's length (seconds).
    pub fn update(&mut self, track: Option<&Track>, serial: u64, playing: bool, pos: f64, dur: f64) {
        let wall = self.last_frame.elapsed().as_secs_f64();
        self.last_frame = Instant::now();
        // A finished sign-in.
        let session = self.report.lock().unwrap().session.take();
        if let Some((key, user)) = session {
            self.prefs.session = key;
            self.prefs.user = user;
            self.prefs.enabled = true;
            self.prefs.save();
        }
        let active = self.active();
        let Some(t) = track else {
            self.current = None;
            return;
        };
        let key = (t.id.clone(), serial);
        if self.current.as_ref().map(|c| &c.key) != Some(&key) {
            self.current = Some(Current { key, play: Play::of(t, unix_now()), heard: 0.0, last_pos: pos, announced: false, scrobbled: false });
        }
        let paused_for = self.last_playing.elapsed();
        if playing {
            self.last_playing = Instant::now();
        }
        let Some(c) = self.current.as_mut() else { return };
        // Songs whose length the list didn't know (YouTube): the player does.
        if c.play.secs == 0 && dur >= 1.0 {
            c.play.secs = dur.round() as u64;
        }
        // Count what was really heard: playback moving forward at about the
        // clock's pace (up to 2x speed). A jump on the seek bar isn't listening.
        let moved = pos - c.last_pos;
        if moved > 0.0 && moved <= wall * 2.2 + 1.5 {
            c.heard += moved;
        }
        c.last_pos = pos;
        if !active {
            // Switched on (or connected) mid-song: it's announced then.
            c.announced = false;
            return;
        }
        // "Now playing": when a song starts, when Last.fm gets connected or
        // switched on during one, and again after a pause of over a minute.
        if playing && (!c.announced || paused_for > Duration::from_secs(60)) && !c.scrobbled_out(dur, pos) {
            c.announced = true;
            let _ = self.tx.send(Msg::NowPlaying(self.prefs.clone(), c.play.clone()));
        }
        if !c.scrobbled && scrobble_after(c.play.secs).is_some_and(|need| c.heard >= need) {
            c.scrobbled = true;
            let _ = self.tx.send(Msg::Scrobble(self.prefs.clone(), c.play.clone()));
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
            // What was last sent, or what went wrong.
            let line = match &status {
                Some((ok, msg)) if !msg.starts_with("Connected as") => Some((*ok, msg.clone())),
                _ if !self.lastfm.prefs.enabled => Some((true, "Scrobbling is switched off".to_string())),
                _ => Some((true, "Nothing sent yet: play a song".to_string())),
            };
            if let Some((ok, msg)) = line {
                let c = if ok { theme.text() } else { Color32::from_rgb(235, 90, 100) };
                p.text(pos2(x0, y + 50.0), Align2::LEFT_CENTER, "Latest", font(12.0, "semibold"), theme.text_dim());
                text_trunc(&p, pos2(x0 + 52.0, y + 50.0), Align2::LEFT_CENTER, &msg, font(12.5, "semibold"), c, w - 52.0);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(700));
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
    fn youtube_videos_are_sent_as_artist_and_song() {
        let yt = Track { id: "youtube:a".into(), title: "Kanye West - All Falls Down (Official Video) [HD]".into(), artist: "KanyeWestVEVO".into(), album: Some("x".into()), artwork: None, permalink: None, duration_ms: 0, source: crate::library::Source::YouTube { video_id: "a".into() } };
        let p = Play::of(&yt, 5);
        assert_eq!((p.artist.as_str(), p.title.as_str(), p.album.clone(), p.secs), ("Kanye West", "All Falls Down", None, 0));
        let file = Track { title: "505".into(), artist: "Arctic Monkeys".into(), album: Some("Favourite Worst Nightmare".into()), duration_ms: 253_000, source: crate::library::Source::File { path: "a.mp3".into() }, ..yt };
        let p = Play::of(&file, 5);
        assert_eq!((p.artist.as_str(), p.title.as_str(), p.album.as_deref(), p.secs), ("Arctic Monkeys", "505", Some("Favourite Worst Nightmare"), 253));
        assert_eq!(p.label(), "505 by Arctic Monkeys");
    }

    #[test]
    fn listening_counts_but_seeking_doesnt() {
        let (tx, rx) = channel();
        let mut l = LastFm { tx, prefs: LfmPrefs { enabled: true, session: "s".into(), ..Default::default() }, report: Default::default(), current: None, last_frame: Instant::now(), last_playing: Instant::now() };
        let t = Track { id: "f".into(), title: "Song".into(), artist: "Artist".into(), album: None, artwork: None, permalink: None, duration_ms: 0, source: crate::library::Source::File { path: "a.mp3".into() } };
        // The song starts: announced at once, with the length the player knows.
        l.update(Some(&t), 1, true, 0.0, 200.0);
        assert!(matches!(rx.try_recv(), Ok(Msg::NowPlaying(_, p)) if p.secs == 200));
        // Jumping to 3:00 on the seek bar isn't three minutes of listening.
        l.update(Some(&t), 1, true, 180.0, 200.0);
        assert!(rx.try_recv().is_err());
        assert!(l.current.as_ref().unwrap().heard < 1.0);
        // Real listening adds up to the scrobble (half of 200 s).
        l.current.as_mut().unwrap().heard = 99.5;
        l.last_frame = Instant::now() - Duration::from_secs(1);
        l.update(Some(&t), 1, true, 181.0, 200.0);
        assert!(matches!(rx.try_recv(), Ok(Msg::Scrobble(_, p)) if p.title == "Song"));
        // Only once.
        l.update(Some(&t), 1, true, 181.5, 200.0);
        assert!(rx.try_recv().is_err());
        // The same song again is a new play.
        l.update(Some(&t), 2, true, 0.0, 200.0);
        assert!(matches!(rx.try_recv(), Ok(Msg::NowPlaying(..))));
    }

    #[test]
    fn connecting_mid_song_announces_it() {
        let (tx, rx) = channel();
        let mut l = LastFm { tx, prefs: LfmPrefs::default(), report: Default::default(), current: None, last_frame: Instant::now(), last_playing: Instant::now() };
        let t = Track { id: "f".into(), title: "Song".into(), artist: "Artist".into(), album: None, artwork: None, permalink: None, duration_ms: 200_000, source: crate::library::Source::File { path: "a.mp3".into() } };
        l.update(Some(&t), 1, true, 10.0, 200.0);
        assert!(rx.try_recv().is_err());
        // You allow Starlite on last.fm while the song plays on.
        l.report.lock().unwrap().session = Some(("key".into(), "me".into()));
        l.update(Some(&t), 1, true, 10.5, 200.0);
        assert!(matches!(rx.try_recv(), Ok(Msg::NowPlaying(_, p)) if p.title == "Song"));
        assert!(l.prefs.enabled && l.prefs.connected());
    }

    #[test]
    fn last_fms_reasons_are_read() {
        let v: Value = serde_json::from_str(r##"{"ok":{"code":"0","#text":""},"artist":{"code":"1","#text":"Artist name failed filter"},"num":{"code":3}}"##).unwrap();
        assert_eq!(ignored(&v["ok"]), None);
        assert_eq!(ignored(&v["artist"]), Some("it doesn't accept that artist name"));
        assert_eq!(ignored(&v["num"]), Some("the time of the play is too far back"));
        assert_eq!(ignored(&v["missing"]), None);
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
